# T7：音频混音器 + OGG/MP3/WAV + MIDI(SF2)

四层的边界恰好就是 feature 的边界，所以这份文档按层讲：

```
字节 ──sisf-media/audio.rs 解码──▶ AudioData(驻留 f32) ─┐
字节 ──sisf-media/midi.rs 合成──▶ &mut dyn Decoder(流) ─┤
                                                        ▼
                                    sisf-audio/mixer.rs  ▶ render(&mut [f32]) 交织立体声
                                                        ▼
                          pump_native.rs (cpal) / pump_web.rs (WebAudio)  ▶ 扬声器
                                                        ▲
                                          AudioBus = mixer + pump 的门面
```

| 层 | 文件 | 碰设备？ | 何时编进来 |
| --- | --- | --- | --- |
| 解码 | `crates/sisf-media/src/audio.rs` | ❌ | `sisf-media/{ogg,mp3,wav}` |
| MIDI | `crates/sisf-media/src/midi.rs` | ❌ | `sisf-media/midi` |
| 混音 | `crates/sisf-audio/src/mixer.rs` | ❌ | **恒编**（`sisf-audio` 的编译底座） |
| 设备 | `pump_native.rs` / `pump_web.rs` | ✅ | `native`（非 wasm32）/ `web`（wasm32），互斥 |
| 门面 | `bus.rs`（`AudioBus`） | ✅ | 只在两条 pump 之一存在时 |

两条 feature 都没开时 `sisf-audio` **只剩 `mixer`**：没有设备通路就没有 `AudioBus`，这是刻意的
（DoD-4：平台不支持的能力要门控 + 写文档，不留空壳类型骗编译）。

## 混音器：只认"帧数"这一个时钟

`Mixer::render(&mut [f32])` 吃任意长度的交织立体声缓冲，把它填满本块混音结果。**没有墙钟**：
fade 到点、SE 回收、循环回绕全部按渲染掉的帧数推进，所以"pump 停了"与"挂起了"是同一件事，
而淡出在 CI 上和在真机上一样可断言。

- **通道**：`Bgm/Bgs/Me` 各占一个语音槽（新的换掉旧的），`Se` 是多路复用池，默认上限
  `DEFAULT_SE_VOICES = 16`，超了就抢占**最旧**那路（`Voice::id` 单调递增）。
- **循环默认值按通道分**（`Channel::loops_by_default`）：只有 BGM/BGS 到尾接上自己，
  **ME 与 SE 播完即止**。这一条是本阶段修出来的：ME 跟 BGM 一样是"独占通道"，早期写法
  `looping = !channel.shared()` 把它算进了循环那一侧，于是一声过关音乐挂住不放
  （`examples/audio_demo.rs` 第一版日志里 ME 的位置每秒在 470/587 之间来回跳，就是这个）。
  显式 `PlayParams::looping(Some(_))` 永远压过通道默认。
- **换歌 = 交叉淡化**：往独占通道 `play` 时旧语音不立刻死，按新 `fade_in_ms` 淡出后进尾音池
  （`MAX_TAILS = 8` 封顶，防止疯狂 `play` 把池子堆成无界队列）。
- **采样率不匹配 = 线性插值**：`rate = src_rate / out_rate` 每输出帧消耗 `rate` 个源帧，
  `pos` 是 f64 分数位置。没有引重采样器（清单外），代价是变了调的文件在高频处会有一点混叠。
  `pitch` 走同一个 `rate`，所以音高倍率同时改变速度 —— 与 RGSS 的 `pitch` 语义一致。
- **失真控制**：输出硬夹 ±1.0（`output_is_hard_clamped`），`AudioError` 而不是 panic 处理
  非法输入（0 长样本、非法通道数、非正 pitch）。
- **挂起 vs 暂停**：`suspend()` 全局冻结（位置不动、渲染静音）+ pump 停止取数；
  `pause(channel)` 只压那一路，且**通道级暂停是独立状态** —— `resume_all()` 只解挂起，
  被 `pause` 的通道仍要各自 `resume`。这是 RGSS 的语义，测试钉住了。
- **确定性**：`render_is_bitwise_deterministic` 用同一份输入跑两遍比逐位相等；四端都无设备
  也能跑（`mixer` 不碰设备）。

## 解码：symphonia 是唯一后端

