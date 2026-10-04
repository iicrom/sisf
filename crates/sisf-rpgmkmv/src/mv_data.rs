//! `data/*.json` 的读取层：先做上游形状修正，再交给引擎的 `*Def::from_json`。
//!
//! 为什么要这一层（两条都是实测，不是防御性臆想）：
//!
//! 1. **`"indent": null`**。MV 的 `JsonEx` 把**没赋值的属性**序列化成 `null`，而移动路线的
//!    每一步（`ROUTE_*`，码 1–19 与 41–45）从来不写 `indent`。真工程里有 **31,205 条**路线步
//!    是 null（分布在 480 张图上），另有 116 条在 `CommonEvents.json`。引擎的
//!    `RawCommand.indent` 是 `#[serde(default)] u32` —— `default` 只补**缺键**、不补 `null`
//!    ⇒ 直接 `MapDef::load` 会在那 480 张图上失败。折成 0 的口径：MV 运行时对这些值做的是
//!    `undefined >= indent`（NaN 比较 ⇒ false），等价于"这一行不比当前层深"，与 0 同义；
//!    路线步本来也没有嵌套，0 不改变任何块边界。
//! 2. **`data` 里的负数图块号**。21 张图的 `data` 数组里共 **36 个**负数（`-1073741824` =
//!    `0xC0000000`、`-2147483633` = `0x8000000F` 这类）—— 那是插件往图块层写进自己的旗标位。
//!    引擎的 `MapDef.data` 是 `Vec<u32>`。这里按**位型折回**（`-1073741824 → 3221225472`）：
//!    数值信息一个比特都不丢，而它在引擎里落到 `TilesetDef::flag()` 的越界分支 ⇒ 0 = 全通行、
//!    无特效，与 MV 的 `flags[negative] || 0` 同一个上层结果。
//!
//! 除这两条以外 `data/*.json` 里没有别的"引擎读不了的形状"：全量扫过 694 张图 + 469 个公共
//! 事件，非 `indent` 的 null 键 0 个；负数只出现在 `parameters`（`serde_json::Value`，无所谓）
//! 与 `System.json` 的 `windowTone`（引擎不读那个键）。

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use sisf_asset::Vfs;
use sisf_asset::VfsPath;
use sisf_rpgmk::map::{MapDef, TilesetDef};

/// 一个公共事件（`CommonEvents.json` 的一条）。
#[derive(Clone, Debug, serde::Deserialize)]
pub struct CommonEvent {
    #[serde(default)]
    pub id: u32,
    #[serde(default)]
    pub name: String,
    /// 0 调用型 / 1 自动 / 2 并行（真工程只有 CE 211 是 2）。
    #[serde(default)]
    pub trigger: u8,
    #[serde(default)]
    pub list: Vec<sisf_rpgmk::map::RawCommand>,
}

/// `data/*.json` 读不动。
#[derive(Debug)]
pub enum MvDataError {
    Missing(String),
    Json(String),
}

impl std::fmt::Display for MvDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MvDataError::Missing(n) => write!(f, "VFS 里没有 {n}"),
            MvDataError::Json(e) => write!(f, "JSON 读取失败: {e}"),
        }
    }
}

impl std::error::Error for MvDataError {}

/// 一次归一改了多少。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NormCounts {
    /// `indent: null → 0` 的条数。
    pub indent_fixed: usize,
    /// `data` 里负数图块号折成 u32 位型的个数。
    pub tiles_wrapped: usize,
}

/// 按模块文档那两条规则归一。
pub fn normalize(value: &mut serde_json::Value) -> NormCounts {
    let mut counts = NormCounts::default();
    walk(value, &mut counts);
    counts
}

/// 只关心 `indent` 那一条规则的便捷入口。
pub fn fix_null_indent(value: &mut serde_json::Value) -> usize {
    normalize(value).indent_fixed
}

