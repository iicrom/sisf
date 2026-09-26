//! 逐 tick 的任务：调度器（[`TaskScheduler`]）+ 关键帧补间（[`Tween`]）+ 移动路线
//! （[`MoveRoute`]）。
//!
//! 为什么是"逐 tick"而不是"逐帧"：本模块的时间单位是 `sisf-app` 的 `TickClock` 产出的
//! **固定拍**，一拍多长由 `tick_hz` 决定、与渲染帧率无关。任务因此只用整数拍记进度 ——
//! 补间在第 12 拍到位、路线在第 30 拍走完，在任何刷新率下都一样。这就是验收那句
//! "Tween 完成回调按 tick 计数"的意思。
//!
//! 三条约定：
//!
//! - **一 tick 只推进一个节点**。`Turn`、`FacingLock` 这类瞬时节点也占一拍，路线时长
//!   因此完全可预测（= 各节点拍数之和），不会出现"这一拍顺手做完了两件事"。
//! - **任务绑定一个精灵**（[`SpriteRef`]）。精灵被回收后任务收到 `None`，应当自我了断
//!   （返回 `true`）而不是留着空转。
//! - **队列**：默认各任务并行推进；[`Task::blocks`] 为 `true` 的任务（`MoveRoute` 的
//!   `wait: true`）独占该精灵的队列 —— 排在它后面的任务要等它结束才起跑。这正是 RPG
//!   Maker 事件指令里"等待移动完成"的形状。

use crate::anim::{DIR_DOWN, DIR_LEFT, DIR_RIGHT, DIR_UP, dir_vector, normalize_dir};
use crate::arena::{SpriteArena, SpriteRef};
use crate::sprite::{Sprite, Vec2};

/// 可被 [`TaskScheduler`] 逐拍推进的东西。
pub trait Task {
    /// 推进一个 tick。`sprite` 为 `None` = 绑定的精灵已不在 arena 里。
    /// 返回 `true` = 本任务结束，调度器随即摘掉它。
    fn step(&mut self, sprite: Option<&mut Sprite>) -> bool;

    /// 是否独占所属精灵的任务队列（直到自己结束）。默认 `false` = 与兄弟任务并行。
    fn blocks(&self) -> bool {
        false
    }
}

/// 任务调度器：`(精灵, 任务)` 列表 + 每拍一次的统一推进点。
#[derive(Default)]
pub struct TaskScheduler {
    entries: Vec<Entry>,
    /// 已推进的拍数（诊断与测试都好用：任务只按它计数）。
    ticks: u64,
}

struct Entry {
    sprite: SpriteRef,
    task: Box<dyn Task>,
}

impl TaskScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    /// 排入一个任务。**同一精灵的任务按提交顺序排队**（队列语义见模块头）。
    pub fn schedule(&mut self, sprite: SpriteRef, task: impl Task + 'static) {
        self.entries.push(Entry {
            sprite,
            task: Box::new(task),
        });
    }

    /// 推进一 tick：按提交顺序逐个 `step`，结束即摘掉。返回本拍结束的任务数。
    pub fn tick(&mut self, arena: &mut SpriteArena) -> usize {
        self.ticks += 1;
        let mut busy: Vec<SpriteRef> = Vec::new();
        let mut done = 0;
        let mut i = 0;
        while i < self.entries.len() {
            let sprite = self.entries[i].sprite;
            // 该精灵的队头还在占位 ⇒ 后面的任务这一拍不起跑（顺序因此得以保持）。
            if busy.contains(&sprite) {
                i += 1;
                continue;
            }
            let entry = &mut self.entries[i];
            let finished = entry.task.step(arena.get_mut(sprite));
            if finished {
                done += 1;
                // `remove` 而非 `swap_remove`：保序 = 保队列语义。
                self.entries.remove(i);
            } else {
                if entry.task.blocks() {
                    busy.push(sprite);
                }
                i += 1;
            }
        }
        done
    }

    /// 已推进的拍数。
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// 在排任务数（含被队列挡住的）。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 某精灵是否还有任务（含排队中）。
    pub fn pending(&self, sprite: SpriteRef) -> bool {
        self.entries.iter().any(|e| e.sprite == sprite)
    }

    /// 摘掉某精灵的全部任务（路线被打断、事件消失时用）。返回摘掉的数量。
    pub fn cancel(&mut self, sprite: SpriteRef) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| e.sprite != sprite);
        before - self.entries.len()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// [`Tween`] 的取值域：`f32`（透明度、缩放）与 [`Vec2`]（坐标）覆盖了本阶段的需求。
pub trait Lerp {
    fn mix(from: &Self, to: &Self, t: f32) -> Self;
}

impl Lerp for f32 {
    fn mix(from: &Self, to: &Self, t: f32) -> Self {
        from + (to - from) * t
    }
}

impl Lerp for Vec2 {
    fn mix(from: &Self, to: &Self, t: f32) -> Self {
        from.lerp(*to, t)
    }
}

/// 缓动曲线：`[0,1] → [0,1]`。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ease {
    #[default]
    Linear,
    /// 慢起步（`t²`）。
    InQuad,
    /// 慢收尾（`1-(1-t)²`）。
    OutQuad,
    /// 两头都慢。
    InOutQuad,
}

