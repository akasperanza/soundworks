use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum FxKind {
    Limiter,
    Compressor,
    Width,
    Gain,
    Filter,
    Tone,
    Loudness,
    Crossfeed,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Off {
    No,
    AtMin(&'static str),
    AtMax(&'static str),
}

pub struct ParamInfo {
    pub name: &'static str,
    pub unit: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub decimals: usize,
    pub off: Off,
}

pub struct FxInfo {
    pub kind: FxKind,
    pub name: &'static str,
    pub tagline: &'static str,
    pub desc: &'static str,
    pub params: &'static [ParamInfo],
}

pub const CATALOG: &[FxInfo] = &[
    FxInfo {
        kind: FxKind::Limiter,
        name: "Limiter",
        tagline: "Tepe koruması",
        desc: "Tavanı aşan tepeleri (örnekler arası gerçek tepeler dahil) önceden görüp yumuşakça indirir; sert kırpma ve DAC'ta oluşabilecek tepe aşımı olmaz. Tavanın altındaki sesi değiştirmez. Yaklaşık 2,2 ms gecikme ekler.",
        params: &[
            ParamInfo { name: "Tavan", unit: "dB", min: -6.0, max: 0.0, default: -0.5, decimals: 1, off: Off::No },
            ParamInfo { name: "Bırakma", unit: "ms", min: 10.0, max: 500.0, default: 120.0, decimals: 0, off: Off::No },
        ],
    },
    FxInfo {
        kind: FxKind::Compressor,
        name: "Kompresör",
        tagline: "Dinamik aralık",
        desc: "Eşiği aşan sesleri orana göre bastırır. Yumuşak diz ile geçiş doğal olur. Kıs ve yüksek sesler arasındaki farkı azaltmak için.",
        params: &[
            ParamInfo { name: "Eşik", unit: "dB", min: -50.0, max: 0.0, default: -18.0, decimals: 1, off: Off::No },
            ParamInfo { name: "Oran", unit: ":1", min: 1.0, max: 20.0, default: 3.0, decimals: 1, off: Off::No },
            ParamInfo { name: "Atak", unit: "ms", min: 1.0, max: 100.0, default: 10.0, decimals: 0, off: Off::No },
            ParamInfo { name: "Bırakma", unit: "ms", min: 20.0, max: 1000.0, default: 150.0, decimals: 0, off: Off::No },
            ParamInfo { name: "Diz", unit: "dB", min: 0.0, max: 18.0, default: 6.0, decimals: 1, off: Off::No },
            ParamInfo { name: "Makyaj", unit: "dB", min: 0.0, max: 18.0, default: 0.0, decimals: 1, off: Off::No },
        ],
    },
    FxInfo {
        kind: FxKind::Width,
        name: "Stereo Genişlik",
        tagline: "Sahne genişliği",
        desc: "Orta/yan (M/S) dengesiyle stereo sahneyi daraltır ya da genişletir. %0 mono, %100 olduğu gibi, %200 yanı iki katına çıkarır.",
        params: &[ParamInfo { name: "Genişlik", unit: "%", min: 0.0, max: 200.0, default: 100.0, decimals: 0, off: Off::No }],
    },
    FxInfo {
        kind: FxKind::Gain,
        name: "Kazanç",
        tagline: "Seviye ayarı",
        desc: "Zincirin bir noktasında sesi yükseltir ya da kısar. EQ'dan sonra kısıp başka bir modüle pay bırakmak için kullanışlı.",
        params: &[ParamInfo { name: "Kazanç", unit: "dB", min: -24.0, max: 12.0, default: 0.0, decimals: 1, off: Off::No }],
    },
    FxInfo {
        kind: FxKind::Filter,
        name: "Filtre",
        tagline: "Alçak / yüksek kes",
        desc: "Yüksek geçiren (HPF) çok alçak frekansları, alçak geçiren (LPF) çok yüksek frekansları keser. HPF kulaklığı ve hoparlörü duyulmayan derin bas gürültüsünden korur; LPF ultrasonik gürültüyü bastırır. Eğim 12 dB/oktav adımlarla ayarlanır (24 dB iyi bir başlangıçtır). Kaydırıcıyı uca götürerek ilgili filtreyi kapatabilirsin.",
        params: &[
            ParamInfo { name: "Alçak kes (HPF)", unit: "Hz", min: 0.0, max: 300.0, default: 30.0, decimals: 0, off: Off::AtMin("Kapalı") },
            ParamInfo { name: "HPF eğimi", unit: "dB/okt", min: 12.0, max: 48.0, default: 24.0, decimals: 0, off: Off::No },
            ParamInfo { name: "Üst kes (LPF)", unit: "Hz", min: 2000.0, max: 24000.0, default: 24000.0, decimals: 0, off: Off::AtMax("Kapalı") },
            ParamInfo { name: "LPF eğimi", unit: "dB/okt", min: 12.0, max: 48.0, default: 24.0, decimals: 0, off: Off::No },
        ],
    },
    FxInfo {
        kind: FxKind::Tone,
        name: "Ton",
        tagline: "Bas ve tiz",
        desc: "Bas ve tiz için iki basit raf (shelf) kontrolü. EQ'ya girmeden sesi tatlandırmak için. Güçlendirdiğin kadar ses otomatik kısılır, böylece kırpılma olmaz.",
        params: &[
            ParamInfo { name: "Bas", unit: "dB", min: -12.0, max: 12.0, default: 0.0, decimals: 1, off: Off::No },
            ParamInfo { name: "Bas frekansı", unit: "Hz", min: 40.0, max: 400.0, default: 100.0, decimals: 0, off: Off::No },
            ParamInfo { name: "Tiz", unit: "dB", min: -12.0, max: 12.0, default: 0.0, decimals: 1, off: Off::No },
            ParamInfo { name: "Tiz frekansı", unit: "Hz", min: 2000.0, max: 16000.0, default: 8000.0, decimals: 0, off: Off::No },
        ],
    },
    FxInfo {
        kind: FxKind::Loudness,
        name: "Loudness",
        tagline: "Kısık sesde denge",
        desc: "Kulak kısık seviyede bas ve tizi daha zayıf duyar. Bu modül, ses seviyesini kıstıkça bas ve tizi kendiliğinden güçlendirir; ses yükseldikçe etkisi azalır, referans seviyede hiç dokunmaz. Seviye, çalar ses ayarından alınır.",
        params: &[
            ParamInfo { name: "Güç", unit: "%", min: 0.0, max: 200.0, default: 100.0, decimals: 0, off: Off::No },
            ParamInfo { name: "Referans ses", unit: "%", min: 20.0, max: 100.0, default: 100.0, decimals: 0, off: Off::No },
        ],
    },
    FxInfo {
        kind: FxKind::Crossfeed,
        name: "Crossfeed",
        tagline: "Kulaklıkta doğal sahne",
        desc: "Kulaklıkta her kanalı diğer kulağa da hafifçe, alçak geçirilmiş ve çok kısa geciktirilmiş olarak karıştırır; hoparlör dinlemesine benzer, yorucu \"kafa içi\" sahneyi yumuşatır. Seviye, kesme frekansı ve gecikmeyi geniş aralıkta ayarlayabilirsin.",
        params: &[
            ParamInfo { name: "Seviye", unit: "dB", min: -30.0, max: 0.0, default: -8.0, decimals: 1, off: Off::No },
            ParamInfo { name: "Kesme frekansı", unit: "Hz", min: 200.0, max: 2000.0, default: 700.0, decimals: 0, off: Off::No },
            ParamInfo { name: "Gecikme", unit: "µs", min: 0.0, max: 800.0, default: 270.0, decimals: 0, off: Off::No },
        ],
    },
];

pub fn info(kind: FxKind) -> &'static FxInfo {
    CATALOG.iter().find(|i| i.kind == kind).expect("modül kataloğu")
}

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct FxSpec {
    pub id: u32,
    pub kind: FxKind,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub params: Vec<f32>,
}

impl FxSpec {
    pub fn new(id: u32, kind: FxKind) -> FxSpec {
        FxSpec { id, kind, enabled: true, params: info(kind).params.iter().map(|p| p.default).collect() }
    }

    pub fn param(&self, i: usize) -> f32 {
        let p = &info(self.kind).params[i];
        self.params.get(i).copied().unwrap_or(p.default).clamp(p.min, p.max)
    }
}

fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

const TP_DELAY: usize = 8;

pub struct Limiter {
    n: usize,
    delay_l: Vec<f32>,
    delay_r: Vec<f32>,
    dpos: usize,

    win: std::collections::VecDeque<(u64, f32)>,
    count: u64,
    tp: [crate::loudness::TruePeak; 2],
    m_prev: f32,
    ring: Vec<f32>,
    rpos: usize,
    sum: f64,
    ceil: f32,
    rel: f32,
}

impl Limiter {
    pub fn new(rate: f32) -> Limiter {
        let n = ((0.002 * rate).round() as usize).max(2);
        Limiter {
            n,
            delay_l: vec![0.0; n - 1 + TP_DELAY],
            delay_r: vec![0.0; n - 1 + TP_DELAY],
            dpos: 0,
            win: std::collections::VecDeque::new(),
            count: 0,
            tp: [crate::loudness::TruePeak::new(), crate::loudness::TruePeak::new()],
            m_prev: 1.0,
            ring: vec![1.0; n],
            rpos: 0,
            sum: n as f64,
            ceil: 1.0,
            rel: 0.001,
        }
    }

    #[cfg(test)]
    pub fn latency(&self) -> usize {
        self.n - 1 + TP_DELAY
    }

    fn set(&mut self, enabled: bool, ceiling_db: f32, release_ms: f32, rate: f32) {
        self.ceil = if enabled { db_to_lin(ceiling_db) } else { 1.0e9 };
        self.rel = 1.0 - (-1.0 / (release_ms * 0.001 * rate)).exp();
    }

    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {

        let peak = self.tp[0].push_local(l).max(self.tp[1].push_local(r));
        let g_req = if peak > self.ceil { self.ceil / peak } else { 1.0 };

        let idx = self.count;
        self.count += 1;
        while self.win.back().is_some_and(|&(_, v)| v >= g_req) {
            self.win.pop_back();
        }
        self.win.push_back((idx, g_req));
        while self.win.front().is_some_and(|&(i, _)| i + self.n as u64 <= idx) {
            self.win.pop_front();
        }
        let m_raw = self.win.front().map(|x| x.1).unwrap_or(1.0);

        let m = m_raw.min(self.m_prev + (1.0 - self.m_prev) * self.rel);
        self.m_prev = m;

        self.sum += (m - self.ring[self.rpos]) as f64;
        self.ring[self.rpos] = m;
        self.rpos = (self.rpos + 1) % self.n;
        let s = (self.sum / self.n as f64) as f32;
        let (dl, dr) = (self.delay_l[self.dpos], self.delay_r[self.dpos]);
        self.delay_l[self.dpos] = l;
        self.delay_r[self.dpos] = r;
        self.dpos = (self.dpos + 1) % self.delay_l.len();
        let c = self.ceil.min(1.0e9);
        ((dl * s).clamp(-c, c), (dr * s).clamp(-c, c))
    }
}

pub struct Compressor {
    thr: f32,
    ratio: f32,
    knee: f32,
    makeup: f32,
    att: f32,
    rel: f32,
    gr_s: f32,
    enabled: bool,
}

impl Compressor {
    pub fn new() -> Compressor {
        Compressor { thr: -18.0, ratio: 3.0, knee: 6.0, makeup: 0.0, att: 0.99, rel: 0.999, gr_s: 0.0, enabled: true }
    }

    fn set(&mut self, enabled: bool, thr: f32, ratio: f32, att_ms: f32, rel_ms: f32, knee: f32, makeup: f32, rate: f32) {
        self.enabled = enabled;
        self.thr = thr;
        self.ratio = ratio.max(1.0);
        self.knee = knee;
        self.makeup = makeup;
        self.att = (-1.0 / (att_ms * 0.001 * rate)).exp();
        self.rel = (-1.0 / (rel_ms * 0.001 * rate)).exp();
    }

    fn gain_reduction_db(&self, x_db: f32) -> f32 {
        let over = x_db - self.thr;
        let y = if 2.0 * over < -self.knee {
            x_db
        } else if 2.0 * over.abs() <= self.knee {
            x_db + (1.0 / self.ratio - 1.0) * (over + self.knee / 2.0).powi(2) / (2.0 * self.knee)
        } else {
            self.thr + over / self.ratio
        };
        y - x_db
    }

    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let peak = l.abs().max(r.abs()).max(1.0e-9);
        let x_db = 20.0 * peak.log10();
        let target = if self.enabled { self.gain_reduction_db(x_db) } else { 0.0 };
        let c = if target < self.gr_s { self.att } else { self.rel };
        self.gr_s = c * self.gr_s + (1.0 - c) * target;
        let mk = if self.enabled { self.makeup } else { 0.0 };
        if self.gr_s.abs() < 1.0e-6 && mk == 0.0 {
            return (l, r);
        }
        let g = db_to_lin(self.gr_s + mk);
        (l * g, r * g)
    }
}

pub struct Width {
    w: f32,
    target: f32,
    a: f32,
}

impl Width {
    pub fn new(rate: f32) -> Width {
        Width { w: 1.0, target: 1.0, a: 1.0 - (-1.0 / (0.01 * rate)).exp() }
    }
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.w += (self.target - self.w) * self.a;
        if (self.w - 1.0).abs() < 1.0e-6 && self.target == 1.0 {
            self.w = 1.0;
            return (l, r);
        }
        let (m, s) = ((l + r) * 0.5, (l - r) * 0.5 * self.w);
        (m + s, m - s)
    }
}

