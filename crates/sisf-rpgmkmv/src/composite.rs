//! **软件合成**：把 [`crate::paint`] 里那条具名操作流**回放**成一颗位图的像素
//! （M10.5 第四格第四小格，2026-10-05）。
//!
//! ## 为什么要有这一层
//!
//! 第三小格留下一条有名字的后果：`$dataSystem.windowTone` 实测 `[-51,-51,-51,0]`（非零），而
//! `adjustTone` 在 core 里唯一的调用点是 `Window.prototype._refreshBack`（`rpg_core.js:6688`）——
//! 那一颗 `Bitmap` 是 `new Bitmap(w,h)` 再 `blt` 画出来的，**没有像素源** ⇒ 色调落在一片零上，
//! 窗皮比上游亮 51/255 每通道（`M10.md` 事实 87）。GPU 那一侧读不回来（`Renderer::readback` 只读
//! 主离屏目标、而且是整张，`crates/sisf-render/src/renderer.rs:800-855`），所以"画出来的那一张要能
//! 被读"这件事只有一条腿：**在 CPU 里把它画出来**。
//!
//! 影响面是量过的，不是猜的（尺子 `../sisf_pray/tools/probes/software-composite.py`）：全工程读/写
//! 像素的调用点 **10 处**，其中落在"画出来的那一张"上的只有上面那一条 + 启用插件里的 `TMNamePop`
//! 一处 `getPixel`。所以合成是**懒的**：只有真的有人问像素时才回放那一条腿之前的操作，
//! 每一笔画面照旧只进操作流（屏幕上画什么仍由宿主的绘制会话证，不在这里）。
//!
//! ## 三条口径
//!
//! 1. **回放不是重画一遍上游**：`Bitmap.blt` 里那道 `sx + sw <= source.width` 的闸、`drawText` 的
//!    对齐与 `maxWidth`、`adjustTone` 的算术，全都已经在上游那一格里算完并落成一条操作
//!    （第二、三小格）。这一层只吃**操作串**，一条判断都不替上游做。
//! 2. **认得下的是算术那六格**：`size` · `blt` · `fillRect` · `clearRect` · `gradientFillRect` ·
//!    `fillArc`（后两名 M10.5 真绘制第二小格加进来 —— 渐变与圆**只需要两格停靠点与一条半径**，
//!    不需要任何外部素材，所以落在 CPU 这一侧，读像素与上屏两条腿同享一份内容）。
//!    剩下 [`NOT_SYNTHESIZED`] 那两格（`fillText`/`strokeText`）今天**不回放**，而且每条都要数得出
//!    名字 —— 文字要的是字形位图，引擎那一份只住图集（`crates/sisf-text/src/atlas.rs:159` 的
//!    `Page.cpu` 没有公开的读手），CPU 里没有第二份可抄 ⇒ 它归宿主的 GPU 绘制会话。
//!    混合值同理：[`BLENDS`] 之外的（`difference`/`saturation`/`destination-out`…，尺子第⑤段量的）
//!    一律**跳过并点名**，不假装画对。
//! 3. **像素的域是预乘**（与 [`crate::raster`] 的镜像同一个域）：`source-over` 就是
//!    `out = src + dst×(1−srcA)`（AGENTS 咬人第 25 条⑵那条腿的另一面），`lighter` 是
//!    预乘相加再夹到 255。缩放开的是 **nearest** —— 上游 `_smooth` 默认 false（`rpg_core.js:838`）
//!    而 103 条启用插件里没有一条写 `.smooth = true`（尺子量的），所以不需要双线性那一档。
//!
//! `size` 那一格带一条**上游的语义**，容易做错也最容易读成"顺手就行"：第一次 `size`（`_createCanvas`，
//! `:754-772`）是"建画布**并把那张图贴上去**"（上游收尾就是 `drawImage(this._image, 0, 0)` 那一句），
//! 而后续的 `size`（逐字的 `resize`，`:1129-1136` 给 `_canvas.width` 赋值）是**改尺寸 + 清空**。
//! 所以播种只在第一次，之后一律抹平 —— 与 [`crate::raster`] 那边"镜像按 URL 共享"合起来才对得上。

use sisf_media::image::RgbaImage;
use std::rc::Rc;

/// 今天回放得了的动作（判据 `the_compositor_knows_exactly_the_arithmetic_ops` 数这张表）。
/// 后两名是 M10.5 真绘制第二小格加进来的：**渐变与圆都是纯算术**（两格停靠点 / 一条半径），
/// 不需要任何外部素材 ⇒ 落在 CPU 这一侧，读像素与上屏两条腿同享一份内容。
pub const SYNTHESIZED: &[&str] = &[
    "size",
    "blt",
    "fillRect",
    "clearRect",
    "gradientFillRect",
    "fillArc",
];
/// 表内但**不回放**的动作：每一条都要在报表里数得出名字与次数。
/// 剩下的这两格要的是**字形位图**，引擎那一份只住图集（`crates/sisf-text/src/atlas.rs:159`，
/// 它的 CPU 影子没有公开的读手）⇒ 不许在 CPU 里另画一份字。
pub const NOT_SYNTHESIZED: &[&str] = &["fillText", "strokeText"];
/// 认得下的混合值。上游已交文本只会写成 `source-over`（`Bitmap.blt`/`bltImage`，`:1157`/`:1182`），
/// `lighter` 在这里是为了 `blur` 那一格接上时不用回头改这一层（它现在还没交）。
pub const BLENDS: &[&str] = &["source-over", "lighter"];

