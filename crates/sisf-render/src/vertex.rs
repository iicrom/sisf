//! GPU 缓冲布局：每实例数据、单位 quad、每帧 uniform。
//!
//! 布局与 `shaders/sprite.wgsl` 的 `@location` 一一对应，改动必须同步。

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt as _;

/// viewport/投影栈深度（与 `FrameU::proj` 长度、`FrameCtx` 栈深一致）。
pub const PROJ_STACK_DEPTH: usize = 8;

/// 每实例数据，stride 80B。
///
/// 字段序沿用设计文档（`pos,size,uv0,uv1,color,rot,tone,_pad`），两处调整：
/// - `layer`：batch 已按 layer 分组，但投影索引需要在着色器里可读，故作为实例属性传入
///   （省下"每层一条 pipeline/动态 offset"）。
/// - `size` 的符号即翻转（负宽=左右翻、负高=上下翻），等价于文档说的"交换 uv0/uv1"，
///   但能组合，且不需要额外的 flip 位。
///
/// 文档写的 64B 装不下这些字段：17 个必需 f32 = 68B，加 layer 后 72B，
/// 补齐到 80B（16 的倍数，便于对齐分配）。见 todo.md「T3 期间发现并修正的事实」。
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Instance {
    /// 中心点（虚拟像素，左上原点）。
    pub pos: [f32; 2],
    /// 宽高；分量为负表示该轴翻转。
    pub size: [f32; 2],
    /// uv 左下角（纹理 rect 的 (u_min, v_min)）。
    pub uv0: [f32; 2],
    /// uv 右上角（(u_max, v_max)）。
    pub uv1: [f32; 2],
    /// 非预乘 RGBA，sRGB 数值域（着色器负责预乘与线性化）。
    pub color: [f32; 4],
    /// 旋转弧度。
    pub rot: f32,
    /// tone：rgb=偏移（`[-1,1]`），a=灰度（`[0,1]`）。
    pub tone: [f32; 4],
    /// viewport 栈索引 0..7。
    pub layer: f32,
    /// **单通道遮罩标记**（T6 文本）：`>0.5` 表示 `uv0..uv1` 指向的是 `R8Unorm` 覆盖率
    /// 图集，采样结果取 `.r` 当 alpha、颜色完全来自 `color`。仍走同一条管线，
    /// 所以"文本是普通 sprite quad"这条约束成立（见 `shaders/sprite.wgsl`）。
    pub mask: f32,
    /// 补齐到 80B，GPU 侧不读。
    pub _pad: f32,
}

const _: () = assert!(size_of::<Instance>() == 80);
const _: () = assert!(align_of::<Instance>() == 4);

impl Instance {
    /// 顶点属性描述（顶点缓冲槽 1，`VertexStepMode::Instance`）。
    pub const ATTRIBUTES: [wgpu::VertexAttribute; 9] = [
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: Self::OFF_POS,
            shader_location: 1,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: Self::OFF_SIZE,
            shader_location: 2,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: Self::OFF_UV0,
            shader_location: 3,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: Self::OFF_UV1,
            shader_location: 4,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: Self::OFF_COLOR,
            shader_location: 5,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: Self::OFF_TONE,
            shader_location: 7,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32,
            offset: Self::OFF_ROT,
            shader_location: 6,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32,
            offset: Self::OFF_LAYER,
            shader_location: 8,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32,
            offset: Self::OFF_MASK,
            shader_location: 9,
        },
    ];

    pub const OFF_POS: u64 = 0;
    pub const OFF_SIZE: u64 = 8;
    pub const OFF_UV0: u64 = 16;
    pub const OFF_UV1: u64 = 24;
    pub const OFF_COLOR: u64 = 32;
    pub const OFF_ROT: u64 = 48;
    pub const OFF_TONE: u64 = 52;
    pub const OFF_LAYER: u64 = 68;
    pub const OFF_MASK: u64 = 72;
    pub const OFF_PAD: u64 = 76;

    /// 单实例字节跨度。
    pub const STRIDE: u64 = size_of::<Self>() as u64;

    /// 一个覆盖整张纹理、位于 `layer` 的纯色矩形。
    pub fn rect(pos: [f32; 2], size: [f32; 2], color: [f32; 4], layer: u8) -> Self {
        Self {
            pos,
            size,
            uv0: [0.0, 0.0],
            uv1: [1.0, 1.0],
            color,
            rot: 0.0,
            tone: [0.0; 4],
            layer: f32::from(layer),
            mask: 0.0,
            _pad: 0.0,
        }
    }
}

