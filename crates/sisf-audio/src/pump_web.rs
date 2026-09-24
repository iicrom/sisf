//! wasm 输出 pump：AudioWorklet + ScriptProcessor 回退（feature `web`）。
//!
//! JS 胶水是 `crates/sisf-audio/src/audio_pump.js`，由 wasm-bindgen 的
//! `module = "/src/audio_pump.js"` 在 `--target web` 下**拷成 `web/pkg/snippets/…/audio_pump.js`**
//! 并被生成的 `pkg/<例>.js` import（不是内联进 pkg —— 所以静态服务必须连 `snippets/` 一起给，
//! `cargo xtask web` 服务整个 `web/` 正好覆盖）。worklet 源码则是内联 Blob（清单里写的
//! `web/audio_worklet.js` 就是这么落地的，理由见胶水文件头）。
//!
//! ## 时钟在浏览器那边
//!
//! 混音器只认帧数，而帧数由 `AudioContext.sampleRate` 决定 —— 它可能不是 44.1k（移动端浏览器
//! 常见 48k，某些 headset 给 16k）。所以 `pump()` 第一件事就是把混音器格式对齐到上下文速率，
//! 第二件事是**问 JS 还差多少帧**（`sisfAudioFramesNeeded`），只渲那个量：
//! JS 侧的缓冲深度就是延迟，"渲满"和"渲够"是两件事。
//!
//! ## 为什么 worklet 不是字面意义的"从 WASM 拉"
//!
//! `AudioWorkletProcessor.process` 跑在独立的音频线程，浏览器不提供从那里同步回调主线程 WASM
//! 的机制（要做到得靠 SharedArrayBuffer + Atomics，而那需要 COOP/COEP 跨源隔离和
//! `+atomics,+mutable-globals` 编译目标 —— 本仓库的 wasm 构建两样都没开）。于是：
//! - **worklet 通路 = 需求上报 + 主线程按需求渲染**（JS 每次回报它消耗了多少帧，Rust 才决定
//!   渲几块），数据靠 `postMessage` 拷贝过去；
//! - **ScriptProcessor 通路 = 字面意义的同步 pull**（它的回调在主线程，直接读 JS 环）。
//!
//! 自动解锁（首次手势 `resume`）与 `visibilitychange` 挂起都在 JS 里做：`sisf-app` 的
//! `page_hidden`/`log_info` 是 crate 私有的，而且 JS 本来就比 Rust 更早知道这两件事。

use wasm_bindgen::prelude::*;

use sisf_media::audio::AudioFormat;

use crate::mixer::Mixer;

/// 拿不到 `AudioContext` 时的名义采样率（混音照常推进，样本丢弃）。
pub const NOMINAL_RATE: u32 = 48_000;
/// 一次 `pump()` 渲染的块长上限（帧）。
pub const BLOCK_FRAMES: usize = 256;
/// 一次 `pump()` 最多渲染几块（追赶上限）。
/// 与 JS 侧 `RING_FRAMES / BLOCK_FRAMES` 对齐：JS 报的需求已经把"这一帧会被播掉的量"算进去
/// （见 `audio_pump.js` 的 `sisfAudioFramesNeeded`），这里若只够 1024 帧就会在 ~47fps 以下
/// 结构性欠载。
pub const MAX_BLOCKS: usize = 16;

/// `sisfAudioStats()` 定长表的槽位（与 `audio_pump.js` 的 `S_*` 常量一一对应）。
const S_RATE: u32 = 0;
const S_BUFFERED: u32 = 1;
const S_UNDERRUN: u32 = 2;
const S_OVERFLOW: u32 = 3;
const S_BLOCKS: u32 = 4;
const S_QUANTA: u32 = 5;
const S_BASE_LATENCY: u32 = 6;
const S_OUTPUT_LATENCY: u32 = 7;
const S_TARGET: u32 = 8;
const S_TOTAL_LATENCY: u32 = 9;

