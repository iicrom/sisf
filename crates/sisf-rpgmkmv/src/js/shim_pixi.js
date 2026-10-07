// 自创的底：**`PIXI` 那一族的底**。不是上游文本 ⇒ 逐字尺子不比它，改由
// `objects-verbatim.py` 的 `SHIM_BASE` 双向白名单数（多一格少一格都红，同 `HOME_MADE` 那条纪律）。
//
// 为什么要有这一层：方向定案第 1 条说 `libs/pixi` **不作为 JS 面存在**，而
// `rpg_core.js` 的十四颗渲染类里有 **13 颗**的链基类写的就是 `PIXI.…`
// （尺子 `tools/probes/pixi-face.py --chains`：`PIXI.Point` · `PIXI.Rectangle` · `PIXI.Sprite` ·
// `PIXI.Container`×6 · `PIXI.extras.PictureTilingSprite` · `PIXI.filters.ColorMatrixFilter`，
// 只有 `ShaderTilemap` 挂在自己人 `Tilemap` 上）。上游文本里的 `Object.create(PIXI.Container.prototype)`
// 与 `PIXI.Point.call(this, x, y)` 在装载那一刻就要读得到这些名字 ⇒ 交不出这一层，那 13 颗一颗都装不上。
//
// 这一层**只做数据形状，不做渲染**：像素与场景图住在引擎那三只把手上
// （`Atlas::new_target` + `Renderer` 的绘制会话 + `TextEngine`，核实过程写在 `M10.md` 事实 75），
// 每一颗的体只回答上游 `X.call(this, …)` 那一句真正会写下的字段。
// 加成员的唯一合法理由是有**量出来的**引用（`pixi-face.py` 那一族），不是照 PIXI 的常识补。
//
// 双端：wasm 腿一整个进程共享一个 realm ⇒ 每一段都是"没有才给"，装第二次不许抛
// （`M10.md`「这一族的规矩」的第 4 条；`the_pixi_shim_base_installs_twice_in_one_realm` 钉这一格）。
// `PIXI.Point`/`PIXI.Rectangle` 本身是普通赋值（不是 `defineProperty`），重assign 天然幂等。

// 命名空间：只在没有的时候新建。以后每一颗渲染类的底都往这同一颗对象上挂，
// 而不是各自 `globalThis.PIXI = {}` —— 后者会把前面那几颗抹掉（共享 realm 里就是"上一批判据
// 装过、这一批把名单清空"的那种假绿）。
if (!globalThis.PIXI) {
    globalThis.PIXI = {};
}

// `PIXI.Point`（PIXI v4 的构造体就是这两行字段直写；上游 `Point.prototype.initialize`
// （`rpg_core.js:628-630`）`call` 的就是它）。
PIXI.Point = function Point(x, y) {
    this.x = x || 0;
    this.y = y || 0;
};

// `PIXI.Rectangle`（同上，四行；上游 `Rectangle.prototype.initialize`（`:664-666`）`call` 它，
// 而 `Window` 的 `_windowRect`/`_paddingRect` 与 `Sprite` 的 `_frame` 读的就是这四格）。
PIXI.Rectangle = function Rectangle(x, y, width, height) {
    this.x = x || 0;
    this.y = y || 0;
    this.width = width || 0;
    this.height = height || 0;
};

// `PIXI.Rectangle.prototype.clone`（**第六颗新底，理由只有一处读者**）：
// `TilingSprite.prototype._refresh`（`rpg_core.js:6079`）第一句就是 `var frame = this._frame.clone();`
// —— 而 `_frame` 是 MV 的 `new Rectangle()`，MV 那一颗自己只有 `initialize`
// （`Rectangle.prototype = Object.create(PIXI.Rectangle.prototype)`，`:3936`）⇒ `clone` 是**基类必须给的**。
// 体照 PIXI v4（`new Rectangle(this.x, this.y, this.width, this.height)`）：交的是**同一对数**的新矩形，
// 所以 `texture.frame` 与 `_frame` 之后互不牵扯（上游 `_refresh` 就是靠这一点才敢改 `frame.width`）。
// 其余读者点名：`core_sprite.js` 那四处 `clone()` 喊的是 `Color`/`Tone`（它们自己那一格，随各自的类），
// 而 `rpg_sprites.js:46` 与 `rpg_windows.js:5046` 那两处喊的是 `Array.prototype.clone`（`shapes.rs` 那颗扩展，
// 尺子 `tiling-sprite-face.py` 第②段量的就是这三档）。
PIXI.Rectangle.prototype.clone = function() {
    return new PIXI.Rectangle(this.x, this.y, this.width, this.height);
};

// `PIXI.SCALE_MODES`：上游 `_createBaseTexture`（`rpg_core.js:780-784`）与 `smooth` 的 setter
// （`:1084-1101`）**只写不读**这两格，所以这一档是给"写了要有个地方放"的，不是给读数的。
// 值照 PIXI v4（`NEAREST: 0` / `LINEAR: 1`）。它不是 `= function` ⇒ `objects-verbatim.py` 的
// `SHIM_BASE` 数不到它，由判据 `the_bitmap_base_texture_shim_carries_the_two_scale_modes` 数
// （与 `_imageCache`/`emptyRectangle` 同一条"数据形状由判据数"的纪律）。
PIXI.SCALE_MODES = { NEAREST: 0, LINEAR: 1 };

