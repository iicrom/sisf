//! winit 0.30 `ApplicationHandler` 共享实现：native / wasm / android 三端同一套状态机。
//!
//! 生命周期约定：
//! - `Resumed`：建窗口（一次）、建/重建 renderer（surface）、上报 `Lifecycle::Resumed`。
//! - 挂起来源三分：winit `suspended()`（Android/退出前）、窗口遮挡（Win 最小化 0x0）、
//!   wasm `visibilitychange`。任一生效即上报 `Suspended` 并冻结 tick；Android 额外释放
//!   `Surface` 与 GPU 资源，`Resumed` 时重建。
//! - `Resized`/`ScaleFactorChanged` → 重算 `FrameCtx` 投影与输入坐标映射，并请求重配交换链。
//!
//! ## 一帧里输入的位置（T9）
//!
//! 设备事件（键盘 / 指针 / 手柄）在**到达的那一刻**就翻译并喂给
//! [`InputState`](sisf_event::InputState)，随后立刻派发给 `App::on_event` —— 不等下一次
//! `RedrawRequested`，否则按住方向键的手感会随渲染帧率抖。翻译只做一次，规则在 `normalize`。
//!
//! `RedrawRequested` 之后：
//! 1. 轮询手柄与 `custom` 通道（宿主 / JS 投进来的消息）；
//! 2. 派发到目前为止攒下的事件；
//! 3. `TickClock::step(真实 dt)` 决定这一帧补几拍，**每拍**先 `input.tick(bus)`（held 边沿与
//!    连发只数拍子不数帧），再派发，再 `App::tick(周期)`；
//! 4. `App::prepare(alpha)`（投影烘焙前的几何对齐）→ `App::draw(alpha)`。
//!
//! 失焦/挂起时 [`forget_held`](sisf_event::InputState::forget_held) 放掉所有按住的动作：
//! 窗口最小化收不到 keyup，不放掉的话游戏回来就朝着一面墙磨。

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use sisf_event::{EngineBus, EngineEvent, Lifecycle};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes, WindowId};

use crate::app::App;
use crate::config::AppConfig;
use crate::custom;
use crate::frame::FrameCtx;
#[cfg(all(
    feature = "gilrs",
    not(target_arch = "wasm32"),
    not(target_os = "android")
))]
use crate::gamepad::Gamepads;
use crate::normalize;
use crate::platform;
use crate::timing::TickClock;

pub(crate) struct RunState<A: App> {
    pub app: A,
    pub config: AppConfig,
    /// 注意：`renderer`（内含 `Surface`）必须声明在 `window` 之前——
    /// Rust 按声明序析构，而 platform 层的 unsafe surface 要求窗口比 surface 活得久。
    pub renderer: Option<sisf_render::Renderer>,
    pub window: Option<Window>,
    pub frame: FrameCtx,
    /// 本批次待派发的事件（裸事件 + `InputState` 翻出来的动作边沿，顺序 = 发生顺序）。
    pub bus: EngineBus,
    /// 最近一次光标/触摸的**物理**位置：winit 的 `MouseInput`/`MouseWheel` 自身不带坐标。
    pub cursor: Option<(f64, f64)>,
    #[cfg(all(
        feature = "gilrs",
        not(target_arch = "wasm32"),
        not(target_os = "android")
    ))]
    pub pads: Option<Gamepads>,
    /// 上一帧的 `platform::now_secs()`；`None` = 挂起后刚恢复，不吃时间。
    pub last: Option<f64>,
    /// 固定步长时钟：把上面那个真实 `dt` 切成 0..=`MAX_CATCHUP` 个 tick + 一个 `alpha`。
    pub clock: TickClock,
    /// 累计运行时间（秒，不含挂起期间）→ `FrameU::time`。
    ///
    /// 这是**墙钟**，不是逻辑时间（逻辑看 `clock.ticks()`）：着色器里的连续量该跟着
    /// 画面走，卡顿补帧时不该跟着跳一格。
    pub time: f32,
    pub(crate) winit_suspended: bool,
    pub(crate) occluded: bool,
    pub(crate) reported_suspended: bool,
    pub(crate) pending_configure: Option<(u32, u32)>,
    pub(crate) init_error: Option<String>,
}

/// 三端共用的事件循环宿主。
pub struct Runner<A: App> {
    pub(crate) state: Rc<RefCell<RunState<A>>>,
}

impl<A: App> Runner<A> {
    pub fn new(app: A, config: AppConfig) -> Self {
        let frame = FrameCtx::new(&config, config.virtual_size, 1.0);
        let clock = TickClock::new(config.tick_hz);
        Self {
            state: Rc::new(RefCell::new(RunState {
                app,
                config,
                renderer: None,
                window: None,
                frame,
                bus: EngineBus::new(),
                cursor: None,
                #[cfg(all(
                    feature = "gilrs",
                    not(target_arch = "wasm32"),
                    not(target_os = "android")
                ))]
                // 手柄起不来就当没有手柄（`None`），不该让窗口开不了。
                pads: Gamepads::new(),
                last: None,
                clock,
                time: 0.0,
                winit_suspended: false,
                occluded: false,
                // 启动即视为"尚未上报过 Resumed"，首次 sync_suspend 会补发 Resumed。
                reported_suspended: true,
                pending_configure: None,
                init_error: None,
            })),
        }
    }
}

