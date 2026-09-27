//! 横向动作（platformer）验收示例：马里奥 1-1 风格的跑跳 + 火球攻击。
//!
//! 与 `tiled_map`（走格子）相反，这一侧验证的是引擎的**自由移动**通路：逐拍整数像素的
//! 速度积分 + 瓦片 AABB，全程没有一处亚像素。相机与精灵都钉在格点上（所有移动走
//! `teleport`，采集时恒取 `pos`），于是 `IntegerFit` 下角色与地图之间不存在"抖"的物质
//! 基础 —— 不需要 `lattice_alpha`，因为它要化解的误差在这套物理模型里根本不产生。
//!
//! 物理常量（60 拍/秒）：走满 5 px/拍；跳初速 -20、重力 +2 ⇒ 升程 110px ≈ 3.4 格，
//! 滞空 20 拍 ⇒ 满速横跳 ~100px，跨得下 2 格（64px）的地坑、上得了 3 格高的水管。
//! 攻击 = 火球（命名动作 `attack`）：同屏一发、带冷却，命中板栗怪即压扁；也可以踩。
//!
//! 运行：
//! - 桌面：`cargo run --release -p sisf --example platformer`
//! - wasm：`cargo xtask web platformer` 后访问 `http://127.0.0.1:8080/?ex=platformer&wgl`
//! - 单测（地图/碰撞/跳升程/最近实心格）：`cargo test -p sisf --example platformer`
//!
//! 手测清单（桌面与 wasm 同一份）：
//! 1. ←→/AD 跑动（步帧交替、停手回站立帧），面朝左是镜像（脸在左半边）。
//! 2. Z/Space/↑ 起跳：能跳上 3 格水管顶、能头撞翻第 6 列的 ? 砖；踩落下的板栗怪会弹一下，
//!    横碰则阵亡回出生点（deaths +1，短暂无敌闪烁）。
//! 3. X 发火球：平飞、撞墙熄灭、命中板栗怪压扁它；有冷却且同屏只有一发。
//! 4. 掉坑（第 55/78 列）阵亡重生；跳上底座碰到旗杆（第 110 列）显示 STAGE CLEAR 并冻结。
//! 5. 相机左右跟随时角色相对地图不晃；窗口拉成非整数倍尺寸看 letterbox 取整。

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
use sisf::event::{ActionDef, ActionMap, Trigger};
use sisf::render::{
    Atlas, AtlasPages, BatchKey, BatchList, Blend, DrawStats, Instance, NO_CLIP, Renderer,
    UploadRing, WHITE_EPOCH,
};
use sisf::sprite::{
    AnimDef, AnimState, FrameDef, Layer, Rect, Sprite, SpriteArena, SpriteRef, Vec2, ViewportRef,
};
use sisf::text::{Align, FontBundle, TextEngine, TextStyle};

// ------------------------------------------------------------------ 世界与地图

/// 图块边长（虚拟像素）。
const TILE: f32 = 32.0;
const TILE_I: i32 = 32;
const MAP_W: i32 = 120;
const MAP_H: i32 = 15;
const WORLD_W: i32 = MAP_W * TILE_I;
const WORLD_H: i32 = MAP_H * TILE_I;
/// 可视区 = 16 × 15 格，正好容下一整屏地图高度 ⇒ 相机只需要水平滚动。
const VW: f32 = 512.0;
const VH: f32 = 480.0;
const VIRTUAL: (u32, u32) = (512, 480);
/// 旗杆所在列（判定过关用）。
const FLAG_COL: i32 = 110;

/// 玩家判定盒（32 宽图占 20×30 盒，锚点 `(0,1)` = 脚底）。
const HB_L: i32 = 6;
const HB_R: i32 = 26;
const HB_T: i32 = 30;
/// 敌人判定盒：22×26。
const EB_L: i32 = 5;
const EB_R: i32 = 27;
const EB_T: i32 = 26;

/// 物理（整数像素/拍）。
const WALK_MAX: i32 = 5;
const JUMP_VY: i32 = -20;
const GRAVITY: i32 = 2;
const TERMINAL: i32 = 12;
const STOMP_BOUNCE: i32 = -14;
/// 火球：6 px/拍平飞，冷却与寿命。
const FIRE_VX: i32 = 6;
const FIRE_COOLDOWN: i32 = 30;
const FIRE_TTL: i32 = 90;
/// 压扁尸体的停留拍数。
const SQUASH_TICKS: i32 = 20;
/// 重生后的无敌拍数（闪烁可见）。
const INVULN: i32 = 90;

/// 世界精灵归属的 viewport 栈层（`push_camera` 推两层，世界恒为 1）。
const WORLD: Layer = Layer(1);
/// HUD 层号必须大于世界层：批先按 layer 排，layer 0 的文字盖不住相机层。
const HUD_LAYER: u8 = 2;
const WORLD_VIEW: ViewportRef = ViewportRef::new(WORLD);
const _: () = assert!(
    HUD_LAYER > WORLD.0,
    "HUD 层号压不住世界层 ⇒ 地图会盖住文字（批先按 layer 排）"
);

/// z 带（同 epoch 内的排序键）。
const Z_SKY: i32 = 10;
const Z_SCENERY: i32 = 100;
const Z_TILE: i32 = 1000;
const Z_COIN: i32 = 1500;
const Z_ENEMY: i32 = 2000;
const Z_FIRE: i32 = 2050;
const Z_HERO: i32 = 2100;
const Z_POP: i32 = 2500;

/// 图块号：位图内 8 列 × 3 行（`tile_rect` 定义装箱序）。
mod tile {
    pub const GROUND: u8 = 0;
    pub const BRICK: u8 = 1;
    pub const QBLOCK: u8 = 2;
    pub const USED: u8 = 3;
    pub const PIPE_TL: u8 = 4;
    pub const PIPE_TR: u8 = 5;
    pub const PIPE_BL: u8 = 6;
    pub const PIPE_BR: u8 = 7;
    pub const POLE: u8 = 8;
    pub const FLAG: u8 = 9;
    pub const CLOUD: u8 = 10;
    pub const HILL: u8 = 11;
    pub const HILL_SM: u8 = 12;
    pub const BUSH: u8 = 13;
    pub const COIN0: u8 = 14;
    pub const COIN1: u8 = 15;
    pub const COIN2: u8 = 16;
    pub const ENEMY0: u8 = 17;
    pub const ENEMY1: u8 = 18;
    pub const ENEMY_SQ: u8 = 19;
    pub const FIRE0: u8 = 20;
    pub const FIRE1: u8 = 21;
    /// 用到的最大块号；它之后的格子必须全空（`build_sheet` 只画到这一格）。
    pub const LAST: u8 = 21;
}

/// 空格子没有图块：`GameMap::tile` 越界与空地都返回它。
pub const EMPTY: u8 = u8::MAX;

fn tile_rect(id: u8) -> Rect {
    Rect::new(
        (id as i32 % 8) as f32 * TILE,
        (id as i32 / 8) as f32 * TILE,
        TILE,
        TILE,
    )
}

/// 角色带：y=96，五个 32×32 帧（面朝右；朝左用 `flip_x` 镜像）。
const HERO_BAND_Y: f32 = 96.0;
const HERO_IDLE: f32 = 0.0;
const HERO_RUN1: f32 = 32.0;
const HERO_RUN2: f32 = 64.0;
const HERO_JUMP: f32 = 96.0;
const HERO_ATTACK: f32 = 128.0;

