Sprite_Base.prototype = Object.create(Sprite.prototype);

Sprite_Base.prototype.constructor = Sprite_Base;

Sprite_Base.prototype.initialize = function() {
    Sprite.prototype.initialize.call(this);
    this._animationSprites = [];
    this._effectTarget = this;
    this._hiding = false;
};

Sprite_Base.prototype.update = function() {
    Sprite.prototype.update.call(this);
    this.updateVisibility();
    this.updateAnimationSprites();
};

Sprite_Base.prototype.hide = function() {
    this._hiding = true;
    this.updateVisibility();
};

Sprite_Base.prototype.show = function() {
    this._hiding = false;
    this.updateVisibility();
};

Sprite_Base.prototype.updateVisibility = function() {
    this.visible = !this._hiding;
};

Sprite_Base.prototype.updateAnimationSprites = function() {
    if (this._animationSprites.length > 0) {
        var sprites = this._animationSprites.clone();
        this._animationSprites = [];
        for (var i = 0; i < sprites.length; i++) {
            var sprite = sprites[i];
            if (sprite.isPlaying()) {
                this._animationSprites.push(sprite);
            } else {
                sprite.remove();
            }
        }
    }
};

Sprite_Base.prototype.startAnimation = function(animation, mirror, delay) {
    var sprite = new Sprite_Animation();
    sprite.setup(this._effectTarget, animation, mirror, delay);
    this.parent.addChild(sprite);
    this._animationSprites.push(sprite);
};

Sprite_Base.prototype.isAnimationPlaying = function() {
    return this._animationSprites.length > 0;
};

Sprite_Button.prototype = Object.create(Sprite.prototype);

Sprite_Button.prototype.constructor = Sprite_Button;

Sprite_Button.prototype.initialize = function() {
    Sprite.prototype.initialize.call(this);
    this._touching = false;
    this._coldFrame = null;
    this._hotFrame = null;
    this._clickHandler = null;
};

Sprite_Button.prototype.update = function() {
    Sprite.prototype.update.call(this);
    this.updateFrame();
    this.processTouch();
};

Sprite_Button.prototype.updateFrame = function() {
    var frame;
    if (this._touching) {
        frame = this._hotFrame;
    } else {
        frame = this._coldFrame;
    }
    if (frame) {
        this.setFrame(frame.x, frame.y, frame.width, frame.height);
    }
};

Sprite_Button.prototype.setColdFrame = function(x, y, width, height) {
    this._coldFrame = new Rectangle(x, y, width, height);
};

Sprite_Button.prototype.setHotFrame = function(x, y, width, height) {
    this._hotFrame = new Rectangle(x, y, width, height);
};

Sprite_Button.prototype.setClickHandler = function(method) {
    this._clickHandler = method;
};

Sprite_Button.prototype.callClickHandler = function() {
    if (this._clickHandler) {
        this._clickHandler();
    }
};

Sprite_Button.prototype.processTouch = function() {
    if (this.isActive()) {
        if (TouchInput.isTriggered() && this.isButtonTouched()) {
            this._touching = true;
        }
        if (this._touching) {
            if (TouchInput.isReleased() || !this.isButtonTouched()) {
                this._touching = false;
                if (TouchInput.isReleased()) {
                    this.callClickHandler();
                }
            }
        }
    } else {
        this._touching = false;
    }
};

Sprite_Button.prototype.isActive = function() {
    var node = this;
    while (node) {
        if (!node.visible) {
            return false;
        }
        node = node.parent;
    }
    return true;
};

Sprite_Button.prototype.isButtonTouched = function() {
    var x = this.canvasToLocalX(TouchInput.x);
    var y = this.canvasToLocalY(TouchInput.y);
    return x >= 0 && y >= 0 && x < this.width && y < this.height;
};

Sprite_Button.prototype.canvasToLocalX = function(x) {
    var node = this;
    while (node) {
        x -= node.x;
        node = node.parent;
    }
    return x;
};

Sprite_Button.prototype.canvasToLocalY = function(y) {
    var node = this;
    while (node) {
        y -= node.y;
        node = node.parent;
    }
    return y;
};

Sprite_Character.prototype = Object.create(Sprite_Base.prototype);

Sprite_Character.prototype.constructor = Sprite_Character;

Sprite_Character.prototype.initialize = function(character) {
    Sprite_Base.prototype.initialize.call(this);
    this.initMembers();
    this.setCharacter(character);
};

Sprite_Character.prototype.initMembers = function() {
    this.anchor.x = 0.5;
    this.anchor.y = 1;
    this._character = null;
    this._balloonDuration = 0;
    this._tilesetId = 0;
    this._upperBody = null;
    this._lowerBody = null;
};

Sprite_Character.prototype.setCharacter = function(character) {
    this._character = character;
};

