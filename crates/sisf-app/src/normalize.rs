//! 设备事件 → [`EngineEvent`] 的**唯一**一处翻译（T9）。
//!
//! 为什么要单独立一个模块：三端（native / wasm / android）共用 `Runner`，如果翻译规则散在
//! `window_event` 的 `match` 里，那么"浏览器里的鼠标坐标和桌面差一格"这类问题就没有一个地方
//! 能单测。这里每个函数都是纯函数（进去 winit 的事件片段，出来一条裸 [`EngineEvent`]），
//! 所以在四端都跑得起来；`Runner` 只负责挑事件、调这些函数、再喂给
//! [`InputState`](sisf_event::InputState)。
//!
//! 三条约定：
//! - 键盘 `code` 取**物理键名**（`"ArrowLeft"`），布局无关 ⇒ 同一张 [`ActionMap`](sisf_event::ActionMap)
//!   在法语键盘上也不错位；布局相关的字符走 `ch`（文本输入用）。
//! - 指针 `pos`/`delta` 一律已经过 [`FrameCtx::map_pointer`]，是**虚拟像素** ⇒ 游戏侧的命中
//!   测试可以直接和 sprite 坐标比，不必知道窗口多大、是不是 letterbox。
//! - 手柄也翻译成 [`EngineEvent::Key`]，键名见 [`pad`]：按钮用 gilrs 的变体名，摇杆用
//!   `"<轴名><符号>"`。这样 `Trigger::Pad` 与 `Trigger::Key` 走同一条匹配路径。

use sisf_event::{EngineEvent, KeyKind, PointerButton, PointerKind};
use sisf_sprite::Vec2;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, TouchPhase};
use winit::keyboard::PhysicalKey;

use crate::frame::FrameCtx;

/// winit 物理键 → 与键盘布局无关的键名（`"ArrowLeft"`、`"KeyW"`）。
///
/// 借 `KeyCode` 的 `Debug` 名当词汇表，而不是自己抄一张 200 项的匹配表：变体名本就是
/// `ActionMap` 要绑的东西。未识别的扫描码带上原编号，不静默成空串。
pub(crate) fn key_name(key: &PhysicalKey) -> String {
    match key {
        PhysicalKey::Code(code) => format!("{code:?}"),
        PhysicalKey::Unidentified(native) => format!("Native({native:?})"),
    }
}

/// winit 的 `state` + `repeat` → 引擎边沿。
///
/// 单列成函数是因为 `KeyEvent` 的字段是私有的（测试造不出来），而"连发不算按下"这件事
/// 恰恰是 `ActionMap` 手感的地基，必须能被钉住。
pub(crate) fn key_kind(state: ElementState, repeat: bool) -> KeyKind {
    match state {
        ElementState::Pressed if repeat => KeyKind::Repeat,
        ElementState::Pressed => KeyKind::Press,
        ElementState::Released => KeyKind::Release,
    }
}

/// 键盘事件归一化。`text` 只在按下（含连发）时有值，松开必为 `None`。
pub(crate) fn key_event(event: &KeyEvent) -> EngineEvent {
    EngineEvent::key(
        key_name(&event.physical_key),
        key_kind(event.state, event.repeat),
        event.text.as_ref().and_then(|t| t.chars().next()),
    )
}

/// winit 鼠标键 → 引擎指针键。`Other` 保留原编号，侧键不必挤成"无"。
pub(crate) fn mouse_button(button: &MouseButton) -> PointerButton {
    match button {
        MouseButton::Left => PointerButton::Left,
        MouseButton::Right => PointerButton::Right,
        MouseButton::Middle => PointerButton::Middle,
        // 后退/前进是 winit 单列出来的两个侧键，而它们在 Win/X11 的原始编号就是 3/4 ⇒
        // 折回编号，动作表里绑一个源就能覆盖两种报告方式。
        MouseButton::Back => PointerButton::Other(3),
        MouseButton::Forward => PointerButton::Other(4),
        MouseButton::Other(raw) => PointerButton::Other(*raw),
    }
}

