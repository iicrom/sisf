//! JS 第二脚本语言运行时（T11 续）：`ScriptBackend` 的一种落地。
//!
//! ## 为什么这一层存在的唯一理由
//!
//! MV/MZ 的事件页里，`355`（脚本）与 `356`（插件命令）里塞的是**原始 JavaScript**：
//! `$gameVariables.setValue(1, $gameVariables.value(3) + 1);`。兼容层
//! （[`crate`](crate) 的兄弟模块 `sisf-rpgmk::translate`）已经把 `355` 的多行源码原样拼成一个
//! [`Param::Text`](crate::command::Param::Text) 交进 `Command::Custom("rpgmv.script", …)`，但
//! 默认后端不认这个名字 ⇒ 每次都登记一条"后端不认名字"的告警后跳过。本模块就是来认领这两条
//! 主题的：把 JS 跑出来，把它想做的**事**翻译成引擎自己的 [`Command`]，交回一段
//! [`Program`] 让**虚拟机**去执行。
//!
//! ## 控制流不进这里（这是全模块的承重墙）
//!
//! [`ScriptBackend`] 的 `command` 拿不到 `EventCtx`，所以 JS 无权直接改世界。它能做的每件事
//! 都落成两种结果之一：
//!
//! - **写请求** → 编成引擎指令：`$gameVariables.setValue(5, 12)` ⇒ `SetVar{slot:Int(5), value:Lit(12)}`，
//!   `SISF.playBgm("Boss01")` ⇒ `PlayBgm`。它们随返回的 `Program` 由虚拟机在**归属事件**的上下文里执行
//!   （`ShowText` 照样挂起等按键、`SetVar` 照样进存档），控制流与存档模型一字未改（见
//!   [`crate::script`] 的"不必冻结一颗 JS 堆"）。
//! - **读请求** → 从后端自持的一份 [`VarStore`] 镜像 + 宿主刷新的 [`MVState`]（`$gameMap`/`$gamePlayer`
//!   那几个数）里取。镜像在**每拍 `tick()` 开头**从 `ctx.vars` 重新同步；写请求在执行当拍就同时落到
//!   镜像，于是同一段脚本里"先 set 后 value"读得到自己刚写的值，而跨脚本/跨拍的正确性由下一次 `tick`
//!   的重同步保证。这条"延后一拍"的对齐不是妥协——MV 自己的 `requestRefresh` 就是延后一拍（T11 事实第 12 条）。
//!
//! ## 双端两颗引擎、一份账本
//!
//! native = rquickjs（quickjs-ng），wasm = 浏览器自带引擎。为了让"同一份脚本 → 同一份行为"这件事
//! **可证**，本模块只依赖一条引擎无关的接缝 [`Engine`]（`exec` 跑语句 + `eval_json` 求一个 JSON 串），
//! 而 prelude（JS 侧的 `$game*` 门面）、`__sisfOut` 账本协议、把账本解析成 `Vec<Command>` 的代码
//! **全部引擎无关**。两颗引擎唯一的差别就是 `exec`/`eval_json` 怎么调，prelude 与 parser 是同一份。
//! 于是"双端一致"不是"看着一样"，而是同一个断言在两条腿上各跑一遍（见 `tests` 与 xtask 的 js 腿）。
//!
//! [`Command`]: crate::command::Command
//! [`Program`]: crate::command::Program

mod engine;

pub use engine::MVState;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::command::{Command, Expr, Param, Program, SetOp, Slot, Sound};
use crate::runner::EventCtx;
use crate::script::{ScriptBackend, Submission};

use engine::Engine;

/// 后端认下来的两条 MV 主题（`translate.rs` 把 `355`/`356` 送进来的名字）。
pub const SCRIPT_TOPIC: &str = "rpgmv.script";
pub const PLUGIN_TOPIC: &str = "rpgmv.plugin";