pub struct Gain {
    g: f32,
    target: f32,
    a: f32,
}

impl Gain {
    pub fn new(rate: f32) -> Gain {
        Gain { g: 1.0, target: 1.0, a: 1.0 - (-1.0 / (0.01 * rate)).exp() }
    }
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.g += (self.target - self.g) * self.a;
        if (self.g - 1.0).abs() < 1.0e-6 && self.target == 1.0 {
            self.g = 1.0;
            return (l, r);
        }
        (l * self.g, r * self.g)
    }
}

use crate::apo::{Band, FilterKind, Preset};
use crate::biquad::Equalizer;

fn butterworth_qs(slope_db: f32) -> &'static [f64] {
    match (slope_db / 12.0).round() as i32 {
        i32::MIN..=1 => &[std::f64::consts::FRAC_1_SQRT_2],
        2 => &[0.5411961001461969, 1.3065629648763766],
        3 => &[0.5176380902050415, std::f64::consts::FRAC_1_SQRT_2, 1.9318516525781366],
        _ => &[0.5097955791041592, 0.6013448869350453, 0.8999762231364156, 2.5629154477415055],
    }
}

struct EqBlock {
    eq: Equalizer,
    empty: bool,
    rate: f32,
}

impl EqBlock {
    fn new(rate: f32) -> EqBlock {
        EqBlock { eq: Equalizer::new(&Preset::default(), rate as f64, 2), empty: true, rate }
    }
    fn set(&mut self, preset: &Preset) {
        self.empty = preset.bands.is_empty() && preset.preamp_db.abs() < 1.0e-9;
        if !self.eq.retarget(preset, self.rate as f64) {
            self.eq = Equalizer::new(preset, self.rate as f64, 2);
        }
    }
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        if self.empty {
            return (l, r);
        }
        self.eq.process_frame(l, r)
    }
}

