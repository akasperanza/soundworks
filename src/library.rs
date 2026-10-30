use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use symphonia::core::codecs::CODEC_TYPE_NULL;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, MetadataRevision, StandardTagKey};
use symphonia::core::probe::Hint;

use crate::player::AUDIO_EXTS;

pub const UNKNOWN_ARTIST: &str = "Bilinmeyen sanatçı";

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct OnlineMeta {
    pub title: String,
    pub artist: String,
    pub album: String,
    #[serde(default)]
    pub album_artist: String,
    #[serde(default)]
    pub year: u32,
    #[serde(default)]
    pub track_no: u32,
    #[serde(default)]
    pub source: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LibTrack {
    pub path: String,
    pub title: String,
    pub track_no: u32,
    pub disc_no: u32,
    pub duration: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Album {
    pub key: String,
    pub title: String,
    pub artist: String,
    pub tracks: Vec<LibTrack>,

    #[serde(default)]
    pub genre: String,
    #[serde(default)]
    pub genre_checked: bool,
}

impl Album {
    pub fn total_secs(&self) -> f32 {
        self.tracks.iter().map(|t| t.duration).sum()
    }
    pub fn paths(&self) -> Vec<PathBuf> {
        self.tracks.iter().map(|t| PathBuf::from(&t.path)).collect()
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct Library {
    pub albums: Vec<Album>,

    #[serde(default)]
    pub folders: Vec<String>,

    #[serde(default)]
    pub online: std::collections::BTreeMap<String, OnlineMeta>,

    #[serde(default)]
    pub online_tried: std::collections::BTreeMap<String, u64>,

    #[serde(default)]
    pub cover_tried: std::collections::BTreeMap<String, u64>,

    #[serde(default)]
    pub ignored: std::collections::BTreeSet<String>,
}

#[derive(Clone, Debug)]
pub struct TrackInfo {
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub key: String,
    pub track_no: u32,
    pub disc_no: u32,
    pub duration: f32,
}

pub enum ScanMsg {
    Track(TrackInfo),

    Progress(usize, usize),
    Done,
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Soundworks")
}

fn lib_path() -> PathBuf {
    data_dir().join("library.json")
}

pub fn cover_path(key: &str) -> PathBuf {

    let mut h: u64 = 0xcbf29ce484222325;
    for b in key.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    data_dir().join("covers").join(format!("{h:016x}.png"))
}

impl Library {
    pub fn load() -> Library {
        crate::fsutil::read_json(&lib_path()).unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(j) = serde_json::to_vec(self) {
            let _ = crate::fsutil::write_atomic(&lib_path(), &j);
        }
    }

    pub fn total_tracks(&self) -> usize {
        self.albums.iter().map(|a| a.tracks.len()).sum()
    }

    pub fn add(&mut self, t: &TrackInfo) -> bool {
        let patched = self.patched(t);
        let t = &patched;
        let idx = match self.albums.iter().position(|a| a.key == t.key) {
            Some(i) => i,
            None => {
                self.albums.push(Album { key: t.key.clone(), title: t.album.clone(), artist: t.artist.clone(), tracks: vec![], genre: String::new(), genre_checked: false });
                self.albums.len() - 1
            }
        };
        let album = &mut self.albums[idx];
        if album.tracks.iter().any(|x| x.path == t.path) {
            return false;
        }
        album.tracks.push(LibTrack {
            path: t.path.clone(),
            title: t.title.clone(),
            track_no: t.track_no,
            disc_no: t.disc_no,
            duration: t.duration,
        });
        album.tracks.sort_by(|a, b| {
            (a.disc_no, a.track_no, a.path.to_lowercase()).cmp(&(b.disc_no, b.track_no, b.path.to_lowercase()))
        });
        true
    }

    fn patched(&self, t: &TrackInfo) -> TrackInfo {
        let Some(m) = self.online.get(&t.path) else { return t.clone() };
        let mut out = t.clone();
        let mut regroup = false;
        let stem = Path::new(&t.path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if out.title == stem && !m.title.is_empty() {
            out.title = m.title.clone();
        }
        if out.artist == UNKNOWN_ARTIST && !m.artist.is_empty() {
            out.artist = if m.album_artist.is_empty() { m.artist.clone() } else { m.album_artist.clone() };
            regroup = true;
        }
        if out.key.starts_with("dir|") && !m.album.is_empty() {
            out.album = m.album.clone();
            regroup = true;
        }
        if out.track_no == 0 {
            out.track_no = m.track_no;
        }
        if regroup {
            let artist = if out.artist == UNKNOWN_ARTIST { String::new() } else { out.artist.clone() };
            let (key, title) = make_key(&out.album, &artist, Path::new(&t.path));
            out.key = key;
            if !out.key.starts_with("dir|") {
                out.album = title;
            }
        }
        out
    }

    fn wants_online(&self, a: &Album, t: &LibTrack, all: bool) -> bool {
        if self.online.contains_key(&t.path) || self.online_tried.contains_key(&t.path) {
            return false;
        }
        all || a.key.starts_with("dir|")
            || a.artist == UNKNOWN_ARTIST
            || Path::new(&t.path).file_stem().map_or(false, |s| s.to_string_lossy() == t.title.as_str())
    }

    pub fn cover_jobs(&self, limit: usize, skip: impl Fn(&str) -> bool) -> Vec<(String, String, String)> {
        let mut out = vec![];
        for a in &self.albums {
            if out.len() >= limit {
                break;
            }
            if a.artist == UNKNOWN_ARTIST || a.title.trim().is_empty() || self.cover_tried.contains_key(&a.key) || skip(&a.key) || cover_path(&a.key).exists() {
                continue;
            }
            out.push((a.key.clone(), a.artist.clone(), a.title.clone()));
        }
        out
    }

    pub fn online_progress(&self, all: bool) -> (usize, usize) {
        let (mut done, mut total) = (0, 0);
        for a in &self.albums {
            for t in &a.tracks {
                let finished = self.online.contains_key(&t.path) || self.online_tried.contains_key(&t.path);
                if finished {
                    done += 1;
                    total += 1;
                } else if self.wants_online(a, t, all) {
                    total += 1;
                }
            }
        }
        (done, total)
    }

    pub fn online_jobs(&self, all: bool, limit: usize, skip: impl Fn(&str) -> bool) -> Vec<(String, crate::web::TrackHint)> {
        let mut out = vec![];
        for a in &self.albums {
            for t in &a.tracks {
                if out.len() >= limit {
                    return out;
                }
                if !self.wants_online(a, t, all) || skip(&t.path) {
                    continue;
                }
                let folder = Path::new(&t.path).parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let hint = crate::web::TrackHint {
                    title: t.title.clone(),
                    artist: if a.artist == UNKNOWN_ARTIST { String::new() } else { a.artist.clone() },
                    duration: t.duration as f64,
                    folder,
                };
                out.push((t.path.clone(), hint));
            }
        }
        out
    }

    pub fn apply_online(&mut self, path: &str) {
        let Some((ai, ti)) = self.albums.iter().enumerate().find_map(|(ai, a)| a.tracks.iter().position(|t| t.path == path).map(|ti| (ai, ti))) else {
            return;
        };
        let album = &mut self.albums[ai];
        let tr = album.tracks.remove(ti);
        let info = TrackInfo {
            path: tr.path,
            title: tr.title,
            artist: album.artist.clone(),
            album: album.title.clone(),
            key: album.key.clone(),
            track_no: tr.track_no,
            disc_no: tr.disc_no,
            duration: tr.duration,
        };
        if album.tracks.is_empty() {
            self.albums.remove(ai);
        }
        self.add(&info);
        self.sort();
    }

    pub fn sort(&mut self) {
        self.albums.sort_by(|a, b| {
            (a.artist.to_lowercase(), a.title.to_lowercase()).cmp(&(b.artist.to_lowercase(), b.title.to_lowercase()))
        });
    }

    pub fn remove_album(&mut self, key: &str) {
        self.albums.retain(|a| a.key != key);
    }

    pub fn artists(&self) -> Vec<(String, usize, usize)> {
        let mut map: std::collections::BTreeMap<String, (String, usize, usize)> = std::collections::BTreeMap::new();
        for a in &self.albums {
            let e = map.entry(a.artist.trim().to_lowercase()).or_insert((a.artist.clone(), 0, 0));
            e.1 += 1;
            e.2 += a.tracks.len();
        }
        map.into_values().collect()
    }

    pub fn add_folder(&mut self, dir: &str) -> bool {
        let norm = |s: &str| s.trim_end_matches(['\\', '/']).to_lowercase().replace('/', "\\");
        let n = norm(dir);
        let inside = |child: &str, parent: &str| child == parent || child.starts_with(&format!("{parent}\\"));
        if self.folders.iter().any(|f| inside(&n, &norm(f))) {
            return false;
        }
        self.folders.retain(|f| !inside(&norm(f), &n));
        self.folders.push(dir.trim_end_matches(['\\', '/']).to_string());
        true
    }

    pub fn remove_folder(&mut self, dir: &str) {
        self.folders.retain(|f| f != dir);
    }

    pub fn remove_missing(&mut self, missing: &HashSet<String>) -> usize {
        let before = self.total_tracks();
        for a in self.albums.iter_mut() {
            a.tracks.retain(|t| !missing.contains(&t.path));
        }
        self.albums.retain(|a| !a.tracks.is_empty());
        before - self.total_tracks()
    }

    pub fn all_paths(&self) -> Vec<String> {
        self.albums.iter().flat_map(|a| a.tracks.iter().map(|t| t.path.clone())).collect()
    }

    /// Paths a folder scan must not add again: everything in the library plus tracks the user removed.
    pub fn known_paths(&self) -> HashSet<String> {
        self.all_paths().into_iter().chain(self.ignored.iter().cloned()).collect()
    }

    pub fn forget_tracks(&mut self, paths: &HashSet<String>, keep_out_of_scans: bool) -> usize {
        let n = self.remove_missing(paths);
        for p in paths {
            self.online.remove(p);
            self.online_tried.remove(p);
            if keep_out_of_scans {
                self.ignored.insert(p.clone());
            }
        }
        n
    }
}

pub fn spawn_missing_check(paths: Vec<String>) -> Receiver<HashSet<String>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let missing: HashSet<String> = paths.into_iter().filter(|p| !Path::new(p).exists()).collect();
        let _ = tx.send(missing);
    });
    rx
}

pub fn collect_audio(dir: &Path, out: &mut Vec<PathBuf>, depth: u32) {
    if depth > 8 {
        return;
    }
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_audio(&p, out, depth + 1);
        } else if p
            .extension()
            .and_then(|x| x.to_str())
            .map(|x| AUDIO_EXTS.contains(&x.to_lowercase().as_str()))
            .unwrap_or(false)
        {
            out.push(p);
        }
    }
}

