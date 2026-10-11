//! **画的那一条腿的契约**（M10.5 第四格第二小格，2026-10-05）。
//!
//! `shim_context`（`src/js/shim_context.js`）那一层把 MV 的每一笔画**当场**兑现成
//! 一条具名操作，交到这里；这里今天交的实现是**记录器**。为什么不画：
//! `&mut Renderer` 只在 `App::draw` 那一拍借得到（`crates/sisf-app/src/app.rs:36` 的
//! `trait App { fn draw(&mut self, alpha, renderer: &mut Renderer); }` 是引擎唯一的公开把手，
//! `Runner` 把 renderer 收在私有 state 里 `crates/sisf-app/src/runner.rs:55,213-256`），
//! 而 JS 的调用点在 `eval` 那一刻。所以这一族的形状只能是"**当场记、绘制那一拍执行**"，
//! 而宿主换绑 `PAINT_GLOBAL` 就是换实现（`jsrt::Host::bind_str` 两条腿都能重复挂同一个名字）。
//!
//! 三条口径：
//!
//! 1. **一颗字符串进、一颗字符串出的契约**。操作串是 `<动作>|<字段>…`，字段里的自由文本
//!    （URL、正文）一律 `encodeURIComponent` 过 —— `|` 与换行都可能出现在台词与素材名里
//!    （AGENTS 咬人第 4 条：数据里的名字有全角、空格、`\|` 转义），不编码就会把一条操作劈成两条。
//! 2. **读不到像素的那一档要说出来，而且要说在哪一格**。`PIXELS_GLOBAL` 有两种回答：有 CPU
//!    像素源的那一张（文件载入的镜像，或写过一次的位图）交逐格读数；没有源的那一张（**画出来的**：
//!    内容只活在操作流里）交 `blank|<w>|<h>`，JS 那一层据此就地造一片透明黑并**只回一条 `blind|<sid>`**
//!    —— 两头的次数都由 `BLIND_GLOBAL` 交回判据数（`M10.md`「第四格第三小格」的边界⑵：色调叠在
//!    一片零上就是"这一格的合成还没到"，那一格的名字是**软件合成**，不许把它读成"色调已生效"）。
//!    为什么没有源：`Renderer::readback` 只读主离屏目标、而且是整张
//!    （`crates/sisf-render/src/renderer.rs:800-855`），引擎也不给位图目标留就地改像素的把手。
//!    文字宽度同一档（宿主没绑测量器 ⇒ `undelivered:measure`），因为"每个字符一样宽"是会被窗口
//!    宽度照抄的假数。
//! 3. **操作流是判据钉的东西**。两条腿（native QuickJS 与 wasm/node 共享 realm）都数得到同一条串；
//!    屏幕像素不在这里证 —— 那要真的绘制会话，落在宿主那一侧（`M10.md` 的任务表里是
//!    「`SceneManager` 主循环 + pray 侧兼容腿 harness」与「容器树」两格）。

use crate::composite::{self, Canvas};
use crate::raster::Rasters;
use sisf_media::image::RgbaImage;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

/// JS 那一层交每一条画的动作时喊的名字（宿主换绑它就是换实现）。
pub const PAINT_GLOBAL: &str = "__sisf_paint";
/// 清掉操作流（每条判据自己那段动作之前喊一次 —— 共享 realm 里"上一个判据的操作"会污染读数）。
pub const PAINT_RESET_GLOBAL: &str = "__sisf_paint_reset";
/// 读出今天记下的操作流（判据用它；宿主换成真绘制之后这一格照样能读，只是会空）。
pub const PAINT_OPS_GLOBAL: &str = "__sisf_paint_ops";
/// `getImageData` 那一条腿（位图自己的副本优先，其次 CPU 镜像；两个都没有 ⇒ `blank|<w>|<h>`）。
pub const PIXELS_GLOBAL: &str = "__sisf_pixels";
/// `putImageData` 那一条腿（第一次写会**为这一颗位图复制一份副本**；没有源 ⇒ 只计数）。
pub const PUT_PIXELS_GLOBAL: &str = "__sisf_put_pixels";
/// `measureText` 那一条腿（只有宿主的 `TextEngine::layout` 答得上）。
pub const MEASURE_GLOBAL: &str = "__sisf_measure_text";
/// **盲区计数**那一条腿：`"<读的次数>,<丢的写次数>"`。这两个数就是"软件合成那一格还没到"的读数
/// —— 判据钉它，而不是让色调静默地叠在一片零上。
pub const BLIND_GLOBAL: &str = "__sisf_pixel_blind";
/// **合成的账**那一条腿：落了像素的条数、没回放的动作（逐名带次数）、认不下的混合值（逐名）、
/// 被预算拒掉的合成、被挤掉的画布。这一格是"操作流与合成器脱钩"的唯一运行时警报
/// （另一头是尺子 `software-composite.py` 的第①段）。
pub const COMPOSITE_GLOBAL: &str = "__sisf_composite_report";

