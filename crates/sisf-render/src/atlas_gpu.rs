//! GPU 侧图集：页纹理、`Bitmap`、每帧单一 staging ring 上传。
//!
//! - 一个 epoch = 一页 = 一张 `Rgba8UnormSrgb` 纹理（`LARGE_EPOCH` 旁路页亦然）；
//!   sRGB 交给纹理格式，像素本身不做手动 gamma。
//! - 上传走**每帧单一 staging ring**：CPU 侧一块复用缓冲，把所有待传矩形按 256
//!   对齐的 `bytes_per_row` 摆进去，一次 `write_buffer` + 每张位图一次
//!   `copy_buffer_to_texture`，最后**一次** submit。容量几何增长后跨帧保留。
//!   刻意不走 `map_async`：wasm 下没法阻塞等映射完成。
//! - **上传的是带 1px gutter 的矩形**（`atlas::GUTTER`，T5 渗色缺陷的修法）：
//!   CPU 像素按"边缘复制"扩一圈再落页，所以采样时线性过滤永远采到自己的边缘色。
//! - `Bitmap` 是 `Arc` 句柄，Clone 共享、最后一个 Drop 时向图集投递释放（页上
//!   最后一张图走 → 整页退役 → `Renderer::unregister_epoch`）。
//! - **布局与 GPU 资源分开**：`Atlas`（纯装箱，`Send`，解码线程也能碰）与
//!   `AtlasPages`（页纹理，只能在主线程/持有 device 的一侧）。wasm 后端的
//!   `wgpu::Texture` 不是 `Send`，混在一起会让 `AssetStore<Bitmap>` 的 `Send` 约束
//!   在 wasm 上直接失败。
//! - **渲染目标**（`Atlas::new_target`，T6）是同一条通路的第三种页：整页独占、无 CPU
//!   像素、纹理多一个 `RENDER_ATTACHMENT`。它的 GPU 视图仍只存在于
//!   `Renderer::epoch_views`，所以 `Bitmap` 在 wasm 上照样 `Send`（见 todo.md：这是与
//!   设计稿 `BitmapData::Rt{texture,view}` 的有意差异）。
//! - 与渲染器的同步点是应用侧每帧一次
//!   `renderer.apply_atlas(&atlas, &mut pages, &mut uploads)`（应用布局事件 + 上传），
//!   在 `App::draw` 里收集实例之前调用，引擎门面里再包装。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use crate::atlas::{Allocation, AtlasLayout, LayoutError, LayoutEvent, PageKind, Region};
use crate::renderer::Renderer;

/// 一页 = 一张 `Rgba8UnormSrgb` 纹理 + 它的默认 view。
///
/// RT 页额外要 `RENDER_ATTACHMENT`；格式三端统一，所以"画进位图"与"画进窗口"
/// 走的是同一条色彩通路（sRGB 编码由硬件负责）。
fn make_page(
    device: &wgpu::Device,
    label: String,
    width: u32,
    height: u32,
    kind: PageKind,
) -> (wgpu::Texture, wgpu::TextureView) {
    let mut usage = wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::COPY_DST
        | wgpu::TextureUsages::COPY_SRC;
    if kind == PageKind::Target {
        usage |= wgpu::TextureUsages::RENDER_ATTACHMENT;
    }
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    (texture, view)
}

/// 跨 `Bitmap` 共享的图集内核：**只有装箱状态**（GPU 页纹理在 [`AtlasPages`]），
/// 因此 `Send`，解码线程里 `new_bitmap` 也只碰它。
pub struct AtlasShared {
    layout: Mutex<AtlasLayout>,
    dropped: Mutex<Vec<u64>>,
}

impl AtlasShared {
    /// `Bitmap` Drop 投递：这个 epoch 可能可以退役了。
    fn notify_dropped(&self, epoch: u64) {
        self.dropped.lock().unwrap().push(epoch);
    }

    fn drain_dropped(&self) -> Vec<u64> {
        std::mem::take(&mut *self.dropped.lock().unwrap())
    }

