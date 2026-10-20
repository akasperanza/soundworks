"""Soundworks sanatçı bilgisi yardımcısı (ytmusicapi).

Sunucu modu: stdin'den satır satır JSON alır {"name","id","dir"}, stdout'a satır satır JSON yazar.
Tek seferlik: python ytm_artist.py --once "Sanatçı" "<çıktı klasörü>"
"""
import json
import os
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

try:
    from ytmusicapi import YTMusic
except Exception as e:  # ytmusicapi kurulu değil
    YTMusic = None
    IMPORT_ERROR = str(e)

UA = {"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Soundworks/0.1"}
_yt = None
_yt_tr = None


def yt():
    """Arama için varsayılan (İngilizce) istemci: tr dilinde filter=artists boş dönüyor."""
    global _yt
    if _yt is None:
        _yt = YTMusic()
    return _yt


def yt_tr():
    """Sanatçı sayfası/açıklama için Türkçe istemci (olmazsa varsayılan)."""
    global _yt_tr
    if _yt_tr is None:
        try:
            _yt_tr = YTMusic(language="tr", location="TR")
        except Exception:
            _yt_tr = yt()
    return _yt_tr


def http_get(url, timeout=20):
    req = urllib.request.Request(url, headers=UA)
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return r.read()


def enlarge(url):
    """googleusercontent küçük resim adresindeki boyut parametresini büyüt."""
    return re.sub(r"=w\d+-h\d+", "=w1920-h1080", url) if re.search(r"=w\d+-h\d+", url) else url


def best_thumb(thumbs):
    if not thumbs:
        return None
    return max(thumbs, key=lambda t: (t.get("width") or 0) * (t.get("height") or 0)).get("url")


MUSIC_WORDS = (
    "müzik", "şarkıcı", "rapçi", "rap ", "hip hop", "hip-hop", "grup", "sanatçı", "besteci", "albüm", "söz yazar",
    "gitarist", "orkestra", "singer", "band", "musician", "rapper", "album", "songwriter", "composer", "vocalist",
)


def looks_musical(text):
    low = (text or "").lower()
    return any(w in low for w in MUSIC_WORDS)


def wiki_summary(name):
    for lang in ("tr", "en"):
        try:
            title = urllib.parse.quote(name.replace(" ", "_"))
            data = json.loads(http_get(f"https://{lang}.wikipedia.org/api/rest_v1/page/summary/{title}").decode("utf-8"))
            if data.get("type") == "standard" and data.get("extract") and looks_musical(data["extract"]):
                return data["extract"]
        except Exception:
            continue
    return ""


_last_lfm = 0.0


