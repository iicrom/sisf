//! MV 的事件指令码表 → 引擎的 [`Command`] 树。
//!
//! ## 码表不是背出来的
//!
//! 每条都对着官方 `js/rpg_objects/Game_Interpreter.js` 里 `prototype.commandNNN` 的
//! **实际取参顺序**写（该文件里存在这些 handler 这一事实就是下面的 `MV_HANDLERS` 表）。
//! 三个从源码里读到、但很容易凭记忆写错的点：
//!
//! - **123 才是"独立开关"**，124 是计时器（VX/Ace 的编号体系不是这样）。
//! - **`command101` 会前瞻吞掉紧随的 102/103/104** —— 文字和选项是同一个窗口，
//!   所以翻译时必须合并成一条 `ShowText{choices}`，而不是两条指令。
//! - **MV 没有"结束 if"的码**：`If` 的边界只由 `indent` 回落表达；`411` 是 ELSE、
//!   `402/403` 是选项分支、`413` 是 `Loop` 的回边、`601/602/603` 是战斗结果分支。
//!   这些都和它们的头同 indent。
//!
//! ## 三条降级规则（对应清单的"未覆盖码降级 + 不 panic"）
//!
//! 1. **MV 自己就没有 handler 的码**（`401/404/405/408/605/655` 这类续行、以及任何
//!    不认识的号）⇒ 静默跳过。这是 `executeCommand` 的原样行为
//!    （`if (typeof this[methodName] === 'function')` 之外只有 `_index++`），
//!    把"续行"报成"未实现"是刷噪音。
//! 2. **MV 有 handler、本层没建模的码** ⇒ [`Command::Custom`] 携带 `rpgmv.unimplemented`
//!    + 原始参数，并计数；同一码只 warn 一次（[`TranslateReport::warn_new`]）。
//! 3. 降级出去的指令**不带走它的缩进体**：MV 在这种码上是"整块跳过"（`skipBranch`），
//!    把 `then` 提到外层无条件执行会直接改变剧情走向，比丢内容严重得多。所以
//!    [`Parser::discard`] 会丢掉更深的行并计数。
//!
//! ## 单位换算
//!
//! - `230` 等待的参数**就是帧**（60fps 的帧），不是 0.1 秒 —— VX Ace 才是 0.1 秒。
//!   换成拍：`frames * tick_hz / 60`。
//! - `242/246` 音频淡出的参数是**秒**（`WebAudio.fadeOut` 用 `currentTime + duration`）。
//! - `221/222` 屏幕淡入淡出走 `fadeSpeed()`，那是一个常量 **24 拍**。
//! - 移动速度：`Game_CharacterBase.distancePerFrame = 2^realMoveSpeed / 256` 个**图块/帧**，
//!   乘图块边长（48）再乘"每拍几帧"才得到引擎要的"像素/拍"（[`mv_speed_px_per_tick`]）。

use std::collections::{BTreeMap, HashSet};
use std::sync::Mutex;

use sisf_event::{
    CmpOp, Command, Cond, EventDef, Expr, FadeTarget, Param, RouteSpec, RouteStep, SetOp, Slot,
};

use crate::game_vars::GameVars;
use crate::map::{MapEvent, MvAudio, RawCommand, RawRoute};

/// MV 的图块边长（`Tilemap.TILE_WIDTH`，MV/MZ 都是 48）。
pub const MV_TILE_PX: f32 = 48.0;
/// `fadeSpeed()` 的常量（拍）。
pub const MV_FADE_FRAMES: u32 = 24;
/// MV 的显示帧率，用来把"帧"换成引擎的"拍"。
pub const MV_FPS: f32 = 60.0;
/// 玩家在这个引擎侧的 actor 号（`MoveRoute.actor` 用；MV 那边是 `-1`）。
///
/// 宿主必须把玩家精灵注册到这个号上，否则 `205` 的"移动玩家"会落到一个不存在的 actor。
pub const PLAYER_ACTOR: u32 = 0;
/// "跳到不存在的页" = 本事件到此结束，对应 MV 的 `115`（`_index = _list.length`）。
///
/// 用这个而不是发明一条 `Command::Halt`：`EventRunner::jump` 对越界页号的定义就是
/// "把该 subject 的帧摘掉"，正是 115 的语义。
pub const EXIT_EVENT_PAGE: u32 = u32::MAX;
/// 降级指令的主题名（宿主可以按它装一个 `ScriptBackend` 来接管）。
pub const UNIMPLEMENTED: &str = "rpgmv.unimplemented";

/// 只作为上一条的续行存在于磁盘上的码（MV 侧无 handler）。
const CONTINUATION: &[u32] = &[401, 404, 405, 408, 605];
/// 与它们的头**同 indent** 的分支标记（由解析器结构化消费，不产出指令）。
const BRANCH_MARKER: &[u32] = &[402, 403, 411, 413, 601, 602, 603];

/// `Game_Interpreter` 里真实存在 `commandNNN` 的码（升序，供二分）。
///
/// 逐条来自官方源码的 handler 清单。**这张表决定了"静默跳过"与"降级 + 计数"的分界**：
/// 不在表里的码 MV 自己也不执行，表里的码 MV 会做某件事、而我们没建模就是丢了内容。
const MV_HANDLERS: &[u32] = &[
    101, 102, 103, 104, 105, 108, 111, 112, 113, 115, 117, 118, 119, 121, 122, 123, 124, 125, 126,
    127, 128, 129, 132, 133, 134, 135, 136, 137, 138, 139, 140, 201, 202, 203, 204, 205, 206, 211,
    212, 213, 214, 216, 217, 221, 222, 223, 224, 225, 230, 231, 232, 233, 234, 235, 236, 241, 242,
    243, 244, 245, 246, 249, 250, 251, 261, 281, 282, 283, 284, 285, 301, 302, 303, 311, 312, 313,
    314, 315, 316, 317, 318, 319, 320, 321, 322, 323, 324, 325, 326, 331, 332, 333, 334, 335, 336,
    337, 339, 340, 342, 351, 352, 353, 354, 355, 356, 402, 403, 411, 413, 601, 602, 603,
];

/// 翻译的单位基准（一个工程一份）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// 引擎的一拍 = 多少赫兹（`AppConfig.tick_hz`）。MV 的一切时间单位都是 60fps 的帧。
    pub tick_hz: u32,
    /// 图块边长（像素）。换算移动速度用，非 48 的工程（插件改过）可覆盖。
    pub tile_px: f32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            tick_hz: 60,
            tile_px: MV_TILE_PX,
        }
    }
}

