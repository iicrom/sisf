//! 归档插槽：把 `.zip` 工程包挂成 [`VfsProvider`](sisf_asset::VfsProvider)。
//!
//! ## 只有 native
//!
//! 依赖写在 `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]` 里，`archive`
//! feature 在 wasm32 上因此是**显式空转**（模块本身也被 `cfg` 挡掉）。理由不是偷懒：
//! 浏览器里没有 `std::fs`，而 zip 的随机读要 `Seek`。wasm 侧的正确做法是宿主 JS 解压后
//! 把字节喂 `MemoryProvider` / `WebManifest` —— 引擎这边读到的仍然是同步的字节。
//! 见 `docs/porting.md`。
//!
//! ## 支持的条目
//!
//! `zip` 只开了 `deflate`（`default-features = false`），所以：
//! - **Stored / Deflate** 能读 —— MV/MZ 的打包习惯与 7-Zip「存储」档都落在这两格里；
//! - BZip2 / LZMA / Zstd / PPdM / AES 加密条目**读不出来**。构造期只看中央目录，所以
//!   打不开的只是那一个条目：[`read`](sisf_asset::VfsProvider::read) 把错误打到 stderr
//!   后返回 `None`（"这个文件不存在"是资产层唯一的降级通道）。
//!
//! ## 键
//!
//! 条目名经 [`VfsPath`] 规范化后进索引，于是查起来和
//! [`DirProvider`](sisf_asset::DirProvider) 一样大小写不敏感，反斜杠条目名（Windows 打的
//! 包）也认。目录条目（结尾 `/`）与畸形名（绝对路径、`..` 越根）不进索引。

use std::collections::HashMap;
use std::io::{Read, Seek};
use std::sync::{Mutex, MutexGuard};

use sisf_asset::VfsPath;
use zip::ZipArchive;

/// 归档打开失败。
#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("读归档失败: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip 结构有问题: {0}")]
    Zip(#[from] zip::result::ZipError),
}

/// `.zip` 工程包 provider。
///
/// 内部一把 `Mutex`：`zip` 8.x 的 `by_name`/`by_index` 取 `&mut self`（它缓存解压游标），
/// 而 [`VfsProvider`](sisf_asset::VfsProvider) 的读是 `&self`。索引在构造期一次建好，
/// `exists` 因此是一次哈希查表 —— trait 文档要求的那条"廉价"。
pub struct ZipProvider<R> {
    archive: Mutex<ZipArchive<R>>,
    /// `VfsPath::key()` → 条目在包里的**原始名**（`by_name` 只认原始名）。
    index: HashMap<String, String>,
}

impl<R: Read + Seek + Send> ZipProvider<R> {
    /// 从任意可随机读的源建包（`std::fs::File`、`std::io::Cursor<Vec<u8>>`…）。
    pub fn from_reader(reader: R) -> Result<Self, ArchiveError> {
        let archive = ZipArchive::new(reader)?;
        let mut names: Vec<String> = archive.file_names().map(|s| s.to_owned()).collect();
        // `file_names()` 走的是 `HashMap` 值序，**每轮进程都可能是别的顺序**。排序是为了
        // 让"两个条目规范化后撞同一个键"（`A.png` 与 `a.png`）的取舍每次运行都一样：
        // 留名字最小的那个，而不是留随机先到的那个。
        names.sort();
        let mut index = HashMap::with_capacity(names.len());
        for name in names {
            // 目录条目（`graphics/`）不是文件：进索引会让 `exists` 说真话而 `read` 给空串。
            if name.ends_with('/') {
                continue;
            }
            // 畸形条目名（绝对路径、`..` 越根、规范化后为空）在 `VfsPath` 构造期就被拒。
            // 它们本来就永远查不到，跳过比让整个包打不开好。
            if let Ok(p) = VfsPath::new(&name) {
                index.entry(p.key().to_owned()).or_insert(name);
            }
        }
        Ok(Self {
            archive: Mutex::new(archive),
            index,
        })
    }

