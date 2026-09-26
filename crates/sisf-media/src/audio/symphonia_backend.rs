//! symphonia 后端：OGG/Vorbis、MP3、WAV 的流式解码（`ogg`/`mp3`/`wav` feature）。
//!
//! 只管"字节 → 交织 f32 帧"这一件事；循环点交给 [`crate::audio::meta`]（lofty），因为
//! symphonia 的元数据通路只到 `MetadataBlock` 为止，而三个容器的标记写法各不相同。
//!
//! symphonia 0.6 与 0.5 是一次大版本断裂：`Decoder::try_new`/`receive`/`SamplesBuffer`
//! 全都不存在了，改成 `default::get_codecs().make_audio_decoder(&AudioCodecParameters, ..)`
//! 再 `decode(&packet) -> GenericAudioBufferRef`，且音频缓冲是**平面**的 —— 交织取样本必须走
//! `copy_to_slice_interleaved`。`default-features = false` 时 `wav` 只是容器，PCM 编解码器
//! 要单独开（`Cargo.toml` 里三条 feature 都带了 `symphonia/pcm`）。

use std::io::Cursor;

use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{TimeBase, Timestamp};

use crate::audio::{AudioData, AudioError, AudioFormat, Container, Decoder};

/// 一次想要解出的包数上限的软基准（帧）；实际粒度由容器/编解码器决定。
const BLOCK_FRAMES: usize = 1024;

/// symphonia 之上的流式解码器。
///
/// `pos` 是**已交给调用方的帧数**，也就是下一次 [`Self::read`] 会给出的第一帧。非 seek
/// 路径完全用自己的计数，不信容器 `pts`：gapless 修剪（Vorbis 的 pre-skip 是从 granulepos
/// 反推的，见 symphonia-codec-vorbis）会让"可播帧"与 `pts` 差掉几十到几百帧。
pub struct SymphoniaDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    fmt: AudioFormat,
    /// 容器时间基（`pts` 一格 = `numer/denom` 秒）。
    time_base: TimeBase,
    /// 第一帧的 `pts`，用于把 `pts` 换成帧号。
    start_ts: Timestamp,
    total: Option<u64>,
    /// 已解码、还没交出去的交织样本。
    buf: Vec<f32>,
    /// `buf` 中已消费的**帧**数（不是样本数）。
    head: usize,
    /// seek 之后要丢到的目标帧；`None` = 顺序解码中。
    skip_to: Option<u64>,
    pos: u64,
    done: bool,
}

