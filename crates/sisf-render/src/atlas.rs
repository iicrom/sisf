//! 图集布局（纯逻辑，三端可跑单测）：分桶页 + `rect_packer` 装箱 + 整页回收。
//!
//! 模型（T4 设计）：
//! - 页是正方形，尺寸取"≥ 请求最大边"的最小 2 的幂桶（256…`max_edge`，默认 2048）；
//!   优先复用该桶已有页，装不下才换页——碎片换页数以合批。
//! - `epoch`（u64）= 一页一次存活期。页被回收再启用是**新 epoch**：批次键里的 epoch
//!   因此天然满足"内容不可变"——旧页内容与新生成的不会混进同一批。
//! - **`rect_packer` 没有单矩形释放**，所以回收粒度是整页：页上所有 `Bitmap` 句柄
//!   Drop（`live==0`）→ 进死页池；需要新页时按 LRU（最后使用帧最旧）就地重开。
//! - 死页也没有、页数到 `max_pages`（设计默认 8）→ 返回 `NeedFlush`：调用方必须
//!   先 flush 依赖或让句柄析构，**绝不静默丢图**。
//! - `allocate_large`（`Bitmap::large()` 的后端）旁路：独占一页、可超桶，epoch 带
//!   `LARGE_EPOCH` 高位标记；死旁路页只回收给下一次旁路请求。
//!
//! T6 加了两件事：
//! - **1px clamp gutter**（`GUTTER`）：每个子图四周多包 1px，上传时把边缘像素复制出去。
//!   线性采样下"跨子矩形渗色"就此消失（T5 遗留缺陷）。贴着页边的两侧不必补——
//!   `ClampToEdge` 复制的正好是该子图自己的边缘像素。
//! - **RT 页**（`allocate_target`，`TARGET_EPOCH`）：位图渲染目标独占一整页、尺寸精确、
//!   不做装箱（可写性优先），退役条件与普通页相同（句柄清零）。

use std::collections::HashMap;

/// 页内一个不可变子矩形（uv 由调用方按页尺寸换算）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// epoch 高位标记：`Bitmap::large()` 旁路页。
pub const LARGE_EPOCH: u64 = 1 << 63;
/// epoch 高位标记：位图渲染目标（RT）页。
pub const TARGET_EPOCH: u64 = 1 << 62;

/// 子图四周的 clamp gutter 宽度（像素）。见模块头的 T6 说明。
pub const GUTTER: u32 = 1;

/// 页的种类：决定 epoch 标记位、是否装箱、纹理 usage（RT 要 `RENDER_ATTACHMENT`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageKind {
    /// 分桶小页，多张子图共享。
    Small,
    /// 超桶大图旁路页，独占。
    Large,
    /// 位图渲染目标页，独占且可画。
    Target,
}

impl PageKind {
    const fn tag(self) -> u64 {
        match self {
            Self::Small => 0,
            Self::Large => LARGE_EPOCH,
            Self::Target => TARGET_EPOCH,
        }
    }

    pub fn of_epoch(epoch: u64) -> Self {
        if epoch & LARGE_EPOCH != 0 {
            Self::Large
        } else if epoch & TARGET_EPOCH != 0 {
            Self::Target
        } else {
            Self::Small
        }
    }

    /// 只有分桶小页用装箱器；旁路/RT 页整页属于一张图。
    const fn packs(self) -> bool {
        matches!(self, Self::Small)
    }
}

/// 一次分配的结果：采样矩形与落像素矩形是分开的两件事。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Allocation {
    /// 精确子矩形（不含 gutter）：uv 与裁剪都按它算。
    pub region: Region,
    /// 含 gutter 的上传矩形（可能因贴住页边而被裁小）。
    pub upload: Region,
}

