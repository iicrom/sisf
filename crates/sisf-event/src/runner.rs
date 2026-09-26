//! 游戏事件解释器：把 [`Command`] 序列跑成副作用。
//!
//! ## 一拍的形状
//!
//! [`EventRunner::tick`] 就是"事件机的一拍"，一次调用推进**恰好一拍**（拍长由
//! `sisf-app` 的 `TickClock` 决定，与帧率无关）。内部顺序是有讲究的：
//!
//! ```text
//! (a) 脚本后端先说话（它能启动程序，也能改精灵）
//! (b) 推进任务调度器（补间与路线走一拍）
//! (c) 解挂：把总线里的事件喂给当前挂起的原因，能解就解
//! (d) 执行指令，直到挂起 / 栈空 / 预算用完
//! (e) 同步 UI（消息窗与黑幕，只在"和上一次推给 UI 的不一样"时才推）
//! ```
//!
//! (b) 在 (c) 之前 ⇒ 一条"跑完最后一格"的路线在同一拍里就被观察到，`wait: true` 的移动
//! 不会白等一拍。(e) 放最后 ⇒ 游戏侧永远在本拍的状态之上画，看不到中间态。
//!
//! ## 谁驱动调度器：事件机
//!
//! [`TaskScheduler`](sisf_sprite::task) 也在 `EventCtx` 里，但游戏侧
//! **不该**自己 `tasks.tick`：一拍推进一次的职责归 [`EventRunner::tick`]。两处都推 = 路线
//! 跑得比逻辑快两倍，而且这种错没有任何报错信息。游戏侧只负责每拍开头
//! [`SpriteArena::begin_tick`](sisf_sprite::SpriteArena::begin_tick)（`prev_pos` 不变量，
//! 见 T8 第五条"抖"）。
//!
//! ## 挂起与预算
//!
//! 需要外部条件的指令（文字、按键、等待、移动、淡入淡出）把 `pc` **留在下一条**，然后
//! 把"在等什么"记进 `Hold`（私有枚举）：这样解挂之后不会重跑那条指令（重跑一个 `ShowText` 等于把
//! 玩家刚按下的确认吃掉）。[`RunnerStatus::OverBudget`] 只在指令边界出现，所以一个没有
//! `Wait` 的 `Loop` 最多吃掉 `budget` 条指令，下一拍从同一个 `pc` 接着跑 —— 死循环写错了
//! 也只是"这一拍慢了"，不会把窗口冻住。
//!
//! ## 总线：唯一消费者是它
//!
//! 游戏侧在 `App::on_event` 里把切片原样转进 `ctx.bus`（见 `Runner::deliver` 之后总线已空
//! 这件事），事件机是这条队列的唯一读者。凡是"这批不是我等的"事件都按原序放回（见 `Batch`），
//! 所以按键序列不会因为有个对话框开着就丢失。
//!
//! [`Command`]: crate::command::Command

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use sisf_asset::Vfs;
use sisf_sprite::task::{Keyframe, MoveRoute, RouteNode, TaskScheduler, Tween};
use sisf_sprite::{SpriteArena, SpriteRef, Tone, Vec2};

use crate::bus::{ActionEdge, EngineBus, EngineEvent, KeyKind};
use crate::command::{
    AnimEffect, Command, EventScript, FadeTarget, Instr, Program, RouteSpec, RouteStep, Slot,
    Sound, SoundChannel,
};
use crate::script::{ScriptBackend, Submission};
use crate::vars::VarStore;

/// 决定/取消/上下移动的默认动作名（与 [`crate::input::ActionMap::rpg_maker`] 同一套）。
///
/// 写死在这里是有意的：动作名可以换，但"选项靠上下 + 决定"这件事必须有个缺省，否则每个
/// 事件表都要重复声明一遍键位。
pub mod actions {
    pub const OK: &str = "ok";
    pub const CANCEL: &str = "cancel";
    pub const UP: &str = "up";
    pub const DOWN: &str = "down";
}

/// 音频端口：把指令里的 `Play*`/`Fade` 落到实际设备上。
///
/// **为什么是 trait 而不是直接持有 `AudioBus`**：`AudioBus` 只在开了 pump feature
/// （`native`/`web`）时才存在，把它写进 `EventCtx` 就等于让"无音频的构建"变成编译错误，
/// 而那是 `check-all` 的一整组。所以这里只提要求，游戏侧用一个 `impl` 把它接到 `AudioBus`
/// 上（`examples/dialog_demo.rs` 里那份就是最小实现），没有音频时 `play` 返回一个原因字符串
/// 让事件机登记成告警 —— 看不见声音时"为什么没声"是要说得出来的。
///
/// 实现方的义务只有一条：**同一个 `name` 只解码一次**。`Sound` 里的名字是 VFS 路径，
/// 每圈重解一份 2s OGG 足以把输出环抽干（`todo.md` T7 事实第 3 条）。
pub trait AudioPort {
    /// 起播。`Err(原因)` 只登记告警，不中断事件。
    fn play(&mut self, vfs: &Vfs, channel: SoundChannel, sound: &Sound) -> Result<(), String>;

    /// `frames` 拍长的淡入淡出（拍→毫秒换算归实现方，它才知道自己的拍频）。
    fn fade(&mut self, target: FadeTarget, frames: u32, out: bool) -> Result<(), String>;

    /// 关掉某条通道（`Erase`/`stop` 之类，端口没有停播需求时实现成空操作并说明）。
    fn stop(&mut self, channel: SoundChannel);
}

/// 消息窗与黑幕端口（T6 的 `windows` 那一套东西在事件机这边的把手）。
///
/// 契约：**UI 状态是事件机状态的函数**。事件机只在内容变化时 `show`/`set_dim`，所以实现方
/// 不必自己去猜"这帧该不该重画"，只要把收到的 `Prompt` 原样落进自己的位图。
pub trait UiPort {
    /// 推一版消息窗内容（`choices` 非空 ⇒ 按 `active` 画高亮）。
    fn show(&mut self, prompt: &Prompt);
    /// 收掉消息窗。
    fn hide(&mut self);
    /// 黑幕量 `0..=1`（1 = 全黑）。事件机按拍推，实现方在 `draw` 里盖一层。
    fn set_dim(&mut self, amount: f32);
}

/// 一版消息窗的内容。
#[derive(Clone, Debug, PartialEq)]
pub struct Prompt {
    pub speaker: Option<String>,
    pub body: String,
    pub choices: Vec<String>,
    /// 高亮下标；`usize::MAX` = 没有选项（普通消息）。
    pub active: usize,
    pub allow_cancel: bool,
}

impl Prompt {
    /// 光标可走几格（普通消息 = 0，此时上下键不该有反应）。
    pub fn cursor_count(&self) -> usize {
        self.choices.len()
    }

    pub fn prev_choice(&self) -> usize {
        let n = self.cursor_count();
        if n == 0 {
            return self.active;
        }
        (self.active + n - 1) % n
    }

    pub fn next_choice(&self) -> usize {
        let n = self.cursor_count();
        if n == 0 {
            return self.active;
        }
        (self.active + 1) % n
    }

    /// 确认时写回 `result` 的值：选项 ⇒ 从 0 起的序号；普通消息 ⇒ 0（"看完了"这件事本身
    /// 就是结果，写 -1 会让"取消"和"看完"分不开）。
    pub fn choice_value(&self) -> i64 {
        if self.choices.is_empty() {
            0
        } else {
            self.active as i64
        }
    }
}

/// 一批事件 + 一张"哪些被事件机吃掉了"的位图。
///
/// 为什么要位图而不是边扫边搬：`match &events[i]` 的借用会一路活着，把同一轮里的
/// `events.push`/`into_iter` 判成冲突；而**顺序必须保持**（松开在按下之后是常态，反过来
/// 就是丢按键）。
struct Batch {
    events: Vec<EngineEvent>,
    keep: Vec<bool>,
}

/// 事件机关心的那四个命名动作（其余动作名一律"不是我的"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Named {
    Up,
    Down,
    Ok,
    Cancel,
}

impl Batch {
    fn new(ctx: &mut EventCtx) -> Self {
        let events = ctx.take_events();
        let keep = vec![true; events.len()];
        Self { events, keep }
    }

    fn len(&self) -> usize {
        self.events.len()
    }

    /// 第 `i` 条是不是四个关心的动作之一的 `Press` 沿。
    fn named_action(&self, i: usize) -> Option<Named> {
        use EngineEvent::Action;
        match &self.events[i] {
            Action { name, edge } if *edge == ActionEdge::Press => match name.as_str() {
                actions::UP => Some(Named::Up),
                actions::DOWN => Some(Named::Down),
                actions::OK => Some(Named::Ok),
                actions::CANCEL => Some(Named::Cancel),
                _ => None,
            },
            _ => None,
        }
    }

    /// 第 `i` 条是一次**带可打印字符**的按下：返回要写进槽的数字（非数字字符 = 0，
    /// 只算"按下了"）。
    ///
    /// 控制字符（`\r`、`\t`、方向键那类没有字符的按下）不算答案，两条理由：
    ///
    /// - 它们不是"报上来的数"。文本输入归选项与 `ch`，数字输入归 `InputKey`。
    /// - 它们正是上一句 `ShowText` 用来"继续"的那一次按下 —— 那次按下解完挂后会按原序放回
    ///   总线，若这里也认它，同一个 Enter 就既关掉了上一句、又答完了下一题：浏览器里
    ///   `examples/dialog_demo.rs` 的井边人因此永远停在"太小了"，任何数字都进不来。
    fn key_press(&self, i: usize) -> Option<i64> {
        match &self.events[i] {
            EngineEvent::Key {
                kind: KeyKind::Press,
                ch: Some(c),
                ..
            } if !c.is_control() => Some(c.to_digit(10).map_or(0, i64::from)),
            _ => None,
        }
    }

    fn consume(&mut self, i: usize) {
        self.keep[i] = false;
    }

    /// 没被吃掉的按原序放回总线（下一拍、以及游戏侧的其他读者都还看得见它们）。
    fn flush(self, ctx: &mut EventCtx) {
        for (e, keep) in self.events.into_iter().zip(self.keep) {
            if keep {
                ctx.bus.push(e);
            }
        }
    }
}

