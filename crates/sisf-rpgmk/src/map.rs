//! MV/MZ 磁盘数据（`data/Map*.json` / `data/Tilesets.json` / `data/System.json`）的读取层。
//!
//! ## 宽松到什么程度
//!
//! 每个结构体都是 `#[serde(default)]` + 不拒未知字段：一个真实工程的 `Map001.json` 里
//! 有几十个本引擎用不到的键（`encounterList`、`parallaxSx`、`battleback1Name`…），而
//! 移植层要能在"工程只填了一半"的时候照样进图。字段名照 MV 的**驼峰**原样写
//! （`rename_all = "camelCase"`），因为磁盘上的键就是这些 —— 这一层是适配器，
//! 不是重新设计的数据模型。
//!
//! ## 字段表是从官方核心脚本反推的，不是抄来的
//!
//! `rpgtkoolmv/corescript` 的 `js/rpg_objects/*.js` 里 `$dataMap.*` / `$dataSystem.*` /
//! `tileset.*` 的全部读点就是本模块的字段清单（`Map.js` 的 `scrollType`、
//! `Game_Map.isLoopHorizontal` 的 `scrollType === 2 || 3`、`Game_Switches.setValue` 里
//! 那句 `switchId > 0 && switchId < $dataSystem.switches.length`）。**清单字面量说的
//! `System.json → ActionMap` 也来自这里**：MV 的键位表根本不在 `System.json`，
//! 而在 `js/rpg_core/Input.js` 的 `Input.keyMapper`（见 [`KEY_MAPPER`]），而
//! `System.json` 在核心脚本里的消费面只有 `switches`/`variables` 两张名字表。
//!
//! ## 图块层数
//!
//! `Game_Map.tileId(x,y,z)` 用的是 `(z * height + y) * width + x`，即 `data` 是
//! `层数 × w × h` 的一维数组。碰撞只看 `layeredTiles` 取的 z=3..0 那四层（外加站在格子上
//! 的事件图块），z=5 是区域号（`regionId`）。清单说"层序简化为三层"，本层照做：把
//! [`MapDef::TILE_LAYERS`] 钉成 3，读图块时取最上面三层 —— 第四层（A4，门那类）在
//! 本引擎的三层图块通路里没有落点（T8 的 `tiled_map` 就是三层）。

use serde::{Deserialize, Serialize};

/// 一次翻译/加载的失败。磁盘上的工程是用户手写的，错误要能指到是哪个文件哪一格。
#[derive(Debug, thiserror::Error)]
pub enum PortError {
    #[error("JSON 读取失败: {0}")]
    Json(String),
    #[error("地图 {map} 的尺寸 {width}x{height} 与 data 长度 {len} 不符（应为层数×w×h 的倍数）")]
    BadDataLen {
        map: u32,
        width: u32,
        height: u32,
        len: usize,
    },
    #[error("VFS 里找不到 {0}")]
    Missing(String),
}

/// 从 VFS 读一份 MV 工程文件（`data/*.json`）。
///
/// 字节按 UTF-8 宽松读（`from_utf8_lossy`）：这些文件是编辑器写出来的，坏字节来自别人的
/// 导出错误，不该让"进不去这张图"变成它的下游症状。
fn read_data(vfs: &sisf_asset::Vfs, name: &str) -> Result<String, PortError> {
    let p = sisf_asset::VfsPath::new(name).map_err(|e| PortError::Missing(e.to_string()))?;
    let bytes = vfs
        .read(&p)
        .ok_or_else(|| PortError::Missing(name.to_owned()))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// 事件与移动路线共用的一条原始指令：`{code, indent, parameters}`。
///
/// `parameters` 保持 `serde_json::Value` 而不是一上来就转成 [`Param`](sisf_event::Param)：
/// MV 的参数里有字符串数组（选项列表）、有嵌套对象（音频引用、移动路线），先按位置取用
/// 才不至于为一个"多半取不到"的形状写一堆转换。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RawCommand {
    #[serde(default)]
    pub code: u32,
    /// MV 用它表达嵌套（`skipBranch` 就是"跳过所有更深 indent 的行"）。
    #[serde(default)]
    pub indent: u32,
    #[serde(default)]
    pub parameters: Vec<serde_json::Value>,
}

impl RawCommand {
    /// 第 `k` 个整数参数；缺失、越界、非数字一律 0（宽松读取的核心卖点）。
    pub fn num(&self, k: usize) -> i64 {
        self.parameters
            .get(k)
            .and_then(|v| match v {
                serde_json::Value::Number(n) => n.as_i64(),
                // MV 偶尔把数字写成字符串（插件改的数据），认下来比报错有用。
                serde_json::Value::String(s) => s.trim().parse().ok(),
                serde_json::Value::Bool(b) => Some(i64::from(*b)),
                _ => None,
            })
            .unwrap_or(0)
    }

    pub fn text(&self, k: usize) -> &str {
        match self.parameters.get(k) {
            Some(serde_json::Value::String(s)) => s,
            _ => "",
        }
    }

    pub fn flag(&self, k: usize) -> bool {
        self.num(k) != 0
    }

