# T7 附录：wasm 音频 pump（WebAudio）

`crates/sisf-audio/src/pump_web.rs` + `crates/sisf-audio/src/audio_pump.js`。Rust 侧只管三件事：
创建时问一次采样率、每次 `pump()` 问"还差多少帧"、把渲好的交织 f32 推过去。其余（通路选择、
autoplay 解锁、页面隐藏省电、统计）都在 JS 里 —— 因为 `sisf-app` 的 `page_hidden`/`log_info`
是 crate 私有的，而且 JS 本来就更早知道这两件事。

## 通路：worklet 与 ScriptProcessor 各自的语义

```
主线程 pump() ──sisfAudioPush(交织 f32 视图)──▶ JS 环形缓冲(4096f，过路)
                                                 ├─ worklet：按 quant 播放 + 每 quant 回报取走/静音量
                                                 └─ ScriptProcessor：主线程回调，同步读环
```

- **AudioWorklet（首选）**：处理器跑在独立音频线程，按 128 帧的 quant 播放，`postMessage` 把
  数据搬过去（结构化克隆 = 拷贝；wasm 内存不可 transfer）。它**会**回报自己**从队列取走了多少帧**
  （以及其中多少是静音），主线程据此决定还补多少 —— 见下面"缓冲深度的记账"。
- **ScriptProcessor 回退**：`addModule` 失败、或根本没有 `audioWorklet`（老 Safari、某些
  WebView）时启用。它的回调在主线程，因此**这条才是清单字面的"从 WASM 拉"** —— 同步读 JS 环。
  代价是主线程一卡就当场欠载，且它已在规范里废弃。
- 两条都不行时 `sisfAudioCreate()` 返回 `''`，`describe()` 报 `initError`，混音仍照常推进
  （与 native 的"无设备不是错误"同一套语义）。

**为什么 worklet 不能同步 pull（清单字面形状）**：从音频线程回调主线程 WASM 需要
SharedArrayBuffer + Atomics，而那要求页面开 COOP/COEP 跨源隔离与 `+atomics` 编译目标 ——
本仓库的 wasm 构建两样都没开（`xtask web` 是 plain `wasm-pack --target web`）。于是 worklet
通路退化成"需求上报 + 主线程按需求渲染"。这是与清单的第三处字面差异，代码与本文档都记着。

`web/audio_worklet.js` 这个文件名因此落地成**内联 Blob**：`audio_pump.js` 里的 `WORKLET_SRC`
字符串 + `URL.createObjectURL(new Blob([…]))`（AudioWorklet 只接受 URL）。为什么 worklet 源码要
内联而胶水不用：`#[wasm_bindgen(module = "/src/audio_pump.js")]` 会把胶水**拷成
`web/pkg/snippets/sisf-audio-<hash>/src/audio_pump.js`**，由生成的 `pkg/<例>.js` 用相对路径
`import` —— 静态服务只要连着 `snippets/` 一起给就没有"pkg 有、页面 404"那种最难查的失败形态
（`cargo xtask web` 服务整个 `web/`，正好覆盖）。worklet 源码则不同：`addModule()` 只吃 URL，
胶水已经在了，再为它单开一个文件不值得。

## 延迟口径

Rust 侧 `PumpStats::total_latency_ms` = `baseLatency + outputLatency + buffered_frames / rate`，
三段分别是什么：

| 项 | 来源 | 典型值（`latencyHint: 'interactive'`） |
| --- | --- | --- |
| `baseLatency` | `AudioContext.baseLatency`（渲染 quantum 粒度） | 128 帧 ≈ 2.7ms@48k；Chrome/Windows 常见 10ms |
| `outputLatency` | `AudioContext.outputLatency`（硬件缓冲） | 0～20ms，设备相关；Firefox 部分平台报 `undefined`（当 0 处理） |
| `buffered/rate` | 我们自己的排队深度，目标 `TARGET_FRAMES = 1024` | ≈ **21.3ms@48k**，是这三项里唯一由引擎决定的 |

清单目标（≤~21ms@1024）针对的就是第三项：缓冲深度是引擎唯一能自己收放的旋钮，前两项归浏览器与
声卡。

## 需求闭环：阈值 = `max(目标深度, 一帧要播掉的量 + 1/4)`