/// 世界状态：actor id → 精灵，加网格与阻挡判定。
///
/// 事件里的 `MoveRoute{actor: 3}` 说的是"3 号角色"，而 arena 说的是"slot 7 generation 2"。
/// 这层映射必须有人持有：放进 arena 会让 `sisf-sprite` 关心游戏语义，放进指令里会让存档
/// 写死 slot 号（删了又建就不对了），所以放这里 —— 它是**世界**的一部分，不是事件的一部分。
#[derive(Default)]
pub struct Scene {
    actors: BTreeMap<u32, SpriteRef>,
    walkable: Option<Rc<dyn Fn(f32, f32) -> bool>>,
    /// 网格边长（虚拟像素）；`≤ 0` = 自由模式（连续坐标，无逐格阻挡）。
    tile: f32,
}

impl std::fmt::Debug for Scene {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scene")
            .field("actors", &self.actors)
            .field("tile", &self.tile)
            .field("walkable", &self.walkable.is_some())
            .finish()
    }
}

impl Scene {
    /// 网格世界（RPG 逐格），`tile` = 一格多少虚拟像素。
    pub fn grid(tile: f32) -> Self {
        Self {
            tile,
            ..Self::default()
        }
    }

    /// 自由世界（SLG）：路线走直线、不查逐格阻挡。
    pub fn free() -> Self {
        Self {
            tile: 0.0,
            ..Self::default()
        }
    }

    pub fn tile(&self) -> f32 {
        self.tile
    }

    pub fn set(&mut self, actor: u32, sprite: SpriteRef) {
        self.actors.insert(actor, sprite);
    }

    pub fn actor(&self, actor: u32) -> Option<SpriteRef> {
        self.actors.get(&actor).copied()
    }

    /// 摘掉一个 actor（`Erase` 用）；没登记过就返回 `None`。
    pub fn remove(&mut self, actor: u32) -> Option<SpriteRef> {
        self.actors.remove(&actor)
    }

    /// 阻挡判定：`(x, y)` 能不能落脚。没设置 = 哪里都能去。
    pub fn set_walkable<F>(&mut self, f: F)
    where
        F: Fn(f32, f32) -> bool + 'static,
    {
        self.walkable = Some(Rc::new(f));
    }

    pub fn walkable(&self, x: f32, y: f32) -> bool {
        match &self.walkable {
            Some(f) => f(x, y),
            None => true,
        }
    }

    /// 阻挡判定的共享句柄（`MoveRoute` 要 `'static` 闭包，所以共享而不是拷表）。
    fn walkable_handle(&self) -> Option<Rc<dyn Fn(f32, f32) -> bool>> {
        self.walkable.clone()
    }
}

/// 事件机每拍能看到的一切（可变的与世界都在这儿）。
///
/// 字段全公开、按 [`crate`] 的文档形状对齐清单：`{ sprites, tasks, audio, vars, bus, vfs, ui }`
/// 之外多了 `scene` —— 清单里的 `MoveRoute`/`Animate`/`Erase` 都要按 actor id 找精灵，那层
/// 映射总得有个地方放（见 [`Scene`] 的理由）。
pub struct EventCtx<'a> {
    pub sprites: &'a mut SpriteArena,
    /// 任务调度器：**由 `EventRunner::tick` 推进**，游戏侧不要再 `tick` 一次。
    pub tasks: &'a mut TaskScheduler,
    pub scene: &'a mut Scene,
    pub audio: &'a mut dyn AudioPort,
    pub ui: &'a mut dyn UiPort,
    pub vars: &'a mut VarStore,
    pub bus: &'a mut EngineBus,
    /// 只读：`Sound` 里的名字靠它解析成字节，交给 [`AudioPort::play`]。
    pub vfs: &'a Vfs,
}

impl EventCtx<'_> {
    /// 取空总线（事件机是唯一消费者）。取走的东西要么用掉、要么由 `Batch::flush`（私有）放回。
    pub fn take_events(&mut self) -> Vec<EngineEvent> {
        self.bus.drain()
    }
}

/// 当前在等什么。一条即可：同一时刻只有一条指令链挂起（见模块文档）。
enum Hold {
    /// 消息窗/选项：等 `ok`（有选项则写回高亮下标）。
    Text {
        prompt: Prompt,
        result: Option<Slot>,
        cursor_se: Option<Sound>,
    },
    /// 等一个按键；`left == 0` = 不限时。
    Key {
        slot: Slot,
        left: u32,
    },
    Wait {
        left: u32,
    },
    /// 等某 actor 的任务队列清空（`MoveRoute`/`Animate` 的 `wait: true`）。
    Task {
        actor: u32,
    },
    /// 屏幕淡入淡出：`from`/`to` 是黑幕量，`left` 每拍 -1。
    Fade {
        left: u32,
        total: u32,
        from: f32,
        to: f32,
    },
}

/// 一拍的结果。游戏侧靠它决定"玩家能不能动"。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunnerStatus {
    /// 指令栈空且没有挂起 ⇒ 世界归玩家。
    Idle { executed: u32 },
    /// 挂在外部条件上。`reason` 是 ASCII 令牌（`text`/`key`/`wait`/`task`/`fade`，见
    /// `EventRunner::hold_variant`（私有）），游戏侧可以直接比较它；"玩家为什么动不了"当场就能答。
    Suspended { executed: u32, reason: &'static str },
    /// 预算用完，停在指令边界 ⇒ 下一拍从同一个 `pc` 继续。
    OverBudget { executed: u32 },
}

impl RunnerStatus {
    pub fn executed(self) -> u32 {
        match self {
            Self::Idle { executed }
            | Self::Suspended { executed, .. }
            | Self::OverBudget { executed } => executed,
        }
    }

    pub fn is_idle(self) -> bool {
        matches!(self, Self::Idle { .. })
    }

    /// 挂起原因（不是挂起状态就返回 `None`）。
    pub fn suspended(self) -> Option<&'static str> {
        match self {
            Self::Suspended { reason, .. } => Some(reason),
            _ => None,
        }
    }
}

/// 一个执行单元：程序 + 位置 + 归属事件。
struct Frame {
    /// 归属的 actor id（`Erase` 与 `MoveRoute` 的默认对象）。
    subject: u32,
    /// 事件页表；`None` = 脚本后端交来的程序（没有页可跳）。
    pages: Option<Arc<EventScript>>,
    prog: Arc<Program>,
    page: usize,
    pc: usize,
}

/// 调用栈深度上限：脚本递归 `Custom` 调自己不该把栈撑破。
pub const MAX_FRAMES: usize = 32;

/// 事件虚拟机。
pub struct EventRunner {
    frames: Vec<Frame>,
    hold: Option<Hold>,
    /// 黑幕当前值（事件机持有，UI 只是镜像）。
    dim: f32,
    shown: Option<Prompt>,
    shown_dim: f32,
    backend: Option<Box<dyn ScriptBackend>>,
    warnings: Vec<String>,
    warned: BTreeMap<String, ()>,
}

impl Default for EventRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for EventRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventRunner")
            .field("frames", &self.frames.len())
            .field("pc", &self.frames.last().map(|fr| (fr.page, fr.pc)))
            .field("hold", &self.hold_variant())
            .field("dim", &self.dim)
            .finish_non_exhaustive()
    }
}

impl EventRunner {
    /// 没有脚本后端的事件机：`Custom` 指令会登记告警后跳过，每拍也不会有后端插话。
    pub fn new() -> Self {
        Self {
            frames: Vec::new(),
            hold: None,
            dim: 0.0,
            shown: None,
            shown_dim: 0.0,
            backend: None,
            warnings: Vec::new(),
            warned: BTreeMap::new(),
        }
    }

    /// 装上脚本后端（换掉已有的）。缺省就是不装 —— 那样连每拍一次的方法调用都省了。
    pub fn with_script(mut self, backend: Box<dyn ScriptBackend>) -> Self {
        self.backend = Some(backend);
        self
    }

    pub fn set_script(&mut self, backend: Option<Box<dyn ScriptBackend>>) {
        self.backend = backend;
    }

    /// 进入场景时转告后端（没装就是空操作）。`entry` 的词汇表由后端自己定义。
    pub fn start_script(&mut self, entry: &str) {
        if let Some(b) = self.backend.as_mut() {
            b.start(entry);
        }
    }

    /// 启动一个事件的某一页。`false` = 页号越界（登记告警，不启动）。
    pub fn start(&mut self, subject: u32, script: Arc<EventScript>, page: usize) -> bool {
        let Some(prog) = script.pages.get(page) else {
            self.warn(format!("事件 {} 没有第 {} 页", script.name, page));
            return false;
        };
        // 先拷程序再交页表：`pages` 与 `prog` 都要活在这帧里，而 `script` 只能移动一次。
        let prog = Arc::new(prog.clone());
        self.frames.push(Frame {
            subject,
            pages: Some(script),
            prog,
            page,
            pc: 0,
        });
        true
    }

    /// 直接启动一段程序（脚本后端与单测的路径；没有页可跳）。
    pub fn start_program(&mut self, subject: u32, prog: Arc<Program>) {
        self.push_frame(subject, prog, None, 0);
    }

    fn push_frame(
        &mut self,
        subject: u32,
        prog: Arc<Program>,
        pages: Option<Arc<EventScript>>,
        page: usize,
    ) {
        if self.frames.len() >= MAX_FRAMES {
            self.warn(format!(
                "调用栈已到 {MAX_FRAMES} 层，拒绝再起一个新程序（多半是脚本自己递归调自己）"
            ));
            return;
        }
        self.frames.push(Frame {
            subject,
            pages,
            prog,
            page,
            pc: 0,
        });
    }

    /// 有没有事件占着世界（在跑或在等）。玩家移动该看这个。
    pub fn busy(&self) -> bool {
        !self.frames.is_empty() || self.hold.is_some()
    }

    pub fn is_idle(&self) -> bool {
        !self.busy()
    }

    /// 当前屏幕消息（`None` = 没开窗）。`draw` 不必读它 —— UI 已经是镜像 —— 它给单测与
    /// 调试面板用。
    pub fn prompt(&self) -> Option<&Prompt> {
        match &self.hold {
            Some(Hold::Text { prompt, .. }) => Some(prompt),
            _ => None,
        }
    }

    /// 黑幕当前值（`0..=1`）。
    pub fn dim(&self) -> f32 {
        self.dim
    }

    /// 是不是在**等玩家按键**（`ShowText` 的选项与 `InputKey` 两种挂起）。
    ///
    /// 存在的理由：游戏侧往 `ctx.bus` 里灌事件，而事件机只在等的时候才读它。没人等的时候
    /// 队列只会越攒越长（一个开着对话框的下午 = 几万个陈年 `Focused`），所以游戏侧要拿这个
    /// 判断"这一拍的按键该不该留下"。等待任务/等待拍数不算，它们不吃输入。
    pub fn waiting_input(&self) -> bool {
        matches!(&self.hold, Some(Hold::Text { .. }) | Some(Hold::Key { .. }))
    }