Sprite_Character.prototype.update = function() {
    Sprite_Base.prototype.update.call(this);
    this.updateBitmap();
    this.updateFrame();
    this.updatePosition();
    this.updateAnimation();
    this.updateBalloon();
    this.updateOther();
};

Sprite_Character.prototype.updateVisibility = function() {
    Sprite_Base.prototype.updateVisibility.call(this);
    if (this._character.isTransparent()) {
        this.visible = false;
    }
};

Sprite_Character.prototype.isTile = function() {
    return this._character.tileId > 0;
};

Sprite_Character.prototype.tilesetBitmap = function(tileId) {
    var tileset = $gameMap.tileset();
    var setNumber = 5 + Math.floor(tileId / 256);
    return ImageManager.loadTileset(tileset.tilesetNames[setNumber]);
};

Sprite_Character.prototype.updateBitmap = function() {
    if (this.isImageChanged()) {
        this._tilesetId = $gameMap.tilesetId();
        this._tileId = this._character.tileId();
        this._characterName = this._character.characterName();
        this._characterIndex = this._character.characterIndex();
        if (this._tileId > 0) {
            this.setTileBitmap();
        } else {
            this.setCharacterBitmap();
        }
    }
};

Sprite_Character.prototype.isImageChanged = function() {
    return (this._tilesetId !== $gameMap.tilesetId() ||
            this._tileId !== this._character.tileId() ||
            this._characterName !== this._character.characterName() ||
            this._characterIndex !== this._character.characterIndex());
};

Sprite_Character.prototype.setTileBitmap = function() {
    this.bitmap = this.tilesetBitmap(this._tileId);
};

Sprite_Character.prototype.setCharacterBitmap = function() {
    this.bitmap = ImageManager.loadCharacter(this._characterName);
    this._isBigCharacter = ImageManager.isBigCharacter(this._characterName);
};

Sprite_Character.prototype.updateFrame = function() {
    if (this._tileId > 0) {
        this.updateTileFrame();
    } else {
        this.updateCharacterFrame();
    }
};

Sprite_Character.prototype.updateTileFrame = function() {
    var pw = this.patternWidth();
    var ph = this.patternHeight();
    var sx = (Math.floor(this._tileId / 128) % 2 * 8 + this._tileId % 8) * pw;
    var sy = Math.floor(this._tileId % 256 / 8) % 16 * ph;
    this.setFrame(sx, sy, pw, ph);
};

Sprite_Character.prototype.updateCharacterFrame = function() {
    var pw = this.patternWidth();
    var ph = this.patternHeight();
    var sx = (this.characterBlockX() + this.characterPatternX()) * pw;
    var sy = (this.characterBlockY() + this.characterPatternY()) * ph;
    this.updateHalfBodySprites();
    if (this._bushDepth > 0) {
        var d = this._bushDepth;
        this._upperBody.setFrame(sx, sy, pw, ph - d);
        this._lowerBody.setFrame(sx, sy + ph - d, pw, d);
        this.setFrame(sx, sy, 0, ph);
    } else {
        this.setFrame(sx, sy, pw, ph);
    }
};

Sprite_Character.prototype.characterBlockX = function() {
    if (this._isBigCharacter) {
        return 0;
    } else {
        var index = this._character.characterIndex();
        return index % 4 * 3;
    }
};

Sprite_Character.prototype.characterBlockY = function() {
    if (this._isBigCharacter) {
        return 0;
    } else {
        var index = this._character.characterIndex();
        return Math.floor(index / 4) * 4;
    }
};

Sprite_Character.prototype.characterPatternX = function() {
    return this._character.pattern();
};

Sprite_Character.prototype.characterPatternY = function() {
    return (this._character.direction() - 2) / 2;
};

Sprite_Character.prototype.patternWidth = function() {
    if (this._tileId > 0) {
        return $gameMap.tileWidth();
    } else if (this._isBigCharacter) {
        return this.bitmap.width / 3;
    } else {
        return this.bitmap.width / 12;
    }
};

Sprite_Character.prototype.patternHeight = function() {
    if (this._tileId > 0) {
        return $gameMap.tileHeight();
    } else if (this._isBigCharacter) {
        return this.bitmap.height / 4;
    } else {
        return this.bitmap.height / 8;
    }
};

Sprite_Character.prototype.updateHalfBodySprites = function() {
    if (this._bushDepth > 0) {
        this.createHalfBodySprites();
        this._upperBody.bitmap = this.bitmap;
        this._upperBody.visible = true;
        this._upperBody.y = - this._bushDepth;
        this._lowerBody.bitmap = this.bitmap;
        this._lowerBody.visible = true;
        this._upperBody.setBlendColor(this.getBlendColor());
        this._lowerBody.setBlendColor(this.getBlendColor());
        this._upperBody.setColorTone(this.getColorTone());
        this._lowerBody.setColorTone(this.getColorTone());
    } else if (this._upperBody) {
        this._upperBody.visible = false;
        this._lowerBody.visible = false;
    }
};