/// 读不到时交回的哨兵前缀（JS 那一层据此**抛**，见模块头的第 2 条口径）。
pub const UNDELIVERED: &str = "undelivered";

/// 记录器：今天每一条画都只进这一条 `Vec`，而**有人读像素时**才把它回放成像素
/// （[`crate::composite`]，M10.5 第四格第四小格）。
pub struct Recorder {
    rasters: Rc<Rasters>,
    ops: RefCell<Vec<String>>,
    /// **没有像素源**的那一张被问/被写的次数（`(读, 写)`，见模块头第 2 条口径）。
    blind: RefCell<(u32, u32)>,
    /// 每一颗位图**回放到第几条操作**了（画布被挤掉时这一格一起丢 —— 重放是从头再来）。
    cursors: RefCell<HashMap<String, usize>>,
    /// 正在合成的那几格：`blt` 的源就是自己那一族时不再递归。
    building: RefCell<Vec<String>>,
    /// 合成器**没回放**的动作 → 次数（文字、渐变、圆，以及表外漂进来的新动作）。
    not_yet: RefCell<BTreeMap<String, u32>>,
    /// 认不下的混合值 → 次数（跳过这一笔，不假装画对）。
    bad_blend: RefCell<BTreeMap<String, u32>>,
    /// 画布超预算而一条都挤不出来 ⇒ 那一次合成被拒的次数。
    refused: RefCell<u32>,
    /// 被挤掉的合成画布数（可重放 ⇒ 不丢正确性，但这一格要看得见）。
    evictions: RefCell<u32>,
    /// 落进像素的操作条数（合成的总账）。
    applied: RefCell<u32>,
    /// 宿主换上的**字形源**（[`Recorder::set_text_source`]，M10.5「字形落像素」）：接上之前
    /// `fillText` 与 [`crate::composite::NOT_SYNTHESIZED`] 同路点名，接上之后它落进合成像素。
    /// 字体字节住在宿主那一侧（`bundled_fonts`），所以这一格只能注入、不能在 crate 里自己引字体。
    text_source: RefCell<Option<Rc<dyn crate::composite::TextRasterSource>>>,
}

impl Recorder {
    pub fn new(rasters: Rc<Rasters>) -> Rc<Self> {
        Rc::new(Self {
            rasters,
            ops: RefCell::new(Vec::new()),
            blind: RefCell::new((0, 0)),
            cursors: RefCell::new(HashMap::new()),
            building: RefCell::new(Vec::new()),
            not_yet: RefCell::new(BTreeMap::new()),
            bad_blend: RefCell::new(BTreeMap::new()),
            refused: RefCell::new(0),
            evictions: RefCell::new(0),
            applied: RefCell::new(0),
            text_source: RefCell::new(None),
        })
    }

    /// 换上宿主的**字形源**（[`crate::composite::TextRasterSource`]，M10.5「字形落像素」）。
    ///
    /// 装载那一侧在字体（`bundled_fonts` 那颗 `FontBundle`）就绪之后调它，之后每一次合成读到
    /// `fillText` 都会把请求交给这颗源、把结果（已着色的预乘位图）落进那颗画布。换绑是**可重入的**
    /// （判据与宿主都可能在同一颗 `Recorder` 上换），所以旧的直接被替换 —— 不留"两颗源"这种状态。
    pub fn set_text_source(&self, source: Rc<dyn crate::composite::TextRasterSource>) {
        *self.text_source.borrow_mut() = Some(source);
    }

    /// 今天有没有字形源（宿主那颗上屏桥 `CompatPaint` 据此决定 `fillText` 该不该点名为
    /// "上不了屏"—— 没源时它是缺口、有源时它已落地）。
    pub fn has_text_source(&self) -> bool {
        self.text_source.borrow().is_some()
    }

    /// 记一条操作。返回值今天没人读（宿主换成真绘制之后它就是要回答的那一句），
    /// 所以交空串而不是交一个看着像读数的东西。
    pub fn paint(&self, spec: &str) -> String {
        self.ops.borrow_mut().push(spec.to_string());
        String::new()
    }

    /// 操作流的 JSON（判据与宿主调试都用这一格）。
    pub fn ops_json(&self) -> String {
        serde_json::to_string(&self.ops.borrow().clone()).unwrap_or_default()
    }