    /// 包里真正可查的条目数（不含目录条目与畸形名，键已折叠大小写）。
    pub fn entries(&self) -> usize {
        self.index.len()
    }

    /// 条目原始名（排序后的清单，给"这个包里到底有什么"用）。
    pub fn listing(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.index.values().map(|s| s.as_str()).collect();
        out.sort();
        out
    }

    fn lock(&self) -> MutexGuard<'_, ZipArchive<R>> {
        // 只有中毒（别的线程在读条目时 panic）会走到这里；读条目不 panic，所以直接接管。
        self.archive.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl ZipProvider<std::fs::File> {
    /// 打开磁盘上的 `.zip` 工程包。
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self, ArchiveError> {
        Self::from_reader(std::fs::File::open(path)?)
    }
}

impl<R: Read + Seek + Send> sisf_asset::VfsProvider for ZipProvider<R> {
    fn read(&self, path: &VfsPath) -> Option<Vec<u8>> {
        // 名字先克隆再进锁：`index.get` 借的是 `&self`，`by_name` 借的是锁里的 archive，
        // 两者不该在同一个作用域里互相牵制。
        let name = self.index.get(path.key())?.clone();
        let mut archive = self.lock();
        let mut file = match archive.by_name(&name) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("rpgmk: 归档条目 {name} 打不开（压缩方法未支持，或已加密）: {e}");
                return None;
            }
        };
        let mut buf = Vec::with_capacity(file.size() as usize);
        if let Err(e) = file.read_to_end(&mut buf) {
            eprintln!("rpgmk: 归档条目 {name} 解压失败: {e}");
            return None;
        }
        Some(buf)
    }

    fn exists(&self, path: &VfsPath) -> bool {
        self.index.contains_key(path.key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sisf_asset::{MemoryProvider, Vfs, vfs::path};
    use std::io::Write;
    use zip::CompressionMethod;
    use zip::write::{SimpleFileOptions, ZipWriter};

    /// 现造一个包。`extra_dirs` 走 `add_directory`，用来验目录条目不进索引。
    fn build(files: &[(&str, &[u8])], dirs: &[&str], deflate: bool) -> Vec<u8> {
        let method = if deflate {
            CompressionMethod::Deflated
        } else {
            CompressionMethod::Stored
        };
        let mut w = ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in files {
            // 每个条目单独取一份 options：`FileOptions` 是按条目传的，共用一个变量只会
            // 让"上一个条目的参数留在下一个条目上"。
            let opts = SimpleFileOptions::default().compression_method(method);
            if dirs.contains(name) {
                w.add_directory(*name, opts).expect("写目录条目");
                continue;
            }
            w.start_file(*name, opts).expect("写文件条目");
            w.write_all(bytes).expect("写字节");
        }
        w.finish().expect("收尾").into_inner()
    }

    fn provider(
        files: &[(&str, &[u8])],
        dirs: &[&str],
        deflate: bool,
    ) -> ZipProvider<std::io::Cursor<Vec<u8>>> {
        ZipProvider::from_reader(std::io::Cursor::new(build(files, dirs, deflate))).expect("开包")
    }

    /// 验收（T11）："zip 内资产可读" —— 走的是和散文件同一条 `Vfs` 路径。
    #[test]
    fn entries_in_a_zip_are_readable_through_the_vfs() {
        let map = br#"{"width":4,"height":3,"data":[0,0,0,0]}"#;
        let p = provider(
            &[
                ("data/Map001.json", map.as_slice()),
                ("Graphics/Characters/Actor1.png", b"\x89PNG".as_slice()),
            ],
            &["Graphics/Characters/"],
            false,
        );
        assert_eq!(p.entries(), 2, "目录条目不进索引");
        assert_eq!(
            p.listing(),
            vec!["Graphics/Characters/Actor1.png", "data/Map001.json"]
        );
        let mut vfs = Vfs::new();
        vfs.register(Box::new(p));
        assert_eq!(vfs.read(&path("data/Map001.json")).unwrap(), map);
        // 大小写不敏感 —— 与 `DirProvider` 一致的查找习惯（MV 的 PNG 命名就是混用的）。
        assert!(vfs.exists(&path("GRAPHICS/characters/ACTOR1.PNG")));
        assert!(!vfs.exists(&path("data/Map002.json")));
    }

    /// `deflate` 是这里唯一开着的压缩方法，工程包几乎都是它。
    #[test]
    fn deflate_entries_round_trip() {
        // 重复字节才压得动：全零 4x3 地图按 MV 的层数是 72 个 0。
        let data = vec![b'0'; 512];
        let p = provider(&[("data/Map001.json", data.as_slice())], &[], true);
        let mut vfs = Vfs::new();
        vfs.register(Box::new(p));
        assert_eq!(vfs.read(&path("data/map001.json")).unwrap().len(), 512);
    }

    /// 补丁优先：`Vfs` 后注册者优先，所以"包 + 散文件热更"是注册顺序问题，不是这里的逻辑。
    #[test]
    fn patch_provider_overrides_the_archive() {
        let base = r#"{"gameTitle":"原"}"#;
        let patch_json = r#"{"gameTitle":"补丁"}"#;
        let mut vfs = Vfs::new();
        vfs.register(Box::new(provider(
            &[("data/System.json", base.as_bytes())],
            &[],
            false,
        )));
        let mut patch = MemoryProvider::new();
        patch.insert(&path("data/System.json"), patch_json.as_bytes());
        vfs.register(Box::new(patch));
        let got = String::from_utf8(vfs.read(&path("data/System.json")).unwrap()).unwrap();
        assert_eq!(got, patch_json, "散文件补丁盖住了包里的同名条目");
    }

    /// 畸形与目录条目只会少进索引，不会让包打不开。
    #[test]
    fn odd_entries_are_skipped_not_fatal() {
        let p = provider(
            &[
                ("graphics/", b"".as_slice()),
                ("../escape.json", b"{}".as_slice()),
                ("/abs.json", b"{}".as_slice()),
                ("data/Game.json", b"{}".as_slice()),
            ],
            &["graphics/"],
            false,
        );
        assert_eq!(p.entries(), 1, "只剩 data/Game.json");
        assert_eq!(p.listing(), vec!["data/Game.json"]);
    }

    /// 验收（T11）"zip 内资产可读"里真正的那一条：压进包的是**仓库里那份散文件 MV 工程**，
    /// 读出来的是 `MapDef::load` —— 与 `rpgmv_shell` 走的是同一个调用点。前面几条用的是
    /// 手写的字节，只能证明解包，证明不了"整个兼容层不需要知道资产在包里还是在盘上"。
    #[test]
    fn the_real_project_loads_after_being_zipped() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/assets/mv");
        let mut owned: Vec<(String, Vec<u8>)> = Vec::new();
        for name in ["data/System.json", "data/Map001.json", "data/Tilesets.json"] {
            owned.push((
                name.to_owned(),
                std::fs::read(dir.join(name)).expect("fixture 在仓库里（examples/assets/mv）"),
            ));
        }
        let files: Vec<(&str, &[u8])> = owned
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_slice()))
            .collect();
        // deflate：真实工程包几乎都是它，顺手把唯一开着的压缩方法走一遍。
        let mut vfs = Vfs::new();
        vfs.register(Box::new(provider(&files, &[], true)));

        let sys = crate::map::SystemDef::load(&vfs).expect("包里的 System.json");
        let map = crate::map::MapDef::load(&vfs, sys.start_map_id).expect("包里的 Map001.json");
        assert_eq!(
            (map.width, map.height),
            (24, 18),
            "尺寸不对 ⇒ 读的是另一份工程"
        );
        assert_eq!(map.event_ids().len(), 4);
        assert_eq!(sys.game_title, "SISF rpgmv_shell");
    }
}
