//! 简易 RPG demo：地图 + 人物 + 对话 + 菜单 + 回合战斗，全部长在 SISF 上。
//!
//! 与 `dialog_demo` 的分工一样，这里验的是**组合**：事件虚拟机负责地图侧的一切演出
//! （走格子、对话、选项、变量），而菜单与回合战斗是纯游戏侧的有限状态机 —— 引擎没有
//! "战斗系统"这一层，也不该有。两条路共享同一份输入词汇表（`ActionMap` 的命名动作）
//! 与同一块画布（世界层 / UI 层），谁在吃什么按键在 `tick` 开头一次定路由。
//!
//! ```text
//! on_event ──原样──► EngineBus ──唯一读者──► EventRunner（走路 / 对话 / 选项）
//!    │                    ▲
//!    │                    └─左键点选项── 游戏侧翻译成"挪光标 + ok"的命名动作
//!    └─ ctx.input（引擎每拍喂好的 InputState）──► tick 开头按 mode 路由：
//!         Map    方向按住 = 走一格；ok = 查面前（村长 / 药师 / 洞窟）；M = 开菜单
//!         Menu   上下选、ok 执行、cancel 关（状态面板 + 用药水）
//!         Battle 上下选指令、ok 执行；消息按 ok 推进（攻击/防御/物品/逃跑）
//! ```
//!
//! 剧情线一共四步：村长给任务（选项 → `Switch(1)`）→ 药师送药水（`Switch(3)`，游戏侧
//! 兑现成物品）→ 东侧草丘随机遇敌练级 → 进洞窟（`Switch(5)` 作信箱，游戏侧起 boss 战）
//! → 打赢 `Switch(2)`，村长与洞窟的对话页随之改写。事件脚本用**内联 RON**，桌面与 wasm
//! 同源一份（`examples/assets/events/` 那种磁盘副本在这里不需要：脚本即代码）。
//!
//! ## 运行
//!
//! - 桌面：`cargo run -p sisf --example rpg_demo`
//! - wasm：`cargo xtask web rpg_demo` 后访问 `http://127.0.0.1:8080/?ex=rpg_demo&wgl`
//!   （拉丁字体 Inter 来自本站 `assets/`，中文仍由宿主运行期从 CDN 补，与 `dialog_demo`
//!   同一套定案；不需要往 `web/assets/` 加任何文件）
//! - 单测：`cargo test -p sisf --example rpg_demo`（地图形状与连通、三份内联事件表
//!   编得过、页表、查表、走格程序、战斗数学期望区间、初始数值能赢第一只草丘怪）
//!
//! ## 手测清单（桌面与 wasm 同一份）
//!
//! 1. 方向键逐格走；水/树/房/洞窟不可进（撞墙只作废这一格）；走进草丘（深绿格）每
//!    完成一步 roll 一次遇敌。
//! 2. 面朝村长按 Enter：两段话 + 选项「愿意/还没准备好」；接受后再问是页 1；打赢 boss
//!    后再问是页 2 的道谢。**鼠标**：普通消息点窗内任意处 = 按 `ok`；有选项时点哪行
//!    就选哪行（游戏侧把点击翻译成"挪光标 + ok"的命名动作喂回事件机）。
//! 3. 面朝药师按 Enter：选「收下」→ 回地图时药水 +1（HUD 第三行报账）；再问是页 1；
//!    按 Esc（`allow_cancel`）等价于「不用了」。
//! 4. 面朝洞窟按 Enter：选「进去」→ boss 战（逃跑被拒）；打赢后洞窟换成页 1。
//! 5. 按 M 开菜单：状态面板数字与 HUD 对账；「使用药水」只在有药水且残血时生效；Esc 关。
//!    鼠标点菜单项 = 光标挪过去并立刻执行。
//! 6. 战斗：命中白闪 / 受击红晕纯计时器表现；防御把下一击减半；药水封顶；草丘遭遇
//!    五成半可逃；团灭回村满血。指令相鼠标点哪条执行哪条；消息相任意点击 = `ok`。
//! 7. 对话开着时按方向键不会推动世界（`busy()`）；消息窗在 UI 层，拉窗口不糊。

use std::collections::BTreeMap;
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
    ActionEdge, AudioPort, Command, EngineBus, EngineEvent, EventCtx, EventDef, EventRunner,
    EventScript, FadeTarget, PointerButton, PointerKind, Program, Prompt, RouteSpec, RouteStep,
    RunnerStatus, Scene, Sound, SoundChannel, UiPort, VarStore,
};
use sisf::render::{
    BatchKey, BatchList, Blend, DrawStats, Instance, NO_CLIP, Renderer, WHITE_EPOCH,
};
use sisf::sprite::{
    DIR_DOWN, DIR_LEFT, DIR_RIGHT, DIR_UP, Layer, Sprite, SpriteArena, SpriteRef, TaskScheduler,
    Tone, Vec2, ViewportRef,
};
use sisf::text::{Align, FontBundle, TextEngine, TextStyle};

// ---------------------------------------------------------------- 世界与几何

const VIRTUAL: (u32, u32) = (544, 416);
const TILE: f32 = 32.0;
const COLS: i32 = 17;
const ROWS: i32 = 13;
/// 与 `dialog_demo` 同速：`TILE / SPEED = 16` 拍一格。
const STEP_SPEED: f32 = 2.0;
const BUDGET: u32 = 64;
const TICK_HZ: f32 = 60.0;

const WORLD: Layer = Layer(0);
const UI_LAYER: u8 = 1;
const WORLD_VIEW: ViewportRef = ViewportRef::new(WORLD);

const ACTOR: Vec2 = Vec2::new(24.0, 24.0);
const HERO: u32 = 1;
const HERO_AT: (i32, i32) = (4, 5);

/// 站着不动的事件对象：`(归属 actor, 列, 行, 事件下标)`。
const PROPS: [(u32, i32, i32, usize); 2] = [(2, 1, 5, 0), (3, 5, 7, 2)];
/// 洞窟：一块不可进入的格子里嵌着事件。
const CAVE: (u32, i32, i32, usize) = (4, 13, 8, 1);

/// 开关约定（游戏侧与内联事件表共同的真相表）：
/// 1 = 接了讨伐；2 = boss 已死；3 = 领过药水；5 = 事件机投递的「起 boss 战」。
const SW_QUEST: u32 = 1;
const SW_BOSS: u32 = 2;
const SW_GIFT: u32 = 3;
const SW_CHALLENGE: u32 = 5;

/// 图例：`.` 草 · `G` 草丘（遇敌区） · `~` 水 · `T` 树 · `H` 房 · `C` 洞窟 · `#` 围墙。
const MAP: [&str; ROWS as usize] = [
    "#################",
    "#.T...~~..GGG...#",
    "#.HH..~~..GGG...#",
    "#.HH......GG....#",
    "#.....T....~....#",
    "#...............#",
    "#..T.......GGG..#",
    "#.HH............#",
    "#.HH.....~...C..#",
    "#............T..#",
    "#......GG.G.....#",
    "#...T...........#",
    "#################",
];

fn tile_at(cell: (i32, i32)) -> char {
    let (x, y) = (cell.0, cell.1);
    if x < 0 || y < 0 || x >= COLS || y >= ROWS {
        return '#';
    }
    MAP[y as usize].chars().nth(x as usize).unwrap_or('#')
}

fn lattice(cell: (i32, i32)) -> Vec2 {
    Vec2::new(cell.0 as f32 * TILE, (cell.1 + 1) as f32 * TILE)
}

fn cell_of(pos: Vec2) -> (i32, i32) {
    (
        (pos.x / TILE).round() as i32,
        (pos.y / TILE).round() as i32 - 1,
    )
}

fn ahead(cell: (i32, i32), dir: u8) -> (i32, i32) {
    match dir {
        DIR_UP => (cell.0, cell.1 - 1),
        DIR_DOWN => (cell.0, cell.1 + 1),
        DIR_LEFT => (cell.0 - 1, cell.1),
        _ => (cell.0 + 1, cell.1),
    }
}

fn dir_of_action(name: &str) -> Option<u8> {
    match name {
        "up" => Some(DIR_UP),
        "down" => Some(DIR_DOWN),
        "left" => Some(DIR_LEFT),
        "right" => Some(DIR_RIGHT),
        _ => None,
    }
}