`decode_audio(&[u8]) -> Result<AudioData, AudioError>` 先 `sniff` 魔数（OggS / ID3 或帧同步 /
`RIFF....WAVE`）再分派，**不认扩展名**：VFS 里 `bgm_001.ogg` 其实是个 WAV 这种事在真实素材包里
天天发生。容器与 feature 一一对应（`Container::enabled`），没开对应 feature 时报
`UnsupportedFormat` 而不是链进解码器。

- **`Decoder` trait**（`format/channels/sample_rate/read/seek_sample/total_frames/loop_points/block_frames`）
  是驻留与流式共用的抽象：symphonia 的每一路都被 `drain()` 抽成 `AudioData`，
  rustysynth 则作为**流**直接实现它。
- **`lewton` 只作回退**：`ogg-lewton` feature（默认关）走 `decode_ogg_lewton`，
  自己解析 Vorbis comments 取循环点（不引 lofty）。它的存在是为了"symphonia 某个版本解坏
  某张图"时有退路，5 项单测与 symphonia 通路断言同样的长度与循环点。

### 循环点：三个容器三种读法

`AudioData{loop_start_samples, loop_end_samples}` 单位是**源帧**（终点不含），由 lofty 0.25.4
按容器走**具体文件类型**（`VorbisFile`/`MpegFile`）而不是通用 `TaggedFile` —— 通用通路会把映射
不到的键丢掉，而 `LOOPSTART` 恰好就是映射不到的那种（`ItemKey` 是封闭枚举，没有 unknown 变体）。

| 容器 | 键放在哪 | 读不到的后果 |
| --- | --- | --- |
| OGG | Vorbis comments `LOOPSTART`/`LOOPEND`（ffmpeg `-metadata` 写的就是它） | 整段 `[0, frames)` 循环 |
| MP3 | ID3v2 TXXX，`LOOPSTART` 与 AIMP 风格 `MP3LOOPSTART` 都试 | 同上 |
| WAV | lofty **没有** `smpl`/`cue` 通路 → 恒 `None` | 同上 |

所以 **WAV 的 BGM 只能整段循环**：要精确循环点请用 OGG（想留 WAV 就得自己遍历 chunk 读 `smpl`，
登记在下方"能力矩阵"）。循环区间由 `AudioData::new` 夹进 `0..=frames` 且保证 `start < end`，
非法元数据降级成"不循环"而不是 panic —— 这些数据来自外部文件。

无缝性由混音器保证：越过 `loop_end` 才回绕到 `loop_start`，**起始位置可以落在循环区之前**
（RPG Maker 的 BGM 有前奏，intro 要照常播完才进循环）。断言在
`metadata_loop_points_wrap_seamlessly`（帧级）+ `ogg_loop_tags_survive_into_a_voice`
（fixture 真值 + `AudioBus` 通路）。听测：`cargo run -p sisf --example audio_demo`，OGG 回绕处无
咔哒（Windows 侧已复跑，见下方"验收与闸门"）。

## native pump：混音在主线程，回调只搬数

清单字面是"回调内直接 `Mixer::render`"。**做不到**：cpal 要求 data callback 是
`FnMut + Send + 'static`，而 `Mixer` 里有 `Box<dyn Decoder>`（`MidiDecoder` 内含 rustysynth
合成器，不是 `Send`），主线程又必须随时能改混音状态。搬进去只有两条路：套锁（清单明令禁止），
或者 `unsafe` 造跨线程 `&mut`（同一状态两线程读写，正是爆音与 UB 的来源）。

于是换了个等价的形状：**混音仍在主线程 `pump()` 里跑，产出的交织 f32 压进一条 SPSC 环；
回调只从环里取数 + 转换成设备样本格式**。锁依然是 0 个（环只有原子下标与 `AtomicU32` 样本位型，
Relaxed 存样本 + Release/Acquire 发布游标），设备线程做的功退化成一次拷贝加一次格式转换 ——
这也正是它该做的事。

关键常量（`pump_native.rs`）：`TARGET_FRAMES = 1024`（≈21ms@48k，稳态深度）、`RING_FRAMES = 4096`
（≈85ms，一次 `pump()` 的补给上限 = 撑得住的最低帧率：48000/4096 ≈ 11fps）、`BLOCK_FRAMES = 256`、
`MAX_BLOCKS = RING_FRAMES / BLOCK_FRAMES = 16`（一次卡帧后能一次追平，又不无限占用主线程）。

