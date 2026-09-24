//! 批量 quad 渲染器：目标生命周期（T2 交换链 / T5 离屏）+ 实例管线与批处理（T3）
//! + clip 栈与位图绘制会话（T6）。
//!
//! 帧序：`begin_frame(FrameU, clear)` → `App::draw` 里 `push`/`push_list` 收集实例 →
//! `end_frame()` 排序分组、写环形缓冲、开**一个** render pass 逐批 draw、提交并呈现。
//! 之所以 pass 在 `end_frame` 里才开：wgpu 的 `RenderPass` 借用 encoder 与视图，
//! 跨方法持有会把 `Renderer` 变成自引用结构。
//!
//! T6 的两条新通路都**复用同一套管线**，只是换了 pass 的目标与 uniform：
//! - `push_clip/pop_clip`：裁剪代号进 `BatchKey::clip`，记录时翻译成 `scissor_rect`。
//!   裁剪一变即断批（合批键里 clip 排在 epoch 之前），所以"改裁剪"与"换纹理"一样自然分批。
//! - `begin_draw/end_draw`（见 `bitmap` 模块）：把实例画进一张位图渲染目标，pass 目标
//!   从交换链换成那块纹理，投影换成 `ortho(w,h)`。会话内**按提交顺序**成批
//!   （`BatchList::build_in_order`），因为 RPG Maker 的绘制指令顺序就是覆盖顺序。
//!
//! 目标有两种（`Target`）：窗口交换链，或 `new_offscreen` 建的离屏纹理 +
//! `readback()`（wgpu 30 无软件光栅，golden 必须走真 adapter，见 `docs/platforms.md`）。
//! 两条路径共用同一套 `Quad`（管线/槽/bind group），所以离屏断言即出货断言。

use std::borrow::Cow;
use std::collections::HashMap;

use crate::batch::{Batch, BatchKey, BatchList, Blend};
use crate::bitmap::{Paint, PainterCore};
use crate::clip::{ClipStack, NO_CLIP, rect_to_pixels};
use crate::ring::{DEFAULT_QUADS_PER_SLOT, FRAMES_IN_FLIGHT, InstanceRing};
use crate::vertex::{FrameU, Instance, PROJ_STACK_DEPTH, ortho};
use crate::vertex::{QUAD_INDICES, create_quad_buffers, frame_u_bytes};

const SPRITE_SHADER: &str = include_str!("../shaders/sprite.wgsl");

/// 白色占位纹理的代号：T4 接图集前所有实例都用它（纯色矩形）。
pub const WHITE_EPOCH: u64 = 0;