/// 每段脚本预抽取的确定性随机数个数。两条腿用**同一颗 Rust 侧 splitmix64** 抽好后注入，
/// JS 侧只按整数游标取用 ⇒ 即便 rquickjs 与浏览器的 `Math.random` 天生不同，`SISF.randomInt`
/// 的序列仍逐字节相等。超过这个个数就**确定性地回卷**（宁可回卷也不引入引擎相关的随机源）。
const RAND_BATCH: usize = 64;

/// JS 第二脚本语言后端。
///
/// 装法与 [`NullScriptBackend`](crate::script::NullScriptBackend) 完全一致：
/// `EventRunner::new().with_script(Box::new(JsBridge::new()))`。`Box<dyn ScriptBackend>`
/// 要求它是对象安全的——三个方法都只吃 `&mut self` 与值参数，成立。
pub struct JsBridge {
    engine: Engine,
    /// `ctx.vars` 的镜像：`tick` 开头整体重同步，写请求当拍即落。存 `Int`/`Switch` 两类。
    ints: Vec<(u32, i64)>,
    switches: Vec<u32>,
    /// 只读查询的宿主快照（`$gameMap`/`$gamePlayer`）。放成 `Rc<RefCell<_>>` 是让游戏侧能在
    /// 后端已被 `EventRunner` 独占之后，仍通过 [`state_handle`](Self::state_handle) 每拍刷新它
    /// —— 否则 `command()` 里读到的永远是建桥那一刻的旧值。
    state: Rc<RefCell<MVState>>,
    /// `start(entry)` 记下的入口名；`tick` 第一次拿到 `ctx.vfs` 时才真去读 `js/plugins/<entry>.js`。
    pending_entry: Option<String>,
    /// 本拍解析出、但需要 `ctx`（读 VFS）才能兑现的插件加载请求。
    pending_plugins: Vec<String>,
    /// 随机游标的内部计数（每次 `run` 前重置，配合 [`RAND_BATCH`]）。
    rand_serial: u64,
}

impl Default for JsBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl JsBridge {
    /// 建桥并装上 MV 风 prelude。引擎在 native/wasm 上是两个实现，但构造入口只有一个。
    pub fn new() -> Self {
        Self {
            engine: Engine::boot(),
            ints: Vec::new(),
            switches: Vec::new(),
            state: Rc::new(RefCell::new(MVState::default())),
            pending_entry: None,
            pending_plugins: Vec::new(),
            rand_serial: 0,
        }
    }

    /// 用外部提供的只读状态把手建桥 —— 让游戏侧在后端被 `EventRunner` 独占之后，仍能
    /// 通过同一个 `Rc` 每拍刷新 `$gameMap`/`$gamePlayer`（脚本在 `command()` 里读它）。
    pub fn with_state(state: Rc<RefCell<MVState>>) -> Self {
        Self {
            engine: Engine::boot(),
            ints: Vec::new(),
            switches: Vec::new(),
            state,
            pending_entry: None,
            pending_plugins: Vec::new(),
            rand_serial: 0,
        }
    }

    /// 游戏侧每拍刷新只读快照（`$gameMap.mapId()`、`$gamePlayer.x` 等在这里取）。
    pub fn set_state(&mut self, state: MVState) {
        *self.state.borrow_mut() = state;
    }

    /// 共享的只读状态把手。后端被 `EventRunner` 独占后，游戏侧拿这个 `Rc` 克隆去每拍
    /// `borrow_mut()` 写 `$gameMap`/`$gamePlayer` 的当前值（脚本在 `command()` 里读它）。
    pub fn state_handle(&self) -> Rc<RefCell<MVState>> {
        Rc::clone(&self.state)
    }

