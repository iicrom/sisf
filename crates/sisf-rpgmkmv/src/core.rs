//! M10.4 的交付面：`rpg_core.js` 的**屏幕读数、报错面与缓存那一圈**。
//!
//! ## 为什么从这些格开
//!
//! 边界是量出来的，不是照类名挑的（尺子 `../sisf_pray/tools/probes/core-touch.py`，判词与读数
//! 在 `M10.md`「M10.4 边界测量」与「M10.4 验收」）。① **上游那一族有多大**：28 颗构造器 / 240 条方法，
//! 其中硬底（PIXI / canvas 2D / Web Audio / DOM）非零的有 11 颗 —— `Bitmap` 38 条里 **22 条有硬底、
//! 16 条没有**（那 16 条里又有几条落在硬底的下一格上，`--class Bitmap` 的 `▸` 那一列）。
//! ② **插件碰得最多的是渲染面读数**：`Graphics` 205 处（`boxWidth` 67 / `boxHeight` 60 / `width` 31 /
//! `height` 27 —— 全是 `YEP_CoreEngine` 那一族覆盖虚拟屏的读数）。③ **已经交出去的那 40 段 JS
//! 今天就够不着的只有 `Graphics` 的六格 10 处**：`Game_Map.screenTileX/Y`
//! （`objects_map_and_events.js:293,297`）与 `Game_CharacterBase` 的屏幕中心
//! （`objects_characters.js:264,1320,1324`）读 `Graphics.width/height`，`DataManager.checkError` 读
//! `Graphics.printLoadingError`，`command261` 读视频那一族。
//! ⇒ 这一族先交**读数、报错面与缓存**，渲染器/位图/音频留给同轮的后面几格。
//!
//! ## 缓存四颗为什么排在位图之前（第二格的边界，2026-10-04 量出来的）
//!
//! `CacheEntry`/`CacheMap`/`ImageCache`/`RequestQueue` 是 core 那一族里**唯一一整圈零硬底**的类
//! （24 条方法，`core-touch.py` ① 的那四行"无"），而 `ImageManager` 脚下那五格自创的底里，
//! 有四格（`_imageCache`/`_generateCacheKey`/`clear`/`update`…）其实**只欠这四颗**：
//! 它们的体里没有位图，只有 `Date.now()`、`Object.keys`、数组与 `item.bitmap.isReady()` 这种
//! **对"存进去的那个东西"的问话**。所以这一格把账拆开：
//!
//! - **交逐字**：四颗类（`:313-611`，24 条 + `ImageCache.limit` + 两颗构造体）与
//!   `ImageManager` 的**九格静态底**（`rpg_managers.js:793-797,799-801,900-902,1001-1003,1090-1096`）。
//! - **继续欠着**：`Bitmap` 的**光栅那半**（22 条有硬底 + `initialize`/`resize` 这些落在硬底下一格的）。
//!   需求面也是量出来的（`--demand`）：103 条插件里 `new Bitmap(` **35 处 / 15 条插件，顶格行 0 处**
//!   ⇒ 装载期不构造位图；覆写点 9 处里只有 `YEP_CoreEngine` 那 6 格是**原格的别名**；
//!   而兼容层**已交出的那 1,365 条对 `Bitmap.` 的引用是 0 处**。所以"位图接 Rust 还是登记成降级"
//!   是 `M10.md` 待定项 2 那一格，不是这一格能定的。
//! - **句柄的契约**：`ImageManager._sisfHandle` 交出来的那个对象必须答得上缓存四颗问的九格
//!   （`width`/`height`/`isReady`/`isError`/`isRequestOnly`/`isRequestReady`/`startRequest`/
//!   `addLoadListener`/`smooth`）—— 判据 `the_handle_answers_the_cache_contract` 钉这一格。
//!   尺寸两格恒 0 ⇒ `_truncateCache` 永远不裁（`ImageCache.limit` 那一格因此由判据点名钉住）。
//!
//! ## 三条口径
//!
//! 1. **文本逐字**：五块 `Object.defineProperty` + `_makeErrorHtml` + `printLoadingError` 全部逐字
//!    对 `rpg_core.js`（`objects-verbatim.py` 现在认第三处上游，并把访问器块算进对表）；
//!    缓存四颗同样逐字，**只有两颗构造体的声明头换了写法**（`function X(…) {` → `X = function (…) {`，
//!    因为 `js_sys::eval` 不把 `function` 声明漏成全局 —— 尺子比的是**体**，体一个字没动）。
//! 2. **`Graphics.initialize` 不交**（实测它的最后八句是 DOM/渲染器）⇒ 宿主写尺寸走**字段直写**，
//!    与上游 `initialize` 里那五行 `this._width = width || 800` 同一形状。写访问器会敲
//!    `Graphics._updateAllElements` —— **那一格没交**，判据把它点名钉住（同 `objects.rs` 里
//!    `the_battler_reaches_the_undelivered_managers_and_says_who` 那个手法）。
//! 3. **数据形状与函数分开数**：`_errorPrinter`/`_video`/`_videoVolume` 那五格右值不是函数 ⇒
//!    逐字尺子数不到，只有这里的判据与 `core-touch.py` 数得到（同 AGENTS 咬人第 13 条那条纪律）。

use crate::jsrt::Host;

/// `rpg_core.js` 的屏幕读数五块访问器 + 报错面两格 + 五格数据形状（逐字部分见文件内注释）。
pub const GRAPHICS_JS: &str = include_str!("js/core_graphics.js");

/// `rpg_core.js:313-611` 的**缓存四颗**：`CacheEntry`(5) + `CacheMap`(5) + `ImageCache`(9) +
/// `RequestQueue`(5) = **24 条 prototype 方法**，另有 `ImageCache.limit` 一格静态常量与
/// `CacheEntry`/`CacheMap` 两颗**构造体**（体的形状不是 `this.initialize.apply(…)`，所以不能沿用
/// [`crate::shapes`] 那颗壳 —— 沿用就是 `new CacheEntry(…)` 当场抛）。
///
/// 文本由 `../sisf_pray/tools/probes/objects-slice.py --file rpg_core.js --classes
/// CacheEntry,CacheMap,ImageCache,RequestQueue --ctor` 切出，`objects-verbatim.py` 逐字对表。
pub const CACHE_JS: &str = include_str!("js/core_cache.js");

/// 自创的底：**`PIXI` 那一族的底**（M10.5 全 shim 的第一格，2026-10-05）。
///
/// 它**不是上游文本**，所以逐字尺子不比它的内容，改由 `objects-verbatim.py` 新加的那一段
/// （`SHIM_BASE` 双向白名单）数它交出/自创了哪几颗。存在的理由量在文件头：`rpg_core.js` 十四颗
/// 渲染类里 **13 颗**的链基类写的是 `PIXI.…`（尺子 `tools/probes/pixi-face.py --chains`），
/// 交不出这一层，那十三颗一颗都装不上。
///
/// 这一格只有两颗（`PIXI.Point`/`PIXI.Rectangle`）—— 就是今天真流要的那两条链的脚下。
/// 剩下的（`PIXI.Container`/`PIXI.Sprite`/`PIXI.extras.PictureTilingSprite`/
/// `PIXI.filters.ColorMatrixFilter`）随各自的渲染类一起加，**一格的理由写在一格头上**：
/// 这一层的成员只由**量出来的**引用增加（`pixi-face.py` 那一族），不照 PIXI 的常识补。
pub const PIXI_SHIM_JS: &str = include_str!("js/shim_pixi.js");

