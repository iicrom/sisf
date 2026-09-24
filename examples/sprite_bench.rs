//! T4 验收示例：VFS + 图集 + 混合图源（PNG/JPG/WebP/BMP）压测。
//!
//! 图源经 `Vfs` 读取 → `AssetStore<Bitmap>` 后台解码（解码闭包顺带装箱进图集）→
//! 主线程单一 staging ring 上传 → 精灵按图集子矩形采样。于是**纹理切换次数 = 用
//! 到的图集页数**，与"多少个图源、多少张图"无关，合批断言就是：
//!
//! ```text
//! batches == 用到的页数 × blend 种数      （槽容量配到 sprite 数 ⇒ 单槽）
//! ```
//!
//! 10 万 sprite 全落在 640×480 虚拟空间（`ScaleMode::Fill`），10% 走 additive；
//! 帧时间连同 batch/页数一起打印，断言不成立直接 panic（跑不到"断言通过"就是失败）。
//!
//! 运行：
//! - 桌面：`cargo run --release -p sisf --example sprite_bench -- <帧数> <sprite 数>`
//!   （缺省 `300 100000`；帧数填 `0` 表示一直跑。图源来自
//!   `crates/sisf-media/tests/fixtures/`，经 `DirProvider` 挂载）
//! - wasm：`cargo xtask web sprite_bench` 后访问
//!   `http://127.0.0.1:8080/?ex=sprite_bench&wgl&sprites=20000`
//!   （图源是 `web/assets/`，清单 `web/assets/manifest.txt` 由宿主 `fetch` 预取灌进
//!   `WebManifest`；`xtask web` 是 debug 构建，浏览器里请调小 `sprites`）
//! - 实测结论见 `docs/bench.md`

use std::sync::Arc;
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
use sisf::app::wasm_bindgen;
use sisf::app::{App, AppConfig, FrameCtx, ScaleMode, platform};
use sisf::asset::{AssetStore, Handle, Request, Vfs, vfs::path as vpath};
#[cfg(not(target_arch = "wasm32"))]
use sisf::asset::{DirProvider, NativeTasks};
#[cfg(target_arch = "wasm32")]
use sisf::asset::{VfsPath, VfsProvider, WasmTasks, WebManifest};
use sisf::event::{EngineEvent, Lifecycle};
use sisf::render::batch::BatchKey;
use sisf::render::vertex::Instance;
use sisf::render::{Atlas, AtlasPages, Bitmap, Blend, DrawStats, Renderer, UploadRing};

/// 虚拟分辨率：`Fill` 下与物理像素 1:1，压测量的是真实填充率。
const VIRTUAL: (u32, u32) = (640, 480);
/// 周期报告的间隔（帧）。
const REPORT_EVERY: u64 = 60;
/// 超过这个间隔算"掉帧"（vsync 16.7ms + 一点余量）。
const SLOW_MS: f64 = 20.0;

/// 混合图源：四种魔数各一个，尺寸决定落进哪个页桶。
const SOURCES: [&str; 4] = [
    "img_64x48.png",
    "img_80x60.jpg",
    "img_40x40.webp",
    "img_24x18_24bpp.bmp",
];

/// wasm 预取清单：一行一个路径（构建期生成，内容同 `web/assets/manifest.txt`）。
#[cfg(target_arch = "wasm32")]
const MANIFEST: &str = "img_64x48.png\nimg_80x60.jpg\nimg_40x40.webp\nimg_24x18_24bpp.bmp\n";

#[cfg(not(target_arch = "wasm32"))]
type Runner = NativeTasks;
#[cfg(target_arch = "wasm32")]
type Runner = WasmTasks;

/// 一个精灵：位置/尺寸/颜色/自旋全随机，每帧积分推进；`src` 是图源下标。
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
    src: usize,
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

/// wasm 下 `Vfs` 要跟着 `App: 'static` 活，而预取任务也要摸同一份清单：
/// 清单放 `Arc`，这里包一层 provider 转调 `WebManifest`。
#[cfg(target_arch = "wasm32")]
struct SharedManifest(Arc<WebManifest>);

