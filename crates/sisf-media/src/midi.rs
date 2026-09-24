//! MIDI：midly 解析 SMF + rustysynth 的 SF2 合成器（feature `midi`，T7）。
//!
//! 三件事说清楚：
//! 1. **引擎不自带 SoundFont**。SF2 是几十 MB 的版权采样包，随引擎分发既不合规也没必要。
//!    因此 [`SoundBank::from_sf2`] 的字节必须由调用方（一般是 VFS）提供，缺它就返回
//!    [`AudioError::MissingSoundBank`] —— **不 panic、不静音**，报错文案里写着该做什么。
//! 2. **MIDI 只能是流式语音**。一首歌合成出来是几十 MB 的驻留样本，所以这里只实现
//!    [`Decoder`]，由 `sisf-audio` 的流式语音（`VoiceData::Stream`）逐块拉。它和 OGG/MP3/WAV
//!    进同一个混音器，通道/音量/淡入淡出全部通用。
//! 3. **兜底路径**：项目里根本没有 SF2、又要在最低配的端上放 BGM 时，离线把 `.mid`
//!    预转成 `.ogg`（`fluidsynth -F out.wav bank.sf2 song.mid` 再 `ffmpeg -i out.wav -c:a
//!    libvorbis -q 0 song.ogg`），走 [`Container::Ogg`](crate::audio::Container::Ogg) 的
//!    驻留解码链。代价是丢掉实时变音量/变速，换来零依赖与零合成开销。
//!
//! 时间单位沿用音频侧约定：一切位置都是**源帧**（一帧 = 2 个样本，rustysynth 只出立体声）。

use std::sync::Arc;

use midly::{MetaMessage, MidiMessage, Smf, TrackEventKind};
use rustysynth::{
    MidiFile, MidiFileLoopType, MidiFileSequencer, SoundFont, Synthesizer, SynthesizerSettings,
};

use crate::audio::{AudioError, AudioFormat, Decoder};

/// rustysynth 接受的合成采样率下界（低于它 `Synthesizer::new` 直接报错）。
const MIN_SYNTH_RATE: u32 = 16_000;
/// 上界同样由后端硬性规定。
const MAX_SYNTH_RATE: u32 = 192_000;
/// SMF 规范默认速度：120 BPM = 每拍 500000µs。没写 tempo 元事件的 MIDI 按它走。
const DEFAULT_TEMPO_US: f64 = 500_000.0;
/// 非循环播放时，最后一个 MIDI 事件之后再渲染多久（让 release 段和混响散尽）。
const TAIL_SECONDS: f64 = 1.5;

/// 一份 SF2 音色库。内部是 `Arc<SoundFont>`，所以 [`Clone`] 很便宜，可以在多路语音间共享。
#[derive(Clone)]
pub struct SoundBank {
    font: Arc<SoundFont>,
}

impl SoundBank {
    /// 从 SF2 字节建库（VFS 读出来的 `Vec<u8>` 直接借给它就行，不需要落盘）。
    ///
    /// 解析失败一律降级成 [`AudioError::Invalid`]（错误串带 rustysynth 的原文）——
    /// 容器认不出与文件坏掉是两回事，所以这里**不**返回 `UnsupportedFormat`。
    pub fn from_sf2(bytes: &[u8]) -> Result<Self, AudioError> {
        let mut reader = bytes;
        match SoundFont::new(&mut reader) {
            Ok(font) => Ok(Self {
                font: Arc::new(font),
            }),
            Err(e) => Err(AudioError::Invalid(format!("SF2 解析失败: {e}"))),
        }
    }

    /// 音色库里的 preset 个数（0 = 一个音色都没有，播了也是静音，诊断用）。
    pub fn preset_count(&self) -> usize {
        self.font.get_presets().len()
    }

    fn synthesizer(&self, sample_rate: u32) -> Result<Synthesizer, AudioError> {
        let settings = SynthesizerSettings::new(clamp_rate(sample_rate) as i32);
        // 块大小/复音数都留默认（64 帧 / 64 声）：混音器一次要 1024 帧量级，
        // sequencer 内部自己按块推进，调用方不需要关心。
        Synthesizer::new(&self.font, &settings)
            .map_err(|e| AudioError::Invalid(format!("合成器初始化失败: {e}")))
    }
}

