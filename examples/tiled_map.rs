//! T8e 验收示例：三层 tilemap + 键盘四向行走 + 事件格碰撞。
//!
//! 三层照搬 RPG Maker 的 tile priority，全部落在同一条 `z` 轴上：
//!
//! ```text
//! 地面 0  <  贴地装饰 1  <  事件与角色 8 + 脚底y（y 排序）  <  上方 1_000_000 + 脚底y
//! ```
//!
//! 地面、事件（树/屋墙/宝箱/告示牌，与角色按脚底 y 互相压）、上方（树冠/屋顶，永远压住
//! 角色）都取自**同一张生成位图**，所以全场同 epoch：`BatchKey` 的序是
//! `(layer, epoch, blend, z, insertion)`，epoch 排在 z 之前，美术一旦分页，RPG 的 y 排序
//! 就只在页内成立。整帧因此是「1 个普通批 + 1 个 additive 批（灯火）+ 文字批」。
//!
//! 相机走现成的 viewport 栈：`App::prepare` 里 pop 掉上一帧的相机层、push 一个平移
//! `(-cam.x, -cam.y)` 的子 viewport，世界精灵的 `Sprite::layer` 就是那一层号，
//! `collect_at` 的产物因此自带相机投影；HUD 文字仍走同一层号但把平移抵消回去（屏幕固定）。
//! 相机取的是 `Sprite::interpolated(lattice_alpha(alpha))` —— 角色**真正被画出来的那个点**，
//! 而且只取落在像素点阵上的那几个点。这两条各治一次用户报回的抖动：
//! - 不能挪到 `tick` 末推：投影在 `tick` 之后、`draw` 之前烘进 `FrameU`，`alpha` 只有 `prepare`
//!   这一刻才知道。钉在拍尾的相机配插值角色，等于每拍把角色相对地图拽回 `(1 - alpha) × 2px`，
//!   那就是"人物一直在抖"。
//! - 也不能拿真亚像素的 `alpha` 去推：相机平移量一带小数，整张地图的整数格点就落到纹素中间
//!   被重采样，硬边缘随 `alpha` 呼吸，于是抖动从"角色相对地图晃"变成"满屏跟着晃"。
//!   把 `alpha` 折到点阵上（一拍 `SPEED = 2` px ⇒ 两个 1px 台阶）后角色与地图同在格点上。
//!
//! 图块尺寸就是清单钉的 32/48：格子 32×32，角色 32×48（占一格、向上伸出半格）。锚点恒取
//! 左下角 `(0,1)`，于是 `pos` 落在格点阵上，`cell_of` 只要一次除法，路线的逐格位移也不会
//! 把位置带偏。行走用 T8c 的 `MoveRoute`（`walkable` 钩子查事件格），碰撞表由地图字符直接
//! 生成；`Tween` 负责开箱闪，`AnimDef` 的时间驱动分支负责灯火的三帧火焰。
//!
//! 运行：
//! - 桌面：`cargo run --release -p sisf --example tiled_map`
//!   （窗口拉到 1088×832 或更大即 2× 整数倍，`IntegerFit` 与边条取整一起验掉）
//! - wasm：`cargo xtask web tiled_map` 后访问 `http://127.0.0.1:8080/?ex=tiled_map&wgl`
//! - 单测（地图/碰撞/三层 z 序/格点阵/相机钳制/位图区间）：
//!   `cargo test -p sisf --example tiled_map`
//!
//! 手测清单（桌面与 wasm 同一份）：
//! 1. 方向键/WASD 四向逐格走，脚下步帧 1↔2 交替，停手即回站立帧且静止不再换帧。
//!    走位期间**角色相对脚下格子不来回跳、整张地图也不跟着晃**（相机跟着点阵化后的插值点走，
//!    见 `prepare` 与 `lattice_alpha`）；且面朝左时脸与眼睛在左半边、头发盖住右半边（朝右镜像）。
//! 2. 走到水面/石墙/树/屋墙前停住，不陷入格子；桥与门可以过去。
//! 3. 站在树或屋子的**南邻**时头被树冠/屋顶压住（上方层），站在其**北邻**时整个人压在它
//!    上面（y 排序）—— 两种叠法同时成立才算三层。
//! 4. 走到宝箱的南邻、面朝它按 Enter：宝箱闪两下换成开箱，那一格随即可走。
//! 5. NPC 绕草地巡逻，走满一段停一拍再走（`wait: true` 独占自己的队列）。
//! 6. 把窗口拖成奇数边长（1001×701）：像素格不跳相；HUD 在 2×/4× 下发不虚是 T8d 的账。

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
use sisf::event::{EngineEvent, KeyKind, Lifecycle};
use sisf::render::{
    Atlas, AtlasPages, BatchKey, BatchList, Blend, DrawStats, Instance, NO_CLIP, Renderer,
    UploadRing, WHITE_EPOCH,
};
use sisf::sprite::{
    AnimDef, AnimState, DIR_DOWN, DIR_LEFT, DIR_RIGHT, DIR_UP, DirPattern, FrameDef, Keyframe,
    Layer, LoopMode, MoveRoute, Rect, RouteNode, Sprite, SpriteArena, SpriteRef, TaskScheduler,
    Tween, Vec2, ViewportRef, dir_vector,
};
use sisf::text::{Align, FontBundle, TextEngine, TextStyle};

// ---------------------------------------------------------------- 地图与图块

/// 图块边长（虚拟像素）。
const TILE: f32 = 32.0;
/// 角色图块：宽一格、高 1.5 格（RPG Maker 的 48 高行走图）。
const CHAR_W: f32 = 32.0;
const CHAR_H: f32 = 48.0;
/// 地图 24×20 格 = 768×640，比 544×416 的可视区大，相机有得可挪。
const MAP_W: usize = 24;
const MAP_H: usize = 20;
const WORLD_W: f32 = MAP_W as f32 * TILE;
const WORLD_H: f32 = MAP_H as f32 * TILE;
/// 可视区：RPG Maker MZ 的经典 544×416，正好 17×13 格。
const VIRTUAL: (u32, u32) = (544, 416);
/// 每拍位移（虚拟像素）。`TILE / SPEED = 16` 拍一格 ⇒ 3.75 格/秒，与 MV 默认速度同量级。
const SPEED: f32 = 2.0;
/// NPC 每段路之前的停拍数。
const NPC_HOLD: u32 = 30;

/// 世界精灵归属的 viewport 栈层。`push_camera` 往栈上推两层（世界 + HUD），世界恒为 1。
const WORLD: Layer = Layer(1);
/// HUD 归属的 viewport 栈层：必须**大于**世界层，因为批的排序先看 `layer` 再看 z，
/// 挂在 layer 0 的文字没法靠 z 盖住相机层（layer 1）上的地图。
const HUD_LAYER: u8 = 2;
/// 采集用可视口：投影层与归属标签同为 `WORLD`（`ViewportRef` 的不变式）。
const WORLD_VIEW: ViewportRef = ViewportRef::new(WORLD);
const _: () = assert!(
    HUD_LAYER > WORLD.0,
    "HUD 层号压不住世界层 ⇒ 地图会盖住文字（批先按 layer 排）"
);

/// 三条 z 带（越小越先画）。地面与贴地装饰不参与 y 排序。
const Z_GROUND: i32 = 0;
const Z_BELOW: i32 = 1;
const Z_STAND: i32 = 8;
const Z_ABOVE: i32 = 1_000_000;

/// 格点阵上第 `(col, row)` 格的**左下角**，即 `anchor = (0,1)` 精灵的 `pos`。
fn lattice(col: i32, row: i32) -> Vec2 {
    Vec2::new(col as f32 * TILE, (row + 1) as f32 * TILE)
}

/// `pos` → 所在格。角色 48 高但只占一格，行号从下沿反推。`round` 让逐格移动中途读它也只
/// 会给出已经 committed 的那一格，不会跳回起点。
fn cell_of(pos: Vec2) -> (i32, i32) {
    (
        (pos.x / TILE).round() as i32,
        (pos.y / TILE).round() as i32 - 1,
    )
}

/// 图块号：位图内 8 列 × 3 行（`tile_rect` 定义装箱序）。
mod tile {
    pub const GRASS_A: u8 = 0;
    pub const GRASS_B: u8 = 1;
    pub const PATH: u8 = 2;
    pub const WATER: u8 = 3;
    pub const BRIDGE: u8 = 4;
    pub const COBBLE: u8 = 5;
    pub const FLOOR: u8 = 6;
    pub const SAND: u8 = 7;
    pub const STONE: u8 = 8;
    pub const TRUNK: u8 = 9;
    pub const CANOPY: u8 = 10;
    pub const CHEST: u8 = 11;
    pub const CHEST_OPEN: u8 = 12;
    pub const SIGN: u8 = 13;
    pub const FLOWER: u8 = 14;
    pub const HOUSE: u8 = 15;
    pub const ROOF: u8 = 16;
    pub const DOOR: u8 = 17;
    pub const LAMP: u8 = 18;
    pub const BUSH: u8 = 19;
    pub const FLAME0: u8 = 20;
    pub const FLAME1: u8 = 21;
    pub const FLAME2: u8 = 22;
    /// 用到的最大块号；它之后的格子必须全空（`build_sheet` 的自证）。
    pub const LAST: u8 = 22;
}

/// 图块号 → 生成位图里的子矩形。
fn tile_rect(id: u8) -> Rect {
    Rect::new(
        (id as usize % 8) as f32 * TILE,
        (id as usize / 8) as f32 * TILE,
        TILE,
        TILE,
    )
}

/// 一格地图：三层里各放什么 + 能不能落脚。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cell {
    ground: u8,
    /// 事件层图块（与角色按 y 互压），`-1` = 无。
    obj: i8,
    /// 上方层图块（永远压住角色），`-1` = 无。
    above: i8,
    walkable: bool,
}

const EMPTY: Cell = Cell {
    ground: tile::GRASS_A,
    obj: -1,
    above: -1,
    walkable: true,
};

/// 地图字符 → 格子内容。`' '` 是沙地（画在地图上没有，留给 `SAND` 块的自证）。
fn spec(c: char) -> Cell {
    let mut cell = EMPTY;
    match c {
        '.' => cell.ground = tile::GRASS_A,
        ',' => cell.ground = tile::GRASS_B,
        ':' => cell.ground = tile::PATH,
        '=' => cell.ground = tile::BRIDGE,
        ' ' => cell.ground = tile::SAND,
        '~' => {
            cell.ground = tile::WATER;
            cell.walkable = false;
        }
        '#' => {
            cell.ground = tile::COBBLE;
            cell.obj = tile::STONE as i8;
            cell.walkable = false;
        }
        'f' => {
            cell.ground = tile::GRASS_B;
            // 贴地装饰：走得上得去，但永远在角色之下（priority 0）。
            cell.obj = tile::FLOWER as i8;
        }
        'B' => {
            cell.ground = tile::GRASS_A;
            cell.obj = tile::BUSH as i8;
            cell.walkable = false;
        }
        'T' => {
            cell.ground = tile::GRASS_B;
            cell.obj = tile::TRUNK as i8;
            cell.above = tile::CANOPY as i8;
            cell.walkable = false;
        }
        'L' => {
            cell.ground = tile::PATH;
            cell.obj = tile::LAMP as i8;
            cell.walkable = false;
        }
        'S' => {
            cell.ground = tile::PATH;
            cell.obj = tile::SIGN as i8;
            cell.walkable = false;
        }
        'C' => {
            cell.ground = tile::GRASS_B;
            cell.obj = tile::CHEST as i8;
            cell.walkable = false;
        }
        'H' => {
            cell.ground = tile::COBBLE;
            cell.obj = tile::HOUSE as i8;
            cell.above = tile::ROOF as i8;
            cell.walkable = false;
        }
        'D' => {
            cell.ground = tile::FLOOR;
            cell.obj = tile::DOOR as i8;
            cell.above = tile::ROOF as i8;
            // 门可去，屋顶照旧压住进去的人 —— 「上方层」在画面上唯一藏不住的证据。
            cell.walkable = true;
        }
        other => panic!("地图用了未知字符 {other:?}"),
    }
    cell
}