impl Options {
    /// MV 的"帧"→ 引擎的"拍"。`tick_hz == 60` 时是恒等。
    pub fn frames_to_ticks(&self, frames: i64) -> u32 {
        if frames <= 0 {
            return 0;
        }
        ((frames as f64 * self.tick_hz as f64) / MV_FPS as f64)
            .round()
            .max(0.0) as u32
    }

    /// MV 的"秒"→ 引擎的"拍"（音频淡入淡出走这个）。
    pub fn seconds_to_ticks(&self, seconds: f64) -> u32 {
        if !seconds.is_finite() || seconds <= 0.0 {
            return 0;
        }
        (seconds * self.tick_hz as f64).round().max(0.0) as u32
    }
}

/// MV 的 `moveSpeed`（`2^s / 256` 图块/帧）→ 引擎要的"像素/拍"。
pub fn mv_speed_px_per_tick(move_speed: i64, opts: Options) -> f32 {
    // 2^s 在 s=8 时也才 256（= 1 图块/帧），不会因为移位溢出；用 powi 保住负数（编辑器
    // 里"慢速"档存在 s<3 的取值）。
    let tiles_per_frame = 2f64.powi(move_speed as i32) / 256.0;
    let px_per_frame = tiles_per_frame * opts.tile_px as f64;
    (px_per_frame * opts.tick_hz as f64 / MV_FPS as f64) as f32
}

/// 一次翻译的账（清单要"unimplemented 计数打印"，所以这份账就是返回值的一部分）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TranslateReport {
    /// 产出的指令条数（**整棵树**，含分支体内 —— 与 [`count_tree`] 同一个口径，
    /// 单事件与单页两种入口对同一份数据要给出同一个数）。
    pub commands: usize,
    /// MV 码 → 降级次数。
    pub unimplemented: BTreeMap<u32, usize>,
    /// 因为头指令被降级而丢掉的缩进行数。
    pub dropped_branch_lines: usize,
    /// 移动路线里被丢掉的步数（`RouteSpec` 表达不了的：改速度/换图/透明度…）。
    pub route_steps_dropped: usize,
}

impl TranslateReport {
    fn count(&mut self, code: u32) {
        *self.unimplemented.entry(code).or_insert(0) += 1;
    }

    /// 降级了多少**条**（不是码的种类数）。
    pub fn total_unimplemented(&self) -> usize {
        self.unimplemented.values().sum()
    }

    pub fn merge(&mut self, other: &TranslateReport) {
        self.commands += other.commands;
        self.dropped_branch_lines += other.dropped_branch_lines;
        self.route_steps_dropped += other.route_steps_dropped;
        for (code, n) in &other.unimplemented {
            *self.unimplemented.entry(*code).or_insert(0) += *n;
        }
    }

    /// 给人看的一行汇总（`rpgmv_shell` 进图时打这条，就是清单要's"计数打印"）。
    pub fn summary(&self) -> String {
        if self.unimplemented.is_empty() && self.route_steps_dropped == 0 {
            return format!("{} 条指令，全部落地", self.commands);
        }
        let codes: Vec<String> = self
            .unimplemented
            .iter()
            .map(|(c, n)| format!("{c}×{n}"))
            .collect();
        format!(
            "{} 条指令，降级 {} 种码 [{}]，丢缩进 {} 行，丢路线步 {} 步",
            self.commands,
            self.unimplemented.len(),
            codes.join(" "),
            self.dropped_branch_lines,
            self.route_steps_dropped
        )
    }

    /// 把**这个进程里第一次出现**的降级码打到 stderr，返回打了几行。
    ///
    /// "同一码只 warn 一次"是清单字面量，也是被 MV 工程的现实逼出来的：一张图里一个
    /// 未覆盖码可能出现几十次，刷屏之后就没有人看告警了。状态是进程级的（`static`），
    /// 所以翻译第二张地图时不会重复播报同一个码；要按工程重置请另开进程。
    pub fn warn_new(&self) -> usize {
        let mut seen = warned();
        let mut printed = 0;
        for code in self.unimplemented.keys() {
            if seen.insert(*code) {
                eprintln!("rpgmv: MV 指令 {code} 未建模，降级为 Custom(\"{UNIMPLEMENTED}\")");
                printed += 1;
            }
        }
        printed
    }
}

/// [`TranslateReport::warn_new`] 的进程级"已经说过"集合。
fn warned() -> std::sync::MutexGuard<'static, HashSet<u32>> {
    // `LazyLock` 而不是 `Mutex::new(HashSet::new())`：后者在 `static`  initializer 里调
    // 非 const 的 `HashSet::new` 编译不过。
    static WARNED: std::sync::LazyLock<Mutex<HashSet<u32>>> =
        std::sync::LazyLock::new(|| Mutex::new(HashSet::new()));
    WARNED.lock().unwrap_or_else(|e| e.into_inner())
}

/// 一个事件（含全部页）翻成 `EventDef`，交出去就能 `compile()` 进虚拟机。
pub fn translate_event(
    event: &MapEvent,
    map_id: u32,
    opts: Options,
) -> (EventDef, TranslateReport) {
    let mut def = EventDef {
        name: event.name.clone(),
        pages: Vec::new(),
    };
    let mut total = TranslateReport::default();
    for page in &event.pages {
        let mut p = Parser::new(&page.list, map_id, event.id, opts);
        let cmds = p.block_of(0);
        total.merge(&p.report);
        // 与 `translate_list` 同一个口径：整棵树，不只是顶层。
        total.commands += count_tree(&cmds);
        def.pages.push(cmds);
    }
    (def, total)
}

/// 一条原始指令列表（事件页或 `CommonEvents` 的一页）→ 引擎指令树。
pub fn translate_list(
    list: &[RawCommand],
    map_id: u32,
    event_id: u32,
    opts: Options,
) -> (Vec<Command>, TranslateReport) {
    let mut p = Parser::new(list, map_id, event_id, opts);
    let cmds = p.block_of(0);
    let mut rep = p.report;
    rep.commands = count_tree(&cmds);
    (cmds, rep)
}

fn count_tree(cmds: &[Command]) -> usize {
    cmds.iter()
        .map(|c| match c {
            Command::If { then, else_, .. } => 1 + count_tree(then) + count_tree(else_),
            Command::Loop(body) => 1 + count_tree(body),
            _ => 1,
        })
        .sum()
}

