//! 固定步长时钟：累加器 + 补帧上限 + `draw` 的插值系数。
//!
//! 三件事被这一个类型管住，因为它们是同一个决定的三个面 —— **逻辑只按 `tick_hz` 走，
//! 渲染帧率不参与**：
//!
//! 1. **累加器**：每帧把真实 `dt` 攒进 `accumulator`，够一个周期就产出一 tick。于是
//!    120Hz 屏幕与 30Hz 屏幕上的游戏速度一致，逐格移动也不会半拍到位。
//! 2. **补帧上限 [`MAX_CATCHUP`]**：一帧最多补 5 tick。上限的理由不是"省 CPU"，而是防
//!    死亡螺旋 —— 一帧卡了 1 秒，若把 60 拍全跑完这一帧就更慢，下一帧欠得更多。超限的
//!    时间直接**丢弃**并计入 [`TickClock::dropped`]（可观测的"我跟不上了"，而不是静默地
//!    假装什么都没发生）。
//! 3. **`alpha`**：累加器剩下的余量除以周期，就是"本渲染帧落在两个 tick 之间的哪里"。
//!    `App::draw(alpha)` 拿它插值 `Sprite::prev_pos`（`sisf_sprite::Sprite::interpolated`），
//!    于是逻辑是离散的、画面是连续的 —— 固定步长与可变帧率两头都照顾到的唯一写法。
//!
//! 挂起（`Suspended`、窗口遮挡、页面隐藏）时调用方 [`TickClock::reset`]：回来后的第一帧
//! 不吃睡眠期间的 `dt`，否则一回来就白跑 5 拍并把时间记进 `dropped`。
//!
//! ```
//! use std::time::Duration;
//! use sisf_app::timing::{TickClock, MAX_CATCHUP};
//!
//! let mut clock = TickClock::new(60.0);
//! let ms = |v: u64| Duration::from_millis(v);
//! // 16ms 不够一个周期：这一帧不跑逻辑，但画面要按 alpha 插到 ~96% 的位置。
//! assert_eq!(clock.step(ms(16)), 0);
//! assert!((clock.alpha() - 0.96).abs() < 1e-2);
//! // 尖峰：1 秒只补 5 拍，其余时间记进 `dropped` 而不是把这一帧拖死。
//! assert_eq!(clock.step(ms(1000)), MAX_CATCHUP);
//! assert!(clock.dropped() >= ms(900));
//! ```

use std::time::Duration;

/// 一帧最多补跑多少个 tick（todo.md T8 定的数）。超出的时间丢弃并计入 `dropped`。
pub const MAX_CATCHUP: u32 = 5;

/// 固定步长时钟。`Runner` 每帧 [`step`](Self::step) 一次；`App` 只看到"这帧跑了几拍"
/// 与"本帧落在哪里"（[`alpha`](Self::alpha)）。
#[derive(Clone, Debug)]
pub struct TickClock {
    period: Duration,
    accumulator: Duration,
    alpha: f32,
    /// 已产出的 tick 总数（不含被上限丢弃的部分）。
    ticks: u64,
    /// 被补帧上限丢弃的累计时间（诊断用：非零即"逻辑比墙钟慢过"）。
    dropped: Duration,
    /// 触发过补帧上限的次数（比 `dropped` 更好读：0 = 从没欠过债）。
    spikes: u64,
}

impl Default for TickClock {
    fn default() -> Self {
        Self::new(60.0)
    }
}

impl TickClock {
    /// 按 `hz` 建时钟。`hz` 非有限或 `<= 0` 时退成 60（配置写错不该让循环停摆）。
    pub fn new(hz: f64) -> Self {
        let hz = if hz.is_finite() && hz > 0.0 { hz } else { 60.0 };
        Self::with_period(Duration::from_secs_f64(1.0 / hz))
    }

    pub fn with_period(period: Duration) -> Self {
        // 零周期会让 `while accumulator >= period` 转到天荒地老，也除不出 alpha。
        let period = if period.is_zero() {
            Duration::from_secs_f64(1.0 / 60.0)
        } else {
            period
        };
        Self {
            period,
            accumulator: Duration::ZERO,
            alpha: 1.0,
            ticks: 0,
            dropped: Duration::ZERO,
            spikes: 0,
        }
    }

    /// 一个 tick 的时长（喂给 `App::tick` 的 `dt` 恒等于它）。
    pub fn period(&self) -> Duration {
        self.period
    }

    /// [`step`](Self::step) 之后本帧的插值系数：0 = 上一拍刚跑完，1 = 下一拍就在眼前。
    pub fn alpha(&self) -> f32 {
        self.alpha
    }

    /// 已产出的 tick 总数。
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// 被补帧上限丢弃的累计时间。
    pub fn dropped(&self) -> Duration {
        self.dropped
    }

    /// 触发过补帧上限的次数。
    pub fn spikes(&self) -> u64 {
        self.spikes
    }

    /// 待处理的余量（不足一个周期的那部分）。
    pub fn accumulator(&self) -> Duration {
        self.accumulator
    }

    /// 喂进一帧的真实时长，返回**本帧应跑的 tick 数**（`0..=MAX_CATCHUP`）。
    pub fn step(&mut self, dt: Duration) -> u32 {
        self.accumulator = self.accumulator.saturating_add(dt);
        let mut ticks = 0u32;
        while self.accumulator >= self.period {
            if ticks == MAX_CATCHUP {
                // 上限已到：欠下的不是"还欠着的逻辑"，是过期的墙钟 —— 丢掉并留痕。
                self.dropped = self.dropped.saturating_add(self.accumulator);
                self.spikes += 1;
                self.accumulator = Duration::ZERO;
                break;
            }
            self.accumulator -= self.period;
            ticks += 1;
            self.ticks += 1;
        }
        self.alpha =
            (self.accumulator.as_secs_f64() / self.period.as_secs_f64()).clamp(0.0, 1.0) as f32;
        ticks
    }

