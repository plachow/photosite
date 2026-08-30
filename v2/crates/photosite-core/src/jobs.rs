//! Práce na pozadí. Dva druhy, protože se chovají opačně.
//!
//! **Seznam přání** ([`Wishlist`]) je pro práci, která má smysl jen dokud je
//! na ni vidět — náhledy dlaždic. Nejde o frontu: volající seznam každý snímek
//! přepíše na to, co je právě potřeba, a co z něj vypadne, se nikdy neudělá.
//! Rušení je tím implicitní a zdarma.
//!
//! Fronta se tu nepoužívá schválně. Prototyp ji měl a znamenala tohle: při
//! tažení scrollbarem přes knihovnu se do ní zařadily tisíce fotek, a po
//! puštění handle se těch patnáct, na které se člověk skutečně díval, dostalo
//! na řadu až za několika tisíci mrtvými požadavky. Sedm sekund prázdných
//! dlaždic.
//!
//! **Úloha** ([`Tasks`]) je opak: sken složky, generování náhledů, export.
//! Doběhne do konce nebo do výslovného zrušení, hlásí průběh a nesmí zmizet
//! bez povšimnutí, když selže.

use crossbeam_channel::{Receiver, Sender};
use std::collections::HashSet;
use std::hash::Hash;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

/// Kolik vláken nechat na práci, aby zbylo na UI a na systém.
pub fn worker_count() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(2).max(1))
        .unwrap_or(4)
}

// ---------------------------------------------------------------- seznam přání

struct WishState<K> {
    /// Skupiny podle priority; nižší index se obsluhuje dřív.
    lanes: Vec<Vec<K>>,
    /// Co právě drží nějaké vlákno.
    running: HashSet<K>,
    /// Co už bylo hotové a odesláno. Bez tohohle by vlákno dokončilo položku,
    /// volající by ji ještě nestihl převzít, příští přání by ji obsahovalo
    /// znovu — a jiné vlákno by ji udělalo podruhé.
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

/// Bazén vláken, který dělá jen to, co je právě na seznamu.
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
    /// `work` dostane klíč a vrátí výsledek, nebo `None`, když to nešlo.
    /// Neúspěch se pamatuje, aby se totéž nezkoušelo donekonečna dokola.
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

    /// Přepíše seznam. Volá se tak často, jak je potřeba — klidně každý snímek.
    pub fn wish(&self, lanes: Vec<Vec<K>>) {
        let mut state = self.shared.state.lock().expect("otrávený zámek");
        if state.lanes == lanes {
            return;
        }

        state.lanes = lanes;
        drop(state);
        self.shared.wake.notify_all();
    }

    /// Vybere hotové, nejvýš `limit` najednou. Volající si tím řídí, kolik
    /// práce si na sebe naloží v jednom snímku.
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

    /// Zapomene, že se klíč už udělal. Volá se, když výsledek vypadl z cache
    /// a bude potřeba znovu.
    pub fn forget(&self, key: &K) {
        self.shared
            .state
            .lock()
            .expect("otrávený zámek")
            .done
            .remove(key);
    }

    /// Kolik položek je právě rozpracovaných.
    pub fn running(&self) -> usize {
        self.shared
            .state
            .lock()
            .expect("otrávený zámek")
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
                let mut state = shared.state.lock().expect("otrávený zámek");
                loop {
                    if state.stop {
                        return;
                    }

                    if let Some(key) = state.take() {
                        break key;
                    }

                    state = shared.wake.wait(state).expect("otrávený zámek");
                }
            };

            let value = work(&key);
            {
                let mut state = shared.state.lock().expect("otrávený zámek");
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

// ---------------------------------------------------------------------- úlohy

/// Podává úloze zprávu, že už ji nikdo nechce.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Kudy úloha hlásí, jak jí to jde.
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

/// Jak na tom úloha je. Tohle vidí uživatel ve stavovém řádku.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStatus {
    pub id: u64,
    pub title: String,
    pub done: u64,
    pub total: Option<u64>,
    pub message: String,
    pub finished: bool,
    pub cancelled: bool,
    /// Proč to spadlo. Úloha nikdy neselže mlčky.
    pub error: Option<String>,
}

impl TaskStatus {
    /// Podíl hotového, když je známý celek.
    pub fn fraction(&self) -> Option<f32> {
        self.total
            .filter(|total| *total > 0)
            .map(|total| (self.done as f32 / total as f32).clamp(0.0, 1.0))
    }
}

/// Dlouho běžící práce s průběhem a rušením.
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

