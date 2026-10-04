//! M10.2 的**平台件**：`Utils` / `Input` / `TouchInput` / `console` / `window` 那一族宿主面。
//!
//! ## 这一族为什么单独一格
//!
//! M10.1 的装载账单（`pray/src/compat.rs`）把"只交形状还差什么"点到了名字一级：挂掉的六条里
//! 四条挂在**宿主**而不是 MV 的类上 —— `Utils.isNwjs`/`isMobileDevice`（`TRP_ParticleEditor:71`、
//! `MPP_EncounterEffect:1448`）、`Input.keyMapper`（`PG_koma_god:4`）、`console`
//! （`FTKR_SkillTreeSystem:1023`）、`window`（`Mano_OneButtonCommonEventCall:262`）。
//! 这一格把**这些名字**交出来。哪些成员该交、各被碰了几次，由
//! `tools/probes/platform-touch.py` 量（判据里引用的数字都能被它照抄复现）。
//!
//! ## 三条写法纪律
//!
//! 1. **逐字照上游**，并把 `rpg_core.js` 的行号写在这一格旁边。凡是我"改良"过的地方都必须在这里
//!    留下名字与理由 —— 兼容层的错最贵的形状是"看起来对"。
//! 2. **缺什么才补什么**：每一个宿主面都写成 `typeof … === "undefined"` 守卫。浏览器腿（真 wasm）
//!    本来就带 `window`/`navigator`/`console`，守卫让它继续用自己的；native（QuickJS）与 wasm 的
//!    测试腿（node）没有，才落我们这一份。⇒ **两条腿同一个答案，但不是同一份实现**，
//!    这正是 `jsrt` 模块头那条"同一份 JS 文本"的用法。
//! 3. **不造 API**：Rust 把输入喂进兼容层时走的必须是**上游那几个私有可能接收事件对象的方法**
//!    （`Input._onKeyDown`/`_onKeyUp`/`_onLostFocus`、`TouchInput._onTrigger`/`_onCancel`/
//!    `_onMove`/`_onRelease`/`_onWheel`），参数还是上游那个事件形状。于是"sisf 的输入"与"浏览器的
//!    输入"落进同一套状态机，而逐帧时序（`keyRepeatWait`/`keyRepeatInterval`）不需要重写一遍。
//!
//! ## 有意没交的那几格（每一格都是量过才留下的）
//!
//! - `Utils.canReadGameFiles`（`rpg_core.js:254`）与 `Utils.isSupportPassiveEvent`（`:298`）：
//!   前者要 `document.getElementsByTagName` + `XMLHttpRequest`，后者要 `window.addEventListener`。
//!   启用插件里 **0 条**调用（platform-touch 普查的 `Utils.*` 名单）。
//! - `Input._setupEventHandlers`（`:3224`）/ `TouchInput._setupEventHandlers`（`:3689`）与它那一族
//!   DOM 监听器：`document.addEventListener` 只有 1 条插件碰过（TRP_ParticleEditor 的编辑器腿）。
//!   事件从 Rust 侧进来，所以监听器那一族不留 —— 不留 `document` 也一样：装了假的 `document`
//!   反而会让"有没有 DOM"这一族特性探测走上浏览器那条路。
//! - `TouchInput._onLeftButtonDown`/`_onRightButtonDown`/`_onMouseMove`/`_onMouseUp`（`:3724-3784`）：
//!   它们的第一件事是 `Graphics.pageToCanvasX(event.pageX)`（M10.4 那一格），而 sisf 的指针本来就是
//!   画布坐标 ⇒ 走 `_onTrigger(x, y)`。
//! - `Graphics` / `Stage` / `PIXI` / `Bitmap` 那一整族：**不属于平台件**，是渲染面，记给 M10.4
//!   （顶层就碰它的只有 `TRP_Particle` 的 UMD 打包件，见 `../sisf_pray/M10.md`「M10.2 验收」）。
//! - `Utils.isNwjs` 是这一格里**唯一**没照抄的函数体（见它下面的注释）。

use crate::jsrt::Host;

/// 宿主全局面：QuickJS 与 node 没有的那几格，补成**桌面浏览器的答案**。
///
/// `window = globalThis` 这一条不是偷懒：浏览器里 `window` 与 `globalThis` 本来就是同一个对象，
/// 所以 `window.setHandler(…)`（FTKR 那一族 8 处）、`window.convertEscapeCharacters(…)`、
/// `window[名字] = 类`（`Mano_OneButtonCommonEventCall:262`）在真浏览器里就是普通的**全局读写**。
/// 把 `window` 指向全局对象 ⇒ 这一族在两条腿上的落点与浏览器一致；造一个独立的假 `window`
/// 反而会让"写进 window 的东西"读不回来（那 11 条插件的写法全落空）。
///
/// `location.search` 只交一条：上游 `Utils.isOptionValid`（`rpg_core.js:193-197`）读的就是这一格，
/// 而启用插件里 `location.*`/`localStorage.*` 各 0 处（platform-touch）。
pub const HOST_GLOBALS_JS: &str = r#"
if (typeof globalThis.window === "undefined") { globalThis.window = globalThis; }
if (typeof globalThis.self === "undefined") { globalThis.self = globalThis; }
if (typeof globalThis.location === "undefined") { globalThis.location = { search: "" }; }
if (typeof globalThis.navigator === "undefined") {
    globalThis.navigator = { userAgent: "Mozilla/5.0 (sisf-rpgmkmv; desktop)" };
}
"#;

