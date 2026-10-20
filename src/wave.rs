//! Parçanın gerçek dalga biçimi: dosyayı çözüp 50 ms'lik pencerelerden zarf çıkarır, 1200 değere indirger, diske önbellekler.

use std::collections::VecDeque;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Condvar, Mutex};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub const BUCKETS: usize = 1200;

fn dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Soundworks").join("waves")
}

fn cache_file(path: &Path) -> PathBuf {
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let mut h: u64 = 0xcbf29ce484222325;
    for b in path.to_string_lossy().to_lowercase().as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    dir().join(format!("{h:016x}_{size}.bin"))
}

fn load_cached(path: &Path) -> Option<Vec<f32>> {
    let b = std::fs::read(cache_file(path)).ok()?;
    (b.len() == BUCKETS).then(|| b.iter().map(|v| *v as f32 / 255.0).collect())
}

fn save_cached(path: &Path, v: &[f32]) {
    let _ = std::fs::create_dir_all(dir());
    let bytes: Vec<u8> = v.iter().map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8).collect();
    let _ = std::fs::write(cache_file(path), bytes);
}

/// 0..1 aralığında BUCKETS adet genlik değeri.
pub fn compute(path: &Path) -> Option<Vec<f32>> {
    if let Some(c) = load_cached(path) {
        return Some(c);
    }
    let env: Vec<f32> = if crate::dsd::is_dsd(path) {
        // DSD: PCM'e çevrilmiş akıştan zarf (352.8 kHz'de 50 ms pencereler)
        let (mut peak, mut sum_sq, mut cnt) = (0f32, 0f64, 0usize);
        let mut env: Vec<f32> = vec![];
        crate::player::decode_native(path, |chunk, rate| {
            let win = (rate as usize / 20).max(1);
            for f in chunk.chunks_exact(2) {
                let m = f[0].abs().max(f[1].abs());
                peak = peak.max(m);
                sum_sq += (m as f64) * (m as f64);
                cnt += 1;
                if cnt >= win {
                    env.push(0.55 * (sum_sq / cnt as f64).sqrt() as f32 + 0.45 * peak);
                    peak = 0.0;
                    sum_sq = 0.0;
                    cnt = 0;
                }
            }
        })
        .ok()?;
        if cnt > 0 {
            env.push(0.55 * (sum_sq / cnt as f64).sqrt() as f32 + 0.45 * peak);
        }
        env
    } else {
        let mss = MediaSourceStream::new(Box::new(File::open(path).ok()?), Default::default());
        let mut hint = Hint::new();
        if let Some(e) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(e);
        }
        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
            .ok()?;
        let mut format = probed.format;
        let t = format.tracks().iter().find(|t| t.codec_params.codec != CODEC_TYPE_NULL)?;
        let id = t.id;
        let rate = t.codec_params.sample_rate? as usize;
        let mut dec = symphonia::default::get_codecs().make(&t.codec_params, &DecoderOptions::default()).ok()?;

        let win = (rate / 20).max(1); // 50 ms
        let (mut peak, mut sum_sq, mut cnt) = (0f32, 0f64, 0usize);
        let mut env: Vec<f32> = vec![];
        while let Ok(p) = format.next_packet() {
            if p.track_id() != id {
                continue;
            }
            let Ok(d) = dec.decode(&p) else { continue };
            let spec = *d.spec();
            let mut sb = SampleBuffer::<f32>::new(d.capacity() as u64, spec);
            sb.copy_interleaved_ref(d);
            let ch = spec.channels.count().max(1);
            for f in sb.samples().chunks_exact(ch) {
                let m = f.iter().fold(0f32, |a, v| a.max(v.abs()));
                peak = peak.max(m);
                sum_sq += (m as f64) * (m as f64);
                cnt += 1;
                if cnt >= win {
                    let rms = (sum_sq / cnt as f64).sqrt() as f32;
                    env.push(0.55 * rms + 0.45 * peak);
                    peak = 0.0;
                    sum_sq = 0.0;
                    cnt = 0;
                }
            }
        }
        if cnt > 0 {
            let rms = (sum_sq / cnt as f64).sqrt() as f32;
            env.push(0.55 * rms + 0.45 * peak);
        }
        env
    };
    if env.len() < 4 {
        return None;
    }
    // BUCKETS değere indir (kova başına ortalama)
    let mut out = vec![0f32; BUCKETS];
    for (i, o) in out.iter_mut().enumerate() {
        let a = i * env.len() / BUCKETS;
        let b = (((i + 1) * env.len()) / BUCKETS).max(a + 1).min(env.len());
        let s = &env[a.min(env.len() - 1)..b];
        *o = s.iter().sum::<f32>() / s.len() as f32;
    }
    // normalize: %98'lik dilime göre, algısal eğri
    let mut sorted = out.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let ref_v = sorted[(sorted.len() as f32 * 0.98) as usize].max(1e-4);
    for v in out.iter_mut() {
        *v = (*v / ref_v).clamp(0.0, 1.0).powf(0.8);
    }
    save_cached(path, &out);
    Some(out)
}

pub struct WaveFetcher {
    queue: Arc<(Mutex<VecDeque<PathBuf>>, Condvar)>,
    pending: Arc<Mutex<std::collections::HashSet<String>>>,
    rx: Receiver<(String, Option<Vec<f32>>)>,
}

impl WaveFetcher {
    pub fn new() -> WaveFetcher {
        let queue: Arc<(Mutex<VecDeque<PathBuf>>, Condvar)> = Arc::new((Mutex::new(VecDeque::new()), Condvar::new()));
        let pending = Arc::new(Mutex::new(std::collections::HashSet::new()));
        let (tx, rx) = channel();
        {
            let (q, p) = (queue.clone(), pending.clone());
            std::thread::spawn(move || loop {
                let path = {
                    let (m, cv) = &*q;
                    let mut g = m.lock().unwrap();
                    while g.is_empty() {
                        g = cv.wait(g).unwrap();
                    }
                    g.pop_front().unwrap()
                };
                let res = compute(&path);
                let key = path.to_string_lossy().to_string();
                p.lock().unwrap().remove(&key);
                if tx.send((key, res)).is_err() {
                    return;
                }
            });
        }
        WaveFetcher { queue, pending, rx }
    }

    pub fn is_pending(&self, key: &str) -> bool {
        self.pending.lock().unwrap().contains(key)
    }

    pub fn request(&self, path: &str) {
        if !self.pending.lock().unwrap().insert(path.to_string()) {
            return;
        }
        let (m, cv) = &*self.queue;
        m.lock().unwrap().push_front(PathBuf::from(path));
        cv.notify_one();
    }

    pub fn poll(&self) -> Vec<(String, Option<Vec<f32>>)> {
        let mut out = vec![];
        while let Ok(x) = self.rx.try_recv() {
            out.push(x);
        }
        out
    }
}
