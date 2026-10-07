// 自创的底：**`PIXI.Container` 那一层**（M10.5 第五小格第一格，2026-10-05）。
// 不是上游文本 ⇒ 逐字尺子不比它的内容，改由 `objects-verbatim.py` 的 `SHIM_BASE` **双向**名单数
// （多一颗少一颗都红，与 `HOME_MADE` 同一条纪律）。
//
// 为什么现在要这一层：`rpg_core.js` 十四颗渲染类里有 **13 颗**的链基类写的就是 `PIXI.…`
// （尺子 `tools/probes/pixi-face.py --chains`），其中 **6 颗**挂的就是 `PIXI.Container`
// （`Tilemap`/`ScreenSprite`/`Window`/`WindowLayer`/`ToneSprite`/`Stage`），而 `Scene_Base extends Stage`
// ⇒ 交不出这一层，真流的那只场景一颗都装不上（装载那一刻 `Object.create(PIXI.Container.prototype)`
// 就抛，不是运行期才红）。
//
// 成员名单**只由量出来的引用增加**（尺子 `tools/probes/container-face.py`：13 颗链类的体里读的
// `this.<名>` 中、上游四文件从没写过定义的 **17 名**）⇒ 这一格只交 `Stage` 今天真读得到的那几格，
// 剩下的（`worldTransform`/`tilePosition`/`calculateVertices`/`pluginName`）随各自的渲染类
// 一起加，**一颗一个理由**（`anchor` 已经加过，但加在 `PIXI.Sprite` 那一颗上而不是这里：
// 它的读者全在 `Sprite` 那一族上，`rpg_sprites.js:198` 起，第五格 `window-face.py` 第③段量的
// 就是这一条 —— 挂在 `Container` 上会让每一只容器都多一颗 PIXI 常识里的字段）。
// ⚠ `viewport` 那一名**故意不给**：`this.viewport` 在 `rpg_core.js:7349` 读一次而全工程
// （四文件 + 103 条启用插件）**从没写过** ⇒ 上游拿到的就是 `undefined`，给了就是把上游的形状改掉
// （同 `CacheMap` 那条"上游坏形状照交"的纪律）。
//
// **不做变换、不做渲染**：`updateTransform`/`renderCanvas`/`renderWebGL` 是**空体**，它们代表的是
// 「宿主那侧的真绘制」那一格还没到（`&mut Renderer` 只在 `App::draw` 那一拍借得到，`M10.md` 事实 82）。
// 空着的代价由判据点名（`the_container_tree_is_structure_only_and_the_draw_session_is_named`），
// 不是悄悄过 —— 与 `PIXI.BaseTexture.prototype.update` 同一手法。
//
// 双端：wasm 腿一整个进程共享一个 realm ⇒ 每一颗都是"重新 define 同一颗全局"，装第二次不许抛
// （`the_container_shim_installs_twice_in_one_realm` 钉这一格）。

// `PIXI.Container`（PIXI v4 的 DisplayObjectContainer）：**今天只交"树"那一半** ——
// 上游 6 颗挂它的类里，`Stage`/`WindowLayer`/`Window` 在装载与建立阶段真正动的是 `children` 与
// `parent`（`addChild` 11 处、`children` 18 处，尺子第③段），而变换那三格属于绘制会话。
// 字段初值只给**量出来被读过**的：`x`/`y`/`rotation`/`alpha`/`visible`/`renderable`/`pivot`
// （`--child` 那一档读到 `child.x`/`child.y`/`child.rotation`/`child.alpha`/`child.visible`/
// `child.renderable`）；上游自己会写的那些（`opacity`/`blendMode`/`filters`/`_bitmap` 那一族 162 名）
// **一个都不给** —— 给了就是第二个真相，而且"谁写的"从此读不出来。
// `scale` 是第三格加进来的：理由不是"PIXI 有这一格"，而是**这一格交出的上游文本自己写它**
// （`ScreenSprite.prototype.anchor` 的 getter，`rpg_core.js:6186-6187` 那两句 `this.scale.x = 1`），
// 而需求面是量出来的（尺子 `tools/probes/tone-and-graphics.py` 第④段：core 14 处 / 启用插件 208 处，
// 另有 `WindowLayer._windowSpriteContainer.scale.y` 与 `Spriteset_Base.updatePosition` 那两支）。
// 与 `pivot` 同一档：**只是那颗能被写的对象**，变换在绘制会话那一格才发生。
PIXI.Container = function Container() {
    this.children = [];
    this.parent = null;
    this.x = 0;
    this.y = 0;
    this.rotation = 0;
    this.alpha = 1;
    this.visible = true;
    this.renderable = true;
    this.pivot = { x: 0, y: 0 };
    this.scale = { x: 1, y: 1 };
};

