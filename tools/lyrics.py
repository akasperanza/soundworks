"""Soundworks sözler (lyrics) yardımcısı.

Kaynaklar dynamic-music-pill (Andbal23) projesindekiyle aynıdır:
  - Apple Music TTML (kelime senkronlu): lrc.red, yedek olarak lyrics-api.boidu.dev
  - BiniLyrics: lyrics-api.binimum.org
  - LRCLib: lrclib.net (satır senkronlu LRC ya da düz metin)

Öncelik: kelime senkronlu > satır senkronlu > düz metin.
Sunucu modu: stdin'den satır satır JSON {"title","artist","album","duration"}, stdout'a JSON satırı.
Tek seferlik: python lyrics.py --once "Şarkı" "Sanatçı" ["Albüm"] [süre_sn]
"""
import concurrent.futures
import difflib
import json
import re
import sys
import unicodedata
import urllib.parse
import urllib.request

TIMEOUT = 9
TAG_SUFFIX_RE = re.compile(
    r"^(?:remaster(?:ed)?|live|official.*|audio|video|music\s*video|lyric.*|visualizer|version|edit|mix|radio\s*edit|acoustic|instrumental|explicit|clean|\d{4}\b.*)",
    re.I,
)


def clean_artist(artist):
    if not artist:
        return ""
    a = re.sub(r"\s*[-–—]\s*Topic$", "", artist, flags=re.I)
    a = re.sub(r"VEVO$", "", a, flags=re.I)
    a = re.sub(r"\s+Official$", "", a, flags=re.I)
    return a.strip()


def clean_title(title, artist=""):
    if not title:
        return ""
    t = re.sub(r"\s*[\(\[\{].*?[\)\]\}]", "", title)
    t = re.sub(r"\s+(?:feat|ft|prod)\.?\s+.*", "", t, flags=re.I)
    t = re.sub(r"\s*\|\s*.*", "", t).strip()
    parts = re.split(r"\s+[-–—]\s+", t)
    if len(parts) >= 2:
        left = parts[0].strip()
        right = " - ".join(parts[1:]).strip()
        ca = clean_artist(artist).lower()
        if TAG_SUFFIX_RE.match(right):
            t = left
        elif ca and (left.lower() == ca or left.lower() in ca or ca in left.lower()):
            t = right
    return t.strip() or title.strip()


def alt_artist_title(title, artist=""):
    if not title:
        return None
    t = re.sub(r"\s*[\(\[\{].*?[\)\]\}]", "", title)
    t = re.sub(r"\s+(?:feat|ft|prod)\.?\s+.*", "", t, flags=re.I)
    t = re.sub(r"\s*\|\s*.*", "", t).strip()
    parts = re.split(r"\s+[-–—]\s+", t)
    if len(parts) == 2:
        left, right = parts[0].strip(), parts[1].strip()
        if left and right and not TAG_SUFFIX_RE.match(right):
            ca = clean_artist(artist).lower()
            if not ca or left.lower() != ca:
                return {"artist": left, "title": right}
    return None


def http_get(url, ua="BetterLyrics/1.0"):
    req = urllib.request.Request(url, headers={"User-Agent": ua})
    with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
        return r.read().decode("utf-8", "replace")


# ------------------------------------------------------------------ eşleşme doğrulama
def norm_txt(s):
    s = (s or "").lower().replace("ı", "i").replace("İ", "i")
    s = unicodedata.normalize("NFKD", s)
    s = "".join(c for c in s if not unicodedata.combining(c))
    s = re.sub(r"[\(\[\{].*?[\)\]\}]", " ", s)
    s = re.sub(r"(feat|ft|prod).*", " ", s)
    s = re.sub(r"[^a-z0-9 ]+", " ", s)
    return re.sub(r"\s+", " ", s).strip()


def title_ok(req_title, cand_title):
    a = norm_txt(clean_title(req_title))
    b = norm_txt(clean_title(cand_title))
    if not a or not b:
        return False
    if a == b:
        return True
    ratio = difflib.SequenceMatcher(None, a, b).ratio()
    if len(a) >= 4 and (a in b or b in a):
        return ratio >= 0.7
    return ratio >= 0.86