/// `console`：**永远记一本账**，并且在宿主本来带真 `console` 的时候**转调它**。
///
/// 为什么不能"只在没有的时候补一颗"：装载账单只看"eval 过没过顶层"，而插件有一种合法的自己放弃
/// 的写法 —— 实测第一例是 `FTKR_SkillTreeSystem:1022-1026`：
/// ```text
/// if (!Window_Base.prototype.reserveFaceImages) { console.error('…コアスクリプトを最新版に…'); return; }
/// ```
/// 它喊一句就 `return` ⇒ 顶层 eval 成功 ⇒ 账单把它读成"起来了"，而整个插件体一条都没落。
/// 所以账必须**按插件归堆**（[`crate::plugins::LoadReport::logged`]），"起得来"与"起得来但自己放弃了"
/// 才是两个数。转调那一句让浏览器腿继续把消息送到 devtools（上游的形状），记账这一格不外传。
pub const CONSOLE_JS: &str = r#"
globalThis.__sisf_console = globalThis.__sisf_console || [];
globalThis.__sisf_consoleText = function (v) {
    if (typeof v === "string") { return v; }
    if (typeof v === "object" && v !== null) {
        try { return JSON.stringify(v); } catch (e) { return String(v); }
    }
    return String(v);
};
globalThis.__sisf_consolePush = function (level, args) {
    var parts = [];
    for (var i = 0; i < args.length; i++) { parts.push(globalThis.__sisf_consoleText(args[i])); }
    globalThis.__sisf_console.push([level, parts.join(" ")]);
    if (globalThis.__sisf_console.length > 256) { globalThis.__sisf_console.shift(); }
};
(function () {
    // **只认第一次**：`__sisf_consoleOrig` 存的是宿主原本那颗 console（QuickJS 这一腿是 null）。
    // 不存这一格，第二次装载就会把"我们自己那颗"当原件转调 ⇒ 同一行记两遍。
    if (globalThis.__sisf_consoleOrig === undefined) {
        globalThis.__sisf_consoleOrig =
            typeof globalThis.console === "object" && globalThis.console !== null
            && typeof globalThis.console.error === "function" ? globalThis.console : null;
    }
    var orig = globalThis.__sisf_consoleOrig;
    var one = function (level) {
        return function () {
            globalThis.__sisf_consolePush(level, arguments);
            if (orig && typeof orig[level] === "function") { orig[level].apply(orig, arguments); }
        };
    };
    var c = {};
    var names = ["log", "info", "warn", "error", "debug", "trace", "dir", "table",
                 "group", "groupCollapsed", "groupEnd", "count"];
    for (var i = 0; i < names.length; i++) { c[names[i]] = one(names[i]); }
    c.assert = function (condition) {
        if (!condition) {
            globalThis.__sisf_consolePush("assert", Array.prototype.slice.call(arguments, 1));
        }
        if (orig && typeof orig.assert === "function") { orig.assert.apply(orig, arguments); }
    };
    // `time`/`timeEnd` 记的是"经过了多少"，所以账本里落的是同一句标签加实测毫秒（上游同形）。
    var starts = {};
    c.time = function (label) { starts[globalThis.__sisf_consoleText(label)] = Date.now(); };
    c.timeEnd = function (label) {
        var key = globalThis.__sisf_consoleText(label);
        if (typeof starts[key] === "number") {
            globalThis.__sisf_consolePush("timeEnd", [label, Date.now() - starts[key] + "ms"]);
            delete starts[key];
        }
        if (orig && typeof orig.timeEnd === "function") { orig.timeEnd.apply(orig, arguments); }
    };
    globalThis.console = c;
})();
"#;

/// `Utils`（`rpg_core.js:161-310`）。除 `isNwjs` 以外**逐字照上游**。
///
/// **`isNwjs` 是这一族唯一没照抄的函数体**（`:206-208` 写的是
/// `typeof require === 'function' && typeof process === 'object'`）：那一格在浏览器里回答 false，
/// 而在 node（本工程 wasm 腿的测试环境）里 `require`/`process` 都是真的 ⇒ 同一段代码会回答 true，
/// 于是两条腿走上两条路（nwjs 那一族走 `fs` 读盘、浏览器那一族走 XHR）。兼容层跑在 sisf 上，
/// **它既不是 nwjs 也不是 DOM 浏览器**，所以这一格的答案由兼容层定：**恒 false**。
/// 这一条是"两边同一个数"的纪律，不是省事 —— 改了回去（照抄那两句）第一个红的就是 node 腿。
pub const UTILS_JS: &str = r#"
Utils.RPGMAKER_NAME = 'MV';
Utils.RPGMAKER_VERSION = "1.6.1";
Utils.isOptionValid = function (name) {
    if (location.search.slice(1).split('&').contains(name)) { return 1; }
    if (typeof nw !== "undefined" && nw.App.argv.length > 0 && nw.App.argv[0].split('&').contains(name)) { return 1; }
    return 0;
};
Utils.isNwjs = function () { return false; };
Utils.isMobileDevice = function () {
    var r = /Android|webOS|iPhone|iPad|iPod|BlackBerry|IEMobile|Opera Mini/i;
    return !!navigator.userAgent.match(r);
};
Utils.isMobileSafari = function () {
    var agent = navigator.userAgent;
    return !!(agent.match(/iPhone|iPad|iPod/) && agent.match(/AppleWebKit/) && !agent.match('CriOS'));
};
Utils.isAndroidChrome = function () {
    var agent = navigator.userAgent;
    return !!(agent.match(/Android/) && agent.match(/Chrome/));
};
Utils.rgbToCssColor = function (r, g, b) {
    r = Math.round(r);
    g = Math.round(g);
    b = Math.round(b);
    return 'rgb(' + r + ',' + g + ',' + b + ')';
};
Utils._id = 1;
Utils.generateRuntimeId = function () { return Utils._id++; };
"#;

