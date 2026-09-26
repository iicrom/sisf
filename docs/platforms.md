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

**投影在 `draw` 之前烘焙 ⇒ 相机要有自己的相位**（T8 复盘）：`runner` 的帧顺序是
`tick ×N → App::prepare(alpha) → FrameCtx::frame_u → begin_frame → draw(alpha)`。
`frame_u` 把 viewport 栈快照进 `FrameU` 后交给 GPU，所以在 `draw` 里 `push_viewport`
只对**下一帧**生效。于是"跟着插值后的 sprite 走"的相机两头都不对：推到 `tick` 末 = 相机钉在
拍尾、角色却画在 `Sprite::interpolated(alpha)` 上，角色相对地图每拍被拽回
`(1 - alpha) × 每拍位移` 像素再弹回来 —— `examples/tiled_map.rs` 那条"人物一直在抖动"就是这么
来的（`SPEED = 2` 时最多差 2 个虚拟像素；只在走位期间出现，但 alpha 每帧都变 ⇒ 一步之内来回
跳）；推到 `draw` 里 = 永远慢一帧。
`App::prepare(alpha, ctx)` 就是为这一刻加的：`alpha` 与本帧 `draw` 同值，而栈还没烘。
钉成单测 `the_following_camera_shares_the_hero_s_interpolation_phase`（含对照组：拍尾相机在
alpha 0/0.5 下的错位恰好是 2/1 虚拟像素）。

**同相位只是必要条件的一半：那个点还得落在像素格点上**（T8 复盘第二步）。把相机改成跟随
`interpolated(alpha)` 之后，"人物在抖"变成了"整个地图在抖"（左右移动时左右晃、上下移动时上下
晃）。原因是图集采样器是 `FilterMode::Linear`（`crates/sisf-render/src/renderer.rs`），相机平移量
一带小数，地图那些本来对齐在物理像素上的整数格点就整体落到纹素中间被重采样 —— 每一根硬边缘
都被摊开成两个像素，且摊开的比例随 `alpha` 每帧变化，于是满屏呼吸。`nearest` 也躲不掉：它把
每个采样点四舍五入回纹素，边缘不糊了，但整张图会随小数部分在两个像素位置之间跳。

像素美术的跟随相机因此不能拿真亚像素的 `alpha` 用：`examples/tiled_map.rs` 加了 `lattice_alpha`
（`(alpha × 每拍位移 × scale).round() / (每拍位移 × scale)`），把插值折到 1px 台阶上 —— `SPEED = 2`
在 1× 下一拍走两步。角色与地图同时落在格点上，两者相对位置仍然精确，代价只是位移按整像素分级
（正是像素美术该有的走法）。前提是每拍位移与格点都是整数；要用真亚像素滚动，得先把采样器换成
`Nearest` 并接受 1px 跳动，或给相机做时间抖动（T10 已就 `filter` 选择权给出结论：不加双
sampler，重开的条件写在 `docs/events.md`）。
**格点是设备像素格，不是虚拟像素格**：被重采样的是纹理，量的是 GPU 平移量，所以台阶取在
`scale` 之后 —— 2× 时一拍走 4 个虚拟像素 = 8 个设备像素，但插值有 5 个设备格点可站
（0/2/4/6/8），比只在虚拟格上取整的 3 个（0/4/8）更细。相机本体也过一遍 `snap_to_device`
（`t/scale` 在 `scale` 为整数时与虚拟格等价，非整数标度才露出缺口）。
钉成单测 `the_following_camera_snaps_the_world_to_the_pixel_lattice`（1×：虚拟格 = 设备格）与
`the_camera_lattice_is_the_device_pixel_grid_not_the_virtual_one`（2×：逐拍读的是
`cam × scale` 那个量，5 个设备台阶、对照组只有 3 个虚拟台阶）。浏览器侧的判据不是"看着
晃不晃"，而是**相邻两帧能否用一个整数位移对齐**（逐 rAF 把 canvas 画进离屏 2D 再 `getImageData`，
在 `dx ∈ -8..8` 上找最小差异比例）：取整后 59 对相邻帧平均 0.13%、最大 0.18% 差异，最佳位移恒为
`dx = 2`（偶尔 1/3，从不为负）；把 `lattice_alpha` 临时换成恒等函数复测同一探针，平均 17.16%、
最大 19.89% 对不齐，且每帧的 distinct 颜色数从 41 涨到 64~145 —— 那些多出来的颜色就是重采样
混出来的中间色。纵向（ArrowUp）同一探针：平均 0.21%、最大 0.34%，`dy` 恒为 2~3。
⚠️ 探针本身有个坑：WebGL canvas 未开 `preserveDrawingBuffer`，在应用自己的 rAF 之外读到的可能
是已清空的缓冲（表现为"两帧之间 18.9% 的像素变了、却任何位移都对不齐"）。按 distinct 颜色数
把空帧滤掉再算，否则测的是读回伪影而不是画面。