def artist_ok(req_artist, cand_artist):
    a = norm_txt(clean_artist(req_artist))
    if not a:
        return True
    b = norm_txt(cand_artist)
    if not b:
        return True
    if a in b or b in a:
        return True
    ta = {w for w in a.split() if len(w) >= 3}
    tb = {w for w in b.split() if len(w) >= 3}
    return bool(ta & tb) or difflib.SequenceMatcher(None, a, b).ratio() >= 0.6


def duration_ok(req, cand):
    if not req or not cand:
        return True
    return abs(req - cand) <= 25


def match_ok(req_title, req_artist, req_duration, cand_title, cand_artist, cand_duration):
    return title_ok(req_title, cand_title) and artist_ok(req_artist, cand_artist) and duration_ok(req_duration, cand_duration)


# ------------------------------------------------------------------ ayrıştırıcılar
def ttml_ms(val):
    if not val:
        return 0
    try:
        s = str(val).strip().lower()
        if s.endswith("ms"):
            return round(float(s[:-2]))
        if s.endswith("s") and ":" not in s:
            s = s[:-1]
        parts = s.split(":")
        if len(parts) == 1:
            return round(float(parts[0]) * 1000)
        if len(parts) == 2:
            return round((int(parts[0]) * 60 + float(parts[1])) * 1000)
        if len(parts) == 3:
            return round((int(parts[0]) * 3600 + int(parts[1]) * 60 + float(parts[2])) * 1000)
    except Exception:
        pass
    return 0


def split_bg(body):
    """TTML satır gövdesini (ana metin, [arka vokal iç içerikleri]) olarak ayırır; iç içe span'leri dengeler."""
    out, bgs, i = [], [], 0
    opener = re.compile(r'<span\s+[^>]*ttm:role="x-bg"[^>]*>', re.I)
    tag = re.compile(r'<span\b|</span>', re.I)
    while True:
        m = opener.search(body, i)
        if not m:
            out.append(body[i:])
            break
        out.append(body[i:m.start()])
        j, depth = m.end(), 1
        inner_start = m.end()
        inner_end = m.end()
        while depth > 0:
            mo = tag.search(body, j)
            if not mo:
                inner_end = len(body)
                j = len(body)
                break
            if mo.group(0).startswith("</"):
                depth -= 1
                if depth == 0:
                    inner_end = mo.start()
            else:
                depth += 1
            j = mo.end()
        bgs.append(body[inner_start:inner_end])
        out.append(" ")
        i = j
    return "".join(out), bgs


_WORD_RE = re.compile(r'<span\s+[^>]*begin="([^"]+)"[^>]*end="([^"]+)"[^>]*>([^<]+)</span>(\s*)', re.I)


def _words_from(body, bg):
    words = []
    for sm in _WORD_RE.finditer(body):
        ws, we, raw = ttml_ms(sm.group(1)), ttml_ms(sm.group(2)), sm.group(3)
        wt = raw.strip()
        if bg:
            wt = wt.strip("()").strip()
        if wt:
            words.append({"t": ws, "d": max(100, we - ws), "text": wt, "sp": bool(sm.group(4)) or raw.endswith(" "), "bg": bg})
    return words