/// `Input`（`rpg_core.js:2961-3440`）：键名表 + 状态机 + 四条判据 + 方向。逐字照上游。
///
/// 三条要紧的形状：
/// - `keyMapper`/`gamepadMapper` 是**普通对象**，插件会就地改一格（`PG_koma_god:4` 写
///   `Input.keyMapper[18] = 'alt'`）⇒ 不许做成冻结表或 Rust 侧只读门面。
/// - 重复键的时序是 `keyRepeatWait = 24` / `keyRepeatInterval = 6`（`:2984`、`:2993`），
///   而 `_pressedTime` 在 `update()` 里加 ⇒ 判据必须在**同一份状态**上走拍（`update` 的调用者
///   是 `SceneManager.updateMain`，M10.5 那一格）。
/// - `dir4`/`dir8`/`date` 上游是 `Object.defineProperty(Input, …, {get, configurable: true})`
///   （`:3166-3199`）⇒ 照抄定义方式，读的是 `_dir4`/`_dir8`/`_date` 三个字段本身。
///
/// `initialize`（`:2971-2975`）只留 `clear()`：另外两句 `_wrapNwjsAlert()`（`isNwjs()` 恒 false ⇒
/// 上游自己也是空转）与 `_setupEventHandlers()`（DOM 监听，见模块头"有意没交"）。
pub const INPUT_JS: &str = r#"
Input.keyRepeatWait = 24;
Input.keyRepeatInterval = 6;
Input.keyMapper = {
    9: 'tab',
    13: 'ok',
    16: 'shift',
    17: 'control',
    18: 'control',
    27: 'escape',
    32: 'ok',
    33: 'pageup',
    34: 'pagedown',
    37: 'left',
    38: 'up',
    39: 'right',
    40: 'down',
    45: 'escape',
    81: 'pageup',
    87: 'pagedown',
    88: 'escape',
    90: 'ok',
    96: 'escape',
    98: 'down',
    100: 'left',
    102: 'right',
    104: 'up',
    120: 'debug'
};
Input.gamepadMapper = {
    0: 'ok',
    1: 'cancel',
    2: 'shift',
    3: 'menu',
    4: 'pageup',
    5: 'pagedown',
    12: 'up',
    13: 'down',
    14: 'left',
    15: 'right'
};
Input.initialize = function () { this.clear(); };
Input.clear = function () {
    this._currentState = {};
    this._previousState = {};
    this._gamepadStates = [];
    this._latestButton = null;
    this._pressedTime = 0;
    this._dir4 = 0;
    this._dir8 = 0;
    this._preferredAxis = '';
    this._date = 0;
};
Input.update = function () {
    this._pollGamepads();
    if (this._currentState[this._latestButton]) {
        this._pressedTime++;
    } else {
        this._latestButton = null;
    }
    for (var name in this._currentState) {
        if (this._currentState[name] && !this._previousState[name]) {
            this._latestButton = name;
            this._pressedTime = 0;
            this._date = Date.now();
        }
        this._previousState[name] = this._currentState[name];
    }
    this._updateDirection();
};
Input.isPressed = function (keyName) {
    if (this._isEscapeCompatible(keyName) && this.isPressed('escape')) { return true; }
    return !!this._currentState[keyName];
};
Input.isTriggered = function (keyName) {
    if (this._isEscapeCompatible(keyName) && this.isTriggered('escape')) { return true; }
    return this._latestButton === keyName && this._pressedTime === 0;
};
Input.isRepeated = function (keyName) {
    if (this._isEscapeCompatible(keyName) && this.isRepeated('escape')) { return true; }
    return (this._latestButton === keyName &&
            (this._pressedTime === 0 ||
             (this._pressedTime >= this.keyRepeatWait &&
              this._pressedTime % this.keyRepeatInterval === 0)));
};
Input.isLongPressed = function (keyName) {
    if (this._isEscapeCompatible(keyName) && this.isLongPressed('escape')) { return true; }
    return (this._latestButton === keyName && this._pressedTime >= this.keyRepeatWait);
};
Object.defineProperty(Input, 'dir4', {
    get: function () { return this._dir4; },
    configurable: true
});
Object.defineProperty(Input, 'dir8', {
    get: function () { return this._dir8; },
    configurable: true
});
Object.defineProperty(Input, 'date', {
    get: function () { return this._date; },
    configurable: true
});
Input._pollGamepads = function () {
    if (navigator.getGamepads) {
        var gamepads = navigator.getGamepads();
        if (gamepads) {
            for (var i = 0; i < gamepads.length; i++) {
                var gamepad = gamepads[i];
                if (gamepad && gamepad.connected) {
                    this._updateGamepadState(gamepad);
                }
            }
        }
    }
};
Input._updateGamepadState = function (gamepad) {
    var lastState = this._gamepadStates[gamepad.index] || [];
    var newState = [];
    var buttons = gamepad.buttons;
    var axes = gamepad.axes;
    var threshold = 0.5;
    newState[12] = false;
    newState[13] = false;
    newState[14] = false;
    newState[15] = false;
    for (var i = 0; i < buttons.length; i++) {
        newState[i] = buttons[i].pressed;
    }
    if (axes[1] < -threshold) { newState[12] = true; }
    else if (axes[1] > threshold) { newState[13] = true; }
    if (axes[0] < -threshold) { newState[14] = true; }
    else if (axes[0] > threshold) { newState[15] = true; }
    for (var j = 0; j < newState.length; j++) {
        if (newState[j] !== lastState[j]) {
            var buttonName = this.gamepadMapper[j];
            if (buttonName) { this._currentState[buttonName] = newState[j]; }
        }
    }
    this._gamepadStates[gamepad.index] = newState;
};
Input._updateDirection = function () {
    var x = this._signX();
    var y = this._signY();
    this._dir8 = this._makeNumpadDirection(x, y);
    if (x !== 0 && y !== 0) {
        if (this._preferredAxis === 'x') { y = 0; } else { x = 0; }
    } else if (x !== 0) {
        this._preferredAxis = 'y';
    } else if (y !== 0) {
        this._preferredAxis = 'x';
    }
    this._dir4 = this._makeNumpadDirection(x, y);
};
Input._signX = function () {
    var x = 0;
    if (this.isPressed('left')) { x--; }
    if (this.isPressed('right')) { x++; }
    return x;
};
Input._signY = function () {
    var y = 0;
    if (this.isPressed('up')) { y--; }
    if (this.isPressed('down')) { y++; }
    return y;
};
Input._makeNumpadDirection = function (x, y) {
    if (x !== 0 || y !== 0) { return 5 - y * 3 + x; }
    return 0;
};
Input._isEscapeCompatible = function (keyName) {
    return keyName === 'cancel' || keyName === 'menu';
};
Input._shouldPreventDefault = function (keyCode) {
    switch (keyCode) {
    case 8:
    case 33:
    case 34:
    case 37:
    case 38:
    case 39:
    case 40:
        return true;
    }
    return false;
};
// 下面三条就是**输入的门**：Rust 侧交一个上游形状的事件对象（`keyCode` + `preventDefault`）进来。
Input._onKeyDown = function (event) {
    if (this._shouldPreventDefault(event.keyCode)) { event.preventDefault(); }
    if (event.keyCode === 144) { this.clear(); }
    var buttonName = this.keyMapper[event.keyCode];
    if (ResourceHandler.exists() && buttonName === 'ok') {
        ResourceHandler.retry();
    } else if (buttonName) {
        this._currentState[buttonName] = true;
    }
};
Input._onKeyUp = function (event) {
    var buttonName = this.keyMapper[event.keyCode];
    if (buttonName) { this._currentState[buttonName] = false; }
    if (event.keyCode === 0) { this.clear(); }
};
Input._onLostFocus = function () { this.clear(); };
"#;

