//! 动画：帧表（[`AnimDef`]）+ 播放状态（[`AnimState`]）+ RPG Maker 的方向图样（[`DirPattern`]）。
//!
//! 两套语义并存，而且是**互斥**的一条岔路：
//!
//! - **时间驱动**（`pattern: None`）：按 `frames[i].dur_ms` 逐帧走，`loop_` 决定到尾之后
//!   停/回环/循环若干次。GIF 多帧（T4 的解码结果）、火把闪烁、门开合都是这一路。
//! - **移动驱动**（`pattern: Some(..)`）：帧号是 `(dir, step)` 的纯函数，`tick` **不动**
//!   `index`。RPG Maker 的行走图就是这一套 —— 走路时脚下换帧，站着永远停在第 0 列，
//!   所以这里没有"每 120ms 换一帧"这回事，换不换由 [`AnimState::set_step`] 的调用方
//!   （T8c 的 `MoveRoute`，或游戏逻辑）决定。这正是验收那条"静止不换帧"的含义。
//!
//! 状态与几何分开：`AnimState` 只算"现在是第几帧、该取哪块 `src`"，落到精灵上是一次
//! [`Sprite::src`](crate::sprite::Sprite::src) 赋值。所以采集路径（`draw.rs`）完全不知道
//! 动画存在，`collect` 的排序/裁剪语义不会因为加了动画而多一个分支。
//!
//! ```
//! use std::sync::Arc;
//! use std::time::Duration;
//! use sisf_sprite::{AnimDef, AnimState, DirPattern, Rect};
//!
//! // MV 式 4×3 行走图：4 个朝向 × 3 步帧，行序 下/左/右/上。
//! let def = Arc::new(AnimDef::grid_pattern(DirPattern::RpgMaker4x3, 24.0, 32.0, 8));
//! let mut anim = AnimState::new(def);
//! anim.set_dir(8); // 上 ⇒ 第 3 行
//! assert_eq!(anim.rect(), Rect::new(0.0, 96.0, 24.0, 32.0));
//! // 时间再走也不换帧：只有步帧变，帧才变。
//! anim.tick(Duration::from_millis(500));
//! assert_eq!(anim.rect(), Rect::new(0.0, 96.0, 24.0, 32.0));
//! anim.set_step(1);
//! assert_eq!(anim.rect(), Rect::new(24.0, 96.0, 24.0, 32.0));
//! assert_eq!(anim.index, 10, "上行第 3 格 = 3*3 + 1");
//! ```

use std::sync::Arc;
use std::time::Duration;

use crate::arena::SpriteArena;
use crate::sprite::{Rect, Sprite, Vec2};

/// 一帧：位图子矩形 + 停留时长。
///
/// `dur_ms` 只在时间驱动下有意义；方向图样模式下它恒被忽略（那条通路根本不读它，
/// 所以留 0 也不会除零）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameDef {
    pub rect: Rect,
    pub dur_ms: u32,
}

impl FrameDef {
    pub const fn new(rect: Rect, dur_ms: u32) -> Self {
        Self { rect, dur_ms }
    }
}

/// 播到最后一帧之后怎么办。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopMode {
    /// 停在最后一帧（`finished()` 此后恒为真）。
    Once,
    /// 原路退回：`0,1,2,1,0,1,2…`（端点不重复停留）。
    Pingpong,
    /// 从头循环 `n` 圈；`0` = 不限次数。跑满后停在第 0 格（回环的终点即起点），
    /// 与 [`Once`](Self::Once) 停在末格相反 —— 那是"演完保持最后一帧"的手势动画。
    Loop(u32),
}

/// RPG Maker 行走图的"朝向 × 步帧"排布。
///
/// 两个变体只差在**列数**（步帧数）与步帧的取模方式，行侧一致：从上到下恒为
/// `下、左、右、上`（方向键 `2/4/6/8`）。帧号 = `dir_row * cols + col`，即清单写的
/// `dir + 3*step` 的行列转置 —— 本仓库统一按行优先读图，与 [`AnimDef::grid`] 的装箱序一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirPattern {
    /// MV/MZ 的 3 列 × 4 行：`step ∈ {0,1,2}`，0 是站立帧。
    RpgMaker4x3,
    /// VX/Ace 的 2 列 × 4 行：内部步帧仍走 0..2，但第 2 步折回第 0 列（RTP 图只有两列）。
    RpgMaker2x2,
}

