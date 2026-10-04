//! M10.2 的 **managers 族（数据那一半）**：`Decrypter` 的名字层、`ResourceHandler.createLoader`、
//! `DataManager` 读库的那一族、`TextManager` 整族。
//!
//! ## 范围是查出来的，不是印象
//!
//! 尺子是 `sisf_pray/tools/probes/manager-touch.py`（只读上游，103 条启用插件）：**81／103 条**插件
//! 碰过 `rpg_managers.js` 的静态成员。按成员数排的交付顺序，与这一格"交哪几条、留哪几条"的判词：
//!
//! - `DataManager` 43 种成员（`isDatabaseLoaded` 8、`isItem` 7、`extractSaveContents` 6…）——
//!   **数据那一半交**，存档/`Game_*` 那一半留 M10.3（见下面第 4 条）。
//! - `TextManager` 26 种成员（`currencyUnit` 7、`levelA` 4、`item`/`weapon`/`armor`/`keyItem` 各 3）——
//!   **整族交**：它只是 `$dataSystem.terms` 的取值器，2,843 行的 managers 里最省的一格。
//! - `PluginManager.parameters` 73 处 —— M10.1 已交（`crate::plugins`）。
//! - **`ImageManager` 留 M10.4**：`loadSystem` 53 处 / `loadBHud` 54 处（后者是 MOG 自己加的成员），
//!   但 `loadNormalBitmap` 的落点是 `Bitmap.load`，而 `ImageCache._truncateCache` 读的是
//!   `bitmap.width * bitmap.height` 与 `isRequestOnly()`（`rpg_core.js:518-553`）——**没有位图就没有
//!   缓存**，先交 `ImageManager` 只能交一颗空壳，量不出任何判据。同一条腿上的 `reserveXXX` 那一族
//!   正是浏览器侧"按图取素材"的波（`sisf_pray/src/load.rs`）⇒ 与 `Bitmap` 一起落 M10.4，
//!   那条 `image_manager_goes_through_the_vfs` 也跟着落过去（`../sisf_pray/M10.md`「M10.2 验收（二）」记这一格改判）。
//! - **`AudioManager`/`SoundManager` 留 M10.4**：`createBuffer`（`rpg_managers.js:1467`）造的是
//!   `WebAudio`/`Html5Audio`，而混音器在 `sisf-audio`，不在这一颗 crate 里（crate 不许反向依赖 app 侧）。
//!   `SoundManager.playCursor` 那 29 处全是**运行期**，不是装载期。**M10.3 的 c 也没有顺手把它交掉**：
//!   `leg-deps.py` 量出 c 的体里有 12 处 `SoundManager.play*`（全在"结果落地/倒下的表演"那一条腿上），
//!   而它们底下就是 `AudioManager` ⇒ 留到 M10.4 一起落，这一轮的判据钉的是
//!   "撞到那一格抛、而且点名是 `SoundManager`"（`objects::the_battler_reaches_the_undelivered_managers_and_says_who`）。
//! - **`StorageManager`/`ConfigManager` 不动**：它们要**写侧**（`require('fs')` 或 `localStorage`），
//!   而"存档走引擎自己的 RON、不兼容 `rpgsave`"那条定案在 M10 改判后**仍挂着等用户判断**
//!   （`../sisf_pray/M10.md`「留给用户判断的三条」之一）⇒ 不许自行落点。
//! - **`BattleManager` 不交**：顶层 47 处读它（MOG/YEP 的战斗 HUD 那一族），但战斗是 AGENTS 的
//!   定案非目标，而且 JS 里"读一个不存在的静态"得到 `undefined`、紧接着的覆盖照样落得下去 ⇒
//!   装载账单里它们本来就是绿的（实测 103 条只挂 1 条）。逐插件开账时（M10.6）再判。
//!
//! ## transport 的两条改判（都在上游的同一格上动刀，逐条写下为什么）
//!
//! 上游 [`DataManager.loadDataFile`]（`rpg_managers.js:78-94`）是 `XMLHttpRequest`：新边界里没有 DOM，
//! 字节由宿主从 [`sisf_asset::Vfs`] 给（`mv_provider::MvAssets` 那一族：加密名映射 + 明文回退 + 内存解密）。
//!
//! 1. **读是同步的**。上游 `onload` 排到下一 tick，所以 `isDatabaseLoaded()` 会先假后真；这里同一句里
//!    就真。方向上只会更宽松（`Scene_Boot` 的等待腿天然通过），而 M10.5 接 `SceneManager` 时
//!    那一族的"等一拍"形状要回来一次 —— 记在这里，别到时候当成 bug 找。
//! 2. **"文件没有"不落成 `_errorUrl`**。上游的 `xhr.onerror` 会设 `_errorUrl`，而 `checkError()` 一看见它
//!    就抛（`:169-173`）。浏览器腿的清单是**按图取**的（`sisf_pray` 咬人第 32 条），"字节还没到"是可重试
//!    状态而不是错误，所以这一格只把名字记进 **`DataManager._pendingFiles`**（兼容层加的一格，不是上游的：
//!    上游那个信号是 `window[name] === null`，它不带名字，HUD 说不出"缺哪一条"）。
//!    `Host::bind_str` 那一格只有字符串可传，所以"没有"用**空串**表达 —— 实测上游 `data/` 713 个
//!    `.json` 里 0 字节的**一个都没有**（最小的是插件私有的 `ContainerProperties.json`，293 字节）。
//!
//! ## 三条写法纪律（继承 `crate::platform` 的模块头）
//!
//! 逐字照上游并把行号写在旁边；装载顺序相关的地方用守卫而不是删句子（守卫的理由必须说得出）；
//! 不造 API —— 上面第 2 条那一格 `_pendingFiles` 是这一族**唯一**新增的名字，理由已经写在它旁边。

use std::rc::Rc;

use sisf_asset::{Vfs, VfsPath};

use crate::jsrt::Host;

/// JS 侧向宿主借文本的那一个全局名（模块头那条 transport 改判的落点）。
pub const READ_TEXT_GLOBAL: &str = "__sisf_read_text";

/// `Decrypter` 的**名字与路径那一半**（`rpg_core.js:9153-9265`）。
///
/// 上游的 `SIGNATURE`/`VER`/`REMAIN`/`_headerlength`/`_ignoreList`/`checkImgIgnore`/
/// `extToEncryptExt`/`readEncryptionkey` 逐字照抄，而 `decryptImg`/`decryptHTML5Audio`/
/// `cutArrayHeader`/`decryptArrayBuffer`/`createBlobUrl` 那五条**不交**：它们是 XHR + `ArrayBuffer` +
/// `Blob` + `<img src>` 那一族（`:9173-9250`），新边界里解密发生在 Rust 的
/// [`crate::decrypt`]（同一把钥匙、同一个 16 字节头，M0.3 起有真素材判据），像素落在 `Bitmap` 上是
/// M10.4 的事。**密钥从 `$dataSystem.encryptionKey` 读**（`readEncryptionkey`，`:9262`）⇒ 兼容层
/// 一个字节都不带它（`sisf_pray/src/key.rs::key_never_hardcoded` 现在扫两个仓库钉这一格）。
pub const DECRYPTER_JS: &str = r#"
Decrypter.hasEncryptedImages = false;
Decrypter.hasEncryptedAudio = false;
Decrypter._requestImgFile = [];
Decrypter._headerlength = 16;
Decrypter._xhrOk = 400;
Decrypter._encryptionKey = "";
Decrypter._ignoreList = [
    "img/system/Window.png"
];
Decrypter.SIGNATURE = "5250474d56000000";
Decrypter.VER = "000301";
Decrypter.REMAIN = "0000000000";
Decrypter.checkImgIgnore = function (url) {
    for (var cnt = 0; cnt < this._ignoreList.length; cnt++) {
        if (url === this._ignoreList[cnt]) return true;
    }
    return false;
};
Decrypter.extToEncryptExt = function (url) {
    var ext = url.split('.').pop();
    var encryptedExt = ext;
    if (ext === "ogg") encryptedExt = ".rpgmvo";
    else if (ext === "m4a") encryptedExt = ".rpgmvm";
    else if (ext === "png") encryptedExt = ".rpgmvp";
    else encryptedExt = ext;
    return url.slice(0, url.lastIndexOf(ext) - 1) + encryptedExt;
};
Decrypter.readEncryptionkey = function () {
    this._encryptionKey = $dataSystem.encryptionKey.split(/(.{2})/).filter(Boolean);
};
"#;

/// `ResourceHandler.createLoader`（`rpg_core.js:9282-9306`）逐字照抄。
///
/// `DataManager.loadMapData`（`rpg_managers.js:109`）在第一句就取它，所以这一格必须有名字。
/// 它**返回**的那颗闭包里才有 `setTimeout`/`Graphics.printLoadingError`/`SceneManager.stop` ——
/// 同步 transport 走不到那一条腿（模块头改判第 2 条），所以这三格留空不影响装载；
/// M10.4 交出失败面（`retry`/`_reloaders` 那两条腿）时它们才真的被叫。
pub const RESOURCE_HANDLER_LOADER_JS: &str = r#"
ResourceHandler._defaultRetryInterval = [500, 1000, 3000];
ResourceHandler.createLoader = function (url, retryMethod, resignMethod, retryInterval) {
    retryInterval = retryInterval || this._defaultRetryInterval;
    var reloaders = this._reloaders;
    var retryCount = 0;
    return function () {
        if (retryCount < retryInterval.length) {
            setTimeout(retryMethod, retryInterval[retryCount]);
            retryCount++;
        } else {
            if (resignMethod) {
                resignMethod();
            }
            if (url) {
                if (reloaders.length === 0) {
                    Graphics.printLoadingError(url);
                    SceneManager.stop();
                }
                reloaders.push(function () {
                    retryCount = 0;
                    retryMethod();
                });
            }
        }
    };
};
"#;

