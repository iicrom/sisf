//! T11 的移植可行性证明点：把一份**散文件的 RPG Maker MV 工程**装进这台引擎。
//!
//! 工程是 `examples/assets/mv/` 那份自造的极简项目（`python examples/assets/mv/gen.py` 生成，
//! 理由写在那份脚本的文档里）：`data/{System,Map001,Tilesets}.json` + `graphics/` + `audio/`。
//! 本示例一行 MV 的数据结构都不重新定义，全部走 `sisf::rpgmk`：
//!
//! ```text
//!  data/System.json  ─► SystemDef ─► 出生点 + 两张名字表 + ActionMap
//!  data/Map001.json  ─► MapDef  ─┬─► Scene_Map（格子、页条件、碰撞）─► 精灵位置
//!  data/Tilesets.json─► TilesetDef┘        │
//!                                          ├─► translate_event ─► EventDef ─► compile ─► EventScript
//!  graphics/*.png   ─► decode_image ─► Atlas ─► Bitmap ─► Sprite.src
//!  audio/*.ogg      ─► MvAudio::sound ─► AudioPort（同一名字只解码一次）
//! ```
//!
//! ## 屏幕上没有一样东西是本示例编排的
//!
//! 图块怎么摆、事件在第几格、哪一页此刻成立、说了什么话、响哪条音频 —— 全在 JSON 里。
//! 本示例只负责三件事：① 把 MV 的形状翻译成引擎的形状（`sisf-rpgmk`），② 把翻译结果交给
//! 事件机（`sisf-event`），③ 把两边剩下对不上的地方**接住并说话**（`ScriptBackend`）。
//!
//! ## 位置为什么由场景层定、像素由精灵追（而不是让 `MoveRoute` 来写）
//!
//! MV 的 `Game_CharacterBase` 同时存 `_x/_y`（格子）与 `_realX/_realY`（像素），前者是真相、
//! 后者是它派生的插值。这里照抄这个分工：[`Scene_Map`] 拿格子（一拍一跳，含碰撞与页条件），
//! 精灵按该页的 `moveSpeed` 逐拍**追赶**自己的格子（[`chase`]）。
//!
//! 因此**没有**把玩家移动翻成 `Command::MoveRoute`（`dialog_demo` 那么做是有理由的：那里
//! 只有虚拟机一个主人）。这一层一旦让路线回写像素，就会出现"格子说你在 (5,8)、像素说在
//! (5.4,8)"两个真相源，而 `can_pass` 读的是格子。同理 [`Scene`] 上不设 `set_walkable`：
//! 没有路线要查阻挡，表设了就是第二份碰撞数据。
//!
//! ## 两处有意不建模（不是"还没写"）
//!
//! 1. **`trigger` 4（并行处理事件）**：这台虚拟机只有一个挂起点（`hold`），一条程序。
//!    并行页要的是"每个事件一个常驻解释器"，那是新的一层，不在 T11 的定价里。
//! 2. **`moveType` 3（自定义路线）**：`Scene_Map` 是格子的唯一真相源，而路线执行器逐拍回写
//!    像素 —— 同上一条的冲突。[`translate_route`] 仍然把 MV 的路线翻成了 [`RouteSpec`]（那是
//!    数据层的活，翻得动），只是本示例没有执行它的通路。
//!
//! 两条都在 `docs/porting.md` 的状态表里，本文件不假装支持。同一格上多个事件只起**第一个**
//! 也是同一台虚拟机的既有形状（挂起点单份），不是这里的取舍。
//!
//! ## 视频桩是真的被走了一遍
//!
//! 石碑亮起来那一页末尾有 `261`（播放影片），翻成 `Command::Custom("rpgmv.movie", …)`。
//! [`ShellBackend`] 接住它并调 [`VideoPlayer`]：`open` 恒 `Err(Unsupported)`，
//! [`VideoPlayer::skip_notice`] 只 warn 一次。于是"movie 播不了"是一句有出处的降级说明，
//! 而不是运行时猜出来的 —— 这正是 T11 留这个桩的用途（DoD-4 的唯一例外走完全程）。
//!
//! ## 运行
//!
//! - 桌面：`cargo run --release -p sisf --example rpgmv_shell --features image,ogg,video,rpgmk`
//!   （工程根 `examples/assets/mv/`，字体根 `crates/sisf-text/tests/fixtures/`，两个 `DirProvider`）
//! - wasm：`cargo xtask web rpgmv_shell` 后访问 `http://127.0.0.1:8080/?ex=rpgmv_shell&wgl`
//!   （`web/assets/mv/` 是 `examples/assets/mv/` 的**副本**，改一处要改两处；wasm 侧要**两份清单
//!   两个 URL 前缀** —— 工程走 `assets/mv/{key}`、字体走 `assets/{key}`，而 VFS 键必须保持 MV 的
//!   `data/…` 约定，见 [`Assets::new`]）
//! - 单测：`cargo test -p sisf --example rpgmv_shell`
//!
//! ## 手测清单（桌面与 wasm 同一份）
//!
//! 1. 开局 1 秒内自动放一条钟响（事件 3 是 `trigger` 3 自动执行）：黑幕淡出 → 一句话 → SE →
//!    开关 4 ON → 淡回，然后**那个事件从图上消失**（它的第 0 页是空页）。HUD 第二行的
//!    `views` 少一个。
//! 2. 出生点在 `(2,8)` 的路上（读自 `data/System.json`，不在地图里），朝右。按右走 4 格到长老
//!    跟前停住（他 `priorityType` 1 挡路），HUD 出现 `blocked`。
//! 3. 面朝长老按 Enter：两行话 + 三个选项，上下键移光标、Enter 选定、Esc 取消（`403`）。
//!    选"去看一看" ⇒ 开关 3 ON（HUD `sw3`）+ 一条 SE；再按一次 Enter 说的是**另一页**那句
//!    "你又回来了。"（自我开关 A 走的是 `123`，页条件是场景层判的）。
//! 4. 走到石碑 `(16,8)` 跟前按 Enter：碑上的字浮出来，然后 console/HUD 出现**一次**
//!    `跳过影片 'ending'`（第二次读同一页不再刷第二条 —— 那是 `skip_notice` 的闩锁）。
//!    走近了才看得见：碑那一格换成 `LIT` 图块，因为**它那一页是图块事件**。
//! 5. 撞到史莱姆 `(4,12)`（它每 60 拍随机挪一格）：一句话、一条 ME，然后**它消失了**
//!    （`214` 擦除事件 → 虚拟机 despawn 精灵，本示例回头把格子状态也标成 erased）。
//! 6. 全程 BGM 在放（`$dataMap.bgm` 的 `autoplayBgm`），HUD 第二行末写着 `BGM ← …（复用样本）`；
//!    最小化窗口 ⇒ 静音，回来 ⇒ 续播（`Lifecycle` 转发给 `AudioBus`）。
//! 7. 中文：桌面是系统字体，正常。**wasm 上台词会是一排方块** —— 定案是"字形来源归宿主"
//!    （本示例只自带 Inter，不去 CDN 取 8MB 的 CJK；那条通路 `dialog_demo` 已经验过）。
//!    所以 wasm 复验看的是 HUD 那三行 ASCII 状态、色块位置与 `views` 计数，不是台词。

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;
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
    ActionEdge, AudioPort, EngineBus, EngineEvent, EventCtx, EventRunner, EventScript, FadeTarget,
    Lifecycle, Param, Program, Prompt, RunnerStatus, Scene, ScriptBackend, Sound, SoundChannel,
    Submission, UiPort,
};
use sisf::media::audio::{self, AudioData};
use sisf::render::{
    Atlas, AtlasPages, BatchKey, BatchList, Bitmap, Blend, DrawStats, Instance, NO_CLIP, Renderer,
    UploadRing, WHITE_EPOCH,
};
use sisf::rpgmk::GameVars;
use sisf::rpgmk::map::{EventPage, MapDef, SystemDef, TilesetDef};
use sisf::rpgmk::scene::{Game_Character, PLAYER, Scene_Map};
use sisf::rpgmk::translate::{
    MV_TILE_PX, Options, TranslateReport, mv_speed_px_per_tick, translate_event,
};
use sisf::sprite::{
    AnimDef, AnimState, DIR_DOWN, DIR_LEFT, DIR_RIGHT, DIR_UP, DirPattern, Layer, Rect, Sprite,
    SpriteArena, SpriteRef, TaskScheduler, Vec2, ViewportRef, dir_vector,
};
use sisf::text::{Align, FontBundle, TextEngine, TextStyle};
use sisf::video::VideoPlayer;

// ---------------------------------------------------------------- 几何与时钟

/// 图块边长（虚拟像素）。MV 的图块是 48×48，而 `translate` 的速度换算用的就是同一个数 ——
/// 这里**不许**另写一个 48.0，否则路线速度与格子大小会各说各话。
const TILE: f32 = MV_TILE_PX;
/// 画面：MV 的默认分辨率 816×624 = 17×13 个 48 格。
const VIRTUAL: (u32, u32) = (816, 624);
const TICK_HZ: f32 = 60.0;
/// 一拍的指令预算。
const BUDGET: u32 = 64;
/// 玩家的 MV 速度档（`distancePerFrame = 2^4/256` 个图块/帧 ⇒ 3 虚拟像素/拍 = 3.75 格/秒）。
const PLAYER_SPEED: i64 = 4;
/// 本示例算出来的玩家每拍像素数（点阵折整用它，见 [`RpgmvShell::lattice_alpha`]）。
///
/// 它不是随便挑的数：`translate` 把 MV 的速度档折成"像素/拍"走的是同一个
/// [`mv_speed_px_per_tick`]，格子大小和路线速度必须同源。等式钉在单测
/// [`player_px_comes_from_the_same_formula_as_routes`] 里（那个换算不是 `const fn`，
/// 编译期断言写不下）。
const PLAYER_PX: f32 = 3.0;

/// `tilesetNames` 里取哪一张当图块美术：`A1..A5,B,C,D,E` 的第 6 张（MV 的 `B`）。
/// 引擎不实现 MV 的 A1–E 七图编号与 48 段自动图块，所以图块号 → 子矩形由宿主定，
/// 而这张图就是 `examples/assets/mv/` 里那份 8 列 × 4 行的 `Shell.png`。
const TILE_SHEET: usize = 5;
/// 玩家的行走图。MV 从 `$dataActors[主角].characterName` 取，而本 fixture 没有
/// `data/Actors.json`（角色数据属于 RPG 层，不属于这一层要证明的通路）⇒ 照 MV 编辑器的
/// 默认值写死 `Actor1`。
const PLAYER_CHARACTER: &str = "Actor1";

