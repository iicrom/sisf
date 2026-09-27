//! 简易视觉小说 demo：三个场景 + 一张会分叉的剧情表，全部长在 SISF 上。
//!
//! 与 `rpg_demo` 的分工一致：**演出归事件虚拟机，世界归游戏侧**。差别是游戏侧这次什么
//! 都不"走" —— 玩家吃的输入只有 `ok`（推进）与选项光标，其余全部写在内联 RON 里：
//! 说话人、正文、分支、角色进出场的路线与淡入淡出。
//!
//! ```text
//! on_event ──原样──► EngineBus ──唯一读者──► EventRunner（台词 / 选项 / 角色位移）
//!    │                                          │
//!    │                                          ├─ Switch(n)   语气分支（改写本章台词）
//!    │                                          └─ Int(20)     「下一章编号」信箱
//!    └─ ctx.input ──ok──► 事件机在等人时由它消费；不等人的时候游戏侧抽干总线
//!
//! 游戏侧只管三件事：
//!   1. 章节表  `CHAPTERS[章号-1] = (场景, 事件表)` ⇒「这一章站在哪个房间里」
//!   2. 黑幕    信箱落下 ⇒ 淡出到全黑 ⇒ 那一拍换背景 + 摆位 + 起下一章 ⇒ 淡入
//!   3. 打字机  纯显示层：一个字都没浮出来之前，选项根本不出现，也不吃点击
//! ```
//!
//! ## 剧情图（分支不是装饰：它决定你走进哪一章）
//!
//! ```text
//! 1 教室 ──「好啊，一起去」──► 2 天台 ──► 4 天台·约定（暖收）
//!    └───────「今天，值日」──► 3 走廊 ──「追上去」──► 2 天台
//!                                 └──────「替我道歉」─► 5 教室·黑板（涩收）
//! ```
//!
//! ## 运行
//!
//! - 桌面：`cargo run -p sisf --example vn_demo`
//! - wasm：`cargo xtask web vn_demo` 后访问 `http://127.0.0.1:8080/?ex=vn_demo&wgl`
//!   （拉丁字体 Inter 来自本站 `assets/`，中文由宿主运行期从 CDN 补 —— 与 `rpg_demo`
//!   同一套定案，不需要往 `web/assets/` 加任何文件）
//! - 单测：`cargo test -p sisf --example vn_demo`（五份内联剧本编得过、剧情图连通且
//!   收口、每个被读的开关都有人写过、选项行不出窗、黑幕与打字机的拍数数学、剧本落点
//!   与舞台表对账）
//!
//! ## 手测清单（桌面与 wasm 同一份）
//!
//! 1. 开机从全黑淡入，标题卡「屋上」压在上面；按 Enter（或点任意处）进场，两拍后不按
//!    也自己走。
//! 2. 葵从左走进画面并淡出来（`MoveRoute` + `Animate Fade`）—— 她的出场是**事件表**排的，
//!    游戏侧只负责把她摆在画面外。
//! 3. Enter / 点击窗内 = 推进一句；正文逐字浮现（≈30 字/秒），没打完时点窗口只是**把这句
//!    补满**，不会跳过去。有选项的那句一定已经铺满，点哪行选哪行。
//! 4. 选项「好啊，一起去」→ 淡出换到**天台**；「今天，值日」→ 淡出换到**走廊**。走廊再分：
//!    追上去 → 天台（回第 2 章，开场多一句"我把健二一个人丢在楼梯口"）；替我道歉 → 教室·黑板。
//! 5. 背景与站位**只在全黑那一拍**换：淡出过程中看不到跳变，淡入进来就是新房间。
//! 6. 天台喂猫 ⇒ 第 4 章多一句"它记得那口小鱼干"；「我帮你说话」⇒ 结局卡文案换一类。
//! 7. 走到任一结局后弹结局卡；按 Enter 变量清零、从第一章重新淡入（可反复）。
//! 8. 事件机挂着（`hold text`）时方向键不推动世界；HUD 两行恒 ASCII，中文没到货也读得出
//!    "在第几章、挂在什么上"。

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
use sisf::app::wasm_bindgen;
use sisf::app::{App, AppConfig, FrameCtx, ScaleMode, Viewport, platform};
use sisf::asset::Vfs;
#[cfg(target_arch = "wasm32")]
use sisf::asset::vfs::path as vpath;
#[cfg(target_arch = "wasm32")]
use sisf::asset::{VfsPath, VfsProvider, WebManifest};
use sisf::event::{
    ActionEdge, AudioPort, EngineBus, EngineEvent, EventCtx, EventDef, EventRunner, EventScript,
    FadeTarget, PointerButton, PointerKind, Prompt, RunnerStatus, Scene, Sound, SoundChannel,
    UiPort, VarStore,
};
use sisf::render::{
    BatchKey, BatchList, Blend, DrawStats, Instance, NO_CLIP, Renderer, WHITE_EPOCH,
};
use sisf::sprite::{Layer, Sprite, SpriteArena, SpriteRef, TaskScheduler, Tone, Vec2, ViewportRef};
use sisf::text::{Align, FontBundle, TextEngine, TextStyle};

// ---------------------------------------------------------------- 画布与节拍

const VIRTUAL: (u32, u32) = (544, 416);
/// 角色脚底所在的横线。消息窗压在它上面，角色只露上半身 —— 视觉小说的惯例。
const GROUND: f32 = 316.0;
/// 矮个子（猫）的脚底线：栏杆那一档。它必须**整只落在消息窗上缘（`WIN[1] = 248`）以上**，
/// 否则"它蹲在栏杆上"那两句台词演的是窗后看不见的东西。
const CAT_LINE: f32 = 244.0;
/// 每逻辑拍最多推进 64 条指令：这台机器上没有真正的死循环，超预算只是把这一拍跑慢。
const BUDGET: u32 = 64;
const TICK_HZ: f32 = 60.0;
/// 淡出 / 淡入各 24 拍（0.4 秒）。
const FADE: u32 = 24;
/// 打字机：每两拍吐一个字（≈30 字/秒）。
const REVEAL_EVERY: u64 = 2;
/// 标题卡停留的拍数（按 `ok` 提前收）。
const TITLE_HOLD: u32 = 150;

const WORLD: Layer = Layer(0);
const UI_LAYER: u8 = 1;
const WORLD_VIEW: ViewportRef = ViewportRef::new(WORLD);

// ---------------------------------------------------------------- 信箱与开关

/// 信箱：事件表写下"下一章编号"（1..=5），0 = 没有。章号→场景由游戏侧的章节表决定 ——
/// 事件机不知道世界上有几个房间，这是 T10 定下的分工。
const INT_NEXT: u32 = 20;
/// 选项结果槽（从 0 起的序号；取消写 -1）。编号写死在内联 RON 里，这里只留一份给对账
/// 测试读的副本 —— 两边不一致就是"改了剧本没改文档"，`the_choice_slots_are_the_documented_ones` 拦。
#[cfg(test)]
const CHOICE_ROOFTOP: u32 = 10;
#[cfg(test)]
const CHOICE_CAT: u32 = 12;
#[cfg(test)]
const CHOICE_PROMISE: u32 = 13;
#[cfg(test)]
const CHOICE_HALL: u32 = 11;

/// 开关 = 语气分支，只改写台词，不改路由（路由只有 `INT_NEXT` 一条路）。
const SW_INVITED: u32 = 1;
const SW_CAME_BACK: u32 = 2;
const SW_FED_CAT: u32 = 3;
const SW_PROMISED: u32 = 4;

// ---------------------------------------------------------------- 场景与角色

/// 0 教室 · 1 走廊 · 2 天台。背景全是程序生成的色块：这个示例验的是事件机，不是美术。
const SCENES: [&str; 3] = ["classroom", "hallway", "rooftop"];

const AOI: u32 = 1;
const KENJI: u32 = 2;
const CAT: u32 = 3;

/// 停机场。
///
/// **必须是"走出画面外"而不是"沉到地板下"**：精灵 `anchor` 是 `(0.5, 1.0)`（底边中心），
/// 一块 172 高的身体停在 `y = GROUND + 180` 时上缘仍然落在画面里，于是每个离场的角色都会
/// 在对话窗后面露出一截身子。摆在 `x` 那头才是真的看不见。
const OFF: f32 = -220.0;

/// 角色规格：`(在场 x, 脚底线 y, 宽, 高, 身体色)`。`x == OFF` 即本章不在场。
#[derive(Clone, Copy)]
struct Staging {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [f32; 4],
}

const fn aoi(x: f32) -> Staging {
    Staging {
        x,
        y: GROUND,
        w: 76.0,
        h: 172.0,
        color: [0.36, 0.46, 0.78, 1.0],
    }
}

const fn kenji(x: f32) -> Staging {
    Staging {
        x,
        y: GROUND,
        w: 82.0,
        h: 178.0,
        color: [0.28, 0.58, 0.44, 1.0],
    }
}

/// 猫。它**不能站在 `GROUND` 上**：消息窗从 `WIN[1] = 248` 压下来，一只 44 高的角色停在
/// 脚底线 316 时整只都在窗后 —— 舞台调度得把它抬到栏杆那一档，观众才看得见它。
const fn cat(x: f32) -> Staging {
    Staging {
        x,
        y: CAT_LINE,
        w: 58.0,
        h: 44.0,
        color: [0.86, 0.62, 0.34, 1.0],
    }
}