    /// 第 `k` 个参数若是个数组（选项列表），取它的字符串元素。
    pub fn texts(&self, k: usize) -> Vec<String> {
        self.parameters
            .get(k)
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .map(|v| v.as_str().unwrap_or_default().to_owned())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// 图块标志位（`Tilesets.json` 的 `flags[i]`），位序取自 `Game_Map` 的三个读点：
/// `isPassable` 的 `(1 << (d / 2 - 1)) & 0x0f`、`checkPassage` 的 `0x10`、
/// `isLadder/isBush/isCounter/isDamageFloor` 的 `0x20/0x40/0x80/0x100`。
pub mod passage {
    /// 该图块对通行"无影响"（`[*]`）：`checkPassage` 会**跳过它继续往下看**，而不是放行。
    pub const NO_EFFECT: u32 = 0x10;
    /// 梯子（`isLadder`）。
    pub const LADDER: u32 = 0x20;
    /// 草丛（下半身透明，`isBush`）。
    pub const BUSH: u32 = 0x40;
    /// 柜台（`isCounter`）。
    pub const COUNTER: u32 = 0x80;
    /// 伤害地板（`isDamageFloor`）。
    pub const DAMAGE: u32 = 0x100;
    /// 船只可通行（`isShipPassable` 的 `0x0400`）。
    pub const SHIP: u32 = 0x400;
    /// 飞艇可降落（`isAirshipLandOk` 的 `0x0800`）。
    pub const AIRSHIP: u32 = 0x800;
    /// 地形标签从第 12 位起（`flags >> 12`）。
    pub const TERRAIN_SHIFT: u32 = 12;

    /// 朝向键值 → 该方向的阻挡位。**位是"阻挡"意思**：`checkPassage` 里
    /// `(flag & bit) === 0` 才是通。朝向沿用 RPG Maker 的 2/4/6/8（与
    /// `sisf_sprite::DIR_*` 同一套，所以这里能照抄 MV 的算式）。
    pub const fn bit(dir: u8) -> u32 {
        match dir {
            2 | 4 | 6 | 8 => 1 << (dir / 2 - 1),
            _ => 0,
        }
    }

    /// 四个阻挡位的合集（`checkPassage(x, y, 0x0f)` 的"任意方向都不通"）。
    pub const ALL: u32 = 0x0f;
}

/// `data/Tilesets.json` 的一条。只需要 `flags` 与 `tilesetNames` 两样。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TilesetDef {
    pub id: u32,
    /// 0=OV, 1=AR, 2=side-view（本引擎不区分，存下来只为诊断）。
    pub mode: u8,
    /// 索引 = 图块号。**稀疏**：MV 写盘时把没用到的位补 0。
    pub flags: Vec<u32>,
    /// 九张图块图的名字（`A1..A5, B, C, D, E`）。
    pub tileset_names: Vec<String>,
}

impl TilesetDef {
    pub fn from_json(src: &str) -> Result<Self, PortError> {
        serde_json::from_str(src).map_err(|e| PortError::Json(e.to_string()))
    }

    /// 从 VFS 读 `data/Tilesets.json` 并取 `id` 那一条。
    ///
    /// 这张表在 MV 里是**顶层数组**（`dataTilesets[id]`，下标 0 是 `null` 洞），所以不能把
    /// 整个文件交给 [`Self::from_json`] —— 那是"一条图块组"的形状。取哪一条由地图的
    /// `tilesetId` 决定，所以这里按 `id` 字段找而不是按数组下标猜。
    pub fn load(vfs: &sisf_asset::Vfs, id: u32) -> Result<Self, PortError> {
        let src = read_data(vfs, "data/Tilesets.json")?;
        let all: Vec<Option<Self>> =
            serde_json::from_str(&src).map_err(|e| PortError::Json(e.to_string()))?;
        all.into_iter()
            .flatten()
            .find(|t| t.id == id)
            .ok_or(PortError::Missing(format!(
                "data/Tilesets.json 里没有 id={id}"
            )))
    }

    /// 图块号 → 标志。越界（含图块号 0 以外的空洞）返回 0 = 全通行、无特效。
    pub fn flag(&self, tile: u32) -> u32 {
        self.flags.get(tile as usize).copied().unwrap_or(0)
    }

    /// `A1..A5,B,C,D,E` 里第 `k` 张图块图的名字（空串 = 没有）。
    pub fn tileset_name(&self, k: usize) -> &str {
        self.tileset_names.get(k).map(String::as_str).unwrap_or("")
    }
}

/// 事件页的显示条件（`conditions`）。字段名与 MV 一字不差，`*Valid` 才生效。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PageCondition {
    pub switch1_id: u32,
    pub switch1_valid: bool,
    pub switch2_id: u32,
    pub switch2_valid: bool,
    pub variable_id: u32,
    pub variable_valid: bool,
    pub variable_value: i64,
    /// `'A'..'J'`（MV 的 self switch 是字母，不是编号）。
    pub self_switch_ch: String,
    pub self_switch_valid: bool,
    pub item_id: u32,
    pub item_valid: bool,
    pub actor_id: u32,
    pub actor_valid: bool,
}

impl PageCondition {
    /// 这一页的条件里有没有"本引擎没有通路"的那两类（持有物品 / 队伍里有某角色）。
    ///
    /// MV 的 `meetsConditions` 对这两条是查 `$gameParty`，而移植层没有物品与队伍概念，
    /// 于是 [`PageCondition::meets`] 只能按**不满足**处理并记下计数（宁可少显示一页，
    /// 也不能让"本该在拿到钥匙后才出现的门"提前出现）。
    pub fn has_unsupported(&self) -> bool {
        (self.item_valid && self.item_id > 0) || (self.actor_valid && self.actor_id > 0)
    }

