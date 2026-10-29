use crate::library::OnlineMeta;
use crate::tagwrite::{self, WriteLogEntry};
use crate::web::{self, TrackHint};
use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

type Job = (String, TrackHint);
type Queue = Arc<(Mutex<VecDeque<Job>>, Condvar)>;

pub type LookupResult = (String, Result<Option<OnlineMeta>, String>);

pub struct MetaFetcher {
    queue: Queue,
    pending: Arc<Mutex<HashSet<String>>>,
    rx: Receiver<LookupResult>,
}

impl MetaFetcher {
    pub fn new() -> MetaFetcher {
        let queue: Queue = Arc::new((Mutex::new(VecDeque::new()), Condvar::new()));
        let pending = Arc::new(Mutex::new(HashSet::new()));
        let (tx, rx) = channel();
        {
            let (queue, pending) = (queue.clone(), pending.clone());
            std::thread::spawn(move || lookup_loop(queue, pending, tx));
        }
        MetaFetcher { queue, pending, rx }
    }

    pub fn is_pending(&self, path: &str) -> bool {
        self.pending.lock().unwrap().contains(path)
    }

    pub fn queue_len(&self) -> usize {
        self.pending.lock().unwrap().len()
    }

    pub fn request(&self, path: String, hint: TrackHint) {
        if !self.pending.lock().unwrap().insert(path.clone()) {
            return;
        }
        let (m, cv) = &*self.queue;
        m.lock().unwrap().push_back((path, hint));
        cv.notify_one();
    }

    pub fn cancel_all(&self) {
        let (m, _) = &*self.queue;
        let mut q = m.lock().unwrap();
        let mut pending = self.pending.lock().unwrap();
        for (path, _) in q.drain(..) {
            pending.remove(&path);
        }
    }

    pub fn poll(&self) -> Vec<LookupResult> {
        let mut out = vec![];
        while let Ok(x) = self.rx.try_recv() {
            out.push(x);
        }
        out
    }
}

fn lookup_loop(queue: Queue, pending: Arc<Mutex<HashSet<String>>>, tx: Sender<LookupResult>) {
    loop {
        let (path, hint) = {
            let (m, cv) = &*queue;
            let mut q = m.lock().unwrap();
            while q.is_empty() {
                q = cv.wait(q).unwrap();
            }
            q.pop_front().unwrap()
        };
        let res = web::track_lookup(&hint);
        pending.lock().unwrap().remove(&path);
        let _ = tx.send((path, res));
        std::thread::sleep(Duration::from_millis(150));
    }
}

pub struct WriteReport {
    pub entries: Vec<WriteLogEntry>,
    pub failed: usize,
    pub undone: bool,
}

fn log_path() -> std::path::PathBuf {
    crate::library::data_dir().join("meta_write_log.json")
}

pub fn load_log() -> Vec<WriteLogEntry> {
    crate::fsutil::read_json(&log_path()).unwrap_or_default()
}

pub fn save_log(entries: &[WriteLogEntry]) {
    if let Ok(j) = serde_json::to_vec(entries) {
        let _ = crate::fsutil::write_atomic(&log_path(), &j);
    }
}

pub fn spawn_write(jobs: Vec<(String, OnlineMeta)>) -> Receiver<WriteReport> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let (mut entries, mut failed) = (vec![], 0);
        for (path, meta) in jobs {
            if !tagwrite::can_write(Path::new(&path)) {
                continue;
            }
            match tagwrite::write_missing(Path::new(&path), &meta) {
                Ok(fields) if !fields.is_empty() => entries.push(WriteLogEntry { path, fields }),
                Ok(_) => {}
                Err(_) => failed += 1,
            }
        }
        let _ = tx.send(WriteReport { entries, failed, undone: false });
    });
    rx
}

pub fn spawn_undo(entries: Vec<WriteLogEntry>) -> Receiver<WriteReport> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let (mut reverted, mut failed) = (vec![], 0);
        for e in entries {
            match tagwrite::undo(&e) {
                Ok(n) if n > 0 => reverted.push(e),
                Ok(_) => {}
                Err(_) => failed += 1,
            }
        }
        let _ = tx.send(WriteReport { entries: reverted, failed, undone: true });
    });
    rx
}
