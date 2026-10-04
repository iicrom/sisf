//! 阶段 0 技术闸门：兼容层的**对象模型**到底能不能成立。
//!
//! ## 为什么这一条排在任何一行 `Game_*` 代码之前
//!
//! 方向是：`rpg_*.js` 那一整层运行时由 Rust 顶替，而 `js/plugins/` 里的游戏逻辑脚本**原样运行**。
//! 这条路的全部风险压在一个形状上 —— 实测于本仓库的 `tools/probes/compat-census.py`
//! （上游 `www/js/`，只读，可照抄复现）：
//!
//! - `rpg_*.js` 交出 **136 个类 / 3,107 条 `Klass.prototype.method`**；
//! - 103 个启用插件（54,490 行）**读走既有原型方法 972 处、改写原型方法 2,756 处**，
//!   另有**跨类读别人的内部字段 8,075 处**（`$gameVariables._value`、`this._messages._texts` 那一族）。
//!
//! ⇒ 兼容层不能只是"Rust 里有一堆同名函数"：交出的每一类必须在 **JS 里是一个 prototype 可写的
//! 真对象**，插件才能用它们的标准写法（别名叠加）把方法套在自己身上；而状态必须**住在 JS 对象上**
//! （或由 Rust 写成 JS 属性），那 8,075 处跨类字段读才量得到东西。
//!
//! 下面 [`tests`] 那四条判据就是这两件事的实测。红了的处置不是"绕过去"，而是**换边界**
//! （退回"兼容层只顶替平台那一层、`rpg_*.js` 照跑"）。
//!
//! ## 两条腿：同一份 JS 文本、同一组断言，但**接线形状不同**（这一格是量出来的）
//!
//! [`HOST_CLASS_JS`] 与 [`PLUGIN_PROBE_JS`] 是引擎无关的字符串常量，native 腿（rquickjs）与
//! wasm 腿（浏览器自带引擎，`cargo xtask wasm-test` 走 node）各自 eval **同一份文本**，断言写
//! 一遍 ⇒ "覆盖生效"在两条腿上是同一个判据，不是"看着一样"（同 `sisf-event/src/js/tests.rs`）。
//!
//! 但两条腿把 Rust 行为挂上 `prototype` 的形状不一样：
//!
//! - **native**：`rquickjs::Function::new` 出来的原生函数**直接**当 prototype 方法，闭包第一参
//!   `This<Object>` 就是接收者 ⇒ 一条 Rust 函数 = 一个方法，中间没有层。
//! - **wasm**：`wasm_bindgen` 的 `Closure` 是个普通 JS 函数，而 **JS 不会把接收者当参数交给它** ⇒
//!   必须在 JS 侧套一层跳板（[`BRIDGE_JS`]：`prototype.setValue = function(id,v){ return __sisf_set(this,id,v); }`），
//!   由跳板把 `this` 显式传进 Rust。⇒ 浏览器腿**每条方法多一个 JS 帧**，而那一族文本只能
//!   **代码生成**（按方法名单出 `BRIDGE_JS` 那样的十几行），手抄 3,107 条抄不完。

/// 兼容层交出一个 MV 类的**最小形状**：构造器在 JS 侧，方法由 Rust 挂进 `prototype`，
/// 状态是对象上的 `_value`（上游 `Game_Variables` 用的就是这个字段名）。
///
/// 名字**显式挂 `globalThis`**而不是靠 `function` 声明外泄 —— 这是两条腿量出来的不对称：
/// 上游 `rpg_*.js` 是 `<script>` 顶层，`function Foo(){}` 天然就是全局；而我们这些文本是
/// **eval 进去的**，`js_sys::eval` 在 wasm-bindgen 的 harness 里不会把声明漏成全局
/// （实测 `ReferenceError: Game_Variables is not defined`），rquickjs 那条腿会。
/// 显式挂法在两条腿上是同一条动作 ⇒ 兼容层的类形状只能这么交。
pub const HOST_CLASS_JS: &str = r#"
globalThis.Game_Variables = function () { this._value = []; };
globalThis.$gameVariables = new Game_Variables();
"#;

/// 模拟一个真插件的标准写法（这一款里 972 处，YEP 一族写作
/// `Yanfly.Core.Game_Variables_setValue = Game_Variables.prototype.setValue;`）。
/// 覆盖之后把值翻倍 —— 于是"流水里记到的是 20 而不是 10"就是**覆盖生效且仍回到 Rust**的证据。
pub const PLUGIN_PROBE_JS: &str = r#"
var aliasSetValue = Game_Variables.prototype.setValue;
Game_Variables.prototype.setValue = function (id, value) {
    aliasSetValue.call(this, id, value * 2);
};
"#;

