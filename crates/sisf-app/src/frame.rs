//! 每帧上下文：物理/虚拟尺寸映射与生命周期状态。
//! T3 将在此加 `push_viewport/pop_viewport`（投影栈）；T8 接固定 tick。

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
}