/// 一条具名操作（只解析到"属于哪颗位图 + 做什么"这一层）。
///
/// `PartialEq` 而**不是** `Eq`：渐变与圆那两条的位置是浮点像素（上游 `drawCircle` 的圆心走
/// `Math.randomInt`，`gradientFillRect` 的矩形是浮点），而 `f64` 没有 `Eq`。
#[derive(Debug, PartialEq)]
pub enum Op<'a> {
    /// `size|<sid>|<w>|<h>|<urlencoded>`
    Size {
        sid: &'a str,
        width: u32,
        height: u32,
        url: &'a str,
    },
    /// `blt|<dst>|<srcSid>|<srcUrl>|<sx>|<sy>|<sw>|<sh>|<dx>|<dy>|<dw>|<dh>|<alpha>|<blend>`
    Blt {
        dst: &'a str,
        src_sid: &'a str,
        src_url: &'a str,
        sx: i64,
        sy: i64,
        sw: u32,
        sh: u32,
        dx: i64,
        dy: i64,
        dw: u32,
        dh: u32,
        alpha: u32,
        blend: &'a str,
    },
    /// `fillRect|<sid>|<x>|<y>|<w>|<h>|<r>,<g>,<b>,<a>|<blend>`（颜色是**非预乘**的那一种）
    FillRect {
        dst: &'a str,
        x: i64,
        y: i64,
        width: u32,
        height: u32,
        rgba: [u32; 4],
        blend: &'a str,
    },
    /// `clearRect|<sid>|<x>|<y>|<w>|<h>`
    ClearRect {
        dst: &'a str,
        x: i64,
        y: i64,
        width: u32,
        height: u32,
    },
    /// `gradientFillRect|<sid>|<x>|<y>|<w>|<h>|<r>,<g>,<b>,<a>|<r>,<g>,<b>,<a>|<vertical>|<blend>`
    /// （两格停靠点是**非预乘**的，与 `fillRect` 那一条同一个域；`vertical` 是 `1`/`0`）
    GradientRect {
        sid: &'a str,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        from: [u32; 4],
        to: [u32; 4],
        vertical: bool,
        blend: &'a str,
    },
    /// `fillArc|<sid>|<cx>|<cy>|<r>|<r>,<g>,<b>,<a>|<blend>`（`Bitmap.drawCircle` 的 `arc` + `fill`）
    Arc {
        sid: &'a str,
        cx: f64,
        cy: f64,
        r: f64,
        rgba: [u32; 4],
        blend: &'a str,
    },
    /// 表内但不回放（[`NOT_SYNTHESIZED`] 那两格）。`sid` 让这一条落在**正确那颗位图**的账上。
    NotYet { sid: &'a str, action: &'a str },
    /// 表外的动作 —— 出现就意味着操作表与合成器脱钩了（尺子第①段就是为这一格存在）。
    Other(&'a str),
    /// 形状不认识（动作认得但字段数不对 —— 契约破了，不猜哪一格缺了）。
    Bad,
}

/// 解析一条具名操作。**字段数就是契约**：每一条都按 `shim_context.js` 那一格的实际拼接对表
/// （`blt` 是 14 格、`fillRect` 是 8 格、`clearRect` 与 `size` 是 6/5 格）——
/// 数字不对就当坏形状点名，不去猜哪一格缺了。
///
/// URL 一律**不解码**：交回的是 `encodeURIComponent` 那一串，解码住在调用方
/// （[`crate::raster`] 的同一条纪律：两处各解一遍就是第二个真相）。
pub fn parse(spec: &str) -> Op<'_> {
    let f: Vec<&str> = spec.split('|').collect();
    let action = f.first().copied().unwrap_or("");
    let sid = f.get(1).copied().unwrap_or("");
    let num = |s: &str| s.parse::<f64>().ok();
    let px = |s: &str| num(s).map(|v| v.round().max(0.0) as u32);
    let ipx = |s: &str| num(s).map(|v| v.round() as i64);
    match action {
        "size" => {
            if f.len() != 5 {
                return Op::Bad;
            }
            match (px(f[2]), px(f[3])) {
                (Some(width), Some(height)) => Op::Size {
                    sid,
                    width,
                    height,
                    url: f[4],
                },
                _ => Op::Bad,
            }
        }
        "clearRect" => {
            if f.len() != 6 {
                return Op::Bad;
            }
            match (ipx(f[2]), ipx(f[3]), px(f[4]), px(f[5])) {
                (Some(x), Some(y), Some(width), Some(height)) => Op::ClearRect {
                    dst: sid,
                    x,
                    y,
                    width,
                    height,
                },
                _ => Op::Bad,
            }
        }
        "fillRect" => {
            if f.len() != 8 {
                return Op::Bad;
            }
            match (ipx(f[2]), ipx(f[3]), px(f[4]), px(f[5]), parse_rgba(f[6])) {
                (Some(x), Some(y), Some(width), Some(height), Some(rgba)) => Op::FillRect {
                    dst: sid,
                    x,
                    y,
                    width,
                    height,
                    rgba,
                    blend: f[7],
                },
                _ => Op::Bad,
            }
        }
        "blt" => {
            if f.len() != 14 {
                return Op::Bad;
            }
            match (
                ipx(f[4]),
                ipx(f[5]),
                px(f[6]),
                px(f[7]),
                ipx(f[8]),
                ipx(f[9]),
                px(f[10]),
                px(f[11]),
                px(f[12]),
            ) {
                (
                    Some(sx),
                    Some(sy),
                    Some(sw),
                    Some(sh),
                    Some(dx),
                    Some(dy),
                    Some(dw),
                    Some(dh),
                    Some(alpha),
                ) => Op::Blt {
                    dst: sid,
                    src_sid: f[2],
                    src_url: f[3],
                    sx,
                    sy,
                    sw,
                    sh,
                    dx,
                    dy,
                    dw,
                    dh,
                    alpha: alpha.min(255),
                    blend: f[13],
                },
                _ => Op::Bad,
            }
        }
        "gradientFillRect" => {
            if f.len() != 10 {
                return Op::Bad;
            }
            match (
                num(f[2]),
                num(f[3]),
                num(f[4]),
                num(f[5]),
                parse_rgba(f[6]),
                parse_rgba(f[7]),
                f[8],
            ) {
                (Some(x), Some(y), Some(width), Some(height), Some(from), Some(to), v) => {
                    Op::GradientRect {
                        sid,
                        x,
                        y,
                        width,
                        height,
                        from,
                        to,
                        vertical: v == "1",
                        blend: f[9],
                    }
                }
                _ => Op::Bad,
            }
        }
        "fillArc" => {
            if f.len() != 7 {
                return Op::Bad;
            }
            match (num(f[2]), num(f[3]), num(f[4]), parse_rgba(f[5])) {
                (Some(cx), Some(cy), Some(r), Some(rgba)) => Op::Arc {
                    sid,
                    cx,
                    cy,
                    r,
                    rgba,
                    blend: f[6],
                },
                _ => Op::Bad,
            }
        }
        "fillText" | "strokeText" => Op::NotYet { sid, action },
        "" => Op::Bad,
        other => Op::Other(other),
    }
}

