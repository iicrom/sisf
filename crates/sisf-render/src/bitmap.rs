//! 位图渲染目标（RPG Maker 的 `Bitmap`）与**绘制会话**。
//!
//! T6 的第三条通路：主帧画进交换链，golden 画进离屏纹理，这里画进**图集里的独占页**。
//! 三条共用同一套管线与 `record`，差别只有 pass 目标、`FrameU` 投影和实例环形片。
//!
//! 会话语义（`begin_draw` → 若干绘制指令 → `end_draw`）：
//! - **一次一个会话**，且不能在主帧 `begin_frame/end_frame` 之外……其实完全可以：会话
//!   自带 encoder 与 submit，因此可以嵌在帧收集中途（先画位图，再把它 blt 上屏）。
//! - 会话内**按提交顺序**成批（[`BatchList::build_in_order`]），因为 RGSS 的
//!   `fill_rect → blt → draw_text` 指令顺序就是覆盖顺序，排序会把后画的埋到下面。
//! - 目标页第一次被画时清成全透明（`pristine`），之后一律 `LoadOp::Load`——位图是
//!   持久画布，`clear` 得用 `clear_rect` 显式做。
//! - 投影恒为 `ortho(w,h)`：会话内的坐标就是**位图像素**，与 viewport 栈无关。
//!
//! 与设计稿（todo.md T6）`enum BitmapData{Atlas{page,region}, Rt{texture,view}}` 的
//! 差异是刻意的：RT 的 GPU 视图只存在 `Renderer::epoch_views` 里，`Bitmap` 句柄本身
//! 不含 `wgpu` 类型。wasm 后端的 `wgpu::Texture` 不是 `Send`，把它塞进句柄会让
//! `AssetStore<Bitmap>` 在 wasm 上直接编译不过（T4 为同一个问题已经把页纹理拆到
//! `AtlasPages`）。语义等价：`PageKind::Target` 就是 `Rt` 分支，`rect()` 就是 `region`。

use crate::batch::{BatchKey, BatchList, Blend};
use crate::clip::ClipStack;
use crate::renderer::{
    DrawStats, Quad, Renderer, Rgba, make_instance_buffers, record, to_color, upload_slots,
};
use crate::ring::{FRAMES_IN_FLIGHT, InstanceRing};
use crate::vertex::{FrameU, Instance, PROJ_STACK_DEPTH, frame_u_bytes, ortho};

/// 一个绘制会话的单槽实例容量。会话指令数很小（窗口一帧几十条），
/// 真超了 `InstanceRing` 会自己倍增扩槽。
const PAINT_QUADS_PER_SLOT: u32 = 512;

/// 位图（含图集页）的纹理格式，也是绘制会话管线绑定的目标格式。
///
/// 刻意与 `OFFSCREEN_FORMAT` 相同：golden 与出货走同一条色彩通路（sRGB 编码由硬件负责）。
pub const BITMAP_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// 一张位图在**页坐标系**里的位置：既够 `blt` 算 uv，也够 `begin_draw` 判定可写。
///
/// 刻意不直接吃 `Bitmap`（那个类型在 `image` feature 后面）：这样绘制会话与图集
/// 解耦，`sisf-text` 也能拿它描述覆盖率页上的一块。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Source {
    /// 所属页的纹理代号。
    pub epoch: u64,
    /// 页内像素矩形 `[x, y, w, h]`（采样矩形，不含 gutter）。
    pub rect: [u32; 4],
    /// 所在页的像素尺寸（uv 归一化的分母）。
    pub page: [u32; 2],
}

impl Source {
    /// 整页源（`begin_draw` 唯一接受的形式，也是 RT 自己的样子）。
    pub const fn whole(epoch: u64, width: u32, height: u32) -> Self {
        Self {
            epoch,
            rect: [0, 0, width, height],
            page: [width, height],
        }
    }

    pub const fn size(&self) -> (u32, u32) {
        (self.rect[2], self.rect[3])
    }