def parse_ttml(text):
    lines = []
    has_words = False
    for pm in re.finditer(r'<p\s+[^>]*begin="([^"]+)"[^>]*>(.*?)</p>', text, re.I | re.S):
        start = ttml_ms(pm.group(1))
        main_body, bg_bodies = split_bg(pm.group(2))
        words = _words_from(main_body, False)
        bg_words = []
        for bb in bg_bodies:
            bw = _words_from(bb, True)
            if not bw:
                t = re.sub(r"\s+", " ", re.sub(r"<[^>]+>", "", bb)).strip().strip("()").strip()
                if t:
                    bw = [{"t": start, "d": 600, "text": t, "sp": False, "bg": True}]
            bg_words += bw
        main_text = re.sub(r"\s+", " ", re.sub(r"<[^>]+>", "", main_body)).strip()
        bg_text = " ".join(w["text"] for w in bg_words)
        line_text = main_text or bg_text
        if not line_text:
            continue
        all_words = words + bg_words
        if all_words:
            has_words = True
            lines.append({"t": start, "text": line_text, "words": all_words})
        else:
            lines.append({"t": start, "text": line_text})
    lines.sort(key=lambda x: x["t"])
    # zamanlar çökmüşse (hepsi aynı an) bu kaynak senkronlu değildir: reddet, başka sağlayıcı denensin
    if len(lines) > 2 and len({x["t"] for x in lines}) <= 1:
        return [], False
    merged = []
    for ln in lines:
        if merged and abs(merged[-1]["t"] - ln["t"]) < 100:
            if ln["text"] not in merged[-1]["text"]:
                merged[-1]["text"] += " " + ln["text"]
                if ln.get("words"):
                    merged[-1]["words"] = merged[-1].get("words", []) + ln["words"]
        else:
            merged.append(ln)
    return merged, has_words


def _int(v, default=0):
    try:
        return int(float(v))
    except Exception:
        return default


def parse_input(inp, allow_plain=False):
    """(satırlar, kelime_seviyesi_var_mı)"""
    if not inp:
        return [], False
    if isinstance(inp, str) and ("<tt" in inp or "<p begin=" in inp):
        return parse_ttml(inp)
    obj = None
    if isinstance(inp, dict):
        obj = inp
    elif isinstance(inp, str) and inp.strip().startswith("{"):
        try:
            obj = json.loads(inp)
        except Exception:
            obj = None
    if isinstance(obj, dict):
        if isinstance(obj.get("ttml"), str):
            return parse_ttml(obj["ttml"])
        lst = obj.get("lines") or obj.get("lyrics") or obj.get("data") or obj.get("result")
        if isinstance(lst, list) and lst:
            lines, has_words = [], False
            for item in lst:
                if not isinstance(item, dict):
                    continue
                t = _int(item.get("startTimeMs") or item.get("time") or item.get("begin") or 0)
                text = item.get("text") if isinstance(item.get("text"), str) else (item.get("words") if isinstance(item.get("words"), str) else item.get("line", ""))
                text = (text or "").strip()
                sub = item.get("syllables") or item.get("wordsArray") or item.get("lead") or item.get("words_list")
                if isinstance(sub, list) and sub:
                    has_words = True
                    words = []
                    for i, w in enumerate(sub):
                        wt = _int(w.get("startTimeMs") or w.get("time") or w.get("begin") or t)
                        nxt = sub[i + 1] if i + 1 < len(sub) else None
                        nt = _int((nxt.get("startTimeMs") or nxt.get("time") or nxt.get("begin")) if nxt else wt + 500, wt + 500)
                        words.append({"t": wt, "d": max(200, nt - wt), "text": w.get("text") or w.get("word") or ""})
                    lines.append({"t": t, "text": text, "words": words})
                elif text:
                    lines.append({"t": t, "text": text})
            if lines:
                lines.sort(key=lambda x: x["t"])
                return lines, has_words
    if not isinstance(inp, str):
        return [], False

    line_re = re.compile(r"\[(\d{2}):(\d{2})[\.:](\d{2,3})\](.*)")
    word_re = re.compile(r"[<\(\[](\d{2}):(\d{2})[\.:](\d{2,3})[>\)\]]([^<\(\[\n]*)")
    out, has_words = [], False
    for raw in inp.split("\n"):
        m = line_re.search(raw)
        if not m:
            continue
        t = int(m.group(1)) * 60000 + int(m.group(2)) * 1000 + round(float("0." + m.group(3)) * 1000)
        body = m.group(4).strip()
        words = []
        for wm in word_re.finditer(body):
            wt = int(wm.group(1)) * 60000 + int(wm.group(2)) * 1000 + round(float("0." + wm.group(3)) * 1000)
            words.append({"t": wt, "text": wm.group(4), "sp": wm.group(4) != wm.group(4).rstrip()})
        if words:
            has_words = True
            for i, w in enumerate(words):
                nxt = words[i + 1]["t"] if i + 1 < len(words) else w["t"] + 1500
                w["d"] = max(100, nxt - w["t"])
                w["text"] = w["text"].strip()
            clean = re.sub(r"[<\(\[].*?[>\)\]]", "", body).strip()
            out.append({"t": t, "text": clean, "words": [w for w in words if w["text"]]})
        else:
            out.append({"t": t, "text": body})
    lines = [x for x in out if x["text"]]
    lines.sort(key=lambda x: x["t"])
    if not lines and allow_plain:
        plain = [l.strip() for l in inp.split("\n") if l.strip() and not l.strip().startswith(("<", "{"))]
        return [{"t": i * 4000, "text": l} for i, l in enumerate(plain)], False
    return lines, has_words


