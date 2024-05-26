mod error;
mod queue;
mod worker;

pub use queue::BlockQueue;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use worker::{Job, Worker};

const COUNT_BITS: u32 = u32::BITS - 3;
const COUNT_MASK: u32 = (1 << COUNT_BITS) - 1;
const RUNING: u32 = 1 << COUNT_BITS;

fn worker_count_of(c: u32) -> u32 {
    c & COUNT_MASK
}

fn ctl_of(rs: u32, wc: u32) -> u32 {
    rs | wc
}

struct Config {
    max_thread_size: u32,
    core_thread_size: u32,
    keep_alive_time: Duration,
    allow_core_thread_time_out: bool,
}

pub struct ThreadPool {
    global_queue: Arc<BlockQueue<Job>>,
    config: Config,
    workers: Mutex<Vec<Worker>>,
    ctl: AtomicU32,
    global_id: AtomicU64,
}

impl ThreadPool {
    pub fn new(
        core_thread_size: u32,
        max_thread_size: u32,
        max_task_queue_size: u32,
        keep_alive_time: u64,
        allow_core_thread_time_out: bool,
    ) -> Self {
        let core_thread_size = if core_thread_size > max_thread_size {
            max_thread_size
        } else {
            core_thread_size
        };

        let config = Config {
            max_thread_size,
            core_thread_size,
            keep_alive_time: Duration::from_millis(keep_alive_time),
            allow_core_thread_time_out,
        };

        let workers = Mutex::new(Vec::with_capacity(core_thread_size as usize));
        let global_queue = Arc::new(BlockQueue::new(max_task_queue_size, keep_alive_time));

        ThreadPool {
            global_queue,
            config,
            workers,
            ctl: AtomicU32::new(ctl_of(RUNING, 0)),
            global_id: AtomicU64::new(0),
        }
    }

    pub fn execute<T>(&self, job: T)
    where
        T: FnOnce() + Send + 'static,
    {
        let core_thread_size = self.config.core_thread_size;

        let mut c = self.ctl.load(Ordering::SeqCst);
        if worker_count_of(c) < core_thread_size {
            if self.add_worker(true, None) {
                self.global_queue.push_back(Box::new(job));
                return;
            }
            c = self.ctl.load(Ordering::SeqCst);
        }

        let job = Box::new(job);
        if let Some(job) = self.global_queue.push_back(job) {
            if self.add_worker(false, Some(job)) {
                // push失败，执行拒绝策略
            }
        }
    }

    fn add_worker(&self, core: bool, job: Option<Job>) -> bool {
        let c = self.ctl.load(Ordering::SeqCst);
        let thread_size = if core {
            self.config.core_thread_size & COUNT_MASK
        } else {
            self.config.max_thread_size & COUNT_MASK
        };

        loop {
            if worker_count_of(c) >= thread_size {
                return false;
            }
            if self.increment_worker_count(c) {
                break;
            }
        }

        let id = self.global_id.fetch_add(1, Ordering::SeqCst);
        let worker = if core {
            Worker::new(
                id,
                Arc::clone(&self.global_queue),
                self.config.keep_alive_time,
                self.config.allow_core_thread_time_out,
                None,
            )
        } else {
            Worker::new(
                id,
                Arc::clone(&self.global_queue),
                self.config.keep_alive_time,
                self.config.allow_core_thread_time_out,
                job,
            )
        };

        self.workers.lock().unwrap().push(worker);

        true
    }

    fn increment_worker_count(&self, expect: u32) -> bool {
        self.ctl
            .compare_exchange(expect, expect + 1, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    fn decrement_worker_count(&self, expect: u32) -> bool {
        self.ctl
            .compare_exchange(expect, expect - 1, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }
}