#[derive(Default)]
struct Raw {
    title: String,
    artist: String,
    album_artist: String,
    album: String,
    track_no: u32,
    disc_no: u32,
    cover: Option<Vec<u8>>,
}

fn leading_num(s: &str) -> u32 {
    s.trim().split(|c: char| !c.is_ascii_digit()).next().and_then(|x| x.parse().ok()).unwrap_or(0)
}

fn read_rev(r: &MetadataRevision, raw: &mut Raw) {
    for tag in r.tags() {
        let v = tag.value.to_string();
        match tag.std_key {
            Some(StandardTagKey::TrackTitle) if raw.title.is_empty() => raw.title = v,
            Some(StandardTagKey::Artist) if raw.artist.is_empty() => raw.artist = v,
            Some(StandardTagKey::AlbumArtist) if raw.album_artist.is_empty() => raw.album_artist = v,
            Some(StandardTagKey::Album) if raw.album.is_empty() => raw.album = v,
            Some(StandardTagKey::TrackNumber) if raw.track_no == 0 => raw.track_no = leading_num(&v),
            Some(StandardTagKey::DiscNumber) if raw.disc_no == 0 => raw.disc_no = leading_num(&v),
            _ => {}
        }
    }
    if raw.cover.is_none() {
        if let Some(vis) = r.visuals().first() {
            raw.cover = Some(vis.data.to_vec());
        }
    }
}

