# 输入：事件总线与动作映射（T9）

三端（native / wasm / Android）只有**一条**输入通路：设备事件在 `sisf-app` 里翻译成裸
[`EngineEvent`](../crates/sisf-event/src/bus.rs)，`sisf-event` 把裸事件折成
**命名动作**（[`ActionMap`](../crates/sisf-event/src/input.rs)），游戏侧只读动作名。

```text
winit WindowEvent ┐
gilrs 手柄事件    ├─ normalize.rs（翻译只做一次）─► EngineEvent（裸）─┬─► App::on_event
触摸 / JS 消息    ┘                                                   │        ▲
                                                                      │        │ drain（FIFO）
                                            InputState.feed/tick ─► EngineBus ─┘
                                            （裸事件 + Action 边沿，按发生顺序）
```

`InputState` 住在 `FrameCtx::input` 里，所以 `App::tick(ctx)` 与 `App::on_event(.., ctx)`
都能直接问"现在 `left` 按住了吗"，不必自己存一份按住状态。

## 事件词汇表（`EngineEvent`）

| 变体 | 载荷 | 约定 |
|---|---|---|
| `Action` | `name` + [`ActionEdge`] | 由引擎在交给游戏之前先算好，见下文 |
| `Key` | `code` + `kind` + `ch` | `code` 是**物理键名**（`"ArrowLeft"`、`"KeyW"`），布局无关；`ch` 是布局相关字符（`Shift+2` → `"@"`），只在按下时有值。文本输入读 `ch`，动作读 `code` |
| `Pointer` | `kind` + `pos` + `button` + `delta` | `pos`/`delta` 已过 `FrameCtx::map_pointer`，单位是**虚拟像素** ⇒ 命中测试可直接和 sprite 坐标比，不必知道窗口多大、有没有 letterbox |
| `Lifecycle` | `Resumed`/`Suspended`/`Focused(bool)`/`CloseRequested` | 三端归一化上报（挂起来源三分见 `docs/platforms.md`） |
| `Custom(u32, Value)` | 主题号 + 任意 JSON | 宿主 / JS / 移植层的口子，载荷用 `serde_json::Value`（跨 wasm 边界就是 JS 侧那个对象） |

[`EngineBus`](../crates/sisf-event/src/bus.rs) 刻意是"没有监听器图"的一层：只保证
**入队顺序 = 发生顺序**、`drain` 一次取空。订阅、优先级、冒泡都在游戏侧做。
去重也不是总线的活（同一批里两条 `Hold` 照原样留着）—— 那是 `ActionMap` 的账。

## 归一化（`crates/sisf-app/src/normalize.rs`）

每个函数都是纯函数（进去 winit 的事件片段，出来一条裸事件），所以四端都能单测。规则：

- **键盘**：`PhysicalKey` 借 `KeyCode` 的 `Debug` 名当键名字典（`"ArrowLeft"`），未识别的扫描码
  带上原编号（`"Native(..)"`）而不是静默成空串。按下 + `repeat` = `KeyKind::Repeat`，
  它是操作系统的连发，`InputState` 不当成一次新的按下（按住状态由动作层自己数拍子）。
- **鼠标键**：`Back`/`Forward` 折回原始编号 `Other(3)`/`Other(4)`，侧键不必挤成"无"。
- **光标移动**：`delta` = 相对上一次位置的位移；**首次**没有上一个位置时记 0，否则光标一进
  窗口就凭空一条几百像素的 `delta`。
- **滚轮**：`LineDelta`（几格）与 `PixelDelta`（像素）折成同一个单位 —— 虚拟像素，一格按
  40px 计。混着两种单位读的人只能猜。
- **触摸**：`Started`/`Moved`/`Ended|Cancelled` → 指针的按下/移动/松开，键固定
  `Other(0)`（一根手指 = 一个可绑定的"键"，`None` 留给纯移动/滚轮）。
- **手柄**：也翻成 `EngineEvent::Key`（见下文码词汇表）⇒ `Trigger::Pad` 与 `Trigger::Key`
  走同一条匹配路径，动作层没有"手柄特例"。