/// 阻挡：围墙、水、树、房、洞窟，加上两位站着的人（他们不动，所以这张表是静态的）。
fn passable(x: f32, y: f32) -> bool {
    let cell = cell_of(Vec2::new(x, y));
    !matches!(tile_at(cell), '#' | '~' | 'T' | 'H' | 'C')
        && !PROPS.iter().any(|&(_, c, r, _)| (c, r) == cell)
}

fn in_grass(cell: (i32, i32)) -> bool {
    tile_at(cell) == 'G'
}

/// 面前的东西 → `(归属 actor, 事件下标)`。
fn examine(from: (i32, i32), dir: u8) -> Option<(u32, usize)> {
    let target = ahead(from, dir);
    if let Some((id, _, _, ev)) = PROPS.iter().find(|(_, c, r, _)| (*c, *r) == target) {
        return Some((*id, *ev));
    }
    if target == (CAVE.1, CAVE.2) {
        return Some((CAVE.0, CAVE.3));
    }
    None
}

/// 页号归游戏侧（"这一页此刻该不该出现"是世界的条件表），翻页归事件机 —— 与
/// `dialog_demo` 同一分工。
fn page_for(vars: &VarStore, ev: usize) -> usize {
    if ev == 0 {
        // 村长：boss 死了先道谢，接了任务再叮嘱，否则开场白。
        if vars.is_on(SW_BOSS) {
            2
        } else if vars.is_on(SW_QUEST) {
            1
        } else {
            0
        }
    } else if ev == 1 {
        if vars.is_on(SW_BOSS) { 1 } else { 0 }
    } else if ev == 2 {
        if vars.is_on(SW_GIFT) { 1 } else { 0 }
    } else {
        0
    }
}

// ---------------------------------------------------------------- 内联事件表

/// 三份脚本内联在代码里：与二进制同源，桌面和 wasm 不需要各自的磁盘副本。磁盘形状
/// 与 `examples/assets/events/*.ron` 一致（`speaker`/`cursor_se` 显式 `None`）。
const SCRIPTS: [&str; 3] = [
    // 0 · 村长：两段话 + 一个选项，接受 ⇒ `Switch(1)`。
    r#"(
    name: "elder",
    pages: [
        [
            ShowText(
                speaker: Some("村长"),
                body: "东边的草丘闹起了怪物，洞窟深处还有个大家伙。",
                cursor_se: None,
            ),
            ShowText(
                speaker: Some("村长"),
                body: "你愿意去东边洞窟讨伐洞窟主吗？",
                choices: ["愿意", "还没准备好"],
                result: Some(Int(10)),
                allow_cancel: false,
                cursor_se: None,
            ),
            If(
                cond: Cmp(lhs: Slot(Int(10)), op: Eq, rhs: Lit(0)),
                then: [
                    SetVar(slot: Switch(1), op: Set, value: Lit(1)),
                    ShowText(
                        speaker: Some("村长"),
                        body: "拜托你了。草丘里走多了会撞见怪物，先拿瓶药水。",
                        cursor_se: None,
                    ),
                ],
                else: [
                    ShowText(
                        speaker: Some("村长"),
                        body: "不急。药师在村南屋边，药水是白给的。",
                        cursor_se: None,
                    ),
                ],
            ),
        ],
        [
            ShowText(
                speaker: Some("村长"),
                body: "洞窟在地图东侧。多打几只草丘怪再进去。",
                cursor_se: None,
            ),
        ],
        [
            ShowText(
                speaker: Some("村长"),
                body: "好样的！洞窟主倒了，村子记你一功。",
                cursor_se: None,
            ),
        ],
    ],
)"#,
    // 1 · 洞窟：「进去」⇒ `Switch(5)`（游戏侧收信的信箱）；boss 死后换页。
    r#"(
    name: "cave",
    pages: [
        [
            ShowText(
                speaker: Some("洞窟"),
                body: "黑洞洞的，里面有沉重的呼吸声。",
                choices: ["进去", "离开"],
                result: Some(Int(12)),
                allow_cancel: true,
                cursor_se: None,
            ),
            If(
                cond: Cmp(lhs: Slot(Int(12)), op: Eq, rhs: Lit(0)),
                then: [
                    ShowText(body: "深处，一双眼睛睁开了。", cursor_se: None),
                    SetVar(slot: Switch(5), op: Set, value: Lit(1)),
                ],
                else: [],
            ),
        ],
        [
            ShowText(
                speaker: Some("洞窟"),
                body: "洞里安静下来了，只剩穿堂风。",
                cursor_se: None,
            ),
        ],
    ],
)"#,
    // 2 · 药师：`allow_cancel: true` 的白给。收下 ⇒ `Switch(3)`，兑现归游戏侧。
    r#"(
    name: "merchant",
    pages: [
        [
            ShowText(
                speaker: Some("药师"),
                body: "受伤了？这瓶药水送你，战斗中也能用。",
                choices: ["收下", "不用了"],
                result: Some(Int(11)),
                allow_cancel: true,
                cursor_se: None,
            ),
            If(
                cond: Cmp(lhs: Slot(Int(11)), op: Eq, rhs: Lit(0)),
                then: [
                    SetVar(slot: Switch(3), op: Set, value: Lit(1)),
                    ShowText(
                        speaker: Some("药师"),
                        body: "回复 30 点，收好。",
                        cursor_se: None,
                    ),
                ],
                else: [
                    ShowText(
                        speaker: Some("药师"),
                        body: "那留着，下次来拿也一样。",
                        cursor_se: None,
                    ),
                ],
            ),
        ],
        [
            ShowText(
                speaker: Some("药师"),
                body: "药都给你了，剩下的心得靠自己。",
                cursor_se: None,
            ),
        ],
    ],
)"#,
];

fn compile_scripts() -> (Vec<Arc<EventScript>>, Vec<String>) {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    for (i, src) in SCRIPTS.iter().enumerate() {
        match EventDef::from_ron(src).and_then(|def| def.compile()) {
            Ok(script) => out.push(Arc::new(script)),
            Err(e) => errors.push(format!("脚本 {i} 编不过: {e}")),
        }
    }
    (out, errors)
}

// ---------------------------------------------------------------- 资源（字体）

/// CJK 状态机与 `dialog_demo` 同一套定案：拉丁随本站清单，中文由宿主运行期从 CDN 补，
/// 引擎不管那个 URL。字形族定形前一个字都不画（中途换族会在覆盖率图集里串字）。
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
                platform::log_info(&format!("[rpg_demo] CJK 到货 {} 字节", bytes.len()));
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

/// 本示例刻意不带音频（事件表里没有一条 `Play*`，`cursor_se` 全 `None`），但
/// [`EventCtx`] 要求一个 [`AudioPort`] —— 桩的唯一义务是"真被调到了要说话"。
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

/// [`UiPort`]：只镜像事件机推来的东西，自己不做任何显示决策。
#[derive(Default)]
struct MessageBox {
    prompt: Option<Prompt>,
    dim: f32,
}

impl UiPort for MessageBox {
    fn show(&mut self, prompt: &Prompt) {
        self.prompt = Some(prompt.clone());
    }

    fn hide(&mut self) {
        self.prompt = None;
    }

    fn set_dim(&mut self, amount: f32) {
        self.dim = amount;
    }
}

// ---------------------------------------------------------------- 数值与战斗

/// 玩家成长：纯游戏侧的整数账。VarStore 只记世界事件（开关），不记属性。
#[derive(Clone, Debug)]
struct Hero {
    lv: i64,
    hp: i64,
    max_hp: i64,
    atk: i64,
    def: i64,
    exp: i64,
    gold: i64,
    potions: i64,
}

impl Hero {
    fn starter() -> Self {
        Self {
            lv: 1,
            hp: 45,
            max_hp: 45,
            atk: 9,
            def: 3,
            exp: 0,
            gold: 20,
            potions: 1,
        }
    }
}

const fn next_exp(lv: i64) -> i64 {
    12 * lv
}

const POTION_HEAL: i64 = 30;

fn roll_damage(atk: i64, def: i64, rng: &mut sisf::event::Rng) -> i64 {
    (atk - def + rng.range(-1, 1)).max(1)
}

/// 敌人规格表。`boss` 只有洞窟主一条；遭遇表靠它，控制流里不散落数值。
struct EnemySpec {
    name: &'static str,
    hp: i64,
    atk: i64,
    def: i64,
    exp: i64,
    gold: i64,
    size: f32,
    color: [f32; 4],
    boss: bool,
}

