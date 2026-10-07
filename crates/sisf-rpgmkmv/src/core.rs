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
//! - **继续欠着**：`Bitmap` 的**光栅那半**（画法十格 + `resize` + `canvas`/`context`/`baseTexture`/
//!   `paintOpacity` 四格读数 + `snap` + 滤镜那一族 = **25 格**，逐名钉在
//!   `the_drawing_half_is_undelivered_and_says_which_cells`）与 `Graphics` 的渲染面。
//!   M10.5 的第四格两小格已经把整颗 `Bitmap` 接到**除了滤镜**的那一处（[`BITMAP_JS`] 逐字 +
//!   [`BITMAP_HOST_JS`] 两格宿主底 + [`SISF_CONTEXT_JS`] 那一层画布上下文 + [`crate::raster`] 的
//!   CPU 像素 + [`crate::paint`] 的操作契约），
//!   所以"位图接 Rust 还是登记成降级"这一格已由 `M10.md` 的待定项 2 定案成**全 shim**：
//!   尺寸的读数从此是真的（`ImageCache._truncateCache` 第一次裁得动），而"画进去"那一半
//!   等的是引擎那三只把手（事实 75），不是等一个决定。
//!   需求面也是量出来的（`--demand`）：103 条插件里 `new Bitmap(` **35 处 / 15 条插件，顶格行 0 处**
//!   ⇒ 装载期不构造位图；覆写点 9 处里只有 `YEP_CoreEngine` 那 6 格是**原格的别名**。
//! - **句柄的契约**：缓存四颗问的那个对象**现在就是上游那一颗真的 `Bitmap`**
//!   （`ImageManager.requestNormalBitmap` 交的是 `Bitmap.request(path)`，M10.5 第四格）。
//!   判据 `the_handle_answers_the_cache_contract` 因此从"宿主自己写的对象形状对不对"改成
//!   "真位图在这一族问话下答得对不对"，并多钉一条**取不到素材时不许谎称就绪**
//!   （旧的 `_sisfHandle` 那一版是恒 `isReady() === true` 的）。
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

/// 自创的底：**`PIXI.Container` 那一层**（M10.5 第五小格第一格，2026-10-05）。
///
/// 与 [`PIXI_SHIM_JS`] 同一档（不比上游，由 `objects-verbatim.py` 的 `SHIM_BASE` 双向名单数）。
/// 存在的理由量在文件头与 `M10.md`「第五小格开工」：13 颗挂 `PIXI.…` 的渲染类里 **6 颗**挂的
/// 就是 `PIXI.Container`，而 `Scene_Base extends Stage extends PIXI.Container` ⇒ 交不出这一层，
/// 上游文本在装载那一刻的 `Object.create(PIXI.Container.prototype)` 就抛。
///
/// **成员面只交已交付文本真读得到的那几格**（尺子 `container-face.py` 的 17 名里，
/// `worldTransform`/`tilePosition`/`calculateVertices`/`pluginName`/`anchor` 随各自的渲染类一起加），
/// 而 `viewport` **故意不给** —— 上游读到的是 `undefined`。
pub const CONTAINER_SHIM_JS: &str = include_str!("js/shim_display.js");

/// `rpg_core.js:7612-7621` 的 **`Stage`**：1 条链 + 1 条 `constructor` 补句 + 1 条 `initialize`。
/// 构造体**沿用 [`crate::shapes`] 那颗壳**（体就是 `this.initialize.apply(this, arguments);`）。
///
/// 文本由 `../sisf_pray/tools/probes/objects-slice.py --file rpg_core.js --classes Stage
/// --allow-base PIXI.Container` 切出，`objects-verbatim.py` 逐字对表。这一颗是容器树的第一颗：
/// `Graphics._stage` 与 `Scene_Base` 都挂在它下面，而它自己只动 `children`/`parent` 那一族。
pub const STAGE_JS: &str = include_str!("js/core_stage.js");

/// 自创的底：**一颗离屏画布**（`document.createElement('canvas')`，第二格 `Sprite` 的 tint 缓冲）。
///
/// `platform.rs` 模块头那条"有意不交 `document`"的判词**没有被推翻**：这一格只交 `createElement`
/// 且只认 `'canvas'`，别的一律点名抛，而且**不给** `addEventListener`/`getElementById`/`body`
/// ⇒ 特性探测读到的仍是"没有这一格"。范围与九处调用点写在文件头
/// （`M10.md`「第五小格第二格开工」那张表的第四行）。
pub const DOM_SHIM_JS: &str = include_str!("js/shim_dom.js");

/// `rpg_core.js:3949-4331` 的 **`Sprite`**：14 条方法 + 4 单属性访问器（`bitmap`/`width`/`height`/
/// `opacity`）+ 1 条链 + 1 条 `constructor` 补句 + 2 条静态常量（`voidFilter`/`_counter`）+
/// **2 条别名赋值**（`_renderCanvas_PIXI`/`_renderWebGL_PIXI`，切片器的第九类形状）。
///
/// 欠的三格由 `--skip` 点名（渲染会话那一族）：`Sprite._renderCanvas`/`_renderWebGL`/
/// `_speedUpCustomBlendModes` —— 判据 `the_sprite_render_cells_are_skipped_while_the_alias_sources_are_there`
/// 逐名钉 `undefined`。文本由 `../sisf_pray/tools/probes/objects-slice.py --file rpg_core.js
/// --classes Sprite --allow-base PIXI.Sprite --skip
/// 'Sprite._renderCanvas,Sprite._renderWebGL,Sprite._speedUpCustomBlendModes'` 切出。
pub const SPRITE_JS: &str = include_str!("js/core_sprite.js");

/// `rpg_core.js:6138-6261` + `:7443-7496` + `:7503-7611` 的**三颗**：`ScreenSprite`（4 条方法 +
/// 3 单属性访问器 + `warnYep` + 那颗静态常量 `YEPWarned`）、`ToneFilter`（**构造体连体交** +
/// 3 条 `adjust*`）、`ToneSprite`（`initialize`/`clear`/`setTone` 三条）。
///
/// 欠的两格由 `--skip` 点名：`ToneSprite._renderCanvas`/`_renderWebGL`（渲染会话那一族，同 `Sprite`
/// 那三格）。⚠ 与 `Sprite` 那一颗**不同的一条腿**：`ToneSprite` 挂在 `PIXI.Container` 上，而底上
/// **没有** `_renderCanvas`（那两颗空体挂在 `PIXI.Sprite.prototype` 上）⇒ 跳掉之后
/// `ToneSprite.prototype._renderCanvas` 是 `undefined` 而不是继承来的空壳，判据
/// `the_tone_sprite_state_is_the_canvas_branch_and_its_render_cells_are_undefined` 两种都钉。
///
/// 文本由 `../sisf_pray/tools/probes/objects-slice.py --file rpg_core.js --classes
/// ScreenSprite,ToneSprite,ToneFilter --allow-base PIXI.Container,PIXI.filters.ColorMatrixFilter
/// --ctor --skip 'ToneSprite._renderCanvas,ToneSprite._renderWebGL'` 切出。
pub const SCREEN_AND_TONE_JS: &str = include_str!("js/core_screen_and_tone.js");

/// `rpg_core.js:6945-7165` 的 **`WindowLayer`**：4 条方法（`initialize`/`onRemoveAsAChild`/`move`/
/// `update`）+ 2 格单属性访问器（`width`/`height`）+ 1 条静态常量（`voidFilter`，装载那一刻就 `new`）
/// + 1 条链 + 1 条 `constructor` 补句。
///
/// 欠的**四格**由 `--skip` 点名，全在渲染会话那一族：`renderCanvas`/`renderWebGL`/
/// `_canvasClearWindowRect`/`_maskWindow`（`:7033-7165`）—— 前两格踩的是 `renderer.context`/
/// `filterManager`/`maskManager`，后两格踩的是 `PIXI.Point` 之外那颗 `_windowMask` 的
/// `_currentBounds`/`boundsDirty`（`rpg_core.js:7163-7164`）⇒ 那两名随「宿主那侧的真绘制」一起加。
/// ⚠ 与 `Sprite` 那一格同一条谎：`renderCanvas`/`renderWebGL` 读出来是 `function` —— 底上那两颗
/// 空体被继承了，点名用的是 own-ness（`M10.md` 事实 96 在这一颗上的第二次应用）。
///
/// 文本由 `../sisf_pray/tools/probes/objects-slice.py --file rpg_core.js --classes WindowLayer
/// --allow-base PIXI.Container --skip
/// 'WindowLayer.renderCanvas,WindowLayer.renderWebGL,WindowLayer._canvasClearWindowRect,WindowLayer._maskWindow'` 切出。
pub const WINDOW_LAYER_JS: &str = include_str!("js/core_window_layer.js");

/// `rpg_core.js:6258-6936` 的 **`Window`**：容器树上**第一颗整颗交出去的类** ——
/// **22 条 prototype 方法 + 10 块单属性访问器 + 1 条链 + 1 条 `constructor` 补句，欠账 0 格**。
///
/// 为什么这一颗能整颗交而前三颗要跳渲染会话：它那 22 条**一条都不喊 WebGL、也不自己画到屏幕上** ——
/// 六格 `_refresh*` 画的是**位图操作**（`Bitmap.blt`，尺子 `window-face.py` 第⑤段数到 `blt` 的文本处数
/// 2/8/9/0/0/0），而那一族今天已经通了（第四格那三小格的账）。于是这一格之后操作流里**第一次有窗皮
/// 那十几笔**，而 `__sisf_composite_report` 的 `applied` 仍是 `0` —— 合成是懒的（事实 98 同一条腿）。
///
/// 它是 **54 颗 `Window_*` 的基类**：`rpg_windows.js:14` 一颗 `Object.create(Window.prototype)`
/// （= `Window_Base`）+ 借格两处（`:18` 的 `initialize`、`:110` 的 `update`），而 `new Window(`
/// 在上游六文件里 **0 处** ⇒ 所有窗都只从 `Window_Base` 这一条腿下来。
///
/// 唯一的新底是 `PIXI.Sprite` 上那颗 `anchor`（尺子第③段：`Window` 自己体里 6 处写
/// `X.anchor.x/.y`，而全仓 `.anchor = new …` 整颗替换 **0** 处 ⇒ 交的是**一颗在场且能被改写的对象**，
/// 不是访问器）。
///
/// 文本由 `../sisf_pray/tools/probes/objects-slice.py --file rpg_core.js --classes Window
/// --allow-base PIXI.Container` 切出（**没有 `--skip`**）。
pub const WINDOW_JS: &str = include_str!("js/core_window.js");

/// `rpg_core.js:4522-5487` 的 **`Tilemap`**：`Tilemap` 那一族（`Tilemap`/`ShaderTilemap`/
/// `TilingSprite`/`Weather`）的第一颗，也是**这一族里唯一一颗零新底**的类 ——
/// **24 条 prototype 方法 + 22 条函数型静态 + 13 格静态常量 + 4 块单属性访问器 + 1 条链 +
/// 1 条 `constructor` 补句，欠账 0 格**（`src/js/core_tilemap.js` **736 行**，本 crate 目前最长的一份）。
///
/// 为什么先交它（尺子 `../sisf_pray/tools/probes/tilemap-face.py` 第④⑤段）：它点名的每一颗都有底
/// （`PIXI.Container` 那层、`new Bitmap`/`Point`/`Sprite`、`Sprite.prototype.move`/`setFrame` 的逐字、
/// `Number.prototype.mod` 与 `Array.prototype.equals` 在 `shapes::install_extensions`、
/// `Graphics.width`/`height` 在 [`GRAPHICS_JS`]），而它自己写位图**只有三种手段**
/// （`bltImage` 5 处 + `fillRect` 1 处 + `clearRect` 2 处）、**绘制会话那一族一个名字都不喊**
/// （`renderer`/`shader`/`gl.` 各 0）⇒ 与 `Window` 落在同一条已经通了的路上。另三颗的对照：
/// `ShaderTilemap` `renderer=10`、`TilingSprite` `renderer=7` 而位图操作 **0**（它们的"画"整个在
/// 没交的那一侧）、`Weather` `fillAll=2 drawCircle=1` 而与 `Tilemap` 同档 ⇒ 排第二。
///
/// **它是真流的那一条腿**：`rpg_sprites.js:2300/:2302` 那一格是 `if (Graphics.isWebGL())` 的岔，
/// 而我们这一层 `isWebGL()` 恒为假（`M10.md` 事实 101 第三次应用）⇒ `Spriteset_Map` 拿到的就是这一颗。
/// 那 13 格静态常量是**图块号的真相来源**（`TILE_ID_A1 = 2048` 那一族 + 三张 `*_AUTOTILE_TABLE`），
/// 右值不是函数 ⇒ 逐字尺子比不到，由判据数（同 [`JSON_EX_JS`] 那 2 格一条腿）。
///
/// 文本由 `../sisf_pray/tools/probes/objects-slice.py --file rpg_core.js --classes Tilemap
/// --allow-base PIXI.Container` 切出（**没有 `--skip`**）。
pub const TILEMAP_JS: &str = include_str!("js/core_tilemap.js");

/// `rpg_core.js:7242-7443` 的 **`Weather`**：`Tilemap` 那一族的第二颗，同样**零新底、整颗交出**
/// —— **13 条 prototype 方法 + 1 条链 + 1 条 `constructor` 补句，静态 0 格、访问器 0 块、欠账 0 格**
/// （`src/js/core_weather.js` 135 行）。排第二的理由由尺子第⑤段给：它写位图用的是 `fillAll` **2** 处 +
/// `drawCircle` **1** 处（`Bitmap.fillRect` 与 `fillArc` 那两条具名操作早就在场），而绘制会话那一族
/// （`renderer`/`shader`/`gl.`）**一个名字都不喊** ⇒ 与 `Tilemap` 同档。
///
/// 它是 `Spriteset_Map:2362` 那一句 `new Weather()` 的落点（`236` Set Weather 的可见后果那一族，
/// `AGENTS.md` 咬人第 14 条那条"同族的另一格"），也是**容器树上第一颗要 `Math.randomInt` 的类**
/// （`_rebornSprite` 那一格，`shapes::install_extensions` 交的那五条扩展之一）⇒ 判据只能钉**区间**
/// 与**关系**（`x == ax - origin.x`、每一拍之后 `opacity ∈ [40,219]`），钉不了具体随机数。
///
/// ⚠ `_addSprite` 那句 `new Sprite(this.viewport)` 里那一格 **`viewport` 全工程从没被写过**
/// （尺子 `container-face.py` 量的那一族，`js/shim_display.js` 的头注也点过名 ⇒ 底上**故意不给**）
/// ⇒ 上游拿到的就是 `undefined`，那颗 `Sprite` 的 `bitmap` 初值随之是 `undefined` 而不是 `null`；
/// 每一拍 `_update*Sprite` 都重新写 `sprite.bitmap`，所以这一格不落到屏幕上，只落到读数里（事实 113）。
///
/// 文本由 `../sisf_pray/tools/probes/objects-slice.py --file rpg_core.js --classes Weather
/// --allow-base PIXI.Container` 切出（**没有 `--skip`**）。
pub const WEATHER_JS: &str = include_str!("js/core_weather.js");

/// 自创的底：**`PIXI.extras` 那一层**（`TilingSprite` 的链基类，M10.5 第五小格第六格第三小格）。
///
/// 与 [`PIXI_SHIM_JS`]/[`CONTAINER_SHIM_JS`] 同一档（不比上游，由 `objects-verbatim.py` 的
/// `SHIM_BASE` 双向名单数 ⇒ 这一格之后 **58 → 63 颗**：那颗命名空间是**数据形状**，数不到，
/// 由判据 `the_tiling_sprite_base_is_the_measured_names_and_clone` 数）。
///
/// 存在的理由量在文件头与 `M10.md`「第五小格第六格第三小格开工」：上游 `TilingSprite` 的**第一句**
/// 就是 `Object.create(PIXI.extras.PictureTilingSprite.prototype)`（`rpg_core.js:5910`），而
/// `PIXI.extras` 在已交付文本里**一颗都没有** ⇒ 装载那一刻就抛。成员面只有 **1 名**是量出来而
/// 两处都没有的（`tilePosition`），另外四颗具名底全是**别名赋值的源**（`:5916`/`:5934`/`:5935`/`:6063`）。
pub const EXTRAS_SHIM_JS: &str = include_str!("js/shim_extras.js");

/// `rpg_core.js:5906-6138` 的 **`TilingSprite`**：`Tilemap` 那一族的第三颗，容器树上**第四颗欠账 0 格**
/// 的类（`src/js/core_tiling_sprite.js` **136 行**：**10 条方法** + 2 块单属性访问器
/// （`bitmap`/`opacity`）+ 1 条链 + 1 条 `constructor` 补句 + **4 条别名赋值**）。
///
/// ⚠ 那 10 条里有**一名写了两遍**：`_renderWebGL` 定义在 `:5956` 与 `:6097`，同一句赋值 ⇒
/// 运行期**后一条盖掉前一条**（后那一条多一句 `_speedUpCustomBlendModes`）⇒ 判据钉"在场的正是
/// `:6103` 那一句的形状"，而切片器按行区间两条都切（去重之后 9 名，尺子 `tiling-sprite-face.py` 第①段）。
///
/// 这一颗是**容器树上第一颗要新底的类**（`Weather`/`Tilemap` 都是零新底）：`PIXI.extras` 那五名，
/// 见 [`EXTRAS_SHIM_JS`]。而 `_speedUpCustomBlendModes` 那一处别名的源**仍是空的** —— 它是第二格
/// 点名跳过的那一格（事实 95），而它的两处读者全在 `Graphics.isWebGL()` 恒假那一条腿上（事实 101 的
/// **第四次**应用）⇒ **不为它造底**，由判据点名它落成 `undefined`。
///
/// 文本由 `../sisf_pray/tools/probes/objects-slice.py --file rpg_core.js --classes TilingSprite
/// --allow-base PIXI.extras.PictureTilingSprite` 切出（**没有 `--skip`**）。
pub const TILING_SPRITE_JS: &str = include_str!("js/core_tiling_sprite.js");

/// `rpg_core.js:5487-5641` 的 **`ShaderTilemap`**：`Tilemap` 那一族的第四颗、容器树上**第五条
/// 「无 `--skip`、欠账 0 格」的读数**（`src/js/core_shader_tilemap.js` **318 行**：
/// **15 条 prototype 方法**（无一名写两遍，与 `TilingSprite` 那一格对照）+ 1 条链
/// （`Object.create(Tilemap.prototype)`，**这一族里唯一挂在“自己人”上的**）+ 1 条 `constructor` 补句
/// **加上 1 颗构造体**（`--ctor` 开：体是 `Tilemap.apply(this, arguments); this.roundPixels = true;`，
/// 不是 `initialize` 派分 ⇒ 构造体 3 → **4 颗**，**再加 4 句对外物的点号名赋值**）。
///
/// 那四句（`:5496-5499`）是切片器的**第十类形状**（`PIXI.glCore.VertexArrayObject.FORCE_NATIVE`
/// / `PIXI.settings.GC_MODE` / `PIXI.tilemap.TileRenderer.SCALE_MODE` / `.DO_CLEAR`）：它们不写在
/// `ShaderTilemap` 这个名字上，而是**改 PIXI 的四格配置** ⇒ 归属只能按“它出现在哪颗类的行区间里”。
/// 右值不是函数 ⇒ 逐字尺子同样比不到（与 [`TILEMAP_JS`] 那 13 格静态常量一条腿），由判据
/// `the_shader_tilemap_load_lands_the_four_gl_settings` 逐格钉值。
///
/// **这一颗是容器树上第一条“走不到的腿”**：`rpg_sprites.js:2299` 那一格是
/// `if (Graphics.isWebGL())`，而我们的 `isWebGL()` 恒假（事实 101 的**第五次**应用）⇒ 真流里
/// `Spriteset_Map` 拿到的永远是 [`TILEMAP_JS`] 那一颗。它仍然按待定项 2 的定案**照交不跳**，
/// 代价是四颗命名空间底（[`PIXI_SHIM_JS`]，装载那一刻读得到）与**两名不给**：
/// `PIXI.tilemap.ZLayer` / `PIXI.tilemap.CompositeRectTileLayer` 只在 `_createLayers` 的构造期被读
/// （`:5604/:5605/:5610/:5612`），给它们造假底就是交一个没有任何读数的空壳 ⇒
/// 判据 `the_shader_tilemap_constructor_stops_at_the_gl_layer_names` 钉的是"`new ShaderTilemap()`
/// 当场抛在 `PIXI.tilemap.ZLayer`"这个**读数**：哪天它不抛了，就是有人开始走那条腿。
///
/// 文本由 `../sisf_pray/tools/probes/objects-slice.py --file rpg_core.js --classes ShaderTilemap
/// --allow-base Tilemap --ctor` 切出（**没有 `--skip`**）。
pub const SHADER_TILEMAP_JS: &str = include_str!("js/core_shader_tilemap.js");

/// **这一族自己写的探针**（不是上游文本）：容器树那十三段有没有已经在**这个 realm** 里跑过。
/// 认的是**最后**那一段（`core_tiling_sprite.js`）才落的那格自有访问器 `bitmap`
/// （`js/core_tiling_sprite.js:51`）：`shapes::install` 那颗壳的 `TilingSprite.prototype` 是空的，
/// 而这一段的第一句 `TilingSprite.prototype = Object.create(…)` 会把**整颗 prototype 换掉**，
/// 所以"壳在场"不算证据 —— 只有换过之后又 define 上去的那一格才算。
const DISPLAY_READY_PROBE: &str = "typeof TilingSprite === 'function' \
     && !!Object.getOwnPropertyDescriptor(TilingSprite.prototype, 'bitmap')";

