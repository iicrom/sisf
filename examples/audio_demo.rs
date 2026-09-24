//! T7 验收示例：四通道混音 + OGG/MP3/WAV 解码 + MIDI(SF2) 起播 + 挂起/恢复。
//!
//! 一条 **12 秒循环的时间线**替你把清单要求的三件事各跑一遍：
//!
//! ```text
//! 0.0s  BGM ← tone_2s.ogg   （元数据 LOOPSTART/LOOPEND，无缝回绕）+ 400ms 淡入
//! 2.0s  SE  ← tone_1s.wav   硬左声相
//! 4.0s  ME  ← tone_1s.mp3   一次性（ME 通道，播完自灭）
//! 6.0s  SE  ← tone_1s.wav   硬右声相
//! 7.0s  BGS ← jingle.mid    有 .sf2 就出声，没有就把 MissingSoundBank 的提示打出来
//! 10.5s BGM 淡出 1.5s        （淡到 0 即停，下一循环再起）
//! ```
//!
//! **为什么不靠按键**：清单写的是 `Space` 播 SE / `M` 播 `.mid`，而键盘事件的归一化
//! （`EngineEvent::Key` + `ActionMap`）是 T9 的第一、三条，本阶段 `EngineEvent` 只有
//! `Lifecycle`。所以这里由时间线驱动**同一段代码**（[`AudioDemo::fire`]），T9 接上
//! `ActionMap` 后把动作名指到这几个函数即可 —— 语义一字不改。
//!
//! 时间线**等资源到货才起步**（wasm 的 `fetch` 在飞），并且挂起时连时间都不走，
//! 所以四条通道的位置就是"混音器被 pump 了多少帧"的直接读数。
//!
//! 观测口（听测 + 看数）：
//! - 屏幕：四条通道各一根进度条（宽度 = 该通道语音的源帧位置），右上红块 = 出现过欠载。
//! - 控制台（native = stdout，wasm = devtools）：每秒一行 `describe()`，含
//!   设备名/采样率/环深度/**underrun 增量**/流重建次数；`sisf-audio` 自己的告警走 stderr。
//! - `cargo run` 起来后会话期间**故意不断**（听测要持续），退出用窗口关闭。
//!
//! 运行：
//! - 桌面：`cargo run -p sisf --example audio_demo --features ogg,mp3,wav,midi -- <秒>`
//!   （`<秒>` 缺省 0 = 一直跑；给个数就能限时自检并打"收工"判决行。
//!   音频来自 `crates/sisf-media/tests/fixtures/`；MIDI 要出声就把自己的 SoundFont
//!   放成 `crates/sisf-media/tests/fixtures/soundfont.sf2`，引擎不附带）
//! - wasm：`cargo xtask web audio_demo` 后访问 `http://127.0.0.1:8080/?ex=audio_demo`
//!   （音频来自 `web/assets/`，`xtask web` 已把解码 feature 一起编进去；`&secs=20` 限时）
//! - 无解码头文件的构建：通道照常起落、`play_bytes` 报 `UnsupportedFormat`，日志看得见

use std::sync::Arc;
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
use sisf::app::wasm_bindgen;
use sisf::app::{App, AppConfig, FrameCtx, ScaleMode, platform};
#[cfg(not(target_arch = "wasm32"))]
use sisf::asset::DirProvider;
use sisf::asset::{Vfs, vfs::path as vpath};
#[cfg(target_arch = "wasm32")]
use sisf::asset::{VfsPath, VfsProvider, WebManifest};
use sisf::audio::AudioBus;
use sisf::audio::mixer::{Channel, PlayParams};
use sisf::event::{EngineEvent, Lifecycle};
use sisf::media::audio::{self, AudioData, AudioError};
use sisf::render::Renderer;