/// `rpg_core.js:621-674` 的 `Point`/`Rectangle` 两颗：**2 条链 + 2 条 `constructor` 补句 +
/// 2 条 `prototype.initialize` + `Rectangle.emptyRectangle` 那一格数据形状**。
///
/// 文本由 `../sisf_pray/tools/probes/objects-slice.py --file rpg_core.js
/// --classes Point,Rectangle --allow-base PIXI.Point,PIXI.Rectangle` 切出，
/// `objects-verbatim.py` 逐字对表。`emptyRectangle` 的右值不是函数 ⇒ 那一把尺子数不到，
/// 由下面的判据数（同 `_imageCache` 那条"数据形状由判据数"的纪律）。
///
/// 构造体**沿用 [`crate::shapes`] 那颗壳**（体就是 `this.initialize.apply(this, arguments);`，
/// 切片器报"沿用壳的 2 颗"），所以 `--ctor` 不开。但那条链把 `Point.prototype` **整颗换掉**成
/// `Object.create(PIXI.Point.prototype)` —— 壳原来挂在那颗 prototype 上的东西就此没有了，
/// 换成上游那一颗 `initialize`，这正是我们要的：`new Point(3,4)` 之后 `x`/`y` 落得下来。
pub const POINT_AND_RECTANGLE_JS: &str = include_str!("js/core_point_and_rectangle.js");

/// 装上 `Point`/`Rectangle`，并把它们脚下那层 `PIXI` 的底一起装好。
///
/// **顺序在这一格里是内部前提，不是调用方的功课**（同
/// `managers_install_does_not_depend_on_the_caller_installing_core_first` 那条纪律）：
/// 上游文本第一句就读 `PIXI.Point.prototype`，先装它就是当场抛。
pub fn install_geometry(host: &Host) -> Result<(), String> {
    host.eval(PIXI_SHIM_JS)
        .map_err(|e| format!("`PIXI` 那一层底装不上：{e}"))?;
    host.eval(POINT_AND_RECTANGLE_JS)
        .map_err(|e| format!("`Point`/`Rectangle` 那两颗装不上：{e}"))
}

/// 装上缓存四颗。**幂等**：四颗都是"同一颗全局上重新 define"，第二次装不抛
/// （`reinstalling_the_cache_classes_keeps_the_entries` 钉这一格：重装之后已存的条目还在）。
///
/// 这一格由 [`install`] 装，也由 [`crate::managers::install`] 在交 `ImageManager` 的静态底之前自己
/// 装一次 —— 那九格里有 `new ImageCache()`，顺序错了就是 `ImageCache is not defined`，
/// 而让装载次序依赖调用方"记得先装 core"会把一格的内部前提变成整族的口径（判据
/// `managers_install_does_not_depend_on_the_caller_installing_core_first` 钉的就是这一格）。
pub fn install_cache(host: &Host) -> Result<(), String> {
    host.eval(CACHE_JS)
        .map_err(|e| format!("`ImageCache` 那一圈装不上：{e}"))
}

/// `rpg_core.js:8913-9146` 的 **`JsonEx`**：**11 条函数型静态成员 + 2 格静态常量**
/// （`maxDepth = 100` / `_id = 1`）。构造体**不交** —— `new JsonEx()` 那句 `throw` 是
/// [`crate::shapes`] 那颗静态类壳的活（M10.3 事实 2⑴ 改判之后壳就交这一句）。
///
/// 文本由 `objects-slice.py --file rpg_core.js --classes JsonEx` 切出，`objects-verbatim.py`
/// 逐字对表；那 2 格常量右值不是函数 ⇒ 尺子数不到，由下面的判据数（同 `_imageCache` 一条腿）。
///
/// **为什么这一格排在渲染面之前**：2026-10-05 把待定项 1 改判成"兼容上游 `rpgsave`"
/// （见本文「三条待定项」），而 `DataManager.saveGame` 那句 `JsonEx.encode(game)` 与
/// `JsonEx.decode` 的往返**就是**那一份存档的编解码器 —— 它比 `Bitmap` 便宜（11 条里 0 处硬底），
/// 而它不通的话，后面那一族（`LZString` / `StorageManager` / `Scene_File`）一块都动不了。
pub const JSON_EX_JS: &str = include_str!("js/core_jsonex.js");

/// 装上 `JsonEx`（要 [`crate::shapes`] 先把 `JsonEx` 那颗静态类壳挂上）。
pub fn install_json_ex(host: &Host) -> Result<(), String> {
    host.eval(JSON_EX_JS)
        .map_err(|e| format!("`JsonEx` 那 11 格装不上：{e}"))
}

/// 装上 core 的这一格：`Point`/`Rectangle`（踩着 `PIXI` 那层底）+ `JsonEx` + 缓存四颗 +
/// `Graphics` 的屏幕读数与报错面 + 音频三圈。
pub fn install(host: &Host) -> Result<(), String> {
    install_geometry(host)?;
    install_json_ex(host)?;
    install_cache(host)?;
    host.eval(GRAPHICS_JS)
        .map_err(|e| format!("`Graphics` 的屏幕那一格装不上：{e}"))?;
    install_audio(host)
}

/// `rpg_core.js:7670-8465` 的 **`WebAudio` 整圈**：29 条 prototype 方法 + 16 条函数型静态成员
/// （含跨类点名的 `Graphics.setVideoVolume`，`:2341-2346` —— 已交付的 `AudioManager.masterVolume`
/// setter 第二句就喊它）+ 4 块 `Object.defineProperty(WebAudio.prototype, …)` 的 `url/volume/pitch/pan`
/// 访问器 + 6 格静态数据形状。文本由
/// `../sisf_pray/tools/probes/objects-slice.py --file rpg_core.js --classes WebAudio
/// --extra Graphics.setVideoVolume` 切出，`objects-verbatim.py` 逐字对表。
///
/// **为什么整圈能交**（`core-touch.py --demand audio` 量的，判词在 `M10.md` 的 M10.4 第三格）：
/// 29 条里只有 2 条有真硬底（`_createNodes` 的 `context.createGain/createBufferSource/createPanner`
/// 与 `_onXhrLoad` 的 `context.decodeAudioData`），而那两条的**入口都被 `WebAudio._context` 挡住**：
/// `_load`（`:8163`）第一句 `if (WebAudio._context)`、`_startPlaying`/`play`/`seek`/`fadeIn`/`fadeOut`
/// 全走 `isReady()`（读 `_buffer`）。`_context` 只在 `_createContext`（`:7767`）里 new，而那一格
/// 认的是 `typeof AudioContext` —— native（QuickJS）没有这个全局 ⇒ **两条分支都不进、不抛**，
/// `_context` 留 `null`。于是这一圈在两条腿上是"全部喊得动、全部静默"，少的是**出声**，
/// 不是形状（`Html5Audio` 那 34 格更整颗不交：`AudioManager.shouldUseHtml5Audio` 的体在
/// `rpg_managers.js:1495` 上游自己注释成恒 `return false`，而交付文本里对它的 4 处引用
/// —— `setup` 2 + `setStaticSe` 1 + `return Html5Audio` 1 —— **全在那道恒 false 的守卫里面**）。
pub const AUDIO_JS: &str = include_str!("js/core_audio.js");

