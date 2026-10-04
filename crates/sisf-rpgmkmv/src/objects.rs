//! M10.3 的 **`Game_*` 族前三条腿：状态类（a）+ 地图与角色（b）+ 玩法（c）**（`rpg_objects.js`）。
//!
//! 范围与体量是查出来的（`sisf_pray/tools/probes/game-touch.py`，腿的外部依赖面是
//! `sisf_pray/tools/probes/leg-deps.py`），机制与 [`crate::managers`] 同一套：交**上游的行为文本**，
//! 构造器由 [`crate::shapes`] 的占位那一格负责，本模块只往 `prototype` 上挂方法 ⇒ 重复装载幂等。
//!
//! a 那一族（184 条）是手抄的，守卫是 `sisf_pray/tools/probes/objects-verbatim.py`。
//! b 与 c（413 + 627 条 `prototype` 赋值）改成**切片**（同一把尺子复验）：
//! `python3 sisf_pray/tools/probes/objects-slice.py --classes … --out …` 直接从
//! `rpg_objects.js` 切，于是"抄错一个字"这一类错在源头就不存在，而那把逐字尺子的职责换成
//! **"切片有没有与上游脱钩"**（上游换了、或有人手动编辑了那一份 JS，它照样红）。
//!
//! 与上游的偏差与前提（每一条都是量出来的，不是"顺手改良"）：
//!
//! 1. **`$game*` 那十三颗名字**交的是 [`GAME_GLOBALS_JS`]（上游 `rpg_managers.js:29-41` 无条件
//!    写 null）。把实例建起来的那一格是 `DataManager.createGameObjects()`，**随 c 一起交**
//!    （[`crate::managers::CREATE_GAME_OBJECTS_JS`]）：c 之前那一颗 `new` 的
//!    `Game_Actors`/`Game_Party`/`Game_Troop` 只有占位构造器，`initialize` 没交 ⇒ `new` 当场抛。
//!    本模块的判据仍然**自己 `new`** 需要的那颗实例（这样每条判据拿到的是干净的一套，
//!    也免得装载顺序的耦合进判据里）。
//! 2. **`Game_{Switches,Variables,SelfSwitches}.onChange` 落 `$gameMap.requestRefresh()`**：
//!    方法在 [`GAME_MAP_AND_EVENTS_JS`] 里，实例在 `createGameObjects` 里 ⇒ c 之后**两处都齐**
//!    （[`crate::managers`] 那一条判据钉"三张表各写一格，旗立三次"）。
//!    判据一边钉"有实例就真敲"，一边钉"没有实例就抛、不许悄悄不敲"
//!    （[`the_write_holes_before_the_map_is_built_and_says_why`]）。
//! 3. **`Game_Timer.onExpire` 落 `BattleManager.abort()`**：成员未交（M10.5 的 `scenes` 那一族）。
//! 4. **`Game_Screen.realPictureId` 读 `$gameParty.inBattle()`**：c 之后**够得着**了
//!    （[`GAME_ACTOR_AND_PARTY_JS`] 交出 `Game_Party.inBattle`），前提是 `$gameParty` 是实例
//!    （= 偏差 1 那一格）。
//! 5. **`Utils.isOptionValid` 交回 `1`/`0` 而不是布尔**（`rpg_core.js:193-197` 原文）⇒
//!    `isBattleTest()` 是 `0`，插件里 `=== false` 那一写法在上游恒假。照上游，不改。
//! 6. **b 交出的不只是方法**：
//!    - **五条 `Object.create` 原型链**连着各自的 `prototype.constructor` 补句
//!      （`Character←CharacterBase`、`Event/Player/Follower/Vehicle←Character`）。
//!      上游十条链里 b 占五条、c 占五条 —— 名字齐（普查那 3,107 条）与够得着（这一族）是两格。
//!    - **`Game_Character.ROUTE_*` 静态常量 46 条**随 b 一起交（切片把行首 `X.NAME = <非函数>`
//!      归成"静态常量"这一类，不吞）。旧移植侧 `sisf_pray/src/route.rs` 手写的步码从此有上游那一份；
//!      少交它的表征不是抛，是 `route.list[i].code` 比 `undefined` 恒 false ⇒ 路线**一步都不走**。
//!    - **四条跨类样品**：`Game_Interpreter` 的 `initialize`/`clear`/`checkOverflow`/`isRunning`。
//!      `Game_Map.initialize` 当场 `new Game_Interpreter()`，而 `isEventRunning()` 读 `isRunning()`；
//!      后两条不是可选的 —— 上游 `initialize` 自己就调 `checkOverflow` + `clear`
//!      （`rpg_objects.js:8771-8780`，那两句调用在 `:8773-8774`），只交前两条时 `new Game_Map()` 抛的是 "not a function"。
//!      少交这四条，`Game_Map` 就构造不出来（与 a 那一轮把 `Game_Picture` 一起交是同一条纪律）。
//!      d 的其余 **147** 条（`commandXXX` 那一族，上游全类 151 条）未到。
//! 7. **c 交出的同样不只是方法**（同一格纪律，量出来的数是：10 条链全部齐了）：
//!    - **四条链** `Actor/Enemy←Battler←BattlerBase`、`Party/Troop←Unit`（`Battler←BattlerBase`
//!      那一条在 [`GAME_BATTLE_JS`] 里，四条在 [`GAME_ACTOR_AND_PARTY_JS`] 里 ⇒ 两段的装载顺序是硬的）；
//!    - **静态常量 55 条**：`Game_Action` 的 `EFFECT_*`/`HITTYPE_*` 17 条 + `Game_BattlerBase` 的
//!      `TRAIT_*`/`FLAG_ID_*`/`ICON_*` 30 条 + `Game_Party.ABILITY_*` 6 条 + `Game_Troop.LETTER_TABLE_*` 2 条
//!      （后两条是**值跨行的数组**，切片器为它加了一条口径，见 `objects-slice.py` 的模块头）；
//!    - **两块属性访问器**：`Game_BattlerBase.prototype` 的 `hp`/`mp`/`tp`/`mhp`… 那一块（复数写法）
//!      与 `Game_Actor.prototype` 的 `level`（**单数 `Object.defineProperty`**，全仓只有这一条）。
//!      它们不是方法 ⇒ "名字齐"那把普查与逐字尺子都数不到，而插件与上游代码全按属性读
//!      （`actor.level`、`battler.hp`）⇒ 少一格的表征是 `undefined`，不是抛（同偏差 6 那条腿）；
//!    - **c 的外部面只有四个名字**（`leg-deps.py` 量的：16 处 `DataManager`、12 处 `SoundManager`、
//!      4 处 `TextManager`、3 处 `BattleManager`）：前三个里 `DataManager` 的
//!      `isSkill`/`isItem`/`isWeapon`/`isArmor` 四句与 `TextManager` 整族**早已随 M10.2 交**，
//!      剩下两格是**记了名的洞** —— `SoundManager.play*`（11 条成员、全在"结果落地/倒下的表演"
//!      那一条腿上，底下是 `AudioManager` ⇒ M10.4）与 `BattleManager` 的
//!      `isForcedTurn`/`canEscape`/`isTurnEnd`（战斗是定案的非目标）。判据一边把 c 的**不碰这两格**
//!      的路径跑通，一边钉"撞到那一格会抛、而且点名是谁"（[`the_battler_reaches_the_undelivered_managers_and_says_who`]）。

use crate::jsrt::Host;

/// `$game*` 那十三颗名字（上游 `rpg_managers.js:29-41`）。
///
/// **是无条件写 null，不是"没有才声明"**（与 [`GRAPHICS_FRAME_COUNT_JS`] 恰好相反，两边都有理由）：
/// 上游那十三句本来就没守卫，而"重新装载一次就把活着的实例抹回 null"正是上游那份文件被
/// 重新 `<script>` 一遍时的行为 —— 建回它们的那一格是 `DataManager.createGameObjects()`。
/// 顺带这一手让 node 那条测试腿（共享一个 realm）的判据彼此**不串味**：每条判据开头调一次
/// [`booted`] 就拿到一套干净的 null。
pub const GAME_GLOBALS_JS: &str = r#"
globalThis.$gameTemp         = null;
globalThis.$gameSystem       = null;
globalThis.$gameScreen       = null;
globalThis.$gameTimer        = null;
globalThis.$gameMessage      = null;
globalThis.$gameSwitches     = null;
globalThis.$gameVariables    = null;
globalThis.$gameSelfSwitches = null;
globalThis.$gameActors       = null;
globalThis.$gameParty        = null;
globalThis.$gameTroop        = null;
globalThis.$gameMap          = null;
globalThis.$gamePlayer       = null;
"#;

/// `Graphics.frameCount`（`rpg_core.js:1798` 的初值）—— `Game_System.playtime` 的唯一输入。
pub const GRAPHICS_FRAME_COUNT_JS: &str = r#"
if (typeof Graphics.frameCount !== 'number') {
    Graphics.frameCount = 0;
}
"#;

/// `Game_Temp`（`rpg_objects.js:10-61`）+ `Game_System`（`:68-276`）。
pub const GAME_TEMP_AND_SYSTEM_JS: &str = include_str!("js/objects_temp_and_system.js");

/// `Game_Timer`（`:283-320`）+ `Game_Message`（`:328-508`）。
pub const GAME_TIMER_AND_MESSAGE_JS: &str = include_str!("js/objects_timer_and_message.js");

/// `Game_Switches`/`Game_Variables`/`Game_SelfSwitches`（`:515-540`、`:547-575`、`:582-609`）。
pub const GAME_STATE_TABLES_JS: &str = include_str!("js/objects_state_tables.js");

/// `Game_Screen`（`:617-926`）。
pub const GAME_SCREEN_JS: &str = include_str!("js/objects_screen.js");

/// `Game_Picture`（`:933-1097`）。
pub const GAME_PICTURE_JS: &str = include_str!("js/objects_picture.js");

/// b 的第一段：`Game_CharacterBase`（`:6244-6834`）+ `Game_Character`
/// （`:6834-7395`，连着 46 条 `ROUTE_*` 静态常量与它自己的链）+ `Game_Player`（`:7395-8016`）
/// + `Game_Follower`（`:8016-8073`）+ `Game_Followers`（`:8073-8194`）。
///
/// 225 条方法 + 3 条链 + 3 条 `constructor` 补句 + 46 条 `ROUTE_*` 常量 + `x`/`y` 那块访问器，
/// **切片自上游**（模块头第 6 条）。
pub const GAME_CHARACTERS_JS: &str = include_str!("js/objects_characters.js");

/// b 的第二段：`Game_Map`（`:5403-6197`）、`Game_CommonEvent`（`:6197-6244`）、
/// `Game_Vehicle`（`:8194-8429`）、`Game_Event`（`:8429-8767`），
/// 外加四条跨类样品（`Game_Interpreter` 的 `initialize`/`clear`/`checkOverflow`/`isRunning`）。
///
/// 187 条方法（含四条 `Game_Interpreter` 样品）+ 2 条链 + 2 条 `constructor` 补句。
///
/// **必须在 [`GAME_CHARACTERS_JS`] 之后装载**：`Game_Event`/`Game_Vehicle` 的链基类
/// `Game_Character` 在前一段里。
pub const GAME_MAP_AND_EVENTS_JS: &str = include_str!("js/objects_map_and_events.js");

/// c 的第一段：`Game_Item`（`:1105-1188`）、`Game_Action`（`:1188-1986`，带 17 条 `EFFECT_*`/`HITTYPE_*`
/// 常量）、`Game_ActionResult`（`:1986-2090`）、`Game_BattlerBase`（`:2090-2850`，带 30 条
/// `TRAIT_*`/`FLAG_ID_*`/`ICON_*` 常量与 `hp`/`mp`/`tp`/`mhp`… 那一块属性访问器）、
/// `Game_Battler`（`:2850-3392`，带它自己的链）。
///
/// **341 条方法** + 1 条链（`Battler←BattlerBase`）+ 1 条 `constructor` 补句 + 47 条静态常量 +
/// 1 块访问器，全部切片自上游（模块头第 7 条）。
pub const GAME_BATTLE_JS: &str = include_str!("js/objects_battle.js");

/// c 的第二段：`Game_Actor`（`:3392-4286`，带 `level` 那颗**单属性**访问器）、
/// `Game_Enemy`（`:4286-4576`）、`Game_Actors`（`:4576-4599`）、`Game_Unit`（`:4599-4747`）、
/// `Game_Party`（`:4747-5199`，带 6 条 `ABILITY_*` 常量）、`Game_Troop`（`:5199-5403`，带
/// `LETTER_TABLE_HALF`/`_FULL` 两条**跨行数组**常量）。
///
/// **281 条方法** + 4 条链（`Actor/Enemy←Battler`、`Party/Troop←Unit`）+ 4 条 `constructor` 补句
/// + 8 条静态常量 + 1 条单属性访问器。
///
/// **必须在 [`GAME_BATTLE_JS`] 之后装载**：`Game_Actor`/`Game_Enemy` 的链基类 `Game_Battler` 在前一段里。
pub const GAME_ACTOR_AND_PARTY_JS: &str = include_str!("js/objects_actor_and_party.js");

/// d 那一腿：`Game_Interpreter`（`:8767-10641`，**151 条方法**：约 100 条 `commandXXX` 加上
/// `setup`/`update`/`executeCommand`/分支与等待那一族），**外加一颗函数型静态成员**
/// `Game_Interpreter.requestImages`（`:10516`）。
///
/// 这一段的形状与 b/c 两处不同，都是量出来的（`tools/probes/objects-slice.py --classes
/// Game_Interpreter`：链 0 条、`constructor` 补句 0 条、静态常量 0 条、访问器 0 块）：
/// 它没有原型链（自己就是根），而它交出的**不是九颗实例而是那台解释器**。
/// `requestImages` 为什么必须跟着交：`setup`（`:8801-8806`）无条件调它，而它沿 `117` 递归进
/// 公共事件、把 `101/129/205/212/216/231/282/283` 的图名都喂给 `ImageManager.requestXxx`
/// ⇒ 静态成员留在"不交"那一档就等于**每一次 setup 都抛**（切片器为此新增一档，见该文件的口径清单）。
///
/// **必须在 [`GAME_MAP_AND_EVENTS_JS`] 与 c 那两段之后装载**：它的方法体读的是那三段的实例与方法
/// （`$gameMap`/`$gamePlayer`/`$gameParty`/`$gameMessage`/`$dataCommonEvents`…）。
pub const GAME_INTERPRETER_JS: &str = include_str!("js/objects_interpreter.js");

/// 装载：名字 → 静态数 → a 的九个类 → b 的角色 → b 的地图与事件 → c 的战斗基础 → c 的角色与队伍
/// → d 的解释器。必须在 shapes/extensions/platform（以及 managers 的数据那一半 + `createGameObjects`
/// + `SceneManager.isSceneChanging` + `ImageManager` 的 request 那一族）之后、插件之前。
///
/// **c 那两段的顺序是硬的**：`Game_Battler←Game_BattlerBase` 那条链在 [`GAME_BATTLE_JS`] 里，
/// 而 `Game_Actor`/`Game_Enemy` 的链基类 `Game_Battler` 就在这一段的**前一段**里 ——
/// `Object.create(Base.prototype)` 读的是基类**当时**那颗 prototype 对象。
/// **d 排在最后**：`Game_Interpreter.setup` 当场要 `ImageManager.requestXxx` 与
/// `Game_Interpreter.requestImages` 沿 `117` 递归时要 `$dataCommonEvents` 已经在场，
/// 而它的方法体读的全是前三段交好的实例。
pub fn install(host: &Host) -> Result<(), String> {
    for piece in [
        GAME_GLOBALS_JS,
        GRAPHICS_FRAME_COUNT_JS,
        GAME_TEMP_AND_SYSTEM_JS,
        GAME_TIMER_AND_MESSAGE_JS,
        GAME_STATE_TABLES_JS,
        GAME_SCREEN_JS,
        GAME_PICTURE_JS,
        GAME_CHARACTERS_JS,
        GAME_MAP_AND_EVENTS_JS,
        GAME_BATTLE_JS,
        GAME_ACTOR_AND_PARTY_JS,
        GAME_INTERPRETER_JS,
    ] {
        host.eval(piece)
            .map_err(|e| format!("`Game_*` 装不上：{e}"))?;
    }
    Ok(())
}

#[cfg(all(test, feature = "js"))]
mod tests {
    //! 机制判据：只用 fixture 的数据（这颗 crate 不许知道某一款游戏在哪）。
    //! 真数据与真插件那两条在 `sisf_pray/src/compat.rs`。
    use super::*;
    use crate::shapes::{self, DeclaredShape, ShapeKind};

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    /// a 那一族的九个类（`game-touch.py` 的名单：160 + `Game_Picture` 24 = **184** 条方法）。
    const CLASSES: &[&str] = &[
        "Game_Temp",
        "Game_System",
        "Game_Timer",
        "Game_Message",
        "Game_Switches",
        "Game_Variables",
        "Game_SelfSwitches",
        "Game_Screen",
        "Game_Picture",
    ];

    /// b 那一族的十个类（`game-touch.py`：381 + `Game_Vehicle` 32 = **413** 条 prototype 赋值，
    /// 其中 5 条是链的 `constructor` 补句 ⇒ **408** 条方法；再四条跨类样品 = **412** 条）。
    ///
    /// `Game_Interpreter` 在这里只为了那四条样品（`initialize`/`clear`/`checkOverflow`/`isRunning`）——
    /// 它的形状必须存在，否则 `new Game_Map()` 在 `Game_Map.initialize` 那一格就抛。
    const CLASSES_B: &[&str] = &[
        "Game_CharacterBase",
        "Game_Character",
        "Game_Player",
        "Game_Follower",
        "Game_Followers",
        "Game_Map",
        "Game_CommonEvent",
        "Game_Vehicle",
        "Game_Event",
        "Game_Interpreter",
    ];

    /// c 那一族的十一个类（`game-touch.py`：627 条 prototype 赋值，其中 5 条是链的 `constructor`
    /// 补句 ⇒ **622** 条方法）。`leg-deps.py` 量的外部面只有四个名字：`DataManager` 16 处
    /// （四句 `isSkill`/`isItem`/`isWeapon`/`isArmor` 早已随 M10.2 交）、`SoundManager` 12 处（M10.4）、
    /// `TextManager` 4 处（随 M10.2 交）、`BattleManager` 3 处（战斗是定案的非目标）。
    const CLASSES_C: &[&str] = &[
        "Game_Item",
        "Game_Action",
        "Game_ActionResult",
        "Game_BattlerBase",
        "Game_Battler",
        "Game_Actor",
        "Game_Enemy",
        "Game_Actors",
        "Game_Unit",
        "Game_Party",
        "Game_Troop",
    ];

