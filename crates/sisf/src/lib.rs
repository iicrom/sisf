//! SISF：基于 wgpu 的跨平台 2D 引擎门面 crate。
//!
//! 只依赖本 crate，用 feature 开关选择平台与能力（见 `Cargo.toml` 矩阵）。
//! 典型用法（T12 会补 `cargo test --doc` 示例）：
//!
//! ```no_run
//! use sisf::app::{App, AppConfig};
//! ```

pub use sisf_app as app;
pub use sisf_asset as asset;
pub use sisf_audio as audio;
pub use sisf_event as event;
#[cfg(any(
    feature = "image",
    feature = "ogg",
    feature = "mp3",
    feature = "wav",
    feature = "midi",
    feature = "video",
    feature = "gif",
    feature = "simd-jpeg"
))]
pub use sisf_media as media;
pub use sisf_render as render;
pub use sisf_sprite as sprite;
pub use sisf_text as text;

#[cfg(feature = "rpgmk")]
pub use sisf_rpgmk as rpgmk;