/// [`PLUGIN_PROBE_JS`] 那一格的调用面。
pub const CALL_JS: &str = "$gameVariables.setValue(5, 10);";

/// wasm 腿的跳板（模块头那条实测的落点）：把 Rust 交出的两个函数（第一参是接收者）挂成
/// prototype 方法。native 腿不需要这一段。
#[cfg(all(feature = "js", target_arch = "wasm32"))]
pub const BRIDGE_JS: &str = r#"
Game_Variables.prototype.setValue = function (id, value) { return __sisf_set(this, id, value); };
Game_Variables.prototype.value = function (id) { return __sisf_value(this, id); };
"#;

/// `(变量号, 落进 Rust 时的值)`，按调用顺序。流水是**证据不是实现**：只用来证明
/// "插件覆盖之后那一次调用真的落到了 Rust"。
#[cfg(feature = "js")]
type Ledger = std::rc::Rc<std::cell::RefCell<Vec<(i32, i32)>>>;

/// 两条腿的宿主是同一个名字（消费方只写 `jsrt::Host`，不分腿）。
#[cfg(all(feature = "js", not(target_arch = "wasm32")))]
pub use native::Host;
#[cfg(all(feature = "js", target_arch = "wasm32"))]
pub use web::Host;

#[cfg(all(feature = "js", not(target_arch = "wasm32")))]
pub mod native {
    //! native 腿：rquickjs（quickjs-ng）。`Runtime` 与 `Context` 都留在结构体里 ——
    //! 运行时先掉，`Context` 就什么都不是了。
    use std::cell::RefCell;
    use std::rc::Rc;

    use rquickjs::context::EvalOptions;
    use rquickjs::prelude::This;
    use rquickjs::{Array, Context, Ctx, Error, Function, Object, Runtime, Value};

    use super::{HOST_CLASS_JS, Ledger};

    pub struct Host {
        _runtime: Runtime,
        ctx: Context,
        calls: Ledger,
    }

    /// **脚本按 sloppy 跑**（`rquickjs` 的 `EvalOptions::default()` 是 `strict: true`）。
    ///
    /// 上游 `rpg_*.js` 与每一个插件都是 `<script>` 里的**全局 sloppy 代码**：普通调用的函数里
    /// `this` 就是 window。用默认的 strict 去 eval 会造出一类浏览器里根本不存在的失败 ——
    /// 实测第一例是 `TRP_Particle` 打包进去的 pixi-particles 那句
    /// `this.PIXI = this.PIXI || {}`（`TRP_Particle.js:210`），报 "cannot read property 'PIXI' of undefined"。
    /// 这一族 UMD 库（`TRP_*`、`MPP_EncounterEffect` 那类打包件）在浏览器腿本来就能跑，
    /// 所以**这不是缺 API，是宿主把语言模式弄错了**；改成 sloppy 之后两条腿同一个模式。
    fn script_options() -> EvalOptions {
        // `EvalOptions` 是 `#[non_exhaustive]` ⇒ 只能从 default 出发把这两位改过来。
        let mut options = EvalOptions::default();
        options.global = true;
        options.strict = false;
        options
    }

    impl Host {
        pub fn boot() -> Self {
            let runtime = Runtime::new().expect("rquickjs Runtime 建不出来");
            let ctx = Context::full(&runtime).expect("rquickjs Context 建不出来");
            let calls: Ledger = Rc::new(RefCell::new(Vec::new()));
            ctx.with(|ctx| {
                ctx.eval_with_options::<(), _>(HOST_CLASS_JS, script_options())
                    .expect("MV 类形状装不进 JS");
                attach(&ctx, &calls).expect("Rust 行为挂不进 Game_Variables.prototype");
            });
            Self {
                _runtime: runtime,
                ctx,
                calls,
            }
        }

        pub fn eval(&self, src: &str) -> Result<(), String> {
            self.ctx.with(|ctx| {
                ctx.eval_with_options::<Value, _>(src, script_options())
                    .map(|_| ())
                    .map_err(|e| describe(&ctx, e))
            })
        }