    /// d 那一腿只有一颗类，而那颗类就是解释器本身（`game-touch.py`：**151 条**，其中约 100 条
    /// `commandXXX`；没有链、没有 `constructor` 补句）。b 那一轮为 `new Game_Map()` 交的四条样品
    /// （`initialize`/`clear`/`checkOverflow`/`isRunning`）在这一腿的文本里**是同一段上游文本**
    /// ⇒ 交出的名字总数加 147 而不是 151（`tools/probes/objects-verbatim.py` 的口径按名字去重）。
    ///
    /// `leg-deps.py --leg d` 量的外部面：八个接收者（`ImageManager` 20 / `SceneManager` 10 /
    /// `AudioManager` 7 / `BattleManager` 5 / `Graphics` 5 / `Utils` 2 / `Input` 1 /
    /// `Window_MenuCommand` 1）+ **八颗裸引用的类名**（`Scene_Battle`/`Scene_Gameover`/`Scene_Menu`/
    /// `Scene_Name`/`Scene_Save`/`Scene_Shop`/`Scene_Title`/`Window_MenuCommand`，**没有一颗是 `new`**）。
    /// 裸引用落在 `SceneManager.goto/push` 的参数上 ⇒ 只查 `NAME.` 的那把尺子看不见它们，
    /// 而它们靠 shapes 那 153 条声明里的 21 颗 `Scene_*` 与 46 颗 `Window_*` 就够得着。
    const CLASSES_D: &[&str] = &["Game_Interpreter"];

    /// M10.4 第二格起，`ImageManager` 的缓存底要这四颗类的**形状**（`ImageCache`/`RequestQueue`
    /// 沿用壳里那句 `this.initialize.apply(this, arguments);`，`CacheEntry`/`CacheMap` 的构造体
    /// 由 [`crate::core::CACHE_JS`] 覆盖）。真装载链里它们本来就在 `shapes::install` 扫出的那 153 颗内。
    const CACHE_CLASSES: &[&str] = &["CacheEntry", "CacheMap", "ImageCache", "RequestQueue"];

    /// a 那一族的体里读到的静态类（`Graphics.frameCount`、`Utils.isOptionValid`，
    /// 以及两处**故意不交**的落点 `AudioManager`/`BattleManager` —— 只有形状在，
    /// 调用才抛得出一句点名是谁的话）。
    const STATICS: &[&str] = &[
        "Utils",
        "Graphics",
        "AudioManager",
        "BattleManager",
        "DataManager",
        // c 的 `Game_Actor` 升级/装备那两族读 `TextManager` 的名字（`levelUp`、`equip`…），
        // 而 `TEXT_MANAGER_JS` 的顶层就往它身上 define 那 64 格 ⇒ 形状必须先在场。
        "TextManager",
        // b 那一腿当场要它（`setImage` 问 `isObjectCharacter`、`setupParallax` 问
        // `isZeroParallax`）；交的只有名字那一半，见 [`crate::managers::IMAGE_MANAGER_NAMES_JS`]。
        "ImageManager",
        // c 的 12 处 `SoundManager.play*`（`leg-deps.py`）落在这颗静态类上 —— 形状在、
        // 成员没交（底下是 `AudioManager`，M10.4），于是那一格抛得出一句点名是谁的话。
        "SoundManager",
        // d 的热路径读 `SceneManager.isSceneChanging()`（`Game_Interpreter.update` 的每一轮），
        // 而那一格就挂在这颗静态类上（`leg-deps.py --leg d` 量的：`SceneManager` 10 处 / 8 个方法，
        // 其余七处是 `goto`/`push`/`prepareNextScene` ⇒ 记 M10.5 的洞）。
        "SceneManager",
    ];

    /// 装到 a/b/c 三档的一颗宿主（`install()` 交全部三段行为文本）。
    ///
    /// `$dataSystem.switches` 是 **1000** 格、`variables` 是 **806** 格 —— 不是随手写的：
    /// 那是本工程实测的上游读数（`../sisf_pray/M10.md`「会咬人的地方（M10 这一族）」第 1 条⑷，`$dataSystem` 的两张名字表），
    /// 而"越界写落空"那一族判据的边界全靠这两个数。fixture 用同样长的数组，
    /// 于是 `setValue(1000)`/`setValue(806)` 在 fixture 里也在界外。
    fn booted() -> Host {
        let shapes: Vec<DeclaredShape> = CLASSES
            .iter()
            .chain(CLASSES_B.iter())
            .chain(CLASSES_C.iter())
            .chain(CLASSES_D.iter())
            .chain(CACHE_CLASSES.iter())
            .map(|n| DeclaredShape {
                name: (*n).to_string(),
                kind: ShapeKind::Class,
            })
            .chain(STATICS.iter().map(|n| DeclaredShape {
                name: (*n).to_string(),
                kind: ShapeKind::StaticClass,
            }))
            .collect();
        let host = Host::boot();
        shapes::install(&host, &shapes).expect("形状装不上");
        shapes::install_extensions(&host).expect("JsExtensions 装不上");
        // `Utils.isOptionValid` 读 `location.search`（`rpg_core.js:193-197`），而那颗 `location`
        // 是 `platform::HOST_GLOBALS_JS` 交的 —— 少了它，`new Game_Temp()` 在第一条 `initialize`
        // 就抛 "location is not defined"（本轮实测）。
        host.eval(crate::platform::HOST_GLOBALS_JS)
            .expect("宿主全局面装不上");
        host.eval(crate::platform::UTILS_JS)
            .expect("Utils 装不上（Game_Temp.initialize 读 isOptionValid）");
        // `ImageManager` 的名字那一半住在 managers 那一段（`contains` 那条内建扩展是它的前提）。
        host.eval(crate::managers::IMAGE_MANAGER_NAMES_JS)
            .expect("ImageManager 的名字那一半装不上");
        // d 的两格前提（`leg-deps.py --leg d` 量出来的边界，不是"顺手多交一点"）：
        // `Game_Interpreter.update` 的每一轮读 `SceneManager.isSceneChanging()`，
        // 而 `setup` 无条件调 `Game_Interpreter.requestImages` → `ImageManager.requestXxx`。
        // 少这一格 = 解释器一步都走不了；少那一格 = 每一次 setup 都抛。
        host.eval(crate::managers::SCENE_MANAGER_SWITCH_JS)
            .expect("SceneManager 的热路径那一格装不上");
        // M10.4 的第二格起，request 那一族**脚下踩的是真缓存**（`_imageCache` 是 `new ImageCache()`、
        // `_generateCacheKey` 与 `_requestQueue` 都在场）⇒ 这一族的三份前提一起装：
        // core 的缓存四颗 + `ImageManager` 的九格静态底 + 那 19 格薄壳。少任何一份，
        // `setup` 的第一次 `requestImages` 就抛（实测报的是 `not a function`，看不出是谁缺）。
        crate::core::install_cache(&host).expect("core 的缓存四颗装不上");
        host.eval(crate::managers::IMAGE_MANAGER_CACHE_JS)
            .expect("ImageManager 的缓存底装不上");
        host.eval(crate::managers::IMAGE_MANAGER_REQUEST_JS)
            .expect("ImageManager 的 request 那一族装不上");
        // c 的体当场要 managers 那三段（`leg-deps.py` 量的：`Game_Item.setObject` 问
        // `DataManager.isSkill/isItem/isWeapon/isArmor`，`Game_Actor` 的升级/装备那两族问
        // `TextManager`，而十三颗实例由 `createGameObjects` 建）。
        // **顺序**：`DATA_MANAGER_JS` 自己把 `$data*` 那十五个名字声明成 `null`（上游 `:14-28`），
        // 所以它必须在下面那份 fixture **之前**装，否则后写的 fixture 会被它抹掉。
        host.eval(crate::managers::DATA_MANAGER_JS)
            .expect("DataManager 装不上（c 的体里 16 处读它）");
        host.eval(crate::managers::TEXT_MANAGER_JS)
            .expect("TextManager 装不上（升级与装备那两族读它）");
        host.eval(crate::managers::CREATE_GAME_OBJECTS_JS)
            .expect("createGameObjects 装不上");
        host.eval(
            r#"
            globalThis.$dataSystem = {
                locale: 'ja_JP', optSideView: true, versionId: 7,
                switches: [], variables: [],
                windowTone: [0, 0, 0, 0], battleBgm: 'battle', victoryMe: 'win', defeatMe: 'lose',
                currencyUnit: 'G',
                // 载具的配置**不在 `$dataVehicles` 里**（那是编辑器里的名词）：上游读的是
                // `$dataSystem.boat/ship/airship` 三格（`Game_Vehicle.vehicle()` 那三句）。
                // 它们现在必须在 `booted` 这一档：1.6.1 的 `Game_Map.initialize` 自己就调
                // `createVehicles()` ⇒ `new Game_Map()`（= `createGameObjects` 的第 12 颗）当场要这三格。
                boat: {startMapId: 3, startX: 4, startY: 5, characterName: 'boat', characterIndex: 0},
                ship: {startMapId: 6, startX: 7, startY: 8, characterName: 'ship', characterIndex: 0},
                airship: {startMapId: 0, startX: 0, startY: 0, characterName: 'airship', characterIndex: 0}
            };
            for (var i = 0; i < 1000; i++) { $dataSystem.switches.push('S' + i); }
            for (var i = 0; i < 806; i++) { $dataSystem.variables.push('V' + i); }
            globalThis.$dataCommonEvents = [null, {id: 1, list: []}];
            globalThis.$dataMap = {bgm: {name: 'map', volume: 90, pitch: 100}};
            "#,
        )
        .expect("fixture 数据装不上");
        install(&host).expect("状态类装不上");
        battler_data(&host);
        host
    }

    /// c 的那颗宿主：三段行为文本 + 十张 `$data*` + `DataManager.createGameObjects()` 跑过一次
    /// ⇒ 十三颗 `$game*` 都是**实例**（b 那一轮的属性替身在这一档不再需要，模块头偏差 1 闭合）。
    fn battler_host() -> Host {
        let host = booted();
        host.eval("DataManager.createGameObjects();")
            .expect("十三颗实例建不出来（c 的那一条腿就断在这里）");
        host
    }

    /// c 的 fixture：`leg-deps.py` 量出的那十张 `$data*`。
    ///
    /// **字段名照上游数据的形状**（实测 `data/Actors.json` 的 `equips` 是 **id 数组**、
    /// `Classes.json` 的 `params` 是 8×100 个数、`Enemies.json` 的 `params` 是 8 个数、
    /// `Troops.json` 的 `members` 是 `{enemyId, x, y}`），值一律中性 —— 这颗 crate 不许知道
    /// 某一款游戏在哪（真数据那两条在 `sisf_pray/src/compat.rs`）。
    fn battler_data(host: &Host) {
        host.eval(
            r#"
            $dataSystem.equipTypes = [0, 'weapon', 'shield', 'head', 'body', 'accessory'];
            $dataSystem.weaponTypes = [0, 'sword'];
            $dataSystem.armorTypes = [0, 'light'];
            $dataSystem.magicSkills = [2];
            $dataSystem.attackMotions = [0, 1];
            $dataSystem.partyMembers = [1];
            $dataSystem.testBattlers = [];
            $dataSystem.optFollowers = true;
            $dataSystem.optSlipDeath = false;
            $dataSystem.optFloorDeath = false;
            $dataSystem.optExtraExp = false;
            $dataSystem.optTransparent = false;
            $dataSystem.terms = {
                basic: ['HP', 'MP', 'TP', 'MHP', 'MMP', 'ATK', 'DEF', 'MAT', 'MDA', 'AGI', 'LUK'],
                params: ['最大HP', '最大MP', '攻撃', '防御', '魔攻', '魔防', '機敏', '運'],
                messages: {attackDamage: 'damage', levelUp: ' reached level',
                           skillExp: ' learned the skill', equipOrBreak: ' equipped',
                           notEnoughRestraint: ' cannot', nowStealing: ' steals',
                           discovered: ' discovered'},
                commands: ['fight', 'item', 'skill', 'equip', 'save', 'gameEnd'],
                names: {}
            };
            var classParams = [];
            for (var p = 0; p < 8; p++) {
                var col = [];
                for (var lv = 0; lv < 100; lv++) { col.push(lv === 0 ? 1 : 40 + lv * 2); }
                classParams.push(col);
            }
            globalThis.$dataClasses = [null, {id: 1, name: 'C1', expParams: [30, 20, 38, 50],
                learnings: [{level: 1, skillId: 1}, {level: 99, skillId: 2}],
                traits: [[21, 0, 1]], params: classParams}];
            function actor(id, equips) {
                return {id: id, name: 'A' + id, nickname: 'N' + id, profile: 'P' + id,
                    classId: 1, initialLevel: 5, maxLevel: 99,
                    characterName: '', characterIndex: 0, faceName: '', faceIndex: 0,
                    battlerName: '', equips: equips, traits: [], skills: []};
            }
            // 角色 1 **不穿装备**：那 50 点 HP 才是纯 `paramBase`（职业表第 5 级）的读数，
            // 判据不必猜"装备那一族到底落没落进 slot"。角色 2 带着上游那串 `[1, 2, 0]`，
            // 它的存在是给"装备路径跑得通、且不抛"那一条用的。
            globalThis.$dataActors = [null, actor(1, []), actor(2, [1, 2, 0])];
            globalThis.$dataSkills = [null,
                {id: 1, name: 'S1', stypeId: 1, mpCost: 0, tpCost: 0, hitType: 1,
                 damage: {type: 3, elementId: 0, variance: 20, critical: false},
                 successRate: 100, repeats: 1, scope: 1, occasion: 2, speed: 0,
                 animationId: 0, message1: '', message2: '', tpGain: 0,
                 requiredWtypeId1: 0, requiredWtypeId2: 0, effects: [],
                 description: '', note: ''},
                {id: 2, name: 'S2', stypeId: 1, mpCost: 5, tpCost: 0, hitType: 1,
                 damage: {type: 0, elementId: 0, variance: 20, critical: false},
                 successRate: 100, repeats: 1, scope: 1, occasion: 2, speed: 0,
                 animationId: 0, message1: '', message2: '', tpGain: 0,
                 requiredWtypeId1: 0, requiredWtypeId2: 0,
                 effects: [{code: 21, dataId: 1, value: 1}], description: '', note: ''}];
            globalThis.$dataItems = [null,
                {id: 1, name: 'I1', itypeId: 1, price: 10, consumable: true, scope: 1,
                 occasion: 0, effects: [], description: '', note: ''}];
            globalThis.$dataWeapons = [null,
                {id: 1, name: 'W1', wtypeId: 1, etypeId: 1, price: 100,
                 params: [0, 0, 0, 0, 0, 0, 0, 0], traits: [[31, 1, 0]],
                 description: '', note: ''}];
            // **下标就是 id**（上游那十张表全是这种稀疏数组）：防具的 id 是 2，所以它必须坐在
            // `$dataArmors[2]` 上。第一版把它写在 `[1]`，于是 `itemContainer($dataArmors[2])`
            // 拿到的是 `undefined`，红成"三张容器对不上"—— 那是 fixture 错，不是实现错。
            globalThis.$dataArmors = [null, null,
                {id: 2, name: 'AR2', atypeId: 1, etypeId: 2, price: 50,
                 params: [1, 0, 0, 0, 0, 0, 0, 0], traits: [], description: '', note: ''}];
            globalThis.$dataStates = [null,
                {id: 1, name: 'ST1', iconIndex: 0, color: 0, priority: 50, motion: 0,
                 overlay: false, releaseByDamage: false, autoRemovalTiming: 0,
                 chanceByDamage: 100, minTurns: 1, maxTurns: 2, removeAtBattleEnd: false,
                 removeByDamage: false, removeByRestriction: false, removeByWalking: false,
                 stepsToRemove: 0, restriction: 0, traits: [], message1: '', message2: '',
                 message3: '', message4: '', note: ''},
                {id: 2, name: 'ST2', iconIndex: 0, color: 0, priority: 10, motion: 0,
                 overlay: false, releaseByDamage: true, autoRemovalTiming: 1,
                 chanceByDamage: 100, minTurns: 3, maxTurns: 5, removeAtBattleEnd: true,
                 removeByDamage: true, removeByRestriction: false, removeByWalking: false,
                 stepsToRemove: 0, restriction: 0, traits: [], message1: 'a',
                 message2: 'b', message3: 'c', message4: 'd', note: ''}];
            globalThis.$dataEnemies = [null,
                {id: 1, name: 'E1', battlerName: '', battlerHue: 0, exp: 120, gold: 60,
                 params: [300, 0, 20, 15, 10, 10, 12, 10], traits: [],
                 actions: [{skillId: 1, conditionType: 0, conditionParam1: 0,
                            conditionParam2: 0, rating: 5}],
                 dropItems: [{kind: 0, dataId: 0, denominator: 1}]}];
            globalThis.$dataTroops = [null,
                {id: 1, name: 'T1', members: [{enemyId: 1, x: 100, y: 200},
                                              {enemyId: 1, x: 300, y: 200}],
                 pages: [null, null]}];
            "#,
        )
        .expect("c 的 fixture 装不上");
    }

    /// 取一个 **JS 数**的读数。
    ///
    /// 为什么不直接用 `eval_string`：JS 的 `number` 在两条腿的宿主边界上都**不是**字符串
    /// （native 实测报 "Error converting from js 'float' into type 'string'"），而这一族的读数
    /// 全是带小数的补间值（`191.25` 那一类）⇒ 让 JS 自己 `String()` 一下再取，两条腿同一形状。
    /// 为什么不夹成整数：那些台阶的**小数部分就是判据**（夹掉就看不出步长走歪）。
    fn num(host: &Host, expr: &str) -> String {
        host.eval_string(&format!("String({expr})"))
    }

    /// 一颗记数的 `$gameMap` 替身 —— b 那一腿还没交，而 a 的三条 `onChange` 的**可见后果**
    /// 全在它身上（模块头第 2 条）。只交 `requestRefresh` 那一格，名字与上游同名。
    fn map_stand_in(host: &Host) {
        host.eval(
            r#"
            globalThis.__knocks = 0;
            globalThis.$gameMap = { requestRefresh: function () { __knocks++; } };
            "#,
        )
        .expect("替身装不上");
    }

    #[test]
    fn the_thirteen_game_globals_are_declared_before_anyone_uses_them() {
        let host = booted();
        // 名单 = `game-touch.py` 从 `DataManager.createGameObjects` 现读的那 13 条。
        let names = [
            "$gameTemp",
            "$gameSystem",
            "$gameScreen",
            "$gameTimer",
            "$gameMessage",
            "$gameSwitches",
            "$gameVariables",
            "$gameSelfSwitches",
            "$gameActors",
            "$gameParty",
            "$gameTroop",
            "$gameMap",
            "$gamePlayer",
        ];
        for name in names {
            // 是声明好的 `null`（与上游 `createGameObjects` 之前同一形状），不是 ReferenceError。
            assert!(
                host.try_eval_bool(&format!("typeof {name} === 'object' && {name} === null"))
                    .unwrap_or_else(|e| panic!("{name} 该是声明过的 null：{e}")),
                "{name} 不是 null"
            );
        }
        assert_eq!(
            names.len(),
            13,
            "上游交出的实例数变了 ⇒ 这把尺子要跟着 game-touch.py 重对名单"
        );
    }

