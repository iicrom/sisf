//! wasm 入口：winit 0.30 无 `run()`，用 `spawn_app` + 浏览器任务调度
//! 驱动与桌面完全相同的 `Runner::frame()` 状态机。

use std::error::Error;

use wasm_bindgen::JsCast;
use web_sys::VisibilityState;
use winit::error::EventLoopError;
use winit::event_loop::{ControlFlow, EventLoop};
use winit::platform::web::{EventLoopExtWebSys, WindowAttributesExtWebSys};
use winit::window::WindowAttributes;

use crate::app::App;
use crate::config::AppConfig;
use crate::runner::Runner;

static PAGE_HIDDEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn page_hidden() -> bool {
    PAGE_HIDDEN.load(std::sync::atomic::Ordering::Relaxed)
}

/// URL 带 `?wgl` 时强制 WebGL2 回退，否则 WebGPU 优先、GL 兜底。
pub(crate) fn requested_backends() -> wgpu::Backends {
    let search = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .unwrap_or_default();
    if search.contains("wgl") {
        wgpu::Backends::GL
    } else {
        wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL
    }
}

fn install_visibility_watcher() {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let closure = wasm_bindgen::closure::Closure::wrap(Box::new(|| {
        let hidden = web_sys::window()
            .and_then(|w| w.document())
            .map(|d| d.visibility_state() == VisibilityState::Hidden)
            .unwrap_or(false);
        PAGE_HIDDEN.store(hidden, std::sync::atomic::Ordering::Relaxed);
    }) as Box<dyn FnMut()>);
    if document
        .add_event_listener_with_callback("visibilitychange", closure.as_ref().unchecked_ref())
        .is_err()
    {
        return;
    }
    closure.forget();
}

/// 复用页面里已有的 `<canvas id="sisf">`，避免 winit 自建游离 canvas。
pub(crate) fn window_attrs(attrs: WindowAttributes) -> WindowAttributes {
    match canvas_from_document() {
        Some(canvas) => attrs.with_canvas(Some(canvas)).with_focusable(true),
        None => attrs,
    }
}

fn canvas_from_document() -> Option<web_sys::HtmlCanvasElement> {
    let element = web_sys::window()?.document()?.get_element_by_id("sisf")?;
    element.dyn_into::<web_sys::HtmlCanvasElement>().ok()
}

pub fn run<A: App + 'static>(app: A, config: AppConfig) -> Result<(), Box<dyn Error>> {
    install_visibility_watcher();
    let event_loop =
        EventLoop::new().map_err(|e: EventLoopError| -> Box<dyn Error> { e.into() })?;
    // 重绘由 `Runner` 每帧末尾 `request_redraw()` 链式驱动，浏览器把它对齐到 rAF；
    // 因此 `Wait` 就够（`Poll` 在 wasm 上会以任务调度器空转）。
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop.spawn_app(Runner::new(app, config));
    Ok(())
}