    /// 是否独占整页（= 可以当绘制目标：子矩形共享页会把邻居画花）。
    pub fn is_whole_page(&self) -> bool {
        self.rect == [0, 0, self.page[0], self.page[1]]
    }

    /// 子矩形（源本地像素 `[x, y, w, h]`）→ 归一化 uv `[u0, v0, u1, v1]`。
    pub fn uv_of(&self, sub: [u32; 4]) -> [f32; 4] {
        let [x0, y0, x1, y1] = self.clipped(sub);
        let [rx, ry, _, _] = self.rect;
        let (pw, ph) = (self.page[0] as f32, self.page[1] as f32);
        let (ax, ay) = (rx + x0, ry + y0);
        [
            ax as f32 / pw,
            ay as f32 / ph,
            (rx + x1) as f32 / pw,
            (ry + y1) as f32 / ph,
        ]
    }

    /// 子矩形**夹回源矩形**后的源本地相交矩形 `[x0, y0, x1, y1]`。
    ///
    /// RGSS 的 `blt` 遇到越界 `src_rect` 就是少画一点，绝不能采到隔壁子图；返回绝对
    /// 交点而不是"截断后的尺寸"，调用方才能把左上角一起挪对。
    pub fn clipped(&self, sub: [u32; 4]) -> [u32; 4] {
        let [_, _, rw, rh] = self.rect;
        let clamp_x = |v: u32| v.min(rw);
        let clamp_y = |v: u32| v.min(rh);
        let (x0, y0) = (clamp_x(sub[0]), clamp_y(sub[1]));
        let (x1, y1) = (
            clamp_x(sub[0].saturating_add(sub[2])),
            clamp_y(sub[1].saturating_add(sub[3])),
        );
        [x0, y0, x1.max(x0), y1.max(y0)]
    }
}

