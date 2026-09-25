//! Sprite arena：可视单元的数据与采集。
//!
//! 四个模块各管一段：[`sprite`] 是数据（位图子矩形 + 变换 + tone/blend），
//! [`arena`] 是存放与句柄（slot + generation），[`draw`] 是把它们翻译成渲染器的
//! `(BatchKey, Instance)`。[`anim`] 叠在数据之上：帧表、播放状态与 RPG Maker 的方向
//! 图样（T8）—— 它只写 `Sprite::src`，采集通路对动画一无所知。[`task`] 是逐 tick 推进
//! 的补间与移动路线（`TaskScheduler` / `Tween` / `MoveRoute`）。
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

pub mod anim;
pub mod arena;
pub mod draw;
pub mod sprite;
pub mod task;

pub use anim::{
    AnimDef, AnimState, DIR_DOWN, DIR_LEFT, DIR_RIGHT, DIR_UP, DirPattern, FrameDef, LoopMode,
    dir_vector, normalize_dir,
};
pub use arena::{SpriteArena, SpriteRef};
pub use draw::{CollectError, ViewportRef};
pub use sprite::{BlendMode, Layer, Rect, Sprite, Tone, Vec2};
pub use task::{Ease, Keyframe, Lerp, MoveRoute, RouteMode, RouteNode, Task, TaskScheduler, Tween};
