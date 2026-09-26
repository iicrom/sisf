//! `AssetStore<T>`：按路径缓存的资产表 + 引用计数 + 异步装载管线。
//!
//! 生命周期（T4 验收逐条对应）：
//! - `load`：命中缓存立即 `Ready(Handle)`；否则投递解码任务并返回 `Loading(Ticket)`；
//! - **同路径不重复解码**：同一槽只有一个在飞任务，后到的 `load` 拿同一张共享 `Ticket`；
//! - **句柄 Drop 后可重载**：外部句柄全部析构后（缓存自身仍持底本），下次 `load`
//!   会**先弃底本再重解码**，`AssetId.generation` 递增——新旧代不会共存；
//! - 失败缓存：解码失败后 `load` 直接返回 `Failed`（不逐帧重试打磁盘），
//!   [`AssetStore::retry`] 显式清掉后下次 `load` 再投。
//!
//! 收割节拍：主循环每帧 `poll_ready(runner)` 一次——先排 runner 完成信号，再把
//! 已到 inbox 的解码结果装槽并唤醒 ticket。任务在 worker 线程上只写 inbox
//! （`Mutex` 信箱），不触碰槽本体。
//!
//! GPU 上传不在本模块：调用方（应用/门面）拿到 `Ready` 后在主线程经
//! `sisf-render::atlas_gpu::UploadRing` 上传。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use crate::path::VfsPath;
use crate::tasks::{Job, TaskRunner, make_job};
use crate::vfs::Vfs;

/// 资产身份：路径键的 FNV-1a 哈希 + 代号。换代只发生在"上代句柄全 Drop 后重载"。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AssetId {
    pub hash: u32,
    pub generation: u32,
}

impl std::fmt::Display for AssetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:08x}#{}", self.hash, self.generation)
    }
}

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in bytes {
        h ^= u32::from(*b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 一次装载的终态凭据。可 Clone——同槽 waiter 共享同一张。
#[derive(Clone)]
pub struct Ticket {
    done: Arc<AtomicBool>,
}

impl Ticket {
    /// 任务是否已终态（Ready 或 Failed）。内容本身从 `AssetStore::get` 或
    /// `poll_ready` 的返回值取。
    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }
}

/// `load` 的三态返回。
pub enum Request<T> {
    Ready(Handle<T>),
    Loading(Ticket),
    Failed(Arc<str>),
}

/// 资产强句柄；某一代的外部句柄全部 Drop 后，下一次针对该路径的 `load`
/// 会弃缓存、换代重解码（store 内部底本不计入外部句柄）。
pub struct Handle<T> {
    value: Arc<T>,
    id: AssetId,
    externals: Arc<AtomicU32>,
}

impl<T> Handle<T> {
    pub fn id(&self) -> AssetId {
        self.id
    }

    pub fn get(&self) -> &T {
        &self.value
    }
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        self.externals.fetch_add(1, Ordering::Relaxed);
        Self {
            value: self.value.clone(),
            id: self.id,
            externals: self.externals.clone(),
        }
    }
}

impl<T> Drop for Handle<T> {
    fn drop(&mut self) {
        self.externals.fetch_sub(1, Ordering::Relaxed);
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handle").field("id", &self.id).finish()
    }
}

/// 槽的装载状态（`state` 锁只保护这几十字节，不在任务线程上持锁跑解码）。
#[derive(Debug, PartialEq)]
enum Loading {
    Idle,
    InFlight { nonce: u64 },
    Done,
}

struct Slot<T> {
    key: VfsPath,
    generation: u32,
    /// 底本缓存。`None` = 从未装入或已被"全 Drop → 弃缓存"回收。
    cached: Option<Arc<T>>,
    error: Option<Arc<str>>,
    loading: Loading,
    ticket: Option<Ticket>,
    /// 分发给调用方的强句柄数（Clone/Drop 维护；底本不计入）。
    externals: Arc<AtomicU32>,
    /// 曾成功装入（区分"全新槽"与"上代已弃"，后者才 bump generation）。
    loaded_once: bool,
}

impl<T> Slot<T> {
    fn new(key: VfsPath) -> Self {
        Self {
            key,
            generation: 0,
            cached: None,
            error: None,
            loading: Loading::Idle,
            ticket: None,
            externals: Arc::new(AtomicU32::new(0)),
            loaded_once: false,
        }
    }

    /// 上代资产已发布过、且外部句柄已全部析构 → 该换代重载了。
    fn stale_after_drop(&self) -> bool {
        self.loaded_once && self.cached.is_some() && self.externals.load(Ordering::Relaxed) == 0
    }

    fn hand_out(&self, arc: Arc<T>) -> Handle<T> {
        self.externals.fetch_add(1, Ordering::Relaxed);
        Handle {
            value: arc,
            id: self.id(),
            externals: self.externals.clone(),
        }
    }

