// 自创的底：**`this._context` 那一层**（MV 的 `CanvasRenderingContext2D` 的替身）。
// 不是上游文本 ⇒ 逐字尺子不比它的内容，改由 `objects-verbatim.py` 的 `SHIM_BASE` **双向**名单数
// （多一颗少一颗都红，与 `HOME_MADE` 同一条纪律）。
//
// 为什么自创面要集中在这里，而不是散进 `Bitmap` 的那 17 格画法体（`M10.md`「第四格第二小格开工」）：
// 实测那 17 格的体**只碰 `this._context` 的成员，不碰 DOM canvas** ——
// `canvas`/`context`/`baseTexture` 三格是纯转发（`rpg_core.js:998-1029`）、`paintOpacity` 的 setter
// 只写 `this._context.globalAlpha`（`:1113-1118`）、`resize` 只写 `this._canvas.width/height` 与
// `this._baseTexture.width/height`（`:1129-1136`），而画法十格 + 描边两格 + `getPixel`/`getAlphaPixel`
// 用的就是下面这 **20 个成员**。把这一层交出来，那 17 格就**全部按上游文本逐字交出**（插件那 9 处
// 覆写点从此有东西可覆写），而自创面缩成"一颗上下文"。
//
// 方向定案第 1 条说 canvas 2D **不作为 JS 面存在**（`M10.md`），所以这一层不是浏览器那一颗：
// 它把每一笔画**当场**兑现成一条**具名操作**（CSS 颜色、字号、对齐、`globalAlpha` 都在这一步算清），
// 交给宿主绑上来的 `__sisf_paint`。**为什么是"记"不是"画"**：`&mut Renderer` 只在 `App::draw`
// 那一拍借得到（`crates/sisf-app/src/app.rs:36`），而这一层的调用点在 `eval` 那一刻。
// 今天绑上来的实现是 `paint.rs` 的**记录器** ⇒ 判据钉操作流的逐字内容；真绘制随帧循环那一格接上，
// 屏幕像素由 `capture` 腿证。
//
// 成员名单的**唯一**增长理由是"上游/插件**量出来**的引用"（尺子 `tools/probes/core-touch.py` 的
// canvas 桶与 `M10.6` 的覆盖账），不是照浏览器 API 的常识补。

// 一颗**线性渐变**的替身（`createLinearGradient` 的返回值）。上游只往它身上写两格
// `addColorStop(0, c1)` / `addColorStop(1, c2)`（`rpg_core.js:1291-1292`），所以这一颗
// 也只接得住这两格：别的停靠点会当场点名报错，不许"多接一点更保险"。
Sisf2DGradient = function(x0, y0, x1, y1) {
    this.x0 = x0;
    this.y0 = y0;
    this.x1 = x1;
    this.y1 = y1;
    this.stops = [];
};

Sisf2DGradient.prototype.addColorStop = function(offset, color) {
    if (this.stops.length >= 2) {
        throw new Error('sisf: addColorStop 只接得住两格（上游 gradientFillRect 就写两格；' +
                        offset + ',' + color + ' 是第三格）⇒ 见 M10.md 第四格');
    }
    this.stops.push({ offset: offset, color: color });
};

// 画布上下文的替身。字段初值照浏览器那一颗（上游不读初值，但**读缺参**：`fillRect` 之前
// 有人不 `save` 就直接改 `fillStyle`，所以每一格都要有自己的值）。
Sisf2DContext = function(bitmap) {
    this._bitmap = bitmap;
    this.globalAlpha = 1;
    this.globalCompositeOperation = 'source-over';
    this.fillStyle = '#000000';
    this.strokeStyle = '#000000';
    this.lineWidth = 1;
    this.lineJoin = 'miter';
    this.miterLimit = 10;
    this.font = '28px GameFont';
    this.textAlign = 'start';
    this.textBaseline = 'alphabetic';
    this.imageSmoothingEnabled = false;
    this._path = null;
    this._stack = [];
};

// 每一颗位图一个号：绘制会话按号找到那一页（`Atlas::new_target` 的句柄住在宿主那侧）。
// 惰性分配 —— 上游的 `Bitmap.load` 走 `Object.create(Bitmap.prototype)` + `_defer`，
// **不建画布**，所以号不能只在 `_createCanvas` 里发。
Sisf2DContext.allocSid = function() {
    Sisf2DContext._lastSid = (Sisf2DContext._lastSid || 0) + 1;
    return Sisf2DContext._lastSid;
};

