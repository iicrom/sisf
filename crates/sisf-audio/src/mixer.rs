//! 纯 Rust 混音器：四通道（BGM/BGS/ME/SE）+ 多路 SE 复用 + fade + 循环 + 变速。
//!
//! **不碰设备**：只暴露 [`Mixer::render`]，由 `pump_native`（cpal 之外的主线程）与
//! `pump_web`（AudioWorklet 的拉取回调）在同一份代码上接各自的时钟。因此这里没有锁、
//! 没有线程、也不读任何墙钟 —— 一切时间都换算成**帧数**（`render` 的调用节拍是唯一的
//! 时钟来源），这正是"四端可跑、逐位一致"的前提（`tests::render_is_bitwise_deterministic`）。
//!
//! 输出约定：**交织立体声 f32**（`out.len()` 必须为偶数），满幅 ±1.0，收口硬夹取。
//! 单声道源复制到左右；源采样率与输出不同时按线性插值重采样
//! （`Voice.rate` = 每个输出帧消耗的源帧数 = `源率 / 出率 × pitch`）。
//!
//! 声相用"每侧常量增益"律：`left = gain·clamp(1-pan, 0..1)`、`right = gain·clamp(1+pan, 0..1)`，
//! `pan = 0` 时两侧都是 1.0（不衰减），`±1` 时一侧归零、另一侧仍是满增益（硬声相不吃 +6dB）。
//! 多路居中同相声相加仍可能超过 1.0，由收口硬夹取兜住；好处是 `-1 / 0 / +1` 三个位置的
//! 期望值一眼可读（断言靠它）。
//!
//! 与清单字段表的差异：[`Voice`] 除 `data/pos/gain/pan/loop_start/loop_end/rate` 外还带
//! - `channel`：淡出中的旧语音要知道自己原属哪路，通道音量才不会串味；
//! - `looping`：`loop_end == 总帧数` 既可能是"整段循环"也可能是"到尾即止"，光靠数值分不开；
//! - `ramp` / `id`：增益斜坡与 SE 抢占用的年龄序（`id` 单调递增，抢占取最小 = 最旧）。

use std::sync::Arc;

use sisf_media::audio::{AudioData, AudioError, AudioFormat, Decoder};

/// SE 同时在飞的最大语音数；超出时抢占最旧的一路。
pub const DEFAULT_SE_VOICES: usize = 16;
/// 交叉淡化中的旧语音上限（防止疯狂 `play` 把尾音池堆成无界队列）。
pub const MAX_TAILS: usize = 8;
/// 独占通道数（BGM/BGS/ME）。
const OWNED: usize = 3;

/// 混音通道。前三路各占**一个**语音槽（新的换掉旧的），SE 是多路复用池。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Channel {
    Bgm,
    Bgs,
    Me,
    Se,
}

impl Channel {
    pub const ALL: [Channel; 4] = [Channel::Bgm, Channel::Bgs, Channel::Me, Channel::Se];

    /// 音量/暂停表的下标。
    pub const fn index(self) -> usize {
        match self {
            Channel::Bgm => 0,
            Channel::Bgs => 1,
            Channel::Me => 2,
            Channel::Se => 3,
        }
    }

    /// 是否多路复用池（SE）。
    pub const fn shared(self) -> bool {
        matches!(self, Channel::Se)
    }

    /// [`PlayParams::looping`] 为 `None` 时该通道是否循环。
    ///
    /// 只有背景该自己接上自己：ME 是过关/结束短乐句，SE 是音效，都是播完即止
    /// （RGSS 的 `me_play` 根本没有循环参数）。
    pub const fn loops_by_default(self) -> bool {
        matches!(self, Channel::Bgm | Channel::Bgs)
    }
}

/// 一路语音的样本来源。
pub enum VoiceData {
    /// 驻留内存的整段样本（OGG/MP3/WAV 的解码结果）。
    Static(Arc<AudioData>),
    /// 流式来源（MIDI 合成器 —— 整首歌驻留是几十 MB 量级，不可取）。
    Stream(StreamVoice),
}

impl From<Arc<AudioData>> for VoiceData {
    fn from(data: Arc<AudioData>) -> Self {
        Self::Static(data)
    }
}

/// 流式语音的滑动窗口：`win` 是交织源帧，`win_base` 是 `win[0]` 的绝对源帧号。
///
/// 只有"顺序前进 + 循环回绕时 `seek_sample`"两种访问，所以后端不需要精确随机读
/// （symphonia 的 seek 只能落到包边界 —— 正因如此 OGG/MP3/WAV 一律走
/// [`VoiceData::Static`]，只有 MIDI 走这条路）。
pub struct StreamVoice {
    dec: Box<dyn Decoder>,
    win: Vec<f32>,
    scratch: Vec<f32>,
    win_base: u64,
    exhausted: bool,
}

impl StreamVoice {
    /// 用流式解码器建一路语音（窗口为空，首次取样时才真正 `read`）。
    pub fn new(dec: Box<dyn Decoder>) -> Self {
        Self {
            dec,
            win: Vec::new(),
            scratch: Vec::new(),
            win_base: 0,
            exhausted: false,
        }
    }

    fn format(&self) -> AudioFormat {
        self.dec.format()
    }

    /// 把游标定位到绝对源帧：清窗口 + `seek_sample` + 复位耗尽标记。
    ///
    /// 起播偏移和循环回绕都走这里。回绕付的是"解到最近包边界"的代价（symphonia 只能
    /// 跳到包边界），所以流式语音的循环点若落在包中间，会多丢几帧再出声。
    fn rewind_to(&mut self, frame: u64) -> Result<(), AudioError> {
        self.dec.seek_sample(frame)?;
        self.win.clear();
        self.win_base = frame;
        self.exhausted = false;
        Ok(())
    }
}

