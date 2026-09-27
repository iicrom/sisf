//! T11 视频播放器：纹槽 + 拉帧把手，**两端都是显式失败的桩**。
//!
//! ## 为什么拆在门面而不是 `sisf-media`
//!
//! 清单把 `VideoSource` 与 `VideoPlayer` 都写在 `sisf-media/src/video.rs` 里，但
//! `VideoPlayer` 的纹理槽是 [`Bitmap`]，而包级依赖图是 `render → asset → media` ——
//! media 反向依赖 render 是环。于是接口（解码、拉帧）留在 `sisf-media::video`，
//! 只有碰 GPU 句柄的这一半搬到这里：门面同时看得见 render 与 media。
//!
//! ## 桩期做什么、不做什么
//!
//! [`VideoPlayer::open`] 恒返回 [`VideoError::Unsupported`]，所以没有"空实现分支"
//! （DoD-4）：每一个入口都会失败，且失败文案说出缺的是哪个后端。移植层因此能把
//! "movie 播不了"写成一次 warn 而不是运行时猜，见 [`VideoPlayer::skip_notice`]。
//!
//! ## `pump` 为什么把帧交回调用方，而不是自己写进 `texture`
//!
//! 清单写的是"帧写入 Bitmap，sprite 直接引用"。**今天没有"往一张已上传的位图重写
//! 像素"的通路**：`Bitmap` 的 CPU 像素在进上传队列时就被交了出去（`is_pending()`
//! 从此为 false），而 RT 页根本没有 CPU 侧（`UploadRing` 也永不排它）。真接线有两条路，
//! 桩不替后端替调用方做这个选择：
//!
//! 1. **每帧新分配一页**：拿到 [`VideoFrame`] 后 `atlas.new_bitmap(w, h, pixels, frame)`，
//!    `set_texture` 换掉旧的。写法最短，代价是图集页按帧率翻腾（旧句柄 Drop 后由 LRU 退役）。
//! 2. **给 `Bitmap` 加一条写回路径**（复用 epoch，`copy_buffer_to_texture` 到原页）：
//!    性能正确的那条，但要动 `sisf-render`，超出"只保留 API 与文档"的本阶段定价。
//!
//! 音轨同理不在这里：[`VideoSource::take_audio`] 交出一个
//! [`Decoder`](crate::media::audio::Decoder)，对齐归游戏（过场可以静音），引擎内没有
//! 第二根时钟去追它。

use sisf_media::video::{VideoError, VideoFrame, VideoSource, open as open_source};
use sisf_render::Bitmap;

/// 一段影片的播放状态：解码源 + 当前帧的位图句柄。
///
/// `Clone` 刻意不 derive：两个播放器抢同一个源会各自解出半截序列，而纹槽是
/// `Bitmap`（Clone 共享页），想要"多个精灵放同一段片"应该共享一个 `Arc<VideoPlayer>`
/// 之外的源，或者干脆只播一处。
#[derive(Default)]
pub struct VideoPlayer {
    source: Option<Box<dyn VideoSource>>,
    texture: Option<Bitmap>,
    /// `skip_notice` 只 warn 一次的闩锁。
    noticed: bool,
}

impl VideoPlayer {
    /// 一个"没在播"的播放器：`pump` 恒 `Ok(None)`，等 [`open`](Self::open) 成功。
    pub fn new() -> Self {
        Self::default()
    }

    /// 打开一段影片字节。桩时代恒 `Err(Unsupported)`，且**认不认容器都是这一句** ——
    /// 报"容器不认识"会把构建问题伪装成文件问题。
    ///
    /// 真后端在这里按 [`sniff`](crate::media::video::sniff) + target 分派 ffmpeg / WebCodecs。
    pub fn open(bytes: &[u8]) -> Result<Self, VideoError> {
        Ok(Self {
            source: Some(open_source(bytes)?),
            texture: None,
            noticed: false,
        })
    }