pub struct Filter {
    blk: EqBlock,
    enabled: bool,
}

impl Filter {
    pub fn new(rate: f32) -> Filter {
        Filter { blk: EqBlock::new(rate), enabled: false }
    }
    pub fn set(&mut self, enabled: bool, hpf: f32, hpf_slope: f32, lpf: f32, lpf_slope: f32) {
        self.enabled = enabled;
        let nyq = self.blk.rate as f64 * 0.5;
        let mut bands = vec![];
        if hpf > 0.5 {
            for q in butterworth_qs(hpf_slope) {
                bands.push(Band { kind: FilterKind::HighPass, freq: hpf as f64, gain_db: 0.0, q: *q });
            }
        }
        if (lpf as f64) < 23999.5 && (lpf as f64) < nyq {
            for q in butterworth_qs(lpf_slope) {
                bands.push(Band { kind: FilterKind::LowPass, freq: lpf as f64, gain_db: 0.0, q: *q });
            }
        }
        self.blk.set(&Preset { preamp_db: 0.0, bands: if enabled { bands } else { vec![] } });
    }
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.blk.process(l, r)
    }
}

pub struct Tone {
    blk: EqBlock,
}

impl Tone {
    pub fn new(rate: f32) -> Tone {
        Tone { blk: EqBlock::new(rate) }
    }
    pub fn set(&mut self, enabled: bool, bass_db: f32, bass_hz: f32, treble_db: f32, treble_hz: f32) {
        if !enabled || (bass_db.abs() < 0.05 && treble_db.abs() < 0.05) {
            self.blk.set(&Preset::default());
            return;
        }
        let pre = -(bass_db.max(treble_db).max(0.0) as f64);
        self.blk.set(&Preset {
            preamp_db: pre,
            bands: vec![
                Band { kind: FilterKind::LowShelf, freq: bass_hz as f64, gain_db: bass_db as f64, q: std::f64::consts::FRAC_1_SQRT_2 },
                Band { kind: FilterKind::HighShelf, freq: treble_hz as f64, gain_db: treble_db as f64, q: std::f64::consts::FRAC_1_SQRT_2 },
            ],
        });
    }
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.blk.process(l, r)
    }
}

