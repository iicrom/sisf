//! MV 的 `$gameSwitches` / `$gameVariables` / `$gameSelfSwitches` 三张表，共用一个
//! [`VarStore`] 后端。
//!
//! ## 为什么是"包一层"而不是另写一套
//!
//! 引擎侧的 `VarStore`（T10）本来就是开关 / 整数 / 文本三态 —— 那个形状就是从
//! RPG Maker 抄来的。移植层只需要补 MV 独有的三件事：
//!
//! 1. **名字表给出的写边界**：`Game_Switches.setValue` 里那句
//!    `switchId > 0 && switchId < $dataSystem.switches.length`（变量同理，且额外
//!    `Math.floor`）。所以 [`GameVars::bind_system`] 把两张名字表的长度记下来。
//! 2. **自我开关的键**：MV 用 `{"1,3,A": true}` 这样的字典，键是
//!    `[mapId, eventId, ch]` 经 JS 数组转字符串得到的**逗号串**。这里把它放进
//!    `VarStore` 的文本槽，加前缀 `ss:` 让开档里能一眼认出是哪一类。
//! 3. **`onChange ⇒ $gameMap.requestRefresh()`**：三张表的每一次写入都要让地图重算
//!    事件页，否则"开关已经开了、那一页事件却没出现"。这就是 [`GameVars::refresh`]。
//!
//! ## 边界只拦宿主，不拦虚拟机
//!
//! `SetVar` 这类指令直接持有 `&mut VarStore`（虚拟机不知道名字表存在），所以越界写
//! **会**落地；MV 那边是静默丢弃。这里选"照写照读"：移植工程里写到名字表之外的开关，
//! 在 MV 是"事件永远不触发"这种查起来极痛的哑失败，在这里只是一个真的开关。
//! 反过来，宿主侧显式走 [`GameVars::set_switch`] / [`GameVars::set_value`] 时按 MV 拦，
//! 因为那条路上调用方就在拿 `SystemDef`，拦下来是它想要的语义。

use serde::{Deserialize, Serialize};
use sisf_event::{CommandError, VarStore};

use crate::map::{PortError, SystemDef};

/// 自我开关在 `VarStore` 文本槽里的键前缀（见模块文档第 2 条）。
const SS_PREFIX: &str = "ss:";

/// MV 存档里 `contents.switches` / `contents.variables` / `contents.selfSwitches`
/// 的形状（`DataManager.makeSaveContents`）。`_class` 是 `JsonEx` 塞进去的类型标记，
/// 本结构不拒未知字段，所以直接忽略即可。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct MvSave {
    #[serde(default)]
    switches: MvTable,
    #[serde(default)]
    variables: MvTable,
    #[serde(default)]
    self_switches: MvDict,
}

/// `{"_data": [null, true, …]}`：MV 的开关/变量数组，**下标 0 恒为空洞**。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct MvTable {
    #[serde(default)]
    _data: Vec<Option<serde_json::Value>>,
}

/// `{"_data": {"1,3,A": true}}`。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct MvDict {
    #[serde(default)]
    _data: std::collections::BTreeMap<String, bool>,
}

/// 一张 MV 风格的游戏状态表。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameVars {
    #[serde(default)]
    store: VarStore,
    /// `$dataSystem.switches.length`；`usize::MAX` = 没绑定名字表 ⇒ 不设界。
    #[serde(default = "uncapped")]
    switch_cap: usize,
    #[serde(default = "uncapped")]
    variable_cap: usize,
    /// `requestRefresh` 那面旗：不序列化（它是"这一拍要说一次的话"，不是存档内容）。
    #[serde(default, skip)]
    refresh: bool,
}

fn uncapped() -> usize {
    usize::MAX
}

/// 手写而不是 derive：derive 出来的 `Default` 会把两张名字表长度当成 0，
/// 于是"新建一张表"变成"这张表拒绝一切写入"——正是最难查的那种静默失败。
impl Default for GameVars {
    fn default() -> Self {
        Self::new()
    }
}

/// 手写而不是 derive：`refresh` 是那面瞬时旗（不序列化），它不参与"两张表是否同态"。
impl PartialEq for GameVars {
    fn eq(&self, other: &Self) -> bool {
        self.store == other.store
            && self.switch_cap == other.switch_cap
            && self.variable_cap == other.variable_cap
    }
}

impl GameVars {
    /// 未绑定名字表（写边界放开）。
    pub fn new() -> Self {
        Self {
            store: VarStore::new(),
            switch_cap: usize::MAX,
            variable_cap: usize::MAX,
            refresh: false,
        }
    }

    /// 指定随机种子的实例（回归与 golden 用；种子跟着存档走，见 `VarStore` 文档）。
    pub fn seeded(seed: u64) -> Self {
        Self {
            store: VarStore::seeded(seed),
            ..Self::new()
        }
    }

