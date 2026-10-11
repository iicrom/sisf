//! M10.5 第四格第一小格：`Bitmap` 的**像素从哪来**（`Bitmap` 的"数据那一半"的 Rust 侧）。
//!
//! ## 为什么尺寸要常驻、像素要按预算留
//!
//! 兼容层里问位图的两格是**不同频次**的：`width`/`height` 每一颗位图都要被问（`ImageCache` 的
//! `_truncateCache` 拿它算面积，`rpg_core.js:518-534`），而像素只有 `getPixel`/`getAlphaPixel`
//! 那两格读 —— 上游的调用点是 `Window_Base.textColor`（从窗皮取一格当字色，`rpg_windows.js:173-177`）
//! 与插件侧的 `paintKey`/hue 那一族。所以这一层交两件事：
//!
//! - **尺寸表**常驻（每个键 12 字节，读不到也记一条 `None` ⇒ 缺失的素材不会被反复解）；
//! - **像素镜像**按 `PIXEL_BUDGET` 的 LRU 留（预算就是上游 `ImageCache.limit` 那一格，
//!   `rpg_core.js:468` 的 `10 * 1000 * 1000` 像素 —— 同一个"留多少位图"的判断，不另立一个数）。
//!   淘汰**不丢正确性**：一份磁盘内容是不可变的，键就是它的路径，被挤掉之后下一次读会重新解一遍，
//!   只是慢一点。这一格与 JS 侧的 `Bitmap` 生命周期无关（那是 GC 的事），所以拿 LRU 兜是安全的。
//!
//! ## 预乘那一格（不是细节，是读数）
//!
//! [`sisf_media::image::decode_image`] 交的是**预乘** RGBA8（`crates/sisf-media/src/image.rs:112-124`
//! 的 `premultiply`，`(c*a+127)/255`），而上游 `getPixel` 问的是 canvas 的 `getImageData` ——
//! 那一份是**非预乘**的（`AGENTS.md` 咬人第 25 条⑵量的就是这两者差一个 alpha 的倍数）。
//! 所以 [`Rasters::pixel`] 在交出读数之前把alpha 除回去。实测这一族的两个档：
//! alpha 255 时 `premultiply` 直接跳过 ⇒ 除回去是恒等的（`textColor` 那 32 格全在这一档，
//! 实测 alpha 全 255）；alpha 128 那一档是 `(128*128+127)/255 = 64` → `(64*255+64)/128 = 128`，
//! 四舍五入两头对得上 ⇒ 夹具 `sample_png_2x2` 的第 4 颗像素就是钉这一格的。
//!
//! ## 名字那一格：URL 空间 → VFS 空间
//!
//! 上游交给 `_requestImage` 的是 **URL**（`ImageManager.requestBitmap` 的
//! `folder + encodeURIComponent(filename) + '.png'`，`rpg_managers.js:1062-1071`），浏览器取字节之前
//! 自己会把它解回字节路径；而 `loadNormalBitmap`（`:884-898`）上游**已经**先 `decodeURIComponent` 了。
//! ⇒ VFS 要的拼写在两条腿上不同，所以 `Rasters::decoded` 做**宽松**的百分号解码：`%XX` 是两位十六进制
//! 才折，否则原样留着（这样真名叫 `50%.png` 的素材不会被解坏）。解码只在 Rust 做一次，
//! JS 那两格自创底把 `_url` 原样交过来 —— 两处各解一遍就是第二个真相。

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use sisf_asset::{Vfs, VfsPath};
use sisf_media::image::{RgbaImage, decode_image};

/// 尺寸读数那一条 transport 的全局名：`__sisf_image_size(url)` → `"w,h"`，读不到交**空串**。
pub const SIZE_GLOBAL: &str = "__sisf_image_size";
/// 单像素读数那一条 transport 的全局名。
///
/// 入参是 **`"x|y|url"`**：[`crate::jsrt::Host::bind_str`] 只带一个字符串参数，而把两个数字放在
/// 前面就能从**左边**切两刀拿到 url —— 文件名里真有 `|` 也不会切错（放在末尾就得从右边切，
/// 而 `splitn` 只能从左）。出参是 `"r,g,b,a"`（非预乘），读不到交 `"0,0,0,0"` ——
/// 与上游 canvas 在越界/空白位图上 `getImageData` 交透明黑同一档。
pub const PIXEL_GLOBAL: &str = "__sisf_image_pixel";

/// 像素镜像的预算，单位是**像素**（不是字节）：与上游 `ImageCache.limit` 同一格
/// （`rpg_core.js:468` 的 `10 * 1000 * 1000`）。
pub const PIXEL_BUDGET: u64 = 10_000_000;

