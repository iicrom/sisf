//! 游戏事件指令（`Command`）：数据层 + 扁平 IR。
//!
//! ## 这一层管什么
//!
//! 事件在磁盘上是一棵 RON（[`EventDef`]，手写友好、和 RPG Maker 的"指令列表 + 嵌套分支"
//! 同形），进虚拟机前先编译成一条**扁平指令流**（[`Program`]）。为什么要编译而不是让
//! 解释器递归走树：`Loop`/`BreakLoop`/`If` 在树里要么靠调用栈、要么靠"重走一遍整棵子树
//! 数第几次"，两者都没法把"跑到哪儿了"说成一个数。拍表（[`crate::runner`]）要求每个执行
//! 单元只有一个 `pc`，所以嵌套在这里就被抹平成带目标的跳转。
//!
//! ```text
//! If{cond, then, else_}   →  Branch{cond, on_false} · then · Goto{end} · else_ · end:
//! Loop{body}              →  body · LoopBack{start}          （BreakLoop → Goto{end}）
//! ```
//!
//! 编译是**一次性的**（`EventDef::compile`），运行期只改 `pc`，所以指令里既没有子程序
//! 也没有闭包 —— 控制流的容量全在 `Cond`/`Expr` 那两棵小表达式树里，它们是纯求值、
//! 无副作用、不占 `pc` 的。
//!
//! ## 地址：为什么是 `u32` 而不是 `SpriteRef`
//!
//! 指令要指向精灵，但 [`SpriteRef`](sisf_sprite::SpriteRef) 的 generation 在"删了又建"之
//! 间会变，写进 `.ron` 就是存一个下次运行多半不对的数。所以这里用**稳定的 actor id**
//! （`u32`，游戏侧给每个事件主角登记一次），查表换成 `SpriteRef` 的活归
//! [`Scene`](crate::runner::Scene)。`Slot` 同理：命名变量而不是下标，`.ron` 里写
//! `var: "party_leader"` 比写 `var: 7` 好读也好改。
//!
//! ## 与清单字面量的差异（都在此登记，见 `todo.md`「T10 期间发现并修正的事实」）
//!
//! - `ShowText` 多了 `result`/`cursor_se`：选项要有返回值才能分支，选项音是 RPG Maker 的
//!   默认行为（缺省即静音，不塞假默认）。
//! - `InputKey` 的 `var` 落成 `Slot`（三态存储的统称），并保留"超时不写值"的语义。
//! - `Wait` 的单位是**拍**（不是毫秒）：虚拟机只在 tick 边界推进，拍是这里唯一的时钟。
//! - `Fade` 多了 `out`：`target` 只说"淡什么"，没说往哪边淡，而渐入渐出是成对出现的。

use serde::{Deserialize, Serialize};

/// 变量槽：三态存储（[`VarStore`](crate::vars::VarStore)）的地址。
///
/// `Switch` 是布尔开关、`Int` 是参与算术的整数、`Var` 是命名文本槽（数字以文本存，
/// 求值时能解析就解析、不能解析算 0，见 [`Expr`]）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Slot {
    Switch(u32),
    Int(u32),
    Var(String),
}

impl Slot {
    pub fn switch(id: u32) -> Self {
        Self::Switch(id)
    }

    pub fn int(id: u32) -> Self {
        Self::Int(id)
    }

    pub fn var(name: impl Into<String>) -> Self {
        Self::Var(name.into())
    }
}

/// 比较运算符。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CmpOp {
    pub fn test(self, a: i64, b: i64) -> bool {
        match self {
            Self::Eq => a == b,
            Self::Ne => a != b,
            Self::Lt => a < b,
            Self::Le => a <= b,
            Self::Gt => a > b,
            Self::Ge => a >= b,
        }
    }
}

/// `SetVar` 的写入方式。`Tog` 只对 `Switch` 有意义（对数值槽按"非 0 即真"取反）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetOp {
    Set,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Tog,
}

