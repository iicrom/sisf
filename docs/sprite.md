# T5：sprite 语义与能力矩阵

`crates/sisf-sprite` 只做一件事：把"引擎语义"的可视单元（RPG Maker 式 `Sprite`）翻译成
渲染器的 `(BatchKey, Instance)`。数据在 `sprite.rs`、存放在 `arena.rs`、翻译在 `draw.rs`，
语义只落码在 `draw.rs` 一处，所以它的单测就是这条通路的定义。

```
SpriteArena ──collect(ViewportRef)──▶ BatchList<Instance> ──append_list──▶ Renderer::end_frame
   (槽+代号)        过滤/排序/uv          (layer,epoch,blend,z,insertion)     分批 + 两条管线
```

## 能力矩阵

| 能力 | 本阶段 | 说明 |
| --- | --- | --- |
| `Tone::Offset{rgb,gray}` | ✅ | 着色器一条通路：`rgb` 是各通道加法偏移（[-1,1]），`gray` 是与自身亮度的混合比 |
| `Tone::Matrix([f32;20])` | ❌ 显式报错 | 采集时返回 `CollectError::UnsupportedTone{sprite}`，指认元凶句柄；不静默当无效果（DoD-4） |
| `Blend::Normal`（预乘 alpha-over） | ✅ | 管线 1 |
| `Blend::Add` | ✅ | 管线 2 |
| `Multiply` / `Screen` | ⏸ 不引入 | `BlendMode = sisf_render::Blend`，与其放一个映射不到的变体，不如等 T12 一起收口（加 pipeline 或删 RGSS 语义） |
| anchor / flip_x / flip_y / rotation / scale | ✅ | 见下 |
| `src` 子矩形 + 越界裁剪 | ✅ | 裁进位图，不拉伸回来 |
| 多 viewport / 多 layer | ✅ | `Layer(u8)` + `ViewportRef`，同一 arena 可被多个可视口各采一次 |
| 动画（`AnimDef` / MoveRoute） | ✅ T8 | 见下"T8：固定拍上的动画与任务" |
| 位图渲染目标作源（`Sprite::bitmap` 是 RT） | ⏸ T6 | 采集端已与来源解耦（只要 `Bitmap` 有 epoch + 页内 rect） |

## 语义

**锚点同时是 `pos` 的对齐点和 `rotation` 的轴心。** 着色器只认"实例中心"这一个枢轴
（`shaders/sprite.wgsl` 里 `half = (corner-0.5)*abs(size)`，`mirror=sign(size)` 在旋转之后施加），
所以 CPU 侧先把"锚点 → 矩形中心"的偏移按 `rotation` 转一遍再加到 `pos` 上：
`center = pos + R(rot) · ((0.5-anchor)·|size|)`。差一个符号就是"绕中心转但锚点跑到别处"，
单测 `rotation_pivots_on_anchor` 钉住锚点到中心的距离在旋转后不变。

**翻转是 `size` 的符号**，不是交换 uv（T3 决定）。`flip_x/flip_y` 与可为负的 `scale` 叠加，
`uv0/uv1` 恒保持递增。

**顺序**：提交给 `BatchList` 的是 `(layer, epoch, blend, z, insertion)`，排序后先按
layer、再按图集页、再按 blend 分批次，`z` 只在同批次内决定先后，`z` 相同保槽序。
换句话说 `z` **不能**跨页或跨 blend 改变覆盖关系——这是合批的代价，跟 RPG Maker 的
"viewport 内 z 全序"略有出入，需要严格跨页 z 时要显式分帧画。

**`src` 越界裁进位图**（`place`）：裁掉的那部分连同它的目的像素一起消失，剩下的保持
"1 源像素 → `scale` 目的像素"，而不是把窄条拉伸回原尺寸。零尺寸、`src` 与位图无交集、
`!visible`、`opacity<=0` 都在采集时跳过（不产出实例）。

**无位图 = 内置 1×1 白图**（`WHITE_EPOCH`）：`Sprite::solid(size)` 画纯色矩形，此时只有
`src.w/h` 有意义且直接就是目的尺寸（虚拟像素）。T7 之前的占位/调试绘制走这条路。