// `addChild`：PIXI v4 就是"接到队尾"并认下父亲，交回进来的那颗
// （上游 `Stage`/`Window`/`Spriteset_*` 全都读这个返回值，`return {Object}` 的注释不是装饰）。
PIXI.Container.prototype.addChild = function(child) {
    return this.addChildAt(child, this.children.length);
};

// `addChildAt`：越界**照 PIXI 抛**（`childIndex out of range`）—— 上游 `WindowLayer` 的
// `addChildAt(child, index)` 用的就是这一档，静默夹住就会把"插错位置"读成"插对了"。
PIXI.Container.prototype.addChildAt = function(child, index) {
    if (index >= 0 && index <= this.children.length) {
        if (child.parent) {
            child.parent.removeChild(child);
        }
        child.parent = this;
        this.children.splice(index, 0, child);
        return child;
    }
    throw new Error('sisf: PIXI.Container.addChildAt 的位子越界（' + index +
        '，children 长 ' + this.children.length + '）');
};

PIXI.Container.prototype.removeChild = function(child) {
    var index = this.children.indexOf(child);
    this.children.splice(index, 1);
    child.parent = null;
    return child;
};

// 从队尾往前摘：PIXI v4 就是这个次序（正着摘会因为下标一边动一边跳）。
PIXI.Container.prototype.removeChildren = function() {
    for (var i = this.children.length - 1; i >= 0; i--) {
        this.removeChild(this.children[i]);
    }
};

// 变换与渲染那三格：**空体，点名欠着**。上游确实喊（`PIXI.Container.prototype.updateTransform.call`
// 3 处、`renderCanvas`/`renderWebGL` 各 1 处被 `WindowLayer` 那两格别名），而算出来的
// `worldTransform` 只有绘制会话读它 ⇒ 今天画的是"树的结构"，不是"树上的位置"。
PIXI.Container.prototype.updateTransform = function() {};

PIXI.Container.prototype.renderCanvas = function() {};

PIXI.Container.prototype.renderWebGL = function() {};

// `this.on('removed', this.onRemoveAsAChild)`（`rpg_core.js:6970`，`WindowLayer.initialize` 那一格）：
// 我们这一层没有事件系统（PIXI 那套 `EventEmitter` 不为渲染之外的任何东西服务），所以这一格
// **收下并忘掉**。收不下就是装载那一刻抛，而它喊的正是"摘掉孩子之后清画布"那一族——
// 那一族的后果在绘制会话那一格一起接（`M10.md`「第五小格开工」定案 3）。
PIXI.Container.prototype.on = function() {};

// `PIXI.Texture`（第二格 `Sprite` 要的那一颗：`new PIXI.Texture(new PIXI.BaseTexture())`，
// `rpg_core.js:3955`）。上游动的是三格：`frame`（赋值，`:4204`/`:4209`/`:4215`）、
// `baseTexture`（赋值，`:4203`/`:4207`）与 `_updateID`（`this.texture._updateID++`，`:4218` ——
// 上游用它告诉渲染端"这张纹理脏了"，我们没有渲染端，所以它是**一条只写不读的账**，
// 由判据 `the_sprite_from_upstream_text_records_a_frame_a_tint_buffer_and_an_update_id` 数）。
// `frame` 的初值取基纹理的尺寸：`_refresh` 最后那一支（`:4212-4215`）会 `Math.max` 把
// `baseTexture.width/height` 抬到帧的尺寸，**那两格必须是数**，否则就是 `NaN`。
PIXI.Texture = function Texture(baseTexture) {
    this.baseTexture = baseTexture || new PIXI.BaseTexture();
    this.frame = new PIXI.Rectangle(0, 0, this.baseTexture.width, this.baseTexture.height);
    this._updateID = 0;
};

