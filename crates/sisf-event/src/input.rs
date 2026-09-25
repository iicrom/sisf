//! 命名动作（[`ActionMap`]）与它的实时状态（[`InputState`]）。
//!
//! 分工：`sisf-app` 只负责把设备输入翻译成裸 [`EngineEvent`]，**这里**负责回答
//! "玩家刚按了'决定'吗"。游戏侧因此不必知道方向键是 `ArrowUp`、`KeyW` 还是手柄的
//! `LeftStickY-` —— 那是 [`ActionMap`] 的内容，可以从 RON 载入（T11 的 `System.json`
//! 键位表也落到同一张表上）。
//!
//! 三条边沿语义（[`ActionEdge`]）：
//! - `Press`：松开→按下那一次，以及连发（[`Autofire`]）放大出来的每一次重触。
//! - `Hold`：**非** one-shot 的动作在每个逻辑拍报一条 —— "按着就一直算按着"就是 held 语义。
//!   它按拍而不是按帧报，所以渲染帧率不影响逻辑手感（T8 的固定步长）。
//! - `Release`：按下→松开。
//!
//! one-shot（`ActionDef::one_shot`）用于"决定/取消"这类动作：菜单里按着 Enter 不该把对话框
//! 一口气跳三行，所以它不报 `Hold`，并且 [`InputState::take_press`] 一次消费。
//!
//! 本模块不碰设备、不碰 GPU、不依赖墙钟：喂事件 + 数拍子就是全部输入，
//! 所以下列单测在 native/wasm32/android 四端都跑得起来（T9 的整段验收判据）。

use serde::{Deserialize, Serialize};

use crate::bus::{ActionEdge, EngineBus, EngineEvent, KeyKind, PointerButton, PointerKind};
/// 一个动作可接受的触发源数上限：按住状态是一个位掩码。
const MAX_TRIGGERS: usize = u64::BITS as usize;

/// 一个动作的触发源。字符串词汇表由 `sisf-app` 的归一化层定义：
/// - [`Trigger::Key`]：winit 的 `PhysicalKey` 名（`"ArrowLeft"`、`"KeyW"`、`"ShiftLeft"`）。
/// - [`Trigger::Pad`]：手柄码 —— 按钮用 gilrs 的变体名（`"South"`、`"DPadUp"`），
///   摇杆用 `"<轴名><符号>"`（`"LeftStickX+"`），过死区后合成一条按下/松开。
/// - [`Trigger::Pointer`]：鼠标/触摸的某个键。
///
/// 没有"任意键"这一档：那是对话层的需求（T10 的"按任意键继续"直接读总线里的裸
/// [`EngineEvent::Key`]），塞进键位表反而会让"哪个源还按着"这笔账多出一个特例。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    Key(String),
    Pad(String),
    Pointer(PointerButton),
}

impl Trigger {
    pub fn key(code: impl Into<String>) -> Self {
        Self::Key(code.into())
    }

    pub fn pad(code: impl Into<String>) -> Self {
        Self::Pad(code.into())
    }

    /// 该触发源此刻是否被这一条裸事件按下去 / 松开来。
    ///
    /// 返回 `None` = 这条事件与它无关（包括操作系统的连发 `Repeat`：按住状态由 `InputState`
    /// 自己维护，Repeat 不该被当成一次新的按下）。
    fn edge_of(&self, event: &EngineEvent) -> Option<bool> {
        match (self, event) {
            (Self::Key(want), EngineEvent::Key { code, kind, .. }) if code == want => {
                kind.down_edge()
            }
            // 手柄键在归一化层就是一条 `Key`，只是键名来自手柄词汇表 ⇒ 匹配逻辑同一条。
            (Self::Pad(want), EngineEvent::Key { code, kind, .. }) if code == want => {
                kind.down_edge()
            }
            (
                Self::Pointer(want),
                EngineEvent::Pointer {
                    button, kind: pk, ..
                },
            ) if button == want => match pk {
                PointerKind::Press => Some(true),
                PointerKind::Release => Some(false),
                _ => None,
            },
            _ => None,
        }
    }
}

impl KeyKind {
    /// 按下 → `Some(true)`，松开 → `Some(false)`，连发 → `None`。
    fn down_edge(self) -> Option<bool> {
        match self {
            Self::Press => Some(true),
            Self::Release => Some(false),
            Self::Repeat => None,
        }
    }
}

