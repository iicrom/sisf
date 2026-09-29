//! T10 验收示例：`assets/events/*.ron` → 事件虚拟机 → 消息窗 / 路线 / BGM / 淡入淡出。
//!
//! 世界小得能一眼看完（两口可查的东西 + 一个会走的 NPC），为的是让**事件机**这件事看得见：
//! 屏幕上没有一行字、没有一次位移是本示例自己编排的，全部来自那三份 RON。
//!
//! ```text
//! on_event  把切片原样转进自己的 EngineBus（事件机是它唯一的读者），
//!    │      同时读"命名动作"的边沿：方向按住 = 走一格，`ok` 按下 = 查面前那一格
//!  tick ────► EventRunner::tick(ctx, 64)
//!                │  (a) 脚本后端（本示例没装 ⇒ 连调用都不发生）
//!                │  (b) TaskScheduler 走一拍（路线与补间因此只被推进一次）
//!                │  (c) 解挂：文字/按键/等待/任务/淡入淡出
//!                │  (d) 执行指令到挂起、栈空或预算用完
//!                │  (e) 同步 UI（只在变化时 show/hide/set_dim）
//!    └────────► UiPort（消息窗与黑幕）+ AudioPort（四条通道，样本只解一次）
//! ```
//!
//! ## 两条队列纪律（这份示范里最容易写错的地方）
//!
//! 1. **谁吃掉一个按键，谁就把它从队列里拿掉。** 事件机只在 `waiting_input()` 时读总线，
//!    所以本示例在每拍开头、事件机没在等输入时把总线清空（`spend_keys`）。不清空的后果很
//!    具体：用来打开对话框的那一下 `ok` 会留在队列里，下一拍被 `ShowText` 的解挂逻辑吃掉 ——
//!    对话刚开就自己关了。
//! 2. **调度器只有一个主人。** `tasks.tick` 由 `EventRunner::tick` 调（`runner.rs` 的模块
//!    文档写了理由），本示例一次都不调。游戏侧再调一次 = 路线跑得比逻辑快两倍，而且不报错。
//!
//! ## 运行
//!
//! - 桌面：`cargo run --release -p sisf --example dialog_demo --features ogg,mp3,wav`
//!   音频取自 `crates/sisf-media/tests/fixtures/`，字体取自 `crates/sisf-text/tests/fixtures/`，
//!   事件表取自 `examples/assets/events/`（三个 `DirProvider`）。
//! - wasm：`cargo xtask web dialog_demo` 后访问 `http://127.0.0.1:8080/?ex=dialog_demo&wgl`
//!   （`web/assets/events/*.ron` 是 `examples/assets/events/*.ron` 的副本：桌面走目录
//!   provider、浏览器走 `fetch`，内容必须一致，改一处要改两处。中文**不在**那份清单里 ——
//!   wasm 没有系统字体，8MB 的 CJK 也不入库，开局由 `fetch_cjk` 从 CDN 取一次）
//! - 单测：`cargo test -p sisf --example dialog_demo`（事件表真的编得过、翻页条件、
//!   阻挡表、消息窗布局数学）
//!
//! ## 手测清单（桌面与 wasm 同一份）
//!
//! 1. 方向键/WASD/手柄十字键都能逐格走（示例代码里没有一个裸键名），走到井/宝箱/井边人跟前
//!    停住 —— 阻挡表在 `passable`，路线撞墙时**作废剩余格数但不卡事件**。
//! 2. 面朝井按 Enter：两个选项，上下键移光标并响一声光标音（SE 通道）。选"探头去看" ⇒ 井沿
//!    抖 12 拍（`Animate{wait:true}` 阻塞）再说话；选"回村说一声" ⇒ HUD 出现
//!    `Custom(dialog_demo.transfer)：这台事件机没装脚本后端`（DoD-4：**没做的能力要说话**）。
//! 3. 面朝宝箱按 Enter：选"撬开它" ⇒ 白闪 → 开关 1 打开 → 黑幕淡满 16 拍 → 说话 → 淡回 →
//!    `Jump{page:1}` 换页；再按一次 Enter，说的是第 1 页那句"空宝箱"（页号由游戏侧按开关
//!    决定，见 `page_for`）。选"走开"或按取消（`allow_cancel: true` ⇒ 结果 `-1`）⇒ 走 else。
//! 4. 面朝井边人按 Enter：BGM+BGS 淡入起播，他往上走一格、停 20 拍、走回来（`MoveRoute`
//!    `wait:true`，这期间玩家按键不该推动世界），问你要数字 —— 按 1..4 被 `Loop` 拦回去再问
//!    一次，按 5..9 才放行（`InputKey` 只认 `Press`，按住连发的 `Repeat` 不算第二个字符），
//!    最后 ME 起播、BGM/BGS 淡出、`Erase` 把他从世界里摘掉（HUD 的 actor 数跟着少一）。
//! 5. 全程 HUD 第一行显示上一拍的结论（`idle`/`hold text`/`hold task`/`over-budget`…，
//!    全 ASCII —— 状态行是给游戏侧 `match`、给像素探针当键用的标识，而且 CDN 挂掉那一拍
//!    中文正好读不出来，状态行必须仍然说得清自己）：
//!    **按着方向键狂按也不会看到 `over`** —— 超预算保护只在指令边界出现，死循环才会。
//! 6. 窗口拉大拉小：消息窗与黑幕都在 UI 层（viewport 栈第 1 层），不会跟着世界层抖。
//! 7. 中文：台词要有笔画而不是清一色方块。HUD 第一行末尾的 `font …` 就是答案 ——
//!    `cjk ok Noto Sans SC+Inter` = CDN 那份到货并注册上了；`cjk MISSING` = 取不到或解
//!    不出，中文会是方块（拉丁照常，因为 Inter 是单独注册的那一份）；桌面恒
//!    `cjk system Inter`，中文由系统字体兜。字体族定形前**一个字都不画**，所以开局只有
//!    色块，那是正常的而不是卡住。

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
use sisf::app::wasm_bindgen;
use sisf::app::{App, AppConfig, FrameCtx, ScaleMode, Viewport, platform};
#[cfg(not(target_arch = "wasm32"))]
use sisf::asset::DirProvider;
use sisf::asset::{Vfs, vfs::path as vpath};
#[cfg(target_arch = "wasm32")]
use sisf::asset::{VfsPath, VfsProvider, WebManifest};
use sisf::audio::AudioBus;
use sisf::audio::mixer::{Channel, PlayParams};
use sisf::event::{
    ActionEdge, AudioPort, Command, EngineBus, EngineEvent, EventCtx, EventDef, EventRunner,
    EventScript, FadeTarget, Program, Prompt, RouteSpec, RouteStep, RunnerStatus, Scene, Sound,
    SoundChannel, UiPort, VarStore,
};
use sisf::media::audio::{self, AudioData};
use sisf::render::{BatchKey, Blend, DrawStats, Instance, NO_CLIP, Renderer, WHITE_EPOCH};
use sisf::sprite::{
    DIR_DOWN, DIR_LEFT, DIR_RIGHT, DIR_UP, Layer, Sprite, SpriteArena, SpriteRef, TaskScheduler,
    Tone, Vec2, ViewportRef,
};
use sisf::text::{Align, FontBundle, TextEngine, TextStyle};