/// 统一颜色类型：RGBA、非预乘、sRGB 数值域 `[0,1]`。
pub type Rgba = [f32; 4];

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// `Rgba` 是 sRGB 数值域，而 `wgpu::Color` 按线性空间解释（sRGB 格式纹理由硬件再编码回 sRGB），
/// 所以清屏前必须转线性——否则颜色会偏亮（实测 0.35 → 160/255 而非 89/255）。
pub(crate) fn to_color(c: Rgba) -> wgpu::Color {
    wgpu::Color {
        r: srgb_to_linear(c[0]) as f64,
        g: srgb_to_linear(c[1]) as f64,
        b: srgb_to_linear(c[2]) as f64,
        a: f64::from(c[3]),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RendererError {
    #[error("创建 surface 失败: {0}")]
    Surface(#[from] wgpu::CreateSurfaceError),
    #[error("没有可用的 GPU adapter: {0}")]
    Adapter(#[from] wgpu::RequestAdapterError),
    #[error("请求设备失败: {0}")]
    Device(#[from] wgpu::RequestDeviceError),
    #[error("窗口句柄不可用: {0}")]
    Handle(String),
    #[error("窗口尚未创建")]
    NoWindow,
    #[error("无法确定 surface 默认配置 {width}x{height}")]
    NoConfig { width: u32, height: u32 },
    #[error("GPU 读回失败: {0}")]
    Readback(String),
}

type SurfaceCfg = wgpu::SurfaceConfiguration;

/// 一帧获取交换链的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameStatus {
    Presented,
    /// 离屏目标：本帧已写进纹理（没有交换链，故不谈"呈现"）。
    Rendered,
    /// Lost/Outdated：已按缓存配置重配，本帧丢弃。
    Reconfigured,
    /// 尺寸无效/遮挡/超时等，跳过本帧呈现。
    Skipped(&'static str),
}

/// 一帧的落地目标：窗口交换链，或离屏纹理（golden 读回、T6 的 Bitmap-RT 同一条通路）。
enum Target {
    Surface {
        surface: wgpu::Surface<'static>,
        config: Option<SurfaceCfg>,
    },
    Offscreen {
        texture: wgpu::Texture,
        view: wgpu::TextureView,
        size: (u32, u32),
    },
}

/// 离屏目标的纹理格式：与最常见的 surface 格式一致，golden 才证明的是同一条管线。
pub(crate) const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

fn make_offscreen_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("sisf-offscreen"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: OFFSCREEN_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    (texture, view)
}

pub struct Renderer {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    target: Target,
    // 以下字段对 `bitmap`（绘制会话）开放：会话与主帧共用同一套管线资源，
    // 拆成公开 API 反而会把内部结构体（`Quad`/`Paint`）漏进门面。
    pub(crate) quad: Option<Quad>,
    ring: InstanceRing,
    pub(crate) frame: Option<PendingFrame>,
    pending: BatchList<Instance>,
    /// 实例落位用的复用暂存（排序后摊平成连续字节，一次 `write_buffer` 一片）。
    pub(crate) scratch: Vec<Instance>,
    stats: Option<DrawStats>,
    /// `epoch → 纹理视图`，T4 起图集页/RT 注册进来。
    pub(crate) epoch_views: HashMap<u64, wgpu::TextureView>,
    pub(crate) warned_epochs: Vec<u64>,
    /// adapter 实际允许建线框管线（`debug_lines` 且 `POLYGON_MODE_LINE`）。
    allow_lines: bool,
    /// 主帧的裁剪栈（`begin_frame` 时按目标尺寸重置）。
    clips: ClipStack,
    /// 活动中的位图绘制会话（`begin_draw` 到 `end_draw` 之间是 `Some`）。
    pub(crate) paint: Option<Paint>,
    /// `RT epoch → 该目标的绘制内核`（uniform + 环形实例片）。跨帧复用，
    /// 因为同一张位图通常每帧都画。
    pub(crate) painters: HashMap<u64, PainterCore>,
    /// 绘制会话专用的管线集（目标恒为 `BITMAP_FORMAT`）。
    ///
    /// 不能直接用主帧那份：交换链格式在 Windows 上常是 `Bgra8UnormSrgb`，而位图页是
    /// `Rgba8UnormSrgb`，同一条管线写不了两种格式的目标。这份 Quad 只建一次、永不重建
    /// （格式固定），所以 `PainterCore` 的 group0 bind group 与它天然一致。
    pub(crate) paint_quad: Option<Quad>,
}

/// 帧间收集的实例与本次的 uniform/清屏色。
pub(crate) struct PendingFrame {
    pub(crate) frame_u: FrameU,
    clear: Rgba,
}

/// 一帧的绘制统计（`examples/sprite_bench.rs` 与单测断言用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawStats {
    pub instances: u32,
    pub batches: u32,
    pub slots: u32,
    /// 累计换槽次数（>0 说明出现过"写满 flush"）。
    pub flushes: u32,
}

/// 与 surface 格式绑定的 GPU 资源：管线、quad 缓冲、环形实例槽。
pub(crate) struct Quad {
    format: wgpu::TextureFormat,
    /// group0 布局：位图绘制会话的 `PainterCore` 要按它建自己的 uniform 组。
    pub(crate) frame_layout: wgpu::BindGroupLayout,
    /// group1 布局，`epoch → bind group` 惰性创建时用。
    batch_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    frame_group: wgpu::BindGroup,
    default_group: wgpu::BindGroup,
    /// `epoch → bind group`（同一 layout，T4 起每图集页一条）。
    groups: HashMap<u64, wgpu::BindGroup>,
    frame_uniform: wgpu::Buffer,
    quad_vb: wgpu::Buffer,
    index: wgpu::Buffer,
    /// 1x1 白色占位图（`default_group` 的目标），随 Quad 一起释放。
    _white: wgpu::Texture,
    slots: Vec<wgpu::Buffer>,
    /// `slots` 对应的每槽容量。
    slot_capacity: u32,
    normal: wgpu::RenderPipeline,
    add: wgpu::RenderPipeline,
    /// 直接替换目标（`Blend::Replace`）：`clear_rect` 用它在裁剪下写出真透明。
    replace: wgpu::RenderPipeline,
    /// `debug_lines` feature 下的线框克隆（需 `POLYGON_MODE_LINE`）。
    lines: Option<wgpu::RenderPipeline>,
}

impl Quad {
    /// `ring` 为 `None` 时不建实例槽：绘制会话用的那份自带 `PainterCore::slots`，
    /// 这里只需要管线与 group0/group1 的资源。
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        ring: Option<&InstanceRing>,
        allow_lines: bool,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sisf-sprite"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SPRITE_SHADER)),
        });
        let g0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sisf-frame-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let g1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sisf-batch-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sisf-sprite-layout"),
            bind_group_layouts: &[Some(&g0), Some(&g1)],
            immediate_size: 0,
        });
        let frame_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sisf-frame-uniform"),
            size: size_of::<FrameU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let white = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sisf-white"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        // 白色（sRGB 格式里 255 即线性 1.0），实例颜色全靠 tint。
        write_rgba8_pixel(queue, &white, &[255, 255, 255, 255]);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sisf-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let white_view = white.create_view(&Default::default());
        let frame_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sisf-frame"),
            layout: &g0,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_uniform.as_entire_binding(),
            }],
        });
        let default_group = batch_bind_group(device, &g1, &white_view, &sampler);
        let (quad_vb, index) = create_quad_buffers(device);
        let slot_capacity = ring.map_or(0, |r| r.capacity().max(1));
        let slots = ring.map_or_else(Vec::new, |r| {
            make_instance_buffers(device, r.buffers(), slot_capacity)
        });
        let buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<[f32; 2]>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 0,
                    shader_location: 0,
                }],
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: Instance::STRIDE,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &Instance::ATTRIBUTES,
            }),
        ];
        Self {
            format,
            normal: pipeline(
                device,
                &shader,
                &layout,
                format,
                Blend::Normal,
                &buffers,
                None,
            ),
            add: pipeline(device, &shader, &layout, format, Blend::Add, &buffers, None),
            replace: pipeline(
                device,
                &shader,
                &layout,
                format,
                Blend::Replace,
                &buffers,
                None,
            ),
            lines: allow_lines.then(|| {
                pipeline(
                    device,
                    &shader,
                    &layout,
                    format,
                    Blend::Normal,
                    &buffers,
                    Some(wgpu::PolygonMode::Line),
                )
            }),
            // pipeline layout 由三条管线各自持有；g1 留给 bind group 惰性创建。
            frame_layout: g0,
            batch_layout: g1,
            sampler,
            frame_group,
            // WHITE_EPOCH 预置为白色组：它永远不会"未注册"，也就不该出现回退告警。
            groups: [(WHITE_EPOCH, default_group.clone())].into_iter().collect(),
            default_group,
            frame_uniform,
            quad_vb,
            index,
            _white: white,
            slots,
            slot_capacity,
        }
    }

    fn pipeline(&self, blend: Blend) -> &wgpu::RenderPipeline {
        // `debug_lines` 构建（且 adapter 支持）下**混合语义的**批次都走线框管线；
        // 其余构建里该臂被 cfg 掉，`lines` 恒为 None。
        // `Replace` 例外：它是 `clear_rect` 的语义本体（写透明），线框会画错。
        match blend {
            Blend::Replace => &self.replace,
            _ => match self.lines.as_ref() {
                #[cfg(feature = "debug_lines")]
                Some(lines) => lines,
                _ => match blend {
                    Blend::Add => &self.add,
                    _ => &self.normal,
                },
            },
        }
    }

    /// 取（必要时创建）某代号的 group1。未注册的代号回退到白图，且只告警一次。
    fn bind_group(
        &mut self,
        device: &wgpu::Device,
        epoch: u64,
        views: &HashMap<u64, wgpu::TextureView>,
        warned: &mut Vec<u64>,
    ) -> &wgpu::BindGroup {
        if !self.groups.contains_key(&epoch) {
            // `contains_key` 的可变借用必须在此之前结束，故分两步。
            // 白色是内置回退，不算异常；其它代号没注册说明上游漏了 `register_epoch`。
            let view = views.get(&epoch);
            if view.is_none() && !warned.contains(&epoch) {
                warned.push(epoch);
                eprintln!("sisf-render: 纹理代号 {epoch} 未注册，本帧回退白色占位");
            }
            let Some(view) = view else {
                return &self.default_group;
            };
            self.groups.insert(
                epoch,
                batch_bind_group(device, &self.batch_layout, view, &self.sampler),
            );
        }
        self.groups.get(&epoch).unwrap_or(&self.default_group)
    }

    /// 容量增长后重建实例槽（旧内容由 GPU 读完，直接换新片）。
    fn resize_slots(&mut self, device: &wgpu::Device, capacity: u32) {
        self.slot_capacity = capacity.max(1);
        self.slots = make_instance_buffers(device, self.slots.len(), self.slot_capacity);
    }
}