/// 一次播放的参数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayParams {
    /// 语音增益（1.0 = 满幅）。
    pub gain: f32,
    /// 声相：`-1` 全左、`0` 居中、`1` 全右。
    pub pan: f32,
    /// 音高倍率（同时改变速度）。
    pub pitch: f32,
    /// 起始偏移（源帧）。
    pub offset_frames: u32,
    /// `None` = 跟随通道（只有 BGM/BGS 默认循环，ME/SE 播完即止）；`Some` = 显式指定。
    pub looping: Option<bool>,
    /// 淡入时长（毫秒）；同时决定被换下的旧语音的淡出长度（交叉淡化）。
    pub fade_in_ms: f32,
}

impl Default for PlayParams {
    fn default() -> Self {
        Self {
            gain: 1.0,
            pan: 0.0,
            pitch: 1.0,
            offset_frames: 0,
            looping: None,
            fade_in_ms: 0.0,
        }
    }
}

impl PlayParams {
    pub const fn gain(mut self, v: f32) -> Self {
        self.gain = v;
        self
    }
    pub const fn pan(mut self, v: f32) -> Self {
        self.pan = v;
        self
    }
    pub const fn pitch(mut self, v: f32) -> Self {
        self.pitch = v;
        self
    }
    pub const fn offset(mut self, frames: u32) -> Self {
        self.offset_frames = frames;
        self
    }
    pub const fn looping(mut self, v: bool) -> Self {
        self.looping = Some(v);
        self
    }
    pub const fn fade_in(mut self, ms: f32) -> Self {
        self.fade_in_ms = ms;
        self
    }
}

/// 增益斜坡（以帧计，绝不读墙钟）。`stop_at_end && to == 0` ⇒ 语音在斜坡终点自灭。
#[derive(Clone, Copy, Debug)]
struct Ramp {
    from: f32,
    to: f32,
    elapsed: f64,
    total: f64,
    stop_at_end: bool,
}

/// 一路正在播（或正在淡出）的声音。
pub struct Voice {
    pub data: VoiceData,
    /// 源帧位置（分数；线性插值的左端点）。
    pub pos: f64,
    /// 当前增益（由淡入与 [`Mixer::fade`] 驱动）。
    pub gain: f32,
    pub pan: f32,
    /// 循环起点（源帧，含）。
    pub loop_start: u32,
    /// 循环终点（源帧，不含）。
    pub loop_end: u32,
    /// 每个输出帧消耗的源帧数。
    pub rate: f64,
    /// 原属通道（尾音按原通道音量混入）。
    pub channel: Channel,
    /// 是否会在 `loop_end` 处回绕。
    pub looping: bool,

    id: u64,
    ramp: Option<Ramp>,
    /// 取不到样本（到尾 / 流尽）后置位，本块混完即回收。
    dead: bool,
    pitch: f64,
}

impl Voice {
    fn new(
        channel: Channel,
        mut data: VoiceData,
        params: &PlayParams,
        out: AudioFormat,
        id: u64,
    ) -> Result<Self, AudioError> {
        let format = match &data {
            VoiceData::Static(d) => d.format,
            VoiceData::Stream(s) => s.format(),
        };
        if !matches!(format.channels, 1 | 2) {
            return Err(AudioError::Invalid(format!(
                "只支持 1/2 通道，实际 {}",
                format.channels
            )));
        }
        if params.pitch <= 0.0 || !params.pitch.is_finite() {
            return Err(AudioError::Invalid(format!(
                "pitch 必须是正有限数: {}",
                params.pitch
            )));
        }
        let total = match &data {
            VoiceData::Static(d) => Some(d.frames() as u64),
            VoiceData::Stream(s) => s.dec.total_frames(),
        };
        if total == Some(0) {
            return Err(AudioError::Invalid("样本长度为 0，无法播放".into()));
        }
        let meta = match &data {
            VoiceData::Static(d) if d.loops() => {
                Some((d.loop_start_samples as u64, d.loop_end_samples as u64))
            }
            VoiceData::Stream(s) => s.dec.loop_points().filter(|(s0, e)| e > s0),
            _ => None,
        };
        // 循环区间：有元数据用元数据，否则整段 [0, total)；总帧数未知 ⇒ 无法回绕。
        let (start, end) = match meta.or(total.map(|t| (0, t))) {
            Some((s, e)) if e > s => (s, e),
            _ => (0, total.unwrap_or(0)),
        };
        let looping =
            params.looping.unwrap_or(channel.loops_by_default()) && end > start && total.is_some();
        // 起始位置**可以**落在循环区之前：RPG Maker 的 BGM 有前奏（intro），要照常播完才进循环，
        // 越过 `loop_end` 才回绕到 `loop_start`。只有偏移本身已经在循环区之后才夹回来。
        let limit = if looping { end } else { total.unwrap_or(end) };
        let start_frame = (params.offset_frames as u64)
            .min(limit.saturating_sub(1))
            .min(total.unwrap_or(u64::MAX).saturating_sub(1));
        if let VoiceData::Stream(s) = &mut data {
            // 起播偏移一次性交给后端跳转，别把前面的样本解了再扔（偏移 0 不付跳转）。
            if start_frame > s.win_base {
                s.rewind_to(start_frame)?;
            }
        }
        let pos = start_frame as f64;
        let fade_frames = ms_to_frames(params.fade_in_ms.max(0.0), out.sample_rate);
        let mut voice = Self {
            data,
            pos,
            gain: if fade_frames > 0.0 { 0.0 } else { params.gain },
            pan: params.pan.clamp(-1.0, 1.0),
            loop_start: start as u32,
            loop_end: end as u32,
            rate: 1.0,
            channel,
            looping,
            id,
            ramp: (fade_frames > 0.0).then_some(Ramp {
                from: 0.0,
                to: params.gain,
                elapsed: 0.0,
                total: fade_frames,
                stop_at_end: false,
            }),
            dead: false,
            pitch: params.pitch as f64,
        };
        voice.recalc_rate(out.sample_rate.max(1));
        Ok(voice)
    }