/// 朝向（RPG Maker 的方向键值）。
pub const DIR_DOWN: u8 = 2;
pub const DIR_LEFT: u8 = 4;
pub const DIR_RIGHT: u8 = 6;
pub const DIR_UP: u8 = 8;

impl DirPattern {
    /// 列数（= 图上的步帧列数）。
    pub const fn cols(self) -> u32 {
        match self {
            Self::RpgMaker4x3 => 3,
            Self::RpgMaker2x2 => 2,
        }
    }

    /// 行数（四个朝向）。
    pub const fn rows(self) -> u32 {
        4
    }

    /// 一张完整图样的格子数。
    pub const fn cell_count(self) -> u32 {
        self.cols() * self.rows()
    }

    /// 方向键值 → 行号；未知朝向按"下"（RPG Maker 的默认朝向）。
    pub const fn dir_row(dir: u8) -> u32 {
        match dir {
            DIR_DOWN => 0,
            DIR_LEFT => 1,
            DIR_RIGHT => 2,
            DIR_UP => 3,
            _ => 0,
        }
    }

    /// `(朝向, 步帧)` → 帧号。步帧取模 ⇒ 越界的值不会走出图外。
    pub const fn frame_index(self, dir: u8, step: u8) -> u32 {
        let col = match self {
            Self::RpgMaker2x2 => (step % 2) as u32,
            Self::RpgMaker4x3 => (step % 3) as u32,
        };
        Self::dir_row(dir) * self.cols() + col
    }
}

/// 一套动画的定义：帧表 + 循环方式 + 可选方向图样。
///
/// `frames` 按行优先排列，方向图样的 `frame_index` 直接下标它，所以要求帧表
/// 覆盖 `cols*rows` 格；短于图样时 [`AnimState::rect`] 自己夹住，不 panic。
#[derive(Clone, Debug, PartialEq)]
pub struct AnimDef {
    pub frames: Vec<FrameDef>,
    pub loop_: LoopMode,
    pub pattern: Option<DirPattern>,
}

impl AnimDef {
    pub fn new(frames: Vec<FrameDef>) -> Self {
        Self {
            frames,
            loop_: LoopMode::Loop(0),
            pattern: None,
        }
    }

    /// 把 `cols × rows` 的均匀网格摊成帧表（行走图/图块集的标准形状）。
    pub fn grid(cell_width: f32, cell_height: f32, cols: u32, rows: u32, dur_ms: u32) -> Self {
        let mut frames = Vec::with_capacity((cols * rows) as usize);
        for row in 0..rows {
            for col in 0..cols {
                frames.push(FrameDef::new(
                    Rect::new(
                        col as f32 * cell_width,
                        row as f32 * cell_height,
                        cell_width,
                        cell_height,
                    ),
                    dur_ms,
                ));
            }
        }
        Self::new(frames)
    }

    /// 方向图样行走图（4 行 × `pattern.cols()` 列，各格等长）。
    pub fn grid_pattern(
        pattern: DirPattern,
        cell_width: f32,
        cell_height: f32,
        dur_ms: u32,
    ) -> Self {
        let frames = Self::grid(
            cell_width,
            cell_height,
            pattern.cols(),
            pattern.rows(),
            dur_ms,
        );
        Self {
            frames: frames.frames,
            loop_: LoopMode::Loop(0),
            pattern: Some(pattern),
        }
    }

    pub fn frame_count(&self) -> u32 {
        self.frames.len() as u32
    }
}

/// 一个精灵的播放状态，挂在 [`Sprite::anim`] 上，由 [`SpriteArena::tick_anims`] 统一推进。
#[derive(Clone, Debug)]
pub struct AnimState {
    pub def: Arc<AnimDef>,
    /// 播放倍率（1.0 = 按 `dur_ms`，2.0 = 快一倍）。方向图样模式下无意义。
    pub speed: f32,
    /// 当前帧号。方向图样模式下它是 `(dir, step)` 的投影，不接受 `tick` 推进。
    pub index: u32,
    pub paused: bool,
    /// 当前帧已停留的时间（毫秒，已按 `speed` 折算）。
    elapsed_ms: f32,
    /// `Pingpong` 的行进方向（1 或 -1）。
    step_dir: i32,
    /// 已完成的整圈数（`Loop(n)` 用它判定到尾）。
    cycles: u32,
    dir: u8,
    step: u8,
}

