//! M10.5 的**场景第三段：`rpg_windows.js` 真流闭包里的那十二颗**（窗那一族）。
//!
//! 名单与体量是查出来的（`../sisf_pray/tools/probes/core-touch.py --closure --no-push` 的
//! `rpg_windows.js` 那一行：**闭包里 12/46 颗 / 356 条** —— 那 356 = 切片的 **344 条方法 +
//! 12 句 `constructor` 补句**，与 sprites 那一格的 `155 = 145 + 10`、scenes 那一格的 `93 = 89 + 4`
//! 同一条口径）。文本由
//! `../sisf_pray/tools/probes/objects-slice.py --file rpg_windows.js --classes Window_Base,
//! Window_Selectable,Window_Command,Window_Message,Window_ChoiceList,Window_NumberInput,
//! Window_EventItem,Window_ItemList,Window_MapName,Window_ScrollText,Window_Gold,
//! Window_TitleCommand --allow-base Window` 切出（**2,475 行**：**344 条方法 + 12 条链 +
//! 12 句 `constructor` 补句 + 1 格函数型静态 `Window_TitleCommand.initCommandPosition` +
//! 5 格静态常量**，访问器/别名/对外物赋值 0、欠账 0），逐字对表在 `objects-verbatim.py` 的
//! **第六处上游**那一桶（`src/js/windows_*.js` ↔ `rpg_windows.js`）。
//!
//! **为什么这一族压在 sprites 之后**：`Scene_Map.createAllWindows`（已随场景第一段交出）里那两句
//! `createMessageWindow`/`createScrollTextWindow` 今天就是它**停下来**的那一格（窗是那颗没交
//! `initialize` 的空壳 ⇒ 抛）。**但交完这十二颗并没有走到"场景 start"** —— 停点往下挪了一格，
//! 而那一格不在本族：`Scene_Map` 那一拍现在建完 spriteset、**图名窗**与 windowLayer，
//! 停在 `createAllWindows` 第一颗要**量字**的窗上（`Window_Message.createSubWindows` 的第一句
//! `new Window_Gold(0, 0)` → `refresh` → `drawCurrencyValue` → `textWidth` → `measureTextWidth` →
//! 宿主的 `__sisf_measure_text` **没绑** ⇒ 点名抛，`_mapLoaded` 留在 `false`）。
//! ⇒ **台词上屏与"屏幕上有任何一扇窗"是同一个前提**，那一格就是
//! 「真绘制第二小格（下）」= **文字那两格**（`M10.md` 事实 140 把它的开工顺序提到了主循环之前）。
//!
//! 与上游的偏差与前提（每一条都是量出来的）：
//!
//! 1. **窗的实身是位图，不是 GL**：这十二颗画的全是 `Bitmap` 操作（`blt`/`fillRect`/`gradientFillRect`/
//!    `drawText`/`clearRect`），而 `Bitmap` 那一族已经随 M10.4 交出 ⇒ **零新底**（`SHIM_BASE` 仍
//!    **64 颗**、`HOME_MADE` 仍 **8 格**）。⚠ 唯一那一格**不落像素**的是文字（`fillText`/`strokeText`
//!    仍在 `composite.rs` 的 `NOT_SYNTHESIZED` 那一桶，`M10.md` 事实 126）⇒ 本族交的是**文本与几何**，
//!    字真正糊上位图是「真绘制第二小格（下）」那一格（前置 = 引擎请求清单第 15 条）。
//! 2. **跨层点名只有 39 个（去重后的 `owner.method`）/ 八颗接收者**，而其中 **38 格今天已在场**：
//!    `TextManager` 8 名（含 `newGame`/`continue_`/`hpA` 那一族，全在复数 `Object.defineProperties`
//!    那块里）、`ImageManager` 6 名（`loadSystem`/`loadCharacter`/`loadFace`/`reserveFace`/
//!    `releaseReservation`/`isBigCharacter` —— 前两名早已在场，后两名是**这一格补的前提**，见偏差 3）、
//!    `Input`/`TouchInput` 14 名（平台件）、`SoundManager` 4 名（`playCursor` 那一族）、
//!    `Graphics` 2 名、`DataManager` 4 名、`Utils` 1 名（`generateRuntimeId`）。
//!    **这 39 格是「三档」的，不是一档**（pray 侧
//!    `the_window_layer_reaches_the_rest_of_the_engine_and_names_its_own_gap` 钉的读数 =
//!    **23 格 `fn` / 15 格 `prop` / 1 格 `absent`**）：`prop` 那一档全是**访问器**
//!    （`Graphics.boxWidth`/`boxHeight`、`Input.date`、`TouchInput.date`/`x`/`y`/`wheelY` 那七格
//!    加 `TextManager` 那八格），它们的 getter 读的是 `_boxWidth`/`_date`/`_x` 那些**字段本身**，
//!    没人调过 `Graphics.initialize`/`Input.initialize` 时值就是 `undefined` ⇒ **文本在场、等的是初始化
//!    那一拍，不是缺口**（`M10.md` 事实 139：拿 `typeof 值` 当缺席来数，会给下一格留下一份不存在的账）。
//!    **真缺的只有一格**：`DataManager.isAnySavefileExists`（`Window_TitleCommand.isContinueEnabled`
//!    那一名，`rpg_windows.js:5758`）⇒ 它踩的是 `loadGlobalInfo` + `StorageManager.load`，
//!    也就是**存档改判那一格**（`M10.md` 待定项 1 / 任务 #9）的账，**不在本格补假底**，
//!    落法是让那条腿**照抛并点名**（`the_title_command_stops_at_the_undelivered_savefile_check_and_names_it`）。
//! 3. **`ImageManager.loadFace` 与 `reserveFace` 是与本族同一轮交出的**（`rpg_managers.js:823-825`/
//!    `:943-945` 逐字 ⇒ managers 那一桶 **219 / 215 → 221 / 217**，`HOME_MADE` 不动）——
//!    事实 130 那一课的**第三次**兑现，方向仍是"开工前先量"：`Window_Base.drawFace` 喊前者，
//!    `Window_Base.reserveFaceImages` 与 `Window_Message.loadMessageFace` 喊后者，少任何一名都不是
//!    "头像不画"，而是 **TypeError 抛穿建窗那一句**。
//! 4. **建一颗窗要的是那颗 `$gameSystem`**：`Window_Base.initialize` 第五句 `updateTone()` 读
//!    `$gameSystem.windowTone()`（磁盘真值 `[-51,-51,-51,0]`，`M10.md` 事实 87），`standardFontFace()`
//!    读 `$gameSystem.isChinese()`，而 `Window_Gold`/`Window_Message` 那两颗还要 `$gameParty`/`$gameMessage`
//!    ⇒ crate 这一侧**只钉静态面**（链、成员面、那五格池子、幂等、边界），**一切"建得出窗"的行为读数
//!    住在 `sisf_pray/src/compat.rs`** 的整套层夹具上：那里跑的是真 `System.json`、真窗皮字节与真
//!    `$gameSystem` —— 同一分工已写过 sprites（`M10.md` 事实 136/137）与「这一族的规矩」第 2 条。
//! 5. **链的基类只有一颗在场外**：`Window`（容器树第五格交出的那颗）。其余十一条挂在这十二颗自己
//!    身上 ⇒ 装载次序是**结构性的**：`install` 自己踩 [`crate::core::install_display`]，判据
//!    `windows_install_does_not_depend_on_the_caller_installing_the_display_layer_first` 钉这一格。
//!    **而"自己踩"不等于"重跑"**：本族是第一个在"容器树已装、且 `Spriteset_*` 已经接在
//!    `Sprite.prototype` 上"之后才踩那一格的层，重跑那一版把已接好的链孤儿化了
//!    （`instanceof Sprite` 静默变 `false`）⇒ 那一格现在是**先问再装、第二次跳过**
//!    （`M10.md` 事实 138；判据
//!    `reinstalling_the_windows_layer_does_not_swap_the_prototypes_the_chains_point_at`）。
//! 6. **那 5 格静态常量是数据形状**（`Window_Base._iconWidth/_iconHeight` = 32/32、
//!    `_faceWidth/_faceHeight` = 144/144，与 `Window_TitleCommand._lastCommandSymbol = null`）
//!    ⇒ 逐字尺子**数不到**（右值不是函数），由 `the_windows_member_face_is_the_upstream_344_with_the_five_pools` 点名；
//!    而 `initCommandPosition` 那一格是**函数型静态**，切片随类交、尺子数得到。