# ------------------------------------------------------------------ sağlayıcılar
def _result(provider, lines, words, synced=True):
    kind = "word" if words else ("synced" if synced else "plain")
    return {"provider": provider, "kind": kind, "lines": lines}


def _from_rawtext(provider, raw):
    lines, words = parse_input(raw)
    return _result(provider, lines, words) if lines else None


def _lrcred_like(provider, url, req=None):
    """req = (başlık, sanatçı, süre): sonuç listesinde yalnızca doğrulanmış eşleşmeler kabul edilir."""
    raw = http_get(url)
    r = _from_rawtext(provider, raw)
    if r:
        return r
    data = json.loads(raw)
    results = data.get("results") if isinstance(data, dict) else None
    if isinstance(results, list) and results:
        if req:
            rt, ra, rd = req
            results = [
                x for x in results
                if match_ok(rt, ra, rd, x.get("track_name") or x.get("title") or "", x.get("artist_name") or x.get("artist") or "", x.get("duration") or 0)
            ]
        if not results:
            return None
        best = next((x for x in results if x.get("timing_type") == "word"), results[0])
        if best and best.get("lyricsUrl"):
            return _from_rawtext(provider, http_get(best["lyricsUrl"]))
    return None


def apple_music(title, artist, album, duration):
    ca = clean_artist(artist)
    alt = alt_artist_title(title, ca)
    qt = alt["title"] if alt else (clean_title(title, ca) or title)
    qa = alt["artist"] if alt else ca
    query = f"{qt} {qa}".strip()
    try:
        r = _lrcred_like("Apple Music", "https://lrc.red/api/v1?q=" + urllib.parse.quote(query), (qt, qa, duration))
        if r:
            return r
    except Exception:
        pass
    try:
        url = "https://lyrics-api.boidu.dev/getLyrics?s=" + urllib.parse.quote(qt) + "&a=" + urllib.parse.quote(qa)
        if album:
            url += "&al=" + urllib.parse.quote(album)
        if duration:
            url += "&d=" + str(round(duration))
        return _from_rawtext("Apple Music", http_get(url))
    except Exception:
        return None


def binilyrics(title, artist, album, duration):
    ca = clean_artist(artist)
    alt = alt_artist_title(title, ca)
    qt = alt["title"] if alt else (clean_title(title, ca) or title)
    qa = alt["artist"] if alt else ca
    try:
        url1 = "https://lyrics-api.binimum.org/getLyrics?s=" + urllib.parse.quote(qt) + "&a=" + urllib.parse.quote(qa)
        if album:
            url1 += "&al=" + urllib.parse.quote(album)
        if duration:
            url1 += "&d=" + str(round(duration))
        try:
            r = _lrcred_like("BiniLyrics", url1, (qt, qa, duration))
            if r:
                return r
        except Exception:
            pass
        return _lrcred_like("BiniLyrics", "https://lyrics-api.binimum.org/getLyrics?q=" + urllib.parse.quote(f"{qt} {qa}".strip()), (qt, qa, duration))
    except Exception:
        return None