impl AnimState {
    pub fn new(def: Arc<AnimDef>) -> Self {
        let mut state = Self {
            def,
            speed: 1.0,
            index: 0,
            paused: false,
            elapsed_ms: 0.0,
            step_dir: 1,
            cycles: 0,
            dir: DIR_DOWN,
            step: 0,
        };
        // 方向图样下帧号不是 0，而是"朝下站着"那一格（图样左上角），两者恰好相同；
        // 走一次投影是为了让 `index` 与 `dir/step` 从第一帧起就自洽。
        state.sync_pattern_index();
        state
    }

    pub fn pattern(&self) -> Option<DirPattern> {
        self.def.pattern
    }

    /// 当前朝向（方向键值）。
    pub fn dir(&self) -> u8 {
        self.dir
    }

    /// 当前步帧（0 = 站立）。
    pub fn step(&self) -> u8 {
        self.step
    }

    /// 是否已播完（`Once` 到尾、或 `Loop(n)` 跑满 `n` 圈）。
    pub fn finished(&self) -> bool {
        match self.def.loop_ {
            LoopMode::Once => self.index >= self.def.frame_count().saturating_sub(1),
            // 回环与不限圈永不到尾。
            LoopMode::Pingpong | LoopMode::Loop(0) => false,
            LoopMode::Loop(n) => self.cycles >= n,
        }
    }

    /// 设朝向；返回帧号是否因此改变。
    pub fn set_dir(&mut self, dir: u8) -> bool {
        let dir = normalize_dir(dir);
        if self.dir == dir {
            return false;
        }
        self.dir = dir;
        self.sync_pattern_index()
    }

    /// 设步帧（RPG 语义里由移动驱动的 `0,1,2` 循环）；返回帧号是否因此改变。
    ///
    /// **静止时不推进帧**是本模块的契约：调用方负责在停下时把步帧归 0，动画本身
    /// 不会自己往前走（见模块头）。
    pub fn set_step(&mut self, step: u8) -> bool {
        if self.step == step {
            return false;
        }
        self.step = step;
        self.sync_pattern_index()
    }

    /// 当前帧的源矩形（空帧表 ⇒ `Rect::ZERO`）。
    pub fn rect(&self) -> Rect {
        self.def
            .frames
            .get(self.slot())
            .map_or(Rect::ZERO, |f| f.rect)
    }

    /// 推进时间；返回**是否换了帧**。
    ///
    /// 方向图样模式下恒为 `false`：那条通路的帧号是 `set_dir/set_step` 的函数。
    pub fn tick(&mut self, dt: Duration) -> bool {
        if self.def.pattern.is_some() || self.paused || self.def.frames.len() < 2 {
            return false;
        }
        let rate = if self.speed.is_finite() {
            self.speed.max(0.0)
        } else {
            1.0
        };
        self.elapsed_ms += dt.as_secs_f32() * 1000.0 * rate;
        let mut changed = false;
        // 一次 `tick` 可以跨多帧（长渲染帧 / 低 fps），所以是循环而不是 if。
        while self.elapsed_ms >= self.frame_dur_ms() {
            self.elapsed_ms -= self.frame_dur_ms();
            if !self.advance_frame() {
                self.elapsed_ms = 0.0; // 到尾停住，别让余量无限攒着
                break;
            }
            changed = true;
        }
        changed
    }

    fn slot(&self) -> usize {
        (self.index as usize).min(self.def.frames.len().saturating_sub(1))
    }

    /// 把帧号重算成 `(dir, step)` 的投影；返回是否改变。非方向图样下是 no-op。
    fn sync_pattern_index(&mut self) -> bool {
        let Some(pattern) = self.def.pattern else {
            return false;
        };
        let next = pattern
            .frame_index(self.dir, self.step)
            .min(self.def.frame_count().saturating_sub(1));
        if next == self.index {
            return false;
        }
        self.index = next;
        true
    }

    fn frame_dur_ms(&self) -> f32 {
        self.def
            .frames
            .get(self.slot())
            .map_or(1, |f| f.dur_ms.max(1)) as f32
    }