const ENEMIES: [EnemySpec; 3] = [
    EnemySpec {
        name: "草丘怪",
        hp: 26,
        atk: 7,
        def: 2,
        exp: 9,
        gold: 6,
        size: 72.0,
        color: [0.42, 0.55, 0.25, 1.0],
        boss: false,
    },
    EnemySpec {
        name: "尖壳兵",
        hp: 36,
        atk: 9,
        def: 4,
        exp: 15,
        gold: 11,
        size: 80.0,
        color: [0.30, 0.44, 0.62, 1.0],
        boss: false,
    },
    EnemySpec {
        name: "洞窟主",
        hp: 64,
        atk: 11,
        def: 4,
        exp: 45,
        gold: 60,
        size: 108.0,
        color: [0.52, 0.16, 0.22, 1.0],
        boss: true,
    },
];

const BOSS: usize = 2;

const COMMANDS: [&str; 4] = ["攻击", "防御", "使用药水", "逃跑"];

/// 一条消息之后要去哪一步。战斗节奏完全由玩家按 `ok` 推动，没有计时器。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Next {
    Command,
    EnemyTurn,
    /// 胜利退场（奖励在生成消息时已入账）。
    Win,
    /// 团灭：回村满血。
    Lose,
    /// 逃跑成功：安静地回地图。
    Fled,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    /// 一句话 + 按 `ok` 之后去哪（存在 `Battle::msg` / `Battle::next`）。
    Msg,
    /// 指令光标。
    Command,
}

struct Battle {
    enemy: usize,
    hp: i64,
    defending: bool,
    cmd: usize,
    phase: Phase,
    msg: String,
    next: Next,
    /// 命中白闪 / 受击红晕的剩余拍数（纯表现，不参与结算）。
    flash: u32,
    hurt: u32,
}

