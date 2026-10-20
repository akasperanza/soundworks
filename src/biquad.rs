//! RBJ audio-EQ-cookbook biquad'ları (Equalizer APO ile aynı formüller).

use crate::apo::{Band, FilterKind, Preset};
use std::f64::consts::PI;

#[derive(Debug, Clone, Copy)]
pub struct Coeffs {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl Coeffs {
    pub const IDENTITY: Coeffs = Coeffs { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };

    pub fn design(band: &Band, fs: f64) -> Coeffs {
        let f0 = band.freq.clamp(1.0, fs * 0.499);
        let q = band.q.max(1e-3);
        let a = 10f64.powf(band.gain_db / 40.0);
        let w0 = 2.0 * PI * f0 / fs;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);

        let (b0, b1, b2, a0, a1, a2) = match band.kind {
            FilterKind::Peak => (
                1.0 + alpha * a,
                -2.0 * cos,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos,
                1.0 - alpha / a,
            ),
            FilterKind::LowShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cos + s),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                    a * ((a + 1.0) - (a - 1.0) * cos - s),
                    (a + 1.0) + (a - 1.0) * cos + s,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                    (a + 1.0) + (a - 1.0) * cos - s,
                )
            }
            FilterKind::HighShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cos + s),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                    a * ((a + 1.0) + (a - 1.0) * cos - s),
                    (a + 1.0) - (a - 1.0) * cos + s,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos),
                    (a + 1.0) - (a - 1.0) * cos - s,
                )
            }
            FilterKind::LowPass => (
                (1.0 - cos) / 2.0,
                1.0 - cos,
                (1.0 - cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            FilterKind::HighPass => (
                (1.0 + cos) / 2.0,
                -(1.0 + cos),
                (1.0 + cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            FilterKind::BandPass => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            FilterKind::Notch => (1.0, -2.0 * cos, 1.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            FilterKind::AllPass => (
                1.0 - alpha,
                -2.0 * cos,
                1.0 + alpha,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
        };
        Coeffs { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
    }

    /// |H(e^jw)| dB cinsinden.
    pub fn magnitude_db(&self, f: f64, fs: f64) -> f64 {
        let w = 2.0 * PI * f / fs;
        let (c1, s1) = (w.cos(), w.sin());
        let (c2, s2) = ((2.0 * w).cos(), (2.0 * w).sin());
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = -(self.b1 * s1 + self.b2 * s2);
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = -(self.a1 * s1 + self.a2 * s2);
        10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).log10()
    }
}

#[derive(Clone, Copy, Default)]
struct State {
    z1: f64,
    z2: f64,
}

/// Katsayı geçiş süresi: EQ düğümü sürüklenirken tık/zipper olmasın.
const RAMP_SECS: f64 = 0.02;

/// Çok kanallı cascade EQ (transposed direct form II).
/// `retarget` ile bant sayısı değişmediği sürece filtre durumu korunur ve katsayılar yumuşakça yeni değere kayar.
pub struct Equalizer {
    coeffs: Vec<Coeffs>,
    target: Vec<Coeffs>,
    delta: Vec<[f64; 5]>,
    ramp_left: u32,
    state: Vec<Vec<State>>, // [kanal][bant]
    preamp: f64,
    preamp_target: f64,
    preamp_delta: f64,
}

fn design_all(preset: &Preset, fs: f64) -> Vec<Coeffs> {
    preset.bands.iter().filter(|b| b.freq < fs * 0.5).map(|b| Coeffs::design(b, fs)).collect()
}

impl Equalizer {
    pub fn new(preset: &Preset, fs: f64, channels: usize) -> Self {
        let coeffs = design_all(preset, fs);
        let state = vec![vec![State::default(); coeffs.len()]; channels];
        let preamp = 10f64.powf(preset.preamp_db / 20.0);
        Equalizer {
            target: coeffs.clone(),
            delta: vec![[0.0; 5]; coeffs.len()],
            coeffs,
            ramp_left: 0,
            state,
            preamp,
            preamp_target: preamp,
            preamp_delta: 0.0,
        }
    }

    /// Aynı bant sayısıyla yeni değerlere yumuşak geçiş. Bant sayısı değiştiyse `false` döner (çağıran yeniden kurar).
    pub fn retarget(&mut self, preset: &Preset, fs: f64) -> bool {
        let tgt = design_all(preset, fs);
        if tgt.len() != self.coeffs.len() {
            return false;
        }
        let n = ((fs * RAMP_SECS) as u32).max(1);
        let nf = n as f64;
        for (i, t) in tgt.iter().enumerate() {
            let c = &self.coeffs[i];
            self.delta[i] = [(t.b0 - c.b0) / nf, (t.b1 - c.b1) / nf, (t.b2 - c.b2) / nf, (t.a1 - c.a1) / nf, (t.a2 - c.a2) / nf];
        }
        self.target = tgt;
        self.preamp_target = 10f64.powf(preset.preamp_db / 20.0);
        self.preamp_delta = (self.preamp_target - self.preamp) / nf;
        self.ramp_left = n;
        true
    }

    /// Hedef (geçiş bittiğindeki) preamp kazancı.
    pub fn preamp_gain(&self) -> f32 {
        self.preamp_target as f32
    }

    /// Hiçbir işlem yapmıyor mu (bant yok, preamp 1)?
    pub fn is_identity(&self) -> bool {
        self.coeffs.is_empty() && (self.preamp_target - 1.0).abs() < 1e-12 && self.ramp_left == 0
    }

    pub fn process_frame(&mut self, l: f32, r: f32) -> (f32, f32) {
        let mut f = [l, r];
        self.process(&mut f, 2);
        (f[0], f[1])
    }

    fn step_ramp(&mut self) {
        self.ramp_left -= 1;
        if self.ramp_left == 0 {
            self.coeffs.clone_from(&self.target);
            self.preamp = self.preamp_target;
            return;
        }
        for (c, d) in self.coeffs.iter_mut().zip(self.delta.iter()) {
            c.b0 += d[0];
            c.b1 += d[1];
            c.b2 += d[2];
            c.a1 += d[3];
            c.a2 += d[4];
        }
        self.preamp += self.preamp_delta;
    }

    /// Interleaved f32 buffer'ı yerinde işler.
    pub fn process(&mut self, buf: &mut [f32], channels: usize) {
        for frame in buf.chunks_exact_mut(channels) {
            if self.ramp_left > 0 {
                self.step_ramp();
            }
            for (ch, s) in frame.iter_mut().enumerate() {
                let mut x = *s as f64 * self.preamp;
                for (c, st) in self.coeffs.iter().zip(self.state[ch].iter_mut()) {
                    let y = c.b0 * x + st.z1;
                    st.z1 = c.b1 * x - c.a1 * y + st.z2;
                    st.z2 = c.b2 * x - c.a2 * y;
                    x = y;
                }
                *s = x as f32;
            }
        }
    }
}

/// Band solo: yalnızca seçilen frekans bölgesini geçirir (sabit 0 dB tepeli bant geçiren, RBJ).
/// Bir enstrümanın hangi frekansta olduğunu duyarak bulmak içindir.
pub struct BandSolo {
    coeffs: Coeffs,
    z: [State; 2],
}

impl BandSolo {
    pub fn new() -> BandSolo {
        BandSolo { coeffs: Coeffs::IDENTITY, z: [State::default(); 2] }
    }

    pub fn set(&mut self, freq: f64, q: f64, fs: f64) {
        let band = Band { kind: FilterKind::BandPass, freq, gain_db: 0.0, q: q.clamp(0.3, 30.0) };
        self.coeffs = Coeffs::design(&band, fs);
    }

    pub fn reset(&mut self) {
        self.z = [State::default(); 2];
    }

    /// `ch`: 0 sol, 1 sağ.
    pub fn process(&mut self, ch: usize, x: f32) -> f32 {
        let c = &self.coeffs;
        let st = &mut self.z[ch & 1];
        let x = x as f64;
        let y = c.b0 * x + st.z1;
        st.z1 = c.b1 * x - c.a1 * y + st.z2;
        st.z2 = c.b2 * x - c.a2 * y;
        y as f32
    }
}

/// Frekansa en yakın nota adı ve sapma (sent): 440 Hz -> ("A4", 0).
pub fn note_name(freq: f64) -> (String, i32) {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    if freq <= 0.0 {
        return ("-".into(), 0);
    }
    let midi = 69.0 + 12.0 * (freq / 440.0).log2();
    let n = midi.round();
    let cents = ((midi - n) * 100.0).round() as i32;
    let idx = (n as i64).rem_euclid(12) as usize;
    let octave = (n as i64).div_euclid(12) - 1;
    (format!("{}{}", NAMES[idx], octave), cents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apo;

    #[test]
    fn peak_gain_at_center() {
        let b = Band { kind: FilterKind::Peak, freq: 1000.0, gain_db: 6.0, q: 1.0 };
        let c = Coeffs::design(&b, 48000.0);
        assert!((c.magnitude_db(1000.0, 48000.0) - 6.0).abs() < 0.01);
        assert!(c.magnitude_db(20.0, 48000.0).abs() < 0.1);
    }

    #[test]
    fn shelves_reach_gain() {
        let ls = Coeffs::design(&Band { kind: FilterKind::LowShelf, freq: 105.0, gain_db: -6.8, q: 0.7 }, 48000.0);
        assert!((ls.magnitude_db(10.0, 48000.0) + 6.8).abs() < 0.1);
        let hs = Coeffs::design(&Band { kind: FilterKind::HighShelf, freq: 10000.0, gain_db: 3.0, q: 0.7 }, 48000.0);
        assert!((hs.magnitude_db(23000.0, 48000.0) - 3.0).abs() < 0.5);
    }

    #[test]
    fn impulse_matches_magnitude() {
        let p = apo::parse("Preamp: 0 dB\nFilter 1: ON PK Fc 2000 Hz Gain 5 dB Q 2\n");
        let mut eq = Equalizer::new(&p, 48000.0, 1);
        let f = 2000.0f64;
        let n = 48000;
        let mut buf: Vec<f32> = (0..n).map(|i| (2.0 * PI * f * i as f64 / 48000.0).sin() as f32).collect();
        eq.process(&mut buf, 1);
        let peak = buf[n / 2..].iter().fold(0f32, |m, v| m.max(v.abs())) as f64;
        assert!((20.0 * peak.log10() - 5.0).abs() < 0.05);
    }

    fn sine(n: usize, f: f64, amp: f64) -> Vec<f32> {
        (0..n).map(|i| (amp * (2.0 * PI * f * i as f64 / 48000.0).sin()) as f32).collect()
    }

    #[test]
    fn retarget_is_click_free_and_converges() {
        let p0 = apo::parse("Preamp: 0 dB\nFilter 1: ON PK Fc 1000 Hz Gain 0 dB Q 1\n");
        let p1 = apo::parse("Preamp: 0 dB\nFilter 1: ON PK Fc 1000 Hz Gain 3 dB Q 1\n");
        let mut eq = Equalizer::new(&p0, 48000.0, 1);
        let mut buf = sine(6000, 1000.0, 0.5);
        eq.process(&mut buf[..3000], 1);
        assert!(eq.retarget(&p1, 48000.0));
        eq.process(&mut buf[3000..], 1);
        // geçiş sırasında örnekten örneğe sıçrama, sinüsün teorik eğimini aşmamalı (aşım = tık)
        let max_step = buf[3000..].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0f32, f32::max);
        assert!(max_step < 0.10, "tik: {max_step}");
        // geçiş bittikten sonra +3 dB
        let peak = buf[5000..].iter().fold(0f32, |m, v| m.max(v.abs())) as f64;
        assert!((20.0 * (peak / 0.5).log10() - 3.0).abs() < 0.1);
    }

    #[test]
    fn retarget_rejects_band_count_change() {
        let p0 = apo::parse("Preamp: 0 dB\nFilter 1: ON PK Fc 1000 Hz Gain 0 dB Q 1\n");
        let p1 = apo::parse("Preamp: 0 dB\nFilter 1: ON PK Fc 1000 Hz Gain 3 dB Q 1\nFilter 2: ON PK Fc 2000 Hz Gain 3 dB Q 1\n");
        let mut eq = Equalizer::new(&p0, 48000.0, 1);
        assert!(!eq.retarget(&p1, 48000.0));
    }

    #[test]
    fn identity_when_empty() {
        let p = apo::parse("Preamp: 0 dB\n");
        let mut eq = Equalizer::new(&p, 48000.0, 2);
        assert!(eq.is_identity());
        let mut buf = vec![0.1234567f32, -0.7654321];
        let orig = buf.clone();
        eq.process(&mut buf, 2);
        assert_eq!(buf, orig);
    }

    fn gain_at(solo: &mut BandSolo, f: f64) -> f64 {
        solo.reset();
        let n = 48000;
        let mut peak = 0f32;
        for i in 0..n {
            let x = (2.0 * PI * f * i as f64 / 48000.0).sin() as f32;
            let y = solo.process(0, x);
            if i > n / 2 {
                peak = peak.max(y.abs());
            }
        }
        20.0 * (peak as f64).log10()
    }

    #[test]
    fn band_solo_passes_only_the_selected_region() {
        let mut s = BandSolo::new();
        s.set(1000.0, 4.0, 48000.0);
        assert!(gain_at(&mut s, 1000.0).abs() < 0.1, "merkez 0 dB olmali");
        assert!(gain_at(&mut s, 250.0) < -12.0, "2 oktav asagi bastirilmali");
        assert!(gain_at(&mut s, 4000.0) < -12.0, "2 oktav yukari bastirilmali");
        assert!(gain_at(&mut s, 100.0) < -25.0);
        // dar Q daha seçici
        let mut narrow = BandSolo::new();
        narrow.set(1000.0, 12.0, 48000.0);
        assert!(gain_at(&mut narrow, 800.0) < gain_at(&mut s, 800.0) - 6.0);
        // kanallar bağımsız
        let mut t = BandSolo::new();
        t.set(1000.0, 4.0, 48000.0);
        for i in 0..2000 {
            let x = (2.0 * PI * 1000.0 * i as f64 / 48000.0).sin() as f32;
            t.process(0, x);
            assert_eq!(t.process(1, 0.0), 0.0); // sağ kanalda sessizlik sızmaz
        }
    }

    #[test]
    fn note_names() {
        assert_eq!(note_name(440.0), ("A4".to_string(), 0));
        assert_eq!(note_name(880.0), ("A5".to_string(), 0));
        assert_eq!(note_name(261.6256), ("C4".to_string(), 0));
        assert_eq!(note_name(27.5), ("A0".to_string(), 0));
        let (n, c) = note_name(450.0); // 1200*log2(450/440) = +38.9 sent
        assert_eq!(n, "A4");
        assert_eq!(c, 39);
        let (n2, c2) = note_name(430.0); // -39.5 sent
        assert_eq!(n2, "A4");
        assert!((c2 + 39).abs() <= 1, "{c2}");
        assert_eq!(note_name(466.1638), ("A#4".to_string(), 0));
    }
}
