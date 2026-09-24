//! 字体装载：VFS 里的 ttf/otf 字节 → `cosmic-text` 的 `FontSystem`。
//!
//! 语义对齐 RPG Maker：游戏工程自带字体（`Graphics/Fonts/*`）优先，读不到就退回
//! 系统默认字体。回退是**静默失效的重灾区**（移植层写错路径时字全没了却没人喊），
//! 所以这里必须 warn——但同一条原因只 warn 一次：一帧里可能构造几十个 `Bitmap`。
//!
//! 平台差异（见 `docs/text.md`）：
//! - 桌面/Android：`FontSystem::new()` 会扫系统字体，中文等 CJK 由系统字体兜底。
//! - wasm32：系统字体不存在。自带字体字节是唯一来源；两者都没有时字形直接不出现
//!   （不放空实现、也不假装成功——回退原因仍在 `FontBundle::fallback()` 里）。

use std::collections::HashSet;
use std::sync::{Arc, LazyLock, Mutex};

use cosmic_text::FontSystem;
use cosmic_text::fontdb::Source;

/// 装载失败的两种成因（同时也是 warn 的 key）。
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FontError {
    /// VFS 里没有这个文件（或读出来是空的）。
    #[error("字体字节为空：VFS 里没有这个文件")]
    Empty,
    /// 字节在，但不是能解析的 ttf/otf（连一个 face 都取不出来）。
    #[error("{0} 字节的字体数据解析不出任何 face（不是 ttf/otf？）")]
    Unparsable(usize),
    /// 解析出了 face，但拿不到族名（无法把它设成默认族）。
    #[error("字体解析出了 face 但没有族名")]
    NoFamily,
}

/// 同一条原因只放行一次的 warn 簿记。
///
/// 刻意做成可构造的类型而不是只留一个全局：单测要能重复验证"第二次不再打"，
/// 而全局状态在并行测试里会互相踩。`FontBundle::from_bytes` 用的是内部全局表。
#[derive(Default)]
pub struct WarnOnce {
    seen: Mutex<HashSet<String>>,
}

impl WarnOnce {
    pub fn new() -> Self {
        Self::default()
    }

    /// `reason` 首次出现返回 `true`（调用方据此打日志）。
    pub fn should_warn(&self, reason: &str) -> bool {
        self.seen.lock().unwrap().insert(reason.to_string())
    }

    /// 已登记的原因条数（诊断用）。
    pub fn reasons(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}

/// 进程级 warn 表：一个游戏通常只有一个字体装载器。
static DEFAULT_WARN: LazyLock<WarnOnce> = LazyLock::new(WarnOnce::new);

/// 一套可用字体（`cosmic-text` 的 `FontSystem`）。
#[derive(Debug)]
pub struct FontBundle {
    fonts: FontSystem,
    /// 生效的自带字体族名（`None` = 只有系统字体）。
    family: Option<String>,
    /// 回退成因（`Some` = 自带字体没用上，走的是默认字体）。
    fallback: Option<FontError>,
    /// 本次构造是否真的打出了 warn（被去重掉就是 `false`）。
    warned: bool,
}

impl FontBundle {
    /// 经 VFS 读到的字体字节装载自带字体；不可用时回退默认并 warn（每种原因一次）。
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_bytes_with(bytes, &DEFAULT_WARN)
    }

    /// 同 `from_bytes`，但 warn 去重表由调用方给出（单测与多装载器场景）。
    pub fn from_bytes_with(bytes: &[u8], warn: &WarnOnce) -> Self {
        let mut bundle = Self::system();
        match bundle.register_bytes(bytes) {
            Ok(name) => {
                bundle.family = Some(name);
                bundle
            }
            Err(err) => {
                let reason = err.to_string();
                if warn.should_warn(&reason) {
                    eprintln!("sisf-text: {reason}，回退默认字体");
                    bundle.warned = true;
                }
                bundle.fallback = Some(err);
                bundle
            }
        }
    }

    /// 只用系统字体（没有自带字体时 `from_bytes` 的回退终点）。
    pub fn system() -> Self {
        Self {
            // 注：wasm32 上 `load_system_fonts` 扫不到任何东西，这是 fontdb 的行为。
            fonts: FontSystem::new(),
            family: None,
            fallback: None,
            warned: false,
        }
    }