/// 一份位图**自己的**像素副本的**颗数**上限。
///
/// 为什么"满了就拒绝新的写"而不是 LRU 淘汰：淘汰一颗副本等于把它的色调/色相**抹回素材原样**，
/// 而上游的 canvas 是 per-bitmap 的，永远不会这样丢东西 ⇒ 宁可让 JS 那一层抛并点名。
/// 这个数是从真数据量的：`data/Animations.json` 的 `animation1Hue`/`animation2Hue` 非零
/// **25 + 12** 条、`data/Enemies.json` 的 `battlerHue` 非零 **4** 条（战斗侧 = 定案的非目标）
/// ⇒ 一共 41 颗，64 是余量。尺子：`../sisf_pray/tools/probes/bitmap-filters.py` 的第③段。
///
/// ⚠ 这一格只管**被 `putImageData` 写过**的那一族（色调/色相的结果**不可重放**）。
/// 软件合成出来的画布（[`crate::composite`]）走另一条腿：它可以被挤掉，因为重放一次操作流
/// 就一模一样地回来 —— 两个档共用 [`SURFACE_BUDGET`] 那一份像素预算，但**淘汰资格**不同。
pub const OWNED_LIMIT: usize = 64;

/// 每颗位图画布的像素预算，单位是**像素**：与镜像 [`PIXEL_BUDGET`] 同一个口径
/// （也是上游 `ImageCache.limit` 那一格）。合成是**懒的**（只有有人读像素才回放），
/// 所以这一档在真流上的实际占用远小于它 —— 但它必须存在，否则一张 1104×736 的画布
/// 被读一次就永久占着（尺子 `software-composite.py` 第④段量的就是这一族的单价）。
pub const SURFACE_BUDGET: u64 = PIXEL_BUDGET;

/// 一份 VFS 之上的位图读数表。`Rc` 之后交给两条 transport 的闭包各自捕获。
pub struct Rasters {
    vfs: Rc<Vfs>,
    /// 像素镜像的预算（**生产用 [`PIXEL_BUDGET`]，判据把小数字只留给淘汰循环本身**）。
    budget: u64,
    /// 位图画布的预算（[`SURFACE_BUDGET`] 同一格；判据另给小数才走得到淘汰）。
    surface_budget: u64,
    /// 尺寸表（`None` = 这一条读不到；负缓存是**必需**的：清单外的图会被反复问）。
    dims: RefCell<HashMap<String, Option<(u32, u32)>>>,
    /// 像素镜像（预乘 RGBA8，交出时在这一层除回非预乘）。
    held: RefCell<HashMap<String, Rc<RgbaImage>>>,
    /// LRU 序（front = 最久没被问的那一条）。
    order: RefCell<VecDeque<String>>,
    /// `held` 里现在一共占了多少像素。
    pixels: RefCell<u64>,
    /// 每颗位图**自己的**像素副本（`putImageData` 第一次写才分配；键 = `Bitmap` 的位图号）。
    /// 上游的 canvas 就是 per-bitmap 的，所以色调/色相**不许写回共享的镜像**。
    owned: RefCell<HashMap<String, Rc<RgbaImage>>>,
    /// `owned` 那一族的 LRU 序（front = 下一个候选被挤掉的）。
    surface_order: RefCell<VecDeque<String>>,
    /// `owned` 现在一共占了多少像素（单位与 [`SURFACE_BUDGET`] 一致）。
    surface_pixels: RefCell<u64>,
    /// 被**像素写回**碰过的那一格位图号：色调/色相的结果不可重放 ⇒ 这一族**永远不被挤掉**
    /// （满了是拒绝并计数，不是抹回原样）。
    filtered: RefCell<std::collections::HashSet<String>>,
}

impl Rasters {
    /// 挂在上游那一份 VFS 上（native 是磁盘 + 内存解密，浏览器是清单 + 预取灌进来的字节）。
    /// 两份预算都是上游 `ImageCache.limit` 那一格（见 [`PIXEL_BUDGET`] 与 [`SURFACE_BUDGET`]）。
    pub fn new(vfs: Rc<Vfs>) -> Rc<Self> {
        Self::with_budget(vfs, PIXEL_BUDGET)
    }

    /// 同一件事，镜像预算另给 —— 存在的理由只有一条：淘汰循环要用**几个像素**的量才走得到，
    /// 而 10·1000·1000 那一档的夹具得 allocate 几十 MB 才咬得动（判据不收那种价格）。
    /// 画布的预算走 [`Rasters::with_surface_budget`]（判据要压它的是另一条腿）。
    pub fn with_budget(vfs: Rc<Vfs>, budget: u64) -> Rc<Self> {
        Self::with_surface_budget(vfs, budget, SURFACE_BUDGET)
    }

