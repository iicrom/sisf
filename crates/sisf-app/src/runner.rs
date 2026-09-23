//! winit 0.30 `ApplicationHandler` 共享实现：native / wasm / android 三端同一套状态机。
//!
//! 生命周期约定：
//! - `Resumed`：建窗口（一次）、建/重建 renderer（surface）、上报 `Lifecycle::Resumed`。
//! - 挂起来源三分：winit `suspended()`（Android/退出前）、窗口遮挡（Win 最小化 0x0）、
//!   wasm `visibilitychange`。任一生效即上报 `Suspended` 并冻结 tick；Android 额外释放
//!   `Surface` 与 GPU 资源，`Resumed` 时重建。
//! - `Resized`/`ScaleFactorChanged` → 重算 `FrameCtx` 投影与输入坐标映射，并请求重配交换链。

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use sisf_event::{EngineEvent, Lifecycle};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes, WindowId};

use crate::app::App;
use crate::config::AppConfig;
use crate::frame::FrameCtx;
use crate::platform;

pub(crate) struct RunState<A: App> {
    pub app: A,
    pub config: AppConfig,
    /// 注意：`renderer`（内含 `Surface`）必须声明在 `window` 之前——
    /// Rust 按声明序析构，而 platform 层的 unsafe surface 要求窗口比 surface 活得久。
    pub renderer: Option<sisf_render::Renderer>,
    pub window: Option<Window>,
    pub frame: FrameCtx,
    pub events: Vec<EngineEvent>,
    /// 上一帧的 `platform::now_secs()`；`None` = 挂起后刚恢复，不吃时间。
    pub last: Option<f64>,
    /// 累计运行时间（秒，不含挂起期间）→ `FrameU::time`。
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
        Self {
            state: Rc::new(RefCell::new(RunState {
                app,
                config,
                renderer: None,
                window: None,
                frame,
                events: Vec::new(),
                last: None,
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

    /// 挂起状态迁移 + 事件派发 + renderer 惰性重建。三处入口共用。
    fn sync_suspend(&mut self) {
        let state = self.state.clone();
        // winit 0.30 只有在 `Window::request_redraw()` 之后才发 `RedrawRequested`，
        // 所以这里负责在"恢复/新建 renderer"时踢起重绘链。
        let mut kick = false;
        {
            let mut s = state.borrow_mut();
            let now = Self::suspended_now(&s);
            if now != s.reported_suspended {
                s.reported_suspended = now;
                let lifecycle = if now {
                    Lifecycle::Suspended
                } else {
                    Lifecycle::Resumed
                };
                s.events.push(EngineEvent::Lifecycle(lifecycle));
                s.last = None; // 不吃挂起期间的时间（T8 冻结累加器同理）
                if now && cfg!(target_os = "android") {
                    s.renderer = None;
                    s.init_error = None;
                }
                kick = !now;
            }
        }
        self.drain_events();
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

    fn drain_events(&mut self) {
        let state = self.state.clone();
        let mut borrow = state.borrow_mut();
        // 单次 deref 后再分字段借用（RefMut 的两次隐式 deref 会被判为重复可变借用）。
        let s: &mut RunState<A> = &mut borrow;
        if s.events.is_empty() {
            return;
        }
        let events = std::mem::take(&mut s.events);
        s.app.on_event(&events, &mut s.frame);
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
        let now = platform::now_secs();
        let dt = s
            .last
            .map(|prev| Duration::from_secs_f64((now - prev).max(0.0)))
            .unwrap_or(Duration::ZERO);
        s.last = Some(now);
        s.time += dt.as_secs_f32();
        s.app.tick(dt, &mut s.frame);
        let frame_u = s.frame.frame_u(s.time);
        let clear = s.config.clear;
        renderer.begin_frame(&frame_u, clear);
        s.app.draw(1.0, &mut renderer);
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
                        let frame = {
                            let cfg = &s.config;
                            FrameCtx::new(cfg, (size.width, size.height), sf)
                        };
                        s.frame = frame;
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
        match event {
            WindowEvent::Resized(size) => {
                let state = self.state.clone();
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
                let state = self.state.clone();
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
                let state = self.state.clone();
                let mut s = state.borrow_mut();
                s.frame.focused = focused;
                s.events
                    .push(EngineEvent::Lifecycle(Lifecycle::Focused(focused)));
            }
            WindowEvent::CloseRequested => {
                let state = self.state.clone();
                state
                    .borrow_mut()
                    .events
                    .push(EngineEvent::Lifecycle(Lifecycle::CloseRequested));
                self.drain_events();
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