Sprite_Character.prototype.createHalfBodySprites = function() {
    if (!this._upperBody) {
        this._upperBody = new Sprite();
        this._upperBody.anchor.x = 0.5;
        this._upperBody.anchor.y = 1;
        this.addChild(this._upperBody);
    }
    if (!this._lowerBody) {
        this._lowerBody = new Sprite();
        this._lowerBody.anchor.x = 0.5;
        this._lowerBody.anchor.y = 1;
        this._lowerBody.opacity = 128;
        this.addChild(this._lowerBody);
    }
};

Sprite_Character.prototype.updatePosition = function() {
    this.x = this._character.screenX();
    this.y = this._character.screenY();
    this.z = this._character.screenZ();
};

Sprite_Character.prototype.updateAnimation = function() {
    this.setupAnimation();
    if (!this.isAnimationPlaying()) {
        this._character.endAnimation();
    }
    if (!this.isBalloonPlaying()) {
        this._character.endBalloon();
    }
};

Sprite_Character.prototype.updateOther = function() {
    this.opacity = this._character.opacity();
    this.blendMode = this._character.blendMode();
    this._bushDepth = this._character.bushDepth();
};

Sprite_Character.prototype.setupAnimation = function() {
    if (this._character.animationId() > 0) {
        var animation = $dataAnimations[this._character.animationId()];
        this.startAnimation(animation, false, 0);
        this._character.startAnimation();
    }
};

Sprite_Character.prototype.setupBalloon = function() {
    if (this._character.balloonId() > 0) {
        this.startBalloon();
        this._character.startBalloon();
    }
};

Sprite_Character.prototype.startBalloon = function() {
    if (!this._balloonSprite) {
        this._balloonSprite = new Sprite_Balloon();
    }
    this._balloonSprite.setup(this._character.balloonId());
    this.parent.addChild(this._balloonSprite);
};

Sprite_Character.prototype.updateBalloon = function() {
    this.setupBalloon();
    if (this._balloonSprite) {
        this._balloonSprite.x = this.x;
        this._balloonSprite.y = this.y - this.height;
        if (!this._balloonSprite.isPlaying()) {
            this.endBalloon();
        }
    }
};

Sprite_Character.prototype.endBalloon = function() {
    if (this._balloonSprite) {
        this.parent.removeChild(this._balloonSprite);
        this._balloonSprite = null;
    }
};

Sprite_Character.prototype.isBalloonPlaying = function() {
    return !!this._balloonSprite;
};

Sprite_Animation.prototype = Object.create(Sprite.prototype);

Sprite_Animation.prototype.constructor = Sprite_Animation;

Sprite_Animation._checker1 = {};

Sprite_Animation._checker2 = {};

Sprite_Animation.prototype.initialize = function() {
    Sprite.prototype.initialize.call(this);
    this._reduceArtifacts = true;
    this.initMembers();
};

Sprite_Animation.prototype.initMembers = function() {
    this._target = null;
    this._animation = null;
    this._mirror = false;
    this._delay = 0;
    this._rate = 4;
    this._duration = 0;
    this._flashColor = [0, 0, 0, 0];
    this._flashDuration = 0;
    this._screenFlashDuration = 0;
    this._hidingDuration = 0;
    this._bitmap1 = null;
    this._bitmap2 = null;
    this._cellSprites = [];
    this._screenFlashSprite = null;
    this._duplicated = false;
    this.z = 8;
};

Sprite_Animation.prototype.setup = function(target, animation, mirror, delay) {
    this._target = target;
    this._animation = animation;
    this._mirror = mirror;
    this._delay = delay;
    if (this._animation) {
        this.remove();
        this.setupRate();
        this.setupDuration();
        this.loadBitmaps();
        this.createSprites();
    }
};

Sprite_Animation.prototype.remove = function() {
    if (this.parent && this.parent.removeChild(this)) {
        this._target.setBlendColor([0, 0, 0, 0]);
        this._target.show();
    }
};

Sprite_Animation.prototype.setupRate = function() {
    this._rate = 4;
};

Sprite_Animation.prototype.setupDuration = function() {
    this._duration = this._animation.frames.length * this._rate + 1;
};

Sprite_Animation.prototype.update = function() {
    Sprite.prototype.update.call(this);
    this.updateMain();
    this.updateFlash();
    this.updateScreenFlash();
    this.updateHiding();
    Sprite_Animation._checker1 = {};
    Sprite_Animation._checker2 = {};
};

Sprite_Animation.prototype.updateFlash = function() {
    if (this._flashDuration > 0) {
        var d = this._flashDuration--;
        this._flashColor[3] *= (d - 1) / d;
        this._target.setBlendColor(this._flashColor);
    }
};

