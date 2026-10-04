//! 移植侧的 VFS provider：把 MV 的磁盘约定翻成引擎能读的字节。
//!
//! 三条职责（PLAN M0.3）：
//! 1. **逻辑名 → 加密名**：`.png→.rpgmvp`、`.ogg→.rpgmvo`、`.m4a→.rpgmvm`（上游
//!    `Decrypter.extToEncryptExt` 只认这三条）。替换在 `VfsPath` 规范化**之后**做
//!    （AGENTS.md 咬人第 4 条），所以全角、空格、CJK 名字都走得通。
//! 2. **明文回退**：加密名不在磁盘上就回退原名。上游 `_ignoreList` 的 `img/system/Window.png`、
//!    实测也明文的 `Loading.png`、`img/pictures/*.bmp`（bmp 根本没有加密名映射）、
//!    `data/*.json`、`fonts/*` 全走这条路 —— 判定靠 `is_encrypted` 的头，不靠扩展名。
//! 3. **解密**：拿到带 MV 16 字节头的字节就地解（[`crate::decrypt`]），内存里完成，
//!    绝不落明文镜像（AGENTS.md 定案：不复制素材）。
//!
//! 另有一条消极职责：`js/` 整棵树对本 VFS **不可见**。上游 103 个启用插件一个都不移植，而引擎的
//! `JsBridge` 遇到 `356` 会去 VFS 读 `js/plugins/<name>.js` 并 `exec`（错误被吞掉）——
//! 让它读不到，就省掉每条插件指令一次几十 KB 的白读（AGENTS.md 咬人第 6 条）。

use std::collections::HashSet;
use std::sync::Mutex;

use sisf_asset::VfsPath;
#[cfg(all(feature = "web", target_arch = "wasm32"))]
use sisf_asset::WebManifest;
use sisf_asset::vfs::VfsProvider;

use crate::decrypt::{HEADER_LEN, encrypted_ext};

/// 这些前缀（按规范化小写键比）下的文件一律当作不存在。
const HIDDEN_PREFIXES: &[&str] = &["js/"];

/// 一个 MV 工程的只读文件系统。native 持磁盘根，wasm 持预取清单。
pub struct MvAssets {
    backend: Backend,
    key: [u8; HEADER_LEN],
    /// 已经 warn 过的缺失键（同一路径只 warn 一次）。
    warned: Mutex<HashSet<String>>,
}

enum Backend {
    #[cfg(not(target_arch = "wasm32"))]
    Disk(std::path::PathBuf),
    #[cfg(all(feature = "web", target_arch = "wasm32"))]
    Web(std::sync::Arc<WebManifest>),
}

// 配错的构建会说"空 enum 的模式不穷尽"（E0004），而真正的问题是没有一种后端被打开 ——
// 浏览器腿必须带 `web`（清单预取那一族）。这条 compile_error 就是为了让下一轮一眼看到原因。
#[cfg(all(target_arch = "wasm32", not(feature = "web")))]
compile_error!("wasm32 上这一层只有 `web` 后端（清单预取）：build 要带 --features web");

impl MvAssets {
    /// 桌面腿：挂在上游 `www/` 根上（那个目录只读，一个字节都不写）。
    #[cfg(not(target_arch = "wasm32"))]
    pub fn disk(root: impl Into<std::path::PathBuf>, key: [u8; HEADER_LEN]) -> Self {
        Self {
            backend: Backend::Disk(root.into()),
            key,
            warned: Mutex::new(HashSet::new()),
        }
    }

    /// 浏览器腿：读宿主 fetch 好的清单字节。
    #[cfg(all(feature = "web", target_arch = "wasm32"))]
    pub fn web(manifest: std::sync::Arc<WebManifest>, key: [u8; HEADER_LEN]) -> Self {
        Self {
            backend: Backend::Web(manifest),
            key,
            warned: Mutex::new(HashSet::new()),
        }
    }

    /// 已 warn 过的缺失路径数（判据：读两次同一条缺失路径只涨 1）。
    pub fn missing_warned(&self) -> usize {
        self.lock_warned().len()
    }

    fn lock_warned(&self) -> std::sync::MutexGuard<'_, HashSet<String>> {
        self.warned.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 一条逻辑路径的磁盘候选，按优先级排（见 [`candidates`]）。
    fn candidates(&self, path: &VfsPath) -> Vec<String> {
        candidates(path)
    }
}

/// 一条逻辑路径的磁盘候选，按优先级排。
///
/// 两种拼写（原始大小写 → 规范化小写键）各先试加密名、再试原名：磁盘上真素材是
/// `img/characters/AAlcy.rpgmvp`，而请求方多半写的是 `AAlcy.png`，所以"原始拼写 +
/// 加密扩展名"必须排在第一。大小写不敏感的卷上四条会塌成一条。
///
/// wasm 那条按需预取的腿（[`crate::web`]）用的也是这一份 —— 它按这些拼写去 fetch、
/// fetch 到的字节也按这些拼写灌回 [`sisf::asset::WebManifest`]，所以 [`MvAssets::exists`]
/// 才问得出"到没到"。留第二份候选表就会两边不一致（加载器永远等一条磁盘上有的路径）。
pub fn candidates(path: &VfsPath) -> Vec<String> {
    let key = path.as_str();
    if HIDDEN_PREFIXES.iter().any(|p| key.starts_with(p)) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for base in [path.original(), key] {
        if let Some(enc) = encrypt_name(base) {
            out.push(enc);
        }
        out.push(base.to_owned());
    }
    let mut seen = HashSet::new();
    out.retain(|s| seen.insert(s.clone()));
    out
}

