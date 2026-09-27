# 从 RPG Maker 移植

先划清状态：`crates/sisf-rpgmk` 已经实装了 MV 工程的**读入侧**（§8）—— `data/*.json` 的
宽松反序列化、事件码表 → `Command` 的翻译、三张 `Game_*` 表、最小 `Scene_Map`、`.zip` 工程包
provider（§9）。这篇文档 T12 起草时它们还只是计划，所以 §1/§2 的状态列已经按 T11 的实际
形状改过。仍然没有的是 UI 控件层与解密（各自标了 ❌ / 待建）。
所以这篇文档是**接口地图 + 语义差清单**，不是"照抄就能跑"的流程。这里写到的每个引擎名字
都能在 `crates/` 里 grep 到；标 ⏸ 的是没实现的，标 ❌ 的是**定案不做**的（原因写在原处，
不在这里重复），标"待建"的是 §9 那张插槽清单。

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
| `Window` / `FileWindow` / 消息窗 | 只有 `UiPort::{show(Prompt),hide,set_dim}` 这三个把手；窗口控件本身 ❌ 不做 —— 菜单/商店/窗口皮肤和战斗一样属于游戏侧 FSM（`examples/rpg_demo.rs` 那条路） | ❌ |
| `Game_Map` / 换图 / 事件页条件 | ✅ T11：`Scene_Map`（`enter` → `refresh` → `tick`）+ `MapDef`/`TilesetDef`/`PageCondition::meets`；换图与场景栈仍归宿主（`examples/rpgmv_shell.rs` 的写法） | ✅ |
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
| 图形/透明度/图块变更 | ⏸ 未建模 ⇒ 整块降级成 `Custom("rpgmv.unimplemented", …)` 并计数；宿主也可以直接改 `Sprite` 字段 | 这些在 RGSS 里也是脚本干的活 |
| 转移玩家 / 车辆 / 位置登记 | `201` → `Custom("rpgmv.transfer", [6 个参数])`；其余同类码 ⏸ 未建模 | **场景栈归宿主**：`examples/rpgmv_shell.rs` 收到 `rpgmv.transfer` 才换 `Scene_Map` |
| 商店处理 / 战斗处理 / 游戏结束 | ⏸ 未建模 → 降级 + **整块丢掉缩进体**（计数在 `TranslateReport::dropped_branch_lines`） | 需要 UI 与存档层，引擎不提供；"把 `then` 提到外层"会直接改剧情，所以宁可丢 |
| 播放影片 | `261` → `Custom("rpgmv.movie", [文件名])` | 播放器是桩：`VideoPlayer::open` 恒 `Unsupported`，宿主调 `VideoPlayer::skip_notice()` 拿"只播一次"的文案后继续（§9） |
| 脚本（Ruby / JS） | `Custom(name, args)` | **脚本不进这台虚拟机**：`ScriptBackend` 收到 `Custom` 自己解释；后端不认得该 `name` ⇒ 登记告警后继续，不静默。`355`/`356` 分别给 `rpgmv.script`/`rpgmv.plugin` |

> `sisf-event` 里唯一认识 JSON 的是 [`Param::from_json`](crates/sisf-event/src/command.rs)
> （`serde_json::Value` → `Param` 的四态折叠）。**没有**第二个"MV 指令解码器"藏在引擎里：
> 码表 → `Command` 的全部知识都在 `sisf-rpgmk::translate`，一份事实只写一处。

## 3. 存档与确定性

`VarStore::seeded(seed)` 里的 `Rng` 是**可复放的**（无全局状态、不进 `HashMap` 迭代顺序），
`Command` 全 `Serialize/Deserialize`（RON）。因此"事件页条件 + 变量 + 开关"这三样能被同一份
JSON/RON 复原；地图状态、精灵槽位、音频通道位置由宿主自己记账 —— 引擎不猜哪些精灵属于存档。

T11 在上面盖了 [`GameVars`](crates/sisf-rpgmk/src/game_vars.rs)（`Game_Switches`/`Game_Variables`/
`Game_SelfSwitches` 三张表同一个 `VarStore` 后端）：

- **引擎格式**：`GameVars::to_ron` / `from_ron`。序列化的是**整个 `GameVars`**，所以
  `System.json` 名字表带来的两条写边界（`switch_cap`/`variable_cap`）也进档；`refresh` 那面旗
  刻意不进（它是"这一拍要说一次的话"），读完一律置起 —— MV 读档走 `Game_Map.setup`，事件页
  是在那里重算的。