// `PIXI.RENDERER_TYPE`（第三格随 `--extra` 交出的 `Graphics.isWebGL`，`rpg_core.js:1898` 体里读的那一颗）：
// 那一句是 `this._renderer && this._renderer.type === PIXI.RENDERER_TYPE.WEBGL` —— 短路在前，
// 所以**今天读不到这一档**（`_renderer` 要等绘制会话才有人写），但名字必须在场：一旦 `_renderer`
// 有了值而这一档缺了，那一句就是 `TypeError` 而不是 `false`，而它守着的正是"色调走 canvas 还是
// webgl"那条分岔（`rpg_sprites.js:2161`/`:2218` 两处，尺子 `tone-and-graphics.py` 第②段）。
// 值照 `libs/pixi.js:8305`（`UNKNOWN: 0` / `CANVAS: 1` / `WEBGL: 2`）。同 `SCALE_MODES` 一条腿：
// 它是**数据形状**（右值不是函数）⇒ `SHIM_BASE` 数不到，由那条逐名钉 `PIXI` 成员名单的判据数。
PIXI.RENDERER_TYPE = { UNKNOWN: 0, CANVAS: 1, WEBGL: 2 };

// `PIXI.BaseTexture`（`_createBaseTexture` 的 `new` 落点）：**这颗底今天只做尺寸与旗标**，
// 它代表的是"这一格位图在渲染端有一处地方"，而**把像素推上去那一步还没有**（那是绘制会话那一格，
// `M10.md` 事实 75 的第⑵只）。所以下面那颗 `update()` 是**空的**：上游 `checkDirty`（`:1649-1654`）
// 喊它，就是在等这一步。空转的代价由判据点名（`a_loaded_bitmap_clears_its_dirty_flag_without_an_upload`），
// 不是悄悄过。
PIXI.BaseTexture = function BaseTexture(source) {
    this.source = source || null;
    this.width = source ? source.width : 0;
    this.height = source ? source.height : 0;
    this.mipmap = false;
    this.scaleMode = 0;
};

// `PIXI.BaseTexture.prototype.update`：上游在 `checkDirty()` 里喊的那一格（`rpg_core.js:1651`）。
PIXI.BaseTexture.prototype.update = function() {};

// **第四小格（`ShaderTilemap`）要的四颗命名空间：只为"装载那一刻读得到"**。
// 上游那四句（`rpg_core.js:5496-5499`，切片器第十类形状随那颗类一起交）是**对 PIXI 的赋值**：
// `PIXI.glCore.VertexArrayObject.FORCE_NATIVE = true;` / `PIXI.settings.GC_MODE = PIXI.GC_MODES.AUTO;`
// / `PIXI.tilemap.TileRenderer.SCALE_MODE = PIXI.SCALE_MODES.NEAREST;` / `…DO_CLEAR = true;`。
// 我们这一层没有 PIXI，那三个 owner 一个都不存在 ⇒ 装载那一刻就是
// `Cannot set properties of undefined`（不是"少一格效果"，是**整份文本装不上**，事实 76 那一条腿）。
//
// 为什么**只给这四颗、不给 `PIXI.tilemap.ZLayer`/`CompositeRectTileLayer`**（尺子
// `../sisf_pray/tools/probes/shader-tilemap-face.py` 第③⑤段）：那两名只在**构造期**被读
// （`_createLayers`，`:5604/:5605/:5610/:5612`），而 `_createLayers` 的唯一入口是
// `new ShaderTilemap()`，`rpg_sprites.js:2300` 那一句又整个裹在 `if (Graphics.isWebGL())` 里
// ⇒ 真流走不到（事实 101 第五次应用）。给它们造假底 = 交不出任何读数的空壳，还会把"这条腿走不到"
// 从可见变成不可见。所以这一族的账由判据
// `the_shader_tilemap_constructor_stops_at_the_gl_layer_names` 钉成**抛**这个读数。
//
// 四颗全是**数据形状**（右值不是函数）⇒ `objects-verbatim.py` 的 `SHIM_BASE` 数不到，
// 由判据数（同 `PIXI.SCALE_MODES`/`PIXI.extras` 一条腿）。`GC_MODES` 的两格值照上游 PIXI v4
// 那份实现（`libs/pixi.js:8454-8457` 实测 `AUTO: 0, MANUAL: 1`），`FORCE_NATIVE` 的默认值
// 同理照 `:862` 的 `false` —— 上游那一句要把它改成 `true`，所以初值必须**是**那个 `false` 的位置，
// 否则"改了"这一格量不出来。
// 每一颗都写成「没有才给」：共享 realm 里装第二次不许把第一次的值抹掉
// （`M10.md`「这一族的规矩」第 4 条）。
PIXI.glCore = PIXI.glCore || {};
PIXI.glCore.VertexArrayObject = PIXI.glCore.VertexArrayObject || { FORCE_NATIVE: false };
PIXI.settings = PIXI.settings || {};
PIXI.GC_MODES = { AUTO: 0, MANUAL: 1 };
PIXI.tilemap = PIXI.tilemap || {};
PIXI.tilemap.TileRenderer = PIXI.tilemap.TileRenderer || {};
