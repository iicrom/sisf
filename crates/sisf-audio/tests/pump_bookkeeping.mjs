// wasm 音频胶水的**记账**核算台（不是浏览器测试：不开 AudioContext，不碰 GPU）。
//
//     node crates/sisf-audio/tests/pump_bookkeeping.mjs
//
// 为什么需要它：`bufferedFrames()` 那个数是 pump 的唯一反馈量，报错了不会崩、不会告警，只会
// 表现成"听一下才发现的"延迟增长或稳态欠载（见 `docs/audio-web.md` 的两节）。这类账 Rust 单测
// 碰不到，而每次都把手动开 Chrome 复验当闸门又慢又依赖人。于是把胶水连同 `WORKLET_SRC` 里的
// **真实处理器源码**一起装进 node，跑一条 48k 虚拟时间线，量三件事：
//
// 1. 报出来的深度 == 处理器那边真实排队的帧数（记账不双扣、不把静音算消耗）；
// 2. 深度不为负；
// 3. 稳态不欠载（预热之后 underrun 不再涨），且 60fps 下深度就是目标 1024 帧 = 21.3ms
//    （不拿延迟换保险）。
//
// 消息投递默认比浏览器**更苛刻**：drain 回报与 write 都是同步送达的，不给消息留队列。
// 所以这里绿不代表浏览器一定绿，最终判据仍是控制台那行 `buffered=`/`underrun=`；
// 它绿只是把"明显的账错"挡在人手复验之前。永久化的家应该是 `xtask wasm-test`（todo.md 后续项）。
//
// 探针（默认关，开着不进判据）：`--transit N` 把"主线程 → 处理器"的 write 投递推迟 N 个 quant，
// 用来量 `bufferedFrames()` 里"已 postMessage 但处理器还没收到"那部分在途帧。N=1 时 60fps 仍稳、
// 50fps 起结构性欠载（报的深度比可取的多 ~900f）—— 也就是说"把在途算成可用"这个建模误差是真的，
// 但它在浏览器上占多大比重还没测（见 `docs/audio-web.md`「残余的 ~384 帧/秒」）。
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
/// 见 `docs/audio-web.md`「残余的 ~384 帧/秒」。
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
// 每个帧率各跑一遍：预热 1s 之后，深度必须贴住、underrun 必须不再涨。
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
  let maxLag = 0;
  while (frame < RATE * 5) {
    if (frame >= nextTick) {
      nextTick += tick;
      for (let block = 0; block < CAP / 256; block++) {
        const frames = Math.min(Math.floor(mod.sisfAudioFramesNeeded()), 256);
        if (frames === 0) break;
        const got = mod.sisfAudioPush(new Float32Array(frames * 2));
        if (got < frames) break;
      }
      const s = mod.sisfAudioStats();
      if (frame >= RATE) {
        // 预热之后：报的深度与真实排队量的差、深度范围、欠载增量都受判据约束。
        if (warmUnderrun === null) warmUnderrun = s[2];
        maxLag = Math.max(maxLag, Math.abs(s[1] - realQueued()));
        minBuf = Math.min(minBuf, s[1]);
        maxBuf = Math.max(maxBuf, s[1]);
      }
      if (verbose) {
        console.log(
          `  fps=${fps} t=${(frame / RATE).toFixed(2)}s buffered=${s[1]} 真实=${realQueued()} underrun=${s[2]}`,
        );
      }
    }
    out[0].fill(0);
    out[1].fill(0);
    deliverDue(); // 到期的 write 这才对处理器可见
    world.proc.process([], [out]);
    world.quantum++;
    frame += QUANT;
  }
  const grew = mod.sisfAudioStats()[2] - warmUnderrun;
  const name = TRANSIT > 0 ? `fps=${fps} transit=${TRANSIT}q` : `fps=${fps}`;
  if (minBuf < 0) fail(`${name}: buffered 出现负值 ${minBuf} ⇒ 记账又双扣了`);
  if (TRANSIT === 0 && maxLag > QUANT) {
    fail(`${name}: 报的深度与真实排队量最大差 ${maxLag} 帧 > 一个 quant ⇒ 有一处帧被扣了两次`);
  }
  if (grew > 0) {
    fail(`${name}: 预热后 underrun 又涨了 ${grew} 帧 ⇒ 需求阈值撑不住这个投递模型（报的深度比可取的多 ${maxLag} 帧）`);
  }
  // 60fps 必须恰好是目标深度：这一条钉住"不拿延迟换保险"。
  if (TRANSIT === 0 && fps === 60 && minBuf > 1024 + QUANT) {
    fail(`fps=60: 稳态深度 ${minBuf} 明显高于目标 1024 ⇒ 需求阈值把延迟白抬了`);
  }
  console.log(
    `ok  fps=${fps}  深度 ${minBuf}~${maxBuf}f(${((minBuf * 1000) / RATE).toFixed(1)}~${((maxBuf * 1000) / RATE).toFixed(1)}ms)` +
      `  记账最大误差 ${maxLag}f  预热后 underrun +${grew}`,
  );
}