/// 一条磁盘上的移动路线 → [`RouteSpec`]（丢掉的步计入报告）。
pub fn translate_route(route: &RawRoute, opts: Options) -> (RouteSpec, TranslateReport) {
    let mut rep = TranslateReport::default();
    let mut steps = Vec::new();
    for c in &route.list {
        match route_step(c, opts) {
            Some(s) => steps.push(s),
            None => {
                if c.code != 0 {
                    // 0 是"路线结束"，本来就该没有步。
                    rep.route_steps_dropped += 1;
                }
            }
        }
    }
    let spec = RouteSpec {
        steps,
        speed: mv_speed_px_per_tick(4, opts).max(1.0),
        through: false,
    };
    (spec, rep)
}

/// `ROUTE_*` 码 → 一步。`None` = `RouteSpec` 表达不了（改速度/换图/透明度那类）。
fn route_step(c: &RawCommand, opts: Options) -> Option<RouteStep> {
    // 基本移动 1..4 = 下/左/右/上；转向 16..19 = 同样四个方向的"转身一拍"。
    const DIRS: [u8; 5] = [0, 2, 4, 6, 8];
    match c.code {
        0 => None,
        1..=4 => Some(RouteStep::Move {
            dir: DIRS[c.code as usize],
            cells: 1,
        }),
        // 5..8 是斜向，本引擎的 `RouteStep` 只有四向（`Jump` 亦然）⇒ 丢。
        9..=13 => None,
        // 跳跃：MV 给的是 `[dx, dy]` 格偏移。只有一轴非零时能翻成 `Jump{dir, cells}`。
        14 => {
            let (dx, dy) = (c.num(0), c.num(1));
            match (dx, dy) {
                (0, 0) => None,
                (0, _) => Some(RouteStep::Jump {
                    dir: if dy > 0 { 2 } else { 8 },
                    cells: dy.unsigned_abs() as u32,
                }),
                (_, 0) => Some(RouteStep::Jump {
                    dir: if dx > 0 { 6 } else { 4 },
                    cells: dx.unsigned_abs() as u32,
                }),
                // 斜跳在本引擎里没有落点（宁可不跳，也不跳成一个 MV 里不存在的位置）。
                _ => None,
            }
        }
        // `ROUTE_WAIT = 15`：MV 写的是 `_waitCount = params[0] - 1`（本帧已经在等了）。
        15 => Some(RouteStep::Wait {
            frames: opts.frames_to_ticks(c.num(0).saturating_sub(1).max(0)),
        }),
        16..=19 => Some(RouteStep::Turn {
            dir: DIRS[c.code as usize - 15],
        }),
        20..=23 => None,
        // 35/36 = 朝向固定 开/关 —— 引擎有对应的一步。
        35 => Some(RouteStep::FacingLock { locked: true }),
        36 => Some(RouteStep::FacingLock { locked: false }),
        _ => None,
    }
}

/// 递归下降的解析游标（`indent` 就是 MV 唯一的块边界）。
struct Parser<'a> {
    list: &'a [RawCommand],
    i: usize,
    map_id: u32,
    event_id: u32,
    opts: Options,
    report: TranslateReport,
}

impl<'a> Parser<'a> {
    fn new(list: &'a [RawCommand], map_id: u32, event_id: u32, opts: Options) -> Self {
        Self {
            list,
            i: 0,
            map_id,
            event_id,
            opts,
            report: TranslateReport::default(),
        }
    }

    fn at(&self, k: usize) -> Option<&RawCommand> {
        self.list.get(self.i + k)
    }

    fn code(&self) -> u32 {
        self.at(0).map(|c| c.code).unwrap_or(0)
    }

    fn indent(&self) -> u32 {
        self.at(0).map(|c| c.indent).unwrap_or(0)
    }

    /// 取走当前行。
    fn take(&mut self) -> RawCommand {
        let c = self.at(0).cloned().unwrap_or_default();
        self.i += 1;
        c
    }

    /// 读到 `indent` 为止的一整块（分支体、循环体都走这里）。
    fn block_of(&mut self, indent: u32) -> Vec<Command> {
        let mut out = Vec::new();
        while self.i < self.list.len() {
            let c = self.at(0).expect("长度查过");
            if c.indent < indent || BRANCH_MARKER.contains(&c.code) {
                break;
            }
            out.extend(self.head());
        }
        out
    }

    /// 一条头部指令（含它的续行与分支）。
    fn head(&mut self) -> Vec<Command> {
        let c = self.take();
        match c.code {
            101 | 102 => self.message(c),
            105 => self.scrolling(c),
            108 => {
                self.consume(CONTINUATION, c.indent);
                Vec::new()
            }
            111 => self.conditional(c),
            112 => self.loop_(c),
            113 => vec![Command::BreakLoop],
            115 => vec![Command::Jump {
                page: EXIT_EVENT_PAGE,
            }],
            121..=123 => self.control(c),
            201 => self.transfer(c),
            205 => self.move_route(c),
            214 => vec![Command::Erase],
            221 | 222 => vec![
                Command::Fade {
                    target: FadeTarget::Screen,
                    frames: MV_FADE_FRAMES,
                    out: c.code == 221,
                },
                // MV 在 `command221/222` 里紧接着 `wait(fadeSpeed())`：淡的过程本身占拍。
                Command::Wait {
                    frames: MV_FADE_FRAMES,
                },
            ],
            230 => vec![Command::Wait {
                frames: self.opts.frames_to_ticks(c.num(0)),
            }],
            241 | 245 | 249 | 250 => self.play(c),
            242 | 246 => vec![Command::Fade {
                target: if c.code == 242 {
                    FadeTarget::Bgm
                } else {
                    FadeTarget::Bgs
                },
                frames: self.opts.seconds_to_ticks(c.num(0) as f64),
                out: true,
            }],
            261 => vec![Command::Custom(
                "rpgmv.movie".into(),
                vec![Param::Text(c.text(0).to_owned())],
            )],
            355 => self.script(c),
            356 => vec![Command::Custom(
                "rpgmv.plugin".into(),
                vec![Param::Text(c.text(0).to_owned())],
            )],
            402 | 403 | 411 | 413 | 601 | 602 | 603 => {
                // 走到这里说明这条标记没有对应的头（畸形数据）。MV 会照跑其 handler，
                // 我们只能跳过它 —— 但它有 handler，属于"没建模"，所以计数而不是沉默。
                self.discard(&c)
            }
            other if MV_HANDLERS.binary_search(&other).is_err() => {
                // MV 自己也没有 handler（`executeCommand` 只做 `_index++`）⇒ 原样沉默跳过。
                Vec::new()
            }
            _ => self.discard(&c),
        }
    }