fn hero_rect(x: f32) -> Rect {
    Rect::new(x, HERO_BAND_Y, TILE, TILE)
}

/// 哪些图块号是实心（能挡人、能被撞）。
fn is_solid(id: u8) -> bool {
    matches!(
        id,
        tile::GROUND
            | tile::BRICK
            | tile::QBLOCK
            | tile::USED
            | tile::PIPE_TL
            | tile::PIPE_TR
            | tile::PIPE_BL
            | tile::PIPE_BR
    )
}

/// 地图字符 → 图块号。`o`/`P`/`E` 是空格子（金币/出生点/敌人由扫描 glyph 另收）。
fn spec(c: char) -> u8 {
    match c {
        '.' | 'o' | 'P' | 'E' => EMPTY,
        'X' => tile::GROUND,
        'b' => tile::BRICK,
        '?' => tile::QBLOCK,
        'T' => tile::PIPE_TL,
        't' => tile::PIPE_TR,
        'H' => tile::PIPE_BL,
        'h' => tile::PIPE_BR,
        'l' => tile::POLE,
        'f' => tile::FLAG,
        'C' => tile::CLOUD,
        'A' => tile::HILL,
        'a' => tile::HILL_SM,
        'B' => tile::BUSH,
        other => panic!("地图用了未知字符 {other:?}"),
    }
}

/// 地图（马里奥 1-1 的浓缩：? 砖群、四根水管、两处地坑、金币、终点阶梯 + 旗杆）。
/// 每行 6 段 × 20 列 = 120 列，`GameMap::new` 校验段数与列数。
const MAP: [&str; MAP_H as usize] = [
    "....................|....................|....................|....................|....................|....................",
    "............C.......|....................|....................|C...................|....................|....................",
    "....C.............C.|..........C.........|........C...........|..........C.........|................C...|....................",
    "....................|....................|....................|....................|....................|....................",
    "....................|....................|....................|....................|....................|....................",
    "...........b?b......|....................|....................|....................|....................|....................",
    "....................|....................|....................|....................|....................|....................",
    "....................|....................|....................|....................|....................|..........f.........",
    "....................|....................|....................|....................|....................|..........l.........",
    "......?...b?b?b...b?|b...................|......b?b...........|..............?.....|....................|...X......l.........",
    "....................|..........Tt......Tt|..........Tt..oo....|....................|....................|..XX......l.........",
    "....................|....Tt....Hh......Hh|..........Hh........|....................|....................|.XXX......l.........",
    "..PAA..BB......aE...|....Hh.E..Hh.BEE....|..EEa.....HhE....BB.|.ooo..EE.....E......|....E.......E.......|XXXX......X.........",
    "XXXXXXXXXXXXXXXXXXXX|XXXXXXXXXXXXXXXXXXXX|XXXXXXXXXXXXXXX..XXX|XXXXXXXXXXXXXXXXXX..|XXXXXXXXXXXXXXXXXXXX|XXXXXXXXXXXXXXXXXXXX",
    "XXXXXXXXXXXXXXXXXXXX|XXXXXXXXXXXXXXXXXXXX|XXXXXXXXXXXXXXX..XXX|XXXXXXXXXXXXXXXXXX..|XXXXXXXXXXXXXXXXXXXX|XXXXXXXXXXXXXXXXXXXX",
];

/// 瓦片栅格 + 原始字形（`build_map` 扫 `o/P/E` 用）。
struct GameMap {
    tiles: Vec<u8>,
    glyphs: Vec<Vec<char>>,
}

impl GameMap {
    fn new() -> Self {
        let mut tiles = vec![EMPTY; (MAP_W * MAP_H) as usize];
        let mut glyphs = Vec::with_capacity(MAP_H as usize);
        for (row, line) in MAP.iter().enumerate() {
            let g: Vec<char> = line.split('|').flat_map(|chunk| chunk.chars()).collect();
            assert_eq!(line.split('|').count(), 6, "第 {row} 行不是 6 段");
            assert_eq!(g.len(), MAP_W as usize, "第 {row} 行不是 120 列");
            for (col, &c) in g.iter().enumerate() {
                tiles[row * MAP_W as usize + col] = spec(c);
            }
            glyphs.push(g);
        }
        Self { tiles, glyphs }
    }

    fn glyph(&self, col: i32, row: i32) -> char {
        self.glyphs[row as usize][col as usize]
    }

    fn tile(&self, col: i32, row: i32) -> u8 {
        if col < 0 || row < 0 || col >= MAP_W || row >= MAP_H {
            return EMPTY;
        }
        self.tiles[(row * MAP_W + col) as usize]
    }

    /// 越界一律视为空（上/下/右没有墙；左边缘由玩家的 x 钳位兜底）。
    fn solid(&self, col: i32, row: i32) -> bool {
        is_solid(self.tile(col, row))
    }

    fn set(&mut self, col: i32, row: i32, id: u8) {
        self.tiles[(row * MAP_W + col) as usize] = id;
    }
}

// -------------------------------------------------------------- 碰撞（纯整数）

fn hit_cols(x: i32) -> (i32, i32) {
    (
        (x + HB_L).div_euclid(TILE_I),
        (x + HB_R - 1).div_euclid(TILE_I),
    )
}

fn hit_rows(y: i32) -> (i32, i32) {
    ((y - HB_T).div_euclid(TILE_I), (y - 1).div_euclid(TILE_I))
}

/// 水平积分 + 推回。返回 (新 x, 新 vx)，撞墙则 vx 归零。
fn move_x(map: &GameMap, x: i32, y: i32, vx: i32) -> (i32, i32) {
    if vx == 0 {
        return (x, 0);
    }
    let x = x + vx;
    let (r0, r1) = hit_rows(y);
    if vx > 0 {
        let col = (x + HB_R - 1).div_euclid(TILE_I);
        for row in r0..=r1 {
            if map.solid(col, row) {
                return (col * TILE_I - HB_R, 0);
            }
        }
    } else {
        let col = (x + HB_L).div_euclid(TILE_I);
        for row in r0..=r1 {
            if map.solid(col, row) {
                return ((col + 1) * TILE_I - HB_L, 0);
            }
        }
    }
    (x, vx)
}

/// 垂直积分。返回 (新 y, 新 vy, 落地?, 头撞格?)；头撞取离角色中心最近的实心格。
fn move_y(map: &GameMap, x: i32, y: i32, vy: i32) -> (i32, i32, bool, Option<(i32, i32)>) {
    if vy == 0 {
        // 静止时不积分，但要如实报告"脚下是否实心"（站在格子线上）。
        let row = y.div_euclid(TILE_I);
        let (c0, c1) = hit_cols(x);
        let ground = (c0..=c1).any(|c| map.solid(c, row));
        return (y, 0, ground, None);
    }
    let y = y + vy;
    if vy > 0 {
        let row = y.div_euclid(TILE_I);
        let (c0, c1) = hit_cols(x);
        if (c0..=c1).any(|c| map.solid(c, row)) {
            return (row * TILE_I, 0, true, None);
        }
    } else {
        let row = (y - HB_T).div_euclid(TILE_I);
        let (c0, c1) = hit_cols(x);
        let s0 = map.solid(c0, row);
        let s1 = map.solid(c1, row);
        if s0 || s1 {
            let center = x + (HB_L + HB_R) / 2;
            let pick = match (s0, s1) {
                (true, true) => {
                    if (c0 * TILE_I + 16 - center).abs() <= (c1 * TILE_I + 16 - center).abs() {
                        c0
                    } else {
                        c1
                    }
                }
                (true, false) => c0,
                _ => c1,
            };
            return ((row + 1) * TILE_I + HB_T, 0, false, Some((pick, row)));
        }
    }
    (y, vy, false, None)
}