    /// 清掉**这一格会话**：操作流、重放游标、画布与全部计数。
    ///
    /// 画布必须跟着丢 —— 判据会自己把 `Sisf2DContext._lastSid` 归零（共享 realm 里那条腿不然会
    /// 串味），于是同一个位图号会指着上一格会话留下的像素。**镜像与尺寸表不动**：那是按 URL
    /// 共享的磁盘内容，重读一次而已。
    pub fn clear(&self) {
        self.ops.borrow_mut().clear();
        *self.blind.borrow_mut() = (0, 0);
        self.cursors.borrow_mut().clear();
        self.not_yet.borrow_mut().clear();
        self.bad_blend.borrow_mut().clear();
        *self.refused.borrow_mut() = 0;
        *self.evictions.borrow_mut() = 0;
        *self.applied.borrow_mut() = 0;
        self.rasters.clear_surfaces();
    }

    /// **把这一颗粒画布的账补到当前**：回放 `[游标, ops.len())` 里属于它的那几条操作。
    ///
    /// 交 `None` = 这一颗粒**根本没有画布**（没有 `size` 也没有镜像 ⇒ 上游那里连 canvas 都没有），
    /// 调用方据此交"一片零 + 计数"，而不是"一片空画布"。
    fn ensure(&self, sid: &str, url: &str) -> Option<Rc<RgbaImage>> {
        if sid.is_empty() {
            return None;
        }
        let start = self.cursors.borrow().get(sid).copied().unwrap_or(0);
        // 只挑属于这一格的（所有操作的第二个字段都是位图号）；克隆出来再动 `Rasters`，
        // 免得把 `ops` 的借用带进递归里。
        let pending: Vec<String> = {
            let all = self.ops.borrow();
            all.iter()
                .skip(start)
                .filter(|spec| composite::target(&composite::parse(spec)) == Some(sid))
                .cloned()
                .collect()
        };
        if pending.is_empty() {
            return self
                .rasters
                .surface(sid)
                .or_else(|| self.rasters.mirror(url));
        }
        // 循环引用那一格（`blt` 的源就是正在合成的这一颗粒）：上游 canvas 读的是**改之前**的
        // 那一份，所以这里交快照，不再递归。
        let base = self
            .rasters
            .surface(sid)
            .map(|img| Canvas::from_image(&img))
            .unwrap_or_else(|| Canvas::blank(0, 0));
        self.building.borrow_mut().push(sid.to_string());
        let mut canvas = base;
        let (report, sized) = {
            let ops: Vec<String> = pending;
            self.replay_with_snapshot_guard(sid, &ops, &mut canvas)
        };
        {
            let mut building = self.building.borrow_mut();
            building.pop();
        }
        *self.applied.borrow_mut() += report.applied;
        for (name, n) in &report.not_yet {
            *self.not_yet.borrow_mut().entry(name.clone()).or_insert(0) += n;
        }
        for (name, n) in &report.bad_blend {
            *self.bad_blend.borrow_mut().entry(name.clone()).or_insert(0) += n;
        }
        if !sized && self.rasters.surface(sid).is_none() {
            return None;
        }
        match self.rasters.put_surface(sid, canvas.into_image(), false) {
            Ok(gone) => {
                *self.evictions.borrow_mut() += gone.len() as u32;
                let mut cursors = self.cursors.borrow_mut();
                for sid in &gone {
                    cursors.remove(sid);
                }
                // 游标推到**当前全长**：这一段里不属于这一格的操作以后也不必重看。
                cursors.insert(sid.to_string(), self.ops.borrow().len());
                self.rasters.surface(sid)
            }
            // 预算挡住 ⇒ 这一格不合成（读数回到"没有源"），并数下来。
            Err(_) => {
                *self.refused.borrow_mut() += 1;
                self.rasters.mirror(url)
            }
        }
    }

    /// 一次回放的源解析：`0` 号是"那张素材图"（`bltImage` 与 `_createCanvas` 收尾那一条走的都是它），
    /// 别的号先问画布、没有再问镜像，最后递归合成 —— 正在合成的那几格只交已有的那份，不再进去。
    fn replay_with_snapshot_guard(
        &self,
        sid: &str,
        ops: &[String],
        canvas: &mut Canvas,
    ) -> (composite::Report, bool) {
        let in_flight: Vec<String> = self.building.borrow().clone();
        let self_snapshot = Rc::new(canvas.snapshot());
        let source = |src_sid: &str, src_url: &str| {
            let decoded = decode_percent(src_url);
            if src_sid == sid {
                // 源就是正在合成的这一颗粒（上游 `blur` 那一格靠的就是这一手）：读快照。
                return Some(Rc::clone(&self_snapshot));
            }
            if src_sid.is_empty() || src_sid == "0" {
                return self.rasters.mirror(&decoded);
            }
            if in_flight.iter().any(|b| b == src_sid) {
                return self
                    .rasters
                    .surface(src_sid)
                    .or_else(|| self.rasters.mirror(&decoded));
            }
            self.ensure(src_sid, &decoded)
        };
        // 字形源那一条借用是**共享读**：`RefCell::borrow` 允许并发只读，所以哪怕 `blt` 递归回
        // `ensure` 再次进这一格、再 `borrow()` 一次也不撞（撞的只有 `borrow_mut`）。而源本身
        // （宿主的 `CompatText`）每次 `rasterize` 都自成一格借它的 `TextEngine`，与这颗画布的借用无关。
        let text_guard = self.text_source.borrow();
        let glyphs = text_guard
            .as_ref()
            .map(|rc| &**rc as &dyn crate::composite::TextRasterSource);
        composite::replay_with_text(sid, ops, canvas, &source, glyphs)
    }