impl Battle {
    fn new(enemy: usize, intro: String) -> Self {
        Self {
            enemy,
            hp: ENEMIES[enemy].hp,
            defending: false,
            cmd: 0,
            phase: Phase::Msg,
            msg: intro,
            next: Next::Command,
            flash: 0,
            hurt: 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Map,
    Menu,
    Battle,
}

// ---------------------------------------------------------------- 示例本体

struct RpgDemo {
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
    scripts: Vec<Arc<EventScript>>,
    walks: BTreeMap<u8, Arc<Program>>,
    hero: SpriteRef,
    hero_data: Hero,
    facing: u8,
    /// 本拍发生的左键点击（虚拟像素），`tick` 里取走即消费。
    clicks: Vec<Vec2>,
    mode: Mode,
    menu_cursor: usize,
    menu_note: String,
    battle: Option<Battle>,
    /// 刚起步的一步要落在草丘上 —— 走完（世界让开）之后才 roll。
    walk_pending: bool,
    /// `Switch(3)` 只兑现一次（事件表记"给过没有"，游戏侧记"入账没有"）。
    gifted: bool,
    /// HUD 第三行的短报账，到点自己没。
    toast: String,
    toast_ticks: u32,
    ticks: u64,
    stats: Option<DrawStats>,
    pushed_ui: bool,
    last: String,
    warnings: u32,
    font: String,
}

impl RpgDemo {
    fn new() -> Self {
        let (scripts, errors) = compile_scripts();
        for e in errors {
            platform::log_info(&format!("[rpg_demo] {e}"));
        }
        assert_eq!(
            scripts.len(),
            SCRIPTS.len(),
            "内联事件表编不过就是代码错，不该带着半套世界开跑"
        );
        let walks = build_walks();

        let mut arena = SpriteArena::new();
        let mut scene = Scene::grid(TILE);
        scene.set_walkable(passable);
        let hero = spawn_world(&mut arena, &mut scene);

        Self {
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
            scripts,
            walks,
            hero,
            hero_data: Hero::starter(),
            facing: DIR_DOWN,
            clicks: Vec::new(),
            mode: Mode::Map,
            menu_cursor: 0,
            menu_note: String::new(),
            battle: None,
            walk_pending: false,
            gifted: false,
            toast: String::new(),
            toast_ticks: 0,
            ticks: 0,
            stats: None,
            pushed_ui: false,
            last: String::from("idle"),
            warnings: 0,
            font: String::new(),
        }
    }

    /// 队列纪律第 1 条：没人等输入 ⇒ 这一拍的按键不往队列里堆。
    fn spend_keys(&mut self) {
        let _ = self.bus.drain();
    }

    fn report_warnings(&mut self) {
        let warns = self.vm.take_warnings();
        self.warnings += warns.len() as u32;
        for w in warns {
            platform::log_info(&format!("[rpg_demo] 告警: {w}"));
        }
    }

    fn set_toast(&mut self, msg: &str, secs: u32) {
        self.toast = msg.to_owned();
        self.toast_ticks = secs * TICK_HZ as u32;
    }

    fn hero_cell(&self) -> (i32, i32) {
        self.arena
            .get(self.hero)
            .map(|s| cell_of(s.pos))
            .unwrap_or(HERO_AT)
    }

    // ---------------------------------------------------------- 地图侧

    fn on_map_decide(&mut self) {
        let facing = self.facing;
        let cell = self.hero_cell();
        if let Some((actor, ev)) = examine(cell, facing) {
            let page = page_for(&self.vars, ev);
            let script = Arc::clone(&self.scripts[ev]);
            let name = script.name.clone();
            if self.vm.start(actor, script, page) {
                self.last = format!("event {name} page {page}");
            }
        }
    }

    fn on_map_walk(&mut self, dir: u8) {
        self.facing = dir;
        self.walk_pending = in_grass(ahead(self.hero_cell(), dir));
        let prog = Arc::clone(&self.walks[&dir]);
        self.vm.start_program(HERO, prog);
    }

    /// 走完一格落在草丘上 ⇒ roll 一次遇敌（RPG Maker 的草丛语义）。
    fn check_encounter(&mut self) {
        if !self.walk_pending || !self.vm.is_idle() {
            return;
        }
        self.walk_pending = false;
        if !in_grass(self.hero_cell()) {
            return;
        }
        if self.vars.rng_mut().range(1, 100) > 16 {
            return;
        }
        let lv = self.hero_data.lv;
        let enemy = if lv >= 3 && self.vars.rng_mut().range(1, 100) >= 70 {
            1
        } else {
            0
        };
        let name = ENEMIES[enemy].name;
        self.start_battle(enemy, format!("野生的{name}挡住了去路！"));
    }

    /// 收信：事件机留下的开关，由游戏侧在世界让开之后兑现。
    fn collect_mailboxes(&mut self) {
        if self.vars.is_on(SW_CHALLENGE) {
            self.vars.set_switch(SW_CHALLENGE, false);
            if self.mode == Mode::Map && self.battle.is_none() {
                self.start_battle(BOSS, "洞窟深处，洞窟主睁开了眼睛。".to_owned());
            }
        }
        if self.vars.is_on(SW_GIFT) && !self.gifted {
            self.gifted = true;
            self.hero_data.potions += 1;
            self.set_toast("获得药水 ×1（药师）", 4);
        }
    }

    /// 点消息窗：有选项 ⇒ 点哪行就选哪行（往总线补"挪光标 + ok"这串命名动作，事件机
    /// 在当拍 `vm.tick` 里照常消费）；普通消息 ⇒ 点窗内任意处等价按 `ok`。
    /// 缺省键位表没给任何动作绑 Pointer，所以这一次翻译是唯一的，不会和 `InputState` 撞车。
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

    /// 点战斗画面：指令相点哪条执行哪条；消息相整屏任意点击都算 `ok`。
    fn click_battle(&mut self, c: Option<Vec2>) -> bool {
        let Some(c) = c else {
            return false;
        };
        match self.battle.as_mut() {
            Some(b) if b.phase == Phase::Command => {
                match (0..COMMANDS.len()).find(|&i| hit(cmd_row(i), c)) {
                    Some(i) => {
                        b.cmd = i;
                        true
                    }
                    None => false,
                }
            }
            Some(_) => true,
            None => false,
        }
    }

    // ---------------------------------------------------------- 菜单

    fn menu_items() -> [&'static str; 3] {
        ["状态", "使用药水", "离开"]
    }

    fn step_menu(&mut self, up: bool, down: bool, ok: bool, cancel: bool) {
        if cancel {
            self.mode = Mode::Map;
            self.menu_note.clear();
            return;
        }
        let n = Self::menu_items().len();
        if up {
            self.menu_cursor = (self.menu_cursor + n - 1) % n;
        }
        if down {
            self.menu_cursor = (self.menu_cursor + 1) % n;
        }
        if !ok {
            return;
        }
        match self.menu_cursor {
            0 => self.menu_note = String::from("状态在右边。下一格经验：打怪去。"),
            1 => {
                let note = {
                    let h = &self.hero_data;
                    if h.potions <= 0 {
                        Some(String::from("没有药水了。"))
                    } else if h.hp >= h.max_hp {
                        Some(String::from("血量是满的，别浪费。"))
                    } else {
                        None
                    }
                };
                if let Some(m) = note {
                    self.menu_note = m;
                } else {
                    let h = &mut self.hero_data;
                    h.potions -= 1;
                    h.hp = (h.hp + POTION_HEAL).min(h.max_hp);
                    self.menu_note = String::from("灌下一瓶药水，回了血。");
                }
            }
            _ => {
                self.mode = Mode::Map;
                self.menu_note.clear();
            }
        }
    }

    // ---------------------------------------------------------- 战斗

    fn start_battle(&mut self, enemy: usize, intro: String) {
        self.mode = Mode::Battle;
        self.battle = Some(Battle::new(enemy, intro));
    }

    fn say(&mut self, msg: String, next: Next) {
        let b = self.battle.as_mut().expect("say 只在战斗中调用");
        b.phase = Phase::Msg;
        b.msg = msg;
        b.next = next;
    }

    fn step_battle(&mut self, up: bool, down: bool, ok: bool) {
        let Some(b) = &self.battle else {
            self.mode = Mode::Map;
            return;
        };
        let (phase, next) = (b.phase, b.next);
        if phase == Phase::Msg {
            if !ok {
                return;
            }
            match next {
                Next::Command => {
                    self.battle.as_mut().unwrap().phase = Phase::Command;
                }
                Next::Win => {
                    self.battle = None;
                    self.mode = Mode::Map;
                }
                Next::Lose => {
                    self.hero_data.hp = self.hero_data.max_hp;
                    let hero = self.hero;
                    if let Some(spr) = self.arena.get_mut(hero) {
                        spr.teleport(lattice(HERO_AT));
                    }
                    self.battle = None;
                    self.mode = Mode::Map;
                    self.set_toast("药师把你拖回了村。", 4);
                }
                Next::Fled => {
                    self.battle = None;
                    self.mode = Mode::Map;
                    self.set_toast("逃出来了。", 3);
                }
                Next::EnemyTurn => self.enemy_turn(),
            }
            return;
        }

        // 指令光标相。
        {
            let b = self.battle.as_mut().unwrap();
            if up {
                b.cmd = (b.cmd + COMMANDS.len() - 1) % COMMANDS.len();
            }
            if down {
                b.cmd = (b.cmd + 1) % COMMANDS.len();
            }
        }
        if !ok {
            return;
        }
        let cmd = self.battle.as_ref().unwrap().cmd;
        match cmd {
            0 => {
                let (edef, name) = {
                    let b = self.battle.as_ref().unwrap();
                    (ENEMIES[b.enemy].def, ENEMIES[b.enemy].name)
                };
                let dmg = roll_damage(self.hero_data.atk, edef, self.vars.rng_mut());
                let dead = {
                    let b = self.battle.as_mut().unwrap();
                    b.hp -= dmg;
                    b.flash = 12;
                    if b.hp <= 0 {
                        b.hp = 0;
                    }
                    (b.hp <= 0, b.enemy)
                };
                let (dead, eidx) = dead;
                if dead {
                    self.win(eidx, name);
                } else {
                    self.say(
                        format!("你砍中{name}，它掉了 {dmg} 点血。"),
                        Next::EnemyTurn,
                    );
                }
            }
            1 => {
                self.battle.as_mut().unwrap().defending = true;
                self.say(
                    "你架起武器，把旧伤那一侧让给它。".to_owned(),
                    Next::EnemyTurn,
                );
            }
            2 => {
                let note = {
                    let h = &self.hero_data;
                    if h.potions <= 0 {
                        Some("口袋里空空如也。")
                    } else if h.hp >= h.max_hp {
                        Some("血是满的，省下这瓶。")
                    } else {
                        None
                    }
                };
                if let Some(m) = note {
                    self.say(m.to_owned(), Next::Command);
                } else {
                    let healed = {
                        let h = &mut self.hero_data;
                        h.potions -= 1;
                        let before = h.hp;
                        h.hp = (h.hp + POTION_HEAL).min(h.max_hp);
                        h.hp - before
                    };
                    self.say(format!("药水见底，你回了 {healed} 点血。"), Next::EnemyTurn);
                }
            }
            _ => {
                let boss = self.battle.as_ref().unwrap().enemy == BOSS;
                if boss {
                    self.say("洞窟主横在洞口，退路没有了。".to_owned(), Next::EnemyTurn);
                } else if self.vars.rng_mut().range(1, 100) <= 55 {
                    self.say("你甩开它跑出了草丘。".to_owned(), Next::Fled);
                } else {
                    self.say("没逃掉，它拦在身前。".to_owned(), Next::EnemyTurn);
                }
            }
        }
    }

    fn enemy_turn(&mut self) {
        let (atk, name, defending) = {
            let b = self.battle.as_ref().unwrap();
            (ENEMIES[b.enemy].atk, ENEMIES[b.enemy].name, b.defending)
        };
        let mut dmg = roll_damage(atk, self.hero_data.def, self.vars.rng_mut());
        if defending {
            dmg = (dmg / 2).max(1);
        }
        self.hero_data.hp -= dmg;
        let down = self.hero_data.hp <= 0;
        if down {
            self.hero_data.hp = 0;
        }
        {
            let b = self.battle.as_mut().unwrap();
            b.defending = false;
            b.hurt = 12;
        }
        if down {
            self.say(format!("{name}的最后一击把你掀倒了……"), Next::Lose);
        } else {
            self.say(
                format!("{name}扑上来，你受了 {dmg} 点伤害。"),
                Next::Command,
            );
        }
    }

    /// 击杀结算：奖励当场入账、升级并进同一句消息；boss 倒下开世界的开关。
    fn win(&mut self, eidx: usize, name: &str) {
        let spec = &ENEMIES[eidx];
        let (exp, gold, boss) = (spec.exp, spec.gold, spec.boss);
        let mut msg = format!("打败了{name}！得到 {exp} 点经验、{gold} 金。");
        {
            let h = &mut self.hero_data;
            h.exp += exp;
            h.gold += gold;
            while h.exp >= next_exp(h.lv) {
                h.exp -= next_exp(h.lv);
                h.lv += 1;
                h.atk += 2;
                h.def += 1;
                h.max_hp += 6;
                h.hp = h.max_hp;
                msg.push_str(&format!("\n升到 Lv{}！满血，攻＋2 防＋1。", h.lv));
            }
        }
        if boss {
            self.vars.set_switch(SW_BOSS, true);
            msg.push_str("\n村子安全了。");
        }
        self.say(msg, Next::Win);
    }

    // ---------------------------------------------------------- 字体与 HUD

    /// wasm：自带字体到货后换上（CJK 先、Inter 后，与 `dialog_demo` 同一注册顺序）。
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
                        platform::log_info(&format!("[rpg_demo] CJK 用不了: {why}"));
                    }
                }
            }
            match bundle.register_bytes(&inter) {
                Ok(family) => named.push(family),
                Err(why) => platform::log_info(&format!("[rpg_demo] 自带字体用不了: {why}")),
            }
            self.font = named.join("+");
            platform::log_info(&format!(
                "[rpg_demo] 字体 {}（{}，face {}）",
                self.font,
                self.assets.cjk.label(),
                bundle.face_count()
            ));
            *self.text.bundle_mut() = bundle;
            self.assets.font_loaded = true;
        }
    }

    /// HUD 状态行恒 ASCII：它最需要出现的那一刻，恰好是 CDN 那份中文到不了的那一刻。
    fn hud_lines(&self) -> [String; 3] {
        let h = &self.hero_data;
        let sw = |id: u32| self.vars.is_on(id) as u8;
        [
            format!(
                "mode {:?} · {} · quest {}/{}/{} · font {} {}",
                self.mode,
                self.last,
                sw(SW_QUEST),
                sw(SW_GIFT),
                sw(SW_BOSS),
                self.assets.cjk.label(),
                self.font,
            ),
            format!(
                "lv {} · hp {}/{} · atk {} · def {} · exp {}/{} · gold {} · potion x{}",
                h.lv,
                h.hp,
                h.max_hp,
                h.atk,
                h.def,
                h.exp,
                next_exp(h.lv),
                h.gold,
                h.potions,
            ),
            if self.toast.is_empty() {
                String::from("arrows move · Enter talk · M menu · click picks · tall grass bites")
            } else {
                self.toast.clone()
            },
        ]
    }
}

