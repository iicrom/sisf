//! 绘制入口：`TextRun` → 一串**普通 sprite quad**，交给现成的 `Renderer::push`。
//!
//! 这一层刻意薄得能一眼看完：它只做"字形覆盖率矩形 + pen 位置 → `Instance`"，
//! 不建管线、不开 pass、不管合批。文本与精灵的唯一区别是 `Instance::mask = 1.0`
//! （着色器改取 `.r` 当 alpha、颜色全来自 tint），所以管线与合批通路一条都不新增。
//!
//! 采集与提交分成两半是有意的：`run_quads`/`text_quads` 纯 CPU，在没有设备的
//! 环境（含 wasm）里就能断言落点与对齐；`draw_run`/`draw_text` 只是把同一批实例
//! 喂进 `push`——而 `push` 自己按当前状态路由（主帧 or 位图绘制会话），于是
//! RGSS 的 `Bitmap#draw_text` 天然要求 `begin_draw` 在前、`end_draw` 在后。
//!
//! RGSS 的 `draw_text(rect, text, align)` 在本层展开成三条：
//! - 水平：按 `style.align` 逐行偏移（[`TextRun::align_offset`]）；
//! - 垂直：整块在盒子里居中（RPG Maker 按行高折算，这里用 `run.height`——单行时
//!   两者差半个 ascender，取"更直观的那个"）；
//! - **不裁剪**：RGSS 的文字本来就允许溢出 `rect`，窗口要裁剪是它自己
//!   `push_clip` 的事（正好走 T6 的 clip 通路）。

use sisf_render::{BatchKey, Blend, Instance, NO_CLIP, Renderer, Rgba};

use crate::atlas::{FontAtlas, GlyphRect};
use crate::bundle::FontBundle;
use crate::layout::{PlacedGlyph, TextRun, TextStyle, layout_with};

/// 字体包 + 覆盖率图集 + 提交，三件事的门面。
pub struct TextEngine {
    bundle: FontBundle,
    atlas: FontAtlas,
}

impl TextEngine {
    pub fn new(bundle: FontBundle) -> Self {
        Self {
            bundle,
            atlas: FontAtlas::new(),
        }
    }

