---
name: sisf-app-anatomy
description: 在 SISF 引擎上写新示例/游戏应用：facade 与 feature 矩阵、App trait 四方法与帧序、AppConfig、桌面/wasm/android 双端启动骨架。当要新建 App、改启动流程、或问"引擎怎么用"时使用。
---

# 用 SISF 写一个应用

游戏侧统一走门面 crate `sisf`，模块别名 `sisf::{app, asset, audio, event, media, render, sprite, text}`
（`crates/sisf/src/lib.rs:86-106`）。示例挂在 sisf 包：`cargo run -p sisf --example <名>`。

## feature 怎么开

`default = ["native"]`（`crates/sisf/Cargo.toml:20`）。按需：`web`（wasm 平台腿）、
`image`（解码链，native/web 各自透传）、`gif`、`ogg`/`mp3`/`wav`/`midi`（音频解码，
**`midi` 必须同时透传 sisf-audio 才看得到 `AudioBus::play_midi`**）、`video`（T11 桩）、
`rpgmk`（**空壳，T11 未实装，不可依赖**）、`simd-jpeg`（native only）。
`inspector` 已定案移除。

## App trait：4 个方法，不是 3 个

```rust
trait App {
    fn on_event(&mut self, events: &[EngineEvent], ctx: &mut FrameCtx) {}  // app.rs:14
    fn tick(&mut self, dt: Duration, ctx: &mut FrameCtx);                  // app.rs:19 必实现
    fn prepare(&mut self, alpha: f32, ctx: &mut FrameCtx) {}               // app.rs:28
    fn draw(&mut self, alpha: f32, renderer: &mut Renderer);               // app.rs:36 必实现
}
```

帧序固定（`runner.rs:245-255`）：`tick ×N（0..=MAX_CATCHUP=5）→ prepare(alpha) →
frame_u → renderer.begin_frame → draw(alpha) → end_frame`。

- **`tick`** 固定步长逻辑；`draw(alpha)` 的 alpha 是 tick 间插值系数。
- **`prepare`** 是投影烘焙前相：跟随插值的相机必须在这里 `push_viewport`
  （放 tick 末差 `(1-alpha)` 抖动、放 draw 慢一帧）——见 tiled_map.rs:14-25 的相机写法。
- `FrameCtx` 是纯数据：`viewport/scale/input/suspended/focused` 字段 +
  `push_viewport(Viewport)->u8 层号 / pop_viewport / map_pointer / raster_scale`（frame.rs）。
  清屏色不在 ctx，走 `AppConfig::clear`。

## AppConfig（config.rs:17-48）

`title="SISF"`、`virtual_size=(544,416)`（RPG Maker 惯例）、`scale_mode=Fit`（还有
`Fill`/`IntegerFit`）、`letterbox`、`tick_hz=60.0`、`clear`、`actions=ActionMap::rpg_maker()`。

## 双端启动骨架（所有示例共用）

wasm-bindgen 不允许 `main` 同时被 bin 与 `#[wasm_bindgen(start)]` 导出，惯例形状
（`examples/hello_window.rs:59-82`）：

```rust
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn sisf_start() { real_main(); }
pub fn main() { real_main(); }
fn real_main() {
    platform::run(MyApp::new(), AppConfig::default()).unwrap();  // platform/mod.rs:186
}
```

- Android：`platform::android::run_with_android_app(app, config, android_app)`，由游戏
  自己的 cdylib `android_main` 调（模板 docs/android.md）。
- 构建/复验 wasm 产物用 `sisf-wasm-verify` skill；闸门用 `sisf-gates` skill。

## 平台铁律（写代码前先过一遍）

1. **wasm 下 `println!` 静默丢弃**、`Instant::now()` 直接 panic。日志一律
   `platform::log_info(&str)`（桌面 stdout / wasm console）；计时走 `platform::now_secs()`。
2. 逻辑时钟用整数拍（`TickClock`），动画/补间按拍记进度，帧率无关。
3. 挂起语义：`Lifecycle::Suspended/Resumed` 要转发给子系统（音频 `bus.suspend()/resume_all()`，
   见 audio_demo.rs:465-484）；wasm 页面隐藏 = Suspended。
4. 宿主→引擎单向通道：`sisf::app::post_custom(channel: u32, serde_json::Value)`。
5. `RenderPass` 只能在 `begin_frame`/`end_frame` 之间由 Renderer 自持；`App::draw` 只借
   `&mut Renderer`，不得跨帧持有 wgpu 借用。

## 惯用导入

```rust
use sisf::app::{platform, App, AppConfig, FrameCtx, ScaleMode, Viewport};
use sisf::event::bus::EngineEvent;   // 变体：Action/Key/Pointer/Lifecycle/Custom
```

参考实现由浅入深：`hello_window`（骨架）→ `tiled_map`（精灵+相机+输入）→
`dialog_demo`（事件机）→ `audio_demo`（四通道）→ `sprite_bench`（VFS+图集）。
