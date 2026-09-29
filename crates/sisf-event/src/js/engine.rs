//! 引擎无关的 JS 接缝 + MV 风 prelude + 注入协议。
//!
//! 只有三件事与引擎有关，其余全在本文件共享：
//! - [`Engine::boot`]：建一颗能用的 JS 堆，装上 prelude。
//! - [`Engine::exec`]：跑一段语句，忽略完成值。
//! - [`Engine::read_output`]：把 `__sisfOut` 账本数组序列化成 JSON 串读回。
//!
//! native 腿用 rquickjs（quickjs-ng），wasm 腿用浏览器自带引擎（`js_sys::eval`）。prelude 是
//! 一份、`__sisfIn`/`__sisfOut` 协议是一份、注入语句构造是一份 ⇒ "同一份脚本 → 同一份账本"
//! 这件事在两条腿上由**同一段 Rust 代码**保证，差别只剩 `exec` 怎么调引擎。

use serde::{Deserialize, Serialize};

/// 只读查询用的宿主快照。`$gameMap`/`$gamePlayer` 那几个数从这里取，由游戏侧每拍
/// [`JsBridge::set_state`](super::JsBridge::set_state) 刷进来。
///
/// 为什么后端自己持有而不是从 `ctx` 现取：`ScriptBackend::command` 拿不到 `EventCtx`
/// （见 `crate::script`），而 `$gameMap.mapId()` 这类读必须在脚本执行**当时**就有值可回。
/// 引擎没有"地图"这一层（地图归 `sisf-rpgmk`），所以它只认识这组扁平的数，谁在跑 MV 谁负责填。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MVState {
    pub map_id: u32,
    pub map_width: u32,
    pub map_height: u32,
    pub player_x: i64,
    pub player_y: i64,
    /// 2=下 4=左 6=右 8=上（MV 的方向码）。
    pub player_direction: i64,
    pub player_moving: bool,
}

/// 把一份 `__sisfIn` JSON 拼成注入语句。
///
/// 走 `JSON.parse(<字符串字面量>)` 而不是把 JSON 直接当 JS 对象字面量塞进去：JSON 里可能含
/// `U+2028`/`U+2029`（字符串槽里的文本），那是合法 JSON 但在 JS 源码里是行终止符，会炸。
/// 用 serde 把整段 JSON 再转成一个 JS 字符串字面量（自动转义），两腿都只依赖 `JSON.parse`。
///
/// 关键：状态量全走 `globalThis.`。`js_sys::eval` 每次调用给它的 `var` 声明一个**一次性**作用域，
/// 不落到全局（而 QuickJS 的 `ctx.eval` 是 global 求值、`var` 直接成全局）——两腿这处行为不同，
/// 所以顶层绑定一律显式写 `globalThis`，让这颗 prelude 在"浏览器/V8 的 transient eval"和
/// "quickjs-ng 的 global eval"下都留得下来（这正是双端一致判据要钉的那类差异）。
fn inject_src(mirror_json: &str) -> String {
    let lit = serde_json::Value::String(mirror_json.to_owned()).to_string();
    format!(
        "globalThis.__sisfIn = JSON.parse({lit}); \
         globalThis.__sisfOut = []; \
         globalThis.__V = null; globalThis.__S = null; globalThis.__R = 0;"
    )
}

/// MV 风格 JS 门面。**只用 ES5.1 语法**写：quickjs-ng 与浏览器都远超这个子集，但把公共面钉在
/// ES5.1 上，才能保证同一份 prelude 在两颗引擎里逐字行为一致（这是本模块能拿双端判据的前提）。
///
/// 门面里每个写操作都往 `__sisfOut` 推一条账本；每个读操作都走 `__V`/`__S` 覆盖层（首次访问
/// 从 `__sisfIn` 播种），于是同一段脚本里"先 setValue 后 value"读得到自己刚写的值。
///
/// 所有顶层名字都挂 `globalThis`（理由见 [`inject_src`]）。函数体内引用 `__V`/`__sisfIn`/`__out`
/// 等裸标识符，靠全局对象属性查找解析——两腿一致。
const PRELUDE: &str = r#"
globalThis.__sisfIn = null;
globalThis.__sisfOut = [];
globalThis.__V = null;
globalThis.__S = null;
globalThis.__R = 0;
globalThis.__sync = function () {
  if (globalThis.__V === null) {
    globalThis.__V = {};
    globalThis.__S = {};
    var iv = __sisfIn.vars, k;
    for (k in iv) { if (Object.prototype.hasOwnProperty.call(iv, k)) __V[k] = iv[k]; }
    var sw = __sisfIn.switches, i;
    for (i = 0; i < sw.length; i++) __S[sw[i]] = true;
  }
};
globalThis.__out = function (o) { __sisfOut.push(o); };
globalThis.__toInt = function (v) {
  v = +v;
  if (!isFinite(v)) return 0;
  return v < 0 ? Math.ceil(v) : Math.floor(v);
};

