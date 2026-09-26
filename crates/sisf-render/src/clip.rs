//! 裁剪栈：`push_clip/pop_clip` → 一次 draw 前的 `scissor_rect`。
//!
//! 模型（T6）：
//! - 裁剪是**帧内**状态。每个 clip 在首次按下时分配一个 `u32` 代号，落进
//!   [`BatchKey::clip`](crate::batch::BatchKey::clip)，`Batch` 再带着它走。代号一旦分配
//!   本帧内不变（同一矩形重复按下也各占一个代号），所以"clip 变化即断批"是排序的天然
//!   结果，不需要显式 flush 调用。
//! - `NO_CLIP`（0）恒等于"整个目标"，因此**主帧与各位图会话共用一张表**也不会串味：
//!   0 号在哪个目标上都不是裁剪，而其它代号存的是**该目标自己的绝对像素矩形**。
//! - 嵌套窗体靠求交：`push` 进去的矩形与当前栈顶求交，交空就得到一个空矩形
//!   （记录时直接跳过 draw，而不是下发一个 0 宽的 scissor）。
//! - 内容坐标 → 目标像素靠该层的投影矩阵（[`rect_to_pixels`]），于是 letterbox、
//!   整数缩放、位图会话（`ortho(w,h)`）三条路径用同一套代码。

use crate::vertex::mat4_xform;

/// 不裁剪（整目标可见）。
pub const NO_CLIP: u32 = 0;

/// 目标像素矩形（左上原点）。`w == 0 || h == 0` 表示"裁到什么都不剩"。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scissor {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Scissor {
    pub const EMPTY: Scissor = Scissor {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    };

    pub const fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    /// 整个目标（即"不裁剪"的几何含义）。
    pub const fn full(target: (u32, u32)) -> Self {
        Self {
            x: 0,
            y: 0,
            w: target.0,
            h: target.1,
        }
    }

    pub const fn is_empty(self) -> bool {
        self.w == 0 || self.h == 0
    }

    /// 交集（`Rect` 语义：`w`/`h` 是尺寸而非右下角）。
    pub fn intersect(self, o: Self) -> Self {
        let (a, b) = ((self.x + self.w, self.y + self.h), (o.x + o.w, o.y + o.h));
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let right = a.0.min(b.0);
        let bottom = a.1.min(b.1);
        if right <= x || bottom <= y {
            return Self::EMPTY;
        }
        Self {
            x,
            y,
            w: right - x,
            h: bottom - y,
        }
    }

    pub const fn ends(self) -> (u32, u32) {
        (self.x + self.w, self.y + self.h)
    }
}

/// 把某层内容坐标系里的矩形 `[x, y, w, h]` 变成目标像素矩形。
///
/// 四个角都投影一遍（矩阵可能带负缩放），取包围盒后**向外取整**——
/// 向内取整会让 1x 映射下的边缘少掉一整列像素，文字/边框就会出现 1px 缺口。
/// 结果再与目标求交，所以完全在目标外的矩形返回 [`Scissor::EMPTY`]。
///
/// 向外取整前先把**离整像素 `1e-3` 以内**的值吸附回去：整数坐标经
/// "乘 scale → 加偏移 → 映到 NDC → 映回像素"一圈下来常带 1e-5 的误差
/// （`72.0` 变 `72.000001`），不吸附就会被 `floor/ceil` 各白送一行一列。
pub fn rect_to_pixels(proj: &[[f32; 4]; 4], rect: [f32; 4], target: (u32, u32)) -> Scissor {
    let (tw, th) = target;
    if tw == 0 || th == 0 {
        return Scissor::EMPTY;
    }
    let (w, h) = (tw as f32, th as f32);
    let snap = |v: f32| -> f32 {
        let r = v.round();
        if (v - r).abs() < 1e-3 { r } else { v }
    };
    let mut min = [f32::MAX, f32::MAX];
    let mut max = [f32::MIN, f32::MIN];
    for corner in [
        [rect[0], rect[1]],
        [rect[0] + rect[2], rect[1]],
        [rect[0], rect[1] + rect[3]],
        [rect[0] + rect[2], rect[1] + rect[3]],
    ] {
        let ndc = mat4_xform(proj, [corner[0], corner[1], 0.0, 1.0]);
        if !ndc[0].is_finite() || !ndc[1].is_finite() {
            return Scissor::EMPTY;
        }
        // NDC([-1,1], y 向上) → 像素(左上原点, y 向下)。
        let px = snap((ndc[0] * 0.5 + 0.5) * w);
        let py = snap((0.5 - ndc[1] * 0.5) * h);
        min = [min[0].min(px), min[1].min(py)];
        max = [max[0].max(px), max[1].max(py)];
    }
    let clamp_i = |v: f32, hi: u32| -> u32 {
        if !v.is_finite() {
            hi
        } else {
            (v.round() as i64).clamp(0, hi as i64) as u32
        }
    };
    let x0 = clamp_i(min[0].floor(), tw);
    let y0 = clamp_i(min[1].floor(), th);
    let x1 = clamp_i(max[0].ceil(), tw);
    let y1 = clamp_i(max[1].ceil(), th);
    if x1 <= x0 || y1 <= y0 {
        return Scissor::EMPTY;
    }
    Scissor {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    }
}

