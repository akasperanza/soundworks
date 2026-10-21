use std::f64::consts::SQRT_2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterKind {
    Peak,
    LowPass,
    HighPass,
    BandPass,
    LowShelf,
    HighShelf,
    Notch,
    AllPass,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    pub kind: FilterKind,
    pub freq: f64,
    pub gain_db: f64,
    pub q: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Preset {
    pub preamp_db: f64,
    pub bands: Vec<Band>,
}

fn number_after(line: &str, key: &str, unit: Option<&str>) -> Option<f64> {
    let lower = line.to_ascii_lowercase();
    let mut from = 0;
    while let Some(pos) = lower[from..].find(key) {
        let start = from + pos;

        let before_ok = start == 0 || !lower.as_bytes()[start - 1].is_ascii_alphanumeric();
        from = start + key.len();
        if !before_ok {
            continue;
        }
        let rest = lower[from..].trim_start();
        let end = rest
            .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | ',' | '+' | '-')))
            .unwrap_or(rest.len());
        if end == 0 {
            continue;
        }
        let num = rest[..end].replace(',', "");
        if let Some(u) = unit {
            if !rest[end..].trim_start().starts_with(u) {
                continue;
            }
        }
        if let Ok(v) = num.parse::<f64>() {
            return Some(v);
        }
    }
    None
}

fn parse_preamp(line: &str) -> Option<f64> {
    let l = line.trim_start().to_ascii_lowercase();
    if !l.starts_with("preamp") {
        return None;
    }
    number_after(&l, "preamp", None).or_else(|| {
        let after = l.split(':').nth(1)?;
        number_after(&format!("x {after}"), "x", Some("db"))
    })
}

fn parse_filter(line: &str) -> Option<Option<Band>> {
    let l = line.trim().to_ascii_lowercase();
    let (head, rest) = l.split_once(':')?;
    let head = head.trim();
    if !head.starts_with("filter") || !head[6..].trim().chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut toks = rest.split_whitespace();
    match toks.next()? {
        "off" => return Some(None),
        "on" => {}
        _ => return None,
    }
    let kind_tok = toks.next()?;

    let mut suffix: Option<&str> = None;
    let mut peek = toks.clone();
    if let Some(n) = peek.next() {
        if n == "6db" || n == "12db" {
            suffix = Some(n);
        }
    }

    let mut freq = number_after(&l, "fc", Some("hz")).unwrap_or(1000.0);
    let gain = number_after(&l, "gain", Some("db")).unwrap_or(0.0);
    let q_parsed = number_after(&l, "q", None);
    let default_q = 1.0 / SQRT_2;
    let mut q = q_parsed.unwrap_or(default_q);

    let (kind, gain_db) = match (kind_tok, suffix) {
        ("pk" | "modal" | "peq", _) => (FilterKind::Peak, gain),
        ("lp", _) => (FilterKind::LowPass, 0.0),
        ("lpq", _) => (FilterKind::LowPass, 0.0),
        ("hp", _) => (FilterKind::HighPass, 0.0),
        ("hpq", _) => (FilterKind::HighPass, 0.0),
        ("bp", _) => (FilterKind::BandPass, 0.0),
        ("ls", None) => {
            q = 2.0 / 3.0;
            (FilterKind::LowShelf, gain)
        }
        ("hs", None) => {
            q = 2.0 / 3.0;
            (FilterKind::HighShelf, gain)
        }
        ("lsc", _) => (FilterKind::LowShelf, gain),
        ("hsc", _) => (FilterKind::HighShelf, gain),
        ("ls", Some("6db")) => {
            freq = freq * 2.0 / 3.0;
            q = SQRT_2 / 3.0;
            (FilterKind::LowShelf, gain)
        }
        ("ls", Some("12db")) => {
            freq = freq * 3.0 / 2.0;
            (FilterKind::LowShelf, gain)
        }
        ("hs", Some("6db")) => {
            freq = freq * SQRT_2;
            q = SQRT_2 / 3.0;
            (FilterKind::HighShelf, gain)
        }
        ("hs", Some("12db")) => {
            freq = freq / SQRT_2;
            (FilterKind::HighShelf, gain)
        }
        ("no", _) => {
            q = 100.0 / 3.0;
            (FilterKind::Notch, 0.0)
        }
        ("ap", _) => (FilterKind::AllPass, 0.0),
        _ => return Some(None),
    };
    Some(Some(Band { kind, freq, gain_db, q }))
}