/// 世界层（图块与角色）：`prepare` 推的第一层 ⇒ 层号 1。
const WORLD: Layer = Layer(1);
/// HUD / 消息窗 / 黑幕所在层。
const UI_LAYER: u8 = 2;
const WORLD_VIEW: ViewportRef = ViewportRef::new(WORLD);
const _: () = assert!(
    UI_LAYER > WORLD.0,
    "UI 层号必须大于世界层，否则消息窗会被地图盖住（批先按 layer 排）"
);

/// 三条地板带（`MapDef` 的三个图块层，都画在角色之下）。
const Z_FLOOR: [i32; 3] = [0, 1_000, 2_000];
/// 三层地板与 `Z_FLOOR` 等长是 `build_floor` 直接 `enumerate()` 的前提。
const _: () = assert!(Z_FLOOR.len() == MapDef::TILE_LAYERS);
/// `priorityType` 0（底下）⇒ 永远在角色之下。
const Z_BELOW_CHAR: i32 = 2_500;
/// `priorityType` 1（与玩家同级）：角色与图块事件共用这一带，按脚底 y 互压。
const Z_CHAR: i32 = 3_000;
/// `priorityType` 2（上方）。上沿留 1000 给角色带（图高 864 < 1000）。
const Z_ABOVE_CHAR: i32 = 5_000;

/// MV 行走图的列序：`[站立, 迈步 A, 迈步 B]`。
///
/// MV 的**站立帧在中间那一列**（`_pattern` 的初值就是 1），而引擎的 `DirPattern` 把 `step` 0
/// 当站立帧（`frame_index = row × cols + step % cols`，路线跑完 `set_step(0)`）。于是挂动画时
/// 必须按 `[1, 0, 2]` 重排列：`step` 0 → 中间列，`step` 1/2 → 左右两列，走路才是一脚一步。
const MV_COLUMNS: [u32; 3] = [1, 0, 2];

/// 第 `(col, row)` 格的**左下角**，即 `anchor = (0,1)` 精灵的 `pos`。
fn lattice(col: i32, row: i32) -> Vec2 {
    Vec2::new(col as f32 * TILE, (row + 1) as f32 * TILE)
}

/// 图块号 → `Shell.png` 里的子矩形。本 fixture 的约定是 `cell = (id - 1) % 32`、
/// 8 列一行（`gen.py` 的"美术约定"一节）。号 0 = 无图块，不画。
fn tile_rect(id: u32) -> Rect {
    let cell = (id - 1) % 32;
    Rect::new(
        (cell % 8) as f32 * TILE,
        (cell / 8) as f32 * TILE,
        TILE,
        TILE,
    )
}

/// 行走图第 `(row, col)` 格的子矩形（一格 48×48，图是 3 列 × 4 行）。
fn char_rect(col: u32, row: u32) -> Rect {
    Rect::new(col as f32 * TILE, row as f32 * TILE, TILE, TILE)
}

/// MV 行走图的帧表：`DirPattern::RpgMaker4x3` 给的是"第 0 列 = 站立"的行优先 4×3，
/// 而 MV 的九宫格把**站立帧放在中间列**（`pattern=1` 才是站立帧，`Game_Character` 的
/// `_pattern` 为 0 时取的是 `bitmapWidth/3 * 1`）。所以列序按 [`MV_COLUMNS`] 重排一次。
///
/// `dur_ms = 0`：这不是时间驱动的动画，步帧由 [`chase`] 按"每走完一格换一帧"推进 ——
/// 与引擎 `MoveRoute` 的节奏一致（`task.rs` 的 `step_frame`）。
fn mv_walk_def() -> AnimDef {
    let pattern = DirPattern::RpgMaker4x3;
    let mut def = AnimDef::grid_pattern(pattern, TILE, TILE, 0);
    let cols = pattern.cols() as usize;
    for (i, f) in def.frames.iter_mut().enumerate() {
        f.rect = char_rect(MV_COLUMNS[i % cols], (i / cols) as u32);
    }
    debug_assert_eq!(def.frames.len(), pattern.cell_count() as usize);
    def
}

/// 某个事件页要画的东西。`None` 在 MV 里是"这一页存在但什么都不画"，不是"这一页不存在"。
#[derive(Clone, Debug, PartialEq)]
enum PageArt {
    None,
    Tile(u32),
    Char(String),
}

/// 页 → 画法 + `priorityType`（场景层只给页数据，摆哪儿是宿主的判断）。
fn page_art(page: Option<&EventPage>) -> PageArt {
    let Some(p) = page else {
        return PageArt::None;
    };
    if let Some(id) = p.tile_id() {
        return PageArt::Tile(id);
    }
    if p.image.character_name.is_empty() {
        return PageArt::None;
    }
    PageArt::Char(p.image.character_name.clone())
}

fn z_of(prio: u8, foot_y: f32) -> i32 {
    match prio {
        0 => Z_BELOW_CHAR,
        2 => Z_ABOVE_CHAR,
        _ => Z_CHAR + foot_y as i32,
    }
}

/// 命名动作 → 朝向。`rpgmk::action_map()` 与 MV 的 `keyMapping` 同源，四个名字是 `up/down/
/// left/right`（`docs/input.md`：示例代码里一个裸键名都不要有）。
fn dir_of_action(name: &str) -> Option<u8> {
    match name {
        "up" => Some(DIR_UP),
        "down" => Some(DIR_DOWN),
        "left" => Some(DIR_LEFT),
        "right" => Some(DIR_RIGHT),
        _ => None,
    }
}

/// 走完一格换步帧：与引擎 `MoveRoute` 的 `0 → 1 → 2 → 1 → 2 …` 同一条序列（站立帧只在停住
/// 或换向时出现）。两边用同一个节奏，玩家与事件才不会走出一快一慢的动画。
fn next_step(step: u8) -> u8 {
    match step {
        0 | 2 => 1,
        _ => 2,
    }
}

/// 精灵朝自己的格子挪最多 `px` 个像素（轴向，MV 的移动从来是横平竖直的）。
/// 返回 `true` = 这一格到位了。**到位**这个信号就是本示例的 `isMoving()`：玩家只在到位的
/// 那一拍才吃新输入，否则按住方向会变成每拍改一次目标格。
fn chase(sprite: &mut Sprite, target: Vec2, px: f32, walk_anime: bool) -> bool {
    let dx = target.x - sprite.pos.x;
    let dy = target.y - sprite.pos.y;
    if dx == 0.0 && dy == 0.0 {
        if sprite.step().unwrap_or(0) != 0 {
            sprite.set_step(0);
        }
        return true;
    }
    // 主轴优先：两个方向同时非零只发生在换向的那一拍，取绝对值大的那个。
    let dir = if dx.abs() >= dy.abs() {
        if dx > 0.0 { DIR_RIGHT } else { DIR_LEFT }
    } else if dy > 0.0 {
        DIR_DOWN
    } else {
        DIR_UP
    };
    let unit = dir_vector(dir);
    let left = if dx.abs() >= dy.abs() {
        dx.abs()
    } else {
        dy.abs()
    };
    let step = px.min(left);
    sprite.set_pos(sprite.pos + unit * step);
    let arrived = px >= left;
    if arrived {
        sprite.teleport(target);
        // 步帧按"格"推进：引擎的 `MoveRoute` 也是走完一格才 `step_frame`（`task.rs`），
        // 两边同节奏，玩家与事件才不会走出一快一慢的动画。`walk_anime` 关掉就站住。
        let step = sprite.step().unwrap_or(0);
        sprite.set_step(if walk_anime { next_step(step) } else { 0 });
    }
    arrived
}

// ---------------------------------------------------------------- 资产

/// wasm 的两份清单（两个 URL 前缀，见文件头）。
#[cfg(target_arch = "wasm32")]
const FONT_MANIFEST: &str = "Inter-Regular.ttf\n";
/// 工程清单：键名必须**就是** MV 的相对路径，因为 `MapDef::load` 拼的是 `data/Map001.json`。
/// 前缀 `assets/mv/` 只出现在 fetch 的 URL 上。
#[cfg(target_arch = "wasm32")]
const MV_MANIFEST: &str = "data/System.json\ndata/Map001.json\ndata/Tilesets.json\n\
                           graphics/tilesets/Shell.png\ngraphics/characters/Actor1.png\n\
                           graphics/characters/Actor2.png\naudio/bgm/Field01.ogg\n\
                           audio/se/Confirm01.ogg\naudio/me/Battle01.ogg\n";

struct Assets {
    vfs: Vfs,
    /// 最近一次读不到的键（wasm 首帧清单还在飞，这句话是"再等一拍"的依据）。
    missing: String,
    font_loaded: bool,
}

impl Assets {
    fn new() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let mut vfs = Vfs::new();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
            vfs.register(Box::new(DirProvider::new(
                root.join("../../examples/assets/mv"),
            )));
            vfs.register(Box::new(DirProvider::new(
                root.join("../../crates/sisf-text/tests/fixtures"),
            )));
        }
        #[cfg(target_arch = "wasm32")]
        let vfs = {
            let mut vfs = Vfs::new();
            // 两份清单两个 provider：VFS 按注册序取第一个命中的，键空间又不重叠（工程的键
            // 全带 `data/`、`graphics/`、`audio/` 前缀，字体是裸文件名），所以并排放就够。
            let mv = Arc::new(WebManifest::new(MV_MANIFEST));
            prefetch(&mv, "assets/mv/");
            vfs.register(Box::new(SharedManifest(mv)));
            let fonts = Arc::new(WebManifest::new(FONT_MANIFEST));
            prefetch(&fonts, "assets/");
            vfs.register(Box::new(SharedManifest(fonts)));
            vfs
        };
        Self {
            vfs,
            missing: String::from("未尝试"),
            font_loaded: false,
        }
    }

    /// 一组键是否都到货。缺哪个就写进 [`Assets::missing`] 并返回 `false`（装载因此是
    /// "整批再来一拍"而不是"半张地图先摆上去"）。
    fn ready(&mut self, keys: &[String]) -> bool {
        for k in keys {
            if !self.vfs.exists(&vpath(k)) {
                self.missing = k.clone();
                return false;
            }
        }
        true
    }
}

