//! 实例缓冲的环形落位计划（纯逻辑，三端可跑单测）。
//!
//! 一个"槽"= 一块 `capacity` 个 quad 的 `MAP_WRITE|VERTEX` 缓冲。帧首换槽——上一帧的
//! 数据可能仍被 GPU 读取，绝不回头覆盖；帧内向前写，写满即换到下一槽（"写满先 flush 再续"）。
//! 单帧需求超过 `capacity × 片数` 时按几何倍增扩槽，而不是丢实例。

/// 设计文档规定的单 pass 默认容量。
pub const DEFAULT_QUADS_PER_SLOT: u32 = 16384;

/// 同时在飞的帧数（= 物理缓冲片数）。`PresentMode::Fifo` 下不会超过交换链图像数。
pub const FRAMES_IN_FLIGHT: usize = 3;

#[derive(Clone, Debug)]
pub struct InstanceRing {
    capacity: u32,
    buffers: usize,
    base: u32,
    used: u32,
    flushes: u32,
}

impl InstanceRing {
    pub fn new(capacity_quads: u32, buffers: usize) -> Self {
        Self {
            capacity: capacity_quads.max(1),
            buffers: buffers.max(1),
            base: 0,
            used: 0,
            flushes: 0,
        }
    }

    /// 单槽可容纳的实例数（`BatchList::build` 用它切分跨槽批次）。
    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    /// 物理缓冲片数。
    pub fn buffers(&self) -> usize {
        self.buffers
    }

    /// 容量可配（xtask/`AppConfig` 侧调优用）。
    pub fn set_capacity(&mut self, capacity_quads: u32) {
        self.capacity = capacity_quads.max(1);
    }

    /// 本帧用到的槽数。
    pub fn slots_used(&self) -> u32 {
        self.used
    }

    /// 本帧基准槽号。
    pub fn base(&self) -> u32 {
        self.base
    }

    /// 累计换槽次数（诊断：非零 = 出现过"写满 flush"）。
    pub fn flushes(&self) -> u32 {
        self.flushes
    }

    /// 帧首：必要时扩槽，推进基准槽号，并算出本帧要几片。
    /// 返回 `Some(新容量)` 表示容量发生了增长（调用方需重建物理缓冲）。
    pub fn begin_frame(&mut self, total_instances: u32) -> Option<u32> {
        let grew = self.grow_to_fit(total_instances);
        self.base = self.base.wrapping_add(1);
        self.used = total_instances.div_ceil(self.capacity);
        debug_assert!(
            self.used <= self.buffers as u32,
            "扩容后本帧仍放不下在飞的缓冲片数"
        );
        self.flushes = self.flushes.wrapping_add(self.used.saturating_sub(1));
        grew
    }

    /// 逻辑槽号 → 物理缓冲下标。
    pub fn buffer_for(&self, logical_slot: u32) -> usize {
        let n = self.buffers as u32;
        ((self.base.wrapping_add(logical_slot)) % n) as usize
    }

    /// 装不下时几何倍增容量，使本帧所需片数 ≤ 片数上限；未变返回 `None`。
    pub fn grow_to_fit(&mut self, total_instances: u32) -> Option<u32> {
        let per_buffer = total_instances.div_ceil(self.buffers as u32);
        if per_buffer <= self.capacity {
            return None;
        }
        let mut cap = self.capacity.max(1);
        while cap < per_buffer {
            cap = cap.saturating_mul(2);
        }
        self.capacity = cap;
        Some(cap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // T12：`cargo xtask wasm-test` 在 wasm 上跑这一组（见 docs/platforms.md）。
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    #[test]
    fn empty_frame_uses_no_slot() {
        let mut ring = InstanceRing::new(4, 3);
        ring.begin_frame(0);
        assert_eq!(ring.slots_used(), 0);
        assert_eq!(ring.flushes(), 0);
    }

    #[test]
    fn overflow_wraps_into_next_buffer_and_counts_flush() {
        let mut ring = InstanceRing::new(4, 3);
        ring.begin_frame(6); // 4 + 2 → 两槽
        assert_eq!(ring.slots_used(), 2);
        assert_eq!(ring.flushes(), 1);
        let a = ring.buffer_for(0);
        let b = ring.buffer_for(1);
        assert_ne!(a, b);
        assert_eq!(ring.buffer_for(0), (ring.base() % 3) as usize);
    }

    #[test]
    fn never_reuses_the_buffer_still_in_flight() {
        let mut ring = InstanceRing::new(8, 3);
        let mut seen = Vec::new();
        for _ in 0..3 {
            ring.begin_frame(3);
            seen.push(ring.buffer_for(0));
        }
        // 三片轮转：连续三帧各用不同缓冲。
        assert_eq!(seen.len(), 3);
        assert!(seen[0] != seen[1] && seen[1] != seen[2] && seen[0] != seen[2]);
        // 第四帧才回到第一片。
        ring.begin_frame(3);
        assert_eq!(ring.buffer_for(0), seen[0]);
    }

    #[test]
    fn capacity_grows_geometrically_to_fit_one_frame() {
        let mut ring = InstanceRing::new(4, 3);
        // 100 个实例、3 片 → 每片至少 34 → 倍增到 64。
        assert_eq!(ring.grow_to_fit(100), Some(64));
        assert_eq!(ring.capacity(), 64);
        ring.begin_frame(100);
        assert!(ring.slots_used() <= ring.buffers() as u32);
        // 小帧不再缩容（环形缓冲只增不减，避免每帧重建）。
        assert_eq!(ring.grow_to_fit(10), None);
    }

    #[test]
    fn begin_frame_reports_growth() {
        let mut ring = InstanceRing::new(4, 3);
        assert_eq!(ring.begin_frame(8), None); // 8 = 2 槽 × 4，无需扩容
        assert_eq!(ring.slots_used(), 2);
        // 12 个实例、3 片 → 每片至少 4 → 刚好够；13 个才要求 5 → 倍增到 8。
        assert_eq!(ring.begin_frame(12), None);
        assert_eq!(ring.begin_frame(13), Some(8));
        assert_eq!(ring.capacity(), 8);
        assert!(ring.slots_used() <= ring.buffers() as u32);
    }

    #[test]
    fn default_capacity_matches_design() {
        let ring = InstanceRing::new(DEFAULT_QUADS_PER_SLOT, FRAMES_IN_FLIGHT);
        assert_eq!(ring.capacity(), 16384);
        assert_eq!(ring.buffers(), 3);
    }

    #[test]
    fn degenerate_config_is_clamped() {
        let mut ring = InstanceRing::new(0, 0);
        assert_eq!(ring.capacity(), 1);
        assert_eq!(ring.buffers(), 1);
        ring.set_capacity(0);
        assert_eq!(ring.capacity(), 1);
    }
}