    /// 降级：`Custom(rpgmv.unimplemented)` + 丢掉这块的缩进内容。
    fn discard(&mut self, c: &RawCommand) -> Vec<Command> {
        self.report.count(c.code);
        let mut args = vec![Param::Int(i64::from(c.code))];
        args.extend(c.parameters.iter().map(Param::from_json));
        // 头下面所有更深的行都属于"这条会决定的分支"，整块丢弃（见模块文档第 3 条）。
        loop {
            while self.i < self.list.len() && self.indent() > c.indent {
                self.i += 1;
                self.report.dropped_branch_lines += 1;
            }
            // 同层的兄弟分支标记（411 / 601..603 / 402/403）也一起吃掉，否则会漏到外层。
            if self.i < self.list.len()
                && self.indent() == c.indent
                && BRANCH_MARKER.contains(&self.code())
            {
                self.i += 1;
                continue;
            }
            break;
        }
        vec![Command::Custom(UNIMPLEMENTED.into(), args)]
    }

    /// 吃掉紧随其后的续行码（`401`/`405`/`408`/`655`…），返回它们的 `parameters[0]`。
    /// `indent` 是那**头指令**的缩进：MV 的续行与头同级。
    fn consume(&mut self, codes: &[u32], indent: u32) -> Vec<String> {
        let mut lines = Vec::new();
        while self.i < self.list.len() && codes.contains(&self.code()) && self.indent() == indent {
            let c = self.take();
            lines.push(c.text(0).to_owned());
        }
        lines
    }

    // --- 消息族 ---------------------------------------------------------------

    /// `101`（可携带 `102` 的选项）与独立的 `102`。
    fn message(&mut self, c: RawCommand) -> Vec<Command> {
        let indent = c.indent;
        let mut out = Vec::new();
        let mut body = String::new();
        let mut choices: Vec<String> = Vec::new();
        let mut cancel = -1i64;
        let mut is_choice = false;
        if c.code == 101 {
            // 脸部图/背景/位置是呈现层：引擎的 `ShowText` 没有对应通路，
            // 走 `Custom` 让宿主自己接（有立绘 UI 的工程能直接接住）。
            let face = c.text(0);
            if !face.is_empty() {
                out.push(Command::Custom(
                    "rpgmv.face".into(),
                    vec![Param::Text(face.to_owned()), Param::Int(c.num(1))],
                ));
            }
            body = self.consume(&[401], indent).join("\n");
            if self.code() == 102 && self.indent() == indent {
                let ch = self.take();
                is_choice = true;
                choices = ch.texts(0);
                cancel = setup_cancel(&ch, &choices);
            } else if matches!(self.code(), 103 | 104) && self.indent() == indent {
                // 103/104（数字输入 / 物品选择）在 MV 与 101 同窗，本引擎没有对应控件。
                let extra = self.take();
                out.extend(self.discard(&extra));
            }
        } else {
            is_choice = true;
            choices = c.texts(0);
            cancel = setup_cancel(&c, &choices);
        }
        let result = if is_choice {
            Some(Slot::Var(choice_slot(self.map_id, self.event_id, self.i)))
        } else {
            None
        };
        out.push(Command::ShowText {
            speaker: None,
            body,
            choices,
            result: result.clone(),
            cursor_se: None,
            // MV：cancelType `-2` = 禁止取消（`>= len` 时上游自己折成 -2）。
            // 只有真的挂上选项才有"取消"这回事：纯消息窗口在 MV 里没有取消一说，
            // 翻成 `allow_cancel: true` 会让玩家按 ESC 时 VM 收到一个不存在的第 -1 项。
            allow_cancel: is_choice && cancel != -2,
        });
        if let Some(slot) = result {
            out.extend(self.choice_branches(indent, slot));
        }
        out
    }

    /// `402 When [n]` / `403 When Cancel` 的分支序列。
    fn choice_branches(&mut self, indent: u32, slot: Slot) -> Vec<Command> {
        let mut out = Vec::new();
        while self.i < self.list.len() && self.indent() == indent {
            let (code, branch) = match self.code() {
                402 => {
                    let c = self.take();
                    (c.code, Expr::Lit(c.num(0)))
                }
                403 => {
                    self.i += 1;
                    // 取消时引擎写 `-1`，而 MV 的 `403` 定义就是"分支值为负"。
                    (403, Expr::Lit(-1))
                }
                _ => break,
            };
            let _ = code;
            let body = self.block_of(indent + 1);
            out.push(Command::If {
                cond: Cond::Cmp {
                    lhs: Expr::Slot(slot.clone()),
                    op: CmpOp::Eq,
                    rhs: branch,
                },
                then: body,
                else_: Vec::new(),
            });
        }
        out
    }

    /// `105` 滚动文本：本引擎没有滚动窗口，降级成一条普通 `ShowText`（内容不丢，只是不动）。
    fn scrolling(&mut self, c: RawCommand) -> Vec<Command> {
        let body = self.consume(&[405], c.indent).join("\n");
        vec![Command::ShowText {
            speaker: None,
            body,
            choices: Vec::new(),
            result: None,
            cursor_se: None,
            allow_cancel: false,
        }]
    }

    // --- 控制流 ---------------------------------------------------------------

    /// `111` 条件分支（含 `411` 的 else）。
    fn conditional(&mut self, c: RawCommand) -> Vec<Command> {
        let Some(cond) = self.condition(&c) else {
            return self.discard(&c);
        };
        let then = self.block_of(c.indent + 1);
        let mut else_ = Vec::new();
        if self.i < self.list.len() && self.code() == 411 && self.indent() == c.indent {
            self.i += 1;
            else_ = self.block_of(c.indent + 1);
        }
        vec![Command::If { cond, then, else_ }]
    }

    /// 只翻译开关 / 变量 / 独立开关三类；其余（计时器、角色、金币、物品、按键、脚本、载具）
    /// 在引擎侧没有对应的读法 ⇒ `None` ⇒ 整条降级。
    fn condition(&mut self, c: &RawCommand) -> Option<Cond> {
        match c.num(0) {
            0 => Some(Cond::Switch {
                id: c.num(1) as u32,
                // MV：`value(switchId) === (params[2] === 0)` ⇒ 2 号参数 0 才表示"ON"。
                on: c.num(2) == 0,
            }),
            1 => {
                let rhs = if c.num(2) == 0 {
                    Expr::Lit(c.num(3))
                } else {
                    Expr::Slot(Slot::int(c.num(3) as u32))
                };
                Some(Cond::Cmp {
                    lhs: Expr::Slot(Slot::int(c.num(1) as u32)),
                    op: cmp_code(c.num(4))?,
                    rhs,
                })
            }
            2 => {
                // 与事件页条件同一个键（`GameVars::ss_key`），读法也同一个。
                let slot = Slot::var(GameVars::ss_key(self.map_id, self.event_id, c.text(1)));
                Some(Cond::Cmp {
                    lhs: Expr::Slot(slot),
                    op: CmpOp::Eq,
                    rhs: Expr::Lit(if c.num(2) == 0 { 1 } else { 0 }),
                })
            }
            _ => None,
        }
    }