globalThis.$gameVariables = {
  value: function (id) { __sync(); var x = __V[id]; return x === undefined ? 0 : x; },
  setValue: function (id, v) { __sync(); id = __toInt(id); v = __toInt(v); __V[id] = v; __out({ op: "setVar", slot: id, value: v }); },
  count: function () { return __sisfIn.vars.__count || 0; }
};
globalThis.$gameSwitches = {
  value: function (id) { __sync(); return __S[id] === true; },
  setValue: function (id, v) { __sync(); id = __toInt(id); v = !!v; if (v) __S[id] = true; else delete __S[id]; __out({ op: "setSwitch", slot: id, value: v }); },
  isActive: function (id) { return this.value(id); }
};
globalThis.$gameTemp = {
  _t: {},
  getTemp: function (k) { return this._t[k]; },
  setTemp: function (k, v) { this._t[k] = v; },
  clearTemp: function () { this._t = {}; }
};
globalThis.$gameMessage = {
  _bg: 0,
  setBackground: function (b) { this._bg = b; },
  background: function () { return this._bg; },
  active: function () { return false; }
};
globalThis.$gameMap = {
  mapId: function () { return __sisfIn.state.map_id; },
  width: function () { return __sisfIn.state.map_width; },
  height: function () { return __sisfIn.state.map_height; },
  isLoopHorizontal: function () { return false; },
  isLoopVertical: function () { return false; }
};
globalThis.$gamePlayer = {
  x: function () { return __sisfIn.state.player_x; },
  y: function () { return __sisfIn.state.player_y; },
  direction: function () { return __sisfIn.state.player_direction; },
  isMoving: function () { return !!__sisfIn.state.player_moving; },
  characterName: function () { return ""; }
};

globalThis.SISF = {
  // 确定性随机：走 Rust 侧预抽好的 __sisfIn.rand（splitmix64），不碰引擎的 Math.random。
  // 两腿拿到的是**同一串数**，游标 __R 是纯整数运算 ⇒ randomInt 序列逐字节相等。
  randomInt: function (lo, hi) {
    var r = __sisfIn.rand[__R % __sisfIn.rand.length]; __R++;
    lo = __toInt(lo); hi = __toInt(hi);
    if (hi < lo) { var t = lo; lo = hi; hi = t; }
    return lo + (r % (hi - lo + 1));
  },
  random: function () { var r = __sisfIn.rand[__R % __sisfIn.rand.length]; __R++; return r / 65536; },
  showText: function (speaker, body, choices, resultVar) {
    __out({ op: "text", speaker: speaker || "", body: body == null ? "" : String(body), choices: choices || [], result: (resultVar == null ? null : __toInt(resultVar)) });
  },
  playBgm: function (name, gain, fade) { __out({ op: "play", channel: "bgm", name: String(name), gain: gain == null ? 1 : +gain, fade: fade == null ? 0 : +fade }); },
  playBgs: function (name, gain, fade) { __out({ op: "play", channel: "bgs", name: String(name), gain: gain == null ? 1 : +gain, fade: fade == null ? 0 : +fade }); },
  playMe: function (name, gain, fade) { __out({ op: "play", channel: "me", name: String(name), gain: gain == null ? 1 : +gain, fade: fade == null ? 0 : +fade }); },
  playSe: function (name, gain, fade) { __out({ op: "play", channel: "se", name: String(name), gain: gain == null ? 1 : +gain, fade: fade == null ? 0 : +fade }); },
  fade: function (target, frames, out) { __out({ op: "fade", target: String(target), frames: __toInt(frames), out: !!out }); },
  transferPlayer: function (map, x, y) { __out({ op: "transfer", map: __toInt(map), x: __toInt(x), y: __toInt(y) }); },
  postCustom: function (id, payload) { __out({ op: "custom", id: __toInt(id), payload: payload === undefined ? null : payload }); },
  loadPlugin: function (name) { __out({ op: "plugin", name: String(name) }); },
  plugin: function (name) { __out({ op: "plugin", name: String(name) }); }
};
"#;

// --- native 腿：rquickjs（quickjs-ng） ------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
pub struct Engine {
    ctx: rquickjs::Context,
}

#[cfg(not(target_arch = "wasm32"))]
impl Engine {
    pub fn boot() -> Self {
        use rquickjs::{Context, Runtime};
        let runtime = Runtime::new().expect("rquickjs Runtime 建不出来");
        let ctx = Context::full(&runtime).expect("rquickjs Context 建不出来");
        let mut engine = Self { ctx };
        // prelude 装不进就整个后端没意义 ⇒ 这里 panic 是有界的：它在 `JsBridge::new()`，
        // 示例在建桥时就崩，而不是跑到某条 355 才诡异地静默。
        engine
            .exec(PRELUDE)
            .expect("MV prelude 在 rquickjs 里装载失败");
        engine
    }

