# T6：文本 + 位图渲染目标 + clip 栈

三条通路共用同一条 sprite 管线，所以这份文档一次讲完 —— 它们的边界恰好就是
`Renderer` 的三种"当前落位目标"：

```
主帧（交换链/离屏）──begin_frame──▶ push ──end_frame──▶ 一个 pass，排序分组
位图会话（RT）──────begin_draw────▶ push ──end_draw───▶ 一个 pass，按提交顺序
文本（字形覆盖率）──TextEngine::draw_*─▶ 还是 push ────▶ 上面两者之一，mask 分支采样
clip 栈──────────────────────────── push_clip/pop_clip ─▶ 记录时给每个 batch 一个 scissor
```

## 文本：三层，边界是"谁需要 GPU"

| 层 | 文件 | 碰 GPU？ | 职责 |
| --- | --- | --- | --- |
| `FontBundle` | `bundle.rs` | ❌ | 字体字节 → `cosmic_text::FontSystem`；自带字体优先，缺失回退系统默认并 warn 一次 |
| `layout` | `layout.rs` | ❌ | shaping/换行/bidi → `TextRun`（整数像素 pen + 字形代号） |
| `FontAtlas` | `atlas.rs` | ✅（只在 `sync`） | `CacheKey → R8Unorm 覆盖率子矩形`，etagere 装箱，页自管 |
| `TextEngine` | `draw.rs` | ✅（只在 `sync`） | 上面三者的门面：`layout` / `text_quads` / `draw_text` / `draw_text_at` / `draw_run` / `sync` |

前两层纯 CPU，所以中英混排、换行、字号单调性这些断言在 wasm 下也能跑单测；只有覆盖率页
的纹理是 GPU 的。

**文本没有自己的管线。** 一个字形就是一个 `Instance`，与精灵唯一的区别是 `mask = 1.0`：

```wgsl
var sampled = textureSample(tex, tex_sampler, i.uv);
if (i.mask > 0.5) { let c = sampled.r; sampled = vec4<f32>(c, c, c, c); }
```

覆盖率同时当 rgb 与 alpha 的系数 —— 输出必须仍是**预乘**的。写成 `vec4(1,1,1,c)` 是本
阶段踩到的第一个坑：rgb 与 alpha 脱钩后，半覆盖像素按 over 混合直接饱和成实心白方块，
golden 基线里一眼可见（先证明 CPU 影子页是对的，才定位到着色器）。

### 覆盖率页为什么不走 sprite 那套图集

`Atlas` 的页是 `Rgba8UnormSrgb`，而覆盖率是单通道线性量：塞进 alpha 着色器取不到 `.r`，
塞进 rgb 则 sRGB 解码会把 0.5 覆盖弯成 0.216、边缘直接糊掉。所以 `FontAtlas` 自管
`R8Unorm` 页（`PAGE_EDGE = 1024`，正好满足 `COPY_BYTES_PER_ROW_ALIGNMENT`，不必垫 staging），
epoch 带自己的标记位 [`MASK_EPOCH`](../crates/sisf-text/src/atlas.rs)（`1<<61`，与
`LARGE_EPOCH`/`TARGET_EPOCH` 各占一位，低 61 位是页计数）—— 否则两条通路会抢同一个 bind group。
三点与 sprite 图集相反、都是刻意的：gutter **填 0**（文字边缘外本该透明）、**不驱逐**
（一个 (face, 字号, 字形) 落位即活到进程结束）、页满到 `MAX_PAGES = 4` 时**跳过该字形并告警**
（宁缺字也不画错，见 DoD-4）。

### `sync` 的调用时机

字形是在 `draw_*` 里头一次光栅化的，所以：

```
draw_* ……（新字形落位、影子页变脏）…… ▶ TextEngine::sync(renderer) ▶ end_draw / end_frame
```

`sync` 必须**晚于**本帧所有 `draw_*`（早了就没有新页可注册，会得到
"纹理代号 … 未注册，本帧回退白色占位"），又必须**早于** pass 记录（`end_frame`/`end_draw`
在那儿按 epoch 取视图建 bind group）。会话中途 `sync` 也安全：它只建纹理 + `register_epoch`。
`examples/windows.rs` 因此调了两次 —— 会话里一次、主帧末尾一次。

### 字体装载与回退