impl Ease {
    /// 输入先夹进 `[0,1]`，所以曲线永不越界（补间值因此不会冲出关键帧两端）。
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::InQuad => t * t,
            Self::OutQuad => 1.0 - (1.0 - t) * (1.0 - t),
            Self::InOutQuad => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
                }
            }
        }
    }
}

/// 一个关键帧：在第 `at` 拍抵达 `value`，`ease` 描述**如何进入**这一帧。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Keyframe<T> {
    pub at: u32,
    pub value: T,
    pub ease: Ease,
}

impl<T> Keyframe<T> {
    pub const fn new(at: u32, value: T) -> Self {
        Self {
            at,
            value,
            ease: Ease::Linear,
        }
    }

    pub const fn eased(at: u32, value: T, ease: Ease) -> Self {
        Self { at, value, ease }
    }
}

/// 关键帧补间：把 `keys` 上的值按拍插出来，用 `apply` 闭包写进精灵。
///
/// 补间哪个字段由闭包决定（`|s, v| s.opacity = *v`、`|s, v| s.set_pos(*v)`），本类型
/// 因此不必为每个可补间的字段开一个变体。走 [`Sprite::set_pos`] 时补间位移会参与渲染
/// 插值，画面是连续的。
///
/// [`Tween`] 自己不认识 arena：它是被 [`TaskScheduler`] 驱动的一个任务，完成回调
/// [`on_finish`](Self::on_finish) 在最后一拍、值已落位之后调用一次。
/// `Tween` 的写值闭包：把这一拍插出来的值落进精灵（补间哪个字段是闭包的事）。
type Apply<T> = Box<dyn FnMut(&mut Sprite, &T)>;
/// `Tween` 的完成回调（最后一拍、值已落位之后调用一次）。
type Finish = Box<dyn FnOnce(&mut Sprite)>;

pub struct Tween<T> {
    keys: Vec<Keyframe<T>>,
    /// 已走过的拍数（`step` 先加后用，所以第 1 拍结束时 `elapsed == 1`）。
    elapsed: u32,
    apply: Apply<T>,
    on_finish: Option<Finish>,
}

impl<T: Lerp + Clone> Tween<T> {
    /// `keys` 需按 `at` 升序（分段查找按此假设；单帧表 = 落一次值即结束）。
    pub fn new(keys: Vec<Keyframe<T>>, apply: impl FnMut(&mut Sprite, &T) + 'static) -> Self {
        Self {
            keys,
            elapsed: 0,
            apply: Box::new(apply),
            on_finish: None,
        }
    }

    /// 跑完时的一次性回调（任务完成即被摘掉，所以只可能调用一次）。
    pub fn on_finish(mut self, f: impl FnOnce(&mut Sprite) + 'static) -> Self {
        self.on_finish = Some(Box::new(f));
        self
    }

    /// 总时长 = 最后一帧的拍号。空表或单帧为 0 ⇒ 第一拍即结束。
    pub fn total(&self) -> u32 {
        self.keys.last().map_or(0, |k| k.at)
    }

    pub fn elapsed(&self) -> u32 {
        self.elapsed
    }

    /// 第 `at` 拍的值（纯函数：单测与"跳到某一拍"的调试都用它）。
    pub fn value_at(&self, at: u32) -> T {
        let first = &self.keys[0];
        if at <= first.at {
            return first.value.clone();
        }
        for pair in self.keys.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if at <= b.at {
                let span = b.at.saturating_sub(a.at).max(1) as f32;
                return T::mix(&a.value, &b.value, b.ease.apply((at - a.at) as f32 / span));
            }
        }
        self.keys.last().expect("非空").value.clone()
    }
}

impl<T: Lerp + Clone + 'static> Task for Tween<T> {
    fn step(&mut self, sprite: Option<&mut Sprite>) -> bool {
        self.elapsed += 1;
        let Some(sprite) = sprite else {
            return true;
        };
        if self.keys.is_empty() {
            return true;
        }
        // 单帧表（`total == 0`）也走这条路径：落一次值即收工。
        let value = self.value_at(self.elapsed.min(self.total()));
        (self.apply)(sprite, &value);
        if self.elapsed >= self.total() {
            if let Some(f) = self.on_finish.take() {
                f(sprite);
            }
            return true;
        }
        false
    }
}

/// 路线节点（RPG Maker `MoveRoute.list` 的本阶段子集）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RouteNode {
    /// 原地停 `ticks` 拍（步帧归 0）。
    Wait { ticks: u32 },
    /// 转身一拍（不位移）。
    Turn { dir: u8 },
    /// 朝 `dir` 走 `cells` 格（自由模式下 `cells` 的单位是 1 像素，见 [`MoveRoute::free`]）。
    Move { dir: u8, cells: u32 },
    /// 走到 `(x, y)`（虚拟像素）。网格模式逐格逼近（目标应落在格点上），自由模式走直线。
    MoveTo { x: f32, y: f32 },
    /// 朝 `dir` 跳 `cells` 格：腾空期间不换步帧、**不查阻挡**（跳过坑与桌台），落地即格点。
    Jump { dir: u8, cells: u32 },
    /// 朝向锁：`true` = 之后的移动不再自动转身（RPG Maker 的 Direction Fix）。
    FacingLock { locked: bool },
}

