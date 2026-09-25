//! App 主循环 trait、固定 tick、winit/wasm/Android 生命周期与虚拟分辨率缩放。

pub mod app;
pub mod config;
pub mod custom;
pub mod frame;
#[cfg(all(
    feature = "gilrs",
    not(target_arch = "wasm32"),
    not(target_os = "android")
))]
pub(crate) mod gamepad;
pub(crate) mod normalize;
pub mod platform;
pub(crate) mod runner;
pub mod timing;

pub use app::App;
pub use config::{AppConfig, ScaleMode};
pub use custom::post_custom;
pub use frame::{FrameCtx, Viewport};
pub use runner::Runner;
pub use timing::{MAX_CATCHUP, TickClock};

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
