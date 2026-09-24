// SISF 的 wasm 音频 pump（T7）。由 `pump_web.rs` 的
// `#[wasm_bindgen(module = "/src/audio_pump.js")]` 引入：`--target web` 下 wasm-bindgen 把
// 本文件**原样拷成 `web/pkg/snippets/sisf-audio-<hash>/src/audio_pump.js`**，生成的 `pkg/<例>.js`
// 再 `import` 它 —— 也就是页面要能取到那个相对路径（`cargo xtask web` 服务整个 `web/`，天然满足）。
// 两个实际后果：改了本文件必须重跑 `cargo xtask web`（wasm-bindgen 才会重新拷贝），
// 且**浏览器会按 URL 缓存它** ⇒ 复验要硬刷新，别拿旧胶水读出"已修"的假象。
//
// 清单里写的 `web/audio_worklet.js` 因此落地成**内联 Blob**（见 WORKLET_SRC）：AudioWorklet
// 只接受一个 URL，`URL.createObjectURL(new Blob([源码]))` 正好免开文件。
//
// ## WORKLET_SRC 里不能出现反引号
//
// 处理器源码是个模板字符串 ⇒ 它的注释里写一个反引号就会把整份胶水截断，而且 Rust 一侧毫无
// 察觉（胶水不参与编译检查）。防线只有一条：改完跑 `node --check crates/sisf-audio/src/audio_pump.js`。
//
// ## 缓冲深度的记账（`bufferedFrames`）
//
// 一帧**只允许被扣一次**。环是"过路"的（`sisfAudioPush` 一写就把成形的块搬走），所以稳态下
// 深度几乎全在 worklet 那边：
// - `workerPending` = 已经交给处理器、**还没播出去**的帧（= 真实的排队延迟）；
// - 处理器的回报里只统计"真正从队列里取走的帧"（`played`），取不到数据的帧算 `silent`（欠载），
//   不算消耗 —— 否则上下文刚 running、主线程还没送到时，静音会被当成已播扣掉，深度直接变负。
//
// 曾经踩过双扣：回报既从 `workerPending` 里减、又加进另一个 `workerConsumed`，而 `bufferedFrames`
// 报的是两者之差 ⇒ 每播一帧就扣两帧，深度一路走负、`sisfAudioFramesNeeded` 永远要满。
// 另一个坑是需求只按目标深度算 ⇒ 每个 rAF 至多补 1024 帧，主线程一慢于 ~47fps 就结构性欠载，
// 详见 `sisfAudioFramesNeeded` 的注释。
//
// ## 数据通路
//
// 主线程 `pump()` → `sisfAudioPush(交织f32视图)` → 本文件的环形缓冲 →
// - AudioWorklet：把环里的块 `postMessage` 给处理器（结构化克隆 = 拷贝；wasm 内存不可
//   transfer），处理器按 128 帧的 quant 播放并回报消耗量；
// - ScriptProcessor 回退：`onaudioprocess` 在主线程同步读环（这才是清单字面的"从 WASM 拉"）。
//
// 为什么 worklet 路径只能是"报需求 + 主线程按需求渲染"而不是同步 pull：AudioWorkletProcessor
// 跑在独立的音频线程，浏览器**不提供**从那个线程同步回调主线程 WASM 的机制（要做到得靠
// SharedArrayBuffer + Atomics，而那需要 COOP/COEP 跨源隔离与 `+atomics` 编译目标）。
//
// ## 自动解锁
//
// 浏览器的 autoplay 策略要求用户手势后才允许出声，页面隐藏时又该省电。这两件事都在 JS 里办
// （`sisf-app` 的 `page_hidden`/`log_info` 是 crate 私有的，本 crate 够不着），Rust 侧只需要
// 在 `pump()` 里读状态。引擎主动暂停（Android 挂起之类）走 `sisfAudioSetPaused`，与手势解锁
// 互不干扰：`pausedByEngine` 为真时，手势与 visibilitychange 都不许把上下文拉回 running。

