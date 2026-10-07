// 自创的底：**`PIXI.extras` 那一层**（M10.5 第五小格第六格第三小格，2026-10-06）。
// 不是上游文本 ⇒ 逐字尺子不比它的内容，改由 `objects-verbatim.py` 的 `SHIM_BASE` **双向**名单数
// （多一颗少一颗都红，与 `HOME_MADE` 同一条纪律）。
//
// 为什么现在要这一层：上游那颗 `TilingSprite` 的**第一句**就是
// `TilingSprite.prototype = Object.create(PIXI.extras.PictureTilingSprite.prototype)`
// （`rpg_core.js:5910`），而 `PIXI.extras` 这个名字在**已交付的文本里一颗都没有**
// （尺子 `tools/probes/tiling-sprite-face.py` 第③段：上游四文件合计 5 处 `PIXI.extras`，全在
// `TilingSprite` 那一段里）⇒ 交不出这一层，`Object.create(undefined)` 在**装载那一刻**就抛，
// 而 `Spriteset_Map` 那条腿上有四处 `new TilingSprite()`（视差 + 两张背景 + `:2378` 那一格的岔）。
//
// 成员名单**只由量出来的引用增加**（尺子第②段：这一颗的体里 `this.<名>` 读到的名，
// 上游四文件与 `PIXI.Sprite`/`PIXI.Container` 那两层底**都没有**的，只有 **`tilePosition`** 一名）。
// `texture`（×8）与 `children`（×1）从已有的底上下来，一颗都不重新给；
// 而 `_bitmap`/`_frame`/`origin`/`spriteId`/`tilingTexture` 那 11 名是**这颗自己写的** ⇒ 给初值就是第二个真相。

// 命名空间：**只在没有的时候新建**（同 `PIXI.filters` 那一条腿）。这一格是**数据形状**
// （右值不是函数 ⇒ 逐字尺子数不到），账由 `the_tiling_sprite_base_is_the_measured_names_and_clone` 数。
PIXI.extras = PIXI.extras || {};

// `PIXI.extras.TilingSprite`（PIXI v4 的平铺精灵）：**链照 v4**（`→ PIXI.Sprite → PIXI.Container`），
// 于是 `texture`/`anchor`/`children` 全部从已有的底上下来。体里只有一句是量出来的：
// `tilePosition` —— 上游 `TilingSprite.prototype.updateTransform`（`rpg_core.js:6054-6055`）
// 写的是 `this.tilePosition.x = Math.round(-this.origin.x)`，**读的是子格**，所以这颗必须
// 是"在场且能被改写的对象"（与 `pivot`/`scale`/`anchor` 同一手法），不是访问器。
// `tileScale`/`_uShader` 那一族仍然不给：四文件与 103 条启用插件**一处都没读过**。
PIXI.extras.TilingSprite = function TilingSprite(texture) {
    PIXI.Sprite.call(this, texture);
    this.tilePosition = { x: 0, y: 0 };
};

PIXI.extras.TilingSprite.prototype = Object.create(PIXI.Sprite.prototype);
PIXI.extras.TilingSprite.prototype.constructor = PIXI.extras.TilingSprite;

// `PIXI.extras.TilingSprite.prototype.updateTransform` 是上游那第三处别名赋值的**源**
// （`rpg_core.js:6063`：`TilingSprite.prototype.updateTransformTS = …`），而它的读者是 MV 自己那句
// `updateTransform` 收尾那一行 `this.updateTransformTS()`（`:6060`）—— **canvas 与 webgl 两条腿都要走**，
// 缺了它 MV 的变换那一喊就是 `updateTransformTS is not a function`。
// 交**空体并点名**：真正的变换发生在「宿主那侧的真绘制」那一格（与 `PIXI.Container` 那三格同一档）。
PIXI.extras.TilingSprite.prototype.updateTransform = function() {};

// `PIXI.extras.PictureTilingSprite`：上游 `initialize` 第一句 `PIXI.extras.PictureTilingSprite
// .call(this, texture)`（`:5916`）读的就是它 ⇒ **建立那一刻**要在场。v4 里它就是"TilingSprite 加宽高"，
// 而那两格（`_width`/`_height`）MV 的 `initialize` 自己写 ⇒ 体里只剩一句转喊基类。
PIXI.extras.PictureTilingSprite = function PictureTilingSprite(texture, width, height) {
    PIXI.extras.TilingSprite.call(this, texture, width, height);
};

PIXI.extras.PictureTilingSprite.prototype = Object.create(PIXI.extras.TilingSprite.prototype);
PIXI.extras.PictureTilingSprite.prototype.constructor = PIXI.extras.PictureTilingSprite;

// 那两块**渲染会话**是上游另两处别名赋值的源（`:5934`/`:5935` ⇒ `_renderCanvas_PIXI`/
// `_renderWebGL_PIXI`），读者是渲染器（`rpg_core.js:7074`/`:7089` 喊 `child.renderCanvas`，
// `:7141`/`:7152` 喊 `renderWebGL`）⇒ 缺了不抛，而是别名落成 `undefined`、插件那一拍才炸
// （与 `PIXI.Sprite._renderCanvas` 同一手法：交空体并点名）。
// ⚠ `_renderWebGL` 这一条腿今天**走不到**：`Graphics.isWebGL()` 在我们这一层恒为假
// （`M10.md` 事实 101 的第四次应用）⇒ 它只为"别名赋值读得到东西"而存在。
PIXI.extras.PictureTilingSprite.prototype._renderCanvas = function(renderer) {};
PIXI.extras.PictureTilingSprite.prototype._renderWebGL = function(renderer) {};
