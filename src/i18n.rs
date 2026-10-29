use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

include!("i18n_data.rs");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Tr,
    En,
    De,
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::Tr, Lang::En, Lang::De];

    pub fn code(self) -> &'static str {
        match self {
            Lang::Tr => "tr",
            Lang::En => "en",
            Lang::De => "de",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.code() == code.trim().to_lowercase())
    }

    pub fn native_name(self) -> &'static str {
        match self {
            Lang::Tr => "Türkçe",
            Lang::En => "English",
            Lang::De => "Deutsch",
        }
    }
}

pub fn set(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
    memo().lock().unwrap().clear();
}

pub fn get() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Lang::En,
        2 => Lang::De,
        _ => Lang::Tr,
    }
}

pub fn from_primary_language_id(id: u16) -> Lang {
    match id & 0x3ff {
        0x1f => Lang::Tr,
        0x07 => Lang::De,
        _ => Lang::En,
    }
}

pub fn detect() -> Lang {
    let id = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
    from_primary_language_id(id)
}

struct Template {
    parts: Vec<String>,
    weight: usize,
    en: &'static str,
    de: &'static str,
}

struct Tables {
    exact: HashMap<&'static str, (&'static str, &'static str)>,
    templates: Vec<Template>,
}

fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| {
        let mut exact = HashMap::new();
        let mut templates = vec![];
        for &(tr, en, de) in ENTRIES {
            if tr.contains("{}") {
                let parts: Vec<String> = tr.split("{}").map(|s| s.to_string()).collect();
                let weight = parts.iter().map(|p| p.chars().count()).sum();
                templates.push(Template { parts, weight, en, de });
            } else {
                exact.insert(tr, (en, de));
            }
        }
        templates.sort_by_key(|t| std::cmp::Reverse(t.weight));
        Tables { exact, templates }
    })
}

fn memo() -> &'static Mutex<HashMap<(u8, String), String>> {
    static M: OnceLock<Mutex<HashMap<(u8, String), String>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

fn pick(lang: Lang, en: &str, de: &str) -> String {
    match lang {
        Lang::De => de.to_string(),
        _ => en.to_string(),
    }
}

fn captures<'a>(parts: &[String], s: &'a str) -> Option<Vec<&'a str>> {
    let first = &parts[0];
    let last = parts.last()?;
    if parts.len() < 2 || !s.starts_with(first.as_str()) || s.len() < first.len() + last.len() || !s.ends_with(last.as_str()) {
        return None;
    }
    let end = s.len() - last.len();
    let mut pos = first.len();
    let mut caps = Vec::with_capacity(parts.len() - 1);
    for mid in &parts[1..parts.len() - 1] {
        if mid.is_empty() {
            return None;
        }
        let found = s[pos..end].find(mid.as_str())? + pos;
        caps.push(&s[pos..found]);
        pos = found + mid.len();
    }
    if pos > end {
        return None;
    }
    caps.push(&s[pos..end]);
    Some(caps)
}

fn fill(template: &str, caps: &[String]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut next = 0usize;
    let mut rest = template;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if let Some(j) = tail.find('}') {
            let inner = &tail[1..j];
            let idx = if inner.is_empty() {
                let k = next;
                next += 1;
                Some(k)
            } else {
                inner.parse::<usize>().ok()
            };
            match idx.and_then(|k| caps.get(k)) {
                Some(c) => out.push_str(c),
                None => out.push_str(&tail[..=j]),
            }
            rest = &tail[j + 1..];
        } else {
            out.push_str(tail);
            rest = "";
        }
    }
    out.push_str(rest);
    out
}

fn translate_uncached(lang: Lang, s: &str) -> Option<String> {
    let t = tables();
    if let Some(&(en, de)) = t.exact.get(s) {
        return Some(pick(lang, en, de));
    }
    for tpl in &t.templates {
        if let Some(caps) = captures(&tpl.parts, s) {
            let caps: Vec<String> = caps
                .iter()
                .map(|c| match t.exact.get(*c) {
                    Some(&(en, de)) => pick(lang, en, de),
                    None => c.to_string(),
                })
                .collect();
            return Some(fill(if lang == Lang::De { tpl.de } else { tpl.en }, &caps));
        }
    }
    None
}