- **对账格式**：`GameVars::to_mv_json` / `from_mv_json`，形状就是 MV 存档的三个字段（`_data`
  数组**按 1-based 直映**、下标 0 是洞、自我开关是 `"map,event,ch"` 逗号键）。存在的理由不是
  "要能被 MV 读"（MV 还要 `system`/`map`/`party` 十几样），是让两边能用同一个 diff 工具比。
- **自我开关在 `VarStore` 里存的是文本槽 `"1"`/`""`**，键带 `ss:` 前缀。真值判断走
  `ss_truthy`（空串与 `"0"`/`"false"` 为假），所以虚拟机用 `Slot::Var(ss_key)` 直接写、和宿主
  用 `set_self_switch` 写，读出来是同一个答案。

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
- **素材容器与解密**：**明文 `.zip` 工程包已经能做**（`archive` feature，native，见 §9）；
  RGSSAD（XP/VX 打包）、RGSS2A/OGGVrypt 的 `.ogg`、MV/MZ 的加密素材仍是**待建**，
  接口骨架在 §9。密钥归项目，引擎永远不猜。

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
5. **BGM 循环点**读的是容器元数据（OGG 的 Vorbis comments `LOOPSTART`/`LOOPEND`、MP3 的 ID3v2
   TXXX 同键与 AIMP 风格 `MP3LOOPSTART`、MIDI 的内部
   循环），所以"能不能无缝循环"由素材决定，不由引擎决定（`docs/audio.md`）。
6. **手柄只在桌面三端**：wasm 要 JS 侧轮询 `navigator.getGamepads()`、Android 上 gilrs 0.11
   没有后端，所以 `native` 里的 `gilrs` feature 与两层 `cfg` 必须一起判（`docs/input.md`）。

## 7. 起手式

1. 素材目录 → `Vfs` + `DirProvider`（`examples/` 里的示例都是这么读的；根 `Cargo.toml` 用
   `path = "../../examples/*.rs"` 把它们挂在 `sisf` 门面 crate 下）。
2. `AppConfig::virtual_size` 定成项目分辨率（MV/MZ 默认 816×624，VXAce 544×416）。
3. 角色表 → `SpriteArena` 槽位 + `Scene::set(actor_id, sprite_ref)`；`actor` 编号由此和事件脚本对齐。
4. 事件 → RON（`examples/assets/events/*.ron` 是形状参考）→ `EventDef::from_ron` → `compile()`。
5. `AudioPort`/`UiPort` 各一个实现对象 —— 两者是**不同**的 `&mut` 借用，合成一个对象会 borrow-check 失败。
6. 地图数据、事件页条件、存档：见 §8（`sisf-rpgmk`），或直接抄 `examples/rpgmv_shell.rs`。

## 8. MV 工程的读入侧（`sisf-rpgmk`，T11）

这一层只管"把磁盘上的 MV 工程变成引擎已有的形状"，四块各管一件事，谁都不抄谁的：

| 块 | 对位上游 | 交出去的东西 |
|---|---|---|
| `map.rs` | `DataManager` + `Game_Map` 的数据面 | `MapDef` / `TilesetDef` / `SystemDef`（宽松反序列化：未知字段忽略、`events` 里的 `null` 洞保留） |
| `translate.rs` | `Game_Interpreter` 的 `commandNNN` handler 表 | `Vec<Command>` + `EventDef` + `TranslateReport` |
| `game_vars.rs` | `Game_Switches` / `Game_Variables` / `Game_SelfSwitches` | `GameVars`（三张表一个 `VarStore`，见 §3） |
| `scene.rs` | `Game_Map.setup` + `Game_CharacterBase` 的通行/自走面 | `Scene_Map`（`enter` → 每拍 `tick`，换页走 `refresh`） |

### 8.1 读三件套

```rust
let mut vfs = sisf_asset::Vfs::new();
vfs.register(Box::new(sisf_asset::DirProvider::new("MyProject")));   // 或 §9 的 ZipProvider
let sys = sisf_rpgmk::SystemDef::load(&vfs)?;                        // data/System.json
let map = sisf_rpgmk::MapDef::load(&vfs, 3)?;                        // data/Map003.json
let ts  = sisf_rpgmk::TilesetDef::load(&vfs, map.tileset_id)?;       // data/Tilesets.json 里按 id 找
```

四条从上游读来、容易凭印象写错的：