Sprite_Animation.prototype.updateScreenFlash = function() {
    if (this._screenFlashDuration > 0) {
        var d = this._screenFlashDuration--;
        if (this._screenFlashSprite) {
            this._screenFlashSprite.x = -this.absoluteX();
            this._screenFlashSprite.y = -this.absoluteY();
            this._screenFlashSprite.opacity *= (d - 1) / d;
            this._screenFlashSprite.visible = (this._screenFlashDuration > 0);
        }
    }
};

Sprite_Animation.prototype.absoluteX = function() {
    var x = 0;
    var object = this;
    while (object) {
        x += object.x;
        object = object.parent;
    }
    return x;
};

Sprite_Animation.prototype.absoluteY = function() {
    var y = 0;
    var object = this;
    while (object) {
        y += object.y;
        object = object.parent;
    }
    return y;
};

Sprite_Animation.prototype.updateHiding = function() {
    if (this._hidingDuration > 0) {
        this._hidingDuration--;
        if (this._hidingDuration === 0) {
            this._target.show();
        }
    }
};

Sprite_Animation.prototype.isPlaying = function() {
    return this._duration > 0;
};

Sprite_Animation.prototype.loadBitmaps = function() {
    var name1 = this._animation.animation1Name;
    var name2 = this._animation.animation2Name;
    var hue1 = this._animation.animation1Hue;
    var hue2 = this._animation.animation2Hue;
    this._bitmap1 = ImageManager.loadAnimation(name1, hue1);
    this._bitmap2 = ImageManager.loadAnimation(name2, hue2);
};

Sprite_Animation.prototype.isReady = function() {
    return this._bitmap1 && this._bitmap1.isReady() && this._bitmap2 && this._bitmap2.isReady();
};

Sprite_Animation.prototype.createSprites = function() {
    if (!Sprite_Animation._checker2[this._animation]) {
        this.createCellSprites();
        if (this._animation.position === 3) {
            Sprite_Animation._checker2[this._animation] = true;
        }
        this.createScreenFlashSprite();
    }
    if (Sprite_Animation._checker1[this._animation]) {
        this._duplicated = true;
    } else {
        this._duplicated = false;
        if (this._animation.position === 3) {
            Sprite_Animation._checker1[this._animation] = true;
        }
    }
};

Sprite_Animation.prototype.createCellSprites = function() {
    this._cellSprites = [];
    for (var i = 0; i < 16; i++) {
        var sprite = new Sprite();
        sprite.anchor.x = 0.5;
        sprite.anchor.y = 0.5;
        this._cellSprites.push(sprite);
        this.addChild(sprite);
    }
};

Sprite_Animation.prototype.createScreenFlashSprite = function() {
    this._screenFlashSprite = new ScreenSprite();
    this.addChild(this._screenFlashSprite);
};

Sprite_Animation.prototype.updateMain = function() {
    if (this.isPlaying() && this.isReady()) {
        if (this._delay > 0) {
            this._delay--;
        } else {
            this._duration--;
            this.updatePosition();
            if (this._duration % this._rate === 0) {
                this.updateFrame();
            }
        }
    }
};

Sprite_Animation.prototype.updatePosition = function() {
    if (this._animation.position === 3) {
        this.x = this.parent.width / 2;
        this.y = this.parent.height / 2;
    } else {
        var parent = this._target.parent;
        var grandparent = parent ? parent.parent : null;
        this.x = this._target.x;
        this.y = this._target.y;
        if (this.parent === grandparent) {
            this.x += parent.x;
            this.y += parent.y;
        }
        if (this._animation.position === 0) {
            this.y -= this._target.height;
        } else if (this._animation.position === 1) {
            this.y -= this._target.height / 2;
        }
    }
};

Sprite_Animation.prototype.updateFrame = function() {
    if (this._duration > 0) {
        var frameIndex = this.currentFrameIndex();
        this.updateAllCellSprites(this._animation.frames[frameIndex]);
        this._animation.timings.forEach(function(timing) {
            if (timing.frame === frameIndex) {
                this.processTimingData(timing);
            }
        }, this);
    }
};

Sprite_Animation.prototype.currentFrameIndex = function() {
    return (this._animation.frames.length -
            Math.floor((this._duration + this._rate - 1) / this._rate));
};

Sprite_Animation.prototype.updateAllCellSprites = function(frame) {
    for (var i = 0; i < this._cellSprites.length; i++) {
        var sprite = this._cellSprites[i];
        if (i < frame.length) {
            this.updateCellSprite(sprite, frame[i]);
        } else {
            sprite.visible = false;
        }
    }
};