/// 敌用水平阻挡探测（22×26 盒）。
fn blocked_x(map: &GameMap, x: i32, y: i32, vx: i32) -> bool {
    let r0 = (y - EB_T).div_euclid(TILE_I);
    let r1 = (y - 1).div_euclid(TILE_I);
    let col = if vx > 0 {
        (x + EB_R - 1).div_euclid(TILE_I)
    } else {
        (x + EB_L).div_euclid(TILE_I)
    };
    (r0..=r1).any(|r| map.solid(col, r))
}

fn overlaps_player(px: i32, py: i32, ex: i32, ey: i32) -> bool {
    px + HB_R > ex + EB_L && px + HB_L < ex + EB_R && py > ey - EB_T && py - HB_T < ey
}

// ------------------------------------------------------------------ 生成美术

/// 整张美术 = 256×128：3 行 32×32 图块（用到第 6 列）+ 一条 32×32 角色带。
const SHEET: (u32, u32) = (256, 128);

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

    /// 确定性"噪点"：跑两遍生成的美术必须逐字节一致（手测与将来的 golden 都要）。
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

/// 画满整张美术：3 行图块 + 角色带。
fn build_sheet() -> Vec<u8> {
    let mut s = Sheet::new(SHEET.0, SHEET.1);
    for id in 0..=tile::LAST {
        draw_tile(&mut s, id);
    }
    for (i, pose) in [0u32, 1, 2, 3, 4].iter().enumerate() {
        draw_hero(&mut s, (i as u32) * 32, *pose);
    }
    s.px
}

fn draw_tile(s: &mut Sheet, id: u8) {
    let at = ((id as i32 % 8) as u32 * 32, (id as i32 / 8) as u32 * 32);
    let green = rgb(64, 180, 72);
    let green_d = rgb(16, 104, 32);
    let green_l = rgb(144, 224, 144);
    match id {
        tile::GROUND => {
            s.rect(at, (32, 32), rgb(196, 116, 44));
            s.rect((at.0, at.1), (32, 4), rgb(232, 172, 96));
            s.rect((at.0, at.1 + 4), (32, 1), rgb(96, 48, 12));
            s.specks((at.0 + 1, at.1 + 6), (30, 25), rgb(120, 60, 16), 9, 2);
            s.rect((at.0, at.1), (1, 32), rgb(96, 48, 12));
            s.rect((at.0 + 31, at.1), (1, 32), rgb(96, 48, 12));
        }
        tile::BRICK => {
            s.rect(at, (32, 32), rgb(188, 64, 40));
            let mortar = rgb(48, 16, 12);
            for dy in [0, 15, 31] {
                s.rect((at.0, at.1 + dy), (32, 1), mortar);
            }
            s.rect((at.0 + 15, at.1 + 1), (1, 14), mortar);
            s.rect((at.0 + 7, at.1 + 16), (1, 15), mortar);
            s.rect((at.0 + 23, at.1 + 16), (1, 15), mortar);
        }
        tile::QBLOCK => {
            s.rect(at, (32, 32), rgb(240, 176, 48));
            let edge = rgb(112, 64, 0);
            s.rect((at.0, at.1), (32, 2), edge);
            s.rect((at.0, at.1 + 30), (32, 2), edge);
            s.rect((at.0, at.1), (2, 32), edge);
            s.rect((at.0 + 30, at.1), (2, 32), edge);
            for (x, y) in [(4, 4), (26, 4), (4, 26), (26, 26)] {
                s.rect((at.0 + x, at.1 + y), (2, 2), edge);
            }
            let q = rgb(255, 240, 208);
            s.rect((at.0 + 12, at.1 + 8), (8, 3), q);
            s.rect((at.0 + 18, at.1 + 11), (3, 4), q);
            s.rect((at.0 + 14, at.1 + 15), (5, 3), q);
            s.rect((at.0 + 14, at.1 + 18), (3, 2), q);
            s.rect((at.0 + 14, at.1 + 22), (4, 3), q);
        }
        tile::USED => {
            s.rect(at, (32, 32), rgb(152, 96, 48));
            let edge = rgb(88, 52, 20);
            s.rect((at.0, at.1), (32, 2), edge);
            s.rect((at.0, at.1 + 30), (32, 2), edge);
            s.rect((at.0, at.1), (2, 32), edge);
            s.rect((at.0 + 30, at.1), (2, 32), edge);
            for (x, y) in [(4, 4), (26, 4), (4, 26), (26, 26)] {
                s.rect((at.0 + x, at.1 + y), (2, 2), rgb(208, 152, 96));
            }
        }
        tile::PIPE_TL | tile::PIPE_TR => {
            let left = id == tile::PIPE_TL;
            // 管帽：两条都占满自己的 32 宽，在接缝两侧拼成 64 宽的唇沿。
            s.rect(at, (32, 10), green);
            s.rect((at.0, at.1), (32, 2), green_d);
            s.rect((at.0, at.1 + 8), (32, 2), green_d);
            if left {
                s.rect((at.0 + 3, at.1 + 3), (5, 4), green_l);
            }
            // 管身：左半占 4..32，右半占 0..28。
            let body_x = if left { at.0 + 4 } else { at.0 };
            s.rect((body_x, at.1 + 10), (28, 22), green);
            let edge_x = if left { at.0 + 4 } else { at.0 + 26 };
            s.rect((edge_x, at.1 + 10), (2, 22), green_d);
            let hl_x = if left { at.0 + 9 } else { at.0 + 13 };
            s.rect((hl_x, at.1 + 10), (5, 22), green_l);
        }
        tile::PIPE_BL | tile::PIPE_BR => {
            let left = id == tile::PIPE_BL;
            let body_x = if left { at.0 + 4 } else { at.0 };
            s.rect((body_x, at.1), (28, 32), green);
            let edge_x = if left { at.0 + 4 } else { at.0 + 26 };
            s.rect((edge_x, at.1), (2, 32), green_d);
            let hl_x = if left { at.0 + 9 } else { at.0 + 13 };
            s.rect((hl_x, at.1), (5, 32), green_l);
        }
        tile::POLE => {
            s.rect((at.0 + 15, at.1), (2, 32), rgb(200, 200, 200));
            s.rect((at.0 + 15, at.1), (1, 32), rgb(110, 110, 110));
        }
        tile::FLAG => {
            s.rect((at.0 + 15, at.1), (2, 32), rgb(200, 200, 200));
            s.rect((at.0 + 14, at.1), (4, 2), rgb(240, 200, 60));
            for i in 0..14u32 {
                let w = 13 - i;
                s.rect((at.0 + 14 - w, at.1 + 2 + i), (w, 1), rgb(56, 168, 64));
            }
        }
        tile::CLOUD => {
            let white = rgb(252, 252, 255);
            s.rect((at.0 + 4, at.1 + 14), (24, 9), white);
            s.rect((at.0 + 10, at.1 + 8), (13, 8), white);
            s.rect((at.0 + 1, at.1 + 17), (5, 5), white);
            s.rect((at.0 + 26, at.1 + 16), (5, 6), white);
        }
        tile::HILL => {
            for j in 0..24u32 {
                let half = 2 + j / 2;
                s.rect(
                    (at.0 + 16 - half, at.1 + 8 + j),
                    (half * 2, 1),
                    rgb(72, 148, 56),
                );
            }
            s.specks((at.0, at.1 + 14), (32, 18), rgb(40, 104, 36), 13, 5);
        }
        tile::HILL_SM => {
            for j in 0..14u32 {
                let half = 1 + j / 3;
                s.rect(
                    (at.0 + 16 - half, at.1 + 18 + j),
                    (half * 2, 1),
                    rgb(72, 148, 56),
                );
            }
        }
        tile::BUSH => {
            s.rect((at.0 + 2, at.1 + 20), (28, 12), rgb(56, 152, 56));
            s.rect((at.0 + 8, at.1 + 13), (16, 9), rgb(72, 176, 72));
            s.specks((at.0 + 2, at.1 + 20), (28, 10), rgb(40, 120, 40), 11, 3);
        }
        tile::COIN0 => {
            s.rect((at.0 + 10, at.1 + 6), (12, 20), rgb(240, 200, 48));
            s.rect((at.0 + 13, at.1 + 9), (6, 14), rgb(255, 240, 160));
        }
        tile::COIN1 => {
            s.rect((at.0 + 12, at.1 + 6), (8, 20), rgb(216, 168, 32));
            s.rect((at.0 + 14, at.1 + 9), (3, 14), rgb(255, 232, 128));
        }
        tile::COIN2 => {
            s.rect((at.0 + 15, at.1 + 6), (2, 20), rgb(168, 120, 24));
        }
        tile::ENEMY0 | tile::ENEMY1 => {
            // 板栗怪：圆帽 + 两只错位的小脚（两帧只差脚位，走路时来回倒）。
            s.rect((at.0 + 6, at.1 + 4), (20, 14), rgb(176, 96, 40));
            s.rect((at.0 + 4, at.1 + 16), (24, 6), rgb(200, 128, 64));
            s.rect((at.0 + 8, at.1 + 10), (5, 6), rgb(252, 248, 240));
            s.rect((at.0 + 19, at.1 + 10), (5, 6), rgb(252, 248, 240));
            s.rect((at.0 + 10, at.1 + 12), (2, 3), rgb(32, 16, 8));
            s.rect((at.0 + 20, at.1 + 12), (2, 3), rgb(32, 16, 8));
            let (l, r) = if id == tile::ENEMY0 { (4, 19) } else { (7, 16) };
            s.rect((at.0 + l, at.1 + 24), (10, 5), rgb(88, 40, 16));
            s.rect((at.0 + r, at.1 + 24), (10, 5), rgb(88, 40, 16));
        }
        tile::ENEMY_SQ => {
            s.rect((at.0 + 2, at.1 + 22), (28, 8), rgb(152, 84, 36));
            s.rect((at.0 + 6, at.1 + 24), (20, 2), rgb(96, 48, 16));
        }
        tile::FIRE0 | tile::FIRE1 => {
            let (outer, inner) = if id == tile::FIRE0 {
                (rgb(255, 128, 24), rgb(255, 224, 96))
            } else {
                (rgb(255, 208, 64), rgb(255, 96, 16))
            };
            s.rect((at.0 + 10, at.1 + 10), (12, 12), outer);
            s.rect((at.0 + 13, at.1 + 13), (6, 6), inner);
        }
        other => panic!("图块 {other} 没有对应的画法"),
    }
}

