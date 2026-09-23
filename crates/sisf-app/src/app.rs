//! 游戏侧实现的主循环接口。
//!
//! 约束：所有签名都不跨帧持有 wgpu 借用（`FrameCtx` 为纯数据，`Renderer` 按 `&mut` 借出）。

use std::time::Duration;

use sisf_event::EngineEvent;
use sisf_render::Renderer;

use crate::frame::FrameCtx;

pub trait App {
    /// 收到归一化引擎事件批次（含生命周期）。
    fn on_event(&mut self, _events: &[EngineEvent], _ctx: &mut FrameCtx) {}

    /// 固定步长逻辑更新（T8 起按 `AppConfig::tick_hz` 累加器驱动；当前每渲染帧一次）。
    fn tick(&mut self, _dt: Duration, _ctx: &mut FrameCtx);

    /// 渲染：`alpha` 为 tick 间插值系数（T8 前恒为 1.0）。
    fn draw(&mut self, _alpha: f32, _renderer: &mut Renderer);
}