    /// 便捷构造：VFS 读到的字体字节进 `FontBundle::from_bytes`（缺失时回退并 warn 一次）。
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self::new(FontBundle::from_bytes(bytes))
    }

    pub fn bundle(&self) -> &FontBundle {
        &self.bundle
    }

    pub fn bundle_mut(&mut self) -> &mut FontBundle {
        &mut self.bundle
    }

    pub fn atlas(&self) -> &FontAtlas {
        &self.atlas
    }

    /// 排版（纯 CPU；结果与设备无关，所以每帧重排也不是错事）。
    pub fn layout(&mut self, text: &str, style: &TextStyle) -> TextRun {
        layout_with(self.bundle.fonts(), text, style)
    }

    /// 给新落位的覆盖率页建纹理并注册进渲染器。**每帧在 `end_frame`/`end_draw` 之前调**。
    ///
    /// 顺序不能反：字形是在 `draw_*` 里头一次光栅化的（`FontAtlas::glyph` 按需装箱），
    /// 所以 `sync` 必须**晚于**本帧所有 `draw_*`；而 pass 在 `end_frame`/`end_draw` 里
    /// 按 epoch 取视图建 bind group，注册晚了这一帧的字就缺着。会话中途调也安全。
    pub fn sync(&mut self, renderer: &mut Renderer) {
        self.atlas.sync(renderer);
    }

    /// 一个 run 的实例（`at` = run 左上角，逐行左对齐）。
    pub fn run_quads(
        &mut self,
        run: &TextRun,
        at: [f32; 2],
        color: Rgba,
        layer: u8,
        z: i32,
    ) -> Vec<(BatchKey, Instance)> {
        self.quads(run, &|_| at, color, layer, z)
    }

    /// 把 run 画出去，返回实例数（= 有覆盖率面积的字形数）。
    pub fn draw_run(
        &mut self,
        renderer: &mut Renderer,
        run: &TextRun,
        at: [f32; 2],
        color: Rgba,
        layer: u8,
        z: i32,
    ) -> u32 {
        let quads = self.run_quads(run, at, color, layer, z);
        Self::push_quads(renderer, quads)
    }

    /// 按 `rect` 盒排版 + 对齐落成实例。换行与否原样听 `style.wrap_width` 的
    /// （RGSS 的 `draw_text` 不换行；消息窗要折行就 `TextStyle::with_wrap`）。
    pub fn text_quads(
        &mut self,
        rect: [f32; 4],
        text: &str,
        style: &TextStyle,
        color: Rgba,
    ) -> Vec<(BatchKey, Instance)> {
        let run = self.layout(text, style);
        let align = style.align;
        let dy = (rect[3] - run.height) * 0.5;
        let offsets = |i: usize| [rect[0] + run.align_offset(i, rect[2], align), rect[1] + dy];
        self.quads(&run, &offsets, color, 0, 0)
    }

    /// RGSS 的 `Bitmap#draw_text(rect, text, align)`。
    ///
    /// 在绘制会话里调用即画进那张位图，会话外画进当前帧目标。
    pub fn draw_text(
        &mut self,
        renderer: &mut Renderer,
        rect: [f32; 4],
        text: &str,
        style: &TextStyle,
        color: Rgba,
    ) -> u32 {
        let quads = self.text_quads(rect, text, style, color);
        Self::push_quads(renderer, quads)
    }

    fn push_quads(renderer: &mut Renderer, quads: Vec<(BatchKey, Instance)>) -> u32 {
        let count = quads.len() as u32;
        for (key, instance) in quads {
            renderer.push(key, instance);
        }
        count
    }

    /// 公共实现：逐行取偏移、逐字形取覆盖率矩形。
    fn quads(
        &mut self,
        run: &TextRun,
        offsets: &dyn Fn(usize) -> [f32; 2],
        color: Rgba,
        layer: u8,
        z: i32,
    ) -> Vec<(BatchKey, Instance)> {
        let mut out = Vec::with_capacity(run.glyph_count());
        for (i, line) in run.lines.iter().enumerate() {
            let at = offsets(i);
            for glyph in &line.glyphs {
                let key = glyph.key.cache();
                // `atlas` 与 `bundle` 是不相交字段，同一表达式里各借一次可变引用。
                let Some(rect) = self.atlas.glyph(self.bundle.fonts(), key) else {
                    continue; // 空格之类：pen 已经走过，只是这块没有可画面积
                };
                out.push((
                    batch_key(&rect, layer, z),
                    instance(&rect, glyph, at, color, layer),
                ));
            }
        }
        out
    }
}

/// 字形批的键：`clip` 由 `Renderer::push` 按当前栈顶覆写，这里只声明"不挑裁剪"。
fn batch_key(rect: &GlyphRect, layer: u8, z: i32) -> BatchKey {
    BatchKey {
        layer,
        clip: NO_CLIP,
        epoch: rect.epoch,
        blend: Blend::Normal,
        z,
        insertion: 0,
    }
}

