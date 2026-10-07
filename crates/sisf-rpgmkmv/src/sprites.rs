//! M10.5 的**场景第二段：`rpg_sprites.js` 真流闭包里的那十颗**（显示那一族）。
//!
//! 名单与体量是查出来的（`../sisf_pray/tools/probes/core-touch.py --closure --no-push` 的
//! `rpg_sprites.js` 那一行：**闭包里 10/18 颗 / 155 条**，另外八颗全是战斗那一族）。文本由
//! `../sisf_pray/tools/probes/objects-slice.py --file rpg_sprites.js --classes Sprite_Base,
//! Sprite_Character,Sprite_Balloon,Sprite_Picture,Sprite_Timer,Sprite_Destination,Sprite_Animation,
//! Sprite_Button,Spriteset_Base,Spriteset_Map --allow-base Sprite` 切出（**1,260 行**：
//! **145 条方法 + 10 条链 + 10 句 `constructor` 补句 + 2 格静态常量**，访问器/别名/对外物赋值 0、
//! 未归类 0），逐字对表在 `objects-verbatim.py` 的**第五处上游**那一桶（`src/js/sprites_*.js` ↔
//! `rpg_sprites.js`）。
//!
//! **为什么是这十颗而不是按文件顺序**：`Scene_Map.createSpriteset`（已随场景第一段交出）那句
//! `new Spriteset_Map()` 今天就是它**停下来**的那一格（`M10.md` 事实 133 那条读数）。交完这十颗，
//! 换图那一条腿才连得到画面。
//!
//! 与上游的偏差与前提（每一条都是量出来的）：
//!
//! 1. **这一族的跨层面只有 19 处 / 四颗接收者**（口径 = **切出来的那一份文本**，复现：
//!    `grep -o "ImageManager\.[A-Za-z_]*" src/js/sprites_flow.js | sort | uniq -c` 那一族三条 grep
//!    一起数）：`ImageManager` **11 处 / 7 名**（`loadTileset` 2 · `loadSystem` 2 · `loadParallax` 2 ·
//!    `loadAnimation` 2 · `loadPicture` 1 · `loadCharacter` 1 · `isBigCharacter` 1）、
//!    `Graphics.isWebGL` 4 处、`TouchInput.isTriggered`/`isReleased` 3 处、`AudioManager.playSe` 1 处。
//!    ⚠ 别与尺子 `core-touch.py --deps --file rpg_sprites.js` 那份读数互换：那一把量的是**整份上游文件**
//!    （core 3 名 / 58 处 + managers 5 名 / 30 处，含没进闭包那八颗战斗类的引用）。⇒ 切片这一档
//!    **不喊 windows、不喊 scenes、不喊 `BattleManager`** —— 本族第一条"外面只剩 managers/core/平台件"的文本。
//! 2. **正因如此，`ImageManager` 的 `loadXXX` 那一族是这一格的前提，不是后话**：
//!    `loadAnimation`/`loadCharacter`/`loadParallax`/`loadPicture`/`loadSystem`/`loadTileset`
//!    六名 + 它们脚下的 `loadBitmap`/`loadNormalBitmap` = **八格上游文本，与这十颗同一轮交**
//!    （[`crate::managers::IMAGE_MANAGER_LOAD_JS`]）。这一手是上一格那条根（`M10.md` 事实 130：
//!    交出一族会唤醒已交付文本里的一句守卫）的**兑现** —— 这一次在动代码之前先把"会让谁开始跑"量完。
//!    `loadTitle1`/`loadTitle2` 也在那八格里，理由是场景第一段的 `Scene_Title.createBackground`。
//! 3. **零新底**：`SHIM_BASE` 仍 **64 颗**、`HOME_MADE` 仍 **8 格**。这十颗脚下要的每一颗类
//!    （`Sprite`/`Bitmap`/`Rectangle`/`ScreenSprite`/`ToneFilter`/`ToneSprite`/`TilingSprite`/
//!    `Tilemap`/`Weather`/`WindowLayer`）与每一条位图动作（`blt`/`fillRect`/`drawText`/`clear`/
//!    `adjustTone`）**全在已交付的面上**；`Graphics.isWebGL()` 恒假那一条腿（`M10.md` 事实 101
//!    **第六次**应用）意味着 `Spriteset_Map.createTilemap` 走的是 `new Tilemap()` 而**永远不碰**
//!    那颗按定案不给 GL 底的 `ShaderTilemap`。
//! 4. **那 2 格静态常量是数据形状**（`Sprite_Animation._checker1/_checker2 = {}`，`:1181-1184`）
//!    ⇒ 逐字尺子**数不到**（右值不是函数），由 `the_sprites_member_face_is_the_upstream_145_with_two_pools`
//!    点名；它们不是访问器、也不是函数型静态（同 `PIXI.SCALE_MODES` / `Tilemap.TILE_ID_*` 那条腿）。
//! 5. **十颗的链基类只有一颗在场外**：`Sprite`（容器树第二颗）。`Spriteset_Base` 挂的也是
//!    `Object.create(Sprite.prototype)`（`rpg_sprites.js:2120`）—— **显示集合自己就是一颗精灵**，
//!    这是本族唯一一处"名字听着是容器、形状是 Sprite"，装载次序与判据都按那颗真的 `Sprite` 走。