/// 整数算术表达式（对 [`VarStore`](crate::vars::VarStore) 求值）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Lit(i64),
    Slot(Slot),
    /// 随机整数，闭区间 `[lo, hi]`。走虚拟机自带的确定性 RNG（同一颗种子 → 同一条序列），
    /// 所以它是"有副作用"的求值：编译期不折叠，运行期每次读都取下一个数。
    Rand {
        lo: i64,
        hi: i64,
    },
    Neg(Box<Expr>),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    /// 整除，向零取整；除数为 0 时结果为 **0**（不是 panic，也不是上一个值：
    /// 配置写错不该有"事件卡住"这种后果）。
    Div(Box<Expr>, Box<Expr>),
    Mod(Box<Expr>, Box<Expr>),
}

/// 条件。求值纯读，不改状态。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Cond {
    /// 开关状态（`on: false` 即"关着才成立"）。
    Switch {
        id: u32,
        on: bool,
    },
    /// 数值比较。
    Cmp {
        lhs: Expr,
        op: CmpOp,
        rhs: Expr,
    },
    /// 文本槽逐字相等（选项结果按文字分支时用；数值分支请走 `Cmp`）。
    TextEq {
        slot: Slot,
        value: String,
    },
    Not(Box<Cond>),
    All(Vec<Cond>),
    Any(Vec<Cond>),
}

impl Cond {
    /// 字面量条件，测试与示例里省事（`Cond::always()` == `All([])`）。
    pub fn always() -> Self {
        Self::All(Vec::new())
    }
}

/// 一份音频资源引用。
///
/// `name` 是 VFS 路径（`audio/bgm01.ogg`），由虚拟机侧的端口解析并**一次性解码驻留**
/// （见 `crate::runner::AudioPort` 与 `todo.md` T7 事实第 3 条：绝不每圈在播放线程解码）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sound {
    pub name: String,
    /// 线性增益，`1.0` = 原样。
    pub gain: f32,
    /// 起播淡入（毫秒）。`0` = 不淡。
    pub fade_in: f32,
}

impl Sound {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            gain: 1.0,
            fade_in: 0.0,
        }
    }

    pub fn gain(mut self, gain: f32) -> Self {
        self.gain = gain;
        self
    }

    pub fn fade_in(mut self, ms: f32) -> Self {
        self.fade_in = ms;
        self
    }
}

/// 混音器的四条通道（对应 [`Command::PlayBgm`] 等四条起播指令）。
///
/// 与 [`FadeTarget`] 分开是必要的：`Screen`/`All` 能淡却不能"起播"，让它们共用一个枚举
/// 就等于给端口留一条"收到起播请求但没有通路"的死分支。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundChannel {
    Bgm,
    Bgs,
    Me,
    Se,
}

impl SoundChannel {
    pub const ALL: [SoundChannel; 4] = [Self::Bgm, Self::Bgs, Self::Me, Self::Se];

    /// 磁盘与运行期都用 snake_case，只有告警和调试面板要读它 —— 所以可读的写法放在这里，
    /// 而不是让每个游戏侧端口自己 match 一遍。
    pub fn label(self) -> &'static str {
        match self {
            Self::Bgm => "BGM",
            Self::Bgs => "BGS",
            Self::Me => "ME",
            Self::Se => "SE",
        }
    }
}

/// `Fade` 的作用对象。音频四项对应混音器的四条通道；`Screen` 走
/// [`UiPort`](crate::runner::UiPort)（黑幕压满整屏，阻塞到拍数走完）；`All` = 音频四条一起。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FadeTarget {
    Bgm,
    Bgs,
    Me,
    Se,
    Screen,
    All,
}

impl FadeTarget {
    /// 本目标覆盖哪些音频通道（`Screen` 为空 ⇒ 端口一条都不用碰）。
    pub fn channels(self) -> &'static [SoundChannel] {
        use FadeTarget::*;
        match self {
            Bgm => &SoundChannel::ALL[0..1],
            Bgs => &SoundChannel::ALL[1..2],
            Me => &SoundChannel::ALL[2..3],
            Se => &SoundChannel::ALL[3..4],
            All => &SoundChannel::ALL,
            Screen => &[],
        }
    }

    pub fn is_screen(self) -> bool {
        matches!(self, Self::Screen)
    }
}