/// 装上容器树的那十三段。**幂等的落法是「先问再装、第二次跳过」，不是「重跑一遍」**：
/// 这一族每一段的第一句都是 `X.prototype = Object.create(…)`（**整颗替换**，上游就这么写的，
/// 逐字交出的文本改不了），重跑一次就把 `Sprite.prototype`、`TilingSprite.prototype` 换成**新的
/// 对象**，而已经接好的那些链 —— `Spriteset_Base`/`Spriteset_Map` 接的就是**旧的**
/// `Sprite.prototype`（在 [`crate::sprites::install`] 那一刻）—— 当场**孤儿化**：
/// `new Spriteset_Map() instanceof Sprite` 从 `true` 变成 `false`，**一声不响**。
/// 这一格是 M10.5 窗那一格报出来的（[`crate::windows::install`] 也踩这里，它的链基类 `Window`
/// 住在这十三段里），判据 `installing_the_display_layer_twice_keeps_a_built_stage` 钉两半：
/// 第二次不抛，而且先前建好的树**与链**一条都不掉。
///
/// 十三段的内容：`PIXI` 的底（`Point`/`Rectangle`/`BaseTexture` 那一层 + `Container`/
/// `Sprite`/`Texture`/`Graphics`/`filters.{VoidFilter,ColorMatrixFilter}` + **第四小格那四颗
/// `ShaderTilemap` 要读到的命名空间**）+ `PIXI.extras` 那一层
/// （**第三小格新加的，压在 `Container` 之后**：它的体第一句就 `PIXI.Sprite.call(this, texture)`，
/// 而那颗底住在 `CONTAINER_SHIM_JS` 里）+ 那颗离屏画布 +
/// 上游的 `Stage`、`Sprite`、`ScreenSprite`/`ToneSprite`/`ToneFilter`、`WindowLayer`、`Window`、
/// `Tilemap`、`ShaderTilemap`、`Weather` 与 `TilingSprite`（`Window` 压在 `WindowLayer` 之后：`WindowLayer.initialize` 的 `children` 里那些
/// 孩子就是窗，而 `Window` 自己那句 `PIXI.Container.call(this)` 要的底早在第一格就装好了；
/// `ShaderTilemap` 紧跟 `Tilemap`（它的链基类就是那颗，晚一步就是装载那一刻 `Object.create(undefined)` 抛）；
/// `TilingSprite` 压在最后是因为它那**四处别名赋值的源**（`:5916`/`:5934`/`:5935`/`:6063`）
/// 全部住在 `EXTRAS_SHIM_JS` 里 ⇒ 晚一步就是装载那一刻 `Object.create(undefined)` 抛）。
///
/// **装载次序是这一格的内部前提**（同 [`install_geometry`] 那条纪律）：上游第一句就读
/// `PIXI.Sprite.prototype`，而 `Sprite.voidFilter` 在装载那一刻就 `new` 一颗滤镜；
/// `Sprite.prototype._renderCanvas_PIXI = PIXI.Sprite.prototype._renderCanvas`（`:4330`）
/// 更是**晚一步就静默成 `undefined`** 的那一格（不抛，插件那一拍才炸）⇒ 底必须先在。
/// 第三格同一条腿的新一格是 `ToneFilter`：它的**构造体第一句**就 `PIXI.filters.ColorMatrixFilter
/// .call(this)`，而 `Object.create(它的 prototype)` 在装载那一刻 —— 那颗底晚一步就是整段装不上。
pub fn install_display(host: &Host) -> Result<(), String> {
    if host.eval_bool(DISPLAY_READY_PROBE) {
        return Ok(());
    }
    host.eval(PIXI_SHIM_JS)
        .map_err(|e| format!("`PIXI` 那一层底装不上：{e}"))?;
    host.eval(CONTAINER_SHIM_JS)
        .map_err(|e| format!("`PIXI.Container` 那一层底装不上：{e}"))?;
    host.eval(EXTRAS_SHIM_JS)
        .map_err(|e| format!("`PIXI.extras` 那一层底装不上：{e}"))?;
    host.eval(DOM_SHIM_JS)
        .map_err(|e| format!("那颗离屏画布装不上：{e}"))?;
    host.eval(STAGE_JS)
        .map_err(|e| format!("`Stage` 那一条链装不上：{e}"))?;
    host.eval(SPRITE_JS)
        .map_err(|e| format!("`Sprite` 那一颗装不上：{e}"))?;
    host.eval(SCREEN_AND_TONE_JS)
        .map_err(|e| format!("`ScreenSprite`/`ToneSprite`/`ToneFilter` 那三颗装不上：{e}"))?;
    host.eval(WINDOW_LAYER_JS)
        .map_err(|e| format!("`WindowLayer` 那一颗装不上：{e}"))?;
    host.eval(WINDOW_JS)
        .map_err(|e| format!("`Window` 那一颗装不上：{e}"))?;
    host.eval(TILEMAP_JS)
        .map_err(|e| format!("`Tilemap` 那一颗装不上：{e}"))?;
    host.eval(SHADER_TILEMAP_JS)
        .map_err(|e| format!("`ShaderTilemap` 那一颗装不上：{e}"))?;
    host.eval(WEATHER_JS)
        .map_err(|e| format!("`Weather` 那一颗装不上：{e}"))?;
    host.eval(TILING_SPRITE_JS)
        .map_err(|e| format!("`TilingSprite` 那一颗装不上：{e}"))
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

/// 自创的底：**`this._context` 那一层**（MV 的 canvas 2D 上下文的替身，M10.5 第四格第二小格）。
///
/// 它**不是上游文本** ⇒ 逐字尺子不比内容，由 `objects-verbatim.py` 的 `SHIM_BASE` 双向名单数。
/// 存在的理由与"为什么自创面集中在这里而不是散进 17 格 `Bitmap` 体"写在文件头（一句话：
/// 实测那 17 格的体只碰 `this._context` 的 **20 个成员**，不碰 DOM canvas）。
/// 交出去的是**一条具名操作**，不是像素 —— 操作住在 [`crate::paint`] 的记录器里，
/// 真绘制随宿主的帧循环接上（`M10.md`「第四格第二小格开工」的边界⑴）。
pub const SISF_CONTEXT_JS: &str = include_str!("js/shim_context.js");

/// `rpg_core.js` 的 **`Bitmap`**：35 条逐字方法 + 延迟创建那一整块
/// （`Object.defineProperties(Bitmap.prototype, {…})`，`:801-821`）+ **九格逐字访问器**
/// （`url`/`baseTexture`/`canvas`/`context`/`width`/`height`/`rect`/`smooth`/`paintOpacity`）
/// + 静态 `load`/`request` 与 `_reuseImages`。
///
/// 文本由 `../sisf_pray/tools/probes/objects-slice.py --file rpg_core.js --classes Bitmap
/// --skip Bitmap._createCanvas,Bitmap._requestImage,Bitmap.blur,Bitmap.snap` 切出
/// （欠账名单见 [`BITMAP_JS`] 下面那一格的判据），`objects-verbatim.py` 逐字对表。
///
/// **为什么切在这里**（2026-10-05 读完整颗 `Bitmap` 之后推翻了原先记的三格切法）：原先写的
/// 第一格"数据那一半 = 12 条零硬底方法 + 六格 canvas-无关 getter"落不了地，因为
/// ⑴ `initialize` 的**第一句**就是 `this._createCanvas(width, height)`（`:831-833`）—— 不交
/// `_createCanvas` 的话 `new Bitmap(w,h)` 当场抛，而 `Bitmap.load`/`Bitmap.request` 都要 `initialize()`；
/// ⑵ `width`/`height` 读的是 `this._canvas.width`（`:1040`、`:1057`）—— 那个 `_canvas` 就是延迟创建
/// 那一块里的 getter，于是"尺寸读数"与"位图的存在方式"是同一格，切不开。
/// 第二小格又推翻了一次"按问得动/画得动切"：实测那 17 格画法体**只碰 context 的成员**，
/// 所以把 [`SISF_CONTEXT_JS`] 交出来之后，它们连同 `canvas`/`context`/`baseTexture`/
/// `paintOpacity`/`resize`/`getPixel`/`getAlphaPixel` **全部回归逐字**；第三小格把
/// `adjustTone`/`rotateHue` 也接回了逐字（`getImageData`/`putImageData` 那两条腿一通，
/// 两颗滤镜体的算术与夹逼全在上游文本里，移植侧一行不写）—— 今天不逐字的只剩两格宿主底
/// （[`BITMAP_HOST_JS`]）与**两格真的还欠**（`blur` 与 `snap`，判据逐名钉着）。
pub const BITMAP_JS: &str = include_str!("js/core_bitmap.js");

/// `Bitmap` 的**两格自创底**（`objects-verbatim.py` 的 `HOME_MADE` 双向白名单数得到这两格；
/// 上一档是四格，`getPixel`/`getAlphaPixel` 在第二小格回归逐字 —— 它们读的
/// `this._context.getImageData` 现在由 [`SISF_CONTEXT_JS`] 答得上）。
///
/// **① `_createCanvas`**（`:754-772`）—— 上游是 `document.createElement('canvas')` 加
/// `getContext('2d')` 加 `drawImage`。这一格交的是**一颗带访问器的尺寸记录**：`width`/`height`
/// 的 setter 喊 [`crate::paint`] 的 `size` 操作（上游给 `canvas.width` 赋值本身就是一次
/// 改尺寸 + 清空内容，`resize`（`:1129-1136`）靠的就是这一手，所以那一格才能逐字），
/// 并把 [`SISF_CONTEXT_JS`] 那一颗挂成 `__context`。
///
/// **② `_requestImage`**（`:1667-1691`）—— 上游是 `new Image()` 加 `Decrypter.decryptImg`
/// 加两支 `addEventListener`。这一格走**已有的 VFS 字节腿**（`MvAssets`：加密名映射、明文回退、
/// 内存解密，与 `data/*.json` 同一条），解码在 [`crate::raster`]。
/// **唯一一处与上游不同的是"哪一拍"**：`<img src>` 的回调在之后的某一拍，而问 VFS 是当场，
/// 所以 `_onLoad` 在这一格末尾同步喊掉。状态机的每一格都不变（`isReady`/`isError`/
/// `isRequestReady`/`isRequestOnly` 与 `_loadingState` 全是上游逐字），变的只有时序 ——
/// 表征是 `Bitmap.load(url)` **当拍就 ready**，而 `RequestQueue` 那条"一次只让一条在飞"的腿
/// 因此少了一拍才放掉队首（判据
/// `a_loaded_bitmap_walks_the_upstream_state_machine_and_answers_its_size` 与 managers 那一族的
/// `the_request_queue_drains_only_when_the_scene_loop_pumps` 一起钉这两头）。
pub const BITMAP_HOST_JS: &str = r##"
// 自创的底（两格）。每一格替换掉上游哪几行、少了什么，写在 `core.rs` 的 `BITMAP_HOST_JS` 文档里。

// ① 上游 `:754-772`。结构照上游（`__canvas` 复用、有 `_image` 时按图重设尺寸并建 base texture、
// 收尾 `_setDirty()`），少的只有 DOM：`__canvas` 是一颗**尺寸记录 + 访问器**（给 `width`/`height`
// 赋值就是一条 `size` 操作，与上游"赋值即改尺寸并清空"同一形状），`__context` 是
// `src/js/shim_context.js` 那一颗（每一笔画在那里兑现成一条具名操作）。
Bitmap.prototype._createCanvas = function(width, height) {
    if (!this.__canvas) {
        var rec = { __sid: Sisf2DContext.allocSid(), __url: '', _w: 1, _h: 1 };
        // **`v >>> 0` 是 `<canvas>` 的那条 IDL**：`width`/`height` 在规范里是 `unsigned long`，
        // 赋小数进去**当场截断**（`Window` 那一族会喂小数进来：`windowWidth()` 是
        // `measureText` 的浮点读数 ⇒ `contents.resize(335.082…)` 在上游落到 canvas 就是 335）。
        // 不截断的话那颗小数会一路走到 `getImageData` 的矩形上，而宿主那一侧的解析是**严格**的
        // （`pixels|…` 的四个数收不下小数 ⇒ `undelivered:bad-spec` 抛）—— 严格是要留的（形状坏了
        // 就是 bug），截断的活儿归 canvas 自己。**谁守着这一格**：`pray` 侧的
        // `the_opening_leg_reaches_map5_on_the_pump` —— 那条腿在 map 8 的选项窗上会真的喂进
        // `measureText` 的小数读数，去掉这个 `>>> 0` 它就在 `getImageData` 那一句抛。
        Object.defineProperty(rec, 'width', {
            get: function () { return rec._w; },
            set: function (v) { rec._w = v >>> 0; Sisf2DContext.setSize(rec.__sid, rec._w, rec._h, rec.__url); },
            configurable: true
        });
        Object.defineProperty(rec, 'height', {
            get: function () { return rec._h; },
            set: function (v) { rec._h = v >>> 0; Sisf2DContext.setSize(rec.__sid, rec._w, rec._h, rec.__url); },
            configurable: true
        });
        this.__canvas = rec;
    }
    if (!this.__context) {
        this.__context = new Sisf2DContext(this);
    }
    this.__canvas.__url = this._url || '';
    this.__canvas.width = Math.max(width || 0, 1);
    this.__canvas.height = Math.max(height || 0, 1);

    if(this._image){
        var w = Math.max(this._image.width || 0, 1);
        var h = Math.max(this._image.height || 0, 1);
        this.__canvas.width = w;
        this.__canvas.height = h;
        this._createBaseTexture(this.__canvas);
        // 上游这一格收尾就是这一句（`rpg_core.js:768` 的 `this.__context.drawImage(this._image, 0, 0)`），
        // 我们这里落成一条**具名操作** ⇒ 素材图的像素走的是与每一笔画同一条腿。
        // 少这一句，"文件载入的那一张"在软件合成（`crate::composite`）那一层就没有源：
        // 色调与 `getImageData` 读的是**画布**，而画布的内容由操作流决定，不由那张图决定。
        this.__context.drawImage(this._image, 0, 0);
    }

    this._setDirty();
};

// ② 上游 `:1667-1691`：`new Image()` + `Decrypter.decryptImg` + 两支 addEventListener。
// `_reuseImages` 那一族照上游用（`_clearImgInstance` 往里 push，这里 pop），否则逐字交出来的
// 那一格会把样品越堆越多却没人取。
Bitmap.prototype._requestImage = function(url){
    if(Bitmap._reuseImages.length !== 0){
        this._image = Bitmap._reuseImages.pop();
    }else{
        this._image = { src: '', width: 0, height: 0, onload: null, onerror: null };
    }
    // 上游的 `_onLoad`/`_onError`/`decode` 都要能对这颗"图"挂与摘监听器（逐字那三格里共四处）。
    this._image.addEventListener = function() {};
    this._image.removeEventListener = function() {};

    this._url = url;
    this._loadingState = 'requesting';

    var size = globalThis.__sisf_image_size(url);
    if(size === ''){
        this._onError();
        return;
    }
    var wh = size.split(',');
    this._image.src = url;
    this._image.width = Number(wh[0]);
    this._image.height = Number(wh[1]);
    // 上游在这里把 onload/onerror 挂上去，由浏览器之后的某一拍回调；问 VFS 是当场的，
    // 于是同一条回调在这里喊掉（时序差的那一条理由见上面的文档注释⑵）。
    this._onLoad();
};

"##;

/// 装上 `Bitmap`：`Sisf2DContext` 那一层底 + 逐字文本 + 两格宿主底，并挂上这一族的 transport
/// （尺寸、整块读像素、整块写像素、每一条画、操作流的读与清、文字宽度、盲区计数 ——
/// 名单在 [`crate::paint::install`]）。返回 [`crate::paint::Recorder`] —— 判据读操作流用的就是它，
/// 宿主换成真绘制时**重新绑同名全局**即可（记录器不必跟着活）。
///
/// 它**不在 [`install`] 里**：这一格要 `Rc<Vfs>`（位图字节的上游），而 `install` 的签名里没有。
/// 调用点是 [`crate::managers::install`]（`ImageManager` 的 `Bitmap.request`/`Bitmap.load` 脚下
/// 就是它），与本族"谁要谁自己装、次序不交给调用方去记"那两条纪律同一条腿
/// （判据 `managers_install_does_not_depend_on_the_caller_installing_core_first` 的同族）。
pub fn install_bitmap(
    host: &Host,
    vfs: std::rc::Rc<sisf_asset::Vfs>,
) -> Result<std::rc::Rc<crate::paint::Recorder>, String> {
    install_geometry(host)?;
    let rasters = crate::raster::Rasters::new(vfs);
    host.bind_str(crate::raster::SIZE_GLOBAL, {
        let rasters = std::rc::Rc::clone(&rasters);
        move |url| rasters.size(&url)
    })?;
    host.bind_str(crate::raster::PIXEL_GLOBAL, {
        let rasters = std::rc::Rc::clone(&rasters);
        move |spec| rasters.pixel(&spec)
    })?;
    let recorder = crate::paint::install(host, std::rc::Rc::clone(&rasters))?;
    // **幂等落在"已经 define 过就不再 define"这一格**（`M10.md`「这一族的规矩」第 4 条：手交的
    // JS 必须能装第二次，而 wasm 腿一个进程共享一个 realm）。上游那一块延迟创建的访问器用的是
    // `Object.defineProperties` 的**默认可配置性**（`configurable: false`，`rpg_core.js:801-821`），
    // 所以同一个 realm 里重 define 直接抛 `property is not configurable` —— 实测就是这么红了
    // `reinstalling_managers_is_the_core_layer_again`。文本不能包进 `if (…) { … }`：
    // 逐字尺子（`objects-verbatim.py`）是按**行首声明**解析的，一包就把那一整块弄丢一档。
    // 守卫的是 `_canvas` 那一格访问器在不在，而不是"这一族装过几次"：
    // `shapes::install` 第二次跑会换成**一颗新的 prototype**（`globalThis.Bitmap = function…`），
    // 那时描述符读不到 ⇒ 重新 define，正是该有的行为（同 `JS_EXTENSIONS_JS` 那条 `typeof` 守卫的
    // 落点，只是那一族的文本本来就不是逐字切片）。
    if host
        .try_eval_bool(
            "typeof Object.getOwnPropertyDescriptor(Bitmap.prototype, '_canvas').get \
             === 'function'",
        )
        .unwrap_or(false)
    {
        return Ok(recorder);
    }
    host.eval(SISF_CONTEXT_JS)
        .map_err(|e| format!("`Sisf2DContext` 那一层底装不上：{e}"))?;
    host.eval(BITMAP_JS)
        .map_err(|e| format!("`Bitmap` 的逐字那一半装不上：{e}"))?;
    host.eval(BITMAP_HOST_JS)
        .map_err(|e| format!("`Bitmap` 的两格宿主底装不上：{e}"))?;
    Ok(recorder)
}

/// 装上 core 的这一格：`Point`/`Rectangle`（踩着 `PIXI` 那层底）+ `JsonEx` + 缓存四颗 +
/// `Graphics` 的屏幕读数与报错面 + 容器树的第一颗（`PIXI.Container` + `Stage`）+ 音频三圈。
pub fn install(host: &Host) -> Result<(), String> {
    install_geometry(host)?;
    install_json_ex(host)?;
    install_cache(host)?;
    host.eval(GRAPHICS_JS)
        .map_err(|e| format!("`Graphics` 的屏幕那一格装不上：{e}"))?;
    install_display(host)?;
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
        install, install_audio, install_bitmap, install_cache, install_display, install_geometry,
        install_json_ex, set_screen_size,
    };
    use crate::jsrt::Host;
    use crate::shapes::{DeclaredShape, ShapeKind, install as install_shapes};
    use sisf_asset::{MemoryProvider, Vfs, VfsPath};
    use std::rc::Rc;

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
        names.extend(DISPLAY_SHELLS.iter().cloned());
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

    /// 容器树那几颗的壳（`Stage`/`Sprite`/`ScreenSprite`/`ToneSprite`）+ 那一层底：判据要的是
    /// "链与树"，不需要音频在场（同 `geometry_booted`/`cache_booted` 的最小化手法）。
    /// **`ToneFilter` 不在这一档**：它的构造体不是 `initialize` 派分（第一句是
    /// `PIXI.filters.ColorMatrixFilter.call(this)`）⇒ 连体交，见 [`super::SCREEN_AND_TONE_JS`]。
    const DISPLAY_SHELLS: &[(&str, ShapeKind)] = &[
        ("Stage", ShapeKind::Class),
        ("Sprite", ShapeKind::Class),
        ("ScreenSprite", ShapeKind::Class),
        ("ToneSprite", ShapeKind::Class),
        ("WindowLayer", ShapeKind::Class),
        ("Window", ShapeKind::Class),
        ("Tilemap", ShapeKind::Class),
        ("Weather", ShapeKind::Class),
        ("TilingSprite", ShapeKind::Class),
        ("ShaderTilemap", ShapeKind::Class),
    ];

    fn display_booted() -> Host {
        let mut names: Vec<(&str, ShapeKind)> = vec![("Graphics", ShapeKind::StaticClass)];
        names.extend(GEOMETRY_SHELLS.iter().cloned());
        names.extend(DISPLAY_SHELLS.iter().cloned());
        let host = shell(&names);
        install_geometry(&host).expect("`Point`/`Rectangle` 那两颗装不上");
        install_display(&host).expect("容器树那两颗装不上");
        host
    }

    /// `Sprite` 那一条腿的夹具：容器树 + `Bitmap` 与那颗画布上下文（`_createTinter` 要的
    /// `document.createElement('canvas')` 与 `getContext('2d')` 都住在 `Sisf2DContext` 上）。
    fn sprite_booted() -> Host {
        let mut names: Vec<(&str, ShapeKind)> = vec![
            ("Graphics", ShapeKind::StaticClass),
            ("Utils", ShapeKind::StaticClass),
        ];
        names.extend(GEOMETRY_SHELLS.iter().cloned());
        names.extend(CACHE_SHELLS.iter().map(|n| (*n, ShapeKind::Class)));
        names.extend(DISPLAY_SHELLS.iter().cloned());
        names.extend(BITMAP_SHELLS.iter().cloned());
        let host = shell(&names);
        crate::shapes::install_extensions(&host).expect("JsExtensions 装不上");
        // 只取 `Utils` 那一块（`_executeTint` 的三支 `fillStyle` 靠 `Utils.rgbToCssColor`）——
        // 整族 `platform::install` 还要 `Input`/`TouchInput` 那几颗壳，与这一格无关
        // （最小化手法同 `bitmap_booted` 只装扩展与缓存）。
        host.eval(crate::platform::UTILS_JS)
            .expect("`Utils` 装不上");
        install_cache(&host).expect("缓存四颗装不上");
        let mut mem = MemoryProvider::new();
        mem.insert(
            &VfsPath::new("img/system/Window.png").unwrap(),
            &crate::raster::sample_png_2x2(),
        );
        let mut vfs = Vfs::new();
        vfs.register(Box::new(mem));
        install_bitmap(&host, Rc::new(vfs)).expect("`Bitmap` 那一半装不上");
        install_geometry(&host).expect("`Point`/`Rectangle` 那两颗装不上");
        install_display(&host).expect("容器树那两颗装不上");
        host
    }

    /// `Tilemap` 与 `Weather` 两颗的夹具 = [`sprite_booted`] 那一层底，再加上两样：[`GRAPHICS_JS`] 那两块
    /// **屏幕读数访问器**，以及夹具 VFS 里多出来的一颗**真尺寸的图块组**
    /// （[`crate::raster::sample_tileset_png`] = 768×576，与上游 `img/tilesets/World_A1.png` 同一对数）。
    ///
    /// 为什么要 `Graphics`：`Tilemap.prototype.initialize` 头两句就是
    /// `Graphics.width + this._margin * 2`（`rpg_core.js:4533-4534`）—— 读不到就是 `NaN`，
    /// 而 `NaN` 进 `Math.ceil(NaN / 48) + 1` 会一路带到 `new Bitmap(NaN, NaN)`，**不抛**，
    /// 只是那颗层位图从此没有尺寸。
    /// ⚠ 屏幕那一格**只能写字段**（`Graphics._width = …`）：走访问器会敲 `_updateAllElements`，
    /// 那一格在 M10.4 第一格就被点名跳过（`js/core_graphics.js` 的头注）。
    ///
    /// 为什么要那颗 768×576：`Tilemap` 画图块走的是 `Bitmap.bltImage`，而**那道闸比的是源那一边**
    /// （事实 107 的同一条腿）—— 拿 2×2 的样品当图块组时一格图块都画不出来，量到的"零笔"会被
    /// 读成"画法没接上"。
    fn tilemap_booted() -> Host {
        let mut names: Vec<(&str, ShapeKind)> = vec![
            ("Graphics", ShapeKind::StaticClass),
            ("Utils", ShapeKind::StaticClass),
        ];
        names.extend(GEOMETRY_SHELLS.iter().cloned());
        names.extend(CACHE_SHELLS.iter().map(|n| (*n, ShapeKind::Class)));
        names.extend(DISPLAY_SHELLS.iter().cloned());
        names.extend(BITMAP_SHELLS.iter().cloned());
        let host = shell(&names);
        // `Number.prototype.mod` 与 `Array.prototype.equals` 都在这一颗里：前者是
        // `_updateLayerPositions`/`_paintTiles`/`_readMapData` 的落点（`:4817-4818`、`:4870-4871`），
        // 后者是 `_paintTiles` 那一格"这一格的图块列表跟上一次比有没有变"的闸（`:4919`）。
        crate::shapes::install_extensions(&host).expect("JsExtensions 装不上");
        host.eval(crate::platform::UTILS_JS)
            .expect("`Utils` 装不上");
        install_cache(&host).expect("缓存四颗装不上");
        let mut mem = MemoryProvider::new();
        mem.insert(
            &VfsPath::new("img/system/Window.png").unwrap(),
            &crate::raster::sample_png_2x2(),
        );
        mem.insert(
            &VfsPath::new("img/tilesets/World_A1.png").unwrap(),
            &crate::raster::sample_tileset_png(),
        );
        let mut vfs = Vfs::new();
        vfs.register(Box::new(mem));
        install_bitmap(&host, Rc::new(vfs)).expect("`Bitmap` 那一半装不上");
        install_geometry(&host).expect("`Point`/`Rectangle` 那两颗装不上");
        install_display(&host).expect("容器树那一族装不上");
        host.eval(super::GRAPHICS_JS)
            .expect("`Graphics` 的屏幕读数那一格装不上");
        host
    }

    /// 第三格那三颗的夹具：`Graphics` 的**屏幕读数**（`setColor` 那一支算 quad 用的就是
    /// `Graphics.width`/`height`，`:6247`）+ `Utils.rgbToCssColor`（写进 `_colorText`）+
    /// **只记账的 `console`**（`ScreenSprite.warnYep` 那一格一辈子喊一次，`:6178`）+ Annex B 的 `clamp`。
    ///
    /// 不需要 `Bitmap`、缓存那一族、音频那一圈 —— `ToneSprite`/`ToneFilter` 今天动的都是自己那几格状态，
    /// 而 `ScreenSprite` 要的那颗 `PIXI.Graphics` 住在容器树那一层底里。
    /// **这一档故意不借 [`install`] 的东风**（它自己会装 `Bitmap`/缓存 ⇒ 同一颗 `PIXI.Container`
    /// 在 QuickJS 里被 `Object.defineProperty` 两次就抛 `already has a property`，共享 realm 的
    /// 那条纪律在这里反过来咬了一次，`M10.md` 事实 99）。
    fn screen_tone_booted() -> Host {
        let mut names: Vec<(&str, ShapeKind)> = vec![
            ("Graphics", ShapeKind::StaticClass),
            ("Utils", ShapeKind::StaticClass),
        ];
        names.extend(GEOMETRY_SHELLS.iter().cloned());
        names.extend(DISPLAY_SHELLS.iter().cloned());
        let host = shell(&names);
        crate::shapes::install_extensions(&host).expect("JsExtensions 装不上");
        host.eval(crate::platform::UTILS_JS)
            .expect("`Utils` 装不上");
        crate::platform::install_console(&host).expect("那颗记账的 `console` 装不上");
        install_geometry(&host).expect("`Point`/`Rectangle` 那两颗装不上");
        host.eval(super::GRAPHICS_JS)
            .expect("`Graphics` 的屏幕读数那一格装不上");
        install_display(&host).expect("容器树那三颗装不上");
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

    /// **`PIXI.Container` 那一层的成员面 = 尺子量出来的那一族，一格不多一格不少**
    /// （`tools/probes/container-face.py`：13 颗链类的体里读的 `this.<名>` 中、上游四文件从没写过
    /// 定义的 17 名，本格只交 `Stage` 真读得到的那一族）。
    ///
    /// 两半都要钉：⑴ `prototype` 上那 8 格（多一颗就是"照 PIXI 的常识补"，少一颗就是下一颗链类
    /// 装载那一刻抛）；⑵ 构造体里的**字段初值只给量出来被读过的**（`opacity`/`blendMode`/`filters`/
    /// `_bitmap` 这些**上游自己会写**的必须还是 `undefined` —— shim 一填就把"谁写的"读不出来了，
    /// 而 `viewport` 那一名是**故意不给**：上游全工程从没写过它，读到的就是 `undefined`）。
    #[test]
    fn the_container_shim_is_exactly_the_measured_member_face() {
        let host = display_booted();
        let js = r#"
var out = [];
out.push(Object.keys(PIXI.Container.prototype).sort().join(','));
var c = new PIXI.Container();
out.push(Object.keys(c).sort().join(','));
out.push(['opacity', 'blendMode', 'filters', 'viewport', '_bitmap']
    .map(function(k) { return String(c[k]); }).join(','));
out.push(String(c.children.length) + ',' + c.alpha + ',' + c.visible + ',' + c.renderable);
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "addChild,addChildAt,on,removeChild,removeChildren,renderCanvas,renderWebGL,updateTransform\
             |alpha,children,parent,pivot,renderable,rotation,scale,visible,x,y\
             |undefined,undefined,undefined,undefined,undefined|0,1,true,true",
            "首格 = `prototype` 的八格（名单由 `SHIM_BASE` 双向数，这里再逐名钉一遍）；\
             第二格 = 构造体给的**十颗**字段（第三格多了 `scale` —— 理由不是 PIXI 有这一格，\
             而是 `ScreenSprite` 的 `anchor` getter 自己写它，`rpg_core.js:6186`）；\
             第三格钉**上游自己写的与全工程没人写的都不给初值**\
             （`viewport` 就是 `M10.md`「第五小格开工」定案 2 那一格）；末格是三个被读过的初值"
        );
    }

    /// **上游那颗 `Stage` 挂得上新交的底**（这一格的存在理由：`Scene_Base extends Stage`，
    /// 而 `Graphics._stage` 与每一层窗都在这棵树下面）。四格读数一条腿走完整棵树：
    /// 链（`instanceof`）→ 建（`initialize` 里 `PIXI.Container.call(this)` 落的字段 +
    /// 上游自己那句 `interactive = false`，注释写的是"会漏内存"）→ 挂孩子 → 摘孩子。
    /// 反面对照是**只装壳不装底**：`new Stage()` 当场抛（壳的 `initialize` 还没有），
    /// 这一档证明"底"不是可选的装饰。
    #[test]
    fn the_stage_from_upstream_text_chains_onto_the_container_base() {
        let shell_only = {
            let names: Vec<(&str, ShapeKind)> = vec![("Stage", ShapeKind::Class)];
            let host = shell(&names);
            host.try_eval_string("String(new Stage().children.length)")
                .is_err()
        };
        assert!(
            shell_only,
            "只有 `shapes` 那颗壳、没装 `PIXI.Container` 那一层底时，`new Stage()` 必须**抛** —— \
             反面对照；哪天它不抛了，就是壳开始自己写字段，那条腿要重看（事实 83 同一条纪律）"
        );

        let host = display_booted();
        let js = r#"
var out = [];
var s = new Stage();
out.push(String(s instanceof PIXI.Container) + ',' + String(s.constructor === Stage));
out.push(String(s.interactive) + ',' + s.children.length);
var a = { name: 'a' };
var b = { name: 'b' };
s.addChild(a);
s.addChildAt(b, 1);
out.push(s.children.length + ',' + (a.parent === s) + ',' + (b.parent === s) + ',' + (s.children[1] === b));
out.push(String(Stage.prototype.renderCanvas === PIXI.Container.prototype.renderCanvas));
s.removeChildren();
out.push(s.children.length + ',' + a.parent + ',' + b.parent);
var threw = 'no';
try { s.addChildAt({ name: 'c' }, 9); } catch (e) { threw = 'yes'; }
out.push(threw + ',' + s.children.length);
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "true,true|false,0|2,true,true,true|true|0,null,null|yes,0",
            "链（`instanceof` + `constructor` 补句）→ 建（`interactive` 是上游那句 false、`children` 空）\
             → 挂两个孩子（队尾与指定位子，`parent` 都认下这颗父亲，`children[1] === b` 钉的是\
             **插对位置**而不是「插进去就行」）→ 摘光（`parent` 回 `null`）→ 越界那一次**抛**并点名\
             （夹住就是把插错读成插对）"
        );
    }

    /// **今天交的是树的结构，不是树上的位置** —— 那三格（`updateTransform`/`renderCanvas`/
    /// `renderWebGL`）是**空体**，它们的名字叫「宿主那侧的真绘制」（`&mut Renderer` 只在
    /// `App::draw` 那一拍借得到，事实 82）。这一格把"空着的代价"钉成读数而不是静默：
    /// 三格都是 `function`（上游的别名 `Sprite._renderCanvas_PIXI` 与
    /// `PIXI.Container.prototype.updateTransform.call` 读得到），而 `worldTransform`
    /// **不在**（谁读它谁读到 `undefined` —— 下一颗挂变换的类（`ToneSprite`）必须显式加那一格，
    /// 而不是在这里偷偷预交）。
    #[test]
    fn the_container_tree_is_structure_only_and_the_draw_session_is_named() {
        let host = display_booted();
        let js = r#"
var s = new Stage();
s.x = 10;
s.updateTransform();
[s.updateTransform, s.renderCanvas, s.renderWebGL, s.on]
    .map(function(f) { return typeof f; }).join(',') + '|' + String(s.worldTransform)
"#;
        assert_eq!(
            num(&host, js),
            "function,function,function,function|undefined",
            "四格在场（上游喊得到）而变换**没算** —— 这就是「软件合成之上还差绘制会话」那一格的读数\
             （`M10.md`「第五小格开工」定案 3）"
        );
    }

    /// **装第二次**（`M10.md`「这一族的规矩」第 4 条：wasm 腿一整个进程共享一个 realm）。
    /// 第二次踩同一格走的是 [`install_display`] 头上那条**先问再装**的腿 ⇒ 不重define，于是两半都要钉：
    /// ① **已经建好的那颗 `Stage` 的孩子不能因为重装被抹掉**（重装换掉的是 `Stage.prototype` 那颗
    /// 对象，实例脚下的 `children` 数组是它自己的字段）；② 同一条腿的另一半在 `windows.rs` 那边钉
    /// —— 窗那一族的 `install` 也踩这里，第一次是**重跑**的写法把 `Spriteset_Map` 的链孤儿化了
    /// （`instanceof Sprite` 静默变 `false`，`M10.md` 事实 138）。
    #[test]
    fn installing_the_display_layer_twice_keeps_a_built_stage() {
        let host = display_booted();
        host.eval("globalThis.__keep = new Stage(); __keep.addChild({ name: 'kept' });")
            .expect("建不出那棵树");
        install_display(&host).expect("第二次装不上（共享 realm 里重define 不许抛）");
        assert_eq!(
            num(
                &host,
                "String(__keep.children.length) + ',' + __keep.children[0].name"
            ),
            "1,kept",
            "重装之后那棵树还要带着它的孩子（抹掉就是「上一批判据装的树没了」那种假绿）"
        );
    }

    /// **那颗离屏画布 = 一颗能装 `Sisf2DContext` 的缓冲，而不是一个 DOM**。
    ///
    /// `platform.rs` 模块头那条"有意不交 `document`"的判词**今天仍然成立**，这一格就是它的边界证明：
    /// ① 只认 `'canvas'`，别的 tag **点名抛**；② `addEventListener`/`getElementById` 这些
    /// "有没有 DOM"的探测位**全部不存在**（`undefined`）⇒ 探测走的还是非浏览器那条分支；
    /// ③ `getContext('webgl')` 交 `null`（`Graphics._detectType` 要的正是这一句答 null 才落到
    /// canvas 那一条，见 `M10.md` 开工定案）；④ `getContext('2d')` 是**幂等的同一颗**，
    /// 而它画的每一笔进的是**同一条操作流**（不是另起一个只有浏览器才有的真相）。
    #[test]
    fn the_offscreen_canvas_answered_by_the_platform_is_the_one_the_tinter_writes_to() {
        let host = sprite_booted();
        let js = r#"
__sisf_paint_reset('x');
var out = [];
out.push(typeof document.addEventListener + ',' + typeof document.getElementById);
var c = document.createElement('canvas');
c.width = 5; c.height = 3;
var ctx = c.getContext('2d');
out.push(c.width + ',' + c.height + ',' + String(ctx === c.getContext('2d')));
out.push(String(c.getContext('webgl')));
ctx.fillRect(0, 0, 2, 2);
var threw = 'no';
try { document.createElement('audio'); } catch (e) { threw = /只接 canvas/.test(e.message) ? 'named' : 'other'; }
out.push(threw);
out.push(__sisf_paint_ops('x'));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "undefined,undefined|5,3,true|null|named|[\"fillRect|1|0|0|2|2|0,0,0,255|source-over\"]",
            "首段 = 两颗探测位都不在（`undefined`）⇒ 与「没有 document」同一条分支，这就是文件头第⑵条\
             存在的全部理由；第二段 = `width`/`height` 落得下来、`getContext('2d')` 是**同一颗**；\
             第三段 = `getContext('webgl')` 交 `null`（`Graphics._detectType` 要的正是这一句）；\
             第四段 = 问 `'audio'` 是**点名抛**（`named`）而不是静默给一颗 canvas；\
             末段 = 那一笔 `fillRect` 进的是**同一条操作流**（颗粒号 1，与位图那一族同一个分配器）"
        );
    }

    /// **上游那颗 `Sprite` 走完 tint 那一条腿**（`_refresh` → `_needsTint` → `_createTinter` →
    /// `_executeTint` → `texture` 换基纹理 + `_updateID++`，`rpg_core.js:4179-4218`）。
    /// 三条口径在这一格里各钉一段：① 帧的尺寸按上游那道算术 clamp 进位图（`(frameW - realX +
    /// frameX).clamp(0, bitmapW - realX)` ⇒ 样品 2×2 上 `setFrame(1,1,4,4)` 只落 **1×1**，
    /// 多出来的三像素被裁掉而不是画到别处）；② tint 走**那颗离屏缓冲**，于是
    /// `texture.baseTexture` 换成 `_tintTexture`、`_canvas` 的尺寸 = 真帧尺寸，而 `_updateID`
    /// 是**只写不读**的账（`3` = 建位图一次 + `setFrame` 一次 + `setColorTone` 一次）；
    /// ③ **合成那一格今天一条都没回放**（`applied=0`）—— 因为**没有人读那颗 tint 缓冲的像素**，
    /// 而合成是懒的（`M10.md` 事实 89）。`_executeTint` 写的六个混合值因此**还没进 `blend=[…]`**：
    /// 那一族的账要等读者出现（或绘制会话接上）才展开。`blt=3` 是三条 `blt` 操作：
    /// 位图载入时 `_createCanvas` 收尾那一笔（第四小格补的 `drawImage(this._image,0,0)`）
    /// 加 tint 缓冲里的两笔 `drawImage`（`copy` 那一支与 `destination-in` 那一支）。
    #[test]
    fn the_sprite_from_upstream_text_records_a_frame_a_tint_buffer_and_an_update_id() {
        let host = sprite_booted();
        let js = r#"
__sisf_paint_reset('x');
var bmp = Bitmap.load('img/system/Window.png');
var s = new Sprite(bmp);
s.setFrame(1, 1, 4, 4);
s.setColorTone([10, 20, 30, 0]);
var out = [];
out.push(s._realFrame.width + 'x' + s._realFrame.height + ' pivot ' + s.pivot.x + ',' + s.pivot.y);
out.push(String(s._tintTexture === s.texture.baseTexture) + ',' +
    s._canvas.width + 'x' + s._canvas.height + ',' + s.texture._updateID);
out.push(String(s.texture.frame.width) + 'x' + String(s.texture.frame.height));
out.push(s.getBlendColor().join(',') + '|' + s.getColorTone().join(','));
out.push(String(s.opacity) + ',' + String(s.width) + ',' + String(s.height));
out.push(__sisf_composite_report('x'));
var n = __sisf_paint_ops('x').split('blt|').length - 1;
out.push('blt=' + n);
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "1x1 pivot 0,0|true,1x1,3|1x1|0,0,0,0|10,20,30,0|255,4,4|\
             applied=0;not=[];blend=[];refused=0;evict=0|blt=3",
            "整颗 `Sprite` 的 tint 腿（逐段口径写在判据的头注释里）：末两段的 `applied=0` 与 `blt=3` \
             一起读才算说清这一格 —— **操作记下了三笔 blt，一笔都没回放**"
        );
    }

    /// **欠的三格逐名点名，而那两条别名的源必须在场**（切片器的第九类形状，见
    /// [`super::SPRITE_JS`] 的注释）。
    ///
    /// ⚠ 这一格里 `typeof` **会说谎**，所以点名用的是 own-ness：跳掉的那两格
    /// `_renderCanvas`/`_renderWebGL` 读出来是 `function` —— 因为
    /// `Sprite.prototype = Object.create(PIXI.Sprite.prototype)`（`:3949`）把基类那两颗**空体**
    /// 继承下来了。"这一格欠着"这件事只有 `hasOwnProperty` 说得出（第三段同时钉两格**指向同一颗**
    /// 空体：别名少了就是 `undefined`，插件日后别名它不会抛，直到喊它那一拍才炸）。
    #[test]
    fn the_sprite_render_cells_are_skipped_while_the_alias_sources_are_there() {
        let host = display_booted();
        let js = r#"
var has = function(name) {
    return Object.prototype.hasOwnProperty.call(Sprite.prototype, name);
};
var out = [];
out.push([typeof Sprite.prototype._renderCanvas, typeof Sprite.prototype._renderWebGL,
    typeof Sprite.prototype._speedUpCustomBlendModes].join(','));
out.push([has('_renderCanvas'), has('_renderWebGL'), has('_speedUpCustomBlendModes'),
    has('_renderCanvas_PIXI'), has('_renderWebGL_PIXI')].join(','));
out.push(String(Sprite.prototype._renderCanvas === PIXI.Sprite.prototype._renderCanvas) +
    ',' + String(Sprite.prototype._renderCanvas_PIXI === PIXI.Sprite.prototype._renderCanvas));
out.push(typeof Sprite.voidFilter + ',' + typeof Sprite._counter +
    ',' + String(Sprite._counter));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "function,function,undefined|false,false,false,true,true|true,true|object,number,0",
            "首段 `function,function,undefined` 就是那句谎（前两格是**继承来的空体**，第三格基类也没有）；\
             第二段才是账：三格都**不是自己的**（欠着），两条别名是自己的（交了）；\
             第三段钉两格指向**同一颗**空体 ⇒ 别名的源在场；末段是两颗静态常量随类一起交\
             （`voidFilter` 在装载那一刻被 `new`，`_counter` 是 `initialize` 的 `spriteId` 底数）"
        );
    }

    /// **`ScreenSprite` 的那颗 `PIXI.Graphics` 是记录，不是绘制**（第三格的第一条腿）。
    ///
    /// `setColor`（`rpg_core.js:6232-6261`）在**颜色真变的那一拍**才重画，而且一画就是**整块屏再乘五**
    /// （`-Graphics.width*5, -Graphics.height*5, width*10, height*10` —— 上游那句注释
    /// "whole screen with zoom"）。这一格钉四段：① 那颗 `Container` 底真的接住了孩子
    /// （`children[0] === _graphics`）；② `opacity` 那一对访问器过的是**底上的 `alpha`**
    /// （`initialize` 里 `this.opacity = 0` ⇒ `alpha` 落 0，读回来还是 0）；③ 记录里就是**一块**
    /// quad，`fillColor` 是 `(r<<16)|(g<<8)|b` 那颗整数，尺寸从 `Graphics.width/height` 算出来
    /// （夹具按宿主的写法把 `_width`/`_height` 落成上游那份工程跑起来的 **1104×736**，
    /// `AGENTS.md` 事实卡第一条 —— YEP 覆盖的那一对参数，不是这一格的账，只是它得是个数）；
    /// ④ 同色再喊一次**不落新的记录**
    /// （那道 `if (this._red !== r || …)` 的闸 —— 幕每一拍都在喊 `setColor`，静默追加就是每拍一块 quad）。
    #[test]
    fn the_screen_sprite_records_one_whole_screen_quad_and_repeats_cost_nothing() {
        let host = screen_tone_booted();
        let js = r#"
// 宿主写屏幕尺寸那一手（`core_graphics.js` 文件头那条纪律：字段直写，不走 setter）
Graphics._width = 1104; Graphics._height = 736;
var s = new ScreenSprite();
var out = [];
out.push(String(s.children.length) + ',' + String(s.children[0] === s._graphics) +
    ',' + String(s.opacity));
var d = s._graphics.graphicsData;
out.push(d.length + '|' + d[0].type + '|' + d[0].fillColor + '|' + d[0].fillAlpha +
    '|' + s._colorText);
out.push(d[0].shape.x + ',' + d[0].shape.y + ',' + d[0].shape.width + 'x' +
    d[0].shape.height);
s.setWhite();
out.push(String(s._graphics.graphicsData[0].fillColor) + '|' + s._colorText);
s.setWhite();
out.push('repeat=' + s._graphics.graphicsData.length);
out.push(String(s.scale.x) + ',' + String(s.children[0].parent === s));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "1,true,0|1|RECT|0|1|rgb(0,0,0)|-5520,-3680,11040x7360|16777215|rgb(255,255,255)|\
             repeat=1|1,true",
            "末段那 `1` 是这一格最要紧的一段：**同色再喊不落新记录**；`1,true` 是 `scale` 那格\
             在底上（值还是初值 1）与 `addChild` 认下了父亲"
        );
    }

    /// **`anchor` 那一格是 YEP 的废弃路径，而它自己写 `this.scale`** ⇒ 底必须答 `scale`；
    /// `warnYep` 一辈子只记一条账（`YEPWarned` 那道闸），`blendMode` 那一对**自己写自己读**。
    ///
    /// ⚠ `blendMode` 在**没人写过之前**读回来必须是 `undefined`：底**不给**它初值 —— 上游 PIXI 给的是
    /// `'normal'`，而真流里 0 个写点（尺子 `tone-and-graphics.py` 第①段），给了就是把上游读到的
    /// `undefined` 改成一个数（同 `viewport` 那一条纪律）。
    #[test]
    fn the_screen_sprite_anchor_walks_the_deprecation_path_and_logs_once() {
        let host = screen_tone_booted();
        let js = r#"
var s = new ScreenSprite();
var out = [];
out.push(String(s.blendMode));
var before = __sisf_console.length;
var a = s.anchor;
out.push(a.x + ',' + a.y + '|' + String(s.scale.x) + ',' + String(s.scale.y) + '|' +
    (__sisf_console.length - before) + '|' + String(ScreenSprite.YEPWarned));
var b = s.anchor;
out.push((__sisf_console.length - before) + '|' + String(b === a));
s.blendMode = 3;
out.push(String(s.blendMode) + ',' + String(s._graphics.blendMode));
s.anchor = 255;
out.push(String(s.opacity));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "undefined|0,0|1,1|1|true|1|false|3,3|255",
            "首段 `undefined` 就是「底不给初值」那一条的读数；第三段那条账**只涨一次**\
             （`1` 而不是 `2`）就是 `YEPWarned` 那道闸；`String(b === a)` 是 `false` —— 上游那格\
             每次 `return` 一颗**新的** `{{x:0,y:0}}`，别把它当缓存；`anchor = 255` 那个 setter 写的是\
             `alpha`（上游就是这么写的，看着像笔误也不能改）"
        );
    }

    /// **`ToneSprite` 是 canvas 那一条支的状态，而它跳掉的两格在底上没有对应物**。
    ///
    /// 与 `Sprite` 那一格的**关键差别**：`Sprite` 跳掉的 `_renderCanvas`/`_renderWebGL` 读出来是
    /// `function`（基类那两颗空体被继承，事实 96 那句谎），而 `ToneSprite` 挂在 `PIXI.Container` 上，
    /// 底上**没有** `_renderCanvas` ⇒ 这里 `typeof` 与 own-ness 一起读都是"没有"。
    /// 两种谎与不谎都钉住，是为了下一次改底的时候这条判据会红。
    /// 状态那一段是上游的四道 clamp（`setTone` 的 `(-255,255)` 与 gray 的 `(0,255)`，`:7536-7545`）。
    #[test]
    fn the_tone_sprite_state_is_the_canvas_branch_and_its_render_cells_are_undefined() {
        let host = screen_tone_booted();
        let js = r#"
var t = new ToneSprite();
var has = function(name) {
    return Object.prototype.hasOwnProperty.call(ToneSprite.prototype, name);
};
var out = [];
out.push(t._red + ',' + t._green + ',' + t._blue + ',' + t._gray);
t.setTone(300, -50, 0.6, 999);
out.push(t._red + ',' + t._green + ',' + t._blue + ',' + t._gray);
t.clear();
out.push(t._red + ',' + t._green + ',' + t._blue + ',' + t._gray);
out.push([typeof ToneSprite.prototype._renderCanvas,
    typeof ToneSprite.prototype._renderWebGL].join(','));
out.push([has('_renderCanvas'), has('_renderWebGL'), has('setTone'), has('initialize'),
    has('clear')].join(','));
out.push(String(t.children.length) + ',' + String(t.visible) + ',' + String(t.parent));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "0,0,0,0|255,-50,1,255|0,0,0,0|undefined,undefined|false,false,true,true,true|0,true,null",
            "第二段是四道 clamp 的读数（`300→255`、`0.6→Math.round→1`、`999→255`）；\
             第三段那两个 `undefined` 与 `Sprite` 那一格的两个 `function` 是同一条腿的两头 —— \
             own-ness 那一段（三个 `false`/两个 `true`）才是账"
        );
    }

    /// **webgl 那一支今天走不到，而 `ToneFilter` 依然在场**（待定项 2 的"接满闭包、不记降级"）。
    ///
    /// 死路是量出来并且**在这一格里钉住**的（尺子 `tone-and-graphics.py` 第②段）：色调有两只换法，
    /// `createToneChanger`/`updateTone` 都按 `Graphics.isWebGL()` 分岔，而 `isWebGL` 与 `hasWebGL`
    /// 那两格今天**一起随 `--extra` 交出**：`hasWebGL`（`:1910`）问的就是
    /// `document.createElement('canvas').getContext('webgl')` —— 那颗假画布对 `'2d'` 之外每一种都回
    /// `null`（事实 97 第⑶条）⇒ 它是**探测的真答案** `false`；`isWebGL`（`:1898`）第一句短路在
    /// `this._renderer` 上，而那一格要等绘制会话才有人写 ⇒ 读出来是 `undefined`。
    /// **两个都是假值** ⇒ 走的就是上面那条 `ToneSprite` 的腿（第三段 `!!` 钉这一句）。
    ///
    /// 第二、三段是那颗底的**记请求，不折矩阵**：`adjustTone` 的矩阵按上游那二十格**原样**存下来
    /// —— 偏移在 3/8/13（每行第 4 格，**不是** PIXI 那 5x4 布局的偏移列 4/9/14/19），而且是
    /// `/255` 之后的值（`rpg_core.js:7479-7496` 自己就这么写的）。移植侧一个字都不改：
    /// 这一族的矩阵只有 WebGL 着色器读，而那一支恒不运行（见上），所以"看着像笔误"也不是这一格该动的地方。
    /// `hue`/`saturate`
    /// 逐名进 `colorOps` 而**不动 `matrix`** —— 矩阵的构造式住在 `libs/pixi.js`（`_multiply` +
    /// `_colorMatrix` 里那手"偏移再除 255"），而定案第 1 条不把 `pixi` 当 JS 面，而这一支的唯一
    /// 消费者是 WebGL 着色器（`uniform float m[20]`），恒不运行 ⇒ 折出来的矩阵没有任何东西能验证。
    /// 末段是 `TRP_ParticleEditor:4435-4440` 那条守卫路径（`if (PIXI.filters.ColorMatrixFilter)` ⇒
    /// 名字在场就会 `new` 它并 `saturate(-1)`）：**不许抛**，效果由 M10.6 的覆盖账点名。
    #[test]
    fn the_webgl_tone_branch_is_answered_false_while_the_filter_records_its_requests() {
        let host = screen_tone_booted();
        let js = r#"
var out = [];
out.push(String(Graphics.hasWebGL()) + ',' + String(Graphics.isWebGL()) + ',' +
    String(!!Graphics.isWebGL()));
var f = new ToneFilter();
out.push(String(f instanceof PIXI.filters.ColorMatrixFilter) + '|' + f.matrix.join(','));
f.adjustTone(100, 0, -50);
out.push(f.matrix[3] + ',' + f.matrix[8] + ',' + f.matrix[13] + '|' + f.colorOps.length);
f.adjustHue(120);
f.adjustSaturation(200);
out.push(f.colorOps[1] + ';' + f.colorOps[2]);
out.push(String(f.colorOps[0].indexOf('matrix|1|')) + '|' + f.matrix[3]);
var g = new PIXI.filters.ColorMatrixFilter();
g.saturate(-1);
out.push(g.colorOps.join(';') + ',' + g.matrix[0]);
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "false,undefined,false|true|1,0,0,0,0,0,1,0,0,0,0,0,1,0,0,0,0,0,1,0|\
             0.39215686274509803,0,-0.19607843137254902|1|hue|120|1;saturate|0.7843137254901961|1|\
             0|0.39215686274509803|saturate|-1|0,1",
            "首段是整条死路的三段读数：`hasWebGL` = **探测的真答案** `false`（那颗假画布对 `'2d` 之外
             每一种都回 `null`），`isWebGL` = `undefined`（第一句短路在还没人写的 `_renderer` 上），
             `!!` = `false` —— 分岔读的就是这最后一个假值；\
             `hue`/`saturate` 之后 `matrix[4]` 还是 `adjustTone` 留下的那个数 ⇒ **它们没折矩阵**，\
             这一格钉的就是这一句"
        );
    }

    /// **`WindowLayer` 那颗掩膜矩形就是 `PIXI.Graphics` 记录里的那一颗对象**（第四格的第一条腿）。
    ///
    /// 上游那一手是 `beginFill → drawRect(0,0,0,0) → endFill`，然后
    /// `this._windowRect = this._windowMask.graphicsData[0].shape`（`rpg_core.js:6960-6963`）——
    /// **拿到的是引用而不是副本**：跳掉的 `_maskWindow` 之后就是改写 `rect.x/y/width/height` 来挪掩膜。
    /// ⇒ 移植侧那颗 `drawRect` 交出的 `shape` 必须与记录里那一格是同一颗（第二段钉同一性，
    /// 第三段"改写之后记录里也变了"钉的是反过来那一头），第四段的 `endFill` 则钉"当前填充放下了"
    /// （`_fill` 回 `null`，后面没有 `beginFill` 的 `drawRect` 才落进默认那一支）。
    /// 首段那三格是**这颗类自己的**读数：`width`/`height` 是它自己的访问器（底上那两颗 `Sprite`
    /// 的访问器不是它的）、`_tempCanvas` 是 `null`（离屏画布要等跳掉的 `renderCanvas` 才要）、
    /// `_translationMatrix` 是那九格初值。末段是 `filters` 那一只数组与 `filterArea` 那颗空矩形。
    #[test]
    fn the_window_layer_mask_is_the_same_object_the_graphics_record_handed_out() {
        let host = display_booted();
        let js = r#"
var l = new WindowLayer();
var out = [];
out.push(l.width + ',' + l.height + '|' + String(l._tempCanvas) + '|' +
    l._translationMatrix.join('.'));
out.push(String(l._windowRect === l._windowMask.graphicsData[0].shape));
l._windowRect.width = 40;
out.push('after=' + l._windowMask.graphicsData[0].shape.width +
    ',' + l._windowMask.graphicsData[0].type);
out.push(String(l._windowMask._fill) + '|' + l._windowMask.graphicsData.length);
out.push(l.filters.length + ',' + String(l.filters[0] instanceof PIXI.filters.VoidFilter) +
    ',' + (l.filterArea.width + 'x' + l.filterArea.height) + ',' +
    String(l.filterArea instanceof PIXI.Rectangle));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "0,0|null|1.0.0.0.1.0.0.0.1|true|after=40,RECT|null|1|1,true,0x0,true",
            "`after=40` 是这一格最要紧的一段：改写 `_windowRect` 之后**记录里那一格跟着变** ⇒ 拿到的是\
             引用；`null`（`_fill`）是 `endFill` 放下了当前填充；`0x0,true` 是 `filterArea` 那颗\
             `new PIXI.Rectangle()` 的默认四格（上游 `:6968` 就是这么建的）"
        );
    }

    /// **`move`/`update`/`onRemoveAsAChild` 那三格的语义**（第四格的第二条腿）。
    ///
    /// `update` 是"孩子里有 `update` 才喊"（`rpg_core.js:7022-7028`）—— 底上那三格空体的孩子
    /// 没有 `update`，所以它必须**跳过而不是抛**；`move` 四格一起落（`width`/`height` 走这颗类
    /// 自己的访问器，落进 `_width`/`_height`，不是底上的字段）；`onRemoveAsAChild` 是"把孩子们全摘掉"，
    /// 而它挂的那条 `removed` 事件**我们这一层收下并忘掉**（`PIXI.Container.prototype.on` 是空体，
    /// 理由写在 `shim_display.js` 文件头）⇒ 这段只能自己喊，摘完之后孩子的 `parent` 回 `null`。
    #[test]
    fn the_window_layer_update_skips_children_without_update_and_move_writes_its_own_accessors() {
        let host = display_booted();
        let js = r#"
var l = new WindowLayer();
var ticks = 0;
var win = { _isWindow: true, update: function() { ticks += 1; } };
var plain = { x: 1 };
l.addChild(win);
l.addChild(plain);
l.move(10, 20, 100, 50);
var out = [];
out.push(l.x + ',' + l.y + ',' + l.width + 'x' + l.height + '|' + l._width + 'x' + l._height);
l.update();
out.push('ticks=' + ticks + ',' + l.children.length);
l.onRemoveAsAChild();
out.push('after=' + l.children.length + ',' + String(plain.parent));
out.push(String(l.children.length === 0) + ',' + typeof PIXI.Container.prototype.on);
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "10,20,100x50|100x50|ticks=1,2|after=0,null|true,function",
            "首段那两对（`100x50|100x50`）钉的是访问器落在**自己的** `_width`/`_height` 上；\
             `ticks=1,2` 是「两个孩子里只有一个有 `update`」—— 静默跳过而不是抛，就是这一段的账；\
             末段 `function` 是那颗 `on` 在场（它收下并忘掉 ⇒ `onRemoveAsAChild` 今天只能被自己喊）"
        );
    }

    /// **跳掉的四格逐名点名，而 `PIXI.Graphics` 那两名还欠着**（第四格的第三条腿）。
    ///
    /// 与 `Sprite` 那一格同一条谎：`renderCanvas`/`renderWebGL` 从底上那两颗**继承**来的空体
    /// 读出来是 `function`（`M10.md` 事实 96 在这一颗上的第二次应用），own-ness 才点得出欠着；
    /// `_canvasClearWindowRect`/`_maskWindow` 底上没有同名格 ⇒ `typeof` 就是 `undefined`。
    /// 那四格里只有 `_maskWindow` 读 `_currentBounds`/`boundsDirty`（`rpg_core.js:7163-7164`）
    /// ⇒ 那两名今天**不给**（首段末两格钉的就是"它们还是 `undefined`"），随「宿主那侧的真绘制」一起加。
    #[test]
    fn the_window_layer_render_session_cells_are_skipped_and_the_mask_flags_are_still_owed() {
        let host = display_booted();
        let js = r#"
var has = function(name) {
    return Object.prototype.hasOwnProperty.call(WindowLayer.prototype, name);
};
var out = [];
out.push([typeof WindowLayer.prototype.renderCanvas,
    typeof WindowLayer.prototype.renderWebGL,
    typeof WindowLayer.prototype._canvasClearWindowRect,
    typeof WindowLayer.prototype._maskWindow].join(','));
out.push([has('renderCanvas'), has('renderWebGL'), has('_canvasClearWindowRect'),
    has('_maskWindow'), has('initialize'), has('move'), has('update'),
    has('onRemoveAsAChild')].join(','));
var l = new WindowLayer();
out.push(String(l._windowMask._currentBounds) + ',' + String(l._windowMask.boundsDirty));
out.push(String(WindowLayer.voidFilter instanceof PIXI.filters.VoidFilter) + ',' +
    String(l.filters[0] === WindowLayer.voidFilter));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "function,function,undefined,undefined|false,false,false,false,true,true,true,true|\
             undefined,undefined|true,true",
            "首段那两个 `function` 是谎（继承来的空体），第二段那四个 `false` 才是账（四格都欠着），\
             而后四个 `true` 钉「这一颗自己的四格都交了」；第三段那两个 `undefined` 就是还欠着的\
             `PIXI.Graphics` 那两名 —— 它们的读者在被跳掉的 `_maskWindow` 里"
        );
    }

    /// **`Window` 那一颗整棵部件树挂对地方**（第五格的第一条腿）。
    ///
    /// `_createAllParts`（`rpg_core.js:6623-6642`）交出的形状是**两层**：顶上六只孩子
    /// （`_windowSpriteContainer`/光标/内容/两只箭/暂停标记），而窗皮与框在那只容器**里面**两只 ——
    /// 这就是咬人第 25 条⑴那一条腿的上游侧（引擎里 `layer` 说话算，而这里靠**挂的位置**说话）。
    /// `addChildToBack` 那一格钉的是"插在容器之后一格"（`:6602-6605` 用的是 `indexOf` + 1，
    /// 不是插到最前），而末段那颗 `alpha` 是 `192/255`（窗背板那一档，`M10.md` 咬人第 29 条⑸
    /// 的线性光差就是从这里来的）。
    #[test]
    fn the_window_parts_tree_is_the_measured_shape() {
        let host = sprite_booted();
        let js = r#"
var w = new Window();
var out = [];
out.push(w.children.length + ',' + w._windowSpriteContainer.children.length + ',' +
    String(w.children[0] === w._windowSpriteContainer) + ',' +
    String(w._windowSpriteContainer.children[0] === w._windowBackSprite));
var extra = new PIXI.Container();
w.addChildToBack(extra);
out.push('back=' + w.children.indexOf(extra) + ',' + String(extra.parent === w));
out.push(String(w._isWindow) + ',' + String(w._openness) + ',' +
    String(w._cursorRect instanceof Rectangle) + ',' + String(w.origin instanceof Point));
out.push(String(w.active) + ',' + String(w.pause) + ',' + String(w.downArrowVisible) + ',' +
    String(w._windowBackSprite.alpha) + ',' + String(w._windowBackSprite.bitmap.width));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "6,2,true,true|back=1,true|true,255,true,true|true,false,false,0.7529411764705882,1",
            "`back=1` 那一段是这一格唯一一处**位置**读数：插在 `_windowSpriteContainer`（第 0 格）\
             之后一格，而不是被 `addChild` 甩到末尾；`0.7529411764705882` 就是 `192/255`"
        );
    }

    /// **`Window` 那六格 `_refresh*` 画的是位图操作，而操作流里第一次有窗皮那十几笔**（第五格的第二条腿）。
    ///
    /// 这一格之所以能**整颗交出去**（不像前三颗要跳渲染会话）就是因为六格走的都是 `Bitmap.blt`
    /// （尺子 `window-face.py` 第⑤段：`_refreshBack` 2 处 / `_refreshFrame` 8 / `_refreshCursor` 9 的
    /// **文本处数**，另外三格 0 处）。`move` 一落 ⇒ `_refreshAllParts` 跑遍六格，而窗皮那一颗
    /// 是 `Bitmap.load` 的真位图 ⇒ 九笔框 + 背板那两笔落进操作流。
    /// ⚠ 末段那条 `applied=0` 与前面的 `blt=` 必须**一起读**：合成是懒的，今天没人读像素
    /// （`M10.md` 事实 98 在这一颗上的第二次应用 —— 操作记下了，一笔都没回放）。
    #[test]
    fn the_window_refresh_cells_land_their_skin_blts_in_the_op_stream() {
        let host = sprite_booted();
        let js = r#"
__sisf_paint_reset('x'); Sisf2DContext._lastSid = 0;
var w = new Window();
// 窗皮用**够大的那一颗**：上游 `Bitmap.blt` 的第一道闸是 `sx + sw <= source.width`
// （`rpg_core.js:1188-1190`），夹具那颗 2×2 的样品图过不了 ⇒ 换一颗 192×192 的（真窗皮就是 192 见方）
w.windowskin = new Bitmap(192, 192);
var ops = function() { return JSON.parse(__sisf_paint_ops('x')); };
var bltCount = function(list) {
    return list.filter(function(o) { return o.indexOf('blt|') === 0; }).length;
};
var before = ops();
w.move(0, 0, 240, 120);
var after = ops();
var out = [];
out.push('all=' + before.length + '->' + after.length + ' blt=' +
    bltCount(before) + '->' + bltCount(after));
out.push(after.slice(before.length).map(function(o) { return o.split('|')[0]; }).join(','));
out.push(after.slice(before.length, before.length + 2).join(' ;; '));
out.push(w._windowBackSprite.bitmap.width + 'x' + w._windowBackSprite.bitmap.height + '|' +
    w._windowFrameSprite.bitmap.width + 'x' + w._windowFrameSprite.bitmap.height + '|' +
    String(w._windowBackSprite._frame.width) + ',' + String(w._windowBackSprite.y));
out.push(String(w._downArrowSprite.anchor.x) + ',' + String(w._downArrowSprite.anchor.y) + '|' +
    String(w._windowPauseSignSprite.anchor.y));
out.push(String(w._windowContentsSprite.bitmap));
out.push(__sisf_composite_report('x'));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "all=10->31 blt=0->15|size,size,blt,blt,blt,blt,blt,blt,blt,size,size,blt,blt,blt,\
             blt,blt,blt,blt,blt,size,size|size|6|232|1| ;; size|6|232|112||232x112|240x120|232,4|\
             0.5,0.5|1|undefined|applied=0;not=[];blend=[];refused=0;evict=0",
            "首段那 `blt=0->15` 是这一格最要紧的一段：**窗皮那十几笔第一次落进操作流**（背板 `1 + 2×3` \
             = 7 笔、框 8 笔；光标那 9 笔一笔都没落，因为 `_cursorRect` 还是 `0×0` 而过不了 \
             `w > 0 && h > 0` 那道闸 —— 上游的闸，不是移植侧漏的）；第二段那 21 个动作名是\
             `size`/`blt` 的**次序**（每颗新位图先落两颗 `size`：`new Bitmap(w, 1)` 再 `resize` 到 `h`）；\
             第三段那两格 `size|6|232|1| ;; size|6|232|112|` 钉的就是那一手（末尾那一根竖线是 `setSize`\
             的第五格 —— 画出来的那一张**没有 URL**，`encodeURIComponent('')` 交出空串），`6` 是它自己的\
             位图号；\
             `232x112` 是 `w - margin*2`（`_margin` 那一格 4）而框那颗是整颗尺寸；`0.5,0.5|1` 是\
             `anchor` 那颗**在场且被改写**的对象（底给的初值是 `0,0`，这一格唯一的新底）；\
             `undefined` 是内容那颗位图**从没被写过**（`_refreshContents` 只 `move`，画内容由 `Window_Base`\
             那一族来）；末段 `applied=0` 与前面的 15 笔必须**一起读**：操作记下了、一笔都没回放"
        );
    }

    /// **`openness` 走的是底上那颗 `scale`，`opacity` 那三格走的是 `alpha`**（第五格的第三条腿）。
    ///
    /// 第三格加 `scale` 那一条纪律在这里第二次兑现：`openness` 的 setter（`:6495-6496`）写
    /// `_windowSpriteContainer.scale.y` 与 `.y`，而底不给 `scale` 就是当场抛。
    /// ⚠ `isOpen`/`isClosed` 比的是 `_openness` 那**一个数**（`>= 255` / `<= 0`），不是 `scale.y` ——
    /// 中间那一档（128）两只都必须 `false`，写成"看 scale"就反了。
    #[test]
    fn the_window_openness_walks_the_container_scale_while_opacity_walks_alpha() {
        let host = sprite_booted();
        let js = r#"
var w = new Window();
w.move(0, 0, 200, 100);
var out = [];
out.push(String(w.isOpen()) + ',' + String(w.isClosed()));
w.openness = 128;
out.push(w._windowSpriteContainer.scale.y + ',' + w._windowSpriteContainer.y + '|' +
    String(w.isOpen()) + ',' + String(w.isClosed()));
w.openness = 128;
out.push('same=' + w._windowSpriteContainer.scale.y);
w.openness = 300;
out.push(String(w._openness) + ',' + String(w.isOpen()));
w.openness = 0;
out.push(String(w.isClosed()) + ',' + w._windowSpriteContainer.y);
w.opacity = 200;
w.backOpacity = 128;
w.contentsOpacity = 64;
out.push(w._windowSpriteContainer.alpha + ',' + w._windowBackSprite.alpha + ',' +
    w._windowContentsSprite.alpha + ',' + String(w.opacity) + ',' + String(w.contentsOpacity));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "true,false|0.5019607843137255,24.901960784313726|false,false|\
             same=0.5019607843137255|255,true|true,50|\
             0.7843137254901961,0.5019607843137255,0.25098039215686274,200,64",
            "`same=` 那一段钉的是上游那道 `if (this._openness !== value)` 的闸（同值再写一次\
             一个数都不动）；`300` 被 clamp 回 255；末段那对 `200,64` 是**读回来还是写进去的那个数**\
             （`opacity` 那一对是 `alpha * 255` 的双向换算，中间不夹）"
        );
    }

    /// **光标那 40 拍的闪烁、两只箭与暂停标记**（第五格的第四条腿：`_update*` 那四格的状态机）。
    ///
    /// `_updateCursor`（`:6810-6822`）的账是 `(_animationCount % 40)` 那一支：**前 20 拍每拍减 8，
    /// 后 20 拍按 `(40 - n)` 加回来**，而 `active` 为假时整条闪烁不跑（恒 `contentsOpacity`）。
    /// `_updatePauseSign`（`:6852-6866`）那支 `alpha` 是 `Math.min(a + 0.1, 1)` 的渐显，
    /// 帧号取 `_animationCount / 16` 的两只奇偶位（`sx + x*p`）—— 这四个数都是**看得见**的节奏，
    /// 少一拍就是"窗在抖"。`_updateContents` 那一格在 `w`/`h` 不为正时要**藏起来**而不是画零宽。
    #[test]
    fn the_window_blink_and_pause_sign_state_machine() {
        let host = sprite_booted();
        let js = r#"
var w = new Window();
w.move(0, 0, 200, 100);
w.contentsOpacity = 255;
var out = [];
[0, 5, 10, 20, 30, 39].forEach(function(n) {
    w._animationCount = n;
    w._updateCursor();
    out.push(n + ':' + Math.round(w._windowCursorSprite.alpha * 255));
});
w.active = false;
w._animationCount = 10;
w._updateCursor();
out.push('inactive:' + Math.round(w._windowCursorSprite.alpha * 255));
w.active = true;
w.downArrowVisible = true;
w._updateArrows();
out.push('arrows=' + String(w._downArrowSprite.visible) + ',' + String(w._upArrowSprite.visible));
w.openness = 0;
w._updateArrows();
out.push('closed-arrows=' + String(w._downArrowSprite.visible));
w.openness = 255;
w.pause = false;
w._animationCount = 0;
w._updatePauseSign();
out.push('pause-off=' + String(w._windowPauseSignSprite.alpha) + ',' +
    String(w._windowPauseSignSprite.visible));
w.pause = true;
[0, 1, 2, 12].forEach(function(n) {
    w._animationCount = n;
    w._updatePauseSign();
});
out.push('pause-on=' + Math.round(w._windowPauseSignSprite.alpha * 10) / 10 + ',' +
    w._windowPauseSignSprite._frame.x + ',' + w._windowPauseSignSprite._frame.y);
w._animationCount = 16;
w._updatePauseSign();
out.push('frame=' + w._windowPauseSignSprite._frame.x + ',' + w._windowPauseSignSprite._frame.y);
w.move(0, 0, 0, 0);
w._updateContents();
out.push('no-size=' + String(w._windowContentsSprite.visible));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "0:255|5:215|10:175|20:95|30:175|39:247|inactive:255|arrows=true,false|\
             closed-arrows=false|pause-off=0,true|pause-on=0.4,144,96|frame=168,96|no-size=false",
            "`20:95` 是那一支转折的那一格（第 20 拍已经落进 `else`）；`pause-on` 那三个数是\
             四拍渐显之后（0.1×4 clamp 到 1）与帧号（`_animationCount` 12 ⇒ x 位 0、y 位 1 ⇒ `144,120`）"
        );
    }

    /// **`Window` 这一颗的成员面就是上游那一整颗**（第五格的第五条腿：容器树上第一次**没有欠账**）。
    ///
    /// 前三颗都要 `--skip` 掉渲染会话那一族，而这一颗 22 条方法 + 10 块访问器一条不落全交
    /// （尺子 `window-face.py` 第①段量的就是这一份名单）⇒ 这一格钉的是"**一名都不许少**"：
    /// 将来有人为了省事跳一格，这一条会红，而逐字尺子那一份管的是"交出去的字有没有跑"。
    /// ⚠ 钉法有两条：⑴ 用**双向缺与多**而不是排序串（事实 99 那条引擎次序的教训这里同样成立）；
    /// ⑵ 点名必须用 `getOwnPropertyNames` 而**不是** `for-in` —— 上游那 10 块访问器的描述符只写
    /// `configurable: true`（`rpg_core.js:6337-6500` 那十块一条都没写 `enumerable`）⇒ 它们
    /// **不可枚举**，`for-in` 只看得见那 22 条方法（首段那一名字面上的 `enumerable` 就是这一格）。
    #[test]
    fn the_window_member_face_is_the_whole_upstream_class() {
        let host = sprite_booted();
        let js = r#"
var want = ['initialize', 'update', 'move', 'isOpen', 'isClosed', 'setCursorRect', 'setTone',
    'addChildToBack', 'updateTransform', '_createAllParts', '_onWindowskinLoad',
    '_refreshAllParts', '_refreshBack', '_refreshFrame', '_refreshCursor', '_refreshContents',
    '_refreshArrows', '_refreshPauseSign', '_updateCursor', '_updateContents', '_updateArrows',
    '_updatePauseSign', 'windowskin', 'contents', 'width', 'height', 'padding', 'margin',
    'opacity', 'backOpacity', 'contentsOpacity', 'openness'];
var own = Object.getOwnPropertyNames(Window.prototype);
var enumerable = [];
for (var k in Window.prototype) {
    if (Object.prototype.hasOwnProperty.call(Window.prototype, k)) enumerable.push(k);
}
var missing = want.filter(function(n) { return own.indexOf(n) < 0; });
var extra = own.filter(function(n) { return n !== 'constructor' && want.indexOf(n) < 0; });
[missing.join(','), extra.join(','), own.length,
    enumerable.length].join('|')
"#;
        assert_eq!(
            num(&host, js),
            "||33|23",
            "两个空串就是「一名不缺、一名不多」；`33` = 上游那 **32** 格（22 条方法 + 10 块访问器）\
             再加切片器随类交的 `constructor` 补句那一名；末段那个 `23` 是 `for-in` 看得见的\
             那一族（22 条方法 + `constructor`）——**十块访问器不可枚举**，写成 33 就是把上游的描述符改了"
        );
    }

    /// **`Weather` 建立那一刻落下的三颗位图、那块"十倍屏"的暗幕，与那九笔操作**（第二小格第一条）。
    ///
    /// 读数全部来自 `Graphics._width/_height = 1104×736` 那一档（真虚拟屏）：
    /// ⑴ 三颗位图的尺寸是**写死的** —— 雨 `1×60`、风暴 `2×100`、雪 `9×9`（`rpg_core.js:7295-7303`），
    /// 而每一颗建立那一刻各落**两颗 `size`**（事实 108 在这一颗上是**六笔**）；
    /// ⑵ 那三笔"画"就是尺子第⑤段量的 `fillAll` **2** 处 + `drawCircle` **1** 处 ⇒ 落成
    /// `fillRect`×2（`255,255,255,255` = `'white'`）与 `fillArc`×1（半径 4 落在 `(4,4)`）；
    /// ⑶ 暗幕那颗是**上游的 `ScreenSprite`**，它的 `graphicsData[0].shape` 是
    /// `-5520,-3680,11040,7360` = **十倍的屏**（`ScreenSprite._update` 那一格的 `width * 10`）——
    /// 第三格量"整屏再乘五"时钉的就是同一只手，这一格第一次从**读者那一边**看到它；
    /// ⑷ 建立那一刻 `opacity`/`alpha` 都还是 `0`（`_createDimmer` 只 `setColor(80,80,80)`，
    /// 暗幕的强度要等 `update()` 里 `_updateDimmer` 那一拍才写）。
    #[test]
    fn the_weather_starts_with_three_bitmaps_and_a_dimmer_that_covers_ten_screens() {
        let host = tilemap_booted();
        let js = r#"
__sisf_paint_reset('x'); Sisf2DContext._lastSid = 0;
Graphics._width = 1104; Graphics._height = 736;
var w = new Weather();
var out = [];
out.push(w._width + ',' + w._height + '|' + w.children.length + '|' + w._sprites.length + '|' +
    w.type + ',' + w.power + '|' + String(w.origin.x) + ',' + String(w.origin.y));
out.push(w._rainBitmap.width + 'x' + w._rainBitmap.height + '|' +
    w._stormBitmap.width + 'x' + w._stormBitmap.height + '|' +
    w._snowBitmap.width + 'x' + w._snowBitmap.height);
out.push(String(w._dimmerSprite.constructor === ScreenSprite) + '|opacity=' +
    w._dimmerSprite.opacity + '|alpha=' + w._dimmerSprite.alpha + '|quad=' +
    w._dimmerSprite._graphics.graphicsData.length + '|shape=' +
    JSON.stringify(w._dimmerSprite._graphics.graphicsData[0].shape));
out.push(JSON.parse(__sisf_paint_ops('x')).join(' ;; '));
out.join('\n')
"#;
        assert_eq!(
            num(&host, js),
            "1104,736|1|0|none,0|0,0\n1x60|2x100|9x9\ntrue|opacity=0|alpha=0|quad=1|shape=\
             {\"x\":-5520,\"y\":-3680,\"width\":11040,\"height\":7360}\n\
             size|1|1|1| ;; size|1|1|60| ;; fillRect|1|0|0|1|60|255,255,255,255|source-over ;; \
             size|2|2|1| ;; size|2|2|100| ;; fillRect|2|0|0|2|100|255,255,255,255|source-over ;; \
             size|3|9|1| ;; size|3|9|9| ;; fillArc|3|4|4|4|255,255,255,255|source-over",
            "第二段那三个尺寸是**上游写死的**（`7295-7303`），不是夹具挑的；第三段那 `quad=1` 与十倍屏那一串\
             是暗幕**只记不画**的形状（`M10.md` 事实 89 那一条腿），而 `opacity=0` 说明暗幕的强度是\
             `update()` 那一拍写的；末段九笔的**次序**（每颗位图两颗 `size` 紧跟它自己的那笔画）就是\
             事实 108 在这一颗上的读数"
        );
    }

    /// **`power` 那一格同时管着两件事：粒子池的颗数（`power × 10`）与暗幕的不透明度（`power × 6`）**。
    ///
    /// `236` 指令的强度参数从 0 到 9（`AGENTS.md` 上游事实卡那一条），而上游的折法是
    /// `_updateAllSprites` 的 `maxSprites = floor(power * 10)`（`:7330`）配 `_updateDimmer` 的
    /// `opacity = floor(power * 6)`（`:7315`）⇒ `power = 3` 那一拍量到 **30 颗粒子 + 孩子 31 只**
    /// （30 + 暗幕）与暗幕 `opacity=18` / `alpha=18/255 = 0.070588…`；把 `power` 降到 1 再走一拍，
    /// `_removeSprite` 那一只手把池子缩回 **10 颗 / 孩子 11 只** ⇒ **加与减都走 `addChild`/`removeChild`**，
    /// 不是"藏起来"。
    ///
    /// ⚠ 两段是这一格里**只有读数能说清**的：⑴ 整段 `update()` 跑完之后操作流**一笔都不多**（还是那九笔）——
    /// 雨/风暴/雪那三颗位图只在建立时各画一次，之后每一拍只是**挪精灵**（`sprite.x/y`），
    /// 所以"天气在动"在操作流里根本不是笔数，而 `applied` 仍是 `0`（事实 98 的同一条腿）；
    /// ⑵ `_addSprite` 那句 `new Sprite(this.viewport)` 里那一格 **`viewport` 是 `undefined`** ——
    /// 全工程从没写过它（尺子 `container-face.py` 量的那一族，`js/shim_display.js` 的头注也点过名，
    /// 移植侧**故意不给**），于是新粒子第一次进池子时 `sprite.bitmap` 读出来是 `undefined` 而不是 `null`；
    /// 这一格不落到屏幕上（同一拍的 `_update*Sprite` 立刻重写它），但它是**上游自己的形状**。
    ///
    /// ⚠ **第三段那条随机数的钉法改过一轮**（`M10.md` 事实 117）：原来钉的是"`_sprites[0].opacity`
    /// 落在 `[160,219]`"，那是**只有重掷那一支才成立**的读数 —— 旧粒子走的是 `-= 6` 那一支
    /// （`[160,219]` 减 6 之后是 `[154,213]`，仍然 ≥ 40 所以不重掷）⇒ 实测**十轮里红一轮**。
    /// 现在钉的是两条支的**形状**：`noneReborn=20/20`（`_addSprite` 落 `opacity = 0`，同一拍
    /// `if (sprite.opacity < 40)` 必重掷）与 `rainSplit=20,10,0`（前 20 颗 = `prev - 6`、
    /// 后 10 颗 = 这一拍新建后重掷、`bad=0`）⇒ **一条读数都不再依赖随机数落在哪一档**。
    #[test]
    fn the_weather_power_walks_the_sprite_pool_and_the_dimmer_opacity() {
        let host = tilemap_booted();
        let js = r#"
__sisf_paint_reset('x'); Sisf2DContext._lastSid = 0;
Graphics._width = 1104; Graphics._height = 736;
var w = new Weather();
var out = [];
w.type = 'none';
w.power = 2;
w.update();
out.push('none2 sprites=' + w._sprites.length + '|bitmap=' + String(w._sprites[0].bitmap) +
    '|type=' + w.type);
var noneRound = 0;
for (var q = 0; q < w._sprites.length; q++) {
    if (w._sprites[q].opacity >= 160 && w._sprites[q].opacity <= 219) { noneRound++; }
}
out.push('noneReborn=' + noneRound + '/20');
var prev = w._sprites.map(function(sp) { return sp.opacity; });
w.type = 'rain';
w.power = 3;
w.update();
out.push('rain3 sprites=' + w._sprites.length + '|children=' + w.children.length + '|dimmer=' +
    w._dimmerSprite.opacity + ',' + w._dimmerSprite.alpha);
var dec = 0, reborn = 0, bad = 0;
for (var i = 0; i < w._sprites.length; i++) {
    var o = w._sprites[i].opacity;
    if (i < prev.length && o === prev[i] - 6) { dec++; }
    else if (o >= 160 && o <= 219 && prev.length <= i) { reborn++; }
    else { bad++; }
}
out.push('rainSplit=' + dec + ',' + reborn + ',' + bad);
var s = w._sprites[0];
out.push('viewport=' + String(w.viewport) + '|band=' + (s.opacity >= 40 && s.opacity <= 219));
w.power = 1;
w.update();
out.push('power1 sprites=' + w._sprites.length + '|children=' + w.children.length + '|dimmer=' +
    w._dimmerSprite.opacity);
var ops = JSON.parse(__sisf_paint_ops('x'));
out.push('ops=' + ops.length + ' names=' +
    ops.map(function(o) { return o.split('|')[0]; })
        .filter(function(v, i, a) { return a.indexOf(v) === i; }).join(','));
out.push('composite ' + __sisf_composite_report('x'));
out.join('\n')
"#;
        assert_eq!(
            num(&host, js),
            "none2 sprites=20|bitmap=undefined|type=none\nnoneReborn=20/20\n\
             rain3 sprites=30|children=31|dimmer=18,0.07058823529411765\n\
             rainSplit=20,10,0\nviewport=undefined|band=true\n\
             power1 sprites=10|children=11|dimmer=6\n\
             ops=9 names=size,fillRect,fillArc\n\
             composite applied=0;not=[];blend=[];refused=0;evict=0",
            "第一段是**最容易被写错的一档**：`type = 'none'` 那一拍池子照样按 `power × 10` 建到 20 颗、\
             暗幕照样写强度，只是每颗粒子的 `bitmap` 读出来是 `undefined`（`_updateSprite` 的 `switch`\
             三个 `case` 都不进 ⇒ 谁都不画），所以「关掉天气」在上游不是把精灵摘干净而是**没有图**；\
             第二段那 `children=31` 是 30 颗粒子加暗幕，`0.07058823529411765` 就是 `18/255`；\
             末段那 `ops=9` 必须与上面那些 `update()` **一起读**：天气在动而操作流一笔不多\
             （粒子只是挪位置，那三颗位图只在建立时各画一次）"
        );
    }

    /// **三种天气各自的"斜度 + 用哪颗位图"，以及 `origin` 只挪屏幕位置**（第二小格的第三条腿）。
    ///
    /// `rotation` 是写死的三档：雨与雪 `Math.PI / 16 = 0.19634954084936207`、风暴 `Math.PI / 8 =
    /// 0.39269908169872414`（`:7391/:7401/:7413`）；`bitmap` 那一格每拍重写一次，所以
    /// `type` 一换，池子里**已有的**粒子就跟着换图（不必重建池子）⇒ 三段读数钉的是同一颗粒子在
    /// 三种 `type` 下的 `(rotation, bitmap)` 配对。
    ///
    /// `_updateAllSprites` 收尾那两句是 `sprite.x = sprite.ax - this.origin.x`（`:7341-7342`）⇒
    /// **`origin` 改的是屏幕位置，不是粒子自己的坐标**（`ax/ay` 由 `_rebornSprite` 用
    /// `Math.randomInt(Graphics.width + 100) - 100 + origin.x` 写）。这一格因此只能用**区间**与
    /// **关系**当尺子：随机数落在 `[160,219]` 的 `opacity`、`x == ax - origin.x` 这一条恒等式，
    /// 以及"走 20 拍之后 `opacity` 仍在 `[40,219]`、`ax/ay` 一个 `NaN` 都没有"——
    /// 前者是 `_updateSprite` 那句 `if (sprite.opacity < 40) _rebornSprite` 的形状（减 6/8/3 之后
    /// 要么还 ≥ 40、要么被重掷到 `[160,219]`），后者钉的是 `Math.randomInt` 那条扩展今天**在场**
    /// （少了它 `_rebornSprite` 当场抛，`sprites.rs` 那一条腿就全塌）。
    #[test]
    fn the_weather_type_decides_rotation_and_bitmap_while_the_origin_shifts_position() {
        let host = tilemap_booted();
        let js = r#"
Graphics._width = 1104; Graphics._height = 736;
var w = new Weather();
w.type = 'rain';
w.power = 2;
w.update();
var out = [];
var s = w._sprites[0];
out.push('rain rot=' + s.rotation + '|bitmapIsRain=' + (s.bitmap === w._rainBitmap));
w.type = 'storm';
w.update();
out.push('storm rot=' + s.rotation + '|bitmapIsStorm=' + (s.bitmap === w._stormBitmap));
w.type = 'snow';
w.update();
out.push('snow rot=' + s.rotation + '|bitmapIsSnow=' + (s.bitmap === w._snowBitmap));
out.push('reused=' + (w._sprites.length) + ',' + (w.children.length - 1));
w.origin.x = 300;
w.origin.y = 120;
w.update();
var ok = true, nan = 0;
for (var i = 0; i < w._sprites.length; i++) {
    var p = w._sprites[i];
    if (p.x !== p.ax - 300 || p.y !== p.ay - 120) ok = false;
    if (isNaN(p.ax) || isNaN(p.ay)) nan++;
}
out.push('originLeg=' + ok + '|nan=' + nan);
var min = 999, max = -999;
for (var k = 0; k < 20; k++) {
    w.update();
    for (var j = 0; j < w._sprites.length; j++) {
        var o = w._sprites[j].opacity;
        if (o < min) min = o;
        if (o > max) max = o;
    }
}
out.push('twentyBeats inRange=' + (min >= 40 && max <= 219) + '|sprites=' + w._sprites.length);
out.join('\n')
"#;
        assert_eq!(
            num(&host, js),
            "rain rot=0.19634954084936207|bitmapIsRain=true\n\
             storm rot=0.39269908169872414|bitmapIsStorm=true\n\
             snow rot=0.19634954084936207|bitmapIsSnow=true\n\
             reused=20,20\noriginLeg=true|nan=0\ntwentyBeats inRange=true|sprites=20",
            "那三个 `rotation` 数是**写死的常数**（雨与雪同一档 `PI/16`，风暴翻倍成 `PI/8`），\
             而 `reused=20,20` 说的是换 `type` **不重建池子**（孩子数一直是 `power × 10`）；\
             `originLeg` 钉的是 `x == ax - origin.x` 那条恒等式对**每一颗**都成立，`nan=0` 钉的是\
             `_rebornSprite` 里那颗 `Math.randomInt` 在场；`inRange` 钉的是 20 拍之后每一颗粒子的\
             `opacity` 仍落在**上游那两个常数**之间（`< 40` 就重掷成 `160 + randomInt(60)`）"
        );
    }

    /// **`Weather` 的格面 = 上游那 13 条，一颗不多、一颗不少**（第二小格的第四条）。
    ///
    /// 与 `Window`/`Tilemap` 那两格对照着读才有意思：`Weather` **一块访问器都没有** ⇒
    /// `getOwnPropertyNames` 与 `for-in` 给出**同一个 14**（13 条方法 + `constructor` 补句）——
    /// 事实 106 那条"两个数差四名"的形状在这一颗上不存在，这正是那一条的**反面对照**。
    /// 静态那一档也一样：`Tilemap` 有 22 + 13 格，而 `Weather` **一格都没有**（`statics=0`）——
    /// 这一把同样**剔掉了引擎给的那五名**（`length`/`name`/`prototype`/`arguments`/`caller`，
    /// 事实 111：node 比 QuickJS 多两颗遗留格），否则两条腿又是两个数。
    #[test]
    fn the_weather_member_face_is_the_whole_upstream_class() {
        let host = tilemap_booted();
        let js = r#"
var want = ['initialize', 'update', '_createBitmaps', '_createDimmer', '_updateDimmer',
    '_updateAllSprites', '_addSprite', '_removeSprite', '_updateSprite', '_updateRainSprite',
    '_updateStormSprite', '_updateSnowSprite', '_rebornSprite'];
var own = Object.getOwnPropertyNames(Weather.prototype);
var enumerable = [];
for (var k in Weather.prototype) {
    if (Object.prototype.hasOwnProperty.call(Weather.prototype, k)) { enumerable.push(k); }
}
var statics = Object.getOwnPropertyNames(Weather).filter(function(n) {
    return ['length', 'name', 'prototype', 'arguments', 'caller'].indexOf(n) < 0;
});
var out = [];
out.push(want.filter(function(n) { return own.indexOf(n) < 0; }).join(','));
out.push(own.filter(function(n) { return n !== 'constructor' && want.indexOf(n) < 0; }).join(','));
out.push(own.length + '|' + enumerable.length + '|' + statics.length);
out.push(String(Weather.prototype.constructor === Weather) + '|' +
    (Object.getPrototypeOf(Weather.prototype) === PIXI.Container.prototype) + '|' +
    String(new Weather() instanceof PIXI.Container));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "||14|14|0|true|true|true",
            "两个空串 = 一名不缺、一名不多；那**两个 14 相等**是这一颗自己的形状（没有访问器，\
             所以没有不可枚举的那一族）；末段三连是链的三格（`constructor` 补句、`prototype` 挂在\
             `PIXI.Container.prototype` 上、建出来的那颗 `instanceof` 那颗底）"
        );
    }

    /// **`Tilemap` 的树形、层位图那四笔 `size`，与"重建不摘旧孩子"那一手**（第五小格第六格第一小格）。
    ///
    /// 夹具把 `Graphics._width`/`_height` 摆成**真虚拟屏 1104×736**（`AGENTS.md` 上游事实卡第一条 =
    /// 启用的 `YEP_CoreEngine` 那两个参数）⇒ 量到的就是 `Spriteset_Map` 真会拿到的那几个数：
    /// 画面 `_width`/`_height` = **1144×776**（虚拟屏 + `margin` 20 各两边），层位图 **1200×864**
    /// （`ceil(1144/48)+1 = 25` 列 × `ceil(776/48)+1 = 18` 行，再各乘 48）。
    ///
    /// ⚠ `_lowerLayer.width` 那两格是 **`0,0`，不是笔误**：上游 `Sprite.prototype.move` 的签名只有
    /// `(x, y)`（`js/core_sprite.js:104`），而 `_createLayers` 喊的是 `move(-margin, -margin, width, height)`
    /// （`rpg_core.js:4791`）⇒ 后两参落空，尺寸那一族要等 `_updateLayerPositions` 里那句 `setFrame`
    /// 才落（同事实 106 那一族：问错那一格就读成"没画"）。
    ///
    /// 末两段是四格访问器那一条腿的两档：`tileWidth = 32` 值不同 ⇒ 重建，而重建走 `_createLayers`，
    /// 它**只 `addChild` 新的两层，不摘旧的**（`:4803-4804`）⇒ `children` 从 2 **累到 4**、旧层位图留在原地
    /// （操作流里也因此多两颗 `size`：`ops=8`）；再写一次同值（`width = 1144`）则一格都不动。
    /// 两档都是上游的形状 —— **照交，不"顺手修成"替换**。
    #[test]
    fn the_tilemap_layer_tree_is_the_measured_shape() {
        let host = tilemap_booted();
        let js = r#"
__sisf_paint_reset('x'); Sisf2DContext._lastSid = 0;
Graphics._width = 1104; Graphics._height = 736;
var t = new Tilemap();
var out = [];
out.push(t._width + ',' + t._height + '|' + t._layerWidth + 'x' + t._layerHeight);
out.push(t.children.length + '|' + t._lowerLayer.z + ',' + t._upperLayer.z + '|' +
    t._lowerLayer.children.length + ',' + t._upperLayer.children.length + '|' +
    t._lowerLayer.x + ',' + t._lowerLayer.y + '|' + t._lowerLayer.width + ',' + t._lowerLayer.height);
out.push(String(t.origin.x) + ',' + String(t.origin.y) + '|' + t.bitmaps.length + ',' + t.flags.length +
    ',' + t.animationCount + ',' + String(t.animationFrame) + '|' +
    String(t.horizontalWrap) + ',' + String(t.verticalWrap));
out.push(JSON.parse(__sisf_paint_ops('x')).map(function(o) { return o.split('|')[0]; }).join(','));
t.tileWidth = 32;
out.push('tw32=' + t._layerWidth + 'x' + t._layerHeight + ',children=' + t.children.length +
    ',ops=' + JSON.parse(__sisf_paint_ops('x')).length);
t.width = t._width;
out.push('same=' + t.width + ',children=' + t.children.length);
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "1144,776|1200x864|2|0,4|4,4|-20,-20|0,0|0,0|0,0,0,undefined|false,false|\
             size,size,size,size|tw32=1184x864,children=4,ops=8|same=1144,children=4",
            "第三段那 `0,0,0,undefined` 是 `bitmaps`/`flags`/`animationCount` 三颗初值加\
             **`animationFrame` 根本不在初值里**（上游 `initialize` 只写 `animationCount = 0`，\
             `rpg_core.js:4574`）—— 这一格在下面那条水面的判据里是主角；第四段那四笔 `size` = \
             两颗层位图 × 每颗两颗（事实 108 的第二次应用）"
        );
    }

    /// **`Tilemap` 的格面 = 上游那 24 条 + 4 块访问器，一张表都不许多、不许少**。
    ///
    /// 这一格是本族**最大的一份文本**（736 行），而其中 13 格静态常量的右值不是函数 ⇒
    /// `objects-verbatim.py` 比不到（同 `JsonEx` 那 2 格一条腿）⇒ 图块号那套真相只有这一条判据在数：
    /// `TILE_ID_B/A1/A2/MAX` = `0,2048,2816,8192`，三张 `*_AUTOTILE_TABLE` 的档数是
    /// **48 / 16 / 4**（水面 48 型、墙面 16 型、瀑布 4 型），而 `FLOOR_AUTOTILE_TABLE[0]` 就是
    /// `[[2,4],[1,4],[2,3],[1,3]]` 那四个**四分之一格的源坐标**（上面那条水面判据里那八个源矩形
    /// 就是从这一行折出来的）。
    ///
    /// ⚠ 点名用 `Object.getOwnPropertyNames` 而不是 `for-in`（事实 106 的第二次应用）：那四块访问器的
    /// 描述符只写 `configurable: true` ⇒ 不可枚举 ⇒ `29`（自有名）与 `25`（可枚举）差的那四名就是它们。
    /// 而**一颗函数自己的自有格，两条腿不是同一档**：`Object.getOwnPropertyNames(Tilemap).length` 在
    /// native（QuickJS）是 `38`、在 wasm 腿（node）是 **`40`** —— 差的两名是 node 给 sloppy 函数挂上的
    /// 遗留格 `arguments`/`caller`（事实 111）⇒ 这一格把引擎给的那五名（`length`/`name`/`prototype`/
    /// `arguments`/`caller`）**剔掉再按类分数**，两条腿才交同一个 `22,35`。
    #[test]
    fn the_tilemap_member_face_and_the_tile_id_tables_are_the_upstream_class() {
        let host = tilemap_booted();
        let js = r#"
var want = ['initialize', 'setData', 'isReady', 'update', 'refresh', 'refreshTileset',
    'updateTransform', '_createLayers', '_updateLayerPositions', '_paintAllTiles', '_paintTiles',
    '_drawTile', '_drawNormalTile', '_drawAutotile', '_drawTableEdge', '_drawShadow',
    '_readMapData', '_isHigherTile', '_isTableTile', '_isOverpassPosition', '_sortChildren',
    '_compareChildOrder', '_readLastTiles', '_writeLastTiles', 'width', 'height',
    'tileWidth', 'tileHeight'];
var own = Object.getOwnPropertyNames(Tilemap.prototype);
var enumerable = [];
for (var k in Tilemap.prototype) {
    if (Object.prototype.hasOwnProperty.call(Tilemap.prototype, k)) { enumerable.push(k); }
}
var out = [];
out.push(want.filter(function(n) { return own.indexOf(n) < 0; }).join(','));
out.push(own.filter(function(n) { return n !== 'constructor' && want.indexOf(n) < 0; }).join(','));
var statics = Object.getOwnPropertyNames(Tilemap).filter(function(n) {
    return ['length', 'name', 'prototype', 'arguments', 'caller'].indexOf(n) < 0;
});
out.push(own.length + '|' + enumerable.length + '|' +
    statics.filter(function(n) { return typeof Tilemap[n] === 'function'; }).length + ',' +
    statics.length);
out.push(Tilemap.TILE_ID_B + ',' + Tilemap.TILE_ID_A1 + ',' + Tilemap.TILE_ID_A2 + ',' +
    Tilemap.TILE_ID_MAX);
out.push(Tilemap.FLOOR_AUTOTILE_TABLE.length + ',' + Tilemap.WALL_AUTOTILE_TABLE.length + ',' +
    Tilemap.WATERFALL_AUTOTILE_TABLE.length + ',' + Tilemap.FLOOR_AUTOTILE_TABLE[0].join('/'));
out.push(Tilemap.isAutotile(2048) + ',' + Tilemap.isAutotile(512) + ',' + Tilemap.isVisibleTile(0) + '|' +
    Tilemap.getAutotileKind(2816) + ',' + Tilemap.getAutotileShape(2816) + ',' +
    Tilemap.makeAutotileId(0, 0) + '|' + Tilemap.isWaterTile(2048) + ',' + Tilemap.isWaterfallTile(2048));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "||29|25|22,35|0,2048,2816,8192|48,16,4,2,4/1,4/2,3/1,3|true,false,false|16,0,2048|true,false",
            "两个空串 = 一名不缺、一名不多（24 条方法 + `constructor` 补句 + 4 块访问器 = 29）；\
             末段那三档是**图块号的算法**：A2 的第一颗 `2816` 是 kind **16**、shape **0**，而\
             `makeAutotileId(0, 0)` 折回 `2048`（往返对上 ⇒ 那 22 条静态不是装饰，是 `Game_Map`\
             图块号的同一套真相）"
        );
    }

    /// **水面那一格：`animationFrame` 不在初值里 ⇒ 第一次画它一格都不落，`update()` 之后才落四笔**。
    ///
    /// 上游 `Tilemap.initialize` 只写 `animationCount = 0`（`rpg_core.js:4574`）而**不写 `animationFrame`**，
    /// `_drawAutotile` 的头一句就是 `[0, 1, 2, 1][this.animationFrame % 4]`（`:5052`）⇒
    /// `undefined % 4` = `NaN` ⇒ 那颗索引 `undefined` ⇒ `bx = NaN` ⇒ 源矩形 `sx = NaN` ⇒
    /// **过不了 `Bitmap.bltImage` 那道 `sx >= 0` 的闸**（事实 110；这一格换的是 `NaN` 那一档，
    /// 与事实 107 的"源尺寸不够"是同一道闸的两头）
    /// ⇒ 第一拍 `updateTransform()` 交出 **9 笔 `clearRect` + 0 笔 `blt`**：空格子照常擦，水那一格静默不落。
    /// 一次 `update()` 之后 `animationFrame = 0`，同一格里落下**四笔**（四个四分之一格）。
    ///
    /// 第二条腿是**"抹账本"与"重画"是两件事**（事实 112）：`refresh()` 只做 `_lastTiles.length = 0`（`:4724`）而
    /// `_needsRepaint` 在整个 `Tilemap` 里**从没被写成真**（`4746` 读、`4753` 抹，写真的两格在
    /// `ShaderTilemap:5548`/`:5600`）⇒ `refresh()` 之后紧接的那一次 `updateTransform()` **一格都不动**
    /// （`+0`），要等帧号或起点真的变了那一拍，被抹掉的账本才让**整片九格**重画（`+13`）。
    ///
    /// 第三条例子是**动画改的是源矩形，不是目的格子**：帧 0 那四格是 `48,96 24,96 48,72 24,72`，
    /// 帧 1 换成 `144,96 120,96 144,72 120,72`（`FLOOR_AUTOTILE_TABLE[0]` 那一行 + `waterSurfaceIndex`
    /// 从 0 走到 1 ⇒ `bx = 2` ⇒ `sx = (bx*2 + qsx) * 24`），而目的格 `48,48 / 72,48 / 48,72 / 72,72` 一格不动。
    #[test]
    fn the_tilemap_water_autotile_lands_only_after_the_first_update() {
        let host = tilemap_booted();
        let js = r#"
__sisf_paint_reset('x'); Sisf2DContext._lastSid = 0;
Graphics._width = 48; Graphics._height = 48;
var ops = function() { return JSON.parse(__sisf_paint_ops('x')); };
var pick = function(list, pre) { return list.filter(function(o) { return o.indexOf(pre) === 0; }); };
var rects = function(list, from) {
    return list.slice(from).filter(function(o) { return o.indexOf('blt|') === 0; })
        .map(function(o) { var f = o.split('|'); return f[4] + ',' + f[5]; });
};
var d = []; for (var i = 0; i < 45; i++) d.push(0);
d[(0 * 3 + 1) * 3 + 1] = 2048;
var t = new Tilemap();
t.bitmaps[0] = Bitmap.load('img/tilesets/World_A1.png');
t.setData(3, 3, d);
var out = [];
out.push('ctor ' + ops().length + ' blt=0|frame=' + String(t.animationFrame) + '|isReady=' + t.isReady());
t.updateTransform();
var a = ops();
out.push('paint1 ' + a.length + ' clear=' + pick(a, 'clearRect').length + ' blt=' +
    pick(a, 'blt|').length + '|gate=' + String(t._lastAnimationFrame) + ',' + t._lastStartX +
    ',' + t._lastStartY + ',' + String(t._needsRepaint) + ',' + String(t._frameUpdated));
t.update();
t.updateTransform();
var b = ops();
out.push('one-update ' + b.length + '(+' + (b.length - a.length) + ') frame=' + t.animationFrame +
    ' blt=+' + (pick(b, 'blt|').length - pick(a, 'blt|').length) + ' rects=' +
    rects(b, a.length).join(' '));
t.refresh();
t.updateTransform();
var c = ops();
out.push('refresh ' + c.length + '(+' + (c.length - b.length) + ') lastTiles=' + t._lastTiles.length);
for (var i = 0; i < 29; i++) t.update();
t.updateTransform();
var e = ops();
out.push('frame-roll ' + e.length + '(+' + (e.length - c.length) + ') frame=' + t.animationFrame +
    ' clear=+' + (pick(e, 'clearRect').length - pick(c, 'clearRect').length) +
    ' blt=+' + (pick(e, 'blt|').length - pick(c, 'blt|').length) + ' rects=' +
    rects(e, c.length).join(' '));
out.push('op ' + pick(e, 'blt|')[pick(e, 'blt|').length - 4]);
t.updateTransform();
out.push('same ' + (ops().length - e.length) + '|' + __sisf_composite_report('x'));
out.join('\n')
"#;
        assert_eq!(
            num(&host, js),
            "ctor 4 blt=0|frame=undefined|isReady=true\n\
             paint1 13 clear=9 blt=0|gate=undefined,-1,-1,false,false\n\
             one-update 18(+5) frame=0 blt=+4 rects=48,96 24,96 48,72 24,72\n\
             refresh 18(+0) lastTiles=0\n\
             frame-roll 31(+13) frame=1 clear=+9 blt=+4 rects=144,96 120,96 144,72 120,72\n\
             op blt|1|0|img%2Ftilesets%2FWorld_A1.png|144|96|24|24|48|48|24|24|255|source-over\n\
             same 0|applied=0;not=[];blend=[];refused=0;evict=0",
            "`paint1` 那行 `clear=9 blt=0` 与 `gate` 里那两个 `undefined` 必须**一起读**——那是\
             `NaN` 过不了闸，不是画法没接上（见上面第二条）；`one-update` 那 `+5` = **只有水那一格**\
             重画（其余八格被 `lowerTiles.equals(_readLastTiles(...))` 挡住）= 1 笔擦 + 4 笔源格；\
             `gate` 里 `-1,-1` 是 `startX/startY`（`origin` 0 减 `margin` 20 再整除 48 向下取整）\
             ⇒ 第一格读的是图外的那一圈，这就是 `_readMapData` 要先把越界折回 0 的那一条腿；\
             `op` 整段那三格 `0|img%2Ftilesets%2FWorld_A1.png` 是**源那一段交的是 URL 而不是位图号**\
             （`bltImage` 读 `source._image`，与 `blt` 读 `source._canvas` 不是同一格 —— 事实 109）；\
             末段 `applied=0` 与前头的 8 笔**一起读**（记下了、一笔都没回放，事实 98 在这一颗上的读数）"
        );
    }

    /// **`flags` 那一格把图块分到上下两张层位图，而上下在操作流里是两个不同的位图号**。
    ///
    /// 夹具在 48×48 的屏上放一张 3×3 的图（**这一档故意把虚拟屏压小**：1104×736 那一档一帧就是
    /// 25×18 = 450 格、900 笔擦，整串没法读；压小之后读数仍是同一套坐标算法）：
    /// `z0 = 512`（普通图块，`setNumber = 5 + floor(512/256) = 7`）、`z1 = 768` 且 `flags[768] = 0x10`
    /// （`_isHigherTile` 那一格 ⇒ **改走上层**）、`z4 = 5`（那一张是**阴影**的四个四分之一格的位图）。
    ///
    /// 四段读数：⑴ 两笔 `blt` 的**目的格完全相同**（`48|48|48|48`）而**位图号一个是 1、一个是 2**
    /// ⇒ "同一格画了两次，一次在下层一次在上层"在操作流里是分得清的（这正是 `Tree` 那一族后面
    /// 「宿主那侧的真绘制」要的两层合成顺序）；⑵ 阴影那两笔 `fillRect` 落在**下层**（`|1|`），
    /// 四格里只落两格，因为 `shadowBits = 5` 只有第 0 与第 2 位（`if (shadowBits & (1 << i))`），
    /// 而颜色 `rgba(0,0,0,0.5)` 被 `Sisf2DContext.parseColor` 折成 `0,0,0,128`；⑶ `last` 那三段是
    /// `_lastTiles` 那本账：下层 `[512, -5, 0, 0]`（**阴影是挤成负数进同一张列表的**，`-shadowBits`）、
    /// 上层 `[768]`、空格子 `[0, 0, 0, 0, 0]`（五格 = z0/z1/-shadow/z2/z3）；⑷ `sid 1|2` 与 `z=0,4`
    /// 是 `_sortChildren` 排完之后那两层的顺序（`_compareChildOrder` 先比 `z`）。
    #[test]
    fn the_tilemap_flags_route_a_higher_tile_onto_the_upper_bitmap() {
        let host = tilemap_booted();
        let js = r#"
__sisf_paint_reset('x'); Sisf2DContext._lastSid = 0;
Graphics._width = 48; Graphics._height = 48;
var ops = function() { return JSON.parse(__sisf_paint_ops('x')); };
var pick = function(list, pre) { return list.filter(function(o) { return o.indexOf(pre) === 0; }); };
var d = []; for (var i = 0; i < 45; i++) d.push(0);
d[(0 * 3 + 1) * 3 + 1] = 512;
d[(1 * 3 + 1) * 3 + 1] = 768;
d[(4 * 3 + 1) * 3 + 1] = 5;
var t = new Tilemap();
t.bitmaps[7] = Bitmap.load('img/tilesets/World_A1.png');
t.bitmaps[8] = Bitmap.load('img/tilesets/World_A1.png');
t.flags[768] = 0x10;
t.setData(3, 3, d);
t.update();
t.updateTransform();
var a = ops();
var out = [];
out.push('all=' + a.length + ' clear=' + pick(a, 'clearRect').length + ' blt=' +
    pick(a, 'blt|').length + ' fillRect=' + pick(a, 'fillRect|').length);
out.push('blts ' + pick(a, 'blt|').join(' ;; '));
out.push('shadow ' + pick(a, 'fillRect|').join(' ;; '));
out.push('last ' + JSON.stringify(t._readLastTiles(0, 1, 1)) + '|' +
    JSON.stringify(t._readLastTiles(1, 1, 1)) + '|' + JSON.stringify(t._readLastTiles(0, 0, 0)));
out.push('layers ' + (t._lowerLayer.children[0].bitmap === t._lowerBitmap) + ',' +
    (t._upperLayer.children[3].bitmap === t._upperBitmap) + '|' +
    t._lowerBitmap.width + 'x' + t._lowerBitmap.height);
out.push('sid ' + t._lowerBitmap._context._sid() + '|' + t._upperBitmap._context._sid() +
    '|z=' + t.children.map(function(x) { return x.z; }).join(','));
out.push('upper-frame ' + t._upperLayer.children[0]._frame.width + ',' +
    t._upperLayer.children[0]._frame.height + '|' + t._upperLayer.children[0].x + ',' +
    t._upperLayer.children[0].y);
out.join('\n')
"#;
        assert_eq!(
            num(&host, js),
            "all=18 clear=10 blt=2 fillRect=2\n\
             blts blt|1|0|img%2Ftilesets%2FWorld_A1.png|0|0|48|48|48|48|48|48|255|source-over ;; \
             blt|2|0|img%2Ftilesets%2FWorld_A1.png|0|0|48|48|48|48|48|48|255|source-over\n\
             shadow fillRect|1|48|48|24|24|0,0,0,128|source-over ;; \
             fillRect|1|48|72|24|24|0,0,0,128|source-over\n\
             last [512,-5,0,0]|[768]|[0,0,0,0,0]\n\
             layers true,true|144x144\n\
             sid 1|2|z=0,4\n\
             upper-frame 20,20|0,0",
            "`clear=10` 那一个**不是** 9：下层九格全擦，上层只擦中心那一格（其余八格的\
             `upperTiles` 是空列表，与账本里的空列表 `equals` ⇒ 不擦）—— 这就是\
             「上层只装 `0x10` 那一族」在笔数上的读数；`upper-frame 20,20|0,0` 是\
             `_updateLayerPositions` 那四格环形拼接的第一格（`x2 = (0 - 20).mod(144) = 124`\
             ⇒ `w1 = 144 - 124 = 20`），而 `(0, 0)` 是那格的**位置**：wrap 用的四格 Sprites\
             就是靠这一手把一张层位图重复铺满屏幕（`Number.prototype.mod` 走的就是这一条腿，\
             少了它 `x2` 会成 `-24` ⇒ 四格尺寸全错）"
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

    // ———— M10.5 第四格第一小格：`Bitmap` 的数据那一半 ————

    /// `Bitmap` 的构造体沿用 [`crate::shapes`] 那颗壳（体就是 `this.initialize.apply(…)`，
    /// 实测 `rpg_core.js:713-715`），所以这里只点名形状、不交构造体。
    const BITMAP_SHELLS: &[(&str, ShapeKind)] = &[("Bitmap", ShapeKind::Class)];

    /// 只装 `Bitmap` 这一格要的那几颗形状 + 那一整圈（不碰 `Graphics`/音频）——
    /// 判据各自最小化，同 `cache_booted` / `geometry_booted` 的手法。
    ///
    /// 夹具 VFS 里那一张 PNG 是 [`crate::raster::sample_png_2x2`]：2×2、四颗像素、
    /// 其中 `(1,1)` 是**半透明**那一档（预乘 ⇄ 非预乘那条域的样品）。
    fn bitmap_booted() -> Host {
        let mut names: Vec<(&str, ShapeKind)> = vec![
            ("Graphics", ShapeKind::StaticClass),
            ("JsonEx", ShapeKind::StaticClass),
        ];
        names.extend(GEOMETRY_SHELLS.iter().cloned());
        names.extend(DISPLAY_SHELLS.iter().cloned());
        names.extend(CACHE_SHELLS.iter().map(|n| (*n, ShapeKind::Class)));
        names.extend(BITMAP_SHELLS.iter().cloned());
        let host = shell(&names);
        // **两条本族之外的直接依赖**，装在 fixture 里而不是让判据各自补：
        // ⑴ `padZero` —— `getPixel` 的那三行十六进制拼写用的就是它（`shapes::install_extensions`）；
        // ⑵ 缓存四颗 —— 这一族的判据要问 `ImageCache.limit`（预算那一格的对表）与真位图的裁剪账。
        crate::shapes::install_extensions(&host).expect("JsExtensions 装不上");
        install_cache(&host).expect("缓存四颗装不上");
        let mut mem = MemoryProvider::new();
        mem.insert(
            &VfsPath::new("img/system/Window.png").unwrap(),
            &crate::raster::sample_png_2x2(),
        );
        let mut vfs = Vfs::new();
        vfs.register(Box::new(mem));
        install_bitmap(&host, Rc::new(vfs)).expect("`Bitmap` 的数据那一半装不上");
        host
    }

    /// **`Bitmap.load` 那一条腿走完整颗上游状态机**，并且尺寸是真的：
    /// `initialize` → `_defer` 那一支不建位图 → `_requestImage`（自创底）→ `_onLoad`（逐字）
    /// → `requestCompleted` → `decode`（逐字）→ `loaded`。
    ///
    /// 第二、三段是同一颗位图的另外两条腿：`Bitmap.request` + `startRequest()` 因为
    /// `_decodeAfterRequest` 是 false ⇒ 落 `'purged'` 并 `_clearImgInstance()`（上游"只请求不解码"
    /// 的那条省内存的腿），随后 `decode()` 从 `'purged'` 那一支**重新请求一次**才到 `loaded`
    /// —— 这三格都是上游的，一条都没改良。而 `_reuseImages` 在中间**取回过**那颗样品
    /// （`_clearImgInstance` push、`_requestImage` pop），所以它不会越堆越多。
    #[test]
    fn a_loaded_bitmap_walks_the_upstream_state_machine_and_answers_its_size() {
        let host = bitmap_booted();
        let js = r#"
var out = [];
var a = Bitmap.load('img/system/Window.png');
out.push(a._loadingState + ',' + String(a.isReady()) + ',' + String(a.isError()));
out.push(String(a.width) + ',' + String(a.height));
out.push(String(a.url) + ',' + String(a._decodeAfterRequest));
var r = a.rect;
out.push(String(r instanceof Rectangle) + ',' + r.width + ',' + r.height);

var b = Bitmap.request('img/system/Window.png');
out.push(b._loadingState + ',' + String(b.isRequestOnly()) + ',' + String(b.isRequestReady()));
b.startRequest();
out.push(b._loadingState + ',' + String(b.width) + ',' + String(b._image));
b.decode();
out.push(b._loadingState + ',' + String(b.width) + ',' + String(b.isReady()));

out.push(String(Bitmap._reuseImages.length));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "loaded,true,false|2,2|img/system/Window.png,true|true,2,2|pending,true,false|purged,0,null|loaded,2,true|0",
            "`loaded` 之后尺寸必须是 **2,2**（旧句柄那一版这里恒 0 ⇒ `_truncateCache` 永远不裁）；\
             `request`+`startRequest` 必须落 `purged` 且 `_image` 已被抹成 null（上游 `:1571-1574` 那\
             条省内存的腿），`decode()` 之后才 `loaded`；倒数第二格是 `_reuseImages` 的账 ——\
             `b` 那一轮 push 进去的样品被 `decode()` 的重新请求**pop 回**去了（自创底⑵照上游那一手），\
             所以它是 0 而不是越堆越多的 1：钉的就是这一取一还。\
             这里**不钉** `typeof ImageManager` 那一格 —— 共享 realm 里它取决于哪条判据先跑过\
             （`M10.md` 事实 78 同一条腿：钉落点，不钉环境）"
        );
    }

    /// **`new Bitmap(w, h)` 有一处地方、还有一条 `size` 操作**。
    ///
    /// 上一档这一格钉的是「有尺寸、没有画布」（`__context` 是 undefined 才算对）；第二小格把
    /// [`SISF_CONTEXT_JS`] 接上之后，画布与上下文都在了，于是它改成钉**干了什么**：
    /// `__canvas` 是带访问器的记录，给 `width`/`height` 赋值就是往 [`crate::paint`] 交一条
    /// `size` 操作（上游给 `canvas.width` 赋值本身就是一次改尺寸 + 清空内容 —— `M10.md`
    /// 「第四格第二小格」的边界⑷），而两条尺寸赋值就是两条操作，一条都不许合。
    #[test]
    fn a_created_bitmap_lands_a_surface_and_records_its_size() {
        let host = bitmap_booted();
        let js = r#"
__sisf_paint_reset('x'); Sisf2DContext._lastSid = 0;
var out = [];
var b = new Bitmap(300, 200);
out.push(String(b.width) + ',' + String(b.height));
out.push(String(b._loadingState) + ',' + String(b.isReady()));
out.push(String(b.rect instanceof Rectangle));
b.smooth = true;
out.push(String(b.smooth) + ',' + String(b._smooth));
out.push(String(typeof b.__context) + ',' + String(b.context === b._context));
out.push(String(b.canvas === b._canvas) + ',' + String(b.paintOpacity));
out.push(JSON.parse(__sisf_paint_ops('x')).join(' ;; '));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "300,200|none,true|true|true,true|object,true|true,255|\
             size|1|300|1| ;; size|1|300|200|",
            "末段那两条 `size` 就是「位图的存在方式」现在真的有一条操作流了；\
             `paintOpacity` 回归**逐字访问器**（上一档它是 undefined）\
             ⇒ 读到字段的 255，而 setter 会把 `globalAlpha` 一起带走（下一条判据钉那一手）"
        );
    }

    /// **`blt` 的几何全在上游那一格里，而它落成一条操作**。逐字文本给出的：`dw`/`dh` 的缺参、
    /// 那道 `sx + sw <= source.width` 的闸、`paintOpacity` → `globalAlpha` → 操作里的 opacity。
    /// 引擎那一侧的 `blt` **没有**目标尺寸参数（`bitmap.rs:352`）⇒ `dw`/`dh` 必须进操作，
    /// 否则缩放那一族到宿主手里就没了（`M10.md` 事实 75 的第⑴条真缺口，这一格把它带到读数里）。
    #[test]
    fn a_blt_lands_the_upstream_geometry_as_one_op() {
        let host = bitmap_booted();
        let js = r#"
__sisf_paint_reset('x'); Sisf2DContext._lastSid = 0;
var out = [];
var src = new Bitmap(8, 6);
var dst = new Bitmap(20, 10);
var loaded = Bitmap.load('img/system/Window.png');
dst.blt(src, 1, 2, 4, 3, 5, 6);
dst.blt(src, 1, 2, 4, 3, 5, 6, 9, 7);
dst.blt(src, 1, 2, 40, 3, 5, 6);
dst.paintOpacity = 128;
dst.blt(src, 0, 0, 2, 2, 0, 0);
dst.paintOpacity = 0;
dst.blt(src, 0, 0, 2, 2, 0, 0);
dst.paintOpacity = 255;
dst.bltImage(loaded, 0, 0, 2, 2, 3, 4);
var ops = JSON.parse(__sisf_paint_ops('x'));
out.push(ops.slice(4).join(' ;; '));
out.push(String(ops.length) + ',' + String(dst.width) + ',' + String(src.height));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "blt|2|1||1|2|4|3|5|6|4|3|255|source-over ;; \
             blt|2|1||1|2|4|3|5|6|9|7|255|source-over ;; \
             blt|2|1||0|0|2|2|0|0|2|2|128|source-over ;; \
             blt|2|1||0|0|2|2|0|0|2|2|0|source-over ;; \
             blt|2|0|img%2Fsystem%2FWindow.png|0|0|2|2|3|4|2|2|255|source-over|9,20,6",
            "五条，不是六条：第三条（`sw = 40` 出界）被上游那道闸挡掉 ⇒ 一条操作都不落。\
             末条的源是 `0|<URL>`（`bltImage` 交的是 `_image`，那颗记录带的是磁盘上那条拼写，\
             不是位图号）；`128`/`0` 两条钉的是「`paintOpacity` 经 `globalAlpha` 流进每一条操作」\
             —— 上游 `alpha = 0` 仍然画（画成全透明），所以那一条**在**而不是**没有**"
        );
    }

    /// **铺、清、渐变、圆**各落成一条具名操作。这一格钉的是"每一笔都只有一条操作"，
    /// 以及 `save`/`restore` 在这一层**不产生操作**（上游 core 的 save/restore 只圈住
    /// `fillStyle`/`font`/`globalAlpha` 这些**当场兑现**的样式，没有 clip）。
    #[test]
    fn fills_clears_gradients_and_circles_land_as_named_ops() {
        let host = bitmap_booted();
        let js = r#"
__sisf_paint_reset('x'); Sisf2DContext._lastSid = 0;
var b = new Bitmap(6, 4);
b.fillRect(1, 1, 4, 2, 'rgba(255, 0, 0, 0.5)');
b.fillAll('#00ff00');
b.gradientFillRect(0, 0, 6, 4, '#ffffff', '#000000', true);
b.gradientFillRect(0, 0, 6, 4, '#fff', 'rgb(0, 0, 0)');
b.drawCircle(3, 2, 2, 'black');
b.clearRect(0, 0, 2, 2);
b.clear();
JSON.parse(__sisf_paint_ops('x')).slice(2).join(' ;; ')
"#;
        assert_eq!(
            num(&host, js),
            "fillRect|1|1|1|4|2|255,0,0,128|source-over ;; \
             fillRect|1|0|0|6|4|0,255,0,255|source-over ;; \
             gradientFillRect|1|0|0|6|4|255,255,255,255|0,0,0,255|1|source-over ;; \
             gradientFillRect|1|0|0|6|4|255,255,255,255|0,0,0,255|0|source-over ;; \
             fillArc|1|3|2|2|0,0,0,255|source-over ;; \
             clearRect|1|0|0|2|2 ;; clearRect|1|0|0|6|4",
            "`255,0,0,128` 那一格钉的是 alpha **乘过 globalAlpha**（0.5 × 255 = 127.5 → 128，\
             上游 canvas 的合成规则）；渐变那条的方向由两端点算出（引擎没有 gradient，\
             事实 75 的第⑵条真缺口 ⇒ 宿主按尺寸现生成一张像素纹理再贴）；\
             `#fff` 与 `rgb(0, 0, 0)` 与 `black` 三种写法都落到同一条腿 —— 认不出的写法**抛**，\
             不许读成黑"
        );
    }

    /// **一行字 = 两条操作**（描边在前、正文在后），而 `ty` 的那个偏移、对齐、`maxWidth`
    /// 全在上游 `drawText` 那一格里算完（`:1331-1357`）—— 这一格钉的就是"我们没有重算它"。
    #[test]
    fn a_drawn_line_of_text_carries_the_font_state() {
        let host = bitmap_booted();
        let js = r#"
__sisf_paint_reset('x'); Sisf2DContext._lastSid = 0;
var b = new Bitmap(100, 50);
b.fontSize = 20;
b.drawText('你好', 5, 7, 40, 20, 'center');
b.drawText('a', 0, 0, 100, 20, 'right');
b.fontItalic = true;
b.drawText('i', 0, 0, 0, 20);
b.drawText(undefined, 0, 0, 10, 10, 'left');
JSON.parse(__sisf_paint_ops('x')).slice(2).join(' ;; ')
"#;
        assert_eq!(
            num(&host, js),
            "strokeText|1|%E4%BD%A0%E5%A5%BD|25|24|40|GameFont|20|0|center|alphabetic|\
             0,0,0,128|4|source-over ;; \
             fillText|1|%E4%BD%A0%E5%A5%BD|25|24|40|GameFont|20|0|center|alphabetic|\
             255,255,255,255|1|source-over ;; \
             strokeText|1|a|100|17|100|GameFont|20|0|right|alphabetic|0,0,0,128|4|source-over ;; \
             fillText|1|a|100|17|100|GameFont|20|0|right|alphabetic|255,255,255,255|1|source-over ;; \
             strokeText|1|i|0|17|4294967295|GameFont|20|1|start|alphabetic|0,0,0,128|4|source-over ;; \
             fillText|1|i|0|17|4294967295|GameFont|20|1|start|alphabetic|255,255,255,255|1|source-over",
            "六条，不是八条：`drawText(undefined, …)` 上游第一句就滤掉 ⇒ 一条都不落。\
             `24`/`17` 是 `y + lineHeight - (lineHeight - fontSize*0.7)/2` 那一句的读数\
             （这一族的偏移一改，屏幕上每行字的高度就一起漂 —— 它必须在**上游那一格**里算）；\
             `4294967295` 是 `maxWidth = maxWidth || 0xffffffff`（0 在上游是「不限宽」）；\
             `start` 是「align 缺参时 canvas 的那一颗不变」这一档；\
             `%E4%BD%A0%E5%A5%BD` 钉的是正文**编码过**——台词里有竖线与换行（反斜杠转义那一族是 MV 的写法），\
             不编码就会把一条操作劈成两条"
        );
    }

    /// **文字宽度是问宿主的**：引擎的 `TextEngine::layout` 是纯 CPU 的（`draw.rs:69`），
    /// 但它住在宿主那一侧。所以宿主没绑测量器时**抛**（"每个字符一样宽"会被窗宽照抄），
    /// 绑了之后这一格的参数表要原样送过去（字号、字面、italic 三格都是它的输入）。
    #[test]
    fn the_text_width_question_goes_to_the_host_and_back() {
        let host = bitmap_booted();
        // ⑴ 今天（记录器）：抛，而且抛的是那一格的名字。
        let thrown = num(
            &host,
            r#"
try { new Bitmap(10, 10).measureTextWidth('abc'); 'no-throw' }
catch (e) { String(e.message.indexOf('measureText 未接') >= 0) }
"#,
        );
        assert_eq!(thrown, "true", "宿主没绑测量器时不许读成 0 或 NaN");

        // ⑵ 宿主实现：读数直接成为 `measureTextWidth` 的读数，而**参数表**要原样送过去
        // （`measureTextWidth` 自己把结果当数字，所以钉"问了什么"得在 Rust 这一侧抄下来）。
        let seen: std::rc::Rc<std::cell::RefCell<String>> = std::rc::Rc::default();
        host.bind_str(crate::paint::MEASURE_GLOBAL, {
            let seen = std::rc::Rc::clone(&seen);
            move |spec| {
                seen.borrow_mut().push_str(&spec);
                "12".to_string()
            }
        })
        .expect("宿主测量器绑不上");
        assert_eq!(
            num(&host, "String(new Bitmap(10, 10).measureTextWidth('你'))"),
            "12",
            "上游 `Window_ChoiceList` 的窗宽就是这一格的读数 ⇒ 它必须真的过宿主那一条腿"
        );
        assert_eq!(
            seen.borrow().as_str(),
            "measure|%E4%BD%A0|GameFont|28|0",
            "缺省的字体那一族走的是 `initialize` 的默认值（`GameFont` / 28 / 不斜体）"
        );

        // ⑶ 参数表里的三格样式确实跟着 `font` 走，而正文里的竖线**必须编码**。
        seen.borrow_mut().clear();
        let width = num(
            &host,
            r#"
__sisf_paint_reset('x');
var b = new Bitmap(10, 10);
b.fontFace = 'Mplus 1m'; b.fontSize = 26; b.fontItalic = true;
String(b.measureTextWidth('a|b'))
"#,
        );
        assert_eq!(width, "12");
        assert_eq!(
            seen.borrow().as_str(),
            "measure|a%7Cb|Mplus%201m|26|1",
            "正文里的竖线编码成 `%7C`（否则参数表自己就被劈开），字面与字号跟着 `font` 那一格走"
        );
    }

    /// **`getPixel` 现在是上游那一格逐字文本**（`:1196-1203` 的 `getImageData` + `padZero` 三行），
    /// 所以"未预乘"这一族要重新在 JS 这一侧钉一遍：`decode_image` 交的是预乘
    /// （`crates/sisf-media/src/image.rs:112-124`），上游 canvas 的 `getImageData` 是**非预乘**，
    /// 除回去那一步是整个落法里唯一会差数字的地方 ⇒ `(1,1)` 那颗 alpha 128 的样品逐格对上原色。
    #[test]
    fn the_pixel_read_is_the_un_premultiplied_color_the_canvas_would_give() {
        let host = bitmap_booted();
        let js = r#"
var b = Bitmap.load('img/system/Window.png');
[b.getPixel(0, 0), b.getPixel(1, 0), b.getPixel(0, 1), b.getPixel(1, 1),
 b.getAlphaPixel(1, 1), b.getPixel(9, 9), b.getAlphaPixel(9, 9)].join('|')
"#;
        assert_eq!(
            num(&host, js),
            "#ff0000|#00ff00|#0000ff|#804020|128|#000000|0",
            "四颗像素 + alpha 那一档 + 越界那一档；`#804020` 是 (128,64,32) 的十六进制，\
             少一次「除回 alpha」就会读成 `#402010`（上一档这一格问的是 `__sisf_image_pixel`，\
             现在问的是逐字的 `getImageData` —— 同一条腿换了入口，读数一处不动）"
        );
    }

    /// **色调落在 `blt` 进去的像素上** —— 第四小格那一层存在的理由，也是这一格唯一的硬读数。
    /// 上游 `Window.prototype._refreshBack`（`rpg_core.js:6669-6690`）的形状就是这一手：
    /// `new Bitmap(w,h)` → 把窗皮 `blt` 进去 → `adjustTone(tone)` → 那张画布**自己要看得到色调**
    /// （`$dataSystem.windowTone` 实测 `[-51,-51,-51,0]` ⇒ 每一只窗建立那一刻都走这一条）。
    /// 第三小格那档"读不到就是透明黑 + 计数"在这里换成了"读得到，因为回放过了"。
    #[test]
    fn a_blt_into_a_created_bitmap_gives_the_tone_real_pixels() {
        let host = bitmap_booted();
        let js = r#"
__sisf_paint_reset('x'); Sisf2DContext._lastSid = 0;
var out = [];
var skin = Bitmap.load('img/system/Window.png');
var back = new Bitmap(2, 2);
back.blt(skin, 0, 0, 2, 2, 0, 0, 2, 2);
back.adjustTone(-10, 20, 30);
out.push(back.getPixel(0, 0) + '|' + back.getPixel(1, 0));
out.push(String(back.getAlphaPixel(1, 1)));
out.push(__sisf_pixel_blind('x'));
out.push(__sisf_composite_report('x'));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "#f5141e|#00ff1e|128|0,0|applied=8;not=[];blend=[];refused=0;evict=0",
            "首格 = 窗皮那三原色被 `blt` 进来之后**再叠上色调**的读数（色调没落在 blt 的像素上就还是 `#000000`）；\
             `blind` 必须是 `0,0`（画出来的那一张从此有源）；report 里 `not=[]`、`blend=[]`、`refused=0`、`evict=0`"
        );
    }

    /// **空画布与没有画布是两种读数**：建了位图但一笔没画 ⇒ 上游那颗空 canvas 同档的透明黑，
    /// 而且**不是盲区**（画布在，只是空的 —— 合成回放那两条 `size` 就够）；真的问不出一颗粒的号
    /// （`pixels|0|…` 那一条 = JS 那层没有 `__canvas` 时的号）⇒ 一片零**并数下来**，不抛。
    /// 顺带钉一条踩过的形状：`naked._context = …` 这种赋值撞在逐字的 `_context` **只读访问器**
    /// 上是静默失败（JS 的 sloppy 模式不报），于是那颗位图其实被懒创建了 —— 判据要的是"没有画布"，
    /// 只能从 transport 那一头问。
    #[test]
    fn an_empty_canvas_reads_black_while_a_missing_one_is_counted() {
        let host = bitmap_booted();
        let js = r#"
__sisf_paint_reset('x'); Sisf2DContext._lastSid = 0;
var out = [];
var b = new Bitmap(4, 4);
out.push(b.getPixel(1, 1) + ',' + String(b.getAlphaPixel(1, 1)));
out.push(__sisf_pixel_blind('x'));
out.push(globalThis.__sisf_pixels('pixels|0|0|0|1|1|'));
out.push(__sisf_pixel_blind('x'));
out.push(__sisf_composite_report('x'));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "#000000,0|0,0|blank|1|1|1,0|applied=2;not=[];blend=[];refused=0;evict=0",
            "第一档透明黑且 `blind` 不动；第二档 `blank|1|1` 并把读的次数推到 1；\
             合成只落了 2 条（`new Bitmap(4,4)` 给宽、高各一条 `size`）⇒ 问不出号那一条\
             **什么都没回放**，它只进盲区数"
        );
    }

    /// 合成器**不回放**的那几格动作必须在报表里数得出名字与次数（今天只剩文字那两格），
    /// 认不下的混合值也是 —— 这一格是"操作表与合成器脱钩"的运行时警报位，另一头是尺子
    /// `../sisf_pray/tools/probes/software-composite.py` 的第①段。
    ///
    /// 渐变与圆**从这一格的名单里出去了**（M10.5 真绘制第二小格）：它们只需要停靠点与半径，
    /// 所以落在 CPU 这一侧 —— 于是这一格里那条水平渐变**落了像素**，`(0,0)` 读的是
    /// `t = 0.125` 处预乘插值的那一份（`223` 而不是 `255`），`applied` 从 3 推到 4。
    #[test]
    fn what_the_compositor_refuses_to_draw_is_named_with_its_count() {
        let host = bitmap_booted();
        let js = r#"
__sisf_paint_reset('x'); Sisf2DContext._lastSid = 0;
var b = new Bitmap(4, 4);
var c = b._context;
c.fillStyle = 'white';
c.fillRect(0, 0, 4, 4);
c.font = '28px GameFont';
c.fillText('你好', 0, 24);
c.fillStyle = c.createLinearGradient(0, 0, 4, 0);
c.fillStyle.addColorStop(0, '#fff');
c.fillStyle.addColorStop(1, '#000');
c.fillRect(0, 0, 4, 4);
c.globalCompositeOperation = 'difference';
c.fillStyle = 'red';
c.fillRect(1, 1, 1, 1);
out = [];
out.push(c.getImageData(0, 0, 1, 1).data.join(','));
out.push(__sisf_composite_report('x'));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "223,223,223,255|applied=4;not=[fillText:1];blend=[difference:1];refused=0;evict=0",
            "文字那一笔**没落**、`difference` 那一笔**没落** ⇒ 两格在报表里点名带次数，不静默也\
             不假装画对；渐变那一笔**落了**（`applied` 3→4），而 `(0,0)` 读的是 `t=0.125` 处\
             预乘插值的那一份 = `223`（不是白的 `255`，也不是停靠点 0 的整份）"
        );
    }

    /// **色调那一格是上游文本**（`:1383-1396` 的 `getImageData` → `pixels[i] += r` →
    /// `putImageData` → `_setDirty`，移植侧一行不写）。它落地靠的两件事都在别处：
    /// ⑴ `getImageData` 交回 `Uint8ClampedArray` —— 上游那三行**不加夹逼**，界外与小数全靠
    /// 这一颗类型学的手；⑵ [`crate::raster`] 的那一份 **per-bitmap 副本**。
    /// 夹具那四颗像素是原色的四档，alpha 全 255 ⇒ 读数没有预乘往返的舍入（那一档单独钉在
    /// `paint.rs` 的判据里）。
    #[test]
    fn a_tone_adds_and_clamps_at_both_ends_like_the_canvas_would() {
        let host = bitmap_booted();
        let js = r#"
__sisf_paint_reset('x'); Sisf2DContext._lastSid = 0;
var out = [];
var b = Bitmap.load('img/system/Window.png');
out.push(String(b._context.getImageData(0, 0, 1, 1).data instanceof Uint8ClampedArray));
b.adjustTone(10, -20, 30);
out.push(b.getPixel(0, 0) + '|' + b.getPixel(1, 0));
b.adjustTone(-200, 0, 0);
out.push(b.getPixel(0, 0) + '|' + b.getPixel(1, 0));
out.push(String(b.getAlphaPixel(0, 0)));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "true|#ff001e|#0aeb1e|#37001e|#00eb1e|255",
            "两端都夹：`255+10` 夹回 255 而 `0-20` 夹回 0（`#ff001e`）；第二次叠的是**副本**\
             上的数（`255-200=55` ⇒ `#37001e`），不是镜像的原色；alpha 一格都不动（上游只加 r/g/b）"
        );
    }

    /// **色相那一格也是上游文本**（`:1404-1466`：两具内嵌函数 `rgbToHsl`/`hslToRgb` 加
    /// `((offset % 360) + 360) % 360` 那一行折负数）。转 120° 就是三原色在色轮上**整整移一档**，
    /// 而 −120° 走的是同一行折法 ⇒ 红转蓝。半透明那一颗（128,64,32,128）转完是 `#208040`，
    /// 它的数全程浮点、进 `Uint8ClampedArray` 才舍成整数。
    #[test]
    fn a_hue_rotation_walks_the_upstream_hsl_math_a_third_of_the_wheel() {
        let host = bitmap_booted();
        let js = r#"
__sisf_paint_reset('x'); Sisf2DContext._lastSid = 0;
var out = [];
var b = Bitmap.load('img/system/Window.png');
b.rotateHue(120);
out.push(b.getPixel(0, 0) + '|' + b.getPixel(1, 0) + '|' + b.getPixel(0, 1));
out.push(b.getPixel(1, 1) + ',' + String(b.getAlphaPixel(1, 1)));
var c = Bitmap.load('img/system/Window.png');
c.rotateHue(-120);
out.push(c.getPixel(0, 0));
out.push(String(__sisf_pixel_blind('x')));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "#00ff00|#0000ff|#ff0000|#208040,128|#0000ff|0,0",
            "三原色各转一档（红→绿→蓝→红）而半透明那颗转成 `#208040`（alpha 还是 128）；\
             第二颗位图（新的号 ⇒ 从镜像重新起）转 −120° 落蓝 ⇒ 副本是**逐位图**的，\
             上一颗的色相不渗过来；盲区数 `0,0` 钉的是这一族的四次读一次写**全都有源**"
        );
    }

    /// 色调**不许写回共享的镜像**：上游的 canvas 是 per-bitmap 的，而这里镜像是按 URL 共享的
    /// （`ImageCache` 那一族）。少了这一格，`a.adjustTone` 会让下一颗 `Bitmap.load` 同一张图
    /// 直接变色 —— 窗皮与所有窗的 `textColor` 读的都是同一张。
    #[test]
    fn a_toned_bitmap_leaves_the_shared_mirror_and_the_next_one_alone() {
        let host = bitmap_booted();
        let js = r#"
__sisf_paint_reset('x'); Sisf2DContext._lastSid = 0;
var out = [];
var a = Bitmap.load('img/system/Window.png');
a.adjustTone(10, -20, 30);
out.push(a.getPixel(0, 0));
out.push(__sisf_image_pixel('0|0|img/system/Window.png'));
var b = Bitmap.load('img/system/Window.png');
out.push(b.getPixel(0, 0));
out.push(String(b._dirty));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "#ff001e|255,0,0,255|#ff0000|true",
            "第一颗变了、`__sisf_image_pixel` 那条 transport 与第二颗都还是原色 ⇒ 副本没有漏进\
             共享的那一份；第二颗 `_dirty` 是 true 钉的是它**照常建了位图**（不是没装到位图所以\
             看着像原色）"
        );
    }

    /// **`resize` 逐字，代价是尺寸赋值必须真的改到那张页**。顺带钉"上游那道 `blt` 的闸
    /// 读的是新的 `source.width`"—— 它读的就是这一格的落点。
    #[test]
    fn a_resize_moves_the_size_the_surface_and_the_guard_with_it() {
        let host = bitmap_booted();
        let js = r#"
__sisf_paint_reset('x'); Sisf2DContext._lastSid = 0;
var out = [];
var b = new Bitmap(10, 4);
b.blt(b, 0, 0, 30, 20, 0, 0);
out.push(String(JSON.parse(__sisf_paint_ops('x')).length));
b.resize(30, 20);
out.push(JSON.parse(__sisf_paint_ops('x')).slice(2).join(' ;; '));
b.blt(b, 0, 0, 30, 20, 0, 0);
var ops = JSON.parse(__sisf_paint_ops('x'));
out.push(String(ops.length) + ',' + ops[ops.length - 1]);
out.push(String(b.width) + ',' + String(b.height) + ',' + String(b.baseTexture.width));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "2|size|1|30|4| ;; size|1|30|20||5,blt|1|1||0|0|30|20|0|0|30|20|255|source-over|30,20,30",
            "第一段是「闸挡住了」：10×4 的时候 `sx + sw = 30 > 10` ⇒ 只有那两条 `size`；\
             `resize` 之后同一条调用落得下 ⇒ 4 条。`baseTexture.width` 是 30 而不是 10 —— \
             上游那一格（`:1134-1135`）写的就是这两格，而引擎的页尺寸 = 请求尺寸\
             （`atlas_gpu.rs:213`）⇒ 改尺寸在宿主那一侧是**重新分配一页**，这条判据就是它的账单"
        );
    }

    /// **CSS 关键字表**是数据形状（右值不是 `= function` ⇒ 逐字尺子与 `SHIM_BASE` 都数不到），
    /// 所以由这一格数（同 `_imageCache`/`emptyRectangle` 那条纪律）。为什么要带全表而不是
    /// "用到哪条补哪条"：`fillStyle` 的串来自**数据与插件**，一条条等玩家撞到就红了。
    #[test]
    fn the_named_color_table_is_the_css_keyword_list() {
        let host = bitmap_booted();
        let js = r#"
var out = [];
out.push(String(Object.keys(Sisf2DContext.COLORS).length));
out.push(Sisf2DContext.parseColor('rebeccapurple').join(','));
out.push(Sisf2DContext.parseColor('RGBA(255, 0, 0, 0.2)').join(','));
out.push(Sisf2DContext.parseColor('#1234abcd').join(','));
out.push((function () { try { Sisf2DContext.parseColor('not-a-color'); return 'no-throw'; }
    catch (e) { return String(e.message.indexOf('认不出的颜色写法') >= 0); } })());
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "141|102,51,153,1|255,0,0,0.2|18,52,171,0.803921568627451|true",
            "条数进账（141 颗，实测于 `shim_context.js` 的那一张表）；`RGBA(...)` 大写也认\
             （上游 canvas 的大小写不敏感）；`#1234abcd` 是带 alpha 的八位写法。\
             认不出的一律**抛**，不落成黑"
        );
    }

    /// 画法与像素那两半都落地了（这一格上一档钉的是 21 格未交 → 4 格 → 现在只剩**两格**：
    /// `blur` 与 `snap`）。它们欠的是**另一格**而不是"读不回像素"：`blur`（`:1473-1500`）的体里
    /// `tempContext.drawImage(canvas, …)` 的**源就是位图自己那颗画布**（`M10.md` 事实 88），
    /// `snap` 要 `PIXI.RenderTexture` + 场景树 ⇒ 两格一起押到「容器树」那一格，逐名钉着。
    #[test]
    fn the_filters_are_still_undelivered_and_says_which_cells() {
        let host = bitmap_booted();
        let js = r#"
var out = [];
out.push('blur,' + typeof Bitmap.prototype.blur);
out.push('snap,' + typeof Bitmap.snap);
// 反过来钉：滤镜那两格今天**回归逐字**了（第三小格），画法那十八格与读数那几格必须在场
// （漏交与多交都要红）。
var landed = ['blt','bltImage','clearRect','clear','fillRect','fillAll','gradientFillRect',
              'drawCircle','drawText','measureTextWidth','_drawTextOutline','_drawTextBody',
              'resize','getPixel','getAlphaPixel','_makeFontNameText',
              'adjustTone','rotateHue'];
var missing = 0;
for (var i = 0; i < landed.length; i++) {
    if (typeof Bitmap.prototype[landed[i]] !== 'function') missing++;
}
out.push(String(missing) + '/' + String(landed.length));
var reads = ['canvas','context','baseTexture','paintOpacity'];
var absent = 0;
for (var j = 0; j < reads.length; j++) {
    var d = Object.getOwnPropertyDescriptor(Bitmap.prototype, reads[j]);
    if (!d || typeof d.get !== 'function' || (reads[j] === 'paintOpacity' && typeof d.set !== 'function')) absent++;
}
out.push(String(absent) + '/' + String(reads.length));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "blur,undefined|snap,undefined|0/18|0/4",
            "两格 undefined 是**点名的欠账**（`blur` 要 DOM 那颗临时 canvas、`snap` 要场景树，\
             两格都押到容器树那一格）· 18 格画法与滤镜**全部在场** · 四格访问器全部有 getter\
             且 `paintOpacity` 还带 setter（上一档它们是 undefined —— 那一档的「现状」判据已经换成\
             这一档的「欠账」判据，见 `a_created_bitmap_lands_a_surface_and_records_its_size`）"
        );
    }

    /// **`checkDirty` 今天清旗不上传**：`PIXI.BaseTexture.prototype.update` 是这颗 shim 里唯一
    /// 一具空体（下一格把它换成绘制会话）。这一格钉的是"脏旗清了、但屏幕上什么都没有"这一档
    /// 现状，不许它被读成"已经接上了"。
    #[test]
    fn a_loaded_bitmap_clears_its_dirty_flag_without_an_upload() {
        let host = bitmap_booted();
        let js = r#"
var b = Bitmap.load('img/system/Window.png');
var out = [];
out.push(String(b._dirty));
out.push(String(b._baseTexture.width) + ',' + String(b._baseTexture.height));
out.push(String(b._baseTexture.scaleMode));
b.checkDirty();
out.push(String(b._dirty));
out.push(String(typeof b._baseTexture.update));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "true|2,2|0|false|function",
            "`_setDirty` 之后 `_dirty` 是 true（上游），`_baseTexture` 的两格尺寸来自图，\
             `scaleMode` 是 `PIXI.SCALE_MODES.NEAREST`（`_smooth` false ⇒ 0），\
             `checkDirty()` 把旗清掉 —— 而 update 是空的 ⇒ 这一格就是「清了但没上传」"
        );
    }

    /// 那层 shim 的**数据形状**（`= { … }`，逐字尺子只认 `= function` ⇒ 这一格由判据数）：
    /// `PIXI` 那颗全局上今天恰好**十五格**（`Graphics` 与 `RENDERER_TYPE` 是第五小格第三格加的，
    /// `extras` 是第六格第三小格加的那颗命名空间，**第四小格一次添四颗**：`glCore`/`settings`/
    /// `GC_MODES`/`tilemap` —— 上游那四句对 `PIXI` 的赋值（`rpg_core.js:5496-5499`）读的就是它们），
    /// 多一格少一格都红（与 `SHIM_BASE` 同一条双向纪律）。
    ///
    /// ⚠ 这一格**自己把容器树那一层装上**再钉名单 —— 不是凑数，是共享 realm 的形状：wasm 腿一整个
    /// 进程一个 realm，只要有一条判据先跑过 `install_display`，全局上就多了 `Container`，
    /// 于是"名单钉四格"会在**只有一条腿**上红（2026-10-05 第五小格第一格实测：native 绿、wasm 红）。
    /// 判据要么**自己装齐**再钉，要么钉**落点**而不是钉顺序（`M10.md` 事实 68⑵/79 同一条腿）。
    ///
    /// ⚠ **第三格又添了一条同源的**：钉"排序之后的名单字符串"这种写法在两条腿上不一样 ——
    /// `Rectangle` 与 `RENDERER_TYPE` 在 QuickJS 里排成前者在前、在 node（V8）里排成后者在前
    /// （实测：native 绿的那条判据 wasm 直接红，反过来说亦然）。⇒ **凡是要钉"一棵对象上恰好有哪几格"，
    /// 用两个方向各列一遍缺与多**（`want` 里没有的与 keys 里多出来的，都必须**空**）加上格数，
    /// 而不是拿 `.sort().join(',')` 的读数当期望串（`M10.md` 事实 99）。
    #[test]
    fn the_bitmap_base_texture_shim_carries_the_two_scale_modes() {
        let host = bitmap_booted();
        install_display(&host).expect("`PIXI.Container` 那一层底装不上");
        let js = r#"
var want = ['BaseTexture', 'Container', 'GC_MODES', 'Graphics', 'Point', 'RENDERER_TYPE',
    'Rectangle', 'SCALE_MODES', 'Sprite', 'Texture', 'extras', 'filters', 'glCore',
    'settings', 'tilemap'];
var keys = Object.keys(PIXI);
var missing = want.filter(function(k) { return keys.indexOf(k) < 0; });
var extra = keys.filter(function(k) { return want.indexOf(k) < 0; });
[missing.join('/'), extra.join('/'), keys.length, PIXI.SCALE_MODES.NEAREST,
    PIXI.SCALE_MODES.LINEAR, typeof PIXI.BaseTexture,
    String(new PIXI.BaseTexture({ width: 7, height: 3 }).width)].join('|')
"#;
        assert_eq!(
            num(&host, js),
            "||15|0|1|function|7",
            "前两格必须**都是空**：`want` 里缺一名就是下一颗类装载那一刻抛，`keys` 里多一名就是\
             自创面无缘无故长了一格（双向纪律的同一条腿）。后面四格是那颗命名空间的读数。\
             `Graphics` 与 `RENDERER_TYPE` 是第三格加的两颗，`extras` 是第六格第三小格加的那一颗，\
             **`glCore`/`settings`/`GC_MODES`/`tilemap` 是第四小格加的四颗**（全是数据形状 ⇒ \
             `SHIM_BASE` 数不到，只有这一条在数；一名一个理由写在 `shim_pixi.js`、`shim_display.js` \
             与 `shim_extras.js` 的文件头，而那四格**落了什么值**由 \
             `the_shader_tilemap_load_lands_the_four_gl_settings` 逐格钉）"
        );
    }

    /// 预算与上游同一格：`ImageCache.limit`（`rpg_core.js:468`）就是 CPU 像素镜像的预算 ——
    /// 两处各写一个数就会有两个真相（判据把 JS 那一格读回来对 Rust 这一格）。
    #[test]
    fn the_pixel_budget_is_the_upstream_cache_limit() {
        let host = bitmap_booted();
        assert_eq!(
            num(&host, "String(ImageCache.limit)"),
            crate::raster::PIXEL_BUDGET.to_string().as_str(),
            "`Rasters` 的像素预算必须等于上游缓存自己那一句 `limit`"
        );
    }

    /// 双装：`Bitmap` 那一格第二次装不抛，而**已经 load 好的那颗位图还能用**
    /// （与 `reinstalling_the_audio_round_is_still_the_core_layer` 同一条腿 —— wasm 腿共享 realm）。
    #[test]
    fn reinstalling_the_bitmap_round_keeps_the_loaded_bitmaps_usable() {
        let host = bitmap_booted();
        host.eval("globalThis.__b = Bitmap.load('img/system/Window.png');")
            .expect("样品位图放不下");
        assert_eq!(num(&host, "String(__b.width)"), "2");
        let mut mem = MemoryProvider::new();
        mem.insert(
            &VfsPath::new("img/system/Window.png").unwrap(),
            &crate::raster::sample_png_2x2(),
        );
        let mut vfs = Vfs::new();
        vfs.register(Box::new(mem));
        install_bitmap(&host, Rc::new(vfs)).expect("`Bitmap` 的数据那一半第二次装不上");
        assert_eq!(
            num(&host, "__b.width + ',' + __b.getPixel(0, 0)"),
            "2,#ff0000",
            "重定义类不动已存在的实例（`ImageManager._imageCache` 里那批就靠这一格活着）"
        );
    }

    /// **缓存那一条腿的收口**：上面 `truncation_fires_only_once_the_items_have_dimensions` 钉的是
    /// "样品有尺寸才裁得动"，而这一条钉的是**真位图带来的尺寸是几** —— 旧句柄那一版恒 0，
    /// `_truncateCache`（`rpg_core.js:518-534`）里那句 `sizeLeft -= bitmap.width * bitmap.height`
    /// 因此永远减不动。顺带钉两格上游语义：`get` **不问就绪**（`:484-492` 交条目本体），
    /// 而取不到的那一张落 `isError()`（旧句柄在这一格是"永远就绪、永远不报错"的谎）。
    #[test]
    fn real_bitmaps_have_a_size_the_cache_can_trim_by() {
        let host = bitmap_booted();
        let js = r#"
var out = [];
var ic = new ImageCache();
var good = Bitmap.load('img/system/Window.png');
var bad = Bitmap.load('img/pictures/big.png');
ic.add('good', good);
ic.add('bad', bad);
out.push(String(ic.get('good') === good) + ',' + String(ic.get('bad') === bad));
out.push(String(good.width * good.height) + ',' + String(bad.width * bad.height));
out.push(String(bad.isError()) + ',' + String(bad.isReady()));
out.push(String(bad.rect instanceof Rectangle) + ',' + String(bad.rect.width));
out.push(String(Object.keys(ic._items).length));
out.join('|')
"#;
        assert_eq!(
            num(&host, js),
            "true,true|4,0|true,false|true,0|2",
            "两条都以**自己的面积**进账（4 像素与 0 像素）⇒ 裁不裁从此真的由 `ImageCache.limit` 决定，\
             不再是「恒 0 ⇒ 恒不裁」；夹具里没有的那一条是 `isError()` 而不谎称就绪，\
             而它的 `rect` 仍然建得出来（0 尺寸，不抛）"
        );
    }

    /// **`TilingSprite` 脚下那一层新底：五颗具名底 + `clone`，一颗都不多**（第三小格第一条）。
    ///
    /// 这一颗是**容器树上第一颗要新底的类**（`Stage`/`Sprite`/`Window`/`Tilemap`/`Weather` 全是零新底）：
    /// 上游第一句 `Object.create(PIXI.extras.PictureTilingSprite.prototype)`（`rpg_core.js:5910`）在
    /// **装载那一刻**就要读得到 `PIXI.extras`（尺子 `tiling-sprite-face.py` 第③段：上游四文件合计 5 处
    /// `PIXI.extras`，全在这一颗的段里，而已交付文本**一颗都没有**）。
    /// 那五名之外还有一名是这一格才逼出来的：`PIXI.Rectangle.prototype.clone` —— 读者只有一处
    /// （`_refresh` 的第一句 `this._frame.clone()`，`:6079`），而 MV 的 `Rectangle` 自己只有
    /// `initialize`（`:3936` 挂的是 `PIXI.Rectangle`）⇒ 基类不给就是**一喊就抛**。
    ///
    /// ⚠ 那一条 `clone` 交回的是 **`PIXI.Rectangle`** 而不是 MV 的 `Rectangle`（上游 PIXI v4 就是这么写的）
    /// ⇒ `instanceof Rectangle` 是 `false` —— 这一格钉的就是"移植侧没有把上游的返回类型改掉"。
    #[test]
    fn the_tiling_sprite_base_is_the_measured_names_and_clone() {
        let host = tilemap_booted();
        let js = r#"
Graphics._width = 1104; Graphics._height = 736;
var out = [];
out.push(Object.keys(PIXI.extras).join(','));
out.push(['TilingSprite', 'PictureTilingSprite'].map(function(n) {
    return typeof PIXI.extras[n];
}).join(',') + '|chainFns=' + [
    typeof PIXI.extras.TilingSprite.prototype.updateTransform,
    typeof PIXI.extras.PictureTilingSprite.prototype._renderCanvas,
    typeof PIXI.extras.PictureTilingSprite.prototype._renderWebGL].join(','));
out.push(Object.getOwnPropertyNames(PIXI.extras.TilingSprite.prototype).join(',') + '||' +
    Object.getOwnPropertyNames(PIXI.extras.PictureTilingSprite.prototype).join(','));
var e = new PIXI.extras.PictureTilingSprite(new PIXI.Texture(new PIXI.BaseTexture()));
out.push([e.tilePosition.x + ',' + e.tilePosition.y, String(e.tileScale), String(e.blendMode),
    String(e.worldTransform), e.children.length, e.x + ',' + e.y, String(e.anchor),
    e.texture.frame.width + 'x' + e.texture.frame.height].join('|'));
out.push(String(Object.prototype.hasOwnProperty.call(PIXI.Container.prototype, 'tilePosition')) + ',' +
    String(new Sprite().tilePosition) + ',' +
    String(Object.getPrototypeOf(PIXI.extras.TilingSprite.prototype) === PIXI.Sprite.prototype) + ',' +
    String(PIXI.extras.TilingSprite.prototype.constructor === PIXI.extras.TilingSprite));
var r = new Rectangle(1, 2, 30, 40);
var c = r.clone();
c.width = 77;
out.push([r.x + ',' + r.y + ',' + r.width + ',' + r.height, c.x + ',' + c.width + ',' + c.height,
    String(c instanceof PIXI.Rectangle), String(c instanceof Rectangle), String(c === r)].join('|'));
out.join('\n')
"#;
        assert_eq!(
            num(&host, js),
            "TilingSprite,PictureTilingSprite\nfunction,function|chainFns=function,function,function\n\
             constructor,updateTransform||constructor,_renderCanvas,_renderWebGL\n\
             0,0|undefined|undefined|undefined|0|0,0|[object Object]|0x0\n\
             false,undefined,true,true\n1,2,30,40|1,77,40|true|false|false",
            "第一段的**两名**就是那颗命名空间的全部（`PIXI.extras` 自己是数据形状 ⇒ 逐字尺子数不到，\
             由这一条数）；第二段的三个 `function` 是那**四处别名赋值的源**（`:5916`/`:5934`/`:5935`/`:6063`）——\
             缺任何一个，装载那一刻 `undefined` 落进别名，喊它那一拍才炸；第三段是两颗底的**成员面**\
             （`constructor` 来自 `Object.create` 那条链的补句）；第四段钉的是 `tilePosition` 在场可改，\
             而 `tileScale`/`blendMode`/`worldTransform` **一个都不给**（前者全工程没人读，后两者是绘制会话那一格）；\
             第五段是**一颗一个理由**的反面对照：`tilePosition` 没有挂在 `PIXI.Container` 上\
             （否则每一只容器、每一只窗都多一颗 PIXI 常识里的字段）；末段那 `false` 是上游的形状 ——\
             `clone()` 交回 `PIXI.Rectangle`，不是 MV 的 `Rectangle`，而 `_frame` 与 `texture.frame` 之后互不牵扯"
        );
    }

    /// **`new TilingSprite(bitmap)` 落下的那一面：纹理、矩形、那颗共用的 `spriteId` 计数器**（第二条）。
    ///
    /// 读数全部来自夹具里那颗 2×2 的 `img/system/Window.png`：`_refresh` 的第一句 `this._frame.clone()`
    /// 交出**新矩形**，而 `frame.width === 0 && frame.height === 0` 那一格才拿位图尺寸顶上
    /// （`rpg_core.js:6080-6083`）⇒ 建立那一刻 `texture.frame` 是 `2x2` 而 `_frame` 还是 `0×0`。
    /// 之后 `move`/`setFrame` 各改哪一格、`_updateID` 涨几次、`tilingTexture` 每次 `_refresh` 抹成 `null`
    /// （上游靠这个让渲染器重建平铺纹理 —— 在我们这一层它是**那颗还没接上的绘制会话的名字**）。
    ///
    /// ⚠ `spriteId` 用的是 `Sprite._counter`（`:5921`）—— 与 `Sprite` **同一颗计数器**，
    /// 而共享 realm 里它的绝对值是先跑的那些判据的函数（事实 111 的同一条腿）⇒ 这一格只钉**增量**。
    #[test]
    fn the_tiling_sprite_initialize_and_frame_land_the_measured_face() {
        let host = tilemap_booted();
        let js = r#"
__sisf_paint_reset('t'); Sisf2DContext._lastSid = 0;
Graphics._width = 1104; Graphics._height = 736;
var out = [];
var before = Sprite._counter;
var bm = Bitmap.load('img/system/Window.png');
var ts = new TilingSprite(bm);
out.push([ts.x, ts.y, ts._width, ts._height, String(ts._frame instanceof Rectangle),
    ts.origin.x + ',' + ts.origin.y, ts.tilePosition.x + ',' + ts.tilePosition.y,
    String(ts.tilingTexture), String(ts.spriteId - before), String(Sprite._counter - before)].join('|'));
out.push([String(ts.texture instanceof PIXI.Texture),
    String(ts.texture.baseTexture instanceof PIXI.BaseTexture),
    ts.texture.frame.width + 'x' + ts.texture.frame.height, String(ts.texture._updateID),
    ts._frame.width + 'x' + ts._frame.height].join('|'));
out.push('counterShares=' + (function() {
    var b = new Sprite();
    var c = new TilingSprite();
    return [b.spriteId - ts.spriteId, c.spriteId - b.spriteId,
        String(Sprite._counter - before === 3)].join(',');
})());
ts.move(10, 20, 200, 100);
ts.setFrame(1, 2, 30, 40);
out.push('afterOps=' + [ts.x, ts.y, ts._width, ts._height,
    ts._frame.x + ',' + ts._frame.y + ',' + ts._frame.width + ',' + ts._frame.height,
    ts.texture.frame.x + ',' + ts.texture.frame.y + ',' + ts.texture.frame.width + ',' +
        ts.texture.frame.height, ts.texture._updateID, String(ts.tilingTexture)].join('|'));
ts._frame.width = 99;
out.push('clonedIndependently=' + ts.texture.frame.width);
ts.bitmap = null;
out.push('nullBitmap=' + [ts.texture.frame.width + 'x' + ts.texture.frame.height,
    String(ts.texture.frame === Rectangle.emptyRectangle), String(ts._bitmap)].join('|'));
out.push('ops=' + JSON.parse(__sisf_paint_ops('t')).length + '|composite=' +
    __sisf_composite_report('t'));
out.join('\n')
"#;
        assert_eq!(
            num(&host, js),
            "0|0|0|0|true|0,0|0,0|null|0|1\ntrue|true|2x2|1|0x0\ncounterShares=1,1,true\n\
             afterOps=10|20|200|100|1,2,30,40|1,2,30,40|2|null\nclonedIndependently=30\n\
             nullBitmap=0x0|true|null\nops=0|composite=applied=0;not=[];blend=[];refused=0;evict=0",
            "第一段那**四个 0** 是上游 `initialize` 写的（`_width`/`_height` 与 `x`/`y` 各两颗），\
             而 `origin`/`tilePosition` 都是 `0,0`、`tilingTexture` 是 `null`；第二段的 `2x2` 来自夹具那颗位图\
             而 `_frame` 还是 `0x0` —— 这就是 `_refresh` 里那一格「只有空矩形才拿位图尺寸顶上」的形状；\
             `counterShares` 钉的是 `TilingSprite` 与 `Sprite` **共用同一颗 `_counter`**（每建一颗涨一格）；\
             `clonedIndependently=30` 钉的是 `texture.frame` 是**另一颗矩形**（改 `_frame` 不会拖着它一起改）；\
             `nullBitmap` 那格是**共享的那颗 `emptyRectangle` 本身**（不是新建的 0×0）；\
             末段 `ops=0` 与上面那些一起读：这一颗**一笔位图操作都不写**（尺子第⑤段 `位图操作（空）`），\
             它的画整个在没接上的绘制会话那一侧（事实 98 的第三次兑现）"
        );
    }

    /// **`opacity` 与 `alpha` 是一对、变换只落到 `tilePosition`，而那两条渲染腿一条走得通一条抛**（第三条）。
    ///
    /// ⑴ `opacity` 的 getter 是 `alpha * 255`、setter 是 `value.clamp(0, 255) / 255`（`:5985-5993`）
    /// ⇒ 写 300 落回 255、写 −5 落成 0，而**改 `alpha` 那一侧也立刻反映在 `opacity` 上**（`0.5 → 127.5`）；
    /// ⑵ `updateTransform` 那两句写的是 `Math.round(-origin)`（`:6054-6055`）⇒ `12.7 → -13`，
    /// 收尾那句 `this.updateTransformTS()` 走的是新底上那颗空体 ⇒ 变换**仍然不发生**
    /// （`worldTransform` 还是 `undefined`），这就是「宿主那侧的真绘制」在这一颗上的名字；
    /// ⑶ 两条渲染腿的**守卫不一样**：`_renderCanvas` 先问 `texture.frame` 的宽高（`:5944`），
    /// 空矩形那一拍**别名一次都不喊**；而活着的 `_renderWebGL`（`:6097` 那一条）第一句之后就是
    /// `this._speedUpCustomBlendModes(renderer)` —— 那一格的源正是第二格点名跳过的那一格（事实 95）
    /// ⇒ **一喊就抛**。这一格因此钉"抛"本身：它不是 bug，而是**那条腿今天走不到**的读数
    /// （`Graphics.isWebGL()` 恒假，事实 101 的第四次应用）。
    #[test]
    fn the_tiling_sprite_opacity_and_transform_are_the_measured_pair() {
        let host = tilemap_booted();
        let js = r#"
Graphics._width = 1104; Graphics._height = 736;
var ts = new TilingSprite();
var out = [];
ts.opacity = 300;
var hi = ts.opacity;
var hiAlpha = ts.alpha;
ts.opacity = -5;
var lo = ts.opacity;
ts.alpha = 0.5;
out.push('opacity=' + hi + ',' + hiAlpha + ',' + lo + ',' + ts.opacity + ',' + ts.alpha);
ts.origin.x = 12.7;
ts.origin.y = -5.2;
ts.updateTransform();
out.push('tile=' + ts.tilePosition.x + ',' + ts.tilePosition.y + '|world=' +
    String(ts.worldTransform) + '|children=' + ts.children.length);
var hits = [];
var saved = TilingSprite.prototype._renderCanvas_PIXI;
TilingSprite.prototype._renderCanvas_PIXI = function() { hits.push('canvas'); };
ts._renderCanvas({});
var emptySkipped = hits.length;
ts.bitmap = Bitmap.load('img/system/Window.png');
ts._renderCanvas({});
TilingSprite.prototype._renderCanvas_PIXI = saved;
out.push('canvasGuard=' + emptySkipped + ',' + hits.length + ',1|restored=' +
    String(TilingSprite.prototype._renderCanvas_PIXI === saved));
var threw = 'no-throw';
try { ts._renderWebGL({}); } catch (e) { threw = 'throw'; }
out.push('webglLeg=' + threw);
ts.update();
out.push('update=' + ts.children.length + ',' + String(ts.renderable));
out.join('\n')
"#;
        assert_eq!(
            num(&host, js),
            "opacity=255,1,0,127.5,0.5\ntile=-13,5|world=undefined|children=0\n\
             canvasGuard=0,1,1|restored=true\nwebglLeg=throw\nupdate=0,true",
            "`hiAlpha` 那格钉的是** setter 真的落在 `alpha` 上**（300 被 `clamp` 夹成 255 ⇒ alpha 1）；\
             `tile=-13,5` 是 `Math.round(-origin)`（`-(-5.2)` 那一格往上取 5）；\
             `world=undefined` 与 `children=0` 一起读：**那颗空体什么变换都没做**（这一格的代价由名字点给\
             「宿主那侧的真绘制」，不是悄悄过）；`canvasGuard` 三个数分别是「空矩形那一拍没喊」、\
             「挂上位图之后喊了一次」、「总共一次」 ⇒ **守卫在别名之前**；`webglLeg=throw` 是这一格最要紧的一行：\
             它钉的正是「`_speedUpCustomBlendModes` 那一格还没交」 —— 别把它读成回归，也别顺手补一颗假底"
        );
    }

    /// **`TilingSprite` 的格面 = 上游那 16 格，且那一名重复的 `_renderWebGL` 是"最后一条赢"**（第四条）。
    ///
    /// 尺子第①段量到的是**10 条方法而只有 9 个名字**：`_renderWebGL` 定义在 `:5956` 与 `:6097` 两处，
    /// 同一句赋值 ⇒ 运行期后那一条生效（它比前一条多一句 `_speedUpCustomBlendModes`）。
    /// 上一条判据里那句 `webglLeg=throw` 就是这一条的**行为学证明**：前那一条只喊 `_renderWebGL_PIXI`，
    /// 不可能抛 ⇒ 抛的只能是后那一条（比 `String(fn)` 那种读数可靠，两条腿同形）。
    ///
    /// `16 | 14` 那一族与 `Window` 同档：`bitmap`/`opacity` 两块访问器的描述符只写 `configurable: true`
    /// ⇒ `for-in` 看不见（事实 106 在这一颗上是**第三次**兑现）；`statics=0` 与 `Weather` 同档（事实 111
    /// 那五名同样先剔掉）。
    #[test]
    fn the_tiling_sprite_member_face_is_the_upstream_class() {
        let host = tilemap_booted();
        let js = r#"
var want = ['initialize', '_renderCanvas_PIXI', '_renderWebGL_PIXI', '_renderCanvas', '_renderWebGL',
    'update', 'move', 'setFrame', 'updateTransform', 'updateTransformTS', '_onBitmapLoad', '_refresh',
    '_speedUpCustomBlendModes', 'bitmap', 'opacity'];
var own = Object.getOwnPropertyNames(TilingSprite.prototype);
var enumerable = [];
for (var k in TilingSprite.prototype) {
    if (Object.prototype.hasOwnProperty.call(TilingSprite.prototype, k)) { enumerable.push(k); }
}
var statics = Object.getOwnPropertyNames(TilingSprite).filter(function(n) {
    return ['length', 'name', 'prototype', 'arguments', 'caller'].indexOf(n) < 0;
});
var out = [];
out.push(want.filter(function(n) { return own.indexOf(n) < 0; }).join(','));
out.push(own.filter(function(n) { return n !== 'constructor' && want.indexOf(n) < 0; }).join(','));
out.push(own.length + '|' + enumerable.length + '|' + statics.length);
out.push([String(TilingSprite.prototype.constructor === TilingSprite),
    Object.getPrototypeOf(TilingSprite.prototype) === PIXI.extras.PictureTilingSprite.prototype,
    String(new TilingSprite() instanceof PIXI.Container),
    String(TilingSprite.prototype.hasOwnProperty('_renderWebGL'))].join('|'));
out.push([typeof TilingSprite.prototype._renderCanvas_PIXI,
    typeof TilingSprite.prototype._renderWebGL_PIXI, typeof TilingSprite.prototype.updateTransformTS,
    typeof TilingSprite.prototype._speedUpCustomBlendModes].join(','));
out.push(String(TilingSprite.prototype._renderCanvas_PIXI ===
    PIXI.extras.PictureTilingSprite.prototype._renderCanvas) + ',' +
    String(TilingSprite.prototype._renderWebGL_PIXI ===
    PIXI.extras.PictureTilingSprite.prototype._renderWebGL) + ',' +
    String(TilingSprite.prototype.updateTransformTS ===
    PIXI.extras.TilingSprite.prototype.updateTransform));
out.join('\n')
"#;
        assert_eq!(
            num(&host, js),
            "\n\n16|14|0\ntrue|true|true|true\nfunction,function,function,undefined\ntrue,true,true",
            "两个空串 = 一名不缺、一名不多（**15 名 + `constructor` 补句 = 16**，其中 `bitmap`/`opacity` 两块\
             访问器不可枚举 ⇒ `for-in` 只看得见 14）；第三段那 `undefined` 是**这一格唯一还欠着的别名源**\
             （第二格跳过的 `Sprite.prototype._speedUpCustomBlendModes`），而它的两处读者全在走不到的那条腿上；\
             末段三个 `true` 钉的是三条别名**指的正是新底上那一格本身**（同一颗函数，不是副本）——\
             切片器把那四处别名赋值逐字交出的兑现处"
        );
    }
    /// **装载那一刻：`ShaderTilemap` 那四句对 `PIXI` 的赋值落了什么**（第一条）。
    ///
    /// 那四句（`rpg_core.js:5496-5499`）**不写在 `ShaderTilemap` 这个名字上**，而是改 `PIXI` 的四格配置
    /// ⇒ 切片器原先一条都不认（**第十类形状**，`../sisf_pray/tools/probes/objects-slice.py` 的口径这一轮
    /// 就是为它加的），而逐字尺子同样比不到（右值不是函数 ⇒ 与 [`TILEMAP_JS`] 那 13 格静态常量、
    /// [`JSON_EX_JS`] 那 2 格同一条腿）⇒ **只有这一条判据在数它们**。四个读数就是那四句唯一的证据：
    /// `FORCE_NATIVE` 从底上的 `false`（上游 PIXI 自己的默认值，`libs/pixi.js:862` 实测）被改成 `true`、
    /// `GC_MODE` 落成 `GC_MODES.AUTO`（值照 `libs/pixi.js:8454-8457` 的 `AUTO: 0, MANUAL: 1`）、
    /// `TileRenderer.SCALE_MODE` 落成 `SCALE_MODES.NEAREST`（`0`）、`DO_CLEAR` 落成 `true`。
    ///
    /// 末段那**两个 `undefined`** 是这一格最要紧的一行，别读成缺口：`ZLayer` 与 `CompositeRectTileLayer`
    /// 是 GL 那侧的两颗真构造器，只在**构造期**被读（`_createLayers` 的 `:5604/:5605/:5610/:5612`）
    /// ⇒ 按「走不到的腿不造假底」故意不给，由下一条判据把它们的缺席钉成**抛**这个读数。
    #[test]
    fn the_shader_tilemap_load_lands_the_four_gl_settings() {
        let host = tilemap_booted();
        let js = r#"
var out = [];
out.push('FORCE_NATIVE=' + PIXI.glCore.VertexArrayObject.FORCE_NATIVE +
    '|GC_MODE=' + PIXI.settings.GC_MODE + '/' + PIXI.GC_MODES.AUTO +
    '|SCALE_MODE=' + PIXI.tilemap.TileRenderer.SCALE_MODE + '/' + PIXI.SCALE_MODES.NEAREST +
    '|DO_CLEAR=' + PIXI.tilemap.TileRenderer.DO_CLEAR);
out.push('namespaces=' + Object.keys(PIXI.glCore).join(',') + ',' +
    Object.keys(PIXI.glCore.VertexArrayObject).join(',') + ',' +
    Object.keys(PIXI.settings).join(',') + ',' + Object.keys(PIXI.tilemap).join(',') + ',' +
    Object.keys(PIXI.GC_MODES).join(','));
out.push('gcModeManual=' + PIXI.GC_MODES.MANUAL + '|nearestIsZero=' +
    String(PIXI.SCALE_MODES.NEAREST === 0));
out.push('glLayers=' + typeof PIXI.tilemap.ZLayer + ',' +
    typeof PIXI.tilemap.CompositeRectTileLayer);
out.join('\n')
"#;
        assert_eq!(
            num(&host, js),
            "FORCE_NATIVE=true|GC_MODE=0/0|SCALE_MODE=0/0|DO_CLEAR=true\n\
namespaces=VertexArrayObject,FORCE_NATIVE,GC_MODE,TileRenderer,AUTO,MANUAL\n\
gcModeManual=1|nearestIsZero=true\n\
glLayers=undefined,undefined",
            "第一行四个数**逐格对上那四句**：`FORCE_NATIVE` 被改成 `true`（底上那颗 `false` 是上游 PIXI \
             自己的默认值，`libs/pixi.js:862`）⇒ 「改写」这件事量得出来而不是恒为 `true`；\
             `GC_MODE` 与 `SCALE_MODE` 都是 `0`，但**两个 0 的来源不同**（前者读 `GC_MODES.AUTO`、\
             后者读 `SCALE_MODES.NEAREST`，第三行把那两个来源各自钉一遍：`MANUAL` 必须是 `1`、\
             `NEAREST` 必须真是 `0` ⇒ 不是巧合）；第二行是**底的面**：五颗命名空间里除了那四句写下的格\
             一个都不许多（`PIXI.settings` 只有 `GC_MODE`、`PIXI.tilemap` 只有 `TileRenderer`）——\
             末行那**两个 `undefined`** 是这一格最要紧的一行：**故意不给**的那两名（判词在文件头，\
             行为学证明在下一条判据）"
        );
    }

    /// **`new ShaderTilemap()` 当场抛在 `PIXI.tilemap.ZLayer` —— 这一条腿今天走不到**（第二条）。
    ///
    /// 尺子 `../sisf_pray/tools/probes/shader-tilemap-face.py` 第④⑤段量的就是这一格：全仓 `new` 那一句
    /// **只有一处**（`rpg_sprites.js:2300`），而它整个裹在 `if (Graphics.isWebGL())` 里 ⇒ 真流里
    /// `Spriteset_Map` 拿到的是 [`TILEMAP_JS`] 那一颗（事实 101 的**第五次**应用）。所以这一颗按待定项 2
    /// 照交，而它的构造期**不造假底**：抛就是它今天正确的样子。
    ///
    /// 怎么量到"抛在哪儿"：把那颗构造体的**第一句之前**插一行留下 `this` 的引用
    /// （`__Probed.prototype = ShaderTilemap.prototype`，其余两句与 `:5487-5490` 逐字同形），
    /// 于是半建起来的那颗对象看得见 ⇒ 四个读数钉住的是**同一条时序**：
    /// ⑴ `PIXI.Container.call(this)` 已经跑过（`children` 是空数组、`origin` 是那颗 `Point`），
    /// ⑵ `Tilemap.initialize` 的那些字段已经写完（`bitmaps` 是 `[]`），
    /// ⑶ `ShaderTilemap._createLayers` 走到了**它自己的第一句之后**（`_needsRepaint = true`，`:5599`），
    /// ⑷ 但 `addChild` 一次都没喊（`children.length == 0`），而那颗构造体的第二句
    /// `this.roundPixels = true` **也没跑到** ⇒ `roundPixels` 还是 `undefined`。
    /// 对照那一句：同一份夹具里 `new Tilemap()` 是**建得起来的**（真流那一条腿），两个读数一起才说明
    /// "红的是 GL 那两名，不是这颗类本身装不上"。
    #[test]
    fn the_shader_tilemap_constructor_stops_at_the_gl_layer_names() {
        let host = tilemap_booted();
        let js = r#"
Graphics._width = 1104; Graphics._height = 736;
var out = [];
var p = null;
globalThis.__Probed = function () { p = this; Tilemap.apply(this, arguments); this.roundPixels = true; };
__Probed.prototype = ShaderTilemap.prototype;
out.push('threw=' + (function () {
    try { new __Probed(); return 'noThrow'; } catch (e) { return String(e instanceof TypeError); }
})());
out.push('halfBuilt=' + [String(p.roundPixels), String(p.lowerZLayer), String(p.upperZLayer),
    p.children.length, String(p._needsRepaint), p.bitmaps.length,
    String(p.origin instanceof Point), String(p._layerWidth)].join('|'));
out.push('tilemapLeg=' + (function () {
    var t = new Tilemap();
    return [String(t.lowerZLayer), t.children.length, String(t.roundPixels),
        String(t instanceof Tilemap)].join('|');
})());
out.push('stillUndefined=' + [typeof PIXI.tilemap.ZLayer,
    typeof PIXI.tilemap.CompositeRectTileLayer].join(','));
out.join('\n')
"#;
        assert_eq!(
            num(&host, js),
            "threw=true\nhalfBuilt=undefined|undefined|undefined|0|true|0|true|1200\n\
tilemapLeg=undefined|2|undefined|true\nstillUndefined=undefined,undefined",
            "`threw=true` 钉的是**抛的形状**而不是消息文本（QuickJS 与 V8 的 `message` 不同串，\
             两条腿会假红；`instanceof TypeError` 才是两边同一个数）；第二行那**八个读数**是同一条时序：\
             容器与 `Tilemap` 的字段已经写完（`origin` 是那颗 `Point`、`bitmaps` 是空数组、\
             `_layerWidth` 已经从 `1104 + 20*2` 折成 `25 × 48 = 1200`），`_createLayers` 走到了\
             **它自己第一句之后**（`_needsRepaint` 已是 `true`），但 `addChild` 一次都没喊（`children` 空）、\
             两颗 Z 层是 `undefined`，而那颗构造体的第二句 `this.roundPixels = true` **也没跑到**\
             （`undefined`，不是 `false`）⇒ 停在 `:5604` 那一句的 `new` 上。第三行是对照：同一份夹具里\
             `new Tilemap()` 建得起来（`children` 是那 2 层）且它自己也没有 `roundPixels`\
             （那一格只属于这一颗）⇒ 红的是 GL 那两名，不是这颗类装不上"
        );
    }

    /// **`ShaderTilemap` 的格面 = 上游那 15 条，链挂在自己人 `Tilemap` 上**（第三条）。
    ///
    /// `Tilemap` 那一族里**只有这一颗**的链基类不是 `PIXI.…`（尺子 `pixi-face.py --chains` 第一次量出来
    /// 的就是这一格）⇒ 它必须压在 [`TILEMAP_JS`] 之后装（装载次序写在 [`install_display`] 的头注里）。
    /// 15 条里 **12 条是覆写**（`refresh`/`refreshTileset`/`updateTransform`/`_createLayers`/
    /// `_updateLayerPositions`/`_paintAllTiles`/`_paintTiles`/`_drawTile`/`_drawNormalTile`/
    /// `_drawAutotile`/`_drawTableEdge`/`_drawShadow`）、**3 条是这颗自有**（`_hackRenderer`/
    /// `renderCanvas`/`renderWebGL` —— 整个 `Tilemap` 那一族只有这一颗有渲染腿）。
    /// ⚠ 点名用 `Object.getOwnPropertyNames`（事实 106），剔掉 node 给 sloppy 函数挂的
    /// `arguments`/`caller`（事实 111）⇒ 两条腿交同一个 `statics=0`。
    /// 与 `TilingSprite` 那一格对照的一处形状：这一颗**没有一名写两遍**（15 条 = 15 名，
    /// `for-in` 与自有名同为 16 因为没有不可枚举的访问器块）。
    #[test]
    fn the_shader_tilemap_member_face_is_the_upstream_class() {
        let host = tilemap_booted();
        let js = r#"
var want = ['_hackRenderer', 'renderCanvas', 'renderWebGL', 'refresh', 'refreshTileset',
    'updateTransform', '_createLayers', '_updateLayerPositions', '_paintAllTiles', '_paintTiles',
    '_drawTile', '_drawNormalTile', '_drawAutotile', '_drawTableEdge', '_drawShadow'];
var own = Object.getOwnPropertyNames(ShaderTilemap.prototype);
var enumerable = [];
for (var k in ShaderTilemap.prototype) {
    if (Object.prototype.hasOwnProperty.call(ShaderTilemap.prototype, k)) { enumerable.push(k); }
}
var statics = Object.getOwnPropertyNames(ShaderTilemap).filter(function (n) {
    return ['length', 'name', 'prototype', 'arguments', 'caller'].indexOf(n) < 0;
});
var out = [];
out.push(want.filter(function(n) { return own.indexOf(n) < 0; }).join(','));
out.push(own.filter(function(n) { return n !== 'constructor' && want.indexOf(n) < 0; }).join(','));
out.push(own.length + '|' + enumerable.length + '|' + statics.length);
out.push([String(Object.getPrototypeOf(ShaderTilemap.prototype) === Tilemap.prototype),
    String(ShaderTilemap.prototype.constructor === ShaderTilemap),
    String(ShaderTilemap.prototype.renderCanvas !== Tilemap.prototype.renderCanvas),
    String(ShaderTilemap.prototype.initialize === Tilemap.prototype.initialize)].join('|'));
out.push('overridden=' + want.filter(function(n) {
    return Object.prototype.hasOwnProperty.call(Tilemap.prototype, n);
}).length + '|ownNew=' + want.filter(function(n) {
    return !Object.prototype.hasOwnProperty.call(Tilemap.prototype, n);
}).join(','));
out.push('ownLegs=' + String(PIXI.Container.prototype.renderCanvas ===
    ShaderTilemap.prototype.renderCanvas) + ',' + typeof ShaderTilemap);
out.join('\n')
"#;
        assert_eq!(
            num(&host, js),
            "\n\n16|16|0\ntrue|true|true|true\noverridden=12|ownNew=_hackRenderer,renderCanvas,renderWebGL\n\
ownLegs=false,function",
            "两个空串 = 一名不缺、一名不多（**15 条 = 15 名 + `constructor` 补句 = 16**，`enumerable` 同\
             16 ⇒ 这一颗**没有访问器块**，与 `TilingSprite` 的 `16|14` 正好对照）；`statics=0` 与 `Weather`\
             同档。第三行四个 `true` 钉的是链：`Object.getPrototypeOf(…prototype) === Tilemap.prototype`\
             （**这一族里唯一挂在「自己人」上的那颗** ⇒ 装载次序必须压在 `Tilemap` 之后）、\
             `initialize` **不是**覆写（这一颗没有自己的 `initialize`，构造走的是 `Tilemap` 那一份）。\
             第四行那 `12` 与三名用的是 `hasOwnProperty` 而不是 `in`：`in` 会顺着链走到\
             `PIXI.Container.prototype`，而底上**真有** `renderCanvas`/`renderWebGL` 那两格空体 ⇒\
             用 `in` 量出的 `14` 是假的「覆写数」。末行 `false` 就是这一手的兑现：那两格**是自己的**，\
             不是底上那两格"
        );
    }

    /// **GL 之外的那一半：`addRect` 的每一颗坐标、那两条渲染腿的借用，与 `refresh` 真的写 `_needsRepaint`**（第四条）。
    ///
    /// `ShaderTilemap` 的"画"不是位图操作，而是**把手里的 layer 喊一遍**（`layer.addRect(setNumber,
    /// sx, sy, dx, dy, w, h[, animX, animY])`）⇒ 这一条用一颗**记账的假 layer** 量出上游算的坐标：
    /// ⑴ `_drawNormalTile` 那两支 `setNumber`（A5 → `4`，其余 → `5 + floor(id/256)`）；
    /// ⑵ `_drawAutotile` 的**桌沿那一支**（`isTable && (qsy === 1 || qsy === 5)`）一次交**两颗**
    /// （上半 `h1` + 下半 `h1/2`，且 `qsy === 1` 时源 x 走 `(4-qsx)%4` 那一格 —— 上游 `:5460` 注释掉的
    /// `[0,3,2,1][qsx]` 与它同值）⇒ `2820` 那一颗出 **5** 笔而不是 4 笔；
    /// ⑶ `_drawTableEdge` 的两笔都是 `h1/2` 高；⑷ `_drawShadow` 的 `setNumber` 是 **-1**；
    /// ⑸ `refresh()` 是事实 112 的另一半：`Tilemap` 那一颗只抹账本，这一颗**真的写 `_needsRepaint`**，
    /// 而它喊 `refreshTileset` 的条件是 `bitmaps.length` 变了（不是位图内容变了）。
    ///
    /// 那两条渲染腿的落点是**借来的**：`this._hackRenderer(renderer)` 之后一句
    /// `PIXI.Container.prototype.renderCanvas.call(this, renderer)`（`:5523`/`:5535`）—— 底上那两格是空体
    /// ⇒ 两条腿都返回 `undefined`、什么都不画，但 `renderer.plugins.tilemap.tileAnim` 那两格**确实被写了**。
    /// `_hackRenderer` 的 `af == 3 ⇒ 1` 那一格（上游写成 `af==3`，无空格）也钉在这里：水那一张表只有
    /// `[0,1,2,1]` 四档，第 3 档要落回 1。
    #[test]
    fn the_shader_tilemap_paints_rects_into_the_layer_it_is_handed() {
        let host = tilemap_booted();
        let js = r#"
Graphics._width = 1104; Graphics._height = 736;
var out = [];
var t = Object.create(ShaderTilemap.prototype);
t._tileWidth = 48; t._tileHeight = 48; t._margin = 20;
t._width = 1144; t._height = 776;
t.origin = new Point(0, 0);
t.bitmaps = [];
t.flags = [];
t._needsRepaint = false;
var rec = [];
var layer = { addRect: function () { rec.push(Array.prototype.slice.call(arguments).join(',')); } };
var rend = { plugins: { tilemap: { tileAnim: [0, 0] } } };
t.animationFrame = 2; t._hackRenderer(rend);
var a2 = rend.plugins.tilemap.tileAnim.join(',');
t.animationFrame = 3; t._hackRenderer(rend);
var a3 = rend.plugins.tilemap.tileAnim.join(',');
t.animationFrame = 5; t._hackRenderer(rend);
var a5 = rend.plugins.tilemap.tileAnim.join(',');
out.push('hack=' + a2 + '|' + a3 + '|' + a5 + '|returns=' + String(t._hackRenderer(rend)));
out.push('legs=' + [String(t.renderCanvas(rend)), String(t.renderWebGL(rend)),
    rend.plugins.tilemap.tileAnim.join(',')].join('|'));
rec.length = 0; t._drawNormalTile(layer, Tilemap.TILE_ID_A5, 100, 200);
t._drawNormalTile(layer, Tilemap.TILE_ID_D, 96, 48);
out.push('normal=' + rec.join(' / '));
rec.length = 0; t.flags[2820] = 0x80;
t._drawAutotile(layer, 2820, 0, 0);
out.push('autotable=' + rec.length + '|' + rec.join(' / '));
rec.length = 0; t._drawTableEdge(layer, 2820, 0, 0);
out.push('edge=' + rec.join(' / '));
rec.length = 0; t._drawShadow(layer, 5, 0, 0);
out.push('shadow=' + rec.join(' / '));
rec.length = 0; t._drawShadow(layer, 0, 0, 0);
out.push('shadowNone=' + rec.length + '|visible=' +
    Tilemap.isVisibleTile(0) + ',' + Tilemap.isVisibleTile(2820) + '|drawn=' +
    (function () { t._drawTile(layer, 0, 0, 0); return rec.length; })());
t.bitmaps = [{ _baseTexture: new PIXI.BaseTexture() }, { plain: 1 }];
t._needsRepaint = false;
t.lowerLayer = { setBitmaps: function (b) { globalThis.__sb = b; } };
t.upperLayer = { setBitmaps: function (b) { globalThis.__ub = b; } };
t.refresh();
out.push('refresh=' + String(t._needsRepaint) + ',' + t._lastBitmapLength + ',' +
    String(__sb === __ub) + ',' + __sb.length + ',' + String(__sb[0] instanceof PIXI.Texture) +
    ',' + String(__sb[1].plain));
out.join('\n')
"#;
        assert_eq!(
            num(&host, js),
            "hack=96,96|48,0|48,96|returns=[object Object]\n\
legs=undefined|undefined|48,96\n\
normal=4,0,0,100,200,48,48 / 7,0,0,96,48,48,48\n\
autotable=5|1,48,96,0,0,24,24,0,0 / 1,24,96,24,0,24,24,0,0 / 1,48,72,0,24,24,24,0,0 / \
1,24,72,24,24,24,24,0,0 / 1,72,24,24,36,24,12,0,0\n\
edge=1,48,84,0,0,24,12 / 1,72,36,24,0,24,12\n\
shadow=-1,0,0,0,0,24,24 / -1,0,0,0,24,24,24\n\
shadowNone=0|visible=false,true|drawn=0\n\
refresh=true,2,true,2,true,1",
            "第一行那三档是 `af == 3 ⇒ 1` 那一格（上游写成 `af==3`，无空格）：`animationFrame` \
             `2/3/5` → `tileAnim` `96,96`/`48,0`/`48,96` —— 后两档合起来钉的是「第 3 档落回 1」与\
             「`(id % 3)` 那一支照走」；第二行两条渲染腿都交回 `undefined`，靠的是\
             `PIXI.Container.prototype.renderCanvas/renderWebGL.call` 那两格**空体**（借来的腿），\
             而 `tileAnim` 确实被写了 ⇒ 「上传动画状态」这一格是通的、「画」那一格没接上，\
             名字点给宿主那侧的真绘制。`normal` 两笔钉 `setNumber` 那两支（A5 → `4`、D → `5 + 2`）；\
             `autotable` 是这一颗最硬的一行：`2820`（A2 的 kind **16**、shape **4**，`flags` 挂上 `0x80`）\
             交的是 **5** 笔而不是 4 笔 —— 第 4 笔的 `qsy === 1` 走桌沿那一支，**同一格拆成上下两笔**\
             （`24,24` 与 `24,12`，后者 `dy1` 再抬 `h1/2` ⇒ `36`），而源 x 走 `(4-qsx)%4` 那一格；\
             `edge` 两笔的高都是 `h1/2`，第二笔的 `dy1` 是 `0`（`Math.floor(1/2)`，**不是** `+24` —— \
             这一格与 `autotable` 的 i=3 不同形，抄错就读不出）；`shadow` 那两笔的 `setNumber` 是 **-1**；\
             第五行钉两道闸：`shadowBits = 0` 一笔都不交，而 `_drawTile(0, …)` 因为\
             `isVisibleTile(0)` 是 `false` 也不交（`0 < id` 是**严格**的）。末行是 `refresh()`：\
             `_needsRepaint` 被写成 `true`（**事实 112 的另一半** —— `Tilemap` 那一颗只抹账本）、\
             `_lastBitmapLength` 记下 `2`、两层拿到的是**同一个数组**（`__sb === __ub`），\
             而 `refreshTileset` 那三元一次的两支都数到：带 `_baseTexture` 的那颗折成 `PIXI.Texture`\
             （`true`），没带的原样交回（读到的就是样品里那一格 `1`）"
        );
    }
}
