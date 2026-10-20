//! Ses motoru modülleri ("market"): sinyal zincirine takılan DSP blokları.
//! Zincir boşsa motor sinyale hiç dokunmaz (bit-perfect yol bozulmaz).

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum FxKind {
    Limiter,
    Compressor,
    Width,
    Gain,
}

pub struct ParamInfo {
    pub name: &'static str,
    pub unit: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub decimals: usize,
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
            ParamInfo { name: "Tavan", unit: "dB", min: -6.0, max: 0.0, default: -0.5, decimals: 1 },
            ParamInfo { name: "Bırakma", unit: "ms", min: 10.0, max: 500.0, default: 120.0, decimals: 0 },
        ],
    },
    FxInfo {
        kind: FxKind::Compressor,
        name: "Kompresör",
        tagline: "Dinamik aralık",
        desc: "Eşiği aşan sesleri orana göre bastırır. Yumuşak diz ile geçiş doğal olur. Kıs ve yüksek sesler arasındaki farkı azaltmak için.",
        params: &[
            ParamInfo { name: "Eşik", unit: "dB", min: -50.0, max: 0.0, default: -18.0, decimals: 1 },
            ParamInfo { name: "Oran", unit: ":1", min: 1.0, max: 20.0, default: 3.0, decimals: 1 },
            ParamInfo { name: "Atak", unit: "ms", min: 1.0, max: 100.0, default: 10.0, decimals: 0 },
            ParamInfo { name: "Bırakma", unit: "ms", min: 20.0, max: 1000.0, default: 150.0, decimals: 0 },
            ParamInfo { name: "Diz", unit: "dB", min: 0.0, max: 18.0, default: 6.0, decimals: 1 },
            ParamInfo { name: "Makyaj", unit: "dB", min: 0.0, max: 18.0, default: 0.0, decimals: 1 },
        ],
    },
    FxInfo {
        kind: FxKind::Width,
        name: "Stereo Genişlik",
        tagline: "Sahne genişliği",
        desc: "Orta/yan (M/S) dengesiyle stereo sahneyi daraltır ya da genişletir. %0 mono, %100 olduğu gibi, %200 yanı iki katına çıkarır.",
        params: &[ParamInfo { name: "Genişlik", unit: "%", min: 0.0, max: 200.0, default: 100.0, decimals: 0 }],
    },
    FxInfo {
        kind: FxKind::Gain,
        name: "Kazanç",
        tagline: "Seviye ayarı",
        desc: "Zincirin bir noktasında sesi yükseltir ya da kısar. EQ'dan sonra kısıp başka bir modüle pay bırakmak için kullanışlı.",
        params: &[ParamInfo { name: "Kazanç", unit: "dB", min: -24.0, max: 12.0, default: 0.0, decimals: 1 }],
    },
];

pub fn info(kind: FxKind) -> &'static FxInfo {
    CATALOG.iter().find(|i| i.kind == kind).expect("modül kataloğu")
}

fn yes() -> bool {
    true
}

/// Zincirdeki bir modülün kaydedilen hali.
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
    /// i. parametre, aralığa kırpılmış; eksikse varsayılan.
    pub fn param(&self, i: usize) -> f32 {
        let p = &info(self.kind).params[i];
        self.params.get(i).copied().unwrap_or(p.default).clamp(p.min, p.max)
    }
}

fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

// ================================================================== Limiter

/// Gerçek tepe tespiti için ek gecikme (örnek): ara noktaların hesabı 8 örnek ileri bakar.
const TP_DELAY: usize = 8;

/// Önden bakan (lookahead) stereo-bağlı limiter. Garanti: |örnek çıkış| <= tavan. Örnekler arası (gerçek) tepeler
/// 4x aşırı örnekleme ile önceden görülüp indirilir; gerçek tepe tavanı yaklaşık olarak (birkaç %) aşmaz.
pub struct Limiter {
    n: usize,
    delay_l: Vec<f32>,
    delay_r: Vec<f32>,
    dpos: usize,
    /// kayan pencere minimumu için tekdüze azalan kuyruk: (örnek sırası, istenen kazanç)
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

    /// Gecikme (örnek).
    pub fn latency(&self) -> usize {
        self.n - 1 + TP_DELAY
    }

    fn set(&mut self, enabled: bool, ceiling_db: f32, release_ms: f32, rate: f32) {
        self.ceil = if enabled { db_to_lin(ceiling_db) } else { 1.0e9 };
        self.rel = 1.0 - (-1.0 / (release_ms * 0.001 * rate)).exp();
    }

    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        // tepe, 8 örnek geriden gelen pencerenin (örnekler arası noktalar dahil) en büyüğü; ses de buna göre gecikmeli
        let peak = self.tp[0].push_local(l).max(self.tp[1].push_local(r));
        let g_req = if peak > self.ceil { self.ceil / peak } else { 1.0 };
        // son n isteğin minimumu, O(1): tekdüze kuyruk
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
        // ani atak, üstel bırakma; hiçbir zaman pencere minimumunun üstüne çıkmaz
        let m = m_raw.min(self.m_prev + (1.0 - self.m_prev) * self.rel);
        self.m_prev = m;
        // kutu ortalaması: atak rampası lookahead süresi kadar yumuşak olur
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

// ================================================================ Kompresör

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

