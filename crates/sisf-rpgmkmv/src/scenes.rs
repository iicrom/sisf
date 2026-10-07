//! M10.5 的**场景那一族的第一段：真流最小闭包里的四颗 `Scene_*`**（`rpg_scenes.js`）。
//!
//! 名单与体量是从闭包尺子量出来的（`../sisf_pray/tools/probes/core-touch.py --closure --no-push`
//! 的那 40 颗 / 785 条；种子 `Scene_Boot,Scene_Title,Scene_Map` 把 `Scene_Base` 一起带进来）：
//! 四颗 **89 条 prototype 方法 + 4 条原型链 + 4 句 `prototype.constructor` 补句 + 1 格函数型静态**
//! （`Scene_Boot.loadSystemImages`，`rpg_scenes.js:220-231`）。文本由
//! `../sisf_pray/tools/probes/objects-slice.py --file rpg_scenes.js
//! --classes Scene_Base,Scene_Boot,Scene_Title,Scene_Map --allow-base Stage` 切出，
//! 逐字对表在 `objects-verbatim.py`（新增的 `src/js/scenes_*.js` ↔ `rpg_scenes.js` 那一桶）。
//!
//! **为什么这四颗先交**：`SceneManager.changeScene`（`rpg_managers.js:1998-2017`，**还没交**，
//! 归 M10.5 的主循环那一格）那一拍要的就是
//! `create/start/update/isReady/stop/terminate` 这六个名字，而 `Scene_Map.fadeInForTransfer`
//! 与 `Scene_Base.isBusy` 正是旧移植侧 AGENTS 咬人第 34 条⑵那句「淡出的 24 拍挡住的是进图」
//! 在上游的出处 —— 交的是**上游那份判断**，不是移植侧手写的等价物。
//!
//! 与上游的偏差与前提（每一条都是量出来的）：
//!
//! 1. **装载那一刻只读一格**：`Scene_Base.prototype = Object.create(Stage.prototype)`
//!    （`:3-4`）⇒ [`install`] 自己先踩 [`crate::core::install_display`]（幂等，与
//!    `managers::install` 自己装缓存四颗同一条纪律）。晚一步就是 `Object.create(undefined)` 抛，
//!    而报回来的文本看不出缺谁。
//! 2. **`Stage` 是这一族唯一的链基类**（`--allow-base Stage` 那一档）：另外三颗挂在
//!    `Scene_Base.prototype` 上 ⇒ 四段文本必须**一次交**，切不开。
//! 3. **体里的外物面是 50 个跨层名 / 88 处调用点**（数法：把切片之后的文本里每一处
//!    `大写接收者.成员(` 都收进来，剔掉三型内建与本族自喊的那六颗）。
//!    整套层之上**当场喊得动的是 31 名、缺席 19 名**，名单由判据
//!    `the_scene_layer_reaches_the_rest_of_the_engine_and_names_its_own_gaps`（在 `pray/src/compat.rs`）
//!    **逐格钉死**：`SceneManager` 的泵那一半 7 格 · `BattleManager` 2 格（战斗是定案的非目标）·
//!    `ConfigManager.load` + `DataManager.setupXXX` 3 格 · 图片 URL 装载那一族 4 格
//!    （`ImageManager.loadTitle1/2` + `Graphics.isFontLoaded/canUseCssFontLoading`）·
//!    windows 那两颗的 `initCommandPosition` 2 格 + `Window_*Command` 两颗本身。**不补假底**：
//!    这些格子落下的形状是"喊到就抛"，判据把**抛**与**抛在哪一句之后**钉住
//!    （同 `M10.md` 事实 119 那条腿的第三次兑现）。
//! 4. **交出这一族会唤醒已交付文本里的一格**（本轮实测到的那条根）：
//!    [`crate::managers`] 的 `DataManager.onLoad` 里那句 `Scene_Boot.loadSystemImages()` 是**守卫而不是删句**
//!    （注释写的正是"等 M10.5 交出身体之后这一句自己会开始跑"）。身体一到，`loadDatabase` 读
//!    `System.json` 那一刻就喊 `ImageManager.reserveSystem` **十次** —— 而那一格原本不在交付面里，
//!    于是 `TypeError` 从 `onLoad` 抛穿 `loadDatabase`，它**后面那五张表一条都没读进来**
//!    （`isDatabaseLoaded()` 恒假，真库上每一条判据一起红）。落法是**同一轮补交那一格上游文本**
//!    （`rpg_managers.js:963-965` 的 `ImageManager.reserveSystem`，逐字；判据
//!    `the_boot_scene_loads_the_real_database_and_then_stops_at_the_undelivered_config` 钉住
//!    "第一句整句走完 + 十次 reserveSystem 的第一张进了缓存 + 停在第二句"四格读数）。
//! 5. **`Scene_Boot.updateDocumentTitle` 那句 `document.title = …`** 不需要底：平台件交出的
//!    `document` 是一颗 `{}`（`M10.md` 事实 97：只认 `createElement('canvas')`，探测位一个都不给），
//!    而对一颗对象**写**一个没读过的属性在 JS 里是合法赋值 ⇒ 这一格静默通过，不抛也不造假。
//!    `Scene_Boot.isGameFontLoaded` 那一格靠 [`crate::core`] 的 `Graphics.isFontLoaded`
//!    （**没交**的那一格是 `canUseCssFontLoading`，而它的读者只在"字体没到"那一条腿上）。
//! 6. **`Scene_Map` 的显示那三格（`createDisplayObjects`/`createSpriteset`/`createAllWindows`）
//!    脚下是 `Spriteset_Map` 与 `Window_Message` 那一族** —— 名字在 `rpg_sprites.js`/`rpg_windows.js`
//!    里，还没到 ⇒ 这三格今天走到就抛（`shapes` 那颗壳的 `initialize` 还没挂上游文本）。
//!    判据不许为它们造底，也不许因为"会抛"就不交这四十五格：`Scene_Map` 的判断那一半
//!    （`isReady`/`updateScene`/`fadeInForTransfer`/`updateCallMenu`…）**现在就落得下去**。