/// 地图。每行 4 段 × 6 格 = 24 列，`World::new` 校验段数与列数。
const MAP: [&str; MAP_H] = [
    "######|######|######|######",
    "#.....|,,,,,,|......|.....#",
    "#,,,,,|......|..T.HH|HHHH.#",
    "#.....|......|.f..HH|HHHH.#",
    "#.....|......|....HH|HHHH.#",
    "#.....|......|....HH|DHHH.#",
    "#.....|......|:C....|:....#",
    "#.....|......|:.....|:..T.#",
    "#.....|B.L...|:.....|:.L..#",
    "#.....|...:::|::::::|:::::#",
    "#.....|......|:S....|.....#",
    "#.T...|,,,,,,|:.....|..f..#",
    "#.,...|......|:....B|.....#",
    "#.~~~~|~.....|......|.....#",
    "#.~~~~|~.....|.f....|.....#",
    "#.====|======|......|.....#",
    "#.~~~~|~.....|..T...|.....#",
    "#.~~~~|~...f.|......|.....#",
    "#.....|.f....|......|..T..#",
    "######|######|######|######",
];

/// 玩家出生格（竖路的最南端）与 NPC 起点。
const HERO_CELL: (i32, i32) = (12, 12);
const NPC_CELL: (i32, i32) = (7, 4);
/// NPC 巡逻矩形（含端点）：一块没有任何物体的草地。
const NPC_PATROL: (i32, i32, i32, i32) = (7, 4, 10, 6);

/// 地图数据：格子内容 + 通行表。
struct World {
    cells: Vec<Cell>,
    /// 与 `cells` 同序的通行位图。`MoveRoute::walkable` 拿它的**克隆**（480 字节一次一格，
    /// 比把 `Rc` 塞进闭包更清楚：路线只看排它那一刻的地图）。
    pass: Vec<bool>,
}

impl World {
    fn new() -> Self {
        let mut cells = Vec::with_capacity(MAP_W * MAP_H);
        for (row, line) in MAP.iter().enumerate() {
            let mut cols = 0;
            for chunk in line.split('|') {
                assert_eq!(
                    chunk.chars().count(),
                    6,
                    "第 {row} 行的一段不是 6 列: {chunk:?}"
                );
                for c in chunk.chars() {
                    cells.push(spec(c));
                    cols += 1;
                }
            }
            assert_eq!(chunk_count(line), 4, "第 {row} 行不是 4 段");
            assert_eq!(cols, MAP_W, "第 {row} 行不是 {MAP_W} 列，实际 {cols}");
        }
        assert_eq!(cells.len(), MAP_W * MAP_H);
        let pass = cells.iter().map(|c| c.walkable).collect();
        Self { cells, pass }
    }

    fn idx(col: i32, row: i32) -> Option<usize> {
        if col < 0 || row < 0 || col as usize >= MAP_W || row as usize >= MAP_H {
            return None;
        }
        Some(row as usize * MAP_W + col as usize)
    }

    fn cell(&self, col: i32, row: i32) -> Option<Cell> {
        Self::idx(col, row).map(|i| self.cells[i])
    }

    /// 格子空间的通行问答。走的那条路只能问像素空间（`MoveRoute` 的钩子拿到的是逐拍坐标，
    /// 而且必须 own 通行表），所以这个函数只有测试在用。
    #[cfg(test)]
    fn is_walkable(&self, col: i32, row: i32) -> bool {
        Self::idx(col, row).is_some_and(|i| self.pass[i])
    }
}

fn chunk_count(line: &str) -> usize {
    line.split('|').count()
}

/// 通行表的落脚判定：`MoveRoute::walkable` 的闭包体，也是单测直接叫的那个函数。
fn landing_ok(pass: &[bool], x: f32, y: f32) -> bool {
    let (col, row) = cell_of(Vec2::new(x, y));
    World::idx(col, row).is_some_and(|i| pass[i])
}

// ------------------------------------------------------------------ 生成美术

/// 整张美术 = 256×288：3 行 32×32 图块 + 下方两条角色带（4 行 × 3/2 列的 32×48）。
const SHEET: (u32, u32) = (256, 288);
const HERO_AT: [f32; 2] = [0.0, 96.0];
const NPC_AT: [f32; 2] = [128.0, 96.0];

/// 角色带第 `(row, col)` 格（row = 朝向行，col = 步帧列）。
fn char_rect(at: [f32; 2], row: u32, col: u32) -> Rect {
    Rect::new(
        at[0] + col as f32 * CHAR_W,
        at[1] + row as f32 * CHAR_H,
        CHAR_W,
        CHAR_H,
    )
}

/// RGBA8 画布（写入即预乘，图集要 premultiplied）。
struct Sheet {
    w: u32,
    h: u32,
    px: Vec<u8>,
}

impl Sheet {
    fn new(w: u32, h: u32) -> Self {
        Self {
            w,
            h,
            px: vec![0; (w * h * 4) as usize],
        }
    }

    fn put(&mut self, x: u32, y: u32, c: [u8; 4]) {
        if x >= self.w || y >= self.h {
            return;
        }
        let i = ((y * self.w + x) * 4) as usize;
        let a = c[3] as u16;
        // 只有 0/255 两档 alpha 时这一步是恒等的，写清楚是为了半透明阴影不串色。
        self.px[i] = (c[0] as u16 * a / 255) as u8;
        self.px[i + 1] = (c[1] as u16 * a / 255) as u8;
        self.px[i + 2] = (c[2] as u16 * a / 255) as u8;
        self.px[i + 3] = c[3];
    }

    fn rect(&mut self, at: (u32, u32), size: (u32, u32), c: [u8; 4]) {
        for y in at.1..at.1 + size.1 {
            for x in at.0..at.0 + size.0 {
                self.put(x, y, c);
            }
        }
    }

    /// 只画矩形边框（图块的接缝与高亮）。
    fn outline(&mut self, at: (u32, u32), size: (u32, u32), c: [u8; 4]) {
        for x in at.0..at.0 + size.0 {
            self.put(x, at.1, c);
            self.put(x, at.1 + size.1 - 1, c);
        }
        for y in at.1..at.1 + size.1 {
            self.put(at.0, y, c);
            self.put(at.0 + size.0 - 1, y, c);
        }
    }

    /// 确定性"噪点"：不用随机数，跑两遍生成的美术必须逐字节一致（手测与将来的 golden 都要）。
    fn specks(&mut self, at: (u32, u32), size: (u32, u32), c: [u8; 4], stride: u32, off: u32) {
        for dy in 0..size.1 {
            for dx in 0..size.0 {
                if (dx * 7 + dy * 13 + off).is_multiple_of(stride) {
                    self.put(at.0 + dx, at.1 + dy, c);
                }
            }
        }
    }
}

fn rgb(r: u8, g: u8, b: u8) -> [u8; 4] {
    [r, g, b, 255]
}

/// 画满整张美术：3 行图块 + 两条角色带。
fn build_sheet() -> Vec<u8> {
    let mut s = Sheet::new(SHEET.0, SHEET.1);
    for id in 0..=tile::LAST {
        draw_tile(&mut s, id);
    }
    // 玩家（MV 4×3）与 NPC（VX/Ace 2×2）：同一张位图，两套图样语义。
    for dir_row in 0..4u32 {
        for step in 0..3u32 {
            draw_body(
                &mut s,
                char_rect(HERO_AT, dir_row, step),
                dir_row,
                step,
                rgb(72, 96, 168),
                rgb(226, 190, 150),
            );
        }
    }
    for dir_row in 0..4u32 {
        for step in 0..2u32 {
            draw_body(
                &mut s,
                char_rect(NPC_AT, dir_row, step),
                dir_row,
                step,
                rgb(150, 78, 74),
                rgb(206, 172, 128),
            );
        }
    }
    s.px
}