/// 光标移动。`physical` 是窗口物理像素，`prev` 是上一次的位置（首次为 `None` ⇒ 位移记 0，
/// 不然光标一进来就凭空一条几百像素的 `delta`）。
pub(crate) fn cursor_moved(
    frame: &FrameCtx,
    physical: (f64, f64),
    prev: Option<(f64, f64)>,
) -> EngineEvent {
    let (dx, dy) = match prev {
        Some((px, py)) => (physical.0 - px, physical.1 - py),
        None => (0.0, 0.0),
    };
    pointer(
        frame,
        PointerKind::Moved,
        physical,
        PointerButton::None,
        (dx, dy),
    )
}

/// 鼠标按下/松开。位置沿用 `frame` 所在窗口的光标位置（`MouseInput` 自带坐标时才用得上，
/// winit 0.30 不带 ⇒ 由 `Runner` 传入它记录的最近光标位置）。
pub(crate) fn mouse_input(
    frame: &FrameCtx,
    state: ElementState,
    button: &MouseButton,
    cursor: (f64, f64),
) -> EngineEvent {
    pointer(
        frame,
        match state {
            ElementState::Pressed => PointerKind::Press,
            ElementState::Released => PointerKind::Release,
        },
        cursor,
        mouse_button(button),
        (0.0, 0.0),
    )
}

/// 滚轮。`delta` 统一成**虚拟像素**：winit 的 `LineDelta` 是"几格"，而菜单滚动逻辑要的是
/// 像素位移，混着两种单位读的人只能猜。一格按 [`LINE_PX`] 折算。
pub(crate) fn mouse_wheel(
    frame: &FrameCtx,
    delta: &MouseScrollDelta,
    cursor: (f64, f64),
) -> EngineEvent {
    const LINE_PX: f64 = 40.0;
    let physical = match delta {
        MouseScrollDelta::LineDelta(x, y) => (f64::from(*x) * LINE_PX, f64::from(*y) * LINE_PX),
        MouseScrollDelta::PixelDelta(p) => (p.x, p.y),
    };
    pointer(
        frame,
        PointerKind::Wheel,
        cursor,
        PointerButton::None,
        physical,
    )
}

/// 触摸：`Started`/`Moved`/`Ended`/`Canceled` 归一化成指针的按下/移动/松开，键固定为
/// `PointerButton::Other(0)`（一根手指 = 一个可绑定的"键"，而 `None` 留给纯移动/滚轮）。
pub(crate) fn touch(frame: &FrameCtx, phase: &TouchPhase, location: (f64, f64)) -> EngineEvent {
    let button = PointerButton::Other(0);
    match phase {
        TouchPhase::Started => pointer(frame, PointerKind::Press, location, button, (0.0, 0.0)),
        TouchPhase::Moved => pointer(
            frame,
            PointerKind::Moved,
            location,
            PointerButton::None,
            (0.0, 0.0),
        ),
        TouchPhase::Ended | TouchPhase::Cancelled => {
            pointer(frame, PointerKind::Release, location, button, (0.0, 0.0))
        }
    }
}

/// 指针事件的公共尾部：把物理坐标与物理位移换算到虚拟像素。
fn pointer(
    frame: &FrameCtx,
    kind: PointerKind,
    pos: (f64, f64),
    button: PointerButton,
    delta_physical: (f64, f64),
) -> EngineEvent {
    let (vx, vy) = frame.map_pointer(pos);
    // 位移同样要除掉缩放，否则 2× 窗口里滚一格 = 虚拟画面滚两格。
    let scale = if frame.scale > 0.0 {
        f64::from(frame.scale)
    } else {
        1.0
    };
    EngineEvent::Pointer {
        kind,
        pos: Vec2::new(vx, vy),
        button,
        delta: Vec2::new(
            (delta_physical.0 / scale) as f32,
            (delta_physical.1 / scale) as f32,
        ),
    }
}

