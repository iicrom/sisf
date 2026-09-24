//! lewton 回退解码链（feature `ogg-lewton`，默认关）。
//!
//! 存在的理由只有一个：symphonia 的 OGG 通路出问题（上游 panic、某条码流失效）时，
//! 有一条**独立实现**能把游戏音乐放出来。因此它刻意不取代 `ogg`：两条 feature 同时开时
//! [`decode_ogg`](crate::audio::decode_ogg) 仍走 symphonia，只有
//! [`decode_ogg_lewton`](crate::audio::decode_ogg_lewton) 能显式指定回退链 —— 对拍也靠它
//! （`tests::fallback_chain_matches_symphonia`）。
//!
//! 与 symphonia 链的三处差异都是 lewton 的能力边界，不是偷懒：
//! - **通道**：只接 1/2 声道。5.1 的 .ogg 在这条链上直接报错（混音器本身也只吃 1/2 声道，
//!   所以 symphonia 链同样会失败，只是失败得晚一步）。
//! - **回跳**：`OggStreamReader::seek_absgp_pg` 只有**页粒度**，跳完不告知落在哪个样本，
//!   而且实测回 0 会落到头部包上报 `BadAudio`。所以这里的"回到 0"是**重开整条流**
//!   （字节就在手里，重开即精确起点），非 0 一律明确报 [`AudioError::Seek`]。
//!   本链只被"一次解完"的通路使用，那条路永不回跳。
//! - **总帧数**：解码前无从得知（`None`），所以这条链上的流式语音不由混音器负责回绕。

use std::io::Cursor;
use std::sync::Arc;

use lewton::inside_ogg::OggStreamReader;

use crate::audio::{
    AudioData, AudioError, AudioFormat, Decoder, LOOP_END_KEYS, LOOP_START_KEYS, drain,
    parse_loop_frame,
};

/// i16 → f32 的满幅换算：lewton 出定点样本，除以 `32768` 保持 ±1.0 约定。
const I16_SCALE: f32 = 1.0 / 32768.0;

type Reader = OggStreamReader<Cursor<Vec<u8>>>;

/// 开一条流并读头。头读不出来 = 容器对但里面不是 Vorbis（或者压根不是 OGG）；报 `Invalid`
/// 而不是 `UnsupportedFormat`，是为了把 lewton 的原文带进错误里方便排查。
fn open(bytes: &[u8]) -> Result<Reader, AudioError> {
    OggStreamReader::new(Cursor::new(bytes.to_vec()))
        .map_err(|e| AudioError::Invalid(format!("lewton 读不了这个 OGG 的头: {e}")))
}

/// lewton 的 OGG/Vorbis 流式解码器。见模块文档。
pub struct LewtonDecoder {
    reader: Reader,
    /// 重开流用（[`Decoder::seek_sample`] 的 `0`）。留 `Arc` 是为了建流那次拷贝之后不再复制。
    bytes: Arc<Vec<u8>>,
    format: AudioFormat,
    /// 已解出但还没交给上层的交织 i16（一个 Vorbis 包最多 2048 帧）。
    pending: Vec<i16>,
    /// `pending` 里下一个待交付样本的下标。
    head: usize,
    done: bool,
}

impl LewtonDecoder {
    /// 从 OGG 字节建解码器（lewton 要 `Read + Seek`，所以整段拷进 `Cursor`）。
    pub fn new(bytes: &[u8]) -> Result<Self, AudioError> {
        let reader = open(bytes)?;
        let channels = u16::from(reader.ident_hdr.audio_channels);
        if !matches!(channels, 1 | 2) {
            return Err(AudioError::Invalid(format!(
                "lewton 回退链只支持 1/2 通道，实际 {channels}（请开 `ogg` feature 走 symphonia）"
            )));
        }
        let rate = reader.ident_hdr.audio_sample_rate;
        if rate == 0 {
            return Err(AudioError::Invalid("OGG 头里的采样率为 0".into()));
        }
        Ok(Self {
            reader,
            bytes: Arc::new(bytes.to_vec()),
            format: AudioFormat {
                sample_rate: rate,
                channels,
            },
            pending: Vec::new(),
            head: 0,
            done: false,
        })
    }