/// 移动指令的一步（可序列化的
/// [`RouteNode`](sisf_sprite::task::RouteNode) 镜像）。
///
/// 不复用 `RouteNode` 本身：它是运行期类型（没有 serde、`Wait` 的单位是拍而磁盘上想写
/// 得清楚），而指令表要能存盘。这里的单位一律写进名字。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum RouteStep {
    /// 原地停 `frames` 拍（步帧归 0）。
    Wait { frames: u32 },
    /// 转身一拍（不位移）。
    Turn { dir: u8 },
    /// 朝 `dir` 走 `cells` 格（自由模式下单位是 1 像素）。
    Move { dir: u8, cells: u32 },
    /// 走到 `(x, y)`（虚拟像素）。
    MoveTo { x: f32, y: f32 },
    /// 朝 `dir` 跳 `cells` 格：腾空期间不查阻挡。
    Jump { dir: u8, cells: u32 },
    /// 朝向锁（RPG Maker 的 Direction Fix）。
    FacingLock { locked: bool },
}

/// 一条可序列化的移动路线。`through` = 无视阻挡；网格的边长来自
/// [`Scene::tile`](crate::runner::Scene)，不在每条路线里重复写。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RouteSpec {
    pub steps: Vec<RouteStep>,
    /// 每拍位移的像素数（`≤ 1` 视作 1）。
    pub speed: f32,
    pub through: bool,
}

impl RouteSpec {
    pub fn new(steps: Vec<RouteStep>) -> Self {
        Self {
            steps,
            speed: 1.0,
            through: false,
        }
    }
}

/// `Animate` 的效果。每一条都被编译成一个 [`Tween`](sisf_sprite::task::Tween)，所以这里
/// 不出现"想要但还没通路"的变体（DoD-4）。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum AnimEffect {
    /// 不透明度淡到 `to`（`frames` 拍）。
    Fade { to: f32, frames: u32 },
    /// 白闪：tone 从满到 0，`frames` 拍。
    Flash { frames: u32 },
    /// 左右抖 `px` 幅度共 `frames` 拍（落回原位）。
    Shake { px: f32, frames: u32 },
    /// 缩放淡入淡出（SLG 选中格那一类脉冲）。
    Pulse { scale: f32, frames: u32 },
}

/// 脚本后端（QuickJS / 移植层）注入自定义指令时的载荷。
///
/// 选这个形状是因为它与 `serde_json::Value` 一一对应（[`Param::to_json`] /
/// [`Param::from_json`]），跨 wasm 边界不引第三方 schema。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Param {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    List(Vec<Param>),
}

impl Param {
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::{Number, Value};
        match self {
            Self::Null => Value::Null,
            Self::Bool(b) => Value::Bool(*b),
            Self::Int(i) => Value::from(*i),
            Self::Float(f) => Number::from_f64(*f)
                .map(Value::Number)
                .unwrap_or(Value::Null),
            Self::Text(t) => Value::String(t.clone()),
            Self::List(l) => Value::Array(l.iter().map(Self::to_json).collect()),
        }
    }

    /// `Value` → `Param`：非有限浮点降级为 `Null`（JSON 里 `NaN` 本来就没有表示）。
    pub fn from_json(v: &serde_json::Value) -> Self {
        use serde_json::Value;
        match v {
            Value::Null => Self::Null,
            Value::Bool(b) => Self::Bool(*b),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Self::Int(i)
                } else {
                    n.as_f64().map(Self::Float).unwrap_or(Self::Null)
                }
            }
            Value::String(s) => Self::Text(s.clone()),
            Value::Array(a) => Self::List(a.iter().map(Self::from_json).collect()),
            // `Param` 没有字典变体（RON 里对象就是具名字段，不需要第二种映射）：
            // 上层要传结构就把它序列化成 `List`，实在拿到对象则退化成它的 JSON 文本。
            Value::Object(_) => Self::Text(v.to_string()),
        }
    }

    /// 取整数（`Float` 向零截断、文本按十进制解析），拿不到就 `None`。
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            Self::Float(f) if f.is_finite() => Some(*f as i64),
            Self::Text(t) => t.trim().parse().ok(),
            Self::Bool(b) => Some(i64::from(*b)),
            _ => None,
        }
    }
}

