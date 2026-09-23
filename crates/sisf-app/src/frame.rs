//! 每帧上下文：物理/虚拟尺寸映射、viewport 投影栈与生命周期状态。

use sisf_render::{PROJ_STACK_DEPTH, mat4_mul, ortho_viewport};

use crate::config::{AppConfig, ScaleMode};

/// 虚拟画面在物理窗口中的矩形（物理像素，左上原点）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Viewport {
    pub const FULL: Viewport = Viewport {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    /// 以父层坐标系的原点偏移出 `w×h` 的子 viewport。
    pub const fn at(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FrameCtx {
    /// 窗口物理像素尺寸（inner_size，已含 scale_factor）。
    pub physical_size: (u32, u32),
    pub virtual_size: (u32, u32),
    pub scale_factor: f64,
    /// 虚拟画面占据的物理矩形（letterbox 后）。
    pub viewport: Viewport,
    /// 虚拟→物理的等比系数（Fill 时为各轴分别缩放，此处存 x 轴）。
    pub scale: f32,
    pub suspended: bool,
    pub focused: bool,
    /// viewport 栈：每层的子矩形（父层坐标系）与对应投影。
    stack: [Layer; PROJ_STACK_DEPTH],
    /// 当前栈深（1..=8）。
    depth: usize,
}

/// 投影栈的一层。
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layer {
    /// 该层在**父层**坐标系里的矩形；layer 0 为窗口物理像素里的 letterbox 区。
    rect: Viewport,
    /// 该层内容坐标（像素，左上原点）→ NDC。
    proj: [[f32; 4]; 4],
}

impl Layer {
    const ZERO: Layer = Layer {
        rect: Viewport::FULL,
        proj: [
            [0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 0.0],
        ],
    };
}

impl FrameCtx {
    pub fn new(config: &AppConfig, physical_size: (u32, u32), scale_factor: f64) -> Self {
        let mut ctx = Self {
            physical_size,
            virtual_size: config.virtual_size,
            scale_factor,
            viewport: Viewport::FULL,
            scale: 1.0,
            suspended: false,
            focused: true,
            stack: [Layer::ZERO; PROJ_STACK_DEPTH],
            depth: 1,
        };
        ctx.recompute(config.scale_mode);
        ctx
    }

    /// `Resized` / `scale_factor` 变化后重算投影与输入映射。
    pub fn set_physical_size(&mut self, size: (u32, u32), scale_mode: ScaleMode) {
        self.physical_size = size;
        self.recompute(scale_mode);
    }

    fn recompute(&mut self, scale_mode: ScaleMode) {
        let (pw, ph) = (self.physical_size.0 as f32, self.physical_size.1 as f32);
        let (vw, vh) = (
            self.virtual_size.0.max(1) as f32,
            self.virtual_size.1.max(1) as f32,
        );
        let (sx, sy) = (pw / vw, ph / vh);
        let (w, h, scale) = match scale_mode {
            ScaleMode::Fill => (pw, ph, sx),
            ScaleMode::Fit => {
                let s = sx.min(sy);
                (vw * s, vh * s, s)
            }
            ScaleMode::IntegerFit => {
                let s = sx.min(sy).floor().max(1.0);
                (vw * s, vh * s, s)
            }
        };
        self.scale = scale;
        self.viewport = Viewport {
            x: ((pw - w) / 2.0).max(0.0),
            y: ((ph - h) / 2.0).max(0.0),
            width: w,
            height: h,
        };
        // layer 0 把虚拟像素映射进 letterbox 矩形（Fill 下两轴系数不同）。
        self.stack[0].rect = self.viewport;
        self.stack[0].proj =
            ortho_viewport(pw, ph, [w / vw, h / vh], [self.viewport.x, self.viewport.y]);
        // 深层 rect 是相对父层的，尺寸不变，只需按新的基准重推投影。
        for i in 1..self.depth {
            let layer = self.derive_layer(i);
            self.stack[i] = layer;
        }
    }

    fn derive_layer(&self, i: usize) -> Layer {
        let parent = self.stack[i - 1];
        let r = self.stack[i].rect;
        // 子层只平移：1 子层像素 = 1 父层像素（缩放由调用方在 rect 尺寸里表达）。
        let offset = mat4_mul(
            &parent.proj,
            &[
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [r.x, r.y, 0.0, 1.0],
            ],
        );
        Layer {
            rect: r,
            proj: offset,
        }
    }

    /// 进入一个子 viewport：`rect` 用当前层坐标系，返回新层索引（即 `BatchKey::layer`）。
    ///
    /// 栈深上限 `PROJ_STACK_DEPTH`（8），越界 `debug_assert`；release 下**复用栈顶层**，
    /// 保证不会 panic 也不会写到别人的投影。
    pub fn push_viewport(&mut self, rect: Viewport) -> u8 {
        debug_assert!(self.depth < PROJ_STACK_DEPTH, "viewport 栈深度超过 8");
        let i = self.depth.min(PROJ_STACK_DEPTH - 1);
        self.depth = (i + 1).min(PROJ_STACK_DEPTH);
        self.stack[i].rect = rect;
        let derived = self.derive_layer(i);
        self.stack[i] = derived;
        i as u8
    }

    /// 退回上一层。栈空时 `debug_assert`（release 下停在 layer 0）。
    pub fn pop_viewport(&mut self) {
        debug_assert!(self.depth > 1, "viewport 栈已空，pop_viewport 多余");
        if self.depth > 1 {
            self.depth -= 1;
        }
    }

    /// 当前层索引（提交实例时填 `BatchKey::layer`）。
    pub fn layer(&self) -> u8 {
        (self.depth - 1) as u8
    }

    /// 当前 viewport 矩形（父层坐标系）。
    pub fn current_viewport(&self) -> Viewport {
        self.stack[self.depth - 1].rect
    }

    /// 当前栈深（1..=8）。
    pub fn viewport_depth(&self) -> usize {
        self.depth
    }

    /// 打出投影栈给渲染器。未使用的层复用 layer 0，避免脏矩阵被误读。
    pub fn frame_u(&self, time: f32) -> sisf_render::FrameU {
        let mut proj = [self.stack[0].proj; PROJ_STACK_DEPTH];
        for (dst, layer) in proj.iter_mut().zip(&self.stack[..self.depth]) {
            *dst = layer.proj;
        }
        sisf_render::FrameU::new(&proj, time)
    }

    /// 物理像素坐标（窗口左上原点）→ 虚拟坐标。指针/触摸命中测试统一走这里。
    pub fn map_pointer(&self, physical: (f64, f64)) -> (f32, f32) {
        let vp = self.viewport;
        let vx = (physical.0 as f32 - vp.x) / if vp.width > 0.0 { vp.width } else { 1.0 }
            * self.virtual_size.0 as f32;
        let vy = (physical.1 as f32 - vp.y) / if vp.height > 0.0 { vp.height } else { 1.0 }
            * self.virtual_size.1 as f32;
        (vx, vy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_letterboxes_and_maps_pointer() {
        let cfg = AppConfig::default(); // 544x416, Fit
        let ctx = FrameCtx::new(&cfg, (1088, 832), 1.0);
        assert_eq!(ctx.scale, 2.0);
        assert_eq!(
            ctx.viewport,
            Viewport {
                x: 0.0,
                y: 0.0,
                width: 1088.0,
                height: 832.0
            }
        );
        assert_eq!(ctx.map_pointer((544.0, 416.0)), (272.0, 208.0));
    }

    #[test]
    fn integer_fit_pads_edges() {
        let cfg = AppConfig {
            scale_mode: ScaleMode::IntegerFit,
            ..Default::default()
        };
        let ctx = FrameCtx::new(&cfg, (1000, 700), 1.0);
        assert_eq!(ctx.scale, 1.0); // 1000/544<2 故取 1
        assert_eq!(ctx.viewport.width, 544.0);
        assert_eq!(ctx.viewport.x, (1000.0 - 544.0) / 2.0);
    }

    #[test]
    fn default_config_is_rpgmaker_size() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.virtual_size, (544, 416));
        assert_eq!(cfg.tick_hz, 60.0);
    }

    /// 列主序矩阵乘列向量。
    fn xform(m: &[[f32; 4]; 4], v: [f32; 4]) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        for (i, o) in out.iter_mut().enumerate() {
            *o = (0..4).map(|j| m[j][i] * v[j]).sum();
        }
        out
    }

    #[test]
    fn layer0_maps_virtual_center_to_ndc_origin() {
        // 544x416 虚拟 → 1088x832 物理（Fit，2x，无 letterbox）。
        let cfg = AppConfig::default();
        let ctx = FrameCtx::new(&cfg, (1088, 832), 1.0);
        let f = ctx.frame_u(0.0);
        let c = xform(&f.proj[0], [272.0, 208.0, 0.0, 1.0]);
        assert!(
            c[0].abs() < 1e-5 && c[1].abs() < 1e-5,
            "中心应落在 NDC 原点: {c:?}"
        );
        let tl = xform(&f.proj[0], [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(tl, [-1.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn eight_viewport_levels_nest_and_pop() {
        let cfg = AppConfig::default();
        let mut ctx = FrameCtx::new(&cfg, (1088, 832), 1.0);
        assert_eq!(ctx.layer(), 0);
        let mut layers = Vec::new();
        for i in 0..(PROJ_STACK_DEPTH - 1) {
            layers.push(ctx.push_viewport(Viewport::at(8.0, 8.0, 100.0, 100.0)));
            assert_eq!(layers.last(), Some(&(i as u8 + 1)));
        }
        assert_eq!(ctx.viewport_depth(), PROJ_STACK_DEPTH);
        assert_eq!(ctx.layer(), (PROJ_STACK_DEPTH - 1) as u8);
        // 每层在父层里平移 8px：7 层嵌套后顶层原点 == layer 0 的 (56,56)。
        let f = ctx.frame_u(0.0);
        let base = xform(&f.proj[0], [56.0, 56.0, 0.0, 1.0]);
        let top = xform(&f.proj[PROJ_STACK_DEPTH - 1], [0.0, 0.0, 0.0, 1.0]);
        for axis in 0..4 {
            assert!(
                (base[axis] - top[axis]).abs() < 1e-5,
                "第 {axis} 分量不符: {base:?} vs {top:?}"
            );
        }
        for _ in 0..(PROJ_STACK_DEPTH - 1) {
            ctx.pop_viewport();
        }
        assert_eq!(ctx.layer(), 0);
        assert_eq!(ctx.current_viewport(), ctx.viewport);
    }

    #[test]
    fn unused_levels_repeat_layer0() {
        let cfg = AppConfig::default();
        let mut ctx = FrameCtx::new(&cfg, (1088, 832), 1.0);
        ctx.push_viewport(Viewport::at(4.0, 4.0, 10.0, 10.0));
        let f = ctx.frame_u(2.5);
        assert_eq!(f.time, 2.5);
        // 栈深 2，第 2 层以上应复用 layer 0 而不是脏数据。
        assert_eq!(f.proj[2], f.proj[0]);
        assert_eq!(f.proj[7], f.proj[0]);
        assert_ne!(f.proj[1], f.proj[0]);
    }
}