#[cfg(target_arch = "wasm32")]
impl VfsProvider for SharedManifest {
    fn read(&self, p: &VfsPath) -> Option<Vec<u8>> {
        self.0.read(p)
    }

    fn exists(&self, p: &VfsPath) -> bool {
        self.0.exists(p)
    }
}

/// 图源装载状态：VFS + 解码任务 + 图集 + 上传队列 + 已就绪的强句柄。
struct Sources {
    vfs: Vfs,
    store: AssetStore<Bitmap>,
    runner: Runner,
    /// 装箱状态（`Send`，解码线程里也碰它）。
    atlas: Atlas,
    /// 页纹理：只能在持有 device 的主线程侧。
    pages: AtlasPages,
    uploads: UploadRing,
    /// 与 `SOURCES` 同下标；`None` = 还在飞。持句柄 = 该页不会被驱逐。
    bitmaps: Vec<Option<Handle<Bitmap>>>,
    /// 装箱/采样时基（LRU 用），每个 draw 递增一次。
    frame: u64,
}

impl Sources {
    fn new() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let mut vfs = Vfs::new();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../crates/sisf-media/tests/fixtures");
            vfs.register(Box::new(DirProvider::new(root)));
        }
        #[cfg(target_arch = "wasm32")]
        let vfs = {
            let manifest = Arc::new(WebManifest::new(MANIFEST));
            prefetch(&manifest);
            let mut vfs = Vfs::new();
            vfs.register(Box::new(SharedManifest(manifest)));
            vfs
        };
        Self {
            vfs,
            store: AssetStore::new(),
            runner: Runner::new(),
            atlas: Atlas::new(),
            pages: AtlasPages::new(),
            uploads: UploadRing::new(),
            bitmaps: (0..SOURCES.len()).map(|_| None).collect(),
            frame: 0,
        }
    }

    fn loaded(&self) -> usize {
        self.bitmaps.iter().filter(|b| b.is_some()).count()
    }

    /// 推进一帧装载：投递未开始的 → 收割在飞的 → 把新就绪的位图排进上传队列。
    ///
    /// 返回是否有新位图落地。
    fn pump(&mut self) -> bool {
        self.frame += 1;
        let now = self.frame;
        let mut fresh = false;
        for (i, name) in SOURCES.iter().enumerate() {
            if self.bitmaps[i].is_some() {
                continue;
            }
            let p = vpath(name);
            // 清单还没抓到就不投递：解码闭包收到空字节会失败并被缓存成长期错误。
            if !self.vfs.exists(&p) {
                continue;
            }
            match self.store.load(
                &self.vfs,
                &mut self.runner,
                &p,
                decoder(self.atlas.clone(), now),
            ) {
                Request::Ready(h) => fresh |= self.adopt(i, h),
                Request::Loading(_) => {}
                Request::Failed(e) => panic!("图源 {name} 解码失败: {e}"),
            }
        }
        // 在飞任务的结果靠 `poll_ready` 结清票据；句柄统一从 `get` 领（见下）。
        for (_id, req) in self.store.poll_ready(&mut self.runner) {
            if let Request::Failed(e) = req {
                println!("[sprite_bench] 解码失败（已缓存，需 retry）: {e}");
            }
        }
        for (i, name) in SOURCES.iter().enumerate() {
            if self.bitmaps[i].is_none()
                && let Some(h) = self.store.get(&vpath(name))
            {
                fresh |= self.adopt(i, h);
            }
        }
        fresh
    }

    /// 收下第 `i` 个图源：句柄入列（保持页存活），克隆位图排上传。
    fn adopt(&mut self, i: usize, handle: Handle<Bitmap>) -> bool {
        if self.bitmaps[i].is_some() {
            return false;
        }
        self.uploads.queue(handle.get().clone());
        self.bitmaps[i] = Some(handle);
        true
    }

    /// 已就绪图源 `(epoch, uv 子矩形)`；`draw` 每帧只读这个，不碰句柄。
    fn ready(&self) -> Vec<(u64, [f32; 4])> {
        self.bitmaps
            .iter()
            .filter_map(|b| b.as_ref().map(|h| (h.get().epoch(), h.get().uv_rect())))
            .collect()
    }

    /// 本帧真正画到的图源刷新 LRU 时基。
    fn touch(&self, used: &[(u64, [f32; 4])]) {
        for (epoch, _) in used {
            self.atlas.touch(*epoch, self.frame);
        }
    }
}

