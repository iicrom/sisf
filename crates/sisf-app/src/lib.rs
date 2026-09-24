//! App 主循环 trait、固定 tick、winit/wasm/Android 生命周期与虚拟分辨率缩放。

pub mod app;
pub mod config;
pub mod frame;
pub mod platform;
pub(crate) mod runner;

pub use app::App;
pub use config::{AppConfig, ScaleMode};
pub use frame::{FrameCtx, Viewport};
pub use runner::Runner;

#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub use wasm_bindgen;

/// 示例/上层代码在 wasm 下打 console 用（避免直接依赖 web-sys）。
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub use web_sys;

/// wasm 宿主胶水（T4 起示例要 `spawn_local` 预取 VFS 清单文件）。
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub use js_sys;
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub use wasm_bindgen_futures;
