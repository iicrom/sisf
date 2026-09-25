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
   屏幕上，手测不靠 devtools。仓库那份 Inter 只有拉丁，中文在 wasm 上无字形（CJK 字体打包不在本阶段）。
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

## T9 引擎事件总线 + 输入映射
- [ ] `sisf-event/src/bus.rs`：`EngineEvent{Action,Key{code,kind,ch},Pointer{kind,pos,button,delta},Lifecycle,Custom(u32,serde_json::Value)}` + `EngineBus{push,drain}`。验收：drain 顺序单测。
- [ ] `sisf-app` 归一化：winit `WindowEvent/DeviceEvent` → `EngineEvent` 只做一次；wasm/Android 同一路径；`post_custom` 通道留给 JS 与移植层。验收：三端 check 绿。
- [ ] `input.rs`：`ActionMap`（命名动作 → 键/手柄按钮/指针，one-shot 与 held 语义），可从 RON 载入（RPG Maker 的 决定/取消/Shift/PageUp 键位）。验收：pressed/held/released 边沿单测。
- [ ] `gilrs 0.11.2` 置于 `native` feature（wasm 无手柄，文档标注），含死区与连发。验收：feature 组合 check。
- 验收（整段）：bus/input 四端单测 → 提交 `P8a engine bus`。

## T10 游戏事件解释器 Command + EventRunner
- [ ] `sisf-event/src/command.rs`：`Command` MVP 集（`ShowText{speaker,body,choices}`、`InputKey{var,timeout}`、`Wait`、`MoveRoute`、`Animate`、`PlayBgm/Bgs/Se/Me`、`Fade{target,frames}`、`SetVar`、`If{cond,then,else_}`、`Loop`、`BreakLoop`、`Jump{page}`、`Erase`、`Custom(String,Vec<Param>)`）+ serde + RON。验收：`.ron` 往返一致。
- [ ] `vars.rs`：`VarStore`（switch/int/var）可序列化存档；`Cond`/`Expr`（比较、开关、变量算术）。验收：嵌套 `If` 与算术单测。
- [ ] `runner.rs`：`EventRunner::tick(&mut EventCtx,budget)->RunnerStatus`，每页显式 `pc` + 调用栈（不引协程）；`Wait` 挂起；choices 消费 `EngineEvent`；`MoveRoute/Animate` 交调度器且 `wait:true` 阻塞。验收：事件 VM 单测（指令序列 → 副作用断言）+ 超预算保护。
- [ ] `EventCtx{ sprites, tasks, audio, vars, bus, vfs, ui }`（接 T6 消息窗）。验收：ShowText 驱动窗体 + 选项高亮 + SE。
- [ ] `script.rs`：`trait ScriptBackend{ start(entry), tick(&mut EventCtx) }` + `NullScriptBackend`；文档写明 QuickJS 只需实现该接口并用 `Command::Custom`/`EngineEvent::Custom` 编解码。验收：trait 对象可持有，默认路径零开销。
- [ ] `examples/dialog_demo.rs`：`assets/events/*.ron` → examine 触发对话、分支、选项写入变量、BGM/SE、Fade。验收：桌面 + wasm 全流程手测。
- 验收（整段）：事件 VM 单测 + dialog_demo → 提交 `P8b event interpreter`。

## T11 rpgmk 兼容层 + 视频桩 + 移植验证
- [ ] `sisf-rpgmk/src/map.rs`：MV/MZ 风格 `data/Map*.json` 宽松反序列化（只取 events/pages/commands 必需字段）、`System.json` → `ActionMap`、`Tilesets.json` + 图块图集（层序简化为三层）。验收：一张真实 MV 地图能加载。
- [ ] `translate.rs`：MV 事件码 → `Vec<Command>` 映射（消息/选项/变量/移动/等待/BGM/SE/淡入淡出/场景转移/条件分支）；未覆盖码降级为 `Command::Custom("rpgmv.unimplemented", ..)` 并 warn 一次（不 panic）。验收：映射表逐条单测 + unimplemented 计数打印。
- [ ] `game_vars.rs`：`Game_Variables/Game_Switches/Game_SelfSwitches` 风格存储 + `save/load`（RON 或 JSON，变量号 1-based 直映）。验收：存档往返 + 读档后事件页状态正确。
- [ ] 最小场景层：`Scene_Map`/`Game_Character`（图块碰撞、随机行走、事件页 `condition{switch,var,trigger}`）。验收：事件按条件出现/消失。
- [ ] 归档插槽：`zip 8.6.0`（native）注册为 `VfsProvider`；`docs/porting.md` 记录 RGSSAD/RGSS2A/MV-MZ 加密为"待建 provider（约 200 行；crates.io 无现成 crate，参考 rpgmad-lib / Luminol）"并给接口骨架。验收：zip 内资产可读 + 文档明确未实现项。
- [ ] `sisf-media/src/video.rs`：`trait VideoSource{ next_frame, audio }` + `VideoPlayer{ texture:BitmapHandle }`（帧写入 Bitmap，sprite 直接引用）；native `video` → `ffmpeg-next`（Win/Linux/macOS/iOS），wasm → `<video>`+canvas / WebCodecs 胶水。**本阶段两端返回 `Unsupported`，只保留 API 与文档**。验收：feature 组合三端 check 绿；`rpgmv_shell` 遇 movie 安全跳过并 warn。
- [ ] `examples/rpgmv_shell.rs`：加载一个散文件 MV 风格工程（`data/*.json` + `graphics/` + `audio/`），进图、走动、触发一个事件、播 BGM。验收：桌面 + Chrome-wasm 跑通（移植可行性证明点）。
- 验收（整段）：`rpgmv_shell` 两端跑通 → 提交 `P9 rpgmk layer+video stub`。

## T12 CI + 打磨 + 三端发布
- [ ] 门面 `sisf`：re-export + `lib.rs` 顶层 doc 示例（≤40 行：开窗 + 一个 sprite + BGM + 一个 RON 事件）。验收：`cargo test --doc`。
- [ ] `docs/`：`porting.md`（RPG Maker 语义 ↔ Sprite/Command 对照、字体与 SF2 自备说明、RGSSAD 待办）、`platforms.md`（三端构建 + iOS 启用）、`audio-web.md`、`bench.md`。验收：命令可复制即成功。
- [ ] `xtask web`：wasm 产物 + 本地静态服务一条命令；`xtask golden --bless`。验收：本地一条命令跑通。
- [ ] `xtask android`：`cargo-ndk` 安装说明 + `adb install` 流程 + 真机手测清单（窗口/触摸/音频/suspend 恢复）。验收：至少一次真机通过记录。
- [ ] `xtask wasm-test`：`wasm-pack test --chrome` 跑纯逻辑测试（VFS、atlas 装箱、batch 排序、事件 VM、mixer 确定性）。验收：本地与 CI 同绿。
- [ ] `.github/workflows/ci.yml`：windows + ubuntu job，`fmt --check`、`clippy --all-targets -- -D warnings`、`xtask check-all` 双 feature 集、wasm 产物上传、golden 仅 windows。验收：PR 上三 job 绿。
- [ ] 混合模式收口：补 `Multiply/Screen` 两条 pipeline，或从 `BlendMode` 移除并在文档标注——二选一，不留空分支。验收：golden 覆盖或枚举已删。
- [ ] 可选 `inspector`（egui 0.36）：sprite/batch/纹理页/帧时间面板。验收：feature 开关下 `check-all` 绿。
- 验收（整段）：CI 全绿 + 4 篇文档 → 提交 `P10 polish+ci`。

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
