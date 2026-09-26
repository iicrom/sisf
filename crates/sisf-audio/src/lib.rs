//! 纯 Rust 混音器（BGM/BGS/ME/SE）+ native/web 设备 pump（T7）。
//!
//! 三层的边界就是 feature 的边界：
//! - [`mixer`] —— **四端通用、不碰设备**，任何 feature 组合下都在（`sisf-audio` 的编译底座）。
//! - [`pump_native`]（`feature = "native"`，非 wasm）/ `pump_web`（`feature = "web"`，wasm，所以宿主 doc 里链不到）
//!   —— 各自接一份设备时钟。两条都不会同时存在：`web` 只在 wasm32 上生效，`native` 只在
//!   非 wasm32 上生效，所以 [`bus`] 里的平台别名不需要 `cfg_if`。
//! - [`bus`] —— 应用侧唯一要碰的 [`AudioBus`]（混音器 + pump + 解码起播）。
//!
//! **两条 feature 都没开时本 crate 只剩 `mixer`**：没有设备通路就没有 `AudioBus`，这是刻意的
//! （引擎 DoD：平台不支持的能力要 feature 门控 + 写文档，不留空壳类型骗编译）。

pub mod mixer;

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod pump_native;
#[cfg(all(feature = "web", target_arch = "wasm32"))]
pub mod pump_web;

#[cfg(any(
    all(feature = "native", not(target_arch = "wasm32")),
    all(feature = "web", target_arch = "wasm32")
))]
pub mod bus;

#[cfg(any(
    all(feature = "native", not(target_arch = "wasm32")),
    all(feature = "web", target_arch = "wasm32")
))]
pub use bus::AudioBus;