/// 单个 32×32 图块，位置由 `tile_rect` 决定，这里只画内容。
fn draw_tile(s: &mut Sheet, id: u8) {
    let at = ((id as usize % 8) as u32 * 32, (id as usize / 8) as u32 * 32);
    let plain = |s: &mut Sheet, c: [u8; 4]| s.rect(at, (32, 32), c);
    match id {
        tile::GRASS_A => {
            plain(s, rgb(58, 116, 52));
            s.specks(at, (32, 32), rgb(74, 138, 66), 11, 3);
        }
        tile::GRASS_B => {
            plain(s, rgb(66, 128, 58));
            s.specks(at, (32, 32), rgb(52, 108, 48), 13, 7);
        }
        tile::PATH => {
            plain(s, rgb(158, 132, 92));
            s.specks(at, (32, 32), rgb(138, 112, 76), 9, 1);
            s.rect((at.0, at.1), (32, 1), rgb(176, 150, 108));
        }
        tile::WATER => {
            plain(s, rgb(42, 92, 158));
            s.rect((at.0 + 2, at.1 + 8), (20, 1), rgb(96, 152, 214));
            s.rect((at.0 + 8, at.1 + 20), (20, 1), rgb(96, 152, 214));
        }
        tile::BRIDGE => {
            plain(s, rgb(126, 90, 52));
            for x in (4..32).step_by(8) {
                s.rect((at.0 + x, at.1), (1, 32), rgb(96, 66, 38));
            }
            s.rect((at.0, at.1 + 2), (32, 2), rgb(150, 112, 66));
            s.rect((at.0, at.1 + 28), (32, 2), rgb(150, 112, 66));
        }
        tile::COBBLE => {
            plain(s, rgb(96, 96, 104));
            s.outline((at.0 + 2, at.1 + 2), (12, 12), rgb(74, 74, 82));
            s.outline((at.0 + 18, at.1 + 4), (10, 10), rgb(74, 74, 82));
            s.outline((at.0 + 6, at.1 + 18), (10, 10), rgb(74, 74, 82));
            s.outline((at.0 + 20, at.1 + 18), (9, 9), rgb(74, 74, 82));
        }
        tile::FLOOR => plain(s, rgb(168, 150, 118)),
        tile::SAND => {
            plain(s, rgb(204, 188, 132));
            s.specks(at, (32, 32), rgb(186, 168, 112), 15, 2);
        }
        tile::STONE => {
            plain(s, rgb(112, 112, 122));
            s.rect((at.0, at.1), (32, 3), rgb(140, 140, 152));
            for y in (6..30).step_by(8) {
                s.rect((at.0, at.1 + y), (32, 1), rgb(84, 84, 94));
            }
            for x in (4..32).step_by(12) {
                s.rect((at.0 + x, at.1), (1, 32), rgb(84, 84, 94));
            }
        }
        tile::TRUNK => {
            s.rect((at.0 + 11, at.1), (10, 32), rgb(96, 66, 40));
            s.rect((at.0 + 11, at.1), (2, 32), rgb(122, 86, 54));
            s.rect((at.0 + 6, at.1 + 26), (20, 6), rgb(76, 52, 32));
        }
        tile::CANOPY => {
            // 上半实心、下半透明：树冠压住南邻角色的头，又让出树干格的下沿。
            s.rect((at.0, at.1), (32, 20), rgb(38, 92, 44));
            s.rect((at.0 + 2, at.1 + 2), (28, 6), rgb(56, 118, 58));
            s.rect((at.0 + 4, at.1 + 20), (24, 4), rgb(30, 78, 38));
            s.specks((at.0 + 1, at.1 + 1), (30, 18), rgb(70, 136, 70), 12, 4);
        }
        tile::CHEST | tile::CHEST_OPEN => {
            s.rect((at.0 + 3, at.1 + 12), (26, 18), rgb(150, 102, 52));
            s.outline((at.0 + 3, at.1 + 12), (26, 18), rgb(96, 62, 30));
            if id == tile::CHEST {
                s.rect((at.0 + 3, at.1 + 6), (26, 8), rgb(132, 88, 44));
                s.rect((at.0 + 14, at.1 + 10), (4, 8), rgb(214, 178, 74));
            } else {
                // 开箱：盖向上翻起，箱里露出暗色与一点金。
                s.rect((at.0 + 5, at.1), (22, 8), rgb(120, 80, 40));
                s.rect((at.0 + 6, at.1 + 14), (20, 8), rgb(48, 34, 20));
                s.rect((at.0 + 10, at.1 + 16), (12, 4), rgb(214, 178, 74));
            }
        }
        tile::SIGN => {
            s.rect((at.0 + 14, at.1 + 16), (4, 16), rgb(112, 80, 46));
            s.rect((at.0 + 4, at.1 + 4), (24, 14), rgb(176, 136, 82));
            s.outline((at.0 + 4, at.1 + 4), (24, 14), rgb(112, 80, 46));
            s.rect((at.0 + 7, at.1 + 8), (18, 1), rgb(96, 68, 36));
            s.rect((at.0 + 7, at.1 + 12), (12, 1), rgb(96, 68, 36));
        }
        tile::FLOWER => {
            // 贴地装饰：只占下沿，大半格透明，画在角色之下也不挡人。
            for (dx, petal) in [
                (4usize, rgb(236, 236, 244)),
                (14, rgb(232, 120, 120)),
                (24, rgb(240, 224, 120)),
            ] {
                s.rect((at.0 + dx as u32, at.1 + 22), (4, 4), petal);
                s.rect((at.0 + dx as u32 + 1, at.1 + 26), (2, 4), rgb(44, 100, 44));
            }
        }
        tile::HOUSE => {
            plain(s, rgb(150, 118, 82));
            for y in (2..32).step_by(5) {
                s.rect((at.0, at.1 + y), (32, 1), rgb(118, 92, 62));
            }
            s.rect((at.0 + 10, at.1 + 10), (12, 12), rgb(72, 96, 128));
            s.outline((at.0 + 10, at.1 + 10), (12, 12), rgb(96, 74, 48));
        }
        tile::ROOF => {
            plain(s, rgb(148, 62, 52));
            for y in (4..32).step_by(6) {
                s.rect((at.0, at.1 + y), (32, 2), rgb(112, 44, 40));
            }
            s.rect((at.0, at.1 + 28), (32, 4), rgb(96, 38, 34));
        }
        tile::DOOR => {
            s.rect((at.0 + 6, at.1 + 2), (20, 30), rgb(92, 60, 34));
            s.outline((at.0 + 6, at.1 + 2), (20, 30), rgb(66, 42, 24));
            s.rect((at.0 + 22, at.1 + 16), (2, 3), rgb(214, 178, 74));
        }
        tile::LAMP => {
            s.rect((at.0 + 14, at.1 + 8), (4, 24), rgb(70, 70, 78));
            s.rect((at.0 + 12, at.1 + 30), (8, 2), rgb(52, 52, 58));
            s.rect((at.0 + 10, at.1 + 2), (12, 8), rgb(46, 46, 52));
            s.rect((at.0 + 12, at.1 + 4), (8, 4), rgb(246, 214, 128));
        }
        tile::BUSH => {
            s.rect((at.0 + 2, at.1 + 10), (28, 20), rgb(44, 104, 50));
            s.rect((at.0 + 6, at.1 + 6), (20, 8), rgb(58, 126, 62));
            s.specks((at.0 + 3, at.1 + 9), (26, 18), rgb(224, 96, 120), 19, 5);
        }
        tile::FLAME0 | tile::FLAME1 | tile::FLAME2 => {
            // additive 通路：底色保持全黑（黑 = 不加亮），火苗画在灯盏那一格的上部。
            let n = (id - tile::FLAME0) as u32;
            let (w, h) = (10 - 2 * n, 12 - 2 * n);
            s.rect(
                (at.0 + 16 - w / 2, at.1 + 8 - h / 2),
                (w, h),
                rgb(240, 148, 44),
            );
            s.rect(
                (at.0 + 16 - w / 4, at.1 + 8 - h / 4),
                (w / 2, h / 2),
                rgb(252, 232, 152),
            );
        }
        _ => {}
    }
}

/// 一个 32×48 的角色格：`dir_row` = 朝向行（0 下 / 1 左 / 2 右 / 3 上，与
/// `DirPattern::dir_row` 同序），`r` 所在列 = 步帧（0 站立）。
fn draw_body(s: &mut Sheet, r: Rect, dir_row: u32, step: u32, tunic: [u8; 4], skin: [u8; 4]) {
    let at = (r.x as u32, r.y as u32);
    let ink = [0u8, 0, 0, 255];
    // 落地阴影（半透明，顺带验证 `put` 的预乘分支）。
    s.rect((at.0 + 6, at.1 + 44), (20, 3), [0, 0, 0, 90]);
    // 腿：0 列并排站立；1/2 列各把一条腿往前（对调 ⇒ 两步一循环）。2×2 图样只有 1 列步帧，
    // 于是"迈步/站立"交替，正是 VX/Ace 的两帧走路。
    let (l, rr) = match step {
        1 => ((at.0 + 6, at.1 + 36), (at.0 + 21, at.1 + 34)),
        2 => ((at.0 + 21, at.1 + 34), (at.0 + 6, at.1 + 36)),
        _ => ((at.0 + 10, at.1 + 36), (at.0 + 19, at.1 + 36)),
    };
    s.rect(l, (5, 8), rgb(62, 48, 36));
    s.rect(rr, (5, 8), rgb(62, 48, 36));
    // 躯干 + 腰带 + 手臂（袖子与上衣同色；走路的读秒全在腿上）。
    s.rect((at.0 + 8, at.1 + 22), (16, 15), tunic);
    s.rect((at.0 + 8, at.1 + 32), (16, 2), rgb(52, 40, 30));
    s.rect((at.0 + 5, at.1 + 23), (4, 9), tunic);
    s.rect((at.0 + 23, at.1 + 23), (4, 9), tunic);
    // 头与头发：四个朝向的差别只在脸。
    s.rect((at.0 + 8, at.1 + 8), (16, 14), skin);
    s.rect((at.0 + 7, at.1 + 6), (18, 5), rgb(58, 40, 28));
    match dir_row {
        0 => {
            s.rect((at.0 + 11, at.1 + 14), (2, 3), ink);
            s.rect((at.0 + 19, at.1 + 14), (2, 3), ink);
        }
        // 侧面：头发盖住**后脑**那半边，脸和眼睛留在朝向的那侧，且必须后画（先画就被盖掉了）。
        1 => {
            s.rect((at.0 + 15, at.1 + 6), (10, 16), rgb(58, 40, 28));
            s.rect((at.0 + 9, at.1 + 14), (2, 3), ink);
        }
        2 => {
            s.rect((at.0 + 7, at.1 + 6), (10, 16), rgb(58, 40, 28));
            s.rect((at.0 + 21, at.1 + 14), (2, 3), ink);
        }
        _ => s.rect((at.0 + 7, at.1 + 6), (18, 12), rgb(58, 40, 28)),
    }
}

// ---------------------------------------------------------------------- 输入

/// 四个方向（WASD 与箭头同绑；`EngineEvent::Key` 给的是布局无关的物理键名）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    fn of_code(code: &str) -> Option<Self> {
        match code {
            "ArrowUp" | "KeyW" => Some(Self::Up),
            "ArrowDown" | "KeyS" => Some(Self::Down),
            "ArrowLeft" | "KeyA" => Some(Self::Left),
            "ArrowRight" | "KeyD" => Some(Self::Right),
            _ => None,
        }
    }

    /// RPG Maker 的方向键值。
    fn rpg(self) -> u8 {
        match self {
            Self::Up => DIR_UP,
            Self::Down => DIR_DOWN,
            Self::Left => DIR_LEFT,
            Self::Right => DIR_RIGHT,
        }
    }

    fn from_rpg(dir: u8) -> Self {
        match dir {
            DIR_UP => Self::Up,
            DIR_LEFT => Self::Left,
            DIR_RIGHT => Self::Right,
            _ => Self::Down,
        }
    }

    fn unit(self) -> Vec2 {
        dir_vector(self.rpg())
    }
}

/// 按住的方向集合，按按下序排列 ⇒ `latest()` 是"最后按下的那个"（RPG 的方向优先级）。
#[derive(Default)]
struct Held {
    order: Vec<Dir>,
}

impl Held {
    fn set(&mut self, dir: Dir, down: bool) {
        self.order.retain(|&d| d != dir);
        if down {
            self.order.push(dir);
        }
    }

    fn latest(&self) -> Option<Dir> {
        self.order.last().copied()
    }

    fn clear(&mut self) {
        self.order.clear();
    }
}

// ---------------------------------------------------------------------- 示例

/// 可交互物件（Enter 面朝它时用）。
struct Item {
    cell: usize,
    sprite: SpriteRef,
    kind: ItemKind,
}

enum ItemKind {
    Chest { open: bool },
    Sign,
}

