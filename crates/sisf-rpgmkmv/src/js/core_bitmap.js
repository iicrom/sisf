Bitmap._reuseImages = [];

Bitmap.prototype._createBaseTexture = function(source){
    this.__baseTexture = new PIXI.BaseTexture(source);
    this.__baseTexture.mipmap = false;
    this.__baseTexture.width = source.width;
    this.__baseTexture.height = source.height;

    if (this._smooth) {
        this._baseTexture.scaleMode = PIXI.SCALE_MODES.LINEAR;
    } else {
        this._baseTexture.scaleMode = PIXI.SCALE_MODES.NEAREST;
    }
};

Bitmap.prototype._clearImgInstance = function(){
    this._image.src = "";
    this._image.onload = null;
    this._image.onerror = null;
    this._errorListener = null;
    this._loadListener = null;

    Bitmap._reuseImages.push(this._image);
    this._image = null;
};

Object.defineProperties(Bitmap.prototype, {
    _canvas: {
        get: function(){
            if(!this.__canvas)this._createCanvas();
            return this.__canvas;
        }
    },
    _context: {
        get: function(){
            if(!this.__context)this._createCanvas();
            return this.__context;
        }
    },

    _baseTexture: {
        get: function(){
            if(!this.__baseTexture) this._createBaseTexture(this._image || this.__canvas);
            return this.__baseTexture;
        }
    }
});

Bitmap.prototype._renewCanvas = function(){
    var newImage = this._image;
    if(newImage && this.__canvas && (this.__canvas.width < newImage.width || this.__canvas.height < newImage.height)){
        this._createCanvas();
    }
};

Bitmap.prototype.initialize = function(width, height) {
    if(!this._defer){
        this._createCanvas(width, height);
    }

    this._image = null;
    this._url = '';
    this._paintOpacity = 255;
    this._smooth = false;
    this._loadListeners = [];
    this._loadingState = 'none';
    this._decodeAfterRequest = false;

    /**
     * Cache entry, for images. In all cases _url is the same as cacheEntry.key
     * @type CacheEntry
     */
    this.cacheEntry = null;

    /**
     * The face name of the font.
     *
     * @property fontFace
     * @type String
     */
    this.fontFace = 'GameFont';

    /**
     * The size of the font in pixels.
     *
     * @property fontSize
     * @type Number
     */
    this.fontSize = 28;

    /**
     * Whether the font is italic.
     *
     * @property fontItalic
     * @type Boolean
     */
    this.fontItalic = false;

    /**
     * The color of the text in CSS format.
     *
     * @property textColor
     * @type String
     */
    this.textColor = '#ffffff';

    /**
     * The color of the outline of the text in CSS format.
     *
     * @property outlineColor
     * @type String
     */
    this.outlineColor = 'rgba(0, 0, 0, 0.5)';

    /**
     * The width of the outline of the text.
     *
     * @property outlineWidth
     * @type Number
     */
    this.outlineWidth = 4;
};

Bitmap.load = function(url) {
    var bitmap = Object.create(Bitmap.prototype);
    bitmap._defer = true;
    bitmap.initialize();

    bitmap._decodeAfterRequest = true;
    bitmap._requestImage(url);

    return bitmap;
};

Bitmap.prototype.isReady = function() {
    return this._loadingState === 'loaded' || this._loadingState === 'none';
};

Bitmap.prototype.isError = function() {
    return this._loadingState === 'error';
};

Bitmap.prototype.touch = function() {
    if (this.cacheEntry) {
        this.cacheEntry.touch();
    }
};

Object.defineProperty(Bitmap.prototype, 'url', {
    get: function() {
        return this._url;
    },
    configurable: true
});

Object.defineProperty(Bitmap.prototype, 'baseTexture', {
    get: function() {
        return this._baseTexture;
    },
    configurable: true
});

Object.defineProperty(Bitmap.prototype, 'canvas', {
    get: function() {
        return this._canvas;
    },
    configurable: true
});

Object.defineProperty(Bitmap.prototype, 'context', {
    get: function() {
        return this._context;
    },
    configurable: true
});