pub fn tr(s: impl AsRef<str>) -> String {
    let s = s.as_ref();
    let lang = get();
    if lang == Lang::Tr || s.is_empty() {
        return s.to_string();
    }
    let key = (lang as u8, s.to_string());
    if let Some(v) = memo().lock().unwrap().get(&key) {
        return v.clone();
    }
    let out = translate_uncached(lang, s).unwrap_or_else(|| {
        log_miss(s);
        s.to_string()
    });
    let mut m = memo().lock().unwrap();
    if m.len() > 8192 {
        m.clear();
    }
    m.insert(key, out.clone());
    out
}

#[cfg(feature = "devtools")]
fn log_miss(s: &str) {
    use std::io::Write;
    if let Ok(path) = std::env::var("SW_I18N_MISS") {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{}", s.replace('\n', "\\n"));
        }
    }
}

#[cfg(not(feature = "devtools"))]
fn log_miss(_: &str) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as TestLock;

    static GUARD: TestLock<()> = TestLock::new(());

    fn with_lang<R>(lang: Lang, f: impl FnOnce() -> R) -> R {
        let _g = GUARD.lock().unwrap_or_else(|e| e.into_inner());
        set(lang);
        let r = f();
        set(Lang::Tr);
        r
    }

    #[test]
    fn turkish_is_the_source_and_passes_through() {
        with_lang(Lang::Tr, || {
            assert_eq!(tr("Kaydet"), "Kaydet");
            assert_eq!(tr("bilinmeyen metin"), "bilinmeyen metin");
        });
    }

    #[test]
    fn exact_entries_translate_per_language() {
        with_lang(Lang::En, || assert_eq!(tr("Kaydet"), "Save"));
        with_lang(Lang::De, || assert_eq!(tr("Kaydet"), "Speichern"));
    }

    #[test]
    fn unknown_text_is_left_alone() {
        with_lang(Lang::De, || assert_eq!(tr("Sanatçı Adı 123"), "Sanatçı Adı 123"));
    }

    #[test]
    fn captures_split_on_literal_parts() {
        let parts = vec!["".to_string(), " albüm · ".to_string(), " parça".to_string()];
        assert_eq!(captures(&parts, "12 albüm · 340 parça"), Some(vec!["12", "340"]));
        assert_eq!(captures(&parts, "12 albüm · 340"), None);
        assert_eq!(captures(&parts, "albüm · parça"), None);
    }

    #[test]
    fn fill_supports_sequential_and_indexed_placeholders() {
        let caps = vec!["A".to_string(), "B".to_string()];
        assert_eq!(fill("{} then {}", &caps), "A then B");
        assert_eq!(fill("{1} before {0}", &caps), "B before A");
        assert_eq!(fill("no placeholders", &caps), "no placeholders");
        assert_eq!(fill("keep {9} unknown", &caps), "keep {9} unknown");
    }

    #[test]
    fn language_ids_map_to_supported_languages() {
        assert_eq!(from_primary_language_id(0x041f), Lang::Tr);
        assert_eq!(from_primary_language_id(0x0407), Lang::De);
        assert_eq!(from_primary_language_id(0x0c07), Lang::De);
        assert_eq!(from_primary_language_id(0x0409), Lang::En);
        assert_eq!(from_primary_language_id(0x040c), Lang::En);
    }

    #[test]
    fn language_codes_roundtrip() {
        for l in Lang::ALL {
            assert_eq!(Lang::from_code(l.code()), Some(l));
        }
        assert_eq!(Lang::from_code(" DE "), Some(Lang::De));
        assert_eq!(Lang::from_code("fr"), None);
    }

    #[test]
    fn every_template_keeps_the_same_placeholder_count_in_all_languages() {
        for &(tr, en, de) in ENTRIES {
            let n = tr.matches("{}").count();
            let max_idx = |s: &str| -> usize {
                let mut seq = 0usize;
                let mut hi = 0usize;
                let mut rest = s;
                while let Some(i) = rest.find('{') {
                    let tail = &rest[i..];
                    let Some(j) = tail.find('}') else { break };
                    let inner = &tail[1..j];
                    let k = if inner.is_empty() {
                        let k = seq;
                        seq += 1;
                        k
                    } else if let Ok(k) = inner.parse::<usize>() {
                        k
                    } else {
                        rest = &tail[j + 1..];
                        continue;
                    };
                    hi = hi.max(k + 1);
                    rest = &tail[j + 1..];
                }
                hi
            };
            if n > 0 {
                assert!(max_idx(en) <= n && max_idx(de) <= n, "yer tutucu sayısı uyuşmuyor: {tr}");
            }
        }
    }
}