/// 一条游戏事件指令（MVP 集）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Command {
    /// 消息窗：正文 + 可选说话人 + 可选选项。
    ///
    /// - `choices` 空 ⇒ 按 `ok` 关掉；非空 ⇒ 上下键移光标、`ok` 确认，把**从 0 起的序号**
    ///   写进 `result`。没给 `result` 就无法分支（窗口照样能选，只是选完没人知道）。
    /// - `body` 里的 `\n` 是硬换行，超宽由文字引擎折行（T6）。
    /// - `cursor_se` 是移动光标时的音效（RPG Maker 惯例），`None` = 静音。
    ShowText {
        speaker: Option<String>,
        body: String,
        #[serde(default)]
        choices: Vec<String>,
        #[serde(default)]
        result: Option<Slot>,
        cursor_se: Option<Sound>,
        /// `false`（缺省）⇒ 按 `cancel` 无效；`true` ⇒ 取消返回 `-1`（写进 `result`）。
        #[serde(default)]
        allow_cancel: bool,
    },
    /// 等一个按键，把它的**十进制数值**写进 `slot`。只有带可打印字符的按下才算答案：非数字
    /// 字符写 `0`（"按下了某个键"），没有字符或只有控制字符（`\r`、方向键那类）的按下**不算
    /// 答案** —— 那一次按下多半是刚把上一句 `ShowText` 关掉的那一下（见 `docs/events.md`
    /// "总线纪律"第 3 条）。`timeout` = 拍数，`0` = 无限等；超时或被 `cancel` 打断时**不改**槽。
    InputKey {
        slot: Slot,
        timeout: u32,
    },
    /// 挂起 `frames` 拍。
    Wait {
        frames: u32,
    },
    /// 把路线交给任务调度器；`wait: true` ⇒ 阻塞到跑完。撞墙作废剩余格数，路线继续。
    MoveRoute {
        actor: u32,
        route: RouteSpec,
        #[serde(default)]
        wait: bool,
    },
    /// 把效果交给任务调度器（补间）；`wait: true` ⇒ 阻塞到播完。
    Animate {
        actor: u32,
        effect: AnimEffect,
        #[serde(default)]
        wait: bool,
    },
    /// 起播（端口是 [`AudioPort`](crate::runner::AudioPort)，四条通道对应混音器的四条）。
    /// `Bgm` 占掉 `Bgs` 时按混音器规则淡掉它。
    PlayBgm(Sound),
    PlayBgs(Sound),
    PlayMe(Sound),
    PlaySe(Sound),
    /// 淡入淡出。`out: true` = 淡到无声/全黑，`false` = 淡回原状。拍数换算成毫秒由端口
    /// 按混音器采样率做（`frames / tick_hz`）。
    Fade {
        target: FadeTarget,
        frames: u32,
        out: bool,
    },
    SetVar {
        slot: Slot,
        op: SetOp,
        value: Expr,
    },
    If {
        cond: Cond,
        then: Vec<Command>,
        #[serde(rename = "else")]
        else_: Vec<Command>,
    },
    Loop(Vec<Command>),
    BreakLoop,
    /// 跳到本事件的第 `page` 页（从 0 起）并从头执行；越界 = 本事件结束。
    ///
    /// 这就是 RPG Maker 的"执行内容 → 翻页"：换页后条件重新求值，事件因此能自删自长。
    Jump {
        page: u32,
    },
    /// 擦掉当前事件自己（精灵从场景里移除）。
    Erase,
    /// 交给 [`ScriptBackend`](crate::script::ScriptBackend) 的口子：`name` 是主题，
    /// `args` 与 `serde_json` 同形。这台事件机没装后端、或者后端不认这个名字 ⇒ 虚拟机
    /// 登记一条告警后继续（不是空实现分支，DoD-4）。
    Custom(String, Vec<Param>),
}

/// 一个事件的磁盘形状：名字 + 页表（每页一条指令序列）。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EventDef {
    pub name: String,
    #[serde(default)]
    pub pages: Vec<Vec<Command>>,
}

