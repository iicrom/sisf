//! 引擎事件总线（EngineEvent）与游戏事件解释器（Command/EventRunner）（T9/T10 实装）。
//!
//! - [`bus`]：三端唯一的输入词汇表（[`EngineEvent`]）+ 一条 FIFO 队列（[`EngineBus`]）。
//!   事件的**产生**在 `sisf-app`（winit/gilrs 各翻译一次），这里只定义形状与排队。
//! - [`input`]：命名动作（[`ActionMap`]，可从 RON 载入）与按住/边沿状态（[`InputState`]）。
//! - T10：[`command`] 是磁盘上的指令数据与编译后的扁平 IR，[`vars`] 是三态变量与条件求值，
//!   [`runner`] 是把指令跑成副作用的虚拟机，[`script`] 是给脚本后端留的那一个口子。
//!
//! 两条总线要分清：[`EngineBus`] 装的是**设备**事件（按键/指针/生命周期），
//! [`Command`](command::Command) 装的是**游戏**事件（对话/移动/BGM）。T9 把前者归一化成
//! 命名动作，T10 让后者去读前者。

pub mod bus;
pub mod command;
pub mod input;
pub mod runner;
pub mod script;
pub mod vars;

pub use bus::{ActionEdge, EngineBus, EngineEvent, KeyKind, Lifecycle, PointerButton, PointerKind};
pub use command::{
    AnimEffect, CmpOp, Command, CommandError, Cond, EventDef, EventScript, Expr, FadeTarget, Instr,
    MAX_NESTING, Param, Program, RouteSpec, RouteStep, SetOp, Slot, Sound, SoundChannel,
};
pub use input::{ActionDef, ActionMap, Autofire, InputState, MapError, Trigger};
pub use runner::{
    AudioPort, EventCtx, EventRunner, MAX_FRAMES, Prompt, RunnerStatus, Scene, UiPort, actions,
};
pub use script::{NullScriptBackend, ScriptBackend, Submission};
pub use vars::{Rng, VarStore};