/// 连发：按住 `delay_ticks` 拍之后，每 `interval_ticks` 拍再报一次 `Press`。
///
/// 数字沿用 RPG Maker 的菜单手感（先等 20 拍，再每 6 拍重触一次），因为它要解决的正是
/// "按着方向键连续翻选项"这件事。只数拍子不数帧。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Autofire {
    pub delay_ticks: u32,
    pub interval_ticks: u32,
}

impl Autofire {
    pub const RPG_MAKER: Autofire = Autofire {
        delay_ticks: 20,
        interval_ticks: 6,
    };
}

/// 一个命名动作的声明。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionDef {
    pub name: String,
    #[serde(default)]
    pub triggers: Vec<Trigger>,
    /// 只报 `Press`/`Release`（决定/取消这类，见模块文档）。
    #[serde(default)]
    pub one_shot: bool,
    #[serde(default)]
    pub autofire: Option<Autofire>,
}

/// 动作表：一张有序列表，按名字查（重名时先声明的那个生效，不做去重）。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActionMap {
    pub actions: Vec<ActionDef>,
}

/// 载入失败的原因。RON 的语法错误原样带出，不折叠成一句话。
#[derive(Debug, thiserror::Error)]
pub enum MapError {
    #[error("解析动作表失败: {0}")]
    Ron(String),
}

impl ActionMap {
    /// 从 RON 载入（内容就是 `Vec<ActionDef>`，见 [`ActionMap`] 上的 `serde(transparent)`）。
    pub fn from_ron(src: &str) -> Result<Self, MapError> {
        ron::from_str(src).map_err(|e| MapError::Ron(e.to_string()))
    }

    /// 写成 RON（"把当前键位存盘"用；与 [`ActionMap::from_ron`] 往返一致）。
    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .expect("动作表只含字符串与整数，序列化不会失败")
    }

    /// RPG Maker 惯例键位（决定/取消/Shift/PageUp/PageDown/菜单 + 四向移动）。
    ///
    /// 这是"没有配置文件时"的兜底，不是唯一真相：T11 会把工程 `System.json` 的
    /// `keyMapping` 翻译成同一张表盖在它上面。
    pub fn rpg_maker() -> Self {
        let dir = |name: &str, arrow: &str, wasd: &str, pad: &str, stick: &str| ActionDef {
            name: name.to_owned(),
            triggers: vec![
                Trigger::key(arrow),
                Trigger::key(wasd),
                Trigger::pad(pad),
                Trigger::pad(stick),
            ],
            one_shot: false,
            // 走格子由 `MoveRoute` 逐拍判定（held 语义），不需要连发重触。
            autofire: None,
        };
        Self {
            actions: vec![
                dir("up", "ArrowUp", "KeyW", "DPadUp", "LeftStickY-"),
                dir("down", "ArrowDown", "KeyS", "DPadDown", "LeftStickY+"),
                dir("left", "ArrowLeft", "KeyA", "DPadLeft", "LeftStickX-"),
                dir("right", "ArrowRight", "KeyD", "DPadRight", "LeftStickX+"),
                ActionDef {
                    name: "ok".to_owned(),
                    triggers: vec![
                        Trigger::key("Enter"),
                        Trigger::key("NumpadEnter"),
                        Trigger::key("Space"),
                        Trigger::key("KeyZ"),
                        Trigger::pad("South"),
                    ],
                    one_shot: true,
                    // 按着 Enter 逐行跳过对话，是 RPG Maker 自己的手感。
                    autofire: Some(Autofire::RPG_MAKER),
                },
                ActionDef {
                    name: "cancel".to_owned(),
                    triggers: vec![
                        Trigger::key("Escape"),
                        Trigger::key("KeyX"),
                        Trigger::pad("East"),
                    ],
                    one_shot: true,
                    autofire: Some(Autofire::RPG_MAKER),
                },
                ActionDef {
                    name: "shift".to_owned(),
                    triggers: vec![
                        Trigger::key("ShiftLeft"),
                        Trigger::key("ShiftRight"),
                        Trigger::pad("LeftShoulder"),
                    ],
                    one_shot: false,
                    autofire: None,
                },
                ActionDef {
                    name: "pageup".to_owned(),
                    triggers: vec![
                        Trigger::key("KeyQ"),
                        Trigger::key("PageUp"),
                        Trigger::pad("LeftTrigger"),
                    ],
                    one_shot: true,
                    autofire: Some(Autofire::RPG_MAKER),
                },
                ActionDef {
                    name: "pagedown".to_owned(),
                    triggers: vec![
                        Trigger::key("KeyE"),
                        Trigger::key("PageDown"),
                        Trigger::pad("RightTrigger"),
                    ],
                    one_shot: true,
                    autofire: Some(Autofire::RPG_MAKER),
                },
                ActionDef {
                    name: "menu".to_owned(),
                    triggers: vec![
                        Trigger::key("KeyM"),
                        Trigger::key("Tab"),
                        Trigger::pad("Start"),
                    ],
                    one_shot: true,
                    autofire: None,
                },
            ],
        }
    }
}