/// 建 `n` 片可容纳 `capacity` 个实例的顶点缓冲（主帧与各位图会话同一种布局）。
pub(crate) fn make_instance_buffers(
    device: &wgpu::Device,
    n: usize,
    capacity: u32,
) -> Vec<wgpu::Buffer> {
    (0..n)
        .map(|i| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&format!("sisf-instances-{i}")),
                size: capacity.max(1) as u64 * Instance::STRIDE,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        })
        .collect()
}

/// 把排序后的实例按逻辑槽写进环形缓冲：每片一次 `write_buffer`。
///
/// `items` 必须是 `BatchList::build*` 之后的 `sorted_items()`——同槽实例在内存里连续，
/// 才能整段上传。`scratch` 只是把 `(BatchKey, Instance)` 对摊平成 `Instance` 序列。
/// `slots` 是这片环形计划的物理缓冲：主帧给 `Quad::slots`，绘制会话给
/// `PainterCore::slots`（各自轮换，互不覆盖）。
pub(crate) fn upload_slots(
    ring: &InstanceRing,
    slots: &[wgpu::Buffer],
    queue: &wgpu::Queue,
    items: &[(BatchKey, Instance)],
    scratch: &mut Vec<Instance>,
) {
    let capacity = ring.capacity();
    debug_assert!(
        slots.len() >= ring.buffers(),
        "环形计划要 {} 片缓冲，实际只有 {}",
        ring.buffers(),
        slots.len()
    );
    for logical in 0..ring.slots_used() {
        let from = (logical * capacity) as usize;
        let to = items.len().min(from + capacity as usize);
        scratch.clear();
        scratch.extend(items[from..to].iter().map(|(_, i)| *i));
        let buffer = &slots[ring.buffer_for(logical)];
        queue.write_buffer(buffer, 0, bytemuck::cast_slice(&*scratch));
    }
}

/// 在**一个** render pass 内逐批记录绘制：按批切管线、bind group、实例缓冲与裁剪。
///
/// 状态变更刻意做了去重：管线只在 blend 变化时重设、顶点缓冲只在跨物理片时重设、
/// scissor 只在裁剪代号变化时重设——这正是"相邻同键合并成一次 draw"的意义所在。
/// 主帧与位图绘制会话共用这一条：差别只在 `frame_group`（投影）、`slots`（环形片）
/// 与 `clips`（哪张裁剪表）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn record(
    quad: &mut Quad,
    pass: &mut wgpu::RenderPass,
    device: &wgpu::Device,
    batches: &[Batch],
    ring: &InstanceRing,
    slots: &[wgpu::Buffer],
    frame_group: &wgpu::BindGroup,
    clips: &ClipStack,
    views: &HashMap<u64, wgpu::TextureView>,
    warned: &mut Vec<u64>,
) {
    pass.set_bind_group(0, frame_group, &[]);
    pass.set_vertex_buffer(0, quad.quad_vb.slice(..));
    pass.set_index_buffer(quad.index.slice(..), wgpu::IndexFormat::Uint16);
    let indices = 0..QUAD_INDICES.len() as u32;
    let mut bound_blend = None;
    // 批里的实例下标是**槽内**下标，所以只有换物理片才需要重绑顶点缓冲。
    let mut bound_buffer = usize::MAX;
    let mut bound_clip = u32::MAX;
    for b in batches {
        if b.clip != bound_clip {
            let Some(r) = clips.rect_of(b.clip) else {
                continue;
            };
            if r.is_empty() {
                // 裁到什么都不剩：跳过 draw（wgpu 的 scissor 不接受 0 宽高）。
                bound_clip = b.clip;
                continue;
            }
            bound_clip = b.clip;
            pass.set_scissor_rect(r.x, r.y, r.w, r.h);
        }
        if bound_blend != Some(b.blend) {
            bound_blend = Some(b.blend);
            let pipeline = quad.pipeline(b.blend).clone();
            pass.set_pipeline(&pipeline);
        }
        let slot_buffer = ring.buffer_for(b.slot);
        if bound_buffer != slot_buffer {
            bound_buffer = slot_buffer;
            pass.set_vertex_buffer(1, slots[slot_buffer].slice(..));
        }
        let group = quad.bind_group(device, b.epoch, views, warned);
        pass.set_bind_group(1, group, &[]);
        pass.draw_indexed(indices.clone(), 0, b.start..b.start + b.len);
    }
}