/// 解码 + 装箱：跑在 worker 线程（native）上，GPU 上传留给主线程。
fn decoder(atlas: Atlas, now_frame: u64) -> impl FnOnce(Arc<Vec<u8>>) -> Result<Bitmap, String> {
    move |bytes| {
        let data = sisf::media::image::decode_image(&bytes).map_err(|e| e.to_string())?;
        let img = data.first();
        atlas
            .new_bitmap(img.width, img.height, img.pixels.clone(), now_frame)
            .map_err(|e| e.to_string())
    }
}

/// wasm 宿主预取：按清单逐个 `fetch`，成字节后灌进 `WebManifest`（VFS 读取因此恒同步）。
#[cfg(target_arch = "wasm32")]
fn prefetch(manifest: &Arc<WebManifest>) {
    use sisf::app::{js_sys, wasm_bindgen, wasm_bindgen_futures, web_sys};
    for p in manifest.pending() {
        let Some(window) = web_sys::window() else {
            break;
        };
        let m = Arc::clone(manifest);
        let key = p.as_str().to_owned();
        wasm_bindgen_futures::spawn_local(async move {
            let url = format!("assets/{key}");
            let outcome: Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> = async {
                let response =
                    wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(&url)).await?;
                // `array_buffer()` 本身是 `catch` 绑定：返回 `Result<Promise, JsValue>`。
                let body = web_sys::Response::from(response).array_buffer()?;
                Ok(wasm_bindgen_futures::JsFuture::from(body).await?)
            }
            .await;
            match outcome {
                Ok(buffer) => m.insert(&vpath(&key), js_sys::Uint8Array::new(&buffer).to_vec()),
                Err(e) => {
                    web_sys::console::log_1(&format!("预取 {url} 失败: {e:?}").into());
                }
            }
        });
    }
}