impl<A: App + 'static> Runner<A> {
    fn suspended_now(s: &RunState<A>) -> bool {
        s.winit_suspended || s.occluded || platform::page_hidden()
    }

    /// 一条**裸**设备事件的唯一去向：进 `InputState`（原样进总线 + 翻出动作边沿），
    /// 然后立刻派发。
    fn feed(s: &mut RunState<A>, event: EngineEvent) {
        s.frame.input.feed(&event, &mut s.bus);
        Self::deliver(s);
    }

    /// 引擎自己产生的事件（生命周期等）：不过 `InputState`，直接排队。
    fn push(s: &mut RunState<A>, event: EngineEvent) {
        s.bus.push(event);
    }

    /// 取空总线交给游戏侧。`post_custom` 的消息在这里并入 —— 派发点唯一，所以并入口也唯一。
    fn deliver(s: &mut RunState<A>) {
        custom::drain_global_into(&mut s.bus);
        let events = s.bus.drain();
        if events.is_empty() {
            return;
        }
        s.app.on_event(&events, &mut s.frame);
    }

    /// 挂起状态迁移 + 事件派发 + renderer 惰性重建。三处入口共用。
    fn sync_suspend(&mut self) {
        let state = self.state.clone();
        // winit 0.30 只有在 `Window::request_redraw()` 之后才发 `RedrawRequested`，
        // 所以这里负责在"恢复/新建 renderer"时踢起重绘链。
        let mut kick = false;
        {
            let mut borrow = state.borrow_mut();
            let s: &mut RunState<A> = &mut borrow;
            let now = Self::suspended_now(s);
            if now != s.reported_suspended {
                s.reported_suspended = now;
                let lifecycle = if now {
                    Lifecycle::Suspended
                } else {
                    Lifecycle::Resumed
                };
                Self::push(s, EngineEvent::Lifecycle(lifecycle));
                if now {
                    // 挂起之后再收不到 keyup，现在不放等于把"按着"这件事带过进程边界。
                    s.frame.input.forget_held(&mut s.bus);
                }
                // 不吃挂起期间的时间（T8）：`last` 归零 + 累加器冻结，回来第一帧 0 拍。
                s.last = None;
                s.clock.reset();
                if now && cfg!(target_os = "android") {
                    s.renderer = None;
                    s.init_error = None;
                }
                kick = !now;
            }
            Self::deliver(s);
        }
        let need_init = {
            let s = state.borrow();
            !s.reported_suspended
                && s.window.is_some()
                && s.renderer.is_none()
                && s.init_error.is_none()
        };
        if need_init {
            platform::start_renderer_init(state.clone());
            kick = true;
        }
        if kick {
            self.request_redraw();
        }
    }

    fn request_redraw(&self) {
        let s = self.state.borrow();
        if let Some(window) = s.window.as_ref() {
            window.request_redraw();
        }
    }

    fn do_frame(&mut self) {
        let state = self.state.clone();
        let mut borrow = state.borrow_mut();
        let s: &mut RunState<A> = &mut borrow;
        if s.reported_suspended || s.init_error.is_some() {
            return; // 挂起时主动断链；恢复由 sync_suspend 踢一帧
        }
        // renderer 取出-使用-放回，避免 app 与 renderer 同时可变借用 *s。
        let Some(mut renderer) = s.renderer.take() else {
            return;
        };
        if let Some((w, h)) = s.pending_configure.take() {
            renderer.configure(w, h);
        }
        // 手柄只在帧边界读：gilrs 的队列由 OS 回调喂，一帧读一次是唯一稳定的消费点。
        #[cfg(all(
            feature = "gilrs",
            not(target_arch = "wasm32"),
            not(target_os = "android")
        ))]
        if let Some(pads) = s.pads.as_mut() {
            pads.poll(&mut s.frame.input, &mut s.bus);
        }
        Self::deliver(s);
        let now = platform::now_secs();
        let dt = s
            .last
            .map(|prev| Duration::from_secs_f64((now - prev).max(0.0)))
            .unwrap_or(Duration::ZERO);
        s.last = Some(now);
        s.time += dt.as_secs_f32();
        // 固定步长：真实 `dt` 只决定"这一帧补几拍"（上限见 `TickClock`），
        // `App::tick` 因此永远只见到一个周期的 `dt`。
        let steps = s.clock.step(dt);
        let period = s.clock.period();
        for _ in 0..steps {
            // held 与连发按拍报 ⇒ 补帧时一帧内多几拍也就多几条 `Hold`，逻辑不随帧率漂。
            s.frame.input.tick(&mut s.bus);
            Self::deliver(s);
            s.app.tick(period, &mut s.frame);
        }
        let alpha = s.clock.alpha();
        // 投影在下一行烘焙进 `FrameU`，`draw` 里再改栈已经晚了 ⇒ 相机一类的几何必须先看这一相。
        s.app.prepare(alpha, &mut s.frame);
        let frame_u = s.frame.frame_u(s.time);
        let clear = s.config.clear;
        renderer.begin_frame(&frame_u, clear);
        s.app.draw(alpha, &mut renderer);
        // 空实例集合也走完 pass，等价于 T2 的纯清屏。
        renderer.end_frame();
        s.renderer = Some(renderer);
        // 链式续帧：本帧已提交，请求下一次 RedrawRequested。
        if let Some(window) = s.window.as_ref() {
            window.request_redraw();
        }
    }
}

