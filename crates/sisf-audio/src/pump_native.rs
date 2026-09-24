//! cpal 输出 pump（feature `native`：桌面 + Android）。
//!
//! ## 为什么不是清单字面上的"回调内直接 `Mixer::render`"
//!
//! cpal 要求 data callback 是 `FnMut + Send + 'static`。而 [`Mixer`] 里有
//! `Box<dyn Decoder>`（流式语音；`MidiDecoder` 内含 rustysynth 的合成器，**不是 `Send`**），
//! 主线程又必须随时能改混音状态。把混音器搬进回调只有两条路：给它套锁（清单明令禁止），
//! 或者用 `unsafe` 造一个跨线程 `&mut`（同一份状态被两个线程同时读写，正是爆音与 UB 的来源）。
//!
//! 所以这里换了个等价的形状：**混音仍在主线程 `pump()` 里跑，产出的交织 f32 压进一条 SPSC
//! 环；音频回调只从环里取数、转换成设备样本格式**。锁依然是 0 个（环只有原子下标 + 原子
//! `u32` 样本位型），设备线程做的功退化成一次拷贝加一次格式转换 —— 这也正是它该做的事。
//!
//! ## 设备变更
//!
//! cpal 不提供"默认设备变了"的通知，错误回调也**不保证**会来（拔掉蓝牙耳机后 WASAPI 常常
//! 只是静默地把流停在原地）。于是两路兜底：
//! 1. `StreamError` → 置 `rebuild`，主线程下一次 `pump()` 重建流；
//! 2. 每 [`DEVICE_POLL`] 主动查一次默认输出设备的 `DeviceId`，变了就重建（重建后环清空，
//!    不播陈旧缓冲）。
//!
//! 两条都受 [`OPEN_RETRY`] 节流，避免设备真的没了时每次都开一遍流。

use std::sync::Arc;
use std::sync::atomic::{
    AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering as AtomicOrdering,
};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, ErrorKind, FromSample, Host, SampleFormat, SizedSample, Stream, StreamConfig};
use sisf_media::audio::AudioFormat;

use crate::mixer::Mixer;

/// 没有可用输出设备时的名义采样率（混音照常推进，只是不出声）。
pub const NOMINAL_RATE: u32 = 48_000;
/// SPSC 环容量（输出帧，1 帧 = 左右各一个样本）。
/// 稳态深度只有 [`TARGET_FRAMES`]，多出来的部分是**给慢帧留的**：一次 `pump()` 至多补到这里，
/// 所以它决定了还能撑住的最低帧率（4096 帧 ≈ 85ms ⇒ 约 11fps@48k，见 [`refill_samples`]）。
pub const RING_FRAMES: usize = 4096;
/// 目标缓冲深度（帧）≈ 21ms@48k。**另加**上一帧被播掉的量，见 [`refill_samples`]。
pub const TARGET_FRAMES: usize = 1024;
/// 一次 `pump()` 渲染的块长（帧）。
pub const BLOCK_FRAMES: usize = 256;
/// 一次 `pump()` 最多渲染几块（卡帧后的追赶上限，保证不无限占用主线程）。
/// 恰好等于环容量 ⇒ 慢帧也能一次补足（见 [`refill_samples`]）。
pub const MAX_BLOCKS: usize = RING_FRAMES / BLOCK_FRAMES;
/// 主动轮询默认输出设备的间隔。
const DEVICE_POLL: Duration = Duration::from_millis(500);
/// 开流/重建失败后的重试间隔。
const OPEN_RETRY: Duration = Duration::from_millis(2_000);

/// 这次 `pump()` 要把环补到多少**样本**才罢手。
///
/// `taken_samples` = 上一次 `pump()` 之后设备回调实际取走的样本数（一帧要播掉的量）。
/// 只按 [`TARGET_FRAMES`] 封顶是个帧率陷阱：每帧至多补到 1024 帧，而每帧要播掉
/// `rate / fps` 帧 ⇒ fps 低于 `rate / TARGET` ≈ 47 时**结构性补给不足**，稳态就一直缺一点
/// （Chrome 软件渲染实测 ~42fps ⇒ 11% 的帧是静音，见 `docs/audio-web.md`）。
/// 所以下限取"目标深度"与"一帧的量 + 1/4 余量"里较大的那个：60fps 下仍是 1024 帧 = 21ms
/// （不拿延迟换保险），掉帧时先加深缓冲、并受 `capacity_samples`（环容量）约束，
/// 因此一次卡帧最多多缓冲 `RING_FRAMES` 帧，之后自动收敛。
pub fn refill_samples(taken_samples: usize, capacity_samples: usize) -> usize {
    let floor = taken_samples + taken_samples / 4;
    floor.max(TARGET_FRAMES * 2).min(capacity_samples)
}