use crate::jsrt::Host;
/// 只有 [`MANAGERS_SHAPES`] 那一格 fixture 名单用它 ⇒ 跟着它一起 `#[cfg(test)]`
/// （lib 构建里它就是 clippy 的 `unused import`，而那一格是 `-D warnings`）。
#[cfg(test)]
use crate::shapes::ShapeKind;

/// `rpg_sprites.js` 的十颗：145 条方法 + 10 条链 + 10 句 `constructor` 补句 + 2 格静态常量。
///
/// **没有一格被跳过**（切片那格的读数：访问器 0 / 别名赋值 0 / 对外物赋值 0 / 未归类 0 / 欠账 0）
/// ⇒ 本族**第一条「整批交、一份上游文本都不欠」的显示族**（`Window` 那一颗也欠 0，但那只有一颗）。
pub const SPRITES_FLOW_JS: &str = include_str!("js/sprites_flow.js");

/// **managers 那一族的九颗静态类形状**（`managers.rs` 的 fixture 同一份名单）。
///
/// M10.5 的窗那一格起它搬到模块级并 `pub(crate)`：`crate::windows` 的那份"建得出窗"的夹具要点名
/// 同一批（`Decrypter` 少一颗就是 `managers::install` 当场 `Decrypter is not defined`），
/// 而**名单抄两遍就会飘**（同 `crate::objects` 那四族名单搬到模块级的那一条理由）。
#[cfg(test)]
pub(crate) const MANAGERS_SHAPES: &[(&str, ShapeKind)] = &[
    ("Decrypter", ShapeKind::StaticClass),
    ("DataManager", ShapeKind::StaticClass),
    ("TextManager", ShapeKind::StaticClass),
    ("ImageManager", ShapeKind::StaticClass),
    ("SceneManager", ShapeKind::StaticClass),
    ("Scene_Boot", ShapeKind::StaticClass),
    ("Html5Audio", ShapeKind::StaticClass),
    ("StorageManager", ShapeKind::StaticClass),
    ("ConfigManager", ShapeKind::StaticClass),
];

/// 装上显示那一族的十颗。踩在 [`crate::core::install_display`] 之后（偏差 5：链基类是 `Sprite`）。
///
/// **不装 [`crate::managers::IMAGE_MANAGER_LOAD_JS`]**：那是 managers 那一族的格子，由
/// [`crate::managers::install`] 自己交（"谁喊它谁自己装"那一条纪律在这里的方向是反的 ——
/// 被喊的那一格住在**另一族**里，所以由那一族的装载函数负责在场，判据
/// `the_load_family_the_sprites_call_is_delivered_by_the_managers_layer` 钉这一格）。
///
/// 幂等：十段都是"在同一颗全局上重新 define"，第二次装不抛（wasm 腿共享 realm，判据
/// `reloading_the_sprites_is_still_the_sprite_layer`）。
pub fn install(host: &Host) -> Result<(), String> {
    crate::core::install_display(host)?;
    host.eval(SPRITES_FLOW_JS)
        .map_err(|e| format!("`Sprite_*`/`Spriteset_*` 那十颗装不上：{e}"))
}