// 位图"存在方式"的那一条：`_createCanvas` 与 `resize` 都汇到这里（上游给 `canvas.width`
// 赋值本身就是一次**改尺寸 + 清空内容**，`M10.md` 第四格第二小格的边界⑷钉的就是这一手）。
Sisf2DContext.setSize = function(sid, width, height, url) {
    Sisf2DContext.emit('size|' + sid + '|' + width + '|' + height + '|' +
                       encodeURIComponent(url || ''));
};

// 唯一的那一条出口：**一条具名操作**。参数只有一颗字符串 ⇒ 与 `jsrt::Host::bind_str`
// 那一格同一条腿（两条腿能共用签名的只有"一个字符串进、一个字符串出"）。
Sisf2DContext.emit = function(spec) {
    return globalThis.__sisf_paint(spec);
};

// 读回像素的那一条腿。三种回答：**逐格读数**（有 CPU 像素源的那一张 —— 文件载入的镜像，
// 或已经写过一次的位图）、**`blank|w|h`**（没有源：画出来的那一张，内容只活在操作流里 ⇒
// 这里就地造一片透明黑并**由宿主计数**，第二小格那条"读不回就抛"的口径在像素层落地之后换了，
// 理由与代价见 `M10.md`「第四格第三小格」的边界⑵）、**`undelivered:*`**（坏形状 ⇒ **抛**，
// 不读成黑）。
Sisf2DContext.prototype._readPixels = function(x, y, width, height) {
    var spec = 'pixels|' + this._sid() + '|' + x + '|' + y + '|' + width + '|' + height +
        '|' + encodeURIComponent(this._bitmap._url || '');
    var raw = globalThis.__sisf_pixels(spec);
    if (raw.indexOf('undelivered') === 0) {
        throw new Error('sisf: getImageData 的形状不对（' + raw + '）⇒ 见 M10.md「第四格第三小格」');
    }
    return raw;
};

// 文字宽度。**只有宿主知道字形**（`TextEngine::layout` 是纯 CPU 的，但它住在宿主那一侧），
// 所以宿主没绑测量器时同样抛，不许读成"每个字符一样宽"。
Sisf2DContext.prototype._measure = function(text) {
    var font = this._fontParts();
    var raw = globalThis.__sisf_measure_text('measure|' + encodeURIComponent(text) + '|' +
        font[0] + '|' + font[1] + '|' + font[2]);
    if (raw.indexOf('undelivered') === 0) {
        throw new Error('sisf: measureText 未接（' + raw + '）⇒ 宿主没绑 __sisf_measure_text');
    }
    return Number(raw);
};

// `this.font` 是 CSS 那一串（`_makeFontNameText` 交回来的形如 `'Italic 28px GameFont'`，
// `rpg_core.js:1521-1524`）。上游 core 只写这一种形状 ⇒ 认这一种，别的点名报错。
Sisf2DContext.prototype._fontParts = function() {
    var italic = 0;
    var rest = this.font;
    var head = rest.split(' ');
    if (head[0] === 'Italic') {
        italic = 1;
        rest = head.slice(1).join(' ');
    }
    var cut = rest.indexOf('px');
    if (cut < 0) {
        throw new Error('sisf: 认不出的 font 写法 ' + this.font + '（上游 _makeFontNameText 只交 ' +
            '"[Italic ]<n>px <face>"）');
    }
    return [encodeURIComponent(rest.slice(cut + 2).trim()), Number(rest.slice(0, cut)), italic];
};

// CSS 颜色 → `r,g,b,a`（0..255，**已经乘过 `globalAlpha`** —— 上游 canvas 就是这一条合成规则，
// `paintOpacity` 那一格的 alpha 因此天然流进每一条操作）。
Sisf2DContext.prototype._color = function(value) {
    var rgba = Sisf2DContext.parseColor(value);
    return rgba[0] + ',' + rgba[1] + ',' + rgba[2] + ',' +
        Math.round(rgba[3] * 255 * this.globalAlpha);
};

Sisf2DContext.prototype._sid = function() {
    var canvas = this._bitmap.__canvas;
    return String(canvas ? canvas.__sid : 0);
};