/// 单生产者单消费者的 f32 环形缓冲（无锁）。
///
/// `head`/`tail` 是**绝对样本游标**（只增不减），槽位下标按 `mask` 取模。样本以 `u32` 位型
/// 存放，是因为 Rust 没有 `AtomicF32` —— `to_bits`/`from_bits` 是无损往返，不改变数值。
/// 生产者只有主线程（`push`），消费者只有音频回调（`pop`），因此读写区间天然不重叠，
/// Relaxed 存取样本 + Release/Acquire 发布游标就够（见 `tests::ring_moves_data_between_threads`）。
pub struct Ring {
    mask: usize,
    slots: Vec<AtomicU32>,
    head: AtomicUsize,
    tail: AtomicUsize,
    /// 回调要的样本数 > 环里有的样本数（缺的部分补零 = 爆音的候选点，必须是可观测量）。
    underrun_samples: AtomicU64,
    /// 主线程压不进环的样本数（环满 = 追不上，同样是观测量）。
    overflow_samples: AtomicU64,
}

impl Ring {
    /// 容量向上取到 2 的幂个帧（`frames * 2` 个样本）。
    pub fn with_frames(frames: usize) -> Self {
        let cap = (frames.max(1) * 2).next_power_of_two();
        Self {
            mask: cap - 1,
            slots: (0..cap).map(|_| AtomicU32::new(0)).collect(),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            underrun_samples: AtomicU64::new(0),
            overflow_samples: AtomicU64::new(0),
        }
    }

    /// 环里现有样本数（生产者视角的下界）。
    pub fn filled(&self) -> usize {
        self.head
            .load(AtomicOrdering::Acquire)
            .wrapping_sub(self.tail.load(AtomicOrdering::Acquire))
    }

    pub fn capacity(&self) -> usize {
        self.mask + 1
    }

    /// 生产者写入；返回实际写入的样本数（环满则小于 `src.len()`，**不记账** ——
    /// 是"回头重试"还是"认栽丢音"由调用方决定，见 [`Ring::push_or_drop`]）。
    pub fn push(&self, src: &[f32]) -> usize {
        let mut head = self.head.load(AtomicOrdering::Relaxed);
        let free = self.capacity() - self.filled();
        let n = src.len().min(free);
        for slot in src.iter().take(n) {
            self.slots[head & self.mask].store(slot.to_bits(), AtomicOrdering::Relaxed);
            head += 1;
        }
        self.head.store(head, AtomicOrdering::Release);
        n
    }

    /// 尽力写入，写不下的部分**当场丢弃**并计入 overflow —— 混音泵用的就是它：
    /// 主线程不能因为环满而阻塞，也绝不重试（重试等于把同一段音频播两遍）。
    pub fn push_or_drop(&self, src: &[f32]) -> usize {
        let n = self.push(src);
        if n < src.len() {
            self.overflow_samples
                .fetch_add((src.len() - n) as u64, AtomicOrdering::Relaxed);
        }
        n
    }

    /// 消费者读取；填满 `dst`，环不够的部分补零并计入 underrun。返回真实读到的样本数。
    pub fn pop(&self, dst: &mut [f32]) -> usize {
        let mut tail = self.tail.load(AtomicOrdering::Relaxed);
        let have = self.filled();
        let n = dst.len().min(have);
        for d in dst.iter_mut().take(n) {
            *d = f32::from_bits(self.slots[tail & self.mask].load(AtomicOrdering::Relaxed));
            tail += 1;
        }
        self.tail.store(tail, AtomicOrdering::Release);
        if n < dst.len() {
            self.underrun_samples
                .fetch_add((dst.len() - n) as u64, AtomicOrdering::Relaxed);
            for d in dst.iter_mut().skip(n) {
                *d = 0.0;
            }
        }
        n
    }