    /// 只查开关/变量/自我开关三类（顺序与 `Game_Event.meetsConditions` 一致）。
    pub fn meets(&self, vars: &crate::game_vars::GameVars, map_id: u32, event_id: u32) -> bool {
        if self.switch1_valid && !vars.switch_value(self.switch1_id) {
            return false;
        }
        if self.switch2_valid && !vars.switch_value(self.switch2_id) {
            return false;
        }
        if self.variable_valid && vars.value(self.variable_id) < self.variable_value {
            return false;
        }
        if self.self_switch_valid && !vars.self_switch(map_id, event_id, &self.self_switch_ch) {
            return false;
        }
        if self.has_unsupported() {
            return false;
        }
        true
    }
}

/// 事件页的图像（`image`）：角色图或图块二选一。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PageImage {
    pub character_name: String,
    pub character_index: u8,
    /// 0=与上次同向，其余是 RPG Maker 的 2/4/6/8。
    pub direction: u8,
    pub pattern: u8,
    /// 非 0 ⇒ 用图块而不是角色图。
    pub tile_id: u32,
}

/// 一条磁盘上的移动路线（`moveRoute`）。`list` 里的 `code` 是 `ROUTE_*` 常量，
/// 见 [`crate::translate`] 的转换表。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RawRoute {
    #[serde(default)]
    pub list: Vec<RawCommand>,
    pub repeat: bool,
    pub skippable: bool,
    /// `true` ⇒ 事件指令 `205` 要等这条路线跑完（`setWaitMode('route')`）。
    pub wait: bool,
    pub walk: bool,
}

/// 一个事件页。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EventPage {
    pub conditions: PageCondition,
    pub image: PageImage,
    /// 0 固定 / 1 随机 / 2 面向玩家 / 3 自定义路线（`moveTypeRandom` 等）。
    pub move_type: u8,
    pub move_speed: u8,
    pub move_frequency: u8,
    /// 0 底下 / 1 与玩家同级 / 2 玩家上方。
    pub priority_type: u8,
    pub walk_anime: bool,
    pub step_anime: bool,
    pub direction_fix: bool,
    pub through: bool,
    /// 0 决定键 / 1 玩家接触 / 2 事件接触 / 3 自动执行 / 4 并行处理。
    pub trigger: u8,
    pub move_route: RawRoute,
    #[serde(default)]
    pub list: Vec<RawCommand>,
}

impl EventPage {
    /// `Game_Event.prototype.isTile`：这一页用图块而不是角色图时的图块号（`None` = 角色图）。
    /// 通行判定要它 —— MV 把站在格子上的图块事件排在图块层前面。
    pub fn tile_id(&self) -> Option<u32> {
        (self.image.tile_id > 0).then_some(self.image.tile_id)
    }
}

/// MV 的音频引用对象：`{name, volume, pitch, pan}`（`AudioManager.makeEmptyAudio` 给的就是
/// 这四个字段，`volume`/`pitch`/`pan` 缺省都是 0）。
///
/// **折成 [`Sound`] 时只带走 `name` 与 `volume`**：引擎的混音器没有"按这条声音变调/pan"的
/// 通路（`Sound` 只有 `gain`/`fade_in`），所以 `pitch`/`pan` 原样存下来只为诊断。
/// 折算式与上游一致 —— `AudioManager.updateBufferParameters` 里
/// `buffer.volume = configVolume * (audio.volume || 0) / 10000`，其中 `configVolume`
/// 是宿主侧的 `AudioManager.bgmVolume`（默认 100），归混音器总音量，不在这里乘。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MvAudio {
    pub name: String,
    /// 0..=100（MV 的滑杆单位）。0 = 静音，与上游 `|| 0` 同义。
    pub volume: i64,
    /// 50..=200，`100` = 原速。**本层不应用**（见上面那句 `Sound` 没有 pitch）。
    pub pitch: i64,
    /// -100..=100。**本层不应用**。
    pub pan: i64,
}

impl MvAudio {
    /// 从一条指令的 `parameters[k]` 读（`241/245/249/250` 与 `$dataMap.bgm` 都是这个对象）。
    pub fn from_value(v: &serde_json::Value) -> Self {
        let num = |k: &str| v.get(k).and_then(|n| n.as_f64()).unwrap_or(0.0) as i64;
        Self {
            name: v
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_owned(),
            volume: num("volume"),
            pitch: num("pitch"),
            pan: num("pan"),
        }
    }

    /// 折成引擎的 `Sound`。空名字返回 `None` —— MV 那边是 `stopBgm()`，而指令表里没有"停"
    /// 这条（只有淡出），所以调用方必须**显式处理**这个 `None`，不能当"音量 0 的同一首"。
    ///
    /// 路径按 MV 的 `createBuffer(folder, name)`：`audio/{文件夹}/{名字}.ogg`。
    pub fn sound(&self, folder: &str) -> Option<sisf_event::Sound> {
        if self.name.is_empty() {
            return None;
        }
        Some(
            sisf_event::Sound::new(format!("audio/{folder}/{}.ogg", self.name))
                .gain(self.volume as f32 / 100.0),
        )
    }
}

/// 地图上的一个事件（`events[i]`，`i` 就是事件号）。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MapEvent {
    pub id: u32,
    pub name: String,
    pub x: u32,
    pub y: u32,
    /// MV 写 `"\\note<text>"` 形式，本层原样留着给宿主。
    pub note: String,
    #[serde(default)]
    pub pages: Vec<EventPage>,
}

