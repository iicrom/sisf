//! 字形覆盖率图集：`CacheKey → R8Unorm 子矩形`，etagere 装箱，页由本模块自管。
//!
//! 为什么不复用 T4 的 `Atlas`：那一套的页是 `Rgba8UnormSrgb`，而覆盖率是单通道量。
//! 走 sRGB 页要么把覆盖率塞进 alpha（着色器取不到 `.r`，且 sRGB 解码只作用于 rgb
//! 三通道，语义上没错但白扔 3/4 显存），要么塞进 rgb 再乘 tint（解码非线性会把
//! 0.5 覆盖弯成 0.216，边缘直接糊掉）。自管 `R8Unorm` 页 = 逐字对应、无换算。
//!
//! 与 sprite 图集的三点不同，都是刻意的：
//! - **epoch 自带标记位** [`MASK_EPOCH`]（`1<<61`，与 `LARGE_EPOCH`/`TARGET_EPOCH`
//!   各占一位，低 61 位是页计数）：字形页不进 `AtlasLayout`，但必须与它的代号互不
//!   相撞，否则两条通路会抢同一个 bind group。
//! - **gutter 填 0**（不是 sprite 那边的"边缘复制"）：文字边缘外本来就该是透明，
//!   线性采样越界读到 0 覆盖才是对的。
//! - **不驱逐**：一个 (字体, 字号, 字形) 一旦落位就活到进程结束。游戏字号只有几档，
//!   页满到 [`MAX_PAGES`] 就跳过该字形并告警（宁缺字也不画错）。
//!
//! 光栅倍率由 [`FontAtlas::set_raster_scale`] 决定（见该方法的文档）：默认 1 = 每个
//! 逻辑像素一份覆盖率；>1 时同一个字形按 `字号 × 倍率` 另落一份更细的位图，两套条目
//! 在 `glyphs` 里天然共存（合成的键里 `font_size_bits` 不同）。
//!
//! 装箱与光栅全在 CPU 侧（`Vec<u8>` 影子页），GPU 只在 [`FontAtlas::sync`] 时碰一次：
//! 所以 `glyph()` 三端（含 wasm）都能跑单测，而"页脏了才整页 `write_texture`"把上传
//! 摊薄成每帧至多一次。页边长取 1024 = 4×256，正好满足 `COPY_BYTES_PER_ROW_ALIGNMENT`，
//! 不必再垫 staging 缓冲。

use std::collections::HashMap;

use cosmic_text::{CacheKey, FontSystem, SubpixelBin, SwashCache, SwashContent};
use etagere::{BucketedAtlasAllocator, size2};
use sisf_render::Renderer;

/// 覆盖率页的 epoch 标记位（见模块头的"epoch 自带标记位"）。
pub const MASK_EPOCH: u64 = 1 << 61;

/// 覆盖率页边长（像素）。必须是 256 的倍数：整页 `write_texture` 的 `bytes_per_row`。
pub(crate) const PAGE_EDGE: u32 = 1024;

/// 覆盖率页上限（每页 CPU+GPU 各 1MB）。
///
/// 这是超采样档位的实际预算闸门：`raster_scale = 2` 让每个字形多吃 4 倍面积，
/// `= 4` 吃 16 倍。档位由 [`raster_scale_for`] 从放大倍率取，页满时按既有约定
/// 跳过新字形并告警（宁缺字不画错），所以这里宁小勿大——提高它前先确认字号档位。
const MAX_PAGES: usize = 4;

/// 字形四周的 0 覆盖 gutter 宽度（线性采样不外渗的保险）。
const GUTTER: u32 = 1;

/// 光栅超采样的最高档（2 的幂）。再高一级面积就是 4 倍，页先撑不住。
pub const MAX_RASTER_SCALE: f32 = 4.0;

