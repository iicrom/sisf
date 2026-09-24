//! T4 图像解码：按魔数分派 png / jpeg-decoder / image-webp / 手写 BMP / gif。
//!
//! 输出统一 RGBA8（sRGB 数值域）且**预乘 alpha**——着色器里的 tone/opacity 全部
//! 按预乘语义走（见 `shaders/sprite.wgsl`），解码边界做一次最省事。
//! feature 缺位时返回明确的 `ImageError::FeatureOff`（DoD-4：不留空实现）。

use std::io::Cursor;

/// 一帧解码结果：RGBA8、预乘、行主序，`pixels.len() == width*height*4`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl RgbaImage {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        debug_assert_eq!(pixels.len(), (width * height * 4) as usize);
        Self {
            width,
            height,
            pixels,
        }
    }

    /// 单像素（测试与回读比较用）。
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        self.pixels[i..i + 4].try_into().unwrap()
    }
}

/// GIF 一帧：`dur_ms` 来自帧延迟（1/100 秒 → 毫秒），0 表示"尽快下一帧"。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnimationFrame {
    pub image: RgbaImage,
    pub dur_ms: u32,
}

/// 多帧图像的动画定义（T8 的 `AnimState` 直接消费此结构）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnimDef {
    pub frames: Vec<AnimationFrame>,
    /// 0 = 无限循环（GIF 的 NETSCAPE 扩展），其余为重复次数。
    pub loops: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageData {
    Single(RgbaImage),
    Animated(AnimDef),
}

impl ImageData {
    /// 静态图或动画首帧（图集上传的单图路径用）。
    pub fn first(&self) -> &RgbaImage {
        match self {
            Self::Single(img) => img,
            Self::Animated(def) => &def.frames[0].image,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error("空输入，无法识别图像格式")]
    Empty,
    #[error("不是可识别的图像格式（魔数: {magic:?}）")]
    UnknownFormat { magic: [u8; 8] },
    #[error("该格式需启用 feature `{feature}` 后才能解码")]
    FeatureOff { feature: &'static str },
    #[error("解码失败: {0}")]
    Corrupt(String),
    #[error("暂不支持的变体: {0}")]
    Unsupported(String),
}

impl ImageError {
    fn corrupt(e: impl std::fmt::Display) -> Self {
        Self::Corrupt(e.to_string())
    }
}

type Result<T> = std::result::Result<T, ImageError>;

/// 按魔数分派解码。覆盖：PNG / JPEG / WebP / BMP（24·32bpp）/ GIF（feature `gif`）。
pub fn decode_image(bytes: &[u8]) -> Result<ImageData> {
    if bytes.len() < 12 {
        return Err(ImageError::Empty);
    }
    let mut magic = [0u8; 8];
    magic.copy_from_slice(&bytes[..8]);
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return decode_png(bytes);
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return decode_jpeg(bytes);
    }
    if bytes.starts_with(b"RIFF") && bytes[8..12] == *b"WEBP" {
        return decode_webp(bytes);
    }
    if bytes.starts_with(b"BM") {
        return Ok(ImageData::Single(decode_bmp(bytes)?));
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return decode_gif(bytes);
    }
    Err(ImageError::UnknownFormat { magic })
}

/// 非预乘 RGBA8 → 预乘（(c*a+127)/255 四舍五入）。alpha=255 的像素直接跳过。
fn premultiply(pixels: &mut [u8]) {
    for px in pixels.chunks_exact_mut(4) {
        let a = px[3];
        if a == 255 {
            continue;
        }
        let a16 = u32::from(a);
        for p in px[..3].iter_mut() {
            *p = ((u32::from(*p) * a16 + 127) / 255) as u8;
        }
    }
}

fn rgba_from_rgb(rgb: Vec<u8>) -> Vec<u8> {
    debug_assert!(rgb.len().is_multiple_of(3));
    let mut out = Vec::with_capacity(rgb.len() / 3 * 4);
    for px in rgb.chunks_exact(3) {
        out.extend_from_slice(&[px[0], px[1], px[2], 255]);
    }
    out
}

fn rgba_from_gray(gray: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(gray.len() * 4);
    for &g in &gray {
        out.extend_from_slice(&[g, g, g, 255]);
    }
    out
}

#[cfg(feature = "image")]
fn decode_png(bytes: &[u8]) -> Result<ImageData> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    // EXPAND：调色板→RGB、低位深→8bit、tRNS→alpha；STRIP_16：16bit→8bit。
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(ImageError::corrupt)?;
    let buf_len = reader
        .output_buffer_size()
        .ok_or_else(|| ImageError::Corrupt("png: 输出尺寸未知".to_owned()))?;
    let mut buf = vec![0u8; buf_len];
    let info = reader.next_frame(&mut buf).map_err(ImageError::corrupt)?;
    buf.truncate(info.buffer_size());
    let pixels = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => rgba_from_rgb(buf),
        png::ColorType::Grayscale => rgba_from_gray(buf),
        png::ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity(buf.len() * 2);
            for ga in buf.chunks_exact(2) {
                out.extend_from_slice(&[ga[0], ga[0], ga[0], ga[1]]);
            }
            out
        }
        other => {
            return Err(ImageError::Unsupported(format!(
                "png 颜色类型 {other:?}（EXPAND 之后仍非 RGB/RGBA）"
            )));
        }
    };
    let mut img = RgbaImage::new(info.width, info.height, pixels);
    premultiply(&mut img.pixels);
    Ok(ImageData::Single(img))
}

