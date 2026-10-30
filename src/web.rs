use crate::artist::ArtistInfo;
use crate::lyrics::{LyricLine, LyricWord, Lyrics};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const TIMEOUT: u64 = 9;

fn re(cell: &'static OnceLock<Regex>, pat: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pat).expect("regex"))
}

fn agent(secs: u64) -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(8)).timeout(Duration::from_secs(secs)).build()
}

fn http_get_ua(url: &str, ua: &str, secs: u64) -> Result<String, String> {
    let r = agent(secs).get(url).set("User-Agent", ua).call().map_err(|e| e.to_string())?;
    let mut s = String::new();
    r.into_reader().take(8 << 20).read_to_string(&mut s).map_err(|e| e.to_string())?;
    Ok(s)
}

fn http_get(url: &str) -> Result<String, String> {
    http_get_ua(url, "BetterLyrics/1.0", TIMEOUT)
}

fn http_bytes(url: &str, max: u64) -> Result<Vec<u8>, String> {
    let r = agent(20).get(url).set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Soundworks/0.1").call().map_err(|e| e.to_string())?;
    let mut b = vec![];
    r.into_reader().take(max).read_to_end(&mut b).map_err(|e| e.to_string())?;
    Ok(b)
}

fn enc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

fn ratio(a: &str, b: &str) -> f64 {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    fn matching(a: &[char], b: &[char]) -> usize {
        if a.is_empty() || b.is_empty() {
            return 0;
        }
        let (mut best, mut ai, mut bi) = (0usize, 0usize, 0usize);
        let mut prev = vec![0usize; b.len() + 1];
        for i in 0..a.len() {
            let mut cur = vec![0usize; b.len() + 1];
            for j in 0..b.len() {
                if a[i] == b[j] {
                    cur[j + 1] = prev[j] + 1;
                    if cur[j + 1] > best {
                        best = cur[j + 1];
                        ai = i + 1 - best;
                        bi = j + 1 - best;
                    }
                }
            }
            prev = cur;
        }
        if best == 0 {
            return 0;
        }
        best + matching(&a[..ai], &b[..bi]) + matching(&a[ai + best..], &b[bi + best..])
    }
    2.0 * matching(&a, &b) as f64 / (a.len() + b.len()) as f64
}

fn fold_char(c: char) -> Option<char> {
    Some(match c {
        'ı' | 'İ' | 'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Î' => 'i',
        'ç' | 'Ç' | 'ć' | 'č' => 'c',
        'ş' | 'Ş' | 'š' | 'ś' => 's',
        'ğ' | 'Ğ' => 'g',
        'ö' | 'Ö' | 'ó' | 'ò' | 'ô' | 'õ' | 'ø' => 'o',
        'ü' | 'Ü' | 'ú' | 'ù' | 'û' => 'u',
        'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'Â' | 'Ä' | 'Á' => 'a',
        'é' | 'è' | 'ê' | 'ë' | 'É' => 'e',
        'ñ' => 'n',
        'ž' | 'ź' | 'ż' => 'z',
        'ł' => 'l',
        '\u{300}'..='\u{36f}' => return None,
        _ => c.to_lowercase().next().unwrap_or(c),
    })
}

fn norm_txt(s: &str) -> String {
    static A: OnceLock<Regex> = OnceLock::new();
    static B: OnceLock<Regex> = OnceLock::new();
    static C: OnceLock<Regex> = OnceLock::new();
    static D: OnceLock<Regex> = OnceLock::new();
    let s: String = s.chars().filter_map(fold_char).collect();
    let s = re(&A, r"[\(\[\{].*?[\)\]\}]").replace_all(&s, " ");
    let s = re(&B, r"(feat|ft|prod).*").replace_all(&s, " ");
    let s = re(&C, r"[^a-z0-9 ]+").replace_all(&s, " ");
    re(&D, r"\s+").replace_all(&s, " ").trim().to_string()
}

fn clean_artist(artist: &str) -> String {
    static A: OnceLock<Regex> = OnceLock::new();
    static B: OnceLock<Regex> = OnceLock::new();
    static C: OnceLock<Regex> = OnceLock::new();
    let a = re(&A, r"(?i)\s*[-–—]\s*Topic$").replace(artist, "");
    let a = re(&B, r"(?i)VEVO$").replace(&a, "");
    re(&C, r"(?i)\s+Official$").replace(&a, "").trim().to_string()
}

fn tag_suffix(s: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"(?i)^(?:remaster(?:ed)?|live|official.*|audio|video|music\s*video|lyric.*|visualizer|version|edit|mix|radio\s*edit|acoustic|instrumental|explicit|clean|\d{4}\b.*)").is_match(s)
}

fn strip_decor(title: &str) -> String {
    static A: OnceLock<Regex> = OnceLock::new();
    static B: OnceLock<Regex> = OnceLock::new();
    static C: OnceLock<Regex> = OnceLock::new();
    let t = re(&A, r"\s*[\(\[\{].*?[\)\]\}]").replace_all(title, "");
    let t = re(&B, r"(?i)\s+(?:feat|ft|prod)\.?\s+.*").replace_all(&t, "");
    re(&C, r"(?s)\s*\|\s*.*").replace_all(&t, "").trim().to_string()
}

fn split_dash(t: &str) -> Vec<String> {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"\s+[-–—]\s+").split(t).map(|s| s.to_string()).collect()
}

fn clean_title(title: &str, artist: &str) -> String {
    if title.is_empty() {
        return String::new();
    }
    let mut t = strip_decor(title);
    let parts = split_dash(&t);
    if parts.len() >= 2 {
        let left = parts[0].trim().to_string();
        let right = parts[1..].join(" - ").trim().to_string();
        let ca = clean_artist(artist).to_lowercase();
        if tag_suffix(&right) {
            t = left;
        } else if !ca.is_empty() && (left.to_lowercase() == ca || ca.contains(&left.to_lowercase()) || left.to_lowercase().contains(&ca)) {
            t = right;
        }
    }
    let t = t.trim().to_string();
    if t.is_empty() { title.trim().to_string() } else { t }
}

fn alt_artist_title(title: &str, artist: &str) -> Option<(String, String)> {
    if title.is_empty() {
        return None;
    }
    let t = strip_decor(title);
    let parts = split_dash(&t);
    if parts.len() == 2 {
        let (left, right) = (parts[0].trim(), parts[1].trim());
        if !left.is_empty() && !right.is_empty() && !tag_suffix(right) {
            let ca = clean_artist(artist).to_lowercase();
            if ca.is_empty() || left.to_lowercase() != ca {
                return Some((left.to_string(), right.to_string()));
            }
        }
    }
    None
}

fn title_ok(req: &str, cand: &str) -> bool {
    let a = norm_txt(&clean_title(req, ""));
    let b = norm_txt(&clean_title(cand, ""));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b {
        return true;
    }
    let r = ratio(&a, &b);
    if a.chars().count() >= 4 && (b.contains(&a) || a.contains(&b)) {
        return r >= 0.7;
    }
    r >= 0.86
}

fn artist_ok(req: &str, cand: &str) -> bool {
    let a = norm_txt(&clean_artist(req));
    if a.is_empty() {
        return true;
    }
    let b = norm_txt(cand);
    if b.is_empty() || a.contains(&b) || b.contains(&a) {
        return true;
    }
    let words = |s: &str| -> std::collections::HashSet<String> { s.split(' ').filter(|w| w.chars().count() >= 3).map(String::from).collect() };
    let (ta, tb) = (words(&a), words(&b));
    ta.intersection(&tb).next().is_some() || ratio(&a, &b) >= 0.6
}

fn match_ok(rt: &str, ra: &str, rd: f64, ct: &str, ca: &str, cd: f64) -> bool {
    let dur_ok = rd <= 0.0 || cd <= 0.0 || (rd - cd).abs() <= 25.0;
    title_ok(rt, ct) && artist_ok(ra, ca) && dur_ok
}