    /// 两份预算一起给（画布那一档的淘汰与拒绝只有压成**几个像素**才走得到，同上面那条理由）。
    pub fn with_surface_budget(vfs: Rc<Vfs>, budget: u64, surface_budget: u64) -> Rc<Self> {
        Rc::new(Self {
            vfs,
            budget,
            surface_budget,
            dims: RefCell::new(HashMap::new()),
            held: RefCell::new(HashMap::new()),
            order: RefCell::new(VecDeque::new()),
            pixels: RefCell::new(0),
            owned: RefCell::new(HashMap::new()),
            surface_order: RefCell::new(VecDeque::new()),
            surface_pixels: RefCell::new(0),
            filtered: RefCell::new(std::collections::HashSet::new()),
        })
    }

    /// `__sisf_image_size` 的那一条腿：`"w,h"` 或空串。
    pub fn size(&self, url: &str) -> String {
        match self.dims_of(url) {
            Some((w, h)) => format!("{w},{h}"),
            None => String::new(),
        }
    }

    /// `__sisf_image_pixel` 的那一条腿：`"x|y|url"` → `"r,g,b,a"`（非预乘）。
    ///
    /// 任何一步读不到（形状不对、坐标越界、素材不在）都交 `"0,0,0,0"`：上游那一格
    /// `getImageData(x, y, 1, 1)` 在越界与空白位图上同样是透明黑，`getPixel` 因此答 `#000000`
    /// 而不是抛 —— 兼容层不许在这一格比上游严格。
    pub fn pixel(&self, spec: &str) -> String {
        let Some((xs, rest)) = spec.split_once('|') else {
            return TRANSPARENT.to_string();
        };
        let Some((ys, url)) = rest.split_once('|') else {
            return TRANSPARENT.to_string();
        };
        let (Ok(x), Ok(y)) = (xs.parse::<u32>(), ys.parse::<u32>()) else {
            return TRANSPARENT.to_string();
        };
        let Some(img) = self.image(url) else {
            return TRANSPARENT.to_string();
        };
        if x >= img.width || y >= img.height {
            return TRANSPARENT.to_string();
        }
        let [r, g, b, a] = straight(img.pixel(x, y));
        format!("{r},{g},{b},{a}")
    }

    /// **整块读数**（上游 `getImageData(x, y, w, h)`）：先问这一颗位图自己的副本，没有才问镜像。
    ///
    /// 交回 **`w*h` 格**的 `r,g,b,a`（**非预乘** —— 与上游同一域），格之间 `;`、行主序。
    /// 越界的那些格交透明黑（上游 canvas 同档）。`None` = 既没有副本也没有镜像（素材读不到，
    /// 或从没写过）⇒ 由 [`crate::paint`] 交回 `blank|<w>|<h>` 并数一次（"软件合成"那一格的名字）。
    pub fn read_rect(
        &self,
        sid: &str,
        url: &str,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Option<String> {
        let base = self.owned_or_mirror(sid, url)?;
        let mut rows: Vec<String> = Vec::with_capacity((width * height) as usize);
        for dy in 0..height {
            for dx in 0..width {
                let (px, py) = (x + dx, y + dy);
                if px < base.width && py < base.height {
                    let [r, g, b, a] = straight(base.pixel(px, py));
                    rows.push(format!("{r},{g},{b},{a}"));
                } else {
                    rows.push(TRANSPARENT.to_string());
                }
            }
        }
        Some(rows.join(";"))
    }

    /// **整块写回**（上游 `putImageData(imageData, x, y)`）：第一次写先从镜像复制一份
    /// **这一颗位图自己的**缓冲（副本里存的仍是预乘，与镜像同域 —— 两个域就是两个真相）。
    ///
    /// 进来的数在 JS 那一侧已经过 `Uint8ClampedArray`（夹到 0..=255、小数按最近偶数舍 ——
    /// 上游 `adjustTone` 的 `pixels[i] += r` 与 `rotateHue` 的浮点 `hslToRgb` 都靠它），
    /// 但它跨边界的形式是**文本**，所以这一层仍然自己夹一次并给坏形状点名（见 `parse_cell`）。
    ///
    /// 参数的形状就是那条 transport 的形状（位图号 + URL + 矩形 + 那一串格）—— 收成一个
    /// `Rect` 只会把一次拆分搬两遍，所以照引擎那一侧的同一格写法
    /// （`crates/sisf-render/src/renderer.rs:475` 同一条 `allow`）留着。
    #[allow(clippy::too_many_arguments)]
    pub fn write_rect(
        &self,
        sid: &str,
        url: &str,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        rows: &str,
    ) -> Result<(), &'static str> {
        let base = self
            .owned_or_mirror(sid, url)
            .ok_or("undelivered:no-source")?;
        let mut pixels = base.pixels.clone();
        let row_bytes = (base.width as usize) * 4;
        let (x0, y0) = (x as usize, y as usize);
        let (w, h) = (width as usize, height as usize);
        // 进来的串与 [`Self::read_rect`] **交出去的那一串是同一个形状**：逐格 `r,g,b,a`、
        // 行主序、`;` 分隔（不是"一行一串" —— 上游 `getImageData` → 改 → `putImageData`
        // 走的就是自己读回来的那一串，两处不同形就是第二个真相）。
        let mut seen = 0usize;
        for (i, cell) in rows.split(';').take(w * h).enumerate() {
            let Some([r, g, b, a]) = parse_cell(cell) else {
                return Err("undelivered:bad-cell");
            };
            let tx = x0 + i % w;
            let ty = y0 + i / w;
            if tx >= base.width as usize || ty >= base.height as usize {
                continue;
            }
            let off = ty * row_bytes + tx * 4;
            // 存进去的是**预乘**（与镜像同域）：`(c*a + 127) / 255`，
            // 与 `crates/sisf-media/src/image.rs:112-124` 那一句同一个式子。
            let premul = |c: u8| (u32::from(c) * u32::from(a) + 127) / 255;
            pixels[off] = premul(r) as u8;
            pixels[off + 1] = premul(g) as u8;
            pixels[off + 2] = premul(b) as u8;
            pixels[off + 3] = a;
            seen += 1;
        }
        if seen == 0 {
            return Err("undelivered:empty-write");
        }
        // 色调/色相的结果**不可重放** ⇒ `filtered = true`：这一族永远不被挤掉。
        self.put_surface(sid, RgbaImage::new(base.width, base.height, pixels), true)
            .map(|_| ())
    }