/// 这一格操作属于哪一颗位图（每一条操作的第二个字段都是它，见 `shim_context.js`）。
pub fn target<'a>(op: &Op<'a>) -> Option<&'a str> {
    match op {
        Op::Size { sid, .. }
        | Op::NotYet { sid, .. }
        | Op::GradientRect { sid, .. }
        | Op::Arc { sid, .. } => Some(sid),
        Op::Blt { dst, .. } | Op::FillRect { dst, .. } | Op::ClearRect { dst, .. } => Some(dst),
        Op::Other(_) | Op::Bad => None,
    }
}

fn parse_rgba(value: &str) -> Option<[u32; 4]> {
    let mut out = [0u32; 4];
    for (i, part) in value.split(',').take(4).enumerate() {
        out[i] = part.trim().parse::<f64>().ok()?.round().max(0.0) as u32;
    }
    Some(out)
}

/// 一次回放的账：每一条没回放的动作都要数得出名字与次数。
#[derive(Debug, Default, Clone)]
pub struct Report {
    /// 落进像素的操作条数。
    pub applied: u32,
    /// 没回放的动作 → 次数（`fillText` 那一族）。
    pub not_yet: Vec<(String, u32)>,
    /// 认不下的混合值 → 次数（跳过，不假装画对）。
    pub bad_blend: Vec<(String, u32)>,
    /// 形状不认识 / 表外动作的条数。
    pub unparsed: u32,
}

impl Report {
    fn bump(list: &mut Vec<(String, u32)>, name: &str) {
        match list.iter_mut().find(|(k, _)| k == name) {
            Some((_, n)) => *n += 1,
            None => list.push((name.to_string(), 1)),
        }
    }

    /// 与另一段回放的账合并（同一颗位图可能被读很多次）。
    pub fn merge(&mut self, other: &Report) {
        self.applied += other.applied;
        self.unparsed += other.unparsed;
        for (name, n) in &other.not_yet {
            match self.not_yet.iter_mut().find(|(k, _)| k == name) {
                Some((_, m)) => *m += n,
                None => self.not_yet.push((name.clone(), *n)),
            }
        }
        for (name, n) in &other.bad_blend {
            match self.bad_blend.iter_mut().find(|(k, _)| k == name) {
                Some((_, m)) => *m += n,
                None => self.bad_blend.push((name.clone(), *n)),
            }
        }
    }
}

/// 一颗 CPU 画布：尺寸 + **预乘** RGBA8（与镜像同一个域，见模块头第 3 条口径）。
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Canvas {
    pub fn blank(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0u8; (width as usize) * (height as usize) * 4],
        }
    }

    pub fn from_image(img: &RgbaImage) -> Self {
        Self {
            width: img.width,
            height: img.height,
            pixels: img.pixels.clone(),
        }
    }

    pub fn into_image(self) -> RgbaImage {
        RgbaImage::new(self.width, self.height, self.pixels)
    }

    /// 单格读数（判据用；生产路径走 [`Canvas::into_image`] 交回 `RgbaImage`，那一颗自带 `pixel`）。
    #[cfg(test)]
    fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }

    /// 交回当前像素的一颗快照（`blt` 的源可能就是**正在合成的这一颗**，
    /// 上游 canvas 读的也是改之前的那一份 —— `blur` 那一格靠的就是这一手）。
    pub fn snapshot(&self) -> RgbaImage {
        RgbaImage::new(self.width, self.height, self.pixels.clone())
    }
}

/// 问源像素的那只手：`(位图号, 编码过的 URL)` → 那一份像素。
/// 交 `None` = 这一张既没合成过也没有镜像（或循环引用）⇒ 上游在这一格画的就是透明。
pub type Source<'a> = dyn Fn(&str, &str) -> Option<Rc<RgbaImage>> + 'a;