/// `DataManager` 的**数据那一半**（`rpg_managers.js:44-198`）。
///
/// 交的十五格：`_globalId`/`_lastAccessedId`/`_errorUrl`/`_databaseFiles`（十四行逐字）、
/// `loadDatabase`、`loadDataFile`（transport 换宿主，其余形状照抄）、`isDatabaseLoaded`、
/// `loadMapData`、`makeEmptyMap`、`isMapLoaded`、`onLoad`、`extractMetadata`、`checkError`、
/// `isBattleTest`/`isEventTest`/`isSkill`/`isItem`/`isWeapon`/`isArmor`。
///
/// **`onLoad` 是热别名点**：YEP 那一族（`processCORENotetags1..4`、`processSkillNotetags`…）写的都是
/// `var alias = DataManager.onLoad; DataManager.onLoad = function(object){ alias.call(this,object); … }`
/// ⇒ 这一格不落，插件的 nota-tag 处理一条都不会跑（manager-touch 量的 12 个 `processXXX` 成员全挂在它后面）。
///
/// 留 M10.3 的那一半里，**`createGameObjects`（`new` 十三颗 `Game_*`）已经随 c 交**
/// （[`CREATE_GAME_OBJECTS_JS`]，单独一段因为它要等 [`crate::objects`] 里 a/b/c 那三族行为文本 ——
/// 十三颗分属状态族、地图与角色、玩法，少一族就 `new` 不出来）。**这一格现在也在
/// `objects-verbatim.py` 的名单里**（d 那一轮起这把尺子量两处上游，`M10.md` d 事实 54），
/// 它是这一族里唯一一段**手抄**的 managers 文本，正是那把尺子最该盯的形状。
/// 还剩的是 `setupNewGame`、`setupBattleTest`/`setupEventTest`、
/// `saveGame`/`loadGame`/`makeSaveContents`/`extractSaveContents`、
/// `loadGlobalInfo`/`isThisGameFile`/`latestSavefileId`/`loadSavefileImages` 那一族（要 `JsonEx` +
/// `LZString` + 写侧 ⇒ 挂在「存档格式」那条待定项上）。
pub const DATA_MANAGER_JS: &str = r#"
// 上游那十五个全局数据名（`rpg_managers.js:14-28` 的 `var $data… = null`）。显式挂 `globalThis`
// 是这一族的老规矩（M10.1 事实第 5 条：`js_sys::eval` 里的 `var` 不留给下一条 eval），而它们**必须**
// 先是 `null` 而不是 `undefined`：`TextManager` 那一族与插件的 `$dataItems[id]` 都按"库还没读时是 null"
// 写，而 `window[name] = null` 只覆盖 `_databaseFiles` 里那十四格（`$dataMap`/`$testEvent` 不在里面）。
globalThis.$dataActors = null;
globalThis.$dataClasses = null;
globalThis.$dataSkills = null;
globalThis.$dataItems = null;
globalThis.$dataWeapons = null;
globalThis.$dataArmors = null;
globalThis.$dataEnemies = null;
globalThis.$dataTroops = null;
globalThis.$dataStates = null;
globalThis.$dataAnimations = null;
globalThis.$dataTilesets = null;
globalThis.$dataCommonEvents = null;
globalThis.$dataSystem = null;
globalThis.$dataMapInfos = null;
globalThis.$dataMap = null;
globalThis.$testEvent = null;
DataManager._globalId = 'RPGMV';
DataManager._lastAccessedId = 1;
DataManager._errorUrl = null;
// 兼容层新加的一格（本族唯一的新名字）：缺文件时报名字，理由见模块头 transport 改判第 2 条。
DataManager._pendingFiles = [];
DataManager._databaseFiles = [
    { name: '$dataActors',       src: 'Actors.json'       },
    { name: '$dataClasses',      src: 'Classes.json'      },
    { name: '$dataSkills',       src: 'Skills.json'       },
    { name: '$dataItems',        src: 'Items.json'        },
    { name: '$dataWeapons',      src: 'Weapons.json'      },
    { name: '$dataArmors',       src: 'Armors.json'       },
    { name: '$dataEnemies',      src: 'Enemies.json'      },
    { name: '$dataTroops',       src: 'Troops.json'       },
    { name: '$dataStates',       src: 'States.json'       },
    { name: '$dataAnimations',   src: 'Animations.json'   },
    { name: '$dataTilesets',     src: 'Tilesets.json'     },
    { name: '$dataCommonEvents', src: 'CommonEvents.json' },
    { name: '$dataSystem',       src: 'System.json'       },
    { name: '$dataMapInfos',     src: 'MapInfos.json'     }
];
DataManager.loadDatabase = function () {
    var test = this.isBattleTest() || this.isEventTest();
    var prefix = test ? 'Test_' : '';
    for (var i = 0; i < this._databaseFiles.length; i++) {
        var name = this._databaseFiles[i].name;
        var src = this._databaseFiles[i].src;
        this.loadDataFile(name, prefix + src);
    }
    if (this.isEventTest()) {
        this.loadDataFile('$testEvent', prefix + 'Event.json');
    }
};
DataManager.loadDataFile = function (name, src) {
    var url = 'data/' + src;
    window[name] = null;
    var text = globalThis.__sisf_read_text(url);
    if (text === "") {
        if (this._pendingFiles.indexOf(url) < 0) {
            this._pendingFiles.push(url);
        }
        return;
    }
    var index = this._pendingFiles.indexOf(url);
    if (index >= 0) {
        this._pendingFiles.splice(index, 1);
    }
    window[name] = JSON.parse(text);
    DataManager.onLoad(window[name]);
};
DataManager.isDatabaseLoaded = function () {
    this.checkError();
    for (var i = 0; i < this._databaseFiles.length; i++) {
        if (!window[this._databaseFiles[i].name]) {
            return false;
        }
    }
    return true;
};
DataManager.loadMapData = function (mapId) {
    if (mapId > 0) {
        var filename = 'Map%1.json'.format(mapId.padZero(3));
        this._mapLoader = ResourceHandler.createLoader(
            'data/' + filename,
            this.loadDataFile.bind(this, '$dataMap', filename)
        );
        this.loadDataFile('$dataMap', filename);
    } else {
        this.makeEmptyMap();
    }
};
DataManager.makeEmptyMap = function () {
    $dataMap = {};
    $dataMap.data = [];
    $dataMap.events = [];
    $dataMap.width = 100;
    $dataMap.height = 100;
    $dataMap.scrollType = 3;
};
DataManager.isMapLoaded = function () {
    this.checkError();
    return !!$dataMap;
};
DataManager.onLoad = function (object) {
    var array;
    if (object === $dataMap) {
        this.extractMetadata(object);
        array = object.events;
    } else {
        array = object;
    }
    if (Array.isArray(array)) {
        for (var i = 0; i < array.length; i++) {
            var data = array[i];
            if (data && data.note !== undefined) {
                this.extractMetadata(data);
            }
        }
    }
    if (object === $dataSystem) {
        Decrypter.hasEncryptedImages = !!object.hasEncryptedImages;
        Decrypter.hasEncryptedAudio = !!object.hasEncryptedAudio;
        // 上游这一句是 Scene_Boot.loadSystemImages()（:149）。装载顺序是
        // 形状 → 平台件 → managers → Game_*（M10.3）→ 渲染面（M10.4）→ scenes（M10.5），
        // 所以此刻 Scene_Boot 只有形状、那一句还没有身体。**守卫而不是删句**：M10.5 交出身体之后
        // 这一句自己会开始跑，删了就变成静默少一格。
        if (typeof Scene_Boot !== 'undefined' &&
            typeof Scene_Boot.loadSystemImages === 'function') {
            Scene_Boot.loadSystemImages();
        }
    }
};
DataManager.extractMetadata = function (data) {
    var re = /<([^<>:]+)(:?)([^>]*)>/g;
    data.meta = {};
    for (;;) {
        var match = re.exec(data.note);
        if (match) {
            if (match[2] === ':') {
                data.meta[match[1]] = match[3];
            } else {
                data.meta[match[1]] = true;
            }
        } else {
            break;
        }
    }
};
DataManager.checkError = function () {
    if (DataManager._errorUrl) {
        throw new Error('Failed to load: ' + DataManager._errorUrl);
    }
};
DataManager.isBattleTest = function () {
    return Utils.isOptionValid('btest');
};
DataManager.isEventTest = function () {
    return Utils.isOptionValid('etest');
};
DataManager.isSkill = function (item) {
    return item && $dataSkills.contains(item);
};
DataManager.isItem = function (item) {
    return item && $dataItems.contains(item);
};
DataManager.isWeapon = function (item) {
    return item && $dataWeapons.contains(item);
};
DataManager.isArmor = function (item) {
    return item && $dataArmors.contains(item);
};
"#;

/// `DataManager.createGameObjects`（`rpg_managers.js:200-214`）—— **随 M10.3 的 c 一起交**。
///
/// 为什么是 c 而不是更早：这一颗把 [`crate::objects`] 那十三颗 `$game*` 名字**建成实例**，
/// 而它的第 9/10/11/13 行 `new` 的是 `Game_Actors`/`Game_Party`/`Game_Troop`/`Game_Player` ——
/// 前三颗的 `initialize` 在 c，而 `Game_Player.initialize` 走 `new Game_Followers()`，
/// 它的体读 `$gameParty.maxBattleMembers()`（也是 c）。所以 b 那一轮把它记成"整颗随 c 交"
/// （`../sisf_pray/M10.md`「M10.3 b 那一腿期间发现并修正的事实」第 33 条）。
///
/// 交出去之后 [`crate::objects`] 模块头的三格偏差同时闭合：`$gameMap` 有实例 ⇒
/// `Game_{Switches,Variables,SelfSwitches}.onChange` 落的 `requestRefresh()` 真敲得着；
/// `$gameParty` 有实例 ⇒ `Game_Screen.realPictureId` 的 `if ($gameParty.inBattle())` 读得动；
/// `$gamePlayer` 有实例 ⇒ b 那一轮的**属性替身**不再是唯一的路。
///
/// **`setupNewGame`（`:216-223`）不在这一格**：它还要 `selectSavefileForNewGame` 那一族
/// （`StorageManager` 的写侧，挂在「存档格式」那条待定项上），并且末行 `Graphics.frameCount = 0`
/// 抹的是宿主推进的那颗计数（[`crate::objects::GRAPHICS_FRAME_COUNT_JS`] 的守卫钉的就是它）
/// ⇒ 不许顺手折进来。
pub const CREATE_GAME_OBJECTS_JS: &str = r#"
DataManager.createGameObjects = function() {
    $gameTemp          = new Game_Temp();
    $gameSystem        = new Game_System();
    $gameScreen        = new Game_Screen();
    $gameTimer         = new Game_Timer();
    $gameMessage       = new Game_Message();
    $gameSwitches      = new Game_Switches();
    $gameVariables     = new Game_Variables();
    $gameSelfSwitches  = new Game_SelfSwitches();
    $gameActors        = new Game_Actors();
    $gameParty         = new Game_Party();
    $gameTroop         = new Game_Troop();
    $gameMap           = new Game_Map();
    $gamePlayer        = new Game_Player();
};
"#;

