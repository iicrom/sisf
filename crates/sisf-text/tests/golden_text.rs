//! T6 golden：把 12/16/24 三档字号的中英混排文字画进离屏目标，读回像素做两件事
//! —— 语义断言（每一档都有墨迹、行高随字号增长、声明盒外不许脏），
//! 以及与基线 PNG 的逐通道比对。
//!
//! 与 `sisf-sprite` 的 golden 同一条通路（`Renderer::new_offscreen` + `readback`），
//! 差别只在实例带 `mask = 1.0`：所以这份基线同时是"文本仍是普通 sprite quad"的证据。
//!
//! 整个文件 `#![cfg(windows)]`：wgpu 30 没有软件光栅后端，离屏同样要真 adapter；
//! 而中文那一半靠系统字体（`FontBundle` 的自带字体只有拉丁），换平台期望值就变。
//! 基线缺失或 `SISF_GOLDEN=bless` 时写出 `tests/golden/text_scene.png` 并通过。

#![cfg(windows)]

use std::io::Cursor;
use std::path::{Path, PathBuf};

use sisf_render::{
    Atlas, AtlasPages, FrameStatus, FrameU, PROJ_STACK_DEPTH, Renderer, Source, UploadRing, ortho,
};
use sisf_text::{Align, TextEngine, TextStyle};

const W: u32 = 320;
const H: u32 = 120;
/// 逐通道容差（8bit 字节）：字形覆盖率是线性量，采样/编码往返有 ±1 抖动。
const TOL: i32 = 2;

/// 背景（不透明黑）：任何偏离都算"墨迹"。
const BG: [u8; 4] = [0, 0, 0, 255];

/// 三档字号各占一条横幅（y 起点、字号）；横幅宽 `BAND_W`，右侧留一条干净的列带。
const BANDS: [(u32, f32); 3] = [(0, 12.0), (32, 16.0), (64, 24.0)];
const BAND_W: f32 = 180.0;
const TEXT: &str = "Ag 引擎 123";

/// 右半那条是"画进位图再 blt 上屏"的通路（T6 的 RT + mask 采样）。
const RT: (u32, u32, u32, u32) = (200, 64, 112, 48);
const RT_SIZE: [u32; 2] = [112, 48];

fn golden_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/text_scene.png")
}

