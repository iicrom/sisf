//! T2 验收示例：跨平台开窗 + 清屏（Windows / wasm / Android 三端共享同一 `Runner`）。
//!
//! 运行：
//! - 桌面：`cargo run -p sisf --example hello_window`
//! - wasm：`cargo xtask web` 后访问 `http://127.0.0.1:8080/`（`?wgl` 走 WebGL2）
//! - Android：见 `docs/android.md`

use std::time::Duration;

#[cfg(target_arch = "wasm32")]
use sisf::app::wasm_bindgen;
#[cfg(target_arch = "wasm32")]
use sisf::app::web_sys;
use sisf::app::{App, AppConfig, FrameCtx, platform};
use sisf::event::EngineEvent;
use sisf::render::Renderer;

struct Hello {
    ticks: u64,
    last_event: Option<EngineEvent>,
}

impl App for Hello {
    fn on_event(&mut self, events: &[EngineEvent], _ctx: &mut FrameCtx) {
        for e in events {
            self.last_event = Some(e.clone());
            println!("[hello_window] {e:?}");
        }
    }

    fn tick(&mut self, dt: Duration, _ctx: &mut FrameCtx) {
        self.ticks += 1;
        if self.ticks.is_multiple_of(600) {
            let line = format!(
                "[hello_window] tick {} dt {dt:?} ctx {:?}",
                self.ticks, _ctx.viewport
            );
            println!("{line}");
            // 浏览器里 stdout 不可见，wasm 端打到 devtools console。
            #[cfg(target_arch = "wasm32")]
            web_sys::console::log_1(&line.into());
        }
    }

    fn draw(&mut self, _alpha: f32, _renderer: &mut Renderer) {
        // T3 接入批量 quad 后这里出图；T2 由 Runner 统一清屏。
    }
}

fn config() -> AppConfig {
    AppConfig {
        title: "SISF hello_window (T2)".to_owned(),
        // 明显的清屏色，用于验收“窗口/画布被清成 clear 色”。
        clear: [0.0, 0.35, 0.6, 1.0],
        ..Default::default()
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn sisf_start() {
    real_main();
}

pub fn main() {
    real_main();
}

fn real_main() {
    // wasm 下 stdout 不可见，启动/失败都打到 devtools console。
    #[cfg(target_arch = "wasm32")]
    web_sys::console::log_1(&"[hello_window] main 已启动".into());
    let app = Hello {
        ticks: 0,
        last_event: None,
    };
    if let Err(e) = platform::run(app, config()) {
        #[cfg(target_arch = "wasm32")]
        web_sys::console::error_1(&format!("[hello_window] 运行失败: {e}").into());
        panic!("hello_window 运行失败: {e}");
    }
}