pub(crate) fn make_key(album: &str, artist: &str, path: &Path) -> (String, String) {
    if album.trim().is_empty() {
        let dir = path.parent().map(|p| p.to_string_lossy().to_lowercase()).unwrap_or_default();
        let name = path
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Bilinmeyen albüm".into());
        (format!("dir|{dir}"), name)
    } else {
        (format!("{}|{}", album.trim().to_lowercase(), artist.trim().to_lowercase()), album.trim().to_string())
    }
}

fn scan_file(path: &Path, covered: &mut HashSet<String>) -> Option<TrackInfo> {
    let (raw, duration) = if crate::dsd::is_dsd(path) {
        let info = crate::dsd::probe(path).ok()?;
        let t = crate::dsd::read_tags(path, &info);
        let raw = Raw {
            title: t.title,
            artist: t.artist,
            album_artist: t.album_artist,
            album: t.album,
            track_no: t.track_no,
            disc_no: t.disc_no,
            cover: t.cover,
        };
        (raw, info.secs() as f32)
    } else {
        let file = File::open(path).ok()?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let mut probed = symphonia::default::get_probe()
            .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
            .ok()?;
        let mut raw = Raw::default();
        if let Some(m) = probed.metadata.get() {
            if let Some(r) = m.current() {
                read_rev(r, &mut raw);
            }
        }
        let mut format = probed.format;
        if let Some(r) = format.metadata().current() {
            read_rev(r, &mut raw);
        }
        let duration = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .and_then(|t| match (t.codec_params.n_frames, t.codec_params.sample_rate) {
                (Some(n), Some(sr)) if sr > 0 => Some(n as f32 / sr as f32),
                _ => None,
            })
            .unwrap_or(0.0);

        (raw, duration)
    };

    let artist = if !raw.album_artist.is_empty() { raw.album_artist.clone() } else { raw.artist.clone() };
    let (key, album_title) = make_key(&raw.album, &artist, path);
    let artist_disp = if artist.is_empty() { UNKNOWN_ARTIST.to_string() } else { artist };
    let title = if raw.title.is_empty() {
        path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
    } else {
        raw.title.clone()
    };

    if let Some(bytes) = &raw.cover {
        let cp = cover_path(&key);
        if !cp.exists() && !covered.contains(&key) {
            if let Ok(img) = image::load_from_memory(bytes) {
                let _ = std::fs::create_dir_all(cp.parent().unwrap());
                let _ = img.thumbnail(360, 360).save_with_format(&cp, image::ImageFormat::Png);
            }
            covered.insert(key.clone());
        }
    }

    Some(TrackInfo {
        path: path.to_string_lossy().to_string(),
        title,
        artist: artist_disp,
        album: album_title,
        key,
        track_no: raw.track_no,
        disc_no: raw.disc_no,
        duration,
    })
}

