Point.prototype = Object.create(PIXI.Point.prototype);

Point.prototype.constructor = Point;

Point.prototype.initialize = function(x, y) {
    PIXI.Point.call(this, x, y);
};

Rectangle.prototype = Object.create(PIXI.Rectangle.prototype);

Rectangle.prototype.constructor = Rectangle;

Rectangle.prototype.initialize = function(x, y, width, height) {
    PIXI.Rectangle.call(this, x, y, width, height);
};

Rectangle.emptyRectangle = new Rectangle(0, 0, 0, 0);
