//! Dahili müzik oynatıcı: symphonia ile çözer, 48 kHz'e çevirir, motorun halkasına yazar.
//! EQ burada uygulanmaz; render tarafı uygular (A/B anlık olsun diye).

use crate::engine::{choose_rate, Ring};
use anyhow::{anyhow, Context, Result};
use rubato::{FftFixedIn, Resampler};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, MetadataRevision, StandardTagKey};
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

pub const PLAYER_RATE: u32 = 48000;
const NONE: usize = usize::MAX;
/// symphonia 0.5.5 Opus çözücüsü içermez, bu yüzden `opus` listede yok.
pub const AUDIO_EXTS: &[&str] = &["mp3", "flac", "wav", "aif", "aiff", "caf", "ogg", "m4a", "aac", "mp4", "alac", "mka", "dsf", "dff"];

/// ReplayGain / R128 etiket değerleri (dB ve tepe).
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub struct Rg {
    pub track_gain: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_gain: Option<f32>,
    pub album_peak: Option<f32>,
}

/// "-6.54 dB", "+3.2dB", "-6.54" -> -6.54
pub fn parse_gain_db(s: &str) -> Option<f32> {
    let t = s.trim().trim_end_matches(|c: char| c == 'B' || c == 'b' || c == 'd' || c == 'D' || c == ' ');
    let v: f32 = t.trim().replace(',', ".").parse().ok()?;
    v.is_finite().then_some(v)
}

/// Tepe değeri (doğrusal, 0..~2).
pub fn parse_peak(s: &str) -> Option<f32> {
    let v: f32 = s.trim().replace(',', ".").parse().ok()?;
    (v.is_finite() && v > 0.0).then_some(v)
}

/// Seçilen moda göre dB kazancı; etiket yoksa `None` (dokunma). `mode`: 0 kapalı, 1 parça, 2 albüm.
/// `prevent_clip`: tepe değeri biliniyorsa kazanç, tepeyi 0 dBFS'in altında tutacak kadarla sınırlanır.
pub fn rg_gain_db(rg: &Rg, mode: u32, preamp_db: f32, prevent_clip: bool) -> Option<f32> {
    let (gain, peak) = match mode {
        1 => (rg.track_gain, rg.track_peak),
        2 => (rg.album_gain.or(rg.track_gain), if rg.album_gain.is_some() { rg.album_peak } else { rg.track_peak }),
        _ => return None,
    };
    let mut total = gain? + preamp_db;
    if prevent_clip {
        if let Some(pk) = peak {
            total = total.min(-20.0 * pk.log10());
        }
    }
    Some(total)
}

/// Arabelleğe (interleaved stereo) `from`'dan `to`'ya doğrusal kazanç rampası uygular.
pub fn apply_gain(buf: &mut [f32], from: f32, to: f32) {
    let n = (buf.len() / 2).max(1) as f32;
    if (from - to).abs() < 1.0e-6 {
        if (to - 1.0).abs() > 1.0e-6 {
            for s in buf.iter_mut() {
                *s *= to;
            }
        }
        return;
    }
    for (i, f) in buf.chunks_exact_mut(2).enumerate() {
        let g = from + (to - from) * (i as f32 + 1.0) / n;
        f[0] *= g;
        f[1] *= g;
    }
}

/// Bu parça DoP ile çalınabilir mi? Gerekenler: DSD dosyası, kullanıcı izni, hız eşleştirme ve cihazın o hızı (DoP için en az
/// 176.4 kHz, 24 bit exclusive) kabul etmesi. Cihaz yalnızca 48/96 kHz destekliyorsa liste boştur ve DoP asla seçilmez.
pub fn dop_decision(allowed: bool, follow: bool, dsd_dop_rate: Option<u32>, device_rates: &[u32]) -> bool {
    allowed && follow && dsd_dop_rate.is_some_and(|r| r >= 176_400 && device_rates.contains(&r))
}

/// Çalan parçanın bilgileri (arayüz için).
#[derive(Default, Clone)]
pub struct TrackMeta {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub codec: String,
    pub src_rate: u32,
    pub bits: u32,
    pub channels: u32,
    /// (genişlik, yükseklik, RGBA) küçültülmüş kapak
    pub cover: Option<(u32, u32, Vec<u8>)>,
    /// kapağın baskın rengi
    pub tint: [u8; 3],
    /// kaynak hız ile çıkış hızı farklıysa true
    pub resampled: bool,
    pub rg: Rg,
}

pub struct Player {
    pub shuffle: AtomicBool,
    pub repeat_one: AtomicBool,
    history: Mutex<Vec<usize>>,
    rng: AtomicU64,
    pub meta: Mutex<TrackMeta>,
    pub meta_id: AtomicU64,
    rate: AtomicU32,
    pub ring: Arc<Ring>,
    pub playlist: Mutex<Vec<PathBuf>>,
    pub index: AtomicUsize,
    pub playing: Arc<AtomicBool>,
    pub repeat: AtomicBool,
    pub error: Mutex<String>,
    volume: AtomicU32,
    /// hız eşleştirme: açıkken çıkış hızı parçanın hızına (cihaz destekliyorsa) çevrilir
    follow: AtomicBool,
    /// ReplayGain: 0 kapalı, 1 parça, 2 albüm
    rg_mode: AtomicU32,
    rg_preamp: AtomicU32,
    rg_prevent: AtomicBool,
    /// şu an uygulanan ReplayGain kazancı (dB, f32 bit); 0 = yok
    rg_applied: AtomicU32,
    supported: Mutex<Vec<u32>>,
    fallback_rate: AtomicU32,
    /// motorun yeni hızla yeniden başlatılmasını isteyen çözücü isteği (0 = yok)
    rate_req: AtomicU32,
    /// DSD dosyaları DoP ile çalınabilir mi (kullanıcı bu cihaz için onayladı, cihaz uygun)
    dop_allowed: AtomicBool,
    /// hedef cihazın DoP için kabul ettiği PCM hızları (boşsa DoP imkânsız)
    dop_rates: Mutex<Vec<u32>>,
    /// arayüzde gösterilecek tek seferlik bilgi (ör. DoP yerine PCM'e düşüldü)
    pub note: Mutex<Option<String>>,
    /// çalışan motor DoP modunda mı (arayüz motoru başlatırken bildirir)
    engine_dop: AtomicBool,
    /// bir sonraki motor başlatması DoP modunda olmalı mı (parça açılırken çözücü belirler)
    want_dop: AtomicBool,
    /// hız değişiminden sonra parçaya bu konumdan devam et
    resume_pos: Mutex<Option<f64>>,
    duration: AtomicU64,
    decoded: AtomicU64,
    seek_req: Mutex<Option<f64>>,
    load_req: AtomicBool,
    quit: AtomicBool,
}

impl Player {
    pub fn new() -> Arc<Player> {
        let playing = Arc::new(AtomicBool::new(false));
        let ring = Ring::with_gate(playing.clone());
        ring.vol.store((0.8f32 * 0.8).to_bits(), Ordering::Relaxed);
        let p = Arc::new(Player {
            shuffle: AtomicBool::new(false),
            repeat_one: AtomicBool::new(false),
            history: Mutex::new(vec![]),
            rng: AtomicU64::new(0),
            meta: Mutex::new(TrackMeta::default()),
            meta_id: AtomicU64::new(0),
            rate: AtomicU32::new(PLAYER_RATE),
            ring,
            playlist: Mutex::new(vec![]),
            index: AtomicUsize::new(NONE),
            playing,
            repeat: AtomicBool::new(false),
            error: Mutex::new(String::new()),
            volume: AtomicU32::new(0.8f32.to_bits()),
            follow: AtomicBool::new(false),
            rg_mode: AtomicU32::new(0),
            rg_preamp: AtomicU32::new(0f32.to_bits()),
            rg_prevent: AtomicBool::new(true),
            rg_applied: AtomicU32::new(0f32.to_bits()),
            supported: Mutex::new(vec![]),
            fallback_rate: AtomicU32::new(PLAYER_RATE),
            rate_req: AtomicU32::new(0),
            dop_allowed: AtomicBool::new(false),
            dop_rates: Mutex::new(vec![]),
            note: Mutex::new(None),
            engine_dop: AtomicBool::new(false),
            want_dop: AtomicBool::new(false),
            resume_pos: Mutex::new(None),
            duration: AtomicU64::new(0f64.to_bits()),
            decoded: AtomicU64::new(0f64.to_bits()),
            seek_req: Mutex::new(None),
            load_req: AtomicBool::new(false),
            quit: AtomicBool::new(false),
        });
        let t = p.clone();
        std::thread::spawn(move || t.decode_loop());
        p
    }