        pub fn eval_bool(&self, expr: &str) -> bool {
            self.try_eval_bool(expr)
                .unwrap_or_else(|e| panic!("求值失败：{expr} —— {e}"))
        }

        pub fn try_eval_bool(&self, expr: &str) -> Result<bool, String> {
            self.ctx.with(|ctx| {
                ctx.eval_with_options::<bool, _>(expr, script_options())
                    .map_err(|e| describe(&ctx, e))
            })
        }

        pub fn eval_i32(&self, expr: &str) -> i32 {
            self.try_eval_i32(expr)
                .unwrap_or_else(|e| panic!("求值失败：{expr} —— {e}"))
        }

        pub fn try_eval_i32(&self, expr: &str) -> Result<i32, String> {
            self.ctx.with(|ctx| {
                ctx.eval_with_options::<i32, _>(expr, script_options())
                    .map_err(|e| describe(&ctx, e))
            })
        }

        /// 读一条 JS 表达式的字符串结果。兼容层的账本大多住在 JS 侧（`console` 那一族就是），
        /// 这一条是把它们取回 Rust 的最短腿 —— 形状上头不给 `Host` 加任何 MV 特有的把手。
        pub fn try_eval_string(&self, expr: &str) -> Result<String, String> {
            self.ctx.with(|ctx| {
                ctx.eval_with_options::<String, _>(expr, script_options())
                    .map_err(|e| describe(&ctx, e))
            })
        }

        pub fn eval_string(&self, expr: &str) -> String {
            self.try_eval_string(expr)
                .unwrap_or_else(|e| panic!("求值失败：{expr} —— {e}"))
        }

        pub fn calls(&self) -> Vec<(i32, i32)> {
            self.calls.borrow().clone()
        }

        /// 把一颗 Rust 函数挂成**全局名**：JS 侧调 `name(字符串)` 得到字符串。
        ///
        /// 为什么只有这一种形状：兼容层的 transport（`DataManager` 读 `data/*.json`、M10.4 的素材
        /// 读法）要的是"JS 当场问宿主一句话"，而两条腿能共用签名的只有**一个字符串进、一个字符串出**
        /// —— native 腿的 `Function` 参数走 rquickjs 的类型转换，wasm 腿的 `Closure` 只收 `JsValue`
        /// （模块头那条"接收者拿不到"是同一条不对称）。对象、位图、异步都不在这一格里：那要跳板表，
        /// 由 [`crate::shapes::trampoline_js`] 生成，属于 M10.4 的渲染面。
        ///
        /// 参数不是字符串时 native 腿直接抛 JS `TypeError`（不做 coercion）—— 这是故意的：
        /// transport 的入参只能是我们的 JS 文本里那个路径串，喂错了要响，不许悄悄读成"没有这个文件"。
        pub fn bind_str(
            &self,
            name: &str,
            f: impl Fn(String) -> String + 'static,
        ) -> Result<(), String> {
            self.ctx.with(|ctx| {
                // `Function::new` 要闭包是 `Clone`，而参数那一颗 `impl Fn` 不是 ⇒ 套一层 `Rc`
                // （与 [`attach`] 捕获 `Rc<RefCell<…>>` 同一条腿）。
                let f = std::rc::Rc::new(f);
                let inner = std::rc::Rc::clone(&f);
                let f = Function::new(ctx.clone(), move |s: String| inner(s))
                    .map_err(|e| describe(&ctx, e))?;
                ctx.globals().set(name, f).map_err(|e| describe(&ctx, e))
            })
        }
    }

    /// 把两条 Rust 行为**直接**挂到 JS 侧那个类的 `prototype` 上（不用 `Class::define`：
    /// 兼容层要的是"上游那个可被覆盖的 prototype"，见模块头）。
    fn attach(ctx: &Ctx, calls: &Ledger) -> Result<(), Error> {
        let class: Function = ctx.globals().get("Game_Variables")?;
        let proto: Object = class.get("prototype")?;

        let set_calls = Rc::clone(calls);
        let set_value = Function::new(
            ctx.clone(),
            move |this: This<Object>, id: i32, value: i32| {
                set_calls.borrow_mut().push((id, value));
                // 状态写回 JS 对象自己的 `_value`（上游的字段名），于是跨类读天然成立。
                let arr: Array = this.0.get("_value")?;
                if id >= 0 {
                    arr.set(id as usize, value)?;
                }
                Ok::<(), Error>(())
            },
        )?;
        proto.set("setValue", set_value)?;

        let value = Function::new(ctx.clone(), |this: This<Object>, id: i32| {
            let arr: Array = this.0.get("_value")?;
            let v: Option<i32> = arr.get(id as usize).ok();
            Ok::<i32, Error>(v.unwrap_or(0))
        })?;
        proto.set("value", value)?;
        Ok(())
    }

