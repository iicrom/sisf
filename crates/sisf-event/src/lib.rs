//! 引擎事件总线（EngineEvent）与游戏事件解释器（Command/EventRunner）（T9/T10 实装）。
//!
//! T2 先落最小事件集：生命周期事件。`Action/Key/Pointer/Custom` 与 `EngineBus` 在 T9 加入。

/// 应用/窗口生命周期状态（由各 platform 归一化上报）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    Resumed,
    Suspended,
    Focused(bool),
    CloseRequested,
}

/// 归一化后的引擎事件。三端（native/wasm/android）只有这一套事件词汇表。
#[derive(Clone, Debug, PartialEq)]
pub enum EngineEvent {
    Lifecycle(Lifecycle),
    // T9: Action / Key / Pointer / Custom(u32, serde_json::Value)
}