    /// 换输出采样率后重算步长（设备变更时调）。
    fn recalc_rate(&mut self, out_rate: u32) {
        let src = match &self.data {
            VoiceData::Static(d) => d.format.sample_rate,
            VoiceData::Stream(s) => s.format().sample_rate,
        };
        self.rate = src as f64 / out_rate.max(1) as f64 * self.pitch;
    }

    fn src_channels(&self) -> usize {
        match &self.data {
            VoiceData::Static(d) => d.format.channels as usize,
            VoiceData::Stream(s) => s.format().channels as usize,
        }
    }

    fn start_ramp(&mut self, to: f32, frames: f64, stop_at_end: bool) {
        self.ramp = Some(Ramp {
            from: self.gain,
            to,
            elapsed: 0.0,
            total: frames.max(1.0),
            stop_at_end,
        });
    }

    /// 本块起点/终点的增益（块内线性插值，免掉 20ms 一级的台阶）。
    fn gain_edges(&self, frames: usize) -> (f32, f32) {
        let Some(r) = &self.ramp else {
            return (self.gain, self.gain);
        };
        let t0 = (r.elapsed / r.total).min(1.0) as f32;
        let t1 = ((r.elapsed + frames as f64) / r.total).min(1.0) as f32;
        (r.from + (r.to - r.from) * t0, r.from + (r.to - r.from) * t1)
    }

    fn advance_ramp(&mut self, frames: usize) {
        let Some(r) = self.ramp.as_mut() else {
            return;
        };
        r.elapsed += frames as f64;
        let t = (r.elapsed / r.total).min(1.0) as f32;
        self.gain = r.from + (r.to - r.from) * t;
        if r.elapsed >= r.total {
            let stop = r.stop_at_end && r.to <= 0.0;
            self.ramp = None;
            if stop {
                self.dead = true;
            }
        }
    }

    /// 位置回绕；流式来源在这里付一次 `seek_sample`。
    fn wrap(&mut self) -> Result<(), AudioError> {
        let start = self.loop_start as f64;
        let end = self.loop_end as f64;
        if self.pos < end {
            return Ok(());
        }
        let len = (end - start).max(1.0);
        self.pos = start + (self.pos - start) % len;
        if let VoiceData::Stream(s) = &mut self.data {
            s.rewind_to(self.pos as u64)?;
        }
        Ok(())
    }

    /// 取一帧一通道；`None` = 越出可播范围。
    fn sample(&mut self, frame: u64, ch: usize) -> Result<Option<f32>, AudioError> {
        let chn = self.src_channels();
        let ch = if chn == 1 { 0 } else { ch };
        match &mut self.data {
            VoiceData::Static(d) => {
                let mut i = frame;
                let start = self.loop_start as u64;
                let end = self.loop_end as u64;
                if self.looping && i >= end && end > start {
                    i = start + (i - start) % (end - start);
                }
                Ok(d.at(u32::try_from(i).unwrap_or(u32::MAX), ch))
            }
            VoiceData::Stream(s) => {
                let StreamVoice {
                    dec,
                    win,
                    scratch,
                    win_base,
                    exhausted,
                } = s;
                loop {
                    let need = (frame + 1) as usize * chn;
                    let have = *win_base as usize * chn + win.len();
                    if need <= have {
                        let off = (frame as usize - *win_base as usize) * chn + ch;
                        return Ok(win.get(off).copied());
                    }
                    if *exhausted {
                        return Ok(None);
                    }
                    // 位置单调向前 ⇒ frame 之前的源帧都已消费，可以放心丢头。
                    let skip = (frame.saturating_sub(*win_base) as usize * chn).min(win.len());
                    win.drain(..skip);
                    *win_base += (skip / chn) as u64;
                    scratch.clear();
                    scratch.resize(dec.block_frames().max(1) * chn, 0.0);
                    let got = dec.read(scratch)?;
                    if got == 0 {
                        *exhausted = true;
                    } else {
                        scratch.truncate(got * chn);
                        win.append(scratch);
                    }
                }
            }
        }
    }

    /// 把本语音混进 `out`（交织立体声）。
    fn mix_into(
        &mut self,
        out: &mut [f32],
        extra_gain: f32,
        master: f32,
    ) -> Result<(), AudioError> {
        let frames = out.len() / 2;
        if frames == 0 || self.dead {
            return Ok(());
        }
        let (g0, g1) = self.gain_edges(frames);
        let total = match &self.data {
            VoiceData::Static(d) => d.frames() as u64,
            VoiceData::Stream(s) => s.dec.total_frames().unwrap_or(u64::MAX),
        };
        for i in 0..frames {
            if self.looping {
                self.wrap()?;
            } else if self.pos >= total as f64 {
                self.dead = true;
                break;
            }
            let f0 = self.pos as u64;
            let frac = (self.pos - f0 as f64) as f32;
            let (Some(a), Some(c)) = (self.sample(f0, 0)?, self.sample(f0, 1)?) else {
                self.dead = true;
                break;
            };
            let b = self.sample(f0 + 1, 0)?.unwrap_or(a);
            let d = self.sample(f0 + 1, 1)?.unwrap_or(c);
            let t = i as f32 / frames as f32;
            let gain = (g0 + (g1 - g0) * t) * extra_gain * master;
            out[i * 2] += (a + (b - a) * frac) * gain * (1.0 - self.pan).clamp(0.0, 1.0);
            out[i * 2 + 1] += (c + (d - c) * frac) * gain * (1.0 + self.pan).clamp(0.0, 1.0);
            self.pos += self.rate;
        }
        self.advance_ramp(frames);
        Ok(())
    }
}

/// 毫秒 → 输出帧数（混音器唯一的"时间 → 帧"换算，四端一致）。
pub fn ms_to_frames(ms: f32, sample_rate: u32) -> f64 {
    ms.max(0.0) as f64 * sample_rate.max(1) as f64 / 1000.0
}

