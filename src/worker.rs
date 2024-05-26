use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::BlockQueue;

pub type Job = Box<dyn FnOnce() + Send + 'static>;

pub struct Worker {
    id: u64,
    thread: JoinHandle<()>,
    running: Arc<AtomicBool>,
    task_queue: Arc<BlockQueue<Job>>,
    keep_alive_time: Duration,
}

impl Worker {
    pub fn new(
        id: u64,
        task_queue: Arc<BlockQueue<Job>>,
        keep_alive_time: Duration,
        allow_core_thread_time_out: bool,
        job: Option<Job>,
    ) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let running_state = Arc::clone(&running);

        let thread = if job.is_none() {
            thread::spawn(move || {
                let allow_core_thread_time_out = allow_core_thread_time_out;
                while running_state.load(Ordering::SeqCst) {}
            })
        } else {
            let first_job = job.unwrap();
            thread::spawn(move || {
                let allow_core_thread_time_out = allow_core_thread_time_out;
                if running_state.load(Ordering::SeqCst) {
                    first_job()
                }
                while running_state.load(Ordering::SeqCst) {}
            })
        };

        Worker {
            id,
            thread,
            running,
            task_queue,
            keep_alive_time,
        }
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}
