// `SceneManager` 的**主循环那一格里、上游体落在宿主那一侧的三格**（M10.5 真绘制第四小格）。
//
// 这三格不是"忘了交"，也不是假底：它们的**上游体**分别要 `Graphics` 的载入图那一层
// （DOM 的 `_upperCanvas`）与 `Bitmap.snap`（PIXI 渲染器的截图）—— 两者在移植侧都没有对应的层，
// 而它们的**产物在真流里一个读者都没有**。逐条：
//
// ⑴ `SceneManager.onSceneCreate`（`rpg_managers.js:2040-2042`）与
//     `SceneManager.onSceneStart`（`:2044-2046`）：上游体是 `Graphics.startLoading()` /
//     `Graphics.endLoading()`（`rpg_core.js:1957-1959`/`:1979-1982`），那一族画的是
//     `_upperCanvas` 上的"Now Loading"图。**`onSceneLoading`（`:2048-2050`）连空体都不给** ——
//     它唯一的读者是没交的 `renderScene`，按"不许先造没人读的底"那条（`host-paint-face.py` ③）
//     留成缺席，喊到就抛。移植侧那一格的等价物是 HUD 的"换图等待 N 拍"（`M10.md` 的 M9.2 那条
//     旧路线落法同一档），它读的是**拍数与缺的条目**而不是那张图。
// ⑵ `SceneManager.snapForBackground`（`:2115-2118`）：上游体是
//     `this._backgroundBitmap = this.snap(); this._backgroundBitmap.blur();` —— 那两格
//     （`Bitmap.snap` = `PIXI.RenderTexture.create` + 渲染器 render-to-texture，`Bitmap.blur` =
//     三遍 `drawImage` + `lighter`）都在 `Bitmap` 那一格的 `--skip` 名单里（四格欠账的两格）。
//     **它写的 `_backgroundBitmap` 在真流里零读者**：全上游只有 `Scene_MenuBase.createBackground`
//     （`rpg_scenes.js:926`）与 `Spriteset_Battle.createBackground`（`rpg_sprites.js:2447`）读它，
//     而菜单与战斗是 `AGENTS.md`「定案」里的非目标 ⇒ 空体不吞掉任何玩家看得见的内容。
//     插件那侧点名的 4 处 `SceneManager.snap` 与本格无关（那是插件自己截图的一格，记 M10.6 的开账）。
//
// 三格都由 `objects-verbatim.py` 的 `HOME_MADE` 名单双向数着（多一格少一格都红）。

SceneManager.onSceneCreate = function() {};

SceneManager.onSceneStart = function() {};

SceneManager.snapForBackground = function() {};