Sprite_Animation.prototype.updateCellSprite = function(sprite, cell) {
    var pattern = cell[0];
    if (pattern >= 0) {
        var sx = pattern % 5 * 192;
        var sy = Math.floor(pattern % 100 / 5) * 192;
        var mirror = this._mirror;
        sprite.bitmap = pattern < 100 ? this._bitmap1 : this._bitmap2;
        sprite.setFrame(sx, sy, 192, 192);
        sprite.x = cell[1];
        sprite.y = cell[2];
        sprite.rotation = cell[4] * Math.PI / 180;
        sprite.scale.x = cell[3] / 100;

        if(cell[5]){
            sprite.scale.x *= -1;
        }
        if(mirror){
            sprite.x *= -1;
            sprite.rotation *= -1;
            sprite.scale.x *= -1;
        }

        sprite.scale.y = cell[3] / 100;
        sprite.opacity = cell[6];
        sprite.blendMode = cell[7];
        sprite.visible = true;
    } else {
        sprite.visible = false;
    }
};

Sprite_Animation.prototype.processTimingData = function(timing) {
    var duration = timing.flashDuration * this._rate;
    switch (timing.flashScope) {
    case 1:
        this.startFlash(timing.flashColor, duration);
        break;
    case 2:
        this.startScreenFlash(timing.flashColor, duration);
        break;
    case 3:
        this.startHiding(duration);
        break;
    }
    if (!this._duplicated && timing.se) {
        AudioManager.playSe(timing.se);
    }
};

Sprite_Animation.prototype.startFlash = function(color, duration) {
    this._flashColor = color.clone();
    this._flashDuration = duration;
};

Sprite_Animation.prototype.startScreenFlash = function(color, duration) {
    this._screenFlashDuration = duration;
    if (this._screenFlashSprite) {
        this._screenFlashSprite.setColor(color[0], color[1], color[2]);
        this._screenFlashSprite.opacity = color[3];
    }
};

Sprite_Animation.prototype.startHiding = function(duration) {
    this._hidingDuration = duration;
    this._target.hide();
};

Sprite_Balloon.prototype = Object.create(Sprite_Base.prototype);

Sprite_Balloon.prototype.constructor = Sprite_Balloon;

Sprite_Balloon.prototype.initialize = function() {
    Sprite_Base.prototype.initialize.call(this);
    this.initMembers();
    this.loadBitmap();
};

Sprite_Balloon.prototype.initMembers = function() {
    this._balloonId = 0;
    this._duration = 0;
    this.anchor.x = 0.5;
    this.anchor.y = 1;
    this.z = 7;
};

Sprite_Balloon.prototype.loadBitmap = function() {
    this.bitmap = ImageManager.loadSystem('Balloon');
    this.setFrame(0, 0, 0, 0);
};

Sprite_Balloon.prototype.setup = function(balloonId) {
    this._balloonId = balloonId;
    this._duration = 8 * this.speed() + this.waitTime();
};

Sprite_Balloon.prototype.update = function() {
    Sprite_Base.prototype.update.call(this);
    if (this._duration > 0) {
        this._duration--;
        if (this._duration > 0) {
            this.updateFrame();
        }
    }
};

Sprite_Balloon.prototype.updateFrame = function() {
    var w = 48;
    var h = 48;
    var sx = this.frameIndex() * w;
    var sy = (this._balloonId - 1) * h;
    this.setFrame(sx, sy, w, h);
};

Sprite_Balloon.prototype.speed = function() {
    return 8;
};

Sprite_Balloon.prototype.waitTime = function() {
    return 12;
};

Sprite_Balloon.prototype.frameIndex = function() {
    var index = (this._duration - this.waitTime()) / this.speed();
    return 7 - Math.max(Math.floor(index), 0);
};

Sprite_Balloon.prototype.isPlaying = function() {
    return this._duration > 0;
};

Sprite_Picture.prototype = Object.create(Sprite.prototype);

Sprite_Picture.prototype.constructor = Sprite_Picture;

Sprite_Picture.prototype.initialize = function(pictureId) {
    Sprite.prototype.initialize.call(this);
    this._pictureId = pictureId;
    this._pictureName = '';
    this._isPicture = true;
    this.update();
};

Sprite_Picture.prototype.picture = function() {
    return $gameScreen.picture(this._pictureId);
};

Sprite_Picture.prototype.update = function() {
    Sprite.prototype.update.call(this);
    this.updateBitmap();
    if (this.visible) {
        this.updateOrigin();
        this.updatePosition();
        this.updateScale();
        this.updateTone();
        this.updateOther();
    }
};

Sprite_Picture.prototype.updateBitmap = function() {
    var picture = this.picture();
    if (picture) {
        var pictureName = picture.name();
        if (this._pictureName !== pictureName) {
            this._pictureName = pictureName;
            this.loadBitmap();
        }
        this.visible = true;
    } else {
        this._pictureName = '';
        this.bitmap = null;
        this.visible = false;
    }
};

Sprite_Picture.prototype.updateOrigin = function() {
    var picture = this.picture();
    if (picture.origin() === 0) {
        this.anchor.x = 0;
        this.anchor.y = 0;
    } else {
        this.anchor.x = 0.5;
        this.anchor.y = 0.5;
    }
};