`FontBundle::from_bytes` 成功即把该族设成 sans/serif 默认族（`FontSystem` 的候选表同分时
"先注册的赢"，而系统字体几乎总是先注册，不设默认族就压不住它）。失败有两条路径，都只
warn 一次（进程级去重表；同帧构造几十个 `Bitmap` 不该刷屏）：`Empty`（VFS 里没这个文件）、
`Unparsable`（不是 ttf/otf）。单测三条路径都在 `bundle.rs`。

多份字体可以叠加（`register_bytes`，拉丁 + CJK 各一份是常见做法）：每注册一份就把 sans/serif
默认族换成它，所以**注册顺序有意义** —— 先 CJK 后 Inter，拉丁才仍出自 Inter，中文靠 per-glyph
回退落到 CJK 那份。`family()` 随之说的是**最后**注册的那一份（与默认族一致，不是第一份，
也不会在第二次注册后停在 `None` 上撒谎）；其余族只作为回退候选存在。

## 位图渲染目标与绘制会话

`crates/sisf-render/src/bitmap.rs`。RGSS 的 `Bitmap` 是一等公民，所以这里直接是
`Renderer` 的方法而不是新对象：

| RGSS | 本引擎 | 备注 |
| --- | --- | --- |
| `Bitmap.new(w,h)` | `Atlas::new_target(w,h,now)` → `Source::from(&bitmap)` | 独占一整页（`PageKind::Target`），页尺寸精确、不抬到 2 的幂 |
| `bitmap.blt(x,y,src,src_rect,opacity)` | `Renderer::blt(dst,&src,src_rect,opacity)` | 画到**当前目标**；`src_rect` 越界夹回源内并同步挪 dst；**不缩放**（`stretch_blt` 是 T8） |
| `fill_rect(x,y,w,h,color)` | `Renderer::fill_rect(rect,rgba)` | alpha-over 纯色 |
| `clear_rect(x,y,w,h)` | `Renderer::clear_rect(rect)` | wgpu 没有"按矩形 clear 纹理"，这是一块 `Blend::Replace` 的 quad |
| `draw_text(rect,text,align)` | `TextEngine::draw_text(&mut renderer, rect, text, style, color)` | 会话里 ⇒ 画进那张位图 |

会话规则（`begin_draw` → 指令 → `end_draw`，`end_draw` 自带一次 submit）：

- 目标必须**独占整页**且已注册（先 `apply_atlas`），否则 `NotExclusive` / `Unregistered`。
- **一次一个会话**，不能嵌套（往 RT 上画时再开一个会话 ⇒ `PaintError::AlreadyDrawing`）；但
  **可以嵌在主帧收集中途** —— 会话有自己的 encoder，所以"先画位图、再把它 blt 上屏"就在同一帧里
  （`examples/windows.rs` 正是这个形状：`begin_frame` 之后开、之后关，再往主帧里 push）。
- 会话内**按提交顺序**成批（`build_in_order`）：`fill_rect → blt → draw_text` 的指令顺序
  就是覆盖顺序，排序会把后画的埋到下面。
- 投影恒 `ortho(w,h)` ⇒ 会话坐标就是位图像素，与 viewport 栈无关。
- 目标页只在**第一次被画之前**清成透明（`pristine`），之后一律 `LoadOp::Load`：位图是持久
  画布，`clear` 得显式 `clear_rect`。滚动文字忘记 clear 就会糊成一片（demo 里能看到）。
- 源 epoch == 当前目标 epoch 时 `blt` 跳过并告警：读写同一张贴会撞 wgpu 校验。

⚠️ **句柄必须活着。** `Bitmap` 析构 = 页退役，下一帧 `apply_atlas` 就把那张纹理删了。
`examples/windows.rs` 因此存 `Bitmap` 而不是 `Source`（`Source` 每次现算），第一版就是因为
RT 句柄掉在 `new()` 里而直接 panic `Unregistered`。

## clip 栈

`crates/sisf-render/src/clip.rs`。`push_clip(layer, rect)` 把该层投影下的内容矩形换成
目标像素 scissor，返回一个 `u32` 代号；代号进 `BatchKey::clip`，于是
**"clip 变更即断批"是排序的天然结果**，不需要任何显式 flush 调用（清单里那句"flush batch"
就是这么满足的）。验收：`session_clip_scissors_and_breaks_batches`（会话内断批次数与栈深）、
`nested_clips_intersect_and_pop`（嵌套求交）、`degenerate_projection_clips_nothing_visible`。

- `NO_CLIP`（0）恒等于"整个目标" ⇒ 主帧与各位图会话**共用一张表**也不会串味；其它代号存的是
  该目标自己的绝对像素矩形。