- **`Map.json` 里没有 `id` 字段**：图号在文件名的三位零填充里（`DataManager.loadMapData` 那句
  `padLeft`）。所以 `MapDef::load` 收 `map_id`，`Scene_Map::enter` 也收 —— 自我开关的键要用它。
- **`data` 是 6 个整层**（z0..z3 图块、z4 阴影、z5 区域），`MapDef::layers()` 报的就是 6；
  但通行只查前三层（`MapDef::TILE_LAYERS = 3`，MV 的 `checkPassage` 循环也是 `layer 2→0`），
  区域号单独走 `MapDef::region`（z=5）。
- **`Tilesets.json` 是顶层数组**（`dataTilesets[id]`，下标 0 是 `null`），不能整文件喂
  `TilesetDef::from_json` —— 那是"一条图块组"的形状。
- **通行位是"阻挡"不是"允许"**，且第一个表态的图块说了算：`passage::NO_EFFECT`（`0x10`，编辑器
  里的 `[*]`）跳过继续往下找，全都不表态 ⇒ **不通**（MV 兜底的 `return false`）。方向位用
  `passage::bit(dir)`（`dir` 是 2/4/6/8）。站在格子上的**图块事件**排在图块层前面，所以
  `MapDef::passable` 多收一个 `ev_tiles`（来自 `Scene_Map::event_tiles`，只给**当前页**的
  `EventPage::tile_id`）。

### 8.2 事件码表

`translate_event(&MapEvent, map_id, Options) -> (EventDef, TranslateReport)`（整棵树的口径），
`translate_list(&[RawCommand], map_id, event_id, Options)`（公共事件与单页走这个），
`translate_route(&RawRoute, Options) -> (RouteSpec, TranslateReport)`。

- `Options { tick_hz, tile_px }` 是**一个工程一份**：MV 的一切时间单位都是 60fps 的帧，
  图块边长是 48，非 48 的外挂插件工程可以覆盖 `tile_px`。
- `MV_HANDLERS`（112 个码，升序供二分）决定"静默跳过"与"降级 + 计数"的分界：不在表里的码
  MV 自己也不执行（`executeCommand` 只做 `_index++`），报成"未实现"只是刷噪音。
- 降级产物是 `Command::Custom("rpgmv.unimplemented", [原始码, 原始参数…])`，**并整块丢掉它的
  缩进体**（对应 MV 的 `skipBranch`）；计数与只 warn 一次的出口是
  `TranslateReport::{summary, warn_new, total_unimplemented}`，`rpgmv_shell` 进图时打一次。

### 8.3 场景层的边界

`Scene_Map` 不管绘制（宿主拿 `chars()` 摆 sprite），也不管解释器（宿主拿 `page(id)` 的
`trigger` 去起 `EventRunner`）—— 那两件事引擎已经有了。有意差异三条，都写在 `scene.rs` 的
模块文档里：没有视口概念（`isNearTheScreen()` 恒真）、玩家不参与"被撞"判定（与 MV 一致）、
一拍走一整格（插值归精灵层）。

自主移动只建模 `moveType` 1（随机，含 `randomInt(6)` 的那副骰子分布与
`stopCountThreshold = 30 * (5 - moveFrequency)` 的节奏）。**2（`moveTypeTowardPlayer`，向玩家
接近）与 3（`moveTypeCustom`）未建模**，且不是空实现：3 要的路线执行器在引擎侧是精灵层的
`MoveRoute`，所以 `translate_route` 已经把路线翻成 `RouteSpec`，剩下"照着走"是宿主摆 sprite
的事（DoD-4）。未建模 ≠ 静默：`Scene_Map::refresh` 的 `Refresh { changed, hidden, unsupported }`
里 `unsupported` 记的是页条件命中"物品/队伍"这两类没有通路的判断（按**不满足**处理，宁可少显示
一页）。

## 9. 归档与解密：已实装 / 待建

### 9.1 已实装：明文 `.zip`（`archive` feature，native only）

```rust
// Cargo.toml: sisf-rpgmk = { version = "*", features = ["archive"] }
let zip = sisf_rpgmk::ZipProvider::open("MyProject.zip")?;   // 或 ZipProvider::from_reader(任意 Read+Seek+Send)
eprintln!("包里有 {} 个文件", zip.entries());                 // listing() 给排序后的条目名
vfs.register(Box::new(zip));                                  // 之后 data/Map003.json 就从包里读
```