Sprite_Picture.prototype.updatePosition = function() {
    var picture = this.picture();
    this.x = Math.floor(picture.x());
    this.y = Math.floor(picture.y());
};

Sprite_Picture.prototype.updateScale = function() {
    var picture = this.picture();
    this.scale.x = picture.scaleX() / 100;
    this.scale.y = picture.scaleY() / 100;
};

Sprite_Picture.prototype.updateTone = function() {
    var picture = this.picture();
    if (picture.tone()) {
        this.setColorTone(picture.tone());
    } else {
        this.setColorTone([0, 0, 0, 0]);
    }
};

Sprite_Picture.prototype.updateOther = function() {
    var picture = this.picture();
    this.opacity = picture.opacity();
    this.blendMode = picture.blendMode();
    this.rotation = picture.angle() * Math.PI / 180;
};

Sprite_Picture.prototype.loadBitmap = function() {
    this.bitmap = ImageManager.loadPicture(this._pictureName);
};

Sprite_Timer.prototype = Object.create(Sprite.prototype);

Sprite_Timer.prototype.constructor = Sprite_Timer;

Sprite_Timer.prototype.initialize = function() {
    Sprite.prototype.initialize.call(this);
    this._seconds = 0;
    this.createBitmap();
    this.update();
};

Sprite_Timer.prototype.createBitmap = function() {
    this.bitmap = new Bitmap(96, 48);
    this.bitmap.fontSize = 32;
};

Sprite_Timer.prototype.update = function() {
    Sprite.prototype.update.call(this);
    this.updateBitmap();
    this.updatePosition();
    this.updateVisibility();
};

Sprite_Timer.prototype.updateBitmap = function() {
    if (this._seconds !== $gameTimer.seconds()) {
        this._seconds = $gameTimer.seconds();
        this.redraw();
    }
};

Sprite_Timer.prototype.redraw = function() {
    var text = this.timerText();
    var width = this.bitmap.width;
    var height = this.bitmap.height;
    this.bitmap.clear();
    this.bitmap.drawText(text, 0, 0, width, height, 'center');
};

Sprite_Timer.prototype.timerText = function() {
    var min = Math.floor(this._seconds / 60) % 60;
    var sec = this._seconds % 60;
    return min.padZero(2) + ':' + sec.padZero(2);
};

Sprite_Timer.prototype.updatePosition = function() {
    this.x = Graphics.width - this.bitmap.width;
    this.y = 0;
};

Sprite_Timer.prototype.updateVisibility = function() {
    this.visible = $gameTimer.isWorking();
};

Sprite_Destination.prototype = Object.create(Sprite.prototype);

Sprite_Destination.prototype.constructor = Sprite_Destination;

Sprite_Destination.prototype.initialize = function() {
    Sprite.prototype.initialize.call(this);
    this.createBitmap();
    this._frameCount = 0;
};

Sprite_Destination.prototype.update = function() {
    Sprite.prototype.update.call(this);
    if ($gameTemp.isDestinationValid()){
        this.updatePosition();
        this.updateAnimation();
        this.visible = true;
    } else {
        this._frameCount = 0;
        this.visible = false;
    }
};

Sprite_Destination.prototype.createBitmap = function() {
    var tileWidth = $gameMap.tileWidth();
    var tileHeight = $gameMap.tileHeight();
    this.bitmap = new Bitmap(tileWidth, tileHeight);
    this.bitmap.fillAll('white');
    this.anchor.x = 0.5;
    this.anchor.y = 0.5;
    this.blendMode = Graphics.BLEND_ADD;
};

Sprite_Destination.prototype.updatePosition = function() {
    var tileWidth = $gameMap.tileWidth();
    var tileHeight = $gameMap.tileHeight();
    var x = $gameTemp.destinationX();
    var y = $gameTemp.destinationY();
    this.x = ($gameMap.adjustX(x) + 0.5) * tileWidth;
    this.y = ($gameMap.adjustY(y) + 0.5) * tileHeight;
};

Sprite_Destination.prototype.updateAnimation = function() {
    this._frameCount++;
    this._frameCount %= 20;
    this.opacity = (20 - this._frameCount) * 6;
    this.scale.x = 1 + this._frameCount / 20;
    this.scale.y = this.scale.x;
};

Spriteset_Base.prototype = Object.create(Sprite.prototype);

Spriteset_Base.prototype.constructor = Spriteset_Base;

Spriteset_Base.prototype.initialize = function() {
    Sprite.prototype.initialize.call(this);
    this.setFrame(0, 0, Graphics.width, Graphics.height);
    this._tone = [0, 0, 0, 0];
    this.opaque = true;
    this.createLowerLayer();
    this.createToneChanger();
    this.createUpperLayer();
    this.update();
};

Spriteset_Base.prototype.createLowerLayer = function() {
    this.createBaseSprite();
};

Spriteset_Base.prototype.createUpperLayer = function() {
    this.createPictures();
    this.createTimer();
    this.createScreenSprites();
};