/// 路线的空间模型。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RouteMode {
    /// 网格（RPG）：一次一跳、逐格查阻挡。位置恒落在同一格点阵上（起点由调用方对齐，
    /// 路线只加 `tile` 的整数倍，不会把它带偏）。
    Grid { tile: f32 },
    /// 自由（SLG）：连续坐标，`MoveTo` 走直线。
    Free,
}

/// 一条移动路线：节点表 + 速度 + 阻挡策略，作为 [`Task`] 逐拍推进。
///
/// 阻挡判定交给 [`walkable`](Self::walkable) 的钩子（通行表在游戏逻辑那边），所以本类型
/// 不依赖任何地图数据结构。[`through`](Self::through) = 无视阻挡继续走（RPG Maker 的
/// Through）。撞墙的后果是**本节点剩余格数作废**，路线继续执行下一节点 —— 事件不该因为
/// 一扇门就整个卡死。
pub struct MoveRoute {
    nodes: Vec<RouteNode>,
    mode: RouteMode,
    /// 每拍位移（虚拟像素）。一格耗时 `ceil(unit / speed)` 拍。
    speed: f32,
    through: bool,
    wait: bool,
    walkable: Option<Box<dyn FnMut(f32, f32) -> bool>>,
    /// 正在执行的节点下标（`nodes.len()` ⇒ 跑完）。
    node: usize,
    /// 当前节点的剩余工作量：`Wait` 是拍数，`Move`/`Jump` 是格数。
    left_units: u32,
    /// 进行中的这一跳还差多少像素；`0` = 下一拍要先起一跳。
    left_px: f32,
    /// 本跳的行进方向（单位向量）。
    unit: Vec2,
    facing_locked: bool,
}

impl std::fmt::Debug for MoveRoute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MoveRoute")
            .field("nodes", &self.nodes)
            .field("mode", &self.mode)
            .field("speed", &self.speed)
            .field("through", &self.through)
            .field("wait", &self.wait)
            .field("node", &self.node)
            .field("facing_locked", &self.facing_locked)
            .finish_non_exhaustive()
    }
}

impl MoveRoute {
    /// 网格模式路线（RPG 逐格）。`speed` = 每拍位移的像素数，一格耗时
    /// `ceil(tile / speed)` 拍。
    pub fn grid(nodes: Vec<RouteNode>, tile: f32, speed: f32) -> Self {
        Self::with_mode(nodes, RouteMode::Grid { tile }, speed)
    }

    /// 自由模式路线（SLG 连续坐标）。`Move` 的 `cells` 单位是 1 像素。
    pub fn free(nodes: Vec<RouteNode>, speed: f32) -> Self {
        Self::with_mode(nodes, RouteMode::Free, speed)
    }

    fn with_mode(nodes: Vec<RouteNode>, mode: RouteMode, speed: f32) -> Self {
        // 每拍至少挪 1px：0 会让一格永远走不完，配置写错不该有"路线卡死"这种后果。
        let speed = if speed.is_finite() && speed > 1.0 {
            speed
        } else {
            1.0
        };
        let mut route = Self {
            nodes,
            mode,
            speed,
            through: false,
            wait: false,
            walkable: None,
            node: 0,
            left_units: 0,
            left_px: 0.0,
            unit: Vec2::ZERO,
            facing_locked: false,
        };
        route.enter_node();
        route
    }

    /// 无视阻挡（RPG Maker 的 Through）。
    pub fn through(mut self, through: bool) -> Self {
        self.through = through;
        self
    }

    /// 独占该精灵的任务队列直到路线跑完（见 [`Task::blocks`]）。
    pub fn wait(mut self, wait: bool) -> Self {
        self.wait = wait;
        self
    }

    /// 阻挡判定：`(x, y)` 能不能落脚。未设置 = 哪里都能去。
    pub fn walkable<F>(mut self, f: F) -> Self
    where
        F: FnMut(f32, f32) -> bool + 'static,
    {
        self.walkable = Some(Box::new(f));
        self
    }

    pub fn mode(&self) -> RouteMode {
        self.mode
    }

    /// 正在执行第几个节点（`nodes.len()` = 已跑完）。
    pub fn node(&self) -> usize {
        self.node
    }

    /// 当前节点剩下的格/拍数（诊断用）。
    pub fn remaining(&self) -> u32 {
        self.left_units
    }

    fn finished(&self) -> bool {
        self.node >= self.nodes.len()
    }

    /// 一跳的单位距离（自由模式下 `Move` 的单位是 1 像素）。
    fn unit_distance(&self) -> f32 {
        match self.mode {
            RouteMode::Grid { tile } => tile,
            RouteMode::Free => 1.0,
        }
    }

    fn is_grid(&self) -> bool {
        matches!(self.mode, RouteMode::Grid { .. })
    }