/// 摆好世界：勇者、村长、药师、洞窟。actor 号与内联事件表里写死的一致。
fn spawn_world(arena: &mut SpriteArena, scene: &mut Scene) -> SpriteRef {
    let hero = arena.spawn(actor_sprite([0.95, 0.96, 1.0, 1.0], 10, lattice(HERO_AT)));
    scene.set(HERO, hero);
    for (id, col, row, _) in PROPS {
        let color = if id == 2 {
            [0.86, 0.62, 0.34, 1.0]
        } else {
            [0.35, 0.75, 0.80, 1.0]
        };
        let s = arena.spawn(actor_sprite(color, 6, lattice((col, row))));
        scene.set(id, s);
    }
    let cave = arena.spawn(actor_sprite(
        [0.20, 0.18, 0.24, 1.0],
        5,
        lattice((CAVE.1, CAVE.2)),
    ));
    scene.set(CAVE.0, cave);
    hero
}

fn actor_sprite(color: [f32; 4], z: i32, at: Vec2) -> Sprite {
    let mut s = Sprite::solid(ACTOR);
    s.anchor = Vec2::new(0.0, 1.0);
    s.teleport(at);
    s.z = z;
    s.layer = WORLD;
    // 纯色块走 `Tone::Offset`：无位图时它是白图，加色分量为负即压暗成想要的颜色。
    s.tone = Tone::offset([color[0] - 1.0, color[1] - 1.0, color[2] - 1.0], 0.0);
    s
}

/// 四个方向的"走一格"程序，开局编好；按下方向键只是把它交给事件机 ——
/// "事件占着世界时玩家不能动"与"撞墙作废剩余格数"由此对玩家自动生效。
fn build_walks() -> BTreeMap<u8, Arc<Program>> {
    [DIR_UP, DIR_DOWN, DIR_LEFT, DIR_RIGHT]
        .into_iter()
        .map(|dir| {
            let prog = Program::compile(&[Command::MoveRoute {
                actor: HERO,
                route: RouteSpec {
                    steps: vec![RouteStep::Move { dir, cells: 1 }],
                    speed: STEP_SPEED,
                    through: false,
                },
                wait: true,
            }])
            .expect("一条 `MoveRoute` 该编得过");
            (dir, Arc::new(prog))
        })
        .collect()
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

/// 血条：底槽 + 按比例的一条（`frac` 钳进 0..=1）。
fn bar(renderer: &mut Renderer, rect: [f32; 4], frac: f32, fill: [f32; 4]) {
    quad(renderer, UI_LAYER, 3, rect, [0.0, 0.0, 0.0, 0.55]);
    let f = frac.clamp(0.0, 1.0);
    if f > 0.0 {
        quad(
            renderer,
            UI_LAYER,
            4,
            [rect[0], rect[1], rect[2] * f, rect[3]],
            fill,
        );
    }
}

// ---------------------------------------------------------------- 画面几何

/// 地图格子色。一眼分得出可去与不可去。
fn tile_color(c: char) -> Option<[f32; 4]> {
    match c {
        '.' => Some([0.36, 0.62, 0.32, 1.0]),
        'G' => Some([0.20, 0.44, 0.20, 1.0]),
        '~' => Some([0.20, 0.42, 0.78, 1.0]),
        'T' => Some([0.10, 0.30, 0.14, 1.0]),
        'H' => Some([0.62, 0.42, 0.30, 1.0]),
        'C' => Some([0.15, 0.14, 0.18, 1.0]),
        _ => None,
    }
}

/// 消息窗（事件对话）几何与 `dialog_demo` 同一份数学。
const WIN: [f32; 4] = [16.0, 248.0, 512.0, 152.0];
const NAME: [f32; 4] = [32.0, 258.0, 480.0, 18.0];
const BODY: [f32; 4] = [32.0, 280.0, 480.0, 54.0];
const CHOICE_X: f32 = 48.0;
const CHOICE_Y: f32 = 340.0;
const CHOICE_H: f32 = 20.0;
const CHOICE_LEAD: f32 = 4.0;
const WINDOW_BG: [f32; 4] = [0.05, 0.07, 0.16, 0.84];
const HIGHLIGHT: [f32; 4] = [0.94, 0.86, 0.42, 0.9];
const INK: [f32; 4] = [0.95, 0.96, 1.0, 0.94];
const INK_DIM: [f32; 4] = [0.72, 0.76, 0.86, 0.9];

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

/// 点选第 `target` 行选项要往总线里补的命名动作序列：先把光标挪过去，再按一次 `ok`。
/// 事件机自己只认 `up`/`down`/`ok`（`Hold::Text` 的解挂路径），所以"鼠标"这件事到它
/// 这里就只是"另一只手在敲命名动作" —— 缺省 `rpg_maker()` 没给任何动作绑 Pointer，
/// 点击不会被 `InputState` 翻译第二次，两边不会互相重复消费。
fn choice_click_seq(active: usize, target: usize) -> Vec<&'static str> {
    let mut seq: Vec<&'static str> = if target > active {
        vec!["down"; target - active]
    } else {
        vec!["up"; active - target]
    };
    seq.push("ok");
    seq
}

const MENU_RECT: [f32; 4] = [120.0, 96.0, 304.0, 190.0];
const MENU_ITEM_X: f32 = 140.0;
const MENU_ITEM_Y: f32 = 140.0;
const MENU_ITEM_H: f32 = 22.0;
const MENU_STAT_X: f32 = 306.0;
const MENU_NOTE: [f32; 4] = [140.0, 232.0, 150.0, 46.0];

/// 菜单第 `i` 项的可点矩形（与高亮条同一份数学，绘制与命中测试共用）。
fn menu_row(i: usize) -> [f32; 4] {
    [
        MENU_ITEM_X - 6.0,
        MENU_ITEM_Y + i as f32 * MENU_ITEM_H - 2.0,
        150.0,
        MENU_ITEM_H,
    ]
}

/// 战斗画面：上半屏敌人，下半屏左玩家右指令，底下一条消息。
const BATTLE_BG: [f32; 4] = [0.04, 0.05, 0.09, 1.0];
const EINFO: [f32; 4] = [16.0, 16.0, 236.0, 58.0];
const EBAR: [f32; 4] = [28.0, 46.0, 200.0, 8.0];
const PINFO: [f32; 4] = [16.0, 262.0, 250.0, 84.0];
const PBAR: [f32; 4] = [28.0, 292.0, 210.0, 8.0];
const CMD_RECT: [f32; 4] = [288.0, 262.0, 240.0, 84.0];
const CMD_X: f32 = 306.0;
const CMD_Y: f32 = 270.0;
const CMD_H: f32 = 16.0;
const MSG_RECT: [f32; 4] = [16.0, 352.0, 512.0, 52.0];

/// 战斗指令第 `i` 条的可点矩形（与指令高亮条同一份数学）。
fn cmd_row(i: usize) -> [f32; 4] {
    [CMD_X - 6.0, CMD_Y + i as f32 * (CMD_H + 3.0), 200.0, CMD_H]
}

fn enemy_rect(size: f32) -> [f32; 4] {
    [300.0 - size / 2.0, 232.0 - size, size, size]
}

impl RpgDemo {
    /// 事件机的消息窗（对话与选项）。纪律与 `dialog_demo` 相同：字体族没定形，
    /// 整个窗一个字都不落图集。
    fn draw_prompt(&mut self, renderer: &mut Renderer) {
        if let Some(p) = self.ui.prompt.clone() {
            quad(renderer, UI_LAYER, 1, WIN, WINDOW_BG);
            let style = TextStyle::new(13.0).aligned(Align::Left);
            let rows = p
                .choices
                .iter()
                .enumerate()
                .filter(|(i, _)| choice_rows_fit(i + 1))
                .collect::<Vec<_>>();
            if !rows.is_empty() {
                let i = p.active.min(rows.len() - 1);
                quad(renderer, UI_LAYER, 2, choice_row(i), HIGHLIGHT);
            }
            if let Some(speaker) = p.speaker.as_deref() {
                self.text
                    .draw_text_at(renderer, NAME, speaker, &style, INK_DIM, UI_LAYER);
            }
            self.text
                .draw_text_at(renderer, BODY, &p.body, &style, INK, UI_LAYER);
            for (i, choice) in rows {
                let mut r = choice_row(i);
                r[0] += 10.0;
                r[2] -= 10.0;
                self.text
                    .draw_text_at(renderer, r, choice, &style, INK, UI_LAYER);
            }
        }
    }