    /// `112` … `413`。
    fn loop_(&mut self, c: RawCommand) -> Vec<Command> {
        let body = self.block_of(c.indent + 1);
        if self.i < self.list.len() && self.code() == 413 && self.indent() == c.indent {
            self.i += 1;
        }
        vec![Command::Loop(body)]
    }

    // --- 变量族 ---------------------------------------------------------------

    fn control(&mut self, c: RawCommand) -> Vec<Command> {
        match c.code {
            121 => {
                // `[begin, end, value]`，value 0 = ON（MV 的 `setValue(i, params[2] === 0)`）。
                let mut out = Vec::new();
                for id in range_ids(c.num(0), c.num(1)) {
                    out.push(Command::SetVar {
                        slot: Slot::switch(id),
                        op: SetOp::Set,
                        value: Expr::Lit(if c.num(2) == 0 { 1 } else { 0 }),
                    });
                }
                out
            }
            122 => {
                // `[begin, end, op, operandType, p4, p5]`。
                let Some(op) = set_op(c.num(2)) else {
                    return self.discard(&c);
                };
                let value = match c.num(3) {
                    0 => Expr::Lit(c.num(4)),
                    1 => Expr::Slot(Slot::int(c.num(4) as u32)),
                    // MV 对范围内**每个**变量各抽一次随机数；每个变量一条 `SetVar` 自然同形。
                    2 => Expr::Rand {
                        lo: c.num(4),
                        hi: c.num(5),
                    },
                    // 3 = 游戏数据（等级/金币/队伍…），4 = 脚本 eval：本层没有读数通路。
                    _ => return self.discard(&c),
                };
                let mut out = Vec::new();
                for id in range_ids(c.num(0), c.num(1)) {
                    out.push(Command::SetVar {
                        slot: Slot::int(id),
                        op,
                        value: value.clone(),
                    });
                }
                out
            }
            // `[ch, value]`，value 0 = ON；键永远落在"当前图当前事件"上（MV 的 `_eventId > 0`）。
            _ => {
                if self.event_id == 0 {
                    // MV 在这种情况下的表现是"什么都不做"（`common event` 没有自我开关）。
                    return Vec::new();
                }
                vec![Command::SetVar {
                    slot: Slot::var(GameVars::ss_key(self.map_id, self.event_id, c.text(0))),
                    op: SetOp::Set,
                    value: Expr::Lit(if c.num(1) == 0 { 1 } else { 0 }),
                }]
            }
        }
    }

    // --- 场景族 ---------------------------------------------------------------

    /// `201` 转移：引擎没有"换图"指令（换图是宿主的事），交成一个具名 `Custom`。
    fn transfer(&mut self, c: RawCommand) -> Vec<Command> {
        vec![Command::Custom(
            "rpgmv.transfer".into(),
            vec![
                // 0 = 直接给定，1 = 三个数都是变量号（宿主读 `VarStore` 再折）。
                Param::Int(c.num(0)),
                Param::Int(c.num(1)),
                Param::Int(c.num(2)),
                Param::Int(c.num(3)),
                Param::Int(c.num(4)),
                Param::Int(c.num(5)),
            ],
        )]
    }

    /// `205` 移动路线。
    fn move_route(&mut self, c: RawCommand) -> Vec<Command> {
        let actor = match c.num(0) {
            // MV：`character(param)` 里负数一律是玩家，0 是"本事件"，>0 是事件号。
            n if n < 0 => PLAYER_ACTOR,
            0 => self.event_id,
            n => n as u32,
        };
        let Some(v) = c.parameters.get(1) else {
            return self.discard(&c);
        };
        let Ok(route) = serde_json::from_value::<RawRoute>(v.clone()) else {
            return self.discard(&c);
        };
        let (spec, rep) = translate_route(&route, self.opts);
        self.report.merge(&rep);
        vec![Command::MoveRoute {
            actor,
            route: spec,
            // MV：`if (params[1].wait) setWaitMode('route')`。
            wait: route.wait,
        }]
    }

    // --- 音频族 ---------------------------------------------------------------

    fn play(&mut self, c: RawCommand) -> Vec<Command> {
        let Some(v) = c.parameters.first() else {
            return self.discard(&c);
        };
        let audio = MvAudio::from_value(v);
        // MV 的 `AudioManager.play*(config)`：`config.name` 为空 = 停掉当前这条，
        // 而本引擎的 `Command` 里没有"停"（只有淡出）⇒ 当作未建模，让计数暴露它。
        let Some(sound) = audio.sound(match c.code {
            241 => "bgm",
            245 => "bgs",
            249 => "me",
            _ => "se",
        }) else {
            return self.discard(&c);
        };
        vec![match c.code {
            241 => Command::PlayBgm(sound),
            245 => Command::PlayBgs(sound),
            249 => Command::PlayMe(sound),
            _ => Command::PlaySe(sound),
        }]
    }

    /// `355` 脚本（`655` 是它的续行）。引擎侧有明确的口子：`Custom` + `ScriptBackend`。
    fn script(&mut self, c: RawCommand) -> Vec<Command> {
        let mut code = c.text(0).to_owned();
        for line in self.consume(&[655], c.indent) {
            code.push('\n');
            code.push_str(&line);
        }
        vec![Command::Custom(
            "rpgmv.script".into(),
            vec![Param::Text(code)],
        )]
    }
}

/// `setupChoices` 里那句 `if (cancelType >= choices.length) cancelType = -2`。
fn setup_cancel(c: &RawCommand, choices: &[String]) -> i64 {
    let cancel = c.num(1);
    if cancel >= choices.len() as i64 {
        -2
    } else {
        cancel
    }
}

/// 选项结果的合成槽：MV 把它放在解释器内部的 `_branch[_indent]`，引擎侧没有那个格子，
/// 于是按"图号 + 事件号 + 行号"起一个唯一的变量名（进存档也无妨：它就是个 `Slot::Var`）。
fn choice_slot(map_id: u32, event_id: u32, line: usize) -> String {
    format!("rpgmv.choice.{map_id}.{event_id}.{line}")
}

/// MV 的比较码（`command111` 的 `params[4]`）。
fn cmp_code(n: i64) -> Option<CmpOp> {
    Some(match n {
        0 => CmpOp::Eq,
        1 => CmpOp::Ge,
        2 => CmpOp::Le,
        3 => CmpOp::Gt,
        4 => CmpOp::Lt,
        5 => CmpOp::Ne,
        _ => return None,
    })
}

