//! T5 golden：离屏 544×416 画一组受控 sprite，读回像素后做两件事
//! —— 语义断言（翻转/锚点/旋转/透明度/tone/加法/z 序/裁剪各自对不对），
//! 以及与基线 PNG 的逐通道比对。
//!
//! 只在本机跑：wgpu 30 没有软件光栅后端，离屏同样要真 adapter（见 `docs/platforms.md`），
//! 所以整个文件 `#![cfg(windows)]`；跨平台的无窗口冒烟由 `sisf-render/tests/readback.rs` 负责。
//!
//! 基线缺失或 `SISF_GOLDEN=bless` 时写出 `tests/golden/sprite_scene.png` 并通过，
//! 否则读基线比对，单通道误差 > [`TOL`] 即失败并打印第一处差异。
//!
//! 采样点仍一律取在象限/单元格中心：T6 起装箱带 1px gutter，边界的半纹素渗色已经没了
//! （基线为此重写过，见 `docs/sprite.md`），但旋转/缩放的格点取整本身就有 ±1 抖动，
//! 取格心最稳。

use std::io::Cursor;
use std::path::{Path, PathBuf};

use sisf_render::{
    Atlas, AtlasPages, BatchList, Bitmap, Blend, FrameStatus, FrameU, PROJ_STACK_DEPTH, Renderer,
    UploadRing, ortho,
};
use sisf_sprite::{Layer, Rect, Sprite, SpriteArena, Tone, Vec2, ViewportRef};

const W: u32 = 544;
const H: u32 = 416;
/// 逐通道容差（8bit 字节）。sRGB 编解码往返本身就有 ±1 的量化抖动。
const TOL: i32 = 1;

/// 单元格 64×64：象限中心偏移（每象限 32×32）与格心。
const QA: u32 = 16;
const QB: u32 = 48;
const MID: u32 = 32;
const HALF: f32 = 32.0;
/// 列原点与行原点（格间留 32px 空隙，基线图好读）。
const COL: [f32; 5] = [32.0, 128.0, 224.0, 320.0, 416.0];
const ROW: [f32; 4] = [32.0, 128.0, 224.0, 320.0];

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];

/// 源象限与整图（位图像素）。
const TL: Rect = Rect::new(0.0, 0.0, 4.0, 4.0);
const TR: Rect = Rect::new(4.0, 0.0, 4.0, 4.0);
const BR: Rect = Rect::new(4.0, 4.0, 4.0, 4.0);
const WHOLE: Rect = Rect::new(0.0, 0.0, 8.0, 8.0);
/// 整图 8 纹素 → 64 像素用 8x；子矩形 4 纹素 → 64 像素用 16x。
const S8: f32 = 8.0;
const S16: f32 = 16.0;

fn golden_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/sprite_scene.png")
}

/// `8×8` 四象限标记图（预乘 RGBA8；`a=255` 时预乘即原值）。
fn marker_pixels() -> Vec<u8> {
    (0..64u32)
        .flat_map(|i| match (i % 8 < 4, i / 8 < 4) {
            (true, true) => RED,
            (false, true) => GREEN,
            (true, false) => BLUE,
            (false, false) => WHITE,
        })
        .collect()
}

/// 整栈同投影的帧 uniform：本场景只用 layer 0 与 layer 2，两者都是全屏 ortho。
fn frame() -> FrameU {
    let proj = ortho(W as f32, H as f32);
    FrameU::new(&[proj; PROJ_STACK_DEPTH], 0.0)
}

/// 从位图取 `src`、按 `scale` 放大、左上锚点落在 `(x, y)`。
fn cell(bitmap: &Bitmap, x: f32, y: f32, src: Rect, scale: f32) -> Sprite {
    Sprite {
        bitmap: Some(bitmap.clone()),
        src,
        pos: Vec2::new(x, y),
        scale: Vec2::new(scale, scale),
        ..Default::default()
    }
}

