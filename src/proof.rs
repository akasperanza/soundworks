//! Bit-perfect kanıtı. Motor, cihaza yazdığı her çerçevede "yazılan değer, halkadan gelen örnekle birebir aynı mı?"
//! diye ölçer (ayar kontrolü değil, ölçüm). Kanıt yazılımdan cihaz tamponuna kadar olan yolu kapsar; sürücü ve DAC içi dışarıda kalır.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};

/// Bir çerçevenin değişmesinin nedenleri (bit maskesi).
pub const DRIFT: u32 = 1; // kesirli konumda ara değer (hız farkı / saat kayması telafisi)
pub const GAIN: u32 = 2; // ses seviyesi ya da dengeleme ≠ %100
pub const DSP: u32 = 4; // EQ, modüller, crossfeed, mono
pub const DITHER: u32 = 8;
pub const CLIP: u32 = 16; // tam ölçeği aştı, kırpıldı
pub const OTHER: u32 = 32;

/// Motorun ses iş parçacığında yazdığı sayaçlar; arayüz farkları okur.
#[derive(Default)]
pub struct Proof {
    /// halkadaki örnekle birebir aynı yazılan çerçeve
    pub ok: AtomicU64,
    /// yumuşak giriş/çıkış sırasındaki çerçeve (başlatma, duraklatma, ses geçişi): ölçüm dışı
    pub ramp: AtomicU64,
    /// değiştirilerek yazılan çerçeve
    pub alt: AtomicU64,
    /// halka boşaldı (tampon yetersizliği)
    pub dry: AtomicU64,
    /// görülen değişiklik nedenleri
    pub causes: AtomicU32,
}

/// Bir ses tamponu boyunca yerelde biriken sayaçlar (atomiklere tek seferde eklenir).
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub struct Counters {
    pub ok: u64,
    pub ramp: u64,
    pub alt: u64,
    pub dry: u64,
    pub causes: u32,
}

impl Proof {
    pub fn add(&self, c: &Counters) {
        if c.ok > 0 {
            self.ok.fetch_add(c.ok, Relaxed);
        }
        if c.ramp > 0 {
            self.ramp.fetch_add(c.ramp, Relaxed);
        }
        if c.alt > 0 {
            self.alt.fetch_add(c.alt, Relaxed);
        }
        if c.dry > 0 {
            self.dry.fetch_add(c.dry, Relaxed);
        }
        if c.causes != 0 {
            self.causes.fetch_or(c.causes, Relaxed);
        }
    }
    pub fn read(&self) -> Counters {
        Counters { ok: self.ok.load(Relaxed), ramp: self.ramp.load(Relaxed), alt: self.alt.load(Relaxed), dry: self.dry.load(Relaxed), causes: self.causes.load(Relaxed) }
    }
}

/// Çerçeve birebir mi: halkadan gelen örnek (kesirli konum yok) hiçbir işleme uğramadan, dither'sız ve kırpılmadan yazılıyor.
/// (`==` kullanılır: +0.0 ile -0.0 aynı tam sayıya dönüşür.)
pub fn frame_exact(in_l: f32, in_r: f32, out_l: f32, out_r: f32, on_grid: bool, dithered: bool) -> bool {
    on_grid && !dithered && in_l == out_l && in_r == out_r && out_l.abs() <= 1.0 && out_r.abs() <= 1.0
}