fn ttml_ms(val: &str) -> i64 {
    let s = val.trim().to_lowercase();
    if s.is_empty() {
        return 0;
    }
    let f = |x: &str| x.parse::<f64>().unwrap_or(0.0);
    if let Some(ms) = s.strip_suffix("ms") {
        return f(ms).round() as i64;
    }
    let s = if s.ends_with('s') && !s.contains(':') { &s[..s.len() - 1] } else { &s[..] };
    let p: Vec<&str> = s.split(':').collect();
    match p.len() {
        1 => (f(p[0]) * 1000.0).round() as i64,
        2 => ((f(p[0]) * 60.0 + f(p[1])) * 1000.0).round() as i64,
        3 => ((f(p[0]) * 3600.0 + f(p[1]) * 60.0 + f(p[2])) * 1000.0).round() as i64,
        _ => 0,
    }
}

fn strip_tags(s: &str) -> String {
    static A: OnceLock<Regex> = OnceLock::new();
    static B: OnceLock<Regex> = OnceLock::new();
    let t = re(&A, r"<[^>]+>").replace_all(s, "");
    re(&B, r"\s+").replace_all(&t, " ").trim().to_string()
}

fn html_unescape(s: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"&(#x[0-9a-fA-F]+|#\d+|[a-zA-Z]+);")
        .replace_all(s, |c: &regex::Captures| {
            let e = &c[1];
            let ch = if let Some(h) = e.strip_prefix("#x") {
                u32::from_str_radix(h, 16).ok().and_then(char::from_u32)
            } else if let Some(d) = e.strip_prefix('#') {
                d.parse().ok().and_then(char::from_u32)
            } else {
                match e {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some(' '),
                    _ => None,
                }
            };
            ch.map(String::from).unwrap_or_else(|| c[0].to_string())
        })
        .into_owned()
}

fn split_bg(body: &str) -> (String, Vec<String>) {
    static OPEN: OnceLock<Regex> = OnceLock::new();
    static TAG: OnceLock<Regex> = OnceLock::new();
    let opener = re(&OPEN, r#"(?i)<span\s+[^>]*ttm:role="x-bg"[^>]*>"#);
    let tag = re(&TAG, r"(?i)<span\b|</span>");
    let (mut out, mut bgs, mut i) = (String::new(), vec![], 0usize);
    loop {
        let Some(m) = opener.find_at(body, i) else {
            out.push_str(&body[i..]);
            break;
        };
        out.push_str(&body[i..m.start()]);
        let (mut j, mut depth) = (m.end(), 1);
        let inner_start = m.end();
        let mut inner_end = m.end();
        while depth > 0 {
            match tag.find_at(body, j) {
                None => {
                    inner_end = body.len();
                    j = body.len();
                    break;
                }
                Some(mo) => {
                    if mo.as_str().starts_with("</") {
                        depth -= 1;
                        if depth == 0 {
                            inner_end = mo.start();
                        }
                    } else {
                        depth += 1;
                    }
                    j = mo.end();
                }
            }
        }
        bgs.push(body[inner_start..inner_end].to_string());
        out.push(' ');
        i = j;
    }
    (out, bgs)
}

fn words_from(body: &str, bg: bool) -> Vec<LyricWord> {
    static R: OnceLock<Regex> = OnceLock::new();
    let r = re(&R, r#"(?i)<span\s+[^>]*begin="([^"]+)"[^>]*end="([^"]+)"[^>]*>([^<]+)</span>(\s*)"#);
    let mut words = vec![];
    for c in r.captures_iter(body) {
        let (ws, we) = (ttml_ms(&c[1]), ttml_ms(&c[2]));
        let raw = &c[3];
        let mut wt = raw.trim().to_string();
        if bg {
            wt = wt.trim_matches(|ch| ch == '(' || ch == ')').trim().to_string();
        }
        if !wt.is_empty() {
            words.push(LyricWord { t: ws.max(0) as u32, d: (we - ws).max(100) as u32, text: wt, sp: !c[4].is_empty() || raw.ends_with(' '), bg });
        }
    }
    words
}

fn parse_ttml(text: &str) -> (Vec<LyricLine>, bool) {
    static P: OnceLock<Regex> = OnceLock::new();
    let pr = re(&P, r#"(?is)<p\s+[^>]*begin="([^"]+)"[^>]*>(.*?)</p>"#);
    let mut lines: Vec<LyricLine> = vec![];
    let mut has_words = false;
    for pm in pr.captures_iter(text) {
        let start = ttml_ms(&pm[1]).max(0) as u32;
        let (main_body, bg_bodies) = split_bg(&pm[2]);
        let words = words_from(&main_body, false);
        let mut bg_words: Vec<LyricWord> = vec![];
        for bb in &bg_bodies {
            let mut bw = words_from(bb, true);
            if bw.is_empty() {
                let t = strip_tags(bb);
                let t = t.trim_matches(|c| c == '(' || c == ')').trim().to_string();
                if !t.is_empty() {
                    bw = vec![LyricWord { t: start, d: 600, text: t, sp: false, bg: true }];
                }
            }
            bg_words.extend(bw);
        }
        let main_text = strip_tags(&main_body);
        let bg_text = bg_words.iter().map(|w| w.text.clone()).collect::<Vec<_>>().join(" ");
        let line_text = if main_text.is_empty() { bg_text } else { main_text };
        if line_text.is_empty() {
            continue;
        }
        let mut all = words;
        all.extend(bg_words);
        if !all.is_empty() {
            has_words = true;
        }
        lines.push(LyricLine { t: start, text: line_text, words: all });
    }
    lines.sort_by_key(|l| l.t);

    if lines.len() > 2 && lines.iter().all(|l| l.t == lines[0].t) {
        return (vec![], false);
    }
    let mut merged: Vec<LyricLine> = vec![];
    for ln in lines {
        match merged.last_mut() {
            Some(last) if (last.t as i64 - ln.t as i64).abs() < 100 => {
                if !last.text.contains(&ln.text) {
                    last.text.push(' ');
                    last.text.push_str(&ln.text);
                    last.words.extend(ln.words);
                }
            }
            _ => merged.push(ln),
        }
    }
    (merged, has_words)
}

fn jint(v: Option<&Value>, default: i64) -> i64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().map(|f| f as i64).unwrap_or(default),
        Some(Value::String(s)) => s.trim().parse::<f64>().map(|f| f as i64).unwrap_or(default),
        _ => default,
    }
}

fn first_of<'a>(o: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().filter_map(|k| o.get(*k)).find(|v| !v.is_null() && *v != &Value::from(0) && *v != &Value::from(""))
}

fn lrc_ms(m: &str, s: &str, frac: &str) -> u32 {
    let f: f64 = format!("0.{frac}").parse().unwrap_or(0.0);
    (m.parse::<u32>().unwrap_or(0) * 60000 + s.parse::<u32>().unwrap_or(0) * 1000) + (f * 1000.0).round() as u32
}

