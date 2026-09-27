---
name: sisf-assets-audio
description: SISF 资产管线（VFS 注册、AssetStore 异步装载、图集上传）与音频（AudioBus 四通道、pump 节奏）。当要加载图片/字体/音频、排查"没声/黑屏/资源不出现"或写 wasm 资源清单时使用。
---

# SISF 资产与音频

## VFS：路径与 provider

- `VfsPath::new(&str) -> Result<Self, VfsPathError>`（path.rs:60）：统一 `/`、大小写不敏感查找键、
  内部 `..` 消解；**越根 `..`、绝对路径、盘符、空串、NUL 在构造期就失败**。
  常量路径可用 panic 版 `vfs::path(input)`。
- `Vfs::new()` + `register(Box<dyn VfsProvider>)`（vfs.rs:51）：**后注册者优先 = 补丁目录覆盖 base**。
- provider：
  - `DirProvider::new(root)` —— 桌面（native only）。
  - `MemoryProvider` —— 内嵌资源/单测。
  - `WebManifest::new(一行一路径的清单)` —— wasm（feature `web`）：宿主 JS `fetch` 完成后
    `insert(&VfsPath, Vec<u8>)`；`pending()` 给出还没到的路径供批量 fetch。
    **清单未就绪的路径表现为"不存在"，应用下一帧重试**（vfs.rs:10-12）。
  - Zip/Rgssad 只有接口登记，未实装——再实现一个 `VfsProvider` 即可，不改核心。

## 游戏侧典型序列：路径 → Handle<Bitmap>

铁律：**解码可异步，GPU 上传恒在主线程**（asset/tasks.rs:3-6）。以 sprite_bench.rs 为准：

1. 建 Vfs：桌面 `register(DirProvider)`；wasm `Arc<WebManifest>` + `spawn_local` 逐个
   fetch→insert，再包 `SharedManifest` provider 注册（sprite_bench.rs:137-152, 259-285）。
2. 每帧 pump：对未就绪源 `store.load(&vfs, &mut runner, &path, decode_fn)` →
   `Request::{Ready, Loading(Ticket), Failed(Arc<str>)}`（store.rs:63-67）。
   **先 `vfs.exists(&path)` 再投**——清单没到货就投，空字节会被 decode 报错并**缓存成长期
   失败，不逐帧重试**（要 `store.retry(&path)` 清掉，store.rs:275）。
3. 解码闭包（跑在 worker 线程，只能捕获 `Send` 的东西）：
   `decode_image(&bytes)? → data.first() → atlas.new_bitmap(w, h, 预乘RGBA8, now_frame)`。
   同路径在飞共享同一 Ticket，绝不二次解码。
4. 每帧一次 `store.poll_ready(&mut runner)` 收割，然后**统一从 `store.get(&path)` 领句柄**
   存进自己的 vec（持有 Handle = 页不被 LRU 驱逐；全 Drop 后 generation+1 重解码）。
5. 主线程：`uploads.queue(bitmap.clone())` → 每帧
   `renderer.apply_atlas(&atlas, &mut pages, &mut uploads)`（atlas_gpu.rs:585）——
   这是布局事件生效的唯一同步点。采样用 `handle.get().uv_rect()/epoch()`。

`load_blocking(path, f)` 用于启动期同步装载，不过 runner（store.rs:236）。
页桶下限 256²：边长 ≤256 的图共用一页；`new_large_bitmap` 走大图旁路。

## 音频：应用侧只碰 AudioBus

- 打开：`AudioBus::open()`（**永不失败**，无设备走静音混音，bus.rs:41）或 `headless()`。
  两个 pump feature 都没开时只有 `Mixer`、没有 AudioBus——刻意设计。
- 播放：`play(channel, Arc<AudioData>, &PlayParams)`（bus.rs:73）、`play_bytes`（83，
  **在调用线程整段解码**）、`play_stream`（94）、`play_midi`（feature `midi`；
  **引擎不带 SoundFont**，bank=None 报 `MissingSoundBank`，`offset_frames` 只接受 0）。
- 控制：`stop(ch)`、`fade(ch, ms)`（淡到 0 即停 = RGSS `bgm_stop(fade_length)`）、
  `fade_to`、`pause/resume`、`set_volume(ch)`、`set_master_gain`、`suspend()/resume_all()`。
- 通道语义（mixer.rs）：`Bgm/Bgs/Me` 各一路独占、`Se` 16 路池超出抢最旧；
  **只有 Bgm/Bgs 默认循环，Me/Se 播完即止**。`PlayParams` builder：
  `.gain/.pan/.pitch/.offset/.looping/.fade_in`（looping=None 跟随通道）。
- 换 BGM 直接再 `play`（旧路自动交叉淡化）；切场景用 `bus.fade(Bgm, ms)`。

## pump 节奏（wasm 出声的关键）

**每次 `App::tick` 调一次 `bus.pump()`**（bus.rs:4-7）——tick 停 = 声音停，这正是挂起语义；
资源未齐、挂起期间也要泵（audio_demo.rs:498-512）。wasm 侧 pump 问 JS 还差多少帧、只渲
那个量；自动解锁/visibilitychange 挂起在 `audio_pump.js` 胶水里——
**静态服务必须连 `web/pkg/snippets/` 一起给**（pump_web.rs:3-7）。

## 已知坑

- **别在播放路径上调 `play_bytes`**：同步解码可达 ~90ms，会抽干 ≈21ms 输出环 ⇒ 爆音。
  正确形状（audio_demo.rs:100-107）：`Sources` 读一次解一次，复用 `Arc<AudioData>`。
- `decode_audio`/`play_bytes` 缺 feature 时明确报 `UnsupportedFormat`，不是静音——
  "看着动、听着没声"先查 feature（`cargo xtask web` 用 `web,ogg,mp3,wav,midi` 组合，
  但 `--features web` 单开时全部解码都是 UnsupportedFormat）。
- WAV 无 `smpl/cue` 循环点（精确循环用 OGG 元数据 `LOOPSTART/LOOPEND`）；只支持 1/2ch；无 3D 音。
- 循环点单位 = **源帧**；非法循环点夹取降级、不 panic。
- wasm 无文件系统：所有字节走 WebManifest fetch 或 `MemoryProvider` 内嵌。