    // ---- kontroller
    pub fn rate(&self) -> u32 {
        self.rate.load(Ordering::Relaxed)
    }
    pub fn set_rate(&self, r: u32) {
        self.rate.store(r, Ordering::Relaxed);
    }
    /// ReplayGain ayarları (oynarken de değişebilir; geçiş yumuşatılır).
    pub fn set_replaygain(&self, mode: u32, preamp_db: f32, prevent_clip: bool) {
        self.rg_mode.store(mode, Ordering::Relaxed);
        self.rg_preamp.store(preamp_db.to_bits(), Ordering::Relaxed);
        self.rg_prevent.store(prevent_clip, Ordering::Relaxed);
    }
    /// Şu an uygulanan ReplayGain kazancı (dB); etiket yoksa/kapalıysa `None`.
    pub fn rg_applied_db(&self) -> Option<f32> {
        // ayar kapalıysa son çözülen parçadan kalan değer geçerli sayılmaz (duraklatılmışken çözücü değeri güncellemez)
        if self.rg_mode.load(Ordering::Relaxed) == 0 {
            return None;
        }
        let v = f32::from_bits(self.rg_applied.load(Ordering::Relaxed));
        (v.abs() > 0.001).then_some(v)
    }
    /// Hız eşleştirmeyi aç/kapat. `supported`: cihazın kabul ettiği hızlar (boşsa `fallback` sabit hızı kullanılır).
    pub fn set_follow(&self, on: bool, supported: Vec<u32>, fallback: u32) {
        *self.supported.lock().unwrap() = supported;
        self.fallback_rate.store(fallback, Ordering::Relaxed);
        self.follow.store(on, Ordering::SeqCst);
    }
    pub fn set_dop_allowed(&self, on: bool) {
        self.dop_allowed.store(on, Ordering::SeqCst);
    }
    pub fn set_dop_rates(&self, r: Vec<u32>) {
        *self.dop_rates.lock().unwrap() = r;
    }
    pub fn set_engine_dop(&self, on: bool) {
        self.engine_dop.store(on, Ordering::SeqCst);
    }
    pub fn want_dop(&self) -> bool {
        self.want_dop.load(Ordering::SeqCst)
    }
    pub fn engine_dop(&self) -> bool {
        self.engine_dop.load(Ordering::SeqCst)
    }
    /// Geçerli parçayı kaldığı yerden yeniden açar (ör. DoP başarısız olunca PCM'e düşmek için).
    pub fn reload_current(&self) {
        self.want_dop.store(false, Ordering::SeqCst);
        if self.current().is_some() {
            *self.resume_pos.lock().unwrap() = Some(self.position());
            self.load_req.store(true, Ordering::SeqCst);
        }
    }
    /// Çözücü yeni bir hız istediyse (motor yeniden başlatılmalı) onu verir.
    pub fn take_rate_req(&self) -> Option<u32> {
        match self.rate_req.swap(0, Ordering::SeqCst) {
            0 => None,
            r => Some(r),
        }
    }
    /// Motorun fiilen açtığı hız beklenenden farklıysa: eşleştirmeyi bırak, bu hıza geç ve parçayı kaldığı yerden yeniden aç.
    pub fn force_rate(&self, r: u32) {
        self.follow.store(false, Ordering::SeqCst);
        self.fallback_rate.store(r, Ordering::Relaxed);
        self.rate.store(r, Ordering::Relaxed);
        if self.current().is_some() {
            *self.resume_pos.lock().unwrap() = Some(self.position());
            self.load_req.store(true, Ordering::SeqCst);
        }
    }
    fn ring_limit(&self) -> usize {
        self.rate() as usize * 150 / 1000
    }
    pub fn current(&self) -> Option<usize> {
        let i = self.index.load(Ordering::SeqCst);
        (i != NONE).then_some(i)
    }
    pub fn play_index(&self, i: usize) {
        if i < self.playlist.lock().unwrap().len() {
            self.index.store(i, Ordering::SeqCst);
            self.load_req.store(true, Ordering::SeqCst);
            self.playing.store(true, Ordering::SeqCst);
        }
    }
    pub fn toggle(&self) {
        if self.current().is_none() {
            self.play_index(0);
        } else {
            let p = self.playing.load(Ordering::SeqCst);
            self.playing.store(!p, Ordering::SeqCst);
        }
    }
    fn random_other(&self, n: usize, avoid: usize) -> usize {
        if n <= 1 {
            return 0;
        }
        let mut x = self.rng.load(Ordering::Relaxed);
        if x == 0 {
            x = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(88172645463325252)
                | 1;
        }
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng.store(x, Ordering::Relaxed);
        let mut r = (x % (n as u64 - 1)) as usize;
        if r >= avoid {
            r += 1;
        }
        r
    }

    fn push_history(&self, i: usize) {
        let mut h = self.history.lock().unwrap();
        h.push(i);
        if h.len() > 200 {
            h.remove(0);
        }
    }

