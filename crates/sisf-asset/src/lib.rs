//! VFS 资源虚拟文件系统 + AssetStore 缓存 + TaskRunner 异步加载（T4 实装）。
//!
//! - [`path::VfsPath`]：规范化、大小写不敏感、拒绝 `..` 越根的相对路径；
//! - [`vfs`]：[`vfs::Vfs`] 叠加多 provider（后注册优先 = 补丁覆盖），已实装
//!   `DirProvider`/`MemoryProvider`/`WebManifest`，zip/RGSSAD 只需新增 provider
//!   实现（接口登记在 `vfs` 模块文档）；
//! - [`store::AssetStore`]：路径→资产缓存（`Weak` 引用计数 + 代际 `AssetId`），
//!   同路径在飞不重复解码，句柄全 Drop 后可重载；
//! - [`tasks`]：[`tasks::TaskRunner`] 解码任务（native 线程+mpsc；wasm 协程队列
//!   就地泵），**GPU 上传恒在主线程**（`sisf-render` 的 `atlas_gpu::UploadRing`）。

pub mod path;
pub mod store;
pub mod tasks;
pub mod vfs;

pub use path::VfsPath;
pub use store::{AssetId, AssetStore, Handle, Request, Ticket};
#[cfg(not(target_arch = "wasm32"))]
pub use tasks::NativeTasks;
pub use tasks::TaskRunner;
#[cfg(target_arch = "wasm32")]
pub use tasks::WasmTasks;
#[cfg(not(target_arch = "wasm32"))]
pub use vfs::DirProvider;
#[cfg(all(feature = "web", target_arch = "wasm32"))]
pub use vfs::WebManifest;
pub use vfs::{MemoryProvider, Vfs, VfsProvider};