use crate::jsrt::Host;

/// `rpg_windows.js` 的十二颗：344 条方法 + 12 条链 + 12 句 `constructor` 补句 + 1 格函数型静态 + 5 格静态常量。
///
/// **没有一格被跳过**（切片读数：访问器 0 / 别名赋值 0 / 对外物赋值 0 / 未归类 0 / 欠账 0）
/// ⇒ 容器树那一族的"整批交"手法（`Window`/`Tilemap`/`Weather` 三格）第一次落到**一整个文件层级**上。
pub const WINDOWS_FLOW_JS: &str = include_str!("js/windows_flow.js");

/// 装上窗那一族的十二颗。踩在 [`crate::core::install_display`] 之后（偏差 5：链基类是 `Window`）。
///
/// **不装 managers 那一族**（`TextManager`/`SoundManager`/`DataManager`/`ImageManager` 的点名格子
/// 与那颗 `$gameSystem` 都不在本格）：谁喊它谁自己装 —— 与 [`crate::sprites::install`] 同一方向，
/// 判据 `the_windows_layer_does_not_install_the_managers_layer_it_calls`。
///
/// 幂等：十二段都是"在同一颗全局上重新 define"，第二次装不抛（wasm 腿共享 realm，判据
/// `reloading_the_windows_is_still_the_window_layer`）。
pub fn install(host: &Host) -> Result<(), String> {
    crate::core::install_display(host)?;
    host.eval(WINDOWS_FLOW_JS)
        .map_err(|e| format!("`Window_*` 那十二颗装不上：{e}"))
}