fn parse_input(inp: &str, allow_plain: bool) -> (Vec<LyricLine>, bool) {
    if inp.trim().is_empty() {
        return (vec![], false);
    }
    if inp.contains("<tt") || inp.contains("<p begin=") {
        return parse_ttml(inp);
    }
    if inp.trim_start().starts_with('{') {
        if let Ok(obj) = serde_json::from_str::<Value>(inp) {
            if let Some(t) = obj.get("ttml").and_then(|x| x.as_str()) {
                return parse_ttml(t);
            }
            let lst = ["lines", "lyrics", "data", "result"].iter().filter_map(|k| obj.get(*k)).find_map(|v| v.as_array().filter(|a| !a.is_empty()));
            if let Some(lst) = lst {
                let (mut lines, mut has_words) = (vec![], false);
                for item in lst.iter().filter(|i| i.is_object()) {
                    let t = jint(first_of(item, &["startTimeMs", "time", "begin"]), 0);
                    let text = ["text", "words", "line"].iter().find_map(|k| item.get(*k).and_then(|x| x.as_str())).unwrap_or("").trim().to_string();
                    let sub = ["syllables", "wordsArray", "lead", "words_list"].iter().find_map(|k| item.get(*k).and_then(|x| x.as_array()).filter(|a| !a.is_empty()));
                    if let Some(sub) = sub {
                        has_words = true;
                        let words = sub
                            .iter()
                            .enumerate()
                            .map(|(i, w)| {
                                let wt = jint(first_of(w, &["startTimeMs", "time", "begin"]), t);
                                let nt = match sub.get(i + 1) {
                                    Some(n) => jint(first_of(n, &["startTimeMs", "time", "begin"]), wt + 500),
                                    None => wt + 500,
                                };
                                let nt = if nt == 0 { wt + 500 } else { nt };
                                LyricWord {
                                    t: wt.max(0) as u32,
                                    d: (nt - wt).max(200) as u32,
                                    text: ["text", "word"].iter().find_map(|k| w.get(*k).and_then(|x| x.as_str())).unwrap_or("").to_string(),
                                    sp: true,
                                    bg: false,
                                }
                            })
                            .collect();
                        lines.push(LyricLine { t: t.max(0) as u32, text, words });
                    } else if !text.is_empty() {
                        lines.push(LyricLine { t: t.max(0) as u32, text, words: vec![] });
                    }
                }
                if !lines.is_empty() {
                    lines.sort_by_key(|l| l.t);
                    return (lines, has_words);
                }
            }
        }
    }
    static LINE: OnceLock<Regex> = OnceLock::new();
    static WORD: OnceLock<Regex> = OnceLock::new();
    static CLEAN: OnceLock<Regex> = OnceLock::new();
    let line_re = re(&LINE, r"\[(\d{2}):(\d{2})[\.:](\d{2,3})\](.*)");
    let word_re = re(&WORD, r"[<\(\[](\d{2}):(\d{2})[\.:](\d{2,3})[>\)\]]([^<\(\[\n]*)");
    let clean_re = re(&CLEAN, r"[<\(\[].*?[>\)\]]");
    let (mut out, mut has_words) = (vec![], false);
    for raw in inp.split('\n') {
        let Some(m) = line_re.captures(raw) else { continue };
        let t = lrc_ms(&m[1], &m[2], &m[3]);
        let body = m[4].trim().to_string();
        let mut words: Vec<LyricWord> = word_re
            .captures_iter(&body)
            .map(|wm| LyricWord { t: lrc_ms(&wm[1], &wm[2], &wm[3]), d: 0, text: wm[4].to_string(), sp: wm[4].len() != wm[4].trim_end().len(), bg: false })
            .collect();
        if !words.is_empty() {
            has_words = true;
            let starts: Vec<u32> = words.iter().map(|w| w.t).collect();
            for (i, w) in words.iter_mut().enumerate() {
                let nxt = starts.get(i + 1).copied().unwrap_or(w.t + 1500);
                w.d = nxt.saturating_sub(w.t).max(100);
                w.text = w.text.trim().to_string();
            }
            words.retain(|w| !w.text.is_empty());
            out.push(LyricLine { t, text: clean_re.replace_all(&body, "").trim().to_string(), words });
        } else {
            out.push(LyricLine { t, text: body, words: vec![] });
        }
    }
    let mut lines: Vec<LyricLine> = out.into_iter().filter(|l| !l.text.is_empty()).collect();
    lines.sort_by_key(|l| l.t);
    if lines.is_empty() && allow_plain {
        let plain: Vec<&str> = inp.split('\n').map(|l| l.trim()).filter(|l| !l.is_empty() && !l.starts_with('<') && !l.starts_with('{')).collect();
        return (plain.iter().enumerate().map(|(i, l)| LyricLine { t: i as u32 * 4000, text: l.to_string(), words: vec![] }).collect(), false);
    }
    (lines, has_words)
}

struct Found {
    provider: &'static str,
    kind: &'static str,
    lines: Vec<LyricLine>,
}

fn found(provider: &'static str, lines: Vec<LyricLine>, words: bool, synced: bool) -> Option<Found> {
    if lines.is_empty() {
        return None;
    }
    Some(Found { provider, kind: if words { "word" } else if synced { "synced" } else { "plain" }, lines })
}

fn from_raw(provider: &'static str, raw: &str) -> Option<Found> {
    let (lines, words) = parse_input(raw, false);
    found(provider, lines, words, true)
}

fn lrcred_like(provider: &'static str, url: &str, req: Option<(&str, &str, f64)>) -> Option<Found> {
    let raw = http_get(url).ok()?;
    if let Some(r) = from_raw(provider, &raw) {
        return Some(r);
    }
    let data: Value = serde_json::from_str(&raw).ok()?;
    let mut results: Vec<Value> = data.get("results")?.as_array()?.clone();
    if results.is_empty() {
        return None;
    }
    if let Some((rt, ra, rd)) = req {
        let s = |x: &Value, a: &str, b: &str| x.get(a).or_else(|| x.get(b)).and_then(|v| v.as_str()).unwrap_or("").to_string();
        results.retain(|x| match_ok(rt, ra, rd, &s(x, "track_name", "title"), &s(x, "artist_name", "artist"), x.get("duration").and_then(|d| d.as_f64()).unwrap_or(0.0)));
    }
    let best = results.iter().find(|x| x.get("timing_type").and_then(|t| t.as_str()) == Some("word")).or(results.first())?;
    let u = best.get("lyricsUrl")?.as_str()?;
    from_raw(provider, &http_get(u).ok()?)
}

fn query_of(title: &str, artist: &str) -> (String, String) {
    let ca = clean_artist(artist);
    match alt_artist_title(title, &ca) {
        Some((a, t)) => (t, a),
        None => {
            let t = clean_title(title, &ca);
            (if t.is_empty() { title.to_string() } else { t }, ca)
        }
    }
}

fn dur_param(d: f64) -> String {
    if d > 0.0 { format!("&d={}", d.round() as i64) } else { String::new() }
}

fn apple_music(title: &str, artist: &str, album: &str, duration: f64) -> Option<Found> {
    let (qt, qa) = query_of(title, artist);
    let query = format!("{qt} {qa}").trim().to_string();
    if let Some(r) = lrcred_like("Apple Music", &format!("https://lrc.red/api/v1?q={}", enc(&query)), Some((&qt, &qa, duration))) {
        return Some(r);
    }
    let mut url = format!("https://lyrics-api.boidu.dev/getLyrics?s={}&a={}", enc(&qt), enc(&qa));
    if !album.is_empty() {
        url.push_str(&format!("&al={}", enc(album)));
    }
    url.push_str(&dur_param(duration));
    from_raw("Apple Music", &http_get(&url).ok()?)
}

fn binilyrics(title: &str, artist: &str, album: &str, duration: f64) -> Option<Found> {
    let (qt, qa) = query_of(title, artist);
    let mut url = format!("https://lyrics-api.binimum.org/getLyrics?s={}&a={}", enc(&qt), enc(&qa));
    if !album.is_empty() {
        url.push_str(&format!("&al={}", enc(album)));
    }
    url.push_str(&dur_param(duration));
    if let Some(r) = lrcred_like("BiniLyrics", &url, Some((&qt, &qa, duration))) {
        return Some(r);
    }
    let q = format!("{qt} {qa}");
    lrcred_like("BiniLyrics", &format!("https://lyrics-api.binimum.org/getLyrics?q={}", enc(q.trim())), Some((&qt, &qa, duration)))
}

