//! 引擎事件总线：三端唯一的一套事件词汇表 + 一条先进先出的队列。
//!
//! 词汇表在这里，**产生**在 `sisf-app`：winit 的 `WindowEvent`/`DeviceEvent` 与 gilrs 的手柄
//! 事件各自只被翻译一次（native/wasm/android 走同一条 `Runner` 路径），游戏侧看到的
//! [`EngineEvent`] 与平台无关。要按"命名动作"而不是裸按键来读输入，见 [`crate::input`]。
//!
//! [`EngineEvent::Custom`] 是留给 JS 与移植层的编解码通道（QuickJS 落地时的进出口）：
//! 载荷是 `serde_json::Value`，因为跨 wasm 边界时它天然对应 JS 侧的那个对象。

use sisf_sprite::Vec2;

/// 应用/窗口生命周期状态（由各 platform 归一化上报）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    Resumed,
    Suspended,
    Focused(bool),
    CloseRequested,
}

/// 按键边沿。`Repeat` 是操作系统的连发（RPG Maker 的按住连走手感靠它，不自己计时）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    Press,
    Repeat,
    Release,
}

/// 指针事件的种类。触摸在三端都归一化成本类型（触摸 = 左手键 + 位置）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerKind {
    /// 位置变化（`pos` 有效，`delta` 是相对上一次的位移）。
    Moved,
    Press,
    Release,
    /// 滚轮：`delta` 是滚动量（虚拟像素/格），`pos` 是光标所在位置。
    Wheel,
}

/// 指针按键。可序列化成 RON，供 [`crate::input::ActionMap`] 绑定。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerButton {
    Left,
    Right,
    Middle,
    Other(u16),
    /// 无按键的纯移动/滚动。
    None,
}

/// 命名动作的三态边沿，语义见 [`crate::input`] 模块文档。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionEdge {
    /// 松开→按下（含连发放大的每一次重触）。
    Press,
    /// 按着不放，每个逻辑拍一条。
    Hold,
    /// 按下→松开。
    Release,
}

/// 归一化后的引擎事件。三端（native/wasm/android）只有这一套事件词汇表。
#[derive(Clone, Debug, PartialEq)]
pub enum EngineEvent {
    /// [`crate::input::ActionMap`] 解析出来的命名动作（决定/取消/上/下……）。
    ///
    /// 由引擎在派发给自己之前先算好：裸按键与手柄摇杆在同一动作下收敛成一条事件，
    /// 游戏代码不必知道玩家用的是哪个设备。
    Action {
        name: String,
        edge: ActionEdge,
    },
    /// 键盘。`code` 是**物理键名**（`"ArrowLeft"`、`"KeyW"`、`"Escape"`……），
    /// 与键盘布局无关；`ch` 是布局相关的字符（`Shift+2` → `"@"`），只在按下时有值，
    /// 文本输入靠它而不是靠拼 `code`。
    Key {
        code: String,
        kind: KeyKind,
        ch: Option<char>,
    },
    /// 鼠标/触摸。`pos` 已经过 `FrameCtx::map_pointer` 映射到**虚拟像素**，
    /// 与 letterbox 边条和缩放无关 ⇒ 命中测试可以直接和 sprite 坐标比。
    Pointer {
        kind: PointerKind,
        pos: Vec2,
        button: PointerButton,
        delta: Vec2,
    },
    Lifecycle(Lifecycle),
    /// 移植层/宿主自定义消息。`id` 是双方约定的主题号，载荷任意 JSON。
    Custom(u32, serde_json::Value),
}

impl EngineEvent {
    /// 构造一条裸按键事件（`ch` 由调用方决定，通常来自 winit 的 `KeyEvent::text`）。
    pub fn key(code: impl Into<String>, kind: KeyKind, ch: Option<char>) -> Self {
        Self::Key {
            code: code.into(),
            kind,
            ch,
        }
    }

    /// 构造一条命名动作事件。
    pub fn action(name: impl Into<String>, edge: ActionEdge) -> Self {
        Self::Action {
            name: name.into(),
            edge,
        }
    }
}