/// 覆盖率矩形 + pen → 实例：左上角 = `at + pen + offset`，`Instance::pos` 是中心。
fn instance(
    rect: &GlyphRect,
    glyph: &PlacedGlyph,
    at: [f32; 2],
    color: Rgba,
    layer: u8,
) -> Instance {
    let (w, h) = (rect.size[0] as f32, rect.size[1] as f32);
    let left = at[0] + glyph.x as f32 + rect.offset[0] as f32;
    let top = at[1] + glyph.y as f32 + rect.offset[1] as f32;
    Instance {
        pos: [left + w / 2.0, top + h / 2.0],
        size: [w, h],
        uv0: rect.uv0,
        uv1: rect.uv1,
        color,
        rot: 0.0,
        tone: [0.0; 4],
        layer: f32::from(layer),
        mask: 1.0,
        _pad: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::MASK_EPOCH;
    use crate::layout::Align;

    const INTER: &[u8] = include_bytes!("../tests/fixtures/Inter-Regular.ttf");

    fn engine() -> TextEngine {
        TextEngine::from_bytes(INTER)
    }

    /// 实例左上角（`pos` 是中心，故减去半宽高）。
    fn top_left(inst: &Instance) -> [f32; 2] {
        [
            inst.pos[0] - inst.size[0] / 2.0,
            inst.pos[1] - inst.size[1] / 2.0,
        ]
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1.0
    }

    /// 画一次并返回首字左上角（对齐/居中那几条断言只关心这个点）。
    fn head(e: &mut TextEngine, rect: [f32; 4], text: &str, style: TextStyle) -> [f32; 2] {
        let quads = e.text_quads(rect, text, &style, [1.0; 4]);
        assert!(!quads.is_empty(), "{text:?} 一个字形都没画出来");
        top_left(&quads[0].1)
    }

    #[test]
    fn a_run_is_plain_masked_sprite_quads() {
        let mut e = engine();
        let run = e.layout("Ag", &TextStyle::new(24.0));
        let quads = e.run_quads(&run, [10.0, 20.0], [1.0, 0.0, 0.0, 1.0], 0, 5);
        assert_eq!(quads.len(), 2, "'A' 与 'g' 都有覆盖率");
        for (key, inst) in &quads {
            assert_eq!(inst.mask, 1.0, "mask 是文本与精灵的唯一区别");
            assert_eq!(inst.color, [1.0, 0.0, 0.0, 1.0], "颜色全来自 tint");
            assert_eq!(inst.rot, 0.0);
            assert_eq!(key.epoch & MASK_EPOCH, MASK_EPOCH, "字形页带 mask 标记位");
            assert_eq!(key.blend, Blend::Normal);
            assert_eq!(key.z, 5);
            assert!(inst.size[0] > 0.0 && inst.size[1] > 0.0);
            assert!(
                inst.uv0.iter().zip(inst.uv1).all(|(a, b)| b > *a),
                "uv 必须落在页内且单调"
            );
        }
        let (a, b) = (top_left(&quads[0].1), top_left(&quads[1].1));
        assert!(b[0] > a[0], "'g' 该排在 'A' 右边：{a:?} vs {b:?}");
        // 'g' 有下伸部：矩形底边该低于 'A' 的底边。
        let bottom = |i: &Instance| i.pos[1] + i.size[1] / 2.0;
        assert!(bottom(&quads[1].1) > bottom(&quads[0].1));
        // 整块落在 `at` 右下侧（24px 文字的高约 1.25 行高以内）。
        assert!(a[0] >= 10.0 - 2.0 && a[1] > 20.0 - 24.0, "落点离谱: {a:?}");
    }

    #[test]
    fn blank_glyphs_take_pen_but_produce_nothing() {
        let mut e = engine();
        let run = e.layout("a b", &TextStyle::new(16.0));
        assert_eq!(run.glyph_count(), 3, "空格仍参与排版");
        let quads = e.run_quads(&run, [0.0, 0.0], [1.0; 4], 0, 0);
        assert_eq!(quads.len(), 2, "空格没有覆盖率矩形");
        let step = top_left(&quads[1].1)[0] - top_left(&quads[0].1)[0];
        assert!(
            step > quads[0].1.size[0],
            "两字距离要装得下一个空格: {step} vs {}",
            quads[0].1.size[0]
        );
    }

    /// 对齐：同一个 run 画进同一个盒子，左/中/右的相对位移正好由行宽与盒宽决定。
    /// 用差值断言，把 pen 起点与字形左边距这两个未知量整体消掉。
    #[test]
    fn draw_text_aligns_within_the_box() {
        let mut e = engine();
        let rect = [100.0, 50.0, 200.0, 40.0];
        let style = TextStyle::new(16.0);
        let line_w = e.layout("abc", &style).lines[0].width;
        let left = head(&mut e, rect, "abc", style.clone().aligned(Align::Left));
        let center = head(&mut e, rect, "abc", style.clone().aligned(Align::Center));
        let right = head(&mut e, rect, "abc", style.aligned(Align::Right));
        assert!(
            near(center[0] - left[0], (rect[2] - line_w) * 0.5),
            "居中位移不对：{} - {} 应为 {}",
            center[0],
            left[0],
            (rect[2] - line_w) * 0.5
        );
        assert!(
            near(right[0] - left[0], rect[2] - line_w),
            "右对齐位移不对：{} - {} 应为 {}",
            right[0],
            left[0],
            rect[2] - line_w
        );

        // 垂直：盒往下挪 ⇒ 整块同幅挪；盒变高 ⇒ 居中的块也变低。
        // 用平移量而不是绝对落点断言：字形顶边相对行顶还差一个 ascender，那个数不该写死。
        let base = head(&mut e, rect, "abc", TextStyle::new(16.0))[1];
        let moved = head(
            &mut e,
            [100.0, 60.0, 200.0, 40.0],
            "abc",
            TextStyle::new(16.0),
        )[1];
        let taller = head(
            &mut e,
            [100.0, 50.0, 200.0, 80.0],
            "abc",
            TextStyle::new(16.0),
        )[1];
        assert!(near(moved - base, 10.0), "y 平移不跟盒: {moved} vs {base}");
        assert!(
            near(taller - base, 20.0),
            "盒高 40→80 的居中位移应是 20px，实得 {}",
            taller - base
        );
    }

    /// 多行：给了换行宽就折，折出来的每一行都回到盒子左边界、且逐行往下。
    #[test]
    fn wrapped_text_lays_out_line_by_line() {
        let mut e = engine();
        let style = TextStyle::new(16.0).with_wrap(60.0);
        let text = "引擎渲染器测试";
        let quads = e.text_quads([0.0, 0.0, 60.0, 200.0], text, &style, [1.0; 4]);
        let lines = e.layout(text, &style);
        assert!(!quads.is_empty());
        assert!(
            lines.line_count() >= 2,
            "60px 盒子里这串中文该被折开: {:?}",
            lines.line_count()
        );
        for line in &lines.lines {
            assert!(line.width <= 61.0, "折行后仍有行越界: {}", line.width);
        }

        // 行顶：同一行内不同字形的顶边会差 1px（各字的 `placement.top` 不同），
        // 所以先排序再按 4px 归并，才得到"有几个视觉行"。
        let mut tops: Vec<f32> = quads.iter().map(|(_, i)| top_left(i)[1]).collect();
        tops.sort_unstable_by(|a, b| a.total_cmp(b));
        tops.dedup_by(|a, b| *a - *b < 4.0);
        assert_eq!(
            tops.len(),
            lines.line_count(),
            "行顶数与排版行数不符: {tops:?}"
        );
        for pair in tops.windows(2) {
            assert!(pair[1] > pair[0], "行必须往下走: {pair:?}");
        }
        // 每行的第一个字都贴回盒子左边界（左对齐不是"整块居中"）。
        let at_left = quads.iter().filter(|(_, i)| top_left(i)[0] < 8.0).count();
        assert_eq!(at_left, tops.len(), "每行都该有一个贴左边界的字");
    }

    #[test]
    fn layer_and_z_reach_every_batch_key() {
        let mut e = engine();
        let run = e.layout("Hi", &TextStyle::new(16.0));
        let quads = e.run_quads(&run, [0.0, 0.0], [1.0; 4], 3, -7);
        assert_eq!(quads.len(), 2);
        for (key, inst) in &quads {
            assert_eq!(key.layer, 3);
            assert_eq!(inst.layer, 3.0, "投影索引还得在实例里可读");
            assert_eq!(key.z, -7);
        }
        // 同页同层 ⇒ 合批键同组，一次 draw 就能画完这批字。
        assert_eq!(quads[0].0.epoch, quads[1].0.epoch, "两个字同一页");
    }

    #[test]
    fn empty_text_draws_nothing() {
        let mut e = engine();
        assert!(
            e.text_quads([0.0, 0.0, 32.0, 32.0], "", &TextStyle::new(16.0), [1.0; 4])
                .is_empty()
        );
        assert!(e.atlas().page_count() <= 1, "没字就不该铺页");
    }
}