/// `TextManager` 整族（`rpg_managers.js:1648-1770`）：`$dataSystem.terms` 的四个取值器 +
/// `getter` 工厂 + 64 格属性。
///
/// 三条要紧的形状：
/// - `currencyUnit` 与那 64 格全是 `configurable: true` 的**取值器**（`:1676`、`:1770`）——
///   插件（`Vitsuno_Difficulty` 等）会就地覆盖其中一格，所以不许折成 Rust 侧的只读门面。
/// - 属性读的是**当拍的 `$dataSystem`**：库还没读出来时上游就是 TypeError（`$dataSystem` 是 `null`），
///   照落，不钳。
/// - `getter(method, param)` 交回的是**描述符对象**（上游就这么写的），所以 `defineProperties` 那一句
///   的参数形状也只能照抄。
pub const TEXT_MANAGER_JS: &str = r#"
TextManager.basic = function (basicId) {
    return $dataSystem.terms.basic[basicId] || '';
};
TextManager.param = function (paramId) {
    return $dataSystem.terms.params[paramId] || '';
};
TextManager.command = function (commandId) {
    return $dataSystem.terms.commands[commandId] || '';
};
TextManager.message = function (messageId) {
    return $dataSystem.terms.messages[messageId] || '';
};
TextManager.getter = function (method, param) {
    return {
        get: function () {
            return this[method](param);
        },
        configurable: true
    };
};
Object.defineProperty(TextManager, 'currencyUnit', {
    get: function () { return $dataSystem.currencyUnit; },
    configurable: true
});
Object.defineProperties(TextManager, {
    level           : TextManager.getter('basic', 0),
    levelA          : TextManager.getter('basic', 1),
    hp              : TextManager.getter('basic', 2),
    hpA             : TextManager.getter('basic', 3),
    mp              : TextManager.getter('basic', 4),
    mpA             : TextManager.getter('basic', 5),
    tp              : TextManager.getter('basic', 6),
    tpA             : TextManager.getter('basic', 7),
    exp             : TextManager.getter('basic', 8),
    expA            : TextManager.getter('basic', 9),
    fight           : TextManager.getter('command', 0),
    escape          : TextManager.getter('command', 1),
    attack          : TextManager.getter('command', 2),
    guard           : TextManager.getter('command', 3),
    item            : TextManager.getter('command', 4),
    skill           : TextManager.getter('command', 5),
    equip           : TextManager.getter('command', 6),
    status          : TextManager.getter('command', 7),
    formation       : TextManager.getter('command', 8),
    save            : TextManager.getter('command', 9),
    gameEnd         : TextManager.getter('command', 10),
    options         : TextManager.getter('command', 11),
    weapon          : TextManager.getter('command', 12),
    armor           : TextManager.getter('command', 13),
    keyItem         : TextManager.getter('command', 14),
    equip2          : TextManager.getter('command', 15),
    optimize        : TextManager.getter('command', 16),
    clear           : TextManager.getter('command', 17),
    newGame         : TextManager.getter('command', 18),
    continue_       : TextManager.getter('command', 19),
    toTitle         : TextManager.getter('command', 21),
    cancel          : TextManager.getter('command', 22),
    buy             : TextManager.getter('command', 24),
    sell            : TextManager.getter('command', 25),
    alwaysDash      : TextManager.getter('message', 'alwaysDash'),
    commandRemember : TextManager.getter('message', 'commandRemember'),
    bgmVolume       : TextManager.getter('message', 'bgmVolume'),
    bgsVolume       : TextManager.getter('message', 'bgsVolume'),
    meVolume        : TextManager.getter('message', 'meVolume'),
    seVolume        : TextManager.getter('message', 'seVolume'),
    possession      : TextManager.getter('message', 'possession'),
    expTotal        : TextManager.getter('message', 'expTotal'),
    expNext         : TextManager.getter('message', 'expNext'),
    saveMessage     : TextManager.getter('message', 'saveMessage'),
    loadMessage     : TextManager.getter('message', 'loadMessage'),
    file            : TextManager.getter('message', 'file'),
    partyName       : TextManager.getter('message', 'partyName'),
    emerge          : TextManager.getter('message', 'emerge'),
    preemptive      : TextManager.getter('message', 'preemptive'),
    surprise        : TextManager.getter('message', 'surprise'),
    escapeStart     : TextManager.getter('message', 'escapeStart'),
    escapeFailure   : TextManager.getter('message', 'escapeFailure'),
    victory         : TextManager.getter('message', 'victory'),
    defeat          : TextManager.getter('message', 'defeat'),
    obtainExp       : TextManager.getter('message', 'obtainExp'),
    obtainGold      : TextManager.getter('message', 'obtainGold'),
    obtainItem      : TextManager.getter('message', 'obtainItem'),
    levelUp         : TextManager.getter('message', 'levelUp'),
    obtainSkill     : TextManager.getter('message', 'obtainSkill'),
    useItem         : TextManager.getter('message', 'useItem'),
    criticalToEnemy : TextManager.getter('message', 'criticalToEnemy'),
    criticalToActor : TextManager.getter('message', 'criticalToActor'),
    actorDamage     : TextManager.getter('message', 'actorDamage'),
    actorRecovery   : TextManager.getter('message', 'actorRecovery'),
    actorGain       : TextManager.getter('message', 'actorGain'),
    actorLoss       : TextManager.getter('message', 'actorLoss'),
    actorDrain      : TextManager.getter('message', 'actorDrain'),
    actorNoDamage   : TextManager.getter('message', 'actorNoDamage'),
    actorNoHit      : TextManager.getter('message', 'actorNoHit'),
    enemyDamage     : TextManager.getter('message', 'enemyDamage'),
    enemyRecovery   : TextManager.getter('message', 'enemyRecovery'),
    enemyGain       : TextManager.getter('message', 'enemyGain'),
    enemyLoss       : TextManager.getter('message', 'enemyLoss'),
    enemyDrain      : TextManager.getter('message', 'enemyDrain'),
    enemyNoDamage   : TextManager.getter('message', 'enemyNoDamage'),
    enemyNoHit      : TextManager.getter('message', 'enemyNoHit'),
    evasion         : TextManager.getter('message', 'evasion'),
    magicEvasion    : TextManager.getter('message', 'magicEvasion'),
    magicReflection : TextManager.getter('message', 'magicReflection'),
    counterAttack   : TextManager.getter('message', 'counterAttack'),
    substitute      : TextManager.getter('message', 'substitute'),
    buffAdd         : TextManager.getter('message', 'buffAdd'),
    debuffAdd       : TextManager.getter('message', 'debuffAdd'),
    buffRemove      : TextManager.getter('message', 'buffRemove'),
    actionFailure   : TextManager.getter('message', 'actionFailure')
});
"#;

/// 一条路径的文本：VFS 的字节 → 去掉 BOM → 字符串。读不到（或路径不合法）交回**空串**。
///
/// 空串 = "没有"，代价是"0 字节的合法 JSON 文件"读不成 —— 实测上游 `data/` 的 713 个 `.json` 里
/// 0 字节的 0 个（最小 293 字节），所以这一格不落第二个返回通道。
/// 去 BOM 与 [`crate::mv_data`] 走的是同一条口径（`data/*.json` 实测无 BOM，只有插件私有的
/// `MV_Project.json` 有，而那一颗我们不读）。
pub fn read_text(vfs: &Vfs, name: &str) -> String {
    let Ok(path) = VfsPath::new(name) else {
        return String::new();
    };
    let Some(bytes) = vfs.read(&path) else {
        return String::new();
    };
    String::from_utf8_lossy(&bytes)
        .trim_start_matches('\u{feff}')
        .to_string()
}

/// `ImageManager` 的**名字那一半**（`rpg_managers.js:908-920` 的三句）。
///
/// 为什么这三句随 M10.3 的 b 一起交（2026-10-04 量出来的）：建一颗 `Game_Map` 就要走到它们 ——
/// `Game_CharacterBase.setImage` 问 `isObjectCharacter`，`Game_Map.setupParallax` 问
/// `isZeroParallax`（不交就是 `ReferenceError: ImageManager is not defined` 那一句，
/// 也就是"地图建不起来"）。而这三句**只碰名字**（`match(/^[^\!\$]+/)` 那一族的反向），
/// 三句的体只有 `filename.match(/^[\!\$]+/)`、`sign[0].contains(…)` 与 `charAt(0)`，一位图都不碰
/// ⇒ 与本模块头里"`ImageManager` 的落点是 `Bitmap`"那条改判不冲突：交的只有名字那一半，
/// `loadXXX` 与缓存那一整族仍记 M10.4（request/reserve 那两族的边界见
/// [`IMAGE_MANAGER_REQUEST_JS`]，那是 d 那一轮量出来之后提前交的一小格）。
///
/// 文本是上游逐字（`contains` 来自 [`crate::shapes::install_extensions`] 交的那条内建扩展，
/// 少它这三句全抛）。
pub const IMAGE_MANAGER_NAMES_JS: &str = r##"
ImageManager.isObjectCharacter = function(filename) {
    var sign = filename.match(/^[\!\$]+/);
    return sign && sign[0].contains('!');
};

ImageManager.isBigCharacter = function(filename) {
    var sign = filename.match(/^[\!\$]+/);
    return sign && sign[0].contains('$');
};

ImageManager.isZeroParallax = function(filename) {
    return filename.charAt(0) === '!';
};
"##;

/// `SceneManager` 的**热路径那一格**（`rpg_managers.js:2052-2054` 逐字）。
///
/// 为什么随 M10.3 的 d 交，而 `goto`/`push`/`prepareNextScene` 那三格不交（2026-10-04 量出来的）：
/// `Game_Interpreter.prototype.update` 的 `while (this.isRunning())` 循环**每一轮**读
/// `SceneManager.isSceneChanging()`（`rpg_objects.js:8835`，就在
/// `Game_Interpreter.prototype.update` 那个 `while` 的第一句）—— 不交这一格，解释器一步都走不了，
/// 而这正是移植侧那条"换图期间旧页冻结"的根（`AGENTS.md` 咬人第 34 条⑷：上游就是靠这一句 break）。
/// 这一格的体只读 `this._exiting` 与 `this._nextScene` 两格，两者在"还没有场景栈"的宿主里都是
/// `undefined` ⇒ 读出 `false`（= 不在换场景），与上游"停在同一场景里跑事件"的那一档同答案；
/// M10.5 交 `goto`/`push`/`prepareNextScene` 之后，这两格由场景机制自己写，这一格一个字都不改。
pub const SCENE_MANAGER_SWITCH_JS: &str = r#"
SceneManager.isSceneChanging = function() {
    return this._exiting || !!this._nextScene;
};
"#;

/// `ImageManager` 的**缓存底**（`rpg_managers.js:793-797,799-801,900-902,1001-1003,1090-1096`
/// 逐字，九格：四格静态底 + `_generateCacheKey` + `clear` + `setDefaultReservationId` +
/// `update` + `clearRequest`）。
///
/// 为什么这一格随 M10.4 的第二格交，而不是等到位图：它底下的那四颗类
/// （`ImageCache`/`CacheEntry`/`CacheMap`/`RequestQueue`）是 core 那一族里**唯一一整圈零硬底**的
/// （`core-touch.py` ① 那四行全是"无"）⇒ 交它不需要位图。交出来之后 `ImageManager` 的三族第一次是真的：
/// ⑴ **去重** —— 同一个 `path + ':' + hue` 的键第二次拿回**同一个对象**（旧写法每次都造一颗新句柄，
/// 而 `Game_Interpreter.requestImages` 沿 `117` 会把同一张图名报几十次）；
/// ⑵ **预留** —— `reserve` 落 `reservationId`、`releaseReservation` 按 id 抹掉，被预留的那一条在
/// `_mustBeHeld` 那一支里"永远留着"（`command282` 与 `Scene_Base` 那一族读的就是这一格）；
/// ⑶ **就绪** —— `ImageManager.isReady()` 走 `ImageCache.isReady()` 的真判据（不是恒真的空壳），
/// 而"request-only 的那些跳过"这一格决定了 `Game_Interpreter` 的 `image` 等待腿等不等得下去。
///
/// `ImageManager.cache = new CacheMap(ImageManager)` 交的是上游的**死代码**，而且它带着上游自己的
/// 毛病（`CacheMap.update` 在上游六颗核心文件里的调用位点是 **0**；`checkTTL` 的 `temp.push` 遇上
/// 死条目就抛，因为 `_lastRemovedEntries` 的初值是空对象；TTL 的判据是反的 —— 刚设下 TTL 的条目
/// 立刻算死）。**逐字交、不修**，形状由 [`crate::core`] 的
/// `the_cache_map_ttl_leg_is_delivered_as_upstreams_broken_shape` 钉住。
///
/// **重灌会把图片缓存换成一颗新的**（这一格的 `new ImageCache()` 与上游 `ImageManager.clear()`
/// （`:900-902`）是同一条腿）：兼容层里素材字节都在宿主 VFS/清单那一侧，所以重灌不丢任何真东西，
/// 但这一格不许静默 ⇒ 判据 `reinstalling_managers_resets_the_image_cache` 钉住它。
pub const IMAGE_MANAGER_CACHE_JS: &str = r##"
ImageManager.cache = new CacheMap(ImageManager);

ImageManager._imageCache = new ImageCache();
ImageManager._requestQueue = new RequestQueue();
ImageManager._systemReservationId = Utils.generateRuntimeId();

ImageManager._generateCacheKey = function(path, hue){
    return  path + ':' + hue;
};

ImageManager.clear = function() {
    this._imageCache = new ImageCache();
};

ImageManager.setDefaultReservationId = function(reservationId){
    this._defaultReservationId = reservationId;
};

ImageManager.update = function(){
    this._requestQueue.update();
};

ImageManager.clearRequest = function(){
    this._requestQueue.clear();
};
"##;

