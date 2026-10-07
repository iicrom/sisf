Graphics.isWebGL = function() {
    return this._renderer && this._renderer.type === PIXI.RENDERER_TYPE.WEBGL;
};

Graphics.hasWebGL = function() {
    try {
        var canvas = document.createElement('canvas');
        return !!(canvas.getContext('webgl') || canvas.getContext('experimental-webgl'));
    } catch (e) {
        return false;
    }
};

ScreenSprite.prototype = Object.create(PIXI.Container.prototype);

ScreenSprite.prototype.constructor = ScreenSprite;

ScreenSprite.prototype.initialize = function () {
    PIXI.Container.call(this);

    this._graphics = new PIXI.Graphics();
    this.addChild(this._graphics);
    this.opacity = 0;

    this._red = -1;
    this._green = -1;
    this._blue = -1;
    this._colorText = '';
    this.setBlack();
};

Object.defineProperty(ScreenSprite.prototype, 'opacity', {
    get: function () {
        return this.alpha * 255;
    },
    set: function (value) {
        this.alpha = value.clamp(0, 255) / 255;
    },
    configurable: true
});

ScreenSprite.YEPWarned = false;

ScreenSprite.warnYep = function () {
    if (!ScreenSprite.YEPWarned) {
        console.log("Deprecation warning. Please update YEP_CoreEngine. ScreenSprite is not a sprite, it has graphics inside.");
        ScreenSprite.YEPWarned = true;
    }
};

Object.defineProperty(ScreenSprite.prototype, 'anchor', {
    get: function () {
        ScreenSprite.warnYep();
        this.scale.x = 1;
        this.scale.y = 1;
        return {x: 0, y: 0};
    },
    set: function (value) {
        this.alpha = value.clamp(0, 255) / 255;
    },
    configurable: true
});

Object.defineProperty(ScreenSprite.prototype, 'blendMode', {
    get: function () {
        return this._graphics.blendMode;
    },
    set: function (value) {
        this._graphics.blendMode = value;
    },
    configurable: true
});

ScreenSprite.prototype.setBlack = function () {
    this.setColor(0, 0, 0);
};

ScreenSprite.prototype.setWhite = function () {
    this.setColor(255, 255, 255);
};

ScreenSprite.prototype.setColor = function (r, g, b) {
    if (this._red !== r || this._green !== g || this._blue !== b) {
        r = Math.round(r || 0).clamp(0, 255);
        g = Math.round(g || 0).clamp(0, 255);
        b = Math.round(b || 0).clamp(0, 255);
        this._red = r;
        this._green = g;
        this._blue = b;
        this._colorText = Utils.rgbToCssColor(r, g, b);

        var graphics = this._graphics;
        graphics.clear();
        var intColor = (r << 16) | (g << 8) | b;
        graphics.beginFill(intColor, 1);
        //whole screen with zoom. BWAHAHAHAHA
        graphics.drawRect(-Graphics.width * 5, -Graphics.height * 5, Graphics.width * 10, Graphics.height * 10);
    }
};

ToneFilter = function () {
    PIXI.filters.ColorMatrixFilter.call(this);
}

ToneFilter.prototype = Object.create(PIXI.filters.ColorMatrixFilter.prototype);

ToneFilter.prototype.constructor = ToneFilter;

ToneFilter.prototype.adjustHue = function(value) {
    this.hue(value, true);
};

ToneFilter.prototype.adjustSaturation = function(value) {
    value = (value || 0).clamp(-255, 255) / 255;
    this.saturate(value, true);
};

ToneFilter.prototype.adjustTone = function(r, g, b) {
    r = (r || 0).clamp(-255, 255) / 255;
    g = (g || 0).clamp(-255, 255) / 255;
    b = (b || 0).clamp(-255, 255) / 255;

    if (r !== 0 || g !== 0 || b !== 0) {
        var matrix = [
            1, 0, 0, r, 0,
            0, 1, 0, g, 0,
            0, 0, 1, b, 0,
            0, 0, 0, 1, 0
        ];

        this._loadMatrix(matrix, true);
    }
};

ToneSprite.prototype = Object.create(PIXI.Container.prototype);

ToneSprite.prototype.constructor = ToneSprite;

ToneSprite.prototype.initialize = function() {
    PIXI.Container.call(this);
    this.clear();
};

ToneSprite.prototype.clear = function() {
    this._red = 0;
    this._green = 0;
    this._blue = 0;
    this._gray = 0;
};

ToneSprite.prototype.setTone = function(r, g, b, gray) {
    this._red = Math.round(r || 0).clamp(-255, 255);
    this._green = Math.round(g || 0).clamp(-255, 255);
    this._blue = Math.round(b || 0).clamp(-255, 255);
    this._gray = Math.round(gray || 0).clamp(0, 255);
};