fn batch_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("sisf-batch"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    blend: Blend,
    buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
    polygon: Option<wgpu::PolygonMode>,
) -> wgpu::RenderPipeline {
    let component = match blend {
        // 预乘输出：src*1 + dst*(1-srcA)；additive 只是把 dst 系数换成 1。
        Blend::Normal => wgpu::BlendComponent::OVER,
        Blend::Add => wgpu::BlendComponent {
            dst_factor: wgpu::BlendFactor::One,
            ..wgpu::BlendComponent::OVER
        },
        // 不做混合：片元输出直接成为目标像素，`clear_rect` 因此能写回"真透明"。
        Blend::Replace => wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::Zero,
            operation: wgpu::BlendOperation::Add,
        },
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(match blend {
            Blend::Normal => "sisf-sprite-normal",
            Blend::Add => "sisf-sprite-add",
            Blend::Replace => "sisf-sprite-replace",
        }),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: None,
            compilation_options: Default::default(),
            buffers,
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            polygon_mode: polygon.unwrap_or_default(),
            ..Default::default()
        },
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: None,
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState {
                    color: component,
                    alpha: match blend {
                        // alpha 通道按 1-srcA 走，additive 也不例外（否则透明区会被写脏）。
                        Blend::Normal | Blend::Add => wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        // 替换：clear_rect 要把 alpha 一起写成 0，混一下就不透明了。
                        Blend::Replace => wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::Zero,
                            operation: wgpu::BlendOperation::Add,
                        },
                    },
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// 把 4 字节写进 1x1 纹理（`bytes_per_row` 用未填充的 4，wgpu 的 `write_texture`
/// 会在内部做 256 对齐暂存，比手写 staging + copy 少一遍提交）。
fn write_rgba8_pixel(queue: &wgpu::Queue, texture: &wgpu::Texture, rgba: &[u8; 4]) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
}