/// `TouchInput`（`rpg_core.js:3448-3682`）的**状态那一半**：清、逐拍、七条判据、五个只读属性、
/// 四个 `_onXxx(x, y)` 门。DOM 监听器那一半不交（模块头"有意没交"）。
///
/// 与上游同一个字段名集（`_mousePressed`/`_screenPressed`/`_events`/`_triggered`…），因为
/// `_events` 是"这一拍里累加、下一次 `update()` 搬到公开位"的两段形状 —— 只读属性读的是搬过的那一份。
pub const TOUCH_INPUT_JS: &str = r#"
TouchInput.keyRepeatWait = 24;
TouchInput.keyRepeatInterval = 6;
TouchInput.initialize = function () { this.clear(); };
TouchInput.clear = function () {
    this._mousePressed = false;
    this._screenPressed = false;
    this._pressedTime = 0;
    this._events = {};
    this._events.triggered = false;
    this._events.cancelled = false;
    this._events.moved = false;
    this._events.released = false;
    this._events.wheelX = 0;
    this._events.wheelY = 0;
    this._triggered = false;
    this._cancelled = false;
    this._moved = false;
    this._released = false;
    this._wheelX = 0;
    this._wheelY = 0;
    this._x = 0;
    this._y = 0;
    this._date = 0;
};
TouchInput.update = function () {
    this._triggered = this._events.triggered;
    this._cancelled = this._events.cancelled;
    this._moved = this._events.moved;
    this._released = this._events.released;
    this._wheelX = this._events.wheelX;
    this._wheelY = this._events.wheelY;
    this._events.triggered = false;
    this._events.cancelled = false;
    this._events.moved = false;
    this._events.released = false;
    this._events.wheelX = 0;
    this._events.wheelY = 0;
    if (this.isPressed()) { this._pressedTime++; }
};
TouchInput.isPressed = function () { return this._mousePressed || this._screenPressed; };
TouchInput.isTriggered = function () { return this._triggered; };
TouchInput.isRepeated = function () {
    return (this.isPressed() &&
            (this._triggered ||
             (this._pressedTime >= this.keyRepeatWait &&
              this._pressedTime % this.keyRepeatInterval === 0)));
};
TouchInput.isLongPressed = function () {
    return this.isPressed() && this._pressedTime >= this.keyRepeatWait;
};
TouchInput.isCancelled = function () { return this._cancelled; };
TouchInput.isMoved = function () { return this._moved; };
TouchInput.isReleased = function () { return this._released; };
Object.defineProperty(TouchInput, 'wheelX', {
    get: function () { return this._wheelX; },
    configurable: true
});
Object.defineProperty(TouchInput, 'wheelY', {
    get: function () { return this._wheelY; },
    configurable: true
});
Object.defineProperty(TouchInput, 'x', {
    get: function () { return this._x; },
    configurable: true
});
Object.defineProperty(TouchInput, 'y', {
    get: function () { return this._y; },
    configurable: true
});
Object.defineProperty(TouchInput, 'date', {
    get: function () { return this._date; },
    configurable: true
});
// 鼠标/触屏的门：坐标已经是画布坐标（上游在 `_onLeftButtonDown` 里才换算，那一格记给 M10.4）。
TouchInput._onTrigger = function (x, y) {
    this._events.triggered = true;
    this._x = x;
    this._y = y;
    this._date = Date.now();
};
TouchInput._onCancel = function (x, y) {
    this._events.cancelled = true;
    this._x = x;
    this._y = y;
};
TouchInput._onMove = function (x, y) {
    this._events.moved = true;
    this._x = x;
    this._y = y;
};
TouchInput._onRelease = function (x, y) {
    this._events.released = true;
    this._x = x;
    this._y = y;
};
TouchInput._onWheel = function (event) {
    this._events.wheelX += event.deltaX;
    this._events.wheelY += event.deltaY;
    event.preventDefault();
};
"#;

/// `ResourceHandler` 的**一格**：`exists()`（`rpg_core.js:9308-9310`，读 `_reloaders`）。
///
/// 只要这一格是因为 `Input._onKeyDown` 无条件问它一句（`:3244`）。上游的 `retry()`（`:3312`）
/// 要 `Graphics.eraseLoadingError` + `SceneManager.resume` ⇒ 那一族属于"资源加载失败的那面窗"，
/// 记给 M10.4；`_reloaders` 在有人 push 之前恒空 ⇒ `exists()` 恒 false ⇒ 走不到那一支。
pub const RESOURCE_HANDLER_JS: &str = r#"
ResourceHandler._reloaders = [];
ResourceHandler.exists = function () { return this._reloaders.length > 0; };
"#;

