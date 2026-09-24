//! 三端平台入口与差异点。
//!
//! - `native`：桌面（Windows/Linux/macOS）+ 通用 winit `run`。
//! - `android`：`android-activity` `android_main` 接入（见 `docs/android.md`）。
//! - `wasm`：`spawn_app` + `requestAnimationFrame`（`?wgl` 强制 WebGL2）。

use std::cell::RefCell;
use std::rc::Rc;

use sisf_render::RendererError;
use winit::window::Window;

use crate::app::App;
use crate::config::AppConfig;
use crate::runner::RunState;

#[cfg(not(target_arch = "wasm32"))]
pub mod native;

#[cfg(target_os = "android")]
pub mod android;

#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod wasm;

/// 初始化失败要能被看到：桌面进 stderr，wasm 进 devtools console。
pub(crate) fn log_error(msg: &str) {
    #[cfg(all(target_arch = "wasm32", feature = "web"))]
    web_sys::console::error_1(&msg.into());
    #[cfg(not(all(target_arch = "wasm32", feature = "web")))]
    eprintln!("{msg}");
}

/// 常规信息（选中了哪个 adapter 之类）：桌面走 stdout，wasm 走 console。
/// 也是示例/上层在 wasm 下唯一的可见输出通道（`println!` 在该 target 是静默丢弃）。
pub fn log_info(msg: &str) {
    #[cfg(all(target_arch = "wasm32", feature = "web"))]
    web_sys::console::log_1(&msg.into());
    #[cfg(not(all(target_arch = "wasm32", feature = "web")))]
    println!("{msg}");
}

/// 平台差异：窗口属性微调（wasm 复用页面 canvas；其他端原样）。
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn window_attrs(
    attrs: winit::window::WindowAttributes,
) -> winit::window::WindowAttributes {
    attrs
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn window_attrs(
    attrs: winit::window::WindowAttributes,
) -> winit::window::WindowAttributes {
    wasm::window_attrs(attrs)
}

/// 平台差异：同步创建 instance + surface（窗口借用只存在于本函数内）。
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn create_instance_and_surface(
    window: &Window,
) -> Result<(wgpu::Instance, wgpu::Surface<'static>), RendererError> {
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = match std::env::var("SISF_BACKEND").as_deref() {
        Ok("gl") => wgpu::Backends::GL,
        _ => wgpu::Backends::all(),
    };
    let instance = wgpu::Instance::new(desc);
    let window_handle = window
        .window_handle()
        .map_err(|e| RendererError::Handle(e.to_string()))?
        .as_raw();
    let display_handle = window
        .display_handle()
        .map_err(|e| RendererError::Handle(e.to_string()))?
        .as_raw();
    // SAFETY: `RunState` 字段序保证 renderer（持有 surface）先于 window 释放；
    // 挂起路径（Android）也先销毁 renderer 再走 winit 析构。
    let surface = unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(display_handle),
            raw_window_handle: window_handle,
        })
    }?;
    Ok((instance, surface))
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn create_instance_and_surface(
    window: &Window,
) -> Result<(wgpu::Instance, wgpu::Surface<'static>), RendererError> {
    use winit::platform::web::WindowExtWebSys;
    let canvas = window
        .canvas()
        .ok_or_else(|| RendererError::Handle("wasm: window has no canvas".to_owned()))?;
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wasm::requested_backends();
    let instance = wgpu::Instance::new(desc);
    let surface = instance.create_surface(wgpu::SurfaceTarget::Canvas(canvas))?;
    Ok((instance, surface))
}

/// renderer 异步初始化（native `block_on`，wasm `spawn_local`，同一个 future）。
pub(crate) async fn init_renderer<A: App + 'static>(
    state: Rc<RefCell<RunState<A>>>,
    created: Result<(wgpu::Instance, wgpu::Surface<'static>), RendererError>,
) {
    let (instance, surface) = match created {
        Ok(v) => v,
        Err(e) => {
            log_error(&format!("[sisf-app] surface 创建失败: {e}"));
            state.borrow_mut().init_error = Some(e.to_string());
            return;
        }
    };
    match sisf_render::Renderer::new(instance, surface).await {
        Ok(mut renderer) => {
            let info = renderer.device.adapter_info();
            log_info(&format!(
                "[sisf-app] adapter: {} ({:?}, {:?})",
                info.name, info.device_type, info.backend
            ));
            let (w, h) = {
                let s = state.borrow();
                s.frame.physical_size
            };
            renderer.configure(w, h);
            state.borrow_mut().renderer = Some(renderer);
        }
        Err(e) => {
            log_error(&format!("[sisf-app] renderer 初始化失败: {e}"));
            state.borrow_mut().init_error = Some(e.to_string());
        }
    }
}

/// 平台差异：单调时钟（秒，进程起点附近无意义，只做差值）。
///
/// 循环计时必须走这里而不是 `std::time::Instant`：wasm32-unknown-unknown 的 std
/// 没有时钟，`Instant::now()` 会直接 panic（`sys/time/unsupported.rs`），
/// 浏览器侧改用 `performance.now()`。
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn now_secs() -> f64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static ZERO: OnceLock<Instant> = OnceLock::new();
    ZERO.get_or_init(Instant::now).elapsed().as_secs_f64()
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn now_secs() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now() / 1000.0)
        .unwrap_or_default()
}

/// 平台差异：页面是否被隐藏（wasm visibilitychange；其他端恒为 false）。
pub(crate) fn page_hidden() -> bool {
    #[cfg(all(target_arch = "wasm32", feature = "web"))]
    {
        wasm::page_hidden()
    }
    #[cfg(not(all(target_arch = "wasm32", feature = "web")))]
    {
        false
    }
}

pub(crate) fn start_renderer_init<A: App + 'static>(state: Rc<RefCell<RunState<A>>>) {
    let created = {
        let s = state.borrow();
        match s.window.as_ref() {
            Some(window) => create_instance_and_surface(window),
            None => Err(RendererError::NoWindow),
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    native::block_on(init_renderer(state, created));
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(init_renderer(state, created));
}

/// 统一 `run`：桌面/android 走 `native::run`，wasm 走 `wasm::run`。
pub fn run<A: App + 'static>(app: A, config: AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        native::run(app, config)
    }
    #[cfg(target_arch = "wasm32")]
    {
        wasm::run(app, config)
    }
}