/// `WebAudio` 那一圈里**只有两格落不进"静默"**：它们的体第一步就碰 `document`。
///
/// 交的是"什么都不做"的底（**自创**，`objects-verbatim.py` 的 `HOME_MADE` 双向白名单数得到这两格），
/// 每一格替换的是上游哪一行、少了什么，都在下面注释里：
/// 1. `WebAudio._detectCodecs`（`:7782-7789`，`document.createElement('audio')` + `canPlayType`）
///    —— 不探测 ⇒ `_canPlayOgg`/`_canPlayM4a` 留在 `false` ⇒ `AudioManager.audioFileExt()`
///    （`rpg_managers.js:1487`）答 `.m4a`，那正是上游在"浏览器不支持 ogg"时的同一答。
///    这条 URL 今天没有谁来请求：`_load` 被 `WebAudio._context` 挡在门外。
/// 2. `WebAudio._setupEventHandlers`（`:7820-7827`，`document.addEventListener` 两支）
///    —— 少的是 touchstart 解锁与 visibilitychange 暂停，两者都在**真出声**之后才有意义。
///
/// 平台件**故意不造假 `document`**（`platform.rs` 模块头第③条：装了假的，插件反而读不出真平台），
/// 所以这两格不许"顺手补一个假 document"来让它们跑起来。接宿主音频（本文待定项 2）那一拍，
/// 这两格换回上游文本还是接到 Rust 的输出面，由数字判 ⇒ 见 `M10.md` 的 M10.4 第三格事实。
pub const WEB_AUDIO_DOM_JS: &str = r#"
// 自创的底（两格）：替换的是上游碰 `document` 的那两格，见上面的文档注释。
WebAudio._detectCodecs = function() {};
WebAudio._setupEventHandlers = function() {};
"#;

/// 装上 `WebAudio` 那一圈，然后压上两格 DOM 底（顺序是这一格的内部前提：底必须在文本之后）。
///
/// 幂等：整圈都是"在同一颗全局上重新 define"，第二次装不抛（判据
/// `reinstalling_the_audio_round_is_still_the_core_layer` 钉这一格）。
pub fn install_audio(host: &Host) -> Result<(), String> {
    host.eval(AUDIO_JS)
        .map_err(|e| format!("`WebAudio` 那一圈装不上：{e}"))?;
    host.eval(WEB_AUDIO_DOM_JS)
        .map_err(|e| format!("`WebAudio` 的两格 DOM 底装不上：{e}"))
}

/// 宿主写虚拟屏那一拍的**上游形状**：直写四个字段 + `_scale`，不经过 setter
/// （上游 `Graphics.initialize`（`:1734-1739`）就是这五行；走 setter 会敲没交的 `_updateAllElements`）。
///
/// `boxWidth/boxHeight` 的落点同样是 `initialize`（`this._boxWidth = this._width`），而
/// `SceneManager.initGraphics`（`rpg_managers.js:1825-1826`）随后用 `Graphics.boxWidth = …` 覆盖它 ——
/// 那一格属于 M10.5（场景管理器），所以这里只提供 `initialize` 那一条腿。
///
/// `{w}`/`{h}` 由 [`set_screen_size`] 用**十进制整数**替进去 —— 不借 JS 全局变量：`js_sys::eval`
/// 不像 QuickJS 那样把 `var __w = …` 漏成全局（同一课 `M10.md` 的 c 事实 41），而这一段的两个数
/// 是 Rust 侧的 `i32`，没有拼接外部文本那条路。
pub const GRAPHICS_SET_SCREEN_JS: &str = r#"
Graphics._width  = {w};
Graphics._height = {h};
Graphics._boxWidth  = {w};
Graphics._boxHeight = {h};
Graphics._scale  = 1;
Graphics._realScale = 1;
"#;

/// 把虚拟屏按上游那五行写进 `Graphics`（`core-touch.py` ③ 那六格里前两格的宿主侧落点）。
pub fn set_screen_size(host: &Host, width: i32, height: i32) -> Result<(), String> {
    let text = GRAPHICS_SET_SCREEN_JS
        .replace("{w}", &width.to_string())
        .replace("{h}", &height.to_string());
    host.eval(&text)
        .map_err(|e| format!("屏幕尺寸写不进 `Graphics`：{e}"))
}

#[cfg(test)]
mod tests {
    //! fixture 判据：两条腿各跑一遍（同一份文本、同一组断言）。真数据那一格（虚拟屏的数从
    //! `YEP_CoreEngine` 的参数落到 `Game_Map.screenTileX`）住在 `pray/src/compat.rs`。
    use super::{
        install, install_audio, install_cache, install_geometry, install_json_ex, set_screen_size,
    };
    use crate::jsrt::Host;
    use crate::shapes::{DeclaredShape, ShapeKind, install as install_shapes};

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    /// 缓存四颗的**形状**（`shapes::scan` 从 `rpg_core.js` 扫出来就是 `Class`，这里照同一档声明）：
    /// `CacheEntry`/`CacheMap` 的构造体由 [`super::CACHE_JS`] 覆盖，`ImageCache`/`RequestQueue`
    /// 沿用壳（它们的体就是 `this.initialize.apply(this, arguments);`）。
    const CACHE_SHELLS: &[&str] = &["CacheEntry", "CacheMap", "ImageCache", "RequestQueue"];

    fn shell(names: &[(&str, ShapeKind)]) -> Host {
        let host = Host::boot();
        let shapes: Vec<DeclaredShape> = names
            .iter()
            .map(|(n, k)| DeclaredShape {
                name: n.to_string(),
                kind: *k,
            })
            .collect();
        install_shapes(&host, &shapes).expect("形状装不上");
        host
    }

    /// 音频那一圈要的两颗形状：`WebAudio` 的构造体就是 `initialize` 派分（沿用壳），
    /// `AudioManager` 的音量访问器要挂在静态类上（本格只借它验 `Graphics.setVideoVolume` 那一格）。
    /// `ResourceHandler` 也在这一档，理由见 `booted`。
    const AUDIO_SHELLS: &[(&str, ShapeKind)] = &[
        ("WebAudio", ShapeKind::Class),
        ("AudioManager", ShapeKind::StaticClass),
        ("SoundManager", ShapeKind::StaticClass),
        ("ResourceHandler", ShapeKind::StaticClass),
    ];

