//! 音频样本模型与解码器接口（T7）。
//!
//! 三件事分层：
//! 1. [`AudioData`] —— **已解码的驻留内存样本**（交织 f32，1.0 满幅），混音器直接按帧取；
//!    `loop_start_samples`/`loop_end_samples` 是从元数据（或调用方）拿到的循环点，单位都是
//!    **源帧**（一帧 = `channels` 个样本），不是字节也不是时间。
//! 2. [`Decoder`] —— **流式**解码器。`sisf-audio` 的流式语音直接用它，MIDI 只有这一条路可走
//!    （整首歌驻留内存是几十 MB 量级）。OGG/MP3/WAV 走 `decode` 一次性解完（换循环点的
//!    正确性简单得多：位置就是数组下标）。
//! 3. 具体后端 —— `symphonia`（`ogg`/`mp3`/`wav` feature）与 `midly`+`rustysynth`（`midi`）。
//!    模块本身不设 feature：`AudioData`/`Decoder` 是 `sisf-audio` 的编译前置，任何 feature
//!    组合下都要在。
//!
//! 采样点格式约定：**交织、非预乘、f32、满幅 ±1.0**。对称于图像侧"解码即预乘"的约定，
//! 音频侧把"格式归一"完全放在解码器里做完，混音器只处理音量/声相/变速。

use thiserror::Error;

#[cfg(feature = "ogg-lewton")]
mod lewton_backend;
#[cfg(feature = "ogg-lewton")]
pub use lewton_backend::LewtonDecoder;
#[cfg(any(feature = "ogg", feature = "mp3", feature = "wav"))]
pub mod meta;
#[cfg(any(feature = "ogg", feature = "mp3", feature = "wav"))]
mod symphonia_backend;
#[cfg(any(feature = "ogg", feature = "mp3", feature = "wav"))]
pub use symphonia_backend::SymphoniaDecoder;

/// 一路音频的物理格式。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AudioFormat {
    /// 采样率（Hz）。
    pub sample_rate: u32,
    /// 通道数（1 = 单声道，2 = 立体声）。
    pub channels: u16,
}

impl AudioFormat {
    pub const fn stereo(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            channels: 2,
        }
    }

    pub const fn mono(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            channels: 1,
        }
    }

    /// 每秒的交织样本个数。
    pub const fn samples_per_second(&self) -> usize {
        self.sample_rate as usize * self.channels as usize
    }
}

/// 解码后的驻留样本 + 循环点。
#[derive(Clone, Debug, PartialEq)]
pub struct AudioData {
    /// 交织样本（`samples[frame * channels + ch]`）。
    pub samples: Vec<f32>,
    pub format: AudioFormat,
    /// 循环起点（源帧，含）。
    pub loop_start_samples: u32,
    /// 循环终点（源帧，不含）。等于 [`AudioData::frames`] 表示不循环（播到尾即止）。
    pub loop_end_samples: u32,
}

impl AudioData {
    /// 以给定循环点建一份样本表；循环点会被夹进 `0..=frames` 且保证 `start < end`
    /// （非法输入降级成"整段不循环"，而不是 panic —— 元数据来自外部文件）。
    pub fn new(samples: Vec<f32>, format: AudioFormat, loop_start: u32, loop_end: u32) -> Self {
        let ch = format.channels.max(1) as usize;
        let frames = (samples.len() / ch) as u32;
        let start = loop_start.min(frames.saturating_sub(1));
        let end = loop_end
            .min(frames)
            .max(start.saturating_add(1))
            .min(frames);
        Self {
            samples,
            format,
            loop_start_samples: start,
            loop_end_samples: end,
        }
    }

    /// 整段播放（无循环标记）的构造：循环点 = `[0, frames)`。
    pub fn one_shot(samples: Vec<f32>, format: AudioFormat) -> Self {
        let frames = (samples.len() / format.channels.max(1) as usize) as u32;
        Self::new(samples, format, 0, frames)
    }

