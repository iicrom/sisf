# SISF 实施 TODO

设计依据见 `C:\Users\fufu\.qoder-cn\plans\flying-arch-thrush.md`（引擎设计：workspace 结构、渲染器、VFS、事件模型）。本清单是它的执行拆解：T1–T12 对应 12 个阶段，每条子任务后附最小可证明判据。

约定：`[ ]` 待办 · `[x]` 已完成 · 每阶段末尾的"验收（整段）"通过才提交一次。
依赖链：T1→T2→T3→T4→{T5,T7}；T5→T6；{T5,T7}→T8；T2→T9；{T6,T8,T9}→T10→T11→T12。

## T0 建立基线
- [x] 本文件入仓；`git init`（若未初始化）+ 首次提交含现有桩代码。验收：`git log --oneline` ≥1 条。（62c39bd）

## T1 workspace + xtask 脚手架
- [x] 根 `Cargo.toml` 改 `[workspace] members=["crates/*","xtask"]` + `[workspace.package] version / edition="2024" / license`；删除根 `src/`。验收：`cargo metadata` 成功。
- [x] 建 10 个 crate 与 `Cargo.toml`：`sisf-render` `sisf-sprite` `sisf-text` `sisf-event` `sisf-asset` `sisf-media` `sisf-audio` `sisf-app` `sisf-rpgmk` `sisf`；依赖边：`asset→media`、`render→asset`、`sprite→render`、`text→render`、`event→sprite`、`audio→{asset,media}`、`app→全部`、`rpgmk→app`、`sisf→app`。验收：`cargo check` 通过（每 crate 仅空 `lib.rs` + 一行 `//!` 说明）。
- [x] `[workspace.dependencies]` 统一钉版：wgpu 30.0.1、winit 0.30.13、bytemuck 1.25.2、rect-packer 0.2.1、etagere 0.3.0、serde 1.0.229、ron 0.12.2、thiserror。验收：子 crate 全部 `{ workspace = true }`。
- [x] `crates/sisf/Cargo.toml` feature 矩阵：`default=["native"]`、`native`、`web`、`ogg`、`mp3`、`wav`、`midi`、`video`、`gif`、`inspector`、`rpgmk`、`simd-jpeg`，每个 feature 标注它启用哪个 crate/dep。验收：`cargo tree -e features -p sisf` 与预期一致。
- [x] `xtask` crate：`check-all`（win `--all-features` / wasm32 `--no-default-features --features web` / android `--no-default-features --features native`）、`fmt`、`clippy`、`web`、`golden`、`wasm-test`、`android`；任一子进程失败即非零退出。验收：`cargo xtask check-all` 三端绿。
- [x] `.cargo/config.toml`：`alias.xtask`。验收：`cargo xtask check-all` 可直接调用。
- 验收（整段）：`cargo xtask clippy` + `check-all` 绿 → 提交 `P1a workspace+xtask`。

## T2 跨平台开窗 + surface 生命周期
- [x] `sisf-app/src/config.rs`：`AppConfig{ title, virtual_size:(u32,u32), scale_mode: Fit|Fill|IntegerFit, letterbox: Rgba, tick_hz: f64 = 60.0, clear: Rgba }`。验收：默认 544×416 可构造。→ `frame::tests::default_config_is_rpgmaker_size` 绿。
- [x] `sisf-app/src/app.rs`：`trait App { fn on_event(&mut,self,&[EngineEvent],&mut FrameCtx); fn tick(&mut,self,dt:Duration,&mut FrameCtx); fn draw(&mut,self,alpha:f32,&mut Renderer); }`。验收：签名不跨帧持有 wgpu 借用（`renderer` 存在 `RunState` 内，`draw` 只借 `&mut Renderer`）。
- [x] `platform/native.rs`：winit 0.30 `EventLoop` + `ActiveEventLoop`；wgpu `Instance`（`Backends::all()`）+ `create_surface_unsafe(RawHandle)` + `Surface::configure`；wgpu 30 用 `CurrentSurfaceTexture::{Lost,Outdated}` → 按缓存配置重配。验收：`hello_window` 出窗口、每帧 `FrameStatus::Presented`（hwnd 与可见窗口一致），清屏色由 `crates/sisf-render/tests/readback.rs` 的 GPU 读回断言（`BitBlt` 抓不到 flip-model 内容，见 `docs/platforms.md`）。
- [x] `platform/wasm.rs` + `web/{index.html,main.js}`：wasm 下 winit 无 `run()`，用 `spawn_app` + 页面既有 `<canvas id="sisf">`，重绘由 `Window::request_redraw()` 链交给浏览器 rAF。验收：`cargo xtask web` 起服务后 `?wgl`（WebGL2）控制台打出"main 已启动"且无 adapter 报错；WebGPU 路径在无权 GPU 的自动化浏览器里报 "No available adapters"（属环境限制，需人工开一次页面复验）。
- [x] `platform/android.rs`：`android-activity` 0.6（`native-activity`）+ `EventLoop::builder().with_android_app()`；`Resumed` 建 surface、`Suspended` 释放 `Surface` 与 GPU 资源（`sync_suspend` 在 android 上置 `renderer = None`）。验收：android 端 check 绿 + `docs/android.md`（cdylib 模板/manifest/真机清单）。
- [x] 生命周期：`Lifecycle{Resumed,Suspended,Focused(bool),CloseRequested}` 上报 `App::on_event`；挂起时 `last = None` 不吃时间（T8 冻结累加器同理）；`Resized`/`ScaleFactorChanged` → 重算投影 + 请求重配。验收：实测最小化 → `Suspended`，恢复 → `Resumed` 并继续出帧，无 panic、surface 未丢。
- [x] iOS：target 未安装 → `docs/platforms.md` 记录启用步骤（`rustup target add aarch64-apple-ios*` + Xcode），代码复用 `platform/native.rs`。
- 验收（整段）：`cargo xtask check-all` 三端绿 + `cargo xtask clippy` 无 warning + `cargo test`（frame 3 项 + readback 1 项）绿 + Windows 最小化/恢复手测 → 提交 `P1b app loop+lifecycle`。

### T2 期间发现并修正的事实（供后续阶段引用）
1. **winit 0.30 的 `RedrawRequested` 不会因 `ControlFlow::Poll` 自动送达**，必须 `Window::request_redraw()` 链式驱动；因此三端统一用 `ControlFlow::Wait`，`Runner::do_frame` 末尾续帧、`sync_suspend` 在恢复时踢帧。
2. **winit 0.30 wasm 无 `PollStrategy::WebAnimationFrame` / `WindowControls`**（0.29 遗留），只有 `IdleCallback|Scheduler`。
3. **`wgpu::Color` 按线性空间解释**，而 `Rgba` 约定为 sRGB 数值域：`to_color` 必须 sRGB→线性，否则 0.35 会渲染成 160/255（readback 测试实测并修正）。
4. wgpu 30 的 `RenderPassColorAttachment` 新增必填 `depth_slice`，且 `get_current_texture()` 返回 `CurrentSurfaceTexture` 枚举（不再有 `SurfaceError`）。
5. `wasm-bindgen` 不允许 `main` 同时被 bin 与 `#[wasm_bindgen(start)]` 导出：示例改为 `sisf_start()` + `real_main()`。

## T3 批量 quad 渲染器
- [x] `sisf-render/src/vertex.rs`：`#[repr(C)] Instance{ pos, size, uv0, uv1, color, rot, tone, layer, _pad }` + 单位 quad 顶点/索引缓冲。验收：`const _: () = assert!(size_of::<Instance>()==80)` + 字段偏移单测。→ `vertex::tests::{instance_field_offsets_match_stride, quad_is_unit_square_ccw}` 绿（64B 装不下，见下方事实 1）。
- [x] `shaders/sprite.wgsl`：group0 `FrameU{ proj: 8×mat4x4, time }`（viewport 栈）；group1 `tex` + `sampler`；VS 做 anchor/rotation/scale，flip 用 `size` 符号实现（等价于交换 uv0/uv1）；FS 做预乘 alpha + tone offset/gray。验收：三端编译无 naga 报错 → 桌面 Vulkan 与 wasm32/WebGL2（ANGLE）同一份 shader 出图，`renderer::gpu_tests` 读回像素断言。
- [x] `renderer.rs`：`Renderer::{new, begin_frame, end_frame}`；两条 pipeline（alpha-over、additive）+ `debug_lines` feature 下 clone 一条 `polygon_mode: Line`。验收：同一 `RenderPass` 内切换 pipeline 正常 → `renderer::gpu_tests::one_pass_switches_pipelines_bind_groups_and_slots`（离屏 64×64，2 条管线 + 2 个 bind group + 跨槽顶点缓冲，读回 6 个探针像素 + 清屏色 2 处）。
- [x] 实例缓冲：环形子分配（`COPY_DST|VERTEX`，3 片在飞），默认 16384 quad/槽，写满先 flush 再续，容量可配（`Renderer::set_quads_per_slot`）。验收：wrap-around 与强制 flush 单测 → `ring::tests::{overflow_wraps_into_next_buffer_and_counts_flush, never_reuses_the_buffer_still_in_flight, capacity_grows_geometrically_to_fit_one_frame, begin_frame_reports_growth, empty_frame_uses_no_slot}`。
- [x] batch 分组：排序键 `(layer:u8, texture_epoch:u64, blend:u8, z:i32, insertion:u32)`，相邻同键合并为一次 `set_bind_group`+`draw`。验收：`batch.rs` 单测断言 batch 数与提交顺序 → 7 项（含 `different_key_breaks_batch_and_keeps_order`、`split_at_capacity_breaks_run`）。
- [x] `FrameCtx::push_viewport/pop_viewport`（深度 8，越界 `debug_assert`）。验收：8 层嵌套单测 → `frame::tests::eight_viewport_levels_nest_and_pop` + `layer0_maps_virtual_center_to_ndc_origin` + `unused_levels_repeat_layer0`。
- [x] `examples/sprite_bench.rs`：10 万 sprite 随机变换 + 帧时间打印。验收：桌面 ≥60fps（实测值记入 `docs/bench.md`），wasm `?wgl` 可跑 → RTX 2080 Ti/Vulkan：100k 实例 avg 13.27ms = **75.4fps**、300 帧仅 1 次 >20ms、整帧 3 batch；`cargo xtask web sprite_bench` + `?ex=sprite_bench&wgl` 出图并持续打印帧时间。
- 验收（整段）：`cargo test --workspace --all-features`（render 19 + render 集成 1 + app 6 + doctest 1 = 27 绿）+ `cargo xtask check-all` 三端 0 warning + `cargo xtask clippy` + `cargo fmt --all --check` + `docs/bench.md` 桌面/wasm 两侧手测 → 提交 `P2 batched quad renderer`。

### T3 期间发现并修正的事实（供后续阶段引用）
1. **`Instance` 是 80B 不是 64B**：设计文档的字段表漏了"着色器要知道 viewport 层"这一项。加了 `layer: f32` 后必需字段共 18 个 f32 = 72B，补到 80B（16 的倍数）。换来的是**一条管线服务 8 层**（`frame.proj[clamp(layer,0,7)]`），不必每层一条 pipeline 或动态 uniform offset。
2. **`FrameU::proj` 必须是 `[[[f32;4];4];8]`（512B）**，不是 `[[f32;4];8]`（128B）：WGSL 的 `array<mat4x4<f32>,8>` 每个矩阵按列拆成 4 个 `vec4` 且带 16B stride。整块 528B（`time`@512 + 3 个 pad），单测 `frame_uniform_layout_matches_wgsl` 钉住。
3. **flip 用 `size` 的符号**，不用交换 uv：负宽=左右翻、负高=上下翻，两轴可组合，且 anchor 在 CPU 侧折进 `pos`，实例里不再放 flip 位。
4. **`RenderPass` 只能在 `end_frame` 内打开**：它借用 encoder 与 surface view，若存进 `Renderer` 就变成自引用。于是帧序固定为 `begin_frame(→清空 pending) → App::draw(push) → end_frame(排序/上传/单 pass/present)`。
5. **`POLYGON_MODE_LINE` 在 WebGL2 后端常常没有**：`debug_lines` 启用但 adapter 缺能力时降级为实心 + 一条 stderr 提示，不当错误（见 `renderer.rs` 的 `allow_lines`）。
6. **`Queue::write_texture` 接受未 256 对齐的 `bytes_per_row`**（wgpu 自己暂存），1×1 白图直接写 4 字节即可，不必手写 staging；T4 的大图上传仍要走 `copy_texture_to_buffer` 对齐。
7. **wasm32-unknown-unknown 的 `std::time::Instant::now()` 会 panic**（std 在该 target 无时钟，`sys/time/unsupported.rs`）。T2 的 `Runner::do_frame` 每帧调它——浏览器里第一帧就会崩（此前只跑到"main 已启动"，没暴露）。现在三端统一 `platform::now_secs()`（wasm = `performance.now()`，需要 web-sys `Performance`）。
8. **合批键不含 `z`/`insertion`**：它们只决定批内实例顺序，所以 10 万随机 z 仍然只有 2–3 个 batch；跨环形槽时必须断批（顶点缓冲不同），这就是 bench 里 `batch=3 / slot=2` 的来源。

## T4 VFS + 图集 + 图像解码 + GPU 上传
- [x] `sisf-asset/src/path.rs`：`VfsPath` 规范化（统一 `/`、查找键大小写不敏感、拒绝 `..` 越界）。验收：路径穿越单测。→ `path::tests::{backslash_and_dots_are_normalized, lookup_key_is_case_insensitive, escaping_root_is_rejected, internal_parent_segment_cancels, absolute_and_empty_inputs_rejected}`（5 项，越根/绝对路径/盘符/NUL 都在构造期失败，provider 不再二次校验）。
- [x] `vfs.rs`：`trait VfsProvider: Send { fn read(&self,&VfsPath)->Option<Vec<u8>>; fn exists(&self,&VfsPath)->bool; }`；`DirProvider`（native `std::fs`；wasm 走预置清单 + fetch）；`Vfs{ providers }` 后注册优先。验收：补丁目录覆盖同名文件；zip/RGSSAD 仅需接口在文档登记。→ `vfs::tests::{later_provider_overrides_earlier, fallthrough_when_patch_lacks_file, patch_dir_overrides_same_named_file, traversal_never_reaches_provider}`（4 项，含真临时目录的双 provider 覆盖）；zip/RGSSAD 只需再实现 `VfsProvider`，登记在 `vfs` 模块文档。
- [x] `store.rs`：`AssetId(u32 hash, u32 gen)`、`Request<T>{Ready(Handle<T>),Loading(Ticket),Failed(Arc<str>)}`、`AssetStore<T>`（`HashMap` + `Arc`/`Weak` refcount + generation）。验收：句柄 Drop 后可重载；同路径不重复解码。→ `store::tests::{same_path_decodes_only_once, handle_drop_allows_reload_with_next_generation, missing_path_fails_and_retry_reresolves, load_blocking_installs_sync_and_nonblocking_load_is_async}`（4 项）。
- [x] `tasks.rs`：`trait TaskRunner`；`NativeTasks`（`std::thread` + `mpsc`，GPU 上传恒在主线程）与 `WasmTasks`（`wasm-bindgen-futures` + `fetch`，`poll_ready()` 主循环收割）；`load_blocking` 两端语义（join / 协作式 pump）。验收：非阻塞 `load` 不使单帧耗时超过一帧。→ `tasks::tests::{submit_reports_inflight_and_clears_on_poll, load_blocking_returns_value_inline, submit_does_not_block_the_caller}`（`nap()` 线程睡 300ms 而提交方立即返回）。
- [x] `sisf-render/src/atlas.rs`：分桶 `Rgba8UnormSrgb` 图集（起始 2048²）+ `rect-packer` 装箱 + `epoch` 参与 batch key + 按最后使用帧 LRU 驱逐 + 满页换新页（`max_pages=8` 后强制 flush）+ `Bitmap::large()` 旁路。验收：装箱/驱逐/换页单测。→ `atlas::tests` 7 项（`packs_until_full_then_switches_page`、`buckets_route_by_max_edge`、`eviction_recycles_dead_pages_with_new_epoch`、`max_pages_hard_limit_flush_not_error`、`large_bypass_pages_reserve_epoch_bit`、`zero_sized_and_dead_epoch_guards`、`touch_moves_lru_recency`）；GPU 侧生命周期在 `atlas_gpu::tests`（Drop 投递→退役、Clone 共享、uv 归一化）。
- [x] 上传：每帧单一 staging ring，`copy_texture_from_buffer` 前按 `bytes_per_row` 256 对齐；sRGB 交给格式不做手动 gamma。验收：17×13 这类非对齐尺寸上传后与 CPU 侧像素一致。→ `atlas_gpu::gpu_tests::upload_17x13_unaligned_is_pixel_exact`（真 adapter/Vulkan，逐像素比对整页，另断言空白区仍是 `[0,0,0,0]`）+ `upload_two_bitmaps_to_one_page_keeps_both`（同页两单互不覆盖）；`atlas_gpu::tests::queue_aligns_rows_to_wgpu_rule` 钉住偏移与对齐算法。
- [x] `sisf-media/src/image.rs`：`decode_image(&[u8])->Result<ImageData>`，按魔数分派 `png 0.18` / `jpeg-decoder 0.3.2`（`simd-jpeg` 下换 native-only `turbojpeg`）/ `image-webp 0.2.4` / 手写 BMP（24/32bpp、bottom-up、位掩码）/ `gif 0.14`（feature，多帧→`AnimDef`）；输出统一 RGBA8 预乘。验收：各格式 fixture 解码正确（`tests/fixtures/`），wasm 下 `simd-jpeg` 关闭。→ `image::tests` 8 项（png/webp 逐像素、jpeg 尺寸+均值、bmp24/bmp32、gif→`AnimDef`、未知魔数、BMP 不支持变体报错）；`simd-jpeg` 只在 `[target.'cfg(not(wasm32))'.dependencies]` 下可见。
- [x] 改造 `examples/sprite_bench.rs` 走 VFS + 图集 + 混图源。验收：混合 PNG/JPG/WebP/BMP 时打印的 batch 数符合断言。→ 断言 `batches == 用到的页数 × blend 种数`、`instances == sprite 数`、`slot == 1`；桌面 4 格式混涂铺在 **1 页** 256² 图集上，10 万实例 `batch=2`，avg 14.03ms = **71.3fps**（与 T3 白图基线 13.27ms 同档，数据与读数陷阱见 `docs/bench.md`）；wasm 侧 `cargo xtask web sprite_bench` + `?ex=sprite_bench&wgl&sprites=20000` 人工复验通过：`第 4 帧: 图源 4/4 就绪（页=1）` → `合批断言通过: … batch=2`，canvas 上是带纹理的 2 万 sprite（软件光栅 ANGLE 下 43–49ms/帧，T3 同环境 71ms）。
- 验收（整段）：VFS/atlas/image 单测 + 桌面与 wasm 加载手测 → 提交 `P3 vfs+atlas+image`。→ `cargo test --workspace --all-features` 58 绿 0 warning（asset 16 + media 8 + render 32 + render 集成 1 + doctest 1）+ `cargo xtask check-all` 三端绿 + `cargo xtask clippy`（win/wasm `-D warnings`，wasm 腿已补 `--all-targets`）+ `cargo fmt --all --check` 净 + 桌面/wasm 两端加载手测均通过（wasm 见 `docs/bench.md` 的 420 帧输出）。

### T4 期间发现并修正的事实（供后续阶段引用）
1. **wasm 后端的 `wgpu::Texture`/`Buffer` 不是 `Send`**：图集原先把页纹理放在 `AtlasShared` 的 `Mutex<HashMap>` 里，于是 `AssetStore<Bitmap>::load`（要求 `T: Send`）在 wasm32 上直接不成立。现在拆成 `Atlas`（纯装箱，`Send`，解码线程可用）+ `AtlasPages`（页纹理，只在持 device 的主线程侧），`Renderer::apply_atlas(&atlas, &mut pages, &mut uploads)` 是唯一的同步点。
2. **页桶下限是 256，不是请求尺寸**：`rect-packer` 的页边长取"≥ 最大边长的 2 的幂"再夹在 `[256, max_edge]`。后果是任意一批边长 ≤256 的图共用一页——这正是 T4 验收里"4 个图源 1 页"的来由，也意味着图集起步内存按 256²（256KB sRGB）计。
3. **`map_async` 不能用于每帧 staging**：wasm 下既不能阻塞等映射完成，`get_mapped_range_mut()` 又要等到 poll 结束。所以 staging ring 是 **CPU 侧一块对齐缓冲 + 一次 `write_buffer` + 逐单 `copy_buffer_to_texture` + 一次 submit**，缓冲容量几何增长后跨帧复用。`write_buffer` 的 offset 需 4 字节对齐——每单尺寸都是 256 的倍数，天然满足。
4. **`LayoutError` 得实现 `Display`**：解码闭包要把装箱失败（`TooBig`/`NeedFlush`）作为 `String` 经由 `Request::Failed` 冒出来，`.to_string()` 是唯一的跨 crate 报错通道，故挂 `thiserror`。
5. **`Response::array_buffer()` 是 `catch` 绑定**：返回 `Result<Promise, JsValue>`（`Blob::array_buffer()` 才直接返回 `Promise`），`JsFuture::from` 之前必须解一次 Result。`Window::fetch_with_str` 反过来是直接返回 `Promise`。
6. **`AssetStore` 的解码闭包不能持有 GPU 资源**：闭包跑在 worker 线程（native）上，只能捕获 `Send` 的东西（这里是 `Atlas` 克隆）。空字节也一律交给解码闭包报错——wasm 下"清单里有但还没 fetch 到"的文件因此**不能提前投递**，否则失败会被缓存成长期错误、需要 `retry`。
7. **门面的 `image` feature 必须拉 `dep:sisf-media`**：`sisf::media::image::decode_image` 此前只在默认 feature 下可见，`--features web` 里 `sisf-media` 整个不在依赖图上。依赖边方向不变（`render`/`asset` 仍不认识 `media`），解码→装箱的胶水属于调用方。
8. **合批断言要先把环形槽配宽**：`set_quads_per_slot(sprite 数)` 之后整帧只用 1 片缓冲，`batches` 才是纯"页 × blend"的结果；默认 16384 quad/槽时 10 万实例会跨槽，跨槽处必须断批（T3 的 `batch=3 slot=2`），会把合批质量混上缓冲管理的噪声。
9. **`println!` 在 wasm32-unknown-unknown 上是静默丢弃**（std 的 unsupported stdout 写进 sink，不 panic）：T3 的 wasm 打印全靠示例里手写 `console::log_1`，T4 重写示例时漏掉，表现为"页面只打了 adapter 一行、其余无输出"。现在示例统一走 `sisf::app::platform::log_info`（桌面 `println!`、wasm `console.log`），该函数由 `pub(crate)` 提升为 `pub`。
10. **`xtask clippy` 的 wasm 腿原先不带 `--all-targets`**，示例/测试不进闸门：`needless_question_mark` 这类告警能在"三端 0 warning"过关的情况下溜进提交。已补 `--all-targets`。
11. **压测读数有两个陷阱**：present 节奏给了约 6ms 的地板（10k/50k 两档读到的是它，不是引擎成本；窗口被遮挡时地板消失，会读出 0.88ms/1139fps 这种数）；100k 恰好落在 vsync 边界附近，跨 run 抖动 11.9–14.2ms，需 300 帧复核。结论都记在 `docs/bench.md`。