    /// 用 `data/System.json` 的两张名字表定写边界（MV 的 `id < length`）。
    pub fn bind_system(&mut self, sys: &SystemDef) -> &mut Self {
        self.switch_cap = sys.switches.len();
        self.variable_cap = sys.variables.len();
        self
    }

    pub fn with_system(sys: &SystemDef) -> Self {
        let mut g = Self::new();
        g.bind_system(sys);
        g
    }

    // --- 三张表的读写 ---------------------------------------------------------

    /// `$gameSwitches.value(id)`：读永远不设界（MV 的 `_data[id]` 读到 `undefined` 即假）。
    pub fn switch_value(&self, id: u32) -> bool {
        self.store.is_on(id)
    }

    /// `$gameSwitches.setValue`。**返回是否真的落了地**（被名字表挡掉时是 `false`）。
    pub fn set_switch(&mut self, id: u32, on: bool) -> bool {
        if !self.in_switches(id) {
            return false;
        }
        self.store.set_switch(id, on);
        self.refresh = true;
        true
    }

    /// `$gameVariables.value(id)`（MV 的 `|| 0`：没写过就是 0）。
    pub fn value(&self, id: u32) -> i64 {
        self.store.int(id)
    }

    /// `$gameVariables.setValue`（MV 在那里 `Math.floor`；`i64` 已经是整数，无需再做）。
    pub fn set_value(&mut self, id: u32, v: i64) -> bool {
        if !self.in_variables(id) {
            return false;
        }
        self.store.set_int(id, v);
        self.refresh = true;
        true
    }

    /// `$gameSelfSwitches.value([map, event, ch])`。
    ///
    /// 规范值是文本 `"1"` 而不是 `"true"`：这一层里自我开关同时有两条写路 ——
    /// 宿主走这里，虚拟机走 `Slot::Var(ss_key(..))` + `set_numeric`，而后者只会写
    /// 十进制文本（`VarStore::numeric` 把 `"true"` 读成 0）。统一成"非 0 即真"才不必
    /// 在两边各记一套真相。
    pub fn self_switch(&self, map: u32, event: u32, ch: &str) -> bool {
        ss_truthy(self.store.text(&Self::ss_key(map, event, ch)))
    }

    /// `$gameSelfSwitches.setValue`：MV 的真值是 `true`，假值是**删键**（`delete this._data[key]`），
    /// 所以这里写假值也把文本置空而不是留字符串 `"0"` —— 读法等价，存档里少一处误导。
    pub fn set_self_switch(&mut self, map: u32, event: u32, ch: &str, on: bool) {
        let key = Self::ss_key(map, event, ch);
        // 键里那个 `ch` 在 MV 编辑器里是 'A'..'J'，但运行期就是个字符串，工程里见过 '0'。
        let v = if on { "1" } else { "" };
        self.store.set_text(key, v);
        self.refresh = true;
    }

    /// MV 那个键的字符串形状（`String([1,3,'A'])` === `"1,3,A"`）。
    pub fn ss_key(map: u32, event: u32, ch: &str) -> String {
        format!("{SS_PREFIX}{map},{event},{ch}")
    }

    fn in_switches(&self, id: u32) -> bool {
        id > 0 && usize::try_from(id).unwrap_or(usize::MAX) < self.switch_cap
    }

    fn in_variables(&self, id: u32) -> bool {
        id > 0 && usize::try_from(id).unwrap_or(usize::MAX) < self.variable_cap
    }

    // --- 与虚拟机/场景的接缝 ----------------------------------------------------

    /// 交给 `EventRunner` 的那份存储（指令直接读写它，见模块文档"边界只拦宿主"）。
    ///
    /// 拿它写任何东西都算"状态变了" ⇒ 顺手置起 refresh。只想要骰子的调用方请用
    /// [`Self::rng_mut`]。
    pub fn store(&self) -> &VarStore {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut VarStore {
        self.refresh = true;
        &mut self.store
    }

    /// 只要 RNG（`Scene_Map::tick` 每拍都要掷骰）。**不置 refresh** —— 抽一个数不是
    /// "开关变了要说一句话"，置旗会让每一拍都重算事件页，把 `setupPage` 定下的朝向刷掉。
    pub fn rng_mut(&mut self) -> &mut sisf_event::Rng {
        self.store.rng_mut()
    }

    /// 有没有人改过状态（场景据此决定要不要重算事件页）。
    pub fn refresh(&self) -> bool {
        self.refresh
    }

    /// 取走这面旗（读一次清一次，对应 `Game_Map.setup` 之后 `refreshTiles` 的"一次性"）。
    pub fn take_refresh(&mut self) -> bool {
        std::mem::replace(&mut self.refresh, false)
    }

    pub fn request_refresh(&mut self) {
        self.refresh = true;
    }

    // --- 存档 ------------------------------------------------------------------

    /// 引擎格式存档（RON，稳定字节 ⇒ 存档 diff 只反映真变化）。
    ///
    /// 存的是整个 `GameVars` 而不是里面的 `VarStore`：两张名字表的长度（`switch_cap`
    /// / `variable_cap`）也是状态的一部分，只存 store 会让读档后"边界凭空消失"。
    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .expect("三张表只含整数与文本，序列化不会失败")
    }