    /// 总帧数（一帧 = `channels` 个样本）。
    pub fn frames(&self) -> u32 {
        (self.samples.len() / self.format.channels.max(1) as usize) as u32
    }

    /// 时长（秒）。
    pub fn duration(&self) -> f64 {
        let rate = self.format.sample_rate.max(1) as f64;
        self.frames() as f64 / rate
    }

    /// 取一帧某个通道的样本（越界返回 `None`，由调用方决定夹取还是静音）。
    pub fn at(&self, frame: u32, ch: usize) -> Option<f32> {
        let i = frame as usize * self.format.channels as usize + ch;
        self.samples.get(i).copied()
    }

    /// 该帧是否落在循环区间内（混音器判断"到尾"用）。
    pub fn loops(&self) -> bool {
        self.loop_end_samples < self.frames() && self.loop_end_samples > self.loop_start_samples
    }
}

/// 解码与播放期错误。`String` 载荷是为了能从后端库的 `Display` 一路冒到
/// `Request::Failed`（跨 crate 的唯一报错通道，见 T4 事实 4）。
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum AudioError {
    /// 认不出容器/编解码（魔数不匹配，或对应 feature 没开）。
    #[error("不支持的音频格式（容器不认识，或 ogg/mp3/wav feature 未启用）")]
    UnsupportedFormat,
    /// 头信息读不出来 / 长度为零 / 采样率或通道数非法。
    #[error("音频数据无效: {0}")]
    Invalid(String),
    /// 解码中途后端报错（截断、非法帧）。
    #[error("解码失败: {0}")]
    Decode(String),
    /// 跳转不支持或越界。
    #[error("无法定位到帧 {frame}: {why}")]
    Seek { frame: u64, why: String },
    /// 需要 SoundFont 却没提供（MIDI 专用，见 `midi.rs`）。
    #[error("缺少 SoundFont（SF2）: {0}")]
    MissingSoundBank(String),
}

/// 流式解码器：把容器/合成器统一成"顺序读帧 + 可按帧号回跳"。
///
/// 约定与 [`AudioData`] 一致：交织 f32、满幅 ±1.0。`read` 每次填满或填不满 `out`
/// 都可以，返回**实际写入的帧数**，`0` = 流结束（不是错误，可继续返回 0）。
pub trait Decoder {
    /// 物理格式（整条流恒定；变率文件请在解码期重采样）。
    fn format(&self) -> AudioFormat;

    fn channels(&self) -> u16 {
        self.format().channels
    }

    fn sample_rate(&self) -> u32 {
        self.format().sample_rate
    }

    /// 读入至多 `out.len() / channels` 帧，返回实际帧数。`0` = 结束。
    fn read(&mut self, out: &mut [f32]) -> Result<usize, AudioError>;

    /// 定位到绝对源帧号（下一帧 `read` 从此开始）。流式语音的循环回绕走这里。
    fn seek_sample(&mut self, frame: u64) -> Result<(), AudioError>;

    /// 总帧数（未知返回 `None`，例如网络流）。
    fn total_frames(&self) -> Option<u64> {
        None
    }

    /// 元数据里的循环点（源帧，终点不含）；`None` = 文件没带循环标记。
    fn loop_points(&self) -> Option<(u64, u64)> {
        None
    }

    /// 一次 `read` 想要的舒适帧数（pump 用它开缓冲；实现可自由选择是否遵守）。
    fn block_frames(&self) -> usize {
        1024
    }
}