## T5 Sprite arena + 变换 / blend / tone
- [x] `sisf-sprite/src/sprite.rs`：`Sprite{ bitmap, src, anchor, pos, scale, rotation, opacity, tone, flip_x, flip_y, z, visible, blend, anim }`；`Tone{Offset{rgb,gray}, Matrix([f32;20])}`；`BlendMode{Normal,Add,Multiply,Screen,None}`。验收：`Default` 与字段 setter 编译通过。→ `sprite::tests::{default_is_opaque_visible_main_layer, fields_are_settable_in_place, matrix_tone_has_no_pipeline}` 绿。⚠️ 两处与清单字面不同，理由见本条末尾：`anim` 字段**未加**（整套动画是 T8 的机制，现在放 `Option<AnimDef>` 只会被 T8 重设计）；`BlendMode` 不是新枚举而是 `pub type BlendMode = sisf_render::Blend`（变体必须与 pipeline 一一对应，`Multiply/Screen` 无处可去，同下条 T12 决策）。
- [x] `arena.rs`：`SpriteArena{ slots, free:Vec<u32>, gen:u32 }` + `SpriteRef(u32,u32)` + `spawn/despawn/get/get_mut/iter_live`。验收：slot 复用后旧句柄因 generation 失效（单测）。→ `arena::tests` 5 项绿，含 `reused_slot_invalidates_old_handle`（同槽不同代号 → 旧句柄 `get`/`get_mut`/`despawn` 全部无效，且写不到新精灵）、`stale_handle_cannot_despawn_new_sprite`、`free_list_recycles_before_growing`、`iter_live_skips_dead_slots_in_slot_order`。⚠️ 字段/方法名是 `generation` 不是 `gen`：edition 2024 把 `gen` 列为保留关键字。
- [x] `draw.rs`：`collect(&self,&mut BatchList)` — 过滤不可见/全透明、按 `(layer,z)` 排序、`src`+atlas 合成 uv（含 flip）、rotation 入 instance。验收：anchor/flip/z 排序单测。→ `draw::tests` 10 项绿：`anchor_offsets_center_from_pos`、`rotation_pivots_on_anchor`（锚点到中心的距离旋转后不变）、`flip_signs_size_and_keeps_uv_ascending`、`uv_comes_from_src_inside_the_page_rect`、`src_out_of_bitmap_is_clipped_not_stretched`、`invisible_fully_transparent_and_zero_area_are_skipped`、`z_orders_within_a_batch_and_layer_splits_batches` 等。锚点要同时当 `pos` 对齐点和旋转轴心，而 shader 只认"实例中心"一个枢轴 ⇒ CPU 侧算 `center = pos + R(rot)·((0.5-anchor)·|size|)`。
- [x] `Tone::Matrix` 本阶段明确标注不支持并返回错误（不写空实现分支）；`Multiply/Screen` 同 T12 决策。验收：文档 + 单测断言错误路径。→ `CollectError::UnsupportedTone{sprite: SpriteRef}`（带元凶句柄），`collect` 先在临时 `Vec` 攒完再提交 ⇒ 报错时不留半成品实例；单测 `tone_offset_maps_to_instance_and_matrix_errors` + `collect_reports_the_offending_handle`；文档 `docs/sprite.md` 的能力矩阵逐行标 ✅/❌/⏸（`Tone::Matrix` ❌ 报错、`Multiply/Screen` ⏸ 不引入变体、`Offset`/`Normal`/`Add` ✅）。
- [x] 多 viewport/layer：`Layer(u8)` + `ViewportRef`，同一 arena 可被两个 viewport 采集（RPG Maker 双层视口 / SLG 小地图）。验收：单测两 viewport 输出独立。→ `draw::tests::two_viewports_collect_the_same_arena_independently` + golden 里 `Layer(2)` 格经第二个 `ViewportRef` 采进同一帧。`ViewportRef{layer, tag}`：`layer` 是投影栈层，`tag` 是归属过滤（`Some` 只收同标签、`all()` 重画整个集合给小地图）。`Layer` 一个类型同时当归属标签与投影层是刻意的（RPG Maker 里两者本就绑定，拆开只会失同步）。
- [x] `tests/golden_sprite.rs`（`#[cfg(windows)]`）：离屏 544×416，DX12/Vulkan adapter + `COPY_DST` 读回，覆盖 opacity=0.5 / tone offset / gray / flip_x,y / anchor(0.5,1) / additive，逐通道容差 ≤1e-3。验收：`cargo xtask golden` 绿，图落 `tests/golden/`（首次 `--bless`）。→ 实现在 `crates/sisf-sprite/tests/golden_sprite.rs`（离屏 `Renderer::new_offscreen` + `readback`，15 个精灵 / 两个 `ViewportRef`）。语义断言是主证据（每条独立：翻转象限置换、`anchor(0.5,1)` 落点与左上锚点同格、`visible=false` 全黑、0.5 白读回 sRGB 188、tone offset G=188、红灰度化 0.2126→127、90° 象限置换 `[BLUE,RED,WHITE,GREEN]`、additive 0.5+0.5 饱和 vs over 0.5 双层 0.75、z=5 盖 z=-5、`src` 越界只剩 2×2 源像素→32×32 目的像素、layer 2 格），基线比对 `TOL=1` 字节/通道。`xtask golden` 由本阶段实装（`--bless` ⇒ `SISF_GOLDEN=bless`）：`cargo xtask golden` → `test sprite_scene_golden ... ok`（先 bless 再比对，两次结果一致 ⇒ 这台机器上可复现），基线 `crates/sisf-sprite/tests/golden/sprite_scene.png`。**注意**：wgpu 30 无软件光栅后端，整个文件 `#![cfg(windows)]`，非 Windows 上该 target 编译成 0 个用例并通过（跨平台无窗口冒烟由 `sisf-render/tests/readback.rs` 承担）。
- 验收（整段）：golden + arena 单测 → 提交 `P4 sprite arena+tone/blend`。→ `cargo test --workspace --all-features` **84 绿 0 warning**（本阶段新增：`sisf-sprite` 18 单测 + golden 1 + lib doctest 1；其余为 asset 16 + app 6 + media 8 + render 32 + render 集成 1 + sisf doctest 1 —— 注：T4 那行记的 58 漏计了 `sisf-app` 的 6 项与 `sisf` doctest，实际当时就是 65）+ `cargo xtask golden` 绿 + `cargo xtask check-all` 三端 0 warning（`--all-features` / wasm32 `web` / aarch64-linux-android `native`）+ `cargo xtask clippy`（win + wasm，`-D warnings`）+ `cargo fmt --all --check` 净 + `docs/sprite.md`。

### T5 期间发现并修正的事实（供后续阶段引用）
1. **锚点当旋转轴心必须换算枢轴，不是"改锚点值就行"**：`shaders/sprite.wgsl` 的顶点式
   `half=(corner-0.5)*abs(size)` + `mirror=sign(size)`（mirror 在旋转**之后**施加）意味着整块矩形
   只绕实例 `pos` 转。要"绕锚点转"就得 CPU 侧算 `center = pos + R(rot)·((0.5-anchor)·|size_abs|)`；
   直接改锚点会同时挪动落点。写反的后果是 `anchor=(0.5,1)` 的精灵转 90° 飞出原位。
2. **`z` 不能跨图集页 / 跨 blend 改变覆盖关系**：合批键 `(layer, epoch, blend, z, insertion)` 里
   `epoch`/`blend` 在 `z` 之前，所以"z=100 的精灵盖住 z=0 的"只在同页同 blend 内成立。
   RPG Maker 是 viewport 内全序 ⇒ 需要严格跨页 z 时得显式分帧画或强制同页。T6 的 clip stack /
   T8 的 `zorder` 语义都要以此为前提。
3. **图集 `src` 边界有半纹素渗色**：页采样器是 `Linear`+`ClampToEdge`，装箱不给子矩形留 gutter，
   于是放大取子矩形时边界最后半个纹素采到页上相邻图（bless 出的基线图上能看到 1–2px 色边）。
   现在 golden 只在格心/象限中心取点绕开它；**修法是装箱留 1px inset 复制边或按 `Bitmap` 选
   Linear/Nearest sampler**，归 T6（与位图渲染目标、子图集一起做）。任何人加"贴边采样"的
   断言前都得先看这条。→ **T6 已按"1px 复制 gutter"修掉**，`sprite_scene.png` 为此重写（见 T6 一节）。
4. **`Tone::Matrix` 保持"报错"而不是"当无效果"**：后者会让 RGSS 移植层写出的调色代码静默失效、
   排查成本极高。`Multiply/Screen` 同样不预先塞进 `Blend` 枚举 —— 加 pipeline 与删 RGSS 语义
   这个二选一连同 `Matrix` 通路一起在 T12 收口。
5. **`collect` 必须全成败**：`BatchList::push` 自己填 `insertion`，边报错边提交会留下序号与实例
   对不上的半批。现在是先攒 `Vec` 再落盘。
6. **`gen` 在 edition 2024 是保留关键字**：`SpriteArena{gen}` 与 `SpriteRef::gen()` 共 17 处编译错误，
   统一改叫 `generation`。清单里凡写 `gen` 的地方都按此读。
7. **离屏读回是 golden 的前置能力**，为 T5 补进 `sisf-render`：`Renderer::new_offscreen` +
   `Target::Offscreen` + `FrameStatus::Rendered` + `readback()`（去掉 256 行对齐 padding 后返回
   紧凑 RGBA8）+ `apply_atlas`。离屏纹理格式取 `Rgba8UnormSrgb`（与最常见 surface 一致 ⇒ 证明的是
   同一条色彩通路）。`readback_clear` 现在建在同一条路上。
8. **`Bitmap` 之前没有 `Debug`**，而 `Sprite` 要 `derive(Debug)`（golden 失败信息要用）：补了手写
   `Debug`（epoch/rect/page_size/pending），不 `derive` 是因为内部 `Arc` 到 GPU 资源，自动派生打不出有用东西。
9. **golden 基线落 crate 内**（`crates/sisf-sprite/tests/golden/`），不是清单写的仓库根 `tests/golden/`：
   用 `env!("CARGO_MANIFEST_DIR")` 定位，多 crate 各有 golden 时不必再约定根目录子结构。
   `png`/`pollster` 因此是 `sisf-sprite` 的 dev-dependencies（不进运行时依赖面）。
10. **期望值要写成线性值再用逆 sRGB 换算**（`byte = 1.055·L^(1/2.4)-0.055`）：0.5→188、0.75→225、
    0.2126→127。第一版把正向变换写反，读回全错。另外目标不透明时预乘混合后的 alpha 字节仍是 255，
    不是 128 —— `opacity=0.5` 只作用于 RGB。
11. **`xtask golden` 只在有 adapter 的机器上才是真比对**（见上），闸门里它绿不代表 wasm/android 侧
    像素正确；那两端仍只有 `readback.rs` 级别的冒烟覆盖。

## T6 文本 + Bitmap 渲染目标 + clip stack
- [x] `sisf-text/src/lib.rs`：`FontAtlas`（`R8Unorm` 子图集 + `etagere`）+ `layout(text,style)->TextRun`（`cosmic-text 0.19`，shaping/bidi/换行，wasm ✓），字形最终输出为普通 sprite quad、不新增 pipeline。验收：中英混排 + 自动换行；字号 12/16/24 golden。
- [x] `FontBundle::from_bytes(&[u8])` 经 VFS 载入 ttf/otf（MV/MZ 自带字体），缺失时回退默认并只 warn 一次。验收：两条路径单测。
- [x] `sisf-render/src/bitmap.rs`：`enum BitmapData{ Atlas{page,region}, Rt{texture,view} }` + `BitmapHandle(Arc<Bitmap>)` + `Bitmap::new(w,h)` + `begin_draw/end_draw`（RT 作嵌套 pass 目标）。验收：RT→RT quad 拷贝 golden 正确。
- [x] RPG Maker `Bitmap` 风格 API：`blt(src,src_rect,dst_pos,opacity)`、`fill_rect`、`clear_rect`、`draw_text(rect,text,align)`。验收：`examples/windows.rs` 全部用到。
- [x] clip stack：`push_clip/pop_clip` → `scissor_rect`，clip 变更即 flush batch。验收：单测断言 flush 次数与栈深。
- [x] `examples/windows.rs`：RPG 消息窗（半透明底 + 9 段边框 blt + 裁剪文字 + 头像 blt）。验收：桌面 + wasm 手测。
- 验收（整段）：文本 golden + 窗体 demo → 提交 `P5 text+bitmap-rt+clip`。全量落地说明见 `docs/text.md`（三条通路共用一条管线、`sync` 时机、RT 句柄生命周期、clip 断批为何是排序结果、能力矩阵与平台矩阵）。→ `cargo test --workspace --all-features` **124 绿 0 warning**（本阶段新增：`sisf-text` 23 单测 + golden 1 + `sisf-render` clip/会话 16 单测；T5 记的 84 为其基线）+ `cargo xtask golden` 绿（该命令已改为遍历 `GOLDENS` 两条：`sprite_scene` + `text_scene`）+ `cargo xtask check-all` 三端 0 warning + `cargo xtask clippy`（win + wasm，`-D warnings`）+ `cargo xtask fmt --check` 净 + 桌面 `cargo run --release -p sisf --example windows` 截图手测 + wasm 手测（`cargo xtask web windows` → `http://127.0.0.1:8080/?ex=windows&wgl`）。

### T6 期间发现并修正的事实（供后续阶段引用）
1. **mask 实例的输出必须仍是预乘的**：`sprite.wgsl` 的 mask 分支要写 `vec4(c,c,c,c)`。写成
   `vec4(1,1,1,c)` 时 rgb 与 alpha 脱钩，半覆盖像素按 over 混合直接饱和成**实心白方块**。基线图上
   一眼可见，但第一版误判成图集问题，先证明 CPU 影子页正确才定位到着色器 —— 顺序值得抄：
   覆盖率页正确性可在无 GPU 侧断言（`atlas.rs` 的单测就是这么跑的）。
2. **覆盖率页不能复用 sprite 那套图集**：页是 `Rgba8UnormSrgb`，塞 alpha 着色器取不到 `.r`，
   塞 rgb 则 sRGB 解码把 0.5 覆盖弯成 0.216、边缘糊掉。`FontAtlas` 自管 `R8Unorm` 页，
   epoch 用独立标记位 `MASK_EPOCH = 1<<61`（与 `TARGET_EPOCH`/`LARGE_EPOCH` 各占一位）——
   否则两条通路会抢同一个 bind group。epoch 的**数值序**（mask < target）就是主帧内的绘制序。
3. **`Bitmap` 句柄析构 = 页退役**：`Drop` 通知渲染器后，下一次 `apply_atlas` 直接删纹理。因此
   长存字段必须存 `Bitmap`（RT 与图集子图都是），`Source` 每次现算。第一版把 `new_target` 的返回
   值掉在构造函数里，运行时 panic `纹理代号 … 尚未注册`。
4. **`TextEngine::sync` 的时机是硬约束**：必须晚于本帧所有 `draw_*`（否则新字形没建纹理，
   回退白占位 + 告警）、早于 `end_frame`/`end_draw`（那里按 epoch 取视图建 bind group）。
   会话中途 `sync` 安全。`examples/windows.rs` 调两次：会话里一次、主帧末尾一次。
5. **清单里"clip 变更即 flush batch"不需要显式 flush**：clip 代号进 `BatchKey::clip`，断批是
   排序分组的天然结果；单测断的是"批数随栈深变化"而不是"flush 调用次数"。`NO_CLIP`(0) 恒等于
   整个目标 ⇒ 主帧与各 RT 会话共用一张 clip 表也不串味。
6. **自带字体必须显式设默认族**：`FontSystem` 候选表同分时"先注册的赢"，而系统字体几乎总是先
   注册 —— 不 `set_sans_serif_family` 就压不住它，装了也白装。回退 warn 走进程级去重（同帧构造
   几十个 `Bitmap` 不该刷屏）。
7. **wasm 上字体是不可自证的**：fontdb 在 wasm32 扫不到系统字体（`FontSystem` 只有自带字节这条
   路），而 `eprintln!` 在浏览器里看不见。所以 demo 把"哪一族 / face 数 / 回退成因 / 批数"直接画在
   屏幕上，手测不靠 devtools。仓库那份 Inter 只有拉丁 —— 中文那条字形来源见下面「wasm 中文」一节
   （运行期从 CDN 取，8MB 不入库）。
8. **`clear_rect` 与"位图是持久画布"**：wgpu 没有按矩形 clear 纹理，`clear_rect` 是一块 `Blend::Replace`
   的 quad。会话目标只在**第一次被画之前**清成透明，之后一律 `LoadOp::Load` —— 滚动文字忘记显式
   `clear_rect` 就糊成一片。
9. **T5 的半纹素渗色已修，代价是重写基线**：装箱加 1px 复制 gutter 后，`sprite_scene.png` 的格边
   从 248 一类混合值变成干净的 255；语义断言一条没改，只是原来"绕开边界采样"的注释换成了
   "边缘同样可断言"。`filter: Linear|Nearest` 的选择权仍留给 T10。
10. **~~⚠️ 遗留缺陷：非整数放大倍率下文字发虚~~**（手测时确认，本阶段未修 → **T8 已修**）。覆盖率纹素数 ==
   逻辑像素数（`Metrics::new(style.size, …)` 直接问 swash 要位图，`quad.size` 就用位图尺寸），
   而 layer 0 的 `Fit` 把 320×240 拉进 800×600 ⇒ 全帧（含字形）被共享的 `Linear` 采样器放大 2.5 倍，
   16px 的字糊在肉眼层面可辨。修法与"为什么不是换 Nearest"见 `docs/text.md`「文字光栅超采样」，
   **落地在 T8 的「虚拟分辨率映射」那条**（它才拥有 Fit/Fill/IntegerFit 的尺度）。golden 不受影响
   （离屏 scale 恰为 1）。
11. **wasm 的中文是"宿主在运行期补字形"，不是引擎能力**（T10 之后补账，起因是用户报
   「web版中文字体缺失」）。8 MB 的整包不入库，所以两个示例各自 `fetch` 一份 Noto Sans SC
   （OFL、全字库、**OTF 而不是 woff2** —— `FontBundle` 读 ttf/otf；jsDelivr `/gh/` 带
   `Access-Control-Allow-Origin: *`，且**钉 commit SHA 不钉 `main`**，分支会漂），字节灌进
   `WebManifest::insert`（它本就收清单外的键）。注册顺序是 **CJK 先、Inter 最后**：
   `register_bytes` 每注册一份就把 sans/serif 默认族换成它，先装 Inter 中文就永远停在方块。
   到货前**一个字都不画**（`Cjk` 四态 pending/system/ok/MISSING + `font_loaded` gate 住窗口与
   HUD）—— `CacheKey` 带 `font_key`，而 face 编号随注册次序重排，中途换 bundle 会让覆盖率图集
   里的旧页按新编号被误读、串出别的字。两条只有实跑才会撞上的事实：**404 页也会 resolve 出一个
   非空响应**，所以必须看 `response.ok()`，否则"取到了一份伪装成 200 的 HTML"和"取到了字体"
   长得一样；**取不到不是一种故障**，中文方块 + 状态行一句 `cjk MISSING`、拉丁照常，一份永远不来的
   请求不能把本来能显示的拉丁一起拖死。引擎侧只改了一处记账：`family()` 现在说**最后**注册的那
   一份（多份注册后它不能停在 `None` 上撒谎）。平台矩阵、失败形状与验收见 `docs/text.md`。