    /// Statik eğri: giriş seviyesi (dB) -> hedef kazanç değişimi (dB, <= 0).
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

// ===================================================================== Width

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

// ====================================================================== Gain

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

// ===================================================================== Zincir

enum Node {
    Limiter(Limiter),
    Compressor(Compressor),
    Width(Width),
    Gain(Gain),
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
                },
            })
            .collect();
        let mut c = FxChain { items, rate };
        c.update(specs);
        c
    }

    /// Aynı modüller aynı sırada mı? (Evetse yalnızca parametreler yerinde güncellenir.)
    pub fn same_structure(&self, specs: &[FxSpec]) -> bool {
        self.items.len() == specs.len() && self.items.iter().zip(specs).all(|(i, s)| i.id == s.id && i.kind == s.kind)
    }

    /// Parametreleri yerinde güncelle (durum korunur). Yapı aynı olmalı.
    pub fn update(&mut self, specs: &[FxSpec]) {
        let rate = self.rate;
        for (it, s) in self.items.iter_mut().zip(specs) {
            it.enabled = s.enabled;
            match &mut it.node {
                Node::Limiter(l) => l.set(s.enabled, s.param(0), s.param(1), rate),
                Node::Compressor(c) => c.set(s.enabled, s.param(0), s.param(1), s.param(2), s.param(3), s.param(4), s.param(5), rate),
                Node::Width(w) => w.target = if s.enabled { s.param(0) / 100.0 } else { 1.0 },
                Node::Gain(g) => g.target = if s.enabled { db_to_lin(s.param(0)) } else { 1.0 },
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Sinyale dokunabilecek açık bir modül var mı?
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
            // gürültü + ani tam ölçek-üstü patlamalar + DC sıçraması
            let burst = if (i / 5000) % 3 == 0 { 3.0 } else { 0.4 };
            let (l, r) = (lcg(&mut seed) * burst, lcg(&mut seed) * burst);
            let (ol, or) = lim.process(l, r);
            worst = worst.max(ol.abs()).max(or.abs());
        }
        assert!(worst <= ceil + 1.0e-6, "tavan asildi: {worst} > {ceil}");
        // ve gerçekten sınırlıyor (tavana yakın tepe var)
        // gerçek tepe tavanda tutulduğu için gürültüde örnek tepeleri tavanın ~1.5 dB altına inebilir
        assert!(worst > ceil * 0.8, "limiter cok fazla indiriyor: {worst}");
    }


    #[test]
    fn limiter_controls_inter_sample_peaks() {
        // fs/4'te, 45° faz: örnekler ±0.85 (tavanın altında) ama sürekli (gerçek) tepe 1.2
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
        // sürekli tepe tavana yaklaşmalı (tespit çalışıyor) ve belirgin aşmamalı
        assert!(worst <= ceil * 1.04, "gerçek tepe {worst} > {ceil}");
        assert!(worst > ceil * 0.8, "gereğinden fazla indiriyor: {worst}");
    }

    #[test]
    fn sliding_minimum_matches_naive() {
        // tekdüze kuyruk, saf pencere minimumuyla aynı sonucu vermeli (rastgele girişte limiter çıkışı sabit kalır)
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
        // patlama bitti; 0.3'lük sinyal 0.5 sn sonra tamamen geri gelmeli
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
        // DC 0.5 (-6.02 dBFS), eşik -18, oran 4, diz 0: beklenen = -18 + 11.98/4 = -15.0 dB
        let mut c = Compressor::new();
        c.set(true, -18.0, 4.0, 5.0, 100.0, 0.0, 0.0, FS);
        let mut y = 0.0;
        for _ in 0..FS as usize {
            y = c.process(0.5, 0.5).0;
        }
        let db = 20.0 * y.log10();
        assert!((db + 15.0).abs() < 0.1, "{db} dB");
        // eşiğin altında dokunmaz
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
        assert_eq!(w.process(0.3, -0.2), (0.3, -0.2)); // %100 birebir
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
        // M=0.4, S=0.2*2=0.4 -> L=0.8, R=0.0
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
        assert!(!chain.same_structure(&[b.clone(), a.clone()])); // sıra değişti
        assert!(!chain.same_structure(&[a.clone()]));
        let mut b2 = b.clone();
        b2.params = vec![-6.0];
        assert!(chain.same_structure(&[a.clone(), b2.clone()])); // yalnız parametre
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
        s.params[0] = -99.0; // aralık dışı
        let j = serde_json::to_string(&s).unwrap();
        let back: FxSpec = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.param(0), -50.0); // kırpıldı
        // eksik parametreli eski kayıt varsayılanla tamamlanır
        let old: FxSpec = serde_json::from_str(r#"{"id":1,"kind":"Limiter"}"#).unwrap();
        assert!(old.enabled);
        assert_eq!(old.param(0), -0.5);
    }
}