    /// QuickJS 的异常文本要从 `ctx.catch()` 取，`Error` 的 `Display` 只会给一句空话
    /// （同 `sisf-event/src/js/engine.rs` 的 `describe`）。
    fn describe(ctx: &Ctx, e: Error) -> String {
        let thrown = ctx.catch();
        format!("{thrown:?}（{e}）")
    }
}

#[cfg(all(feature = "js", target_arch = "wasm32"))]
pub mod web {
    //! wasm 腿：浏览器自带引擎（`js_sys::eval`）。跑的是与 native 腿**同一份 JS 文本**，
    //! 差别只在接受者那一格（见 [`super::BRIDGE_JS`]）。
    use std::cell::RefCell;
    use std::rc::Rc;

    use js_sys::{Array, Reflect, eval};
    use wasm_bindgen::JsCast;
    use wasm_bindgen::JsValue;
    use wasm_bindgen::closure::Closure;

    use super::{BRIDGE_JS, HOST_CLASS_JS, Ledger};

    pub struct Host {
        calls: Ledger,
        /// `Closure` 被 JS 侧引用之后必须继续活着 ⇒ 收进结构体，不掉。
        /// `RefCell` 是因为 [`Host::bind_str`] 只借 `&self`（与 native 腿同一个签名）。
        _keepalive: std::cell::RefCell<Vec<JsValue>>,
    }

    impl Host {
        pub fn boot() -> Self {
            let key = |s: &str| JsValue::from_str(s);
            // `js_sys::eval` 是**间接 eval** ⇒ 非严格模式下里面声明的 `function`/`var` 落到
            // globalThis，与 native 腿的 `ctx.eval` 同一格效果。
            eval(HOST_CLASS_JS).expect("MV 类形状装不进 JS");
            let g = js_sys::global();

            let calls: Ledger = Rc::new(RefCell::new(Vec::new()));
            let set_calls = Rc::clone(&calls);
            let setter = Closure::wrap(Box::new(move |this: JsValue, id: f64, value: f64| {
                set_calls.borrow_mut().push((id as i32, value as i32));
                let held = Reflect::get(&this, &key("_value")).expect("`_value` 该在实例上");
                let arr: &Array = held.unchecked_ref();
                arr.set(id as u32, JsValue::from(value));
            }) as Box<dyn FnMut(JsValue, f64, f64)>);
            Reflect::set(&g, &key("__sisf_set"), setter.as_ref()).expect("__sisf_set 挂不上全局");

            let getter = Closure::wrap(Box::new(|this: JsValue, id: f64| {
                let held = Reflect::get(&this, &key("_value")).expect("`_value` 该在实例上");
                let arr: &Array = held.unchecked_ref();
                arr.get(id as u32).as_f64().unwrap_or(0.0)
            }) as Box<dyn FnMut(JsValue, f64) -> f64>);
            Reflect::set(&g, &key("__sisf_value"), getter.as_ref())
                .expect("__sisf_value 挂不上全局");

            // 跳板：JS 侧负责把 `this` 交给那两个 Rust 函数。
            eval(BRIDGE_JS).expect("跳板装不上 prototype");

            Self {
                calls,
                _keepalive: std::cell::RefCell::new(vec![
                    setter.into_js_value(),
                    getter.into_js_value(),
                ]),
            }
        }

        pub fn eval(&self, src: &str) -> Result<(), String> {
            eval(src).map(|_| ()).map_err(|e| format!("{e:?}"))
        }

        pub fn eval_bool(&self, expr: &str) -> bool {
            self.try_eval_bool(expr)
                .unwrap_or_else(|e| panic!("求值失败：{expr} —— {e}"))
        }

        pub fn try_eval_bool(&self, expr: &str) -> Result<bool, String> {
            eval(expr)
                .map(|v| v.as_bool().unwrap_or(false))
                .map_err(|e| format!("{e:?}"))
        }

        pub fn eval_i32(&self, expr: &str) -> i32 {
            self.try_eval_i32(expr)
                .unwrap_or_else(|e| panic!("求值失败：{expr} —— {e}"))
        }

