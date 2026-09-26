# 从 RPG Maker 移植（T12 起草）

先划清状态：`crates/sisf-rpgmk` 现在只有一行模块文档 —— Map/Event 的 translate 是 **T11** 的活。
所以这篇文档是**接口地图 + 语义差清单**，不是"照抄就能跑"的流程。这里写到的每个引擎名字
都能在 `crates/` 里 grep 到；标 ⏸ T11 的才是没实现的，标 ❌ 的是**定案不做**的（原因写在原处，
不在这里重复）。

## 1. RGSS 对象在哪

| RGSS（XP/VX/VXA） | 引擎落点 | 状态 |
|---|---|---|
| `Bitmap` | `sisf_render::Bitmap`（图集页内的 rect + epoch），装载走 `Vfs` + `sisf-media` 解码链 | ✅ |
| `Sprite` | `sisf_sprite::Sprite` —— arena 里的**槽**，不是引用对象：`SpriteArena::spawn(..) -> SpriteRef`，`get`/`get_mut` 取回 | ✅ |
| `Viewport` | `ViewportRef` + `Layer` + `FrameCtx::push_viewport`（同一 arena 可被多个可视口各采一次） | ✅ |
| `Color(r,g,b,a)` | `Sprite::solid`（1×1 白图 `WHITE_EPOCH` 上色）与 `Tone::Offset{rgb,gray}` | ✅ |
| `Tone`（矩阵） | `Tone::Offset` ✅；`Tone::Matrix` ❌ 采集时返回 `CollectError::UnsupportedTone`（`docs/sprite.md`） | ❌ |
| `blend_type` | `Blend::{Normal,Add,Replace}`；`2/5`（Multiply/Screen）❌ 不移植，降级写法见 §4 | ❌ |
| `Graphics.frame_rate` | `AppConfig::tick_hz` + 每帧的 `alpha`（插值），帧序 `tick ×N → prepare → draw` | ✅ |
| `Audio.bgm/bgs/me/se_play` | `SoundChannel::{Bgm,Bgs,Me,Se}` + `AudioBus::play_bytes`；事件侧走 `AudioPort` | ✅ |
| `Input.repeat?/trigger?` | `EngineBus` → `ActionMap`（命名动作）→ `InputState` 的 Press/Hold/Release 边沿 | ✅ |
| `Interpreter` | `EventRunner`：`EventDef::from_ron → compile() → EventScript → start/tick(… , budget)` | ✅ |
| `Window` / `FileWindow` / 消息窗 | 只有 `UiPort::{show(Prompt),hide,set_dim}` 这三个把手；窗口控件本身 ⏸ T11 | ⏸ |
| `Game_Map` / 换图 / 事件页条件 | `Scene`（actor 表 + `walkable` 回调 + `tile` 网格）有了，地图数据与 translate ⏸ T11 | ⏸ |
| `Font`（`System.font_name`） | `sisf_text::FontBundle::from_bytes` / `register_bytes`，**字体文件由宿主提供**（§5） | ⚠️ |

## 2. 事件指令对照（MV/MZ「事件指令」→ `Command`）

`Command` 一共 17 个变体（`crates/sisf-event/src/command.rs`），够用到的这些：