    /// 追加一份字体字节，返回它的族名。
    ///
    /// 注册成功即把它设成 sans/serif 默认族——`FontSystem` 的候选表按"权重差、
    /// 宽度差、样式差、face id"排序，同分时**先注册的赢**，而系统字体几乎总是先
    /// 注册的；只有把族名设为默认族，`db.query` 才会把它挪到候选表最前面。
    pub fn register_bytes(&mut self, bytes: &[u8]) -> Result<String, FontError> {
        if bytes.is_empty() {
            return Err(FontError::Empty);
        }
        let ids = self
            .fonts
            .db_mut()
            .load_font_source(Source::Binary(Arc::new(bytes.to_vec())));
        // fontdb 对垃圾字节是 warn + 返回更少的 id，不会 panic，所以这里判空即可。
        let Some(&first) = ids.first() else {
            return Err(FontError::Unparsable(bytes.len()));
        };
        let name = self
            .fonts
            .db()
            .face(first)
            .and_then(|face| face.families.first().map(|(n, _)| n.clone()))
            .filter(|n| !n.is_empty())
            .ok_or(FontError::NoFamily)?;
        self.fonts.db_mut().set_sans_serif_family(&name);
        self.fonts.db_mut().set_serif_family(&name);
        Ok(name)
    }

    /// 是否走了回退路径（自带字体没用上）。
    pub fn is_fallback(&self) -> bool {
        self.fallback.is_some()
    }

    pub fn fallback(&self) -> Option<&FontError> {
        self.fallback.as_ref()
    }

    /// 本次构造是否真的打了 warn（第二次同样的缺失只有第一次会 `true`）。
    pub fn warned_now(&self) -> bool {
        self.warned
    }

    /// 生效的自带字体族名。
    pub fn family(&self) -> Option<&str> {
        self.family.as_deref()
    }

    /// 可用 face 数（0 = 这台机器上一个字都排不出来）。
    pub fn face_count(&self) -> usize {
        self.fonts.db().len()
    }

    pub(crate) fn fonts(&mut self) -> &mut FontSystem {
        &mut self.fonts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真字体（Inter，OFL）：随仓库带一份，免得单测依赖各机系统字体。
    const INTER: &[u8] = include_bytes!("../tests/fixtures/Inter-Regular.ttf");

    #[test]
    fn valid_bytes_register_their_own_family() {
        let bundle = FontBundle::from_bytes(INTER);
        assert!(
            !bundle.is_fallback(),
            "好字节不该回退：{:?}",
            bundle.fallback()
        );
        assert!(!bundle.warned_now());
        assert_eq!(bundle.family(), Some("Inter"));
        assert!(bundle.face_count() > 0);
    }

    /// 两条回退路径：空字节（VFS 没这个文件）与垃圾字节（不是 ttf/otf）。
    #[test]
    fn missing_and_garbage_bytes_both_fall_back() {
        let cases: &[(&[u8], FontError)] = &[
            (&[], FontError::Empty),
            (&[0u8, 1, 2, 3, 4, 5, 6, 7], FontError::Unparsable(8)),
        ];
        for (bytes, want) in cases {
            let bundle = FontBundle::from_bytes_with(bytes, &WarnOnce::new());
            assert!(bundle.is_fallback(), "{want:?} 该标记成回退");
            assert_eq!(bundle.fallback().unwrap(), want);
            assert_eq!(bundle.family(), None, "回退时不该设自带族名");
            assert!(bundle.warned_now(), "新去重表 ⇒ 第一次一定真打了 warn");
        }
    }

    /// warn-once：同一条原因只放行一次，不同原因各算一次。
    #[test]
    fn fallback_warns_only_once_per_reason() {
        let warn = WarnOnce::new();
        let first = FontBundle::from_bytes_with(&[], &warn);
        let again = FontBundle::from_bytes_with(&[], &warn);
        assert!(first.warned_now(), "第一次该打");
        assert!(!again.warned_now(), "同样的缺失不该再打");
        assert_eq!(warn.reasons(), 1);
        // 垃圾字节是另一条原因，仍要放行一次。
        let third = FontBundle::from_bytes_with(&[9; 16], &warn);
        assert!(third.warned_now());
        assert_eq!(warn.reasons(), 2);
    }

    /// 多份字体可叠加（拉丁 + CJK 各一份是常见做法）。
    #[test]
    fn later_registrations_add_faces() {
        let mut bundle = FontBundle::system();
        let before = bundle.face_count();
        assert_eq!(bundle.register_bytes(INTER).unwrap(), "Inter");
        assert!(bundle.face_count() > before);
        // 最后注册的赢默认族（RPG Maker 只有一个自带字体，够用）。
        assert_eq!(bundle.register_bytes(INTER).unwrap(), "Inter");
    }
}