    /// 副本优先，其次镜像。
    fn owned_or_mirror(&self, sid: &str, url: &str) -> Option<Rc<RgbaImage>> {
        if let Some(copy) = self.owned.borrow().get(sid) {
            return Some(Rc::clone(copy));
        }
        self.image(url)
    }

    /// 只问这一颗位图的**画布**（不碰镜像）—— 合成器要的基座与增量回放的落点。
    pub fn surface(&self, sid: &str) -> Option<Rc<RgbaImage>> {
        self.owned.borrow().get(sid).cloned()
    }

    /// 只问镜像（按 URL 共享的那一份解码字节）—— `blt` 的源与"这张图就是画布内容"的那一格走它。
    pub fn mirror(&self, url: &str) -> Option<Rc<RgbaImage>> {
        self.image(url)
    }

    /// 这一颗位图被**像素写回**碰过吗？（色调/色相的结果不可重放 ⇒ 它不能像合成画布那样被挤掉）
    pub fn is_filtered(&self, sid: &str) -> bool {
        self.filtered.borrow().contains(sid)
    }

    /// 丢掉**全部**每颗粒图的画布（镜像与尺寸表不动 —— 那是按 URL 共享的磁盘内容）。
    ///
    /// 判据那条 `__sisf_paint_reset` 要它：那一段还会把 JS 侧的 `Sisf2DContext._lastSid` 归零，
    /// 于是位图号会**重复使用** ⇒ 不清画布就会读到上一条判据留下的像素。
    pub fn clear_surfaces(&self) {
        self.owned.borrow_mut().clear();
        self.surface_order.borrow_mut().clear();
        *self.surface_pixels.borrow_mut() = 0;
        self.filtered.borrow_mut().clear();
    }