/// 每一章的舞台调度：`(actor, 规格)`。下标 = 章号 - 1。
///
/// 这是"开场站位"，不是"全程站位" —— 角色在章内的移动全部由事件表的 `MoveRoute` 排。
const BLOCKINGS: [(Staging, Staging, Staging); 5] = [
    // 1 · 教室：葵从画面外走进来（她的 x 由剧本的 `MoveTo` 决定）
    (aoi(OFF), kenji(OFF), cat(OFF)),
    // 2 · 天台：葵已站定，猫从左边踱到栏杆下
    (aoi(348.0), kenji(OFF), cat(112.0)),
    // 3 · 走廊：健二从右侧压进来
    (aoi(OFF), kenji(470.0), cat(OFF)),
    // 4 · 天台·约定：两个人加一只猫都在
    (aoi(330.0), kenji(OFF), cat(172.0)),
    // 5 · 教室·黑板：只剩她
    (aoi(246.0), kenji(OFF), cat(OFF)),
];

/// 章号 → 场景下标。章号越界按最后一章处理（HUD 与淡入都要能读出数，不该 panic）。
const fn chapter_scene(chapter: usize) -> usize {
    match chapter {
        1 | 5 => 0,
        3 => 1,
        2 | 4 => 2,
        _ => 2,
    }
}

fn staging_of(chapter: usize, actor: u32) -> Staging {
    let row = BLOCKINGS
        .get(chapter.wrapping_sub(1))
        .copied()
        .unwrap_or(BLOCKINGS[0]);
    match actor {
        AOI => row.0,
        KENJI => row.1,
        _ => row.2,
    }
}

// ---------------------------------------------------------------- 内联事件表

/// 五章剧本内联在代码里（与 `rpg_demo` 同理：脚本即代码，桌面与 wasm 同源一份）。
/// 形状是 `EventDef { name, pages: [[Command]] }`，每章一页 —— 分支走 `If`（语气）与
/// `SetVar(Int(20))`（路由），"这一页此刻该不该出现"的页号选择这一版没用到。
const SCRIPTS: [&str; 5] = [
    // 1 · 教室：第一个分叉。答应葵 ⇒ 天台；推掉 ⇒ 走廊。
    r#"(
    name: "classroom",
    pages: [
        [
            Animate(actor: 1, effect: Fade(to: 1.0, frames: 12), wait: false),
            MoveRoute(
                actor: 1,
                route: (steps: [MoveTo(x: 176.0, y: 316.0)], speed: 2.2, through: true),
                wait: false,
            ),
            ShowText(
                speaker: None,
                body: "周五最后一节下课。空教室里只剩粉笔灰，和我没写完的值日表。",
                cursor_se: None,
            ),
            ShowText(
                speaker: Some("葵"),
                body: "那、那个……放学以后，你有空吗？",
                cursor_se: None,
            ),
            ShowText(
                speaker: Some("葵"),
                body: "天台上有只猫，它只肯让我以外的人靠近……大概吧。想让你也见见它。",
                choices: ["好啊，一起去", "今天，我值日"],
                result: Some(Int(10)),
                allow_cancel: false,
                cursor_se: None,
            ),
            If(
                cond: Cmp(lhs: Slot(Int(10)), op: Eq, rhs: Lit(0)),
                then: [
                    SetVar(slot: Switch(1), op: Set, value: Lit(1)),
                    ShowText(
                        speaker: Some("葵"),
                        body: "太好了！那我先上去放猫粮——你不许偷看我喂猫的样子。",
                        cursor_se: None,
                    ),
                    SetVar(slot: Int(20), op: Set, value: Lit(2)),
                ],
                else: [
                    ShowText(
                        speaker: Some("葵"),
                        body: "……这样啊。那、就当我没说过。值日辛苦了。",
                        cursor_se: None,
                    ),
                    ShowText(
                        speaker: None,
                        body: "她抱着笔记本快步出门。走廊那头，有人在拍篮球。",
                        cursor_se: None,
                    ),
                    SetVar(slot: Int(20), op: Set, value: Lit(3)),
                ],
            ),
        ],
    ],
)"#,
    // 2 · 天台：喂猫与约定。暖收的入口。
    r#"(
    name: "rooftop",
    pages: [
        [
            Animate(actor: 1, effect: Fade(to: 1.0, frames: 12), wait: false),
            Animate(actor: 3, effect: Fade(to: 1.0, frames: 12), wait: false),
            If(
                cond: Switch(id: 2, on: true),
                then: [
                    ShowText(
                        speaker: None,
                        body: "我把健二一个人丢在楼梯口，爬回了天台。门一开，风就把话吹散了。",
                        cursor_se: None,
                    ),
                ],
                else: [],
            ),
            ShowText(
                speaker: None,
                body: "天台。栏杆外是橙色的城市，那只三花已经蹲在消防梯口等我们。",
                cursor_se: None,
            ),
            MoveRoute(
                actor: 3,
                route: (steps: [MoveTo(x: 150.0, y: 244.0)], speed: 1.4, through: true),
                wait: false,
            ),
            ShowText(
                speaker: Some("葵"),
                body: "它今天来得好早。你看——它根本没躲。",
                cursor_se: None,
            ),
            ShowText(
                speaker: None,
                body: "喵。（她的指尖离它还有一拃，它先把头伸了出去。）",
                choices: ["喂它一口小鱼干", "只是看着"],
                result: Some(Int(12)),
                allow_cancel: false,
                cursor_se: None,
            ),
            If(
                cond: Cmp(lhs: Slot(Int(12)), op: Eq, rhs: Lit(0)),
                then: [
                    SetVar(slot: Switch(3), op: Set, value: Lit(1)),
                    Animate(actor: 3, effect: Flash(frames: 10), wait: false),
                    ShowText(
                        speaker: Some("葵"),
                        body: "它吃了！它昨天连我的都不吃——你到底是怎么做到的？",
                        cursor_se: None,
                    ),
                ],
                else: [
                    ShowText(
                        speaker: Some("葵"),
                        body: "不喂也好。它就喜欢有人陪着吹吹风，我也一样。",
                        cursor_se: None,
                    ),
                ],
            ),
            ShowText(
                speaker: Some("葵"),
                body: "……其实我叫你上来，是想说毕业的事。我想考东京的学校，家里人不同意。",
                cursor_se: None,
            ),
            ShowText(
                speaker: Some("葵"),
                body: "如果是你，会不会觉得——这个城市太小了？",
                choices: ["那就去，我帮你说话", "别冲动，先考试"],
                result: Some(Int(13)),
                allow_cancel: true,
                cursor_se: None,
            ),
            If(
                cond: Cmp(lhs: Slot(Int(13)), op: Eq, rhs: Lit(0)),
                then: [
                    SetVar(slot: Switch(4), op: Set, value: Lit(1)),
                    ShowText(
                        speaker: None,
                        body: "她把「我帮你说话」记在笔记本第一页，写完抬头冲我笑了一下。",
                        cursor_se: None,
                    ),
                ],
                else: [
                    ShowText(
                        speaker: Some("葵"),
                        body: "嗯……我知道。只是想说给一个人听，说了就当没说好了。",
                        cursor_se: None,
                    ),
                ],
            ),
            Animate(actor: 1, effect: Fade(to: 0.0, frames: 18), wait: false),
            Animate(actor: 3, effect: Fade(to: 0.0, frames: 18), wait: false),
            SetVar(slot: Int(20), op: Set, value: Lit(4)),
        ],
    ],
)"#,
    // 3 · 走廊：第二个分叉 —— 追上去（回天台），或者托话（走向涩收）。
    r#"(
    name: "hallway",
    pages: [
        [
            Animate(actor: 2, effect: Fade(to: 1.0, frames: 10), wait: false),
            MoveRoute(
                actor: 2,
                route: (steps: [MoveTo(x: 352.0, y: 316.0)], speed: 4.2, through: true),
                wait: false,
            ),
            ShowText(
                speaker: None,
                body: "楼梯口的风把值日表吹得哗哗响。有人比我先下来了。",
                cursor_se: None,
            ),
            ShowText(
                speaker: Some("健二"),
                body: "喂。你把葵撵走了吧？我在楼下都听见那句「算了」。",
                cursor_se: None,
            ),
            ShowText(
                speaker: Some("健二"),
                body: "她在那儿站了很久。你要现在上去，还是等我替你带句话？",
                choices: ["追上去", "拜托你替我说声抱歉"],
                result: Some(Int(11)),
                allow_cancel: false,
                cursor_se: None,
            ),
            If(
                cond: Cmp(lhs: Slot(Int(11)), op: Eq, rhs: Lit(0)),
                then: [
                    SetVar(slot: Switch(2), op: Set, value: Lit(1)),
                    Animate(actor: 2, effect: Shake(px: 3.0, frames: 12), wait: false),
                    ShowText(
                        speaker: Some("健二"),
                        body: "行啊你，跑得比我快。天台门给你留着——别又是半路回头。",
                        cursor_se: None,
                    ),
                    SetVar(slot: Int(20), op: Set, value: Lit(2)),
                ],
                else: [
                    ShowText(
                        speaker: Some("健二"),
                        body: "话带到。你自己不上去，回头可别怪我没提醒你。",
                        cursor_se: None,
                    ),
                    ShowText(
                        speaker: None,
                        body: "他的脚步声在拐角处没了。我一个人往教室那边走。",
                        cursor_se: None,
                    ),
                    Animate(actor: 2, effect: Fade(to: 0.0, frames: 14), wait: false),
                    SetVar(slot: Int(20), op: Set, value: Lit(5)),
                ],
            ),
        ],
    ],
)"#,
    // 4 · 天台·约定（暖收）：同一块场地，四句按开关改写。
    r#"(
    name: "rooftop_promise",
    pages: [
        [
            Animate(actor: 1, effect: Fade(to: 1.0, frames: 14), wait: false),
            Animate(actor: 3, effect: Fade(to: 1.0, frames: 14), wait: false),
            If(
                cond: Switch(id: 4, on: true),
                then: [
                    ShowText(
                        speaker: Some("葵"),
                        body: "我真的去跟家里说了。他们没答应，但也没说不许考。",
                        cursor_se: None,
                    ),
                ],
                else: [
                    ShowText(
                        speaker: Some("葵"),
                        body: "我想了很久，还是先把模拟考做完，再说那件事。",
                        cursor_se: None,
                    ),
                ],
            ),
            If(
                cond: Switch(id: 3, on: true),
                then: [
                    ShowText(
                        speaker: None,
                        body: "猫趴在我们中间，吃得毫无防备。它记得那口小鱼干。",
                        cursor_se: None,
                    ),
                ],
                else: [
                    ShowText(
                        speaker: None,
                        body: "猫蹲在栏杆上，尾巴慢慢扫。它没靠近，也没走。",
                        cursor_se: None,
                    ),
                ],
            ),
            ShowText(
                speaker: Some("葵"),
                body: "毕业那天，我们还来这里。谁都不许先离开这个城市。",
                cursor_se: None,
            ),
            ShowText(
                speaker: None,
                body: "风把这句话按在夕阳上，很久都没掉下去。（天台篇 · 完）",
                cursor_se: None,
            ),
        ],
    ],
)"#,
    // 5 · 教室·黑板（涩收）：值日擦掉的是黑板，不是那句话。
    r#"(
    name: "blackboard",
    pages: [
        [
            Animate(actor: 1, effect: Fade(to: 1.0, frames: 14), wait: false),
            MoveRoute(
                actor: 1,
                route: (steps: [MoveTo(x: 252.0, y: 316.0)], speed: 1.8, through: true),
                wait: false,
            ),
            If(
                cond: Switch(id: 1, on: true),
                then: [
                    ShowText(
                        speaker: None,
                        body: "黑板上还留着她说的那半句，板擦在讲台上等我伸手。",
                        cursor_se: None,
                    ),
                ],
                else: [
                    ShowText(
                        speaker: None,
                        body: "我回到教室，黑板已经被擦得很干净——干净得像没人说过话。",
                        cursor_se: None,
                    ),
                ],
            ),
            ShowText(
                speaker: Some("葵"),
                body: "……你怎么又回来了？猫粮忘在这儿了？",
                cursor_se: None,
            ),
            ShowText(
                speaker: None,
                body: "我说不出口，于是真的走过去，拿起了板擦。",
                cursor_se: None,
            ),
            ShowText(
                speaker: None,
                body: "我们一句话没说，把两块黑板都擦完了。（教室篇 · 完）",
                cursor_se: None,
            ),
        ],
    ],
)"#,
];

