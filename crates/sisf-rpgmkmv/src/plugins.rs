//! `plugins.js` 的注册表与**装载账单**（M10.1 那一族）。
//!
//! ## 两件事
//!
//! 1. [`Registry`]：读 MV 生成的 `var $plugins = [ {name, status, description, parameters} , … ]`，
//!    把**每个插件对象的原文切片**留着，交进 JS 时逐字还原。参数对象不在 Rust 里重建：
//!    `serde_json` 默认的 map 会按键排序，而 `PluginManager.parameters(name)` 在上游是
//!    `JSON.parse` 的产物 ⇒ 键序 = 文件里的顺序，个别插件确实按顺序取参。少一次重排就少一个真相。
//! 2. [`load`]：把启用那一族的源码逐个 eval 进宿主，**每个失败都记名带原因**。
//!    这一份账单就是兼容层的"还缺哪一格"清单 —— 它不许 panic、不许静默，也不许把
//!    "0 个插件起得来"当成绿（同本项目"0 个用例通过不是绿"那条纪律）。
//!
//! ## 引擎侧不知道游戏在哪
//!
//! 文件路径与源码由调用方递进来（`pray` 那侧走 `upstream_www()` + VFS）。本 crate 只认文本，
//! 所以别的 MV 工程也能用同一张注册表。

use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plugin {
    pub name: String,
    pub status: bool,
    /// 这个插件对象在 `plugins.js` 里的**原文**（含 `parameters` 的键序）。
    pub raw: String,
    /// `"parameters"` 那一段的原文（`{…}` 一整块）。缺键就是 `None` ⇒ 上游交 `{}`。
    pub parameters_raw: Option<String>,
}

/// 一张注册表：`plugins.js` 里声明的全部插件，顺序照文件。
#[derive(Clone, Debug)]
pub struct Registry {
    pub entries: Vec<Plugin>,
    /// 整个数组的原文，`install` 时逐字交给 JS。
    array_text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryError {
    /// 找不到 `var $plugins = [...]` 那一段（文件形状不是 MV 生成的那一族）。
    NoPluginArray,
    /// 某个元素不是对象或没有 `name`。
    MalformedEntry(String),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoPluginArray => f.write_str("`plugins.js` 里找不到 `var $plugins = [...]`"),
            Self::MalformedEntry(s) => write!(f, "插件项形状不认识：{s}"),
        }
    }
}

impl std::error::Error for RegistryError {}

impl Registry {
    /// 从 `plugins.js` 的全文解析。
    pub fn parse(src: &str) -> Result<Self, RegistryError> {
        let start = find_array(src).ok_or(RegistryError::NoPluginArray)?;
        let array_text = slice_brackets(&src[start..])
            .ok_or(RegistryError::NoPluginArray)?
            .to_string();
        let mut entries = Vec::new();
        for raw in top_level_objects(&array_text) {
            let value: Value = serde_json::from_str(raw)
                .map_err(|e| RegistryError::MalformedEntry(format!("{raw}（{e}）")))?;
            let name = value
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| RegistryError::MalformedEntry(raw.to_string()))?
                .to_string();
            let status = match value.get("status") {
                // MV 1.6.1 写布尔；更早的版本写 0/1/2。两条都认，缺字段当关闭。
                Some(Value::Bool(b)) => *b,
                Some(Value::Number(n)) => n.as_i64().unwrap_or(0) != 0,
                _ => false,
            };
            entries.push(Plugin {
                parameters_raw: parameters_slice(raw),
                name,
                status,
                raw: raw.to_string(),
            });
        }
        Ok(Self {
            entries,
            array_text,
        })
    }

    pub fn total(&self) -> usize {
        self.entries.len()
    }

    /// 启用那一族的名字（顺序照文件）。
    pub fn enabled(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|p| p.status)
            .map(|p| p.name.as_str())
            .collect()
    }

    pub fn is_enabled(&self, name: &str) -> bool {
        self.entries.iter().any(|p| p.name == name && p.status)
    }

    /// 把 `$plugins` 这颗全局对象逐字交进宿主（上游 `rpg_managers.js` 的 `PluginManager`
    /// 读的就是这一颗）。
    #[cfg(feature = "js")]
    pub fn install(&self, host: &crate::jsrt::Host) -> Result<(), String> {
        host.eval(&format!("globalThis.$plugins = {};\n", self.array_text))
    }

    /// 交出 `PluginManager` 那一族的**可见形状**（上游 `rpg_managers.js:600-660`）：
    /// `_parameters`（键是小写名）、`parameters(name)`、`setParameters`、`checkErrors`、
    /// `_scripts`、`_errorUrls`、`_path`。
    ///
    /// **不交 `loadScript`** —— 上游那一格是 `document.createElement('script')`，而兼容层的
    /// 边界里没有 DOM：源码由调用方递给 [`load`]。`_scripts` 因此在这里按注册表顺序预填，
    /// 与上游同一条语义（上游 `setup` 是 `loadScript` 之后**当场** `push`，不等 onload）。
    #[cfg(feature = "js")]
    pub fn install_manager(&self, host: &crate::jsrt::Host) -> Result<(), String> {
        let mut js = String::from(
            "PluginManager._path = 'js/plugins/';\n\
             PluginManager._scripts = [];\n\
             PluginManager._errorUrls = [];\n\
             PluginManager._parameters = {};\n\
             PluginManager.setParameters = function (name, parameters) { this._parameters[name.toLowerCase()] = parameters; };\n\
             PluginManager.parameters = function (name) { return this._parameters[name.toLowerCase()] || {}; };\n\
             PluginManager.checkErrors = function () { var url = this._errorUrls.shift(); if (url) { throw new Error('Failed to load: ' + url); } };\n",
        );
        for p in self.entries.iter().filter(|p| p.status) {
            let name = serde_json::to_string(&p.name).unwrap_or_else(|_| "\"\"".to_string());
            let params = p.parameters_raw.as_deref().unwrap_or("{}");
            js.push_str(&format!(
                "PluginManager.setParameters({name}, {params}); PluginManager._scripts.push({name});\n"
            ));
        }
        host.eval(&js)
    }
}