- 嵌套靠求交，交空得到空矩形 ⇒ 记录时直接跳过 draw，不下发 0 宽 scissor。
- `pop_clip` 栈空时 no-op。`clip_depth()`/`clip_ids_used()` 报的都是**当前落位目标**的栈。
- `draw_text` 本身**不裁剪**（RGSS 的文字本来就允许溢出 `rect`）：要裁就外面套 `push_clip`。
- 层号：`draw_text` 恒落在 viewport 栈的 layer 0（与 RGSS 的"画进当前目标"对齐）。要把文字压在
  相机层之上得显式换层 —— `draw_text_at(.., layer)` / `text_quads(.., layer)` / `draw_run(.., layer, z)`，
  层号同时写进 `BatchKey::layer` 与 `Instance::layer`（排序读前者，着色器读后者，不一致就错位）。
  为什么不能靠大 `z`：批的排序先看 `layer`，layer 0 的字盖不住 layer 1 的地图（T8e 实测缺陷）。

## 文字光栅超采样（T8 修掉的「非整数倍率下发虚」）

原为 T6 遗留缺陷。复现：`cargo run --release -p sisf --example windows`，默认窗口尺寸下
第一帧日志是 `[windows] 启动 320x240 物理 (800, 600) scale 2.50 文字档位 4x` —— 修之前
12/16px 的字明显软。

成因链是**分辨率**问题，不是通路问题：

1. 覆盖率原本按逻辑尺寸光栅。`layout` 用 `Metrics::new(style.size, line_height)`，
   `FontAtlas::rasterize` 直接拿这个 `CacheKey` 问 swash 要位图，落位时
   `quad.size = 位图 px 尺寸` ⇒ **1 个覆盖率纹素 = 1 个虚拟像素**，没有超采样档。
2. layer 0 把虚拟映射进 letterbox。`FrameCtx::recompute` 的 `Fit` 分支算出 `scale = sx.min(sy)`，
   `ortho_viewport` 按 `[w/vw, h/vh]` 放大 ⇒ 这一帧所有纹素都被 `Linear` 采样器拉伸 2.5 倍。
   16 级纵向覆盖细节摊到 40 个物理像素上，就是"糊"。
3. 窗口里的字还多走一层位图，但那是一次 1:1 `blt`（`WIN` 位图按虚拟像素贴回），
   所以**没有**二次采样损失 —— 左上角那行直接画在主帧里的 12px 状态文字同样软，两者同源。

**为什么不是"把采样器换成 Nearest"**：缺的是信息量而不是对齐。最近邻只会把平滑插值换成
阶梯，字变小且笔画抖动；而且这个 sampler 是**全页共享**的一个（`renderer.rs:282`），
背景那种整帧拉伸正依赖它。golden 也不覆盖这个场景 —— `text_scene.png` 是 544×416 离屏、
scale 恰好 1，基线里的字本来就是清晰的。

### 二选一：选光栅超采样档，不做全分辨率文字层

T8 的两条候选里选了 **1. 光栅超采样**。全分辨率文字层（UI 文字不进虚拟分辨率通路、投影
直接用物理 ortho）确实最省显存，但它把"整帧像素风"劈成两套坐标系：文字不再受 viewport
栈、clip 栈与 `Instance::tone` 的统一管辖，RGSS 的 `Bitmap#draw_text`（画进**位图**，位图
当然是虚拟分辨率）会立刻变成两套语义；而且 RPG Maker MZ 自己的真实行为就是整帧 canvas
重采样，与现状一致。超采样档则一行 API 都不新增通路。

落地是三个动作，`layout` 完全不知道档位存在：

- `atlas::raster_scale_for(scale)`：把"虚拟 → 物理"的有效倍率**向上**取到 2 的幂，
  档位 ∈ {1, 2, 4}（`MAX_RASTER_SCALE`）。倍率 ≤1（缩小采样）恒为 1 档：多光栅的像素会被
  扔掉，只白吃页容量。整数 2× 落 2 档，所以像素风那条通路也不吃亏。
- `FrameCtx::raster_scale()` 给出档位，调用方在 `tick`/`on_event`（拿得到 `ctx` 的地方）
  喂给 `TextEngine::set_raster_scale`。`examples/windows.rs` 就是这么接的。