    fn id(&self) -> AssetId {
        AssetId {
            hash: fnv1a32(self.key.key().as_bytes()),
            generation: self.generation,
        }
    }
}

/// worker → 主线程的解码结果信箱。
type Inbox<T> = Arc<Mutex<Vec<(u64, usize, AssetId, Result<T, String>)>>>;

/// 路径 → 资产的缓存表。主线程对象。
pub struct AssetStore<T> {
    slots: Vec<Slot<T>>,
    index: HashMap<String, usize>,
    inbox: Inbox<T>,
    next_nonce: u64,
}

impl<T> AssetStore<T> {
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            index: HashMap::new(),
            inbox: Arc::new(Mutex::new(Vec::new())),
            next_nonce: 0,
        }
    }
}

impl<T> Default for AssetStore<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Send + 'static> AssetStore<T> {
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// 只查缓存，不触发装载（即使外部句柄已全 Drop，底本仍在）。
    pub fn get(&self, path: &VfsPath) -> Option<Handle<T>> {
        let &i = self.index.get(path.key())?;
        let slot = &self.slots[i];
        slot.cached.as_ref().map(|a| slot.hand_out(a.clone()))
    }

    /// 同步取用或异步装载：未缓存时经 `runner` 投递 `decode(VFS 字节)`。
    ///
    /// 闭包收到的是 `Arc<Vec<u8>>`（VFS 读出的原始文件字节；路径不存在时为
    /// 空 Vec，由闭包自决错误文案，store 不猜格式语义）。
    pub fn load(
        &mut self,
        vfs: &Vfs,
        runner: &mut dyn TaskRunner,
        path: &VfsPath,
        decode: impl FnOnce(Arc<Vec<u8>>) -> Result<T, String> + Send + 'static,
    ) -> Request<T> {
        self.pump(runner);
        let i = self.slot_for(path);
        let id = self.slots[i].id();
        match self.start_or_attach(i, id, vfs, runner, decode) {
            Start::Sent => {
                let ticket = self.slots[i].ticket.clone().expect("刚建票");
                Request::Loading(ticket)
            }
            Start::Direct(req) => req,
        }
    }

    /// 启动期同步装载（不过 runner）：`f` 就地执行、直接入缓存。
    pub fn load_blocking(
        &mut self,
        path: &VfsPath,
        f: impl FnOnce() -> Result<T, String>,
    ) -> Request<T> {
        let i = self.slot_for(path);
        match crate::tasks::load_blocking(f) {
            Ok(v) => Request::Ready(self.install(i, v)),
            Err(e) => Request::Failed(self.fail(i, e)),
        }
    }

    /// 每帧一次：排 runner 信号 + 装 inbox。返回本轮到达终态的 `(AssetId, Ready/Failed)`。
    pub fn poll_ready(&mut self, runner: &mut dyn TaskRunner) -> Vec<(AssetId, Request<T>)> {
        self.pump(runner);
        let mut out = Vec::new();
        for (nonce, i, _id, result) in self.drain_inbox() {
            let fresh =
                matches!(self.slots[i].loading, Loading::InFlight { nonce: n } if n == nonce);
            if !fresh {
                continue; // retry 之后才姗姗来迟的旧任务结果，丢弃
            }
            self.slots[i].loading = Loading::Done;
            let req = match result {
                Ok(v) => Request::Ready(self.install(i, v)),
                Err(e) => Request::Failed(self.fail(i, e)),
            };
            if matches!(req, Request::Ready(_)) {
                // install 不碰票（失败路径由 fail 结清），成功在这里终结票据。
                if let Some(t) = self.slots[i].ticket.take() {
                    t.done.store(true, Ordering::Release);
                }
            }
            out.push((self.slots[i].id(), req));
        }
        out
    }

    /// 清掉失败缓存，允许下次 `load` 重新投递。
    pub fn retry(&mut self, path: &VfsPath) -> bool {
        let Some(&i) = self.index.get(path.key()) else {
            return false;
        };
        if self.slots[i].error.take().is_some() {
            self.slots[i].loading = Loading::Idle;
            true
        } else {
            false
        }
    }

    // —— 内部 ——

    fn pump(&mut self, runner: &mut dyn TaskRunner) {
        runner.poll_ready();
    }

    fn drain_inbox(&self) -> Vec<(u64, usize, AssetId, Result<T, String>)> {
        std::mem::take(&mut *self.inbox.lock().unwrap())
    }

    fn slot_for(&mut self, path: &VfsPath) -> usize {
        if let Some(&i) = self.index.get(path.key()) {
            return i;
        }
        let i = self.slots.len();
        self.slots.push(Slot::<T>::new(path.clone()));
        self.index.insert(path.key().to_owned(), i);
        i
    }

    /// Ready/Failed 直返，或投递新任务（同路径在飞时共享票据、绝不二次解码）。
    fn start_or_attach(
        &mut self,
        i: usize,
        id: AssetId,
        vfs: &Vfs,
        runner: &mut dyn TaskRunner,
        decode: impl FnOnce(Arc<Vec<u8>>) -> Result<T, String> + Send + 'static,
    ) -> Start<T> {
        let key = self.slots[i].key.clone();
        {
            // 第一遍：能在槽状态上判掉的直接返回。换代 bump 必须在"无在飞任务"
            // 确认之后做，否则会把在飞任务的代也 bump 掉。
            let slot = &mut self.slots[i];
            if slot.stale_after_drop() {
                // 外部句柄已全 Drop → 弃底本、换代，落到下面的重投路径。
                slot.cached = None;
                slot.generation += 1;
            } else if let Some(arc) = slot.cached.clone() {
                return Start::Direct(Request::Ready(slot.hand_out(arc)));
            }
            if let Some(err) = slot.error.clone() {
                return Start::Direct(Request::Failed(err));
            }
            if let Loading::InFlight { .. } = slot.loading {
                let ticket = slot.ticket.clone().expect("在飞必有票");
                return Start::Direct(Request::Loading(ticket));
            }
        }
        let nonce = self.next_nonce;
        self.next_nonce += 1;
        let bytes = Arc::new(vfs.read(&key).unwrap_or_default());
        let found = !bytes.is_empty();
        let inbox = self.inbox.clone();
        self.slots[i].ticket = Some(Ticket {
            done: Arc::new(AtomicBool::new(false)),
        });
        self.slots[i].loading = Loading::InFlight { nonce };
        let job: Job = make_job(move || {
            let result = if found {
                decode(bytes)
            } else {
                Err(format!("VFS 中不存在: {key}"))
            };
            inbox.lock().unwrap().push((nonce, i, id, result));
        });
        runner.submit(job);
        Start::Sent
    }

    /// 装入成功值并返回句柄。
    fn install(&mut self, i: usize, value: T) -> Handle<T> {
        let arc = Arc::new(value);
        let slot = &mut self.slots[i];
        slot.cached = Some(arc.clone());
        slot.error = None;
        slot.loading = Loading::Idle;
        slot.loaded_once = true;
        slot.hand_out(arc)
    }

    fn fail(&mut self, i: usize, msg: String) -> Arc<str> {
        let msg: Arc<str> = msg.into();
        let slot = &mut self.slots[i];
        slot.error = Some(msg.clone());
        slot.loading = Loading::Idle;
        if let Some(t) = slot.ticket.take() {
            t.done.store(true, Ordering::Release);
        }
        msg
    }
}

