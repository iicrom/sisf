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
- [ ] `sisf-app/src/config.rs`：`AppConfig{ title, virtual_size:(u32,u32), scale_mode: Fit|Fill|IntegerFit, letterbox: Rgba, tick_hz: f64 = 60.0, clear: Rgba }`。验收：默认 544×416 可构造。
- [ ] `sisf-app/src/app.rs`：`trait App { fn on_event(&mut,self,&[EngineEvent],&mut FrameCtx); fn tick(&mut,self,dt:Duration,&mut FrameCtx); fn draw(&mut,self,alpha:f32,&mut Renderer); }`。验收：签名不跨帧持有 wgpu 借用。
- [ ] `platform/native.rs`：winit 0.30 `EventLoop` + `ActiveEventLoop`；wgpu `Instance`（`Backends::all()`）+ `SurfaceTarget` + `Surface::configure`；`SurfaceError::Lost|Outdated` → 重配。验收：Windows 出窗口并清成 `clear` 色。
- [ ] `platform/wasm.rs` + `web/{index.html,main.js}`：wasm 下 winit 无 `run()`，用 `Canvas` + `requestAnimationFrame` 闭包驱动同一个 `frame()`。验收：`cargo xtask web` 起本地服务后 canvas 有色；WebGPU 与 WebGL2（`?wgl`）各测一次。
- [ ] `platform/android.rs`：`android-activity` 0.6 配置 + `_native_activity` manifest metadata；`Resumed` 建 surface、`Suspended` 释放 `Surface` 与 GPU 资源。验收：android 端 check 绿，`docs/android.md` 写出构建/安装步骤。
- [ ] 生命周期：`Lifecycle{Resumed,Suspended,Focused(bool),CloseRequested}` 上报 `App::on_event`；`Suspended` 冻结累加器；`Resized`/`scale_factor` → 重算投影与输入坐标映射。验收：Win 最小化→恢复不 panic、不丢 surface。
- [ ] iOS：target 未安装 → 仅在 `docs/platforms.md` 记录启用步骤（`rustup target add aarch64-apple-ios` + Xcode），代码复用 `platform/native.rs`。验收：文档段落存在。
- 验收（整段）：三端 check + Windows 手测 → 提交 `P1b app loop+lifecycle`。

## T3 批量 quad 渲染器
- [ ] `sisf-render/src/vertex.rs`：`#[repr(C)] Instance{ pos:Vec2, size:Vec2, uv0:Vec2, uv1:Vec2, color:[f32;4], rot:f32, tone:[f32;4], _pad:[f32;2] }`（64B）+ 单位 quad 顶点/索引缓冲。验收：`const _: () = assert!(size_of::<Instance>()==64)` + 字段偏移单测。
- [ ] `shaders/sprite.wgsl`：group0 `FrameU{ proj: [[f32;4];8], time }`（viewport 栈）；group1 `tex` + `sampler`；VS 做 anchor/rotation/scale，flip 用 uv0/uv1 交换实现；FS 做预乘 alpha + tone offset/gray。验收：三端编译无 naga 报错。
- [ ] `renderer.rs`：`Renderer::{new, begin_frame, end_frame}`；两条 pipeline（alpha-over、additive）+ `debug_lines` feature 下 clone 一条 `polygon_mode: Line`。验收：同一 `RenderPass` 内切换 pipeline 正常。
- [ ] 实例缓冲：单 `Buffer`（`MAP_WRITE|VERTEX`）环形子分配，容量 16384 quad，写满先 flush 再续，容量可配。验收：wrap-around 与强制 flush 单测。
- [ ] batch 分组：排序键 `(layer:u8, texture_epoch:u64, blend:u8, z:i32, insertion:u32)`，相邻同键合并为一次 `set_bind_group`+`draw`。验收：`batcher.rs` 单测断言 batch 数与提交顺序。
- [ ] `FrameCtx::push_viewport/pop_viewport`（深度 8，越界 `debug_assert`）。验收：8 层嵌套单测。
- [ ] `examples/sprite_bench.rs`：10 万 sprite 随机变换 + 帧时间打印。验收：桌面 ≥60fps（实测值记入 `docs/bench.md`），wasm `?wgl` 可跑。
- 验收（整段）：单测 + bench 手测 → 提交 `P2 batched quad renderer`。

