//! 脚本后端接口：给 QuickJS / 移植层留的那一个口子。
//!
//! ## 为什么是一个 trait 而不是"嵌进虚拟机"
//!
//! 事件虚拟机（[`EventRunner`](crate::runner::EventRunner)）必须能独立于任何脚本引擎跑起
//! 来 —— 三端 check、单测、`dialog_demo` 都不该需要一个 JS 引擎才能过。所以引擎侧只定义
//! 三个问题：进入场景时后端要不要起跑（[`start`](ScriptBackend::start)）、每拍有没有话要
//! 说（[`tick`](ScriptBackend::tick)）、以及一条
//! [`Command::Custom`](crate::command::Command) 该怎么办（[`command`](ScriptBackend::command)）。
//! QuickJS 落地时实现这三条就够了，**不必改动虚拟机一行**。
//!
//! ## 编解码：只有两条通道
//!
//! - 引擎 → 脚本：[`Command::Custom(name, args)`](crate::command::Command)，`args` 是
//!   [`Param`]，与 `serde_json::Value` 一一对应（[`Param::to_json`]）。后端读到不认识的名字
//!   就返回 `None`，虚拟机登记一条告警后继续（不是静默空实现，见 DoD-4）。
//! - 脚本 → 引擎：[`EngineEvent::Custom(topic, value)`](crate::bus::EngineEvent)，后端在
//!   [`tick`](ScriptBackend::tick) 里从总线取走自己那几条（**其余按原序放回**，按键序列不能
//!   被脚本吃掉），编译成 [`Program`] 后压进 `queue` 交回。wasm 侧宿主用
//!   `sisf::app::post_custom` 投进来的就是这个通道。
//!
//! 也就是说：脚本能做的每件事，最后都落成"启动一段指令序列"，控制流仍由虚拟机持有。这条
//! 约束换来的东西很值 —— 存档只需要 [`VarStore`](crate::vars::VarStore) 与页号，不必冻结
//! 一颗 JS 堆。
//!
//! [`Param`]: crate::command::Param

use std::sync::Arc;

use crate::command::{Param, Program};
use crate::runner::EventCtx;

/// 一次性投递给虚拟机的"请启动这段程序"（`(归属事件 id, 程序)`）。
pub type Submission = (u32, Arc<Program>);

/// 脚本后端。三个方法都可以是空实现 —— 但空在这里是**有意的**（这台机器上没有脚本引擎），
/// 而不是"还没写"。
pub trait ScriptBackend {
    /// 进入一个场景/地图时调用一次（`entry` = 约定的入口名，例如 `"map001.on_load"`）。
    /// 不需要起跑就什么都不做。
    fn start(&mut self, entry: &str);

    /// 每拍一次，在指令执行**之前**。后端可以从 `ctx.bus` 取走自己的
    /// [`EngineEvent::Custom`](crate::bus::EngineEvent)（记得把别的原序放回），往 `queue`
    /// 里压要启动的程序，也可以通过 `ctx` 直接改精灵与变量。
    fn tick(&mut self, ctx: &mut EventCtx, queue: &mut Vec<Submission>);

    /// 一条 `Command::Custom` 的落地。`Some(program)` ⇒ 虚拟机把它压进调用栈，跑完回到
    /// 发起指令的下一条（**处理完了但没有指令要跑就交一个空程序**，`Program::default()`）。
    ///
    /// `None` = 这个名字我不认 ⇒ 虚拟机登记一次告警后继续。告警通道在事件机上（后端只拿到
    /// `name`/`args`，它没有别的地方可说话），所以"为什么这条没落地"这句话只能由虚拟机说，
    /// 而 `None` 就必须留给"不知道"这一种情况。
    fn command(&mut self, name: &str, args: &[Param]) -> Option<Arc<Program>>;
}

/// 没有脚本引擎时的后端（缺省）。
///
/// 装了它 = 每拍多一次空函数调用，且每条 `Custom` 都会被告知"后端不认这个名字"；不装
/// （`EventRunner::new()` 的形状）则连调用都不发生，告警换成"这台事件机没装脚本后端"。
/// 两条路径都留着是因为门面想让"我要不要脚本"这件事在**类型**上看得出来。
pub struct NullScriptBackend;

impl ScriptBackend for NullScriptBackend {
    fn start(&mut self, _entry: &str) {}

    fn tick(&mut self, _ctx: &mut EventCtx, _queue: &mut Vec<Submission>) {}

    fn command(&mut self, _name: &str, _args: &[Param]) -> Option<Arc<Program>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // T12：`cargo xtask wasm-test` 在 wasm 上跑这一组（见 docs/platforms.md）。
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    /// 接口得是对象安全的（`Box<dyn ScriptBackend>` 才装得进虚拟机）。完整的
    /// "装上去、三个钩子都 fire" 在 `runner::tests::backend_hooks_fire` —— 那边才有测试世界。
    #[test]
    fn null_backend_is_a_trait_object() {
        let mut b: Box<dyn ScriptBackend> = Box::new(NullScriptBackend);
        b.start("title");
        assert!(b.command("anything", &[Param::Int(1)]).is_none());
    }
}