/// 事件队列：入队即本帧有效，`drain` 保序取空。
///
/// 刻意做成"没有监听器图"的一层（设计文档 §Event (a)）：订阅、优先级、冒泡都在游戏侧做，
/// 引擎只保证「同一次输入只翻译一次」和「顺序 = 发生顺序」。
#[derive(Clone, Debug, Default)]
pub struct EngineBus {
    pending: Vec<EngineEvent>,
}

impl EngineBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, event: EngineEvent) {
        self.pending.push(event);
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// 不取走地看一眼当前排队（诊断/断言用）。
    pub fn peek(&self) -> &[EngineEvent] {
        &self.pending
    }

    /// 取空队列并保持入队顺序（FIFO）。派发点唯一，故返回所有权比迭代器更好用：
    /// 调用方要在持有事件的同时可变借用别的字段。
    pub fn drain(&mut self) -> Vec<EngineEvent> {
        std::mem::take(&mut self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // T12：`cargo xtask wasm-test` 在 wasm 上跑这一组（见 docs/platforms.md）。
    use serde_json::json;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    fn key(code: &str, kind: KeyKind) -> EngineEvent {
        EngineEvent::key(code, kind, None)
    }

    /// 验收点名的那条：`drain` 必须原样还原入队顺序，且真的把队列腾空。
    #[test]
    fn drain_preserves_push_order_and_empties_the_bus() {
        let mut bus = EngineBus::new();
        bus.push(EngineEvent::Lifecycle(Lifecycle::Resumed));
        bus.push(key("ArrowLeft", KeyKind::Press));
        bus.push(EngineEvent::action("left", ActionEdge::Press));
        bus.push(EngineEvent::Pointer {
            kind: PointerKind::Moved,
            pos: Vec2::new(3.0, 4.0),
            button: PointerButton::None,
            delta: Vec2::ZERO,
        });
        bus.push(EngineEvent::Custom(7, json!({"scene": "title"})));
        bus.push(key("ArrowLeft", KeyKind::Release));
        assert_eq!(bus.len(), 6);

        let out = bus.drain();
        assert_eq!(
            out,
            vec![
                EngineEvent::Lifecycle(Lifecycle::Resumed),
                key("ArrowLeft", KeyKind::Press),
                EngineEvent::action("left", ActionEdge::Press),
                EngineEvent::Pointer {
                    kind: PointerKind::Moved,
                    pos: Vec2::new(3.0, 4.0),
                    button: PointerButton::None,
                    delta: Vec2::ZERO,
                },
                EngineEvent::Custom(7, json!({"scene": "title"})),
                key("ArrowLeft", KeyKind::Release),
            ]
        );
        assert!(bus.is_empty() && bus.drain().is_empty());
    }

    /// 两次 drain 之间 push 的东西不能串到上一批里去（派发点每帧只认自己那批）。
    #[test]
    fn pushes_after_a_drain_start_a_fresh_batch() {
        let mut bus = EngineBus::new();
        bus.push(key("A", KeyKind::Press));
        assert_eq!(bus.drain().len(), 1);
        bus.push(key("B", KeyKind::Press));
        assert_eq!(bus.drain(), vec![key("B", KeyKind::Press)]);
    }

    /// 同一批里重复的边沿照原样留着：去重是 `ActionMap` 的活，不是队列的活。
    #[test]
    fn duplicates_are_kept_not_collapsed() {
        let mut bus = EngineBus::new();
        bus.push(EngineEvent::action("ok", ActionEdge::Hold));
        bus.push(EngineEvent::action("ok", ActionEdge::Hold));
        assert_eq!(bus.drain().len(), 2);
    }

    /// `peek` 看得见但不取走（`drain` 才是唯一的取走方式）。
    #[test]
    fn peek_does_not_empty_the_queue() {
        let mut bus = EngineBus::new();
        bus.push(key("A", KeyKind::Press));
        assert_eq!(bus.peek().len(), 1);
        assert_eq!(bus.peek().len(), 1);
        assert_eq!(bus.drain().len(), 1);
        assert!(bus.peek().is_empty());
    }
}
