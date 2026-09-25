# 平台支持矩阵（T2）

三端共用同一套 `Runner<A>`（`crates/sisf-app/src/runner.rs`）：winit 0.30 的
`ApplicationHandler` 只实现一次，平台层仅负责"如何把事件循环跑起来"和"从哪里拿窗口句柄"。

| 平台 | 入口 | surface 来源 | 检查命令 |
|---|---|---|---|
| Windows / Linux / macOS | `platform::run` → `platform/native.rs`（`EventLoop::run_app`） | `create_surface_unsafe(RawHandle)`，句柄取自 `HasWindowHandle`/`HasDisplayHandle` | `cargo check -p sisf --all-targets --all-features` |
| Web (wasm32) | `platform::run` → `platform/wasm.rs`（`spawn_app`，winit 在 wasm 无 `run()`） | `SurfaceTarget::Canvas`，复用页面 `<canvas id="sisf">` | `cargo check -p sisf --target wasm32-unknown-unknown --no-default-features --features web` |
| Android | `android_main` → `platform::android::run_with_android_app` | 同 native（`NativeActivity` 的 ANativeWindow） | `cargo check -p sisf --target aarch64-linux-android --no-default-features --features native` |
| iOS | 未启用（target 未安装），代码路径与 native 相同 | `UIViewLayer` 句柄 | 见下文"启用 iOS" |

## 一条命令检查三端

```sh
cargo xtask check-all     # 宿主 --all-features + wasm32 web + aarch64-linux-android native
cargo xtask clippy        # -D warnings
cargo test                # 纯逻辑 + GPU 读回（见下）
```

## 桌面

```sh
cargo run -p sisf --example hello_window
```

窗口按 `AppConfig::virtual_size`（默认 544×416）+ `ScaleMode::Fit` 做 letterbox；
最小化会送 `Resized(0,0)`，`Runner` 视为遮挡 → 上报 `Lifecycle::Suspended` 并停帧，
恢复后重新 `request_redraw()` 续帧（不重建 surface）。

**letterbox 边条取整**（T8）：`FrameCtx::recompute` 对 `(物理 - 画面)/2` 向下取整。整数倍
缩放（`IntegerFit`）遇奇数边长窗口时未取整的原点会落在半像素上，而 1:1 的内容平移半像素
= 每个像素采样在两个纹素正中（整帧发糊），拖动窗口时还在两个相位之间跳。左右边条因此最多
差一个像素，换来的是虚拟像素格与物理像素格严格同相。指针/触摸一律 `FrameCtx::map_pointer`
（物理 → 虚拟，与投影同一个 `Viewport` 出处），所以取整之后命中仍然对得上画出来的位置。
文字则按 `FrameCtx::raster_scale()` 取光栅超采样档，非整数倍下发虚那条缺陷由它收口
（见 `docs/text.md`）。验收：`integer_fit_with_odd_padding_stays_on_the_pixel_grid`、
`pointer_hits_the_expected_cell_after_the_letterbox`、`text_raster_tier_follows_the_upscale`。

`SISF_BACKEND=gl` 可强制 wgpu 的 OpenGL 后端，用于排查 Vulkan/DX12 呈现问题。

**letterbox 只管投影，不管裁剪**（T8e）：`Viewport` 把画面放进边条内的矩形，但 scissor 仍然
是整个 surface。于是"相机层"（`push_viewport(at(-cam.x,-cam.y,..))`）自己平移出去的那部分内容
会照原样画进黑边里 —— 谁推了 viewport 层，谁就得自己 `push_clip(该层, 该层坐标下的画面盒)`。
另一半后果是画序：`BatchKey` 的排序先看 `layer` 再看 `z`，所以挂在 layer 0 的 HUD
**没法**用一个大 `z` 盖住相机层上的地图，只能再往栈上推一层（层号更大）并把文字也投到那一层
（`TextEngine::draw_text_at`）。两层各对应一个真实缺陷，`examples/tiled_map.rs` 里
`the_hud_layer_nets_out_the_camera_and_outranks_the_world` 把"相机层自己裁、HUD 层净投影回画面"
钉成单测，桌面手测的像素证据见 `todo.md` 的 T8e。

> 呈现验证提示：Windows 上若显卡走硬件覆盖合成（MPO / flip-model），`BitBlt`/`PrintWindow`
> 抓到的客户区可能是白色占位，而屏幕上实际有内容。因此"清屏色是否正确"由
> `crates/sisf-render/tests/readback.rs` 用 GPU 读回断言（无需窗口），
> 呈现是否成功由 `FrameStatus::Presented` 判定。T5 的 golden 测试沿用同一读回路径。

## Web

```sh
cargo xtask web           # 构建 wasm + wasm-bindgen 到 web/pkg，并在 127.0.0.1:8080 起静态服务
```

- 打开 `http://127.0.0.1:8080/index.html`：WebGPU 优先。
- 打开 `http://127.0.0.1:8080/index.html?wgl`：强制 WebGL2 回退（`requested_backends()` 读 URL）。
- `visibilitychange` 映射为挂起：页面隐藏时上报 `Lifecycle::Suspended` 并停帧，
  浏览器暂停 rAF 也不会吃时间（累加器在 T8 同样冻结）。
- 需要 `hello_window_bg.wasm` 的 MIME 为 `application/wasm`（xtask 内置服务已处理）。

## Android

见 `docs/android.md`（cdylib 模板、`NativeActivity` 清单、`cargo ndk` 流程、真机手测清单）。

## 启用 iOS

当前机器未安装 iOS target，属于外部工具链工作（需要 macOS + Xcode）：

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
cargo check -p sisf --target aarch64-apple-ios-sim --no-default-features --features native
```

代码侧无需新增模块：`platform/native.rs` 的 winit + `create_surface_unsafe` 路径同样适用于
iOS（`UIView` 句柄），差异只在 Xcode 工程与 `Metal` backend 选择；音频（T7）需要
`AVAudioSession` 胶水，视频（T11）走 `ffmpeg-next`。这些在真正启用 iOS 时再补，
不在本阶段留空实现。