/// ECMA-262 **Annex B 的 `RegExp.$1`..`$9`**：浏览器与 node 都带，QuickJS **不带**（实测
/// `typeof RegExp.$1 === 'undefined'`，而 `'a'.match(/(a)/)` 本身正常返回）。
///
/// 为什么兼容层要交这一格，数字来自 `tools/probes/regexp-statics.py`：上游 **128 处**引用、
/// **9 条**启用插件，写法固定是
/// `if (line.match(/<(?:MAX ITEM):[ ](\d+)>/i)) obj.maxItem = parseInt(RegExp.$1);`
/// ⇒ 缺它**不抛**，只是 `parseInt(undefined)` 变成 `NaN`，于是 notetag 一族静默失效
/// （实测第一例：`YEP_CoreEngine` 给道具落的 `maxItem` 读数是 0，而磁盘上那条 `<Max Item: 30>`
/// 该落 30、启用参数 `Default Max` 该落 999）。这一族正是"eval 过了顶层、读数看着还是绿的"那一类。
///
/// 三条形状：
/// 1. **交给守卫**：宿主本来带（浏览器腿、node 腿都带）就继续用自己的 ⇒ 两条腿同一个答案、
///    不必同一份实现（[`HOST_GLOBALS_JS`] 那一条纪律）。
/// 2. **只覆盖 `exec` 不够**（实测）：QuickJS 的 `String.prototype.match`/`RegExp.prototype.test`
///    走的是内部那颗 exec，**不经过** JS 侧覆盖过的那一颗 ⇒ 三个入口各自补一次同参数扫描。
///    扫描用**克隆**（`new RegExp(source, flags+g)`），所以被匹配的那颗正则的 `lastIndex` 一点不动。
/// 3. **Annex B 的槽语义**：初值是空串（不是 undefined）、失败的匹配**不动**上一次的槽、
///    全局那一族落的是**最后一次**匹配（判据三条都钉）。
///
/// `replace`/`split` 之后读 `RegExp.$n` 的那一支**不交**：上游实测 **0 处**（同一把尺子），
/// 而这两条在文本流水里是热路（转义码那一族），不为零用量加一次扫描。
pub const REGEXP_CAPTURE_JS: &str = r#"
if (typeof RegExp.$1 === "undefined") {
    (function () {
        var nativeExec = RegExp.prototype.exec;
        var nativeTest = RegExp.prototype.test;
        var nativeMatch = String.prototype.match;
        var nativeSearch = String.prototype.search;

        var setStatics = function (arr) {
            if (!arr) { return; }
            for (var i = 1; i <= 9; i++) {
                RegExp['$' + i] = arr[i] === undefined ? '' : String(arr[i]);
            }
        };
        for (var i = 1; i <= 9; i++) { RegExp['$' + i] = ''; }

        var scan = function (re, str, once) {
            var flags = String(re.flags || '');
            var probe = new RegExp(re.source, flags.indexOf('g') >= 0 ? flags : flags + 'g');
            var last = null;
            var m;
            while ((m = nativeExec.call(probe, str)) !== null) {
                last = m;
                if (once) { break; }
                // 空匹配不许原地不动（否则这一圈永不停）。
                if (m[0] === '') { probe.lastIndex++; }
            }
            setStatics(last);
        };

        RegExp.prototype.exec = function (input) {
            var arr = nativeExec.call(this, input);
            setStatics(arr);
            return arr;
        };
        RegExp.prototype.test = function (input) {
            var out = nativeTest.call(this, input);
            if (out) { scan(this, input === undefined ? '' : String(input), !this.global); }
            return out;
        };
        String.prototype.match = function (pattern) {
            var out = nativeMatch.apply(this, arguments);
            if (pattern instanceof RegExp) {
                scan(pattern, String(this), !pattern.global);
            }
            return out;
        };
        String.prototype.search = function (pattern) {
            var out = nativeSearch.apply(this, arguments);
            if (pattern instanceof RegExp && out >= 0) { scan(pattern, String(this), true); }
            return out;
        };
    })();
}
"#;

/// 装载顺序：`console` → 全局面 → `Utils` → `Input` → `TouchInput` → `ResourceHandler` →
/// `RegExp` 的捕获槽那一族。
///
/// **必须在 [`crate::shapes::install`] 之后**（`Utils`/`Input`/`TouchInput`/`ResourceHandler`
/// 四个名字要先存在 —— 它们是被顶替的那一层交出的静态类形状，这一格只往上面挂成员）。
/// [`crate::shapes::install_extensions`] 也要在前：`Utils.isOptionValid` 用的是
/// `Array.prototype.contains`（`JsExtensions` 那一族）。
pub fn install(host: &Host) -> Result<(), String> {
    // `console` 先装：装载平台件这一段本身也可能喊话（守卫让重复装载安全）。
    install_console(host)?;
    for piece in [
        HOST_GLOBALS_JS,
        UTILS_JS,
        INPUT_JS,
        TOUCH_INPUT_JS,
        RESOURCE_HANDLER_JS,
        REGEXP_CAPTURE_JS,
    ] {
        host.eval(piece).map_err(|e| format!("平台件装不上：{e}"))?;
    }
    Ok(())
}

/// 只交记账的那颗 `console`（装载器单独要它 —— [`crate::plugins::load`] 逐条插件取账，
/// 不需要 `Utils`/`Input` 那一族）。
pub fn install_console(host: &Host) -> Result<(), String> {
    host.eval(CONSOLE_JS)
        .map_err(|e| format!("console 装不上：{e}"))
}

/// 把 JS 侧那本 `console` 账读出来（不清空）。宿主没装 [`CONSOLE_JS`] 时返回空。
pub fn console_lines(host: &Host) -> Vec<(String, String)> {
    take(host, false)
}

