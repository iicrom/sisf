//! 文本子系统：cosmic-text shaping + `R8Unorm` 字形覆盖率图集。
//!
//! 输出**仍是普通 sprite quad**（`Instance::mask = 1.0`），不为文字新增管线：
//! 片元着色器看到 mask 标记就把纹理的 `.r`（覆盖率）当 alpha、颜色全取实例 tint，
//! 于是文字与精灵共用同一条 batch 通路、同一次 `end_frame`。
//!
//! 三层各司其职，边界就是"谁需要 GPU"：
//! - [`FontBundle`]：字体字节 → `FontSystem`（缺失回退默认 + warn 一次）。
//! - [`layout`]：shaping/换行 → [`TextRun`]（纯 CPU，三端可跑单测）。
//! - [`FontAtlas`]：`CacheKey → 覆盖率子矩形`，etagere 装箱 + 自管 `R8Unorm` 页。
//!
//! [`TextEngine`] 把三者收在一起，对外只到"往渲染器里画一段文字"。

pub mod atlas;
pub mod bundle;
pub mod draw;
pub mod layout;

pub use atlas::{FontAtlas, GlyphRect, MASK_EPOCH};
pub use bundle::{FontBundle, FontError, WarnOnce};
pub use draw::TextEngine;
pub use layout::{Align, GlyphKey, PlacedGlyph, TextLine, TextRun, TextStyle, layout};