impl Renderer {
    /// 同步部分（instance + surface）由调用方完成后交给这里做异步 adapter/device。
    pub async fn new(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
    ) -> Result<Self, RendererError> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await?;
        // 线框调试是可选能力：WebGL2 后端常常没有 POLYGON_MODE_LINE，
        // 缺失时降级为实心渲染并打一条提示（不算错误，见 docs/render-debug.md）。
        let want_lines = cfg!(feature = "debug_lines");
        let allow_lines = want_lines
            && adapter
                .features()
                .contains(wgpu::Features::POLYGON_MODE_LINE);
        if want_lines && !allow_lines {
            eprintln!(
                "sisf-render: debug_lines 已启用，但当前 adapter 不支持 POLYGON_MODE_LINE，线框关闭"
            );
        }
        let mut required = wgpu::Features::empty();
        if allow_lines {
            required |= wgpu::Features::POLYGON_MODE_LINE;
        }
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("sisf"),
                required_features: required,
                required_limits: adapter.limits().clone(),
                ..Default::default()
            })
            .await?;
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            target: Target::Surface {
                surface,
                config: None,
            },
            quad: None,
            ring: InstanceRing::new(DEFAULT_QUADS_PER_SLOT, FRAMES_IN_FLIGHT),
            frame: None,
            pending: BatchList::new(),
            scratch: Vec::new(),
            stats: None,
            epoch_views: HashMap::new(),
            warned_epochs: Vec::new(),
            allow_lines,
            clips: ClipStack::default(),
            paint: None,
            painters: HashMap::new(),
            paint_quad: None,
        })
    }

    /// 无窗口渲染器：离屏 `OFFSCREEN_FORMAT` 纹理（`COPY_SRC`），配 `readback` 读像素。
    ///
    /// 与窗口路径共用同一套 `Quad`/管线/环形槽，所以 golden 断言的就是实际出货的管线
    /// （wgpu 30 没有软件光栅后端，离屏也必须要有真 adapter，见 `docs/platforms.md`）。
    pub async fn new_offscreen(width: u32, height: u32) -> Result<Self, RendererError> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::all();
        let instance = wgpu::Instance::new(desc);
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("sisf-offscreen"),
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits().clone(),
                ..Default::default()
            })
            .await?;
        let (texture, view) = make_offscreen_texture(&device, width, height);
        let mut me = Self {
            instance,
            adapter,
            device,
            queue,
            target: Target::Offscreen {
                texture,
                view,
                size: (width, height),
            },
            quad: None,
            ring: InstanceRing::new(DEFAULT_QUADS_PER_SLOT, FRAMES_IN_FLIGHT),
            frame: None,
            pending: BatchList::new(),
            scratch: Vec::new(),
            stats: None,
            epoch_views: HashMap::new(),
            warned_epochs: Vec::new(),
            // 线框只用于窗口调试，离屏通路永远实心。
            allow_lines: false,
            clips: ClipStack::default(),
            paint: None,
            painters: HashMap::new(),
            paint_quad: None,
        };
        me.ensure_quad(OFFSCREEN_FORMAT);
        Ok(me)
    }

    /// 配置/重配交换链（离屏目标则按新尺寸重建纹理）。零尺寸直接忽略（Windows 最小化会先送 0x0）。
    pub fn configure(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        let format = match &mut self.target {
            Target::Surface { surface, config } => {
                let Some(cfg) = surface.get_default_config(&self.adapter, width, height) else {
                    eprintln!("sisf-render: surface {width}x{height} 无默认配置，跳过");
                    return;
                };
                surface.configure(&self.device, &cfg);
                let format = cfg.format;
                *config = Some(cfg);
                format
            }
            Target::Offscreen {
                texture,
                view,
                size,
            } => {
                if *size == (width, height) {
                    return;
                }
                let (t, v) = make_offscreen_texture(&self.device, width, height);
                *texture = t;
                *view = v;
                *size = (width, height);
                OFFSCREEN_FORMAT
            }
        };
        self.ensure_quad(format);
    }

    /// 是否已经有一个可用的绘制目标。
    pub fn is_configured(&self) -> bool {
        match &self.target {
            Target::Surface { config, .. } => config.is_some(),
            Target::Offscreen { .. } => true,
        }
    }

    /// 离屏目标：把整张纹理读回成 `RGBA8`（sRGB 字节，行无填充，`w*h*4`）。
    ///
    /// 每次调用都新建读回缓冲——golden 是低频路径，不做池化。
    pub fn readback(&mut self) -> Result<Vec<u8>, RendererError> {
        let (texture, (width, height)) = match &self.target {
            Target::Offscreen { texture, size, .. } => (texture.clone(), *size),
            Target::Surface { .. } => {
                return Err(RendererError::Readback(
                    "交换链目标不能读回，请用 `new_offscreen`".to_owned(),
                ));
            }
        };
        let pitch = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sisf-readback"),
            size: u64::from(pitch) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(pitch),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(std::iter::once(encoder.finish()));
        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| RendererError::Readback(e.to_string()))?;
        rx.recv()
            .map_err(|e| RendererError::Readback(e.to_string()))?
            .map_err(|e| RendererError::Readback(e.to_string()))?;
        let mapped = slice
            .get_mapped_range()
            .map_err(|e| RendererError::Readback(e.to_string()))?;
        let row_bytes = usize::try_from(width * 4).expect("行字节数不溢出");
        let mut out = vec![0u8; row_bytes * usize::try_from(height).expect("行号不溢出")];
        for y in 0..height {
            let from = usize::try_from(y * pitch).expect("偏移不溢出");
            let to = usize::try_from(y).expect("行号不溢出") * row_bytes;
            out[to..to + row_bytes].copy_from_slice(&mapped[from..from + row_bytes]);
        }
        drop(mapped);
        buffer.unmap();
        Ok(out)
    }

    /// 按当前 surface 格式准备（或重建）GPU 资源；格式变了必须重建管线。
    fn ensure_quad(&mut self, format: wgpu::TextureFormat) {
        let stale = self.quad.as_ref().is_some_and(|q| q.format != format);
        if stale {
            self.quad = None;
            // 旧 bind group 随 Quad 一起释放，epoch 视图本身保留在 epoch_views。
            // 绘制会话的 uniform/bind group 是按旧 layout 建的，必须一起丢。
            self.painters.clear();
            self.paint = None;
        }
        if self.quad.is_none() {
            let quad = Quad::new(
                &self.device,
                &self.queue,
                format,
                Some(&self.ring),
                self.allow_lines,
            );
            self.quad = Some(quad);
        }
    }

    /// 当前落位目标的像素尺寸；未配置返回 `(0,0)`（裁剪因此恒为空）。
    pub(crate) fn target_size(&self) -> (u32, u32) {
        match &self.target {
            Target::Surface { config, .. } => {
                config.as_ref().map_or((0, 0), |c| (c.width, c.height))
            }
            Target::Offscreen { size, .. } => *size,
        }
    }

    /// 当前目标裁剪栈的栈深（0 = 未裁剪；`push_clip`/`pop_clip` 的镜像计数）。
    ///
    /// 绘制会话里报的是**那张位图**的栈深——`push_clip` 作用的正是它，两者必须一致。
    pub fn clip_depth(&self) -> usize {
        match &self.paint {
            Some(p) => p.clips.depth(),
            None => self.clips.depth(),
        }
    }

    /// 当前目标分配过多少个裁剪代号（= 至少会出现多少次断批）。
    pub fn clip_ids_used(&self) -> u32 {
        match &self.paint {
            Some(p) => p.clips.pushed(),
            None => self.clips.pushed(),
        }
    }

    /// 按下一层裁剪，作用在**当前正在画的目标**上：主帧是窗口/离屏，绘制会话内是那张位图。
    ///
    /// `rect` 是第 `layer` 层投影下的内容坐标 `[x, y, w, h]`（会话内投影恒为
    /// `ortho(w,h)`，`layer` 被忽略、`rect` 即位图像素）。返回裁剪代号，
    /// 它进 `BatchKey::clip`；嵌套自动求交，交空即"这一层什么都不画"。
    pub fn push_clip(&mut self, layer: u32, rect: [f32; 4]) -> u32 {
        let (proj, target) = match self.paint.as_ref() {
            Some(p) => (p.proj(), p.size),
            None => {
                let proj = self
                    .frame
                    .as_ref()
                    .map(|f| f.frame_u.proj[layer as usize % PROJ_STACK_DEPTH])
                    .unwrap_or_else(|| ortho(0.0, 0.0));
                (proj, self.clips.target())
            }
        };
        let scissor = rect_to_pixels(&proj, rect, target);
        match self.paint.as_mut() {
            Some(p) => p.clips.push(scissor),
            None => self.clips.push(scissor),
        }
    }

    /// 弹出一层裁剪（栈空时 no-op，回到"不裁剪"）。
    pub fn pop_clip(&mut self) {
        match self.paint.as_mut() {
            Some(p) => p.clips.pop(),
            None => self.clips.pop(),
        }
    }

    /// 无窗口地把 `clear` 清进一张 4x4 离屏纹理并读回左上像素。
    /// 用途：在没有可用 surface 的环境里证明"GPU 清屏通路 + 颜色换算"正确
    /// （走的就是 `new_offscreen` + `end_frame` + `readback`，即 T5 golden 的同一条路径）。
    pub async fn readback_clear(clear: Rgba) -> Result<[u8; 4], RendererError> {
        let mut r = Self::new_offscreen(4, 4).await?;
        r.begin_frame(&FrameU::identity(0.0), clear);
        match r.end_frame() {
            FrameStatus::Rendered => {}
            other => return Err(RendererError::Readback(format!("离屏帧未完成: {other:?}"))),
        }
        let px = r.readback()?;
        let mut out = [0u8; 4];
        out.copy_from_slice(&px[..4]);
        Ok(out)
    }

    /// 渲染器持有的 device/queue。外部资源（T6 的字形覆盖率页）必须用**同一套**，
    /// 否则建出来的纹理进不了这个设备的 bind group。
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// 登记一个纹理代号（图集页/Bitmap-RT）供批次取用。T4 的图集走这里。
    /// 重复登记同一代号会换图并丢弃缓存的 bind group（epoch 语义 = 内容不可变）。
    pub fn register_epoch(&mut self, epoch: u64, view: wgpu::TextureView) {
        self.epoch_views.insert(epoch, view);
        for quad in [&mut self.quad, &mut self.paint_quad] {
            if let Some(quad) = quad.as_mut() {
                quad.groups.remove(&epoch);
            }
        }
    }

    pub fn unregister_epoch(&mut self, epoch: u64) {
        self.epoch_views.remove(&epoch);
        for quad in [&mut self.quad, &mut self.paint_quad] {
            if let Some(quad) = quad.as_mut() {
                quad.groups.remove(&epoch);
            }
        }
    }

    /// 单槽实例容量（可配；写满即换槽，一次 `write_buffer` + 一段 draw）。
    pub fn set_quads_per_slot(&mut self, capacity: u32) {
        let capacity = capacity.max(1);
        self.ring.set_capacity(capacity);
        if let Some(quad) = self.quad.as_mut() {
            quad.resize_slots(&self.device, capacity);
        }
    }

    pub fn quads_per_slot(&self) -> u32 {
        self.ring.capacity()
    }

    /// 上一帧的绘制统计（`end_frame` 之前为 `None`）。
    pub fn stats(&self) -> Option<DrawStats> {
        self.stats
    }

    /// 帧首：记下本帧 uniform 与清屏色，并清空上一帧实例与裁剪栈。必须在 `App::draw` 前调用。
    pub fn begin_frame(&mut self, frame: &FrameU, clear: Rgba) {
        self.pending.reset();
        self.clips.begin(self.target_size());
        self.frame = Some(PendingFrame {
            frame_u: *frame,
            clear,
        });
    }

    /// 收集一个实例（真正落位与分组发生在 `end_frame`/`end_draw`）。
    ///
    /// `key.clip` 由这里按当前裁剪栈**覆写**：调用方填什么都不作数，因为只有渲染器
    /// 知道此刻正在往哪个目标画、裁剪栈顶是谁。绘制会话内的实例直接进会话列表。
    pub fn push(&mut self, key: BatchKey, instance: Instance) {
        match self.paint.as_mut() {
            Some(p) => {
                let clip = p.clips.top();
                p.list.push(BatchKey { clip, ..key }, instance);
            }
            None => {
                let clip = self.clips.top();
                self.pending.push(BatchKey { clip, ..key }, instance);
            }
        }
    }

    /// 并入一个子列表（精灵采集 / 文本 / 子层渲染器产出）。
    ///
    /// 与 `push` 同一条规则：列表里携带的 `clip` 一律按当前栈顶重填，所以
    /// "采集时不知道自己会被画进哪个裁剪区"的列表可以直接复用。
    pub fn append_list(&mut self, list: BatchList<Instance>) {
        let clip = match self.paint.as_ref() {
            Some(p) => p.clips.top(),
            None => self.clips.top(),
        };
        let mut list = list;
        if clip != NO_CLIP {
            list.set_clip(clip);
        }
        match self.paint.as_mut() {
            Some(p) => p.list.append(list),
            None => self.pending.append(list),
        }
    }

    /// 本帧已收集的实例数（绘制会话内的实例不算，它们在自己的列表里）。
    pub fn instance_count(&self) -> usize {
        self.pending.len()
    }

    /// 收尾：排序分组 → 写 uniform 与实例槽 → **一个** render pass 逐批 draw → 提交（交换链再呈现）。
    ///
    /// 空实例集合也走完整流程，等价于一次清屏（T2 的行为）。离屏目标返回
    /// `Rendered`，随后用 `readback` 取像素。
    pub fn end_frame(&mut self) -> FrameStatus {
        // 忘了 end_draw 也不能丢内容：先把它当本帧最后一次会话补完。
        if self.paint.is_some()
            && let Err(e) = self.end_draw()
        {
            eprintln!("sisf-render: end_frame 补收尾绘制会话失败: {e}");
        }
        let Some(PendingFrame { frame_u, clear }) = self.frame.take() else {
            self.pending.reset();
            return FrameStatus::Skipped("缺少 begin_frame");
        };
        // 取出 Quad：`quad.bind_group` 需要可变借用，而 `self.*` 其余字段同时要用。
        let Some(mut quad) = self.quad.take() else {
            return FrameStatus::Skipped("surface 未配置");
        };
        let device = self.device.clone();
        let queue = self.queue.clone();

        let total = self.pending.len() as u32;
        if let Some(cap) = self.ring.begin_frame(total) {
            quad.resize_slots(&device, cap);
        }
        let batches = self.pending.build(self.ring.capacity());
        queue.write_buffer(&quad.frame_uniform, 0, frame_u_bytes(&frame_u));

        // 按槽落位 + 逐批记录，都走同一个 `record`（无 surface 的单测复用这条路径）。
        upload_slots(
            &self.ring,
            &quad.slots,
            &queue,
            self.pending.sorted_items(),
            &mut self.scratch,
        );
        // `wgpu::Buffer` 是 `Arc` 句柄，克隆一份列表 = 三个引用计数，
        // 但足以把 `quad` 的可变借用与 `slots` 的不可变借用分开。
        let frame_group = quad.frame_group.clone();
        let slots = quad.slots.clone();

        // 目标视图：交换链每帧现取（`SurfaceTexture` 必须活到 submit 之后），
        // 离屏则借现有视图（clone 一下即可解开对 `self.target` 的借用）。
        let acquired = match &self.target {
            Target::Surface { surface, config } => acquire(surface, &self.device, config)
                .map(|st| (st.texture.create_view(&Default::default()), Some(st))),
            Target::Offscreen { view, .. } => Ok((view.clone(), None)),
        };
        let (view, st) = match acquired {
            Ok(pair) => pair,
            Err(status) => {
                self.quad = Some(quad);
                return status;
            }
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("sisf"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sisf-sprite-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(to_color(clear)),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            record(
                &mut quad,
                &mut pass,
                &device,
                &batches,
                &self.ring,
                &slots,
                &frame_group,
                &self.clips,
                &self.epoch_views,
                &mut self.warned_epochs,
            );
        }
        self.stats = Some(DrawStats {
            instances: total,
            batches: batches.len() as u32,
            slots: self.ring.slots_used(),
            flushes: self.ring.flushes(),
        });
        queue.submit(std::iter::once(encoder.finish()));
        self.quad = Some(quad);
        match st {
            Some(st) => {
                queue.present(st);
                FrameStatus::Presented
            }
            None => FrameStatus::Rendered,
        }
    }
}