    /// 只装 core 这一格要的那几颗形状（`Graphics` + `Point`/`Rectangle` + 缓存四颗 + 音频那一族）
    /// ⇒ 断言的是这一格自己，不借别族的东风。
    ///
    /// **但音频那一圈之前要先摆上 `ResourceHandler.createLoader` 那一半**，这一手不是为了
    /// 凑齐依赖，是为了让 `_standAlone` 这一格**与 realm 的历史无关**：
    /// `WebAudio._standAlone = (function(top){ return !top.ResourceHandler; })(this)` 是在
    /// eval 那一刻读全局的，而 **wasm 腿一个进程共享一个 realm** —— `managers.rs` 那几条先跑过，
    /// 全局里就多了 `ResourceHandler`（native 腿每测试一个新 realm，永远没有）。于是同一句
    /// `new WebAudio(url)` 在两条腿上会走进不同分支：`_standAlone` 假 ⇒ `initialize` 第二句就喊
    /// `ResourceHandler.createLoader`，而共享 realm 里那一颗只有平台件的**状态那一半**、
    /// 没有 `createLoader` ⇒ `TypeError`。这一档读的是**浏览器里的形状**（上游整文件靠函数声明
    /// 提升拿到 `ResourceHandler` ⇒ 真机上 `_standAlone` 恒 false 且 `createLoader` 在场），
    /// `managers.rs` 的 `managers_install_installs_the_audio_round_itself` 钉另一头。
    fn booted() -> Host {
        let mut names: Vec<(&str, ShapeKind)> = vec![
            ("Graphics", ShapeKind::StaticClass),
            ("JsonEx", ShapeKind::StaticClass),
        ];
        names.extend(GEOMETRY_SHELLS.iter().cloned());
        names.extend(CACHE_SHELLS.iter().map(|n| (*n, ShapeKind::Class)));
        names.extend(AUDIO_SHELLS.iter().cloned());
        let host = shell(&names);
        host.eval(crate::managers::RESOURCE_HANDLER_LOADER_JS)
            .expect("ResourceHandler 的 loader 那一半装不上");
        install(&host).expect("core 的这一格装不上");
        host
    }

    /// `Point`/`Rectangle` 两颗的形状（`shapes::scan` 从 `rpg_core.js` 扫出来是 `Class`，
    /// 构造体沿用壳 ⇒ `--ctor` 不开，见 [`super::POINT_AND_RECTANGLE_JS`] 那段注释）。
    const GEOMETRY_SHELLS: &[(&str, ShapeKind)] =
        &[("Point", ShapeKind::Class), ("Rectangle", ShapeKind::Class)];

    /// 只装那两颗（不碰缓存/`Graphics`/音频）—— 判据各自最小化，同 `cache_booted` 的手法。
    fn geometry_booted() -> Host {
        let names: Vec<(&str, ShapeKind)> = GEOMETRY_SHELLS.to_vec();
        let host = shell(&names);
        install_geometry(&host).expect("`Point`/`Rectangle` 那两颗装不上");
        host
    }