    pub fn is_alive(&self, epoch: u64) -> bool {
        self.layout.lock().unwrap().is_alive(epoch)
    }

    pub fn page_size(&self, epoch: u64) -> Option<(u32, u32)> {
        self.layout.lock().unwrap().page_size(epoch)
    }
}

/// `epoch → 页纹理`。由持有 device 的一侧（应用/渲染器）持有，不进 `Atlas`。
#[derive(Default)]
pub struct AtlasPages {
    by_epoch: HashMap<u64, wgpu::Texture>,
}

impl AtlasPages {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, epoch: u64) -> Option<&wgpu::Texture> {
        self.by_epoch.get(&epoch)
    }

    pub fn len(&self) -> usize {
        self.by_epoch.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_epoch.is_empty()
    }

    /// 消费 Drop 投递与布局事件，建/毁页纹理。
    ///
    /// 返回 `(新建的 (epoch, view), 退役的 epoch)`，调用方据此换 bind group。布局状态
    /// 在 `atlas` 里（可能由解码线程并发更新），这里只负责把它翻译成 GPU 资源。
    pub fn synchronize(
        &mut self,
        device: &wgpu::Device,
        atlas: &Atlas,
    ) -> (Vec<(u64, wgpu::TextureView)>, Vec<u64>) {
        for epoch in atlas.shared.drain_dropped() {
            atlas.shared.layout.lock().unwrap().free(epoch);
        }
        let events = atlas.shared.layout.lock().unwrap().take_events();
        let mut created = Vec::new();
        let mut retired = Vec::new();
        for ev in events {
            match ev {
                LayoutEvent::PageAllocated {
                    epoch,
                    width,
                    height,
                    kind,
                } => {
                    let (texture, view) =
                        make_page(device, format!("sisf-atlas-{epoch}"), width, height, kind);
                    self.by_epoch.insert(epoch, texture);
                    created.push((epoch, view));
                }
                LayoutEvent::PageRetired { epoch } => {
                    self.by_epoch.remove(&epoch);
                    retired.push(epoch);
                }
            }
        }
        (created, retired)
    }
}

/// 应用侧图集句柄（可 Clone，共享同一 `AtlasShared`）。
#[derive(Clone)]
pub struct Atlas {
    shared: Arc<AtlasShared>,
}

impl Atlas {
    /// 页边长按桶取 2 的幂、最大 2048；最多 8 页在飞。
    pub fn new() -> Self {
        Self::with_limits(2048, 8)
    }

    pub fn with_limits(max_edge: u32, max_pages: usize) -> Self {
        Self {
            shared: Arc::new(AtlasShared {
                layout: Mutex::new(AtlasLayout::new(max_edge, max_pages)),
                dropped: Mutex::new(Vec::new()),
            }),
        }
    }

    /// 普通位图：装箱进分桶页（四周自动补 1px gutter）。`pixels` 必须是 premultiplied RGBA8。
    pub fn new_bitmap(
        &self,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
        now_frame: u64,
    ) -> Result<Bitmap, LayoutError> {
        let (epoch, alloc) = self
            .shared
            .layout
            .lock()
            .unwrap()
            .allocate(width, height, now_frame)?;
        Ok(self.finish_bitmap(epoch, width, height, pixels, alloc))
    }

    /// 大图旁路：超过桶的位图独占一页。
    pub fn new_large_bitmap(
        &self,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
        now_frame: u64,
    ) -> Result<Bitmap, LayoutError> {
        let (epoch, alloc) = self
            .shared
            .layout
            .lock()
            .unwrap()
            .allocate_large(width, height, now_frame)?;
        Ok(self.finish_bitmap(epoch, width, height, pixels, alloc))
    }