#[cfg(all(test, feature = "js"))]
mod tests {
    use super::*;
    use crate::shapes::{DeclaredShape, ShapeKind};
    use crate::{core, managers};
    use sisf_asset::{MemoryProvider, Vfs, VfsPath};
    use std::rc::Rc;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    /// 本族 fixture 的形状名单：容器树那十一颗 + `Bitmap` 那一族 + **窗这十二颗**（壳由
    /// `shapes::install` 给，实身由本族的文本重新 define —— 与 scenes/sprites 两格同一手法）。
    const SHELLS: &[(&str, ShapeKind)] = &[
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
        ("Window_Base", ShapeKind::Class),
        ("Window_Selectable", ShapeKind::Class),
        ("Window_Command", ShapeKind::Class),
        ("Window_Message", ShapeKind::Class),
        ("Window_ChoiceList", ShapeKind::Class),
        ("Window_NumberInput", ShapeKind::Class),
        ("Window_EventItem", ShapeKind::Class),
        ("Window_ItemList", ShapeKind::Class),
        ("Window_MapName", ShapeKind::Class),
        ("Window_ScrollText", ShapeKind::Class),
        ("Window_Gold", ShapeKind::Class),
        ("Window_TitleCommand", ShapeKind::Class),
    ];

    fn shells(host: &Host) {
        let shapes: Vec<DeclaredShape> = SHELLS
            .iter()
            .map(|(n, k)| DeclaredShape {
                name: n.to_string(),
                kind: *k,
            })
            .collect();
        crate::shapes::install(host, &shapes).expect("形状装不上");
        crate::shapes::install_extensions(host).expect("JsExtensions 装不上");
    }

    /// 只到"文本装得上"那一档：形状 + core（含 `Bitmap`）+ 本族，**没有 managers、没有 `Game_*`**。
    ///
    /// 这一档只能问**静态面**（链、成员面、那五格池子）—— 任何一句 `new Window_Base(...)` 都会
    /// 在 `loadWindowskin` 那一名上抛，而那正是偏差 2 要钉的方向。
    fn booted() -> Host {
        let host = Host::boot();
        shells(&host);
        host.eval(crate::platform::UTILS_JS)
            .expect("`Utils` 装不上");
        host.eval(core::GRAPHICS_JS)
            .expect("`Graphics` 的屏幕读数装不上");
        let mut vfs = Vfs::new();
        vfs.register(Box::new(MemoryProvider::new()));
        core::install_bitmap(&host, Rc::new(vfs)).expect("`Bitmap` 那一半装不上");
        install(&host).expect("那十二颗装不上");
        core::set_screen_size(&host, 1104, 736).expect("屏幕尺寸写不进 `Graphics`");
        host
    }