    /// 丢弃未读样本（**只能由生产者调**，且必须在消费者已经停了之后 —— 本模块只在
    /// `Stream` 被 drop（cpal 在 drop 里停回调）之后调用它）。
    pub fn clear(&self) {
        let head = self.head.load(AtomicOrdering::Relaxed);
        self.tail.store(head, AtomicOrdering::Release);
    }

    pub fn underrun_samples(&self) -> u64 {
        self.underrun_samples.load(AtomicOrdering::Relaxed)
    }

    pub fn overflow_samples(&self) -> u64 {
        self.overflow_samples.load(AtomicOrdering::Relaxed)
    }
}

/// 主线程与音频回调共享的一切（无锁）。
struct Shared {
    ring: Ring,
    /// 错误回调要求重建流；由主线程消费。
    rebuild: AtomicBool,
    /// 最近一次 `StreamError` 的 `ErrorKind` 判别码（0 = 没有错误）。
    last_error: AtomicU32,
    stream_errors: AtomicU64,
    callbacks: AtomicU64,
    /// 环里被读走的样本数（延迟统计的分母）。
    consumed: AtomicU64,
}

impl Shared {
    fn new() -> Self {
        Self {
            ring: Ring::with_frames(RING_FRAMES),
            rebuild: AtomicBool::new(false),
            last_error: AtomicU32::new(0),
            stream_errors: AtomicU64::new(0),
            callbacks: AtomicU64::new(0),
            consumed: AtomicU64::new(0),
        }
    }
}

/// cpal 的错误类别是 `#[non_exhaustive]` 枚举，跨线程传字符串要锁或分配；这里把它压成
/// 一个码 + 回程查表，`stats()` 就能给出"DeviceNotAvailable"这样可读的原因。
fn error_code(kind: ErrorKind) -> u32 {
    match kind {
        ErrorKind::DeviceBusy => 1,
        ErrorKind::DeviceChanged => 2,
        ErrorKind::DeviceNotAvailable => 3,
        ErrorKind::HostUnavailable => 4,
        ErrorKind::InvalidInput => 5,
        ErrorKind::PermissionDenied => 6,
        ErrorKind::RealtimeDenied => 7,
        ErrorKind::ResourceExhausted => 8,
        ErrorKind::StreamInvalidated => 9,
        ErrorKind::UnsupportedConfig => 10,
        ErrorKind::UnsupportedOperation => 11,
        ErrorKind::Xrun => 12,
        ErrorKind::BackendError => 13,
        ErrorKind::Other => 14,
        _ => 99,
    }
}

fn error_name(code: u32) -> &'static str {
    match code {
        1 => "DeviceBusy",
        2 => "DeviceChanged",
        3 => "DeviceNotAvailable",
        4 => "HostUnavailable",
        5 => "InvalidInput",
        6 => "PermissionDenied",
        7 => "RealtimeDenied",
        8 => "ResourceExhausted",
        9 => "StreamInvalidated",
        10 => "UnsupportedConfig",
        11 => "UnsupportedOperation",
        12 => "Xrun",
        13 => "BackendError",
        14 => "Other",
        _ => "Unknown",
    }
}

/// pump 的可观测状态（听测与"拔插耳机不断流"的判据都读这里）。
#[derive(Clone, Debug, Default)]
pub struct PumpStats {
    /// 输出设备描述；`None` = 本次构建没有可用设备（静音但照常混音）。
    pub device: Option<String>,
    pub sample_rate: u32,
    pub channels: u16,
    /// cpal 样本格式名（`F32`/`I16`/…）。
    pub sample_format: String,
    /// 是否已经建起 cpal 流。
    pub streaming: bool,
    /// cpal 报告的每次回调帧数（`buffer_size()` 失败时 0）。
    pub callback_frames: u32,
    /// 环里现有多少**帧**还没被设备取走（≈ 排队延迟）。
    pub ring_frames_filled: usize,
    /// 回调取空过多少次（补零的**样本**数）。稳态播放时该几乎不涨。
    pub underrun_samples: u64,
    /// 主线程写爆过多少次（丢掉的**样本**数）。按 `pump()` 的上限构造恒为 0。
    pub overflow_samples: u64,
    pub callbacks: u64,
    pub stream_errors: u64,
    /// 最近一次流错误（`"DeviceNotAvailable"` 之类）。
    pub last_error: Option<String>,
    pub opened_streams: u64,
}