## T4 VFS + 图集 + 图像解码 + GPU 上传
- [ ] `sisf-asset/src/path.rs`：`VfsPath` 规范化（统一 `/`、查找键大小写不敏感、拒绝 `..` 越界）。验收：路径穿越单测。
- [ ] `vfs.rs`：`trait VfsProvider: Send { fn read(&self,&VfsPath)->Option<Vec<u8>>; fn exists(&self,&VfsPath)->bool; }`；`DirProvider`（native `std::fs`；wasm 走预置清单 + fetch）；`Vfs{ providers }` 后注册优先。验收：补丁目录覆盖同名文件；zip/RGSSAD 仅需接口在文档登记。
- [ ] `store.rs`：`AssetId(u32 hash, u32 gen)`、`Request<T>{Ready(Handle<T>),Loading(Ticket),Failed(Arc<str>)}`、`AssetStore<T>`（`HashMap` + `Arc`/`Weak` refcount + generation）。验收：句柄 Drop 后可重载；同路径不重复解码。
- [ ] `tasks.rs`：`trait TaskRunner`；`NativeTasks`（`std::thread` + `mpsc`，GPU 上传恒在主线程）与 `WasmTasks`（`wasm-bindgen-futures` + `fetch`，`poll_ready()` 主循环收割）；`load_blocking` 两端语义（join / 协作式 pump）。验收：非阻塞 `load` 不使单帧耗时超过一帧。
- [ ] `sisf-render/src/atlas.rs`：分桶 `Rgba8UnormSrgb` 图集（起始 2048²）+ `rect-packer` 装箱 + `epoch` 参与 batch key + 按最后使用帧 LRU 驱逐 + 满页换新页（`max_pages=8` 后强制 flush）+ `Bitmap::large()` 旁路。验收：装箱/驱逐/换页单测。
- [ ] 上传：每帧单一 staging ring，`copy_texture_from_buffer` 前按 `bytes_per_row` 256 对齐；sRGB 交给格式不做手动 gamma。验收：17×13 这类非对齐尺寸上传后与 CPU 侧像素一致。
- [ ] `sisf-media/src/image.rs`：`decode_image(&[u8])->Result<ImageData>`，按魔数分派 `png 0.18` / `jpeg-decoder 0.3.2`（`simd-jpeg` 下换 native-only `turbojpeg`）/ `image-webp 0.2.4` / 手写 BMP（24/32bpp、bottom-up、位掩码）/ `gif 0.14`（feature，多帧→`AnimDef`）；输出统一 RGBA8 预乘。验收：各格式 fixture 解码正确（`tests/fixtures/`），wasm 下 `simd-jpeg` 关闭。
- [ ] 改造 `examples/sprite_bench.rs` 走 VFS + 图集 + 混图源。验收：混合 PNG/JPG/WebP/BMP 时打印的 batch 数符合断言。
- 验收（整段）：VFS/atlas/image 单测 + 桌面与 wasm 加载手测 → 提交 `P3 vfs+atlas+image`。