/// 四通道混音器。见模块文档。
pub struct Mixer {
    format: AudioFormat,
    /// BGM/BGS/ME 各自的独占槽。
    slots: [Option<Voice>; OWNED],
    se: Vec<Voice>,
    /// 被新语音换下、正在淡出的旧语音。
    tails: Vec<Voice>,
    volume: [f32; 4],
    master_gain: f32,
    paused: [bool; 4],
    suspended: bool,
    max_se_voices: usize,
    next_id: u64,
    rendered_frames: u64,
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new(AudioFormat::stereo(44_100))
    }
}

impl Mixer {
    /// 输出格式里的 `channels` 恒被夹成 2（混音输出就是交织立体声）。
    pub fn new(format: AudioFormat) -> Self {
        Self {
            format: AudioFormat {
                channels: 2,
                sample_rate: format.sample_rate.max(1),
            },
            slots: [None, None, None],
            se: Vec::new(),
            tails: Vec::new(),
            volume: [1.0; 4],
            master_gain: 1.0,
            paused: [false; 4],
            suspended: false,
            max_se_voices: DEFAULT_SE_VOICES,
            next_id: 0,
            rendered_frames: 0,
        }
    }

    pub fn format(&self) -> AudioFormat {
        self.format
    }

    /// 换输出格式（设备变更）：位置不动，只重算各语音步长。
    pub fn set_format(&mut self, format: AudioFormat) {
        self.format = AudioFormat {
            channels: 2,
            sample_rate: format.sample_rate.max(1),
        };
        let rate = self.format.sample_rate;
        self.each_mut(|v| v.recalc_rate(rate));
    }

    /// 已渲染的输出帧总数（混音器自己的时钟；确定性测试与延迟统计都靠它）。
    pub fn rendered_frames(&self) -> u64 {
        self.rendered_frames
    }

    pub fn master_gain(&self) -> f32 {
        self.master_gain
    }

    pub fn set_master_gain(&mut self, gain: f32) {
        self.master_gain = gain.max(0.0);
    }

    pub fn volume(&self, channel: Channel) -> f32 {
        self.volume[channel.index()]
    }

    pub fn set_volume(&mut self, channel: Channel, volume: f32) {
        self.volume[channel.index()] = volume.max(0.0);
    }

    pub fn max_se_voices(&self) -> usize {
        self.max_se_voices
    }

    pub fn set_max_se_voices(&mut self, n: usize) {
        self.max_se_voices = n.max(1);
    }

    pub fn is_suspended(&self) -> bool {
        self.suspended
    }

    pub fn channel_paused(&self, channel: Channel) -> bool {
        self.paused[channel.index()]
    }