**阈值不是 `TARGET_FRAMES` 本身**，而是 `refill_samples(上次 pump 以来回调取走的样本数, 环容量)`
= `max(TARGET×2, 一帧的量 + 1/4)`。只按目标深度封顶是个帧率陷阱：每帧至多补到 1024 帧，而每帧要
播掉 `rate / fps` 帧 ⇒ 主线程一慢于 ≈47fps 就是**结构性补给不足**，稳态一直缺一点（Chrome 软件
渲染实测 ~42fps ⇒ 11% 的帧是静音，见 `docs/audio-web.md`）。抬高阈值后 60fps 下仍取 1024 帧
= 21ms，也就是**不拿延迟换保险**，只在掉帧时临时加深、并受环容量约束。native 侧这条由
`pump_native::tests::refill_target_covers_a_whole_slow_frame` 钉住。

**设备变更两条兜底**，因为 cpal 不给"默认设备变了"的通知、`StreamError` 也**不保证**会来
（拔掉蓝牙耳机后 WASAPI 常常只是静默把流停在原地）：

1. `StreamError` → 置 `rebuild`，主线程下一次 `pump()` 重建流；
2. 每 `DEVICE_POLL = 500ms` 主动查默认输出设备的 `DeviceId`，变了就重建（重建后环清空，
   不播陈旧缓冲）。

两条都受 `OPEN_RETRY = 2s` 节流。**开不出设备不是错误**：`Pump::new()` 永不失败，没有输出设备
时走"静音混音"（混音时钟照常推进、fade 照常到点、SE 照常回收），`stats().device == None` +
`describe()` 把那行状态打出来。这台机器上就真实发生过：进程启动时宿主**没有默认输出设备**，
日志先是 `cpal: 无输出设备（静音混音，时钟照常）`，2s 后重试拿到
`Headphones (High Definition Audio Device) @48000Hz/2ch F32`，期间 `underrun` 累计 64 样本
（≈1.3ms，只发生在设备到货前那一窗），之后 12s 播放 `underrun+0`。

## wasm pump：worklet 报需求，主线程按需求渲

见 [docs/audio-web.md](audio-web.md)（通路选择、autoplay 解锁、延迟实测口径与数字）。
与清单字面不同的一点在这里也值得复述：**AudioWorklet 线程不能同步回调主线程 WASM**
（要做到得靠 SharedArrayBuffer + Atomics，而那需要 COOP/COEP 跨源隔离与 `+atomics` 编译目标，
本仓库两样都没开）。所以 worklet 通路 = 需求上报 + 主线程渲染 + `postMessage` 拷贝；
`ScriptProcessor` 回退通路才是字面意义的同步 pull。

## MIDI：一路流语音，不是"另一种播放"

`midly 0.5.3` 解析（`midi_info` 报 SMF 格式/轨数/`ticks_per_beat`/按 tempo 加权的时长/音符数）
+ `rustysynth 1.3.6` 合成，产物 `MidiDecoder` 实现同一个 `Decoder`，于是 MIDI 是
`VoiceData::Stream` 的一路普通 Voice，参与同一套音量/淡出/挂起规则。整首歌驻留是几十 MB 量级，
流式是唯一合理选择。`midi_info` 同时是**输入校验**：rustysynth 的 `MidiFile::get_length` 对空轨会
panic，所以"没有事件""认不出格式"的文件在 midly 这一层就返回 `AudioError::Invalid`。

- **引擎不附带 SoundFont**（体积 + 授权）。`SoundBank::from_sf2(&[u8])` 的字节怎么来由应用决定，
  推荐经 VFS：`vfs.read(&vpath("soundfont.sf2"))`。
- **"给了 SF2 就能播"这条只有用户素材能自证**，所以两个真合成测试做成 env 开关（默认 ignore，
  不进闸门）：`SISF_SF2=/path/to/bank.sf2 cargo test -p sisf-media --features midi -- --ignored`
  —— 断的是"整段合成确定（两遍逐位相等）"与"内部循环模式永不结束、只在 0 处回绕"。
- **没有 bank 时是错误，不是 panic，也不是静音**：`MissingSoundBank(提示)`，提示里直接写两条
  出路 —— 载入 `.sf2`，或离线预渲染成 `.ogg`：
  `fluidsynth -F out.wav bank.sf2 song.mid && ffmpeg -i out.wav -c:a libvorbis -q:a 6 -metadata LOOPSTART=… -metadata LOOPEND=… song.ogg`。
  引擎自身不碰 SF2 授权，所以这条路只写在文档里。