/// MIDI 的循环方式。
///
/// 两条完全不同的实现路径，选哪条取决于"谁负责回绕"：
/// - [`MidiLoop::Once`]：sequencer 不循环。[`Decoder::total_frames`] 报出确切长度，
///   于是 BGM 通道由**混音器**在 `loop_end` 处回绕（走 [`Decoder::seek_sample`] 的 `0`），
///   SE/ME 通道则播到尾自灭。
/// - 其余：sequencer **内部**循环，`total_frames` 报 `None`，混音器于是永不回绕、语音永不
///   死亡（也就不会为回绕付一次 `seek`）。循环点由文件里的 CC 标记决定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MidiLoop {
    /// 不内部循环：播完 `total_frames` 即止（交给混音器决定回绕还是收掉）。
    Once,
    /// sequencer 内部整段回绕（等价于从 tick 0 循环）。
    Whole,
    /// RPG Maker 风格：CC#111 标循环起点。
    RpgMaker,
    /// Incredible Machine 风格：CC#110/#111 标起止。
    IncredibleMachine,
    /// Final Fantasy 风格：CC#116/#117 标起止。
    FinalFantasy,
    /// 显式给定循环起点（tick）。
    Tick(usize),
}

impl MidiLoop {
    /// 交给 rustysynth 的回绕方式；`None` = 让 sequencer 不循环（[`MidiLoop::Once`]）。
    const fn play_loop(self) -> bool {
        !matches!(self, Self::Once)
    }

    fn to_synth(self) -> MidiFileLoopType {
        match self {
            Self::Once | Self::Whole => MidiFileLoopType::LoopPoint(0),
            Self::RpgMaker => MidiFileLoopType::RpgMaker,
            Self::IncredibleMachine => MidiFileLoopType::IncredibleMachine,
            Self::FinalFantasy => MidiFileLoopType::FinalFantasy,
            Self::Tick(t) => MidiFileLoopType::LoopPoint(t),
        }
    }
}

/// 建 MIDI 语音的参数。
#[derive(Clone, Copy, Debug)]
pub struct MidiOptions {
    /// 期望的输出采样率。超出 rustysynth 的 `16k..=192k` 会被夹住，差额由混音器线性重采样。
    pub sample_rate: u32,
    /// 循环方式，见 [`MidiLoop`]。
    pub loop_mode: MidiLoop,
}

impl Default for MidiOptions {
    fn default() -> Self {
        Self {
            sample_rate: 44_100,
            loop_mode: MidiLoop::Whole,
        }
    }
}

impl MidiOptions {
    pub const fn sample_rate(mut self, rate: u32) -> Self {
        self.sample_rate = rate;
        self
    }
    pub const fn loop_mode(mut self, mode: MidiLoop) -> Self {
        self.loop_mode = mode;
        self
    }
}

/// `.mid` 的元信息（不碰合成器，因此**不需要 SF2** 就能拿到）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MidiInfo {
    /// SMF 格式：0 = 单轨，1 = 多轨同时，2 = 多轨各自独立。
    pub format: u8,
    pub tracks: usize,
    /// 每拍 tick 数（`Timing::Timecode` 的文件为 0，此时 [`MidiInfo::duration_seconds`] 按帧率算）。
    pub ticks_per_beat: u32,
    /// 时长（秒）：按 tempo 变更加权积分，所以变速的曲子也准。
    pub duration_seconds: f64,
    /// `NoteOn`（非零力度）事件个数。0 = 这文件其实没有音符，播了也只有静音。
    pub notes: usize,
}