fn px(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

fn near(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter()
        .zip(&b)
        .all(|(x, y)| (*x as i32 - *y as i32).abs() <= TOL)
}

/// `[x0,x1) × [y0,y1)` 内的墨迹上下边界（首尾含端点）；无墨迹返回 `None`。
fn ink_rows(buf: &[u8], box_: (u32, u32, u32, u32)) -> Option<(u32, u32)> {
    let (x0, y0, w, h) = box_;
    let (x1, y1) = (x0 + w, (y0 + h).min(H));
    let mut rows: Option<(u32, u32)> = None;
    for y in y0..y1 {
        for x in x0..x1 {
            if !near(px(buf, x, y), BG) {
                let (_, bottom) = rows.get_or_insert((y, y));
                *bottom = y;
                break;
            }
        }
    }
    rows
}

/// 墨迹的水平跨度，用来验证对齐确实把文字挪了。
fn ink_cols(buf: &[u8], box_: (u32, u32, u32, u32)) -> Option<(u32, u32)> {
    let (x0, y0, w, h) = box_;
    let (x1, y1) = (x0 + w, (y0 + h).min(H));
    let mut cols: Option<(u32, u32)> = None;
    for x in x0..x1 {
        for y in y0..y1 {
            if !near(px(buf, x, y), BG) {
                let (_, right) = cols.get_or_insert((x, x));
                *right = x;
                break;
            }
        }
    }
    cols
}

fn frame() -> FrameU {
    let proj = ortho(W as f32, H as f32);
    FrameU::new(&[proj; PROJ_STACK_DEPTH], 0.0)
}

fn encode(png_path: &Path, pixels: &[u8]) {
    let mut file = std::fs::File::create(png_path).expect("创建基线文件");
    let mut encoder = png::Encoder::new(&mut file, W, H);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .expect("PNG 头")
        .write_image_data(pixels)
        .expect("写像素");
}

fn decode(png_path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(png_path).expect("读基线文件");
    let mut reader = png::Decoder::new(Cursor::new(bytes))
        .read_info()
        .expect("基线不是合法 PNG");
    let mut buf = vec![0u8; reader.output_buffer_size().expect("PNG 缓冲尺寸可知")];
    let info = reader.next_frame(&mut buf).expect("读基线像素");
    assert_eq!(info.color_type, png::ColorType::Rgba, "基线必须是 RGBA8");
    assert_eq!((info.width, info.height), (W, H), "基线尺寸不符");
    buf.truncate(info.buffer_size());
    buf
}

fn compare_golden(pixels: &[u8]) {
    let path = golden_path();
    let bless = std::env::var("SISF_GOLDEN").is_ok_and(|v| v == "bless");
    if bless || !path.exists() {
        std::fs::create_dir_all(path.parent().unwrap()).expect("建 golden 目录");
        encode(&path, pixels);
        eprintln!("已写入基线 {}", path.display());
        return;
    }
    let base = decode(&path);
    assert_eq!(base.len(), pixels.len(), "基线字节数不符");
    let mut worst = 0i32;
    let mut first: Option<(u32, u32, [u8; 4], [u8; 4])> = None;
    for (i, (a, b)) in base.chunks_exact(4).zip(pixels.chunks_exact(4)).enumerate() {
        let (a, b) = ([a[0], a[1], a[2], a[3]], [b[0], b[1], b[2], b[3]]);
        for c in 0..4 {
            worst = worst.max((a[c] as i32 - b[c] as i32).abs());
        }
        if first.is_none() && !near(a, b) {
            first = Some(((i as u32 % W), (i as u32 / W), a, b));
        }
    }
    assert!(
        worst <= TOL,
        "与基线最大单通道误差 {worst} > {TOL}，首处差异 (x,y,基线,当前) = {first:?}；\
         确认是预期变更后用 `cargo xtask golden --bless` 刷新",
    );
}

/// 画一条三档字号 + 一块位图文字的 scene，返回读回像素。
///
/// 三处都套了 `push_clip`：既是 T6 裁剪通路的验收，也让"条带外必须干净"这条
/// 断言与字形宽度无关（`draw_text` 本身不裁剪，24px 那串字会越出 180px 的盒）。
fn render_scene() -> Vec<u8> {
    let mut renderer = pollster::block_on(Renderer::new_offscreen(W, H))
        .expect("离屏渲染器需要可用 adapter（DX12/Vulkan）");
    let mut text = TextEngine::from_bytes(FONT);
    // 位图通路：一张 RT 当画布，会话里画字，再 blt 上屏。
    let atlas = Atlas::with_limits(256, 6);
    let target = atlas
        .new_target(RT_SIZE[0], RT_SIZE[1], 0)
        .expect("RT 装箱");
    let mut pages = AtlasPages::new();
    let mut uploads = UploadRing::new();
    renderer.apply_atlas(&atlas, &mut pages, &mut uploads);

    // 会话：覆盖率页在字形落位之后、pass 记录之前才注册（`sync` 每帧调，见 `docs/text.md`）。
    renderer.begin_draw(&Source::from(&target)).expect("进会话");
    renderer.fill_rect([0.0, 0.0, RT_SIZE[0] as f32, 2.0], [1.0, 1.0, 1.0, 1.0]);
    let clip = renderer.push_clip(0, [0.0, 4.0, RT_SIZE[0] as f32, RT_SIZE[1] as f32 - 8.0]);
    assert_ne!(clip, 0, "会话里也该有裁剪代号");
    let boxed = text.draw_text(
        &mut renderer,
        [0.0, 4.0, RT_SIZE[0] as f32, RT_SIZE[1] as f32 - 8.0],
        "会话字",
        &TextStyle::new(16.0).aligned(Align::Center),
        [1.0, 1.0, 1.0, 1.0],
    );
    assert!(boxed > 0, "会话里也该画出字来");
    renderer.pop_clip();
    text.sync(&mut renderer);
    renderer.end_draw().expect("收会话");

    renderer.begin_frame(&frame(), [0.0, 0.0, 0.0, 1.0]);
    for (top, size) in BANDS {
        let band = [8.0, top as f32, BAND_W, 32.0];
        renderer.push_clip(0, band);
        let drawn = text.draw_text(
            &mut renderer,
            band,
            &format!("{TEXT} {}", size as u32),
            &TextStyle::new(size),
            [1.0, 1.0, 1.0, 1.0],
        );
        renderer.pop_clip();
        assert!(drawn > 5, "{size}px 那档只画出了 {drawn} 个字形");
    }
    // 同一档字号、三种对齐各占一条窄带：居中/右对齐必须把整块往右推。
    for (i, align) in [Align::Left, Align::Center, Align::Right]
        .into_iter()
        .enumerate()
    {
        let box_ = [8.0 + i as f32 * 64.0, 96.0, 56.0, 20.0];
        renderer.push_clip(0, box_);
        text.draw_text(
            &mut renderer,
            box_,
            "RPG",
            &TextStyle::new(16.0).aligned(align),
            [1.0, 1.0, 1.0, 1.0],
        );
        renderer.pop_clip();
    }
    // 位图内容上屏（RT 当普通位图采样）。
    let src = Source::from(&target);
    renderer.blt(
        [RT.0 as f32, RT.1 as f32],
        &src,
        [0, 0, RT_SIZE[0], RT_SIZE[1]],
        255,
    );
    // 本帧新增的字形页在记录前注册（上面会话已建第一页，这里覆盖"会话外又长了字"的情况）。
    text.sync(&mut renderer);
    assert!(
        matches!(renderer.end_frame(), FrameStatus::Rendered),
        "离屏帧必须完成"
    );
    renderer.readback().expect("读回离屏纹理")
}

/// 语义断言：与基线文件无关，字号/对齐/位图通路任一条写反时直接指出是哪一条。
fn check_semantics(buf: &[u8]) {
    let mut heights = Vec::new();
    for (top, size) in BANDS {
        let box_ = (8, top, BAND_W as u32, 32);
        let (first, last) = ink_rows(buf, box_)
            .unwrap_or_else(|| panic!("{size}px 那档一个像素都没画出来（中文缺字体？）"));
        heights.push(last - first + 1);
        assert!(
            first >= top && last < top + 32,
            "{size}px 的墨迹越出了本档横幅: {first}..{last}"
        );
    }
    // 字号 12→24 ⇒ 同一串文字的墨迹高度必须单调增长（覆盖率矩形按字号光栅）。
    assert!(
        heights[0] < heights[1] && heights[1] < heights[2],
        "墨迹高度没随字号增长: {heights:?}"
    );
    // 对齐：左/中/右三条窄带里，墨迹的起点必须依次右移。
    let starts: Vec<u32> = (0..3)
        .map(|i| {
            ink_cols(buf, (8 + i * 64, 96, 56, 20))
                .unwrap_or((u32::MAX, 0))
                .0
        })
        .collect();
    assert!(
        starts[0] < starts[1] && starts[1] < starts[2],
        "左/中/右对齐的墨迹起点没递增: {starts:?}"
    );
    // 位图通路：RT 上屏那块必须有墨迹（mask 采样经过 RT 的 sRGB 页仍是白字）。
    assert!(
        ink_rows(buf, (RT.0, RT.1, RT_SIZE[0], RT_SIZE[1])).is_some(),
        "位图里的文字 blt 上屏后不见了"
    );
    // 声明盒之外的像素必须还是背景：横幅之间不许互相污染。
    for y in (0..H).step_by(3) {
        for x in (192..200).step_by(2) {
            assert!(
                near(px(buf, x, y), BG),
                "列带 ({x},{y}) 本不该有东西，读回 {:?}",
                px(buf, x, y)
            );
        }
    }
}

#[test]
fn text_scene_golden() {
    let buf = render_scene();
    check_semantics(&buf);
    compare_golden(&buf);
}

/// 自带字体（Inter，OFL）：拉丁那一半由它承担，中文交给系统字体回退。
const FONT: &[u8] = include_bytes!("../tests/fixtures/Inter-Regular.ttf");
