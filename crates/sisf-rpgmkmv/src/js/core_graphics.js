// `rpg_core.js` 的**屏幕读数与报错面**（M10.4 的第一格，2026-10-04）。
//
// 这一族的交付顺序由 `tools/probes/core-touch.py` 量出来：已交付的 39 段 JS 里点名引用、而今天就
// 够不着的只有 `Graphics` 的六格 10 处（`width`/`height` 在 `Game_Map.screenTileX` 与
// `Game_CharacterBase` 的屏幕中心读数里，`printLoadingError` 在 `DataManager.checkError` 里，
// 视频那三格在 `Game_Interpreter.command261` 里）。上面这五行到 `});` 为止是**上游逐字**
// （`rpg_core.js:2242-2334` 的五块 `Object.defineProperty`）。
//
// **为什么交访问器而不交 `Graphics.initialize`**（实测）：上游 `initialize`（`:1734-1773`）的最后
// 八句是 `_testCanvasBlendModes`/`_modifyExistingElements`/`_updateRealScale`/`_createAllElements`/
// `_disableTextSelection`/`_disableContextMenu`/`_setupEventHandlers`/`_setupCssFontLoading`
// —— 一整族 DOM 与渲染器，那一截是 M10.4 的**渲染腿**，不在这一格。而 `initialize` 写尺寸走的是
// **字段直写**（`this._width = width || 800`），不经过访问器的 setter ⇒ 宿主照同一形状直写
// `_width`/`_height`/`_boxWidth`/`_boxHeight`/`_scale` 就好；**写访问器**（`Graphics.width = …`）
// 会敲 `_updateAllElements`，那一格没交 ⇒ 判据把它点名钉住，不伪装成能跑。
Object.defineProperty(Graphics, 'width', {
    get: function() {
        return this._width;
    },
    set: function(value) {
        if (this._width !== value) {
            this._width = value;
            this._updateAllElements();
        }
    },
    configurable: true
});

Object.defineProperty(Graphics, 'height', {
    get: function() {
        return this._height;
    },
    set: function(value) {
        if (this._height !== value) {
            this._height = value;
            this._updateAllElements();
        }
    },
    configurable: true
});

Object.defineProperty(Graphics, 'boxWidth', {
    get: function() {
        return this._boxWidth;
    },
    set: function(value) {
        this._boxWidth = value;
    },
    configurable: true
});

Object.defineProperty(Graphics, 'boxHeight', {
    get: function() {
        return this._boxHeight;
    },
    set: function(value) {
        this._boxHeight = value;
    },
    configurable: true
});

Object.defineProperty(Graphics, 'scale', {
    get: function() {
        return this._scale;
    },
    set: function(value) {
        if (this._scale !== value) {
            this._scale = value;
            this._updateAllElements();
        }
    },
    configurable: true
});

// ↓↓↓ 到这一行为止是**上游逐字**。下面两格同样是逐字（`rpg_core.js:2385-2388` 与 `:1991-2006`），
// 它们在这一格是因为 `DataManager.checkError`（已交）第一句就喊 `printLoadingError`。
// 它的体里那半截 DOM（`document.createElement('button')`）在守卫**里面** ——
// `_errorPrinter` 不在场时整句短路，所以逐字交是安全的，报错面的 DOM 那一半留给渲染腿。
Graphics._makeErrorHtml = function(name, message) {
    return ('<font color="yellow"><b>' + name + '</b></font><br>' +
            '<font color="white">' + message + '</font><br>');
};

Graphics.printLoadingError = function(url) {
    if (this._errorPrinter && !this._errorShowed) {
        this._errorPrinter.innerHTML = this._makeErrorHtml('Loading Error', 'Failed to load: ' + url);
        var button = document.createElement('button');
        button.innerHTML = 'Retry';
        button.style.fontSize = '24px';
        button.style.color = '#ffffff';
        button.style.backgroundColor = '#000000';
        button.onmousedown = button.ontouchstart = function(event) {
            ResourceHandler.retry();
            event.stopPropagation();
        };
        this._errorPrinter.appendChild(button);
        this._loadingCount = -Infinity;
    }
};

// ↓↓↓ 下面是**数据形状**（右值不是函数 ⇒ 逐字尺子数不到，由判据数）。上游那四格的落点在
// `Graphics.initialize`（`:1746-1749`）与 `:1720-1722`；这一格把它们提前摆出来，是为了让
// `printLoadingError` 与"有没有视频元件"这两件事在**没有渲染面**的宿主里问得出答案，
// 而不是读到 `undefined` 之后被当成"已经准备好了"。
Graphics._errorShowed = false;
Graphics._errorPrinter = null;
Graphics._video = null;
Graphics._videoLoading = false;
Graphics._videoVolume = 1;