// ---------------------------------------------------------------- 世界与几何

/// 虚拟分辨率：RPG Maker MZ 的 544×416 = 17×13 格。
const VIRTUAL: (u32, u32) = (544, 416);
const TILE: f32 = 32.0;
/// 每拍位移（虚拟像素），与 `examples/tiled_map.rs` 同一个数：`TILE / SPEED = 16` 拍一格
/// ⇒ 3.75 格/秒，RPG Maker MV 的默认速度量级。
const STEP_SPEED: f32 = 2.0;
/// 一拍的缺省指令预算：够任何一条正常事件链在一拍内跑完，又拦得住死循环。
const BUDGET: u32 = 64;
/// 逻辑拍频（`Fade` 的拍→毫秒换算靠它）。
const TICK_HZ: f32 = 60.0;

/// 世界层（精灵与布景）：viewport 栈的第 0 层。
const WORLD: Layer = Layer(0);
/// 消息窗与黑幕所属的层：`prepare` 里恒推一层，所以恒为 1。批序先看 `layer`，UI 层因此
/// 天然压住世界层，与 `z`/`epoch` 无关。
const UI_LAYER: u8 = 1;
const WORLD_VIEW: ViewportRef = ViewportRef::new(WORLD);
const _: () = assert!(
    UI_LAYER > WORLD.0,
    "UI 层号必须大于世界层，否则黑幕盖不住地图（批先按 layer 排）"
);

/// 角色块：24×24 的纯色方块。行走图是 T8 的验收对象，这里要看的是一拍一动。
const ACTOR: Vec2 = Vec2::new(24.0, 24.0);

/// 1 号 actor 恒为玩家（事件表里的 `MoveRoute{actor: 1}` 与 `Erase` 都靠这个号找精灵）。
const HERO: u32 = 1;
/// 静态可查对象：`(归属 actor, 列, 行, 事件下标)`。
const OBJECTS: [(u32, i32, i32, usize); 2] = [(2, 5, 4, 0), (3, 8, 4, 1)];
/// 井边人：占用第 3 份事件表，格子每拍现算（他会走）。
const NPC: u32 = 4;
const NPC_EVENT: usize = 2;
const NPC_AT: (i32, i32) = (6, 7);
const HERO_AT: (i32, i32) = (7, 9);

/// 事件表（与 [`OBJECTS`] 的事件下标、[`NPC_EVENT`] 对齐）。
const EVENTS: [&str; 3] = ["events/well.ron", "events/chest.ron", "events/npc.ron"];

const FONT: &str = "Inter-Regular.ttf";
/// wasm 那份 CJK 在 VFS 里的键（**不在** [`MANIFEST`] 里：清单走本站 `assets/`，它走 CDN）。
#[cfg(target_arch = "wasm32")]
const CJK: &str = "NotoSansSC-Regular.otf";
/// 运行期从 CDN 取的中文轮廓（约 8 MB，**不入仓**）。三条选型理由：
/// - OFL 授权，且是全字库整包（不是只盖住本示例那几十个字的子集）；
/// - OTF 而不是 woff2 —— `FontBundle` 读的是 ttf/otf（`docs/text.md`）；
/// - jsDelivr 的 `/gh/` 带 `Access-Control-Allow-Origin: *`，钉 SHA 而不是 `main`
///   （分支会漂，漂了就是一版"今天还能显示中文"的验收）。
#[cfg(target_arch = "wasm32")]
const CJK_URL: &str = "https://cdn.jsdelivr.net/gh/notofonts/noto-cjk@f8d157532fbfaeda587e826d4cd5b21a49186f7c/Sans/SubsetOTF/SC/NotoSansSC-Regular.otf";
/// wasm 预取清单：字体 + 三份事件表 + 三份音频（`web/assets/` 下的同名文件，
/// 内容与 `web/assets/manifest.txt` 重合）。
#[cfg(target_arch = "wasm32")]
const MANIFEST: &str = "Inter-Regular.ttf\nevents/well.ron\nevents/chest.ron\nevents/npc.ron\n\
                        tone_2s.ogg\ntone_1s.wav\ntone_1s.mp3\n";

/// 格点阵上第 `(col, row)` 格的**左下角**（`anchor = (0,1)` 精灵的 `pos`）。
fn lattice(cell: (i32, i32)) -> Vec2 {
    Vec2::new(cell.0 as f32 * TILE, (cell.1 + 1) as f32 * TILE)
}

/// `pos` → 所在格。`round` 让逐格移动中途读它也只给出已经 committed 的那一格。
fn cell_of(pos: Vec2) -> (i32, i32) {
    (
        (pos.x / TILE).round() as i32,
        (pos.y / TILE).round() as i32 - 1,
    )
}

/// 朝向 → 该方向的下一格。
fn ahead(cell: (i32, i32), dir: u8) -> (i32, i32) {
    match dir {
        DIR_UP => (cell.0, cell.1 - 1),
        DIR_DOWN => (cell.0, cell.1 + 1),
        DIR_LEFT => (cell.0 - 1, cell.1),
        _ => (cell.0 + 1, cell.1),
    }
}

/// 命名动作 → 朝向（[`ActionMap::rpg_maker`] 的那四个名字；换键位不必碰这里）。
fn dir_of_action(name: &str) -> Option<u8> {
    match name {
        "up" => Some(DIR_UP),
        "down" => Some(DIR_DOWN),
        "left" => Some(DIR_LEFT),
        "right" => Some(DIR_RIGHT),
        _ => None,
    }
}

/// 阻挡表：井与宝箱占格，其余都能去（包括井边人此刻站的那一格）。
///
/// 角色之间不互相阻挡是**刻意的简化**：本示例要看的是"路线撞墙会作废剩余格数、但事件不
/// 卡死"（`MoveRoute` 的 T8 语义），谁挡谁属于 T11。
fn passable(x: f32, y: f32) -> bool {
    let cell = cell_of(Vec2::new(x, y));
    let cols = (VIRTUAL.0 as i32) / TILE as i32;
    let rows = (VIRTUAL.1 as i32) / TILE as i32;
    let inside = cell.0 >= 0 && cell.1 >= 0 && cell.0 < cols && cell.1 < rows;
    inside && !OBJECTS.iter().any(|(_, c, r, _)| (*c, *r) == cell)
}