`sisfAudioFramesNeeded()` 决定这个旋钮，它必须同时满足两件事：**稳态贴着 `TARGET_FRAMES`**（延迟），
**慢帧时补得上一帧的消耗**（不欠载）。只写 `TARGET_FRAMES - bufferedFrames()` 满足不了第二件 ——
每个 rAF 至多把深度补到 1024 帧，而每个 rAF 要播掉 `rate / fps` 帧 ⇒ 主线程一慢于
`rate / TARGET` ≈ **47fps** 就结构性补给不足，稳态永久"缺一点"。这不是理论值：Chrome 在软件
GPU（WARP）下跑这个示例约 42fps，实测每秒涨 ~5000 帧静音，且 `buffered` 恒显示 512 —— 看起来
"还有半缓冲"，实际每一帧都在补不上。改法（native 侧同一个 `refill_samples`）：

```
catchUp = 上次报需求以来处理器真正取走的帧            // 静音不算，见下节
target  = max(TARGET_FRAMES, catchUp + catchUp / 4)   // 60fps: 1024 ⇒ 21ms，不掉帧就不加延迟
want    = max(0, target - buffered) 且不超过环剩余空间
```

两个必须留意的实现细节，都是踩过才知道的：

1. **它得是"阈值"，不能是"第一次问时多要一块"**。Rust 侧一次 `pump()` 会反复问这个函数
   （每 `BLOCK_FRAMES` 一块），只有第一次问看得到新鲜的消耗量；把追赶量加成一项加数，等于
   把它限制成一块 256 帧，实测照样欠载。所以 `consumedSinceQuery` 只在有新鲜消耗时刷新
   `catchUpFrames`，整个 tick 内阈值保持稳定。
2. **回报间隔 = 深度的陈旧上界**。`workerPending` 只在收到 drain 回报时才降，攒够 512 帧再报
   会让深度**虚高** 512 帧，正好吃掉上面那 1/4 余量（60fps 下 catchUp=800 ⇒ 余量 200 < 512）。
   所以处理器改成**每个 quant 报一次**（48k 下 375 条/秒、每条三个小整数），陈旧上界压到 128 帧。

## 缓冲深度的记账：一帧只许扣一次

`sisfAudioFramesNeeded()` 的闭环只看一个数 —— `bufferedFrames()`，也就是"交出去了但还没播出去"
的帧数。环是**过路**的（`sisfAudioPush` 一写就把成形的块搬去 worklet），所以稳态下这个数几乎
全等于处理器那边排队的帧数，它同时就是**真实延迟**：报小了多渲（延迟一路涨），报大了少渲（欠载）。

第一版在这里踩了**双扣**：处理器的回报既从 `workerPending` 里减一次，又加进另一个
`workerConsumed`，而 `bufferedFrames()` 报的是 `workerPending - workerConsumed` ⇒ 每播一帧扣两帧。
更糟的是处理器把**静音也算消耗**（`this.consumed += n`，取不到数据的那帧照样涨），于是上下文刚
running、主线程还没送到时，纯静音就把深度做负了。两个错误叠起来的形态很有欺骗性：
`underrun` 冻结在启动期的一个值，`buffered` 看着是"满"的，实际排队长度一路涨（延迟跟着涨，
且永远不收敛）—— 它反过来"治好"了下一节的慢帧欠载，所以第一版在浏览器里听着是连续的。修法是
砍掉第二个计数器，并且让处理器分开记两件事：

- `played` —— **真正从队列取走**的帧，回报它，主线程从 `workerPending` 里扣它（只扣这一次）；
- `silent` —— 取不到数据而输出的静音帧，回报它，只进 `underrun`，不进消耗。

取不到数据时 `played` 不涨，所以回报条件不能只挂在它上面（静音非零也报），否则全静音期间
主线程完全收不到消息、以为一切正常。

## 怎么测（手测步骤）

```
cargo xtask web audio_demo
python -m http.server 8080 -d web        # 必须 http://，file:// 下 wasm module 与 fetch 都不给
```

打开 `http://127.0.0.1:8080/?ex=audio_demo`，**点一下页面**（autoplay 解锁：首次
`pointerdown`/`keydown` 触发 `ctx.resume()`），然后看 Console。`examples/audio_demo.rs` 每秒打
一行，其中 `describe()`（= `sisfAudioState()`）那段就是本页通路的状态：

```
state=running kind=worklet worklet=yes baseLatency=0.0100s outputLatency=0.0400s buffered=1024f underrun=0f overflow=0s
```

