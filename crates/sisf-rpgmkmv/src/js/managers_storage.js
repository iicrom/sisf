// `StorageManager` 的**写侧那一族**（`rpg_managers.js:571-782` 的 24 条函数型静态，逐字切片）。
// 2026-10-07 用户改判"存档兼容上游 `rpgsave`"之后 M10.5 的最后一格（任务 #9）交的就是这一段 ——
// 编解码器（`LZString` + `JsonEx`）早在第三格就位，缺的从来不是算法，是**落盘那一句**。
//
// `isLocalMode()` = `Utils.isNwjs()` 恒假 ⇒ 走 webStorage 那一支，`require('fs')` 那几支**只在死支
// 里被解析、不被求值**（sloppy 里 `require` 是自由标识符，`var fs = require('fs')` 编译得过、只有真
// 跑到才 `ReferenceError`）⇒ 整段逐字交出。活的那一支踩 `localStorage`，那颗底在 `shim_storage.js`，
// 脚下踩 `managers.rs` 绑的三格 `__sisf_store_get/set/remove`；换绑到磁盘留给 `pray`（`compat_save.rs`）。
StorageManager.save = function(savefileId, json) {
    if (this.isLocalMode()) {
        this.saveToLocalFile(savefileId, json);
    } else {
        this.saveToWebStorage(savefileId, json);
    }
};

StorageManager.load = function(savefileId) {
    if (this.isLocalMode()) {
        return this.loadFromLocalFile(savefileId);
    } else {
        return this.loadFromWebStorage(savefileId);
    }
};

StorageManager.exists = function(savefileId) {
    if (this.isLocalMode()) {
        return this.localFileExists(savefileId);
    } else {
        return this.webStorageExists(savefileId);
    }
};

StorageManager.remove = function(savefileId) {
    if (this.isLocalMode()) {
        this.removeLocalFile(savefileId);
    } else {
        this.removeWebStorage(savefileId);
    }
};

StorageManager.backup = function(savefileId) {
    if (this.exists(savefileId)) {
        if (this.isLocalMode()) {
            var data = this.loadFromLocalFile(savefileId);
            var compressed = LZString.compressToBase64(data);
            var fs = require('fs');
            var dirPath = this.localFileDirectoryPath();
            var filePath = this.localFilePath(savefileId) + ".bak";
            if (!fs.existsSync(dirPath)) {
                fs.mkdirSync(dirPath);
            }
            fs.writeFileSync(filePath, compressed);
        } else {
            var data = this.loadFromWebStorage(savefileId);
            var compressed = LZString.compressToBase64(data);
            var key = this.webStorageKey(savefileId) + "bak";
            localStorage.setItem(key, compressed);
        }
    }
};

StorageManager.backupExists = function(savefileId) {
    if (this.isLocalMode()) {
        return this.localFileBackupExists(savefileId);
    } else {
        return this.webStorageBackupExists(savefileId);
    }
};

StorageManager.cleanBackup = function(savefileId) {
	if (this.backupExists(savefileId)) {
		if (this.isLocalMode()) {
			var fs = require('fs');
            var dirPath = this.localFileDirectoryPath();
            var filePath = this.localFilePath(savefileId);
            fs.unlinkSync(filePath + ".bak");
		} else {
		    var key = this.webStorageKey(savefileId);
			localStorage.removeItem(key + "bak");
		}
	}
};

StorageManager.restoreBackup = function(savefileId) {
    if (this.backupExists(savefileId)) {
        if (this.isLocalMode()) {
            var data = this.loadFromLocalBackupFile(savefileId);
            var compressed = LZString.compressToBase64(data);
            var fs = require('fs');
            var dirPath = this.localFileDirectoryPath();
            var filePath = this.localFilePath(savefileId);
            if (!fs.existsSync(dirPath)) {
                fs.mkdirSync(dirPath);
            }
            fs.writeFileSync(filePath, compressed);
            fs.unlinkSync(filePath + ".bak");
        } else {
            var data = this.loadFromWebStorageBackup(savefileId);
            var compressed = LZString.compressToBase64(data);
            var key = this.webStorageKey(savefileId);
            localStorage.setItem(key, compressed);
            localStorage.removeItem(key + "bak");
        }
    }
};