const TARGET_FRAMES = 1024; // 目标缓冲深度（帧）≈ 21ms@48k
const RING_FRAMES = 4096; // 单次补给的容量上限（帧）；稳态深度仍是 TARGET，见 framesNeeded
const CAP_SAMPLES = RING_FRAMES * 2;
const MASK = CAP_SAMPLES - 1;
// `sisfAudioStats` 定长表的槽位号（Rust 侧 `pump_web.rs::stat` 同一份约定）。
const S_RATE = 0;
const S_BUFFERED = 1;
const S_UNDERRUN = 2;
const S_OVERFLOW = 3;
const S_BLOCKS = 4;
const S_QUANTA = 5;
const S_BASE_LATENCY = 6;
const S_OUTPUT_LATENCY = 7;
const S_TARGET = 8;
const S_TOTAL_LATENCY = 9;
// 下面两个只给控制台与 node 核算台看（Rust 侧的 PumpStats 不读）：把"投递深度"拆成
// 处理器可取的那部分与还在消息路上飞的那部分，见 inFlightFrames。
const S_WORKER_QUEUED = 10;
const S_IN_FLIGHT = 11;

const WORKLET_SRC = `
class SisfMixProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.chunks = [];
    this.off = 0;
    this.played = 0; // 真正从队列取走并输出的帧（静音不算）
    this.received = 0; // 已收到、也就是曾经可取的总帧数（queued = received - played）
    this.acked = 0;
    this.silent = 0;
    this.quanta = 0;
    this.port.onmessage = (e) => {
      const d = e.data;
      if (d.cmd === 'write') {
        this.chunks.push(d.chunk);
        this.received += d.chunk.length / 2;
      } else if (d.cmd === 'flush') {
        this.chunks.length = 0;
        this.off = 0;
        this.received = this.played; // 清空后"可取"归零，played 保持单调以便算增量
      }
    };
  }
  process(inputs, outputs) {
    const chans = outputs[0];
    if (!chans || chans.length === 0) return true;
    const left = chans[0];
    const right = chans.length > 1 ? chans[1] : chans[0];
    const n = left.length;
    let silent = 0;
    let played = 0;
    for (let i = 0; i < n; i++) {
      let l = 0.0, r = 0.0;
      let have = false;
      while (this.chunks.length > 0) {
        const c = this.chunks[0];
        if (this.off + 1 >= c.length) {
          this.chunks.shift();
          this.off = 0;
          continue;
        }
        l = c[this.off];
        r = c[this.off + 1];
        this.off += 2;
        have = true;
        break;
      }
      if (have) played++; else silent++;
      left[i] = l;
      right[i] = r;
    }
    this.played += played;
    this.silent += silent;
    this.quanta++;
    // 每个 quant 报一次（48k 下 375 条/秒，每条三个小整数 —— 结构化克隆这点量不构成负担）。
    // 回报间隔 = 主线程看到的深度的**陈旧上界**：workerPending 只在收到回报时才降，所以
    // 攒够 512 帧再报会让深度虚高 512 帧，60fps 下正好吃掉 framesNeeded 那 1/4 余量
    // （实测仍欠载）。取走与静音任一非零就报，纯静音期也必须让主线程知道。
    const took = this.played - this.acked;
    if (took > 0 || this.silent > 0) {
      // queued = 处理器此刻自己队里还剩多少帧（报的是发这条消息那一瞬的值，主线程看到它时
      // 最多少算了一个 quant 的消耗）。有了它，主线程才能把"已投递的深度"拆成
      // "可取的"与"还在消息路上飞的"，见主线程侧的 inFlightFrames。
      this.port.postMessage({ cmd: 'drain', frames: took, silent: this.silent,
                              quanta: this.quanta, queued: this.received - this.played });
      this.acked = this.played;
      this.silent = 0;
      this.quanta = 0;
    }
    return true;
  }
}
registerProcessor('sisf-mix', SisfMixProcessor);
`;

let ctx = null;
let kind = ''; // 'worklet' | 'script-processor' | '' = 还没创建
let initError = '';
let runtimeError = '';
let ring = new Float32Array(CAP_SAMPLES);
let head = 0; // 绝对样本游标（只增）
let tail = 0;
let port = null;
let workerPending = 0; // 已 postMessage、处理器还没取走的帧
let workerQueued = 0; // 处理器自己报的"队里还剩多少"（陈旧度 ≤ 一个 quant）
let consumedSinceQuery = 0; // 上次 `sisfAudioFramesNeeded` 之后被取走的帧
let catchUpFrames = 0; // 上一条里搬过来的"这一帧要播掉的量"，整个 tick 内保持一致
let workerUnderrun = 0;
let quanta = 0;
let blocks = 0;
let overflowSamples = 0;
let pausedByEngine = false;
let scriptNode = null;