/// 取当前交换链图像；Lost/Outdated 时按缓存配置重配并报告。
fn acquire(
    surface: &wgpu::Surface<'static>,
    device: &wgpu::Device,
    config: &Option<SurfaceCfg>,
) -> Result<wgpu::SurfaceTexture, FrameStatus> {
    match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(st) | wgpu::CurrentSurfaceTexture::Suboptimal(st) => {
            Ok(st)
        }
        wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
            if let Some(cfg) = config.clone() {
                surface.configure(device, &cfg);
            }
            Err(FrameStatus::Reconfigured)
        }
        wgpu::CurrentSurfaceTexture::Timeout => Err(FrameStatus::Skipped("timeout")),
        wgpu::CurrentSurfaceTexture::Occluded => Err(FrameStatus::Skipped("occluded")),
        wgpu::CurrentSurfaceTexture::Validation => Err(FrameStatus::Skipped("validation")),
    }
}

/// 无 surface 的 GPU 测试：证明"同一个 RenderPass 内切换两条管线 + 换 bind group +
/// 跨物理槽重绑顶点缓冲"真的能出图（wasm 无 headless adapter，只在 native 跑）。
#[cfg(all(test, not(target_arch = "wasm32")))]
mod gpu_tests {
    use super::*;
    use crate::clip::NO_CLIP;
    use crate::vertex::{PROJ_STACK_DEPTH, ortho};