/// 只用 midly 扫一遍 SMF，拿格式/时长/音符数。
///
/// 这一步同时是**输入校验**：rustysynth 的 `MidiFile::get_length` 对空轨会 panic，
/// 所以先把"没有事件""认不出格式"的文件在这里挡掉（返回 `AudioError::Invalid`）。
pub fn midi_info(bytes: &[u8]) -> Result<MidiInfo, AudioError> {
    let smf = Smf::parse(bytes).map_err(|e| AudioError::Invalid(format!("SMF 解析失败: {e}")))?;
    if smf.tracks.is_empty() {
        return Err(AudioError::Invalid("MIDI 没有任何轨".into()));
    }
    let format = match smf.header.format {
        midly::Format::SingleTrack => 0,
        midly::Format::Parallel => 1,
        midly::Format::Sequential => 2,
    };

    let mut notes = 0usize;
    let mut end = 0u64;
    let mut tempo: Vec<(u64, f64)> = Vec::new();
    for track in &smf.tracks {
        // 每条轨的 tick 各自从 0 起算（格式 1 的多轨是并行的，绝对位置 = delta 累加）。
        let mut last = 0u64;
        for ev in track {
            last += u64::from(ev.delta.as_int());
            end = end.max(last);
            match &ev.kind {
                TrackEventKind::Midi { message, .. } => {
                    if matches!(message, MidiMessage::NoteOn { vel, .. } if vel.as_int() > 0) {
                        notes += 1;
                    }
                }
                TrackEventKind::Meta(MetaMessage::Tempo(us)) => {
                    tempo.push((last, f64::from(us.as_int())))
                }
                _ => (),
            }
        }
    }
    if notes == 0 {
        return Err(AudioError::Invalid(
            "MIDI 里没有任何 NoteOn，播出来只会是静音".into(),
        ));
    }
    tempo.sort_by_key(|(t, _)| *t);

    let duration_seconds = match smf.header.timing {
        // 时间码计时：一拍不是常数，改成"每 tick = 1/(fps·subframe) 秒"，tempo 元事件无效。
        midly::Timing::Timecode(fps, subframes) => {
            let ticks_per_second = f64::from(fps.as_f32()) * f64::from(subframes.max(1));
            end as f64 / ticks_per_second
        }
        midly::Timing::Metrical(tpb) => {
            let tpb = u64::from(tpb.as_int()).max(1);
            // 沿 tick 轴分段积分：每段时长 = Δtick / 每拍tick × 每拍µs。
            let (mut tick, mut us_per_beat, mut secs) = (0u64, DEFAULT_TEMPO_US, 0.0f64);
            for &(at, tempo_us) in &tempo {
                if at > tick {
                    secs += (at - tick) as f64 / tpb as f64 * us_per_beat / 1e6;
                    tick = at;
                }
                us_per_beat = tempo_us;
            }
            if end > tick {
                secs += (end - tick) as f64 / tpb as f64 * us_per_beat / 1e6;
            }
            secs
        }
    };

    let ticks_per_beat = match smf.header.timing {
        midly::Timing::Metrical(tpb) => u32::from(tpb.as_int()),
        midly::Timing::Timecode(_, _) => 0,
    };
    Ok(MidiInfo {
        format,
        tracks: smf.tracks.len(),
        ticks_per_beat,
        duration_seconds,
        notes,
    })
}

/// 一路 MIDI 的流式合成语音：`MidiFileSequencer` + 交织收口。
///
/// rustysynth 只往**两块等长互不重叠**的单声道缓冲里渲染（它内部按 `block_size` 推进），
/// 所以这里持有一对临时缓冲，读完再交织成 [`Decoder`] 约定的交织立体声。
#[derive(Debug)]
pub struct MidiDecoder {
    seq: MidiFileSequencer,
    file: Arc<MidiFile>,
    format: AudioFormat,
    /// 内部循环时为 `None`：流没有尽头，混音器因此不回绕也不收语音。
    total: Option<u64>,
    left: Vec<f32>,
    right: Vec<f32>,
    rendered: u64,
    play_loop: bool,
}