struct TiledMap {
    world: World,
    atlas: Atlas,
    pages: AtlasPages,
    uploads: UploadRing,
    /// 整张美术（唯一位图 ⇒ 唯一 epoch ⇒ z 有全序权），同时是 spawn 用的模板精灵。
    proto: Sprite,
    arena: SpriteArena,
    tasks: TaskScheduler,
    hero: SpriteRef,
    npc: SpriteRef,
    items: Vec<Item>,
    keys: Held,
    decide: bool,
    /// 拍尾的角色位置（HUD 读数与格子日志读它，避免 `draw` 再摸 arena）。
    hero_at: Vec2,
    /// 本帧烘进投影的相机，由 `prepare` 按插值后的角色位置算。
    cam: Vec2,
    /// 本帧画 sprite 用的点阵系数，由 `prepare` 按屏幕倍率算好（`draw` 拿不到 `FrameCtx`）。
    draw_alpha: f32,
    /// `prepare` 里是否已 push 过相机层（`pop`/`push` 成对 ⇒ 栈深恒在 1..=3）。
    cam_pushed: bool,
    stats: Option<DrawStats>,
    /// 上一次看到的物理尺寸；变了就把 letterbox 读数打进行日志（手测 resize 靠它取证）。
    phys: (u32, u32),
    /// 上一拍角色所在的格。换格才打日志：桌面与 wasm 的手测靠这条链取证
    /// "按键 → `MoveRoute` 逐格走 → 相机跟着挪"，频率上限就是走路速度（3.75 格/秒）。
    at_cell: (i32, i32),
    /// 装箱时基（`Atlas::touch`）。
    frame: u64,
    ticks: u64,
    vfs: Vfs,
    font_loaded: bool,
    text: TextEngine,
    msg: String,
}

impl TiledMap {
    fn new() -> Self {
        let world = World::new();
        let atlas = Atlas::new();
        // 默认桶（`with_limits(2048, 8)`）就装得下 256×288（含 gutter 落进 512 桶的一页）。
        let sheet = atlas
            .new_bitmap(SHEET.0, SHEET.1, build_sheet(), 0)
            .expect("生成美术该占得下一页");
        let mut text = TextEngine::new(FontBundle::system());
        text.set_raster_scale(1.0);
        let mut app = Self {
            atlas,
            world,
            uploads: UploadRing::new(),
            pages: AtlasPages::new(),
            arena: SpriteArena::with_capacity(MAP_W * MAP_H * 3),
            tasks: TaskScheduler::new(),
            hero: SpriteRef(0, 0),
            npc: SpriteRef(0, 0),
            items: Vec::new(),
            keys: Held::default(),
            decide: false,
            hero_at: lattice(HERO_CELL.0, HERO_CELL.1),
            cam: Vec2::ZERO,
            draw_alpha: 0.0,
            cam_pushed: false,
            stats: None,
            phys: (0, 0),
            at_cell: HERO_CELL,
            frame: 0,
            ticks: 0,
            proto: Sprite::new(sheet.clone()),
            vfs: vfs(),
            font_loaded: false,
            text,
            msg: "walk with WASD/arrows, Enter uses".to_owned(),
        };
        app.uploads.queue(sheet);
        app.build_map();
        app.spawn_actors();
        app
    }

    /// 由地图数据生成三层精灵：地面/装饰/事件/上方各占一条 z 带。
    fn build_map(&mut self) {
        for row in 0..MAP_H as i32 {
            for col in 0..MAP_W as i32 {
                let Some(cell) = self.world.cell(col, row) else {
                    continue;
                };
                let at = lattice(col, row);
                let foot = at.y as i32;
                self.spawn_tile(cell.ground, at, Z_GROUND);
                if cell.obj >= 0 {
                    let id = cell.obj as u8;
                    let z = if id == tile::FLOWER {
                        Z_BELOW
                    } else {
                        Z_STAND + foot
                    };
                    let sprite = self.spawn_tile(id, at, z);
                    if id == tile::LAMP {
                        self.spawn_flame(at);
                    }
                    let kind = match id {
                        tile::CHEST => Some(ItemKind::Chest { open: false }),
                        tile::SIGN => Some(ItemKind::Sign),
                        _ => None,
                    };
                    if let Some(kind) = kind {
                        self.items.push(Item {
                            cell: World::idx(col, row).expect("在格内"),
                            sprite,
                            kind,
                        });
                    }
                }
                if cell.above >= 0 {
                    self.spawn_tile(cell.above as u8, at, Z_ABOVE + foot);
                }
            }
        }
    }

    fn spawn_tile(&mut self, id: u8, at: Vec2, z: i32) -> SpriteRef {
        let mut s = self.proto.clone();
        s.src = tile_rect(id);
        s.anchor = Vec2::new(0.0, 1.0);
        s.teleport(at);
        s.z = z;
        s.layer = WORLD;
        self.arena.spawn(s)
    }

    /// 灯火：时间驱动三帧（`pattern: None`）+ additive 混合，与移动驱动的行走图并存。
    fn spawn_flame(&mut self, at: Vec2) {
        let frames = [tile::FLAME0, tile::FLAME1, tile::FLAME2]
            .iter()
            .map(|id| FrameDef::new(tile_rect(*id), 120));
        let mut s = self.proto.clone();
        s.anchor = Vec2::new(0.0, 1.0);
        // 火苗在灯盏里：从灯杆格向上挪 24px（灯座那一格的上半）。
        s.teleport(Vec2::new(at.x, at.y - 24.0));
        s.z = Z_ABOVE + at.y as i32;
        s.layer = WORLD;
        s.blend = Blend::Add;
        let def = AnimDef {
            frames: frames.collect(),
            loop_: LoopMode::Loop(0),
            pattern: None,
        };
        s.set_animation(Some(AnimState::new(Arc::new(def))));
        self.arena.spawn(s);
    }

    /// 两个角色：玩家（MV 4×3）与巡逻 NPC（VX/Ace 2×2），同一张图两套图样。
    fn spawn_actors(&mut self) {
        self.hero = self.spawn_actor(HERO_AT, DirPattern::RpgMaker4x3, HERO_CELL);
        self.npc = self.spawn_actor(NPC_AT, DirPattern::RpgMaker2x2, NPC_CELL);
    }

    fn spawn_actor(&mut self, at: [f32; 2], pattern: DirPattern, cell: (i32, i32)) -> SpriteRef {
        // 帧表由 `DirPattern` 生成，再整表平移到角色带：`AnimDef` 只管行优先帧序，
        // 美术摆在位图的哪里是调用方的事。
        let mut def = AnimDef::grid_pattern(pattern, CHAR_W, CHAR_H, 0);
        for (i, f) in def.frames.iter_mut().enumerate() {
            f.rect = char_rect(
                at,
                (i / pattern.cols() as usize) as u32,
                (i % pattern.cols() as usize) as u32,
            );
        }
        assert_eq!(def.frames.len(), pattern.cell_count() as usize);
        assert_eq!(
            def.frames[0].rect,
            char_rect(at, 0, 0),
            "第 0 帧 = 朝下站立帧"
        );
        let mut s = self.proto.clone();
        s.anchor = Vec2::new(0.0, 1.0);
        s.teleport(lattice(cell.0, cell.1));
        s.z = Z_STAND + s.pos.y as i32;
        s.layer = WORLD;
        s.set_animation(Some(AnimState::new(Arc::new(def))));
        self.arena.spawn(s)
    }

    /// 一拍的玩家意图：上一格的路线没跑完就不吃新输入（RPG 的"移动中不改向"）。
    fn player_tick(&mut self) {
        if self.tasks.pending(self.hero) {
            return;
        }
        if self.decide {
            self.decide = false;
            self.interact();
            return;
        }
        let Some(dir) = self.keys.latest() else {
            return;
        };
        let pass = self.world.pass.clone();
        // 一格一条路线：`wait(true)` 让这一格独占队列；撞墙由 `MoveRoute` 自己作废本节点。
        self.tasks.schedule(
            self.hero,
            MoveRoute::grid(
                vec![RouteNode::Move {
                    dir: dir.rpg(),
                    cells: 1,
                }],
                TILE,
                SPEED,
            )
            .walkable(move |x, y| landing_ok(&pass, x, y))
            .wait(true),
        );
    }

    /// Enter：面朝那一格上的物件（宝箱开、告示牌读）。
    fn interact(&mut self) {
        let (col, row, facing) = self
            .arena
            .get(self.hero)
            .map(|h| (cell_of(h.pos), h.dir().unwrap_or(DIR_DOWN)))
            .map(|((c, r), d)| (c, r, Dir::from_rpg(d)))
            .expect("玩家在 arena 里");
        let unit = facing.unit();
        let target = (col + unit.x as i32, row + unit.y as i32);
        let Some(idx) = World::idx(target.0, target.1) else {
            return;
        };
        let Some(hit) = self.items.iter().position(|it| it.cell == idx) else {
            self.msg = format!("nothing to use at {},{}", target.0, target.1);
            return;
        };
        let sprite = self.items[hit].sprite;
        match &mut self.items[hit].kind {
            ItemKind::Sign => {
                self.msg = "sign: tiles 32, characters 48".to_owned();
                platform::log_info("[tiled_map] 读告示牌");
            }
            ItemKind::Chest { open } if *open => {
                self.msg = "the chest is already open".to_owned();
            }
            ItemKind::Chest { open } => {
                *open = true;
                self.open_chest(sprite, idx);
            }
        }
    }

    /// 开箱：`Tween` 闪两下，完成回调（按 tick 计数）里换图块并把那一格改成可走。
    fn open_chest(&mut self, sprite: SpriteRef, cell: usize) {
        self.world.pass[cell] = true;
        self.msg = "opened the chest".to_owned();
        platform::log_info("[tiled_map] 宝箱打开，该格转为可走");
        let open = tile_rect(tile::CHEST_OPEN);
        self.tasks.schedule(
            sprite,
            Tween::<f32>::new(
                vec![
                    Keyframe::new(0, 1.0),
                    Keyframe::new(2, 0.25),
                    Keyframe::new(4, 1.0),
                    Keyframe::new(6, 0.25),
                    Keyframe::new(8, 1.0),
                ],
                |s, v| s.opacity = *v,
            )
            .on_finish(move |s| s.src = open),
        );
    }

    /// NPC：矩形巡逻，每段之前停一拍（`wait(true)` 独占队列 ⇒ 停拍期间谁也别想插进来）。
    fn npc_tick(&mut self) {
        if self.tasks.pending(self.npc) {
            return;
        }
        let at = self
            .arena
            .get(self.npc)
            .map(|s| s.pos)
            .expect("NPC 在 arena 里");
        let (col, row) = cell_of(at);
        let (x0, y0, x1, y1) = NPC_PATROL;
        // 顺时针：右 → 下 → 左 → 上；到边就换轴，回到起点后停一拍再来。
        let (dir, cells): (u8, u32) = if col < x1 {
            (DIR_RIGHT, (x1 - col) as u32)
        } else if row < y1 {
            (DIR_DOWN, (y1 - row) as u32)
        } else if col > x0 {
            (DIR_LEFT, (col - x0) as u32)
        } else if row > y0 {
            (DIR_UP, (row - y0) as u32)
        } else {
            (DIR_RIGHT, (x1 - x0) as u32)
        };
        let nodes = vec![
            RouteNode::Wait { ticks: NPC_HOLD },
            RouteNode::Move { dir, cells },
        ];
        let pass = self.world.pass.clone();
        self.tasks.schedule(
            self.npc,
            MoveRoute::grid(nodes, TILE, SPEED)
                .walkable(move |x, y| landing_ok(&pass, x, y))
                .wait(true),
        );
    }