    /// **位图渲染目标**（RPG Maker 的 `Bitmap.new(w,h)`）：独占一整页、可被画。
    ///
    /// 没有 CPU 像素（`is_pending()` 恒 false，故永不进上传队列），内容只由
    /// `Renderer` 的绘制会话写入。页尺寸 = 请求尺寸（不抬到 2 的幂），所以
    /// `uv_rect()` 就是整个 `[0,1]²`。
    pub fn new_target(
        &self,
        width: u32,
        height: u32,
        now_frame: u64,
    ) -> Result<Bitmap, LayoutError> {
        let epoch = self
            .shared
            .layout
            .lock()
            .unwrap()
            .allocate_target(width, height, now_frame)?;
        let alloc = Allocation {
            region: Region {
                x: 0,
                y: 0,
                width,
                height,
            },
            upload: Region {
                x: 0,
                y: 0,
                width,
                height,
            },
        };
        Ok(self.finish_bitmap(epoch, width, height, Vec::new(), alloc))
    }

    fn finish_bitmap(
        &self,
        epoch: u64,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
        alloc: Allocation,
    ) -> Bitmap {
        debug_assert!(
            pixels.is_empty()
                || pixels.len()
                    == usize::try_from(u64::from(width) * u64::from(height) * 4).unwrap(),
            "像素缓冲必须是 width*height*4（空 = RT 页，没有 CPU 像素）"
        );
        let layout = self.shared.layout.lock().unwrap();
        Bitmap {
            inner: Arc::new(BitmapInner {
                shared: Arc::downgrade(&self.shared),
                epoch,
                kind: PageKind::of_epoch(epoch),
                rect: [alloc.region.x, alloc.region.y, width, height],
                upload: [
                    alloc.upload.x,
                    alloc.upload.y,
                    alloc.upload.width,
                    alloc.upload.height,
                ],
                page_size: layout.page_size(epoch).expect("刚分配的页一定在"),
                staging: Mutex::new(if pixels.is_empty() {
                    None
                } else {
                    Some(pixels)
                }),
            }),
        }
    }

    /// 本帧 draw 用到的 epoch → 刷新 LRU 时基。
    pub fn touch(&self, epoch: u64, now_frame: u64) {
        self.shared.layout.lock().unwrap().touch(epoch, now_frame);
    }

    pub fn live_pages(&self) -> usize {
        self.shared.layout.lock().unwrap().live_pages()
    }
}

impl Default for Atlas {
    fn default() -> Self {
        Self::new()
    }
}

struct BitmapInner {
    shared: Weak<AtlasShared>,
    epoch: u64,
    kind: PageKind,
    /// `[x, y, w, h]`，页内像素坐标（不含 gutter）。
    rect: [u32; 4],
    /// 含 gutter 的上传矩形 `[x, y, w, h]`；RT/旁路页与 `rect` 相同。
    upload: [u32; 4],
    page_size: (u32, u32),
    /// 上传前暂存的像素；`upload` 后置 None。
    staging: Mutex<Option<Vec<u8>>>,
}

/// 一张待上传/已上传的位图句柄。Clone 共享；最后 Drop 触发页退役。
pub struct Bitmap {
    inner: Arc<BitmapInner>,
}

impl Bitmap {
    /// 所属页的 epoch：填进 `BatchKey.epoch`，也是采样子矩形的定位键。
    pub fn epoch(&self) -> u64 {
        self.inner.epoch
    }

    pub fn is_large(&self) -> bool {
        self.inner.kind == PageKind::Large
    }

    /// 是否可作绘制目标（`Atlas::new_target` 造的页）。
    pub fn is_target(&self) -> bool {
        self.inner.kind == PageKind::Target
    }

    /// 页内矩形 `[x, y, w, h]`（采样用，不含 gutter）。
    pub fn rect(&self) -> [u32; 4] {
        self.inner.rect
    }

    /// 含 gutter 的上传矩形 `[x, y, w, h]`（GPU 侧写入范围，比 `rect` 大一圈）。
    pub fn upload_rect(&self) -> [u32; 4] {
        self.inner.upload
    }

    pub fn size(&self) -> (u32, u32) {
        (self.inner.rect[2], self.inner.rect[3])
    }

    pub fn page_size(&self) -> (u32, u32) {
        self.inner.page_size
    }

    /// 源矩形（uv 归一化的分母是页尺寸，图集语义下子矩形不可变）。
    pub fn src_rect(&self) -> Region {
        let [x, y, w, h] = self.inner.rect;
        Region {
            x,
            y,
            width: w,
            height: h,
        }
    }