/// 把"虚拟像素 → 物理像素"的有效放大倍率量化成光栅超采样档位。
///
/// 取 **向上取到的 2 的幂**（1/2/4，上限 [`MAX_RASTER_SCALE`]）：`Fit` 下 1.44× 放大
/// 会把 1 像素宽的覆盖率位图摊成 1.44 像素的渐变（发虚），而 2 档的位图在同一投影下
/// 是 1.39× 缩小采样 —— 边缘重新拿到真实细节。整数倍（`IntegerFit` 的 1×/2×）正好
/// 落在档位上，所以像素风那条通路也不吃亏。
///
/// `scale ≤ 1`（窗口比虚拟分辨率还小）恒取 1：那时是缩小采样，多光栅的像素会被扔掉，
/// 只白吃页容量。
pub fn raster_scale_for(scale: f32) -> f32 {
    if scale <= 1.0 {
        1.0
    } else if scale <= 2.0 {
        2.0
    } else {
        MAX_RASTER_SCALE
    }
}

/// 一个字形的落位：够填一个 `Instance` 的 uv/size，外加相对 pen 的偏移。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphRect {
    /// 所属覆盖率页（进 `BatchKey::epoch`）。
    pub epoch: u64,
    /// 归一化 uv 左下/右上（页尺寸是 `PAGE_EDGE`，不是字形尺寸）。
    pub uv0: [f32; 2],
    pub uv1: [f32; 2],
    /// 覆盖率位图尺寸（**档内**像素）。
    pub size: [u32; 2],
    /// 相对 pen 位置（`PhysicalGlyph.x/y`）的**左上角**偏移，y 已按"屏幕向下"翻好，
    /// 单位是**逻辑像素**（= 位图像素 / `scale`）。
    pub offset: [f32; 2],
    /// 光栅超采样档位（[`FontAtlas::raster_scale`] 在该字形落位时的值）。
    ///
    /// 画 quad 前要用它把 `size` 折回逻辑像素：`w = size[0] as f32 / scale`。
    /// 记在条目里而不是让绘制方去问图集，是为了让"换档"这件事不可能被某个调用点漏掉
    /// ——图集里同时住着 1 档和 2 档的同一个字形。
    pub scale: f32,
}

/// 逻辑键 → raster 键，顺带取出被归零的 bin 余量（见 [`FontAtlas::set_raster_scale`]）。
///
/// 只动 `font_size_bits` 与两个 bin，所以 `layout` 完全不用知道档位的存在：pen 位置
/// 仍是逻辑像素整数，字形形状按 `字号 × 档位` 光栅。1 档原样返回，行为与加这个
/// 参数之前逐字相同。
fn synth_key(key: CacheKey, scale: f32) -> ([f32; 2], CacheKey) {
    if scale <= 1.0 {
        return ([0.0; 2], key);
    }
    let residue = [key.x_bin.as_float(), key.y_bin.as_float()];
    let synth = CacheKey {
        font_size_bits: (f32::from_bits(key.font_size_bits) * scale).to_bits(),
        x_bin: SubpixelBin::Zero,
        y_bin: SubpixelBin::Zero,
        ..key
    };
    (residue, synth)
}

/// 一页覆盖率：CPU 影子 + 惰性建的 GPU 纹理。
struct Page {
    epoch: u64,
    cpu: Vec<u8>,
    alloc: BucketedAtlasAllocator,
    texture: Option<wgpu::Texture>,
    /// 本模块自己的视图拷贝；注册给渲染器的是它的另一份 clone（`sync` 里换 epoch）。
    view: Option<wgpu::TextureView>,
    /// 有字形新落位 ⇒ 下次 `sync` 整页重传。
    dirty: bool,
}

impl Page {
    fn new(epoch: u64) -> Self {
        Self {
            epoch,
            cpu: vec![0u8; (PAGE_EDGE * PAGE_EDGE) as usize],
            alloc: BucketedAtlasAllocator::new(size2(PAGE_EDGE as i32, PAGE_EDGE as i32)),
            texture: None,
            view: None,
            dirty: true,
        }
    }

    fn upload(&self, queue: &wgpu::Queue, texture: &wgpu::Texture) {
        // 整页一次：页边长是 256 的倍数，所以不需要 staging 行对齐。
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.cpu,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(PAGE_EDGE), // R8Unorm = 1 字节/像素
                rows_per_image: Some(PAGE_EDGE),
            },
            wgpu::Extent3d {
                width: PAGE_EDGE,
                height: PAGE_EDGE,
                depth_or_array_layers: 1,
            },
        );
    }
}

