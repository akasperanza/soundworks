use crate::apo::{self, Preset};
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, SystemTime};

const BASE: &str = "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results";

const INDEX_MAX_AGE_DAYS: u64 = 30;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {

    pub name: String,

    pub source: String,

    pub rig: String,

    pub form: String,

    pub path: String,
}

impl Entry {

    pub fn url(&self) -> String {
        let last = self.path.rsplit('/').next().unwrap_or("");
        format!("{BASE}/{}/{last}%20ParametricEQ.txt", self.path)
    }

    pub fn file_name(&self) -> String {
        let raw = format!("{} ({})", self.name, self.source);
        let clean: String = raw.chars().map(|c| if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') { '-' } else { c }).collect();
        format!("{}.txt", clean.trim())
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(v) = hex {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn parse_index(text: &str) -> Vec<Entry> {
    let mut out = vec![];
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("- [") else { continue };
        let Some(split) = rest.find("](./") else { continue };
        let after = &rest[split + 4..];

        let path_end = after.find(' ').unwrap_or(after.len());

        let tok = &after[..path_end];
        let path = tok.strip_suffix(')').unwrap_or(tok).to_string();
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() != 3 {
            continue;
        }
        let source = percent_decode(parts[0]);
        let mid = percent_decode(parts[1]);
        let name = percent_decode(parts[2]);
        let (rig, form) = ["over-ear", "in-ear", "earbud"]
            .iter()
            .find_map(|f| mid.strip_suffix(f).map(|r| (r.trim().to_string(), f.to_string())))
            .unwrap_or((String::new(), mid.clone()));
        out.push(Entry { name, source, rig, form, path });
    }
    out
}

pub fn device_query(name: &str) -> String {

    let inner = match (name.find('('), name.rfind(')')) {
        (Some(a), Some(b)) if b > a => &name[a + 1..b],
        _ => name,
    };
    let mut s = inner.trim().to_string();

    let digits = s.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let rest = s[digits..].trim_start();
        if let Some(r) = rest.strip_prefix('-') {
            s = r.trim().to_string();
        }
    }

    const NOISE: [&str; 12] = ["hands-free", "handsfree", "stereo", "a2dp", "headset", "headphones", "headphone", "kulaklıklar", "kulaklık", "ag audio", "bluetooth", "audio"];
    let mut words: Vec<&str> = s.split_whitespace().collect();
    words.retain(|w| !NOISE.contains(&w.to_lowercase().as_str()));
    words.join(" ")
}

fn words(s: &str) -> Vec<String> {
    s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(|w| w.to_string()).collect()
}

fn source_rank(source: &str) -> u32 {
    match source {
        "oratory1990" => 0,
        "crinacle" => 1,
        "Rtings" => 2,
        "Headphone.com Legacy" | "Innerfidelity" => 4,
        _ => 3,
    }
}

pub fn search(entries: &[Entry], query: &str, limit: usize) -> Vec<usize> {
    let q = words(query);
    if q.is_empty() {
        return vec![];
    }
    let mut hits: Vec<(i64, usize)> = vec![];
    for (i, e) in entries.iter().enumerate() {
        let w = words(&e.name);
        let mut score = 0i64;
        let mut ok = true;
        for t in &q {
            if w.iter().any(|x| x == t) {
                score += 10;
            } else if t.len() >= 3 && w.iter().any(|x| x.starts_with(t.as_str())) {
                score += 4;
            } else {
                ok = false;
                break;
            }
        }
        if !ok {
            continue;
        }

        let extra = w.iter().filter(|x| !q.iter().any(|t| *x == t || x.starts_with(t.as_str()))).count() as i64;
        score = score * 10 - extra * 6 - source_rank(&e.source) as i64;
        hits.push((score, i));
    }
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(entries[a.1].name.cmp(&entries[b.1].name)));
    hits.into_iter().take(limit).map(|(_, i)| i).collect()
}

#[derive(Clone, Debug, PartialEq)]
pub enum IndexState {
    Idle,
    Loading,