        pub fn try_eval_i32(&self, expr: &str) -> Result<i32, String> {
            eval(expr)
                .map(|v| v.as_f64().unwrap_or(f64::NAN) as i32)
                .map_err(|e| format!("{e:?}"))
        }

        /// 读一条 JS 表达式的字符串结果（与 native 腿同名同形，见 `native::Host::try_eval_string`）。
        pub fn try_eval_string(&self, expr: &str) -> Result<String, String> {
            eval(expr)
                .map(|v| v.as_string().unwrap_or_default())
                .map_err(|e| format!("{e:?}"))
        }

        pub fn eval_string(&self, expr: &str) -> String {
            self.try_eval_string(expr)
                .unwrap_or_else(|e| panic!("求值失败：{expr} —— {e}"))
        }

        pub fn calls(&self) -> Vec<(i32, i32)> {
            self.calls.borrow().clone()
        }

        /// 把一颗 Rust 函数挂成全局名（与 native 腿同名同形，签名也只有一个字符串进出一个字符串）。
        ///
        /// wasm 腿不需要 [`super::BRIDGE_JS`] 那一层跳板：这一格的调用方**本来就是我们的 JS 文本**，
        /// 它自己把参数写全（没有 `this` 要传）。跳板那一族只在"Rust 行为当 prototype 方法"时才要。
        pub fn bind_str(
            &self,
            name: &str,
            f: impl Fn(String) -> String + 'static,
        ) -> Result<(), String> {
            let c = Closure::wrap(Box::new(move |s: JsValue| -> JsValue {
                let input = s.as_string().unwrap_or_default();
                JsValue::from_str(&f(input))
            }) as Box<dyn FnMut(JsValue) -> JsValue>);
            let key = JsValue::from_str(name);
            Reflect::set(&js_sys::global(), &key, c.as_ref())
                .map_err(|e| format!("{name:?} 挂不上全局：{e:?}"))?;
            self._keepalive.borrow_mut().push(c.into_js_value());
            Ok(())
        }
    }
}

#[cfg(all(test, feature = "js"))]
mod tests {
    //! 四条判据写**一遍**，两条腿各跑一次（写法与 `sisf-event/src/js/tests.rs` 一致）。
    use super::{CALL_JS, PLUGIN_PROBE_JS};

    #[cfg(target_arch = "wasm32")]
    use super::web::Host;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    #[cfg(not(target_arch = "wasm32"))]
    use super::native::Host;

    #[test]
    fn a_rust_method_on_the_prototype_is_overwritable_from_js() {
        // 判据①：插件的 `Klass.prototype.m = function` 这一写法必须落得下去。
        let h = Host::boot();
        assert!(
            h.try_eval_bool("Object.getOwnPropertyDescriptor(Game_Variables.prototype, 'setValue').writable === true")
                .unwrap_or(false),
            "Rust 挂进 prototype 的方法不可写 ⇒ 2,756 处插件覆盖一条都接不住"
        );
    }

    #[test]
    fn an_overriding_plugin_still_reaches_the_rust_side() {
        // 判据②：覆盖 + alias 回调之后，那一次调用仍然进 Rust，而且带的是插件改过的值。
        let h = Host::boot();
        h.eval(PLUGIN_PROBE_JS).expect("插件脚本装载失败");
        h.eval(CALL_JS).expect("调用失败");
        assert_eq!(
            h.calls(),
            vec![(5, 20)],
            "覆盖没生效（记到 (5,10)）或 alias 没回到 Rust（流水为空）"
        );
        assert_eq!(h.eval_i32("$gameVariables.value(5)"), 20);
    }

    #[test]
    fn internal_fields_written_by_rust_are_readable_across_classes() {
        // 判据③：那 8,075 处跨类内部字段读，读的是 Rust 写进去的那一份。
        let h = Host::boot();
        h.eval(PLUGIN_PROBE_JS).expect("插件脚本装载失败");
        h.eval(CALL_JS).expect("调用失败");
        assert!(
            h.eval_bool("$gameVariables._value[5] === 20"),
            "JS 侧读不到 Rust 写的 `_value`"
        );
        assert!(
            h.eval_bool("Array.isArray($gameVariables._value)"),
            "`_value` 不是上游那个数组"
        );
    }