#[cfg(feature = "image")]
impl From<&crate::atlas_gpu::Bitmap> for Source {
    fn from(b: &crate::atlas_gpu::Bitmap) -> Self {
        let (pw, ph) = b.page_size();
        Self {
            epoch: b.epoch(),
            rect: b.rect(),
            page: [pw, ph],
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PaintError {
    /// `begin_draw` 时已有一个会话在跑（RT 套 RT 需要嵌套 pass，本期不支持）。
    #[error("已有一个绘制会话在跑，先 end_draw")]
    AlreadyDrawing,
    /// 目标必须是独占整页的位图（图集子矩形共享一页，画它会把邻居花掉）。
    #[error("目标 {rect:?} 不是独占整页（{page:?}），不能作为绘制目标")]
    NotExclusive { rect: [u32; 4], page: [u32; 2] },
    /// 目标代号没注册过：`Atlas::new_target` 之后要先 `Renderer::apply_atlas`。
    #[error("纹理代号 {0} 尚未注册（先 apply_atlas）")]
    Unregistered(u64),
    /// `end_draw` 没有对应会话。
    #[error("当前没有绘制会话")]
    NoSession,
    /// 渲染目标还没配置好（`Renderer::configure` 未跑）。
    #[error("渲染器尚未配置目标")]
    NoTarget,
    /// 零尺寸目标画不出东西。
    #[error("位图尺寸 {0}x{1} 无效")]
    Empty(u32, u32),
}

/// 活动中的绘制会话：目标视图 + 本会话的实例列表与裁剪栈。
///
/// 字段对 `renderer` 可见（`push`/`push_clip` 要按当前会话路由），故 `pub(crate)`。
pub(crate) struct Paint {
    pub(crate) epoch: u64,
    pub(crate) size: (u32, u32),
    pub(crate) list: BatchList<Instance>,
    pub(crate) clips: ClipStack,
    view: wgpu::TextureView,
    proj: [[f32; 4]; 4],
    time: f32,
}

impl Paint {
    /// 会话坐标 → 目标像素的投影（`push_clip` 用它把内容矩形换成 scissor）。
    pub(crate) fn proj(&self) -> [[f32; 4]; 4] {
        self.proj
    }

    /// 会话 uniform：八层全填同一个 `ortho(w,h)`，所以实例写哪层都一样。
    fn frame_u(&self) -> FrameU {
        FrameU::new(&[self.proj; PROJ_STACK_DEPTH], self.time)
    }
}

/// 一个目标的常驻 GPU 资源（独立于主帧的 uniform 与环形实例片）。
///
/// 按 RT 代号存在 `Renderer::painters` 里跨帧复用：一张位图通常每帧都要重画，
/// 每帧重建缓冲与 bind group 是白送的性能。
pub(crate) struct PainterCore {
    uniform: wgpu::Buffer,
    frame_group: wgpu::BindGroup,
    slots: Vec<wgpu::Buffer>,
    ring: InstanceRing,
    /// 页纹理刚建好时内容未定义：首次会话先清成透明，此后 `Load` 保留旧像素。
    pristine: bool,
}

impl PainterCore {
    fn new(device: &wgpu::Device, quad: &Quad) -> Self {
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sisf-paint-uniform"),
            size: size_of::<FrameU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sisf-paint-frame"),
            layout: &quad.frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let ring = InstanceRing::new(PAINT_QUADS_PER_SLOT, FRAMES_IN_FLIGHT);
        let slots = make_instance_buffers(device, ring.buffers(), ring.capacity());
        Self {
            uniform,
            frame_group,
            slots,
            ring,
            pristine: true,
        }
    }
}

impl Renderer {
    /// 是否正处于绘制会话中。
    pub fn in_draw(&self) -> bool {
        self.paint.is_some()
    }

    /// 当前活动目标（位图）的像素尺寸；会话外是主帧目标尺寸。
    pub fn paint_size(&self) -> (u32, u32) {
        match &self.paint {
            Some(p) => p.size,
            None => self.target_size(),
        }
    }

    /// 开始往一张位图上画。之后的 `push`/`push_clip`/`fill_rect`/`blt` 都作用在它身上，
    /// 直到 [`Renderer::end_draw`]。
    ///
    /// `target` 必须是独占整页的位图（`Atlas::new_target` 造的 RT），且已经过
    /// `apply_atlas` 注册。会话开始即重置该目标自己的裁剪栈。
    pub fn begin_draw(&mut self, target: &Source) -> Result<(), PaintError> {
        if self.paint.is_some() {
            return Err(PaintError::AlreadyDrawing);
        }
        if self.quad.is_none() {
            return Err(PaintError::NoTarget);
        }
        let (w, h) = target.size();
        if w == 0 || h == 0 {
            return Err(PaintError::Empty(w, h));
        }
        if !target.is_whole_page() {
            return Err(PaintError::NotExclusive {
                rect: target.rect,
                page: target.page,
            });
        }
        let Some(view) = self.epoch_views.get(&target.epoch).cloned() else {
            return Err(PaintError::Unregistered(target.epoch));
        };
        let mut clips = ClipStack::default();
        clips.begin((w, h));
        self.paint = Some(Paint {
            epoch: target.epoch,
            size: (w, h),
            list: BatchList::new(),
            clips,
            view,
            proj: ortho(w as f32, h as f32),
            // 会话内没有动画语义；沿用本帧时间，便于同一条管线复用。
            time: self.frame.as_ref().map(|f| f.frame_u.time).unwrap_or(0.0),
        });
        Ok(())
    }

    /// 收尾并**独立提交**本会话（一个 render pass，按提交顺序逐批 draw）。
    pub fn end_draw(&mut self) -> Result<DrawStats, PaintError> {
        let Some(paint) = self.paint.take() else {
            return Err(PaintError::NoSession);
        };
        // 先取走需要整个 `paint` 的东西（uniform 与键），再拆字段。
        let epoch = paint.epoch;
        let frame_u = paint.frame_u();
        let Paint {
            mut list,
            clips,
            view,
            ..
        } = paint;
        // 会话管线集绑定的是位图格式，与交换链格式无关（见 `Renderer::paint_quad`）。
        let device = self.device.clone();
        let queue = self.queue.clone();
        let mut quad = self
            .paint_quad
            .take()
            .unwrap_or_else(|| Quad::new(&device, &queue, BITMAP_FORMAT, None, false));
        let mut core = self
            .painters
            .remove(&epoch)
            .unwrap_or_else(|| PainterCore::new(&device, &quad));
        let total = list.len() as u32;
        if let Some(cap) = core.ring.begin_frame(total) {
            core.slots = make_instance_buffers(&device, core.ring.buffers(), cap);
        }
        let batches = list.build_in_order(core.ring.capacity());
        let stats = DrawStats {
            instances: total,
            batches: batches.len() as u32,
            slots: core.ring.slots_used(),
            flushes: core.ring.flushes(),
        };
        queue.write_buffer(&core.uniform, 0, frame_u_bytes(&frame_u));
        upload_slots(
            &core.ring,
            &core.slots,
            &queue,
            list.sorted_items(),
            &mut self.scratch,
        );
        let frame_group = core.frame_group.clone();
        let slots = core.slots.clone();
        let clear_first = core.pristine;
        core.pristine = false;
        {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("sisf-paint"),
            });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("sisf-bitmap-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            // 未初始化过的页先清透明；位图是持久画布，之后一律保留。
                            load: if clear_first {
                                wgpu::LoadOp::Clear(to_color([0.0, 0.0, 0.0, 0.0]))
                            } else {
                                wgpu::LoadOp::Load
                            },
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
                    &core.ring,
                    &slots,
                    &frame_group,
                    &clips,
                    &self.epoch_views,
                    &mut self.warned_epochs,
                );
            }
            queue.submit(std::iter::once(encoder.finish()));
        }
        self.paint_quad = Some(quad);
        self.painters.insert(epoch, core);
        Ok(stats)
    }