    /// 节点切换：按节点自身内容登记工作量。
    fn enter_node(&mut self) {
        self.left_px = 0.0;
        self.unit = Vec2::ZERO;
        self.left_units = match self.nodes.get(self.node) {
            Some(RouteNode::Wait { ticks }) => *ticks,
            Some(RouteNode::Move { cells, .. }) | Some(RouteNode::Jump { cells, .. }) => *cells,
            _ => 0,
        };
    }

    fn advance_node(&mut self) {
        self.node += 1;
        self.enter_node();
    }

    /// 落点是否可去（`through` 与未设钩子都放行）。
    fn passable(&mut self, at: Vec2) -> bool {
        self.through || self.walkable.as_mut().is_none_or(|f| f(at.x, at.y))
    }

    /// 起一跳：定朝向（未锁时写回精灵）+ 单位向量。
    fn aim(&mut self, sprite: &mut Sprite, dir: u8) {
        let dir = normalize_dir(dir);
        if !self.facing_locked {
            sprite.set_dir(dir);
        }
        self.unit = dir_vector(dir);
    }

    /// 走完一格 ⇒ 换步帧（跳跃不走这里，所以腾空时恒为站立帧）。
    ///
    /// 序列是 `0 → 1 → 2 → 1 → 2 …`：站立帧只在停住或换向时出现，两列图样（VX/Ace）
    /// 下 `2` 会折回第 0 列，得到 1/0 交替的两帧步行动画。
    fn step_frame(&mut self, sprite: &mut Sprite) {
        let next = match sprite.step().unwrap_or(0) {
            0 | 2 => 1,
            _ => 2,
        };
        sprite.set_step(next);
    }

    /// 走一拍：`speed` 像素（不足则补到本跳剩余）。返回 `true` = 这一跳走完了。
    fn hop_tick(&mut self, sprite: &mut Sprite) -> bool {
        let step = self.speed.min(self.left_px);
        sprite.set_pos(sprite.pos + self.unit * step);
        self.left_px -= step;
        if self.left_px > 0.0 {
            return false;
        }
        self.left_px = 0.0;
        true
    }

    /// 路线收尾：步帧归零（站住）。
    fn finish(&mut self, sprite: &mut Sprite) -> bool {
        sprite.set_step(0);
        true
    }

    /// `Move` / `Jump`。
    fn walk(&mut self, sprite: &mut Sprite, dir: u8, jump: bool) {
        if self.left_px == 0.0 {
            if self.left_units == 0 {
                self.advance_node();
                return;
            }
            self.aim(sprite, dir);
            let hop = self.unit_distance();
            // 网格模式：起跳前查落点（跳跃永不查 —— 它的意义就是跳过障碍）。
            if !jump && self.is_grid() && !self.passable(sprite.pos + self.unit * hop) {
                self.advance_node();
                return;
            }
            self.left_units -= 1;
            self.left_px = hop;
        }
        if !self.hop_tick(sprite) {
            return;
        }
        if !jump {
            self.step_frame(sprite);
        }
        if self.left_units == 0 {
            self.advance_node();
        }
    }

    /// 网格 `MoveTo`：每跳挑差值较大的那个轴，逐格逼近。
    fn move_to_grid(&mut self, sprite: &mut Sprite, goal: Vec2, tile: f32) {
        if self.left_px == 0.0 {
            let delta = goal - sprite.pos;
            let (dx, dy) = ((delta.x / tile).round(), (delta.y / tile).round());
            if dx == 0.0 && dy == 0.0 {
                self.advance_node();
                return;
            }
            let dir = if dx.abs() >= dy.abs() {
                if dx > 0.0 { DIR_RIGHT } else { DIR_LEFT }
            } else if dy > 0.0 {
                DIR_DOWN
            } else {
                DIR_UP
            };
            self.aim(sprite, dir);
            let landing = sprite.pos + self.unit * tile;
            if !self.passable(landing) {
                self.advance_node();
                return;
            }
            self.left_px = tile;
        }
        if self.hop_tick(sprite) {
            self.step_frame(sprite);
        }
    }

    /// 自由 `MoveTo`：走直线，末拍补到目标点。
    fn move_to_free(&mut self, sprite: &mut Sprite, goal: Vec2) {
        let delta = goal - sprite.pos;
        let dist = delta.length();
        if dist <= self.speed {
            sprite.set_pos(goal);
            sprite.set_step(0);
            self.advance_node();
            return;
        }
        if self.left_px == 0.0 {
            self.unit = delta * (1.0 / dist);
            self.left_px = dist;
            if !self.facing_locked {
                sprite.set_dir(dir_of(self.unit));
            }
        }
        if self.hop_tick(sprite) {
            self.step_frame(sprite);
        }
    }
}