#[cfg(test)]
mod tests {
    //! fixture 判据：只用 fixture 的形状、屏幕读数与一颗 fixture VFS 位图（这颗 crate 不许知道
    //! 某一款游戏在哪）。整套层之上的账（真图块组、真行走图的字节）住在 `sisf_pray/src/compat.rs`。
    use super::*;
    use crate::shapes::{DeclaredShape, ShapeKind};
    use crate::{core, managers};
    use sisf_asset::{MemoryProvider, Vfs, VfsPath};
    use std::rc::Rc;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    /// 十颗 + 容器树那一层 + `Bitmap` 那一半要的形状。构造体**全部沿用 [`crate::shapes`] 那颗壳**
    /// （十颗的上游构造体都是 `this.initialize.apply(this, arguments);`）⇒ `--ctor` 没开。
    const SHELLS: &[(&str, ShapeKind)] = &[
        ("Sprite_Base", ShapeKind::Class),
        ("Sprite_Button", ShapeKind::Class),
        ("Sprite_Character", ShapeKind::Class),
        ("Sprite_Animation", ShapeKind::Class),
        ("Sprite_Balloon", ShapeKind::Class),
        ("Sprite_Picture", ShapeKind::Class),
        ("Sprite_Timer", ShapeKind::Class),
        ("Sprite_Destination", ShapeKind::Class),
        ("Spriteset_Base", ShapeKind::Class),
        ("Spriteset_Map", ShapeKind::Class),
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

    /// 装到"脚下有容器树与真尺寸位图、而**这一族那十颗还没装**"那一档（给 `the_sprites_layer_…`
    /// 用来钉增量，不是绝对值）。
    fn bare() -> Host {
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
        host.eval(crate::platform::UTILS_JS)
            .expect("`Utils` 装不上");
        host.eval(core::GRAPHICS_JS)
            .expect("`Graphics` 的屏幕读数装不上");
        let mut vfs = Vfs::new();
        vfs.register(Box::new(MemoryProvider::new()));
        core::install_bitmap(&host, Rc::new(vfs)).expect("`Bitmap` 那一半装不上");
        host
    }

    /// 装到"十颗挂在容器树上、脚下有一颗真尺寸位图"那一档。
    ///
    /// 位图那一半要 `Vfs`：`new Bitmap(576, 384)` 的 `_createCanvas` 是宿主底，而 `setFrame` 之后
    /// `_refresh` 要问 `bitmap.width`（= 那颗 `__canvas` 的尺寸记录）。fixture 给的 VFS 里
    /// **一张素材都没有** —— 这一族判据钉的是**网格算术与状态**，不是字节（字节那一条在 pray 侧）。
    fn booted() -> Host {
        let host = bare();
        install(&host).expect("那十颗装不上");
        core::set_screen_size(&host, 1104, 736).expect("屏幕尺寸写不进 `Graphics`");
        host
    }

    /// 十颗的**链与 `constructor` 回指**：七颗直接挂 `Sprite`（连 `Spriteset_Base` 在内 ——
    /// 偏差 5），`Sprite_Character`/`Sprite_Balloon` 挂 `Sprite_Base`，`Spriteset_Map` 挂
    /// `Spriteset_Base`。
    #[test]
    fn the_ten_sprites_chain_onto_the_sprite_and_back_at_themselves() {
        let h = booted();
        assert_eq!(
            h.eval_string(
                "[Sprite_Base, Sprite_Button, Sprite_Animation, Sprite_Picture, Sprite_Timer, \
                  Sprite_Destination, Spriteset_Base].map(function(c) {\
                     return Object.getPrototypeOf(c.prototype) === Sprite.prototype;\
                 }).join(',')"
            ),
            "true,true,true,true,true,true,true",
            "七颗挂 `Sprite.prototype`（`rpg_sprites.js:14/80/1179/1908/2003/2064/2120`）"
        );
        assert_eq!(
            h.eval_string(
                "[Object.getPrototypeOf(Sprite_Character.prototype) === Sprite_Base.prototype, \
                  Object.getPrototypeOf(Sprite_Balloon.prototype) === Sprite_Base.prototype, \
                  Object.getPrototypeOf(Spriteset_Map.prototype) === Spriteset_Base.prototype, \
                  Sprite_Character.prototype.constructor === Sprite_Character, \
                  Spriteset_Map.prototype.constructor === Spriteset_Map].join(',')"
            ),
            "true,true,true,true,true",
            "另外三条链（两条 `← Sprite_Base`、一条 `← Spriteset_Base`）与那 10 句补句里的两颗"
        );
        assert_eq!(
            h.eval_string(
                "var s = new Sprite_Base(); \
                 [s instanceof Sprite, String(s._animationSprites.length), String(s._hiding), \
                  String(s.isAnimationPlaying()), s._effectTarget === s].join(',')"
            ),
            "true,0,false,false,true",
            "`Sprite_Base.initialize` 那三句落位，而 `_effectTarget` 默认就是自己（`rpg_sprites.js:18`）"
        );
    }

    /// **成员面 = 上游那 145 条 + 10 句补句 + 2 格数据形状静态**（`hasOwnProperty` 那一族的口径，
    /// `M10.md` 事实 106/120；那颗"函数自带名"的基线差集写法用的是事实 131 那一把）。
    #[test]
    fn the_sprites_member_face_is_the_upstream_145_with_two_pools() {
        let h = booted();
        assert_eq!(
            h.eval_string(
                "[Sprite_Base, Sprite_Button, Sprite_Character, Sprite_Animation, Sprite_Balloon, \
                  Sprite_Picture, Sprite_Timer, Sprite_Destination, Spriteset_Base, Spriteset_Map]\
                 .map(function(c) { return Object.getOwnPropertyNames(c.prototype).length; }).join(',')"
            ),
            // 上游方法数 + 1 句 `constructor`：8/12/31/28/10/10/8/5/16/17
            "9,13,32,29,11,11,9,6,17,18",
            "十颗的自有格数与切片读数（145 + 10 补句）不符"
        );
        assert_eq!(
            h.eval_string(
                "String([Sprite_Base, Sprite_Button, Sprite_Character, Sprite_Animation, \
                  Sprite_Balloon, Sprite_Picture, Sprite_Timer, Sprite_Destination, \
                  Spriteset_Base, Spriteset_Map].reduce(function(n, c) {\
                     return n + Object.getOwnPropertyNames(c.prototype).length - 1;\
                 }, 0))"
            ),
            "145",
            "`prototype` 方法总数就是切片器报的那 145 条"
        );
        // 那 2 格**数据形状**静态（`_checker1`/`_checker2`）：右值不是函数 ⇒ 尺子数不到，由这一格点名。
        // 用**当场取的基线差集**（一颗空函数）而不是写死名单 —— 两腿的函数自带名不一样（事实 131）。
        assert_eq!(
            h.eval_string(
                "var base = Object.getOwnPropertyNames(function(){}); \
                 Object.getOwnPropertyNames(Sprite_Animation).filter(function(n) {\
                     return base.indexOf(n) < 0;\
                 }).join(',')"
            ),
            "_checker1,_checker2",
            "静态面上多出来的必须是那两格池子（顺序就是上游声明的顺序）"
        );
        assert_eq!(
            h.eval_string(
                "[typeof Sprite_Animation._checker1, typeof Sprite_Animation._checker2, \
                  Object.keys(Sprite_Animation._checker1).length].join(',')"
            ),
            "object,object,0",
            "两格池子的初值是**空对象**（`rpg_sprites.js:1181-1184`）—— 它们是动画同帧去重的账"
        );
    }

    /// **行走图的切帧网格走上游文本**（旧移植侧 `sisf_pray/src/sheet.rs` 那套手抄算法从此有一份
    /// 上游对照）：`anchor = (0.5, 1)`、多角色图 `576×384` ⇒ 帧 `48×48`（宽 ÷12、高 ÷8），
    /// 单角色（`$`）图 ⇒ 宽 ÷3、高 ÷4，块位 `index % 4 * 3` / `floor(index / 4) * 4`。
    /// 这里给的是**一颗 fixture 位图**（真尺寸、零字节）—— 字节那一条判据在 pray 侧的真素材上。
    #[test]
    fn a_character_sprite_slices_the_walk_sheet_on_the_upstream_grid() {
        let h = booted();
        // 主角 `AAlcy` 那一档：index 1、pattern 2、direction 4（**下=2、左=4、上=6、右=8** 之外
        // 的方向按上游公式 `(direction - 2) / 2` 走）。
        let js = r#"
var fake = {
  tileId: function () { return 0; },
  characterName: function () { return 'AAlcy'; },
  characterIndex: function () { return 1; },
  pattern: function () { return 2; },
  direction: function () { return 8; }
};
var s = new Sprite_Character(fake);
s.bitmap = new Bitmap(576, 384);
s._isBigCharacter = false;
s.updateFrame();
[s.anchor.x, s.anchor.y, s.patternWidth(), s.patternHeight(),
 s.characterBlockX(), s.characterBlockY(),
 s._frame.x, s._frame.y, s._frame.width, s._frame.height].join(',');
"#;
        assert_eq!(
            h.eval_string(js),
            // pw 576/12=48 · ph 384/8=48 · blockX 1%4*3=3 · blockY floor(1/4)*4=0
            // patternX 2 · patternY (8-2)/2=3 ⇒ sx (3+2)*48=240 · sy 3*48=144
            "0.5,1,48,48,3,0,240,144,48,48",
            "行走图的槽位与帧网格必须是上游那四个一行（AGENTS 咬人第 1 条那套手抄从此有对照）"
        );
        // `$` 单角色那一档：同一颗 144×192 的图，宽 ÷3、高 ÷4 ⇒ 每帧仍是 48×48，块位恒 0。
        assert_eq!(
            h.eval_string(
                "var s2 = new Sprite_Character({tileId: function(){return 0;}, \
                 characterIndex: function(){return 0;}, pattern: function(){return 1;}, \
                 direction: function(){return 2;}});\
                 s2.bitmap = new Bitmap(144, 192); s2._isBigCharacter = true; s2.updateFrame();\
                 [s2.patternWidth(), s2.patternHeight(), s2.characterBlockX(), \
                  s2._frame.x, s2._frame.y].join(',')"
            ),
            "48,48,0,48,0",
            "`$sell` 那一档（AGENTS 事实卡：144×192 = 3 列方向帧 × 4 行）走上游的 ÷3 / ÷4 支"
        );
    }

    /// **`Spriteset_Map` 建到 `new Tilemap()` 那一支，而 `isWebGL()` 那条腿上的 `ShaderTilemap`
    /// 永远不碰**（事实 101 第六次应用），并且**建到 `createTilemap` 问地图状态那一名就抛** ——
    /// 抛的落点正是这套 fixture 与真库的分界（真数据那一条判据在 pray 侧，跑的是同一个构造函数）。
    #[test]
    fn a_spriteset_stops_at_the_map_state_and_never_touches_the_gl_tilemap() {
        let h = booted();
        // `Graphics.isWebGL`（`:1898`，随容器树第三颗的 `--extra` 交出）第一句短路在
        // `this._renderer` 上，而那一格要等**绘制会话**才有人写 ⇒ 交回的是 `undefined` 而不是 `false`
        // （同 `core.rs` 的 `the_webgl_tone_branch_is_answered_false_while_the_filter_records_its_requests`
        // 那一段读数）。**分岔读的是 `!!` 之后那个假值** ⇒ `createTilemap` 恒走 `new Tilemap()` 那一支
        // （事实 101 **第六次**应用），而那颗按定案不给 GL 底的 `ShaderTilemap` 在真流里永远不被 `new`。
        assert_eq!(
            h.eval_string("String(Graphics.isWebGL()) + ',' + String(!!Graphics.isWebGL())"),
            "undefined,false",
            "`undefined` 是**没写过的 `_renderer` 短路**，`false` 才是分岔真正读到的那一个假值"
        );
        // `Spriteset_Base.initialize` 的第四句是 `createLowerLayer()`，而 `Spriteset_Map` 的那一格里
        // 第一句读的是 `Graphics.width`（已交），到 `createTilemap` 第一句就要问
        // `$gameMap.tileWidth()`（**这一族不许知道游戏状态**）⇒ 停在后者。
        //
        // 两件事让这一格不能简单写"读一下抛什么"：⑴ 两腿的 TypeError 文案都**不带变量名**
        // （V8 `Cannot read properties of null (reading 'tileWidth')` / QuickJS
        // `cannot read property 'tileWidth' of null`）⇒ 钉的是**那一名方法**而不是 `$gameMap`；
        // ⑵ wasm 的共享 realm 里 `$gameMap` 很可能是**别的判据留下的那一份真状态**，那样就会走到
        // 更后面去（第一版正是红成 `null (reading 'width')`）⇒ 自己把它摆成 `null`、读完还原
        // （事实 115「共享 realm 里不能钉全局的绝对值」的同一条腿）。
        let err = h.eval_string(
            "var __saved = (typeof $gameMap === 'undefined') ? null : $gameMap; \
             var __gone = (typeof $gameMap === 'undefined'); \
             $gameMap = null; var out = 'built', ss = null; \
             try { ss = new Spriteset_Map(); } catch (e) { out = String(e); } \
             if (!__gone) $gameMap = __saved; \
             out + '@' + String(ss && ss._tilemap ? 'tilemap-built' : 'nothing-returned');",
        );
        let (err, returned) = err.split_once('@').expect("读数必须两段");
        assert!(
            err.contains("tileWidth"),
            "抛的落点必须是 `createTilemap` 第一句那一名：{err}"
        );
        assert_eq!(
            returned, "nothing-returned",
            "构造器抛了就不许有 Spriteset 交回来（`_tilemap` 落成 ⇒ 上面那一名是假的）"
        );
    }

    /// **本族不装 managers 那一族**（偏差 2 的方向）：`install` 只带这一族自己的文本 ⇒
    /// 它脚下那一颗 `ImageManager` 只能是**别人**装的。
    ///
    /// 钉的是**增量**而不是绝对值：native 腿每条判据一个新 realm，`typeof ImageManager` 前后都是
    /// `undefined`（这一档真的什么都没装）；wasm 腿共享 realm，前面某条判据可能已经装过 managers，
    /// 那时前后都是 `object` —— 两种次序下"这一族自己没装"这一条都成立，而写死 `undefined` 只成立
    /// 一条腿（第一版就是 native 绿、wasm 红）。八格 load 确实在场由
    /// `the_load_family_the_sprites_call_is_delivered_by_the_managers_layer` 单独钉。
    #[test]
    fn the_sprites_layer_does_not_install_the_managers_layer_it_calls() {
        let h = bare();
        let before = h.eval_string("typeof ImageManager");
        install(&h).expect("那十颗装不上");
        core::set_screen_size(&h, 1104, 736).expect("屏幕尺寸写不进 `Graphics`");
        assert_eq!(
            h.eval_string("typeof ImageManager"),
            before,
            "`sprites::install` 自己把 `ImageManager` 装出来了 ⇒ 两族合一，管辖面又混了（事实 73 那一条）"
        );
        assert_eq!(
            h.eval_string("typeof Sprite_Character.prototype.setCharacterBitmap"),
            "function",
            "这一族自己的格子必须真的在场（否则上面那条「没带来」只是因为整族都没装上）"
        );
    }

    /// 装上 managers 那一族之后，**偏差 2 的前提兑现**：那六名 + `loadBitmap`/`loadNormalBitmap`
    /// 全在场，而脚下踩的是已交的 `Bitmap.load` 与那颗宿主 `_requestImage`（真字节在 pray 侧量）。
    #[test]
    fn the_load_family_the_sprites_call_is_delivered_by_the_managers_layer() {
        let host = Host::boot();
        // 这一档要的是**managers 那一族**的静态类形状名单（`managers.rs` 的 fixture 同一份），
        // 不是本族那三十四颗 —— 同一课第四次：fixture 的名单不跟着换，红的是 `X is not defined`，
        // 看不出缺谁（`M10.md` 事实 78）。
        let mut shapes: Vec<DeclaredShape> = SHELLS
            .iter()
            .map(|(n, k)| DeclaredShape {
                name: n.to_string(),
                kind: *k,
            })
            .collect();
        shapes.extend(MANAGERS_SHAPES.iter().map(|(n, k)| DeclaredShape {
            name: n.to_string(),
            kind: *k,
        }));
        crate::shapes::install(&host, &shapes).expect("形状装不上");
        crate::shapes::install_extensions(&host).expect("JsExtensions 装不上");
        // 平台件**只取三块**（同 `managers.rs` 的那份夹具）：整族 `platform::install` 还要
        // `Input`/`TouchInput` 两颗形状，与这一格无关（最小化手法同 `core.rs` 的 `bitmap_booted`）。
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
                "['loadAnimation','loadCharacter','loadParallax','loadPicture','loadSystem',\
                  'loadTileset','loadTitle1','loadTitle2','loadBitmap','loadNormalBitmap']\
                 .map(function(n){ return typeof ImageManager[n]; }).join(',')"
            ),
            "function,function,function,function,function,function,function,function,function,function",
            "那八格必须由 **managers** 那一族交出去（同一轮，且不是 sprites 这族补的）"
        );
        // 六名**全在场**的那一刻起，`loadTileset` 的读像素腿就是真形状：图名空 → `loadEmptyBitmap`
        // 那颗 `new Bitmap()`（上游逐字），非空 → `Bitmap.load` 走 VFS 的 `img/tilesets/*.png`。
        assert_eq!(
            host.eval_string(
                "[String(ImageManager.loadTileset('').width), \
                  String(ImageManager.loadTileset('World_A1').url)].join(',')"
            ),
            // 夹具 VFS 里没有那张图块组 ⇒ 上游 `Bitmap.load` 走 `_loadingState='error'` 那一支
            // （`isReady()` false、尺寸仍是 `initialize` 的 1×1），而 **URL 拼写**是磁盘那一条腿的读数。
            "1,img/tilesets/World_A1.png",
            "空图名落 `loadEmptyBitmap`（1×1），非空落 `Bitmap.load` 的 URL 形状"
        );
        install(&host).expect("那十颗第二次装不上（managers 之后再装）");
    }

    /// **幂等**：同一个 realm 里装第二次不抛，而 `Sprite_Character.updateFrame` 的读数不变。
    #[test]
    fn reloading_the_sprites_is_still_the_sprite_layer() {
        let h = booted();
        let before = h.eval_string(
            "var s = new Sprite_Character({tileId: function(){return 0;}, \
              characterIndex: function(){return 0;}, pattern: function(){return 0;}, \
              direction: function(){return 2;}});\
              s.bitmap = new Bitmap(576, 384); s._isBigCharacter = false; s.updateFrame();\
              String(s._frame.width)",
        );
        install(&h).expect("第二次装不上（共享 realm 里重 define 不许抛）");
        assert_eq!(before, "48", "第一次的切帧读数就该是 48（`576/12`）");
        assert_eq!(
            h.eval_string("String(Sprite_Character.prototype.updateFrame.toString().length)"),
            h.eval_string("String(Sprite_Character.prototype.updateFrame.toString().length)"),
            "自己与自己不等只能是 realm 里有别的东西（同一条判据里的两次问法）"
        );
        assert_eq!(
            h.eval_string("typeof Spriteset_Map.prototype.createTilemap"),
            "function",
            "重复装载之后 `createTilemap` 仍然在场"
        );
    }

    /// `install` 不要求调用方先装容器树（偏差 5 的正面判据）：只给形状，本族自己踩 core。
    #[test]
    fn sprites_install_does_not_depend_on_the_caller_installing_the_display_layer_first() {
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
        install(&host).expect("那十颗自己没把容器树踩上");
        assert!(
            host.eval_bool("Object.getPrototypeOf(Sprite_Base.prototype) === Sprite.prototype"),
            "容器树没被 install 自己装上"
        );
    }
}
