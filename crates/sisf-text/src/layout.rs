//! shaping → `TextRun`：cosmic-text 的排版结果摊成引擎自己的、不含 GPU 依赖的结构。
//!
//! 只做三件事：`Buffer`（字号/行高/换行宽）→ `shape_until_scroll` → 逐
//! `LayoutRun`/`LayoutGlyph` 取 `physical((0, line_y), 1.0)` 的 **pen 位置**。
//! 于是 `TextRun` 是一串"整数像素 pen + 字形代号"，画的时候再去图集取覆盖率矩形
//! ——形状与位图解耦，所以本模块三端（含 wasm）都能跑单测，不需要设备。
//!
//! 两处刻意的选择：
//! - **对齐不在 shaping 里做**：`set_text` 的 `alignment` 会按盒宽平移字形，而 RPG
//!   Maker 的 `draw_text` 是"同一个 run 可以画进不同宽的盒子"。所以恒按左排，
//!   对齐在绘制期用 `TextLine::width` 现算。
//! - **pen 位置取整**（`physical` 的 `x` 是 `i32`）：文字要落在像素格上，否则
//!   1x 映射下每行都会糊。字形覆盖率自身的亚像素偏移由 swash 烘进位图里。

use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Weight, Wrap};

use crate::bundle::FontBundle;

/// 水平对齐（RGSS 的 `draw_text` 第三参）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// 一个字形的身份：`字体 face + 字形号 + 字号 + 位置`。
///
/// 内部是 cosmic-text 的 `CacheKey`，包一层是为了不让它出现在公开签名里 ——
/// 换 shaping 后端时 `TextRun` 的形状不必跟着变。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GlyphKey(cosmic_text::CacheKey);

impl GlyphKey {
    pub(crate) fn cache(self) -> cosmic_text::CacheKey {
        self.0
    }

    /// 该字形来自哪个 face（中英混排时中文与拉丁落在不同 face 上，测试要用）。
    #[cfg(test)]
    pub(crate) fn face(self) -> cosmic_text::fontdb::ID {
        self.0.font_id
    }
}

/// 排版输入。字号/行高都是**像素**（引擎的虚拟像素，与 viewport 缩放无关）。
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    /// 字号（像素）。
    pub size: f32,
    /// 行高（像素）。
    pub line_height: f32,
    /// 指定族名（`None` = 字体包里的默认 sans，也就是自带字体优先的那一族）。
    pub family: Option<String>,
    /// 字重（100..=900，400=常规）。
    pub weight: u16,
    /// 换行宽度（`None` = 不换行）。
    pub wrap_width: Option<f32>,
    /// 水平对齐（`draw_text` 用；`draw_run` 一律按左对齐落位）。
    pub align: Align,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self::new(16.0)
    }
}

impl TextStyle {
    /// 行高按 1.25 倍字号（RPG Maker 的 20/36 换算下来正好这个比例附近）。
    pub fn new(size: f32) -> Self {
        Self {
            size,
            line_height: size * 1.25,
            family: None,
            weight: 400,
            wrap_width: None,
            align: Align::Left,
        }
    }

    pub fn with_wrap(mut self, width: f32) -> Self {
        self.wrap_width = Some(width);
        self
    }

    pub fn aligned(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    pub fn weight(mut self, weight: u16) -> Self {
        self.weight = weight;
        self
    }
}

/// 一个落好位的字形：`x/y` 是 pen（buffer 局部整数像素，y 向下）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedGlyph {
    pub key: GlyphKey,
    pub x: i32,
    pub y: i32,
}

/// 一行。
#[derive(Clone, Debug, PartialEq)]
pub struct TextLine {
    /// 行顶（buffer 局部 y）。
    pub top: f32,
    /// 基线（buffer 局部 y）。
    pub baseline: f32,
    /// 行宽（用于对齐）。
    pub width: f32,
    pub glyphs: Vec<PlacedGlyph>,
}

/// 一次排版的完整结果。
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TextRun {
    pub lines: Vec<TextLine>,
    /// 最宽一行的宽度。
    pub width: f32,
    /// 整块高度（最后一行的底边）。
    pub height: f32,
    /// 排版用的行高。
    pub line_height: f32,
}

impl TextRun {
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn glyph_count(&self) -> usize {
        self.lines.iter().map(|l| l.glyphs.len()).sum()
    }