function ringFilledSamples() {
  return head - tail;
}

function ringWrite(view) {
  const n = view.length;
  const free = CAP_SAMPLES - ringFilledSamples();
  const take = Math.min(n, free);
  if (take < n) overflowSamples += n - take;
  for (let i = 0; i < take; i++) {
    ring[head & MASK] = view[i];
    head++;
  }
  return take;
}

/// 从环里取 `frames` 帧填进左右声道；不足的部分补零并计 underrun。
function ringFillStereo(left, right, frames) {
  let short = 0;
  for (let i = 0; i < frames; i++) {
    if (ringFilledSamples() < 2) {
      left[i] = 0.0;
      right[i] = 0.0;
      short++;
      continue;
    }
    left[i] = ring[tail & MASK];
    tail++;
    right[i] = ring[tail & MASK];
    tail++;
  }
  return short;
}

function bufferedFrames() {
  return ringFilledSamples() / 2 + (kind === 'worklet' ? workerPending : 0);
}

/// 处理器**现在就能取**的帧数（不含还在消息路上的、也不含环里的 —— 那两类都要等下一次
/// `sisfAudioPush` 的批发才对它可见）。用它把 `bufferedFrames()` 拆开看，才能回答
/// "缺的那 384 帧到底是在途还是预算不够"，见 `docs/audio-web.md`。
function queuedFrames() {
  return kind === 'worklet' ? workerQueued : ringFilledSamples() / 2;
}

/// 已投递但对处理器还不可见的帧数。
function inFlightFrames() {
  return Math.max(0, Math.round(bufferedFrames() - queuedFrames()));
}

/// 处理器回报：`frames` 帧已被它取走播出 ⇒ 从在途量里扣掉（只扣这一次），并记进"这一帧要补"。
function applyWorkletStats(d) {
  workerPending -= Math.min(workerPending, d.frames);
  consumedSinceQuery += d.frames;
  // 老浏览器/老胶水可能没带 queued ⇒ 保守地当成"全部已投递的都可见"。
  workerQueued = typeof d.queued === 'number' ? d.queued : workerPending;
  workerUnderrun += d.silent;
  quanta += d.quanta;
}

function createScriptProcessor(c) {
  // 2048 = 浏览器默认 quant 粒度；0 输入通道、2 输出通道。
  scriptNode = c.createScriptProcessor(2048, 0, 2);
  scriptNode.onaudioprocess = (e) => {
    const chans = e.outputBuffer.numberOfChannels;
    const left = e.outputBuffer.getChannelData(0);
    const right = chans > 1 ? e.outputBuffer.getChannelData(1) : left;
    const short = ringFillStereo(left, right, left.length);
    // ScriptProcessor 是同步取的：取走的就是"这一帧要补的量"（静音不算，另计欠载）。
    consumedSinceQuery += left.length - short;
    if (short > 0) workerUnderrun += short;
    quanta++;
  };
  scriptNode.connect(c.destination);
  kind = 'script-processor';
}

function attachWorklet(c) {
  const url = URL.createObjectURL(new Blob([WORKLET_SRC], { type: 'application/javascript' }));
  return c.audioWorklet
    .addModule(url)
    .then(() => {
      URL.revokeObjectURL(url);
      const node = new AudioWorkletNode(c, 'sisf-mix', {
        numberOfInputs: 0,
        numberOfOutputs: 1,
        outputChannelCount: [2],
      });
      port = node.port;
      port.onmessage = (e) => {
        if (e.data && e.data.cmd === 'drain') applyWorkletStats(e.data);
      };
      node.connect(c.destination);
      // 附加完成前 ring 已经攒了主线程的出货；一次性搬进去，别让它们排在尾部不动。
      const backlog = Math.floor(ringFilledSamples() / 2);
      if (backlog > 0) {
        const chunk = new Float32Array(backlog * 2);
        ringFillInto(chunk, backlog);
        workerPending += backlog;
        port.postMessage({ cmd: 'write', chunk });
      }
      kind = 'worklet';
      return kind;
    })
    .catch((err) => {
      URL.revokeObjectURL(url);
      runtimeError = 'audioWorklet.addModule 失败: ' + err;
      createScriptProcessor(c);
      return kind;
    });
}