**采集是全成败的**：`collect` 先在临时 `Vec` 里攒完再提交，遇到 `Matrix` tone 报错时不会
留下半批实例（`insertion` 由 `push` 填，半途提交会让序号与实例对不上）。

## 多 viewport

`ViewportRef{layer, tag}`：`layer` 是投影栈层（0..7，写进 `BatchKey::layer` 与实例的投影索引，
必须是同一个数），`tag` 是归属过滤（`Some(t)` 只收 `Sprite::layer == t`）。
`ViewportRef::new(Layer(2))` 画第二层的精灵，`ViewportRef::all(Layer(1))` 把整个集合重画进
另一层（小地图/镜像视口用它）。同一个 arena 被两个可视口各采一次、产物 `append` 进同一张
`BatchList`，两次的输出互不影响 —— 断言见 `two_viewports_collect_the_same_arena_independently`
与 golden 里的 `Layer(2)` 格。

`Layer` 一个类型同时当"精灵归属标签"和"投影栈层"用：RPG Maker 里这两个概念本来就是绑定的
（精灵属于哪个 viewport 就用哪个 viewport 的投影），拆成两个类型只会让它们失同步。

## T8：固定拍上的动画与任务

时间单位换了：`sisf-app::timing::TickClock` 把渲染帧率挡在逻辑之外（累加器 + 一帧最多补
`MAX_CATCHUP = 5` 拍 + `draw(alpha)` 用余量插值），`anim.rs`/`task.rs` 因此**只用整数拍**记进度。
补间在第 12 拍到位、路线在第 30 拍走完，120Hz 与 30Hz 屏幕上完全一样。

**动画有两条互斥的通路**（`AnimDef::pattern` 是那条岔路）：

| | 时间驱动（`pattern: None`） | 移动驱动（`pattern: Some(DirPattern)`） |
| --- | --- | --- |
| 帧号来自 | `frames[i].dur_ms` 逐帧走，`LoopMode` 管停/回环/往返 | `(dir, step)` 的纯函数：`dir_row(dir) * cols + step % cols` |
| 谁推进 | `SpriteArena::tick_anims(dt)`，只碰 `pattern: None` 的精灵 | 调用方（`MoveRoute` 每完成一格 `set_step`，或游戏逻辑） |
| 静止时 | 照常在换帧（火把、灯火） | **永远停在第 0 列** —— 验收那条"静止不换帧"就是这个意思 |

`DirPattern` 两个变体只差列数：`RpgMaker4x3` 是 3 步帧（0 为站立帧）、`RpgMaker2x2` 是 2 帧
（VX/Ace 的两帧走路）；行侧恒为 `下、左、右、上`（方向键 `2/4/6/8`），帧表按行优先读，与
`AnimDef::grid` 的装箱序一致。

**`Sprite::dir()` / `step()` 是动画状态，不是普通字段**：精灵没挂 `AnimDef` 时 `Turn` 节点与
`set_dir` 都是 no-op（没有图样就不知道"朝上"该取哪块 `src`）。写示例时先踩了这个坑 ——
断言 `MoveRoute` 的 `Turn` 生效，得先给精灵一条 `AnimDef::grid_pattern(..)`。

**任务**（`task.rs`）三条约定：一 tick 只推进一个节点（`Turn`/`FacingLock` 这类瞬时节点也占一拍，
路线时长 = 各节点拍数之和，完全可预测）；任务绑定一个 `SpriteRef`，精灵被回收后任务自己了断；
队列默认并行，`MoveRoute::wait(true)` 独占该精灵的队列（RPG Maker 事件指令的"等待移动完成"）。
`MoveRoute` 有网格与自由两种模式，网格模式逐格走、`walkable` 钩子拿的是**逐拍坐标**且必须 own
通行表（示例里因此是 `pass.clone()` 进闭包），撞墙只作废本节点的剩余格数、不作废整条路线。
**"路线时长 = 各节点拍数之和"是有代价的**：`step` 在最后一格走完的**那一拍**就要返回 `true`
（`finish` 当场收尾），不能留一拍"下一趟才报告完成"。看着无害的收尾拍会让一个被消耗的周期不推进
`prev_pos`/`pos`，而 `TickClock` 照样消耗周期、把 `alpha` 归零 ⇒ 渲染端把最后一步重播一遍
（每格一次），整条路线还慢一拍一格（`tile/(ceil(tile/speed)+1)` 而非声明值）。不变量：
**每个被 `TickClock` 消耗的周期都必须推进 `prev_pos`/`pos`**。