    fn draw_menu(&mut self, renderer: &mut Renderer) {
        let style = TextStyle::new(13.0).aligned(Align::Left);
        let cursor = self.menu_cursor;
        let (lv, hp, max_hp, atk, def, exp, gold, potions) = {
            let h = &self.hero_data;
            (h.lv, h.hp, h.max_hp, h.atk, h.def, h.exp, h.gold, h.potions)
        };
        quad(renderer, UI_LAYER, 4, MENU_RECT, WINDOW_BG);
        self.text.draw_text_at(
            renderer,
            [MENU_RECT[0] + 16.0, MENU_RECT[1] + 6.0, 200.0, 18.0],
            "菜单",
            &style,
            INK_DIM,
            UI_LAYER,
        );
        quad(renderer, UI_LAYER, 5, menu_row(cursor), HIGHLIGHT);
        let labels = {
            let items = Self::menu_items();
            [
                items[0].to_owned(),
                format!("{} x{}", items[1], potions),
                items[2].to_owned(),
            ]
        };
        for (i, label) in labels.iter().enumerate() {
            self.text.draw_text_at(
                renderer,
                [
                    MENU_ITEM_X,
                    MENU_ITEM_Y + i as f32 * MENU_ITEM_H,
                    150.0,
                    MENU_ITEM_H,
                ],
                label,
                &style,
                INK,
                UI_LAYER,
            );
        }
        let lines = [
            format!("Lv {lv}"),
            format!("HP {hp}/{max_hp}"),
            format!("攻击 {atk}"),
            format!("防御 {def}"),
            format!("经验 {exp}/{}", next_exp(lv)),
            format!("金 {gold}"),
        ];
        for (i, line) in lines.iter().enumerate() {
            self.text.draw_text_at(
                renderer,
                [MENU_STAT_X, MENU_ITEM_Y + i as f32 * 16.0, 110.0, 16.0],
                line,
                &style,
                if i < 2 { INK } else { INK_DIM },
                UI_LAYER,
            );
        }
        if !self.menu_note.is_empty() {
            let note = self.menu_note.clone();
            self.text
                .draw_text_at(renderer, MENU_NOTE, &note, &style, INK_DIM, UI_LAYER);
        }
    }

    fn draw_battle(&mut self, renderer: &mut Renderer) {
        // 快照先行：`self.text` 要 `&mut`，战斗状态与英雄数值必须先落成局部值。
        let Some(snap) = self.battle.as_ref().map(|b| {
            (
                b.enemy,
                b.hp,
                b.cmd,
                b.phase,
                b.flash,
                b.hurt,
                b.msg.clone(),
            )
        }) else {
            return;
        };
        let (eidx, ehp, cmd, phase, flash, hurt, msg) = snap;
        let (lv, hp, max_hp, potions, gold) = {
            let h = &self.hero_data;
            (h.lv, h.hp, h.max_hp, h.potions, h.gold)
        };
        let spec = &ENEMIES[eidx];
        let small = TextStyle::new(11.0).aligned(Align::Left);
        let style = TextStyle::new(13.0).aligned(Align::Left);
        let (vw, vh) = (VIRTUAL.0 as f32, VIRTUAL.1 as f32);

        // 矩形相（字体没到货也照画：色块不解释自己，但先给画面）。
        quad(renderer, UI_LAYER, 0, [0.0, 0.0, vw, vh], BATTLE_BG);
        quad(
            renderer,
            UI_LAYER,
            1,
            [0.0, 232.0, vw, 16.0],
            [0.13, 0.15, 0.20, 1.0],
        );
        let er = enemy_rect(spec.size);
        quad(renderer, UI_LAYER, 2, er, spec.color);
        if flash > 0 {
            quad(
                renderer,
                UI_LAYER,
                3,
                er,
                [1.0, 1.0, 1.0, flash as f32 / 12.0 * 0.8],
            );
        }
        quad(renderer, UI_LAYER, 2, EINFO, WINDOW_BG);
        quad(renderer, UI_LAYER, 2, PINFO, WINDOW_BG);
        bar(
            renderer,
            EBAR,
            ehp as f32 / spec.hp as f32,
            [0.30, 0.75, 0.35, 1.0],
        );
        bar(
            renderer,
            PBAR,
            hp as f32 / max_hp as f32,
            [0.85, 0.35, 0.30, 1.0],
        );
        if phase == Phase::Command {
            quad(renderer, UI_LAYER, 2, CMD_RECT, WINDOW_BG);
            quad(renderer, UI_LAYER, 3, cmd_row(cmd), HIGHLIGHT);
        }
        quad(renderer, UI_LAYER, 2, MSG_RECT, WINDOW_BG);
        if hurt > 0 {
            quad(
                renderer,
                UI_LAYER,
                6,
                [0.0, 0.0, vw, vh],
                [0.9, 0.1, 0.1, hurt as f32 / 12.0 * 0.22],
            );
        }

        if !self.assets.font_loaded {
            return;
        }
        self.text.draw_text_at(
            renderer,
            [EINFO[0] + 12.0, EINFO[1] + 6.0, 200.0, 18.0],
            spec.name,
            &style,
            INK,
            UI_LAYER,
        );
        self.text.draw_text_at(
            renderer,
            [EBAR[0], EBAR[1] + 10.0, 200.0, 14.0],
            &format!("HP {ehp}/{}", spec.hp),
            &small,
            INK_DIM,
            UI_LAYER,
        );
        self.text.draw_text_at(
            renderer,
            [PINFO[0] + 12.0, PINFO[1] + 6.0, 220.0, 18.0],
            &format!("勇者 Lv{lv}"),
            &style,
            INK,
            UI_LAYER,
        );
        self.text.draw_text_at(
            renderer,
            [PBAR[0], PBAR[1] + 10.0, 224.0, 16.0],
            &format!("HP {hp}/{max_hp} · 药水 x{potions} · 金 {gold}"),
            &small,
            INK_DIM,
            UI_LAYER,
        );
        if phase == Phase::Command {
            for (i, label) in COMMANDS.iter().enumerate() {
                self.text.draw_text_at(
                    renderer,
                    [CMD_X, CMD_Y + i as f32 * (CMD_H + 3.0), 200.0, CMD_H],
                    label,
                    &style,
                    INK,
                    UI_LAYER,
                );
            }
            self.text.draw_text_at(
                renderer,
                [
                    MSG_RECT[0] + 12.0,
                    MSG_RECT[1] + 6.0,
                    MSG_RECT[2] - 24.0,
                    40.0,
                ],
                "上/下选指令 · Enter 执行。防御吃一半，草丘里跑得掉，洞窟主面前跑不掉。",
                &style,
                INK_DIM,
                UI_LAYER,
            );
        } else {
            self.text.draw_text_at(
                renderer,
                [
                    MSG_RECT[0] + 12.0,
                    MSG_RECT[1] + 6.0,
                    MSG_RECT[2] - 44.0,
                    44.0,
                ],
                &msg,
                &style,
                INK,
                UI_LAYER,
            );
            self.text.draw_text_at(
                renderer,
                [
                    MSG_RECT[0] + MSG_RECT[2] - 40.0,
                    MSG_RECT[1] + 32.0,
                    30.0,
                    16.0,
                ],
                "ok▼",
                &small,
                INK_DIM,
                UI_LAYER,
            );
        }
    }
}

fn describe(status: RunnerStatus) -> String {
    match status {
        RunnerStatus::Idle { executed } => format!("idle {executed}"),
        RunnerStatus::Suspended { executed, reason } => format!("hold {reason} {executed}"),
        RunnerStatus::OverBudget { executed } => format!("over-budget {executed}"),
    }
}