/// 摆出全部用例（每格 64×64，格子坐标见 `COL`/`ROW`，断言见 `check_semantics`）。
fn spawn_scene(bitmap: &Bitmap, arena: &mut SpriteArena) {
    // 第一行：基准 / flip_x / flip_y / 底边锚点 / 不可见。
    arena.spawn(cell(bitmap, COL[0], ROW[0], WHOLE, S8));
    arena.spawn(Sprite {
        flip_x: true,
        ..cell(bitmap, COL[1], ROW[0], WHOLE, S8)
    });
    arena.spawn(Sprite {
        flip_y: true,
        ..cell(bitmap, COL[2], ROW[0], WHOLE, S8)
    });
    // 底边中点锚点：pos 落在单元格底边中点，画出来的矩形必须与基准格全等。
    arena.spawn(Sprite {
        anchor: Vec2::new(0.5, 1.0),
        ..cell(bitmap, COL[3] + HALF, ROW[0] + 2.0 * HALF, WHOLE, S8)
    });
    arena.spawn(Sprite {
        visible: false,
        ..cell(bitmap, COL[4], ROW[0], WHOLE, S8)
    });

    // 第二行：透明度 / tone 偏移 / tone 灰度 / 绕中心旋转。
    arena.spawn(Sprite {
        opacity: 0.5,
        ..cell(bitmap, COL[0], ROW[1], BR, S16)
    });
    arena.spawn(Sprite {
        tone: Tone::offset([0.0, 0.5, 0.0], 0.0),
        ..cell(bitmap, COL[1], ROW[1], TL, S16)
    });
    arena.spawn(Sprite {
        tone: Tone::offset([0.0, 0.0, 0.0], 1.0),
        ..cell(bitmap, COL[2], ROW[1], TL, S16)
    });
    arena.spawn(Sprite {
        anchor: Vec2::CENTER,
        rotation: std::f32::consts::FRAC_PI_2,
        ..cell(bitmap, COL[3] + HALF, ROW[1] + HALF, WHOLE, S8)
    });

    // 第三行：加法混合对 / 普通过渡对 / z 序 / `src` 越界裁剪。
    let add_a = Sprite {
        opacity: 0.5,
        ..cell(bitmap, COL[0], ROW[2], BR, S16)
    };
    arena.spawn(add_a.clone());
    arena.spawn(Sprite {
        blend: Blend::Add,
        z: 1,
        ..add_a
    });
    let over = Sprite {
        opacity: 0.5,
        ..cell(bitmap, COL[1], ROW[2], BR, S16)
    };
    arena.spawn(over.clone());
    arena.spawn(over);
    // 先 spawn 的 z 大 → 排序后仍在后面 → 盖住后 spawn 的绿。
    arena.spawn(Sprite {
        z: 5,
        ..cell(bitmap, COL[2], ROW[2], TL, S16)
    });
    arena.spawn(Sprite {
        z: -5,
        ..cell(bitmap, COL[2], ROW[2], TR, S16)
    });
    // `src` 越界裁进位图：只剩 2×2 源像素 → 32×32 目的像素（不拉伸回 64）。
    arena.spawn(cell(
        bitmap,
        COL[3],
        ROW[2],
        Rect::new(6.0, 6.0, 4.0, 4.0),
        S16,
    ));

    // 第四行：只属于第二层（tag=Layer(2)）的格子，主视口采集取不到它。
    arena.spawn(Sprite {
        layer: Layer(2),
        ..cell(bitmap, COL[3], ROW[3], WHOLE, S8)
    });
}