    #[test]
    fn the_upstream_constructor_shape_survives_the_behavior_text() {
        // 本模块不许重述构造器：`new X()` 必须仍走 shapes 那一格的
        // `this.initialize.apply(this, arguments)` ⇒ `initialize` 真的跑过（字段在）。
        let host = booted();
        assert!(host.eval_bool("new Game_Switches()._data instanceof Array"));
        assert!(host.eval_bool("new Game_Variables()._data instanceof Array"));
        assert!(host.eval_bool("typeof new Game_SelfSwitches()._data === 'object'"));
        assert_eq!(host.eval_i32("new Game_Picture()._scaleX"), 100);
        assert!(host.eval_bool("new Game_Message().positionType() === 2"));
        // 构造器还是 shapes 交的那一颗（身份没被换掉 ⇒ 插件在装载期压的原型不悬空）。
        assert!(host.eval_bool("Game_Screen.prototype.constructor === Game_Screen"));
        assert!(host.eval_bool("typeof Game_System.prototype.initialize === 'function'"));
    }

    #[test]
    fn the_state_writes_knock_on_the_map_once_per_accepted_write() {
        let host = booted();
        map_stand_in(&host);
        host.eval(
            "$gameSwitches = new Game_Switches(); $gameVariables = new Game_Variables(); $gameSelfSwitches = new Game_SelfSwitches();",
        )
        .unwrap();
        host.eval("$gameSwitches.setValue(10, true)").unwrap();
        assert_eq!(host.eval_i32("__knocks"), 1, "开关的合法写该敲一次");
        host.eval("$gameVariables.setValue(3, 42)").unwrap();
        assert_eq!(host.eval_i32("__knocks"), 2, "变量的合法写该敲一次");
        host.eval("$gameSelfSwitches.setValue('108,28,B', true)")
            .unwrap();
        assert_eq!(host.eval_i32("__knocks"), 3, "自我开关的写该敲一次");
        // 越界那一族一格都不许敲（上游的闸 `id > 0 && id < length` 在 `onChange` **之前**）。
        for expr in [
            "$gameSwitches.setValue(0, true)",
            "$gameSwitches.setValue(1000, true)",
            "$gameVariables.setValue(0, 1)",
            "$gameVariables.setValue(806, 1)",
            "$gameVariables.setValue(-5, 1)",
        ] {
            host.eval(expr).unwrap();
        }
        assert_eq!(
            host.eval_i32("__knocks"),
            3,
            "越界的写一条都不该敲旗（敲了就是凭空立起 refresh）"
        );
        assert!(host.eval_bool("$gameSwitches.value(1000) === false"));
        assert!(host.eval_bool("$gameVariables.value(806) === 0"));
    }

    #[test]
    fn the_write_holes_before_the_map_is_built_and_says_why() {
        // 模块头第 2 条那条空洞：`$gameMap` 还是 null 时写变量必须**抛**，
        // 而不是悄悄不敲旗 —— 悄悄不敲 = 页永远不重刷（旧移植侧 `request_refresh` 那面旗的等价物）。
        let host = booted();
        let err = host
            .try_eval_bool(
                "$gameVariables = new Game_Variables(); $gameVariables.setValue(1, 1); true",
            )
            .expect_err("$gameMap 是 null 时这一句该抛");
        assert!(
            err.contains("requestRefresh"),
            "抛的句子要点名缺的是哪一格，实际：{err}"
        );
    }

    #[test]
    fn variable_values_are_floors_and_switch_values_are_forced_booleans() {
        let host = booted();
        map_stand_in(&host);
        host.eval("$gameVariables = new Game_Variables(); $gameSwitches = new Game_Switches();")
            .unwrap();
        // `Math.floor` 只对 number 做（上游 `typeof value === 'number'`），字符串原样落。
        host.eval("$gameVariables.setValue(1, 7.9)").unwrap();
        assert_eq!(host.eval_i32("$gameVariables.value(1)"), 7);
        host.eval("$gameVariables.setValue(2, -7.9)").unwrap();
        assert_eq!(
            host.eval_i32("$gameVariables.value(2)"),
            -8,
            "floor 是向下，不是截断"
        );
        host.eval("$gameVariables.setValue(3, '文本')").unwrap();
        assert_eq!(host.eval_string("$gameVariables.value(3)"), "文本");
        // 没写过的格子是 `|| 0`（上游），开关是 `!!` ⇒ 一律布尔。
        assert_eq!(host.eval_i32("$gameVariables.value(77)"), 0);
        assert!(host.eval_bool("$gameSwitches.value(77) === false"));
        host.eval("$gameSwitches.setValue(5, 1)").unwrap();
        assert!(
            host.eval_bool("$gameSwitches.value(5) === true"),
            "非布尔的值要折成布尔（上游 `!!`）"
        );
        // 读**没有**界的闸：越界读不抛、也不敲旗（敲与不敲之间差的是前面那几次写，
        // 所以这一格比的是"读数前后旗数不动"，不是绝对值）。
        assert!(host.eval_bool("$gameSwitches.value(999999) === false"));
        assert_eq!(host.eval_i32("$gameVariables.value(999999)"), 0);
        let knocks_after_writes = host.eval_i32("__knocks");
        for id in [0, 1, 77, 999999] {
            host.eval(&format!(
                "$gameVariables.value({id}); $gameSwitches.value({id});"
            ))
            .unwrap();
        }
        assert_eq!(
            host.eval_i32("__knocks"),
            knocks_after_writes,
            "读格子一格旗都不许敲"
        );
    }

    #[test]
    fn self_switch_false_deletes_the_key_not_a_false() {
        let host = booted();
        map_stand_in(&host);
        host.eval("$gameSelfSwitches = new Game_SelfSwitches();")
            .unwrap();
        // 键是 `"图,事件,字母"` 那串逗号（`AGENTS.md` 存档事实卡那一族的键形）。
        host.eval("$gameSelfSwitches.setValue('108,28,B', true)")
            .unwrap();
        assert!(host.eval_bool("$gameSelfSwitches.value('108,28,B')"));
        assert_eq!(
            host.eval_i32("Object.keys($gameSelfSwitches._data).length"),
            1
        );
        host.eval("$gameSelfSwitches.setValue('108,28,B', false)")
            .unwrap();
        // 上游是 `delete` ⇒ **缺键**，不是留一个 `false`（存档里"没用过的槽"因此是缺键）。
        assert_eq!(
            host.eval_i32("Object.keys($gameSelfSwitches._data).length"),
            0
        );
        assert!(host.eval_bool("$gameSelfSwitches.value('108,28,B') === false"));
        assert_eq!(
            host.eval_i32("__knocks"),
            2,
            "自我开关连 false 都敲（上游那一条没闸）"
        );
    }

    #[test]
    fn the_message_defaults_are_the_upstream_numbers_that_drive_the_window() {
        let host = booted();
        host.eval("globalThis.$gameMessage = new Game_Message();")
            .unwrap();
        // `positionType 2` / `background 0` / `choicePositionType 2` 是移植侧窗几何的输入
        // （旧侧 `dialog::Geom::of` 读的就是这两格，`AGENTS.md` 咬人第 29 条⑴）。
        assert_eq!(host.eval_i32("$gameMessage.positionType()"), 2);
        assert_eq!(host.eval_i32("$gameMessage.background()"), 0);
        assert_eq!(host.eval_i32("$gameMessage.choicePositionType()"), 2);
        assert_eq!(host.eval_i32("$gameMessage.choiceBackground()"), 0);
        assert_eq!(host.eval_i32("$gameMessage.scrollSpeed()"), 2);
        assert!(host.eval_bool("$gameMessage.isBusy() === false"));
        assert!(host.eval_bool("$gameMessage.faceName() === ''"));
        // `101` 的"空图名抹掉上一条"（咬人第 24 条）就在 `setFaceImage` 这一格。
        host.eval("$gameMessage.setFaceImage('A', 2); $gameMessage.setFaceImage('', 0);")
            .unwrap();
        assert!(host.eval_bool("$gameMessage.faceName() === ''"));
        assert_eq!(host.eval_i32("$gameMessage.faceIndex()"), 0);
        // 折页的记号是 `\f`（charCode 12），而且**接在最后一行的尾巴上** —— 旧侧 `paginate`
        // 的另一半真相（`:500-504`）。
        host.eval(
            "$gameMessage.add('第一行'); $gameMessage.add('第二行'); $gameMessage.newPage();",
        )
        .unwrap();
        assert!(
            host.eval_bool("$gameMessage._texts[1].charCodeAt(3) === 12"),
            "newPage 该在最后一行尾巴上接一个 \\f"
        );
        // `allText` 用 `\n` 串起来（`:506-508`）。
        assert!(host.eval_bool("$gameMessage.allText().indexOf(String.fromCharCode(10)) > 0"));
        // `isBusy` 的四条腿：`clear` 之后必须回假 —— 否则 `201` 那条
        // `!$gameMessage.isBusy()` 的换图闸（咬人第 33 条⑸量的 14 条）在这一腿是通的。
        host.eval("$gameMessage.clear()").unwrap();
        assert!(host.eval_bool("$gameMessage.isBusy() === false"));
        assert_eq!(host.eval_i32("$gameMessage._texts.length"), 0);
    }

    #[test]
    fn the_choice_callback_runs_once_and_is_then_dropped() {
        let host = booted();
        host.eval(
            r#"
            globalThis.$gameMessage = new Game_Message();
            globalThis.__got = -1;
            $gameMessage.setChoices(['甲','乙'], 0, -1);
            $gameMessage.setChoiceCallback(function (n) { __got = n; });
            "#,
        )
        .unwrap();
        assert_eq!(host.eval_i32("$gameMessage.choices().length"), 2);
        assert!(host.eval_bool("$gameMessage.isChoice()"));
        host.eval("$gameMessage.onChoice(1)").unwrap();
        assert_eq!(host.eval_i32("__got"), 1, "回调没跑到");
        assert!(
            host.eval_bool("$gameMessage._choiceCallback === null"),
            "跑完要抹掉（上游 `:472-477`）"
        );
        host.eval("__got = -1; $gameMessage.onChoice(0)").unwrap();
        assert_eq!(host.eval_i32("__got"), -1, "抹掉之后第二次不许再跑");
    }

    #[test]
    fn the_timer_counts_beats_and_holes_at_expire() {
        let host = booted();
        host.eval("globalThis.$gameTimer = new Game_Timer();")
            .unwrap();
        assert_eq!(host.eval_i32("$gameTimer.seconds()"), 0);
        assert!(host.eval_bool("$gameTimer.isWorking() === false"));
        // `sceneActive` 为假的那一拍一格都不走（上游的第一条闸）。
        host.eval("$gameTimer.start(4); $gameTimer.update(false);")
            .unwrap();
        assert_eq!(
            host.eval_i32("$gameTimer._frames"),
            4,
            "sceneActive 假的那拍不许走"
        );
        host.eval("$gameTimer.update(true)").unwrap();
        assert_eq!(host.eval_i32("$gameTimer._frames"), 3, "到点前每一拍减一格");
        assert_eq!(
            host.eval_i32("$gameTimer.seconds()"),
            0,
            "seconds 是 floor(frames/60)"
        );
        // 到点那一拍落到 `onExpire` ⇒ `BattleManager.abort`（模块头第 3 条的空洞）。
        host.eval("$gameTimer.update(true); $gameTimer.update(true);")
            .unwrap();
        assert_eq!(host.eval_i32("$gameTimer._frames"), 1, "还差一拍才到点");
        let err = host
            .try_eval_bool("$gameTimer.update(true)")
            .expect_err("第 4 拍到点，该抛在 BattleManager 那一格");
        // **两条腿的异常文本不一样**（实测）：QuickJS 只说 "not a function"，
        // node 会说 "BattleManager.abort is not a function" ⇒ 尺子钉得住的是**共同的另一半**：
        // 抛之前 `_frames` 已经减到 0（上游的顺序是 `frames--; if (frames === 0) onExpire()`），
        // 而 `_working` 还是真（`onExpire` 没跑完，旗没抹）—— 那一拍的状态落点才是判据。
        assert!(
            err.contains("not a function"),
            "两条腿都会在这一格点名'喊的不是函数'，实际：{err}"
        );
        assert_eq!(
            host.eval_i32("$gameTimer._frames"),
            0,
            "减帧在 onExpire 之前"
        );
        assert!(
            host.eval_bool("$gameTimer._working === true"),
            "`onExpire` 抛在半路 ⇒ 工作旗还在（上游 stop 是 `BattleManager` 之后由场景喊的）"
        );
        // `start(0)` 那一格：上游因为 `_frames > 0` 不成立而**不**触发 onExpire，
        // 但工作旗照样落下 —— 不许"顺手改良"成"0 就是没起"。
        host.eval("$gameTimer.start(0); $gameTimer.update(true);")
            .unwrap();
        assert!(
            host.eval_bool("$gameTimer.isWorking()"),
            "start 落的工作旗不许被 0 抹掉（`:301-304`）"
        );
    }

    #[test]
    fn the_screen_ramps_are_the_upstream_exponential_form() {
        let host = booted();
        host.eval("globalThis.$gameScreen = new Game_Screen();")
            .unwrap();
        assert_eq!(host.eval_i32("$gameScreen.brightness()"), 255);
        assert_eq!(host.eval_string("$gameScreen.weatherType()"), "none");
        assert_eq!(host.eval_i32("$gameScreen.maxPictures()"), 100);
        // 淡出 `b = b*(d-1)/d`：四拍从 255 走到 0，逐拍是 255·k/4（与旧侧 `Fade::ramp` 同形）。
        host.eval("$gameScreen.startFadeOut(4)").unwrap();
        let mut seen = Vec::new();
        for _ in 0..4 {
            host.eval("$gameScreen.update()").unwrap();
            seen.push(num(&host, "$gameScreen.brightness()"));
        }
        assert_eq!(
            seen,
            vec!["191.25", "127.5", "63.75", "0"],
            "淡出必须走上游的指数逼近并在第 4 拍精确落到 0"
        );
        // 淡入往 255 收（`:816-822`），`duration === 0` 那一条一步到位（`:757-759`）。
        host.eval("$gameScreen.startFadeIn(4)").unwrap();
        host.eval("$gameScreen.update()").unwrap();
        assert_eq!(num(&host, "$gameScreen.brightness()"), "63.75");
        host.eval("$gameScreen.startTint([100, -50, 0, 30], 0)")
            .unwrap();
        assert!(host.eval_bool("$gameScreen.tone().equals([100, -50, 0, 30])"));
        // 闪烁的 alpha 是 `*= (d-1)/d`，不是减常数（`:834-840`）。
        host.eval("$gameScreen.startFlash([255, 255, 255, 100], 4)")
            .unwrap();
        host.eval("$gameScreen.update()").unwrap();
        assert_eq!(num(&host, "$gameScreen.flashColor()[3]"), "75");
        // 震动：`_shakeDuration` 能减成**负数**而那一支还在跑（`AGENTS.md` 咬人第 26 条⑸），
        // 旧侧因此用 `i64`；这里是 JS 的数，形状要照钉。
        host.eval("$gameScreen.startShake(5, 4, 2)").unwrap();
        host.eval("$gameScreen.update(); $gameScreen.update(); $gameScreen.update();")
            .unwrap();
        assert_eq!(
            host.eval_i32("$gameScreen._shakeDuration"),
            -1,
            "震动时长该能减成负"
        );
        assert_ne!(num(&host, "$gameScreen.shake()"), "0");
        // 天气：`type !== 'none' || duration === 0` 才落型，而 `duration === 0` 当场落功率。
        host.eval("$gameScreen.changeWeather('rain', 3, 0)")
            .unwrap();
        assert_eq!(host.eval_string("$gameScreen.weatherType()"), "rain");
        assert_eq!(host.eval_i32("$gameScreen.weatherPower()"), 3);
    }

    #[test]
    fn the_picture_lands_on_the_last_beat_and_erase_keeps_the_spot() {
        let host = booted();
        host.eval("globalThis.__p = new Game_Picture();").unwrap();
        host.eval("__p.show('图名', 0, 0, 0, 100, 100, 255, 0); __p.move(0, 100, 200, 100, 100, 255, 0, 4);")
            .unwrap();
        let mut xs = Vec::new();
        for _ in 0..4 {
            host.eval("__p.update()").unwrap();
            xs.push(num(&host, "__p.x()"));
        }
        assert_eq!(
            xs,
            vec!["25", "50", "75", "100"],
            "四拍补间要逐拍是 25 的台阶并在第 4 拍精确落位（旧侧 `picture.rs` 同一条腿）"
        );
        // `duration === 0` 在上游是"一步都不走"（咬人第 14 条⑵），不许修成瞬移。
        host.eval("__p.move(0, 500, 0, 100, 100, 255, 0, 0); __p.update();")
            .unwrap();
        assert_eq!(host.eval_i32("__p.x()"), 100, "duration 0 该原地不动");
        // 旋转是 `angle += speed / 2` 每一拍（`:1093-1097`，那个 2 是分母）。
        host.eval("__p.rotate(6); __p.update();").unwrap();
        assert_eq!(host.eval_i32("__p.angle()"), 3);
        // `erase` 抹名字/色调/旋转，但**不抹 _x/_y**（`:1057-1063` 只调三个 init）。
        host.eval("__p.erase()").unwrap();
        assert!(host.eval_bool("__p.name() === ''"));
        assert!(host.eval_bool("__p.tone() === null"));
        assert_eq!(host.eval_i32("__p.x()"), 100);
    }

    #[test]
    fn playtime_is_the_frame_counter_in_upstream_units() {
        let host = booted();
        host.eval("$gameSystem = new Game_System();").unwrap();
        // `Graphics.frameCount` 是上游的一条静态数（`rpg_core.js:1798` 初值 0，`:1888` 每拍自加）。
        assert_eq!(host.eval_i32("Graphics.frameCount"), 0);
        assert_eq!(host.eval_i32("$gameSystem.playtime()"), 0);
        assert_eq!(host.eval_string("$gameSystem.playtimeText()"), "00:00:00");
        host.eval("Graphics.frameCount = 3735;").unwrap();
        assert_eq!(
            host.eval_i32("$gameSystem.playtime()"),
            62,
            "60 帧 = 1 秒（`Options` 的 `tick_hz = 60` 同一条腿）"
        );
        assert_eq!(host.eval_string("$gameSystem.playtimeText()"), "00:01:02");
        host.eval("Graphics.frameCount = 219660; // 3661 秒 × 60 帧")
            .unwrap();
        assert_eq!(host.eval_string("$gameSystem.playtimeText()"), "01:01:01");
        // `padZero` 那一族来自 `JsExtensions` ⇒ 这一条同时是"extensions 必须在状态类之前"的证明。
    }

