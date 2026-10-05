//! 兼容层的**形状普查**：从上游 `rpg_*.js` 的源码里读出"它交出过哪些全局名字"，
//! 再由这些名字生成 JS 侧的类形状与跳板表。
//!
//! ## 为什么是"扫上游源码"而不是"抄一份名单"
//!
//! 要顶替的是 `rpg_*.js` 那一层，而插件按名字压它的形状（实测：读走原型方法 972 处、
//! 改写 2,756 处，见 `sisf_pray/tools/probes/compat-census.py`）。名单只要手抄过一遍，
//! 就会与上游飘开，而飘开那一格在浏览器里的表现是"某条插件静默不生效"——最难查的那一类。
//! 所以名单**只从被顶替的那份源码算**：[`scan`] 是唯一来源，[`install_js`] 与 [`trampoline_js`]
//! 都从它的输出长出来（判据 `the_trampoline_table_is_generated_from_the_shape_census`
//! 钉的就是"表里几格 ⇔ 文本几段"）。
//!
//! ## 这一格只是**形状**，不是行为
//!
//! [`install_js`] 交出的是空的构造器与空的对象（`globalThis.Game_Variables = function () {};`），
//! 目的是让插件**装载期**那两句 `X.prototype.m = …` / `var alias = X.prototype.m` 落得下去。
//! 行为从 M10.2 起一条条填，填一条就进 [`trampoline_js`] 那张表一条 —— wasm 腿必须有跳板，
//! 因为 `wasm_bindgen` 的 `Closure` 拿不到 JS 的接收者（实测记在 `jsrt` 模块头第 1 条事实）。
//!
//! ## 上游的声明形状（实测于 `www/js/`，两条尺子都对得上）
//!
//! 六个核心文件里**全部**形状都是行首 `function X(` 声明，共 **153 个**；其中 **19 个**是
//! MV 的静态类（`DataManager`/`AudioManager`/`Graphics`/`Utils` 那一族），标记是体内的
//! `throw new Error('This is a static class')` —— 这一族**没有人给它写 prototype 方法**
//! （另外 134 个类才有，共 3,107 条 `X.prototype.m =`，那两个数来自 `Number`/`String` 的
//! 扩展，由 `JsExtensions` 加，不是 MV 自己的类）。
//!
//! **只有顶格才算**：插件与内层代码里的 `function helper()` 带着缩进，不是全局形状。
//! 上游不用 `var X = {` 那一族声明全局（实测 0 条），所以扫描器不认它——认了就是臆想。

/// 一个被上游交出的全局名字的形状种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    /// 类（有 `prototype`，插件压 `X.prototype.m`）。
    Class,
    /// MV 的静态类（`DataManager` 那一族：只挂静态方法，**不许 `new`**）。
    StaticClass,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredShape {
    pub name: String,
    pub kind: ShapeKind,
}

/// 从一份上游 JS 源码里扫出全部行首声明。顺序 = 源码顺序；同名只留第一次
/// （后一份声明只是在同一形状上补方法，形状本身只有一份）。
pub fn scan(src: &str) -> Vec<DeclaredShape> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out: Vec<DeclaredShape> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(rest) = line.strip_prefix("function ") else {
            continue;
        };
        let Some(name) = ident(rest) else { continue };
        let kind = if is_static_guard(rest, &lines[i + 1..]) {
            ShapeKind::StaticClass
        } else {
            ShapeKind::Class
        };
        push(&mut out, name, kind);
    }
    out
}

/// MV 静态类的那一句守卫。只看**声明行与它后面第一条非空行** —— 多 look 一行就会把紧挨着的
/// 下一个声明误判成静态类（`function Game_Switches() {}` 后面跟着 `function AudioManager()`
/// 的 throw，第一次就是这么红的）。
fn is_static_guard(decl_line: &str, following: &[&str]) -> bool {
    if decl_line.contains("This is a static class") {
        return true;
    }
    // 声明行自己就收尾（`function X() {}`）的，体内不会再有守卫句。
    if decl_line.contains('}') {
        return false;
    }
    following
        .iter()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .is_some_and(|l| l.contains("This is a static class"))
}

fn push(out: &mut Vec<DeclaredShape>, name: String, kind: ShapeKind) {
    if !out.iter().any(|s| s.name == name) {
        out.push(DeclaredShape { name, kind });
    }
}