**但这条不变量不该由每个消费者各自守住 —— 它由 `SpriteArena::begin_tick` 统一兜住**。
只有 `set_pos` 会推进 `prev_pos`，于是"这一拍没动"的精灵（走一格后的停拍、`RouteNode::Wait`、
撞墙被作废）会把 `prev_pos` 永远留在最后一跳上，被 `alpha` 的 `0→1` 一遍遍重播。收尾拍修掉之后
剩下的这一半才是第四次报回的抖动（"红色人物移动后人物抖，玩家角色移动后地图抖"）：NPC 在 `Wait`
里来回弹，玩家角色一站定，被相机带着的就是整张地图。**约定：`App::tick` 的第一行调一次
`arena.begin_tick()`**（把全场 `prev_pos` 收拢成 `pos`），顺序有讲究 —— `tick_anims` 在拍尾，
那时收拢等于把本拍位移当场抹掉。位移本身仍走 `set_pos`/`teleport`，直接写 `pos` 依然禁用。

**插值点只有一个消费者是不够的：相机也得看见它**。`collect_at(view, alpha, ..)` 把精灵画在
`Sprite::interpolated(alpha)`，而跟随相机是 viewport 栈上的一层、投影在 `draw` 之前就烘完了 ⇒
相机必须取同一个 `interpolated(alpha)`（`App::prepare`，见 `docs/platforms.md`）。放 `tick` 末
= 相机钉拍尾 ⇒ 角色每拍相对地图回拽 `(1 - alpha) × 每拍位移`，一步之内来回跳；这是逐拍位移
（`MoveRoute::walk`）独有的坑，静止的世界图块 `prev_pos == pos`，插值对它们是恒等的。
但"同一个插值点"要把 `alpha` 折到像素格点上（示例里的 `lattice_alpha`）：精灵与地图都按
`interpolated(alpha)` 的**小数**位置画出来时，抖的就换成整张地图了 —— 线性采样下硬边缘被摊到
两个纹素之间、摊法每帧再变，判据与量法见 `docs/platforms.md`。

验收：`anim.rs` 15 条（两条图样的朝向行表与四向帧索引、静止不换帧、短帧表夹住不外跑、
`begin_tick` 收拢全场）、`sprite.rs` 5 条（默认值、字段可原地写、`Tone::Matrix` 无通路、
`set_pos`/`teleport` 的插值对、`begin_tick` 抹掉的是"上一跳"而不是"本拍位移"）、
`task.rs` 16 条（补间按拍回调、撞墙、wait 队列、`MoveTo` 逐轴、
`Jump` 跨缝、`FacingLock`、`Wait` 期间画出的点恒定）、`timing.rs` 6 条（不足一周期不跑逻辑、
`alpha` = 余量、尖峰只补 5 拍并记 `dropped`、`reset` 冻结）；端到端在 `examples/tiled_map.rs`
（三层 tilemap + 键盘四向行走 + 事件格碰撞，外加"松手后相机逐帧不动"与 4 方向 × 2 倍率 × 6 节拍
的相机不回退穷举）。

## golden

`crates/sisf-sprite/tests/golden_sprite.rs`，`cargo xtask golden`（`--bless` 重写基线）。
离屏 544×416 + `COPY_DST` 读回，真 adapter（Vulkan/DX12），64×64 单元格 5 列 × 4 行，
源图是 8×8 的四象限标记图（左上红 / 右上绿 / 左下蓝 / 右下白，预乘 RGBA8）：

