//! Background work. Two kinds, because they behave in opposite ways.
//!
//! A **wishlist** ([`Wishlist`]) is for work that only makes sense while it
//! can be seen — tile thumbnails. It is not a queue: the caller overwrites
//! the list every frame with whatever is needed right now, and whatever drops
//! out of it is never done. Cancellation is implicit and free.
//!
//! A queue is deliberately not used here. The prototype had one and it meant
//! this: dragging the scrollbar across a library enqueued thousands of
//! photographs, and once the handle was released, the fifteen somebody was
//! actually looking at came up behind several thousand dead requests. Seven
//! seconds of blank tiles.
//!
//! A **task** ([`Tasks`]) is the opposite: scanning a folder, generating
//! thumbnails, exporting. It runs to the end or to an explicit cancellation,
//! reports progress, and must not disappear unnoticed when it fails.

use crossbeam_channel::{Receiver, Sender};
use std::collections::HashSet;
use std::hash::Hash;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

/// How many threads to leave to the work, so the UI and the system still
/// get some.
pub fn worker_count() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(2).max(1))
        .unwrap_or(4)
}

// -------------------------------------------------------------------- wishlist

struct WishState<K> {
    /// Groups by priority; a lower index is served first.
    lanes: Vec<Vec<K>>,
    /// What some thread is holding right now.
    running: HashSet<K>,
    /// What is already done and sent. Without this, a thread would finish an
    /// item, the caller would not have collected it yet, the next wish would
    /// contain it again — and another thread would do it a second time.
    done: HashSet<K>,
    stop: bool,
}

impl<K: Eq + Hash + Clone> WishState<K> {
    fn take(&mut self) -> Option<K> {
        for lane in &self.lanes {
            if let Some(key) = lane
                .iter()
                .find(|key| !self.running.contains(key) && !self.done.contains(key))
                .cloned()
            {
                self.running.insert(key.clone());
                return Some(key);
            }
        }

        None
    }
}

/// A thread pool that does only what is on the list right now.
#[derive(Debug)]
pub struct Wishlist<K, V> {
    shared: Arc<WishShared<K>>,
    rx: Receiver<(K, V)>,
}

struct WishShared<K> {
    state: Mutex<WishState<K>>,
    wake: Condvar,
}

impl<K> std::fmt::Debug for WishShared<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WishShared")
    }
}

impl<K, V> Wishlist<K, V>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: Send + 'static,
{
    /// `work` is handed a key and returns a result, or `None` when it could
    /// not be done. A failure is remembered, so the same thing is not
    /// attempted over and over forever.
    pub fn new<F>(threads: usize, work: F) -> Self
    where
        F: Fn(&K) -> Option<V> + Send + Sync + 'static,
    {
        let shared = Arc::new(WishShared {
            state: Mutex::new(WishState {
                lanes: Vec::new(),
                running: HashSet::new(),
                done: HashSet::new(),
                stop: false,
            }),
            wake: Condvar::new(),
        });
        let (tx, rx) = crossbeam_channel::unbounded();
        let work = Arc::new(work);
        for _ in 0..threads.max(1) {
            spawn_wish_worker(shared.clone(), work.clone(), tx.clone());
        }

        Self { shared, rx }
    }

    /// Overwrites the list. Called as often as needed — every frame is fine.
    pub fn wish(&self, lanes: Vec<Vec<K>>) {
        let mut state = self.shared.state.lock().expect("poisoned lock");
        if state.lanes == lanes {
            return;
        }

        state.lanes = lanes;
        drop(state);
        self.shared.wake.notify_all();
    }

    /// Collects what is finished, at most `limit` at a time. That is how the
    /// caller decides how much work it takes on in one frame.
    pub fn drain(&self, limit: usize) -> Vec<(K, V)> {
        let mut out = Vec::new();
        while out.len() < limit {
            match self.rx.try_recv() {
                Ok(item) => out.push(item),
                Err(_) => break,
            }
        }

        out
    }

    /// Forgets that a key was done. Called when the result fell out of the
    /// cache and will be needed again.
    pub fn forget(&self, key: &K) {
        self.shared
            .state
            .lock()
            .expect("poisoned lock")
            .done
            .remove(key);
    }

    /// How many items are in flight right now.
    pub fn running(&self) -> usize {
        self.shared
            .state
            .lock()
            .expect("poisoned lock")
            .running
            .len()
    }
}

impl<K, V> Drop for Wishlist<K, V> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.shared.state.lock() {
            state.stop = true;
        }

        self.shared.wake.notify_all();
    }
}

fn spawn_wish_worker<K, V, F>(shared: Arc<WishShared<K>>, work: Arc<F>, tx: Sender<(K, V)>)
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: Send + 'static,
    F: Fn(&K) -> Option<V> + Send + Sync + 'static,
{
    std::thread::spawn(move || {
        loop {
            let key = {
                let mut state = shared.state.lock().expect("poisoned lock");
                loop {
                    if state.stop {
                        return;
                    }

                    if let Some(key) = state.take() {
                        break key;
                    }

                    state = shared.wake.wait(state).expect("poisoned lock");
                }
            };

            let value = work(&key);
            {
                let mut state = shared.state.lock().expect("poisoned lock");
                state.running.remove(&key);
                state.done.insert(key.clone());
            }

            if let Some(value) = value
                && tx.send((key, value)).is_err()
            {
                return;
            }
        }
    });
}