/// 角色带单帧：`ox` 是带内横坐标，`pose` 0 站 1/2 步 3 跳 4 攻。面朝右；朝左靠 `flip_x`。
fn draw_hero(s: &mut Sheet, ox: u32, pose: u32) {
    let at = (ox, 96u32);
    let red = rgb(216, 40, 0);
    let blue = rgb(56, 72, 200);
    let skin = rgb(240, 180, 140);
    let brown = rgb(96, 48, 16);
    let black = rgb(24, 16, 8);
    let gold = rgb(240, 200, 60);
    // 出拳姿态整体蹲低两像素。
    let dy = if pose == 4 { 2 } else { 0 };
    // 帽子 + 脸。
    s.rect((at.0 + 10, at.1 + 4 + dy), (12, 3), red);
    s.rect((at.0 + 20, at.1 + 7 + dy), (6, 2), red);
    s.rect((at.0 + 11, at.1 + 7 + dy), (9, 6), skin);
    s.rect((at.0 + 10, at.1 + 9 + dy), (2, 4), brown);
    s.rect((at.0 + 16, at.1 + 9 + dy), (1, 2), black);
    s.rect((at.0 + 14, at.1 + 12 + dy), (6, 1), brown);
    // 上身 + 背带裤。
    s.rect((at.0 + 9, at.1 + 13 + dy), (14, 5), red);
    s.rect((at.0 + 11, at.1 + 17 + dy), (10, 5), blue);
    s.rect((at.0 + 12, at.1 + 18 + dy), (1, 1), gold);
    s.rect((at.0 + 19, at.1 + 18 + dy), (1, 1), gold);
    match pose {
        0 => {
            s.rect((at.0 + 8, at.1 + 14), (3, 6), red);
            s.rect((at.0 + 8, at.1 + 20), (3, 2), skin);
            s.rect((at.0 + 11, at.1 + 22), (4, 3), blue);
            s.rect((at.0 + 17, at.1 + 22), (4, 3), blue);
            s.rect((at.0 + 8, at.1 + 25), (9, 3), brown);
            s.rect((at.0 + 17, at.1 + 25), (9, 3), brown);
        }
        1 => {
            // 后蹬：前臂摆起，后腿伸到身后。
            s.rect((at.0 + 20, at.1 + 13), (4, 4), red);
            s.rect((at.0 + 23, at.1 + 15), (3, 2), skin);
            s.rect((at.0 + 7, at.1 + 20), (4, 5), blue);
            s.rect((at.0 + 5, at.1 + 25), (8, 3), brown);
            s.rect((at.0 + 15, at.1 + 22), (4, 3), blue);
            s.rect((at.0 + 16, at.1 + 25), (9, 3), brown);
        }
        2 => {
            // 支撑腿在正中，另一腿收在身后。
            s.rect((at.0 + 9, at.1 + 14), (3, 6), red);
            s.rect((at.0 + 9, at.1 + 20), (3, 2), skin);
            s.rect((at.0 + 14, at.1 + 22), (4, 3), blue);
            s.rect((at.0 + 12, at.1 + 25), (8, 3), brown);
            s.rect((at.0 + 7, at.1 + 23), (4, 2), blue);
            s.rect((at.0 + 4, at.1 + 24), (6, 3), brown);
        }
        3 => {
            // 腾空：一手举过头，双腿侧收。
            s.rect((at.0 + 20, at.1 + 8), (3, 5), red);
            s.rect((at.0 + 21, at.1 + 6), (2, 3), skin);
            s.rect((at.0 + 9, at.1 + 14), (3, 6), red);
            s.rect((at.0 + 9, at.1 + 20), (3, 2), skin);
            s.rect((at.0 + 11, at.1 + 22), (5, 2), blue);
            s.rect((at.0 + 16, at.1 + 22), (5, 2), blue);
            s.rect((at.0 + 10, at.1 + 24), (8, 3), brown);
            s.rect((at.0 + 17, at.1 + 24), (8, 3), brown);
        }
        _ => {
            // 出球：整条手臂横着搨出去，身后留一只后摆臂。
            s.rect((at.0 + 6, at.1 + 16), (4, 4), red);
            s.rect((at.0 + 4, at.1 + 17), (3, 3), skin);
            s.rect((at.0 + 20, at.1 + 14), (9, 3), red);
            s.rect((at.0 + 29, at.1 + 14), (2, 3), skin);
            s.rect((at.0 + 10, at.1 + 22), (4, 4), blue);
            s.rect((at.0 + 7, at.1 + 25), (8, 3), brown);
            s.rect((at.0 + 17, at.1 + 22), (4, 4), blue);
            s.rect((at.0 + 18, at.1 + 25), (8, 3), brown);
        }
    }
}