/// 动作表 + 实时按住状态。每个动作一个位掩码（哪些触发源正被按住），
/// 所以"两个键绑同一个动作"要两个都松开才算松开 —— 不是最后一条事件说了算。
#[derive(Clone, Debug, PartialEq)]
pub struct InputState {
    map: ActionMap,
    slots: Vec<Slot>,
}

#[derive(Clone, Debug, PartialEq)]
struct Slot {
    /// 第 i 位置 1 = 该动作的第 i 个触发源当前按住。
    held: u64,
    /// 已按住的拍数（连发计时用它，故只数拍子不数帧）。
    ticks_down: u32,
    /// `Press` 是否还没被消费（[`InputState::take_press`]）。
    press_pending: bool,
}

impl Default for InputState {
    fn default() -> Self {
        Self::new(ActionMap::rpg_maker())
    }
}

impl InputState {
    pub fn new(map: ActionMap) -> Self {
        let slots = map
            .actions
            .iter()
            .map(|_| Slot {
                held: 0,
                ticks_down: 0,
                press_pending: false,
            })
            .collect();
        Self { map, slots }
    }

    pub fn map(&self) -> &ActionMap {
        &self.map
    }

    /// 换键位表（读档、T11 套用 `System.json`）。按住状态全部作废且不补发 `Release`：
    /// 旧表里的动作名可能已经不存在，补发只会让游戏读到一次假松开。
    pub fn set_map(&mut self, map: ActionMap) {
        *self = Self::new(map);
    }

    /// 喂进一条**裸**输入事件：原样进总线，再把翻译出来的动作边沿按顺序追进去
    /// （顺序 = 裸事件在前、动作边沿在后，游戏侧两条通路都读得到）。
    pub fn feed(&mut self, event: &EngineEvent, bus: &mut EngineBus) {
        bus.push(event.clone());
        for i in 0..self.map.actions.len() {
            for bit in 0..self.trigger_count(i) {
                let mask = 1u64 << bit;
                match self.map.actions[i].triggers[bit].edge_of(event) {
                    Some(true) => self.press(i, mask, bus),
                    Some(false) => self.release(i, mask, bus),
                    None => {}
                }
            }
        }
    }

    /// 每个逻辑拍调用一次：报 held 边沿、推进连发。
    pub fn tick(&mut self, bus: &mut EngineBus) {
        for i in 0..self.map.actions.len() {
            if self.slots[i].held == 0 {
                self.slots[i].ticks_down = 0;
                continue;
            }
            self.slots[i].ticks_down += 1;
            let (name, one_shot, autofire) = (
                self.map.actions[i].name.clone(),
                self.map.actions[i].one_shot,
                self.map.actions[i].autofire,
            );
            if !one_shot {
                bus.push(EngineEvent::action(name.clone(), ActionEdge::Hold));
            }
            let Some(fire) = autofire else { continue };
            let held = self.slots[i].ticks_down;
            if held > fire.delay_ticks
                && fire.interval_ticks > 0
                && (held - fire.delay_ticks - 1).is_multiple_of(fire.interval_ticks)
            {
                self.slots[i].press_pending = true;
                bus.push(EngineEvent::action(name, ActionEdge::Press));
            }
        }
    }

    /// 该动作当前是否按住（任一触发源按住即为真）。
    pub fn is_down(&self, name: &str) -> bool {
        self.index_of(name).is_some_and(|i| self.slots[i].held != 0)
    }