    /// **显示那一侧读像素的把手**：把这一颗粒画布的账补到当前，并交出它的全部像素。
    ///
    /// 与 [`Recorder::pixels`] 走的是**同一条** `ensure` —— 差的就是不跨 JS 边界、不编成十六进制串。
    /// 为什么要有这一格：合成是**懒**的（今天只有 `getImageData` 会叫醒它），而"屏幕上有没有画对"
    /// 这件事的读者是宿主的绘制会话（`M10.md`「宿主那侧的真绘制」第一小格）。**屏幕也是一个读者**，
    /// 不是第二条画布：如果显示自己再拼一遍操作，就有两份真相，而合成那一格的账（`applied` /
    /// `not=[…]` / `refused`）会只盖住读像素那一条腿。
    ///
    /// 交 `None` = 这一颗粒**没有画布**（没有 `size` 也没有镜像）⇒ 显示那侧要跳过它并数下来，
    /// 而不是画一片凭空的零。
    pub fn painted(&self, sid: &str, url: &str) -> Option<Rc<RgbaImage>> {
        self.ensure(sid, url)
    }

    /// 合成的账（[`COMPOSITE_GLOBAL`]）：一条串里把每一格没回放的动作与混合值都点名。
    pub fn report(&self) -> String {
        let names: Vec<String> = self
            .not_yet
            .borrow()
            .iter()
            .map(|(k, v)| format!("{k}:{v}"))
            .collect();
        let blends: Vec<String> = self
            .bad_blend
            .borrow()
            .iter()
            .map(|(k, v)| format!("{k}:{v}"))
            .collect();
        format!(
            "applied={};not=[{}];blend=[{}];refused={};evict={}",
            self.applied.borrow(),
            names.join(","),
            blends.join(","),
            self.refused.borrow(),
            self.evictions.borrow()
        )
    }

    /// `pixels|<sid>|<x>|<y>|<w>|<h>|<urlencoded>` —— **整块读数**（上游 `getImageData`）。
    ///
    /// 先问这一颗位图自己的副本，没有才问 CPU 镜像（[`crate::raster::Rasters::read_rect`]）；
    /// 两个都没有 = 这一张是**画出来的**（内容只活在操作流里，"软件合成"那一格还没到）⇒
    /// 交 `blank|<w>|<h>` 并把读的次数加一格。JS 那一层据此造一片透明黑 ——
    /// **不是**把这一张读成黑然后说它画好了（模块头第 2 条口径）。
    pub fn pixels(&self, spec: &str) -> String {
        let fields: Vec<&str> = spec.split('|').collect();
        if fields.len() != 7 || fields[0] != "pixels" {
            return format!("{UNDELIVERED}:bad-spec");
        }
        let num = |v: &str| v.parse::<u32>().ok();
        let (Some(x), Some(y), Some(w), Some(h)) = (
            num(fields[2]),
            num(fields[3]),
            num(fields[4]),
            num(fields[5]),
        ) else {
            return format!("{UNDELIVERED}:bad-spec");
        };
        let url = decode_percent(fields[6]);
        // 先把这一颗粒的账补到当前（回放它自己的操作），再来问像素。
        let sid = fields[1].to_string();
        self.ensure(&sid, &url);
        match self.rasters.read_rect(&sid, &url, x, y, w, h) {
            Some(rows) => rows,
            None => {
                self.blind.borrow_mut().0 += 1;
                format!("blank|{w}|{h}")
            }
        }
    }