def lastfm_bio(name):
    """Last.fm sanatçı wiki sayfasından biyografi (API anahtarı gerekmez). Yoksa boş döner."""
    global _last_lfm
    import html as _html
    wait = 0.5 - (time.time() - _last_lfm)
    if wait > 0:
        time.sleep(wait)
    _last_lfm = time.time()
    slug = urllib.parse.quote(name.strip().replace(" ", "+"), safe="+")
    url = f"https://www.last.fm/music/{slug}/+wiki"
    req = urllib.request.Request(
        url,
        headers={
            "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124 Safari/537.36",
            "Accept": "text/html,application/xhtml+xml",
            "Accept-Language": "tr,en;q=0.8",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=20) as r:
            page = r.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as e:
        if e.code == 404:
            return ""
        raise
    m = re.search(r'class="wiki-content"[^>]*>(.*?)</div>', page, re.S)
    if not m:
        return ""
    text = re.sub(r"<br\s*/?>", "\n", m.group(1))
    text = re.sub(r"</p>", "\n\n", text)
    text = re.sub(r"<[^>]+>", "", text)
    text = _html.unescape(text)
    text = re.sub(r"[ \t]+", " ", text)
    text = re.sub(r"\n{3,}", "\n\n", text).strip()
    # "We don't have a wiki..." gibi boş durum mesajlarını ele
    if len(text) < 40 or "don't have a wiki" in text.lower():
        return ""
    return text


def pick_artist(results, name):
    low = name.strip().lower()
    for r in results:
        if (r.get("artist") or r.get("title") or "").strip().lower() == low:
            return r
    return results[0] if results else None


def lookup(name, out_id, out_dir):
    if YTMusic is None:
        return {"ok": False, "error": "ytmusicapi yok: " + IMPORT_ERROR}
    api = yt()
    res = api.search(name, filter="artists", limit=5)
    hit = pick_artist(res, name)
    if not hit or not hit.get("browseId"):
        desc = lastfm_bio(name)
        return {"ok": True, "found": bool(desc), "name": name, "description": desc, "photo": None, "source": "lastfm" if desc else ""}
    try:
        art = yt_tr().get_artist(hit["browseId"])
    except Exception:
        art = api.get_artist(hit["browseId"])
    display = art.get("name") or name
    # açıklama: önce Last.fm (doğru kişiye ait), yoksa YouTube Music; Vikipedi kullanılmaz (aynı adlı şeylerle karışıyor)
    desc = lastfm_bio(display)
    source = "lastfm"
    if not desc:
        desc = (art.get("description") or "").strip()
        source = "ytmusic" if desc else ""
    photo = None
    thumbs = art.get("thumbnails") or hit.get("thumbnails") or []
    url = best_thumb(thumbs)
    if url:
        os.makedirs(out_dir, exist_ok=True)
        path = os.path.join(out_dir, out_id + ".jpg")
        for candidate in (enlarge(url), url):
            try:
                data = http_get(candidate)
                if len(data) > 1000:
                    with open(path, "wb") as f:
                        f.write(data)
                    photo = path
                    break
            except Exception:
                continue
    return {
        "ok": True,
        "found": True,
        "name": display,
        "description": desc,
        "photo": photo,
        "source": source,
        "browse_id": hit["browseId"],
    }


# ----------------------------------------------------------------------------- tür bilgisi
MB_UA = {"User-Agent": "Soundworks/0.1 ( kisisel muzik calar )", "Accept": "application/json"}
_last_mb = 0.0
_artist_tag_cache = {}

# (anahtar kelime listesi, Türkçe kategori) — özelden genele sıralı
CATEGORIES = [
    (("anatolian rock", "anadolu"), "Anadolu Rock"),
    (("arabesk", "arabesque"), "Arabesk"),
    (("turkish folk", "türk halk", "turku", "türkü", "halk müziği"), "Türk Halk Müziği"),
    (("turkish pop", "türk pop", "türkçe pop"), "Türkçe Pop"),
    (("hip hop", "hip-hop", "rap", "trap", "boom bap"), "Hip-Hop"),
    (("electronic", "electro", "techno", "house", "ambient", "synth", "dance", "idm", "trance", "dubstep", "elektronik"), "Elektronik"),
    (("metal",), "Metal"),
    (("punk", "hardcore"), "Punk"),
    (("jazz", "caz"), "Caz"),
    (("classical", "orchestral", "opera", "klasik"), "Klasik"),
    (("blues",), "Blues"),
    (("reggae", "ska", "dub"), "Reggae"),
    (("soul", "funk", "r&b", "rhythm and blues", "disco"), "Soul & Funk"),
    (("country", "bluegrass"), "Country"),
    (("psychedelic",), "Psychedelic"),
    (("progressive",), "Progresif Rock"),
    (("indie", "alternative", "alternatif", "shoegaze", "post-punk", "new wave", "grunge"), "Alternatif / Indie"),
    (("folk", "singer-songwriter", "acoustic"), "Folk"),
    (("rock",), "Rock"),
    (("pop",), "Pop"),
    (("soundtrack", "score", "film", "game"), "Film & Oyun"),
    (("latin", "salsa", "bossa", "samba"), "Latin"),
    (("world", "dünya"), "Dünya Müziği"),
]
TURKISH_SPECIFIC = {"Anadolu Rock", "Arabesk", "Türk Halk Müziği", "Türkçe Pop"}


def categorize(cands):
    mapped = []
    for c in cands:
        low = c.lower()
        for keys, cat in CATEGORIES:
            if any(k in low for k in keys):
                mapped.append(cat)
                break
        else:
            mapped.append(None)
    for m in mapped:
        if m in TURKISH_SPECIFIC:
            return m
    first = next((m for m in mapped if m), "")
    turkish = any(c.lower() in ("turkish", "turkey", "türkçe") or c.lower().startswith("turkish ") for c in cands)
    if turkish:
        return {"Folk": "Türk Halk Müziği", "Pop": "Türkçe Pop", "Rock": "Türkçe Rock", "Alternatif / Indie": "Türkçe Rock"}.get(first, first)
    return first


def lq(s):
    return s.replace("\\", "\\\\").replace('"', '\\"')


def mb_get(url):
    global _last_mb
    for attempt in range(4):
        wait = 1.1 - (time.time() - _last_mb)
        if wait > 0:
            time.sleep(wait)
        _last_mb = time.time()
        try:
            req = urllib.request.Request(url, headers=MB_UA)
            with urllib.request.urlopen(req, timeout=20) as r:
                return json.loads(r.read().decode("utf-8"))
        except urllib.error.HTTPError as e:
            if e.code in (429, 503):
                time.sleep(2 + attempt * 2)
                continue
            raise
    raise RuntimeError("MusicBrainz şu an meşgul")


def _tags(entry):
    tags = sorted(entry.get("tags", []) or [], key=lambda t: -t.get("count", 0))
    return [t["name"] for t in tags]


def _same_artist(a, credit):
    a = a.lower().strip()
    c = " ".join((x.get("name") or "") for x in credit).lower()
    return a in c or c in a or bool(set(a.split()) & set(c.split()))


def mb_album_tags(artist, album):
    q = f'releasegroup:"{lq(album)}" AND artist:"{lq(artist)}"'
    data = mb_get("https://musicbrainz.org/ws/2/release-group/?query=" + urllib.parse.quote(q) + "&fmt=json&limit=5")
    for rg in data.get("release-groups", []):
        if rg.get("score", 0) < 85 or not _same_artist(artist, rg.get("artist-credit", [])):
            continue
        t = _tags(rg)
        if t:
            return t
    return []


def mb_artist_tags(artist):
    k = artist.lower().strip()
    if k in _artist_tag_cache:
        return _artist_tag_cache[k]
    q = f'artist:"{lq(artist)}"'
    data = mb_get("https://musicbrainz.org/ws/2/artist/?query=" + urllib.parse.quote(q) + "&fmt=json&limit=3")
    tags = []
    for a in data.get("artists", []):
        if a.get("score", 0) >= 90 and (a.get("name", "").lower() == k or _same_artist(artist, [{"name": a.get("name", "")}])):
            tags = _tags(a)
            if tags:
                break
    _artist_tag_cache[k] = tags
    return tags


def deezer_album_genres(artist, album):
    q = urllib.parse.quote(f'artist:"{lq(artist)}" album:"{lq(album)}"')
    r = json.loads(http_get(f"https://api.deezer.com/search/album?q={q}&limit=3").decode("utf-8"))
    if not r.get("data"):
        return []
    alb = json.loads(http_get(f"https://api.deezer.com/album/{r['data'][0]['id']}").decode("utf-8"))
    return [g["name"] for g in alb.get("genres", {}).get("data", [])]


def genre_lookup(artist, album):
    if not artist or artist.lower() == "bilinmeyen sanatçı":
        return {"ok": True, "genre": "", "raw": [], "source": ""}
    # 1) MusicBrainz albüm etiketleri
    cands = mb_album_tags(artist, album) if album else []
    source = "musicbrainz"
    cat = categorize(cands)
    # 2) Deezer
    if not cat:
        try:
            cands = deezer_album_genres(artist, album)
            source = "deezer"
            cat = categorize(cands)
        except Exception:
            pass
    # 3) sanatçı düzeyinde MusicBrainz etiketleri
    if not cat:
        cands = mb_artist_tags(artist)
        source = "musicbrainz-artist"
        cat = categorize(cands)
    return {"ok": True, "genre": cat, "raw": cands[:6], "source": source if cat else ""}


def handle(req):
    try:
        if req.get("type") == "genre":
            return genre_lookup(req.get("artist", ""), req.get("album", ""))
        return lookup(req["name"], req["id"], req["dir"])
    except Exception as e:  # ağ hatası vb.: önbelleğe yazılmaz, sonra tekrar denenir
        return {"ok": False, "error": str(e)}


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stdin.reconfigure(encoding="utf-8")
    if len(sys.argv) >= 4 and sys.argv[1] == "--once":
        out = handle({"name": sys.argv[2], "id": "once", "dir": sys.argv[3]})
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
        print(json.dumps({"name": req.get("name"), **handle(req)}, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    main()
