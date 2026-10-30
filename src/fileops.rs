use crate::tagwrite;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

pub struct EditJob {
    pub path: String,
    pub edits: Vec<(&'static str, String)>,
}

pub struct EditReport {
    /// (path, album key before, album key after)
    pub done: Vec<(String, String, String)>,
    pub failed: Vec<(String, String)>,
}

pub struct DeleteReport {
    pub failed: usize,
}

fn key_of(path: &Path) -> String {
    let get = |f: &str| tagwrite::read_fields(path).ok().and_then(|v| v.into_iter().find(|(k, _)| *k == f).map(|(_, v)| v)).unwrap_or_default();
    let (album, artist, album_artist) = (get("album"), get("artist"), get("album_artist"));
    let artist = if album_artist.is_empty() { artist } else { album_artist };
    crate::library::make_key(&album, &artist, path).0
}

pub fn spawn_edit(jobs: Vec<(EditJob, String)>) -> Receiver<EditReport> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let (mut done, mut failed) = (vec![], vec![]);
        for (job, old_key) in jobs {
            let path = Path::new(&job.path);
            match tagwrite::write_fields(path, &job.edits) {
                Ok(()) => {
                    let new_key = key_of(path);
                    done.push((job.path, old_key, new_key));
                }
                Err(e) => failed.push((job.path, e)),
            }
        }
        let _ = tx.send(EditReport { done, failed });
    });
    rx
}

/// Moves the files to the recycle bin. A file that was just playing may stay locked for a moment, so each is retried.
pub fn spawn_recycle(paths: Vec<String>) -> Receiver<DeleteReport> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut failed = 0;
        for p in paths {
            let path = PathBuf::from(&p);
            if !path.exists() {
                continue;
            }
            let mut ok = false;
            for _ in 0..6 {
                if crate::gui::recycle(&path).is_ok() {
                    ok = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(400));
            }
            if !ok {
                failed += 1;
            }
        }
        let _ = tx.send(DeleteReport { failed });
    });
    rx
}