/// 把任意 [`Decoder`] 抽干成驻留样本（循环点取自 `loop_points`，没有就整段播）。
pub fn drain<D: Decoder + ?Sized>(dec: &mut D) -> Result<AudioData, AudioError> {
    let format = dec.format();
    if format.channels == 0 || format.sample_rate == 0 {
        return Err(AudioError::Invalid(format!(
            "非法格式 {}Hz/{}ch",
            format.sample_rate, format.channels
        )));
    }
    let ch = format.channels as usize;
    let mut samples: Vec<f32> = Vec::new();
    let mut block = vec![0.0f32; dec.block_frames().max(1) * ch];
    loop {
        let n = dec.read(&mut block)?;
        if n == 0 {
            break;
        }
        samples.extend_from_slice(&block[..n * ch]);
        if samples.len() > 1 << 27 {
            return Err(AudioError::Invalid("解码结果超过 512MiB，拒绝继续".into()));
        }
    }
    if samples.is_empty() {
        return Err(AudioError::Invalid("没有解出任何样本".into()));
    }
    let frames = (samples.len() / ch) as u32;
    let (start, end) = match dec.loop_points() {
        Some((s, e)) if e > s && s < frames as u64 => {
            (s.min(frames as u64 - 1) as u32, e.min(frames as u64) as u32)
        }
        _ => (0, frames),
    };
    Ok(AudioData::new(samples, format, start, end))
}

/// 循环标记的候选键名（大小写不敏感）。引擎认的是 RPG Maker / ffmpeg 那一套自由键名，
/// 单位是**源帧**。MP3 一路把 `MP3LOOPSTART`（AIMP 风格）也试上。
///
/// 键名表放在这里而不是 `meta.rs`，是因为读注释有两条独立通路：lofty（symphonia 链）和
/// lewton 自己解析的 Vorbis comments（`ogg-lewton` 链，不引 lofty）。两条都得认同样的键。
///
/// `allow(dead_code)`：四条 OGG 相关 feature 全关的构建（比如只开 `image` 的 wasm 组合）
/// 里没人读它们，而这词表是无条件编译的，不值得再套一层 cfg。
#[allow(dead_code)]
pub(crate) const LOOP_START_KEYS: [&str; 2] = ["LOOPSTART", "MP3LOOPSTART"];
/// 见 [`LOOP_START_KEYS`]。
#[allow(dead_code)]
pub(crate) const LOOP_END_KEYS: [&str; 2] = ["LOOPEND", "MP3LOOPEND"];

/// 容忍 `"12345"`、`"12345.0"`（有些工具写浮点）、前后空白、负数（→ `None`）。
#[allow(dead_code)]
pub(crate) fn parse_loop_frame(raw: &str) -> Option<u32> {
    let t = raw.trim();
    if let Some(int) = t.split('.').next()
        && let Ok(v) = int.parse::<i64>()
    {
        return u32::try_from(v).ok();
    }
    t.parse::<f64>().ok().and_then(|v| {
        if v.is_finite() && v >= 0.0 {
            u32::try_from(v.round() as i64).ok()
        } else {
            None
        }
    })
}

/// 支持的音频容器。魔数决定它，[`sniff`] 负责认。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Container {
    /// Ogg + Vorbis。
    Ogg,
    /// MPEG 音频（Layer III，也兼容 symphonia 认的 Layer I/II 帧同步）。
    Mp3,
    /// RIFF/WAVE（PCM）。
    Wav,
}

impl Container {
    /// 给 symphonia 的 `Hint::with_extension` 用（容器含糊时帮它一把）。
    pub const fn extension(self) -> Option<&'static str> {
        match self {
            Self::Ogg => Some("ogg"),
            Self::Mp3 => Some("mp3"),
            Self::Wav => Some("wav"),
        }
    }

    /// 该容器的解码 feature 是否参与本次编译（`ogg-lewton` 也算 OGG 可用）。
    ///
    /// 用来把"文件没问题、是这次构建没带解码器"和真正的坏文件区分开报错。
    pub const fn enabled(self) -> bool {
        match self {
            Self::Ogg => cfg!(any(feature = "ogg", feature = "ogg-lewton")),
            Self::Mp3 => cfg!(feature = "mp3"),
            Self::Wav => cfg!(feature = "wav"),
        }
    }

    /// 一次解完成驻留样本（含循环点）。
    pub fn decode(self, bytes: &[u8]) -> Result<AudioData, AudioError> {
        match self {
            Self::Ogg => decode_ogg(bytes),
            Self::Mp3 => decode_mp3(bytes),
            Self::Wav => decode_wav(bytes),
        }
    }
}