Object.defineProperty(Bitmap.prototype, 'width', {
    get: function() {
        if(this.isReady()){
            return this._image? this._image.width: this._canvas.width;
        }

        return 0;
    },
    configurable: true
});

Object.defineProperty(Bitmap.prototype, 'height', {
    get: function() {
        if(this.isReady()){
            return this._image? this._image.height: this._canvas.height;
        }

        return 0;
    },
    configurable: true
});

Object.defineProperty(Bitmap.prototype, 'rect', {
    get: function() {
        return new Rectangle(0, 0, this.width, this.height);
    },
    configurable: true
});

Object.defineProperty(Bitmap.prototype, 'smooth', {
    get: function() {
        return this._smooth;
    },
    set: function(value) {
        if (this._smooth !== value) {
            this._smooth = value;
            if(this.__baseTexture){
                if (this._smooth) {
                    this._baseTexture.scaleMode = PIXI.SCALE_MODES.LINEAR;
                } else {
                    this._baseTexture.scaleMode = PIXI.SCALE_MODES.NEAREST;
                }
            }
        }
    },
    configurable: true
});

Object.defineProperty(Bitmap.prototype, 'paintOpacity', {
    get: function() {
        return this._paintOpacity;
    },
    set: function(value) {
      if (this._paintOpacity !== value) {
          this._paintOpacity = value;
          this._context.globalAlpha = this._paintOpacity / 255;
      }
    },
    configurable: true
});

Bitmap.prototype.resize = function(width, height) {
    width = Math.max(width || 0, 1);
    height = Math.max(height || 0, 1);
    this._canvas.width = width;
    this._canvas.height = height;
    this._baseTexture.width = width;
    this._baseTexture.height = height;
};

Bitmap.prototype.blt = function(source, sx, sy, sw, sh, dx, dy, dw, dh) {
    dw = dw || sw;
    dh = dh || sh;
    if (sx >= 0 && sy >= 0 && sw > 0 && sh > 0 && dw > 0 && dh > 0 &&
            sx + sw <= source.width && sy + sh <= source.height) {
        this._context.globalCompositeOperation = 'source-over';
        this._context.drawImage(source._canvas, sx, sy, sw, sh, dx, dy, dw, dh);
        this._setDirty();
    }
};

Bitmap.prototype.bltImage = function(source, sx, sy, sw, sh, dx, dy, dw, dh) {
    dw = dw || sw;
    dh = dh || sh;
    if (sx >= 0 && sy >= 0 && sw > 0 && sh > 0 && dw > 0 && dh > 0 &&
        sx + sw <= source.width && sy + sh <= source.height) {
        this._context.globalCompositeOperation = 'source-over';
        this._context.drawImage(source._image, sx, sy, sw, sh, dx, dy, dw, dh);
        this._setDirty();
    }
};

Bitmap.prototype.getPixel = function(x, y) {
    var data = this._context.getImageData(x, y, 1, 1).data;
    var result = '#';
    for (var i = 0; i < 3; i++) {
        result += data[i].toString(16).padZero(2);
    }
    return result;
};

Bitmap.prototype.getAlphaPixel = function(x, y) {
    var data = this._context.getImageData(x, y, 1, 1).data;
    return data[3];
};

Bitmap.prototype.clearRect = function(x, y, width, height) {
    this._context.clearRect(x, y, width, height);
    this._setDirty();
};

Bitmap.prototype.clear = function() {
    this.clearRect(0, 0, this.width, this.height);
};

Bitmap.prototype.fillRect = function(x, y, width, height, color) {
    var context = this._context;
    context.save();
    context.fillStyle = color;
    context.fillRect(x, y, width, height);
    context.restore();
    this._setDirty();
};

Bitmap.prototype.fillAll = function(color) {
    this.fillRect(0, 0, this.width, this.height, color);
};

