//! 每帧上下文：物理/虚拟尺寸映射、viewport 投影栈、输入状态与生命周期状态。

use sisf_event::InputState;
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
    /// 虚拟画面占据的物理矩形（letterbox 后）。边条宽度取整，故整数倍缩放时原点落在
    /// 物理像素格上（不半像素）。
    pub viewport: Viewport,
    /// 虚拟→物理的等比系数（Fill 时为各轴分别缩放，此处存 x 轴）。
    pub scale: f32,
    pub suspended: bool,
    pub focused: bool,
    /// 命名动作的实时状态（T9）。归一化只发生在 `Runner` 一处，这里存的已经是
    /// "`ok` 按下了吗"这一层，游戏侧不需要知道设备。
    pub input: InputState,
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
            input: InputState::new(config.actions.clone()),
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
        // letterbox 原点向下取整：整数倍缩放下窗口边长为奇数时 `(pw - w)/2` 落在半像素
        // 上，而 1:1 的内容平移半像素意味着每个像素都采样在两个纹素正中（整体发糊），
        // resize 一格还会在两个相位之间跳（抖动）。左右边条差一个像素是无害的。
        let pad = |slack: f32| (slack / 2.0).max(0.0).floor();
        self.viewport = Viewport {
            x: pad(pw - w),
            y: pad(ph - h),
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

    /// 文字覆盖率的光栅超采样档位，喂给 `TextEngine::set_raster_scale`。
    ///
    /// 取自 [`Viewport`] 的那个等比系数（`FrameCtx::scale`）：`Fill` 下两轴倍率不同，
    /// 按 x 轴取档 —— 少的那一轴只会更少地用到位图细节，不会反过来发虚。
    /// 虚拟→物理是缩小（`scale ≤ 1`）时为 1 档，多光栅的像素会被扔掉。
    pub fn raster_scale(&self) -> f32 {
        sisf_text::raster_scale_for(self.scale)
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

    /// 整数倍缩放 + 奇数边长的窗口：边条取整，虚拟像素格与物理像素格必须同相。
    ///
    /// 这就是"半像素抖动"那条验收：1001×701 装 544×416 时未取整的原点是 (228.5, 142.5)，
    /// 1:1 的内容被平移半像素 ⇒ 每个像素都采样在两个纹素正中，糊且随 resize 跳相。
    #[test]
    fn integer_fit_with_odd_padding_stays_on_the_pixel_grid() {
        let cfg = AppConfig {
            scale_mode: ScaleMode::IntegerFit,
            ..Default::default()
        };
        let ctx = FrameCtx::new(&cfg, (1001, 701), 1.0);
        assert_eq!(ctx.scale, 1.0);
        assert_eq!(
            (ctx.viewport.x, ctx.viewport.y),
            (228.0, 142.0),
            "边条没取整"
        );
        let f = ctx.frame_u(0.0);
        for v in [0.0f32, 1.0, 272.0, 543.0, 544.0] {
            let c = xform(&f.proj[0], [v, v, 0.0, 1.0]);
            let (px, py) = ((c[0] + 1.0) * 1001.0 / 2.0, (1.0 - c[1]) * 701.0 / 2.0);
            assert!(
                (px - px.round()).abs() < 1e-3 && (py - py.round()).abs() < 1e-3,
                "虚拟格线 {v} 落在了亚像素上: px={px} py={py}"
            );
        }
    }

    /// 三种缩放下的点击命中：物理像素中心 → 虚拟像素中心，取整就是格子号。
    #[test]
    fn pointer_hits_the_expected_cell_after_the_letterbox() {
        let int_fit = AppConfig {
            scale_mode: ScaleMode::IntegerFit,
            ..Default::default()
        };
        let ctx = FrameCtx::new(&int_fit, (1001, 701), 1.0);
        // 第 (3,2) 号虚拟像素的中心在物理 (228+3.5, 142+2.5)。
        let (vx, vy) = ctx.map_pointer((231.5, 144.5));
        assert_eq!(
            (vx.floor() as i32, vy.floor() as i32),
            (3, 2),
            "边条取整后命中点偏了: {vx},{vy}"
        );
        // 右下角最后一个像素仍落在画面内（边条取整不许吃掉内容）。
        let (lx, ly) = ctx.map_pointer((771.5, 557.5));
        assert_eq!(
            (lx.floor() as i32, ly.floor() as i32),
            (543, 415),
            "右下角被切掉了一格: {lx},{ly}"
        );

        // Fill 拉伸铺满，无 letterbox：几何中心仍是中心。
        let fill = AppConfig {
            scale_mode: ScaleMode::Fill,
            ..Default::default()
        };
        let ctx = FrameCtx::new(&fill, (1001, 701), 1.0);
        assert_eq!((ctx.viewport.x, ctx.viewport.y), (0.0, 0.0));
        let (cx, cy) = ctx.map_pointer((500.5, 350.5));
        assert!(
            (cx - 272.0).abs() < 0.5 && (cy - 208.0).abs() < 0.5,
            "Fill 下中心不对: {cx},{cy}"
        );

        // Fit 非整数倍 + 取整边条：中心与角落都要自洽（投影与映射同源）。
        let ctx = FrameCtx::new(&AppConfig::default(), (1000, 700), 1.0);
        assert_eq!(ctx.viewport.x, 42.0, "Fit 的边条也该取整");
        let mid = f64::from(ctx.viewport.x + ctx.viewport.width / 2.0);
        let (cx, _) = ctx.map_pointer((mid, 350.0));
        assert!(
            (cx - 272.0).abs() <= 1.0,
            "Fit 下画面水平中心映射偏了: {cx}"
        );
    }

    /// 文字光栅档位跟着有效倍率走：1:1 不超采样，整数 2× 要 2 档才不糊。
    #[test]
    fn text_raster_tier_follows_the_upscale() {
        let fit = AppConfig::default();
        assert_eq!(
            FrameCtx::new(&fit, (544, 416), 1.0).raster_scale(),
            1.0,
            "1:1 映射多光栅纯属浪费页容量"
        );
        assert_eq!(FrameCtx::new(&fit, (1088, 832), 1.0).raster_scale(), 2.0);
        assert_eq!(
            FrameCtx::new(&fit, (400, 300), 1.0).raster_scale(),
            1.0,
            "缩小采样时档位回 1"
        );
        let int_fit = AppConfig {
            scale_mode: ScaleMode::IntegerFit,
            ..Default::default()
        };
        assert_eq!(
            FrameCtx::new(&int_fit, (1632, 1248), 1.0).raster_scale(),
            4.0,
            "3× 整数放大取 4 档"
        );
    }
}