/// `data/MapNNN.json`。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MapDef {
    pub width: u32,
    pub height: u32,
    pub tileset_id: u32,
    /// 0 闭合 / 1 纵向循环 / 2 横向循环 / 3 四向循环（`Game_Map.isLoopHorizontal`）。
    pub scroll_type: u8,
    pub display_name: String,
    pub disable_dashing: bool,
    pub parallax_name: String,
    /// 事件号 → 事件。**下标 0 恒为 `null`**（MV 的事件号从 1 起），空洞也必须保留，
    /// 否则"事件号即下标"这条前提就断了。
    #[serde(default)]
    pub events: Vec<Option<MapEvent>>,
    #[serde(default)]
    pub data: Vec<u32>,
    /// 进图时是否自动播 `bgm`（`autoplayBgm`）。
    pub autoplay_bgm: bool,
    pub autoplay_bgs: bool,
    /// `$dataMap.bgm` / `$dataMap.bgs`：进图自动播的那两条音频引用。
    #[serde(default)]
    pub bgm: MvAudio,
    #[serde(default)]
    pub bgs: MvAudio,
}

impl MapDef {
    /// 图块层数：清单定的"三层"（T8 的 `tiled_map` 同一形状）。`data` 里 MV 实际写了
    /// 六层，本层只取最上面的三层做碰撞与绘制，见模块文档。
    pub const TILE_LAYERS: usize = 3;

    pub fn from_json(src: &str) -> Result<Self, PortError> {
        serde_json::from_str(src).map_err(|e| PortError::Json(e.to_string()))
    }

    /// 从 VFS 读 `data/Map{map_id:03}.json`（MV 的命名约定）。
    pub fn load(vfs: &sisf_asset::Vfs, map_id: u32) -> Result<Self, PortError> {
        Self::from_json(&read_data(vfs, &format!("data/Map{map_id:03}.json"))?)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("地图数据只含数字/字符串/数组")
    }

    /// `data` 里到底有几个整层（`Game_Map.tileId` 的层数就是它）。
    pub fn layers(&self) -> usize {
        let cells = usize::try_from(self.width as u64 * self.height as u64).unwrap_or(usize::MAX);
        // 除数为 0（0 宽/0 高的空图）算 0 层，与 MV 读到空 `data` 同义。
        self.data.len().checked_div(cells).unwrap_or(0)
    }

    /// 校验 `data` 是整层（不然 `tile()` 会读到隔壁层去）。
    pub fn check_data(&self, map: u32) -> Result<(), PortError> {
        let cells = usize::try_from(self.width as u64 * self.height as u64).unwrap_or(usize::MAX);
        if cells == 0 || !self.data.len().is_multiple_of(cells) {
            return Err(PortError::BadDataLen {
                map,
                width: self.width,
                height: self.height,
                len: self.data.len(),
            });
        }
        Ok(())
    }

    pub fn in_bounds(&self, x: u32, y: u32) -> bool {
        x < self.width && y < self.height
    }

    /// `(x, y, 层)` 的图块号；越界或该格没写 ⇒ 0（`tileId` 末尾那个 `|| 0`）。
    pub fn tile(&self, x: u32, y: u32, z: usize) -> u32 {
        let (w, h) = (self.width as usize, self.height as usize);
        if !self.in_bounds(x, y) || z >= self.layers() {
            return 0;
        }
        *self
            .data
            .get((z * h + y as usize) * w + x as usize)
            .unwrap_or(&0)
    }

    /// 区域号：MV 把它放在 z=5 那一层（`Game_Map.regionId`）。
    pub fn region(&self, x: u32, y: u32) -> u32 {
        self.tile(x, y, 5)
    }

    /// 循环地图上的取模（`roundX`：只在 `scrollType` 允许的那个轴上循环）。
    pub fn round_x(&self, x: i64) -> u32 {
        let w = i64::from(self.width.max(1));
        if self.scroll_type == 2 || self.scroll_type == 3 {
            x.rem_euclid(w) as u32
        } else {
            x.clamp(0, (w - 1).max(0)) as u32
        }
    }

    pub fn round_y(&self, y: i64) -> u32 {
        let h = i64::from(self.height.max(1));
        if self.scroll_type == 1 || self.scroll_type == 3 {
            y.rem_euclid(h) as u32
        } else {
            y.clamp(0, (h - 1).max(0)) as u32
        }
    }

    /// 沿 `dir`（2/4/6/8）走一格的落点，已按循环规则折回。
    pub fn step(&self, x: u32, y: u32, dir: u8) -> (u32, u32) {
        let (dx, dy): (i64, i64) = match dir {
            2 => (0, 1),
            4 => (-1, 0),
            6 => (1, 0),
            8 => (0, -1),
            _ => (0, 0),
        };
        (
            self.round_x(i64::from(x) + dx),
            self.round_y(i64::from(y) + dy),
        )
    }

    /// 能不能从 `(x, y)` 朝 `dir` 走一步 —— `Game_Map.checkPassage` 的逐字翻译：
    /// 依次看"`ev_tiles`（站在格子上的事件图块）+ z=2..0 三层图块"，`NO_EFFECT` 的跳过，
    /// 第一个有表态的图块说了算；全都"无表态"⇒ 不通（MV 的 `return false` 兜底）。
    ///
    /// MV 在这一层查的是**出发格**的 `dir`；`Game_CharacterBase.isMapPassable` 还要
    /// 目标格的反向 `dir` 也点头，那半条在 [`crate::scene`] 里补。
    pub fn passable(&self, ts: &TilesetDef, x: u32, y: u32, dir: u8, ev_tiles: &[u32]) -> bool {
        let bit = passage::bit(dir);
        let mut tiles = ev_tiles.to_vec();
        tiles.extend((0..Self::TILE_LAYERS).rev().map(|z| self.tile(x, y, z)));
        self_passage(ts, bit, &tiles)
    }