fn lrclib_candidates(title: &str, artist: &str, duration: f64) -> Vec<Value> {
    let q = format!("{artist} {title}");
    let q = q.trim();
    if q.is_empty() {
        return vec![];
    }
    let Ok(raw) = http_get_ua(&format!("https://lrclib.net/api/search?q={}", enc(q)), "Soundworks/0.1", TIMEOUT) else { return vec![] };
    let Ok(Value::Array(data)) = serde_json::from_str::<Value>(&raw) else { return vec![] };
    if duration > 0.0 {
        let close: Vec<Value> = data.iter().filter(|d| (d.get("duration").and_then(|x| x.as_f64()).unwrap_or(0.0) - duration).abs() <= 15.0).cloned().collect();
        if !close.is_empty() {
            return close;
        }
    }
    data
}

fn lrclib(title: &str, artist: &str, album: &str, duration: f64) -> Option<Found> {
    let ca = clean_artist(artist);
    let alt = alt_artist_title(title, &ca);
    let qt = clean_title(title, &ca);
    let qt = if qt.is_empty() { title.to_string() } else { qt };
    let mut items: Vec<Value> = vec![];
    let accept = |x: &Value, items: &[Value]| -> bool {
        let Some(id) = x.get("id").filter(|i| !i.is_null()) else { return false };
        if items.iter().any(|i| i.get("id") == Some(id)) {
            return false;
        }
        let ct = x.get("trackName").or_else(|| x.get("name")).and_then(|v| v.as_str()).unwrap_or("");
        let cart = x.get("artistName").and_then(|v| v.as_str()).unwrap_or("");
        let cd = x.get("duration").and_then(|v| v.as_f64()).unwrap_or(0.0);
        match_ok(&qt, &ca, duration, ct, cart, cd) || alt.as_ref().is_some_and(|(a, t)| match_ok(t, a, duration, ct, cart, cd))
    };
    macro_rules! add {
        ($x:expr) => {{
            let x: Value = $x;
            if accept(&x, &items) {
                items.push(x);
            }
        }};
    }
    if duration > 0.0 {
        let url = format!(
            "https://lrclib.net/api/get?track_name={}&artist_name={}&album_name={}&duration={}",
            enc(&qt),
            enc(&ca),
            enc(album),
            duration.round() as i64
        );
        if let Some(v) = http_get_ua(&url, "Soundworks/0.1", TIMEOUT).ok().and_then(|r| serde_json::from_str::<Value>(&r).ok()) {
            add!(v);
        }
    }
    for c in lrclib_candidates(&qt, &ca, duration) {
        add!(c);
    }
    if let Some((a, t)) = &alt {
        for c in lrclib_candidates(t, a, duration) {
            add!(c);
        }
    }
    let empty = items.is_empty();
    if empty && !qt.is_empty() {
        for c in lrclib_candidates(&qt, "", duration) {
            add!(c);
        }
    }
    let score = |it: &Value| -> f64 {
        match it.get("duration").and_then(|d| d.as_f64()) {
            Some(d) if duration > 0.0 && d > 0.0 => -(d - duration).abs(),
            _ => 0.0,
        }
    };
    let best = items
        .iter()
        .filter(|i| i.get("syncedLyrics").and_then(|s| s.as_str()).is_some_and(|s| !s.is_empty()))
        .fold(None::<&Value>, |b, it| match b {
            Some(cur) if score(cur) >= score(it) => Some(cur),
            _ => Some(it),
        });
    if let Some(b) = best {
        let (lines, words) = parse_input(b["syncedLyrics"].as_str().unwrap_or(""), false);
        if let Some(f) = found("LRCLib", lines, words, true) {
            return Some(f);
        }
    }
    for it in &items {
        if let Some(p) = it.get("plainLyrics").and_then(|s| s.as_str()).filter(|s| !s.is_empty()) {
            let (lines, _) = parse_input(p, true);
            if let Some(f) = found("LRCLib", lines, false, false) {
                return Some(f);
            }
        }
    }
    None
}

pub fn lyrics_lookup(title: &str, artist: &str, album: &str, duration: f64) -> Lyrics {
    let mut out = Lyrics { kind: String::new(), provider: String::new(), lines: vec![], ver: 4 };
    if title.trim().is_empty() && artist.trim().is_empty() {
        return out;
    }
    type Provider = fn(&str, &str, &str, f64) -> Option<Found>;
    let funcs: [Provider; 3] = [apple_music, binilyrics, lrclib];
    let results: Vec<Found> = std::thread::scope(|s| {
        let hs: Vec<_> = funcs.iter().map(|f| s.spawn(move || f(title, artist, album, duration))).collect();
        hs.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
    });
    for kind in ["word", "synced", "plain"] {
        let group: Vec<&Found> = results.iter().filter(|r| r.kind == kind).collect();
        if let Some(chosen) = group.iter().find(|r| r.provider == "Apple Music").or(group.first()) {
            out.kind = kind.to_string();
            out.provider = chosen.provider.to_string();
            out.lines = chosen.lines.clone();
            break;
        }
    }
    out
}

fn looks_musical(text: &str) -> bool {
    const W: [&str; 16] = [
        "müzik", "şarkıcı", "rapçi", "rap ", "hip hop", "hip-hop", "grup", "sanatçı", "besteci", "albüm", "söz yazar", "gitarist", "orkestra", "singer", "band", "musician",
    ];
    let low = text.to_lowercase();
    W.iter().any(|w| low.contains(w)) || ["rapper", "album", "songwriter", "composer", "vocalist"].iter().any(|w| low.contains(w))
}

fn wiki_summary(name: &str) -> String {
    for lang in ["tr", "en"] {
        let title = enc(&name.replace(' ', "_"));
        let Ok(raw) = http_get_ua(&format!("https://{lang}.wikipedia.org/api/rest_v1/page/summary/{title}"), "Soundworks/0.1", 20) else { continue };
        let Ok(d) = serde_json::from_str::<Value>(&raw) else { continue };
        if d["type"] == "standard" {
            if let Some(x) = d["extract"].as_str().filter(|x| !x.is_empty() && looks_musical(x)) {
                return x.to_string();
            }
        }
    }
    String::new()
}

static LAST_LFM: Mutex<Option<Instant>> = Mutex::new(None);