    Ready { cached: bool },
    Failed(String),
}

#[derive(Clone, Debug)]
pub enum Profile {
    Loading,
    Ready(Preset),
    Failed(String),
}

fn get_text(url: &str, max: u64) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(8)).timeout(Duration::from_secs(25)).build();
    let r = agent.get(url).set("User-Agent", "Soundworks/0.1").call().map_err(|e| match e {
        ureq::Error::Status(404, _) => "bu profil sunucuda bulunamadı".to_string(),
        ureq::Error::Status(code, _) => format!("sunucu {code} döndürdü"),
        _ => "bağlantı kurulamadı; internet bağlantını kontrol et".to_string(),
    })?;
    let mut s = String::new();
    r.into_reader().take(max).read_to_string(&mut s).map_err(|_| "yanıt okunamadı".to_string())?;
    Ok(s)
}

pub fn parse_profile(text: &str) -> Result<Preset, String> {
    let p = apo::parse(text);
    if p.bands.is_empty() {
        Err("profil okunamadı (bant bulunamadı)".to_string())
    } else {
        Ok(p)
    }
}

pub struct Client {
    cache: PathBuf,
    pub state: IndexState,
    pub entries: Vec<Entry>,
    rx_index: Option<Receiver<Result<String, String>>>,

    refreshing: bool,
    profiles: HashMap<String, Profile>,
    rx_profiles: Vec<(String, Receiver<Result<Preset, String>>)>,
}

impl Client {
    pub fn new(cache: PathBuf) -> Client {
        Client { cache, state: IndexState::Idle, entries: vec![], rx_index: None, refreshing: false, profiles: HashMap::new(), rx_profiles: vec![] }
    }

    fn cache_age_days(&self) -> Option<u64> {
        let m = std::fs::metadata(&self.cache).ok()?.modified().ok()?;
        Some(SystemTime::now().duration_since(m).ok()?.as_secs() / 86400)
    }

