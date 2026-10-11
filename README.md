# SISF

基于 wgpu 的跨平台 2D 引擎（Rust）。语义取向是 RPG Maker 那一类：固定步长逻辑 + 插值渲染、
批量 quad 精灵、位图文字与裁剪栈、磁盘上的 RON 事件表跑在一台小虚拟机里。三端共享同一个
`Runner`：Windows/桌面、浏览器（wasm32，WebGPU 与 WebGL2 两条后端）、Android。

- 版本 `0.1.0`，edition 2024，`rust-version = 1.85`（fmt/clippy 的判定标准由 nightly 钉，见下）
- 许可：`MIT OR Apache-2.0`；第三方素材各自带许可证（Inter 为 SIL OFL 1.1）
- 规模：11 个 crate + `xtask` + 10 个示例，约 6 万行 Rust；`crates/sisf-rpgmkmv/src/js/` 另有
  40 份 JS（约 2.1 万行），其中上游 `rpg_*.js` 的逐字切片与自创 shim 两类各有名单（数它们的尺子
  在移植侧 `../sisf_pray/tools/probes/objects-verbatim.py`）

## 能力

| 方向 | 把手 | 说明 |
| --- | --- | --- |
| 应用与帧序 | `App::{on_event, tick, prepare, draw}`、`AppConfig` | 固定 `tick_hz` 累加器 + `MAX_CATCHUP`；`prepare` 是投影烘焙前的相机对齐相 |
| 渲染 | `Renderer`、`SpriteArena`、`BatchList` | 单条管线服务 8 层 viewport；合批键 `(layer, clip, epoch, blend, z, insertion)`；实例缓冲环形子分配 |
| 文字 | `TextEngine`（cosmic-text）、位图渲染目标、clip 栈 | 文字走同一条 sprite 管线；9-slice 消息窗与滚动裁剪是示例级验收 |
| 资产 | `Vfs` + `VfsProvider`、`AssetStore<T>`、图集/上传环 | 路径构造期拒绝越根/绝对/盘符/NUL；PNG/JPG/WebP/BMP/GIF 解码，句柄引用计数 + generation |
| 音频 | `AudioBus` 四通道（BGM/BGS/ME/SE）、淡入淡出、声相 | OGG/MP3/WAV 走 symphonia，MIDI 走 midly + rustysynth（SoundFont 由宿主自备）；wasm 侧 pump 在 JS 里 |
| 输入与事件 | `ActionMap` 命名动作、`EngineEvent` 总线、`EventRunner` | 设备事件只翻译一次；控制流留在虚拟机里，脚本只能"再启动一段程序"，所以存档只要 `VarStore` + 页号 |
| MV 兼容层 | `sisf-rpgmk`（翻译期）、`sisf-rpgmkmv`（运行期，`js` feature） | 前者把 `data/*.json` 的事件码表折成 `Command`；后者顶替整套 `rpg_*.js`（逐字切片 + 自创 shim 两类各有名单），让 `js/plugins/` 里的上游脚本原样跑。两条线都不进门面那一侧的默认 feature，逐格账在移植侧 |
| 平台 | native / wasm / android | 挂起时冻结累加器并忘掉按住的键；Android 上还释放 surface 与 GPU 资源；不支持的能力一律 feature 关掉，不留空实现 |

## 快速开始

```sh
# 桌面：任一示例
cargo run -p sisf --example hello_window
cargo run --release -p sisf --example sprite_bench   # 压测请带 --release

# 浏览器：构建 wasm + 起本地静态服务（缺省示例 hello_window）
cargo xtask web dialog_demo
# 访问 http://127.0.0.1:8080/?ex=dialog_demo        （加 &wgl 强制走 WebGL2）

# 闸门
cargo xtask fmt --check
cargo xtask clippy
cargo xtask check-all
cargo test --workspace --all-features
cargo xtask wasm-test
cargo xtask golden            # 离屏像素基线，--bless 重写
```

工具链：本仓库的 fmt 与 clippy 判定标准出自 nightly（本机 `1.95.0-nightly`），CI 因此也钉
nightly —— 换到 stable 时红的可能只是工具版本。Linux 上编译 `--all-features` 需要
`libasound2-dev`（cpal）与 `libudev-dev`（gilrs）。Android 真机构建需要 NDK + `cargo-ndk`，
步骤与手测清单在 `docs/android.md`。

## 结构

