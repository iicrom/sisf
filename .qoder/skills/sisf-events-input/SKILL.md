---
name: sisf-events-input
description: SISF 输入与事件虚拟机：ActionMap 命名动作、EngineEvent 总线词汇、EventRunner 启动事件页、AudioPort/UiPort 义务、总线纪律。当处理按键/手柄输入、对话/事件流程或写事件脚本时使用。
---

# SISF 输入与事件机

## 输入：游戏侧只读命名动作

设备事件（winit/gilrs）由 sisf-app **翻译一次**进统一词汇表 `EngineEvent`
（sisf-event/src/bus.rs:66-94）：

- `Action { name, edge }` —— ActionMap 归一化后的命名动作，`edge: Press | Hold | Release`
- `Key { code, kind, ch }` —— code=物理键名（与布局无关），ch 仅按下有值
- `Pointer { kind, pos, button, delta }` —— **pos 已映射到虚拟像素**，可直接与 sprite 命中测试
- `Lifecycle(Resumed|Suspended|Focused|CloseRequested)`、`Custom(u32, serde_json::Value)`

**规矩（docs/input.md:150-154）：`on_event` 里一个裸键名都不要有，只读 `Action`；
改键位 = 换 `ActionMap`（RON 可载），不碰代码。**

- 缺省 `ActionMap::rpg_maker()`：up/down/left/right/ok/cancel/shift/pageup/pagedown/menu；
  经 `AppConfig { actions }` 注入，运行期可 `InputState::set_map`（换表作废按住状态、不补 Release）。
- `Trigger::{Key("ArrowLeft"), Pad("South"/"LeftStickX-"), Pointer(Left..)}`；
  `ActionDef { name, triggers, one_shot, autofire }`；`Autofire::RPG_MAKER = {delay 20, interval 6}` 拍。
- 读法（tiled_map.rs:1192-1222）：方向按住连走读 `Press|Hold`（或 `ctx.input.is_down(name)`），
  "决定"类只认 `Press`（或 `take_press(name)` 读一次即消费）。
- 引擎在 Runner 内自动 feed + 每拍 `input.tick`（runner.rs:128-135）；失焦/挂起自动
  `forget_held` 补发 Release，游戏侧不必自己清键。手柄仅桌面。

## 事件机：Command / EventRunner

磁盘形状 = RON `EventDef { name, pages: Vec<Vec<Command>> }` → `compile() -> EventScript`
（command.rs:449-604）。`Command` 目录（command.rs:362-445）：`ShowText`（choices/result/
cursor_se）、`InputKey`、`Wait{frames 拍}`、`MoveRoute`、`Animate`、`PlayBgm/Bgs/Me/Se`、
`Fade`、`SetVar`、`If/Loop/BreakLoop/Jump{page}`、`Erase`、`Custom(String, Vec<Param>)`（脚本口子）。

**RON 坑**（docs/events.md:65-69）：`speaker`/`cursor_se` 必须显式写 `None`；
磁盘上的 `else` 在代码里是 `else_`；可省字段挂 serde(default)。

### 驱动（dialog_demo.rs:920-961 为范本）

```rust
vm.start(actor_id, Arc<EventScript>, page) -> bool;   // 页号由游戏侧按开关决定
vm.start_program(subject, Arc<Program>);              // 现编一段指令（玩家移动走这条）
let st = vm.tick(&mut ctx, BUDGET);                   // 每逻辑拍一次；预算只在指令边界生效
```

`EventCtx`（runner.rs:310-321）聚合 `sprites/tasks/scene/audio/ui/vars/bus/vfs`——
在 `App::tick` 里解构 self 借出各字段构造它（借用检查器会教你怎么拆）。
挂起查询：`prompt()/waiting_input()/busy()`；`RunnerStatus::Suspended.reason ∈
"text"|"key"|"wait"|"task"|"fade"`（ASCII 令牌，可 match，也是 wasm 无中文字形时的面板读数）。

### 游戏侧要实现的两个端口

- `AudioPort { play(vfs, SoundChannel, &Sound), fade(FadeTarget, frames, out), stop }`
  —— **唯一义务："同一 name 只解码一次"**（复用 `Arc<AudioData>`，见 sisf-assets-audio）。
- `UiPort { show(&Prompt), hide(), set_dim(0..=1) }` —— 契约：UI 状态是事件机状态的函数，
  **只在变化时推**。

### 总线纪律（docs/events.md:85-107，四条）

1. 事件机是游戏侧总线的**唯一读者**；`on_event` 把切片原样转进自己的 `EngineBus`。
2. 没人等输入时每拍抽干总线（`spend_keys`），别让按键积压。
3. `tasks.tick` 归 EventRunner 独占——游戏侧再推一次 = MoveRoute 双倍速且**不报错**。
4. `InputKey` 不认无字符/控制字符按下（wasm 的 Enter 是 `'\r'`）；不认识的事件按原序放回。

### 变量与脚本

- `VarStore`：switches/ints/texts 三态 + 可存盘 splitmix64 RNG（`set_seed` 复现）；
  除 0 = 0、文本槽按十进制宽容解析。
- 脚本后端 `ScriptBackend { start/tick/command(name,args)->Option<Arc<Program>> }`：
  `command` 返回 `None` = 不认 ⇒ 登记告警继续跑（`take_warnings()`）；控制流留在虚拟机里，
  脚本只能"再启动一段程序"。缺省 `NullScriptBackend` 什么都不认；`js` feature（非默认）提供
  `sisf_event::js::JsBridge`（native=rquickjs、wasm=宿主引擎）认领 `rpgmv.script`/`rpgmv.plugin`，
  把 JS 写请求翻成 `Command` 交 VM —— 详见 `docs/porting.md` 第 10 节。
- 事件机不冻窗：死循环只超预算拖慢一拍（`MAX_NESTING=64`）。

## 测试入口

`EngineEvent::key(..)/::action(..)` 辅助构造直接喂 `on_event`；`cargo xtask wasm-test`
的 sisf-event 腿覆盖 command/vars/runner/bus/input（纯逻辑，无 GPU/文件系统）；`js` 腿
（`--features js -- js::`）覆盖 JS 桥的账本→`Command` 映射，同一份断言在 native 与 wasm 两腿各跑一遍。