    fn spawn_index(&mut self) {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(get_text(&format!("{BASE}/INDEX.md"), 8 << 20));
        });
        self.rx_index = Some(rx);
    }

    pub fn ensure_index(&mut self) {
        if self.state != IndexState::Idle {
            return;
        }
        if let Ok(text) = std::fs::read_to_string(&self.cache) {
            let e = parse_index(&text);
            if !e.is_empty() {
                self.entries = e;
                self.state = IndexState::Ready { cached: true };
                if self.cache_age_days().map_or(true, |d| d >= INDEX_MAX_AGE_DAYS) {
                    self.refreshing = true;
                    self.spawn_index();
                }
                return;
            }
        }
        self.state = IndexState::Loading;
        self.spawn_index();
    }

    pub fn retry(&mut self) {
        if self.entries.is_empty() {
            self.state = IndexState::Loading;
        } else {
            self.refreshing = true;
        }
        self.spawn_index();
    }

    pub fn is_refreshing(&self) -> bool {
        self.refreshing
    }

    pub fn profile(&mut self, e: &Entry) -> &Profile {
        if !self.profiles.contains_key(&e.path) {
            self.profiles.insert(e.path.clone(), Profile::Loading);
            let (tx, rx) = channel();
            let url = e.url();
            std::thread::spawn(move || {
                let _ = tx.send(get_text(&url, 1 << 20).and_then(|t| parse_profile(&t)));
            });
            self.rx_profiles.push((e.path.clone(), rx));
        }
        &self.profiles[&e.path]
    }

    pub fn forget_profile(&mut self, e: &Entry) {
        self.profiles.remove(&e.path);
    }

    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        if let Some(rx) = &self.rx_index {
            match rx.try_recv() {
                Ok(Ok(text)) => {
                    let e = parse_index(&text);
                    if e.is_empty() {
                        if self.entries.is_empty() {
                            self.state = IndexState::Failed("indeks okunamadı".into());
                        }
                    } else {
                        let _ = crate::fsutil::write_atomic(&self.cache, text.as_bytes());
                        self.entries = e;
                        self.state = IndexState::Ready { cached: false };
                    }
                    self.rx_index = None;
                    self.refreshing = false;
                    changed = true;
                }
                Ok(Err(msg)) => {

                    if self.entries.is_empty() {
                        self.state = IndexState::Failed(msg);
                    }
                    self.rx_index = None;
                    self.refreshing = false;
                    changed = true;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.rx_index = None;
                    self.refreshing = false;
                }
                Err(_) => {}
            }
        }
        let mut i = 0;
        while i < self.rx_profiles.len() {
            match self.rx_profiles[i].1.try_recv() {
                Ok(res) => {
                    let (path, _) = self.rx_profiles.remove(i);
                    self.profiles.insert(path, match res {
                        Ok(p) => Profile::Ready(p),
                        Err(e) => Profile::Failed(e),
                    });
                    changed = true;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    let (path, _) = self.rx_profiles.remove(i);
                    self.profiles.insert(path, Profile::Failed("indirme tamamlanamadı".into()));
                    changed = true;
                }
                Err(_) => i += 1,
            }
        }
        changed
    }

    pub fn busy(&self) -> bool {
        self.rx_index.is_some() || !self.rx_profiles.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Index\nThis is a list.\n\n\
- [Sennheiser Momentum 4 Wireless](./oratory1990/over-ear/Sennheiser%20Momentum%204%20Wireless) by oratory1990\n\
- [Sennheiser Momentum 4 Wireless](./crinacle/GRAS%2043AG-7%20over-ear/Sennheiser%20Momentum%204%20Wireless) by crinacle on GRAS 43AG-7\n\
- [Sennheiser Momentum 4 Wireless (passive)](./crinacle/GRAS%2043AG-7%20over-ear/Sennheiser%20Momentum%204%20Wireless%20(passive)) by crinacle on GRAS 43AG-7\n\
- [Sennheiser Momentum 3 Wireless](./oratory1990/over-ear/Sennheiser%20Momentum%203%20Wireless) by oratory1990\n\
- [1MORE Aero (ANC Off)](./HypetheSonics/GRAS%20RA0045%20in-ear/1MORE%20Aero%20(ANC%20Off)) by HypetheSonics on GRAS RA0045\n\
- [Sony WH-1000XM5](./Rtings/Bruel%20&%20Kjaer%205128%20over-ear/Sony%20WH-1000XM5) by Rtings on Bruel & Kjaer 5128\n\
- [Sony WH-1000XM4](./oratory1990/over-ear/Sony%20WH-1000XM4) by oratory1990\n";

    #[test]
    fn index_lines_are_parsed_into_entries() {
        let e = parse_index(SAMPLE);
        assert_eq!(e.len(), 7);
        assert_eq!(e[0], Entry { name: "Sennheiser Momentum 4 Wireless".into(), source: "oratory1990".into(), rig: "".into(), form: "over-ear".into(), path: "oratory1990/over-ear/Sennheiser%20Momentum%204%20Wireless".into() });
        assert_eq!((e[1].source.as_str(), e[1].rig.as_str(), e[1].form.as_str()), ("crinacle", "GRAS 43AG-7", "over-ear"));
        assert_eq!(e[2].name, "Sennheiser Momentum 4 Wireless (passive)", "parantezli ad bozulmamalı");
        assert_eq!(e[4].rig, "GRAS RA0045");
        assert_eq!(e[5].rig, "Bruel & Kjaer 5128");
    }

    #[test]
    fn profile_url_and_file_name() {
        let e = &parse_index(SAMPLE)[2];
        assert_eq!(
            e.url(),
            "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results/crinacle/GRAS%2043AG-7%20over-ear/Sennheiser%20Momentum%204%20Wireless%20(passive)/Sennheiser%20Momentum%204%20Wireless%20(passive)%20ParametricEQ.txt"
        );
        assert_eq!(e.file_name(), "Sennheiser Momentum 4 Wireless (passive) (crinacle).txt");
        let weird = Entry { name: "A/B: \"x\"?".into(), source: "s".into(), rig: "".into(), form: "".into(), path: "s/f/n".into() };
        assert_eq!(weird.file_name(), "A-B- -x-- (s).txt");
    }

    #[test]
    fn device_names_become_model_names() {
        assert_eq!(device_query("Kulaklıklar (2- MOMENTUM 4)"), "MOMENTUM 4");
        assert_eq!(device_query("Kulaklıklar (MOMENTUM 4)"), "MOMENTUM 4");
        assert_eq!(device_query("Kulaklık (MOMENTUM 4 Hands-Free)"), "MOMENTUM 4");
        assert_eq!(device_query("Headphones (WH-1000XM5 Stereo)"), "WH-1000XM5");
        assert_eq!(device_query("Hoparlör (Realtek(R) Audio)"), "Realtek(R)");
        assert_eq!(device_query("Sennheiser HD 600"), "Sennheiser HD 600");
        assert_eq!(device_query(""), "");
    }

    #[test]
    fn search_finds_the_model_regardless_of_brand_prefix_or_case() {
        let e = parse_index(SAMPLE);
        let r = search(&e, "MOMENTUM 4", 10);
        let names: Vec<(&str, &str)> = r.iter().map(|&i| (e[i].name.as_str(), e[i].source.as_str())).collect();

        assert_eq!(names.len(), 3, "{names:?}");
        assert_eq!(names[0], ("Sennheiser Momentum 4 Wireless", "oratory1990"));
        assert_eq!(names[1], ("Sennheiser Momentum 4 Wireless", "crinacle"));
        assert!(names[2].0.contains("passive"));

        assert!(search(&e, "momentum 4", 10).iter().all(|&i| !e[i].name.contains("Momentum 3")));

        assert_eq!(search(&e, "wh-1000xm5", 10).len(), 1);
        assert_eq!(search(&e, "sony", 10).len(), 2);

        assert_eq!(search(&e, "momen", 10).len(), 4);

        assert!(search(&e, "   ", 10).is_empty());
        assert!(search(&e, "xyzzy", 10).is_empty());

        assert_eq!(search(&e, "sennheiser", 2).len(), 2);
    }

    #[test]
    fn real_profile_text_parses_with_shelves() {
        let t = "Preamp: -6.4 dB\nFilter 1: ON LSC Fc 105 Hz Gain -7.4 dB Q 0.70\nFilter 2: ON PK Fc 195 Hz Gain -3.6 dB Q 1.22\nFilter 3: ON HSC Fc 10000 Hz Gain 0.5 dB Q 0.70\n";
        let p = parse_profile(t).unwrap();
        assert_eq!(p.bands.len(), 3);
        assert!((p.preamp_db - -6.4).abs() < 1e-9);
        assert_eq!(p.bands[0].kind, apo::FilterKind::LowShelf);
        assert!(parse_profile("hello").is_err());
    }

    #[test]
    fn percent_decoding_handles_edges() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("x%2"), "x%2");
        assert_eq!(percent_decode("%aé"), "%aé", "çok baytlı karakter bozulmamalı");
        assert_eq!(percent_decode("%C3%A9"), "é");
    }

    #[test]
    #[ignore]
    fn live_autoeq_index_and_profile() {
        let text = get_text(&format!("{BASE}/INDEX.md"), 8 << 20).expect("indeks");
        let e = parse_index(&text);
        assert!(e.len() > 5000, "{}", e.len());
        let hits = search(&e, "MOMENTUM 4", 5);
        assert!(!hits.is_empty());
        let top = &e[hits[0]];
        println!("ilk sonuç: {} · {} · {}", top.name, top.source, top.url());
        let p = parse_profile(&get_text(&top.url(), 1 << 20).expect("profil")).expect("ayrıştır");
        println!("{} bant, preamp {}", p.bands.len(), p.preamp_db);
        assert!(p.bands.len() >= 3);
    }
}