// ----------------------------------------------------------------------- tasks

/// Tells a task that nobody wants it any more.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// Says nobody wants this any more.
    ///
    /// Public because a task can decide to stop itself — and because a test
    /// of something that takes a `Cancel` has to be able to set one without
    /// standing up a whole task board.
    pub fn stop(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// How a task reports on how it is getting on.
#[derive(Debug, Clone)]
pub struct Progress {
    id: u64,
    board: Arc<Mutex<Vec<TaskStatus>>>,
}

impl Progress {
    pub fn report(&self, done: u64, total: Option<u64>, message: impl Into<String>) {
        let message = message.into();
        if let Ok(mut board) = self.board.lock()
            && let Some(task) = board.iter_mut().find(|t| t.id == self.id)
        {
            task.done = done;
            task.total = total;
            task.message = message;
        }
    }
}

/// How a task stands. This is what a person sees in the status bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStatus {
    pub id: u64,
    pub title: String,
    pub done: u64,
    pub total: Option<u64>,
    pub message: String,
    pub finished: bool,
    pub cancelled: bool,
    /// Why it failed. A task never fails in silence.
    pub error: Option<String>,
}

impl TaskStatus {
    /// The fraction done, when the whole is known.
    pub fn fraction(&self) -> Option<f32> {
        self.total
            .filter(|total| *total > 0)
            .map(|total| (self.done as f32 / total as f32).clamp(0.0, 1.0))
    }
}

/// Long-running work with progress and cancellation.
#[derive(Debug, Clone, Default)]
pub struct Tasks {
    board: Arc<Mutex<Vec<TaskStatus>>>,
    cancels: Arc<Mutex<Vec<(u64, Cancel)>>>,
    next: Arc<AtomicU64>,
}

impl Tasks {
    pub fn new() -> Self {
        Self::default()
    }

    /// Puts a task on the board and hands back what it needs to report.
    fn enrol(&self, title: &str) -> (u64, Cancel, Progress) {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let cancel = Cancel::default();
        self.board.lock().expect("poisoned lock").push(TaskStatus {
            id,
            title: title.to_owned(),
            done: 0,
            total: None,
            message: String::new(),
            finished: false,
            cancelled: false,
            error: None,
        });
        self.cancels
            .lock()
            .expect("poisoned lock")
            .push((id, cancel.clone()));
        let progress = Progress {
            id,
            board: self.board.clone(),
        };
        (id, cancel, progress)
    }

    /// Marks a task done, and says in the log how it went.
    fn settle(&self, id: u64, title: &str, cancelled: bool, error: Option<String>) {
        if let Ok(mut board) = self.board.lock()
            && let Some(task) = board.iter_mut().find(|t| t.id == id)
        {
            task.finished = true;
            task.cancelled = cancelled;
            task.error = error.clone();
        }

        match error {
            None => tracing::info!(%id, %title, "task finished"),
            // A failed task has to surface; this is the spot where an
            // application typically pretends all is well.
            Some(error) => tracing::error!(%id, %title, %error, "task failed"),
        }
    }

    /// Runs work on the calling thread, with the same progress and
    /// cancellation a spawned one gets.
    ///
    /// What a command line uses: it has nothing else to do while it waits,
    /// and a thread it would only turn round and join is a thread for
    /// nothing. It reports the same way, so anything watching the board
    /// sees a task either way.
    pub fn here<T>(
        &self,
        title: impl Into<String>,
        work: impl FnOnce(&Cancel, &Progress) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let title = title.into();
        let (id, cancel, progress) = self.enrol(&title);
        let outcome = work(&cancel, &progress);
        self.settle(
            id,
            &title,
            cancel.cancelled(),
            outcome.as_ref().err().map(|error| format!("{error:#}")),
        );
        outcome
    }

    pub fn spawn<F>(&self, title: impl Into<String>, work: F) -> u64
    where
        F: FnOnce(&Cancel, &Progress) -> anyhow::Result<()> + Send + 'static,
    {
        let title = title.into();
        let (id, cancel, progress) = self.enrol(&title);
        let board = self.board.clone();
        std::thread::spawn(move || {
            let outcome = work(&cancel, &progress);
            let error = outcome.as_ref().err().map(|error| format!("{error:#}"));
            if let Ok(mut board) = board.lock()
                && let Some(task) = board.iter_mut().find(|t| t.id == id)
            {
                task.finished = true;
                task.cancelled = cancel.cancelled();
                task.error = error.clone();
            }

            match error {
                None => tracing::info!(%id, %title, "task finished"),
                // A failed task has to surface; this is the spot where an
                // application typically pretends all is well.
                Some(error) => tracing::error!(%id, %title, %error, "task failed"),
            }
        });

        id
    }

    pub fn snapshot(&self) -> Vec<TaskStatus> {
        self.board.lock().expect("poisoned lock").clone()
    }