impl MidiDecoder {
    /// 用 SF2 音色库合成一段 SMF。
    ///
    /// `bank` 为 `None` 时返回 [`AudioError::MissingSoundBank`]（引擎不自带 SoundFont，
    /// 见模块文档第 1 条）。这是**预期路径**而不是异常：上层照样能把 MIDI 请求以错误形式
    /// 冒给日志，不必为"没音色库"写特判。
    pub fn new(
        bytes: &[u8],
        bank: Option<&SoundBank>,
        options: MidiOptions,
    ) -> Result<Self, AudioError> {
        let bank = bank.ok_or_else(|| {
            AudioError::MissingSoundBank(
                "引擎不带 SoundFont：请用 VFS 载入 .sf2 后 SoundBank::from_sf2，\
                 或离线把 .mid 预转成 .ogg（见 sisf_media::midi 模块文档）"
                    .into(),
            )
        })?;
        let info = midi_info(bytes)?;
        let rate = clamp_rate(options.sample_rate);
        let synth = bank.synthesizer(rate)?;
        let mut reader = bytes;
        let file = Arc::new(
            MidiFile::new_with_loop_type(&mut reader, options.loop_mode.to_synth())
                .map_err(|e| AudioError::Invalid(format!("MIDI 载入失败: {e}")))?,
        );
        let play_loop = options.loop_mode.play_loop();
        let mut seq = MidiFileSequencer::new(synth);
        seq.play(&file, play_loop);
        let total = (!play_loop)
            .then(|| ((info.duration_seconds + TAIL_SECONDS) * rate as f64).ceil() as u64);
        Ok(Self {
            seq,
            file,
            format: AudioFormat::stereo(rate),
            total,
            left: Vec::new(),
            right: Vec::new(),
            rendered: 0,
            play_loop,
        })
    }

    /// 当前播放位置（秒，不含尾音）。诊断与进度条用。
    pub fn position_seconds(&self) -> f64 {
        self.seq.get_position().min(self.song_seconds())
    }

    /// 是否已把最后一个事件发完（内部循环时恒为 `false`）。
    pub fn end_of_sequence(&self) -> bool {
        self.seq.end_of_sequence()
    }

    fn song_seconds(&self) -> f64 {
        // `MidiFile::get_length` 对空轨会 panic，而轨非空已由 `midi_info` 保证。
        self.file.get_length()
    }
}

impl Decoder for MidiDecoder {
    fn format(&self) -> AudioFormat {
        self.format
    }

    /// 交织写出一块。rustysynth 只出立体声，所以步长恒为 2（[`Self::format`] 也恒报 2 通道）。
    fn read(&mut self, out: &mut [f32]) -> Result<usize, AudioError> {
        let mut want = out.len() / 2;
        if let Some(total) = self.total {
            let left = total.saturating_sub(self.rendered) as usize;
            want = want.min(left);
        }
        if want == 0 {
            return Ok(0);
        }
        self.left.clear();
        self.left.resize(want, 0.0);
        self.right.clear();
        self.right.resize(want, 0.0);
        self.seq.render(&mut self.left, &mut self.right);
        for (i, (l, r)) in self.left.iter().zip(&self.right).enumerate() {
            out[i * 2] = *l;
            out[i * 2 + 1] = *r;
        }
        self.rendered += want as u64;
        Ok(want)
    }

    /// 只有"回到 0"有意义（`Once` 模式下混音器在 `loop_end` 处的回绕）。
    ///
    /// MIDI 是实时合成出来的，中间位置没法随机访问 —— 要跳进曲子中段只能把事件快进一遍，
    /// 那既不是 rustysynth 的能力也不该假装是，所以明确报 [`AudioError::Seek`]。
    fn seek_sample(&mut self, frame: u64) -> Result<(), AudioError> {
        if frame != 0 {
            return Err(AudioError::Seek {
                frame,
                why: format!(
                    "MIDI 只能从头重建（当前在 {}s / 全长 {}s）",
                    self.position_seconds(),
                    self.song_seconds()
                ),
            });
        }
        self.seq.stop();
        // `play` 会 reset 合成器并把事件游标归零，等价于重新起播。
        self.seq.play(&self.file, self.play_loop);
        self.rendered = 0;
        Ok(())
    }

    fn total_frames(&self) -> Option<u64> {
        self.total
    }