- **MIDI 不能中段 seek**：顺序合成，`seek_sample(0)` 是唯一可靠的跳转（回绕用）。
  所以 `play_midi` 只接受 `offset_frames = 0`，要"从中间开始"请先离线转 OGG。
- **循环两种实现路径**（`MidiLoop`，谁负责回绕不一样）：`Once` = sequencer 不循环、
  `total_frames` 报确切长度，回绕交给混音器（BGM 走 `seek_sample(0)`，SE/ME 到尾自灭）；
  其余（`Whole`/`RpgMaker` CC#111/`IncredibleMachine`/`FinalFantasy`/`Tick(n)`）= sequencer
  内部循环、`total_frames` 报 `None`，混音器因此永不回绕、语音永不死亡，也就不为回绕付一次 seek。
- `MidiOptions.sample_rate` 会被夹进 rustysynth 支持的 `16k..=192k`，与设备率的差额由混音器线性消化。

## 用法与两个真坑

```rust
// App::tick 里，一次 tick 一次 pump（渲染帧率与混音无关；tick 停 = 声音停，正是挂起语义）
self.bus.pump();
// 起播：BGM 循环 + 淡入，SE 定位
self.bus.play(Channel::Bgm, self.bgm.clone(), &PlayParams::default().gain(0.8).fade_in(400.0))?;
self.bus.play(Channel::Se, self.confirm.clone(), &PlayParams::default().pan(-0.7))?;
// 换 BGM：直接再 play（旧的那路会自动交叉淡化出去）
// 切场景：self.bus.fade(Channel::Bgm, 1000.0);
```

1. **`play_bytes` 在调用线程整段解码**。2s 立体声 OGG 在 debug 构建里要 ~90ms —— 而输出环只有
   `TARGET_FRAMES = 1024`（≈21ms）。实测：第一版 demo 每圈 `play_bytes`，回绕那一下
   `underrun+4480`，耳朵里就是一声爆。正确形状是**读一次、解一次、之后复用 `Arc<AudioData>`**
   （异步版 = `AssetStore` 后台解码，T4 已有通路），demo 现在照它写。
2. **句柄要活着**：`AudioBus` 内部持 `Mixer`，语音持 `Arc<AudioData>`；`Mixer` 本身不 clone
   样本，所以驻留内存由 `Arc` 计数决定。想要"资源随场景卸载"就在 `stop_all()` 后把 `Arc` 丢掉。

`examples/audio_demo.rs` 用**确定性时间线**驱动 cue（12s 一圈：BGM → 左 SE → ME → 右 SE →
`.mid` → BGM 淡出），因为 `EngineEvent` 现在只有 `Lifecycle`，键盘事件要到 T9 才有 ——
`Space`/`M` 的绑定点在示例头注释里写明，T9 落地后把时间线换成事件回调即可。它同时把可观测量
画在屏幕上（四通道进度条、播放头、欠载灯、挂起条）并每秒打一行日志。

## 能力矩阵（不支持项）

| 能力 | 本阶段 | 说明 |
| --- | --- | --- |
| OGG(Vorbis)/MP3/WAV 解码、任意采样率/通道（1/2ch）播放 | ✅ | symphonia，四端通用 |
| OGG/MP3 循环点（`LOOPSTART`/`LOOPEND`） | ✅ | lofty；见上表 |
| WAV `smpl`/`cue` 循环点 | ❌ | lofty 无该通路 ⇒ 整段循环；要精确点请用 OGG |
| MIDI(SF2) 播放 | ✅ feature `midi` | **需用户自带 SoundFont**；无 SF2 ⇒ `MissingSoundBank` |
| 3D 定位音、低通/回声等 RGSS `AudioContext` 高级效果 | ❌ | 只有等功率 pan + 增益；留给 T10 兼容层按需再加 |
| 变速不变调、离线重采样、时域拉伸 | ❌ | 混音器只有线性插值 |
| 每路独立声道数（如 5.1 SE） | ❌ 报 `Invalid` | 只支持 1/2ch，多声道请在素材期下混 |
| 异步预取解码 | 未接 | `AudioBus` 只吃 `&[u8]`；接 `AssetStore` 是应用侧几行代码 |
| 双 pump 同时存在 | 不可能 | `native` 只在非 wasm32、`web` 只在 wasm32，故 `bus.rs` 无需 `cfg_if` |