/// 换幕时打在 HUD 上的章名（中文，字体没到货时靠 ASCII 那两行读状态）。
const CHAPTER_TITLES: [&str; 5] = ["教室", "天台", "走廊", "天台·约定", "教室·黑板"];

struct Chapter {
    scene: usize,
    script: Arc<EventScript>,
}

fn compile_scripts() -> (Vec<EventScript>, Vec<String>) {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    for (i, src) in SCRIPTS.iter().enumerate() {
        match EventDef::from_ron(src).and_then(|def| def.compile()) {
            Ok(script) => out.push(script),
            Err(e) => errors.push(format!("第 {} 章编不过: {e}", i + 1)),
        }
    }
    (out, errors)
}

// ---------------------------------------------------------------- 资源（字体）

/// CJK 状态机与 `dialog_demo`/`rpg_demo` 同一套定案：拉丁随本站清单，中文由宿主运行期
/// 从 CDN 补，引擎不管那个 URL。字形族定形前一个字都不画（中途换族会在覆盖率图集里串字）。
#[derive(Clone)]
struct Cjk(Arc<AtomicU8>);

const CJK_PENDING: u8 = 0;
const CJK_SYSTEM: u8 = 1;
const CJK_ARRIVED: u8 = 2;
const CJK_NONE: u8 = 3;

impl Cjk {
    fn new() -> Self {
        #[cfg(target_arch = "wasm32")]
        let start = CJK_PENDING;
        #[cfg(not(target_arch = "wasm32"))]
        let start = CJK_SYSTEM;
        Self(Arc::new(AtomicU8::new(start)))
    }

    fn state(&self) -> u8 {
        self.0.load(Ordering::Relaxed)
    }

    #[cfg(target_arch = "wasm32")]
    fn set(&self, s: u8) {
        self.0.store(s, Ordering::Relaxed);
    }

    fn label(&self) -> &'static str {
        match self.state() {
            CJK_PENDING => "cjk pending",
            CJK_SYSTEM => "cjk system",
            CJK_ARRIVED => "cjk ok",
            CJK_NONE => "cjk MISSING",
            _ => "cjk ?",
        }
    }
}

#[cfg(target_arch = "wasm32")]
const FONT: &str = "Inter-Regular.ttf";
#[cfg(target_arch = "wasm32")]
const CJK: &str = "NotoSansSC-Regular.otf";
#[cfg(target_arch = "wasm32")]
const CJK_URL: &str = "https://cdn.jsdelivr.net/gh/notofonts/noto-cjk@f8d157532fbfaeda587e826d4cd5b21a49186f7c/Sans/SubsetOTF/SC/NotoSansSC-Regular.otf";
#[cfg(target_arch = "wasm32")]
const MANIFEST: &str = "Inter-Regular.ttf\n";

struct Assets {
    vfs: Vfs,
    font_loaded: bool,
    cjk: Cjk,
}

impl Assets {
    fn new() -> Self {
        let cjk = Cjk::new();
        #[cfg(not(target_arch = "wasm32"))]
        let vfs = Vfs::new();
        #[cfg(target_arch = "wasm32")]
        let vfs = {
            let manifest = Arc::new(WebManifest::new(MANIFEST));
            prefetch(&manifest);
            fetch_cjk(&manifest, &cjk);
            let mut vfs = Vfs::new();
            vfs.register(Box::new(SharedManifest(manifest)));
            vfs
        };
        Self {
            vfs,
            font_loaded: false,
            cjk,
        }
    }
}

/// wasm 宿主预取：按清单逐个 `fetch`，成字节后灌进 `WebManifest`（VFS 读取因此恒同步）。
#[cfg(target_arch = "wasm32")]
fn prefetch(manifest: &Arc<WebManifest>) {
    use sisf::app::{js_sys, wasm_bindgen_futures, web_sys};
    for p in manifest.pending() {
        let Some(window) = web_sys::window() else {
            break;
        };
        let m = Arc::clone(manifest);
        let key = p.as_str().to_owned();
        wasm_bindgen_futures::spawn_local(async move {
            let url = format!("assets/{key}");
            let outcome: Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> = async {
                let response =
                    wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(&url)).await?;
                let body = web_sys::Response::from(response).array_buffer()?;
                wasm_bindgen_futures::JsFuture::from(body).await
            }
            .await;
            match outcome {
                Ok(buffer) => m.insert(&vpath(&key), js_sys::Uint8Array::new(&buffer).to_vec()),
                Err(e) => {
                    web_sys::console::log_1(&format!("预取 {url} 失败: {e:?}").into());
                }
            }
        });
    }
}

/// 从 CDN 取中文轮廓。两头都要说话：到货说字节数，取不到说原因并写回 [`CJK_NONE`] ——
/// 少了后者，`load_font` 会一直等一份永远不来的字体，把本来能显示的拉丁一起拖死。
#[cfg(target_arch = "wasm32")]
fn fetch_cjk(manifest: &Arc<WebManifest>, cjk: &Cjk) {
    use sisf::app::{js_sys, wasm_bindgen_futures, web_sys};
    let Some(window) = web_sys::window() else {
        return;
    };
    let m = Arc::clone(manifest);
    let c = cjk.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let outcome: Result<Vec<u8>, String> = async {
            let response = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(CJK_URL))
                .await
                .map_err(|e| format!("{e:?}"))?;
            let response = web_sys::Response::from(response);
            if !response.ok() {
                return Err(format!("HTTP {}", response.status()));
            }
            let body = response.array_buffer().map_err(|e| format!("{e:?}"))?;
            let buffer = wasm_bindgen_futures::JsFuture::from(body)
                .await
                .map_err(|e| format!("{e:?}"))?;
            Ok(js_sys::Uint8Array::new(&buffer).to_vec())
        }
        .await;
        match outcome {
            Ok(bytes) if !bytes.is_empty() => {
                platform::log_info(&format!("[vn_demo] CJK 到货 {} 字节", bytes.len()));
                m.insert(&vpath(CJK), bytes);
                c.set(CJK_ARRIVED);
            }
            Ok(_) => {
                web_sys::console::log_1(&format!("{CJK_URL} 响应是空的").into());
                c.set(CJK_NONE);
            }
            Err(why) => {
                web_sys::console::log_1(&format!("取 {CJK_URL} 失败: {why}").into());
                c.set(CJK_NONE);
            }
        }
    });
}

#[cfg(target_arch = "wasm32")]
struct SharedManifest(Arc<WebManifest>);
#[cfg(target_arch = "wasm32")]
impl VfsProvider for SharedManifest {
    fn read(&self, p: &VfsPath) -> Option<Vec<u8>> {
        self.0.read(p)
    }

    fn exists(&self, p: &VfsPath) -> bool {
        self.0.exists(p)
    }
}

// ---------------------------------------------------------------- 两个端口

/// 本示例不带音频（剧本里没有一条 `Play*`，`cursor_se` 全 `None`），但 [`EventCtx`] 要求
/// 一个 [`AudioPort`] —— 桩的唯一义务是"真被调到了要说话"。
struct SilentAudio;