    /// 把 `alpha` 折到**设备像素点阵**上：采样发生在物理像素上，所以要让
    /// `prev + alpha × SPEED` 乘倍率之后是整数 —— 即 `alpha × SPEED × scale` 取整。倍率 1 时
    /// 这就是"每拍 `SPEED` px 拆成 1px 台阶"；倍率 2 时一拍走 4 个虚拟像素 = 8 个物理像素，
    /// 插值有 5 个台阶可站（0/2/4/6/8），比只在虚拟格取整的 3 个（0/4/8）更细。
    /// 本示例是 `IntegerFit` ⇒ 倍率恒为整数；非整数倍率（换 `Fit`/`Fill`）下台阶是
    /// `1 / (SPEED × scale)` 个虚拟像素。
    ///
    /// 代价是位移按整物理像素分级（正是像素美术该有的走法）；真亚像素滚动的代价是整张地图
    /// 糊掉再来回晃。前提：`SPEED` 与图块格点都是整数，且角色的位移只来自 walk 节点。
    fn lattice_alpha(alpha: f32, scale: f32) -> f32 {
        (alpha * SPEED * scale).round() / (SPEED * scale)
    }

    /// 把相机折到**设备像素**格点上：`ctx.scale` 是虚拟 → 物理的等比倍率，只有 `cam × scale`
    /// 是整数时，地图的采样相位才不随帧变化（角色因此也恰好钉在屏幕中心，见单测里的推导）。
    fn snap_to_device(v: Vec2, scale: f32) -> Vec2 {
        Vec2::new((v.x * scale).round() / scale, (v.y * scale).round() / scale)
    }

    /// 相机：把角色视觉中心带到视口中心，再夹在地图里（贴边时空出的是地图外，不该滚）。
    fn camera_for(hero: Vec2) -> Vec2 {
        let (vw, vh) = (VIRTUAL.0 as f32, VIRTUAL.1 as f32);
        Vec2::new(
            (hero.x + CHAR_W / 2.0 - vw / 2.0).clamp(0.0, (WORLD_W - vw).max(0.0)),
            (hero.y - CHAR_H / 2.0 - vh / 2.0).clamp(0.0, (WORLD_H - vh).max(0.0)),
        )
    }

    /// 把当前相机打进 viewport 栈：世界层按相机反向平移，HUD 层再平移回去（净投影 =
    /// letterbox 里的虚拟画面），于是 HUD 既不跟着滚，又压在地图之上。
    fn push_camera(&mut self, ctx: &mut FrameCtx) {
        if self.cam_pushed {
            ctx.pop_viewport();
            ctx.pop_viewport();
        }
        let (vw, vh) = (VIRTUAL.0 as f32, VIRTUAL.1 as f32);
        let world = ctx.push_viewport(Viewport::at(-self.cam.x, -self.cam.y, vw, vh));
        debug_assert_eq!(world, WORLD.0, "世界层号与 `WORLD` 的约定不符");
        let hud = ctx.push_viewport(Viewport::at(self.cam.x, self.cam.y, vw, vh));
        debug_assert_eq!(hud, HUD_LAYER, "HUD 层号与 `HUD_LAYER` 的约定不符");
        self.cam_pushed = true;
    }

    /// 自带字体到货后换上（wasm 没有系统字体，HUD 全靠它）。
    fn load_font(&mut self) {
        if self.font_loaded {
            return;
        }
        let Some(bytes) = self.vfs.read(&vpath(FONT)) else {
            return;
        };
        let bundle = FontBundle::from_bytes(&bytes);
        platform::log_info(&format!(
            "[tiled_map] 字体 {:?}（face {}）",
            bundle.family(),
            bundle.face_count()
        ));
        *self.text.bundle_mut() = bundle;
        self.font_loaded = true;
    }

    /// HUD 三行：逻辑时间/相机/格子、合批读数、操作提示与最近一次交互结果。
    fn hud_lines(&self, alpha: f32) -> [String; 3] {
        let (col, row) = cell_of(self.hero_at);
        let (batches, instances) = self.stats.map_or((0, 0), |s| (s.batches, s.instances));
        [
            format!(
                "tick {} a{:.2} cell {},{} cam {:.0},{:.0}",
                self.ticks, alpha, col, row, self.cam.x, self.cam.y
            ),
            format!(
                "batches {batches} quads {instances} sprites {} pages {} tier {:.0}x",
                self.arena.len(),
                self.atlas.live_pages(),
                self.text.raster_scale()
            ),
            if self.font_loaded {
                self.msg.clone()
            } else {
                "loading Inter…".to_owned()
            },
        ]
    }
}

const FONT: &str = "Inter-Regular.ttf";
/// wasm 预取清单（一行一个路径，取自 `web/assets/manifest.txt`）。
#[cfg(target_arch = "wasm32")]
const MANIFEST: &str = "Inter-Regular.ttf\n";