function applyAutoplayGuards(c) {
  const unlock = () => {
    if (pausedByEngine || !ctx || ctx.state === 'running' || ctx.state === 'closed') return;
    ctx.resume().catch((err) => {
      runtimeError = 'resume 被拒: ' + err;
    });
  };
  // capture 阶段 + 一次性：任何首次手势都够解锁，之后交给 visibilitychange。
  window.addEventListener('pointerdown', unlock, { capture: true });
  window.addEventListener('keydown', unlock, { capture: true });
  document.addEventListener('visibilitychange', () => {
    if (!ctx || ctx.state === 'closed' || pausedByEngine) return;
    const p = document.hidden ? ctx.suspend() : ctx.resume();
    if (p && p.catch) p.catch((err) => { runtimeError = 'visibility 切换失败: ' + err; });
  });
}

export function sisfAudioCreate() {
  if (ctx) return kind;
  try {
    const C = window.AudioContext || window.webkitAudioContext;
    if (!C) {
      initError = '这个浏览器没有 AudioContext';
      return '';
    }
    ctx = new C({ latencyHint: 'interactive' });
    applyAutoplayGuards(ctx);
    if (ctx.audioWorklet && typeof AudioWorkletNode !== 'undefined') {
      // addModule 是异步的：先把返回值当"已选定 worklet"报出去，真正失败时回退链会改写 kind。
      kind = 'worklet(pending)';
      attachWorklet(ctx).catch((err) => {
        runtimeError = 'worklet 初始化失败: ' + err;
        try {
          createScriptProcessor(ctx);
        } catch (e2) {
          initError = '两条通路都不可用: ' + e2;
        }
      });
    } else {
      createScriptProcessor(ctx);
    }
    return kind;
  } catch (err) {
    initError = '' + err;
    ctx = null;
    kind = '';
    return '';
  }
}

export function sisfAudioKind() {
  return kind;
}

export function sisfAudioSampleRate() {
  return ctx ? ctx.sampleRate : 0.0;
}

/// 还希望主线程渲多少帧。
///
/// 一次 `pump()` 里会反复问这个函数（每块 256 帧），所以需求必须写成**整个 tick 都一致的
/// 深度阈值**，不能是"第一次问时多要一块"—— 后者等于把追赶量限制成一块，实测照样欠载。
///
/// 阈值 = `max(TARGET_FRAMES, 一帧要播掉的量 + 1/4 余量)`。只按 `TARGET_FRAMES` 封顶是个
/// 帧率陷阱：每 tick 至多补到 1024 帧，而每 tick 要播掉 `rate / fps` 帧 ⇒ 主线程一慢于
/// `rate / TARGET` ≈ 47fps 就结构性补给不足（Chrome 软件渲染实测 ~42fps ⇒ 11% 的帧是静音，
/// 见 `docs/audio-web.md`）。60fps 下取的是较大者里的 `TARGET_FRAMES`，也就是**不拿延迟换保险**；
/// 掉帧时才临时加深，且受 `RING_FRAMES` 容量约束，之后自动收敛。
/// `consumedSinceQuery` 只在有新鲜消耗时刷新阈值；同一 tick 内后续调用看不到新消息
/// （同步的 wasm 调用链中间不投递任务），所以阈值在整个 tick 内保持稳定。
export function sisfAudioFramesNeeded() {
  if (!ctx) return 0.0;
  if (consumedSinceQuery > 0) {
    catchUpFrames = consumedSinceQuery;
    consumedSinceQuery = 0;
  }
  const floor = catchUpFrames + (catchUpFrames >> 2);
  const target = Math.max(TARGET_FRAMES, floor);
  const want = Math.max(0, target - bufferedFrames());
  const room = RING_FRAMES - ringFilledSamples() / 2;
  return Math.min(want, room);
}