## T7 音频混音器 + OGG/MP3/WAV + MIDI(SF2)
- [x] `sisf-audio/src/mixer.rs`（纯 Rust，不碰设备）：`Voice{data,pos,gain,pan,loop_start,loop_end,rate}`、`render(&mut,self,out:&mut [f32])` 交织立体声；通道 `Bgm/Bgs/Me/Se`（Se 多路复用 + voice 上限抢占）、`fade`、`play/stop/pause/resume/suspend`。验收：`render` 确定性单测（四端可跑，无设备）。→ `mixer::tests` **19 项**，含 `render_is_bitwise_deterministic`（同输入两遍逐位相等）/`se_pool_preempts_the_oldest_voice`/`bgm_replacement_crossfades_the_old_voice`/`fade_stops_the_channel_exactly_at_requested_frames`（淡出按帧不按墙钟）/`suspend_freezes_positions_and_silences`/`pause_silences_the_channel_and_freezes_its_position`（通道级暂停是独立状态）/`output_is_hard_clamped`/`invalid_inputs_are_rejected_not_panicked`。`mixer` 无条件编译，任何 feature 组合都在。
- [x] `sisf-media/src/audio.rs`：`trait Decoder{ channels, sample_rate, read, seek_sample }`，`symphonia 0.6.1 features=["ogg","vorbis","mp3","wav"]`（纯 Rust，wasm ✓）；`lewton 0.10.2` 仅作 `ogg-lewton` 回退 feature（默认关）。验收：OGG/MP3/WAV fixture 采样长度断言。→ `audio::tests` 7 项 + `audio::lewton_backend::tests` 5 项（`ogg-lewton` 下）：三份 fixture 的帧数/时长/采样率逐条断言（OGG 88200 帧 = 2.00s@44.1k、WAV 22050、MP3 44100），`sniff` 按魔数分派（不认扩展名）、未开 feature 的容器报 `UnsupportedFormat`。
- [x] 循环点：`lofty 0.25.4` 读元数据 → `AudioData{loop_start_samples,loop_end_samples}`。验收：带 loop 的 OGG 无缝循环（samples 级断言 + 听测）。→ fixture 的 `LOOPSTART/LOOPEND = [8820, 35280)` 解码后原样保留（`audio::tests` 逐值断言 + `bus::tests::ogg_loop_tags_survive_into_a_voice` 在 `AudioBus` 通路上断言 pump 80 次后落在循环区内）；`metadata_loop_points_wrap_seamlessly`/`whole_file_loop_restarts_at_zero` 钉住"越过 `loop_end` 才回绕、intro 照常播完"。听测：`cargo run -p sisf --example audio_demo --features ogg,mp3,wav,midi -- 14` 复跑，12s OGG 回绕处 `underrun+0`（修前是 `+4480`，见下第 7 条）。
- [x] `pump_native.rs`：`cpal 0.18.2` ~~回调内直接 `Mixer::render`~~（做不到，见下第 5 条：改为主线程混音 + SPSC 环，锁仍为 0）；设备变更与 `StreamError` 重建流。验收：Win 播放无爆音，插拔耳机不断流。→ `pump_native::tests` 7 项（环跨线程搬运逐样本对齐、环绕与 overflow 计数、`pump_runs_silently_without_a_device`、`refill_target_covers_a_whole_slow_frame` 钉住第 14 条那个帧率阈值）+ 真设备复跑 14s（完整日志不过 pipe）：`ring=1024~1152f underrun=64s overflow=0s callbacks=1389 streams=1 err=-`、逐秒 `underrun+0`、`混音推进 13.93s / 墙钟 14.00s（99%）`。64 样本全部来自"启动时宿主无默认输出设备 → `OPEN_RETRY` 2s 后拿到 `Headphones … @48000Hz/2ch F32`"那一窗（降级路径被真实撞到）。⚠️ **插拔耳机/切默认设备不断流仍待人工**（需要物理操作，机器替代不了；客观判据 = `开过 N 条流` 从 1 变 2，而那 14s 里 `streams=1` ⇒ 这条路径至今**没有**被真实走过）。
- [x] `pump_web.rs` + ~~`web/audio_worklet.js`~~（落地成 `crates/sisf-audio/src/audio_pump.js` 内联 Blob，见下第 6 条）：`AudioWorklet`（不可用则 `ScriptProcessor`）从 WASM 拉混音输出；`AudioBus::suspend/resume` 绑 visibilitychange + 首次点击解锁。验收：Chrome-wasm 播 BGM/SE，`docs/audio-web.md` 记延迟实测（目标 ≤~21ms@1024）。→ 编译面绿：`cargo xtask check-all` 新增的第四组 `wasm32 --no-default-features --features web,ogg,mp3,wav,midi`（含 `--all-targets`，即示例本身）0 warning。通路选择/解锁/延迟口径写成 `docs/audio-web.md`。这一行浏览器真实读数（`state=running kind=worklet worklet=yes baseLatency=0.0100s outputLatency=0.0400s`）反过来暴露了两个只有看数才会发现的 pump 缺陷，修完的可复现证据是
`node crates/sisf-audio/tests/pump_bookkeeping.mjs`（把胶水连同 `WORKLET_SRC` 里**真实的**处理器
一起装进 node 跑 48k 虚拟时间线；判据 = 报的深度 == 真实排队量 ±1 quant、不为负、预热后
`underrun` 不涨、60fps 深度恰为 1024、两条拆账恒等式 `queued`==真实排队量 与
`buffered-真实==inflight`）：fps=60 峰值 1024~1120f 21.3~23.3ms 低水位 128f queued 误差 0.5f +0 /
50 +0 / 42 1280~1440 +0 / 30 1920~2080 +0 / 20 2880~3040 +0；反例（修账前的胶水放同一位置）
`FAIL 最大差 238507 帧`。见下第 12、14 条。人工复验**已做**（用户硬刷新 + 点击，我从页面里直接采
`sisfAudioStats()`）：`state=running kind=worklet`，稳态 `buffered=1024~1280f`（=21.3~26.7ms，
合计 68.7ms 里 42ms 是 `baseLatency 10ms + outputLatency 32ms`），`buffered` 全程不为负 ⇒ 实测表
两列已填。残余已定量（ack 回报 `queued` + 2ms 采水位）：是 ~3 个 quant 的**在途窗口**、只在 >25ms 的
主线程长帧上被击穿，不是账错，见下第 15 条。
- [x] `sisf-media/src/midi.rs`（feature `midi`）：`midly 0.5.3` 解析 + `rustysynth 1.3.6` SF2 合成，`SoundBank::from_sf2(&[u8])` 经 VFS（**引擎不附带 SoundFont**），MIDI 作为一路 Voice 参与同一 Mixer。验收：给定用户 SF2 时播放 fixture `.mid`；无 SF2 返回 `MissingSoundBank` 并提示（不 panic）；文档给出"离线把 .mid 预渲染成 .ogg"退路。→ `midi::tests` 9 项（`midi_info` 轨数/时长、`MidiDecoder` 定长读、`seek_sample(0)` 回绕、循环类型分派、无 bank ⇒ `MissingSoundBank`；其中 2 项需要用户素材，做成 env 开关：`SISF_SF2=/path/to/bank.sf2 cargo test -p sisf-media --features midi -- --ignored`，断"同参数两次渲染逐位一致"与"内部循环模式永不结束、只在 0 处回绕"）+ 真机两遍：demo 的 `M` cue 打出 `缺少 SoundFont（SF2）: …或离线把 .mid 预转成 .ogg`，通道状态不变、不 panic。SF2 那条只能由用户自备素材自证（仓库故意不存，退路命令在 `docs/audio.md`）。
- [x] `examples/audio_demo.rs`：BGM 循环 + `Space` 播 SE + `M` 播 `.mid`（键盘事件本阶段还没有，改成确定性时间线驱动同一套 `fire()`，见下第 8 条）。验收：`cargo xtask check-all` 含 `--features midi` 组合绿。→ 示例注册带 `required-features = ["ogg","mp3","wav"]`，故 web-only / native-only 两组是**跳过**而不是失败；`xtask web` 构建 feature 从 `web` 改成 `WEB_AUDIO_FEATURES = "web,ogg,mp3,wav,midi"`。四份 fixture 已进 `web/assets/manifest.txt`。
- 验收（整段）：mixer 单测 + Win/Chrome 听测 → 提交 `P6 audio+ogg/mp3/wav+midi`。落地说明见 `docs/audio.md`（四层与 feature 边界、通道模型与循环默认值、三个容器三种循环点读法、native 主线程混音 + SPSC 环、MIDI 三档兜底、能力/平台矩阵、两个真坑）+ `docs/audio-web.md`（worklet 与 ScriptProcessor 的语义差、深度记账与帧率阈值两节、延迟口径、手测步骤与实测表）。→ 已提交 **`3ef3c66`**：`cargo test -p sisf-audio --all-features` **34 绿**（mixer 19 + bus 8 + pump_native 7）+ `cargo test --workspace --all-features` 全绿 0 红（2 项 `SISF_SF2` ignored）+ `cargo xtask check-all` 四组 0 warning + `cargo xtask clippy`（win + wasm，`-D warnings`）+ `cargo xtask fmt --check` 净 + `node --check` 与上面那条 node 侧契约测试 + Windows 真设备有界复跑（第 4 条那行）。⚠️ 整段验收还差两件人工：Win 播放中切默认输出端点/插拔耳机（判据 `streams` 1→2）；这两听之外，Chrome 侧的"缺口是否可闻"也只能由耳朵定 —— 若可闻就把 `TARGET_FRAMES` 抬到 1536（可控延迟 21.3→32ms，见 `docs/audio-web.md` 的定价段）。

### T7 期间发现并修正的事实（供后续阶段引用）
1. **`sisf/{ogg,mp3,wav,midi}` 必须同时转发 `sisf-media/*` 与 `sisf-audio/*`**。第一版只转给 media，
   于是示例里 `bus.play_midi(...)` 报 `E0599 no method named play_midi`（`AudioBus` 的 MIDI 方法门控在
   `sisf-audio` 自己的 `midi` 上）。门面转发规则：**下游 crate 里凡是 gate 了某个 API 的 feature，
   转发时不能省**，否则 API 在门面层蒸发。
2. **ME 不能默认循环**。独占通道 ≠ 循环：早期写法 `looping = !channel.shared()` 把 ME 算进了 BGM 那一侧，
   一声过关音乐就此挂住不放（demo 日志里 ME 的位置每秒在 470/587 之间来回 = 每秒回绕一次）。现在
   `Channel::loops_by_default()` 只对 `Bgm/Bgs` 为真，`only_background_channels_loop_by_default` 逐通道钉住。
3. **`play_bytes` 在调用线程整段解码**，2s 立体声 OGG 在 debug 构建 ~90ms，而输出环目标深度只有
   `TARGET_FRAMES = 1024`（≈21ms）⇒ 回绕那一下真能听见爆音（实测 `underrun+4480`）。游戏侧正确形状是
   **读一次、解一次、之后复用 `Arc<AudioData>`**；异步版走 T4 的 `AssetStore`。这条决定了示例的形状。
4. **lofty 必须保留默认 feature `id3v2_compression_support`**：0.25.4 关掉它会在
   `id3/v2/frame/read.rs` 留下未导入的 `Id3v2Error*`（上游 cfg 断裂）→ 直接编译失败。另外循环点必须走
   `VorbisFile`/`MpegFile` 这类**具体文件类型**：通用 `TaggedFile` 通路会把映射不到的键丢掉，而
   `LOOPSTART` 恰好就是映射不到的那种（`ItemKey` 是封闭枚举，无 unknown 变体）。
5. **cpal 的 data callback 是 `FnMut + Send + 'static`，`Mixer` 里有 `Box<dyn Decoder>`（非 `Send`）**
   ⇒ 清单字面的"回调内直接 `Mixer::render`"做不到：要么套锁（清单禁止），要么 `unsafe` 造跨线程 `&mut`
   （UB）。形状改成"主线程混音 + SPSC 环（原子下标 + `AtomicU32` 样本位型，Relaxed 存样本 /
   Release-Acquire 发布游标）+ 回调只取数与格式转换"，锁依然是 0 个。`Ring` 的 `underrun_samples` /
   `overflow_samples` 是这条通路的观测量。
6. **AudioWorklet 不能同步回调主线程 WASM**（需要 SharedArrayBuffer + Atomics ⇒ COOP/COEP 跨源隔离 +
   `+atomics` 目标，本仓库两样都没开）。所以 worklet 通路 = 需求上报（`sisfAudioFramesNeeded`）+ 主线程
   按需求渲染 + `postMessage` 拷贝；只有 `ScriptProcessor` 回退通路才是字面的同步 pull。清单里的
   `web/audio_worklet.js` 落地成 `audio_pump.js` 里的 `WORKLET_SRC` 内联 Blob（`addModule()` 只吃
   URL，用 `URL.createObjectURL(new Blob([…]))` 免开文件）。**胶水本身不是内联的**：
   `module = "/src/…js"` 让 wasm-bindgen 把它拷成 `web/pkg/snippets/sisf-audio-<hash>/src/audio_pump.js`
   再由 `pkg/<例>.js` 相对 import —— 静态服务必须连 `snippets/` 一起给（`xtask web` 服务整个 `web/`，
   正好覆盖），改了胶水要重跑 `xtask web` 且浏览器可能缓存那个 URL。
7. **"开不出输出设备"必须是一等可观测状态而不是错误**。`Pump::new()` 永不失败；无设备时走静音混音
   （混音时钟照常推进、fade 照常到点、SE 照常回收），`stats().device == None` + `describe()` 打出来。
   本机真实撞到：进程启动时宿主没有默认输出设备，2s 重试后才拿到 `Headphones … F32`，全程 `streams=1`
   （没有重建流）· `err=-`。CI 与无声卡机器因此能跑完整段验收。
8. **`EngineEvent` 现在只有 `Lifecycle`**（Key/Pointer/Action/Custom 在 T9）⇒ 示例无法用 `Space`/`M`，
   改成 12s 确定性时间线驱动**同一个** `fire(cue)` 函数。T9 落地后把时间线换成事件回调即可，播放代码不动。
   另外 wasm 首帧素材还在飞（`fetch` 异步），示例每帧试读 VFS 直到齐。
9. **`&[f32]` 进 wasm-bindgen import 参数是零拷贝 Float32Array 视图**（指向 wasm 内存）。它只在"胶水绝不
   回调 Rust"的前提下安全 —— 回调会让内存增长、视图失效。`audio_pump.js` 目前只做 JS 侧分配与
   `postMessage`（结构化克隆在调用内同步取快照），成立；哪天要回调就得改成先拷一份。
10. **native 与 web 的"欠载"单位不同，不要硬凑成一个数**。cpal 回调按样本要数 ⇒ `underrun_samples`；
    WebAudio 按帧记 ⇒ `underrun_frames`。`AudioBus::underrun_units()` 返回各自单位（判据一致：稳态不涨），
    设备换没换、通路是 worklet 还是 script-processor 留在 `describe()` 的一行文字里。曾试过 `if cfg!(…)`
    在两个不同 typed `PumpStats` 字段间选一个 —— 两个分支都要过类型检查，编不出来，改成各 pump 自己
    实现 `underrun_units()`。
11. **`cargo xtask check-all` 从三组变四组**：新增 `wasm32 --no-default-features --features web,ogg,mp3,wav,midi`
    （音频解码面在 wasm 上必须绿，而默认 `web` 组不含解码 feature，覆盖不到 `play_midi` 这类门控 API）。
    `xtask web` 同步换成 `WEB_AUDIO_FEATURES`，否则浏览器上的音频 demo 会缺解码后端。
12. **worklet 的缓冲深度账不能双扣，静音不算消耗**。第一版 `applyWorkletStats` 既从 `workerPending`
    里减、又加进 `workerConsumed`，而 `bufferedFrames()` 报的是两者之差 ⇒ 播一帧扣两帧；处理器那边
    又是 `this.consumed += n`（取不到数据的静音帧也计）⇒ 上下文刚 running 时深度直接做负。
    浏览器上看到的形态很骗人：`underrun` 冻结在启动值、`buffered` 忽 0 忽 -1024，而**真实排队长度一路
    涨**（延迟跟着涨，闭环永不收敛）。改成只留 `workerPending` 一个在途量 + 处理器分开报 `played`
    （真取走的帧）和 `silent`（只进 underrun）。回报节奏也一起改：原以为"攒够 512 帧再报一次"能省
    消息，实际**回报间隔 = 主线程看到的深度的陈旧上界**（`workerPending` 只在收到回报时才降），
    512 帧虚高正好吃掉需求闭环那 1/4 余量，60fps 下仍欠载 ⇒ 改成每个 quant 都报（48k 下 375 条/秒、
    每条三个小整数），取不到数据的纯静音期也必须报（`frames: 0` 是有效回报）。
    这类账 Rust 单测碰不到、只能靠浏览器控制台又太慢，落地成 node 侧的契约测试：
    `crates/sisf-audio/tests/pump_bookkeeping.mjs` 直接 import 真胶水 + 真 `WORKLET_SRC`，stub 掉
    WebAudio，在 48k 虚拟时间线上跑 60/50/42/30/20fps，断言"报出去的深度 == 真实排队（±1 quant）"、
    非负、预热 1s 后 underrun 不再涨。跑法：`node crates/sisf-audio/tests/pump_bookkeeping.mjs`。
13. **`WORKLET_SRC` 是模板字符串 ⇒ 里面（含注释）不能出现反引号**。修账时随手写的注释两次截断了模
    板（`` `played - acked` ``、`` `workerPending` ``），JS 语法当场坏掉，而 cargo 一侧毫无感觉 ——
    胶水文件不参与编译检查。防线：`node --check crates/sisf-audio/src/audio_pump.js`（几毫秒，抓住模板
    截断/引号失衡），上面那个 `.mjs` 契约测试顺带也起同样作用（它要能把文件 import 进来）。
14. **需求闭环的阈值不能只写"补到目标深度"**。第一版 `framesNeeded = TARGET - buffered`，于是每个 rAF
    至多送 `TARGET` 帧，而这一帧本身要播掉 `rate/fps` 帧 ⇒ 主线程慢于 `rate/TARGET ≈ 47fps`（48k 下）
    就**结构性欠载**，与是否有 bug 无关，只是账没算够。改成
    `target = max(TARGET_FRAMES, 这一帧要播掉的量 + 1/4)`，再被环容量夹住；native 侧同一个道理同一个式子
    （`pump_native.rs::refill_samples(taken, capacity)`，`[GAME][AUDIO]` 那行日志两平台对齐）。两个坑
    记在 `docs/audio-web.md`：① 这个增量必须**在一个 tick 内保持一致**（先按"上次报的量"算，若只让第一
    次调用看得见就会退化成每 rAF 只补 256 帧的一个 block，实测 `want=2048 pushed=1024` 仍欠载），所以
    JS 里存了 `catchUpFrames`；② 判据用 `refill_samples(2286, cap) > 2286` 这种"一帧 30fps 也补得回来"
    的形状断言，别只断"等于 TARGET"。
15. **浏览器上的残余 `underrun` 是"在途窗口"，不是记账错，也不是按比例预算不足**。先采到形态：每
    rAF 采一次 `sisfAudioStats()`（同 URL 动态 import ⇒ 同一个模块实例），8s / 681 tick：帧时长 p50
    12ms、p99 25ms、max 28ms，**8 个 tick 有增量、其余 673 个全零，且每个增量都是 384 帧**。缺口
    *不随 dt 增长* ⇒ 不像预算，像某个固定量被错算成了可用深度。于是让处理器随 ack 报回自己队里的
    真实剩余（`drain{..., queued}`，槽 10/11 = `queued`/`inflight`，控制台那行同步打印），再按 2ms
    采样把水位量出来：9s / 774 tick，`queued` 低水位均值 376f、高水位均值 **760f**，而同一时刻报的
    `buffered` 峰值 ~1150f —— 差的 **390f ≈ 3 个 quant ≈ 8ms 正等于那个恒定的 384 帧**。含义：
    `postMessage` 出去、处理器还没拿到（拿到到可见之间又播掉一点）的帧被算进了可用深度，补给一慢，
    处理器先把**看得见的**吃完，缺口长度就等于这笔在途量。记账本身是对的（两条恒等式在 node 里
    ≤1.5f：`queued` == 真实排队量、`buffered - 真实 == inflight`），复现形态要靠探针
    `--transit 1`（60fps 稳、50fps 起结构性欠载）与 `--transit 3`（60fps 就崩）。
    定价：改需求阈值为按 `queued` 与把 `TARGET_FRAMES` 抬到 1536 **在延迟上等价**（都要多囤 ~8ms），
    而清单钉的是"≤~21ms@1024" ⇒ 本阶段保留 1024，把"长帧（>25ms，这里是 WARP 软件渲染）上每次缺
    8ms"如实记成待决，不偷偷改延迟契约。native 侧无此纠结（cpal 线程不抖，13s `underrun+0`）。


## T8 动画 + MoveRoute + 固定 tick
- [x] `sisf-sprite/src/anim.rs`：`AnimDef{ frames:[FrameDef{rect,dur_ms}], loop_: Once|Pingpong|Loop{n}, pattern: Option<DirPattern> }`、`AnimState{def,speed,index,paused}`、`DirPattern::RpgMaker4x3`（MV，`dir + 3*step`）/ `RpgMaker2x2`（VX/Ace）。验收：两图样帧索引单测（下/左/右/上四向）。→ `anim::tests` **14 项**：`mv_4x3_indexes_every_direction_by_step` 与 `ace_2x2_folds_the_third_step_into_the_first_column`（就是验收点名的"两图样四向帧索引"）、`both_patterns_agree_on_direction_rows`（两图样的 下/左/右/上 行号逐条相同，只差列数 3 vs 2）、`turning_and_stepping_compose_into_the_expected_cell`、`grid_frames_read_row_major`、`short_frame_tables_clamp_instead_of_running_off_the_sheet`（帧表短于图样时夹住，不 panic）。清单字面的 `dir + 3*step` 是列优先读法，本仓库统一行优先 `dir_row*cols + step%cols`（见下第 5 条）。
- [x] 步帧由移动驱动（RPG Maker 语义）：`set_step(u8)`，静止时不推进帧。验收：单测静止不换帧。→ `movement_driven_frames_never_advance_on_their_own`：`pattern: Some(..)` 时 `tick(500ms)` 之后 `rect()` 与 `index` 一动不动，只有 `set_step(1)` 才换列（`arena_tick_applies_frames_only_to_sprites_that_changed` 同向钉住 `tick_anims` 只碰 `pattern: None`）。时间驱动那一侧由 `time_driven_anim_advances_and_stops_at_the_end` / `loop_counts_cycles_and_zero_means_forever` / `pingpong_bounces_without_repeating_the_ends` / `speed_and_pause_drive_the_clock` 覆盖。
- [x] `sisf-app/src/timing.rs`：固定 tick + 累加器（补帧上限 5）+ `draw(alpha)` 插值；`Suspended` 冻结累加器。验收：dt 尖峰与补帧上限单测。→ `timing::tests` **6 项**：`a_huge_spike_is_capped_and_its_debt_is_dropped`（1s 只补 `MAX_CATCHUP=5` 拍，其余进 `dropped`，不拖死下一帧）、`a_frame_shorter_than_a_period_produces_no_tick`、`alpha_is_the_leftover_of_the_accumulator`（16ms@60Hz ⇒ alpha≈0.96）、`steady_frame_rates_are_all_one_tick_per_frame_or_none`（30/60/120Hz 下游戏速度一致）、`reset_freezes_the_accumulator_for_a_resume`、`bad_config_degrades_instead_of_stalling_the_loop`。接线在 `runner.rs`：`RedrawRequested → clock.step(dt) → 0..=5 次 App::tick(周期) → App::draw(alpha)`，挂起那一支同时 `last=None` + `clock.reset()`。
- [x] `sisf-sprite/src/task.rs`：`schedule/tick` 任务调度器，承载 `Tween<T>` 关键帧与 MoveRoute。验收：Tween 完成回调按 tick 计数。→ `task::tests` **15 项**：`tween_completion_is_counted_in_ticks`（回调落在第 N 拍，与墙钟无关）、`tween_interpolates_its_keys_and_eases_each_segment`、`easing_curves_stay_inside_their_interval`、`a_single_key_tween_lands_once_and_finishes`、`a_task_on_a_despawned_sprite_ends_itself`、`cancel_drops_only_that_sprites_tasks`。示例里 `open_chest` 用 5 关键帧闪烁（按拍），完成回调换图块 + 把该格转可走。
- [x] `MoveRoute{ nodes:[RouteNode], speed, through }`：`Wait/Turn/Move(dir,n)/MoveTo/Jump/FacingLock`，网格（RPG）与自由（SLG）双模式。验收：逐格移动 + `wait:true` 存活至完成。→ `move_route_walks_cell_by_cell`（`TILE/speed` 拍一格，逐格 committed）、`a_waiting_route_keeps_the_sprite_queue_alive` vs `a_non_waiting_route_runs_alongside_its_sibling`（队列语义正反两面）、`walls_void_the_rest_of_a_node_but_not_the_route`、`grid_move_to_hops_axis_by_axis_until_it_arrives`、`free_mode_goes_straight_and_snaps_to_the_goal`、`a_jump_clears_a_gap_without_stepping`、`facing_lock_moves_without_turning_the_graphic`、`turn_and_wait_cost_one_tick_each`。
- [x] 虚拟分辨率映射：`Fit|Fill|IntegerFit` + letterbox，指针/触摸坐标自动映射虚拟空间。验收：整数缩放无半像素抖动、点击命中正确。**接 T6 遗留**：文字覆盖率按逻辑像素光栅、放大倍率非整数时会发虚（`docs/text.md`「已知缺陷」），本条要一并给出光栅超采样档（`raster_scale`）或全分辨率文字层，二选一并写进文档。→ `frame::tests` **9 项**：`integer_fit_with_odd_padding_stays_on_the_pixel_grid`（1001×701 装 544×416：未取整的原点是 (228.5,142.5) ⇒ 每像素采样在两个纹素正中，现在钉成整数格线）、`pointer_hits_the_expected_cell_after_the_letterbox`（三种缩放下中心与右下角都自洽，映射与投影同一个 `Viewport` 出处）、`text_raster_tier_follows_the_upscale`（1:1→1 档、2×→2 档、缩小→1 档、3×→4 档）。**二选一选的是光栅档位**（不做全分辨率文字层，理由与代价写在 `docs/text.md`「文字光栅超采样」：页容量按档位平方涨，档位有 `MAX_RASTER_SCALE` 上限）；`atlas::tests` 补 `supersampling_adds_detail_without_moving_the_glyph`（换档只改粗细，逻辑宽高与落点不动）与 `a_nonsense_tier_falls_back_instead_of_poisoning_the_atlas`（NaN 回 1 档，见下第 7 条）。
- [x] `examples/tiled_map.rs`：三层 tilemap（地面/事件/上方，图块 32/48）+ 键盘 4 向行走 + 事件格碰撞。验收：桌面 IntegerFit 544×416 + wasm 手测。→ 示例注册 `required-features=["image"]` + `test=true`，`cargo test -p sisf --all-features --example tiled_map` **10 绿**（地图形状/边框封死、格点阵往返、相机居中后夹边、三层 z 序 ground<decal<y-sort<above、`MoveRoute` 撞在真实通行表上、位图区域不越界、预乘 alpha + 空边距、按键 held 语义、HUD 层净投影、四向键）。美术整张程序生成（256×288 单页 ⇒ 单 epoch ⇒ y 排序有全序权），不新增素材/依赖。桌面 IntegerFit 实测（`cargo run --release -p sisf --features image --example tiled_map`，日志 + 抓屏像素扫描）：
  - `物理 800x600 ⇒ 画面 (128,92) 544x416，倍率 1.00，光栅档 1` / `物理 1185x862 ⇒ 画面 (48,15) 1088x832，倍率 2.00，光栅档 2`；`第 1 帧合成: batches 4 quads 728 sprites 637 pages 1 tier 1x`（三层 + HUD 合成 4 批）。
  - 键盘链路由 `PostMessageW` 打方向键走真窗口：`角色到格 13,8（相机 174,56）` → `16,12` → 北向 `16,8…13,4`，相机跟着挪并在北边/西边夹住（`cam 224,184`→`224,70`→`160,0`）；`Enter` 打出 `宝箱打开，该格转为可走`，随后同一格走得过去（碰撞表被事件改掉）。
  - 边条不渗色（像素级）：修前 800×600 的 y=300 一行在 client x∈[0,127] 上是石墙/草地色；修后同一行 client x=0..127 恒为清屏色 `rgb(10,15,23)`、x=136（= 画面左沿）起才有内容；2× 下图区左沿正好落在 client x=48；顶条整行（client 0..1184）零跳变。HUD 可读性：1× 的 HUD 带内 ≥200 亮度像素 1417 个（同图地图区只有 56 个 = 灯火），2× 带内 6148 个，裁剪 1:1 读数 `tick 25472 a0.92 cell 13,4 cam 160,0` / `opened the chest`。
  - wasm 手测（`http://127.0.0.1:8080/?ex=tiled_map&wgl`，ANGLE Microsoft Basic Render Driver /
    WebGL2）：`物理 544x416 ⇒ 画面 (0,0) 544x416，倍率 1.00，光栅档 1` → `字体 Some("Inter")（face 1）`
    → HUD `tick 2431 a0.15 cell 12,12 cam 128,184` / `batches 4 quads 737 sprites 637 pages 1 tier 1x`
    （与桌面同一读数）。按住方向键（合成 `keydown`/`keyup` 派发）逐格日志：北向
    `12,11→12,10→12,9→12,8→12,7`（相机 `128,166→…→128,38`），再东向 `13,7→14,7→15,7`、北向
    `15,6→15,5` 且相机在北边夹住（`cam 224,0`）⇒ 键盘→`MoveRoute`→相机整条链在 wasm 上通。
    ⚠️ 首帧那行 `第 1 帧合成: batches 0 quads 0` 是真的：`stats()` 读上一次 `end_frame` 的账，
    而 wasm 的第一帧跑在尺寸到货之前（0×0 的空 scissor），桌面则第一帧就有尺寸。