StorageManager.isLocalMode = function() {
    return Utils.isNwjs();
};

StorageManager.saveToLocalFile = function(savefileId, json) {
    var data = LZString.compressToBase64(json);
    var fs = require('fs');
    var dirPath = this.localFileDirectoryPath();
    var filePath = this.localFilePath(savefileId);
    if (!fs.existsSync(dirPath)) {
        fs.mkdirSync(dirPath);
    }
    fs.writeFileSync(filePath, data);
};

StorageManager.loadFromLocalFile = function(savefileId) {
    var data = null;
    var fs = require('fs');
    var filePath = this.localFilePath(savefileId);
    if (fs.existsSync(filePath)) {
        data = fs.readFileSync(filePath, { encoding: 'utf8' });
    }
    return LZString.decompressFromBase64(data);
};

StorageManager.loadFromLocalBackupFile = function(savefileId) {
    var data = null;
    var fs = require('fs');
    var filePath = this.localFilePath(savefileId) + ".bak";
    if (fs.existsSync(filePath)) {
        data = fs.readFileSync(filePath, { encoding: 'utf8' });
    }
    return LZString.decompressFromBase64(data);
};

StorageManager.localFileBackupExists = function(savefileId) {
    var fs = require('fs');
    return fs.existsSync(this.localFilePath(savefileId) + ".bak");
};

StorageManager.localFileExists = function(savefileId) {
    var fs = require('fs');
    return fs.existsSync(this.localFilePath(savefileId));
};

StorageManager.removeLocalFile = function(savefileId) {
    var fs = require('fs');
    var filePath = this.localFilePath(savefileId);
    if (fs.existsSync(filePath)) {
        fs.unlinkSync(filePath);
    }
};

StorageManager.saveToWebStorage = function(savefileId, json) {
    var key = this.webStorageKey(savefileId);
    var data = LZString.compressToBase64(json);
    localStorage.setItem(key, data);
};

StorageManager.loadFromWebStorage = function(savefileId) {
    var key = this.webStorageKey(savefileId);
    var data = localStorage.getItem(key);
    return LZString.decompressFromBase64(data);
};

StorageManager.loadFromWebStorageBackup = function(savefileId) {
    var key = this.webStorageKey(savefileId) + "bak";
    var data = localStorage.getItem(key);
    return LZString.decompressFromBase64(data);
};

StorageManager.webStorageBackupExists = function(savefileId) {
    var key = this.webStorageKey(savefileId) + "bak";
    return !!localStorage.getItem(key);
};

StorageManager.webStorageExists = function(savefileId) {
    var key = this.webStorageKey(savefileId);
    return !!localStorage.getItem(key);
};

StorageManager.removeWebStorage = function(savefileId) {
    var key = this.webStorageKey(savefileId);
    localStorage.removeItem(key);
};

StorageManager.localFileDirectoryPath = function() {
    var path = require('path');

    var base = path.dirname(process.mainModule.filename);
    return path.join(base, 'save/');
};

StorageManager.localFilePath = function(savefileId) {
    var name;
    if (savefileId < 0) {
        name = 'config.rpgsave';
    } else if (savefileId === 0) {
        name = 'global.rpgsave';
    } else {
        name = 'file%1.rpgsave'.format(savefileId);
    }
    return this.localFileDirectoryPath() + name;
};

StorageManager.webStorageKey = function(savefileId) {
    if (savefileId < 0) {
        return 'RPG Config';
    } else if (savefileId === 0) {
        return 'RPG Global';
    } else {
        return 'RPG File%1'.format(savefileId);
    }
};