fn scan_loop(paths: Vec<PathBuf>, known: &HashSet<String>, tx: &std::sync::mpsc::Sender<ScanMsg>) {
    let total = paths.len();
    let mut covered = HashSet::new();
    for (i, p) in paths.iter().enumerate() {
        let ps = p.to_string_lossy().to_string();
        if !known.contains(&ps) {
            if let Some(info) = scan_file(p, &mut covered) {
                if tx.send(ScanMsg::Track(info)).is_err() {
                    return;
                }
            }
        }
        if i % 4 == 0 || i + 1 == total {
            let _ = tx.send(ScanMsg::Progress(i + 1, total));
        }
    }
    let _ = tx.send(ScanMsg::Done);
}

pub fn spawn_scan(paths: Vec<PathBuf>, known: HashSet<String>) -> Receiver<ScanMsg> {
    let (tx, rx) = channel();
    std::thread::spawn(move || scan_loop(paths, &known, &tx));
    rx
}

pub fn spawn_folder_scan(folders: Vec<String>, known: HashSet<String>) -> Receiver<ScanMsg> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut files = vec![];
        for f in &folders {
            collect_audio(Path::new(f), &mut files, 0);
        }
        files.sort();
        files.dedup();
        scan_loop(files, &known, &tx);
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(path: &str, title: &str, n: u32) -> TrackInfo {
        TrackInfo {
            path: path.into(),
            title: title.into(),
            artist: "Sanatçı".into(),
            album: "Albüm".into(),
            key: "albüm|sanatçı".into(),
            track_no: n,
            disc_no: 1,
            duration: 100.0,
        }
    }

    #[test]
    fn online_overlay_fills_missing_fields_and_regroups() {
        let mut lib = Library::default();
        let raw = |p: &str, stem: &str| TrackInfo {
            path: p.into(),
            title: stem.into(),
            artist: UNKNOWN_ARTIST.into(),
            album: "Klasör".into(),
            key: "dir|c:/m/klasör".into(),
            track_no: 0,
            disc_no: 0,
            duration: 100.0,
        };
        lib.add(&raw("C:/m/Klasör/a.mp3", "a"));
        lib.add(&raw("C:/m/Klasör/b.mp3", "b"));
        assert_eq!(lib.albums.len(), 1);
        assert_eq!(lib.online_jobs(false, 10, |_| false).len(), 2);
        let meta = OnlineMeta { title: "Gerçek Ad".into(), artist: "Sanatçı".into(), album: "Gerçek Albüm".into(), album_artist: String::new(), year: 2001, track_no: 4, source: "t".into() };
        lib.online.insert("C:/m/Klasör/a.mp3".into(), meta);
        lib.apply_online("C:/m/Klasör/a.mp3");
        assert_eq!(lib.albums.len(), 2);
        let real = lib.albums.iter().find(|a| a.title == "Gerçek Albüm").unwrap();
        assert_eq!((real.artist.as_str(), real.tracks[0].title.as_str(), real.tracks[0].track_no), ("Sanatçı", "Gerçek Ad", 4));
        assert_eq!(lib.albums.iter().find(|a| a.key.starts_with("dir|")).unwrap().tracks.len(), 1);
        assert!(lib.online_jobs(true, 10, |_| false).iter().all(|(p, _)| p != "C:/m/Klasör/a.mp3"));
        assert_eq!(lib.online_progress(false), (1, 2));
        let mut again = Library { online: lib.online.clone(), ..Default::default() };
        again.add(&raw("C:/m/Klasör/a.mp3", "a"));
        assert_eq!(again.albums[0].title, "Gerçek Albüm");
    }

    #[test]
    fn sixth_track_joins_same_album() {
        let mut lib = Library::default();
        for n in 1..=5 {
            assert!(lib.add(&info(&format!("a{n}.flac"), "x", n)));
        }
        assert_eq!(lib.albums.len(), 1);

        assert!(lib.add(&info("a6.flac", "x", 6)));
        assert_eq!(lib.albums.len(), 1);
        assert_eq!(lib.albums[0].tracks.len(), 6);

        assert!(!lib.add(&info("a3.flac", "x", 3)));
        let nums: Vec<u32> = lib.albums[0].tracks.iter().map(|t| t.track_no).collect();
        assert_eq!(nums, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn watched_folders_are_deduped_and_nested() {
        let mut lib = Library::default();
        assert!(lib.add_folder("D:\\Müzik\\Caz"));
        assert!(!lib.add_folder("d:\\müzik\\caz\\"));
        assert!(!lib.add_folder("D:\\Müzik\\Caz\\Miles"));
        assert!(lib.add_folder("D:\\Müzik\\Rock"));
        assert!(lib.add_folder("D:\\Müzik"));
        assert_eq!(lib.folders, vec!["D:\\Müzik".to_string()]);
        assert!(lib.add_folder("D:\\Müzik2"));
        assert_eq!(lib.folders.len(), 2);
        lib.remove_folder("D:\\Müzik2");
        assert_eq!(lib.folders.len(), 1);
    }

    #[test]
    fn missing_tracks_and_empty_albums_are_removed() {
        let mut lib = Library::default();
        lib.add(&info("a1.flac", "x", 1));
        lib.add(&info("a2.flac", "x", 2));
        let mut other = info("b1.flac", "y", 1);
        other.key = "diger|sanatci".into();
        lib.add(&other);
        let missing: HashSet<String> = ["a2.flac", "b1.flac"].iter().map(|s| s.to_string()).collect();
        assert_eq!(lib.remove_missing(&missing), 2);
        assert_eq!(lib.albums.len(), 1);
        assert_eq!(lib.albums[0].tracks.len(), 1);
        assert_eq!(lib.all_paths(), vec!["a1.flac".to_string()]);
    }

    #[test]
    fn removed_tracks_stay_out_of_later_scans() {
        let mut lib = Library::default();
        lib.add(&info("a1.flac", "x", 1));
        lib.add(&info("a2.flac", "x", 2));
        let gone: HashSet<String> = ["a2.flac".to_string()].into_iter().collect();
        assert_eq!(lib.forget_tracks(&gone, true), 1);
        assert!(lib.known_paths().contains("a2.flac") && lib.known_paths().contains("a1.flac"));
        assert_eq!(lib.all_paths(), vec!["a1.flac".to_string()]);
        let back: HashSet<String> = ["a1.flac".to_string()].into_iter().collect();
        lib.forget_tracks(&back, false);
        assert!(!lib.known_paths().contains("a1.flac"));
        assert!(lib.albums.is_empty());
    }

    #[test]
    fn missing_check_and_folder_scan_use_the_disk() {
        let dir = std::env::temp_dir().join("sw_lib_scan_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("alt")).unwrap();
        let here = dir.join("var.wav");
        std::fs::write(&here, b"x").unwrap();
        std::fs::write(dir.join("alt").join("b.mp3"), b"x").unwrap();
        std::fs::write(dir.join("not.txt"), b"x").unwrap();
        let gone = dir.join("yok.flac").to_string_lossy().to_string();
        let m = spawn_missing_check(vec![here.to_string_lossy().to_string(), gone.clone()]).recv().unwrap();
        assert_eq!(m, [gone].into_iter().collect());

        let rx = spawn_folder_scan(vec![dir.to_string_lossy().to_string()], HashSet::new());
        let mut total = 0;
        loop {
            match rx.recv().unwrap() {
                ScanMsg::Progress(_, t) => total = t,
                ScanMsg::Done => break,
                ScanMsg::Track(_) => {}
            }
        }
        assert_eq!(total, 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn leading_number_parses() {
        assert_eq!(leading_num("3/12"), 3);
        assert_eq!(leading_num(" 07 "), 7);
        assert_eq!(leading_num("x"), 0);
    }
}