- 提交后用户手测报回两条缺陷（"小问题：人物左右贴图反了；大问题：人物一直在抖动"），两条都已修 + 钉住（修复提交 = `eeb86b6`）：
  - **抖动 = 相机与角色不同相位**（第 15 条）。修完在 `?ex=tiled_map&wgl`（544×416、倍率 1.00，1 虚拟
    像素 = 1 物理像素）用逐帧像素探针复验：按住 ArrowRight、每个 rAF 把 canvas `drawImage` 到离屏再
    `getImageData`，取玩家上衣蓝的最左列 x。旧相位 37 帧读数是 `260,260,261,261,259,260,…`（跨度 2px
    的锯齿，正是用户看到的抖），新相位同一探针 38 帧恒为 `261`（跨度 0）而地图照旧在滚。
  - **左右反了 = 自绘素材把头发画在朝向侧并盖掉眼睛**（第 16 条）。同一探针读头顶那 5 行的墨点列：
    朝左 ⇒ 墨点在格内 x=9/10、头发 72 px 在右半；朝右 ⇒ 墨点在 x=21、头发 50 px 在左半。截图可见
    蓝衣角色后脑在左、脸在右（当时正按住 ArrowRight）。
  - **修完第 15 条后用户复测报回"整个地图在抖"（第 18 条）**：把 `lattice_alpha` 加上之后同一探针换
    成"相邻两帧最佳整数位移"判据，在 `?ex=tiled_map&wgl`（544×416 ⇒ 1 虚拟像素 = 1 物理像素）实测：
    按住 ArrowLeft 60 个有效帧 ⇒ 59 对相邻帧平均 0.13% / 最大 0.18% 对不齐，最佳位移恒为 `dx = 2`
    （偶尔 1/3，从不为负），每帧 distinct 颜色数恒为 41；把 `lattice_alpha` 临时改成恒等函数重编
    重测（同一探针、同一按法）⇒ 平均 17.16% / 最大 19.89% 对不齐，distinct 颜色数 64~145（多出来
    的就是重采样混出的中间色）。纵向按住 ArrowUp 55 帧：平均 0.21% / 最大 0.34%，`dy` 恒为 2~3。
    恢复取整、重编后再测一遍确认回到 0.13%。桌面同版本（release）冒烟见下条。
  - 桌面同版本冒烟：`tiled_map.exe` 起 `adapter: NVIDIA GeForce RTX 2080 Ti (DiscreteGpu, Vulkan)`、
    `物理 800x600 ⇒ 画面 (128,92) 544x416，倍率 1.00，光栅档 1`、`batches 4 quads 732 sprites 637 pages 1 tier 1x`，
    无 panic。闸门重跑（原命令）：`cargo xtask fmt --check` 净、`cargo xtask clippy` 两端 0 warning、
    `cargo xtask check-all` 四组 0 warning、`cargo test --workspace --all-features` **238 绿 0 红**
    （本阶段 +3：`the_following_camera_shares_the_hero_s_interpolation_phase`、
    `the_side_walk_frames_actually_look_left_and_right`、
    `the_following_camera_snaps_the_world_to_the_pixel_lattice`）、`cargo xtask golden` 两条全绿。
    第 18 条的修复落在 `322e520` 之后的那条提交里，闸门用同一套原命令重跑：`cargo xtask fmt --check`
    净、`cargo xtask clippy` 两端 0 warning、`cargo xtask check-all` 四组 0 warning、
    `cargo test --workspace --all-features` **238 绿 0 红**、`cargo xtask golden` 两条全绿，桌面
    release 冒烟（Vulkan，`batches 4 quads 732 sprites 637`）无 panic。
  - **第二次复测仍报回"还是抖，人物不抖背景抖"（第 20 条）**：前两条都对，晃的不是映射而是记账 ——
    `MoveRoute` 每格多烧一拍空转 ⇒ `TickClock` 消耗周期把 `alpha` 归零，`prev_pos`/`pos` 却没动，
    `interpolated(alpha)` 把最后一步重播一遍；相机把角色钉在屏幕正中，所以只有背景露出错位。
    判据用**离线复放**（浏览器像素探针在这条路上不可信：页面被遮挡时 `getImageData` 返回冻结缓冲，
    见第 22 条）：把 `?ex=tiled_map&wgl` 实测的节拍 `[18.2,12.1,18.2,24.3,…]`（均值 17.7ms ⇒
    56.5fps）逐帧喂进 `TickClock`，84 帧后读 `cam × scale` ——
    **离理想滚动 `|cam - 120·t|`：修账前 12px、修账后 2px**（= 一拍的固定滞后，且从不回退）；
    **总位移：166px → 176px**（理想 178px）。钉成单测
    `the_camera_tracks_the_ideal_scroll_under_a_jittery_frame_rate`。
    浏览器侧同一动作的每格耗时（应用自己的带时间戳逐格日志）从 ~283ms（3.53 格/s，慢 9%）回到
    **263/267/257ms**（理想 266.7ms = 3.75 格/s = 16 拍/格）。字节新鲜的重建（`cargo xtask web tiled_map`
    ⇒ `web/pkg/tiled_map_bg.wasm` 15:20:34；注意不带示例名时它缺省只构建 `hello_window`）在同一页面
    按住 ArrowLeft 2409ms 复测：9 个格间隔 `279,257,279,253,270,275,265,268,255` ⇒ **均值 266.8ms**
    （±12ms = rAF 的 18ms 量化，不是速度偏差）。
    顺手把格点从虚拟格推广到设备格（第 21 条，2× 下 5 个台阶 vs 3 个；1× 下恒等 ⇒ 不是这次的解药），
    并把 `the_following_camera_snaps_the_world_to_the_pixel_lattice` 挪到 1×、新增
    `the_camera_lattice_is_the_device_pixel_grid_not_the_virtual_one`（2×，含虚拟格对照组）。
    残余的逐帧位移 1~3px 散布已定量 = 软件 WebGL 的节拍噪声，位置误差 ≤2px 且单调 ⇒ **不是未修问题**。
    闸门重跑（原命令）：`cargo xtask fmt --check` 净、`cargo xtask clippy` 两端 0 warning、
    `cargo xtask check-all` 四组 0 warning、`cargo test --workspace --all-features` **240 绿 0 红**
    （较上条 +2：`the_camera_tracks_the_ideal_scroll_under_a_jittery_frame_rate`、
    `the_camera_lattice_is_the_device_pixel_grid_not_the_virtual_one`；
    `cargo test -p sisf --all-features --example tiled_map` 10 → **15 绿**，
    `cargo test -p sisf-sprite --lib` **48 绿**）、`cargo xtask golden` 两条全绿。
- 第四次 + 第五次报回（"**红色人物移动后人物抖，玩家角色移动后地图抖**"，桌面 release 与浏览器同时复现，
  与前三次同形）：第 20 条只修掉了**路线收尾**那一种空转拍，剩下的一半是**所有"这一拍没动"的拍** ——
  `prev_pos` 只被 `set_pos` 推进，于是玩家走完一格松手的停拍、NPC `RouteNode::Wait` 的每一拍、
  撞墙被作废的剩余格数，都把最后一跳留在 `prev_pos`/`pos` 之间，被 `alpha` 的 `0→1` 一遍遍重播
  （NPC 自己弹 ⇒ "人物抖"；玩家弹带动跟随相机 ⇒ "地图抖"）。逐帧日志坐实：静止段
  `heroD 256.00,386.00 camD 0.00,154.00` → `385/153` → `384/152` → 回到 `385/153`，
  而 `alpha` 只是在 `0.18→0 / 0.55→0.5 / 0.91→1` 之间循环。
  **修法不是让每个逻辑各自记得推进，而是引擎侧统一收拢**：新增 `Sprite::begin_tick` /
  `SpriteArena::begin_tick`（`App::tick` 第一行调一次，必须在任何移动之前 —— `tick_anims` 在拍尾，
  那时收拢等于把本拍位移当场抹掉），约定写进 `docs/sprite.md`。四道新单测全部**先证伪再证真**
  （把 `begin_tick` 那一行注掉即红）：`begin_tick_erases_a_hop_that_the_new_tick_does_not_repeat`
  （红：`alpha 0 重播了最后一跳`）、`arena_begin_tick_folds_every_live_sprite`、
  `a_wait_node_draws_the_same_point_every_frame`（红：`Wait 第 1 拍画面上还在动：0.0/16.0/32.0`）、
  `standing_still_must_not_replay_the_last_hop`（红：`相机从没站稳 [170,171,172,…,191,191,192,190,…]`），
  并给 `the_camera_tracks_the_ideal_scroll_under_a_jittery_frame_rate` 补了**松手相**
  （红：`停手之后相机还在来回弹 [31,33,35,…,55,54,54,55,56,55,…]`；先倒回可滚动区间再松手 ——
  顶到地图边缘时怎么弹读数都不变，那是白测）。第 24 条那句"相机回退已穷举排除"据此修正：
  那个穷举只按住方向键不放（走位段每一拍都在推进 `prev_pos`，本来就测不到），**缺的正是停手相**。
  闸门重跑（原命令，退出码全 0）：`cargo xtask fmt --check` 净、`cargo xtask clippy` 两组 0 warning、
  `cargo xtask check-all` 四组 0 warning、`cargo test --workspace --all-features` **245 绿 0 红**
  （较上条 +5：上面四条新单测 + `the_camera_never_steps_backwards_in_any_direction`
  由临时扫描转正；`--example tiled_map` 15 → **17 绿**，`sisf-sprite --lib` 48 → **51 绿**）、
  `cargo xtask golden` 两条全绿。用户肉眼复验（2026-09-25）：**"解决了"** —— 桌面与浏览器两条链路
  都由用户自己看过，本次不再需要逐帧读数。
- 复验之后顺手收掉那条挂着的**帧节奏**决定：`cargo xtask web` 从 debug 改成 **`--release`**
  （`xtask/src/main.rs::web`，产物路径随之从 `target/wasm32-unknown-unknown/debug/examples` 换到
  `release/examples`）。理由与实测：同一个 tiled_map，debug 的 `web/pkg/tiled_map_bg.wasm`
  **32.5MB → 8.17MB**，而浏览器那条链路多半跑在软件 WebGL（`?wgl` ⇒ ANGLE Microsoft Basic Render
  Driver）上，未优化的产物本身就是节拍噪声的一大来源；增量重编 **19.89s**，日常可接受。
  诊断通道不受档位影响（panic 钩子在 `platform/wasm.rs::install_panic_hook` 里无条件安装）。
  新链路实跑（`cargo xtask web tiled_map` + `?ex=tiled_map&wgl`）：console 打出
  `main 已启动` → `adapter: ANGLE (Microsoft, Microsoft Basic Render Driver …, Gl)` →
  `物理 544x416 ⇒ 画面 (0,0) 544x416，倍率 1.00，光栅档 1` → `字体 Some("Inter")（face 1）`，
  两次抓屏 HUD 从 `tick 1847 a0.50 cell 12,12 cam 128,184` 走到 `tick 3604 a0.00`（⇒ 逻辑拍在跑，
  不是定格），同一页面自测 `requestAnimationFrame` 计数 = **72 次/秒**。
  ⚠️ 这条只证明"release 链路通、画面在动"，**没有**给出帧时长分布 —— 那要的是另一套探针，
  且 debug native 的 7.6% 长帧那条读数仍然只在 native 侧成立。文档同步在
  `docs/platforms.md`「Web」。
- 验收（整段）：anim/route/timing 单测 + tiled_map → 提交 `P7 anim+route+tick`。→ 落地说明见 `docs/sprite.md`「T8：固定拍上的动画与任务」（两条互斥动画通路的分工表、任务三约定、`dir()/step()` 是动画状态）+ `docs/platforms.md`（letterbox 只管投影不管裁剪、HUD 必须挂更高 viewport 层）+ `docs/text.md`（文字层号：`draw_text_at`）。闸门（都用原命令）：`cargo xtask fmt --check` 净、`cargo xtask clippy`（win `--all-targets -D warnings` + wasm32 web）0 warning、`cargo xtask check-all` 四组 0 warning、`cargo test --workspace --all-features` 全绿 0 红、`cargo xtask golden` 全绿（文字基线不受新参数影响）。

### T8 期间发现并修正的事实（供后续阶段引用）

1. **`BatchKey` 先按 `layer` 排、`z` 只在同批内有决定权 ⇒ 相机层之上的 HUD 没法靠大 `z` 盖住
   地图**。第一版 tiled_map 把 HUD 底板与三行文字挂在 layer 0、地图挂在相机层（layer 1），
   抓屏里 HUD 一个像素都不出现（不是画错，是整层被压在下面）。修法：`push_camera` 往栈上推
   **两层** —— layer 1 = `Viewport::at(-cam.x,-cam.y,vw,vh)`（世界），layer 2 =
   `Viewport::at(+cam.x,+cam.y,vw,vh)`（HUD，净投影正好回到 letterbox 里的虚拟画面，既不跟着滚
   又压得住地图）。单测 `the_hud_layer_nets_out_the_camera_and_outranks_the_world` 钉住两层的投影
   与"pop/push 成对 ⇒ 栈深恒 3"。
2. **letterbox 只保证投影，不保证裁剪**。`Viewport` 把画面放进边条内的矩形，但 scissor 仍然是整个
   surface ⇒ 相机层自己平移出去的那部分内容照原样画进黑边（800×600/1× 下地图西侧石墙正好落在
   128px 边条上，像素扫描看得见）。**谁推 viewport 层谁就得自己 `push_clip`**，这条写进
   `docs/platforms.md`，T10 的消息窗/小地图同样受约束。
3. **裁剪盒用的是那一层自己的坐标，相机层的"画面左上角"不是 `(0,0)`**。第 2 条的第一版修法是
   `push_clip(1, [0,0,vw,vh])` —— 仍然渗色，因为 layer 1 的 `(0,0)` 已经在画面左上角往回 `-cam`
   的位置上；正确的是 `[cam.x, cam.y, vw, vh]`。`push_viewport` 的 `rect.w/h` 只用于平移（不参与
   派生投影），所以"层尺寸"和"该层可见盒"是两件事，别混。
4. **文字要能落在指定 viewport 层**。`TextEngine::draw_text` 原先把 `layer` 写死 0（与 RGSS 的
   "画进当前目标"对齐），于是第 1 条那个 HUD 层根本挂不上去。新增 `draw_text_at(.., layer)` /
   `text_quads(.., layer)`，层号**同时**写进 `BatchKey::layer`（排序读它）与 `Instance::layer`
   （着色器读它），只改一个就是"字被投到 A 层、却按 B 层合批"的错位；
   `text_quads_carry_the_layer_into_key_and_instance` 钉住两者一致。
5. **`RpgMaker4x3` 是 4 行 × 3 列，不是 4 列**。清单字面的 `dir + 3*step` 是列优先读法；本仓库帧表
   按行优先装箱（与 `AnimDef::grid` 同一约定），故 `index = dir_row(dir)*cols + step%cols`，
   `cols` = 3（MV）或 2（VX/Ace）。示例里按 4 列写 `draw_body` 直接算越界（减法还 panic 过一次）。
6. **`Sprite::dir()` / `step()` 是动画状态，不是普通字段**：精灵没挂 `AnimDef` 时 `Turn` 节点与
   `set_dir` 都是 no-op（没有图样就不知道"朝上"取哪块 `src`）。断言 `Turn` 生效的单测必须先给一条
   `AnimDef::grid_pattern(..)` —— 第一版断完拿到 `None` 才看清这条。
7. **`raster_scale` 档位是字号乘数，NaN 不能靠 `clamp` 兜**：`f32::clamp` 原样返回 NaN，带着 NaN 去
   装箱等于毁掉整页覆盖率 ⇒ `FontAtlas::set_raster_scale` 单独把 NaN 打回 1 档，`±∞` 才让 `clamp`
   夹到端点档。
8. **viewport 层的投影在 resize 后自动重推，但 `push` 必须发生在 `frame_u` 之前**。`Runner` 的帧序是
   `tick* → frame.frame_u() → draw`，而 `FrameCtx::recompute` 只按存的 `rect` 重新派生 `1..depth`
   的投影 ⇒ 推过一次的层在 resize 后不用手动重建；反过来在 `draw` 里推层会晚于本帧的
   `frame_u`（下一帧才被读到）。⚠️ 本条原先还写着"相机层在 `tick` 里推一次就够"——那句是错的，
   正是第 15 条那个抖动的成因：跟着插值 sprite 走的相机必须在 `App::prepare` 里推。
9. **整数 2× 放大下 `Linear` 采样不等于像素翻倍**：物理像素中心落在纹素内部的 0.25/0.75 处，相邻纹素
   按 25/75 混合（实测同一逻辑像素的两列差 `rgb` 1~4）。像素风要真翻倍得 `Bitmap::filter = Nearest`
   那条双 sampler（`docs/sprite.md` 已记为 T10 决策），本阶段不动 —— 但别把"整数倍缩放"当成"自动
   像素翻倍"来宣传，它保证的是**格线同相**（不抖、不整体发糊）。
10. **`DrawStats` 没有 `Default`，且 `Renderer::stats()` 读的是上一次 `end_frame` 的账**：示例里存
    `Option<DrawStats>` + `map_or`；合成读数要到第 2 帧才打印得出第 1 帧的结果（`if self.frame == 2`）。
11. **Windows 无人值守取证的三条工具性事实**：HWND 用 `Get-Process | ? MainWindowTitle -like` 拿
    （`FindWindowW` 经 PowerShell `Add-Type` 走 ANSI marshalling，返回 0）；`SetWindowPos` 的 `0x1`
    是 `SWP_NOSIZE`（改尺寸要 `NOZORDER|NOACTIVATE = 0x14`）；`PostMessageW WM_KEYDOWN/WM_KEYUP` 配
    `MapVirtualKeyW` 的 scan code（方向键再加扩展位 `0x1000000`）能驱动 winit 输入 ⇒ 键盘链路可在
    没有人点击窗口的情况下取证。另外 Git Bash 的 `kill $PID` 杀不掉 Windows 进程，残留示例会锁住
    `target/.../<example>.exe` 报 `LNK1104`，要用 `Get-Process <name> | Stop-Process -Force`。
12. **wasm 上的 panic 到 JS 只剩 `RuntimeError: unreachable`，原文与位置全丢** ⇒ "页面黑着、
    console 一行不涨"这种现场根本没法查（T8e 第一次 wasm 手测就是这样）。`platform/wasm.rs::run`
    现在第一件事就是装 panic 钩子，把 payload + `file:line` 送进 console（`log_error`）。这条钩子
    是本次定位真因的唯一途径，写进 `docs/platforms.md` 的 Web 一节。
13. **一个 face 都没有时不能把排字请求交给 cosmic-text**：`shape.rs` 里那句
    `font_iter.next().expect("no default font found")` 直接 panic，而 wasm 上"自带字体还在 fetch
    路上"是每帧都会经过的常态（`FontSystem::new()` 在 wasm 扫不到系统字体）⇒ 第一次排字就把 rAF
    链打断。`layout_with` 先查 `db().is_empty()` 返回空 `TextRun`（行高照常给），"没字体 = 字形不
    出现"这条约定才真的成立。断言 `an_empty_font_system_lays_out_nothing_instead_of_trapping`。
14. **wasm 侧的键盘链路可以无人值守取证**：往 canvas 与 window 派发合成的
    `KeyboardEvent('keydown'/'keyup', {code:'ArrowUp'})`、中间 `setTimeout` 挂住不放，winit 照收
    ⇒ "按住 N 毫秒 ⇒ 逐格走 N 格"这类断言能在浏览器里复现，不必真人按键。
15. **相机推到 `tick` 末 = 每拍把角色相对地图拽回去一次（"人物一直在抖动"的真因）**：帧顺序是
    `tick ×N → frame_u → draw(alpha)`，`frame_u` 一旦把 viewport 栈烘进 `FrameU`，`draw` 里再
    `push_viewport` 就只对下一帧生效。第一版 tiled_map 因此把相机钉在拍尾（`hero_at = h.pos`），
    角色却画在 `interpolated(alpha)` 上 ⇒ 走起来错位 `(1 - alpha) × SPEED`（SPEED=2 时最多 2 个
    虚拟像素，且 alpha 每帧都变 ⇒ 一步之内来回跳；站着不动时 `prev_pos == pos`，反倒看不出来）。
    文件头当时还写过"差别肉眼不可分"——
    肉眼分得很清。修法：给 `App` 加一相 `prepare(alpha, ctx)`（tick 之后、烘焙之前，`alpha` 与本帧
    `draw` 同值），相机取 `Sprite::interpolated(alpha)`，于是跟随目标与投影同相位。断言
    `the_following_camera_shares_the_hero_s_interpolation_phase`：五档 alpha 下角色的屏幕 NDC
    完全相同，另配对照组量出旧写法在 alpha 0/0.5 时恰好错 2/1 虚拟像素。静态图块
    `prev_pos == pos`，插值对它们是恒等的 —— 这个坑只有跟着人走的相机暴露得出来。
    浏览器侧的量化复验（同一份探针跑两次，只换 `prepare` 取哪个相位）：按住 ArrowRight 700ms、
    逐 rAF 把 canvas `drawImage` 到离屏 2d 再 `getImageData`，取玩家上衣蓝（72,96,168）的最左列
    x —— 旧相位 37 帧在 `260,261,259,260,…` 之间锯齿（跨度 2px），修好后同一批帧恒为 `261`
    （跨度 0），而世界仍在滚。
16. **侧面行走图：头发盖后脑，且必须后画脸**（小问题，但一眼就是"左右反了"）。`draw_body` 的
    侧向两行原先是"眼睛画在朝向那侧 → 头发矩形再盖住它"，于是朝左的那格露出的脸在右边，四张
    图读起来左右互换。正确顺序是先用头发盖掉**后脑**那半边（朝左 ⇒ 盖 x∈[15,25)），再画眼睛。
    `DirPattern::dir_row` 的行序 `下/左/右/上` 本身没错，错的是自绘素材。断言
    `the_side_walk_frames_actually_look_left_and_right`（在生成的位图上按像素钉"眼睛在朝向那半、
    头发在后脑那半"，玩家与 NPC 两条带各一遍）；浏览器侧同一条探针读出：朝左那帧墨点在格内
    x=9/10、头发 72 像素在右半，朝右那帧墨点在 x=21、头发 50 像素在左半。
