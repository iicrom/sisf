# 事件解释器：指令表、挂起与端口（T10）

[`sisf-event`](../crates/sisf-event) 的这台虚拟机把磁盘上的 RON 指令表跑成副作用。**控制流
始终在虚拟机手里**：脚本引擎（T11 的 QuickJS / rpgmk 移植层）能做的每件事，最后都落成
"请再启动一段程序"。这条约束换来的东西很具体 —— 存档只需要 [`VarStore`] 与页号，不需要冻结
一颗 JS 堆。

```text
examples/assets/events/*.ron ──编译──► Program（扁平 IR）──┐
ScriptBackend::command（Custom 的落地）──────────────────┘─► EventRunner
                                                              │ 每拍一次 tick(ctx, budget)
                                        ┌─────────────────────┼──────────────────┐
                                        ▼                     ▼                  ▼
                                   UiPort                 AudioPort        TaskScheduler
                                 （消息窗/黑幕）        （四条通道）        （补间/路线）
```

## 一拍的形状（[`EventRunner::tick`]）

```text
(a) 脚本后端先说话（它能启动程序，也能改精灵）
(b) 推进任务调度器（补间与路线走一拍）
(c) 解挂：把总线里的事件喂给当前挂起的原因
(d) 执行指令，直到挂起 / 栈空 / 预算用完
(e) 同步 UI（只在"和上次推给 UI 的不一样"时才推）
```

(b) 在 (c) 之前 ⇒ "跑完最后一格"的路线在同一拍里就被观察到，`wait: true` 的移动不白等一拍。
(e) 放最后 ⇒ 游戏侧永远在本拍的状态之上画，看不到中间态。

**为什么是显式 `pc` + 调用栈而不是协程**：挂起要能跨拍存活、要能进存档、还要在 wasm 里跑
（`async` 一挂起就得持有 future）。一个 `Vec<Frame>`（`pc` + 循环回跳点 + `MAX_FRAMES = 32`
的深度上限）够用且可调试 —— 调试面板直接读 `pc` 就知道卡在哪条指令上。

## 指令表（[`command.rs`]）

MVP 十七发（`Play*` 四条、`Loop`/`BreakLoop` 两条各算一发）。"挂起"列写的是这条指令会不会
把 `pc` 留在下一条然后进 [`Hold`]。

| 指令 | 挂起 | 落到哪里 | 备注 |
|---|---|---|---|
| `ShowText{speaker,body,choices,result,cursor_se,allow_cancel}` | 是（`text`） | `UiPort::show` + `AudioPort`（移动光标 SE） | 有选项 ⇒ 写回高亮下标；无选项 ⇒ 写 `0`（"看完了"本身是结果，写 `-1` 会让"取消"和"看完"分不开）；`allow_cancel: true` 时取消写 `-1` |
| `InputKey{slot,timeout}` | 是（`key`） | 读总线裸 `Key` 按下 | `timeout: 0` = **无限等**；超时/取消**不写槽**（留原值） |
| `Wait{frames}` | 是（`wait`） | 纯计数 | 单位是拍，不是毫秒 |
| `MoveRoute{actor,route,wait}` | `wait: true` 时（`task`） | `TaskScheduler` | 撞墙作废剩余格数，路线继续 |
| `Animate{actor,effect,wait}` | `wait: true` 时（`task`） | `TaskScheduler`（编译成 `Tween`） | 四种效果每条都有通路，所以这里不存在"想要但没有端口"的变体 |
| `PlayBgm/Bgs/Me/Se(Sound)` | 否 | `AudioPort::play` | `Bgm` 占掉 `Bgs` 时按混音器规则淡掉它 |
| `Fade{target,frames,out}` | 音频：否；`Screen`：是（`fade`） | `AudioPort::fade` / `UiPort::set_dim` | `Screen` 与 `All` 分家：`Screen` **永远不经过 `AudioPort`** |
| `SetVar{slot,op,value}` | 否 | `VarStore::apply` | `value` 是 [`Expr`]（能算，含 `Rand`） |
| `If{cond,then,else}` | 否 | 编译成 `Branch` | `cond` 是 [`Cond`]（比较/开关/`All`/`Any`/`Not`/文本相等） |
| `Loop{…}` / `BreakLoop` | 否 | `LoopBack` 回边 | 回边也算执行了一条指令 ⇒ 无限 `Loop` 会被预算拦下 |
| `Jump{page}` | 否 | 换页并从头执行 | 越界 = 本事件结束（不是 panic） |
| `Erase` | 否 | `Scene::remove` + 任务与精灵一起没 | 擦的是**本事件的归属 actor** |
| `Custom(String,Vec<Param>)` | 取决于后端 | `ScriptBackend::command` | 没装后端或不认这个名字 ⇒ 登记告警后继续（DoD-4：没做的能力要说话） |