/// 虚拟分辨率：与 `windows` 同尺寸，`Fill` 下 1 虚拟像素 = 1 物理像素。
const VIRTUAL: (u32, u32) = (320, 240);
/// 时间线一循环的长度（秒）。
const CYCLE: f32 = 12.0;
/// 进度条几何。
const BAR_X: f32 = 44.0;
const BAR_Y: f32 = 40.0;
const BAR_W: f32 = 200.0;
const BAR_H: f32 = 12.0;
const BAR_GAP: f32 = 22.0;
/// 状态行报告的间隔（秒）。
const REPORT_EVERY: f32 = 1.0;

/// 四份音频 fixture + 一份**可选**的用户 SoundFont。
const BGM: &str = "tone_2s.ogg";
const SE: &str = "tone_1s.wav";
const ME: &str = "tone_1s.mp3";
const MIDI: &str = "jingle.mid";
const SF2: &str = "soundfont.sf2";

/// wasm 预取清单：一行一个路径，是 `web/assets/manifest.txt` 里本示例要用的那一子集。
/// `soundfont.sf2` 刻意不在里面 —— 引擎不附带 SoundFont，见 `docs/audio.md`。
#[cfg(target_arch = "wasm32")]
const MANIFEST: &str = "tone_2s.ogg\ntone_1s.wav\ntone_1s.mp3\njingle.mid\n";

/// 一次播放请求（时间线的一段，也是 T9 的 `ActionMap` 该绑的东西）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cue {
    Bgm,
    SeLeft,
    SeRight,
    Me,
    Midi,
    BgmOut,
}

/// 时间线：`(循环内秒, 动作)`，必须按时间升序（`advance` 顺序扫描）。
const TIMELINE: [(f32, Cue); 6] = [
    (0.0, Cue::Bgm),
    (2.0, Cue::SeLeft),
    (4.0, Cue::Me),
    (6.0, Cue::SeRight),
    (7.0, Cue::Midi),
    (10.5, Cue::BgmOut),
];

/// 音频内容：VFS 读字节 + **一次性**解码成驻留样本。
///
/// 为什么不每圈 `play_bytes`：`play_bytes` 在**调用线程**整段解码，2s 立体声 OGG 在 debug
/// 构建里要 ~90ms —— 实测足以把 21ms 的输出环抽干，回绕那一下真能听见爆音。正确形状是
/// "读一次、解一次、之后复用 `Arc<AudioData>`"（异步版 = `AssetStore` 后台解码，T4），
/// 本示例照它写。
///
/// wasm 首帧清单还在飞，所以每帧试读直到齐；`SF2` 允许永远读不到（可选）。
struct Sources {
    vfs: Vfs,
    loaded: bool,
    /// 解码失败的原因：记下来就停止重试（"静等资源"是最难查的失败形态）。
    failed: Option<String>,
    bgm: Option<Arc<AudioData>>,
    se: Option<Arc<AudioData>>,
    me: Option<Arc<AudioData>>,
    /// `.mid` 只有字节要驻留 —— 解码器是一路**流**，起播时现场建（fixture 才 76 字节）。
    midi: Vec<u8>,
    sf2: Option<Vec<u8>>,
    /// 上一次尝试的结果（缺哪份文件要说得出来，不能静等）。
    missing: String,
}