17. **"截图之外还能读像素"：canvas 每帧 `drawImage` + `getImageData` 能在浏览器里做定量手测**。
    wgpu 的 WebGL2 呈现缓冲在同一个 rAF 回调链里还活着（下一次呈现才清），所以在自己注册的
    rAF 里 `drawImage(canvas)` 到一张离屏 2d canvas，就能拿到本帧像素 ⇒ 抖动/相位/贴图朝向这类
    "只在动的过程中成立"的断言不必靠肉眼读截图。配合合成 `KeyboardEvent` 按住 N 毫秒（第 14
    条）就是完整的无人值守走位取证。注意：静止角色的位置要等 `keyup` 之后再采（走路时脚底格
    子还在换），而颜色匹配别用近似 —— 水色 (42,92,158) 与上衣蓝 (72,96,168) 只差 30/4/10。
18. **同相位只解决"谁相对谁晃"，还得保证那个点在像素格点上**（第二次报回的抖动："现在变成整个
    地图都在抖，左右移动时左右抖、上下移动时上下抖"）。第 15 条把相机改成跟随 `interpolated(alpha)`
    之后，晃的对象从角色换成了整张地图：图集采样器是 `FilterMode::Linear`
    （`crates/sisf-render/src/renderer.rs:286`），相机平移量一带小数 ⇒ 本来对齐物理像素的地图格点
    整体落到纹素中间被重采样，每根硬边缘摊成两个像素、摊法还随 `alpha` 每帧变。`Nearest` 也躲不掉
    （边缘不糊，但整图随小数部分在两个像素位置之间跳）。修法在示例侧：`lattice_alpha` 把插值折到
    1px 台阶（`(alpha × 每拍位移).round() / 每拍位移`，`SPEED = 2` ⇒ 一拍两步），角色与地图同时
    落在格点上、相对位置仍精确。真亚像素滚动要么换 `Nearest` 接受 1px 跳，要么时间抖动 ⇒ 与 T10 的
    `filter` 选择权一起定。钉成单测 `the_following_camera_snaps_the_world_to_the_pixel_lattice`
    （含对照组：不取整时 `alpha = 0.3` 就把相机推到格点外）。判据不是"看着晃不晃"，而是**相邻两帧
    能否用一个整数位移对齐** —— 量法与读数见 T8e 的证据块与 `docs/platforms.md`。
19. **canvas 像素探针会读到"已清空的缓冲"，且必须确认相机真的在滚**（第 17 条那两个前提没写就白测
    一轮）。WebGL canvas 未开 `preserveDrawingBuffer`，在应用自己的 rAF 之外 `drawImage` 有时拿到的是
    本帧、有时是清空后的 ⇒ 症状是"相邻两帧 18.9% 的像素变了、却任何整数位移都对不齐"，看着像亚像素
    重采样、其实是读回伪影。滤法：按 distinct 颜色数（清空帧 <8，真实帧 ≥40）丢掉再算。另一半：
    tiled_map 的相机在 `cam.x` 夹到 224（`WORLD_W 768 - 544`）之后就不再滚，角色改成在屏内走 ——
    于是"按住 ArrowRight 25 帧、像素只变 0.1%"，量的根本不是滚动。**滚动方向上的探针要从出生格
    （12,12 ⇒ cam 128）往能滚的那侧按**（ArrowLeft/ArrowUp/ArrowDown）。
20. **"人物不抖背景抖"的第三次报回不在映射上，在记账上：空转的一拍会让插值重播最后一步**。
    前两条（15、18）都对之后仍然晃，真因是 `MoveRoute` 每走完一格多烧一拍"下一趟才报告完成"
    的收尾拍（`crates/sisf-sprite/src/task.rs`）。那一拍 `prev_pos`/`pos` 原地不动，而
    `TickClock` 照样消耗一个周期把 `alpha` 归零 ⇒ `interpolated(alpha)` 把最后一步**重播**一遍
    （画面退回一步再走回来），每格一次；相机把角色钉在屏幕正中，于是错位的只有背景 ——
    正是"人物不抖背景抖"的形状。修法：节点在这一拍跑完就**当场** `finish`。不变量：**每个被
    `TickClock` 消耗的周期都必须推进 `prev_pos`/`pos`**。顺带的速度账：路线平均速度原本是
    `tile/(ceil(tile/speed)+1)`（`32/(16+1)` ⇒ 慢 9%），不是声明值。
    判据 = 把浏览器实测的乱节拍 `[18.2,12.1,18.2,24.3,…]` 离线复放进 `TickClock`，逐帧读
    `cam × scale`：从不回退、`|cam - 120·t|` ≤ 2 物理像素、总位移对得上 —— 修账前偏差 12px、
    1.486s 只滚 166px（理想 178px），修账后 2px/176px。钉成单测
    `the_camera_tracks_the_ideal_scroll_under_a_jittery_frame_rate`；浏览器侧同一动作的每格耗时
    从 ~283ms（3.53 格/s）回到 263/267/257ms（理想 266.7ms = 3.75 格/s）。
    残余的逐帧位移散布（1~3px）是软件 WebGL 下 54~56fps 的节拍噪声，不是位置错误（位置单调、
    误差 ≤2px）—— 这条要**定量登记**，别再当未修问题查。
21. **像素格点是"设备格"不是"虚拟格"，但这条在 1× 下是恒等的（别把它当第 20 条的解药）**。
    被重采样的是纹理，量的是 GPU 平移量 ⇒ `lattice_alpha` 的台阶应取在 `scale` 之后
    （`(alpha × 每拍位移 × scale).round() / (每拍位移 × scale)`），相机再过一遍 `snap_to_device`。
    2× 时一拍走 4 虚拟像素 = 8 设备像素，插值有 5 个设备格点可站（0/2/4/6/8），比只在虚拟格取整
    的 3 个（0/4/8）更细；1× 下两者完全相同，所以它是**真实改进但不是那次抖动的修法**。
    钉成单测 `the_camera_lattice_is_the_device_pixel_grid_not_the_virtual_one`（2×，含虚拟格对照组）。
    另一半：`ScaleMode::IntegerFit` 下 `scale` 恒为整数 ⇒ 曾经担心的"1.5× 分数标度"这条路根本走不到，
    要复现分数标度得换 `Fit`/`Fill`。
22. **合成按键只能派发到 canvas，且被遮挡页面的 canvas 读回会"冻结"**（修正第 14、17 条）。
    winit 0.30.13 的 `keydown` 绑在 canvas 元素上（`platform_impl/web/web_sys/canvas.rs:301`），
    往 `window` 派发那一版是空转的（探针读数恒为 0）。另一半更要命：页面没在合成（嵌入式标签页
    被遮挡）时，`drawImage` + `getImageData` 返回的是**同一份冻结缓冲**而不是空缓冲 —— 于是"每帧
    像素 100% 相同"看着像"画面根本没动"，而应用自己的带时间戳日志证明它在动。像素探针的前提是
    页面真的在前台合成；不能确认时改用应用侧的逐帧计数/时间戳日志（离线复放同样能出第 20 条的
    判据）。附：逐帧 `console.log` 本身会扰动节拍（Heisenbug），聚合到 96 帧一批再打。
23. **"刷新后没有变化"先怀疑产物没到货，而不是修复没生效**：`xtask` 的静态服务原先不发任何缓存头
    （也没 `ETag`/`Last-Modified`）⇒ 浏览器对那个 32MB 的 `pkg/*_bg.wasm` 做**启发式缓存**，普通刷新
    （F5）直接用磁盘上的旧副本，旧代码的"收尾拍重播最后一步"（= 每格往反方向弹一下）当然还在。
    修法 = 响应加 `Cache-Control: no-store`（开发服务宁可每次读盘），复验一律先看 console 里
    `物理 …倍率 …` 那行是不是新的。同一次查清的第二件事：**winit 0.30.13 的 `set_canvas_size` 只写
    canvas 的 CSS style，不改 `width`/`height` 属性**
    （`platform_impl/web/web_sys/mod.rs:71-85`），而 `web/index.html` 把属性钉成 544×416 ⇒
    **网页上 `IntegerFit` 只能算出倍率 1.00**（不管 DPR 是多少），2× 的设备格点那条路只有桌面窗口
    拉大才会走到。所以"网页抖"绝不能拿 2× 单测当解释。
24. ~~**"相机回退"已经穷举排除，别再往这个方向查**~~ —— **这条被第四次报回推翻，教训比结论有用**：
    穷举的形状是 2 个倍率（1×/2×）× 4 个方向 × {60.00/59.94/120/144Hz, 实测乱拍} 跑 200 帧、
    读 `cam × scale` 的逐帧差分，**回退次数恒为 0**（分布里那些 `0` 是相机夹边
    `cam ∈ [0,224]`，不是停顿）。问题不在测量，在**取样区间**：整段都按着方向键，而走位期间每一拍
    都在推进 `prev_pos` ⇒ 重播最后一跳这个 bug 在那段数据里根本不存在。停手之后才有停拍，
    而停手之后的读数没人看过一眼。判据转正成单测
    `the_camera_never_steps_backwards_in_any_direction`（4 方向 × {1×,2×} × 6 节拍，180 帧，
    含尖峰与奇偶配对节拍），并且**必须配一条停手相的**（`standing_still_must_not_replay_the_last_hop`）：
    单调性判据在"一直按着键"时是绿的，对静止段的来回弹完全失明。
25. **`prev_pos` 的不变量只能由引擎兜，不能由每个逻辑兜**（第四次报回的真因，也是第 20 条的另一半）。
    "每个被 `TickClock` 消耗的周期都必须推进 `prev_pos`/`pos`"这条写在第 20 条里的不变量，
    修法是把最后一步的收尾拍在 `MoveRoute::finish` 里补掉 —— 但消费者不止路线：停手后的玩家、
    `Wait` 里的 NPC、撞墙被作废的剩余格数，全都是"这一拍没有 `set_pos`"的拍，每一个都得自己推进
    `prev_pos`，漏一个就弹一个。所以改成 `App::tick` 第一行 `arena.begin_tick()`（全场
    `prev_pos = pos`），把"没动"变成一种由引擎保证的常态。**顺序是这条的一半**：`tick_anims`
    之类在拍尾的收拢会把本拍位移当场抹掉，所以只有拍首一个位置是对的。
26. **wasm32-unknown-unknown 上没有 `std::time::Instant`，示例侧的帧探针不能自己计时**：
    std 在该 target 无时钟，`Instant::now()` 直接 panic（`sys/time/unsupported.rs`），而引擎的
    `platform::now_secs()`（wasm 走 `performance.now()`）是 `pub(crate)`，示例拿不到。
    要量帧时长就换**不需要时钟的形状**：固定步长下"这一帧补了几拍"就是帧长（`拍数 × 周期`），
    逐帧只数 `App::tick` 的调用次数。⚠️ 两条读数陷阱：`MAX_CATCHUP=5` 会截断尖峰（读到的是下界）、
    `TickClock::reset()` 那一拍记 0；而 ticks/帧恒为 0 说明这段时间根本没在出帧
    （页面被遮挡/挂起），拿它算出来的"很干净"是假的。
27. **"抖动"报回要按"谁在抖 + 什么时候抖"两轴收窄，而不是按代码模块**：前三次分别落在
    相机相位（走位期间）、像素格点（走位期间满屏）、周期记账（每格一次）—— 共同点是**都在走位期间**，
    于是排查也一路锁在走位段。第四次"人物不动也抖、停手才抖"才把取样区间换掉。同一条判据
    （逐帧位移是否回退）在两个取样区间下给出相反的答案，而只有后一个是用户看到的那个。

## T9 引擎事件总线 + 输入映射
- [x] `sisf-event/src/bus.rs`：`EngineEvent{Action,Key{code,kind,ch},Pointer{kind,pos,button,delta},Lifecycle,Custom(u32,serde_json::Value)}` + `EngineBus{push,drain}`。验收：drain 顺序单测。→ `bus::tests` **4 项**：`drain_preserves_push_order_and_empties_the_bus`（六条混类事件按入队序原样出来 + 真的腾空）、`pushes_after_a_drain_start_a_fresh_batch`、`duplicates_are_kept_not_collapsed`（去重是 `ActionMap` 的活不是队列的活）、`peek_does_not_empty_the_queue`。`Custom` 载荷定 `serde_json::Value`（跨 wasm 边界就是 JS 侧那个对象，两边不各抄一套编解码）。总线刻意做成"没有监听器图"的一层：只保证顺序 = 发生顺序。
- [x] `sisf-app` 归一化：winit `WindowEvent/DeviceEvent` → `EngineEvent` 只做一次；wasm/Android 同一路径；`post_custom` 通道留给 JS 与移植层。验收：三端 check 绿。→ 新 `crates/sisf-app/src/normalize.rs`（全纯函数，四端可单测）**8 项**：`keyboard_edges_survive_the_mapping`（物理键名 + 按下/连发/松开三态）、`the_layout_char_rides_along_with_the_physical_code`（`code` 布局无关、`ch` 布局相关，不二选一）、`pointer_positions_are_virtual_pixels`（首帧 delta=0 + 位移除掉缩放）、`wheel_delta_collapses_line_and_pixel_units`（`LineDelta` 与 `PixelDelta` 折成同一个数）、`mouse_buttons_are_pointer_edges_the_action_map_can_bind`、`extra_mouse_buttons_keep_their_number`（侧键不挤成"无"）、`touch_phases_become_pointer_edges`。三端同源：三端共用同一个 `Runner`，设备事件一律 `Runner::feed → InputState.feed → deliver`，**到达即翻译**（不等 `RedrawRequested`，否则按住方向键的手感随渲染帧率抖）。`post_custom`（`custom.rs`，**3 项**：`posts_drain_in_arrival_order`、`a_second_post_round_starts_a_fresh_batch`、`the_global_channel_round_trips`）走全局 `Mutex<CustomInbox>`，`Runner::deliver` 每次派发前取空追加到总线尾部 —— 本阶段只给通道与顺序，不订阅/不路由/不去重。check 证据见整段验收。
- [x] `input.rs`：`ActionMap`（命名动作 → 键/手柄按钮/指针，one-shot 与 held 语义），可从 RON 载入（RPG Maker 决定/取消/Shift/PageUp 键位）。验收：pressed/held/released 边沿单测。→ `input::tests` **19 项**，边沿三态点名的是 `held_action_reports_press_then_one_hold_per_tick_then_release`；另钉 `two_triggers_on_one_action_need_both_releases`（多源位掩码：两个键绑一个动作要两个都松开）、`one_shot_action_never_reports_hold` + `take_press_consumes_the_edge_once`、`autofire_repeats_press_on_the_tick_grid` + `autofire_stops_the_moment_the_key_is_released`（连发只数拍子不数帧）、`a_repeated_key_is_not_a_new_press`（操作系统的连发不是新按下）、`unbound_input_still_reaches_the_bus`（没绑的动作也照样递裸事件）、`pad_and_keyboard_triggers_converge_on_one_action` + `stick_codes_are_just_another_trigger`、`pointer_buttons_can_trigger_an_action`、`forget_held_reports_a_release_for_every_held_action`（+ 松开不吞掉未消费的 `Press`）+ `release_where_only_touches_the_matching_device`（拔柄牵连不到键盘）+ `set_map_drops_held_state_without_emitting_anything`、`ron_roundtrip_rebuilds_the_same_map` / `a_loaded_map_actually_resolves_its_names` / `bad_ron_is_an_error_not_a_panic`、`rpg_maker_defaults_cover_the_named_buttons`（决定/取消/Shift/PageUp/PageDown/菜单 + 四向）、`an_empty_map_reports_nothing_at_all`。默认表 = `ActionMap::rpg_maker()`，挂在 `AppConfig::actions` → `FrameCtx::input`，故 `App::tick(ctx)`/`App::on_event(.., ctx)` 都能直接问 `ctx.input.is_down("left")`，游戏侧不必自己存按住状态。
- [x] `gilrs 0.11.2` 置于 `native` feature（wasm 无手柄，文档标注），含死区与连发。验收：feature 组合 check。→ 新 `crates/sisf-app/src/gamepad.rs`（`StickEdges` 按 0.75/松 0.65 迟滞，与 gilrs 折算十字键同阈值）+ `normalize::pad`（码词汇表：按钮取 gilrs 变体名、摇杆取 `"<轴名><符号>"`，统一翻成 `EngineEvent::Key` ⇒ `Trigger::Pad` 与 `Trigger::Key` 走同一条匹配路径）。单测 **4 项**（无手柄也能跑，因为测的是边沿机器不是设备）：`a_stick_edge_needs_the_press_threshold_and_the_release_band_clears_it`（死区 + 迟滞带不来回跳）、`crossing_to_the_other_side_releases_first_then_presses`、`unplugging_forgets_every_held_stick_direction`（拔柄不留"永远按着"）、`stick_codes_resolve_through_the_same_action_map_as_the_keyboard`（摇杆码落进 `ActionMap`，顺带钉住"−1.0 才是左"的符号约定）。连发在动作层（`Autofire`），gilrs 的 `ButtonRepeated` 忽略。feature 边：`native = ["dep:pollster", …, "gilrs"]` + `gilrs = ["dep:gilrs"]`；`cfg` 叠 `not(wasm32)+not(android)` 两层。平台矩阵 + 理由写在 `docs/input.md`（新增）与模块文档：wasm 要 Gamepad API 得 JS 侧每帧轮询 `navigator.getGamepads()`（本阶段不接），Android 上 gilrs 0.11 无 `android-activity` 后端（依赖连声明都不给）—— 两端 `Trigger::Pad` 匹配不上但不报错，键位表照常。
- [x] 验收（整段）：bus/input 四端单测 → 提交 `P8a engine bus`。→ `cargo test --workspace --all-features` **全绿 0 warning**（`sisf-event` 23 = bus 4 + input 19；`sisf-app` 30 = normalize 8 + gamepad 4 + custom 3 + frame 9 + timing 6，另有 lib doctest 1；`sisf --example tiled_map` 18）。"四端单测"成立的形式：`bus.rs`/`input.rs`/`normalize.rs` 的测试全是纯逻辑（不碰设备、不碰 GPU、不依赖墙钟），而 `cargo xtask check-all` 的四组都以 `--all-targets` 编译测试 ⇒ wasm32 `web`、wasm32 `web,ogg,mp3,wav,midi`、aarch64-linux-android `native` 三腿上这些测试都过了类型检查。闸门（全部原命令实跑）：`cargo xtask fmt --check` 净、`cargo xtask clippy`（宿主 + wasm32，`-D warnings`）净、`cargo xtask check-all` 四组净、`cargo test --workspace --all-features` 全绿、`cargo xtask golden` 两条基线绿。桌面运行时：`./target/release/examples/tiled_map.exe` 起来到第 1 帧无 panic（`adapter: NVIDIA GeForce RTX 2080 Ti (DiscreteGpu, Vulkan)`、`物理 800x600 ⇒ 画面 (128,92) 544x416，倍率 1.00`、`第 1 帧合成: batches 4 quads 732 sprites 637 pages 1 tier 1x`）。
  - ⚠️ **未做的一半：改完输入通路后的"按键真的能走"运行时复验没拿到**。示例的 `on_event` 这次从"读裸键名"换成"只读命名动作"，`Runner` 那侧 `feed/deliver/每拍 input.tick` 是新接线，而这两处**都不在单测覆盖内**（示例单测是手工把 `EngineEvent::Key` 喂进 `InputState` 再调 `on_event`，绕过了 `Runner`）。本轮里浏览器导航与 Windows 按键注入两次被工具权限挡下（`.trash/cap.ps1` + `?ex=tiled_map&wgl` 两条路都没跑成），所以只有"起窗口到第 1 帧"这一条弱证据。待用户肉眼复验清单：桌面 `cargo run --release -p sisf --features image --example tiled_map` 按方向键/WASD 逐格走 + `Enter` 开箱 + `Esc` 清键 + 失焦（Alt-Tab）后角色不当场磨墙；`cargo xtask web tiled_map` 后 `?ex=tiled_map&wgl` 同一份；有手柄则验十字键/左摇杆同向、拔柄当拍停。

### T9 期间发现并修正的事实
1. **`dep:x` 形式的可选依赖不会生成同名 feature**：`native = ["dep:pollster", "dep:gilrs"]` 之后 `cfg!(feature = "gilrs")` 恒为假 —— 手柄代码整块蒸发而三端 check 全绿（这是"绿着骗过去"的那种错）。必须显式写 `gilrs = ["dep:gilrs"]`；而 Cargo feature 是**跨 target** 的，所以代码里的 `cfg` 还要再叠 `not(target_arch = "wasm32") + not(target_os = "android")`，只判 feature 会让 wasm/android 腿挂在一条链上根本不存在的依赖上。
2. **winit 0.30 的 `KeyEvent` 字段是私有的（`platform_specific`），测试构造不出来** ⇒ "按下/连发/松开"的翻译规则想被单测就得从事件里拆出来（`key_kind(state, repeat)` 纯函数，`key_event` 只是它的调用者）。连带两条：`KeyEvent.text` 是 `Option<SmolStr>`（用 `as_deref()`，`as_ref().and_then` 会撞上 move 报错）；`PhysicalKey::Unidentified` 包的是**平台相关**的 `NativeKey`（Windows 是枚举、wasm 是 `String`），所以测试里连一个"未识别键"都没法跨端构造 —— 那一条分支只能靠读代码保证。
3. **gilrs 0.11.2 的 `EventType` 是元组变体，而且 `Axis` 里没有 `LeftTrigger`**：`ButtonPressed(Button, Code)` / `ButtonChanged(Button, f32, Code)` / `AxisChanged(Axis, f32, Code)`（文档站点的渲染容易看成结构体，`registry/…/gilrs-0.11.2/src/ev/mod.rs` 才是准的）。扳机是 **`Button` 的模拟量**（0.0..=1.0，故过 0.5 才算按下），`Axis` 剩下的 `LeftZ/RightZ/DPadX/DPadY` 都不是摇杆 ⇒ 摇杆方向码只认四根 `*Stick*` 轴，否则十字键会被摇杆死区吃掉。
4. **摇杆轴的正方向在屏幕坐标里是"下/右"**（`-1.0` 才是 `left`）。符号约定一旦定反，用手柄走路朝镜像方向，而纯键盘的测试永远看不出来 —— 所以方向码测试必须直接断言 `LeftStickX- ⇒ is_down("left")`。
5. **摇杆死区的阈值必须与 gilrs 折算十字键用的是同一对数字**（按 0.75 / 松 0.65）。两套码来自同一根轴，阈值不一致就留下"十字键已松开、摇杆码还按着"的半格重叠死区，而动作层的位掩码要求**所有源都松开**才算松开 ⇒ 表现为"松了摇杆角色还在往右走"。换向时还得先松旧再按新（位掩码要的就是这个顺序）。
6. **松开不许吞掉还没被消费的 `Press`**：失焦/挂起时 `forget_held` 补发 `Release` 是对的，但如果连 `press_pending` 一起清，窗口边缘的一次快速点按（按下→立刻失焦）就会让"决定"在丢焦点那一刻凭空消失。
7. **`EngineBus::drain` 返回 `Vec` 而不是迭代器**：派发点要在持有事件的同时可变借用同一结构体的另一个字段（`app.on_event(&events, &mut s.frame)`），`&mut self` 的迭代器借用会一路活着把这次调用判成双借。
8. **`RefCell` 的"提前放掉借用"要 drop 拥有者本身**：`let s = &mut borrow; drop(s);` 是 `dropping_references`（no-op），`RefMut` 的再借用放不掉底层计数 —— 三端 `Runner` 里原先那几处 `drop(s)` 全是自欺，真正的释放点是作用域结束。
9. **归一化的时机与位置都得唯一**：旧 `Runner` 把事件塞进 `events` 等到"下一次挂起迁移/帧首"才派发（`CursorMoved` 这类高频事件还在中间堆积），改成"到达即 `InputState.feed` + 立刻 `deliver`"之后，held/连发仍按拍走（`input.tick` 在每拍开头），两者不冲突 —— 前者管手感延迟，后者管逻辑不随帧率漂。
10. 按 DoD-3 登记两条清单外新增（见下方"钉版增补"）：`serde_json 1.0.151`、`gilrs 0.11.2`。