## T5 Sprite arena + 变换 / blend / tone
- [ ] `sisf-sprite/src/sprite.rs`：`Sprite{ bitmap, src, anchor, pos, scale, rotation, opacity, tone, flip_x, flip_y, z, visible, blend, anim }`；`Tone{Offset{rgb,gray}, Matrix([f32;20])}`；`BlendMode{Normal,Add,Multiply,Screen,None}`。验收：`Default` 与字段 setter 编译通过。
- [ ] `arena.rs`：`SpriteArena{ slots, free:Vec<u32>, gen:u32 }` + `SpriteRef(u32,u32)` + `spawn/despawn/get/get_mut/iter_live`。验收：slot 复用后旧句柄因 generation 失效（单测）。
- [ ] `draw.rs`：`collect(&self,&mut BatchList)` — 过滤不可见/全透明、按 `(layer,z)` 排序、`src`+atlas 合成 uv（含 flip）、rotation 入 instance。验收：anchor/flip/z 排序单测。
- [ ] `Tone::Matrix` 本阶段明确标注不支持并返回错误（不写空实现分支）；`Multiply/Screen` 同 T12 决策。验收：文档 + 单测断言错误路径。
- [ ] 多 viewport/layer：`Layer(u8)` + `ViewportRef`，同一 arena 可被两个 viewport 采集（RPG Maker 双层视口 / SLG 小地图）。验收：单测两 viewport 输出独立。
- [ ] `tests/golden_sprite.rs`（`#[cfg(windows)]`）：离屏 544×416，DX12/Vulkan adapter + `COPY_DST` 读回，覆盖 opacity=0.5 / tone offset / gray / flip_x,y / anchor(0.5,1) / additive，逐通道容差 ≤1e-3。验收：`cargo xtask golden` 绿，图落 `tests/golden/`（首次 `--bless`）。
- 验收（整段）：golden + arena 单测 → 提交 `P4 sprite arena+tone/blend`。

## T6 文本 + Bitmap 渲染目标 + clip stack
- [ ] `sisf-text/src/lib.rs`：`FontAtlas`（`R8Unorm` 子图集 + `etagere`）+ `layout(text,style)->TextRun`（`cosmic-text 0.19`，shaping/bidi/换行，wasm ✓），字形最终输出为普通 sprite quad、不新增 pipeline。验收：中英混排 + 自动换行；字号 12/16/24 golden。
- [ ] `FontBundle::from_bytes(&[u8])` 经 VFS 载入 ttf/otf（MV/MZ 自带字体），缺失时回退默认并只 warn 一次。验收：两条路径单测。
- [ ] `sisf-render/src/bitmap.rs`：`enum BitmapData{ Atlas{page,region}, Rt{texture,view} }` + `BitmapHandle(Arc<Bitmap>)` + `Bitmap::new(w,h)` + `begin_draw/end_draw`（RT 作嵌套 pass 目标）。验收：RT→RT quad 拷贝 golden 正确。
- [ ] RPG Maker `Bitmap` 风格 API：`blt(src,src_rect,dst_pos,opacity)`、`fill_rect`、`clear_rect`、`draw_text(rect,text,align)`。验收：`examples/windows.rs` 全部用到。
- [ ] clip stack：`push_clip/pop_clip` → `scissor_rect`，clip 变更即 flush batch。验收：单测断言 flush 次数与栈深。
- [ ] `examples/windows.rs`：RPG 消息窗（半透明底 + 9 段边框 blt + 裁剪文字 + 头像 blt）。验收：桌面 + wasm 手测。
- 验收（整段）：文本 golden + 窗体 demo → 提交 `P5 text+bitmap-rt+clip`。