    /// 读引擎存档。读完一律置起 `refresh`：MV 读档走 `Game_Map.setup`，
    /// 而事件页是在那里重算的 —— 不置旗就是"开关对了、页没换"。
    pub fn from_ron(src: &str) -> Result<Self, CommandError> {
        let mut g: Self = ron::from_str(src).map_err(|e| CommandError::Ron(e.to_string()))?;
        g.refresh = true;
        Ok(g)
    }

    /// 写成 MV 存档里那三个字段的形状（`_data` 数组**按 1-based 直映**，下标 0 补 `null`）。
    ///
    /// 存在的理由不是"要能被 MV 读"（MV 还要 `system`/`map`/`party` 十几样），而是让
    /// 移植者能拿同一个 diff 工具比较两边的存档，也逼这张表把"变量号即下标"钉成测试。
    pub fn to_mv_json(&self) -> String {
        let mut sw: Vec<Option<serde_json::Value>> = Vec::new();
        let mut vi: Vec<Option<serde_json::Value>> = Vec::new();
        let mut ss = std::collections::BTreeMap::new();
        for id in self.store.switches() {
            resize_to(&mut sw, id);
            sw[id as usize] = Some(serde_json::Value::Bool(true));
        }
        for (id, v) in self.store.ints() {
            resize_to(&mut vi, id);
            vi[id as usize] = Some(serde_json::Value::from(v));
        }
        for (k, v) in self.store.texts() {
            if let Some(key) = k.strip_prefix(SS_PREFIX) {
                resize_dict(&mut ss, key.to_owned(), ss_truthy(v));
            }
        }
        let save = MvSave {
            switches: MvTable { _data: sw },
            variables: MvTable { _data: vi },
            self_switches: MvDict { _data: ss },
        };
        serde_json::to_string(&save).expect("三张表只含布尔与整数")
    }

    /// 读 MV 那三个字段的形状（只认这三样，其余存场字段忽略）。
    pub fn from_mv_json(src: &str) -> Result<Self, PortError> {
        let save: MvSave = serde_json::from_str(src).map_err(|e| PortError::Json(e.to_string()))?;
        let mut store = VarStore::new();
        for (i, v) in save.switches._data.iter().enumerate() {
            if i > 0 && truthy(v) {
                store.set_switch(i as u32, true);
            }
        }
        for (i, v) in save.variables._data.iter().enumerate() {
            if i > 0 {
                // MV 的 `_data` 里可能是数字，也可能是被插件写进去的字符串。
                let n = v.as_ref().and_then(|v| {
                    v.as_f64()
                        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
                });
                if let Some(n) = n {
                    store.set_int(i as u32, n.floor() as i64);
                }
            }
        }
        for (key, on) in &save.self_switches._data {
            if *on {
                store.set_text(format!("{SS_PREFIX}{key}"), "1");
            }
        }
        Ok(Self {
            store,
            switch_cap: usize::MAX,
            variable_cap: usize::MAX,
            refresh: true,
        })
    }
}

/// MV 里 `_data[id] = value` 的数组要长到下标 `id`（空洞是 `null`）。
fn resize_to(v: &mut Vec<Option<serde_json::Value>>, id: u32) {
    let need = id as usize + 1;
    if v.len() < need {
        v.resize(need, None);
    }
}

fn resize_dict(d: &mut std::collections::BTreeMap<String, bool>, key: String, on: bool) {
    if on {
        d.insert(key, true);
    } else {
        d.remove(&key);
    }
}

/// 自我开关文本的真值判定（与 `VarStore::numeric` 对文本槽的读法一致：非 0 即真）。
fn ss_truthy(s: &str) -> bool {
    let s = s.trim();
    !s.is_empty() && s != "0" && s != "false"
}