fn lastfm_bio(name: &str) -> String {
    {
        let mut g = LAST_LFM.lock().unwrap();
        if let Some(t) = *g {
            let wait = Duration::from_millis(500).saturating_sub(t.elapsed());
            if !wait.is_zero() {
                std::thread::sleep(wait);
            }
        }
        *g = Some(Instant::now());
    }
    let slug = enc(&name.trim().replace(' ', "+")).replace("%2B", "+");
    let page = match agent(20)
        .get(&format!("https://www.last.fm/music/{slug}/+wiki"))
        .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124 Safari/537.36")
        .set("Accept-Language", "tr,en;q=0.8")
        .call()
    {
        Ok(r) => r.into_string().unwrap_or_default(),
        Err(_) => return String::new(),
    };
    static A: OnceLock<Regex> = OnceLock::new();
    static B: OnceLock<Regex> = OnceLock::new();
    static C: OnceLock<Regex> = OnceLock::new();
    static D: OnceLock<Regex> = OnceLock::new();
    static E: OnceLock<Regex> = OnceLock::new();
    let Some(m) = re(&A, r#"(?s)class="wiki-content"[^>]*>(.*?)</div>"#).captures(&page) else { return String::new() };
    let t = re(&B, r"(?i)<br\s*/?>").replace_all(&m[1], "\n");
    let t = t.replace("</p>", "\n\n");
    let t = re(&C, r"<[^>]+>").replace_all(&t, "");
    let t = html_unescape(&t);
    let t = re(&D, r"[ \t]+").replace_all(&t, " ");
    let t = re(&E, r"\n{3,}").replace_all(&t, "\n\n").trim().to_string();
    if t.chars().count() < 40 || t.to_lowercase().contains("don't have a wiki") {
        return String::new();
    }
    t
}

pub fn artist_lookup(name: &str, out_id: &str, out_dir: &Path) -> Result<ArtistInfo, String> {

    let raw = http_get_ua(&format!("https://api.deezer.com/search/artist?q={}&limit=5", enc(name)), "Soundworks/0.1", 20)?;
    let data: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let list = data["data"].as_array().cloned().unwrap_or_default();
    let low = name.trim().to_lowercase();
    let hit = list.iter().find(|a| a["name"].as_str().map(|n| n.trim().to_lowercase()) == Some(low.clone())).or(list.first());
    let display = hit.and_then(|h| h["name"].as_str()).unwrap_or(name).to_string();
    let mut photo = None;
    if let Some(h) = hit {
        let urls: Vec<&str> = ["picture_xl", "picture_big", "picture_medium"].iter().filter_map(|k| h[*k].as_str()).collect();
        let _ = std::fs::create_dir_all(out_dir);
        let path = out_dir.join(format!("{out_id}.jpg"));
        for u in urls {
            if let Ok(b) = http_bytes(u, 8 << 20) {

                if b.len() > 2000 && std::fs::write(&path, &b).is_ok() {
                    photo = Some(path.to_string_lossy().to_string());
                    break;
                }
            }
        }
    }
    let mut desc = lastfm_bio(&display);
    let mut source = "lastfm";
    if desc.is_empty() {
        desc = wiki_summary(&display);
        source = "wikipedia";
    }
    if desc.is_empty() {
        source = "";
    }
    let found = photo.is_some() || !desc.is_empty();
    Ok(ArtistInfo { name: display, description: desc, photo, found, source: source.to_string(), ver: 2 })
}

const CATEGORIES: &[(&[&str], &str)] = &[
    (&["anatolian rock", "anadolu"], "Anadolu Rock"),
    (&["arabesk", "arabesque"], "Arabesk"),
    (&["turkish folk", "türk halk", "turku", "türkü", "halk müziği"], "Türk Halk Müziği"),
    (&["turkish pop", "türk pop", "türkçe pop"], "Türkçe Pop"),
    (&["hip hop", "hip-hop", "rap", "trap", "boom bap"], "Hip-Hop"),
    (&["electronic", "electro", "techno", "house", "ambient", "synth", "dance", "idm", "trance", "dubstep", "elektronik"], "Elektronik"),
    (&["metal"], "Metal"),
    (&["punk", "hardcore"], "Punk"),
    (&["jazz", "caz"], "Caz"),
    (&["classical", "orchestral", "opera", "klasik"], "Klasik"),
    (&["blues"], "Blues"),
    (&["reggae", "ska", "dub"], "Reggae"),
    (&["soul", "funk", "r&b", "rhythm and blues", "disco"], "Soul & Funk"),
    (&["country", "bluegrass"], "Country"),
    (&["psychedelic"], "Psychedelic"),
    (&["progressive"], "Progresif Rock"),
    (&["indie", "alternative", "alternatif", "shoegaze", "post-punk", "new wave", "grunge"], "Alternatif / Indie"),
    (&["folk", "singer-songwriter", "acoustic"], "Folk"),
    (&["rock"], "Rock"),
    (&["pop"], "Pop"),
    (&["soundtrack", "score", "film", "game"], "Film & Oyun"),
    (&["latin", "salsa", "bossa", "samba"], "Latin"),
    (&["world", "dünya"], "Dünya Müziği"),
];
const TURKISH_SPECIFIC: [&str; 4] = ["Anadolu Rock", "Arabesk", "Türk Halk Müziği", "Türkçe Pop"];

pub fn categorize(cands: &[String]) -> String {
    let mapped: Vec<Option<&str>> = cands
        .iter()
        .map(|c| {
            let low = c.to_lowercase();
            CATEGORIES.iter().find(|(keys, _)| keys.iter().any(|k| low.contains(k))).map(|(_, cat)| *cat)
        })
        .collect();
    if let Some(m) = mapped.iter().flatten().find(|m| TURKISH_SPECIFIC.contains(m)) {
        return m.to_string();
    }
    let first = mapped.iter().flatten().next().copied().unwrap_or("");
    let turkish = cands.iter().any(|c| {
        let l = c.to_lowercase();
        l == "turkish" || l == "turkey" || l == "türkçe" || l.starts_with("turkish ")
    });
    if turkish {
        return match first {
            "Folk" => "Türk Halk Müziği",
            "Pop" => "Türkçe Pop",
            "Rock" | "Alternatif / Indie" => "Türkçe Rock",
            o => o,
        }
        .to_string();
    }
    first.to_string()
}

static LAST_MB: Mutex<Option<Instant>> = Mutex::new(None);

fn mb_get(url: &str) -> Result<Value, String> {
    for attempt in 0..4u64 {
        {
            let mut g = LAST_MB.lock().unwrap();
            if let Some(t) = *g {
                let wait = Duration::from_millis(1100).saturating_sub(t.elapsed());
                if !wait.is_zero() {
                    std::thread::sleep(wait);
                }
            }
            *g = Some(Instant::now());
        }
        match agent(20).get(url).set("User-Agent", "Soundworks/0.1 ( kisisel muzik calar )").set("Accept", "application/json").call() {
            Ok(r) => return serde_json::from_str::<Value>(&r.into_string().map_err(|e| e.to_string())?).map_err(|e| e.to_string()),
            Err(ureq::Error::Status(c, _)) if c == 429 || c == 503 => std::thread::sleep(Duration::from_secs(2 + attempt * 2)),
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("MusicBrainz şu an meşgul".into())
}

fn lq(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn tags_of(e: &Value) -> Vec<String> {
    let mut t: Vec<(i64, String)> =
        e["tags"].as_array().map(|a| a.iter().filter_map(|x| Some((x["count"].as_i64().unwrap_or(0), x["name"].as_str()?.to_string()))).collect()).unwrap_or_default();
    t.sort_by_key(|x| std::cmp::Reverse(x.0));
    t.into_iter().map(|x| x.1).collect()
}

fn same_artist(a: &str, credit: &str) -> bool {
    let (a, c) = (a.to_lowercase(), credit.to_lowercase());
    let (a, c) = (a.trim(), c.trim());
    a.contains(c) || c.contains(a) || a.split_whitespace().any(|w| c.split_whitespace().any(|v| v == w))
}

fn mb_album_tags(artist: &str, album: &str) -> Result<Vec<String>, String> {
    let q = format!("releasegroup:\"{}\" AND artist:\"{}\"", lq(album), lq(artist));
    let data = mb_get(&format!("https://musicbrainz.org/ws/2/release-group/?query={}&fmt=json&limit=5", enc(&q)))?;
    for rg in data["release-groups"].as_array().into_iter().flatten() {
        let credit: String = rg["artist-credit"].as_array().map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(" ")).unwrap_or_default();
        if rg["score"].as_i64().unwrap_or(0) < 85 || !same_artist(artist, &credit) {
            continue;
        }
        let t = tags_of(rg);
        if !t.is_empty() {
            return Ok(t);
        }
    }
    Ok(vec![])
}

fn mb_artist_tags(artist: &str) -> Result<Vec<String>, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Vec<String>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let k = artist.to_lowercase().trim().to_string();
    if let Some(t) = cache.lock().unwrap().get(&k) {
        return Ok(t.clone());
    }
    let q = format!("artist:\"{}\"", lq(artist));
    let data = mb_get(&format!("https://musicbrainz.org/ws/2/artist/?query={}&fmt=json&limit=3", enc(&q)))?;
    let mut tags = vec![];
    for a in data["artists"].as_array().into_iter().flatten() {
        let n = a["name"].as_str().unwrap_or("");
        if a["score"].as_i64().unwrap_or(0) >= 90 && (n.to_lowercase() == k || same_artist(artist, n)) {
            tags = tags_of(a);
            if !tags.is_empty() {
                break;
            }
        }
    }
    cache.lock().unwrap().insert(k, tags.clone());
    Ok(tags)
}

fn deezer_album_genres(artist: &str, album: &str) -> Result<Vec<String>, String> {
    let q = enc(&format!("artist:\"{}\" album:\"{}\"", lq(artist), lq(album)));
    let r: Value = serde_json::from_str(&http_get_ua(&format!("https://api.deezer.com/search/album?q={q}&limit=3"), "Soundworks/0.1", 20)?).map_err(|e| e.to_string())?;
    let Some(id) = r["data"].get(0).and_then(|d| d["id"].as_i64()) else { return Ok(vec![]) };
    let alb: Value = serde_json::from_str(&http_get_ua(&format!("https://api.deezer.com/album/{id}"), "Soundworks/0.1", 20)?).map_err(|e| e.to_string())?;
    Ok(alb["genres"]["data"].as_array().map(|a| a.iter().filter_map(|g| g["name"].as_str().map(String::from)).collect()).unwrap_or_default())
}

pub fn genre_lookup(artist: &str, album: &str) -> Result<String, String> {
    if artist.is_empty() || artist.to_lowercase() == "bilinmeyen sanatçı" {
        return Ok(String::new());
    }
    let mut cat = String::new();
    if !album.is_empty() {
        cat = categorize(&mb_album_tags(artist, album)?);
    }
    if cat.is_empty() {
        if let Ok(c) = deezer_album_genres(artist, album) {
            cat = categorize(&c);
        }
    }
    if cat.is_empty() {
        cat = categorize(&mb_artist_tags(artist)?);
    }
    Ok(cat)
}


#[derive(Clone, Debug, Default)]
pub struct TrackHint {
    pub title: String,
    pub artist: String,
    pub duration: f64,
    pub folder: String,
}

const DURATION_SLACK: f64 = 3.0;

fn strip_numbering(stem: &str) -> String {
    static A: OnceLock<Regex> = OnceLock::new();
    static B: OnceLock<Regex> = OnceLock::new();
    let t = stem.replace('_', " ");
    let t = re(&A, r"^\s*\d{1,3}\s*[-._)\]]+\s*").replace(&t, "").to_string();
    let t = re(&B, r"^\s*\d{1,3}\s+").replace(&t, "").to_string();
    t.trim().to_string()
}

pub fn derive_guesses(h: &TrackHint) -> Vec<(String, String)> {
    let title = strip_numbering(&h.title);
    let artist = h.artist.trim().to_string();
    let mut out: Vec<(String, String)> = vec![];
    let mut push = |a: &str, t: &str| {
        let (a, t) = (a.trim().to_string(), t.trim().to_string());
        if !t.is_empty() && !out.iter().any(|(x, y)| x.to_lowercase() == a.to_lowercase() && y.to_lowercase() == t.to_lowercase()) {
            out.push((a, t));
        }
    };
    let parts = split_dash(&title);
    if parts.len() == 2 {
        let (left, right) = (parts[0].trim().to_string(), parts[1].trim().to_string());
        if artist.is_empty() {
            push(&left, &right);
            push(&right, &left);
        } else if norm_txt(&left) == norm_txt(&artist) {
            push(&artist, &right);
        }
    }
    if !artist.is_empty() {
        push(&artist, &title);
    }
    let folder_parts = split_dash(&h.folder);
    if artist.is_empty() && folder_parts.len() == 2 && !title.is_empty() {
        push(folder_parts[0].trim(), &title);
    }
    push("", &title);
    out
}

struct DzTrack {
    id: i64,
    title: String,
    artist: String,
    album_id: i64,
    album_title: String,
    duration: f64,
}

fn dz_search(base: &str, artist: &str, title: &str, plain: bool) -> Result<Vec<DzTrack>, String> {
    let q = match (plain, artist.is_empty()) {
        (true, true) => title.to_string(),
        (true, false) => format!("{artist} {title}"),
        (false, true) => format!("track:\"{}\"", lq(title)),
        (false, false) => format!("artist:\"{}\" track:\"{}\"", lq(artist), lq(title)),
    };
    let body = http_get_ua(&format!("{base}/search?q={}&limit=10", enc(&q)), "Soundworks/0.1", 20)?;
    let v: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    Ok(v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| {
            Some(DzTrack {
                id: d["id"].as_i64()?,
                title: d["title"].as_str()?.to_string(),
                artist: d["artist"]["name"].as_str().unwrap_or("").to_string(),
                album_id: d["album"]["id"].as_i64().unwrap_or(0),
                album_title: d["album"]["title"].as_str().unwrap_or("").to_string(),
                duration: d["duration"].as_f64().unwrap_or(0.0),
            })
        })
        .collect())
}

fn accepted(guess_artist: &str, guess_title: &str, h: &TrackHint, cand_title: &str, cand_artist: &str, cand_dur: f64) -> bool {
    if !title_ok(guess_title, cand_title) || !artist_ok(guess_artist, cand_artist) {
        return false;
    }
    if h.duration > 0.0 && cand_dur > 0.0 {
        return (h.duration - cand_dur).abs() <= DURATION_SLACK;
    }
    !guess_artist.is_empty() && norm_txt(&clean_title(guess_title, "")) == norm_txt(&clean_title(cand_title, ""))
}

fn album_rank(record_type: &str) -> u8 {
    match record_type {
        "album" => 0,
        "ep" => 1,
        "single" => 2,
        _ => 3,
    }
}

fn year_of(date: &str) -> u32 {
    date.get(..4).and_then(|y| y.parse().ok()).unwrap_or(0)
}

fn deezer_lookup(base: &str, h: &TrackHint, guesses: &[(String, String)]) -> Result<Option<crate::library::OnlineMeta>, String> {
    for (artist, title) in guesses {
        if artist.is_empty() && h.duration <= 0.0 {
            continue;
        }
        let mut cands = dz_search(base, artist, title, false)?;
        let mut ok: Vec<&DzTrack> = cands.iter().filter(|c| accepted(artist, title, h, &c.title, &c.artist, c.duration)).take(3).collect();
        if ok.is_empty() {
            cands = dz_search(base, artist, title, true)?;
            ok = cands.iter().filter(|c| accepted(artist, title, h, &c.title, &c.artist, c.duration)).take(3).collect();
        }
        if ok.is_empty() {
            continue;
        }
        let mut best: Option<(&DzTrack, Value)> = None;
        ok.sort_by_key(|c| c.album_id);
        for c in ok {
            let alb: Value = http_get_ua(&format!("{base}/album/{}", c.album_id), "Soundworks/0.1", 20).ok().and_then(|b| serde_json::from_str(&b).ok()).unwrap_or(Value::Null);
            let key = |a: &Value| (album_rank(a["record_type"].as_str().unwrap_or("")), year_of(a["release_date"].as_str().unwrap_or("")).max(1));
            let better = match &best {
                None => true,
                Some((_, b)) => key(&alb) < key(b),
            };
            if better {
                best = Some((c, alb));
            }
        }
        let Some((c, alb)) = best else { continue };
        let tr: Value = http_get_ua(&format!("{base}/track/{}", c.id), "Soundworks/0.1", 20).ok().and_then(|b| serde_json::from_str(&b).ok()).unwrap_or(Value::Null);
        let album_artist = alb["artist"]["name"].as_str().unwrap_or("").to_string();
        return Ok(Some(crate::library::OnlineMeta {
            title: c.title.clone(),
            artist: c.artist.clone(),
            album: alb["title"].as_str().map(String::from).filter(|s| !s.is_empty()).unwrap_or_else(|| c.album_title.clone()),
            album_artist: if album_artist.to_lowercase() == c.artist.to_lowercase() { String::new() } else { album_artist },
            year: year_of(alb["release_date"].as_str().unwrap_or("")),
            track_no: tr["track_position"].as_u64().unwrap_or(0) as u32,
            source: "deezer".into(),
        }));
    }
    Ok(None)
}

fn mb_lookup(base: &str, h: &TrackHint, guesses: &[(String, String)]) -> Result<Option<crate::library::OnlineMeta>, String> {
    for (artist, title) in guesses {
        if artist.is_empty() && h.duration <= 0.0 {
            continue;
        }
        let q = if artist.is_empty() { format!("recording:\"{}\"", lq(title)) } else { format!("recording:\"{}\" AND artist:\"{}\"", lq(title), lq(artist)) };
        let data = mb_get(&format!("{base}/ws/2/recording/?query={}&fmt=json&limit=5", enc(&q)))?;
        for rec in data["recordings"].as_array().into_iter().flatten() {
            let credit: String = rec["artist-credit"].as_array().map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(" ")).unwrap_or_default();
            let len = rec["length"].as_f64().map(|ms| ms / 1000.0).unwrap_or(0.0);
            let name = rec["title"].as_str().unwrap_or("");
            if rec["score"].as_i64().unwrap_or(100) < 85 || !accepted(artist, title, h, name, &credit, len) {
                continue;
            }
            let mut releases: Vec<&Value> = rec["releases"].as_array().into_iter().flatten().collect();
            releases.sort_by_key(|r| {
                let official = r["status"].as_str() == Some("Official");
                let kind = r["release-group"]["primary-type"].as_str().map(|k| k.to_lowercase()).unwrap_or_default();
                let extras = r["release-group"]["secondary-types"].as_array().map_or(false, |a| !a.is_empty());
                (!official, extras, album_rank(&kind), year_of(r["date"].as_str().unwrap_or("")).max(1))
            });
            let Some(rel) = releases.first() else { continue };
            return Ok(Some(crate::library::OnlineMeta {
                title: name.to_string(),
                artist: rec["artist-credit"][0]["name"].as_str().unwrap_or(&credit).to_string(),
                album: rel["title"].as_str().unwrap_or("").to_string(),
                album_artist: String::new(),
                year: year_of(rel["date"].as_str().unwrap_or("")),
                track_no: 0,
                source: "musicbrainz".into(),
            }));
        }
    }
    Ok(None)
}