    pub fn spawn<F>(&self, title: impl Into<String>, work: F) -> u64
    where
        F: FnOnce(&Cancel, &Progress) -> anyhow::Result<()> + Send + 'static,
    {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let title = title.into();
        let cancel = Cancel::default();
        self.board.lock().expect("otrávený zámek").push(TaskStatus {
            id,
            title: title.clone(),
            done: 0,
            total: None,
            message: String::new(),
            finished: false,
            cancelled: false,
            error: None,
        });
        self.cancels
            .lock()
            .expect("otrávený zámek")
            .push((id, cancel.clone()));

        let board = self.board.clone();
        let progress = Progress {
            id,
            board: board.clone(),
        };
        std::thread::spawn(move || {
            let outcome = work(&cancel, &progress);
            if let Ok(mut board) = board.lock()
                && let Some(task) = board.iter_mut().find(|t| t.id == id)
            {
                task.finished = true;
                task.cancelled = cancel.cancelled();
                if let Err(error) = &outcome {
                    task.error = Some(format!("{error:#}"));
                }
            }

            match outcome {
                Ok(()) => tracing::info!(%id, %title, "úloha hotová"),
                // Selhaná úloha se musí objevit; tohle je to místo, kde se
                // aplikace typicky tváří, že je všechno v pořádku.
                Err(error) => {
                    tracing::error!(%id, %title, error = %format!("{error:#}"), "úloha selhala")
                }
            }
        });

        id
    }

    pub fn snapshot(&self) -> Vec<TaskStatus> {
        self.board.lock().expect("otrávený zámek").clone()
    }

    /// Běžící úlohy, tedy to, co patří do stavového řádku.
    pub fn running(&self) -> Vec<TaskStatus> {
        self.snapshot()
            .into_iter()
            .filter(|t| !t.finished)
            .collect()
    }

    /// Úlohy, které selhaly a člověk o nich ještě neví.
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
            .expect("otrávený zámek")
            .iter()
            .find(|(task, _)| *task == id)
        {
            cancel.cancel();
        }
    }

    /// Uklidí dokončené úlohy, o kterých už není co říct.
    pub fn forget_finished(&self) {
        let mut board = self.board.lock().expect("otrávený zámek");
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
    fn seznam_prani_udela_co_je_na_nem() {
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
    fn co_ze_seznamu_vypadne_se_nikdy_neudela() {
        // Tohle je ta chyba, kvůli které prototyp čekal sedm sekund: práce
        // objednaná při tažení se musí dát zahodit dřív, než se udělá.
        let started = Arc::new(Mutex::new(Vec::new()));
        let seen = started.clone();
        let pool: Wishlist<u32, u32> = Wishlist::new(1, move |key| {
            seen.lock().unwrap().push(*key);
            std::thread::sleep(Duration::from_millis(30));
            Some(*key)
        });

        pool.wish(vec![vec![1]]);
        assert!(wait_for(|| !started.lock().unwrap().is_empty()));
        // Ještě než první doběhne, přepíšeme přání úplně jinam.
        pool.wish(vec![vec![99]]);

        assert!(wait_for(|| started.lock().unwrap().contains(&99)));
        let done = started.lock().unwrap().clone();
        assert!(
            !done.contains(&2),
            "zahozené přání se nesmí udělat: {done:?}"
        );
    }

    #[test]
    fn prednost_ma_prvni_skupina() {
        let order = Arc::new(Mutex::new(Vec::new()));
        let seen = order.clone();
        let pool: Wishlist<u32, u32> = Wishlist::new(1, move |key| {
            seen.lock().unwrap().push(*key);
            Some(*key)
        });
        pool.wish(vec![vec![10], vec![20, 21]]);

        assert!(wait_for(|| order.lock().unwrap().len() == 3));
        assert_eq!(order.lock().unwrap()[0], 10, "přednostní skupina jde první");
    }

    #[test]
    fn totez_se_nedela_dvakrat() {
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
        assert_eq!(count.load(Ordering::Relaxed), 2, "každý klíč právě jednou");
    }

    #[test]
    fn uloha_hlasi_prubeh_a_dobehne() {
        let tasks = Tasks::new();
        let id = tasks.spawn("sken", |_, progress| {
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
    fn selhani_ulohy_se_nikde_neztrati() {
        let tasks = Tasks::new();
        tasks.spawn("rozbitá", |_, _| anyhow::bail!("prasklo to"));
        assert!(wait_for(|| !tasks.failures().is_empty()));
        assert!(
            tasks.failures()[0]
                .error
                .as_ref()
                .unwrap()
                .contains("prasklo to")
        );
    }

    #[test]
    fn zruseni_uloha_pozna() {
        let tasks = Tasks::new();
        let id = tasks.spawn("dlouhá", |cancel, _| {
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