    /// 十二颗的**链**：一条 `← Window`、四条 `← Window_Base`（`Window_Base` 自己那一格之外还有
    /// `Window_Gold`/`Window_Message`/`Window_ScrollText`/`Window_MapName`）、四条 `← Window_Selectable`、
    /// 三条命令链（`Window_ChoiceList`/`Window_TitleCommand` 与 `Window_Command` 自己）、
    /// 一条 `← Window_ItemList`。⚠ `Window_EventItem` 挂的是 `Window_ItemList` 而**不是**
    /// `Window_Selectable`（我第一版就是这么猜错的 —— 上游 `rpg_windows.js:4172`），
    /// 而 `Window_Gold` 虽然名字里带着"命令窗那一族"，链上其实是 `Window_Base`（`:4152` 的
    /// `Window_Gold` 自己实现 `refresh`）。
    #[test]
    fn the_twelve_windows_chain_onto_the_window_and_back_at_themselves() {
        let h = booted();
        assert_eq!(
            h.eval_string(
                "[Window_Base, Window_Selectable, Window_Command, Window_Gold, Window_ItemList, \
                  Window_ChoiceList, Window_NumberInput, Window_EventItem, Window_Message, \
                  Window_ScrollText, Window_MapName, Window_TitleCommand].map(function(c) {\
                     return Object.getPrototypeOf(c.prototype).constructor.name;\
                 }).join(',')"
            ),
            "Window,Window_Base,Window_Selectable,Window_Base,Window_Selectable,Window_Command,\
             Window_Selectable,Window_ItemList,Window_Base,Window_Base,Window_Base,Window_Command",
            "十二条链的基类必须逐个是这一族自己那十一颗之一（或容器树那颗 `Window`）"
        );
        assert_eq!(
            h.eval_string(
                "[Window_Base, Window_Selectable, Window_Command, Window_Gold, Window_ItemList, \
                  Window_ChoiceList, Window_NumberInput, Window_EventItem, Window_Message, \
                  Window_ScrollText, Window_Gold, Window_TitleCommand]\
                 .map(function(c){ return c.prototype.constructor === c; }).join(',')"
            ),
            "true,true,true,true,true,true,true,true,true,true,true,true",
            "十二句 `constructor` 补句每一颗都回指自己"
        );
        // 链在**运行时**也走得通：一颗 `Window_TitleCommand` 的实例同时是 Selectable/Command/Base/Window。
        assert_eq!(
            h.eval_string(
                "var w = Object.create(Window_TitleCommand.prototype); \
                 [w instanceof Window_Command, w instanceof Window_Selectable, \
                  w instanceof Window_Base, w instanceof Window, \
                  w instanceof PIXI.Container].join(',')"
            ),
            "true,true,true,true,true",
            "四层级加容器树那一条腿，实例一路 `instanceof` 上去（`--allow-base Window` 的兑现处）"
        );
    }

    /// **成员面 = 上游那 344 条 + 12 句补句 + 5 格数据形状静态**（`hasOwnProperty` 那一族的口径，
    /// 事实 106/120/131；那五格右值不是函数 ⇒ 逐字尺子数不到，只有这一格数）。
    #[test]
    fn the_windows_member_face_is_the_upstream_344_with_the_five_pools() {
        let h = booted();
        let classes = "[Window_Base, Window_Selectable, Window_Command, Window_Message, \
                       Window_ChoiceList, Window_NumberInput, Window_EventItem, Window_ItemList, \
                       Window_MapName, Window_ScrollText, Window_Gold, Window_TitleCommand]";
        assert_eq!(
            h.eval_string(&format!(
                "{classes}.map(function(c) {{ \
                   return Object.getOwnPropertyNames(c.prototype).length; }}).join(',')"
            )),
            "98,84,25,38,17,28,10,18,11,11,8,8",
            "每颗 = 上游方法数 + 1 句 `constructor`（97/83/24/37/16/27/9/17/10/10/7/7）"
        );
        assert_eq!(
            h.eval_string(&format!(
                "String({classes}.reduce(function(n, c) {{ \
                   return n + Object.getOwnPropertyNames(c.prototype).length - 1; }}, 0))"
            )),
            "344",
            "`prototype` 方法总数就是切片器报的那 344 条（而闭包尺子那 356 = 344 + 12 句补句）"
        );
        // 五格数据形状静态 + 那颗命名空间用的都是**当场取的基线差集**（事实 131）。
        assert_eq!(
            h.eval_string(
                "var base = Object.getOwnPropertyNames(function(){}); \
                 Object.getOwnPropertyNames(Window_Base).filter(function(n) {\
                     return base.indexOf(n) < 0; }).join(',')"
            ),
            "_iconWidth,_iconHeight,_faceWidth,_faceHeight",
            "`Window_Base` 的静态面必须恰好是那四格池子（顺序 = 上游声明顺序，两腿一致才钉）"
        );
        assert_eq!(
            h.eval_string(
                "var base = Object.getOwnPropertyNames(function(){}); \
                 Object.getOwnPropertyNames(Window_TitleCommand).filter(function(n) {\
                     return base.indexOf(n) < 0; }).join(',')"
            ),
            "_lastCommandSymbol,initCommandPosition",
            "那一格**函数型静态**与那一格数据形状池子同颗 —— 前者尺子数得到、后者数不到"
        );
        assert_eq!(
            h.eval_string(
                "[Window_Base._iconWidth, Window_Base._iconHeight, Window_Base._faceWidth, \
                  Window_Base._faceHeight, String(Window_TitleCommand._lastCommandSymbol), \
                  typeof Window_TitleCommand.initCommandPosition].join(',')"
            ),
            "32,32,144,144,null,function",
            "四格的数就是上游那四个（图标 32×32、头像 144×144 = M7 那一族窗内头像的尺寸），\
             那颗命令池子的初值是 `null`，而 `initCommandPosition` 是真函数"
        );
    }