impl AudioPort for SilentAudio {
    fn play(&mut self, _vfs: &Vfs, channel: SoundChannel, sound: &Sound) -> Result<(), String> {
        Err(format!(
            "本示例没有音频：{} ← {}",
            channel.label(),
            sound.name
        ))
    }

    fn fade(&mut self, _target: FadeTarget, _frames: u32, _out: bool) -> Result<(), String> {
        Ok(())
    }

    fn stop(&mut self, _channel: SoundChannel) {}
}

/// [`UiPort`]：只镜像事件机推来的东西，自己不做任何显示决策。打字机长度也挂在这里 ——
/// 它是"这句该显示几个字"，属于显示状态，不属于剧情状态。
#[derive(Default)]
struct MessageBox {
    prompt: Option<Prompt>,
    dim: f32,
    /// 当前这句已经浮现出来的字数。
    revealed: usize,
    /// 上一次 `show` 看到的正文；变了 ⇒ 打字机从头开始。
    body: String,
}

impl UiPort for MessageBox {
    fn show(&mut self, prompt: &Prompt) {
        if prompt.body != self.body {
            self.body.clone_from(&prompt.body);
            self.revealed = 0;
        }
        self.prompt = Some(prompt.clone());
    }

    fn hide(&mut self) {
        self.prompt = None;
        self.body.clear();
        self.revealed = 0;
    }

    fn set_dim(&mut self, amount: f32) {
        self.dim = amount;
    }
}

impl MessageBox {
    fn typing_done(&self) -> bool {
        match &self.prompt {
            None => true,
            Some(p) => self.revealed >= p.body.chars().count(),
        }
    }

    /// 把当前这句一次补满（玩家按得太急时的"跳过打字机"，不是"跳过这一句"）。
    fn finish_typing(&mut self) {
        if let Some(p) = &self.prompt {
            self.revealed = p.body.chars().count();
        }
    }
}

// ---------------------------------------------------------------- 黑幕（换幕）

/// 换幕的黑幕归游戏侧：换背景与摆位都必须发生在"全黑"那一拍。事件机那条 `Fade{Screen}`
/// 走 [`UiPort::set_dim`]，两者绘制时取较大值 —— 谁也不会把对方盖掉。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Wipe {
    None,
    Out { left: u32 },
    In { left: u32 },
}

impl Wipe {
    /// 本拍的黑幕浓度（0..=1）。淡出：`left` 从 `FADE` 数到 0 ⇒ 0→1；淡入反之。
    fn dim(self) -> f32 {
        let f = match self {
            Self::None => return 0.0,
            Self::Out { left } => 1.0 - left as f32 / FADE as f32,
            Self::In { left } => left as f32 / FADE as f32,
        };
        f.clamp(0.0, 1.0)
    }
}

// ---------------------------------------------------------------- 示例本体

struct VnDemo {
    assets: Assets,
    text: TextEngine,
    arena: SpriteArena,
    tasks: TaskScheduler,
    scene: Scene,
    vars: VarStore,
    bus: EngineBus,
    audio: SilentAudio,
    ui: MessageBox,
    vm: EventRunner,
    chapters: Vec<Chapter>,
    cast: Vec<(u32, SpriteRef)>,
    scene_id: usize,
    chapter: usize,
    wipe: Wipe,
    /// 黑幕浓度（游戏侧那一半）。
    dim: f32,
    /// 淡出到全黑之后要起的章号；0 = 没有。
    pending: usize,
    title_left: u32,
    /// 已经走到结局：等 `ok` 重开。
    ended: bool,
    /// 本拍发生的左键点击（虚拟像素），`tick` 里取走即消费。
    clicks: Vec<Vec2>,
    ticks: u64,
    stats: Option<DrawStats>,
    pushed_ui: bool,
    last: String,
    warnings: u32,
    font: String,
}

impl VnDemo {
    fn new() -> Self {
        let (scripts, errors) = compile_scripts();
        for e in errors {
            platform::log_info(&format!("[vn_demo] {e}"));
        }
        assert_eq!(
            scripts.len(),
            SCRIPTS.len(),
            "内联剧本编不过就是代码错，不该带着半套故事开跑"
        );
        let chapters = scripts
            .iter()
            .enumerate()
            .map(|(i, script)| Chapter {
                scene: chapter_scene(i + 1),
                script: Arc::new(script.clone()),
            })
            .collect();

        let mut arena = SpriteArena::new();
        let mut scene = Scene::free();
        let mut cast = Vec::new();
        for actor in [AOI, KENJI, CAT] {
            let r = arena.spawn(block_actor(1, actor));
            scene.set(actor, r);
            cast.push((actor, r));
        }

        let mut app = Self {
            assets: Assets::new(),
            text: TextEngine::new(FontBundle::system()),
            arena,
            tasks: TaskScheduler::new(),
            scene,
            vars: VarStore::new(),
            bus: EngineBus::new(),
            audio: SilentAudio,
            ui: MessageBox::default(),
            vm: EventRunner::new(),
            chapters,
            cast,
            scene_id: 0,
            chapter: 1,
            wipe: Wipe::In { left: FADE },
            dim: 1.0,
            pending: 0,
            title_left: TITLE_HOLD,
            ended: false,
            clicks: Vec::new(),
            ticks: 0,
            stats: None,
            pushed_ui: false,
            last: String::from("idle"),
            warnings: 0,
            font: String::new(),
        };
        app.place_chapter(1);
        app
    }

    /// 队列纪律第 1 条：没人等输入 ⇒ 这一拍的按键不往队列里堆。
    fn spend_keys(&mut self) {
        let _ = self.bus.drain();
    }

    fn report_warnings(&mut self) {
        let warns = self.vm.take_warnings();
        self.warnings += warns.len() as u32;
        for w in warns {
            platform::log_info(&format!("[vn_demo] 告警: {w}"));
        }
    }

    /// 摆好某一章的舞台：换背景、把三个角色挪到该在的位置（含"不在场"）、起剧本。
    ///
    /// 只在黑幕里调用：摆位不动画，`teleport` 因此永远不会被玩家看见。透明度归剧本
    /// 开头那几条 `Animate Fade` —— "谁在什么时候淡出来"是演出，不是舞台调度。
    fn place_chapter(&mut self, chapter: usize) {
        let chapter = chapter.clamp(1, self.chapters.len());
        // 先只取出需要的两样：`&self.chapters[..]` 跨过下面的 `&mut self` 会打不起来。
        let (scene, script) = (
            self.chapters[chapter - 1].scene,
            Arc::clone(&self.chapters[chapter - 1].script),
        );
        self.chapter = chapter;
        self.scene_id = scene;
        for (actor, sprite) in self.cast.iter().copied() {
            let st = staging_of(chapter, actor);
            self.tasks.cancel(sprite);
            if let Some(s) = self.arena.get_mut(sprite) {
                s.teleport(Vec2::new(st.x, st.y));
                s.opacity = 0.0;
            }
        }
        assert!(self.vm.start(0, script, 0), "第 {chapter} 章没有第 0 页");
        self.last = format!("ch{chapter} {}", self.chapters[chapter - 1].script.name);
    }

    /// 黑幕推进：淡出 → 全黑那一拍换幕 → 淡入。
    fn step_wipe(&mut self) {
        self.wipe = match self.wipe {
            Wipe::Out { left } => {
                let left = left.saturating_sub(1);
                if left > 0 {
                    Wipe::Out { left }
                } else {
                    let next = self.pending;
                    self.pending = 0;
                    if next > 0 {
                        self.place_chapter(next);
                    }
                    Wipe::In { left: FADE }
                }
            }
            Wipe::In { left } => {
                let left = left.saturating_sub(1);
                if left > 0 {
                    Wipe::In { left }
                } else {
                    Wipe::None
                }
            }
            Wipe::None => Wipe::None,
        };
        self.dim = self.wipe.dim();
    }

    fn request_chapter(&mut self, next: usize) {
        self.pending = next;
        self.wipe = Wipe::Out { left: FADE };
        self.dim = self.wipe.dim();
    }

    // ---------------------------------------------------------- 事件机之后

    /// 收信：剧本留下的"下一章"信箱。没留言 ⇒ 这条线走完了（由 `check_ending` 判定）。
    fn collect_mailbox(&mut self) {
        let next = self.vars.int(INT_NEXT);
        if next == 0 {
            return;
        }
        self.vars.set_int(INT_NEXT, 0);
        if !(1..=self.chapters.len() as i64).contains(&next) {
            platform::log_info(&format!("[vn_demo] 非法章号 {next}，忽略"));
            return;
        }
        self.request_chapter(next as usize);
    }

    /// 事件机空了、信箱也读过了、黑幕也走完了 ⇒ 这就是结局。
    fn check_ending(&mut self) {
        if self.ended || self.pending != 0 || self.wipe != Wipe::None {
            return;
        }
        if self.vm.busy() {
            return;
        }
        self.ended = true;
        platform::log_info(&format!("[vn_demo] 抵达结局：ch{}", self.chapter));
    }

    fn restart(&mut self) {
        self.vars = VarStore::new();
        self.ended = false;
        self.request_chapter(1);
    }

    // ---------------------------------------------------------- 输入

