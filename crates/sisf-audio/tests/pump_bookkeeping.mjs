// wasm 音频胶水的**记账**核算台（不是浏览器测试：不开 AudioContext，不碰 GPU）。
//
//     node crates/sisf-audio/tests/pump_bookkeeping.mjs
//
// 为什么需要它：`bufferedFrames()` 那个数是 pump 的唯一反馈量，报错了不会崩、不会告警，只会
// 表现成"听一下才发现的"延迟增长或稳态欠载（见 `docs/audio-web.md` 的两节）。这类账 Rust 单测
// 碰不到，而每次都把手动开 Chrome 复验当闸门又慢又依赖人。于是把胶水连同 `WORKLET_SRC` 里的
// **真实处理器源码**一起装进 node，跑一条 48k 虚拟时间线，量四件事：
//
// 1. 报出来的深度 == 处理器那边真实排队的帧数（记账不双扣、不把静音算消耗）；
// 2. 深度不为负；
// 3. 稳态不欠载（预热之后 underrun 不再涨），且 60fps 下峰值深度就是目标 1024 帧 = 21.3ms
//    （不拿延迟换保险）；
// 4. 拆账成立：处理器自报的 `queued` == 它此刻的真实排队量，且 `buffered - 真实 == inflight`。
//    这一条是"缺的那几百帧到底是在途还是预算不够"的判据（浏览器侧看同一组数：控制台那行的
//    `queued=`/`inflight=`）。
//
// 消息投递默认比浏览器**更苛刻**：drain 回报与 write 都是同步送达的，不给消息留队列。
// 所以这里绿不代表浏览器一定绿，最终判据仍是控制台那行 `buffered=`/`underrun=`；
// 它绿只是把"明显的账错"挡在人手复验之前。永久化的家应该是 `xtask wasm-test`（todo.md 后续项）。
//
// 探针（默认关，开着不进判据）：`--transit N` 把"主线程 → 处理器"的 write 投递推迟 N 个 quant，
// 用来量 `bufferedFrames()` 里"已 postMessage 但处理器还没收到"那部分在途帧。N=1 时 60fps 仍稳，
// 50fps 起低水位归零、结构性欠载 ⇒ "把在途算成可用深度"这个建模误差是真的。浏览器上它占多大比重
// 已经用同一组观测量测掉了：≈3 个 quant（8ms），正好等于长帧上那个恒定的缺口，
// 见 `docs/audio-web.md`「残余的每 ~384 帧」。
//
// 退出码非 0 = 判据不过。

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join, resolve } from 'node:path';

const HERE = dirname(fileURLToPath(import.meta.url));
const GLUE = resolve(HERE, '../src/audio_pump.js');
const SRC = readFileSync(GLUE, 'utf8');

// ---- 从胶水里把真实的 worklet 处理器类取出来（不复制第二份源码，避免两边各改各的） ----

const tpl = SRC.match(/const WORKLET_SRC = `([\s\S]*?)\n`;/);
if (!tpl) fail('取不出 WORKLET_SRC —— 模板字符串被截断了吗？（注释里不能写反引号）');
let Processor = null;
globalThis.AudioWorkletProcessor = class {
  constructor() {
    this.port = globalThis.__pendingPort;
  }
};
globalThis.registerProcessor = (_name, cls) => {
  Processor = cls;
};
new Function(tpl[1])();
if (!Processor) fail('registerProcessor 没被调用');

// ---- 最小的 WebAudio 替身 ----

const RATE = 48_000;
const QUANT = 128;
const CAP = 4096; // 与 glues 里的 RING_FRAMES 对齐（只影响"能补多少"，不参与判据)

/// 主线程 → 处理器的消息延迟（单位：quant）。0 = 同步投递（默认，判据按它写）。
/// >0 用来复现浏览器上那个"报出来的深度比处理器真实可取的深度多一截"的现象：
/// worklet 在独立线程，`postMessage` 出去的数据要等一次消息投递才对它可见，而
/// `workerPending` 在 post 的那一刻就把它算进 `bufferedFrames()` ⇒ 报出的深度含"在途"。
/// 见 `docs/audio-web.md`「残余的每 ~384 帧」。
const TRANSIT = (() => {
  const i = process.argv.indexOf('--transit');
  return i >= 0 ? Math.max(0, parseInt(process.argv[i + 1], 10) || 0) : 0;
})();