#[wasm_bindgen(module = "/src/audio_pump.js")]
extern "C" {
    #[wasm_bindgen(js_name = "sisfAudioCreate")]
    fn js_create() -> String;
    #[wasm_bindgen(js_name = "sisfAudioKind")]
    fn js_kind() -> String;
    #[wasm_bindgen(js_name = "sisfAudioSampleRate")]
    fn js_sample_rate() -> f64;
    #[wasm_bindgen(js_name = "sisfAudioFramesNeeded")]
    fn js_frames_needed() -> f64;
    #[wasm_bindgen(js_name = "sisfAudioPush")]
    fn js_push(chunk: &[f32]) -> f64;
    #[wasm_bindgen(js_name = "sisfAudioSetPaused")]
    fn js_set_paused(paused: bool) -> String;
    #[wasm_bindgen(js_name = "sisfAudioFlush")]
    fn js_flush();
    #[wasm_bindgen(js_name = "sisfAudioStats")]
    fn js_stats() -> Vec<f64>;
    #[wasm_bindgen(js_name = "sisfAudioState")]
    fn js_state() -> String;
}

/// wasm 侧的设备状态（`audio_pump.js` 的定长表 + 一行文字状态）。
#[derive(Clone, Debug, Default)]
pub struct PumpStats {
    /// `'worklet'` / `'script-processor'` / `''`（还没创建上下文）。
    pub kind: String,
    pub sample_rate: u32,
    /// JS 侧已缓冲的帧数（= 排队延迟的主要构成）。
    pub buffered_frames: f64,
    /// 目标缓冲深度（帧），来自 JS。
    pub target_frames: f64,
    pub underrun_frames: f64,
    pub overflow_samples: f64,
    pub blocks_pushed: f64,
    pub process_quanta: f64,
    pub base_latency_ms: f64,
    pub output_latency_ms: f64,
    /// `baseLatency + outputLatency + buffered/rate`。
    pub total_latency_ms: f64,
    /// `AudioContext.state` 之类的一行文字（`state=running kind=worklet …`）。
    pub state: String,
    /// Rust 侧累计被拒的帧（JS 环满）。
    pub rejected_frames: u64,
}

/// wasm 输出 pump。见模块文档。
pub struct Pump {
    scratch: Vec<f32>,
    created: bool,
    headless: bool,
    rate: u32,
    kind: String,
    blocks: u64,
    rejected_frames: u64,
    output_error: Option<String>,
}

impl Default for Pump {
    fn default() -> Self {
        Self::new()
    }
}

impl Pump {
    /// 第一次 [`Pump::pump`] 时才创建 `AudioContext`（构造期可能还没有用户手势）。
    pub fn new() -> Self {
        Self::build(false)
    }

    /// 完全不碰 WebAudio：混音照常推进，样本丢弃。给单元测试与"只要逻辑"的构建。
    pub fn headless() -> Self {
        Self::build(true)
    }

    fn build(headless: bool) -> Self {
        Self {
            scratch: Vec::new(),
            created: false,
            headless,
            rate: 0,
            kind: String::new(),
            blocks: 0,
            rejected_frames: 0,
            output_error: None,
        }
    }

    /// 懒创建：`sisfAudioCreate` 返回选定的通路名（`worklet(pending)` 也算成功）。
    fn ensure_created(&mut self) {
        if self.headless || self.created {
            return;
        }
        self.created = true;
        self.kind = js_create();
        self.rate = js_sample_rate() as u32;
        if self.kind.is_empty() {
            self.output_error = Some(js_state());
        }
    }

    pub fn format(&self) -> AudioFormat {
        AudioFormat::stereo(if self.rate > 0 {
            self.rate
        } else {
            NOMINAL_RATE
        })
    }

