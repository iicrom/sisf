//! 手柄 → 裸 [`EngineEvent`]（T9，只在桌面三端编译）。
//!
//! ## 平台矩阵
//!
//! | 目标 | 手柄 | 原因 |
//! | --- | --- | --- |
//! | Windows / Linux / macOS | ✅ gilrs 0.11 | `native` feature 下才声明依赖 |
//! | `wasm32` | ❌ | 浏览器要 Gamepad API 得每帧轮询 `navigator.getGamepads()`，那是 JS 侧的活；本阶段不接 |
//! | Android | ❌ | gilrs 0.11 没有 `android-activity` 后端，依赖连声明都不给 |
//!
//! 也就是说 [`Trigger::Pad`](sisf_event::Trigger::Pad) 在后两端永远匹配不上事件但不报错 ——
//! 键位表照常工作，只是少一个设备。这是"平台不支持的能力靠 feature-gate + 文档，而不是
//! 散落的空实现"那条约定的落点（`docs/input.md` 有同一张表）。
//!
//! ## 死区与边沿
//!
//! gilrs 的默认过滤器已经做了抖动抑制与死区，并把左摇杆额外折算成一组十字键按下
//! （`DPadUp`…）。这里再做一层**摇杆方向码**（`"LeftStickX+"` 这类，见
//! [`crate::normalize::pad`]），阈值刻意取 gilrs 折算十字键用的同一对数字
//! （按 0.75 / 松 0.65）：两套码来自同一根轴，阈值不一致就会出现"十字键已松开、摇杆码还
//! 按着"那种半格重叠的死区，而 [`ActionMap`](sisf_event::ActionMap) 的位掩码要求两个源都
//! 松开才算松开。
//!
//! 多只手柄共用一套码（与 `Trigger::Pad` 的词汇表一致）：拔掉任一只手柄会放掉全部手柄键，
//! 下一拍还按着的键由对应手柄的事件重新按下。T9 不做按序号区分。

use std::collections::HashMap;

use gilrs::{Button, EventType, Gilrs};
use sisf_event::{EngineBus, EngineEvent, InputState, KeyKind, Trigger};

use crate::normalize::pad;

/// 与 gilrs 十字键折算同源的一对阈值（带迟滞，免得停在临界点上反复抖）。
const PRESS: f32 = 0.75;
const RELEASE: f32 = 0.65;
/// 模拟扳机按下判定（`ButtonChanged` 的值域是 0.0..=1.0）。
const TRIGGER_PRESS: f32 = 0.5;

/// 摇杆"过阈值 = 按下、回中 = 松开"的边沿机器：轴名 → 当前已报按下的方向码。
///
/// 单独抽出来是因为它决定了手感（死区太大会走不动、太小会自己飘），而这件事跟 gilrs 怎么
/// 把事件递给我们无关 —— 于是它可以在没有手柄的机器上被单测覆盖。
#[derive(Default)]
pub(crate) struct StickEdges {
    held: HashMap<&'static str, Option<String>>,
}

impl StickEdges {
    /// 一根轴的读数变化。产出的边沿追加进 `out`（先松旧的、再按新的，顺序即因果顺序）。
    pub(crate) fn changed(&mut self, axis: &'static str, value: f32, out: &mut Vec<EngineEvent>) {
        let prev = self.held.entry(axis).or_insert(None);
        let direction = if value >= PRESS {
            Some('+')
        } else if value <= -PRESS {
            Some('-')
        } else if value.abs() >= RELEASE {
            // 落在迟滞带里：保持上一步的状态，不来回跳。
            prev.as_ref().and_then(|code| code.chars().last())
        } else {
            None
        };
        let next = direction.map(|sign| pad::stick_code(axis, sign));
        if *prev == next {
            return;
        }
        if let Some(old) = prev.take() {
            out.push(EngineEvent::key(old, KeyKind::Release, None));
        }
        if let Some(new) = &next {
            out.push(EngineEvent::key(new.clone(), KeyKind::Press, None));
        }
        *prev = next;
    }

    /// 拔掉手柄：把还按着的方向码全松开（gilrs 不会替我们补发回中事件），并交回键名。
    pub(crate) fn forget_all(&mut self, out: &mut Vec<EngineEvent>) {
        for (_, code) in self.held.drain() {
            if let Some(code) = code {
                out.push(EngineEvent::key(code, KeyKind::Release, None));
            }
        }
    }
}

/// 桌面手柄适配器。`Gilrs::new()` 在没有手柄的机器上也会成功（只是没有事件）；初始化失败
/// （无权限/无后端）不是致命错误 —— 整个引擎不该因为插了个怪手柄而起不来。
pub(crate) struct Gamepads {
    gilrs: Gilrs,
    sticks: StickEdges,
}

impl Gamepads {
    /// `None` = 这台机器上手柄子系统起不来（键盘照常工作）。
    pub(crate) fn new() -> Option<Self> {
        Gilrs::new().ok().map(|gilrs| Self {
            gilrs,
            sticks: StickEdges::default(),
        })
    }