/// 把 `ops` 里属于 `sid` 的那几条回放进 `canvas`。
///
/// **`size` 一律是"改尺寸 + 清空"** —— 上游的两条赋值腿（`_createCanvas` 的 `:758-765` 与逐字的
/// `resize`）都是这个效果；至于"那张素材图的像素从哪来"，是 `BITMAP_HOST_JS::_createCanvas` 收尾
/// 那一条**具名操作**（上游在那一格写的就是 `this.__context.drawImage(this._image, 0, 0)`，`:768`），
/// 不是这一层的特例。所以操作流是唯一的真相：回放它 == 上游画过的东西。
///
/// 交回的第二个数是"这一段里见过 `size` 吗" —— 没见过就意味着这一颗位图**根本没有画布**，
/// 调用方据此交回"没有源"而不是"一片空画布"（两种读数不能混）。
pub fn replay(sid: &str, ops: &[String], canvas: &mut Canvas, source: &Source) -> (Report, bool) {
    let mut report = Report::default();
    let mut sized = false;
    for spec in ops {
        let op = parse(spec);
        // 契约坏掉的两类**不按位图过滤**：它们意味着操作表脱钩，多算一次比漏算好查。
        match &op {
            Op::Bad => {
                report.unparsed += 1;
                continue;
            }
            Op::Other(name) => {
                report.unparsed += 1;
                Report::bump(&mut report.not_yet, name);
                continue;
            }
            _ => {}
        }
        if target(&op) != Some(sid) {
            continue;
        }
        match op {
            Op::Size { width, height, .. } => {
                canvas.width = width;
                canvas.height = height;
                canvas.pixels = vec![0u8; (width as usize) * (height as usize) * 4];
                sized = true;
                report.applied += 1;
            }
            Op::ClearRect {
                x,
                y,
                width,
                height,
                ..
            } => {
                // `clearRect` 是**抹掉**而不是叠一层透明：source-over 的公式在 srcA=0 时是恒等的
                // （`out = dst×1`），拿它当抹除会得到"什么都没清"的假动作。
                erase(canvas, x, y, width, height);
                report.applied += 1;
            }
            Op::FillRect {
                x,
                y,
                width,
                height,
                rgba,
                blend,
                ..
            } => {
                if !note_blend(&mut report, blend) {
                    continue;
                }
                // 交进来的颜色是**非预乘**的（`_color` 那一格算的是 CSS 的那一份 alpha，0..255 的
                // 整数），而画布存预乘 ⇒ 与 `crates/sisf-media/src/image.rs:112-124` 同一条转换。
                let a = rgba[3];
                let premul = [
                    (rgba[0] * a + 127) / 255,
                    (rgba[1] * a + 127) / 255,
                    (rgba[2] * a + 127) / 255,
                    a,
                ];
                fill(canvas, x, y, width, height, premul, blend == "source-over");
                report.applied += 1;
            }
            Op::Blt {
                src_sid,
                src_url,
                sx,
                sy,
                sw,
                sh,
                dx,
                dy,
                dw,
                dh,
                alpha,
                blend,
                ..
            } => {
                if !note_blend(&mut report, blend) {
                    continue;
                }
                let lighter = blend == "lighter";
                // 源读不到 ⇒ 上游在这一格画的也是透明（不是"这一笔丢了"）。
                let Some(img) = source(src_sid, src_url) else {
                    continue;
                };
                blt(canvas, &img, sx, sy, sw, sh, dx, dy, dw, dh, alpha, lighter);
                report.applied += 1;
            }
            Op::GradientRect {
                x,
                y,
                width,
                height,
                from,
                to,
                vertical,
                blend,
                ..
            } => {
                if !note_blend(&mut report, blend) {
                    continue;
                }
                gradient(
                    canvas,
                    x,
                    y,
                    width,
                    height,
                    &from,
                    &to,
                    vertical,
                    blend == "source-over",
                );
                report.applied += 1;
            }
            Op::Arc {
                cx,
                cy,
                r,
                rgba,
                blend,
                ..
            } => {
                if !note_blend(&mut report, blend) {
                    continue;
                }
                disc(canvas, cx, cy, r, &rgba, blend == "source-over");
                report.applied += 1;
            }
            Op::NotYet { action, .. } => {
                Report::bump(&mut report.not_yet, action);
            }
            Op::Other(_) | Op::Bad => unreachable!("上面两条已经拦掉了"),
        }
    }
    (report, sized)
}

/// 混合值认得下吗？认不下就记一笔（并让这一条操作**不落像素**）。
fn note_blend(report: &mut Report, blend: &str) -> bool {
    if BLENDS.contains(&blend) {
        return true;
    }
    Report::bump(&mut report.bad_blend, blend);
    false
}

/// `(c * a + 127) / 255` 那一格整数舍入，全文件只用这一支笔（与解码那侧同一条预乘公式）。
fn mul(a: u32, b: u32) -> u32 {
    (a * b + 127) / 255
}

fn fill(
    canvas: &mut Canvas,
    x: i64,
    y: i64,
    width: u32,
    height: u32,
    premul: [u32; 4],
    over: bool,
) {
    for dy in 0..height as i64 {
        for dx in 0..width as i64 {
            let (px, py) = (x + dx, y + dy);
            if px < 0 || py < 0 || px >= canvas.width as i64 || py >= canvas.height as i64 {
                continue;
            }
            canvas.blend_at(px as u32, py as u32, premul, over);
        }
    }
}

/// `clearRect` 那一格：把矩形**抹平**（预乘域里的"什么都没有"就是四个 0）。
fn erase(canvas: &mut Canvas, x: i64, y: i64, width: u32, height: u32) {
    for dy in 0..height as i64 {
        let py = y + dy;
        if py < 0 || py >= canvas.height as i64 {
            continue;
        }
        for dx in 0..width as i64 {
            let px = x + dx;
            if px < 0 || px >= canvas.width as i64 {
                continue;
            }
            let i = ((py as u32 * canvas.width + px as u32) * 4) as usize;
            canvas.pixels[i..i + 4].fill(0);
        }
    }
}

impl Canvas {
    /// 一格合成：`source-over` 是 `out = src + dst×(1−srcA)`，`lighter` 是预乘相加夹到 255。
    fn blend_at(&mut self, x: u32, y: u32, src: [u32; 4], over: bool) {
        let i = ((y * self.width + x) * 4) as usize;
        for c in 0..4 {
            let d = self.pixels[i + c] as u32;
            let s = src[c];
            self.pixels[i + c] = if over {
                (s + mul(d, 255 - src[3])).min(255) as u8
            } else {
                (s + d).min(255) as u8
            };
        }
    }
}