    const TARGET: u32 = 64;

    fn near(got: [u8; 4], want: [u8; 4], tol: i32) -> bool {
        got.iter()
            .zip(want.iter())
            .all(|(g, w)| (*g as i32 - *w as i32).abs() <= tol)
    }

    async fn headless_device() -> (wgpu::Device, wgpu::Queue) {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::all();
        let instance = wgpu::Instance::new(desc);
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .expect("测试环境需要可用 adapter");
        adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("sisf-test"),
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits(),
                ..Default::default()
            })
            .await
            .expect("request_device 失败")
    }

    fn rgba8_pixel(device: &wgpu::Device, queue: &wgpu::Queue, rgba: &[u8; 4]) -> wgpu::Texture {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("test-pixel"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        write_rgba8_pixel(queue, &texture, rgba);
        texture
    }

    fn rect(pos: [f32; 2], color: [f32; 4]) -> Instance {
        Instance {
            pos,
            // 28x18 留出 2px 的行列间隙，用于验证"未被覆盖的像素仍是清屏色"。
            size: [28.0, 18.0],
            uv0: [0.0, 0.0],
            uv1: [1.0, 1.0],
            color,
            rot: 0.0,
            tone: [0.0; 4],
            layer: 0.0,
            mask: 0.0,
            _pad: 0.0,
        }
    }

    /// 6 个实例分属 3 个批次（Normal / Add / 另一个 epoch），单槽容量 4 → 跨 2 个物理槽。
    #[test]
    fn one_pass_switches_pipelines_bind_groups_and_slots() {
        let (device, queue) = pollster::block_on(headless_device());
        let mut ring = InstanceRing::new(4, FRAMES_IN_FLIGHT); // 容量 4 才能逼出跨槽批次
        let mut quad = Quad::new(
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            Some(&ring),
            false,
        );

        let blue = rgba8_pixel(&device, &queue, &[0, 0, 255, 255]);
        let mut views: HashMap<u64, wgpu::TextureView> = HashMap::new();
        views.insert(7, blue.create_view(&Default::default()));
        let mut warned = Vec::new();

        let key = |epoch: u64, blend: Blend, z: i32| BatchKey {
            layer: 0,
            clip: NO_CLIP,
            epoch,
            blend,
            z,
            insertion: 0,
        };
        let mut list: BatchList<Instance> = BatchList::new();
        // 第一行：alpha-over + 白图，红 / 绿。
        list.push(
            key(WHITE_EPOCH, Blend::Normal, 0),
            rect([16.0, 11.0], [1.0, 0.0, 0.0, 1.0]),
        );
        list.push(
            key(WHITE_EPOCH, Blend::Normal, 1),
            rect([48.0, 11.0], [0.0, 1.0, 0.0, 1.0]),
        );
        // 第二行：additive + 白图，青 / 黄。
        list.push(
            key(WHITE_EPOCH, Blend::Add, 0),
            rect([16.0, 32.0], [0.0, 1.0, 1.0, 1.0]),
        );
        list.push(
            key(WHITE_EPOCH, Blend::Add, 1),
            rect([48.0, 32.0], [1.0, 1.0, 0.0, 1.0]),
        );
        // 第三行：alpha-over + 代号 7 的蓝图，白 tint / 半亮 tint。
        list.push(
            key(7, Blend::Normal, 0),
            rect([16.0, 53.0], [1.0, 1.0, 1.0, 1.0]),
        );
        list.push(
            key(7, Blend::Normal, 1),
            rect([48.0, 53.0], [0.5, 0.5, 0.5, 1.0]),
        );

        ring.begin_frame(list.len() as u32);
        let batches = list.build(ring.capacity());
        let shape: Vec<(u64, Blend, u32, u32)> = batches
            .iter()
            .map(|b| (b.epoch, b.blend, b.start, b.slot))
            .collect();
        assert_eq!(
            shape,
            vec![
                (0, Blend::Normal, 0, 0),
                (0, Blend::Add, 2, 0),
                (7, Blend::Normal, 0, 1),
            ],
            "批次应按 (layer,epoch,blend) 分组，并跨到第二个物理槽"
        );
        assert_eq!(ring.slots_used(), 2);

        let mut scratch = Vec::new();
        upload_slots(
            &ring,
            &quad.slots,
            &queue,
            list.sorted_items(),
            &mut scratch,
        );
        let stack = [ortho(TARGET as f32, TARGET as f32); PROJ_STACK_DEPTH];
        queue.write_buffer(
            &quad.frame_uniform,
            0,
            frame_u_bytes(&FrameU::new(&stack, 0.0)),
        );

        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("test-target"),
            size: wgpu::Extent3d {
                width: TARGET,
                height: TARGET,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("test-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(to_color([0.0, 0.0, 0.0, 1.0])),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            let frame_group = quad.frame_group.clone();
            let slots = quad.slots.clone();
            let mut clips = ClipStack::default();
            clips.begin((TARGET, TARGET));
            record(
                &mut quad,
                &mut pass,
                &device,
                &batches,
                &ring,
                &slots,
                &frame_group,
                &clips,
                &views,
                &mut warned,
            );
        }
        let pitch = TARGET * 4;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("test-readback"),
            size: (pitch * TARGET) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(pitch),
                    rows_per_image: Some(TARGET),
                },
            },
            wgpu::Extent3d {
                width: TARGET,
                height: TARGET,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));
        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll 失败");
        rx.recv().unwrap().expect("map_async 失败");
        let data = slice.get_mapped_range().expect("读回映射");
        let at = |x: u32, y: u32| {
            let mut p = [0u8; 4];
            p.copy_from_slice(&data[((y * TARGET + x) * 4) as usize..][..4]);
            p
        };

        assert!(
            near(at(8, 11), [255, 0, 0, 255], 3),
            "批次A左应为红 {:?}",
            at(8, 11)
        );
        assert!(
            near(at(40, 11), [0, 255, 0, 255], 3),
            "批次A右应为绿 {:?}",
            at(40, 11)
        );
        assert!(
            near(at(8, 32), [0, 255, 255, 255], 3),
            "Add批左应为青 {:?}",
            at(8, 32)
        );
        assert!(
            near(at(40, 32), [255, 255, 0, 255], 3),
            "Add批右应为黄 {:?}",
            at(40, 32)
        );
        assert!(
            near(at(8, 53), [0, 0, 255, 255], 3),
            "代号7批左应为蓝 {:?}",
            at(8, 53)
        );
        assert!(
            near(at(40, 53), [0, 0, 128, 255], 6),
            "代号7批右应为半亮蓝 {:?}",
            at(40, 53)
        );
        assert!(
            near(at(0, 0), [0, 0, 0, 255], 1),
            "间隙应保持清屏色 {:?}",
            at(0, 0)
        );
        assert!(
            near(at(32, 32), [0, 0, 0, 255], 1),
            "列间隙同 {:?}",
            at(32, 32)
        );
        assert!(warned.is_empty(), "未注册代号不该出现（白走内置回退）");
        drop(data);
        readback.unmap();
    }
}