fn walk(value: &mut serde_json::Value, counts: &mut NormCounts) {
    match value {
        // 指令对象的特征是带 `code`；只有这种对象的 `indent` 才归一。
        serde_json::Value::Object(map) => {
            if map.contains_key("code")
                && let Some(v) = map.get_mut("indent")
                && v.is_null()
            {
                *v = serde_json::Value::from(0);
                counts.indent_fixed += 1;
            }
            for (k, v) in map.iter_mut() {
                // `data` 才是图块层数组：只有那里的负数是"图块号"。
                if k == "data" {
                    wrap_negative_tiles(v, counts);
                }
                walk(v, counts);
            }
        }
        serde_json::Value::Array(items) => {
            for v in items.iter_mut() {
                walk(v, counts);
            }
        }
        _ => {}
    }
}

fn wrap_negative_tiles(value: &mut serde_json::Value, counts: &mut NormCounts) {
    if let serde_json::Value::Array(items) = value {
        for v in items.iter_mut() {
            let Some(n) = v.as_i64() else { continue };
            if n < 0 {
                *v = serde_json::Value::from(n as u32 as u64);
                counts.tiles_wrapped += 1;
            }
        }
    }
}

/// 缓存表的形状：按（规范化路径, 类型）在进程里存一份解析结果。
type ParsedTable = Mutex<HashMap<(String, TypeId), Arc<dyn Any + Send + Sync>>>;

/// 一份 `data/*.json` 的解析结果，进程级唯一。
static PARSED: OnceLock<ParsedTable> = OnceLock::new();

/// 读一份 `data/*.json`，**同一个进程里只解析一次**，交回 `Arc`（调用方不复制整张表）。
///
/// 上游 `Scene_Boot` 的 `DataManager.loadDatabase` 一次把全部 `data/*.json` 读成内存对象，
/// 之后每一次用都是引用；移植侧原来每算一族闭包就把 1.4MB 的 `CommonEvents.json` 重读重解一遍
/// —— 实测一族 ~90ms、五族 ~550ms，而换图那一拍要算两遍（进图门槛 + 美术那一波）
/// ⇒ 浏览器腿"换图很慢"的主要成本是这一格，不是网络。
///
/// 键只有路径：VFS 在整个进程里是**只读**的（素材只在内存解密，一个字节都不写），
/// 同一个路径不会换内容。将来若有判据要拿假数据顶掉同一份路径，先调 [`clear_parsed`]。
pub fn shared_with<T>(
    vfs: &Vfs,
    name: &str,
    build: impl FnOnce(&Vfs) -> Result<T, MvDataError>,
) -> Result<Arc<T>, MvDataError>
where
    T: Any + Send + Sync + 'static,
{
    let path = VfsPath::new(name).map_err(|e| MvDataError::Missing(e.to_string()))?;
    let key = (path.key().to_owned(), TypeId::of::<T>());
    let table = PARSED.get_or_init(Default::default);
    let hit = table.lock().unwrap().get(&key).cloned();
    if let Some(hit) = hit {
        // `Arc<dyn Any>::downcast` 就是"这份缓存正好是同一个类型"的意思；不是就当没命中。
        if let Ok(typed) = hit.downcast::<T>() {
            return Ok(typed);
        }
    }
    let built: Arc<T> = Arc::new(build(vfs)?);
    // 解析在锁外（一次 ~90ms，抱着锁会把判据那 240 条并发用例弄成串行）。
    // 于是两个线程可能同时算出同一份 —— 这里让**先放进去的那一份赢**，所有人共享它，
    // `Arc::ptr_eq` 才是稳定的。
    let mut guard = table.lock().unwrap();
    match guard.get(&key).cloned() {
        Some(other) => Ok(other.downcast::<T>().unwrap_or_else(|_| built.clone())),
        None => {
            guard.insert(key, built.clone() as Arc<dyn Any + Send + Sync>);
            Ok(built)
        }
    }
}

/// [`shared_with`] 的 serde 那一格：走 [`load`] 的读法（含归一）。
pub fn shared<T>(vfs: &Vfs, name: &str) -> Result<Arc<T>, MvDataError>
where
    T: serde::de::DeserializeOwned + Any + Send + Sync + 'static,
{
    shared_with(vfs, name, |v| load::<T>(v, name))
}