/// 一条编译后的指令。控制流已被抹平，所以这里没有子树。
#[derive(Clone, Debug, PartialEq)]
pub enum Instr {
    /// 一条叶子指令（含 `Jump`/`Erase`/`Custom`）。
    Run(Command),
    /// 条件为假则跳到 `on_false`（`pc` 指向的下一条 = 为真的分支入口）。
    Branch {
        cond: Cond,
        on_false: usize,
    },
    Goto {
        to: usize,
    },
    /// 循环回边。`pc` 回跳也算执行了一条指令，所以无限 `Loop` 会被预算拦下。
    LoopBack {
        to: usize,
    },
}

/// 编译结果：一条扁平指令流。多事件共享同一份程序时用 `Arc` 包着。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Program {
    pub code: Vec<Instr>,
}

impl Program {
    pub fn is_empty(&self) -> bool {
        self.code.is_empty()
    }

    /// 由嵌套指令表编译（[`Command::If`]/[`Command::Loop`] 在这里被展平）。
    pub fn compile(commands: &[Command]) -> Result<Self, CommandError> {
        let mut p = Program::default();
        p.emit_block(commands, &mut Vec::new(), 0)?;
        Ok(p)
    }

    fn emit(&mut self, i: Instr) -> usize {
        self.code.push(i);
        self.code.len() - 1
    }

    /// 一个指令序列（`If` 的分支、`Loop` 的循环体都算）编译进当前尾部。
    ///
    /// `loops` 是外层循环的 `(start, exits)` 栈：`BreakLoop` 要知道回不去的那头在哪，
    /// 而"跳出去"的位置在编译到一半时还是未知数，于是登记进 `exits`，等外层收口时统一
    /// 打补丁（所以编译不做两遍）。
    fn emit_block(
        &mut self,
        commands: &[Command],
        loops: &mut Vec<(usize, Vec<usize>)>,
        depth: u32,
    ) -> Result<(), CommandError> {
        if depth > MAX_NESTING {
            return Err(CommandError::TooDeep { limit: MAX_NESTING });
        }
        for c in commands {
            match c {
                Command::If { cond, then, else_ } => {
                    let br = self.emit(Instr::Branch {
                        cond: cond.clone(),
                        on_false: 0,
                    });
                    self.emit_block(then, loops, depth + 1)?;
                    if else_.is_empty() {
                        let end = self.code.len();
                        self.code[br] = Instr::Branch {
                            cond: cond.clone(),
                            on_false: end,
                        };
                    } else {
                        let skip = self.emit(Instr::Goto { to: 0 });
                        let alt = self.code.len();
                        self.code[br] = Instr::Branch {
                            cond: cond.clone(),
                            on_false: alt,
                        };
                        self.emit_block(else_, loops, depth + 1)?;
                        let end = self.code.len();
                        self.code[skip] = Instr::Goto { to: end };
                    }
                }
                Command::Loop(body) => {
                    let start = self.code.len();
                    loops.push((start, Vec::new()));
                    self.emit_block(body, loops, depth + 1)?;
                    let (start, exits) = loops.pop().expect("配对 push");
                    self.emit(Instr::LoopBack { to: start });
                    let end = self.code.len();
                    for at in exits {
                        self.code[at] = Instr::Goto { to: end };
                    }
                }
                Command::BreakLoop => match loops.last_mut() {
                    Some((_, exits)) => {
                        exits.push(self.emit(Instr::Goto { to: 0 }));
                    }
                    None => return Err(CommandError::BreakOutsideLoop),
                },
                other => {
                    self.emit(Instr::Run(other.clone()));
                }
            }
        }
        Ok(())
    }
}

/// 嵌套深度的上限。`.ron` 是给用户写的，栈溢出不能当成一种可接受的失败模式；
/// 64 层分支对一份事件表来说也远远够了。
pub const MAX_NESTING: u32 = 64;

/// 编译/载入错误。
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("RON 解析失败: {0}")]
    Ron(String),
    #[error("嵌套超过 {limit} 层")]
    TooDeep { limit: u32 },
    #[error("BreakLoop 出现在 Loop 之外")]
    BreakOutsideLoop,
}

/// 一个事件编译后的页表：每页一条程序。运行期持有它的是
/// [`EventRunner`](crate::runner::EventRunner) 的调用帧。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EventScript {
    pub name: String,
    pub pages: Vec<Program>,
}

