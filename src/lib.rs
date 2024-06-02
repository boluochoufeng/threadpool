mod error;
mod queue;

pub use queue::JobQueue;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

pub type Job = Box<dyn FnOnce() + Send + 'static>;

struct SharedData {
    max_threads_size: AtomicU32,
    alive_count: AtomicU32,
    active_count: AtomicU32,
    is_running: AtomicBool,
    global_queue: JobQueue<Job>,
    threads_idle: Condvar,
    idle_lock: Mutex<()>,
}

impl SharedData {
    fn has_work(&self) -> bool {
        self.active_count.load(Ordering::SeqCst) > 0 || !self.global_queue.is_empty()
    }

    fn increment_alive_count(&self) {
        self.alive_count.fetch_add(1, Ordering::SeqCst);
    }

    fn decrement_alive_count(&self) {
        self.alive_count.fetch_sub(1, Ordering::SeqCst);
    }

    fn increment_active_count(&self) {
        self.active_count.fetch_add(1, Ordering::SeqCst);
    }

    fn decrement_active_count(&self) {
        self.active_count.fetch_sub(1, Ordering::SeqCst);
    }

    fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }
}

pub struct ThreadPool {
    workers: Mutex<Vec<Worker>>,
    shared_data: Arc<SharedData>,
    global_id: AtomicU64,
}

impl ThreadPool {
    pub fn new(max_threads_size: u32, task_queue_size: u32) -> Self {
        let mut max_threads_size = max_threads_size;
        if max_threads_size == 0 {
            max_threads_size = 1;
        }

        let shared_data = Arc::new(SharedData {
            max_threads_size: AtomicU32::new(max_threads_size),
            alive_count: AtomicU32::new(0),
            active_count: AtomicU32::new(0),
            is_running: AtomicBool::new(true),
            global_queue: JobQueue::new(task_queue_size),
            threads_idle: Condvar::new(),
            idle_lock: Mutex::new(()),
        });

        let mut workers = Vec::with_capacity(max_threads_size as usize);
        for i in 0..max_threads_size {
            let worker = Worker::new(i as u64, Arc::clone(&shared_data));
            workers.push(worker);
        }

        ThreadPool {
            workers: Mutex::new(workers),
            shared_data,
            global_id: AtomicU64::new(max_threads_size as u64),
        }
    }

    pub fn execute<F>(&self, job: F)
    where
        F: FnOnce() + Send + 'static,
    {
        if self.shared_data.is_running() {
            return;
        }
        self.shared_data.global_queue.push_back(Box::new(job));
    }

    pub fn set_workers_num(&self, num: u32) {
        if num == 0 {
            return;
        }

        let prev_num = self
            .shared_data
            .max_threads_size
            .swap(num, Ordering::SeqCst);
        if prev_num >= num {
            return;
        }

        let sub = num - prev_num;

        let mut workers = self.workers.lock().unwrap();
        for _ in 0..sub {
            let id = self.global_id.load(Ordering::SeqCst);
            let worker = Worker::new(id, Arc::clone(&self.shared_data));
            workers.push(worker);
        }
    }

    pub fn join(&self) {
        let mut lock = self.shared_data.idle_lock.lock().unwrap();
        while self.shared_data.has_work() {
            lock = self.shared_data.threads_idle.wait(lock).unwrap();
        }
    }

    pub fn terminate(&self) {
        self.shared_data.is_running.store(false, Ordering::SeqCst);
        while self.shared_data.alive_count.load(Ordering::SeqCst) > 0 {
            self.shared_data.global_queue.notify_all();
        }
    }

    pub fn alive_count(&self) -> u32 {
        self.shared_data.alive_count.load(Ordering::SeqCst)
    }

    pub fn job_count(&self) -> u64 {
        self.shared_data.global_queue.len()
    }

    pub fn completed_tasks(&self) -> u64 {
        let mut res = 0u64;
        let workers = self.workers.lock().unwrap();
        for worker in &(*workers) {
            res += worker.completed_tasks();
        }

        res
    }
}

struct Worker {
    id: u64,
    thread: JoinHandle<()>,
    completed_tasks: Arc<AtomicU64>,
}

impl Worker {
    fn new(id: u64, shared_data: Arc<SharedData>) -> Self {
        let completed_tasks = Arc::new(AtomicU64::new(0));
        let tasks_count = Arc::clone(&completed_tasks);
        let thread = thread::spawn(move || {
            shared_data.increment_alive_count();

            while shared_data.is_running() {
                let wc = shared_data.alive_count.load(Ordering::SeqCst);
                let max_worker_count = shared_data.max_threads_size.load(Ordering::SeqCst);
                if wc > max_worker_count {
                    break;
                }

                shared_data.global_queue.wait();
                if !shared_data.is_running() {
                    break;
                }

                shared_data.increment_active_count();

                let job = shared_data.global_queue.pop_front();
                if let Some(job) = job {
                    job();
                    tasks_count.fetch_add(1, Ordering::SeqCst);
                }

                shared_data.decrement_active_count();
                if !shared_data.has_work() {
                    *shared_data.idle_lock.lock().unwrap();
                    shared_data.threads_idle.notify_all();
                }
            }

            shared_data.decrement_alive_count();
        });

        Worker {
            id,
            thread,
            completed_tasks,
        }
    }

    fn completed_tasks(&self) -> u64 {
        self.completed_tasks.load(Ordering::SeqCst)
    }

    fn id(&self) -> u64 {
        self.id
    }
}