**前两步都对，仍然"人物不抖背景抖"（T8 复盘第三步）**：抖动不在映射上，在**记账**上 ——
`MoveRoute` 每走完一格多烧一拍空转（"下一趟才报告完成"的收尾拍）。`TickClock` 照样为这一拍消耗
一个周期并把 `alpha` 归零，可那一拍 `prev_pos`/`pos` 原地不动 ⇒
`interpolated(alpha)` 把**最后一步重播一遍**（画面退回一步再走回来），每格一次。因为相机把角色
钉在屏幕正中，错位的只有背景 —— 这正是"人物不抖背景抖"的形状。修法是节点跑完当场收尾
（`crates/sisf-sprite/src/task.rs`），代价是平均速度从 `tile/(ceil(tile/speed)+1)` 回到声明值。
不变量：**每个被消耗的周期都必须推进 `prev_pos`/`pos`**，否则插值在重播而不是在插值。
判据（离线复放浏览器实测的乱节拍 `[18.2, 12.1, 18.2, 24.3, …]` 喂进 `TickClock`，逐帧读
`cam × scale`）：相机从不回退、离理想滚动位置 `|cam - 120·t|` ≤ 2 物理像素、总位移对得上 ——
修账前偏差 12px、1.486s 只滚出 166px（理想 178px，慢 9%），修账后 2px/176px。
钉成单测 `the_camera_tracks_the_ideal_scroll_under_a_jittery_frame_rate`。
残余的逐帧位移量仍有 1~3px 的散布，那是软件 WebGL 下 54~56fps 的节拍噪声（12/18/24ms），
不是位置错误：位置单调、误差 ≤2px。

**修完收尾拍仍然"红色人物移动后人物抖，玩家角色移动后地图抖"（T8 复盘第四步）**：同一条不变量
只在一个消费者身上成立是不够的 —— `prev_pos` 只被 `set_pos` 推进，所以**任何"这一拍没动"的拍**
都还在重播最后一跳：玩家走完一格松手的停拍、NPC `RouteNode::Wait` 的每一拍、撞墙被作废的剩余格数。
停拍里 `interpolated(alpha)` 沿刚才那一格来回走 ±一格，被相机放大成"整张地图朝移动方向来回弹"，
形态与第三次报回的一模一样 —— 差别只在**静止时可见**（前三步的判据全在"走位期间"取样，
静止段一直没被看过，所以本地穷举 4 方向 × 2 倍率 × 6 节拍都判"通过"）。
判据要按"每次启动/每个阶段"分段，而且要包含停手相：
`standing_still_must_not_replay_the_last_hop`（165Hz 恒定节拍，松手后逐帧读数必须全等）、
`the_camera_tracks_the_ideal_scroll_under_a_jittery_frame_rate` 的松手相（乱节拍，先倒回可滚动
区间再松手 —— 顶到地图边缘时怎么弹都读数不变，是白测）、
`a_wait_node_draws_the_same_point_every_frame`（路线层：`Wait` 期间 `interpolated(0/0.5/1)` 恒定）、
`begin_tick_erases_a_hop_that_the_new_tick_does_not_repeat` + `arena_begin_tick_folds_every_live_sprite`
（数据层）。修法不是让每个游戏逻辑各自记得推进，而是 `App::tick` 第一行统一
`arena.begin_tick()`（见 `docs/sprite.md`）。