    /// 采样用的归一化子矩形 `[u0, v0, u1, v1]`。
    pub fn uv_rect(&self) -> [f32; 4] {
        let [x, y, w, h] = self.inner.rect;
        let (pw, ph) = self.inner.page_size;
        let (pw, ph) = (pw as f32, ph as f32);
        [
            x as f32 / pw,
            y as f32 / ph,
            (x + w) as f32 / pw,
            (y + h) as f32 / ph,
        ]
    }

    /// 像素还在不在（上传后为 false）——门面上色前判断要不要排上传。
    pub fn is_pending(&self) -> bool {
        self.inner
            .staging
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|v| !v.is_empty())
    }
}

impl Clone for Bitmap {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

/// 手写而不是 derive：`staging` 是几十 KB 的像素，打出来只会淹掉日志。
impl std::fmt::Debug for Bitmap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bitmap")
            .field("epoch", &self.inner.epoch)
            .field("kind", &self.inner.kind)
            .field("rect", &self.inner.rect)
            .field("page_size", &self.inner.page_size)
            .field("pending", &self.is_pending())
            .finish()
    }
}

impl Drop for BitmapInner {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.upgrade() {
            shared.notify_dropped(self.epoch);
        }
    }
}

/// 一次 `copy_buffer_to_texture` 的目的地与它在 staging 里的布局。
struct CopyJob {
    bitmap: Bitmap,
    staging_offset: u64,
    bytes_per_row: u32,
}

/// 每帧单一 staging ring：CPU 摆齐 → 一次 `write_buffer` → 逐页 copy → 一次 submit。
pub struct UploadRing {
    /// CPU 侧行对齐暂存，容量跨帧保留。
    staging: Vec<u8>,
    buffer: Option<wgpu::Buffer>,
    capacity: u64,
    jobs: Vec<CopyJob>,
    bytes_used: u64,
}

const ROW_ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT; // 256
const MIN_STAGING: u64 = 1 << 16;
const PIX: usize = 4;

impl UploadRing {
    pub fn new() -> Self {
        Self {
            staging: Vec::new(),
            buffer: None,
            capacity: 0,
            jobs: Vec::new(),
            bytes_used: 0,
        }
    }

    /// 排入一个位图的上传（像素还躺在 `Bitmap` 的 staging 里）。
    ///
    /// 渲染目标没有 CPU 像素，排队即忽略（它的内容由绘制会话负责）。
    pub fn queue(&mut self, bitmap: Bitmap) {
        if !bitmap.is_pending() {
            return;
        }
        let [_, _, w, h] = bitmap.rect();
        let [_, _, uw, uh] = bitmap.upload_rect();
        debug_assert!(uw >= w && uh >= h, "上传矩形必须包住采样矩形");
        let bytes_per_row = (uw * PIX as u32).next_multiple_of(ROW_ALIGN);
        let size = u64::from(bytes_per_row) * u64::from(uh);
        let offset = self.bytes_used;
        self.bytes_used += size;
        self.jobs.push(CopyJob {
            bitmap,
            staging_offset: offset,
            bytes_per_row,
        });
    }

    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    pub fn pending(&self) -> usize {
        self.jobs.len()
    }