// 源的 `(号, URL)` 两格：`blt` 交的是 `source._canvas`，`bltImage` 交的是 `source._image`，
// 两者在我们这里都是记录 —— 前者带 `__sid`/`__url`，后者带 `src`（就是那条 URL 拼写）。
Sisf2DContext.prototype._source = function(image) {
    var sid = image && image.__sid ? image.__sid : 0;
    var url = image ? (image.__url || image.src || '') : '';
    return sid + '|' + encodeURIComponent(url);
};

// 每一条操作都带上混合方式。上游 core 只写 `'source-over'` 与 `'lighter'`（后者在 `blur` 里，
// 而 `blur` 是**没交**的那一格），而引擎的会话只硬编 `Blend::Normal`（`bitmap.rs:422`）⇒
// 这一格不是"顺手支持一下"，是**把差值带到读数里**：宿主接到非 `source-over` 要报回来。
Sisf2DContext.prototype._blend = function() {
    return '|' + this.globalCompositeOperation;
};

Sisf2DContext.parseColor = function(value) {
    if (typeof value === 'string' && value.charAt(0) === '#') {
        var hex = value.slice(1);
        var pick = function(i, n) {
            var chunk = hex.substr(i, n);
            return n === 1 ? parseInt(chunk + chunk, 16) : parseInt(chunk, 16);
        };
        if (hex.length === 3 || hex.length === 4) {
            return [pick(0, 1), pick(1, 1), pick(2, 1), hex.length === 4 ? pick(3, 1) / 255 : 1];
        }
        if (hex.length === 6 || hex.length === 8) {
            return [pick(0, 2), pick(2, 2), pick(4, 2), hex.length === 8 ? pick(6, 2) / 255 : 1];
        }
    }
    if (typeof value === 'string') {
        var lower = value.toLowerCase();
        var fn = lower.indexOf('rgb(') === 0 ? 3 : (lower.indexOf('rgba(') === 0 ? 4 : 0);
        if (fn) {
            var parts = lower.slice(lower.indexOf('(') + 1, lower.lastIndexOf(')')).split(',');
            if (parts.length === fn || (fn === 3 && parts.length === 4)) {
                return [Number(parts[0]), Number(parts[1]), Number(parts[2]),
                        fn === 4 ? Number(parts[3]) : 1];
            }
        }
        var named = Sisf2DContext.COLORS[lower];
        if (named) {
            return Sisf2DContext.parseColor(named);
        }
    }
    throw new Error('sisf: 认不出的颜色写法 ' + JSON.stringify(value) +
        '（认 #rgb/#rgba/#rrggbb/#rrggbbaa/rgb()/rgba() 与 CSS 名那一族）');
};