#[cfg(not(feature = "image"))]
fn decode_png(_: &[u8]) -> Result<ImageData> {
    Err(ImageError::FeatureOff { feature: "image" })
}

/// `simd-jpeg`（native）：turbojpeg = libjpeg-turbo 绑定，直出 RGBA。
#[cfg(all(feature = "image", feature = "simd-jpeg", not(target_arch = "wasm32")))]
fn decode_jpeg(bytes: &[u8]) -> Result<ImageData> {
    let image =
        turbojpeg::decompress(bytes, turbojpeg::PixelFormat::RGBA).map_err(ImageError::corrupt)?;
    let mut img = RgbaImage::new(
        u32::try_from(image.width).unwrap(),
        u32::try_from(image.height).unwrap(),
        image.pixels,
    );
    premultiply(&mut img.pixels);
    Ok(ImageData::Single(img))
}

/// 纯 Rust 路径（默认；`simd-jpeg` 在 wasm 上无包可指，也走这里）。
#[cfg(all(
    feature = "image",
    any(not(feature = "simd-jpeg"), target_arch = "wasm32")
))]
fn decode_jpeg(bytes: &[u8]) -> Result<ImageData> {
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    let data = decoder.decode().map_err(ImageError::corrupt)?;
    let info = decoder
        .info()
        .ok_or_else(|| ImageError::Corrupt("jpeg: 解码后无帧信息".to_owned()))?;
    let (w, h) = (u32::from(info.width), u32::from(info.height));
    let pixels = match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => rgba_from_rgb(data),
        jpeg_decoder::PixelFormat::L8 => rgba_from_gray(data),
        other => {
            return Err(ImageError::Unsupported(format!("jpeg 像素格式 {other:?}")));
        }
    };
    let mut img = RgbaImage::new(w, h, pixels);
    premultiply(&mut img.pixels);
    Ok(ImageData::Single(img))
}

#[cfg(not(feature = "image"))]
fn decode_jpeg(_: &[u8]) -> Result<ImageData> {
    Err(ImageError::FeatureOff { feature: "image" })
}

#[cfg(feature = "image")]
fn decode_webp(bytes: &[u8]) -> Result<ImageData> {
    let mut decoder =
        image_webp::WebPDecoder::new(Cursor::new(bytes)).map_err(ImageError::corrupt)?;
    if decoder.is_animated() {
        return Err(ImageError::Unsupported(
            "动画 WebP（多帧 AnimDef 只接了 GIF；此处为单帧语义）".to_owned(),
        ));
    }
    let (w, h) = decoder.dimensions();
    let len = decoder
        .output_buffer_size()
        .ok_or_else(|| ImageError::Corrupt("webp: 输出尺寸未知".to_owned()))?;
    let mut pixels = vec![0u8; len];
    decoder
        .read_image(&mut pixels)
        .map_err(ImageError::corrupt)?;
    let mut img = RgbaImage::new(w, h, pixels);
    premultiply(&mut img.pixels);
    Ok(ImageData::Single(img))
}