/// 按魔数认容器。认不出 = `None`（不等价于"文件坏了"，也可能只是没实现的格式：FLAC、
/// AIFF、MP4 都在这里落空，`sisf` 目前只钉了 ogg/mp3/wav 三条解码链）。
pub fn sniff(bytes: &[u8]) -> Option<Container> {
    if bytes.starts_with(b"OggS") {
        return Some(Container::Ogg);
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        return Some(Container::Wav);
    }
    // ID3v2 前缀，或 MPEG 帧同步（11 个 1）。
    if bytes.starts_with(b"ID3") {
        return Some(Container::Mp3);
    }
    let sync =
        matches!(bytes.first(), Some(0xFF)) && matches!(bytes.get(1), Some(b) if b & 0xE0 == 0xE0);
    sync.then_some(Container::Mp3)
}

#[cfg(feature = "ogg")]
fn decode_ogg(bytes: &[u8]) -> Result<AudioData, AudioError> {
    symphonia_backend::decode_container(bytes, Container::Ogg)
}

// OGG 回退：symphonia 出问题时用 lewton（默认关，见 `audio/lewton_backend.rs`）。
#[cfg(all(feature = "ogg-lewton", not(feature = "ogg")))]
fn decode_ogg(bytes: &[u8]) -> Result<AudioData, AudioError> {
    lewton_backend::decode_container(bytes)
}

/// 显式走 lewton 回退链解 OGG。
///
/// 两条 feature 同时开时 `decode_ogg`（私有）仍然首选 symphonia —— 这个入口是用来**对拍**和
/// 急救的（"symphonia 解不出声、lewton 能" 这种问题时，不必重编一个只开回退的构建）。
#[cfg(feature = "ogg-lewton")]
pub fn decode_ogg_lewton(bytes: &[u8]) -> Result<AudioData, AudioError> {
    lewton_backend::decode_container(bytes)
}

#[cfg(not(any(feature = "ogg", feature = "ogg-lewton")))]
fn decode_ogg(_: &[u8]) -> Result<AudioData, AudioError> {
    Err(AudioError::UnsupportedFormat)
}

#[cfg(feature = "mp3")]
fn decode_mp3(bytes: &[u8]) -> Result<AudioData, AudioError> {
    symphonia_backend::decode_container(bytes, Container::Mp3)
}

#[cfg(not(feature = "mp3"))]
fn decode_mp3(_: &[u8]) -> Result<AudioData, AudioError> {
    Err(AudioError::UnsupportedFormat)
}

#[cfg(feature = "wav")]
fn decode_wav(bytes: &[u8]) -> Result<AudioData, AudioError> {
    symphonia_backend::decode_container(bytes, Container::Wav)
}

#[cfg(not(feature = "wav"))]
fn decode_wav(_: &[u8]) -> Result<AudioData, AudioError> {
    Err(AudioError::UnsupportedFormat)
}