/// 取行首的合法 JS 标识符（`$`/`_` 起头也算 —— `ImageManager`、`Yanfly` 那一族都在里面）。
fn ident(rest: &str) -> Option<String> {
    let mut out = String::new();
    for ch in rest.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
            out.push(ch);
        } else {
            break;
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

impl DeclaredShape {
    /// 这一个形状在 JS 侧的赋值语句。显式挂 `globalThis` —— `js_sys::eval` 不把 `function`
    /// 声明漏成全局（实测记在 `jsrt` 模块头第 2 条事实）。
    ///
    /// **类那一支的构造体不是空的，是上游那一句**（M10.3 量出来的，尺子
    /// `sisf_pray/tools/probes/game-touch.py`）：六个核心文件的 **153** 条声明里
    /// **130** 条的体就是 `this.initialize.apply(this, arguments);`，而其余 **23** 条是
    /// **18** 条静态类守卫 + `JsExtensions`（`This is not a class`）+ 四条例外
    /// （`CacheEntry`/`CacheMap`/`ShaderTilemap`/`ToneFilter`，全在 `rpg_core.js`，M10.4 那一族）。
    /// 交空构造体（旧写法）的代价是实测出来的：`new Game_Variables()` 得到一个**不会 `initialize`**
    /// 的对象，于是 M10.3 的行为件必须每条都自己喊 `obj.initialize()` —— 那是兼容层比上游严，
    /// 而插件的 `new Game_X()` 恰恰假定构造体在那一拍已经把字段建好了（`Game_Picture.prototype.show`
    /// 之后紧跟的 `update` 读的就是构造期建的 `_targetX`）。
    /// 所以这一格交**上游的构造体**，行为件只往 `prototype` 上挂方法（挂方法可重复 ⇒ 双装安全）。
    pub fn install_line(&self) -> String {
        // **构造体必须带名字**（`function X (` 而不是匿名 `function (`）：上游是顶层
        // `function Game_Variables(){}`，浏览器里那颗的 `Function.prototype.name` 就是 `"Game_Variables"`，
        // 而 `JsonEx._encode`（`rpg_core.js:9035-9038`）写的 `"@"` 读的正是在这里，
        // `JsonEx._decode`（`:9082-9087`）再拿那个名字去 `window[名字]` 找回 prototype。
        // 匿名 ⇒ `name === ""` ⇒ 存档往返把每一颗 `Game_*` 都读回成普通 `Object`
        // （2026-10-05 定案"兼容上游 rpgsave"那一格的第一条判据抓到的，见 `M10.md` 事实 77）。
        match self.kind {
            ShapeKind::Class => format!(
                "globalThis.{n} = function {n} () {{ this.initialize.apply(this, arguments); }};\n",
                n = self.name
            ),
            // 静态类保住上游那句守卫：`new DataManager()` 在上游就是抛，这里不许它悄悄成功。
            ShapeKind::StaticClass => format!(
                "globalThis.{n} = function {n} () {{ throw new Error('This is a static class'); }};\n",
                n = self.name
            ),
        }
    }
}

/// 一批形状的 JS 文本。
pub fn install_js(shapes: &[DeclaredShape]) -> String {
    shapes.iter().map(DeclaredShape::install_line).collect()
}

/// 一条**已交行为**的登记项：`class` 上的 `method`。JS 侧的符号名由 [`symbol_name`] 唯一决定，
/// 不留手命名的空间（两张名字表就是两个真相）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delivered {
    pub class: String,
    pub method: String,
}

impl Delivered {
    pub fn new(class: &str, method: &str) -> Self {
        Self {
            class: class.to_string(),
            method: method.to_string(),
        }
    }

    /// 这条行为在 JS 侧的符号名。带类名前缀是因为两个类会有同名方法
    /// （上游 `Sprite_Base.update` 与 `Window_Base.update` 各一份）。
    pub fn symbol(&self) -> String {
        symbol_name(&self.class, &self.method)
    }
}

pub fn symbol_name(class: &str, method: &str) -> String {
    format!("__sisf_{class}_{method}")
}

/// 一条已交方法的跳板（wasm 腿用）。ES5 写法：MV 本体是 ES5，而插件混着各家写法，
/// 生成物保守到 ES5 就不在这一格挑引擎（rquickjs 与浏览器走同一条腿）。
pub fn trampoline_line(d: &Delivered) -> String {
    format!(
        "{n}.prototype.{m} = function () {{ return {s}.apply(null, [this].concat(Array.prototype.slice.call(arguments))); }};\n",
        n = d.class,
        m = d.method,
        s = d.symbol(),
    )
}

/// 跳板表：每条已交方法一段，把 JS 的接收者显式当第一参交给 Rust。
pub fn trampoline_js(delivered: &[Delivered]) -> String {
    delivered.iter().map(trampoline_line).collect()
}