## T10 游戏事件解释器 Command + EventRunner
- [x] `sisf-event/src/command.rs`：`Command` MVP 集（`ShowText{speaker,body,choices}`、`InputKey{var,timeout}`、`Wait`、`MoveRoute`、`Animate`、`PlayBgm/Bgs/Se/Me`、`Fade{target,frames}`、`SetVar`、`If{cond,then,else_}`、`Loop`、`BreakLoop`、`Jump{page}`、`Erase`、`Custom(String,Vec<Param>)`）+ serde + RON。验收：`.ron` 往返一致。→ 十七发全在（`Play*` 四条各一），外加 `Slot`/`SetOp`/`Expr`/`Cond`/`Sound`/`SoundChannel`/`FadeTarget`/`RouteStep`/`RouteSpec`/`AnimEffect`/`Param`，以及"嵌套 RON → 扁平 IR"的编译（`Instr{Run,Branch,Goto,LoopBack}` + `Program{code}`，`If` 出 `Branch+Goto`、`Loop` 出回边、`BreakLoop` 出 `Goto{end}`）。与清单字面量的四处差异（`ShowText` 多 `result`/`cursor_se`、`InputKey.var` 落成 `Slot`、`Wait` 单位是拍、`Fade` 多 `out`）写在模块文档里并登记在下方事实第 1 条。验收 → 单测 **7 项**：`ron_roundtrips_every_variant`（每条指令写盘再读回、逐字节相等）、`hand_written_ron_parses`（人手写的 `examples/assets/events/*.ron` 那三种风格也解得进 —— 序列化器写得出 ≠ 人写得进，这两条分开钉）、`compile_flattens_control_flow`、`nesting_is_capped`（`MAX_NESTING` 在编译期把关，运行期没有递归可以爆）、`break_outside_loop_is_an_error`、`empty_pages_compile_empty`、`params_roundtrip_through_json`（`Param ↔ serde_json::Value` 双向，非有限浮点降级为 `Null`）。
- [x] `vars.rs`：`VarStore`（switch/int/var）可序列化存档；`Cond`/`Expr`（比较、开关、变量算术）。验收：嵌套 `If` 与算术单测。→ 三态槽 + `numeric/textual/set_numeric/apply` 按槽型分派；`Expr` 九变体（含 `Rand{lo,hi}`）、`Cond` 六变体（`Switch/Cmp/TextEq/Not/All/Any`）。随机数在 `Rng`（splitmix64）里，**不读墙钟**，`VarStore::seeded(seed)` 之后整段事件可复现 —— golden 与存档回放都靠这条。验收 → 单测 **5 项**：`slots_default_and_ops_apply`、`arithmetic_is_total`（整除向零取整、**除零结果是 0**，不是 panic 也不是"保持上一个值"）、`nested_conditions_compose`（三层 `All/Any/Not` 套 `Cmp`/`Switch`）、`rng_is_deterministic_and_bounded`（同种子同序列 + 闭区间）、`save_roundtrips_and_is_stable`（`to_ron`/`from_ron` 往返 + 同一状态两次序列化字节相同 —— 存档 diff 才有意义）。
- [x] `runner.rs`：`EventRunner::tick(&mut EventCtx,budget)->RunnerStatus`，每页显式 `pc` + 调用栈（不引协程）；`Wait` 挂起；choices 消费 `EngineEvent`；`MoveRoute/Animate` 交调度器且 `wait:true` 阻塞。验收：事件 VM 单测（指令序列 → 副作用断言）+ 超预算保护。→ 一拍的顺序是 `(a) 后端 → (b) tasks.tick → (c) 解挂 → (d) 执行到挂起/栈空/预算用完 → (e) 同步 UI`：(b) 在 (c) 前 ⇒ "跑完最后一格"的路线同一拍就被观察到，`wait:true` 不白等一拍；(e) 放最后 ⇒ 游戏侧永远在本拍状态之上画。挂起把 `pc` **留在下一条**，解挂后不重跑（重跑一个 `ShowText` 等于把玩家刚按下的确认吃掉）。调用栈 `Vec<Frame>` + `MAX_FRAMES = 32`。验收 → 单测 **25 项**，指令序列 → 副作用点名：`sequence_writes_vars_through_both_branches`、`wait_holds_exactly_its_frames`、`a_blocking_route_walks_the_grid_one_cell_per_beat`（逐拍一格）、`a_wall_voids_the_rest_of_a_step_but_not_the_event`、`through_ignores_the_passability_table`、`animate_fades_and_blocks_until_done`、`a_flash_leaves_the_tone_it_found`（对照组：闪完恢复成**闪之前**那份 tone，不是归零）、`jumping_a_page_restarts_the_event_with_its_new_page`、`erase_removes_the_actor_and_ends_its_event`、`cancel_is_opt_in_and_reports_minus_one`、`fading_all_touches_four_channels_only`、`playing_audio_reaches_the_right_channel_and_failure_only_warns`、`an_unknown_actor_warns_once_and_moves_on`、`stopping_resets_the_window_and_the_darkness`；超预算保护 = `a_runaway_loop_cannot_hog_a_beat`（没有 `Wait` 的 `Loop` 最多吃掉 `budget` 条、下一拍从同一个 `pc` 接着跑）。
- [x] `EventCtx{ sprites, tasks, audio, vars, bus, vfs, ui }`（接 T6 消息窗）。验收：ShowText 驱动窗体 + 选项高亮 + SE。→ 两个端口 trait 而不是直接持有实现：`AudioPort`（`AudioBus` 只在开了 pump feature 时才存在，写进 `EventCtx` 就等于让"无音频的构建"变成编译错误，而那是 `check-all` 的一整组）、`UiPort`（契约：**UI 状态是事件机状态的函数**，只在内容变化时 `show/hide/set_dim`）。`Prompt{speaker,body,choices,active,allow_cancel}` 带 `prev_choice/next_choice/choice_value`，游戏侧不必自己数光标。验收 → `show_text_drives_the_window_and_reports_the_choice`（窗体内容 + 上下移光标 + `ok` 写回序号 + 移动时那发 `cursor_se` 落到 SE 通道）＋ `examples/dialog_demo.rs` 的浏览器读数（高亮行 `229,209,103`、窗高按 `choices.len()` 定，见下方整段验收）。
- [x] `script.rs`：`trait ScriptBackend{ start(entry), tick(&mut EventCtx) }` + `NullScriptBackend`；文档写明 QuickJS 只需实现该接口并用 `Command::Custom`/`EngineEvent::Custom` 编解码。验收：trait 对象可持有，默认路径零开销。→ 三个方法（多一个 `command(name,args) -> Option<Arc<Program>>`：`None` 只能表示"这个名字我不认"，"处理完了但没有指令要跑"必须交 `Program::default()` —— 告警通道在事件机上，后端只拿到 `name`/`args`，它没有别的地方可说话）。编解码只有两条通道：引擎→脚本 `Command::Custom`（`Param` 与 `Value` 一一对应），脚本→引擎 `EngineEvent::Custom`（后端在 `tick` 里取走自己那几条、**其余原序放回**，按键序列不能被脚本吃掉；wasm 侧宿主投进来走 `sisf::app::post_custom`）。脚本能做的每件事最后都落成"启动一段指令序列" ⇒ 存档只要 `VarStore` + 页号，不必冻结一颗 JS 堆。验收 → `null_backend_is_a_trait_object`（`Box<dyn ScriptBackend>` 装得进虚拟机 = 接口是对象安全的）+ `backend_hooks_fire`（三钩子都 fire）+ `recursive_custom_is_capped`（后端回 `Custom` 套 `Custom` 到 `MAX_FRAMES` 就停）+ `custom_without_a_backend_is_explained`。"默认路径零开销"的形状：`EventRunner::new()` 根本不装后端（连调用都不发生），装 `NullScriptBackend` 才是每拍多一次空函数调用 —— 两条路径都留着，让"我要不要脚本"在**类型**上看得出来。
- [x] `examples/dialog_demo.rs`：`assets/events/*.ron` → examine 触发对话、分支、选项写入变量、BGM/SE、Fade。验收：桌面 + wasm 全流程手测。→ 三份表（`well`/`chest`/`npc`）+ 一个能看的场景：方向键走位（走一格 = 转一次身，纯色方块没有朝向状态）、examine 面朝才触发、`page_for` 按开关换页、`MoveRoute`/`Animate`/`Fade(Screen)`/四条音频通道/`InputKey`/`Loop`/`Erase` 全用上了。HUD 三行刻意全 ASCII（`hold key 0 · none` / `actors 3 · ui pushes 6 · BGS … 0.0 / 750ms · warn 0` / `sw 1/2 = 0/0 · answer=1`）—— 这一行最需要出现的那一刻是 CJK 字库还没到货、或者根本取不到的那一刻（见「T6 期间发现并修正的事实」第 11 条），那时中文正是方块，而"事件机现在挂在什么上面"是 wasm 手测唯一要读的东西。示例自带单测 **9 项**（`cargo test -p sisf --example dialog_demo --features image,ogg,mp3,wav`）：`the_event_tables_on_disk_parse_and_compile`（三份 `.ron` 真从磁盘解出来并编译）、`the_window_holds_every_choice_list_on_disk`（拿磁盘上最长的选项表钉窗高）、`the_object_table_blocks_exactly_what_it_draws`（画出来的就是挡得住的）、`one_walk_beat_is_exactly_one_move_instruction`、`examine_reads_the_table_for_props_and_the_ground_for_the_npc`，加三条事件形状（`the_well_asks_a_two_choice_question_and_leaves_a_script_hook` / `the_chest_jumps_to_its_opened_page` / `the_npc_walks_fades_and_erases_himself`）。手测读数见下一条。
- [x] 验收（整段）：事件 VM 单测 + dialog_demo → 提交 `P8b event interpreter`。→ 单测：`sisf-event` **61 项**（T9 的 bus 4 + input 19 之外，T10 新增 command 7 + vars 5 + runner 25 + script 1）、`sisf --example dialog_demo` **9 项**，全绿 0 warning。**桌面**：`./target/debug/examples/dialog_demo.exe` 起来无 panic（`事件 well ← events/well.ron（页 1，指令 10 条）`、`chest（页 2，12 条）`、`npc（页 1，17 条）`、`adapter: NVIDIA GeForce RTX 2080 Ti (DiscreteGpu, Vulkan)`、`字体 Inter（cjk system，face 266）`、`cpal: 无输出设备（静音混音，时钟照常）` —— 这台机器没有输出设备，音频层按"静音混音、时钟照常"降级）。⚠️ 桌面这一腿只有"起窗口 + 无 panic + 三份表解出来"这一条弱证据，一整条 NPC 链没有肉眼复验过（本轮没有可注入的窗口焦点），要补的清单与下面 wasm 那份一一对应。**浏览器**（`cargo xtask web dialog_demo` → `?ex=dialog_demo&wgl`，逐帧像素探针 + 合成按键）：井边人整条链量到 —— `BGM`/`BGS` 起播（HUD `BGS ← tone_2s.ogg … gain 0.25`）→ `MoveRoute(wait:true)` 把他从 `(6,7)` 走到 `(6,6)` 再回来、这期间玩家按键推动不了世界 → `hold text` → Enter → **`hold key` 且持续 1300+ 拍不自行放行** → `Digit3` 被 `Loop` 拦回（`ui pushes 3` = 多一句"太小了"）→ 再 Enter 回到 `hold key` → `Digit7` 放行 → **`answer=1`**（`BreakLoop` + `SetVar`）→ `PlayMe` + `Fade(Bgm/Bgs,45)` → 2.5s 后 `idle 0`、**`actors 3`**（`Erase` 生效，橙色精灵不见）、`BGS … 0.0 / 750ms`（45 拍 = 750ms）、全程 `warn 0`。宝箱：`Flash` 白闪后精确回到 `206,212,229` → `Fade(Screen)` 压到黑再淡回 → `Switch(1)` + `Jump{page:1}` 换页（再触发说的是"空宝箱"那句）→ `Animate{Fade{to:0.3}}` 落在 `122,127,136`（0.3 在 sRGB↔linear 混合下的理论值，⚠️ 探针读的是混合后的显示值）。**补账（wasm 中文，两个示例各一遍）**：`?ex=dialog_demo&wgl` 控制台 `CJK 到货 8331336 字节` → `字体 Noto Sans SC+Inter（cjk ok，face 2）`，HUD「静默」与 NPC 窗「井边人 / 报个数我才让开。五以上。」有笔画、拉丁仍是 Inter；把 `CJK_URL` 临时换成一个 404 再跑（改回，不入库）→ `取 … 失败: HTTP 404` → `字体 Inter（cjk MISSING，face 1）`，中文方块、状态行照常、tick 继续走。`?ex=windows&wgl` → `自带字体: Some("Inter")（ok，face 2）`、状态行 `cjk ok`、窗体「艾达 · Ada」「风从右边吹过来。」有笔画。闸门（全部原命令实跑）：`cargo xtask fmt --check` 净、`cargo xtask clippy`（宿主 + wasm32，`-D warnings`）净、`cargo xtask check-all` 四组净（native 宿主 / wasm32 / android / wasm32 音频全开）、`cargo test --workspace --all-features` 全绿（**331 项通过、0 失败、0 warning**）、`cargo xtask golden` 两条基线绿。

### T10 期间发现并修正的事实
1. **清单字面量与实物的四处差**（都记在 `command.rs` 模块文档）：`ShowText` 多 `result`/`cursor_se`（选项没有返回值就没法分支，选项音是 RPG Maker 默认行为，缺省即静音而不是塞个假默认）；`InputKey.var` 落成 `Slot`（三态存储的统称）；`Wait` 单位是**拍**（虚拟机只在 tick 边界推进，拍是这里唯一的时钟）；`Fade` 多 `out`（`target` 只说淡什么、没说往哪边淡，而渐入渐出成对出现）。
2. **RON 里能省的字段是挂了 `#[serde(default)]` 的字段**：`choices`/`result`/`allow_cancel`/`wait` 可以整条不写，`speaker`/`cursor_se` 不行 —— 所以每发 `ShowText` 都得显式写 `cursor_se: None`。`else` 是 Rust 关键字，磁盘上叫 `else`、代码里是 `else_`（`#[serde(rename = "else")]`）。
3. **`RunnerStatus::Suspended.reason` 是 ASCII 令牌**（`text`/`key`/`wait`/`task`/`fade`）而不是一句中文：它是给游戏侧 `match`、给调试面板当键用的标识，而且浏览器里只有仓库自带那份拉丁字形（`docs/text.md`）—— 中文的挂起原因恰好会在最需要它的 wasm 手测里读成一排方块。示例的 HUD 状态行同理。
4. **预算数的是每一条指令，包括 `LoopBack` 回边**。只数"有副作用的指令"就拦不住任何东西 —— 死循环的形状恰恰是"没有副作用地在指令之间跳"。回边计入之后，`a_runaway_loop_cannot_hog_a_beat` 才成立，而它在游戏侧的可见结果是"这一拍慢了"而不是"窗口冻住"。
5. **`Fade{target: Screen}` 永远不该经过 `AudioPort`**：`FadeTarget::channels()` 对 `Screen` 返回空切片。把 `Screen`/`All` 塞进 `SoundChannel` 就等于给端口留一条"收到起播请求但没有通路"的死分支 —— `fading_all_touches_four_channels_only` 钉的是 `All` 恰好四条、不多不少。
6. **`ScriptBackend::command` 返回 `None` 只能表示"不认这个名字"**。"处理完了但没有指令要跑"必须交 `Program::default()`，否则后端永远无法安静地什么都不做，而告警通道在事件机上（后端只拿到 `name`/`args`，没有别的地方可说话）—— 所以"为什么这条没落地"这句话只能由虚拟机说。
7. **`RouteStep::Turn` 烧一拍**（不位移但占拍）。不烧的话"转身→走"在路线里会被压成同一拍，玩家看到的是角色瞬间朝左已经在往左走 —— 转身动画整段消失。
8. **调度器归事件机推进，游戏侧只负责每拍开头 `SpriteArena::begin_tick`**。两处都 `tasks.tick` = 路线跑得比逻辑快两倍，而且没有任何报错信息。这是 T8 那条"`prev_pos` 不变量"在 T10 的续集：谁拥有时钟，谁就拥有推进权。
9. **总线纪律四条**：① 没人等输入 ⇒ 游戏侧每拍抽干总线（`waiting_input()` 存在的全部理由，否则一个开着对话框的下午能让队列涨到几万条）；② 不认识的事件**按原序放回**（批量消费用位图标记再 `flush`，边扫边搬会被借用检查判死，而顺序必须保持 —— 松开在按下之后是常态，反过来就是丢按键）；③ 一次按下只解一次挂（见第 10 条）；④ 脚本后端取走自己的 `Custom` 时同理。
10. **解掉上一句 `ShowText` 的那次按下，不该答下一题**。`InputState::feed` 先放裸设备事件、再放它折出来的 `Action` 边沿（同一次物理按下，相邻两条），所以 `ok` 边沿被 `ShowText` 吃掉之后，裸按下会按原序留在总线里给紧跟着的 `InputKey` 看见。规则：**没有可打印字符、或只有控制字符的按下不算 `InputKey` 的答案**。⚠️ 只判 `ch: Some(c)` 挡不住 —— winit 在 wasm 里递过来的 Enter 字符是 `'\r'`（控制字符），不是 `None`。浏览器里的表现是"井边人永远停在'太小了'，任何数字都进不来"；钉成 `the_press_that_closed_the_last_line_does_not_answer_the_next_prompt`。
11. **`InputKey{timeout: 0}`（无限等）必须把挂起装回去**。`resolve` 是先把 `Hold` `take()` 出来、再按分支装回的，`timeout == 0` 那一支漏装 = "无限等"实际只等一拍，数字永远进不来 —— 与第 10 条**症状一模一样**。两条都在浏览器里各撞了一次，所以两条各有单测（同一份测试断言 `Some("key")` 连续两拍不变）。教训：症状相同的两个 bug，修完第一个就要立刻复测，否则会把第二个当成"没修好"再改一遍第一处。
12. **像素探针的两个自坑处**：① `drawImage` 必须在应用自己的 rAF 之内，否则读到已清空的缓冲（`docs/platforms.md` 早写过，这一轮又踩）；② `Animate{Fade{to:0.3}}` 在 `206,212,229` 之上读出来是 `122,127,136` —— sRGB↔linear 混合后的显示值，不是"30% 直觉混色"。拿直觉判"颜色错了"会白查一轮任务调度器与补间实现。
13. **网格走位的事件必须 `wait: true`**。不阻塞的话事件机在玩家还没走完那一格时就开始说话，窗体压在角色身上 —— 而 `MoveRoute{wait:false}` 在演示里只有一处合理用法：`Animate{Fade}` 那种"淡出个精灵"的收尾。
14. 按 DoD-3 登记一条清单外新增依赖边（见下方"钉版增补"）：`sisf-event → sisf-asset`（`AudioPort::play` 要把 `Sound.name` 交给 VFS 解析，虚拟机自己不解资源）。**没有新增第三方 crate。**