- 索引在构造期按中央目录一次建好，`exists` 是哈希查表；条目名走 `VfsPath` 规范化，所以
  **大小写不敏感**、反斜杠条目名（Windows 打的包）也认；目录条目与畸形名（绝对路径、`..` 越根）
  不进索引，而不是让整个包打不开。
- 后注册者优先 ⇒ "包 + 散文件热更"就是注册顺序（`sisf-asset` 的 `later_provider_overrides_earlier`
  钉的就是这条）。
- **`zip` 只开了 `deflate`**（`default-features = false`）：Stored/Deflate 之外的条目（BZip2、
  LZMA、Zstd、PPdM）与 AES 加密条目读不出来 —— 读取时把错误打到 stderr 后返回 `None`。
  这不是"静默丢文件"：资产层唯一的降级通道就是 `None`，而错误信息由那一条 `eprintln!` 说清。
- **wasm 上没有这条**：依赖写在 `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`，
  `archive` feature 在 wasm32 是显式空转（模块被 `cfg` 挡掉，`cargo xtask check-all` 会跑到这条腿）。
  浏览器的正确做法是宿主 JS 解压后把字节喂 `MemoryProvider` / `WebManifest`，见 `docs/platforms.md`。

### 9.2 待建：三种加密容器

**都还没有实现**，也都没有现成 crate 可拉（crates.io 上没有一个能解 RGSSAD/RGSS2A/MV-MZ 素材加密的
维护中包）；算法在 rpgmad-lib（Python，`rpgmad`）与 Luminol（Rust 编辑器，`game-sync`/`rpgmz` 模块）
里各有一份可读的实现。每个约 200 行，纯字节操作，**不需要 `VfsProvider` trait 改动** ——
骨架就长这样：

```text
pub struct RgssadProvider;              // .rgssad / .rgss2a / .rgss3a（XP/VX/VXA）
pub struct MvCryptoProvider;            // MV/MZ 的加密素材（Graphics/*.rgss3a 之类）

impl RgssadProvider {
    /// `password` = Game.exe 里的 4 字节 key（RGSS 用它做 XOR 流的种子）。
    pub fn open(bytes: &[u8], password: &[u8]) -> Result<Self, ArchiveError>;
    /// 建索引：逐条读加密的文件头（名字长度、偏移、`Marshal.load` 那段子头）。
    fn index(&mut self) -> Result<(), ArchiveError>;
}

impl sisf_asset::VfsProvider for RgssadProvider {
    fn read(&self, path: &sisf_asset::VfsPath) -> Option<Vec<u8>>;   // 解密后返回明文
    fn exists(&self, path: &sisf_asset::VfsPath) -> bool;            // 一次哈希查表
}
```

三件要一起决定、别在实现时才想起的事：

1. **密钥从哪来**：RGSS 的 key 在可执行文件里，MV/MZ 的 `EncryptionKey` 在 `Game.rgsspec`/
   `Game.ini` 旁边。引擎**不猜、不硬编码** ⇒ 只能作为构造参数由宿主传入（授权素材的密钥归项目）。
2. **`.ogg` 的 RGSS2A/OGGVrypt 壳**：那是**素材文件级**的加密，不是容器级的，所以它不属于
   provider，而属于 `sisf-media` 的解码入口 —— 接法应是"解出明文字节后再交给解码器"。
   现在这类文件会走到音频解码失败并报错，不是静音。
3. **要不要在 wasm 上解**：解密是纯字节操作，可以跨端；但密钥在浏览器里等同于公开，
   所以 wasm 侧要不要开这条腿是**授权决定**，不是技术决定。

### 9.3 影片：接口在，两端都是桩

`sisf_media::video` 放解码侧（`VideoSource`、`open`、`sniff`、`VideoError`、`VideoFrame`），
`sisf::video::VideoPlayer` 放纹槽那一半（`texture`/`set_texture`/`pump`）—— 因为包级依赖图是
`render → asset → media`，`VideoPlayer` 的槽是 `Bitmap`，放在 media 就是环。
`VideoPlayer::open` 恒 `Err(VideoError::Unsupported(STUB_NOTE))`，两端同一句话；移植层遇到
`261`（`Custom("rpgmv.movie", …)`）用 `VideoPlayer::skip_notice()` 取"只说一次"的文案后继续播，
不 panic、不静默。真后端要补的两条路（每帧新页 / 给 `Bitmap` 加写回）写在 `crates/sisf/src/video.rs`
的模块文档里。