/// `ImageManager` 的 **request / reserve 两族的薄壳**（`rpg_managers.js:904-1068` 逐字，19 格），
/// 外加**四格自创的底**（M10.4 第二格之前是五格：`_imageCache` 那一格已经换成上游那颗真的
/// `new ImageCache()`，见 [`IMAGE_MANAGER_CACHE_JS`]）。
///
/// 为什么这一族随 M10.3 的 d 交，而不是等 M10.4：`Game_Interpreter.prototype.setup`
/// （`rpg_objects.js:8801-8806`）**无条件**调静态的 `Game_Interpreter.requestImages(list)`，
/// 而那一句沿 `117` 递归进公共事件、把列表里每一条 `101/129/205/212/216/231/282/283` 的图名都喂给
/// `ImageManager.requestXxx` ⇒ 少交这一族就是**每一次 setup 都抛**，d 的 151 条方法一条也跑不起来。
/// 数量是量出来的（`tools/probes/leg-deps.py --leg d`）：d 的体里 `ImageManager` 出现 20 处，
/// 落在 `command282` / `pluginCommand` / `updateWaitMode` 与那颗静态成员上。
///
/// **上游那 19 格一条都不改**：它们全是薄壳，真正有价值的是**路径拼写**
/// （`folder + encodeURIComponent(filename) + '.png'` 与那十四个 `img/…/` 目录名），
/// 而这一串正是移植侧的预取泵与 `VfsPath` 规范化要对表的东西（`AGENTS.md` 咬人第 32 条⑶同一条腿）。
///
/// **底下四格是自创的**（`_sisfHandle`/`loadEmptyBitmap`/`requestNormalBitmap`/`reserveNormalBitmap`）：
/// 上游那四格的底是 `Bitmap`（`rpg_core.js`，38 条里 **22 条有硬底**、16 条没有，而"没有"里好几条
/// 又落在硬底的下一格上），本轮不交。四格的**账目走上面那颗真 `ImageCache`**，与上游逐字的差只有
/// 替换掉的三处：⑴ `Bitmap.request(path)` / `new Bitmap()` → `this._sisfHandle(…)`；
/// ⑵ `loadNormalBitmap(path, hue)` → `requestNormalBitmap(path, hue)`（load 那一族底下是
/// `Bitmap.load` → `_requestImage`，DOM `Image` + `Decrypter.decryptImg`，本轮不交）；
/// ⑶ `addLoadListener(function(){ bitmap.rotateHue(hue); })` 那一整支不挂 —— 这不是"顺手省掉"：
/// `rotateHue` 对 `hue == 0` 在上游就是**立刻返回**（`:1448` 的
/// `if (offset && this.width > 0 && this.height > 0)`），而 `Game_Interpreter.requestImages`
/// 那一条腿**一支 hue 都不传**（`:10516-10576` 全是单参调用 ⇒ `hue || 0`）。数据侧的非零 hue 也量了
/// （`core-touch.py --demand`）：`Animations.json` 444 条里 32 条、`Enemies.json` 234 条里 4 条，
/// 两族都走 sprites / 战斗那一侧（M10.5 与非目标）。
///
/// 换底的时候上面那 19 格一个字都不动，而判据
/// `the_home_made_image_manager_bases_are_exactly_four` 会先红（它原来钉的是五格），逼着把账改过来。
pub const IMAGE_MANAGER_REQUEST_JS: &str = r##"
ImageManager.requestAnimation = function(filename, hue) {
    return this.requestBitmap('img/animations/', filename, hue, true);
};

ImageManager.requestBattleback1 = function(filename, hue) {
    return this.requestBitmap('img/battlebacks1/', filename, hue, true);
};

ImageManager.requestBattleback2 = function(filename, hue) {
    return this.requestBitmap('img/battlebacks2/', filename, hue, true);
};

ImageManager.requestEnemy = function(filename, hue) {
    return this.requestBitmap('img/enemies/', filename, hue, true);
};

ImageManager.requestCharacter = function(filename, hue) {
    return this.requestBitmap('img/characters/', filename, hue, false);
};

ImageManager.requestFace = function(filename, hue) {
    return this.requestBitmap('img/faces/', filename, hue, true);
};

ImageManager.requestParallax = function(filename, hue) {
    return this.requestBitmap('img/parallaxes/', filename, hue, true);
};

ImageManager.requestPicture = function(filename, hue) {
    return this.requestBitmap('img/pictures/', filename, hue, true);
};

ImageManager.requestSvActor = function(filename, hue) {
    return this.requestBitmap('img/sv_actors/', filename, hue, false);
};

ImageManager.requestSvEnemy = function(filename, hue) {
    return this.requestBitmap('img/sv_enemies/', filename, hue, true);
};

ImageManager.requestSystem = function(filename, hue) {
    return this.requestBitmap('img/system/', filename, hue, false);
};

ImageManager.requestTileset = function(filename, hue) {
    return this.requestBitmap('img/tilesets/', filename, hue, false);
};

ImageManager.requestTitle1 = function(filename, hue) {
    return this.requestBitmap('img/titles1/', filename, hue, true);
};

ImageManager.requestTitle2 = function(filename, hue) {
    return this.requestBitmap('img/titles2/', filename, hue, true);
};

ImageManager.requestBitmap = function(folder, filename, hue, smooth) {
    if (filename) {
        var path = folder + encodeURIComponent(filename) + '.png';
        var bitmap = this.requestNormalBitmap(path, hue || 0);
        bitmap.smooth = smooth;
        return bitmap;
    } else {
        return this.loadEmptyBitmap();
    }
};

ImageManager.reserveTileset = function(filename, hue, reservationId) {
    return this.reserveBitmap('img/tilesets/', filename, hue, false, reservationId);
};

ImageManager.reserveBitmap = function(folder, filename, hue, smooth, reservationId) {
    if (filename) {
        var path = folder + encodeURIComponent(filename) + '.png';
        var bitmap = this.reserveNormalBitmap(path, hue || 0, reservationId || this._defaultReservationId);
        bitmap.smooth = smooth;
        return bitmap;
    } else {
        return this.loadEmptyBitmap();
    }
};

ImageManager.releaseReservation = function(reservationId){
    this._imageCache.releaseReservation(reservationId);
};

ImageManager.isReady = function() {
    return this._imageCache.isReady();
};

// ↓↓↓ 上面到这一行为止都是**上游逐字**。下面四格是**自创的底**（见文档注释）：
// 底是 `Bitmap`（本轮不交），但**账目走上面那颗真的 `ImageCache`/`RequestQueue`** ——
// 句柄必须答得上缓存问的那九格，否则 `_truncateCache`/`_mustBeHeld`/`isReady`/`RequestQueue.update`
// 会在自己人不认识的东西上抛（判据 `the_handle_answers_the_cache_contract`）。
ImageManager._sisfHandle = function(url) {
    return {
        url: url,
        smooth: false,
        width: 0,
        height: 0,
        // 素材的"取"发生在宿主的 VFS/清单那一侧，JS 这一侧的句柄永远算就绪（同 d 那一轮的口径）。
        isReady: function() { return true; },
        isError: function() { return false; },
        // 上游 `isRequestOnly()` 是 `!(this._decodeAfterRequest || this.isReady())` ⇒ 就绪的句柄是 false。
        isRequestOnly: function() { return false; },
        isRequestReady: function() { return true; },
        startRequest: function() {},
        addLoadListener: function(listener) { listener(this); }
    };
};

// 上游 `:873-882`，只有 `new Bitmap()` 那一句换成句柄（`Bitmap` 的构造体第一句就是 `_createCanvas`）。
ImageManager.loadEmptyBitmap = function() {
    var empty = this._imageCache.get('empty');
    if(!empty){
        empty = this._sisfHandle('');
        this._imageCache.add('empty', empty);
        this._imageCache.reserve('empty', empty, this._systemReservationId);
    }

    return empty;
};

// 上游 `:1073-1088`，只有 `Bitmap.request(path)` 换成句柄、`addLoadListener(rotateHue)` 那一支不挂
// （hue 在已交付的文本里恒 0，理由见文档注释⑶）。
ImageManager.requestNormalBitmap = function(path, hue){
    var key = this._generateCacheKey(path, hue);
    var bitmap = this._imageCache.get(key);
    if(!bitmap){
        bitmap = this._sisfHandle(path);
        this._imageCache.add(key, bitmap);
        this._requestQueue.enqueue(key, bitmap);
    }else{
        this._requestQueue.raisePriority(key);
    }

    return bitmap;
};

// 上游 `:990-995`，只有 `loadNormalBitmap` 换成 `requestNormalBitmap`（load 那一族欠的是 `Bitmap.load`）。
ImageManager.reserveNormalBitmap = function(path, hue, reservationId){
    var bitmap = this.requestNormalBitmap(path, hue);
    this._imageCache.reserve(this._generateCacheKey(path, hue), bitmap, reservationId);

    return bitmap;
};

"##;

/// `rpg_managers.js:1103-1641` 的**音频两圈静态成员**：`AudioManager`（38 条函数型静态成员 +
/// 15 格数据形状 + 5 块音量访问器 = 58 格）与 `SoundManager`（27 条函数型静态成员）= **85 格**。文本由
/// `../sisf_pray/tools/probes/objects-slice.py --file rpg_managers.js --classes AudioManager,SoundManager`
/// 切出，`objects-verbatim.py` 逐字对表（那一把只数 `= function` 与访问器块 ⇒ 数到 **70** 格，
/// 15 格数据形状由判据 `the_audio_circles_are_fifty_three_and_twenty_seven_cells` 数）。
///
/// **为什么这一格非交不可**（`core-touch.py --demand audio` 的"交付文本"那一列）：已经交出去的
/// 那 44 段 JS 今天就在喊 `AudioManager` 的 **10 格**（19 处：`playBgm` 5 / `playBgs` 3 / `saveBgm` 3 /
/// `playSe` 2 / `fadeOutBgm`·`fadeOutBgs`·`playMe`·`stopSe`·`replayBgm`·`saveBgs` 各 1，
/// 落在 `objects_interpreter.js` 7 处、`objects_temp_and_system.js` 8 处、`objects_map_and_events.js` 3 处、
/// `objects_characters.js` 1 处）与 `SoundManager` 的 **11 格**（13 处，`objects_battle.js` 7 +
/// `objects_actor_and_party.js` 6）—— 那些壳只保住"名字在场"，喊上去就是 `not a function`。
/// 53 + 27 格里**没有一格有硬底**（`AudioManager` 那六格原先报的 "Web Audio 1" 是尺子把
/// `this.createBuffer` 当成 `AudioContext.prototype.createBuffer` 的假阳性，`M10.md` 事实 67）。
pub const MANAGERS_AUDIO_JS: &str = include_str!("js/managers_audio.js");

/// 装上 managers 的数据那一半。`vfs` 是宿主的那一份只读来源（native 磁盘 / 浏览器清单）。
///
/// 顺序要求在 [`crate::platform::install`] **之后**（`DataManager.isBattleTest` 读 `Utils.isOptionValid`，
/// `ResourceHandler._reloaders` 由平台件交出），在 [`crate::shapes::install`] 与
/// [`crate::shapes::install_extensions`] 之后（静态类形状 + `.format`/`.padZero`/`.contains` 三条内建扩展
/// 是本族函数体的直接依赖）。重复装载 = 核心层又声明一遍（与 `platform::install` 同一条语义，
/// 判据 `reinstalling_managers_is_the_core_layer_again` 钉住 `TextManager` 那 64 格重复 define 不抛）。
///
/// **本族自己先把 core 的缓存四颗装上**（[`crate::core::install_cache`]，幂等）：
/// [`IMAGE_MANAGER_CACHE_JS`] 那九格里有三句 `new ImageCache()` / `new CacheMap(…)` /
/// `new RequestQueue()`，装载期就要这三颗类在场。把"谁先装"留给调用方去记，就会有一格
/// 内部前提变成整族的口径（判据 `managers_install_does_not_depend_on_the_caller_installing_core_first`
/// 钉这一格：只装 shapes + managers，不装 core，也必须有那三颗类）。
pub fn install(host: &Host, vfs: Rc<Vfs>) -> Result<(), String> {
    let source = Rc::clone(&vfs);
    host.bind_str(READ_TEXT_GLOBAL, move |path| read_text(&source, &path))?;
    crate::core::install_cache(host)?;
    for text in [
        DECRYPTER_JS,
        RESOURCE_HANDLER_LOADER_JS,
        DATA_MANAGER_JS,
        CREATE_GAME_OBJECTS_JS,
        TEXT_MANAGER_JS,
        IMAGE_MANAGER_NAMES_JS,
        SCENE_MANAGER_SWITCH_JS,
        IMAGE_MANAGER_CACHE_JS,
        IMAGE_MANAGER_REQUEST_JS,
    ] {
        host.eval(text)?;
    }
    // **音频那一圈排在 managers 的装载末尾**，而且它要 `WebAudio` 那颗类在场：
    // `AudioManager.audioFileExt`（`rpg_managers.js:1487`）的第一句就是 `WebAudio.canPlayOgg()`，
    // 而 `canPlayOgg` 不是壳能给的一格。与缓存四颗同一条纪律 —— 次序由本族自己负，
    // 不由调用方记得（判据 `managers_install_installs_the_audio_round_itself` 钉这一格）。
    crate::core::install_audio(host)?;
    host.eval(MANAGERS_AUDIO_JS)
        .map_err(|e| format!("`AudioManager`/`SoundManager` 那两圈装不上：{e}"))
}