    /// **本族不装 managers 那一族**（偏差 2 的方向，钉**增量**而不是绝对值 —— 事实 135 那一族的
    /// 第二次兑现：共享 realm 里"某颗全局缺席"不能写死）。
    #[test]
    fn the_windows_layer_does_not_install_the_managers_layer_it_calls() {
        let host = Host::boot();
        shells(&host);
        let before = host.eval_string("[typeof TextManager, typeof SoundManager].join(',')");
        install(&host).expect("那十二颗装不上");
        assert_eq!(
            host.eval_string("[typeof TextManager, typeof SoundManager].join(',')"),
            before,
            "`windows::install` 自己把 managers 那一族装出来了 ⇒ 两族合一，管辖面又混了（事实 73）"
        );
        assert_eq!(
            host.eval_string("typeof Window_Base.prototype.processEscapeCharacter"),
            "function",
            "这一族自己的格子必须真的在场（否则上面那条只是因为整族都没装上）"
        );
    }

    /// **偏差 3 的前提兑现**：窗喊的那两格 face 装载确实在场，且踩的是已交的 `loadBitmap`/
    /// `reserveBitmap` 那一条 URL 腿（真字节在 pray 侧量）。
    #[test]
    fn the_two_face_cells_the_windows_call_are_delivered_by_the_managers_layer() {
        let host = Host::boot();
        let mut shapes: Vec<DeclaredShape> = Vec::new();
        // managers 那一族的九颗形状 + 本族那三十四颗：少任何一颗，`managers::install` 就在
        // `Decrypter is not defined` 上抛（事实 78 那一条"fixture 的名单要在同一轮里改"的第四次兑现）。
        for (n, k) in SHELLS.iter().chain(crate::sprites::MANAGERS_SHAPES.iter()) {
            shapes.push(DeclaredShape {
                name: n.to_string(),
                kind: *k,
            });
        }
        crate::shapes::install(&host, &shapes).expect("形状装不上");
        crate::shapes::install_extensions(&host).expect("JsExtensions 装不上");
        host.eval(crate::platform::HOST_GLOBALS_JS)
            .expect("宿主全局面装不上");
        host.eval(crate::platform::UTILS_JS)
            .expect("`Utils` 装不上");
        host.eval(crate::platform::RESOURCE_HANDLER_JS)
            .expect("`ResourceHandler` 的状态那一半装不上");
        let mut mem = MemoryProvider::new();
        mem.insert(
            &VfsPath::new("data/System.json").unwrap(),
            br#"{"hasEncryptedImages":false,"hasEncryptedAudio":false,
                 "encryptionKey":"","currencyUnit":"G","startMapId":8,"startX":19,"startY":14,
                 "terms":{"basic":[],"params":[],"commands":[],"messages":{}}}"#,
        );
        let mut vfs = Vfs::new();
        vfs.register(Box::new(mem));
        managers::install(&host, Rc::new(vfs)).expect("managers 装不上");
        assert_eq!(
            host.eval_string(
                "[typeof ImageManager.loadFace, typeof ImageManager.reserveFace, \
                  decodeURIComponent(ImageManager.loadFace('TCGAlcybase_1_1').url), \
                  decodeURIComponent(ImageManager.reserveFace('Alcy', 0, 7).url)].join('|')"
            ),
            "function|function|img/faces/TCGAlcybase_1_1.png|img/faces/Alcy.png",
            "两名都是函数，而 URL 拼写就是磁盘那一份（`img/faces/` + 名字 + `.png`）"
        );
    }

    /// 重复装载幂等（wasm 腿共享 realm，「这一族的文本要能装第二次」那条规矩）。
    #[test]
    fn reloading_the_windows_is_still_the_window_layer() {
        let h = booted();
        let before =
            h.eval_string("String(Object.getOwnPropertyNames(Window_Message.prototype).length)");
        install(&h).expect("第二次装那十二颗抛了");
        assert_eq!(
            h.eval_string("String(Object.getOwnPropertyNames(Window_Message.prototype).length)"),
            before,
            "第二次装载不许往同一颗 prototype 上追加成员"
        );
    }

    /// **`install` 不要求调用方先装容器树**（它自己踩 `core::install_display`）。
    #[test]
    fn windows_install_does_not_depend_on_the_caller_installing_the_display_layer_first() {
        let host = Host::boot();
        shells(&host);
        host.eval(crate::platform::UTILS_JS)
            .expect("`Utils` 装不上");
        install(&host).expect("`install` 要求自己先装容器树 ⇒ 那一层成了调用方的隐含前提");
        assert_eq!(
            host.eval_string(
                "String(Object.getPrototypeOf(Window.prototype) === PIXI.Container.prototype)",
            ),
            "true",
            "容器树那十一颗是由本族的 `install` 自己踩进来的"
        );
    }

    /// **`install` 第二次踩容器树那一格必须是「跳过」而不是「重跑」**（`M10.md` 事实 138）。
    ///
    /// 窗这一族是**第一个**在"容器树已装、而且已经有人把链接在 `Sprite.prototype`/`TilingSprite.prototype`
    /// 上"之后才踩 [`crate::core::install_display`] 的层 ⇒ 它把那条"重装天然幂等"的假设打红了：
    /// 十三段里每一段的第一句都是 `X.prototype = Object.create(…)`（**整颗替换**，上游的写法，
    /// 逐字文本改不了），重跑就把那颗 prototype 换成新的对象，而先前接好的链还指着**旧的**那一颗
    /// ⇒ `new X() instanceof Sprite` 静默变 `false`（不抛）。pray 侧那条真流判据
    /// （`a_real_map8_spriteset_builds_the_whole_display_tree` 的第一格读数）就是这么红的。
    ///
    /// 手法与事实 135 同一条腿：**不钉全局的绝对值，钉"还是不是同一颗对象"这个关系** —— 先把三颗
    /// prototype 存进 `globalThis`，再造一颗按 `sprites_flow` 同一手法接在 `Sprite.prototype` 上的探针类，
    /// 第二次 `install` 之后四格必须全真。（窗自己的文本重跑**确实**会换掉 `Window_Base.prototype`，
    /// 那是 `reloading_the_windows_is_still_the_window_layer` 管的那一档；今天没有任何一层接在
    /// 窗身上，所以那一档只需要"实例的字段还在"，本格管的是**跨层**那一条。）
    #[test]
    fn reinstalling_the_windows_layer_does_not_swap_the_prototypes_the_chains_point_at() {
        let h = booted();
        h.eval(
            "globalThis.__pixiSprite = PIXI.Sprite.prototype; \
             globalThis.__spriteProto = Sprite.prototype; \
             globalThis.__tsProto = TilingSprite.prototype; \
             var chained = function(){}; chained.prototype = Object.create(Sprite.prototype); \
             globalThis.__chained = chained;",
        )
        .expect("接不出那颗探针类");
        assert_eq!(
            h.eval_string("String(new __chained() instanceof Sprite)"),
            "true",
            "接好之后必须先证明它认得 `Sprite`（否则下面的四格全真是空的）"
        );
        install(&h).expect("第二次装不上（共享 realm 里重define 不许抛）");
        assert_eq!(
            h.eval_string(
                "[String(new __chained() instanceof Sprite), String(__pixiSprite === PIXI.Sprite.prototype), \
                 String(__spriteProto === Sprite.prototype), String(__tsProto === TilingSprite.prototype)]\
                 .join(',')"
            ),
            "true,true,true,true",
            "第二次踩同一格不能换掉任何一颗**已经有人接在上面**的 prototype（跳过，不是重跑）"
        );
    }
}