    /// 事件号列表（跳过 `null` 洞）。
    pub fn event_ids(&self) -> Vec<u32> {
        self.events
            .iter()
            .enumerate()
            .filter(|(_, e)| e.is_some())
            .map(|(i, _)| i as u32)
            .collect()
    }

    pub fn event(&self, id: u32) -> Option<&MapEvent> {
        self.events.get(id as usize).and_then(|e| e.as_ref())
    }
}

/// `checkPassage` 的纯函数内核（`tiles` 已按 MV 的顺序排好）。
fn self_passage(ts: &TilesetDef, bit: u32, tiles: &[u32]) -> bool {
    for &t in tiles {
        let flag = ts.flag(t);
        if flag & passage::NO_EFFECT != 0 {
            continue;
        }
        // `bit` 为 0（非法朝向）时 `flag & 0 == 0` ⇒ 判为通；调用方负责只传 2/4/6/8。
        return flag & bit == 0;
    }
    false
}

/// `data/System.json` 里本层真会读到的那部分。
///
/// 核心脚本对 `System.json` 的全部读点就是 `switches` / `variables` 两张名字表
/// （`Game_Switches.setValue` 用 `switchId < $dataSystem.switches.length` 做**边界**），
/// 加上的确要用的标题、两个特殊地图号与新游戏出生点。键位不在这里（见 [`KEY_MAPPER`]）。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SystemDef {
    pub game_title: String,
    /// 索引 = 开关号（MV 把 0 号写成空串占位）。
    pub switches: Vec<String>,
    pub variables: Vec<String>,
    pub title_id: u32,
    pub gameover_id: u32,
    pub locale: String,
    /// 新游戏的出生点（`Scene_Boot.startGamePlayer` 读的就是这四位数，MV 写死在
    /// `data/System.json` 而不是地图里）。`start_map_id` 是图号 = `data/Map{NNN}.json`。
    pub start_map_id: u32,
    pub start_x: u32,
    pub start_y: u32,
    /// 2/4/6/8；MV 编辑器允许 0 = "朝上"以外的无效值，[`crate::scene::Scene_Map::enter`]
    /// 自己不认的朝向会留默认值。
    pub start_direction: u8,
}

impl SystemDef {
    pub fn from_json(src: &str) -> Result<Self, PortError> {
        serde_json::from_str(src).map_err(|e| PortError::Json(e.to_string()))
    }

    pub fn load(vfs: &sisf_asset::Vfs) -> Result<Self, PortError> {
        Self::from_json(&read_data(vfs, "data/System.json")?)
    }

    /// 开关号在不在名字表里（MV 的写边界，`0 < id < switches.len()`）。
    pub fn has_switch(&self, id: u32) -> bool {
        id > 0 && usize::try_from(id).unwrap_or(usize::MAX) < self.switches.len()
    }

    pub fn has_variable(&self, id: u32) -> bool {
        id > 0 && usize::try_from(id).unwrap_or(usize::MAX) < self.variables.len()
    }

    pub fn switch_name(&self, id: u32) -> &str {
        self.switches
            .get(id as usize)
            .map(String::as_str)
            .unwrap_or("")
    }

    pub fn variable_name(&self, id: u32) -> &str {
        self.variables
            .get(id as usize)
            .map(String::as_str)
            .unwrap_or("")
    }

    /// MV 惯例的键位表 → 引擎的 `ActionMap`（见 [`KEY_MAPPER`]）。
    pub fn action_map(&self) -> sisf_event::ActionMap {
        action_map()
    }
}

/// 一个按键码及其绑到的 MV 按钮名。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyBinding {
    /// JS 侧的 `keyCode`。
    pub code: u32,
    /// `Input.keyMapper` 的值：`ok/cancel?` —— MV 里没有 `cancel`，取消是 `escape`。
    pub button: &'static str,
    /// 对应的 winit 物理键名（`Trigger::Key` 的词汇表）。
    pub key: &'static str,
}