#[cfg(all(test, feature = "js"))]
mod tests {
    //! 机制判据：全部用 fixture 的 `Vfs`（这一颗 crate 不许知道某一款游戏在哪），
    //! 真数据那两条在 `sisf_pray/src/compat.rs`。
    use super::*;
    use crate::shapes::{self, DeclaredShape, ShapeKind};
    use sisf_asset::MemoryProvider;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    /// 本族依赖的静态类形状（`Scene_Boot` 只装形状，用来量 `onLoad` 那一条守卫）。
    const STATICS: &[&str] = &[
        "Decrypter",
        "ResourceHandler",
        "DataManager",
        "TextManager",
        "ImageManager",
        "Utils",
        "Scene_Boot",
        "Graphics",
        "SceneManager",
        // M10.4 的第三格：`MANAGERS_AUDIO_JS` 那 85 格挂在三颗静态类上，而这一族的 fixture
        // 只 shapes 自己点名的那些 ⇒ 不点名就是 `AudioManager is not defined`。
        "AudioManager",
        "SoundManager",
        "Html5Audio",
    ];

    /// M10.4 第二格起，`managers::install` 自己会装 core 的缓存四颗 ⇒ 这四颗的**形状**要在场
    /// （`ImageCache`/`RequestQueue` 沿用壳的 `initialize` 派分，`CacheEntry`/`CacheMap` 的构造体
    /// 由那一份文本覆盖）。
    /// `WebAudio` 也在这一档：`managers::install` 现在自己先装 core 的音频那一圈
    /// （`AudioManager.audioFileExt` 第一句就喊 `WebAudio.canPlayOgg`），而它的构造体是
    /// `this.initialize.apply(this, arguments);` ⇒ 沿用壳。
    const CLASSES: &[&str] = &[
        "CacheEntry",
        "CacheMap",
        "ImageCache",
        "RequestQueue",
        "WebAudio",
    ];

    /// fixture 的密钥：**运行期算出来**，源码里一个 hex 字面量都不写。
    ///
    /// 这一族的纪律是"密钥只从 `data/System.json` 读"（`Decrypter.readEncryptionkey`，
    /// `rpg_core.js:9262`），而 `sisf_pray/src/key.rs::key_never_hardcoded` 现在扫**两个仓库**的
    /// `.rs` —— 兼容层的 fixture 里写一串长得像密钥的东西，下一轮就分不清是样品还是漏了。
    /// 0..16 那十六个字节折成的串与真工程的那一条不可能相同，而判据要的"16 对 / 首 `00` / 末 `0f`"
    /// 全都读得到。
    fn fixture_key() -> String {
        (0u8..16).map(|b| format!("{b:02x}")).collect()
    }

