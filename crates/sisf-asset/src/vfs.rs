//! VFS：多 provider 叠加的只读虚拟文件系统。
//!
//! [`Vfs`] 按注册顺序查找，**后注册的优先**——RPG Maker 语义里"补丁目录覆盖同名
//! 文件"就是 `register(base)` 后再 `register(patch)`。
//!
//! 已实装的 provider：
//! - [`DirProvider`]（native）：挂一个目录，`std::fs` 读取；先按规范化小写键找，
//!   找不到再按原始拼写找（区分大小写的文件系统上两种上架方式都能命中）。
//! - [`MemoryProvider`]：内存表，单测与内嵌资源用。
//! - `WebManifest`（`web` + wasm32，所以宿主 doc 里链不到）：预置清单；宿主 JS 把文件 fetch 成字节后
//!   经 `WebManifest::insert` 灌进内部 [`MemoryProvider`]，`VfsProvider::read`
//!   因此永远是同步的。清单未就绪的路径表现为"不存在"，应用侧下一帧重试。
//!
//! 归档 provider 在移植层，不在这里：
//! - `ZipProvider`（T11，**native only**，见 `sisf-rpgmk::archive`）：把 `.zip` 工程包按
//!   中央目录建索引后挂进来。它不进 `sisf-asset` 是因为 `zip` 这条依赖在 wasm 上根本
//!   没有意义（没有 `std::fs`，随机读要 `Seek`），而 asset 是四端都编的核心 crate。
//!
//! 仍未实装的 provider（只登记接口，见 `docs/porting.md`）：
//! - `RgssadProvider`：VX/RGSS 加密包（手写 XOR+meta 解密，纯字节操作）。
//!   它的入口形状与 `MemoryProvider` 相同（构造期拿到全 `&[u8]` 或预建索引），
//!   不需要 `VfsProvider` trait 变化。

use std::collections::HashMap;
use std::sync::Arc;
#[cfg(all(feature = "web", target_arch = "wasm32"))]
use std::sync::Mutex;

use crate::path::VfsPath;

/// 一个只读字节来源。实现必须廉价地回答 `exists`（索引或一次 `stat`）。
pub trait VfsProvider: Send {
    fn read(&self, path: &VfsPath) -> Option<Vec<u8>>;
    fn exists(&self, path: &VfsPath) -> bool;
}

/// 便捷构造：`path("Graphics/A.png")`，非法输入直接 panic（仅测试与确定合法的
/// 常量路径可用）。
pub fn path(input: &str) -> VfsPath {
    VfsPath::new(input).unwrap_or_else(|e| panic!("{e}"))
}

/// provider 集合；`Vec` 尾部 = 最后注册 = 最高优先级。
#[derive(Default)]
pub struct Vfs {
    providers: Vec<Box<dyn VfsProvider>>,
}

impl Vfs {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册 provider；后注册者优先（补丁覆盖）。
    pub fn register(&mut self, provider: Box<dyn VfsProvider>) {
        self.providers.push(provider);
    }

    /// 按优先级解析出第一个命中的 provider 下标。
    fn find(&self, path: &VfsPath) -> Option<usize> {
        self.providers
            .iter()
            .rev()
            .position(|p| p.exists(path))
            .map(|skip| {
                // rev().position 给出的是"从尾数第几个"，换回正序下标。
                self.providers.len() - 1 - skip
            })
    }

    pub fn read(&self, path: &VfsPath) -> Option<Vec<u8>> {
        self.find(path).and_then(|i| self.providers[i].read(path))
    }

    pub fn exists(&self, path: &VfsPath) -> bool {
        self.find(path).is_some()
    }

    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }
}

/// 内存表 provider：键 = `VfsPath::key()`。
#[derive(Default)]
pub struct MemoryProvider {
    files: HashMap<String, Arc<[u8]>>,
}

impl MemoryProvider {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: &VfsPath, bytes: &[u8]) {
        self.files.insert(path.key().to_owned(), bytes.into());
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

impl VfsProvider for MemoryProvider {
    fn read(&self, path: &VfsPath) -> Option<Vec<u8>> {
        self.files.get(path.key()).map(|b| b.to_vec())
    }

