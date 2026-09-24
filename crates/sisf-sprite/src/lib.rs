//! Sprite arena：可视单元的数据与采集。
//!
//! 三个模块各管一段：[`sprite`] 是数据（位图子矩形 + 变换 + tone/blend），
//! [`arena`] 是存放与句柄（slot + generation），[`draw`] 是把它们翻译成渲染器的
//! `(BatchKey, Instance)`。动画状态在 T8 叠加，位图渲染目标在 T6 接入。
//!
//! 能力边界（哪些 tone/blend 有通路、`src` 越界怎么裁、锚点为什么同时是旋转轴心）
//! 见 `docs/sprite.md`。
//!
//! ```
//! use sisf_sprite::{Layer, Sprite, SpriteArena, Vec2, ViewportRef};
//! use sisf_render::BatchList;
//!
//! let mut arena = SpriteArena::new();
//! let mut s = Sprite::solid(Vec2::new(16.0, 16.0));
//! s.pos = Vec2::new(100.0, 80.0);
//! s.anchor = Vec2::CENTER;
//! let handle = arena.spawn(s);
//!
//! let mut list = BatchList::new();
//! arena.collect(ViewportRef::MAIN, &mut list).unwrap();
//! assert_eq!(list.len(), 1);
//! assert_eq!(arena.get(handle).unwrap().layer, Layer::MAIN);
//! ```

pub mod arena;
pub mod draw;
pub mod sprite;

pub use arena::{SpriteArena, SpriteRef};
pub use draw::{CollectError, ViewportRef};
pub use sprite::{BlendMode, Layer, Rect, Sprite, Tone, Vec2};