    /// RGSS 的 `Bitmap#blt(x, y, src, src_rect, opacity)`：把源子矩形贴到当前目标。
    ///
    /// `dst` 是**左上角**（RGSS 语义），`opacity` 取 0..=255（`u8`，与 RPG Maker 一致）。
    /// 当前没有会话时画进主帧（layer 0）。源就是当前目标时跳过并告警：同张贴自己
    /// 是读写了同一张纹理，wgpu 会判校验错误，不如少画这一笔。
    pub fn blt(&mut self, dst: [f32; 2], src: &Source, src_rect: [u32; 4], opacity: u8) {
        let [x0, y0, x1, y1] = src.clipped(src_rect);
        if x0 == x1 || y0 == y1 {
            return;
        }
        if self.paint.as_ref().is_some_and(|p| p.epoch == src.epoch) {
            eprintln!("sisf-render: blt 源就是当前绘制目标，跳过（读写同一张贴）");
            return;
        }
        let (pw, ph) = (src.page[0] as f32, src.page[1] as f32);
        let [rx, ry, _, _] = src.rect;
        // 被夹掉的那部分源像素，目标左上角要一起挪，否则贴出来的内容是"往回缩"的。
        let (shift_x, shift_y) = (
            i64::from(x0) - i64::from(src_rect[0]),
            i64::from(y0) - i64::from(src_rect[1]),
        );
        let (w, h) = ((x1 - x0) as f32, (y1 - y0) as f32);
        let key = self.blit_key(src.epoch);
        let mut inst = Instance::rect(
            [
                dst[0] + shift_x as f32 + w / 2.0,
                dst[1] + shift_y as f32 + h / 2.0,
            ],
            [w, h],
            [1.0, 1.0, 1.0, f32::from(opacity) / 255.0],
            0,
        );
        inst.uv0 = [(rx + x0) as f32 / pw, (ry + y0) as f32 / ph];
        inst.uv1 = [(rx + x1) as f32 / pw, (ry + y1) as f32 / ph];
        self.push(key, inst);
    }