    /// `putPixels|<sid>|<x>|<y>|<w>|<h>|<urlencoded>|<rows>` —— **整块写回**（`putImageData`）；
    /// `blind|<sid>` —— 写回那一张**没有源**的（一片零，不必把它搬过边界，只数一次）。
    ///
    /// 成功交 `ok`；失败交回那一格的名字（JS 据此抛），所以"副本满了"与"形状坏了"是两种读数，
    /// 不是一种沉默。
    pub fn put_pixels(&self, spec: &str) -> String {
        let Some((head, rest)) = spec.split_once('|') else {
            return format!("{UNDELIVERED}:bad-spec");
        };
        if head == "blind" {
            self.blind.borrow_mut().1 += 1;
            return "ok".to_string();
        }
        if head != "putPixels" {
            return format!("{UNDELIVERED}:bad-spec");
        }
        let mut fields = rest.splitn(7, '|');
        let (Some(sid), Some(x), Some(y), Some(w), Some(h), Some(url), Some(rows)) = (
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
        ) else {
            return format!("{UNDELIVERED}:bad-spec");
        };
        let num = |v: &str| v.parse::<u32>().ok();
        let (Some(x), Some(y), Some(w), Some(h)) = (num(x), num(y), num(w), num(h)) else {
            return format!("{UNDELIVERED}:bad-spec");
        };
        // 色调与色相落在的是**回放之后**的那一份像素（上游合成进画布的就是那一格内容）。
        let decoded = decode_percent(url);
        self.ensure(sid, &decoded);
        match self.rasters.write_rect(sid, &decoded, x, y, w, h, rows) {
            Ok(()) => "ok".to_string(),
            // 画布装不下（全被色调占着）⇒ 丢掉这一次写并数下来，**不抛**：抛会打断
            // 每一只窗的建立（`M10.md` 事实 87 同一条腿）。坏形状照旧抛。
            Err("undelivered:owned-full" | "undelivered:no-source") => {
                self.blind.borrow_mut().1 += 1;
                "ok".to_string()
            }
            Err(reason) => reason.to_string(),
        }
    }

    /// 盲区读数：`"<读的次数>,<丢的写次数>"`（判据用它，见 [`BLIND_GLOBAL`]）。
    pub fn blind(&self) -> String {
        let (reads, writes) = *self.blind.borrow();
        format!("{reads},{writes}")
    }

    /// 文字宽度：宿主没绑真测量器时**一律**交哨兵（模块头第 2 条口径的另一半）。
    pub fn measure(&self, _spec: &str) -> String {
        format!("{UNDELIVERED}:measure")
    }
}

/// 两个十六进制字符 → 一个字节；不是十六进制就交 `None`（宽容解码的那一半）。
fn hex_byte(pair: &[u8]) -> Option<u8> {
    let dig = |b: u8| -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some((b - b'a') + 10),
            b'A'..=b'F' => Some((b - b'A') + 10),
            _ => None,
        }
    };
    Some(dig(*pair.first()?)? * 16 + dig(*pair.get(1)?)?)
}

/// `encodeURIComponent` 的那一串 `%XX`（UTF-8 字节）解回去；解不动的原样留着
/// （与 [`crate::raster`] 的宽容解码同一条腿 —— 真名叫 `50%.png` 的素材不会被解坏）。
///
/// **现在是公开的**（M10.5「文字那两格」的测量腿）：宿主那一侧的 `__sisf_measure_text` 实现
/// 收到的是**同一条协议**（正文与字族名都 `encodeURIComponent` 过），拿同一个解码器才不留第二份真相。
pub fn decode_percent(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &bytes[i + 1..i + 3];
            if let Some(byte) = hex_byte(hex) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(&e.into_bytes()).into_owned())
}

/// 把这一格的八只把手绑上去（[`PAINT_GLOBAL`] / [`PAINT_OPS_GLOBAL`] / [`PAINT_RESET_GLOBAL`] /
/// [`PIXELS_GLOBAL`] / [`PUT_PIXELS_GLOBAL`] / [`MEASURE_GLOBAL`] / [`BLIND_GLOBAL`] /
/// [`COMPOSITE_GLOBAL`]）。由 [`crate::core::install_bitmap`] 调用 —— 装载次序是它的内部前提，
/// 不是调用方的功课（`managers_install_does_not_depend_on_the_caller_installing_core_first` 同一条腿）。
pub fn install(host: &crate::jsrt::Host, rasters: Rc<Rasters>) -> Result<Rc<Recorder>, String> {
    let recorder = Recorder::new(rasters);
    host.bind_str(PAINT_GLOBAL, {
        let recorder = Rc::clone(&recorder);
        move |spec| recorder.paint(&spec)
    })?;
    host.bind_str(PAINT_OPS_GLOBAL, {
        let recorder = Rc::clone(&recorder);
        move |_| recorder.ops_json()
    })?;
    host.bind_str(PIXELS_GLOBAL, {
        let recorder = Rc::clone(&recorder);
        move |spec| recorder.pixels(&spec)
    })?;
    host.bind_str(PUT_PIXELS_GLOBAL, {
        let recorder = Rc::clone(&recorder);
        move |spec| recorder.put_pixels(&spec)
    })?;
    host.bind_str(MEASURE_GLOBAL, {
        let recorder = Rc::clone(&recorder);
        move |spec| recorder.measure(&spec)
    })?;
    host.bind_str(BLIND_GLOBAL, {
        let recorder = Rc::clone(&recorder);
        move |_| recorder.blind()
    })?;
    host.bind_str(COMPOSITE_GLOBAL, {
        let recorder = Rc::clone(&recorder);
        move |_| recorder.report()
    })?;
    host.bind_str(PAINT_RESET_GLOBAL, {
        let recorder = Rc::clone(&recorder);
        move |_| {
            recorder.clear();
            String::new()
        }
    })?;
    Ok(recorder)
}