/// `gradientFillRect`：两格停靠点之间的线性插值。
///
/// **插值发生在预乘 sRGB 字节域** —— canvas 2D 的渐变就是这个域（停靠点先各自预乘，再逐通道插）。
/// 这一档与 AGENTS 咬人第 29 条⑸ 那条"移植侧的混合在线性光里"**不是同一件事**：那一条说的是 GPU
/// 把 sRGB 纹理解成线性再混，而 CPU 这一整条腿自始至终是 sRGB 数值，与上游 canvas 同一档。
/// 方向与跨度照上游那两句（`vertical` ⇒ `(x,y)→(x,y+h)`，否则 `(x,y)→(x+w,y)`，`rpg_core.js:1286-1290`），
/// 而 `t` 取**像素中心**（`(p + 0.5 − 起点) / 跨度`，夹回 `[0,1]`）—— 拿像素左上角量就会让第一格
/// 直接等于停靠点 0，与上游差半格。
/// 九格参数 = 那一条操作自己的字段数（矩形四格 + 两格停靠点 + 方向 + 混合），
/// 拆开就要在调用点外面再造一颗结构体去装同一条串 —— 与 `blt` 同一格处理。
#[allow(clippy::too_many_arguments)]
fn gradient(
    canvas: &mut Canvas,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    from: &[u32; 4],
    to: &[u32; 4],
    vertical: bool,
    over: bool,
) {
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let premul = |c: &[u32; 4]| [mul(c[0], c[3]), mul(c[1], c[3]), mul(c[2], c[3]), c[3]];
    let (a, b) = (premul(from), premul(to));
    let (span, origin) = if vertical { (height, y) } else { (width, x) };
    let (gx0, gy0) = (x.floor() as i64, y.floor() as i64);
    for dy in 0..height.ceil() as i64 {
        for dx in 0..width.ceil() as i64 {
            let (px, py) = (gx0 + dx, gy0 + dy);
            if px < 0 || py < 0 || px >= canvas.width as i64 || py >= canvas.height as i64 {
                continue;
            }
            let p = if vertical { py as f64 } else { px as f64 };
            let t = ((p + 0.5 - origin) / span).clamp(0.0, 1.0);
            let at = |c: usize| (a[c] as f64 + (b[c] as f64 - a[c] as f64) * t + 0.5) as u32;
            canvas.blend_at(px as u32, py as u32, [at(0), at(1), at(2), at(3)], over);
        }
    }
}

/// `fillArc`（上游 `drawCircle` 的 `arc` + `fill`）：一颗实心圆盘。
///
/// 边缘给**一格宽的覆盖率抗锯齿**（`cover = clamp(0.5 − (|p−c| − r), 0, 1)`，乘在**预乘**色上）。
/// 这一档是明写的近似，不是"上游一模一样"：浏览器按解析覆盖率抗锯齿，而这里量的是像素中心的
/// 符号距离 —— 差在最外那一格的半格以内。为什么值得做而不是硬边：`drawCircle` 在这份工程里画的是
/// **雨点与雪**（`1×60`/`2×100`/`9×9` 那三颗写死尺寸的位图，半径 2 与 4），硬边会让小半径那一档
/// 整颗大一圈（半径 2 ⇒ 13 格 vs 上游约 12.6 格的覆盖）。读数由判据钉。
fn disc(canvas: &mut Canvas, cx: f64, cy: f64, r: f64, rgba: &[u32; 4], over: bool) {
    if r <= 0.0 {
        return;
    }
    let a = rgba[3];
    let solid = [mul(rgba[0], a), mul(rgba[1], a), mul(rgba[2], a), a];
    let (x0, y0) = ((cx - r).floor() as i64, (cy - r).floor() as i64);
    let (x1, y1) = ((cx + r).ceil() as i64, (cy + r).ceil() as i64);
    for py in y0..y1 {
        for px in x0..x1 {
            if px < 0 || py < 0 || px >= canvas.width as i64 || py >= canvas.height as i64 {
                continue;
            }
            let d = ((px as f64 + 0.5 - cx).powi(2) + (py as f64 + 0.5 - cy).powi(2)).sqrt() - r;
            let cover = (0.5 - d).clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }
            let at = |c: usize| (solid[c] as f64 * cover + 0.5) as u32;
            canvas.blend_at(px as u32, py as u32, [at(0), at(1), at(2), at(3)], over);
        }
    }
}

/// 一次 `blt`：源矩形 → 目标矩形，**nearest** 采样 + `globalAlpha`（已经在那一格乘过）+ 混合。
#[allow(clippy::too_many_arguments)]
fn blt(
    canvas: &mut Canvas,
    img: &RgbaImage,
    sx: i64,
    sy: i64,
    sw: u32,
    sh: u32,
    dx: i64,
    dy: i64,
    dw: u32,
    dh: u32,
    alpha: u32,
    lighter: bool,
) {
    if dw == 0 || dh == 0 || sw == 0 || sh == 0 {
        return;
    }
    for oy in 0..dh as i64 {
        for ox in 0..dw as i64 {
            let (px, py) = (dx + ox, dy + oy);
            if px < 0 || py < 0 || px >= canvas.width as i64 || py >= canvas.height as i64 {
                continue;
            }
            // nearest：目标第 k 格取源的第 `floor(k * sh / dh)` 行（与引擎的
            // `ScaleMode::IntegerFit` 那条口径同源；上游 `_smooth` false ⇒ 本就没有插值）。
            let ux = sx + (ox * sw as i64 / dw as i64);
            let uy = sy + (oy * sh as i64 / dh as i64);
            if ux < 0 || uy < 0 || ux >= img.width as i64 || uy >= img.height as i64 {
                continue;
            }
            let s = img.pixel(ux as u32, uy as u32);
            let mut premul = [s[0] as u32, s[1] as u32, s[2] as u32, s[3] as u32];
            if alpha != 255 {
                for c in premul.iter_mut() {
                    *c = mul(*c, alpha);
                }
            }
            canvas.blend_at(px as u32, py as u32, premul, !lighter);
        }
    }
}

#[cfg(all(test, feature = "js"))]
mod tests {
    use super::*;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    fn img_2x2() -> Rc<RgbaImage> {
        let bytes = crate::raster::sample_png_2x2();
        let decoded = sisf_media::image::decode_image(&bytes).unwrap();
        Rc::new(decoded.first().clone())
    }

    fn source_of(img: Rc<RgbaImage>) -> impl Fn(&str, &str) -> Option<Rc<RgbaImage>> {
        move |_, _| Some(Rc::clone(&img))
    }

    fn none_source() -> impl Fn(&str, &str) -> Option<Rc<RgbaImage>> {
        |_, _| None
    }