Bitmap.prototype.gradientFillRect = function(x, y, width, height, color1,
                                             color2, vertical) {
    var context = this._context;
    var grad;
    if (vertical) {
        grad = context.createLinearGradient(x, y, x, y + height);
    } else {
        grad = context.createLinearGradient(x, y, x + width, y);
    }
    grad.addColorStop(0, color1);
    grad.addColorStop(1, color2);
    context.save();
    context.fillStyle = grad;
    context.fillRect(x, y, width, height);
    context.restore();
    this._setDirty();
};

Bitmap.prototype.drawCircle = function(x, y, radius, color) {
    var context = this._context;
    context.save();
    context.fillStyle = color;
    context.beginPath();
    context.arc(x, y, radius, 0, Math.PI * 2, false);
    context.fill();
    context.restore();
    this._setDirty();
};

Bitmap.prototype.drawText = function(text, x, y, maxWidth, lineHeight, align) {
    // Note: Firefox has a bug with textBaseline: Bug 737852
    //       So we use 'alphabetic' here.
    if (text !== undefined) {
        var tx = x;
        var ty = y + lineHeight - (lineHeight - this.fontSize * 0.7) / 2;
        var context = this._context;
        var alpha = context.globalAlpha;
        maxWidth = maxWidth || 0xffffffff;
        if (align === 'center') {
            tx += maxWidth / 2;
        }
        if (align === 'right') {
            tx += maxWidth;
        }
        context.save();
        context.font = this._makeFontNameText();
        context.textAlign = align;
        context.textBaseline = 'alphabetic';
        context.globalAlpha = 1;
        this._drawTextOutline(text, tx, ty, maxWidth);
        context.globalAlpha = alpha;
        this._drawTextBody(text, tx, ty, maxWidth);
        context.restore();
        this._setDirty();
    }
};

Bitmap.prototype.measureTextWidth = function(text) {
    var context = this._context;
    context.save();
    context.font = this._makeFontNameText();
    var width = context.measureText(text).width;
    context.restore();
    return width;
};

Bitmap.prototype.adjustTone = function(r, g, b) {
    if ((r || g || b) && this.width > 0 && this.height > 0) {
        var context = this._context;
        var imageData = context.getImageData(0, 0, this.width, this.height);
        var pixels = imageData.data;
        for (var i = 0; i < pixels.length; i += 4) {
            pixels[i + 0] += r;
            pixels[i + 1] += g;
            pixels[i + 2] += b;
        }
        context.putImageData(imageData, 0, 0);
        this._setDirty();
    }
};

Bitmap.prototype.rotateHue = function(offset) {
    function rgbToHsl(r, g, b) {
        var cmin = Math.min(r, g, b);
        var cmax = Math.max(r, g, b);
        var h = 0;
        var s = 0;
        var l = (cmin + cmax) / 2;
        var delta = cmax - cmin;

        if (delta > 0) {
            if (r === cmax) {
                h = 60 * (((g - b) / delta + 6) % 6);
            } else if (g === cmax) {
                h = 60 * ((b - r) / delta + 2);
            } else {
                h = 60 * ((r - g) / delta + 4);
            }
            s = delta / (255 - Math.abs(2 * l - 255));
        }
        return [h, s, l];
    }

    function hslToRgb(h, s, l) {
        var c = (255 - Math.abs(2 * l - 255)) * s;
        var x = c * (1 - Math.abs((h / 60) % 2 - 1));
        var m = l - c / 2;
        var cm = c + m;
        var xm = x + m;

        if (h < 60) {
            return [cm, xm, m];
        } else if (h < 120) {
            return [xm, cm, m];
        } else if (h < 180) {
            return [m, cm, xm];
        } else if (h < 240) {
            return [m, xm, cm];
        } else if (h < 300) {
            return [xm, m, cm];
        } else {
            return [cm, m, xm];
        }
    }

    if (offset && this.width > 0 && this.height > 0) {
        offset = ((offset % 360) + 360) % 360;
        var context = this._context;
        var imageData = context.getImageData(0, 0, this.width, this.height);
        var pixels = imageData.data;
        for (var i = 0; i < pixels.length; i += 4) {
            var hsl = rgbToHsl(pixels[i + 0], pixels[i + 1], pixels[i + 2]);
            var h = (hsl[0] + offset) % 360;
            var s = hsl[1];
            var l = hsl[2];
            var rgb = hslToRgb(h, s, l);
            pixels[i + 0] = rgb[0];
            pixels[i + 1] = rgb[1];
            pixels[i + 2] = rgb[2];
        }
        context.putImageData(imageData, 0, 0);
        this._setDirty();
    }
};