    /// 上传本帧排入的全部位图；排空后 `jobs` 归零（staging 容量留着复用）。
    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, pages: &AtlasPages) {
        if self.jobs.is_empty() {
            return;
        }
        // 每单尺寸都是 256 的倍数，`bytes_used` 天然满足 write_buffer 的 4 字节对齐。
        let needed = self.bytes_used;
        if needed > self.capacity {
            let mut cap = self.capacity.max(MIN_STAGING);
            while cap < needed {
                cap *= 2;
            }
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("sisf-atlas-staging"),
                size: cap,
                usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.capacity = cap;
        }
        let used = needed as usize;
        if self.staging.len() < used {
            self.staging.resize(used, 0);
        }
        for job in &self.jobs {
            job.write_rows(&mut self.staging);
        }
        let buffer = self.buffer.as_ref().expect("刚确保过容量");
        queue.write_buffer(buffer, 0, &self.staging[..used]);
        let mut encoder = device.create_command_encoder(&Default::default());
        for job in &self.jobs {
            let epoch = job.bitmap.epoch();
            let [x, y, w, h] = job.bitmap.upload_rect();
            let texture = pages
                .get(epoch)
                .unwrap_or_else(|| panic!("epoch {epoch} 没有页纹理（synchronize 没跑？）"));
            encoder.copy_buffer_to_texture(
                wgpu::TexelCopyBufferInfo {
                    buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: job.staging_offset,
                        bytes_per_row: Some(job.bytes_per_row),
                        rows_per_image: Some(h),
                    },
                },
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
        }
        queue.submit(std::iter::once(encoder.finish()));
        // GPU 已是权威副本，放掉 CPU 像素。
        for job in &self.jobs {
            *job.bitmap.inner.staging.lock().unwrap() = None;
        }
        self.jobs.clear();
        self.bytes_used = 0;
    }
}

impl CopyJob {
    /// 把 CPU 像素**按行**摊进 staging，顺带把 1px gutter 复制出来。
    ///
    /// 上传矩形比采样矩形宽/高各 `2*GUTTER`，多出来的那一圈取"最近的内层像素"
    /// （边缘复制），于是线性采样跨出子矩形时读到的是自己的边缘色而不是隔壁的图。
    fn write_rows(&self, staging: &mut [u8]) {
        let [ix, iy, w, h] = self.bitmap.rect();
        let [ux, uy, uw, uh] = self.bitmap.upload_rect();
        let guard = self.bitmap.inner.staging.lock().unwrap();
        let pixels = guard
            .as_ref()
            .expect("同一位图不允许二次上传（上传后像素已释放）");
        let row_bytes = usize::try_from(u64::from(uw) * PIX as u64).unwrap();
        debug_assert_eq!(
            pixels.len(),
            usize::try_from(u64::from(w) * u64::from(h) * PIX as u64).unwrap()
        );
        for row in 0..uh {
            // 目标行的绝对页行号夹回内层 [0,h)：上/下 gutter 行因此复制第一/最后一行。
            let sy = (uy + row).saturating_sub(iy).clamp(0, h.saturating_sub(1));
            let dst =
                usize::try_from(self.staging_offset + u64::from(row * self.bytes_per_row)).unwrap();
            let src_row = usize::try_from(u64::from(sy * w) * PIX as u64).unwrap();
            let (dst, src) = (&mut staging[dst..dst + row_bytes], &pixels[src_row..]);
            for x in 0..uw {
                // 同理按列夹回 [0,w)，左右 gutter 各复制本行的边缘像素。
                let sx = (ux + x).saturating_sub(ix).clamp(0, w.saturating_sub(1));
                let at = usize::try_from(u64::from(x) * PIX as u64).unwrap();
                let from = usize::try_from(u64::from(sx) * PIX as u64).unwrap();
                dst[at..at + PIX].copy_from_slice(&src[from..from + PIX]);
            }
        }
    }
}

impl Default for UploadRing {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    /// 应用图集的布局事件 + 本帧上传，每帧一次、放在收集实例之前。
    ///
    /// 三件事：消费 `Bitmap` Drop 投递 → 布局层 free（可能整页退役）；布局事件 →
    /// 建/毁页纹理并同步 epoch 的 bind group；待传位图走 staging ring 发出 copy。
    pub fn apply_atlas(&mut self, atlas: &Atlas, pages: &mut AtlasPages, uploads: &mut UploadRing) {
        let (created, retired) = pages.synchronize(&self.device, atlas);
        for (epoch, view) in created {
            self.register_epoch(epoch, view);
        }
        for epoch in retired {
            self.unregister_epoch(epoch);
        }
        uploads.upload(&self.device, &self.queue, pages);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
        vec![rgba; usize::try_from(u64::from(w) * u64::from(h)).unwrap()].concat()
    }

