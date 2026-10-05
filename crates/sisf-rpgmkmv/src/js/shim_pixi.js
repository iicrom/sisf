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
PIXI.Point = function(x, y) {
    this.x = x || 0;
    this.y = y || 0;
};

// `PIXI.Rectangle`（同上，四行；上游 `Rectangle.prototype.initialize`（`:664-666`）`call` 它，
// 而 `Window` 的 `_windowRect`/`_paddingRect` 与 `Sprite` 的 `_frame` 读的就是这四格）。
PIXI.Rectangle = function(x, y, width, height) {
    this.x = x || 0;
    this.y = y || 0;
    this.width = width || 0;
    this.height = height || 0;
};
