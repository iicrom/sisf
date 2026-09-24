//! 图像/音频/MIDI/视频等媒体解码：平台门控依赖集中处（T4 起实装图像）。

pub mod audio;
#[cfg(feature = "image")]
pub mod image;
#[cfg(feature = "midi")]
pub mod midi;