```
crates/
  sisf          门面：feature 矩阵 + 模块 re-export，示例都挂在这里
  sisf-app      Runner / App trait / AppConfig / 三端 platform / 生命周期
  sisf-render   批量 quad 渲染器、图集、上传环、位图渲染目标、clip 栈、viewport 栈、WGSL 管线
  sisf-sprite   Sprite 语义、arena、动画、MoveRoute/Tween、任务调度
  sisf-text     cosmic-text 排版、字形覆盖率页（R8Unorm）、blt / sync 通路
  sisf-event    EngineEvent 总线、ActionMap、指令表、EventRunner、VarStore
  sisf-asset    VfsPath / Vfs / AssetStore
  sisf-media    图像与音频解码（按 feature 分链）
  sisf-audio    mixer（纯逻辑）+ cpal / WebAudio 两条 pump
  sisf-rpgmk    RPG Maker MV/MZ 的**翻译期**那半（T11）：`data/*.json` → `Command`、三张 `Game_*` 表、最小 `Scene_Map`、`.zip` provider
  sisf-rpgmkmv  RPG Maker MV 的**运行期**那半（M10 兼容层）：`rpg_*.js` 的逐字顶替、JS 宿主两条腿、位图与软件合成、存档编解码
xtask/          仓库统一构建与验证入口
examples/       10 个示例，兼作各阶段的验收物
web/            wasm 页面的 index.html / main.js / 预取清单
docs/           模块文档
todo.md         T0–T12 实施清单 + 每阶段的验收证据与"发现并修正的事实"（M 系列的引擎侧补账在文末）
```

## 示例

| 示例 | 验的是 |
| --- | --- |
| `hello_window` | 三端共享 `Runner`、清屏与 surface 生命周期 |
| `sprite_bench` | VFS → 图集 → 上传环 + 10 万 sprite 的合批断言与帧时间 |
| `windows` | RPG 消息窗：9 段边框 blt、裁剪滚动文字、头像、半透明底 |
| `audio_demo` | 12 秒时间线跑完四通道 + OGG/MP3/WAV + MIDI(SF2) + 淡入淡出 |
| `tiled_map` | 三层 tilemap、命名动作四向行走、事件格碰撞 |
| `platformer` | 自由移动通路：逐拍整数像素速度积分 + 瓦片 AABB + 火球 |
| `dialog_demo` | 三份磁盘 RON → 事件虚拟机 → 消息窗/路线/BGM/淡入淡出 |
| `rpg_demo` | 组合：事件机管地图演出，菜单与回合战斗是纯游戏侧 FSM |
| `vn_demo` | 组合：演出全在内联 RON 里，游戏侧只吃 `ok` 与选项光标 |
| `rpgmv_shell` | T11 的移植可行性证明点：读一个散文件 MV 工程（`data/*.json` + `graphics/` + `audio/`），进图、走动、触发事件、播 BGM，影片指令 `261` 走显式跳过 |

桌面运行 `cargo run -p sisf --example <名>`（默认 `native` 已含 `image`；`audio_demo` 与
`dialog_demo` 还要 `--features ogg,mp3,wav`），wasm 运行 `cargo xtask web <名>`。其中七个带
`test = true`，验收跑的是示例自带的单测（地图形状与 z 序、剧本连通、事件表编得过、四条解码链各用一发）。

## 文档

- `docs/platforms.md` — 平台支持矩阵、三端 surface 与后端差异
- `docs/sprite.md` — sprite 语义与能力矩阵
- `docs/text.md` — 文字 + 位图渲染目标 + clip 栈
- `docs/bench.md` — 批量 quad 渲染器压测方法与实测数字
- `docs/audio.md` / `docs/audio-web.md` — 混音器与解码链；wasm pump 的 JS 侧记账
- `docs/input.md` — 事件总线与动作映射
- `docs/events.md` — 事件解释器：指令表、挂起与端口契约
- `docs/android.md` — Android 构建、安装与真机清单
- `docs/porting.md` — 从 RPG Maker 移植的接口地图与语义差清单

## 状态

T0–T12 已收口（`todo.md` 逐条带验收证据），其后的 M 系列（`sisf-rpgmkmv` MV 兼容层）也已跑通并
接进闸门 —— 它的**逐格账不住在本仓库**，见下。不要从 README 读出"全做完了"，还开着的是这四件：

1. **CI 从没在 GitHub 上跑过一次** —— `origin` 已经挂上（`git remote get-url origin` =
   `git@github.com:/iicrom/sisf`，那串里多出来的 `:` 是远端地址的原样），但本地 `main` 一直领先
   `origin/main` 未推送（2026-10-08 补账时数到 6 个，加上那轮的两次提交是 8 个；下一轮照
   `git rev-list --count origin/main..main` 现数），`.github/workflows/ci.yml` 的每条命令只在**本机**绿。
2. **Android 真机清单一条未执行**（`docs/android.md` 第 6 节，本机 0 设备无 NDK）。
3. **`cargo xtask golden` 只在 Windows 是真比对** —— 基线出自 Windows/RTX；在 macOS 上
   `sprite_scene_golden` 会因跨后端取整红在单通道误差 2（>TOL 1），那是**环境决定的红、不许 `--bless`**
   （读数与复现见 `todo.md`「T11 续 期间发现并修正的事实」第 4 条）。非 Windows 上整段
   `cargo test --workspace --all-features` 会被同一条顶成 exit 101，要加 `-- --skip golden`。
4. **兼容层的两处退役只记账未动手** —— `sisf-event::js` 的 façade 与 `sisf-rpgmk::translate`
   是给"不跑 JS 的工程"留的路，动它是本仓库六道闸门的事（定案在 `../sisf_pray/M10.md`）。

M 系列的路线、拆解（M10.0–M10.7）与逐格验收证据在**移植侧仓库**的 `../sisf_pray/M10.md`；
`crates/sisf-rpgmkmv` 的注释里那 115 处 `M10.md` 引用的就是那一份。本仓库这一侧只补记引擎侧落点与
当前闸门读数（`todo.md` 末尾「M 系列补账」）。

wasm 侧的中文字形由宿主在运行期从 CDN 取（约 8 MB 的 Noto Sans SC，不入库、不在预取清单），
取不到就显式报错并退回方块 —— 字形来源归宿主，不归引擎。
