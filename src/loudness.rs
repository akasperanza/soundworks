//! Ses seviyesi analizi (ITU-R BS.1770 / EBU R128): etiketi olmayan parçalar için ReplayGain değeri üretir.
//! Sonuçlar `%APPDATA%\Soundworks\loudness.json` önbelleğinde tutulur (dosya boyutu + değişme zamanıyla doğrulanır).

use crate::player::Rg;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// ReplayGain 2.0 referans seviyesi (LUFS).
pub const RG_REFERENCE_LUFS: f32 = -18.0;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Analysis {
    /// bütünleşik (kapılı) ses şiddeti, LUFS
    pub lufs: f32,
    /// örnek tepesi (doğrusal)
    pub peak: f32,
    /// kapılı ortalama güç (doğrusal, K-ağırlıklı)
    pub power: f64,
    pub secs: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Entry {
    size: u64,
    mtime: u64,
    a: Analysis,
    #[serde(default)]
    album_gain: Option<f32>,
    #[serde(default)]
    album_peak: Option<f32>,
}

// ------------------------------------------------------------------ K-ağırlıklı süzgeç

#[derive(Clone, Copy)]
struct Bq {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Bq {
    fn step(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// BS.1770 K-ağırlıklandırma (yüksek raf + 38 Hz yüksek geçiren), herhangi bir örnekleme hızı için.
fn k_weighting(fs: f64) -> [Bq; 2] {
    use std::f64::consts::PI;
    // aşama 1: yüksek raf
    let (f0, g, q) = (1681.974450955533, 3.999843853973347, 0.7071752369554196);
    let k = (PI * f0 / fs).tan();
    let vh = 10f64.powf(g / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + k / q + k * k;
    let s1 = Bq {
        b: [(vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0],
        a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        z: [0.0; 2],
    };
    // aşama 2: RLB yüksek geçiren
    let (f0, q) = (38.13547087602444, 0.5003270373238773);
    let k = (PI * f0 / fs).tan();
    let a0 = 1.0 + k / q + k * k;
    let s2 = Bq { b: [1.0, -2.0, 1.0], a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0], z: [0.0; 2] };
    [s1, s2]
}

/// Parça boyunca akıtılarak ölçüm yapan analizci (stereo interleaved f32).
pub struct Meter {
    fs: f64,
    filt: [[Bq; 2]; 2], // [kanal][aşama]
    step_len: usize,
    step_acc: f64,
    step_n: usize,
    steps: Vec<f64>, // her 100 ms'lik adımın güç toplamı
    peak: f32,
    frames: u64,
}

impl Meter {
    pub fn new(rate: u32) -> Meter {
        let fs = rate as f64;
        let f = k_weighting(fs);
        Meter { fs, filt: [f, f], step_len: ((fs * 0.1).round() as usize).max(1), step_acc: 0.0, step_n: 0, steps: vec![], peak: 0.0, frames: 0 }
    }

    pub fn feed(&mut self, interleaved: &[f32]) {
        for fr in interleaved.chunks_exact(2) {
            let mut p = 0.0;
            for ch in 0..2 {
                let x = fr[ch];
                self.peak = self.peak.max(x.abs());
                let y1 = self.filt[ch][0].step(x as f64);
                let y = self.filt[ch][1].step(y1);
                p += y * y;
            }
            self.step_acc += p;
            self.step_n += 1;
            self.frames += 1;
            if self.step_n == self.step_len {
                self.steps.push(self.step_acc);
                self.step_acc = 0.0;
                self.step_n = 0;
            }
        }
    }

    /// Bütünleşik ses şiddeti (mutlak -70 LUFS ve göreli -10 LU kapılı). Çok kısa/sessiz kayıtta `None`.
    pub fn finish(&self) -> Option<Analysis> {
        let (lufs, power) = integrated_from_steps(&self.steps, self.step_len as f64)?;
        Some(Analysis { lufs, peak: self.peak, power, secs: self.frames as f64 / self.fs })
    }
}

/// 100 ms'lik güç toplamlarından (adım başına `n` örnek) kapılı bütünleşik şiddet: (LUFS, ortalama güç).
fn integrated_from_steps(steps: &[f64], n: f64) -> Option<(f32, f64)> {
    if steps.len() < 4 {
        return None;
    }
    // 400 ms'lik bloklar, 100 ms atlamalı (%75 örtüşme)
    let blocks: Vec<f64> = steps.windows(4).map(|w| w.iter().sum::<f64>() / (4.0 * n)).collect();
    let abs_thr = 10f64.powf((-70.0 + 0.691) / 10.0);
    let above: Vec<f64> = blocks.iter().copied().filter(|p| *p > abs_thr).collect();
    if above.is_empty() {
        return None;
    }
    let rel_thr = above.iter().sum::<f64>() / above.len() as f64 / 10.0; // -10 LU
    let gated: Vec<f64> = above.into_iter().filter(|p| *p > rel_thr).collect();
    if gated.is_empty() {
        return None;
    }
    let power = gated.iter().sum::<f64>() / gated.len() as f64;
    Some(((-0.691 + 10.0 * power.log10()) as f32, power))
}

// ------------------------------------------------------------------ gerçek tepe (ITU-R BS.1770 Ek 2)

const TP_TAPS: usize = 16;
/// Süzgeç penceresinin merkez örneğinin dizindeki yeri (güncel örnekten `TP_TAPS - 1 - TP_CENTER` geride).
pub const TP_CENTER: usize = 7;

/// 4x aşırı örnekleme ile örnekler arası (inter-sample) tepeyi bulur. Pencereli sinc, 3 ara faz.
pub struct TruePeak {
    hist: [f32; TP_TAPS],
    coef: [[f32; TP_TAPS]; 3],
    max: f32,
}

impl TruePeak {
    pub fn new() -> TruePeak {
        let mut coef = [[0f32; TP_TAPS]; 3];
        for (ph, c) in coef.iter_mut().enumerate() {
            let f = (ph + 1) as f64 * 0.25;
            let mut sum = 0.0;
            for k in 0..TP_TAPS {
                let d = (k as f64 - 7.0) - f; // örnek k ile ara nokta arası uzaklık (örnek cinsinden)
                let sinc = if d.abs() < 1e-12 { 1.0 } else { (std::f64::consts::PI * d).sin() / (std::f64::consts::PI * d) };
                let win = if d.abs() >= 8.0 { 0.0 } else { 0.5 * (1.0 + (std::f64::consts::PI * d / 8.0).cos()) };
                let v = sinc * win;
                c[k] = v as f32;
                sum += v;
            }
            for v in c.iter_mut() {
                *v = (*v as f64 / sum) as f32; // DC kazancı 1
            }
        }
        TruePeak { hist: [0.0; TP_TAPS], coef, max: 0.0 }
    }

    /// Bir örnek ekle; o ana kadarki en büyük gerçek tepeyi (doğrusal) döndürür.
    pub fn push(&mut self, x: f32) -> f32 {
        self.hist.copy_within(1.., 0);
        self.hist[TP_TAPS - 1] = x;
        self.max = self.max.max(x.abs());
        for c in &self.coef {
            let y: f32 = self.hist.iter().zip(c.iter()).map(|(h, k)| h * k).sum();
            self.max = self.max.max(y.abs());
        }
        self.max
    }

    /// Bir örnek ekler ve yalnızca şu anki pencerenin gerçek tepesini döndürür (koşan maksimum değil).
    /// Pencerenin merkezi 8 örnek geridedir: dönen değer `x[n-8]` ile `x[n-7]` arasındaki ara noktaları da kapsar.
    pub fn push_local(&mut self, x: f32) -> f32 {
        self.hist.copy_within(1.., 0);
        self.hist[TP_TAPS - 1] = x;
        let mut m = self.hist[TP_CENTER].abs();
        for c in &self.coef {
            let y: f32 = self.hist.iter().zip(c.iter()).map(|(h, k)| h * k).sum();
            m = m.max(y.abs());
        }
        self.max = self.max.max(m);
        m
    }

    pub fn max(&self) -> f32 {
        self.max
    }

    pub fn reset(&mut self) {
        self.max = 0.0;
        self.hist = [0.0; TP_TAPS];
    }
}

// ------------------------------------------------------------------ canlı ölçer (çıkış sinyali)

/// Çıkış akışı üzerinde sürekli: gerçek tepe, anlık (400 ms), kısa vadeli (3 sn) ve bütünleşik LUFS.
pub struct LiveMeter {
    filt: [[Bq; 2]; 2],
    step_len: usize,
    step_acc: f64,
    step_n: usize,
    /// yalnızca anlık (4) ve kısa vadeli (30) ölçüm için son adımlar
    steps: Vec<f64>,
    /// bütünleşik ölçüm için 400 ms'lik blok güçlerinin histogramı (0.1 LU'luk kutular): geçmiş boyutundan bağımsız, O(1) bellek
    hist: Vec<(u32, f64)>,
    tp: [TruePeak; 2],
    use_tp: bool,
    sample_peak: f32,
}

/// Son adımlardan fazlası tutulmaz (en uzun pencere 3 sn = 30 adım).
const KEEP_STEPS: usize = 40;
/// Histogram: -70 LUFS'tan +30 LUFS'a, 0.1 LU'luk kutular.
const HIST_BINS: usize = 1001;

fn hist_bin(lufs: f64) -> usize {
    (((lufs + 70.0) * 10.0).floor().max(0.0) as usize).min(HIST_BINS - 1)
}

impl LiveMeter {
    pub fn new(rate: u32) -> LiveMeter {
        let f = k_weighting(rate as f64);
        LiveMeter {
            filt: [f, f],
            step_len: ((rate as f64 * 0.1).round() as usize).max(1),
            step_acc: 0.0,
            step_n: 0,
            steps: vec![],
            hist: vec![(0, 0.0); HIST_BINS],
            tp: [TruePeak::new(), TruePeak::new()],
            use_tp: rate < 96000, // yüksek hızda örnekler arası tepe ihmal edilebilir
            sample_peak: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.steps.clear();
        self.hist.iter_mut().for_each(|b| *b = (0, 0.0));
        self.step_acc = 0.0;
        self.step_n = 0;
        self.tp[0].reset();
        self.tp[1].reset();
        self.sample_peak = 0.0;
    }

    /// Bir stereo çerçeve ekle; her 100 ms'de bir `true` döner (göstergeler yenilenebilir).
    pub fn feed(&mut self, l: f32, r: f32) -> bool {
        let mut p = 0.0;
        for (ch, x) in [l, r].into_iter().enumerate() {
            self.sample_peak = self.sample_peak.max(x.abs());
            if self.use_tp {
                self.tp[ch].push(x);
            }
            let y1 = self.filt[ch][0].step(x as f64);
            let y = self.filt[ch][1].step(y1);
            p += y * y;
        }
        self.step_acc += p;
        self.step_n += 1;
        if self.step_n == self.step_len {
            self.steps.push(self.step_acc);
            self.step_acc = 0.0;
            self.step_n = 0;
            if self.steps.len() >= 4 {
                // 400 ms'lik blok (son 4 adım): mutlak kapıyı (-70 LUFS) geçenler histograma girer
                let p = self.steps[self.steps.len() - 4..].iter().sum::<f64>() / (4.0 * self.step_len as f64);
                let lufs = -0.691 + 10.0 * p.log10();
                if lufs > -70.0 {
                    let b = &mut self.hist[hist_bin(lufs)];
                    b.0 += 1;
                    b.1 += p;
                }
            }
            if self.steps.len() > KEEP_STEPS {
                let extra = self.steps.len() - KEEP_STEPS;
                self.steps.drain(..extra);
            }
            return true;
        }
        false
    }

    fn mean_lufs(&self, count: usize) -> Option<f32> {
        if self.steps.len() < count {
            return None;
        }
        let tail = &self.steps[self.steps.len() - count..];
        let power = tail.iter().sum::<f64>() / (count as f64 * self.step_len as f64);
        if power <= 0.0 {
            return None;
        }
        let v = (-0.691 + 10.0 * power.log10()) as f32;
        (v > -120.0).then_some(v)
    }

    pub fn momentary(&self) -> Option<f32> {
        self.mean_lufs(4)
    }
    pub fn short_term(&self) -> Option<f32> {
        self.mean_lufs(30)
    }
    /// Bütünleşik ölçüm (mutlak -70 LUFS ve göreli -10 LU kapılı), histogramdan; ~1000 kutu üzerinde, bellek ayırmaz.
    /// Göreli kapının sınır kutusundaki bloklar kutu çözünürlüğü (0.1 LU) kadar belirsizdir.
    pub fn integrated(&self) -> Option<f32> {
        let (cnt, sum) = self.hist.iter().fold((0u64, 0.0f64), |a, b| (a.0 + b.0 as u64, a.1 + b.1));
        if cnt == 0 {
            return None;
        }
        let rel_thr = sum / cnt as f64 / 10.0;
        let first = hist_bin(-0.691 + 10.0 * rel_thr.log10()) + 1;
        let (gc, gs) = self.hist.iter().skip(first).fold((0u64, 0.0f64), |a, b| (a.0 + b.0 as u64, a.1 + b.1));
        if gc == 0 {
            return None;
        }
        Some((-0.691 + 10.0 * (gs / gc as f64).log10()) as f32)
    }
    /// Gerçek tepe (dBTP); sinyal yoksa `None`.
    pub fn true_peak_db(&self) -> Option<f32> {
        let m = if self.use_tp { self.tp[0].max().max(self.tp[1].max()) } else { self.sample_peak };
        (m > 1.0e-6).then(|| 20.0 * m.log10())
    }
}

/// ReplayGain kazancı (dB): referans seviyeye ulaşmak için gereken.
pub fn gain_for(lufs: f32) -> f32 {
    RG_REFERENCE_LUFS - lufs
}

// ------------------------------------------------------------------ önbellek

fn cache() -> &'static Mutex<HashMap<String, Entry>> {
    static C: OnceLock<Mutex<HashMap<String, Entry>>> = OnceLock::new();
    C.get_or_init(|| {
        let m: HashMap<String, Entry> = crate::fsutil::read_json(&cache_path()).unwrap_or_default();
        Mutex::new(m)
    })
}

fn cache_path() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Soundworks").join("loudness.json")
}

fn save_cache() {
    // test/ekran görüntüsü modunda kullanıcının önbelleğini yazma
    if crate::devenv::dev_var("SW_OFFSCREEN").is_ok() || crate::devenv::dev_var("SW_SHOT").is_ok() {
        return;
    }
    if let Ok(j) = serde_json::to_vec(&*cache().lock().unwrap()) {
        let _ = crate::fsutil::write_atomic(&cache_path(), &j);
    }
}

fn file_stamp(path: &Path) -> Option<(u64, u64)> {
    let md = std::fs::metadata(path).ok()?;
    let mtime = md.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some((md.len(), mtime))
}

/// Önbellekten ReplayGain değerleri (dosya değişmediyse).
pub fn lookup(path: &Path) -> Option<Rg> {
    let (size, mtime) = file_stamp(path)?;
    let c = cache().lock().unwrap();
    let e = c.get(&path.to_string_lossy().to_string())?;
    if e.size != size || e.mtime != mtime {
        return None;
    }
    Some(Rg { track_gain: Some(gain_for(e.a.lufs)), track_peak: Some(e.a.peak), album_gain: e.album_gain, album_peak: e.album_peak })
}

/// Önbellekte geçerli kaydı olan dosya sayısı.
pub fn cached_count(paths: &[String]) -> usize {
    paths.iter().filter(|p| lookup(Path::new(p)).is_some()).count()
}

// ------------------------------------------------------------------ analiz ve arka plan tarayıcı

/// Bir dosyayı baştan sona çözüp ölçer.
pub fn analyze_file(path: &Path) -> Option<Analysis> {
    let mut meter: Option<Meter> = None;
    crate::player::decode_native(path, |chunk, rate| {
        meter.get_or_insert_with(|| Meter::new(rate)).feed(chunk);
    })
    .ok()?;
    meter?.finish()
}

#[derive(Default)]
pub struct Scanner {
    pub total: AtomicUsize,
    pub done: AtomicUsize,
    pub running: AtomicBool,
    pub stop: AtomicBool,
}

/// Albüm kazancı: parçaların güçlerinin süreyle ağırlıklı ortalaması (kapılı bütünleşik değerin yakın bir yaklaşımı).
pub fn album_gain(items: &[Analysis]) -> Option<(f32, f32)> {
    let dur: f64 = items.iter().map(|a| a.secs).sum();
    if items.is_empty() || dur <= 0.0 {
        return None;
    }
    let power: f64 = items.iter().map(|a| a.power * a.secs).sum::<f64>() / dur;
    let peak = items.iter().map(|a| a.peak).fold(0.0, f32::max);
    Some((gain_for((-0.691 + 10.0 * power.log10()) as f32), peak))
}

/// Arka planda analiz et: önbellekte olanları atlar, bitince albüm kazançlarını hesaplar.
pub fn spawn(scanner: Arc<Scanner>, albums: Vec<Vec<String>>) {
    if scanner.running.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};
        unsafe {
            let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
        }
        let all: Vec<String> = albums.iter().flatten().cloned().collect();
        scanner.total.store(all.len(), Ordering::Relaxed);
        scanner.done.store(0, Ordering::Relaxed);
        let mut since_save = 0;
        for p in &all {
            if scanner.stop.load(Ordering::Relaxed) {
                break;
            }
            let path = Path::new(p);
            if lookup(path).is_none() {
                if let (Some(a), Some((size, mtime))) = (analyze_file(path), file_stamp(path)) {
                    cache().lock().unwrap().insert(p.clone(), Entry { size, mtime, a, album_gain: None, album_peak: None });
                    since_save += 1;
                }
            }
            scanner.done.fetch_add(1, Ordering::Relaxed);
            if since_save >= 25 {
                save_cache();
                since_save = 0;
            }
        }
        // albüm kazançları
        {
            let mut c = cache().lock().unwrap();
            for album in &albums {
                let items: Vec<Analysis> = album.iter().filter_map(|p| c.get(p).map(|e| e.a)).collect();
                if items.len() != album.len() {
                    continue; // eksik analiz varsa albüm değeri hesaplanmaz
                }
                if let Some((g, pk)) = album_gain(&items) {
                    for p in album {
                        if let Some(e) = c.get_mut(p) {
                            e.album_gain = Some(g);
                            e.album_peak = Some(pk);
                        }
                    }
                }
            }
        }
        save_cache();
        scanner.running.store(false, Ordering::SeqCst);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_stereo(fs: u32, secs: f32, amp: f32, f: f32) -> Vec<f32> {
        let n = (fs as f32 * secs) as usize;
        (0..n)
            .flat_map(|i| {
                let v = amp * (2.0 * std::f32::consts::PI * f * i as f32 / fs as f32).sin();
                [v, v]
            })
            .collect()
    }

    fn measure(fs: u32, buf: &[f32]) -> Analysis {
        let mut m = Meter::new(fs);
        m.feed(buf);
        m.finish().unwrap()
    }

    #[test]
    fn filter_matches_published_coefficients_at_48k() {
        // BS.1770-4 Tablo 1 (48 kHz)
        let [s1, s2] = k_weighting(48000.0);
        assert!((s1.b[0] - 1.53512485958697).abs() < 1e-9);
        assert!((s1.b[1] + 2.69169618940638).abs() < 1e-9);
        assert!((s1.b[2] - 1.19839281085285).abs() < 1e-9);
        assert!((s1.a[0] + 1.69065929318241).abs() < 1e-9);
        assert!((s1.a[1] - 0.73248077421585).abs() < 1e-9);
        assert!((s2.a[0] + 1.99004745483398).abs() < 1e-8);
        assert!((s2.a[1] - 0.99007225036621).abs() < 1e-8);
    }

    #[test]
    fn full_scale_stereo_sine_is_about_minus_20_lufs_at_minus_20_dbfs() {
        // iki kanalda -20 dBFS tepe (0.1) 997 Hz sinüs ≈ -20 LUFS
        let a = measure(48000, &sine_stereo(48000, 6.0, 0.1, 997.0));
        assert!((a.lufs + 20.0).abs() < 0.2, "{}", a.lufs);
        assert!((a.peak - 0.1).abs() < 1e-4);
        // 6 dB yükselince 6 LU artar
        let b = measure(48000, &sine_stereo(48000, 6.0, 0.2, 997.0));
        assert!((b.lufs - a.lufs - 6.02).abs() < 0.05);
    }

    #[test]
    fn other_sample_rates_give_the_same_loudness() {
        let a48 = measure(48000, &sine_stereo(48000, 6.0, 0.1, 997.0)).lufs;
        for fs in [44100u32, 96000, 192000] {
            let a = measure(fs, &sine_stereo(fs, 6.0, 0.1, 997.0)).lufs;
            assert!((a - a48).abs() < 0.15, "{fs}: {a} vs {a48}");
        }
    }

    #[test]
    fn silence_is_gated_out() {
        // 4 sn ses + 6 sn sessizlik: bütünleşik değer yalnızca sesli kısma bakar
        let mut loud = sine_stereo(48000, 4.0, 0.1, 997.0);
        let solo = measure(48000, &loud).lufs;
        loud.extend(vec![0.0f32; 48000 * 2 * 6]);
        let with_silence = measure(48000, &loud).lufs;
        assert!((with_silence - solo).abs() < 0.2, "{with_silence} vs {solo}");
        // tam sessizlik: ölçülemez
        let mut m = Meter::new(48000);
        m.feed(&vec![0.0f32; 48000 * 2 * 3]);
        assert!(m.finish().is_none());
    }

    #[test]
    fn relative_gate_ignores_quiet_passages() {
        // yüksek bölüm (-20 dBFS) + çok daha sessiz bölüm (-50 dBFS): göreli kapı sessizi dışarıda bırakır
        let mut x = sine_stereo(48000, 5.0, 0.1, 997.0);
        let loud_only = measure(48000, &x).lufs;
        x.extend(sine_stereo(48000, 5.0, 0.003, 997.0));
        let mixed = measure(48000, &x).lufs;
        assert!((mixed - loud_only).abs() < 0.3, "{mixed} vs {loud_only}");
    }

    #[test]
    fn replaygain_values() {
        // -20 LUFS parça: +2 dB ile -18 LUFS referansa gelir
        assert!((gain_for(-20.0) - 2.0).abs() < 1e-6);
        assert!((gain_for(-10.0) + 8.0).abs() < 1e-6);
    }

    #[test]
    fn album_gain_weights_by_duration() {
        let t = |lufs: f32, secs: f64| Analysis { lufs, peak: 0.5, power: 10f64.powf(((lufs as f64) + 0.691) / 10.0), secs };
        // iki aynı seviyeli parça: albüm = parça
        let (g, pk) = album_gain(&[t(-14.0, 200.0), t(-14.0, 100.0)]).unwrap();
        assert!((g + 4.0).abs() < 0.01 && pk == 0.5);
        // farklı seviyeler: güç ortalaması (süreyle ağırlıklı), yüksek parçaya yaklaşır
        let (g2, _) = album_gain(&[t(-10.0, 100.0), t(-20.0, 100.0)]).unwrap();
        // güç ortalaması: 10*log10((1 + 0.1)/2) = -2.596 dB -> -12.596 LUFS -> kazanç -5.404 dB
        assert!((g2 + 5.404).abs() < 0.01, "{g2}");
        // aritmetik ortalamadan (-15 LUFS) daha yüksek: enerji ortalaması yüksek parçaya yakındır
        assert!(g2 < gain_for(-15.0), "{g2}");
        assert!(album_gain(&[]).is_none());
    }

    #[test]
    fn true_peak_finds_inter_sample_overshoot() {
        // fs/4 sinüsü, 45° faz: örnekler ±0.707*A, gerçek tepe A (+3 dB fark)
        let a = 0.8f32;
        let mut tp = TruePeak::new();
        let mut sample_peak = 0f32;
        for n in 0..4000 {
            let x = a * (std::f32::consts::FRAC_PI_2 * n as f32 + std::f32::consts::FRAC_PI_4).sin();
            sample_peak = sample_peak.max(x.abs());
            tp.push(x);
        }
        assert!((sample_peak - a * 0.7071).abs() < 1e-3);
        let tp_db = 20.0 * tp.max().log10();
        assert!((tp_db - 20.0 * a.log10()).abs() < 0.15, "gercek tepe {tp_db} dBTP, beklenen {}", 20.0 * a.log10());
        assert!(tp.max() > sample_peak * 1.35);
        // sıfırlama
        tp.reset();
        assert_eq!(tp.max(), 0.0);
        // sabit (DC) sinyalde ara noktalar şişmemeli. Basamak başlangıcı gerçekten aşım yapar (Gibbs), o yüzden ısındıktan sonra ölç.
        let mut dc = TruePeak::new();
        for _ in 0..40 {
            dc.push(0.5);
        }
        dc.max = 0.0;
        for _ in 0..100 {
            dc.push(0.5);
        }
        assert!((dc.max() - 0.5).abs() < 0.01, "{}", dc.max());
    }

    fn feed_sine(m: &mut LiveMeter, fs: u32, secs: f32, amp: f32) {
        for i in 0..(fs as f32 * secs) as usize {
            let v = amp * (2.0 * std::f32::consts::PI * 997.0 * i as f32 / fs as f32).sin();
            m.feed(v, v);
        }
    }

    #[test]
    fn live_meter_momentary_short_and_integrated() {
        let mut m = LiveMeter::new(48000);
        assert!(m.momentary().is_none() && m.integrated().is_none() && m.true_peak_db().is_none());
        feed_sine(&mut m, 48000, 0.5, 0.1);
        let mo = m.momentary().unwrap();
        assert!((mo + 20.0).abs() < 0.3, "anlik {mo}");
        assert!(m.short_term().is_none(), "3 sn dolmadan kisa vadeli yok");
        feed_sine(&mut m, 48000, 4.0, 0.1);
        assert!((m.short_term().unwrap() + 20.0).abs() < 0.2);
        assert!((m.integrated().unwrap() + 20.0).abs() < 0.2);
        let tp = m.true_peak_db().unwrap();
        assert!((tp + 20.0).abs() < 0.3, "{tp}"); // 0.1 genlik = -20 dBFS tepe
        // sessizlik anlık değeri düşürür, bütünleşik değişmez (kapı)
        let integ = m.integrated().unwrap();
        for _ in 0..48000 * 3 {
            m.feed(0.0, 0.0);
        }
        assert!(m.momentary().is_none() || m.momentary().unwrap() < -60.0);
        assert!((m.integrated().unwrap() - integ).abs() < 0.2);
        // sıfırla
        m.reset();
        assert!(m.integrated().is_none() && m.true_peak_db().is_none());
    }

    #[test]
    fn histogram_integrated_matches_exact_gating_and_keeps_memory_flat() {
        // yüksek bölüm + 25 dB kısık bölüm + sessizlik: göreli kapı kısık bölümü dışarıda bırakır
        let mut live = LiveMeter::new(48000);
        let mut off = Meter::new(48000);
        let mut buf = vec![];
        for (secs, amp) in [(6.0f32, 0.2f32), (6.0, 0.2 * 0.056), (2.0, 0.0), (5.0, 0.15)] {
            for i in 0..(48000.0 * secs) as usize {
                let v = amp * (2.0 * std::f32::consts::PI * 300.0 * i as f32 / 48000.0).sin();
                live.feed(v, v);
                buf.extend([v, v]);
            }
        }
        off.feed(&buf);
        let exact = off.finish().unwrap().lufs;
        let got = live.integrated().unwrap();
        assert!((got - exact).abs() < 0.15, "histogram {got} vs tam {exact}");
        // geçmiş boyutu sabit: uzun süre beslense de adım listesi büyümez
        for _ in 0..48000 * 15 {
            live.feed(0.05, 0.05);
        }
        assert!(live.steps.len() <= KEEP_STEPS);
        assert!(live.integrated().is_some());
    }

    #[test]
    fn live_meter_matches_offline_meter() {
        let mut live = LiveMeter::new(44100);
        let mut off = Meter::new(44100);
        let buf: Vec<f32> = (0..44100 * 6)
            .flat_map(|i| {
                let v = 0.2 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 44100.0).sin();
                [v, v * 0.5]
            })
            .collect();
        off.feed(&buf);
        for f in buf.chunks_exact(2) {
            live.feed(f[0], f[1]);
        }
        let (a, b) = (live.integrated().unwrap(), off.finish().unwrap().lufs);
        assert!((a - b).abs() < 0.01, "{a} vs {b}");
    }
}
