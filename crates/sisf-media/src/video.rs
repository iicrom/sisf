//! T11 视频桩：容器识别 + `VideoSource` 接口，**两端都不解码**。
//!
//! ## 为什么这一层现在是"每次都失败"
//!
//! 清单（todo.md T11）对本阶段的定价很明确：只保留 API 与文档，native 与 wasm 两侧一律
//! 返回 [`VideoError::Unsupported`]。DoD-4 禁止的是"空实现散落" —— 这里没有空实现，
//! 每个入口都**显式失败**并说清缺的是哪个后端，移植层因此可以按类型写死降级路径
//! （`rpgmv_shell` 遇 movie 指令安全跳过 + warn 一次），而不是运行时猜"这块黑的是没播还是播完了"。
//!
//! ## 为什么这里只有接口，没有 `VideoPlayer`
//!
//! 清单写的是 `sisf-media/src/video.rs` 里同时放 `VideoSource` 与持 `BitmapHandle` 的
//! `VideoPlayer`。**包级依赖图不允许**：`render → asset → media`，media 反向依赖 render
//! 是环（与 T4 那条"依赖边方向不变：`render`/`asset` 仍不认识 `media`"是同一条纪律）。
//! 于是拆两半：这一层是"解码接口"（和 `audio::Decoder` 平级，不碰 GPU），
//! `VideoPlayer`（纹理槽 + pump）落在门面 `sisf::video` —— 那里 render 与 media 都在依赖图上。
//!
//! ## 接线时的形状（后端归 T11 之后的真实交付）
//!
//! - native：`ffmpeg-next`（Win/Linux/macOS/iOS）。解码可在**调用方线程**进行，所以
//!   [`VideoSource`] 要求 `Send`；输出恒为 RGBA8 预乘（与 [`image`](crate::image) 那侧同一约定）。
//! - wasm：`<video>` + canvas，或 WebCodecs。两条都要 JS 胶水，`Send` 约束在 wasm 上
//!   退化为"只在主线程碰"（与 [`audio`](crate::audio) 侧 pump 同样的处境）。
//! - 带音轨的影片：视频帧走 [`VideoSource::next_frame`]，音频走 [`VideoSource::take_audio`]
//!   （一个 [`Decoder`](crate::audio::Decoder)），**由游戏侧按时间轴对齐二者**。引擎不做
//!   同步：把时钟塞进桩里等于桩一旦被替换就要改签名，而"谁跟谁对齐"本来就是游戏决定
//!   （过场可以静音）。
//!
//! 与 `image` feature 刻意无关：帧类型是本模块自己的 [`VideoFrame`]，所以
//! "只开 video 不开 image"的构建编得过 —— 但那种构建里位图永远传不上 GPU（上传队列在
//! `image` 后面），要真放电影得两个都开。

use crate::audio::{AudioError, AudioFormat, Decoder};

/// 视频后端缺位/解码失败。载荷是 `String` 或 `'static`，与 [`image`](crate::image)
/// 那侧一样要能冒到 `Request::Failed`（跨 crate 只认 `Display`）。
#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum VideoError {
    /// 桩的失败：**构建里就没有后端**。认不认容器都会走到这里 —— 报"容器不认识"
    /// 会比真相更误导（听像是文件的问题，其实是构建的问题）。
    #[error("视频后端尚未接入（{0}）：显式失败而不是静默黑屏")]
    Unsupported(&'static str),
    /// 真后端接上之后才会走到的分支：容器/解码错误。
    #[error("视频数据无效: {0}")]
    Corrupt(String),
}

/// 桩时代给 [`VideoError::Unsupported`] 用的同一句话：各入口说的必须是一件事，
/// 否则文档与运行期文案会漂。
pub const STUB_NOTE: &str = "native 需 ffmpeg-next、wasm 需 <video>/WebCodecs 胶水";

/// 认得出的视频容器（只用于诊断文案，桩不会真按它分派）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Container {
    /// ISO-BMFF/MP4（`ftyp` box）。
    Mp4,
    /// Matroska/WebM（EBML 头）。
    Mkv,
    /// RIFF/AVI。
    Avi,
}

/// 按魔数认容器 —— 与 [`audio::sniff`](crate::audio::sniff) 同一条纪律：认内容不认扩展名。
pub fn sniff(bytes: &[u8]) -> Option<Container> {
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Some(Container::Mp4);
    }
    if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some(Container::Mkv);
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"AVI ") {
        return Some(Container::Avi);
    }
    None
}

/// 解码出来的一帧：RGBA8、预乘、行主序（与 `image::RgbaImage` 同一约定，但故意不复用
/// 那个类型 —— 见模块文档的 feature 说明）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    /// 这一帧的展示时长（毫秒），来自容器时间戳；`0` = 尽快下一帧。
    pub dur_ms: u32,
}

impl VideoFrame {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>, dur_ms: u32) -> Self {
        debug_assert_eq!(pixels.len(), (width * height * 4) as usize);
        Self {
            width,
            height,
            pixels,
            dur_ms,
        }
    }

    /// 单像素（测试与回读比较用）。
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        self.pixels[i..i + 4].try_into().unwrap()
    }
}