    /// RGSS 的 `Bitmap#fill_rect(x, y, w, h, color)`：alpha-over 铺一块纯色。
    pub fn fill_rect(&mut self, rect: [f32; 4], color: Rgba) {
        let key = self.blit_key(crate::renderer::WHITE_EPOCH);
        self.push(
            key,
            Instance::rect(
                [rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0],
                [rect[2], rect[3]],
                color,
                0,
            ),
        );
    }

    /// RGSS 的 `Bitmap#clear_rect(x, y, w, h)`：把一块写回**真透明**。
    ///
    /// wgpu 没有"按矩形 clear 纹理"的接口（`clear_texture` 是整个 subresource），
    /// 所以这是一个 `Blend::Replace` 的纯色 quad——它不做混合，片元输出即目标像素。
    pub fn clear_rect(&mut self, rect: [u32; 4]) {
        let key = self.blit_key(crate::renderer::WHITE_EPOCH);
        let k = BatchKey {
            blend: Blend::Replace,
            ..key
        };
        let (w, h) = (rect[2] as f32, rect[3] as f32);
        self.push(
            k,
            Instance::rect(
                [rect[0] as f32 + w / 2.0, rect[1] as f32 + h / 2.0],
                [w, h],
                [0.0, 0.0, 0.0, 0.0],
                0,
            ),
        );
    }