impl Task for MoveRoute {
    fn step(&mut self, sprite: Option<&mut Sprite>) -> bool {
        let Some(sprite) = sprite else {
            return true;
        };
        if self.finished() {
            return self.finish(sprite);
        }
        match self.nodes[self.node] {
            RouteNode::Wait { .. } => {
                sprite.set_step(0);
                self.left_units = self.left_units.saturating_sub(1);
                if self.left_units == 0 {
                    self.advance_node();
                }
            }
            RouteNode::Turn { dir } => {
                sprite.set_dir(dir);
                sprite.set_step(0);
                self.advance_node();
            }
            RouteNode::FacingLock { locked } => {
                self.facing_locked = locked;
                self.advance_node();
            }
            RouteNode::Move { dir, .. } => self.walk(sprite, dir, false),
            RouteNode::Jump { dir, .. } => self.walk(sprite, dir, true),
            RouteNode::MoveTo { x, y } => match self.mode {
                RouteMode::Grid { tile } => self.move_to_grid(sprite, Vec2::new(x, y), tile),
                RouteMode::Free => self.move_to_free(sprite, Vec2::new(x, y)),
            },
        }
        // 节点表在这一拍跑完 ⇒ **当场**收尾。留一拍"下一趟才报告完成"看着无害，实际是每格
        // 白烧一拍：那一拍 `prev_pos`/`pos` 原地不动，而 `TickClock` 照旧消耗一个周期把
        // `alpha` 归零，于是 `interpolated(alpha)` 把最后一步重播一遍（画面退回一步再走回来），
        // 且整条路线的平均速度低成 `tile/(ceil(tile/speed)+1)` 拍。
        if self.finished() {
            return self.finish(sprite);
        }
        false
    }

    fn blocks(&self) -> bool {
        self.wait && !self.finished()
    }
}

