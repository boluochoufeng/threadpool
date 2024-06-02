use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

pub struct JobQueue<T> {
    queue: Mutex<VecDeque<T>>,
    no_empty: Condvar,
    v: Mutex<i32>,
}

impl<T> JobQueue<T> {
    pub fn new(max_size: u32) -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            no_empty: Condvar::new(),
            v: Mutex::new(0),
        }
    }

    pub fn wait(&self) {
        let mut v = self.v.lock().unwrap();
        while *v != 1 {
            v = self.no_empty.wait(v).unwrap();
        }
        *v = 0;
    }

    pub fn wait_time_out(&self, time_out: Duration) {
        let v = self.v.lock().unwrap();
        let (mut v, _) = self.no_empty.wait_timeout(v, time_out).unwrap();
        *v = 0;
    }

    pub fn notify_one(&self) {
        let mut v = self.v.lock().unwrap();
        *v = 1;
        self.no_empty.notify_one();
    }

    pub fn notify_all(&self) {
        let mut v = self.v.lock().unwrap();
        *v = 1;
        self.no_empty.notify_all();
    }

    pub fn push_back(&self, t: T) {
        let mut queue = self.queue.lock().unwrap();
        queue.push_back(t);
        self.notify_one();
    }

    pub fn pop_front(&self) -> Option<T> {
        let mut queue = self.queue.lock().unwrap();
        let t = queue.pop_front();
        if queue.len() > 0 {
            self.notify_one();
        }
        t
    }

    pub fn is_empty(&self) -> bool {
        self.queue.lock().unwrap().is_empty()
    }

    pub fn len(&self) -> u64 {
        self.queue.lock().unwrap().len() as u64
    }
}