    /// 某事件当前跑到第几页（`None` = 不在跑）。`Jump{page}` 之后游戏侧靠它同步表现。
    pub fn page_of(&self, subject: u32) -> Option<usize> {
        self.frames
            .iter()
            .find(|f| f.subject == subject)
            .map(|f| f.page)
    }

    /// 取走攒下的告警（画面与 stdout 各打一份，端口自己决定）。同一句话只登记一次：
    /// 一个每拍都踩到的配置错误刷屏，等于没有告警。
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    fn warn(&mut self, msg: impl Into<String>) {
        let msg: String = msg.into();
        if self.warned.insert(msg.clone(), ()).is_none() {
            self.warnings.push(msg);
        }
    }

    /// 挂在什么上面。五个取值：`text`（文字/选项）、`key`（等按键）、`wait`（等拍数）、
    /// `task`（等路线或补间）、`fade`（等淡入淡出）。
    ///
    /// 为什么是 ASCII 令牌而不是一句中文：它是给游戏侧 `match`、给调试面板当键用的**标识**，
    /// 而调试面板要读的那几行恰好出现在最需要它的 wasm 手测里 —— 那边中文有字形与否取决于
    /// 运行期取没取到 CJK 字体（`docs/text.md` 平台矩阵），取不到时中文就是一排方块。
    fn hold_variant(&self) -> &'static str {
        match &self.hold {
            None => "none",
            Some(Hold::Text { .. }) => "text",
            Some(Hold::Key { .. }) => "key",
            Some(Hold::Wait { .. }) => "wait",
            Some(Hold::Task { .. }) => "task",
            Some(Hold::Fade { .. }) => "fade",
        }
    }

    /// 推进一拍。见模块文档的顺序。
    pub fn tick(&mut self, ctx: &mut EventCtx, budget: u32) -> RunnerStatus {
        // (a) 后端先说话：它要起的事件从下一段起就能跑。
        let mut queue: Vec<Submission> = Vec::new();
        if let Some(b) = self.backend.as_mut() {
            b.tick(ctx, &mut queue);
        }
        for (subject, prog) in queue {
            self.start_program(subject, prog);
        }

        // (b) 补间与路线走一拍。
        ctx.tasks.tick(&mut *ctx.sprites);

        // (c) 解挂。
        if self.hold.is_some() {
            self.resolve(ctx);
        }

        // (d) 执行指令。
        let mut executed = 0u32;
        let limit = budget.max(1);
        loop {
            if self.hold.is_some() {
                break;
            }
            let pc = match self.frames.last() {
                Some(f) if f.pc < f.prog.code.len() => f.pc,
                Some(_) => {
                    // 本程序跑到尾：调用帧弹回去接着走，栈空则事件结束。
                    self.frames.pop();
                    continue;
                }
                None => break,
            };
            if executed >= limit {
                self.sync_ui(ctx);
                return RunnerStatus::OverBudget { executed };
            }
            // 取一条就算一条：回边和 `Erase` 这种"没有副作用但确实跑了"的指令也要记账，
            // 否则一个纯 `Loop {}` 的死循环能把预算检查绕过去。
            executed += 1;
            // 程序是 `Arc`，取一条指令只要加一次引用计数 —— 指令里带着 `Vec<String>`
            // 这类东西，每取一条就深拷一份是不可接受的开销。
            let prog = Arc::clone(&self.frames.last().expect("上面刚确认").prog);
            match &prog.code[pc] {
                Instr::Branch { cond, on_false } => {
                    let taken = ctx.vars.test(cond);
                    self.frames.last_mut().expect("同上").pc =
                        if taken { pc + 1 } else { *on_false };
                }
                Instr::Goto { to } | Instr::LoopBack { to } => {
                    self.frames.last_mut().expect("同上").pc = *to;
                }
                Instr::Run(cmd) => {
                    if !self.exec(ctx, cmd, pc) {
                        // `Erase`/`Jump` 越界这类"本链到此为止"：pc 已经不该再往前走。
                        continue;
                    }
                }
            }
        }

        // (e) UI 只在变化时收消息。
        self.sync_ui(ctx);
        // 走到这里只有两种可能：挂在外部条件上，或者栈已经空了（超预算那条在循环里返回）。
        match &self.hold {
            Some(_) => RunnerStatus::Suspended {
                executed,
                reason: self.hold_variant(),
            },
            None => RunnerStatus::Idle { executed },
        }
    }

    /// 解挂：把总线里的事件喂给"当前在等什么"，能解就解。
    ///
    /// 一次性 `take` 再按需装回去，是为了让每个分支都能自由改 `prompt`/`dim` 而不和
    /// `&mut self.hold` 打架。
    fn resolve(&mut self, ctx: &mut EventCtx) {
        let Some(hold) = self.hold.take() else {
            return;
        };
        match hold {
            Hold::Wait { left } => {
                if left > 1 {
                    self.hold = Some(Hold::Wait { left: left - 1 });
                }
            }
            Hold::Fade {
                left,
                total,
                from,
                to,
            } => {
                let left = left - 1;
                // 线性：`left` 从 `total` 数到 0，黑幕从 `from` 走到 `to`。数到 0 那一拍
                // 正好落在 `to`，所以淡出结束时屏幕是满黑，不会差最后一步。
                self.dim = to + (from - to) * (left as f32 / total as f32);
                if left > 0 {
                    self.hold = Some(Hold::Fade {
                        left,
                        total,
                        from,
                        to,
                    });
                }
            }
            Hold::Task { actor } => {
                let done = match ctx.scene.actor(actor) {
                    // actor 被摘掉了（`Erase` 或场景切换）就当它跑完了：等待方不该因为
                    // 被等的那个消失而永远挂在这里。
                    Some(sprite) => !ctx.tasks.pending(sprite),
                    None => true,
                };
                if !done {
                    self.hold = Some(Hold::Task { actor });
                }
            }
            Hold::Text {
                mut prompt,
                result,
                cursor_se,
            } => {
                let mut batch = Batch::new(ctx);
                let mut done: Option<i64> = None;
                let mut moved = false;
                for i in 0..batch.len() {
                    if done.is_some() {
                        // 决定已经下了：这一批剩下的（含同一拍里的后续按键）原样退回。
                        break;
                    }
                    let hit = batch.named_action(i);
                    match hit {
                        Some(Named::Up) if prompt.cursor_count() > 1 => {
                            prompt.active = prompt.prev_choice();
                            moved = true;
                            batch.consume(i);
                        }
                        Some(Named::Down) if prompt.cursor_count() > 1 => {
                            prompt.active = prompt.next_choice();
                            moved = true;
                            batch.consume(i);
                        }
                        Some(Named::Ok) => {
                            done = Some(prompt.choice_value());
                            batch.consume(i);
                        }
                        Some(Named::Cancel) if prompt.allow_cancel => {
                            done = Some(-1);
                            batch.consume(i);
                        }
                        _ => {}
                    }
                }
                batch.flush(ctx);
                // 光标音走 SE 通道：它该盖在 BGM 上，也不该被 BGM 的淡入淡出牵动。
                if moved
                    && let Some(se) = &cursor_se
                    && let Err(why) = ctx.audio.play(ctx.vfs, SoundChannel::Se, se)
                {
                    self.warn(format!("光标音起播失败: {why}"));
                }
                match done {
                    Some(value) => {
                        if let Some(slot) = &result {
                            ctx.vars.set_numeric(slot, value);
                        }
                    }
                    None => {
                        self.hold = Some(Hold::Text {
                            prompt,
                            result,
                            cursor_se,
                        });
                    }
                }
            }
            Hold::Key { slot, left } => {
                let mut batch = Batch::new(ctx);
                let mut got: Option<i64> = None;
                let mut cancelled = false;
                for i in 0..batch.len() {
                    if let Some(v) = batch.key_press(i) {
                        // 只认 `Press`：按住不放连发出的一串 `Repeat` 在"输入一个数字"这件事
                        // 上应该是一个字符，不是十个。
                        got = Some(v);
                        batch.consume(i);
                        break;
                    }
                    if batch.named_action(i) == Some(Named::Cancel) {
                        cancelled = true;
                        batch.consume(i);
                        break;
                    }
                }
                batch.flush(ctx);
                // 超时/取消都**不写槽**（清单语义：留原值），所以只有拿到按键才解除挂起。
                if cancelled {
                    // 取消 = 玩家自己结束了这次输入：放行，槽留原值。
                } else if let Some(v) = got {
                    ctx.vars.set_numeric(&slot, v);
                } else if left > 0 {
                    let left = left - 1;
                    if left > 0 {
                        self.hold = Some(Hold::Key { slot, left });
                    }
                } else {
                    // `timeout == 0` 是**无限**等：这一拍没等到不等于等到超时，装回去。
                    // 少了这一支，"报个数"会在它挂起后的第一拍自己放行（数字永远进不来）。
                    self.hold = Some(Hold::Key { slot, left: 0 });
                }
            }
        }
    }

    fn sync_ui(&mut self, ctx: &mut EventCtx) {
        let want = match &self.hold {
            Some(Hold::Text { prompt, .. }) => Some(prompt.clone()),
            _ => None,
        };
        if want != self.shown {
            match &want {
                Some(p) => ctx.ui.show(p),
                None => ctx.ui.hide(),
            }
            self.shown = want;
        }
        if self.dim != self.shown_dim {
            ctx.ui.set_dim(self.dim);
            self.shown_dim = self.dim;
        }
    }

    /// 全部清掉（场景切换/读档）：窗口收掉、黑幕归零、栈清空。
    pub fn stop(&mut self, ctx: &mut EventCtx) {
        self.frames.clear();
        self.hold = None;
        self.dim = 0.0;
        if self.shown.take().is_some() {
            ctx.ui.hide();
        }
        if self.shown_dim != 0.0 {
            self.shown_dim = 0.0;
            ctx.ui.set_dim(0.0);
        }
    }

    /// 执行一条叶子指令。返回 `false` = 本条链已经结束（帧被弹掉），调用方别再动 `pc`。
    fn exec(&mut self, ctx: &mut EventCtx, cmd: &Command, pc: usize) -> bool {
        match cmd {
            Command::ShowText {
                speaker,
                body,
                choices,
                result,
                cursor_se,
                allow_cancel,
            } => {
                let active = if choices.is_empty() { usize::MAX } else { 0 };
                self.hold = Some(Hold::Text {
                    prompt: Prompt {
                        speaker: speaker.clone(),
                        body: body.clone(),
                        choices: choices.clone(),
                        active,
                        allow_cancel: *allow_cancel,
                    },
                    result: result.clone(),
                    cursor_se: cursor_se.clone(),
                });
                self.set_pc(pc + 1);
                true
            }
            Command::InputKey { slot, timeout } => {
                // `timeout == 0` = 无限等（见 `Hold::Key`）。第一拍就把 `left` 摆成 0 的话
                // 下一条指令永远等不到超时。
                self.hold = Some(Hold::Key {
                    slot: slot.clone(),
                    left: *timeout,
                });
                self.set_pc(pc + 1);
                true
            }
            Command::Wait { frames } => {
                if *frames > 0 {
                    self.hold = Some(Hold::Wait { left: *frames });
                }
                self.set_pc(pc + 1);
                true
            }
            Command::MoveRoute { actor, route, wait } => {
                let sprite = self.resolve_actor(ctx.scene, *actor);
                self.set_pc(pc + 1);
                let Some(sprite) = sprite else { return true };
                match build_route(route, ctx.scene, *wait) {
                    Ok(task) => ctx.tasks.schedule(sprite, task),
                    Err(why) => self.warn(format!("actor {actor} 的路线起不来: {why}")),
                }
                if *wait {
                    self.hold = Some(Hold::Task { actor: *actor });
                }
                true
            }
            Command::Animate {
                actor,
                effect,
                wait,
            } => {
                let sprite = self.resolve_actor(ctx.scene, *actor);
                self.set_pc(pc + 1);
                let Some(sprite) = sprite else { return true };
                // 补间的写值与排程都在 `spawn_anim` 里一次做完：`Tween<f32>` 与
                // `Tween<Vec2>` 是两个类型，没法用一个返回值表达。
                if let Err(why) = spawn_anim(effect, sprite, ctx.sprites, ctx.tasks) {
                    self.warn(format!("actor {actor} 的动画起不来: {why}"));
                }
                if *wait {
                    self.hold = Some(Hold::Task { actor: *actor });
                }
                true
            }
            Command::PlayBgm(sound) => self.play(ctx, SoundChannel::Bgm, sound, pc),
            Command::PlayBgs(sound) => self.play(ctx, SoundChannel::Bgs, sound, pc),
            Command::PlayMe(sound) => self.play(ctx, SoundChannel::Me, sound, pc),
            Command::PlaySe(sound) => self.play(ctx, SoundChannel::Se, sound, pc),
            Command::Fade {
                target,
                frames,
                out,
            } => {
                // 黑幕是事件机自己的状态（`dim`），音频端口管不着它；反过来通道淡入淡出
                // 不占拍（BGM 自己淡，事件继续走）。两者都写成 `Fade` 只是为了事件表读起来
                // 是一条指令，不是两条。
                if !target.channels().is_empty()
                    && let Err(why) = ctx.audio.fade(*target, *frames, *out)
                {
                    self.warn(format!("淡入淡出失败: {why}"));
                }
                if target.is_screen() {
                    let to = if *out { 1.0 } else { 0.0 };
                    if *frames == 0 {
                        self.dim = to;
                    } else {
                        self.hold = Some(Hold::Fade {
                            left: *frames,
                            total: *frames,
                            from: self.dim,
                            to,
                        });
                    }
                }
                self.set_pc(pc + 1);
                true
            }
            Command::SetVar { slot, op, value } => {
                // 先求值再写：`apply` 要 `&mut vars`，两者同借会打架。
                let n = ctx.vars.eval(value);
                ctx.vars.apply(slot, *op, n);
                self.set_pc(pc + 1);
                true
            }
            Command::Erase => {
                let subject = self.frames.last().map(|f| f.subject);
                match subject {
                    Some(id) => self.erase(ctx, id),
                    None => self.warn("Erase 没有在跑的事件（栈是空的）"),
                }
                // 本链已经被摘掉，pc 归外层帧自己管。
                false
            }
            // `page` 在磁盘上是 `u32`（存档要稳定宽度），页表下标是 `usize`；u32 → usize
            // 只会变宽，不会截断。
            Command::Jump { page } => self.jump(*page as usize, pc),
            Command::Custom(name, args) => {
                // `None` 一律登记告警：后端手里没有告警通道（它只有 `name`/`args`），
                // "为什么这条没落地"这句话只能由事件机说（DoD-4 不许静默空实现）。
                // 处理完了但没有指令要跑的后端，返回一个空程序即可（见 `ScriptBackend::command`）。
                let (prog, unknown) = match self.backend.as_mut() {
                    Some(b) => (b.command(name, args), "脚本后端不认这个名字"),
                    None => (None, "这台事件机没装脚本后端"),
                };
                if prog.is_none() {
                    self.warn(format!("Custom({name})：{unknown}（指令跳过）"));
                }
                self.set_pc(pc + 1);
                if let Some(prog) = prog {
                    let subject = self.frames.last().map(|f| f.subject).unwrap_or(0);
                    self.push_frame(subject, prog, None, 0);
                }
                true
            }
            // 控制流在编译期就没了，这里出现只可能是编译器漏了。
            Command::If { .. } | Command::Loop(_) | Command::BreakLoop => {
                self.warn("嵌套指令没被编译掉（内部错误）");
                self.set_pc(pc + 1);
                true
            }
        }
    }

    fn play(
        &mut self,
        ctx: &mut EventCtx,
        channel: SoundChannel,
        sound: &Sound,
        pc: usize,
    ) -> bool {
        if let Err(why) = ctx.audio.play(ctx.vfs, channel, sound) {
            self.warn(format!(
                "{} 起播 {} 失败: {why}",
                channel.label(),
                sound.name
            ));
        }
        self.set_pc(pc + 1);
        true
    }

    /// actor id → 精灵；找不到就登记告警（配置写错该留下话，不该静默跳过）。
    fn resolve_actor(&mut self, ctx: &Scene, actor: u32) -> Option<SpriteRef> {
        let sprite = ctx.actor(actor);
        if sprite.is_none() {
            self.warn(format!("actor {actor} 不在场景里（这条指令跳过）"));
        }
        sprite
    }

    fn set_pc(&mut self, pc: usize) {
        if let Some(f) = self.frames.last_mut() {
            f.pc = pc;
        }
    }

    /// `Jump{page}`：换页并从第一条指令重来（RPG Maker 的"执行内容 → 翻页"）。
    fn jump(&mut self, page: usize, pc: usize) -> bool {
        let subject = match self.frames.last() {
            Some(f) => f.subject,
            None => return false,
        };
        let pages = match self.frames.last().and_then(|f| f.pages.clone()) {
            Some(p) => p,
            None => {
                self.warn("脚本程序里没有事件页，Jump 无处可跳（指令跳过）");
                self.set_pc(pc + 1);
                return true;
            }
        };
        let Some(prog) = pages.pages.get(page) else {
            // 越界 = 本事件到此结束（见 `Command::Jump` 的文档）。
            self.frames.retain(|f| f.subject != subject);
            return false;
        };
        let prog = Arc::new(prog.clone());
        let subject_pages = Some(pages);
        if let Some(f) = self.frames.last_mut() {
            f.page = page;
            f.prog = prog;
            f.pages = subject_pages;
            f.pc = 0;
        }
        true
    }

    /// `Erase`：把某个 actor 从世界里摘掉（任务、精灵、登记一起），并结束它名下的事件。
    fn erase(&mut self, ctx: &mut EventCtx, subject: u32) {
        if let Some(sprite) = ctx.scene.remove(subject) {
            ctx.tasks.cancel(sprite);
            ctx.sprites.despawn(sprite);
        }
        self.frames.retain(|f| f.subject != subject);
        if self
            .hold
            .as_ref()
            .is_some_and(|h| matches!(h, Hold::Task { actor } if *actor == subject))
        {
            self.hold = None;
        }
    }
}