/// `operateVariable` 的 op 0..5（0 设定、1 加、2 减、3 乘、4 除、5 取模）。
fn set_op(n: i64) -> Option<SetOp> {
    Some(match n {
        0 => SetOp::Set,
        1 => SetOp::Add,
        2 => SetOp::Sub,
        3 => SetOp::Mul,
        4 => SetOp::Div,
        5 => SetOp::Mod,
        _ => return None,
    })
}

/// MV 的范围参数（`for (i = params[0]; i <= params[1]; i++)`）：0 号被 MV 的写边界挡掉，
/// 反过来的范围当空处理（宁可不写，也不写出一片不该动的槽）。
fn range_ids(begin: i64, end: i64) -> Vec<u32> {
    let begin = begin.max(1);
    if end < begin {
        return Vec::new();
    }
    (begin..=end)
        .filter_map(|i| u32::try_from(i).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sisf_event::Sound;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    fn cmd(code: u32, params: serde_json::Value) -> RawCommand {
        RawCommand {
            code,
            indent: 0,
            parameters: match params {
                serde_json::Value::Array(a) => a,
                v => vec![v],
            },
        }
    }

    fn indented(code: u32, indent: u32, params: serde_json::Value) -> RawCommand {
        let mut c = cmd(code, params);
        c.indent = indent;
        c
    }

    fn t(list: Vec<RawCommand>) -> (Vec<Command>, TranslateReport) {
        translate_list(&list, 1, 3, Options::default())
    }

    /// 验收（T11）：`101 + 401` 合成一条 `ShowText`，正文逐行拼接。
    #[test]
    fn show_text_merges_its_continuation_lines() {
        let list = vec![
            cmd(101, json!(["", 0, 0, 0])),
            cmd(401, json!(["第一行"])),
            cmd(401, json!(["第二行"])),
        ];
        let (out, rep) = t(list);
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0],
            Command::ShowText {
                speaker: None,
                body: "第一行\n第二行".into(),
                choices: Vec::new(),
                result: None,
                cursor_se: None,
                allow_cancel: false,
            }
        );
        // 401 在 MV 里没有 handler，不该被算成"未实现"。
        assert!(rep.unimplemented.is_empty());
    }

    /// `101` 后面紧跟 `102` 时两者同窗（`command101` 前瞻消费），选项分支跟着排下来。
    #[test]
    fn text_and_choices_share_one_window() {
        let list = vec![
            cmd(101, json!(["Actor1", 0, 0, 0])),
            cmd(401, json!(["往哪走？"])),
            indented(102, 0, json!([["左边", "右边"], -1, 0, 2, 0])),
            indented(402, 0, json!([0])),
            indented(101, 1, json!(["", 0, 0, 0])),
            indented(401, 1, json!(["往左"])),
            indented(402, 0, json!([1])),
            indented(101, 1, json!(["", 0, 0, 0])),
            indented(401, 1, json!(["往右"])),
            indented(403, 0, json!([])),
            indented(101, 1, json!(["", 0, 0, 0])),
            indented(401, 1, json!(["别乱跑"])),
        ];
        let (out, rep) = t(list);
        // Custom(rpgmv.face) + ShowText + 3 个 If（左/右/取消）。
        assert_eq!(out.len(), 5, "{out:?}");
        let Command::ShowText {
            body,
            choices,
            result,
            allow_cancel,
            ..
        } = &out[1]
        else {
            panic!("第 2 条应该是 ShowText：{:?}", out[1]);
        };
        assert_eq!(body, "往哪走？");
        assert_eq!(choices, &["左边".to_owned(), "右边".to_owned()]);
        assert!(*allow_cancel, "cancelType -1 = 允许取消");
        let slot = result.clone().expect("选项要有结果槽");
        assert!(matches!(slot, Slot::Var(_)));
        // 分支 0：`result == 0`，体里正是那条 101。
        let Command::If { cond, then, .. } = &out[2] else {
            panic!("应该是 If");
        };
        assert_eq!(
            cond,
            &Cond::Cmp {
                lhs: Expr::Slot(slot.clone()),
                op: CmpOp::Eq,
                rhs: Expr::Lit(0),
            }
        );
        assert_eq!(then.len(), 1);
        // 取消分支比较的是 `-1`（引擎在 allow_cancel 时写 -1）。
        let Command::If { cond, .. } = &out[4] else {
            panic!("取消分支");
        };
        assert_eq!(
            cond,
            &Cond::Cmp {
                lhs: Expr::Slot(slot),
                op: CmpOp::Eq,
                rhs: Expr::Lit(-1),
            }
        );
        assert!(rep.unimplemented.is_empty(), "{}", rep.summary());
    }

    /// `cancelType >= 选项数` 时 MV 自己折成 `-2`（禁止取消），翻译层跟着关。
    #[test]
    fn cancel_type_clamp_matches_setup_choices() {
        let list = vec![cmd(102, json!([["是", "否"], 5, 0, 2, 0]))];
        let (out, _) = t(list);
        let Command::ShowText { allow_cancel, .. } = &out[0] else {
            panic!()
        };
        assert!(!allow_cancel);
    }

    /// 条件分支的三类读数 + `411` 的 else。
    #[test]
    fn conditional_branch_folds_indent_into_if() {
        let list = vec![
            indented(111, 0, json!([0, 7, 0])), // 开关 7 为 ON
            indented(121, 1, json!([2, 2, 0])),
            indented(411, 0, json!([])),
            indented(121, 1, json!([2, 2, 1])),
        ];
        let (out, rep) = t(list);
        assert_eq!(
            out[0],
            Command::If {
                cond: Cond::Switch { id: 7, on: true },
                then: vec![Command::SetVar {
                    slot: Slot::switch(2),
                    op: SetOp::Set,
                    value: Expr::Lit(1),
                }],
                else_: vec![Command::SetVar {
                    slot: Slot::switch(2),
                    op: SetOp::Set,
                    value: Expr::Lit(0),
                }],
            }
        );
        assert_eq!(rep.commands, 3);

        // 变量比较：操作数来自另一个变量。
        let list = vec![indented(111, 0, json!([1, 4, 1, 9, 1]))];
        let (out, _) = t(list);
        let Command::If { cond, .. } = &out[0] else {
            panic!()
        };
        assert_eq!(
            cond,
            &Cond::Cmp {
                lhs: Expr::Slot(Slot::int(4)),
                op: CmpOp::Ge,
                rhs: Expr::Slot(Slot::int(9)),
            }
        );

        // 自我开关：键与事件页条件同源。
        let list = vec![indented(111, 0, json!([2, "A", 0]))];
        let (out, _) = t(list);
        let Command::If { cond, .. } = &out[0] else {
            panic!()
        };
        assert_eq!(
            cond,
            &Cond::Cmp {
                lhs: Expr::Slot(Slot::var("ss:1,3,A")),
                op: CmpOp::Eq,
                rhs: Expr::Lit(1),
            }
        );
    }

    /// 计时器/角色/金币/按键/脚本那一类条件在本层没有读数通路 ⇒ 整条降级，**分支不提上来**。
    #[test]
    fn unsupported_conditions_drop_their_bodies() {
        let list = vec![
            indented(111, 0, json!([7, 0, 100])), // 金币比较
            indented(121, 1, json!([5, 5, 0])),
        ];
        let (out, rep) = t(list);
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0],
            Command::Custom(
                UNIMPLEMENTED.into(),
                vec![
                    Param::Int(111),
                    Param::Int(7),
                    Param::Int(0),
                    Param::Int(100)
                ]
            )
        );
        assert_eq!(rep.dropped_branch_lines, 1, "开关 5 那行不该被无条件执行");
        assert_eq!(*rep.unimplemented.get(&111).unwrap_or(&0), 1);
    }

    /// `112 … 413` 成环，`113` 是断环；`115` 用"跳到不存在的页"表达。
    #[test]
    fn loop_break_and_exit_map_onto_the_vm_control_flow() {
        let list = vec![
            indented(112, 0, json!([])),
            indented(122, 1, json!([1, 1, 1, 0, 1])),
            indented(113, 1, json!([])),
            indented(413, 0, json!([])),
            indented(115, 0, json!([])),
        ];
        let (out, rep) = t(list);
        assert_eq!(
            out[0],
            Command::Loop(vec![
                Command::SetVar {
                    slot: Slot::int(1),
                    op: SetOp::Add,
                    value: Expr::Lit(1),
                },
                Command::BreakLoop,
            ])
        );
        assert_eq!(
            out[1],
            Command::Jump {
                page: EXIT_EVENT_PAGE
            }
        );
        assert_eq!(rep.dropped_branch_lines, 0);
        // 编译得过得去（`BreakLoop` 在 `Loop` 里面）。
        assert!(sisf_event::Program::compile(&out).is_ok());
    }

    /// 变量族：范围展开、操作码、以及随机数"每个变量各抽一次"。
    #[test]
    fn control_switches_and_variables_expand_ranges() {
        let (out, _) = t(vec![cmd(121, json!([0, 3, 1]))]);
        assert_eq!(out.len(), 3, "0 号被 MV 的写边界挡掉，1..=3 各一条");
        assert_eq!(
            out[0],
            Command::SetVar {
                slot: Slot::switch(1),
                op: SetOp::Set,
                value: Expr::Lit(0),
            }
        );
        let (out, _) = t(vec![cmd(122, json!([2, 3, 5, 2, 10, 20]))]);
        assert_eq!(
            out,
            vec![
                Command::SetVar {
                    slot: Slot::int(2),
                    op: SetOp::Mod,
                    value: Expr::Rand { lo: 10, hi: 20 },
                },
                Command::SetVar {
                    slot: Slot::int(3),
                    op: SetOp::Mod,
                    value: Expr::Rand { lo: 10, hi: 20 },
                },
            ]
        );
        // 游戏数据 / 脚本操作数没有读数通路 ⇒ 整条降级。
        let (out, rep) = t(vec![cmd(122, json!([1, 1, 0, 3, 6, 0]))]);
        assert!(matches!(&out[0], Command::Custom(n, _) if n == UNIMPLEMENTED));
        assert_eq!(*rep.unimplemented.get(&122).unwrap(), 1);
        // 独立开关 123（不是 124！）
        let (out, _) = t(vec![cmd(123, json!(["B", 0]))]);
        assert_eq!(
            out[0],
            Command::SetVar {
                slot: Slot::var("ss:1,3,B"),
                op: SetOp::Set,
                value: Expr::Lit(1),
            }
        );
    }

    /// 场景与音频族：转移/移动路线/淡入淡出/等待/播放，以及单位换算。
    #[test]
    fn scene_and_audio_commands_land_on_engine_primitives() {
        let (out, _) = t(vec![cmd(201, json!([0, 5, 8, 3, 6, 0]))]);
        assert_eq!(
            out[0],
            Command::Custom(
                "rpgmv.transfer".into(),
                vec![
                    Param::Int(0),
                    Param::Int(5),
                    Param::Int(8),
                    Param::Int(3),
                    Param::Int(6),
                    Param::Int(0)
                ]
            )
        );
        // 221 淡出 = Fade + Wait（MV 自己也是这么排的）。
        let (out, _) = t(vec![cmd(221, json!([]))]);
        assert_eq!(out.len(), 2);
        assert_eq!(
            out[0],
            Command::Fade {
                target: FadeTarget::Screen,
                frames: MV_FADE_FRAMES,
                out: true
            }
        );
        // 230 的 40 帧在 30Hz 的 tick 下是 20 拍。
        let (out, _) = translate_list(
            &[cmd(230, json!([40]))],
            1,
            3,
            Options {
                tick_hz: 30,
                tile_px: MV_TILE_PX,
            },
        );
        assert_eq!(out[0], Command::Wait { frames: 20 });
        // 241 播 BGM：音量 90 → 增益 0.9，路径按 MV 的目录约定拼。
        let (out, rep) = t(vec![cmd(
            241,
            json!([{"name": "Field01", "pan": 0, "pitch": 100, "volume": 90}]),
        )]);
        assert_eq!(
            out[0],
            Command::PlayBgm(Sound::new("audio/bgm/Field01.ogg").gain(0.9))
        );
        assert_eq!(rep.total_unimplemented(), 0);
        // 空 name = MV 的"停止"，本层没有停 ⇒ 计数，不静默。
        let (_, rep) = t(vec![cmd(241, json!([{"name": ""}]))]);
        assert_eq!(*rep.unimplemented.get(&241).unwrap(), 1);
        // 242 的参数是秒。
        let (out, _) = t(vec![cmd(242, json!([2]))]);
        assert_eq!(
            out[0],
            Command::Fade {
                target: FadeTarget::Bgm,
                frames: 120,
                out: true
            }
        );
        // 251 停 SE 没有通路。
        let (_, rep) = t(vec![cmd(251, json!([]))]);
        assert_eq!(*rep.unimplemented.get(&251).unwrap(), 1);
        // 214 擦除事件 / 261 播电影（交给视频桩）。
        let (out, _) = t(vec![cmd(214, json!([])), cmd(261, json!(["opening"]))]);
        assert_eq!(out[0], Command::Erase);
        assert_eq!(
            out[1],
            Command::Custom("rpgmv.movie".into(), vec![Param::Text("opening".into())])
        );
    }

    /// 移动路线：四向移动/转向/等待/朝向锁能翻，表达不了的步**计数**而不是瞎编。
    #[test]
    fn move_routes_translate_what_the_spec_can_hold() {
        let route = RawRoute {
            list: vec![
                cmd(2, json!([])),  // 左转（ROUTE_LEFT）
                cmd(18, json!([])), // 转向上（ROUTE_TURN_UP? → 19 才是上）
                cmd(15, json!([30])),
                cmd(35, json!([])),            // 朝向固定
                cmd(41, json!(["Actor1", 1])), // 换图：RouteSpec 装不下
                cmd(29, json!([6])),           // 改速度：同上
                cmd(0, json!([])),
            ],
            repeat: false,
            skippable: false,
            wait: true,
            walk: false,
        };
        let (spec, rep) = translate_route(&route, Options::default());
        assert_eq!(
            spec.steps,
            vec![
                RouteStep::Move { dir: 4, cells: 1 },
                RouteStep::Turn { dir: 6 },
                RouteStep::Wait { frames: 29 },
                RouteStep::FacingLock { locked: true },
            ]
        );
        assert_eq!(rep.route_steps_dropped, 2);
    }

    /// 速度换算钉住 MV 的 `2^s / 256` 图块/帧。
    #[test]
    fn move_speed_is_the_mv_power_curve() {
        let o = Options::default();
        assert_eq!(mv_speed_px_per_tick(4, o), 3.0, "s=4 ⇒ 3 像素/帧");
        assert_eq!(mv_speed_px_per_tick(3, o), 1.5);
        assert_eq!(mv_speed_px_per_tick(8, o), 48.0, "s=8 正好一格一帧");
        // 30Hz 的 tick 下每拍要跨两帧。
        let slow = Options {
            tick_hz: 30,
            tile_px: MV_TILE_PX,
        };
        assert_eq!(mv_speed_px_per_tick(4, slow), 1.5);
    }

    /// 脚本与插件命令走引擎留好的 `Custom` + `ScriptBackend` 口子。
    #[test]
    fn script_and_plugin_become_named_customs() {
        let list = vec![
            cmd(355, json!(["$gameVariables.setValue(1, 2);"])),
            cmd(655, json!(["$gameSwitches.setValue(3, true);"])),
            cmd(356, json!(["MyPlugin foo bar"])),
        ];
        let (out, rep) = t(list);
        assert_eq!(
            out[0],
            Command::Custom(
                "rpgmv.script".into(),
                vec![Param::Text(
                    "$gameVariables.setValue(1, 2);\n$gameSwitches.setValue(3, true);".into()
                )]
            )
        );
        assert_eq!(
            out[1],
            Command::Custom(
                "rpgmv.plugin".into(),
                vec![Param::Text("MyPlugin foo bar".into())]
            )
        );
        assert_eq!(rep.total_unimplemented(), 0, "655 是续行，不是未实现");
    }

    /// 验收（T11）："未覆盖码降级 + 不 panic + 只 warn 一次 + 计数打印"。
    #[test]
    fn unknown_codes_degrade_once_and_are_counted() {
        // 301（战斗）在 MV 有 handler，本层没有 ⇒ 降级，且它的 601/602 分支整块丢弃。
        let list = vec![
            indented(301, 0, json!([0, 1, false, true, 1])),
            indented(121, 1, json!([9, 9, 0])),
            indented(601, 0, json!([])),
            indented(121, 1, json!([10, 10, 0])),
            indented(603, 0, json!([])),
            indented(353, 1, json!([])),
        ];
        let (out, rep) = t(list);
        assert_eq!(out.len(), 1);
        // 计数口径是"缩进行"（字段文档那句）：同层的 601/603 只是分支标记，不算内容行。
        assert_eq!(
            rep.dropped_branch_lines, 3,
            "两个分支体里的 121 + 胜利分支里那条更深的 353"
        );
        // 999 这种 MV 自己都没有 handler 的码 ⇒ 沉默跳过（原样语义），不进计数。
        let (out, rep) = t(vec![cmd(999, json!([])), cmd(404, json!([]))]);
        assert!(out.is_empty(), "MV 不执行的码不该凭空产出指令");
        assert_eq!(rep.total_unimplemented(), 0);

        // 计数打印 + 同一码只 warn 一次（进程级集合，这里用两个互不相干的码各验一次）。
        let mut once = TranslateReport::default();
        once.count(907);
        assert_eq!(once.warn_new(), 1, "第一次要播出去");
        assert_eq!(once.warn_new(), 0, "同一码不再刷屏");
        let mut other = TranslateReport::default();
        other.count(908);
        other.count(908);
        assert_eq!(other.warn_new(), 1, "两种码各播一次，重复的那次被吸收");
        assert!(other.summary().contains("908×2"), "{}", other.summary());
        // 全落地的账要说得出口。
        let clean = TranslateReport {
            commands: 3,
            ..Default::default()
        };
        assert_eq!(clean.summary(), "3 条指令，全部落地");
    }

    /// 一个真事件的整页翻译：条件 → 消息 → 变量 → 转移，且 `EventDef` 编得进虚拟机。
    #[test]
    fn a_whole_event_page_compiles() {
        let ev = MapEvent {
            id: 3,
            name: "Well".into(),
            x: 2,
            y: 2,
            note: String::new(),
            pages: vec![crate::map::EventPage {
                list: vec![
                    indented(101, 0, json!(["", 0, 0, 0])),
                    indented(401, 0, json!(["井底有光。"])),
                    indented(122, 0, json!([1, 1, 0, 0, 1])),
                    indented(111, 0, json!([1, 1, 0, 1, 1])),
                    indented(201, 1, json!([0, 2, 4, 4, 8, 2])),
                ],
                ..Default::default()
            }],
        };
        let (def, rep) = translate_event(&ev, 1, Options::default());
        assert_eq!(def.name, "Well");
        assert_eq!(def.pages.len(), 1);
        assert_eq!(rep.total_unimplemented(), 0, "{}", rep.summary());
        let script = def.compile().expect("编译");
        assert_eq!(script.pages.len(), 1);
        // 页里的程序非空，且分支嵌在 If 里（`Program::compile` 会展平）。
        assert!(!script.pages[0].code.is_empty());
    }
}