pub fn album_cover_lookup(artist: &str, album: &str, out: &Path) -> Result<bool, String> {
    album_cover_at("https://api.deezer.com", artist, album, out)
}

pub fn album_cover_at(base: &str, artist: &str, album: &str, out: &Path) -> Result<bool, String> {
    if artist.trim().is_empty() || album.trim().is_empty() {
        return Ok(false);
    }
    let various = artist.to_lowercase().contains("various");
    for (i, q) in [format!("artist:\"{}\" album:\"{}\"", lq(artist), lq(album)), format!("{artist} {album}")].iter().enumerate() {
        let body = http_get_ua(&format!("{base}/search/album?q={}&limit=10", enc(q)), "Soundworks/0.1", 20)?;
        let v: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        let hit = v["data"].as_array().into_iter().flatten().find(|d| {
            title_ok(album, d["title"].as_str().unwrap_or("")) && (various || artist_ok(artist, d["artist"]["name"].as_str().unwrap_or("")))
        });
        let Some(hit) = hit else {
            if i == 0 {
                continue;
            }
            return Ok(false);
        };
        let Some(url) = ["cover_xl", "cover_big", "cover_medium"].iter().filter_map(|k| hit[*k].as_str()).find(|u| !u.is_empty()) else {
            return Ok(false);
        };
        let bytes = http_bytes(url, 8 << 20)?;
        let img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
        if let Some(dir) = out.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = out.with_extension("tmp");
        img.thumbnail(360, 360).save_with_format(&tmp, image::ImageFormat::Png).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, out).map_err(|e| e.to_string())?;
        return Ok(true);
    }
    Ok(false)
}