impl EventDef {
    /// 从 RON 文本载入并编译（`examples/*` 与 T11 的 `data/*.json` 转换层都走这里）。
    pub fn from_ron(src: &str) -> Result<Self, CommandError> {
        ron::from_str(src).map_err(|e| CommandError::Ron(e.to_string()))
    }

    /// 写成 RON（存盘/生成器用；与 [`EventDef::from_ron`] 往返一致）。
    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .expect("指令表只含文本/整数/浮点与有限嵌套，序列化不会失败")
    }

    pub fn compile(&self) -> Result<EventScript, CommandError> {
        Ok(EventScript {
            name: self.name.clone(),
            pages: self
                .pages
                .iter()
                .map(|page| Program::compile(page.as_slice()))
                .collect::<Result<_, _>>()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // T12：`cargo xtask wasm-test` 在 wasm 上跑这一组（见 docs/platforms.md）。
    use Command::*;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    /// 一份把**每个变体**都过一遍的事件表（往返测试的覆盖面就靠它撑）。
    fn kitchen_sink() -> EventDef {
        EventDef {
            name: "elder".into(),
            pages: vec![
                vec![
                    PlayBgm(Sound::new("audio/bgm01.ogg").gain(0.8).fade_in(400.0)),
                    ShowText {
                        speaker: Some("长老".into()),
                        body: "井里有光。\nYou sure?".into(),
                        choices: vec!["去看".into(), "回村".into()],
                        result: Some(Slot::int(3)),
                        cursor_se: Some(Sound::new("audio/se_cursor.wav").gain(0.6)),
                        allow_cancel: true,
                    },
                    If {
                        cond: Cond::Cmp {
                            lhs: Expr::Slot(Slot::int(3)),
                            op: CmpOp::Eq,
                            rhs: Expr::Lit(0),
                        },
                        then: vec![
                            MoveRoute {
                                actor: 1,
                                route: RouteSpec {
                                    steps: vec![
                                        RouteStep::Turn { dir: 6 },
                                        RouteStep::Move { dir: 6, cells: 3 },
                                        RouteStep::Jump { dir: 6, cells: 1 },
                                        RouteStep::MoveTo { x: 208.0, y: 96.0 },
                                        RouteStep::Wait { frames: 6 },
                                        RouteStep::FacingLock { locked: true },
                                    ],
                                    speed: 2.0,
                                    through: false,
                                },
                                wait: true,
                            },
                            Animate {
                                actor: 1,
                                effect: AnimEffect::Flash { frames: 8 },
                                wait: false,
                            },
                            SetVar {
                                slot: Slot::var("seen"),
                                op: SetOp::Set,
                                value: Expr::Lit(1),
                            },
                        ],
                        else_: vec![
                            SetVar {
                                slot: Slot::switch(7),
                                op: SetOp::Tog,
                                value: Expr::Lit(0),
                            },
                            Loop(vec![
                                Wait { frames: 4 },
                                If {
                                    cond: Cond::Switch { id: 7, on: true },
                                    then: vec![BreakLoop],
                                    else_: vec![],
                                },
                            ]),
                            InputKey {
                                slot: Slot::var("answer"),
                                timeout: 120,
                            },
                        ],
                    },
                    Fade {
                        target: FadeTarget::Screen,
                        frames: 20,
                        out: true,
                    },
                    PlaySe(Sound::new("audio/se_door.wav")),
                    PlayBgs(Sound::new("audio/bgs_wind.ogg")),
                    PlayMe(Sound::new("audio/me_win.mp3")),
                    Fade {
                        target: FadeTarget::Bgm,
                        frames: 30,
                        out: true,
                    },
                    Custom(
                        "rpgmv.transfer".into(),
                        vec![
                            Param::Int(2),
                            Param::Text("cave".into()),
                            Param::List(vec![Param::Float(1.5), Param::Bool(false), Param::Null]),
                        ],
                    ),
                    Jump { page: 1 },
                    Erase,
                ],
                vec![Wait { frames: 1 }],
            ],
        }
    }

    /// 验收：`.ron` 往返一致。
    #[test]
    fn ron_roundtrips_every_variant() {
        let def = kitchen_sink();
        let text = def.to_ron();
        let back = EventDef::from_ron(&text).expect("写出来的 RON 必须读得回来");
        assert_eq!(back, def);
    }

    /// 磁盘上的 RON 必须能直接手写：手写一段，读出同样的树。
    #[test]
    fn hand_written_ron_parses() {
        let src = r#"
(
    name: "sign",
    pages: [
        [
            ShowText(speaker: None, body: "村口", choices: [], result: None, cursor_se: None),
            Wait(frames: 10),
        ],
    ],
)
"#;
        let def = EventDef::from_ron(src).expect("缺省字段该能省");
        assert_eq!(def.pages[0].len(), 2);
        match &def.pages[0][0] {
            ShowText { body, .. } => assert_eq!(body, "村口"),
            other => panic!("期望 ShowText，得到 {other:?}"),
        }
    }

    /// 编译：分支/循环变成带目标的跳转，`BreakLoop` 落到循环之后。
    #[test]
    fn compile_flattens_control_flow() {
        let prog = Program::compile(&[If {
            cond: Cond::always(),
            then: vec![Loop(vec![
                SetVar {
                    slot: Slot::int(1),
                    op: SetOp::Add,
                    value: Expr::Lit(1),
                },
                BreakLoop,
            ])],
            else_: vec![Wait { frames: 2 }],
        }])
        .expect("合法");
        // 布局：Branch(→5) · SetVar · Goto(→4 出循环) · LoopBack(→1) · Goto(→6 跳过 else) · Wait
        assert_eq!(prog.code.len(), 6, "{prog:?}");
        assert!(matches!(prog.code[0], Instr::Branch { on_false: 5, .. }));
        assert!(matches!(
            prog.code[1],
            Instr::Run(SetVar { op: SetOp::Add, .. })
        ));
        // break 落在循环体之后、else 之前（`to: 4` = `LoopBack` 的下一条）。
        assert!(matches!(prog.code[2], Instr::Goto { to: 4 }));
        assert!(matches!(prog.code[3], Instr::LoopBack { to: 1 }));
        assert!(matches!(prog.code[4], Instr::Goto { to: 6 }));
        assert!(matches!(prog.code[5], Instr::Run(Wait { frames: 2 })));
    }

    /// `BreakLoop` 落在循环外是配置错误，不是"静默无事发生"。
    #[test]
    fn break_outside_loop_is_an_error() {
        assert_eq!(
            Program::compile(&[BreakLoop]).err().map(|e| e.to_string()),
            Some("BreakLoop 出现在 Loop 之外".into())
        );
    }

    /// 恶意深度要在**编译期**挡掉，而不是把测试进程的栈打穿。
    #[test]
    fn nesting_is_capped() {
        let mut cmds = vec![Wait { frames: 1 }];
        for _ in 0..(MAX_NESTING + 2) {
            cmds = vec![Command::Loop(cmds)];
        }
        assert!(matches!(
            Program::compile(&cmds),
            Err(CommandError::TooDeep { .. })
        ));
    }

    /// `Param` 与 JSON 的往返（脚本后端的编解码面）。
    #[test]
    fn params_roundtrip_through_json() {
        let src = vec![
            Param::Null,
            Param::Bool(true),
            Param::Int(-7),
            Param::Float(0.5),
            Param::Text("井".into()),
            Param::List(vec![Param::Int(1), Param::List(vec![])]),
        ];
        let json = Param::List(src.clone()).to_json();
        assert_eq!(Param::from_json(&json), Param::List(src));
        assert_eq!(Param::Text("42".into()).as_int(), Some(42));
        assert_eq!(Param::Text("井".into()).as_int(), None);
        // JSON 没有 NaN 的表示：降级成 Null 而不是炸掉。
        assert_eq!(
            Param::from_json(&serde_json::to_value(f64::NAN).unwrap()),
            Param::Null
        );
    }

    /// 空指令表编译成空程序（`Erase` 之后什么都没有的事件是合法配置）。
    #[test]
    fn empty_pages_compile_empty() {
        let s = EventDef {
            name: "empty".into(),
            pages: vec![vec![], vec![]],
        }
        .compile()
        .expect("空页合法");
        assert_eq!(s.pages.len(), 2);
        assert!(s.pages.iter().all(Program::is_empty));
    }
}