`MouseInput`/`MouseWheel` 在 winit 0.30 里自带坐标缺失，故 `Runner` 记录最近一次光标位置
（从没动过鼠标就点时按左上角算，不丢掉这次点击）。

## 动作表（`ActionMap` / `InputState`）

一个动作 = 一组触发源（`Trigger::Key` / `Pad` / `Pointer`）+ 两条语义开关。
RPG Maker 的默认键位（决定/取消/Shift/PageUp 是清单点名的四个）见
[`ActionMap::rpg_maker()`](../crates/sisf-event/src/input.rs)，
可从 RON 载入（`ActionMap::from_ron`，内容就是 `Vec<ActionDef>`）：

```ron
[
  ( name: "ok", triggers: [ key("Enter"), key("Space"), pad("South") ],
    one_shot: true, autofire: Some((delay_ticks: 20, interval_ticks: 6)) ),
  ( name: "run", triggers: [ key("ShiftLeft") ], one_shot: false ),
]
```

**按住状态是每个动作一个位掩码**（第 i 位 = 第 i 个触发源当前按住），所以"两个键绑同一个
动作"要**两个都松开**才算松开 —— 不是最后一条事件说了算。反过来同时按 ← 和 → 时，
松开后按的那个不该让角色停下（`examples/tiled_map.rs` 的
`the_app_reads_named_actions_rather_than_raw_keys` 钉住了这两头）。

三种边沿（[`ActionEdge`]）：

| 边沿 | 何时产生 |
|---|---|
| `Press` | 松开→按下那一次；外加连发（`Autofire`）放大出来的每一次重触 |
| `Hold` | **非** one-shot 动作在**每个逻辑拍**报一条 —— "按着就一直算按着" |
| `Release` | 按下→松开（掩码归零那一刻，多源时是最后一个源松开） |

- **one-shot**（决定/取消）：不报 `Hold`，且 `InputState::take_press` 一次消费 ⇒ 菜单里按着
  Enter 不会一口气跳三行。
- **`Autofire`**：按住 `delay_ticks` 拍后每 `interval_ticks` 拍再报一次 `Press`。数字取 RPG
  Maker 的菜单手感（20 / 6）。
- `Hold` 与连发**只数拍子不数帧**（在 `InputState::tick` 里，由 `Runner` 每拍调一次），
  所以渲染帧率不影响逻辑手感 —— 这是 T8 固定步长在输入侧的续集。
- **失焦 / 挂起 / 关窗**：`Runner` 调 `forget_held` 放掉全部按住的动作并补发 `Release`。
  窗口最小化之后是收不到 keyup 的，不放掉的话游戏回来就朝一面墙磨。松开**不**吞掉还没被
  消费的 `Press`（丢焦点那一刻的一次快速点按仍要生效）。

## 手柄（gilrs 0.11.2，只在桌面）

### 平台矩阵（DoD-4：feature 关闭 + 文档标注，不留空实现）

| 目标 | 手柄 | 原因 |
|---|---|---|
| Windows / Linux / macOS | ✅ | `native` feature 下 `gilrs = ["dep:gilrs"]` |
| `wasm32` | ❌ | 浏览器要 Gamepad API 得每帧轮询 `navigator.getGamepads()`，那是 JS 侧的活；本阶段不接 |
| Android | ❌ | gilrs 0.11 没有 `android-activity` 后端，依赖连声明都不给 |

`cfg` 是**两层**叠加的：`feature = "gilrs"` + `not(target_arch = "wasm32")` +
`not(target_os = "android")`。Cargo feature 本身跨 target，只判 feature 会让 wasm/android
的 check 挂在一条链不上存在的依赖上。`Trigger::Pad` 在后两端永远匹配不上事件但不报错 ——
键位表照常工作，只是少一个设备。

### 码词汇表