Bitmap.prototype.addLoadListener = function(listner) {
    if (!this.isReady()) {
        this._loadListeners.push(listner);
    } else {
        listner(this);
    }
};

Bitmap.prototype._makeFontNameText = function() {
    return (this.fontItalic ? 'Italic ' : '') +
            this.fontSize + 'px ' + this.fontFace;
};

Bitmap.prototype._drawTextOutline = function(text, tx, ty, maxWidth) {
    var context = this._context;
    context.strokeStyle = this.outlineColor;
    context.lineWidth = this.outlineWidth;
    context.lineJoin = 'round';
    context.strokeText(text, tx, ty, maxWidth);
};

Bitmap.prototype._drawTextBody = function(text, tx, ty, maxWidth) {
    var context = this._context;
    context.fillStyle = this.textColor;
    context.fillText(text, tx, ty, maxWidth);
};

Bitmap.prototype._onLoad = function() {
    this._image.removeEventListener('load', this._loadListener);
    this._image.removeEventListener('error', this._errorListener);

    this._renewCanvas();

    switch(this._loadingState){
        case 'requesting':
            this._loadingState = 'requestCompleted';
            if(this._decodeAfterRequest){
                this.decode();
            }else{
                this._loadingState = 'purged';
                this._clearImgInstance();
            }
            break;

        case 'decrypting':
            window.URL.revokeObjectURL(this._image.src);
            this._loadingState = 'decryptCompleted';
            if(this._decodeAfterRequest){
                this.decode();
            }else{
                this._loadingState = 'purged';
                this._clearImgInstance();
            }
            break;
    }
};

Bitmap.prototype.decode = function(){
    switch(this._loadingState){
        case 'requestCompleted': case 'decryptCompleted':
            this._loadingState = 'loaded';

            if(!this.__canvas) this._createBaseTexture(this._image);
            this._setDirty();
            this._callLoadListeners();
            break;

        case 'requesting': case 'decrypting':
            this._decodeAfterRequest = true;
            if (!this._loader) {
                this._loader = ResourceHandler.createLoader(this._url, this._requestImage.bind(this, this._url), this._onError.bind(this));
                this._image.removeEventListener('error', this._errorListener);
                this._image.addEventListener('error', this._errorListener = this._loader);
            }
            break;

        case 'pending': case 'purged': case 'error':
            this._decodeAfterRequest = true;
            this._requestImage(this._url);
            break;
    }
};

Bitmap.prototype._callLoadListeners = function() {
    while (this._loadListeners.length > 0) {
        var listener = this._loadListeners.shift();
        listener(this);
    }
};

Bitmap.prototype._onError = function() {
    this._image.removeEventListener('load', this._loadListener);
    this._image.removeEventListener('error', this._errorListener);
    this._loadingState = 'error';
};

Bitmap.prototype._setDirty = function() {
    this._dirty = true;
};

Bitmap.prototype.checkDirty = function() {
    if (this._dirty) {
        this._baseTexture.update();
        this._dirty = false;
    }
};

Bitmap.request = function(url){
    var bitmap = Object.create(Bitmap.prototype);
    bitmap._defer = true;
    bitmap.initialize();

    bitmap._url = url;
    bitmap._loadingState = 'pending';

    return bitmap;
};

Bitmap.prototype.isRequestOnly = function(){
    return !(this._decodeAfterRequest || this.isReady());
};

Bitmap.prototype.isRequestReady = function(){
    return this._loadingState !== 'pending' &&
        this._loadingState !== 'requesting' &&
        this._loadingState !== 'decrypting';
};

Bitmap.prototype.startRequest = function(){
    if(this._loadingState === 'pending'){
        this._decodeAfterRequest = false;
        this._requestImage(this._url);
    }
};