    /// 走一帧；返回是否仍在推进（`false` = 到尾且循环方式不允许再走）。
    fn advance_frame(&mut self) -> bool {
        let last = self.def.frame_count().saturating_sub(1);
        match self.def.loop_ {
            LoopMode::Once => {
                if self.index >= last {
                    return false;
                }
                self.index += 1;
                true
            }
            LoopMode::Pingpong => {
                if last == 0 {
                    return false;
                }
                if self.step_dir > 0 {
                    if self.index >= last {
                        self.step_dir = -1;
                        self.index -= 1;
                    } else {
                        self.index += 1;
                    }
                } else if self.index == 0 {
                    self.step_dir = 1;
                    self.index = 1;
                } else {
                    self.index -= 1;
                }
                true
            }
            LoopMode::Loop(n) => {
                if n != 0 && self.cycles >= n {
                    return false;
                }
                if self.index >= last {
                    self.index = 0;
                    self.cycles += 1;
                } else {
                    self.index += 1;
                }
                true
            }
        }
    }
}

/// 把任意方向值收成 RPG Maker 的四个键值之一。
pub const fn normalize_dir(dir: u8) -> u8 {
    match dir {
        DIR_DOWN | DIR_LEFT | DIR_RIGHT | DIR_UP => dir,
        _ => DIR_DOWN,
    }
}

/// 朝向键值 → 单位位移（y 向下，与 `Sprite::pos` 同坐标系）。
///
/// 归一化后的四个值之外一律零向量：非法朝向不该让路线莫名漂走一格。
pub const fn dir_vector(dir: u8) -> Vec2 {
    match normalize_dir(dir) {
        DIR_DOWN => Vec2 { x: 0.0, y: 1.0 },
        DIR_LEFT => Vec2 { x: -1.0, y: 0.0 },
        DIR_RIGHT => Vec2 { x: 1.0, y: 0.0 },
        _ => Vec2 { x: 0.0, y: -1.0 },
    }
}

impl Sprite {
    /// 挂上/摘下动画。挂上即把当前帧写进 `src`，所以不存在"挂了动画但这一帧还画旧 `src`"
    /// 的中间态；摘下则 `src` 保持最后一次落位的样子（动画不负责还原）。
    pub fn set_animation(&mut self, anim: Option<AnimState>) {
        self.anim = anim;
        if let Some(state) = &self.anim {
            self.src = state.rect();
        }
    }

    /// 当前动画的朝向；无动画时 `None`（朝向是动画状态，不是精灵几何）。
    pub fn dir(&self) -> Option<u8> {
        self.anim.as_ref().map(AnimState::dir)
    }

    /// 当前动画的步帧；无动画时 `None`。
    pub fn step(&self) -> Option<u8> {
        self.anim.as_ref().map(AnimState::step)
    }

    /// 设朝向并把换掉的帧写回 `src`。返回是否换了帧；无动画时是 no-op。
    pub fn set_dir(&mut self, dir: u8) -> bool {
        self.apply_anim(|a| a.set_dir(dir))
    }

    /// 设步帧并把换掉的帧写回 `src`。返回是否换了帧；无动画时是 no-op。
    pub fn set_step(&mut self, step: u8) -> bool {
        self.apply_anim(|a| a.set_step(step))
    }

    /// 推进本精灵的时间驱动动画。
    pub fn tick_anim(&mut self, dt: Duration) -> bool {
        self.apply_anim(|a| a.tick(dt))
    }

    /// 三条动画入口共用的形状：状态变了就把当前帧落进 `src`。
    fn apply_anim(&mut self, f: impl FnOnce(&mut AnimState) -> bool) -> bool {
        let Some(anim) = self.anim.as_mut() else {
            return false;
        };
        let changed = f(anim);
        if changed {
            self.src = anim.rect();
        }
        changed
    }
}

impl SpriteArena {
    /// 每拍开头调一次：把全场精灵的 `prev_pos` 收拢到 `pos`。
    ///
    /// 必须在任何移动之前调（`App::tick` 的第一行），否则这一拍的位移会被当场抹掉。停拍
    /// 不推进 `prev_pos` 会让 [`Sprite::interpolated`] 把最后一跳重播一遍，见
    /// [`Sprite::begin_tick`]。
    pub fn begin_tick(&mut self) {
        for (_, sprite) in self.iter_live_mut() {
            sprite.begin_tick();
        }
    }

