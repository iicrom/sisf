WindowLayer.prototype = Object.create(PIXI.Container.prototype);

WindowLayer.prototype.constructor = WindowLayer;

WindowLayer.prototype.initialize = function() {
    PIXI.Container.call(this);
    this._width = 0;
    this._height = 0;
    this._tempCanvas = null;
    this._translationMatrix = [1, 0, 0, 0, 1, 0, 0, 0, 1];

    this._windowMask = new PIXI.Graphics();
    this._windowMask.beginFill(0xffffff, 1);
    this._windowMask.drawRect(0, 0, 0, 0);
    this._windowMask.endFill();
    this._windowRect = this._windowMask.graphicsData[0].shape;

    this._renderSprite = null;
    this.filterArea = new PIXI.Rectangle();
    this.filters = [WindowLayer.voidFilter];

    //temporary fix for memory leak bug
    this.on('removed', this.onRemoveAsAChild);
};

WindowLayer.prototype.onRemoveAsAChild = function() {
    this.removeChildren();
}

WindowLayer.voidFilter = new PIXI.filters.VoidFilter();

Object.defineProperty(WindowLayer.prototype, 'width', {
    get: function() {
        return this._width;
    },
    set: function(value) {
        this._width = value;
    },
    configurable: true
});

Object.defineProperty(WindowLayer.prototype, 'height', {
    get: function() {
        return this._height;
    },
    set: function(value) {
        this._height = value;
    },
    configurable: true
});

WindowLayer.prototype.move = function(x, y, width, height) {
    this.x = x;
    this.y = y;
    this.width = width;
    this.height = height;
};

WindowLayer.prototype.update = function() {
    this.children.forEach(function(child) {
        if (child.update) {
            child.update();
        }
    });
};
