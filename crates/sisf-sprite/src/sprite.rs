//! 可视单元的数据：位图子矩形 + 变换 + 混合/tone。
//!
//! 语义对照 RPG Maker 的 `Sprite`（`src_rect/ox/oy/zoom_x/rotation/opacity/tone/
//! flip_x/flip_y/z/visible/blend_type`），字段公开、无 setter 仪式。采集与实例生成在
//! [`crate::draw`]，能力边界（哪些 tone/blend 有通路）见 `docs/sprite.md`。

use crate::anim::AnimState;
use sisf_render::{Bitmap, Blend, WHITE_EPOCH};

/// 二维量。`pos` 与 `Sprite::extent` 是虚拟像素，`anchor`/`scale` 是比值。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };
    pub const ONE: Vec2 = Vec2 { x: 1.0, y: 1.0 };
    /// 中心锚点（RPG Maker 的 `ox = oy = 0.5`）。
    pub const CENTER: Vec2 = Vec2 { x: 0.5, y: 0.5 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// 线性插值（T8 的 tick 间插值：`t = 0` 取上一 tick 的位置，`t = 1` 取当前的）。
    pub fn lerp(self, to: Vec2, t: f32) -> Self {
        Self {
            x: self.x + (to.x - self.x) * t,
            y: self.y + (to.y - self.y) * t,
        }
    }

    /// 模长（自由路线用它算剩余距离）。
    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
}

impl std::ops::Add for Vec2 {
    type Output = Vec2;
    fn add(self, rhs: Vec2) -> Vec2 {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl std::ops::Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, rhs: Vec2) -> Vec2 {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl std::ops::Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, rhs: f32) -> Vec2 {
        Self {
            x: self.x * rhs,
            y: self.y * rhs,
        }
    }
}

/// 源矩形（位图像素，左上原点；`w`/`h` 是尺寸而非右下角坐标）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const ZERO: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    };

    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// 原点处 `w×h` 的矩形（"取整图"的构造器）。
    pub const fn with_size(w: f32, h: f32) -> Self {
        Self::new(0.0, 0.0, w, h)
    }
}

/// 色彩校正。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tone {
    /// 各通道加 `rgb`（`[-1,1]`），再与自身亮度按 `gray`（`[0,1]`）混合。着色器的一条通路。
    Offset { rgb: [f32; 3], gray: f32 },
    /// RGSS 的 5×4 色彩矩阵。**没有着色器通路，采集时显式报错**：[`Tone::Matrix`] 走
    /// [`CollectError::UnsupportedTone`](crate::draw::CollectError)，而不是静默当无效果
    /// （DoD-4：不留空实现分支；静默失效的调色代码排查成本极高，见 `docs/sprite.md`）。
    /// 需要矩阵调色请在 CPU 侧改写位图像素，或只写 [`Tone::offset`]。
    Matrix([f32; 20]),
}

impl Default for Tone {
    fn default() -> Self {
        Self::NONE
    }
}

impl Tone {
    /// 无效果。
    pub const NONE: Tone = Tone::Offset {
        rgb: [0.0; 3],
        gray: 0.0,
    };

    pub const fn offset(rgb: [f32; 3], gray: f32) -> Self {
        Self::Offset { rgb, gray }
    }

    /// 实例的 `tone` 分量（rgb=偏移、a=灰度）。`Matrix` 无对应通路 → `None`。
    pub fn as_instance_vec(self) -> Option<[f32; 4]> {
        match self {
            Self::Offset { rgb, gray } => Some([rgb[0], rgb[1], rgb[2], gray]),
            Self::Matrix(_) => None,
        }
    }
}

/// 混合模式即管线选择：`Normal`（alpha-over）与 `Add` 两条管线，加上只为 `clear_rect`
/// 存在的 `Replace`。它是 [`Blend`] 的别名而不是新枚举 —— 变体必须与管线一一对应，
/// `Multiply/Screen` 因此不在引擎里出现（T12 定案不移植，理由与替代写法见 `docs/sprite.md`）。
pub type BlendMode = Blend;

/// viewport 栈索引（0..7）。一个类型同时承担两个角色：`Sprite::layer` 的**归属标签**，
/// 以及 `FrameCtx::push_viewport` 返回的**投影栈层**——它们必须是同一个数，否则
/// 精灵会被画进别人的投影里，所以不另设 `ViewportRef` 数值类型。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Layer(pub u8);

impl Layer {
    /// 主视口（虚拟分辨率整屏）。
    pub const MAIN: Layer = Layer(0);
}