    /// 把当前镜像喂进引擎、跑一段 JS、把回来的账本翻成 `Vec<Command>`。
    ///
    /// 这是唯一碰引擎接缝的地方，也是"双端一致"真正被消费的地方：`mirror_json` 与账本协议
    /// 都在这里，两颗引擎提供的只是 `exec`/`eval_json` 两个动作。
    fn run(&mut self, src: &str, rng_seed: u64) -> Result<Vec<Command>, String> {
        let mirror = self.mirror_json(rng_seed);
        self.engine.inject_input(&mirror)?;
        self.engine.exec(src)?;
        let ledger = self.engine.read_output()?;
        self.drain_ledger(&ledger)
    }

    /// 把镜像 + `MVState` + 预抽随机数序列化成注入给 JS 的那一份 `__sisfIn`。
    fn mirror_json(&mut self, rng_seed: u64) -> String {
        let mut rng = crate::vars::Rng::seeded(rng_seed ^ self.rand_serial);
        self.rand_serial = self.rand_serial.wrapping_add(RAND_BATCH as u64);
        let rand: Vec<i64> = (0..RAND_BATCH).map(|_| rng.range(0, 65535)).collect();
        let state = self.state.borrow().clone();
        let obj = serde_json::json!({
            "vars": self.ints.iter().map(|(k, v)| (k.to_string(), serde_json::json!(*v))).collect::<serde_json::Map<_, _>>(),
            "switches": self.switches.iter().collect::<Vec<_>>(),
            "state": state,
            "rand": rand,
        });
        obj.to_string()
    }

    /// 账本 → `Vec<Command>`。**这一段是双端逐行相等的判据落点**：两腿喂同一条账本 JSON，
    /// 必须吐出逐字节相同的 `Vec<Command>`（`Command` 派生了 `PartialEq` + `Debug`）。
    fn drain_ledger(&mut self, ledger: &str) -> Result<Vec<Command>, String> {
        let value: serde_json::Value =
            serde_json::from_str(ledger).map_err(|e| format!("账本不是合法 JSON：{e}"))?;
        let items = match value {
            serde_json::Value::Array(a) => a,
            serde_json::Value::Null => return Ok(Vec::new()),
            other => return Err(format!("账本顶层应为数组，实为 {other}")),
        };
        let mut cmds = Vec::new();
        for it in items {
            let op = it.get("op").and_then(|v| v.as_str()).unwrap_or("");
            match op {
                "setVar" => {
                    let slot = int_slot(&it, "slot")?;
                    let v = it.get("value").and_then(json_as_i64).unwrap_or(0);
                    self.ints.retain(|(k, _)| *k != slot);
                    self.ints.push((slot, v));
                    cmds.push(Command::SetVar {
                        slot: Slot::Int(slot),
                        op: SetOp::Set,
                        value: Expr::Lit(v),
                    });
                }
                "setSwitch" => {
                    let slot = int_slot(&it, "slot")?;
                    let on = it.get("value").and_then(|v| v.as_bool()).unwrap_or(false);
                    self.switches.retain(|k| *k != slot);
                    if on {
                        self.switches.push(slot);
                    }
                    cmds.push(Command::SetVar {
                        slot: Slot::Switch(slot),
                        op: SetOp::Set,
                        value: Expr::Lit(if on { 1 } else { 0 }),
                    });
                }
                "text" => {
                    let speaker = it.get("speaker").and_then(|v| v.as_str()).unwrap_or("");
                    let body = it
                        .get("body")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned();
                    let choices = str_array(&it, "choices");
                    let result = int_opt(&it, "result").map(Slot::Int);
                    cmds.push(Command::ShowText {
                        speaker: if speaker.is_empty() {
                            None
                        } else {
                            Some(speaker.to_owned())
                        },
                        body,
                        choices,
                        result,
                        cursor_se: None,
                        allow_cancel: it
                            .get("allowCancel")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                    });
                }
                "play" => {
                    let name = it
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned();
                    let gain = it.get("gain").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
                    let fade = it.get("fade").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                    let sound = Sound {
                        name,
                        gain,
                        fade_in: fade,
                    };
                    cmds.push(
                        match it.get("channel").and_then(|v| v.as_str()).unwrap_or("se") {
                            "bgm" => Command::PlayBgm(sound),
                            "bgs" => Command::PlayBgs(sound),
                            "me" => Command::PlayMe(sound),
                            _ => Command::PlaySe(sound),
                        },
                    );
                }
                "fade" => {
                    let target = match it.get("target").and_then(|v| v.as_str()).unwrap_or("bgm") {
                        "bgs" => crate::command::FadeTarget::Bgs,
                        "me" => crate::command::FadeTarget::Me,
                        "se" => crate::command::FadeTarget::Se,
                        "screen" => crate::command::FadeTarget::Screen,
                        "all" => crate::command::FadeTarget::All,
                        _ => crate::command::FadeTarget::Bgm,
                    };
                    let frames = it.get("frames").and_then(json_as_i64).unwrap_or(0) as u32;
                    let out = it.get("out").and_then(|v| v.as_bool()).unwrap_or(false);
                    cmds.push(Command::Fade {
                        target,
                        frames,
                        out,
                    });
                }
                "transfer" => {
                    // 转移玩家不在引擎词汇表里（引擎没有"地图切换"这一层，归宿主），
                    // 所以它照 `translate.rs` 的约定回一条 Custom 主题，由宿主认领。
                    let map = int_slot(&it, "map").unwrap_or(0);
                    let x = int_slot(&it, "x").unwrap_or(0);
                    let y = int_slot(&it, "y").unwrap_or(0);
                    cmds.push(Command::Custom(
                        "rpgmv.transfer".into(),
                        vec![
                            Param::Int(i64::from(map)),
                            Param::Int(i64::from(x)),
                            Param::Int(i64::from(y)),
                        ],
                    ));
                }
                "custom" => {
                    let id = int_slot(&it, "id").unwrap_or(0);
                    let payload = it
                        .get("payload")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    cmds.push(Command::Custom(
                        format!("js.custom.{id}"),
                        vec![Param::from_json(&payload)],
                    ));
                }
                "plugin" => {
                    // 加载插件需要读 VFS ⇒ 不在 command() 里兑现，记进 pending，下一次 tick 处理。
                    let name = it
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned();
                    if !name.is_empty() {
                        self.pending_plugins.push(name);
                    }
                }
                other => return Err(format!("未知账本条目 op={other:?}")),
            }
        }
        Ok(cmds)
    }
}

