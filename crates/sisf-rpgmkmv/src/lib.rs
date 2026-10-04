//! RPG Maker MV 的**通用层**：数据解析、资产加载，以及顶替 `rpg_*.js` 的 JS 兼容层。
//!
//! ## 四条职责
//!
//! 1. **读 `data/*.json`**：做上游形状修正（`indent: null → 0`、负数图块号折成 u32 位型），
//!    再交给引擎的 `*Def::from_json`。
//! 2. **VFS provider**：把 MV 的磁盘约定翻成引擎能读的字节（逻辑名 → 加密名、明文回退、解密）。
//! 3. **密钥解析**：从 `System.json` 里读出 MV 的 encryptionKey，供解密用。
//! 4. **JS 兼容层**（`js` feature）：[`jsrt`] 是两条腿的宿主与对象模型闸门，
//!    [`shapes`] 从被顶替的 `rpg_*.js` 源码里算全局形状名单并生成跳板表，
//!    `platform` 交出平台件（`Utils`/`Input`/`TouchInput`/`console`/`window` 那一族宿主面），
//!    [`managers`] 交出 managers 族的数据那一半（`Decrypter` 的名字层、`DataManager` 读库、
//!    `TextManager`），[`objects`] 交出 `Game_*` 族的第一条腿（状态类：开关/变量/自我开关/
//!    系统/消息/计时/画面效果/图片层），[`plugins`] 读 `plugins.js` 注册表、逐个装载启用那一族并交出账单，
//!    [`core`] 交出 `rpg_core.js` 的那几格（M10.4 的第一格：`Graphics` 的屏幕读数与报错面）。
//!    目标是让 `js/plugins/` 里的游戏逻辑脚本**原样运行**。
//!
//! ## 为什么这一层存在
//!
//! 引擎的 `sisf-rpgmk` 负责"翻译期"（把 MV 指令树翻成 `Command`），而这一层负责"运行期"
//! （读真素材、解真密钥、提供 VFS，以及跑上游的 JS）。两者分开是为了让别的 MV 工程也能复用
//! 这一整套，而不必绑在某一款游戏的移植代码里。

pub mod decrypt;
pub mod key;
pub mod mv_data;
pub mod mv_provider;
pub mod plugins;
pub mod shapes;

#[cfg(feature = "js")]
pub mod core;
#[cfg(feature = "js")]
pub mod jsrt;
#[cfg(feature = "js")]
pub mod managers;
#[cfg(feature = "js")]
pub mod objects;
#[cfg(feature = "js")]
pub mod platform;

// Re-export 常用的东西，让上层不用写 `sisf_rpgmkmv::mv_data::CommonEvent` 这么长。
pub use decrypt::{HEADER_LEN, decrypt, encrypted_ext, is_encrypted};
pub use key::from_system_json;
pub use mv_data::{CommonEvent, MvDataError, NormCounts, fix_null_indent, normalize};
pub use mv_provider::MvAssets;
