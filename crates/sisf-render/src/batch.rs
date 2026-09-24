//! 批次分组：排序键 → 相邻同键合并为一次 `set_bind_group` + `draw`。
//!
//! 纯逻辑，不碰 wgpu，三端（含 wasm）都能跑单测。

/// 混合模式对应的管线。`Multiply/Screen` 在 T12 决策（见 todo.md），此处不占位。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Blend {
    /// alpha-over（预乘 src）。
    Normal,
    /// 加法发光。
    Add,
    /// **直接替换**目标像素（不做混合）。只为 `clear_rect` 存在：
    /// 一个 alpha=0 的矩形经它写出去就是"该矩形透明"，从而不需要 wgpu 的
    /// 区域 clear（`CommandEncoder::clear_texture` 只能清整个 subresource）。
    /// 精灵语义里不用它（RPG Maker 的 `blt` 是 source-over）。
    Replace,
}

impl Blend {
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::Normal => 0,
            Self::Add => 1,
            Self::Replace => 2,
        }
    }
}

/// 排序键。元组序即绘制序：`layer` 最粗，`insertion` 保证同键稳定。
///
/// 合并批次时只看 `(layer, epoch, blend)`——`z`/`insertion` 只决定批内实例顺序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchKey {
    /// viewport 栈索引（0..7）。
    pub layer: u8,
    /// 裁剪代号（`Renderer::clip_id`，0 = 不裁剪）。
    ///
    /// 由 `Renderer::push` 在提交时注入，采集端不必（也不该）自己填：裁剪是**帧内**
    /// 状态，只有渲染器知道当前栈顶。它排在 `epoch` 之前 ⇒ clip 一变即断批（= flush）。
    pub clip: u32,
    /// 纹理代号（T4 起由图集页/RT 提供；相同 epoch 才能共享 bind group）。
    pub epoch: u64,
    pub blend: Blend,
    /// 同一 viewport 内的深度（RPG Maker 的 z，稳定排序的下级键）。
    pub z: i32,
    /// 提交序号，用于完全同键时保持提交顺序。
    pub insertion: u32,
}

impl BatchKey {
    /// 排序键（展开成元组，避免手写比较漏字段）。
    const fn order(self) -> (u8, u32, u64, u8, i32, u32) {
        (
            self.layer,
            self.clip,
            self.epoch,
            self.blend.as_u8(),
            self.z,
            self.insertion,
        )
    }

    /// 合批键：相邻两项相等则可并入同一批。
    const fn group(self) -> (u8, u32, u64, u8) {
        (self.layer, self.clip, self.epoch, self.blend.as_u8())
    }
}

/// 一段可合并的实例：在环形缓冲的**同一个槽**内连续，使用同一 bind group。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Batch {
    pub layer: u8,
    /// 该批生效的裁剪代号（`u32::MAX` = 跟随目标，见 `clip` 模块）。
    pub clip: u32,
    pub epoch: u64,
    pub blend: Blend,
    /// 槽内起始实例下标。
    pub start: u32,
    /// 实例数（一次 draw 的 instanceCount）。
    pub len: u32,
    /// 环形缓冲槽位（跨槽 = 缓冲写满 flush 的产物）。
    pub slot: u32,
}

/// 待绘实例集合：`(BatchKey, Instance)` 对，按 `order()` 排序后分组。
#[derive(Clone, Debug, Default)]
pub struct BatchList<I> {
    items: Vec<(BatchKey, I)>,
    /// 提交计数（跨 clear 递增，用于 insertion 键）。
    seq: u32,
}

impl<I> BatchList<I> {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            seq: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 追加一个实例，`insertion` 由列表自己填。
    pub fn push(&mut self, key: BatchKey, instance: I) {
        let key = BatchKey {
            insertion: self.seq,
            ..key
        };
        self.seq = self.seq.wrapping_add(1);
        self.items.push((key, instance));
    }

    /// 把另一个列表按序并入（子层/位图 pass 收集用）。
    pub fn append(&mut self, mut other: Self) {
        self.items.append(&mut other.items);
    }

    /// 清空实例但保留 `seq`（插入序号跨帧递增，避免复用旧序号）。
    pub fn reset(&mut self) {
        self.items.clear();
    }

    /// 把全部实例的裁剪代号重填为 `clip`（并入别的目标前用；`insertion` 保持不变，
    /// 所以同键实例的相对顺序还是提交顺序）。
    pub fn set_clip(&mut self, clip: u32) {
        for (key, _) in self.items.iter_mut() {
            key.clip = clip;
        }
    }

