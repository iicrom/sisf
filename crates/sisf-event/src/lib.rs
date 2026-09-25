//! 引擎事件总线（EngineEvent）与游戏事件解释器（Command/EventRunner）（T9/T10 实装）。
//!
//! T2 先落最小事件集：生命周期事件。T8 为了 `examples/tiled_map` 的键盘行走，把
//! [`EngineEvent::Key`] 这一条从 T9 提前接了过来；`Action/Pointer/Custom` 与 `EngineBus`
//! 仍在 T9 加入。

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

/// 归一化后的引擎事件。三端（native/wasm/android）只有这一套事件词汇表。
#[derive(Clone, Debug, PartialEq)]
pub enum EngineEvent {
    Lifecycle(Lifecycle),
    /// 键盘。`code` 是**物理键名**（`"ArrowLeft"`、`"KeyW"`、`"Escape"`……），
    /// 由 `sisf-app` 从 winit 的 `PhysicalKey` 归一化而来，与键盘布局无关 ——
    /// 布局相关的字符（`ch`）与命名动作（`Action`）等 T9 补。
    Key {
        code: String,
        kind: KeyKind,
    },
    // T9: Action / Pointer / Custom(u32, serde_json::Value)
}