use crate::jsrt::Host;

/// `rpg_scenes.js` 的四颗 `Scene_*`：89 条方法 + 4 条链 + 4 句 `constructor` 补句 + 1 格函数型静态。
///
/// **没有一格被跳过**（切片器那次自检的读数：访问器 0 / 别名赋值 0 / 对外物赋值 0 / 未归类 0）
/// ⇒ 这一族是容器树之外**第一条「整颗交、零新底」的场景文本**：体里要的三颗类
/// （`ScreenSprite`/`WindowLayer`/`Bitmap`）与那批 manager 成员全在已交付的面上，
/// 所以 `SHIM_BASE` 这一格**不涨**（仍 64 颗）。
pub const SCENES_FLOW_JS: &str = include_str!("js/scenes_flow.js");

/// 装上场景那一族的第一段。踩在 [`crate::core::install_display`] 之后（偏差 1）。
///
/// 幂等：四段都是"在同一颗全局上重新 define"，第二次装不抛（wasm 腿共享 realm，判据
/// `reloading_the_scenes_is_still_the_scene_layer` 钉这一格）。
pub fn install(host: &Host) -> Result<(), String> {
    crate::core::install_display(host)?;
    host.eval(SCENES_FLOW_JS)
        .map_err(|e| format!("`Scene_*` 那四颗装不上：{e}"))
}