/// 把一批形状装进宿主。
#[cfg(feature = "js")]
pub fn install(host: &crate::jsrt::Host, shapes: &[DeclaredShape]) -> Result<(), String> {
    host.eval(&install_js(shapes))
}

/// 上游往 **JS 内建**上挂的那一族（`rpg_core.js` 的 `JsExtensions`：
/// `Number.prototype.clamp/mod/padZero`、`String.prototype.format/padZero/contains`、
/// `Array.prototype.equals/clone/contains`、`Math.randomInt`）。
///
/// 这一族**不在"153 个形状"里**（形状普查盖不到它），而几乎每个插件的顶层都在用它 ——
/// 装载账单里"not a function"那一堆的根就在这一格。文本逐条照上游（`rpg_core.js:24-152`
/// 与 `:79-131`），两条腿同一份常量。数组那三条留着 `enumerable: false`：上游刻意不让它们
/// 进 `for (var k in arr)`，这一格照抄而不是顺手写成普通赋值。
pub const JS_EXTENSIONS_JS: &str = r#"
Number.prototype.clamp = function (min, max) { return Math.min(Math.max(this, min), max); };
Number.prototype.mod = function (n) { return ((this % n) + n) % n; };
Number.prototype.padZero = function (length) { return String(this).padZero(length); };
String.prototype.format = function () {
    var args = arguments;
    return this.replace(/%([0-9]+)/g, function (s, n) { return args[Number(n) - 1]; });
};
String.prototype.padZero = function (length) {
    var s = String(this);
    while (s.length < length) { s = '0' + s; }
    return s;
};
String.prototype.contains = function (string) { return this.indexOf(string) >= 0; };
Math.randomInt = function (max) { return Math.floor(max * Math.random()); };
// **只在没有的时候定义**：上游那三条的描述符只有 `enumerable: false`（⇒ `configurable` 默认 false），
// 第二次 `Object.defineProperties` 必抛 `Cannot redefine property: equals`。这一格是量出来的腿差：
// native 每颗 `Host` 一个独立的 QuickJS realm，而 **node 的 wasm 测试腿一整个进程共享一个 realm**
// ⇒ 同一份文本会被多个测试反复装。上游碰不到，是因为 `rpg_core.js` 一个页面只 `<script>` 一次。
// 守"只定义一次"而不是给描述符加 `configurable: true`：后者会让 `delete Array.prototype.contains`
// 这一族在兼容层里成功、在上游里失败。
if (typeof Array.prototype.equals !== 'function') {
    Object.defineProperties(Array.prototype, {
        equals: {
            enumerable: false,
            value: function (array) {
                if (!array || this.length !== array.length) { return false; }
                for (var i = 0; i < this.length; i++) {
                    if (this[i] instanceof Array && array[i] instanceof Array) {
                        if (!this[i].equals(array[i])) { return false; }
                    } else if (this[i] !== array[i]) {
                        return false;
                    }
                }
                return true;
            }
        },
        clone: { enumerable: false, value: function () { return this.slice(0); } },
        contains: { enumerable: false, value: function (element) { return this.indexOf(element) >= 0; } }
    });
}
"#;

/// 交出 `JsExtensions` 那一族：装完形状、装插件之前调一次。
#[cfg(feature = "js")]
pub fn install_extensions(host: &crate::jsrt::Host) -> Result<(), String> {
    host.eval(JS_EXTENSIONS_JS)
}

#[cfg(all(test, feature = "js"))]
mod tests {
    use super::*;
    use crate::jsrt::Host;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    /// 上游声明形状的样品（照 `rpg_objects.js` 与 `rpg_managers.js` 的写法缩出来）。
    const FIXTURE_JS: &str = "\
function Game_Variables() {
    this.initialize.apply(this, arguments);
}

Game_Variables.prototype.initialize = function () {
    this._value = [];
};

function Game_Switches() {}

function AudioManager() {
    throw new Error('This is a static class');
}