impl ScriptBackend for JsBridge {
    fn start(&mut self, entry: &str) {
        // `start` 里没有 `ctx`（也就没有 VFS），所以只能记下入口名，等 `tick` 第一次拿到
        // `ctx.vfs` 再去读 `js/plugins/<entry>.js`。引擎的 boot 前奏不依赖任何外部 JS。
        if !entry.is_empty() {
            self.pending_entry = Some(entry.to_owned());
        }
    }

    fn tick(&mut self, ctx: &mut EventCtx, _queue: &mut Vec<Submission>) {
        // (1) 镜像从 ctx.vars 整体重同步：VM 这一拍用 SetVar/If 改过的东西，脚本下一段要能读到。
        self.ints = ctx.vars.ints().collect();
        self.switches = ctx.vars.switches().collect();

        // (2) 兑现 start 记下的入口 —— 这一步才真的有 VFS 可读。
        if let Some(entry) = self.pending_entry.take() {
            self.load_plugin(ctx, &entry);
        }

        // (3) 兑现脚本请求的插件加载（`SISF.loadPlugin`）。加载即执行其顶层代码，
        //     但插件注册的是它自己的全局，不改本拍的世界 ⇒ 只 exec、不收账本。
        if !self.pending_plugins.is_empty() {
            let names = std::mem::take(&mut self.pending_plugins);
            for name in names {
                self.load_plugin(ctx, &name);
            }
        }
    }