Spriteset_Base.prototype.update = function() {
    Sprite.prototype.update.call(this);
    this.updateScreenSprites();
    this.updateToneChanger();
    this.updatePosition();
};

Spriteset_Base.prototype.createBaseSprite = function() {
    this._baseSprite = new Sprite();
    this._baseSprite.setFrame(0, 0, this.width, this.height);
    this._blackScreen = new ScreenSprite();
    this._blackScreen.opacity = 255;
    this.addChild(this._baseSprite);
    this._baseSprite.addChild(this._blackScreen);
};

Spriteset_Base.prototype.createToneChanger = function() {
    if (Graphics.isWebGL()) {
        this.createWebGLToneChanger();
    } else {
        this.createCanvasToneChanger();
    }
};

Spriteset_Base.prototype.createWebGLToneChanger = function() {
    var margin = 48;
    var width = Graphics.width + margin * 2;
    var height = Graphics.height + margin * 2;
    this._toneFilter = new ToneFilter();
    this._baseSprite.filters = [this._toneFilter];
    this._baseSprite.filterArea = new Rectangle(-margin, -margin, width, height);
};

Spriteset_Base.prototype.createCanvasToneChanger = function() {
    this._toneSprite = new ToneSprite();
    this.addChild(this._toneSprite);
};

Spriteset_Base.prototype.createPictures = function() {
    var width = Graphics.boxWidth;
    var height = Graphics.boxHeight;
    var x = (Graphics.width - width) / 2;
    var y = (Graphics.height - height) / 2;
    this._pictureContainer = new Sprite();
    this._pictureContainer.setFrame(x, y, width, height);
    for (var i = 1; i <= $gameScreen.maxPictures(); i++) {
        this._pictureContainer.addChild(new Sprite_Picture(i));
    }
    this.addChild(this._pictureContainer);
};

Spriteset_Base.prototype.createTimer = function() {
    this._timerSprite = new Sprite_Timer();
    this.addChild(this._timerSprite);
};

Spriteset_Base.prototype.createScreenSprites = function() {
    this._flashSprite = new ScreenSprite();
    this._fadeSprite = new ScreenSprite();
    this.addChild(this._flashSprite);
    this.addChild(this._fadeSprite);
};

Spriteset_Base.prototype.updateScreenSprites = function() {
    var color = $gameScreen.flashColor();
    this._flashSprite.setColor(color[0], color[1], color[2]);
    this._flashSprite.opacity = color[3];
    this._fadeSprite.opacity = 255 - $gameScreen.brightness();
};

Spriteset_Base.prototype.updateToneChanger = function() {
    var tone = $gameScreen.tone();
    if (!this._tone.equals(tone)) {
        this._tone = tone.clone();
        if (Graphics.isWebGL()) {
            this.updateWebGLToneChanger();
        } else {
            this.updateCanvasToneChanger();
        }
    }
};

Spriteset_Base.prototype.updateWebGLToneChanger = function() {
    var tone = this._tone;
    this._toneFilter.reset();
    this._toneFilter.adjustTone(tone[0], tone[1], tone[2]);
    this._toneFilter.adjustSaturation(-tone[3]);
};

Spriteset_Base.prototype.updateCanvasToneChanger = function() {
    var tone = this._tone;
    this._toneSprite.setTone(tone[0], tone[1], tone[2], tone[3]);
};

Spriteset_Base.prototype.updatePosition = function() {
    var screen = $gameScreen;
    var scale = screen.zoomScale();
    this.scale.x = scale;
    this.scale.y = scale;
    this.x = Math.round(-screen.zoomX() * (scale - 1));
    this.y = Math.round(-screen.zoomY() * (scale - 1));
    this.x += Math.round(screen.shake());
};

Spriteset_Map.prototype = Object.create(Spriteset_Base.prototype);

Spriteset_Map.prototype.constructor = Spriteset_Map;

Spriteset_Map.prototype.initialize = function() {
    Spriteset_Base.prototype.initialize.call(this);
};

Spriteset_Map.prototype.createLowerLayer = function() {
    Spriteset_Base.prototype.createLowerLayer.call(this);
    this.createParallax();
    this.createTilemap();
    this.createCharacters();
    this.createShadow();
    this.createDestination();
    this.createWeather();
};

Spriteset_Map.prototype.update = function() {
    Spriteset_Base.prototype.update.call(this);
    this.updateTileset();
    this.updateParallax();
    this.updateTilemap();
    this.updateShadow();
    this.updateWeather();
};

Spriteset_Map.prototype.hideCharacters = function() {
    for (var i = 0; i < this._characterSprites.length; i++) {
        var sprite = this._characterSprites[i];
        if (!sprite.isTile()) {
            sprite.hide();
        }
    }
};

Spriteset_Map.prototype.createParallax = function() {
    this._parallax = new TilingSprite();
    this._parallax.move(0, 0, Graphics.width, Graphics.height);
    this._baseSprite.addChild(this._parallax);
};