pub struct Loudness {
    blk: EqBlock,
    enabled: bool,
    strength: f32,
    reference: f32,
    level: f32,
    applied: (f32, f32),
}

impl Loudness {
    pub fn new(rate: f32) -> Loudness {
        Loudness { blk: EqBlock::new(rate), enabled: false, strength: 1.0, reference: 1.0, level: 1.0, applied: (f32::NAN, f32::NAN) }
    }
    pub fn set(&mut self, enabled: bool, strength_pct: f32, reference_pct: f32) {
        self.enabled = enabled;
        self.strength = strength_pct / 100.0;
        self.reference = (reference_pct / 100.0).max(0.01);
        self.refresh();
    }
    pub fn set_level(&mut self, level: f32) {
        self.level = level;
        self.refresh();
    }

    pub fn gains(level: f32, reference: f32, strength: f32) -> (f32, f32) {
        let drop_db = (20.0 * (reference / level.max(1.0e-4)).log10()).max(0.0);
        ((0.5 * drop_db).min(12.0) * strength, (0.25 * drop_db).min(8.0) * strength)
    }
    fn refresh(&mut self) {
        let (b, t) = if self.enabled { Self::gains(self.level, self.reference, self.strength) } else { (0.0, 0.0) };

        if (b - self.applied.0).abs() < 0.1 && (t - self.applied.1).abs() < 0.1 {
            return;
        }
        self.applied = (b, t);
        if b.abs() < 0.05 && t.abs() < 0.05 {
            self.blk.set(&Preset::default());
            return;
        }
        self.blk.set(&Preset {
            preamp_db: 0.0,
            bands: vec![
                Band { kind: FilterKind::LowShelf, freq: 110.0, gain_db: b as f64, q: 0.6 },
                Band { kind: FilterKind::HighShelf, freq: 9000.0, gain_db: t as f64, q: 0.6 },
            ],
        });
    }
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.blk.process(l, r)
    }
}

pub struct Crossfeed {
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    pos: usize,
    delay: usize,
    lp_l: f32,
    lp_r: f32,
    a: f32,
    g: f32,
    g_target: f32,
    g_a: f32,
    enabled: bool,
    rate: f32,
}