enum Start<T> {
    Sent,
    Direct(Request<T>),
}

#[cfg(test)]
mod tests {
    use super::*;
    // 这一组**不在** `cargo xtask wasm-test` 里：`Harness::wait` 靠 `thread::sleep` 泵
    // `WasmTasks`，而 wasm32-unknown-unknown 既没有线程也没有时钟（`docs/platforms.md`）。

    #[cfg(not(target_arch = "wasm32"))]
    use crate::tasks::NativeTasks;
    #[cfg(target_arch = "wasm32")]
    use crate::tasks::WasmTasks as NativeTasks;
    use crate::vfs::{MemoryProvider, path};
    use std::sync::atomic::AtomicUsize;

    /// 测试用：把 store + runner 绑在一起，模拟主循环节拍。
    struct Harness {
        vfs: Vfs,
        store: AssetStore<String>,
        runner: NativeTasks,
    }

    impl Harness {
        fn new(files: &[(&str, &[u8])]) -> Self {
            let mut provider = MemoryProvider::new();
            for (name, bytes) in files {
                provider.insert(&path(name), bytes);
            }
            let mut vfs = Vfs::new();
            vfs.register(Box::new(provider));
            Self {
                vfs,
                store: AssetStore::new(),
                runner: NativeTasks::new(),
            }
        }

        /// 阻塞等到票结清，**保有所得句柄**（测试驱动；真实应用自然持有句柄）。
        fn wait(&mut self, ticket: &Ticket) -> Vec<(AssetId, Request<String>)> {
            let mut got = Vec::new();
            while !ticket.is_done() {
                got.extend(self.store.poll_ready(&mut self.runner));
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            got
        }

        fn decode_counting(
            &mut self,
            path_str: &str,
            counter: &Arc<AtomicUsize>,
        ) -> Request<String> {
            let c = counter.clone();
            self.store
                .load(&self.vfs, &mut self.runner, &path(path_str), move |bytes| {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok(String::from_utf8_lossy(&bytes[..]).into_owned())
                })
        }
    }