- `FontAtlas::glyph` 查缓存前先 `synth_key`：把键里的 `font_size_bits` 换成
  `字号 × 档位`、两个 `SubpixelBin` 归零，于是位图按高档光栅，而 **1 档与 2 档的同一个字形
  在 `glyphs` 里是两个条目**（正是 T6 那条备注担心的"同一 (face,size) 在不同倍率下各占一份
  装箱"——它是有意的，换档不失效、不重排）。

两处随之而来的细节，都是刻意的：

- **`GlyphRect` 现在带 `scale`**，`size` 是**档内**像素、`offset` 是**逻辑**像素。绘制方
  算 quad 时 `w = size[0] / scale`。把档位记在条目里而不是让调用点去问图集，是因为图集里
  同时住着几档，漏问一次就画出一个 2 倍大的字。
- **bin 归零 ≠ 丢亚像素定位**。1 档时那个 ≤0.375 逻辑像素的余量由 swash 烘进位图；高档
  把它取出来加在 `offset` 上（即落在 quad 位置上），所以字形之间的相对排布与 1 档一致，
  只是"从改位图变成改 quad"。

### 代价：页容量按档位平方涨

`MAX_PAGES = 4`（每页 CPU+GPU 各 1MB）没动。2 档让一个字形吃 4 倍面积、4 档 16 倍，所以
档位跨过去之后页更容易满 —— 满了按既有约定**跳过新字形并告警一次**（宁缺字不画错），
不会画错东西。因此 `set_raster_scale` 的文档写明"只在窗口尺寸变化时调，不要每帧调"：
拖窗口时每跨一档就多一套字形进页。真要长期跑四档 + 多字号，抬 `MAX_PAGES` 是唯一的旋钮
（先确认字号档位数量）。

验收：`tiers_round_up_to_powers_of_two`（档位表）、
`supersampling_adds_detail_without_moving_the_glyph`（位图翻倍、逻辑尺寸与落点不变、条目共存）、
`every_tier_keeps_the_same_relative_layout`（三档逐字对齐）、
`supersampling_changes_coverage_detail_not_the_quads`（从 `draw` 入口看：uv 跨度翻倍而
`Instance.size/pos` 一像素内不动）、`text_raster_tier_follows_the_upscale`（`FrameCtx` 侧）。
桌面手测：把窗口从 1:1 拖到 2.5×，日志里的"文字档位"跟着 1x→2x→4x，字在 2.5× 下明显比
修前硬。

## 平台矩阵

| 平台 | 文本 | 位图会话 | clip | 说明 |
| --- | --- | --- | --- | --- |
| Windows / Linux（native） | ✅ | ✅ | ✅ | 系统字体兜 CJK，自带字体只管拉丁 |
| Android（native） | ✅ 编译面 | ✅ | ✅ | `FontSystem::new()` 会扫系统字体；真机手测归 T12 一起走 |
| wasm32 | ✅ 中文靠运行期取字库 | ✅ | ✅ | `cargo xtask web windows` + `?ex=windows&wgl` |

wasm 的三条硬约束（不是空实现，是平台事实）：

1. **没有系统字体**（fontdb 在 wasm32 上扫不到任何东西）。自带字体字节是唯一来源，
   而仓库里那份 Inter 只有拉丁 —— 中文要 CJK 字体才有字形。8 MB 的整包不入库，
   所以**字形来源由宿主在运行期补**：示例各自从 CDN `fetch` 一份 Noto Sans SC
   （OFL、全字库、OTF 而非 woff2、jsDelivr `/gh/` 钉 commit SHA），塞进
   `WebManifest`（`insert` 允许清单外的键），到货前 `Cjk` 状态是 pending、到货后
   先注册 CJK 再注册 Inter（注册顺序 = 默认族顺序，拉丁仍出自 Inter，中文走
   per-glyph 回退）。取不到就是中文方块 + 状态行一句 `cjk MISSING`，拉丁照常 ——
   一份永远不来的请求不能把本来能显示的拉丁一起拖死。**引擎不管这个 URL**：
   网络 fetch 归宿主（`crates/sisf-asset` 的约定），`FontBundle` 只多了一条
   `register_bytes` 的调用。
2. **`eprintln!` 在浏览器里看不见**。`FontBundle` 的回退 warn 因此只在桌面/Android 有效；
   wasm 上自查用 `FontBundle::fallback()` / `is_fallback()` / `face_count()`。