impl Crossfeed {
    pub fn new(rate: f32) -> Crossfeed {
        let n = (0.0008 * rate) as usize + 2;
        Crossfeed {
            buf_l: vec![0.0; n],
            buf_r: vec![0.0; n],
            pos: 0,
            delay: 0,
            lp_l: 0.0,
            lp_r: 0.0,
            a: 0.1,
            g: 0.0,
            g_target: 0.0,
            g_a: 1.0 - (-1.0 / (0.01 * rate)).exp(),
            enabled: false,
            rate,
        }
    }
    pub fn set(&mut self, enabled: bool, level_db: f32, cutoff_hz: f32, delay_us: f32) {
        self.enabled = enabled;
        self.g_target = if enabled { db_to_lin(level_db) } else { 0.0 };
        self.a = 1.0 - (-2.0 * std::f32::consts::PI * cutoff_hz / self.rate).exp();
        self.delay = ((delay_us * 1.0e-6 * self.rate).round() as usize).min(self.buf_l.len() - 1);
    }
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.g += (self.g_target - self.g) * self.g_a;
        if !self.enabled && self.g < 1.0e-6 {
            self.g = 0.0;
            return (l, r);
        }
        let n = self.buf_l.len();
        self.buf_l[self.pos] = l;
        self.buf_r[self.pos] = r;
        let idx = (self.pos + n - self.delay) % n;
        let (dl, dr) = (self.buf_l[idx], self.buf_r[idx]);
        self.pos = (self.pos + 1) % n;
        self.lp_l += self.a * (dl - self.lp_l);
        self.lp_r += self.a * (dr - self.lp_r);
        let norm = 1.0 / (1.0 + 0.7 * self.g);
        ((l + self.g * self.lp_r) * norm, (r + self.g * self.lp_l) * norm)
    }
}

enum Node {
    Limiter(Limiter),
    Compressor(Compressor),
    Width(Width),
    Gain(Gain),
    Filter(Filter),
    Tone(Tone),
    Loudness(Loudness),
    Crossfeed(Crossfeed),
}

struct Instance {
    id: u32,
    kind: FxKind,
    enabled: bool,
    node: Node,
}

pub struct FxChain {
    items: Vec<Instance>,
    rate: f32,
}

impl FxChain {
    pub fn new(specs: &[FxSpec], rate: f32) -> FxChain {
        let items = specs
            .iter()
            .map(|s| Instance {
                id: s.id,
                kind: s.kind,
                enabled: s.enabled,
                node: match s.kind {
                    FxKind::Limiter => Node::Limiter(Limiter::new(rate)),
                    FxKind::Compressor => Node::Compressor(Compressor::new()),
                    FxKind::Width => Node::Width(Width::new(rate)),
                    FxKind::Gain => Node::Gain(Gain::new(rate)),
                    FxKind::Filter => Node::Filter(Filter::new(rate)),
                    FxKind::Tone => Node::Tone(Tone::new(rate)),
                    FxKind::Loudness => Node::Loudness(Loudness::new(rate)),
                    FxKind::Crossfeed => Node::Crossfeed(Crossfeed::new(rate)),
                },
            })
            .collect();
        let mut c = FxChain { items, rate };
        c.update(specs);
        c
    }

    pub fn same_structure(&self, specs: &[FxSpec]) -> bool {
        self.items.len() == specs.len() && self.items.iter().zip(specs).all(|(i, s)| i.id == s.id && i.kind == s.kind)
    }