// CSS 关键字表。为什么带全表而不是"用到哪条补哪条"：`fillStyle`/`strokeStyle` 的字符串是
// **数据里的**（台词颜色、插件的窗皮），一条条等玩家撞到就红了；条数由判据
// `the_named_color_table_is_the_css_keyword_list` 数（右值不是 `= function` ⇒ 尺子数不到）。
Sisf2DContext.COLORS = {
    aliceblue: '#f0f8ff', antiquewhite: '#faebd7', aqua: '#00ffff', aquamarine: '#7fffd4',
    azure: '#f0ffff', beige: '#f5f5dc', bisque: '#ffe4c4', black: '#000000',
    blanchedalmond: '#ffebcd', blue: '#0000ff', blueviolet: '#8a2be2', brown: '#a52a2a',
    burlywood: '#deb887', cadetblue: '#5f9ea0', chartreuse: '#7fff00', chocolate: '#d2691e',
    coral: '#ff7f50', cornflowerblue: '#6495ed', cornsilk: '#fff8dc', crimson: '#dc143c',
    cyan: '#00ffff', darkblue: '#00008b', darkcyan: '#008b8b', darkgoldenrod: '#b8860b',
    darkgray: '#a9a9a9', darkgreen: '#006400', darkkhaki: '#bdb76b', darkmagenta: '#8b008b',
    darkolivegreen: '#556b2f', darkorange: '#ff8c00', darkorchid: '#9932cc', darkred: '#8b0000',
    darksalmon: '#e9967a', darkseagreen: '#8fbc8f', darkslateblue: '#483d8b',
    darkslategray: '#2f4f4f', darkturquoise: '#00ced1', darkviolet: '#9400d3',
    deeppink: '#ff1493', deepskyblue: '#00bfff', dimgray: '#696969', dodgerblue: '#1e90ff',
    firebrick: '#b22222', floralwhite: '#fffaf0', forestgreen: '#228b22', fuchsia: '#ff00ff',
    gainsboro: '#dcdcdc', ghostwhite: '#f8f8ff', gold: '#ffd700', goldenrod: '#daa520',
    gray: '#808080', green: '#008000', greenyellow: '#adff2f', honeydew: '#f0fff0',
    hotpink: '#ff69b4', indianred: '#cd5c5c', indigo: '#4b0082', ivory: '#fffff0',
    khaki: '#f0e68c', lavender: '#e6e6fa', lavenderblush: '#fff0f5', lawngreen: '#7cfc00',
    lemonchiffon: '#fffacd', lightblue: '#add8e6', lightcoral: '#f08080', lightcyan: '#e0ffff',
    lightgoldenrodyellow: '#fafad2', lightgray: '#d3d3d3', lightgreen: '#90ee90',
    lightpink: '#ffb6c1', lightsalmon: '#ffa07a', lightseagreen: '#20b2aa',
    lightskyblue: '#87cefa', lightslategray: '#778899', lightsteelblue: '#b0c4de',
    lightyellow: '#ffffe0', lime: '#00ff00', limegreen: '#32cd32', linen: '#faf0e6',
    magenta: '#ff00ff', maroon: '#800000', mediumaquamarine: '#66cdaa', mediumblue: '#0000cd',
    mediumorchid: '#ba55d3', mediumpurple: '#9370db', mediumseagreen: '#3cb371',
    mediumslateblue: '#7b68ee', mediumspringgreen: '#00fa9a', mediumturquoise: '#48d1cc',
    mediumvioletred: '#c71585', midnightblue: '#191970', mintcream: '#f5fffa',
    mistyrose: '#ffe4e1', moccasin: '#ffe4b5', navajowhite: '#ffdead', navy: '#000080',
    oldlace: '#fdf5e6', olive: '#808000', olivedrab: '#6b8e23', orange: '#ffa500',
    orangered: '#ff4500', orchid: '#da70d6', palegoldenrod: '#eee8aa', palegreen: '#98fb98',
    paleturquoise: '#afeeee', palevioletred: '#db7093', papayawhip: '#ffefd5',
    peachpuff: '#ffdab9', peru: '#cd853f', pink: '#ffc0cb', plum: '#dda0dd',
    powderblue: '#b0e0e6', purple: '#800080', rebeccapurple: '#663399', red: '#ff0000',
    rosybrown: '#bc8f8f', royalblue: '#4169e1', saddlebrown: '#8b4513', salmon: '#fa8072',
    sandybrown: '#f4a460', seagreen: '#2e8b57', seashell: '#fff5ee', sienna: '#a0522d',
    silver: '#c0c0c0', skyblue: '#87ceeb', slateblue: '#6a5acd', slategray: '#708090',
    snow: '#fffafa', springgreen: '#00ff7f', steelblue: '#4682b4', tan: '#d2b48c',
    teal: '#008080', thistle: '#d8bfd8', tomato: '#ff6347', turquoise: '#40e0d0',
    violet: '#ee82ee', wheat: '#f5deb3', white: '#ffffff', whitesmoke: '#f5f5f5',
    yellow: '#ffff00', yellowgreen: '#9acd32'
};

Sisf2DContext.prototype.save = function() {
    this._stack.push({
        globalAlpha: this.globalAlpha,
        globalCompositeOperation: this.globalCompositeOperation,
        fillStyle: this.fillStyle,
        strokeStyle: this.strokeStyle,
        lineWidth: this.lineWidth,
        lineJoin: this.lineJoin,
        miterLimit: this.miterLimit,
        font: this.font,
        textAlign: this.textAlign,
        textBaseline: this.textBaseline,
        imageSmoothingEnabled: this.imageSmoothingEnabled
    });
};

Sisf2DContext.prototype.restore = function() {
    var state = this._stack.pop();
    if (!state) {
        return;
    }
    this.globalAlpha = state.globalAlpha;
    this.globalCompositeOperation = state.globalCompositeOperation;
    this.fillStyle = state.fillStyle;
    this.strokeStyle = state.strokeStyle;
    this.lineWidth = state.lineWidth;
    this.lineJoin = state.lineJoin;
    this.miterLimit = state.miterLimit;
    this.font = state.font;
    this.textAlign = state.textAlign;
    this.textBaseline = state.textBaseline;
    this.imageSmoothingEnabled = state.imageSmoothingEnabled;
};