/// 帧时间统计：`win_*` 每个报告周期清零，其余累计到收工。
struct Bench {
    sources: Sources,
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
    /// 合批基准（图源齐后算一次）：`用到的页数 × blend 种数`。
    expect_batches: Option<u32>,
    /// 图源齐了之后走过的帧数（断言要用"上一帧"的 stats，所以至少等到第 2 帧）。
    full_frames: u64,
    asserted: bool,
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
                src: (rng.next_u64() % SOURCES.len() as u64) as usize,
            })
            .collect();
        Self {
            sources: Sources::new(),
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
            expect_batches: None,
            full_frames: 0,
            asserted: false,
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

    /// `avg/min/max` + 页数/batch/slot 数（"合并成一次 draw"的直观证据）。
    fn line(&self, tag: &str, avg: f64, min: f64, max: f64) {
        println!(
            "[sprite_bench] {tag} 帧 {} {}x{} sprites={} 图源 {}/{} 页={} batch={} \
             slot={} inst={} 期望 batch={:?} 帧时间 avg={:.2}ms({:.1}fps) min={:.2} \
             max={:.2} 累计掉帧(>{SLOW_MS:.0}ms)={}",
            self.frames,
            self.physical.0,
            self.physical.1,
            self.count,
            self.sources.loaded(),
            SOURCES.len(),
            self.sources.atlas.live_pages(),
            self.stats.map_or(0, |s| s.batches),
            self.stats.map_or(0, |s| s.slots),
            self.stats.map_or(0, |s| s.instances),
            self.expect_batches,
            avg,
            1000.0 / avg,
            min,
            max,
            self.slow,
        );
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

    /// 图源齐了就重算一次合批基准：sprite 列表用到的页数 × 出现的 blend 种数。
    fn recompute_expectation(&mut self) {
        let ready = self.sources.ready();
        let mut pages: Vec<u64> = self
            .sprites
            .iter()
            .map(|s| ready[s.src % ready.len()].0)
            .collect();
        pages.sort_unstable();
        pages.dedup();
        let normal = self.sprites.iter().any(|s| !s.add);
        let add = self.sprites.iter().any(|s| s.add);
        let blends = usize::from(normal) + usize::from(add);
        self.expect_batches = Some((pages.len() * blends) as u32);
    }

    /// 断言合批结果（图源齐 + 上一帧 stats 可信才跑，一次就够）。
    fn assert_batching(&mut self, stats: DrawStats) {
        if self.asserted || self.sources.loaded() != SOURCES.len() || self.full_frames < 2 {
            return;
        }
        let Some(expect) = self.expect_batches else {
            return;
        };
        assert_eq!(stats.instances, self.count, "每个精灵都该产出一个实例");
        assert_eq!(stats.slots, 1, "槽容量已配到 sprite 数，整帧该在一片上");
        assert_eq!(
            stats.batches, expect,
            "batch 数必须等于「用到的页数 × blend 种数」：图源数量不该炸批"
        );
        let pages = self.sources.atlas.live_pages();
        assert!(
            pages <= SOURCES.len(),
            "{} 个小图不该吃掉超过 {} 页（实际 {pages}）",
            SOURCES.len(),
            SOURCES.len()
        );
        self.asserted = true;
        println!(
            "[sprite_bench] 合批断言通过: {} 张图源铺在 {pages} 页图集上，{} 个实例只有 batch={expect}",
            SOURCES.len(),
            stats.instances,
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
        // 首帧间隔含 renderer 异步初始化；图源未齐的帧含 fetch/解码，都不计入统计。
        if self.frames > 0 && self.full_frames > 0 {
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
        // 槽容量配到 sprite 数：整帧一片，batch 数才是纯合批结果（只设一次，避免重建缓冲）。
        if self.count > 0 && renderer.quads_per_slot() != self.count {
            renderer.set_quads_per_slot(self.count);
        }
        // 图集同步点：布局事件（建/毁页）+ 本帧上传，都发生在收集实例之前。
        if self.sources.pump() && self.sources.loaded() == SOURCES.len() {
            self.recompute_expectation();
        }
        let ready = self.sources.ready();
        self.sources.touch(&ready);
        renderer.apply_atlas(
            &self.sources.atlas,
            &mut self.sources.pages,
            &mut self.sources.uploads,
        );
        if ready.len() == SOURCES.len() {
            self.full_frames += 1;
        }
        // `stats()` 是本帧之前最后一次 `end_frame` 的统计（上一帧）。
        self.stats = renderer.stats();
        if let Some(stats) = self.stats {
            self.assert_batching(stats);
        }
        if ready.is_empty() {
            return;
        }
        let (vw, vh) = (VIRTUAL.0 as f32, VIRTUAL.1 as f32);
        let dt = self.dt;
        for (i, s) in self.sprites.iter_mut().enumerate() {
            s.x = (s.x + s.vx * dt).rem_euclid(vw);
            s.y = (s.y + s.vy * dt).rem_euclid(vh);
            s.rot += s.spin * dt;
            let (epoch, uv) = ready[s.src % ready.len()];
            // 偶数下标把宽取负：翻转走 `size` 符号路径（着色器交换 uv 两端）。
            let size = [if i & 1 == 0 { -s.w } else { s.w }, s.h];
            let mut instance = Instance::rect([s.x, s.y], size, s.color, 0);
            instance.uv0 = [uv[0], uv[1]];
            instance.uv1 = [uv[2], uv[3]];
            instance.rot = s.rot;
            renderer.push(
                BatchKey {
                    layer: 0,
                    epoch,
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
        title: "SISF sprite_bench (T4)".to_owned(),
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
        use sisf::app::web_sys;
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
    println!(
        "[sprite_bench] 启动 sprites={sprites} limit={frames} 图源={:?}",
        SOURCES
    );
    let mut app = Bench::new(sprites);
    app.limit = frames;
    if let Err(e) = platform::run(app, config()) {
        panic!("sprite_bench 运行失败: {e}");
    }
}
