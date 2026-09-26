//! T6 验收示例：RPG 消息窗（半透明底 + 9 段边框 blt + 裁剪滚动文字 + 头像 blt）。
//!
//! RGSS 的四条 `Bitmap` 指令一帧内全走一遍，而且都在**绘制会话**里：
//!
//! ```text
//! begin_draw(窗口位图) → clear_rect → fill_rect(半透明底) → 9 段边框 blt
//!   → draw_text(说话人名) → push_clip → draw_run(滚动正文) → pop_clip
//!   → 头像 blt → text.sync → end_draw
//! ```
//!
//! 主帧再把这块位图 `blt` 上屏：半透明是位图自己的 alpha，背景图在它下面滚，
//! 所以"透不透"是能直接看见的；正文被 `push_clip` 裁在内框里，裁掉的那半行就是
//! clip 通路的肉眼验收。左上角那行状态是给自己看的（用的哪一族、几个 face、
//! 批数/页数），wasm 里开 devtools 反而不如画在屏幕上快。
//!
//! 图源与字体都经 `Vfs` 读：桌面挂两个 `DirProvider`（图 = `sisf-media` 的
//! fixtures，字体 = `sisf-text` 的 Inter），wasm 由宿主按 `web/assets/` 清单 `fetch`
//! 灌进 `WebManifest`。字体到货前只用系统字体，到货后换一次 —— 于是
//! `FontBundle::from_bytes` 的"自带字体优先"在 demo 里真跑过。刻意不走 `AssetStore`
//! 后台解码：那是 T4/`sprite_bench` 的验收对象，这里三张小图主线程解码就够。
//!
//! 运行：
//! - 桌面：`cargo run --release -p sisf --example windows`
//! - wasm：`cargo xtask web windows` 后访问 `http://127.0.0.1:8080/?ex=windows&wgl`
//!   （wasm 没有系统字体：拉丁来自自带 Inter，中文来自 `fetch_cjk` 运行时从 CDN 取的
//!   那一份 Noto —— 8MB 不入库，见 `docs/text.md` 平台矩阵。左上角状态行的 `cjk …` 就是
//!   它的到货结论。debug 构建下把窗口调小些即可）

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
use sisf::app::wasm_bindgen;
use sisf::app::{App, AppConfig, FrameCtx, ScaleMode, platform};
#[cfg(not(target_arch = "wasm32"))]
use sisf::asset::DirProvider;
use sisf::asset::{Vfs, vfs::path as vpath};
#[cfg(target_arch = "wasm32")]
use sisf::asset::{VfsPath, VfsProvider, WebManifest};
use sisf::event::EngineEvent;
use sisf::render::{Atlas, AtlasPages, Bitmap, DrawStats, Renderer, Source, UploadRing};
use sisf::text::{Align, FontBundle, TextEngine, TextStyle};

/// 虚拟分辨率：RGSS classic 的 320×240，`Fill` 下与物理像素 1:1。
const VIRTUAL: (u32, u32) = (320, 240);
/// 窗口位图尺寸（RT 独占一页）。`288-2*16 = 256 = 8×32`、`96-2*16 = 64 = 4×16`，
/// 正好被边框平铺整除，平铺不留半块。
const WIN: (u32, u32) = (288, 96);
/// 窗口左上角（虚拟像素）。
const WIN_AT: [f32; 2] = [16.0, 128.0];
/// 九宫格的角块边长；上下边块宽 = 皮肤中列宽。
const CORNER: u32 = 16;
/// 半透明窗底（RPG Maker 的窗色偏蓝黑）。
const WINDOW_BG: [f32; 4] = [0.06, 0.09, 0.2, 0.72];
/// 文字/分隔线的白。
const INK: [f32; 4] = [0.94, 0.96, 1.0, 1.0];
/// 正文裁剪盒：头像（`[24,40]` 的 32×32）右边到内框右边。
const BODY: [f32; 4] = [72.0, 40.0, 200.0, 44.0];
/// 说话人名那一行。
const NAME: [f32; 4] = [72.0, 18.0, 200.0, 18.0];