    /// 没有任何可画字形（空串、或全是不显示的控制字符）。
    pub fn is_empty(&self) -> bool {
        self.glyph_count() == 0
    }

    /// 第 `i` 行在宽 `box_width` 的盒子里按 `align` 对齐时的水平偏移。
    pub fn align_offset(&self, i: usize, box_width: f32, align: Align) -> f32 {
        let width = self.lines.get(i).map_or(0.0, |l| l.width);
        match align {
            Align::Left => 0.0,
            Align::Center => (box_width - width) * 0.5,
            Align::Right => box_width - width,
        }
    }

    #[cfg(test)]
    pub(crate) fn glyph_keys(&self) -> Vec<cosmic_text::CacheKey> {
        self.lines
            .iter()
            .flat_map(|l| l.glyphs.iter().map(|g| g.key.cache()))
            .collect()
    }
}

/// 排一段文字。`text` 里的 `\n` 强制换行，其余按 `style.wrap_width` 自动折行。
pub fn layout(bundle: &mut FontBundle, text: &str, style: &TextStyle) -> TextRun {
    let fonts = bundle.fonts();
    layout_with(fonts, text, style)
}

/// 上面那行的实现体（`TextEngine` 直接拿 `FontSystem` 时也用它）。
pub(crate) fn layout_with(fonts: &mut FontSystem, text: &str, style: &TextStyle) -> TextRun {
    let (size, line_height) = (style.size.max(1.0), style.line_height.max(1.0));
    let mut buf = Buffer::new_empty(Metrics::new(size, line_height));
    if style.wrap_width.is_some() {
        // 词边界优先、单个超长词按字形折 —— 中英混排的换行要靠它（中文没有空格）。
        buf.set_wrap(Wrap::WordOrGlyph);
    }
    // 高度不设限：不设视口就不会滚动，一次排完。
    buf.set_size(style.wrap_width, None);
    let family = style
        .family
        .as_deref()
        .map_or(Family::SansSerif, Family::Name);
    let attrs = Attrs::new().family(family).weight(Weight(style.weight));
    buf.set_text(text, &attrs, Shaping::Advanced, None);
    buf.shape_until_scroll(fonts, false);

    let mut run = TextRun {
        lines: Vec::new(),
        width: 0.0,
        height: line_height,
        line_height,
    };
    for line in buf.layout_runs() {
        let glyphs = line
            .glyphs
            .iter()
            .map(|g| {
                let pen = g.physical((0.0, line.line_y), 1.0);
                PlacedGlyph {
                    key: GlyphKey(pen.cache_key),
                    x: pen.x,
                    y: pen.y,
                }
            })
            .collect();
        run.width = run.width.max(line.line_w);
        run.height = run.height.max(line.line_top + line_height);
        run.lines.push(TextLine {
            top: line.line_top,
            baseline: line.line_y,
            width: line.line_w,
            glyphs,
        });
    }
    run
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTER: &[u8] = include_bytes!("../tests/fixtures/Inter-Regular.ttf");

    fn bundle() -> FontBundle {
        FontBundle::from_bytes(INTER)
    }

    #[test]
    fn size_drives_metrics_monotonically() {
        let mut b = bundle();
        let widths: Vec<(f32, f32, f32)> = [12.0f32, 16.0, 24.0]
            .into_iter()
            .map(|size| {
                let run = layout(&mut b, "Hello 引擎", &TextStyle::new(size));
                (size, run.width, run.height)
            })
            .collect();
        assert_eq!(widths.len(), 3);
        for w in widths.windows(2) {
            assert!(w[1].1 > w[0].1, "字号 {} 反而不比 {} 宽", w[1].0, w[0].0);
            assert!(w[1].2 > w[0].2, "字号 {} 行高没涨", w[1].0);
            assert_eq!(w[1].1 / w[0].1, w[1].1 / w[0].1, "比值该是有限数");
        }
        // 同一串文字，字号翻倍 ⇒ 宽度近似翻倍（取整误差以内）。
        let ratio = widths[2].1 / widths[0].1;
        assert!((2.0 - ratio).abs() < 0.25, "12→24 的宽度倍数离谱: {ratio}");
    }

    /// 自动换行：给宽就该折，且没有任何一行超出盒宽。
    #[test]
    fn wrap_breaks_lines_within_the_box() {
        let mut b = bundle();
        let text = "The quick brown fox jumps over the lazy dog 引擎渲染器测试文本";
        let nowrap = layout(&mut b, text, &TextStyle::new(16.0));
        assert_eq!(nowrap.line_count(), 1, "不换行就该是一行");

        let wrapped = layout(&mut b, text, &TextStyle::new(16.0).with_wrap(120.0));
        assert!(
            wrapped.line_count() >= 4,
            "120px 宽该把它折成好几行，实际 {}",
            wrapped.line_count()
        );
        for line in &wrapped.lines {
            assert!(line.width <= 120.0 + 1.0, "行宽 {} 越过盒宽", line.width);
        }
        assert!(
            wrapped.glyph_count() > 0 && wrapped.width <= 121.0,
            "折行后总字形数/宽度不对"
        );
    }

    /// 超长单词（中文整句就没有空格）必须按字形硬折，而不是撑破盒子。
    #[test]
    fn overlong_word_breaks_by_glyph() {
        let mut b = bundle();
        let run = layout(
            &mut b,
            "引擎渲染器测试文本一二三四五六七八九十",
            &TextStyle::new(16.0).with_wrap(50.0),
        );
        assert!(
            run.line_count() >= 3,
            "50px 装不下这么多中文: {:?}",
            run.line_count()
        );
        assert!(run.lines.iter().all(|l| l.width <= 51.0));
    }

    /// 中英混排：两种文字该由两个不同 face 承担（自带字体没有中文时由系统字体补）。
    #[test]
    fn mixed_script_uses_two_faces() {
        let mut b = bundle();
        if b.face_count() < 2 {
            // 没有任何系统字体（wasm 上就是这台机器的正常状态）：跳过而不是造假。
            eprintln!("跳过：本环境只有一个 face，测不到混排回退");
            return;
        }
        let run = layout(&mut b, "A引擎", &TextStyle::new(16.0));
        let faces: Vec<_> = run
            .lines
            .iter()
            .flat_map(|l| l.glyphs.iter())
            .map(|g| g.key.face())
            .collect();
        assert!(faces.len() >= 2, "中文没被排出来: {faces:?}");
        assert!(
            faces.windows(2).any(|w| w[0] != w[1]),
            "拉丁与中文该落在不同 face 上"
        );
    }

    #[test]
    fn pen_positions_are_integral_and_monotonic_per_line() {
        let mut b = bundle();
        let run = layout(&mut b, "abc", &TextStyle::new(16.0));
        let glyphs = &run.lines[0].glyphs;
        assert_eq!(glyphs.len(), 3);
        for pair in glyphs.windows(2) {
            assert!(pair[1].x >= pair[0].x, "拉丁行内 pen 不该后退");
        }
        assert!(glyphs.iter().all(|g| g.y == glyphs[0].y), "同一行共用基线");
        // y 就是 `line_y`（基线）：字形矩形相对它上下浮动。
        assert!(glyphs[0].y > 0);
    }

    #[test]
    fn empty_text_has_nothing_to_draw() {
        let mut b = bundle();
        let run = layout(&mut b, "", &TextStyle::new(16.0));
        assert!(run.is_empty());
        assert_eq!(run.glyph_count(), 0);
        assert!(
            run.line_count() <= 1,
            "空串最多一行占位: {:?}",
            run.line_count()
        );
    }

    #[test]
    fn alignment_offsets_follow_the_box() {
        let mut b = bundle();
        let run = layout(&mut b, "abc", &TextStyle::new(16.0));
        let w = run.lines[0].width;
        let box_w = w + 40.0;
        assert_eq!(run.align_offset(0, box_w, Align::Left), 0.0);
        assert_eq!(run.align_offset(0, box_w, Align::Center), 20.0);
        assert_eq!(run.align_offset(0, box_w, Align::Right), 40.0);
        // 越界行号按空行处理（画不出东西，但也不该 panic）。
        assert_eq!(run.align_offset(9, box_w, Align::Center), box_w * 0.5);
    }

    #[test]
    fn explicit_newline_forces_a_break_before_wrapping() {
        let mut b = bundle();
        let run = layout(&mut b, "one\ntwo", &TextStyle::new(16.0));
        assert_eq!(run.line_count(), 2);
        assert_eq!(run.glyph_count(), 6, "换行符本身不占字形");
    }
}