    /// 一份最小可用的 MV `data/`：System（带密钥与 terms）+ 一张有 `indent: null` 与负图块号的图 +
    /// Items/Actors + `_databaseFiles` 那十四格里剩下的九格（`[null]` 就够 —— 本族只看名字在不在）。
    ///
    /// `classes = false` 那一档**故意少 `data/Classes.json`**：那是 transport 改判第 2 条
    /// "还没到 vs 坏了"那条腿的样品（判据 `a_missing_data_file_is_pending_not_an_error`）。
    fn fixture(with_classes: bool) -> Vfs {
        let mut mem = MemoryProvider::new();
        let put = |mem: &mut MemoryProvider, name: &str, text: &str| {
            mem.insert(&VfsPath::new(name).unwrap(), text.as_bytes());
        };
        let system = format!(
            r#"{{"hasEncryptedImages":true,"hasEncryptedAudio":true,
                "encryptionKey":"{key}",
                "currencyUnit":"¥","startMapId":8,"startX":19,"startY":14,
                "terms":{{"basic":["Lv","LvA","HP","HPA","MP","MPA","TP","TPA","EXP","EXPA"],
                  "params":["maxhp","mp"],
                  "commands":["fight","escape","attack","guard","item","skill","equip","status",
                    "formation","save","gameEnd","options","weapon","armor","keyItem","equip2",
                    "optimize","clear","newGame","continue","","标题","取消","","购买","卖出"],
                  "messages":{{"alwaysDash":"Always Dash","possession":"Gold","actionFailure":"%1 failed!"}}}}}}"#,
            key = fixture_key()
        );
        put(&mut mem, "data/System.json", &system);
        put(
            &mut mem,
            "data/Map008.json",
            r#"{"width":40,"height":30,"scrollType":3,"events":[null,
                {"id":1,"name":"EV001","note":"<宝箱:3>\n<只标>","pages":[{"indent":null,
                "list":[{"code":101,"indent":null,"parameters":["A","","",0]}]}]}],
                "data":[-1073741824, 0, 1]}"#,
        );
        put(
            &mut mem,
            "data/Items.json",
            r#"[null,{"id":1,"name":"药水","note":"","itypeId":1}]"#,
        );
        put(
            &mut mem,
            "data/Actors.json",
            r#"[null,{"id":1,"name":"阿尔cy"}]"#,
        );
        for name in [
            "Skills",
            "Weapons",
            "Armors",
            "Enemies",
            "Troops",
            "States",
            "Animations",
            "Tilesets",
            "CommonEvents",
            "MapInfos",
        ] {
            put(&mut mem, &format!("data/{name}.json"), "[null]");
        }
        // `classes = false` 那一档把这一格抽掉：它就是"缺文件"那条腿的样品。
        if with_classes {
            put(
                &mut mem,
                "data/Classes.json",
                r#"[null,{"id":1,"name":"战士","note":""}]"#,
            );
        }
        let mut vfs = Vfs::new();
        vfs.register(Box::new(mem));
        vfs
    }

    fn fixture_data() -> Vfs {
        fixture(true)
    }

    /// 与 `install` 同一顺序的完整装载（形状 → 内建扩展 → managers）。平台件那一族只有
    /// `Utils.isOptionValid` 是本族的直接依赖，所以这里装的是 `platform` 里那一颗 —— 用
    /// `DataManager.isBattleTest` 问它，答案必须是 false（QuickJS 与 node 都没有查询串）。
    fn booted(vfs: Vfs) -> Host {
        let host = Host::boot();
        let mut shapes: Vec<DeclaredShape> = STATICS
            .iter()
            .map(|n| DeclaredShape {
                name: (*n).to_string(),
                kind: ShapeKind::StaticClass,
            })
            .collect();
        shapes.extend(CLASSES.iter().map(|n| DeclaredShape {
            name: (*n).to_string(),
            kind: ShapeKind::Class,
        }));
        shapes::install(&host, &shapes).expect("静态类形状装不上");
        shapes::install_extensions(&host).expect("JsExtensions 装不上");
        host.eval(crate::platform::HOST_GLOBALS_JS)
            .expect("宿主全局面装不上");
        host.eval(crate::platform::UTILS_JS)
            .expect("Utils 装不上（isOptionValid 是本族的依赖）");
        host.eval(crate::platform::RESOURCE_HANDLER_JS)
            .expect("ResourceHandler 的状态那一半装不上");
        install(&host, Rc::new(vfs)).expect("managers 装不上");
        host
    }

    fn booted_default() -> Host {
        booted(fixture_data())
    }

    #[test]
    fn load_database_fills_the_names_the_registry_asks_for() {
        let h = booted_default();
        h.eval("DataManager.loadDatabase();")
            .expect("loadDatabase 失败");
        assert_eq!(h.eval_i32("DataManager._databaseFiles.length"), 14);
        assert!(
            h.eval_bool("DataManager.isDatabaseLoaded()"),
            "十四格都该有（fixture 给齐了），而 `isDatabaseLoaded` 只看名单不看别的"
        );
        assert_eq!(h.eval_string("typeof $dataSystem"), "object");
        assert_eq!(h.eval_i32("$dataSystem.startMapId"), 8);
        // 数组形状（下标 0 是空槽）必须原样交回 —— 插件全靠 `$dataItems[params[i]]` 这一写法。
        assert_eq!(h.eval_i32("$dataItems.length"), 2);
        assert!(h.eval_bool("$dataItems[0] === null && $dataItems[1].name === '药水'"));
        assert!(
            h.eval_bool("DataManager._pendingFiles.length === 0"),
            "给齐了就不该报名字"
        );
    }

    #[test]
    fn load_database_sets_the_decrypter_flags_from_the_data() {
        // `onLoad` 的那三条腿（`rpg_managers.js:146-150`）：两位旗标 + 密钥从数据里来。
        let h = booted_default();
        assert!(
            !h.eval_bool("Decrypter.hasEncryptedImages"),
            "读库之前两位该是上游的初值 false（:9153）"
        );
        h.eval("DataManager.loadDatabase();")
            .expect("loadDatabase 失败");
        assert!(h.eval_bool("Decrypter.hasEncryptedImages"));
        assert!(h.eval_bool("Decrypter.hasEncryptedAudio"));
        // 密钥那一格只有 `readEncryptionkey` 会取，而它读的是 `$dataSystem.encryptionKey` ——
        // 于是"兼容层不带密钥"这一格在机制层就成立（真数据那一条在 pray 侧扫两个仓库）。
        h.eval("Decrypter.readEncryptionkey();")
            .expect("readEncryptionkey 失败");
        assert_eq!(h.eval_i32("Decrypter._encryptionKey.length"), 16);
        assert_eq!(h.eval_string("Decrypter._encryptionKey[0]"), "00");
        assert_eq!(h.eval_string("Decrypter._encryptionKey[15]"), "0f");
    }

    #[test]
    fn a_missing_data_file_is_pending_not_an_error() {
        // transport 改判第 2 条的落点：`Classes.json` 不在 VFS 里 ⇒ 名字报进 `_pendingFiles`、
        // `_errorUrl` 保持 null（`checkError` 不抛）、`isDatabaseLoaded()` 假。
        let h = booted(fixture(false));
        h.eval("DataManager.loadDatabase();")
            .expect("loadDatabase 失败");
        assert_eq!(h.eval_string("typeof $dataClasses"), "object");
        assert!(
            h.eval_bool("!$dataClasses"),
            "缺文件时那一格必须是 null（上游的 `window[name] = null` 先落这一格）"
        );
        assert_eq!(
            h.eval_string("DataManager._pendingFiles[0]"),
            "data/Classes.json",
            "十三格到齐、一格没到 ⇒ 报名字的那一本必须说得出是哪一格"
        );
        assert_eq!(h.eval_i32("DataManager._pendingFiles.length"), 1);
        assert!(
            h.eval_bool("!DataManager._errorUrl"),
            "还没到的字节不是错误 —— 设了 _errorUrl，checkError 就把可重试读成坏了"
        );
        h.eval("DataManager.checkError();")
            .expect("checkError 不该抛");
        assert!(!h.eval_bool("DataManager.isDatabaseLoaded()"));
        // 补上字节再问一次：那一格变真、名字从 pending 里掉出去（浏览器腿的波就是这个形状）。
        // 用**第二颗宿主**而不是同一颗：`cargo xtask wasm-test` 那条腿（node）一整个进程共享一个
        // realm，`DataManager` 这些全局名两边是同一颗（`../sisf_pray/M10.md`「这一族的规矩」"手交的 JS 文本要能装
        // 第二次"那条同源），所以在同一 realm 里断言"上一本的 pending 还在"读的不是实现。
        let h2 = booted(fixture(true));
        h2.eval("DataManager.loadDatabase();")
            .expect("补齐后 loadDatabase 失败");
        assert_eq!(h2.eval_i32("$dataClasses.length"), 2);
        assert!(
            h2.eval_bool("DataManager._pendingFiles.length === 0"),
            "补齐那一格要把名字从 pending 里摘掉，否则 HUD 一直报缺"
        );
        assert!(h2.eval_bool("DataManager.isDatabaseLoaded()"));
    }

    #[test]
    fn note_tags_are_extracted_at_load_for_the_map_and_every_entry() {
        // `onLoad` 是 YEP 那一族的别名锚点（12 个 `processXXXNotetags` 挂在它后面），
        // 所以它必须自己先把 `meta` 做出来：图对象 + 每个 `note !== undefined` 的条目。
        let h = booted_default();
        h.eval("DataManager.loadDatabase();")
            .expect("loadDatabase 失败");
        h.eval("DataManager.loadMapData(8);")
            .expect("loadMapData 失败");
        assert_eq!(h.eval_string("$dataMap.events[1].meta['宝箱']"), "3");
        assert!(h.eval_bool("$dataMap.events[1].meta['只标'] === true"));
        // 上游的 `onLoad` 只对 **`$dataMap` 本身**与**数组条目**做 meta（`:130-145`）：`$dataSystem`
        // 是对象又不是 `$dataMap` ⇒ 一条都不做。别"顺手补上"，插件里有按 `meta === undefined` 分支的。
        assert!(
            h.eval_bool("$dataSystem.meta === undefined"),
            "对象那一格上游不做 extractMetadata"
        );
        // `note` 是空串的条目照样过（`note !== undefined`）⇒ `meta` 是一颗空对象。
        assert!(h.eval_bool("Object.keys($dataItems[1].meta).length === 0"));
        // `_mapLoader` 那一格：上游第一句就取 `ResourceHandler.createLoader`，同步 transport
        // 用不到它，但它必须是函数（M10.4 的失败面要回来叫它）。
        assert_eq!(
            h.eval_string("typeof DataManager._mapLoader"),
            "function",
            "`createLoader` 没交 ⇒ loadMapData 第一句就会抛"
        );
        assert!(h.eval_bool("DataManager.isMapLoaded()"));
        // mapId 0 走 `makeEmptyMap`（上游那六个字段）。
        h.eval("DataManager.loadMapData(0);")
            .expect("loadMapData(0) 失败");
        assert_eq!(h.eval_i32("$dataMap.width"), 100);
        assert!(h.eval_bool("Array.isArray($dataMap.data) && $dataMap.scrollType === 3"));
    }

    #[test]
    fn the_js_and_rust_encrypted_extension_legs_are_the_same_rule() {
        // 两条腿各有一份"逻辑名 → 加密名"：JS 侧是给插件看的 `Decrypter.extToEncryptExt`，
        // Rust 侧是 provider 用的 [`crate::mv_provider::encrypted_ext`]。两份实现必须同一个答案，
        // 否则加载器等的路径与 provider 取到的路径不是同一条（`sisf_pray` 咬人第 32 条⑶那条根）。
        let h = booted_default();
        // 两边**必须**一致的只有上游真的会造的那三条（`Bitmap` 一律补 `.png`、`AudioManager`
        // 一律 `.ogg`/`.m4a`，见 `rpg_managers.js:1487` 的 `audioFileExt`）。
        for (name, enc) in [("png", "rpgmvp"), ("ogg", "rpgmvo"), ("m4a", "rpgmvm")] {
            let url = format!("img/pictures/a.{name}");
            let js = h.eval_string(&format!("Decrypter.extToEncryptExt('{url}')"));
            let rust = format!(
                "img/pictures/a.{}",
                crate::decrypt::encrypted_ext(name).expect("那三条是上游的映射表")
            );
            assert_eq!(js, rust, "{name} 那一条两边不一致");
            assert_eq!(js, format!("img/pictures/a.{enc}"));
        }
        // **量出来的分歧**（不是 bug，是上游那一格的形状）：扩展名不在那三条里时
        // `extToEncryptExt` 会把点吃掉（`a.bmp` → `abmp`），而 Rust 侧的 provider 对没映射的
        // 名字返回 `None` ⇒ 走原名。这一格不落 JS 侧的原因：上游从不把 `.bmp` 喂给它
        // （`ImageManager.loadBitmap` 只接 `.png`），真工程那 13 张 `.bmp` 是**明文回退**那条腿
        // 读到的（`mv_provider` 模块头第 2 条），而那条腿只有 Rust 侧有。
        assert_eq!(
            h.eval_string("Decrypter.extToEncryptExt('img/pictures/a.bmp')"),
            "img/pictures/abmp"
        );
        assert!(
            h.eval_bool("Decrypter.checkImgIgnore('img/system/Window.png')"),
            "`_ignoreList` 里就有这一格（:9159-9161）"
        );
        assert!(
            !h.eval_bool("Decrypter.checkImgIgnore('img/system/Loading.png')"),
            "上游的 `_ignoreList` **只**列了 Window.png（Loading.png 实测明文，走的也是明文回退那条腿）"
        );
    }

    #[test]
    fn text_manager_terms_come_from_the_loaded_system() {
        let h = booted_default();
        h.eval("DataManager.loadDatabase();")
            .expect("loadDatabase 失败");
        assert_eq!(h.eval_string("TextManager.levelA"), "LvA");
        assert_eq!(h.eval_string("TextManager.currencyUnit"), "¥");
        // 上游那 26 格命令位里，`TextManager` 用的索引不是连号的（`buy` 是 24、`sell` 是 25，
        // 中间 23 是空槽）—— 这一格钉的就是"照抄的那张索引表没飘"。
        assert_eq!(h.eval_string("TextManager.buy"), "购买");
        assert_eq!(h.eval_string("TextManager.sell"), "卖出");
        assert_eq!(h.eval_string("TextManager.toTitle"), "标题");
        assert_eq!(h.eval_string("TextManager.cancel"), "取消");
        assert_eq!(h.eval_string("TextManager.param(0)"), "maxhp");
        assert_eq!(h.eval_string("TextManager.alwaysDash"), "Always Dash");
        assert_eq!(h.eval_string("TextManager.actionFailure"), "%1 failed!");
        // 那 64 格是**取值器**，插件会就地覆盖一格（`Vitsuno_Difficulty` 就改 difficulty 那一族）。
        assert!(h.eval_bool(
            "Object.getOwnPropertyDescriptor(TextManager, 'level').configurable === true"
        ));
        // 空槽（`command(20)` 上游是 ''、`getter('command', 23)` 那格我们没列）→ `|| ''` 兜住。
        assert_eq!(
            h.eval_string("TextManager.getter('command', 999).get.call(TextManager)"),
            ""
        );
    }

    #[test]
    fn the_contains_leg_decides_is_item_and_friends() {
        // 四条 `is*` 用的是 `Array.prototype.contains`（`JsExtensions`，M10.1 那一格）而不是 `indexOf`，
        // 所以"同一个对象在不在表里"必须按引用成立。
        let h = booted_default();
        h.eval("DataManager.loadDatabase();")
            .expect("loadDatabase 失败");
        assert!(h.eval_bool("DataManager.isItem($dataItems[1])"));
        assert!(
            !h.eval_bool("DataManager.isItem({id:1,name:'药水'})"),
            "上游按引用比（`contains`），新建的同内容对象不算"
        );
        assert!(!h.eval_bool("DataManager.isSkill($dataItems[1])"));
        assert!(h.eval_bool("!DataManager.isItem(null)"));
        // 测试模式那一族：`loadDatabase` 的 `Test_` 前缀由 `Utils.isOptionValid` 决定，宿主没查询串 ⇒ 0。
        // 读成整数而不是布尔是**上游的形状**：`isOptionValid` 交回的是 `1`/`0`（`rpg_core.js:193-197`，
        // `platform.rs` 逐字照抄），于是 `isBattleTest()` 是 0 而不是 false —— 插件里
        // `DataManager.isBattleTest() === false` 那一写法在上游本来就是假。
        assert_eq!(h.eval_i32("DataManager.isBattleTest()"), 0);
        assert_eq!(h.eval_i32("DataManager.isEventTest()"), 0);
        assert_eq!(h.eval_i32("$dataSkills.length"), 1);
    }

    #[test]
    fn reinstalling_managers_is_the_core_layer_again() {
        // 共享 realm 那条纪律（`../sisf_pray/M10.md`「这一族的规矩」：手交的每段 JS 文本都要能装第二次）：
        // `TextManager` 那 64 格 + `currencyUnit` 全是 `configurable: true`，重复 define 不抛；
        // 而 `_pendingFiles` 回到空一本（核心层重声明，与 `platform` 同一语义）。
        let h = booted(fixture(false));
        h.eval("DataManager.loadDatabase();")
            .expect("第一次装载失败");
        assert_eq!(h.eval_i32("DataManager._pendingFiles.length"), 1);
        install(&h, Rc::new(fixture(true))).expect("第二次装载失败");
        // 重装之后、还没读库时读一格 term：上游也是抛（`$dataSystem` 是 `null`，`:1652` 直接点它）。
        // 钳成空串就是造第二个真相 —— 这一格钉"照上游抛"。
        assert!(
            h.try_eval_bool("TextManager.levelA === 'LvA'").is_err(),
            "库还没读时 term 必须抛 TypeError，与上游同形"
        );
        assert!(
            h.eval_bool("DataManager._pendingFiles.length === 0"),
            "重装把上一本的 pending 洗掉是对的（核心层又声明一遍）"
        );
        h.eval("DataManager.loadDatabase();")
            .expect("重装后 loadDatabase 失败");
        assert_eq!(h.eval_string("TextManager.levelA"), "LvA");
        assert!(h.eval_bool("DataManager.isDatabaseLoaded()"));
    }

    #[test]
    fn the_static_class_guard_still_refuses_new() {
        // `shapes` 保住的那条守卫在本族身上也要成立：`new DataManager()` 在上游就是抛。
        let h = booted_default();
        let threw = h
            .try_eval_bool("try { new DataManager(); false } catch (e) { e.message === 'This is a static class' }")
            .unwrap_or(false);
        assert!(
            threw,
            "静态类守卫被哪一步弄没了 —— 插件会 `new` 出半个管理器"
        );
    }

    #[test]
    fn the_image_manager_name_predicates_answer_like_upstream() {
        // 三句只碰名字，所以"交得出这一半"是能数出来的：`!` 是物件角色、`$` 是单张大图、
        // 视差名以 `!` 开头 = 零号视差。旧移植侧 `sisf_pray/src/sheet.rs` 剥前缀走的是同一条规则，
        // 这两条腿必须在同一个数上（少了 `contains` 那条内建扩展，这三句全抛）。
        let host = booted_default();
        let js = r#"
            var out = [];
            out.push(Boolean(ImageManager.isObjectCharacter('!AAlcy')));
            out.push(Boolean(ImageManager.isObjectCharacter('$sell')));
            out.push(Boolean(ImageManager.isObjectCharacter('AAlcy')));
            out.push(ImageManager.isObjectCharacter('AAlcy') === null);
            out.push(Boolean(ImageManager.isBigCharacter('$sell')));
            out.push(Boolean(ImageManager.isBigCharacter('!AAlcy')));
            out.push(ImageManager.isBigCharacter('AAlcy') === null);
            out.push(ImageManager.isZeroParallax('!goburinn2migi') === true);
            out.push(ImageManager.isZeroParallax('sea_of_trees') === false);
            out.join(',')
        "#;
        // 读数是**三条布尔 + 三条"是不是 null"**：上游那两句的体是 `sign && sign[0].contains(…)`，
        // 名字里没有 `!`/`$` 时 `match` 交出 `null` ⇒ 返回的是 `null` 而不是 `false`
        // （`Array.prototype.join` 又把 `null` 折成空串，所以只看 `join` 会读成"少了个值"）。
        // 判据因此把两格分开钉：真值走 `Boolean(…)`，`null` 那一格直接比 `=== null`。
        assert_eq!(
            host.eval_string(js),
            "true,false,false,true,true,false,true,true,true"
        );
    }

    #[test]
    fn reinstalling_the_image_manager_names_is_still_the_core_layer() {
        let host = booted_default();
        host.eval(IMAGE_MANAGER_NAMES_JS)
            .expect("名字那一半第二次装不上");
        assert_eq!(
            host.eval_string("String(ImageManager.isObjectCharacter('!x'))"),
            "true"
        );
    }

    /// `ImageManager` 的 request 那一族交的是**路径拼写**，不是位图（d 那一轮的边界）。
    ///
    /// 上游那十四格薄壳里唯一有信息量的一串是 `folder + encodeURIComponent(filename) + '.png'`
    /// 与那十四个目录名 —— 移植侧的预取泵与 `VfsPath` 规范化对的就是这一串（`AGENTS.md` 咬人
    /// 第 32 条⑶："fetch 用哪份拼写、insert 就必须用同一份"）。空格必须变成 `%20`，
    /// 而**空图名**走的是另一条腿（`loadEmptyBitmap`，句柄的 `url` 是空串）。
    #[test]
    fn the_request_family_spells_the_upstream_folders_and_the_png_suffix() {
        let host = booted_default();
        let js = r#"
var out = [];
out.push(ImageManager.requestFace('TCGAlcybase_1_1').url);
out.push(ImageManager.requestCharacter('AAlcy').url);
out.push(String(ImageManager.requestCharacter('$sell').smooth));
out.push(String(ImageManager.requestFace('sea of trees').smooth));
out.push(ImageManager.requestFace('01 LAZZULI LUVS').url);
out.push(ImageManager.requestFace('ノエト树海').url);
out.push(ImageManager.requestTileset('World_A1').url);
out.push(ImageManager.requestParallax('!goburinn2migi').url);
out.push(ImageManager.requestFace('').url);
out.join('|')
"#;
        assert_eq!(
            host.eval_string(js),
            "img/faces/TCGAlcybase_1_1.png|img/characters/AAlcy.png|false|true|\
             img/faces/01%20LAZZULI%20LUVS.png|img/faces/%E3%83%8E%E3%82%A8%E3%83%88%E6%A0%91%E6%B5%B7.png|\
             img/tilesets/World_A1.png|img/parallaxes/!goburinn2migi.png|"
        );
    }

    /// `command282`（Change Tileset）那一格要的三句：`reserveTileset` 交回"就绪"的句柄、
    /// `releaseReservation` 不抛、`isReady()` 说整个缓存都到位了。
    ///
    /// 上游那一句 `allReady = ….every(function(bitmap) { return bitmap.isReady(); })` 读的是**句柄**，
    /// 所以这一族的落点只要"每颗句柄自己说就绪"就够了；`_waitMode === 'image'` 那一格同一把尺子
    /// （`updateWaitMode` 比的是 `!ImageManager.isReady()`）⇒ 不等待，与上游"字节已在内存"同答案。
    #[test]
    fn the_reservation_leg_answers_for_command282() {
        let host = booted_default();
        let js = r#"
var out = [];
out.push(String(ImageManager.reserveTileset('World_A1', 0, 7).isReady()));
out.push(ImageManager.reserveTileset('World_A1', 0, 7).url);
out.push(String(ImageManager.reserveTileset('', 0, 7).isReady()));
out.push(ImageManager.reserveTileset('', 0, 7).url);
out.push(String(ImageManager.isReady()));
ImageManager.releaseReservation(7);
out.push('released');
out.join('|')
"#;
        assert_eq!(
            host.eval_string(js),
            "true|img/tilesets/World_A1.png|true||true|released"
        );
    }

    /// **自创面只许这四格**（`tools/probes/objects-verbatim.py` 的 `HOME_MADE` 白名单是同一份账）。
    ///
    /// M10.4 的第二格把五格减到四格：`_imageCache` 那一格不再是自创的对象，而是上游那句
    /// `new ImageCache()`（`rpg_managers.js:795`）⇒ 这一条同时钉住"缓存底已经换成真的"。
    /// 剩下的四格都欠同一颗类：`Bitmap`。位图那半落地时这一条**先红**，逼着改账。
    #[test]
    fn the_home_made_image_manager_bases_are_exactly_four() {
        let host = booted_default();
        let js = r#"
var bases = ['_sisfHandle', 'loadEmptyBitmap', 'requestNormalBitmap', 'reserveNormalBitmap'];
var out = [];
for (var i = 0; i < bases.length; i++) out.push(typeof ImageManager[bases[i]]);
out.push(String(ImageManager._imageCache instanceof ImageCache));
out.push(String(ImageManager._requestQueue instanceof RequestQueue));
out.push(String(ImageManager.cache instanceof CacheMap));
out.push(String(typeof ImageManager.loadNormalBitmap));
out.join('|')
"#;
        assert_eq!(
            host.eval_string(js),
            "function|function|function|function|true|true|true|undefined",
            "四格函数底在场、三颗静态底是**上游那三颗类的实例**，而 `loadNormalBitmap` 这一族仍然没交 \
             （它底下是 `Bitmap.load` → `_requestImage`）—— 最后一格钉的就是这一格还没还"
        );
        host.eval(IMAGE_MANAGER_REQUEST_JS)
            .expect("request 那一族第二次装不上（自创的底不许只许装一次）");
        assert_eq!(
            host.eval_string("String(ImageManager.requestFace('x').url)"),
            "img/faces/x.png"
        );
    }

    /// 缓存四颗问"存进去的那个东西"的**九格契约**，宿主的句柄必须全答得上。
    ///
    /// 这是换底的那一格真代价：底从"永远就绪的空壳"换成上游的 `ImageCache` 之后，
    /// `_truncateCache` 会读 `width*height`、`_mustBeHeld` 与 `isReady` 会喊三个谓词、
    /// `RequestQueue.update` 会喊 `isRequestReady()`/`startRequest()`。少一格就是**缓存自己**
    /// 在 `command282` 那一拍上抛，而不是位图那半的账。
    /// 尺寸恒 0 的那两格是**有意**的（像素不在 JS 这一侧）⇒ 裁cache永远不触发，
    /// 由 [`crate::core`] 的 `truncation_fires_only_once_the_items_have_dimensions` 钉住那一段账。
    #[test]
    fn the_handle_answers_the_cache_contract() {
        let host = booted_default();
        let js = r#"
var out = [];
var h = ImageManager._sisfHandle('img/pictures/x.png');
out.push(h.url);
out.push(String(h.width) + ',' + String(h.height));
out.push(String(h.isReady()));
out.push(String(h.isError()));
out.push(String(h.isRequestOnly()));
out.push(String(h.isRequestReady()));
var threw = 'no';
try { h.startRequest(); } catch (e) { threw = 'yes'; }
out.push(threw);
var heard = 'none';
h.addLoadListener(function(b) { heard = b.url; });
out.push(heard);
out.push(String(typeof ImageManager.isReady));
out.join('|')
"#;
        assert_eq!(
            host.eval_string(js),
            "img/pictures/x.png|0,0|true|false|false|true|no|img/pictures/x.png|function",
            "`isRequestOnly()` 必须是 false（就绪的句柄在上游那一支里就不算 request-only），\
             否则 `ImageCache.isReady()` 会把它整条跳过、`command282` 的等待腿变成「永远说就绪」"
        );
    }

    /// 换底之后第一次拿得到的三格账：**同一个图名回来的是同一个对象**、
    /// **hue 参与键**（同图不同 hue 是两条）、**空图名只有一条 `empty`**。
    ///
    /// 旧写法（自创的 `_imageCache` 空壳）三条都不成立：每次 `requestXxx` 都造新句柄。
    /// `requestImages` 沿 `117` 会把同一张图名报几十次（`M10.md` 的 d 那一轮量过 map 8 开场腿
    /// 一次报出 25 条），去重之后 JS 这一侧的条目数才与磁盘上的素材数是一个量。
    #[test]
    fn the_same_path_comes_back_as_the_same_object() {
        let host = booted_default();
        let js = r#"
var out = [];
var a = ImageManager.requestFace('Alcy');
var b = ImageManager.requestFace('Alcy');
out.push(String(a === b));
out.push(String(a === ImageManager._imageCache.get(a.url + ':0')));
out.push(String(ImageManager.requestFace('Alcy', 0) === ImageManager.requestFace('Alcy', 60)));
out.push(String(ImageManager.loadEmptyBitmap() === ImageManager.loadEmptyBitmap()));
out.push(String(Object.keys(ImageManager._imageCache._items).length));
out.join('|')
"#;
        assert_eq!(
            host.eval_string(js),
            "true|true|false|true|3",
            "键是 `path + ':' + hue`（`:799-801`）⇒ 60 度那一支是**另一条**；最后的 3 = \
             `Alcy:0` + `Alcy:60` + `empty` 三条 —— 换底之前这条必然红（旧写法每次造新句柄、\
             也没有 `empty` 那一条的账）"
        );
    }

    /// `_requestQueue` 今天**没人泵**：上游的泵是 `SceneManager.updateManagers`
    /// （`rpg_managers.js:1994-1996`），那一格属 M10.5 的场景主循环。
    /// 所以兼容层里报进队列的条目会攒着 —— 与 `_imageCache` 是同一批对象，不额外拿字节，
    /// 但这一格不许静默：判据把"不泵就不排空、`ImageManager.update()` 一拍就排空"钉住。
    #[test]
    fn the_request_queue_drains_only_when_the_scene_loop_pumps() {
        let host = booted_default();
        let js = r#"
var out = [];
ImageManager.clearRequest();
ImageManager.requestFace('q1');
ImageManager.requestFace('q2');
out.push(String(ImageManager._requestQueue._queue.length));
ImageManager.update();
out.push(String(ImageManager._requestQueue._queue.length));
ImageManager.requestFace('q3');
out.push(String(ImageManager._requestQueue._queue.length));
ImageManager.clearRequest();
out.push(String(ImageManager._requestQueue._queue.length));
out.join('|')
"#;
        assert_eq!(
            host.eval_string(js),
            "2|1|2|0",
            "`ImageManager.update()` **一拍只推进队首那一格**（上游 `RequestQueue.update` 只看 `_queue[0]`）⇒ \
             两条要泵两拍才排空。样品句柄 `isRequestReady()` 恒真，所以每一泵都是「放掉队首、让新的队首起跑」；\
             真正的在飞顺序由 core 那一格的 `the_request_queue_keeps_one_in_flight_and_raises_priority` 钉"
        );
    }

    /// **重灌 managers 会把图片缓存换成一颗新的**（上游 `ImageManager.clear()` 的同一条腿）。
    ///
    /// 这一格是"换底"带来的新语义，不许悄悄长出来：旧写法的 `_imageCache` 是手抄的空壳，
    /// 重灌只是重写一个对象；现在是 `new ImageCache()` ⇒ 旧条目全部脱落。兼容层里这不丢东西
    /// （素材字节在宿主 VFS/清单那一侧，句柄只是名字与账），但**读数必须说话**。
    #[test]
    fn reinstalling_managers_resets_the_image_cache() {
        let host = booted_default();
        host.eval(
            "globalThis.__before = ImageManager._imageCache; ImageManager.requestFace('keep');",
        )
        .expect("样品放不下");
        host.eval(IMAGE_MANAGER_CACHE_JS)
            .expect("缓存底第二次装不上（不许只许装一次）");
        assert_eq!(
            host.eval_string("String(ImageManager._imageCache === __before)"),
            "false",
            "重灌换的是**整颗** `ImageCache`（同 `:795` 那一句的形状），不是往里追加"
        );
        assert_eq!(
            host.eval_string("String(Object.keys(ImageManager._imageCache._items).length)"),
            "0",
            "重灌之后旧条目脱落 —— 这一格与 `core.rs` 的 \
             `reinstalling_the_cache_classes_keeps_the_entries` 是同一条腿的两半：\
             **重定义类**不动实例，**重跑静态底**换实例"
        );
        assert_eq!(
            host.eval_string(
                "String(ImageManager._imageCache.get('img/faces/keep.png:0') === null)"
            ),
            "true"
        );
    }

    /// `managers::install` **不要求调用方先装 core**：缓存四颗由本族自己带上（幂等）。
    ///
    /// 这一格钉的是装载次序的归属：`IMAGE_MANAGER_CACHE_JS` 里有三句 `new`，
    /// 把"先装 core"变成调用方的义务，就等于把一格的内部前提变成整族的口径 ——
    /// 而 d 那一轮的教训（`M10.md` 事实 41）是同一条腿的另一半：漏一个名字只会得到一句
    /// `X is not defined`，看不出是谁的责任。
    #[test]
    fn managers_install_does_not_depend_on_the_caller_installing_core_first() {
        let host = Host::boot();
        let mut shapes: Vec<DeclaredShape> = STATICS
            .iter()
            .map(|n| DeclaredShape {
                name: (*n).to_string(),
                kind: ShapeKind::StaticClass,
            })
            .collect();
        shapes.extend(CLASSES.iter().map(|n| DeclaredShape {
            name: (*n).to_string(),
            kind: ShapeKind::Class,
        }));
        shapes::install(&host, &shapes).expect("形状装不上");
        shapes::install_extensions(&host).expect("内建扩展装不上");
        host.eval(crate::platform::HOST_GLOBALS_JS)
            .expect("宿主全局面装不上");
        host.eval(crate::platform::UTILS_JS).expect("Utils 装不上");
        // 故意**不**调 `crate::core::install`。
        install(&host, Rc::new(fixture_data())).expect("只装 managers 应当自带缓存四颗");
        assert_eq!(
            host.eval_string(
                "[typeof ImageCache, typeof CacheMap, typeof RequestQueue, typeof CacheEntry].join(',')"
            ),
            "function,function,function,function"
        );
        assert_eq!(
            host.eval_string("String(ImageManager.reserveTileset('World_A1', 0, 3).isReady())"),
            "true",
            "静态底里的 `new ImageCache()` 已经在场，`reserve` 那一族当场就要用它"
        );
    }

    /// `SceneManager.isSceneChanging`：没有场景栈时读 **false**，而 `_nextScene`/`_exiting`
    /// 任意一格落下去就变 true。
    ///
    /// 这一格是 d 的热路径：`Game_Interpreter.update` 的 `while` 每一轮读它，true 就 break ——
    /// 移植侧那条"换图的 24 拍里旧页一条指令都不再执行"（`AGENTS.md` 咬人第 34 条⑷）在上游
    /// 就是这一句。宿主还没有场景栈（M10.5）时两格都是 `undefined` ⇒ 恒 false ⇒ 解释器照常跑，
    /// 而那正是"停在同一场景里跑事件"的样子。
    #[test]
    fn the_scene_switch_read_is_false_without_a_scene_stack() {
        let host = booted_default();
        assert!(!host.eval_bool("SceneManager.isSceneChanging()"));
        host.eval("globalThis.__scene = { name: 'Scene_Map' };")
            .expect("样品场景摆不下去");
        host.eval("SceneManager._nextScene = __scene;")
            .expect(" `_nextScene` 写不进");
        assert!(
            host.eval_bool("SceneManager.isSceneChanging()"),
            "`_nextScene` 已经落下去，这一句还是 false ⇒ 上游那一句 break 不来了"
        );
        host.eval("SceneManager._nextScene = null; SceneManager._exiting = true;")
            .expect("换法不对");
        assert!(host.eval_bool("SceneManager.isSceneChanging()"));
        host.eval("SceneManager._exiting = false;")
            .expect("收尾写不回去");
        assert!(!host.eval_bool("SceneManager.isSceneChanging()"));
    }

    #[test]
    fn reinstalling_the_scene_switch_is_still_the_core_layer() {
        let host = booted_default();
        host.eval(SCENE_MANAGER_SWITCH_JS)
            .expect("热路径那一格第二次装不上");
        assert!(!host.eval_bool("SceneManager.isSceneChanging()"));
    }
    /// 两圈的**格数**与 `_path`/`_replayFadeTime` 两格数据形状：`Object.keys` 数出来的
    /// 53 / 27 与 `core-touch.py --demand audio` 那两行读数一字不差（尺子问的是上游文本，
    /// 这一格问的是运行期真挂上去的键 —— 两边同数才算"整圈交满，没漏形状"）。
    ///
    /// `_standAlone` 在这里是 **false**：`managers::install` 先装 `RESOURCE_HANDLER_LOADER_JS`
    /// 再装音频那一圈，而上游整文件里 `WebAudio._standAlone = (function(top){ return
    /// !top.ResourceHandler; })(this)`（`rpg_core.js:7674`）靠函数声明提升拿到 `ResourceHandler`
    /// ⇒ 次序反了就会读到 true，`WebAudio.prototype.initialize` 就走另一条 `_loader` 分支。
    #[test]
    fn the_audio_circles_are_fifty_three_and_twenty_seven_cells() {
        let host = booted_default();
        let js = "[String(WebAudio._standAlone), String(typeof AudioManager.playBgm), \
                  String(typeof SoundManager.playCursor), AudioManager._path, \
                  String(AudioManager._replayFadeTime), String(Object.keys(AudioManager).length), \
                  String(Object.keys(SoundManager).length)].join('|')";
        assert_eq!(
            host.eval_string(js),
            "false|function|function|audio/|0.5|53|27",
            "53 + 27 是与尺子对表的读数；`_standAlone` 必须是 false（装载次序的归属写进代码， \
             见 `M10.md` 的 M10.4 第三格）"
        );
    }

    /// 播放一条 BGM 之后，**上游写的那几格状态全写上**（`_currentBgm` 的 name/volume/pos、
    /// `_bgmBuffer` 是一颗真的 `WebAudio`、URL 拼写走 `audioFileExt` 的那一支 `.m4a`），
    /// 而 `isPlaying()` 恒 false —— 少的是出声，不是状态。`saveBgm()` 折回来的那五格
    /// 也照上游（`pos` 在没有 `currentTime` 时是 0）。
    #[test]
    fn a_played_bgm_lands_the_upstream_state_but_never_reports_playing() {
        let host = booted_default();
        let js = "AudioManager.playBgm({name: 'sea_of_trees', volume: 90, pitch: 100, pan: 0}, 3); \
                  var s = AudioManager.saveBgm(); \
                  [AudioManager._currentBgm.name, String(AudioManager._currentBgm.volume), \
                   String(AudioManager._currentBgm.pos), \
                   String(AudioManager._bgmBuffer instanceof WebAudio), \
                   AudioManager._bgmBuffer.url, String(AudioManager._bgmBuffer.isPlaying()), \
                   String(AudioManager._bgmBuffer.volume), s.name, String(s.volume), String(s.pos)] \
                  .join('|')";
        assert_eq!(
            host.eval_string(js),
            "sea_of_trees|90|3|true|audio/bgm/sea_of_trees.m4a|false|0.9|sea_of_trees|90|0",
            "`volume` 0.9 = `100 * 90 / 10000`（上游 `updateBufferParameters` 那一格）， \
             而 `isPlaying()` false 是 `isReady()` 被 `WebAudio._context` 挡住的后果，不是 catch"
        );
    }

    /// 音量访问器是**三处一起落**：`AudioManager._masterVolume` → `WebAudio.setMasterVolume`
    /// → `Graphics.setVideoVolume`。后两格都是这一轮交进来的上游文本（后者是跨类点名的那一格），
    /// 所以 setter 全程不抛 —— 这与 M10.4 第一格那条 `Graphics.width`（先落字段再抛在没交的
    /// `_updateAllElements`）**不是同一种形状**，钉的就是"这一族的三格都到得了"。
    #[test]
    fn the_master_volume_setter_reaches_all_three_fields() {
        let host = booted_default();
        let js = "AudioManager.masterVolume = 0.5; \
                  [String(AudioManager._masterVolume), String(WebAudio._masterVolume), \
                   String(Graphics._videoVolume)].join('|')";
        assert_eq!(host.eval_string(js), "0.5|0.5|0.5");
    }

    /// 停乐那一族与错误面：`playSe` 之后 `_seBuffers` 有一条，`stopAll()` 把三颗缓冲、
    /// `_currentBgm`/`_currentBgs` 与 `_seBuffers` 全清回去，`checkErrors()` 在这一圈里
    /// 不抛（`isError()` 恒 false）。旧路线那两格"空曲名 = 停乐"（AGENTS 咬人第 8 条⑵）
    /// 在兼容层是**上游自己的路**：`AudioManager.playBgm({name:''})` 不进 `createBuffer`，
    /// 直接 `stopBgm()`。
    #[test]
    fn stop_all_and_check_errors_leave_the_same_ledger_upstream_would() {
        let host = booted_default();
        let js = "AudioManager.playBgm({name: 'x', volume: 0, pitch: 0, pan: 0}); \
                  AudioManager.playSe({name: 'Cursor', volume: 90, pitch: 100, pan: 0}); \
                  var before = AudioManager._seBuffers.length; \
                  AudioManager.stopAll(); \
                  var out = [String(before), String(AudioManager._currentBgm), \
                             String(AudioManager._bgmBuffer), String(AudioManager._seBuffers.length)]; \
                  out.push(String(typeof AudioManager.checkErrors)); AudioManager.checkErrors(); \
                  AudioManager.playBgm({name: '', volume: 0, pitch: 0, pan: 0}); \
                  out.push(String(AudioManager._bgmBuffer)); \
                  out.join('|')";
        assert_eq!(host.eval_string(js), "1|null|null|0|function|null");
    }

    /// `SoundManager` 那一族不直接碰宿主：`playSystemSound(1)` 走的是
    /// `AudioManager.playStaticSe($dataSystem.sounds[1])` ⇒ 条目落在 **`_staticBuffers`**
    /// （带 `_reservedSeName`），`_seBuffers` 恒 0；`bgmVolume` 的 setter 经
    /// `updateBgmParameters` 把当拍那颗缓冲的 `volume` 重算成 `55 * 40 / 10000`。
    #[test]
    fn sound_manager_routes_through_the_static_se_buffers() {
        let host = booted_default();
        // `globalThis.$dataSystem` 在这一格里是**样品**，而 wasm 的测试腿一个进程共享一个 realm
        // （`M10.md`「这一族的规矩」那条"要能装第二次"）⇒ 读完必须还原，否则下一条例子读到的是
        // 这一颗五格哈希，红的就不是本判据要问的那件事。
        let js = "var __prev = globalThis.$dataSystem; \
                  globalThis.$dataSystem = {sounds: [null, {name: 'Cursor', volume: 90, pitch: 100, pan: 0}]}; \
                  SoundManager.playSystemSound(1); \
                  AudioManager.playBgm({name: 'x', volume: 40, pitch: 120, pan: 0}); \
                  AudioManager.bgmVolume = 55; \
                  [String(AudioManager._seBuffers.length), \
                   String(AudioManager._staticBuffers.length), \
                   AudioManager._staticBuffers[0].url, \
                   AudioManager._staticBuffers[0]._reservedSeName, \
                   String(AudioManager._bgmBuffer.volume)].join('|') \
                  + '|' + (function () { globalThis.$dataSystem = __prev; return 'restored'; })()";
        assert_eq!(
            host.eval_string(js),
            "0|1|audio/se/Cursor.m4a|Cursor|0.22|restored",
            "`_staticBuffers[0].url` 里那条 `audio/se/…m4a` 是 `createBuffer` 上游那一格的原文 \
             （`_path + folder + '/' + encodeURIComponent(name) + ext`）"
        );
    }

    /// `Html5Audio` 整颗**不交**的理由是数字，不是"看着像不用"：`shouldUseHtml5Audio()` 的体
    /// 在上游 1.6.1 自己注释成恒 `return false`（`rpg_managers.js:1495`），于是
    /// `createBuffer('bgm', …)` 走的是 `new WebAudio(url)` 那一支，而 `Html5Audio` 只剩
    /// `shapes` 那颗壳（`typeof` 是 function、格全没交 ⇒ 读 `undefined`，但那条腿永远走不到）。
    #[test]
    fn the_html5_branch_is_dead_so_that_class_stays_a_shell() {
        let host = booted_default();
        let js = "[String(AudioManager.shouldUseHtml5Audio()), String(typeof Html5Audio), \
                  String(Html5Audio.setup), \
                  String(AudioManager.createBuffer('bgm', 'sea_of_trees') instanceof WebAudio)].join('|')";
        assert_eq!(host.eval_string(js), "false|function|undefined|true");
    }

    /// 装载次序由本族负（同缓存四颗那条纪律）：`managers::install` 自己先 `core::install_audio`，
    /// 所以只 shapes + managers 的宿主也已经有 `AudioManager` 的 53 格与 `WebAudio` 的那一圈；
    /// 而 `WebAudio._standAlone` 必须是 false（`RESOURCE_HANDLER_LOADER_JS` 在它之前）。
    #[test]
    fn managers_install_installs_the_audio_round_itself() {
        let host = booted_default();
        assert_eq!(
            host.eval_string("[String(typeof AudioManager.playBgm), String(typeof WebAudio.prototype._load), String(WebAudio._standAlone)].join('|')"),
            "function|function|false",
            "少一格就是 `Game_Interpreter` 的 `241`/`250` 那一族在兼容腿上 `not a function`"
        );
    }
}
