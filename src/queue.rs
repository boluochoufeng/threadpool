use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

pub struct BlockQueue<T> {
    queue: Mutex<VecDeque<T>>,
    no_empty: Condvar,
    no_full: Condvar,
    wait_time: Duration,
    max_size: u32,
}

impl<T> BlockQueue<T> {
    pub fn new(size: u32, wait_time: u64) -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            no_empty: Condvar::new(),
            no_full: Condvar::new(),
            wait_time: Duration::from_millis(wait_time),
            max_size: size,
        }
    }

    pub fn push_back(&self, t: T) -> Option<T> {
        let mut q = self.queue.lock().unwrap();
        while q.len() as u32 == self.max_size {
            let (nq, res) = self.no_full.wait_timeout(q, self.wait_time).unwrap();
            if res.timed_out() {
                return Some(t);
            }
            q = nq;
        }

        q.push_back(t);
        self.no_empty.notify_one();

        None
    }

    pub fn pop_front(&self) -> Option<T> {
        let mut q = self.queue.lock().unwrap();
        while q.len() == 0 {
            let (nq, res) = self.no_empty.wait_timeout(q, self.wait_time).unwrap();
            if res.timed_out() {
                return None;
            }
            q = nq;
        }

        let res = q.pop_front();
        self.no_full.notify_one();

        res
    }

    pub fn len(&self) -> usize {
        self.queue.lock().unwrap().len()
    }
}

#[cfg(test)]
mod test {
    use std::{
        sync::{
            atomic::{AtomicI32, Ordering},
            Arc,
        },
        thread,
        time::Instant,
    };

    use crate::BlockQueue;

    #[test]
    fn bq_len() {
        let q = BlockQueue::<i32>::new(1000, 1000);
        assert_eq!(q.len(), 0);
    }

    #[test]
    fn bq_push_and_pop() {
        let q = BlockQueue::<i32>::new(1000, 1000);
        q.push_back(0);
        q.push_back(1);
        q.push_back(2);
        assert_eq!(q.len(), 3);
        assert_eq!(q.pop_front().unwrap(), 0);
        assert_eq!(q.pop_front().unwrap(), 1);
        assert_eq!(q.pop_front().unwrap(), 2);
        assert_eq!(q.len(), 0);
    }

    #[test]
    fn bq_time_out() {
        let q = BlockQueue::<i32>::new(1000, 1000);
        for _ in 0..20 {
            q.push_back(0);
            q.push_back(1);
            q.push_back(2);
            q.pop_front();
            q.pop_front();
            q.pop_front();

            let start = Instant::now();
            assert_eq!(q.pop_front(), None);
            let end = Instant::now();
            // println!("{} {}", end.elapsed().as_millis(), start.elapsed().as_millis());
            // println!("{}", (end - start).as_millis());
            assert!((end - start).as_millis() >= 999);
        }
    }

    #[test]
    fn bq_thread() {
        let q = Arc::new(BlockQueue::<i32>::new(1000, 1000));

        let q1 = Arc::clone(&q);
        let t1 = thread::spawn(move || {
            start_push_thread(q1);
        });

        let q2 = Arc::clone(&q);
        let res = Arc::new(AtomicI32::new(0));
        let res_t = Arc::clone(&res);
        let t2 = thread::spawn(move || {
            start_pop_thread(q2, res_t);
        });

        t1.join().unwrap();
        t2.join().unwrap();

        assert!(res.load(Ordering::Relaxed) <= 100 * 99 / 2);
        assert_eq!(q.len(), 0);
    }

    fn start_push_thread(q: Arc<BlockQueue<i32>>) {
        let mut vec = vec![];
        for i in 0..100 {
            let queue = Arc::clone(&q);
            let j = thread::spawn(move || {
                queue.push_back(i);
                // println!("push {}", i);
            });
            vec.push(j);
        }
        vec.into_iter().for_each(|j| j.join().unwrap());
    }

    fn start_pop_thread(q: Arc<BlockQueue<i32>>, res: Arc<AtomicI32>) {
        let mut vec = vec![];
        for _ in 0..100 {
            let queue = Arc::clone(&q);
            let res = Arc::clone(&res);
            let j = thread::spawn(move || {
                let i = queue.pop_front().unwrap();
                res.fetch_add(i, Ordering::SeqCst);
                // println!("pop {}", id);
            });
            vec.push(j);
        }
        vec.into_iter().for_each(|j| j.join().unwrap());
    }
}