判据（与 native 同一条）：稳态播放时 `underrun` 不涨、`kind` 是 `worklet`（若为
`script-processor` 要查 `addModule` 为什么失败）、`state` 在点击后变 `running`、
**`buffered` 不为负**（为负就是上面那笔账又记岔了）。
切到别的标签页应静音（`visibilitychange` → `ctx.suspend()`），切回来继续；
`?secs=30` 可以让它跑久一点。GPU 那边若报"没有可用的 adapter"就再加 `&wgl`（WebGL2 回退）。

### 实测记录

| 环境 | 通路 | baseLatency | outputLatency | 排队 | 合计 | underrun |
| --- | --- | --- | --- | --- | --- | --- |
| Chrome / Windows，`?ex=audio_demo&wgl`（软件 GPU WARP），修账前 | worklet | 0.0100s | 0.0400s | **不可信**（双扣，见上节） | — | 11392f（t<1s 一次性，之后 68s 全程不涨） |
| 同上，修账后（t≈85fps 稳态） | worklet | 0.0100s | 0.0320s | 1024~1280f = **21.3~26.7ms** | **68.7ms**（= 10 + 32 + 26.7） | 稳态每 tick 增量为 0；残余 ~384f/s（见下节，逐条对得上长帧） |

> **"排队"列就是引擎唯一可控的那一段**：合计 68.7ms 里有 42ms 是 Chrome/这块声卡的
> `baseLatency + outputLatency`（`outputLatency` 还会随设备在 32~40ms 之间跳），与引擎无关。
> 清单目标 ≤~21ms@1024 指的是可控那一段，修账后实测贴着 1024（=21.3ms）走，`buffered` 全程不为负。
>
> 这一行是直接从页面里取的：动态 import 胶水模块（与 bundle 同一个 URL ⇒ 同一个实例）后
> `sisfAudioStats()`，每个 rAF 采一次，跑 8s。取数脚本见下节的"怎么复现"。

> 表里第一行是**真实控制台读数**，但它是在双扣那版上取的：`baseLatency`/`outputLatency`/通路/
> 解锁/欠载只涨一次这几项照旧有效，`buffered` 那一列则被记账错误污染 ⇒ 合计延迟要等修账后的
> 复现读数（DoD-1：不拿推算填表）。
>
> 那 11392 帧（≈237ms）启动静音**不是 pump 的锅**：首次点击后 `examples/audio_demo.rs` 在主线程
> 一次性解码四份 fixture（debug 构建下约 90ms/份，见 `docs/audio.md` 的"两个真坑"），pump 拿不到
> CPU 就只能输出静音。所以它是**示例**的启动代价，不是 pump 的稳态行为 —— 稳态判据始终是
> "启动那一次之后 `underrun` 不再涨"。
>
> 记账这一段的可复现证据（不开浏览器、不碰 GPU）：
>
> ```
> node crates/sisf-audio/tests/pump_bookkeeping.mjs
> ```
>
> 它把胶水**连同 `WORKLET_SRC` 里的真实处理器源码**一起装进 node（不是复制一份模型），跑 48k
> 虚拟时间线，判据是"报的深度 == 处理器真实排队量（误差 ≤ 一个 quant）+ 深度不为负 + 预热后
> `underrun` 不再涨 + 60fps 下深度就是目标 1024 帧"。当前输出：
>
> ```
> ok  fps=60  深度 1024~1120f(21.3~23.3ms)  记账最大误差 2f    预热后 underrun +0
> ok  fps=50  深度 1120~1280f(23.3~26.7ms)  记账最大误差 2.5f  预热后 underrun +0
> ok  fps=42  深度 1280~1440f(26.7~30.0ms)  记账最大误差 3.5f  预热后 underrun +0
> ok  fps=30  深度 1920~2080f(40.0~43.3ms)  记账最大误差 4.5f  预热后 underrun +0
> ok  fps=20  深度 2880~3040f(60.0~63.3ms)  记账最大误差 6.5f  预热后 underrun +0
> ```
>
> 反例也验过：把修账前的胶水放到同一个位置跑，第一行就
> `FAIL fps=60: 报的深度与真实排队量最大差 238507.5 帧 > 一个 quant`。
> 消息投递比浏览器更急（这里是同步的），所以它绿不等于浏览器绿 —— 最终判据仍是控制台那行，
> 但这个把"明显的账错"挡在了人手复验之前。永久的家应该是 `xtask wasm-test`（todo.md 后续项）。
>
> 另一条防线：`node --check crates/sisf-audio/src/audio_pump.js`（`WORKLET_SRC` 是模板字符串，
> 注释里写反引号会当场截断它，而 cargo 一侧毫无感觉 —— 修账时真踩过两次）。

### 残余的 ~384 帧/秒：是主线程的长尾，不是账错