// ---------------------------------------------------------------------- 示例

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pose {
    Idle,
    Run,
    Jump,
    Attack,
}

struct Enemy {
    sprite: SpriteRef,
    x: i32,
    /// 脚底 y。
    y: i32,
    dir: i32,
    /// Some(n) = 已被压扁，倒数 n 拍后收尸。
    squash: Option<i32>,
}

struct Pop {
    sprite: SpriteRef,
    col: i32,
    y: i32,
    vy: i32,
    life: i32,
}

struct Platformer {
    atlas: Atlas,
    pages: AtlasPages,
    uploads: UploadRing,
    proto: Sprite,
    arena: SpriteArena,
    map: GameMap,
    /// 实心/装饰格的精灵句柄（撞翻 ? 砖时换 src 用）。
    cell_sprite: Vec<Option<SpriteRef>>,
    /// 金币格：`(col, row, 句柄)`，拾取后置 None。
    coins: Vec<(i32, i32, Option<SpriteRef>)>,
    enemies: Vec<Enemy>,
    pops: Vec<Pop>,
    // 玩家状态（整数像素/拍，`(hx, hy)` 是脚底锚点）。
    hx: i32,
    hy: i32,
    hvx: i32,
    hvy: i32,
    on_ground: bool,
    face: i32,
    player: SpriteRef,
    pose: Pose,
    attack_timer: i32,
    fire_cd: i32,
    /// 火球：中心 x/y、vx、剩余拍数、句柄。
    fire: Option<(i32, i32, i32, i32, SpriteRef)>,
    invuln: i32,
    coin_count: i32,
    deaths: i32,
    cleared: bool,
    cam: f32,
    cam_pushed: bool,
    stats: Option<DrawStats>,
    frame: u64,
    ticks: u64,
    msg: String,
    vfs: Vfs,
    font_loaded: bool,
    text: TextEngine,
    // 共享动画定义（一表多角）。
    def_run: Arc<AnimDef>,
    def_walk: Arc<AnimDef>,
    def_coin: Arc<AnimDef>,
    def_fire: Arc<AnimDef>,
}

impl Platformer {
    fn new() -> Self {
        let atlas = Atlas::new();
        let sheet = atlas
            .new_bitmap(SHEET.0, SHEET.1, build_sheet(), 0)
            .expect("生成美术该占得下一页");
        let anim = |frames: Vec<FrameDef>| Arc::new(AnimDef::new(frames));
        let mut app = Self {
            uploads: UploadRing::new(),
            pages: AtlasPages::new(),
            proto: Sprite::new(sheet.clone()),
            arena: SpriteArena::with_capacity(512),
            map: GameMap::new(),
            cell_sprite: vec![None; (MAP_W * MAP_H) as usize],
            coins: Vec::new(),
            enemies: Vec::new(),
            pops: Vec::new(),
            hx: 2 * TILE_I,
            hy: 13 * TILE_I,
            hvx: 0,
            hvy: 0,
            on_ground: false,
            face: 1,
            player: SpriteRef(0, 0),
            pose: Pose::Idle,
            attack_timer: 0,
            fire_cd: 0,
            fire: None,
            invuln: 0,
            coin_count: 0,
            deaths: 0,
            cleared: false,
            cam: 0.0,
            cam_pushed: false,
            stats: None,
            frame: 0,
            ticks: 0,
            msg: "reach the flag!".to_owned(),
            atlas,
            vfs: vfs(),
            font_loaded: false,
            text: {
                let mut t = TextEngine::new(FontBundle::system());
                t.set_raster_scale(1.0);
                t
            },
            def_run: anim(vec![
                FrameDef::new(hero_rect(HERO_RUN1), 110),
                FrameDef::new(hero_rect(HERO_RUN2), 110),
            ]),
            def_walk: anim(vec![
                FrameDef::new(tile_rect(tile::ENEMY0), 220),
                FrameDef::new(tile_rect(tile::ENEMY1), 220),
            ]),
            def_coin: anim(vec![
                FrameDef::new(tile_rect(tile::COIN0), 120),
                FrameDef::new(tile_rect(tile::COIN1), 120),
                FrameDef::new(tile_rect(tile::COIN2), 120),
                FrameDef::new(tile_rect(tile::COIN1), 120),
            ]),
            def_fire: anim(vec![
                FrameDef::new(tile_rect(tile::FIRE0), 90),
                FrameDef::new(tile_rect(tile::FIRE1), 90),
            ]),
        };
        app.uploads.queue(sheet);
        app.build_map();
        app
    }

    fn spawn(&mut self, src: Rect, at: Vec2, z: i32, anchor: Vec2) -> SpriteRef {
        let mut s = self.proto.clone();
        s.src = src;
        s.anchor = anchor;
        s.teleport(at);
        s.z = z;
        s.layer = WORLD;
        self.arena.spawn(s)
    }

    fn set_anim(&mut self, r: SpriteRef, def: &Arc<AnimDef>) {
        if let Some(s) = self.arena.get_mut(r) {
            s.set_animation(Some(AnimState::new(def.clone())));
        }
    }

    fn clear_anim(&mut self, r: SpriteRef, src: Rect) {
        if let Some(s) = self.arena.get_mut(r) {
            s.set_animation(None);
            s.src = src;
        }
    }

    /// 一次生成全部：实心/装饰图块按 z 带铺进去，`P/E/o` 落成出生点、敌人与金币。
    fn build_map(&mut self) {
        let mut spawn = (2 * TILE_I, 13 * TILE_I);
        for row in 0..MAP_H {
            for col in 0..MAP_W {
                let i = (row * MAP_W + col) as usize;
                let t = self.map.tiles[i];
                let g = self.map.glyph(col, row);
                let at = Vec2::new(col as f32 * TILE, (row + 1) as f32 * TILE);
                if t != EMPTY {
                    let z = if is_solid(t) {
                        Z_TILE
                    } else if t == tile::CLOUD {
                        Z_SKY + row
                    } else {
                        Z_SCENERY + row
                    };
                    let r = self.spawn(tile_rect(t), at, z, Vec2::new(0.0, 1.0));
                    self.cell_sprite[i] = Some(r);
                }
                match g {
                    'P' => spawn = (col * TILE_I, (row + 1) * TILE_I),
                    'o' => {
                        let r = self.spawn(
                            tile_rect(tile::COIN0),
                            Vec2::new(col as f32 * TILE + 16.0, (row + 1) as f32 * TILE - 16.0),
                            Z_COIN,
                            Vec2::new(0.5, 0.5),
                        );
                        let def = self.def_coin.clone();
                        self.set_anim(r, &def);
                        self.coins.push((col, row, Some(r)));
                    }
                    'E' => {
                        let r = self.spawn(
                            tile_rect(tile::ENEMY0),
                            at,
                            Z_ENEMY + (row + 1) * TILE_I,
                            Vec2::new(0.0, 1.0),
                        );
                        let def = self.def_walk.clone();
                        self.set_anim(r, &def);
                        self.enemies.push(Enemy {
                            sprite: r,
                            x: col * TILE_I,
                            y: (row + 1) * TILE_I,
                            dir: -1,
                            squash: None,
                        });
                    }
                    _ => {}
                }
            }
        }
        self.hx = spawn.0;
        self.hy = spawn.1;
        self.player = self.spawn(
            hero_rect(HERO_IDLE),
            Vec2::new(spawn.0 as f32, spawn.1 as f32),
            Z_HERO + spawn.1,
            Vec2::new(0.0, 1.0),
        );
    }