/// 图源：三种魔数各一张（皮肤 PNG / 头像 WebP / 背景 JPG）。
const IMAGES: [&str; 3] = ["img_80x60.jpg", "img_64x48.png", "img_40x40.webp"];
const SKIN: usize = 1;
const FACE: usize = 2;
/// 自带字体（Inter，OFL）。桌面与 wasm 同一个 VFS 路径。
const FONT: &str = "Inter-Regular.ttf";
/// wasm 预取清单：一行一个路径，内容同 `web/assets/manifest.txt`。
#[cfg(target_arch = "wasm32")]
const MANIFEST: &str = "img_80x60.jpg\nimg_64x48.png\nimg_40x40.webp\nInter-Regular.ttf\n";
/// 中文轮廓：与 `examples/dialog_demo.rs` 的 `CJK` 同一套策略 —— 同一个 CDN、同一个
/// SHA、同样的"OTF 而不是 woff2"、同样**不入仓**（8MB）。为什么非要运行期取一次、
/// 以及四种到货状态为什么少一种都不行，写在那边的 `Cjk` 上；这里只留最短的版本。
#[cfg(target_arch = "wasm32")]
const CJK: &str = "NotoSansSC-Regular.otf";
#[cfg(target_arch = "wasm32")]
const CJK_URL: &str = "https://cdn.jsdelivr.net/gh/notofonts/noto-cjk@f8d157532fbfaeda587e826d4cd5b21a49186f7c/Sans/SubsetOTF/SC/NotoSansSC-Regular.otf";

/// 滚动正文：中英混排 + 硬换行 + 一条长拉丁句（靠 `wrap_width` 折行）。
const LINES: &str = "勇者艾达与精灵们踏进了遗迹第三层。\nThe stone door reads: ENGINE 1234567890 \
— keep walking, the map is drawn at 320x240.\n风从右边吹过来。";

/// CDN 那份 CJK 的到货状态。为什么不是一个"读得到字节就算到"：`load_font` 等的是
/// **这件事结束了** —— 一份永远不来的请求不能把本来能显示的拉丁一起拖死，所以"结束了
/// 但没有"必须与"还在飞"分得开。四种结局都进状态行（方块不解释自己）。
#[derive(Clone)]
struct Cjk(Arc<AtomicU8>);

const CJK_PENDING: u8 = 0;
/// 桌面：这一趟 fetch 根本不存在，中文由系统字体兜（`docs/text.md` 平台矩阵）。
const CJK_SYSTEM: u8 = 1;
/// 字节已在清单里，等着注册。
const CJK_ARRIVED: u8 = 2;
/// 拿不到：请求挂了、响应是空的、或者那字节解不出 face。中文会是方块。
const CJK_NONE: u8 = 3;

impl Cjk {
    fn new() -> Self {
        #[cfg(target_arch = "wasm32")]
        let start = CJK_PENDING;
        #[cfg(not(target_arch = "wasm32"))]
        let start = CJK_SYSTEM;
        Self(Arc::new(AtomicU8::new(start)))
    }

    fn state(&self) -> u8 {
        self.0.load(Ordering::Relaxed)
    }

    /// 只有 wasm 会改写它（桌面的 `CJK_SYSTEM` 在 `new` 里一次定形）。
    #[cfg(target_arch = "wasm32")]
    fn set(&self, s: u8) {
        self.0.store(s, Ordering::Relaxed);
    }

    /// 状态行里 `cjk …` 那一段。
    fn label(&self) -> &'static str {
        match self.state() {
            CJK_PENDING => "pending",
            CJK_SYSTEM => "system",
            CJK_ARRIVED => "ok",
            CJK_NONE => "MISSING",
            _ => "?",
        }
    }
}

/// 图源装载状态：VFS 读取 + 图集装箱 + 上传队列。
struct Assets {
    vfs: Vfs,
    /// CDN 那份 CJK 到哪一步了（`load_font` 等的是"这件事结束了"，见 `dialog_demo`）。
    cjk: Cjk,
    atlas: Atlas,
    pages: AtlasPages,
    uploads: UploadRing,
    /// 与 `IMAGES` 同下标；`None` = 还没读到字节（wasm 首帧清单在飞）。
    ///
    /// 必须**持有** `Bitmap`：句柄析构 = 页退役（`PageRetired`），下一帧 `apply_atlas`
    /// 就把这张页的纹理删了。`Source` 每次现算，不单独存。
    held: Vec<Option<Bitmap>>,
    /// 装箱时基（LRU 用），每帧递增一次。
    frame: u64,
}