    /// 当前在飞的语音数（含尾音）。诊断与抢占断言用。
    pub fn voice_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count() + self.se.len() + self.tails.len()
    }

    /// 该通道是否有语音在飞（尾音不算：它已经在淡出了）。
    pub fn is_playing(&self, channel: Channel) -> bool {
        match channel {
            Channel::Se => !self.se.is_empty(),
            ch => self.slots[ch.index()].is_some(),
        }
    }

    /// 该通道主语音的源帧位置（诊断/测试用）。
    pub fn position(&self, channel: Channel) -> Option<u64> {
        match channel {
            Channel::Se => self.se.first().map(|v| v.pos as u64),
            ch => self.slots[ch.index()].as_ref().map(|v| v.pos as u64),
        }
    }

    /// 起一路声音。BGM/BGS/ME 会换掉旧语音：带 `fade_in_ms` 时旧语音转入尾音池淡出，
    /// 否则立即丢弃。SE 超上限时抢占**最旧**的一路（`id` 最小者）。
    pub fn play(
        &mut self,
        channel: Channel,
        data: VoiceData,
        params: &PlayParams,
    ) -> Result<(), AudioError> {
        let id = self.next_id;
        self.next_id += 1;
        let voice = Voice::new(channel, data, params, self.format, id)?;
        if channel.shared() {
            while self.se.len() >= self.max_se_voices {
                let victim = self
                    .se
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, v)| v.id)
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                self.se.remove(victim);
            }
            self.se.push(voice);
        } else {
            let slot = &mut self.slots[channel.index()];
            if slot.is_some() && params.fade_in_ms > 0.0 {
                let mut old = slot.take().expect("刚判过 some");
                old.start_ramp(
                    0.0,
                    ms_to_frames(params.fade_in_ms, self.format.sample_rate),
                    true,
                );
                if self.tails.len() >= MAX_TAILS {
                    self.tails.remove(0);
                }
                self.tails.push(old);
            }
            *slot = Some(voice);
        }
        Ok(())
    }

    /// 立即停止该通道（不淡出），并清掉属于它的尾音与暂停态。
    pub fn stop(&mut self, channel: Channel) {
        if channel.shared() {
            self.se.clear();
        } else {
            self.slots[channel.index()] = None;
            self.tails.retain(|v| v.channel != channel);
        }
        self.paused[channel.index()] = false;
    }

    pub fn stop_all(&mut self) {
        for ch in Channel::ALL {
            self.stop(ch);
        }
    }

    /// 暂停该通道：位置冻结、输出静音，`resume` 后原地继续。
    pub fn pause(&mut self, channel: Channel) {
        self.paused[channel.index()] = true;
    }

    pub fn resume(&mut self, channel: Channel) {
        self.paused[channel.index()] = false;
    }

    /// 全局挂起（Android `Suspended` / 页面隐藏）：整条混音静音且**不推进任何位置**。
    pub fn suspend(&mut self) {
        self.suspended = true;
    }

    /// 解除全局挂起。通道级暂停是独立状态，需要各自 [`Mixer::resume`]。
    pub fn resume_all(&mut self) {
        self.suspended = false;
    }

    /// 淡出并在结束时停止该通道（RGSS 的 `bgm_stop(fade_length)`）。`ms < 1` 等同 stop。
    pub fn fade(&mut self, channel: Channel, ms: f32) {
        let frames = ms_to_frames(ms, self.format.sample_rate);
        if frames < 1.0 {
            self.stop(channel);
            return;
        }
        self.each_of(channel, |v| v.start_ramp(0.0, frames, true));
    }

    /// 把该通道的语音增益淡入到 `target`（RGSS 的 `bgm_play(.., fade_length)` 那一侧）。
    pub fn fade_to(&mut self, channel: Channel, target: f32, ms: f32) {
        let frames = ms_to_frames(ms, self.format.sample_rate);
        if frames < 1.0 {
            let to = target.max(0.0);
            self.each_of(channel, move |v| v.gain = to);
            return;
        }
        self.each_of(channel, move |v| v.start_ramp(target, frames, false));
    }

    /// 混音一块输出。`out` 长度必须为偶数（交织立体声）。
    ///
    /// 流式后端的解码错误在这里冒出来：`out` 里已经是本块能混出的部分。
    pub fn render(&mut self, out: &mut [f32]) -> Result<(), AudioError> {
        assert_eq!(out.len() % 2, 0, "输出必须是交织立体声（长度取偶数）");
        let frames = out.len() / 2;
        out.fill(0.0);
        if frames == 0 || self.suspended {
            return Ok(());
        }
        let (volume, paused, master) = (self.volume, self.paused, self.master_gain);
        let mut error: Option<AudioError> = None;
        for i in 0..OWNED {
            if paused[i] {
                continue;
            }
            let Some(v) = self.slots[i].as_mut() else {
                continue;
            };
            if let Err(e) = v.mix_into(out, volume[i], master) {
                error.get_or_insert(e);
            }
        }
        for slot in &mut self.slots {
            // `dead` 的语音本块仍然出过力，混完这一步才回收。
            if slot.as_ref().is_some_and(|v| v.dead) {
                *slot = None;
            }
        }
        for v in &mut self.tails {
            if paused[v.channel.index()] {
                continue;
            }
            let extra = volume[v.channel.index()];
            if let Err(e) = v.mix_into(out, extra, master) {
                error.get_or_insert(e);
            }
        }
        self.tails.retain(|v| !v.dead);
        if !paused[Channel::Se.index()] {
            for v in &mut self.se {
                if let Err(e) = v.mix_into(out, volume[Channel::Se.index()], master) {
                    error.get_or_insert(e);
                }
            }
            self.se.retain(|v| !v.dead);
        }
        for x in out.iter_mut() {
            *x = x.clamp(-1.0, 1.0);
        }
        self.rendered_frames += frames as u64;
        match error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    fn each_of(&mut self, channel: Channel, mut f: impl FnMut(&mut Voice)) {
        if channel.shared() {
            for v in &mut self.se {
                f(v);
            }
        } else {
            for v in self.slots.iter_mut().flatten() {
                if v.channel == channel {
                    f(v);
                }
            }
            for v in &mut self.tails {
                if v.channel == channel {
                    f(v);
                }
            }
        }
    }

    fn each_mut(&mut self, mut f: impl FnMut(&mut Voice)) {
        for v in self.slots.iter_mut().flatten() {
            f(v);
        }
        for v in &mut self.tails {
            f(v);
        }
        for v in &mut self.se {
            f(v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // T12：`cargo xtask wasm-test` 在 wasm 上跑这一组（见 docs/platforms.md）。
    use std::cell::RefCell;
    use std::rc::Rc;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    /// 确定性样本：**每个样本都编码自己所在的帧号**（左右同值，`f as f32 / 1000`，
    /// 每 500 帧回绕一次以免顶到满幅）。于是"输出里读到哪个值"直接反推"读到了第几帧"，
    /// 循环与变速断言一目了然；声相判据另用 [`dc`]（左右同值的样本分不出左右）。
    fn counted(frames: u32, format: AudioFormat, loop_from: u32, loop_to: u32) -> Arc<AudioData> {
        let ch = format.channels as usize;
        let samples: Vec<f32> = (0..frames as usize)
            .flat_map(|f| {
                let v = (f % 500) as f32 / 1000.0;
                (0..ch).map(move |_| v)
            })
            .collect();
        Arc::new(AudioData::new(samples, format, loop_from, loop_to))
    }

    /// 恒定直流样本（声相判据用：左右各自等于 `v·(1∓pan)`，一眼可读）。
    fn dc(frames: u32, v: f32) -> Arc<AudioData> {
        let format = AudioFormat::stereo(44_100);
        Arc::new(AudioData::one_shot(
            std::iter::repeat_n(v, frames as usize * 2).collect(),
            format,
        ))
    }

    fn stereo(frames: u32) -> Arc<AudioData> {
        counted(frames, AudioFormat::stereo(44_100), 0, frames)
    }

    fn mixer() -> Mixer {
        Mixer::new(AudioFormat::stereo(44_100))
    }

    /// 渲染 `n` 个输出帧，返回交织缓冲。
    fn render_n(m: &mut Mixer, n: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; n * 2];
        m.render(&mut out).expect("渲染不应失败");
        out
    }

    fn frame(out: &[f32], i: usize) -> [f32; 2] {
        [out[i * 2], out[i * 2 + 1]]
    }

    #[test]
    fn render_is_bitwise_deterministic() {
        let build = || {
            let mut m = mixer();
            let bgm = stereo(2048);
            let se = counted(512, AudioFormat::mono(22_050), 0, 512);
            m.play(
                Channel::Bgm,
                bgm.clone().into(),
                &PlayParams::default().gain(0.7).pan(-0.25).fade_in(20.0),
            )
            .unwrap();
            for i in 0..3 {
                m.play(
                    Channel::Se,
                    se.clone().into(),
                    &PlayParams::default().pitch(1.0 + i as f32 * 0.1),
                )
                .unwrap();
            }
            m
        };
        let (mut a, mut b) = (build(), build());
        for _ in 0..5 {
            let ra = render_n(&mut a, 97); // 奇数块长：一并覆盖非整块边界
            let rb = render_n(&mut b, 97);
            assert_eq!(
                ra.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
                rb.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
                "同样的调用序列必须逐位一致（否则四端可跑无从谈起）"
            );
            assert_eq!(a.rendered_frames(), b.rendered_frames());
        }
        assert!(render_n(&mut a, 4).iter().any(|&x| x != 0.0));
    }

    #[test]
    fn empty_mixer_renders_silence() {
        let mut m = mixer();
        assert!(render_n(&mut m, 256).iter().all(|&x| x == 0.0));
        assert_eq!(m.rendered_frames(), 256);
    }

    #[test]
    fn suspend_freezes_positions_and_silences() {
        let mut m = mixer();
        m.play(Channel::Bgm, stereo(4096).into(), &PlayParams::default())
            .unwrap();
        render_n(&mut m, 128);
        let before = m.position(Channel::Bgm).unwrap();
        m.suspend();
        let out = render_n(&mut m, 1000);
        assert!(out.iter().all(|&x| x == 0.0), "挂起期间必须全静音");
        assert_eq!(
            m.position(Channel::Bgm).unwrap(),
            before,
            "挂起不得推进位置"
        );
        m.resume_all();
        render_n(&mut m, 1);
        assert!(m.position(Channel::Bgm).unwrap() > before, "恢复后继续走");
    }

    #[test]
    fn pause_silences_the_channel_and_freezes_its_position() {
        let mut m = mixer();
        m.play(Channel::Bgm, stereo(4096).into(), &PlayParams::default())
            .unwrap();
        render_n(&mut m, 32);
        let at = m.position(Channel::Bgm).unwrap();
        m.pause(Channel::Bgm);
        let out = render_n(&mut m, 100);
        assert!(out.iter().all(|&x| x == 0.0), "暂停中的通道不得出力");
        assert_eq!(m.position(Channel::Bgm).unwrap(), at, "暂停必须冻结位置");
        // 对照：同样帧数下不暂停的混音器走到了更后面，而且有声。
        let mut other = mixer();
        other
            .play(Channel::Bgm, stereo(4096).into(), &PlayParams::default())
            .unwrap();
        let active = render_n(&mut other, 132);
        assert!(other.position(Channel::Bgm).unwrap() > at);
        assert!(active.iter().any(|&x| x != 0.0));
        m.resume(Channel::Bgm);
        assert!(
            render_n(&mut m, 4).iter().any(|&x| x != 0.0),
            "resume 后原地继续发声"
        );
    }

    #[test]
    fn pan_extremes_and_gain_split() {
        // 声相律：`left = v·(1-pan)`、`right = v·(1+pan)` ⇒ 三个位置的期望值一眼可读。
        let render_with = |pan: f32| {
            let mut m = mixer();
            m.play(
                Channel::Se,
                dc(64, 0.3).into(),
                &PlayParams::default().pan(pan).looping(true),
            )
            .unwrap();
            render_n(&mut m, 2)
        };
        let left = render_with(-1.0);
        assert_eq!(
            frame(&left, 1),
            [0.3, 0.0],
            "全左：右路必须为 0，左路不吃亏也不加成"
        );
        let right = render_with(1.0);
        assert_eq!(frame(&right, 1), [0.0, 0.3], "全右：左路必须为 0");
        let mid = render_with(0.0);
        assert_eq!(frame(&mid, 1), [0.3, 0.3], "居中不衰减");
    }

    #[test]
    fn mono_source_is_duplicated_to_both_ears() {
        let mut m = mixer();
        let mono = counted(64, AudioFormat::mono(44_100), 0, 64);
        m.play(
            Channel::Se,
            mono.into(),
            &PlayParams::default().looping(true),
        )
        .unwrap();
        let out = render_n(&mut m, 4);
        for i in 0..4 {
            assert_eq!(
                frame(&out, i)[0],
                frame(&out, i)[1],
                "单声道应等值复制到两耳"
            );
        }
    }

    #[test]
    fn metadata_loop_points_wrap_seamlessly() {
        // 200 帧、循环 [20, 60)：先播完前奏 0..19，然后 20..59 无限往复，永不越出 60。
        let data = counted(200, AudioFormat::stereo(44_100), 20, 60);
        let mut m = mixer();
        m.play(Channel::Bgm, data.into(), &PlayParams::default())
            .unwrap();
        let out = render_n(&mut m, 200);
        let got: Vec<u32> = out
            .chunks(2)
            .map(|f| (f[0] * 1000.0).round() as u32)
            .collect();
        assert!(got.iter().all(|&f| f < 60), "越出循环区间: {got:?}");
        assert_eq!(
            &got[..4],
            &[0, 1, 2, 3],
            "前奏（循环区之前）要照常播，不能被跳过"
        );
        assert_eq!(&got[19..24], &[19, 20, 21, 22, 23], "进循环不跳帧");
        assert_eq!(got[59], 59, "循环终点前一帧还在");
        assert_eq!(got[60], 20, "终点后必须回到起点");
        assert_eq!(got[61], 21, "回绕后不得跳帧（无缝判据）");
        // 第 100 帧 = 回绕后再走 40 帧，正好回到循环起点（20 + (100-60) % 40）。
        assert_eq!(got[100], 20, "第 100 帧仍在 40 帧的循环里");
    }

    #[test]
    fn whole_file_loop_restarts_at_zero() {
        let mut m = mixer();
        m.play(Channel::Bgm, stereo(8).into(), &PlayParams::default())
            .unwrap();
        let out = render_n(&mut m, 20);
        let got: Vec<u32> = out
            .chunks(2)
            .map(|f| (f[0] * 1000.0).round() as u32)
            .collect();
        assert_eq!(&got[..8], &[0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(&got[8..16], &[0, 1, 2, 3, 4, 5, 6, 7], "整段循环");
        assert_eq!(m.voice_count(), 1, "BGM 循环语音不该被回收");
    }

    /// 循环默认值按通道分：背景（BGM/BGS）到尾接上自己，ME 与 SE 到尾即止。
    ///
    /// 写反过一次：ME 跟 BGM 一样是"独占通道"，于是被算进默认循环那一侧，一声过关
    /// 音乐就此挂住不放。
    #[test]
    fn only_background_channels_loop_by_default() {
        for (ch, loops) in [
            (Channel::Bgm, true),
            (Channel::Bgs, true),
            (Channel::Me, false),
            (Channel::Se, false),
        ] {
            assert_eq!(ch.loops_by_default(), loops, "{ch:?} 的默认循环档");
            let mut m = mixer();
            m.play(ch, stereo(8).into(), &PlayParams::default())
                .unwrap();
            render_n(&mut m, 20);
            assert_eq!(m.is_playing(ch), loops, "{ch:?} 播完 2.5 段后");
        }
        // 显式 `Some` 压过通道默认。
        let mut m = mixer();
        m.play(
            Channel::Me,
            stereo(8).into(),
            &PlayParams::default().looping(true),
        )
        .unwrap();
        render_n(&mut m, 20);
        assert!(m.is_playing(Channel::Me), "ME 显式 looping(true) 该循环");
    }

    #[test]
    fn one_shot_se_is_reclaimed_at_end() {
        let mut m = mixer();
        m.play(Channel::Se, stereo(10).into(), &PlayParams::default())
            .unwrap();
        assert!(m.is_playing(Channel::Se));
        render_n(&mut m, 8);
        assert!(m.is_playing(Channel::Se), "还没到尾");
        render_n(&mut m, 8);
        assert!(!m.is_playing(Channel::Se), "到尾后必须自动回收");
        assert_eq!(m.voice_count(), 0);
    }

    #[test]
    fn se_pool_preempts_the_oldest_voice() {
        let mut m = mixer();
        m.set_max_se_voices(3);
        let long = stereo(100_000);
        for i in 0..5 {
            m.play(
                Channel::Se,
                long.clone().into(),
                &PlayParams::default().gain(0.1 * i as f32),
            )
            .unwrap();
        }
        assert_eq!(m.max_se_voices(), 3);
        assert_eq!(m.se.len(), 3, "池子不能超上限");
        // 剩下的必须是最新三路（id 2/3/4）⇒ 其增益分别是 0.2/0.3/0.4
        let gains: Vec<f32> = m.se.iter().map(|v| v.gain).collect();
        assert_eq!(gains, vec![0.2, 0.3, 0.4], "抢占规则：最旧先走");
    }

    #[test]
    fn bgm_replacement_crossfades_the_old_voice() {
        let mut m = mixer();
        m.play(Channel::Bgm, stereo(100_000).into(), &PlayParams::default())
            .unwrap();
        let first_id = m.slots[0].as_ref().unwrap().id;
        render_n(&mut m, 32);
        // 441 帧 = 10ms@44.1k 的淡入淡出
        m.play(
            Channel::Bgm,
            stereo(100_000).into(),
            &PlayParams::default().fade_in(10.0),
        )
        .unwrap();
        let second_id = m.slots[0].as_ref().unwrap().id;
        assert_ne!(first_id, second_id, "新语音要换掉旧的");
        assert_eq!(m.tails.len(), 1, "旧语音进尾音池");
        assert_eq!(m.voice_count(), 2);
        assert_eq!(m.slots[0].as_ref().unwrap().gain, 0.0, "淡入从 0 起坡");
        // 淡完：旧的自灭，新的到目标增益
        render_n(&mut m, 512);
        assert!(m.tails.is_empty(), "淡出到 0 的尾音必须回收");
        assert_eq!(m.voice_count(), 1);
        assert!((m.slots[0].as_ref().unwrap().gain - 1.0).abs() < 1e-6);
    }

    #[test]
    fn fade_stops_the_channel_exactly_at_requested_frames() {
        let mut m = mixer();
        m.play(Channel::Bgm, stereo(100_000).into(), &PlayParams::default())
            .unwrap();
        m.fade(Channel::Bgm, 100.0); // 100ms @44.1k = 4410 帧
        render_n(&mut m, 4409);
        assert!(m.is_playing(Channel::Bgm), "差一帧还该在");
        let out = render_n(&mut m, 2);
        assert!(!m.is_playing(Channel::Bgm), "淡到 0 即停");
        assert!(out.iter().all(|x| x.abs() < 1e-3), "最后两帧应几乎无声");
    }

    #[test]
    fn pitch_and_source_rate_set_the_step() {
        let mut m = mixer();
        // 22050 源进 44100 出 ⇒ 每个输出帧消耗 0.5 源帧
        let slow = counted(64, AudioFormat::stereo(22_050), 0, 64);
        m.play(
            Channel::Bgm,
            slow.into(),
            &PlayParams::default().looping(true),
        )
        .unwrap();
        assert!((m.slots[0].as_ref().unwrap().rate - 0.5).abs() < 1e-9);
        // pitch=2 ⇒ 1.0（等速）且音高升八度
        m.set_format(AudioFormat::stereo(44_100));
        let mut m2 = mixer();
        m2.play(
            Channel::Bgm,
            counted(64, AudioFormat::stereo(22_050), 0, 64).into(),
            &PlayParams::default().pitch(2.0).looping(true),
        )
        .unwrap();
        assert!((m2.slots[0].as_ref().unwrap().rate - 1.0).abs() < 1e-9);
        // 换到 88200 输出：步长减半，位置不动
        let before = m2.slots[0].as_ref().unwrap().pos;
        m2.set_format(AudioFormat::stereo(88_200));
        assert!((m2.slots[0].as_ref().unwrap().rate - 0.5).abs() < 1e-9);
        assert_eq!(m2.slots[0].as_ref().unwrap().pos, before);
    }

    #[test]
    fn volume_and_master_scale_the_mix() {
        // 两台混音器从同一位置起步，逐元素比 `0.5 通道音量 × 0.5 主音量 = 0.25`。
        let build = |volume: f32, master: f32| {
            let mut m = mixer();
            m.set_volume(Channel::Bgm, volume);
            m.set_master_gain(master);
            m.play(
                Channel::Bgm,
                stereo(1000).into(),
                &PlayParams::default().looping(true),
            )
            .unwrap();
            render_n(&mut m, 6)
        };
        let loud = build(1.0, 1.0);
        let quiet = build(0.5, 0.5);
        assert!(loud.iter().any(|&x| x != 0.0), "对照组必须有声");
        for (i, (l, q)) in loud.iter().zip(&quiet).enumerate() {
            assert!(
                (q - l * 0.25).abs() < 1e-7,
                "样本 {i}: 满幅 {l} 的 0.25 倍应是 {}, 实得 {q}",
                l * 0.25
            );
        }
        // 通道之间互不串味：SE 音量归零不影响 BGM。
        let mut m = mixer();
        m.set_volume(Channel::Se, 0.0);
        m.play(Channel::Bgm, stereo(1000).into(), &PlayParams::default())
            .unwrap();
        assert!(render_n(&mut m, 4).iter().any(|&x| x != 0.0));
    }

    #[test]
    fn output_is_hard_clamped() {
        let mut m = mixer();
        let hot = Arc::new(AudioData::one_shot(
            vec![0.9; 64 * 2],
            AudioFormat::stereo(44_100),
        ));
        for _ in 0..4 {
            m.play(Channel::Se, hot.clone().into(), &PlayParams::default())
                .unwrap();
        }
        let out = render_n(&mut m, 8);
        assert!(out.iter().all(|&x| x <= 1.0), "收口必须硬夹取到 ±1");
        assert!(out.contains(&1.0), "夹取应当真的贴到 +1");
    }

    #[test]
    fn invalid_inputs_are_rejected_not_panicked() {
        let mut m = mixer();
        assert!(matches!(
            m.play(
                Channel::Bgm,
                VoiceData::Static(AudioData::one_shot(vec![], AudioFormat::stereo(44_100)).into()),
                &PlayParams::default()
            ),
            Err(AudioError::Invalid(_))
        ));
        let err = m.play(
            Channel::Bgm,
            stereo(64).into(),
            &PlayParams::default().pitch(0.0),
        );
        assert!(
            matches!(err, Err(AudioError::Invalid(_))),
            "pitch=0 要报错: {err:?}"
        );
        assert_eq!(m.voice_count(), 0, "失败的 play 不留半成品");
        assert!(render_n(&mut m, 2).iter().all(|&x| x == 0.0));
    }

    #[test]
    fn streaming_voice_loops_via_seek_and_stays_seamless() {
        let seeks = Rc::new(RefCell::new(Vec::new()));
        let mut m = mixer();
        m.play(
            Channel::Bgm,
            VoiceData::Stream(StreamVoice::new(Box::new(FakeStream::new(
                44100,
                8,
                40,
                seeks.clone(),
            )))),
            &PlayParams::default(),
        )
        .unwrap();
        let out = render_n(&mut m, 60);
        let got: Vec<u32> = out
            .chunks(2)
            .map(|f| (f[0] * 1000.0).round() as u32)
            .collect();
        assert!(
            got.iter().all(|&f| f < 40),
            "流式回绕后不得越出循环终点: {got:?}"
        );
        assert_eq!(got[7], 7, "前奏最后一样");
        assert_eq!(got[39], 39, "终点前一帧");
        assert_eq!(got[40], 8, "回绕到循环起点");
        assert_eq!(got[41], 9, "回绕后不跳帧");
        // 回绕只付 `seek_sample`，且只有终点那一次（起播偏移 0 不该跳）。
        assert_eq!(*seeks.borrow(), vec![8]);
    }

    #[test]
    fn streaming_voice_dies_when_the_source_runs_out() {
        let mut m = mixer();
        // 30 帧的流、不循环：读完即止。
        m.play(
            Channel::Se,
            VoiceData::Stream(StreamVoice::new(Box::new(FakeStream::new(
                44100,
                4,
                30,
                Rc::new(RefCell::new(Vec::new())),
            )))),
            &PlayParams::default().looping(false),
        )
        .unwrap();
        render_n(&mut m, 16);
        assert!(m.is_playing(Channel::Se));
        let out = render_n(&mut m, 16);
        assert!(!m.is_playing(Channel::Se), "流耗尽后必须回收");
        assert!(out.iter().any(|&x| x != 0.0), "耗尽前的样本仍要出力");
    }

    /// 可控流式后端：产出 `samples[f] = (f % 500)/1000`（左右同值），`seek_sample` 只改游标。
    struct FakeStream {
        format: AudioFormat,
        block: usize,
        total: u64,
        cursor: u64,
        seeks: Rc<RefCell<Vec<u64>>>,
    }

    impl FakeStream {
        fn new(rate: u32, block: usize, total: u64, seeks: Rc<RefCell<Vec<u64>>>) -> Self {
            Self {
                format: AudioFormat::stereo(rate),
                block,
                total,
                cursor: 0,
                seeks,
            }
        }
    }

    impl Decoder for FakeStream {
        fn format(&self) -> AudioFormat {
            self.format
        }

        fn read(&mut self, out: &mut [f32]) -> Result<usize, AudioError> {
            let ch = self.format.channels as usize;
            let want = (out.len() / ch).min(self.block) as u64;
            let mut n = 0u64;
            while n < want && self.cursor + n < self.total {
                let f = self.cursor + n;
                let v = (f % 500) as f32 / 1000.0;
                for c in 0..ch {
                    out[n as usize * ch + c] = v;
                }
                n += 1;
            }
            self.cursor += n;
            Ok(n as usize)
        }

        fn seek_sample(&mut self, frame: u64) -> Result<(), AudioError> {
            self.seeks.borrow_mut().push(frame);
            self.cursor = frame;
            Ok(())
        }

        fn total_frames(&self) -> Option<u64> {
            Some(self.total)
        }

        fn loop_points(&self) -> Option<(u64, u64)> {
            Some((8, 40))
        }

        fn block_frames(&self) -> usize {
            self.block
        }
    }
}