[`AnimEffect`] 的 `Flash` 播完会把 tone 恢复成**闪之前**那份，不是"归零" —— 一个本来就半透
明的宝箱被闪一下之后不该变成不透明。

## 编译：嵌套 RON → 扁平 IR

`Program{code: Vec<Instr>}`，`Instr` 只有四条：`Run` / `Branch{cond,on_false}` / `Goto` /
`LoopBack{to}`。嵌套深度上限 `MAX_NESTING` 在编译期就把关，因为运行期没有递归可以爆 —— 但
一份写错的 `.ron` 也不该等到跑起来才发现。

RON 侧两个坑（`examples/assets/events/*.ron` 是照着写的）：**能省的字段是挂了
`#[serde(default)]` 的字段** —— `choices`/`result`/`allow_cancel`/`wait` 可以整条不写，
`speaker` 与 `cursor_se` 不行（所以每发 `ShowText` 都得显式写 `cursor_se: None`，`Option` 在
RON 里写 `None` 是合法的）；`else` 是 Rust 关键字，磁盘上叫 `else`、代码里是 `else_`
（`#[serde(rename = "else")]`）。

## 挂起：五种原因，五个 ASCII 令牌

[`RunnerStatus::Suspended{reason}`] 的 `reason` 是 `"text" | "key" | "wait" | "task" | "fade"`
（没挂起 = `Idle`）。**为什么是令牌而不是一句中文**：它是给游戏侧 `match`、给调试面板当键用的
标识，而且浏览器里只有仓库自带那份拉丁字形（`docs/text.md`：Inter 没有 CJK）—— 中文的挂起原因
恰好会在最需要它的 wasm 手测里读成一排方块。

## 预算

`tick(ctx, budget)` 每执行一条指令扣一格，**只在指令边界**返回
[`OverBudget`]。所以：按着方向键狂按永远不会看到 `over`（一次走一格 = 一条指令），只有没有
`Wait` 的死循环才会 —— 而那也只是"这一拍慢了"，下一拍从同一个 `pc` 接着跑，窗口不会冻住。
`examples/dialog_demo.rs` 用的是 `BUDGET = 64`。

## 总线纪律：谁吃什么

事件机是 [`EngineBus`] 上游戏侧那一段的**唯一读者**。四条：

1. **没人等输入 ⇒ 这一拍的按键不该往队列里堆**。游戏侧每拍问一次
   [`EventRunner::waiting_input`]，返回 `false` 就把总线抽干（`dialog_demo` 的 `spend_keys`）。
   少了这一条，一个开着对话框的下午能让队列涨到几万条。
2. **调度器由事件机推进**。游戏侧只负责每拍开头 `SpriteArena::begin_tick`（`prev_pos` 不变量，
   见 T8 第五条"抖"）。两处都 `tasks.tick` = 路线跑得比逻辑快两倍，而且没有任何报错。
