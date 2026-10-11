// 自创的底：**一颗 `localStorage`**（M10.5 任务 #9 = 存档写侧，2026-10-07）。
// 不是上游文本 ⇒ 逐字尺子不比它，由 `objects-verbatim.py` 的 `SHIM_BASE` **双向**名单数。
//
// ## 为什么这一格要交 `localStorage`
//
// `isLocalMode()` = `Utils.isNwjs()` 恒 `false`（`platform.rs`，判据 `!Utils.isNwjs()` 钉两条腿同答案）
// ⇒ `StorageManager` 每一格走的都是 `else`（webStorage）那一支，而那一支踩的正是
// `localStorage.getItem/setItem/removeItem`（`rpg_managers.js:619/663/671/723/730/736/742/747/752`）。
// `platform-touch.py` 量到启用插件对 `localStorage.*` **0 处**（它们不碰存档那一族），所以交的范围
// 收到"存档那三格用到的"为止 —— 与 `shim_dom.js` 只交 `canvas` 一种标签同一条纪律。
//
// ## 落盘落进哪一颗 sink 由宿主决定，crate 不许知道
//
// 三格方法脚下踩 `managers.rs` 绑上来的 `__sisf_store_get/set/remove`。crate 的 fixture 装一颗
// **进程内 `HashMap`**（wasm 共享 realm 也只用它 ⇒ 判据钉 save→load 往返，不问磁盘）；native 真实游戏
// 由 `pray` 的 `src/compat_save.rs` 把这三格**换绑**成 `saves/fileN.rpgsave` 的磁盘读写 —— 与
// `__sisf_paint`/`__sisf_measure_text` 同一条"宿主换绑"的腿。key 与 value 挤在一根 `bind_str` 上，
// 用 `\u0001` 分隔（`LZString.compressToBase64` 的字母表只有 `A-Za-z0-9+/=`，控制字符不会撞进去）。
if (typeof globalThis.localStorage === 'undefined') {
    globalThis.localStorage = {};
}

localStorage.getItem = function(key) {
    return globalThis.__sisf_store_get(String(key));
};

localStorage.setItem = function(key, value) {
    globalThis.__sisf_store_set(String(key) + '\u0001' + String(value));
};

localStorage.removeItem = function(key) {
    globalThis.__sisf_store_remove(String(key));
};