/// 面前的东西 → `(归属 actor, 事件下标)`。井边人按他**当前**站的格子算（他会走，所以不能查表）。
fn examine(arena: &SpriteArena, scene: &Scene, from: (i32, i32), dir: u8) -> Option<(u32, usize)> {
    let target = ahead(from, dir);
    if let Some((id, _, _, ev)) = OBJECTS.iter().find(|(_, c, r, _)| (*c, *r) == target) {
        return Some((*id, *ev));
    }
    arena
        .get(scene.actor(NPC)?)
        .filter(|s| cell_of(s.pos) == target)
        .map(|_| (NPC, NPC_EVENT))
}

/// 同一个事件的哪一页：宝箱开过之后固定说第 1 页那句"空宝箱"（`Switch(1)`）。
///
/// 分工是刻意的 —— **页的选择归游戏侧**（"这一页此刻该不该出现"是世界的条件表），
/// **演出中途翻页归事件机**（`Jump{page}`）。
fn page_for(vars: &VarStore, actor: u32, ev: usize) -> usize {
    if ev == 1 && actor == 3 && vars.is_on(1) {
        1
    } else {
        0
    }
}

/// 消息窗几何（虚拟像素）。窗底留在 400（画面下沿留 16），三块区域从上到下是
/// 说话人 / 正文（最多三行）/ 选项区。
const WIN: [f32; 4] = [16.0, 248.0, 512.0, 152.0];
const NAME: [f32; 4] = [32.0, 258.0, 480.0, 18.0];
const BODY: [f32; 4] = [32.0, 280.0, 480.0, 54.0];
/// 选项区：左边 x、首格上沿 y、行高、行距。
const CHOICE_X: f32 = 48.0;
const CHOICE_Y: f32 = 340.0;
const CHOICE_H: f32 = 20.0;
const CHOICE_LEAD: f32 = 4.0;
const WINDOW_BG: [f32; 4] = [0.05, 0.07, 0.16, 0.84];
const HIGHLIGHT: [f32; 4] = [0.94, 0.86, 0.42, 0.9];
const INK: [f32; 4] = [0.95, 0.96, 1.0, 0.94];
const INK_DIM: [f32; 4] = [0.72, 0.76, 0.86, 0.9];

/// 第 `i` 项选项的行盒（高亮块与文字共用它，所以两者不可能错位）。
fn choice_row(i: usize) -> [f32; 4] {
    [
        CHOICE_X,
        CHOICE_Y + i as f32 * (CHOICE_H + CHOICE_LEAD),
        WIN[2] - 2.0 * (CHOICE_X - WIN[0]),
        CHOICE_H,
    ]
}

/// 选项最多能排几行还留在窗内（窗口不跟着内容长，超了就说明事件表写太多项）。
fn choice_rows_fit(n: usize) -> bool {
    n == 0 || choice_row(n - 1)[1] + CHOICE_H <= WIN[1] + WIN[3] - 8.0
}

// ---------------------------------------------------------------- 资源

/// CDN 那份 CJK 的到货状态。为什么要专门有这么一个东西：`load_font` 得等一份**异步**的
/// 字体，而一份永远不来的请求不能把拉丁一起拖死 —— 所以"结束了但没有"必须是一个比
/// "还在飞"更可分辨的状态。四种结局都进 HUD（方块不解释自己）。
#[derive(Clone)]
struct Cjk(Arc<AtomicU8>);

const CJK_PENDING: u8 = 0;
/// 桌面：这一趟 fetch 根本不存在，中文由系统字体兜（`docs/text.md` 平台矩阵）。
const CJK_SYSTEM: u8 = 1;
/// 字节已在清单里，等着注册。
const CJK_ARRIVED: u8 = 2;
/// 拿不到：请求挂了、响应是空的、或者那字节解不出 face。中文会是方块。
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

    /// 只有 wasm 会改写它（桌面的 [`CJK_SYSTEM`] 在 `new` 里一次定形）。
    #[cfg(target_arch = "wasm32")]
    fn set(&self, s: u8) {
        self.0.store(s, Ordering::Relaxed);
    }

    /// HUD 的 `font …` 段。四种结局都读得出来：中文排成方块这件事本身不解释自己。
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

/// VFS + 读表状态。音频样本的缓存不在这里（在 [`GameAudio`]，那里才是"只解一次"的现场）。
struct Assets {
    vfs: Vfs,
    font_loaded: bool,
    /// 最近一次读事件表缺了哪个（wasm 首帧清单还在飞）。
    missing: String,
    /// CDN 那份 CJK 到哪一步了（见 [`Cjk`]）。
    cjk: Cjk,
}