/// 帧内裁剪表 + 活动栈。
#[derive(Clone, Debug, Default)]
pub struct ClipStack {
    /// `id → 生效矩形`；`None` = 不裁剪（只有 `NO_CLIP` 会是 `None`）。
    rects: Vec<Option<Scissor>>,
    /// 活动栈（根是 `NO_CLIP`，故 `depth() == stack.len()`）。
    stack: Vec<u32>,
    target: (u32, u32),
    /// 本帧分配的 clip 数（= 至少会出现多少次断批）。
    pushed: u32,
}

impl ClipStack {
    /// 帧首/会话首：清空栈，目标尺寸决定"不裁剪"的边界。
    pub fn begin(&mut self, target: (u32, u32)) {
        self.rects = vec![None];
        self.stack.clear();
        self.target = target;
        self.pushed = 0;
    }

    pub fn target(&self) -> (u32, u32) {
        self.target
    }

    /// 当前生效代号（提交实例时填进 `BatchKey::clip`）。
    pub fn top(&self) -> u32 {
        self.stack.last().copied().unwrap_or(NO_CLIP)
    }

    /// 栈深（0 = 未裁剪）。
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// 本帧分配过的 clip 代号数。
    pub fn pushed(&self) -> u32 {
        self.pushed
    }

    /// 代号 → 生效矩形（`None` = 不裁剪）。
    pub fn rect_of(&self, id: u32) -> Option<Scissor> {
        match self.rects.get(id as usize) {
            Some(Some(r)) => Some(*r),
            Some(None) => Some(Scissor::full(self.target)),
            None => Some(Scissor::EMPTY),
        }
    }

    /// 按下一层裁剪：与当前栈顶求交后入栈，返回新代号。
    pub fn push(&mut self, rect: Scissor) -> u32 {
        let clipped = match self.top() {
            NO_CLIP => rect.intersect(Scissor::full(self.target)),
            id => self.rect_of(id).unwrap_or(Scissor::EMPTY).intersect(rect),
        };
        let id = self.rects.len() as u32;
        self.rects.push(Some(clipped));
        self.stack.push(id);
        self.pushed += 1;
        id
    }