    /// 该动作是否处于"刚按下"状态；读一次即消费。
    ///
    /// 这就是菜单里"按着 Enter 只跳一格"的依据：连 `Hold` 都不报的 one-shot 动作，
    /// 每拍轮询也只会被满足一次。
    pub fn take_press(&mut self, name: &str) -> bool {
        let Some(i) = self.index_of(name) else {
            return false;
        };
        std::mem::take(&mut self.slots[i].press_pending)
    }

    /// 松开所有按住的动作，并补发 `Release` 边沿。
    ///
    /// 引擎在失焦/挂起时调用：窗口最小化时收不到 keyup，不放掉的话游戏回来就朝一面墙磨。
    pub fn forget_held(&mut self, bus: &mut EngineBus) {
        self.release_where(|_| true, bus);
    }

    /// 只放过满足条件的触发源（拔手柄：`|t| matches!(t, Trigger::Pad(_))`，
    /// 键盘按住的方向不受牵连）。
    pub fn release_where(&mut self, matches: impl Fn(&Trigger) -> bool, bus: &mut EngineBus) {
        for i in 0..self.map.actions.len() {
            let clear = (0..self.trigger_count(i))
                .filter(|&bit| matches(&self.map.actions[i].triggers[bit]))
                .fold(0u64, |m, bit| m | (1u64 << bit));
            let was = self.slots[i].held;
            self.slots[i].held &= !clear;
            if was != 0 && self.slots[i].held == 0 {
                self.slots[i].ticks_down = 0;
                bus.push(EngineEvent::action(
                    self.map.actions[i].name.clone(),
                    ActionEdge::Release,
                ));
            }
        }
    }

    /// 一个动作实际参与掩码的触发源数（超过 [`MAX_TRIGGERS`] 的部分忽略，不静默算错）。
    fn trigger_count(&self, i: usize) -> usize {
        self.map.actions[i].triggers.len().min(MAX_TRIGGERS)
    }

    fn press(&mut self, i: usize, mask: u64, bus: &mut EngineBus) {
        if self.slots[i].held & mask != 0 {
            return; // 同一个源重复按下（丢过 Release 之后）不重复报边沿
        }
        let was_down = self.slots[i].held != 0;
        self.slots[i].held |= mask;
        if was_down {
            return;
        }
        self.slots[i].ticks_down = 0;
        self.slots[i].press_pending = true;
        bus.push(EngineEvent::action(
            self.map.actions[i].name.clone(),
            ActionEdge::Press,
        ));
    }

    fn release(&mut self, i: usize, mask: u64, bus: &mut EngineBus) {
        if self.slots[i].held & mask == 0 {
            return; // 没按过就收到松开（焦点切换后的补发）：静默，不报假边沿
        }
        self.slots[i].held &= !mask;
        if self.slots[i].held == 0 {
            self.slots[i].ticks_down = 0;
            bus.push(EngineEvent::action(
                self.map.actions[i].name.clone(),
                ActionEdge::Release,
            ));
        }
    }