/// 字体从 `Vfs` 读：桌面挂 `DirProvider`，wasm 由宿主 `fetch` 灌进 `WebManifest`。
/// 美术不走 VFS —— 它是 `build_sheet` 现画的，于是 native/wasm 的图源路径完全一致。
fn vfs() -> Vfs {
    #[cfg(not(target_arch = "wasm32"))]
    let vfs = {
        let mut vfs = Vfs::new();
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        vfs.register(Box::new(DirProvider::new(
            manifest_dir.join("../../crates/sisf-text/tests/fixtures"),
        )));
        vfs
    };
    #[cfg(target_arch = "wasm32")]
    let vfs = {
        let manifest = Arc::new(WebManifest::new(MANIFEST));
        prefetch(&manifest);
        let mut vfs = Vfs::new();
        vfs.register(Box::new(SharedManifest(manifest)));
        vfs
    };
    vfs
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

impl App for TiledMap {
    fn on_event(&mut self, events: &[EngineEvent], _ctx: &mut FrameCtx) {
        for e in events {
            match e {
                EngineEvent::Key { code, kind } => {
                    let down = matches!(kind, KeyKind::Press | KeyKind::Repeat);
                    if let Some(dir) = Dir::of_code(code) {
                        self.keys.set(dir, down);
                    } else if down && (code == "Enter" || code == "Space" || code == "NumpadEnter")
                    {
                        self.decide = true;
                    } else if code == "Escape" {
                        self.keys.clear();
                        self.msg = "keys cleared".to_owned();
                    }
                }
                EngineEvent::Lifecycle(l) => match l {
                    // 失焦/挂起必须放掉按住的方向键，否则回来之后角色朝一个方向磨墙。
                    Lifecycle::Suspended
                    | Lifecycle::Focused(false)
                    | Lifecycle::CloseRequested => {
                        self.keys.clear();
                    }
                    Lifecycle::Resumed | Lifecycle::Focused(true) => {}
                },
            }
        }
    }

    fn tick(&mut self, dt: Duration, ctx: &mut FrameCtx) {
        self.ticks += 1;
        if ctx.physical_size != self.phys {
            self.phys = ctx.physical_size;
            let v = ctx.viewport;
            platform::log_info(&format!(
                "[tiled_map] 物理 {}x{} ⇒ 画面 ({:.0},{:.0}) {:.0}x{:.0}，倍率 {:.2}，光栅档 {:.0}",
                ctx.physical_size.0,
                ctx.physical_size.1,
                v.x,
                v.y,
                v.width,
                v.height,
                ctx.scale,
                ctx.raster_scale()
            ));
        }
        // 文字光栅档位跟着倍率取（T8d）：HUD 在 2×/4× 下发不虚就靠它。
        self.text.set_raster_scale(ctx.raster_scale());
        self.player_tick();
        self.npc_tick();
        self.tasks.tick(&mut self.arena);
        // 时间驱动的动画（灯火）与移动驱动的步帧互不干涉：`tick_anims` 只碰 `pattern: None`。
        self.arena.tick_anims(dt);
        if let Some(h) = self.arena.get(self.hero) {
            self.hero_at = h.pos;
        }
        let cell = cell_of(self.hero_at);
        if cell != self.at_cell {
            self.at_cell = cell;
            let target = Self::camera_for(self.hero_at);
            platform::log_info(&format!(
                "[tiled_map] 角色到格 {},{}（相机 {:.0},{:.0}）",
                cell.0, cell.1, target.x, target.y
            ));
        }
    }

    /// 把相机打到投影栈上 —— 必须在 `prepare` 而不是 `tick`：这一刻才拿得到本帧的 `alpha`，
    /// 而下一行就要把栈烘进 `FrameU`。相机取角色**真正被画出来的那个点**
    /// （`interpolated(lattice_alpha(alpha))`），两边同相位，角色才不会相对地图每拍回拽
    /// `(1 - alpha) × SPEED` 像素（第一次报回的抖动）；而这个点还须落在像素格点上，否则整张
    /// 地图跟着亚像素晃（第二次报回的抖动）。
    fn prepare(&mut self, alpha: f32, ctx: &mut FrameCtx) {
        let scale = ctx.scale;
        let a = Self::lattice_alpha(alpha, scale);
        self.draw_alpha = a;
        let hero = self
            .arena
            .get(self.hero)
            .map_or(self.hero_at, |h| h.interpolated(a));
        self.cam = Self::snap_to_device(Self::camera_for(hero), scale);
        self.push_camera(ctx);
    }

    fn draw(&mut self, _alpha: f32, renderer: &mut Renderer) {
        // 与 `prepare` 推相机用的是同一个点阵系数：投影与 sprite 只能同相位、同格点。
        let alpha = self.draw_alpha;
        self.frame += 1;
        self.load_font();
        // 槽容量配到精灵数：整帧一片，批数才是纯合批结果（只设一次，避免重建缓冲）。
        let n = self.arena.len().max(1) as u32;
        if renderer.quads_per_slot() != n {
            renderer.set_quads_per_slot(n);
        }
        self.atlas.touch(self.proto.epoch(), self.frame);
        renderer.apply_atlas(&self.atlas, &mut self.pages, &mut self.uploads);

        let vw = VIRTUAL.0 as f32;
        let vh = VIRTUAL.1 as f32;

        // 世界：一次采集 ⇒ 一层投影 ⇒ 按 (epoch, blend, z) 排好的三层。
        let mut list = BatchList::new();
        if self.arena.collect_at(WORLD_VIEW, alpha, &mut list).is_err() {
            platform::log_info("[tiled_map] 采集失败（本阶段的图块不该有 tone）");
            return;
        }
        // 相机层得自己裁剪：letterbox 只改投影、不改 scissor，于是平移出画面的那部分地图
        // 照原样画进黑边里（800×600 的 1× 下，地图西侧石墙正好落在 128px 边条上）。
        // 裁剪盒用世界层自己的坐标：这一层的 (cam.x, cam.y) 才对应画面的左上角。
        renderer.push_clip(u32::from(WORLD.0), [self.cam.x, self.cam.y, vw, vh]);
        renderer.append_list(list);
        renderer.pop_clip();

        // HUD 在 `HUD_LAYER`：净投影 = 虚拟画面（见 `push_camera`），既不跟着滚又能盖住地图。
        renderer.push_clip(u32::from(HUD_LAYER), [0.0, 0.0, vw, 64.0]);
        renderer.push(
            BatchKey {
                layer: HUD_LAYER,
                clip: NO_CLIP,
                epoch: WHITE_EPOCH,
                blend: Blend::Normal,
                z: -1,
                insertion: 0,
            },
            Instance::rect(
                [vw / 2.0, 32.0],
                [vw, 64.0],
                [0.0, 0.0, 0.0, 0.55],
                HUD_LAYER,
            ),
        );
        for (i, line) in self.hud_lines(alpha).iter().enumerate() {
            self.text.draw_text_at(
                renderer,
                [8.0, 6.0 + i as f32 * 18.0, vw - 16.0, 18.0],
                line,
                &TextStyle::new(13.0).aligned(Align::Left),
                [1.0, 1.0, 1.0, 0.92],
                HUD_LAYER,
            );
        }
        renderer.pop_clip();
        assert_eq!(renderer.clip_depth(), 0, "裁剪栈没成对收回");
        // `stats()` 是本帧之前最后一次 `end_frame` 的统计（上一帧）。
        self.stats = renderer.stats();
        // `stats()` 是上一次 `end_frame` 的账 ⇒ 第 2 帧才读得到第 1 帧的合成结果。
        // 把 HUD 第二行原样打到控制台：桌面与浏览器的手测都靠它确认"三层同 epoch ⇒ 批数
        // 是个位数"，而不是只看见画面动了。
        if self.frame == 2 {
            platform::log_info(&format!(
                "[tiled_map] 第 1 帧合成: {}",
                self.hud_lines(alpha)[1]
            ));
        }
        // 主帧里新长出来的字形页，赶在 runner 收帧之前注册。
        self.text.sync(renderer);
    }
}

fn config() -> AppConfig {
    AppConfig {
        title: "SISF tiled_map (T8)".to_owned(),
        virtual_size: VIRTUAL,
        // 整数倍缩放：与"无半像素抖动"那条验收同源（边条取整在 `FrameCtx::recompute`）。
        scale_mode: ScaleMode::IntegerFit,
        clear: [0.04, 0.06, 0.09, 1.0],
        ..Default::default()
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn sisf_start() {
    real_main();
}

pub fn main() {
    real_main();
}

fn real_main() {
    platform::log_info("[tiled_map] main 已启动");
    if let Err(e) = platform::run(TiledMap::new(), config()) {
        panic!("tiled_map 运行失败: {e}");
    }
}

// ---------------------------------------------------------------------- 单测
//
// 例子里能自动化的部分都在这里：地图形状、通行表、三层 z 的合成序、格点阵往返、相机钳制、
// 位图区间与角色图样。跑法 `cargo test -p sisf --example tiled_map`（`[[example]]` 里
// `test = true`）。剩下的是画面与手感，走文件头的手测清单。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_is_a_rectangle_of_known_chars() {
        let w = World::new();
        assert_eq!(w.cells.len(), MAP_W * MAP_H);
        for line in MAP {
            assert_eq!(chunk_count(line), 4, "每行都该是 4 段: {line:?}");
        }
        // 未知字符在 `spec` 里就 panic 了；这里再确认边框封死（否则相机夹不住、角色出界）。
        for col in 0..MAP_W as i32 {
            for row in [0, MAP_H as i32 - 1] {
                assert!(!w.is_walkable(col, row), "第 {row} 行边框可走");
            }
        }
        for row in 0..MAP_H as i32 {
            for col in [0, MAP_W as i32 - 1] {
                assert!(!w.is_walkable(col, row), "第 {col} 列边框可走");
            }
        }
    }

    #[test]
    fn spawns_and_patrol_are_on_walkable_ground() {
        let w = World::new();
        assert!(w.is_walkable(HERO_CELL.0, HERO_CELL.1), "出生点不能是墙");
        assert!(w.is_walkable(NPC_CELL.0, NPC_CELL.1), "NPC 起点不能是墙");
        let (x0, y0, x1, y1) = NPC_PATROL;
        for row in y0..=y1 {
            for col in x0..=x1 {
                assert!(w.is_walkable(col, row), "巡逻矩形里 ({col},{row}) 不可走");
                // 矩形内不该有物体：否则 NPC 巡逻会被自己的地图卡住。
                assert_eq!(w.cell(col, row).unwrap().obj, -1, "({col},{row}) 上有物体");
            }
        }
    }

    /// 图块 32 / 角色 48 的格点阵往返：锚点取左下角，一次除法就回到格子号。
    #[test]
    fn lattice_and_cell_round_trip() {
        for row in 0..MAP_H as i32 {
            for col in 0..MAP_W as i32 {
                assert_eq!(
                    cell_of(lattice(col, row)),
                    (col, row),
                    "48 高角色的锚点不该错行"
                );
            }
        }
        // 逐格移动的中途（离墙还有半格）归到已经 committed 的那一格，不该跳回起点。
        let at = lattice(6, 7);
        assert_eq!(cell_of(at - Vec2::new(0.0, 8.0)), (6, 7));
        assert_eq!(cell_of(at - Vec2::new(8.0, 0.0)), (6, 7));
    }

    #[test]
    fn camera_centers_then_clamps_to_the_map() {
        let max = Vec2::new(WORLD_W - VIRTUAL.0 as f32, WORLD_H - VIRTUAL.1 as f32);
        // 地图中部：角色视觉中心（脚底往上半格）落在视口中心。
        let at = lattice(12, 12);
        assert_eq!(
            TiledMap::camera_for(at),
            Vec2::new(at.x + 16.0 - 272.0, at.y - 24.0 - 208.0)
        );
        // 四角：夹在 `[0, world - virtual]`，不许滚出地图。
        assert_eq!(TiledMap::camera_for(lattice(1, 1)), Vec2::ZERO);
        assert_eq!(
            TiledMap::camera_for(lattice(MAP_W as i32 - 2, MAP_H as i32 - 2)),
            max
        );
        // 贴北边 ⇒ 只有 y 被夹住（x 仍在"角色居中"的 128）；贴西边同理，反过来。
        assert_eq!(TiledMap::camera_for(lattice(12, 1)), Vec2::new(128.0, 0.0));
        assert_eq!(TiledMap::camera_for(lattice(1, 12)), Vec2::new(0.0, 184.0));
    }

    /// 列主序矩阵乘列向量（和 `FrameCtx` 的投影同一条通路，这里只用来对答案）。
    fn xform(m: &[[f32; 4]; 4], v: [f32; 4]) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        for (i, o) in out.iter_mut().enumerate() {
            *o = (0..4).map(|j| m[j][i] * v[j]).sum();
        }
        out
    }

    /// 相机推两层：世界层跟着相机走，HUD 层把平移抵消回去 ⇒ HUD 不滚、且层号在世界上方。
    ///
    /// 世界点 (cam.x, cam.y) 落在画面左上角（下表里的 `[0,0]` 那一项）—— 这正是
    /// `draw` 里裁剪盒取 `[cam.x, cam.y, vw, vh]` 而不是 `[0, 0, vw, vh]` 的原因。
    ///
    /// 这两条各对应一个真实缺陷：只推一层时 HUD 落在 layer 0，被整层地图盖住（`BatchKey`
    /// 先按 layer 排，z 再大也没用）；而 layer 1 的画面没人裁剪，地图边缘会画进 letterbox。
    #[test]
    fn the_hud_layer_nets_out_the_camera_and_outranks_the_world() {
        let mut app = TiledMap::new();
        app.cam = Vec2::new(128.0, 64.0);
        let mut ctx = FrameCtx::new(&config(), (1088, 832), 1.0);
        app.push_camera(&mut ctx);
        app.push_camera(&mut ctx); // 第二拍：pop/push 成对 ⇒ 栈不能越长越高
        assert_eq!(ctx.viewport_depth(), 3, "相机层没成对收回");
        assert_eq!(ctx.layer(), HUD_LAYER);
        let f = ctx.frame_u(0.0);
        for p in [[0.0, 0.0], [272.0, 208.0], [543.0, 415.0]] {
            let at = [p[0], p[1], 0.0, 1.0];
            let world = xform(&f.proj[WORLD.0 as usize], at);
            let behind = xform(&f.proj[0], [p[0] - app.cam.x, p[1] - app.cam.y, 0.0, 1.0]);
            let hud = xform(&f.proj[HUD_LAYER as usize], at);
            let base = xform(&f.proj[0], at);
            for axis in 0..4 {
                assert!(
                    (world[axis] - behind[axis]).abs() < 1e-4,
                    "世界层没跟着相机走: {world:?} vs {behind:?}"
                );
                assert!(
                    (hud[axis] - base[axis]).abs() < 1e-4,
                    "HUD 层被相机带着滚了: {hud:?} vs {base:?}"
                );
            }
        }
    }

    /// 第一次报回的抖动（"人物一直在抖"）回归：相机得跟着角色**真正被画出来的那个点**
    /// （`interpolated(lattice_alpha(alpha))`）走，而不是拍尾位置 —— 两边同相位，角色在画面上
    /// 的位置才不会随 `alpha` 来回跳。
    ///
    /// `App::prepare` 的存在理由：投影在 `tick` 之后、`draw` 之前烘进 `FrameU`，所以相机推到
    /// `tick` 末就是钉死在拍尾，角色每拍被拽回 `(1 - alpha) × 每拍位移` 像素再弹回去。
    #[test]
    fn the_following_camera_shares_the_hero_s_interpolation_phase() {
        let mut app = TiledMap::new();
        let mut ctx = FrameCtx::new(&config(), (1088, 832), 1.0);
        // 一步 = 一拍的真实位移（`MoveRoute::walk` 每拍挪 `SPEED` px），且相机两端不碰 clamp。
        let from = lattice(12, 12);
        let to = Vec2::new(from.x + SPEED, from.y);
        let hero = app.hero;
        app.arena.get_mut(hero).unwrap().teleport(from);
        app.arena.get_mut(hero).unwrap().set_pos(to);

        let mut first: Option<(f32, f32)> = None;
        for alpha in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            app.prepare(alpha, &mut ctx);
            let drawn = app
                .arena
                .get(hero)
                .unwrap()
                .interpolated(TiledMap::lattice_alpha(alpha, ctx.scale));
            let f = ctx.frame_u(0.0);
            let p = xform(&f.proj[WORLD.0 as usize], [drawn.x, drawn.y, 0.0, 1.0]);
            let ndc = (p[0] / p[3], p[1] / p[3]);
            match first {
                None => first = Some(ndc),
                Some(want) => assert!(
                    (ndc.0 - want.0).abs() < 1e-6 && (ndc.1 - want.1).abs() < 1e-6,
                    "alpha {alpha} 下角色在屏上挪了：{ndc:?} vs {want:?}"
                ),
            }
        }

        // 对照组：把相机钉在拍尾（旧写法）时角色在屏上挪多少 —— 每拍最多 `SPEED` 虚拟像素，
        // 正是用户看到的抖动。留着它才说明上面那条"同相位"断言不是空转出来的。
        // （1088×832 配 544×416 是干净的 2×、无边条，于是 NDC × (虚拟宽 / 2) 就是虚拟像素。）
        let mut pinned = TiledMap::new();
        pinned.cam = TiledMap::camera_for(to);
        let mut pctx = FrameCtx::new(&config(), (1088, 832), 1.0);
        pinned.push_camera(&mut pctx);
        let pf = pctx.frame_u(0.0);
        let on_screen = |alpha: f32| {
            let d = from.lerp(to, alpha);
            let p = xform(&pf.proj[WORLD.0 as usize], [d.x, d.y, 0.0, 1.0]);
            p[0] / p[3] * (VIRTUAL.0 as f32 / 2.0)
        };
        let rest = on_screen(1.0);
        for alpha in [0.0f32, 0.5] {
            let drift = (rest - on_screen(alpha)).abs();
            assert!(
                (drift - (1.0 - alpha) * SPEED).abs() < 1e-3,
                "拍尾相机的错位该是 {} 虚拟像素，实际 {drift}",
                (1.0 - alpha) * SPEED
            );
        }
    }

    /// 第二次报回的抖动（"整个地图都在抖，左右移动时左右抖"）回归：把相机改成跟随
    /// `interpolated(alpha)` 之后，角色确实不跳了，可相机自身的平移量变成了亚像素 ——
    /// 地图那些整数格点被搬到纹素中间重采样，整屏的硬边缘随 `alpha` 呼吸，于是抖动从
    /// "角色 relative 地图"变成"地图 relative 屏幕"。修法是让插值只落在**像素点阵**上
    /// （`lattice_alpha`）：一拍 `SPEED = 2` px ⇒ 两个 1px 台阶，角色与地图同时在格点上。
    #[test]
    fn the_following_camera_snaps_the_world_to_the_pixel_lattice() {
        let mut app = TiledMap::new();
        // 倍率 1（544×416）：这一档虚拟格点 == 物理格点，先单独钉住"相机不能在亚像素上"。
        // 倍率 ≥ 2 时两者差 `scale` 倍，那是下面 `..._device_pixel_grid_...` 那条的职责。
        let mut ctx = FrameCtx::new(&config(), (544, 416), 1.0);
        assert_eq!(ctx.scale, 1.0);
        let from = lattice(12, 12);
        let to = Vec2::new(from.x + SPEED, from.y);
        let hero = app.hero;
        app.arena.get_mut(hero).unwrap().teleport(from);
        app.arena.get_mut(hero).unwrap().set_pos(to);

        let on_grid = |v: Vec2| v.x == v.x.round() && v.y == v.y.round();
        let mut steps = Vec::new();
        for alpha in [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0] {
            app.prepare(alpha, &mut ctx);
            // 相机层把世界点整体平移 `-cam`：cam 不在格点上，地图纹素就一律落在两个物理
            // 像素正中（每一行边缘同时糊一次），这一帧之后再恢复 —— 满屏晃的出处。
            assert!(
                on_grid(app.cam),
                "alpha {alpha} 下相机离开了格点: {:?}",
                app.cam
            );
            let d = app
                .arena
                .get(hero)
                .unwrap()
                .interpolated(TiledMap::lattice_alpha(alpha, ctx.scale));
            assert!(on_grid(d), "alpha {alpha} 下角色画在格点外: {d:?}");
            steps.push(d.x - from.x);
        }
        // 台阶不得倒退（那就是锯齿），也不得是 0→2 一跳（那等于把插值整个丢掉）。
        assert!(
            steps.windows(2).all(|w| w[1] >= w[0]),
            "点阵台阶倒退了: {steps:?}"
        );
        let mut uniq: Vec<f32> = steps.clone();
        uniq.dedup();
        assert_eq!(
            uniq,
            vec![0.0, 1.0, 2.0],
            "一拍该走两个 1px 台阶: {steps:?}"
        );

        // 对照组：不取整时 alpha 0.3 就把相机推到格点外（`0.3 × 2 = 0.6` px）。
        let raw = app.arena.get(hero).unwrap().interpolated(0.3);
        assert!(
            !on_grid(TiledMap::camera_for(raw)),
            "对照组失效：未取整的相机本来该落在格点外，实际 {raw:?}"
        );
    }

    /// 上一条只覆盖倍率 1。**采样发生在物理像素上**，所以格点也得是物理格点：`lattice_alpha`
    /// 与 `snap_to_device` 取整的都是 `× scale` 之后的坐标。倍率 2 下按虚拟像素取整等于把台阶
    /// 粗了一倍（一拍 4 个物理像素只能踩 0/2/4 三个点），地图每帧要么不动要么跳两格 —— 角色被
    /// 相机钉死在屏幕正中，于是这种"粗"全部落在背景上。（第三次报回的"人物不抖背景抖"另有真因
    /// = `MoveRoute` 的收尾拍，见下面那条节拍回放单测；本条只是顺带把格点推广对。）
    #[test]
    fn the_camera_lattice_is_the_device_pixel_grid_not_the_virtual_one() {
        let mut app = TiledMap::new();
        // 1088×832 配 544×416 ⇒ `IntegerFit` 正好 2×。
        let mut ctx = FrameCtx::new(&config(), (1088, 832), 1.0);
        assert_eq!(ctx.scale, 2.0);
        let from = lattice(12, 12);
        let to = Vec2::new(from.x + SPEED, from.y);
        let hero = app.hero;
        app.arena.get_mut(hero).unwrap().teleport(from);
        app.arena.get_mut(hero).unwrap().set_pos(to);

        let scale = ctx.scale;
        let alphas = [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0];
        let mut cam_dev = Vec::new();
        let mut hero_phase = Vec::new();
        let mut hero_on_screen = Vec::new();
        for alpha in alphas {
            app.prepare(alpha, &mut ctx);
            let c = app.cam.x * scale;
            assert!(
                c == c.round(),
                "alpha {alpha} 下相机不在物理格点上: cam={:?} ×{scale}={c}",
                app.cam
            );
            cam_dev.push(c);
            let d = app
                .arena
                .get(hero)
                .unwrap()
                .interpolated(TiledMap::lattice_alpha(alpha, scale));
            let dev = d.x * scale;
            assert!(
                dev == dev.round(),
                "alpha {alpha} 下角色不在物理格点上: {d:?} ×{scale}={dev}"
            );
            hero_phase.push(dev - dev.round());
            hero_on_screen.push((d.x - app.cam.x) * scale);
        }
        // 角色的物理相位一动不动（贴图不会每帧换采样点），且它相对画面钉死。
        let spread = |v: &[f32]| {
            v.iter().cloned().fold(f32::MIN, f32::max) - v.iter().cloned().fold(f32::MAX, f32::min)
        };
        assert!(
            spread(&hero_phase) < 1e-4,
            "角色的物理相位在漂: {hero_phase:?}"
        );
        assert!(
            spread(&hero_on_screen) < 1e-3,
            "角色在屏幕上被相机甩了: {hero_on_screen:?}"
        );
        // 一拍 `SPEED × scale = 4` 个物理像素，被拆成 0/1/2/3/4 五级台阶 —— 物理格点能给的
        // 最细走法。对照组：按虚拟像素取整（上一版的 `round(alpha × SPEED) / SPEED`）在同一拍
        // 里只给 0/2/4，一半的物理像素位置被白白跳过。
        let mut uniq: Vec<f32> = cam_dev.clone();
        uniq.dedup();
        assert_eq!(
            uniq,
            vec![256.0, 257.0, 258.0, 259.0, 260.0],
            "一拍该有 5 个物理台阶: {cam_dev:?}"
        );
        let virtual_lattice: Vec<f32> = alphas
            .iter()
            .map(|a| (a * SPEED).round() * scale + cam_dev[0])
            .collect();
        let mut coarse = virtual_lattice.clone();
        coarse.dedup();
        assert_eq!(
            coarse,
            vec![256.0, 258.0, 260.0],
            "对照组失效：虚拟格点本该只给 0/2/4"
        );
    }

    /// 第三次报回的抖动（"还是抖，人物不抖背景抖"）回归：把浏览器实测的乱拍（54fps 上下，
    /// 12/18/24ms 交替）喂进 `TickClock`，逐帧读相机的物理位置。
    ///
    /// 判据不是"每帧位移相等"（帧率本身在抖，做不到），而是**相机始终贴着理想位置**：
    /// `|cam − 理想位移| ≤ 2` 个物理像素（那 2px 是"画的是上一拍的结果"带来的固定滞后），
    /// 且从不回退。角色被相机钉在屏幕正中，所以这条曲线的误差就是用户看到的背景抖动量。
    ///
    /// 出处是 `MoveRoute` 的收尾拍：路线跑完的那一拍不当场报告完成，下一拍 `prev_pos`/`pos`
    /// 原地不动而 `alpha` 照旧归零重数 ⇒ 每 17 拍白停一拍，误差读到 12px、平均速度掉 9%。
    #[test]
    fn the_camera_tracks_the_ideal_scroll_under_a_jittery_frame_rate() {
        let mut app = TiledMap::new();
        let mut ctx = FrameCtx::new(&config(), (544, 416), 1.0);
        let scale = ctx.scale;
        app.keys.set(Dir::Up, true);
        let mut clock = sisf::app::TickClock::new(60.0);
        let period = clock.period();
        // 一次 rAF 采样的实测节拍：18.2ms 为主，掺 12.1/24.3ms（均值 17.7ms ⇒ 56.5fps）。
        let cadence = [
            18.2, 12.1, 18.2, 24.3, 18.2, 12.1, 18.2, 18.2, 24.3, 18.2, 12.1, 18.2,
        ];
        let mut seq = Vec::new();
        let mut times = Vec::new();
        let mut t = 0.0f64;
        for i in 0..84 {
            t += cadence[i % cadence.len()] / 1000.0;
            times.push(t);
            for _ in 0..clock.step(Duration::from_secs_f64(cadence[i % cadence.len()] / 1000.0)) {
                app.tick(period, &mut ctx);
            }
            app.prepare(clock.alpha(), &mut ctx);
            seq.push(app.cam.y * scale);
        }
        assert!(
            seq.windows(2).all(|w| w[1] <= w[0]),
            "相机回退了（地图朝反方向弹一下）: {seq:?}"
        );
        let worst = (0..seq.len())
            .map(|k| ((seq[k] - seq[0]) as f64 + (120.0 * times[k]).round()).abs())
            .fold(0.0f64, f64::max);
        assert!(
            worst <= 2.0,
            "相机离理想滚动位置最远 {worst} 物理像素（该 ≤2，即一拍的固定滞后）"
        );
        // 位移总量也得对得上：84 帧 = 1.486s ⇒ 理想 178px，收尾拍那一档只走出 166px。
        let moved = seq[0] - seq[seq.len() - 1];
        let ideal_total = (120.0 * t).round() as f32;
        assert!(
            (moved - ideal_total).abs() <= 2.0,
            "{t:.3}s 里只滚了 {moved}px，理想 {ideal_total}px"
        );
    }

    /// 三层 + y 排序：同一 epoch 之下 z 才有决定权，这里合成一批并读出画序。
    #[test]
    fn the_three_layers_order_as_ground_decal_y_sort_above() {
        let mut arena = SpriteArena::new();
        // 无位图 ⇒ 全场 WHITE_EPOCH ⇒ 恰一批，`build` 之后的序就是画序。
        let placed = [
            (lattice(4, 5), Z_GROUND),
            (lattice(4, 6), Z_BELOW),
            (lattice(5, 6), Z_STAND),
            (lattice(4, 7), Z_STAND),
            (lattice(4, 5), Z_ABOVE),
        ];
        for (i, (pos, band)) in placed.iter().enumerate() {
            let mut s = Sprite::solid(Vec2::new(TILE, TILE));
            s.layer = WORLD;
            s.teleport(*pos);
            // 事件带按脚底 y 排：z 的个位留给"同格内谁先 spawn"，不影响带的顺序。
            s.z = *band + pos.y as i32 + i as i32 / 1000;
            arena.spawn(s);
        }
        let mut list = BatchList::new();
        arena.collect(WORLD_VIEW, &mut list).unwrap();
        assert_eq!(list.len(), placed.len());
        let batches = list.build(64);
        assert_eq!(
            batches.len(),
            1,
            "同 epoch 同 blend 必须合成一批，否则 z 序只在页内成立"
        );
        let order: Vec<i32> = list.sorted_items().iter().map(|(k, _)| k.z).collect();
        let expect: Vec<i32> = placed
            .iter()
            .map(|&(pos, band)| band + pos.y as i32)
            .collect();
        let mut sorted = expect.clone();
        sorted.sort_unstable();
        assert_eq!(order, sorted, "地面 < 贴地 < 按脚底 y 的事件与角色 < 上方");
        // 显式写死形状，免得 `placed` 一改就 quietly 验不到东西。
        assert_eq!(
            order[0..3],
            [Z_GROUND + 192, Z_BELOW + 224, Z_STAND + 224,],
            "南边（脚底 y 大）的角色/事件要盖住北边的"
        );
        assert_eq!(order[4], Z_ABOVE + 192, "上方层压住全场");
    }

    /// 事件格碰撞：路线撞墙只作废本节点剩余格数，人停在墙前（读的是地图真数据）。
    #[test]
    fn move_route_stops_at_the_map_collision_table() {
        let w = World::new();
        let mut arena = SpriteArena::new();
        // (12,4) 往东 4 格：13/14/15 是路面，16 是屋墙 ⇒ 停 (15,4)，后面的 Turn 照做。
        let mut s = Sprite::solid(Vec2::new(CHAR_W, CHAR_H));
        s.layer = WORLD;
        s.anchor = Vec2::new(0.0, 1.0);
        s.teleport(lattice(12, 4));
        // 朝向是**动画状态**：没挂方向图样的精灵连 `Turn` 都是 no-op，这里要观察的正是
        // "路线 → 朝向/步帧"那条接线，所以给它一套和玩家同款的 MV 图样（帧表是纯数字，不需要位图）。
        s.set_animation(Some(AnimState::new(Arc::new(AnimDef::grid_pattern(
            DirPattern::RpgMaker4x3,
            CHAR_W,
            CHAR_H,
            0,
        )))));
        let hero = arena.spawn(s);
        let mut sched = TaskScheduler::new();
        let pass = w.pass.clone();
        sched.schedule(
            hero,
            MoveRoute::grid(
                vec![
                    RouteNode::Move {
                        dir: DIR_RIGHT,
                        cells: 4,
                    },
                    RouteNode::Turn { dir: DIR_UP },
                ],
                TILE,
                TILE,
            )
            .walkable(move |x, y| landing_ok(&pass, x, y)),
        );
        for _ in 0..8 {
            sched.tick(&mut arena);
        }
        let s = arena.get(hero).unwrap();
        assert_eq!(cell_of(s.pos), (15, 4), "撞墙那一格没走成");
        assert_eq!(s.dir(), Some(DIR_UP), "墙不该把后面的指令一起掐掉");

        // 桥：正西 (1,15) 往东 6 格，桥面全可走 ⇒ 走到 (7,15)。
        let mut arena2 = SpriteArena::new();
        let mut b = Sprite::solid(Vec2::new(CHAR_W, CHAR_H));
        b.layer = WORLD;
        b.anchor = Vec2::new(0.0, 1.0);
        b.teleport(lattice(1, 15));
        let walker = arena2.spawn(b);
        let mut sched2 = TaskScheduler::new();
        let pass2 = w.pass.clone();
        sched2.schedule(
            walker,
            MoveRoute::grid(
                vec![RouteNode::Move {
                    dir: DIR_RIGHT,
                    cells: 6,
                }],
                TILE,
                TILE,
            )
            .walkable(move |x, y| landing_ok(&pass2, x, y)),
        );
        for _ in 0..20 {
            sched2.tick(&mut arena2);
        }
        assert_eq!(
            cell_of(arena2.get(walker).unwrap().pos),
            (7, 15),
            "桥该走过去"
        );

        // 门可去、屋墙不可去：同一个 `above` 屋顶下两种通行结论。
        assert!(w.is_walkable(18, 5), "门格必须可走");
        assert!(!w.is_walkable(19, 5), "门旁的屋墙必须不可走");
        assert_eq!(w.cell(18, 5).unwrap().above, w.cell(19, 5).unwrap().above);
    }

    #[test]
    fn sheet_regions_stay_inside_the_bitmap() {
        for id in 0..=tile::LAST {
            let r = tile_rect(id);
            assert!(
                r.x + r.w <= SHEET.0 as f32 && r.y + r.h <= SHEET.1 as f32,
                "图块 {id} 出了位图"
            );
        }
        for (at, pattern) in [
            (HERO_AT, DirPattern::RpgMaker4x3),
            (NPC_AT, DirPattern::RpgMaker2x2),
        ] {
            for row in 0..pattern.rows() {
                for col in 0..pattern.cols() {
                    let r = char_rect(at, row, col);
                    assert!(
                        r.x + r.w <= SHEET.0 as f32 && r.y + r.h <= SHEET.1 as f32,
                        "角色格 {r:?} 出了位图"
                    );
                }
            }
        }
        assert!(
            char_rect(HERO_AT, 0, 0).x + CHAR_W <= NPC_AT[0],
            "两条角色带重叠了"
        );
        // `DirPattern::frame_index` 直接下标帧表 ⇒ 图样与位图区间必须一致。
        let p = DirPattern::RpgMaker4x3;
        assert_eq!(
            char_rect(HERO_AT, 3, 1),
            char_rect(
                HERO_AT,
                DirPattern::dir_row(DIR_UP),
                p.frame_index(DIR_UP, 1) % p.cols()
            ),
            "上行第 2 帧必须在角色带第 3 行第 1 列"
        );
    }

    #[test]
    fn generated_sheet_has_premultiplied_alpha_and_empty_margins() {
        let px = build_sheet();
        assert_eq!(px.len(), (SHEET.0 * SHEET.1 * 4) as usize);
        for chunk in px.chunks_exact(4) {
            let a = chunk[3];
            assert!(a == 0 || a == 255 || a == 90, "只该三档 alpha: {a}");
            if a < 255 {
                assert!(chunk[..3].iter().all(|&v| v <= a), "非预乘: {chunk:?}");
            }
        }
        // 用过的格子之外必须全空：越界作画 = 采样到隔壁子图。
        let at = |x: u32, y: u32| {
            let i = ((y * SHEET.0 + x) * 4) as usize;
            &px[i..i + 4]
        };
        assert_eq!(
            at(255, 100),
            &[0, 0, 0, 0][..],
            "角色带右侧的空白区被画到了"
        );
        assert_eq!(at(240, 80), &[0, 0, 0, 0][..], "第 23 号空格该全空");
        // 玩家带宽 3×32 = 96、NPC 带从 x = 128 起 ⇒ 两条带之间那条竖直空档谁都不许碰。
        assert_eq!(
            at(100, 250),
            &[0, 0, 0, 0][..],
            "两条角色带之间的空档被画到了"
        );
        // 步帧与朝向必须在**画面上**成立：否则 `set_step`/`set_dir` 只是状态机里的数字。
        let frame = |col: u32, row: u32| {
            let r = char_rect(HERO_AT, row, col);
            let (x, y) = (r.x as u32, r.y as u32);
            let mut v = Vec::new();
            for j in 0..CHAR_H as u32 {
                for i in 0..CHAR_W as u32 {
                    v.extend_from_slice(at(x + i, y + j));
                }
            }
            v
        };
        assert!(
            frame(0, 0).iter().any(|&b| b != 0),
            "玩家朝下站立帧该有内容"
        );
        assert_ne!(frame(0, 0), frame(1, 0), "站立帧与迈步帧必须不同");
        assert_ne!(frame(0, 0), frame(0, 3), "朝下与朝上的脸必须不同");
    }

    /// 侧面行走图必须在**像素**上分得清左右：脸和眼睛在朝向那半边，头发盖住后脑那半边。
    ///
    /// 第一版把头发矩形画在朝向侧、正好盖掉刚画好的眼睛（先画眼睛后画头发），于是"朝左"那格
    /// 露出的脸在右边 —— 用户读到的就是"左右贴图反了"。行序 `下/左/右/上` 本身没错，
    /// 错的是自绘素材，所以这条断言只能钉在像素上。
    #[test]
    fn the_side_walk_frames_actually_look_left_and_right() {
        let px = build_sheet();
        let at = |x: u32, y: u32| {
            let i = ((y * SHEET.0 + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        let ink = [0u8, 0, 0, 255];
        let hair = [58u8, 40, 28, 255];
        for (band, who) in [(HERO_AT, "玩家"), (NPC_AT, "NPC")] {
            for (row, left) in [(1u32, true), (2, false)] {
                let r = char_rect(band, row, 0);
                let (x, y) = (r.x as u32, r.y as u32);
                // 眼睛 y ∈ [14,17)：朝向那侧一颗，背面那半边一颗都不许有。
                let (front, back) = if left { (9, 21) } else { (21, 9) };
                assert_eq!(
                    at(x + front, y + 15),
                    ink,
                    "{who} 行 {row} 的眼睛不在朝向那侧"
                );
                assert_ne!(at(x + back, y + 15), ink, "{who} 行 {row} 两张侧脸分不出来");
                // 后脑那半边得是头发（画在眼睛之前 ⇒ 反了顺序就把眼睛盖掉，见上）。
                let behind_eye = if left {
                    (x + 20, y + 15)
                } else {
                    (x + 11, y + 15)
                };
                assert_eq!(
                    at(behind_eye.0, behind_eye.1),
                    hair,
                    "{who} 行 {row} 的头发没盖住后脑"
                );
            }
        }
    }

    #[test]
    fn held_keys_keep_the_last_pressed_and_release_clears_them() {
        let mut keys = Held::default();
        keys.set(Dir::Left, true);
        keys.set(Dir::Up, true);
        assert_eq!(keys.latest(), Some(Dir::Up), "后按下的方向优先");
        keys.set(Dir::Up, false);
        assert_eq!(keys.latest(), Some(Dir::Left));
        // 连发（Repeat）是同一个按下态的重述，不该在序里插两份。
        keys.set(Dir::Left, true);
        assert_eq!(keys.order.len(), 1);
        keys.clear();
        assert_eq!(keys.latest(), None);
        assert_eq!(Dir::of_code("KeyW"), Some(Dir::Up));
        assert_eq!(Dir::of_code("ArrowLeft"), Some(Dir::Left));
        assert_eq!(Dir::of_code("Escape"), None);
        assert_eq!(Dir::Up.unit(), dir_vector(DIR_UP));
        assert_eq!(Dir::from_rpg(DIR_RIGHT), Dir::Right);
    }
}