/// `!!this._data[switchId]`：MV 读开关时任何真值都算开，`null`/`0`/`""`/`false` 算关。
fn truthy(v: &Option<serde_json::Value>) -> bool {
    match v {
        None => false,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(serde_json::Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sisf_event::Slot;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    fn system() -> SystemDef {
        SystemDef {
            switches: vec![String::new(), "开门".into(), "boss已死".into()],
            variables: vec![String::new(), "线索".into()],
            ..SystemDef::default()
        }
    }

    /// 验收（T11）：三张表的语义与 MV 逐条对得上，含写边界与自我开关的键形状。
    #[test]
    fn the_three_tables_follow_mv_semantics() {
        let mut g = GameVars::with_system(&system());
        // 开关：0 号与越界都写不进去（`switchId > 0 && switchId < len`）。
        assert!(!g.set_switch(0, true));
        assert!(!g.set_switch(3, true), "len 是 3 ⇒ 3 号出界");
        assert!(g.set_switch(2, true));
        assert!(g.switch_value(2) && !g.switch_value(1));
        // 变量：同上，且读空洞是 0。
        assert!(!g.set_value(2, 7));
        assert!(g.set_value(1, -3));
        assert_eq!(g.value(1), -3);
        assert_eq!(g.value(99), 0);
        // 自我开关的键就是 `"1,3,A"`（MV 靠数组转字符串），且假值等于删键。
        assert_eq!(GameVars::ss_key(1, 3, "A"), "ss:1,3,A");
        g.set_self_switch(1, 3, "A", true);
        assert!(g.self_switch(1, 3, "A") && !g.self_switch(1, 3, "B"));
        g.set_self_switch(1, 3, "A", false);
        assert!(!g.self_switch(1, 3, "A"));
        // 规范编码必须是 VM 写得出的十进制文本：虚拟机走 `Slot::Var` 只会写 `"1"`/`"0"`，
        // 若这里存成 `"true"`，VM 的 `Cond::Cmp` 数值读会得到 0 ⇒ 同一件事两种真相。
        let slot = Slot::Var(GameVars::ss_key(1, 3, "B"));
        g.store_mut().set_numeric(&slot, 1);
        assert!(g.self_switch(1, 3, "B"));
        g.store_mut().set_numeric(&slot, 0);
        assert!(!g.self_switch(1, 3, "B"), "VM 写的 \"0\" 必须读成假");
        // `onChange ⇒ requestRefresh`：每次写都置旗，取走才清。
        assert!(g.refresh());
        assert!(g.take_refresh());
        assert!(!g.refresh());
    }

    /// 没绑定名字表时不设界 —— 缺 `System.json` 不该把写操作静默吞掉。
    #[test]
    fn unbound_tables_accept_every_id() {
        let mut g = GameVars::new();
        assert!(g.set_switch(500, true) && g.set_value(500, 1));
        assert!(g.switch_value(500) && g.value(500) == 1);
    }

    /// 验收（T11）：存档往返之后事件页条件仍然成立（读档必须置起 refresh）。
    #[test]
    fn a_save_round_trip_keeps_page_conditions_true() {
        use crate::map::PageCondition;
        let mut g = GameVars::with_system(&system());
        g.set_switch(2, true);
        g.set_value(1, 5);
        g.set_self_switch(1, 3, "A", true);
        g.take_refresh();

        let back = GameVars::from_ron(&g.to_ron()).expect("读档");
        assert_eq!(back, g, "往返必须逐位相同（含没写过的洞）");
        assert!(back.refresh(), "读档后必须让场景重算事件页");

        // 这一页要三个条件同时成立 —— 读档前它成立，读档后也必须成立。
        let c = PageCondition {
            switch2_id: 2,
            switch2_valid: true,
            variable_id: 1,
            variable_valid: true,
            variable_value: 5,
            self_switch_ch: "A".into(),
            self_switch_valid: true,
            ..PageCondition::default()
        };
        assert!(c.meets(&g, 1, 3));
        assert!(c.meets(&back, 1, 3), "读档后事件页状态必须一致");
    }

    /// MV 存档形状：`_data` 数组按 1-based 直映（下标 0 是空洞），自我开关是逗号键字典。
    #[test]
    fn mv_json_is_one_based_and_round_trips() {
        let mut g = GameVars::seeded(7);
        g.set_switch(2, true);
        g.set_value(3, 42);
        g.set_self_switch(1, 3, "A", true);
        let json = g.to_mv_json();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["switches"]["_data"][0], serde_json::Value::Null);
        assert_eq!(v["switches"]["_data"][2], true);
        assert_eq!(v["variables"]["_data"][3], 42);
        assert_eq!(v["selfSwitches"]["_data"]["1,3,A"], true);
        // 往返：引擎侧的读法不受影响。
        let back = GameVars::from_mv_json(&json).expect("读 MV 存档");
        assert!(back.switch_value(2) && back.value(3) == 42 && back.self_switch(1, 3, "A"));
        // 虚拟机直接写 `VarStore` 时不查名字表（模块文档那条"边界只拦宿主"）。
        let mut bound = GameVars::with_system(&system());
        bound.store_mut().set_int(999, 1);
        assert_eq!(bound.value(999), 1);
        // 读档置旗，写 store 也置旗。
        assert!(bound.take_refresh());
    }
}