    #[test]
    fn the_action_table_is_split_exactly_once_between_arithmetic_and_rest() {
        // 操作表与合成器不许脱钩：表里每一条的形状都按 `shim_context.js` 的实际拼接对一遍。
        assert_eq!(
            parse("size|7|2|2|img%2Fa.png"),
            Op::Size {
                sid: "7",
                width: 2,
                height: 2,
                url: "img%2Fa.png"
            }
        );
        assert_eq!(
            parse("clearRect|7|1|2|3|4"),
            Op::ClearRect {
                dst: "7",
                x: 1,
                y: 2,
                width: 3,
                height: 4
            }
        );
        assert_eq!(
            parse("gradientFillRect|7|0|0|6|4|255,255,255,255|0,0,0,255|1|source-over"),
            Op::GradientRect {
                sid: "7",
                x: 0.0,
                y: 0.0,
                width: 6.0,
                height: 4.0,
                from: [255, 255, 255, 255],
                to: [0, 0, 0, 255],
                vertical: true,
                blend: "source-over"
            },
            "停靠点两格 + 方向那一名（`shim_context.js` 的 `_gradientFill`，`vertical` 是 `1`/`0`）"
        );
        assert_eq!(
            parse("fillArc|7|3|2|2|0,0,0,255|source-over"),
            Op::Arc {
                sid: "7",
                cx: 3.0,
                cy: 2.0,
                r: 2.0,
                rgba: [0, 0, 0, 255],
                blend: "source-over"
            },
            "圆心与半径（`arc` + `fill` 合成出来的那一名）"
        );
        // 字段数就是契约：少一格与多一格都点名，不猜哪一格缺了。
        assert_eq!(
            parse("gradientFillRect|7|0|0|6|4|255,255,255,255|0,0,0,255|1"),
            Op::Bad
        );
        assert_eq!(parse("fillArc|7|3|2|0,0,0,255|source-over"), Op::Bad);
        assert_eq!(
            parse("fillRect|7|1|2|3|4|255,0,0,128|source-over"),
            Op::FillRect {
                dst: "7",
                x: 1,
                y: 2,
                width: 3,
                height: 4,
                rgba: [255, 0, 0, 128],
                blend: "source-over"
            }
        );
        // 验收里那条真的 `blt` 读数（`M10.md` 第四格第二小格）：14 格字段。
        assert_eq!(
            parse("blt|2|1||1|2|4|3|5|6|4|3|255|source-over"),
            Op::Blt {
                dst: "2",
                src_sid: "1",
                src_url: "",
                sx: 1,
                sy: 2,
                sw: 4,
                sh: 3,
                dx: 5,
                dy: 6,
                dw: 4,
                dh: 3,
                alpha: 255,
                blend: "source-over"
            }
        );
        for action in NOT_SYNTHESIZED {
            assert_eq!(
                parse(&format!("{action}|7|1|2|3|4|5|6|7|8|9|0|1|2|3")),
                Op::NotYet { sid: "7", action },
                "{action} 必须点名不回放，而不是静默落一格像素"
            );
        }
        assert_eq!(parse("mystery|7"), Op::Other("mystery"));
        // 少一格的坏形状 ⇒ 点名，不猜。
        assert_eq!(parse("fillRect|7|1|2|3|4|255,0,0,128"), Op::Bad);
        assert_eq!(parse(""), Op::Bad);
    }

    #[test]
    fn a_blt_lands_the_source_pixels_with_the_upstream_alpha() {
        let mut canvas = Canvas::blank(4, 2);
        let ops = vec![
            "size|1|4|2|".to_string(),
            "blt|1|2||0|0|2|2|0|0|2|2|255|source-over".to_string(),
        ];
        let (report, _) = replay("1", &ops, &mut canvas, &source_of(img_2x2()));
        assert_eq!(report.applied, 2, "size + 一条 blt");
        assert_eq!(canvas.pixel(0, 0), [255, 0, 0, 255]);
        assert_eq!(canvas.pixel(1, 0), [0, 255, 0, 255]);
        assert_eq!(canvas.pixel(0, 1), [0, 0, 255, 255]);
        // 半透明那一颗是**预乘**进来的（镜像同域），与 `sample_png_2x2` 的第四格一致。
        assert_eq!(canvas.pixel(1, 1), [64, 32, 16, 128]);
        // 画布剩下的那一半没被动过。
        assert_eq!(canvas.pixel(3, 1), [0, 0, 0, 0]);

        // globalAlpha 128 那一档：每一格乘过去（上游 `blt` 之前先写 `globalAlpha`）。
        let mut faded = Canvas::blank(2, 2);
        let faded_ops = vec![
            "size|1|2|2|".to_string(),
            "blt|1|2||0|0|2|2|0|0|2|2|128|source-over".to_string(),
        ];
        replay("1", &faded_ops, &mut faded, &source_of(img_2x2()));
        assert_eq!(
            faded.pixel(0, 0),
            [128, 0, 0, 128],
            "预乘的 alpha 乘两格：255×128 ⇒ 128 而 alpha 255×128 ⇒ 128"
        );
    }

    #[test]
    fn a_scaled_blt_picks_the_nearest_source_cell_because_smooth_is_false() {
        // 上游 `_refreshBack` 的第一笔就是这种形状：96×96 的源贴到比它大的目标。
        // 这里用 2×2 → 4×4：每一格源像素撑成 2×2 的一块（nearest，无插值）。
        let mut canvas = Canvas::blank(4, 4);
        let ops = vec![
            "size|1|4|4|".to_string(),
            "blt|1|2||0|0|2|2|0|0|4|4|255|source-over".to_string(),
        ];
        let (report, _) = replay("1", &ops, &mut canvas, &source_of(img_2x2()));
        assert_eq!(report.applied, 2);
        assert_eq!(canvas.pixel(0, 0), [255, 0, 0, 255]);
        assert_eq!(canvas.pixel(1, 0), [255, 0, 0, 255]);
        assert_eq!(canvas.pixel(2, 0), [0, 255, 0, 255]);
        assert_eq!(canvas.pixel(2, 2), [64, 32, 16, 128]);
    }

