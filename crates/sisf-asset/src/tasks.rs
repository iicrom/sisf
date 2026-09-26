//! `TaskRunner`：解码任务的异步执行。
//!
//! 分工铁律：**解码可以异步，GPU 上传恒在主线程**（wgpu 的 `Queue` 在三端都不是
//! 跨线程对象，且 `Renderer::end_frame` 独占 encoder）。因此本模块只负责把闭包任务
//! 跑掉；结果由任务自己写进捕获的共享通道（见 `store.rs` 的投递方式），主循环
//! `poll_ready` 之后再从 store 收割。
//!
//! 两端语义：
//! - [`NativeTasks`]：每个任务一条 `std::thread`（解码是 ms~几十 ms 级的粗任务，
//!   线程池的调度收益小于复杂度），完成信号走 `mpsc` 回主线程。
//! - `WasmTasks`（只在 wasm32 上存在）：协程式——`submit` 只入队（立即返回），`poll_ready` 逐个就地
//!   执行（单个解码 ≪ 16ms，超时保护留给 T12 的 perf 门）。网络 fetch 不在这里，
//!   由 `vfs::WebManifest` 的宿主侧预取完成，保证 `VfsProvider::read` 同步。
//! - [`load_blocking`]：启动期同步加载——native 就地跑（调用线程即工作线程，
//!   启动阶段没必要再过一道 handoff），wasm 同样就地跑。

#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{Sender, channel};

/// 一个已投递任务：native 要求 Send（跨线程），wasm 单线程无此约束。
#[cfg(not(target_arch = "wasm32"))]
pub type Job = Box<dyn FnOnce() + Send + 'static>;

/// wasm：允许捕获非 Send 的 `Rc` 族对象。
#[cfg(target_arch = "wasm32")]
pub type Job = Box<dyn FnOnce() + 'static>;

/// 把闭包装箱成 `Job`（native 版本顺带在编译期检查 Send——Send 化的判断点收敛在
/// 这一处，store 里不出现 cfg）。
#[cfg(not(target_arch = "wasm32"))]
pub fn make_job(f: impl FnOnce() + Send + 'static) -> Job {
    Box::new(f)
}

#[cfg(target_arch = "wasm32")]
pub fn make_job(f: impl FnOnce() + 'static) -> Job {
    Box::new(f)
}

/// 任务执行器。主线程每帧 `poll_ready` 收割已完成任务（不阻塞）。
pub trait TaskRunner {
    /// 投递任务。native 立即返回（线程在跑）；wasm 入队，`poll_ready` 时就地执行。
    fn submit(&mut self, job: Job);

    /// 非阻塞收割已完成任务：native 排空信号队列，wasm 跑完整个待办队列。
    fn poll_ready(&mut self);

    /// 当前排队/在飞任务数（诊断与"非阻塞 load"验收用）。
    fn inflight(&self) -> usize;
}

/// 启动期同步加载：就地执行 `f` 并返回其结果。两端同语义（native 不起线程——
/// 调用点在主线程且后面紧跟 `store.deliver`，再起线程只是白付一次 handoff）。
pub fn load_blocking<R, F: FnOnce() -> R>(f: F) -> R {
    f()
}

/// 解码线程栈：递归解码（png/zlib）在默认 2MiB 栈上可能不够。
#[cfg(not(target_arch = "wasm32"))]
const JOB_STACK: usize = 8 << 20;

/// native：每任务一条线程 + mpsc 完成信号。
#[cfg(not(target_arch = "wasm32"))]
pub struct NativeTasks {
    done_tx: Sender<()>,
    done_rx: std::sync::mpsc::Receiver<()>,
    inflight: usize,
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for NativeTasks {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl NativeTasks {
    pub fn new() -> Self {
        let (done_tx, done_rx) = channel();
        Self {
            done_tx,
            done_rx,
            inflight: 0,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl TaskRunner for NativeTasks {
    fn submit(&mut self, job: Job) {
        let tx = self.done_tx.clone();
        self.inflight += 1;
        match std::thread::Builder::new()
            .stack_size(JOB_STACK)
            .spawn(move || {
                // 任务 panic 时也要回信号：否则 inflight 永远归不了零。
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                let _ = tx.send(());
                if let Err(p) = result {
                    std::panic::resume_unwind(p);
                }
            }) {
            Ok(_) => {}
            Err(e) => {
                self.inflight -= 1;
                panic!("NativeTasks: 无法起线程: {e}");
            }
        }
    }

    fn poll_ready(&mut self) {
        while self.done_rx.try_recv().is_ok() {
            self.inflight -= 1;
        }
    }

    fn inflight(&self) -> usize {
        self.inflight
    }
}

/// wasm：协程队列，`poll_ready` 就地跑完。
#[cfg(target_arch = "wasm32")]
pub struct WasmTasks {
    queue: Vec<Job>,
    inflight: usize,
}

#[cfg(target_arch = "wasm32")]
impl Default for WasmTasks {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_arch = "wasm32")]
impl WasmTasks {
    pub fn new() -> Self {
        Self {
            queue: Vec::new(),
            inflight: 0,
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl TaskRunner for WasmTasks {
    fn submit(&mut self, job: Job) {
        self.inflight += 1;
        self.queue.push(job);
    }

    fn poll_ready(&mut self) {
        for job in self.queue.drain(..) {
            job();
            self.inflight -= 1;
        }
    }

    fn inflight(&self) -> usize {
        self.inflight
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// 两端各自的 runner（原生测试实际跑 NativeTasks；wasm 侧至少保证编译）。
    fn make_runner() -> impl TaskRunner {
        #[cfg(not(target_arch = "wasm32"))]
        return NativeTasks::new();
        #[cfg(target_arch = "wasm32")]
        return WasmTasks::new();
    }

    #[test]
    fn submit_reports_inflight_and_clears_on_poll() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut tasks = make_runner();
        for _ in 0..8 {
            let c = counter.clone();
            tasks.submit(make_job(move || {
                c.fetch_add(1, Ordering::SeqCst);
            }));
        }
        assert_eq!(tasks.inflight(), 8, "submit 应已立即返回");
        for _ in 0..1000 {
            tasks.poll_ready();
            if tasks.inflight() == 0 {
                break;
            }
            nap();
        }
        assert_eq!(tasks.inflight(), 0);
        assert_eq!(counter.load(Ordering::SeqCst), 8);
    }

    #[test]
    fn load_blocking_returns_value_inline() {
        assert_eq!(load_blocking(|| 6u32 * 7), 42);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn submit_does_not_block_the_caller() {
        use std::time::Instant;
        let mut tasks = NativeTasks::new();
        let started = Instant::now();
        tasks.submit(make_job(|| {
            std::thread::sleep(std::time::Duration::from_millis(120))
        }));
        // 单帧预算 16ms：submit 若阻塞在任务上，这里就超了。
        assert!(started.elapsed() < std::time::Duration::from_millis(16));
        while tasks.inflight() > 0 {
            tasks.poll_ready();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn nap() {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    #[cfg(target_arch = "wasm32")]
    fn nap() {}
}
