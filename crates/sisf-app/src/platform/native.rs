//! 桌面/Android 共用入口：winit 0.30 `EventLoop` + `run_app`。

use std::error::Error;
use std::future::Future;

use winit::event_loop::{ControlFlow, EventLoop};

use crate::app::App;
use crate::config::AppConfig;
use crate::runner::Runner;

pub fn run<A: App + 'static>(app: A, config: AppConfig) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::new()?;
    // winit 0.30 的 `RedrawRequested` 由 `Window::request_redraw()` 链式驱动
    // （见 `Runner::do_frame`），因此 `Wait` 即可满帧运行且不空转 CPU。
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut runner = Runner::new(app, config);
    let result = event_loop.run_app(&mut runner);
    // 先释放 renderer(surface/GPU)再让 window 析构（RunState 字段序 + 此处显式 drop）。
    drop(runner);
    result?;
    Ok(())
}

pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    pollster::block_on(future)
}