/// 通用入口：嗅探 + 解码。**没开对应 feature 时明确报错**，而不是悄悄返回静音 ——
/// 引擎的 DoD 要求"平台/构建不支持"必须是看得见的错误（`AudioError::UnsupportedFormat`
/// 的 Display 文案里就写着该开哪条 feature）。
pub fn decode_audio(bytes: &[u8]) -> Result<AudioData, AudioError> {
    let c = sniff(bytes).ok_or(AudioError::UnsupportedFormat)?;
    if !c.enabled() {
        return Err(AudioError::UnsupportedFormat);
    }
    c.decode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 三份 fixture 都只有解码 feature 开着才用得上（`--all-targets` 跑默认 feature 时
    // 下面这些测试全被 cfg 掉，助手函数也就成了死代码）。
    #[cfg(any(feature = "ogg", feature = "mp3", feature = "wav"))]
    fn fixture(name: &str) -> Vec<u8> {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read(&path).unwrap_or_else(|e| panic!("读 {path} 失败: {e}（先跑 gen_audio.py）"))
    }

    #[cfg(any(feature = "ogg", feature = "mp3", feature = "wav"))]
    fn peak(data: &AudioData, ch: usize) -> f32 {
        (0..data.frames())
            .filter_map(|f| data.at(f, ch))
            .fold(0.0f32, |a, v| a.max(v.abs()))
    }

    #[test]
    fn sniff_reads_magic_only() {
        assert_eq!(sniff(b"OggS....."), Some(Container::Ogg));
        assert_eq!(
            sniff(b"RIFF\0\0\0\0WAVEfmt "),
            Some(Container::Wav),
            "RIFF 要再看 8..12 的 WAVE，否则 AVI 会被当成音频"
        );
        assert_eq!(sniff(b"RIFF\0\0\0\0AVI "), None);
        assert_eq!(sniff(b"ID3\x04\x00\x00"), Some(Container::Mp3));
        // MPEG 帧同步：11 个 1。
        assert_eq!(sniff(&[0xFF, 0xFB, 0, 0]), Some(Container::Mp3));
        assert_eq!(sniff(b"fLaC\0\0\0\x22"), None, "FLAC 不在钉版解码链里");
        // 每个 fixture 的魔数都得认出来（生成的文件头变了要立刻在这里发现）。
        #[cfg(feature = "wav")]
        assert_eq!(sniff(&fixture("tone_1s.wav")), Some(Container::Wav));
        #[cfg(feature = "mp3")]
        assert_eq!(sniff(&fixture("tone_1s.mp3")), Some(Container::Mp3));
        #[cfg(feature = "ogg")]
        assert_eq!(sniff(&fixture("tone_2s.ogg")), Some(Container::Ogg));
    }

    #[cfg(feature = "wav")]
    #[test]
    fn wav_fixture_length_channels_and_levels() {
        // fixture = 22050Hz 立体声整 1 秒（手写 RIFF，不经编码器，所以长度是精确的）。
        let data = decode_audio(&fixture("tone_1s.wav")).expect("WAV 解码");
        assert_eq!(
            data.format,
            AudioFormat {
                sample_rate: 22050,
                channels: 2
            }
        );
        assert_eq!(data.frames(), 22050);
        assert_eq!(data.samples.len(), 44100);
        assert!(
            (peak(&data, 0) - 0.5).abs() < 0.01,
            "左声道 440Hz 半幅: {}",
            peak(&data, 0)
        );
        assert!(
            (peak(&data, 1) - 0.25).abs() < 0.01,
            "右声道 330Hz 四分之一幅"
        );
        // 没有循环标记 ⇒ 整段播（`loops()` 假）。
        assert_eq!((data.loop_start_samples, data.loop_end_samples), (0, 22050));
        assert!(!data.loops());
    }

    #[cfg(feature = "mp3")]
    #[test]
    fn mp3_fixture_decodes_to_about_a_second() {
        let data = decode_audio(&fixture("tone_1s.mp3")).expect("MP3 解码");
        assert_eq!((data.format.sample_rate, data.format.channels), (44100, 2));
        // LAME 的 delay/padding 由 symphonia 的 gapless 修剪，剩下的误差在几十帧量级。
        assert!(
            data.frames() > 43_000 && data.frames() < 46_000,
            "MP3 帧数离谱: {}",
            data.frames()
        );
        assert!(peak(&data, 0) > 0.4);
        // MP3 没带循环标记 ⇒ 整段播。
        assert_eq!(data.loop_end_samples, data.frames());
    }

    #[cfg(feature = "ogg")]
    #[test]
    fn ogg_fixture_carries_loop_tags_verbatim() {
        let data = decode_audio(&fixture("tone_2s.ogg")).expect("OGG 解码");
        assert_eq!((data.format.sample_rate, data.format.channels), (44100, 2));
        assert!(
            data.frames() > 86_000 && data.frames() < 90_000,
            "OGG 帧数离谱: {}",
            data.frames()
        );
        // LOOPSTART/LOOPEND 是文件里写死的值，解码器不得改写它们（单位 = 源帧）。
        assert_eq!(
            (data.loop_start_samples, data.loop_end_samples),
            (8820, 35280)
        );
        assert!(data.loops());
        assert!(peak(&data, 0) > 0.4);
    }

    #[cfg(feature = "ogg")]
    #[test]
    fn loop_tags_outside_the_file_degrade_to_no_loop() {
        // 元数据来自外部文件，可能胡说八道：夹住而不是 panic（`AudioData::new` 的职责）。
        let mut data = decode_audio(&fixture("tone_2s.ogg")).expect("OGG 解码");
        let (frames, format) = (data.frames(), data.format);
        let data = AudioData::new(
            std::mem::take(&mut data.samples),
            format,
            frames + 999,
            u32::MAX,
        );
        assert!(data.loop_start_samples < frames);
        assert!(data.loop_end_samples <= frames);
        assert!(!data.loops(), "越界的循环点应退化成整段播");
    }

    #[test]
    fn unsupported_magic_is_an_error_not_silence() {
        // FLAC/MP4/垃圾字节：认不出容器 ⇒ `UnsupportedFormat`（绝不能给出一份空样本）。
        for bytes in [
            &b"fLaC\0\0\0\x22"[..],
            b"\x00\x00\x00\x18ftypM4A ",
            b"hello world",
        ] {
            assert_eq!(
                decode_audio(bytes).err(),
                Some(AudioError::UnsupportedFormat),
                "{bytes:?} 不该被解码成功"
            );
        }
    }

    #[cfg(feature = "wav")]
    #[test]
    fn streaming_seek_matches_the_static_decode() {
        // 流式通路（混音器的 `StreamVoice` 走它）与一次性解码必须给出同一批样本。
        let static_data = decode_audio(&fixture("tone_1s.wav")).expect("WAV 解码");
        let mut dec = SymphoniaDecoder::new(&fixture("tone_1s.wav"), Container::Wav).expect("开流");
        assert_eq!(dec.format(), static_data.format);

        let mut block = vec![0.0f32; 512 * 2];
        let mut head = 0u32;
        loop {
            let n = Decoder::read(&mut dec, &mut block).expect("顺序读");
            if n == 0 {
                break;
            }
            for f in 0..n {
                let got = block[f * 2..f * 2 + 2].to_vec();
                let want = [
                    static_data.at(head + f as u32, 0),
                    static_data.at(head + f as u32, 1),
                ];
                assert_eq!(Some(got[0]), want[0], "帧 {} 不符", head + f as u32);
                assert_eq!(Some(got[1]), want[1]);
            }
            head += n as u32;
        }
        assert_eq!(head, static_data.frames());

        // 跳到中段再读：容器只能跳到包边界，PCM 的边界就是样本本身 ⇒ 逐样本相等。
        const TARGET: u64 = 12_345;
        Decoder::seek_sample(&mut dec, TARGET).expect("seek");
        let n = Decoder::read(&mut dec, &mut block).expect("seek 后读");
        assert_eq!(n, 512);
        for f in 0..n {
            let want = static_data.at(TARGET as u32 + f as u32, 0);
            assert_eq!(Some(block[f * 2]), want, "seek 后第 {f} 帧不符");
        }
        // 往回跳（重放循环点）也要落得准。
        Decoder::seek_sample(&mut dec, 8820).expect("反向 seek");
        let n = Decoder::read(&mut dec, &mut block).expect("反向 seek 后读");
        assert_eq!(Some(block[0]), static_data.at(8820, 0));
        assert!(n > 0);
    }
}