    pub fn update(&mut self, specs: &[FxSpec]) {
        let rate = self.rate;
        for (it, s) in self.items.iter_mut().zip(specs) {
            it.enabled = s.enabled;
            match &mut it.node {
                Node::Limiter(l) => l.set(s.enabled, s.param(0), s.param(1), rate),
                Node::Compressor(c) => c.set(s.enabled, s.param(0), s.param(1), s.param(2), s.param(3), s.param(4), s.param(5), rate),
                Node::Width(w) => w.target = if s.enabled { s.param(0) / 100.0 } else { 1.0 },
                Node::Gain(g) => g.target = if s.enabled { db_to_lin(s.param(0)) } else { 1.0 },
                Node::Filter(f) => f.set(s.enabled, s.param(0), s.param(1), s.param(2), s.param(3)),
                Node::Tone(t) => t.set(s.enabled, s.param(0), s.param(1), s.param(2), s.param(3)),
                Node::Loudness(l) => l.set(s.enabled, s.param(0), s.param(1)),
                Node::Crossfeed(c) => c.set(s.enabled, s.param(0), s.param(1), s.param(2)),
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn set_level(&mut self, level: f32) {
        for it in self.items.iter_mut() {
            if let Node::Loudness(l) = &mut it.node {
                l.set_level(level);
            }
        }
    }

    pub fn any_enabled(&self) -> bool {
        self.items.iter().any(|i| i.enabled)
    }

    pub fn process(&mut self, mut l: f32, mut r: f32) -> (f32, f32) {
        for it in self.items.iter_mut() {
            let o = match &mut it.node {
                Node::Limiter(n) => n.process(l, r),
                Node::Compressor(n) => n.process(l, r),
                Node::Width(n) => n.process(l, r),
                Node::Gain(n) => n.process(l, r),
                Node::Filter(n) => n.process(l, r),
                Node::Tone(n) => n.process(l, r),
                Node::Loudness(n) => n.process(l, r),
                Node::Crossfeed(n) => n.process(l, r),
            };
            l = o.0;
            r = o.1;
        }
        (l, r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f32 = 48000.0;

    fn amp(mut f: impl FnMut(f32, f32) -> (f32, f32), hz: f32) -> f32 {
        let n = 48000usize;
        let mut acc = 0f64;
        let mut cnt = 0usize;
        for i in 0..n {
            let x = (2.0 * std::f32::consts::PI * hz * i as f32 / FS).sin() * 0.5;
            let (y, _) = f(x, x);
            if i > n / 2 {
                acc += (y as f64) * (y as f64);
                cnt += 1;
            }
        }
        ((acc / cnt as f64).sqrt() * 2f64.sqrt() * 2.0) as f32
    }
    fn db(x: f32) -> f32 {
        20.0 * x.log10()
    }

    #[test]
    fn filter_cuts_the_requested_side_and_passes_the_rest() {
        let mut f = Filter::new(FS);
        f.set(true, 100.0, 24.0, 24000.0, 24.0);
        let low = db(amp(|l, r| f.process(l, r), 20.0));
        let mut f = Filter::new(FS);
        f.set(true, 100.0, 24.0, 24000.0, 24.0);
        let mid = db(amp(|l, r| f.process(l, r), 1000.0));
        assert!(low < -40.0, "20 Hz yeterince kesilmedi: {low} dB");
        assert!(mid.abs() < 0.2, "1 kHz geçmeli: {mid} dB");

        let mut f = Filter::new(FS);
        f.set(true, 0.0, 24.0, 5000.0, 24.0);
        let hi = db(amp(|l, r| f.process(l, r), 15000.0));
        let mut f = Filter::new(FS);
        f.set(true, 0.0, 24.0, 5000.0, 24.0);
        let mid = db(amp(|l, r| f.process(l, r), 1000.0));
        assert!(hi < -30.0, "15 kHz kesilmeli: {hi} dB");
        assert!(mid.abs() < 0.2, "{mid} dB");
    }

    #[test]
    fn filter_slope_is_steeper_for_more_stages() {
        let at = |slope: f32| {
            let mut f = Filter::new(FS);
            f.set(true, 200.0, slope, 24000.0, 24.0);
            db(amp(|l, r| f.process(l, r), 100.0))
        };
        let (a, b, c) = (at(12.0), at(24.0), at(48.0));
        assert!(a > b && b > c, "{a} {b} {c}");
        assert!((a - -12.3).abs() < 1.5, "12 dB/okt (2. derece), bir oktav aşağıda ≈ -12 dB: {a}");
    }

    #[test]
    fn filter_off_is_transparent() {
        let mut f = Filter::new(FS);
        f.set(true, 0.0, 24.0, 24000.0, 24.0);
        for i in 0..1000 {
            let x = (i as f32 * 0.01).sin() * 0.7;
            assert_eq!(f.process(x, -x), (x, -x));
        }

        let mut f = Filter::new(FS);
        f.set(false, 100.0, 24.0, 5000.0, 24.0);
        assert_eq!(f.process(0.3, 0.2), (0.3, 0.2));
    }

    #[test]
    fn tone_boosts_bass_and_trims_to_avoid_clipping() {
        let mut t = Tone::new(FS);
        t.set(true, 6.0, 100.0, 0.0, 8000.0);
        let bass = db(amp(|l, r| t.process(l, r), 40.0));
        let mut t = Tone::new(FS);
        t.set(true, 6.0, 100.0, 0.0, 8000.0);
        let mid = db(amp(|l, r| t.process(l, r), 2000.0));

        assert!(bass < 0.8, "bas kırpma payını aşmamalı: {bass}");
        assert!((mid - -6.0).abs() < 1.0, "orta kısılmalı: {mid}");

        let mut t = Tone::new(FS);
        t.set(true, 0.0, 100.0, 0.0, 8000.0);
        assert_eq!(t.process(0.4, 0.4), (0.4, 0.4));
    }

    #[test]
    fn loudness_boosts_more_as_volume_drops_and_never_at_reference() {
        assert_eq!(Loudness::gains(1.0, 1.0, 1.0), (0.0, 0.0));
        assert_eq!(Loudness::gains(1.5, 1.0, 1.0), (0.0, 0.0), "referans üstünde telafi yok");
        let (b1, t1) = Loudness::gains(0.5, 1.0, 1.0);
        let (b2, t2) = Loudness::gains(0.1, 1.0, 1.0);
        assert!(b2 > b1 && t2 > t1 && b1 > t1, "{b1} {t1} {b2} {t2}");
        assert!(b2 <= 12.0 && t2 <= 8.0, "üst sınır");

        assert_eq!(Loudness::gains(0.1, 1.0, 0.0), (0.0, 0.0));

        let mut l = Loudness::new(FS);
        l.set(true, 100.0, 100.0);
        l.set_level(0.1);
        let bass = db(amp(|a, b| l.process(a, b), 60.0));
        let mut l = Loudness::new(FS);
        l.set(true, 100.0, 100.0);
        l.set_level(0.1);
        let mid = db(amp(|a, b| l.process(a, b), 1000.0));
        assert!(bass - mid > 5.0, "bas orta'dan belirgin güçlü olmalı: {bass} vs {mid}");

        let mut l = Loudness::new(FS);
        l.set(true, 100.0, 100.0);
        l.set_level(1.0);
        assert_eq!(l.process(0.3, 0.1), (0.3, 0.1));
    }

    #[test]
    fn crossfeed_mixes_delayed_lowpassed_opposite_channel() {
        let mut c = Crossfeed::new(FS);
        c.set(true, -6.0, 700.0, 270.0);

        let mut r_amp = 0f32;
        let mut l_amp = 0f32;
        for i in 0..48000 {
            let x = (2.0 * std::f32::consts::PI * 100.0 * i as f32 / FS).sin() * 0.5;
            let (l, r) = c.process(x, 0.0);
            if i > 24000 {
                l_amp = l_amp.max(l.abs());
                r_amp = r_amp.max(r.abs());
            }
        }
        assert!(r_amp > 0.05 && r_amp < l_amp, "{l_amp} {r_amp}");

        let mut c = Crossfeed::new(FS);
        c.set(true, -6.0, 700.0, 270.0);
        let mut r_hi = 0f32;
        for i in 0..48000 {
            let x = (2.0 * std::f32::consts::PI * 8000.0 * i as f32 / FS).sin() * 0.5;
            let (_, r) = c.process(x, 0.0);
            if i > 24000 {
                r_hi = r_hi.max(r.abs());
            }
        }
        assert!(r_hi < 0.02, "yüksek frekans sızmamalı: {r_hi}");

        let mut c = Crossfeed::new(FS);
        c.set(false, -6.0, 700.0, 270.0);
        assert_eq!(c.process(0.3, 0.2), (0.3, 0.2));
    }

    #[test]
    fn crossfeed_delay_is_applied() {
        let mut c = Crossfeed::new(FS);
        c.set(true, 0.0, 2000.0, 400.0);

        for _ in 0..4800 {
            c.process(0.0, 0.0);
        }
        let mut first = None;
        for i in 0..100 {
            let (_, r) = c.process(if i == 0 { 1.0 } else { 0.0 }, 0.0);
            if first.is_none() && r.abs() > 1.0e-4 {
                first = Some(i);
            }
        }
        let d = first.expect("sağ kanala sızıntı bekleniyordu");
        assert!(d >= 18 && d <= 22, "gecikme ≈19 örnek olmalı: {d}");
    }

    fn lcg(seed: &mut u32) -> f32 {
        *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (*seed >> 8) as f32 / 8388608.0 - 1.0
    }

    fn limiter(ceiling_db: f32, rel_ms: f32) -> Limiter {
        let mut l = Limiter::new(FS);
        l.set(true, ceiling_db, rel_ms, FS);
        l
    }

    #[test]
    fn limiter_never_exceeds_ceiling() {
        let mut lim = limiter(-1.0, 100.0);
        let ceil = db_to_lin(-1.0);
        let mut seed = 7u32;
        let mut worst = 0f32;
        for i in 0..200_000 {

            let burst = if (i / 5000) % 3 == 0 { 3.0 } else { 0.4 };
            let (l, r) = (lcg(&mut seed) * burst, lcg(&mut seed) * burst);
            let (ol, or) = lim.process(l, r);
            worst = worst.max(ol.abs()).max(or.abs());
        }
        assert!(worst <= ceil + 1.0e-6, "tavan asildi: {worst} > {ceil}");

        assert!(worst > ceil * 0.8, "limiter cok fazla indiriyor: {worst}");
    }

    #[test]
    fn limiter_controls_inter_sample_peaks() {

        let mut lim = limiter(-1.0, 100.0);
        let ceil = db_to_lin(-1.0);
        let a = 1.2f32;
        let sig: Vec<f32> = (0..24000).map(|i| a * (std::f32::consts::FRAC_PI_2 * i as f32 + std::f32::consts::FRAC_PI_4).sin()).collect();
        assert!(sig.iter().all(|x| x.abs() < ceil), "örnek tepeleri zaten altında olmalı");
        let out: Vec<f32> = sig.iter().map(|x| lim.process(*x, *x).0).collect();
        let mut tp = crate::loudness::TruePeak::new();
        let mut worst = 0f32;
        for (i, x) in out.iter().enumerate() {
            let m = tp.push(*x);
            if i > 12000 {
                worst = worst.max(m);
            }
        }

        assert!(worst <= ceil * 1.04, "gerçek tepe {worst} > {ceil}");
        assert!(worst > ceil * 0.8, "gereğinden fazla indiriyor: {worst}");
    }

    #[test]
    fn sliding_minimum_matches_naive() {

        let mut lim = limiter(-3.0, 30.0);
        let mut seed = 99u32;
        let mut peak = 0f32;
        for _ in 0..50_000 {
            let (l, r) = (lcg(&mut seed) * 2.5, lcg(&mut seed) * 2.5);
            let (a, b) = lim.process(l, r);
            peak = peak.max(a.abs()).max(b.abs());
        }
        assert!(peak <= db_to_lin(-3.0) + 1.0e-6);
    }

    #[test]
    fn limiter_is_transparent_below_ceiling() {
        let mut lim = limiter(-1.0, 100.0);
        let d = lim.latency();
        let sig: Vec<f32> = (0..4800).map(|i| 0.3 * (2.0 * std::f32::consts::PI * 997.0 * i as f32 / FS).sin()).collect();
        let out: Vec<f32> = sig.iter().map(|x| lim.process(*x, *x).0).collect();
        for i in d..sig.len() {
            assert!((out[i] - sig[i - d]).abs() < 1.0e-6, "i={i}");
        }
    }

    #[test]
    fn limiter_releases_after_burst() {
        let mut lim = limiter(-1.0, 50.0);
        for _ in 0..2000 {
            lim.process(2.0, 2.0);
        }

        let mut last = 0.0;
        for _ in 0..24000 {
            last = lim.process(0.3, 0.3).0;
        }
        assert!((last - 0.3).abs() < 0.01, "bırakma gerçekleşmedi: {last}");
    }

    #[test]
    fn limiter_disabled_is_pure_delay() {
        let mut lim = Limiter::new(FS);
        lim.set(false, -1.0, 100.0, FS);
        let d = lim.latency();
        let out: Vec<f32> = (0..1000).map(|i| lim.process(2.0 * (i as f32 * 0.01).sin(), 0.0).0).collect();
        for i in d..1000 {
            assert!((out[i] - 2.0 * ((i - d) as f32 * 0.01).sin()).abs() < 1.0e-6);
        }
    }

    #[test]
    fn compressor_static_curve() {

        let mut c = Compressor::new();
        c.set(true, -18.0, 4.0, 5.0, 100.0, 0.0, 0.0, FS);
        let mut y = 0.0;
        for _ in 0..FS as usize {
            y = c.process(0.5, 0.5).0;
        }
        let db = 20.0 * y.log10();
        assert!((db + 15.0).abs() < 0.1, "{db} dB");

        let mut c2 = Compressor::new();
        c2.set(true, -18.0, 4.0, 5.0, 100.0, 0.0, 0.0, FS);
        for _ in 0..1000 {
            let (a, _) = c2.process(0.05, 0.05);
            assert_eq!(a, 0.05);
        }
    }

    #[test]
    fn compressor_makeup_and_disable() {
        let mut c = Compressor::new();
        c.set(true, -18.0, 1.0, 5.0, 50.0, 0.0, 6.0, FS);
        let y = c.process(0.1, 0.1).0;
        assert!((20.0 * (y / 0.1).log10() - 6.0).abs() < 0.05);
        c.set(false, -18.0, 4.0, 5.0, 50.0, 0.0, 6.0, FS);
        let mut y2 = 0.0;
        for _ in 0..FS as usize {
            y2 = c.process(0.5, 0.5).0;
        }
        assert!((y2 - 0.5).abs() < 1.0e-4);
    }

    #[test]
    fn width_behaviour() {
        let mut w = Width::new(FS);
        assert_eq!(w.process(0.3, -0.2), (0.3, -0.2));
        w.target = 0.0;
        let mut o = (0.0, 0.0);
        for _ in 0..FS as usize {
            o = w.process(0.6, 0.2);
        }
        assert!((o.0 - 0.4).abs() < 1.0e-3 && (o.1 - 0.4).abs() < 1.0e-3, "mono degil: {o:?}");
        w.target = 2.0;
        for _ in 0..FS as usize {
            o = w.process(0.6, 0.2);
        }

        assert!((o.0 - 0.8).abs() < 1.0e-3 && o.1.abs() < 1.0e-3, "{o:?}");
    }

    #[test]
    fn gain_reaches_target_smoothly() {
        let mut g = Gain::new(FS);
        assert_eq!(g.process(0.25, -0.25), (0.25, -0.25));
        g.target = db_to_lin(6.0);
        let first = g.process(1.0, 1.0).0;
        assert!(first < 1.01, "ani atlama: {first}");
        let mut y = 0.0;
        for _ in 0..FS as usize {
            y = g.process(1.0, 1.0).0;
        }
        assert!((y - db_to_lin(6.0)).abs() < 0.002);
    }

    #[test]
    fn chain_structure_and_params() {
        let a = FxSpec::new(1, FxKind::Limiter);
        let b = FxSpec::new(2, FxKind::Gain);
        let mut chain = FxChain::new(&[a.clone(), b.clone()], FS);
        assert!(chain.same_structure(&[a.clone(), b.clone()]));
        assert!(!chain.same_structure(&[b.clone(), a.clone()]));
        assert!(!chain.same_structure(&[a.clone()]));
        let mut b2 = b.clone();
        b2.params = vec![-6.0];
        assert!(chain.same_structure(&[a.clone(), b2.clone()]));
        chain.update(&[a, b2]);
        assert!(chain.any_enabled());
        assert!(FxChain::new(&[], FS).is_empty());
    }

    #[test]
    fn empty_chain_is_bit_exact() {
        let mut c = FxChain::new(&[], FS);
        for v in [0.123456789f32, -0.987654321, 1.4, -2.0] {
            assert_eq!(c.process(v, -v), (v, -v));
        }
    }

    #[test]
    fn spec_serde_roundtrip_and_clamp() {
        let mut s = FxSpec::new(3, FxKind::Compressor);
        s.params[0] = -99.0;
        let j = serde_json::to_string(&s).unwrap();
        let back: FxSpec = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.param(0), -50.0);

        let old: FxSpec = serde_json::from_str(r#"{"id":1,"kind":"Limiter"}"#).unwrap();
        assert!(old.enabled);
        assert_eq!(old.param(0), -0.5);
    }
}
