//! 采集：arena 里的精灵 → `(BatchKey, Instance)`。
//!
//! 这一步把"引擎语义"（锚点、翻转、缩放、tone、z、`src` 裁剪）翻译成着色器契约
//! （中心点、带符号尺寸、页内 uv 子矩形、tone 偏移向量），所以 anchor/flip/裁剪的
//! 语义只在这里落码，单测覆盖它就等于覆盖了整条通路。能力边界见 `docs/sprite.md`。

use sisf_render::{BatchKey, BatchList, Instance};
use thiserror::Error;

use crate::arena::{SpriteArena, SpriteRef};
use crate::sprite::{Layer, Sprite, Vec2};

/// 一次采集的目标可视口。
///
/// 一次 `collect` 只喂**一个 viewport**：`layer` 同时写进 `BatchKey::layer` 与实例的
/// 投影索引，两者必须一致（`FrameCtx::push_viewport` 返回几就填几）。同一个 arena 可以
/// 被两个 viewport 各采一次，产物再 `append` 进同一张 `BatchList`（T6 的位图 pass）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewportRef {
    /// 投影栈层（0..7）。
    pub layer: Layer,
    /// 归属过滤：`Some(t)` 只收 `Sprite::layer == t` 的精灵，`None` 收全部。
    ///
    /// 默认（`Some`）保证"多个可视口共用一份 arena"时各画各的；[`ViewportRef::all`]
    /// 用于把整个集合重画进另一个层。
    pub tag: Option<Layer>,
}

impl ViewportRef {
    /// 主视口：层 0，只收 `Layer::MAIN` 标签的精灵。
    pub const MAIN: ViewportRef = ViewportRef {
        layer: Layer::MAIN,
        tag: Some(Layer::MAIN),
    };

    /// 第 `layer` 层，只收同一 `layer` 标签的精灵。
    pub const fn new(layer: Layer) -> Self {
        Self {
            layer,
            tag: Some(layer),
        }
    }

    /// 第 `layer` 层，忽略标签收全部精灵。
    pub const fn all(layer: Layer) -> Self {
        Self { layer, tag: None }
    }

    fn accepts(&self, s: &Sprite) -> bool {
        self.tag.is_none_or(|t| s.layer == t)
    }
}

/// 采集失败。当前只有一条：无着色器通路的 tone。
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum CollectError {
    /// [`crate::sprite::Tone::Matrix`] 没有着色器通路（T12 定案不做，也不静默当无效果）。
    #[error("Tone::Matrix 无着色器通路（精灵 {sprite:?}）")]
    UnsupportedTone { sprite: SpriteRef },
}

/// 无 arena 来源（[`Sprite::to_instance`]）的占位句柄。
impl SpriteRef {
    pub const ANON: SpriteRef = SpriteRef(u32::MAX, u32::MAX);
}

impl SpriteArena {
    /// 采集可视口内的活精灵，追加到 `out`（不插值，等价 [`collect_at`](Self::collect_at) 的 `alpha = 1.0`）。
    ///
    /// 过滤掉：`visible == false`、`opacity <= 0`、落点面积为零、`src` 与位图无交集。
    /// 提交顺序是槽号序，最终绘制顺序由 `BatchList` 按 `(layer, epoch, blend, z,
    /// insertion)` 排序决定——同批次内即 `z` 升序，`z` 相同保槽序。
    /// 出错时整次采集作废（不留半成品实例）：`Matrix` tone 是写法错误，不是运行时状态。
    pub fn collect(
        &self,
        view: ViewportRef,
        out: &mut BatchList<Instance>,
    ) -> Result<(), CollectError> {
        self.collect_at(view, 1.0, out)
    }