**输入也走同一条 `Runner`**（T9）：winit 的 `WindowEvent` 与 gilrs 的手柄事件各自只被翻译一次
（`crates/sisf-app/src/normalize.rs`），三端拿到的是同一套 `EngineEvent`，游戏侧读的是
`ActionMap` 里的命名动作。差别只在设备层 —— 手柄只有桌面三端有（wasm 要 JS 侧轮询
`navigator.getGamepads()`、Android 上 gilrs 0.11 没有后端），所以 `native` 里那条 `gilrs`
feature 与两层 `cfg` 必须一起判。词汇表、边沿语义（Press/Hold/Release、one-shot、连发）、
死区与平台矩阵见 `docs/input.md`。

> 呈现验证提示：Windows 上若显卡走硬件覆盖合成（MPO / flip-model），`BitBlt`/`PrintWindow`
> 抓到的客户区可能是白色占位，而屏幕上实际有内容。因此"清屏色是否正确"由
> `crates/sisf-render/tests/readback.rs` 用 GPU 读回断言（无需窗口），
> 呈现是否成功由 `FrameStatus::Presented` 判定。T5 的 golden 测试沿用同一读回路径。

## Web

```sh
cargo xtask web           # 构建 wasm + wasm-bindgen 到 web/pkg，并在 127.0.0.1:8080 起静态服务
cargo xtask web tiled_map # 换示例：不带示例名时缺省只构建 hello_window
```

- **构建走 `--release`**：同一个 `tiled_map` 示例，debug 的 `web/pkg/tiled_map_bg.wasm` 是 32.5MB、
  release 8.17MB，而浏览器侧多半跑在软件 WebGL（`?wgl` ⇒ ANGLE Microsoft Basic Render Driver）上，
  没优化的产物本身就是节拍噪声的一大来源（`todo.md` T8 复盘）。增量重编 20s，可接受。
  诊断通道与档位无关：panic 钩子在 `platform/wasm.rs` 里无条件安装。
- 打开 `http://127.0.0.1:8080/index.html`：WebGPU 优先。
- 打开 `http://127.0.0.1:8080/index.html?wgl`：强制 WebGL2 回退（`requested_backends()` 读 URL）。
- **响应带 `Cache-Control: no-store`**：这个服务不发 `ETag`/`Last-Modified`，浏览器只能启发式缓存，
  于是改完重编后**普通刷新（F5）还在跑旧的几十 MB wasm**，看着完全像"修了没生效"。开发服务宁可每次读盘。
- **网页上示例的倍率恒为 1.00**：winit 的 `set_canvas_size` 只写 canvas 的 **CSS style**，不动
  `width/height` 属性（`winit-0.30.13/src/platform_impl/web/web_sys/mod.rs:71-85`），而
  `web/index.html` 把属性钉成 544×416 ⇒ 物理尺寸永远是 544×416、`IntegerFit` 只能给出 1×。
  要看 2× 的设备格点只能开桌面窗口（例如 1185×862 ⇒ 倍率 2.00）。
- `visibilitychange` 映射为挂起：页面隐藏时上报 `Lifecycle::Suspended` 并停帧，
  浏览器暂停 rAF 也不会吃时间（累加器在 T8 同样冻结）。
- 需要 `hello_window_bg.wasm` 的 MIME 为 `application/wasm`（xtask 内置服务已处理）。
- **panic 会送到 console**（`platform/wasm.rs::install_panic_hook`，`run` 第一件事）：wasm 上
  Rust panic 到 JS 只剩一个 `RuntimeError: unreachable`，原文与位置全丢，而 rAF 链一断页面就
  定格成黑屏 —— 没有任何可诊断输出。T8e 的"打不开、也没日志"就是靠这条钩子才认出真因是
  cosmic-text 的 `expect("no default font found")`（见 `docs/text.md` wasm 约束第 3 条）。

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