/// 取一个插件对象里 `"parameters": {…}` 那一块的原文（键序保住，不在 Rust 里重建）。
fn parameters_slice(obj: &str) -> Option<String> {
    for (at, _) in obj.match_indices("\"parameters\"") {
        let after = &obj[at + "\"parameters\"".len()..];
        if after.trim_start().starts_with(':') {
            let brace = after.find('{')? + at + "\"parameters\"".len();
            return span(&obj[brace..], b'{', b'}').map(str::to_string);
        }
    }
    None
}

/// 找 `var $plugins =` 之后第一个 `[` 的位置。
fn find_array(src: &str) -> Option<usize> {
    let at = src.find("var $plugins")?;
    let tail = &src[at..];
    let bracket = tail.find('[')?;
    Some(at + bracket)
}

/// 从 `s` 的 `[` 起，配对到与之平衡的 `]`（字符串里的括号与转义都不算数）。
fn slice_brackets(s: &str) -> Option<&str> {
    span(s, b'[', b']')
}

fn span(s: &str, open: u8, close: u8) -> Option<&str> {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&open) {
        return None;
    }
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (i, b) in bytes.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if *b == b'\\' {
                escaped = true;
            } else if *b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            x if *x == open => depth += 1,
            x if *x == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// 数组原文里每个顶层 `{…}` 的切片。
fn top_level_objects(array_text: &str) -> Vec<&str> {
    let bytes = array_text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let rest = &array_text[i..];
            if let Some(piece) = span(rest, b'{', b'}') {
                out.push(piece);
                i += piece.len();
                continue;
            }
        }
        i += 1;
    }
    out
}

/// 一次装载的账单。
#[derive(Clone, Debug, Default)]
pub struct LoadReport {
    /// 真正 eval 过的插件名（= 启用 ∩ 递进来的源码）。
    pub attempted: Vec<String>,
    pub loaded: Vec<String>,
    /// `(插件名, 引擎交回的异常文本)` —— 每一条都必须带名字，否则查无可查。
    pub failed: Vec<(String, String)>,
    /// 注册表里启用、但调用方没给源码的插件名（素材缺失/没接的那一族）。
    pub missing_source: Vec<String>,
    /// **eval 过了顶层、却在装载期间往 `console` 喊了话的插件**：`(插件名, 那几条)`。
    ///
    /// 为什么这一格必须存在：插件放弃自己的一种合法写法是"喊一句就 `return`"—— 实测第一例
    /// `FTKR_SkillTreeSystem:1022-1026`（`if (!Window_Base.prototype.reserveFaceImages)
    /// { console.error('…コアスクリプトを最新版に…'); return; }`）。顶层 eval 成功 ⇒ 它进
    /// [`LoadReport::loaded`]，而整个插件体一条都没落。只有 `loaded` 一个数的话，进度尺就把它读成
    /// "起来了"。所以账单把"喊过话"单独列一堆，`起 102` 这一行才是可解读的。
    /// 账来自 [`crate::platform`] 那颗记账 `console`（两条腿都记），逐条插件取走 ⇒ 归堆是准的。
    pub logged: Vec<(String, Vec<(String, String)>)>,
}

impl LoadReport {
    pub fn loaded_count(&self) -> usize {
        self.loaded.len()
    }