    pub fn next(&self) {
        let n = self.playlist.lock().unwrap().len();
        if let Some(i) = self.current() {
            if self.shuffle.load(Ordering::Relaxed) && n > 1 {
                self.push_history(i);
                let j = self.random_other(n, i);
                self.play_index(j);
            } else if i + 1 < n {
                self.push_history(i);
                self.play_index(i + 1);
            } else if self.repeat.load(Ordering::Relaxed) && n > 0 {
                self.push_history(i);
                self.play_index(0);
            }
        }
    }
    pub fn prev(&self) {
        if let Some(i) = self.current() {
            if self.position() > 3.0 {
                self.seek(0.0);
            } else if self.shuffle.load(Ordering::Relaxed) {
                match self.history.lock().unwrap().pop() {
                    Some(j) => self.play_index(j),
                    None => self.seek(0.0),
                }
            } else if i == 0 {
                self.seek(0.0);
            } else {
                self.play_index(i - 1);
            }
        }
    }
    pub fn seek(&self, secs: f64) {
        *self.seek_req.lock().unwrap() = Some(secs.max(0.0));
    }
    pub fn set_volume(&self, v: f32) {
        let v = v.clamp(0.0, 1.0);
        self.volume.store(v.to_bits(), Ordering::Relaxed);
        // ses motorda uygulanır: yumuşatılmış, 64 bit hassasiyetle ve dither'dan hemen önce (yaklaşık algısal eğri v²)
        self.ring.vol.store((v * v).to_bits(), Ordering::Relaxed);
    }
    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }
    pub fn duration(&self) -> f64 {
        f64::from_bits(self.duration.load(Ordering::Relaxed))
    }
    pub fn position(&self) -> f64 {
        let buffered = self.ring.q.lock().unwrap().len() as f64 / 2.0 / self.rate() as f64;
        (f64::from_bits(self.decoded.load(Ordering::Relaxed)) - buffered).max(0.0)
    }
    pub fn add(&self, paths: Vec<PathBuf>) {
        self.playlist.lock().unwrap().extend(paths);
    }
    pub fn remove(&self, i: usize) {
        let mut pl = self.playlist.lock().unwrap();
        if i >= pl.len() {
            return;
        }
        pl.remove(i);
        let cur = self.index.load(Ordering::SeqCst);
        if cur == i {
            self.index.store(NONE, Ordering::SeqCst);
            self.playing.store(false, Ordering::SeqCst);
            self.load_req.store(true, Ordering::SeqCst);
        } else if cur != NONE && i < cur {
            self.index.store(cur - 1, Ordering::SeqCst);
        }
    }
    /// Kuyrukta bir parçanın yerini değiştir; `to`, taşıma sonrası listedeki son konumdur. Çalan parça izlenir.
    pub fn move_item(&self, from: usize, to: usize) {
        let mut pl = self.playlist.lock().unwrap();
        if from >= pl.len() || to >= pl.len() || from == to {
            return;
        }
        let item = pl.remove(from);
        pl.insert(to, item);
        let cur = self.index.load(Ordering::SeqCst);
        if cur == NONE {
            return;
        }
        let new_cur = if cur == from {
            to
        } else if from < cur && to >= cur {
            cur - 1
        } else if from > cur && to <= cur {
            cur + 1
        } else {
            cur
        };
        self.index.store(new_cur, Ordering::SeqCst);
    }

    /// Parçaları çalan parçanın hemen arkasına ekler (hiçbiri çalmıyorsa sona).
    pub fn insert_next(&self, paths: Vec<PathBuf>) {
        let mut pl = self.playlist.lock().unwrap();
        let cur = self.index.load(Ordering::SeqCst);
        let at = if cur == NONE { pl.len() } else { (cur + 1).min(pl.len()) };
        for (k, p) in paths.into_iter().enumerate() {
            pl.insert(at + k, p);
        }
    }

    /// Parçayı yükler ve konumuna götürür ama ÇALMAZ (kaldığın yerden devam için). Çal'a basınca oradan başlar.
    pub fn load_paused(&self, i: usize, pos_secs: f64) {
        if i >= self.playlist.lock().unwrap().len() {
            return;
        }
        self.playing.store(false, Ordering::SeqCst);
        self.index.store(i, Ordering::SeqCst);
        *self.resume_pos.lock().unwrap() = Some(pos_secs.max(0.0));
        self.load_req.store(true, Ordering::SeqCst);
    }

    pub fn clear(&self) {
        self.playlist.lock().unwrap().clear();
        self.index.store(NONE, Ordering::SeqCst);
        self.playing.store(false, Ordering::SeqCst);
        self.load_req.store(true, Ordering::SeqCst);
    }
    pub fn shutdown(&self) {
        self.quit.store(true, Ordering::SeqCst);
    }

    /// Parçayı açar. Çıkış modu (PCM / DoP) ya da hızı parçaya uymuyorsa: önceki parçanın kuyruğunun çalıp bitmesini bekler,
    /// motorun yeni mod/hızla yeniden başlatılmasını ister ve onay gelince parçayı açar. Mod uyuşmazlığında (DoP motoruna PCM
    /// verisi ya da tersi) veri ASLA gönderilmez: onay gelmezse hata döner (DoP'a yanlış veri yüksek gürültü olur).
    fn open_track(&self, path: &Path) -> Result<Track> {
        let follow = self.follow.load(Ordering::SeqCst);
        let dsd_rate = if crate::dsd::is_dsd(path) { crate::dsd::probe(path).ok().map(|i| i.dop_rate()) } else { None };
        let allowed = self.dop_allowed.load(Ordering::SeqCst);
        let dop = dop_decision(allowed, follow, dsd_rate, &self.dop_rates.lock().unwrap());
        if allowed && follow && dsd_rate.is_some() && !dop {
            // izin var ama cihaz bu DSD hızının DoP biçimini (ör. DSD128 için 352.8 kHz) kabul etmiyor: bu parça PCM'e çevrilir
            *self.note.lock().unwrap() = Some("Cihaz bu DSD hızı için DoP'u desteklemiyor; PCM'e çevrilerek çalınıyor".to_string());
        }
        let engine_dop = self.engine_dop.load(Ordering::SeqCst);
        self.want_dop.store(dop, Ordering::SeqCst);
        let (t, want) = if dop {
            let t = Track::open_dsd(path, self.rate(), true)?;
            let want = t.rate;
            (t, want)
        } else {
            let t = Track::open(path, self.rate())?;
            let want = if follow {
                choose_rate(t.rate, &self.supported.lock().unwrap(), self.fallback_rate.load(Ordering::Relaxed))
            } else if engine_dop {
                self.fallback_rate.load(Ordering::Relaxed)
            } else {
                self.rate()
            };
            (t, want)
        };
        if want == self.rate() && engine_dop == dop {
            return Ok(t);
        }
        // önceki parçanın son sesi bitsin (kesilmesin); duraklatılmışsa bekleme
        let t0 = std::time::Instant::now();
        // motor, halkada 2 çerçeveden azı kalınca tüketmez (enterpolasyon için iki çerçeve gerekir): o kırıntı beklenmez,
        // yoksa her hız değişiminde 3 sn sessizlik olur
        while self.playing.load(Ordering::Relaxed)
            && self.ring.q.lock().unwrap().len() >= 4
            && t0.elapsed() < Duration::from_secs(3)
            && !self.quit.load(Ordering::Relaxed)
            && !self.load_req.load(Ordering::Relaxed)
        {
            std::thread::sleep(Duration::from_millis(3));
        }
        self.ring.q.lock().unwrap().clear();
        self.rate_req.store(want, Ordering::SeqCst);
        // arayüz motoru yeniden başlatıp `set_rate(want)` ve `set_engine_dop` ile onaylar
        let t1 = std::time::Instant::now();
        let confirmed = |s: &Player| s.rate() == want && s.engine_dop.load(Ordering::SeqCst) == dop;
        while !confirmed(self) && t1.elapsed() < Duration::from_secs(4) && !self.quit.load(Ordering::Relaxed) && !self.load_req.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(4));
        }
        if !confirmed(self) {
            if dop || self.engine_dop.load(Ordering::SeqCst) {
                return Err(anyhow!("çıkış modu değiştirilemedi (DoP / PCM)"));
            }
            // onay gelmedi (motor yok / kullanıcı başka parçaya geçti): mevcut hızla devam, gerekirse yeniden örneklenir
            return Ok(t);
        }
        if dop { Track::open_dsd(path, want, true) } else { Track::open(path, want) }
    }

    /// Halkayı temizlemeden önce motoru birkaç ms'de sessize indirir (seek/parça değişiminde tık olmasın).
    fn quiesce(&self) {
        self.ring.hold.store(true, Ordering::SeqCst);
        let t0 = std::time::Instant::now();
        while !self.ring.muted.load(Ordering::SeqCst) && t0.elapsed() < Duration::from_millis(40) {
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    // ---- çözücü thread'i
    fn decode_loop(&self) {
        let mut track: Option<Track> = None;
        while !self.quit.load(Ordering::Relaxed) {
            if self.load_req.swap(false, Ordering::SeqCst) {
                self.quiesce();
                self.ring.q.lock().unwrap().clear();
                *self.seek_req.lock().unwrap() = None;
                self.decoded.store(0f64.to_bits(), Ordering::Relaxed);
                self.duration.store(0f64.to_bits(), Ordering::Relaxed);
                track = None;
                if let Some(i) = self.current() {
                    let path = self.playlist.lock().unwrap().get(i).cloned();
                    if let Some(path) = path {
                        match self.open_track(&path) {
                            Ok(mut t) => {
                                if let Some(pos) = self.resume_pos.lock().unwrap().take() {
                                    t.seek(pos);
                                    self.decoded.store(t.pos_secs().to_bits(), Ordering::Relaxed);
                                }
                                self.set_meta(&t, &path);
                                self.duration.store(t.duration.to_bits(), Ordering::Relaxed);
                                self.error.lock().unwrap().clear();
                                track = Some(t);
                            }
                            Err(e) => {
                                *self.error.lock().unwrap() = format!("{}: {e:#}", path.display());
                                self.playing.store(false, Ordering::SeqCst);
                            }
                        }
                    }
                }
                self.ring.hold.store(false, Ordering::SeqCst);
            }
            let Some(t) = track.as_mut() else {
                std::thread::sleep(Duration::from_millis(30));
                continue;
            };
            let seek_to = self.seek_req.lock().unwrap().take();
            if let Some(s) = seek_to {
                self.quiesce();
                t.seek(s);
                self.ring.q.lock().unwrap().clear();
                self.decoded.store(t.pos_secs().to_bits(), Ordering::Relaxed);
                self.ring.hold.store(false, Ordering::SeqCst);
            }
            if !self.playing.load(Ordering::Relaxed)
                || self.ring.q.lock().unwrap().len() / 2 > self.ring_limit()
            {
                std::thread::sleep(Duration::from_millis(4));
                continue;
            }
            match t.next_chunk() {
                Some(mut samples) => {
                    // ReplayGain: parçaya özgü kazanç, parça sınırında bile doğru (gapless'ta eski parçaya uygulanmaz)
                    if t.is_dop() {
                        // DoP: örnekler DSD bitleridir; hiçbir kazanç/işlem uygulanamaz
                        self.rg_applied.store(0f32.to_bits(), Ordering::Relaxed);
                        self.ring.q.lock().unwrap().extend(samples.iter());
                        self.decoded.store(t.pos_secs().to_bits(), Ordering::Relaxed);
                        continue;
                    }
                    let mode = self.rg_mode.load(Ordering::Relaxed);
                    let db = rg_gain_db(&t.meta.rg, mode, f32::from_bits(self.rg_preamp.load(Ordering::Relaxed)), self.rg_prevent.load(Ordering::Relaxed)).unwrap_or(0.0);
                    let gl = 10f32.powf(db / 20.0);
                    let prev = t.gain_prev.unwrap_or(gl);
                    apply_gain(&mut samples, prev, gl);
                    t.gain_prev = Some(gl);
                    self.rg_applied.store(db.to_bits(), Ordering::Relaxed);
                    self.ring.q.lock().unwrap().extend(samples.iter());
                    self.decoded.store(t.pos_secs().to_bits(), Ordering::Relaxed);
                }
                None => {
                    // parça bitti: halkanın boşalmasını bekle, sonra sıradakine geç
                    let n = self.playlist.lock().unwrap().len();
                    let i = self.current().unwrap_or(0);
                    track = None;
                    if self.repeat_one.load(Ordering::Relaxed) && n > 0 {
                        self.open_current(&mut track);
                    } else if self.shuffle.load(Ordering::Relaxed) && n > 1 {
                        self.push_history(i);
                        let j = self.random_other(n, i);
                        self.index.store(j, Ordering::SeqCst);
                        self.open_current(&mut track);
                    } else if i + 1 < n {
                        self.index.store(i + 1, Ordering::SeqCst);
                        self.load_req.store(true, Ordering::SeqCst);
                        // load_req halkayı temizlemesin: kuyruktaki son ses çalsın
                        self.load_req.store(false, Ordering::SeqCst);
                        self.open_current(&mut track);
                    } else if self.repeat.load(Ordering::Relaxed) && n > 0 {
                        self.index.store(0, Ordering::SeqCst);
                        self.open_current(&mut track);
                    } else {
                        self.playing.store(false, Ordering::SeqCst);
                        self.index.store(NONE, Ordering::SeqCst);
                    }
                }
            }
        }
    }

    fn set_meta(&self, t: &Track, path: &Path) {
        let mut m = t.meta.clone();
        if m.title.is_empty() {
            m.title = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        }
        *self.meta.lock().unwrap() = m;
        self.meta_id.fetch_add(1, Ordering::SeqCst);
    }

    fn open_current(&self, track: &mut Option<Track>) {
        let Some(i) = self.current() else { return };
        let path = self.playlist.lock().unwrap().get(i).cloned();
        let Some(path) = path else { return };
        match self.open_track(&path) {
            Ok(t) => {
                self.set_meta(&t, &path);
                self.duration.store(t.duration.to_bits(), Ordering::Relaxed);
                self.decoded.store(0f64.to_bits(), Ordering::Relaxed);
                *track = Some(t);
            }
            Err(e) => {
                *self.error.lock().unwrap() = format!("{}: {e:#}", path.display());
                self.playing.store(false, Ordering::SeqCst);
            }
        }
    }
}

/// Dosyayı kendi örnekleme hızında (yeniden örneklemeden) baştan sona çözüp stereo parçaları `f`'e verir.
pub(crate) fn decode_native(path: &Path, mut f: impl FnMut(&[f32], u32)) -> Result<()> {
    let rate = Track::open(path, 48000)?.rate;
    let mut t = Track::open(path, rate)?;
    while let Some(c) = t.next_chunk() {
        f(&c, rate);
    }
    Ok(())
}

/// DSD kaynağı: PCM'e çevrilerek (`conv` dolu) ya da DoP olarak (`conv` boş, `dop`) okunur.
struct DsdPlay {
    src: crate::dsd::DsdSource,
    conv: Option<crate::dsd::Dsd2Pcm>,
    dop: bool,
}

/// Bir kaynaktan gelen sonuç.
enum Got {
    Data(Vec<f32>),
    Skip,
    End,
}

struct Track {
    format: Option<Box<dyn FormatReader>>,
    decoder: Option<Box<dyn Decoder>>,
    dsd: Option<DsdPlay>,
    track_id: u32,
    channels: usize,
    rate: u32,
    out_rate: u32,
    meta: TrackMeta,
    resampler: Option<FftFixedIn<f32>>,
    inbuf: [Vec<f32>; 2],
    out_frames: u64,
    offset: f64,
    pub duration: f64,
    /// resampler'a verilen toplam giriş kare sayısı (kuyruk boşaltımında beklenen çıkış uzunluğu için)
    in_total: u64,
    /// resampler'ın başlangıç gecikmesinden atılacak kalan çıkış kareleri
    delay_left: usize,
    flushed: bool,
    /// son uygulanan ReplayGain doğrusal kazancı (rampa başlangıcı)
    gain_prev: Option<f32>,
    /// çok kanallı kaynak için stereo downmix tablosu (kanal başına sol/sağ katsayısı)
    map: Vec<(f32, f32)>,
    map_ch: usize,
}

/// Kanal düzenine göre stereo downmix katsayıları (ITU-R BS.775); toplam katsayı > 1 ise tepe aşımı olmasın diye normalize edilir.
/// LFE atılır. 1-2 kanalda tablo kullanılmaz (doğrudan geçiş).
fn downmix_map(ch: symphonia::core::audio::Channels) -> Vec<(f32, f32)> {
    use symphonia::core::audio::Channels as C;
    const K: f32 = 0.707_106_8;
    let mut m: Vec<(f32, f32)> = ch
        .iter()
        .map(|c| match c {
            C::FRONT_LEFT => (1.0, 0.0),
            C::FRONT_RIGHT => (0.0, 1.0),
            C::FRONT_CENTRE => (K, K),
            C::LFE1 | C::LFE2 => (0.0, 0.0),
            C::REAR_LEFT | C::SIDE_LEFT | C::TOP_REAR_LEFT => (K, 0.0),
            C::REAR_RIGHT | C::SIDE_RIGHT | C::TOP_REAR_RIGHT => (0.0, K),
            C::FRONT_LEFT_CENTRE | C::FRONT_LEFT_WIDE | C::TOP_FRONT_LEFT => (0.8, 0.2),
            C::FRONT_RIGHT_CENTRE | C::FRONT_RIGHT_WIDE | C::TOP_FRONT_RIGHT => (0.2, 0.8),
            C::REAR_LEFT_CENTRE => (0.8 * K, 0.2 * K),
            C::REAR_RIGHT_CENTRE => (0.2 * K, 0.8 * K),
            _ => (0.5 * K, 0.5 * K),
        })
        .collect();
    let sum_l: f32 = m.iter().map(|x| x.0).sum();
    let sum_r: f32 = m.iter().map(|x| x.1).sum();
    let g = 1.0 / sum_l.max(sum_r).max(1.0);
    for x in m.iter_mut() {
        x.0 *= g;
        x.1 *= g;
    }
    m
}

impl Track {
    fn open(path: &Path, out_rate: u32) -> Result<Track> {
        if crate::dsd::is_dsd(path) {
            return Track::open_dsd(path, out_rate, false);
        }
        let file = File::open(path).context("dosya açılamadı")?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let mut probed = symphonia::default::get_probe()
            .format(&hint, mss, &FormatOptions { enable_gapless: true, ..Default::default() }, &MetadataOptions::default())
            .map_err(|e| anyhow!("format tanınmadı: {e}"))?;
        let mut meta = TrackMeta::default();
        if let Some(m) = probed.metadata.get() {
            if let Some(r) = m.current() {
                read_meta(r, &mut meta);
            }
        }
        let mut format = probed.format;
        if let Some(r) = format.metadata().current() {
            read_meta(r, &mut meta);
        }
        // etiket yoksa kendi ölçümümüzü (önbellekten) kullan; dosya etiketi her zaman önceliklidir
        if meta.rg.track_gain.is_none() || meta.rg.album_gain.is_none() {
            if let Some(c) = crate::loudness::lookup(path) {
                meta.rg.track_gain = meta.rg.track_gain.or(c.track_gain);
                meta.rg.track_peak = meta.rg.track_peak.or(c.track_peak);
                meta.rg.album_gain = meta.rg.album_gain.or(c.album_gain);
                meta.rg.album_peak = meta.rg.album_peak.or(c.album_peak);
            }
        }
        let t = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| anyhow!("ses kanalı yok"))?;
        let track_id = t.id;
        let rate = t.codec_params.sample_rate.ok_or_else(|| anyhow!("örnekleme hızı yok"))?;
        let channels = t.codec_params.channels.map(|c| c.count()).unwrap_or(2).max(1);
        let duration = t.codec_params.n_frames.map(|n| n as f64 / rate as f64).unwrap_or(0.0);
        let decoder = symphonia::default::get_codecs()
            .make(&t.codec_params, &DecoderOptions::default())
            .map_err(|e| anyhow!("codec desteklenmiyor: {e}"))?;
        meta.codec = symphonia::default::get_codecs()
            .get_codec(t.codec_params.codec)
            .map(|d| d.short_name.to_string())
            .unwrap_or_default();
        meta.src_rate = rate;
        meta.bits = t.codec_params.bits_per_sample.unwrap_or(0);
        meta.channels = chans_count(t.codec_params.channels.map(|c| c.count()));
        meta.resampled = rate != out_rate;
        let resampler = if rate != out_rate {
            Some(FftFixedIn::<f32>::new(rate as usize, out_rate as usize, 1024, 2, 2).map_err(|e| anyhow!("{e}"))?)
        } else {
            None
        };
        let delay_left = resampler.as_ref().map(|r| r.output_delay()).unwrap_or(0);
        Ok(Track {
            format: Some(format),
            decoder: Some(decoder),
            dsd: None,
            track_id,
            channels,
            rate,
            out_rate,
            meta,
            resampler,
            inbuf: [vec![], vec![]],
            out_frames: 0,
            offset: 0.0,
            duration,
            in_total: 0,
            delay_left,
            flushed: false,
            gain_prev: None,
            map: vec![],
            map_ch: 0,
        })
    }

    fn pos_secs(&self) -> f64 {
        self.offset + self.out_frames as f64 / self.out_rate as f64
    }

    fn seek(&mut self, secs: f64) {
        let secs = if self.duration > 0.0 { secs.min(self.duration - 0.05).max(0.0) } else { secs };
        let ok = if let Some(d) = self.dsd.as_mut() {
            d.src.seek_secs(secs);
            if let Some(c) = d.conv.as_mut() {
                c.reset();
            }
            true
        } else {
            let tid = self.track_id;
            let ok = self
                .format
                .as_mut()
                .map(|f| f.seek(SeekMode::Accurate, SeekTo::Time { time: Time::from(secs), track_id: Some(tid) }).is_ok())
                .unwrap_or(false);
            if ok {
                if let Some(d) = self.decoder.as_mut() {
                    d.reset();
                }
            }
            ok
        };
        if ok {
            self.inbuf = [vec![], vec![]];
            if let Some(r) = self.resampler.as_mut() {
                r.reset();
                self.delay_left = r.output_delay();
            }
            self.in_total = 0;
            self.flushed = false;
            self.offset = secs;
            self.out_frames = 0;
        }
    }

    /// Resampler'a yetecek kadar giriş birikmişse işler; başlangıç gecikmesini atar.
    fn drain_resampler(&mut self) -> Option<Vec<f32>> {
        let Track { resampler, inbuf, delay_left, .. } = self;
        let rs = resampler.as_mut()?;
        let mut out = vec![];
        while inbuf[0].len() >= rs.input_frames_next() {
            let n = rs.input_frames_next();
            let chunk = [inbuf[0].drain(..n).collect::<Vec<f32>>(), inbuf[1].drain(..n).collect::<Vec<f32>>()];
            let o = rs.process(&chunk, None).ok()?;
            let skip = (*delay_left).min(o[0].len());
            *delay_left -= skip;
            for k in skip..o[0].len() {
                out.push(o[0][k]);
                out.push(o[1][k]);
            }
        }
        Some(out)
    }

    /// Parça sonunda resampler kuyruğunu boşaltır: dosyanın gerçek uzunluğu kadar çıkış verir (son ~20 ms kaybolmaz).
    fn flush_tail(&mut self) -> Option<Vec<f32>> {
        if self.flushed || self.resampler.is_none() {
            return None;
        }
        self.flushed = true;
        let expected = (self.in_total as f64 * self.out_rate as f64 / self.rate as f64).round() as u64;
        let need = expected.saturating_sub(self.out_frames) as usize;
        if need == 0 {
            return None;
        }
        let mut out: Vec<f32> = vec![];
        for _ in 0..16 {
            if out.len() / 2 >= need {
                break;
            }
            // eksik girişi sessizlikle tamamla
            let want = self.resampler.as_ref()?.input_frames_next();
            while self.inbuf[0].len() < want {
                self.inbuf[0].push(0.0);
                self.inbuf[1].push(0.0);
            }
            out.extend(self.drain_resampler()?);
        }
        out.truncate(need * 2);
        if out.is_empty() {
            return None;
        }
        self.out_frames += (out.len() / 2) as u64;
        Some(out)
    }

    /// symphonia kaynağından bir paket: stereo interleaved f32.
    fn sym_next(&mut self) -> Got {
        let (Some(format), Some(decoder)) = (self.format.as_mut(), self.decoder.as_mut()) else { return Got::End };
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(_) => return Got::End,
        };
        if packet.track_id() != self.track_id {
            return Got::Skip;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymError::DecodeError(_)) => return Got::Skip,
            Err(_) => return Got::End,
        };
        let spec = *decoded.spec();
        let mut sb = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        sb.copy_interleaved_ref(decoded);
        let s = sb.samples();
        let ch = spec.channels.count().max(1);
        let frames = s.len() / ch;

        // stereo/mono doğrudan; çok kanallı kaynak tablo ile stereo'ya indirgenir
        let mut st: Vec<f32> = Vec::with_capacity(frames * 2);
        if ch <= 2 {
            for i in 0..frames {
                let l = s[i * ch];
                let r = if ch > 1 { s[i * ch + 1] } else { l };
                st.push(l);
                st.push(r);
            }
        } else {
            if self.map_ch != ch {
                self.map = downmix_map(spec.channels);
                self.map_ch = ch;
            }
            for i in 0..frames {
                let (mut l, mut r) = (0.0f32, 0.0f32);
                for (k, (gl, gr)) in self.map.iter().enumerate() {
                    let v = s[i * ch + k];
                    l += v * gl;
                    r += v * gr;
                }
                st.push(l);
                st.push(r);
            }
        }
        Got::Data(st)
    }

    /// DSD kaynağından bir parça: PCM'e çevrilmiş stereo (352.8/384 kHz) ya da DoP için 16 bitlik DSD değerleri.
    fn dsd_next(&mut self) -> Got {
        let Some(d) = self.dsd.as_mut() else { return Got::End };
        let (mut l, mut r) = match d.src.read(if d.dop { 8192 } else { 8192 }) {
            Ok(Some(x)) => x,
            _ => return Got::End,
        };
        if d.dop {
            if l.len() % 2 == 1 {
                l.push(crate::dsd::DSD_SILENCE);
                r.push(crate::dsd::DSD_SILENCE);
            }
            return Got::Data(crate::dsd::dop_pairs(&l, &r));
        }
        match d.conv.as_mut() {
            Some(c) => Got::Data(c.process(&l, &r)),
            None => Got::End,
        }
    }

    /// DoP modunda mı (çıkış 16 bitlik DSD değerleri taşır; kazanç, EQ vb. uygulanamaz).
    fn is_dop(&self) -> bool {
        self.dsd.as_ref().is_some_and(|d| d.dop)
    }

    /// Sıradaki ~birkaç ms'lik sesi çıkış hızında interleaved stereo olarak verir; None = parça bitti.
    fn next_chunk(&mut self) -> Option<Vec<f32>> {
        loop {
            let got = if self.dsd.is_some() { self.dsd_next() } else { self.sym_next() };
            let st = match got {
                Got::End => return self.flush_tail(),
                Got::Skip => continue,
                Got::Data(st) => st,
            };
            let frames = st.len() / 2;
            let out: Vec<f32> = if self.resampler.is_some() {
                self.in_total += frames as u64;
                for f in st.chunks_exact(2) {
                    self.inbuf[0].push(f[0]);
                    self.inbuf[1].push(f[1]);
                }
                match self.drain_resampler() {
                    Some(o) => o,
                    None => return None,
                }
            } else {
                st
            };
            if out.is_empty() {
                continue;
            }
            self.out_frames += (out.len() / 2) as u64;
            return Some(out);
        }
    }

    /// DSD dosyasını açar. `dop`: DoP için (çıkış hızı DSD hızı / 16); aksi halde PCM'e çevrilir ve `out_rate`'e uyarlanır.
    fn open_dsd(path: &Path, out_rate: u32, dop: bool) -> Result<Track> {
        let src = crate::dsd::DsdSource::open(path)?;
        let info = src.info.clone();
        let tags = crate::dsd::read_tags(path, &info);
        let mut meta = TrackMeta { title: tags.title.clone(), album: tags.album.clone(), ..Default::default() };
        meta.artist = if tags.artist.is_empty() { tags.album_artist.clone() } else { tags.artist.clone() };
        if let Some(c) = &tags.cover {
            apply_cover(c, &mut meta);
        }
        if let Some(c) = crate::loudness::lookup(path) {
            meta.rg = Rg { track_gain: c.track_gain, track_peak: c.track_peak, album_gain: c.album_gain, album_peak: c.album_peak };
        }
        let rate = if dop { info.dop_rate() } else { info.pcm_rate() };
        meta.codec = if dop { format!("{} DoP", info.name()) } else { info.name() };
        meta.src_rate = rate;
        meta.bits = 1;
        meta.channels = info.channels as u32;
        meta.resampled = !dop && rate != out_rate;
        let resampler = if !dop && rate != out_rate {
            Some(FftFixedIn::<f32>::new(rate as usize, out_rate as usize, 1024, 2, 2).map_err(|e| anyhow!("{e}"))?)
        } else {
            None
        };
        let delay_left = resampler.as_ref().map(|r| r.output_delay()).unwrap_or(0);
        let conv = if dop { None } else { Some(crate::dsd::Dsd2Pcm::new(&info)) };
        Ok(Track {
            format: None,
            decoder: None,
            dsd: Some(DsdPlay { src, conv, dop }),
            track_id: 0,
            channels: info.channels.max(1),
            rate,
            out_rate: if dop { rate } else { out_rate },
            meta,
            resampler,
            inbuf: [vec![], vec![]],
            out_frames: 0,
            offset: 0.0,
            duration: info.secs(),
            in_total: 0,
            delay_left,
            flushed: false,
            gain_prev: None,
            map: vec![],
            map_ch: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_wav(path: &Path, rate: u32, secs: u32) {
        let n = rate * secs;
        let mut b: Vec<u8> = vec![];
        b.extend(b"RIFF");
        b.extend((36 + n * 4).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(2u16.to_le_bytes());
        b.extend(rate.to_le_bytes());
        b.extend((rate * 4).to_le_bytes());
        b.extend(4u16.to_le_bytes());
        b.extend(16u16.to_le_bytes());
        b.extend(b"data");
        b.extend((n * 4).to_le_bytes());
        for i in 0..n {
            let v = ((i as f64 * 2.0 * std::f64::consts::PI * 1000.0 / rate as f64).sin() * 16000.0) as i16;
            b.extend(v.to_le_bytes());
            b.extend(v.to_le_bytes());
        }
        std::fs::write(path, b).unwrap();
    }

    #[test]
    fn decodes_and_resamples() {
        let dir = std::env::temp_dir();
        for rate in [48000u32, 44100] {
            let p = dir.join(format!("sw_test_{rate}.wav"));
            write_wav(&p, rate, 2);
            let mut t = Track::open(&p, 48000).unwrap();
            assert!((t.duration - 2.0).abs() < 0.01);
            let mut frames = 0usize;
            while let Some(c) = t.next_chunk() {
                frames += c.len() / 2;
            }
            // kuyruk boşaltıldığı için çıkış uzunluğu tam olmalı
            assert!((frames as i64 - 96000).abs() <= 1, "rate {rate}: {frames}");
            // seek
            t.seek(1.0);
            let c = t.next_chunk().unwrap();
            assert!(!c.is_empty());
            assert!(t.pos_secs() >= 1.0);
        }
    }

    fn write_wav_i16(path: &Path, rate: u32, samples_l: &[i16]) {
        let n = samples_l.len() as u32;
        let mut b: Vec<u8> = vec![];
        b.extend(b"RIFF");
        b.extend((36 + n * 4).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(2u16.to_le_bytes());
        b.extend(rate.to_le_bytes());
        b.extend((rate * 4).to_le_bytes());
        b.extend(4u16.to_le_bytes());
        b.extend(16u16.to_le_bytes());
        b.extend(b"data");
        b.extend((n * 4).to_le_bytes());
        for v in samples_l {
            b.extend(v.to_le_bytes());
            b.extend(v.to_le_bytes());
        }
        std::fs::write(path, b).unwrap();
    }

    fn decode_all(path: &Path, out_rate: u32) -> Vec<f32> {
        let mut t = Track::open(path, out_rate).unwrap();
        let mut all = vec![];
        while let Some(c) = t.next_chunk() {
            all.extend(c);
        }
        all
    }

    #[test]
    fn odd_length_output_is_exact() {
        let p = std::env::temp_dir().join("sw_test_odd.wav");
        let n = 44100 + 123; // 1024'ün katı değil
        let s: Vec<i16> = (0..n).map(|i| ((i as f64 * 0.05).sin() * 12000.0) as i16).collect();
        write_wav_i16(&p, 44100, &s);
        let out = decode_all(&p, 48000);
        let expected = (n as f64 * 48000.0 / 44100.0).round() as i64;
        assert!((out.len() as i64 / 2 - expected).abs() <= 1, "{} != {expected}", out.len() / 2);
        // son kareler sessizlik olmamalı (kuyruk atılmadı)
        let tail_peak = out[out.len() - 2 * 400..].iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!(tail_peak > 0.05, "kuyruk kaybolmus: {tail_peak}");
    }

    #[test]
    fn resampler_delay_is_compensated() {
        // 0.5 sn'de tek bir dürtü: yeniden örneklemeden sonra da 0.5 sn'de (24000. kare) olmalı
        let p = std::env::temp_dir().join("sw_test_imp.wav");
        let mut s = vec![0i16; 44100];
        s[22050] = 30000;
        write_wav_i16(&p, 44100, &s);
        let out = decode_all(&p, 48000);
        let (mut idx, mut pk) = (0usize, 0f32);
        for (i, f) in out.chunks_exact(2).enumerate() {
            if f[0].abs() > pk {
                pk = f[0].abs();
                idx = i;
            }
        }
        assert!((idx as i64 - 24000).abs() <= 1, "durtu {idx}. karede (beklenen 24000)");
    }

    fn write_wav_rate(path: &Path, rate: u32, frames: u32) {
        let s: Vec<i16> = (0..frames).map(|i| ((i as f64 * 0.03).sin() * 9000.0) as i16).collect();
        write_wav_i16(path, rate, &s);
    }

    #[test]
    fn rate_follow_requests_restart_and_plays_tail() {
        let dir = std::env::temp_dir();
        let (a, b) = (dir.join("sw_follow_a.wav"), dir.join("sw_follow_b.wav"));
        write_wav_rate(&a, 44100, 44100); // 1 sn
        write_wav_rate(&b, 96000, 96000); // 1 sn
        let p = Player::new();
        p.set_rate(48000);
        p.set_follow(true, vec![44100, 48000, 96000], 48000);
        p.add(vec![a, b]);
        p.play_index(0);

        // motor + arayüz simülasyonu: halkayı tüket, hız isteklerini onayla
        let mut requests: Vec<u32> = vec![];
        let mut phase_frames: Vec<u64> = vec![0];
        let t0 = std::time::Instant::now();
        loop {
            if let Some(r) = p.take_rate_req() {
                requests.push(r);
                phase_frames.push(0);
                p.set_rate(r); // motor yeni hızla yeniden başladı
            }
            let n = {
                let mut q = p.ring.q.lock().unwrap();
                let n = q.len();
                q.clear();
                n
            };
            *phase_frames.last_mut().unwrap() += (n / 2) as u64;
            if !p.playing.load(Ordering::Relaxed) && p.current().is_none() {
                break;
            }
            assert!(t0.elapsed() < Duration::from_secs(15), "zaman asimi: {requests:?} {phase_frames:?}");
            std::thread::sleep(Duration::from_millis(25));
        }
        p.shutdown();
        assert_eq!(requests, vec![44100, 96000], "{phase_frames:?}");
        // her parça kendi hızında, yeniden örneklenmeden ve kuyruğu kesilmeden çalındı
        assert_eq!(phase_frames[0], 0);
        assert_eq!(phase_frames[1], 44100, "{phase_frames:?}");
        assert_eq!(phase_frames[2], 96000, "{phase_frames:?}");
    }

    #[test]
    fn rate_follow_off_never_requests() {
        let a = std::env::temp_dir().join("sw_follow_c.wav");
        write_wav_rate(&a, 44100, 22050);
        let p = Player::new();
        p.set_rate(48000);
        p.add(vec![a]);
        p.play_index(0);
        let t0 = std::time::Instant::now();
        let mut total = 0u64;
        while t0.elapsed() < Duration::from_secs(5) {
            assert!(p.take_rate_req().is_none());
            let n = {
                let mut q = p.ring.q.lock().unwrap();
                let n = q.len();
                q.clear();
                n
            };
            total += (n / 2) as u64;
            if !p.playing.load(Ordering::Relaxed) && p.current().is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        p.shutdown();
        // 22050 kare @44.1k -> 48k: 24000 kare
        assert!((total as i64 - 24000).abs() <= 1, "{total}");
    }

    #[test]
    fn replaygain_parsing() {
        assert_eq!(parse_gain_db("-6.54 dB"), Some(-6.54));
        assert_eq!(parse_gain_db("+3.20dB"), Some(3.2));
        assert_eq!(parse_gain_db(" -0,5 db "), Some(-0.5));
        assert_eq!(parse_gain_db("-7.1"), Some(-7.1));
        assert_eq!(parse_gain_db("abc"), None);
        assert_eq!(parse_peak("0.988553"), Some(0.988553));
        assert_eq!(parse_peak("0"), None);
        assert_eq!(parse_peak("nan"), None);
    }

    #[test]
    fn replaygain_modes_and_clip_prevention() {
        let rg = Rg { track_gain: Some(-6.0), track_peak: Some(0.95), album_gain: Some(-4.0), album_peak: Some(1.0) };
        assert_eq!(rg_gain_db(&rg, 0, 0.0, true), None); // kapalı
        assert_eq!(rg_gain_db(&rg, 1, 0.0, true), Some(-6.0));
        assert_eq!(rg_gain_db(&rg, 2, 0.0, true), Some(-4.0));
        assert_eq!(rg_gain_db(&rg, 1, 2.0, true), Some(-4.0)); // preamp
        // albüm etiketi yoksa parçaya düş
        let t_only = Rg { track_gain: Some(-5.0), ..Default::default() };
        assert_eq!(rg_gain_db(&t_only, 2, 0.0, true), Some(-5.0));
        // etiket yok: dokunma
        assert_eq!(rg_gain_db(&Rg::default(), 1, 3.0, true), None);
        // kırpılma önleme: +6 dB isteniyor ama tepe 0.9 -> en fazla +0.915 dB
        let loud = Rg { track_gain: Some(6.0), track_peak: Some(0.9), ..Default::default() };
        let capped = rg_gain_db(&loud, 1, 0.0, true).unwrap();
        assert!((capped - 0.915).abs() < 0.01, "{capped}");
        assert_eq!(rg_gain_db(&loud, 1, 0.0, false), Some(6.0));
        // tepe 1.0 ve kazanç negatif: sınır etkisiz
        let neg = Rg { track_gain: Some(-3.0), track_peak: Some(1.0), ..Default::default() };
        assert_eq!(rg_gain_db(&neg, 1, 0.0, true), Some(-3.0));
    }

    #[test]
    fn replaygain_off_means_not_applied_even_if_value_is_stale() {
        let p = Player::new();
        p.set_replaygain(1, 0.0, true);
        p.rg_applied.store((-6.0f32).to_bits(), Ordering::Relaxed);
        assert_eq!(p.rg_applied_db(), Some(-6.0));
        p.set_replaygain(0, 0.0, true);
        assert_eq!(p.rg_applied_db(), None, "kapatılınca eski kazanç gösterilmemeli");
    }

    #[test]
    fn apply_gain_ramps_smoothly() {
        let mut b = vec![1.0f32; 2000];
        apply_gain(&mut b, 1.0, 0.5);
        assert!((b[0] - 1.0).abs() < 0.01);
        assert!((b[1998] - 0.5).abs() < 1.0e-3);
        // monoton azalır, sıçrama yok
        for w in b.chunks_exact(2).collect::<Vec<_>>().windows(2) {
            assert!(w[1][0] <= w[0][0] + 1.0e-6);
            assert!((w[0][0] - w[1][0]).abs() < 0.001);
        }
        // sabit kazanç ve 1.0 (hiç dokunma)
        let mut c = vec![0.3f32, -0.3, 0.7, 0.1];
        apply_gain(&mut c, 1.0, 1.0);
        assert_eq!(c, vec![0.3, -0.3, 0.7, 0.1]);
        apply_gain(&mut c, 0.5, 0.5);
        assert_eq!(c, vec![0.15, -0.15, 0.35, 0.05]);
    }

    #[test]
    fn dsd_file_plays_as_pcm_and_as_dop() {
        use crate::dsd::tests::{pack_msb, tone_bits, write_dsf};
        let fs = 2_822_400u32;
        let bytes = pack_msb(&tone_bits(fs as f64, 1000.0, 0.5, 1.0));
        let p = std::env::temp_dir().join("sw_player_dsd.dsf");
        write_dsf(&p, fs, &bytes, &bytes, &[]);
        // PCM'e çevirerek, 48 kHz çıkışa
        let mut t = Track::open(&p, 48000).unwrap();
        assert!((t.duration - 1.0).abs() < 0.01);
        assert_eq!((t.meta.codec.as_str(), t.meta.src_rate, t.meta.resampled, t.rate), ("DSD64", 352_800, true, 352_800));
        let (mut frames, mut peak) = (0usize, 0f32);
        while let Some(c) = t.next_chunk() {
            frames += c.len() / 2;
            peak = c.iter().fold(peak, |m, v| m.max(v.abs()));
        }
        assert!((frames as i64 - 48000).abs() <= 2, "{frames}");
        assert!(peak > 0.45 && peak < 0.6, "tepe {peak}"); // 0.5 genlikli sinüs
        t.seek(0.5);
        assert!(t.next_chunk().is_some());
        assert!(t.pos_secs() >= 0.5);
        // DoP: hız = DSD / 16, her çıkış çifti 16 DSD biti
        let mut d = Track::open_dsd(&p, 48000, true).unwrap();
        assert!(d.is_dop());
        assert_eq!((d.meta.codec.as_str(), d.rate, d.out_rate, d.meta.resampled), ("DSD64 DoP", 176_400, 176_400, false));
        let mut words = vec![];
        while let Some(c) = d.next_chunk() {
            words.extend(c);
        }
        assert_eq!(words.len(), bytes.len()); // 2 bayt/kanal -> 1 çerçeve = 2 değer
        let first = ((bytes[0] as u16) << 8 | bytes[1] as u16) as f32;
        assert_eq!(words[0], first);
        assert!(words.iter().all(|w| *w >= 0.0 && *w <= 65535.0 && w.fract() == 0.0));
    }

    #[test]
    fn dop_mode_switches_with_engine_restart_and_never_mixes_data() {
        use crate::dsd::tests::{pack_msb, tone_bits, write_dsf};
        let dir = std::env::temp_dir();
        let fs = 2_822_400u32;
        let bytes = pack_msb(&tone_bits(fs as f64, 1000.0, 0.5, 0.4));
        let (dsf, wav) = (dir.join("sw_mode_a.dsf"), dir.join("sw_mode_b.wav"));
        write_dsf(&dsf, fs, &bytes, &bytes, &[]);
        write_wav_rate(&wav, 44100, 44100); // 1 sn
        let p = Player::new();
        p.set_rate(48000);
        p.set_follow(true, vec![44100, 48000, 176400], 48000);
        p.set_dop_allowed(true);
        p.set_dop_rates(vec![176400]); // cihaz DSD64'ün DoP hızını (176.4 kHz / 24 bit) kabul ediyor
        p.add(vec![dsf, wav]);
        p.play_index(0);

        // motor + arayüz simülasyonu: istekleri onayla (hız + mod), halkayı tüket ve verinin motor moduna uyduğunu denetle
        let mut requests: Vec<(u32, bool)> = vec![];
        let (mut dop_frames, mut pcm_frames) = (0usize, 0usize);
        let t0 = std::time::Instant::now();
        loop {
            if let Some(r) = p.take_rate_req() {
                requests.push((r, p.want_dop()));
                p.set_rate(r);
                p.set_engine_dop(p.want_dop());
            }
            let dop_engine = p.engine_dop();
            let drained: Vec<f32> = {
                let mut q = p.ring.q.lock().unwrap();
                q.drain(..).collect()
            };
            if dop_engine {
                assert!(drained.iter().all(|v| *v >= 0.0 && *v <= 65535.0 && v.fract() == 0.0), "DoP motoruna PCM verisi geldi");
                dop_frames += drained.len() / 2;
            } else {
                assert!(drained.iter().all(|v| v.abs() <= 1.0), "PCM motoruna DoP verisi geldi");
                pcm_frames += drained.len() / 2;
            }
            if !p.playing.load(Ordering::Relaxed) && p.current().is_none() {
                break;
            }
            assert!(t0.elapsed() < Duration::from_secs(20), "zaman asimi: {requests:?}");
            std::thread::sleep(Duration::from_millis(5));
        }
        p.shutdown();
        assert_eq!(requests, vec![(176400, true), (44100, false)]);
        assert_eq!(dop_frames, bytes.len() / 2, "DoP çerçeve sayısı (2 bayt/kanal = 1 çerçeve)");
        assert!((pcm_frames as i64 - 44100).abs() <= 1, "PCM {pcm_frames}");
    }

    #[test]
    fn rate_change_does_not_wait_for_an_unplayable_tail() {
        // Gerçek motor, halkada 2 değerden (1 çerçeve) azı kalınca tüketmez (iki çerçeve olmadan enterpolasyon yok).
        // Bu artık, hız değişimi bekleme döngüsünün "kuyruk bitsin" şartını 3 sn boyunca takılı bırakmamalı.
        let dir = std::env::temp_dir();
        let (a, b) = (dir.join("sw_tail_a.wav"), dir.join("sw_tail_b.wav"));
        write_wav_rate(&a, 44100, 22050);
        write_wav_rate(&b, 96000, 48000);
        let p = Player::new();
        p.set_rate(44100);
        p.set_follow(true, vec![44100, 96000], 48000);
        p.add(vec![a, b]);
        p.play_index(0);
        let t0 = std::time::Instant::now();
        let mut at_second: Option<std::time::Instant> = None;
        let mut req_after: Option<f64> = None;
        loop {
            if let Some(r) = p.take_rate_req() {
                if at_second.is_some() && req_after.is_none() {
                    req_after = Some(at_second.unwrap().elapsed().as_secs_f64());
                }
                p.set_rate(r);
                p.set_engine_dop(false);
            }
            {
                // gerçek motor gibi: 4 değerden azsa dokunma, aksi halde son çerçeveyi bırakıp tüket
                let mut q = p.ring.q.lock().unwrap();
                if q.len() >= 4 {
                    let keep = 2;
                    let n = q.len() - keep;
                    q.drain(..n);
                }
            }
            if p.current() == Some(1) && at_second.is_none() {
                at_second = Some(std::time::Instant::now());
            }
            if req_after.is_some() || (!p.playing.load(Ordering::Relaxed) && p.current().is_none()) {
                break;
            }
            assert!(t0.elapsed() < Duration::from_secs(12), "zaman asimi");
            std::thread::sleep(Duration::from_millis(5));
        }
        p.shutdown();
        let d = req_after.expect("hız isteği hiç gelmedi");
        assert!(d < 1.0, "hız değişimi {d:.2} sn bekledi (takılı kuyruk)");
    }

    #[test]
    fn dsd_stays_pcm_when_device_reports_no_dop_rates() {
        use crate::dsd::tests::{pack_msb, tone_bits, write_dsf};
        let fs = 2_822_400u32;
        let bytes = pack_msb(&tone_bits(fs as f64, 1000.0, 0.5, 0.2));
        let dsf = std::env::temp_dir().join("sw_nodop.dsf");
        write_dsf(&dsf, fs, &bytes, &bytes, &[]);
        let p = Player::new();
        p.set_rate(48000);
        p.set_follow(true, vec![44100, 48000, 96000], 48000); // en fazla 96 kHz destekleyen cihaz
        p.set_dop_allowed(true); // kullanıcı DoP dese bile
        p.set_dop_rates(vec![]); // ... cihaz hiçbir DoP hızını kabul etmiyor
        p.add(vec![dsf]);
        p.play_index(0);
        let t0 = std::time::Instant::now();
        let mut peak = 0f32;
        loop {
            if let Some(r) = p.take_rate_req() {
                assert!(!p.want_dop(), "DoP istendi");
                p.set_rate(r);
                p.set_engine_dop(false);
            }
            let drained: Vec<f32> = p.ring.q.lock().unwrap().drain(..).collect();
            peak = drained.iter().fold(peak, |m, v| m.max(v.abs()));
            if !p.playing.load(Ordering::Relaxed) && p.current().is_none() {
                break;
            }
            assert!(t0.elapsed() < Duration::from_secs(20), "zaman asimi");
            std::thread::sleep(Duration::from_millis(5));
        }
        p.shutdown();
        assert!(peak > 0.3 && peak <= 1.0, "PCM beklenirdi, tepe {peak}");
        assert!(p.note.lock().unwrap().is_some(), "kullanıcıya PCM'e düşüldüğü bildirilmeli");
    }

    #[test]
    fn dop_needs_device_support_for_that_exact_rate() {
        let dac = [44100, 48000, 96000, 176400, 192000]; // DSD64 (176.4k) olur, DSD128 (352.8k) olmaz
        let rates = [176400u32, 192000];
        assert!(dop_decision(true, true, Some(176400), &rates));
        assert!(!dop_decision(true, true, Some(352800), &rates), "DSD128 için cihaz 352.8 kHz kabul etmiyor");
        // en fazla 96 kHz destekleyen cihaz: liste boş, DoP asla
        assert!(!dop_decision(true, true, Some(176400), &[]));
        let _ = dac;
        // izin, hız eşleştirme ya da DSD dosyası yoksa DoP yok
        assert!(!dop_decision(false, true, Some(176400), &rates));
        assert!(!dop_decision(true, false, Some(176400), &rates));
        assert!(!dop_decision(true, true, None, &rates));
        // 176.4 kHz altı hiçbir koşulda DoP hızı sayılmaz
        assert!(!dop_decision(true, true, Some(96000), &[96000]));
    }

    fn paths(n: usize) -> Vec<PathBuf> {
        (0..n).map(|i| PathBuf::from(format!("t{i}.flac"))).collect()
    }
    fn names(p: &Player) -> Vec<String> {
        p.playlist.lock().unwrap().iter().map(|x| x.to_string_lossy().to_string()).collect()
    }

    #[test]
    fn queue_move_keeps_track_of_current() {
        let p = Player::new();
        p.add(paths(5)); // t0..t4
        p.index.store(2, Ordering::SeqCst); // çalan: t2
        p.move_item(0, 4); // başı sona: çalan geriye kayar
        assert_eq!(names(&p), ["t1.flac", "t2.flac", "t3.flac", "t4.flac", "t0.flac"]);
        assert_eq!(p.current(), Some(1));
        p.move_item(4, 0); // sondakini başa: çalan öne kayar
        assert_eq!(p.current(), Some(2));
        assert_eq!(names(&p)[2], "t2.flac");
        p.move_item(2, 3); // çalanı aşağı taşı: çalan onu izler
        assert_eq!(p.current(), Some(3));
        assert_eq!(names(&p)[3], "t2.flac");
        // çalanın üstünden altına, altından üstüne atlayanlar
        p.move_item(0, 3);
        assert_eq!(names(&p)[p.current().unwrap()], "t2.flac");
        p.move_item(4, 1);
        assert_eq!(names(&p)[p.current().unwrap()], "t2.flac");
        // geçersiz taşıma değiştirmez
        let before = names(&p);
        p.move_item(9, 0);
        p.move_item(0, 9);
        p.move_item(2, 2);
        assert_eq!(names(&p), before);
        p.shutdown();
    }

    #[test]
    fn insert_next_goes_right_after_current() {
        let p = Player::new();
        p.add(paths(3));
        p.index.store(0, Ordering::SeqCst);
        p.insert_next(vec![PathBuf::from("x.flac"), PathBuf::from("y.flac")]);
        assert_eq!(names(&p), ["t0.flac", "x.flac", "y.flac", "t1.flac", "t2.flac"]);
        assert_eq!(p.current(), Some(0));
        // hiçbiri çalmıyorsa sona
        p.index.store(NONE, Ordering::SeqCst);
        p.insert_next(vec![PathBuf::from("z.flac")]);
        assert_eq!(names(&p).last().unwrap(), "z.flac");
        p.shutdown();
    }

    #[test]
    fn load_paused_restores_position_without_playing() {
        let dir = std::env::temp_dir();
        let (a, b) = (dir.join("sw_resume_a.wav"), dir.join("sw_resume_b.wav"));
        write_wav_rate(&a, 48000, 48000 * 2);
        write_wav_rate(&b, 48000, 48000 * 4);
        let p = Player::new();
        p.set_rate(48000);
        p.add(vec![a, b]);
        p.load_paused(1, 1.5);
        // yüklenmesini bekle
        let t0 = std::time::Instant::now();
        while p.duration() == 0.0 && t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(p.current(), Some(1));
        assert!(!p.playing.load(Ordering::Relaxed), "kendiliginden calmaya basladi");
        assert!(p.ring.q.lock().unwrap().is_empty(), "duraklatilmisken ses uretilmemeli");
        assert!((p.position() - 1.5).abs() < 0.05, "konum {}", p.position());
        assert!((p.duration() - 4.0).abs() < 0.05);
        // çal: oradan devam eder
        p.toggle();
        let t1 = std::time::Instant::now();
        while p.ring.q.lock().unwrap().is_empty() && t1.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(p.position() >= 1.49, "{}", p.position());
        p.shutdown();
    }

    #[test]
    fn downmix_5_1_is_normalised_and_drops_lfe() {
        use symphonia::core::audio::Channels as C;
        let ch = C::FRONT_LEFT | C::FRONT_RIGHT | C::FRONT_CENTRE | C::LFE1 | C::REAR_LEFT | C::REAR_RIGHT;
        let m = downmix_map(ch);
        assert_eq!(m.len(), 6);
        let g = 1.0 / (1.0 + 0.7071 + 0.7071);
        assert!((m[0].0 - g).abs() < 1e-3 && m[0].1 == 0.0);
        assert!((m[1].1 - g).abs() < 1e-3 && m[1].0 == 0.0);
        assert!((m[2].0 - 0.7071 * g).abs() < 1e-3 && (m[2].1 - 0.7071 * g).abs() < 1e-3);
        assert_eq!(m[3], (0.0, 0.0));
        assert!((m[4].0 - 0.7071 * g).abs() < 1e-3 && m[4].1 == 0.0);
        // tüm kanallar tam ölçekteyken bile çıkış 1.0'ı aşmaz
        let (l, r): (f32, f32) = (m.iter().map(|x| x.0).sum(), m.iter().map(|x| x.1).sum());
        assert!(l <= 1.0001 && r <= 1.0001);
        // 3.0 (L R C): yine normalize
        let m3 = downmix_map(C::FRONT_LEFT | C::FRONT_RIGHT | C::FRONT_CENTRE);
        let s3: f32 = m3.iter().map(|x| x.0).sum();
        assert!(s3 <= 1.0001);
    }
}


fn chans_count(c: Option<usize>) -> u32 {
    c.unwrap_or(2) as u32
}

fn read_meta(r: &MetadataRevision, m: &mut TrackMeta) {
    for tag in r.tags() {
        let key = tag.key.to_uppercase();
        let val = tag.value.to_string();
        match key.as_str() {
            "REPLAYGAIN_TRACK_GAIN" => m.rg.track_gain = m.rg.track_gain.or(parse_gain_db(&val)),
            "REPLAYGAIN_TRACK_PEAK" => m.rg.track_peak = m.rg.track_peak.or(parse_peak(&val)),
            "REPLAYGAIN_ALBUM_GAIN" => m.rg.album_gain = m.rg.album_gain.or(parse_gain_db(&val)),
            "REPLAYGAIN_ALBUM_PEAK" => m.rg.album_peak = m.rg.album_peak.or(parse_peak(&val)),
            // Opus R128: Q7.8 dB, -23 LUFS'e göre; ReplayGain referansı (-18 LUFS) için +5 dB
            "R128_TRACK_GAIN" if m.rg.track_gain.is_none() => m.rg.track_gain = val.trim().parse::<f32>().ok().map(|v| v / 256.0 + 5.0),
            "R128_ALBUM_GAIN" if m.rg.album_gain.is_none() => m.rg.album_gain = val.trim().parse::<f32>().ok().map(|v| v / 256.0 + 5.0),
            _ => {}
        }
        match tag.std_key {
            Some(StandardTagKey::ReplayGainTrackGain) if m.rg.track_gain.is_none() => m.rg.track_gain = parse_gain_db(&val),
            Some(StandardTagKey::ReplayGainTrackPeak) if m.rg.track_peak.is_none() => m.rg.track_peak = parse_peak(&val),
            Some(StandardTagKey::ReplayGainAlbumGain) if m.rg.album_gain.is_none() => m.rg.album_gain = parse_gain_db(&val),
            Some(StandardTagKey::ReplayGainAlbumPeak) if m.rg.album_peak.is_none() => m.rg.album_peak = parse_peak(&val),
            _ => {}
        }
        match tag.std_key {
            Some(StandardTagKey::TrackTitle) if m.title.is_empty() => m.title = tag.value.to_string(),
            Some(StandardTagKey::Artist) if m.artist.is_empty() => m.artist = tag.value.to_string(),
            Some(StandardTagKey::AlbumArtist) if m.artist.is_empty() => m.artist = tag.value.to_string(),
            Some(StandardTagKey::Album) if m.album.is_empty() => m.album = tag.value.to_string(),
            _ => {}
        }
    }
    if m.cover.is_none() {
        if let Some(v) = r.visuals().first() {
            apply_cover(&v.data, m);
        }
    }
}


/// Gömülü kapak resmini küçültüp `TrackMeta`'ya koyar ve baskın rengi hesaplar.
fn apply_cover(data: &[u8], m: &mut TrackMeta) {
    if let Ok(img) = image::load_from_memory(data) {
        let img = img.thumbnail(420, 420).to_rgba8();
        let (w, h) = img.dimensions();
        // baskın renk: doygunluk ağırlıklı ortalama
        let (mut r_, mut g_, mut b_, mut wsum) = (0f64, 0f64, 0f64, 0f64);
        for px in img.pixels().step_by(7) {
            let (r, g, b) = (px[0] as f64, px[1] as f64, px[2] as f64);
            let mx = r.max(g).max(b);
            let mn = r.min(g).min(b);
            let wt = (mx - mn) / 255.0 + 0.05;
            r_ += r * wt;
            g_ += g * wt;
            b_ += b * wt;
            wsum += wt;
        }
        if wsum > 0.0 {
            m.tint = [(r_ / wsum) as u8, (g_ / wsum) as u8, (b_ / wsum) as u8];
        }
        m.cover = Some((w, h, img.into_raw()));
    }
}
