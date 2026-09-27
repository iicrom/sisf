//! SISF：基于 wgpu 的跨平台 2D 引擎门面 crate。
//!
//! 只依赖本 crate，用 feature 开关选择平台与能力（矩阵见 `Cargo.toml`）。下面这段是引擎
//! 的全部主要把手：开窗 + 一块纯色精灵 + 一路 BGM + 一段磁盘上的 RON 事件。
//! 它需要一条平台腿（默认 `native`；`AudioBus` 就门控在设备通路上，见 `docs/audio.md`）。
//!
//! ```no_run
//! use std::{sync::Arc, time::Duration};
//! use sisf::app::{App, AppConfig, FrameCtx, platform};
//! use sisf::asset::{DirProvider, Vfs, VfsPath};
//! use sisf::audio::{AudioBus, mixer::{Channel, PlayParams}};
//! use sisf::event::{AudioPort, EngineBus, EngineEvent, EventCtx, EventDef, EventRunner,
//!     FadeTarget, Prompt, Scene, Sound, SoundChannel, UiPort, VarStore};
//! use sisf::render::{BatchList, Renderer};
//! use sisf::sprite::{Sprite, SpriteArena, TaskScheduler, Vec2, ViewportRef};
//!
//! /// 两个端口的桩。真形状（消息窗位图 / 混音器起播）在 `examples/dialog_demo.rs`；
//! /// 分成两个类型不是洁癖：`EventCtx` 对 `audio` 与 `ui` 各要一个 `&mut`。
//! struct Silent;
//! impl AudioPort for Silent {
//!     fn play(&mut self, _: &Vfs, _: SoundChannel, _: &Sound) -> Result<(), String> { Ok(()) }
//!     fn fade(&mut self, _: FadeTarget, _: u32, _: bool) -> Result<(), String> { Ok(()) }
//!     fn stop(&mut self, _: SoundChannel) {}
//! }
//! struct NoWindow;
//! impl UiPort for NoWindow {
//!     fn show(&mut self, _: &Prompt) {} fn hide(&mut self) {} fn set_dim(&mut self, _: f32) {}
//! }
//!
//! struct Mini {
//!     arena: SpriteArena, tasks: TaskScheduler, scene: Scene, vars: VarStore, bus: EngineBus,
//!     vm: EventRunner, audio: AudioBus, vfs: Vfs,
//! }
//! impl Mini {
//!     fn new() -> Self {
//!         let mut vfs = Vfs::new();
//!         vfs.register(Box::new(DirProvider::new("examples/assets")));
//!         // BGM：解码一次、整段驻留（在调用线程解，见 `docs/audio.md` 第 3 条）。
//!         let ogg = vfs.read(&VfsPath::new("audio/tone_2s.ogg").unwrap()).unwrap();
//!         let mut audio = AudioBus::open();
//!         audio.play_bytes(Channel::Bgm, &ogg, &PlayParams::default()).unwrap();
//!         // 精灵：`bitmap = None` 走内置 1×1 白图，于是 `src` 的宽高就是目的尺寸。
//!         let mut arena = SpriteArena::new();
//!         arena.spawn(Sprite::solid(Vec2::new(96.0, 96.0)));
//!         // RON 事件：磁盘 → 编译成扁平 IR → 启动第 0 页，之后每拍由虚拟机推进。
//!         let ron = std::fs::read_to_string("examples/assets/events/well.ron").unwrap();
//!         let script = Arc::new(EventDef::from_ron(&ron).unwrap().compile().unwrap());
//!         let mut vm = EventRunner::new();
//!         vm.start(1, script, 0);
//!         Self { arena, tasks: TaskScheduler::new(), scene: Scene::grid(32.0),
//!             vars: VarStore::default(), bus: EngineBus::default(), vm, audio, vfs }
//!     }
//! }
//! impl App for Mini {
//!     fn on_event(&mut self, events: &[EngineEvent], _: &mut FrameCtx) {
//!         for e in events { self.bus.push(e.clone()); } // 事件机是总线唯一的读者
//!     }
//!     fn tick(&mut self, _: Duration, _: &mut FrameCtx) {
//!         self.arena.begin_tick(); // 每拍第一行，早于任何移动（`docs/sprite.md`）
//!         let Mini { arena, tasks, scene, vars, bus, vm, vfs, .. } = self;
//!         let (mut ports, mut window) = (Silent, NoWindow);
//!         let mut ctx = EventCtx { sprites: arena, tasks, scene, audio: &mut ports,
//!             ui: &mut window, vars, bus, vfs };
//!         vm.tick(&mut ctx, 64); // 64 = 一拍的指令预算（`docs/events.md`）
//!     }
//!     fn draw(&mut self, alpha: f32, r: &mut Renderer) {
//!         let mut list = BatchList::new();
//!         self.arena.collect_at(ViewportRef::MAIN, alpha, &mut list).unwrap();
//!         r.append_list(list); // 分批、上传、present 都在 `Runner` 的 `end_frame` 里
//!     }
//! }
//! fn main() { platform::run(Mini::new(), AppConfig::default()).unwrap(); }
//! ```
//!
//! 四条边界值得先看文档再改代码：
//!
//! - **帧序是固定的**：`tick ×N → prepare → draw(alpha) → end_frame`。相机这类"跟着插值
//!   走"的投影必须在 `prepare` 里推，`draw` 里推只对下一帧生效（`docs/platforms.md`）。
//! - **一个混合变体 = 一条管线**：`Blend` 只有 `Normal/Add/Replace`，`Tone::Matrix` 采集时
//!   显式报错 —— 两条都是 T12 的定案，理由见 `docs/sprite.md`。
//! - **合批键 `(layer, clip, epoch, blend, z, insertion)`**：`z` 只在同批次内有决定权
//!   （`docs/sprite.md`），HUD 要盖住地图得靠 viewport 层而不是大 `z`。
//! - **平台不支持的能力是 feature 关掉的**，不是空实现：视频是 T11 的桩
//!   （[`video::VideoPlayer`] 的每个入口都显式失败），SF2 与中文字形由宿主自备
//!   （`docs/porting.md`）。

pub use sisf_app as app;
pub use sisf_asset as asset;
pub use sisf_audio as audio;
pub use sisf_event as event;
#[cfg(any(
    feature = "image",
    feature = "ogg",
    feature = "mp3",
    feature = "wav",
    feature = "midi",
    feature = "video",
    feature = "gif",
    feature = "simd-jpeg"
))]
pub use sisf_media as media;
pub use sisf_render as render;
pub use sisf_sprite as sprite;
pub use sisf_text as text;

#[cfg(feature = "rpgmk")]
pub use sisf_rpgmk as rpgmk;

#[cfg(feature = "video")]
pub mod video;