## T7 音频混音器 + OGG/MP3/WAV + MIDI(SF2)
- [ ] `sisf-audio/src/mixer.rs`（纯 Rust，不碰设备）：`Voice{data,pos,gain,pan,loop_start,loop_end,rate}`、`render(&mut,self,out:&mut [f32])` 交织立体声；通道 `Bgm/Bgs/Me/Se`（Se 多路复用 + voice 上限抢占）、`fade`、`play/stop/pause/resume/suspend`。验收：`render` 确定性单测（四端可跑，无设备）。
- [ ] `sisf-media/src/audio.rs`：`trait Decoder{ channels, sample_rate, read, seek_sample }`，`symphonia 0.6.1 features=["ogg","vorbis","mp3","wav"]`（纯 Rust，wasm ✓）；`lewton 0.10.2` 仅作 `ogg-lewton` 回退 feature（默认关）。验收：OGG/MP3/WAV fixture 采样长度断言。
- [ ] 循环点：`lofty 0.25.4` 读元数据 → `AudioData{loop_start_samples,loop_end_samples}`。验收：带 loop 的 OGG 无缝循环（samples 级断言 + 听测）。
- [ ] `pump_native.rs`：`cpal 0.18.2` 回调内直接 `Mixer::render`（混音状态由主线程改、回调读快照/双缓冲，不加锁）；设备变更与 `StreamError` 重建流。验收：Win 播放无爆音，插拔耳机不断流。
- [ ] `pump_web.rs` + `web/audio_worklet.js`：`AudioWorklet`（不可用则 `ScriptProcessor`）从 WASM 拉混音输出；`AudioBus::suspend/resume` 绑 visibilitychange + 首次点击解锁。验收：Chrome-wasm 播 BGM/SE，`docs/audio-web.md` 记延迟实测（目标 ≤~21ms@1024）。
- [ ] `sisf-media/src/midi.rs`（feature `midi`）：`midly 0.5.3` 解析 + `rustysynth 1.3.6` SF2 合成，`SoundBank::from_sf2(&[u8])` 经 VFS（**引擎不附带 SoundFont**），MIDI 作为一路 Voice 参与同一 Mixer。验收：给定用户 SF2 时播放 fixture `.mid`；无 SF2 返回 `MissingSoundBank` 并提示（不 panic）；文档给出"离线把 .mid 预渲染成 .ogg"退路。
- [ ] `examples/audio_demo.rs`：BGM 循环 + `Space` 播 SE + `M` 播 `.mid`。验收：`cargo xtask check-all` 含 `--features midi` 组合绿。
- 验收（整段）：mixer 单测 + Win/Chrome 听测 → 提交 `P6 audio+ogg/mp3/wav+midi`。

## T8 动画 + MoveRoute + 固定 tick
- [ ] `sisf-sprite/src/anim.rs`：`AnimDef{ frames:[FrameDef{rect,dur_ms}], loop_: Once|Pingpong|Loop{n}, pattern: Option<DirPattern> }`、`AnimState{def,speed,index,paused}`、`DirPattern::RpgMaker4x3`（MV，`dir + 3*step`）/ `RpgMaker2x2`（VX/Ace）。验收：两图样帧索引单测（下/左/右/上四向）。
- [ ] 步帧由移动驱动（RPG Maker 语义）：`set_step(u8)`，静止时不推进帧。验收：单测静止不换帧。
- [ ] `sisf-app/src/timing.rs`：固定 tick + 累加器（补帧上限 5）+ `draw(alpha)` 插值；`Suspended` 冻结累加器。验收：dt 尖峰与补帧上限单测。
- [ ] `sisf-sprite/src/task.rs`：`schedule/tick` 任务调度器，承载 `Tween<T>` 关键帧与 MoveRoute。验收：Tween 完成回调按 tick 计数。
- [ ] `MoveRoute{ nodes:[RouteNode], speed, through }`：`Wait/Turn/Move(dir,n)/MoveTo/Jump/FacingLock`，网格（RPG）与自由（SLG）双模式。验收：逐格移动 + `wait:true` 存活至完成。
- [ ] 虚拟分辨率映射：`Fit|Fill|IntegerFit` + letterbox，指针/触摸坐标自动映射虚拟空间。验收：整数缩放无半像素抖动、点击命中正确。
- [ ] `examples/tiled_map.rs`：三层 tilemap（地面/事件/上方，图块 32/48）+ 键盘 4 向行走 + 事件格碰撞。验收：桌面 IntegerFit 544×416 + wasm 手测。
- 验收（整段）：anim/route/timing 单测 + tiled_map → 提交 `P7 anim+route+tick`。

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