impl SymphoniaDecoder {
    /// 探测容器并建解码器。`bytes` 会被拷进 `Cursor`（symphonia 的 `MediaSource` 要
    /// `Read + Seek + Send + 'static`，借切片就得给结构体加生命周期，而调用方在解码期
    /// 本来就要把字节握住 —— 拷贝一次换来 `'static`，值得）。
    pub fn new(bytes: &[u8], container: Container) -> Result<Self, AudioError> {
        let mss = MediaSourceStream::new(Box::new(Cursor::new(bytes.to_vec())), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = container.extension() {
            hint.with_extension(ext);
        }
        let format = symphonia::default::get_probe()
            .probe(
                &hint,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(|e| AudioError::Decode(format!("{container:?} 探测失败: {e}")))?;
        // `tracks()` 借用 `format`，取完参数就要收束，否则后面 `next_packet()` 撞借用检查。
        let (track_id, params, time_base, start_ts, total) = {
            let track = format
                .default_track(TrackType::Audio)
                .ok_or_else(|| AudioError::Invalid("容器里没有可解码音轨".to_owned()))?;
            let id = track.id;
            let cp = track.codec_params.clone();
            let audio = cp
                .as_ref()
                .and_then(|c| c.audio())
                .ok_or_else(|| AudioError::Invalid("音轨的编解码参数不是音频".to_owned()))?
                .clone();
            (id, audio, track.time_base, track.start_ts, track.num_frames)
        };
        let decoder = symphonia::default::get_codecs()
            // 编解码器没进本次编译（比如只开了 `wav` 却拿到 FLAC）也走这条路。
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .map_err(|_| AudioError::UnsupportedFormat)?;
        // 解码器可能把参数改了（MP3 的采样率/通道以帧头为准），以它报的为准。
        let out = decoder.codec_params();
        let sample_rate = out
            .sample_rate
            .ok_or_else(|| AudioError::Invalid(format!("{container:?} 没有报出采样率")))?;
        let channels = out
            .channels
            .as_ref()
            .map(|c| c.count())
            .filter(|n| *n > 0)
            .unwrap_or(2);
        let fmt = AudioFormat {
            sample_rate,
            channels: channels as u16,
        };
        Ok(Self {
            format,
            decoder,
            track_id,
            fmt,
            time_base: time_base
                .unwrap_or_else(|| TimeBase::try_from_recip(sample_rate).unwrap_or_default()),
            start_ts,
            total,
            buf: Vec::new(),
            head: 0,
            skip_to: None,
            pos: 0,
            done: false,
        })
    }

    /// `pts` → 可播帧号（可能为负：seek 落在第一帧之前）。
    fn frame_of(&self, ts: Timestamp) -> i64 {
        let ticks = ts.get() - self.start_ts.get();
        let numer = self.time_base.numer.get() as i128;
        let denom = self.time_base.denom.get() as i128;
        let rate = self.fmt.sample_rate as i128;
        ((ticks as i128) * numer * rate / denom).clamp(i64::MIN as i128, i64::MAX as i128) as i64
    }

    /// 帧号 → `pts`（[`Self::frame_of`] 的逆运算）。
    fn ts_of(&self, frame: u64) -> Timestamp {
        let numer = self.time_base.numer.get() as i128;
        let denom = self.time_base.denom.get() as i128;
        let rate = self.fmt.sample_rate as i128;
        let ticks = (frame as i128 * denom) / numer / rate + self.start_ts.get() as i128;
        Timestamp::new(ticks.clamp(i64::MIN as i128, i64::MAX as i128) as i64)
    }

    /// 解一包进 `buf`（含 seek 后的丢弃逻辑）。返回 `false` = 流结束。
    fn pull(&mut self) -> Result<bool, AudioError> {
        let ch = self.fmt.channels as usize;
        loop {
            if self.done {
                return Ok(false);
            }
            let Some(packet) = self.format.next_packet().map_err(decode_err)? else {
                self.done = true;
                return Ok(false);
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let mut tmp = std::mem::take(&mut self.buf);
            tmp.clear();
            let n = match self.decoder.decode(&packet) {
                Ok(got) => {
                    tmp.resize(got.samples_interleaved(), 0.0);
                    // 平面 → 交织的转换（含位深/格式归一）全在这一步里做完。
                    got.copy_to_slice_interleaved(&mut tmp[..]);
                    tmp.len() / ch
                }
                Err(SymError::ResetRequired) => {
                    self.decoder.reset();
                    self.buf = tmp;
                    continue;
                }
                // 坏包/IO 错按 symphonia 的约定跳过，不终止整条流。
                Err(SymError::DecodeError(_)) | Err(SymError::IoError(_)) => {
                    self.buf = tmp;
                    continue;
                }
                Err(e) => {
                    self.buf = tmp;
                    return Err(decode_err(e));
                }
            };
            let first = self.frame_of(packet.pts);
            let mut start = 0usize;
            if let Some(target) = self.skip_to {
                if first + n as i64 <= target as i64 {
                    // 整包都在目标之前：丢掉继续解。
                    self.buf = tmp;
                    continue;
                }
                if first < target as i64 {
                    start = (target - first.max(0) as u64).min(n as u64) as usize;
                }
                self.skip_to = None;
                // 契约：调用方要的正是 `target`，此后位置由我们自己数（见类型注释）。
                self.pos = target;
            }
            let from = start.min(n) * ch;
            self.buf = tmp;
            self.head = from / ch;
            return Ok(true);
        }
    }
}

impl Decoder for SymphoniaDecoder {
    fn format(&self) -> AudioFormat {
        self.fmt
    }

    fn read(&mut self, out: &mut [f32]) -> Result<usize, AudioError> {
        let ch = self.fmt.channels as usize;
        if out.len() < ch {
            return Err(AudioError::Invalid("输出缓冲放不下一个帧".to_owned()));
        }
        let want_total = out.len() / ch;
        let mut written = 0usize;
        while written < want_total {
            let avail = self.buf.len() / ch - self.head;
            if avail == 0 {
                self.buf.clear();
                self.head = 0;
                if !self.pull()? {
                    break;
                }
                continue;
            }
            let take = (want_total - written).min(avail);
            let s = self.head * ch;
            let e = (self.head + take) * ch;
            out[written * ch..(written + take) * ch].copy_from_slice(&self.buf[s..e]);
            self.head += take;
            self.pos += take as u64;
            written += take;
        }
        Ok(written)
    }

    /// 定位到绝对帧号：容器只能跳到**包边界**（`SeekMode::Accurate` 保证落在目标之前），
    /// 剩下的帧由 `Self::pull`（私有）解码后丢弃。
    fn seek_sample(&mut self, frame: u64) -> Result<(), AudioError> {
        let ts = self.ts_of(frame);
        self.format
            .seek(
                SeekMode::Accurate,
                SeekTo::Timestamp {
                    ts,
                    track_id: self.track_id,
                },
            )
            .map_err(|e| AudioError::Seek {
                frame,
                why: e.to_string(),
            })?;
        self.decoder.reset();
        self.buf.clear();
        self.head = 0;
        self.skip_to = Some(frame);
        self.done = false;
        Ok(())
    }

    fn total_frames(&self) -> Option<u64> {
        self.total
    }

    fn block_frames(&self) -> usize {
        BLOCK_FRAMES
    }
}

fn decode_err(e: SymError) -> AudioError {
    AudioError::Decode(e.to_string())
}

/// 一次解完：容器 → 驻留样本，循环点由 [`crate::audio::meta`] 提供。
pub fn decode_container(bytes: &[u8], container: Container) -> Result<AudioData, AudioError> {
    let mut dec = SymphoniaDecoder::new(bytes, container)?;
    let mut buf = Vec::new();
    let mut block = vec![0.0f32; dec.block_frames() * dec.fmt.channels as usize];
    loop {
        let n = Decoder::read(&mut dec, &mut block)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&block[..n * dec.fmt.channels as usize]);
        if buf.len() > 1 << 27 {
            return Err(AudioError::Invalid(
                "解码结果超过 512MiB，拒绝继续".to_owned(),
            ));
        }
    }
    if buf.is_empty() {
        return Err(AudioError::Invalid("没有解出任何样本".to_owned()));
    }
    let format = dec.format();
    let frames = (buf.len() / format.channels as usize) as u32;
    let (start, end) = crate::audio::meta::loop_points(bytes, container)
        .map(|(s, e)| (s.min(frames.saturating_sub(1)), e.min(frames).max(1)))
        .unwrap_or((0, frames));
    Ok(AudioData::new(buf, format, start, end))
}