/// `System.json` 里引擎的 `SystemDef` **没有**的那一格：开局队伍（`partyMembers`）。
///
/// 上游 `Game_Party.setupStartingMembers` 就是照这一格填 `$gameParty._actors`，
/// 实测真工程是 `[1, 2]`。
///
/// 走 [`shared_with`]（键带 `TypeId` ⇒ 与引擎那份 `SystemDef` 各存各的，两条读法互不干扰）。
pub fn party_members(vfs: &Vfs) -> Result<Vec<u32>, MvDataError> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Extras {
        #[serde(default)]
        party_members: Vec<u32>,
    }
    let all = shared_with(vfs, "data/System.json", |v| {
        load::<Extras>(v, "data/System.json")
    })?;
    Ok(all.party_members.clone())
}

/// `data/Tilesets.json` 里的第 `id` 组，**整张表在一个进程里只解一次**。
///
/// 读法与引擎的 `TilesetDef::load` 一模一样（顶层数组、下标 0 是空槽、**按 `id` 字段找**而不是
/// 按下标猜，也不归一 —— 归一那两条形状只挂在地图与公共事件的指令上），差别只是这里缓存了
/// 那 2.4MB 的解析结果。
pub fn tileset(vfs: &Vfs, id: u32) -> Result<TilesetDef, MvDataError> {
    let all = shared_with(
        vfs,
        "data/Tilesets.json",
        |v| -> Result<Vec<Option<TilesetDef>>, MvDataError> {
            let path = VfsPath::new("data/Tilesets.json")
                .map_err(|e| MvDataError::Missing(e.to_string()))?;
            let bytes = v
                .read(&path)
                .ok_or_else(|| MvDataError::Missing("data/Tilesets.json".into()))?;
            let text = String::from_utf8_lossy(&bytes);
            serde_json::from_str(text.trim_start_matches('\u{feff}'))
                .map_err(|e| MvDataError::Json(e.to_string()))
        },
    )?;
    all.iter()
        .flatten()
        .find(|t| t.id == id)
        .cloned()
        .ok_or_else(|| MvDataError::Missing(format!("data/Tilesets.json 里没有 id={id}")))
}

/// 清掉解析缓存（假数据判据要用同一个路径时先清一次；桌面与浏览器两条腿各自进程，互不影响）。
pub fn clear_parsed() {
    if let Some(t) = PARSED.get() {
        t.lock().unwrap().clear();
    }
}

/// 读一份 `data/*.json`：VFS → 文本 → `Value` → 归一 → 目标类型。
pub fn load<T>(vfs: &Vfs, name: &str) -> Result<T, MvDataError>
where
    T: serde::de::DeserializeOwned,
{
    load_norm(vfs, name).map(|(v, _)| v)
}

/// [`load`] 加上归一计数（账单与判据要引用这些读数）。
pub fn load_norm<T>(vfs: &Vfs, name: &str) -> Result<(T, NormCounts), MvDataError>
where
    T: serde::de::DeserializeOwned,
{
    let path = VfsPath::new(name).map_err(|e| MvDataError::Missing(e.to_string()))?;
    let bytes = vfs
        .read(&path)
        .ok_or_else(|| MvDataError::Missing(name.to_owned()))?;
    let text = String::from_utf8_lossy(&bytes);
    // 上游 `data/*.json` 实测无 BOM，多一道 trim 不亏。
    let mut value: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| MvDataError::Json(e.to_string()))?;
    let counts = normalize(&mut value);
    let parsed = serde_json::from_value(value).map_err(|e| MvDataError::Json(e.to_string()))?;
    Ok((parsed, counts))
}

/// `data/Map{NNN}.json`。把 MV 的命名约定收在一处。
pub fn load_map(vfs: &Vfs, map_id: u32) -> Result<MapDef, MvDataError> {
    load(vfs, &format!("data/Map{map_id:03}.json"))
}

/// `data/MapInfos.json` 里的图号（下标 0 是空槽）。
pub fn map_ids(vfs: &Vfs) -> Result<Vec<u32>, MvDataError> {
    #[derive(serde::Deserialize)]
    struct Info {
        id: u32,
    }
    let infos: Vec<Option<Info>> = load(vfs, "data/MapInfos.json")?;
    Ok(infos.into_iter().flatten().map(|i| i.id).collect())
}

/// `data/CommonEvents.json`（下标 0 是空槽，共 469 条）。
pub fn common_events(vfs: &Vfs) -> Result<Vec<Option<CommonEvent>>, MvDataError> {
    load(vfs, "data/CommonEvents.json")
}