/// 已建好的 cpal 流（`Stream` 不是 `Send`，必须留在建它的那个线程上）。
struct Live {
    stream: Stream,
    rate: u32,
    channels: u16,
    format: SampleFormat,
    /// 设备 `id()` 的字符串指纹，用于轮询默认设备是否换了。
    device_id: String,
    device: String,
}

/// cpal 输出 pump。见模块文档。
pub struct Pump {
    shared: Arc<Shared>,
    live: Option<Live>,
    scratch: Vec<f32>,
    next_device_poll: Instant,
    next_open_try: Instant,
    opened_streams: u64,
    /// 关掉设备通路（[`Pump::headless`]）：混音照常推进，样本丢弃。
    device_allowed: bool,
    /// `Shared::consumed` 的上一次读数；差值 = 这次要补多少。
    last_consumed: u64,
}

impl Default for Pump {
    fn default() -> Self {
        Self::new()
    }
}

impl Pump {
    /// 不碰设备：设备留给第一次 [`Pump::pump`]（打不开也不报错，见 `stats().device`）。
    pub fn new() -> Self {
        Self::build(true)
    }

    /// 永不触碰 cpal 的 pump：混音时钟照常推进、语音照常回收，只是不出声。
    /// 给 CI、无声卡机器、以及"音频设备被别的应用独占时仍要跑逻辑"用。
    pub fn headless() -> Self {
        Self::build(false)
    }

    fn build(device_allowed: bool) -> Self {
        let now = Instant::now();
        Self {
            shared: Arc::new(Shared::new()),
            live: None,
            scratch: Vec::new(),
            next_device_poll: now,
            next_open_try: now,
            opened_streams: 0,
            device_allowed,
            last_consumed: 0,
        }
    }

    /// 当前混音应使用的输出格式。
    pub fn format(&self) -> AudioFormat {
        AudioFormat::stereo(match self.live.as_ref() {
            Some(l) => l.rate,
            None => NOMINAL_RATE,
        })
    }

    /// 驱动一次混音：把环补到 [`TARGET_FRAMES`]，顺带处理设备变更与重建。
    ///
    /// 返回 `true` = 这次调用真的渲染过（`false` = 环里存货还够，一帧没渲）。
    pub fn pump(&mut self, mixer: &mut Mixer) -> bool {
        self.maintain_device(mixer);
        let rate = self.format().sample_rate;
        if mixer.format().sample_rate != rate {
            // 换率只重算各语音步长，位置不动（`Mixer::set_format` 的约定）。
            mixer.set_format(AudioFormat::stereo(rate));
        }
        let block = BLOCK_FRAMES * 2;
        if self.live.is_none() {
            // 没有设备：仍按名义率混一块丢掉。游戏逻辑（fade 到点、SE 回收）依赖混音时钟，
            // 不能因为"听不见"就停摆 —— CI 与无声卡机器都走这条路。
            self.scratch.resize(block, 0.0);
            let _ = mixer.render(&mut self.scratch[..block]);
            return true;
        }
        let mut rendered = false;
        // 上一次 pump 之后回调取走了多少样本 = 这一帧要补的量。
        let taken = self
            .shared
            .consumed
            .load(AtomicOrdering::Relaxed)
            .saturating_sub(self.last_consumed);
        self.last_consumed += taken;
        let target = refill_samples(taken as usize, self.shared.ring.capacity());
        for _ in 0..MAX_BLOCKS {
            if self.shared.ring.filled() >= target {
                break;
            }
            self.scratch.resize(block, 0.0);
            let _ = mixer.render(&mut self.scratch[..block]);
            self.shared.ring.push_or_drop(&self.scratch[..block]);
            rendered = true;
        }
        rendered
    }