    /// 每帧调一次：取空 gilrs 队列，逐条翻译成裸事件喂进 `input`（与键盘/指针同一条路）。
    pub(crate) fn poll(&mut self, input: &mut InputState, bus: &mut EngineBus) {
        let mut out = Vec::new();
        while let Some(event) = self.gilrs.next_event() {
            match event.event {
                EventType::ButtonPressed(button, _) => button_edge(&mut out, button, true),
                EventType::ButtonReleased(button, _) => button_edge(&mut out, button, false),
                // 十字键按住时操作系统的连发：按下状态由 `InputState` 记账，不当新按下。
                EventType::ButtonRepeated(..) => {}
                // 扳机是模拟量（0.0..=1.0），过 [`TRIGGER_PRESS`] 才算"这个键按下了"。
                EventType::ButtonChanged(button, value, _) => {
                    button_edge(&mut out, button, value >= TRIGGER_PRESS)
                }
                EventType::AxisChanged(axis, value, _) => {
                    if let Some(name) = pad::stick_axis_name(axis) {
                        self.sticks.changed(name, value, &mut out);
                    }
                }
                EventType::Disconnected | EventType::Dropped => {
                    self.sticks.forget_all(&mut out);
                    // 按钮的松开 gilrs 也不补发，只能整批放掉（见模块文档的多手柄说明）。
                    input.release_where(is_pad_trigger, bus);
                }
                // 连接与力反馈完成不影响"现在按着什么"，边沿机器按后续事件记账。
                EventType::Connected | EventType::ForceFeedbackEffectCompleted => {}
                _ => {}
            }
        }
        for event in out {
            input.feed(&event, bus);
        }
    }
}

fn is_pad_trigger(trigger: &Trigger) -> bool {
    matches!(trigger, Trigger::Pad(_))
}

fn button_edge(out: &mut Vec<EngineEvent>, button: Button, pressed: bool) {
    out.push(EngineEvent::key(
        pad::button_code(button),
        if pressed {
            KeyKind::Press
        } else {
            KeyKind::Release
        },
        None,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    // T12：`cargo xtask wasm-test` 在 wasm 上跑这一组（见 docs/platforms.md）。
    use sisf_event::{ActionEdge, ActionMap};
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    fn codes(events: &[EngineEvent]) -> Vec<(&str, KeyKind)> {
        events
            .iter()
            .map(|e| match e {
                EngineEvent::Key { code, kind, .. } => (code.as_str(), *kind),
                other => panic!("不是裸按键事件: {other:?}"),
            })
            .collect()
    }

    /// 验收点名的死区：过阈值才按下，回中带里不许来回跳。
    #[test]
    fn a_stick_edge_needs_the_press_threshold_and_the_release_band_clears_it() {
        let mut sticks = StickEdges::default();
        let mut out = Vec::new();
        sticks.changed("LeftStickX", 0.3, &mut out);
        assert!(out.is_empty(), "没过阈值不算按");
        sticks.changed("LeftStickX", 0.8, &mut out);
        assert_eq!(codes(&out), vec![("LeftStickX+", KeyKind::Press)]);
        out.clear();
        // 迟滞带（0.65..0.75）：还认上一步的状态。
        sticks.changed("LeftStickX", 0.7, &mut out);
        assert!(out.is_empty(), "迟滞带里跳一下就是抖动");
        sticks.changed("LeftStickX", -0.7, &mut out);
        assert!(out.is_empty(), "反方向也还在迟滞带里，不该换向");
        out.clear();
        sticks.changed("LeftStickX", 0.0, &mut out);
        assert_eq!(codes(&out), vec![("LeftStickX+", KeyKind::Release)]);
    }

    /// 同一根轴上换向 = 先松旧后按新（位掩码那边要的就是这个顺序）。
    #[test]
    fn crossing_to_the_other_side_releases_first_then_presses() {
        let mut sticks = StickEdges::default();
        let mut out = Vec::new();
        sticks.changed("LeftStickY", -1.0, &mut out);
        sticks.changed("LeftStickY", 1.0, &mut out);
        assert_eq!(
            codes(&out),
            vec![
                ("LeftStickY-", KeyKind::Press),
                ("LeftStickY-", KeyKind::Release),
                ("LeftStickY+", KeyKind::Press),
            ]
        );
    }

    /// 拔手柄不许留下"永远按着"的方向。
    #[test]
    fn unplugging_forgets_every_held_stick_direction() {
        let mut sticks = StickEdges::default();
        let mut out = Vec::new();
        sticks.changed("LeftStickX", 1.0, &mut out);
        sticks.changed("RightStickY", 1.0, &mut out);
        out.clear();
        sticks.forget_all(&mut out);
        let mut got: Vec<String> = codes(&out)
            .into_iter()
            .map(|(code, kind)| format!("{kind:?} {code}"))
            .collect();
        got.sort();
        assert_eq!(
            got,
            vec!["Release LeftStickX+", "Release RightStickY+"],
            "两只手柄的方向码都要放掉"
        );
        out.clear();
        sticks.forget_all(&mut out);
        assert!(out.is_empty(), "已经放掉的不该再放一次");
    }

    /// 手柄码最终落进 `ActionMap`：`Trigger::Pad` 与键盘码在同一个动作下收敛。
    ///
    /// 顺带钉住符号约定 —— 摇杆轴的正方向在屏幕坐标里是"下/右"，所以 `-1.0` 才是 `left`、
    /// `+1.0` 是 `right`。这一格反了的话，用手柄走路会朝镜像方向。
    #[test]
    fn stick_codes_resolve_through_the_same_action_map_as_the_keyboard() {
        let mut input = InputState::new(ActionMap::rpg_maker());
        let mut bus = EngineBus::new();
        let mut sticks = StickEdges::default();
        let mut out = Vec::new();
        sticks.changed("LeftStickX", -1.0, &mut out);
        sticks.changed("LeftStickY", -1.0, &mut out);
        for e in out {
            input.feed(&e, &mut bus);
        }
        assert!(input.is_down("left"), "摇杆左推要能动 `left`");
        assert!(input.is_down("up"), "摇杆上推要能动 `up`");
        assert!(!input.is_down("right"), "同一根轴不许两头同时按着");
        assert!(
            bus.peek()
                .contains(&EngineEvent::action("left", ActionEdge::Press))
        );
        assert!(
            bus.peek()
                .contains(&EngineEvent::action("up", ActionEdge::Press))
        );
    }
}