    /// Running tasks, which is what belongs in the status bar.
    pub fn running(&self) -> Vec<TaskStatus> {
        self.snapshot()
            .into_iter()
            .filter(|t| !t.finished)
            .collect()
    }

    /// Tasks that failed and that nobody has been told about yet.
    pub fn failures(&self) -> Vec<TaskStatus> {
        self.snapshot()
            .into_iter()
            .filter(|t| t.error.is_some())
            .collect()
    }

    pub fn cancel(&self, id: u64) {
        if let Some((_, cancel)) = self
            .cancels
            .lock()
            .expect("poisoned lock")
            .iter()
            .find(|(task, _)| *task == id)
        {
            cancel.stop();
        }
    }

    /// Clears away finished tasks there is nothing left to say about.
    pub fn forget_finished(&self) {
        let mut board = self.board.lock().expect("poisoned lock");
        board.retain(|task| !task.finished || task.error.is_some());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait_for(mut ready: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if ready() {
                return true;
            }

            std::thread::sleep(Duration::from_millis(5));
        }

        false
    }

    #[test]
    fn a_wishlist_does_what_is_on_it() {
        let pool: Wishlist<u32, u32> = Wishlist::new(4, |key| Some(key * 2));
        pool.wish(vec![vec![1, 2, 3]]);

        let mut got = Vec::new();
        assert!(wait_for(|| {
            got.extend(pool.drain(16));
            got.len() == 3
        }));
        got.sort();
        assert_eq!(got, vec![(1, 2), (2, 4), (3, 6)]);
    }

    #[test]
    fn what_drops_off_the_list_is_never_done() {
        // This is the fault that cost the prototype seven seconds: work
        // ordered while dragging has to be discardable before it is done.
        let started = Arc::new(Mutex::new(Vec::new()));
        let seen = started.clone();
        let pool: Wishlist<u32, u32> = Wishlist::new(1, move |key| {
            seen.lock().unwrap().push(*key);
            std::thread::sleep(Duration::from_millis(30));
            Some(*key)
        });

        pool.wish(vec![vec![1]]);
        assert!(wait_for(|| !started.lock().unwrap().is_empty()));
        // Before the first one finishes, the wish is rewritten elsewhere.
        pool.wish(vec![vec![99]]);

        assert!(wait_for(|| started.lock().unwrap().contains(&99)));
        let done = started.lock().unwrap().clone();
        assert!(
            !done.contains(&2),
            "a discarded wish must not be carried out: {done:?}"
        );
    }

    #[test]
    fn the_first_group_takes_precedence() {
        let order = Arc::new(Mutex::new(Vec::new()));
        let seen = order.clone();
        let pool: Wishlist<u32, u32> = Wishlist::new(1, move |key| {
            seen.lock().unwrap().push(*key);
            Some(*key)
        });
        pool.wish(vec![vec![10], vec![20, 21]]);

        assert!(wait_for(|| order.lock().unwrap().len() == 3));
        assert_eq!(
            order.lock().unwrap()[0],
            10,
            "the priority group goes first"
        );
    }

    #[test]
    fn the_same_thing_is_not_done_twice() {
        let count = Arc::new(AtomicU64::new(0));
        let counter = count.clone();
        let pool: Wishlist<u32, u32> = Wishlist::new(4, move |key| {
            counter.fetch_add(1, Ordering::Relaxed);
            Some(*key)
        });
        for _ in 0..20 {
            pool.wish(vec![vec![7]]);
            pool.wish(vec![vec![7, 8]]);
        }

        assert!(wait_for(|| count.load(Ordering::Relaxed) >= 2));
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(count.load(Ordering::Relaxed), 2, "every key exactly once");
    }

    #[test]
    fn a_task_reports_progress_and_finishes() {
        let tasks = Tasks::new();
        let id = tasks.spawn("scan", |_, progress| {
            for done in 1..=4 {
                progress.report(done, Some(4), format!("krok {done}"));
            }

            Ok(())
        });

        assert!(wait_for(|| tasks
            .snapshot()
            .iter()
            .any(|t| t.id == id && t.finished)));
        let task = tasks.snapshot().into_iter().find(|t| t.id == id).unwrap();
        assert_eq!(task.done, 4);
        assert_eq!(task.fraction(), Some(1.0));
        assert!(task.error.is_none());
    }

    #[test]
    fn a_failed_task_is_lost_nowhere() {
        let tasks = Tasks::new();
        tasks.spawn("broken", |_, _| anyhow::bail!("it burst"));
        assert!(wait_for(|| !tasks.failures().is_empty()));
        assert!(
            tasks.failures()[0]
                .error
                .as_ref()
                .unwrap()
                .contains("it burst")
        );
    }

    #[test]
    fn a_task_notices_cancellation() {
        let tasks = Tasks::new();
        let id = tasks.spawn("long one", |cancel, _| {
            while !cancel.cancelled() {
                std::thread::sleep(Duration::from_millis(2));
            }

            Ok(())
        });
        tasks.cancel(id);
        assert!(wait_for(|| tasks
            .snapshot()
            .iter()
            .any(|t| t.id == id && t.cancelled)));
    }
}
