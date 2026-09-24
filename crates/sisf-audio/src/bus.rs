//! [`AudioBus`] —— 混音器 + 设备 pump 的门面（应用侧唯一需要碰的音频类型）。
//!
//! 分工：[`Mixer`] 只认"帧数"这个时钟，[平台 pump](crate) 只认设备。`AudioBus` 把它们钉在
//! 一起，并把 `sisf-media` 的解码结果变成一路语音。调用节奏由**游戏**决定：
//! 每次 `App::tick` 调一次 [`AudioBus::pump`]（不是每渲染帧 —— 混音与渲染帧率无关，
//! tick 停了声音也就停了，这正是挂起时想要的语义）。
//!
//! 三条刻意的形状：
//! 1. **开不出设备不算错误**。`open()` 永不失败：拿不到输出设备时 pump 走"静音混音"，
//!    混音时钟照常推进、fade 照常到点、SE 照常回收。无声卡机器与 CI 上游戏仍要能跑完，
//!    而"听不见"这种降级必须是可观测的（[`AudioBus::stats`] 的 `device: None`），不是静默。
//! 2. **不做异步预取**。解码是同步的（symphonia/rustysynth 都是纯计算），把"从 VFS 读字节"
//!    也揽进来会让 `AudioBus` 依赖 `sisf-asset` 的异步任务模型；本层只吃 `&[u8]`，
//!    字节怎么来到应用侧决定（`AssetStore` 或一次 `read`）。
//! 3. **采样率归设备**。`pump()` 会把混音器格式对齐到设备率（换率只重算步长、不动位置），
//!    应用不需要自己谈采样率。

use std::sync::Arc;

use sisf_media::audio::{self, AudioData, AudioError, AudioFormat, Decoder};

#[cfg(feature = "midi")]
use sisf_media::midi::{MidiDecoder, MidiLoop, MidiOptions, SoundBank};

use crate::mixer::{Channel, Mixer, PlayParams, StreamVoice, VoiceData};
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
use crate::pump_native as pump;
#[cfg(all(feature = "web", target_arch = "wasm32"))]
use crate::pump_web as pump;

pub use pump::PumpStats;

/// 混音器 + 输出设备。见模块文档。
pub struct AudioBus {
    mixer: Mixer,
    pump: pump::Pump,
}

impl AudioBus {
    /// 打开输出设备（失败不报错，见模块文档第 1 条）。
    pub fn open() -> Self {
        Self::with_pump(pump::Pump::new())
    }

    /// 不碰设备的总线：混音照常推进，样本丢弃。给测试、无音频构建与"只要逻辑"的场合。
    pub fn headless() -> Self {
        Self::with_pump(pump::Pump::headless())
    }

    fn with_pump(pump: pump::Pump) -> Self {
        let format = pump.format();
        Self {
            mixer: Mixer::new(format),
            pump,
        }
    }

    pub fn mixer(&self) -> &Mixer {
        &self.mixer
    }

    /// 直接操作混音器（通道音量、SE 上限、语音表等）。
    pub fn mixer_mut(&mut self) -> &mut Mixer {
        &mut self.mixer
    }

    /// 驱动混音并把结果交给设备。**每个 tick 调一次**；不调就不出声也不推进。
    pub fn pump(&mut self) {
        self.pump.pump(&mut self.mixer);
    }

    /// 起一路驻留样本（OGG/MP3/WAV 解码结果）。
    pub fn play(
        &mut self,
        channel: Channel,
        data: Arc<AudioData>,
        params: &PlayParams,
    ) -> Result<(), AudioError> {
        self.mixer.play(channel, VoiceData::from(data), params)
    }

    /// 解码 + 起播（一次性驻留内存）。`Err` 时通道状态不变。
    pub fn play_bytes(
        &mut self,
        channel: Channel,
        bytes: &[u8],
        params: &PlayParams,
    ) -> Result<(), AudioError> {
        let data = audio::decode_audio(bytes)?;
        self.play(channel, Arc::new(data), params)
    }

    /// 起一路流式语音（MIDI 合成器，或任何不想整段驻留的长内容）。
    pub fn play_stream(
        &mut self,
        channel: Channel,
        dec: Box<dyn Decoder>,
        params: &PlayParams,
    ) -> Result<(), AudioError> {
        self.mixer
            .play(channel, VoiceData::Stream(StreamVoice::new(dec)), params)
    }

    /// 起一路 MIDI（`feature = "midi"`）。
    ///
    /// 需要用户自己的 SoundFont：引擎不附带（见 `docs/audio.md`）。`bank = None` 时返回
    /// [`AudioError::MissingSoundBank`] 并带上"离线预转 .ogg"的退路提示，不 panic。
    /// MIDI 只能顺序合成，`params.offset_frames` 只接受 0，起播偏移请改用
    /// [`AudioBus::fade_to`] / 让它自然播。
    #[cfg(feature = "midi")]
    pub fn play_midi(
        &mut self,
        channel: Channel,
        bytes: &[u8],
        bank: Option<&SoundBank>,
        loop_mode: MidiLoop,
        gain: f32,
    ) -> Result<(), AudioError> {
        let rate = self.mixer.format().sample_rate;
        let dec = MidiDecoder::new(
            bytes,
            bank,
            MidiOptions {
                sample_rate: rate,
                loop_mode,
            },
        )?;
        self.play_stream(channel, Box::new(dec), &PlayParams::default().gain(gain))
    }