## T11 rpgmk 兼容层 + 视频桩 + 移植验证
- [x] `sisf-rpgmk/src/map.rs`：MV/MZ 风格 `data/Map*.json` 宽松反序列化（只取 events/pages/commands 必需字段）、`System.json` → `ActionMap`、`Tilesets.json` + 图块图集（层序简化为三层）。验收：一张真实 MV 地图能加载。→ `MapDef::load(vfs, id)` 读 `data/Map{NNN}.json`（`TILE_LAYERS=3`，`tile(x,y,z)` 按 `[(z*h+y)*w+x]`，遮挡旗 bit0..3=下/左/右/上）、`SystemDef::load` 读 `System.json`（**出生点在这里**：start_map_id/x/y/direction）、`action_map()` 从 corescript 的 `Input.keyMapper` 造 `sisf_event::ActionMap`。验收 → 仓库里那份自造散文件工程（`examples/assets/mv/`，`gen.py` 生成、BFS 验过 237 格连通）+ `archive.rs` 的 `the_real_project_loads_after_being_zipped`：把**同一份** `System.json/Map001.json/Tilesets.json` deflate 压包再解，走 `MapDef::load` 同一调用点，断言 `(24,18)` 与 4 个事件 —— "整个兼容层不需要知道资产在包里还是在盘上"就是这条测的。
- [x] `translate.rs`：MV 事件码 → `Vec<Command>` 映射（消息/选项/变量/移动/等待/BGM/SE/淡入淡出/场景转移/条件分支）；未覆盖码降级为 `Command::Custom("rpgmv.unimplemented", ..)` 并 warn 一次（不 panic）。验收：映射表逐条单测 + unimplemented 计数打印。→ 30 条指令/事件表的翻译表 + `TranslateReport`（条数/降级码计数/丢缩进/丢路线步）。运行期账单在两端一致：`翻译账单: 30 条指令，降级 1 种码 [251×1]，丢缩进 0 行，丢路线步 0 步` + `首次降级 warn 1 条`。`sisf-rpgmk` 本地 9 条几何/翻译/页选择/遮挡单测（`Cargo.toml` 注释点名的口径）。
- [x] `game_vars.rs`：`Game_Variables/Game_Switches/Game_SelfSwitches` 风格存储 + `save/load`（RON 或 JSON，变量号 1-based 直映）。验收：存档往返 + 读档后事件页状态正确。→ `VarStore`（switches/ints/texts + 可存盘 splitmix64 RNG，`set_seed` 复现）；`request_refresh` 由 `store_mut` 立旗、下一拍 `world.tick` 消费（MV 的 `requestRefresh` 同款延后一拍）。运行期证据：长老选项 ⇒ `var1 11`（HUD 第三行）+ `refresh：换页 1`（console），钟声 `switch 4 ON` ⇒ 同一条 refresh 计数。
- [x] 最小场景层：`Scene_Map`/`Game_Character`（图块碰撞、随机行走、事件页 `condition{switch,var,trigger}`）。验收：事件按条件出现/消失。→ `enter/refresh/tick/move_straight/can_pass/front/events_at` + `Refresh{changed,hidden,unsupported}` 账单；页选择按 MV 从后往前取第一条成立的页（石碑亮页带 `switch 3` 条件、暗页无条件，双端行为一致）。运行期证据：`refresh：换页 1，隐没 0/1，不支持的条件 0（按页计）` 出现三处（钟声开关、史莱姆擦除、长老开关）；事件 4（史莱姆，trigger 1）被 `214` 擦除后 `views` 计数跟着掉。
- [x] 归档插槽：`zip 8.6.0`（native）注册为 `VfsProvider`；`docs/porting.md` 记录 RGSSAD/RGSS2A/MV-MZ 加密为"待建 provider（约 200 行；crates.io 无现成 crate，参考 rpgmad-lib / Luminol）"并给接口骨架。验收：zip 内资产可读 + 文档明确未实现项。→ `crates/sisf-rpgmk/src/archive.rs`（`archive` feature，`cfg(not(wasm32))`；`ZipProvider: VfsProvider`，5 条单测含"真工程压包再读"那条）。`docs/porting.md` 第 5 节给 `RgssadProvider` 接口骨架与"约 200 行"定价。⚠️ 偏离：**本示例与单测没有从真实商业 MV 工程取 zip 样本**（授权），包内资产可读的证据是自造工程的那条单测。
- [x] `sisf-media/src/video.rs`：`trait VideoSource{ next_frame, audio }` + `VideoPlayer{ texture:BitmapHandle }`（帧写入 Bitmap，sprite 直接引用）；native `video` → `ffmpeg-next`（Win/Linux/macOS/iOS），wasm → `<video>`+canvas / WebCodecs 胶水。**本阶段两端返回 `Unsupported`，只保留 API 与文档**。验收：feature 组合三端 check 绿；`rpgmv_shell` 遇 movie 安全跳过并 warn。→ 解码/拉帧留 `sisf-media::video`，碰 GPU 的 `VideoPlayer`（`texture: Option<Bitmap>` + `skip_notice` 闩锁）上门面 `sisf/src/video.rs`（render→media 反向依赖不可行，见模块头）。`open` 恒 `VideoError::Unsupported` 且认不认容器都是同一句文案（有单测钉"门面与 media 层文案同源"）。运行期：石碑亮页 `261 ['ending']` → `跳过影片 \`ending\`：视频后端尚未接入（native 需 ffmpeg-next、wasm 需 <video>/WebCodecs 胶水）：显式失败而不是静默黑屏` —— **两端 console 各实测一次**（桌面 `/tmp/rpgmv_desktop2.log` 末行；wasm console msgid 138），第二次读同页不再刷（闩锁，`done("movie skipped again")`）。三端 check：`cargo xtask check-all` 四组净。
- [x] `examples/rpgmv_shell.rs`：加载一个散文件 MV 风格工程（`data/*.json` + `graphics/` + `audio/`），进图、走动、触发一个事件、播 BGM。验收：桌面 + Chrome-wasm 跑通（移植可行性证明点）。→ 两端全链实测（2026-09-27）。**桌面**（`cargo run -p sisf --example rpgmv_shell --features image,ogg,video,rpgmk`，stdout 留档）：17 行启动链（RTX 2080 Ti/Vulkan、Inter 266 字形、3 张图 epoch、进图 1「Shell」24x18 出生点 (2,8,6)、4 事件页/指令数、翻译账单、地板 488 块、BGM 88200 帧/2.00s 只此一次）；PostMessage 逐格走动日志 `(2,8)→(3,8)→(4,8)→(4,9)→(5,9)→(6,9)→(7,9)→(7,8)→(8,8)…(15,8)`（被挡那几条打印 `Blocked` 且坐标不动）；事件链 `事件 3 启动（钟声，trigger 3）→ SE Confirm01 → refresh` → `事件 1 启动（长老）` 选项 → `refresh：换页 1` → `事件 2 启动（页 0，石碑）` → **`跳过影片 \`ending\`…`**。**wasm**（Chrome，`?ex=rpgmv_shell&wgl`，浏览器 MCP 真实按键 + 截图 + 逐帧像素探针）：启动链 17 条与桌面逐行一致；像素探针确认地图真渲染（主导色草 `(80,120,40)`/岩 `(120,80,80)`/路 `(200,160,120)`）；HUD 读数 `pos 4,8 · spr 192.0,432.0`（精灵逐拍追到设备像素点阵）、`hold text 0 · choices · pos 7,8`（长老选项挂起）、`pos 15,8` 后 `跳过影片`；史莱姆接触 `ME ← audio/me/Battle01.ogg` + `事件 4 被 214 擦除`。⚠️ **桌面画面不可截屏**（CopyFromScreen/PrintWindow 拿不到 wgpu Vulkan 交换链，客户区白/黑）——像素级证据全在 wasm 侧补，桌面证据是 stdout 逐行账。
- 验收（整段）：`rpgmv_shell` 两端跑通 → 提交 `P9 rpgmk layer+video stub`。→ **闸门（2026-09-27，全部原命令实跑）**：`cargo xtask fmt --check` 净 / `cargo xtask clippy` **四条腿**净 0 warning（宿主 `--workspace --all-targets`、宿主 `--all-features`、wasm32 `web,ogg,mp3,wav,midi,rpgmk,video`、wasm32 `web` —— 后两条是本轮新增，见事实第 3 条）/ `cargo xtask check-all` 四组净 / `cargo test --workspace --all-features` **30 个测试二进制 407 通过 2 ignored 0 失败** / `cargo xtask wasm-test` **222 通过**（八条腿：原七条 + `sisf-rpgmk` 27，exit 0）/ `cargo xtask golden` 两条基线绿。复验一次按一次做：示例在复验中修了两处真 bug（玩家画法、轻点一步），修后闸门整组重跑。

### T11 期间发现并修正的事实（供后续阶段引用）
1. **MV 的出生点在 `data/System.json` 不在地图里**（`startMapId/startX/startY/startDirection`）——`Map001.json` 只有地形与事件。`Scene_Map::enter` 因此吃的是 `SystemDef` 那一组，示例的"进图"日志把四个数都打了。
2. **MV 行走图的站立帧是中列**（三列 = 左步/站立/右步），帧序映射是 `[1,0,2]`；照"第 0 列是站立"写会让所有角色走路时永远缺一帧。`MoveRoute` 的步帧序列是 `0→1→2→1→2…`，宿主的 `chase` 用同一条序列才不会玩家与事件一快一慢。
3. **带 `required-features` 的 target 在 `clippy --workspace --all-targets` 下是静默跳过**：feature 不满足 ⇒ 整个 target（含其单测）不进编译，闸门照绿。`rpgmv_shell`（要 `image,ogg,video,rpgmk`）连同 `sisf-rpgmk` 的 `archive` 模块因此隐身——`video.rs` 里一条 `doc_lazy_continuation` 就这么绿过去了。修法在 xtask：`clippy` 从两条腿加到四条（补宿主 `--all-features` 与 wasm32 `web_features()` 口径），与测试闸门 `cargo test --workspace --all-features` 对齐。教训同 T12 事实第 12 条的形状："某组合没被编译"不报错，得靠闸门自己把口径铺满。
4. **宿主给玩家摆精灵不能套 `page_art(Option<&EventPage>)`**：玩家没有事件页，`page_art(None)` 把他判成"不画不追"——精灵停在出生格，第一步之后 `arrived` 永远 false，`player_tick` 的到位闸门把**整条输入**卡死（wasm 复验实际踩到：pos 走了、spr 纹丝不动，后续轻点全部无效）。桌面同代码同病，此前没暴露只因为桌面没走到第二步。修法：`poses()` 给玩家显式 `PageArt::Char(PLAYER_CHARACTER)`（`System.json` 惯例的初始角色）+ 玩家速度。
5. **"轻点"必须当一步意图记下来，不能靠 `held`**：wasm 的 CDP 按键与 PostMessage 短按都把 keydown+keyup 送进**同一批事件**——Press 设 `held`、Release 同批清零，`player_tick` 一个方向都没吃到（症状：轻点永远不走，长按才走）。修法：Press 记 `intent`（轻点 = 走一格，MV 同款行为），Release 只清**同方向**的 `held`（换向后迟到的 Release 不许把新方向也清掉）。轻点撞墙 = 原地转向，消耗掉这次意图，不挂到下一拍补走。
6. **事件页条件决定能拿到哪页，双端行为差异先查剧情线再查平台**：石碑亮页（带 `261`）挂 `switch 3`，暗页无条件。第一轮桌面复验"没有跳过影片"不是桌面代码有差——那条路线绕开了长老、switch 3 没开，拿到的是暗页。跟长老对话拿到开关后，同一串按键原样复现了亮页与跳过日志。
7. **图集装箱只吃 premultiplied RGBA8**（`atlas_gpu.rs` 的入参契约，`sisf-media` 解码链统一预乘）：MV 素材必须从 `AssetStore` 的解码链取位图，宿主自己 `image::load` 裸塞进图集是直通 alpha。
8. **事件速度折算成"像素/拍"后要落到设备像素点阵**：玩家 3px/拍 × 整倍率才逐格精确落格（`lattice_alpha`/`snap_to_device`）；`moveSpeed` 折出来不是 3 的整数倍的事件，追逐会停在半像素上，`arrived`（`<0.001` 的判据）跟着失灵——场景层把速度折到格子几何，宿主把插值折到点阵，两边必须用同一份折算。
9. **`boot()` 的"到货 ≠ 装箱"**：`assets.ready(&sheets)` 只说明字节都在，`paint` 查的是装箱后的 `art.tiles`/`art.chars`——漏一张装箱就是"启动时 panic 在 expect 上"而不是"这一格不画"。
10. **视图缓存按页号做键**：换页（页号变）必然重画并重挂动画；画法是页的函数，不能拿画法当键（同一画法的两页要区分自开关等状态）。
11. **wasm 侧 MV 工程要两份清单两个 URL 前缀**：工程键保持 MV 的 `data/…`（`MapDef::load` 拼路径用），fetch 前缀是 `assets/mv/`；字体那份还是 `assets/`。`web/assets/mv/` 是 `examples/assets/mv/` 的**副本**（清单从本站抓），改一处要改两处。
12. **MV 指令 `251` 官方有归属、兼容层没建模** ⇒ 进 `Custom("rpgmv.unimplemented")` 计为降级（翻译账单 `[251×1]`），不是丢弃也不是 panic。`VarStore::store_mut()` 则**无条件**立 refresh 旗——它不知道"这次是不是真改了"，宿主每拍消费即可，别指望它省一轮 `refresh`。

## T12 CI + 打磨 + 三端发布
- [x] 门面 `sisf`：re-export + `lib.rs` 顶层 doc 示例（≤40 行：开窗 + 一个 sprite + BGM + 一个 RON 事件）。验收：`cargo test --doc`。→ 四样把手齐：`platform::run(Mini::new(), AppConfig::default())` 开窗、`Sprite::solid(Vec2::new(96.,96.))`（`bitmap = None` ⇒ 内置白图，`src` 宽高即目的尺寸）、`AudioBus::open()` + `play_bytes(Channel::Bgm, …)` 放 `audio/tone_2s.ogg`、`EventDef::from_ron(well.ron).compile()` + `vm.start(1, script, 0)` 跑磁盘上的 RON。`EventCtx` 八个字段全都递到（`sprites/tasks/scene/audio/ui/vars/bus/vfs`），`draw` 走 `collect_at(ViewportRef::MAIN, alpha, …)` + `append_list`。验收 → `cargo test --doc -p sisf --all-features` = `test crates\sisf\src\lib.rs - (line 7) - compile ... ok / 1 passed`（默认 feature 同样 1 passed）。⚠️ **偏离清单字面量：示例 65 行而不是 ≤40**（fence 在 `lib.rs:7`/`lib.rs:73`）。超出的部分不是废话：两个端口桩 12 行（含它们自己那 2 行说明；`EventCtx` 对 `audio`/`ui` **各要一个 `&mut`** ⇒ 一个 struct 同时实现两个 trait 也省不掉那对 `impl`，见下方事实第 9 条）+ 7 处解释性注释（3 处整行 + 4 处行内：`begin_tick` 必须是每拍第一行、64 是一拍的指令预算、present 在 `Runner::end_frame` 里、事件机是总线唯一读者……）。压到 40 行只有删注释这一条路，而这个示例的存在理由就是"照抄一遍就能学会边界"。
- [x] `docs/`：`porting.md`（RPG Maker 语义 ↔ Sprite/Command 对照、字体与 SF2 自备说明、RGSSAD 待办）、`platforms.md`（三端构建 + iOS 启用）、`audio-web.md`、`bench.md`。验收：命令可复制即成功。→ `porting.md` 新增，七节：①RGSS 对象在哪（`Bitmap/Sprite/Viewport/Color/Tone/blend_type/Graphics/Audio/Input/Interpreter/Window/Game_Map/Font` ↔ 本引擎落点，逐行给 ✅/❌/⏸）②事件指令对照（MV/MZ 的"事件指令"页 → `Command` 的**十七**个变体；没覆盖的三组 —— 图形/透明度/图块变更、转移玩家、商店/战斗/游戏结束 —— 明确标 ⏸ T11 归宿主，脚本那行指向 `Custom(name, args)` + `ScriptBackend`）③存档与确定性（`VarStore::seeded` + `to_ron` 稳定 ⇒ 存档 diff 才有意义）④`blend_type` 2/5 的降级写法 ⑤宿主自备三样（CJK 字形、SF2、RGSSAD 不解密 + provider 接口骨架）⑥会咬人的语义差 6 条 ⑦起手式 6 步。写的时候**四次**把 API 名写错（`Scene::register_actor`、`SpriteArena::slot`、把 IR 包装 `Instr::Run(Command)` 当成可写的"公共事件"指令、`AppConfig::tick_rate` 实为 `tick_hz`）→ 全部改成真签名，并把复核固化成一条脚本：抽出文档里所有反引号标识符、按最后一段在 `crates/**.rs` 里 grep，剩下的未命中项应当只有 RGSS 侧的名字（本轮结果就是三个：`FileWindow`/`Game_Map`/`Interpreter`，全在左列）。教训写在事实第 10 条。`platforms.md` 新增"## wasm 上的纯逻辑测试（T12）"（七条腿 / 195 / 未上 wasm 的套件与其原因 / 三条防空绿设计 + 一段"harness 只吃 filter/`--skip`/`--list`/`--exact`/`--nocapture`/`--format`，`--quiet` 与 `--test-threads` 会 exit 1"）+ "一条命令"段补 `cargo test --workspace`、`xtask wasm-test`、`xtask golden`；`audio-web.md` 修掉"`xtask web` 是 plain wasm-pack"那句过期说法，并把泵账 mjs 归位成"JS 胶水契约测试，**不在** `xtask wasm-test` 那一腿里"。命令逐条复跑（本轮原命令）：`cargo xtask check-all`/`clippy`/`fmt --check`/`golden`/`wasm-test`/`web sprite_bench --no-serve`（→ exit 0，`产物在 D:\Projects\sisf\web\pkg`）全绿；`node crates/sisf-audio/tests/pump_bookkeeping.mjs` exit 0 且五条 fps 全 ok，`--transit 1`/`--transit 3` 的 FAIL 字面量（`12800 帧，低水位 0f` / transit=3 连 60fps 都撑不住）与文档正文**一字不差** —— 那两条是"复现缺口"的手动实验、不是闸门（事实第 5 条）。⚠️ `bench.md` 桌面那行 `cargo run --release -p sisf --example sprite_bench -- 300 100000` 本轮**未复跑**（沙箱不放行起窗口跑基准），表内读数仍是 2026-09-23 的实测；`platforms.md:26` 的 `cargo run -p sisf --example hello_window` 与 iOS 那两行（`rustup target add aarch64-apple-ios*` + `cargo check --target aarch64-apple-ios-sim`）同理 —— 前者是手测步骤、后者要 macOS，都不是本轮能签字的。
- [x] `xtask web`：wasm 产物 + 本地静态服务一条命令；`xtask golden --bless`。验收：本地一条命令跑通。→ `cargo xtask web [示例名] [--no-serve]`：本轮 `cargo xtask web sprite_bench --no-serve` exit 0，里面那条真实构建是 `cargo build --release -p sisf --example sprite_bench --target wasm32-unknown-unknown --no-default-features --features web,ogg,mp3,wav,midi`（15.03s），产物落 `web/pkg`，`--no-serve` 后直接返回而不是进 `incoming()` 死循环；不带示例名时缺省 `hello_window`（CI 就钉这一条）。`cargo xtask golden --bless` exit 0（两条场景 `sprite_scene_golden` + `text_scene_golden` 各 1 passed），且 `git status --short -- '*.png'` **空输出** ⇒ bless 重写出来的基线与仓库里那份逐字节相同 —— 这一条顺带证明了 golden 是确定性的（`VarStore::seeded` + 固定 `alpha`），而不只是"跑通了"。
- [x] `xtask android`：`cargo-ndk` 安装说明 + `adb install` 流程 + 真机手测清单（窗口/触摸/音频/suspend 恢复）。验收：至少一次真机通过记录。→ 拆成两条：`cargo xtask android` = 只读体检（exit 恒 0），`cargo xtask android build <crate>` = 真构建。体检本轮实跑读数：`ANDROID_NDK_HOME / NDK_HOME 未设置`、`cargo-ndk : error: no such command`（附 `cargo install cargo-ndk --locked` 指引）、`adb Android Debug Bridge version 1.0.41 / 36.0.2-14143358 / D:\LanguageTools\AndroidSDK\platform-tools`、`aarch64-linux-android [x]`、`（无设备）`。两条硬失败也都实跑过：`android build sisf` → **exit 9**（引擎 crate 是 rlib，链不进 APK），`android build my_game` → **exit 8**（无 NDK）。判据顺序本轮改成"先包名后环境"，否则没有 NDK 的机器上连 exit 9 这条都验不了（事实第 6 条）。⚠️ **真机通过记录：没有** —— 这台机器 0 设备、无 NDK，`docs/android.md` 第 6 节那份清单（窗口/触摸/back 键/音频/suspend-resume/权限）标题已改成"T12 交付清单本身，执行待有设备时回填"并逐条挂着 ⚠️ 未执行。这一条按清单字面量是**没达成**的，别当已通过。
- [x] `xtask wasm-test`：`wasm-pack test --chrome` 跑纯逻辑测试（VFS、atlas 装箱、batch 排序、事件 VM、mixer 确定性）。验收：本地与 CI 同绿。→ **合计 195 个用例通过，七条腿**（清单在 `xtask` 的 `WASM_TESTS`，每腿打印各自计数）：`sisf-asset` 5（`VfsPath` 规范化）、`sisf-audio` 19（`--no-default-features` ⇒ 只有纯 mixer，设备通路不在 wasm）、`sisf-render` 34（atlas 装箱 / batch 排序 / ring / vertex / clip）、`sisf-sprite` 51（arena/draw/sprite/anim/task）、`sisf-app` 15 + 10（两条腿，见事实第 3 条）、`sisf-event` 61（command/vars/runner/script/bus/input = 事件 VM）。清单点名的五组**全在里面**。⚠️ **偏离清单字面量：驱动是 `wasm-bindgen-test-runner` + node，不是 `wasm-pack test --chrome`**（本机没有 wasm-pack/Chrome；且"跑在浏览器里"是 wasm-bindgen 的**编译期**选择而非命令行开关，`--chrome` 这类参数根本不会透传）—— 记在事实第 1、2 条。没上 wasm 的套件（vfs 的 DirProvider、golden、`sisf-text`、GPU 读回、cpal 侧）逐条列了原因，"跳过"必须写成一个短语而不是一行注释。CI 与本地同绿这一半**未验证**（无 remote，事实第 8 条）。
- [x] `.github/workflows/ci.yml`：windows + ubuntu job，`fmt --check`、`clippy --all-targets -- -D warnings`、`xtask check-all` 双 feature 集、wasm 产物上传、golden 仅 windows。验收：PR 上三 job 绿。→ 四个 job：`lint`（ubuntu：`xtask fmt --check` + `clippy` + `check-all`，apt 装 `libasound2-dev libudev-dev pkg-config`，cpal 要 ALSA、gilrs 要 udev 头）、`test`（windows 全跑 / ubuntu `-- --skip gpu_tests --skip readback_clear`，因为那两组是 `.expect(adapter)` 硬失败不是跳过）、`wasm`（`wasm-bindgen-cli` 版本从 `Cargo.lock` 里 awk 出来 —— 本机验：`0.2.128`，与 `wasm-bindgen-test 0.3.78` 同族；+ `xtask wasm-test` + node 泵账 + `xtask web hello_window --no-serve` + `upload-artifact web/pkg/`）、`golden`（**只在 windows**：那几个测试文件整个 `#![cfg(windows)]`，在别的 OS 上编译成 0 用例然后"通过"，那种绿没有意义）。工具链钉 nightly（本地判定标准出自 1.95.0-nightly，stable 的 rustfmt 与它 disagreement 时红的是 CI 不是代码）。⚠️ **"PR 上三 job 绿"未达成、也无法达成** —— `git remote -v` 为空，这个仓库没有远端，CI 从没在 GitHub 上跑过一次。本轮能给的替代证据：四个 job 里除 `apt-get` 与 `cargo install` 两步之外的**每一条命令都在本机原样跑过**（含 `--no-default-features --features web` 那一组），YAML 只做了手写的缩进/键序复核（本机没有 pyyaml：`import yaml` 直接 ImportError，没为它装东西）。复核过程中**抓到并修掉一个真的常红**：我原先把 `pump_bookkeeping.mjs --transit 1/3` 也写成 CI 步骤，而 `--transit 3` 退出码是 1 —— 那两条是"复现已知的在途缺口"的实验，按设计就 FAIL（事实第 5 条）。
- [x] 混合模式收口：补 `Multiply/Screen` 两条 pipeline，或从 `BlendMode` 移除并在文档标注——二选一，不留空分支。验收：golden 覆盖或枚举已删。→ **选了删枚举侧**（用户定案）：`Blend` 现在只有 `Normal/Add/Replace` 三个变体、`renderer.rs` 里 `component` 与 `label` 两个 `match` 都**没有 `_` 分支** ⇒ 以后加变体必然编译失败，"写得出、画不出"这条路在类型层面被堵死。`Replace` 只为 `clear_rect` 存在（不做混合、片元输出即目标像素，替代 wgpu 的区域 clear）。`docs/sprite.md` 的对照表两行从"⏸ 不引入/待收口"改成"❌ 不移植（T12 定案）"，并新增"## 混合模式与矩阵调色的定案（T12）"一节讲清三件事：为什么不加 pipeline、为什么 `Tone::Matrix` 保持报错（5×4 矩阵既进不了 80B 逐精灵 `Instance`，也不能塞 per-draw uniform —— 那会把同批不同精灵的调色并成一份）、为什么绝不"静默当无效果"。RGSS 侧 `blend_type=2/5` 的降级写法落在 `docs/porting.md` 第 4 节。同一节里顺手把过期的一行（"位图 RT 作源 ⏸ T6"）改成 ✅。验收 → `cargo xtask golden` 两条基线绿（上面那条命令）+ `cargo test --workspace --all-features` 331 全绿。
- [x] 可选 `inspector`（egui 0.36）：sprite/batch/纹理页/帧时间面板。验收：feature 开关下 `check-all` 绿。→ **删掉这个 feature**（用户定案，DoD-4）：`crates/sisf/Cargo.toml` 与 `crates/sisf-app/Cargo.toml` 里的 `inspector` 一起删，原地留一行说明"没有 `inspector`：T12 定案不引 egui 调试面板"。理由是它当时已经是**空 feature**（开了什么都不发生）—— 一个不改变任何代码的开关比没有开关更糟，它会让 `check-all` 少覆盖一条真实组合却看起来覆盖了。不引 egui 0.36 ⇒ 钉版清单没动。要做面板的话它属于宿主（`FrameCtx` + `BatchList` 的只读视图已经够画），不该是引擎的 feature 面。
- 验收（整段）：CI 全绿 + 4 篇文档 → 提交 `P10 polish+ci`。→ **闸门（2026-09-26，全部原命令实跑，退出码逐个记过）**：`cargo xtask fmt --check` 净 / `cargo xtask clippy` 净（宿主 + wasm32 两条 `-D warnings`）/ `cargo xtask check-all` 四组净 0 warning / `cargo test --workspace --all-features` **331 通过 0 失败 0 warning** / `cargo test --doc -p sisf --all-features` **1 通过** / `cargo xtask golden` **2 条基线绿**（`--bless` 后 png 无 diff）/ `cargo xtask wasm-test` **195 通过**（七条腿，exit 0）/ 文档侧 `node pump_bookkeeping.mjs` exit 0 + `xtask web sprite_bench --no-serve` exit 0 + `xtask android` 体检 exit 0（build 两条 8/9 都验）+ `cargo doc --no-deps --workspace --all-features` **0 警告**（原本 16 条，见事实第 13 条；没接进 CI/xtask，判据就是那句"输出里没有 `warning:`"）。上面这七个退出码在文档链接修完之后**整组重跑过一次**，不是修前读数的复述。4 篇文档 = `porting.md`(新) / `platforms.md` / `audio-web.md` / `bench.md`（外加 `android.md` 与 `sprite.md` 的定案段）。⚠️ **两处"整段验收"里没做到的**：①"CI 全绿"这一半只能是"CI 的每条命令在本机绿"，GitHub 上一次都没跑过（无 remote）；②android 真机清单一条未执行。T11 整段仍是未完成状态（`crates/sisf-rpgmk` 只有 `lib.rs`，`video` 还是 `[]` 空 feature）—— P10 只代表 T12，别读成"计划全做完了"。