    /// 排序并切成批次。`capacity` = 环形缓冲单槽可容纳的实例数；
    /// 一段同键实例跨越槽边界时按边界拆成两批（写满先 flush 再续）。
    pub fn build(&mut self, capacity: u32) -> Vec<Batch> {
        self.items.sort_unstable_by_key(|(key, _)| key.order());
        self.group_runs(capacity)
    }

    /// **不排序**地成批：只把相邻的同组项合并。
    ///
    /// 位图绘制会话（`begin_draw`/`end_draw`）用这条：RPG Maker 的
    /// `fill_rect → blt → draw_text` 是**指令顺序即覆盖顺序**，若按 `(epoch, blend)`
    /// 排序会把"先贴的图"排到"后填的矩形"下面。代价是相邻两指令即便同组也要求
    /// 提交顺序相邻才合批——绘制会话的指令数很小，这个代价可以接受。
    pub fn build_in_order(&mut self, capacity: u32) -> Vec<Batch> {
        self.group_runs(capacity)
    }

    fn group_runs(&self, capacity: u32) -> Vec<Batch> {
        assert!(capacity > 0, "batch capacity must be non-zero");
        let mut out: Vec<Batch> = Vec::new();
        // (段首键, 段首下标, 段长)
        let mut cur: Option<(BatchKey, u32, u32)> = None;
        for (idx, (key, _)) in self.items.iter().enumerate() {
            let idx = idx as u32;
            let extend = match cur {
                // 合批键相同、紧接上段、且不跨槽边界，才可并成一次 draw。
                Some((prev, start, len)) => {
                    prev.group() == key.group()
                        && idx == start + len
                        && idx / capacity == start / capacity
                }
                None => false,
            };
            if extend {
                if let Some((_, _, len)) = cur.as_mut() {
                    *len += 1;
                }
            } else {
                if let Some((prev, start, len)) = cur {
                    out.push(batch_of(prev, start, len, capacity));
                }
                cur = Some((*key, idx, 1));
            }
        }
        if let Some((key, start, len)) = cur {
            out.push(batch_of(key, start, len, capacity));
        }
        out
    }

    /// 已排序的键与实例（`build` 之后按 `Batch.start/len` + `slot` 定位）。
    pub fn sorted_items(&self) -> &[(BatchKey, I)] {
        &self.items
    }
}