    /// 冻结：清空余量（挂起/恢复、读档后调用）。产出计数与诊断量保留。
    pub fn reset(&mut self) {
        self.accumulator = Duration::ZERO;
        self.alpha = 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(v: u64) -> Duration {
        Duration::from_millis(v)
    }

    /// 60Hz 的周期不是整毫秒（16.666…ms），所以下面用 50Hz 做"正好整除"的断言，
    /// 用 60Hz 验证"攒够才补"的非整除路径。
    #[test]
    fn a_frame_shorter_than_a_period_produces_no_tick() {
        let mut clock = TickClock::new(60.0);
        assert_eq!(clock.step(ms(8)), 0, "8ms 不到一个周期");
        assert_eq!(clock.step(ms(8)), 0, "16ms 还差 0.67ms 才算一拍");
        assert_eq!(clock.step(ms(8)), 1, "24ms 攒出一拍，余量留着");
        assert!(clock.accumulator() < ms(8));
        assert_eq!(clock.ticks(), 1);
    }

    #[test]
    fn alpha_is_the_leftover_of_the_accumulator() {
        let mut clock = TickClock::new(50.0); // 周期正好 20ms
        assert_eq!(clock.step(ms(20)), 1);
        assert!(
            clock.alpha() < 1e-6,
            "刚跑完一拍 ⇒ alpha 回到 0，got {}",
            clock.alpha()
        );
        assert_eq!(clock.step(ms(9)), 0);
        assert!(
            (clock.alpha() - 0.45).abs() < 1e-3,
            "余 9ms / 20ms ⇒ alpha 0.45，got {}",
            clock.alpha()
        );
        assert_eq!(clock.step(ms(11)), 1);
        clock.step(ms(10_000));
        assert!(
            (0.0..=1.0).contains(&clock.alpha()),
            "超长帧也不能把 alpha 挤出 [0,1]"
        );
    }

    /// 验收正题：`dt` 尖峰只补 `MAX_CATCHUP` 拍，余量丢弃且留痕，循环不被拖死。
    #[test]
    fn a_huge_spike_is_capped_and_its_debt_is_dropped() {
        let mut clock = TickClock::new(60.0);
        assert_eq!(clock.step(Duration::from_secs(1)), MAX_CATCHUP);
        assert_eq!(clock.ticks(), MAX_CATCHUP as u64);
        assert!(clock.dropped() > ms(900), "1s 里只跑了 5 拍");
        assert_eq!(clock.spikes(), 1);
        // 尖峰过后回到正常节奏：欠的债不会慢慢补上一通（那正是死亡螺旋）。
        assert_eq!(clock.step(ms(16)), 0);
        assert_eq!(clock.step(ms(50)), 3);
        assert_eq!(clock.spikes(), 1, "正常帧不该再算尖峰");
    }

    #[test]
    fn steady_frame_rates_are_all_one_tick_per_frame_or_none() {
        // 50Hz 渲染 + 50Hz 逻辑：一帧一拍，不多不少。
        let mut clock = TickClock::new(50.0);
        for i in 0..60 {
            assert_eq!(clock.step(ms(20)), 1, "第 {i} 帧");
        }
        assert_eq!(clock.ticks(), 60);
        assert_eq!(clock.dropped(), Duration::ZERO, "稳定帧率不该丢时间");
        // 120Hz 渲染 + 50Hz 逻辑：一半的帧不跑逻辑，且总拍数只跟墙钟走。
        let mut clock = TickClock::new(50.0);
        let mut total = 0;
        for _ in 0..120 {
            let ticks = clock.step(ms(8));
            assert!(ticks <= 1, "120Hz 下一帧补两拍 = 累加器算错了");
            total += ticks;
        }
        assert_eq!(total, 48, "960ms / 20ms");
        assert_eq!(clock.spikes(), 0);
    }

    /// 挂起冻结：回来第一帧不吃睡眠时长。
    #[test]
    fn reset_freezes_the_accumulator_for_a_resume() {
        let mut clock = TickClock::new(60.0);
        clock.step(ms(10));
        assert!(clock.accumulator() >= ms(10));
        clock.reset();
        assert_eq!(clock.accumulator(), Duration::ZERO);
        assert_eq!(clock.alpha(), 1.0);
        // 睡了 30 秒再回来（调用方给 dt=0 + reset）：一帧不多跑。
        assert_eq!(clock.step(Duration::ZERO), 0);
        assert_eq!(
            clock.dropped(),
            Duration::ZERO,
            "reset 不该把睡眠时间记成欠债"
        );
        assert_eq!(clock.ticks(), 0);
    }

    #[test]
    fn bad_config_degrades_instead_of_stalling_the_loop() {
        for hz in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let mut clock = TickClock::new(hz);
            assert_eq!(clock.period(), Duration::from_secs_f64(1.0 / 60.0));
            assert_eq!(clock.step(ms(20)), 1, "非法 hz 仍按 60Hz 推进");
        }
        // 零周期单独防住（`with_period` 是公开构造器）。
        let mut clock = TickClock::with_period(Duration::ZERO);
        assert!(!clock.period().is_zero());
        assert_eq!(clock.step(ms(5)), 0);
        // 时间只增不减：`Duration::MAX` 也不会把累加器绕回小值。
        let mut clock = TickClock::new(50.0);
        clock.step(Duration::MAX);
        assert!(clock.dropped() > Duration::from_secs(3600));
        assert_eq!(clock.ticks(), MAX_CATCHUP as u64);
    }
}