### T12 期间发现并修正的事实
1. **`.cargo/config.toml` 的 wasm runner 原本是 `echo 'run via cargo xtask web instead'`** —— 那是本轮最危险的一处：任何 `cargo test --target wasm32-unknown-unknown` 都会"跑成功"（echo 退出码 0）而一个用例都没执行。换成 `wasm-bindgen-test-runner` 之后第一件事就是发现**大部分 crate 在 wasm 上根本没有用例**：libtest 的 `#[test]` 属性对 wasm-bindgen-test-runner 不可见，必须在每个 `mod tests` 里加 `#[cfg(target_arch = "wasm32")] use wasm_bindgen_test::wasm_bindgen_test as test;`（六个 crate、逐模块，非 wasm 侧一行不变）。为此新增清单外依赖 `wasm-bindgen-test 0.3.78`（只在 dev-dependencies 且只在 wasm32 target 段声明 ⇒ 运行时依赖面零增长，见"钉版增补"）。
2. **防空绿要写进外壳而不是写进文档**：`xtask wasm-test` 现在数每腿的 `test result: ok. N passed`，**合计为 0 且没透传任何过滤器时返回 exit 7** 并解释第 1 条那个 alias。为什么带过滤器就放行：`cargo xtask wasm-test atlas` 合法地让**其余六条腿各 0 个用例**（本轮实跑两次：只有 `sisf-render` 出 9 个，合计 9、exit 0），把这种情况也判红就成了"过滤器不可用"。
3. **`sisf-app` 在 wasm 上必须拆成两条腿**，原因是 node 而不是代码：一次跑完 25 个用例会在最后一个用例之后、进程退出之前以 `0xC0000409`（fastfail）中止，用例本身全 ok。边界量过：`frame+normalize` ≥13 个用例即崩、≤11 个正常退出；`frame+timing` 15 个不崩；`timing+normalize+custom` 16 个不崩；`sisf-sprite` 51 个不崩 ⇒ 崩的是 `sisf-app` 这一 crate 的某一组，不是"用例多了"。`--format terse` 照崩 ⇒ 也不是输出量。**处理方式是拆腿而不是吞退出码**（吞掉就等于把"子进程没干净退出"这件事从闸门里删了）。拆成 15 + 10 两条之后各自 exit 0。
4. **`cargo test -p sisf-app --target wasm32-unknown-unknown --no-default-features` 编译不过**（E0433 `web_sys`）：`--no-default-features` 在这里的含义是"不开 `native`"，而 wasm 侧的事件翻译代码要 `web` 那条 feature 才存在。所以 `WASM_TESTS` 里两条 `sisf-app` 腿都带 `--features web` —— `check-all` 里那一组 wasm 组合是 `--no-default-features --features web,...`，别把它的字面抄成"no-default-features 就够"。
5. **"某条命令在 CI 里是不是闸门"要看它的退出码，不是看它打印了什么**。`pump_bookkeeping.mjs --transit 3` 打印的 FAIL 是文档正文预言过的复现（`docs/audio-web.md`"定价"一节：在途缺口只发生在 >25ms 的长帧上，本阶段保留 `TARGET_FRAMES = 1024` 不修），退出码 1；把它写成 CI 步骤就是造一个常红的 job。默认那条 exit 0，才是闸门。
6. **两条硬失败的判据顺序是有内容的**：`xtask android build sisf` 原本先查 NDK 环境变量、后查包名 ⇒ 在没有 NDK 的机器上永远只看到 exit 8，exit 9 那条"你在要求引擎给自己编 APK"的判据**从来没被执行过**。调换成先包名，两条都在本机验了。
7. **"命令可复制即成功"这一类的复核，抓到的是自己写的过期句**：`docs/audio-web.md` 还写着"`xtask web` 是 plain `wasm-pack`"（T7 之后就不是了），`docs/sprite.md` 的表里还挂着"位图 RT 作源 ⏸ T6"（T6 就做完了）。写文档的人不会主动回来修，所以复核必须是"逐条命令 + 逐条断言重新对一遍代码"，不是"扫一眼有没有明显错字"。
8. **CI 里凡是"环境决定"的步骤都要写清它红了意味着什么**：golden job 钉 windows 并注明 runner 上的 adapter 通常是 D3D12 WARP，某天取不到会红在那行 `.expect(...)` —— 那不是回归。ubuntu 那条 `--skip gpu_tests --skip readback_clear` 也写明了它们是硬失败而不是跳过，否则下一轮有人会把 skip 删掉，然后收获一腿"随机红"。
9. **门面 doc 示例里两个端口桩不是可选的整洁**：`EventCtx` 要 `audio: &mut dyn AudioPort` **和** `ui: &mut dyn UiPort` 两个不同的 `&mut`（同一对象同时 `impl` 两个 trait 也拿不出两份可变借用），所以"一个 struct 打发两个端口"写不出来。示例里为省这 12 行而把 `ui` 塞进 `AudioPort` 那一个实现，会在读者手里变成编译错误。
10. **写移植文档最容易犯的错是"发明一个更顺手的 API 名"**：`porting.md` 初稿里错过四次 —— `Scene::register_actor` 与 `SpriteArena::slot` 都不存在（真名 `Scene::set`、`spawn/get/get_mut`），`AppConfig::tick_rate` 实为 `tick_hz`，最坏的一个是把 `Instr::Run(Command)` 写成"公共事件"的可写指令（`Run` 是编译后给每条叶子指令用的**包装**，枚举里根本没有这一条，真把手是 `EventRunner::start_program`）。前两个靠肉眼发现，后两个靠把规则改成脚本：抽出文档里每个反引号标识符 → 按最后一段 grep `crates/**.rs` → 未命中的必须逐个解释（本轮剩下的三个未命中全是 RGSS 左列的名字）。**"看着像"不算证据。**
11. **`inspector` 那类"可选 feature"在没实现前先建开关是负资产**：它不进 `check-all` 的组合就永远无人编译，进了组合就得到一个"开了什么都不发生"的绿。删掉之后门面的 feature 面反而更好读。
12. **`xtask wasm-test` 的每腿 feature 不一样，不能统一注入 `--no-default-features`，也不能统一注入 `--all-features`**：`sisf-audio` 那一腿刻意不带默认 feature，为的是"只跑纯 mixer 那半边"（两条 pump 各自带 `target_arch` 条件，见 T7 的记账），而 `sisf-render`/`sisf-sprite`/`sisf-event` 用默认就行 —— 所以表里第四列写的是"这条腿在测什么"、每腿自己带参数。统一注入某一种"更宽"或"更窄"的写法，症状都是某一腿静默少编译一半代码，而用例数照样好看。本轮为 `sisf-audio --no-default-features` 之外又加了一条 `--features web`（第 4 条那个 E0433）。**顺带撤回一条本轮自己写下的假事实**：记账时我写过"`--all-features` 在 wasm 上会挂（`wgpu` 编译失败：`no such command script 'web'` / `BindPipelines`）"，重跑时四条全绿 —— `cargo check --target wasm32-unknown-unknown --all-features` 分别加 `-p sisf-audio` / `-p sisf-render` / `-p sisf` / `--workspace` 都 exit 0，`cargo test -p sisf --target wasm32 --all-features --no-run` 也 exit 0（锁里的 wgpu 是 30.0.1，不是那句里的 29.0.1）。那句话**不成立**，已从 `docs/platforms.md` 删掉；留在账里是因为它记的是比"少一个事实"更糟的事：**只在一次输出里见过、重跑不复现的失败，不能当成事实写下来**（`grep -c` 读到 0 时先怀疑命令压根没跑）。这一轮真正确认过的只有两条：`cargo test -p sisf-app --target wasm32 --no-default-features` 编译不过（第 4 条），以及 `--quiet` / `--test-threads=1` 会被 `wasm-bindgen-test-runner` 判 exit 1（两条本轮各重跑一次）—— 所以能透传给 harness 的只有 filter / `--skip` / `--list` / `--exact` / `--nocapture` / `--format`。
13. **`cargo doc` 从来没进过任何闸门，攒了 16 条警告**（DoD-2 说的是编译警告，rustdoc 的一条都不算）。本轮清零，形状记下来免得再攒：① intra-doc 链接指向 **private 项**（`DEVICE_POLL`、`OPEN_RETRY`、`MAX_PAGES`、`Hold`、`Batch::flush`、`EventRunner::hold_variant`、`GLOBAL`、`crate::runner`）—— 写成 `` `名字` `` 而不是链接；② 指向**只在另一端存在**的项（`pump_web`/`WasmTasks`/`WebManifest` 只在 wasm32 编出来，宿主文档解不到）—— 同上，并注明"只在 wasm32"；③ **链接文本与显式目标相同**（`[`X`](a::b::X)` 判 redundant，去掉目标即可）与 `fn layout`/`mod layout` 的歧义（要写 `[layout](mod@crate::layout)`）；④ 文档里裸写的 `[-1,1]`/`[0,1]` 会被当成链接解析（`unresolved link to '0,1'`）—— 区间要包进反引号。还顺手改了一条**真错链**：`command.rs` 写 `[`crate::vars::Expr`]`，而 `Expr` 就定义在 `command.rs` 自己身上。判据 = `cargo doc --no-deps --workspace --all-features` 输出里没有 `warning:`；本轮**没有**把它接进 CI/xtask（不新增闸门），下一轮要接就照这条判据写。

## 全局 Definition of Done
1. 每条子任务的验收必须留下可执行证据（命令 + 输出，或手测步骤 + 观察结果）。
2. 每阶段结束时这三种组合均无 warning：`--all-features`（win）、`--no-default-features --features web`（wasm32）、`--no-default-features --features native`（android）。
3. 不引入本清单外的依赖；确需新增时先改清单再改 `Cargo.toml`。
4. 平台不支持的能力一律 feature 关闭 + 文档标注，禁止空实现散落（除 T11 显式声明的视频桩）。

## 附：钉版增补（T1 执行时按 DoD-3 登记）
`[workspace.dependencies]` 在设计文档列表之外补充：`pollster 1.0.1`、`raw-window-handle 0.6.2`（均在设计 §依赖清单已验证）、`web-sys 0.3`、`js-sys 0.3`（wasm-bindgen 生态配套，canvas/visibilitychange 胶水需要）、`android-activity 0.6`（T2 明确）。`sisf-media` 的 `ogg/mp3/wav/midi/video/gif/simd-jpeg` 为占位 feature，对应解码依赖在 T4/T7/T11 接线（避免提前引入 turbojpeg/cpal 等破坏三端 check）。

T2 追加（只加 feature，不加新 crate）：`web-sys` 开 `Element`（复用页面 canvas）与 `console`（初始化错误打到 devtools）；`crates/sisf-render` 增加 `pollster` 作为 **dev-dependency**（GPU 读回测试用）。

T3 追加（只加 feature，不加新 crate）：`web-sys` 开 `Performance` —— wasm32-unknown-unknown 下 `std::time::Instant::now()` 会 panic（见「T3 期间发现并修正的事实」第 7 条），主循环与 `sprite_bench` 的计时统一走 `platform::now_secs()` = `performance.now()`。`xtask web` 改为接受示例名（`cargo xtask web sprite_bench`），`web/main.js` 按 `?ex=` 动态 import 对应 pkg。

T4 追加（按 DoD-3 登记，均为设计 §依赖清单已验证版本）：`[workspace.dependencies]` 新增 `png 0.18.1`、`jpeg-decoder 0.3.2`、`image-webp 0.2.4`、`gif 0.14.2`、`turbojpeg 1.5.1`（仅非 wasm target 可见，`simd-jpeg` 用）；`web-sys` 追加 `Response` feature（wasm fetch）。crate 间新增 feature 边：`sisf-media/image`（png+jpeg+webp）与 `sisf-media/gif`、`sisf-media/simd-jpeg`（换 turbojpeg）、`sisf-asset/{image,gif,web}`（`web` = wasm fetch provider）、`sisf-render/image`（默认开，图集/上传需要）；门面把 `image` 并入 `native`/`web` 两条主线（图像解码是引擎核心能力）。`xtask web` 构建后把 `crates/sisf-media/tests/fixtures/` 拷进 `web/assets/img/` 供 wasm fetch。图像 fixture（真实 PNG/JPG/WebP/BMP/GIF 小图）入仓在 `crates/sisf-media/tests/fixtures/`。T4 执行时发现：钉版名 `rect-packer` 有误，crates.io 包名是 **`rect_packer`**（下划线），T1 因无人引用未暴露——`[workspace.dependencies]` 已改名。

T6 追加（按 DoD-3 登记）：`[workspace.dependencies]` 新增 `cosmic-text 0.19.0`（T6 清单点名的版本；`etagere 0.3.0` 早在 T1 就钉了，本阶段才第一次被使用）。`crates/sisf-text` 新增依赖边 `wgpu`（自管 `R8Unorm` 覆盖率页，不复用 `sisf-render` 的 sRGB 页）、`thiserror`，dev-dependencies `png` + `pollster`（golden 自己写基线、读回要阻塞 poll）—— 与 `sisf-sprite` 的 golden 依赖面一致，都不进运行时依赖。**没有新增 crate 间 feature 边**：门面本就无条件依赖 `sisf-text`。字体资源：`Inter-Regular.ttf`（SIL OFL 1.1，许可证随文件同目录）入仓两份 —— `crates/sisf-text/tests/fixtures/`（单测 + golden 用，Inter 只有拉丁）与 `web/assets/`（wasm 手测，已追加进 `web/assets/manifest.txt` 供预取）。T6 副作用：图集装箱加 1px 复制 gutter，`crates/sisf-sprite/tests/golden/sprite_scene.png` 基线重写（渗色消失，语义断言未改）；`xtask golden` 从单场景改为遍历 `GOLDENS` 表（`sisf-sprite/golden_sprite` + `sisf-text/golden_text`），任一失败即返回其退出码。

T7 追加（按 DoD-3 登记）：**没有新增 crate**，钉版清单里的音频六件全部按原版本接线 —— `symphonia 0.6.1`（`default-features = false`，按 `ogg/vorbis/mp3/wav/pcm` 逐条开）、`lewton 0.10.2`（只在 `ogg-lewton`，默认关）、`lofty 0.25.4`、`midly 0.5.3`、`rustysynth 1.3.6`、`cpal 0.18.2`。两条上游坑登记在此：`lofty` **必须保留默认 feature `id3v2_compression_support`**（0.25.4 关掉它会留下未导入的 `Id3v2Error*`，编译直接失败，见根 `Cargo.toml` 注释）；`midly` 用 `default-features = false, features = ["std"]`（不要它的 `alloc`/wasm 附加面）。新增 feature 边：`sisf-audio/{native,web,ogg,mp3,wav,midi}`（`web = ["dep:wasm-bindgen"]`，**`js-sys` 试过又删了** —— 胶水返回普通 `Array`、Rust 侧 `Vec<f64>` 就够，留着就是清单外死依赖），以及门面 `sisf/{ogg,mp3,wav,midi}` 现在**同时**转发 `sisf-media/*` 与 `sisf-audio/*`。`sisf-audio` 两条 pump feature 互斥且各自带 `target_arch` 条件（`native` 只在非 wasm32、`web` 只在 wasm32），都没开时该 crate 只剩 `mixer` —— 没有设备通路就没有 `AudioBus`，这是 DoD-4 的刻意形状而不是空壳。音频 fixture（`tone_2s.ogg`/`tone_1s.wav`/`tone_1s.mp3`/`jingle.mid`）在 `crates/sisf-media/tests/fixtures/` 存一份、`web/assets/` 拷一份（已进 `manifest.txt`）；**SoundFont 不进仓**（体积 + 授权），`xtask check-all` 相应加第四组 `wasm32 --no-default-features --features web,ogg,mp3,wav,midi`，`xtask web` 的构建 feature 换成 `WEB_AUDIO_FEATURES`。

T9 追加（按 DoD-3 登记，均为设计 §依赖清单里点名的版本）：`[workspace.dependencies]` 新增 **`serde_json 1.0.151`**（`EngineEvent::Custom` 的载荷与 `post_custom` 的跨 wasm 编解码 —— 选 `Value` 而不是 `Vec<u8>` 是因为它在 JS 侧就是那个对象，两边不必各抄一套编解码；与已有的 `serde`/`ron` 同族，不新拉第三条序列化线）与 **`gilrs 0.11.2`**（手柄，只在 `crates/sisf-app` 的"非 wasm32 且非 android"target 段里声明）。新增 feature 边一条：`sisf-app/gilrs = ["dep:gilrs"]`，`native` 从 `dep:gilrs` 改为引用它 —— `dep:` 语法**不**生成同名 feature，所以只写 `dep:gilrs` 时 `cfg(feature = "gilrs")` 恒假、手柄代码整块蒸发而三端 check 照样全绿（见「T9 期间发现并修正的事实」第 1 条）。**没有新增素材、没有新增 xtask 步骤、门面的 feature 面未变**（`gilrs` 不上门面，`native` 已经把它带进依赖图）。文档落点：`docs/input.md`（平台矩阵 / 码词汇表 / 死区 / 边沿语义 / 一帧里输入的位置）。

T10 追加（按 DoD-3 登记）：**没有新增第三方 crate**（`serde`/`serde_json`/`ron`/`thiserror` 全是清单里已有的）。新增 crate 间依赖边一条：`sisf-event` 依赖 **`sisf-asset`** —— `AudioPort::play(&Vfs, channel, sound)` 要把 `Sound.name`（VFS 路径）交给虚拟文件系统解析，虚拟机自己不解资源也不该知道解码器在哪；这条边**不打开 `sisf-asset` 的任何 feature**（`Vfs` 与
`image`/`gif`/`web` 三条都无关），所以**门面的 feature 面未变**。门面新增 `[[example]] dialog_demo`（`test = true`，`required-features = ["image","ogg","mp3","wav"]` —— 三条解码链各用一发才验得出"四条通道都通"）。新素材：`examples/assets/events/{well,chest,npc}.ron` 三份事件表，同一份拷进 `web/assets/events/` 并追加进 `web/assets/manifest.txt` 供预取（wasm 手测跑的就是磁盘上这三份，不是内嵌字面量）。文档落点：`docs/events.md`（指令表 / 一拍的形状 / 挂起与预算 / 总线纪律四条 / 端口契约 / 存档形状 / 脚本后端 / 平台矩阵 / 以及 sprite 那边挂账的 `filter` 之答）。xtask 步骤未变。

T10 之后补账（wasm 中文，按 DoD-3 登记）：**没有新增 crate、没有新增 feature 边、门面的 feature 面未变**（`web-sys` 的 `Response`/`console` 早在 T4 就开了，`fetch` 走的是它）。新增的是**一条运行期外部依赖**：`https://cdn.jsdelivr.net/gh/notofonts/noto-cjk@f8d1575…/Sans/SubsetOTF/SC/NotoSansSC-Regular.otf`（约 8 MB，SIL OFL 1.1），写在两个示例的 `CJK_URL` 里、**不在 `web/assets/manifest.txt`**（清单只从本站 `assets/` 抓，它走 CDN），也**不入库**。风险与缓解都记在「T6 期间发现并修正的事实」第 11 条：钉 commit SHA 而非分支、404 要显式判、取不到就退回方块。引擎侧唯一改动是 `FontBundle::family()` 的记账（多份注册时返回最后那一份）。

T12 追加（按 DoD-3 登记）：`[workspace.dependencies]` 新增 **`wasm-bindgen-test = "0.3.78"`** —— 它是清单字面那句 `wasm-pack test --chrome` 背后的实际框架（`wasm-bindgen 0.2.128` 早在 T1 就钉了，同一个 `wasm-bindgen-cli` 包提供 `wasm-bindgen` 与 `wasm-bindgen-test-runner` 两个 bin ⇒ CI 里 CLI 版本从 `Cargo.lock` 现取，不写第二处）。**它只出现在六个 crate 的 `[target.'cfg(target_arch = "wasm32")'.dev-dependencies]` 段里**（asset/audio/render/sprite/app/event），所以 native 与 android 的运行时依赖面、以及发布产物的大小都没变 —— 复现：`cargo tree -e features -p sisf --all-features` 与加 `--target wasm32-unknown-unknown` 的那一份里 `wasm-bindgen-test` **各出现 0 次**（本轮实跑）；非 wasm 侧的 `mod tests` 一行没改，wasm 侧靠每模块那行 `use wasm_bindgen_test::wasm_bindgen_test as test;` alias（见「T12 期间发现并修正的事实」第 1 条）。**没有新增素材、没有新增 xtask 步骤之外的依赖**：`.cargo/config.toml` 的 `[target.wasm32-unknown-unknown] runner` 从 `echo '...'`（那是 T1 挂的占位，等于任何 wasm 测试都"0 用例 + exit 0 通过"）换成 `wasm-bindgen-test-runner`，是配置不是 crate。清单外唯一被**删掉**的东西是 `inspector` 空 feature（门面 + `sisf-app` 各一处，不引 egui 0.36 ⇒ 钉版清单反而少一项）。另有两处不属于依赖的记账：`pump_native.rs` 的 `ring_moves_data_between_threads` 把一条"应该有"式概率断言换成结构性的（开头先断言环是空的），以及 `xtask` 的 `android_build` 判据顺序改成"先包名后环境"。

T11 追加（按 DoD-3 登记；顺序排在 T12 之后，因为 T12 先做完了，本阶段是最后一块）：`[workspace.dependencies]` 新增 **`zip = "8.6.0"`**（清单字面点名的版本。crates.io 实查：8.6.0 发布于 2026-04-25，同族只有 9.0.0-pre1/2/3 更新 ⇒ 不碰预发布）。**`default-features = false, features = ["deflate"]`**：它的默认集是 `aes-crypto/bzip2/deflate64/deflate/lzma/ppmd/time/zstd/xz` 九条，会把四个解压器加一整套加密栈拖进依赖图，而 MV/MZ 工程归档（含 Steam 版）实际只有 deflate；`deflate` 自身展开为 `deflate-zopfli + deflate-flate2-zlib-rs`（纯 Rust，无需 C 工具链），非可选的 normal 依赖只有 `crc32fast/indexmap/memchr/typed-path`。声明位置是 `crates/sisf-rpgmk` 的 **`cfg(not(target_arch = "wasm32"))`** 段 + 该 crate 的 `archive = ["dep:zip"]` feature：**wasm 侧的 MV 工程走 `web/assets/manifest.txt` 散文件，没有归档可读**，所以门面 `sisf` 不转发 `archive`、**门面的 feature 面未变**。新增 crate 间依赖边：`sisf-rpgmk → { sisf-event, sisf-asset }`（翻译表产出 [`Command`](crates/sisf-event/src/command.rs)、`MapDef::load` 从 `Vfs` 读 `data/Map*.json`），两条都不打开对侧任何 feature。新增素材：**没有**——`data/*.json` 的 fixture 以字符串内嵌在单测里（官方 corescript 的 `template/` 只有 `js/` 与 `audio/`，**没有 `data/` 目录**，所以拿不到现成的真地图，见「T11 期间发现并修正的事实」）。xtask 新增一步：`WASM_TESTS` 加 `sisf-rpgmk` 那一腿（map/vars/translate 三组都是纯逻辑）。