    #[test]
    fn the_temp_flags_come_from_the_platform_face() {
        let host = booted();
        host.eval("globalThis.$gameTemp = new Game_Temp();")
            .unwrap();
        // `Utils.isOptionValid('test')` 交回 `1`/`0`（不是布尔，`../sisf_pray/M10.md`「会咬人的地方（M10 这一族）」第 1 条⑷），
        // 而 `_isPlaytest` 就是那颗值的**原样**（上游没折布尔）。
        assert_eq!(host.eval_i32("$gameTemp._isPlaytest"), 0);
        assert!(
            host.eval_bool("$gameTemp.isPlaytest() === 0"),
            "不许顺手把它折成布尔（两条腿与浏览器默认同一答案）"
        );
        // 公共事件那一格读 `$dataCommonEvents`（managers 那一族的产物）。
        host.eval("$gameTemp.reserveCommonEvent(1)").unwrap();
        assert!(host.eval_bool("$gameTemp.isCommonEventReserved()"));
        assert_eq!(host.eval_i32("$gameTemp.reservedCommonEvent().id"), 1);
        host.eval("$gameTemp.clearCommonEvent()").unwrap();
        assert!(host.eval_bool("$gameTemp.isCommonEventReserved() === false"));
        // 目的地那三格落回 `null`（不是 0），而 `isDestinationValid` 比的正是 `!== null`
        // ⇒ `setDestination(0, 0)` 必须算**有效**（旧侧的图块 0 那一族同源）。
        host.eval("$gameTemp.setDestination(0, 0)").unwrap();
        assert!(host.eval_bool("$gameTemp.isDestinationValid()"));
        host.eval("$gameTemp.clearDestination()").unwrap();
        assert!(host.eval_bool("$gameTemp.isDestinationValid() === false"));
        assert!(host.eval_bool("$gameTemp.destinationX() === null"));
    }

    #[test]
    fn the_system_accessors_read_the_loaded_data_with_the_upstream_fallbacks() {
        let host = booted();
        host.eval("$gameSystem = new Game_System();").unwrap();
        // `||` 那一条腿：没 set 过读 `$dataSystem`；抹回 null 又落回数据。
        assert!(host.eval_bool("$gameSystem.windowTone().equals([0, 0, 0, 0])"));
        host.eval("$gameSystem.setWindowTone([1, 2, 3, 4])")
            .unwrap();
        assert!(host.eval_bool("$gameSystem.windowTone().equals([1, 2, 3, 4])"));
        host.eval("$gameSystem.setWindowTone(null)").unwrap();
        assert!(host.eval_bool("$gameSystem.windowTone() === $dataSystem.windowTone"));
        assert_eq!(host.eval_string("$gameSystem.battleBgm()"), "battle");
        assert_eq!(host.eval_string("$gameSystem.victoryMe()"), "win");
        // `isJapanese` 交回的是 `match` 的**数组**（不是布尔）—— 上游就这么写的，
        // 而插件里 `if ($gameSystem.isJapanese())` 靠的是真值（与 `isOptionValid` 的 1/0 同一族）。
        assert!(host.eval_bool("!!$gameSystem.isJapanese()"));
        assert!(
            host.eval_bool("$gameSystem.isJapanese() !== true"),
            "兼容层不许把 match 的数组折成布尔"
        );
        assert!(host.eval_bool("!!$gameSystem.isCJK()"));
        assert!(!host.eval_bool("!!$gameSystem.isChinese()"));
        assert!(host.eval_bool("$gameSystem.isSideView() === true"));
        // 那两面玩家看得见的闸（`134`/`135`，咬人第 17 条）就在这一族。
        host.eval("$gameSystem.disableMenu(); $gameSystem.disableSave();")
            .unwrap();
        assert!(host.eval_bool("$gameSystem.isMenuEnabled() === false"));
        assert!(host.eval_bool("$gameSystem.isSaveEnabled() === false"));
        host.eval("$gameSystem.enableMenu(); $gameSystem.enableSave();")
            .unwrap();
        assert!(host.eval_bool("$gameSystem.isMenuEnabled()"));
        // `saveWalkingBgm2` 读 `$dataMap.bgm`（`:274-276`）—— 202 换图时"存下当前图 BGM"那半句。
        host.eval("$gameSystem.saveWalkingBgm2()").unwrap();
        assert_eq!(host.eval_string("$gameSystem._walkingBgm.name"), "map");
        // 计数器那一族与 `versionId` 的初值。
        for _ in 0..3 {
            host.eval("$gameSystem.onBattleStart()").unwrap();
        }
        host.eval("$gameSystem.onBattleWin(); $gameSystem.onBattleEscape();")
            .unwrap();
        assert_eq!(host.eval_i32("$gameSystem.battleCount()"), 3);
        assert_eq!(host.eval_i32("$gameSystem.winCount()"), 1);
        assert_eq!(host.eval_i32("$gameSystem.escapeCount()"), 1);
        assert_eq!(
            host.eval_i32("$gameSystem.versionId()"),
            0,
            "没存过档时是 0（initialize）"
        );
    }

    #[test]
    fn reinstalling_the_state_family_keeps_the_host_state() {
        // 判据纪律：兼容层手交的每一段 JS 文本都要能"装第二次"（node 那条腿共享一个 realm）。
        let host = booted();
        map_stand_in(&host);
        host.eval("Graphics.frameCount = 120; $gameVariables = new Game_Variables(); $gameVariables.setValue(1, 9);")
            .unwrap();
        assert_eq!(host.eval_i32("$gameVariables.value(1)"), 9);
        install(&host).expect("第二次装状态类该成功");
        // **守卫那一格才是这一条的正文**：`Graphics.frameCount` 是宿主的计数，第二次装不许把它
        // 抹回 0 —— 抹掉的表征是 `playtime()` 倒退，而读数看着还是绿的（最难查的那一类）。
        assert_eq!(
            host.eval_i32("Graphics.frameCount"),
            120,
            "重复装载不许抹掉宿主已经推进的帧计数"
        );
        assert_eq!(
            host.eval_string("$gameSystem = new Game_System(); $gameSystem.playtimeText()"),
            "00:00:02"
        );
        // **记下的形状**：那十三颗名字第二次装回 `null` —— 这就是"把 `rpg_managers.js:29-41`
        // 重新 `<script>` 一遍"在上游的同一形状（那一句本来是无条件写 null），而把它们再建起来
        // 的那一格是 `DataManager.createGameObjects()`（要等 b/c，见模块头）。
        // 所以测试里的替身也要跟着重装 —— 不是判据迁就实现，是这一格本来就该长这样。
        assert!(host.eval_bool("$gameVariables === null"));
        map_stand_in(&host);
        host.eval("$gameVariables = new Game_Variables(); $gameVariables.setValue(2, 7.5);")
            .unwrap();
        // 行为文本幂等：第二次装完还是同一份实现（不是叠两层，也不是少一层）。
        assert_eq!(host.eval_i32("$gameVariables.value(2)"), 7);
        assert_eq!(
            host.eval_i32("__knocks"),
            1,
            "第二次装完之后写一格该敲一次旗"
        );
        assert_eq!(host.eval_i32("$gameVariables.value(700)"), 0);
    }

    #[test]
    fn the_delivered_texts_cover_exactly_the_census_method_names() {
        // 尺子跟到代码这一格：`game-touch.py` 量出 a 那一族是 160 条 + `Game_Picture` 24 条 = **184**，
        // b 那一族是 381 + `Game_Vehicle` 32 = 413 条 **prototype 赋值**，其中 5 条是链的
        // `constructor` 补句（`X.prototype.constructor = X` 不是函数）⇒ 408 条方法，
        // 再加四条跨类样品 = **412** 条；c 那一族是 627 条 prototype 赋值，其中 5 条补句
        // ⇒ **622** 条方法。合计 **1,218**（`python3 sisf_pray/tools/probes/objects-verbatim.py`
        // 报的是同一个数 —— 两边不是同一个数的时候，红的是这一条，不是那把尺子）。
        // 而"名字齐不齐"只有数得出来才算交付 —— 少一条，插件压上来的别名锚点就落在 `undefined` 上
        // （实测 a 这一族被压上 36 条：`Game_System` 19、`Game_Temp` 11、`Game_Screen` 3、
        // 三条 `onChange` 各 1 —— 最后那三条就是 `MOG_Weather_EX`，见 compat.rs 那条真插件判据；
        // b 这一族被压上 33 条，`Game_Event` 那 27 条是最大的一格；
        // c 这一族被压上 **206 条**，`Game_Battler` 53 / `Game_BattlerBase` 50 / `Game_Action` 31
        // 是插件压得最狠的三格 —— 全上游 30 类之内一共 289 条，c 占掉 206 条）。
        let expect: &[(&str, usize)] = &[
            ("Game_Temp", 11),
            ("Game_System", 44),
            ("Game_Timer", 7),
            ("Game_Message", 37),
            ("Game_Switches", 5),
            ("Game_Variables", 5),
            ("Game_SelfSwitches", 5),
            ("Game_Screen", 46),
            ("Game_Picture", 24),
            ("Game_CharacterBase", 98),
            ("Game_Character", 33),
            ("Game_Player", 70),
            ("Game_Follower", 6),
            ("Game_Followers", 18),
            ("Game_Map", 109),
            ("Game_CommonEvent", 6),
            ("Game_Vehicle", 31),
            ("Game_Event", 37),
            ("Game_Interpreter", 151),
            ("Game_Item", 12),
            ("Game_Action", 95),
            ("Game_ActionResult", 16),
            ("Game_BattlerBase", 125),
            ("Game_Battler", 93),
            ("Game_Actor", 129),
            ("Game_Enemy", 44),
            ("Game_Actors", 2),
            ("Game_Unit", 20),
            ("Game_Party", 68),
            ("Game_Troop", 18),
        ];
        let all = [
            GAME_TEMP_AND_SYSTEM_JS,
            GAME_TIMER_AND_MESSAGE_JS,
            GAME_STATE_TABLES_JS,
            GAME_SCREEN_JS,
            GAME_PICTURE_JS,
            GAME_CHARACTERS_JS,
            GAME_MAP_AND_EVENTS_JS,
            GAME_BATTLE_JS,
            GAME_ACTOR_AND_PARTY_JS,
            GAME_INTERPRETER_JS,
        ]
        .join("");
        let mut total = 0usize;
        for (class, count) in expect {
            let needle = format!("{class}.prototype.");
            // **按名字去重**：b 那一轮为 `new Game_Map()` 交的四条 `Game_Interpreter` 样品
            // （`initialize`/`clear`/`checkOverflow`/`isRunning`）现在与 d 那一段的全文重合，
            // 文本里是 155 行、名字只有 151 个。两份都是上游逐字 ⇒ 装载顺序在后的一份覆盖在前的一份
            // 上面，语义不变；不去重的话这里会读出 155（= 假的"多交了四条"）。
            let mut lines: Vec<&str> = all
                .lines()
                .filter(|l| l.starts_with(needle.as_str()) && l.contains("= function"))
                .collect();
            lines.sort_unstable();
            lines.dedup();
            let found = lines.len();
            assert_eq!(
                found, *count,
                "{class} 交出的条数与上游 census 不再是同一个数"
            );
            total += found;
        }
        assert_eq!(total, 1365, "四条腿交出的方法总数变了");
        // 文本里**不许**有构造器（那是 shapes 的那一格），也不许有 `Object.create` 之外的链写法。
        assert!(
            !all.lines().any(|l| l.starts_with("function Game_")),
            "交出去的文本不许重述构造器 ⇒ 插件压上来的原型会被换成另一颗对象"
        );
        // 十条链 + 十条补句：上游 `rpg_objects.js` 的 10 条 `Object.create` **到 c 才算交齐**
        // （b 五条：`Character←CharacterBase` + `Event/Player/Follower/Vehicle←Character`；
        // c 五条：`Battler←BattlerBase` + `Actor/Enemy←Battler` + `Party/Troop←Unit`）。
        let chains = all
            .lines()
            .filter(|l| l.contains("= Object.create(") && l.ends_with(".prototype);"))
            .count();
        let ctor_links = all
            .lines()
            .filter(|l| l.contains(".prototype.constructor = "))
            .count();
        assert_eq!(
            chains, 10,
            "上游十条原型链（b 五 + c 五）必须全交，少一条就是子类的实例够不着基类"
        );
        assert_eq!(ctor_links, 10, "每条链都带着上游那一句 `constructor` 补句");
        // 静态常量那一格：全大写名字的顶层赋值，随类一起交（少它的表征各不一样，见模块头 6/7 两条）。
        // `ROUTE_*` 46 + `Game_Action` 17 + `Game_BattlerBase` 30 + `Game_Party` 6 + `Game_Troop` 2 = **101**。
        let is_static_const_line = |l: &str| -> bool {
            let Some(head) = l.split(' ').next() else {
                return false;
            };
            let Some((class, member)) = head.split_once('.') else {
                return false;
            };
            class.starts_with("Game_")
                && member != "prototype"
                && member
                    .chars()
                    .all(|c| c.is_uppercase() || c == '_' || c.is_ascii_digit())
                && member.len() > 1
        };
        let consts: Vec<&str> = all.lines().filter(|l| is_static_const_line(l)).collect();
        assert_eq!(consts.len(), 101, "静态常量的条数与上游不再是同一个数");
        let routes = all
            .lines()
            .filter(|l| l.starts_with("Game_Character.ROUTE_"))
            .count();
        assert_eq!(routes, 46, "`ROUTE_*` 的条数与上游不再是同一个数");
        // 属性访问器那一族：`Object.defineProperties` 两块（`Game_CharacterBase` 的 `x`/`y` +
        // `Game_BattlerBase` 的 `hp`/`mp`/`tp`/`mhp`…）与 `Object.defineProperty` 一条
        // （`Game_Actor.prototype` 的 `level`，实测全仓只有这一条单数写法）。
        // "名字齐"那把尺子看不见它们（右值不是函数），少交的表征是读出 `undefined`，不抛。
        let accessors = all
            .lines()
            .filter(|l| l.starts_with("Object.defineProperties("))
            .count();
        assert_eq!(
            accessors, 2,
            "`Game_CharacterBase` 与 `Game_BattlerBase` 两块属性访问器都要随类交"
        );
        let accessor_one = all
            .lines()
            .filter(|l| l.starts_with("Object.defineProperty("))
            .count();
        assert_eq!(
            accessor_one, 1,
            "`Game_Actor.prototype` 那颗单数写法的 `level` 访问器（全仓唯一一条）"
        );
        // `Game_Troop` 那两条是**值跨行的数组**：头一行被上一条计数吃掉了，但数组本体
        // 有没有跟着切出来要单独钉（少它的表征是 `envelope()` 读出 `undefined`）。
        assert!(
            all.contains("'Ｚ'\n];") && all.contains("' Z'\n];"),
            "`LETTER_TABLE_HALF`/`_FULL` 的数组本体必须跟着头一行一起交"
        );
    }