| MV/MZ | `Command` | 备注 |
|---|---|---|
| 显示文字 / 显示选项 | `ShowText{speaker,body,choices,result,cursor_se,allow_cancel}` | 一个变体同时是"消息窗"和"选择窗"；`result` 不给就没法分支 |
| 数字输入 / 键入字符 | `InputKey{slot,timeout}` | 写的是按键的**十进制数值**；"只按下了关掉上一句的那一下"不算答案 |
| 等待 / 等待动画完成 | `Wait{frames}` / `MoveRoute{wait:true}` / `Animate{wait:true}` | 拍数，不是毫秒 |
| 移动路线 | `MoveRoute{actor,route,wait}` | 撞墙作废剩余格数，路线本身继续（RPG 惯例） |
| 播放 BGM/BGS/ME/SE | `PlayBgm/PlayBgs/PlayMe/PlaySe(Sound)` | BGM 顶掉 BGS 按混音器规则淡出 |
| 淡入淡出画面 / 音乐 | `Fade{target,frames,out}` | 拍→毫秒换算归端口 |
| 控制变量 / 控制开关 | `SetVar{slot,op,value}` + `VarStore::{set_switch,set_int,set_text}` | 变量、开关、文本三种槽位同一套 `Slot` |
| 条件分支 / 循环 / 中断循环 | `If{cond,then,else_}` / `Loop` / `BreakLoop` | 嵌套深度有上限（`CommandError`） |
| 跳转页 / 退出事件 | `Jump{page}` / `Erase` | `Jump` 就是 MV 的"执行内容→翻页"，换页后条件重新求值 |
| 公共事件 / 本事件 | 宿主调 `EventRunner::start_program(subject, prog)`（或 `start` 起某个事件的某一页） | **虚拟机里没有"调用另一个事件"这条指令**：`Instr::Run(Command)` 是编译后给每条叶子指令用的包装，不是可写的指令。嵌套就是压一帧，`MAX_FRAMES = 32` 封顶 |
| 图形/透明度/图块变更 | ⏸ T11（宿主直接改 `Sprite` 字段） | 这些在 RGSS 里也是脚本干的活 |
| 转移玩家 / 车辆 / 位置登记 | ⏸ T11 | 场景与传送归宿主地图层 |
| 商店处理 / 战斗处理 / 游戏结束 | ⏸ T11 | 需要 UI 与存档层，引擎不提供 |
| 脚本（Ruby / JS） | `Custom(name, args)` | **脚本不进这台虚拟机**：`ScriptBackend` 收到 `Custom` 自己解释；后端不认得该 `name` ⇒ 登记告警后继续，不静默 |

## 3. 存档与确定性

`VarStore::seeded(seed)` 里的 `Rng` 是**可复放的**（无全局状态、不进 `HashMap` 迭代顺序），
`Command` 全 `Serialize/Deserialize`（RON）。因此"事件页条件 + 变量 + 开关"这三样能被同一份
JSON/RON 复原；地图状态、精灵槽位、音频通道位置由宿主自己记账 —— 引擎不猜哪些精灵属于存档。

## 4. `blend_type` 2/5 的降级写法

RGSS 的 `Sprite#blend_type` 有 0 正常 / 1 加法 / 2 乘算（/5 屏幕）。引擎只留了 0/1 与
`Replace`，因为**一个混合变体对应一条 pipeline**，塞进枚举却没有管线就是"写得出、画不出"
（定案与完整理由在 `docs/sprite.md` 的「混合模式与矩阵调色的定案」）。移植时的三种写法：

1. **乘算变暗**：绝大多数 MV 用法是"阴影/暗幕"，用 `Add` + `opacity` 反着做不到，但用一层
   半透明黑 `Sprite::solid` + `Blend::Normal` 覆盖就是同一张画面的效果，而且合批只多一个实例。
2. **发光/高光**（原本 Screen 或 Add）：直接 `Blend::Add`，观感差异在过曝区；要防过曝就压 `opacity`。
3. **真要逐像素乘**：在 CPU 侧改写位图（把两层 `Bitmap` 相乘后重新上传，epoch 自然变），
   代价是一次读回 + 上传，只适合静态合成，不能每帧做。

`Tone`（矩阵调色）同理：`Tone::Offset{rgb,gray}` 覆盖 RGSS 的 `Color` 调子用法；矩阵那一套
（`tone.set(1, -0.5, …)` 之类的交叉通道）要的是 per-draw 20 元矩阵，与本阶段的 80B `Instance`
装不下、也不值得开一条 uniform 通路 ⇒ 需要时同样在 CPU 侧改写位图。

## 5. 宿主必须自备的三样

引擎**故意不带**，也不留空实现（DoD-4）：