const world = { proc: null, quantum: 0, inFlight: [] };

function makePort() {
  return {
    onmessage: null,
    peer: null,
    // 同步投递：比浏览器更急，不给消息留队列。
    postMessage(data) {
      if (TRANSIT > 0 && this.deliverLater) {
        world.inFlight.push({ at: world.quantum + TRANSIT, port: this.peer, data });
        return;
      }
      this.peer?.onmessage?.({ data });
    },
  };
}

/// 把到期的消息送进对端（每个 quant 之前调一次）。
function deliverDue() {
  if (!world.inFlight.length) return;
  const due = [];
  world.inFlight = world.inFlight.filter((m) => (m.at <= world.quantum ? (due.push(m), false) : true));
  for (const m of due) m.port.onmessage?.({ data: m.data });
}

globalThis.AudioWorkletNode = class {
  constructor() {
    const main = makePort();
    const proc = makePort();
    main.peer = proc;
    proc.peer = main;
    // 只有"主线程 → 处理器"这一个方向按 quant 排队投递（TRANSIT>0 时）。
    main.deliverLater = TRANSIT > 0;
    globalThis.__pendingPort = proc;
    world.proc = new Processor();
    this.port = main;
  }
  connect() {}
};
globalThis.AudioContext = class {
  constructor() {
    this.sampleRate = RATE;
    this.state = 'running';
    this.baseLatency = 0.01;
    this.outputLatency = 0.04;
    this.audioWorklet = { addModule: () => Promise.resolve() };
  }
  createScriptProcessor() {
    throw new Error('这份测试只走 worklet 通路');
  }
  suspend() {
    return Promise.resolve();
  }
  resume() {
    return Promise.resolve();
  }
};
globalThis.window = { AudioContext: globalThis.AudioContext, addEventListener() {} };
globalThis.document = { addEventListener() {}, hidden: false };
globalThis.URL = { createObjectURL: () => 'blob:x', revokeObjectURL() {} };
globalThis.Blob = class {};

// ---- 虚拟时间线 ----

/// 处理器此刻真实排队的帧数（不看任何记账变量，作为独立参照）。
function realQueued() {
  let q = 0;
  for (const c of world.proc.chunks) q += (c.length - 1) / 2;
  return q - (world.proc.off - 1) / 2;
}

function fail(msg) {
  console.error(`FAIL  ${msg}`);
  process.exit(1);
}