3. **一个 face 都没有时不能把请求交给 cosmic-text**（T8e 实测）。`shape.rs` 里那句
   `font_iter.next().expect("no default font found")` 会直接 panic，而 wasm 上"字体还在 fetch
   路上"是每帧都会经过的常态 ⇒ 排一次字就把整条 rAF 链打断，页面定格成黑屏。所以
   `layout_with` 先查 `db().is_empty()` 就返回空 `TextRun`（行高照常给，盒布局不塌）：
   "没字体 = 字形不出现"这条约定到这里才真的成立。断言
   `an_empty_font_system_lays_out_nothing_instead_of_trapping`（含"有字体时必须排得出字"的反向对照）。
4. **换字体族之前一个字都不能画过**（第 1 条的运行期注册把它变成了常规路径，不再是纯启动期
   的事）。`CacheKey` 里带 `font_key`，而 `FontSystem` 的 face 编号会随注册次序重排 ⇒ 中途换
   bundle 会让覆盖率图集里的旧页按新编号被误读，串出别的字。示例的画法是把所有文字都 gate 在
   `font_loaded` 之后（`examples/dialog_demo.rs` 的窗口与 HUD 都如此）。

## 能力矩阵（不支持项）

| 能力 | 本阶段 | 说明 |
| --- | --- | --- |
| 中英混排 / 自动换行（`WordOrGlyph`）/ bidi | ✅ | `Shaping::Advanced`；bidi 只由 cosmic-text 提供，未做黄金验证 |
| 字号 12/16/24 三档 | ✅ | golden `text_scene.png` 同幅验证 |
| 光栅超采样档 1/2/4（非整数倍率不发虚） | ✅ T8 | `raster_scale_for` + `FontAtlas::set_raster_scale`，见上一节 |
| 左/中/右对齐、垂直居中 | ✅ | 对齐在绘制期按 `TextLine::width` 现算，不交给 `set_text` |
| 彩色字形（CBDT/COLR）、亚像素覆盖 | ❌ 显式跳过 + 告警一次 | 塞进 R8 页会画出错色，不放空实现 |
| 字形驱逐 / 字号变化后的页回收 | ❌ | 见 `atlas.rs` 模块头；游戏字号只有几档，页满则跳字 |
| RT 的 CPU 读回（位图渲染目标的像素回 CPU） | ❌ | 仍然只有 GPU 侧。兼容层那两格 `Bitmap.getPixel`/`getAlphaPixel` **不走这条路**：它们读的是解码镜像与操作流回放（`crates/sisf-rpgmkmv/src/raster.rs` 的 `Rasters::pixel` + `composite::replay`/`replay_with_text`），而 `fillText` 要的**字形覆盖率**走新增的 `FontAtlas::glyph_coverage`（`atlas.rs`，把图集页那份唯一的 CPU 影子缓冲借出去读一次，不另光栅一份）|
| 会话嵌套（往 RT 上画时再开一个 RT 会话） | ❌ 报错 | 需要嵌套 render pass，`PaintError::AlreadyDrawing` |

## 验收与闸门

```
cargo xtask golden          # 现在跑 GOLDENS 两条：sprite_scene + text_scene
cargo xtask golden --bless  # 重写基线（确认过差异是预期变更之后）
cargo test --workspace      # 文本 30 单测 + golden 1；渲染侧新增 clip/会话 16 单测
cargo xtask check-all && cargo xtask clippy && cargo xtask fmt --check
cargo run --release -p sisf --example windows        # 桌面手测
cargo xtask web windows                              # wasm：http://127.0.0.1:8080/?ex=windows&wgl
```

- `crates/sisf-text/tests/golden_text.rs`（`#![cfg(windows)]`）：三档字号横幅 + 三种对齐窄带
  + 一块"会话里画字再 blt 上屏"的 RT，语义断言（墨迹随字号单调、对齐起点递增、声明盒外必须
  干净）与逐通道比对各跑一遍。每段文字都套了 `push_clip`，既验收裁剪通路，也让"盒外干净"
  这条断言与字形宽度无关。
- T6 的 gutter 改动**重写了 `sprite_scene.png`**：格边从 248 一类的混合值变成干净的 255，
  语义断言一条没改（见 `docs/sprite.md`）。

## 依赖与资源登记

- `cosmic-text 0.19.0`、`etagere 0.3.0`：已登记在 `todo.md`「附：钉版增补」，`sisf-text` 走
  `{ workspace = true }`。
- `Inter-Regular.ttf`（SIL OFL 1.1，许可证同目录 `Inter-LICENSE.txt`）：仓库里两份副本 ——
  `crates/sisf-text/tests/fixtures/`（单测与 golden 用）与 `web/assets/`（wasm 手测用，
  已进 `web/assets/manifest.txt`）。