fn batch_of(key: BatchKey, start: u32, len: u32, capacity: u32) -> Batch {
    Batch {
        layer: key.layer,
        clip: key.clip,
        epoch: key.epoch,
        blend: key.blend,
        start: start % capacity,
        len,
        slot: start / capacity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(layer: u8, epoch: u64, blend: Blend, z: i32) -> BatchKey {
        BatchKey {
            layer,
            clip: 0,
            epoch,
            blend,
            z,
            insertion: 0,
        }
    }

    #[test]
    fn adjacent_same_key_merge_into_one_batch() {
        let mut list: BatchList<u32> = BatchList::new();
        for i in 0..5 {
            list.push(key(0, 7, Blend::Normal, i), i as u32);
        }
        let batches = list.build(u32::MAX);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].len, 5);
        assert_eq!(batches[0].epoch, 7);
    }

    /// clip 在合批键里排在 epoch 之前：同图同 blend、只是裁剪不同，也必须断批。
    #[test]
    fn clip_change_breaks_batch() {
        let mut list: BatchList<u32> = BatchList::new();
        for (i, clip) in [0u32, 1, 0].iter().enumerate() {
            list.push(
                BatchKey {
                    layer: 0,
                    clip: *clip,
                    epoch: 3,
                    blend: Blend::Normal,
                    z: 0,
                    insertion: 0,
                },
                i as u32,
            );
        }
        let batches = list.build_in_order(u32::MAX);
        let shape: Vec<(u32, u32)> = batches.iter().map(|b| (b.clip, b.len)).collect();
        assert_eq!(shape, vec![(0, 1), (1, 1), (0, 1)], "clip 变化处必须断批");
    }

    /// 绘制会话按提交顺序成批：跨 epoch 的相邻指令不会像 `build` 那样被重排。
    #[test]
    fn build_in_order_keeps_submission_order_across_epochs() {
        let mut list: BatchList<&'static str> = BatchList::new();
        list.push(key(0, 9, Blend::Normal, 0), "atlas");
        list.push(key(0, 0, Blend::Replace, 0), "clear");
        list.push(key(0, 9, Blend::Normal, 0), "atlas again");
        let sorted: Vec<u64> = list.build(u32::MAX).iter().map(|b| b.epoch).collect();
        assert_eq!(
            sorted,
            vec![0, 9],
            "build 会按 epoch 重排：两条 atlas 指令被拉到一起合成一批"
        );
        let mut list2: BatchList<&'static str> = BatchList::new();
        list2.push(key(0, 9, Blend::Normal, 0), "atlas");
        list2.push(key(0, 0, Blend::Replace, 0), "clear");
        list2.push(key(0, 9, Blend::Normal, 0), "atlas again");
        let runs: Vec<(u64, Blend, u32)> = list2
            .build_in_order(u32::MAX)
            .iter()
            .map(|b| (b.epoch, b.blend, b.len))
            .collect();
        assert_eq!(
            runs,
            vec![
                (9, Blend::Normal, 1),
                (0, Blend::Replace, 1),
                (9, Blend::Normal, 1)
            ],
            "同键但不相邻 ⇒ 三条批，顺序即提交顺序"
        );
        assert_eq!(list2.sorted_items()[0].1, "atlas");
    }

    #[test]
    fn different_key_breaks_batch_and_keeps_order() {
        let mut list: BatchList<&'static str> = BatchList::new();
        list.push(key(0, 1, Blend::Normal, 0), "a0");
        list.push(key(0, 2, Blend::Normal, 0), "b0");
        list.push(key(0, 1, Blend::Normal, 1), "a1");
        list.push(key(1, 1, Blend::Normal, 0), "l1");
        list.push(key(0, 1, Blend::Add, 0), "add");
        let batches = list.build(u32::MAX);
        // 期望分组：(L0,E1,normal) → (L0,E1,add) → (L0,E2,normal) → (L1,E1,normal)
        let shape: Vec<(u8, u64, Blend, u32)> = batches
            .iter()
            .map(|b| (b.layer, b.epoch, b.blend, b.len))
            .collect();
        assert_eq!(
            shape,
            vec![
                (0, 1, Blend::Normal, 2),
                (0, 1, Blend::Add, 1),
                (0, 2, Blend::Normal, 1),
                (1, 1, Blend::Normal, 1),
            ]
        );
        // 提交顺序 = 排序后的实例顺序。
        let flat: Vec<&str> = batches
            .iter()
            .flat_map(|b| {
                list.sorted_items()[b.start as usize..(b.start + b.len) as usize]
                    .iter()
                    .map(|(_, i)| *i)
            })
            .collect();
        assert_eq!(flat, vec!["a0", "a1", "add", "b0", "l1"]);
    }

    #[test]
    fn insertion_breaks_ties() {
        let mut list: BatchList<u32> = BatchList::new();
        for i in 0..3 {
            // 完全同键：只能靠 insertion 稳定。
            list.push(key(0, 0, Blend::Normal, 0), i);
        }
        let batches = list.build(u32::MAX);
        assert_eq!(batches.len(), 1);
        let order: Vec<u32> = list.sorted_items().iter().map(|(_, v)| *v).collect();
        assert_eq!(order, vec![0, 1, 2]);
    }

    #[test]
    fn z_orders_within_batch() {
        let mut list: BatchList<char> = BatchList::new();
        list.push(key(0, 0, Blend::Normal, 10), 'c');
        list.push(key(0, 0, Blend::Normal, -5), 'a');
        list.push(key(0, 0, Blend::Normal, 0), 'b');
        list.build(u32::MAX);
        let zs: Vec<i32> = list.sorted_items().iter().map(|(k, _)| k.z).collect();
        assert_eq!(zs, vec![-5, 0, 10]);
    }

    #[test]
    fn split_at_capacity_breaks_run() {
        let mut list: BatchList<u32> = BatchList::new();
        for i in 0..6 {
            list.push(key(0, 0, Blend::Normal, 0), i);
        }
        // 单槽容量 4：[0,4) 落槽 0，[4,6) 续到槽 1（写满先 flush 再续）。
        let batches = list.build(4);
        assert_eq!(batches.len(), 2);
        assert_eq!(
            (batches[0].start, batches[0].len, batches[0].slot),
            (0, 4, 0)
        );
        assert_eq!(
            (batches[1].start, batches[1].len, batches[1].slot),
            (0, 2, 1)
        );
    }

    #[test]
    fn empty_list_yields_no_batch() {
        let mut list: BatchList<u32> = BatchList::new();
        assert!(list.build(u32::MAX).is_empty());
        list.push(key(0, 0, Blend::Normal, 0), 1);
        list.reset();
        assert!(list.build(u32::MAX).is_empty());
    }
}