    fn index_of(&self, name: &str) -> Option<usize> {
        self.map.actions.iter().position(|a| a.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sisf_sprite::Vec2;

    fn press(code: &str) -> EngineEvent {
        EngineEvent::key(code, KeyKind::Press, None)
    }

    fn release(code: &str) -> EngineEvent {
        EngineEvent::key(code, KeyKind::Release, None)
    }

    /// 只取指定动作的那些边沿，按到达顺序列出来。
    fn edges(bus: &EngineBus, name: &str) -> Vec<ActionEdge> {
        bus.peek()
            .iter()
            .filter_map(|e| match e {
                EngineEvent::Action { name: n, edge } if n == name => Some(*edge),
                _ => None,
            })
            .collect()
    }

    fn map_of(action: ActionDef) -> ActionMap {
        ActionMap {
            actions: vec![action],
        }
    }

    /// 验收点名的一条：pressed / held / released 三条边沿各自只出现在该在的位置。
    #[test]
    fn held_action_reports_press_then_one_hold_per_tick_then_release() {
        let mut input = InputState::new(map_of(ActionDef {
            name: "left".to_owned(),
            triggers: vec![Trigger::key("ArrowLeft")],
            one_shot: false,
            autofire: None,
        }));
        let mut bus = EngineBus::new();

        input.feed(&press("ArrowLeft"), &mut bus);
        assert_eq!(edges(&bus, "left"), vec![ActionEdge::Press]);
        assert!(input.is_down("left"));

        for _ in 0..3 {
            bus = EngineBus::new();
            input.tick(&mut bus);
            assert_eq!(edges(&bus, "left"), vec![ActionEdge::Hold]);
        }

        bus = EngineBus::new();
        input.feed(&release("ArrowLeft"), &mut bus);
        assert_eq!(edges(&bus, "left"), vec![ActionEdge::Release]);
        assert!(!input.is_down("left"));

        // 松开之后再拍一次：什么也不报。
        bus = EngineBus::new();
        input.tick(&mut bus);
        assert!(edges(&bus, "left").is_empty());
    }

    #[test]
    fn one_shot_action_never_reports_hold() {
        let mut input = InputState::new(map_of(ActionDef {
            name: "ok".to_owned(),
            triggers: vec![Trigger::key("Enter")],
            one_shot: true,
            autofire: None,
        }));
        let mut bus = EngineBus::new();
        input.feed(&press("Enter"), &mut bus);
        for _ in 0..5 {
            input.tick(&mut bus);
        }
        assert_eq!(
            edges(&bus, "ok"),
            vec![ActionEdge::Press],
            "one-shot 按着不放也只该有一次 Press"
        );
    }

    /// one-shot 的"按一次只跳一格"：`take_press` 消费掉那次按下。
    #[test]
    fn take_press_consumes_the_edge_once() {
        let mut input = InputState::new(map_of(ActionDef {
            name: "ok".to_owned(),
            triggers: vec![Trigger::key("Enter")],
            one_shot: true,
            autofire: None,
        }));
        let mut bus = EngineBus::new();
        input.feed(&press("Enter"), &mut bus);
        assert!(input.take_press("ok"));
        assert!(!input.take_press("ok"), "同一次按下不该被读到第二遍");
        assert!(input.is_down("ok"), "消费掉的是边沿，不是按住状态");

        input.feed(&release("Enter"), &mut bus);
        input.feed(&press("Enter"), &mut bus);
        assert!(input.take_press("ok"), "重新按下要重新可消费");
        assert!(!input.take_press("nope"), "没绑过的动作恒为假");
    }

    /// 连发：按下先报一次，之后在 `delay` 拍外每 `interval` 拍再报一次 `Press`。
    #[test]
    fn autofire_repeats_press_on_the_tick_grid() {
        let mut input = InputState::new(map_of(ActionDef {
            name: "cancel".to_owned(),
            triggers: vec![Trigger::key("Escape")],
            one_shot: true,
            autofire: Some(Autofire {
                delay_ticks: 4,
                interval_ticks: 2,
            }),
        }));
        let mut bus = EngineBus::new();
        input.feed(&press("Escape"), &mut bus);
        assert_eq!(edges(&bus, "cancel"), vec![ActionEdge::Press]);

        let mut per_tick = Vec::new();
        for _ in 1..=8 {
            bus = EngineBus::new();
            input.tick(&mut bus);
            per_tick.push(edges(&bus, "cancel").len());
        }
        // 第 5、7 拍（4 拍延迟之后每 2 拍一次）。
        assert_eq!(per_tick, vec![0, 0, 0, 0, 1, 0, 1, 0], "连发节拍不对");

        bus = EngineBus::new();
        input.feed(&release("Escape"), &mut bus);
        input.feed(&press("Escape"), &mut bus);
        assert_eq!(edges(&bus, "cancel").last(), Some(&ActionEdge::Press));
    }

    #[test]
    fn autofire_stops_the_moment_the_key_is_released() {
        let mut input = InputState::new(map_of(ActionDef {
            name: "ok".to_owned(),
            triggers: vec![Trigger::key("Enter")],
            one_shot: true,
            autofire: Some(Autofire {
                delay_ticks: 1,
                interval_ticks: 1,
            }),
        }));
        let mut bus = EngineBus::new();
        input.feed(&press("Enter"), &mut bus);
        assert_eq!(edges(&bus, "ok"), vec![ActionEdge::Press]);
        // 延迟是"按住满 1 拍之后再开始"，所以第一拍只有按下本身。
        input.tick(&mut bus);
        assert_eq!(edges(&bus, "ok"), vec![ActionEdge::Press]);
        input.tick(&mut bus);
        assert_eq!(
            edges(&bus, "ok"),
            vec![ActionEdge::Press, ActionEdge::Press],
            "第 2 拍该放出第一次连发"
        );
        bus = EngineBus::new();
        input.feed(&release("Enter"), &mut bus);
        input.tick(&mut bus);
        input.tick(&mut bus);
        assert_eq!(
            edges(&bus, "ok"),
            vec![ActionEdge::Release],
            "松开之后还报 Press 就是把连发当成了按键"
        );
    }

    /// 两个键绑同一个动作：必须两个都松开才算松开（听最后一条事件会误报）。
    #[test]
    fn two_triggers_on_one_action_need_both_releases() {
        let mut input = InputState::new(map_of(ActionDef {
            name: "up".to_owned(),
            triggers: vec![Trigger::key("ArrowUp"), Trigger::key("KeyW")],
            one_shot: false,
            autofire: None,
        }));
        let mut bus = EngineBus::new();
        input.feed(&press("ArrowUp"), &mut bus);
        input.feed(&press("KeyW"), &mut bus);
        assert_eq!(
            edges(&bus, "up"),
            vec![ActionEdge::Press],
            "第二个源不该再报一次按下"
        );

        bus = EngineBus::new();
        input.feed(&release("ArrowUp"), &mut bus);
        assert!(edges(&bus, "up").is_empty());
        assert!(input.is_down("up"), "W 还按着");

        bus = EngineBus::new();
        input.feed(&release("KeyW"), &mut bus);
        assert_eq!(edges(&bus, "up"), vec![ActionEdge::Release]);
    }

    #[test]
    fn a_repeated_key_is_not_a_new_press() {
        let mut input = InputState::new(map_of(ActionDef {
            name: "left".to_owned(),
            triggers: vec![Trigger::key("ArrowLeft")],
            one_shot: false,
            autofire: None,
        }));
        let mut bus = EngineBus::new();
        input.feed(&press("ArrowLeft"), &mut bus);
        for _ in 0..4 {
            input.feed(
                &EngineEvent::key("ArrowLeft", KeyKind::Repeat, None),
                &mut bus,
            );
        }
        assert_eq!(edges(&bus, "left"), vec![ActionEdge::Press]);
        input.tick(&mut bus);
        assert_eq!(
            edges(&bus, "left").last(),
            Some(&ActionEdge::Hold),
            "连发不该改变按住状态"
        );
    }

    #[test]
    fn unbound_input_still_reaches_the_bus() {
        let mut input = InputState::new(map_of(ActionDef {
            name: "ok".to_owned(),
            triggers: vec![Trigger::key("Enter")],
            one_shot: true,
            autofire: None,
        }));
        let mut bus = EngineBus::new();
        input.feed(&press("F13"), &mut bus);
        assert!(edges(&bus, "ok").is_empty());
        assert_eq!(
            bus.peek().len(),
            1,
            "裸事件仍要原样进总线，游戏可以自己去读"
        );
        assert!(!input.is_down("ok"));
    }

    /// 手柄键与键盘键在同一个动作下收敛成同一条边沿。
    #[test]
    fn pad_and_keyboard_triggers_converge_on_one_action() {
        let mut input = InputState::new(ActionMap::rpg_maker());
        let mut bus = EngineBus::new();
        input.feed(&press("South"), &mut bus);
        assert_eq!(edges(&bus, "ok"), vec![ActionEdge::Press]);
        input.feed(&release("South"), &mut bus);
        input.feed(&press("Enter"), &mut bus);
        assert_eq!(edges(&bus, "ok").last(), Some(&ActionEdge::Press));
    }

    /// 摇杆过死区后合成出来那条码，和 DPad 一起落在同一个方向动作上。
    #[test]
    fn stick_codes_are_just_another_trigger() {
        let mut input = InputState::new(ActionMap::rpg_maker());
        let mut bus = EngineBus::new();
        input.feed(&press("DPadRight"), &mut bus);
        input.feed(&press("LeftStickX+"), &mut bus);
        assert_eq!(
            edges(&bus, "right"),
            vec![ActionEdge::Press],
            "两个源指向同一个方向，只该报一次按下"
        );
        assert!(input.is_down("right"));
        bus = EngineBus::new();
        input.feed(&release("LeftStickX+"), &mut bus);
        assert!(
            edges(&bus, "right").is_empty(),
            "DPad 还按着 ⇒ 松摇杆不该报松开"
        );
        assert!(input.is_down("right"));
        bus = EngineBus::new();
        input.feed(&release("DPadRight"), &mut bus);
        assert_eq!(edges(&bus, "right"), vec![ActionEdge::Release]);
    }

    #[test]
    fn pointer_buttons_can_trigger_an_action() {
        let mut input = InputState::new(map_of(ActionDef {
            name: "ok".to_owned(),
            triggers: vec![Trigger::Pointer(PointerButton::Left)],
            one_shot: true,
            autofire: None,
        }));
        let mut bus = EngineBus::new();
        input.feed(
            &EngineEvent::Pointer {
                kind: PointerKind::Press,
                pos: Vec2::new(12.0, 34.0),
                button: PointerButton::Left,
                delta: Vec2::ZERO,
            },
            &mut bus,
        );
        assert_eq!(edges(&bus, "ok"), vec![ActionEdge::Press]);
        input.feed(
            &EngineEvent::Pointer {
                kind: PointerKind::Moved,
                pos: Vec2::new(20.0, 40.0),
                button: PointerButton::None,
                delta: Vec2::new(8.0, 6.0),
            },
            &mut bus,
        );
        assert_eq!(
            edges(&bus, "ok"),
            vec![ActionEdge::Press],
            "移动不该被当成松开"
        );
        input.feed(
            &EngineEvent::Pointer {
                kind: PointerKind::Release,
                pos: Vec2::new(20.0, 40.0),
                button: PointerButton::Left,
                delta: Vec2::ZERO,
            },
            &mut bus,
        );
        assert_eq!(edges(&bus, "ok").last(), Some(&ActionEdge::Release));
    }

    /// 失焦/挂起：补发 Release，游戏侧不会在回来时读到"还在按着"。
    #[test]
    fn forget_held_reports_a_release_for_every_held_action() {
        let mut input = InputState::new(ActionMap::rpg_maker());
        let mut bus = EngineBus::new();
        input.feed(&press("ArrowLeft"), &mut bus);
        input.feed(&press("Enter"), &mut bus);
        bus = EngineBus::new();
        input.forget_held(&mut bus);
        assert_eq!(edges(&bus, "left"), vec![ActionEdge::Release]);
        assert_eq!(edges(&bus, "ok"), vec![ActionEdge::Release]);
        assert!(!input.is_down("left") && !input.is_down("ok"));
        // 失焦放键不吞掉未消费的 `Press`：窗口边缘的一次快速点按（按下→立刻失焦）
        // 仍要生效，否则"决定"会在丢焦点的那一刻凭空消失。
        assert!(input.take_press("ok"), "松开不吞掉未消费的按下");
        assert!(!input.take_press("ok"), "消费一次即失效");
    }

    /// 拔手柄不能顺手把键盘按住的方向也放掉。
    #[test]
    fn release_where_only_touches_the_matching_device() {
        let mut input = InputState::new(ActionMap::rpg_maker());
        let mut bus = EngineBus::new();
        input.feed(&press("ArrowLeft"), &mut bus);
        input.feed(&press("DPadRight"), &mut bus);
        bus = EngineBus::new();
        input.release_where(|t| matches!(t, Trigger::Pad(_)), &mut bus);
        assert_eq!(edges(&bus, "right"), vec![ActionEdge::Release]);
        assert!(
            edges(&bus, "left").is_empty(),
            "手柄断了不该牵连键盘按住的方向"
        );
        assert!(input.is_down("left"));
    }

    #[test]
    fn set_map_drops_held_state_without_emitting_anything() {
        let mut input = InputState::new(ActionMap::rpg_maker());
        let mut bus = EngineBus::new();
        input.feed(&press("Enter"), &mut bus);
        input.set_map(ActionMap::default());
        bus = EngineBus::new();
        input.tick(&mut bus);
        assert!(bus.drain().is_empty(), "换表不补发旧表的边沿");
        assert!(!input.is_down("ok"), "旧表里的名字在新表里不存在");
    }

    /// 验收点名的另一条：动作表可以从 RON 载入，且与 `to_ron` 往返一致。
    #[test]
    fn ron_roundtrip_rebuilds_the_same_map() {
        let src = r#"
[
    (
        name: "ok",
        triggers: [key("Enter"), key("Space"), pad("South")],
        one_shot: true,
        autofire: Some((delay_ticks: 20, interval_ticks: 6)),
    ),
    (
        name: "run",
        triggers: [key("ShiftLeft")],
    ),
]
"#;
        let map = ActionMap::from_ron(src).expect("这段 RON 该能载入");
        assert_eq!(map.actions.len(), 2);
        assert_eq!(map.actions[0].triggers.len(), 3);
        assert_eq!(
            map.actions[0].autofire,
            Some(Autofire {
                delay_ticks: 20,
                interval_ticks: 6
            })
        );
        assert_eq!(
            map.actions[1],
            ActionDef {
                name: "run".to_owned(),
                triggers: vec![Trigger::key("ShiftLeft")],
                one_shot: false,
                autofire: None,
            },
            "省略的字段该走 default"
        );

        // 反向：Rust 侧构造的表（含指针触发源）写得出去也读得回来。
        let mut with_pointer = map.clone();
        with_pointer.actions[0]
            .triggers
            .push(Trigger::Pointer(PointerButton::Left));
        with_pointer.actions[1]
            .triggers
            .push(Trigger::Pointer(PointerButton::Other(5)));
        assert_eq!(
            ActionMap::from_ron(&with_pointer.to_ron()).unwrap(),
            with_pointer,
            "指针触发源在 RON 里没能往返"
        );
        assert_eq!(ActionMap::from_ron(&map.to_ron()).unwrap(), map);
    }

    #[test]
    fn a_loaded_map_actually_resolves_its_names() {
        let map = ActionMap::from_ron(r#"[ ( name: "jump", triggers: [key("Space")] ) ]"#).unwrap();
        let mut input = InputState::new(map);
        let mut bus = EngineBus::new();
        input.feed(&press("Space"), &mut bus);
        assert_eq!(edges(&bus, "jump"), vec![ActionEdge::Press]);
    }

    #[test]
    fn bad_ron_is_an_error_not_a_panic() {
        let err = ActionMap::from_ron("[ ( name: 12 ) ]").unwrap_err();
        assert!(matches!(err, MapError::Ron(_)), "语法错误要报出来: {err}");
    }

    /// RPG Maker 那套默认键位得真的能用（决定/取消/Shift/PageUp 是清单点名的四个）。
    #[test]
    fn rpg_maker_defaults_cover_the_named_buttons() {
        let map = ActionMap::rpg_maker();
        let names: Vec<&str> = map.actions.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "up", "down", "left", "right", "ok", "cancel", "shift", "pageup", "pagedown",
                "menu"
            ]
        );
        let mut input = InputState::new(map);
        let mut bus = EngineBus::new();
        for code in [
            "Enter",
            "Escape",
            "ShiftLeft",
            "KeyQ",
            "KeyE",
            "ArrowUp",
            "ArrowLeft",
        ] {
            input.feed(&press(code), &mut bus);
            input.feed(&release(code), &mut bus);
        }
        for name in ["ok", "cancel", "shift", "pageup", "pagedown", "up", "left"] {
            let got = edges(&bus, name);
            assert_eq!(
                (got.first(), got.last()),
                (Some(&ActionEdge::Press), Some(&ActionEdge::Release)),
                "默认键位里 {name} 没走通: {got:?}"
            );
        }
        assert!(!input.is_down("up"), "松开之后不该还剩按住状态");
    }

    /// 空表也要能干活：`InputState` 恒等，不 panic。
    #[test]
    fn an_empty_map_reports_nothing_at_all() {
        let mut input = InputState::new(ActionMap::default());
        let mut bus = EngineBus::new();
        input.feed(&press("Enter"), &mut bus);
        input.tick(&mut bus);
        input.forget_held(&mut bus);
        assert_eq!(bus.peek().len(), 1, "只剩那条裸按键");
        assert!(!input.is_down("ok") && !input.take_press("ok"));
    }
}