## 平台矩阵

| 平台 | 通路 | 状态 |
| --- | --- | --- |
| Windows / Linux（native） | cpal（WASAPI / ALSA） | ✅ 本次实测 Win：见"验收与闸门" |
| Android（native） | cpal（OpenSLES/AAudio） | ⚠️ 编译面绿（`--no-default-features --features native`）；真机手测归 T12。挂起语义已接：`App::on_event` 的 `Lifecycle::Suspended` → `AudioBus::suspend()` |
| wasm32（Chrome） | AudioWorklet + ScriptProcessor 回退 | ⚠️ 编译面绿；浏览器听测与延迟数字见 [docs/audio-web.md](audio-web.md) |
| iOS Safari | 同上 | ⚠️ 未验证；autoplay 策略更严，解锁已在 JS 侧做（首次手势 `resume`） |

## 验收与闸门

```
cargo test -p sisf-audio --all-features        # 33 项：mixer 19 + bus 8 + pump_native 6
cargo test -p sisf-media  --all-features       # 音频侧 21 项（audio 12，含 lewton 回退 5；midi 9，其中 2 项 `--ignored`）
cargo test --workspace --all-features          # 176 绿 0 红（图像/渲染/文本各阶段不变）
cargo xtask check-all                          # 四组 feature 组合（含新增的 wasm 音频全开）
cargo xtask clippy && cargo xtask fmt --check
cargo run -p sisf --example audio_demo --features ogg,mp3,wav,midi            # 桌面手测（听）
cargo run -p sisf --example audio_demo --features ogg,mp3,wav,midi -- 14      # 有界复跑：打判决行
cargo xtask web audio_demo                                                   # wasm：?ex=audio_demo
```

有界复跑（14s）在本机看到的判决行 —— 判据是"混音推进贴着墙钟"与"欠载贴着 0"：

```
收工: tick 25587 帧 / 墙钟 14.00s / 混音推进 13.91s（99%）· 欠载读数 64 ·
      cpal: Headphones (High Definition Audio Device) @48000Hz/2ch F32
            ring=1120f underrun=64s overflow=0s callbacks=1388 streams=1 err=-
```

逐秒行里 `underrun+0` 全程成立（含 12s OGG 回绕与 ME/SE 起播），`overflow=0s`、`streams=1`
（设备到货后没有重建过流）、`err=-`。64 样本全部来自"启动时无默认输出设备 → 2s 后重试拿到设备"
那一窗，即上面记录的那条降级路径。

**仍需人工的两条**（清单验收里写着头听，机器替代不了）：
① 长时间播放 + **插拔耳机/切换默认设备**不断流；② Chrome-wasm 播 BGM/SE 并记延迟。

## 依赖与资源登记（DoD-3）

- 全部在钉版清单内，**本阶段没有新增 crate**：`symphonia 0.6.1`（`default-features = false`，
  按 `ogg/vorbis/mp3/wav/pcm` 开）、`lewton 0.10.2`（仅 `ogg-lewton`）、`lofty 0.25.4`、
  `midly 0.5.3`、`rustysynth 1.3.6`、`cpal 0.18.2`。
- **`lofty` 必须保留默认 feature `id3v2_compression_support`**：0.25.4 关掉它会在
  `id3/v2/frame/read.rs` 留下未导入的 `Id3v2Error*`（上游 cfg 断裂），直接编译失败。
- 新增 crate 间 feature 边：`sisf-audio/{native,web,ogg,mp3,wav,midi}`（解码 feature 转发到
  `sisf-media/*`）；门面 `sisf/{ogg,mp3,wav,midi}` 必须**同时**转发 `sisf-media/*` 与
  `sisf-audio/*` —— 第一版只转了 media，于是 `bus.rs` 的 `play_midi` 编不出来（`E0599`）。
- `sisf-audio` 的 `web` 只需 `dep:wasm-bindgen`：`js-sys` 试过又删了（胶水返回普通 `Array`，
  Rust 侧签名 `Vec<f64>` 就够），留着就是清单外的死依赖。
- 音频 fixture（真实小文件，仓库里只存一份，示例与单测共用）：
  `crates/sisf-media/tests/fixtures/{tone_2s.ogg,tone_1s.wav,tone_1s.mp3,jingle.mid}`。
  wasm 手测用的副本在 `web/assets/`（已进 `web/assets/manifest.txt`）；**SoundFont 故意不进仓**。
