use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ArtistInfo {
    pub name: String,
    pub description: String,
    pub photo: Option<String>,
    pub found: bool,
    pub source: String,

    #[serde(default)]
    pub ver: u32,
}

pub fn norm(name: &str) -> String {
    name.trim().to_lowercase()
}

fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Soundworks").join("artists")
}

fn id_of(name: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in norm(name).as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

pub fn load_cached(name: &str) -> Option<ArtistInfo> {
    let p = data_dir().join(format!("{}.json", id_of(name)));
    let info: ArtistInfo = serde_json::from_slice(&std::fs::read(p).ok()?).ok()?;

    if info.ver < 2 {
        return None;
    }
    Some(info)
}

fn save_cached(name: &str, info: &ArtistInfo) {
    let _ = std::fs::create_dir_all(data_dir());
    if let Ok(j) = serde_json::to_vec(info) {
        let _ = std::fs::write(data_dir().join(format!("{}.json", id_of(name))), j);
    }
}

pub struct Fetcher {
    queue: Arc<(Mutex<VecDeque<String>>, Condvar)>,
    pending: Arc<Mutex<HashSet<String>>>,
    rx: Receiver<(String, Option<ArtistInfo>)>,
    pub last_error: Arc<Mutex<Option<String>>>,
}

impl Fetcher {
    pub fn new() -> Fetcher {
        let queue: Arc<(Mutex<VecDeque<String>>, Condvar)> = Arc::new((Mutex::new(VecDeque::new()), Condvar::new()));
        let pending = Arc::new(Mutex::new(HashSet::new()));
        let last_error = Arc::new(Mutex::new(None));
        let (tx, rx) = channel();
        {
            let (queue, pending, last_error) = (queue.clone(), pending.clone(), last_error.clone());
            std::thread::spawn(move || worker_loop(queue, pending, last_error, tx));
        }
        Fetcher { queue, pending, rx, last_error }
    }

    pub fn is_pending(&self, name: &str) -> bool {
        self.pending.lock().unwrap().contains(&norm(name))
    }

    pub fn request(&self, name: &str, front: bool) {
        let k = norm(name);
        if !self.pending.lock().unwrap().insert(k.clone()) {
            if front {
                let (m, _) = &*self.queue;
                let mut q = m.lock().unwrap();
                if let Some(pos) = q.iter().position(|x| norm(x) == k) {
                    let item = q.remove(pos).unwrap();
                    q.push_front(item);
                }
            }
            return;
        }
        let (m, cv) = &*self.queue;
        let mut q = m.lock().unwrap();
        if front {
            q.push_front(name.to_string());
        } else {
            q.push_back(name.to_string());
        }
        cv.notify_one();
    }

    pub fn poll(&self) -> Vec<(String, Option<ArtistInfo>)> {
        let mut out = vec![];
        while let Ok(x) = self.rx.try_recv() {
            out.push(x);
        }
        out
    }
}

fn worker_loop(
    queue: Arc<(Mutex<VecDeque<String>>, Condvar)>,
    pending: Arc<Mutex<HashSet<String>>>,
    last_error: Arc<Mutex<Option<String>>>,
    tx: Sender<(String, Option<ArtistInfo>)>,
) {
    loop {
        let name = {
            let (m, cv) = &*queue;
            let mut q = m.lock().unwrap();
            while q.is_empty() {
                q = cv.wait(q).unwrap();
            }
            q.pop_front().unwrap()
        };
        let finish = |info: Option<ArtistInfo>| {
            pending.lock().unwrap().remove(&norm(&name));
            let _ = tx.send((name.clone(), info));
        };
        let id = id_of(&name);
        match crate::web::artist_lookup(&name, &id, &data_dir()) {
            Ok(info) => {
                *last_error.lock().unwrap() = None;
                save_cached(&name, &info);
                finish(Some(info));
            }
            Err(e) => {
                *last_error.lock().unwrap() = Some(e);
                finish(None);

                std::thread::sleep(Duration::from_millis(300));
            }
        }
    }
}

type GenreJob = (String, String, String);

pub struct GenreFetcher {
    queue: Arc<(Mutex<VecDeque<GenreJob>>, Condvar)>,
    pending: Arc<Mutex<HashSet<String>>>,
    rx: Receiver<(String, Option<String>)>,
}

impl GenreFetcher {
    pub fn new() -> GenreFetcher {
        let queue: Arc<(Mutex<VecDeque<GenreJob>>, Condvar)> = Arc::new((Mutex::new(VecDeque::new()), Condvar::new()));
        let pending = Arc::new(Mutex::new(HashSet::new()));
        let (tx, rx) = channel();
        {
            let (queue, pending) = (queue.clone(), pending.clone());
            std::thread::spawn(move || genre_loop(queue, pending, tx));
        }
        GenreFetcher { queue, pending, rx }
    }

    pub fn is_pending(&self, key: &str) -> bool {
        self.pending.lock().unwrap().contains(key)
    }

    pub fn queue_len(&self) -> usize {
        self.pending.lock().unwrap().len()
    }

    pub fn request(&self, key: String, artist: String, album: String) {
        if !self.pending.lock().unwrap().insert(key.clone()) {
            return;
        }
        let (m, cv) = &*self.queue;
        m.lock().unwrap().push_back((key, artist, album));
        cv.notify_one();
    }

    pub fn poll(&self) -> Vec<(String, Option<String>)> {
        let mut out = vec![];
        while let Ok(x) = self.rx.try_recv() {
            out.push(x);
        }
        out
    }
}

fn genre_loop(
    queue: Arc<(Mutex<VecDeque<GenreJob>>, Condvar)>,
    pending: Arc<Mutex<HashSet<String>>>,
    tx: Sender<(String, Option<String>)>,
) {
    loop {
        let (key, artist, album) = {
            let (m, cv) = &*queue;
            let mut q = m.lock().unwrap();
            while q.is_empty() {
                q = cv.wait(q).unwrap();
            }
            q.pop_front().unwrap()
        };
        let finish = |g: Option<String>| {
            pending.lock().unwrap().remove(&key);
            let _ = tx.send((key.clone(), g));
        };
        match crate::web::genre_lookup(&artist, &album) {
            Ok(g) => finish(Some(g)),
            Err(_) => finish(None),
        }
    }
}