impl App for RpgDemo {
    /// 只做一件事：把切片原样转进自己的总线（事件机是它唯一的读者），顺手记下左键
    /// 点击的位置 —— 指针的 `pos` 已是虚拟像素，命中测试直接和 UI 矩形比。
    /// 这里连一个裸键名都没有。
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

        // 命名动作一次读齐、按 mode 路由。`take_press` 读一次即消费 —— 对话框上
        // 那一下关窗的 Enter 也在这里被吃掉（当时的 `busy()` 挡掉地图侧分支），
        // 不会漏到下一拍又查一次面前。
        let up = ctx.input.take_press("up");
        let down = ctx.input.take_press("down");
        let ok = ctx.input.take_press("ok");
        let cancel = ctx.input.take_press("cancel");
        let menu = ctx.input.take_press("menu");
        let held = ["up", "down", "left", "right"]
            .iter()
            .rev()
            .find_map(|name| {
                ctx.input
                    .is_down(name)
                    .then(|| dir_of_action(name).unwrap())
            })
            .unwrap_or(0);

        if !self.vm.waiting_input() {
            self.spend_keys();
        }

        if menu {
            match self.mode {
                Mode::Map if self.vm.is_idle() => self.mode = Mode::Menu,
                Mode::Menu => {
                    self.mode = Mode::Map;
                    self.menu_note.clear();
                }
                _ => {}
            }
        }

        // 走完一格之后再 roll 遇敌（世界让开是切战斗的前提）。
        self.check_encounter();

        // 鼠标：一拍最多处理一次点击（连点五下会散在后续几拍里各算一次）。
        let click = self.clicks.pop();
        self.clicks.clear();

        match self.mode {
            Mode::Map => {
                if let Some(c) = click {
                    self.click_prompt(c);
                }
                if self.vm.is_idle() {
                    if ok {
                        self.on_map_decide();
                    } else if held != 0 {
                        self.on_map_walk(held);
                    }
                }
            }
            Mode::Menu => {
                let row = click
                    .and_then(|c| (0..Self::menu_items().len()).find(|&i| hit(menu_row(i), c)));
                match row {
                    // 点到哪条就把光标挪到哪条并立刻执行 —— 和键盘的"上下选、ok 执行"同一条路。
                    Some(i) => {
                        self.menu_cursor = i;
                        self.step_menu(false, false, true, false);
                    }
                    None => self.step_menu(up, down, ok, cancel),
                }
            }
            Mode::Battle => {
                let ok = ok || self.click_battle(click);
                self.step_battle(up, down, ok)
            }
        }

        if self.toast_ticks > 0 {
            self.toast_ticks -= 1;
            if self.toast_ticks == 0 {
                self.toast.clear();
            }
        }
        if let Some(b) = self.battle.as_mut() {
            b.flash = b.flash.saturating_sub(1);
            b.hurt = b.hurt.saturating_sub(1);
        }