Sisf2DContext.prototype.clearRect = function(x, y, width, height) {
    Sisf2DContext.emit('clearRect|' + this._sid() + '|' + x + '|' + y + '|' + width + '|' + height);
};

Sisf2DContext.prototype.fillRect = function(x, y, width, height) {
    if (this.fillStyle && typeof this.fillStyle === 'object' && this.fillStyle.stops) {
        this._gradientFill(x, y, width, height);
        return;
    }
    Sisf2DContext.emit('fillRect|' + this._sid() + '|' + x + '|' + y + '|' + width + '|' + height +
        '|' + this._color(this.fillStyle) + this._blend());
};

// 渐变：引擎的会话**没有** gradient（`M10.md` 事实 75 的第⑵条真缺口），落法是"按调用尺寸现生成
// 一张像素纹理再贴" —— 那一步在宿主，而这里交的是**一条具名操作**：两格停靠点 + 方向。
// 方向照上游那两句（`vertical` ⇒ `(x,y)→(x,y+h)`，否则 `(x,y)→(x+w,y)`，`:1286-1290`）。
Sisf2DContext.prototype._gradientFill = function(x, y, width, height) {
    var grad = this.fillStyle;
    var vertical = grad.y0 !== grad.y1 && grad.x0 === grad.x1;
    Sisf2DContext.emit('gradientFillRect|' + this._sid() + '|' + x + '|' + y + '|' + width + '|' +
        height + '|' + this._color(grad.stops[0].color) + '|' + this._color(grad.stops[1].color) +
        '|' + (vertical ? 1 : 0) + this._blend());
};

Sisf2DContext.prototype.createLinearGradient = function(x0, y0, x1, y1) {
    return new Sisf2DGradient(x0, y0, x1, y1);
};

// `drawImage` 的三种参数形状（3 / 5 / 9）都从上游那一格来：`_createCanvas` 用 3 参那一格
// （`:768`），`blt`/`bltImage` 用 9 参（`:1158`、`:1183`），`blur`（**没交**的那一格）用 5 参与 9 参。
// 目标尺寸缺参时等于源尺寸（上游 `blt` 自己写了 `dw = dw || sw`，到不了这一格，但 3/5 参那一族要）。
Sisf2DContext.prototype.drawImage = function(image, a, b, c, d, e, f, g, h) {
    var sx = 0, sy = 0, sw = 0, sh = 0, dx = a, dy = b, dw = 0, dh = 0;
    if (arguments.length >= 9) {
        sx = a; sy = b; sw = c; sh = d; dx = e; dy = f; dw = g; dh = h;
    } else if (arguments.length >= 5) {
        sw = c; sh = d; dw = e; dh = f;
    } else {
        sw = image.width; sh = image.height; dw = sw; dh = sh;
    }
    Sisf2DContext.emit('blt|' + this._sid() + '|' + this._source(image) + '|' +
        sx + '|' + sy + '|' + sw + '|' + sh + '|' + dx + '|' + dy + '|' + dw + '|' + dh +
        '|' + Math.round(this.globalAlpha * 255) + this._blend());
};

Sisf2DContext.prototype.beginPath = function() {
    this._path = null;
};

Sisf2DContext.prototype.arc = function(x, y, radius, startAngle, endAngle, counterclockwise) {
    this._path = { cx: x, cy: y, r: radius, a0: startAngle, a1: endAngle, ccw: !!counterclockwise };
};

// 上游 core 走到 `fill()` 的只有 `drawCircle` 那一格（`:1311-1315`），而它画的就是**一颗实心圆** ⇒
// 这一格只接得住"一条 arc 之后 fill"，别的形状点名报错（`M10.md` 第四格第二小格的边界⑴）。
Sisf2DContext.prototype.fill = function() {
    var path = this._path;
    if (!path) {
        throw new Error('sisf: fill() 接不住没有 arc 的路径（上游 core 只有 drawCircle 走这一族）');
    }
    Sisf2DContext.emit('fillArc|' + this._sid() + '|' + path.cx + '|' + path.cy + '|' + path.r +
        '|' + this._color(this.fillStyle) + this._blend());
    this._path = null;
};

// 文字两格（描边与正文）。上游 `drawText`（`:1331-1357`）把对齐、`maxWidth`、`globalAlpha` 的账
// **在它自己那一格里**算完（逐字），走到这里已经只剩"这一串字、落在这个点"。
// `baseline` 上游恒写 `'alphabetic'`，而 `ty` 的偏移是 `y + lineHeight - (lineHeight - fontSize*0.7)/2`
// —— 都在上游那一格，这里一条都不重算。
Sisf2DContext.prototype.fillText = function(text, x, y, maxWidth) {
    this._text('fillText', text, x, y, maxWidth);
};