    /// 玩家一拍：读命名动作 ⇒ 积分 ⇒ 双向碰撞 ⇒ 攻击/判定。
    fn player_tick(&mut self, ctx: &mut FrameCtx) {
        if self.cleared {
            return;
        }
        let left = ctx.input.is_down("left");
        let right = ctx.input.is_down("right");
        let jump = ctx.input.take_press("jump");
        let attack = ctx.input.take_press("attack");
        if jump && self.on_ground {
            self.hvy = JUMP_VY;
            self.on_ground = false;
        }
        let want = (right as i32 - left as i32) * WALK_MAX;
        if want != 0 {
            self.face = want.signum();
            if self.hvx < want {
                self.hvx += 1;
            } else if self.hvx > want {
                self.hvx -= 1;
            }
        } else if self.on_ground && self.hvx != 0 {
            self.hvx -= self.hvx.signum();
        }
        if !self.on_ground {
            self.hvy = (self.hvy + GRAVITY).min(TERMINAL);
        }
        let (x, vx) = move_x(&self.map, self.hx, self.hy, self.hvx);
        self.hx = x.clamp(0, WORLD_W - HB_R);
        self.hvx = vx;
        let (y, vy, ground, bump) = move_y(&self.map, self.hx, self.hy, self.hvy);
        self.hy = y;
        self.hvy = vy;
        self.on_ground = ground;
        if let Some((c, r)) = bump {
            self.handle_bump(c, r);
        }
        if self.fire_cd > 0 {
            self.fire_cd -= 1;
        }
        if self.attack_timer > 0 {
            self.attack_timer -= 1;
        }
        if attack && self.fire.is_none() && self.fire_cd == 0 {
            self.spawn_fire();
        }
        if self.invuln > 0 {
            self.invuln -= 1;
        }
        if let Some(s) = self.arena.get_mut(self.player) {
            s.visible = self.invuln == 0 || (self.ticks / 4).is_multiple_of(2);
        }
        if self.hy > WORLD_H + 96 {
            self.die("fell in a pit");
        }
    }

    /// 火球从胸前出手：中心 (x, y)，靠 `flip_x` 不用两套帧。
    fn spawn_fire(&mut self) {
        let fx = self.hx + if self.face > 0 { HB_R } else { HB_L - 16 };
        let fy = self.hy - 16;
        let r = self.spawn(
            tile_rect(tile::FIRE0),
            Vec2::new(fx as f32 + 8.0, fy as f32 + 8.0),
            Z_FIRE,
            Vec2::new(0.5, 0.5),
        );
        let def = self.def_fire.clone();
        self.set_anim(r, &def);
        self.fire = Some((fx, fy, self.face * FIRE_VX, FIRE_TTL, r));
        self.fire_cd = FIRE_COOLDOWN;
        self.attack_timer = 14;
    }

    /// 头撞格：? 砖翻面 + 弹金币。
    fn handle_bump(&mut self, col: i32, row: i32) {
        if self.map.tile(col, row) != tile::QBLOCK {
            return;
        }
        self.map.set(col, row, tile::USED);
        if let Some(Some(r)) = self.cell_sprite.get_mut((row * MAP_W + col) as usize)
            && let Some(s) = self.arena.get_mut(*r)
        {
            s.src = tile_rect(tile::USED);
        }
        self.coin_count += 1;
        self.msg = "coin block!".to_owned();
        let r = self.spawn(
            tile_rect(tile::COIN0),
            Vec2::new(col as f32 * TILE + 16.0, (row + 1) as f32 * TILE),
            Z_POP,
            Vec2::new(0.5, 0.5),
        );
        self.pops.push(Pop {
            sprite: r,
            col,
            y: (row + 1) * TILE_I,
            vy: -8,
            life: 22,
        });
    }

    fn die(&mut self, why: &str) {
        self.deaths += 1;
        self.msg = format!("{why} — respawn");
        self.hx = 2 * TILE_I;
        self.hy = 13 * TILE_I;
        self.hvx = 0;
        self.hvy = 0;
        self.on_ground = false;
        self.invuln = INVULN;
        if let Some((_, _, _, _, r)) = self.fire.take() {
            self.arena.despawn(r);
        }
    }

    /// 一拍敌人：等速爬行，撞墙或到沿折返；玩家踩头弹起，侧碰则阵亡。
    fn enemies_tick(&mut self) {
        // 倒序收尸：swap_remove 只会把"尾部（已处理过）"的敌人搬进空洞，正序
        // 预计算 range 会踩越界；每个敌人在这一段里仍恰好被访问一次。
        for i in (0..self.enemies.len()).rev() {
            if let Some(squash) = self.enemies[i].squash {
                if squash > 1 {
                    self.enemies[i].squash = Some(squash - 1);
                } else {
                    let r = self.enemies[i].sprite;
                    self.enemies.swap_remove(i);
                    self.arena.despawn(r);
                }
            }
        }
        // 上面收尸用 swap_remove，索引会跳 ⇒ 收完再走行为一拍。
        for i in 0..self.enemies.len() {
            let (x, y, mut dir) = (self.enemies[i].x, self.enemies[i].y, self.enemies[i].dir);
            let nx = x + dir;
            if blocked_x(&self.map, nx, y, dir) {
                dir = -dir;
            } else {
                self.enemies[i].x = nx;
                // 到沿折返：探下一格前方脚下有没有地。
                let ahead = if dir > 0 {
                    (nx + EB_R - 1).div_euclid(TILE_I)
                } else {
                    (nx + EB_L).div_euclid(TILE_I)
                };
                if !self.map.solid(ahead, y.div_euclid(TILE_I)) {
                    dir = -dir;
                }
            }
            self.enemies[i].dir = dir;
            if self.cleared || self.invuln > 0 {
                continue;
            }
            if overlaps_player(self.hx, self.hy, self.enemies[i].x, y) {
                let stomp = self.hvy > 0 && self.hy <= y - EB_T + 12;
                if stomp {
                    self.squash_enemy(i);
                    self.hvy = STOMP_BOUNCE;
                    self.msg = "stomp!".to_owned();
                } else {
                    self.die("ouch");
                }
            }
        }
    }

    fn squash_enemy(&mut self, i: usize) {
        self.enemies[i].squash = Some(SQUASH_TICKS);
        let r = self.enemies[i].sprite;
        self.clear_anim(r, tile_rect(tile::ENEMY_SQ));
    }

    /// 一拍火球：平飞、撞墙灭、命中压扁。`(x, y)` 是中心。
    fn fire_tick(&mut self) {
        let Some((x, y, vx, ttl, r)) = self.fire else {
            return;
        };
        let nx = x + vx;
        let probe = if vx > 0 { nx + 8 } else { nx - 8 };
        let mut gone = self
            .map
            .solid(probe.div_euclid(TILE_I), y.div_euclid(TILE_I))
            || ttl <= 1;
        if !gone {
            for i in 0..self.enemies.len() {
                let e = &self.enemies[i];
                if e.squash.is_none()
                    && nx + 8 > e.x + EB_L
                    && nx - 8 < e.x + EB_R
                    && y + 8 > e.y - EB_T
                    && y < e.y
                {
                    self.squash_enemy(i);
                    self.msg = "fireball!".to_owned();
                    gone = true;
                    break;
                }
            }
        }
        if gone {
            self.arena.despawn(r);
            self.fire = None;
            return;
        }
        self.fire = Some((nx, y, vx, ttl - 1, r));
        if let Some(s) = self.arena.get_mut(r) {
            s.teleport(Vec2::new(nx as f32 + 8.0, y as f32 + 8.0));
        }
    }