#[cfg(not(feature = "image"))]
fn decode_webp(_: &[u8]) -> Result<ImageData> {
    Err(ImageError::FeatureOff { feature: "image" })
}

#[cfg(feature = "gif")]
fn decode_gif(bytes: &[u8]) -> Result<ImageData> {
    // set_color_output 是 &mut self；正确链是先配置再 read_info 拿到 Decoder。
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options
        .read_info(Cursor::new(bytes))
        .map_err(ImageError::corrupt)?;
    let mut frames = Vec::new();
    while let Some(frame) = decoder.read_next_frame().map_err(ImageError::corrupt)? {
        // gif 0.14 已按处置码把子矩形合成成全画布帧。
        let mut img = RgbaImage::new(
            u32::from(frame.width),
            u32::from(frame.height),
            frame.buffer.to_vec(),
        );
        premultiply(&mut img.pixels);
        frames.push(AnimationFrame {
            dur_ms: u32::from(frame.delay) * 10,
            image: img,
        });
    }
    if frames.is_empty() {
        return Err(ImageError::Corrupt("gif: 无帧".to_owned()));
    }
    // 语义映射：NETSCAPE repeat=0 → Infinite（本引擎 loops=0 表示无限）；
    // Finite(n) = 首放后再播 n 次 → 总播放 n+1 次；缺省 Finite(0) → 播一次。
    let loops = match decoder.repeat() {
        gif::Repeat::Infinite => 0,
        gif::Repeat::Finite(n) => u32::from(n) + 1,
    };
    Ok(ImageData::Animated(AnimDef { frames, loops }))
}

#[cfg(not(feature = "gif"))]
fn decode_gif(_: &[u8]) -> Result<ImageData> {
    Err(ImageError::FeatureOff { feature: "gif" })
}