- **字体**：`FontBundle::from_bytes` 成功即把该族设成 sans/serif 默认族，多份可叠加
  （`register_bytes`，拉丁 + CJK 各一份是常见做法）。仓库里那份 Inter 只有拉丁；中文要 CJK
  字形，而 8MB 的整包不入库 ⇒ **字形来源由宿主在运行期补**（wasm 侧示例从 CDN `fetch` 一份
  Noto Sans SC 再注册，见 `docs/text.md` 与 `todo.md` 的 T12 记账）。没有字体时文本引擎走
  `fallback()`，画不出字但不 panic。
- **SoundFont**：`midi` feature 只解决"能不能解 `.mid`"，音色库要用户自己的 `.sf2`
  （`SoundBank::from_sf2(&[u8])`，字节建议经 `Vfs`）。没有 SF2 ⇒ `MissingSoundBank` 报错 + 提示，
  不 panic。离线退路（`fluidsynth` 预渲染成带 LOOP 元数据的 `.ogg`）写在 `docs/audio.md`。
- **素材容器与解密**：RGSSAD（XP/VX 打包）、RPGRT/MV 的加密图片、以及 `.zip` 归档 —— 引擎侧
  的 `Vfs` 只认 `DirProvider` 这类明文目录，**解密不做**（授权素材的密钥归项目）。
  接法是把解出来的字节再喂给一个 provider；实装排在 T11 之后，本轮只登记待办。

## 6. 会咬人的语义差

移植时最先撞上这几条，都已在别处钉成单测，这里只列方向：

1. **`z` 不是全局画序**。合批键是 `(layer, clip, epoch, blend, z, insertion)`，`z` 只在同批次内
   说话 ⇒ HUD 想盖住相机层的地图，得推一层更大的 `layer`，不是加大 `z`（`docs/platforms.md`）。
2. **锚点同时是旋转轴心**，且**翻转是 `size` 的符号**（`docs/sprite.md`）。RGSS 的 `ox/oy` 对得上
   锚点，但 `mirror_x` 与 `angle` 的相互作用顺序不同，绕一转就跑到别处。
3. **拍频与毫秒不要混**。`Wait{frames}`、`Fade{frames}` 全是拍；`AppConfig::tick_hz` 一改，
   同样的事件脚本就变快变慢，这正是 RGSS `Graphics.frame_rate` 的语义。
4. **亚像素插值**：RGSS 每帧 `update` 都在整像素上；引擎有 `alpha`，跟着 `interpolated(alpha)`
   走的相机会把地图抖出呼吸感。像素美术的相机要把插值折回格点 —— 示例里那两个把手
   `lattice_alpha` / `snap_to_device`（`examples/tiled_map.rs`，**引擎不带**：它是玩法选择不是
   渲染职责）；它的前因后果与逐帧判据都在 `docs/platforms.md` 的 T8 复盘第 2/3/4 步。
5. **BGM 循环点**读的是容器元数据（OGG 的 `LOOPSTART`/`LOOPEND`、MP3 的 `LOOP`、MIDI 的内部
   循环），所以"能不能无缝循环"由素材决定，不由引擎决定（`docs/audio.md`）。
6. **手柄只在桌面三端**：wasm 要 JS 侧轮询 `navigator.getGamepads()`、Android 上 gilrs 0.11
   没有后端，所以 `native` 里的 `gilrs` feature 与两层 `cfg` 必须一起判（`docs/input.md`）。

## 7. 起手式

1. 素材目录 → `Vfs` + `DirProvider`（`crates/sisf/examples/` 里的示例都是这么读的）。
2. `AppConfig::virtual_size` 定成项目分辨率（MV/MZ 默认 816×624，VXAce 544×416）。
3. 角色表 → `SpriteArena` 槽位 + `Scene::set(actor_id, sprite_ref)`；`actor` 编号由此和事件脚本对齐。
4. 事件 → RON（`examples/assets/events/*.ron` 是形状参考）→ `EventDef::from_ron` → `compile()`。
5. `AudioPort`/`UiPort` 各一个实现对象 —— 两者是**不同**的 `&mut` 借用，合成一个对象会 borrow-check 失败。
6. 地图数据、事件页条件、存档：⏸ T11（`sisf-rpgmk`）。