/// `dir/name.png` → `dir/name.rpgmvp`；扩展名不在上游那三条里（或没有扩展名）返回 `None`。
fn encrypt_name(name: &str) -> Option<String> {
    let dot = name.rfind('.')?;
    let sep = name.rfind('/')?;
    if dot < sep {
        return None;
    }
    let enc = encrypted_ext(&name[dot + 1..].to_ascii_lowercase())?;
    Some(format!("{}.{enc}", &name[..dot]))
}

impl MvAssets {
    /// 命中加密容器就就地解密，明文原样给出。
    fn decode_if_encrypted(&self, raw: Vec<u8>) -> Vec<u8> {
        match crate::decrypt::decrypt(&raw, &self.key) {
            Some(cow) => cow.into_owned(),
            None => raw,
        }
    }

    fn note_missing(&self, path: &VfsPath) {
        if self.lock_warned().insert(path.key().to_owned()) {
            eprintln!("mv_provider: VFS 里没有 {}", path.as_str());
        }
    }
}

impl VfsProvider for MvAssets {
    fn read(&self, path: &VfsPath) -> Option<Vec<u8>> {
        let found = match &self.backend {
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Disk(root) => self.candidates(path).into_iter().find_map(|rel| {
                let file = root.join(&rel);
                file.is_file().then(|| std::fs::read(file).ok()).flatten()
            }),
            #[cfg(all(feature = "web", target_arch = "wasm32"))]
            Backend::Web(manifest) => self
                .candidates(path)
                .into_iter()
                .find_map(|rel| VfsPath::new(&rel).ok().and_then(|p| manifest.read(&p))),
        };
        match found {
            Some(bytes) => Some(self.decode_if_encrypted(bytes)),
            None => {
                self.note_missing(path);
                None
            }
        }
    }

    fn exists(&self, path: &VfsPath) -> bool {
        match &self.backend {
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Disk(root) => self
                .candidates(path)
                .into_iter()
                .any(|rel| root.join(&rel).is_file()),
            #[cfg(all(feature = "web", target_arch = "wasm32"))]
            Backend::Web(manifest) => self.candidates(path).into_iter().any(|rel| {
                VfsPath::new(&rel)
                    .map(|p| manifest.exists(&p))
                    .unwrap_or(false)
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    //! 名字映射那两条（纯函数 ⇒ 两条腿都数得到）。真素材那一族的六条在 `pray/src/mv_provider.rs`
    //! —— 这一层不知道任何一个工程在哪。
    use super::*;
    use sisf_asset::vfs::path;

    // 这两条不碰磁盘 ⇒ 浏览器/node 那条腿也该数得到（`#[test]` 在 wasm32 上不会被
    // `wasm-bindgen-test-runner` 登记，要用同名别名，同 `jsrt`/`shapes`/`platform` 的写法）。
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    #[test]
    fn encrypt_name_follows_upstream_map() {
        assert_eq!(encrypt_name("a/b/C.png").as_deref(), Some("a/b/C.rpgmvp"));
        assert_eq!(
            encrypt_name("audio/bgm/x.Ogg").as_deref(),
            Some("audio/bgm/x.rpgmvo")
        );
        assert_eq!(encrypt_name("a/b/x.m4a").as_deref(), Some("a/b/x.rpgmvm"));
        assert_eq!(encrypt_name("a/b/x.bmp"), None);
        assert_eq!(encrypt_name("data/System.json"), None);
        assert_eq!(encrypt_name("no_extension"), None);
        // 点出现在目录名里（`a.b/c`）不算扩展名。
        assert_eq!(encrypt_name("img.a.b/c"), None);
    }

    #[test]
    fn candidates_try_encrypted_spelling_first_and_hide_the_js_tree() {
        let cands = candidates(&path("img/characters/AAlcy.png"));
        assert_eq!(cands[0], "img/characters/AAlcy.rpgmvp", "{cands:?}");
        assert!(cands.contains(&"img/characters/AAlcy.png".to_owned()));
        assert!(cands.contains(&"img/characters/aalcy.rpgmvp".to_owned()));
        // `js/` 那一族一个候选都不给（AGENTS 咬人第 6 条：`356` 会白读几十 KB 又吞错）。
        assert!(candidates(&path("js/plugins.js")).is_empty());
        assert!(candidates(&path("js/plugins/SomePlugin.js")).is_empty());
        // 明文那两条没有加密名映射 ⇒ 原名就是第一候选。
        assert_eq!(candidates(&path("data/System.json"))[0], "data/System.json");
    }
}