/// 压入交织立体声 f32（长度必须是偶数）。返回被接受的帧数。
export function sisfAudioPush(view) {
  if (!ctx) return 0.0;
  const frames = (view.length / 2) | 0;
  const got = ringWrite(view.subarray(0, frames * 2));
  blocks++;
  if (kind === 'worklet' && port) {
    // 把环里成形的块整批发给处理器（拷贝，不 transfer —— wasm 内存不可 transfer）。
    const pending = Math.floor(got / 2);
    if (pending > 0) {
      const chunk = new Float32Array(pending * 2);
      ringFillInto(chunk, pending);
      workerPending += pending;
      port.postMessage({ cmd: 'write', chunk });
    }
  }
  return got / 2;
}

/// worklet 通路专用：把环里已有的 `frames` 帧搬进 `chunk`（并推进读标）。
function ringFillInto(chunk, frames) {
  for (let i = 0; i < frames * 2; i++) {
    chunk[i] = ring[tail & MASK];
    tail++;
  }
}

export function sisfAudioSetPaused(paused) {
  pausedByEngine = !!paused;
  if (!ctx) return '';
  const target = paused ? 'suspended' : 'running';
  if (ctx.state === target || ctx.state === 'closed') return '';
  const p = paused ? ctx.suspend() : ctx.resume();
  if (p && p.catch) {
    p.catch((err) => {
      runtimeError = 'setPaused 失败: ' + err;
    });
  }
  return '';
}

/// 定长数值表（槽位见文件头常量）+ `sisfAudioState()` 给文字状态。
/// 返回普通 Array 而不是 Float64Array：Rust 侧签名是 `Vec<f64>`，wasm-bindgen 对前者的
/// 转换路径最直（`getArrayFromMemory` 只认 Array 外形的对象）。
export function sisfAudioStats() {
  const out = new Array(16).fill(0);
  const rate = ctx ? ctx.sampleRate : 0.0;
  const buffered = bufferedFrames();
  out[S_RATE] = rate;
  out[S_BUFFERED] = buffered;
  out[S_UNDERRUN] = workerUnderrun;
  out[S_OVERFLOW] = overflowSamples;
  out[S_BLOCKS] = blocks;
  out[S_QUANTA] = quanta;
  out[S_BASE_LATENCY] = ctx && ctx.baseLatency ? ctx.baseLatency * 1000.0 : 0.0;
  out[S_OUTPUT_LATENCY] = ctx && ctx.outputLatency ? ctx.outputLatency * 1000.0 : 0.0;
  out[S_TARGET] = TARGET_FRAMES;
  out[S_WORKER_QUEUED] = queuedFrames();
  out[S_IN_FLIGHT] = inFlightFrames();
  out[S_TOTAL_LATENCY] = rate > 0
    ? (out[S_BASE_LATENCY] + out[S_OUTPUT_LATENCY] + (buffered * 1000.0) / rate)
    : 0.0;
  return out;
}

export function sisfAudioState() {
  const err = initError || runtimeError;
  if (!ctx) return err ? '未初始化: ' + err : '未初始化';
  const bits = ['state=' + ctx.state, 'kind=' + (kind || '-')];
  if (ctx.audioWorklet) bits.push('worklet=yes');
  else bits.push('worklet=no');
  if (ctx.baseLatency !== undefined) bits.push('baseLatency=' + ctx.baseLatency.toFixed(4) + 's');
  if (ctx.outputLatency !== undefined) {
    bits.push('outputLatency=' + ctx.outputLatency.toFixed(4) + 's');
  }
  bits.push('buffered=' + bufferedFrames() + 'f');
  // 把 buffered 拆开：queued = 处理器现在就能取的，inflight = 已投递但还看不见的。
  bits.push('queued=' + Math.round(queuedFrames()) + 'f');
  bits.push('inflight=' + inFlightFrames() + 'f');
  bits.push('underrun=' + workerUnderrun + 'f');
  bits.push('overflow=' + overflowSamples + 's');
  if (err) bits.push('err=' + err);
  return bits.join(' ');
}

/// 丢弃所有已缓冲样本（重建/挂起后用，避免恢复瞬间把陈旧音频倒出来）。
export function sisfAudioFlush() {
  tail = head;
  if (port) port.postMessage({ cmd: 'flush' });
  workerPending = 0;
  workerQueued = 0;
  consumedSinceQuery = 0;
  catchUpFrames = 0;
  return 0.0;
}