(function () {
    function notAWorldShape() {}
})();
";

    #[test]
    fn scan_reads_line_anchored_declarations_only() {
        let shapes = scan(FIXTURE_JS);
        let names: Vec<&str> = shapes.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Game_Variables", "Game_Switches", "AudioManager"],
            "只认行首声明：闭包里的 `function notAWorldShape()` 不是全局形状"
        );
        assert_eq!(shapes[0].kind, ShapeKind::Class);
        assert_eq!(shapes[2].kind, ShapeKind::StaticClass);
    }

    #[test]
    fn installed_shapes_take_plugin_overrides_at_load_time() {
        // M10.1 的全部目的：插件装载期那两句（读走 + 改写）落得下去。
        let h = Host::boot();
        install(&h, &scan(FIXTURE_JS)).expect("形状装不上");
        // 兼容层"已交的一条行为"（M10.2 起由 Rust 挂上来；`jsrt` 判据②钉的就是 Rust 那一支，
        // 这里只钉形状那一格，所以用一条 JS 方法代表"已交"）。
        h.eval("Game_Variables.prototype.initialize = function () { this._value = []; };")
            .expect("已交行为挂不上");
        h.eval(
            "var alias = Game_Variables.prototype.initialize; \
             Game_Variables.prototype.initialize = function () { alias.call(this); this._fromPlugin = 7; }; \
             globalThis.__gv = new Game_Variables(); __gv.initialize();",
        )
        .expect("插件式覆盖没落下去");
        assert!(
            h.eval_bool("__gv._fromPlugin === 7 && __gv._value.length === 0"),
            "覆盖或 alias 没生效"
        );
    }

    #[test]
    fn a_one_line_class_next_to_a_static_one_is_not_static() {
        // 守卫那一眼只能看一行：这条就是第一次红的那个形状。
        let src = "function Game_Switches() {}\n\nfunction DataManager() {\n    throw new Error('This is a static class');\n}\n";
        let shapes = scan(src);
        assert_eq!(shapes.len(), 2);
        assert_eq!(shapes[0].kind, ShapeKind::Class);
        assert_eq!(shapes[1].kind, ShapeKind::StaticClass);
    }

    #[test]
    fn installed_static_classes_keep_the_upstream_guard() {
        // `new DataManager()` 在上游是抛，兼容层不许让它悄悄成功（那是另一类静默降级）。
        let h = Host::boot();
        install(&h, &scan(FIXTURE_JS)).expect("形状装不上");
        assert!(
            h.eval_bool(
                "var threw = false; try { new AudioManager(); } catch (e) { threw = true; } threw;"
            ),
            "静态类的守卫没保住"
        );
        assert!(h.eval_bool("typeof AudioManager.playBgm === 'undefined'"));
    }

    /// **每一颗形状的构造体都要带名字**（`M10.md` 事实 77，2026-10-05 存档改判之后抓到的）。
    ///
    /// 上游是顶层 `function Game_Variables(){}` ⇒ 浏览器里 `Game_Variables.name` 就是那颗类名，
    /// 而 `JsonEx._encode` 写进存档的 `"@"` 那一键读的正是它（`rpg_core.js:9035-9038`），
    /// `_decode`（`:9082-9087`）再拿那个名字去 `window[名字]` 找回 prototype。
    /// 壳一旦写成匿名（`globalThis.X = function () {}`）：赋值给**属性**不做具名求值 ⇒
    /// `name === ""` ⇒ `"@"` 是假值 ⇒ 每一颗 `Game_*` 读回来都退化成普通 `Object`，
    /// **不抛、不红、下一拍才咬人**（`M10.md` 咬人第 1 条 `RegExp.$n` 同一种形状）。
    ///
    /// 这一条钉**源头那一行**，`core.rs` 的
    /// `json_ex_round_trips_the_class_graph_with_its_circular_references` 钉**往返真的对得上** ——
    /// 两头都要有，只有一头的话说不出另一头什么时候坏。
    #[test]
    fn the_shape_names_survive_for_json_ex() {
        let h = Host::boot();
        install(&h, &scan(FIXTURE_JS)).expect("形状装不上");
        assert_eq!(
            h.eval_string(
                "Game_Variables.name + '|' + Game_Switches.name + '|' + AudioManager.name"
            ),
            "Game_Variables|Game_Switches|AudioManager",
            "`install_line` 又交回匿名构造体了（两种 kind 都要带名字）—— 存档往返会静默丢掉每一颗类的 prototype"
        );
    }

    #[test]
    fn the_js_extensions_match_the_upstream_bodies() {
        // `JsExtensions` 那一族（`rpg_core.js:24-152`）的语义，逐条钉住上游的算法。
        let h = Host::boot();
        install_extensions(&h).expect("JsExtensions 该装得动");
        for (expr, want) in [
            ("(-10).mod(60)", 50.0), // 上游：((this % n) + n) % n，负数也要正余数
            ("(5).clamp(1, 3)", 3.0),
            ("('a b').replace(/x/, 'y')", 0.0),
        ] {
            let _ = want;
            let _ = expr;
        }
        assert!(h.eval_bool("(5).clamp(1, 3) === 3 && (-10).mod(60) === 50"));
        assert!(
            h.eval_bool("'01234'.padZero(6) === '001234'"),
            "String.padZero"
        );
        assert!(
            h.eval_bool("(12).padZero(4) === '0012'"),
            "Number.padZero 走 String.padZero"
        );
        assert!(
            h.eval_bool("'%2/%1'.format('a', 'b') === 'b/a'"),
            "format 是 %1 起算，不是 0 起算"
        );
        assert!(
            h.eval_bool("'abc'.contains('bc') && [1, 2].contains(2) && [1, [2]].equals([1, [2]])")
        );
        assert!(h.eval_bool("[1, 2].clone().equals([1, 2])"));
        // `enumerable: false` 那一格：上游刻意不让这三条进 `for…in`。
        assert!(
            h.eval_bool("Object.keys(Array.prototype).length === 0"),
            "数组那三条扩展是可枚举的，与上游不一致（上游 enumerable: false）"
        );
        // 同一条描述符的第二格：上游没写 `configurable` ⇒ **不可配置**。
        // 这一格不是洁癖：加了 `configurable: true` 就能"重新定义"过去，而 wasm 测试腿共享一个
        // realm 时那正是我想省掉守卫的写法 —— 守卫 + 保住描述符才是与上游同一颗属性。
        assert!(
            h.eval_bool(
                "var d = Object.getOwnPropertyDescriptor(Array.prototype, 'contains'); \
                 d.enumerable === false && d.configurable === false && d.writable === false"
            ),
            "`Array.prototype.contains` 的描述符与上游不同（上游只给 enumerable: false）"
        );
    }

    #[test]
    fn the_js_extensions_can_be_installed_twice_in_one_realm() {
        // 实测的腿差（node 那条腿一整个进程共享一个 realm，native 每颗 `Host` 一个 realm）：
        // 同一份文本装第二次不许抛 `Cannot redefine property: equals`。
        let h = Host::boot();
        install_extensions(&h).expect("第一次装 JsExtensions 失败");
        install_extensions(&h).expect(
            "第二次装 JsExtensions 失败 ⇒ 幂等那一格守卫没接住（共享 realm 的那条腿会整批红）",
        );
        assert!(h.eval_bool("[1, 2].contains(2)"));
    }

    #[test]
    fn the_trampoline_forwards_the_receiver_as_the_first_argument() {
        // 跳板的语义（两条腿同一条断言）：Rust 那侧的第一参必须是 JS 的接收者。
        let delivered = [Delivered::new("Game_Variables", "initialize")];
        let h = Host::boot();
        install(&h, &scan(FIXTURE_JS)).expect("形状装不上");
        // 用一个 JS 桩替 Rust 函数：浏览器腿的真函数由 `wasm_bindgen::Closure` 提供，
        // 而它的调用形状与这个桩一样（第一参是接收者）。桩也显式挂 `globalThis` ——
        // `function` 声明在 `js_sys::eval` 里不会漏成全局（`jsrt` 模块头第 2 条事实，
        // 这一条测试第一次跑就是这样红的）。
        h.eval("globalThis.__sisf_Game_Variables_initialize = function (receiver) { receiver.marker = 1; };")
            .expect("桩装不上");
        h.eval(&trampoline_js(&delivered)).expect("跳板装不上");
        h.eval("globalThis.__gv = new Game_Variables(); __gv.initialize();")
            .expect("调用失败");
        assert!(
            h.eval_bool("__gv.marker === 1"),
            "接收者没作为第一参交出去 ⇒ 这一族跳板接不住 Rust 行为"
        );
    }

    #[test]
    fn the_trampoline_table_is_generated_from_the_shape_census() {
        // 判据⑷的本体：表里几格 ⇔ 文本几段，一段不多一段不少。
        let delivered: Vec<Delivered> = scan(FIXTURE_JS)
            .iter()
            .filter(|s| s.kind == ShapeKind::Class)
            .flat_map(|s| ["initialize", "update"].map(|m| Delivered::new(&s.name, m)))
            .collect();
        assert_eq!(delivered.len(), 4, "两个类 × 两条方法");
        let table = trampoline_js(&delivered);
        assert_eq!(
            table.lines().filter(|l| !l.trim().is_empty()).count(),
            delivered.len(),
            "跳板段数与登记表不符（生成了手抄的份）"
        );
        assert!(
            table.contains("__sisf_Game_Switches_update.apply(null, [this]"),
            "符号名不是从 `class`+`method` 算出来的：{table}"
        );
    }
}
