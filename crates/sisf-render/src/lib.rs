//! 批量 quad 渲染器：wgpu 唯一消费者（图集/管线/Bitmap-RT/clip 栈）。
//!
//! 帧序（T3）：`begin_frame(&FrameU, clear)` → `App::draw` 里 `push` 收集实例 →
//! `end_frame()` 排序分组、写环形缓冲、单个 render pass 逐批 draw。
//! 纯逻辑部分（`vertex`/`batch`/`ring`）不碰 GPU 句柄，三端含 wasm 都能跑单测。

pub mod batch;
pub mod renderer;
pub mod ring;
pub mod vertex;

pub use batch::{Batch, BatchKey, BatchList, Blend};
pub use renderer::{DrawStats, FrameStatus, Renderer, RendererError, Rgba, WHITE_EPOCH};
pub use ring::{DEFAULT_QUADS_PER_SLOT, FRAMES_IN_FLIGHT, InstanceRing};
pub use vertex::{
    FrameU, Instance, PROJ_STACK_DEPTH, QUAD_INDICES, QUAD_VERTS, mat4_mul, ortho, ortho_viewport,
};