    /// 采集端公共的 `BatchKey`：会话里 layer 恒 0（投影与栈无关），z 用提交序号即可
    /// （`build_in_order` 不看 z，只用它保持同组实例的相对顺序）。
    fn blit_key(&self, epoch: u64) -> BatchKey {
        BatchKey {
            layer: 0,
            clip: 0,
            epoch,
            blend: Blend::Normal,
            z: 0,
            insertion: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_page_source_is_the_only_drawable() {
        let rt = Source::whole(1 << 62 | 7, 320, 200);
        assert!(rt.is_whole_page());
        assert_eq!(rt.size(), (320, 200));
        let sub = Source {
            epoch: 3,
            rect: [4, 4, 32, 16],
            page: [256, 256],
        };
        assert!(!sub.is_whole_page(), "图集子矩形不能当绘制目标");
    }

    /// uv 换算按页尺寸归一化，且越界子矩形夹回源内（不得采到隔壁子图）。
    #[test]
    fn uv_of_normalizes_by_page_and_clips_to_source() {
        let b = Source {
            epoch: 1,
            rect: [10, 20, 30, 40],
            page: [100, 200],
        };
        assert_eq!(b.uv_of([0, 0, 30, 40]), [0.1, 0.1, 0.4, 0.3]);
        assert_eq!(b.uv_of([5, 5, 10, 10]), [0.15, 0.125, 0.25, 0.175]);
        // 右下角越界：夹回源矩形，绝对坐标仍落在 [0.1,0.4]x[0.1,0.3] 内。
        assert_eq!(b.clipped([25, 35, 10, 10]), [25, 35, 30, 40]);
        assert_eq!(b.uv_of([25, 35, 10, 10]), [0.35, 0.275, 0.4, 0.3]);
        // 完全在右侧/下侧外 ⇒ 退化成交线（x0 == x1），`blt` 因此直接跳过。
        assert_eq!(b.clipped([40, 0, 10, 10]), [30, 0, 30, 10]);
        assert_eq!(b.clipped([0, 50, 10, 10]), [0, 40, 10, 40]);
    }
}

/// T6 验收：位图渲染目标能被画、能被当普通位图采样，且会话里的裁剪真的下发 scissor。
///
/// 走 `Renderer::new_offscreen`（真 adapter + 读回），因为 wgpu 30 没有软件光栅；
/// wasm 上拿不到 headless adapter，故只在 native 跑（见 `docs/platforms.md`）。
#[cfg(all(test, feature = "image", not(target_arch = "wasm32")))]
mod gpu_tests {
    use super::{BITMAP_FORMAT, Source};
    use crate::atlas_gpu::{Atlas, AtlasPages, UploadRing};
    use crate::renderer::{FrameStatus, Renderer};
    use crate::vertex::{FrameU, PROJ_STACK_DEPTH, ortho};

    /// 主目标：64x32，左半贴"拷贝结果"、右半贴"原图"，清屏蓝。
    const TW: u32 = 64;
    const TH: u32 = 32;
    const SIDE: u32 = 32;
    const BLUE: [u8; 4] = [0, 0, 255, 255];

    fn near(got: &[u8], want: [u8; 4], tol: i32) -> bool {
        got.iter()
            .zip(want.iter())
            .all(|(g, w)| (*g as i32 - *w as i32).abs() <= tol)
    }

    fn harness() -> (Renderer, Atlas, AtlasPages, UploadRing) {
        let mut r = pollster::block_on(Renderer::new_offscreen(TW, TH)).expect("new_offscreen");
        let atlas = Atlas::with_limits(256, 6);
        let mut pages = AtlasPages::new();
        let mut uploads = UploadRing::new();
        // 页纹理在 `apply_atlas` 里才建出来，会话要的是注册后的视图。
        r.apply_atlas(&atlas, &mut pages, &mut uploads);
        (r, atlas, pages, uploads)
    }

    /// 主帧：把两块位图并排贴到离屏目标，读回像素。
    fn present(left: &Source, right: &Source, r: &mut Renderer) -> Vec<u8> {
        let proj = ortho(TW as f32, TH as f32);
        r.begin_frame(
            &FrameU::new(&[proj; PROJ_STACK_DEPTH], 0.0),
            [0.0, 0.0, 1.0, 1.0],
        );
        r.blt([0.0, 0.0], left, [0, 0, SIDE, SIDE], 255);
        r.blt([SIDE as f32, 0.0], right, [0, 0, SIDE, SIDE], 255);
        assert_eq!(r.end_frame(), FrameStatus::Rendered);
        r.readback().expect("读回")
    }

    /// 一次画布：`fill_rect → clear_rect` → 另一张 `blt` 整页 → 上屏。
    ///
    /// 断言的是"RT 既能写也能读、且拷贝不改像素"——`PageKind::Target` 走的就是
    /// 设计稿里 `BitmapData::Rt` 那条分支。
    #[test]
    fn bitmap_target_survives_a_round_trip_copy() {
        let (mut r, atlas, mut pages, mut uploads) = harness();
        let a = atlas.new_target(SIDE, SIDE, 0).expect("RT A");
        let b = atlas.new_target(SIDE, SIDE, 1).expect("RT B");
        r.apply_atlas(&atlas, &mut pages, &mut uploads);

        r.begin_draw(&Source::from(&a)).expect("会话 A");
        r.fill_rect([0.0, 0.0, SIDE as f32, SIDE as f32], [1.0, 0.0, 0.0, 1.0]);
        r.clear_rect([8, 8, 8, 8]);
        let stats = r.end_draw().expect("收尾 A");
        assert_eq!(stats.instances, 2);
        assert_eq!(stats.batches, 2, "Normal 与 Replace 混不得 ⇒ 两批");

        r.begin_draw(&Source::from(&b)).expect("会话 B");
        r.blt([0.0, 0.0], &Source::from(&a), [0, 0, SIDE, SIDE], 255);
        let stats = r.end_draw().expect("收尾 B");
        assert_eq!(stats.batches, 1, "B 只有一条 blt");

        let px = present(&Source::from(&b), &Source::from(&a), &mut r);
        let at = |x: u32, y: u32| {
            let i = usize::try_from((y * TW + x) * 4).expect("偏移不溢出");
            &px[i..i + 4]
        };
        // 左上：拷贝过来的红；洞里透出清屏蓝。
        assert!(
            near(at(2, 2), [255, 0, 0, 255], 3),
            "B(2,2) 应为红 {:?}",
            at(2, 2)
        );
        assert!(near(at(12, 12), BLUE, 3), "B 的洞应透出蓝 {:?}", at(12, 12));
        // 右上：原图 A，与 B 逐点一致 ⇒ 拷贝是恒等的。
        assert!(
            near(at(34, 2), [255, 0, 0, 255], 3),
            "A(2,2) 应为红 {:?}",
            at(34, 2)
        );
        assert!(near(at(44, 12), BLUE, 3), "A 的洞应透出蓝 {:?}", at(44, 12));
        // 右下角：A 的 (31,31) 也必须是红——两半逐点一致才叫"拷贝恒等"。
        assert!(
            near(at(63, 31), [255, 0, 0, 255], 3),
            "A(31,31) 应为红 {:?}",
            at(63, 31)
        );
        assert!(
            near(at(31, 31), [255, 0, 0, 255], 3),
            "B(31,31) 应为红 {:?}",
            at(31, 31)
        );
    }

    /// 会话内 `push_clip` → scissor 真生效，且"裁剪一变即断批"。
    #[test]
    fn session_clip_scissors_and_breaks_batches() {
        let (mut r, atlas, mut pages, mut uploads) = harness();
        let c = atlas.new_target(SIDE, SIDE, 0).expect("RT C");
        r.apply_atlas(&atlas, &mut pages, &mut uploads);

        r.begin_draw(&Source::from(&c)).expect("会话 C");
        let half = r.push_clip(0, [0.0, 0.0, SIDE as f32 / 2.0, SIDE as f32]);
        assert_ne!(half, crate::clip::NO_CLIP);
        assert_eq!(r.clip_depth(), 1, "会话里的栈深要报位图自己的");
        // 白：只有左半出得来。
        r.fill_rect([0.0, 0.0, SIDE as f32, SIDE as f32], [1.0, 1.0, 1.0, 1.0]);
        r.pop_clip();
        // 绿：整宽，但只在 y>=16。
        r.fill_rect(
            [0.0, SIDE as f32 / 2.0, SIDE as f32, SIDE as f32 / 2.0],
            [0.0, 1.0, 0.0, 1.0],
        );
        let ids = r.clip_ids_used();
        let stats = r.end_draw().expect("收尾 C");
        assert_eq!(ids, 1);
        assert_eq!(r.clip_depth(), 0, "收尾前栈应已弹空");
        assert_eq!(stats.instances, 2);
        assert_eq!(stats.batches, 2, "同图同 blend，只因 clip 不同 ⇒ 断批");

        let px = present(&Source::from(&c), &Source::from(&c), &mut r);
        let at = |x: u32, y: u32| {
            let i = usize::try_from((y * TW + x) * 4).expect("偏移不溢出");
            &px[i..i + 4]
        };
        assert!(
            near(at(4, 4), [255, 255, 255, 255], 3),
            "clip 内应为白 {:?}",
            at(4, 4)
        );
        assert!(
            near(at(20, 4), BLUE, 3),
            "clip 外应仍是透明→蓝 {:?}",
            at(20, 4)
        );
        assert!(
            near(at(4, 20), [0, 255, 0, 255], 3),
            "下半应被绿覆盖 {:?}",
            at(4, 20)
        );
        assert!(
            near(at(20, 20), [0, 255, 0, 255], 3),
            "绿不受上一条 clip 影响 {:?}",
            at(20, 20)
        );
    }

    /// 位图格式与离屏/图集页一致：三端只此一条色彩通路。
    #[test]
    fn bitmap_format_is_the_offscreen_format() {
        assert_eq!(BITMAP_FORMAT, crate::renderer::OFFSCREEN_FORMAT);
    }
}