#[cfg(all(test, feature = "js"))]
mod tests {
    use super::*;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    /// 一份只有 `img/system/Window.png`（2×2 样品，含半透明那一档）的夹具 VFS。
    fn window_vfs() -> Rc<sisf_asset::Vfs> {
        use sisf_asset::{MemoryProvider, Vfs, VfsPath};
        let mut mem = MemoryProvider::new();
        mem.insert(
            &VfsPath::new("img/system/Window.png").unwrap(),
            &crate::raster::sample_png_2x2(),
        );
        let mut vfs = Vfs::new();
        vfs.register(Box::new(mem));
        Rc::new(vfs)
    }

    fn recorder() -> Rc<Recorder> {
        Recorder::new(Rasters::new(window_vfs()))
    }

    /// 操作流是判据要钉的东西：进得来、读得出、**顺序不动**（会话里"后画的盖前面"靠的就是提交序）。
    #[test]
    fn the_recorder_keeps_the_ops_in_the_order_they_came() {
        let r = recorder();
        r.paint("clearRect|7|0|0|816|144");
        r.paint("fillRect|7|0|0|816|4|255,0,0,128|source-over");
        assert_eq!(
            r.ops_json(),
            "[\"clearRect|7|0|0|816|144\",\"fillRect|7|0|0|816|4|255,0,0,128|source-over\"]",
            "顺序反了就是「后画的被埋在下面」—— 上游 canvas 的覆盖顺序是玩家看得见的"
        );
        r.clear();
        assert_eq!(r.ops_json(), "[]");
    }

    /// 1×1 且**有镜像**：交回那一格的四个数（未预乘，[`crate::raster`] 已经除回 alpha）。
    #[test]
    fn a_one_pixel_read_answers_from_the_cpu_mirror() {
        let r = recorder();
        assert_eq!(
            r.pixels("pixels|7|1|1|1|1|img%2Fsystem%2FWindow.png"),
            "128,64,32,128",
            "半透明那一样品就是为「除回 alpha」放的（AGENTS 咬人第 25 条⑵同一条腿）"
        );
        // 越界照 raster 那一格答透明黑（上游空白处也是透明黑），不抛。
        assert_eq!(
            r.pixels("pixels|9|9|9|1|1|img%2Fsystem%2FWindow.png"),
            "0,0,0,0"
        );
    }

    /// **没有像素源**的那一张：读交 `blank|<w>|<h>`、写只数不搬，两个次数都读得到
    /// （`__sisf_pixel_blind` 那一格），坏形状与没绑测量器仍然点名。
    #[test]
    fn a_surface_with_no_pixel_source_is_blank_and_counted() {
        let r = recorder();
        assert_eq!(r.pixels("pixels|7|0|0|1|1|"), "blank|1|1");
        assert_eq!(r.put_pixels("blind|7"), "ok");
        assert_eq!(
            r.blind(),
            "1,1",
            "一次读一次写 ⇒ 各数一格（色调落在一片零上 = 合成那一格没到）"
        );
        r.clear();
        assert_eq!(
            r.blind(),
            "0,0",
            "`__sisf_paint_reset` 把盲区计数一起清掉，判据才数得到自己那一段"
        );
        assert_eq!(r.pixels("nonsense"), "undelivered:bad-spec");
        assert_eq!(r.put_pixels("nonsense|1"), "undelivered:bad-spec");
        assert_eq!(
            r.measure("measure|%E4%BD%A0|GameFont|28|0"),
            "undelivered:measure"
        );
    }