impl<A: App + 'static> ApplicationHandler for Runner<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let state = self.state.clone();
        {
            let mut s = state.borrow_mut();
            if s.window.is_none() {
                let attrs = platform::window_attrs(
                    WindowAttributes::default().with_title(s.config.title.clone()),
                );
                match event_loop.create_window(attrs) {
                    Ok(window) => {
                        let size = window.inner_size();
                        let sf = window.scale_factor();
                        // 只在这里整体换 `FrameCtx`（此刻还没有任何按住状态）；Android 挂起
                        // 回来时窗口还在，不走这条路径，`input` 因此不会被打断。
                        let cfg = &s.config;
                        s.frame = FrameCtx::new(cfg, (size.width, size.height), sf);
                        s.window = Some(window);
                    }
                    Err(e) => {
                        s.init_error = Some(format!("创建窗口失败: {e}"));
                        return;
                    }
                }
            }
            s.winit_suspended = false;
        }
        self.sync_suspend();
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.state.borrow_mut().winit_suspended = true;
        self.sync_suspend();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let state = self.state.clone();
        match event {
            WindowEvent::Resized(size) => {
                let mut s = state.borrow_mut();
                if size.width == 0 || size.height == 0 {
                    s.occluded = true; // Win 最小化：停帧，等恢复
                    drop(s);
                    self.sync_suspend();
                    return;
                }
                s.occluded = false;
                let mode = s.config.scale_mode;
                s.frame.set_physical_size((size.width, size.height), mode);
                s.pending_configure = Some((size.width, size.height));
                drop(s);
                self.sync_suspend();
                self.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let mut s = state.borrow_mut();
                let size = s.frame.physical_size;
                let mode = s.config.scale_mode;
                s.frame.scale_factor = scale_factor;
                s.frame.set_physical_size(size, mode);
                s.pending_configure = Some(size);
                drop(s);
                self.request_redraw();
            }
            WindowEvent::Focused(focused) => {
                let mut borrow = state.borrow_mut();
                let s: &mut RunState<A> = &mut borrow;
                s.frame.focused = focused;
                Self::push(s, EngineEvent::Lifecycle(Lifecycle::Focused(focused)));
                if !focused {
                    // 失焦收不到剩下的 keyup：主动放掉，回来才不会朝一面墙磨。
                    s.frame.input.forget_held(&mut s.bus);
                }
                Self::deliver(s);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let mut borrow = state.borrow_mut();
                let s: &mut RunState<A> = &mut borrow;
                Self::feed(s, normalize::key_event(&event));
            }
            WindowEvent::CursorMoved { position, .. } => {
                let mut borrow = state.borrow_mut();
                let s: &mut RunState<A> = &mut borrow;
                let at = (position.x, position.y);
                let event = normalize::cursor_moved(&s.frame, at, s.cursor);
                s.cursor = Some(at);
                Self::feed(s, event);
            }
            WindowEvent::MouseInput {
                state: btn_state,
                button,
                ..
            } => {
                let mut borrow = state.borrow_mut();
                let s: &mut RunState<A> = &mut borrow;
                // 从没动过鼠标就点（触摸板/刚进窗口）时按左上角算，而不是丢掉这次点击。
                let cursor = s.cursor.unwrap_or((0.0, 0.0));
                let event = normalize::mouse_input(&s.frame, btn_state, &button, cursor);
                Self::feed(s, event);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let mut borrow = state.borrow_mut();
                let s: &mut RunState<A> = &mut borrow;
                let cursor = s.cursor.unwrap_or((0.0, 0.0));
                let event = normalize::mouse_wheel(&s.frame, &delta, cursor);
                Self::feed(s, event);
            }
            WindowEvent::Touch(touch) => {
                let mut borrow = state.borrow_mut();
                let s: &mut RunState<A> = &mut borrow;
                let at = (touch.location.x, touch.location.y);
                let event = normalize::touch(&s.frame, &touch.phase, at);
                s.cursor = Some(at);
                Self::feed(s, event);
            }
            WindowEvent::CloseRequested => {
                let mut borrow = state.borrow_mut();
                let s: &mut RunState<A> = &mut borrow;
                Self::push(s, EngineEvent::Lifecycle(Lifecycle::CloseRequested));
                Self::deliver(s);
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                self.sync_suspend(); // 轮询 wasm visibilitychange
                self.do_frame();
            }
            _ => {}
        }
    }
}