/// 每帧 uniform（group0 binding0）：viewport 投影栈 + 时间。
///
/// `proj[i]` 是**列主序** mat4x4（4 列 × 4 行 = 64B），与 WGSL 的
/// `array<mat4x4<f32>, 8>` 一一对应，整块 512B。
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct FrameU {
    pub proj: [[[f32; 4]; 4]; PROJ_STACK_DEPTH],
    pub time: f32,
    /// WGSL 结构按 16 对齐，尾部补到 528B。
    pub _pad: [f32; 3],
}

const _: () = assert!(size_of::<FrameU>() == 528);

impl FrameU {
    /// 全栈一次写入；`stack[i]` 为第 i 层的列主序矩阵。
    pub fn new(stack: &[[[f32; 4]; 4]; PROJ_STACK_DEPTH], time: f32) -> Self {
        Self {
            proj: *stack,
            time,
            _pad: [0.0; 3],
        }
    }

    /// 单位栈 + time=0：无 viewport 时的占位（保证 uniform 永远合法）。
    pub fn identity(time: f32) -> Self {
        const ONE: [[f32; 4]; 4] = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        Self {
            proj: [ONE; PROJ_STACK_DEPTH],
            time,
            _pad: [0.0; 3],
        }
    }
}

/// 单位 quad：4 顶点（corner ∈ {0,1}²，左上原点）+ 2 三角形索引。
pub const QUAD_VERTS: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

pub const QUAD_INDICES: [u16; 6] = [0, 1, 2, 0, 2, 3];

/// 左上原点、单位"像素"的正交投影，映射到 NDC（列主序 `[[列]; 4]`）。
///
/// y 轴翻转：引擎坐标 y 向下，NDC y 向上。`FrameCtx` 的每一层 viewport 都靠它，
/// 离屏测试也用它把矩形精确落到某个像素上。
pub fn ortho(width: f32, height: f32) -> [[f32; 4]; 4] {
    // 退化尺寸退化成"什么都不画"，比产生 NaN/inf 好排查。
    let (sx, sy) = if width > 0.0 && height > 0.0 {
        (2.0 / width, -2.0 / height)
    } else {
        (0.0, 0.0)
    };
    [
        [sx, 0.0, 0.0, 0.0],
        [0.0, sy, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0, 1.0],
    ]
}

/// `ortho` 的 viewport 变体：内容先按**逐轴** `scale` 放大、再平移 `offset`（像素），
/// 最后投影进 `width × height` 的物理像素空间。`Fill` 缩放下两轴系数不同。
pub fn ortho_viewport(width: f32, height: f32, scale: [f32; 2], offset: [f32; 2]) -> [[f32; 4]; 4] {
    let base = ortho(width, height);
    let m = [
        [scale[0], 0.0, 0.0, 0.0],
        [0.0, scale[1], 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [offset[0], offset[1], 0.0, 1.0],
    ];
    mat4_mul(&base, &m)
}

/// 列主序 4x4 矩阵乘法（`a * b`：对列向量右乘，即 `a` 后接 `b`）。
pub fn mat4_mul(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0.0f32; 4]; 4];
    for (col, out_col) in out.iter_mut().enumerate() {
        for (row, v) in out_col.iter_mut().enumerate() {
            // a 的第 row 行 · b 的第 col 列（列主序：b[col][k] 是第 col 列第 k 行）。
            *v = (0..4).map(|k| a[k][row] * b[col][k]).sum();
        }
    }
    out
}

/// 列主序矩阵乘列向量：结果第 `i` 个分量 = `Σ_j m[j][i] * v[j]`。
pub fn mat4_xform(m: &[[f32; 4]; 4], v: [f32; 4]) -> [f32; 4] {
    let mut out = [0.0f32; 4];
    for (i, o) in out.iter_mut().enumerate() {
        *o = (0..4).map(|j| m[j][i] * v[j]).sum();
    }
    out
}

/// 把 CPU 侧 `[[f32;4];4]`（列主序）矩阵转成 wgpu 可写入的字节。
pub fn frame_u_bytes(frame: &FrameU) -> &[u8] {
    bytemuck::bytes_of(frame)
}

/// 创建单位 quad 顶点缓冲 + 索引缓冲。
pub fn create_quad_buffers(device: &wgpu::Device) -> (wgpu::Buffer, wgpu::Buffer) {
    let vertex = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("sisf-quad-verts"),
        contents: bytemuck::cast_slice(&QUAD_VERTS),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("sisf-quad-indices"),
        contents: bytemuck::cast_slice(&QUAD_INDICES),
        usage: wgpu::BufferUsages::INDEX,
    });
    (vertex, index)
}

