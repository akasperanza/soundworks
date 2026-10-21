use crate::artist::norm;
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct LyricWord {
    pub t: u32,
    #[serde(default)]
    pub d: u32,
    pub text: String,

    #[serde(default = "yes")]
    pub sp: bool,

    #[serde(default)]
    pub bg: bool,
}

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct LyricLine {
    pub t: u32,
    pub text: String,
    #[serde(default)]
    pub words: Vec<LyricWord>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Lyrics {

    pub kind: String,
    pub provider: String,
    pub lines: Vec<LyricLine>,

    #[serde(default)]
    pub ver: u32,
}

impl Lyrics {
    pub fn found(&self) -> bool {
        !self.lines.is_empty()
    }
}

pub fn key(artist: &str, title: &str) -> String {
    format!("{}|{}", norm(artist), norm(title))
}

fn dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Soundworks").join("lyrics")
}

fn file_of(k: &str) -> PathBuf {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in k.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    dir().join(format!("{h:016x}.json"))
}

pub fn load_cached(k: &str) -> Option<Lyrics> {
    let l: Lyrics = serde_json::from_slice(&std::fs::read(file_of(k)).ok()?).ok()?;
    if l.ver < 4 {
        return None;
    }
    Some(l)
}

fn save_cached(k: &str, l: &Lyrics) {
    let _ = std::fs::create_dir_all(dir());
    if let Ok(j) = serde_json::to_vec(l) {
        let _ = std::fs::write(file_of(k), j);
    }
}

pub fn forget(k: &str) {
    let _ = std::fs::remove_file(file_of(k));
}

type Job = (String, String, String, String, f64);

pub struct LyricsFetcher {
    queue: Arc<(Mutex<VecDeque<Job>>, Condvar)>,
    pending: Arc<Mutex<HashSet<String>>>,
    rx: Receiver<(String, Option<Lyrics>)>,
    pub last_error: Arc<Mutex<Option<String>>>,
}

impl LyricsFetcher {
    pub fn new() -> LyricsFetcher {
        let queue: Arc<(Mutex<VecDeque<Job>>, Condvar)> = Arc::new((Mutex::new(VecDeque::new()), Condvar::new()));
        let pending = Arc::new(Mutex::new(HashSet::new()));
        let last_error = Arc::new(Mutex::new(None));
        let (tx, rx) = channel();
        {
            let (q, p, e) = (queue.clone(), pending.clone(), last_error.clone());
            std::thread::spawn(move || worker(q, p, e, tx));
        }
        LyricsFetcher { queue, pending, rx, last_error }
    }

    pub fn is_pending(&self, k: &str) -> bool {
        self.pending.lock().unwrap().contains(k)
    }

    pub fn request(&self, k: String, title: String, artist: String, album: String, duration: f64) {
        if !self.pending.lock().unwrap().insert(k.clone()) {
            return;
        }
        let (m, cv) = &*self.queue;

        m.lock().unwrap().push_front((k, title, artist, album, duration));
        cv.notify_one();
    }

    pub fn poll(&self) -> Vec<(String, Option<Lyrics>)> {
        let mut out = vec![];
        while let Ok(x) = self.rx.try_recv() {
            out.push(x);
        }
        out
    }
}

fn worker(
    queue: Arc<(Mutex<VecDeque<Job>>, Condvar)>,
    pending: Arc<Mutex<HashSet<String>>>,
    last_error: Arc<Mutex<Option<String>>>,
    tx: Sender<(String, Option<Lyrics>)>,
) {
    loop {
        let (k, title, artist, album, duration) = {
            let (m, cv) = &*queue;
            let mut q = m.lock().unwrap();
            while q.is_empty() {
                q = cv.wait(q).unwrap();
            }
            q.pop_front().unwrap()
        };
        let finish = |l: Option<Lyrics>| {
            pending.lock().unwrap().remove(&k);
            let _ = tx.send((k.clone(), l));
        };
        let l = crate::web::lyrics_lookup(&title, &artist, &album, duration);

        if l.found() {
            save_cached(&k, &l);
        }
        *last_error.lock().unwrap() = None;
        finish(Some(l));
    }
}