/// `CacheKey → 覆盖率子矩形` 的常驻图集。
pub struct FontAtlas {
    /// 只为复用它的 `ScaleContext`；结果一律取"不缓存"版本，缓存归本模块的 `glyphs`。
    swash: SwashCache,
    pages: Vec<Page>,
    /// `None` = 这个字形确认没有面积（空格）或不支持，负缓存免得每帧重试。
    glyphs: HashMap<CacheKey, Option<GlyphRect>>,
    next_page: u64,
    warned_color: bool,
    warned_full: bool,
    /// 落位新字形时使用的光栅超采样档位（[`Self::set_raster_scale`]）。
    raster_scale: f32,
}

impl Default for FontAtlas {
    fn default() -> Self {
        Self::new()
    }
}

impl FontAtlas {
    pub fn new() -> Self {
        Self {
            swash: SwashCache::new(),
            pages: Vec::new(),
            glyphs: HashMap::new(),
            next_page: 0,
            warned_color: false,
            warned_full: false,
            raster_scale: 1.0,
        }
    }

    /// 当前光栅超采样档位。
    pub fn raster_scale(&self) -> f32 {
        self.raster_scale
    }

    /// 设定**之后落位**的字形的超采样档位（已缓存的条目原样保留）。
    ///
    /// 只在窗口尺寸变化时调，不要每帧调：`raster_scale_for` 是向上取整的档位，
    /// 拖窗口时倍率连续变化 ⇒ 每跨一档就多一套字形进页，档位不变就不重复装箱。
    /// 已经画出去的 quad 里带着自己的 `scale`，所以换档不需要重排或失效任何东西。
    pub fn set_raster_scale(&mut self, scale: f32) {
        // NaN 单独回 1 档而不是 `clamp`（它把 NaN 原样返回）：档位是字号乘数，
        // 带 NaN 去装箱等于毁掉整页覆盖率。±∞ 则让 `clamp` 夹到端点档，符合"往大了取"。
        self.raster_scale = if scale.is_nan() {
            1.0
        } else {
            scale.clamp(1.0, MAX_RASTER_SCALE)
        };
    }

    /// 已用的覆盖率页数。
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// 缓存条目数（含负缓存）。
    pub fn cached(&self) -> usize {
        self.glyphs.len()
    }

    /// 第 `i` 页的 epoch（`sync` 之后即已注册进渲染器）。
    pub fn page_epoch(&self, i: usize) -> Option<u64> {
        self.pages.get(i).map(|p| p.epoch)
    }

    /// 取一个字形的覆盖率矩形，必要时**只在 CPU 侧**光栅并装箱。
    ///
    /// `key` 是 `layout` 给的**逻辑**键（字号 = 虚拟像素），档位由
    /// [`Self::set_raster_scale`] 决定：查缓存前先合成成 raster 键，所以同一档只在
    /// 第一次光栅，换档则各存一份。
    ///
    /// 返回 `None` 表示这个字形没有可画面积（空格、零尺寸）或本页不支持其内容格式；
    /// 结论会进负缓存，所以每帧重复排版不会重复光栅。
    pub fn glyph(&mut self, fonts: &mut FontSystem, key: CacheKey) -> Option<GlyphRect> {
        let scale = self.raster_scale;
        let (residue, raster) = synth_key(key, scale);
        if let Some(hit) = self.glyphs.get(&raster) {
            return *hit;
        }
        let placed = self.rasterize(fonts, raster, scale, residue);
        self.glyphs.insert(raster, placed);
        placed
    }