    /// 写回**先复制一份这一颗位图自己的**缓冲：镜像（共享的那一份）一个字都不动。
    /// 上游的 canvas 是 per-bitmap 的，色调写回镜像就是"第二颗 `Bitmap.load` 同一张图也变色"。
    #[test]
    fn a_write_lands_on_the_bitmap_copy_and_not_on_the_shared_mirror() {
        let r = recorder();
        let url = "img%2Fsystem%2FWindow.png";
        // 先钉**写之前**镜像与读数都是原色（不然下面的红了分不清是写坏的还是读坏的）。
        assert_eq!(r.pixels(&format!("pixels|7|0|0|1|1|{url}")), "255,0,0,255");
        assert_eq!(
            r.put_pixels(&format!(
                "putPixels|7|0|0|2|2|{url}|204,0,0,255;0,235,0,255;0,0,235,255;77,13,0,128"
            )),
            "ok"
        );
        assert_eq!(
            r.pixels(&format!("pixels|7|0|0|1|1|{url}")),
            "204,0,0,255",
            "同一颗位图再读要读到写进去的那一格"
        );
        assert_eq!(
            r.pixels(&format!("pixels|8|0|0|1|1|{url}")),
            "255,0,0,255",
            "另一颗位图（另一个号）读到的还是镜像原色 ⇒ 副本没有漏进共享的那一份"
        );
    }

    /// 半透明那一档的**往返**：进来的是非预乘（canvas 那一族），存的是预乘（镜像那一族），
    /// 两次整数除法各带一次舍入 ⇒ 界内的数能回来，界外与小数**不能**（JS 那侧的
    /// `Uint8ClampedArray` 已经夹过、舍过，见 `shim_context.js` 的 `getImageData`）。
    #[test]
    fn a_semi_transparent_write_round_trips_through_the_premultiplied_store() {
        let r = recorder();
        let url = "img%2Fsystem%2FWindow.png";
        assert_eq!(
            r.put_pixels(&format!("putPixels|7|1|1|1|1|{url}|77,13,0,128")),
            "ok"
        );
        assert_eq!(
            r.pixels(&format!("pixels|7|1|1|1|1|{url}")),
            "78,14,0,128",
            "预乘往返的读数（77→78 / 13→14 是 `(c*a+127)/255` 与除回去的两步舍入）"
        );
    }

    /// **挤掉不丢东西**（画布预算那一档的淘汰资格在 `raster.rs` 那条判据里）：被挤掉的合成画布
    /// 靠重放自己的操作流一模一样地回来 —— 这一格钉的是"合成可重放"这条设计前提，不是顺手支持 LRU。
    /// 预算压成一颗 2×2（4 像素）才走得到淘汰，理由与 `raster.rs` 那两条预算判据同一格。
    #[test]
    fn an_evicted_canvas_comes_back_from_its_own_ops() {
        let r = Recorder::new(Rasters::with_surface_budget(window_vfs(), u64::MAX, 4));
        let url = "img%2Fsystem%2FWindow.png";
        r.paint(&format!("size|1|2|2|{url}"));
        r.paint("fillRect|1|0|0|1|1|255,0,0,255|source-over");
        r.paint(&format!("size|2|2|2|{url}"));
        r.paint("fillRect|2|0|0|1|1|0,255,0,255|source-over");
        assert_eq!(r.pixels(&format!("pixels|1|0|0|1|1|{url}")), "255,0,0,255");
        assert_eq!(r.pixels(&format!("pixels|2|0|0|1|1|{url}")), "0,255,0,255");
        // 装下第 2 颗的那一拍，第 1 颗的画布被挤掉了（两颗都是可重放的那一档）。
        assert_eq!(
            r.report(),
            "applied=4;not=[];blend=[];refused=0;evict=1",
            "读数里必须看得见那一次挤掉 —— 看不见就是「静默丢像素」的另一面。\
             `applied=4` 是两颗位图各回放自己那两条（`size` + `fillRect`）；\
             下面那一次重读还要再落两条，但它不在这一格的范围里（游标从头再来正是这一格要钉的）"
        );
        assert_eq!(
            r.pixels(&format!("pixels|1|0|0|1|1|{url}")),
            "255,0,0,255",
            "被挤掉之后再读，必须从重放里一模一样地回来（游标跟着画布一起丢 ⇒ 从头重放）"
        );
        assert_eq!(
            r.blind(),
            "0,0",
            "重放回来的是**真像素**，不是「没有源」那一档"
        );
    }