    /// 收下回放或写出来的画布，交回**被挤掉的**旧位图号（调用方要把它们的重放游标一起丢）。
    ///
    /// 两种失败都点名，不静默：
    /// · `undelivered:owned-full` = 色调写过的那一族满了（[`OWNED_LIMIT`]，量出来的 41 + 余量）；
    /// · `undelivered:over-budget` = 合成画布超 [`SURFACE_BUDGET`] 而一条都挤不出来
    ///   （全被色调占着 ⇒ 色调**不许**被抹回原样）。
    /// 合成的那一档被拒时调用方回到"一片零 + 计数"；色调那一档被拒时**丢掉这一次写并计数**
    /// —— 抛会打断每一只窗的建立，同 `M10.md` 事实 87 那条腿。
    pub fn put_surface(
        &self,
        sid: &str,
        img: RgbaImage,
        filtered: bool,
    ) -> Result<Vec<String>, &'static str> {
        let area = u64::from(img.width) * u64::from(img.height);
        let mut held = self.owned.borrow_mut();
        if filtered && !held.contains_key(sid) && self.filtered.borrow().len() >= OWNED_LIMIT {
            return Err("undelivered:owned-full");
        }
        let mut total = self.surface_pixels.borrow_mut();
        if let Some(old) = held.get(sid) {
            *total -= u64::from(old.width) * u64::from(old.height);
        }
        let mut evicted: Vec<String> = Vec::new();
        // 先挤**最旧且没被色调写过**的那一条，挤不出来说明全被色调占着 ⇒ 合成拒、色调留。
        while *total + area > self.surface_budget {
            let victim = self
                .surface_order
                .borrow()
                .iter()
                .position(|k| k != sid && !self.filtered.borrow().contains(k.as_str()));
            let Some(at) = victim else {
                if !filtered {
                    return Err("undelivered:over-budget");
                }
                break;
            };
            let gone = self
                .surface_order
                .borrow_mut()
                .remove(at)
                .expect("position 与队列一致");
            if let Some(old) = held.remove(&gone) {
                *total -= u64::from(old.width) * u64::from(old.height);
                evicted.push(gone);
            }
        }
        held.insert(sid.to_string(), Rc::new(img));
        *total += area;
        drop(total);
        let mut order = self.surface_order.borrow_mut();
        if let Some(at) = order.iter().position(|k| k == sid) {
            order.remove(at);
        }
        order.push_back(sid.to_string());
        drop(order);
        if filtered {
            self.filtered.borrow_mut().insert(sid.to_string());
        }
        Ok(evicted)
    }

    /// 尺寸（带负缓存）。这是"每一颗位图都要被问"的那一格，所以它只留 12 字节，不留像素。
    fn dims_of(&self, url: &str) -> Option<(u32, u32)> {
        let key = decoded(url);
        if let Some(found) = self.dims.borrow().get(&key) {
            return *found;
        }
        let read = self.image(&key).map(|img| (img.width, img.height));
        self.dims.borrow_mut().insert(key, read);
        read
    }

    /// 像素镜像：命中就把它挪到 LRU 尾，没命中就现读现解。
    fn image(&self, url: &str) -> Option<Rc<RgbaImage>> {
        let key = decoded(url);
        if let Some(img) = self.held.borrow().get(&key).cloned() {
            self.touch(&key);
            return Some(img);
        }
        let path = VfsPath::new(&key).ok()?;
        let bytes = self.vfs.read(&path)?;
        let img = match decode_image(&bytes) {
            Ok(found) => found.first().clone(),
            Err(_) => return None,
        };
        let img = Rc::new(img);
        self.hold(&key, &img);
        Some(img)
    }

    /// 记一条命中（并把总数压回预算之内）。
    fn hold(&self, key: &str, img: &Rc<RgbaImage>) {
        let area = u64::from(img.width) * u64::from(img.height);
        self.held
            .borrow_mut()
            .insert(key.to_string(), Rc::clone(img));
        self.order.borrow_mut().push_back(key.to_string());
        *self.pixels.borrow_mut() += area;
        while *self.pixels.borrow() > self.budget {
            let Some(old_key) = self.order.borrow_mut().pop_front() else {
                break;
            };
            if let Some(old) = self.held.borrow_mut().remove(&old_key) {
                *self.pixels.borrow_mut() -= u64::from(old.width) * u64::from(old.height);
            }
        }
    }

    /// 命中之后把它挪到队尾（`held` 里可能已经被淘汰，那就什么都不做 —— 下一次读会重新解）。
    fn touch(&self, key: &str) {
        if !self.held.borrow().contains_key(key) {
            return;
        }
        let mut order = self.order.borrow_mut();
        if let Some(at) = order.iter().position(|k| k == key) {
            order.remove(at);
        }
        order.push_back(key.to_string());
    }
}

/// 一格像素串 `r,g,b,a` → 四个数，**每格夹回 0..=255**。
///
/// 为什么还要夹一次：JS 那一侧的 `Uint8ClampedArray` 已经夹过，但跨边界的形状是**文本**，
/// 而 `as u8` 在界外是**绕回**（256 → 0）而不是夹 —— 少这一格就是"越界的数静默变成另一个数"。
/// 解不动（不是数）交 `None` ⇒ 坏形状点名报错，不写回半条。
fn parse_cell(cell: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    for (i, part) in cell.split(',').take(4).enumerate() {
        let value: i64 = part.trim().parse().ok()?;
        out[i] = value.clamp(0, 255) as u8;
    }
    Some(out)
}

const TRANSPARENT: &str = "0,0,0,0";