    pub fn stats(&self) -> PumpStats {
        let s = &self.shared;
        let mut st = PumpStats {
            opened_streams: self.opened_streams,
            ring_frames_filled: s.ring.filled() / 2,
            underrun_samples: s.ring.underrun_samples(),
            overflow_samples: s.ring.overflow_samples(),
            callbacks: s.callbacks.load(AtomicOrdering::Relaxed),
            stream_errors: s.stream_errors.load(AtomicOrdering::Relaxed),
            ..Default::default()
        };
        let code = s.last_error.load(AtomicOrdering::Relaxed);
        if code != 0 {
            st.last_error = Some(error_name(code).to_owned());
        }
        if let Some(l) = self.live.as_ref() {
            st.device = Some(l.device.clone());
            st.sample_rate = l.rate;
            st.channels = l.channels;
            st.sample_format = format!("{:?}", l.format);
            st.streaming = true;
            st.callback_frames = l.stream.buffer_size().unwrap_or(0);
        }
        st
    }

    /// 一行人类可读的设备状态（示例/日志用，与 wasm 侧同格式）。
    pub fn describe(&self) -> String {
        let s = self.stats();
        match self.live.as_ref() {
            Some(_) => format!(
                "cpal: {} @{}Hz/{}ch {s_sample} ring={filled}f underrun={un}s overflow={ov}s \
                 callbacks={cb} streams={open} err={err}",
                s.device.clone().unwrap_or_default(),
                s.sample_rate,
                s.channels,
                s_sample = s.sample_format,
                filled = s.ring_frames_filled,
                un = s.underrun_samples,
                ov = s.overflow_samples,
                cb = s.callbacks,
                open = s.opened_streams,
                err = s.last_error.clone().unwrap_or_else(|| "-".to_owned()),
            ),
            None => format!(
                "cpal: 无输出设备（静音混音，时钟照常）frames={} err={}",
                s.ring_frames_filled,
                s.last_error.clone().unwrap_or_else(|| "-".to_owned())
            ),
        }
    }

    /// 累计欠载**样本**数（设备回调补零的个数）。判据：稳态播放时几乎不涨。
    pub fn underrun_units(&self) -> u64 {
        self.shared.ring.underrun_samples()
    }

    /// 冻结/恢复硬件输出（页面隐藏、Android `Suspended`）。混音状态不动。
    ///
    /// 返回 cpal 的错误文本（`Some` = 没做成），由调用方决定要不要报出来。
    pub fn set_output_active(&mut self, active: bool) -> Option<String> {
        let l = self.live.as_ref()?;
        let r = if active {
            l.stream.play()
        } else {
            l.stream.pause()
        };
        r.err().map(|e| e.to_string())
    }

    /// 三件事：错误回调要求重建、默认设备换了、没设备时定期重试。
    fn maintain_device(&mut self, mixer: &mut Mixer) {
        if !self.device_allowed {
            return;
        }
        let now = Instant::now();
        let want_rebuild = self.shared.rebuild.swap(false, AtomicOrdering::AcqRel);
        let mut poll = false;
        if now >= self.next_device_poll {
            self.next_device_poll = now + DEVICE_POLL;
            poll = true;
        }
        if want_rebuild {
            self.reopen(mixer);
            return;
        }
        if let Some(l) = self.live.as_ref() {
            if poll {
                let id = current_device_id();
                if id.as_deref() != Some(l.device_id.as_str()) {
                    self.reopen(mixer);
                }
            }
            return;
        }
        if now >= self.next_open_try {
            self.next_open_try = now + OPEN_RETRY;
            self.reopen(mixer);
        }
    }

    /// 拆掉旧流（回调随 `Stream` 的 drop 停止）→ 重开 → 清环。
    fn reopen(&mut self, mixer: &mut Mixer) {
        self.live = None;
        self.shared.ring.clear();
        match self.open_stream() {
            Ok(l) => {
                if mixer.format().sample_rate != l.rate {
                    mixer.set_format(AudioFormat::stereo(l.rate));
                }
                self.live = Some(l);
            }
            Err(e) => {
                self.next_open_try = Instant::now() + OPEN_RETRY;
                eprintln!("[sisf-audio] 打不开输出设备，改用静音混音: {e}");
            }
        }
    }