    /// **那两条链真的落在 `PIXI` 那层底上**：`new Point(3,4)` 之后 `x`/`y` 落得下来、
    /// `instanceof PIXI.Point` 成立、`constructor` 补句指回自己；`Rectangle` 四格同理，
    /// 再加上 `Rectangle.emptyRectangle` 那一格**数据形状**（逐字尺子只认 `= function`，
    /// 数不到它 ⇒ 这一格的账由判据数，同 `_imageCache` 那条纪律）。
    ///
    /// 反面对照也一起钉：**只有壳没有交付文本时 `new Point(3,4)` 当场抛**
    /// （`M10.md` 咬人第 2 条⑴ 改判之后占位构造器就是这一格形状 —— 它不悄悄交出一个没字段的对象，
    /// 而是敲在 `this.initialize.apply` 上）。钉的是"**抛不抛**"这件状态，不是句子：
    /// 两条腿的异常文本不一样（QuickJS 只说 "not a function"，node 带上属性名）。
    #[test]
    fn the_point_and_rectangle_chains_land_on_the_shim_base() {
        let shell_only = {
            let names: Vec<(&str, ShapeKind)> = GEOMETRY_SHELLS.to_vec();
            let host = shell(&names);
            host.try_eval_string("String(new Point(3, 4).x)").is_err()
        };
        assert!(
            shell_only,
            "只有 `shapes` 那颗壳时 `new Point(3,4)` 必须**抛**（壳的体是 \
             `this.initialize.apply(this, arguments)`，而交付文本之前 prototype 上没有 `initialize`）—— \
             这是这一格的反面对照；哪天它不抛了，就是壳自己开始写字段，那条腿要重看"
        );

        let host = geometry_booted();
        let js = r#"
var out = [];
var p = new Point(3, 4);
out.push(String(p.x) + ',' + p.y);
out.push(String(p instanceof PIXI.Point));
out.push(String(Point.prototype.constructor === Point));
var d = new Point();
out.push(String(d.x) + ',' + d.y);
var r = new Rectangle(1, 2, 3, 4);
out.push(String(r.x) + ',' + r.y + ',' + r.width + ',' + r.height);
out.push(String(r instanceof PIXI.Rectangle));
var e = Rectangle.emptyRectangle;
out.push(String(e instanceof Rectangle));
out.push(String(e.x) + ',' + e.y + ',' + e.width + ',' + e.height);
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "3,4|true|true|0,0|1,2,3,4|true|true|0,0,0,0",
            "两条链落不进那层底（`x`/`y`/`width`/`height` 读不到）、或 `emptyRectangle` 那一格没建出来"
        );
    }

    /// **手交的每一段都要能装第二次**（`M10.md`「这一族的规矩」第 4 条）：wasm 腿一整个进程
    /// 共享一个 realm，`install_geometry` 会被后来的判据再装一遍。这一格两条腿都数得到，
    /// 而只有 wasm 腿能真正咬到它（native 每条判据一个新 realm）。
    #[test]
    fn the_pixi_shim_base_installs_twice_in_one_realm() {
        let names: Vec<(&str, ShapeKind)> = GEOMETRY_SHELLS.to_vec();
        let host = shell(&names);
        install_geometry(&host).expect("第一次装不上");
        install_geometry(&host).expect("第二次装不上（共享 realm 里重assign 不许抛）");
        assert_eq!(
            num(&host, "String(new Point(5, 6).x) + ',' + new Point(5, 6).y"),
            "5,6",
            "重装之后那两颗还要能用"
        );
    }

    /// `JsonEx` 的**往返**：类名、字段、循环引用三样都要回来，而那份 JSON 里的元数据键
    /// 必须就是上游那四种拼写（`@c` 每条一个 id / `@` 类名 / `@r` 指回同一个对象 / `@a` 数组包装，
    /// `rpg_core.js:9026-9066` 的 `_encode` 与 `:9077-9113` 的 `_decode`）。
    ///
    /// 这一格是 2026-10-05 改判"存档兼容上游 `rpgsave`"的第一块：`DataManager.saveGame` 的
    /// `JsonEx.encode(game)` / `decode` 就是那一份文件的编解码器，**它不通后面那一族一块都动不了**。
    /// 两条腿都要绿 ⇒ 类用 `globalThis.X = function…` 而不是 `function X(){}` 声明
    /// （`js_sys::eval` 不把声明漏成全局，`M10.0` 事实 2）。
    #[test]
    fn json_ex_round_trips_the_class_graph_with_its_circular_references() {
        let names: Vec<(&str, ShapeKind)> = vec![
            ("Graphics", ShapeKind::StaticClass),
            ("JsonEx", ShapeKind::StaticClass),
            // 样品就用**这一族的真实形状**：`shapes` 挂的那颗壳 + JS 侧往 `prototype` 上挂的
            // `initialize`。这样这一条判据同时钉住"壳的名字在场"（`JsonEx` 找回 prototype 靠它）。
            ("Game_Sample", ShapeKind::Class),
        ];
        let host = shell(&names);
        // **跨族的一条真实前提**（不是测试凑数）：`JsonEx._decode` 找回 prototype 走的是
        // `window[value['@']]`（`rpg_core.js:9083`），而 native 腿的 `window` 来自平台件那一句
        // `globalThis.window = globalThis`（浏览器里两者本来就是同一颗）。少了它，
        // 每一次读档都在这一行抛 "window is not defined" —— 这一格就是这么量出来的。
        host.eval(crate::platform::HOST_GLOBALS_JS)
            .expect("平台件的全局那一格摆不下");
        install_json_ex(&host).expect("`JsonEx` 装不上");
        let js = r#"
Game_Sample.prototype.initialize = function (n) { this.value = n; };
var s = new Game_Sample(7);
s.other = new Game_Sample(9);
s.back = s;
var text = JsonEx.stringify(s);
var t = JsonEx.parse(text);
var out = [];
out.push(String(Game_Sample.name));
out.push(t.constructor.name);
out.push(String(t.value) + ',' + t.other.value);
out.push(String(t.back === t));
out.push(String(text.indexOf('"@":') >= 0) + ',' + String(text.indexOf('"@r":') >= 0));
out.push(String(text.indexOf('"@c":') >= 0));
out.push(String(JsonEx.maxDepth) + ',' + (typeof JsonEx._id));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "Game_Sample|Game_Sample|7,9|true|true,true|true|100,number",
            "往返丢了类名 / 循环引用没有指回同一个对象 / 元数据键的拼写不是上游那四种 —— \
             每一种都是 `rpgsave` 读不回来的表征。头两格（`Game_Sample.name` 与 \
             `t.constructor.name`）钉的是 [`crate::shapes::DeclaredShape::install_line`] 那一格\
             换掉匿名构造体的那一手：`@` 那一键读的就是它。"
        );
    }

    /// 只装缓存四颗（不碰 `Graphics`）—— 缓存那几条判据各自最小化。
    fn cache_booted() -> Host {
        let names: Vec<(&str, ShapeKind)> = CACHE_SHELLS
            .iter()
            .map(|n| (*n, ShapeKind::Class))
            .collect();
        let host = shell(&names);
        install_cache(&host).expect("缓存四颗装不上");
        host
    }

    fn num(host: &Host, expr: &str) -> String {
        host.eval_string(expr)
    }

    /// 缓存那一圈问"存进去的那个东西"的**契约**：`_truncateCache` 读 `width*height`，
    /// `_mustBeHeld`/`isReady`/`getErrorBitmap` 读 `isRequestOnly()`/`isReady()`/`isError()`，
    /// `RequestQueue.update` 读 `isRequestReady()`/`startRequest()`。
    /// 判据的样品照这一份契约造 ⇒ 数的是**缓存自己**的行为，与宿主的句柄无关
    /// （句柄那一格由 `managers.rs` 的 `the_handle_answers_the_cache_contract` 钉）。
    const ITEM: &str = r#"