3. **一次按下只解一次挂**。`InputState::feed` 把裸设备事件放在前、它折出来的 `Action` 边沿
   放在后（同一次物理按下，相邻两条）。于是解掉 `ShowText` 的那次 `ok` 之后，裸按下会按原序
   留在总线里给下一段用 —— 如果 `InputKey` 也认它，同一个 Enter 就既关掉了上一句、又答完了
   下一题。规则：**按下不带可打印字符、或带的是控制字符（`\r`、`\t`、方向键那类），都不算
   `InputKey` 的答案**。浏览器里这条错的表现是"井边人永远停在'太小了'，任何数字都进不来"
   （winit 在 wasm 递过来的 Enter 字符是 `'\r'`，它是控制字符，不是 `None` —— 只判 `Some(c)`
   挡不住）。
4. **不认识的事件按原序放回**。批量消费用位图标记再 `flush`（边扫边搬会被借用检查判死，
   而顺序必须保持：松开在按下之后是常态，反过来就是丢按键）。

⚠️ 与规则 3 **症状完全相同**的另一条：`InputKey{timeout: 0}` 是"无限等"，而 `resolve` 是先把
`Hold` `take()` 出来、再按分支装回去的 —— `timeout == 0` 那一支漏装，"无限等"就只等一拍。
两条各撞了一次、各钉一个断言（同一份测试里连续两拍都断 `Some("key")`）。修完第一条必须立刻
复测，否则会把第二条当成"第一条没修好"，再改一遍第一处。

## 端口契约

- [`AudioPort`]：三条方法，义务只有一条 —— **同一个 `name` 只解码一次**。`Sound.name` 是 VFS
  路径，每圈重解一份 2s OGG 足以把输出环抽干（`todo.md` T7 事实第 3 条）。`play` 返回
  `Err(原因)` 时事件机登记告警而不中断事件：看不见声音时"为什么没声"是要说得出来的。
- [`UiPort`]：契约是**UI 状态是事件机状态的函数**。事件机只在内容变化时 `show`/`hide`/
  `set_dim`，实现方不必自己猜"这帧该不该重画"，只要把收到的 [`Prompt`] 原样落进位图。
  消息窗几何归游戏侧，但有一条硬要求：**窗高必须容得下选项行**（`dialog_demo` 按
  `choices.len()` 定高，单测 `the_window_holds_every_choice_list_on_disk` 拿磁盘上三份
  `.ron` 里最长的选项表钉住这件事）。
- [`Scene`]：`actor id → SpriteRef` 的登记表 + `walkable` 闭包。网格边长来自 `Scene::grid(tile)`，
  不写在每条路线里。`free()` 模式（SLG/无网格）下 `Move` 的单位是 1 像素。
- `Vfs`：只经由 `AudioPort::play` 传给实现方 —— 事件机自己不解资源。

## 变量与存档

[`VarStore`] 三态：`switch(u32)` / `int(u32)` / `var(名字)`（文本）。`Slot` 就是这三态的句柄，
`numeric`/`textual`/`set_numeric` 按槽型分派。[`Expr`] 能算（`Add/Sub/Mul/Div/Mod/Rand/Neg`），
[`Cond`] 能判（`Switch/Cmp/TextEq/Not/All/Any`）。两个约定值得钉住：整除**向零取整**、除数为
`0` 时结果是 **0**（不是 panic，也不是"保持上一个值"—— 后者会让一个除零 bug 伪装成"它一直
是这个数"）。

随机数在 [`Rng`]（splitmix64）里，**不读墙钟**：`VarStore::seeded(seed)` 之后整段事件可复现，
这是 golden 与存档回放的前提。

存档 = `VarStore::to_ron()` + 每个事件的页号。没有协程栈、没有 JS 堆要冻结。

## 脚本后端的口子

[`ScriptBackend`] 三个方法，都可以是空实现 —— 但空在**这里**是有意的（这台机器上还没有脚本
引擎），不是"还没写"：

- `start(entry)`：进入场景/地图时一次，要不要起跑由后端决定。
- `tick(&mut EventCtx, &mut Vec<Submission>)`：每拍在指令执行**之前**。后端可以从 `ctx.bus`
  取走自己的 `EngineEvent::Custom`（其余原序放回），往 `queue` 压要启动的程序。