pub fn track_lookup(h: &TrackHint) -> Result<Option<crate::library::OnlineMeta>, String> {
    track_lookup_at(h, "https://api.deezer.com", "https://musicbrainz.org")
}

pub fn track_lookup_at(h: &TrackHint, deezer: &str, musicbrainz: &str) -> Result<Option<crate::library::OnlineMeta>, String> {
    let guesses = derive_guesses(h);
    if guesses.is_empty() {
        return Ok(None);
    }
    match deezer_lookup(deezer, h, &guesses) {
        Ok(Some(m)) => return Ok(Some(m)),
        Ok(None) => {}
        Err(e) => {
            if let Ok(Some(m)) = mb_lookup(musicbrainz, h, &guesses) {
                return Ok(Some(m));
            }
            return Err(e);
        }
    }
    mb_lookup(musicbrainz, h, &guesses)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn serve_json(routes: Vec<(&'static str, String)>) -> u16 {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let routes = routes.clone();
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut stream = stream;
                    let mut line = String::new();
                    let _ = reader.read_line(&mut line);
                    let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                    loop {
                        let mut h = String::new();
                        if reader.read_line(&mut h).is_err() || h.trim().is_empty() {
                            break;
                        }
                    }
                    let found = routes.iter().find(|(p, _)| path.starts_with(p)).map(|(_, b)| b.clone());
                    let (status, body) = match found {
                        Some(b) => ("200 OK", b),
                        None => ("404 Not Found", String::new()),
                    };
                    let head = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(body.as_bytes());
                });
            }
        });
        port
    }

    fn deezer_routes() -> Vec<(&'static str, String)> {
        vec![
            (
                "/search",
                r#"{"data":[
                {"id":1,"title":"Vukuat","duration":165,"artist":{"name":"90BPM"},"album":{"id":11,"title":"Vukuat Single"}},
                {"id":2,"title":"Vukuat","duration":165,"artist":{"name":"90BPM"},"album":{"id":10,"title":"Kötülük Bizim İşimiz"}},
                {"id":3,"title":"Vukuat (Live)","duration":240,"artist":{"name":"90BPM"},"album":{"id":12,"title":"Canlı"}}]}"#
                    .to_string(),
            ),
            ("/album/10", r#"{"title":"Kötülük Bizim İşimiz","record_type":"album","release_date":"2015-03-02","artist":{"name":"90BPM"}}"#.to_string()),
            ("/album/11", r#"{"title":"Vukuat Single","record_type":"single","release_date":"2014-12-01","artist":{"name":"90BPM"}}"#.to_string()),
            ("/track/2", r#"{"track_position":1,"disk_number":1}"#.to_string()),
            ("/track/1", r#"{"track_position":1,"disk_number":1}"#.to_string()),
        ]
    }

    fn hint(title: &str, artist: &str, folder: &str, duration: f64) -> TrackHint {
        TrackHint { title: title.into(), artist: artist.into(), duration, folder: folder.into() }
    }

    #[test]
    #[ignore]
    fn live_album_cover() {
        let out = std::env::temp_dir().join("sw_live_cover.png");
        let _ = std::fs::remove_file(&out);
        assert!(album_cover_lookup("Daft Punk", "Discovery", &out).unwrap());
        let img = image::open(&out).unwrap();
        assert!(img.width() >= 300);
        assert!(!album_cover_lookup("Daft Punk", "zzqx nonexistent album 91827", &out.with_extension("x.png")).unwrap());
    }

    #[test]
    fn cover_is_not_downloaded_for_mismatching_album() {
        let port = serve_json(vec![(
            "/search/album",
            r#"{"data":[{"title":"Other Thing","artist":{"name":"Someone Else"},"cover_xl":"http://127.0.0.1:1/x.jpg"}]}"#.to_string(),
        )]);
        let out = std::env::temp_dir().join(format!("sw_cover_test_{port}.png"));
        let _ = std::fs::remove_file(&out);
        let got = album_cover_at(&format!("http://127.0.0.1:{port}"), "Daft Punk", "Discovery", &out).unwrap();
        assert!(!got && !out.exists());
        assert!(!album_cover_at("http://127.0.0.1:1", "", "Discovery", &out).unwrap());
    }

    #[test]
    #[ignore]
    fn live_track_lookup() {
        let m = track_lookup(&hint("03 - Daft Punk - Giorgio by Moroder", "", "Random Access Memories", 544.0)).unwrap();
        println!("{m:?}");
        assert_eq!(m.unwrap().album, "Random Access Memories");
        let m = track_lookup(&hint("Karma Police", "Radiohead", "", 0.0)).unwrap();
        println!("{m:?}");
        assert!(m.is_some());
    }

    #[test]
    fn guesses_come_from_file_names_and_folders() {
        let g = derive_guesses(&hint("01 - 90BPM - Vukuat", "", "", 0.0));
        assert!(g.contains(&("90BPM".to_string(), "Vukuat".to_string())), "{g:?}");
        assert!(g.contains(&("Vukuat".to_string(), "90BPM".to_string())));
        let g = derive_guesses(&hint("07_Dünya_Dar", "", "Sanatçı - Albüm", 0.0));
        assert_eq!(g[0], ("Sanatçı".to_string(), "Dünya Dar".to_string()));
        assert!(g.contains(&(String::new(), "Dünya Dar".to_string())));
        let g = derive_guesses(&hint("Vukuat", "90BPM", "", 0.0));
        assert_eq!(g[0], ("90BPM".to_string(), "Vukuat".to_string()));
        assert!(derive_guesses(&hint("", "", "", 0.0)).is_empty());
    }

    #[test]
    fn deezer_match_prefers_the_real_album_and_checks_the_duration() {
        let port = serve_json(deezer_routes());
        let base = format!("http://127.0.0.1:{port}");
        let m = track_lookup_at(&hint("01 Vukuat", "90BPM", "", 164.7), &base, &base).unwrap().unwrap();
        assert_eq!(m.source, "deezer");
        assert_eq!(m.album, "Kötülük Bizim İşimiz");
        assert_eq!((m.year, m.track_no), (2015, 1));
        assert_eq!(m.artist, "90BPM");
        assert!(m.album_artist.is_empty());
    }

    #[test]
    fn wrong_duration_or_artist_is_rejected() {
        let port = serve_json(deezer_routes());
        let base = format!("http://127.0.0.1:{port}");
        assert_eq!(track_lookup_at(&hint("Vukuat", "90BPM", "", 400.0), &base, &format!("{base}/none")).unwrap_or(None), None);
        assert_eq!(track_lookup_at(&hint("Vukuat", "Başka Biri", "", 0.0), &base, &format!("{base}/none")).unwrap_or(None), None);
    }

    #[test]
    fn title_only_searches_need_a_known_duration() {
        let port = serve_json(deezer_routes());
        let base = format!("http://127.0.0.1:{port}");
        let found = track_lookup_at(&hint("Vukuat", "", "", 165.0), &base, &base).unwrap().unwrap();
        assert_eq!(found.album, "Kötülük Bizim İşimiz");
        assert_eq!(track_lookup_at(&hint("Vukuat", "", "", 0.0), &base, &format!("{base}/none")).unwrap_or(None), None);
    }

    #[test]
    fn musicbrainz_is_the_fallback() {
        let dz = serve_json(vec![("/search", r#"{"data":[]}"#.to_string())]);
        let mb = serve_json(vec![(
            "/ws/2/recording",
            r#"{"recordings":[{"score":100,"title":"Gece","length":201000,"artist-credit":[{"name":"Sanatçı"}],
               "releases":[{"title":"Derleme","date":"2020-01-01","status":"Official","release-group":{"primary-type":"Compilation"}},
                           {"title":"İlk Albüm","date":"2012-05-05","status":"Official","release-group":{"primary-type":"Album"}}]}]}"#
                .to_string(),
        )]);
        let m = track_lookup_at(&hint("Gece", "Sanatçı", "", 200.0), &format!("http://127.0.0.1:{dz}"), &format!("http://127.0.0.1:{mb}")).unwrap().unwrap();
        assert_eq!((m.source.as_str(), m.album.as_str(), m.year), ("musicbrainz", "İlk Albüm", 2012));
    }


    #[test]
    fn ratio_matches_difflib() {
        assert!((ratio("abcd", "bcde") - 0.75).abs() < 1e-9);
        assert_eq!(ratio("", ""), 1.0);
        assert_eq!(ratio("abc", "xyz"), 0.0);
    }

    #[test]
    fn title_and_artist_matching() {
        assert!(title_ok("Şarkı (Remastered 2011)", "sarki"));
        assert!(title_ok("Hello - Official Video", "Hello"));
        assert!(!title_ok("Hello", "Goodbye"));
        assert!(artist_ok("Adele - Topic", "Adele"));
        assert!(!match_ok("Hello", "Adele", 200.0, "Hello", "Adele", 300.0));
    }

    #[test]
    fn lrc_parsing_line_and_word() {
        let (l, w) = parse_input("[00:01.50]Merhaba dünya\n[00:05.00]İkinci satır", false);
        assert!(!w);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].t, 1500);
        let (l, w) = parse_input("[00:10.00]<00:10.00>Bir <00:10.50>iki", false);
        assert!(w);
        assert_eq!(l[0].words.len(), 2);
        assert_eq!(l[0].words[0].d, 500);
        assert_eq!(l[0].text, "Bir iki");
    }

    #[test]
    fn plain_text_fallback() {
        let (l, _) = parse_input("bir\niki\nüç", true);
        assert_eq!(l.len(), 3);
        assert_eq!(l[2].t, 8000);
        assert!(parse_input("bir\niki", false).0.is_empty());
    }

    #[test]
    fn ttml_words_and_background() {
        let t = r#"<tt><body><div><p begin="0:01.000" end="0:03.000"><span begin="0:01.000" end="0:02.000">Hello</span> <span begin="0:02.000" end="0:03.000">world</span><span ttm:role="x-bg"><span begin="0:02.500" end="0:03.000">(ooh)</span></span></p></div></body></tt>"#;
        let (l, w) = parse_input(t, false);
        assert!(w);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].t, 1000);
        assert!(l[0].words.iter().any(|x| x.bg && x.text == "ooh"));
        assert_eq!(ttml_ms("1:02.5"), 62500);
        assert_eq!(ttml_ms("350ms"), 350);
    }

    #[test]
    fn json_lines_parsing() {
        let (l, _) = parse_input(r#"{"lines":[{"time":2000,"text":"a"},{"time":500,"text":"b"}]}"#, false);
        assert_eq!(l.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(), ["b", "a"]);
    }

    #[test]
    fn genre_categories() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(categorize(&s(&["Anatolian Rock", "rock"])), "Anadolu Rock");
        assert_eq!(categorize(&s(&["turkish", "pop"])), "Türkçe Pop");
        assert_eq!(categorize(&s(&["jazz fusion"])), "Caz");
        assert_eq!(categorize(&s(&["xyz"])), "");
    }

    #[test]
    fn html_entities() {
        assert_eq!(html_unescape("a &amp; b &#39;c&#x21;"), "a & b 'c!");
    }

    #[test]
    #[ignore]
    fn live_lookups() {
        let l = lyrics_lookup("Hello", "Adele", "25", 295.0);
        println!("sözler: {} / {} / {} satır", l.kind, l.provider, l.lines.len());
        assert!(l.found());
        let d = std::env::temp_dir().join("sw_artist_live");
        let a = artist_lookup("Adele", "adele", &d).unwrap();
        println!("sanatçı: {} foto={:?} açıklama={} kaynak={}", a.name, a.photo, a.description.len(), a.source);
        assert!(a.found);
        println!("tür: {:?}", genre_lookup("Adele", "25"));
    }
}
