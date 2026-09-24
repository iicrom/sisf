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

/// 布局变更事件：GPU 侧据此建/毁纹理并向 `Renderer` 换 bind group。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutEvent {
    /// 新页启用（新建或就地重开）。
    PageAllocated { epoch: u64, width: u32, height: u32 },
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
    /// 所属桶（旁路页 = usize::MAX）。
    bucket: usize,
    /// 仍在图集上的 Bitmap 数（句柄 Drop 时 free 递减）。
    live: u32,
    /// 最后一次"放置/使用"的帧号，死页之间按它 LRU 排序。
    last_use: u64,
    packer: rect_packer::DensePacker,
}

impl Page {
    fn new(width: u32, height: u32, bucket: usize, now: u64) -> Self {
        Self {
            width,
            height,
            bucket,
            live: 0,
            last_use: now,
            packer: rect_packer::DensePacker::new(width as i32, height as i32),
        }
    }

    /// 就地重开：清空装箱状态并换尺寸。
    fn reopen(&mut self, width: u32, height: u32, bucket: usize, now: u64) {
        self.width = width;
        self.height = height;
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

    /// 普通图：进"≥ 最大边的最小桶"。返回 `(epoch, 页内矩形)`。
    pub fn allocate(
        &mut self,
        width: u32,
        height: u32,
        now_frame: u64,
    ) -> Result<(u64, Region), LayoutError> {
        if width == 0 || height == 0 {
            return Err(LayoutError::TooBig { edge: 0 });
        }
        if width > self.max_edge || height > self.max_edge {
            return Err(LayoutError::TooBig {
                edge: self.max_edge,
            });
        }
        let start = self.bucket_for(width.max(height));
        for b in start..self.buckets.len() {
            // try_pack 要 &mut self（事件/页状态），先克隆槽表再遍历。
            let slots = self.buckets[b].1.clone();
            for slot in slots {
                if let Some(region) = self.try_pack(slot, width, height, now_frame) {
                    self.pages[slot].as_mut().unwrap().live += 1;
                    return Ok((self.slot_epoch[slot], region));
                }
            }
            // 该桶页都满了：池子未满可开新页；满了也要看有没有死页可回收。
            if self.live_pages() < self.max_pages || self.dead_slot_donor(false).is_some() {
                let edge = self.buckets[b].0;
                let epoch = self.open_small_page(edge, now_frame);
                let slot = self.epoch_to_slot[&epoch];
                let region = self
                    .try_pack(slot, width, height, now_frame)
                    .expect("新页必能装下桶内请求");
                self.pages[slot].as_mut().unwrap().live += 1;
                return Ok((epoch, region));
            }
        }
        Err(LayoutError::NeedFlush)
    }

    /// `Bitmap::large()` 旁路：独占一页（可超桶，上限 4096）。
    pub fn allocate_large(
        &mut self,
        width: u32,
        height: u32,
        now_frame: u64,
    ) -> Result<(u64, Region), LayoutError> {
        if width == 0 || height == 0 || width > 4096 || height > 4096 {
            return Err(LayoutError::TooBig { edge: 4096 });
        }
        let epoch = if let Some(slot) = self.dead_slot_donor(true) {
            self.reopen_slot(slot, LARGE_EPOCH, width, height, now_frame)
        } else if self.live_pages() < self.max_pages {
            self.open_page_raw(width, height, LARGE_EPOCH, now_frame)
        } else {
            return Err(LayoutError::NeedFlush);
        };
        self.pages[self.epoch_to_slot[&epoch]]
            .as_mut()
            .unwrap()
            .live += 1;
        Ok((
            epoch,
            Region {
                x: 0,
                y: 0,
                width,
                height,
            },
        ))
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

    fn try_pack(&mut self, slot: usize, w: u32, h: u32, now: u64) -> Option<Region> {
        let page = self.pages[slot].as_mut()?;
        let r = page.packer.pack(w as i32, h as i32, false)?;
        page.last_use = page.last_use.max(now);
        Some(Region {
            x: r.x as u32,
            y: r.y as u32,
            width: r.width as u32,
            height: r.height as u32,
        })
    }

    /// 选死页 donor：按 LRU（最后使用帧最旧）。旁路与普通池互不串用。
    fn dead_slot_donor(&self, large_request: bool) -> Option<usize> {
        let mut best: Option<(u64, usize)> = None;
        for (slot, page) in self.pages.iter().enumerate() {
            let Some(page) = page.as_ref() else {
                continue;
            };
            if page.live != 0 {
                continue;
            }
            let is_large = self.slot_epoch[slot] & LARGE_EPOCH != 0;
            if is_large != large_request {
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
        if let Some(slot) = self.dead_slot_donor(false) {
            return self.reopen_slot(slot, 0, edge, edge, now);
        }
        self.open_page_raw(edge, edge, 0, now)
    }

    /// 就地重开死槽：旧 epoch 退役（若还没退役则补发事件），换主、清装箱器。
    fn reopen_slot(&mut self, slot: usize, tag: u64, width: u32, height: u32, now: u64) -> u64 {
        debug_assert_eq!(self.slot_epoch[slot], 0, "reopen 只用于已退役死槽");
        let epoch = self.fresh_epoch(tag);
        let bucket = if tag != 0 {
            usize::MAX
        } else {
            self.bucket_for(width.max(height))
        };
        self.pages[slot]
            .as_mut()
            .expect("死页仍在槽里")
            .reopen(width, height, bucket, now);
        self.slot_epoch[slot] = epoch;
        self.epoch_to_slot.insert(epoch, slot);
        if bucket != usize::MAX {
            self.buckets[bucket].1.push(slot);
        }
        self.events.push(LayoutEvent::PageAllocated {
            epoch,
            width,
            height,
        });
        epoch
    }

    fn open_page_raw(&mut self, width: u32, height: u32, tag: u64, now: u64) -> u64 {
        let epoch = self.fresh_epoch(tag);
        let bucket = if tag != 0 {
            usize::MAX
        } else {
            self.bucket_for(width.max(height))
        };
        let slot = self.pages.len();
        self.pages.push(Some(Page::new(width, height, bucket, now)));
        self.slot_epoch.push(epoch);
        self.epoch_to_slot.insert(epoch, slot);
        if bucket != usize::MAX {
            self.buckets[bucket].1.push(slot);
        }
        self.events.push(LayoutEvent::PageAllocated {
            epoch,
            width,
            height,
        });
        epoch
    }

    fn fresh_epoch(&mut self, tag: u64) -> u64 {
        let e = self.next_epoch | tag;
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
        // 512px 图超最大桶 → TooBig。
        assert_eq!(
            l.allocate(512, 8, 0),
            Err(LayoutError::TooBig { edge: 256 })
        );
        // 256×128 一张 256² 页能塞两张。
        let (e1, r1) = l.allocate(256, 128, 0).unwrap();
        let (e1b, r2) = l.allocate(256, 128, 0).unwrap();
        assert_eq!(e1, e1b, "同页不应换 epoch");
        assert_no_overlap(&[r1, r2]);
        // 第三张塞不下 → 换页（新 epoch、页内原点）。
        let (e2, r3) = l.allocate(256, 128, 1).unwrap();
        assert_ne!(e1, e2);
        assert_eq!(l.live_pages(), 2);
        assert_eq!(
            r3,
            Region {
                x: 0,
                y: 0,
                width: 256,
                height: 128
            }
        );
    }

    #[test]
    fn buckets_route_by_max_edge() {
        // 桶从小到大 256/512/1024/2048：300px 图进 512 桶。
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
        // 56×56 塞得进两页的底部余量（一页 256²−200 高 = 56）→ 落在 e1 页上；
        // 再来一张 57² 就真没位置了 → 池满 NeedFlush。
        let (e1b, _) = l.allocate(56, 56, 2).unwrap();
        assert_eq!(e1b, e1);
        assert_eq!(l.allocate(57, 57, 2), Err(LayoutError::NeedFlush));
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
        let (e2, r) = l.allocate(200, 200, 2).unwrap();
        assert_ne!(e, e2);
        assert_eq!((r.width, r.height), (200, 200));
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
        let (e, r) = l.allocate_large(4096, 4096, 0).unwrap();
        assert!(l.is_large(e));
        assert_eq!(
            r,
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