/// 把磁盘上的路线编译成运行期的 [`MoveRoute`]。
///
/// 网格 vs 自由由 [`Scene::tile`] 决定（一份世界的角色不该在事件表里各自声明边长）。
/// 阻挡判定从 `Scene` 里克隆出那个 `Rc`：`MoveRoute::walkable` 要的是 `'static` 闭包，
/// 而通行表属于世界、会在场景切换时换掉，所以共享所有权而不是拷一份表。
fn build_route(spec: &RouteSpec, scene: &Scene, wait: bool) -> Result<MoveRoute, String> {
    let nodes: Vec<RouteNode> = spec
        .steps
        .iter()
        .map(|s| match *s {
            RouteStep::Wait { frames } => RouteNode::Wait { ticks: frames },
            RouteStep::Turn { dir } => RouteNode::Turn { dir },
            RouteStep::Move { dir, cells } => RouteNode::Move { dir, cells },
            RouteStep::MoveTo { x, y } => RouteNode::MoveTo { x, y },
            RouteStep::Jump { dir, cells } => RouteNode::Jump { dir, cells },
            RouteStep::FacingLock { locked } => RouteNode::FacingLock { locked },
        })
        .collect();
    let mut route = if scene.tile() > 0.0 {
        MoveRoute::grid(nodes, scene.tile(), spec.speed)
    } else {
        MoveRoute::free(nodes, spec.speed)
    }
    .through(spec.through)
    .wait(wait);
    if let Some(f) = scene.walkable_handle() {
        route = route.walkable(move |x, y| f(x, y));
    }
    Ok(route)
}