impl Assets {
    fn new() -> Self {
        let cjk = Cjk::new();
        #[cfg(not(target_arch = "wasm32"))]
        let mut vfs = Vfs::new();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
            vfs.register(Box::new(DirProvider::new(
                root.join("../../examples/assets"),
            )));
            vfs.register(Box::new(DirProvider::new(
                root.join("../../crates/sisf-text/tests/fixtures"),
            )));
            vfs.register(Box::new(DirProvider::new(
                root.join("../../crates/sisf-media/tests/fixtures"),
            )));
        }
        #[cfg(target_arch = "wasm32")]
        let vfs = {
            let manifest = Arc::new(WebManifest::new(MANIFEST));
            prefetch(&manifest);
            // 中文那份不排在清单里：清单只从本站 `assets/` 抓，而 8MB 的字体不入库。
            fetch_cjk(&manifest, &cjk);
            let mut vfs = Vfs::new();
            vfs.register(Box::new(SharedManifest(manifest)));
            vfs
        };
        Self {
            vfs,
            font_loaded: false,
            missing: String::from("未尝试"),
            cjk,
        }
    }

    fn read(&mut self, name: &str) -> Option<Vec<u8>> {
        match self.vfs.read(&vpath(name)) {
            Some(b) if !b.is_empty() => {
                self.missing.clear();
                Some(b)
            }
            _ => {
                self.missing = name.to_owned();
                None
            }
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

/// wasm：从 CDN 取 [`CJK`] 那份中文字体，成字节后灌进清单（`insert` 收清单外的键，
/// 见 `sisf-asset/src/vfs.rs` 的"运行期追加"）。
///
/// 两头都要说话：到货说字节数，取不到说原因，并把 [`CJK_NONE`] 写回状态。少了后者，
/// `load_font` 会一直等一份永远不来的字体，把本来能显示的拉丁一起拖死。
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
            // 404 页也会 resolve 出一个非空响应，而它解不出 face —— 那时候话说成
            // "字节解析不出任何 face"，排查的人会去查字体格式，而真正的错是 URL。
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
                // 字节数要说得出：这一趟是本示例唯一新增的网络依赖。
                platform::log_info(&format!("[dialog_demo] CJK 到货 {} 字节", bytes.len()));
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

/// 读齐三份事件表就返回三份 `EventScript`；任何一份没到货或编不过就返回已有的那几份
/// （下一拍再试）。编不过要留下话：那是**作者错误**，静默等于把事件表删了。
fn load_events(assets: &mut Assets) -> Vec<Arc<EventScript>> {
    let mut out: Vec<Arc<EventScript>> = Vec::new();
    for name in EVENTS {
        let Some(bytes) = assets.read(name) else {
            break;
        };
        let text = String::from_utf8_lossy(&bytes).into_owned();
        match EventDef::from_ron(&text).and_then(|def| def.compile()) {
            Ok(script) => {
                platform::log_info(&format!(
                    "[dialog_demo] 事件 `{}` ← {name}（页 {}，指令 {} 条）",
                    script.name,
                    script.pages.len(),
                    script.pages.iter().map(|p| p.code.len()).sum::<usize>()
                ));
                out.push(Arc::new(script));
            }
            Err(e) => {
                platform::log_info(&format!("[dialog_demo] {name} 编不过: {e}"));
                out.clear();
                break;
            }
        }
    }
    out
}

// ---------------------------------------------------------------- 两个端口

/// [`AudioPort`] 的实现：`sisf-audio` 的 `AudioBus` + 一份样本缓存。
struct GameAudio {
    bus: AudioBus,
    /// 名字 → **已解码**样本。缓存是端口契约的一部分（同一名字只解一次），不是优化。
    samples: BTreeMap<String, Arc<AudioData>>,
    /// 一拍的毫秒数：`Fade` 写的是拍，混音器要的是毫秒。
    beat_ms: f32,
    /// 最近一次落地的结论，进 HUD 第二行（"听不见"必须能说成原因）。
    status: String,
}

impl GameAudio {
    fn new() -> Self {
        let mut bus = AudioBus::open();
        // fixture 是满幅正弦，四路同开时压一档，听测不必被吓一跳。
        bus.set_master_gain(0.4);
        platform::log_info(&format!("[dialog_demo] 输出: {}", bus.describe()));
        Self {
            bus,
            samples: BTreeMap::new(),
            beat_ms: 1000.0 / TICK_HZ,
            status: String::from("静默"),
        }
    }

    fn sample(&mut self, vfs: &Vfs, name: &str) -> Result<Arc<AudioData>, String> {
        if let Some(hit) = self.samples.get(name) {
            return Ok(Arc::clone(hit));
        }
        let bytes = vfs
            .read(&vpath(name))
            .filter(|b| !b.is_empty())
            .ok_or_else(|| format!("VFS 里没有 {name}"))?;
        let data = audio::decode_audio(&bytes).map_err(|e| format!("{name} 解码失败: {e}"))?;
        platform::log_info(&format!(
            "[dialog_demo] 解码 {name}: {} 帧 / {:.2}s（只此一次）",
            data.frames(),
            data.duration()
        ));
        let arc = Arc::new(data);
        self.samples.insert(name.to_owned(), Arc::clone(&arc));
        Ok(arc)
    }
}

impl AudioPort for GameAudio {
    fn play(&mut self, vfs: &Vfs, channel: SoundChannel, sound: &Sound) -> Result<(), String> {
        let data = self.sample(vfs, &sound.name)?;
        let from_cache = self.samples.contains_key(&sound.name);
        self.bus
            .play(
                mixer_channel(channel),
                data,
                &PlayParams::default()
                    .gain(sound.gain)
                    .fade_in(sound.fade_in),
            )
            .map_err(|e| format!("{} 起播失败: {e}", channel.label()))?;
        self.status = format!(
            "{} ← {}（gain {:.2}{}）",
            channel.label(),
            sound.name,
            sound.gain,
            if from_cache { ", 复用样本" } else { "" }
        );
        Ok(())
    }

    fn fade(&mut self, target: FadeTarget, frames: u32, out: bool) -> Result<(), String> {
        let ms = frames as f32 * self.beat_ms;
        let to = if out { 0.0 } else { 1.0 };
        // `Screen` 的 `channels()` 是空的 ⇒ 这里一次都不碰混音器（黑幕归事件机自己管）。
        for ch in target.channels() {
            self.bus.fade_to(mixer_channel(*ch), to, ms);
        }
        self.status = format!("{} 淡向 {to:.1} / {ms:.0}ms", target_label(target));
        Ok(())
    }

    fn stop(&mut self, channel: SoundChannel) {
        self.bus.stop(mixer_channel(channel));
        self.status = format!("{} 停播", channel.label());
    }
}

fn mixer_channel(channel: SoundChannel) -> Channel {
    match channel {
        SoundChannel::Bgm => Channel::Bgm,
        SoundChannel::Bgs => Channel::Bgs,
        SoundChannel::Me => Channel::Me,
        SoundChannel::Se => Channel::Se,
    }
}

fn target_label(target: FadeTarget) -> &'static str {
    match target {
        FadeTarget::Bgm => "BGM",
        FadeTarget::Bgs => "BGS",
        FadeTarget::Me => "ME",
        FadeTarget::Se => "SE",
        FadeTarget::All => "四条通道",
        FadeTarget::Screen => "黑幕（端口不碰）",
    }
}

/// [`UiPort`] 的实现：只把事件机推来的东西存下来，画在 `draw` 里。
///
/// 契约是"UI 状态是事件机状态的函数"，所以这里没有任何自己的显示逻辑 —— 有第二份真相
/// 就有对不上账的那一天。
#[derive(Default)]
struct Window {
    prompt: Option<Prompt>,
    dim: f32,
    /// `show`/`hide` 的总次数：验收"内容没变就不该重复推"靠它（HUD 第二行）。
    pushes: u32,
}

impl UiPort for Window {
    fn show(&mut self, prompt: &Prompt) {
        self.pushes += 1;
        self.prompt = Some(prompt.clone());
    }

    fn hide(&mut self) {
        self.pushes += 1;
        self.prompt = None;
    }

    fn set_dim(&mut self, amount: f32) {
        self.dim = amount;
    }
}

// ---------------------------------------------------------------- 示例本体

struct DialogDemo {
    assets: Assets,
    text: TextEngine,
    arena: SpriteArena,
    tasks: TaskScheduler,
    scene: Scene,
    vars: VarStore,
    bus: EngineBus,
    audio: GameAudio,
    ui: Window,
    vm: EventRunner,
    /// 三份事件表（编译后的扁平 IR，`Arc` 共享，翻页只是换个 `Arc`）。
    scripts: Vec<Arc<EventScript>>,
    /// 四个方向的"走一格"程序，开局编好；按下方向键只是把它交给事件机。
    walks: BTreeMap<u8, Arc<Program>>,
    hero: SpriteRef,
    /// 此刻按住的方向（RPG 惯例：多键同按时后按者优先）。`0` = 没按住。
    held: u8,
    /// 角色面向。走一格转一次身，松手后仍然保留 —— "查面前那一格"用的是它。之所以记在
    /// 游戏侧而不是问精灵：`Sprite::set_dir` 只在挂了动画时生效，本示例的纯色方块没动画。
    facing: u8,
    decide: bool,
    ticks: u32,
    stats: Option<DrawStats>,
    /// `prepare` 推过 UI 层 ⇒ 下一帧的 `prepare` 要先 pop 回去。
    pushed_ui: bool,
    /// 上一拍的结论，进 HUD 第一行。
    last: String,
    /// 攒下没打印的告警条数（打印见 `report_warnings`）。
    warnings: u32,
    /// 装好的自带字体族名（HUD 第一行；`load_font` 之前是空串）。
    font: String,
}

impl DialogDemo {
    fn new() -> Self {
        let mut assets = Assets::new();
        let scripts = load_events(&mut assets);
        let walks = build_walks();

        let mut arena = SpriteArena::new();
        let mut scene = Scene::grid(TILE);
        scene.set_walkable(passable);
        let hero = spawn_world(&mut arena, &mut scene);

        let mut vars = VarStore::new();
        // 开关 0 = "世界已经摆好"。事件表没读它，它存在的意义是让 HUD 第三行有东西可显示，
        // 也顺便证明 `VarStore` 的开关集合不是只读的。
        vars.set_switch(0, true);

        Self {
            assets,
            text: TextEngine::new(FontBundle::system()),
            arena,
            tasks: TaskScheduler::new(),
            scene,
            vars,
            bus: EngineBus::new(),
            audio: GameAudio::new(),
            ui: Window::default(),
            vm: EventRunner::new(),
            scripts,
            walks,
            hero,
            held: 0,
            facing: DIR_DOWN,
            decide: false,
            ticks: 0,
            stats: None,
            pushed_ui: false,
            last: String::from("idle"),
            warnings: 0,
            font: String::new(),
        }
    }

    /// 事件机这一拍不在等输入 ⇒ 把总线倒空。见文件头的队列纪律第 1 条。
    fn spend_keys(&mut self) {
        let _ = self.bus.drain();
    }

    /// 把事件机的告警打出来（同一句话它每类只登记一次，所以这里不会刷屏）。
    fn report_warnings(&mut self) {
        let warns = self.vm.take_warnings();
        self.warnings += warns.len() as u32;
        for w in warns {
            platform::log_info(&format!("[dialog_demo] 告警: {w}"));
        }
    }

    /// 自带字体到货后换上（wasm 没有系统字体，消息窗全靠它）。
    ///
    /// 注册顺序 = 桌面的顺序：**CJK 先、Inter 最后**。[`FontBundle::register_bytes`] 把
    /// 最后注册的那份设成 sans/serif 默认族，所以拉丁仍然出自 Inter（与桌面同源），中文
    /// 则靠 per-glyph 回退落到那份 Noto 上 —— 与"系统字体兜 CJK"是同一套机制，只是字体
    /// 由谁提供不同（`docs/text.md` 平台矩阵）。
    ///
    /// [`CJK_PENDING`] 时整批再等一拍：先装 Inter 会让中文永远停在方块，而中途再换一次
    /// 字体族会在覆盖率图集里串字（`examples/windows.rs` 同一条 —— 换之前必须一个字都
    /// 没画过）。等的是"这件事结束了"而不是"字体到了"，见 [`Cjk`]。
    fn load_font(&mut self) {
        if self.assets.font_loaded || self.assets.cjk.state() == CJK_PENDING {
            return;
        }
        let Some(inter) = self.assets.vfs.read(&vpath(FONT)) else {
            return;
        };
        let mut bundle = FontBundle::system();
        let mut named: Vec<String> = Vec::new();
        #[cfg(target_arch = "wasm32")]
        if self.assets.cjk.state() == CJK_ARRIVED
            // 只读一次：这份字节有 8MB，而 `Vfs::read` 是拷贝出来的。
            && let Some(bytes) = self.assets.vfs.read(&vpath(CJK))
        {
            match bundle.register_bytes(&bytes) {
                Ok(family) => named.push(family),
                // 解不出 face ⇒ 与"根本没到货"同一个下场，但原因要分开说：那 URL 给的
                // 可能压根不是字体（`fetch_cjk` 只看 HTTP 状态，看不穿一份伪装成 200 的页）。
                Err(why) => {
                    self.assets.cjk.set(CJK_NONE);
                    platform::log_info(&format!("[dialog_demo] CJK 用不了: {why}"));
                }
            }
        }
        match bundle.register_bytes(&inter) {
            Ok(family) => named.push(family),
            Err(why) => platform::log_info(&format!("[dialog_demo] 自带字体用不了: {why}")),
        }
        self.font = named.join("+");
        platform::log_info(&format!(
            "[dialog_demo] 字体 {}（{}，face {}）",
            self.font,
            self.assets.cjk.label(),
            bundle.face_count()
        ));
        *self.text.bundle_mut() = bundle;
        self.assets.font_loaded = true;
    }

    fn hud_lines(&self) -> [String; 3] {
        [
            format!(
                "tick {} · {} · {} · font {} {}",
                self.ticks,
                self.last,
                self.vm_prompt_kind(),
                self.assets.cjk.label(),
                self.font,
            ),
            format!(
                "actors {} · ui pushes {} · {} · warn {}",
                self.arena.len(),
                self.ui.pushes,
                self.audio.status,
                self.warnings
            ),
            if self.scripts.len() == EVENTS.len() {
                format!(
                    "arrows move · Enter examine · sw 1/2 = {}/{} · answer={}",
                    self.vars.is_on(1) as u8,
                    self.vars.is_on(2) as u8,
                    self.vars.text("answer")
                )
            } else {
                format!("loading events… missing {}", self.assets.missing)
            },
        ]
    }

    fn vm_prompt_kind(&self) -> &'static str {
        match self.ui.prompt.as_ref() {
            Some(p) if !p.choices.is_empty() => "choices",
            Some(_) => "text",
            None if self.ui.dim > 0.0 => "black",
            None => "none",
        }
    }
}