def _lrclib_candidates(title, artist, duration):
    q = f"{artist or ''} {title or ''}".strip()
    if not q:
        return []
    try:
        data = json.loads(http_get("https://lrclib.net/api/search?q=" + urllib.parse.quote(q), ua="Soundworks/0.1"))
    except Exception:
        return []
    if not isinstance(data, list):
        return []
    if duration:
        close = [d for d in data if abs((d.get("duration") or 0) - duration) <= 15]
        if close:
            return close
    return data


def lrclib(title, artist, album, duration):
    ca = clean_artist(artist)
    alt = alt_artist_title(title, ca)
    qt = clean_title(title, ca) or title
    items = []

    def add(x):
        if not (isinstance(x, dict) and x.get("id")) or any(i.get("id") == x["id"] for i in items):
            return
        cand_title = x.get("trackName") or x.get("name") or ""
        # başlık/sanatçı/süre uyuşmayan sonuçlar (aynı sanatçının başka şarkısı vb.) reddedilir
        ok_main = match_ok(qt, ca, duration, cand_title, x.get("artistName") or "", x.get("duration") or 0)
        ok_alt = bool(alt) and match_ok(alt["title"], alt["artist"], duration, cand_title, x.get("artistName") or "", x.get("duration") or 0)
        if ok_main or ok_alt:
            items.append(x)

    if duration:
        try:
            url = (
                "https://lrclib.net/api/get?track_name=" + urllib.parse.quote(qt) + "&artist_name=" + urllib.parse.quote(ca)
                + "&album_name=" + urllib.parse.quote(album or "") + "&duration=" + str(round(duration))
            )
            add(json.loads(http_get(url, ua="Soundworks/0.1")))
        except Exception:
            pass
    for c in _lrclib_candidates(qt, ca, duration):
        add(c)
    if alt:
        for c in _lrclib_candidates(alt["title"], alt["artist"], duration):
            add(c)
    if not items and qt:
        for c in _lrclib_candidates(qt, "", duration):
            add(c)
    best, best_score = None, None
    for it in items:
        if not it.get("syncedLyrics"):
            continue
        score = -abs(it["duration"] - duration) if (duration and it.get("duration")) else 0
        if best_score is None or score > best_score:
            best, best_score = it, score
    if best:
        lines, words = parse_input(best["syncedLyrics"])
        if lines:
            return _result("LRCLib", lines, words)
    for it in items:
        if it.get("plainLyrics"):
            lines, _ = parse_input(it["plainLyrics"], allow_plain=True)
            if lines:
                return _result("LRCLib", lines, False, synced=False)
    return None


def get_lyrics(title, artist, album="", duration=0):
    if not (title or "").strip() and not (artist or "").strip():
        return {"ok": True, "kind": "", "provider": "", "lines": []}
    funcs = [apple_music, binilyrics, lrclib]
    results = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=3) as ex:
        futs = [ex.submit(f, title, artist, album, duration or 0) for f in funcs]
        for f in futs:
            try:
                r = f.result(timeout=TIMEOUT * 3)
                if r and r["lines"]:
                    results.append(r)
            except Exception:
                pass
    for kind in ("word", "synced", "plain"):
        group = [r for r in results if r["kind"] == kind]
        if group:
            chosen = next((r for r in group if r["provider"] == "Apple Music"), group[0])
            return {"ok": True, "kind": kind, "provider": chosen["provider"], "lines": chosen["lines"]}
    return {"ok": True, "kind": "", "provider": "", "lines": []}


def handle(req):
    try:
        return get_lyrics(req.get("title", ""), req.get("artist", ""), req.get("album", ""), float(req.get("duration") or 0))
    except Exception as e:
        return {"ok": False, "error": str(e)}


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stdin.reconfigure(encoding="utf-8")
    if len(sys.argv) >= 4 and sys.argv[1] == "--once":
        out = get_lyrics(sys.argv[2], sys.argv[3], sys.argv[4] if len(sys.argv) > 4 else "", float(sys.argv[5]) if len(sys.argv) > 5 else 0)
        print(json.dumps(out, ensure_ascii=False))
        return
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except Exception:
            continue
        res = handle(req)
        res["id"] = req.get("id")
        print(json.dumps(res, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    main()