/// 手柄事件的键名词汇表（喂进 [`EngineEvent::Key`]，`Trigger::Pad` 按这些字符串匹配）。
///
/// 只在真的链上 gilrs 的组合里存在（桌面三端 + `native`；wasm 无手柄、Android 上 gilrs 0.11
/// 没有可用后端 —— 矩阵见 `docs/input.md`）。
#[cfg(all(
    feature = "gilrs",
    not(target_arch = "wasm32"),
    not(target_os = "android")
))]
pub(crate) mod pad {
    /// 摇杆轴的码名（`"LeftStickX"`），只有摇杆有；扳机是模拟量、不做方向语义。
    pub(crate) fn stick_axis_name(axis: gilrs::Axis) -> Option<&'static str> {
        Some(match axis {
            gilrs::Axis::LeftStickX => "LeftStickX",
            gilrs::Axis::LeftStickY => "LeftStickY",
            gilrs::Axis::RightStickX => "RightStickX",
            gilrs::Axis::RightStickY => "RightStickY",
            _ => return None,
        })
    }

    /// 摇杆码 = 轴名 + 过阈值方向的 `"<轴名><符号>"`（`"LeftStickX+"`）。
    ///
    /// 归一化层把一根轴拆成两个"键"，是因为 [`ActionMap`](sisf_event::ActionMap) 的按住状态
    /// 本来就按边沿记账：回中即松开，游戏侧不必自己写过阈值/回中的判断。阈值归
    /// [`crate::gamepad::StickEdges`]（那层带迟滞），这里只管拼名字 ⇒ 全工程只有一处格式。
    pub(crate) fn stick_code(name: &'static str, direction: char) -> String {
        format!("{name}{direction}")
    }

    /// 按钮名（`"South"`、`"DPadUp"`、`"LeftTrigger"`……），直接取 gilrs 的变体名。
    ///
    /// 借用它而不是自己抄一张表：`ActionMap::rpg_maker()` 绑的就是这些字符串，
    /// 而 `Debug` 名跟着 gilrs 的变体重命名走（抄表则会静默错位）。
    pub(crate) fn button_code(button: gilrs::Button) -> String {
        format!("{button:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // T12：`cargo xtask wasm-test` 在 wasm 上跑这一组（见 docs/platforms.md）。
    use crate::config::AppConfig;
    use sisf_event::ActionEdge;
    use sisf_event::InputState;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;
    use winit::keyboard::KeyCode;

    fn frame() -> FrameCtx {
        // 1088×832 = 544×416 的整数 2 倍，letterbox 宽度为 0 ⇒ 换算只看倍率。
        FrameCtx::new(&AppConfig::default(), (1088, 832), 1.0)
    }

    /// 验收点名的一条：按下/连发/松开三种 winit 形态各翻成一种 `KeyKind`，且物理键名稳定。
    ///
    /// `KeyEvent` 的字段私有（构造不出来），所以翻译的两半分开钉：键名归 `key_name`，
    /// 边沿归 `key_kind`，二者拼起来正是 `key_event` 做的事。
    #[test]
    fn keyboard_edges_survive_the_mapping() {
        let f = frame();
        let mut input = InputState::new(f.input.map().clone());
        let mut bus = sisf_event::EngineBus::new();
        for _ in 0..2 {
            input.tick(&mut bus);
        }
        assert!(bus.peek().is_empty());

        assert_eq!(
            key_name(&PhysicalKey::Code(KeyCode::ArrowLeft)),
            "ArrowLeft"
        );
        assert_eq!(
            key_kind(ElementState::Pressed, false),
            KeyKind::Press,
            "首次按下"
        );
        assert_eq!(
            key_kind(ElementState::Pressed, true),
            KeyKind::Repeat,
            "系统连发不算重新按下"
        );
        assert_eq!(key_kind(ElementState::Released, false), KeyKind::Release);
        // 松开时 repeat 无意义，但也不许翻成连发（否则动作层会看到第三种状态）。
        assert_eq!(key_kind(ElementState::Released, true), KeyKind::Release);

        // 翻成动作：物理键名与 `ActionMap` 里的字符串同源，所以这一步不需要第二张表。
        let press = EngineEvent::key("ArrowLeft", KeyKind::Press, None);
        input.feed(&press, &mut bus);
        assert!(
            bus.peek()
                .iter()
                .any(|e| *e == EngineEvent::action("left", ActionEdge::Press))
        );
        input.feed(
            &EngineEvent::key("ArrowLeft", KeyKind::Repeat, None),
            &mut bus,
        );
        input.feed(
            &EngineEvent::key("ArrowLeft", KeyKind::Release, None),
            &mut bus,
        );
        assert!(
            bus.peek()
                .iter()
                .any(|e| *e == EngineEvent::action("left", ActionEdge::Release))
        );
    }

    /// 文本输入靠 `ch`，方向键是布局无关的 —— 二者不能互相顶替。
    #[test]
    fn the_layout_char_rides_along_with_the_physical_code() {
        let EngineEvent::Key { code, kind, ch } =
            EngineEvent::key("KeyA", KeyKind::Press, Some('A'))
        else {
            panic!("构造器就不是按键事件");
        };
        assert_eq!(code, "KeyA");
        assert_eq!(kind, KeyKind::Press);
        assert_eq!(ch, Some('A'), "字符跟着物理键名走，不必二选一");
    }

    /// 指针坐标必须在归一化这一步就落到虚拟像素上：这是"游戏侧不做映射"的前提。
    #[test]
    fn pointer_positions_are_virtual_pixels() {
        let f = frame();
        let e = cursor_moved(&f, (544.0, 416.0), None);
        let EngineEvent::Pointer {
            pos, delta, kind, ..
        } = e
        else {
            panic!("不是指针事件: {e:?}");
        };
        assert_eq!(kind, PointerKind::Moved);
        assert_eq!((pos.x, pos.y), (272.0, 208.0));
        assert_eq!((delta.x, delta.y), (0.0, 0.0), "首帧没有上一个光标位置");

        let moved = cursor_moved(&f, (544.0, 416.0), Some((416.0, 312.0)));
        let EngineEvent::Pointer { delta, .. } = moved else {
            panic!();
        };
        assert_eq!(
            (delta.x, delta.y),
            (64.0, 52.0),
            "物理位移要除掉缩放，否则 2× 窗口里 delta 翻倍"
        );
    }

    /// 滚轮的两种单位折成同一种：虚拟像素。
    #[test]
    fn wheel_delta_collapses_line_and_pixel_units() {
        let f = frame();
        let lines = mouse_wheel(&f, &MouseScrollDelta::LineDelta(0.0, 3.0), (0.0, 0.0));
        let pixels = mouse_wheel(
            &f,
            &MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition { x: 0.0, y: 120.0 }),
            (0.0, 0.0),
        );
        let EngineEvent::Pointer {
            kind: lines_kind,
            delta: ld,
            button,
            ..
        } = lines
        else {
            panic!();
        };
        assert_eq!(lines_kind, PointerKind::Wheel);
        assert_eq!(button, PointerButton::None);
        assert_eq!(ld.y, 60.0, "3 格 × 40px ÷ 2 倍缩放");
        let EngineEvent::Pointer { delta: pd, .. } = pixels else {
            panic!();
        };
        assert_eq!(pd.y, ld.y, "两种单位必须折成同一个数");
    }

    /// 鼠标键 = 指针动作源：`Trigger::Pointer` 靠 `Press`/`Release` 两种 kind 工作。
    #[test]
    fn mouse_buttons_are_pointer_edges_the_action_map_can_bind() {
        let f = frame();
        let pressed = mouse_input(
            &f,
            ElementState::Pressed,
            &MouseButton::Right,
            (100.0, 100.0),
        );
        let EngineEvent::Pointer {
            kind, button, pos, ..
        } = pressed
        else {
            panic!();
        };
        assert_eq!(kind, PointerKind::Press);
        assert_eq!(button, PointerButton::Right);
        assert_eq!((pos.x, pos.y), (50.0, 50.0));

        let mut input = InputState::new(sisf_event::ActionMap {
            actions: vec![sisf_event::ActionDef {
                name: "ok".to_owned(),
                triggers: vec![sisf_event::Trigger::Pointer(PointerButton::Left)],
                one_shot: true,
                autofire: None,
            }],
        });
        let mut bus = sisf_event::EngineBus::new();
        let left = mouse_input(&f, ElementState::Pressed, &MouseButton::Left, (0.0, 0.0));
        input.feed(&left, &mut bus);
        assert!(input.is_down("ok"));
        assert!(
            bus.peek()
                .contains(&EngineEvent::action("ok", ActionEdge::Press))
        );
        // 右键不该把左键的动作放开。
        bus = sisf_event::EngineBus::new();
        let right = mouse_input(&f, ElementState::Released, &MouseButton::Right, (0.0, 0.0));
        input.feed(&right, &mut bus);
        assert!(input.is_down("ok"));
        assert!(
            !bus.peek()
                .iter()
                .any(|e| *e == EngineEvent::action("ok", ActionEdge::Release))
        );
    }

    /// 侧键不许被挤成"无"，否则绑不上也没法诊断。
    #[test]
    fn extra_mouse_buttons_keep_their_number() {
        assert_eq!(
            mouse_button(&MouseButton::Other(3)),
            PointerButton::Other(3)
        );
    }

    /// 触摸 = 按下/移动/松开，且坐标同样已经映射。
    #[test]
    fn touch_phases_become_pointer_edges() {
        let f = frame();
        let started = touch(&f, &TouchPhase::Started, (1088.0, 832.0));
        let EngineEvent::Pointer {
            kind, button, pos, ..
        } = started
        else {
            panic!();
        };
        assert_eq!(kind, PointerKind::Press);
        assert_eq!(button, PointerButton::Other(0));
        assert_eq!((pos.x, pos.y), (544.0, 416.0), "右下角要落在虚拟右下角");

        let EngineEvent::Pointer {
            kind: k, button: b, ..
        } = touch(&f, &TouchPhase::Moved, (544.0, 416.0))
        else {
            panic!();
        };
        assert_eq!(k, PointerKind::Moved);
        assert_eq!(b, PointerButton::None, "移动没有按键");
        for phase in [TouchPhase::Ended, TouchPhase::Cancelled] {
            let EngineEvent::Pointer { kind: k, .. } = touch(&f, &phase, (0.0, 0.0)) else {
                panic!();
            };
            assert_eq!(k, PointerKind::Release, "{phase:?} 要放开手指");
        }
    }

    /// 摇杆码就是"另一种键名"：轴名只认两根摇杆，符号由死区那层决定。
    #[cfg(all(
        feature = "gilrs",
        not(target_arch = "wasm32"),
        not(target_os = "android")
    ))]
    #[test]
    fn stick_axes_become_directional_codes() {
        for (axis, name) in [
            (gilrs::Axis::LeftStickX, "LeftStickX"),
            (gilrs::Axis::LeftStickY, "LeftStickY"),
            (gilrs::Axis::RightStickX, "RightStickX"),
            (gilrs::Axis::RightStickY, "RightStickY"),
        ] {
            assert_eq!(pad::stick_axis_name(axis), Some(name));
            assert_eq!(pad::stick_code(name, '-'), format!("{name}-"));
        }
        // 扳机在 gilrs 里根本不是轴（是 `Button::LeftTrigger` 的模拟量），剩下的
        // `LeftZ`/`DPadX` 也不是摇杆 ⇒ 一律不产方向码，免得摇杆死区吃掉十字键。
        for axis in [gilrs::Axis::LeftZ, gilrs::Axis::RightZ, gilrs::Axis::DPadX] {
            assert_eq!(pad::stick_axis_name(axis), None, "{axis:?} 不该有方向码");
        }
        assert_eq!(pad::button_code(gilrs::Button::DPadUp), "DPadUp");
        assert_eq!(pad::button_code(gilrs::Button::LeftTrigger), "LeftTrigger");
    }
}