/// 四个方向的"走一格"程序，开局编好；按下方向键只是把它交给事件机。
///
/// 为什么走格子要绕事件机一圈，而不是直接改 `pos`：这样"事件占着世界时玩家不能动"
/// （`busy()`）与"撞墙作废剩余格数"两条规则自动对玩家生效，和 `MoveRoute` 走的是同一条路。
///
/// `wait: true` 是**逐格**的开关，不是风格问题：不阻塞的话事件机一拍就空出来，按着方向键
/// 每拍又排一条新路线 ⇒ 队列越堆越长，逻辑位置跑得比画面快一倍。
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

/// 摆好世界：玩家、两口可查的东西、井边人。`scene` 里的 actor 号必须和事件表里写的一致。
fn spawn_world(arena: &mut SpriteArena, scene: &mut Scene) -> SpriteRef {
    let hero = arena.spawn(actor_sprite(INK, 10, lattice(HERO_AT)));
    scene.set(HERO, hero);
    for (id, col, row, _) in OBJECTS {
        let s = arena.spawn(actor_sprite(
            [0.62, 0.66, 0.78, 1.0],
            4,
            lattice((col, row)),
        ));
        scene.set(id, s);
    }
    let npc = arena.spawn(actor_sprite([0.86, 0.62, 0.34, 1.0], 6, lattice(NPC_AT)));
    scene.set(NPC, npc);
    hero
}