/// 读出来**并清空** —— [`crate::plugins::load`] 每条插件之间叫一次，于是喊出来的话能按插件归堆。
pub fn take_console_lines(host: &Host) -> Vec<(String, String)> {
    take(host, true)
}

fn take(host: &Host, drain: bool) -> Vec<(String, String)> {
    // 账本住在 JS 侧（那是插件写字的地方），取回 Rust 只走一条腿：让 JS 自己 `JSON.stringify`。
    // `splice` 在 stringify 之后做 ⇒ 中途抛异常也不会把账丢掉。
    let expr = format!(
        "(function () {{ var l = globalThis.__sisf_console; var s = l ? JSON.stringify(l) : '[]'; \
         if ({drain} && l) {{ l.splice(0, l.length); }} return s; }})()"
    );
    let json = match host.try_eval_string(&expr) {
        Ok(text) => text,
        Err(_) => return Vec::new(),
    };
    serde_json::from_str::<Vec<[String; 2]>>(&json)
        .unwrap_or_default()
        .into_iter()
        .map(|pair| (pair[0].clone(), pair[1].clone()))
        .collect()
}

#[cfg(all(test, feature = "js"))]
mod tests {
    //! fixture 判据：两条腿各跑一遍（同一份文本、同一组断言）。真数据的尺子在
    //! `pray/src/compat.rs` 的装载账单 —— 这一族只证明**形状与时序是上游那一份**。
    use super::{CONSOLE_JS, HOST_GLOBALS_JS};
    use crate::shapes::{self, DeclaredShape, ShapeKind};

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    use crate::jsrt::Host;

    /// 静态类名单里这一族要的四颗 + 一条被 `Utils.isOptionValid` 用到的内建扩展。
    fn statics(host: &Host, names: &[&str]) {
        let shapes: Vec<DeclaredShape> = names
            .iter()
            .map(|n| DeclaredShape {
                name: (*n).to_string(),
                kind: ShapeKind::StaticClass,
            })
            .collect();
        shapes::install(host, &shapes).expect("静态类形状装不上");
        shapes::install_extensions(host).expect("JsExtensions 装不上");
    }

    /// 平台件全套（与 `install` 同一顺序，只多了"名单由测试给"这一层）。
    fn booted() -> Host {
        let host = Host::boot();
        statics(&host, &["Utils", "Input", "TouchInput", "ResourceHandler"]);
        super::install(&host).expect("平台件装不上");
        host
    }

    #[test]
    fn utils_answers_like_a_desktop_browser() {
        let h = booted();
        // 版本串是被顶替那一层的实测数据（`rpg_core.js:183`），插件拿它做特性门。
        assert_eq!(h.eval_string("Utils.RPGMAKER_NAME"), "MV");
        assert_eq!(h.eval_string("Utils.RPGMAKER_VERSION"), "1.6.1");
        // 三条门判：sisf 不是 nwjs、没有查询串、不是移动设备。
        assert!(
            h.eval_bool("!Utils.isNwjs()"),
            "isNwjs 必须是 false（两条腿同一个答案）"
        );
        assert_eq!(h.eval_i32("Utils.isOptionValid('test')"), 0);
        assert!(h.eval_bool("!Utils.isMobileDevice()"));
        assert!(h.eval_bool("!Utils.isMobileSafari() && !Utils.isAndroidChrome()"));
        // 两条纯函数 + 一条运行时 id（上游 `Utils._id++`）。
        assert_eq!(
            h.eval_string("Utils.rgbToCssColor(255.4, 128.6, 0)"),
            "rgb(255,129,0)"
        );
        h.eval("globalThis.__id1 = Utils.generateRuntimeId();")
            .expect("generateRuntimeId 调用失败");
        assert_eq!(h.eval_i32("__id1"), 1);
        assert_eq!(h.eval_i32("Utils.generateRuntimeId()"), 2);
    }

    #[test]
    fn the_window_global_is_the_global_object() {
        // `window = globalThis` 那一格的可观察后果：写进 window 的名字就是全局名（浏览器同形）。
        let h = booted();
        h.eval("globalThis.MA_Thing = function () {}; window['other'] = 7;")
            .expect("window 写不通");
        assert!(h.eval_bool("window.MA_Thing === globalThis.MA_Thing"));
        assert_eq!(h.eval_i32("other"), 7);
        assert!(h.eval_bool("self === globalThis && typeof location.search === 'string'"));
    }

    #[test]
    fn console_lines_reach_the_bill() {
        // 两条腿同一个断言：宿主带不带真 console 都一样记（带的话还转调，消息照进 devtools）。
        let h = Host::boot();
        h.eval(HOST_GLOBALS_JS).expect("全局面装不上");
        h.eval(CONSOLE_JS).expect("console 装不上");
        // 先取走 realm 里已有的账：**node 那条腿一整个进程共享一个 realm**，别的测试也喊过话
        // （native 每颗 Host 一个独立 realm，所以这一格只在 wasm 腿量得出来）。
        super::take_console_lines(&h);
        h.eval("console.error('缺 Skill Tree Id'); console.log('a', 1, null); console.assert(false, '闸门');")
            .expect("console 调用失败");
        let lines = super::take_console_lines(&h);
        assert_eq!(
            lines,
            vec![
                ("error".to_string(), "缺 Skill Tree Id".to_string()),
                ("log".to_string(), "a 1 null".to_string()),
                ("assert".to_string(), "闸门".to_string()),
            ],
            "插件自己喊的话必须原样报回来（多参用空格连，`assert` 只在条件假时记）"
        );
        assert!(
            super::console_lines(&h).is_empty(),
            "取走之后账本该是空的（`plugins::load` 逐条插件归堆靠这一格）"
        );
        // 再装一次（装载顺序里 `install` 被叫两次是真的会发生）：`orig` 只认第一次那颗真的，
        // 于是不会出现"转调我们自己那一颗 ⇒ 同一行记两遍"。
        h.eval(CONSOLE_JS).expect("console 第二次装载失败");
        h.eval("console.warn('第二遍');").expect("第二次调用失败");
        assert_eq!(
            super::take_console_lines(&h),
            vec![("warn".to_string(), "第二遍".to_string())],
            "重装之后重复记了 ⇒ `__sisf_consoleOrig` 那一格没守住"
        );
    }