    /// 弹出一层裁剪（栈空时是 no-op，回到 `NO_CLIP`）。
    pub fn pop(&mut self) {
        self.stack.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // T12：`cargo xtask wasm-test` 在 wasm 上跑这一组（见 docs/platforms.md）。
    use crate::vertex::ortho;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    #[test]
    fn nested_clips_intersect_and_pop() {
        let mut c = ClipStack::default();
        c.begin((200, 100));
        assert_eq!(c.top(), NO_CLIP);
        assert_eq!(c.depth(), 0);
        let outer = c.push(Scissor::new(10, 10, 100, 80));
        assert_eq!(c.top(), outer);
        assert_eq!(c.depth(), 1);
        let inner = c.push(Scissor::new(50, 40, 500, 500));
        assert_eq!(c.depth(), 2);
        assert_ne!(inner, outer, "每次按下都是新代号");
        assert_eq!(c.rect_of(inner), Some(Scissor::new(50, 40, 60, 50)));
        // 交空：仍给代号，但矩形是 EMPTY。
        let gone = c.push(Scissor::new(0, 0, 5, 5));
        assert_eq!(c.rect_of(gone), Some(Scissor::EMPTY));
        assert_eq!(c.pushed(), 3);
        c.pop();
        assert_eq!(c.top(), inner);
        c.pop();
        assert_eq!(c.top(), outer);
        c.pop();
        assert_eq!(c.top(), NO_CLIP);
        c.pop(); // 多余 pop 不 panic
        assert_eq!(c.rect_of(NO_CLIP), Some(Scissor::full((200, 100))));
        // 未知代号（上一帧残留）按"裁光"处理，宁可少画也不画错。
        assert_eq!(c.rect_of(9999), Some(Scissor::EMPTY));
    }

    #[test]
    fn intersect_is_commutative_and_rejects_disjoint() {
        let a = Scissor::new(0, 0, 10, 10);
        let b = Scissor::new(5, 5, 10, 10);
        assert_eq!(a.intersect(b), Scissor::new(5, 5, 5, 5));
        assert_eq!(b.intersect(a), a.intersect(b));
        assert!(
            a.intersect(Scissor::new(10, 0, 4, 4)).is_empty(),
            "边相接不算交"
        );
        assert!(Scissor::EMPTY.intersect(a).is_empty());
    }

    /// 1x 映射（位图会话）：矩形逐像素照搬，不做任何缩放。
    #[test]
    fn rect_to_pixels_matches_ortho_one_to_one() {
        let proj = ortho(320.0, 200.0);
        assert_eq!(
            rect_to_pixels(&proj, [8.0, 12.0, 64.0, 32.0], (320, 200)),
            Scissor::new(8, 12, 64, 32)
        );
        // 越界裁进目标；完全在外为 EMPTY。
        assert_eq!(
            rect_to_pixels(&proj, [-10.0, -10.0, 20.0, 20.0], (320, 200)),
            Scissor::new(0, 0, 10, 10)
        );
        assert!(rect_to_pixels(&proj, [400.0, 0.0, 10.0, 10.0], (320, 200)).is_empty());
        assert!(rect_to_pixels(&proj, [0.0, 0.0, 10.0, 10.0], (0, 0)).is_empty());
    }

    /// 2x 虚拟→物理（letterbox 偏移 60,40）：内容坐标要乘scale再加偏移。
    #[test]
    fn rect_to_pixels_follows_layer_projection() {
        use crate::vertex::ortho_viewport;
        let proj = ortho_viewport(1280.0, 960.0, [2.0, 2.0], [60.0, 40.0]);
        // 虚拟 (10,20) 起 30x40 → 物理 (60+20, 40+40) 起 60x80。
        assert_eq!(
            rect_to_pixels(&proj, [10.0, 20.0, 30.0, 40.0], (1280, 960)),
            Scissor::new(80, 80, 60, 80)
        );
        // 分数 scale：向外取整，保证边缘一列不被丢掉（宁可多一像素也不缺）。
        let half = ortho_viewport(544.0, 416.0, [0.5, 0.5], [0.0, 0.0]);
        let r = rect_to_pixels(&half, [1.0, 1.0, 3.0, 3.0], (544, 416));
        assert_eq!((r.x, r.y), (0, 0), "0.5x 下 [1,4) → [0.5,2) 取整");
        assert!(r.w >= 2 && r.h >= 2, "不得因取整退化为空：{r:?}");
    }

    /// 退化投影（尺寸 0 → `ortho` 返回全零矩阵）不能算出裁剪矩形。
    #[test]
    fn degenerate_projection_clips_nothing_visible() {
        let proj = ortho(0.0, 0.0);
        assert!(rect_to_pixels(&proj, [0.0, 0.0, 10.0, 10.0], (100, 100)).is_empty());
    }
}