    /// 立即停止通道（不淡出）。
    pub fn stop(&mut self, channel: Channel) {
        self.mixer.stop(channel);
    }

    /// 淡出并在结束时停止（RGSS 的 `bgm_stop(fade_length)`）。
    pub fn fade(&mut self, channel: Channel, ms: f32) {
        self.mixer.fade(channel, ms);
    }

    /// 淡入到目标增益（RGSS 的 `bgm_play(.., fade_length)` 那一侧）。
    pub fn fade_to(&mut self, channel: Channel, target: f32, ms: f32) {
        self.mixer.fade_to(channel, target, ms);
    }

    pub fn pause(&mut self, channel: Channel) {
        self.mixer.pause(channel);
    }

    pub fn resume(&mut self, channel: Channel) {
        self.mixer.resume(channel);
    }

    pub fn volume(&self, channel: Channel) -> f32 {
        self.mixer.volume(channel)
    }

    pub fn set_volume(&mut self, channel: Channel, volume: f32) {
        self.mixer.set_volume(channel, volume);
    }

    pub fn set_master_gain(&mut self, gain: f32) {
        self.mixer.set_master_gain(gain);
    }

    /// 挂起：混音冻结（位置不动）+ 设备停止取数。Android `Suspended` / 页面隐藏走这里。
    ///
    /// 返回设备侧的失败原因（`Some` = 硬件没配合，混音冻结仍然生效）。
    pub fn suspend(&mut self) -> Option<String> {
        self.mixer.suspend();
        self.pump.set_output_active(false)
    }

    /// 解除 [`AudioBus::suspend`]（命名与 [`Mixer::resume_all`] 对齐：通道级暂停是独立状态，
    /// 要各自 [`AudioBus::resume`]）。
    pub fn resume_all(&mut self) -> Option<String> {
        self.mixer.resume_all();
        self.pump.set_output_active(true)
    }

    pub fn is_suspended(&self) -> bool {
        self.mixer.is_suspended()
    }

    /// 当前混音输出格式（= 设备格式；`pump()` 会把它同步给混音器）。
    pub fn output_format(&self) -> AudioFormat {
        self.mixer.format()
    }

    /// 设备侧可观测状态（听测/延迟/underrun 判据）。
    pub fn stats(&self) -> PumpStats {
        self.pump.stats()
    }

    /// 累计"设备要样本而混音器没及时给到"的量（爆音/卡顿的直接前兆）。
    ///
    /// 两端单位不同是事实：native 数**样本**（cpal 回调补零的个数），web 数**帧**
    /// （worklet 取到静音的帧数）。判据相同 —— 稳态播放时它该几乎不涨。
    /// 至于"设备换过没有"，两端口径差得更大（cpal 有 `StreamError` 计数，WebAudio 只有
    /// `state`），所以留在 [`AudioBus::describe`] 的一行文字里，不硬凑成一个数。
    pub fn underrun_units(&self) -> u64 {
        self.pump.underrun_units()
    }

