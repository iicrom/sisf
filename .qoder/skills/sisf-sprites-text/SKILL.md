---
name: sisf-sprites-text
description: 在 SISF 里绘制精灵与文字：SpriteArena 生命周期、begin_tick/插值契约、动画两条通路、MoveRoute/Tween、TextEngine 排版与 sync 节奏。当涉及 sprite/text 绘制、动画、抖动或文字显示问题时使用。
---

# SISF 精灵与文字

## Sprite 关键字段（sprite.rs:154-195）

`bitmap: Option<Bitmap>`（None=内置白图纯色矩形）、`src: Rect`、`anchor`（同时是旋转轴心）、
`pos`、`prev_pos`、`scale`（可负）、`rotation`（弧度）、`opacity`（0 整只跳过）、`tone`、
`flip_x/y`、`z`（**仅同批次内决定序**，跨 epoch/blend 压不过）、`visible`、`blend`、
`layer`、`anim`（挂上后 `src` 归它）。
构造：`Sprite::new(bitmap)` / `Sprite::solid(Vec2)`。
`Tone::Offset` 可用；**`Tone::Matrix` 无着色器通路，采集时报 `CollectError::UnsupportedTone`**。

## 移动与插值的三条铁律（docs/sprite.md:113-128）

1. **每拍第一行 `arena.begin_tick()`**（anim.rs:456）——全场 `prev_pos = pos` 收拢。
   漏掉 = 停拍重播最后一跳的"抖"。
2. 位移只走 `set_pos`（旧 pos 挪进 prev_pos）或 `teleport`（不参与插值）；**直写 `pos` 禁用**。
3. 相机/跟随物在 `prepare` 里取 `interpolated(alpha)`，且 alpha 要点阵折整
   （tiled_map.rs:14-25 的 `lattice_alpha`），否则亚像素重采样糊屏。

## 采集进帧

`draw(alpha, renderer)` 里：`arena.collect_at(ViewportRef::MAIN, alpha, &mut list)`
（draw.rs:88；错误整次作废）→ `renderer.append_list(list)`。
`ViewportRef`：`MAIN` 只收 `Layer::MAIN`；`new(layer)` 收同标签；`all(layer)` 忽略标签全收。
合批排序键 `(layer, clip, epoch, blend, z, insertion)`——想少 batch 就少换图源/blend。

## 动画两条互斥通路（docs/sprite.md:86-100）

- `AnimDef.pattern = None`：**时间驱动**，每拍 `arena.tick_anims(dt)`（anim.rs:465）推进；
  `Turn`/`set_dir` 对它是 no-op。
- `pattern = Some(DirPattern::RpgMaker4x3 | RpgMaker2x2)`：**移动驱动**，
  `set_dir(DIR_DOWN=2/LEFT=4/RIGHT=6/UP=8)` + 每次移一格 `set_step`；**静止不换帧是契约**；
  `tick(dt)` 恒 false。`AnimDef::grid_pattern(pattern, cell_w, cell_h, dur_ms)` 直接建。
- `LoopMode::{Once, Pingpong, Loop(n)}`（n=0 无限，跑满停第 0 格）。

## 补间与路线（task.rs）

`TaskScheduler::schedule(SpriteRef, impl Task)` + `tick(&mut arena) -> usize`。
**装了 EventRunner 时 `tasks.tick` 归它独占，游戏侧再推一次 = 路线双倍速且不报错**
（见 sisf-events-input skill）。
- `Tween::new(keys, |sprite, v| ...)`——补哪个字段由闭包决定；`Ease::{Linear,InQuad,OutQuad,InOutQuad}`。
- `MoveRoute::grid(nodes, tile, speed)` 逐格查阻挡 / `.free(...)` 直线；
  节点 `Wait/Turn/Move/MoveTo/Jump/FacingLock`；`walkable(Fn)` 挂通行钩子；
  撞墙 = 本节点剩余格数作废、路线继续。

## 文字（sisf-text）

文字输出仍是普通 quad（`Instance::mask>0.5`，同管线同批通路）。

```rust
let mut te = TextEngine::from_bytes(font_bytes);       // draw.rs:40；字体字节通常来自 VFS
let run = te.layout("你好", &TextStyle::new(16.0).with_wrap(w).aligned(Align::Center));
te.draw_run(renderer, &run, [x, y], color, layer, z);  // 或 draw_text / text_quads
te.sync(renderer);                                      // ★ 每帧必调，见下
```

- **`sync(renderer)` 必须晚于本帧所有 `draw_*`、早于 `end_frame`**，否则新覆盖率页未注册、
  回退白占位（docs/text.md:47-58）；一次 draw 里需要时可调两次。
- resize 后 `te.set_raster_scale(ctx.raster_scale())`（1/2/4 档位，MAX 4.0）。
- **字体注册顺序有意义**：`from_bytes` 成功即设默认族、后注册者覆盖——先 CJK 后拉丁则
  中文走 per-glyph 回退（docs/text.md:60-70）。wasm 无系统字体，CJK 字形由宿主运行期补
  （见 docs/text.md 与 wasm-cjk 定案）。
- RGSS `Bitmap#draw_text` 形状：`begin_draw` 会话内画进位图，会话外落 layer 0。
- 字号/行高都是**虚拟像素**，与窗口缩放无关。

## 图集位图（render/atlas_gpu，image feature）

`Bitmap` 的 **Drop = 页退役**：必须持有句柄（Handle/Bitmap clone），不然下帧 `apply_atlas`
就把它删了（windows.rs:139 的注释）。大图用 `new_large_bitmap` 旁路；
渲染目标 `new_target`（RGSS `Bitmap.new`），配 `blt/fill_rect/clear_rect` 会话
（`begin_draw(Source)/end_draw`）。`Atlas::new_bitmap` 的 pixels **必须 premultiplied RGBA8**。