    /// 从 lewton 自己解析的 Vorbis comments 里取循环点（不引 lofty）。
    ///
    /// 键名与容错解析和 symphonia 链共用同一份词表（见 [`LOOP_START_KEYS`]），否则
    /// "换个解码器循环点就没了"这种差异没人会去查。
    pub fn loop_tags(&self) -> Option<(u32, u32)> {
        let comments = &self.reader.comment_hdr.comment_list;
        let pick = |keys: &[&str]| {
            keys.iter().find_map(|k| {
                comments
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(k))
                    .map(|(_, value)| value.clone())
            })
        };
        let start = parse_loop_frame(&pick(&LOOP_START_KEYS)?)?;
        let end = parse_loop_frame(&pick(&LOOP_END_KEYS)?)?;
        (end > start).then_some((start, end))
    }

    /// 解出一个 Vorbis 包。`false` = 流结束。
    fn refill(&mut self) -> Result<bool, AudioError> {
        match self.reader.read_dec_packet_itl() {
            Err(e) => Err(AudioError::Decode(format!("lewton 解码失败: {e}"))),
            Ok(None) => {
                self.done = true;
                Ok(false)
            }
            Ok(Some(samples)) => {
                self.pending = samples;
                self.head = 0;
                Ok(true)
            }
        }
    }
}

impl Decoder for LewtonDecoder {
    fn format(&self) -> AudioFormat {
        self.format
    }

    fn read(&mut self, out: &mut [f32]) -> Result<usize, AudioError> {
        let ch = self.format.channels as usize;
        let mut frames = 0usize;
        while (frames + 1) * ch <= out.len() {
            if self.head >= self.pending.len() {
                if self.done || !self.refill()? {
                    break;
                }
                continue;
            }
            let avail = (self.pending.len() - self.head) / ch;
            let want = (out.len() - frames * ch) / ch;
            // `pending` 长度与 `head` 都是 `ch` 的整数倍，所以 `avail >= 1`。
            let n = avail.min(want);
            let src = &self.pending[self.head..self.head + n * ch];
            for (i, s) in src.iter().enumerate() {
                out[frames * ch + i] = f32::from(*s) * I16_SCALE;
            }
            self.head += n * ch;
            frames += n;
        }
        Ok(frames)
    }

    /// 只支持回到 0，且用**重开整条流**实现（原因见模块文档的"回跳"）。
    fn seek_sample(&mut self, frame: u64) -> Result<(), AudioError> {
        if frame != 0 {
            return Err(AudioError::Seek {
                frame,
                why: "lewton 的回跳只有页粒度、跳完又不知道落在哪个样本；\
                      精确回跳请用 `ogg` feature 的 symphonia 链"
                    .into(),
            });
        }
        self.reader = open(&self.bytes)?;
        self.pending.clear();
        self.head = 0;
        self.done = false;
        Ok(())
    }

    /// 解码前无从得知（lewton 不提供总时长）。
    fn total_frames(&self) -> Option<u64> {
        None
    }

    fn loop_points(&self) -> Option<(u64, u64)> {
        self.loop_tags().map(|(s, e)| (u64::from(s), u64::from(e)))
    }

    /// Vorbis 包最多 2048 帧；按它开缓冲，一次 `read` 就能填满一整包。
    fn block_frames(&self) -> usize {
        2048
    }
}