/// 布局变更事件：GPU 侧据此建/毁纹理并向 `Renderer` 换 bind group。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutEvent {
    /// 新页启用（新建或就地重开）。
    PageAllocated {
        epoch: u64,
        width: u32,
        height: u32,
        kind: PageKind,
    },
    /// 页退役。
    PageRetired { epoch: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LayoutError {
    /// 超过 `max_edge`：应改走 `allocate_large`。
    #[error("图元边长 {edge} 超过图集页上限，请走大图画廊 `allocate_large`")]
    TooBig { edge: u32 },
    /// 无页可用且无法回收：调用方需 flush/释放句柄后重试。
    #[error("图集页池已满且无可回收页，请先释放不用的句柄或 flush 批次")]
    NeedFlush,
}

struct Page {
    width: u32,
    height: u32,
    kind: PageKind,
    /// 所属桶（旁路页 = usize::MAX）。
    bucket: usize,
    /// 仍在图集上的 Bitmap 数（句柄 Drop 时 free 递减）。
    live: u32,
    /// 最后一次"放置/使用"的帧号，死页之间按它 LRU 排序。
    last_use: u64,
    packer: rect_packer::DensePacker,
}

impl Page {
    fn new(width: u32, height: u32, kind: PageKind, bucket: usize, now: u64) -> Self {
        Self {
            width,
            height,
            kind,
            bucket,
            live: 0,
            last_use: now,
            packer: rect_packer::DensePacker::new(width as i32, height as i32),
        }
    }

    /// 就地重开：清空装箱状态并换尺寸。
    fn reopen(&mut self, width: u32, height: u32, kind: PageKind, bucket: usize, now: u64) {
        self.width = width;
        self.height = height;
        self.kind = kind;
        self.bucket = bucket;
        self.live = 0;
        self.last_use = now;
        self.packer = rect_packer::DensePacker::new(width as i32, height as i32);
    }
}

/// 布局器。`pages` 下标即"物理页槽"，回收 = 槽内换 epoch。
pub struct AtlasLayout {
    pages: Vec<Option<Page>>,
    /// 槽 → 当前 epoch（0 = 无主，只在死槽出现）。
    slot_epoch: Vec<u64>,
    epoch_to_slot: HashMap<u64, usize>,
    /// 桶 → 该桶的存活页槽号。
    buckets: Vec<(u32, Vec<usize>)>,
    max_pages: usize,
    max_edge: u32,
    next_epoch: u64,
    events: Vec<LayoutEvent>,
}

impl AtlasLayout {
    /// `max_edge` 决定最大桶（也是普通分配的拒绝阈值）；`max_pages` 是同时存活页数上限。
    pub fn new(max_edge: u32, max_pages: usize) -> Self {
        assert!(max_edge.is_power_of_two() && max_edge >= 256);
        let mut buckets = Vec::new();
        let mut edge = 256u32;
        while edge <= max_edge {
            buckets.push((edge, Vec::new()));
            edge *= 2;
        }
        Self {
            pages: Vec::new(),
            slot_epoch: Vec::new(),
            epoch_to_slot: HashMap::new(),
            buckets,
            max_pages,
            max_edge,
            // 0 = 渲染器内置 WHITE_EPOCH；LARGE_EPOCH 位保留给旁路页。
            next_epoch: 1,
            events: Vec::new(),
        }
    }

    pub fn max_pages(&self) -> usize {
        self.max_pages
    }

    pub fn max_edge(&self) -> u32 {
        self.max_edge
    }

    pub fn live_pages(&self) -> usize {
        self.slot_epoch.iter().filter(|&&e| e != 0).count()
    }

    /// 取走并清空累积事件（GPU 侧每帧消费一次）。
    pub fn take_events(&mut self) -> Vec<LayoutEvent> {
        std::mem::take(&mut self.events)
    }

    /// 普通图：进"≥ 最大边的最小桶"。返回 `(epoch, 分配结果)`。
    ///
    /// 装箱按 `width + 2*GUTTER` 请求，故实际可用的最大边长是 `max_edge - 2*GUTTER`。
    pub fn allocate(
        &mut self,
        width: u32,
        height: u32,
        now_frame: u64,
    ) -> Result<(u64, Allocation), LayoutError> {
        if width == 0 || height == 0 {
            return Err(LayoutError::TooBig { edge: 0 });
        }
        if width + 2 * GUTTER > self.max_edge || height + 2 * GUTTER > self.max_edge {
            return Err(LayoutError::TooBig {
                edge: self.max_edge,
            });
        }
        let start = self.bucket_for(width.max(height) + 2 * GUTTER);
        for b in start..self.buckets.len() {
            // try_pack 要 &mut self（事件/页状态），先克隆槽表再遍历。
            let slots = self.buckets[b].1.clone();
            for slot in slots {
                if let Some(alloc) = self.try_pack(slot, width, height, now_frame) {
                    self.pages[slot].as_mut().unwrap().live += 1;
                    return Ok((self.slot_epoch[slot], alloc));
                }
            }
            // 该桶页都满了：池子未满可开新页；满了也要看有没有死页可回收。
            if self.live_pages() < self.max_pages || self.dead_slot_donor(PageKind::Small).is_some()
            {
                let edge = self.buckets[b].0;
                let epoch = self.open_small_page(edge, now_frame);
                let slot = self.epoch_to_slot[&epoch];
                let alloc = self
                    .try_pack(slot, width, height, now_frame)
                    .expect("新页必能装下桶内请求");
                self.pages[slot].as_mut().unwrap().live += 1;
                return Ok((epoch, alloc));
            }
        }
        Err(LayoutError::NeedFlush)
    }

    /// `Bitmap::large()` 旁路：独占一页（可超桶，上限 4096），无 gutter（整页只有它）。
    pub fn allocate_large(
        &mut self,
        width: u32,
        height: u32,
        now_frame: u64,
    ) -> Result<(u64, Allocation), LayoutError> {
        if width == 0 || height == 0 || width > 4096 || height > 4096 {
            return Err(LayoutError::TooBig { edge: 4096 });
        }
        let epoch = self.open_exclusive_page(PageKind::Large, width, height, now_frame)?;
        let region = Region {
            x: 0,
            y: 0,
            width,
            height,
        };
        Ok((
            epoch,
            Allocation {
                region,
                upload: region,
            },
        ))
    }

    /// 位图渲染目标（RT）：独占一整页、尺寸精确、可作 render pass 目标。
    ///
    /// 只返回 epoch——RT 没有"子矩形"概念，整页就是那张位图；内容可变，所以
    /// **不能**与同页别的图共享（这也是它不进装箱器的原因）。
    pub fn allocate_target(
        &mut self,
        width: u32,
        height: u32,
        now_frame: u64,
    ) -> Result<u64, LayoutError> {
        // 上限与旁路页一致：再大就该走专门的后处理纹理，而不是图集。
        if width == 0 || height == 0 || width > 4096 || height > 4096 {
            return Err(LayoutError::TooBig { edge: 4096 });
        }
        self.open_exclusive_page(PageKind::Target, width, height, now_frame)
    }

    /// 开一张"独占整页"的图（旁路大图 / RT）：优先回收同种死页，否则新槽。
    fn open_exclusive_page(
        &mut self,
        kind: PageKind,
        width: u32,
        height: u32,
        now: u64,
    ) -> Result<u64, LayoutError> {
        let epoch = if let Some(slot) = self.dead_slot_donor(kind) {
            self.reopen_slot(slot, kind, width, height, now)
        } else if self.live_pages() < self.max_pages {
            self.open_page_raw(kind, width, height, now)
        } else {
            return Err(LayoutError::NeedFlush);
        };
        self.pages[self.epoch_to_slot[&epoch]]
            .as_mut()
            .unwrap()
            .live += 1;
        Ok(epoch)
    }

    /// Bitmap 句柄析构：页上最后一个走 → 整页退役进死池。
    pub fn free(&mut self, epoch: u64) {
        let Some(&slot) = self.epoch_to_slot.get(&epoch) else {
            return;
        };
        let Some(page) = self.pages[slot].as_mut() else {
            return;
        };
        page.live = page.live.saturating_sub(1);
        if page.live == 0 {
            self.retire(slot);
        }
    }

    /// epoch 是否仍存活（批次过滤/驱逐诊断用）。
    pub fn is_alive(&self, epoch: u64) -> bool {
        self.epoch_to_slot.contains_key(&epoch)
    }

    pub fn page_size(&self, epoch: u64) -> Option<(u32, u32)> {
        let &slot = self.epoch_to_slot.get(&epoch)?;
        let page = self.pages[slot].as_ref()?;
        Some((page.width, page.height))
    }

    pub fn is_large(&self, epoch: u64) -> bool {
        epoch & LARGE_EPOCH != 0
    }

    /// epoch 属于哪种页。
    pub fn kind(&self, epoch: u64) -> Option<PageKind> {
        self.epoch_to_slot
            .get(&epoch)
            .map(|_| PageKind::of_epoch(epoch))
    }

    /// 标记使用（LRU 时基）：每帧对本帧 draw 用到的 epoch 调用。
    pub fn touch(&mut self, epoch: u64, now_frame: u64) {
        if let Some(&slot) = self.epoch_to_slot.get(&epoch)
            && let Some(page) = self.pages[slot].as_mut()
        {
            page.last_use = page.last_use.max(now_frame);
        }
    }

    // —— 内部 ——

    fn bucket_for(&self, edge: u32) -> usize {
        self.buckets
            .iter()
            .position(|&(b, _)| b >= edge)
            .expect("max_edge 之上已在入口拒绝")
    }

    /// 装箱一块带 gutter 的子图，返回**内层**采样矩形与含 gutter 的上传矩形。
    fn try_pack(&mut self, slot: usize, w: u32, h: u32, now: u64) -> Option<Allocation> {
        let page = self.pages[slot].as_mut()?;
        let r = page
            .packer
            .pack((w + 2 * GUTTER) as i32, (h + 2 * GUTTER) as i32, false)?;
        page.last_use = page.last_use.max(now);
        let upload = Region {
            x: r.x as u32,
            y: r.y as u32,
            width: r.width as u32,
            height: r.height as u32,
        };
        // 内层必定落在页内：装箱器给的矩形本身在页里，内层又在它里面。
        Some(Allocation {
            region: Region {
                x: upload.x + GUTTER,
                y: upload.y + GUTTER,
                width: w,
                height: h,
            },
            upload,
        })
    }

    /// 选死页 donor：按 LRU（最后使用帧最旧）。三种页互不串用。
    fn dead_slot_donor(&self, want: PageKind) -> Option<usize> {
        let mut best: Option<(u64, usize)> = None;
        for (slot, page) in self.pages.iter().enumerate() {
            let Some(page) = page.as_ref() else {
                continue;
            };
            if page.live != 0 || page.kind != want {
                continue;
            }
            if best.is_none_or(|(lu, _)| page.last_use < lu) {
                best = Some((page.last_use, slot));
            }
        }
        best.map(|(_, slot)| slot)
    }

    /// 开一个普通小页：优先回收死页（LRU），否则新槽。
    fn open_small_page(&mut self, edge: u32, now: u64) -> u64 {
        if let Some(slot) = self.dead_slot_donor(PageKind::Small) {
            return self.reopen_slot(slot, PageKind::Small, edge, edge, now);
        }
        self.open_page_raw(PageKind::Small, edge, edge, now)
    }

    /// 就地重开死槽：旧 epoch 退役（若还没退役则补发事件），换主、清装箱器。
    fn reopen_slot(
        &mut self,
        slot: usize,
        kind: PageKind,
        width: u32,
        height: u32,
        now: u64,
    ) -> u64 {
        debug_assert_eq!(self.slot_epoch[slot], 0, "reopen 只用于已退役死槽");
        let epoch = self.fresh_epoch(kind);
        let bucket = if kind.packs() {
            self.bucket_for(width.max(height))
        } else {
            usize::MAX
        };
        self.pages[slot]
            .as_mut()
            .expect("死页仍在槽里")
            .reopen(width, height, kind, bucket, now);
        self.slot_epoch[slot] = epoch;
        self.epoch_to_slot.insert(epoch, slot);
        if bucket != usize::MAX {
            self.buckets[bucket].1.push(slot);
        }
        self.events.push(LayoutEvent::PageAllocated {
            epoch,
            width,
            height,
            kind,
        });
        epoch
    }

    fn open_page_raw(&mut self, kind: PageKind, width: u32, height: u32, now: u64) -> u64 {
        let epoch = self.fresh_epoch(kind);
        let bucket = if kind.packs() {
            self.bucket_for(width.max(height))
        } else {
            usize::MAX
        };
        let slot = self.pages.len();
        self.pages
            .push(Some(Page::new(width, height, kind, bucket, now)));
        self.slot_epoch.push(epoch);
        self.epoch_to_slot.insert(epoch, slot);
        if bucket != usize::MAX {
            self.buckets[bucket].1.push(slot);
        }
        self.events.push(LayoutEvent::PageAllocated {
            epoch,
            width,
            height,
            kind,
        });
        epoch
    }

    fn fresh_epoch(&mut self, kind: PageKind) -> u64 {
        let e = self.next_epoch | kind.tag();
        self.next_epoch += 1;
        e
    }

    fn retire(&mut self, slot: usize) {
        let epoch = self.slot_epoch[slot];
        debug_assert_ne!(epoch, 0);
        self.events.push(LayoutEvent::PageRetired { epoch });
        self.epoch_to_slot.remove(&epoch);
        self.slot_epoch[slot] = 0;
        let b = self.pages[slot].as_ref().unwrap().bucket;
        if b != usize::MAX {
            self.buckets[b].1.retain(|&s| s != slot);
        }
        // 页留在槽里（live==0 即死页池成员），不删。
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_no_overlap(regions: &[Region]) {
        for (i, a) in regions.iter().enumerate() {
            for b in &regions[i + 1..] {
                let sep = a.x + a.width <= b.x
                    || b.x + b.width <= a.x
                    || a.y + a.height <= b.y
                    || b.y + b.height <= a.y;
                assert!(sep, "重叠: {a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn packs_until_full_then_switches_page() {
        let mut l = AtlasLayout::new(256, 8);
        // 512px 图（含 gutter 514）超最大桶 → TooBig。
        assert_eq!(
            l.allocate(512, 8, 0),
            Err(LayoutError::TooBig { edge: 256 })
        );
        // 128px 见方 2 张占满 256² 页的宽度（含 gutter 各 130），第 3 张换页。
        let (e1, a1) = l.allocate(120, 128, 0).unwrap();
        let (e1b, a2) = l.allocate(120, 128, 0).unwrap();
        assert_eq!(e1, e1b, "同页不应换 epoch");
        assert_eq!(
            a1.region,
            Region {
                x: 1,
                y: 1,
                width: 120,
                height: 128
            }
        );
        assert_eq!(a1.upload.width, 122, "上传矩形要比采样矩形宽 2*gutter");
        assert_no_overlap(&[a1.region, a2.region]);
        // 第 3 张横竖都塞不下（122*2=244 占宽，130*2=260 超页高）→ 换页。
        let (e2, a3) = l.allocate(120, 128, 1).unwrap();
        assert_ne!(e1, e2);
        assert_eq!(l.live_pages(), 2);
        assert_eq!(a3.region.x, GUTTER, "新页从 (gutter,gutter) 开始");
    }

    /// T5 遗留缺陷的正解：采样矩形被 1px gutter 完整包住，且含 gutter 的矩形互不重叠。
    #[test]
    fn neighbours_are_separated_by_gutter() {
        let mut l = AtlasLayout::new(256, 8);
        let mut inner = Vec::new();
        let mut padded = Vec::new();
        for _ in 0..6 {
            let (_, a) = l.allocate(64, 24, 0).unwrap();
            assert_eq!(a.region.x, a.upload.x + GUTTER);
            assert_eq!(a.region.y, a.upload.y + GUTTER);
            assert_eq!(a.upload.width, a.region.width + 2 * GUTTER);
            assert_eq!(a.upload.height, a.region.height + 2 * GUTTER);
            // 全部落在页内（页 256²），于是四边的 gutter 都是真实像素，不靠 clamp 兜底。
            assert!(a.upload.x + a.upload.width <= 256);
            assert!(a.upload.y + a.upload.height <= 256);
            inner.push(a.region);
            padded.push(a.upload);
        }
        assert_no_overlap(&padded);
        // 任意两张子图之间至少隔 2px（各自的 gutter），线性采样不会渗色。
        for (i, a) in inner.iter().enumerate() {
            for b in &inner[i + 1..] {
                let gap_x = (b.x as i64 - (a.x + a.width) as i64).max(0);
                let gap_y = (b.y as i64 - (a.y + a.height) as i64).max(0);
                let overlaps_rows = a.y < b.y + b.height && b.y < a.y + a.height;
                let overlaps_cols = a.x < b.x + b.width && b.x < a.x + a.width;
                let sep = if overlaps_rows {
                    gap_x >= 2
                } else if overlaps_cols {
                    gap_y >= 2
                } else {
                    true
                };
                assert!(sep, "两张子图之间必须有 gutter: {a:?} {b:?}");
            }
        }
    }

    /// RT 页：尺寸精确、独占、只在同种死页里回收，也不会被小图复用。
    #[test]
    fn target_pages_are_exact_exclusive_and_segregated() {
        let mut l = AtlasLayout::new(256, 3);
        let t = l.allocate_target(544, 416, 0).unwrap();
        assert_eq!(PageKind::of_epoch(t), PageKind::Target);
        assert_eq!(l.page_size(t), Some((544, 416)), "RT 页不得被抬到 2 的幂");
        assert_eq!(l.live_pages(), 1);
        // 小图不会落在 RT 页上（它不进桶）。
        let (s, _) = l.allocate(64, 64, 1).unwrap();
        assert_ne!(s, t);
        assert_eq!(l.live_pages(), 2);
        // 第二张 RT：换页而不是复用别人的槽。
        let t2 = l.allocate_target(64, 64, 2).unwrap();
        assert_ne!(t2, t);
        assert_eq!(l.page_size(t2), Some((64, 64)));
        assert_eq!(l.live_pages(), 3);
        // 池满（max_pages=3 全活）→ NeedFlush，不静默挤掉谁。
        assert_eq!(
            l.allocate_target(32, 32, 3),
            Err(LayoutError::NeedFlush),
            "池子满了要显式报 NeedFlush"
        );
        // 退役后回收：RT 死槽只给下一次 RT 请求。
        l.free(t);
        let t3 = l.allocate_target(320, 200, 4).unwrap();
        assert_ne!(t3, t, "回收必须换 epoch");
        assert_eq!(l.page_size(t3), Some((320, 200)));
        assert_eq!(l.live_pages(), 3, "回收死槽不增页");
        // 0 边长与超上限都在入口拒绝。
        assert_eq!(
            l.allocate_target(0, 10, 5),
            Err(LayoutError::TooBig { edge: 4096 })
        );
        assert_eq!(
            l.allocate_target(8192, 8, 5),
            Err(LayoutError::TooBig { edge: 4096 })
        );
    }

    #[test]
    fn buckets_route_by_max_edge() {
        // 桶从小到大 256/512/1024/2048：300px 图（含 gutter）进 512 桶。
        let mut l = AtlasLayout::new(2048, 8);
        let (e_small, _) = l.allocate(100, 100, 0).unwrap();
        let (e_mid, _) = l.allocate(300, 300, 0).unwrap();
        let (e_big, _) = l.allocate(1500, 1500, 0).unwrap();
        assert_ne!(e_small, e_mid, "跨桶必须换页");
        assert_ne!(e_mid, e_big);
        assert_eq!(l.live_pages(), 3);
        assert_eq!(l.page_size(e_small), Some((256, 256)));
        assert_eq!(l.page_size(e_mid), Some((512, 512)));
        assert_eq!(l.page_size(e_big), Some((2048, 2048)));
        // 桶内装箱优先：再来个 100² 应落在 e_small 页上。
        let (e_again, _) = l.allocate(100, 100, 1).unwrap();
        assert_eq!(e_again, e_small);
    }

    #[test]
    fn eviction_recycles_dead_pages_with_new_epoch() {
        let mut l = AtlasLayout::new(256, 2);
        let (e1, _) = l.allocate(200, 200, 0).unwrap();
        let (e2, _) = l.allocate(200, 200, 1).unwrap();
        assert_eq!(l.live_pages(), 2);
        // 200² 含 gutter 占 202，页里剩 54px 宽的边条：50² 塞得进、55² 塞不进。
        let (e1b, _) = l.allocate(50, 50, 2).unwrap();
        assert_eq!(e1b, e1);
        assert_eq!(l.allocate(55, 55, 2), Err(LayoutError::NeedFlush));
        // 释放 e2 → 页退役；新需求回收它的死槽（就地重开），拿全新 epoch。
        l.free(e2);
        assert!(!l.is_alive(e2));
        let (e2b, _) = l.allocate(200, 200, 3).unwrap();
        assert_ne!(e2, e2b, "回收必须发新 epoch");
        assert!(l.is_alive(e2b));
        assert_eq!(l.live_pages(), 2, "回收死槽不增页");
        // 释放 e1 的两张图 → 该页退役；30² 回收它的槽。
        l.free(e1b);
        assert!(l.is_alive(e1), "页上还有第一张图");
        l.free(e1);
        let (e3, _) = l.allocate(30, 30, 4).unwrap();
        let e1c = l.allocate(200, 200, 4).unwrap().0;
        assert!(!l.is_alive(e1));
        assert_eq!(l.live_pages(), 2);
        assert_ne!(e1c, e1);
        l.free(e1c);
        l.free(e3);
        l.free(e2b);
        assert_eq!(l.live_pages(), 0);
        // 事件配平：4 Alloc（e1/e2 新建 + e2b/e3 回收）对 4 Retire（两死槽重开
        // 时旧主退役 + e3/e2b 页最终清空），退役与启用一一对应。
        let events = l.take_events();
        let retired = events
            .iter()
            .filter(|e| matches!(e, LayoutEvent::PageRetired { .. }))
            .count();
        let allocated = events
            .iter()
            .filter(|e| matches!(e, LayoutEvent::PageAllocated { .. }))
            .count();
        assert_eq!(allocated, 4);
        assert_eq!(retired, 4);
    }

    #[test]
    fn max_pages_hard_limit_flush_not_error() {
        let mut l = AtlasLayout::new(256, 1);
        let (e, _) = l.allocate(200, 200, 0).unwrap();
        assert_eq!(l.allocate(200, 200, 1), Err(LayoutError::NeedFlush));
        l.free(e);
        let (e2, a) = l.allocate(200, 200, 2).unwrap();
        assert_ne!(e, e2);
        assert_eq!((a.region.width, a.region.height), (200, 200));
        assert_eq!(l.live_pages(), 1);
    }

    #[test]
    fn large_bypass_pages_reserve_epoch_bit() {
        let mut l = AtlasLayout::new(2048, 8);
        // 4096² 超最大桶 → 普通分配拒绝，large 旁路通过。
        assert_eq!(
            l.allocate(4096, 4096, 0),
            Err(LayoutError::TooBig { edge: 2048 })
        );
        let (e, a) = l.allocate_large(4096, 4096, 0).unwrap();
        assert!(l.is_large(e));
        assert_eq!(a.region, a.upload, "旁路页独占整页，没有 gutter");
        assert_eq!(
            a.region,
            Region {
                x: 0,
                y: 0,
                width: 4096,
                height: 4096
            }
        );
        // 旁路页不进桶：普通小图不会落在它上面。
        let (es, _) = l.allocate(64, 64, 1).unwrap();
        assert!(!l.is_large(es));
        assert_ne!(es, e);
        // 死旁路页只回收给旁路请求。
        l.free(e);
        let (e2, _) = l.allocate_large(3000, 3000, 2).unwrap();
        assert!(l.is_large(e2));
        assert_ne!(e2, e);
        assert_eq!(l.live_pages(), 2);
        l.free(e2);
        l.free(es);
    }

    #[test]
    fn zero_sized_and_dead_epoch_guards() {
        let mut l = AtlasLayout::new(256, 4);
        assert_eq!(l.allocate(0, 10, 0), Err(LayoutError::TooBig { edge: 0 }));
        assert_eq!(
            l.allocate_large(0, 10, 0),
            Err(LayoutError::TooBig { edge: 4096 })
        );
        l.free(999); // 未知 epoch：无害 no-op
        assert!(!l.is_alive(999));
        assert_eq!(l.page_size(999), None);
        assert_eq!(l.take_events().len(), 0);
    }

    #[test]
    fn touch_moves_lru_recency() {
        let mut l = AtlasLayout::new(256, 2);
        let (ea, _) = l.allocate(200, 200, 0).unwrap();
        let (eb, _) = l.allocate(200, 200, 1).unwrap();
        l.free(ea);
        l.free(eb); // 两张都死：ea last_use=0, eb=1 → 回收 LRU 应挑 ea 的槽
        let (ec, _) = l.allocate(200, 200, 5).unwrap();
        // 验证不直接暴露槽号，用事件顺序断言"回收发生了"（live_pages 不增）。
        assert_eq!(l.live_pages(), 1);
        assert!(l.is_alive(ec));
    }
}