修账后的浏览器复验并没有把 `underrun` 压到绝对 0，但把它**定性**清楚了。取数办法（不用手抄控制台，
直接从页面里问胶水要数）：

```js
// DevTools Console：与 bundle 同 URL ⇒ 拿到的是同一个模块实例
const mod = await import('/pkg/snippets/<hash>/src/audio_pump.js');
const st = () => mod.sisfAudioStats();          // [rate, buffered, underrun, overflow, blocks, quanta, ...]
let prev = st(), t0 = performance.now(), hits = [];
for (;;) { await new Promise(r => requestAnimationFrame(r));
  const t = performance.now(), s = st();
  if (s[2] > prev[2]) hits.push([+(t - t0).toFixed(1), s[2] - prev[2], s[1]]);
  prev = s; t0 = t; }
```

8 秒窗口的实测（Chrome / Windows / WARP，页面自身 ~85fps）：

| 量 | 读数 |
| --- | --- |
| tick 数 / 帧时长 p50 / p90 / p99 / max | 681 / 12.0 / 15.0 / 25.0 / **28.0** ms |
| 时长 >20ms 的 tick | 24 |
| 出现 `underrun` 增量的 tick | **8**（增量都是 384f） |
| 时长 ≤20ms 的 tick 的增量 | **0**（673 个 tick 全零） |
| 增量时刻的报出深度 | 1280f |

对得上的算术：`TARGET_FRAMES = 1024` 就是 **21.3ms 的预算**，一个 tick 只要不超过它就不会缺；
超过就缺 `dt − 预算` 那点量，再按 128 帧的 quant 向上取整 ⇒ 21.6~28ms 的长帧正好各缺
**384 帧 = 3 个 quant = 8ms**。而 681 个 tick 里只有 8 个跨过这条线 —— 报出来的深度全程贴着目标、
不为负、与真实排队量一致（node 核算台那两条判据在浏览器里同样成立），所以这是**主线程抖动撞预算**，
不是第 12 条那种账目错误。

要不要吃掉这条尾巴，是一个**拿延迟换保险**的定价问题，不是 bug 修复：`TARGET_FRAMES` 抬到 1536
（32ms）就能盖住这里的 p99=25ms，可控延迟从 21.3ms 变 32ms（合计那列再加 ~11ms）。清单目标写的是
"≤~21ms@1024"，所以本阶段**保留 1024**、把长尾如实记在这里。native 侧不需要这么纠结：cpal 的
拉数线程不会像浏览器主线程这样抖（实测 13s `underrun+0`，见 `docs/audio.md`）。

## 挂起与恢复

- **页面隐藏**：JS 直接 `ctx.suspend()`，Rust 不参与（省电要在下一帧之前就生效）。
- **引擎主动挂起**（Android `Lifecycle::Suspended`、应用自己调）：`AudioBus::suspend()` →
  `sisfAudioSetPaused(true)`，并顺手 `sisfAudioFlush()` 丢掉 JS 侧存货 —— 否则恢复瞬间会先把
  陈旧缓冲倒出来。`pausedByEngine` 为真时，手势与 `visibilitychange` 都**不许**把上下文拉回
  `running`，两套路互不干扰；恢复走 `resume_all()`。
- **混音侧的冻结**（位置不动、fade 停在原处）与设备侧无关，由 `Mixer::suspend()` 负责，
  所以没有 WebAudio 的端上也一样能测（`bus::tests::suspend_freezes_and_resume_continues`）。

## wasm 专属的三条约束

1. **`eprintln!` 在浏览器里看不见**。所有诊断走 `platform::log_info`（→ `console.log`）。
   打不开控制台就什么都看不到，所以 demo 把同样的信息也画在屏幕上（四通道进度条 + 播放头 +
   欠载灯）。
2. **`&[f32]` 进 import 参数是零拷贝视图**（指向 wasm 内存的 Float32Array）。它只在"胶水绝不
   回调 Rust"的前提下安全 —— 回调会让内存增长、视图失效。哪天 `audio_pump.js` 要回调 Rust，
   这里必须先 `Float32Array.from(slice)` 拷一份。
3. **首帧素材还在飞**。wasm 的 `fetch` 是异步的，示例每帧试读 VFS 直到齐（`Sources::pump()`），
   音频 fixture 已进 `web/assets/manifest.txt`；**SoundFont 故意不进仓**，所以 `M` 那一 cue 在
   浏览器里必然报 `MissingSoundBank`，那是预期行为不是回归。