        // 调度器由事件机推进（队列纪律第 2 条：这里一次都不调 `tasks.tick`）。
        let status = {
            let RpgDemo {
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
        self.collect_mailboxes();
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

        if self.mode == Mode::Battle {
            // 战斗整屏接管：地图这一帧不入画（精灵不采集也就不插值）。
            self.draw_battle(renderer);
            if self.assets.font_loaded {
                let line = self.hud_lines()[0].clone();
                self.text.draw_text_at(
                    renderer,
                    [8.0, vh - 16.0, vw - 16.0, 14.0],
                    &line,
                    &TextStyle::new(11.0).aligned(Align::Left),
                    [1.0, 1.0, 1.0, 0.5],
                    UI_LAYER,
                );
            }
            self.stats = renderer.stats();
            self.text.sync(renderer);
            return;
        }

        // 世界层：地图格子（静态矩形 `z = 0`）+ 人物（精灵 `z >= 5`，同一白图批）。
        for row in 0..ROWS {
            for col in 0..COLS {
                if let Some(color) = tile_color(tile_at((col, row))) {
                    quad(
                        renderer,
                        WORLD.0,
                        0,
                        [col as f32 * TILE, row as f32 * TILE, TILE, TILE],
                        color,
                    );
                }
            }
        }
        let mut list = BatchList::new();
        if self.arena.collect_at(WORLD_VIEW, alpha, &mut list).is_err() {
            platform::log_info("[rpg_demo] 采集失败（本示例只该有纯色块）");
            return;
        }
        renderer.append_list(list);

        // 黑幕（`Fade{Screen}` 归事件机）→ 对话窗 → 菜单 → HUD。
        let dim = self.ui.dim;
        if dim > 0.0 {
            quad(
                renderer,
                UI_LAYER,
                0,
                [0.0, 0.0, vw, vh],
                [0.0, 0.0, 0.0, dim],
            );
        }
        if self.assets.font_loaded {
            self.draw_prompt(renderer);
            if self.mode == Mode::Menu {
                self.draw_menu(renderer);
            }
            for (i, line) in self.hud_lines().iter().enumerate() {
                self.text.draw_text_at(
                    renderer,
                    [8.0, 6.0 + i as f32 * 18.0, vw - 16.0, 18.0],
                    line,
                    &TextStyle::new(13.0).aligned(Align::Left),
                    [1.0, 1.0, 1.0, 0.9],
                    UI_LAYER,
                );
            }
        }
        self.stats = renderer.stats();
        self.text.sync(renderer);
    }
}

fn config() -> AppConfig {
    AppConfig {
        title: "SISF rpg_demo · 简易 RPG".to_owned(),
        virtual_size: VIRTUAL,
        scale_mode: ScaleMode::Fit,
        tick_hz: TICK_HZ as f64,
        clear: [0.08, 0.10, 0.09, 1.0],
        ..Default::default()
    }
}

fn real_main() {
    platform::log_info("[rpg_demo] main 已启动");
    if let Err(e) = platform::run(RpgDemo::new(), config()) {
        panic!("[rpg_demo] 运行失败: {e}");
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
// 能自动化的部分都在例子文件里：地图形状与全图连通（阻挡表若割裂剧情就在这炸）、
// 三份内联事件表从源码解析并编译、页表与信箱开关、查表、走格程序、战斗数学期望
// 区间与"初始数值打得过第一只草丘怪"的确定性对局。跑法
// `cargo test -p sisf --example rpg_demo`。画面、手感与剧情推进走文件头手测清单。
#[cfg(test)]
mod tests {
    use super::*;
    use sisf::event::{Expr, Instr, Slot};

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

    #[test]
    fn the_map_is_a_closed_room_with_the_spots_in_the_right_places() {
        for (row, line) in MAP.iter().enumerate() {
            assert_eq!(line.chars().count(), COLS as usize, "第 {row} 行不是一行");
        }
        assert_eq!(MAP.len(), ROWS as usize);
        for x in 0..COLS {
            assert!(tile_at((x, 0)) == '#' && tile_at((x, ROWS - 1)) == '#');
        }
        for y in 0..ROWS {
            assert!(tile_at((0, y)) == '#' && tile_at((COLS - 1, y)) == '#');
        }
        assert!(
            passable(lattice(HERO_AT).x, lattice(HERO_AT).y),
            "出生点是墙"
        );
        for (id, c, r, _) in PROPS {
            assert_eq!(tile_at((c, r)), '.', "actor {id} 站进了非草地");
            assert!(
                !passable(lattice((c, r)).x, lattice((c, r)).y),
                "({c},{r}) 站着人却还能走进去"
            );
        }
        let cave_at = (CAVE.1, CAVE.2);
        assert_eq!(tile_at(cave_at), 'C');
        assert!(
            [DIR_UP, DIR_DOWN, DIR_LEFT, DIR_RIGHT].iter().any(|d| {
                let n = ahead(cave_at, *d);
                passable(lattice(n).x, lattice(n).y)
            }),
            "洞窟没有一面能被面朝"
        );
    }

    #[test]
    fn the_whole_floor_is_one_room() {
        let mut seen = vec![false; (COLS * ROWS) as usize];
        let mut queue = vec![HERO_AT];
        seen[(HERO_AT.1 * COLS + HERO_AT.0) as usize] = true;
        while let Some(cell) = queue.pop() {
            for dir in [DIR_UP, DIR_DOWN, DIR_LEFT, DIR_RIGHT] {
                let next = ahead(cell, dir);
                if next.0 < 0 || next.1 < 0 || next.0 >= COLS || next.1 >= ROWS {
                    continue;
                }
                let idx = (next.1 * COLS + next.0) as usize;
                if seen[idx] || !passable(lattice(next).x, lattice(next).y) {
                    continue;
                }
                seen[idx] = true;
                queue.push(next);
            }
        }
        for y in 0..ROWS {
            for x in 0..COLS {
                if passable(lattice((x, y)).x, lattice((x, y)).y) {
                    assert!(seen[(y * COLS + x) as usize], "({x},{y}) 孤立在图外");
                }
            }
        }
        // 遇敌区不许是死路：整片 G 都要从出生点到得了。
        for y in 0..ROWS {
            for x in 0..COLS {
                if tile_at((x, y)) == 'G' {
                    assert!(seen[(y * COLS + x) as usize], "草丘 ({x},{y}) 去不了");
                }
            }
        }
    }

    #[test]
    fn the_three_inline_scripts_compile_with_the_promised_pages() {
        let (scripts, errors) = compile_scripts();
        assert!(errors.is_empty(), "内联表编不过: {errors:?}");
        assert_eq!(
            scripts.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["elder", "cave", "merchant"],
            "`PROPS`/`CAVE` 的事件下标按这个顺序写死"
        );
        assert_eq!(
            scripts.iter().map(|s| s.pages.len()).collect::<Vec<_>>(),
            [3, 2, 2],
            "页表和 `page_for` 对不上"
        );
        let elder = page_cmds(&scripts[0], 0);
        assert!(
            elder.iter().any(|c| matches!(
                c,
                Command::SetVar {
                    slot: Slot::Switch(1),
                    value: Expr::Lit(1),
                    ..
                }
            )),
            "村长接受分支该落 Switch(1): {elder:?}"
        );
        let cave = page_cmds(&scripts[1], 0);
        assert!(
            cave.iter().any(|c| matches!(
                c,
                Command::SetVar {
                    slot: Slot::Switch(5),
                    ..
                }
            )),
            "洞窟不投递 Switch(5)，游戏侧永远等不到 boss 战: {cave:?}"
        );
        let merchant = page_cmds(&scripts[2], 0);
        assert!(
            merchant.iter().any(|c| matches!(
                c,
                Command::SetVar {
                    slot: Slot::Switch(3),
                    ..
                }
            )),
            "药师该落 Switch(3)（送药水的凭据）: {merchant:?}"
        );
        for cmds in [elder, cave, merchant] {
            for c in &cmds {
                if let Command::ShowText { choices, .. } = c {
                    assert!(choice_rows_fit(choices.len()), "{c:?} 的选项出窗");
                }
            }
        }
    }

    #[test]
    fn pages_switch_on_the_world_switches() {
        let mut vars = VarStore::new();
        assert_eq!(page_for(&vars, 0), 0, "开场是页 0");
        vars.set_switch(SW_QUEST, true);
        assert_eq!(page_for(&vars, 0), 1);
        vars.set_switch(SW_BOSS, true);
        assert_eq!(page_for(&vars, 0), 2, "boss 死后村长先道谢");
        assert_eq!(page_for(&vars, 1), 1, "洞窟换安静的页");
        assert_eq!(page_for(&vars, 2), 0, "药师页只跟药水开关走");
        vars.set_switch(SW_GIFT, true);
        assert_eq!(page_for(&vars, 2), 1);
    }

    #[test]
    fn examine_reads_props_and_the_cave_cell() {
        assert_eq!(examine((1, 4), DIR_DOWN), Some((2, 0)), "朝村长看");
        assert_eq!(examine((5, 6), DIR_DOWN), Some((3, 2)), "朝药师看");
        assert_eq!(examine((12, 8), DIR_RIGHT), Some((4, 1)), "朝洞窟看");
        assert_eq!(examine(HERO_AT, DIR_UP), None);
    }

    #[test]
    fn one_walk_beat_is_exactly_one_blocking_move_instruction() {
        let walks = build_walks();
        assert_eq!(walks.len(), 4);
        for (dir, prog) in &walks {
            assert_eq!(prog.code.len(), 1, "方向 {dir} 的程序该只有一条指令");
            assert!(
                matches!(
                    &prog.code[0],
                    Instr::Run(Command::MoveRoute { actor, wait: true, .. }) if *actor == HERO
                ),
                "方向 {dir} 走的不是阻塞路线"
            );
        }
        assert_eq!(dir_of_action("ok"), None, "动作对照表只认那四个名字");
    }

    #[test]
    fn battle_math_stays_in_its_promised_bands() {
        let mut rng = sisf::event::Rng::seeded(0xC0FFEE);
        for _ in 0..1000 {
            let d = roll_damage(9, 2, &mut rng);
            assert!((6..=8).contains(&d), "atk9-def2 的伤害 {d} 出了 [-1,+1] 带");
            assert!(roll_damage(1, 40, &mut rng) >= 1, "保底 1 点没了");
            let e = rng.range(1, 100);
            assert!((1..=100).contains(&e));
        }
        assert_eq!(next_exp(1), 12);
        assert!(ENEMIES[BOSS].boss && !ENEMIES[0].boss);
        assert!(
            ENEMIES[BOSS].hp > ENEMIES[1].hp && ENEMIES[BOSS].atk > ENEMIES[1].atk,
            "boss 必须比练级怪明显硬，手测清单第 4 条才是话"
        );
    }

    #[test]
    fn the_starter_beats_the_first_grass_monster_before_it_dies() {
        let mut hero = Hero::starter();
        let mut battle_hp = ENEMIES[0].hp;
        let mut rng = sisf::event::Rng::seeded(7);
        let mut turns = 0;
        let mut hero_wins = false;
        loop {
            turns += 1;
            battle_hp -= roll_damage(hero.atk, ENEMIES[0].def, &mut rng);
            if battle_hp <= 0 {
                hero_wins = true;
                break;
            }
            hero.hp -= roll_damage(ENEMIES[0].atk, hero.def, &mut rng);
            if hero.hp <= 0 {
                break;
            }
            assert!(turns < 50, "这场仗不该打这么久");
        }
        assert!(hero_wins, "初始数值下第一只草丘怪不该灭队");
        assert!(turns <= 6, "{turns} 回合才打死，太拖了");
    }

    #[test]
    fn hit_testing_is_left_inclusive_right_exclusive() {
        let r = [10.0, 20.0, 30.0, 40.0];
        assert!(hit(r, Vec2::new(10.0, 20.0)));
        assert!(!hit(r, Vec2::new(9.9, 30.0)), "左缘外不该算命中");
        assert!(!hit(r, Vec2::new(40.0, 60.0)), "右缘是开区间");
        assert!(!hit(r, Vec2::new(25.0, 60.0)), "下缘是开区间");
    }

    #[test]
    fn clickable_rows_sit_inside_their_windows_and_never_overlap() {
        // 选项行：脚本里最多两个，两行都该在消息窗内（留 8px 底边距）。
        for i in 0..2 {
            assert!(choice_rows_fit(i + 1), "第 {i} 行选项出窗");
            let row = choice_row(i);
            assert!(row[1] + row[3] <= WIN[1] + WIN[3] - 8.0);
        }
        for i in 0..3 {
            let row = menu_row(i);
            assert!(
                row[0] >= MENU_RECT[0]
                    && row[1] >= MENU_RECT[1]
                    && row[0] + row[2] <= MENU_RECT[0] + MENU_RECT[2]
                    && row[1] + row[3] <= MENU_RECT[1] + MENU_RECT[3],
                "菜单第 {i} 项出窗"
            );
            if i > 0 {
                let prev = menu_row(i - 1);
                assert!(prev[1] + prev[3] <= row[1], "菜单行 {i} 与上一行重叠");
            }
        }
        for i in 0..COMMANDS.len() {
            let row = cmd_row(i);
            assert!(
                row[1] >= CMD_RECT[1] && row[1] + row[3] <= CMD_RECT[1] + CMD_RECT[3],
                "指令第 {i} 条出窗"
            );
            if i > 0 {
                let prev = cmd_row(i - 1);
                assert!(prev[1] + prev[3] <= row[1], "指令行 {i} 与上一行重叠");
            }
        }
    }

    #[test]
    fn a_choice_click_moves_the_cursor_then_presses_ok_once() {
        assert_eq!(choice_click_seq(0, 2), vec!["down", "down", "ok"]);
        assert_eq!(choice_click_seq(2, 0), vec!["up", "up", "ok"]);
        assert_eq!(choice_click_seq(1, 1), vec!["ok"], "点当前行只补一次 ok");
    }
}