impl Assets {
    fn new() -> Self {
        let cjk = Cjk::new();
        #[cfg(not(target_arch = "wasm32"))]
        let mut vfs = Vfs::new();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
            vfs.register(Box::new(DirProvider::new(
                manifest_dir.join("../../crates/sisf-media/tests/fixtures"),
            )));
            vfs.register(Box::new(DirProvider::new(
                manifest_dir.join("../../crates/sisf-text/tests/fixtures"),
            )));
        }
        #[cfg(target_arch = "wasm32")]
        let vfs = {
            let manifest = Arc::new(WebManifest::new(MANIFEST));
            prefetch(&manifest);
            // 中文那份不排在清单里：清单只从本站 `assets/` 抓，而 8MB 的字体不入库。
            fetch_cjk(&manifest, &cjk);
            let mut vfs = Vfs::new();
            vfs.register(Box::new(SharedManifest(manifest)));
            vfs
        };
        Self {
            vfs,
            cjk,
            atlas: Atlas::with_limits(256, 6),
            pages: AtlasPages::new(),
            uploads: UploadRing::new(),
            held: (0..IMAGES.len()).map(|_| None).collect(),
            frame: 0,
        }
    }

    /// 把还没装上的图读出来、解码进图集、排进上传队列（一次性的三张小图，主线程解）。
    fn pump(&mut self) {
        self.frame += 1;
        for (i, name) in IMAGES.iter().enumerate() {
            if self.held[i].is_some() {
                continue;
            }
            let p = vpath(name);
            // 清单还没抓到就不读：`read` 返回空字节会让解码以"格式不认识"收场，
            // 排查起来比"这一帧还没图"难得多。
            if !self.vfs.exists(&p) {
                continue;
            }
            let bytes = self.vfs.read(&p).expect("exists 刚刚通过");
            let data = sisf::media::image::decode_image(&bytes)
                .unwrap_or_else(|e| panic!("图源 {name} 解码失败: {e}"));
            let img = data.first();
            let bitmap = self
                .atlas
                .new_bitmap(img.width, img.height, img.pixels.clone(), self.frame)
                .unwrap_or_else(|e| panic!("图源 {name} 装箱失败: {e}"));
            self.uploads.queue(bitmap.clone());
            platform::log_info(&format!(
                "[windows] {name} {}x{} → 页 {}（图集 {} 页）",
                img.width,
                img.height,
                bitmap.epoch(),
                self.atlas.live_pages(),
            ));
            self.held[i] = Some(bitmap);
        }
    }

    fn ready(&self) -> bool {
        self.held.iter().all(Option::is_some)
    }

    fn source(&self, i: usize) -> Option<Source> {
        self.held[i].as_ref().map(Source::from)
    }
}

/// wasm 下 `Vfs` 要跟着 `App: 'static` 活，预取任务也要摸同一份清单：
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

/// wasm 宿主预取：按清单逐个 `fetch`，成字节后灌进 `WebManifest`（VFS 读取因此恒同步）。
#[cfg(target_arch = "wasm32")]
fn prefetch(manifest: &Arc<WebManifest>) {
    use sisf::app::{js_sys, wasm_bindgen_futures, web_sys};
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
                let body = web_sys::Response::from(response).array_buffer()?;
                wasm_bindgen_futures::JsFuture::from(body).await
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

/// wasm：从 CDN 取 [`CJK`] 那份中文字体灌进清单（`insert` 收清单外的键）。两头都要说话：
/// 到货说字节数，取不到说原因并写下 [`CJK_NONE`] —— 少了后者，`load_font` 会一直等一份
/// 永远不来的字体，把本来能显示的拉丁一起拖死。
#[cfg(target_arch = "wasm32")]
fn fetch_cjk(manifest: &Arc<WebManifest>, cjk: &Cjk) {
    use sisf::app::{js_sys, wasm_bindgen_futures, web_sys};
    let Some(window) = web_sys::window() else {
        return;
    };
    let m = Arc::clone(manifest);
    let c = cjk.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let outcome: Result<Vec<u8>, String> = async {
            let response = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(CJK_URL))
                .await
                .map_err(|e| format!("{e:?}"))?;
            let response = web_sys::Response::from(response);
            // 404 页也会 resolve 出一个非空响应，而它解不出 face —— 那时候话说成"解析
            // 不出任何 face"，排查的人会去查字体格式，而真正的错是 URL。
            if !response.ok() {
                return Err(format!("HTTP {}", response.status()));
            }
            let body = response.array_buffer().map_err(|e| format!("{e:?}"))?;
            let buffer = wasm_bindgen_futures::JsFuture::from(body)
                .await
                .map_err(|e| format!("{e:?}"))?;
            Ok(js_sys::Uint8Array::new(&buffer).to_vec())
        }
        .await;
        match outcome {
            Ok(bytes) if !bytes.is_empty() => {
                platform::log_info(&format!("[windows] CJK 到货 {} 字节", bytes.len()));
                m.insert(&vpath(CJK), bytes);
                c.set(CJK_ARRIVED);
            }
            Ok(_) => {
                web_sys::console::log_1(&format!("{CJK_URL} 响应是空的").into());
                c.set(CJK_NONE);
            }
            Err(why) => {
                web_sys::console::log_1(&format!("取 {CJK_URL} 失败: {why}").into());
                c.set(CJK_NONE);
            }
        }
    });
}