    #[test]
    fn every_delivered_member_is_a_function_on_the_installed_prototype() {
        // 名字级的那一半（上一条只数文本）：在 realm 里逐格问 `typeof`，
        // 于是"文本里有、装载时被语法吃掉"的那种红也抓得住。
        let host = booted();
        let mut names: Vec<(String, String)> = [
            GAME_TEMP_AND_SYSTEM_JS,
            GAME_TIMER_AND_MESSAGE_JS,
            GAME_STATE_TABLES_JS,
            GAME_SCREEN_JS,
            GAME_PICTURE_JS,
            GAME_CHARACTERS_JS,
            GAME_MAP_AND_EVENTS_JS,
            GAME_BATTLE_JS,
            GAME_ACTOR_AND_PARTY_JS,
            GAME_INTERPRETER_JS,
        ]
        .join("")
        .lines()
        .filter(|l| l.contains(".prototype.") && l.contains("= function"))
        .filter_map(|line| {
            let head = line.split(".prototype.").next()?.to_string();
            let tail = line.split(".prototype.").nth(1)?;
            let method = tail
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
                .collect::<String>();
            Some((head, method))
        })
        .collect::<Vec<(String, String)>>();
        // 同一格里 b 的四条 `Game_Interpreter` 样品与 d 的全文重合 ⇒ 名字去重之后才是 1,365。
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 1365);
        for (class, method) in &names {
            assert_eq!(
                host.eval_string(&format!("typeof {class}.prototype.{method}")),
                "function",
                "{class}.{method} 没落到 realm 里"
            );
        }
    }

    // ==== b 那一腿：地图与角色 ====

    /// 装到 b 那一档，并把**一颗真的 `Game_Map`** 建起来的宿主。
    ///
    /// `$gameMap` 是判据自己 `new` 的：`DataManager.createGameObjects` 还没交（模块头第 1 条 ——
    /// 它 `new` 的 `Game_Actors`/`Game_Party`/`Game_Troop` 是 c 的类，占位构造器没有 `initialize`）。
    ///
    /// fixture 的形状是照着要钉的判据挑的，四个数都是**判据的输入**：
    /// - 地图 4×4，六层全填图块 1（`flags[1] = 0` ⇒ 四面都通），只有 `(2,1)` 那一格是图块 0
    ///   （`flags[0] = 0x0f` ⇒ 四面都不通）—— 方向位 `(1 << (d/2-1) & 0x0f)` 就是这一族唯一的读法；
    /// - 事件 1 **三页**：第 0 页无条件（`conditions` 全 false —— 旧侧咬人第 16 条那一格），
    ///   第 1 页要"开关 5 ON"，第 2 页要"开关 6 ON"（上游的两格开关条件**都是要 ON**，
    ///   不是"一 ON 一 OFF" —— 第一版照直觉写成一 ON 一 OFF，判据当场把它量出来了）；
    /// - 事件 2 的页表只有终止符那一条（`[{code:0}]`）—— 旧侧咬人第 22 条⑴ 的那道闸；
    /// - 公共事件 1 是 `trigger 2`（并行）+ 开关 7 ⇒ `setupEvents` 会替它建一个 `Game_CommonEvent`。
    fn map_host() -> Host {
        let host = booted();
        build_map(&host);
        host
    }

    /// 上面那段 fixture 的本体 —— 抽出来是因为**重装行为族会把 `$gameMap` 抹回 null**
    /// （[`GAME_GLOBALS_JS`] 照上游无条件写 null），重装的判据要能把实例再建一次。
    fn build_map(host: &Host) {
        host.eval(
            r#"
            // 载具那三格已经在 `booted` 的 `$dataSystem` 里（`new Game_Map()` 当场要它们），
            // 这一档只补地图本体。
            globalThis.$dataTilesets = [null, {flags: [0x0f, 0x00]}];
            globalThis.$dataCommonEvents = [null, {id: 1, trigger: 2, switchId: 7, list: []}];
            var NO_COND = {switch1Valid: false, switch2Valid: false, variableValid: false,
                           selfSwitchValid: false, itemValid: false, actorValid: false};
            var SW5_ON  = {switch1Valid: true, switch1Id: 5, switch2Valid: false, variableValid: false,
                           selfSwitchValid: false, itemValid: false, actorValid: false};
            var SW6_ON  = {switch1Valid: true, switch1Id: 6, switch2Valid: false, variableValid: false,
                           selfSwitchValid: false, itemValid: false, actorValid: false};
            var IMAGE = {tileId: 0, characterName: 'AAlcy', characterIndex: 0, direction: 2, pattern: 0};
            var ROUTE = {list: [], repeat: false, skipping: true, wait: false};
            function page(cond) {
                return {conditions: cond, image: IMAGE, moveType: 1, moveSpeed: 3, moveFrequency: 3,
                        moveRoute: ROUTE, walkAnime: true, stepAnime: true, directionFix: false,
                        through: false, priorityType: 0, trigger: 0,
                        list: [{code: 101, indent: 0, parameters: ['', 0, 0, 0]},
                               {code: 0, indent: 0, parameters: []}]};
            }
            function terminatorOnly() {
                var p = page(NO_COND);
                p.list = [{code: 0, indent: 0, parameters: []}];
                return p;
            }
            globalThis.$dataMap = {
                bgm: {name: 'map', volume: 90, pitch: 100}, tilesetId: 1, width: 4, height: 4,
                parallaxName: '', battleback1Name: '', battleback2Name: '',
                data: [],
                events: [null,
                    {id: 1, x: 0, y: 0, name: 'E1', pages: [page(NO_COND), page(SW5_ON), page(SW6_ON)]},
                    {id: 2, x: 3, y: 3, name: 'E2', pages: [terminatorOnly()]}]
            };
            for (var z = 0; z < 6; z++) {
                for (var y = 0; y < 4; y++) {
                    for (var x = 0; x < 4; x++) {
                        $dataMap.data.push((x === 2 && y === 1) ? 0 : 1);
                    }
                }
            }
            globalThis.$gameSwitches     = new Game_Switches();
            globalThis.$gameVariables    = new Game_Variables();
            globalThis.$gameSelfSwitches = new Game_SelfSwitches();
            globalThis.$gameMap          = new Game_Map();
            $gameMap.setup(1);
            // `$gamePlayer` 这一格交的是**属性替身**：`Game_Event.isNearThePlayer()` 读的是
            // `$gamePlayer.x`/`.y` 两格**属性**（访问器那一族）。真实例现在还建不出来 ——
            // `Game_Player.initialize` 走 `new Game_Followers()`，而它的体读
            // `$gameParty.maxBattleMembers()`，那是 c 的类（见 `the_player_still_needs_the_party`）。
            globalThis.$gamePlayer = {x: 3, y: 3};
            "#,
        )
        .expect("b 的 fixture 装不上（一颗 Game_Map 都建不起来）");
    }

    #[test]
    fn the_player_still_needs_the_party() {
        // b 那一轮这条钉的是"c 还没到"；c 之后**换了住户，边界还在**：
        // `Game_Player.initialize` 走 `new Game_Followers()`，它的体是
        // `for (var i = 1; i < $gameParty.maxBattleMembers(); i++)` —— 方法现在是 c 交的了，
        // 缺的是 **`$gameParty` 这颗实例**（= `createGameObjects`，见 `battler_host`）。
        // fixture 这一格仍然交**属性替身**（`{x: 3, y: 3}`）：`Game_Event.isNearThePlayer()`
        // 只读那两格，而把真实例引进 `map_host` 会让 b 的每一条判据都欠 `createGameObjects` 一次。
        let host = map_host();
        assert_eq!(
            host.eval_string("String($gamePlayer instanceof Game_Player)"),
            "false",
            "`$gamePlayer` 是替身，不是实例（这颗宿主没跑 `createGameObjects`）"
        );
        assert_eq!(
            host.eval_string("String($gamePlayer.x)"),
            "3",
            "替身读的是**属性** `x`（真实例那一格是访问器，两条腿同一形状）"
        );
        // c 到了：方法在、链也接上了（**这两格 b 那一轮是反过来钉的**，改判写在 `M10.md` 验收（三））。
        assert_eq!(
            host.eval_string("String(typeof Game_Party.prototype.maxBattleMembers)"),
            "function",
            "`Game_Party` 的行为随 c 交到了 —— 别再把它读成「只是形状」"
        );
        assert_eq!(
            host.eval_string(
                "String(Object.getPrototypeOf(Game_Actor.prototype) === Game_Battler.prototype)"
            ),
            "true",
            "`Game_Actor←Game_Battler` 那条链是 c 交的五条之一"
        );
        // 而实例化这一格**照样抛**，只是原因换成了 `$gameParty` 是 null：不许悄悄建出半颗玩家。
        let err = host
            .eval("globalThis.__try = new Game_Player();")
            .expect_err("`$gameParty` 还是 null，玩家的实例化必须抛");
        assert!(
            host.eval_string("String($gameParty === null)") == "true",
            "抛的前提没了：{err}"
        );
        // 事件那一侧不受影响：`isNearThePlayer` 只读两格属性 ⇒ 替身就够它把锁上。
        assert_eq!(
            host.eval_string("String($gameMap.event(1).isNearThePlayer())"),
            "true"
        );
    }

    #[test]
    fn the_five_inheritance_chains_reach_the_base_methods() {
        // 名字齐（普查那 3,107 条）与**够得着**（这一族）是两格：`Game_Event` 的实例必须能调到
        // `Game_CharacterBase` 那 98 条，否则插件压在 `Game_Event.prototype` 上的 27 条别名一调用就断。
        let host = map_host();
        let js = r#"
            var ev = $gameMap.event(1);
            var out = [];
            out.push(ev instanceof Game_Event);
            out.push(ev instanceof Game_Character);
            out.push(ev instanceof Game_CharacterBase);
            out.push(Game_Event.prototype.constructor === Game_Event);
            out.push(Object.getPrototypeOf(Game_Event.prototype) === Game_Character.prototype);
            out.push(Object.getPrototypeOf(Game_Character.prototype) === Game_CharacterBase.prototype);
            out.push(typeof ev.refresh === 'function');
            out.push(Object.getPrototypeOf(Game_Player.prototype) === Game_Character.prototype);
            out.push(new Game_Vehicle('ship') instanceof Game_CharacterBase);
            out.push(new Game_Follower(0) instanceof Game_CharacterBase);
            // 反面对照（c 之后仍然成立的那一种红）：**形状有、成员没交**的那颗静态类。
            // `shapes` 交出 `SoundManager` 这颗名字，而 11 条 `play*` 成员在 M10.4 ——
            // 于是"名字取到"而"调用抛"，与"类不存在"是两种不同的红。
            out.push(typeof SoundManager === 'function');
            out.push(typeof SoundManager.playMiss === 'undefined');
            out.join(',')
        "#;
        assert_eq!(
            host.eval_string(js),
            "true,true,true,true,true,true,true,true,true,true,true,true"
        );
    }

    #[test]
    fn the_other_five_chains_reach_the_battler_base_methods() {
        // 上游十条链的**后五条**随 c 交（`Battler←BattlerBase`、`Actor/Enemy←Battler`、
        // `Party/Troop←Unit`）。这一条钉的是"够得着"而不是"名字齐"：插件压在
        // `Game_Battler.prototype` 上的 53 条与 `Game_BattlerBase` 上的 50 条别名，
        // 要靠这条链从 `Game_Actor` 的实例走到基类那 125 条。
        let host = battler_host();
        let js = r#"
            var out = [];
            out.push(Object.getPrototypeOf(Game_Battler.prototype) === Game_BattlerBase.prototype);
            out.push(Object.getPrototypeOf(Game_Actor.prototype) === Game_Battler.prototype);
            out.push(Object.getPrototypeOf(Game_Enemy.prototype) === Game_Battler.prototype);
            out.push(Object.getPrototypeOf(Game_Party.prototype) === Game_Unit.prototype);
            out.push(Object.getPrototypeOf(Game_Troop.prototype) === Game_Unit.prototype);
            // 每条链都带着上游那一句 `constructor` 补句（子类实例的 `.constructor` 才是自己）。
            out.push(Game_Actor.prototype.constructor === Game_Actor);
            out.push(Game_Troop.prototype.constructor === Game_Troop);
            // 基类成员从子类实例真的调得通：`Game_BattlerBase` 的 `isStateAffected`、
            // `Game_Unit.prototype.members`、`Game_Battler` 的 `result`。
            out.push(typeof new Game_Actor(1).isStateAffected === 'function');
            out.push(typeof new Game_Troop().members === 'function');
            out.push(typeof new Game_Party().members === 'function');
            // 实例跨过了两层基类：`instanceof` 一路查到 `Game_BattlerBase` / `Game_Unit`。
            out.push(new Game_Enemy(1, 0, 0) instanceof Game_BattlerBase);
            out.push(new Game_Party() instanceof Game_Unit);
            out.join(',')
        "#;
        assert_eq!(
            host.eval_string(js),
            "true,true,true,true,true,true,true,true,true,true,true,true"
        );
    }

    #[test]
    fn the_route_step_constants_come_with_the_character_class() {
        // 这 46 条不是"锦上添花"：`route.list[i].code` 比的是 `Game_Character.ROUTE_*`。
        // 少交它们的表征**不是抛**，是每一个 `===` 都恒 false ⇒ 自定义路线一步都不走
        // （旧移植侧 `sisf_pray/src/route.rs` 手写的就是这一族步码，从此有上游那一份）。
        let host = booted();
        assert_eq!(num(&host, "Game_Character.ROUTE_END"), "0");
        assert_eq!(num(&host, "Game_Character.ROUTE_MOVE_DOWN"), "1");
        assert_eq!(num(&host, "Game_Character.ROUTE_JUMP"), "14");
        assert_eq!(num(&host, "Game_Character.ROUTE_WAIT"), "15");
        assert_eq!(num(&host, "Game_Character.ROUTE_SWITCH_ON"), "27");
        // 1.6.1 新增的那一格：39/40 透明、43 混合模式、45 脚本（旧侧靠步码 39/40/41 接隐身）。
        assert_eq!(num(&host, "Game_Character.ROUTE_TRANSPARENT_ON"), "39");
        assert_eq!(num(&host, "Game_Character.ROUTE_TRANSPARENT_OFF"), "40");
        assert_eq!(num(&host, "Game_Character.ROUTE_CHANGE_BLEND_MODE"), "43");
        assert_eq!(num(&host, "Game_Character.ROUTE_SCRIPT"), "45");
        assert_eq!(
            num(
                &host,
                "Object.keys(Game_Character).filter(function(k){ return k.indexOf('ROUTE_') === 0; }).length"
            ),
            "46",
            "`ROUTE_*` 的条数与上游不再是同一个数"
        );
    }

    #[test]
    fn the_map_setup_builds_events_vehicles_and_common_events() {
        let host = map_host();
        assert_eq!(num(&host, "$gameMap.mapId()"), "1");
        assert_eq!(num(&host, "$gameMap.tilesetId()"), "1");
        assert_eq!(num(&host, "$gameMap.events().length"), "2");
        assert_eq!(num(&host, "$gameMap.event(1).eventId()"), "1");
        assert_eq!(num(&host, "$gameMap.event(1).x"), "0");
        assert_eq!(num(&host, "$gameMap.event(1).y"), "0");
        // `Game_Map.initialize` 里 `new Game_Vehicle` 三次：0 船 / 1 艇 / 2 飞艇。
        assert_eq!(num(&host, "$gameMap.vehicles().length"), "3");
        assert_eq!(
            host.eval_string("String($gameMap.vehicle(2).isAirship())"),
            "true"
        );
        // 并行公共事件（`trigger === 2`）那一条被建成实例。
        assert_eq!(num(&host, "$gameMap._commonEvents.length"), "1");
        // `Game_Map.initialize` 当场 `new Game_Interpreter()` —— 那四条样品没交就建不出地图。
        assert_eq!(
            host.eval_string("String($gameMap._interpreter instanceof Game_Interpreter)"),
            "true"
        );
        assert_eq!(
            host.eval_string("String($gameMap.isEventRunning())"),
            "false",
            "`isRunning()` 没交到位时这一格会抛，而不是交出 false"
        );
    }

    #[test]
    fn a_multi_page_event_picks_the_highest_valid_page() {
        // 上游 `findProperPageIndex` 是 `for (i = pages.length - 1; i >= 0; i--)` ⇒ **取最高的成立页**。
        // 第 0 页无条件那一格就是旧侧咬人第 16 条（引擎取"第一个成立页"⇒ 永远停在 0，实测影响
        // 1,624/2,019 个事件；旧侧用 `sisf_pray/src/pages.rs::flip` 把页表倒序收口）。
        // 这一条钉的是兼容层**不需要那一手**：三页 = [无条件, 开关 5 ON, 开关 6 ON]。
        let host = map_host();
        assert_eq!(
            num(&host, "$gameMap.event(1)._pageIndex"),
            "0",
            "两格开关都没立 ⇒ 只剩无条件的那一页"
        );
        host.eval("$gameSwitches.setValue(5, true);").unwrap();
        refresh_map(&host);
        assert_eq!(num(&host, "$gameMap.event(1)._pageIndex"), "1");
        host.eval("$gameSwitches.setValue(6, true);").unwrap();
        refresh_map(&host);
        assert_eq!(
            num(&host, "$gameMap.event(1)._pageIndex"),
            "2",
            "两格都立 ⇒ 取最高的那一页（旧侧没有 `flip` 的话这里会停在 1）"
        );
        // 页号读的是 `_pageIndex`，而上游对外的页号 = 数组下标（旧侧那两套口径的同一条腿）。
        assert_eq!(
            host.eval_string(
                "String($gameMap.event(1).page() === $gameMap.event(1).event().pages[2])"
            ),
            "true"
        );
        host.eval("$gameSwitches.setValue(5, false);").unwrap();
        host.eval("$gameSwitches.setValue(6, false);").unwrap();
        refresh_map(&host);
        assert_eq!(num(&host, "$gameMap.event(1)._pageIndex"), "0");
        // 擦掉事件：`_erased` ⇒ 页号 -1（旧侧"隐藏"的那一格）。
        host.eval("$gameMap.event(1).erase();").unwrap();
        assert_eq!(num(&host, "$gameMap.event(1)._pageIndex"), "-1");
    }

    /// 把 `$gameMap` 刷一遍（`refreshIfNeeded` 之外的那一条手路）。
    fn refresh_map(host: &Host) {
        host.eval("$gameMap.refresh();").unwrap();
    }

    #[test]
    fn a_page_with_only_the_terminator_does_not_start() {
        // 上游 `Game_Event.prototype.start`：`if (list && list.length > 1)` ⇒ 只剩终止符的**空页不设启动旗**。
        // 浏览器复验报回过"无法和 NPC 对话"：空事件把决定键吃掉，而它面前的 NPC 就永远轮不到
        // （旧侧咬人第 22 条⑴，落点是 `Game::start_event` 那道 `p.list.len() <= 1` 的闸）。
        let host = map_host();
        host.eval("$gameMap.event(2).start();").unwrap();
        assert_eq!(
            host.eval_string("String($gameMap.event(2).isStarting())"),
            "false",
            "空页不许设启动旗"
        );
        host.eval("$gameMap.event(1).start();").unwrap();
        assert_eq!(
            host.eval_string("String($gameMap.event(1).isStarting())"),
            "true",
            "有正文的页要立旗"
        );
        // 立了旗的那一条同时把"锁"上了（`isTriggerIn([0,1,2])` 那一支）—— 决定键事件的形状。
        assert_eq!(
            host.eval_string("String($gameMap.event(1)._locked)"),
            "true"
        );
    }

    #[test]
    fn passage_reads_the_direction_bit_of_the_tileset_flag() {
        // `(1 << (d/2 - 1) & 0x0f)`：2 下 / 4 左 / 6 右 / 8 上 各占一个位；`flags[tileId]` 的 0x10 是
        // "[*] 不影响通行"。fixture 里只有 `(2,1)` 是四面不通的图块 0。
        let host = map_host();
        assert_eq!(
            host.eval_string("String($gameMap.isPassable(1, 1, 6))"),
            "true"
        );
        assert_eq!(
            host.eval_string("String($gameMap.isPassable(2, 1, 6))"),
            "false"
        );
        // 同一格换个方向问也是不通（0x0f 四个位全占）。
        assert_eq!(
            host.eval_string("String($gameMap.isPassable(2, 1, 8))"),
            "false"
        );
        // `canPass` 还要**两边**都通（上游 `isMapPassable` 问 `d` 与 `reverseDir(d)` 两格）：
        // (1,1)→(2,1) 那一格自己不通 ⇒ 整步不通。
        assert_eq!(
            host.eval_string("String($gameMap.isValid(2, 1))"),
            "true",
            "4×4 的图里 (2,1) 在界内 —— 这一格先把 `isValid` 钉住，免得把越界读成不通"
        );
        // 斜向是"两条 L 形通路任一成立"，不是四格全清（旧侧咬人第 15 条）。
        let js = r#"
            var ch = new Game_Character();
            ch.locate(1, 1);
            var out = [];
            out.push(ch.canPass(1, 1, 6));            // 右：(2,1) 是墙 ⇒ false
            out.push(ch.canPass(1, 1, 4));            // 左：(0,1) 四面通 ⇒ true
            out.push(ch.canPassDiagonally(1, 1, 6, 2)); // 右下：右不通但下通 ⇒ 两条 L 之一成立 ⇒ true
            out.join(',')
        "#;
        assert_eq!(host.eval_string(js), "false,true,true");
    }

    #[test]
    fn move_straight_lands_on_the_grid_or_reports_failure() {
        // 上游那一格的形状（旧侧咬人第 15 条第一条）：**走不通的步不前进路线索引**，而 `moveStraight`
        // 无论成败都会转一次朝向（失败那一支走 `checkEventTriggerTouchFront`）。
        let host = map_host();
        let js = r#"
            var ch = new Game_Character();
            ch.locate(1, 1);
            var out = [];
            ch.moveStraight(6);
            out.push(ch._x === 1 && ch._y === 1);
            out.push(ch.isMovementSucceeded());
            out.push(ch._direction === 6);
            ch._stopCount = 5;
            ch.moveStraight(4);
            out.push(ch._x === 0 && ch._y === 1);
            out.push(ch.isMovementSucceeded());
            out.push(ch._stopCount === 0);
            out.join(',')
        "#;
        assert_eq!(host.eval_string(js), "true,false,true,true,true,true");
    }

    #[test]
    fn the_vehicle_loads_its_own_start_position_from_the_system_json() {
        // 三条实测：⑴ 载具配置**住在 `System.json` 的 `boat/ship/airship` 三格里**，不是一张
        // `$dataVehicles` 表（`Game_Vehicle.vehicle()` 那三句就是这一格的形状）—— 第一版照编辑器
        // 的名词写成 `$dataVehicles`，当场红在 `loadSystemSettings` 读 `startMapId` 那一句；
        // ⑵ `Game_Map.vehicle(type)` 的 `type` 既收 0/1/2 也收 'boat'/'ship'/'airship'；
        // ⑶ 不在自己那张图上的载具是**隐身**的（`setTransparent(_mapId !== $gameMap.mapId())`）。
        let host = map_host();
        assert_eq!(num(&host, "$gameMap.vehicle(0).x"), "4");
        assert_eq!(num(&host, "$gameMap.vehicle(0).y"), "5");
        assert_eq!(num(&host, "$gameMap.vehicle(0)._mapId"), "3");
        assert_eq!(
            host.eval_string("String($gameMap.vehicle(0).isBoat())"),
            "true"
        );
        assert_eq!(
            host.eval_string("String($gameMap.vehicle('ship').isShip())"),
            "true",
            "两种拼写都要认（插件按字符串写）"
        );
        assert_eq!(num(&host, "$gameMap.vehicle(1)._mapId"), "6");
        assert_eq!(
            host.eval_string("String($gameMap.vehicle(2).isAirship())"),
            "true"
        );
        assert_eq!(
            host.eval_string("String($gameMap.vehicle(2)._transparent)"),
            "true"
        );
        assert_eq!(
            host.eval_string("String($gameMap.vehicle(0)._transparent)"),
            "true",
            "船停在 map 3，而当前是 map 1 ⇒ 透明"
        );
        // 行走图名也来自那三格（`setImage(vehicle.characterName, vehicle.characterIndex)`）。
        assert_eq!(
            host.eval_string("String($gameMap.vehicle(0).characterName())"),
            "boat"
        );
    }

    #[test]
    fn the_map_knock_lands_once_the_map_is_built() {
        // 模块头第 2 条那一格**翻回成功**：a 那一轮 `onChange` 只能靠记数的 `$gameMap` 替身钉，
        // 因为 `Game_Map.prototype.requestRefresh` 还不存在。b 之后它存在了 —— 这一条钉的是
        // "写开关 → 旗立 → `refreshIfNeeded` 放掉"整条腿在**真类**上落得下去。
        let host = map_host();
        host.eval("$gameMap._needsRefresh = false;").unwrap();
        host.eval("$gameSwitches.setValue(11, true);").unwrap();
        assert_eq!(host.eval_string("String($gameMap._needsRefresh)"), "true");
        host.eval("$gameMap.refreshIfNeeded();").unwrap();
        assert_eq!(
            host.eval_string("String($gameMap._needsRefresh)"),
            "false",
            "`refreshIfNeeded` 要放掉那面旗，否则每拍都重刷一遍"
        );
        // 三张表都敲（同一条 `onChange` 的形状），越界写不敲（界在 a 那一轮已经钉过）。
        for (expr, knock) in [
            ("$gameVariables.setValue(11, 1);", "true"),
            ("$gameSelfSwitches.setValue([1, 1, 'A'], true);", "true"),
            ("$gameSwitches.setValue(1000, true);", "false"),
        ] {
            host.eval("$gameMap._needsRefresh = false;").unwrap();
            host.eval(expr).unwrap();
            assert_eq!(
                host.eval_string("String($gameMap._needsRefresh)"),
                knock,
                "{expr} 之后的旗不对"
            );
        }
    }

    #[test]
    fn the_common_event_raises_an_interpreter_only_when_active() {
        // 并行公共事件（CE211「鉱脈掘り画像処理」那一族的形状）：`isActive` = `trigger === 2`
        // **且** 它自己那颗开关为真。开关 OFF 时不建解释器 —— 旧侧的并行 runner 走的是另一条腿
        // （旧侧咬人第 7 条），这一条只钉兼容层交出的形状与上游一致。
        let host = map_host();
        assert_eq!(
            host.eval_string("String($gameMap._commonEvents[0].isActive())"),
            "false",
            "开关 7 还没立，并行事件不该活跃"
        );
        assert_eq!(
            host.eval_string("String($gameMap._commonEvents[0]._interpreter === undefined || $gameMap._commonEvents[0]._interpreter === null)"),
            "true"
        );
        host.eval("$gameSwitches.setValue(7, true);").unwrap();
        host.eval("$gameMap._commonEvents[0].refresh();").unwrap();
        assert_eq!(
            host.eval_string(
                "String($gameMap._commonEvents[0]._interpreter instanceof Game_Interpreter)"
            ),
            "true",
            "开关立起来之后解释器要建出来（那四条样品就是为这一格交的）"
        );
        assert_eq!(
            host.eval_string("String($gameMap._commonEvents[0].isActive())"),
            "true"
        );
    }

    #[test]
    fn reinstalling_leg_b_keeps_the_chains_and_the_methods_resolvable() {
        // wasm 那条测试腿一整个进程共享一个 realm ⇒ 装载第二次是**常态**（`M10.md`「这一族的规矩」）。
        // 链那一族是新风险：`X.prototype = Object.create(Base.prototype)` 再走一遍会换掉那颗
        // prototype 对象，而方法是在它上面挂的 ⇒ 判据分三步钉：
        // ① 重装之后行为面仍然解得开（方法在、链在、`ROUTE_*` 在）；
        // ② `$gameMap` 被 [`GAME_GLOBALS_JS`] 抹回 `null` 是**上游的样子**（重新 `<script>` 一遍
        //    `rpg_managers.js` 就是这个结果，把实例建回来的那一格是 `createGameObjects`）；
        // ③ 重新建出来的实例照样能选页、能上锁（幂等）。
        let host = map_host();
        host.eval("$gameMap.event(1).start();").unwrap();
        install(&host).expect("b 重装不上");
        let js = r#"
            var out = [];
            out.push(typeof Game_Event.prototype.setupPage === 'function');
            out.push(typeof Game_CharacterBase.prototype.moveStraight === 'function');
            out.push(Object.getPrototypeOf(Game_Event.prototype) === Game_Character.prototype);
            out.push(Game_Character.ROUTE_WAIT === 15);
            out.push($gameMap === null);
            out.join(',')
        "#;
        assert_eq!(host.eval_string(js), "true,true,true,true,true");
        build_map(&host);
        let again = r#"
            var ev = $gameMap.event(1);
            var out = [];
            out.push(ev instanceof Game_CharacterBase);
            out.push(ev._pageIndex === 0);
            ev.start();
            out.push(ev.isStarting());
            out.push(ev._locked === true);
            out.join(',')
        "#;
        assert_eq!(host.eval_string(again), "true,true,true,true");
    }

    // ==== c 那一腿：玩法（战斗与队伍） ====

    #[test]
    fn create_game_objects_builds_the_thirteen_instances() {
        // 模块头偏差 1 闭合的那一格：`GAME_GLOBALS_JS` 抹回 null 的十三颗名字，
        // 由 `DataManager.createGameObjects`（`managers.rs`，上游 `rpg_managers.js:200-214` 逐字）
        // 建成实例。**顺序也是上游的**：`Game_Actors` 在 `Game_Party` 之前（`allMembers` 要
        // `$gameActors.actor`），而 `Game_Player` 最后一颗（它的 `initialize` 走
        // `new Game_Followers()`，读 `$gameParty`）。
        let host = booted();
        host.eval("Graphics.frameCount = 120;").unwrap();
        host.eval("DataManager.createGameObjects();")
            .expect("十三颗实例建不出来");
        let js = r#"
            var out = [];
            out.push($gameTemp instanceof Game_Temp);
            out.push($gameSystem instanceof Game_System);
            out.push($gameScreen instanceof Game_Screen);
            out.push($gameTimer instanceof Game_Timer);
            out.push($gameMessage instanceof Game_Message);
            out.push($gameSwitches instanceof Game_Switches);
            out.push($gameVariables instanceof Game_Variables);
            out.push($gameSelfSwitches instanceof Game_SelfSwitches);
            out.push($gameActors instanceof Game_Actors);
            out.push($gameParty instanceof Game_Party);
            out.push($gameTroop instanceof Game_Troop);
            out.push($gameMap instanceof Game_Map);
            out.push($gamePlayer instanceof Game_Player);
            // 玩家这一颗是 b 那一轮唯一没能落成实例的一格（`new Game_Followers()` 读 `$gameParty`）。
            out.push($gamePlayer.followers() instanceof Game_Followers);
            // 那颗帧计数不是这一颗的输入：上游 `createGameObjects` 没碰 `Graphics.frameCount`
            // （碰它的是没交的 `setupNewGame` 末行）⇒ 宿主推进的 120 拍必须还在。
            out.push(Graphics.frameCount === 120);
            // 队伍这时候还是**空的**：填人是 `setupNewGame` 那一格（`setupStartingMembers`），
            // 它挂在存档那条待定项上；这里只钉"方法是 c 交的、显式调得动"。
            out.push($gameParty.members().length === 0);
            $gameParty.setupStartingMembers();
            out.push($gameParty.members().length === 1);
            out.push($gameParty.members()[0] instanceof Game_Actor);
            out.push($gameParty.members()[0].name() === 'A1');
            out.join(',')
        "#;
        assert_eq!(
            host.eval_string(js),
            "true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true"
        );
    }

    #[test]
    fn the_battler_accessors_are_the_upstream_property_names() {
        // 上游那一块 `Object.defineProperties(Game_BattlerBase.prototype, {…})` 交的是**属性**，
        // 不是方法 ⇒ "名字齐"的普查与逐字尺子都数不到它，而插件与上游代码全按属性读
        // （`actor.hp`、`enemy.mhp`）。少一格的表征还是 `undefined`，不是抛（同 b 那轮的 `{x, y}`）。
        let host = battler_host();
        let names = host.eval_string(
            r#"
            Object.getOwnPropertyNames(Game_BattlerBase.prototype).filter(function(k) {
                var d = Object.getOwnPropertyDescriptor(Game_BattlerBase.prototype, k);
                return d && typeof d.get === 'function';
            }).sort().join(',')
            "#,
        );
        assert_eq!(
            names,
            "agi,atk,cev,cnt,cri,def,eva,exr,fdr,grd,hit,hp,hrg,luk,mat,mcr,mdf,mdr,mev,mhp,mmp,mp,\
             mrf,mrg,pdr,pha,rec,tcr,tgr,tp,trg",
            "`Game_BattlerBase` 的属性访问器名单变了（实测 1.6.1 那一块共 **31** 格：\
             基础 11 格 + XPARAM 8 格 + SPARAM 12 格，而魔攻的名字是 `mat` 不是 `mag`）"
        );
        // 每一条都是 `configurable: true`（上游 `rpg_objects.js:2125` 那一族）⇒ 插件可以就地覆盖，
        // 而兼容层重复装载不抛（见 `reinstalling_leg_c_keeps_the_methods_chains_and_accessors`）。
        assert_eq!(
            host.eval_string(
                "String(Object.getOwnPropertyDescriptor(Game_BattlerBase.prototype, 'hp').configurable)"
            ),
            "true"
        );
        // 那颗**单数**写法的全仓唯一一条：`Game_Actor.prototype` 的 `level`。
        assert_eq!(
            host.eval_string(
                "String(Object.getOwnPropertyDescriptor(Game_Actor.prototype, 'level') !== undefined)"
            ),
            "true"
        );
        // 属性读的是当拍的状态：`recoverAll` 之后 `hp === mhp === param(0)`；
        // 而 `paramMin(1)`（MMP）是这一族里唯一"下限 0"的那一格（上游照落，不改良）。
        assert_eq!(num(&host, "$gameActors.actor(1).level"), "5");
        assert_eq!(num(&host, "$gameActors.actor(1).mhp"), "50");
        assert_eq!(num(&host, "$gameActors.actor(1).hp"), "50");
        assert_eq!(num(&host, "$gameActors.actor(1).param(0)"), "50");
        assert_eq!(num(&host, "$gameActors.actor(1).paramMin(1)"), "0");
        assert_eq!(num(&host, "$gameActors.actor(1).paramMin(0)"), "1");
    }

    #[test]
    fn the_actor_exp_curve_and_learnings_come_from_the_class_table() {
        // `Game_Actor.setup` 里 `this._level = actor.initialLevel`（fixture 5 级），
        // 升级那条腿的输入是职业表的 `expParams = [30, 20, 38, 50]`。
        let host = battler_host();
        assert_eq!(
            host.eval_string(
                "var a = $gameActors.actor(1); String(a.expForLevel(6) > a.expForLevel(5))"
            ),
            "true",
            "经验曲线不是单调增 ⇒ `expParams` 没读进职业表"
        );
        assert_eq!(num(&host, "$gameActors.actor(1).maxLevel()"), "99");
        assert_eq!(
            host.eval_string("String($gameActors.actor(1).isMaxLevel())"),
            "false"
        );
        // 职业表 `learnings` 的等级闸：`{skillId:1, level:1}` 学到了，`{skillId:2, level:99}` 没有。
        // （成员名是 `skills()`，而它交回的是 **`$dataSkills` 里的那些对象**，不是 id ——
        // id 那一串住在 `_skills`，上游插件两种都读。上游 `Game_Actor` **没有** `skillIds`，
        // 那一格是 VX 的，别照 VX 写判据。）
        assert_eq!(
            host.eval_string(
                "var a = $gameActors.actor(1); [a.isLearnedSkill(1), a.isLearnedSkill(2), a._skills.contains(1), a.skills().length, a.skills()[0] === $dataSkills[1]].join(',')"
            ),
            "true,false,true,1,true"
        );
        // 涨到下一级：`changeExp` 才是那条"会升级"的腿（`gainExp` 见下面那条实测）。
        assert_eq!(
            host.eval_string(
                "var a = new Game_Actor(1); a.changeExp(a.expForLevel(6), false); String(a.level)"
            ),
            "6",
            "`changeExp` 到 6 级那一档之后等级没动 ⇒ `levelUp`/`nextLevelExp` 那一条腿断了"
        );
        // 99 级那一格：越界喂经验不许把等级顶过 `maxLevel`（而 99 级把 `learnings` 的另一条学到）。
        assert_eq!(
            host.eval_string(
                "var a = new Game_Actor(1); a.changeExp(a.expForLevel(140), false); [String(a.level), String(a.isLearnedSkill(2))].join(',')"
            ),
            "99,true"
        );
        // **一条实测的形状，不是笔误**：`gainExp` 走
        // `finalExpRate = exr × (在战场 ? 1 : benchMembersExpRate())`，而
        // `benchMembersExpRate()` 是 `$dataSystem.optExtraExp ? 1 : 0`
        // （`rpg_objects.js:3935` 那一句）⇒ **开关关掉时，场外加经验是 0，等级一格都不动**。
        // 上游的这条闸照落，不许"顺手"让场外也涨。
        assert_eq!(
            host.eval_string(
                "var a = new Game_Actor(1); a.gainExp(a.expForLevel(6)); String(a.level)"
            ),
            "5",
            "场外的 `gainExp` 在 `optExtraExp` 关掉时必须不动等级"
        );
        // `gainExp` 加的是 `exp × finalExpRate`，所以喂进去的是**差值**才刚好落到 6 级
        // （喂"6 级那一档的累计经验"会一口气越过好几级 —— 上游的读数，不是笔误）。
        assert_eq!(
            host.eval_string(
                "var a = new Game_Actor(1); $dataSystem.optExtraExp = true; a.gainExp(a.expForLevel(6) - a.expForLevel(5)); String(a.level)"
            ),
            "6",
            "开关打开之后同一条腿要立刻走得动（上一条不是把腿焊死）"
        );
        assert_eq!(
            host.eval_string(
                "var a = new Game_Actor(1); $dataSystem.optExtraExp = true; a.gainExp(a.expForLevel(6)); String(a.level > 6)"
            ),
            "true",
            "把整档累计经验当增量喂进去 ⇒ 上游会连跳几级，这一格不许被「改良」成只跳一级"
        );
    }

    #[test]
    fn the_party_ledger_is_three_containers_and_clamps_at_ninety_nine() {
        // 旧移植侧 `src/party.rs` 把这套账写在引擎变量的保留段（金币 806、道具 1107.. 那一族），
        // 而 c 之后**上游自己有一份**：三张容器 + `numItems` + `clamp(0, maxItems)`。
        // 这一条钉的是"账落在哪三格里、夹逼在哪一拍生效"，M10.7 退役 `party.rs` 的输入就是它。
        let host = battler_host();
        let js = r#"
            var out = [];
            out.push($gameParty.itemContainer($dataItems[1]) === $gameParty._items);
            out.push($gameParty.itemContainer($dataWeapons[1]) === $gameParty._weapons);
            out.push($gameParty.itemContainer($dataArmors[2]) === $gameParty._armors);
            out.push($gameParty.maxItems($dataItems[1]) === 99);
            $gameParty.gainItem($dataWeapons[1], 3);
            out.push($gameParty.numItems($dataWeapons[1]) === 3);
            // 夹到 99：上游是 `newNumber.clamp(0, this.maxItems(item))`，多发的部分不留账。
            $gameParty.gainItem($dataWeapons[1], 200);
            out.push($gameParty.numItems($dataWeapons[1]) === 99);
            // 上游的签名是 `consumeItem(item)` —— **一个参数**，而且只对"道具类且 `consumable`
            // 为真"的那一格生效（体是 `if (DataManager.isItem(item) && item.consumable)
            // this.loseItem(item, 1)`）。VX 那种 `consumeItem(item, amount)` 不是 1.6.1 的形状，
            // 而武器/防具要扣账走的是 `loseItem`。
            $gameParty.gainItem($dataItems[1], 4);
            $gameParty.consumeItem($dataItems[1]);
            out.push($gameParty.numItems($dataItems[1]) === 3);
            $gameParty.loseItem($dataWeapons[1], 99);
            out.push($gameParty.numItems($dataWeapons[1]) === 0);
            // 扣到 0 那一格是**删键**，不是写 0（`gainItem` 里的 `if (container[id] === 0) delete …`）。
            out.push($gameParty._weapons[1] === undefined);
            // 金币那两格是同一支笔的两个方向（`loseGold` 不许把它扣成负数）。
            $gameParty.gainGold(500);
            out.push($gameParty.gold() === 500);
            $gameParty.loseGold(600);
            out.push($gameParty.gold() === 0);
            // `gainItem` 末行敲的是 `$gameMap.requestRefresh()` —— 模块头偏差 2 的另一半：
            // 现在 `$gameMap` 是实例，而它写的那颗旗叫 **`_needsRefresh`**
            // （b 那一轮的替身只能数敲了几次，落不到具体字段上）。
            $gameMap._needsRefresh = false;
            $gameParty.gainItem($dataItems[1], 1);
            out.push($gameMap._needsRefresh === true);
            out.join(',')
        "#;
        assert_eq!(
            host.eval_string(js),
            "true,true,true,true,true,true,true,true,true,true,true,true"
        );
    }

    #[test]
    fn the_troop_setup_letters_the_enemies_from_the_multi_line_table() {
        // `Game_Troop.LETTER_TABLE_FULL` 是**值跨行的数组**（切片器为它加了一条口径）。
        // 少它的表征不是抛在装载期，是 `letterTable()` 交出 `undefined` ⇒ `setup()` 里的
        // `makeUniqueNames` 在 `table.length` 上抛。这一条把"数组本体到了"落成读数。
        let host = battler_host();
        let js = r#"
            var out = [];
            // `$dataSystem.locale = 'ja_JP'` ⇒ 上游取的是全宽那一串（`Utils.isJapanese()`）。
            out.push($gameTroop.letterTable().length === 26);
            out.push($gameTroop.letterTable()[0] === 'Ａ');
            $gameTroop.setup(1);
            out.push($gameTroop.members().length === 2);
            out.push($gameTroop.members()[0] instanceof Game_Enemy);
            // 排完字母之后 `name()` 交回的是**带字母的那一串**（'E1Ａ'），`originalName()` 才是表里那个。
            out.push($gameTroop.members()[0].name() === 'E1Ａ');
            out.push($gameTroop.members()[0].originalName() === 'E1');
            // 两个同名 ⇒ 上游给它们排字母，并标成"复数"（字段是 `_plural`，没有 `isPlural` 那一格）。
            out.push($gameTroop.members()[0]._letter === 'Ａ');
            out.push($gameTroop.members()[1]._letter === 'Ｂ');
            out.push($gameTroop.members()[0]._plural === true);
            // `Game_Enemy.paramBase` 读的是 `enemy().params[paramId]`（**不是**按等级取的数组）。
            out.push($gameTroop.members()[0].mhp === 300);
            // **`expTotal()` 数的是 `deadMembers()`**（打完才结算，上游那一句就是这一格）：
            // 建出来的那一拍它是 0，不是"两只怪的经验之和"。
            out.push($gameTroop.expTotal() === 0);
            $gameTroop.members()[0].addState(1);
            out.push($gameTroop.expTotal() === 120);
            out.join(',')
        "#;
        assert_eq!(
            host.eval_string(js),
            "true,true,true,true,true,true,true,true,true,true,true,true"
        );
    }

    #[test]
    fn the_game_item_kinds_and_the_action_read_their_tables() {
        // `Game_Item` 是 c 那一族的最小一格：角色装备、`Game_Action` 的技能、队伍账本
        // 全经由它把"数据对象"折成**两个字段**。1.6.1 的形状是 `_dataClass`（字符串
        // `''`/`'skill'`/`'item'`/`'weapon'`/`'armor'`）+ `_itemId` —— **不是** VX 那一种
        // `_data = {id, kind}`，判据照 1.6.1 写（`Game_Item.prototype.isNull` 也只在 1.6.1 有）。
        let host = battler_host();
        let js = r#"
            var out = [];
            var w = new Game_Item();
            w.setEquip(true, 1);
            out.push(w.isWeapon() && w._dataClass === 'weapon' && w._itemId === 1);
            out.push(w.object() === $dataWeapons[1]);
            var ar = new Game_Item();
            ar.setEquip(false, 2);
            out.push(ar.isArmor() && ar.object() === $dataArmors[2]);
            var it = new Game_Item();
            it.setObject($dataItems[1]);
            out.push(it.isItem() && it.itemId() === 1 && it.object() === $dataItems[1]);
            // 空格：不带参数的 `new Game_Item()` 才是"没有"（`setEquip(false, 0)` 落到
            // `armor` + id 0，`object()` 交出 `$dataArmors[0]` 即 null —— 两种"空"不是一回事）。
            var blank = new Game_Item();
            out.push(blank.isNull() && blank.object() === null);
            out.push(ar.itemId() === 2 && !blank.isEquipItem());
            // `Game_Action` 的常量表（17 条 `EFFECT_*`/`HITTYPE_*`）随类一起交到了。
            out.push(Game_Action.EFFECT_RECOVER_HP === 11);
            out.push(Game_Action.EFFECT_ADD_STATE === 21);
            out.push(Game_Action.EFFECT_COMMON_EVENT === 44);
            out.push(Game_Action.HITTYPE_PHYSICAL === 1);
            // 主体 + 技能：`item()` 取回的是 `$dataSkills` 里那**一颗对象**（不是 id）。
            $gameTroop.setup(1);
            var actor = $gameActors.actor(1);
            var ev = new Game_Action(actor);
            ev.setSkill(1, false);
            out.push(ev.isSkill() && ev.item() === $dataSkills[1]);
            out.push(ev.subject() === actor);
            // 射程 1（对单一敌人）⇒ `makeTargets` 从 `$gameTroop` 里挑一个活的。
            out.push(ev.makeTargets().length === 1);
            out.push(ev.makeTargets()[0] instanceof Game_Enemy);
            out.join(',')
        "#;
        assert_eq!(
            host.eval_string(js),
            "true,true,true,true,true,true,true,true,true,true,true,true,true,true"
        );
    }

    #[test]
    fn the_battler_states_buffs_and_costs_answer_like_upstream() {
        // 状态那一条腿钉三格：`deathStateId()` 是 1（`isDead()` 的输入）、`addState` 过
        // `traitObjects` 的抗性闸、`removeState` 之后 `states()` 空。
        // buff 那一条钉的是 `paramBuffRate`（`_buffs[n] * 0.25 + 1`）—— fixture 的攻击是 50：
        // 一层 buff ⇒ `50 * 1.25 = 62.5` → `Math.round` = **63**，一层 debuff ⇒ `50 * 0.75 = 37.5` → **38**。
        // 那 0.5 的不对称是 JS `Math.round` 的（向上取半），照落不改 —— 它是"buff 折在 traits 之后"的读数。
        let host = battler_host();
        let js = r#"
            var a = $gameActors.actor(1);
            var out = [];
            out.push(a.deathStateId() === 1);
            a.addState(2);
            out.push(a.isStateAffected(2) && a.states().length === 1);
            a.removeState(2);
            out.push(a.states().length === 0);
            a.addState(1);
            out.push(a.isDead());
            a.removeState(1);
            out.push(!a.isDead() && a.isAlive());
            // 上游 1.6.1 的签名是 `addBuff(paramId, turns)`（**没有** VX 那第三个 maxStack 参数），
            // 而它一次只加**一层**（`increaseBuff`），`turns` 只写进 `_buffTurns`。
            a.addBuff(2, 3);
            out.push(a._buffs[2] === 1);
            out.push(a._buffTurns[2] === 3);
            out.push(a.param(2) === 63);
            a.decreaseBuff(2);
            out.push(a._buffs[2] === 0 && a.param(2) === 50);
            a.addDebuff(2, 2);
            out.push(a._buffs[2] === -1 && a.param(2) === 38);
            a.removeAllBuffs();
            out.push(a._buffs[2] === 0 && a._buffTurns[2] === 0 && a.param(2) === 50);
            // 技能价：`mpCost` 从 `_mp` 扣，而 `canPaySkillCost` 查的是"够不够"。
            a.setMp(0);
            out.push(!a.canPaySkillCost($dataSkills[2]));
            a.setMp(20);
            a.paySkillCost($dataSkills[2]);
            out.push(a.mp === 15);
            out.join(',')
        "#;
        assert_eq!(
            host.eval_string(js),
            "true,true,true,true,true,true,true,true,true,true,true,true,true"
        );
    }

    #[test]
    fn the_battler_reaches_the_undelivered_managers_and_says_who() {
        // `leg-deps.py` 量出 c 的外部面只有四个名字，其中两格**故意不交**：
        // `SoundManager.play*` 12 处（底下是 `AudioManager` ⇒ M10.4）与 `BattleManager` 3 句
        // （战斗是定案的非目标）。这一条把"没交"落成读数：形状在、成员没有 ⇒
        // 撞到那一格**抛**，不许静默跳过。钉的是状态落点（结果已经写好、抛在喊话那一句），
        // 不是异常句子 —— 两条腿的句子不一样（QuickJS 只说 "not a function"）。
        let host = battler_host();
        assert_eq!(
            host.eval_string("String(typeof SoundManager)"),
            "function",
            "`SoundManager` 的**形状**在（`shapes` 的静态类那一档），成员没交"
        );
        assert_eq!(
            host.eval_string("String(typeof SoundManager.playRecovery)"),
            "undefined"
        );
        assert_eq!(
            host.eval_string("String(typeof BattleManager.canEscape)"),
            "undefined"
        );
        // `Game_Battler.applyResult` 的 `result.recovered` 那一支喊 `playRecovery`
        // （`rpg_objects.js:3362`）：先把结果写成型，再问这一句抛不抛。
        // **跨 eval 的名字要显式挂 `globalThis`**：`js_sys::eval` 不把 `var` 漏成全局，而 QuickJS 漏
        // （M10.1 记过的不对称）—— 只写 `var a` 的话 native 腿过、wasm 腿报 "a is not defined"，
        // 这一格实测抓到过一次。
        host.eval(
            "globalThis.a = $gameActors.actor(1); a.clearResult(); a._result.recovered = true;",
        )
        .expect("结果写不进去");
        let err = host
            .eval("a.applyResult();")
            .expect_err("`SoundManager.playRecovery` 没交，这一格必须抛而不是悄悄跳过");
        assert!(!err.is_empty(), "抛了但没有句子：读数说不出是谁没交");
        assert_eq!(
            host.eval_string("String(a._result.recovered)"),
            "true",
            "抛在喊话那一句，而结果本身已经落好（下一轮接上 `SoundManager` 时这一格不用改）"
        );
        // `Game_Battler.onTurnEnd` **无条件**读 `BattleManager.isForcedTurn()`
        // （`rpg_objects.js:3276-3282`）⇒ 同一格红，而且抛之前 `clearResult`/`regenerateAll`
        // 已经跑过（钉的是状态落点，不是句子）。
        host.eval("globalThis.a = $gameActors.actor(1); a.onTurnEnd();")
            .expect_err("`BattleManager.isForcedTurn` 没交，这一格必须抛而不是悄悄跳过");
        // **短路的另一侧**也要钉：`Game_Actor.meetsUsableItemConditions` 里那一句是
        // `if ($gameParty.inBattle() && !BattleManager.canEscape() && …)`（`:4275`），
        // 在场外根本走不到 `BattleManager` ⇒ 它照样交回一个布尔，不抛。这才叫"洞有边界"。
        assert_eq!(
            host.eval_string("String($gameParty.inBattle())"),
            "false",
            "上面那条短路的前提：这一格没进战场"
        );
        assert_eq!(
            host.eval_string(
                "String(typeof $gameActors.actor(1).meetsUsableItemConditions($dataSkills[1]))"
            ),
            "boolean"
        );
        // c 的**不碰这两格**的路径照样跑得通：
        assert_eq!(num(&host, "new Game_Actor(2).mhp"), "50");
        assert_eq!(
            host.eval_string("String($gameTroop.members().length)"),
            "0",
            "`$gameTroop` 这时候还是空的（`setup` 是战斗那侧的调用者，非目标）"
        );
    }

    #[test]
    fn reinstalling_leg_c_keeps_the_methods_chains_and_accessors() {
        // 共享 realm 的那条老规矩（`../sisf_pray/M10.md`「这一族的规矩」）：c 那两段里**有**
        // `Object.defineProperties(Game_BattlerBase.prototype, …)` 与那颗单数 `level`。
        // 上游每条描述符都写了 `configurable: true` ⇒ 第二次装是"核心层又声明一遍"，
        // 不抛 `Cannot redefine property`。这一条钉的就是那一格，别为了让测试过而改描述符。
        let host = battler_host();
        install(&host).expect("c 第二次装不上（幂等）");
        let js = r#"
            var out = [];
            out.push(typeof Game_BattlerBase.prototype.param === 'function');
            out.push(typeof Game_Actor.prototype.setup === 'function');
            out.push(typeof Game_Party.prototype.gainItem === 'function');
            out.push(Object.getPrototypeOf(Game_Actor.prototype) === Game_Battler.prototype);
            out.push(Object.getPrototypeOf(Game_Troop.prototype) === Game_Unit.prototype);
            out.push(Game_BattlerBase.TRAIT_PARAM === 21);
            out.push(Game_BattlerBase.ICON_DEBUFF_START === 48);
            out.push(Game_Troop.LETTER_TABLE_FULL.length === 26);
            out.push(Object.getOwnPropertyDescriptor(Game_BattlerBase.prototype, 'hp') !== undefined);
            // 十三颗名字被 `GAME_GLOBALS_JS` 抹回 null 是**上游的样子**（模块头偏差 1），
            // 重新建一遍之后照样能用（幂等的那一半）。
            out.push($gameParty === null);
            out.join(',')
        "#;
        assert_eq!(
            host.eval_string(js),
            "true,true,true,true,true,true,true,true,true,true"
        );
        host.eval(crate::managers::CREATE_GAME_OBJECTS_JS)
            .expect("createGameObjects 又装一遍（幂等的那一格）");
        host.eval("DataManager.createGameObjects(); $gameParty.gainItem($dataItems[1], 2);")
            .expect("重装之后建不出实例、或写不进账本");
        assert_eq!(
            host.eval_string("String($gameParty.numItems($dataItems[1]))"),
            "2"
        );
    }

    // ==== d 那一腿：`Game_Interpreter`（解释器本身） ====

    /// d 的那颗宿主：b 那张四格地图的 fixture 数据 + c 的十三颗**真实例**。
    ///
    /// 顺序是有原因的：[`build_map`] 末行把 `$gamePlayer` 换成 `{x: 3, y: 3}` 那一份**属性替身**
    /// （b 那一轮真实例建不出来，见 `the_player_still_needs_the_party`），而 d 的每一条指令写的
    /// 就是真实例 —— 所以这里在 fixture 摆好之后**再跑一次 `createGameObjects`** 把替身覆盖掉，
    /// 然后 `$gameMap.setup(1)` 让新造的那颗 `Game_Map` 读同一份 `$dataMap`。
    fn interpreter_host() -> Host {
        let host = booted();
        build_map(&host);
        host.eval("DataManager.createGameObjects(); $gameMap.setup(1);")
            .expect("d 的宿主建不出十三颗真实例（替身没被覆盖掉）");
        host
    }

    /// 起一台解释器并走 `steps` 拍（`update()` 一次 = 上游的一拍）。
    fn start(host: &Host, list_js: &str, event_id: i32) {
        host.eval(
            &(String::from("globalThis.__list = ")
                + list_js
                + &format!(
                    "; globalThis.__it = new Game_Interpreter(); __it.setup(__list, {event_id});"
                )),
        )
        .expect("解释器起不来（这一格的读数说不出是谁没交）");
    }

    fn step(host: &Host, times: usize) {
        for _ in 0..times {
            host.eval("__it.update();").expect("update 抛了");
        }
    }

    /// 一条列表走完到底：写两张表、没有 handler 的码原样走过去、终止符把 `_list` 抹成 null。
    ///
    /// `404`（选项块的收尾）在 1.6.1 **没有 handler**（`command404` 不存在，实测上游只有
    /// 402/403/411/413/601/602/603 那一族 4xx/6xx），而 `executeCommand` 是
    /// `if (typeof this[methodName] === 'function')` —— 所以它是"**原样走过去**"，
    /// 不是"降级"。这一格与移植侧 `degrade.rs` 对 `604`/`655` 的判词同一把尺子。
    #[test]
    fn the_interpreter_walks_a_list_to_the_terminator_and_writes_both_tables() {
        let host = interpreter_host();
        start(
            &host,
            "[{code: 121, indent: 0, parameters: [3, 3, 0]},
              {code: 122, indent: 0, parameters: [5, 5, 0, 0, 7]},
              {code: 404, indent: 0, parameters: []},
              {code: 0,   indent: 0, parameters: []}]",
            0,
        );
        step(&host, 1);
        let out = num(
            &host,
            "[__it.isRunning(), $gameSwitches.value(3), $gameVariables.value(5),
              __it._index, String(__it._list)].join(',')",
        );
        // `_index` 停在 4 = 列表长度：`currentCommand()` 交出 undefined ⇒ `terminate()`。
        assert_eq!(out, "false,true,7,4,null");
        assert_eq!(num(&host, "String(__it._comments)"), "");
    }

    /// `101` 的等待腿：**设下旗的那一拍就断环**，而台词是"紧随的同缩进 401"那几条。
    ///
    /// 上游的 `command101` 见 `$gameMessage.isBusy()` 就已经在播下一条 ⇒ 返回 false 不动索引
    /// （这就是移植侧 `say_hold` 那一条只有 1 拍的 `Wait` 子程序的来处，`AGENTS.md` 咬人第 15 条⑵）；
    /// 而它把 `_index` 推到 401 串之后的下一条，所以清掉消息之后**下一条指令接着跑**，
    /// 台词本身一句都不少。
    #[test]
    fn command101_holds_the_parent_until_the_message_is_cleared() {
        let host = interpreter_host();
        start(
            &host,
            "[{code: 101, indent: 0, parameters: ['', 0, 0, 0]},
              {code: 401, indent: 0, parameters: ['第一句']},
              {code: 401, indent: 0, parameters: ['第二句']},
              {code: 122, indent: 0, parameters: [6, 6, 0, 0, 1]},
              {code: 0,   indent: 0, parameters: []}]",
            0,
        );
        step(&host, 1);
        assert_eq!(
            num(
                &host,
                "[String($gameMessage.isBusy()), $gameMessage._texts.length,
                  __it._waitMode, __it._index, $gameVariables.value(6)].join(',')"
            ),
            "true,2,message,3,0",
            "台词的两条 401 必须都进消息，而 122 一条都不能跑（这就是等待腿）"
        );
        step(&host, 3);
        assert_eq!(
            num(
                &host,
                "[$gameMessage._texts.length, $gameVariables.value(6)].join(',')"
            ),
            "2,0",
            "消息没清 ⇒ 再多拍也不许放行（`isBusy()` 那一格是闸）"
        );
        host.eval("$gameMessage.clear();")
            .expect("消息清不掉（`Game_Message.clear` 是 a 那一族的格）");
        step(&host, 1);
        assert_eq!(
            num(&host, "[__it._waitMode, $gameVariables.value(6)].join(',')"),
            ",1"
        );
    }

    /// `117` 起一台**子解释器**跑公共事件，父的等待腿是 `_childInterpreter` 本身。
    ///
    /// `setupChild` 传的是 `_depth + 1`（上游那条 `checkOverflow` 在 `initialize` 里查
    /// `_depth >= 100` 抛 `'Common event calls exceeded the limit'` —— 递归的闸在这一格，
    /// 不在移植侧的帧栈上）；子跑完那一拍父在**同一次 `update()`** 里继续往下走。
    #[test]
    fn command117_runs_a_child_interpreter_and_the_parent_continues_after_it() {
        let host = interpreter_host();
        host.eval(
            r#"
            $dataCommonEvents[3] = {id: 3, name: 'CE3', trigger: 0, switchId: 1, list: [
                {code: 121, indent: 0, parameters: [4, 4, 0]},
                {code: 122, indent: 0, parameters: [9, 9, 0, 0, 10]},
                {code: 0,   indent: 0, parameters: []}]};
            "#,
        )
        .expect("公共事件的 fixture 摆不上");
        start(
            &host,
            "[{code: 117, indent: 0, parameters: [3]},
              {code: 122, indent: 0, parameters: [7, 7, 0, 0, 2]},
              {code: 0,   indent: 0, parameters: []}]",
            0,
        );
        step(&host, 1);
        assert_eq!(
            num(
                &host,
                "[String(__it._childInterpreter), String($gameSwitches.value(4)),
                  $gameVariables.value(9), $gameVariables.value(7)].join(',')"
            ),
            "null,true,10,2",
            "子解释器跑完必须自己摘掉，而 CE 写的开关/变量与父的下一条落在同一拍"
        );
        // 子那一台的深度 = 父 + 1（`AGENTS.md` 咬人第 5 条那条"独立开关归属调用方"的另一半：
        // 上游的递归深度就是靠这一格管住的）。
        host.eval("globalThis.__p = new Game_Interpreter(4); __p.setup(__list, 0); __p.setupChild(__list, 1);")
            .expect("父那一台的子解释器起不来");
        assert_eq!(num(&host, "__p._childInterpreter._depth"), "5");
        // 递归的闸：`_depth` 到 100 是**抛**，不是静默截断。
        let err = host
            .eval("new Game_Interpreter(100);")
            .expect_err("`checkOverflow` 那道 100 层的闸没落下去");
        assert!(
            err.contains("Common event calls exceeded the limit"),
            "抛了但不是上游那一句：{err}"
        );
    }

    /// `111` 的分支：条件写在 `_branch[_indent]`，假的话**整块体被跳过**，`411` 才走另一支。
    ///
    /// 上游 1.6.1 的 IF 块**没有 402 那一行**（实测真数据：`111` 之后直接是同缩进 +1 的体，
    /// `else` 是 `411`，收尾是 `412` 而没有 handler），而 402/403 属于选项那一族 ——
    /// 第一版照"IF 也该有 402"写就红在这里（`command402` 比的是 `_branch !== params[0]`，
    /// 布尔 `true` 与 `0` 在严格相等下不等）。
    #[test]
    fn command111_takes_one_arm_and_skips_the_body_of_the_other() {
        let host = interpreter_host();
        let list = "[{code: 111, indent: 0, parameters: [0, 3, 0]},
                     {code: 122, indent: 1, parameters: [8, 8, 0, 0, 1]},
                     {code: 411, indent: 0, parameters: []},
                     {code: 122, indent: 1, parameters: [8, 8, 0, 0, 2]},
                     {code: 412, indent: 0, parameters: []},
                     {code: 122, indent: 0, parameters: [10, 10, 0, 0, 1]},
                     {code: 0,   indent: 0, parameters: []}]";
        // 开关 3 = OFF ⇒ 走 `411` 那一支。
        start(&host, list, 0);
        step(&host, 1);
        assert_eq!(
            num(
                &host,
                "[$gameVariables.value(8), $gameVariables.value(10)].join(',')"
            ),
            "2,1",
            "条件不成立时必须走 else 那一支"
        );
        // 同一份列表，开关 ON ⇒ 走体不走 else。
        host.eval("$gameVariables.setValue(8, 0); $gameSwitches.setValue(3, true);")
            .expect("换条件没换上去");
        start(&host, list, 0);
        step(&host, 1);
        assert_eq!(
            num(
                &host,
                "[$gameVariables.value(8), $gameVariables.value(10)].join(',')"
            ),
            "1,1"
        );
        // `111` 类型 2（独立开关）的键是 `[mapId, eventId, 字母]` 三元串 ⇒ `eventId == 0` 时
        // 上游那一格是 `if (this._eventId > 0)` 直接不算（不是"算成 false"）。
        host.eval(
            "globalThis.__l2 = [{code: 111, indent: 0, parameters: [2, 'A', 0]},\n                                 {code: 122, indent: 1, parameters: [11, 11, 0, 0, 1]},\n                                 {code: 0,   indent: 0, parameters: []}];\n              globalThis.__it2 = new Game_Interpreter(); __it2.setup(__l2, 0); __it2.update();",
        )
        .expect("独立开关那一条列表起不来");
        let value = num(&host, "$gameVariables.value(11)");
        assert_eq!(
            value, "0",
            "独立开关条件在 eventId 0 时是**不成立**（`result` 停在 false ⇒ 体被跳过），不是跳过比较"
        );
    }

    /// 选项那一族：`102` 把选择回调写进 `$gameMessage`，**答案落下来才放行**。
    #[test]
    fn the_choice_callback_lands_on_the_matching_arm_and_not_before() {
        let host = interpreter_host();
        start(
            &host,
            "[{code: 102, indent: 0, parameters: [[\"要去\", '不去'], 2, 0]},
              {code: 402, indent: 0, parameters: [0]},
              {code: 122, indent: 1, parameters: [12, 12, 0, 0, 7]},
              {code: 402, indent: 0, parameters: [1]},
              {code: 122, indent: 1, parameters: [12, 12, 0, 0, 8]},
              {code: 404, indent: 0, parameters: []},
              {code: 0,   indent: 0, parameters: []}]",
            0,
        );
        step(&host, 1);
        assert_eq!(
            num(
                &host,
                "[String($gameMessage.isChoice()), $gameMessage._choices.length,
                         __it._waitMode, $gameVariables.value(12)].join(',')"
            ),
            "true,2,message,0",
            "选项没答之前必须停在这里"
        );
        host.eval("$gameMessage.onChoice(1); $gameMessage.clear();")
            .expect("选第二项没落进去");
        step(&host, 1);
        assert_eq!(
            num(
                &host,
                "[$gameVariables.value(12), String(__it._branch[0])].join(',')"
            ),
            "8,1",
            "回调写的 `_branch` 必须落在 `402` 读的那一格上（这一对是上游的形状）"
        );
    }

    /// `201`：登记换图 + 设下 `'transfer'` 等待腿，而**落地之前同页一条都不走**。
    ///
    /// 这一条是 `AGENTS.md` 咬人第 34 条⑷在上游那一侧的样子：移植侧要用 `transfer_frozen()`
    /// 冻住旧页，而上游只有 `setWaitMode('transfer')` + `$gamePlayer.isTransferring()` 这一对。
    #[test]
    fn command201_registers_the_transfer_and_holds_until_the_player_arrives() {
        let host = interpreter_host();
        start(
            &host,
            "[{code: 201, indent: 0, parameters: [0, 5, 12, 8, 0, 2]},
              {code: 122, indent: 0, parameters: [13, 13, 0, 0, 1]},
              {code: 0,   indent: 0, parameters: []}]",
            0,
        );
        step(&host, 1);
        assert_eq!(
            num(
                &host,
                "[String($gamePlayer.isTransferring()), $gamePlayer._newMapId,
                  $gamePlayer._newX, $gamePlayer._newY, $gamePlayer._fadeType,
                  __it._waitMode, $gameVariables.value(13)].join(',')"
            ),
            "true,5,12,8,2,transfer,0",
            "第 6 格是 fadeType（不是等待旗），而旧页的下一条必须还没跑"
        );
        // 落地（上游由 `Scene_Map` 调 `performTransfer`）之后同一拍放行。
        host.eval("$gamePlayer.performTransfer();")
            .expect("落地走不了");
        step(&host, 1);
        assert_eq!(
            num(
                &host,
                "[$gameVariables.value(13), __it._waitMode].join(',')"
            ),
            "1,"
        );
    }

    /// 热循环被 `SceneManager.isSceneChanging()` 打断：那一拍**一条指令都不执行**。
    ///
    /// 这是 d 交那一格的**理由**（`managers::SCENE_MANAGER_SWITCH_JS`），也是移植侧那 24 拍
    /// 冻结的上游原型（`AGENTS.md` 咬人第 34 条⑷：并行页与自动页共用同一台，所以一起停）。
    #[test]
    fn the_hot_loop_breaks_before_the_first_command_when_the_scene_is_changing() {
        let host = interpreter_host();
        start(
            &host,
            "[{code: 121, indent: 0, parameters: [3, 3, 0]},
              {code: 121, indent: 0, parameters: [4, 4, 0]},
              {code: 0,   indent: 0, parameters: []}]",
            0,
        );
        host.eval("globalThis.__next = { name: 'Scene_Map' }; SceneManager._nextScene = __next;")
            .expect("场景栈那一格摆不上");
        step(&host, 1);
        assert_eq!(
            num(
                &host,
                "[String($gameSwitches.value(3)), __it._index].join(',')"
            ),
            "false,0",
            "`isSceneChanging()` 挡住的那一拍应当一条都不走、索引也不动"
        );
        host.eval("SceneManager._nextScene = null;")
            .expect("收尾写不回去");
        step(&host, 1);
        assert_eq!(
            num(
                &host,
                "[String($gameSwitches.value(3)), String($gameSwitches.value(4))].join(',')"
            ),
            "true,true"
        );
    }

    /// `205` 的等待旗在路线**自己**那一格：`wait` 为真才停，否则同页继续。
    #[test]
    fn command205_waits_only_when_the_route_itself_says_wait() {
        let host = interpreter_host();
        let route = |wait: &str| -> String {
            let head = "[{code: 205, indent: 0, parameters: [-1, {list: [], repeat: false, \
                         skipping: true, wait: ";
            let tail = "}]},\n              {code: 122, indent: 0, parameters: [14, 14, 0, 0, 1]},\n \
                        {code: 0, indent: 0, parameters: []}]";
            String::from(head) + wait + tail
        };
        start(&host, &route("true"), 0);
        step(&host, 1);
        assert_eq!(
            num(
                &host,
                "[__it._waitMode, $gameVariables.value(14)].join(',')"
            ),
            "route,0",
            "`wait: true` 的路线要把同页的后面那条挡住"
        );
        // 等待腿解开走上游那一句 `setMoveRoute`（它把 `_moveRouteForcing` 写成 **false**）——
        // 第一版用 `forceMoveRoute` 解不开，因为那一句只会**再立一次旗**（`objects_characters.js:713`）。
        host.eval(
            "$gamePlayer.setMoveRoute({list: [], repeat: false, skipping: true, wait: false});",
        )
        .expect("路线解不开");
        step(&host, 1);
        assert_eq!(num(&host, "String($gameVariables.value(14))"), "1");
        // 反面：`wait: false` ⇒ 设下路线**不等**，同拍就走下一条。
        host.eval("$gameVariables.setValue(14, 0);")
            .expect("复位没写成");
        start(&host, &route("false"), 0);
        step(&host, 1);
        assert_eq!(
            num(
                &host,
                "[__it._waitMode, $gameVariables.value(14)].join(',')"
            ),
            ",1"
        );
    }

    /// `setup` 把整张列表（含沿 `117` 递归的公共事件）要的那十四种图**报一遍名**。
    ///
    /// 这里钉的是**路径拼写**（`requestBitmap` 的 `folder + encodeURIComponent(name) + '.png'`），
    /// 不是"取到位图"：移植侧的预取泵用的就是同一串（`AGENTS.md` 咬人第 32 条⑶），
    /// 名字对不上就是浏览器腿"屏幕上没有立绘而且一声不吭"那一格。
    /// `commonList` 那一格是递归的**去重**闸：CE 调自己不许无限展开。
    #[test]
    fn setup_reports_the_images_of_the_whole_list_including_the_common_events() {
        let host = interpreter_host();
        host.eval(
            r#"
            globalThis.__asked = [];
            ImageManager._sisfHandle = function (url) {
                if (url) __asked.push(url);
                return { url: url, smooth: false, width: 0, height: 0,
                         isReady: function () { return true; } };
            };
            $dataCommonEvents[5] = {id: 5, name: 'CE5', trigger: 0, switchId: 1, list: [
                {code: 101, indent: 0, parameters: ['CEface', 0, 0, 0]},
                {code: 117, indent: 0, parameters: [5]},
                {code: 0,   indent: 0, parameters: []}]};
            $dataTilesets[2] = {id: 2, mode: 1, tilesetNames: ['Outside_A1', 'Outside_B']};
            "#,
        )
        .expect("样品与记数把手摆不上");
        start(
            &host,
            "[{code: 101, indent: 0, parameters: ['TCGAlcybase', 1, 0, 0]},
              {code: 401, indent: 0, parameters: ['一句']},
              {code: 117, indent: 0, parameters: [5]},
              {code: 231, indent: 0, parameters: [1, 'pic one', 0, 0, 0, 0, 0, 100, 0]},
              {code: 282, indent: 0, parameters: [2, 0]},
              {code: 0,   indent: 0, parameters: []}]",
            0,
        );
        assert_eq!(
            num(&host, "__asked.join('|')"),
            "img/faces/TCGAlcybase.png|img/faces/CEface.png|img/pictures/pic%20one.png|\
             img/tilesets/Outside_A1.png|img/tilesets/Outside_B.png",
            "沿 117 的那一格（CE 自己的头像）与 282 的两张图块名都必须报出来"
        );
    }

    /// `301` 走到**没交的那两格**上：抛，且句子点名是谁没交（`SoundManager` 同一条腿）。
    #[test]
    fn command301_says_which_hole_it_reached() {
        let host = interpreter_host();
        // 有兵：`BattleManager.setup` 那一格先炸（`BattleManager` 只有形状，M10 的非目标）。
        start(
            &host,
            "[{code: 301, indent: 0, parameters: [0, 1, 0, 0]}, {code: 0, indent: 0, parameters: []}]",
            0,
        );
        let err = host
            .eval("__it.update();")
            .expect_err("`BattleManager.setup` 没交，这一格必须抛");
        // QuickJS 的异常句子**不带属性名**（只有 "not a function"，node 那条才带 —— M10.1 的老账），
        // 所以名字由这两格 typeof 自己钉，句子只钉"抛了而且不是空的"。
        assert!(!err.is_empty(), "抛了但没有句子：读数说不出是谁没交");
        assert_eq!(
            num(
                &host,
                "[typeof BattleManager.setup, typeof SceneManager.push, typeof $dataTroops[1]].join(',')"
            ),
            "undefined,undefined,object",
            "洞的两个名字必须点名，而走到洞之前 `$dataTroops[troopId]` 那一格必须已经查到（查不到就整块跳过）。\n             那颗裸引用的 `Scene_Battle` 在这颗 crate 的宿主里也没有（153 条 shapes 由移植侧从上游读，\n             见 `sisf_pray/src/compat.rs`），而 `SceneManager.push` 先抛 ⇒ 参数那一句根本读不到。"
        );
        // 没兵（`$dataTroops[troopId]` 落空）⇒ 上游整块跳过，一声不响也不抛。
        host.eval("$dataTroops[77] = null;")
            .expect("把不存在的队伍那一格抹掉");
        start(
            &host,
            "[{code: 301, indent: 0, parameters: [0, 77, 0, 0]}, {code: 0, indent: 0, parameters: []}]",
            0,
        );
        host.eval("__it.update();")
            .expect("队伍不存在时那一格不该走到没交的地方");
        assert!(!host.eval_bool("__it.isRunning()"));
    }

    /// `119`/`118` 的标签跳转 + `checkFreeze` 那道十万条的保险。
    ///
    /// 上游对**死循环**的态度是"同一拍里跑满 100,000 条就 break"（`Graphics.frameCount` 变了才
    /// 重新计数），而移植侧的对应物是帧栈 `MAX_FRAMES = 32` 到了只 warn（`AGENTS.md` 咬人第 5 条）。
    /// 列表的形状是**实测**出来的（不是照移植侧的帧栈想的）：`118` 标签在前，跳回的 `119` 在后，
    /// 而 `command119` 找到标签之后是 `this.jumpTo(i); return;` —— **没有返回值**，
    /// 于是 `executeCommand` 那句 `if (!this[methodName]()) return false;` 当场把这一拍断掉，
    /// `_index` 停在标签那一格（`jumpTo` 写的是 `_index = index`，不是 `index - 1`）。
    /// 所以"标签循环"在上游是**每拍一次**，十万条那道保险给的是"同拍不跳标签的死循环"
    /// （`111`+`402` 那一族原地自跳），两者不是同一条腿。移植侧的对应物是帧栈
    /// `MAX_FRAMES = 32` 到了只 warn（`AGENTS.md` 咬人第 5 条）。
    #[test]
    fn the_label_jump_spends_one_iteration_per_beat_and_keeps_running() {
        let host = interpreter_host();
        start(
            &host,
            "[{code: 118, indent: 0, parameters: ['仍然这么选']},
              {code: 122, indent: 0, parameters: [15, 15, 1, 0, 1]},
              {code: 119, indent: 0, parameters: ['仍然这么选']},
              {code: 0,   indent: 0, parameters: []}]",
            0,
        );
        step(&host, 1);
        assert_eq!(
            num(
                &host,
                "[String(__it.isRunning()), __it._index, $gameVariables.value(15), __it._freezeChecker].join(',')"
            ),
            "true,0,1,2",
            "跳完标签必须停在**标签那一格**、这一拍只走一圈（`119` 那条 `return;` 让 `checkFreeze` 少跑一次）"
        );
        step(&host, 4);
        assert_eq!(
            num(
                &host,
                "[$gameVariables.value(15), String(__it.isRunning()), __it._freezeChecker].join(',')"
            ),
            "5,true,10",
            "每拍一圈（再多五拍就是五次加一），而 `_freezeChecker` 在帧计数不动时跨拍累计（2×5）"
        );
    }

    /// d 的第二档：重装 = 核心层又声明一遍，方法、静态把手与等待旗都还在。
    #[test]
    fn reinstalling_leg_d_keeps_the_methods_and_the_static_helper() {
        let host = interpreter_host();
        host.eval(GAME_INTERPRETER_JS)
            .expect("d 的文本第二次装不上（上游那一份的描述符是 configurable）");
        let js = r#"
var out = [];
out.push(typeof Game_Interpreter.prototype.command117);
out.push(typeof Game_Interpreter.prototype.command201);
out.push(typeof Game_Interpreter.requestImages);
out.push(typeof Game_Interpreter.prototype.executeCommand);
out.push(String($gameMessage.isBusy()));
out.join(',')
"#;
        assert_eq!(
            host.eval_string(js),
            "function,function,function,function,false"
        );
    }
}