/// 排入一个补间效果（`Animate` 的四条效果都真有自己的通路，见 `docs/sprite.md` 的能力表）。
///
/// 起点取自精灵**当前**的值，所以同一个 actor 上叠两次 `Fade` 是接着淡，而不是跳回
/// 某个默认值。`wait` 时长按指令里写的拍数，与补间自己的时长是同一件事（最后一帧的拍号）。
fn spawn_anim(
    effect: &AnimEffect,
    sprite: SpriteRef,
    sprites: &mut SpriteArena,
    tasks: &mut TaskScheduler,
) -> Result<(), String> {
    let Some(s) = sprites.get(sprite) else {
        return Err("精灵已经不在 arena 里".to_owned());
    };
    match *effect {
        AnimEffect::Fade { to, frames } => {
            let from = s.opacity;
            let keys = vec![Keyframe::new(0, from), Keyframe::new(frames.max(1), to)];
            tasks.schedule(sprite, Tween::new(keys, |sp, v| sp.opacity = *v));
        }
        AnimEffect::Flash { frames } => {
            // 白闪 = tone 的加色分量从满到 0（`Tone::Offset` 的一条通路，不需要新管线）。
            // 收尾还原的是**闪之前**那颗 tone，不是 `Tone::NONE`：纯色精灵全都靠 tone 染色
            // （`examples/dialog_demo.rs` 就是），清成 NONE 等于把颜色一起闪没。
            let base = s.tone;
            let keys = vec![Keyframe::new(0, 1.0f32), Keyframe::new(frames.max(1), 0.0)];
            tasks.schedule(
                sprite,
                Tween::new(keys, |sp, v| {
                    sp.tone = Tone::offset([*v, *v, *v], 0.0);
                })
                .on_finish(move |sp| sp.tone = base),
            );
        }
        AnimEffect::Shake { px, frames } => {
            let base = s.pos;
            // 每 2 拍换一次方向：抖动因此与拍频无关（补间只按拍取值）。向上取整是宁可
            // 多抖半格，也不要"说要抖 3 拍、实际抖了 2 拍"。
            let mut keys = vec![Keyframe::new(0, base)];
            let steps = frames.div_ceil(2).max(1);
            for i in 1..steps {
                let side = if i % 2 == 1 { px } else { -px };
                keys.push(Keyframe::new(i * 2, base + Vec2::new(side, 0.0)));
            }
            keys.push(Keyframe::new(steps * 2, base));
            tasks.schedule(sprite, Tween::new(keys, |sp, v| sp.set_pos(*v)));
        }
        AnimEffect::Pulse { scale, frames } => {
            let from = s.scale;
            let half = (frames / 2).max(1);
            let keys = vec![
                Keyframe::new(0, from),
                Keyframe::new(half, Vec2::new(scale, scale)),
                Keyframe::new(half * 2, from),
            ];
            tasks.schedule(sprite, Tween::new(keys, |sp, v| sp.scale = *v));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    // T12：`cargo xtask wasm-test` 在 wasm 上跑这一组（见 docs/platforms.md）。
    use crate::bus::{EngineEvent, Lifecycle};
    use crate::command::{CmpOp, Cond, EventDef, Expr, Param, RouteStep, SetOp};
    use sisf_sprite::{Sprite, SpriteArena, Vec2};
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    /// 录下所有音频请求的端口（顺便当"没有解码 feature 的构建"的替身：`fail_with` 一设，
    /// 每条起播都失败，正好验告警通路）。
    #[derive(Default)]
    struct MockAudio {
        played: Vec<(SoundChannel, String)>,
        fades: Vec<(FadeTarget, u32, bool)>,
        fail_with: Option<String>,
    }

    impl AudioPort for MockAudio {
        fn play(&mut self, _vfs: &Vfs, channel: SoundChannel, sound: &Sound) -> Result<(), String> {
            if let Some(why) = &self.fail_with {
                return Err(why.clone());
            }
            self.played.push((channel, sound.name.clone()));
            Ok(())
        }

        fn fade(&mut self, target: FadeTarget, frames: u32, out: bool) -> Result<(), String> {
            if let Some(why) = &self.fail_with {
                return Err(why.clone());
            }
            self.fades.push((target, frames, out));
            Ok(())
        }

        fn stop(&mut self, channel: SoundChannel) {
            self.played.retain(|(c, _)| *c != channel);
        }
    }

    #[derive(Default)]
    struct MockUi {
        shown: Option<Prompt>,
        shows: u32,
        hides: u32,
        dim: f32,
    }

    impl UiPort for MockUi {
        fn show(&mut self, prompt: &Prompt) {
            self.shows += 1;
            self.shown = Some(prompt.clone());
        }

        fn hide(&mut self) {
            self.hides += 1;
            self.shown = None;
        }

        fn set_dim(&mut self, amount: f32) {
            self.dim = amount;
        }
    }

    /// 一拍的全部可变世界。`tick` 是唯一入口（它建 `EventCtx` 并只跑一次 `EventRunner::tick`，
    /// 所以"调度器由事件机推进"这件事在测试里也是真的）。
    struct World {
        sprites: SpriteArena,
        tasks: TaskScheduler,
        scene: Scene,
        audio: MockAudio,
        ui: MockUi,
        vars: VarStore,
        bus: EngineBus,
        vfs: Vfs,
    }

    impl World {
        fn new() -> Self {
            Self {
                sprites: SpriteArena::new(),
                tasks: TaskScheduler::new(),
                scene: Scene::grid(16.0),
                audio: MockAudio::default(),
                ui: MockUi::default(),
                vars: VarStore::new(),
                bus: EngineBus::new(),
                vfs: Vfs::new(),
            }
        }

        /// 登记一个 actor（`u32` id → 精灵），返回句柄。
        fn spawn(&mut self, actor: u32, at: Vec2) -> SpriteRef {
            let s = Sprite::solid(Vec2::new(16.0, 16.0)).at(at);
            let r = self.sprites.spawn(s);
            self.scene.set(actor, r);
            r
        }

        fn pos(&self, r: SpriteRef) -> Vec2 {
            self.sprites.get(r).expect("还在 arena 里").pos
        }

        fn press(&mut self, action: &str) {
            self.bus.push(EngineEvent::Action {
                name: action.to_owned(),
                edge: ActionEdge::Press,
            });
        }

        fn hold_action(&mut self, action: &str) {
            self.bus.push(EngineEvent::Action {
                name: action.to_owned(),
                edge: ActionEdge::Hold,
            });
        }

        fn type_key(&mut self, ch: char) {
            self.bus.push(EngineEvent::key(
                format!("Key{ch}"),
                KeyKind::Press,
                Some(ch),
            ));
        }

        fn tick(&mut self, vm: &mut EventRunner, budget: u32) -> RunnerStatus {
            let mut ctx = EventCtx {
                sprites: &mut self.sprites,
                tasks: &mut self.tasks,
                scene: &mut self.scene,
                audio: &mut self.audio,
                ui: &mut self.ui,
                vars: &mut self.vars,
                bus: &mut self.bus,
                vfs: &self.vfs,
            };
            vm.tick(&mut ctx, budget)
        }

        /// 一拍的缺省预算（够跑完任何一条正常指令链，只拦不住死循环）。
        fn tick1(&mut self, vm: &mut EventRunner) -> RunnerStatus {
            self.tick(vm, 64)
        }

        fn run(&mut self, vm: &mut EventRunner, beats: usize) -> RunnerStatus {
            let mut last = RunnerStatus::Idle { executed: 0 };
            for _ in 0..beats {
                last = self.tick1(vm);
            }
            last
        }
    }

    trait At {
        fn at(self, pos: Vec2) -> Self;
    }

    impl At for Sprite {
        fn at(mut self, pos: Vec2) -> Self {
            self.teleport(pos);
            self
        }
    }

    fn prog(commands: &[Command]) -> Arc<Program> {
        Arc::new(Program::compile(commands).expect("测试用的指令表该编译得动"))
    }

    /// 验收主线：一串指令 → 副作用（分支走对边、算术写对槽）。
    #[test]
    fn sequence_writes_vars_through_both_branches() {
        let mut vm = EventRunner::new();
        let mut w = World::new();
        let p = prog(&[
            Command::SetVar {
                slot: Slot::int(1),
                op: SetOp::Set,
                value: Expr::Lit(5),
            },
            Command::If {
                cond: Cond::Cmp {
                    lhs: Expr::Slot(Slot::int(1)),
                    op: CmpOp::Gt,
                    rhs: Expr::Lit(3),
                },
                then: vec![Command::SetVar {
                    slot: Slot::int(2),
                    op: SetOp::Set,
                    value: Expr::Lit(1),
                }],
                else_: vec![Command::SetVar {
                    slot: Slot::int(2),
                    op: SetOp::Set,
                    value: Expr::Lit(-1),
                }],
            },
            // 嵌套：外层为假才进内层，内层再比一次算术。
            Command::If {
                cond: Cond::Switch { id: 7, on: true },
                then: vec![],
                else_: vec![Command::SetVar {
                    slot: Slot::int(3),
                    op: SetOp::Set,
                    value: Expr::Add(
                        Box::new(Expr::Mul(
                            Box::new(Expr::Slot(Slot::int(1))),
                            Box::new(Expr::Lit(2)),
                        )),
                        Box::new(Expr::Lit(1)),
                    ),
                }],
            },
        ]);
        vm.start_program(0, p);
        assert_eq!(
            w.tick1(&mut vm),
            RunnerStatus::Idle { executed: 6 },
            "8 条平铺指令走掉 6 条：没进的那条 else，和条件为假时跳过去的 Goto"
        );
        assert_eq!(w.vars.int(1), 5);
        assert_eq!(w.vars.int(2), 1, "条件为真该走 then");
        assert_eq!(w.vars.int(3), 11, "5*2+1");
    }

    /// `Wait` 挂起 frames 拍，其间下一条指令不该提前执行。
    #[test]
    fn wait_holds_exactly_its_frames() {
        let mut vm = EventRunner::new();
        let mut w = World::new();
        vm.start_program(
            0,
            prog(&[
                Command::Wait { frames: 2 },
                Command::SetVar {
                    slot: Slot::int(1),
                    op: SetOp::Set,
                    value: Expr::Lit(1),
                },
            ]),
        );
        assert_eq!(w.tick1(&mut vm).suspended(), Some("wait"));
        assert_eq!(w.vars.int(1), 0);
        assert_eq!(w.tick1(&mut vm).suspended(), Some("wait"));
        assert_eq!(w.vars.int(1), 0, "第二拍还挂着，下一条不该跑");
        assert!(w.tick1(&mut vm).is_idle());
        assert_eq!(w.vars.int(1), 1);
    }

    /// 验收（`EventCtx` 那条）：ShowText 开窗 + 选项高亮 + 光标 SE + 结果写变量。
    #[test]
    fn show_text_drives_the_window_and_reports_the_choice() {
        let mut vm = EventRunner::new();
        let mut w = World::new();
        w.audio.played.clear();
        vm.start_program(
            0,
            prog(&[show(
                "井里有光。",
                &["去看", "回村"],
                Slot::int(3),
                Some("se_cursor.wav"),
                true,
            )]),
        );
        // 第 1 拍：开窗、光标在第 0 项。
        assert_eq!(w.tick1(&mut vm).suspended(), Some("text"));
        assert!(vm.waiting_input(), "开着窗 ⇒ 队列该留给事件机");
        let shown = w.ui.shown.clone().expect("窗该开着");
        assert_eq!(shown.body, "井里有光。");
        assert_eq!(shown.active, 0);
        assert_eq!(shown.choices.len(), 2);
        assert_eq!(w.ui.shows, 1, "内容没变就不该重复推");
        // 第 2 拍：下移一格 ⇒ 高亮跟着走，且响一声光标音。
        w.press(actions::DOWN);
        w.hold_action(actions::DOWN);
        assert_eq!(w.tick1(&mut vm).suspended(), Some("text"));
        assert_eq!(w.ui.shown.clone().expect("还开着").active, 1);
        assert_eq!(w.ui.shows, 2, "高亮变了才推第二次");
        assert_eq!(
            w.audio.played,
            vec![(SoundChannel::Se, "se_cursor.wav".to_owned())],
            "光标音走 SE 通道"
        );
        // `Hold`（按住每拍一条）不是边沿 ⇒ 不该动光标。
        w.hold_action(actions::DOWN);
        assert_eq!(w.tick1(&mut vm).suspended(), Some("text"));
        assert_eq!(w.ui.shown.clone().expect("还开着").active, 1);
        // 第 3 拍：确认 ⇒ 序号写进 result、窗收掉、后面的指令继续。
        w.press(actions::OK);
        assert!(w.tick1(&mut vm).is_idle());
        assert_eq!(w.vars.int(3), 1);
        assert_eq!(w.ui.hides, 1);
        assert!(w.ui.shown.is_none());
        assert!(!vm.waiting_input(), "事件跑完 ⇒ 游戏侧该清队列");
    }

    /// 取消：`allow_cancel` 才吃 `cancel`，并且写 `-1`（"没选"和"选了第 0 项"必须分得开）。
    #[test]
    fn cancel_is_opt_in_and_reports_minus_one() {
        let mut vm = EventRunner::new();
        let mut w = World::new();
        w.vars.set_int(3, 99);
        vm.start_program(
            0,
            prog(&[show("抬头。", &["走"], Slot::int(3), None, false)]),
        );
        assert_eq!(w.tick1(&mut vm).suspended(), Some("text"));
        w.press(actions::CANCEL);
        assert_eq!(
            w.tick1(&mut vm).suspended(),
            Some("text"),
            "没开 allow_cancel ⇒ 取消无效"
        );
        assert_eq!(w.vars.int(3), 99);
        w.press(actions::OK);
        assert!(w.tick1(&mut vm).is_idle());
        assert_eq!(w.vars.int(3), 0, "只有一项时选中它 = 0");
    }

    /// 事件机不吃与自己无关的事件，也不改变它们的顺序（松开在按下之后是常态）。
    #[test]
    fn events_that_are_not_its_business_stay_in_the_queue() {
        // "right" 在 `ActionMap::rpg_maker` 里存在，但事件机只关心上下左右里的上下 + 决定/取消。
        const RIGHT: &str = "right";

        let mut vm = EventRunner::new();
        let mut w = World::new();
        vm.start_program(
            0,
            prog(&[show("读条中。", &["a", "b"], Slot::int(1), None, false)]),
        );
        assert_eq!(w.tick1(&mut vm).suspended(), Some("text"));
        w.press(RIGHT);
        w.bus
            .push(EngineEvent::Lifecycle(Lifecycle::Focused(false)));
        w.press(actions::DOWN);
        w.bus
            .push(EngineEvent::key("Escape", KeyKind::Press, Some('\u{1b}')));
        assert_eq!(w.tick1(&mut vm).suspended(), Some("text"));
        let left = w.bus.drain();
        assert_eq!(
            left,
            vec![
                EngineEvent::Action {
                    name: RIGHT.to_owned(),
                    edge: ActionEdge::Press
                },
                EngineEvent::Lifecycle(Lifecycle::Focused(false)),
                EngineEvent::key("Escape", KeyKind::Press, Some('\u{1b}')),
            ],
            "只有 `down` 该被吃掉，其余按原序放回"
        );
    }

    /// `InputKey`：数字进槽；超时/取消都**不改**槽。
    #[test]
    fn input_key_writes_a_digit_and_timeouts_leave_the_slot() {
        let mut vm = EventRunner::new();
        let mut w = World::new();
        w.vars.set_int(1, 42);
        vm.start_program(
            0,
            prog(&[
                Command::InputKey {
                    slot: Slot::int(1),
                    timeout: 0,
                },
                Command::SetVar {
                    slot: Slot::int(2),
                    op: SetOp::Set,
                    value: Expr::Lit(7),
                },
            ]),
        );
        assert_eq!(w.tick1(&mut vm).suspended(), Some("key"));
        // 按住不放的连发不该被当成第二个字符。
        w.type_key('4');
        w.bus
            .push(EngineEvent::key("Key4", KeyKind::Repeat, Some('4')));
        assert!(w.tick1(&mut vm).is_idle());
        assert_eq!(w.vars.int(1), 4);
        assert_eq!(w.vars.int(2), 7, "解除挂起的同一拍就把下一条跑了");
        // 只剩一条未消费的 Repeat（上面那拍只认掉一个 Press）。
        assert_eq!(
            w.bus.drain(),
            vec![EngineEvent::key("Key4", KeyKind::Repeat, Some('4'))]
        );

        // 超时：`timeout: 2` ⇒ 挂两拍，第三拍继续，槽还是 42。
        w.vars.set_int(1, 42);
        let mut vm = EventRunner::new();
        vm.start_program(
            0,
            prog(&[Command::InputKey {
                slot: Slot::int(1),
                timeout: 2,
            }]),
        );
        assert_eq!(w.tick1(&mut vm).suspended(), Some("key"));
        assert_eq!(w.tick1(&mut vm).suspended(), Some("key"));
        assert!(w.tick1(&mut vm).is_idle());
        assert_eq!(w.vars.int(1), 42, "超时不该往槽里写东西");

        // 取消：同样不写。
        let mut vm = EventRunner::new();
        vm.start_program(
            0,
            prog(&[Command::InputKey {
                slot: Slot::int(1),
                timeout: 0,
            }]),
        );
        assert_eq!(w.tick1(&mut vm).suspended(), Some("key"));
        w.press(actions::CANCEL);
        assert!(w.tick1(&mut vm).is_idle());
        assert_eq!(w.vars.int(1), 42);
    }

    /// 一次按下只该解一次挂。浏览器里量出来的那条：`ShowText` 的"继续"按下解完挂后按原序
    /// 放回总线，紧跟着的 `InputKey` 于是又看见它 —— 控制字符被当成 0，
    /// `examples/dialog_demo.rs` 的井边人永远停在"太小了"，任何数字都进不来。
    #[test]
    fn the_press_that_closed_the_last_line_does_not_answer_the_next_prompt() {
        let mut vm = EventRunner::new();
        let mut w = World::new();
        w.vars.set_int(1, 42);
        vm.start_program(
            0,
            prog(&[
                show("报个数我才让开。", &[], Slot::int(9), None, false),
                Command::InputKey {
                    slot: Slot::int(1),
                    timeout: 0,
                },
            ]),
        );
        assert_eq!(w.tick1(&mut vm).suspended(), Some("text"));
        // 一次真实的 Enter：裸按下 + 它折出来的 `ok` 边沿（`InputState::feed` 就这个顺序）。
        // 字符是 `\r` 而不是 `None` —— 浏览器里 winit 递过来的就是这个控制字符。
        let enter = EngineEvent::key("Enter", KeyKind::Press, Some('\r'));
        w.bus.push(enter.clone());
        w.press(actions::OK);
        assert_eq!(w.tick1(&mut vm).suspended(), Some("key"));
        // 空着的一拍：那次 Enter 已经答过上一题了。
        assert_eq!(w.tick1(&mut vm).suspended(), Some("key"));
        assert_eq!(w.vars.int(1), 42, "控制字符不该写槽");
        // 带字符的按下才是答案。
        w.type_key('7');
        assert!(w.tick1(&mut vm).is_idle());
        assert_eq!(w.vars.int(1), 7);
        assert!(
            w.bus.drain().contains(&enter),
            "没吃掉的外来事件要按原序留在总线里"
        );
    }

    /// 验收：超预算保护 —— 没有 `Wait` 的死循环最多吃掉 `budget` 条，下一拍接着走。
    #[test]
    fn a_runaway_loop_cannot_hog_a_beat() {
        let mut vm = EventRunner::new();
        let mut w = World::new();
        vm.start_program(
            0,
            prog(&[Command::Loop(vec![Command::SetVar {
                slot: Slot::int(1),
                op: SetOp::Add,
                value: Expr::Lit(1),
            }])]),
        );
        // 一拍 4 条预算 = 两条 `SetVar`（另一条是回边）。
        assert_eq!(w.tick(&mut vm, 4), RunnerStatus::OverBudget { executed: 4 });
        assert_eq!(w.vars.int(1), 2);
        // `budget = 0` 该退化成"至少一条"，而不是把事件机冻住。
        assert_eq!(w.tick(&mut vm, 0), RunnerStatus::OverBudget { executed: 1 });
        assert_eq!(w.vars.int(1), 3);
        assert!(!vm.is_idle() && vm.busy());
    }

    /// `MoveRoute{wait:true}` 阻塞到路线跑完，位置逐格落在格点上。
    #[test]
    fn a_blocking_route_walks_the_grid_one_cell_per_beat() {
        let mut vm = EventRunner::new();
        let mut w = World::new();
        let sprite = w.spawn(1, Vec2::new(32.0, 32.0));
        vm.start_program(
            0,
            prog(&[
                Command::MoveRoute {
                    actor: 1,
                    route: RouteSpec {
                        steps: vec![
                            RouteStep::Turn { dir: 6 },
                            RouteStep::Move { dir: 6, cells: 2 },
                        ],
                        speed: 16.0,
                        through: false,
                    },
                    wait: true,
                },
                Command::SetVar {
                    slot: Slot::int(1),
                    op: SetOp::Set,
                    value: Expr::Lit(1),
                },
            ]),
        );
        assert_eq!(w.tick1(&mut vm).suspended(), Some("task"), "排程的一拍");
        assert_eq!(
            w.pos(sprite),
            Vec2::new(32.0, 32.0),
            "起挂这一拍路线还没起步"
        );
        // `Turn` 自己占一拍（`RouteStep::Turn` 的文档：转身一拍、不位移）。
        assert_eq!(w.tick1(&mut vm).suspended(), Some("task"), "转身拍");
        assert_eq!(w.pos(sprite), Vec2::new(32.0, 32.0));
        assert_eq!(w.tick1(&mut vm).suspended(), Some("task"));
        assert_eq!(
            w.pos(sprite),
            Vec2::new(48.0, 32.0),
            "speed = 一格 ⇒ 一拍一格"
        );
        assert!(w.tick1(&mut vm).is_idle(), "第二格跑完就该解除挂起");
        assert_eq!(w.pos(sprite), Vec2::new(64.0, 32.0));
        assert_eq!(w.vars.int(1), 1);
    }

    /// 撞墙：剩余格数作废，但路线继续往下走（事件不该因为一扇门整个卡死）。
    #[test]
    fn a_wall_voids_the_rest_of_a_step_but_not_the_event() {
        let mut vm = EventRunner::new();
        let mut w = World::new();
        w.scene.set_walkable(|x, _| x < 48.0);
        let sprite = w.spawn(1, Vec2::new(32.0, 32.0));
        vm.start_program(
            0,
            prog(&[
                Command::MoveRoute {
                    actor: 1,
                    route: RouteSpec {
                        steps: vec![RouteStep::Move { dir: 6, cells: 4 }],
                        speed: 16.0,
                        through: false,
                    },
                    wait: true,
                },
                Command::SetVar {
                    slot: Slot::int(1),
                    op: SetOp::Set,
                    value: Expr::Lit(1),
                },
            ]),
        );
        let last = w.run(&mut vm, 8);
        assert!(last.is_idle(), "撞墙之后事件必须收尾: {last:?}");
        assert_eq!(w.pos(sprite), Vec2::new(32.0, 32.0), "一格都过不去");
        assert_eq!(w.vars.int(1), 1);
    }

    /// `through` 才是"无视阻挡"，别和 `wait` 搞混。
    #[test]
    fn through_ignores_the_passability_table() {
        let mut vm = EventRunner::new();
        let mut w = World::new();
        w.scene.set_walkable(|_, _| false);
        let sprite = w.spawn(1, Vec2::new(32.0, 32.0));
        vm.start_program(
            0,
            prog(&[Command::MoveRoute {
                actor: 1,
                route: RouteSpec {
                    steps: vec![RouteStep::Move { dir: 6, cells: 1 }],
                    speed: 16.0,
                    through: true,
                },
                wait: true,
            }]),
        );
        w.run(&mut vm, 4);
        assert_eq!(w.pos(sprite), Vec2::new(48.0, 32.0));
    }

    /// `Animate` 走补间：`wait:true` 阻塞到淡完，透明度落在终点值上。
    #[test]
    fn animate_fades_and_blocks_until_done() {
        let mut vm = EventRunner::new();
        let mut w = World::new();
        let sprite = w.spawn(2, Vec2::new(16.0, 16.0));
        vm.start_program(
            0,
            prog(&[Command::Animate {
                actor: 2,
                effect: AnimEffect::Fade { to: 0.0, frames: 2 },
                wait: true,
            }]),
        );
        assert_eq!(w.tick1(&mut vm).suspended(), Some("task"));
        assert_eq!(w.tick1(&mut vm).suspended(), Some("task"));
        assert_eq!(
            w.sprites.get(sprite).unwrap().opacity,
            0.5,
            "两拍淡完 ⇒ 中间那拍在半路"
        );
        assert!(w.tick1(&mut vm).is_idle());
        assert_eq!(w.sprites.get(sprite).unwrap().opacity, 0.0);
    }

    /// 白闪收尾要还原**闪之前**的 tone。浏览器里量出来的那条：`examples/dialog_demo.rs`
    /// 的宝箱靠 `Tone::offset` 染成灰蓝，闪完却变成纯白 —— 因为收尾写的是 `Tone::NONE`。
    /// 纯色精灵的"颜色"就是 tone，清成 NONE 等于把颜色一起闪没了。
    #[test]
    fn a_flash_leaves_the_tone_it_found() {
        let mut vm = EventRunner::new();
        let mut w = World::new();
        let sprite = w.spawn(2, Vec2::new(16.0, 16.0));
        let before = Tone::offset([-0.4, -0.3, -0.2], 0.0);
        w.sprites.get_mut(sprite).unwrap().tone = before;
        vm.start_program(
            0,
            prog(&[Command::Animate {
                actor: 2,
                effect: AnimEffect::Flash { frames: 2 },
                wait: true,
            }]),
        );
        assert_eq!(
            w.tick1(&mut vm).suspended(),
            Some("task"),
            "排程那一拍补间还没开始跑"
        );
        assert_eq!(w.sprites.get(sprite).unwrap().tone, before);
        assert_eq!(w.tick1(&mut vm).suspended(), Some("task"));
        assert_eq!(
            w.sprites.get(sprite).unwrap().tone,
            Tone::offset([0.5, 0.5, 0.5], 0.0),
            "两拍闪完 ⇒ 中间那拍是半闪"
        );
        assert!(w.tick1(&mut vm).is_idle());
        assert_eq!(w.sprites.get(sprite).unwrap().tone, before);
    }

    /// 屏幕淡出：黑幕按拍推进，UI 只在变化时收到 `set_dim`，且它阻塞。
    #[test]
    fn a_screen_fade_steps_the_darkness_and_holds() {
        let mut vm = EventRunner::new();
        let mut w = World::new();
        vm.start_program(
            0,
            prog(&[
                Command::Fade {
                    target: FadeTarget::Screen,
                    frames: 2,
                    out: true,
                },
                Command::Fade {
                    target: FadeTarget::Bgm,
                    frames: 30,
                    out: true,
                },
            ]),
        );
        assert_eq!(w.tick1(&mut vm).suspended(), Some("fade"));
        assert_eq!(vm.dim(), 0.0, "起挂这一拍还没有推进");
        assert_eq!(w.tick1(&mut vm).suspended(), Some("fade"));
        assert_eq!(vm.dim(), 0.5);
        assert_eq!(w.ui.dim, 0.5, "UI 是镜像");
        assert!(w.tick1(&mut vm).is_idle());
        assert_eq!(vm.dim(), 1.0);
        assert_eq!(w.ui.dim, 1.0);
        assert_eq!(
            w.audio.fades,
            vec![(FadeTarget::Bgm, 30, true)],
            "音频淡出只登记请求，不阻塞事件（BGM 在后面自己淡）"
        );
    }

    /// `All` 淡的是四条音频通道，不含黑幕。
    #[test]
    fn fading_all_touches_four_channels_only() {
        let mut vm = EventRunner::new();
        let mut w = World::new();
        vm.start_program(
            0,
            prog(&[Command::Fade {
                target: FadeTarget::All,
                frames: 8,
                out: false,
            }]),
        );
        assert!(w.tick1(&mut vm).is_idle());
        assert_eq!(w.audio.fades, vec![(FadeTarget::All, 8, false)]);
        assert_eq!(vm.dim(), 0.0);
    }

    /// 起播四条通道各归各位；端口失败只留下告警，不中断事件。
    #[test]
    fn playing_audio_reaches_the_right_channel_and_failure_only_warns() {
        let mut vm = EventRunner::new();
        let mut w = World::new();
        vm.start_program(
            0,
            prog(&[
                Command::PlayBgm(Sound::new("bgm01.ogg")),
                Command::PlayBgs(Sound::new("wind.ogg")),
                Command::PlayMe(Sound::new("fanfare.mp3")),
                Command::PlaySe(Sound::new("door.wav")),
            ]),
        );
        assert!(w.tick1(&mut vm).is_idle());
        assert_eq!(
            w.audio.played,
            vec![
                (SoundChannel::Bgm, "bgm01.ogg".to_owned()),
                (SoundChannel::Bgs, "wind.ogg".to_owned()),
                (SoundChannel::Me, "fanfare.mp3".to_owned()),
                (SoundChannel::Se, "door.wav".to_owned()),
            ]
        );

        // 没有解码 feature / 资源缺失：事件照跑，话要说清楚。
        let mut vm = EventRunner::new();
        w.audio.fail_with = Some("这台构建没带 ogg 解码器".to_owned());
        vm.start_program(0, prog(&[Command::PlayBgm(Sound::new("x.ogg"))]));
        assert!(w.tick1(&mut vm).is_idle());
        let warns = vm.take_warnings();
        assert_eq!(warns.len(), 1, "{warns:?}");
        assert!(warns[0].contains("没带 ogg 解码器"), "{warns:?}");
        assert!(warns[0].contains("BGM"), "得说清是哪条通道: {warns:?}");
    }

    /// `Jump{page}` 换页并从头执行；越界 = 本事件结束。
    #[test]
    fn jumping_a_page_restarts_the_event_with_its_new_page() {
        let mut vm = EventRunner::new();
        let mut w = World::new();
        let def = EventDef {
            name: "npc".into(),
            pages: vec![
                vec![
                    Command::SetVar {
                        slot: Slot::int(1),
                        op: SetOp::Set,
                        value: Expr::Lit(1),
                    },
                    Command::Jump { page: 1 },
                    // 换页之后这几条不该再执行。
                    Command::SetVar {
                        slot: Slot::int(1),
                        op: SetOp::Set,
                        value: Expr::Lit(99),
                    },
                ],
                vec![Command::SetVar {
                    slot: Slot::int(2),
                    op: SetOp::Set,
                    value: Expr::Lit(2),
                }],
            ],
        };
        let script = Arc::new(def.compile().expect("编译该过"));
        assert!(vm.start(1, Arc::clone(&script), 0));
        assert_eq!(vm.page_of(1), Some(0));
        assert_eq!(
            w.tick1(&mut vm),
            RunnerStatus::Idle { executed: 3 },
            "三条：写值、翻页（它自己也算一条）、新页的写值"
        );
        assert_eq!(w.vars.int(1), 1, "跳页之后旧页剩下的指令不该再跑");
        assert_eq!(w.vars.int(2), 2);
        assert_eq!(vm.page_of(1), None, "事件已经跑完");

        // 越界：本事件到此为止（不写后面的值）。
        let mut vm = EventRunner::new();
        vm.start(1, Arc::clone(&script), 0);
        let only_jump = prog(&[Command::Jump { page: 7 }]);
        vm.start_program(9, only_jump);
        w.tick1(&mut vm);
        assert_eq!(vm.page_of(9), None, "跳去处没有这一页 ⇒ 事件结束");
        assert_eq!(w.vars.int(1), 1);
    }

    /// 起一个不存在的页要留下话，而不是静默什么都不做。
    #[test]
    fn starting_a_missing_page_warns() {
        let mut vm = EventRunner::new();
        let def = EventDef {
            name: "one".into(),
            pages: vec![vec![]],
        };
        let script = Arc::new(def.compile().expect("编译该过"));
        assert!(!vm.start(1, script, 3));
        let warns = vm.take_warnings();
        assert_eq!(warns, vec!["事件 one 没有第 3 页".to_owned()]);
    }

    /// `Erase` 摘掉 actor（任务、精灵、登记一起）并结束它名下的事件。
    #[test]
    fn erase_removes_the_actor_and_ends_its_event() {
        let mut vm = EventRunner::new();
        let mut w = World::new();
        let sprite = w.spawn(5, Vec2::new(16.0, 16.0));
        vm.start_program(
            5,
            prog(&[
                Command::Erase,
                Command::SetVar {
                    slot: Slot::int(1),
                    op: SetOp::Set,
                    value: Expr::Lit(1),
                },
            ]),
        );
        assert_eq!(w.tick1(&mut vm), RunnerStatus::Idle { executed: 1 });
        assert!(w.scene.actor(5).is_none());
        assert!(w.sprites.get(sprite).is_none(), "精灵也要从 arena 里摘掉");
        assert_eq!(w.vars.int(1), 0, "Erase 之后的指令属于这个事件，不该再跑");
        assert!(vm.is_idle());
    }

    /// 没登记过的 actor：跳过该指令并说清楚是谁。
    #[test]
    fn an_unknown_actor_warns_once_and_moves_on() {
        let mut vm = EventRunner::new();
        let mut w = World::new();
        vm.start_program(
            0,
            prog(&[
                Command::MoveRoute {
                    actor: 11,
                    route: RouteSpec::new(vec![RouteStep::Wait { frames: 5 }]),
                    wait: true,
                },
                Command::SetVar {
                    slot: Slot::int(1),
                    op: SetOp::Set,
                    value: Expr::Lit(1),
                },
            ]),
        );
        // `wait: true` 也不该挂住：要等的任务根本没排上，挂了就是死等。
        assert_eq!(w.tick1(&mut vm), RunnerStatus::Idle { executed: 2 });
        assert_eq!(
            vm.take_warnings(),
            vec!["actor 11 不在场景里（这条指令跳过）".to_owned()]
        );
    }

    /// 同一句话每拍都踩到只登记一次（刷屏等于没有告警）。
    #[test]
    fn repeated_warnings_are_deduplicated() {
        let mut vm = EventRunner::new();
        let mut w = World::new();
        w.audio.fail_with = Some("没带解码器".to_owned());
        vm.start_program(
            0,
            prog(&[Command::Loop(vec![
                Command::PlayBgm(Sound::new("a.ogg")),
                Command::Wait { frames: 1 },
            ])]),
        );
        w.run(&mut vm, 4);
        let warns = vm.take_warnings();
        assert_eq!(warns.len(), 1, "{warns:?}");
        // 换一条不同的消息还会再说一次。
        vm.start_program(
            0,
            prog(&[
                Command::PlaySe(Sound::new("b.wav")),
                Command::PlayBgm(Sound::new("c.ogg")),
            ]),
        );
        w.tick1(&mut vm);
        assert_eq!(vm.take_warnings().len(), 2);
    }

    /// 验收（`script.rs` 那条）：后端装上去之后三个钩子都真能落地 —— `command` 的程序压栈
    /// 执行、`tick` 投递的程序起跑、`start` 被转告。
    #[test]
    fn backend_hooks_fire() {
        use std::cell::RefCell;
        use std::rc::Rc;

        #[derive(Default)]
        struct Spy {
            log: Rc<RefCell<Vec<String>>>,
        }

        impl ScriptBackend for Spy {
            fn start(&mut self, entry: &str) {
                self.log.borrow_mut().push(format!("start:{entry}"));
            }

            fn tick(&mut self, _ctx: &mut EventCtx, queue: &mut Vec<Submission>) {
                self.log.borrow_mut().push("tick".to_owned());
                queue.push((0, write_int(3, 30)));
            }

            fn command(&mut self, name: &str, args: &[Param]) -> Option<Arc<Program>> {
                self.log.borrow_mut().push(format!(
                    "command:{name}({})",
                    args.iter()
                        .map(|p| p.to_json().to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                ));
                match name {
                    "spy.set" => Some(write_int(1, 11)),
                    // "我处理完了，但没有指令要跑"：交一个空程序，而不是 `None`。
                    "spy.noop" => Some(Arc::new(Program::default())),
                    _ => None,
                }
            }
        }

        fn write_int(id: u32, value: i64) -> Arc<Program> {
            prog(&[Command::SetVar {
                slot: Slot::int(id),
                op: SetOp::Set,
                value: Expr::Lit(value),
            }])
        }

        let log = Rc::new(RefCell::new(Vec::new()));
        let mut vm = EventRunner::new().with_script(Box::new(Spy {
            log: Rc::clone(&log),
        }));
        let mut w = World::new();
        vm.start_script("map001.on_load");
        // `Custom` 的返回位置： callee 跑完回到调用者的下一条。
        vm.start_program(
            0,
            prog(&[
                Command::Custom(
                    "spy.set".into(),
                    vec![Param::Int(2), Param::Text("井".into())],
                ),
                Command::SetVar {
                    slot: Slot::int(2),
                    op: SetOp::Set,
                    value: Expr::Lit(2),
                },
            ]),
        );
        assert_eq!(w.tick1(&mut vm), RunnerStatus::Idle { executed: 4 });
        assert_eq!(w.vars.int(1), 11, "后端程序该改了 1 号槽");
        assert_eq!(w.vars.int(2), 2, "回到调用者之后该接着走");
        assert_eq!(w.vars.int(3), 30, "`tick` 投递的程序也该起跑");
        assert_eq!(
            log.borrow().as_slice(),
            &[
                "start:map001.on_load".to_owned(),
                "tick".to_owned(),
                r#"command:spy.set(2,"井")"#.to_owned(),
            ]
        );

        // 后端不认的名字：告警由事件机补（后端没有告警通道）。"处理完了但没有指令要跑"
        // 则是另一件事 —— 后端交一个空程序，谁都不该报警。
        vm.start_program(
            0,
            prog(&[
                Command::Custom("nope".into(), vec![]),
                Command::Custom("spy.noop".into(), vec![]),
            ]),
        );
        assert_eq!(vm.take_warnings(), Vec::<String>::new(), "上一拍没留下话");
        assert!(w.tick1(&mut vm).is_idle());
        assert_eq!(
            vm.take_warnings(),
            vec!["Custom(nope)：脚本后端不认这个名字（指令跳过）".to_owned()],
            "{:?}",
            log.borrow()
        );
    }

    /// 没装后端时 `Custom` 也必须说清楚为什么什么都没发生。
    #[test]
    fn custom_without_a_backend_is_explained() {
        let mut vm = EventRunner::new();
        let mut w = World::new();
        vm.start_program(0, prog(&[Command::Custom("rpgmv.transfer".into(), vec![])]));
        assert!(w.tick1(&mut vm).is_idle());
        assert_eq!(
            vm.take_warnings(),
            vec!["Custom(rpgmv.transfer)：这台事件机没装脚本后端（指令跳过）".to_owned()]
        );
    }

    /// 递归 `Custom` 不能把栈撑破（到上限就拒绝再起新程序）。
    #[test]
    fn recursive_custom_is_capped() {
        struct Recursive;

        impl ScriptBackend for Recursive {
            fn start(&mut self, _entry: &str) {}
            fn tick(&mut self, _ctx: &mut EventCtx, _queue: &mut Vec<Submission>) {}
            fn command(&mut self, _name: &str, _args: &[Param]) -> Option<Arc<Program>> {
                Some(prog(&[Command::Custom("again".into(), vec![])]))
            }
        }

        let mut vm = EventRunner::new().with_script(Box::new(Recursive));
        let mut w = World::new();
        vm.start_program(0, prog(&[Command::Custom("again".into(), vec![])]));
        let status = w.tick(&mut vm, 1024);
        // 每层帧恰好跑掉一条 `Custom` 就再起一层，到上限那条被拒 ⇒ 平的这一拍把栈烧穿后
        // 一路弹空。数字对上 `MAX_FRAMES` 就是"上限真的在拦"的证据。
        assert_eq!(
            status,
            RunnerStatus::Idle {
                executed: MAX_FRAMES as u32
            }
        );
        let warns = vm.take_warnings();
        assert!(
            warns
                .iter()
                .any(|x| x.contains(&MAX_FRAMES.to_string()) && x.contains("调用栈")),
            "{warns:?}"
        );
    }

    /// `stop`（场景切换/读档）：窗口收掉、黑幕归零、栈清空。
    #[test]
    fn stopping_resets_the_window_and_the_darkness() {
        let mut vm = EventRunner::new();
        let mut w = World::new();
        vm.start_program(
            0,
            prog(&[
                Command::Fade {
                    target: FadeTarget::Screen,
                    frames: 2,
                    out: true,
                },
                show("别走。", &[], Slot::int(1), None, false),
            ]),
        );
        w.run(&mut vm, 3);
        assert_eq!(vm.dim(), 1.0);
        assert_eq!(w.tick1(&mut vm).suspended(), Some("text"));
        let mut ctx = EventCtx {
            sprites: &mut w.sprites,
            tasks: &mut w.tasks,
            scene: &mut w.scene,
            audio: &mut w.audio,
            ui: &mut w.ui,
            vars: &mut w.vars,
            bus: &mut w.bus,
            vfs: &w.vfs,
        };
        vm.stop(&mut ctx);
        assert!(vm.is_idle());
        assert_eq!(w.ui.shown, None);
        assert_eq!(w.ui.hides, 1);
        assert_eq!(w.ui.dim, 0.0);
    }

    fn show(
        body: &str,
        choices: &[&str],
        result: Slot,
        cursor_se: Option<&str>,
        allow_cancel: bool,
    ) -> Command {
        Command::ShowText {
            speaker: None,
            body: body.to_owned(),
            choices: choices.iter().map(|s| (*s).to_owned()).collect(),
            result: Some(result),
            cursor_se: cursor_se.map(Sound::new),
            allow_cancel,
        }
    }
}