#[cfg(test)]
mod tests {
    //! fixture 判据：只用 fixture 的形状与屏幕读数（这颗 crate 不许知道某一款游戏在哪）。
    //! 整套层之上的账（真库、真闭包、跨层够得着谁）住在 `sisf_pray/src/compat.rs`。
    use super::*;
    use crate::shapes::{DeclaredShape, ShapeKind};
    use crate::{core, platform};

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    /// 这一格要的形状：四颗 `Scene_*`（`shapes::scan` 从 `rpg_scenes.js` 扫出来是 `Class`，
    /// 构造体沿用壳 —— 上游那四颗的构造体就是 `this.initialize.apply(this, arguments);`）
    /// 加上容器树那一层要的十一颗与三颗静态类。
    const SHELLS: &[(&str, ShapeKind)] = &[
        ("Scene_Base", ShapeKind::Class),
        ("Scene_Boot", ShapeKind::Class),
        ("Scene_Title", ShapeKind::Class),
        ("Scene_Map", ShapeKind::Class),
        ("Graphics", ShapeKind::StaticClass),
        ("Utils", ShapeKind::StaticClass),
        ("JsonEx", ShapeKind::StaticClass),
        ("Point", ShapeKind::Class),
        ("Rectangle", ShapeKind::Class),
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
        ("CacheEntry", ShapeKind::Class),
        ("CacheMap", ShapeKind::Class),
        ("ImageCache", ShapeKind::Class),
        ("RequestQueue", ShapeKind::Class),
        ("WebAudio", ShapeKind::Class),
        ("AudioManager", ShapeKind::StaticClass),
        ("SoundManager", ShapeKind::StaticClass),
        ("ResourceHandler", ShapeKind::StaticClass),
        ("Bitmap", ShapeKind::Class),
    ];

    /// 装到"四颗 `Scene_*` 挂上了容器树"那一档：形状 → `install`（它自己踩 core 的容器树）
    /// → 屏幕读数写成 1104×736（AGENTS 事实卡那条虚拟屏，fixture 用同一个数是为了让
    /// `createWindowLayer` 那两格除法有真读数，不是"顺手"）。
    fn scene_booted() -> crate::jsrt::Host {
        let host = Host::boot();
        let shapes: Vec<DeclaredShape> = SHELLS
            .iter()
            .map(|(n, k)| DeclaredShape {
                name: n.to_string(),
                kind: *k,
            })
            .collect();
        crate::shapes::install(&host, &shapes).expect("形状装不上");
        crate::shapes::install_extensions(&host).expect("JsExtensions 装不上");
        host.eval(platform::UTILS_JS).expect("`Utils` 装不上");
        // `Graphics` 的**屏幕读数那一格**（`boxWidth`/`boxHeight` 两块访问器）：
        // `createWindowLayer` 那两行除法读的就是它，而它住在 [`core::install`] 里、不在
        // `install_display` 里 ⇒ 本族不替调用方决定要不要交屏幕（那是 `SceneManager.initGraphics`
        // 的那一格，M10.5 的下一档），只在需要它的判据里自己写上。
        host.eval(core::GRAPHICS_JS)
            .expect("`Graphics` 的屏幕读数装不上");
        install(&host).expect("`Scene_*` 装不上");
        core::set_screen_size(&host, 1104, 736).expect("屏幕尺寸写不进 `Graphics`");
        host
    }

    /// 四颗的**链与 `constructor` 回指**：`Scene_Base` 踩在 `Stage` 上，其余三颗踩在
    /// `Scene_Base.prototype` 上；`new Scene_Boot()` 同时是 `Scene_Base` 与 `Stage`。
    #[test]
    fn the_four_scenes_chain_onto_the_stage_and_point_back_at_themselves() {
        let h = scene_booted();
        assert_eq!(
            h.eval_string(
                "[Object.getPrototypeOf(Scene_Base.prototype) === Stage.prototype, \
                  Object.getPrototypeOf(Scene_Boot.prototype) === Scene_Base.prototype, \
                  Object.getPrototypeOf(Scene_Title.prototype) === Scene_Base.prototype, \
                  Object.getPrototypeOf(Scene_Map.prototype) === Scene_Base.prototype].join(',')"
            ),
            "true,true,true,true",
            "四颗的链基类读的不是那一刻的 `prototype` 对象 ⇒ 装载次序飘了（偏差 2）"
        );
        assert_eq!(
            h.eval_string(
                "[Scene_Base.prototype.constructor === Scene_Base, \
                  Scene_Boot.prototype.constructor === Scene_Boot, \
                  Scene_Title.prototype.constructor === Scene_Title, \
                  Scene_Map.prototype.constructor === Scene_Map].join(',')"
            ),
            "true,true,true,true",
            "`constructor` 补句是切片器数到的那 4 句，少一句 `X.prototype.constructor` 就指着 Stage"
        );
        // 构造体是壳（`this.initialize.apply(this, arguments)`）⇒ `new` 得动，而且 initialize
        // 的第一句 `Stage.prototype.initialize.call(this)` 落在已交付的容器树上。
        assert_eq!(
            h.eval_string(
                "var s = new Scene_Boot(); \
                 [s instanceof Scene_Boot, s instanceof Scene_Base, s instanceof Stage, \
                  s.children.length, String(s._active), String(s._fadeDuration), \
                  typeof s._imageReservationId].join(',')"
            ),
            "true,true,true,0,false,0,number",
            "`new Scene_Boot()` 走的是上游那六句 initialize（`_active` 假、幕长 0、孩子空），\
             而 `_imageReservationId` 是 `Utils.generateRuntimeId()` 给的**数**\
             （`rpg_core.js:200` 那句是 `this._runtimeCount++`，不是字符串）"
        );
    }