    fn open_stream(&mut self) -> Result<Live, String> {
        let host = Host::default();
        let device = host
            .default_output_device()
            .ok_or_else(|| "系统没有默认输出设备".to_owned())?;
        let desc = device
            .description()
            .map(|d| d.name().to_owned())
            .unwrap_or_else(|_| "<unknown>".to_owned());
        let device_id = current_device_id().unwrap_or_else(|| desc.clone());
        let (config, format) = pick_config(&device)?;
        let shared = Arc::clone(&self.shared);
        let channels = config.channels.max(1) as usize;
        let stream = match format {
            SampleFormat::F32 => spawn::<f32>(&device, config, shared, channels)?,
            SampleFormat::I16 => spawn::<i16>(&device, config, shared, channels)?,
            SampleFormat::U16 => spawn::<u16>(&device, config, shared, channels)?,
            SampleFormat::I32 => spawn::<i32>(&device, config, shared, channels)?,
            SampleFormat::U32 => spawn::<u32>(&device, config, shared, channels)?,
            other => return Err(format!("cpal 样本格式 {other:?} 未接")),
        };
        stream.play().map_err(|e| format!("启动输出流失败: {e}"))?;
        self.opened_streams += 1;
        Ok(Live {
            stream,
            rate: config.sample_rate,
            channels: config.channels,
            format,
            device_id,
            device: desc,
        })
    }
}

/// 取一份能用的输出配置：优先设备默认配置，格式不认识时退到"立体声 + 标准采样率"。
///
/// 返回 `(StreamConfig, SampleFormat)` —— cpal 0.18 的 `StreamConfig` 里**没有**样本格式，
/// 它只存在于 `SupportedStreamConfig`，所以必须单独带出去做泛型分发。
fn pick_config(device: &Device) -> Result<(StreamConfig, SampleFormat), String> {
    let supported = |fmt: SampleFormat| {
        matches!(
            fmt,
            SampleFormat::F32
                | SampleFormat::I16
                | SampleFormat::U16
                | SampleFormat::I32
                | SampleFormat::U32
        )
    };
    if let Ok(c) = device.default_output_config()
        && supported(c.sample_format())
    {
        return Ok((c.config(), c.sample_format()));
    }
    // 默认配置用了我们没实现的格式（比如 I24/U24）：在支持的配置里挑一个立体声标准率的。
    let mut fallback: Option<(StreamConfig, SampleFormat)> = None;
    for range in device
        .supported_output_configs()
        .map_err(|e| e.to_string())?
    {
        let fmt = range.sample_format();
        if !supported(fmt) {
            continue;
        }
        if let Some(c) = range.try_with_standard_sample_rate() {
            let cfg = c.config();
            if cfg.channels == 2 {
                return Ok((cfg, fmt));
            }
            fallback.get_or_insert((cfg, fmt));
        }
    }
    fallback.ok_or_else(|| "设备没有本模块支持的输出配置".to_owned())
}

/// 建一路泛型样本格式的输出流。
fn spawn<S>(
    device: &Device,
    config: StreamConfig,
    shared: Arc<Shared>,
    channels: usize,
) -> Result<Stream, String>
where
    S: SizedSample + FromSample<f32>,
{
    let err = Arc::clone(&shared);
    let mut scratch: Vec<f32> = Vec::new();
    device
        .build_output_stream(
            config,
            move |data: &mut [S], _info| {
                let frames = data.len() / channels;
                scratch.clear();
                scratch.resize(frames * 2, 0.0);
                let got = shared.ring.pop(&mut scratch);
                shared
                    .consumed
                    .fetch_add(got as u64, AtomicOrdering::Relaxed);
                shared.callbacks.fetch_add(1, AtomicOrdering::Relaxed);
                for (i, slot) in data.iter_mut().enumerate() {
                    let f = i / channels;
                    let c = i % channels;
                    let v = if channels == 1 {
                        (scratch[f * 2] + scratch[f * 2 + 1]) * 0.5
                    } else if c < 2 {
                        scratch[f * 2 + c]
                    } else {
                        0.0
                    };
                    *slot = S::from_sample(v);
                }
            },
            move |e: cpal::Error| {
                err.last_error
                    .store(error_code(e.kind()), AtomicOrdering::Relaxed);
                err.stream_errors.fetch_add(1, AtomicOrdering::Relaxed);
                // 任何流错误都重建：`DeviceChanged` 时 cpal 说流还能用，但新设备的采样率
                // 可能已经不同，重建顺带把混音率对齐，代价只有一次 2s 节流内的静默。
                err.rebuild.store(true, AtomicOrdering::Release);
                eprintln!("[sisf-audio] cpal 流错误（{:?}），请求重建输出流", e.kind());
            },
            None,
        )
        .map_err(|e| format!("build_output_stream 失败: {e}"))
}