// `PIXI.Sprite`（`Sprite.prototype = Object.create(PIXI.Sprite.prototype)`，`:3949` 与
// `PIXI.Sprite.call(this, texture)`，`:3956`）。**只交量出来被写的到的东西**：`texture` 是构造体
// 那一句自己写的，`anchor` 是**上一格还没有、这一格才有理由**的那一颗 —— `Window` 的
// `_refreshArrows`（`rpg_core.js:6779-6785` 那四处）与 `_refreshPauseSign`（`:6799-6800` 那两处）
// 写 `X.anchor.x/.y` 共 **6** 处，而全仓 `.anchor = new …`（整颗换掉）是 **0** 处
// （尺子 `window-face.py` 第③段：改写 core 6 / scenes 2 / sprites 32 / 启用插件 77，整颗替换 0）
// ⇒ 交的是**一颗在场且能被改写的对象**，不是访问器：上游从不替换它，给访问器就是造第二个真相。
// `tint`/`blendMode`/`shader`/`mask`/`filters` 仍然**不给**：它们在 `Sprite` 这一颗里只出现在
// **跳掉的**那五格（`_renderCanvas`/`_renderWebGL`/`_speedUpCustomBlendModes` 与两条 `_PIXI` 别名，
// `M10.md`「第五小格第二格开工」的切法表），随各自的格子一起加 —— 预交就是"照 PIXI 的常识补"。
PIXI.Sprite = function Sprite(texture) {
    PIXI.Container.call(this);
    this.texture = texture || new PIXI.Texture();
    this.anchor = { x: 0, y: 0 };
};

// 链：`Sprite` 的基类要真是那颗 `Container`（`addChild`/`children`/`pivot` 全靠这一条）。
PIXI.Sprite.prototype = Object.create(PIXI.Container.prototype);
PIXI.Sprite.prototype.constructor = PIXI.Sprite;

// 上游那两条**别名赋值**的源（`rpg_core.js:4330-4331`：
// `Sprite.prototype._renderCanvas_PIXI = PIXI.Sprite.prototype._renderCanvas;`）：**装载那一刻**
// 就要读得到这两格，缺了就是 `undefined` 而不是抛 —— 于是插件日后别名 `_renderCanvas_PIXI`
// 拿到的是 undefined，喊它那一拍才炸（比装载那一刻晚一整局，且炸在插件里）。
// 所以这里交**空体并点名**：它们代表的是「宿主那侧的真绘制」那一格（与上面 `Container` 那三格
// 同一档，`M10.md`「第五小格第二格开工」的切法表第⑵行）。
PIXI.Sprite.prototype._renderCanvas = function() {};

PIXI.Sprite.prototype._renderWebGL = function() {};

// `PIXI.filters.VoidFilter`（两颗类的**静态**数据形状就在装载那一刻 `new` 它：
// `Sprite.voidFilter = new PIXI.filters.VoidFilter()`，`:3952`；`WindowLayer.voidFilter`，`:6977`）。
// 空体：它是"没有滤镜"的那个哨兵，被赋给 `sprite.filters` 才起作用，而那一族赋值在**跳掉**的
// 渲染格子里 ⇒ 今天这颗只需要能被 `new`。
PIXI.filters = PIXI.filters || {};

PIXI.filters.VoidFilter = function VoidFilter() {};

// `PIXI.Graphics`（第三格起，`rpg_core.js` 里两处 `new PIXI.Graphics()`：`ScreenSprite:6148` 与
// `WindowLayer._windowMask:6959`；启用插件那一侧 **0** 处 —— 尺子 `tone-and-graphics.py` 第①段）。
// 交的是**已交付文本喊到的那四名**：`clear`/`beginFill`/`drawRect`（第三格的 `ScreenSprite.setColor`）
// 加 `endFill`（第四格 `WindowLayer.initialize:6962` 那三句连写 `beginFill→drawRect→endFill`）。
// 还剩 `_currentBounds`/`boundsDirty` 两名（`:7163-7164`）—— 它们只出现在**跳掉的** `_maskWindow` 里
// ⇒ 随「宿主那侧的真绘制」那一格一起加，这一族欠的现在只剩那一格里的那两名。
//
// 落法与位图那一族同一条腿：**记，不画**。`drawRect` 把这一笔压进 `graphicsData`（一颗
// `{type, shape, fillColor, fillAlpha}`），因为 `setColor` 每一次颜色变化都重画那一整块屏
// （`-Graphics.width*5, -Graphics.height*5, width*10, height*10`），而"谁在什么时候要哪一块quad"
// 只有从记录里才读得出来 —— `WindowLayer` 正是从 `graphicsData[0].shape` **拿到那颗矩形的引用**
// （`this._windowRect = …shape`，`:6963`）再改写它的四格 ⇒ `shape` 必须是**同一颗对象**，
// 复制一份就把上游"改掩膜就是改那条引用"的那一手弄断了。
// `blendMode` 不在这里给初值：`ScreenSprite` 的 `blendMode` 那一对访问器**自己写自己读**
// （真流里 0 个写点，尺子第①段那一族的读点就是访问器本身），给了就是把上游的 `undefined` 改成数。
PIXI.Graphics = function Graphics() {
    this.graphicsData = [];
    this._fill = null;
};

PIXI.Graphics.prototype.clear = function() {
    this.graphicsData = [];
    this._fill = null;
};