/// 预乘 → 非预乘（上游 canvas `getImageData` 的那一份域）。
///
/// alpha 0 与 255 两档直接返回原值：255 那一档 `premultiply` 本来就跳过（恒等）；0 那一档
/// `premultiply` 已经把颜色乘成 0，除回去会得到 `0/0` —— 而**全透的像素没有"原色"可读**，
/// canvas 的 `getImageData` 在那里交的也是 `(0,0,0,0)`（同一档答案，不是猜的：
/// `crates/sisf-media/src/image.rs:113-124` 的 `*p = (c*a+127)/255` 在 `a == 0` 时就是 0）。
fn straight(px: [u8; 4]) -> [u8; 4] {
    let [r, g, b, a] = px;
    if a == 0 || a == 255 {
        return px;
    }
    let a16 = u32::from(a);
    let back = |c: u8| -> u8 {
        let v = (u32::from(c) * 255 + a16 / 2) / a16;
        v.min(255) as u8
    };
    [back(r), back(g), back(b), a]
}

/// URL 拼写 → VFS 拼写（宽松的百分号解码：不是 `%XX` 十六进制的原样留着）。
fn decoded(url: &str) -> String {
    let bytes = url.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = hex(bytes[i + 1]);
            let lo = hex(bytes[i + 2]);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some((b - b'a') + 10),
        b'A'..=b'F' => Some((b - b'A') + 10),
        _ => None,
    }
}

/// 夹具：一张 **2×2 RGBA** 的真 PNG，四颗像素是量出来的四档：
/// `(255,0,0,255)`（纯红，alpha 255 ⇒ 预乘跳过）、`(0,255,0,255)`、`(0,0,255,255)`、
/// `(128,64,32,128)`（**半透明那一档**：`decode_image` 会把它预乘成 `(64,32,16,128)`，
/// 而 [`Rasters::pixel`] 必须除回 `(128,64,32,128)` —— 这一颗像素就是那条域的判据）。
///
/// 它走的是 `png` 那条编码腿（dev-dependency），而读的那一侧走的是**生产的同一条**
/// [`sisf_media::image::decode_image`] ⇒ 不留第二个真相。
#[cfg(test)]
pub fn sample_png_2x2() -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut encoder = png::Encoder::new(std::io::Cursor::new(&mut buf), 2, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("PNG 头写不出去");
        writer
            .write_image_data(&[
                255, 0, 0, 255, // 0,0
                0, 255, 0, 255, // 1,0
                0, 0, 255, 255, // 0,1
                128, 64, 32, 128, // 1,1 —— 半透明那一档
            ])
            .expect("PNG 像素写不出去");
    }
    buf
}

/// 夹具：**768×576** 的真 PNG —— 那是**图块组那一张的形状**（`img/tilesets/World_A1.png` 实测
/// 768×576 = 16×12 格 × 48，`AGENTS.md` 上游事实卡第 1 条）。它存在的理由只有一条：
/// `Tilemap` 画图块走的是 `Bitmap.bltImage`，而**那道闸比的是源那一边**
/// （`sx + sw <= source.width && sy + sh <= source.height`，`js/core_bitmap.js:258-262`）——
/// 拿上面那颗 2×2 的样品当图块组时**一格图块都画不出来**（`M10.md` 事实 107 在图块这一族的第二次应用）。
/// 像素是四条 48 高的横带轮色，**内容不参与任何判据**（这一格数的是操作流），但**尺寸必须是真的**，
/// 否则"源不够大"那一档就量不出来。
#[cfg(test)]
pub fn sample_tileset_png() -> Vec<u8> {
    // 四条 48 高的横带，颜色轮流（像素内容不参与任何判据 —— 这一格数的是**操作**，不是像素）。
    let mut pixels = Vec::with_capacity((768 * 576 * 4) as usize);
    let bands: [[u8; 4]; 4] = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 0, 255],
    ];
    for y in 0..576u32 {
        pixels.extend_from_slice(&bands[(y / 48) as usize % 4]);
        for _ in 1..768 {
            pixels.extend_from_slice(&[0, 0, 0, 0]);
        }
    }
    let mut buf = Vec::new();
    {
        let mut encoder = png::Encoder::new(std::io::Cursor::new(&mut buf), 768, 576);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("PNG 头写不出去");
        writer.write_image_data(&pixels).expect("PNG 像素写不出去");
    }
    buf
}

#[cfg(test)]
mod tests {
    //! 这一族的判据只用**夹具 VFS**（这一颗 crate 不许知道某一款游戏在哪）；
    //! 真素材那一条（窗皮那 32 格的 `getPixel`）住在 `sisf_pray/src/compat.rs`。
    use super::*;
    use sisf_asset::MemoryProvider;