- `command(name, args) -> Option<Arc<Program>>`：一条 `Custom` 的落地。`Some(空程序)` =
  "处理完了，但没有指令要跑"；`None` = "这个名字我不认" ⇒ 虚拟机登记告警。这两者必须分开，
  因为告警通道在事件机上（后端只拿到 `name`/`args`，它没有别的地方可说话）。

QuickJS 落地时实现这三条就够了，**不必改动虚拟机一行**。装 `NullScriptBackend` 与不装后端
（`EventRunner::new()` 的形状）两条路径都留着，是为了让"我要不要脚本"这件事在**类型**上
看得出来。

## 平台矩阵

三端跑的是同一份代码 —— 这台虚拟机不碰设备、不碰 GPU、不读墙钟。

| | native | wasm | Android |
|---|---|---|---|
| 指令表 / 控制流 / 变量 | 同一份 | 同一份 | 同一份 |
| `AudioPort` 实现 | `AudioBus`（cpal） | `AudioBus`（WebAudio） | `AudioBus`（cpal/AAudio） |
| `UiPort` 实现 | 游戏侧位图 | 同左 | 同左 |
| 脚本后端 | 缺 `NullScriptBackend` | 同左 | 同左 |

wasm 侧一条实测注意：**HUD 上的诊断行要 ASCII**。台词是中文（那是内容，桌面有系统字体兜着），
但"事件机现在挂在什么上面"是 wasm 手测唯一要读的东西，缺字形就读不到。`dialog_demo` 的状态
行因此写成 `hold key 0 · none` 这种形状。

## 挂账的回答：`Bitmap` 的 `filter: Linear|Nearest` 这一期不做

`docs/sprite.md` 把"像素风要不要最近点采样"推给 T10，`docs/platforms.md` 指望 T10 顺带定
`filter` 选择权。T10 跑完，答案是**不加双 sampler**，理由三条：

1. 这台事件机带来的唯一一个亚像素消费者是 `Animate{Shake}`。它的极值取在整数虚拟像素上
   （`base ± px`，键帧落在拍边界），被重采样的只是两个极值之间那一拍 —— 那正是抖动该有的
   样子，而且只持续一拍。像素美术里为它换采样器，省下的东西看不见，付出的东西（管线集翻倍、
   采集器要按 filter 分实例）看得见。
2. `Linear` + 1px gutter（T6）在格点上与 `Nearest` 等价 —— 采样点落在纹素中心时两者取到同一
   个纹素。也就是说在 `lattice_alpha` + `snap_to_device` 已经保证的这条路径上，`Nearest`
   **什么都改变不了**。
3. 真正需要 `Nearest` 的是"非整数标度"或"真亚像素滚动"（SLG 缩放、亚像素相机）。那一天到了，
   决策依据应该是 `docs/platforms.md` 里那个"相邻两帧能否用整数位移对齐"的探针读数，而不是
   提前铺接口。届时再定，接口现在不留。

## 手测证据（`examples/dialog_demo.rs`）

`cargo test -p sisf --example dialog_demo --features image,ogg,mp3,wav` → **9 项全绿**，其中
`the_event_tables_on_disk_parse_and_compile`（三份 `.ron` 真的从磁盘解出来并编译）、
`the_npc_walks_fades_and_erases_himself`、`the_chest_jumps_to_its_opened_page`、
`the_window_holds_every_choice_list_on_disk`。

桌面（native，`./target/debug/examples/dialog_demo.exe`）起来到被掐掉为止无 panic：
`事件 well ← events/well.ron（页 1，指令 10 条）`、`chest（页 2，12 条）`、`npc（页 1，17 条）`、
`adapter: NVIDIA GeForce RTX 2080 Ti (DiscreteGpu, Vulkan)`、`字体 Some("Inter")（face 266）`、
`cpal: 无输出设备（静音混音，时钟照常）`（这台机器没有输出设备，端口按契约返回告警，
`warn` 计数为 0 说明没有指令落空）。