    pub fn inject_input(&mut self, mirror_json: &str) -> Result<(), String> {
        self.exec(&inject_src(mirror_json))
    }

    pub fn exec(&mut self, src: &str) -> Result<(), String> {
        self.ctx.with(|ctx| {
            ctx.eval::<rquickjs::Value, _>(src)
                .map(|_| ())
                .map_err(|e| describe(ctx, e))
        })
    }

    pub fn read_output(&mut self) -> Result<String, String> {
        self.ctx.with(|ctx| {
            ctx.eval::<String, _>("JSON.stringify(__sisfOut)")
                .map_err(|e| describe(ctx, e))
        })
    }
}

/// 把 QuickJS 抛出的异常取成人能读的文本。`rquickjs::Error` 的 `Display` 只会给出
/// "Exception generated by QuickJS"，真正的原因在 `ctx.catch()` 返回的抛出值里。
#[cfg(not(target_arch = "wasm32"))]
fn describe(ctx: rquickjs::Ctx<'_>, e: rquickjs::Error) -> String {
    let thrown = ctx.catch();
    format!("{thrown:?}（{e}）")
}

// --- wasm 腿：浏览器自带引擎（js-sys） ------------------------------------------------------

#[cfg(target_arch = "wasm32")]
pub struct Engine {
    _priv: (),
}

#[cfg(target_arch = "wasm32")]
impl Engine {
    pub fn boot() -> Self {
        let engine = Self { _priv: () };
        engine
            .inner_exec(PRELUDE)
            .expect("MV prelude 在宿主 JS 里装载失败");
        engine
    }

    pub fn inject_input(&mut self, mirror_json: &str) -> Result<(), String> {
        self.inner_exec(&inject_src(mirror_json))
    }

    pub fn exec(&mut self, src: &str) -> Result<(), String> {
        self.inner_exec(src)
    }

    pub fn read_output(&mut self) -> Result<String, String> {
        self.inner_eval("JSON.stringify(__sisfOut)")
    }

    fn inner_exec(&self, src: &str) -> Result<(), String> {
        js_sys::eval(src).map(|_| ()).map_err(js_err)
    }

    fn inner_eval(&self, expr: &str) -> Result<String, String> {
        let v = js_sys::eval(expr).map_err(js_err)?;
        v.as_string()
            .ok_or_else(|| "JSON.stringify 结果不是字符串".to_owned())
    }
}

#[cfg(target_arch = "wasm32")]
fn js_err(e: wasm_bindgen::JsValue) -> String {
    use wasm_bindgen::JsCast;
    if let Some(s) = e.as_string() {
        s
    } else if let Some(o) = e.dyn_ref::<js_sys::Error>() {
        o.message()
            .as_string()
            .unwrap_or_else(|| "未知 JS 异常".to_owned())
    } else {
        "未知 JS 异常".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    /// prelude 在两腿都装得起来，且一个最小程序能吐一条 `setVar` 账本。这是 `Engine` 接缝的
    /// 冒烟测试 —— 真正的双端逐行相等判据在 `js::tests`（同一断言 native + wasm 各跑一遍）。
    #[test]
    fn prelude_boots_and_records_one_write() {
        let mut e = Engine::boot();
        e.inject_input(r#"{"vars":{},"switches":[],"state":{},"rand":[0,1,2]}"#)
            .unwrap();
        e.exec("$gameVariables.setValue(5, 12);").unwrap();
        let ledger = e.read_output().unwrap();
        assert!(ledger.contains("\"setVar\""), "账本里没有 setVar：{ledger}");
        assert!(ledger.contains("\"slot\":5"), "账本 slot 不对：{ledger}");
        assert!(ledger.contains("\"value\":12"), "账本 value 不对：{ledger}");
    }

    /// 同段脚本内"先写后读"必须读到刚写的值（覆盖层，不是引擎差异）。
    #[test]
    fn overlay_read_sees_same_run_write() {
        let mut e = Engine::boot();
        e.inject_input(r#"{"vars":{},"switches":[],"state":{},"rand":[0]}"#)
            .unwrap();
        e.exec("$gameVariables.setValue(1, 7); $gameVariables.setValue(2, $gameVariables.value(1) + 1);")
            .unwrap();
        let ledger = e.read_output().unwrap();
        assert!(ledger.contains("\"value\":7"), "{ledger}");
        assert!(
            ledger.contains("\"value\":8"),
            "第二发没读到第一写的值：{ledger}"
        );
    }
}