/// 一个可视单元。`Default` 可用（左上锚点、不透明、可见、主层、normal 混合、零尺寸）。
#[derive(Clone, Debug)]
pub struct Sprite {
    /// 纹理来源；`None` = 渲染器内置的 1×1 白图（纯色矩形/占位）。
    pub bitmap: Option<Bitmap>,
    /// 取位图的哪一块（位图像素，越界会被裁进位图矩形）。无位图时只有 `w/h` 有意义，
    /// 它直接就是目的尺寸（虚拟像素）。
    pub src: Rect,
    /// 锚点：`(0,0)`=左上、`(0.5,0.5)`=中心、`(0.5,1)`=底边中点。
    /// 既是 `pos` 的对齐点，也是 `rotation` 的轴心。
    pub anchor: Vec2,
    /// 锚点在虚拟空间里的位置（当前 viewport 坐标系，左上原点、y 向下）。
    pub pos: Vec2,
    /// 上一个 tick 的 `pos`：固定步长 + `draw(alpha)` 的插值起点（T8）。
    ///
    /// 采集时画的是"`[prev_pos, pos]` 按 `alpha` 取点"，所以逐 tick 位移要走
    /// [`Sprite::set_pos`]（它把旧的 `pos` 挪进本字段）。不想被插值的跳变（传送、
    /// 读档、路线起点）一律走 [`Sprite::teleport`]：一次改两个值。直接写 `pos`
    /// 是最坏的那种写法 —— 本字段会停在原地，精灵从出生点一路"滑"过来。
    ///
    /// **不变量：本字段是"上一拍的位置"，所以没动的拍也得推进它** —— 每拍开头对全场精灵调
    /// 一次 [`Sprite::begin_tick`]（[`crate::arena::SpriteArena::begin_tick`]），否则停拍会把最后一跳反复
    /// 重播（角色原地来回弹）。
    pub prev_pos: Vec2,
    /// 逐轴缩放（可为负，与 `flip_*` 叠加）。
    pub scale: Vec2,
    /// 绕锚点旋转（弧度）。
    pub rotation: f32,
    /// 不透明度 `[0,1]`，乘在预乘色上；`0.0` 视作不可见，整只跳过。
    pub opacity: f32,
    pub tone: Tone,
    pub flip_x: bool,
    pub flip_y: bool,
    /// 同一 viewport 内的绘制次序（小者先画）。跨图集页/跨 blend 的批次顺序由
    /// `(layer, epoch, blend)` 先决定，`z` 只在同批次内排序。
    pub z: i32,
    pub visible: bool,
    pub blend: BlendMode,
    /// 归属标签：默认只被同一层的采集取走（[`Layer::MAIN`]）。
    pub layer: Layer,
    /// 动画状态（`Some` 时 [`Rect`] `src` 由它持有，见 [`crate::anim`]）。
    ///
    /// 挂/摘走 [`Sprite::set_animation`]，它顺手把当前帧落进 `src`。
    pub anim: Option<AnimState>,
}

impl Default for Sprite {
    fn default() -> Self {
        Self {
            bitmap: None,
            src: Rect::ZERO,
            anchor: Vec2::ZERO,
            pos: Vec2::ZERO,
            // 与 `pos` 同一个值：新精灵没有"上一 tick"，插值不该把它拖回原点。
            prev_pos: Vec2::ZERO,
            scale: Vec2::ONE,
            rotation: 0.0,
            opacity: 1.0,
            tone: Tone::NONE,
            flip_x: false,
            flip_y: false,
            z: 0,
            visible: true,
            blend: BlendMode::Normal,
            layer: Layer::MAIN,
            anim: None,
        }
    }
}

impl Sprite {
    /// 画整张位图：`src` 取位图全尺寸。
    pub fn new(bitmap: Bitmap) -> Self {
        let (w, h) = bitmap.size();
        Self {
            bitmap: Some(bitmap),
            src: Rect::with_size(w as f32, h as f32),
            ..Default::default()
        }
    }

    /// 纯色矩形（无位图，走白图占位）；尺寸即 `src` 的宽高（虚拟像素）。
    pub fn solid(size: Vec2) -> Self {
        Self {
            src: Rect::with_size(size.x, size.y),
            ..Default::default()
        }
    }

    /// 含 `scale` 的目的尺寸（绝对值；翻转由实例 `size` 的符号表达，不进这里）。
    pub fn extent(&self) -> Vec2 {
        Vec2 {
            x: (self.src.w * self.scale.x).abs(),
            y: (self.src.h * self.scale.y).abs(),
        }
    }

    /// 批次键里的纹理代号：所在图集页的 epoch，无位图时是内置白图。
    pub fn epoch(&self) -> u64 {
        self.bitmap.as_ref().map_or(WHITE_EPOCH, Bitmap::epoch)
    }

    /// 逐 tick 移动走这里：旧的 `pos` 挪进 [`Sprite::prev_pos`]，`draw(alpha)` 因此在
    /// 两个 tick 之间插值（固定步长的全部意义）。
    pub fn set_pos(&mut self, pos: Vec2) {
        self.prev_pos = self.pos;
        self.pos = pos;
    }

    /// 不该被插值的跳变（传送、读档、路线起点）：一次改两个值。
    pub fn teleport(&mut self, pos: Vec2) {
        self.prev_pos = pos;
        self.pos = pos;
    }