    /// 恒为 `None`：MIDI 的循环区间是 tick 而不是帧（要换算就得先解完一遍），
    /// 交给 sequencer 内部循环更准。
    fn loop_points(&self) -> Option<(u64, u64)> {
        None
    }

    fn block_frames(&self) -> usize {
        1024
    }
}

fn clamp_rate(rate: u32) -> u32 {
    rate.clamp(MIN_SYNTH_RATE, MAX_SYNTH_RATE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read(&path).unwrap_or_else(|e| panic!("读 {path} 失败: {e}（先跑 gen_audio.py）"))
    }

    /// 用户自备的 SF2（没有就跳过需要音色库的那几条测试）。
    fn sf2() -> Option<Vec<u8>> {
        std::env::var("SISF_SF2")
            .ok()
            .and_then(|p| std::fs::read(p).ok())
    }

    #[test]
    fn midi_info_reads_the_fixture_without_a_soundbank() {
        let info = midi_info(&fixture("jingle.mid")).expect("fixture 必须可解析");
        assert_eq!(info.format, 0, "fixture 是格式 0 单轨");
        assert_eq!(info.tracks, 1);
        assert_eq!(info.ticks_per_beat, 480);
        assert_eq!(info.notes, 4, "C4 E4 G4 C5");
        // 最后一个事件是 tick 1800 的 note-off（1440 起音 + 360），120bpm → 1.875s。
        assert!(
            (info.duration_seconds - 1.875).abs() < 0.001,
            "实际 {}",
            info.duration_seconds
        );
    }

    #[test]
    fn midi_info_rejects_garbage_instead_of_panicking() {
        // 不是 SMF（缺 MThd）：报错，不 panic。
        assert!(matches!(
            midi_info(b"not a midi file at all"),
            Err(AudioError::Invalid(_))
        ));
        // 头对但没有音符轨。
        assert!(matches!(
            midi_info(&fixture("tone_1s.wav")),
            Err(AudioError::Invalid(_))
        ));
        // 头合法、轨数 0：不能 panic。
        let empty_header = b"MThd\0\0\0\x06\0\0\0\0\x01\0".to_vec();
        assert!(midi_info(&empty_header).is_err());
    }

    #[test]
    fn missing_soundbank_is_an_error_not_a_panic() {
        let e = MidiDecoder::new(&fixture("jingle.mid"), None, MidiOptions::default())
            .expect_err("没给 SF2 必须报错");
        assert!(matches!(e, AudioError::MissingSoundBank(_)), "实际: {e:?}");
        // 文案里得写着解决办法，否则调用方无从下手。
        assert!(e.to_string().contains("SoundFont"));
    }

    #[test]
    fn bad_sf2_bytes_are_rejected() {
        assert!(matches!(
            SoundBank::from_sf2(b"RIFF\x04\0\0\0sfbk"),
            Err(AudioError::Invalid(_))
        ));
        assert!(matches!(
            SoundBank::from_sf2(&[]),
            Err(AudioError::Invalid(_))
        ));
    }

    /// `MidiFileLoopType` 没实现 `Debug`，断言前自己字符串化。
    fn synth_repr(mode: MidiLoop) -> String {
        match mode.to_synth() {
            MidiFileLoopType::LoopPoint(t) => format!("LoopPoint({t})"),
            MidiFileLoopType::RpgMaker => "RpgMaker".into(),
            MidiFileLoopType::IncredibleMachine => "IncredibleMachine".into(),
            MidiFileLoopType::FinalFantasy => "FinalFantasy".into(),
            _ => "?".into(),
        }
    }

    #[test]
    fn loop_mode_decides_who_wraps() {
        // Once = sequencer 不循环（长度有限，回绕交给混音器）；其余 = 内部循环。
        assert!(!MidiLoop::Once.play_loop());
        assert!(MidiLoop::Whole.play_loop());
        assert!(MidiLoop::RpgMaker.play_loop());
        assert_eq!(synth_repr(MidiLoop::Tick(480)), "LoopPoint(480)");
        assert_eq!(synth_repr(MidiLoop::FinalFantasy), "FinalFantasy");
        // Once 也给出一个合法但用不上的回绕点：sequencer 只会走 `play_loop=false` 分支。
        assert_eq!(synth_repr(MidiLoop::Once), "LoopPoint(0)");
    }

    #[test]
    #[ignore = "需要用户自备的 SF2：SISF_SF2=/path/to/bank.sf2 cargo test -p sisf-media --features midi -- --ignored"]
    fn synthesizes_the_fixture_and_is_deterministic() {
        let bytes = sf2().expect("设 SISF_SF2 指向一个 .sf2");
        let bank = SoundBank::from_sf2(&bytes).expect("SF2 应能解析");
        assert!(bank.preset_count() > 0, "空音色库？");
        let mid = fixture("jingle.mid");

        let mut a = MidiDecoder::new(
            &mid,
            Some(&bank),
            MidiOptions::default().loop_mode(MidiLoop::Once),
        )
        .expect("有 SF2 就该建成");
        assert_eq!(a.channels(), 2);
        assert_eq!(a.sample_rate(), 44_100);
        assert!(a.total_frames().unwrap() > 44_100, "2s 曲子 + 尾音");
        assert_eq!(
            a.loop_points(),
            None,
            "循环交给 sequencer/混音器，不报循环点"
        );

        let mut out = vec![0.0f32; 2048 * 2];
        let mut got = Vec::new();
        loop {
            let n = a.read(&mut out).unwrap();
            if n == 0 {
                break;
            }
            got.extend_from_slice(&out[..n * 2]);
            assert!(got.len() < 1 << 26, "非循环模式却读不完？");
        }
        let peak = got.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.01, "有音色库就该出声，峰值 {peak}");
        assert!(a.end_of_sequence(), "读完 total_frames 后事件游标该到尾");
        assert!(a.position_seconds() <= a.song_seconds() + f64::EPSILON);

        // 逐位一致：同一段 MIDI + 同一个音色库，两次渲染必须完全相同（确定性是四端复验的前提）。
        let mut b = MidiDecoder::new(
            &mid,
            Some(&bank),
            MidiOptions::default().loop_mode(MidiLoop::Once),
        )
        .unwrap();
        let mut other = vec![0.0f32; got.len()];
        let mut done = 0usize;
        while done < other.len() {
            let n = b.read(&mut other[done..]).unwrap();
            assert_ne!(n, 0, "第二次渲染提前结束：sequencer 不干净");
            done += n * 2;
        }
        assert_eq!(got, other, "同参数两次渲染必须逐位一致");
    }

    #[test]
    #[ignore = "同 synthesizes_the_fixture_and_is_deterministic，需要 SISF_SF2"]
    fn looping_mode_never_ends_and_restarts_only_at_zero() {
        let bank = SoundBank::from_sf2(&sf2().expect("设 SISF_SF2")).unwrap();
        let mut d = MidiDecoder::new(
            &fixture("jingle.mid"),
            Some(&bank),
            MidiOptions::default().loop_mode(MidiLoop::Whole),
        )
        .unwrap();
        assert_eq!(d.total_frames(), None, "内部循环时流没有尽头");
        assert!(!d.end_of_sequence(), "内部循环时游标会被拉回循环点");
        let mut out = vec![0.0f32; 44_100 * 2 * 3]; // 3 倍曲长
        assert_eq!(d.read(&mut out).unwrap(), 44_100 * 3);
        assert!(
            d.seek_sample(0).is_ok(),
            "回到 0 必须支持（混音器的回绕走这条路）"
        );
        let e = d.seek_sample(1).expect_err("中段跳转必须报错");
        assert!(matches!(e, AudioError::Seek { frame: 1, .. }), "{e:?}");
    }

    #[test]
    fn sample_rate_is_clamped_into_the_synthesizer_range() {
        assert_eq!(clamp_rate(8_000), 16_000);
        assert_eq!(clamp_rate(44_100), 44_100);
        assert_eq!(clamp_rate(1_000_000), 192_000);
    }

    #[test]
    fn bank_is_shareable_across_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SoundBank>();
    }
}