Sisf2DContext.prototype.strokeText = function(text, x, y, maxWidth) {
    this._text('strokeText', text, x, y, maxWidth);
};

Sisf2DContext.prototype._text = function(kind, text, x, y, maxWidth) {
    var font = this._fontParts();
    Sisf2DContext.emit(kind + '|' + this._sid() + '|' + encodeURIComponent(String(text)) + '|' +
        x + '|' + y + '|' + (maxWidth === undefined ? '' : maxWidth) + '|' + font[0] + '|' +
        font[1] + '|' + font[2] + '|' + (this.textAlign || 'start') + '|' + (this.textBaseline || 'alphabetic') + '|' +
        this._color(kind === 'strokeText' ? this.strokeStyle : this.fillStyle) + '|' +
        (kind === 'strokeText' ? this.lineWidth : 1) + this._blend());
};

Sisf2DContext.prototype.measureText = function(text) {
    return { width: this._measure(text) };
};

// **`Uint8ClampedArray`** 而不是普通数组 —— 这一格不是"顺手像浏览器"，是上游那两格滤镜体的命门：
// `adjustTone` 的 `pixels[i + 0] += r` 与 `rotateHue` 的 `pixels[i + 0] = rgb[0]` 交出来的数
// **在界外、而且带小数**（`hslToRgb` 全程浮点），界内与舍入全靠这颗类型学的手
// （夹到 0..255、按最近偶数舍）。普通数组会把 265 与 -51 原样写回宿主 ⇒ 色调叠两次就飘。
Sisf2DContext.prototype.getImageData = function(x, y, width, height) {
    var raw = this._readPixels(x, y, width, height);
    var blank = raw.indexOf('blank|') === 0;
    var data = new Uint8ClampedArray(width * height * 4);
    if (!blank) {
        var rows = raw.split(';');
        for (var i = 0; i < rows.length; i++) {
            var cell = rows[i].split(',');
            for (var j = 0; j < 4; j++) {
                data[i * 4 + j] = Number(cell[j] || 0);
            }
        }
    }
    var out = { width: width, height: height, data: data };
    // 没有像素源的那一张：写回那一腿据此**只计数不transport 整片零**（一窗的背板是
    // 816×144 ⇒ 一点三 MB 的 `0,0,0,0;`，而那一片本来就是零）。
    if (blank) out.__blank = true;
    return out;
};

// 写回像素那一条腿（`putImageData`）：今天的两只手是逐字交出来的 `adjustTone`/`rotateHue`
// （`rpg_core.js:1383-1396`、`:1404-1466`，`M10.md`「第四格第三小格」）。
// 宿主那一侧第一次收到某一位图的写会**为它复制一份自己的缓冲**（`raster.rs` 的 `owned`）——
// 色调/色相绝不许写回共享的镜像，否则第二颗 `Bitmap.load` 同一张图就跟着变色。
// 没有源的那一张（`__blank`）交一条 `blind|<sid>` 只计数：上游在这里合成进画布，
// 我们这里要等「软件合成」那一格才有画布，那一格的名字由判据点名
// （`a_tone_on_a_drawn_bitmap_is_dropped_and_counted`）。
Sisf2DContext.prototype.putImageData = function(imageData, x, y) {
    var spec;
    if (imageData.__blank) {
        spec = 'blind|' + this._sid();
    } else {
        var data = imageData.data;
        var n = imageData.width * imageData.height;
        var rows = new Array(n);
        for (var i = 0; i < n; i++) {
            var off = i * 4;
            rows[i] = data[off] + ',' + data[off + 1] + ',' + data[off + 2] + ',' + data[off + 3];
        }
        spec = 'putPixels|' + this._sid() + '|' + x + '|' + y + '|' + imageData.width + '|' +
            imageData.height + '|' + encodeURIComponent(this._bitmap._url || '') + '|' +
            rows.join(';');
    }
    var raw = globalThis.__sisf_put_pixels(spec);
    if (raw.indexOf('undelivered') === 0) {
        throw new Error('sisf: putImageData 写不回（' + raw + '）⇒ 副本满了不许把色调抹回原样，' +
            '见 `raster.rs` 的 `OWNED_LIMIT`');
    }
};
