//! 循环点读取（lofty）。
//!
//! 引擎认的是 RPG Maker / ffmpeg 那一套自由键名：`LOOPSTART` / `LOOPEND`，单位**源帧**。
//! 三个容器各有各的坑，所以这里按容器走 lofty 的**具体文件类型**而不是通用 `TaggedFile`：
//! 通用通路会把映射不到的键丢掉（`ItemKey` 是宏生成的封闭枚举，没有 unknown 变体），
//! `LOOPSTART` 恰好就是映射不到的那种。
//!
//! - OGG：Vorbis comments，任意键名都在 → [`VorbisComments::get`]。
//! - MP3：ID3v2 的 TXXX（用户文本帧），ffmpeg 写 `-metadata LOOPSTART=…` 就落在这里。
//! - WAV：lofty 只认 `fmt /fact/data/LIST`，**没有** `smpl`/`cue` 通路 → 恒 `None`，
//!   于是 WAV 整段循环（见 `docs/audio.md`）。要 `smpl` 得自己走 chunk 遍历。

use std::io::Cursor;

use lofty::config::ParseOptions;
use lofty::file::AudioFile;
use lofty::mpeg::MpegFile;
use lofty::ogg::VorbisFile;

use crate::audio::{Container, LOOP_END_KEYS, LOOP_START_KEYS, parse_loop_frame};

/// 读出 `[loop_start, loop_end)`（源帧）。缺任一端、或 `end <= start` 都返回 `None`。
pub fn loop_points(bytes: &[u8], container: Container) -> Option<(u32, u32)> {
    let mut cur = Cursor::new(bytes);
    let opts = ParseOptions::default();
    let (start, end): (Option<String>, Option<String>) = match container {
        Container::Ogg => {
            let file = <VorbisFile as AudioFile>::read_from(&mut cur, opts).ok()?;
            let comments = file.vorbis_comments();
            (
                pick(comments, &LOOP_START_KEYS),
                pick(comments, &LOOP_END_KEYS),
            )
        }
        Container::Mp3 => {
            let file = <MpegFile as AudioFile>::read_from(&mut cur, opts).ok()?;
            let id3 = file.id3v2()?;
            (
                LOOP_START_KEYS
                    .iter()
                    .find_map(|k| id3.get_user_text(k).map(str::to_owned)),
                LOOP_END_KEYS
                    .iter()
                    .find_map(|k| id3.get_user_text(k).map(str::to_owned)),
            )
        }
        Container::Wav => return None,
    };
    let (s, e) = (parse_loop_frame(&start?)?, parse_loop_frame(&end?)?);
    (e > s).then_some((s, e))
}

fn pick(comments: &lofty::ogg::tag::VorbisComments, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| comments.get(k).map(str::to_owned))
}