fn actor_sprite(color: [f32; 4], z: i32, at: Vec2) -> Sprite {
    let mut s = Sprite::solid(ACTOR);
    s.anchor = Vec2::new(0.0, 1.0);
    s.teleport(at);
    s.z = z;
    s.layer = WORLD;
    // 纯色方块走 `Tone::Offset`：无位图时它是白图，加色分量为负即压暗成想要的颜色。
    s.tone = Tone::offset([color[0] - 1.0, color[1] - 1.0, color[2] - 1.0], 0.0);
    s
}

/// 一块纯色矩形（`WHITE_EPOCH` ⇒ 与同层其它矩形一起并进同一个批，靠 `z` 分先后）。
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

impl App for DialogDemo {
    /// 两件事：把切片原样转进自己的总线（事件机是它唯一的读者），以及读游戏侧要用的边沿。
    ///
    /// 方向看的是 `Hold` 而不是只看 `Press`：`ActionMap::rpg_maker()` 给四向移动设的是
    /// `one_shot: false, autofire: None`，所以按住期间每拍都有一条 `Hold`，只认 `Press`
    /// 的话按着不放会一走一停。
    fn on_event(&mut self, events: &[EngineEvent], _ctx: &mut FrameCtx) {
        for e in events {
            self.bus.push(e.clone());
            let EngineEvent::Action { name, edge } = e else {
                continue;
            };
            if let Some(dir) = dir_of_action(name) {
                self.held = match edge {
                    ActionEdge::Press | ActionEdge::Hold => dir,
                    // 松开手上这个方向就退回中性：还有别的方向按着时，下一拍的 `Hold`
                    // 事件会把它再填回来（连发节拍比一帧快）。
                    ActionEdge::Release => 0,
                };
                continue;
            }
            if *edge == ActionEdge::Press && name == "ok" {
                self.decide = true;
            }
        }
    }