/// 一次解完成驻留样本（含循环点）。签名与 `symphonia_backend::decode_container` 对齐，
/// 这样 `audio::decode_ogg` 的两条 cfg 分支可以互换。
pub(crate) fn decode_container(bytes: &[u8]) -> Result<AudioData, AudioError> {
    let mut dec = LewtonDecoder::new(bytes)?;
    drain(&mut dec)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ogg() -> Vec<u8> {
        let path = format!("{}/tests/fixtures/tone_2s.ogg", env!("CARGO_MANIFEST_DIR"));
        std::fs::read(&path).unwrap_or_else(|e| panic!("读 {path} 失败: {e}（先跑 gen_audio.py）"))
    }

    #[test]
    fn lewton_decodes_the_same_fixture_as_symphonia() {
        let d = LewtonDecoder::new(&ogg()).expect("fixture 必须是 lewton 能解的 OGG");
        assert_eq!(d.sample_rate(), 44_100);
        assert_eq!(d.channels(), 2);
        assert_eq!(d.loop_tags(), Some((8820, 35280)), "注释里的循环点得读到");
        assert_eq!(d.total_frames(), None, "lewton 给不出总帧数");
        let data = decode_container(&ogg()).expect("整段解完");
        // 与 `audio::tests::ogg_fixture_carries_loop_tags_verbatim` 用同一套容差。
        assert!(
            data.frames() > 86_000 && data.frames() < 90_000,
            "OGG 帧数离谱: {}",
            data.frames()
        );
        assert_eq!(data.format, AudioFormat::stereo(44_100));
        assert_eq!(
            (data.loop_start_samples, data.loop_end_samples),
            (8820, 35280)
        );
        assert!(data.loops());
    }

    #[test]
    fn left_and_right_channels_are_distinguishable() {
        // fixture 左 440Hz 半幅、右 330Hz 四分之一幅：两路峰值必须都可闻且不相等。
        let data = decode_container(&ogg()).unwrap();
        let peak = |ch: usize| {
            (0..data.frames())
                .filter_map(|f| data.at(f, ch))
                .fold(0.0f32, |a, v| a.max(v.abs()))
        };
        let (l, r) = (peak(0), peak(1));
        assert!(l > 0.4 && l <= 1.0, "左路峰值 {l}");
        assert!(r > 0.15 && r < l * 0.75, "右路峰值 {r}，左路 {l}");
    }

    #[test]
    fn restart_is_exact_and_middle_seeks_say_so() {
        let mut d = LewtonDecoder::new(&ogg()).unwrap();
        assert!(matches!(
            d.seek_sample(1),
            Err(AudioError::Seek { frame: 1, .. })
        ));
        // 回到 0 = 重开整条流：再起一遍必须逐位一致（`Once` 模式下混音器的回绕走这条路）。
        let mut first = vec![0.0f32; 4096 * 2];
        assert_eq!(d.read(&mut first).unwrap(), 4096);
        assert_eq!(d.seek_sample(0), Ok(()));
        let mut again = vec![0.0f32; 4096 * 2];
        assert_eq!(d.read(&mut again).unwrap(), 4096);
        assert_eq!(first, again);
    }

    #[test]
    fn not_an_ogg_is_rejected_not_panicking() {
        assert!(matches!(
            LewtonDecoder::new(b"RIFF\x04\0\0\0WAVEfmt "),
            Err(AudioError::Invalid(_))
        ));
        assert!(decode_container(b"OggS not really vorbis").is_err());
    }

    /// 两条链同时编进来时，回退链必须给出**同一份**内容（长度与循环点一致），
    /// 否则"换后端"这件事本身就是个隐藏的音频差异。
    #[cfg(feature = "ogg")]
    #[test]
    fn fallback_chain_matches_symphonia() {
        use crate::audio::{Container, symphonia_backend};

        let bytes = ogg();
        let primary = symphonia_backend::decode_container(&bytes, Container::Ogg).unwrap();
        let fallback = decode_container(&bytes).unwrap();
        assert_eq!(fallback.format, primary.format);
        assert_eq!(
            (fallback.loop_start_samples, fallback.loop_end_samples),
            (primary.loop_start_samples, primary.loop_end_samples)
        );
        assert!(
            (fallback.frames() as i64 - primary.frames() as i64).abs() < 4_410,
            "lewton {} 帧 vs symphonia {} 帧",
            fallback.frames(),
            primary.frames()
        );
    }
}