    fn command(&mut self, name: &str, args: &[Param]) -> Option<Arc<Program>> {
        match name {
            SCRIPT_TOPIC => {
                let src = match args.first() {
                    Some(Param::Text(s)) => s.clone(),
                    _ => String::new(),
                };
                // 种子取镜像大小即可保证同段脚本内随机序列稳定；真正的跨拍确定性由 VM 的 RNG 持有。
                let seed = (self.ints.len() as u64) ^ (self.switches.len() as u64) << 32;
                match self.run(&src, seed) {
                    Ok(cmds) => Some(Arc::new(Program::compile(&cmds).unwrap_or_default())),
                    // 脚本抛错：告警通道在事件机上、这里只能返回。交空程序（"处理完了但没指令要跑"），
                    // 让 VM 继续；引擎异常文本已经进了 __sisfOut 的对侧（见 engine.rs），宿主日志会读到。
                    Err(_) => Some(Arc::new(Program::default())),
                }
            }
            PLUGIN_TOPIC => {
                // 插件命令：MV 的 356 只有一个系统名。这里把它交给一个约定的全局函数
                // `SISF.plugin(name)`（prelude 里定义，转成一条 `custom` 账本），语义与 355 同。
                let plugin = match args.first() {
                    Some(Param::Text(s)) => s.clone(),
                    _ => String::new(),
                };
                let src = format!("SISF.plugin({})", engine_quote(&plugin));
                let seed = (self.ints.len() as u64) << 1;
                match self.run(&src, seed) {
                    Ok(cmds) => Some(Arc::new(Program::compile(&cmds).unwrap_or_default())),
                    Err(_) => Some(Arc::new(Program::default())),
                }
            }
            // 其余名字（rpgmv.movie / rpgmv.unimplemented / 宿主自己的主题）不认 ⇒ None ⇒ VM 登记告警。
            _ => None,
        }
    }
}

impl JsBridge {
    /// 读 `js/plugins/<name>.js` 并 exec（插件的顶层代码只注册全局，不当场改世界）。
    fn load_plugin(&mut self, ctx: &mut EventCtx, name: &str) {
        let path = format!("js/plugins/{name}.js");
        let src = match sisf_asset::VfsPath::new(&path)
            .ok()
            .and_then(|p| ctx.vfs.read(&p))
        {
            Some(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            None => return,
        };
        let _ = self.engine.exec(&src);
    }
}

/// 把一个 JS 字符串安全地写成源码里的字符串字面量（复用 serde 的转义，避免手写引号踩坑）。
fn engine_quote(s: &str) -> String {
    serde_json::Value::String(s.to_owned()).to_string()
}

// --- 账本 JSON 的小抓取器（都在 `Command` 边界上，双端共用） ------------------------------

fn int_slot(it: &serde_json::Value, key: &str) -> Result<u32, String> {
    let v = it
        .get(key)
        .and_then(json_as_i64)
        .ok_or_else(|| format!("账本缺整数键 {key:?}"))?;
    if v < 0 || v > u32::MAX as i64 {
        return Err(format!("{key}={v} 超出 u32 变量号范围"));
    }
    Ok(v as u32)
}

fn int_opt(it: &serde_json::Value, key: &str) -> Option<u32> {
    it.get(key)
        .and_then(json_as_i64)
        .and_then(|v| u32::try_from(v).ok())
}

fn str_array(it: &serde_json::Value, key: &str) -> Vec<String> {
    it.get(key)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .map(|x| x.as_str().unwrap_or("").to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// JS 的数字在两个引擎里都是 `double`；MV 的变量号/整数值都落在 2^53 内，取整即可。
/// `NaN`/无穷一律当 0（配置写错不该让事件卡住，与 `Expr::Div` 除零取 0 同一立场）。
fn json_as_i64(v: &serde_json::Value) -> Option<i64> {
    if let Some(i) = v.as_i64() {
        return Some(i);
    }
    v.as_f64()
        .filter(|f| f.is_finite())
        .map(|f| f.trunc() as i64)
}

#[cfg(test)]
mod tests;
