//! 可视单元的数据：位图子矩形 + 变换 + 混合/tone。
//!
//! 语义对照 RPG Maker 的 `Sprite`（`src_rect/ox/oy/zoom_x/rotation/opacity/tone/
//! flip_x/flip_y/z/visible/blend_type`），字段公开、无 setter 仪式。采集与实例生成在
//! [`crate::draw`]，能力边界（哪些 tone/blend 有通路）见 `docs/sprite.md`。

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
    /// 各通道加 `rgb`（[-1,1]），再与自身亮度按 `gray`（[0,1]）混合。着色器的一条通路。
    Offset { rgb: [f32; 3], gray: f32 },
    /// RGSS 的 5×4 色彩矩阵。**本阶段没有着色器通路**：采集时对它返回
    /// [`CollectError::UnsupportedTone`](crate::draw::CollectError)，而不是静默当无效果
    /// （DoD-4：不留空实现分支）。补通路还是删变体，与 `Multiply/Screen` 一起在 T12 收口。
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

/// 混合模式即管线选择：当前只有 alpha-over 与 additive 两条管线，所以直接用渲染器的
/// [`Blend`]。`Multiply/Screen` 不在此引入无法映射的变体（T12 二选一，见 `docs/sprite.md`）。
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
}

impl Default for Sprite {
    fn default() -> Self {
        Self {
            bitmap: None,
            src: Rect::ZERO,
            anchor: Vec2::ZERO,
            pos: Vec2::ZERO,
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