    /// 定步长采集：落点取 `pos` 按 `App::draw(alpha)` 的 alpha 插值。
    ///
    /// `alpha` 只在 `tick_hz` 固定步长下有意义（T8 的 `timing.rs`）：`0` = 上一个 tick
    /// 的位置、`1` = 本 tick 之后的位置，所以 60Hz 渲染 + 60Hz tick 下逐帧位移看起来是
    /// 连续的，而逻辑永远看不到半个位置。只有走过 [`Sprite::set_pos`] 的精灵才会动 ——
    /// 直接写 `pos` 的精灵 `prev_pos` 停在原地（见该字段的文档）。
    pub fn collect_at(
        &self,
        view: ViewportRef,
        alpha: f32,
        out: &mut BatchList<Instance>,
    ) -> Result<(), CollectError> {
        let mut buf = Vec::new();
        for (r, s) in self.iter_live() {
            if !view.accepts(s) {
                continue;
            }
            let Some(inst) = instance_of(s, view.layer, r, s.interpolated(alpha))? else {
                continue;
            };
            buf.push((
                BatchKey {
                    // 采集端不知道会被画进哪个裁剪区，填 0 即可：`Renderer::push`
                    // 会按当前栈顶覆写它。
                    clip: 0,
                    layer: view.layer.0,
                    epoch: s.epoch(),
                    blend: s.blend,
                    z: s.z,
                    insertion: 0,
                },
                inst,
            ));
        }
        // 先全成败再落盘：`push` 会自己填 `insertion`，所以只能在确认无误后提交。
        for (key, inst) in buf {
            out.push(key, inst);
        }
        Ok(())
    }
}

impl Sprite {
    /// 不经 arena 的单精灵换算，配合 [`sisf_render::Renderer::push`] 手工画一块。
    /// 不可见 / 全透明 / 零面积返回 `Ok(None)`。
    pub fn to_instance(&self, layer: Layer) -> Result<Option<Instance>, CollectError> {
        instance_of(self, layer, SpriteRef::ANON, self.pos)
    }
}

/// 精灵 → 实例。`sprite` 参数只用于错误信息里指认元凶，`at` 是本帧的落点
/// （[`Sprite::interpolated`] 的结果；单独传是为了让插值这件事只出现在采集这一层）。
fn instance_of(
    s: &Sprite,
    layer: Layer,
    sprite: SpriteRef,
    at: Vec2,
) -> Result<Option<Instance>, CollectError> {
    if !s.visible || s.opacity <= 0.0 {
        return Ok(None);
    }
    let tone = s
        .tone
        .as_instance_vec()
        .ok_or(CollectError::UnsupportedTone { sprite })?;
    let Some(placed) = place(s) else {
        return Ok(None);
    };
    // 锚点既是 `pos` 的对齐点也是旋转轴心：先把"锚点 → 矩形中心"的偏移按 `rotation`
    // 旋转再加到 `pos` 上，整块矩形就绕锚点转（着色器只认中心点这一个枢轴）。
    let ox = (0.5 - s.anchor.x) * placed.size_abs[0];
    let oy = (0.5 - s.anchor.y) * placed.size_abs[1];
    let (c, sn) = (s.rotation.cos(), s.rotation.sin());
    let center = [at.x + c * ox - sn * oy, at.y + sn * ox + c * oy];
    let mut inst = Instance::rect(
        center,
        placed.size,
        [1.0, 1.0, 1.0, s.opacity.clamp(0.0, 1.0)],
        layer.0,
    );
    inst.uv0 = placed.uv0;
    inst.uv1 = placed.uv1;
    inst.rot = s.rotation;
    inst.tone = tone;
    Ok(Some(inst))
}

/// 落点：目的尺寸（带符号即翻转）+ 页内 uv 子矩形。
struct Placed {
    size: [f32; 2],
    size_abs: [f32; 2],
    uv0: [f32; 2],
    uv1: [f32; 2],
}

/// 算目的矩形与采样矩形；不可见面积（零尺寸、`src` 完全越界）返回 `None`。
///
/// `src` 越界会被**裁进位图**，裁掉的那部分连目的尺寸一起缩——保持"1 源像素 → scale
/// 个目的像素"的观感，而不是把剩下的条带拉伸回原尺寸。
fn place(s: &Sprite) -> Option<Placed> {
    let Some(bitmap) = s.bitmap.as_ref() else {
        // 内置白图（1×1）：`src` 的宽高就是目的尺寸（虚拟像素），uv 铺满整张。
        let (w, h) = (s.src.w * s.scale.x, s.src.h * s.scale.y);
        return nonzero([w, h], s.flip_x, s.flip_y, [0.0, 0.0], [1.0, 1.0]);
    };
    let [px, py, bw, bh] = bitmap.rect();
    let (pw, ph) = bitmap.page_size();
    let (px, py) = (px as f32, py as f32);
    let (right, bottom) = (px + bw as f32, py + bh as f32);
    let x0 = (px + s.src.x).clamp(px, right);
    let x1 = (px + s.src.x + s.src.w).clamp(px, right);
    let y0 = (py + s.src.y).clamp(py, bottom);
    let y1 = (py + s.src.y + s.src.h).clamp(py, bottom);
    let u = |v: f32| v / pw as f32;
    let v = |h: f32| h / ph as f32;
    nonzero(
        [(x1 - x0) * s.scale.x, (y1 - y0) * s.scale.y],
        s.flip_x,
        s.flip_y,
        [u(x0), v(y0)],
        [u(x1), v(y1)],
    )
}