    /// 点消息窗：有选项 ⇒ 点哪行选哪行（往总线补"挪光标 + ok"这串命名动作，事件机在当拍
    /// `vm.tick` 里照常消费）；普通消息 ⇒ 点窗内任意处 = 按 `ok`。缺省键位表没给任何动作
    /// 绑 Pointer，所以这一次翻译是唯一的，不会和 `InputState` 撞车。
    fn click_prompt(&mut self, c: Vec2) {
        if !self.vm.waiting_input() {
            return;
        }
        let Some(p) = self.ui.prompt.clone() else {
            return;
        };
        if p.choices.is_empty() {
            if hit(WIN, c) {
                self.bus.push(EngineEvent::action("ok", ActionEdge::Press));
            }
            return;
        }
        // 选项只在句子铺满之后才画出来，也就只在铺满之后才可点。
        if !self.ui.typing_done() {
            return;
        }
        let rows = p
            .choices
            .iter()
            .enumerate()
            .filter(|(i, _)| choice_rows_fit(i + 1))
            .collect::<Vec<_>>();
        if let Some(&(i, _)) = rows.iter().find(|(k, _)| hit(choice_row(*k), c)) {
            let active = p.active.min(rows.len() - 1);
            for name in choice_click_seq(active, i) {
                self.bus.push(EngineEvent::action(name, ActionEdge::Press));
            }
        }
    }

    fn tick_typing(&mut self) {
        if self.ticks.is_multiple_of(REVEAL_EVERY) && !self.ui.typing_done() {
            self.ui.revealed += 1;
        }
    }

    // ---------------------------------------------------------- 字体与 HUD

    /// wasm：自带字体到货后换上（CJK 先、Inter 后，与 `rpg_demo` 同一注册顺序）。
    /// `CJK_PENDING` 时整批再等一拍 —— 等的是"这件事结束了"，不是"字体到了"。
    fn load_font(&mut self) {
        if self.assets.font_loaded || self.assets.cjk.state() == CJK_PENDING {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.font = String::from("system");
            self.assets.font_loaded = true;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let Some(inter) = self.assets.vfs.read(&vpath(FONT)) else {
                return;
            };
            let mut bundle = FontBundle::system();
            let mut named: Vec<String> = Vec::new();
            if self.assets.cjk.state() == CJK_ARRIVED
                // 只读一次：这份字节有 8MB，而 `Vfs::read` 是拷贝出来的。
                && let Some(bytes) = self.assets.vfs.read(&vpath(CJK))
            {
                match bundle.register_bytes(&bytes) {
                    Ok(family) => named.push(family),
                    Err(why) => {
                        self.assets.cjk.set(CJK_NONE);
                        platform::log_info(&format!("[vn_demo] CJK 用不了: {why}"));
                    }
                }
            }
            match bundle.register_bytes(&inter) {
                Ok(family) => named.push(family),
                Err(why) => platform::log_info(&format!("[vn_demo] 自带字体用不了: {why}")),
            }
            self.font = named.join("+");
            platform::log_info(&format!(
                "[vn_demo] 字体 {}（{}，face {}）",
                self.font,
                self.assets.cjk.label(),
                bundle.face_count()
            ));
            *self.text.bundle_mut() = bundle;
            self.assets.font_loaded = true;
        }
    }

    /// HUD 两行恒 ASCII：它最需要出现的那一刻，恰好是 CDN 那份中文到不了的那一刻。
    fn hud_lines(&self) -> [String; 2] {
        let sw = |id: u32| self.vars.is_on(id) as u8;
        [
            format!(
                "ch{} {} · {} · font {} {}",
                self.chapter,
                SCENES[self.scene_id],
                self.last,
                self.assets.cjk.label(),
                self.font,
            ),
            format!(
                "sw {}/{}/{}/{} · wipe {:?} · reveal {} · warnings {}",
                sw(SW_INVITED),
                sw(SW_CAME_BACK),
                sw(SW_FED_CAT),
                sw(SW_PROMISED),
                self.wipe,
                self.ui.revealed,
                self.warnings,
            ),
        ]
    }

    // ---------------------------------------------------------- 画面

    /// 三张背景：全走同一张白图的纯色批，`z` 从 0 往上叠。
    fn draw_scene(&mut self, renderer: &mut Renderer) {
        let (vw, vh) = (VIRTUAL.0 as f32, VIRTUAL.1 as f32);
        match self.scene_id {
            // 教室：暖墙 + 黑板 + 三扇夕阳窗 + 两张课桌剪影。
            0 => {
                quad(
                    renderer,
                    WORLD.0,
                    0,
                    [0.0, 0.0, vw, vh],
                    [0.46, 0.35, 0.28, 1.0],
                );
                quad(
                    renderer,
                    WORLD.0,
                    1,
                    [0.0, 300.0, vw, vh - 300.0],
                    [0.30, 0.22, 0.18, 1.0],
                );
                quad(
                    renderer,
                    WORLD.0,
                    2,
                    [24.0, 84.0, 236.0, 112.0],
                    [0.10, 0.22, 0.16, 1.0],
                );
                for (i, w) in [58.0, 42.0, 66.0].iter().enumerate() {
                    quad(
                        renderer,
                        WORLD.0,
                        3,
                        [40.0, 100.0 + i as f32 * 26.0, *w, 7.0],
                        [0.86, 0.88, 0.80, 0.5],
                    );
                }
                for i in 0..3 {
                    let x = 296.0 + i as f32 * 76.0;
                    quad(renderer, WORLD.0, 3, [x, 56.0, 60.0, 194.0], SUNSET);
                    quad(
                        renderer,
                        WORLD.0,
                        4,
                        [x + 27.0, 56.0, 6.0, 194.0],
                        [0.30, 0.22, 0.18, 1.0],
                    );
                    quad(
                        renderer,
                        WORLD.0,
                        4,
                        [x, 150.0, 60.0, 6.0],
                        [0.30, 0.22, 0.18, 1.0],
                    );
                }
                for x in [64.0, 158.0] {
                    quad(
                        renderer,
                        WORLD.0,
                        5,
                        [x, 252.0, 78.0, 12.0],
                        [0.62, 0.44, 0.28, 1.0],
                    );
                    quad(
                        renderer,
                        WORLD.0,
                        5,
                        [x + 8.0, 264.0, 8.0, 60.0],
                        [0.42, 0.30, 0.22, 1.0],
                    );
                    quad(
                        renderer,
                        WORLD.0,
                        5,
                        [x + 62.0, 264.0, 8.0, 60.0],
                        [0.42, 0.30, 0.22, 1.0],
                    );
                }
            }
            // 走廊：一整排窗把地板切成明暗相间的一段。
            1 => {
                quad(
                    renderer,
                    WORLD.0,
                    0,
                    [0.0, 0.0, vw, 292.0],
                    [0.52, 0.48, 0.44, 1.0],
                );
                quad(
                    renderer,
                    WORLD.0,
                    1,
                    [0.0, 292.0, vw, vh - 292.0],
                    [0.34, 0.32, 0.38, 1.0],
                );
                for i in 0..4 {
                    let x = 18.0 + i as f32 * 136.0;
                    quad(renderer, WORLD.0, 2, [x, 54.0, 104.0, 152.0], SUNSET);
                    quad(
                        renderer,
                        WORLD.0,
                        3,
                        [x + 48.0, 54.0, 8.0, 152.0],
                        [0.30, 0.30, 0.34, 1.0],
                    );
                    quad(
                        renderer,
                        WORLD.0,
                        3,
                        [x + 10.0, 238.0, 98.0, 150.0],
                        [1.0, 0.80, 0.52, 0.14],
                    );
                }
                quad(
                    renderer,
                    WORLD.0,
                    4,
                    [0.0, 212.0, vw, 8.0],
                    [0.24, 0.24, 0.30, 0.7],
                );
                quad(
                    renderer,
                    WORLD.0,
                    4,
                    [432.0, 116.0, 84.0, 176.0],
                    [0.22, 0.20, 0.26, 0.9],
                );
            }
            // 天台：天空三段 + 远处楼群 + 栏杆 + 水泥地。
            _ => {
                quad(
                    renderer,
                    WORLD.0,
                    0,
                    [0.0, 0.0, vw, 132.0],
                    [0.20, 0.26, 0.46, 1.0],
                );
                quad(
                    renderer,
                    WORLD.0,
                    1,
                    [0.0, 132.0, vw, 58.0],
                    [0.62, 0.38, 0.36, 1.0],
                );
                quad(
                    renderer,
                    WORLD.0,
                    1,
                    [0.0, 190.0, vw, 26.0],
                    [0.95, 0.62, 0.36, 1.0],
                );
                for (x, w, h) in [
                    (12.0, 46.0, 74.0),
                    (64.0, 34.0, 104.0),
                    (104.0, 58.0, 62.0),
                    (170.0, 40.0, 90.0),
                    (218.0, 66.0, 56.0),
                    (300.0, 44.0, 84.0),
                    (350.0, 58.0, 66.0),
                    (418.0, 36.0, 98.0),
                    (462.0, 66.0, 60.0),
                ] {
                    quad(
                        renderer,
                        WORLD.0,
                        2,
                        [x, 216.0 - h, w, h],
                        [0.16, 0.14, 0.22, 1.0],
                    );
                }
                quad(
                    renderer,
                    WORLD.0,
                    3,
                    [0.0, 212.0, vw, 6.0],
                    [0.72, 0.70, 0.66, 1.0],
                );
                for i in 0..12 {
                    quad(
                        renderer,
                        WORLD.0,
                        3,
                        [10.0 + i as f32 * 46.0, 212.0, 5.0, 70.0],
                        [0.72, 0.70, 0.66, 1.0],
                    );
                }
                quad(
                    renderer,
                    WORLD.0,
                    4,
                    [0.0, 282.0, vw, vh - 282.0],
                    [0.44, 0.44, 0.48, 1.0],
                );
                quad(
                    renderer,
                    WORLD.0,
                    5,
                    [0.0, 282.0, vw, 4.0],
                    [0.30, 0.30, 0.34, 1.0],
                );
                quad(
                    renderer,
                    WORLD.0,
                    5,
                    [404.0, 150.0, 62.0, 132.0],
                    [0.26, 0.26, 0.32, 1.0],
                );
            }
        }
    }