    #[test]
    fn a_fill_rect_premultiplies_and_a_clear_rect_erases() {
        let mut canvas = Canvas::blank(2, 2);
        // `_color` 交出来的 alpha 是 **0..255 的整数**（`Math.round(rgba[3]*255*globalAlpha)`），
        // 而画布存预乘 ⇒ 落笔前先乘一次 alpha（AGENTS 咬人第 25 条⑵同一条腿）。
        let ops = vec![
            "size|1|2|2|".to_string(),
            "fillRect|1|0|0|2|2|255,0,0,128|source-over".to_string(),
        ];
        let (report, _) = replay("1", &ops, &mut canvas, &none_source());
        assert_eq!(report.applied, 2);
        assert_eq!(
            canvas.pixel(0, 0),
            [128, 0, 0, 128],
            "(255×128+127)/255 = 128 ⇒ 预乘的那一格"
        );

        // 叠在已有像素上：`out = src + dst×(1−srcA)`（不除回，因为它就在预乘域里）。
        let mut over = Canvas::blank(1, 1);
        let over_ops = vec![
            "size|1|1|1|".to_string(),
            "fillRect|1|0|0|1|1|255,0,0,255|source-over".to_string(),
            "fillRect|1|0|0|1|1|0,0,255,128|source-over".to_string(),
        ];
        replay("1", &over_ops, &mut over, &none_source());
        assert_eq!(
            over.pixel(0, 0),
            [127, 0, 128, 255],
            "红被半透明蓝压住：255×(255−128)/255 = 127，蓝落 128，alpha 合起来满 255"
        );

        // `lighter` 是预乘相加（`blur` 那一格要的就是这一手）。
        let mut add = Canvas::blank(1, 1);
        let add_ops = vec![
            "size|1|1|1|".to_string(),
            "fillRect|1|0|0|1|1|200,0,0,255|source-over".to_string(),
            "fillRect|1|0|0|1|1|0,100,0,255|lighter".to_string(),
        ];
        replay("1", &add_ops, &mut add, &none_source());
        assert_eq!(add.pixel(0, 0), [200, 100, 0, 255]);

        // clearRect 抹回透明。
        let cleared = vec![
            "size|1|1|1|".to_string(),
            "fillRect|1|0|0|1|1|255,255,255,255|source-over".to_string(),
            "clearRect|1|0|0|1|1".to_string(),
        ];
        let mut cm = Canvas::blank(1, 1);
        replay("1", &cleared, &mut cm, &none_source());
        assert_eq!(cm.pixel(0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn anything_the_compositor_cannot_draw_is_named_and_counted() {
        let mut canvas = Canvas::blank(2, 2);
        let ops = vec![
            "size|1|2|2|".to_string(),
            "fillText|1|%E4%BD%A0|0|0||GameFont|28|0|start|alphabetic|255,255,255,1|1|source-over"
                .to_string(),
            "strokeText|1|%E5%A5%BD|0|0||GameFont|28|0|start|alphabetic|255,255,255,1|1|source-over"
                .to_string(),
            "fillRect|1|0|0|2|2|255,0,0,1|difference".to_string(),
        ];
        let (report, _) = replay("1", &ops, &mut canvas, &none_source());
        assert_eq!(report.applied, 1, "只有 size 落了像素");
        let names: Vec<&str> = report.not_yet.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, ["fillText", "strokeText"]);
        assert_eq!(report.bad_blend, [("difference".to_string(), 1)]);
        assert_eq!(
            canvas.pixel(0, 0),
            [0, 0, 0, 0],
            "认不下的混合值一律不落像素"
        );
    }

    /// **渐变落在预乘字节域、按像素中心插值**（第二小格第一档）。
    ///
    /// 垂直那一格钉三件事：⑴ 沿 x **一行内四格相等**（渐变轴是 y，垂直方向必须恒定 —— 拿
    /// "按对角插"会得到每格不同）；⑵ `t` 是**像素中心** ⇒ 第一行不等于停靠点 0（`255` 而是 `223`），
    /// 最后一行也不等于停靠点 1（`32` 而不是 `0`）；⑶ 停靠点**先各自预乘再插**（第二格里
    /// `255,0,0,128` 与 `0,0,255,255` 的中点：alpha `191`、蓝通道不是 `255/2` 而是预乘域的那一份）。
    #[test]
    fn a_gradient_interpolates_premultiplied_bytes_at_pixel_centers() {
        let mut canvas = Canvas::blank(4, 4);
        let ops = vec![
            "size|1|4|4|".to_string(),
            "gradientFillRect|1|0|0|4|4|255,255,255,255|0,0,0,255|1|source-over".to_string(),
        ];
        let (report, _) = replay("1", &ops, &mut canvas, &none_source());
        assert_eq!(report.applied, 2, "size + 一条渐变");
        assert_eq!(
            (
                canvas.pixel(0, 0),
                canvas.pixel(3, 0),
                canvas.pixel(0, 3),
                canvas.pixel(1, 1),
                canvas.pixel(2, 2)
            ),
            (
                [223, 223, 223, 255],
                [223, 223, 223, 255],
                [32, 32, 32, 255],
                [159, 159, 159, 255],
                [96, 96, 96, 255]
            ),
            "同一行四格相等、逐行按 1/4 递减（读数即 `t = (y+0.5)/4`）"
        );
        // 停靠点先预乘再插：`255,0,0,128` 的预乘是 `(128,0,0,128)`（`(255*128+127)/255`）。
        let mut half = Canvas::blank(2, 1);
        let ops = vec![
            "size|1|2|1|".to_string(),
            "gradientFillRect|1|0|0|2|1|255,0,0,128|0,0,255,255|0|source-over".to_string(),
        ];
        replay("1", &ops, &mut half, &none_source());
        assert_eq!(
            (half.pixel(0, 0), half.pixel(1, 0)),
            ([96, 0, 64, 160], [32, 0, 191, 223]),
            "两格分别是 `t = 0.25` 与 `t = 0.75` 处**预乘停靠点**的插值：停靠点 0 的预乘那份是\
             `(128,0,0,128)`（不是 `255,0,0,128` 那一份），停靠点 1 是 `(0,0,255,255)` ⇒\
             `0.75×(128,0,0,128) + 0.25×(0,0,255,255) = (96,0,64,160)`。alpha 也在同一个域里插\
             （`160 = 0.75×128 + 0.25×255`），所以半透明的停靠点不会在中途浮出一条不该有的边"
        );
    }

    /// **圆是一颗带一格抗锯齿的实心盘**（第二小格第二档；读数钉的是近似的那一档）。
    ///
    /// 拿上游 `drawCircle` 真在天气那三颗位图上画的那一格量：半径 4 落在 `(4,4)` 的 9×9。
    /// 完全在内的（符号距离 ≤ −0.5）交整色，完全在外的（≥ +0.5）交 `0`，最外那一圈按覆盖率
    /// 缩放预乘色 —— 这一档与浏览器的解析覆盖率差在最外那一格的半格以内，`M10.md` 明写着记账。
    #[test]
    fn a_disc_covers_the_disk_with_a_one_pixel_feather() {
        let mut canvas = Canvas::blank(9, 9);
        let ops = vec![
            "size|1|9|9|".to_string(),
            "fillArc|1|4|4|4|255,255,255,255|source-over".to_string(),
        ];
        let (report, _) = replay("1", &ops, &mut canvas, &none_source());
        assert_eq!(report.applied, 2);
        assert_eq!(canvas.pixel(4, 4), [255, 255, 255, 255], "圆心整格");
        assert_eq!(canvas.pixel(0, 0), [0, 0, 0, 0], "对角在盘外");
        assert_eq!(
            canvas.pixel(1, 4),
            [255, 255, 255, 255],
            "水平方向内一格整格"
        );
        // 白色不透明 ⇒ 每格的 alpha 就是那一格的覆盖率（255 = 整格），加起来除以 255 就是格数。
        let total: u32 = (0..9)
            .flat_map(|y| (0..9).map(move |x| (x, y)))
            .map(|(x, y)| canvas.pixel(x, y)[3] as u32)
            .sum();
        let cells = total as f64 / 255.0;
        assert!(
            (cells - std::f64::consts::PI * 16.0).abs() < 1.0,
            "覆盖总量对得上 πr² = {:.2} 格（实量 {cells:.2}）—— 硬边那一档会浮到 61 格（半径 4 的\
             方块），差的就是最外那一圈",
            std::f64::consts::PI * 16.0
        );
        // 半格那一圈必须是**部分**覆盖（不是整格就是零 = 硬边）。
        let edge = canvas.pixel(7, 4);
        assert!(
            edge[3] > 0 && edge[3] < 255,
            "半径边缘那一格要读成部分覆盖：{edge:?}"
        );
    }

    #[test]
    fn a_size_always_clears_and_a_loaded_image_arrives_as_its_own_blt() {
        // `_createCanvas` 与逐字的 `resize` 在操作上都是「改尺寸 + 清空」（上游给
        // `_canvas.width` 赋值就是这一效果）。那张素材图的像素不是 `size` 的特例，而是
        // `_createCanvas` 收尾**自己那一条 `drawImage(this._image, 0, 0)`**
        // （`rpg_core.js:768`）落成的具名操作。
        let created = vec![
            "size|1|2|2|img%2Fsystem%2FWindow.png".to_string(),
            "blt|1|0|img%2Fsystem%2FWindow.png|0|0|2|2|0|0|2|2|255|source-over".to_string(),
        ];
        let mut canvas = Canvas::blank(1, 1);
        let (report, sized) = replay("1", &created, &mut canvas, &source_of(img_2x2()));
        assert!(
            sized,
            "见过 size ⇒ 这一颗有画布；它与「根本没有画布」是两种读数"
        );
        assert_eq!(report.applied, 2);
        assert_eq!(canvas.width, 2);
        assert_eq!(canvas.pixel(0, 0), [255, 0, 0, 255]);
        assert_eq!(canvas.pixel(1, 1), [64, 32, 16, 128]);

        // 之后 resize：清空，图不会自己回来（逐字那一格没有回贴那一手）。
        let grown = vec![
            created[0].clone(),
            created[1].clone(),
            "size|1|3|1|img%2Fsystem%2FWindow.png".to_string(),
        ];
        let mut cleared = Canvas::blank(1, 1);
        replay("1", &grown, &mut cleared, &source_of(img_2x2()));
        assert_eq!(cleared.width, 3);
        assert_eq!(cleared.height, 1);
        assert_eq!(cleared.pixel(0, 0), [0, 0, 0, 0]);

        // 一条 `size` 都没见过 ⇒ `sized` false ⇒ 调用方交回「没有源」而不是「一片空画布」。
        let mut naked = Canvas::blank(2, 2);
        let (report, sized) = replay(
            "1",
            &["fillRect|1|0|0|1|1|255,0,0,255|source-over".to_string()],
            &mut naked,
            &none_source(),
        );
        assert!(!sized);
        assert_eq!(report.applied, 1);
    }

    #[test]
    fn the_replay_only_touches_the_bitmap_it_is_building() {
        let mut canvas = Canvas::blank(2, 2);
        let ops = vec![
            "size|9|8|8|".to_string(),
            "fillRect|9|0|0|8|8|255,0,0,1|source-over".to_string(),
            "size|1|2|2|".to_string(),
            "clearRect|1|0|0|1|1".to_string(),
        ];
        let (report, _) = replay("1", &ops, &mut canvas, &none_source());
        assert_eq!(report.applied, 2, "9 号那两条一条都不许落进 1 号");
    }

    #[test]
    fn a_source_the_host_cannot_answer_paints_nothing_instead_of_throwing() {
        // 合成器**不问磁盘**：像素全部由 `(操作流, 源解析器)` 决定 ⇒ 它可以搬去任何一条腿跑
        // （宿主那侧的真绘制要复用同一套算术）。解析器交 `None` 就是"这一格没有源像素"，
        // 上游 canvas 在那里画的也是透明 —— 不抛、也不假装读到黑。
        let mut canvas = Canvas::blank(1, 1);
        let ops = vec![
            "size|1|1|1|".to_string(),
            "blt|1|0||0|0|1|1|0|0|1|1|255|source-over".to_string(),
        ];
        let (report, _) = replay("1", &ops, &mut canvas, &none_source());
        assert_eq!(
            report.applied, 1,
            "size 落了，blt 因为没源不落像素（也不算错）"
        );
        assert_eq!(canvas.pixel(0, 0), [0, 0, 0, 0]);
    }
}