/// 当前默认输出设备的 id 指纹（cpal 的 `DeviceId` 没有 Display，用 Debug 文本比较）。
fn current_device_id() -> Option<String> {
    let device = Host::default().default_output_device()?;
    match device.id() {
        Ok(id) => Some(format!("{id:?}")),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mixer::{Channel, PlayParams, VoiceData};
    use sisf_media::audio::AudioData;

    fn arc_data(frames: u32) -> Arc<AudioData> {
        let samples: Vec<f32> = (0..frames as usize)
            .flat_map(|f| {
                let v = (f % 500) as f32 / 1000.0;
                [v, v]
            })
            .collect();
        Arc::new(AudioData::one_shot(samples, AudioFormat::stereo(44_100)))
    }

    #[test]
    fn ring_is_fifo_and_zero_fills_the_shortfall() {
        let r = Ring::with_frames(8);
        assert_eq!(r.capacity(), 16);
        assert_eq!(r.push(&[1.0, 2.0, 3.0]), 3);
        assert_eq!(r.filled(), 3);
        let mut dst = [0.0f32; 5];
        assert_eq!(r.pop(&mut dst), 3);
        assert_eq!(
            dst,
            [1.0, 2.0, 3.0, 0.0, 0.0],
            "缺的部分必须补零而不是留陈旧样本"
        );
        assert_eq!(r.underrun_samples(), 2);
        assert_eq!(r.filled(), 0);
    }

    #[test]
    fn ring_wraps_and_reports_overflow() {
        let r = Ring::with_frames(4); // 容量 8 样本
        let src: Vec<f32> = (0..10).map(|i| i as f32).collect();
        // `push` 只写能写下的，不记账（调用方可能要重试）。
        assert_eq!(r.push(&src), 8, "写满即停，绝不越界覆盖未读样本");
        assert_eq!(r.overflow_samples(), 0, "push 的拒绝不算丢音");
        assert_eq!(r.filled(), 8);
        // `push_or_drop` 才记 overflow —— 泵用它，因为它不重试。
        r.pop(&mut [0.0f32; 8]);
        assert_eq!(r.push_or_drop(&src), 8);
        assert_eq!(r.overflow_samples(), 2, "写不下的 2 个样本必须被记账");
        // 读走一半再绕回去写：下标必须按 mask 折回。
        let mut dst = [0.0f32; 4];
        assert_eq!(r.pop(&mut dst), 4);
        assert_eq!(dst, [0.0, 1.0, 2.0, 3.0]);
        assert_eq!(r.push(&[9.0, 9.5, 10.0, 11.0, 12.0]), 4);
        assert_eq!(r.pop(&mut dst), 4);
        assert_eq!(dst, [4.0, 5.0, 6.0, 7.0], "环形回绕后顺序不能乱");
    }

    #[test]
    fn clear_drops_unread_audio() {
        let r = Ring::with_frames(8);
        r.push(&[1.0, 2.0, 3.0]);
        r.clear();
        assert_eq!(r.filled(), 0);
        let mut dst = [0.0f32; 2];
        assert_eq!(r.pop(&mut dst), 0);
        assert_eq!(dst, [0.0, 0.0]);
    }

    /// 真起两个线程跑环：Sanitizer 之外，这也是"无锁 SPSC 的内存序写对了"的最小证据。
    #[test]
    fn ring_moves_data_between_threads() {
        const N: usize = 200_000;
        let r = Arc::new(Ring::with_frames(256));
        let (prod, cons) = (Arc::clone(&r), Arc::clone(&r));
        let producer = std::thread::spawn(move || {
            let mut i = 0usize;
            let mut block = [0.0f32; 64];
            while i < N {
                for (n, slot) in block.iter_mut().enumerate() {
                    *slot = (i + n) as f32;
                }
                let want = (N - i).min(block.len());
                let src = &mut block[..want];
                let mut off = 0;
                while off < want {
                    off += prod.push(&src[off..]);
                    if off < want {
                        std::thread::yield_now();
                    }
                }
                i += want;
            }
        });
        let consumer = std::thread::spawn(move || {
            let mut got = 0usize;
            let mut dst = [0.0f32; 37]; // 故意与生产块长不互质，覆盖所有相位
            while got < N {
                let want = (N - got).min(dst.len());
                let n = cons.pop(&mut dst[..want]);
                for (k, v) in dst.iter().enumerate().take(n) {
                    assert_eq!(*v, (got + k) as f32, "第 {} 个样本错位", got + k);
                }
                got += n;
                if n < want {
                    std::thread::yield_now();
                }
            }
        });
        producer.join().expect("生产者线程");
        consumer.join().expect("消费者线程");
        // 逐样本相等（上面断言过）+ 一个不少 = 顺序与内容都没被并发破坏。
        assert_eq!(r.overflow_samples(), 0, "生产者重试不是丢音");
        assert_eq!(r.filled(), 0, "全部样本都被消费掉了");
        assert!(
            r.underrun_samples() > 0,
            "消费线程是自由自旋的，理应有取空的时候（否则这测试没压到空环路径）"
        );
    }

    /// 补给阈值必须覆盖"这一帧会被播掉的量"，否则主线程一掉到 ~47fps 以下就结构性欠载。
    /// Chrome 软件渲染实测 ~42fps 时 11% 的帧是静音，就是这个阈值（`docs/audio-web.md`）。
    #[test]
    fn refill_target_covers_a_whole_slow_frame() {
        let cap = RING_FRAMES * 2;
        // 60fps：每帧播 800 帧 = 1600 样本，+1/4 余量仍不到目标深度 ⇒ 延迟保持 21ms。
        assert_eq!(refill_samples(1_600, cap), TARGET_FRAMES * 2);
        // 42fps：每帧 1143 帧 = 2286 样本 ⇒ 阈值必须抬高，否则每帧都欠一次。
        assert!(
            refill_samples(2_286, cap) > 2_286,
            "42fps 时阈值 {} ≤ 一帧要播的 2286 样本 ⇒ 稳态欠载",
            refill_samples(2_286, cap)
        );
        // 24fps 同理。
        assert!(refill_samples(4_000, cap) >= 4_000);
        // 再慢也只锁在环容量：一次卡帧最多多缓冲 RING_FRAMES 帧，不会无限涨。
        assert_eq!(refill_samples(cap * 8, cap), cap);
    }

    /// 没有设备也不能 panic，且混音时钟照常推进（CI/无声卡机器的实际路径）。
    #[test]
    fn pump_runs_silently_without_a_device() {
        let mut p = Pump::headless();
        let mut m = Mixer::new(AudioFormat::stereo(NOMINAL_RATE));
        m.play(
            Channel::Bgm,
            VoiceData::from(arc_data(4096)),
            &PlayParams::default().looping(true),
        )
        .unwrap();
        let before = m.rendered_frames();
        assert!(
            p.pump(&mut m),
            "无设备时 pump 仍要渲染（否则 fade/回收会停摆）"
        );
        assert!(m.rendered_frames() > before, "混音时钟必须照常推进");
        let st = p.stats();
        assert!(!st.streaming, "headless 不该建流");
        assert_eq!(st.ring_frames_filled, 0, "无设备时不该往环里压数据");
        assert_eq!(st.underrun_samples, 0);
        // 名义率：混音器跟着 pump 的输出格式走。
        assert_eq!(m.format().sample_rate, NOMINAL_RATE);
    }

    #[test]
    fn error_codes_round_trip() {
        for kind in [
            ErrorKind::DeviceBusy,
            ErrorKind::DeviceNotAvailable,
            ErrorKind::UnsupportedConfig,
            ErrorKind::Xrun,
        ] {
            let code = error_code(kind);
            assert_ne!(code, 0);
            assert_eq!(error_name(code), format!("{kind:?}"));
        }
        assert_eq!(error_name(99), "Unknown");
    }
}