    /// 每拍开头收拢一次：`prev_pos = pos`。
    ///
    /// [`Sprite::prev_pos`] 的含义是"上一拍的位置"，但只有 [`Sprite::set_pos`] 会推进它 ——
    /// 于是"这一拍没动"的精灵（走一格后的停拍、`MoveRoute` 的 `Wait`、撞墙被作废）会让
    /// `prev_pos` 永远停在最后一跳上，被 `alpha` 的 `0→1` 一遍遍重播：画面退回去一步再走
    /// 回来。收拢之后，没动就没有可播的位移。每拍调一次，且必须在任何移动之前。
    pub fn begin_tick(&mut self) {
        self.prev_pos = self.pos;
    }

    /// `prev_pos`/`pos` 按 `alpha` 取点（`collect` 的落点）。
    pub fn interpolated(&self, alpha: f32) -> Vec2 {
        self.prev_pos.lerp(self.pos, alpha)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // T12：`cargo xtask wasm-test` 在 wasm 上跑这一组（见 docs/platforms.md）。
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    #[test]
    fn default_is_opaque_visible_main_layer() {
        let s = Sprite::default();
        assert_eq!(s.opacity, 1.0);
        assert!(s.visible);
        assert_eq!(s.layer, Layer::MAIN);
        assert_eq!(s.blend, BlendMode::Normal);
        assert_eq!(s.scale, Vec2::ONE);
        assert_eq!(s.anchor, Vec2::ZERO);
        assert_eq!(s.tone, Tone::NONE);
        assert_eq!(s.epoch(), WHITE_EPOCH);
        assert_eq!(s.extent(), Vec2::ZERO);
    }

    #[test]
    fn fields_are_settable_in_place() {
        let mut s = Sprite {
            pos: Vec2::new(10.0, 20.0),
            src: Rect::new(4.0, 4.0, 32.0, 16.0),
            anchor: Vec2::CENTER,
            scale: Vec2::new(2.0, -1.0),
            tone: Tone::offset([0.1, -0.2, 0.0], 0.5),
            z: 7,
            ..Default::default()
        };
        s.flip_x = true;
        s.rotation = 1.0;
        assert_eq!(s.extent(), Vec2::new(64.0, 16.0));
        assert_eq!(s.tone.as_instance_vec(), Some([0.1, -0.2, 0.0, 0.5]));
    }

    #[test]
    fn matrix_tone_has_no_pipeline() {
        assert_eq!(Tone::Matrix([0.0; 20]).as_instance_vec(), None);
    }

    /// 固定步长的插值对：`set_pos` 留痕、`teleport` 不留痕。
    #[test]
    fn set_pos_tracks_the_previous_tick_and_teleport_does_not() {
        let mut s = Sprite::default();
        s.teleport(Vec2::new(10.0, 4.0));
        assert_eq!(s.prev_pos, s.pos);
        s.set_pos(Vec2::new(12.0, 4.0));
        assert_eq!(
            (s.prev_pos, s.pos),
            (Vec2::new(10.0, 4.0), Vec2::new(12.0, 4.0))
        );
        // alpha = 0 是上一 tick 的样子，1 是当前的，0.5 在正中。
        assert_eq!(s.interpolated(0.0), Vec2::new(10.0, 4.0));
        assert_eq!(s.interpolated(1.0), Vec2::new(12.0, 4.0));
        assert_eq!(s.interpolated(0.5), Vec2::new(11.0, 4.0));
        // 传送之后中途取点也还在原地（两值相同 ⇒ 插值退化成常量）。
        s.teleport(Vec2::new(100.0, 100.0));
        assert_eq!(s.interpolated(0.0), s.interpolated(0.5));
    }

    /// 停拍不变量：`begin_tick` 之后的"没动的那一拍"没有任何可播位移 —— 否则
    /// `interpolated(alpha)` 会把上一跳来回重播（角色原地来回弹）。
    #[test]
    fn begin_tick_erases_a_hop_that_the_new_tick_does_not_repeat() {
        let mut s = Sprite::default();
        s.teleport(Vec2::new(10.0, 4.0));
        s.set_pos(Vec2::new(12.0, 4.0));
        // 动的那一拍：中途取点确实落在两点之间。
        assert_eq!(s.interpolated(0.0), Vec2::new(10.0, 4.0));
        assert_eq!(s.interpolated(0.5), Vec2::new(11.0, 4.0));
        // 下一拍开头收拢，且这一拍没有再动 ⇒ alpha 走遍 0→1 也只画在一个点上。
        s.begin_tick();
        for a in [0.0, 0.18, 0.5, 0.91, 1.0] {
            assert_eq!(
                s.interpolated(a),
                Vec2::new(12.0, 4.0),
                "alpha {a} 重播了最后一跳"
            );
        }
        // 收拢不能吃掉本拍的位移：begin_tick 之后再 set_pos 照旧留痕。
        s.set_pos(Vec2::new(14.0, 4.0));
        assert_eq!(s.prev_pos, Vec2::new(12.0, 4.0));
        assert_eq!(s.interpolated(0.5), Vec2::new(13.0, 4.0));
    }
}