    fn draw_prompt(&mut self, renderer: &mut Renderer) {
        let Some(p) = self.ui.prompt.clone() else {
            return;
        };
        quad(renderer, UI_LAYER, 1, WIN, WINDOW_BG);
        let style = TextStyle::new(13.0).aligned(Align::Left);
        let shown: String = p.body.chars().take(self.ui.revealed).collect();
        let done = self.ui.typing_done();
        if let Some(speaker) = p.speaker.as_deref() {
            let w = (28.0 + speaker.chars().count() as f32 * 15.0).min(WIN[2] - 32.0);
            quad(
                renderer,
                UI_LAYER,
                2,
                [WIN[0] + 16.0, NAME[1] - 5.0, w, 26.0],
                [0.12, 0.16, 0.30, 0.95],
            );
            self.text
                .draw_text_at(renderer, NAME, speaker, &style, NAME_INK, UI_LAYER);
        }
        self.text
            .draw_text_at(renderer, BODY, &shown, &style, INK, UI_LAYER);
        if !done {
            return;
        }
        let rows = p
            .choices
            .iter()
            .enumerate()
            .filter(|(i, _)| choice_rows_fit(i + 1))
            .collect::<Vec<_>>();
        let active = if rows.is_empty() {
            0
        } else {
            p.active.min(rows.len() - 1)
        };
        if let Some(&(i, _)) = rows.iter().find(|(k, _)| *k == active) {
            quad(renderer, UI_LAYER, 2, choice_row(i), HIGHLIGHT);
        }
        for (i, choice) in rows {
            let mut r = choice_row(i);
            r[0] += 12.0;
            r[2] -= 12.0;
            let ink = if i == active { CURSOR_INK } else { INK };
            self.text
                .draw_text_at(renderer, r, choice, &style, ink, UI_LAYER);
        }
    }

    /// 标题卡与结局卡都画在黑幕之上（它们本来就是"幕布上的字"）。
    fn draw_card(&mut self, renderer: &mut Renderer, top: &str, mid: &str, bottom: &str) {
        let (vw, vh) = (VIRTUAL.0 as f32, VIRTUAL.1 as f32);
        quad(
            renderer,
            UI_LAYER,
            10,
            [0.0, 0.0, vw, vh],
            [0.02, 0.03, 0.06, 0.55],
        );
        self.text.draw_text_at(
            renderer,
            [40.0, 148.0, vw - 80.0, 56.0],
            top,
            &TextStyle::new(44.0).aligned(Align::Center),
            INK,
            UI_LAYER,
        );
        self.text.draw_text_at(
            renderer,
            [40.0, 214.0, vw - 80.0, 22.0],
            mid,
            &TextStyle::new(13.0).aligned(Align::Center),
            INK_DIM,
            UI_LAYER,
        );
        self.text.draw_text_at(
            renderer,
            [40.0, 250.0, vw - 80.0, 20.0],
            bottom,
            &TextStyle::new(12.0).aligned(Align::Center),
            [1.0, 1.0, 1.0, 0.5],
            UI_LAYER,
        );
    }
}

/// 一块纯色角色精灵（无位图时白图加负色分量 = 想要的颜色，与 `rpg_demo` 同一手法）。
///
/// `chapter` 只用来取初始尺寸；站位与透明度每次换幕都由 `place_chapter` 重写。
fn block_actor(chapter: usize, actor: u32) -> Sprite {
    let st = staging_of(chapter, actor);
    let mut s = Sprite::solid(Vec2::new(st.w, st.h));
    s.anchor = Vec2::new(0.5, 1.0);
    s.teleport(Vec2::new(st.x, st.y));
    s.z = actor as i32 * 4;
    s.layer = WORLD;
    s.opacity = 0.0;
    s.tone = Tone::offset(
        [st.color[0] - 1.0, st.color[1] - 1.0, st.color[2] - 1.0],
        0.0,
    );
    s
}

fn describe(status: RunnerStatus) -> String {
    match status {
        RunnerStatus::Idle { executed } => format!("idle {executed}"),
        RunnerStatus::Suspended { executed, reason } => format!("hold {reason} {executed}"),
        RunnerStatus::OverBudget { executed } => format!("over-budget {executed}"),
    }
}

impl App for VnDemo {
    /// 只做两件事：把切片原样转进自己的总线（事件机是它唯一的读者），顺手记下左键点击的
    /// 位置 —— 指针的 `pos` 已是虚拟像素，与 UI 矩形直接比。这里连一个裸键名都没有。
    fn on_event(&mut self, events: &[EngineEvent], _ctx: &mut FrameCtx) {
        for e in events {
            if let EngineEvent::Pointer {
                kind: PointerKind::Press,
                button: PointerButton::Left,
                pos,
                ..
            } = e
            {
                self.clicks.push(*pos);
            }
            self.bus.push(e.clone());
        }
    }

    fn tick(&mut self, _dt: Duration, ctx: &mut FrameCtx) {
        self.ticks += 1;
        // 每拍第一行收拢 `prev_pos`（T8 第五条"抖"），且必须在任何移动之前。
        self.arena.begin_tick();
        self.text.set_raster_scale(ctx.raster_scale());
        self.load_font();
        self.step_wipe();

        // 命名动作只有这一个入口（`take_press` 读一次即消费），这里连一个裸键名都没有。
        let ok = ctx.input.take_press("ok");
        let click = self.clicks.pop();
        self.clicks.clear();

        if !self.vm.waiting_input() {
            self.spend_keys();
        }

        // 标题卡不进事件机：它是"幕布"，不是"演出"。
        if self.title_left > 0 {
            self.title_left -= 1;
            if ok || click.is_some() || self.title_left == 0 {
                self.title_left = 0;
            }
            return;
        }

        if self.ended {
            if ok || click.is_some() {
                self.restart();
            }
            return;
        }

        // 玩家那一下"推进"到哪一层为止，取决于打字机：
        // - 句子还没铺满 ⇒ 只是把这句补满，总线里平台那份 `ok` 一并抽掉（否则事件机
        //   会在同一拍把它读成第二次按下：一句话两拍跳过）。
        // - 已经铺满且事件机在等人 ⇒ 什么都不做，那份 `ok` 就在总线里，事件机自己吃；
        //   点击才需要游戏侧翻译成命名动作（`click_prompt`）。
        // - 没人在等 ⇒ 上面的 `spend_keys` 已经把它丢了。
        if !self.ui.typing_done() {
            if ok || click.is_some_and(|c| hit(WIN, c)) {
                self.ui.finish_typing();
            }
            self.spend_keys();
        } else if let Some(c) = click {
            self.click_prompt(c);
        }

        // 调度器由事件机推进（队列纪律第 3 条：这里一次都不调 `tasks.tick`）。
        let status = {
            let VnDemo {
                arena,
                tasks,
                scene,
                audio,
                ui,
                vars,
                bus,
                assets,
                vm,
                ..
            } = self;
            let mut evctx = EventCtx {
                sprites: arena,
                tasks,
                scene,
                audio,
                ui,
                vars,
                bus,
                vfs: &assets.vfs,
            };
            vm.tick(&mut evctx, BUDGET)
        };
        self.last = describe(status);
        self.report_warnings();
        self.tick_typing();
        self.collect_mailbox();
        self.check_ending();
    }

    fn prepare(&mut self, _alpha: f32, ctx: &mut FrameCtx) {
        if self.pushed_ui {
            ctx.pop_viewport();
        }
        let (vw, vh) = (VIRTUAL.0 as f32, VIRTUAL.1 as f32);
        // UI 层 = 与原画同尺寸、零平移的 viewport：投影恒等，层号比世界大一档。
        let layer = ctx.push_viewport(Viewport::at(0.0, 0.0, vw, vh));
        debug_assert_eq!(layer, UI_LAYER, "UI 层号与 `UI_LAYER` 的约定不符");
        self.pushed_ui = true;
    }

    fn draw(&mut self, alpha: f32, renderer: &mut Renderer) {
        let n = self.arena.len().max(1) as u32;
        if renderer.quads_per_slot() != n {
            renderer.set_quads_per_slot(n);
        }
        let (vw, vh) = (VIRTUAL.0 as f32, VIRTUAL.1 as f32);

        self.draw_scene(renderer);
        let mut list = BatchList::new();
        if self.arena.collect_at(WORLD_VIEW, alpha, &mut list).is_err() {
            platform::log_info("[vn_demo] 采集失败（本示例只该有纯色块）");
            return;
        }
        renderer.append_list(list);

        // 黑幕取两路之大：游戏侧换幕 + 事件机 `Fade{Screen}`（本示例没用后者）。
        let dim = self.dim.max(self.ui.dim);
        if dim > 0.0 {
            quad(
                renderer,
                UI_LAYER,
                9,
                [0.0, 0.0, vw, vh],
                [0.0, 0.0, 0.0, dim],
            );
        }

        if self.assets.font_loaded {
            if self.title_left > 0 {
                self.draw_card(
                    renderer,
                    "屋上",
                    "一个长在 SISF 上的视觉小说 demo",
                    "press Enter",
                );
            } else if self.ended {
                let warm = self.chapter == 4;
                let (top, mid) = if warm {
                    ("毕业那天，天台见。", "the end · press Enter")
                } else {
                    ("两块黑板都擦完了。", "the end · press Enter")
                };
                self.draw_card(renderer, top, mid, "");
            } else if dim < 0.35 {
                // 淡入过半才把对话窗放出来：换幕那一拍推上来的句子，此刻还是黑的。
                self.draw_prompt(renderer);
            }
            for (i, line) in self.hud_lines().iter().enumerate() {
                self.text.draw_text_at(
                    renderer,
                    [8.0, 6.0 + i as f32 * 14.0, vw - 16.0, 14.0],
                    line,
                    &TextStyle::new(10.0).aligned(Align::Left),
                    [1.0, 1.0, 1.0, 0.42],
                    UI_LAYER,
                );
            }
            if self.chapter > 0 && !self.ended && self.title_left == 0 {
                let tag = format!("{} · {}", self.chapter, CHAPTER_TITLES[self.chapter - 1]);
                self.text.draw_text_at(
                    renderer,
                    [vw - 220.0, 6.0, 212.0, 16.0],
                    &tag,
                    &TextStyle::new(11.0).aligned(Align::Right),
                    [1.0, 1.0, 1.0, 0.35],
                    UI_LAYER,
                );
            }
        }
        self.stats = renderer.stats();
        self.text.sync(renderer);
    }
}