/// 由单位向量挑最接近的四个朝向之一（自由模式的动画朝向）。
fn dir_of(unit: Vec2) -> u8 {
    if unit.x.abs() >= unit.y.abs() {
        if unit.x >= 0.0 { DIR_RIGHT } else { DIR_LEFT }
    } else if unit.y >= 0.0 {
        DIR_DOWN
    } else {
        DIR_UP
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // T12：`cargo xtask wasm-test` 在 wasm 上跑这一组（见 docs/platforms.md）。
    use crate::anim::{AnimDef, AnimState, DirPattern};
    use crate::sprite::Rect;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    const TILE: f32 = 32.0;

    /// 走路中的精灵：挂着 4×3 行走图（格 32×48），位置从原点起。
    fn walker(arena: &mut SpriteArena) -> SpriteRef {
        arena.spawn(Sprite {
            anim: Some(AnimState::new(Arc::new(AnimDef::grid_pattern(
                DirPattern::RpgMaker4x3,
                TILE,
                48.0,
                0,
            )))),
            ..Default::default()
        })
    }

    fn run(sched: &mut TaskScheduler, arena: &mut SpriteArena, ticks: u32) {
        for _ in 0..ticks {
            sched.tick(arena);
        }
    }

    /// 验收正题：完成回调按 tick 计数 —— 第 12 拍准点触发，且只触发一次。
    #[test]
    fn tween_completion_is_counted_in_ticks() {
        let tick_no = Rc::new(RefCell::new(0u32));
        let log = Rc::new(RefCell::new(Vec::new()));
        let tween = Tween::new(
            vec![Keyframe::new(0, 1.0f32), Keyframe::new(12, 0.0)],
            |s, v| s.opacity = *v,
        )
        .on_finish({
            let (tick_no, log) = (tick_no.clone(), log.clone());
            move |_| log.borrow_mut().push(*tick_no.borrow())
        });
        let mut arena = SpriteArena::new();
        let handle = arena.spawn(Sprite::default());
        let mut sched = TaskScheduler::new();
        sched.schedule(handle, tween);
        let mut values = Vec::new();
        for _ in 0..20 {
            *tick_no.borrow_mut() = sched.ticks() as u32 + 1;
            sched.tick(&mut arena);
            values.push(arena.get(handle).unwrap().opacity);
        }
        assert_eq!(*log.borrow(), vec![12], "回调恰在第 12 拍跑过一次");
        assert_eq!(values[5], 0.5, "第 6 拍在中点");
        assert_eq!(values[11], 0.0, "第 12 拍到末值");
        assert_eq!(&values[12..], &vec![0.0; 8], "结束之后不再被写");
        assert_eq!(sched.ticks(), 20);
    }

    #[test]
    fn tween_interpolates_its_keys_and_eases_each_segment() {
        let tween = Tween::new(
            vec![
                Keyframe::new(0, 0.0f32),
                Keyframe::new(4, 100.0),
                Keyframe::eased(8, 0.0, Ease::InQuad),
            ],
            |_: &mut Sprite, _: &f32| {},
        );
        assert_eq!(tween.total(), 8);
        assert_eq!(tween.value_at(2), 50.0, "线性段：两拍走一半");
        assert_eq!(tween.value_at(4), 100.0, "关键帧准点取准值");
        // 第二段用 `InQuad` 进入：t=0.5 → 0.25 ⇒ 100 + (0-100)*0.25。
        assert_eq!(tween.value_at(6), 75.0);
        assert_eq!(tween.value_at(99), 0.0, "超出总长就停在末帧");
        assert_eq!(tween.value_at(0), 0.0);
    }

    #[test]
    fn easing_curves_stay_inside_their_interval() {
        for ease in [Ease::Linear, Ease::InQuad, Ease::OutQuad, Ease::InOutQuad] {
            assert_eq!(ease.apply(0.0), 0.0);
            assert_eq!(ease.apply(1.0), 1.0);
            assert_eq!(ease.apply(-3.0), 0.0, "输入先夹住，曲线不越界");
            assert_eq!(ease.apply(9.0), 1.0);
            assert!(ease.apply(0.5) >= 0.0 && ease.apply(0.5) <= 1.0);
        }
        assert!(
            Ease::InQuad.apply(0.5) < Ease::OutQuad.apply(0.5),
            "慢起步 vs 慢收尾"
        );
    }

    #[test]
    fn a_single_key_tween_lands_once_and_finishes() {
        let mut arena = SpriteArena::new();
        let handle = arena.spawn(Sprite::default());
        let mut sched = TaskScheduler::new();
        sched.schedule(
            handle,
            Tween::new(vec![Keyframe::new(0, 0.5f32)], |s, v| s.opacity = *v),
        );
        assert_eq!(sched.tick(&mut arena), 1);
        assert_eq!(arena.get(handle).unwrap().opacity, 0.5);
        assert!(sched.is_empty());
    }

    #[test]
    fn a_task_on_a_despawned_sprite_ends_itself() {
        let mut arena = SpriteArena::new();
        let handle = walker(&mut arena);
        arena.despawn(handle);
        let mut sched = TaskScheduler::new();
        sched.schedule(
            handle,
            Tween::new(
                vec![Keyframe::new(0, 1.0f32), Keyframe::new(9, 0.0)],
                |_, _| {},
            ),
        );
        assert_eq!(sched.tick(&mut arena), 1, "精灵没了就自我了断，不该空转");
        assert!(sched.is_empty());
    }

    /// 验收正题：网格路线逐格移动（32px 格、每拍 8px ⇒ 4 拍一格）。
    #[test]
    fn move_route_walks_cell_by_cell() {
        let mut arena = SpriteArena::new();
        let handle = walker(&mut arena);
        let mut sched = TaskScheduler::new();
        sched.schedule(
            handle,
            MoveRoute::grid(vec![RouteNode::Move { dir: 6, cells: 3 }], TILE, 8.0),
        );
        let mut seen = Vec::new();
        for _ in 0..14 {
            sched.tick(&mut arena);
            let s = arena.get(handle).unwrap();
            seen.push((s.pos.x, s.dir().unwrap(), s.step().unwrap()));
        }
        assert_eq!(seen[0], (8.0, DIR_RIGHT, 0), "第一拍只往前走，还没换帧");
        assert_eq!(seen[3], (32.0, DIR_RIGHT, 1), "4 拍一格，进格才换步帧");
        assert_eq!(seen[7], (64.0, DIR_RIGHT, 2));
        assert_eq!(
            seen[10],
            (88.0, DIR_RIGHT, 2),
            "步帧循环 1↔2，走路途中不踩站立帧"
        );
        assert_eq!(
            seen[11],
            (96.0, DIR_RIGHT, 0),
            "走完最后一格的那一拍当场收尾站住"
        );
        assert_eq!(seen[12], (96.0, DIR_RIGHT, 0), "收尾拍不再白烧一拍");
        assert_eq!(arena.get(handle).unwrap().pos.x, 96.0);
        // 换帧落到 `src` 上：右那一行的第 0 列（采集通路对动画一无所知）。
        assert_eq!(
            arena.get(handle).unwrap().src,
            Rect::new(0.0, 96.0, 32.0, 48.0)
        );
        assert!(sched.is_empty());
    }

    #[test]
    fn walls_void_the_rest_of_a_node_but_not_the_route() {
        // 格心 x=64 是墙：`Move 4 格` 在第 2 格前贴墙停住，后面的 `Turn` 照常执行。
        let wall = |x: f32, _y: f32| (x - 64.0).abs() >= f32::EPSILON;
        let mut arena = SpriteArena::new();
        let handle = walker(&mut arena);
        let mut sched = TaskScheduler::new();
        sched.schedule(
            handle,
            MoveRoute::grid(
                vec![
                    RouteNode::Move { dir: 6, cells: 4 },
                    RouteNode::Turn { dir: DIR_UP },
                ],
                TILE,
                TILE,
            )
            .walkable(wall),
        );
        run(&mut sched, &mut arena, 6);
        let s = arena.get(handle).unwrap();
        assert_eq!(s.pos.x, 32.0, "撞墙那一格没走成");
        assert_eq!(s.dir(), Some(DIR_UP), "墙不该把后面的指令一起掐掉");
        assert_eq!(s.step(), Some(0));

        // `through` = RPG Maker 的 Through：同一堵墙直接穿过去。
        let mut arena2 = SpriteArena::new();
        let h2 = walker(&mut arena2);
        let mut sched2 = TaskScheduler::new();
        sched2.schedule(
            h2,
            MoveRoute::grid(vec![RouteNode::Move { dir: 6, cells: 4 }], TILE, TILE)
                .walkable(wall)
                .through(true),
        );
        run(&mut sched2, &mut arena2, 6);
        assert_eq!(arena2.get(h2).unwrap().pos.x, 128.0);
    }

    /// 验收正题：`wait: true` 让路线活到跑完，后面的任务被队列挡住。
    #[test]
    fn a_waiting_route_keeps_the_sprite_queue_alive() {
        let mut arena = SpriteArena::new();
        let handle = walker(&mut arena);
        let mut sched = TaskScheduler::new();
        sched.schedule(
            handle,
            MoveRoute::grid(vec![RouteNode::Wait { ticks: 3 }], TILE, 1.0).wait(true),
        );
        sched.schedule(
            handle,
            Tween::new(
                vec![Keyframe::new(0, 0.0f32), Keyframe::new(2, 1.0)],
                |s, v| s.opacity = *v,
            ),
        );
        let mut trace = Vec::new();
        for _ in 0..8 {
            sched.tick(&mut arena);
            let s = arena.get(handle).unwrap();
            trace.push((s.pos.x, s.opacity));
        }
        assert_eq!(trace[0].1, 1.0, "排队中的补间连一次都没写");
        assert_eq!(trace[1].1, 1.0);
        assert_eq!(trace[2].1, 0.5, "队头松开那一拍补间才起跑");
        assert!(trace.iter().all(|&(x, _)| x == 0.0), "Wait 不位移");
        assert!(sched.is_empty(), "两条都跑完了");
    }

    #[test]
    fn a_non_waiting_route_runs_alongside_its_sibling() {
        let mut arena = SpriteArena::new();
        let handle = walker(&mut arena);
        let mut sched = TaskScheduler::new();
        sched.schedule(
            handle,
            MoveRoute::grid(vec![RouteNode::Wait { ticks: 4 }], TILE, 1.0).wait(false),
        );
        sched.schedule(
            handle,
            Tween::new(
                vec![Keyframe::new(0, 0.0f32), Keyframe::new(2, 1.0)],
                |s, v| s.opacity = *v,
            ),
        );
        sched.tick(&mut arena);
        assert_eq!(arena.get(handle).unwrap().opacity, 0.5, "不等待 ⇒ 并行推进");
        assert!(sched.pending(handle));
    }

    /// 停拍不变量在任务层的形状（"红色人物移动后人物抖"）：走完一格进入 `Wait` 之后，
    /// **被画出来的点**（[`Sprite::interpolated`]）必须逐拍恒定。
    ///
    /// 路线只管位移，`prev_pos` 得由每拍开头的 [`SpriteArena::begin_tick`] 收拢；少了那一行，
    /// `Wait` 的每一拍都把最后一跳留在 `prev_pos`/`pos` 之间，被 `alpha` 的 `0→1` 来回重播。
    #[test]
    fn a_wait_node_draws_the_same_point_every_frame() {
        let mut arena = SpriteArena::new();
        let handle = walker(&mut arena);
        let mut sched = TaskScheduler::new();
        sched.schedule(
            handle,
            MoveRoute::grid(
                vec![
                    RouteNode::Move {
                        dir: DIR_RIGHT,
                        cells: 1,
                    },
                    RouteNode::Wait { ticks: 6 },
                ],
                TILE,
                TILE,
            )
            .wait(true),
        );

        let mut drawn = Vec::new();
        for _ in 0..8 {
            arena.begin_tick();
            sched.tick(&mut arena);
            let s = arena.get(handle).unwrap();
            drawn.push((
                s.interpolated(0.0),
                s.interpolated(0.5),
                s.interpolated(1.0),
            ));
        }
        // 对照组：第一拍确实跳了一格（否则"恒定"是白测）。
        assert_eq!((drawn[0].0.x, drawn[0].2.x), (0.0, 32.0), "第一拍没有位移");
        for (i, &(a, b, c)) in drawn[1..].iter().enumerate() {
            assert!(
                a == b && b == c && a.x == 32.0,
                "Wait 第 {} 拍画面上还在动：{a:?} / {b:?} / {c:?}",
                i + 1
            );
        }
    }

    #[test]
    fn grid_move_to_hops_axis_by_axis_until_it_arrives() {
        let mut arena = SpriteArena::new();
        let handle = walker(&mut arena);
        let mut sched = TaskScheduler::new();
        sched.schedule(
            handle,
            MoveRoute::grid(vec![RouteNode::MoveTo { x: 96.0, y: 64.0 }], TILE, 16.0),
        );
        run(&mut sched, &mut arena, 20);
        let s = arena.get(handle).unwrap();
        assert_eq!((s.pos.x, s.pos.y), (96.0, 64.0), "3 格宽 2 格高都要走到");
        assert_eq!(s.step(), Some(0));
        assert_eq!(s.src, Rect::new(0.0, 0.0, 32.0, 48.0), "收尾是朝下站立帧");
    }

    #[test]
    fn free_mode_goes_straight_and_snaps_to_the_goal() {
        let mut arena = SpriteArena::new();
        let handle = walker(&mut arena);
        let mut sched = TaskScheduler::new();
        sched.schedule(
            handle,
            MoveRoute::free(vec![RouteNode::MoveTo { x: 30.0, y: 40.0 }], 12.5),
        );
        sched.tick(&mut arena);
        let s = arena.get(handle).unwrap();
        assert!(
            (s.pos.x - 7.5).abs() < 1e-4 && (s.pos.y - 10.0).abs() < 1e-4,
            "斜线：位置不在轴上，{s:?}"
        );
        assert_eq!(
            s.dir(),
            Some(DIR_DOWN),
            "朝向取最接近的四向之一（|dy| > |dx|）"
        );
        run(&mut sched, &mut arena, 6);
        let s = arena.get(handle).unwrap();
        assert_eq!((s.pos.x, s.pos.y), (30.0, 40.0), "末拍补到目标点");
        assert_eq!(s.step(), Some(0));
    }

    #[test]
    fn a_jump_clears_a_gap_without_stepping() {
        let mut arena = SpriteArena::new();
        let handle = walker(&mut arena);
        arena.get_mut(handle).unwrap().set_step(1);
        let mut sched = TaskScheduler::new();
        sched.schedule(
            handle,
            // 全程不可去：跳跃不查阻挡，这正是它存在的意义（跳过坑与桌台）。
            MoveRoute::grid(
                vec![RouteNode::Jump {
                    dir: DIR_UP,
                    cells: 2,
                }],
                TILE,
                16.0,
            )
            .walkable(|_x, _y| false),
        );
        run(&mut sched, &mut arena, 6);
        let s = arena.get(handle).unwrap();
        assert_eq!(s.pos.y, -64.0, "两格落地");
        assert_eq!(s.dir(), Some(DIR_UP));
        assert_eq!(s.step(), Some(0), "腾空不算步帧");
    }

    #[test]
    fn facing_lock_moves_without_turning_the_graphic() {
        let mut arena = SpriteArena::new();
        let handle = walker(&mut arena);
        arena.get_mut(handle).unwrap().set_dir(DIR_LEFT);
        let mut sched = TaskScheduler::new();
        sched.schedule(
            handle,
            MoveRoute::grid(
                vec![
                    RouteNode::FacingLock { locked: true },
                    RouteNode::Move {
                        dir: DIR_RIGHT,
                        cells: 1,
                    },
                    RouteNode::FacingLock { locked: false },
                    RouteNode::Move {
                        dir: DIR_RIGHT,
                        cells: 1,
                    },
                ],
                TILE,
                TILE,
            ),
        );
        run(&mut sched, &mut arena, 2);
        let s = arena.get(handle).unwrap();
        assert_eq!(
            (s.pos.x, s.dir()),
            (32.0, Some(DIR_LEFT)),
            "锁着 ⇒ 走而不转身"
        );
        run(&mut sched, &mut arena, 2);
        let s = arena.get(handle).unwrap();
        assert_eq!(
            (s.pos.x, s.dir()),
            (64.0, Some(DIR_RIGHT)),
            "解锁后照旧转身"
        );
    }

    #[test]
    fn turn_and_wait_cost_one_tick_each() {
        let mut arena = SpriteArena::new();
        let handle = walker(&mut arena);
        let mut sched = TaskScheduler::new();
        sched.schedule(
            handle,
            MoveRoute::grid(
                vec![
                    RouteNode::Turn { dir: DIR_UP },
                    RouteNode::Wait { ticks: 2 },
                ],
                TILE,
                TILE,
            ),
        );
        sched.tick(&mut arena);
        assert_eq!(arena.get(handle).unwrap().dir(), Some(DIR_UP));
        assert_eq!(
            arena.get(handle).unwrap().src,
            Rect::new(0.0, 144.0, 32.0, 48.0),
            "上 = 第 3 行（格高 48）"
        );
        // Turn(1) + Wait(2) = 3 拍：第 3 拍**当场**报告完成并被摘掉。旧写法要多烧一拍才报告
        // 完成，而那一拍 `prev_pos`/`pos` 原地不动、`alpha` 照旧归零重数，`interpolated(alpha)`
        // 于是把最后一步重播一遍 —— 跟随相机下抖的正是整张地图（见 tiled_map 的节拍回放单测）。
        assert_eq!(sched.tick(&mut arena), 0);
        assert_eq!(sched.tick(&mut arena), 1, "3 拍跑完，不多烧收尾拍");
        assert!(sched.is_empty(), "完成的那一拍就该摘掉");
        assert_eq!(
            arena.get(handle).unwrap().pos,
            Vec2::ZERO,
            "整条路线没动过位置"
        );
    }

    #[test]
    fn cancel_drops_only_that_sprites_tasks() {
        let mut arena = SpriteArena::new();
        let handle = walker(&mut arena);
        let other = arena.spawn(Sprite::default());
        let mut sched = TaskScheduler::new();
        sched.schedule(
            handle,
            MoveRoute::grid(vec![RouteNode::Wait { ticks: 9 }], TILE, 1.0).wait(true),
        );
        sched.schedule(
            handle,
            Tween::new(vec![Keyframe::new(0, 1.0f32)], |_, _| {}),
        );
        sched.schedule(
            other,
            Tween::new(vec![Keyframe::new(0, 0.25f32)], |s, v| s.opacity = *v),
        );
        assert_eq!(sched.cancel(handle), 2);
        assert!(!sched.pending(handle));
        assert_eq!(sched.len(), 1);
        sched.tick(&mut arena);
        assert_eq!(
            arena.get(other).unwrap().opacity,
            0.25,
            "别人的任务不受牵连"
        );
        assert!(!sched.pending(other));
    }
}