    /// 统一推进点：每 tick 调一次，把换掉的帧写回各精灵的 `src`。
    ///
    /// 返回**本拍换帧的精灵数**（0 = 全场静止，是"这一拍没白干"的直观读数）。
    pub fn tick_anims(&mut self, dt: Duration) -> u32 {
        let mut changed = 0;
        for (_, sprite) in self.iter_live_mut() {
            changed += sprite.tick_anim(dt) as u32;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // T12：`cargo xtask wasm-test` 在 wasm 上跑这一组（见 docs/platforms.md）。
    use crate::draw::ViewportRef;
    use crate::sprite::Vec2;
    use sisf_render::{Atlas, BatchList};
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    /// 每帧 120ms（时间驱动用例里才是"节奏"，方向图样用例只看它不被读）。
    const MS: u32 = 120;

    fn sheet(pattern: DirPattern) -> Arc<AnimDef> {
        Arc::new(AnimDef::grid_pattern(pattern, 24.0, 32.0, MS))
    }

    /// 等长的纯时间动画：`n` 帧、每帧 10ms。
    fn clock(frames: u32, dur_ms: u32, loop_: LoopMode) -> Arc<AnimDef> {
        Arc::new(AnimDef {
            frames: (0..frames)
                .map(|i| FrameDef::new(Rect::new(i as f32 * 8.0, 0.0, 8.0, 8.0), dur_ms))
                .collect(),
            loop_,
            pattern: None,
        })
    }

    /// 验收正题：两图样 × 四向的帧索引表。
    #[test]
    fn mv_4x3_indexes_every_direction_by_step() {
        let p = DirPattern::RpgMaker4x3;
        let expect = [
            (DIR_DOWN, [0u32, 1, 2]),
            (DIR_LEFT, [3, 4, 5]),
            (DIR_RIGHT, [6, 7, 8]),
            (DIR_UP, [9, 10, 11]),
        ];
        for (dir, frames) in expect {
            for (step, want) in frames.into_iter().enumerate() {
                assert_eq!(
                    p.frame_index(dir, step as u8),
                    want,
                    "MV 4×3 的 (dir {dir}, step {step}) 帧号"
                );
            }
        }
        assert_eq!(p.cell_count(), 12);
        assert_eq!(p.cols() * p.rows(), 12);
        // 越界的朝向退成"下"，步帧取模 ⇒ 永远不会走出图外。
        assert_eq!(p.frame_index(0, 7), p.frame_index(DIR_DOWN, 1));
    }

    #[test]
    fn ace_2x2_folds_the_third_step_into_the_first_column() {
        let p = DirPattern::RpgMaker2x2;
        let expect = [
            (DIR_DOWN, [0u32, 1, 0]),
            (DIR_LEFT, [2, 3, 2]),
            (DIR_RIGHT, [4, 5, 4]),
            (DIR_UP, [6, 7, 6]),
        ];
        for (dir, frames) in expect {
            for (step, want) in frames.into_iter().enumerate() {
                assert_eq!(
                    p.frame_index(dir, step as u8),
                    want,
                    "VX/Ace 2×2 的 (dir {dir}, step {step}) 帧号"
                );
            }
        }
        assert_eq!((p.cols(), p.cell_count()), (2, 8));
    }

    /// 同一套朝向语义，两档图样的行序必须一致（换 RTP 图不用改方向逻辑）。
    #[test]
    fn both_patterns_agree_on_direction_rows() {
        for (dir, row) in [(DIR_DOWN, 0u32), (DIR_LEFT, 1), (DIR_RIGHT, 2), (DIR_UP, 3)] {
            assert_eq!(DirPattern::dir_row(dir), row);
            assert_eq!(
                DirPattern::RpgMaker4x3.frame_index(dir, 0),
                row * 3,
                "第 0 步恒在该行第 0 列"
            );
            assert_eq!(DirPattern::RpgMaker2x2.frame_index(dir, 0), row * 2);
        }
        assert_eq!(normalize_dir(3), DIR_DOWN);
        assert_eq!(normalize_dir(DIR_UP), DIR_UP);
    }

    #[test]
    fn grid_frames_read_row_major() {
        let def = AnimDef::grid(24.0, 32.0, 3, 4, MS);
        assert_eq!(def.frame_count(), 12);
        assert_eq!(def.frames[0].rect, Rect::new(0.0, 0.0, 24.0, 32.0));
        assert_eq!(def.frames[3].rect, Rect::new(0.0, 32.0, 24.0, 32.0));
        assert_eq!(def.frames[11].rect, Rect::new(48.0, 96.0, 24.0, 32.0));
        assert_eq!(def.loop_, LoopMode::Loop(0));
        assert_eq!(def.pattern, None);
    }

    /// 验收正题：**静止不换帧**。方向图样下 `tick` 不推进，只有步帧变才换帧。
    #[test]
    fn movement_driven_frames_never_advance_on_their_own() {
        let mut anim = AnimState::new(sheet(DirPattern::RpgMaker4x3));
        anim.set_dir(DIR_UP);
        let rest = anim.rect();
        assert_eq!(rest, Rect::new(0.0, 96.0, 24.0, 32.0));
        for _ in 0..50 {
            assert!(
                !anim.tick(Duration::from_millis(MS as u64)),
                "方向图样不接受时间推进"
            );
            assert_eq!(anim.rect(), rest, "站着就该一直同一帧");
        }
        assert!(anim.set_step(1));
        assert_eq!(anim.rect(), Rect::new(24.0, 96.0, 24.0, 32.0));
        assert!(anim.set_step(2));
        assert_eq!(anim.rect(), Rect::new(48.0, 96.0, 24.0, 32.0));
        // 走完一圈回到站立帧：步帧归 0 ⇒ 帧号跟着归位。
        assert!(anim.set_step(0));
        assert_eq!(anim.rect(), rest);
        // 重复设同一值不产生变化（调用方可据此跳过 `src` 赋值）。
        assert!(!anim.set_step(0));
        assert!(!anim.set_dir(DIR_UP));
    }

    #[test]
    fn turning_and_stepping_compose_into_the_expected_cell() {
        let mut anim = AnimState::new(sheet(DirPattern::RpgMaker2x2));
        assert_eq!((anim.dir(), anim.step()), (DIR_DOWN, 0));
        assert!(anim.set_step(1));
        assert_eq!(anim.index, 1);
        assert!(anim.set_dir(DIR_RIGHT));
        assert_eq!(anim.index, 5, "右那一行的第 1 列");
        assert_eq!(anim.rect(), Rect::new(24.0, 64.0, 24.0, 32.0));
        assert_eq!((anim.dir(), anim.step()), (DIR_RIGHT, 1));
    }

    /// 帧表短于图样：夹在最后一格，不 panic 也不越界。
    #[test]
    fn short_frame_tables_clamp_instead_of_running_off_the_sheet() {
        let def = Arc::new(AnimDef {
            frames: AnimDef::grid(16.0, 16.0, 3, 1, MS).frames,
            loop_: LoopMode::Loop(0),
            pattern: Some(DirPattern::RpgMaker4x3),
        });
        let mut anim = AnimState::new(def);
        anim.set_dir(DIR_UP); // 想要第 9 格，表里只有 3 格 ⇒ 退回第 2 格
        assert_eq!(anim.index, 2);
        assert_eq!(anim.rect(), Rect::new(32.0, 0.0, 16.0, 16.0));
        // 空表也不 panic（画不出东西，但状态还在）。
        let empty = AnimState::new(Arc::new(AnimDef::new(Vec::new())));
        assert_eq!(empty.rect(), Rect::ZERO);
    }

    #[test]
    fn time_driven_anim_advances_and_stops_at_the_end() {
        let mut anim = AnimState::new(clock(4, 100, LoopMode::Once));
        assert!(!anim.finished());
        assert!(anim.tick(Duration::from_millis(100)));
        assert_eq!(anim.index, 1);
        // 一帧时间里跨了两格：一次 `tick` 走两帧，余量留在内部计时里。
        assert!(anim.tick(Duration::from_millis(250)));
        assert_eq!(anim.index, 3);
        assert!(anim.finished(), "Once 到尾");
        assert!(!anim.tick(Duration::from_millis(500)), "到尾后不再换帧");
        assert_eq!(anim.index, 3);
        assert_eq!(anim.rect(), Rect::new(24.0, 0.0, 8.0, 8.0));
    }

    #[test]
    fn loop_counts_cycles_and_zero_means_forever() {
        let mut anim = AnimState::new(clock(3, 10, LoopMode::Loop(2)));
        // 3 帧 × 2 圈 = 6 次推进；第 7 次起时间照给，帧号不再动。
        for _ in 0..10 {
            anim.tick(Duration::from_millis(10));
        }
        assert!(anim.finished(), "Loop(2) 该到尾");
        assert_eq!(anim.index, 0, "回环到尾停在第 0 格，不是末格");
        assert!(!anim.tick(Duration::from_millis(10)));

        let mut forever = AnimState::new(clock(3, 10, LoopMode::Loop(0)));
        for _ in 0..20 {
            forever.tick(Duration::from_millis(10));
        }
        assert!(!forever.finished(), "Loop(0) 永不到尾");
        assert!(forever.index < 3);
    }

    #[test]
    fn pingpong_bounces_without_repeating_the_ends() {
        let mut anim = AnimState::new(clock(3, 10, LoopMode::Pingpong));
        let mut trace = vec![anim.index];
        for _ in 0..8 {
            anim.tick(Duration::from_millis(10));
            if *trace.last().unwrap() != anim.index {
                trace.push(anim.index);
            }
        }
        assert_eq!(trace, vec![0, 1, 2, 1, 0, 1, 2, 1, 0], "端点不重复停留");
    }

    #[test]
    fn speed_and_pause_drive_the_clock() {
        let mut anim = AnimState::new(clock(4, 100, LoopMode::Loop(0)));
        anim.paused = true;
        anim.tick(Duration::from_secs(1));
        assert_eq!(anim.index, 0, "暂停时时间不推进");
        anim.paused = false;
        anim.speed = 4.0;
        anim.tick(Duration::from_millis(100));
        assert_eq!(anim.index, 0, "4 倍速下 100ms 正好走满一圈");
        anim.speed = f32::NAN;
        anim.tick(Duration::from_millis(100));
        assert_eq!(anim.index, 1, "NaN 倍率退回 1.0 而不是卡死");
        // 只有一帧的动画没什么可推进的。
        let mut single = AnimState::new(clock(1, 10, LoopMode::Loop(0)));
        assert!(!single.tick(Duration::from_secs(10)));
        assert_eq!(single.index, 0);
    }

    /// 单精灵挂载：挂上即对齐 `src`，`set_step/set_dir` 直接写回。
    #[test]
    fn attaching_an_anim_writes_the_current_frame_into_src() {
        let mut s = Sprite {
            pos: Vec2::new(4.0, 4.0),
            src: Rect::with_size(99.0, 99.0),
            ..Default::default()
        };
        assert!(!s.set_step(1), "无动画时是 no-op");
        assert_eq!(s.src, Rect::with_size(99.0, 99.0));
        assert_eq!((s.dir(), s.step()), (None, None));
        let mut anim = AnimState::new(sheet(DirPattern::RpgMaker4x3));
        anim.set_dir(DIR_LEFT);
        anim.set_step(2);
        s.set_animation(Some(anim));
        assert_eq!(s.src, Rect::new(48.0, 32.0, 24.0, 32.0));
        assert_eq!(s.dir(), Some(DIR_LEFT));
        assert!(s.set_step(0));
        assert_eq!(s.src, Rect::new(0.0, 32.0, 24.0, 32.0));
        assert!(!s.set_step(0));
        // 摘下动画后 `src` 保持最后一次落位的样子。
        s.set_animation(None);
        assert_eq!(s.src, Rect::new(0.0, 32.0, 24.0, 32.0));
        assert!(!s.set_dir(DIR_RIGHT));
    }

    /// 整场推进：只有时间驱动的精灵被计数，静止的一个都不动。
    #[test]
    fn arena_tick_applies_frames_only_to_sprites_that_changed() {
        let mut arena = SpriteArena::new();
        let walking = arena.spawn(Sprite {
            anim: Some(AnimState::new(sheet(DirPattern::RpgMaker4x3))),
            ..Default::default()
        });
        let idle = arena.spawn(Sprite {
            src: Rect::new(1.0, 2.0, 3.0, 4.0),
            ..Default::default()
        });
        let torch = arena.spawn(Sprite {
            anim: Some(AnimState::new(clock(3, 50, LoopMode::Loop(0)))),
            ..Default::default()
        });
        let before = arena.get(idle).unwrap().src;
        assert_eq!(
            arena.tick_anims(Duration::from_millis(60)),
            1,
            "只有火把换帧"
        );
        assert_eq!(arena.get(idle).unwrap().src, before, "没动画的精灵不该被碰");
        assert_eq!(arena.get(torch).unwrap().src, Rect::new(8.0, 0.0, 8.0, 8.0));
        // 行走图：时间推进一格都不动它，直到移动逻辑给步帧 —— 所以本拍的 1 只可能来自火把。
        let walking_rest = arena.get(walking).unwrap().src;
        assert_eq!(arena.tick_anims(Duration::from_millis(600)), 1);
        assert_eq!(
            arena.get(walking).unwrap().src,
            walking_rest,
            "600ms 也不该让行走图换帧"
        );
        assert!(arena.get_mut(walking).unwrap().set_step(1));
        assert_eq!(
            arena.get(walking).unwrap().src,
            Rect::new(24.0, 0.0, 24.0, 32.0)
        );
    }

    /// 收拢是"全场"的：每个活精灵的 `prev_pos` 都被拉到 `pos`，本拍真正动了的那个照旧留痕。
    #[test]
    fn arena_begin_tick_folds_every_live_sprite() {
        let mut arena = SpriteArena::new();
        let moved = arena.spawn(Sprite::default());
        let still = arena.spawn(Sprite::default());
        for h in [moved, still] {
            let s = arena.get_mut(h).unwrap();
            s.teleport(Vec2::new(8.0, 8.0));
            s.set_pos(Vec2::new(16.0, 8.0));
            assert_ne!(s.interpolated(0.0), s.pos, "上一跳还该看得见");
        }
        arena.begin_tick();
        arena.get_mut(moved).unwrap().set_pos(Vec2::new(24.0, 8.0));

        let still = arena.get(still).unwrap();
        assert_eq!(still.prev_pos, still.pos, "没动的精灵收拢成一点");
        assert_eq!(still.interpolated(0.0), still.interpolated(0.75));
        let moved = arena.get(moved).unwrap();
        assert_eq!(
            (moved.prev_pos, moved.pos),
            (Vec2::new(16.0, 8.0), Vec2::new(24.0, 8.0)),
            "收拢不能抹掉本拍的位移"
        );
    }

    /// 采集通路不感知动画：动画只通过 `src` 生效，所以 `uv` 是唯一被改变的输出。
    #[test]
    fn animated_sprites_collect_through_the_plain_path() {
        let bitmap = Atlas::new()
            .new_bitmap(96, 128, vec![0u8; 96 * 128 * 4], 0)
            .unwrap();
        let [px, py, _, _] = bitmap.rect();
        let (pw, ph) = bitmap.page_size();
        let uv = |x: u32, y: u32| [x as f32 / pw as f32, y as f32 / ph as f32];

        let mut arena = SpriteArena::new();
        let mut sprite = Sprite::new(bitmap);
        sprite.pos = Vec2::new(10.0, 10.0);
        // `Sprite::new` 把 `src` 铺满整图，挂上动画后立刻被第一格覆盖。
        sprite.set_animation(Some(AnimState::new(sheet(DirPattern::RpgMaker4x3))));
        let handle = arena.spawn(sprite);

        let mut first = BatchList::new();
        arena.collect(ViewportRef::MAIN, &mut first).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first.sorted_items()[0].1.uv0, uv(px, py));
        assert_eq!(
            first.sorted_items()[0].1.size,
            [24.0, 32.0],
            "一格的尺寸就是目的尺寸"
        );

        assert!(arena.get_mut(handle).unwrap().set_dir(DIR_UP));
        let mut second = BatchList::new();
        arena.collect(ViewportRef::MAIN, &mut second).unwrap();
        assert_eq!(
            second.sorted_items()[0].1.uv0,
            uv(px, py + 96),
            "换了帧 ⇒ 采样点走到第 3 行"
        );
        assert_eq!(
            first.sorted_items()[0].1.pos,
            second.sorted_items()[0].1.pos,
            "换帧不改落点（那是 `pos` 的事）"
        );
    }
}