    /// **成员面 = 上游那 89 条 + 4 句补句 + 1 格静态**（逐颗点名用 `hasOwnProperty` 那一族的口径：
    /// `Object.getOwnPropertyNames`，`M10.md` 事实 106/120 —— 这一族没有访问器，但同一把尺子
    /// 不能换）。静态那一格**尺子数不到**（右值是函数 ⇒ 逐字尺子认它是"函数型静态"，而
    /// `Scene_Boot` 的**成员面**里它算一格）⇒ 由这一条点名。
    #[test]
    fn the_scene_member_face_is_the_upstream_89_plus_one_static() {
        let h = scene_booted();
        // 每颗 prototype 上的**自有名**数（含 `constructor` 补句）：23/8/15/43 + 1 = 24/9/16/44。
        assert_eq!(
            h.eval_string(
                "[Scene_Base, Scene_Boot, Scene_Title, Scene_Map].map(function(c) {\
                     return Object.getOwnPropertyNames(c.prototype).length;\
                 }).join(',')"
            ),
            "24,9,16,44",
            "四颗的自有格数与切片读数（23+1 / 8+1 / 15+1 / 43+1）不符"
        );
        assert_eq!(
            h.eval_string(
                "String([Scene_Base, Scene_Boot, Scene_Title, Scene_Map].reduce(function(n, c) {\
                     return n + Object.getOwnPropertyNames(c.prototype).length - 1;\
                 }, 0))"
            ),
            "89",
            "`prototype` 方法总数就是切片器报的那 89 条"
        );
        // 上游那 89 条**一条都不能是 undefined**（别名/访问器这一族没有 ⇒ 全名可点名）。
        assert_eq!(
            h.eval_string(
                r#"var names = {
  Scene_Base: ['initialize','attachReservation','detachReservation','create','isActive','isReady',
    'start','update','stop','isBusy','terminate','createWindowLayer','addWindow','startFadeIn',
    'startFadeOut','createFadeSprite','updateFade','updateChildren','popScene','checkGameover',
    'fadeOutAll','fadeSpeed','slowFadeSpeed'],
  Scene_Boot: ['initialize','create','loadSystemWindowImage','isReady','isGameFontLoaded','start',
    'updateDocumentTitle','checkPlayerLocation'],
  Scene_Title: ['initialize','create','start','update','isBusy','terminate','createBackground',
    'createForeground','drawGameTitle','centerSprite','createCommandWindow','commandNewGame',
    'commandContinue','commandOptions','playTitleMusic'],
  Scene_Map: ['initialize','create','isReady','onMapLoaded','start','update','updateMainMultiply',
    'updateMain','isFastForward','stop','isBusy','terminate','needsFadeIn','needsSlowFadeOut',
    'updateWaitCount','updateDestination','isMapTouchOk','processMapTouch','isSceneChangeOk',
    'updateScene','createDisplayObjects','createSpriteset','createAllWindows','createMapNameWindow',
    'createMessageWindow','createScrollTextWindow','updateTransferPlayer','updateEncounter',
    'updateCallMenu','isMenuEnabled','isMenuCalled','callMenu','updateCallDebug','isDebugCalled',
    'fadeInForTransfer','fadeOutForTransfer','launchBattle','stopAudioOnBattleStart',
    'startEncounterEffect','updateEncounterEffect','snapForBattleBackground','startFlashForEncounter',
    'encounterEffectSpeed']
};
var miss = [];
Object.keys(names).forEach(function(c) {
  names[c].forEach(function(m) {
    if (typeof eval(c).prototype[m] !== 'function') { miss.push(c + '.' + m); }
  });
});
[miss.length, miss.join(' ')].join('|')"#
            ),
            "0|",
            "点名到的 89 条里有一格不是函数"
        );
        assert_eq!(
            h.eval_string("typeof Scene_Boot.loadSystemImages"),
            "function",
            "那 1 格函数型静态（`rpg_scenes.js:220`）没落位"
        );
        // 静态只有这一格：`Scene_Boot` 上除 `prototype` 之外只多这一名。
        // ⚠ **两腿的「函数自带名」不一样**（这一族第二条两腿差，同 `M10.md` 事实 99 那条腿）：
        // QuickJS 给函数对象的是 `length`/`name`/`prototype`，V8 还多 `arguments`/`caller` 两格。
        // 所以这里**不把名字写死**，而是当场取一份基线（同一台引擎上的一颗空函数）再问差集 ——
        // 钉的仍然是"额外只有 `loadSystemImages` 一格"，而跨腿读得到同一个数。
        assert_eq!(
            h.eval_string(
                "var base = Object.getOwnPropertyNames(function(){});\
                 Object.getOwnPropertyNames(Scene_Boot).filter(function(n) {\
                     return base.indexOf(n) < 0;\
                 }).join(',')"
            ),
            "loadSystemImages",
            "静态面上多出一格就是**自创**（那一格函数型静态之外，只许有这颗引擎的函数自带名）"
        );
    }

    /// **`isBusy()` 就是 `_fadeDuration > 0`**，而幕强走的是上游那支「每拍用**剩余**拍数」的除法
    /// —— 解出来正好是 `255·k/24` 的直线（旧移植侧 AGENTS 咬人第 34 条⑵那条根，现在是**上游文本**）。
    /// 这一格同时钉住「淡出的 24 拍挡住的是进图」那 24 拍的**出处**：`Scene_Base.fadeSpeed()` 就是 24。
    #[test]
    fn the_scene_fade_is_busy_for_exactly_its_duration_and_rides_a_straight_line() {
        let h = scene_booted();
        assert_eq!(
            h.eval_string("new Scene_Base().fadeSpeed() + ',' + new Scene_Base().slowFadeSpeed()"),
            "24,48",
            "`slowFadeSpeed` 是 `fadeSpeed()*2` ⇒ 48（`:131-137`），淡出那一族用的就是它"
        );
        // 淡出：opacity 从 0 起，每拍 `op += (255-op)/剩余拍` ⇒ 第 k 拍 = 255k/24。
        assert_eq!(
            h.eval_string(
                r#"var s = new Scene_Map();
s.startFadeOut(s.fadeSpeed(), false);
var out = [String(s.isBusy()), String(s._fadeSprite.opacity)];
for (var k = 0; k < 26; k++) {
  s.updateFade();
  out.push(String(s._fadeSprite.opacity) + '/' + String(s._fadeDuration));
}
out.join(',')"#
            ),
            // 期望串只钉三档：第 1 拍（255/24 = 10.625）、第 12 拍（半程 = 127.5）、第 24 拍（满幕 255）
            // 与"第 25 拍起一拍都不动"（`_fadeDuration` 到 0 就收，JS 里减成负数那一格在**淡入**那侧不存在）。
            "true,0,10.625/23,21.25/22,31.875/21,42.5/20,53.125/19,63.75/18,74.375/17,85/16,\
             95.625/15,106.25/14,116.875/13,127.5/12,138.125/11,148.75/10,159.375/9,170/8,\
             180.625/7,191.25/6,201.875/5,212.5/4,223.125/3,233.75/2,244.375/1,255/0,255/0,255/0",
            "幕强必须是 255·k/24 的直线（AGENTS 咬人第 34 条⑵），且第 24 拍交出的就是满幕 255"
        );
        assert_eq!(
            h.eval_string(
                "var s = new Scene_Map(); s.startFadeOut(24,false);\
                 for (var k=0;k<23;k++) { s.updateFade(); }\
                 [s.isBusy(), s._fadeDuration].join(',')"
            ),
            "true,1",
            "第 23 拍**还在忙** ⇒ `SceneManager.changeScene` 那一拍还过不去（上游的闸门是这一格）"
        );
        // 淡入：opacity 从 255 起、`op -= op/剩余拍` ⇒ 第 k 拍 = 255·(24-k)/24，落地那一拍交 23/24。
        assert_eq!(
            h.eval_string(
                "var s = new Scene_Map(); s.startFadeIn(24,false);\
                 [String(s._fadeSprite.opacity)].concat([0,1,2].map(function(){\
                 s.updateFade(); return String(s._fadeSprite.opacity); })).join(',')"
            ),
            "255,244.375,233.75,223.125",
            "淡入第一拍交出的是 **23/24 不是 24/24**（AGENTS 咬人第 34 条⑵那句「屏幕上第一帧」）"
        );
        assert_eq!(
            h.eval_string(
                "var s = new Scene_Base(); s.startFadeOut(0,false);\
                 [s._fadeDuration, s.isBusy()].join(',')"
            ),
            "30,true",
            "`duration || 30`（`:70`）：上游把 0 折成 30 拍，**照上游**不修成「一步都不走」（同咬人第 15 条那条腿）"
        );
    }

    /// **窗层落在"盒"上**（`createWindowLayer` 那四行除法），而 `Scene_Base.update` 是
    /// `updateFade` + `updateChildren` 两拍 ⇒ 有 `update` 的孩子被喊、没有的跳过（`WindowLayer` 有，
    /// `ScreenSprite` 没有 —— 上游就这么分的）。
    #[test]
    fn a_scene_puts_its_window_layer_on_the_box_and_updates_only_children_that_can() {
        let h = scene_booted();
        // 盒 == 屏（`set_screen_size` 那五行把 `_boxWidth/_boxHeight` 写成同一对数），
        // 所以 x/y 落 0；把盒改小一格才看得出那两处除法。
        h.eval("Graphics._boxWidth = 1040; Graphics._boxHeight = 688;")
            .expect("盒尺寸写不进 `Graphics`");
        assert_eq!(
            h.eval_string(
                r#"var s = new Scene_Base();
s.createWindowLayer();
s.addWindow(new ScreenSprite());
[String(s.children.length), s._windowLayer instanceof WindowLayer,
 String(s.children[0] === s._windowLayer),
 [s._windowLayer.x, s._windowLayer.y, s._windowLayer.width, s._windowLayer.height].join(':'),
 String(s._windowLayer.children.length)].join(',')"#
            ),
            "1,true,true,32:24:1040:688,1",
            "`move((1104-1040)/2, (736-688)/2, 1040, 688)` 的四格读数，加 `addWindow` 的那一格方向：\
             **窗进的是层**（`_windowLayer.children` = 1），场景那一侧仍然只有那一只层（children = 1）"
        );
        // 数孩子被喊的次数：WindowLayer 有 `update`（`:59`）、ScreenSprite 没有 ⇒ 一喊一不喊。
        assert_eq!(
            h.eval_string(
                r#"var s = new Scene_Base();
s.createWindowLayer();
s.addWindow(new ScreenSprite());
var calls = 0;
var real = WindowLayer.prototype.update;
s._windowLayer.update = function() { calls++; real.call(this); };
s.update();
s.update();
[calls, String(s.children.length)].join(',')"#
            ),
            "2,1",
            "`updateChildren` 的 `if (child.update)` 那一格：两拍两喊（`WindowLayer` 有 `update`），\
             而那颗 `ScreenSprite` 一次都不挨（它没有 `update`）；场景那一侧的孩子仍然是那一只层"
        );
    }

    /// **幂等**（这一族「手交的 JS 要能装第二次」那条规矩）：同一个 realm 里装第二次不抛，
    /// 而 `new Scene_Map()` 之后**旧的那颗不受影响**（原型换成了同一批文本 ⇒ 行为不变）。
    #[test]
    fn reloading_the_scenes_is_still_the_scene_layer() {
        let h = scene_booted();
        let before = h.eval_string("String(Scene_Map.prototype.updateMain.toString().length)");
        install(&h).expect("第二次装不上（共享 realm 里重 define 不许抛）");
        assert_eq!(
            h.eval_string("String(Scene_Map.prototype.updateMain.toString().length)"),
            before,
            "第二次装之后 `updateMain` 换了文本 ⇒ 幂等没落在「同一批 define」上"
        );
        assert_eq!(
            h.eval_string(
                "var s = new Scene_Map(); s.initialize(); String(s._mapLoaded) + ',' + String(s._waitCount)"
            ),
            "false,0",
            "重复装载之后 `initialize` 仍然是上游那五句"
        );
    }

    /// **走不到的腿照交、底不造假**：`Scene_Boot.create` 的第三句踩在没交的 `ConfigManager.load` 上，
    /// 而 `Scene_Map.createDisplayObjects` 踩在没交的 `Spriteset_Map` 上。两条判据把**抛**本身钉住
    /// （读数 = 抛之前已经落下的那半截），因为补假底会把"下一格交什么"变成看不见的账
    /// （`M10.md` 事实 119 那条腿的第二次兑现，也是偏差 3/6 的落点）。
    #[test]
    fn the_unreached_scene_steps_stop_at_the_names_we_have_not_delivered_and_say_which() {
        let h = scene_booted();
        // **这一档只钉「停在哪一格」**：fixture 里没有 managers，`Scene_Boot.create` 的第二句
        // `DataManager.loadDatabase()` 就是它够不到的第一颗名字 ⇒ 抛的文本里必须点得出那颗名。
        // 整套层之上的读数（managers 在场、`loadDatabase` 落得下去，而第三句停在
        // `ConfigManager.load` 那一格没交的成员上）住在 pray 的
        // `the_scene_layer_reaches_the_rest_of_the_engine_and_names_its_own_gaps`。
        let err = h
            .try_eval_string("var s = new Scene_Boot(); s.create(); 'no-throw'")
            .expect_err("Scene_Boot.create 在没有 managers 的 fixture 上必须抛");
        assert!(
            err.contains("DataManager"),
            "抛的不是第一颗够不到的接收者，读不出停在哪一格：{err}"
        );
        // `Scene_Map` 的**判断**那一半现在就落得下去（偏差 6）：`isReady` 走的是
        // `Scene_Base.isReady` → `ImageManager.isReady`，这一格由 managers 交，不在这一档 ⇒
        // 这里钉的是它**之前**的两格读数（`_mapLoaded` 的翻转只看 `DataManager.isMapLoaded`）。
        assert_eq!(
            h.eval_string(
                "var s = new Scene_Map(); s.initialize(); \
                 [String(s._mapLoaded), String(s._touchCount), String(s._encounterEffectDuration)].join(',')"
            ),
            "false,0,0",
            "`Scene_Map.initialize` 那四句落位（这一格与 managers 无关）"
        );
    }

    /// `install` 不要求调用方先装容器树（偏差 1 的正面判据）：只给形状，本族自己踩 core。
    #[test]
    fn scenes_install_does_not_depend_on_the_caller_installing_the_display_layer_first() {
        let host = Host::boot();
        let shapes: Vec<DeclaredShape> = SHELLS
            .iter()
            .map(|(n, k)| DeclaredShape {
                name: n.to_string(),
                kind: *k,
            })
            .collect();
        crate::shapes::install(&host, &shapes).expect("形状装不上");
        // **不**先调 core::install_display —— 调了就把这一格的内部前提交给调用方去记。
        install(&host).expect("`Scene_*` 自己没把容器树踩上");
        assert!(
            host.eval_bool("Object.getPrototypeOf(Scene_Base.prototype) === Stage.prototype"),
            "容器树没被 install 自己装上"
        );
    }
}
