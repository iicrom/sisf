Game_Switches.prototype.initialize = function() {
    this.clear();
};

Game_Switches.prototype.clear = function() {
    this._data = [];
};

Game_Switches.prototype.value = function(switchId) {
    return !!this._data[switchId];
};

Game_Switches.prototype.setValue = function(switchId, value) {
    if (switchId > 0 && switchId < $dataSystem.switches.length) {
        this._data[switchId] = value;
        this.onChange();
    }
};

Game_Switches.prototype.onChange = function() {
    $gameMap.requestRefresh();
};

Game_Variables.prototype.initialize = function() {
    this.clear();
};

Game_Variables.prototype.clear = function() {
    this._data = [];
};

Game_Variables.prototype.value = function(variableId) {
    return this._data[variableId] || 0;
};

Game_Variables.prototype.setValue = function(variableId, value) {
    if (variableId > 0 && variableId < $dataSystem.variables.length) {
        if (typeof value === 'number') {
            value = Math.floor(value);
        }
        this._data[variableId] = value;
        this.onChange();
    }
};

Game_Variables.prototype.onChange = function() {
    $gameMap.requestRefresh();
};

Game_SelfSwitches.prototype.initialize = function() {
    this.clear();
};

Game_SelfSwitches.prototype.clear = function() {
    this._data = {};
};

Game_SelfSwitches.prototype.value = function(key) {
    return !!this._data[key];
};

Game_SelfSwitches.prototype.setValue = function(key, value) {
    if (value) {
        this._data[key] = true;
    } else {
        delete this._data[key];
    }
    this.onChange();
};

Game_SelfSwitches.prototype.onChange = function() {
    $gameMap.requestRefresh();
};