    /// 一行读数，进 `--nocapture` 与账单。
    pub fn summary(&self) -> String {
        format!(
            "插件装载：起 {n}（其中自己喊过话 {s}）／试 {t}／失败 {f}／缺源码 {m}",
            n = self.loaded.len(),
            s = self.logged.len(),
            t = self.attempted.len(),
            f = self.failed.len(),
            m = self.missing_source.len(),
        )
    }
}

/// 逐个 eval 启用那一族的源码。`sources` 是 `(插件名, 源码)`；没启用的**一条都不跑**
/// （上游 `PluginManager.setup` 就是这个条件，跑禁用插件会让"没启用的插件"反过来影响状态）。
#[cfg(feature = "js")]
pub fn load(host: &crate::jsrt::Host, registry: &Registry, sources: &[(&str, &str)]) -> LoadReport {
    let mut report = LoadReport::default();
    for name in registry.enabled() {
        if let Some(src) = sources.iter().find(|(n, _)| *n == name).map(|(_, s)| *s) {
            report.attempted.push(name.to_string());
            // 先清一次账（正常情况下上一条已经在取走时清空），再取这一条插件喊的那几句。
            crate::platform::take_console_lines(host);
            let eval = host.eval(src);
            let lines = crate::platform::take_console_lines(host);
            match eval {
                Ok(()) => {
                    report.loaded.push(name.to_string());
                    if !lines.is_empty() {
                        report.logged.push((name.to_string(), lines));
                    }
                }
                // 挂掉的插件也可能先喊过话（那一族文本在 `failed` 的异常文本里够用了，
                // 不重复列，免得同一条在两个堆里各记一次）。
                Err(e) => report.failed.push((name.to_string(), e)),
            }
        } else {
            report.missing_source.push(name.to_string());
        }
    }
    report
}

#[cfg(all(test, feature = "js"))]
mod tests {
    use super::*;
    use crate::jsrt::Host;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    /// 上游那一族的形状（`description` 里带 `\r\n` 与 `{}`，就是为了让切片器踩坑）。
    const PLUGINS_JS: &str = r#"// Generated by RPG Maker.
// Do not edit this file directly.
var $plugins =
[
{"name":"YEP_CoreEngine","status":true,"description":"v1.13 Also\r\ncontains {} fixes.","parameters":{"Screen Width":"1104","Screen Height":"736"}},
{"name":"DeadPlugin","status":false,"description":"没启用","parameters":{"A":"1","B":"2"}},
{"name":"ADVTalkSystem","status":true,"description":"talk","parameters":{"Talk Mode Switch":"101"}}
];
"#;