fn px(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

/// 单元格 `(x, y)` 的四个象限中心（左上、右上、左下、右下）。
fn quad(buf: &[u8], x: u32, y: u32) -> [[u8; 4]; 4] {
    [
        px(buf, x + QA, y + QA),
        px(buf, x + QB, y + QA),
        px(buf, x + QA, y + QB),
        px(buf, x + QB, y + QB),
    ]
}

fn near(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter()
        .zip(&b)
        .all(|(x, y)| (*x as i32 - *y as i32).abs() <= TOL)
}

/// 某通道是否落在线性值 `v` 对应的 sRGB 字节附近。
fn is_linear(buf: &[u8], x: u32, y: u32, channel: usize, v: f32, tol: i32) -> bool {
    (px(buf, x, y)[channel] as i32 - srgb_byte(v) as i32).abs() <= tol
}

/// RGB 三通道都等于线性值 `v`（灰/白像素）。
fn is_gray(buf: &[u8], x: u32, y: u32, v: f32, tol: i32) -> bool {
    (0..3).all(|c| is_linear(buf, x, y, c, v, tol))
}

/// 线性 → 8bit sRGB 字节（着色器/格式那条换算的逆运算，期望因此能写成线性值）。
fn srgb_byte(v: f32) -> u8 {
    let e = if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (e.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// 语义断言：每条独立于基线文件，翻转/锚点/z 序写反时直接指出是哪一条。
fn check_semantics(buf: &[u8]) {
    let (r0, r1, r2, r3) = (ROW[0] as u32, ROW[1] as u32, ROW[2] as u32, ROW[3] as u32);
    let (c0, c1, c2, c3, c4) = (
        COL[0] as u32,
        COL[1] as u32,
        COL[2] as u32,
        COL[3] as u32,
        COL[4] as u32,
    );
    // 第一行：基准 左上红 / 右上绿 / 左下蓝 / 右下白。
    assert_eq!(quad(buf, c0, r0), [RED, GREEN, BLUE, WHITE], "基准四象限");
    assert_eq!(quad(buf, c1, r0), [GREEN, RED, WHITE, BLUE], "flip_x 换列");
    assert_eq!(quad(buf, c2, r0), [BLUE, WHITE, RED, GREEN], "flip_y 换行");
    assert_eq!(
        quad(buf, c3, r0),
        [RED, GREEN, BLUE, WHITE],
        "anchor(0.5,1) 的落点必须与左上锚点同格"
    );
    assert!(
        quad(buf, c4, r0).iter().all(|p| near(*p, BLACK)),
        "visible=false 必须完全不画"
    );
    // 第二行：0.5 白（线性 0.5 → ≈188）；目标本就不透明，混完 alpha 仍是 255。
    let faded = px(buf, c0 + MID, r1 + MID);
    assert!(
        is_gray(buf, c0 + MID, r1 + MID, 0.5, 8) && faded[3] == 255,
        "opacity=0.5 读回 {faded:?}"
    );
    // tone 偏移：红 + (0,0.5,0) → R 饱和、G≈188、B 仍 0。
    let tinted = px(buf, c1 + MID, r1 + MID);
    assert!(
        tinted[0] > 250 && is_linear(buf, c1 + MID, r1 + MID, 1, 0.5, 8) && tinted[2] < 5,
        "tone offset 读回 {tinted:?}"
    );
    // tone 灰度：红的亮度 0.2126（线性）→ ≈127，三通道相等。
    let grayed = px(buf, c2 + MID, r1 + MID);
    assert!(
        is_gray(buf, c2 + MID, r1 + MID, 0.2126, 8)
            && near(grayed, [grayed[0], grayed[0], grayed[0], 255]),
        "tone gray 读回 {grayed:?}"
    );
    // 绕中心转 90°（y 向下）：屏幕左上取到内容的左下。
    assert_eq!(
        quad(buf, c3, r1),
        [BLUE, RED, WHITE, GREEN],
        "rotation 90° 的象限置换"
    );
    // 第三行：加法 0.5+0.5=1.0 线性；普通过渡 0.5 over 0.5 = 0.75。
    let add = px(buf, c0 + MID, r2 + MID);
    let over = px(buf, c1 + MID, r2 + MID);
    assert!(
        add[0] > 250 && near(add, [add[0], add[1], add[2], 255]),
        "additive 应饱和，读回 {add:?}"
    );
    assert!(
        is_gray(buf, c1 + MID, r2 + MID, 0.75, 8),
        "alpha-over 两层 0.5 应是 0.75 线性，读回 {over:?}"
    );
    assert_eq!(px(buf, c2 + MID, r2 + MID), RED, "z=5 的红该盖住 z=-5 的绿");
    // 裁剪：`src` 越界只剩 2×2 源像素 → 只覆盖单元格左上 32×32。
    assert_eq!(px(buf, c3 + 8, r2 + 8), WHITE, "裁剪后剩下的内容还在");
    assert_eq!(px(buf, c3 + QB, r2 + 8), BLACK, "裁掉的那半不该被拉伸回来");
    // 第二层（tag=Layer(2)）单独采集后完整出现，同行左侧仍空着。
    assert_eq!(
        quad(buf, c3, r3),
        [RED, GREEN, BLUE, WHITE],
        "layer 2 的精灵该被 HUD 采集"
    );
    assert!(
        quad(buf, c0, r3).iter().all(|p| near(*p, BLACK)),
        "第四行第一列本就没有精灵"
    );
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

/// 比对或刷新基线。
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
         确认是预期变更后用 `SISF_GOLDEN=bless cargo test -p sisf-sprite --test golden_sprite` 刷新",
    );
}

#[test]
fn sprite_scene_golden() {
    let mut renderer = pollster::block_on(Renderer::new_offscreen(W, H))
        .expect("离屏渲染器需要可用 adapter（DX12/Vulkan）");
    let atlas = Atlas::new();
    let bitmap = atlas
        .new_bitmap(8, 8, marker_pixels(), 0)
        .expect("8x8 标记图装箱");
    let mut pages = AtlasPages::new();
    let mut uploads = UploadRing::new();
    uploads.queue(bitmap.clone());

    let mut arena = SpriteArena::new();
    spawn_scene(&bitmap, &mut arena);

    renderer.begin_frame(&frame(), [0.0, 0.0, 0.0, 1.0]);
    renderer.apply_atlas(&atlas, &mut pages, &mut uploads);
    // 两个可视口采同一个 arena，产物并入同一帧。
    let mut main = BatchList::new();
    let mut hud = BatchList::new();
    arena.collect(ViewportRef::MAIN, &mut main).unwrap();
    arena.collect(ViewportRef::new(Layer(2)), &mut hud).unwrap();
    let instances = (main.len() + hud.len()) as u32;
    renderer.append_list(main);
    renderer.append_list(hud);
    assert!(
        matches!(renderer.end_frame(), FrameStatus::Rendered),
        "离屏帧必须完成"
    );
    let pixels = renderer.readback().expect("读回离屏纹理");
    assert_eq!(pixels.len(), (W * H * 4) as usize);
    assert_eq!(
        renderer.stats().map_or(0, |s| s.instances),
        instances,
        "每个活精灵都该产出一个实例"
    );

    check_semantics(&pixels);
    compare_golden(&pixels);
}