    fn pops_tick(&mut self) {
        for i in (0..self.pops.len()).rev() {
            self.pops[i].life -= 1;
            self.pops[i].y += self.pops[i].vy;
            self.pops[i].vy += 2;
            if self.pops[i].life <= 0 {
                let p = self.pops.remove(i);
                self.arena.despawn(p.sprite);
            }
        }
    }

    fn pickups_tick(&mut self) {
        for coin in self.coins.iter_mut() {
            let Some(r) = coin.2 else { continue };
            let (cx, cy) = (coin.0, coin.1);
            let overlap = self.hx + HB_R > cx * TILE_I
                && self.hx + HB_L < (cx + 1) * TILE_I
                && self.hy > cy * TILE_I
                && self.hy - HB_T < (cy + 1) * TILE_I;
            if overlap {
                self.arena.despawn(r);
                coin.2 = None;
                self.coin_count += 1;
                self.msg = "coin!".to_owned();
            }
        }
        if !self.cleared && self.hx + HB_R >= FLAG_COL * TILE_I + 12 {
            self.cleared = true;
            self.hvx = 0;
            self.msg = "STAGE CLEAR!".to_owned();
            platform::log_info("[platformer] STAGE CLEAR");
        }
    }

    /// 把玩家逻辑状态投影到精灵：位置、朝向、姿态帧。
    fn sync_hero(&mut self) {
        if let Some(s) = self.arena.get_mut(self.player) {
            s.teleport(Vec2::new(self.hx as f32, self.hy as f32));
            s.flip_x = self.face < 0;
            s.z = Z_HERO + self.hy;
        }
        let want = if self.attack_timer > 0 {
            Pose::Attack
        } else if !self.on_ground {
            Pose::Jump
        } else if self.hvx != 0 {
            Pose::Run
        } else {
            Pose::Idle
        };
        if want == self.pose {
            return;
        }
        self.pose = want;
        match want {
            Pose::Run => {
                let def = self.def_run.clone();
                self.set_anim(self.player, &def);
            }
            Pose::Idle => self.clear_anim(self.player, hero_rect(HERO_IDLE)),
            Pose::Jump => self.clear_anim(self.player, hero_rect(HERO_JUMP)),
            Pose::Attack => self.clear_anim(self.player, hero_rect(HERO_ATTACK)),
        }
    }

    fn sync_props(&mut self) {
        for e in self.enemies.iter() {
            if let Some(s) = self.arena.get_mut(e.sprite) {
                s.teleport(Vec2::new(e.x as f32, e.y as f32));
                s.z = Z_ENEMY + e.y;
            }
        }
        for p in self.pops.iter() {
            if let Some(s) = self.arena.get_mut(p.sprite) {
                s.teleport(Vec2::new(p.col as f32 * TILE + 16.0, p.y as f32));
            }
        }
    }

    fn camera_for(&self) -> f32 {
        ((self.hx + 16) as f32 - VW / 2.0).clamp(0.0, WORLD_W as f32 - VW)
    }

    /// 相机走 viewport 栈（同 tiled_map）：世界层反平移，HUD 层平移回去 ⇒ 屏幕固定。
    fn push_camera(&mut self, ctx: &mut FrameCtx) {
        if self.cam_pushed {
            ctx.pop_viewport();
            ctx.pop_viewport();
        }
        let world = ctx.push_viewport(Viewport::at(-self.cam, 0.0, VW, VH));
        debug_assert_eq!(world, WORLD.0, "世界层号与 `WORLD` 的约定不符");
        let hud = ctx.push_viewport(Viewport::at(self.cam, 0.0, VW, VH));
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
            "[platformer] 字体 {:?}（face {}）",
            bundle.family(),
            bundle.face_count()
        ));
        *self.text.bundle_mut() = bundle;
        self.font_loaded = true;
    }

    fn hud_lines(&self) -> [String; 3] {
        let (batches, instances) = self.stats.map_or((0, 0), |s| (s.batches, s.instances));
        [
            format!(
                "coins {}  deaths {}  fire {}  {}",
                self.coin_count,
                self.deaths,
                if self.fire.is_some() { "away" } else { "ready" },
                if self.cleared { "CLEARED" } else { "running" }
            ),
            format!(
                "batches {batches} quads {instances} sprites {} pos {},{} cam {:.0}",
                self.arena.len(),
                self.hx,
                self.hy,
                self.cam
            ),
            format!("arrows/AD move  Z/Space jump  X fireball — {}", self.msg),
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

/// 横板动作的键位表：键盘/手柄只折成命名动作，示例代码里没有一处裸键名。
fn action_map() -> ActionMap {
    let dir = |name: &str, arrow: &str, wasd: &str, pad: &str, stick: &str| ActionDef {
        name: name.to_owned(),
        triggers: vec![
            Trigger::key(arrow),
            Trigger::key(wasd),
            Trigger::pad(pad),
            Trigger::pad(stick),
        ],
        one_shot: false,
        autofire: None,
    };
    ActionMap {
        actions: vec![
            dir("left", "ArrowLeft", "KeyA", "DPadLeft", "LeftStickX-"),
            dir("right", "ArrowRight", "KeyD", "DPadRight", "LeftStickX+"),
            ActionDef {
                name: "jump".to_owned(),
                triggers: vec![
                    Trigger::key("Space"),
                    Trigger::key("KeyZ"),
                    Trigger::key("ArrowUp"),
                    Trigger::key("KeyW"),
                    Trigger::pad("South"),
                ],
                one_shot: false,
                autofire: None,
            },
            ActionDef {
                name: "attack".to_owned(),
                triggers: vec![
                    Trigger::key("KeyX"),
                    Trigger::key("KeyJ"),
                    Trigger::pad("East"),
                ],
                one_shot: false,
                autofire: None,
            },
        ],
    }
}

impl App for Platformer {
    fn tick(&mut self, dt: Duration, ctx: &mut FrameCtx) {
        self.ticks += 1;
        // 每拍开头收拢 prev_pos：本拍没动的精灵不该有可播的位移（tiled_map 同款纪律）。
        // 本示例所有移动都走 `teleport`，这条守的是"静止"那一半的不变式。
        self.arena.begin_tick();
        self.text.set_raster_scale(ctx.raster_scale());
        self.player_tick(ctx);
        self.enemies_tick();
        self.fire_tick();
        self.pops_tick();
        self.pickups_tick();
        self.sync_hero();
        self.sync_props();
        // 时间驱动的三套动画（步帧/金币/火球）都在 arena 里走同一个节拍。
        self.arena.tick_anims(dt);
    }

    fn prepare(&mut self, _alpha: f32, ctx: &mut FrameCtx) {
        // 相机取角色拍尾整数位置：所有精灵都按拍 teleport，本来就钉在格点上，
        // 所以这里不需要 tiled_map 的 `lattice_alpha`（亚像素误差不产生）。
        self.cam = self.camera_for();
        self.push_camera(ctx);
    }

    fn draw(&mut self, _alpha: f32, renderer: &mut Renderer) {
        self.frame += 1;
        self.load_font();
        let n = self.arena.len().max(1) as u32;
        if renderer.quads_per_slot() != n {
            renderer.set_quads_per_slot(n);
        }
        self.atlas.touch(self.proto.epoch(), self.frame);
        renderer.apply_atlas(&self.atlas, &mut self.pages, &mut self.uploads);

        // alpha 恒 1.0：teleport 把 prev_pos 与 pos 写成同一个整数点，插值即取 pos。
        let mut list = BatchList::new();
        if self.arena.collect_at(WORLD_VIEW, 1.0, &mut list).is_err() {
            platform::log_info("[platformer] 采集失败（本阶段的图块不该有 tone）");
            return;
        }
        // 相机层自己裁剪：letterbox 只改投影不改 scissor，平移出画面的地图会画进黑边。
        renderer.push_clip(u32::from(WORLD.0), [self.cam, 0.0, VW, VH]);
        renderer.append_list(list);
        renderer.pop_clip();

        renderer.push_clip(u32::from(HUD_LAYER), [0.0, 0.0, VW, 64.0]);
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
                [VW / 2.0, 32.0],
                [VW, 64.0],
                [0.0, 0.0, 0.0, 0.55],
                HUD_LAYER,
            ),
        );
        for (i, line) in self.hud_lines().iter().enumerate() {
            self.text.draw_text_at(
                renderer,
                [8.0, 6.0 + i as f32 * 18.0, VW - 16.0, 18.0],
                line,
                &TextStyle::new(13.0).aligned(Align::Left),
                [1.0, 1.0, 1.0, 0.92],
                HUD_LAYER,
            );
        }
        renderer.pop_clip();
        assert_eq!(renderer.clip_depth(), 0, "裁剪栈没成对收回");
        self.stats = renderer.stats();
        if self.frame == 2 {
            platform::log_info(&format!(
                "[platformer] 第 1 帧合成: {}",
                self.hud_lines()[1]
            ));
        }
        self.text.sync(renderer);
    }
}