/// 一个已打开的视频流的只读拉取接口。
pub trait VideoSource: Send {
    /// 视频轨道尺寸（像素）。后端必须在**第一帧之前**就能回答（RT 按它分配）。
    fn size(&self) -> (u32, u32);

    /// 帧率（Hz）。`None` = 后端不知道（网络流），由调用方按拍自取。
    fn fps(&self) -> Option<f64>;

    /// 取下一帧。`Ok(None)` = 流结束。
    fn next_frame(&mut self) -> Result<Option<VideoFrame>, VideoError>;

    /// 音轨（无音轨 ⇒ `None`）。所有权交给调用方：引擎不做音视频对齐。
    fn take_audio(&mut self) -> Option<Box<dyn Decoder>>;
}

/// 桩的另一半：当"这个源就没有音轨"是**定案**而不是"还没解出来"时，用它代替 `None`，
/// 好让播放侧拿到一个恒定静音的流而不是自己造一个 `Decoder`（静音的形状归引擎，不归游戏）。
#[derive(Clone, Copy, Debug)]
pub struct SilentDecoder {
    format: AudioFormat,
}

impl SilentDecoder {
    pub fn stereo_48k() -> Self {
        Self {
            format: AudioFormat::stereo(48_000),
        }
    }
}

/// 手写而不是 derive：`AudioFormat` 自己没有 `Default`（混音器的缺省格式与"一条恒定
/// 静音的流该长什么样"是两件事），而播放侧要能 `default()` 出一个不炸的流。
impl Default for SilentDecoder {
    fn default() -> Self {
        Self::stereo_48k()
    }
}

impl Decoder for SilentDecoder {
    fn format(&self) -> AudioFormat {
        self.format
    }

    fn read(&mut self, _out: &mut [f32]) -> Result<usize, AudioError> {
        Ok(0)
    }

    fn seek_sample(&mut self, _frame: u64) -> Result<(), AudioError> {
        Ok(())
    }
}

/// 桩时代唯一的"打开"：什么后端都没有，于是恒失败。
///
/// 真接线时这个函数按 [`sniff`] + target 分派到 `ffmpeg-next` / WebCodecs 胶水，
/// 返回 `Box<dyn VideoSource>`；调用方（`sisf::video::VideoPlayer`）不变。
pub fn open(_bytes: &[u8]) -> Result<Box<dyn VideoSource>, VideoError> {
    Err(VideoError::Unsupported(STUB_NOTE))
}

#[cfg(test)]
mod tests {
    use super::*;
    // T12 约定：wasm 腿也跑这一组（`cargo xtask wasm-test` 的 `sisf-media` 条目）。
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    /// 验收（T11）：两端一律返回 `Unsupported`，而且是**同一句话** ——
    /// 移植层的 warn 文案因此在桌面与浏览器上一字不差。
    #[test]
    fn the_stub_fails_explicitly_with_one_shared_sentence() {
        // 不用 `unwrap_err`：成功分支装的是 `Box<dyn VideoSource>`，它没有 `Debug`，
        // 而给一个 trait object 补 `Debug` 只为写得出测试断言是本末倒置。
        let known = match open(b"xxxxftypisom") {
            Err(e) => e,
            Ok(_) => panic!("桩不该真打开一个源"),
        };
        let junk = match open(b"junkjunkjunk") {
            Err(e) => e,
            Ok(_) => panic!("桩不该真打开一个源"),
        };
        assert_eq!(
            known, junk,
            "认不认容器都该报同一句，否则构建问题会伪装成文件问题"
        );
        match known {
            VideoError::Unsupported(note) => assert!(
                note.contains("ffmpeg") && note.contains("WebCodecs"),
                "失败要说出缺的后端: {note}"
            ),
            other => panic!("期望 Unsupported，得到 {other:?}"),
        }
        assert_eq!(
            VideoError::Corrupt("截断".into()).to_string(),
            "视频数据无效: 截断"
        );
    }

    /// 认容器只到"认出来"为止：魔数与音频容器互不侵占。
    #[test]
    fn sniff_recognizes_video_containers_only() {
        assert_eq!(sniff(b"xxxxftypisom"), Some(Container::Mp4));
        assert_eq!(
            sniff(&[0x1A, 0x45, 0xDF, 0xA3, 0, 0, 0, 0]),
            Some(Container::Mkv)
        );
        assert_eq!(sniff(b"RIFFxxxxAVI "), Some(Container::Avi));
        assert_eq!(sniff(b"RIFFxxxxWEBP"), None, "WebP 是图像不是视频");
        assert_eq!(sniff(b"OggS"), None, "音频容器不算视频");
        assert_eq!(sniff(b"sh"), None, "短到读不出魔数");
    }

    /// 静音流是"定案"的形状：读 0 帧、seek 永成立，而不是 `None` 逼调用方自己造 Decoder。
    #[test]
    fn the_silent_stream_reads_as_ended_without_seeking() {
        let mut silent = SilentDecoder::default();
        let mut buf = [0.0f32; 16];
        assert_eq!(silent.read(&mut buf).unwrap(), 0);
        assert_eq!(silent.channels(), 2);
        assert_eq!(silent.sample_rate(), 48_000);
        assert!(silent.total_frames().is_none());
        assert_eq!(silent.loop_points(), None);
        assert!(silent.seek_sample(999).is_ok());
    }
}