    fn exists(&self, path: &VfsPath) -> bool {
        self.files.contains_key(path.key())
    }
}

/// 目录 provider（native）：把 `VfsPath` 拼到 `root` 下用 `std::fs` 读。
#[cfg(not(target_arch = "wasm32"))]
pub struct DirProvider {
    root: std::path::PathBuf,
}

#[cfg(not(target_arch = "wasm32"))]
impl DirProvider {
    pub fn new(root: impl Into<std::path::PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// 候选相对路径：先小写键，再原始拼写（覆盖两种上架习惯）。
    fn candidates(&self, path: &VfsPath) -> [std::path::PathBuf; 2] {
        [path.as_str().into(), path.original().into()]
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl VfsProvider for DirProvider {
    fn read(&self, path: &VfsPath) -> Option<Vec<u8>> {
        self.candidates(path)
            .iter()
            .map(|rel| self.root.join(rel))
            .find(|file| file.is_file())
            .and_then(|file| std::fs::read(file).ok())
    }

    fn exists(&self, path: &VfsPath) -> bool {
        self.candidates(path)
            .iter()
            .any(|rel| self.root.join(rel).is_file())
    }
}

/// wasm 预置清单：宿主侧 fetch 好后灌入，读取走内部 [`MemoryProvider`]。
///
/// 清单内容 = 游戏需要的所有文件路径（构建期生成，如 `manifest.txt` 一行一个）。
/// [`Self::pending`] 给出还没抓到的路径，供宿主 JS 批量 `fetch`。
#[cfg(all(feature = "web", target_arch = "wasm32"))]
pub struct WebManifest {
    inner: Arc<Mutex<MemoryProvider>>,
    /// 清单全集（构造期定形，只读快照语义）。
    listing: Vec<VfsPath>,
}

#[cfg(all(feature = "web", target_arch = "wasm32"))]
impl WebManifest {
    /// 从清单（一行一个路径）建立；非法行忽略。
    pub fn new(manifest: &str) -> Self {
        Self {
            inner: Arc::new(Mutex::new(MemoryProvider::new())),
            listing: manifest
                .lines()
                .filter_map(|line| VfsPath::new(line.trim()).ok())
                .collect(),
        }
    }

    /// fetch 完成回调：灌入字节。键不在清单里也接受（运行期追加）。
    pub fn insert(&self, path: &VfsPath, bytes: Vec<u8>) {
        self.inner.lock().unwrap().insert(path, &bytes);
    }

    /// 尚未就绪的清单路径（宿主按此 fetch）。
    pub fn pending(&self) -> Vec<VfsPath> {
        let inner = self.inner.lock().unwrap();
        self.listing
            .iter()
            .filter(|p| !inner.exists(p))
            .cloned()
            .collect()
    }
}

#[cfg(all(feature = "web", target_arch = "wasm32"))]
impl VfsProvider for WebManifest {
    fn read(&self, path: &VfsPath) -> Option<Vec<u8>> {
        self.inner.lock().unwrap().read(path)
    }

    fn exists(&self, path: &VfsPath) -> bool {
        self.inner.lock().unwrap().exists(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem(files: &[(&str, &[u8])]) -> MemoryProvider {
        let mut p = MemoryProvider::new();
        for (name, bytes) in files {
            p.insert(&path(name), bytes);
        }
        p
    }

    #[test]
    fn later_provider_overrides_earlier() {
        let mut vfs = Vfs::new();
        vfs.register(Box::new(mem(&[("a.png", b"base".as_slice())])));
        vfs.register(Box::new(mem(&[("a.png", b"patch".as_slice())])));
        assert_eq!(vfs.read(&path("A.PNG")).unwrap(), b"patch");
    }

    #[test]
    fn fallthrough_when_patch_lacks_file() {
        let mut vfs = Vfs::new();
        vfs.register(Box::new(mem(&[("base-only.png", b"B".as_slice())])));
        vfs.register(Box::new(mem(&[("patch-only.png", b"P".as_slice())])));
        assert_eq!(vfs.read(&path("base-only.png")).unwrap(), b"B");
        assert_eq!(vfs.read(&path("patch-only.png")).unwrap(), b"P");
        assert!(!vfs.exists(&path("missing.png")));
        assert!(vfs.read(&path("missing.png")).is_none());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn patch_dir_overrides_same_named_file() {
        // 补丁目录覆盖验收：base/ 与 patch/ 各有一个同名文件，注册 patch 后读到补丁版。
        let base = std::env::temp_dir().join("sisf-vfs-base");
        let patch = std::env::temp_dir().join("sisf-vfs-patch");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::create_dir_all(&patch).unwrap();
        std::fs::write(base.join("hero.png"), b"ORIGINAL").unwrap();
        std::fs::write(patch.join("hero.png"), b"OVERRIDDEN").unwrap();
        std::fs::write(base.join("unique.png"), b"only-base").unwrap();

        let mut vfs = Vfs::new();
        vfs.register(Box::new(DirProvider::new(&base)));
        assert_eq!(vfs.read(&path("hero.png")).unwrap(), b"ORIGINAL");
        vfs.register(Box::new(DirProvider::new(&patch)));
        assert_eq!(vfs.read(&path("hero.png")).unwrap(), b"OVERRIDDEN");
        // 大小写不同的查找也命中（Windows 天然命中；Unix 走小写键候选）。
        assert_eq!(vfs.read(&path("Hero.PNG")).unwrap(), b"OVERRIDDEN");
        // 补丁没有的文件落回 base。
        assert_eq!(vfs.read(&path("unique.png")).unwrap(), b"only-base");

        std::fs::remove_dir_all(&base).unwrap();
        std::fs::remove_dir_all(&patch).unwrap();
    }

    #[test]
    fn traversal_never_reaches_provider() {
        // 穿越在 VfsPath 构造期就被拒，provider 永远收不到越根路径。
        assert!(VfsPath::new("../secret.txt").is_err());
        let mut vfs = Vfs::new();
        vfs.register(Box::new(mem(&[("secret.txt", b"x".as_slice())])));
        assert!(vfs.read(&VfsPath::new("./secret.txt").unwrap()).is_some());
    }
}