    #[test]
    fn the_keymapper_is_a_mutable_upstream_table() {
        let h = booted();
        // 上游表里三格（`:3002-3027`）：`18` 本来就叫 control，插件把它改成 alt。
        assert_eq!(h.eval_string("Input.keyMapper[13]"), "ok");
        assert_eq!(h.eval_string("Input.keyMapper[18]"), "control");
        assert_eq!(h.eval_string("Input.keyMapper[120]"), "debug");
        h.eval("Input.keyMapper[18] = 'alt';").expect("改一格失败");
        assert_eq!(h.eval_string("Input.keyMapper[18]"), "alt");
        assert_eq!(
            h.eval_string("Input.keyMapper[13]"),
            "ok",
            "表被整颗换掉了（键名表必须是就地可写的那个对象）"
        );
        assert_eq!(h.eval_string("Input.gamepadMapper[1]"), "cancel");
    }

    #[test]
    fn input_predicates_follow_the_upstream_key_repeat() {
        // 这一条是"逐字照上游"的证据：时序数（0/1/24/25）只能由上游那三个函数体算出来。
        let h = booted();
        h.eval("Input.initialize();")
            .expect("Input.initialize 失败");
        assert!(
            !h.eval_bool("Input.isPressed('left')"),
            "没按过就该是 false（`_currentState` 是空表）"
        );
        // 按一下左（keyCode 37 ⇒ keyMapper 'left'），走一帧。
        h.eval("globalThis.__ev = { keyCode: 37, preventDefault: function () {} };")
            .expect("事件对象建不出来");
        h.eval("Input._onKeyDown(__ev); Input.update();")
            .expect("输入门失败");
        assert!(h.eval_bool("Input.isPressed('left')"));
        assert!(h.eval_bool("Input.isTriggered('left')"));
        assert!(h.eval_bool("Input.isRepeated('left')"));
        assert_eq!(
            h.eval_i32("Input.dir4"),
            4,
            "左 = 数字键盘 4（5 - y*3 + x）"
        );
        assert_eq!(h.eval_i32("Input.dir8"), 4);
        // 第二拍：`_pressedTime` 从 1 起（`update` 里先加后判），triggered 就没了。
        h.eval("Input.update();").expect("第二拍失败");
        assert!(
            !h.eval_bool("Input.isTriggered('left')"),
            "第 2 拍还算 triggered ⇒ 上游那句 `update` 里的加拍位置被挪了"
        );
        assert_eq!(h.eval_i32("Input._pressedTime"), 1);
        // 按住不放：到第 24 拍 `isLongPressed` 才真，`isRepeated` 落在 24/30… 那几拍。
        h.eval("(function () { for (var i = 0; i < 23; i++) { Input.update(); } })()")
            .expect("连走 23 拍失败");
        assert_eq!(h.eval_i32("Input._pressedTime"), 24);
        assert!(h.eval_bool("Input.isLongPressed('left')"));
        assert!(h.eval_bool("Input.isRepeated('left')"));
        h.eval("Input.update();").expect("第 26 拍失败");
        assert_eq!(h.eval_i32("Input._pressedTime"), 25);
        assert!(
            !h.eval_bool("Input.isRepeated('left')"),
            "25 不是 6 的整数倍 ⇒ isRepeated 的取模那一支错了"
        );
        // 松手：`_latestButton` 在下一次 `update` 里清掉，但 `isPressed` 当场就没。
        h.eval("Input._onKeyUp(__ev); Input.update();")
            .expect("松手失败");
        assert!(!h.eval_bool("Input.isPressed('left')"));
        assert!(h.eval_bool("Input._latestButton === null"));
        assert_eq!(h.eval_i32("Input.dir4"), 0);
    }

    #[test]
    fn escape_carries_cancel_and_menu() {
        // `_isEscapeCompatible`（`:3438`）：escape 按下去，cancel 与 menu 都算按下去。
        let h = booted();
        h.eval("Input.initialize(); globalThis.__esc = { keyCode: 27, preventDefault: function () {} };")
            .expect("事件建不出来");
        h.eval("Input._onKeyDown(__esc); Input.update();")
            .expect("escape 门失败");
        assert!(h.eval_bool("Input.isPressed('cancel') && Input.isPressed('menu')"));
        assert!(h.eval_bool("Input.isTriggered('cancel')"));
        // 而 shift 不受这条影响（否则"escape 兼容"就变成"什么都算"）。
        assert!(!h.eval_bool("Input.isPressed('shift')"));
        h.eval("Input._onLostFocus();").expect("_onLostFocus 失败");
        assert!(
            !h.eval_bool("Input.isPressed('escape')"),
            "失焦要清空状态（`_onLostFocus`）"
        );
    }

    #[test]
    fn touch_input_moves_the_two_stage_state() {
        // 上游那两段形状：`_onTrigger` 写 `_events`，`update()` 才搬到公开位，下一拍自动清。
        let h = booted();
        h.eval("TouchInput.initialize(); TouchInput._onTrigger(300, 200);")
            .expect("触屏门失败");
        assert!(
            !h.eval_bool("TouchInput.isTriggered()"),
            "还没走拍就该还没触发"
        );
        h.eval("TouchInput.update();").expect("触屏 update 失败");
        assert!(h.eval_bool("TouchInput.isTriggered()"));
        assert_eq!(h.eval_i32("TouchInput.x"), 300);
        assert_eq!(h.eval_i32("TouchInput.y"), 200);
        assert!(h.eval_bool("TouchInput.date > 0"));
        h.eval("TouchInput.update();").expect("第二拍失败");
        assert!(!h.eval_bool("TouchInput.isTriggered()"), "触发只留一拍");
        // 滚轮累加到 `_events`，`update()` 搬完就归零。
        h.eval("TouchInput._onWheel({ deltaY: 3, deltaX: 0, preventDefault: function () {} }); TouchInput.update();")
            .expect("滚轮门失败");
        assert_eq!(h.eval_i32("TouchInput.wheelY"), 3);
        h.eval("TouchInput.update();").expect("第三拍失败");
        assert_eq!(h.eval_i32("TouchInput.wheelY"), 0);
        // 按下状态：`_mousePressed` 不是我们的门写的（DOM 那一族不交），所以走字段本身。
        h.eval("TouchInput._mousePressed = true; TouchInput.clear();")
            .expect("清状态失败");
        assert!(!h.eval_bool("TouchInput.isPressed()"));
    }

