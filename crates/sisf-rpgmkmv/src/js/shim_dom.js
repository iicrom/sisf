// 自创的底：**一颗离屏画布**（`document.createElement('canvas')`，M10.5 第五小格第二格，2026-10-05）。
// 不是上游文本 ⇒ 逐字尺子不比它，由 `objects-verbatim.py` 的 `SHIM_BASE` 双向名单数。
//
// ## 为什么这一格要交 `document`
//
// `platform.rs` 模块头第 25 节那条"有意不交 `document`"的判词**今天仍然成立**，这一格没有推翻它，
// 而是把范围收到**一颗离屏缓冲**上：`rpg_core.js` 里 `document.createElement('canvas')` 共
// **9 处**（尺子 `container-face.py` 数过），容器树这一族真的走到的是两处 ——
// `Sprite.prototype._createTinter`（`:4253`，tint 的缓冲）与 `WindowLayer` 的 `_tempCanvas`
// （`:7051`，窗的遮罩缓冲）。其余七处分别在 `Bitmap._createCanvas`（**我们的宿主底已经绕开 DOM**，
// 见 `core.rs` 的 `BITMAP_HOST_JS` 那一格）、`Bitmap.blur`（**没交**的那格）、以及 `Graphics`
// 的 DOM 那一族（`:1911`/`:2099`/`:2406`/`:2475`/`:2527`，**没交**的那八句）⇒
// 启用插件那侧 **0 条**。所以交的不是"一个 DOM"，是"一个能装 `Sisf2DContext` 的缓冲"。
//
// ## 为什么不会把特性探测带上浏览器那条路
//
// 这是 `platform.rs` 当初不交 `document` 的**唯一理由**，所以这一格把它钉成判据
// （`the_offscreen_canvas_answered_by_the_platform_is_the_one_the_tinter_writes_to`）：
// ⑴ 只认 `'canvas'` 这一种 tag，别的一律**点名抛**（`'audio'`/`'button'`/`'script'` 那三种
//    在兼容层里各有各的替代腿：`_detectCodecs` 与 `_setupEventHandlers` 是自创底、
//    `loadScript` 不交、`Graphics` 的 `button` 在守卫里面）；
// ⑵ **没有** `addEventListener`/`removeEventListener`/`getElementById`/`body`/`visibilityState`
//    ⇒ 任何"有没有 DOM"的探测在这一族上拿到的仍是"没有这一格"（`undefined`），
//    走的还是上游那条**非浏览器**分支（`WebAudio._setupEventHandlers` 那五句监听器正因为如此
//    才是自创的空体底，不是"有 document 就挂上"）；
// ⑶ 只在 `typeof document === 'undefined'` 时才交（浏览器腿有真 `document` ⇒ 一个字节都不覆盖）。

// `document` 那一颗：**只有 `createElement`**。见上面第⑵条 —— 多加一格就是特性探测换分支。
if (typeof globalThis.document === 'undefined') {
    globalThis.document = {};
}

// 一颗离屏画布：`width`/`height` 是**普通字段**（与上游那颗 DOM canvas 一样可以赋值），
// 而 `getContext('2d')` 交回的是 `src/js/shim_context.js` 那一颗 ⇒ 往这颗缓冲上画的每一笔
// 都进**同一条操作流**（`crate::paint`），tint 的结果因此也走软件合成那条腿，
// 而不是另起一个只有浏览器才有的真相。
// `_sid` 是**这颗缓冲自己的**颗粒号：`Sisf2DContext._sid()` 读的是 `this._bitmap.__canvas.__sid`，
// 所以这里给一个 `__canvas` 指向自己、`_url` 为空串的宿主形状（空 URL ⇒ 合成时没有镜像源，
// 内容全靠操作流 —— 正是"离屏缓冲"在上游的样子）。
document.createElement = function(tag) {
    if (tag !== 'canvas') {
        throw new Error('sisf: document.createElement 只接 canvas（问的是 ' + JSON.stringify(tag) +
            '）⇒ 兼容层没有 DOM，那一格各有各的替代腿，见 src/js/shim_dom.js 的文件头');
    }
    var canvas = { __sid: Sisf2DContext.allocSid(), __url: '', width: 1, height: 1 };
    canvas.getContext = function(kind) {
        if (kind !== '2d') {
            // WebGL 那一族**不交**：兼容层的绘制走引擎的渲染器（`M10.md` 事实 75），
            // 而 `Graphics._detectType` 要的正是这一句答 null 才落到 canvas 那条分支。
            return null;
        }
        if (!canvas.__context) {
            canvas.__context = new Sisf2DContext({ __canvas: canvas, _url: canvas.__url });
        }
        return canvas.__context;
    };
    return canvas;
};