#[cfg(test)]
mod tests {
    use super::*;
    // T12：`cargo xtask wasm-test` 在 wasm 上跑这一组（见 docs/platforms.md）。
    use std::mem::offset_of;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    #[test]
    fn instance_field_offsets_match_stride() {
        assert_eq!(size_of::<Instance>(), 80);
        assert_eq!(offset_of!(Instance, pos) as u64, Instance::OFF_POS);
        assert_eq!(offset_of!(Instance, size) as u64, Instance::OFF_SIZE);
        assert_eq!(offset_of!(Instance, uv0) as u64, Instance::OFF_UV0);
        assert_eq!(offset_of!(Instance, uv1) as u64, Instance::OFF_UV1);
        assert_eq!(offset_of!(Instance, color) as u64, Instance::OFF_COLOR);
        assert_eq!(offset_of!(Instance, rot) as u64, Instance::OFF_ROT);
        assert_eq!(offset_of!(Instance, tone) as u64, Instance::OFF_TONE);
        assert_eq!(offset_of!(Instance, layer) as u64, Instance::OFF_LAYER);
        assert_eq!(offset_of!(Instance, mask) as u64, Instance::OFF_MASK);
        assert_eq!(offset_of!(Instance, _pad) as u64, Instance::OFF_PAD);
        // 属性表不得越过最后一个字段。
        for a in Instance::ATTRIBUTES {
            assert!(a.offset + a.format.size() <= Instance::STRIDE);
        }
    }

    #[test]
    fn frame_uniform_layout_matches_wgsl() {
        let stack = [[[7.0; 4]; 4]; PROJ_STACK_DEPTH];
        let f = FrameU::new(&stack, 1.5);
        assert_eq!(size_of::<FrameU>(), 528);
        // 每个 mat4x4 占 64B：第 i 层起始 = i*64，time 落在 512。
        assert_eq!(offset_of!(FrameU, time), 512);
        assert_eq!(f.proj[3][0], [7.0; 4]);
        let id = FrameU::identity(0.0);
        assert_eq!(
            id.proj[7],
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0]
            ]
        );
    }

    #[test]
    fn quad_is_unit_square_ccw() {
        assert_eq!(QUAD_VERTS.len(), 4);
        assert_eq!(QUAD_INDICES, [0, 1, 2, 0, 2, 3]);
    }

    /// `(m * v)`，列主序：结果第 i 个分量 = Σ_j m[j][i] * v[j]。
    fn xform(m: &[[f32; 4]; 4], v: [f32; 4]) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        for (i, o) in out.iter_mut().enumerate() {
            *o = (0..4).map(|j| m[j][i] * v[j]).sum();
        }
        out
    }

    #[test]
    fn ortho_maps_pixel_space_to_ndc() {
        let m = ortho(64.0, 32.0);
        // 左上角 → NDC (-1, +1)。
        assert_eq!(xform(&m, [0.0, 0.0, 0.0, 1.0]), [-1.0, 1.0, 0.0, 1.0]);
        // 右下角 → NDC (+1, -1)：y 轴翻转。
        assert_eq!(xform(&m, [64.0, 32.0, 0.0, 1.0]), [1.0, -1.0, 0.0, 1.0]);
        // 中心 → 原点。
        let c = xform(&m, [32.0, 16.0, 0.0, 1.0]);
        assert!((c[0] - 0.0).abs() < 1e-6 && (c[1] - 0.0).abs() < 1e-6);
        // 退化尺寸不得产出 NaN/inf。
        let bad = ortho(0.0, 0.0);
        assert!(
            xform(&bad, [10.0, 10.0, 0.0, 1.0])
                .iter()
                .all(|v| v.is_finite())
        );
    }

    #[test]
    fn mat4_mul_is_function_composition() {
        let scale = [
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 3.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let translate = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [10.0, 20.0, 0.0, 1.0],
        ];
        // 先缩放后平移：(1,1) → (2,3) → (12,23)。
        let m = mat4_mul(&translate, &scale);
        assert_eq!(xform(&m, [1.0, 1.0, 0.0, 1.0]), [12.0, 23.0, 0.0, 1.0]);
        // viewport 组合：内容坐标 (1,1) 经 scale=2、offset=(4,8) 落到像素 (6,10)。
        let vp = ortho_viewport(64.0, 32.0, [2.0, 2.0], [4.0, 8.0]);
        let p = xform(&vp, [1.0, 1.0, 0.0, 1.0]);
        let q = xform(&ortho(64.0, 32.0), [6.0, 10.0, 0.0, 1.0]);
        assert_eq!(p, q);
        // 恒等 viewport == 纯 ortho。
        assert_eq!(
            ortho_viewport(64.0, 32.0, [1.0, 1.0], [0.0, 0.0]),
            ortho(64.0, 32.0)
        );
    }
}