/// Değişen bir çerçevenin nedenleri.
pub fn causes(on_grid: bool, gain_off: bool, dsp: bool, dithered: bool, clipped: bool) -> u32 {
    let mut c = 0;
    if !on_grid {
        c |= DRIFT;
    }
    if gain_off {
        c |= GAIN;
    }
    if dsp {
        c |= DSP;
    }
    if dithered {
        c |= DITHER;
    }
    if clipped {
        c |= CLIP;
    }
    if c == 0 {
        c = OTHER;
    }
    c
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Light {
    /// çalmıyor / motor kapalı
    Idle,
    /// ölçüldü: birebir
    Perfect,
    /// bilinen bir nedenle değiştiriliyor
    Altered,
    /// kesinti (tampon boşaldı)
    Glitch,
}

pub struct LightIn {
    pub playing: bool,
    pub dop: bool,
    pub loopback: bool,
    pub exclusive: bool,
    /// kaynak çıkış hızına dönüştürülüyor
    pub resampled: bool,
    /// ReplayGain uygulanıyor
    pub rg: bool,
    /// son saniyede değiştirilmiş çerçeve görüldü
    pub recent_alt: bool,
    /// bu parçada görülen kesinti
    pub dry: u64,
}

pub fn light(i: &LightIn) -> Light {
    if !i.playing {
        return Light::Idle;
    }
    if i.dry > 0 {
        return Light::Glitch;
    }
    if i.dop {
        return Light::Perfect; // DoP'ta DSD verisi hiçbir işleme uğramadan paketlenir
    }
    if i.loopback || !i.exclusive || i.resampled || i.rg || i.recent_alt {
        return Light::Altered;
    }
    Light::Perfect
}

/// Binlik ayraçlı sayı ("41.280.000").
pub fn group(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push('.');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_frame_is_exact_and_any_change_is_caught() {
        assert!(frame_exact(0.25, -0.5, 0.25, -0.5, true, false));
        assert!(frame_exact(0.0, 0.0, -0.0, 0.0, true, false), "+0.0 ve -0.0 aynı tam sayıdır");
        // tek bir en küçük adımlık fark bile yakalanır
        assert!(!frame_exact(0.25, -0.5, f32::from_bits(0.25f32.to_bits() + 1), -0.5, true, false));
        assert!(!frame_exact(0.25, -0.5, 0.25, -0.5, false, false), "ara değerli çerçeve birebir değildir");
        assert!(!frame_exact(0.25, -0.5, 0.25, -0.5, true, true), "dither birebir değildir");
        assert!(!frame_exact(1.5, 0.0, 1.5, 0.0, true, false), "kırpılan çerçeve birebir değildir");
    }

    #[test]
    fn causes_name_the_reason() {
        assert_eq!(causes(true, true, false, false, false), GAIN);
        assert_eq!(causes(false, false, true, false, false), DRIFT | DSP);
        assert_eq!(causes(true, false, false, false, false), OTHER);
    }

    #[test]
    fn light_follows_the_evidence() {
        let base = LightIn { playing: true, dop: false, loopback: false, exclusive: true, resampled: false, rg: false, recent_alt: false, dry: 0 };
        assert_eq!(light(&base), Light::Perfect);
        assert_eq!(light(&LightIn { playing: false, ..base }), Light::Idle);
        assert_eq!(light(&LightIn { exclusive: false, ..base }), Light::Altered);
        assert_eq!(light(&LightIn { resampled: true, ..base }), Light::Altered);
        assert_eq!(light(&LightIn { rg: true, ..base }), Light::Altered);
        assert_eq!(light(&LightIn { recent_alt: true, ..base }), Light::Altered);
        assert_eq!(light(&LightIn { loopback: true, ..base }), Light::Altered);
        assert_eq!(light(&LightIn { dry: 2, ..base }), Light::Glitch, "kesinti her şeyden önce gelir");
        assert_eq!(light(&LightIn { dop: true, exclusive: true, ..base }), Light::Perfect);
    }

    #[test]
    fn counters_accumulate() {
        let p = Proof::default();
        p.add(&Counters { ok: 10, ramp: 2, alt: 0, dry: 0, causes: 0 });
        p.add(&Counters { ok: 5, ramp: 0, alt: 3, dry: 1, causes: GAIN });
        let c = p.read();
        assert_eq!((c.ok, c.ramp, c.alt, c.dry, c.causes), (15, 2, 3, 1, GAIN));
    }

    #[test]
    fn thousands_grouping() {
        assert_eq!(group(0), "0");
        assert_eq!(group(999), "999");
        assert_eq!(group(1000), "1.000");
        assert_eq!(group(41_280_000), "41.280.000");
    }
}