PIXI.Graphics.prototype.beginFill = function(color, alpha) {
    this._fill = { color: color || 0, alpha: alpha === undefined ? 1 : alpha };
};

// 上游只用到矩形（`drawRect`），所以这一格交出的记录里 `type` 恒 `'RECT'`，
// 而 `shape` 必须是一颗**能被改写的对象**：`WindowLayer` 拿到 `graphicsData[0].shape` 之后
// 直接写它的 `x`/`y`/`width`/`height`（`rpg_core.js:6963` 那一格起）。
PIXI.Graphics.prototype.drawRect = function(x, y, width, height) {
    var fill = this._fill || { color: 0, alpha: 1 };
    this.graphicsData.push({
        type: 'RECT',
        shape: { x: x, y: y, width: width, height: height },
        fillColor: fill.color,
        fillAlpha: fill.alpha
    });
};

// `endFill`（第四格，`WindowLayer.initialize:6962` 那三句连写的第三句）：PIXI 那一侧就是"这一段
// 填充结束了"，我们这一层的记录里对应的就是**把当前填充放下**，于是后面没有 `beginFill` 的
// `drawRect` 落进 `{color:0,alpha:1}` 那一支（`ScreenSprite.setColor` 每次都是 `clear` 之后
// 立刻 `beginFill`，所以它不受这一格影响）。
PIXI.Graphics.prototype.endFill = function() {
    this._fill = null;
};

// `PIXI.filters.ColorMatrixFilter`（`ToneFilter` 的链基类，`rpg_core.js:7447` 与 `:7444` 那两句）。
// ⚠ **这一支在真流里走不到**，而且是量出来的（尺子第②段）：色调有两只换法，
// `Spriteset_Base.createToneChanger`（`rpg_sprites.js:2161`）与 `updateTone`（`:2218`）都按
// `Graphics.isWebGL()` 分岔，而我们这一侧它答 **false** —— 根在 `Graphics.hasWebGL`
// （`rpg_core.js:1910`）问的是 `document.createElement('canvas').getContext('webgl')`，
// `src/js/shim_dom.js` 交出的那颗对 `'2d'` 之外的每一种都回 `null`（事实 97 第⑶条）。
// 所以走的是 canvas 那一支（`ToneSprite`），而 `ToneFilter` 是 webgl 那一支 ——
// **与 `Html5Audio` 同形**（`M10.md` M10.4 第三格那两条死路证据）。
// 但这一格**照交**，不是跳：待定项 2 的定案是"接满真流闭包、不记降级"，
// 而交出的 `ToneFilter` 那三格喊 `hue`/`saturate`/`_loadMatrix` 三名，装载那一刻链也要读得到这颗。
//
// 三名的落法是**记请求，不折矩阵**：`_loadMatrix` 存下最后那一份（MV 自己写的矩阵是二十格、
// 偏移在 4/9/14/19，`rpg_core.js:7485-7491`），`hue`/`saturate` 把请求逐名压进 `colorOps`。
// 为什么不折：折叠要的是 PIXI 的 `_multiply` + `_colorMatrix` 那两格（`libs/pixi.js:32657` 起，
// 里面还有 `Float32Array` 与偏移再除 255 那一手），而定案第 1 条不把 `libs/pixi` 当 JS 面；
// 更关键的是**这一族的矩阵只有一个消费者 —— WebGL 着色器**（`uniform float m[20]`），
// 我们这一支恒不走它，折出来的矩阵没有任何东西能验证。⇒ 记名，交 M10.6 的覆盖账点名，
// 而"走不到"这一格由判据钉住（不是靠这段注释）。
PIXI.filters.ColorMatrixFilter = function ColorMatrixFilter() {
    this.matrix = [
        1, 0, 0, 0, 0,
        0, 1, 0, 0, 0,
        0, 0, 1, 0, 0,
        0, 0, 0, 1, 0
    ];
    this.colorOps = [];
};

PIXI.filters.ColorMatrixFilter.prototype._loadMatrix = function(matrix, multiply) {
    this.colorOps.push('matrix|' + (multiply ? 1 : 0) + '|' + matrix.join(','));
    this.matrix = matrix.slice();
};

PIXI.filters.ColorMatrixFilter.prototype.hue = function(rotation, multiply) {
    this.colorOps.push('hue|' + (rotation || 0) + '|' + (multiply ? 1 : 0));
};

PIXI.filters.ColorMatrixFilter.prototype.saturate = function(amount, multiply) {
    this.colorOps.push('saturate|' + (amount || 0) + '|' + (multiply ? 1 : 0));
};