Spriteset_Map.prototype.createTilemap = function() {
    if (Graphics.isWebGL()) {
        this._tilemap = new ShaderTilemap();
    } else {
        this._tilemap = new Tilemap();
    }
    this._tilemap.tileWidth = $gameMap.tileWidth();
    this._tilemap.tileHeight = $gameMap.tileHeight();
    this._tilemap.setData($gameMap.width(), $gameMap.height(), $gameMap.data());
    this._tilemap.horizontalWrap = $gameMap.isLoopHorizontal();
    this._tilemap.verticalWrap = $gameMap.isLoopVertical();
    this.loadTileset();
    this._baseSprite.addChild(this._tilemap);
};

Spriteset_Map.prototype.loadTileset = function() {
    this._tileset = $gameMap.tileset();
    if (this._tileset) {
        var tilesetNames = this._tileset.tilesetNames;
        for (var i = 0; i < tilesetNames.length; i++) {
            this._tilemap.bitmaps[i] = ImageManager.loadTileset(tilesetNames[i]);
        }
        var newTilesetFlags = $gameMap.tilesetFlags();
        this._tilemap.refreshTileset();
        if (!this._tilemap.flags.equals(newTilesetFlags)) {
            this._tilemap.refresh();
        }
        this._tilemap.flags = newTilesetFlags;
    }
};

Spriteset_Map.prototype.createCharacters = function() {
    this._characterSprites = [];
    $gameMap.events().forEach(function(event) {
        this._characterSprites.push(new Sprite_Character(event));
    }, this);
    $gameMap.vehicles().forEach(function(vehicle) {
        this._characterSprites.push(new Sprite_Character(vehicle));
    }, this);
    $gamePlayer.followers().reverseEach(function(follower) {
        this._characterSprites.push(new Sprite_Character(follower));
    }, this);
    this._characterSprites.push(new Sprite_Character($gamePlayer));
    for (var i = 0; i < this._characterSprites.length; i++) {
        this._tilemap.addChild(this._characterSprites[i]);
    }
};

Spriteset_Map.prototype.createShadow = function() {
    this._shadowSprite = new Sprite();
    this._shadowSprite.bitmap = ImageManager.loadSystem('Shadow1');
    this._shadowSprite.anchor.x = 0.5;
    this._shadowSprite.anchor.y = 1;
    this._shadowSprite.z = 6;
    this._tilemap.addChild(this._shadowSprite);
};

Spriteset_Map.prototype.createDestination = function() {
    this._destinationSprite = new Sprite_Destination();
    this._destinationSprite.z = 9;
    this._tilemap.addChild(this._destinationSprite);
};

Spriteset_Map.prototype.createWeather = function() {
    this._weather = new Weather();
    this.addChild(this._weather);
};

Spriteset_Map.prototype.updateTileset = function() {
    if (this._tileset !== $gameMap.tileset()) {
        this.loadTileset();
    }
};

Spriteset_Map.prototype._canvasReAddParallax = function() {
    var index = this._baseSprite.children.indexOf(this._parallax);
    this._baseSprite.removeChild(this._parallax);
    this._parallax = new TilingSprite();
    this._parallax.move(0, 0, Graphics.width, Graphics.height);
    this._parallax.bitmap = ImageManager.loadParallax(this._parallaxName);
    this._baseSprite.addChildAt(this._parallax,index);
};

Spriteset_Map.prototype.updateParallax = function() {
    if (this._parallaxName !== $gameMap.parallaxName()) {
        this._parallaxName = $gameMap.parallaxName();

        if (this._parallax.bitmap && Graphics.isWebGL() != true) {
            this._canvasReAddParallax();
        } else {
            this._parallax.bitmap = ImageManager.loadParallax(this._parallaxName);
        }
    }
    if (this._parallax.bitmap) {
        this._parallax.origin.x = $gameMap.parallaxOx();
        this._parallax.origin.y = $gameMap.parallaxOy();
    }
};

Spriteset_Map.prototype.updateTilemap = function() {
    this._tilemap.origin.x = $gameMap.displayX() * $gameMap.tileWidth();
    this._tilemap.origin.y = $gameMap.displayY() * $gameMap.tileHeight();
};

Spriteset_Map.prototype.updateShadow = function() {
    var airship = $gameMap.airship();
    this._shadowSprite.x = airship.shadowX();
    this._shadowSprite.y = airship.shadowY();
    this._shadowSprite.opacity = airship.shadowOpacity();
};

Spriteset_Map.prototype.updateWeather = function() {
    this._weather.type = $gameScreen.weatherType();
    this._weather.power = $gameScreen.weatherPower();
    this._weather.origin.x = $gameMap.displayX() * $gameMap.tileWidth();
    this._weather.origin.y = $gameMap.displayY() * $gameMap.tileHeight();
};