    /// 预算全被**色调**占着时的那一档：合成挤不出候选 ⇒ **拒**，读数回到"没有源"并且两边都点名
    /// （`refused` 数合成被拒的次数，`__sisf_pixel_blind` 数那几次读）。不许"顺手把色调抹回原样
    /// 腾地方" —— 那比不画更错（上游的 canvas 永远不会这样丢东西，见 `raster.rs` 的 `OWNED_LIMIT`
    /// 那一段与上一条淘汰判据）。
    ///
    /// 这一格里那一次带 URL 的读（`pixels|2|…`）答的是**镜像原色**，这是拒绝那一支的既定退路
    /// （`Recorder::ensure` 的 `Err` 分支交 `mirror(url)`）：素材图的内容本来就等于那张图的像素，
    /// 而"画上去的内容"这一族没有 URL ⇒ 只能读成一片透明黑并计数。
    #[test]
    fn a_canvas_the_budget_won_not_hold_is_refused_and_named() {
        let r = Recorder::new(Rasters::with_surface_budget(window_vfs(), u64::MAX, 4));
        let url = "img%2Fsystem%2FWindow.png";
        // 先让色调占满那一档（`write_rect` 走的是 `filtered = true` 那一条腿）。
        assert_eq!(
            r.put_pixels(&format!(
                "putPixels|1|0|0|2|2|{url}|204,0,0,255;0,235,0,255;0,0,235,255;77,13,0,128"
            )),
            "ok"
        );
        // 再请合成进来：一颗 2×2 的画布正好放不进（色调不淘汰）。
        r.paint(&format!("size|2|2|2|{url}"));
        r.paint("fillRect|2|0|0|1|1|0,255,0,255|source-over");
        assert_eq!(r.pixels(&format!("pixels|2|0|0|1|1|{url}")), "255,0,0,255");
        assert_eq!(
            r.pixels("pixels|3|0|0|1|1|"),
            "blank|1|1",
            "这一颗粒没 URL 可退回镜像 ⇒ 读成一片透明黑并**数一次**（不是把没画读成画了）"
        );
        r.paint("size|3|2|2|");
        r.paint("fillRect|3|0|0|1|1|0,0,255,255|source-over");
        assert_eq!(
            r.pixels("pixels|3|0|0|1|1|"),
            "blank|1|1",
            "重放被预算挡住 ⇒ 这一格还是「没有源」那一档，而且一条都不静默"
        );
        assert_eq!(r.blind(), "2,0", "两次读都数到（写一次都没有丢）");
        assert_eq!(
            r.report(),
            "applied=4;not=[];blend=[];refused=2;evict=0",
            "两次合成都被拒（`2` 与 `3` 各一次，操作各两条 ⇒ `applied` 照数，落点由 `refused` 点名）；\
             色调那一颗一个像素都没动（`evict=0`）—— 宁可读成「没有源」，不许把色调抹回原样"
        );
    }

    /// **显示那一侧叫醒的是同一台合成器**（[`Recorder::painted`]）：宿主的绘制会话问像素时，
    /// 合成必须像 `getImageData` 那样把这一颗粒的账补到当前 —— 而不是让显示去自己重放一遍操作
    /// （那就有两份画布，而 `applied`/`not=[…]`/`refused` 那本账只盖得住一条腿）。
    /// 最后一行钉的是"没有画布"那一档交 `None`，宿主据此**跳过并数下来**。
    #[test]
    fn the_display_side_wakes_the_same_composite_as_the_pixel_reader() {
        let r = recorder();
        r.paint("size|7|2|2|");
        r.paint("fillRect|7|0|0|2|1|255,0,0,255|source-over");
        let img = r
            .painted("7", "")
            .expect("显示那一侧问像素 ⇒ 合成把账补到当前");
        assert_eq!(img.width, 2);
        assert_eq!(
            (img.pixels[0..4].to_vec(), img.pixels[8..12].to_vec()),
            (vec![255, 0, 0, 255], vec![0, 0, 0, 0]),
            "第一行是那一次 `fillRect`，第二行仍然透明 —— 预乘域里 alpha 255 时字节与非预乘同一档"
        );
        assert_eq!(
            r.report(),
            "applied=2;not=[];blend=[];refused=0;evict=0",
            "显示这一侧读一次，落的还是那两条操作（`size` + `fillRect`）"
        );
        assert_eq!(r.blind(), "0,0", "有画布 ⇒ 一次都不算盲区");
        assert!(
            r.painted("8", "").is_none(),
            "没有 `size` 也没有镜像的那一颗：交 `None`，让显示那侧跳过它"
        );
    }

    /// 编码那一格：台词里有 `|` 与换行（MV 的 `\|` 是右对齐转义，但插件文本里就是字面量），
    /// 素材名里有 `%` 却不是合法转义（`50%.png` 那一条真样品）。
    #[test]
    fn the_decoder_survives_both_spellings_that_are_on_the_disk() {
        assert_eq!(
            decode_percent("img%2Fpictures%2F%E4%BD%A0.png"),
            "img/pictures/\u{4f60}.png"
        );
        assert_eq!(decode_percent("50%25.png"), "50%.png");
        assert_eq!(
            decode_percent("50%.png"),
            "50%.png",
            "不是合法 %XX 的原样留着"
        );
        assert_eq!(decode_percent("a%7Cb"), "a|b");
    }
}