浏览器（`cargo xtask web dialog_demo` → `?ex=dialog_demo&wgl`，逐帧像素探针 + 合成按键）：

- 井边人整条链：触发 → `BGM`/`BGS` 起播（HUD `BGS ← tone_2s.ogg … gain 0.25`）→
  `MoveRoute(wait:true)` 把他从 `(6,7)` 走到 `(6,6)` 再走回来，这期间玩家按键推动不了世界 →
  `ShowText` → `hold text` → Enter → **`hold key` 且持续 1300+ 拍不自行放行**（上面那两条
  同症状 bug 的现场证据）→ `Digit3` 被 `Loop` 拦回去（`ui pushes 3` = 多了一句"太小了"）→ 再 Enter →
  回到 `hold key` → `Digit7` 放行 → HUD **`answer=1`**（`BreakLoop` + `SetVar` 生效）→
  `PlayMe` + `Fade(Bgm/Bgs,45)` → 2.5s 后 `idle 0`、**`actors 3`**（`Erase` 生效，橙色精灵不见）、
  `BGS … 0.0 / 750ms`（45 拍 = 750ms 淡到 0）、全程 `warn 0`。
- 宝箱：选项高亮 `229,209,103` 与窗体几何（按 `choices.len()` 定高）都在这条腿上量到 →
  `Flash` 白闪后精确回到 `206,212,229` → `Fade(Screen)` 压到黑
  再淡回 → `Switch(1)` 写入 + `Jump{page:1}` 换页（再触发说的是"空宝箱"那句）→
  `Animate{Fade{to:0.3}}` 落在 `122,127,136`（0.3 不透明度在 sRGB↔linear 混合下的理论值，
  ⚠️ 探针读的是混合后的显示值，别拿它当"颜色错了"）。
- 井：一次两问的选项表（`choices` + `result: Int(5)` + `cursor_se` + `allow_cancel: false`），
  分支后 `PlaySe` + `Shake(wait:true)` + `Switch(2)`；第二分支那条
  `Custom(dialog_demo.transfer)` 由单测钉住（`runner.rs` 里两条告警路径：
  `这台事件机没装脚本后端` 与 `脚本后端不认这个名字` —— DoD-4"没做的能力要说话"的
  可执行证据）。

⚠️ 桌面这一腿只有"起窗口 + 无 panic + 三份表解出来"这一条弱证据，**没有肉眼复验过一整条
NPC 链**（这一轮没有可注入的窗口焦点）。要补的清单与 wasm 那份一一对应：面朝井边人按 Enter
→ 他走一趟 → 报 `3` 被拦回、报 `7` 放行 → HUD `answer=1` → 淡出后他从地图上消失。

[`VarStore`]: ../crates/sisf-event/src/vars.rs
[`Rng`]: ../crates/sisf-event/src/vars.rs
[`EventRunner::tick`]: ../crates/sisf-event/src/runner.rs
[`Hold`]: ../crates/sisf-event/src/runner.rs
[`RunnerStatus::Suspended{reason}`]: ../crates/sisf-event/src/runner.rs
[`OverBudget`]: ../crates/sisf-event/src/runner.rs
[`EngineBus`]: ../crates/sisf-event/src/bus.rs
[`EventRunner::waiting_input`]: ../crates/sisf-event/src/runner.rs
[`AudioPort`]: ../crates/sisf-event/src/runner.rs
[`UiPort`]: ../crates/sisf-event/src/runner.rs
[`Prompt`]: ../crates/sisf-event/src/runner.rs
[`Scene`]: ../crates/sisf-event/src/runner.rs
[`ScriptBackend`]: ../crates/sisf-event/src/script.rs
[`command.rs`]: ../crates/sisf-event/src/command.rs
[`AnimEffect`]: ../crates/sisf-event/src/command.rs
[`Expr`]: ../crates/sisf-event/src/command.rs
[`Cond`]: ../crates/sisf-event/src/command.rs