/// `Input.keyMapper` 的逐字搬运（`js/rpg_core/Input.js`）。
///
/// 三点值得注意，都是"看着像笔误其实是上游"的地方：
/// - **`cancel` 这个按钮名在 MV 里不存在**：取消/菜单都是 `escape`，`Scene_Map` 收到
///   `isTriggered('cancel')` 才开菜单 —— 所以这里把 `escape` 同时翻成引擎的
///   `cancel` 与 `menu` 两个动作（见 [`action_map`]）。
/// - `87: 'pagedown'` ⇒ **W 在 MV 里不是"上"**，是 PageDown。引擎自己的
///   `ActionMap::rpg_maker()` 把 `KeyW` 给了 `up`，那是给自家示例的便利，移植工程里
///   按这张表覆盖掉。
/// - `17`/`18` 都叫 `control`；`45`(Insert) 与 `96`(小键盘 0) 也走 `escape`。
pub const KEY_MAPPER: &[KeyBinding] = &[
    KeyBinding {
        code: 9,
        button: "tab",
        key: "Tab",
    },
    KeyBinding {
        code: 13,
        button: "ok",
        key: "Enter",
    },
    KeyBinding {
        code: 16,
        button: "shift",
        key: "ShiftLeft",
    },
    KeyBinding {
        code: 17,
        button: "control",
        key: "ControlLeft",
    },
    KeyBinding {
        code: 18,
        button: "control",
        key: "AltLeft",
    },
    KeyBinding {
        code: 27,
        button: "escape",
        key: "Escape",
    },
    KeyBinding {
        code: 32,
        button: "ok",
        key: "Space",
    },
    KeyBinding {
        code: 33,
        button: "pageup",
        key: "PageUp",
    },
    KeyBinding {
        code: 34,
        button: "pagedown",
        key: "PageDown",
    },
    KeyBinding {
        code: 37,
        button: "left",
        key: "ArrowLeft",
    },
    KeyBinding {
        code: 38,
        button: "up",
        key: "ArrowUp",
    },
    KeyBinding {
        code: 39,
        button: "right",
        key: "ArrowRight",
    },
    KeyBinding {
        code: 40,
        button: "down",
        key: "ArrowDown",
    },
    KeyBinding {
        code: 45,
        button: "escape",
        key: "Insert",
    },
    KeyBinding {
        code: 81,
        button: "pageup",
        key: "KeyQ",
    },
    KeyBinding {
        code: 87,
        button: "pagedown",
        key: "KeyW",
    },
    KeyBinding {
        code: 88,
        button: "escape",
        key: "KeyX",
    },
    KeyBinding {
        code: 90,
        button: "ok",
        key: "KeyZ",
    },
    KeyBinding {
        code: 96,
        button: "escape",
        key: "Numpad0",
    },
    KeyBinding {
        code: 98,
        button: "down",
        key: "Numpad2",
    },
    KeyBinding {
        code: 100,
        button: "left",
        key: "Numpad4",
    },
    KeyBinding {
        code: 102,
        button: "right",
        key: "Numpad6",
    },
    KeyBinding {
        code: 104,
        button: "up",
        key: "Numpad8",
    },
    KeyBinding {
        code: 120,
        button: "debug",
        key: "F9",
    },
];

/// 把 [`KEY_MAPPER`] 编成引擎的动作表。
///
/// 按钮名 → 动作名的映射只做了两处必要的合并：`escape` 同时是 `cancel` 和 `menu`
/// （MV 用同一个键干两件事，靠场景决定），`control`/`tab`/`debug` 原样成名。
/// 手柄腿**不在这里**：MV 的核心脚本没有任何手柄支持（`Input.js` 只有键盘与触摸），
/// 要手柄请走 [`action_map_with_pads`]。
pub fn action_map() -> sisf_event::ActionMap {
    use sisf_event::{ActionDef, Autofire, Trigger};
    // (动作名, 接受的 MV 按钮名, one_shot, 连发)
    let spec: [(&str, &[&str], bool, Option<Autofire>); 11] = [
        ("up", &["up"], false, None),
        ("down", &["down"], false, None),
        ("left", &["left"], false, None),
        ("right", &["right"], false, None),
        ("ok", &["ok"], true, Some(Autofire::RPG_MAKER)),
        // 见上面的合并说明：MV 里"取消"和"开菜单"都是 escape。
        ("cancel", &["escape"], true, Some(Autofire::RPG_MAKER)),
        ("menu", &["escape"], true, None),
        ("shift", &["shift"], false, None),
        ("pageup", &["pageup"], true, Some(Autofire::RPG_MAKER)),
        ("pagedown", &["pagedown"], true, Some(Autofire::RPG_MAKER)),
        ("debug", &["debug"], true, None),
    ];
    let triggers = |buttons: &[&str]| {
        KEY_MAPPER
            .iter()
            .filter(|k| buttons.contains(&k.button))
            .map(|k| Trigger::key(k.key))
            .collect::<Vec<_>>()
    };
    sisf_event::ActionMap {
        actions: spec
            .iter()
            .map(|(name, buttons, one_shot, autofire)| ActionDef {
                name: (*name).to_owned(),
                triggers: triggers(buttons),
                one_shot: *one_shot,
                autofire: *autofire,
            })
            .collect(),
    }
}

/// [`action_map`] 的键盘腿 + `ActionMap::rpg_maker()` 的手柄腿（`Trigger::Pad` 全部搬过来）。
///
/// 存在的理由：MV 工程只给键盘答案，而三端验证要能用手柄跑同一条链。键盘腿**以 MV 为准**
/// （`KeyW` 在 MV 是 PageDown，不是"上"），只把 `rpg_maker()` 里那些 `Trigger::Pad` 补进
/// 同名动作；动作名对不上的（MV 没有的）整条追加。
pub fn action_map_with_pads() -> sisf_event::ActionMap {
    use sisf_event::Trigger;
    let mut mv = action_map();
    for base in sisf_event::ActionMap::rpg_maker().actions {
        if let Some(mine) = mv.actions.iter_mut().find(|a| a.name == base.name) {
            mine.triggers.extend(
                base.triggers
                    .into_iter()
                    .filter(|t| matches!(t, Trigger::Pad(_))),
            );
        } else {
            mv.actions.push(base);
        }
    }
    mv
}

