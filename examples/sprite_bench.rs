//! T3 验收示例：10 万 sprite 随机变换 + 帧时间打印（批量渲染器压测）。
//!
//! 精灵全部落在 640×480 虚拟空间（`ScaleMode::Fill`），每帧按随机速度/自旋推进，
//! 10% 走 additive。纹理只有内置白图（`WHITE_EPOCH`，图集是 T4 的事），所以
//! 整帧应当只切 **2 个 batch / 2 次 draw**——打印里把它和实测帧时间一起给出。
//!
//! 运行：
//! - 桌面：`cargo run --release -p sisf --example sprite_bench -- <帧数> <sprite 数>`
//!   （缺省 `300 100000`；帧数填 `0` 表示一直跑）
//! - wasm：`cargo xtask web sprite_bench` 后访问
//!   `http://127.0.0.1:8080/?ex=sprite_bench&wgl&sprites=20000`
//!   （`xtask web` 走 debug 构建，浏览器里请调小 `sprites`）
//! - 实测结论见 `docs/bench.md`

use std::time::Duration;

#[cfg(target_arch = "wasm32")]
use sisf::app::wasm_bindgen;
#[cfg(target_arch = "wasm32")]
use sisf::app::web_sys;
use sisf::app::{App, AppConfig, FrameCtx, ScaleMode, platform};
use sisf::event::{EngineEvent, Lifecycle};
use sisf::render::batch::BatchKey;
use sisf::render::vertex::Instance;
use sisf::render::{Blend, DrawStats, Renderer, WHITE_EPOCH};

/// 虚拟分辨率：`Fill` 下与物理像素 1:1，压测量的是真实填充率。
const VIRTUAL: (u32, u32) = (640, 480);
/// 周期报告的间隔（帧）。
const REPORT_EVERY: u64 = 60;
/// 超过这个间隔算"掉帧"（vsync 16.7ms + 一点余量）。
const SLOW_MS: f64 = 20.0;

/// 一个精灵：位置/尺寸/颜色/自旋全随机，每帧积分推进。
#[derive(Clone, Copy)]
struct Sprite {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    vx: f32,
    vy: f32,
    rot: f32,
    spin: f32,
    color: [f32; 4],
    add: bool,
}

/// xorshift64*：压测只要可复现的均匀分布，不引入 `rand`。
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// `[0,1)` 均匀浮点（取 24 位尾数）。
    fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

/// 帧时间统计：`win_*` 每个报告周期清零，其余累计到收工。
struct Bench {
    sprites: Vec<Sprite>,
    count: u32,
    frames: u64,
    limit: u64,
    dt: f32,
    sum_ms: f64,
    n: u64,
    min_ms: f64,
    max_ms: f64,
    slow: u64,
    win_sum_ms: f64,
    win_n: u64,
    win_min: f64,
    win_max: f64,
    stats: Option<DrawStats>,
    physical: (u32, u32),
}

impl Bench {
    fn new(count: u32) -> Self {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let sprites = (0..count)
            .map(|_| Sprite {
                x: rng.range(0.0, VIRTUAL.0 as f32),
                y: rng.range(0.0, VIRTUAL.1 as f32),
                w: rng.range(6.0, 34.0),
                h: rng.range(6.0, 34.0),
                vx: rng.range(-140.0, 140.0),
                vy: rng.range(-140.0, 140.0),
                rot: rng.range(0.0, std::f32::consts::TAU),
                spin: rng.range(-2.5, 2.5),
                color: [
                    rng.range(0.15, 1.0),
                    rng.range(0.15, 1.0),
                    rng.range(0.15, 1.0),
                    rng.range(0.55, 1.0),
                ],
                add: rng.unit() < 0.1,
            })
            .collect();
        Self {
            sprites,
            count,
            frames: 0,
            limit: 0,
            dt: 0.0,
            sum_ms: 0.0,
            n: 0,
            min_ms: f64::INFINITY,
            max_ms: 0.0,
            slow: 0,
            win_sum_ms: 0.0,
            win_n: 0,
            win_min: f64::INFINITY,
            win_max: 0.0,
            stats: None,
            physical: VIRTUAL,
        }
    }

    fn record(&mut self, ms: f64) {
        self.sum_ms += ms;
        self.n += 1;
        self.min_ms = self.min_ms.min(ms);
        self.max_ms = self.max_ms.max(ms);
        if ms > SLOW_MS {
            self.slow += 1;
        }
        self.win_sum_ms += ms;
        self.win_n += 1;
        self.win_min = self.win_min.min(ms);
        self.win_max = self.win_max.max(ms);
    }