    // 「这一族的规矩」第 1 条：wasm32 上 `#[test]` **不登记**（只有 `#[wasm_bindgen_test]` 会被
    // runner 认），所以不带这一行的话这五条判据在 wasm 腿上"不存在"，而闸门 ⑤ 的名单差集会
    // 多出五条 —— 同一课在 `M10.md` 里记过（本轮第一次写 raster 那五条时就是这样红的）。
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    fn vfs_with(entries: &[(&str, Vec<u8>)]) -> Rc<Vfs> {
        let mut mem = MemoryProvider::new();
        for (name, bytes) in entries {
            mem.insert(&VfsPath::new(name).unwrap(), bytes);
        }
        let mut vfs = Vfs::new();
        vfs.register(Box::new(mem));
        Rc::new(vfs)
    }

    fn fixture() -> Rc<Rasters> {
        Rasters::new(vfs_with(&[("img/system/Window.png", sample_png_2x2())]))
    }

    /// 一颗 2×2 的画布（**4 像素** —— 预算单位就是像素，所以这一颗正好是"一格"）。
    /// 内容只用来区分谁是谁，淘汰判据读的是 `filtered` 那一名。
    fn canvas(seed: u8) -> RgbaImage {
        RgbaImage::new(2, 2, vec![seed; 16])
    }

    #[test]
    fn the_size_answers_the_png_header_and_nothing_else() {
        let r = fixture();
        assert_eq!(r.size("img/system/Window.png"), "2,2");
        // 读不到的那一条交**空串**（JS 那侧据此走 `_onError` 那条腿）。
        assert_eq!(r.size("img/system/Nope.png"), "");
        // 负缓存：问第二次还是同一个数，且不会把缺失当命中。
        assert_eq!(r.size("img/system/Nope.png"), "");
        assert_eq!(r.size("img/system/Window.png"), "2,2");
    }

    #[test]
    fn the_pixel_read_is_un_premultiplied_like_the_canvas() {
        let r = fixture();
        assert_eq!(r.pixel("0|0|img/system/Window.png"), "255,0,0,255");
        assert_eq!(r.pixel("1|0|img/system/Window.png"), "0,255,0,255");
        assert_eq!(r.pixel("0|1|img/system/Window.png"), "0,0,255,255");
        // 半透明那一颗：`decode_image` 交出的是预乘 (64,32,16,128)，这里必须除回原色。
        assert_eq!(r.pixel("1|1|img/system/Window.png"), "128,64,32,128");
        // 越界与坏形状都交透明黑，不抛（上游的同一档）。
        assert_eq!(r.pixel("9|9|img/system/Window.png"), "0,0,0,0");
        assert_eq!(r.pixel("garbage"), "0,0,0,0");
        assert_eq!(r.pixel("x|y|img/system/Window.png"), "0,0,0,0");
    }

    #[test]
    fn a_percent_encoded_url_finds_the_same_bytes_as_the_plain_name() {
        // 上游 `requestBitmap` 交的是 `encodeURIComponent(filename)` 那一族的拼写
        // （`rpg_managers.js:1064`），而 `loadNormalBitmap`（`:888`）交的是已经解过的那一种。
        let r = Rasters::new(vfs_with(&[("img/pictures/格力姆.png", sample_png_2x2())]));
        // 先钉** plainly 拼写**本身读得到（否则下面那一格红了分不清是解码器坏还是夹具坏）。
        assert_eq!(r.size("img/pictures/格力姆.png"), "2,2");
        assert_eq!(
            r.size("img/pictures/%E6%A0%BC%E5%8A%9B%E5%A7%86.png"),
            "2,2",
            "两种拼写必须找到同一份字节（`格力姆` 的 `encodeURIComponent` 就是这一串）"
        );
        // 而"不是合法 `%XX`"的那一种原样留着（真名叫 `50%.png` 的素材不会被解坏）。
        assert_eq!(r.size("img/pictures/50%.png"), "");
    }

    #[test]
    fn eviction_loses_nothing_because_the_disk_content_is_immutable() {
        // 预算单位是**像素**（2×2 = 4 像素）。这一格把预算压到 6 ⇒ 装下第二条就必须挤掉第一条。
        // 生产里那个数是上游的 `ImageCache.limit`（判据 `the_pixel_budget_is_the_upstream_cache_limit`
        // 把它与 JS 侧那一格对表），这里换小数只是为了让淘汰循环真的走到。
        let r = Rasters::with_budget(
            vfs_with(&[("a.png", sample_png_2x2()), ("b.png", sample_png_2x2())]),
            6,
        );
        assert_eq!(r.size("a.png"), "2,2");
        assert_eq!(r.held.borrow().len(), 1);
        assert_eq!(r.pixel("1|1|b.png"), "128,64,32,128");
        // `a` 被挤掉了 —— 但读数**不变**：键就是内容所在的那条路径，重新解一遍而已。
        assert_eq!(r.pixel("1|1|a.png"), "128,64,32,128");
        assert_eq!(
            r.held.borrow().len(),
            1,
            "挤掉之后重新读，仍然只留得下最新那一条"
        );
        assert_eq!(
            *r.pixels.borrow(),
            4,
            "面积账要跟着走（加一条记一次、挤一条扣一次）"
        );
        // 尺寸表不受淘汰影响（它是"每颗位图都要被问"的那一格）。
        assert_eq!(r.size("a.png"), "2,2");
        assert_eq!(r.size("b.png"), "2,2");
    }