// ---------------------------------------------------------------- 画面几何

/// 消息窗几何与 `rpg_demo` 同一份数学，只是给三个选项留了行。
const WIN: [f32; 4] = [16.0, 248.0, 512.0, 156.0];
const NAME: [f32; 4] = [36.0, 253.0, 300.0, 18.0];
const BODY: [f32; 4] = [36.0, 278.0, 472.0, 42.0];
const CHOICE_X: f32 = 40.0;
const CHOICE_Y: f32 = 326.0;
const CHOICE_H: f32 = 20.0;
const CHOICE_LEAD: f32 = 5.0;
const WINDOW_BG: [f32; 4] = [0.05, 0.07, 0.16, 0.86];
const HIGHLIGHT: [f32; 4] = [0.94, 0.86, 0.42, 0.9];
const INK: [f32; 4] = [0.95, 0.96, 1.0, 0.94];
const INK_DIM: [f32; 4] = [0.72, 0.76, 0.86, 0.9];
const NAME_INK: [f32; 4] = [1.0, 0.90, 0.62, 0.96];
const CURSOR_INK: [f32; 4] = [0.10, 0.10, 0.14, 1.0];
const SUNSET: [f32; 4] = [1.0, 0.72, 0.42, 0.85];

fn choice_row(i: usize) -> [f32; 4] {
    [
        CHOICE_X,
        CHOICE_Y + i as f32 * (CHOICE_H + CHOICE_LEAD),
        WIN[2] - 2.0 * (CHOICE_X - WIN[0]),
        CHOICE_H,
    ]
}

fn choice_rows_fit(n: usize) -> bool {
    n == 0 || choice_row(n - 1)[1] + CHOICE_H <= WIN[1] + WIN[3] - 8.0
}

/// 虚拟像素命中测试：矩形是 `[x, y_top, w, h]`（y 向下），点在闭左开右区间内算命中。
fn hit(rect: [f32; 4], p: Vec2) -> bool {
    p.x >= rect[0] && p.x < rect[0] + rect[2] && p.y >= rect[1] && p.y < rect[1] + rect[3]
}

/// 点选第 `target` 行选项要往总线里补的命名动作序列：先挪光标，再按一次 `ok`。
/// 事件机只认 `up`/`down`/`ok`，所以"鼠标"到它这里就只是"另一只手在敲命名动作"。
fn choice_click_seq(active: usize, target: usize) -> Vec<&'static str> {
    let mut seq: Vec<&'static str> = if target > active {
        vec!["down"; target - active]
    } else {
        vec!["up"; active - target]
    };
    seq.push("ok");
    seq
}

/// 一块纯色矩形（同一 `WHITE_EPOCH`，靠 `z` 分先后；文字自带 epoch 恒在其上）。
fn quad(renderer: &mut Renderer, layer: u8, z: i32, rect: [f32; 4], rgba: [f32; 4]) {
    renderer.push(
        BatchKey {
            layer,
            clip: NO_CLIP,
            epoch: WHITE_EPOCH,
            blend: Blend::Normal,
            z,
            insertion: 0,
        },
        Instance::rect(
            [rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0],
            [rect[2], rect[3]],
            rgba,
            layer,
        ),
    );
}

fn config() -> AppConfig {
    AppConfig {
        title: "SISF vn_demo · 屋上（视觉小说）".to_owned(),
        virtual_size: VIRTUAL,
        scale_mode: ScaleMode::Fit,
        tick_hz: TICK_HZ as f64,
        clear: [0.05, 0.06, 0.10, 1.0],
        ..Default::default()
    }
}