/// 零尺寸即不可见；否则把翻转编进 `size` 的符号。
fn nonzero(
    size: [f32; 2],
    flip_x: bool,
    flip_y: bool,
    uv0: [f32; 2],
    uv1: [f32; 2],
) -> Option<Placed> {
    if size[0] == 0.0 || size[1] == 0.0 {
        return None;
    }
    Some(Placed {
        size: [
            if flip_x { -size[0] } else { size[0] },
            if flip_y { -size[1] } else { size[1] },
        ],
        size_abs: [size[0].abs(), size[1].abs()],
        uv0,
        uv1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    // T12：`cargo xtask wasm-test` 在 wasm 上跑这一组（见 docs/platforms.md）。
    use crate::sprite::{Rect, Tone, Vec2};
    use sisf_render::{Atlas, Blend};
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    /// 一张 `128×128` 页上的 `32×16` 位图；返回位图与其页内矩形。
    fn bitmap_at(w: u32, h: u32) -> sisf_render::Bitmap {
        Atlas::new()
            .new_bitmap(w, h, vec![0u8; (w * h * 4) as usize], 0)
            .unwrap()
    }

    fn solid(x: f32, y: f32, w: f32, h: f32) -> Sprite {
        Sprite {
            pos: Vec2::new(x, y),
            src: Rect::with_size(w, h),
            ..Default::default()
        }
    }

    #[test]
    fn anchor_offsets_center_from_pos() {
        // 左上锚点：pos 是矩形左上角 → 中心在 (pos + 尺寸/2)。
        let tl = solid(10.0, 20.0, 8.0, 4.0)
            .to_instance(Layer::MAIN)
            .unwrap()
            .unwrap();
        assert_eq!(tl.pos, [14.0, 22.0]);
        // 中心锚点：中心即 pos。
        let mut c = solid(10.0, 20.0, 8.0, 4.0);
        c.anchor = Vec2::CENTER;
        assert_eq!(
            c.to_instance(Layer::MAIN).unwrap().unwrap().pos,
            [10.0, 20.0]
        );
        // 底边中点（RPG Maker 的脚底对齐）：中心在 pos 上方半个高。
        let mut b = solid(10.0, 20.0, 8.0, 4.0);
        b.anchor = Vec2::new(0.5, 1.0);
        assert_eq!(
            b.to_instance(Layer::MAIN).unwrap().unwrap().pos,
            [10.0, 18.0]
        );
        // 缩放跟着锚点走：2x 之后底边中点仍落在 pos。
        b.scale = Vec2::new(2.0, 2.0);
        assert_eq!(
            b.to_instance(Layer::MAIN).unwrap().unwrap().pos,
            [10.0, 16.0]
        );
        assert_eq!(
            b.to_instance(Layer::MAIN).unwrap().unwrap().size,
            [16.0, 8.0]
        );
    }

    /// 锚点即旋转轴心：矩形绕 `pos` 转，而不是绕自己的中心转。
    #[test]
    fn rotation_pivots_on_anchor() {
        // 未旋转时"锚点 → 中心"的偏移 = (0.5-anchor)*尺寸 = (10, -5)。
        let mut s = solid(50.0, 50.0, 20.0, 10.0);
        s.anchor = Vec2::new(0.0, 1.0);
        let rest = s.to_instance(Layer::MAIN).unwrap().unwrap();
        assert_eq!(rest.pos, [60.0, 45.0]);
        // 转 90°：该偏移跟着转 (10,-5) → (5,10)，锚点本身仍钉在 pos。
        s.rotation = std::f32::consts::FRAC_PI_2;
        let i = s.to_instance(Layer::MAIN).unwrap().unwrap();
        assert!(
            (i.pos[0] - 55.0).abs() < 1e-4 && (i.pos[1] - 60.0).abs() < 1e-4,
            "{i:?}"
        );
        let d = |p: [f32; 2]| ((p[0] - 50.0).powi(2) + (p[1] - 50.0).powi(2)).sqrt();
        assert!(
            (d(rest.pos) - d(i.pos)).abs() < 1e-4,
            "旋转不得改变锚点到中心的距离"
        );
        assert_eq!(i.rot, s.rotation);
    }

    #[test]
    fn flip_signs_size_and_keeps_uv_ascending() {
        let mut s = solid(0.0, 0.0, 8.0, 4.0);
        s.flip_x = true;
        s.flip_y = true;
        let i = s.to_instance(Layer::MAIN).unwrap().unwrap();
        assert_eq!(i.size, [-8.0, -4.0]);
        // 翻转只动几何符号，uv 仍按 rect 顺序（着色器靠 mirror 采样反向）。
        assert_eq!(i.uv0, [0.0, 0.0]);
        assert_eq!(i.uv1, [1.0, 1.0]);
        // 锚点偏移用绝对尺寸，翻转不改变对齐。
        let mut a = solid(10.0, 10.0, 8.0, 4.0);
        a.anchor = Vec2::new(0.5, 1.0);
        a.flip_x = true;
        let mut b = a.clone();
        b.flip_x = false;
        assert_eq!(
            a.to_instance(Layer::MAIN).unwrap().unwrap().pos,
            b.to_instance(Layer::MAIN).unwrap().unwrap().pos
        );
    }

    #[test]
    fn uv_comes_from_src_inside_the_page_rect() {
        let b = bitmap_at(32, 16);
        let [px, py, bw, _bh] = b.rect();
        let (pw, ph) = b.page_size();
        let mut s = Sprite::new(b.clone());
        s.src = Rect::new(4.0, 2.0, 8.0, 4.0);
        let i = s.to_instance(Layer::MAIN).unwrap().unwrap();
        let expect = |x: u32, y: u32| [x as f32 / pw as f32, y as f32 / ph as f32];
        assert_eq!(i.uv0, expect(px + 4, py + 2));
        assert_eq!(i.uv1, expect(px + 12, py + 6));
        assert_eq!(bw, 32);
        // 子矩形决定目的尺寸：1x 时就是 src 的宽高。
        assert_eq!(i.size, [8.0, 4.0]);
    }

    #[test]
    fn src_out_of_bitmap_is_clipped_not_stretched() {
        let b = bitmap_at(32, 16);
        let mut s = Sprite::new(b.clone());
        // 右一半越界：只剩 16px 宽，目的尺寸跟着缩到 16，而不是把 32 压回 32。
        s.src = Rect::new(16.0, 0.0, 32.0, 16.0);
        let i = s.to_instance(Layer::MAIN).unwrap().unwrap();
        assert_eq!(i.size, [16.0, 16.0]);
        // 完全在外 → 不可见。
        s.src = Rect::new(40.0, 0.0, 8.0, 8.0);
        assert!(s.to_instance(Layer::MAIN).unwrap().is_none());
        // 零宽同理。
        s.src = Rect::new(2.0, 2.0, 0.0, 8.0);
        assert!(s.to_instance(Layer::MAIN).unwrap().is_none());
    }

    #[test]
    fn invisible_fully_transparent_and_zero_area_are_skipped() {
        let mut hidden = solid(0.0, 0.0, 8.0, 8.0);
        hidden.visible = false;
        assert!(hidden.to_instance(Layer::MAIN).unwrap().is_none());
        let mut clear = solid(0.0, 0.0, 8.0, 8.0);
        clear.opacity = 0.0;
        assert!(clear.to_instance(Layer::MAIN).unwrap().is_none());
        // opacity 乘在 color.a 上（rgb 不带 alpha，着色器分开处理）。
        let mut half = solid(0.0, 0.0, 8.0, 8.0);
        half.opacity = 0.5;
        let i = half.to_instance(Layer::MAIN).unwrap().unwrap();
        assert_eq!(i.color, [1.0, 1.0, 1.0, 0.5]);
        // 零尺寸（scale 归零同样算不可见）。
        let mut flat = solid(0.0, 0.0, 8.0, 8.0);
        flat.scale = Vec2::new(1.0, 0.0);
        assert!(flat.to_instance(Layer::MAIN).unwrap().is_none());
    }

    #[test]
    fn tone_offset_maps_to_instance_and_matrix_errors() {
        let mut s = solid(0.0, 0.0, 8.0, 8.0);
        s.tone = Tone::offset([0.2, -0.1, 0.05], 0.75);
        assert_eq!(
            s.to_instance(Layer::MAIN).unwrap().unwrap().tone,
            [0.2, -0.1, 0.05, 0.75]
        );
        s.tone = Tone::Matrix([0.0; 20]);
        assert_eq!(
            s.to_instance(Layer::MAIN),
            Err(CollectError::UnsupportedTone {
                sprite: SpriteRef::ANON
            })
        );
    }

    /// arena 里带上 `Matrix`：错误里要能点出是哪个句柄，且整次采集不留半成品。
    #[test]
    fn collect_reports_the_offending_handle() {
        let mut a = SpriteArena::new();
        let ok1 = a.spawn(solid(0.0, 0.0, 4.0, 4.0));
        let bad = a.spawn({
            let mut s = solid(4.0, 0.0, 4.0, 4.0);
            s.tone = Tone::Matrix([0.0; 20]);
            s
        });
        let _ = a.spawn(solid(8.0, 0.0, 4.0, 4.0));
        let mut list = BatchList::new();
        assert_eq!(
            a.collect(ViewportRef::MAIN, &mut list),
            Err(CollectError::UnsupportedTone { sprite: bad })
        );
        assert!(list.is_empty(), "出错时不得留下已采集的实例");
        a.get_mut(bad).unwrap().tone = Tone::NONE;
        a.collect(ViewportRef::MAIN, &mut list).unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(
            list.sorted_items()[0].1.pos[0],
            a.get(ok1).unwrap().pos.x + 2.0
        );
    }

    #[test]
    fn z_orders_within_a_batch_and_layer_splits_batches() {
        let mut a = SpriteArena::new();
        for (i, z) in [10, -5, 0].iter().enumerate() {
            let mut s = solid(i as f32, 0.0, 4.0, 4.0);
            s.z = *z;
            a.spawn(s);
        }
        let mut list = BatchList::new();
        a.collect(ViewportRef::MAIN, &mut list).unwrap();
        let batches = list.build(1024);
        assert_eq!(batches.len(), 1, "同层同图同 blend 该合成一批");
        let zs: Vec<i32> = list.sorted_items().iter().map(|(k, _)| k.z).collect();
        assert_eq!(zs, vec![-5, 0, 10]);
    }

    /// 两个 viewport 采同一个 arena：各取各的标签，实例的投影索引互不串台。
    #[test]
    fn two_viewports_collect_the_same_arena_independently() {
        let mut a = SpriteArena::new();
        let mut in_main = solid(1.0, 1.0, 4.0, 4.0);
        in_main.layer = Layer::MAIN;
        let mut in_hud = solid(2.0, 2.0, 4.0, 4.0);
        in_hud.layer = Layer(3);
        in_hud.blend = Blend::Add;
        a.spawn(in_main);
        a.spawn(in_hud);

        let mut main = BatchList::new();
        let mut hud = BatchList::new();
        a.collect(ViewportRef::MAIN, &mut main).unwrap();
        a.collect(ViewportRef::new(Layer(3)), &mut hud).unwrap();
        assert_eq!(main.len(), 1);
        assert_eq!(hud.len(), 1);
        let (mk, mi) = &main.sorted_items()[0];
        let (hk, hi) = &hud.sorted_items()[0];
        assert_eq!((mk.layer, mi.pos, mi.color[3]), (0, [3.0, 3.0], 1.0));
        assert_eq!((hk.layer, hi.pos), (3, [4.0, 4.0]));
        assert_eq!(hi.layer, 3.0, "投影索引必须跟着可视口走");
        assert_eq!(
            (mk.epoch, hk.epoch),
            (sisf_render::WHITE_EPOCH, sisf_render::WHITE_EPOCH)
        );
        // `all()` 忽略标签：整份集合重画进层 5（离屏位图 pass 的用法）。
        let mut both = BatchList::new();
        a.collect(ViewportRef::all(Layer(5)), &mut both).unwrap();
        assert_eq!(both.len(), 2);
        assert!(
            both.sorted_items()
                .iter()
                .all(|(k, i)| k.layer == 5 && i.layer == 5.0)
        );
    }
}