const verbose = process.argv.includes('--verbose');
// 每个帧率各跑一遍：预热 1s 之后，报的深度必须与真实排队量对得上、underrun 必须不再涨。
// 采样点很讲究：**先让处理器跑完一个 quant，再在 tick 边界上取数**（此刻 ack 与真实排队同属一个
// 瞬间，两条误差才可比），然后才模拟这一帧的补给。于是同时量到两个数：
//
// - 峰值深度（补给之后）—— 判据"60fps 就是目标 1024 帧 = 21.3ms，不拿延迟换保险"看它；
// - 低水位（补给之前，处理器手上真有货的量）—— 出事就出在它身上，只打印不设判据。
//
// 另外两条恒等式：报的 `queued` 必须等于处理器真实排队量（误差 ≤ 一个 quant），且
// `buffered - 真实 == 在途`（拆账要对得上）。
for (const fps of [60, 50, 42, 30, 20]) {
  world.quantum = 0;
  world.inFlight = [];
  const mod = await import('file://' + GLUE.replace(/\\/g, '/') + '?fps=' + fps);
  mod.sisfAudioCreate();
  await Promise.resolve();
  await Promise.resolve(); // attachWorklet 的 backlog 搬迁
  const tick = Math.round(RATE / fps);
  const out = [new Float32Array(QUANT), new Float32Array(QUANT)];
  let frame = 0;
  let nextTick = 0;
  let warmUnderrun = null;
  let minBuf = Infinity;
  let maxBuf = -Infinity;
  let minLow = Infinity;
  let maxLag = 0;
  let maxQueuedLag = 0;
  let maxSplit = 0;
  const name = TRANSIT > 0 ? `fps=${fps} transit=${TRANSIT}q` : `fps=${fps}`;
  while (frame < RATE * 5) {
    // 1) 处理器先推进一个 quant（到期的 write 这才可见），ack 是同步回来的。
    out[0].fill(0);
    out[1].fill(0);
    deliverDue();
    world.proc.process([], [out]);
    world.quantum++;
    frame += QUANT;
    // 2) 到 tick 边界：先按"刚 process 完、主线程还没补"的口径记账，再模拟那一帧的补给。
    //    这个时点上处理器报的 queued 就是它此刻的真实排队量（ack 与判据同属一个 quant），
    //    所以两条误差才可比 —— 在补给之后再量会把"刚 post 出去"的帧错算成记账误差。
    if (frame >= nextTick) {
      nextTick += tick;
      const s = mod.sisfAudioStats();
      const real = realQueued();
      if (frame >= RATE) {
        if (warmUnderrun === null) warmUnderrun = s[2];
        maxLag = Math.max(maxLag, Math.abs(s[1] - real));
        maxQueuedLag = Math.max(maxQueuedLag, Math.abs(s[10] - real));
        maxSplit = Math.max(maxSplit, Math.abs(s[1] - real - s[11]));
        // 低水位 = 这一 tick 补给**之前**处理器手上的量（真出事就出在它身上），按"报的
        // 可取深度"取，免得把在途的帧当成已经在手上。
        minLow = Math.min(minLow, s[10]);
      }
      if (verbose) {
        console.log(
          `  ${name} t=${(frame / RATE).toFixed(2)}s buffered=${s[1]} queued=${s[10]}` +
            ` inflight=${s[11]} 真实=${real} underrun=${s[2]}`,
        );
      }
      for (let block = 0; block < CAP / 256; block++) {
        const frames = Math.min(Math.floor(mod.sisfAudioFramesNeeded()), 256);
        if (frames === 0) break;
        const got = mod.sisfAudioPush(new Float32Array(frames * 2));
        if (got < frames) break;
      }
      // 补给之后的峰值深度才是"贴着目标"那个判据要量的东西（补给之前的是低水位，见上面的 s）。
      if (frame >= RATE) {
        const peak = mod.sisfAudioStats()[1];
        minBuf = Math.min(minBuf, peak);
        maxBuf = Math.max(maxBuf, peak);
      }
    }
  }
  const grew = mod.sisfAudioStats()[2] - warmUnderrun;
  if (minBuf < 0) fail(`${name}: buffered 出现负值 ${minBuf} ⇒ 记账又双扣了`);
  // 处理器自报的"可取深度"必须等于它此刻的真实排队量 —— 与投递延迟无关，这条是拆账的地基。
  if (maxQueuedLag > QUANT) {
    fail(`${name}: 报的 queued 与真实排队量最大差 ${maxQueuedLag} 帧 > 一个 quant`);
  }
  // buffered 与真实之差必须就是"在途"那一项（拆账要对得上）。
  if (maxSplit > QUANT) {
    fail(`${name}: buffered - 真实 = 在途 这条恒等式最大偏差 ${maxSplit} 帧 > 一个 quant`);
  }
  if (TRANSIT === 0 && maxLag > QUANT) {
    fail(`${name}: 报的深度与真实排队量最大差 ${maxLag} 帧 > 一个 quant ⇒ 有一处帧被扣了两次`);
  }
  if (grew > 0) {
    fail(`${name}: 预热后 underrun 又涨了 ${grew} 帧，低水位 ${minLow}f ⇒ 投递一慢就没货（见 docs/audio-web.md 的在途账）`);
  }
  // 60fps 必须恰好是目标深度：这一条钉住"不拿延迟换保险"。
  if (TRANSIT === 0 && fps === 60 && minBuf > 1024 + QUANT) {
    fail(`fps=60: 稳态深度 ${minBuf} 明显高于目标 1024 ⇒ 需求阈值把延迟白抬了`);
  }
  console.log(
    `ok  ${name}  峰值深度 ${minBuf}~${maxBuf}f(${((minBuf * 1000) / RATE).toFixed(1)}~${((maxBuf * 1000) / RATE).toFixed(1)}ms)` +
      ` 低水位 ${minLow}f  queued 误差 ${maxQueuedLag}f  在途账误差 ${maxSplit}f  underrun +${grew}`,
  );
}