    fn tick(&mut self, _dt: Duration, ctx: &mut FrameCtx) {
        self.ticks += 1;
        // 每拍开头收拢 `prev_pos`（T8 第五条"抖"），且必须在任何移动之前。
        self.arena.begin_tick();
        self.text.set_raster_scale(ctx.raster_scale());
        if self.scripts.len() < EVENTS.len() {
            self.scripts = load_events(&mut self.assets);
        }

        // 队列纪律第 1 条：没人等输入 ⇒ 这一拍的按键不该往队列里堆。
        if !self.vm.waiting_input() {
            self.spend_keys();
        }

        // 玩家想动，且事件机没占着世界 —— `busy()` 是这条判断的全部内容。
        if self.vm.is_idle() && self.scripts.len() == EVENTS.len() {
            if self.decide {
                let facing = if self.held == 0 {
                    self.facing
                } else {
                    self.held
                };
                self.facing = facing;
                let at = self.arena.get(self.hero).map(|s| cell_of(s.pos));
                let found = at.and_then(|cell| examine(&self.arena, &self.scene, cell, facing));
                if let Some((actor, ev)) = found {
                    let page = page_for(&self.vars, actor, ev);
                    let script = Arc::clone(&self.scripts[ev]);
                    let name = script.name.clone();
                    if self.vm.start(actor, script, page) {
                        self.last = format!("event {name} page {page}");
                    }
                }
            } else if self.held != 0 {
                // 走一格 = 转一次身：面向记在这里，因为纯色方块没有朝向状态。
                self.facing = self.held;
                let prog = Arc::clone(&self.walks[&self.held]);
                self.vm.start_program(HERO, prog);
            }
        }
        self.decide = false;

        // 调度器由事件机推进（队列纪律第 2 条：这里一次都不调 `tasks.tick`）。
        let status = {
            let DialogDemo {
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
    }

    fn prepare(&mut self, _alpha: f32, ctx: &mut FrameCtx) {
        if self.pushed_ui {
            ctx.pop_viewport();
        }
        let (vw, vh) = (VIRTUAL.0 as f32, VIRTUAL.1 as f32);
        // UI 层 = 一个与原画同尺寸、零平移的 viewport：投影是恒等，层号却比世界大一层。
        let layer = ctx.push_viewport(Viewport::at(0.0, 0.0, vw, vh));
        debug_assert_eq!(layer, UI_LAYER, "UI 层号与 `UI_LAYER` 的约定不符");
        self.pushed_ui = true;
    }

    fn draw(&mut self, alpha: f32, renderer: &mut Renderer) {
        self.load_font();
        let n = self.arena.len().max(1) as u32;
        if renderer.quads_per_slot() != n {
            renderer.set_quads_per_slot(n);
        }

        // 世界：角色与布景都在 `WORLD` 层，一次采集 ⇒ 一个白图批。
        let mut list = sisf::render::BatchList::new();
        if self.arena.collect_at(WORLD_VIEW, alpha, &mut list).is_err() {
            platform::log_info("[dialog_demo] 采集失败（本示例不该有 tone 之外的东西）");
            return;
        }
        renderer.append_list(list);

        // UI 层：黑幕 → 窗底 → 光标 → 文字。同一 `WHITE_EPOCH` 里靠 `z` 分先后，
        // 文字自带 epoch 所以恒在矩形之上。
        let (vw, vh) = (VIRTUAL.0 as f32, VIRTUAL.1 as f32);
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
        // 字体没到货就整个窗都不画：覆盖率图集的 `CacheKey` 里带着 face 序号，中途换字体
        // 族会让旧页被新 face 命中 —— 那就是串字（`examples/windows.rs` 同一条）。
        if let Some(p) = self.ui.prompt.clone()
            && self.assets.font_loaded
        {
            quad(renderer, UI_LAYER, 1, WIN, WINDOW_BG);
            let style = TextStyle::new(13.0).aligned(Align::Left);
            // 窗高是写死的 ⇒ 装不下的选项宁可不画，也不要溢出到窗框外面去。
            let rows = p
                .choices
                .iter()
                .enumerate()
                .filter(|(i, _)| choice_rows_fit(i + 1))
                .collect::<Vec<_>>();
            if !rows.is_empty() {
                // `rows` 恒是从 0 起的连续前缀，所以光标下标只需夹进可见行数。
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
        // 同一条理由：状态行也是字，没字体族定形之前一个字都不落图集。
        if self.assets.font_loaded {
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

/// 上一拍的结论 → HUD 上那一行短句。
///
/// 状态行刻意全 ASCII：这一行最需要出现的那一刻，是字体还没到货、或者运行期那份 CJK 根本
/// 取不到的那一刻（`docs/text.md` 平台矩阵），那时中文在这块画布上就是一排方块 —— 而"事件机
/// 现在挂在什么上面"正是 wasm 手测唯一要读的东西。台词本身仍是中文（那是内容）。
fn describe(status: RunnerStatus) -> String {
    match status {
        RunnerStatus::Idle { executed } => format!("idle {executed}"),
        RunnerStatus::Suspended { executed, reason } => {
            format!("hold {reason} {executed}")
        }
        RunnerStatus::OverBudget { executed } => format!("over-budget {executed}"),
    }
}

fn config() -> AppConfig {
    AppConfig {
        title: "SISF dialog_demo (T10)".to_owned(),
        virtual_size: VIRTUAL,
        scale_mode: ScaleMode::Fit,
        tick_hz: TICK_HZ as f64,
        clear: [0.11, 0.15, 0.13, 1.0],
        ..Default::default()
    }
}

fn real_main() {
    platform::log_info("[dialog_demo] main 已启动");
    if let Err(e) = platform::run(DialogDemo::new(), config()) {
        panic!("[dialog_demo] 运行失败: {e}");
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
// 例子内能自动化的部分都在这里：三份事件表**从磁盘**解析并编译（字段名写错就在这炸，
// 而不是开局静默地没事件）、页结构与 `Jump` 的目标、翻页条件、`examine` 的查表、阻挡表、
// 消息窗布局数学。跑法 `cargo test -p sisf --example dialog_demo`（`[[example]]` 里
// `test = true`）。剩下的是画面、声音和手感 —— 走文件头那份手测清单。
#[cfg(test)]
mod tests {
    use super::*;
    use sisf::event::{Instr, Slot};

    /// 读齐三份事件表。读不齐直接 panic：那正是下面几条测试要看的事。
    fn loaded() -> Vec<Arc<EventScript>> {
        let mut assets = Assets::new();
        let scripts = load_events(&mut assets);
        assert_eq!(
            scripts.len(),
            EVENTS.len(),
            "事件表没读齐，最后缺的是 {:?}",
            assets.missing
        );
        scripts
    }

    /// 某一页上看得见的动作（控制流已被 `Program::compile` 抹平，这里只取叶子）。
    fn runs(script: &EventScript, page: usize) -> Vec<Command> {
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
    fn the_event_tables_on_disk_parse_and_compile() {
        let s = loaded();
        assert_eq!(
            s.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["well", "chest", "npc"],
            "`EVENTS` 的顺序就是事件下标，改了顺序 `OBJECTS`/`NPC_EVENT` 全都得跟着改"
        );
        // 页数 = `page_for` 与 `Jump{page}` 依赖的那张表：只有宝箱开过之后换一页。
        assert_eq!(
            s.iter().map(|e| e.pages.len()).collect::<Vec<_>>(),
            [1, 2, 1],
            "页表和 `page_for`/`chest.ron` 的 `Jump` 对不上了"
        );
        assert!(
            s.iter().all(|e| e.pages.iter().all(|p| !p.code.is_empty())),
            "空页会被事件机当成'这个事件没内容'，等于静默删表"
        );
    }

    #[test]
    fn the_well_asks_a_two_choice_question_and_leaves_a_script_hook() {
        let s = loaded();
        let cmds = runs(&s[0], 0);
        let Command::ShowText {
            choices,
            result,
            cursor_se,
            allow_cancel,
            ..
        } = &cmds[0]
        else {
            panic!("井的第一条该是 `ShowText`，实际是 {cmds:?}");
        };
        assert_eq!(choices, &["探头去看", "回村说一声"]);
        assert_eq!(*result, Some(Slot::Int(5)));
        assert!(
            cursor_se.is_some(),
            "移光标没有反馈 ⇒ 玩家不知道该当成选项在动"
        );
        assert!(
            !allow_cancel,
            "这两条选项里没有'都不选'这条路，取消就该无效"
        );
        // 文件头的手测清单承诺了这句话会出现在 HUD 上，话题名必须对得上。
        assert!(
            cmds.iter().any(|c| matches!(c, Command::Custom(topic, args)
                    if topic == "dialog_demo.transfer" && args.len() == 2)),
            "井的第二分支该把'回村'交给脚本后端: {cmds:?}"
        );
    }

    #[test]
    fn the_chest_jumps_to_its_opened_page() {
        let s = loaded();
        let cmds = runs(&s[1], 0);
        assert!(
            cmds.iter()
                .any(|c| matches!(c, Command::SetVar { slot, .. } if *slot == Slot::Switch(1))),
            "撬开宝箱要落下开关 1，`page_for` 就是按它换页的"
        );
        assert!(
            cmds.iter().any(|c| matches!(c, Command::Jump { page: 1 })),
            "开完该翻到第 1 页: {cmds:?}"
        );
        assert!(
            matches!(cmds.last(), Some(Command::ShowText { .. })),
            "两条分支汇合后还剩一句'它还在原地': {cmds:?}"
        );
        // 第 1 页 = 开过的样子：一句话 + 一次不阻塞的淡出（阻塞会把窗挂在那儿）。
        let opened = runs(&s[1], 1);
        assert!(
            matches!(
                opened.as_slice(),
                [
                    Command::ShowText { .. },
                    Command::Animate { wait: false, .. }
                ]
            ),
            "第 1 页的形状变了: {opened:?}"
        );
    }

    #[test]
    fn the_npc_walks_fades_and_erases_himself() {
        let s = loaded();
        let cmds = runs(&s[2], 0);
        assert!(
            matches!(
                cmds.first(),
                Some(Command::PlayBgm { .. } | Command::PlayBgs { .. })
            ),
            "他一开口就该起 BGM: {cmds:?}"
        );
        assert!(
            cmds.iter()
                .any(|c| matches!(c, Command::MoveRoute { wait: true, .. })),
            "路线不阻塞的话他会边走边说话，验收点就没了: {cmds:?}"
        );
        assert!(
            matches!(cmds.last(), Some(Command::Erase)),
            "井边人最后要把自己从世界里摘掉: {cmds:?}"
        );
        assert!(
            cmds.iter().any(|c| matches!(
                c,
                Command::Fade {
                    target: FadeTarget::Bgm,
                    out: true,
                    ..
                }
            )),
            "走人之前 BGM 该淡出: {cmds:?}"
        );
    }

    #[test]
    fn only_the_chest_turns_its_page() {
        let mut vars = VarStore::new();
        assert_eq!(page_for(&vars, 3, 1), 0, "没撬开的宝箱说第一页");
        vars.set_switch(1, true);
        assert_eq!(page_for(&vars, 3, 1), 1);
        // 牵连别的 actor 或别的事件都会导致页号越界（事件机只会告警，不会替你猜）。
        assert_eq!(page_for(&vars, 2, 0), 0, "井没有第二页");
        assert_eq!(page_for(&vars, 4, 2), 0, "NPC 也没有");
        assert_eq!(page_for(&vars, 3, 0), 0, "下标 0 的事件不是宝箱");
    }

    #[test]
    fn examine_reads_the_table_for_props_and_the_ground_for_the_npc() {
        let mut arena = SpriteArena::new();
        let mut scene = Scene::grid(TILE);
        let hero = spawn_world(&mut arena, &mut scene);
        assert_eq!(cell_of(arena.get(hero).unwrap().pos), HERO_AT);
        // 井在 (5,4)、宝箱在 (8,4)：站在旁边朝它们看才查得到。
        assert_eq!(examine(&arena, &scene, (5, 5), DIR_UP), Some((2, 0)));
        assert_eq!(examine(&arena, &scene, (7, 4), DIR_RIGHT), Some((3, 1)));
        assert_eq!(examine(&arena, &scene, HERO_AT, DIR_DOWN), None);
        // 井边人按**当前**格子算：他会走，把他挪一格后原来的位置就该查不到了。
        assert_eq!(
            examine(&arena, &scene, (6, 8), DIR_UP),
            Some((NPC, NPC_EVENT))
        );
        let npc = scene.actor(NPC).unwrap();
        arena.get_mut(npc).unwrap().teleport(lattice((6, 6)));
        assert_eq!(examine(&arena, &scene, (6, 8), DIR_UP), None);
        assert_eq!(
            examine(&arena, &scene, (6, 7), DIR_UP),
            Some((NPC, NPC_EVENT))
        );
    }

    #[test]
    fn the_object_table_blocks_exactly_what_it_draws() {
        for (col, row) in OBJECTS.iter().map(|&(_, c, r, _)| (c, r)) {
            let at = lattice((col, row));
            assert!(
                !passable(at.x, at.y),
                "({col},{row}) 上摆着东西却还能走进去"
            );
        }
        assert!(
            passable(lattice(HERO_AT).x, lattice(HERO_AT).y),
            "出生点是墙"
        );
        assert!(
            passable(lattice(NPC_AT).x, lattice(NPC_AT).y),
            "NPC 起点是墙"
        );
        // npc.ron 的路线：从他脚下往上一步。那格不可走的话事件会被阻挡判定作废，
        // 手测清单第 4 条就看不出是路线的锅还是地图的锅。
        assert!(
            passable(
                lattice(ahead(NPC_AT, DIR_UP)).x,
                lattice(ahead(NPC_AT, DIR_UP)).y
            ),
            "井边人的路线第一步撞墙"
        );
        let (cols, rows) = (
            VIRTUAL.0 as i32 / TILE as i32,
            VIRTUAL.1 as i32 / TILE as i32,
        );
        for outside in [(-1, 5), (cols, 5), (0, -1), (0, rows)] {
            let at = lattice(outside);
            assert!(!passable(at.x, at.y), "{outside:?} 在画面外却还能站");
        }
    }

    #[test]
    fn the_window_holds_every_choice_list_on_disk() {
        assert!(
            WIN[1] + WIN[3] <= VIRTUAL.1 as f32,
            "消息窗整个儿得留在画面里"
        );
        assert!(choice_rows_fit(0) && choice_rows_fit(2));
        assert!(
            !choice_rows_fit(3),
            "三行就该溢出，这条测试的前提是窗装不下三行"
        );
        let widest = loaded()
            .iter()
            .flat_map(|e| e.pages.iter())
            .flat_map(|p| p.code.iter())
            .map(|i| match i {
                Instr::Run(Command::ShowText { choices, .. }) => choices.len(),
                _ => 0,
            })
            .max()
            .unwrap_or(0);
        assert!(
            choice_rows_fit(widest),
            "有事件表写了 {widest} 条选项，消息窗装不下（要么改窗高要么砍选项）"
        );
    }

    #[test]
    fn one_walk_beat_is_exactly_one_move_instruction() {
        let walks = build_walks();
        assert_eq!(walks.len(), 4);
        for (dir, prog) in walks {
            assert_eq!(prog.code.len(), 1, "方向 {dir} 的程序该只有一条指令");
            assert!(
                matches!(
                    &prog.code[0],
                    Instr::Run(Command::MoveRoute { actor, wait: true, .. })
                        if *actor == HERO
                ),
                "方向 {dir} 走的不是玩家的阻塞 `MoveRoute`: {:?}",
                prog.code[0]
            );
            assert_eq!(dir_of_action("up"), Some(DIR_UP), "对照表不该在循环里变");
        }
        // 动作名 ↔ 朝向的对照表只认那四个名字（其余动作归事件机自己读总线）。
        assert_eq!(dir_of_action("ok"), None);
    }
}