按钮取 gilrs 的变体名（`"South"`=Xbox A、`"East"`=B、`"DPadUp"`…、`"LeftTrigger"`、
`"LeftShoulder"`）；摇杆取 `"<轴名><符号>"`（`"LeftStickX-"`、`"LeftStickY+"`）。借变体名
而不是自己抄一张表，是因为它跟着 gilrs 的变体重命名走，抄表会静默错位。
屏幕坐标里摇杆轴的正方向是"下/右"，所以 `-1.0` 才是 `left` —— 符号约定由
`stick_codes_resolve_through_the_same_action_map_as_the_keyboard` 钉住（反了的话用手柄走路
会朝镜像方向）。

### 死区与边沿

gilrs 的默认过滤器先做抖动抑制，并把左摇杆额外折算成一组十字键按下（`DPadUp`…）。
本层（`crates/sisf-app/src/gamepad.rs` 的 `StickEdges`）再加一层**方向码**：
按 0.75 / 松 0.65 的迟滞对，**刻意与 gilrs 折算十字键用的是同一对数字** —— 两套码来自同一根
轴，阈值不一致就会出现"十字键已松开、摇杆码还按着"那种半格重叠死区，而动作层的位掩码要求
两个源都松开才算松开。换向时先松旧再按新（位掩码要的就是这个顺序）。
同一根轴换向、回中、拔掉手柄（`forget_all` + `release_where(Trigger::Pad)`）都会补齐松开边沿，
不许留下"永远按着"的方向。

扳机是模拟量（`ButtonChanged` 的 0.0..=1.0），过 0.5 才算"这个键按下了"；
操作系统的十字键连发（`ButtonRepeated`）忽略，按住状态由 `InputState` 记账。
`Gilrs::new()` 起不来（无权限/无后端/插了个怪手柄）时 `pads = None`，键盘照常 ——
引擎不该因此起不来。

多只手柄共用一套码（与 `Trigger::Pad` 的词汇表一致），T9 不做按序号区分。

## `post_custom`：宿主 → 总线的单向通道

`sisf::app::post_custom(channel, payload)` 把一条 `EngineEvent::Custom` 投进全局队列，
`Runner` **每次派发前**取空并按到达顺序追加到总线尾部。用全局队列是因为调用点不在 `Runner`
手里（JS 那边连 Rust 对象都拿不到），而 T10 的 `Command::Custom` / QuickJS / 移植层都要
这个口子。本阶段只提供通道与顺序保证 —— 不订阅、不路由、不去重。

## 一帧里输入的位置（`Runner::do_frame`）

1. 设备事件在**到达的那一刻**就 `InputState.feed` + 立刻派发（不等 `RedrawRequested`，
   否则按住方向键的手感会随渲染帧率抖）；
2. 帧首：轮询手柄 → 并入 `custom` → 派发；
3. `TickClock::step(真实 dt)` 决定这一帧补几拍，**每拍** `input.tick(bus)` → 派发 → `App::tick`；
4. `App::prepare(alpha)` → `App::draw(alpha)`。

`Resized` 只改 `FrameCtx` 的投影与映射，**不**换 `InputState`（换了等于把"正按着什么"丢掉）。
`FrameCtx` 整体重建只发生在建窗口那一刻（此刻不可能有按住状态）。

## 示例侧怎么写（`examples/tiled_map.rs`）

`on_event` 里**一个裸键名都没有**：只读 `EngineEvent::Action`，方向由动作名查，
"决定"只认 `Press`。示例仍留一个 `Held`（方向按下的 LIFO 序），因为它要的是 RPG 的
"最后按下的方向优先"，而动作层的按住是位掩码（无序）。

## 命令与测试证据

```sh
cargo test -p sisf-event                 # bus 4 + input 19（四端纯逻辑，不碰设备）
cargo test -p sisf-app                   # normalize + gamepad(死区) + custom
cargo test -p sisf --example tiled_map   # 端到端：手柄码 → 动作 → 走格子
cargo xtask check-all                    # 四组 feature 组合（含 wasm32 web、aarch64 android）
```

`cargo xtask web` 只在 `native` 组合里链 gilrs；wasm 侧的输入路径靠
`cargo xtask check-all` 的第二/第四组覆盖编译，行为与桌面同源（同一个 `Runner`、同一批
`normalize` 函数）。