    pub fn is_open(&self) -> bool {
        self.source.is_some()
    }

    /// 当前帧的位图句柄。精灵侧照常用它填 `Sprite.bitmap`（Clone 即共享页）。
    pub fn texture(&self) -> Option<&Bitmap> {
        self.texture.as_ref()
    }

    /// 换进新的一帧（见模块文档"每帧新分配一页"那条路）。
    pub fn set_texture(&mut self, bitmap: Bitmap) {
        self.texture = Some(bitmap);
    }

    /// 拉下一帧。`Ok(None)` = 没有在播或流已结束的下一拍；返回的帧**由调用方决定怎么上 GPU**。
    pub fn pump(&mut self) -> Result<Option<VideoFrame>, VideoError> {
        match self.source.as_mut() {
            Some(src) => src.next_frame(),
            None => Ok(None),
        }
    }

    /// 视频轨尺寸（没打开 ⇒ `(0, 0)`），位图按它分配。
    pub fn size(&self) -> (u32, u32) {
        self.source.as_ref().map_or((0, 0), |s| s.size())
    }

    /// 移植层遇到 movie 指令时的降级把手：第一次给一句要 warn 的话，之后给 `None`。
    ///
    /// 为什么把"warn 一次"做成状态而不是让调用方自己记：`rpgmv_shell` 一类移植工程里
    /// 同一张片头可能被反复触发（重开场景、存档回退），每拍刷一条日志等于没有日志。
    pub fn skip_notice(&mut self) -> Option<&'static str> {
        if self.noticed {
            return None;
        }
        self.noticed = true;
        Some(crate::media::video::STUB_NOTE)
    }

    /// 关掉这一片（纹槽留给调用方自己 Drop，页面因此可能还在别处被引用）。
    pub fn close(&mut self) {
        self.source = None;
        self.texture = None;
    }
}

/// 手写而不是 derive：`source` 是 trait object，没有 `Debug`；调试时真正想知道的也就是
/// "开没开、多大、纹槽在不在"。
impl std::fmt::Debug for VideoPlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (w, h) = self.size();
        f.debug_struct("VideoPlayer")
            .field("open", &self.is_open())
            .field("size", &(w, h))
            .field("texture", &self.texture.as_ref().map(|b| b.size()))
            .field("noticed", &self.noticed)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sisf_media::video::STUB_NOTE;

    /// 验收（T11）：播放器两端一律 `Unsupported`，而且说的和 `open_source` 是同一句。
    #[test]
    fn the_player_refuses_everything_it_is_asked_to_do() {
        let known = VideoPlayer::open(b"xxxxftypisom").unwrap_err();
        let junk = VideoPlayer::open(b"not a movie").unwrap_err();
        assert_eq!(known, junk, "构建缺后端，不该被报成文件有问题");
        assert_eq!(
            known,
            VideoError::Unsupported(STUB_NOTE),
            "门面与 media 层的失败文案必须同源"
        );

        // 没在播的播放器不是"报错的播放器"：拉帧是 Ok(None)，尺寸是 0。
        let mut idle = VideoPlayer::new();
        assert!(!idle.is_open());
        assert_eq!(idle.pump().unwrap(), None);
        assert_eq!(idle.size(), (0, 0));
        assert!(idle.texture().is_none());
    }

    /// `skip_notice` 只响一次，而且说的是"为什么放不了"。
    #[test]
    fn the_skip_notice_is_latched_and_names_the_missing_backend() {
        let mut p = VideoPlayer::new();
        let first = p.skip_notice().expect("第一次要 warn");
        assert!(
            first.contains("ffmpeg") && first.contains("WebCodecs"),
            "{first}"
        );
        assert!(p.skip_notice().is_none(), "反复触发同一张片不该刷屏");
        // 闩锁属于播放器而不是进程：换新一个播放器还能再 warn 一次。
        assert!(VideoPlayer::new().skip_notice().is_some());
        p.close();
        assert!(!p.is_open());
    }
}
