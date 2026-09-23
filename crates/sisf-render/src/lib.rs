//! 批量 quad 渲染器：wgpu 唯一消费者（图集/管线/Bitmap-RT/clip 栈）。
//!
//! T2 阶段仅提供 surface 生命周期所需的最小实现（instance/adapter/device +
//! 清屏呈现）；批量管线、图集与 `push/pop_viewport` 在 T3/T4 展开。

pub mod renderer;

pub use renderer::{FrameStatus, Renderer, RendererError};

/// 统一颜色类型：RGBA、非预乘、sRGB 数值域 `[0,1]`。
pub type Rgba = [f32; 4];