    /// 问 JS 还差多少帧 → 渲那么多次（每块至多 [`BLOCK_FRAMES`] 帧）。
    ///
    /// 返回 `true` = 这次真的渲染过。
    pub fn pump(&mut self, mixer: &mut Mixer) -> bool {
        self.ensure_created();
        let rate = self.format().sample_rate;
        if mixer.format().sample_rate != rate {
            mixer.set_format(AudioFormat::stereo(rate));
        }
        if self.headless || self.kind.is_empty() {
            // 没有上下文可喂：仍渲一块，保住混音时钟（fade 到点、SE 回收都靠它）。
            self.scratch.resize(BLOCK_FRAMES * 2, 0.0);
            let _ = mixer.render(&mut self.scratch[..BLOCK_FRAMES * 2]);
            return true;
        }
        // 上下文速率可能在解锁后才定下来（有些浏览器创建时报 0）。
        let live = js_sample_rate() as u32;
        if live > 0 && live != self.rate {
            self.rate = live;
            mixer.set_format(AudioFormat::stereo(live));
        }
        let mut rendered = false;
        for _ in 0..MAX_BLOCKS {
            let needed = js_frames_needed() as usize;
            let frames = needed.min(BLOCK_FRAMES);
            if frames == 0 {
                break;
            }
            self.scratch.resize(frames * 2, 0.0);
            let _ = mixer.render(&mut self.scratch[..frames * 2]);
            // `&[f32]` 进 import 参数时 wasm-bindgen 给 JS 的是**一块指向 wasm 内存的
            // Float32Array 视图**（零拷贝）。它在这次调用内有效的理由是：胶水只做 JS 侧分配
            // 与 `postMessage`（结构化克隆在调用内同步取快照），从不回调 Rust，
            // 所以 wasm 内存不会在这次调用里增长 —— 一旦哪天胶水要回调 Rust，这里必须改成先
            // `Float32Array::from(slice)` 拷贝一份。
            let got = js_push(&self.scratch[..frames * 2]) as u64;
            self.blocks += 1;
            rendered = true;
            if got < frames as u64 {
                self.rejected_frames += frames as u64 - got;
                break;
            }
        }
        rendered
    }

    /// 冻结/恢复硬件输出（引擎主动挂起）。挂起时顺手清掉 JS 侧的存货，
    /// 否则恢复瞬间会先把陈旧缓冲倒出来。
    pub fn set_output_active(&mut self, active: bool) -> Option<String> {
        if self.headless {
            return None;
        }
        self.ensure_created();
        if !active {
            js_flush();
        }
        let msg = js_set_paused(!active);
        self.output_error = if msg.is_empty() {
            None
        } else {
            Some(msg.clone())
        };
        (!msg.is_empty()).then_some(msg)
    }

    pub fn stats(&self) -> PumpStats {
        if self.headless {
            return PumpStats {
                kind: String::new(),
                sample_rate: NOMINAL_RATE,
                state: "headless（不碰 WebAudio）".to_owned(),
                rejected_frames: self.rejected_frames,
                ..Default::default()
            };
        }
        let s = js_stats();
        let get = |i: u32| -> f64 { s.get(i as usize).copied().unwrap_or(0.0) };
        PumpStats {
            kind: js_kind(),
            sample_rate: get(S_RATE) as u32,
            buffered_frames: get(S_BUFFERED),
            target_frames: get(S_TARGET),
            underrun_frames: get(S_UNDERRUN),
            overflow_samples: get(S_OVERFLOW),
            blocks_pushed: get(S_BLOCKS),
            process_quanta: get(S_QUANTA),
            base_latency_ms: get(S_BASE_LATENCY),
            output_latency_ms: get(S_OUTPUT_LATENCY),
            total_latency_ms: get(S_TOTAL_LATENCY),
            state: js_state(),
            rejected_frames: self.rejected_frames,
        }
    }

    /// 累计欠载**帧**数（JS 侧取到静音的帧数；native 数的是样本，见 `pump_native`）。
    /// 判据相同：稳态播放时几乎不涨。
    pub fn underrun_units(&self) -> u64 {
        self.stats().underrun_frames as u64
    }

    /// 一行人类可读的设备状态（示例/日志用，四端同格式）。
    pub fn describe(&self) -> String {
        if self.headless {
            return "headless: 不碰 WebAudio，混音照常推进".to_owned();
        }
        js_state()
    }
}