#[cfg(test)]
mod tests {
    use super::*;
    use sisf_event::Trigger;
    // T12 约定：这一组是纯逻辑，wasm 上照样跑（`cargo xtask wasm-test`）。
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    /// 一份"能进图"的最小 MV 地图：4x3，第二层右下有一格墙，事件 1 站在 (1,1)。
    fn map_json() -> &'static str {
        r#"{
            "autoplayBgm":true,"autoplayBgs":false,"battleback1Name":"","battleback2Name":"",
            "bgm":{"name":"Field01","pan":0,"pitch":100,"volume":90},
            "disableDashing":false,"displayName":"Forest","encounterList":[],"encounterStep":30,
            "height":3,"note":"","parallaxLoopX":false,"parallaxName":"","scrollType":2,
            "showName":true,"specifyBattleback":false,"tilesetId":1,"width":4,
            "events":[null,
                {"id":1,"name":"Elder","x":1,"y":1,"note":"","pages":[
                    {"conditions":{"actorId":1,"actorValid":false,"itemId":1,"itemValid":false,
                        "selfSwitchCh":"A","selfSwitchValid":false,"switch1Id":3,"switch1Valid":true,
                        "switch2Id":0,"switch2Valid":false,"variableId":2,"variableValid":true,"variableValue":1},
                     "directionFix":false,"image":{"characterIndex":0,"characterName":"Actor1",
                        "direction":2,"pattern":0,"tileId":0},
                     "list":[{"code":101,"indent":0,"parameters":["",0,0,0]},
                             {"code":401,"indent":0,"parameters":["井里有光。"]}],
                     "moveFrequency":3,"moveRoute":{"list":[{"code":0,"parameters":[]}],
                        "repeat":true,"skippable":false,"wait":false,"walk":false},
                     "moveSpeed":3,"moveType":0,"priorityType":1,"stepAnime":false,
                     "through":false,"trigger":0,"walkAnime":true}]}],
            "data":[0,0,0,0, 0,0,0,0, 0,0,0,0,
                    0,0,0,0, 0,0,0,0, 0,0,0,0,
                    0,0,0,0, 0,0,0,0, 0,0,0,7,
                    0,0,0,0, 0,0,0,0, 0,0,0,0,
                    0,0,0,0, 0,0,0,0, 0,0,0,0,
                    0,0,0,0, 0,0,0,0, 0,0,0,0]
        }"#
    }

    /// 验收（T11）：一张 MV 风格地图加载得起来，未知字段与 `null` 洞都不影响。
    #[test]
    fn a_real_mv_map_loads_with_unknown_fields_and_null_events() {
        let m = MapDef::from_json(map_json()).expect("加载地图");
        assert_eq!((m.width, m.height), (4, 3));
        assert_eq!(m.tileset_id, 1);
        assert_eq!(m.display_name, "Forest");
        assert!(m.autoplay_bgm);
        // 自动播的 BGM 折成引擎侧的 `Sound`：路径按 MV 的目录约定，增益 = volume/100。
        assert_eq!(
            m.bgm.sound("bgm"),
            Some(sisf_event::Sound::new("audio/bgm/Field01.ogg").gain(0.9))
        );
        assert!(
            m.bgs.sound("bgs").is_none(),
            "`bgs` 字段整个缺失 = 空名字，不是「静音的同一首」"
        );
        assert_eq!(m.layers(), 6, "data 有 6 层（4 图块 + 阴影 + 区域）");
        assert!(m.check_data(1).is_ok());
        // 事件号 = 下标；0 号洞保留。
        assert_eq!(m.event_ids(), vec![1]);
        assert!(m.event(0).is_none());
        let e = m.event(1).expect("事件 1");
        assert_eq!((e.x, e.y, e.name.as_str()), (1, 1, "Elder"));
        let p = &e.pages[0];
        assert_eq!(p.trigger, 0);
        assert!(p.conditions.switch1_valid);
        assert_eq!(p.conditions.self_switch_ch, "A");
        // 图块：z=2 的 (3,2) 是 7，其余 0。
        assert_eq!(m.tile(3, 2, 2), 7);
        assert_eq!(m.tile(3, 2, 1), 0);
        assert_eq!(m.tile(9, 9, 0), 0, "越界读成 0 而不是 panic");
    }

    /// 通行按 `checkPassage` 的规矩来：**位是"阻挡"**，最上层先表态，`[*]` 才继续往下找。
    #[test]
    fn passage_follows_mv_flag_precedence() {
        let mut ts = TilesetDef {
            flags: vec![0; 8],
            ..TilesetDef::default()
        };
        ts.flags[7] = 1 << 1; // 7 号图块：bit1 = "左不通"（方向 4）
        ts.flags[0] = 0; // 空洞 = 全通
        let m = MapDef::from_json(map_json()).unwrap();
        // (3,2) 的 z=2 是 7 ⇒ 朝左不通，朝下通。
        assert!(!m.passable(&ts, 3, 2, 4, &[]));
        assert!(m.passable(&ts, 3, 2, 2, &[]));
        // 空格子照旧全通。
        assert!(m.passable(&ts, 0, 0, 4, &[]));
        // `[*]`（0x10）盖在上面时不看它，继续看下一层。
        ts.flags[6] = passage::NO_EFFECT;
        let mut star = MapDef::from_json(map_json()).unwrap();
        let cells = (star.width * star.height) as usize;
        star.data[2 * cells + 2 * 4 + 3] = 6; // z=2 的 (3,2) 换成 `[*]`
        assert!(
            star.passable(&ts, 3, 2, 4, &[]),
            "无表态的图块要让位给下一层"
        );
        // 站在格子上的**图块事件**排在图块层前面（`allTiles`）：6 号无表态，换成会表态的
        // 7 号就由它说了算。
        assert!(!star.passable(&ts, 3, 2, 4, &[7]));
        // 层数不够时 tile() 直接给 0（= 通），而不是读错层。
        assert_eq!(star.tile(0, 0, 99), 0);
    }

    /// 循环性只由 `scrollType` 决定（MV 没有两个布尔字段）。
    #[test]
    fn scroll_type_drives_wrapping() {
        let m = MapDef::from_json(map_json()).unwrap(); // scrollType 2 = 横向循环
        assert_eq!(m.step(3, 0, 6), (0, 0), "横向该绕回");
        assert_eq!(m.step(0, 0, 8), (0, 0), "纵向到头就停");
        assert_eq!(m.round_x(-1), 3);
        let mut both = m.clone();
        both.scroll_type = 3;
        assert_eq!(both.step(0, 0, 8), (0, 2), "四向循环");
        both.scroll_type = 0;
        assert_eq!(both.step(3, 0, 6), (3, 0), "闭合边界");
    }

    /// 名字表就是 MV 的写边界：`0 < id < len()`，超出的一律不生效。
    #[test]
    fn system_name_table_bounds_ids() {
        let s = SystemDef {
            game_title: "深渊之井".into(),
            switches: vec![String::new(), "开门".into()],
            variables: vec![String::new(), "线索".into()],
            title_id: 1,
            gameover_id: 2,
            locale: "ja_JP".into(),
            start_map_id: 1,
            start_x: 8,
            start_y: 6,
            start_direction: 2,
        };
        assert!(s.has_switch(1) && !s.has_switch(2) && !s.has_switch(0));
        assert_eq!(s.switch_name(1), "开门");
        assert_eq!(s.variable_name(9), "");
    }

    /// 键位表逐条对得上，而且 W 在 MV 里真是 PageDown 不是"上"。
    #[test]
    fn keymapper_produces_the_engine_action_table() {
        let am = action_map();
        let find = |n: &str| am.actions.iter().find(|a| a.name == n).expect("动作存在");
        let keys = |n: &str| -> Vec<String> {
            find(n)
                .triggers
                .iter()
                .filter_map(|t| match t {
                    Trigger::Key(k) => Some(k.clone()),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(keys("ok"), ["Enter", "Space", "KeyZ"]);
        assert_eq!(keys("up"), ["ArrowUp", "Numpad8"]);
        assert_eq!(keys("pagedown"), ["PageDown", "KeyW"]);
        assert!(!keys("up").contains(&"KeyW".to_owned()));
        // escape 一键两用：cancel 与 menu 共用。
        assert_eq!(keys("cancel"), ["Escape", "Insert", "KeyX", "Numpad0"]);
        assert_eq!(keys("menu"), keys("cancel"));
        assert!(find("ok").one_shot && find("up").autofire.is_none());
        // 手柄腿可选地补进来，且不动键盘腿。
        let with = action_map_with_pads();
        let pad = with
            .actions
            .iter()
            .find(|a| a.name == "ok")
            .unwrap()
            .triggers
            .iter()
            .filter(|t| matches!(t, Trigger::Pad(_)))
            .count();
        assert_eq!(pad, 1, "MV 自己没有手柄表，手柄腿来自 rpg_maker()");
        assert_eq!(
            with.actions
                .iter()
                .find(|a| a.name == "ok")
                .unwrap()
                .triggers
                .iter()
                .filter(|t| matches!(t, Trigger::Key(_)))
                .count(),
            3
        );
    }

    /// 验收（T11）：工程三件套按 MV 的文件名约定从 VFS 读齐 —— 散文件目录与 zip 包走的是
    /// 同一条 `VfsProvider::read`，所以这条通了那两条都通。
    #[test]
    fn the_three_data_files_load_by_mv_naming() {
        let mut mem = sisf_asset::MemoryProvider::new();
        let mut put = |name: &str, src: &str| {
            let p = sisf_asset::VfsPath::new(name).unwrap();
            mem.insert(&p, src.as_bytes());
        };
        // 三位零填充是 MV 的硬约定（`DataManager.loadMapData` 里那句 `padLeft($dataMapId)`）。
        put("data/Map007.json", map_json());
        put(
            "data/System.json",
            r#"{"gameTitle":"深渊之井","switches":["","开门"],"variables":["","线索"],"titleId":1,"gameoverId":2,"locale":"zh_CN","startMapId":7,"startX":12,"startY":9,"startDirection":6}"#,
        );
        // `Tilesets.json` 是顶层数组，下标 0 是 `null` 洞。
        put(
            "data/Tilesets.json",
            r#"[null,{"id":1,"mode":1,"flags":[0,0,0,0,0,0,0,7],"tilesetNames":["A1","A2","A3","","","","","",""]}]"#,
        );
        let mut vfs = sisf_asset::Vfs::new();
        vfs.register(Box::new(mem));

        let m = MapDef::load(&vfs, 7).expect("读 Map007");
        assert_eq!(m.display_name, "Forest");
        let s = SystemDef::load(&vfs).expect("读 System");
        assert_eq!(s.switch_name(1), "开门");
        assert_eq!(s.game_title, "深渊之井");
        // 出生点在 `System.json` 而不在地图里（`Scene_Boot.startGamePlayer`）。
        assert_eq!(
            (s.start_map_id, s.start_x, s.start_y, s.start_direction),
            (7, 12, 9, 6)
        );
        let t = TilesetDef::load(&vfs, 1).expect("读图块组 1");
        assert_eq!(t.flag(7), 7);
        assert_eq!(t.tileset_name(2), "A3");
        // 找不着的两种：文件不在，与文件在但没这一条。都是 `Missing`，不是 panic。
        assert!(matches!(MapDef::load(&vfs, 8), Err(PortError::Missing(_))));
        assert!(matches!(
            TilesetDef::load(&vfs, 9),
            Err(PortError::Missing(_))
        ));
    }
}
