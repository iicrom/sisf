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

`SISF_BACKEND=gl` 可强制 wgpu 的 OpenGL 后端，用于排查 Vulkan/DX12 呈现问题。

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