struct WindowDemo {
    assets: Assets,
    text: TextEngine,
    /// 窗口位图（RT）：会话画它，主帧 blt 它。存 `Bitmap` 而不是 `Source` 是有原因的
    /// —— 句柄析构即页退役，那张位图会在 `apply_atlas` 里被删掉，会话就没有目标了。
    win: Bitmap,
    /// 自带字体是否已经换上（`false` = 仍用启动时的系统字体）。
    bundled: bool,
    /// 累计时间（秒），驱动背景滚动与正文滚动。
    t: f32,
    frames: u64,
    /// 上一帧主帧的批统计 + 上一次会话的统计，画进状态行。
    stats: Option<DrawStats>,
    paint: Option<DrawStats>,
}

impl WindowDemo {
    fn new() -> Self {
        let assets = Assets::new();
        // RT 必须长在**同一本图集**上：`apply_atlas` 只认 `assets.atlas` 报出来的页，
        // 换一本图集造的页在渲染器里没有视图，`begin_draw` 直接 `Unregistered`。
        let win = assets
            .atlas
            .new_target(WIN.0, WIN.1, 0)
            .expect("窗口位图该占得下一页");
        // 启动时先只用系统字体：wasm 上它是空的（`face_count()==0`），桌面能兜住中文。
        // 自带字体到货后在 `draw` 里换一次，见 `bundled`。
        Self {
            assets,
            text: TextEngine::new(FontBundle::system()),
            win,
            bundled: false,
            t: 0.0,
            frames: 0,
            stats: None,
            paint: None,
        }
    }

    /// 自带字体到货则换上去（只换一次）。
    ///
    /// 换之前一个字都没画过（见 `draw` 的门控），所以覆盖率图集里不会有旧 face 的
    /// 缓存项 —— 字体族中途换脸是会串字的。也正因如此，等的不是"Inter 到了"而是
    /// "**CJK 这件事结束了**"：先装 Inter 再补 CJK 就是中途换脸。
    fn load_font(&mut self) {
        if self.bundled || self.assets.cjk.state() == CJK_PENDING {
            return;
        }
        let Some(inter) = self.assets.vfs.read(&vpath(FONT)) else {
            return;
        };
        // 注册顺序 = 桌面的顺序：**CJK 先、Inter 最后**，于是拉丁出自 Inter（与桌面
        // 同源），中文靠 per-glyph 回退落到那份 Noto 上（`docs/text.md` 平台矩阵）。
        let mut bundle = FontBundle::system();
        // 只读一次：这份字节有 8MB，而 `Vfs::read` 是拷贝出来的。
        #[cfg(target_arch = "wasm32")]
        if self.assets.cjk.state() == CJK_ARRIVED
            && let Some(bytes) = self.assets.vfs.read(&vpath(CJK))
            && let Err(why) = bundle.register_bytes(&bytes)
        {
            self.assets.cjk.set(CJK_NONE);
            platform::log_info(&format!("[windows] CJK 用不了: {why}"));
        }
        if let Err(why) = bundle.register_bytes(&inter) {
            platform::log_info(&format!("[windows] 自带字体用不了: {why}"));
        }
        platform::log_info(&format!(
            "[windows] 自带字体: {:?}（{}，face {}）",
            bundle.family(),
            self.assets.cjk.label(),
            bundle.face_count()
        ));
        *self.text.bundle_mut() = bundle;
        self.bundled = true;
    }