    /// 一行人类可读的输出状态（示例与日志用；native/web 同格式）。
    pub fn describe(&self) -> String {
        self.pump.describe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 借用 `sisf-media` 的 fixture（同一个文件，避免仓库里存两份二进制）。
    #[cfg(any(feature = "ogg", feature = "mp3", feature = "wav", feature = "midi"))]
    fn media_fixture(name: &str) -> Vec<u8> {
        let path = format!(
            "{}/../sisf-media/tests/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read(&path).unwrap_or_else(|e| panic!("读 {path} 失败: {e}"))
    }

    fn tone(frames: u32, rate: u32) -> Arc<AudioData> {
        let samples: Vec<f32> = (0..frames as usize * 2)
            .map(|i| ((i % 500) as f32 / 1000.0) * if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        Arc::new(AudioData::one_shot(samples, AudioFormat::stereo(rate)))
    }

    #[test]
    fn bus_starts_on_the_device_format() {
        let bus = AudioBus::headless();
        assert_eq!(bus.output_format().channels, 2, "输出恒为交织立体声");
        assert_eq!(bus.mixer().voice_count(), 0);
        assert!(!bus.stats().streaming, "headless 不该报告在跑的流");
    }

    #[test]
    fn pump_advances_the_mix_and_recycles_finished_voices() {
        let mut bus = AudioBus::headless();
        bus.play(Channel::Se, tone(300, 48_000), &PlayParams::default())
            .expect("起播 SE");
        for _ in 0..20 {
            bus.pump();
        }
        assert!(
            bus.mixer().rendered_frames() >= 300,
            "20 次 pump 至少推进 20 块"
        );
        assert!(
            !bus.mixer().is_playing(Channel::Se),
            "300 帧的 SE 该在 pump 中被回收"
        );
        assert_eq!(bus.mixer().voice_count(), 0);
    }

    #[test]
    fn fade_completes_in_pump_frames_not_wall_clock() {
        let mut bus = AudioBus::headless();
        bus.play(
            Channel::Bgm,
            tone(100_000, 48_000),
            &PlayParams::default().looping(true),
        )
        .unwrap();
        // 480 帧 = 10ms@48k。headless 每次 pump 渲 256 帧。
        bus.fade(Channel::Bgm, 10.0);
        bus.pump();
        assert!(bus.mixer().is_playing(Channel::Bgm), "淡出中");
        bus.pump();
        bus.pump();
        assert!(!bus.mixer().is_playing(Channel::Bgm), "淡到 0 即停");
    }

    #[test]
    fn suspend_freezes_and_resume_continues() {
        let mut bus = AudioBus::headless();
        bus.play(
            Channel::Bgm,
            tone(100_000, 48_000),
            &PlayParams::default().looping(true),
        )
        .unwrap();
        bus.pump();
        let pos = bus.mixer().position(Channel::Bgm).unwrap();
        assert!(bus.suspend().is_none(), "headless 没有硬件可抱怨");
        for _ in 0..4 {
            bus.pump();
        }
        assert_eq!(
            bus.mixer().position(Channel::Bgm).unwrap(),
            pos,
            "挂起期间位置必须冻结"
        );
        assert!(bus.resume_all().is_none(), "headless 没有硬件可抱怨");
        assert!(!bus.is_suspended());
        bus.pump();
        assert!(
            bus.mixer().position(Channel::Bgm).unwrap() > pos,
            "恢复后继续走"
        );
    }

    /// 解码 → 起播 → pump 出声，整条链在任何 feature 组合之外都不可拆。
    #[cfg(feature = "wav")]
    #[test]
    fn decoded_wav_fixture_plays_through_the_bus() {
        let mut bus = AudioBus::headless();
        bus.play_bytes(
            Channel::Bgm,
            &media_fixture("tone_1s.wav"),
            &PlayParams::default(),
        )
        .expect("WAV 解码 + 起播");
        assert!(bus.mixer().is_playing(Channel::Bgm));
        bus.pump();
        assert!(bus.mixer().position(Channel::Bgm).unwrap() > 0);
        // fixture 是 22050Hz，输出是设备率：混音器按步长消化，不改采样率。
        assert_eq!(bus.output_format().channels, 2);
    }

    #[cfg(feature = "ogg")]
    #[test]
    fn ogg_loop_tags_survive_into_a_voice() {
        let mut bus = AudioBus::headless();
        let bytes = media_fixture("tone_2s.ogg");
        bus.play_bytes(Channel::Bgm, &bytes, &PlayParams::default().looping(true))
            .expect("OGG 解码 + 起播");
        let data = audio::decode_audio(&bytes).expect("对照解码");
        assert!(data.loops(), "fixture 该带 LOOPSTART/LOOPEND");
        // 循环点由 `Voice::new` 从 `AudioData` 取，BGM 会一直在循环区内回绕。
        for _ in 0..80 {
            bus.pump();
        }
        let pos = bus.mixer().position(Channel::Bgm).unwrap();
        assert!(
            (data.loop_start_samples as u64..data.loop_end_samples as u64).contains(&pos),
            "pump 80 次后应停在循环区内，实际帧 {pos}"
        );
    }

    #[test]
    fn garbage_bytes_leave_the_channel_untouched() {
        let mut bus = AudioBus::headless();
        bus.play(Channel::Bgm, tone(4096, 48_000), &PlayParams::default())
            .unwrap();
        let err = bus
            .play_bytes(Channel::Bgm, b"not audio at all", &PlayParams::default())
            .expect_err("垃圾字节必须报错");
        assert!(matches!(err, AudioError::UnsupportedFormat), "{err:?}");
        assert_eq!(bus.mixer().voice_count(), 1, "失败的播放不得换掉原有语音");
    }

    #[cfg(feature = "midi")]
    #[test]
    fn midi_without_a_soundbank_is_an_error_not_a_panic() {
        let mut bus = AudioBus::headless();
        let err = bus
            .play_midi(
                Channel::Bgm,
                &media_fixture("jingle.mid"),
                None,
                MidiLoop::Whole,
                1.0,
            )
            .expect_err("引擎不带 SoundFont，缺 bank 必须报错");
        assert!(matches!(err, AudioError::MissingSoundBank(_)), "{err:?}");
        assert_eq!(bus.mixer().voice_count(), 0);
    }
}