globalThis.__started = [];
globalThis.__item = function (url, size, ready, requestOnly, requestReady) {
    return {
        url: url, width: size, height: size, smooth: false,
        isReady: function() { return ready; },
        isError: function() { return false; },
        isRequestOnly: function() { return requestOnly; },
        isRequestReady: function() { return requestReady !== false; },
        startRequest: function() { __started.push(this.url); },
        addLoadListener: function(f) { f(this); }
    };
};
"#;

    /// `ImageCache` 的四句基本账：`add`/`get`（命中同一个对象、`get` 顺手 `touch`）、
    /// 没命中交 `null`（不是 `undefined`）、`isReady()` 只看**不是 request-only 又没就绪**的那些。
    #[test]
    fn the_image_cache_round_trips_and_reports_readiness() {
        let host = cache_booted();
        host.eval(ITEM).expect("样品句柄摆不下");
        let js = r#"
var out = [];
var ic = new ImageCache();
var a = __item('a.png', 100, true, false);
ic.add('a', a);
out.push(String(ic.get('a') === a));
out.push(String(ic.get('nope')));
out.push(String(ic.isReady()));
ic.add('b', __item('b.png', 100, false, false));
out.push(String(ic.isReady()));
var c = new ImageCache();
c.add('r', __item('r.png', 100, false, true));
out.push(String(c.isReady()));
out.push(String(ic._items['a'].key));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "true|null|true|false|true|a",
            "`requestOnly` 的那一条必须被 `isReady()` 跳过（上游 `:549-551` 的 `!isRequestOnly() && !isReady()`），\
             否则 `command282` 的等待腿会永远等不下去"
        );
    }

    /// 预留那一圈的账：`reserve` 落 `reservationId`、`releaseReservation` 按 id **抹掉**（不是删条目），
    /// 而 `_mustBeHeld` 因此把预留过的条目当成"必须留着"。
    #[test]
    fn the_reservation_leg_marks_and_clears_the_item_flag() {
        let host = cache_booted();
        host.eval(ITEM).expect("样品句柄摆不下");
        let js = r#"
var out = [];
var ic = new ImageCache();
var a = __item('a.png', 100, true, false);
ic.add('a', a);
ic.reserve('a', a, 7);
out.push(String(ic._items['a'].reservationId));
out.push(String(ic._mustBeHeld(ic._items['a'])));
ic.releaseReservation(7);
out.push(String(ic._items['a'].reservationId === undefined));
out.push(String(ic._mustBeHeld(ic._items['a'])));
out.push(String(Object.keys(ic._items).length));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "7|true|true|false|1",
            "`releaseReservation` 抹的是旗标不是条目（上游 `:506-516` 的 `delete item.reservationId`）"
        );
    }

    /// **裁不裁由尺寸决定**，而宿主的句柄没有尺寸（恒 0）⇒ 今天一条都不会被裁。
    /// 三档都钉：零尺寸不裁（`limit` 减不动）、单条超界只留得下**第一条**、被预留的那条**永远留着**
    /// （`_mustBeHeld` 的 `reservationId` 那一支）。
    ///
    /// **这一段把 `Date.now()` 冻住**（样品里三颗条目落在同一个 `touch` 上）：上游
    /// `_truncateCache`（`rpg_core.js:518-534`）的排序键就是 `touch`，而第一句是
    /// `if (sizeLeft > 0 || this._mustBeHeld(item))` —— **第一条无条件留下**（哪怕它一条就超界）。
    /// 于是"留下的是哪一条"完全由毫秒决定：本轮实测同一份判据在 `1,true` 与 `2,true` 之间飘
    /// （同一毫秒 ⇒ 插入序 ⇒ 只有 `rsv` 留；跨了毫秒 ⇒ 最新那条排第一 ⇒ `n1` 与 `rsv` 都留）。
    /// 第二格的账原来写的是"不钉名字，因为钉名字就是拿时钟分辨率当判据"—— 那一半是对的，
    /// 但**留下的条数**同样随时钟飘 ⇒ 正确的落法是把时钟交回判据手里，然后连身份一起钉。
    #[test]
    fn truncation_fires_only_once_the_items_have_dimensions() {
        let host = cache_booted();
        host.eval(ITEM).expect("样品句柄摆不下");
        let js = r#"
var out = [];
var real = Date.now;
Date.now = function () { return 1000; };
out.push(String(ImageCache.limit));
var a = new ImageCache();
for (var i = 0; i < 3; i++) a.add('k' + i, __item('k' + i, 0, true, false));
out.push(String(Object.keys(a._items).length));
var b = new ImageCache();
b.add('big', __item('big', 4000, true, false));
b.add('big2', __item('big2', 4000, true, false));
b.add('big3', __item('big3', 4000, true, false));
out.push(Object.keys(b._items).join(','));
out.push(String(b.getErrorBitmap()));
var c = new ImageCache();
c.reserve('rsv', __item('rsv', 4000, true, false), 7);
c.add('n1', __item('n1', 4000, true, false));
c.add('n2', __item('n2', 4000, true, false));
out.push(Object.keys(c._items).join(',') + ',' + String(c._items['rsv'].reservationId));
Date.now = real;
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "10000000|3|big|null|rsv,7",
            "`ImageCache.limit` 是 10·1000·1000（`:468`）：4000×4000 = 1600 万像素一条就超界 ⇒ 三条只剩\
             排第一的那条（`big`）；`reserve` 过的那条被 `_mustBeHeld` 保住（`command282` 那一族的底）。\
             尺寸恒 0 时永远不裁 —— 这一格是位图那半欠账的读数（`M10.md` 待定项 2）"
        );
    }

    /// `CacheMap`/`CacheEntry` 交出**上游的形状连同它那一族死代码**（不改良）：
    /// ⑴ TTL 的判据是 `touch + ttl < now` ⇒ **刚设下 TTL 的条目立刻算死**；
    /// ⑵ `checkTTL` 对死条目走 `temp.push(...)`，而 `_lastRemovedEntries` 的初值是 `{}`（不是 `[]`）
    /// ⇒ 真有一条死的就抛。两处合起来 = 这一族上游就没人喊：`CacheMap.update` 在六颗核心文件里的
    /// 调用位点是 **0**（`ImageManager.cache` 只有 `rpg_managers.js:793` 那一句构造，
    /// 而 `Bitmap.cacheEntry` 恒 `null` —— `:847` 之后无人写）。
    /// ⇒ 兼容层**照逐字交**并钉住这两格，等 M10.5 之后是否要修由账目决定，不由切片顺手改。
    #[test]
    fn the_cache_map_ttl_leg_is_delivered_as_upstreams_broken_shape() {
        let host = cache_booted();
        let js = r#"
var out = [];
var map = new CacheMap({});
var e = map.setItem('x', 41);
out.push(String(Object.keys(map._inner).length));
out.push(String(map.getItem('x')));
out.push(String(map._inner['x'] === e));
out.push(String(map.getItem('zz')));
out.push(String(e.isStillAlive()));
e.setTimeToLive(5, 5);
out.push(String(e.isStillAlive()));
e.setTimeToLive(0, 0);
out.push(String(e.isStillAlive()));
e.free();
out.push(String(Object.keys(map._inner).length) + ',' + String(e.cached) + ',' + String(e.freedByTTL));
var e2 = map.setItem('y', 1);
e2.free(true);
out.push(String(e2.freedByTTL));
e2.touch();
out.push(String(Object.keys(map._inner).length));
map.clear();
out.push(String(Object.keys(map._inner).length));
var threw = 'no';
var m3 = new CacheMap({});
var d = m3.setItem('d', 1);
d.setTimeToLive(5, 5);
try { m3.update(1, 200); } catch (err) { threw = 'yes'; }
out.push(threw);
out.push(String(typeof m3._lastRemovedEntries));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "1|41|true|null|true|false|true|0,false,false|true|1|1|yes|object",
            "`free()` 不带参 ⇒ `freedByTTL` 是 false；`touch()` 在 `freedByTTL` 那一支把条目挂回 `_inner` \
             但**不置 `cached`** ⇒ 随后 `clear()` 摘不掉它（第 11 格读数是 1 不是 0 —— 同一族死代码的第三格）；\
             TTL 一旦设了就立刻算死、`checkTTL` 遇死条目就抛（`_lastRemovedEntries` 的初值是空对象）—— \
             这三格都是**上游的**，逐字交出来就不许在这里悄悄修"
        );
    }

    /// `RequestQueue` 的账：**一次只让一条在飞**。队首还没 `isRequestReady()` 就反复对它
    /// `startRequest()`（不 shift、不碰第二条）；队首到齐才 shift 并立刻让新队首起跑。
    /// `raisePriority` 把键挪到队首，找不到键不动任何东西（不抛）。
    #[test]
    fn the_request_queue_keeps_one_in_flight_and_raises_priority() {
        let host = cache_booted();
        host.eval(ITEM).expect("样品句柄摆不下");
        let js = r#"
var out = [];
var q = new RequestQueue();
q.enqueue('p', __item('p', 1, true, false));
q.enqueue('r', __item('r', 1, true, false));
out.push(String(q._queue.length));
q.raisePriority('r');
out.push(q._queue[0].key);
q.raisePriority('nope');
out.push(q._queue[0].key);
q.update();
out.push(String(q._queue.length) + ',' + __started.join(','));
q.update();
out.push(String(q._queue.length) + ',' + __started.join(','));
q.clear();
out.push(String(q._queue.length));
__started.length = 0;
var q2 = new RequestQueue();
q2.enqueue('s1', __item('s1', 1, false, true, false));
q2.enqueue('s2', __item('s2', 1, false, true, false));
q2.update();
out.push(String(q2._queue.length) + ',' + __started.join(','));
q2.update();
out.push(String(q2._queue.length) + ',' + __started.join(','));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "2|r|r|1,p|0,p|0|2,s1|2,s1,s1",
            "样品说\"永远就绪\"时，第一拍就把队首放掉、只让**新**队首起跑（`p` 那一次是 `:591` 的那一句），\
             而说\"还没就绪\"时就地对队首反复 `startRequest`（`s1,s1`）—— 兼容层今天不泵这一格 \
             （`ImageManager.update` 的泵属 M10.5），但账必须是对上游的形状"
        );
    }

    /// 双装：四颗类重 define 不抛，而且**已经存进去的条目还在**（`_items` 挂在实例上，
    /// 类的重定义不动它）—— 与 `ImageManager._imageCache` 被重灌时换成一颗新的那条账分开，
    /// 那一条由 `managers.rs` 的 `reinstalling_managers_resets_the_image_cache` 钉。
    #[test]
    fn reinstalling_the_cache_classes_keeps_the_entries() {
        let host = cache_booted();
        host.eval(ITEM).expect("样品句柄摆不下");
        host.eval(
            "globalThis.__keep = new ImageCache(); __keep.add('a', __item('a', 1, true, false));",
        )
        .expect("样品缓存摆不下");
        install_cache(&host).expect("缓存四颗第二次装不上（不幂等）");
        assert_eq!(
            num(&host, "String(__keep.get('a') !== null)"),
            "true",
            "重装只换类形状，不该动已存在的实例"
        );
    }

    /// 五块访问器读的是**字段**，而宿主直写字段就是上游 `initialize` 的那五行。
    /// `boxWidth`/`boxHeight` 是插件侧碰得最多的两格（`core-touch.py` ② 量的 67/60 处）。
    #[test]
    fn the_five_screen_accessors_answer_the_fields_the_host_writes() {
        let host = booted();
        set_screen_size(&host, 1104, 736).expect("屏幕尺寸写不进");
        assert_eq!(
            num(
                &host,
                "[Graphics.width, Graphics.height, Graphics.boxWidth, Graphics.boxHeight, \
                 Graphics.scale].join(',')"
            ),
            "1104,736,1104,736,1",
            "五块访问器与上游那五格字段必须一一对上（写一路经 setter 会敲 `_updateAllElements`，\
             所以宿主走 `initialize` 的直写形状）"
        );
    }

    /// **写 `Graphics.width` 打到哪一格**：上游那一支 `this._updateAllElements()` 属于渲染器，
    /// 本轮不交 ⇒ 判据钉"它确实没交、而且报的是这个名字"，而不是让它静悄悄变成"能写"。
    #[test]
    fn writing_the_screen_accessor_reaches_the_undelivered_renderer_leg() {
        let host = booted();
        assert_eq!(
            num(&host, "typeof Graphics._updateAllElements"),
            "undefined",
            "渲染面那一格今天不该在场（在的话本判据要改成断它干了什么）"
        );
        let err = host
            .eval("Graphics.width = 640;")
            .expect_err("写访问器应当打到没交的 `_updateAllElements`");
        assert!(!err.is_empty(), "抛了但句子是空的：{err}");
        // 反面（同一条腿的另一半）：`boxWidth` 的 setter 上游就只写字段 ⇒ 写得动，不敲渲染面。
        host.eval("Graphics.boxWidth = 816;")
            .expect("`boxWidth` 的 setter 不该敲渲染面（上游体里只有一格赋值）");
        assert_eq!(num(&host, "String(Graphics.boxWidth)"), "816");
        assert_eq!(
            num(&host, "String(Graphics.width)"),
            "640",
            "上游那一支是**先写字段、再敲 `_updateAllElements`**（`rpg_core.js:2247-2251`）⇒ \
             字段落上了，抛的是渲染面那一格。这一格差就是要照上游钉住：写了、但没重排"
        );
    }

    /// 报错面：`_errorPrinter` 不在场时 `printLoadingError` **整句短路**（上游的守卫），
    /// 于是它既不抛也不点亮 `_errorShowed` —— 这正是 `DataManager.checkError` 那条真路径要的。
    #[test]
    fn the_loading_error_leg_is_a_guarded_no_op_without_an_error_printer() {
        let host = booted();
        assert_eq!(
            num(
                &host,
                "[typeof Graphics._errorPrinter, String(Graphics._errorShowed), \
                 String(Graphics._video), String(Graphics._videoLoading), \
                 Graphics._videoVolume].join(',')"
            ),
            "object,false,null,false,1",
            "五格数据形状必须在场（右值不是函数 ⇒ 逐字尺子数不到它们，只有这一条数得到）。\
             `typeof null` 是 'object'，那一格是上游 `initialize` 的初值"
        );
        host.eval("Graphics.printLoadingError('data/Map999.json');")
            .expect("报错面在没有错误窗时不该抛");
        assert_eq!(num(&host, "String(Graphics._errorShowed)"), "false");
        assert_eq!(
            num(
                &host,
                "String(Graphics._makeErrorHtml('Loading Error', 'Failed to load: x').length)"
            ),
            "100",
            "`_makeErrorHtml` 逐字在上游：两支 `<font>` 模板固定 **70** 个字符 + `Loading Error`(13) \
             + `Failed to load: x`(17) ⇒ 100（钉长度，不钉句子，免得被引号种类折腾）"
        );
    }

    /// 视频那一族**故意不交**（磁盘 `261`/`262` 各 **0** 条、`movies/` 目录不存在 ⇒ 零影响面），
    /// 而 `command261` 已经随 d 交出去 ⇒ 报出洞名的正好是 `Graphics.playVideo` 这一格。
    #[test]
    fn the_video_leg_is_undelivered_and_the_interpreter_names_it() {
        let host = booted();
        assert_eq!(num(&host, "typeof Graphics.playVideo"), "undefined");
        assert_eq!(num(&host, "typeof Graphics.isVideoPlaying"), "undefined");
        assert_eq!(num(&host, "typeof Graphics.canPlayVideoType"), "undefined");
        // `_video` 是 null ⇒ 上游那句 `this._video && this._video.canPlayType(type)` 即便交了也只是
        // falsy（不抛）。这条断言钉的是"为什么可以不交"，不是"想不交"。
        assert_eq!(num(&host, "String(Graphics._video)"), "null");
    }

    /// 双装：五块访问器带 `configurable: true` ⇒ 第二次装是重新定义，读数不丢。
    #[test]
    fn reinstalling_the_graphics_leg_is_still_the_core_layer() {
        let host = booted();
        set_screen_size(&host, 1104, 736).expect("第一次写尺寸");
        install(&host).expect("`Graphics` 的这一格装第二次应当幂等");
        assert_eq!(
            num(&host, "[Graphics.width, Graphics.boxHeight].join(',')"),
            "1104,736",
            "重装之后字段还在（`defineProperty` 只换描述符，不动字段值）"
        );
    }

    /// 访问器**真的是访问器**（不是被谁降级成字段）：描述符里有 `get`/`set`、`configurable` 为真。
    #[test]
    fn the_screen_readings_are_accessors_not_plain_fields() {
        let host = booted();
        assert_eq!(
            num(
                &host,
                "[typeof Object.getOwnPropertyDescriptor(Graphics, 'width').get, \
                 typeof Object.getOwnPropertyDescriptor(Graphics, 'width').set, \
                 String(Object.getOwnPropertyDescriptor(Graphics, 'boxWidth').configurable), \
                 String(Object.getOwnPropertyDescriptor(Graphics, 'scale').get.call(Graphics))].join(',')"
            ),
            "function,function,true,undefined",
            "`width` 有 get+set、`boxWidth` 可重定义（双装的前提）、`scale` 在没人写字段时读 undefined"
        );
    }
    /// `WebAudio` 那一圈装完之后，兼容层的音频面是**"全都喊得动、一声不出"**：
    /// `_context` 留在 `null`（native 没有 `AudioContext` 这个全局，`_createContext` 的
    /// 两条分支都不进），两颗 codec 谓词答 `false`，而 `canPlayOgg` 第一次把 `_initialized`
    /// 落成 `true`（上游那一格就是"没初始化就先初始化"）。
    ///
    /// 后两格是**浏览器那一边**的形状（`booted` 的文档讲为什么要按这一档读）：
    /// `_standAlone` 恒 `false`、`ResourceHandler.createLoader` 在场。钉死这两格 ⇒
    /// 同一份断言在两条腿上是同一个读数，不会由"这一条测试前面跑过什么"决定。
    #[test]
    fn the_audio_round_boots_quietly_and_without_a_context() {
        let host = booted();
        let js = "[String(typeof WebAudio.prototype._load), String(WebAudio._context), \
                  String(WebAudio._initialized), String(WebAudio._standAlone), \
                  String(WebAudio.canPlayOgg()), String(WebAudio.canPlayM4a()), \
                  String(WebAudio._initialized), String(typeof ResourceHandler.createLoader)].join('|')";
        assert_eq!(
            num(&host, js),
            "function|null|false|false|false|false|true|function",
            "`_context` 必须是 null、codec 必须是 false ⇒ 整圈是「安静」的；`canPlayOgg` 之后 \
             `_initialized` 才翻成 true（上游那一格的先后）；`_standAlone` 与 `createLoader` \
             两格钉的是 realm 无关性 —— 只数 native 会把「浏览器腿上这条判据根本不存在」读成「两边都过了」"
        );
    }

    /// 一整条播放腿（`play`/`fadeIn`/`fadeOut`/`seek`/`stop` 与两支监听器）**一个都不抛**，
    /// 而"就绪"三谓词全 false、三个字段是上游的初值 —— 这就是这一格今天的形状：
    /// 插件压 `AudioManager.playBgm`/`playSe` 的别名链跑得动，少的是出声。
    #[test]
    fn a_web_audio_buffer_answers_every_play_call_and_stays_not_ready() {
        let host = booted();
        let js = "var t = ''; try { \
                      var w = new WebAudio('audio/bgm/sea_of_trees.m4a'); \
                      w.play(true, 3); w.fadeIn(0.5); w.fadeOut(0.5); w.seek(1); w.stop(); \
                      w.addLoadListener(function() {}); w.addStopListener(function() {}); \
                      t = [w.url, String(w.isReady()), String(w.isPlaying()), String(w.isError()), \
                           String(w.volume), String(w.pitch), String(w.pan), \
                           String(WebAudio._context)].join('|'); \
                  } catch (e) { t = 'THROW:' + e; } t";
        assert_eq!(
            num(&host, js),
            "audio/bgm/sea_of_trees.m4a|false|false|false|1|1|0|null",
            "`url` 是上游那一句 `this._url` 的原样（`AudioManager` 的 URL 拼写就靠它）； \
             三支谓词 false ⇒ `isReady()` 把播放挡住，这才是「不出声」的落点，不是有人 catch 了异常"
        );
    }

    /// `Graphics.setVideoVolume` 是**跨类点名**切进来的一格（`--extra Graphics.setVideoVolume`）：
    /// 已交付的 `AudioManager.masterVolume` setter 第二句就喊它，而它的体只读 `_video`
    /// （null ⇒ 整块不进）⇒ 落得下字段、不碰没交的视频那一族。
    #[test]
    fn set_video_volume_lands_on_the_field_without_a_video_element() {
        let host = booted();
        let js = "Graphics.setVideoVolume(0.5); \
                  [String(Graphics._videoVolume), String(Graphics._video), \
                   String(typeof Graphics.playVideo)].join('|')";
        assert_eq!(
            num(&host, js),
            "0.5|null|undefined",
            "`_videoVolume` 落上、`_video` 还是 null，而 `playVideo` 仍然没交（M10.4 第一格那条 \
             视频零影响面的读数：磁盘 `261`/`262` 各 0 条）"
        );
    }

    /// **自创面只有这两格**：`_detectCodecs`（`document.createElement('audio')`）与
    /// `_setupEventHandlers`（`document.addEventListener` 两支）。两格的体是空的，
    /// 而这一格里其余 21 格静态成员、29 条 prototype 方法都是上游文本。
    /// 平台的假 `document` 不许用来救这两格（`platform.rs` 模块头第③条）。
    #[test]
    fn the_two_dom_cells_are_the_only_bodies_the_layer_substitutes() {
        let host = booted();
        assert_eq!(
            num(
                &host,
                "String(WebAudio._detectCodecs.length) + '|' + String(WebAudio._setupEventHandlers.length)"
            ),
            "0|0",
            "两格的体必须是空的（自创的底，`objects-verbatim.py` 的 `HOME_MADE` 名单数得到）"
        );
        assert_eq!(
            num(&host, "String(Object.keys(WebAudio).length)"),
            "21",
            "静态成员 21 格：六格数据形状 + 十五格函数（含跨类点名的那一格不算在这里，它挂 `Graphics`）"
        );
    }

    /// 重装是核心层：第二次 `install_audio` 不抛，而且**已经 new 出来的那颗 buffer 还能用**
    /// （与 `reinstalling_the_cache_classes_keeps_the_entries` 同一条腿）。
    #[test]
    fn reinstalling_the_audio_round_is_still_the_core_layer() {
        let host = booted();
        let before = num(
            &host,
            "globalThis.__w = new WebAudio('audio/bgm/x.m4a'); __w.url",
        );
        assert_eq!(before, "audio/bgm/x.m4a");
        install_audio(&host).expect("音频那一圈第二次装不上");
        assert_eq!(
            num(
                &host,
                "__w.url + '|' + String(__w.isReady()) + '|' + String(WebAudio._context)"
            ),
            "audio/bgm/x.m4a|false|null",
            "重定义类不动已存在的实例，也不把 `_context` 变成别的东西"
        );
    }
}