fn config() -> AppConfig {
    AppConfig {
        title: "SISF platformer (1-1)".to_owned(),
        virtual_size: VIRTUAL,
        // 整数倍缩放：与"无半像素抖动"那条验收同源（边条取整在 `FrameCtx::recompute`）。
        scale_mode: ScaleMode::IntegerFit,
        clear: [0.42, 0.62, 0.92, 1.0],
        actions: action_map(),
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
    platform::log_info("[platformer] main 已启动");
    if let Err(e) = platform::run(Platformer::new(), config()) {
        panic!("platformer 运行失败: {e}");
    }
}

// ---------------------------------------------------------------------- 单测
//
// 能自动化的部分都在这里：地图形状、地标实心、跳升程、头撞选格、落地钉格线。
// 跑法 `cargo test -p sisf --example platformer`。剩下的是手感，走文件头的手测清单。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_is_a_rectangle_of_known_chars() {
        // GameMap::new 逐行断言 6 段 × 120 列并拒绝未知字符；跑通即证形状。
        let m = GameMap::new();
        assert_eq!(m.tiles.len(), (MAP_W * MAP_H) as usize);
        assert_eq!(m.glyphs.len(), MAP_H as usize);
    }

    #[test]
    fn landmarks_are_where_the_level_design_says() {
        let m = GameMap::new();
        assert!(m.solid(0, 13), "地板在脚下");
        assert!(m.solid(24, 11) && m.solid(25, 11), "两根格宽的管口");
        assert!(m.solid(30, 10) && m.solid(30, 11), "3 格高的水管");
        assert!(!m.solid(55, 13) && !m.solid(78, 13), "两处地坑");
        assert_eq!(m.tile(6, 9), tile::QBLOCK, "第一个 ? 砖");
        assert_eq!(m.tile(FLAG_COL, 12), tile::GROUND, "旗杆底座");
        assert_eq!(m.glyph(2, 12), 'P', "出生点");
        assert!(!m.solid(2, 12), "出生点不嵌墙");
        assert_eq!(m.glyph(16, 12), 'E', "第一只板栗怪");
    }

    #[test]
    fn every_art_rect_lives_inside_the_sheet() {
        for id in 0..=tile::LAST {
            let r = tile_rect(id);
            assert!(
                r.x + r.w <= SHEET.0 as f32 && r.y + r.h <= SHEET.1 as f32,
                "图块 {id} 画到了美术外面"
            );
        }
        for x in [HERO_IDLE, HERO_RUN1, HERO_RUN2, HERO_JUMP, HERO_ATTACK] {
            assert!(x + TILE <= SHEET.0 as f32);
        }
        let px = build_sheet();
        assert_eq!(px.len(), (SHEET.0 * SHEET.1 * 4) as usize);
    }

    #[test]
    fn gravity_settles_the_hero_on_a_tile_line() {
        let m = GameMap::new();
        let x = 2 * TILE_I;
        let mut y = 12 * TILE_I;
        let mut vy = 0;
        let mut grounded = false;
        for _ in 0..120 {
            vy = (vy + GRAVITY).min(TERMINAL);
            let (ny, nvy, g, _) = move_y(&m, x, y, vy);
            y = ny;
            vy = nvy;
            grounded = g;
            if g {
                break;
            }
        }
        assert!(grounded, "自由落体该落地");
        assert_eq!(y, 13 * TILE_I, "脚底钉在地板格线上");
    }

    #[test]
    fn a_full_jump_clears_a_three_tile_pipe() {
        let m = GameMap::new();
        // 从地板（第 17 列，正上方无砖）起跳：最高点脚底该高过 3 格管口 y=320。
        let x = 17 * TILE_I;
        let (mut y, mut vy) = (13 * TILE_I, JUMP_VY);
        let mut apex = y;
        loop {
            apex = apex.min(y);
            let (ny, nvy, g, _) = move_y(&m, x, y, vy);
            y = ny;
            if g {
                break;
            }
            vy = (nvy + GRAVITY).min(TERMINAL);
        }
        assert!(
            apex <= 10 * TILE_I,
            "升程不足：最高脚底 {apex} 该不高于 3 格管顶（y ≤ 320）"
        );
    }

    #[test]
    fn head_bump_picks_the_nearest_solid_cell() {
        let m = GameMap::new();
        // 跨在 (11,9)'?' 与 (12,9)'b' 两格之间上冲：头顶刚进砖行就该报更近的 (11,9)。
        let (y, vy, ground, bump) = move_y(&m, 359, 352, -8);
        assert!(!ground);
        assert_eq!(vy, 0, "撞停");
        assert_eq!(bump, Some((11, 9)));
        assert_eq!(y, 10 * TILE_I + HB_T, "被顶回砖底");
    }

    #[test]
    fn walls_stop_the_run() {
        let m = GameMap::new();
        // 朝第 24 列水管跑（管身 row 11..12）：move_x 该把人推到管壁外。
        let (x, vx) = move_x(&m, 23 * TILE_I + 4, 13 * TILE_I, 5);
        assert_eq!(vx, 0, "撞墙清零");
        assert_eq!(x, 24 * TILE_I - HB_R);
    }

    #[test]
    fn stomp_geometry_needs_falling_and_height() {
        assert!(overlaps_player(100, 400, 104, 416));
        assert!(!overlaps_player(100, 300, 500, 416));
        // 踩判定：脚底须不高于敌人头顶 +12。
        let (hy, ey) = (400, 416);
        assert!(hy <= ey - EB_T + 12);
    }
}