pub fn parse(text: &str) -> Preset {
    let mut p = Preset::default();
    for line in text.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        if let Some(Some(b)) = parse_filter(line) {
            p.bands.push(b);
        } else if let Some(g) = parse_preamp(line) {
            p.preamp_db = g;
        }
    }
    p
}

impl FilterKind {
    pub fn label(&self) -> &'static str {
        match self {
            FilterKind::Peak => "Bell",
            FilterKind::LowShelf => "Lo-shelf",
            FilterKind::HighShelf => "Hi-shelf",
            FilterKind::LowPass => "Lo-pass",
            FilterKind::HighPass => "Hi-pass",
            FilterKind::BandPass => "Bandpass",
            FilterKind::Notch => "Notch",
            FilterKind::AllPass => "Allpass",
        }
    }
    pub fn has_gain(&self) -> bool {
        matches!(self, FilterKind::Peak | FilterKind::LowShelf | FilterKind::HighShelf)
    }
    pub const ALL: [FilterKind; 8] = [
        FilterKind::Peak,
        FilterKind::LowShelf,
        FilterKind::HighShelf,
        FilterKind::LowPass,
        FilterKind::HighPass,
        FilterKind::BandPass,
        FilterKind::Notch,
        FilterKind::AllPass,
    ];
    fn apo_tag(&self) -> &'static str {
        match self {
            FilterKind::Peak => "PK",
            FilterKind::LowShelf => "LSC",
            FilterKind::HighShelf => "HSC",
            FilterKind::LowPass => "LPQ",
            FilterKind::HighPass => "HPQ",
            FilterKind::BandPass => "BP",
            FilterKind::Notch => "NO",
            FilterKind::AllPass => "AP",
        }
    }
}

pub fn to_text(p: &Preset) -> String {
    let mut s = format!("Preamp: {:.2} dB
", p.preamp_db);
    for (i, b) in p.bands.iter().enumerate() {
        s += &format!("Filter {}: ON {} Fc {:.1} Hz", i + 1, b.kind.apo_tag(), b.freq);
        if b.kind.has_gain() {
            s += &format!(" Gain {:.1} dB", b.gain_db);
        }
        s += &format!(" Q {:.2}
", b.q);
    }
    s
}

pub fn load(path: &std::path::Path) -> anyhow::Result<Preset> {
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    let text = text.trim_start_matches('\u{feff}');
    let p = parse(text);
    anyhow::ensure!(!p.bands.is_empty(), "preset içinde geçerli filtre yok");
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "Preamp: -6.58 dB\nFilter 1: ON LSC Fc 105.0 Hz Gain -6.8 dB Q 0.70\nFilter 2: ON PK Fc 23.0 Hz Gain -1.9 dB Q 5.51\nFilter 10: ON HSC Fc 10000.0 Hz Gain 0.3 dB Q 0.70\n";

    #[test]
    fn parses_autoeq() {
        let p = parse(SAMPLE);
        assert_eq!(p.preamp_db, -6.58);
        assert_eq!(p.bands.len(), 3);
        assert_eq!(p.bands[0].kind, FilterKind::LowShelf);
        assert_eq!(p.bands[0].freq, 105.0);
        assert_eq!(p.bands[0].gain_db, -6.8);
        assert_eq!(p.bands[1].q, 5.51);
        assert_eq!(p.bands[2].kind, FilterKind::HighShelf);
    }

    #[test]
    fn roundtrip_text() {
        let p = parse(SAMPLE);
        let q = parse(&to_text(&p));
        assert_eq!(p.bands.len(), q.bands.len());
        assert!((p.bands[1].q - q.bands[1].q).abs() < 0.01);
    }

    #[test]
    fn skips_comments_and_off() {
        let p = parse("# x\nFilter 1: OFF\nFilter 2: ON PK Fc 1,000 Hz Gain 2 dB Q 1\n");
        assert_eq!(p.bands.len(), 1);
        assert_eq!(p.bands[0].freq, 1000.0);
    }
}