    /// 往窗口位图里画一整帧：清屏 → 半透明底 → 九宫格 → 名字 → 裁剪正文 → 头像。
    fn draw_window(&mut self, renderer: &mut Renderer) {
        let (Some(skin), Some(face)) = (self.assets.source(SKIN), self.assets.source(FACE)) else {
            return;
        };
        let win = Source::from(&self.win);
        if let Err(e) = renderer.begin_draw(&win) {
            panic!("开窗口位图会话失败: {e}");
        }
        // 位图是**持久画布**（目标页只在第一次画之前清一次），所以滚动文字必须自己
        // clear_rect，否则上一帧的字会糊在底下。
        renderer.clear_rect([0, 0, WIN.0, WIN.1]);
        renderer.fill_rect([0.0, 0.0, WIN.0 as f32, WIN.1 as f32], WINDOW_BG);
        let (sw, sh) = skin.size();
        draw_border(renderer, &skin, sw, sh);
        // 分隔线：又是一次 `fill_rect`，1px 高、压暗的白。
        renderer.fill_rect([NAME[0], 36.0, NAME[2], 1.0], [0.6, 0.7, 1.0, 0.35]);
        // RGSS 的 `draw_text(rect, text, align)`：会话里 ⇒ 画进这张位图。
        renderer.push_clip(0, NAME);
        self.text.draw_text(
            renderer,
            NAME,
            "艾达 · Ada",
            &TextStyle::new(14.0).aligned(Align::Center),
            INK,
        );
        renderer.pop_clip();
        // 正文：按内框宽度折行，整块从盒下沿滚进、上沿滚出 —— 裁掉的那半行就是
        // `push_clip` 的证据。
        let style = TextStyle::new(16.0).with_wrap(BODY[2]);
        let run = self.text.layout(LINES, &style);
        let span = run.height + BODY[3];
        let scroll = (self.t * 26.0) % span;
        renderer.push_clip(0, BODY);
        self.text.draw_run(
            renderer,
            &run,
            [BODY[0], BODY[1] + BODY[3] - scroll],
            INK,
            0,
            0,
        );
        assert_eq!(renderer.clip_depth(), 1, "pop_clip 之后必须只剩根裁剪");
        renderer.pop_clip();
        // 头像：源子矩形往里收 4px，顺手把 `blt` 的 `src_rect` 裁剪走一遍。
        renderer.blt([24.0, 40.0], &face, [4, 4, 32, 32], 255);
        // 会话内新光栅化的字形页要在 pass 记录之前注册 —— 即 `end_draw` 之前。
        self.text.sync(renderer);
        match renderer.end_draw() {
            Ok(stats) => self.paint = Some(stats),
            Err(e) => panic!("收窗口位图会话失败: {e}"),
        }
    }

    /// 主帧：滚动的背景 + 窗口位图上屏 + 左上角状态行。
    fn draw_scene(&mut self, renderer: &mut Renderer) {
        if let Some(tile) = self.assets.source(0) {
            let (tw, th) = tile.size();
            let (ox, oy) = ((self.t * 14.0) % tw as f32, (self.t * 8.0) % th as f32);
            let mut y = -(th as f32) + oy;
            while y < VIRTUAL.1 as f32 {
                let mut x = -(tw as f32) + ox;
                while x < VIRTUAL.0 as f32 {
                    renderer.blt([x, y], &tile, [0, 0, tw, th], 255);
                    x += tw as f32;
                }
                y += th as f32;
            }
        }
        if self.assets.ready() {
            // 位图整页贴回屏幕。窗底本来就只有 0.72 alpha，这里再压一档，`blt` 的
            // opacity 参数也就过了一遍。
            let win = Source::from(&self.win);
            renderer.blt(WIN_AT, &win, [0, 0, WIN.0, WIN.1], 240);
        }
        if self.bundled || self.assets.ready() {
            self.draw_status(renderer);
        }
    }

    /// 状态行：会话外 `draw_text` ⇒ 画进当前帧目标。wasm 上这就是"自检报告"。
    fn draw_status(&mut self, renderer: &mut Renderer) {
        let bundle = self.text.bundle();
        let line = format!(
            "f{} · font {:?}({}) cjk {} · win {} + main {} batches · {} pages",
            self.frames,
            bundle.family().unwrap_or("system"),
            bundle.face_count(),
            self.assets.cjk.label(),
            self.paint.map_or(0, |s| s.batches),
            self.stats.map_or(0, |s| s.batches),
            self.assets.atlas.live_pages(),
        );
        renderer.push_clip(0, [0.0, 0.0, VIRTUAL.0 as f32, 18.0]);
        self.text.draw_text(
            renderer,
            [4.0, 2.0, 312.0, 16.0],
            &line,
            &TextStyle::new(12.0),
            [1.0, 1.0, 1.0, 0.9],
        );
        renderer.pop_clip();
    }
}