| 格 | 用例 | 断言 |
| --- | --- | --- |
| 1 行 | 基准 / `flip_x` / `flip_y` / `anchor(0.5,1)` / `visible=false` | 象限置换表；锚点格与基准格逐点全等；不可见格仍是清屏黑 |
| 2 行 | `opacity=0.5` / `Tone::offset([0,0.5,0])` / `gray=1.0` / `rotation=90°` | 0.5 线性白 → sRGB 188；红 +0.5 绿 → G≈188、R 饱和；红灰度化 → 亮度 0.2126 → 127；90° 象限置换 `[BLUE,RED,WHITE,GREEN]` |
| 3 行 | additive 对 / alpha-over 对 / z 序 / `src` 越界 | 0.5+0.5 → 1.0（饱和）；0.5 over 0.5 → 0.75 线性（225）；`z=5` 的红盖住 `z=-5` 的绿；越界只剩 2×2 源像素 → 32×32 目的像素，被裁的一半不出现 |
| 4 行 | `Layer(2)` 经第二个 `ViewportRef` 采集 | 同一帧里第二层格完整出图 |

断言分两层：**语义断言**独立于基线文件，写反翻转/锚点/z 序时直接指出是哪一条；
**基线比对**逐通道 `TOL=1` 字节（sRGB 编解码往返本身就有 ±1 量化抖动）。
两条都跑在同一批读回像素上。bless 之后再跑一次比对结果一致 ⇒ 该场景在这台机器上可复现。

⚠️ 整个测试文件是 `#![cfg(windows)]`：wgpu 30 没有软件光栅后端，离屏渲染同样要可用 adapter，
CI 里若没有 GPU 就会拿不到 adapter。跨平台的无窗口冒烟由 `crates/sisf-render/tests/readback.rs`
（清屏读回）负责。非 Windows 上 `cargo xtask golden` 编译成 0 个用例并通过，不代表像素验证过。

采样点一律取在象限/单元格中心，见下。

## 已修：图集 `src` 边界的半纹素渗色（T6）

页纹理采样器是 `Linear` + `ClampToEdge`。T5 装箱时子矩形之间没有 gutter，于是放大绘制某个
`src` 子矩形时，边界最后半个纹素会采到隔壁像素（页上相邻的那张图），基线图里能看到一圈
1–2px 的色边。影响面：所有"从共享页里取子矩形 + `scale>1`"的绘制。

T6 起装箱四周留 `GUTTER = 1px` 复制边（`AtlasLayout` 的 `upload` 矩形比 `region` 每边宽
2px，上传时把边缘像素复制出去；贴住页边的两侧省掉，`ClampToEdge` 复制的正好是自己的边缘
像素）。**基线因此重写**：`cargo xtask golden --bless` 后 `sprite_scene.png` 的格边由
"248 一类的混合值"变成干净的 255，语义断言一条没改。

`Bitmap` 的 `filter: Linear|Nearest` 双 sampler 仍然没做 —— 像素风要不要最近点采样是 T10
的事，届时再决定，别提前铺接口。

## T5 与清单的差

1. ~~**`Sprite` 没有 `anim` 字段**~~ —— T8 补上了（`Option<AnimState>` + `SpriteArena::tick_anims`），
   采集路径仍然不知道动画存在：换帧只是把 `rect()` 写回 `src`，`collect` 一个分支都没多。
2. **`BlendMode` 是类型别名**（`= sisf_render::Blend`）而不是新枚举 —— 变体必须与管线一一对应，
   多出来的 `Multiply/Screen` 无处可去。
3. **代号字段叫 `generation` 不叫 `gen`**：edition 2024 把 `gen` 列为保留关键字，
   `SpriteArena{slots, free, generation}` 与 `SpriteRef::generation()` 都受影响（17 处编译错误换来的教训）。
4. **golden 落在 crate 内**（`crates/sisf-sprite/tests/golden/sprite_scene.png`），不是仓库根
   `tests/golden/`：基线图跟着被测 crate 走，`env!("CARGO_MANIFEST_DIR")` 定位，多 crate 加 golden 时不必再约定根目录子结构。
5. 为了离屏读回，`sisf-render` 新增 `Renderer::new_offscreen` / `Target::Offscreen` /
   `FrameStatus::Rendered` / `Renderer::readback` / `Renderer::apply_atlas`；`Bitmap` 补了手写
   `Debug`（`Sprite` 要 `derive(Debug)`，而 `Bitmap` 内部是 `Arc` 到 GPU 资源，自动派生没意义）。
   `png` / `pollster` 是 `sisf-sprite` 的 dev-dependencies，不进运行时依赖面。
