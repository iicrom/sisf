//! 引擎事件总线（EngineEvent）与游戏事件解释器（Command/EventRunner）（T9/T10 实装）。
//!
//! - [`bus`]：三端唯一的输入词汇表（[`EngineEvent`]）+ 一条 FIFO 队列（[`EngineBus`]）。
//!   事件的**产生**在 `sisf-app`（winit/gilrs 各翻译一次），这里只定义形状与排队。
//! - [`input`]：命名动作（[`ActionMap`]，可从 RON 载入）与按住/边沿状态（[`InputState`]）。
//! - T10 追加：`command.rs`/`runner.rs`/`vars.rs`/`script.rs` —— 游戏事件解释器。

pub mod bus;
pub mod input;

pub use bus::{ActionEdge, EngineBus, EngineEvent, KeyKind, Lifecycle, PointerButton, PointerKind};
pub use input::{ActionDef, ActionMap, Autofire, InputState, MapError, Trigger};