    /// 单测里没有渲染器，手动跑 `synchronize` 消费 Drop 的那半段。
    fn settle(atlas: &Atlas) {
        for epoch in atlas.shared.drain_dropped() {
            atlas.shared.layout.lock().unwrap().free(epoch);
        }
    }

    #[test]
    fn bitmap_drop_retires_page_and_reuses_epoch_slot() {
        let atlas = Atlas::with_limits(256, 1);
        let b = atlas
            .new_bitmap(100, 100, solid(100, 100, [255, 0, 0, 255]), 0)
            .unwrap();
        let e1 = b.epoch();
        assert_eq!(atlas.live_pages(), 1);
        drop(b);
        settle(&atlas);
        let b2 = atlas
            .new_bitmap(100, 100, solid(100, 100, [0, 255, 0, 255]), 1)
            .unwrap();
        assert_ne!(b2.epoch(), e1, "回收必须换 epoch");
        assert_eq!(atlas.live_pages(), 1, "回收不该加页");
    }

    #[test]
    fn clone_shares_and_last_drop_frees() {
        let atlas = Atlas::with_limits(256, 1);
        let b = atlas
            .new_bitmap(64, 64, solid(64, 64, [1, 2, 3, 255]), 0)
            .unwrap();
        let c = b.clone();
        let epoch = c.epoch();
        drop(b);
        settle(&atlas);
        assert!(atlas.shared.is_alive(epoch), "Clone 还活着，页不该退役");
        drop(c);
        settle(&atlas);
        assert!(!atlas.shared.is_alive(epoch));
    }

    #[test]
    fn queue_aligns_rows_to_wgpu_rule() {
        // 17×13 含 gutter 是 19×15：bytes_per_row 必须从 76 抬到 256 的倍数；
        // 第二单的偏移跟着上传矩形（不是采样矩形）走。
        let atlas = Atlas::with_limits(256, 2);
        let a = atlas
            .new_bitmap(17, 13, solid(17, 13, [255, 0, 0, 255]), 0)
            .unwrap();
        let b = atlas
            .new_bitmap(17, 13, solid(17, 13, [8, 8, 8, 255]), 0)
            .unwrap();
        assert_eq!(a.upload_rect(), [0, 0, 19, 15]);
        assert_eq!(a.rect(), [1, 1, 17, 13]);
        let mut uploads = UploadRing::new();
        uploads.queue(a);
        uploads.queue(b);
        assert_eq!(uploads.pending(), 2);
        assert_eq!(uploads.jobs[0].bytes_per_row, ROW_ALIGN);
        assert_eq!(uploads.jobs[1].staging_offset, u64::from(ROW_ALIGN) * 15);
        assert_eq!(uploads.bytes_used, u64::from(ROW_ALIGN) * 15 * 2);
    }

    /// 渲染目标没有 CPU 像素：既不该排上传，也不该被当成普通图混用。
    #[test]
    fn target_bitmaps_queue_nothing_and_report_their_kind() {
        let atlas = Atlas::with_limits(256, 2);
        let t = atlas.new_target(320, 200, 0).expect("RT 分配");
        assert!(t.is_target());
        assert!(!t.is_large());
        assert!(!t.is_pending(), "RT 没有 CPU 像素");
        assert_eq!(t.size(), (320, 200));
        assert_eq!(t.page_size(), (320, 200), "RT 页尺寸必须精确，否则 uv 会偏");
        assert_eq!(t.uv_rect(), [0.0, 0.0, 1.0, 1.0]);
        let mut uploads = UploadRing::new();
        uploads.queue(t.clone());
        assert_eq!(uploads.pending(), 0, "RT 排队即忽略");
        // RT 与分桶小页互不干扰。
        let b = atlas
            .new_bitmap(64, 64, solid(64, 64, [1, 2, 3, 255]), 0)
            .unwrap();
        assert_ne!(b.epoch(), t.epoch());
        assert!(!b.is_target());
    }