/// RPG Maker `window.png` 的九宫格。
///
/// `blt` **不缩放**（缩放是 `stretch_blt`，T8 的活），所以四条边只能平铺：上下按皮肤
/// 中列宽平铺、左右按角块边长平铺，最后一块用 `src_rect` 截短（越界部分由 `blt` 自己
/// 夹回源内）。皮肤子矩形全在源图里，不会采到隔壁子图。
fn draw_border(renderer: &mut Renderer, skin: &Source, sw: u32, sh: u32) {
    let (w, h) = (WIN.0, WIN.1);
    let c = CORNER;
    let (mid_w, right_x, bottom_y) = (sw - 2 * c, sw - c, sh - c);
    renderer.blt([0.0, 0.0], skin, [0, 0, c, c], 255);
    renderer.blt([(w - c) as f32, 0.0], skin, [right_x, 0, c, c], 255);
    renderer.blt([0.0, (h - c) as f32], skin, [0, bottom_y, c, c], 255);
    renderer.blt(
        [(w - c) as f32, (h - c) as f32],
        skin,
        [right_x, bottom_y, c, c],
        255,
    );
    let mut x = c;
    while x < w - c {
        let tw = mid_w.min(w - c - x);
        renderer.blt([x as f32, 0.0], skin, [c, 0, tw, c], 255);
        renderer.blt([x as f32, (h - c) as f32], skin, [c, bottom_y, tw, c], 255);
        x += mid_w;
    }
    let mut y = c;
    while y < h - c {
        let th = c.min(h - c - y);
        renderer.blt([0.0, y as f32], skin, [0, c, c, th], 255);
        renderer.blt([(w - c) as f32, y as f32], skin, [right_x, c, c, th], 255);
        y += c;
    }
}

impl App for WindowDemo {
    fn on_event(&mut self, events: &[EngineEvent], _ctx: &mut FrameCtx) {
        for e in events {
            platform::log_info(&format!("[windows] {e:?}"));
        }
    }

    fn tick(&mut self, dt: Duration, ctx: &mut FrameCtx) {
        self.t += dt.as_secs_f32();
        self.frames += 1;
        // 文字光栅档位跟着 viewport 倍率取（T8 收掉的那条 T6 遗留缺陷）：非整数倍放大
        // 时靠它把覆盖率位图细化一档，而不是拉伸 1 虚拟像素的纹素。重复赋同值是 no-op，
        // 只有跨档才多花页容量。
        self.text.set_raster_scale(ctx.raster_scale());
        if self.frames == 1 {
            platform::log_info(&format!(
                "[windows] 启动 {}x{} 物理 {:?} scale {:.2} 文字档位 {:.0}x",
                VIRTUAL.0,
                VIRTUAL.1,
                ctx.physical_size,
                ctx.scale,
                self.text.raster_scale()
            ));
        }
    }

    fn draw(&mut self, _alpha: f32, renderer: &mut Renderer) {
        self.assets.pump();
        self.load_font();
        // 图集同步点：建/毁页 + 本帧上传，都要在收集实例之前。
        renderer.apply_atlas(
            &self.assets.atlas,
            &mut self.assets.pages,
            &mut self.assets.uploads,
        );
        // 字体没到货就不画字：wasm 上这会画出一屏空白，状态行也就没有意义了。
        if self.bundled {
            self.draw_window(renderer);
        }
        self.draw_scene(renderer);
        // `stats()` 是本帧之前最后一次 `end_frame` 的统计（上一帧）。
        self.stats = renderer.stats();
        // 主帧里新长出来的字形页，赶在 runner 收帧（`end_frame`）之前注册。
        self.text.sync(renderer);
    }
}

fn config() -> AppConfig {
    AppConfig {
        title: "SISF windows (T6)".to_owned(),
        virtual_size: VIRTUAL,
        scale_mode: ScaleMode::Fit,
        clear: [0.05, 0.06, 0.1, 1.0],
        ..Default::default()
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
    platform::log_info("[windows] main 已启动");
    if let Err(e) = platform::run(WindowDemo::new(), config()) {
        panic!("windows 运行失败: {e}");
    }
}