/// wasm 宿主预取：按清单逐个 `fetch` 到 `prefix + key`，成字节后灌进 `WebManifest`
/// （VFS 读取因此恒同步）。前缀是参数而不是常量，因为本示例要跑两遍：工程与字体不同源。
#[cfg(target_arch = "wasm32")]
fn prefetch(manifest: &Arc<WebManifest>, prefix: &str) {
    use sisf::app::{js_sys, wasm_bindgen_futures, web_sys};
    for p in manifest.pending() {
        let Some(window) = web_sys::window() else {
            break;
        };
        let m = Arc::clone(manifest);
        let key = p.as_str().to_owned();
        let url = format!("{prefix}{key}");
        wasm_bindgen_futures::spawn_local(async move {
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

/// 装箱好的位图：图块集一张 + 若干行走图。
///
/// `Bitmap` 句柄**必须持有**：句析构 = 页退役（`PageRetired`），下一拍 `apply_atlas` 就把那页
/// 删了，引用它的精灵当场变黑块。所以 `Art` 是这些位图的所有者，`Sprite.bitmap` 只是共享。
struct Art {
    atlas: Atlas,
    pages: AtlasPages,
    uploads: UploadRing,
    /// 装箱/采样时基（LRU）。
    frame: u64,
    tiles: Option<Bitmap>,
    chars: BTreeMap<String, Bitmap>,
}

impl Art {
    fn new() -> Self {
        Self {
            // 缺省桶（`with_limits(2048, 8)`）：图块图 384×192、行走图 144×192，各自一页。
            atlas: Atlas::new(),
            pages: AtlasPages::new(),
            uploads: UploadRing::new(),
            frame: 0,
            tiles: None,
            chars: BTreeMap::new(),
        }
    }

    /// 读一个 PNG、解码、装箱、排上传。**主线程解**：一次性三份图，不值得为它铺
    /// `AssetStore` 的异步管线（`sprite_bench` 那条是给十万张图准备的）。
    fn load(&mut self, vfs: &Vfs, key: &str) -> Option<Bitmap> {
        let bytes = vfs.read(&vpath(key))?;
        if bytes.is_empty() {
            return None;
        }
        self.frame += 1;
        let data = sisf::media::image::decode_image(&bytes)
            .unwrap_or_else(|e| panic!("{key} 解码失败: {e}"));
        let img = data.first();
        let bitmap = self
            .atlas
            .new_bitmap(img.width, img.height, img.pixels.clone(), self.frame)
            .unwrap_or_else(|e| panic!("{key} 装箱失败: {e}"));
        self.uploads.queue(bitmap.clone());
        platform::log_info(&format!(
            "[rpgmv_shell] {key} {}x{} → epoch {}",
            img.width,
            img.height,
            bitmap.epoch()
        ));
        Some(bitmap)
    }

    fn tileset(&mut self, vfs: &Vfs, name: &str) -> Option<Bitmap> {
        if let Some(b) = self.tiles.as_ref() {
            return Some(b.clone());
        }
        let key = format!("graphics/tilesets/{name}.png");
        let b = self.load(vfs, &key)?;
        self.tiles = Some(b.clone());
        Some(b)
    }

    fn character(&mut self, vfs: &Vfs, name: &str) -> Option<Bitmap> {
        if let Some(b) = self.chars.get(name) {
            return Some(b.clone());
        }
        let key = format!("graphics/characters/{name}.png");
        let b = self.load(vfs, &key)?;
        self.chars.insert(name.to_owned(), b.clone());
        Some(b)
    }

    /// 刷新本帧真正画到的位图的 LRU 时基，并把新页交给渲染器。
    fn touch(&self) {
        if let Some(b) = self.tiles.as_ref() {
            self.atlas.touch(b.epoch(), self.frame);
        }
        for b in self.chars.values() {
            self.atlas.touch(b.epoch(), self.frame);
        }
    }
}

// ---------------------------------------------------------------- 两个端口

/// [`AudioPort`]：`AudioBus` + 一份样本缓存。
///
/// 这一层的唯一义务是"**同一名字只解码一次**"。MV 工程里 `Field01` 会被地图 BGM、战斗、
/// 存档画面各放一次，而 `.ogg` 解码是几十毫秒级的活 —— 不复用就是每次起播卡一拍。
struct GameAudio {
    bus: AudioBus,
    samples: BTreeMap<String, Arc<AudioData>>,
    /// 一拍的毫秒数：`Fade` 写的是拍，混音器要毫秒。
    beat_ms: f32,
    /// 最近一次落地的结论，进 HUD 第二行（"听不见"必须能说成原因）。
    status: String,
}

impl GameAudio {
    fn new() -> Self {
        let mut bus = AudioBus::open();
        // fixture 是满幅正弦，压一档再听（与 `dialog_demo` 同一处理）。
        bus.set_master_gain(0.4);
        platform::log_info(&format!("[rpgmv_shell] 输出: {}", bus.describe()));
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
            "[rpgmv_shell] 解码 {name}: {} 帧 / {:.2}s（只此一次）",
            data.frames(),
            data.duration()
        ));
        let arc = Arc::new(data);
        self.samples.insert(name.to_owned(), Arc::clone(&arc));
        Ok(arc)
    }

    pub(crate) fn suspend(&mut self) {
        self.bus.suspend();
    }

    pub(crate) fn resume(&mut self) {
        self.bus.resume_all();
    }
}

impl AudioPort for GameAudio {
    fn play(&mut self, vfs: &Vfs, channel: SoundChannel, sound: &Sound) -> Result<(), String> {
        let data = self.sample(vfs, &sound.name)?;
        let reused = self.samples.contains_key(&sound.name);
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
            if reused { ", 复用样本" } else { "" }
        );
        Ok(())
    }

    fn fade(&mut self, target: FadeTarget, frames: u32, out: bool) -> Result<(), String> {
        let ms = frames as f32 * self.beat_ms;
        let to = if out { 0.0 } else { 1.0 };
        // `Screen` 的 `channels()` 是空的 ⇒ 一次都不碰混音器（黑幕归 `UiPort`）。
        for ch in target.channels() {
            self.bus.fade_to(mixer_channel(*ch), to, ms);
        }
        self.status = match target {
            FadeTarget::Screen => "黑幕淡入淡出（端口不碰）".to_owned(),
            // `FadeTarget` 没有 `label()`（只有 `SoundChannel` 有），而这里是状态行 ——
            // derive 的 `Debug` 就是 ASCII 的通道名，够用。
            _ => format!("{target:?} 淡向 {to:.1} / {ms:.0}ms"),
        };
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

/// [`UiPort`]：只存事件机推来的东西，画在 `draw` 里。
///
/// 契约是"UI 状态是事件机状态的函数，只在变化时推"，所以这里没有任何自己的显示逻辑。
#[derive(Default)]
struct Window {
    prompt: Option<Prompt>,
    dim: f32,
    /// `show`/`hide` 的总次数（HUD 用它证明"内容没变就没重复推"）。
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

// ---------------------------------------------------------------- 脚本口子

/// `Command::Custom` 的落地处：MV 有内容、引擎没有通路的那几条，在这里**说话而不是沉默**。
///
/// 四条话题全是本 fixture 真会走到的：
/// - `rpgmv.movie`（`261`）→ 视频桩，见文件头；
/// - `rpgmv.transfer`（`201`）→ 换图。本示例只装一张图，且 `MapDef::load` 要的图号在
///   参数里，接起来只差"重新装载 + 淡入淡出"，但那已经是场景管理，不属于 T11；
/// - `rpgmv.unimplemented`（任何有 handler 却没建模的码，本 fixture 里是 `251` 停止 SE）；
/// - `rpgmv.face`（`101` 的脸部图）→ 引擎的消息窗没有立绘槽。
///
/// 返回 `None` 的名字 = "这个名字我不认" ⇒ 虚拟机会替它登记一条告警（`take_warnings`），
/// 所以这里不自己数。
struct ShellBackend {
    video: VideoPlayer,
    /// 最近一次落地的话题，HUD 第三行读它。后端被 `EventRunner` 独占，所以共享一个把手。
    status: Rc<RefCell<String>>,
}

impl ShellBackend {
    fn new(status: Rc<RefCell<String>>) -> Self {
        Self {
            video: VideoPlayer::new(),
            status,
        }
    }

    /// `261` 的参数 0 是影片名（MV 的 `movieFileName`）。
    fn text_arg(args: &[Param]) -> String {
        args.iter()
            .find_map(|p| match p {
                Param::Text(t) if !t.is_empty() => Some(t.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "(无名)".to_owned())
    }

    /// 整数参数列表（`201` 的六个数）；非整数一律跳过，因为这条只为了把话说清楚。
    fn ints(args: &[Param]) -> Vec<i64> {
        args.iter()
            .filter_map(|p| match p {
                Param::Int(n) => Some(*n),
                _ => None,
            })
            .collect()
    }
}

impl ScriptBackend for ShellBackend {
    fn start(&mut self, _entry: &str) {}

    fn tick(&mut self, _ctx: &mut EventCtx, _queue: &mut Vec<Submission>) {}

    fn command(&mut self, name: &str, args: &[Param]) -> Option<Arc<Program>> {
        let done = |what: String| {
            *self.status.borrow_mut() = what;
            Some(Arc::new(Program::default()))
        };
        match name {
            "rpgmv.movie" => {
                let movie = Self::text_arg(args);
                // 只 warn 一次：同一张片头在移植工程里会被反复触发（重开场景、存档回退），
                // 每拍刷一条日志等于没有日志。闩锁在播放器上，文案与 media 层同源。
                let Some(note) = self.video.skip_notice() else {
                    return done(format!("movie {movie} skipped again"));
                };
                // 顺手问一次"那到底开不开得起来" —— 答案必须与 `note` 是同一句话，
                // 否则文档与运行期文案就漂了。
                let why = VideoPlayer::open(movie.as_bytes())
                    .err()
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| note.to_owned());
                platform::log_info(&format!("[rpgmv_shell] 跳过影片 `{movie}`：{why}"));
                done(format!("movie {movie} skipped"))
            }
            "rpgmv.transfer" => {
                let n = Self::ints(args);
                platform::log_info(&format!(
                    "[rpgmv_shell] 201 转移本示例不接（只装一张图）：{n:?}"
                ));
                done("transfer ignored".into())
            }
            "rpgmv.unimplemented" => {
                // 第一个参数就是 MV 的指令码，后面是它原本的参数。
                let code = match args.first() {
                    Some(Param::Int(c)) => *c,
                    _ => -1,
                };
                platform::log_info(&format!(
                    "[rpgmv_shell] MV 指令 {code} 没建模，参数 {args:?}"
                ));
                done(format!("mv {code} unimplemented"))
            }
            "rpgmv.face" => {
                let face = Self::text_arg(args);
                done(format!("face {face} ignored"))
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------- 示例本体

/// 一个事件的"画法缓存"：换页才重建动画与位图。
struct View {
    sprite: SpriteRef,
    /// 上一次画的是哪一页。比对页号而不是比对 [`PageArt`]：两页可以画法完全相同却各有自己的
    /// `walk_anime`/`priorityType`，只比画法会把"换了页"漏成"什么都没变"。
    page: Option<usize>,
}

/// 一拍开头从场景层抄下来的姿势表（格子是整数，精灵是像素，两边在这里对账）。
#[derive(Clone)]
struct Pose {
    id: u32,
    at: Vec2,
    dir: u8,
    page: Option<usize>,
    art: PageArt,
    prio: u8,
    speed: f32,
    walk_anime: bool,
    visible: bool,
}

struct RpgmvShell {
    assets: Assets,
    art: Art,
    text: TextEngine,
    arena: SpriteArena,
    tasks: TaskScheduler,
    scene: Scene,
    /// 装好之后才有：`None` = 还在等清单里的 `data/*.json`。
    world: Option<Scene_Map>,
    vars: GameVars,
    bus: EngineBus,
    audio: GameAudio,
    ui: Window,
    vm: EventRunner,
    /// 事件号 → 编译后的脚本（`translate_event` + `EventDef::compile` 的产物）。
    scripts: BTreeMap<u32, Arc<EventScript>>,
    views: BTreeMap<u32, View>,
    /// 图块层摆好的那批精灵（一次性，不随页变化）。
    floor: Vec<SpriteRef>,
    backend_status: Rc<RefCell<String>>,
    /// 翻译账单（HUD 读 ASCII 版，日志读 `summary()` 的中文原句）。
    report: String,
    title: String,
    map_label: String,
    ticks: u64,
    /// 上一拍的结论（`idle/hold text/…`）。
    last: String,
    held: u8,
    /// 轻点 = 一步：wasm 的 `press_key` 把 keydown+keyup 送进同一批事件，`held` 在
    /// `player_tick` 跑到之前就被 Release 清掉；MV 里轻点方向本来就走一格。
    intent: Option<u8>,
    decide: bool,
    cam: Vec2,
    draw_alpha: f32,
    cam_pushed: bool,
    stats: Option<DrawStats>,
    phys: (u32, u32),
    warnings: u32,
    font: String,
    erased: u32,
}

impl RpgmvShell {
    fn new() -> Self {
        let status = Rc::new(RefCell::new(String::from("nothing yet")));
        let vm = EventRunner::new().with_script(Box::new(ShellBackend::new(Rc::clone(&status))));
        Self {
            assets: Assets::new(),
            art: Art::new(),
            text: TextEngine::new(FontBundle::system()),
            // 24×18 的三层地板 + 少量角色。
            arena: SpriteArena::with_capacity(24 * 18 * 3 + 32),
            tasks: TaskScheduler::new(),
            scene: Scene::grid(TILE),
            world: None,
            vars: GameVars::seeded(7),
            bus: EngineBus::new(),
            audio: GameAudio::new(),
            ui: Window::default(),
            vm,
            scripts: BTreeMap::new(),
            views: BTreeMap::new(),
            floor: Vec::new(),
            backend_status: status,
            report: String::from("未装载"),
            title: String::from("rpgmv_shell"),
            map_label: String::new(),
            ticks: 0,
            last: String::from("idle"),
            held: 0,
            intent: None,
            decide: false,
            cam: Vec2::ZERO,
            draw_alpha: 0.0,
            cam_pushed: false,
            stats: None,
            phys: (0, 0),
            warnings: 0,
            font: String::new(),
            erased: 0,
        }
    }

    // --- 装载 ---------------------------------------------------------------

    /// 进图：`data/*.json` → `SystemDef`/`MapDef`/`TilesetDef` → `Scene_Map` + 每事件一份
    /// `EventScript`。任何一份没到货就返回 `false`，下一拍整批再试（wasm 首帧清单还在飞）。
    ///
    /// 分两批 `ready` 是因为**图号与美术名都在数据里**：`data/Map{id:03}.json` 的 `id` 来自
    /// `System.json` 的 `startMapId`，图块图名来自 `Tilesets.json`，行走图名来自事件页。
    fn boot(&mut self) -> bool {
        if !self.assets.ready(&[
            "data/System.json".to_owned(),
            "data/Tilesets.json".to_owned(),
        ]) {
            return false;
        }
        let sys = match SystemDef::load(&self.assets.vfs) {
            Ok(s) => s,
            Err(e) => {
                platform::log_info(&format!("[rpgmv_shell] data/System.json 读不了: {e}"));
                return false;
            }
        };
        let map_key = format!("data/Map{:03}.json", sys.start_map_id);
        if !self.assets.ready(std::slice::from_ref(&map_key)) {
            return false;
        }
        let map = match MapDef::load(&self.assets.vfs, sys.start_map_id) {
            Ok(m) => m,
            Err(e) => {
                platform::log_info(&format!("[rpgmv_shell] {map_key} 读不了: {e}"));
                return false;
            }
        };
        let tileset = match TilesetDef::load(&self.assets.vfs, map.tileset_id) {
            Ok(t) => t,
            Err(e) => {
                platform::log_info(&format!(
                    "[rpgmv_shell] 图块组 {} 读不了: {e}",
                    map.tileset_id
                ));
                return false;
            }
        };

        // 事件页要的行走图（含玩家那一张）。
        let mut sheets: Vec<String> = vec![format!(
            "graphics/tilesets/{}.png",
            tileset.tileset_name(TILE_SHEET)
        )];
        for id in map.event_ids() {
            let Some(ev) = map.event(id) else {
                continue;
            };
            for p in &ev.pages {
                if !p.image.character_name.is_empty() {
                    let k = format!("graphics/characters/{}.png", p.image.character_name);
                    if !sheets.contains(&k) {
                        sheets.push(k);
                    }
                }
            }
        }
        let player_sheet = format!("graphics/characters/{PLAYER_CHARACTER}.png");
        if !sheets.contains(&player_sheet) {
            sheets.push(player_sheet);
        }
        if !self.assets.ready(&sheets) {
            return false;
        }
        // 到货 ≠ 装箱。`paint` 直接查 `art.tiles` / `art.chars`，所以这里必须把每一份都
        // 解出来放进取用表；漏一张就是"启动时 panic 在 expect 上"，而不是"这一格不画"。
        // 第一份约定是图块集（`build_floor` 与图块页都读 `art.tiles`），其余是行走图。
        for (i, key) in sheets.iter().enumerate() {
            let name = key
                .trim_start_matches("graphics/tilesets/")
                .trim_start_matches("graphics/characters/")
                .trim_end_matches(".png");
            if i == 0 {
                self.art.tileset(&self.assets.vfs, name);
            } else {
                self.art.character(&self.assets.vfs, name);
            }
        }

        // 三张 JSON + 全部美术齐了 ⇒ 可以建世界。
        let world = Scene_Map::enter(
            sys.start_map_id,
            map,
            tileset,
            (sys.start_x, sys.start_y, sys.start_direction),
            &self.vars,
        );
        platform::log_info(&format!(
            "[rpgmv_shell] 进图 {}「{}」{}x{}，出生点 ({},{},{})",
            sys.start_map_id,
            world.map().display_name,
            world.map().width,
            world.map().height,
            sys.start_x,
            sys.start_y,
            sys.start_direction,
        ));
        self.translate_all(&world);
        self.title = sys.game_title.clone();
        self.map_label = format!(
            "{}x{} tileset {}",
            world.map().width,
            world.map().height,
            world.map().tileset_id
        );
        self.vars.bind_system(&sys);
        self.build_floor(&world);
        self.world = Some(world);
        // 出生点那一格先落到精灵上（第一帧就有画面，而不是"等一拍才出现"）。
        self.sync_views();
        self.snap_all();
        if self.world.as_ref().is_some_and(|w| w.map().autoplay_bgm) {
            let vfs = &self.assets.vfs;
            let bgm = self.world.as_ref().unwrap().map().bgm.sound("bgm");
            match bgm {
                Some(sound) => {
                    let _ = self.audio.play(vfs, SoundChannel::Bgm, &sound);
                }
                // 空名字在 MV 那边是 `stopBgm()`，而指令表里没有"停"这条 —— 说清楚比静默好。
                None => self.audio.status = "autoplayBgm 但没有 BGM 名".to_owned(),
            }
        }
        true
    }

    /// 每个事件：`translate_event` → `EventDef::compile`。账单合起来打一次。
    fn translate_all(&mut self, world: &Scene_Map) {
        let opts = Options {
            tick_hz: TICK_HZ as u32,
            tile_px: TILE,
        };
        let mut total = TranslateReport::default();
        for id in world.map().event_ids() {
            let Some(ev) = world.map().event(id) else {
                continue;
            };
            let (def, rep) = translate_event(ev, world.map_id(), opts);
            total.merge(&rep);
            match def.compile() {
                Ok(script) => {
                    let pages = script.pages.len();
                    let cmds = script.pages.iter().map(|p| p.code.len()).sum::<usize>();
                    platform::log_info(&format!(
                        "[rpgmv_shell] 事件 {id}：页 {pages} 指令 {cmds}（{}）",
                        def.name
                    ));
                    self.scripts.insert(id, Arc::new(script));
                }
                // 编不过是**作者错误**（页号/槽越界这类），必须留下话；这张图其余事件照装。
                Err(e) => platform::log_info(&format!("[rpgmv_shell] 事件 {id} 编不过: {e}")),
            }
        }
        // 计数打印是清单要的验收：中文原句进日志，ASCII 摘要进 HUD（wasm 没中文字形时
        // 这一行仍要读得出来）。
        platform::log_info(&format!("[rpgmv_shell] 翻译账单: {}", total.summary()));
        self.report = ascii_summary(&total);
        // 同一条账的第二种说法：未覆盖的码第一次要 warn（进程级闩锁，见 `warn_new`）。
        let warned = total.warn_new();
        platform::log_info(&format!("[rpgmv_shell] 首次降级 warn {warned} 条"));
    }

    /// 地板：`MapDef::tile(x, y, z)` 的三个层，每格至多一个精灵（同一格两层叠两层）。
    fn build_floor(&mut self, world: &Scene_Map) {
        let sheet = self
            .art
            .tileset(&self.assets.vfs, world.tileset().tileset_name(TILE_SHEET))
            .expect("boot 已经确认这张图到货");
        let (w, h) = (world.map().width, world.map().height);
        for y in 0..h {
            for x in 0..w {
                for (z, &floor_z) in Z_FLOOR.iter().enumerate() {
                    let tile = world.map().tile(x, y, z);
                    if tile == 0 {
                        continue;
                    }
                    let mut s = Sprite::new(sheet.clone());
                    s.src = tile_rect(tile);
                    s.anchor = Vec2::new(0.0, 1.0);
                    s.teleport(lattice(x as i32, y as i32));
                    s.z = floor_z;
                    s.layer = WORLD;
                    self.floor.push(self.arena.spawn(s));
                }
            }
        }
        platform::log_info(&format!(
            "[rpgmv_shell] 地板 {} 块（{} 层）",
            self.floor.len(),
            MapDef::TILE_LAYERS
        ));
    }

    // --- 每拍 ---------------------------------------------------------------

    /// 从场景层抄一张姿势表。格子与页归场景层，这里只读不写。
    fn poses(&self) -> Vec<Pose> {
        let Some(world) = self.world.as_ref() else {
            return Vec::new();
        };
        let opts = Options {
            tick_hz: TICK_HZ as u32,
            tile_px: TILE,
        };
        world
            .chars()
            .iter()
            .map(|c: &Game_Character| {
                let page = world.page(c.id);
                // 玩家没有事件页，`page_art(None)` 会把他判成"不画不追"—— 然后第一步
                // 之后 `arrived` 永远 false，输入整条卡死（wasm 复验实际踩到）。他的画法
                // 从 `System.json` 惯例的初始角色来，跟事件页无关。
                let (art, prio, walk_anime) = if c.is_player() {
                    (PageArt::Char(PLAYER_CHARACTER.to_owned()), 1, true)
                } else {
                    (
                        page_art(page),
                        page.map_or(1, |p| p.priority_type),
                        page.is_none_or(|p| p.walk_anime),
                    )
                };
                let speed = if c.is_player() {
                    mv_speed_px_per_tick(PLAYER_SPEED, opts)
                } else {
                    // 没页的角色不动；给个数只为别让 `chase` 拿到 0 之外还去换步帧。
                    page.map_or(0.0, |p| mv_speed_px_per_tick(i64::from(p.move_speed), opts))
                };
                Pose {
                    id: c.id,
                    at: lattice(c.x as i32, c.y as i32),
                    dir: c.dir,
                    art,
                    prio,
                    page: c.page,
                    speed,
                    walk_anime,
                    visible: c.visible(),
                }
            })
            .collect()
    }

    /// 出现 / 消失 / 换页：把姿势表变成精灵。
    ///
    /// 换页要换的不只是 `src`：图块页与角色页是**两个不同的位图**，而 MV 的"这一页是图块"
    /// 还会连带改变碰撞（见 `scene.rs` 的验收）。这里只画，判定已经在场景层做过了。
    fn sync_views(&mut self) {
        let poses = self.poses();
        for p in &poses {
            if !p.visible {
                if let Some(v) = self.views.remove(&p.id) {
                    self.scene.remove(p.id);
                    self.arena.despawn(v.sprite);
                }
                continue;
            }
            let sprite = self.view_for(p);
            // 位置/z/朝向每拍都刷（画法没变的页也要跟着格子走）。
            let s = self.arena.get_mut(sprite).expect("views 里的精灵是活的");
            s.z = z_of(p.prio, p.at.y);
            s.visible = !matches!(p.art, PageArt::None);
            s.set_dir(p.dir);
        }
    }

    /// 拿到这个角色这一页该用的精灵：没有就 spawn，换页就重画。返回句柄，为的是让
    /// [`Self::sync_views`] 后面那段"每拍刷位置"不必再回头查 `views`。
    fn view_for(&mut self, p: &Pose) -> SpriteRef {
        let known = self.views.get(&p.id).is_some_and(|v| v.page == p.page);
        if known {
            return self.views[&p.id].sprite;
        }
        let sprite = match self.views.get(&p.id) {
            Some(v) => v.sprite,
            None => {
                let fresh = self.arena.spawn(self.blank_sprite());
                self.scene.set(p.id, fresh);
                fresh
            }
        };
        self.views.insert(
            p.id,
            View {
                sprite,
                page: p.page,
            },
        );
        self.paint(sprite, p);
        sprite
    }

    fn blank_sprite(&self) -> Sprite {
        let mut s = Sprite::solid(Vec2::new(TILE, TILE));
        s.anchor = Vec2::new(0.0, 1.0);
        s.layer = WORLD;
        s
    }

    /// 按页的画法换图源：图块页 = 图块集的一个子矩形；角色页 = 行走图 + 方向图样动画。
    fn paint(&mut self, sprite: SpriteRef, p: &Pose) {
        let bitmap = match &p.art {
            PageArt::None => None,
            PageArt::Tile(_) => Some(self.art.tiles.clone().expect("boot 已经装箱了图块集")),
            PageArt::Char(name) => Some(
                self.art
                    .chars
                    .get(name)
                    .cloned()
                    .expect("boot 已经装箱了这张行走图"),
            ),
        };
        let s = self.arena.get_mut(sprite).expect("刚 spawn 的精灵");
        s.bitmap = bitmap;
        // `set_animation` 挂上即把当前帧写进 `src`，所以角色页不需要自己摆 `src`；
        // 图块页反过来 —— 它没有动画，`src` 就是那格图块。
        if matches!(p.art, PageArt::Char(_)) {
            s.set_animation(Some(AnimState::new(Arc::new(mv_walk_def()))));
        } else {
            s.set_animation(None);
            if let PageArt::Tile(id) = p.art {
                s.src = tile_rect(id);
            }
        }
    }

    /// 一次性将全部精灵钉到自己的格子上（进图那一拍用；之后靠 [`chase`]）。
    fn snap_all(&mut self) {
        let poses = self.poses();
        for p in &poses {
            if let Some(v) = self.views.get(&p.id)
                && let Some(s) = self.arena.get_mut(v.sprite)
            {
                s.teleport(p.at);
            }
        }
    }

    /// 精灵到不到位（= MV 的 `isMoving()` 反义）。
    fn arrived(&self, id: u32) -> bool {
        let Some(world) = self.world.as_ref() else {
            return false;
        };
        let Some(c) = world.get(id) else {
            return false;
        };
        let Some(v) = self.views.get(&id) else {
            return false;
        };
        let Some(s) = self.arena.get(v.sprite) else {
            return false;
        };
        (s.pos - lattice(c.x as i32, c.y as i32)).length() < 0.001
    }

    /// 某个事件**当前页**的触发方式（`None` = 这一拍它不存在：页条件不成立或已擦除）。
    ///
    /// 存在的理由是把借用剪短：`Scene_Map::page` 给出的是借出的 `&EventPage`，而下面四个
    /// `*_tick` 在循环里还要 `self.start_event(..)`（要 `&mut self`）。一次只把一个 `u8`
    /// 拷出来，编译器就不用在一整张场景图和一次调用之间为难谁。
    fn trigger_of(&self, id: u32) -> Option<u8> {
        self.world.as_ref()?.page(id).map(|p| p.trigger)
    }

    /// 玩家这一拍的意图：只在**已经到位**且事件机没占着世界时才吃输入。
    fn player_tick(&mut self) {
        if !self.vm.is_idle() || !self.arrived(PLAYER) {
            return;
        }
        if self.decide {
            // `trigger` 0（决定键）：查面前那一格。
            let ids = self
                .world
                .as_ref()
                .and_then(|world| world.front(PLAYER).map(|(x, y)| world.events_at(x, y)))
                .unwrap_or_default();
            for id in ids {
                if self.trigger_of(id) == Some(0) && self.start_event(id) {
                    return;
                }
            }
            return;
        }
        // `intent.take()` 放在最前：轻点的一步宁可对着墙原地转向（MV 同款），也不许
        // 挂到下一拍补走 —— 起事件的那一拍 `is_idle` 已经是 false 了。
        let dir = self.intent.take().unwrap_or(self.held);
        if dir != 0
            && let Some(world) = self.world.as_mut()
        {
            let ok = world.move_straight(PLAYER, dir);
            let (x, y) = world.player().pos();
            // 一步一条（到位才移动），给桌面端"走动"留可复读的证据；被挡的那条是转向。
            platform::log_info(&format!(
                "[rpgmv_shell] 玩家 {} ({x},{y})",
                if ok { "→" } else { " Blocked" }
            ));
        }
    }

    /// `trigger` 1（玩家接触）：玩家**到位**到某一格之后，看那一格上有没有它。
    fn touch_tick(&mut self) {
        if !self.vm.is_idle() || !self.arrived(PLAYER) {
            return;
        }
        let ids = match self.world.as_ref() {
            Some(world) => {
                let (x, y) = world.player().pos();
                world.events_at(x, y)
            }
            None => return,
        };
        for id in ids {
            if self.trigger_of(id) == Some(1) && self.start_event(id) {
                return;
            }
        }
    }

    /// `trigger` 2（事件接触）：事件**挪到玩家那一格**上时触发。与 1 的差别只是"谁在动"，
    /// 所以两条各查各的时机（1 在玩家到位那拍，2 在场景层走过一格之后）。
    fn event_touch_tick(&mut self) {
        if !self.vm.is_idle() {
            return;
        }
        let ids = match self.world.as_ref() {
            Some(world) => {
                let (x, y) = world.player().pos();
                world.events_at(x, y)
            }
            None => return,
        };
        for id in ids {
            if self.trigger_of(id) == Some(2) && self.start_event(id) {
                return;
            }
        }
    }

    /// `trigger` 3（自动执行）：没有挂起就起一个。同拍多个只起第一个 —— 挂起点是单份的。
    fn auto_tick(&mut self) {
        if !self.vm.is_idle() {
            return;
        }
        let ids: Vec<u32> = match self.world.as_ref() {
            Some(world) => world
                .chars()
                .iter()
                .skip(1)
                .filter(|c| c.visible())
                .map(|c| c.id)
                .collect(),
            None => return,
        };
        for id in ids {
            if self.trigger_of(id) == Some(3) && self.start_event(id) {
                return;
            }
        }
    }

    /// 起一个事件的**当前页**。页号不在这儿算：它由 `Scene_Map::refresh` 按页条件定，
    /// 这正是"哪一页此刻该出现"归游戏侧、"演出中途翻页"归事件机的那条分工。
    fn start_event(&mut self, id: u32) -> bool {
        let Some(world) = self.world.as_ref() else {
            return false;
        };
        let Some(page) = world.get(id).and_then(|c| c.page) else {
            return false;
        };
        let Some(script) = self.scripts.get(&id).cloned() else {
            return false;
        };
        let name = script.name.clone();
        let ok = self.vm.start(id, script, page);
        if ok {
            self.last = format!("event {id} page {page} ({name})");
            platform::log_info(&format!(
                "[rpgmv_shell] 事件 {id} 启动（页 {page}，{name}）"
            ));
        }
        ok
    }

    /// 虚拟机擦掉的事件：`Erase` 会 despawn 精灵并从 `Scene` 摘掉，但格子状态还在
    /// `Scene_Map` 里 —— 对账就在这儿做，否则它会"下一拍又站着画出来"。
    fn reconcile_erase(&mut self) {
        let dead: Vec<u32> = self
            .views
            .iter()
            .filter(|(_, v)| !self.arena.is_alive(v.sprite))
            .map(|(id, _)| *id)
            .collect();
        if dead.is_empty() {
            return;
        }
        for id in dead {
            self.views.remove(&id);
            if let Some(world) = self.world.as_mut() {
                world.erase(id);
            }
            self.erased += 1;
            platform::log_info(&format!(
                "[rpgmv_shell] 事件 {id} 被 214 擦除（本图内不再出现）"
            ));
        }
    }

    /// 逐拍像素追赶。位移单位是"像素/拍"，来自该页的 `moveSpeed`（MV 的
    /// `2^speed/256` 个图块/帧），换算走 `mv_speed_px_per_tick` 而不是自己开一张表。
    fn chase_all(&mut self) {
        let poses = self.poses();
        for p in &poses {
            if !p.visible || matches!(p.art, PageArt::None) {
                continue;
            }
            let Some(v) = self.views.get(&p.id) else {
                continue;
            };
            let Some(s) = self.arena.get_mut(v.sprite) else {
                continue;
            };
            let _ = chase(s, p.at, p.speed, p.walk_anime);
        }
    }

    /// 事件机跑过之后有没有人改了状态。`executed` 只说明"做了事"，改了开关才要重算页，
    /// 而 `GameVars::store_mut()` 本身就是"可能改了"的记号 —— 所以每拍把旗立起来，
    /// 由下一拍开头的 `world.tick` 消费（MV 的 `requestRefresh` 也是延后一拍）。
    fn run_vm(&mut self) -> RunnerStatus {
        let Self {
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
        let mut ctx = EventCtx {
            sprites: arena,
            tasks,
            scene,
            audio,
            ui,
            vars: vars.store_mut(),
            bus,
            vfs: &assets.vfs,
        };
        vm.tick(&mut ctx, BUDGET)
    }

    fn report_warnings(&mut self) {
        let warns = self.vm.take_warnings();
        self.warnings += warns.len() as u32;
        for w in warns {
            platform::log_info(&format!("[rpgmv_shell] 告警: {w}"));
        }
    }

    /// 自带字体到货后换上（wasm 没有系统字体）。中文不在这里：定案是"字形来源归宿主"，
    /// 那条 CDN 通路 `dialog_demo` 已经验过，本示例只证明**工程**装得起来。
    fn load_font(&mut self) {
        if self.assets.font_loaded {
            return;
        }
        let Some(bytes) = self.assets.vfs.read(&vpath("Inter-Regular.ttf")) else {
            return;
        };
        let mut bundle = FontBundle::system();
        let mut named = Vec::new();
        match bundle.register_bytes(&bytes) {
            Ok(family) => named.push(family),
            Err(why) => platform::log_info(&format!("[rpgmv_shell] 自带字体用不了: {why}")),
        }
        self.font = named.join("+");
        platform::log_info(&format!(
            "[rpgmv_shell] 字体 {}（face {}）",
            self.font,
            bundle.face_count()
        ));
        *self.text.bundle_mut() = bundle;
        self.assets.font_loaded = true;
    }

    /// 相机：把玩家视觉中心带到视口中心，再夹在地图里。
    ///
    /// 地图尺寸从 `MapDef` 的格数算，不写死 —— fixture 改图大小之后相机才不会跑出世界。
    fn camera_for(&self, foot: Vec2) -> Vec2 {
        let (vw, vh) = (VIRTUAL.0 as f32, VIRTUAL.1 as f32);
        let (ww, wh) = self
            .world
            .as_ref()
            .map(|w| (w.map().width as f32 * TILE, w.map().height as f32 * TILE))
            .unwrap_or((vw, vh));
        Vec2::new(
            (foot.x + TILE / 2.0 - vw / 2.0).clamp(0.0, (ww - vw).max(0.0)),
            (foot.y - TILE / 2.0 - vh / 2.0).clamp(0.0, (wh - vh).max(0.0)),
        )
    }

    /// 把 `alpha` 折到设备像素点阵上（与 `tiled_map` 同一条理由）：玩家的每一拍位移
    /// `PLAYER_PX` 乘倍率之后必须是整数，否则整张地图跟着亚像素晃。
    fn lattice_alpha(alpha: f32, scale: f32) -> f32 {
        (alpha * PLAYER_PX * scale).round() / (PLAYER_PX * scale)
    }

    fn snap_to_device(v: Vec2, scale: f32) -> Vec2 {
        Vec2::new((v.x * scale).round() / scale, (v.y * scale).round() / scale)
    }

    fn push_camera(&mut self, ctx: &mut FrameCtx) {
        if self.cam_pushed {
            ctx.pop_viewport();
            ctx.pop_viewport();
        }
        let (vw, vh) = (VIRTUAL.0 as f32, VIRTUAL.1 as f32);
        let world = ctx.push_viewport(Viewport::at(-self.cam.x, -self.cam.y, vw, vh));
        debug_assert_eq!(world, WORLD.0, "世界层号与 `WORLD` 的约定不符");
        let ui = ctx.push_viewport(Viewport::at(self.cam.x, self.cam.y, vw, vh));
        debug_assert_eq!(ui, UI_LAYER, "UI 层号与 `UI_LAYER` 的约定不符");
        self.cam_pushed = true;
    }

    /// HUD 三行。**全 ASCII**：这一行最需要出现的那一刻，正是中文还是方块的那一刻
    /// （wasm 没取 CJK），而"现在挂在什么上面"是复验唯一要读的东西。台词本身是中文（内容）。
    fn hud_lines(&self) -> [String; 3] {
        let player_cell = self
            .world
            .as_ref()
            .map(|w| (w.player().x, w.player().y))
            .unwrap_or((0, 0));
        let (batches, instances) = self.stats.map_or((0, 0), |s| (s.batches, s.instances));
        let spr = self
            .views
            .get(&PLAYER)
            .and_then(|v| self.arena.get(v.sprite))
            .map(|s| (s.pos.x, s.pos.y, s.step().unwrap_or(0)));
        let spr = spr.map_or_else(String::new, |(x, y, st)| {
            format!(" · spr {x:.1},{y:.1} f{st}")
        });
        [
            format!(
                "tick {} · {} · {} · pos {},{} · script {}{}",
                self.ticks,
                self.last,
                self.vm_prompt_kind(),
                player_cell.0,
                player_cell.1,
                self.backend_status.borrow(),
                spr,
            ),
            format!(
                "batches {batches} quads {instances} views {} pages {} · erased {} · warn {}",
                self.views.len(),
                self.art.atlas.live_pages(),
                self.erased,
                self.warnings,
            ),
            format!(
                "{} · sw3/4 {}{} · var1 {} · {}",
                self.audio.status,
                self.vars.switch_value(3) as u8,
                self.vars.switch_value(4) as u8,
                self.vars.value(1),
                if self.world.is_some() {
                    self.report.clone()
                } else {
                    format!("loading mv… missing {}", self.assets.missing)
                },
            ),
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

/// `TranslateReport::summary()` 的 ASCII 版：HUD 那一行在 wasm 上也要读得出来。
fn ascii_summary(rep: &TranslateReport) -> String {
    if rep.unimplemented.is_empty() && rep.route_steps_dropped == 0 {
        return format!("translate {} cmds ok", rep.commands);
    }
    let codes: Vec<String> = rep
        .unimplemented
        .iter()
        .map(|(c, n)| format!("{c}x{n}"))
        .collect();
    format!(
        "translate {} cmds, down[{}] drop {} lines {} steps",
        rep.commands,
        codes.join(" "),
        rep.dropped_branch_lines,
        rep.route_steps_dropped,
    )
}

impl App for RpgmvShell {
    /// 切片原样转进自己的总线（事件机是它唯一的读者），另外读游戏侧要用的边沿。
    ///
    /// 挂起/恢复要转发给音频（wasm 页面隐藏 = `Suspended`），否则切回标签页时
    /// 听见的是一段已经跑完的 BGM 的尾巴。
    fn on_event(&mut self, events: &[EngineEvent], _ctx: &mut FrameCtx) {
        for e in events {
            self.bus.push(e.clone());
            match e {
                EngineEvent::Action { name, edge } => {
                    if let Some(dir) = dir_of_action(name) {
                        match edge {
                            ActionEdge::Press => {
                                self.held = dir;
                                self.intent = Some(dir);
                            }
                            ActionEdge::Hold => self.held = dir,
                            // 换向之后再收到的 Release 不许把新方向也清掉。
                            ActionEdge::Release => {
                                if self.held == dir {
                                    self.held = 0;
                                }
                            }
                        }
                    } else if *edge == ActionEdge::Press && name == "ok" {
                        self.decide = true;
                    }
                }
                EngineEvent::Lifecycle(Lifecycle::Suspended) => self.audio.suspend(),
                EngineEvent::Lifecycle(Lifecycle::Resumed) => self.audio.resume(),
                _ => {}
            }
        }
    }

    fn tick(&mut self, _dt: Duration, ctx: &mut FrameCtx) {
        self.ticks += 1;
        // 每拍开头先收拢 `prev_pos`，且必须在任何移动之前（T8 第五条"抖"）。
        self.arena.begin_tick();
        self.text.set_raster_scale(ctx.raster_scale());
        if ctx.physical_size != self.phys {
            self.phys = ctx.physical_size;
            let v = ctx.viewport;
            platform::log_info(&format!(
                "[rpgmv_shell] 物理 {}x{} ⇒ 画面 ({:.0},{:.0}) {:.0}x{:.0}，倍率 {:.2}，光栅档 {:.0}",
                self.phys.0,
                self.phys.1,
                v.x,
                v.y,
                v.width,
                v.height,
                ctx.scale,
                ctx.raster_scale()
            ));
        }

        if self.world.is_none() && !self.boot() {
            return;
        }

        // 场景层这一拍：先重算页（上一拍有人改了开关），再让 `moveType` 1 的事件自己挪一格。
        let tick = if let Some(world) = self.world.as_mut() {
            world.tick(&mut self.vars)
        } else {
            Default::default()
        };
        if let Some(r) = tick.refreshed
            && r.changed > 0
        {
            platform::log_info(&format!(
                "[rpgmv_shell] refresh：换页 {}，隐没 {}，不支持的条件 {}（按页计）",
                r.changed, r.hidden, r.unsupported
            ));
        }
        // 事件接触要紧跟"事件挪过一格"之后查。
        if tick.moved > 0 {
            self.event_touch_tick();
        }

        // 顺序照 MV 的 `Game_Map.updateEvents`：自动执行 → 玩家接触 → 决定键。
        // 总线在事件机没等输入时每拍抽干，否则开对话的那一下 `ok` 会留在队列里，
        // 下一拍被 `ShowText` 的解挂逻辑吃掉 ⇒ 对话刚开就自己关了。
        if !self.vm.waiting_input() {
            let _ = self.bus.drain();
        }
        self.auto_tick();
        self.touch_tick();
        self.player_tick();
        self.decide = false;

        let status = self.run_vm();
        self.last = describe(status);
        if status.executed() > 0 {
            self.vars.request_refresh();
        }
        self.report_warnings();

        // 擦除对账要在换页同步之前：`Erase` 之后那张精灵已经不在 arena 里了。
        self.reconcile_erase();
        self.sync_views();
        self.chase_all();
    }

    /// 相机必须在 `prepare`：这一刻才拿得到本帧的 `alpha`，而下一行就要把投影栈烘进 `FrameU`。
    /// 取玩家**真正被画出来的那个点**（点阵化后的插值位置），两边同相位才不回拽。
    fn prepare(&mut self, alpha: f32, ctx: &mut FrameCtx) {
        let scale = ctx.scale;
        let a = Self::lattice_alpha(alpha, scale);
        self.draw_alpha = a;
        let foot = self
            .views
            .get(&PLAYER)
            .and_then(|v| self.arena.get(v.sprite))
            .map(|s| s.interpolated(a))
            .unwrap_or(Vec2::ZERO);
        let cam = self.camera_for(foot);
        self.cam = Self::snap_to_device(cam, scale);
        self.push_camera(ctx);
    }

    fn draw(&mut self, _alpha: f32, renderer: &mut Renderer) {
        self.load_font();
        let n = self.arena.len().max(1) as u32;
        if renderer.quads_per_slot() != n {
            renderer.set_quads_per_slot(n);
        }
        self.art.touch();
        renderer.apply_atlas(&self.art.atlas, &mut self.art.pages, &mut self.art.uploads);

        // 世界：地板 + 角色一次采集。层内按 (epoch, blend, z) 排序，所以三层地板与 y 排序
        // 是同一个批里的次序问题，不是多次提交。
        let mut list = BatchList::new();
        if self
            .arena
            .collect_at(WORLD_VIEW, self.draw_alpha, &mut list)
            .is_err()
        {
            platform::log_info("[rpgmv_shell] 采集失败（本示例不该有 tone 之外的东西）");
            return;
        }
        let (vw, vh) = (VIRTUAL.0 as f32, VIRTUAL.1 as f32);
        // 相机层得自己裁剪：letterbox 只改投影不改 scissor，平移出画面的那部分地图会照原样
        // 画进黑边里。裁剪盒用世界层自己的坐标。
        renderer.push_clip(u32::from(WORLD.0), [self.cam.x, self.cam.y, vw, vh]);
        renderer.append_list(list);
        renderer.pop_clip();

        // UI 层：黑幕 → 消息窗 → 窗底 → 高亮 → 文字。
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
        // 字体族定形之前一个字都不画：覆盖率图集的键带着 face 序号，中途换字体族会串字。
        if let Some(p) = self.ui.prompt.clone()
            && self.assets.font_loaded
        {
            quad(renderer, UI_LAYER, 1, WIN, WINDOW_BG);
            let style = TextStyle::new(15.0).aligned(Align::Left);
            if let Some(speaker) = p.speaker.as_deref() {
                self.text
                    .draw_text_at(renderer, NAME, speaker, &style, INK_DIM, UI_LAYER);
            }
            self.text
                .draw_text_at(renderer, BODY, &p.body, &style, INK, UI_LAYER);
            let rows: Vec<_> = p
                .choices
                .iter()
                .enumerate()
                .filter(|(i, _)| choice_rows_fit(i + 1))
                .collect();
            if !rows.is_empty() {
                let i = p.active.min(rows.len() - 1);
                quad(renderer, UI_LAYER, 2, choice_row(i), HIGHLIGHT);
            }
            for (i, choice) in rows {
                let mut r = choice_row(i);
                r[0] += 12.0;
                r[2] -= 12.0;
                self.text
                    .draw_text_at(renderer, r, choice, &style, INK, UI_LAYER);
            }
        }
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
        if self.ticks == 2 {
            platform::log_info(&format!(
                "[rpgmv_shell] 第二拍账单: {}",
                self.hud_lines()[1]
            ));
        }
        self.text.sync(renderer);
    }
}

// ---------------------------------------------------------------- 消息窗几何

const WIN: [f32; 4] = [48.0, 380.0, 720.0, 196.0];
const NAME: [f32; 4] = [64.0, 390.0, 688.0, 20.0];
const BODY: [f32; 4] = [64.0, 414.0, 688.0, 56.0];
const CHOICE_X: f32 = 80.0;
const CHOICE_Y: f32 = 478.0;
const CHOICE_H: f32 = 22.0;
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

/// 一块纯色矩形。
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

/// 上一拍的结论 → HUD 那一行短句（`reason` 是 ASCII 令牌，可 match）。
fn describe(status: RunnerStatus) -> String {
    match status {
        RunnerStatus::Idle { executed } => format!("idle {executed}"),
        RunnerStatus::Suspended { executed, reason } => format!("hold {reason} {executed}"),
        RunnerStatus::OverBudget { executed } => format!("over-budget {executed}"),
    }
}

/// `Command` 树里的自定义话题（给单测用：账单说降级了，画面上得真找得到那条）。
#[cfg(test)]
fn custom_topics(scripts: &BTreeMap<u32, Arc<EventScript>>) -> Vec<String> {
    let mut out = Vec::new();
    for s in scripts.values() {
        for page in &s.pages {
            for i in &page.code {
                if let sisf::event::Instr::Run(sisf::event::Command::Custom(name, _)) = i {
                    out.push(name.clone());
                }
            }
        }
    }
    out
}

fn config() -> AppConfig {
    AppConfig {
        title: "SISF rpgmv_shell (T11)".to_owned(),
        virtual_size: VIRTUAL,
        // 整数倍缩放：与"点阵折整不晃"那条同源（`lattice_alpha` 的台阶数按倍率算）。
        scale_mode: ScaleMode::IntegerFit,
        tick_hz: TICK_HZ as f64,
        clear: [0.03, 0.04, 0.07, 1.0],
        // MV 的 `keyMapping` 折成的就是这张表（`SystemDef::action_map()` 走同一条路，
        // 但 `AppConfig` 在装载 `System.json` 之前就要交出去）。
        actions: sisf::rpgmk::map::action_map(),
        ..Default::default()
    }
}

fn real_main() {
    platform::log_info("[rpgmv_shell] main 已启动");
    if let Err(e) = platform::run(RpgmvShell::new(), config()) {
        panic!("[rpgmv_shell] 运行失败: {e}");
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
// 例内能自动化的部分都在这里：**fixture 真能按 MV 的约定从 VFS 读出来**（键名/图号/字段名
// 写错就在这炸，而不是开局静默地黑屏）、每个事件翻得动并编得过、降级计数、`[1,0,2]` 列序、
// 图块号 → 子矩形、追赶到位的判据、消息窗装得下三行选项。
// 跑法 `cargo test -p sisf --example rpgmv_shell`。
#[cfg(test)]
mod tests {
    use super::*;

    fn vfs() -> Assets {
        Assets::new()
    }

    /// 验收（T11）：散文件的 MV 工程按 MV 的磁盘约定读得出来 —— 出生点在 `System.json` 而
    /// 不在地图里，图块组按 `id` 字段找而不按数组下标猜。
    #[test]
    fn the_project_on_disk_loads_through_the_vfs() {
        let mut a = vfs();
        assert!(
            a.ready(&[
                "data/System.json".to_owned(),
                "data/Map001.json".to_owned(),
                "data/Tilesets.json".to_owned()
            ]),
            "清单缺键：{}",
            a.missing
        );
        let sys = SystemDef::load(&a.vfs).expect("System.json");
        assert_eq!(sys.game_title, "SISF rpgmv_shell");
        assert_eq!(
            (
                sys.start_map_id,
                sys.start_x,
                sys.start_y,
                sys.start_direction
            ),
            (1, 2, 8, 6),
            "出生点读不出来，进图就站在一格里"
        );
        assert_eq!(sys.switch_name(3), "石碑点亮");
        assert_eq!(sys.variable_name(1), "听过次数");

        let map = MapDef::load(&a.vfs, sys.start_map_id).expect("Map001.json");
        assert_eq!((map.width, map.height), (24, 18));
        assert_eq!(
            map.event_ids(),
            vec![1, 2, 3, 4],
            "events[0] 那个 null 洞不该算事件"
        );
        assert!(map.autoplay_bgm);
        assert_eq!(map.bgm.name, "Field01");

        let ts = TilesetDef::load(&a.vfs, map.tileset_id).expect("Tilesets.json");
        assert_eq!(ts.tileset_name(TILE_SHEET), "Shell");
        // 水/树/石/墙/两块碑四向都不通；花是 NO_EFFECT（画得出、走得进）。
        for id in [3u32, 4, 5, 6, 9, 10] {
            assert_eq!(ts.flag(id), 0x0F, "图块 {id} 该四向不通");
        }
        assert_eq!(ts.flag(8), 0x10, "花该是 NO_EFFECT 而不是挡路");
        assert_eq!(ts.flag(1), 0, "草地全通行");

        // 音频的路径约定（`audio/{folder}/{name}.ogg`）与 VFS 键对得上，否则端口只会说
        // "VFS 里没有"，而真正的错是名字拼错了。
        let bgm = map.bgm.sound("bgm").expect("BGM 有名");
        assert!(a.vfs.exists(&vpath(&bgm.name)));
        assert!((bgm.gain - 0.9).abs() < 1e-6, "volume 90 ⇒ gain 0.9");
    }

    /// 验收（T11）：每个事件都翻得动、编得过，且降级计数与 fixture 的写法逐条对得上。
    #[test]
    fn every_event_translates_compiles_and_reports_its_gaps() {
        let a = vfs();
        let map = MapDef::load(&a.vfs, 1).unwrap();
        let opts = Options {
            tick_hz: TICK_HZ as u32,
            tile_px: TILE,
        };
        let mut total = TranslateReport::default();
        let mut topics: Vec<String> = Vec::new();
        for id in map.event_ids() {
            let ev = map.event(id).unwrap();
            let (def, rep) = translate_event(ev, 1, opts);
            total.merge(&rep);
            let script = def
                .compile()
                .unwrap_or_else(|e| panic!("事件 {id} 编不过: {e}"));
            assert!(
                !script.pages.is_empty(),
                "事件 {id} 一页都没有 ⇒ 这台 VM 会当它不存在"
            );
            topics.extend(custom_topics(&BTreeMap::from([(id, Arc::new(script))])));
        }
        // fixture 里只有 `251`（停止 SE）有 handler 却没建模，出现一次（长老的 `411` 支）。
        assert_eq!(total.total_unimplemented(), 1, "降级计数漂了: {total:?}");
        assert_eq!(total.unimplemented.get(&251), Some(&1));
        assert_eq!(
            total.dropped_branch_lines, 0,
            "251 那一块不该带走任何缩进行"
        );
        assert!(total.commands > 20, "指令条数少得不像装齐了四个事件");
        // 影片与转移的话题名是宿主接的那两个把手，名字必须与 `ShellBackend` 里的一致。
        assert!(topics.contains(&"rpgmv.movie".to_owned()), "{topics:?}");
        assert!(!topics.contains(&"rpgmv.transfer".to_owned()));
        assert!(
            !topics.contains(&"rpgmv.face".to_owned()),
            "101 的脸名写的是空串"
        );
        // HUD 那句 ASCII 摘要要把降级码带出来（wasm 没中文字形时它是唯一的读数）。
        assert!(ascii_summary(&total).contains("251x1"));
    }

    /// 页条件真的能选页：长老两页（自我开关 A）、石碑两页（开关 3）。
    #[test]
    fn page_conditions_pick_the_right_page() {
        let a = vfs();
        let sys = SystemDef::load(&a.vfs).unwrap();
        let map = MapDef::load(&a.vfs, 1).unwrap();
        let ts = TilesetDef::load(&a.vfs, map.tileset_id).unwrap();
        let mut vars = GameVars::with_system(&sys);
        let mut world = Scene_Map::enter(1, map, ts, (2, 8, 6), &vars);
        // 条件更严的那一页在前，所以没开任何东西时取到的是"后面那一页"。
        assert_eq!(world.get(1).unwrap().page, Some(1), "长老该在第二页");
        assert_eq!(world.get(2).unwrap().page, Some(1), "碑该是暗的那页");
        assert_eq!(world.get(3).unwrap().page, Some(1), "钟声还没响过");
        assert_eq!(page_art(world.page(2)), PageArt::Tile(9));

        vars.set_switch(3, true);
        vars.set_switch(4, true);
        vars.set_self_switch(1, 1, "A", true);
        let r = world.refresh(&vars);
        assert_eq!(r.changed, 3, "三处页条件都成立 ⇒ 三页都要换");
        assert_eq!(r.unsupported, 0, "fixture 不含物品/队伍条件");
        assert_eq!(world.get(1).unwrap().page, Some(0));
        assert_eq!(page_art(world.page(2)), PageArt::Tile(10), "碑亮 ⇒ 换图块");
        // 钟声的第 0 页是空页（无图无指令）⇒ 事件"消失"在画法上就是 `PageArt::None`。
        assert_eq!(world.get(3).unwrap().page, Some(0));
        assert_eq!(page_art(world.page(3)), PageArt::None);
    }

    /// 验收（T11）：进图、走到长老跟前、他被挡、面前查得到他 —— 手测清单第 2/3 条的数学。
    #[test]
    fn the_walk_to_the_elder_is_blocked_by_the_elder() {
        let a = vfs();
        let sys = SystemDef::load(&a.vfs).unwrap();
        let map = MapDef::load(&a.vfs, 1).unwrap();
        let ts = TilesetDef::load(&a.vfs, map.tileset_id).unwrap();
        let vars = GameVars::with_system(&sys);
        let mut world = Scene_Map::enter(
            sys.start_map_id,
            map,
            ts,
            (sys.start_x, sys.start_y, sys.start_direction),
            &vars,
        );
        assert_eq!(world.player().pos(), (2, 8));
        // 长老在 (6,8) 且 `priorityType` 1（与玩家同级）⇒ 他挡路。从 (2,8) 向东三格站到
        // 他西邻 (5,8)，第四格才撞墙。
        for _ in 0..3 {
            assert!(world.move_straight(PLAYER, 6), "路上不该有阻挡");
        }
        assert_eq!(world.player().pos(), (5, 8));
        assert!(!world.move_straight(PLAYER, 6), "他站着，走不进去");
        assert_eq!(world.player().pos(), (5, 8), "被挡就一步都没走");
        assert_eq!(world.player().dir, 6, "MV 的 moveStraight 失败也转向");
        // 面朝他（6 = 东）才谈得上"决定键"：`front` 给的就是那一格。
        assert_eq!(world.front(PLAYER), Some((6, 8)));
        assert_eq!(world.events_at(6, 8), vec![1]);
        assert!(world.page(1).is_some_and(|p| p.trigger == 0));
    }

    /// MV 的站立帧在行走图**中间那一列**，而引擎把 `step` 0 当站立帧 ⇒ 列序必须重排。
    #[test]
    fn standing_frame_is_the_middle_column() {
        let def = mv_walk_def();
        assert_eq!(def.frames.len(), 12);
        // 第 0 帧（row 0 = 朝下, step 0）该落在中间列。
        assert_eq!(def.frames[0].rect, char_rect(1, 0));
        assert_eq!(def.frames[1].rect, char_rect(0, 0));
        assert_eq!(def.frames[2].rect, char_rect(2, 0));
        // 朝上那一行（row 3）同理。
        assert_eq!(def.frames[9].rect, char_rect(1, 3));
        // 步帧序列 0→1→2→1→2：换脚两次才回到站立帧（与引擎路线同一个节奏）。
        let mut s = 0u8;
        let mut seen = vec![s];
        for _ in 0..5 {
            s = next_step(s);
            seen.push(s);
        }
        assert_eq!(seen, vec![0, 1, 2, 1, 2, 1]);
    }

    #[test]
    fn tile_rect_wraps_the_generated_sheet() {
        assert_eq!(tile_rect(1), Rect::new(0.0, 0.0, TILE, TILE));
        // 一张 8 列 × 4 行：1..8 在第一行，9 起换行。
        assert_eq!(tile_rect(8), Rect::new(7.0 * TILE, 0.0, TILE, TILE));
        assert_eq!(tile_rect(9), Rect::new(0.0, TILE, TILE, TILE));
        assert_eq!(tile_rect(11), Rect::new(2.0 * TILE, TILE, TILE, TILE));
        // `cell = (id-1) % 32`：号段用满之后回到同一张图（真工程的 A1–E 编号不在这层）。
        assert_eq!(tile_rect(33), tile_rect(1));
    }

    /// 追赶的判据：到位那一拍返回 `true` 并且像素恰好落在格点（差一个像素都不行 ——
    /// `arrived` 用它当 MV 的 `isMoving()` 闸）。
    #[test]
    fn chase_arrives_exactly_and_holds_the_gate() {
        let mut s = Sprite::solid(Vec2::new(TILE, TILE));
        s.teleport(lattice(2, 8));
        let target = lattice(6, 8);
        let mut ticks = 0;
        while !chase(&mut s, target, PLAYER_PX, true) {
            ticks += 1;
            assert!(ticks < 100, "永远到不了格 ⇒ 步长或目标算错了");
        }
        assert_eq!(s.pos, target, "到位必须一个像素都不差");
        // 4 格 = 192px，每拍 3px ⇒ 64 拍走完（63 次未到位 + 到位那一拍）。
        assert_eq!(ticks, 63, "四格该走 64 拍");
        assert!(chase(&mut s, target, PLAYER_PX, true), "已经在那一格");

        // 换向的中途：两轴都非零时取绝对值大的主轴。
        let mut d = Sprite::solid(Vec2::new(TILE, TILE));
        d.teleport(Vec2::new(48.0, 96.0));
        chase(&mut d, lattice(3, 1), PLAYER_PX, true);
        assert_eq!(d.pos.x, 51.0, "横向 48→144 更远，先走横轴");
        assert_eq!(d.pos.y, 96.0);
    }

    /// 消息窗装得下 fixture 里最长的那串选项（长老三行）。
    #[test]
    fn the_window_holds_three_choices() {
        assert!(WIN[1] + WIN[3] <= VIRTUAL.1 as f32);
        assert!(choice_rows_fit(3), "三行选项装不下");
        assert!(!choice_rows_fit(5), "装得下五行就等于窗高失控");
        let a = vfs();
        let map = MapDef::load(&a.vfs, 1).unwrap();
        let widest = map
            .event_ids()
            .iter()
            .filter_map(|id| map.event(*id))
            .flat_map(|e| e.pages.iter())
            .flat_map(|p| p.list.iter())
            .filter(|c| c.code == 102)
            .map(|c| c.texts(0).len())
            .max()
            .unwrap_or(0);
        assert_eq!(widest, 3, "fixture 里最长的一串选项是三个");
        assert!(choice_rows_fit(widest));
    }

    /// 页数据的读法：史莱姆是"角色图 + 玩家接触 + 随机挪 + 擦除"，钟声是"自动执行 + 空页"。
    #[test]
    fn event_pages_carry_the_triggers_this_example_dispatches_on() {
        let a = vfs();
        let map = MapDef::load(&a.vfs, 1).unwrap();
        let slime = map.event(4).unwrap();
        let p = &slime.pages[0];
        assert_eq!(p.trigger, 1, "玩家接触");
        assert_eq!(p.move_type, 1, "随机自主移动");
        assert_eq!(page_art(Some(p)), PageArt::Char("Actor2".to_owned()));
        // `moveFrequency` 3 ⇒ 阈值 30×(5-3)=60 拍，像素追得上（8 的话每拍都掷骰）。
        assert_eq!(p.move_frequency, 3);
        // 接触/擦除都在它的列表里（249 ME + 214 擦除）。
        let codes: Vec<u32> = p.list.iter().map(|c| c.code).collect();
        assert!(codes.contains(&249) && codes.contains(&214), "{codes:?}");

        let bell = map.event(3).unwrap();
        assert_eq!(bell.pages[1].trigger, 3, "自动执行");
        assert_eq!(bell.pages[1].priority_type, 0, "它不该挡路");
        assert_eq!(
            page_art(Some(&bell.pages[0])),
            PageArt::None,
            "第 0 页 = 消失"
        );

        // 速度档 → 像素/拍：MV 的 `2^s/256` 图块/帧，48px 图块下 4 档 = 3px/拍。
        assert_eq!(mv_speed_px_per_tick(4, Options::default()), PLAYER_PX);
    }
}