    #[test]
    fn uv_rect_normalizes_by_page_size() {
        let atlas = Atlas::with_limits(256, 1);
        let b = atlas
            .new_bitmap(64, 32, solid(64, 32, [1, 1, 1, 255]), 0)
            .unwrap();
        let [x, y, w, h] = b.rect();
        let (pw, ph) = b.page_size();
        let want = [
            x as f32 / pw as f32,
            y as f32 / ph as f32,
            (x + w) as f32 / pw as f32,
            (y + h) as f32 / ph as f32,
        ];
        for (got, e) in b.uv_rect().iter().zip(want) {
            assert!((got - e).abs() < 1e-6, "uv {got} 应为 {e}");
        }
        assert!(b.is_pending(), "没上传前像素该还在");
    }
}

/// 真实 GPU：证明非对齐尺寸上传后逐像素一致。
#[cfg(all(test, not(target_arch = "wasm32")))]
mod gpu_tests {
    use super::*;

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
                label: Some("sisf-atlas-test"),
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits(),
                ..Default::default()
            })
            .await
            .expect("request_device")
    }

    fn gradient(w: u32, h: u32) -> Vec<u8> {
        let mut px = Vec::with_capacity(usize::try_from(u64::from(w) * u64::from(h) * 4).unwrap());
        for y in 0..h {
            for x in 0..w {
                px.extend_from_slice(&[(x * 15) as u8, (y * 19) as u8, ((x + y) * 7) as u8, 255]);
            }
        }
        px
    }

    /// 读回整页，返回 `(bytes_per_row, 像素)`。
    fn read_page(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pages: &AtlasPages,
        epoch: u64,
    ) -> (u32, Vec<u8>) {
        let texture = pages.get(epoch).expect("页纹理").clone();
        let dim = texture.size();
        let pitch = (dim.width * 4).next_multiple_of(ROW_ALIGN);
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("atlas-readback"),
            size: u64::from(pitch) * u64::from(dim.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(pitch),
                    rows_per_image: Some(dim.height),
                },
            },
            dim,
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
        let data = slice.get_mapped_range().expect("读回映射").to_vec();
        readback.unmap();
        (pitch, data)
    }

    #[test]
    fn upload_17x13_unaligned_is_pixel_exact() {
        let (device, queue) = pollster::block_on(headless_device());
        let atlas = Atlas::with_limits(256, 4);
        let (w, h) = (17u32, 13u32);
        let pixels = gradient(w, h);
        let bitmap = atlas
            .new_bitmap(w, h, pixels.clone(), 0)
            .expect("17x13 该能装箱");
        let epoch = bitmap.epoch();

        // 走真实建页路径（不进渲染器，所以手动消费返回的 view）。
        let mut pages = AtlasPages::new();
        let (created, retired) = pages.synchronize(&device, &atlas);
        assert!(retired.is_empty());
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].0, epoch);

        let mut uploads = UploadRing::new();
        uploads.queue(bitmap.clone());
        uploads.upload(&device, &queue, &pages);
        assert_eq!(uploads.pending(), 0);
        assert!(!bitmap.is_pending(), "上传后 CPU 像素应释放给 GPU 独占");

        let [ix, iy, _, _] = bitmap.rect();
        let [ux, uy, uw, uh] = bitmap.upload_rect();
        let (pw, ph) = atlas.shared.page_size(epoch).expect("页还活着");
        let (pitch, data) = read_page(&device, &queue, &pages, epoch);
        let src = |x: u32, y: u32| {
            let i = usize::try_from(u64::from(y * w + x) * 4).unwrap();
            &pixels[i..i + 4]
        };
        for y in 0..ph {
            for x in 0..pw {
                let i = usize::try_from(u64::from(y * pitch + x * 4)).unwrap();
                let got = &data[i..i + 4];
                let in_inner = x >= ix && y >= iy && x - ix < w && y - iy < h;
                let in_upload = x >= ux && y >= uy && x - ux < uw && y - uy < uh;
                if in_inner {
                    assert_eq!(got, src(x - ix, y - iy), "像素 ({x},{y}) 不一致");
                } else if in_upload {
                    // gutter：夹回最近的内层像素。
                    let gx = (ux + (x - ux)).saturating_sub(ix).min(w - 1);
                    let gy = (uy + (y - uy)).saturating_sub(iy).min(h - 1);
                    assert_eq!(got, src(gx, gy), "gutter ({x},{y}) 没按边缘复制");
                } else {
                    assert_eq!(got, &[0, 0, 0, 0], "页空白区被写坏 ({x},{y})");
                }
            }
        }
    }

    /// 同页两张图：一张的 gutter 只能包含自己的边缘色，绝不与邻图内容混。
    #[test]
    fn upload_two_bitmaps_to_one_page_keeps_both() {
        let (device, queue) = pollster::block_on(headless_device());
        let atlas = Atlas::with_limits(256, 4);
        let a = atlas
            .new_bitmap(17, 13, gradient(17, 13), 0)
            .expect("装箱 a");
        let b = atlas.new_bitmap(23, 5, gradient(23, 5), 0).expect("装箱 b");
        assert_eq!(a.epoch(), b.epoch(), "同桶该落在同一页");
        let [px, py, bw, bh] = b.rect();
        let want = gradient(bw, bh);

        let mut pages = AtlasPages::new();
        pages.synchronize(&device, &atlas);
        let mut uploads = UploadRing::new();
        uploads.queue(a.clone());
        uploads.queue(b.clone());
        uploads.upload(&device, &queue, &pages);

        let epoch = b.epoch();
        let (pitch, data) = read_page(&device, &queue, &pages, epoch);
        for y in 0..bh {
            for x in 0..bw {
                let i = usize::try_from(u64::from((y + py) * pitch + (x + px) * 4)).unwrap();
                let src = usize::try_from(u64::from(y * bw + x) * 4).unwrap();
                assert_eq!(
                    &data[i..i + 4],
                    &want[src..src + 4],
                    "第二张图 ({x},{y}) 被覆盖"
                );
            }
        }
        // b 的左侧 gutter 列（px-1）必须等于 b 自己的第一列，而不是 a 的边缘。
        let [_, _, _, ah] = a.rect();
        let b_left_gutter = usize::try_from(u64::from(py * pitch + (px - 1) * 4)).unwrap();
        let b_first_col = usize::try_from(u64::from(py * pitch + px * 4)).unwrap();
        assert_eq!(
            &data[b_left_gutter..b_left_gutter + 4],
            &data[b_first_col..b_first_col + 4],
            "b 的左 gutter 复制的应是 b 自己的边缘"
        );
        // a 占据页首，它的右下角之外（既不在 a 也不在 b 里）仍是透明的。
        let [ax, ay, _, _] = a.rect();
        let outside = usize::try_from(u64::from((ay + ah + 3) * pitch + ax * 4)).unwrap();
        assert_eq!(
            &data[outside..outside + 4],
            &[0, 0, 0, 0],
            "两图之外不该被写"
        );
    }

    /// RT 页建出来必须真能当 render pass 目标（`RENDER_ATTACHMENT`），
    /// 且尺寸精确——否则"画进位图"这条通路在 golden 之前就断了。
    #[test]
    fn target_page_is_renderable_and_exact() {
        let (device, _queue) = pollster::block_on(headless_device());
        let atlas = Atlas::with_limits(256, 4);
        let rt = atlas.new_target(100, 60, 0).expect("RT 分配");
        let mut pages = AtlasPages::new();
        let (created, _) = pages.synchronize(&device, &atlas);
        assert_eq!(created.len(), 1);
        let texture = pages.get(rt.epoch()).expect("RT 页纹理");
        assert_eq!(texture.size().width, 100);
        assert_eq!(texture.size().height, 60);
        assert!(
            texture
                .usage()
                .contains(wgpu::TextureUsages::RENDER_ATTACHMENT),
            "RT 页必须可画，实际 usage={:?}",
            texture.usage()
        );
        // 直接开一个 pass 打清屏色，证明驱动接受这块纹理。
        let mut encoder = device.create_command_encoder(&Default::default());
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rt-probe"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &created[0].1,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        drop(pass);
        _queue.submit([encoder.finish()]);
    }
}