    /// 建页纹理、登记 epoch、上传脏页。**每帧在 `end_frame` 之前调一次**。
    ///
    /// 放在这里而不是 `glyph()` 里：装箱要能在没有设备的单测里跑（三端），而且
    /// "这一帧新增了几页"只有在这里才有意义。
    pub fn sync(&mut self, renderer: &mut Renderer) {
        // `wgpu::Device`/`Queue` 是引用计数句柄，克隆出来才能与 `&mut renderer` 共存。
        let device = renderer.device().clone();
        let queue = renderer.queue().clone();
        for page in &mut self.pages {
            if page.texture.is_none() {
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(&format!("sisf-glyphs-{}", page.epoch)),
                    size: wgpu::Extent3d {
                        width: PAGE_EDGE,
                        height: PAGE_EDGE,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::R8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let view = texture.create_view(&Default::default());
                renderer.register_epoch(page.epoch, view.clone());
                page.view = Some(view);
                page.texture = Some(texture);
                page.dirty = true;
            }
            if page.dirty {
                if let Some(texture) = page.texture.as_ref() {
                    page.upload(&queue, texture);
                }
                page.dirty = false;
            }
        }
    }

    fn rasterize(
        &mut self,
        fonts: &mut FontSystem,
        key: CacheKey,
        scale: f32,
        residue: [f32; 2],
    ) -> Option<GlyphRect> {
        let img = self.swash.get_image_uncached(fonts, key)?;
        let (w, h) = (img.placement.width, img.placement.height);
        if w == 0 || h == 0 {
            return None; // 空格之类没有面积的字形：pen 照样要走，但没东西可画
        }
        if img.content != SwashContent::Mask {
            // `Color`（CBDT/COLR 表情）与 `SubpixelMask`（未请求）都不是单通道覆盖，
            // 塞进 R8 页会画出错色 —— 显式跳过并告警一次，不放空实现。
            if !self.warned_color {
                self.warned_color = true;
                eprintln!(
                    "sisf-text: 字形内容格式 {:?} 不是单通道覆盖率，已跳过（彩色字形本阶段不支持）",
                    img.content
                );
            }
            return None;
        }
        let (page, x, y) = self.reserve(w, h)?;
        // 内层写到装箱位置 + gutter；`chunks_exact` 只会少写，绝不越界碰邻居。
        let stride = PAGE_EDGE as usize;
        for (row, src) in img.data.chunks_exact(w as usize).enumerate() {
            let at = (y as usize + row) * stride + x as usize;
            self.pages[page].cpu[at..at + w as usize].copy_from_slice(src);
        }
        self.pages[page].dirty = true;
        let epoch = self.pages[page].epoch;
        let edge = PAGE_EDGE as f32;
        Some(GlyphRect {
            epoch,
            uv0: [x as f32 / edge, y as f32 / edge],
            uv1: [(x + w) as f32 / edge, (y + h) as f32 / edge],
            size: [w, h],
            // swash 的 `top` 是"基线以上为正"，屏幕 y 向下故取负。1 档时 placement 已经把
            // bin 的亚像素余量算进去了（`residue` 为 0），>1 档才手工补回来。
            offset: [
                img.placement.left as f32 / scale + residue[0],
                -img.placement.top as f32 / scale + residue[1],
            ],
            scale,
        })
    }

    /// 找一个装得下 `(w+2, h+2)` 的位置，必要时开新页；页满返回 `None`。
    fn reserve(&mut self, w: u32, h: u32) -> Option<(usize, u32, u32)> {
        let need = size2((w + 2 * GUTTER) as i32, (h + 2 * GUTTER) as i32);
        for (i, page) in self.pages.iter_mut().enumerate() {
            if let Some(alloc) = page.alloc.allocate(need) {
                let r = alloc.rectangle;
                return Some((
                    i,
                    (r.min.x + GUTTER as i32) as u32,
                    (r.min.y + GUTTER as i32) as u32,
                ));
            }
        }
        if self.pages.len() >= MAX_PAGES {
            if !self.warned_full {
                self.warned_full = true;
                eprintln!(
                    "sisf-text: {MAX_PAGES} 页覆盖率图集已满，后续字形不再落位（提高 MAX_PAGES 或减少字号档位）"
                );
            }
            return None;
        }
        let epoch = MASK_EPOCH | self.next_page;
        self.next_page += 1;
        self.pages.push(Page::new(epoch));
        let i = self.pages.len() - 1;
        let r = self.pages[i].alloc.allocate(need)?.rectangle;
        Some((
            i,
            (r.min.x + GUTTER as i32) as u32,
            (r.min.y + GUTTER as i32) as u32,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::FontBundle;
    use crate::layout::{TextStyle, layout};
    use sisf_render::atlas::{LARGE_EPOCH, PageKind, TARGET_EPOCH};

    const INTER: &[u8] = include_bytes!("../tests/fixtures/Inter-Regular.ttf");

    /// 取一段文字的全部字形键（顺带证明与 `layout` 的衔接是通的）。
    fn keys(bundle: &mut FontBundle, text: &str, size: f32) -> Vec<CacheKey> {
        let run = layout(bundle, text, &TextStyle::new(size));
        run.glyph_keys()
    }

    #[test]
    fn mask_epochs_cannot_collide_with_atlas_pages() {
        // 三个标记位各占一位，且低 61 位留给计数器 ⇒ 谁也不会抢到别人的代号。
        assert_eq!(MASK_EPOCH & (LARGE_EPOCH | TARGET_EPOCH), 0);
        assert_ne!(MASK_EPOCH, 0, "0 号是渲染器内置的 WHITE_EPOCH");
        for n in 0..MAX_PAGES as u64 {
            let epoch = MASK_EPOCH | n;
            assert_eq!(PageKind::of_epoch(epoch), PageKind::Small);
        }
    }

    #[test]
    fn glyphs_get_coverage_rects_with_gutter() {
        let mut bundle = FontBundle::from_bytes(INTER);
        assert!(!bundle.is_fallback());
        let mut atlas = FontAtlas::new();
        let got: Vec<(CacheKey, GlyphRect)> = keys(&mut bundle, "Ag", 24.0)
            .into_iter()
            .filter_map(|k| atlas.glyph(bundle.fonts(), k).map(|r| (k, r)))
            .collect();
        assert_eq!(got.len(), 2, "'A' 与 'g' 都该有覆盖率");
        let edge = PAGE_EDGE as f32;
        for (_, r) in &got {
            assert!(r.size[0] > 2 && r.size[1] > 2, "24px 字形不该只有几个像素");
            for (u, v) in r.uv0.iter().zip(r.uv1) {
                assert!(v > *u && *u >= GUTTER as f32 / edge, "uv 必须单调且在页内");
            }
            // uv 跨度正好等于覆盖率位图尺寸（页边长归一）。
            let du = (r.uv1[0] - r.uv0[0]) * edge;
            let dv = (r.uv1[1] - r.uv0[1]) * edge;
            assert_eq!([du.round() as u32, dv.round() as u32], r.size);
        }
        assert_eq!(atlas.page_count(), 1, "两个字形用不到第二页");
    }

    /// 装箱必须互不重叠，且每个字形都真的把覆盖率写进了自己的格子。
    #[test]
    fn packing_is_disjoint_and_coverage_written() {
        let mut bundle = FontBundle::from_bytes(INTER);
        let mut atlas = FontAtlas::new();
        let mut placed = Vec::new();
        for size in [12.0, 16.0, 24.0] {
            for k in keys(&mut bundle, "SISF 引擎文本", size) {
                if let Some(r) = atlas.glyph(bundle.fonts(), k) {
                    placed.push(r);
                }
            }
        }
        assert!(placed.len() > 20, "三档字号的拉丁+中文该有一批字形");
        let edge = PAGE_EDGE as usize;
        let rects: Vec<(usize, usize, usize, usize)> = placed
            .iter()
            .map(|r| {
                (
                    (r.uv0[0] * edge as f32) as usize - GUTTER as usize,
                    (r.uv0[1] * edge as f32) as usize - GUTTER as usize,
                    r.size[0] as usize + 2 * GUTTER as usize,
                    r.size[1] as usize + 2 * GUTTER as usize,
                )
            })
            .collect();
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                let disjoint =
                    a.0 + a.2 <= b.0 || b.0 + b.2 <= a.0 || a.1 + a.3 <= b.1 || b.1 + b.3 <= a.1;
                assert!(disjoint, "含 gutter 的装箱矩形重叠：{a:?} vs {b:?}");
            }
        }
        // 覆盖率确实落进了影子页（非零像素存在）。
        let page = &atlas.pages[0];
        assert!(page.cpu.iter().any(|&v| v > 200), "整页没有一个全黑像素？");
        assert!(page.dirty, "装箱过后页必须是脏的");
    }

    /// 覆盖率必须是"有深有浅"的量：实心一块说明光栅或上传串了位（golden 里表现为方块字）。
    #[test]
    fn coverage_is_grayscale_not_a_solid_block() {
        let mut bundle = FontBundle::from_bytes(INTER);
        let mut atlas = FontAtlas::new();
        let keys = keys(&mut bundle, "A", 24.0);
        let r = atlas.glyph(bundle.fonts(), keys[0]).expect("'A' 有覆盖率");
        let edge = PAGE_EDGE as usize;
        let (x, y) = (
            (r.uv0[0] * edge as f32) as usize,
            (r.uv0[1] * edge as f32) as usize,
        );
        let mut hist = [0u32; 4];
        for row in 0..r.size[1] as usize {
            for col in 0..r.size[0] as usize {
                let v = atlas.pages[0].cpu[(y + row) * edge + x + col];
                hist[usize::min(v as usize / 64, 3)] += 1;
            }
        }
        assert!(hist[0] > 0, "矩形内没有一个空像素（实心？）: {hist:?}");
        assert!(hist[3] > 0, "矩形内没有一个实心像素: {hist:?}");
        assert!(
            hist[1] + hist[2] > 0,
            "24px 的 'A' 边缘该有过渡像素: {hist:?}"
        );
    }

    /// 没有面积的字形（空格）走负缓存：`cached` 只长一次，且不会占页。
    #[test]
    fn empty_glyphs_are_negative_cached() {
        let mut bundle = FontBundle::from_bytes(INTER);
        let mut atlas = FontAtlas::new();
        let space = keys(&mut bundle, " ", 16.0);
        assert_eq!(space.len(), 1, "空格仍是一个字形，只是没有面积");
        let k = space[0];
        assert!(atlas.glyph(bundle.fonts(), k).is_none());
        assert!(atlas.glyph(bundle.fonts(), k).is_none());
        assert_eq!(atlas.cached(), 1, "第二次查的是负缓存");
        assert_eq!(atlas.page_count(), 0, "没落位就不该建页");
    }

    /// 档位表：向上取到 2 的幂，1× 与缩小采样都停在 1 档（多光栅的像素会被扔掉）。
    #[test]
    fn tiers_round_up_to_powers_of_two() {
        assert_eq!(raster_scale_for(0.5), 1.0);
        assert_eq!(raster_scale_for(1.0), 1.0, "IntegerFit 1× 就该 1:1");
        assert_eq!(raster_scale_for(1.01), 2.0, "刚越过 1 倍就开始发虚");
        assert_eq!(raster_scale_for(1.44), 2.0, "Fit 到 1080p 的典型倍率");
        assert_eq!(raster_scale_for(2.0), 2.0, "IntegerFit 2× 也要 2 档才不糊");
        assert_eq!(raster_scale_for(2.01), MAX_RASTER_SCALE);
        assert_eq!(
            raster_scale_for(9.0),
            MAX_RASTER_SCALE,
            "档位有上限，页容量先撑不住"
        );
    }

    /// 档位 setter 自己就是最后一道闸：坏值不许进装箱（NaN 会把整页覆盖率算成 NaN）。
    #[test]
    fn a_nonsense_tier_falls_back_instead_of_poisoning_the_atlas() {
        let mut atlas = FontAtlas::new();
        atlas.set_raster_scale(f32::NAN);
        assert_eq!(atlas.raster_scale(), 1.0, "NaN 不是档位，必须回 1 档");
        for (weird, want) in [(0.0, 1.0), (-4.0, 1.0), (64.0, MAX_RASTER_SCALE)] {
            atlas.set_raster_scale(weird);
            assert_eq!(atlas.raster_scale(), want, "{weird} 该夹到端点档");
        }
        atlas.set_raster_scale(f32::INFINITY);
        assert_eq!(atlas.raster_scale(), MAX_RASTER_SCALE, "无穷大 = 最大档");
    }

    /// 换档只改位图粗细，不改排版：逻辑宽高与相对 pen 的落点都不许动。
    #[test]
    fn supersampling_adds_detail_without_moving_the_glyph() {
        let mut bundle = FontBundle::from_bytes(INTER);
        let mut atlas = FontAtlas::new();
        let k = keys(&mut bundle, "A", 24.0)[0];
        let one = atlas.glyph(bundle.fonts(), k).expect("1 档有覆盖率");
        assert_eq!(one.scale, 1.0, "默认档就是 1，与旧行为逐字相同");

        atlas.set_raster_scale(2.0);
        let two = atlas.glyph(bundle.fonts(), k).expect("2 档有覆盖率");
        assert_eq!(two.scale, 2.0);
        assert!(
            (two.size[0] as i32 - one.size[0] as i32 * 2).abs() <= 2,
            "2 档的位图宽度该翻倍: {:?} vs {:?}",
            two.size,
            one.size
        );
        // 逻辑度量不变 —— 这条才是"排版不用跟着改"的证据。
        let logical = |r: &GlyphRect| [r.size[0] as f32 / r.scale, r.size[1] as f32 / r.scale];
        let (a, b) = (logical(&one), logical(&two));
        for axis in 0..2 {
            assert!(
                (a[axis] - b[axis]).abs() <= 1.0,
                "第 {axis} 轴的逻辑尺寸被换档改了: {a:?} vs {b:?}"
            );
            assert!(
                (one.offset[axis] - two.offset[axis]).abs() <= 1.0,
                "第 {axis} 轴的落点被换档改了: {:?} vs {:?}",
                one.offset,
                two.offset
            );
        }
        // 两套条目在缓存里共存（合成键的字号位不同），回手查 1 档拿到的还是原矩形。
        assert_eq!(atlas.cached(), 2, "换档不该顶掉旧条目");
        atlas.set_raster_scale(1.0);
        assert_eq!(atlas.glyph(bundle.fonts(), k), Some(one));
    }

    /// 亚像素余量在高档位上落到 quad 位置，而不是被 bin 归零顺手丢掉。
    ///
    /// 一整个词、三档一起过：`layout` 把每字 pen 取整（`x_bin` 就是被舍掉那点推进），
    /// 高档位归零 bin 之后，各字之间的**相对**排布必须仍与 1 档一致（差在一像素内）。
    #[test]
    fn every_tier_keeps_the_same_relative_layout() {
        let mut bundle = FontBundle::from_bytes(INTER);
        let mut atlas = FontAtlas::new();
        let keys = keys(&mut bundle, "raster 引擎", 24.0);
        let mut per_tier = Vec::new();
        for tier in [1.0, 2.0, MAX_RASTER_SCALE] {
            atlas.set_raster_scale(tier);
            let rects: Vec<GlyphRect> = keys
                .iter()
                .filter_map(|k| atlas.glyph(bundle.fonts(), *k))
                .collect();
            assert!(rects.len() >= 6, "'raster 引擎' 光栅出的字形太少");
            for r in &rects {
                assert_eq!(r.scale, tier);
            }
            per_tier.push(rects);
        }
        let left = |r: &GlyphRect| r.offset[0];
        for (tier, rects) in [2.0, MAX_RASTER_SCALE].iter().zip(&per_tier[1..]) {
            for (a, b) in per_tier[0].iter().zip(rects) {
                assert!(
                    (left(a) - left(b)).abs() <= 1.0,
                    "{tier} 档把字形横向挪位了: {} vs {}",
                    left(a),
                    left(b)
                );
                assert!(
                    (a.offset[1] - b.offset[1]).abs() <= 1.0,
                    "{tier} 档把字形纵向挪位了: {} vs {}",
                    a.offset[1],
                    b.offset[1]
                );
            }
        }
        // 每档各一套条目，谁也不顶掉谁（合成键里的字号位不同）。
        assert!(
            atlas.cached() > per_tier[0].len(),
            "三档跑完缓存只装得下一档？cached={} / 一档 {} 个字形",
            atlas.cached(),
            per_tier[0].len()
        );
    }
}