fn real_main() {
    platform::log_info("[vn_demo] main 已启动");
    if let Err(e) = platform::run(VnDemo::new(), config()) {
        panic!("[vn_demo] 运行失败: {e}");
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn sisf_start() {
    real_main();
}

fn main() {
    real_main();
}

// ---------------------------------------------------------------------- 单测
//
// 能自动化的部分：五份内联剧本编得过、剧情图连通且两个结局都真能走到、每个被 `If` 读的
// 开关都有人写过（否则那个分支永远走不到）、选项行不出窗、剧本里的落点与舞台表对账、
// 黑幕与打字机的拍数数学。跑法 `cargo test -p sisf --example vn_demo`。
// 画面、节奏与剧情推进走文件头的手测清单。
#[cfg(test)]
mod tests {
    use super::*;
    use sisf::event::{Command, Cond, Expr, Instr, RouteSpec, RouteStep, SetOp, Slot};

    fn page_cmds(script: &EventScript, page: usize) -> Vec<Command> {
        script.pages[page]
            .code
            .iter()
            .filter_map(|i| match i {
                Instr::Run(c) => Some(c.clone()),
                _ => None,
            })
            .collect()
    }

    fn walk(cmds: &[Command], out: &mut Vec<Command>) {
        for c in cmds {
            out.push(c.clone());
            match c {
                Command::If { then, else_, .. } => {
                    walk(then, out);
                    walk(else_, out);
                }
                Command::Loop(body) => walk(body, out),
                _ => {}
            }
        }
    }

    fn flat(chapter: usize) -> Vec<Command> {
        let (scripts, _) = compile_scripts();
        let mut out = Vec::new();
        walk(&page_cmds(&scripts[chapter - 1], 0), &mut out);
        out
    }

    /// 一章里所有 `SetVar(Int(20), Set, Lit(n))` ⇒ 它可能送玩家去的所有下一章。
    fn routes_to(cmds: &[Command]) -> Vec<usize> {
        cmds.iter()
            .filter_map(|c| match c {
                Command::SetVar {
                    slot: Slot::Int(INT_NEXT),
                    op: SetOp::Set,
                    value: Expr::Lit(n),
                } => Some(*n as usize),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_five_inline_scripts_compile_in_order() {
        let (scripts, errors) = compile_scripts();
        assert!(errors.is_empty(), "内联剧本编不过: {errors:?}");
        assert_eq!(scripts.len(), SCRIPTS.len());
        assert_eq!(
            scripts.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            [
                "classroom",
                "rooftop",
                "hallway",
                "rooftop_promise",
                "blackboard"
            ],
            "`chapter_scene`/`BLOCKINGS` 按这个顺序写死"
        );
        for s in &scripts {
            assert_eq!(s.pages.len(), 1, "每章只有一页，翻页这件事这一版没用到");
        }
    }

    #[test]
    fn the_story_graph_forks_twice_and_both_ending_are_reachable() {
        let cmds: Vec<Vec<Command>> = (1..=SCRIPTS.len()).map(flat).collect();
        let routes: Vec<Vec<usize>> = cmds.iter().map(|c| routes_to(c)).collect();
        assert_eq!(routes[0], vec![2, 3], "教室该分出去两条路");
        assert_eq!(routes[1], vec![4], "天台该收在约定");
        assert_eq!(routes[2], vec![2, 5], "走廊要么回天台，要么去黑板");
        assert!(routes[3].is_empty() && routes[4].is_empty(), "结局不留出口");

        let mut seen = vec![false; SCRIPTS.len() + 1];
        let mut queue = vec![1usize];
        seen[1] = true;
        while let Some(c) = queue.pop() {
            for &to in &routes[c - 1] {
                assert!(
                    (1..=SCRIPTS.len()).contains(&to),
                    "第 {c} 章送玩家去不存在的 {to}"
                );
                if !seen[to] {
                    seen[to] = true;
                    queue.push(to);
                }
            }
        }
        // `seen[0]` 是"章号从 1 起"留的空位。
        assert!(seen[1..].iter().all(|&s| s), "有章节从开场到不了: {seen:?}");
        assert!(seen[4] && seen[5], "两个结局都得真的走得通");
    }

    #[test]
    fn every_choice_writes_a_slot_and_every_read_switch_is_written() {
        let cmds: Vec<Vec<Command>> = (1..=SCRIPTS.len()).map(flat).collect();
        let all: Vec<Command> = cmds.concat();
        let mut set = Vec::new();
        let mut read = Vec::new();
        for c in &all {
            match c {
                Command::SetVar {
                    slot: Slot::Switch(id),
                    ..
                } => set.push(*id),
                Command::If { cond, .. } => read_switches(cond, &mut read),
                Command::ShowText {
                    choices, result, ..
                } => {
                    if !choices.is_empty() {
                        assert!(result.is_some(), "有选项却没有 result 槽 ⇒ 选完没人知道");
                    } else {
                        assert!(result.is_none(), "没选项却写 result 是死代码");
                    }
                }
                _ => {}
            }
        }
        for id in &read {
            assert!(
                set.contains(id),
                "读开关 {id}，却没有任何剧本写过它（分支永远走不到）"
            );
        }
        let written: std::collections::BTreeSet<u32> = set.into_iter().collect();
        assert_eq!(
            written,
            [SW_INVITED, SW_CAME_BACK, SW_FED_CAT, SW_PROMISED]
                .into_iter()
                .collect::<std::collections::BTreeSet<u32>>(),
            "写过的开关集合对不上（多写或漏写）"
        );
    }

    /// 选项结果槽是"剧本 ↔ 章节表"之间唯一的暗号：常量与实际写死的编号必须一致，
    /// 且每个编号只出现一次（同一章里两个选项共用一个槽 ⇒ 后一个把前一个覆盖掉）。
    #[test]
    fn the_choice_slots_are_the_documented_ones() {
        let all: Vec<Command> = (1..=SCRIPTS.len()).map(flat).collect::<Vec<_>>().concat();
        let slots: Vec<u32> = all
            .iter()
            .filter_map(|c| match c {
                Command::ShowText {
                    choices,
                    result: Some(Slot::Int(i)),
                    ..
                } if !choices.is_empty() => Some(*i),
                _ => None,
            })
            .collect();
        let mut want = [CHOICE_ROOFTOP, CHOICE_HALL, CHOICE_CAT, CHOICE_PROMISE];
        want.sort_unstable();
        let mut got = slots.clone();
        got.sort_unstable();
        assert_eq!(got, want, "选项结果槽与常量表对不上: {slots:?}");
    }

    fn read_switches(cond: &Cond, out: &mut Vec<u32>) {
        match cond {
            Cond::Switch { id, .. } => out.push(*id),
            Cond::Not(b) => read_switches(b, out),
            Cond::All(v) | Cond::Any(v) => {
                for c in v {
                    read_switches(c, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn choices_fit_the_window_and_lines_fit_the_box() {
        let all: Vec<Command> = (1..=SCRIPTS.len()).map(flat).collect::<Vec<_>>().concat();
        let mut lines = 0;
        for c in &all {
            let Command::ShowText {
                choices,
                body,
                speaker,
                ..
            } = c
            else {
                continue;
            };
            lines += 1;
            assert!(
                body.chars().count() <= 2 * 34,
                "正文超过两行会被裁掉（{} 字）: {body}",
                body.chars().count()
            );
            assert!(
                speaker.as_deref().is_none_or(|s| s.chars().count() <= 6),
                "说话人太宽会顶出名牌: {speaker:?}"
            );
            assert!(
                choices.len() <= 3 && choice_rows_fit(choices.len()),
                "{choices:?} 的选项出窗"
            );
            assert!(
                choices
                    .iter()
                    .all(|c| !c.is_empty() && c.chars().count() <= 12)
            );
        }
        assert!(lines >= 20, "五章至少该有二十句台词，现在是 {lines}");
    }

    #[test]
    fn the_scripts_land_on_the_blocking_table() {
        // 剧本写死的落点必须与 `BLOCKINGS` 的开场站位对得上，否则角色会"淡出来却站在别处"。
        for chapter in 1..=SCRIPTS.len() {
            for c in flat(chapter) {
                let Command::MoveRoute {
                    actor,
                    route: RouteSpec { steps, .. },
                    ..
                } = c
                else {
                    continue;
                };
                let st = staging_of(chapter, actor);
                assert!(!st.x.is_nan());
                for step in &steps {
                    let RouteStep::MoveTo { x, y } = *step else {
                        continue;
                    };
                    assert_eq!(
                        y,
                        staging_of(chapter, actor).y,
                        "第 {chapter} 章 actor {actor} 的落点不在它的脚底线上"
                    );
                    assert!(
                        (0.0..=VIRTUAL.0 as f32).contains(&x),
                        "第 {chapter} 章把角色 {actor} 送出了画布"
                    );
                    // 落点必须落在它自己的宽度以内（同一章不许两个人叠在一条地上）。
                    assert!(
                        (st.x - x).abs() < st.w * 2.0 || st.x == OFF,
                        "第 {chapter} 章 actor {actor} 开场在 {}，剧本却把它挪到 {x}",
                        st.x
                    );
                }
            }
        }
    }

    #[test]
    fn blocking_keeps_absent_actors_out_of_sight_and_present_ones_apart() {
        for chapter in 1..=BLOCKINGS.len() {
            let mut on = Vec::new();
            for actor in [AOI, KENJI, CAT] {
                let st = staging_of(chapter, actor);
                if st.x == OFF {
                    // 离场必须是"整只走出画面"：连最宽的角色也算上。
                    assert!(st.x + st.w / 2.0 < 0.0, "离场角色 {actor} 还漏在画面里");
                } else {
                    assert!(
                        st.x > st.w / 2.0 && st.x < VIRTUAL.0 as f32 - st.w / 2.0,
                        "第 {chapter} 章 actor {actor} 站出了画布"
                    );
                    // 在场 = 观众真看得见：头必须探出消息窗上缘。猫曾经整只站在窗后
                    // （脚底 316、高 44 ⇒ 272..316 全在 `WIN[1] = 248` 以下），于是"它蹲在
                    // 栏杆上"演的是看不见的东西 —— 这条判据就是那次留下的。
                    assert!(
                        st.y - st.h < WIN[1],
                        "第 {chapter} 章 actor {actor} 整个儿躲在消息窗后面"
                    );
                    if st.y < WIN[1] {
                        // 站在窗上缘之上的（架子上的矮个子）不该有一只脚垂进窗里。
                        assert!(st.y - st.h >= 0.0, "第 {chapter} 章 actor {actor} 顶出画布");
                    }
                    on.push((actor, st.x));
                }
            }
            for (i, (a, ax)) in on.iter().enumerate() {
                for (b, bx) in &on[i + 1..] {
                    assert!(
                        (ax - bx).abs() > 90.0,
                        "第 {chapter} 章 {a} 与 {b} 叠在同一个位置"
                    );
                }
            }
        }
    }

    #[test]
    fn chapter_scenes_are_the_three_rooms() {
        assert_eq!(chapter_scene(1), 0);
        assert_eq!(chapter_scene(3), 1);
        assert_eq!(chapter_scene(2), 2);
        assert_eq!(chapter_scene(4), 2);
        assert_eq!(chapter_scene(5), 0);
        assert_eq!(SCENES.len(), 3);
        assert_eq!(chapter_scene(99), 2, "越界章号该安静地落在最后一个场景");
    }

    #[test]
    fn the_wipe_is_symmetric_and_lands_exactly_on_black() {
        // 淡出：`left` 从 `FADE` 数到 0（`request_chapter` 那一拍是 `left == FADE`，浓度 0，
        // 也就是"这一拍还完全亮着"），此后每一步严格变暗，且只有最后一拍满黑。
        let mut prev = -1.0;
        let mut blacks = 0;
        for i in 0..=FADE {
            let d = Wipe::Out { left: FADE - i }.dim();
            assert!(d > prev, "淡出第 {i} 拍没有变暗");
            blacks += usize::from(d >= 1.0);
            prev = d;
        }
        assert_eq!(prev, 1.0, "走完 {FADE} 拍该落在全黑");
        assert_eq!(
            blacks, 1,
            "只有最后一拍满黑 —— 换幕就在那一拍，不能提前跳背景"
        );

        // 淡入：从满黑单调变亮，最后一步正好回到 0（`step_wipe` 在那里换成 `None`）。
        let mut prev = 2.0;
        for i in 0..=FADE {
            let d = Wipe::In { left: FADE - i }.dim();
            assert!(d < prev, "淡入第 {i} 拍没有变亮");
            prev = d;
        }
        assert_eq!(prev, 0.0, "淡入收尾该正好是全亮");
        assert_eq!(Wipe::None.dim(), 0.0);
    }

    #[test]
    fn the_typewriter_reveals_one_line_at_a_time_and_resets_on_a_new_one() {
        let line = "风把这句话按在夕阳上";
        let total = line.chars().count();
        let mut ui = MessageBox::default();
        ui.show(&prompt(line, &[]));
        assert_eq!(ui.revealed, 0, "新句子必须从头开始");
        for _ in 0..total {
            ui.revealed += 1;
        }
        assert!(ui.typing_done());
        // 移动光标会再推一次 `show`：正文没变 ⇒ 不该把已经显示的字吞回去。
        ui.show(&prompt(line, &["甲".to_owned(), "乙".to_owned()]));
        assert_eq!(ui.revealed, total, "同一句的重推不该重置打字机");
        // 但换了一句就得从头再来，哪怕长度一样。
        ui.show(&prompt("今天的黑板很干净。", &[]));
        assert_eq!(ui.revealed, 0);
        ui.finish_typing();
        assert!(ui.typing_done(), "补满之后就该可以吃点击");
        ui.hide();
        assert_eq!(ui.revealed, 0);
        assert!(ui.typing_done(), "没有窗的时候不算「还在打字」");
    }

    fn prompt(body: &str, choices: &[String]) -> Prompt {
        Prompt {
            speaker: None,
            body: body.to_owned(),
            choices: choices.to_vec(),
            active: 0,
            allow_cancel: false,
        }
    }

    #[test]
    fn hit_testing_and_click_translation_stay_on_one_page() {
        let r = [10.0, 20.0, 30.0, 40.0];
        assert!(hit(r, Vec2::new(10.0, 20.0)));
        assert!(!hit(r, Vec2::new(9.9, 30.0)), "左缘外不该算命中");
        assert!(!hit(r, Vec2::new(40.0, 60.0)), "右缘是开区间");
        assert_eq!(choice_click_seq(0, 2), vec!["down", "down", "ok"]);
        assert_eq!(choice_click_seq(2, 0), vec!["up", "up", "ok"]);
        assert_eq!(choice_click_seq(1, 1), vec!["ok"]);
        assert!(choice_rows_fit(3), "三个选项必须摆得进消息窗");
        assert!(!choice_rows_fit(4), "四个就不该塞进来（这条边界要盯住）");
    }
}
