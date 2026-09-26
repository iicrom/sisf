//! `VfsPath`：VFS 内的规范化相对路径。
//!
//! 规则（T4 DoD）：
//! - 统一 `/` 分隔（Windows 的 `\` 与 URL 的 `%2F` 在解析前由调用方替换为 `/`）；
//! - 查找键大小写不敏感（ASCII 折叠，与 RGSS 补丁目录习惯一致）；
//! - 拒绝 `..` 越出根、拒绝绝对路径与空路径 —— 路径穿越在这里就是构造失败，
//!   provider 实现里不再有任何二次校验的责任。

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VfsPathError {
    pub input: String,
    pub reason: &'static str,
}

impl fmt::Display for VfsPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "非法 VFS 路径 {:?}: {}", self.input, self.reason)
    }
}

impl std::error::Error for VfsPathError {}

/// 规范化后的 VFS 路径：`Graphics/A/B.png` → 显示形如 `graphics/a/b.png`（小写键）。
/// 等值/哈希/排序只看 `key`（大小写不敏感）；`original` 保留输入拼写，供
/// `DirProvider` 在区分大小写的文件系统上二次尝试。
#[derive(Clone, Debug)]
pub struct VfsPath {
    /// 规范化、已折叠小写的键；同时用作显示形式，保证键唯一。
    key: String,
    /// 去掉分隔符归一后的原始拼写。
    original: String,
}

impl PartialEq for VfsPath {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl Eq for VfsPath {}
impl std::hash::Hash for VfsPath {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.key.hash(h);
    }
}
impl PartialOrd for VfsPath {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for VfsPath {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.key.cmp(&other.key)
    }
}

impl VfsPath {
    /// 解析并规范化。`.` 段与空段被丢弃；出现越根的 `..`、绝对路径或无有效段时报错。
    pub fn new(input: &str) -> Result<Self, VfsPathError> {
        let bad = |reason: &'static str| VfsPathError {
            input: input.to_owned(),
            reason,
        };
        if input.contains('\0') {
            return Err(bad("含 NUL"));
        }
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(bad("空路径"));
        }
        if trimmed.starts_with('/') || trimmed.starts_with('\\') {
            return Err(bad("绝对路径"));
        }
        // 只在形如 `x:` 时判盘符，避免误伤 `a:b/c` 这类合法文件名。
        if let [c, b':', ..] = trimmed.as_bytes()
            && c.is_ascii_alphabetic()
        {
            return Err(bad("带盘符的绝对路径"));
        }
        let mut stack: Vec<&str> = Vec::new();
        for seg in trimmed.split(['/', '\\']) {
            match seg {
                "" | "." => continue,
                ".." => {
                    if stack.pop().is_none() {
                        return Err(bad("`..` 越出根"));
                    }
                }
                s => stack.push(s),
            }
        }
        if stack.is_empty() {
            return Err(bad("规范化后为空"));
        }
        let original = stack.join("/");
        let mut key = original.clone();
        key.push('\0');
        key.make_ascii_lowercase();
        Ok(Self { key, original })
    }

    /// 大小写折叠后的查找键（provider 之间约定用它比对）。
    pub fn key(&self) -> &str {
        &self.key
    }

    /// 归一分隔符后的原始拼写（区分大小写的文件系统上的二次候选用）。
    pub fn original(&self) -> &str {
        &self.original
    }

    /// 形如 `dir/name.ext` 的规范化显示路径（去掉段分隔哨兵）。
    pub fn as_str(&self) -> &str {
        self.key.trim_end_matches('\0')
    }
}

impl fmt::Display for VfsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // T12：`cargo xtask wasm-test` 在 wasm 上跑这一组（见 docs/platforms.md）。
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    #[test]
    fn backslash_and_dots_are_normalized() {
        let p = VfsPath::new("Graphics\\/./Battleships\\Ship01.png").unwrap();
        assert_eq!(p.as_str(), "graphics/battleships/ship01.png");
    }

    #[test]
    fn lookup_key_is_case_insensitive() {
        let a = VfsPath::new("Graphics/A/B.PNG").unwrap();
        let b = VfsPath::new("graphics/a/b.png").unwrap();
        assert_eq!(a, b);
        assert_eq!(a.key(), b.key());
    }

    #[test]
    fn escaping_root_is_rejected() {
        // 路径穿越：任何能爬出挂载根的输入必须在构造期失败。
        for s in [
            "../secret",
            "a/../../b",
            "..\\..\\windows\\system32",
            "a/b/../../..",
        ] {
            let e = VfsPath::new(s).unwrap_err();
            assert!(
                e.reason.contains("`..`"),
                "{s} 应以越根被拒，实际: {}",
                e.reason
            );
        }
    }

    #[test]
    fn internal_parent_segment_cancels() {
        let p = VfsPath::new("a/b/../c/d.txt").unwrap();
        assert_eq!(p.as_str(), "a/c/d.txt");
    }

    #[test]
    fn absolute_and_empty_inputs_rejected() {
        assert!(VfsPath::new("").is_err());
        assert!(VfsPath::new("   ").is_err());
        assert!(VfsPath::new("/etc/passwd").is_err());
        assert!(VfsPath::new("C:/Windows").is_err());
        assert!(VfsPath::new("a\0b").is_err());
        assert!(VfsPath::new(".").is_err());
        assert_eq!(VfsPath::new("a/.").unwrap().as_str(), "a");
    }
}