    /// **画布预算的两档淘汰资格**（M10.5 第四格第四小格）。合成出来的画布可以挤掉（重放就回来），
    /// 色调写过的那一族永远不许挤掉（那等于把色调抹回素材原样，上游的 canvas 不会这样丢东西）；
    /// 全被色调占着时合成**拒**并点名 `undelivered:over-budget`，而色调自己超预算是**照落**并多占
    /// —— 唯一的色调上限是颗数 [`OWNED_LIMIT`]（真数据 41 颗 + 余量，`bitmap-filters.py` 第③段）。
    #[test]
    fn the_canvas_budget_evicts_canvases_and_never_a_tone() {
        // 预算压成 4 像素（正好一颗 2×2）：生产里那一档是 `SURFACE_BUDGET` = 上游
        // `ImageCache.limit`，这里换小数只为让淘汰循环真的走到（同上面两条预算判据的理由）。
        let r = Rasters::with_surface_budget(
            vfs_with(&[("img/system/Window.png", sample_png_2x2())]),
            u64::MAX,
            4,
        );
        assert_eq!(
            r.put_surface("1", canvas(9), false).unwrap(),
            Vec::<String>::new(),
            "第一颗合成进来时没人可挤"
        );
        // 色调那一次进来 ⇒ 挤掉的必须是**可重放**的那一条。
        assert_eq!(
            r.put_surface("2", canvas(11), true).unwrap(),
            vec!["1".to_string()],
            "淘汰资格只看「可不可重放」：色调那一族是候选名单外的"
        );
        assert!(r.surface("1").is_none());
        assert!(
            r.is_filtered("2"),
            "像素写回碰过的号要留名 —— 它是候选名单的依据"
        );
        // 预算全被色调占着：合成**拒**（调用方回到"没有源"+ 计数），色调一条都不动。
        assert_eq!(
            r.put_surface("3", canvas(13), false),
            Err("undelivered:over-budget")
        );
        assert!(r.surface("2").is_some());
        // 而色调那一档自己超预算是「照落」：宁可多占，不许丢色调。
        assert_eq!(
            r.put_surface("3", canvas(13), true).unwrap(),
            Vec::<String>::new()
        );
        assert!(
            r.surface("2").is_some() && r.surface("3").is_some(),
            "两颗色调都在 ⇒ 面积账超预算是设计，不是漏"
        );
        assert_eq!(
            *r.surface_pixels.borrow(),
            8,
            "超预算的那一格要看得见（挤不出就别挤，别静默少落一笔）"
        );
    }

    #[test]
    fn a_hit_moves_the_entry_to_the_back_of_the_lru() {
        let r = Rasters::with_budget(
            vfs_with(&[
                ("a.png", sample_png_2x2()),
                ("b.png", sample_png_2x2()),
                ("c.png", sample_png_2x2()),
            ]),
            8,
        );
        assert_eq!(r.size("a.png"), "2,2");
        assert_eq!(r.size("b.png"), "2,2");
        // 命中 `a` 之后再装 `c`：该被挤掉的是**最久没被问**的 `b`，不是 `a`。
        assert_eq!(r.pixel("0|0|a.png"), "255,0,0,255");
        assert_eq!(r.size("c.png"), "2,2");
        assert_eq!(
            r.held.borrow().len(),
            2,
            "预算 8 像素 ⇒ 三条 4 像素的样品只能留下两条"
        );
        assert!(
            r.held.borrow().contains_key("a.png") && r.held.borrow().contains_key("c.png"),
            "挤掉的是**最久没被问**的那一条（`b`）—— 刚命中过的 `a` 必须还在（LRU 序没跟着命中走）"
        );
        assert!(!r.held.borrow().contains_key("b.png"));
        assert_eq!(
            r.order
                .borrow()
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .as_slice(),
            &["a.png", "c.png"],
            "LRU 队列本身就是序（front = 下一个被挤掉的）"
        );
    }
}