    #[test]
    fn each_instance_keeps_its_own_fields() {
        // 判据④：`DataManager.createGameObjects` 会 `new` 一遍所有类 ⇒ 状态必须是**每实例**的。
        let h = Host::boot();
        // 两个实例显式挂 `globalThis`：eval 里的 `var` 不留给下一条 eval（与 [`HOST_CLASS_JS`]
        // 那条注释是同一次实测的两格）。
        h.eval("globalThis.__a = new Game_Variables(); globalThis.__b = new Game_Variables(); __a.setValue(1, 11);")
            .expect("两实例调用失败");
        assert_eq!(h.calls().len(), 1);
        assert_eq!(h.eval_i32("__a.value(1)"), 11);
        assert_eq!(
            h.eval_i32("__b.value(1)"),
            0,
            "两个实例共用了一份状态（多半是挂到了类上而不是对象上）"
        );
        assert_eq!(h.eval_i32("$gameVariables.value(1)"), 0);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn script_code_is_evaluated_sloppy_like_a_browser_script() {
        // UMD 那一族（`TRP_Particle` 里的 pixi-particles、`MPP_EncounterEffect` 那类打包库）写的不是
        // "顶层 this"，而是 **IIFE 里的 `this`**：浏览器里 `<script>` 是 sloppy 代码，
        // `(function(){ this.PIXI = … })()` 的 `this` = window；而这一格在 QuickJS 里量出来是 undefined。
        // ⇒ 三条读数分别钉住：顶层是全局、普通调用的 this 是 undefined、显式 `.call(globalThis)` 能救。
        let h = Host::boot();
        assert!(
            h.eval_bool("this === globalThis"),
            "顶层 this 不是全局 ⇒ 连全局读写都接不住"
        );
        // sloppy 那一格：普通调用的函数里 `this` 必须是全局（上游 `TRP_Particle:210` 就靠它）。
        assert!(
            h.eval_bool("(function () { return this === globalThis; })()"),
            "普通调用的 this 不是全局 ⇒ 脚本又被当 strict 跑了（UMD 那一族会假死）"
        );
        assert!(
            h.eval_bool("(function () { return this === globalThis; }).call(globalThis)"),
            "显式 receiver 必须生效，否则包装那一格也没有出路"
        );
    }

    /// 墙钟探针（**不是判据**，`--ignored` 才跑）：状态住 JS 侧、行为住 Rust 侧那一格的**单价**。
    ///
    /// 为什么要有这一条：兼容层的形状是"每帧几万次 JS 调一个 Rust 挂上去的方法"（一张图几百个
    /// 事件 × 每个每帧十几个方法读数），单价乘上次数就是这一族能不能跑到 60fps 的全部依据。
    /// 按本项目 M9.6 那条纪律，绝对毫秒数不进默认闸门（机器速度会把假阈值弄红），只报读数与比值。
    /// 只在 native 腿跑：wasm 上 `std::time::Instant` 直接 panic（`xtask` 的 T3 事实第 7 条）。
    #[cfg(not(target_arch = "wasm32"))]
    #[ignore = "墙钟读数，不是判据；跑法 cargo test -p sisf-rpgmkmv --release --features js -- --ignored --nocapture"]
    #[test]
    fn the_rust_to_js_call_is_measured_per_beat() {
        const N: usize = 200_000;
        let h = Host::boot();
        // 同一语义的纯 JS 孪生方法（读同一个 `_value` 数组），当对照组。
        h.eval(
            "Game_Variables.prototype.jsValue = function (id) { return this._value[id] || 0; };",
        )
        .expect("对照方法装不上");
        h.eval("$gameVariables.setValue(5, 20);")
            .expect("预热写入失败");

        let one = |body: &str| {
            let src = format!("(function(){{var n={N},s=0;while(--n>=0)s+={body};return s;}})()");
            let t = std::time::Instant::now();
            let got = h.eval_i32(&src);
            (got, t.elapsed().as_nanos() as f64 / N as f64)
        };
        let (a, ns_rust) = one("$gameVariables.value(5)");
        let (b, ns_js) = one("$gameVariables.jsValue(5)");
        let (c, ns_field) = one("$gameVariables._value[5]");
        assert_eq!(a, 20 * N as i32);
        assert_eq!(b, 20 * N as i32);
        assert_eq!(c, 20 * N as i32);
        println!(
            "每拍单价（release，{N} 次）：Rust 桥 {ns_rust:.1} ns · 纯 JS 方法 {ns_js:.1} ns · 直读字段 {ns_field:.1} ns ⇒ Rust/JS 比值 {:.1}",
            ns_rust / ns_js,
        );
    }
}