impl Sources {
    fn new() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let mut vfs = Vfs::new();
        #[cfg(not(target_arch = "wasm32"))]
        vfs.register(Box::new(DirProvider::new(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../crates/sisf-media/tests/fixtures"),
        )));
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
            loaded: false,
            failed: None,
            bgm: None,
            se: None,
            me: None,
            midi: Vec::new(),
            sf2: None,
            missing: String::from("未尝试"),
        }
    }

    /// 读齐 + 解齐三份驻留 fixture 与 MIDI 字节；没弄全就返回 `false`（下一帧再试，
    /// wasm 的 `fetch` 是异步的）。
    fn pump(&mut self) -> bool {
        if self.loaded || self.failed.is_some() {
            return self.loaded;
        }
        let mut missing: Vec<&str> = Vec::new();
        let mut raw: [Option<Vec<u8>>; 4] = [None, None, None, None];
        for (i, name) in [BGM, SE, ME, MIDI].iter().enumerate() {
            let p = vpath(name);
            match self.vfs.read(&p) {
                Some(b) if !b.is_empty() => raw[i] = Some(b),
                _ => missing.push(*name),
            }
        }
        if !missing.is_empty() {
            self.missing = missing.join(", ");
            return false;
        }
        let mut decoded: Vec<Arc<AudioData>> = Vec::with_capacity(3);
        for (name, bytes) in [BGM, SE, ME].into_iter().zip(raw[..3].iter_mut()) {
            let bytes = bytes.take().expect("上面刚确认读到字节");
            match audio::decode_audio(&bytes) {
                Ok(data) => decoded.push(Arc::new(data)),
                Err(e) => {
                    self.failed = Some(format!("{name} 解码失败: {e}"));
                    return false;
                }
            }
        }
        // SoundFont 可选：读到了就留给 `fire_midi` 建库。
        let p = vpath(SF2);
        if self.vfs.exists(&p) {
            self.sf2 = self.vfs.read(&p);
        }
        self.midi = raw[3].take().expect("上面刚确认读到字节");
        let (bgm, se, me) = (decoded.remove(0), decoded.remove(0), decoded.remove(0));
        platform::log_info(&format!(
            "[audio_demo] 解码就绪: BGM {} 帧 / {:.2}s / 循环 [{}, {}) · SE {} 帧 · ME {} 帧 \
             · MIDI {} B · SoundFont {}",
            bgm.frames(),
            bgm.duration(),
            bgm.loop_start_samples,
            bgm.loop_end_samples,
            se.frames(),
            me.frames(),
            self.midi.len(),
            match &self.sf2 {
                Some(b) => format!("{} B", b.len()),
                None => "未提供".to_owned(),
            }
        ));
        self.bgm = Some(bgm);
        self.se = Some(se);
        self.me = Some(me);
        self.loaded = true;
        self.missing.clear();
        true
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

struct AudioDemo {
    bus: AudioBus,
    src: Sources,
    /// 循环内时间（秒）。资源未齐或挂起时不走。
    t: f32,
    /// 累计时间（秒），驱动报告。
    clock: f32,
    next_report: f32,
    /// 时间线游标（`TIMELINE` 下标）。
    cue: usize,
    frames: u64,
    /// 跑这么多秒就收工（`0` = 一直跑）。桌面取 `argv[1]`，wasm 取 `?secs=`。
    limit: f32,
    finished: bool,
    /// 上一次报告的欠载读数，用来打增量（绝对值在 wasm/native 单位不同，见 `underrun_units`）。
    last_underrun: u64,
    /// 最近一次动作的结果，进报告行（"听不见"必须能说成原因）。
    status: String,
    /// 出现过欠载（画红块；听测时"有没有爆音"要一眼可见）。
    blink: bool,
    /// `feature = "midi"` 时预先建好的音色库（`None` = 没提供 .sf2）。
    #[cfg(feature = "midi")]
    bank: Option<sisf::media::midi::SoundBank>,
}

impl AudioDemo {
    fn new(limit: f32) -> Self {
        let mut bus = AudioBus::open();
        // 主增益压一档：fixture 本身是半幅正弦，四路同开时留点余量，也顺带过一遍
        // `set_master_gain`（真机听测不必被吓一跳）。
        bus.set_master_gain(0.35);
        platform::log_info(&format!("[audio_demo] 输出: {}", bus.describe()));
        Self {
            bus,
            src: Sources::new(),
            t: 0.0,
            clock: 0.0,
            next_report: 0.0,
            cue: 0,
            frames: 0,
            limit,
            finished: false,
            last_underrun: 0,
            status: String::from("等资源"),
            blink: false,
            #[cfg(feature = "midi")]
            bank: None,
        }
    }

    /// 执行一条时间线动作（= T9 之后一个"动作名"该做的事）。
    ///
    /// 样本一律取自 [`Sources`] 里那份**驻留**的 `Arc<AudioData>`：起播因此只有
    /// `Voice::new` 的量级，不会再在播放线程上解一次码。
    fn fire(&mut self, cue: Cue) {
        let r = match cue {
            Cue::Bgm => {
                let data = self.src.bgm.clone().expect("时间线只在样本齐后跑");
                self.bus.play(
                    Channel::Bgm,
                    data,
                    &PlayParams::default().gain(0.8).fade_in(400.0),
                )
            }
            Cue::SeLeft => {
                let data = self.src.se.clone().expect("时间线只在样本齐后跑");
                self.bus.play(
                    Channel::Se,
                    data,
                    &PlayParams::default().gain(0.9).pan(-0.7),
                )
            }
            Cue::SeRight => {
                let data = self.src.se.clone().expect("时间线只在样本齐后跑");
                self.bus
                    .play(Channel::Se, data, &PlayParams::default().gain(0.9).pan(0.7))
            }
            Cue::Me => {
                let data = self.src.me.clone().expect("时间线只在样本齐后跑");
                self.bus
                    .play(Channel::Me, data, &PlayParams::default().gain(0.6))
            }
            Cue::BgmOut => {
                self.bus.fade(Channel::Bgm, 1500.0);
                self.status = format!("{cue:?}: 淡出 1.5s");
                platform::log_info(&format!("[audio_demo] {}", self.status));
                return;
            }
            #[cfg(feature = "midi")]
            Cue::Midi => self.fire_midi(),
            #[cfg(not(feature = "midi"))]
            Cue::Midi => {
                self.status = format!("{cue:?}: 本次构建没开 midi feature");
                platform::log_info(&format!("[audio_demo] {}", self.status));
                return;
            }
        };
        self.status = match r {
            Ok(()) => format!("{cue:?}: ok"),
            Err(e) => format!("{cue:?}: {e}"),
        };
        platform::log_info(&format!(
            "[audio_demo] {} | {}",
            self.status,
            self.bus.describe()
        ));
    }

    /// `.mid` 起播。音色库缺失时 `play_midi` 给 `MissingSoundBank`（提示，不 panic）。
    #[cfg(feature = "midi")]
    fn fire_midi(&mut self) -> Result<(), AudioError> {
        use sisf::media::midi::{MidiLoop, SoundBank};
        if self.bank.is_none() {
            self.bank = self
                .src
                .sf2
                .as_deref()
                .map(SoundBank::from_sf2)
                .transpose()?;
        }
        let info = sisf::media::midi::midi_info(&self.src.midi)?;
        let r = self.bus.play_midi(
            Channel::Bgs,
            &self.src.midi,
            self.bank.as_ref(),
            MidiLoop::Whole,
            0.6,
        );
        match &r {
            Ok(()) => platform::log_info(&format!(
                "[audio_demo] MIDI {} 轨 / {:.2}s 起播，音色库 {} preset",
                info.tracks,
                info.duration_seconds,
                self.bank.as_ref().map_or(0, SoundBank::preset_count)
            )),
            Err(e) => platform::log_info(&format!(
                "[audio_demo] MIDI 播不了（把 {SF2} 放进音频目录即可出声）: {e}"
            )),
        }
        r
    }

    /// 时间线推进：只在资源齐、且未挂起时走（挂起时声音本来就冻住，不该偷跑）。
    fn advance(&mut self, dt: Duration) {
        if self.finished {
            return;
        }
        self.t += dt.as_secs_f32();
        if self.t >= CYCLE {
            self.t -= CYCLE;
            self.cue = 0;
        }
        while self.cue < TIMELINE.len() && self.t >= TIMELINE[self.cue].0 {
            let cue = TIMELINE[self.cue].1;
            self.cue += 1;
            self.fire(cue);
        }
    }

    fn report(&mut self) {
        let underrun = self.bus.underrun_units();
        if underrun > self.last_underrun {
            self.blink = true;
        }
        let line = format!(
            "[audio_demo] t={:.2}s 循环内 cues={}/{} · {} · 语音 {} · underrun+{} · 通道位置 {:?}",
            self.clock,
            self.cue,
            TIMELINE.len(),
            self.status,
            self.bus.mixer().voice_count(),
            underrun - self.last_underrun,
            Channel::ALL.map(|c| self.bus.mixer().position(c).unwrap_or(u64::MAX)),
        );
        // native 侧再加一句硬指标：环被写爆 = 主线程 pump 逻辑错了（设备变更不影响它）。
        #[cfg(all(feature = "native", not(target_arch = "wasm32")))]
        let line = {
            let s = self.bus.stats();
            format!(
                "{line} · 环深 {}f 溢出 {}s 开过 {} 条流",
                s.ring_frames_filled, s.overflow_samples, s.opened_streams
            )
        };
        platform::log_info(&line);
        self.last_underrun = underrun;
    }

    /// 收工判决：混音推进的输出帧数要贴着墙钟（差得远 = pump 节奏错了），欠载要贴着 0。
    fn summary(&self) {
        let m = self.bus.mixer();
        let rate = m.format().sample_rate.max(1) as f64;
        let audio_secs = m.rendered_frames() as f64 / rate;
        platform::log_info(&format!(
            "[audio_demo] 收工: tick {} 帧 / 墙钟 {:.2}s / 混音推进 {:.2}s（{:.0}%）\
             · 欠载读数 {} · {}",
            self.frames,
            self.clock,
            audio_secs,
            100.0 * audio_secs / self.clock.max(0.01) as f64,
            self.bus.underrun_units(),
            self.bus.describe(),
        ));
    }
}

impl App for AudioDemo {
    fn on_event(&mut self, events: &[EngineEvent], _ctx: &mut FrameCtx) {
        for e in events {
            match e {
                // 挂起：混音冻结（位置不动）+ 设备停止取数；恢复接着原处走。
                EngineEvent::Lifecycle(Lifecycle::Suspended) => {
                    self.status = match self.bus.suspend() {
                        Some(err) => format!("挂起（设备侧: {err}）"),
                        None => "挂起：混音与设备都停了".to_owned(),
                    };
                }
                EngineEvent::Lifecycle(Lifecycle::Resumed) => {
                    self.status = match self.bus.resume_all() {
                        Some(err) => format!("恢复（设备侧: {err}）"),
                        None => "恢复".to_owned(),
                    };
                }
                other => self.status = format!("{other:?}"),
            }
            platform::log_info(&format!("[audio_demo] {} | {e:?}", self.status));
        }
    }

    fn tick(&mut self, dt: Duration, ctx: &mut FrameCtx) {
        self.frames += 1;
        if self.frames == 1 {
            platform::log_info(&format!(
                "[audio_demo] 启动 {}x{} 物理 {:?} · 混音输出 {:?}",
                VIRTUAL.0,
                VIRTUAL.1,
                ctx.physical_size,
                self.bus.output_format()
            ));
        }
        if !self.src.pump() {
            self.status = match self.src.failed.clone() {
                Some(f) => f,
                None => format!("等音频: {}", self.src.missing),
            };
            // 资源未齐也要 pump：混音器空转，泵的节奏与设备一致（wasm 靠它把上下文带起来）。
            self.bus.pump();
            return;
        }
        if self.bus.is_suspended() {
            self.bus.pump();
            return;
        }
        self.advance(dt);
        self.bus.pump();
        self.clock += dt.as_secs_f32();
        if self.clock >= self.next_report {
            self.next_report = self.clock + REPORT_EVERY;
            self.report();
        }
        if self.limit > 0.0 && !self.finished && self.clock >= self.limit {
            self.finished = true;
            self.summary();
            // 听测要能限时跑（自动化/回归），但 wasm 没有"退出进程"这回事：留着窗口。
            #[cfg(not(target_arch = "wasm32"))]
            std::process::exit(0);
        }
    }

    fn draw(&mut self, _alpha: f32, renderer: &mut Renderer) {
        // 四条通道：底槽 + 进度（宽度 = 该通道主语音的源帧位置）。
        for (i, ch) in Channel::ALL.iter().enumerate() {
            let y = BAR_Y + i as f32 * BAR_GAP;
            renderer.fill_rect([BAR_X, y, BAR_W, BAR_H], [1.0, 1.0, 1.0, 0.08]);
            let live = self.bus.mixer().is_playing(*ch);
            let pos = self.bus.mixer().position(*ch).unwrap_or(0) as f32;
            if live {
                renderer.fill_rect([BAR_X, y, (pos % BAR_W).max(2.0), BAR_H], CHANNEL_COLOR[i]);
            }
        }
        // 左上角一条走针 = 时间线在动（比看窗口标题靠谱）。
        let x = BAR_X + (self.t / CYCLE) * BAR_W;
        renderer.fill_rect(
            [x, BAR_Y - 14.0, 2.0, BAR_H + 4.0 * 3.0 + 14.0],
            [1.0, 1.0, 1.0, 0.5],
        );
        // 右上：欠载指示（出现过就常亮，配 `underrun+` 增量看）。
        let lamp = if self.blink {
            [1.0, 0.25, 0.2, 1.0]
        } else {
            [0.25, 0.25, 0.3, 1.0]
        };
        renderer.fill_rect([BAR_X + BAR_W + 12.0, BAR_Y - 14.0, 14.0, 14.0], lamp);
        // 底部一条 = 挂起态（黄 = 混音冻结中）。
        if self.bus.is_suspended() {
            renderer.fill_rect([0.0, 0.0, VIRTUAL.0 as f32, 4.0], [1.0, 0.85, 0.2, 1.0]);
        }
    }
}

/// 通道色（从上到下 BGM/BGS/ME/SE）。
const CHANNEL_COLOR: [[f32; 4]; 4] = [
    [0.20, 0.75, 1.00, 1.0],
    [0.35, 0.95, 0.45, 1.0],
    [1.00, 0.75, 0.20, 1.0],
    [1.00, 0.40, 0.85, 1.0],
];

fn config() -> AppConfig {
    AppConfig {
        title: "SISF audio_demo (T7)".to_owned(),
        virtual_size: VIRTUAL,
        scale_mode: ScaleMode::Fill,
        clear: [0.05, 0.06, 0.10, 1.0],
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

/// 跑多久收工（秒）：桌面 `--example audio_demo -- 20`，wasm `?ex=audio_demo&secs=20`。
/// `0` 或缺省 = 一直跑（听测用）。
fn limit_secs() -> f32 {
    #[cfg(target_arch = "wasm32")]
    {
        use sisf::app::web_sys;
        web_sys::window()
            .and_then(|w| w.location().search().ok())
            .unwrap_or_default()
            .split('&')
            .filter_map(|kv| kv.split_once('='))
            .find(|(k, _)| *k == "secs")
            .and_then(|(_, v)| v.parse().ok())
            .unwrap_or(0.0f32)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::args()
            .nth(1)
            .and_then(|a| a.parse().ok())
            .unwrap_or(0.0f32)
    }
}

fn real_main() {
    platform::log_info("[audio_demo] main 已启动");
    if let Err(e) = platform::run(AudioDemo::new(limit_secs()), config()) {
        panic!("audio_demo 运行失败: {e}");
    }
}