    #[test]
    fn registry_reads_status_and_keeps_the_parameter_text_verbatim() {
        let r = Registry::parse(PLUGINS_JS).expect("注册表该解析得动");
        assert_eq!(r.total(), 3);
        assert_eq!(r.enabled(), vec!["YEP_CoreEngine", "ADVTalkSystem"]);
        // 键序逐字保住：`{"B":…,"A":…}` 那种"排过序"的输出就是这里飘出去的。
        let core = &r.entries[0].raw;
        assert!(
            core.contains(r#""Screen Width":"1104","Screen Height":"736""#),
            "参数对象被重排了：{core}"
        );
        assert!(!r.is_enabled("DeadPlugin"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_installed_registry_is_the_object_js_would_see() {
        let h = Host::boot();
        let r = Registry::parse(PLUGINS_JS).expect("注册表该解析得动");
        r.install(&h).expect("$plugins 该装进宿主");
        assert_eq!(h.eval_i32("$plugins.length"), 3);
        assert_eq!(
            h.eval_i32(r#"Number($plugins[0].parameters["Screen Width"])"#),
            1104
        );
        assert!(h.eval_bool("$plugins[0].status === true && $plugins[1].status === false"));
        // 上游那一族还会读到 description 里的 `{} fixes.` —— 切片没切坏的证据。
        assert!(h.eval_bool(r#"$plugins[0].description.indexOf("{} fixes.") >= 0"#));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn plugin_manager_speaks_the_upstream_parameter_contract() {
        // 装载账单里"not a function"那一堆的根：`PluginManager.parameters(name)`。
        // 上游语义（`rpg_managers.js:625-640`）：键是**小写名**，取不到交 `{}`。
        let h = Host::boot();
        crate::shapes::install_extensions(&h).expect("内建扩展装不上");
        let shapes = crate::shapes::scan(
            "function PluginManager() {\n    throw new Error('This is a static class');\n}\n",
        );
        crate::shapes::install(&h, &shapes).expect("PluginManager 的形状装不上");
        let r = Registry::parse(PLUGINS_JS).expect("解析");
        r.install_manager(&h).expect("PluginManager 那一族装不上");
        assert_eq!(
            h.eval_i32("Number(PluginManager.parameters('YEP_CoreEngine')['Screen Width'])"),
            1104
        );
        assert!(
            h.eval_bool("PluginManager.parameters('yep_coreengine')['Screen Height'] === '736'"),
            "键没有按小写归一（上游是 name.toLowerCase()）"
        );
        assert!(h.eval_bool("PluginManager.parameters('Nope').constructor === Object"));
        assert!(
            h.eval_bool("!('DeadPlugin' in PluginManager._parameters) && PluginManager._scripts.contains('YEP_CoreEngine')"),
            "禁用的那条不该被登记，启用的要进 _scripts（上游 setup 当场 push）"
        );
    }

    #[test]
    fn only_enabled_plugins_are_loaded() {
        let h = Host::boot();
        let r = Registry::parse(PLUGINS_JS).expect("解析");
        let report = load(
            &h,
            &r,
            &[
                ("YEP_CoreEngine", "globalThis.__ran1 = 1;"),
                ("DeadPlugin", "globalThis.__ran2 = 1;"),
                ("ADVTalkSystem", "globalThis.__ran3 = 1;"),
            ],
        );
        assert_eq!(report.loaded_count(), 2, "启用的两条都该起：{report:?}");
        assert_eq!(report.attempted, vec!["YEP_CoreEngine", "ADVTalkSystem"]);
        assert!(h.eval_bool("__ran1 === 1 && __ran3 === 1"));
        assert!(
            h.eval_bool("typeof __ran2 === 'undefined'"),
            "禁用的插件被跑了 —— 与上游 `PluginManager.setup` 不一致"
        );
    }

    #[test]
    fn a_plugin_that_throws_is_counted_with_its_name_not_hidden() {
        let h = Host::boot();
        let r = Registry::parse(PLUGINS_JS).expect("解析");
        let report = load(
            &h,
            &r,
            &[
                ("YEP_CoreEngine", "Game_Talk.prototype.initialize();"),
                ("ADVTalkSystem", "globalThis.__ok = 1;"),
            ],
        );
        assert_eq!(report.loaded, vec!["ADVTalkSystem"]);
        assert_eq!(report.failed.len(), 1, "失败必须记账：{report:?}");
        let (name, err) = &report.failed[0];
        assert_eq!(name, "YEP_CoreEngine", "账单没带插件名：{name}");
        assert!(
            err.contains("Game_Talk"),
            "异常文本没带引擎给的原因，查无可查：{err}"
        );
        // 一条挂了不许把后面的带走（上游是每插件一个 `<script>`）。
        assert!(h.eval_bool("__ok === 1"));
    }

    #[test]
    fn a_plugin_that_aborts_with_a_console_error_is_still_named() {
        // "起来了"与"起来了但自己喊完就 return"是两个数 —— 账单必须把后一坨单独报出来。
        // 真数据第一例：`FTKR_SkillTreeSystem:1022-1026`（缺 `reserveFaceImages` 就 console.error + return）。
        let h = Host::boot();
        crate::platform::install_console(&h).expect("记账 console 装不上");
        let r = Registry::parse(PLUGINS_JS).expect("解析");
        let report = load(
            &h,
            &r,
            &[
                (
                    "YEP_CoreEngine",
                    "(function () { console.error('コアスクリプトが古い'); return; })();",
                ),
                ("ADVTalkSystem", "globalThis.__ok = 1;"),
            ],
        );
        // 两条都 eval 过了顶层 ⇒ 都进 loaded；但 YEP 那条喊过话。
        assert_eq!(report.loaded, vec!["YEP_CoreEngine", "ADVTalkSystem"]);
        assert_eq!(report.failed.len(), 0);
        assert_eq!(
            report.logged.len(),
            1,
            "喊过话的那一条必须进账，否则进度尺把它读成正常：{report:?}"
        );
        assert_eq!(report.logged[0].0, "YEP_CoreEngine");
        assert_eq!(
            report.logged[0].1,
            vec![("error".into(), "コアスクリプトが古い".into())]
        );
        assert!(h.eval_bool("__ok === 1"), "上一条喊过话不许影响下一条");
    }

    #[test]
    fn a_missing_file_is_reported_not_a_silent_hole() {
        let h = Host::boot();
        let r = Registry::parse(PLUGINS_JS).expect("解析");
        let report = load(&h, &r, &[("YEP_CoreEngine", "globalThis.__ran1 = 1;")]);
        assert_eq!(report.missing_source, vec!["ADVTalkSystem"]);
        assert_eq!(
            report.summary(),
            "插件装载：起 1（其中自己喊过话 0）／试 1／失败 0／缺源码 1",
            "账单读数形状要稳定，进 PLAN 与 HUD 都读它"
        );
    }

    #[test]
    fn a_file_that_is_not_the_mv_shape_is_an_error_not_a_zero() {
        assert_eq!(
            Registry::parse("var $plugins = 0;").unwrap_err(),
            RegistryError::NoPluginArray
        );
    }
}