/// 手写 BMP：24/32bpp、BI_RGB 与 BI_BITFIELDS 位掩码、自下而上（负高=自上而下）。
/// 调色板/RLE/16bpp 明确报 Unsupported（移植工程图集中不出现这些变体）。
fn decode_bmp(bytes: &[u8]) -> Result<RgbaImage> {
    let read_u32 = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    let read_i32 = |o: usize| i32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    let read_u16 = |o: usize| u16::from_le_bytes(bytes[o..o + 2].try_into().unwrap());
    if bytes.len() < 54 {
        return Err(ImageError::Corrupt("bmp: 头不完整".to_owned()));
    }
    let pixel_offset = read_u32(10) as usize;
    let dib = read_u32(14);
    if !matches!(dib, 12 | 40 | 56 | 108 | 124) {
        return Err(ImageError::Unsupported(format!("bmp: DIB 头 {dib}")));
    }
    let (width, height) = if dib == 12 {
        (read_u16(18) as i32, read_u16(20) as i32)
    } else {
        (read_i32(18), read_i32(22))
    };
    let bpp = read_u16(28);
    let compression = if dib == 12 { 0 } else { read_u32(30) };
    if width <= 0 || height == 0 {
        return Err(ImageError::Corrupt("bmp: 非法尺寸".to_owned()));
    }
    let (w, h) = (width as u32, height.unsigned_abs());
    let top_down = height < 0;
    if !matches!(compression, 0 | 3) {
        return Err(ImageError::Unsupported(format!(
            "bmp 压缩方式 {compression}（仅 BI_RGB/BI_BITFIELDS）"
        )));
    }
    if !matches!(bpp, 24 | 32) {
        return Err(ImageError::Unsupported(format!(
            "bmp {bpp}bpp（仅 24/32；调色板位图请在源端转换）"
        )));
    }
    // BI_RGB 的 32bpp 第 4 字节未定义 → 按不透明处理（掩码全 0 → alpha 255）。
    let mut masks = [0x00FF_0000u32, 0x0000_FF00, 0x0000_00FF, 0];
    if compression == 3 {
        // BI_BITFIELDS：4 个掩码紧跟 40 字节 NT DIB 核心（dib>=40 时偏移固定 54；
        // dib=56/108/124 只是"核心+掩码(+更多)"，掩码位置不变）。
        let m = if dib == 12 { 26 } else { 54 };
        if bytes.len() < m + 16 {
            return Err(ImageError::Corrupt("bmp: 掩码缺失".to_owned()));
        }
        masks = [
            read_u32(m),
            read_u32(m + 4),
            read_u32(m + 8),
            read_u32(m + 12),
        ];
    }
    let shifts = |mask: u32| -> Option<(u32, u32)> {
        if mask == 0 {
            return None;
        }
        let lo = mask.trailing_zeros();
        let bits = mask.count_ones();
        (mask == (((1u64 << bits) - 1) as u32) << lo).then_some((lo, bits))
    };
    let chans = masks.map(shifts);
    if chans[..3].iter().any(Option::is_none) {
        return Err(ImageError::Unsupported(
            "bmp: RGB 掩码非 8bit 连续".to_owned(),
        ));
    }
    let bytes_pp = (bpp / 8) as usize;
    let row_bytes = (bpp as usize * w as usize).div_ceil(32) * 4;
    if bytes.len() < pixel_offset + row_bytes * h as usize {
        return Err(ImageError::Corrupt("bmp: 像素数据截断".to_owned()));
    }
    let take = |v: u32, c: Option<(u32, u32)>| -> u8 {
        match c {
            Some((lo, 8)) => (v >> lo) as u8,
            Some((lo, bits)) => {
                let raw = (v >> lo) & ((1u64 << bits) as u32 - 1);
                (raw * 255 / ((1u64 << bits) as u32 - 1)) as u8
            }
            None => 255,
        }
    };
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let sy = if top_down { y } else { h - 1 - y };
        let row = &bytes[pixel_offset + sy as usize * row_bytes..];
        for x in 0..w {
            let px = &row[x as usize * bytes_pp..][..bytes_pp];
            let v = if bpp == 32 {
                u32::from_le_bytes(px[..4].try_into().unwrap())
            } else {
                u32::from_le_bytes([px[0], px[1], px[2], 0])
            };
            let o = ((y * w + x) * 4) as usize;
            out[o] = take(v, chans[0]);
            out[o + 1] = take(v, chans[1]);
            out[o + 2] = take(v, chans[2]);
            out[o + 3] = take(v, chans[3]);
        }
    }
    premultiply(&mut out);
    Ok(RgbaImage::new(w, h, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 与 `tests/fixtures/gen.py::pattern` 完全一致的原始（非预乘）图案。
    fn pattern(x: u32, y: u32, w: u32, h: u32, phase: u32) -> [u8; 4] {
        let r = (x * 255 / (w - 1).max(1)) as u8;
        let g = (y * 255 / (h - 1).max(1)) as u8;
        let b = ((x * 37 + y * 91 + phase * 7) & 0xFF) as u8;
        let a = (255 - ((x + 3 * y + phase) & 0x3F) * 2) as u8;
        [r, g, b, a]
    }

    fn premul(px: [u8; 4]) -> [u8; 4] {
        let a = u32::from(px[3]);
        [
            ((u32::from(px[0]) * a + 127) / 255) as u8,
            ((u32::from(px[1]) * a + 127) / 255) as u8,
            ((u32::from(px[2]) * a + 127) / 255) as u8,
            px[3],
        ]
    }

    fn fixture(name: &str) -> Vec<u8> {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read(&path).unwrap_or_else(|e| panic!("读取 fixture {path}: {e}"))
    }

    #[cfg(feature = "image")]
    #[test]
    fn png_pixels_match_premultiplied_pattern() {
        let ImageData::Single(img) = decode_image(&fixture("img_64x48.png")).unwrap() else {
            panic!("png 应为单帧");
        };
        assert_eq!((img.width, img.height), (64, 48));
        for (x, y) in [(0, 0), (63, 47), (31, 12), (5, 40)] {
            let want = premul(pattern(x, y, 64, 48, 0));
            assert_eq!(img.pixel(x, y), want, "png 像素 ({x},{y})");
        }
    }

    #[cfg(feature = "image")]
    #[test]
    fn webp_lossless_matches_pattern() {
        let ImageData::Single(img) = decode_image(&fixture("img_40x40.webp")).unwrap() else {
            panic!("webp 应为单帧");
        };
        assert_eq!((img.width, img.height), (40, 40));
        for (x, y) in [(0, 0), (39, 39), (13, 27)] {
            let want = premul(pattern(x, y, 40, 40, 0));
            let got = img.pixel(x, y);
            for c in 0..4 {
                assert!(
                    (got[c] as i32 - want[c] as i32).abs() <= 2,
                    "webp ({x},{y}) 通道 {c}: {got:?} vs {want:?}"
                );
            }
        }
    }

    #[cfg(feature = "image")]
    #[test]
    fn jpeg_decodes_size_and_average_color() {
        let ImageData::Single(img) = decode_image(&fixture("img_80x60.jpg")).unwrap() else {
            panic!("jpeg 应为单帧");
        };
        assert_eq!((img.width, img.height), (80, 60));
        // 有损：只断言平均色与图案均色接近（JPEG 无 alpha → a=255）。
        let mean = |c: usize| -> f64 {
            img.pixels[c..]
                .iter()
                .step_by(4)
                .map(|&v| f64::from(v))
                .sum::<f64>()
                / (f64::from(img.width) * f64::from(img.height))
        };
        assert!((mean(0) - 127.5).abs() < 8.0, "红通道均值 {:?}", mean(0));
        assert!((mean(1) - 127.5).abs() < 8.0, "绿通道均值 {:?}", mean(1));
        assert!(mean(3) > 254.0);
    }

    #[test]
    fn bmp_24bpp_bottom_up_matches_pattern() {
        let ImageData::Single(img) = decode_image(&fixture("img_24x18_24bpp.bmp")).unwrap() else {
            panic!("bmp 应为单帧");
        };
        assert_eq!((img.width, img.height), (24, 18));
        for (x, y) in [(0, 0), (23, 17), (7, 11)] {
            let mut src = pattern(x, y, 24, 18, 0);
            src[3] = 255; // 24bpp 无 alpha
            assert_eq!(img.pixel(x, y), premul(src), "bmp ({x},{y})");
        }
    }

    #[test]
    fn bmp_32bpp_bitfields_alpha() {
        let img = decode_image(&fixture("img_16x16_32bpp.bmp"))
            .unwrap()
            .first()
            .clone();
        assert_eq!((img.width, img.height), (16, 16));
        for (x, y) in [(0, 0), (15, 15), (4, 9)] {
            assert_eq!(img.pixel(x, y), premul(pattern(x, y, 16, 16, 0)));
        }
    }

    #[cfg(feature = "gif")]
    #[test]
    fn gif_multi_frame_yields_animdef() {
        let data = decode_image(&fixture("img_24x24.gif")).unwrap();
        let ImageData::Animated(def) = data else {
            panic!("gif 应为多帧");
        };
        assert_eq!(def.frames.len(), 2);
        assert_eq!(
            (def.frames[0].image.width, def.frames[0].image.height),
            (24, 24)
        );
        // 调色板量化：只断言首帧亮度分布与图案同向（左暗右亮）+ 帧延迟。
        let img = &def.frames[0].image;
        let lum = |x: u32| -> u32 {
            (0..24)
                .map(|y| {
                    let p = img.pixel(x, y);
                    (p[0] as u32 + p[1] as u32 + p[2] as u32) / 3
                })
                .sum()
        };
        assert!(lum(2) < lum(21), "gif 左暗右亮: {} vs {}", lum(2), lum(21));
    }

    #[test]
    fn unknown_magic_is_not_image() {
        let err = decode_image(b"NOTANIMAGE!!12").unwrap_err();
        assert!(matches!(err, ImageError::UnknownFormat { .. }), "{err:?}");
        assert!(matches!(decode_image(&[]).unwrap_err(), ImageError::Empty));
    }

    #[test]
    fn bmp_rejects_unsupported_variants() {
        // 8bpp 调色板 → 明确 Unsupported 而非乱解。
        let mut bytes = fixture("img_24x18_24bpp.bmp");
        bytes[28..30].copy_from_slice(&8u16.to_le_bytes());
        assert!(matches!(
            decode_image(&bytes).unwrap_err(),
            ImageError::Unsupported(_)
        ));
        // RLE 压缩同样拒绝。
        let mut bytes = fixture("img_24x18_24bpp.bmp");
        bytes[30..34].copy_from_slice(&1u32.to_le_bytes());
        assert!(matches!(
            decode_image(&bytes).unwrap_err(),
            ImageError::Unsupported(_)
        ));
    }
}
