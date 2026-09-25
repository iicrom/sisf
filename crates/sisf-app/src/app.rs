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

    /// 固定步长逻辑更新：`dt` 恒为 `AppConfig::tick_hz` 的一个周期，一渲染帧内可能被
    /// 调用 0..=`MAX_CATCHUP`（[`crate::timing`]）次。渲染帧率不参与本方法的调用节奏，
    /// 所以逐格移动、路线计时这类逻辑在这里做才是确定的。
    fn tick(&mut self, _dt: Duration, _ctx: &mut FrameCtx);

    /// 渲染：`alpha` 是本帧落在两个 tick 之间的插值系数（0 = 上一 tick 之后立刻，
    /// 1 = 下一 tick 之前）。逐 tick 走过的位置用 `Sprite::prev_pos` 参与插值，
    /// 见 [`Sprite::interpolated`](sisf_sprite::Sprite::interpolated)。
    fn draw(&mut self, _alpha: f32, _renderer: &mut Renderer);
}