    /// `avg/min/max` + batch/slot 数（"合并成一次 draw"的直观证据）。
    fn line(&self, tag: &str, avg: f64, min: f64, max: f64) {
        println!(
            "[sprite_bench] {tag} 帧 {} {}x{} sprites={} batch={} slot={} inst={} \
             帧时间 avg={:.2}ms({:.1}fps) min={:.2} max={:.2} 累计掉帧(>{SLOW_MS:.0}ms)={}",
            self.frames,
            self.physical.0,
            self.physical.1,
            self.count,
            self.stats.map_or(0, |s| s.batches),
            self.stats.map_or(0, |s| s.slots),
            self.stats.map_or(0, |s| s.instances),
            avg,
            1000.0 / avg,
            min,
            max,
            self.slow,
        );
        #[cfg(target_arch = "wasm32")]
        web_sys::console::log_1(&format!("[sprite_bench] {tag} avg={avg:.2}ms").into());
    }

    fn report_window(&mut self) {
        if self.win_n == 0 {
            return;
        }
        self.line(
            "周期",
            self.win_sum_ms / self.win_n as f64,
            self.win_min,
            self.win_max,
        );
        self.win_sum_ms = 0.0;
        self.win_n = 0;
        self.win_min = f64::INFINITY;
        self.win_max = 0.0;
    }

    fn report_result(&self) {
        if self.n == 0 {
            return;
        }
        self.line(
            "RESULT",
            self.sum_ms / self.n as f64,
            self.min_ms,
            self.max_ms,
        );
    }
}

impl App for Bench {
    fn on_event(&mut self, events: &[EngineEvent], _ctx: &mut FrameCtx) {
        for e in events {
            if matches!(e, EngineEvent::Lifecycle(Lifecycle::CloseRequested)) {
                self.report_result();
            }
        }
    }

    fn tick(&mut self, dt: Duration, ctx: &mut FrameCtx) {
        self.dt = dt.as_secs_f32();
        self.physical = ctx.physical_size;
        // 首帧间隔含 renderer 异步初始化，不计入统计。
        if self.frames > 0 {
            self.record(dt.as_secs_f64() * 1000.0);
        }
        self.frames += 1;
        if self.frames.is_multiple_of(REPORT_EVERY) {
            self.report_window();
        }
        if self.limit > 0 && self.frames >= self.limit {
            self.report_result();
            #[cfg(not(target_arch = "wasm32"))]
            std::process::exit(0);
        }
    }

    fn draw(&mut self, _alpha: f32, renderer: &mut Renderer) {
        // `stats()` 是本帧之前最后一次 `end_frame` 的统计（上一帧）。
        self.stats = renderer.stats();
        let (vw, vh) = (VIRTUAL.0 as f32, VIRTUAL.1 as f32);
        let dt = self.dt;
        for (i, s) in self.sprites.iter_mut().enumerate() {
            s.x = (s.x + s.vx * dt).rem_euclid(vw);
            s.y = (s.y + s.vy * dt).rem_euclid(vh);
            s.rot += s.spin * dt;
            // 偶数下标把宽取负：翻转走 `size` 符号路径（等价于交换 uv0/uv1）。
            let size = [if i & 1 == 0 { -s.w } else { s.w }, s.h];
            let mut instance = Instance::rect([s.x, s.y], size, s.color, 0);
            instance.rot = s.rot;
            renderer.push(
                BatchKey {
                    layer: 0,
                    epoch: WHITE_EPOCH,
                    blend: if s.add { Blend::Add } else { Blend::Normal },
                    z: (i as i32) & 1023,
                    insertion: 0,
                },
                instance,
            );
        }
    }
}

fn config() -> AppConfig {
    AppConfig {
        title: "SISF sprite_bench (T3)".to_owned(),
        virtual_size: VIRTUAL,
        scale_mode: ScaleMode::Fill,
        clear: [0.0, 0.0, 0.0, 1.0],
        ..Default::default()
    }
}

/// `(跑满多少帧退出, 精灵数)`；wasm 下精灵数来自 `?sprites=`。
fn bench_args() -> (u64, u32) {
    #[cfg(target_arch = "wasm32")]
    {
        let search = web_sys::window()
            .and_then(|w| w.location().search().ok())
            .unwrap_or_default();
        let sprites = search
            .split('&')
            .filter_map(|kv| kv.split_once('='))
            .find(|(k, _)| *k == "sprites")
            .and_then(|(_, v)| v.parse().ok())
            .unwrap_or(20_000u32);
        // `xtask web` 是 debug 构建，且浏览器里没有"退出进程"。
        (0, sprites)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut it = std::env::args().skip(1);
        let frames = it.next().and_then(|a| a.parse().ok()).unwrap_or(300);
        let sprites = it.next().and_then(|a| a.parse().ok()).unwrap_or(100_000);
        (frames, sprites)
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn sisf_start() {
    real_main();
}

pub fn main() {
    real_main();
}

fn real_main() {
    let (frames, sprites) = bench_args();
    println!("[sprite_bench] 启动 sprites={sprites} limit={frames}");
    #[cfg(target_arch = "wasm32")]
    web_sys::console::log_1(&format!("[sprite_bench] 启动 sprites={sprites}").into());
    let mut app = Bench::new(sprites);
    app.limit = frames;
    if let Err(e) = platform::run(app, config()) {
        panic!("sprite_bench 运行失败: {e}");
    }
}