    #[test]
    fn annex_b_capture_statics_answer_on_every_match_entry() {
        // 上游那 128 处 `RegExp.$n` 的写法固定是 `if (line.match(/…/)) { parseInt(RegExp.$1) }`
        // （尺子：`sisf_pray/tools/probes/regexp-statics.py`）。QuickJS 不带这一族 ⇒ 缺它不抛，
        // 只是读出来是 `NaN`，所以四个入口各自都要落对（浏览器腿与 node 腿本来就有 ⇒ 同一答案）。
        let h = booted();
        assert_eq!(
            h.eval_string("typeof RegExp.$1"),
            "string",
            "Annex B 的初值是空串，不是 undefined"
        );
        assert_eq!(h.eval_string("RegExp.$1"), "");

        // ① `.match`（上游用得最多的一条）。
        h.eval("globalThis.__mm = '<Max Item: 30>'.match(/<(?:MAX ITEM):[ ](\\d+)>/i);")
            .expect("match 那一腿失败");
        assert!(h.eval_bool("__mm !== null"));
        assert_eq!(h.eval_string("RegExp.$1"), "30");

        // ② `.test`（QuickJS 的原生 test 不经过覆盖过的 exec ⇒ 这一格是补出来的）。
        h.eval("globalThis.__tt = /<(\\w+)=[ ]?(\\d+)>/i.test('<rate=7>');")
            .expect("test 那一腿失败");
        assert!(h.eval_bool("__tt"));
        assert_eq!(h.eval_string("RegExp.$1"), "rate");
        assert_eq!(h.eval_string("RegExp.$2"), "7");

        // ③ 裸 `.exec`。
        h.eval("globalThis.__ee = /(a)(b)/.exec('ab');")
            .expect("exec 那一腿失败");
        assert_eq!(h.eval_string("RegExp.$1"), "a");
        assert_eq!(h.eval_string("RegExp.$2"), "b");

        // ④ **失败的那一次不许抹掉上一次**（Annex B 的形状，插件靠它连续 if-else 取上一格的组）。
        h.eval("globalThis.__f = 'nothing'.match(/<(\\d+)>/);")
            .expect("失败匹配那一腿失败");
        assert!(h.eval_bool("__f === null"));
        assert_eq!(h.eval_string("RegExp.$1"), "a", "失败的匹配把槽抹掉了");

        // ⑤ 全局那一条落的是**最后一次**匹配。
        h.eval("globalThis.__g = '<a=1><a=2><a=3>'.match(/<a=(\\d+)>/g);")
            .expect("全局 match 失败");
        assert_eq!(h.eval_i32("__g.length"), 3);
        assert_eq!(h.eval_string("RegExp.$1"), "3");

        // ⑥ 补的这一族**不许动被匹配那颗正则自己的状态**（扫描走克隆）。
        h.eval("globalThis.__re = /<a=(\\d+)>/g; __re.lastIndex = 0;")
            .expect("那颗正则建不出来");
        assert_eq!(h.eval_i32("'<a=1><a=2>'.match(__re).length"), 2);
        assert_eq!(h.eval_string("typeof RegExp.$1"), "string");
        h.eval("globalThis.__one = __re.exec('<a=9>');")
            .expect("lastIndex 那条腿失败");
        assert_eq!(h.eval_string("RegExp.$1"), "9");
        // `lastIndex` 由 exec 自己推进（克隆不参与）。
        assert!(h.eval_i32("__re.lastIndex") > 0);
    }

    #[test]
    fn reinstalling_the_platform_is_the_core_layer_being_declared_again() {
        // 装载顺序里 `install` 可能被叫两次（宿主重建、或两条腿共一份 JS 文本反复装）。
        // 上游的语义是"**那一层脚本又声明了一遍**"：所以键名表回到核心那一份，而 `console` 的账本
        // 不重建（守卫看到已经有了就不动）。插件的覆盖本来就排在核心之后 —— 洗掉它是**对**的，
        // 这一条判据钉的就是这个方向，别把它读成 bug。
        let h = booted();
        h.eval("Input.keyMapper[18] = 'alt'; console.log('第一次');")
            .expect("第一次装载失败");
        super::install(&h).expect("第二次装载失败");
        assert_eq!(
            h.eval_string("Input.keyMapper[18]"),
            "control",
            "核心层重装的顺序错了会反过来：插件的覆盖必须排在核心之后（同上游 `<script>` 次序）"
        );
        // `console` 那一族的守卫在 `console_lines_reach_the_bill` 里钉；这里钉核心面重装的形状。
        assert_eq!(
            h.eval_string("Utils.RPGMAKER_VERSION"),
            "1.6.1",
            "重装把静态成员洗掉了 —— 那一条腿必须重新交（`install` 就是干这个的）"
        );
        assert_eq!(
            h.eval_i32("Utils._id"),
            1,
            "`Utils._id` 该回到核心那份（`:285`）"
        );
        assert_eq!(
            h.eval_string("typeof ResourceHandler.exists"),
            "function",
            "`Input._onKeyDown` 问的那一句必须有答案"
        );
        assert!(!h.eval_bool("ResourceHandler.exists()"));
    }
}
