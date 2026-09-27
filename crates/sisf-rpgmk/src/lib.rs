//! RPG Maker MV/MZ 移植兼容层（T11）：磁盘数据读取、事件指令翻译、三张变量表。
//!
//! 这一层**不是**引擎的核心，也不该被核心依赖：它把 MV 工程的那套磁盘约定
//! （`data/Map*.json` + `data/System.json` + `data/Tilesets.json`、`Game_*` 三张表、
//! 事件指令码表）翻译成引擎已有的形状（[`MapDef`](map::MapDef) → 图块查询、
//! [`Command`](sisf_event::Command) 序列、[`VarStore`](sisf_event::VarStore)）。
//! 翻译不了的码降级成 `Command::Custom("rpgmv.unimplemented", …)` 并计数，绝不 panic
//! —— 一个没实现的指令不该让整张地图进不去。
//!
//! 字段表与指令语义都取自官方 `rpgtkoolmv/corescript` 的 `js/rpg_objects/*.js`
//! （`Game_Interpreter` 的 handler 表就是 [`translate`] 的码表），逐条对应关系写在
//! `docs/porting.md`。

#[cfg(all(feature = "archive", not(target_arch = "wasm32")))]
pub mod archive;
pub mod game_vars;
pub mod map;
pub mod scene;
pub mod translate;

#[cfg(all(feature = "archive", not(target_arch = "wasm32")))]
pub use archive::{ArchiveError, ZipProvider};
pub use game_vars::GameVars;
pub use map::{
    EventPage, KEY_MAPPER, MapDef, MapEvent, MvAudio, PageCondition, PageImage, PortError,
    RawCommand, RawRoute, SystemDef, TilesetDef, action_map, action_map_with_pads, passage,
};
pub use scene::{DIRS, Game_Character, PLAYER, Refresh, Scene_Map, Tick, reverse_dir};
pub use translate::{Options, TranslateReport, translate_event, translate_list, translate_route};