    #[test]
    fn same_path_decodes_only_once() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut h = Harness::new(&[("a.dat", b"AAA".as_slice())]);
        let r1 = h.decode_counting("a.dat", &counter);
        let ticket = match r1 {
            Request::Loading(t) => t,
            other => {
                // 极快的 worker 也可能已收割成 Ready；那更没有重复解码可言。
                assert!(matches!(other, Request::Ready(_)));
                return;
            }
        };
        // 在飞期间再 request 两次：只挂票，不再投任务。
        for _ in 0..2 {
            let again = h.decode_counting("a.dat", &counter);
            match again {
                Request::Loading(t2) => assert!(Arc::ptr_eq(&t2.done, &ticket.done)),
                Request::Ready(_) => break,
                Request::Failed(e) => panic!("{e}"),
            }
        }
        let got = h.wait(&ticket);
        assert_eq!(got.len(), 1);
        let h2 = match &got[0].1 {
            Request::Ready(handle) => handle.clone(),
            Request::Failed(e) => panic!("{e}"),
            Request::Loading(_) => panic!("票结清即终态"),
        };
        assert_eq!(h2.get(), "AAA");
        assert_eq!(h2.id().generation, 0);
        assert_eq!(counter.load(Ordering::SeqCst), 1, "同路径重复解码");
    }

    #[test]
    fn handle_drop_allows_reload_with_next_generation() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut h = Harness::new(&[("a.dat", b"AAA".as_slice())]);
        let req = h.decode_counting("a.dat", &counter);
        let ticket = match req {
            Request::Loading(t) => t,
            Request::Ready(_) => panic!("刚投的任务不应同帧收割"),
            Request::Failed(e) => panic!("{e}"),
        };
        let got = h.wait(&ticket);
        let first = match &got[0].1 {
            Request::Ready(handle) => handle.clone(),
            Request::Failed(e) => panic!("{e}"),
            Request::Loading(_) => unreachable!(),
        };
        assert_eq!(first.id().generation, 0);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        // 底本还在：句柄没全 Drop 前 load 直返缓存。
        let again_cached = h.decode_counting("a.dat", &counter);
        assert!(matches!(again_cached, Request::Ready(_)));
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        let first_id = first.id();
        drop(again_cached);
        drop(first);
        drop(got);
        // 全 Drop 后重载：换代、重新解码。
        let ticket2 = match h.decode_counting("a.dat", &counter) {
            Request::Loading(t) => t,
            other => panic!("句柄已没，应重投，实际={}", tag(&other)),
        };
        let got2 = h.wait(&ticket2);
        let second = match &got2[0].1 {
            Request::Ready(handle) => handle.clone(),
            Request::Failed(e) => panic!("{e}"),
            Request::Loading(_) => unreachable!(),
        };
        assert_eq!(second.id().generation, 1);
        assert_ne!(first_id, second.id());
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    fn tag<T>(r: &Request<T>) -> &'static str {
        match r {
            Request::Ready(_) => "Ready",
            Request::Loading(_) => "Loading",
            Request::Failed(_) => "Failed",
        }
    }

    #[test]
    fn missing_path_fails_and_retry_reresolves() {
        let mut h = Harness::new(&[]);
        let req = h.store.load(&h.vfs, &mut h.runner, &path("nope.dat"), |_| {
            Ok(String::new())
        });
        let ticket = match req {
            Request::Loading(t) => t,
            _ => panic!("缺文件应走异步失败路径"),
        };
        h.wait(&ticket);
        let cached = h.store.load(&h.vfs, &mut h.runner, &path("nope.dat"), |_| {
            Ok(String::new())
        });
        assert!(
            matches!(cached, Request::Failed(_)),
            "失败要缓存，不能逐帧重试"
        );
        // retry + 补文件（直接塞 provider 不可行——Vfs 已 move 进 harness；
        // 语义验证到此为止：retry 清的是错误缓存）。
        assert!(h.store.retry(&path("nope.dat")));
        assert!(!h.store.retry(&path("nope.dat")));
    }

    #[test]
    fn load_blocking_installs_sync_and_nonblocking_load_is_async() {
        let mut h = Harness::new(&[("b.dat", b"BB".as_slice())]);
        let req = h.store.load_blocking(&path("b.dat"), || Ok("sync".into()));
        assert!(matches!(req, Request::Ready(_)));
        // 已缓存 → 异步 load 直接命中，不经 runner。
        let r2 = h.store.load(
            &h.vfs,
            &mut h.runner,
            &path("b.dat"),
            |_| Ok("never".into()),
        );
        match r2 {
            Request::Ready(handle) => {
                assert_eq!(handle.get(), "sync");
                assert_eq!(
                    handle.id(),
                    AssetId {
                        hash: fnv1a32(b"b.dat\0"),
                        generation: 0
                    }
                );
            }
            _ => panic!("缓存命中应直返"),
        }
        assert_eq!(h.runner.inflight(), 0);
    }
}
