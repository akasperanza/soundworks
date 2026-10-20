//! DSD (DSF / DFF) desteği: kapsayıcı okuyucu, 1 bit -> PCM çevirici ve DoP (DSD over PCM) paketleyici.
//!
//! Bit sırası içeride her zaman "MSB önce = en eski bit"tir (DFF'in doğal sırası); DSF'te (LSB önce) bayt başına ters çevrilir.

use anyhow::{bail, Context, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub const DSD_EXTS: &[&str] = &["dsf", "dff"];

pub fn is_dsd(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).map(|e| DSD_EXTS.contains(&e.to_lowercase().as_str())).unwrap_or(false)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Dsf,
    Dff,
}

#[derive(Clone, Debug)]
pub struct DsdInfo {
    pub kind: Kind,
    /// DSD örnekleme hızı (Hz), ör. 2 822 400
    pub fs: u32,
    pub channels: usize,
    /// kanal başına bit sayısı
    pub bits_per_ch: u64,
    /// DSF: kanal başına blok boyutu (bayt)
    block: usize,
    data_off: u64,
    /// DSF'te bitler LSB önce saklanır
    lsb_first: bool,
    /// ID3v2 etiketinin (başlangıç, uzunluk) yeri
    id3: Option<(u64, u64)>,
}

impl DsdInfo {
    pub fn bytes_per_ch(&self) -> u64 {
        self.bits_per_ch.div_ceil(8)
    }
    pub fn secs(&self) -> f64 {
        self.bits_per_ch as f64 / self.fs as f64
    }
    /// DSD64 / DSD128 / ... (44.1 kHz ailesi 64×44100 = 2 822 400 baz alınır; 48 kHz ailesi için de aynı ad)
    pub fn name(&self) -> String {
        let base = if self.fs % 44100 == 0 { 44100 } else { 48000 };
        format!("DSD{}", self.fs / base)
    }
    /// 8 DSD baytına karşılık gelen PCM örneği: PCM çıkış hızı 352.8 kHz (44.1 ailesi) ya da 384 kHz (48 ailesi).
    pub fn pcm_rate(&self) -> u32 {
        if self.fs % 44100 == 0 { 352_800 } else { 384_000 }
    }
    /// Çıkış örneği başına ilerleyen bayt sayısı (DSD64'te 1, DSD128'de 2, ...).
    pub fn adv(&self) -> usize {
        (self.fs / self.pcm_rate() / 8).max(1) as usize
    }
    /// DoP için PCM hızı: 16 DSD biti = 1 PCM örneği.
    pub fn dop_rate(&self) -> u32 {
        self.fs / 16
    }
}

fn rd<const N: usize>(f: &mut File) -> Result<[u8; N]> {
    let mut b = [0u8; N];
    f.read_exact(&mut b).context("dosya beklenenden kısa")?;
    Ok(b)
}
fn le32(b: [u8; 4]) -> u32 {
    u32::from_le_bytes(b)
}
fn le64(b: [u8; 8]) -> u64 {
    u64::from_le_bytes(b)
}
fn be32(b: [u8; 4]) -> u32 {
    u32::from_be_bytes(b)
}
fn be64(b: [u8; 8]) -> u64 {
    u64::from_be_bytes(b)
}

pub fn probe(path: &Path) -> Result<DsdInfo> {
    let mut f = File::open(path).context("dosya açılamadı")?;
    let magic = rd::<4>(&mut f)?;
    f.seek(SeekFrom::Start(0))?;
    match &magic {
        b"DSD " => probe_dsf(&mut f),
        b"FRM8" => probe_dff(&mut f),
        _ => bail!("DSD dosyası değil"),
    }
}

fn probe_dsf(f: &mut File) -> Result<DsdInfo> {
    let _ = rd::<4>(f)?; // "DSD "
    let _chunk = le64(rd::<8>(f)?);
    let _total = le64(rd::<8>(f)?);
    let meta_ptr = le64(rd::<8>(f)?);
    if &rd::<4>(f)? != b"fmt " {
        bail!("DSF: fmt bölümü yok");
    }
    let fmt_size = le64(rd::<8>(f)?);
    let _version = le32(rd::<4>(f)?);
    let format_id = le32(rd::<4>(f)?);
    let _ch_type = le32(rd::<4>(f)?);
    let channels = le32(rd::<4>(f)?) as usize;
    let fs = le32(rd::<4>(f)?);
    let bps = le32(rd::<4>(f)?);
    let samples = le64(rd::<8>(f)?);
    let block = le32(rd::<4>(f)?) as usize;
    let _ = rd::<4>(f)?;
    if format_id != 0 {
        bail!("DSF: desteklenmeyen biçim");
    }
    if channels == 0 || block == 0 || fs == 0 {
        bail!("DSF: geçersiz başlık");
    }
    // fmt bölümü beklenenden uzunsa atla
    f.seek(SeekFrom::Start(28 + fmt_size))?;
    if &rd::<4>(f)? != b"data" {
        bail!("DSF: data bölümü yok");
    }
    let _data_size = le64(rd::<8>(f)?);
    let data_off = f.stream_position()?;
    let id3 = if meta_ptr > 0 {
        let len = f.metadata()?.len().saturating_sub(meta_ptr);
        (len > 10).then_some((meta_ptr, len))
    } else {
        None
    };
    Ok(DsdInfo { kind: Kind::Dsf, fs, channels, bits_per_ch: samples, block, data_off, lsb_first: bps == 1, id3 })
}

fn probe_dff(f: &mut File) -> Result<DsdInfo> {
    let _ = rd::<4>(f)?; // "FRM8"
    let frm_size = be64(rd::<8>(f)?);
    if &rd::<4>(f)? != b"DSD " {
        bail!("DFF: DSD biçimi değil");
    }
    let end = (12 + frm_size).min(f.metadata()?.len());
    let (mut fs, mut channels, mut data_off, mut data_len, mut id3) = (0u32, 0usize, 0u64, 0u64, None);
    while f.stream_position()? + 12 <= end {
        let id = rd::<4>(f)?;
        let size = be64(rd::<8>(f)?);
        let body = f.stream_position()?;
        match &id {
            b"PROP" => {
                if &rd::<4>(f)? == b"SND " {
                    let prop_end = body + size;
                    while f.stream_position()? + 12 <= prop_end {
                        let sid = rd::<4>(f)?;
                        let ssize = be64(rd::<8>(f)?);
                        let sbody = f.stream_position()?;
                        match &sid {
                            b"FS  " => fs = be32(rd::<4>(f)?),
                            b"CHNL" => channels = u16::from_be_bytes(rd::<2>(f)?) as usize,
                            b"CMPR" => {
                                if &rd::<4>(f)? != b"DSD " {
                                    bail!("DFF: DST sıkıştırmalı dosyalar desteklenmiyor");
                                }
                            }
                            _ => {}
                        }
                        f.seek(SeekFrom::Start(sbody + ssize + (ssize & 1)))?;
                    }
                }
            }
            b"DSD " => {
                data_off = body;
                data_len = size;
            }
            b"ID3 " => id3 = Some((body, size)),
            _ => {}
        }
        f.seek(SeekFrom::Start(body + size + (size & 1)))?;
    }
    if fs == 0 || channels == 0 || data_off == 0 {
        bail!("DFF: başlık eksik");
    }
    Ok(DsdInfo { kind: Kind::Dff, fs, channels, bits_per_ch: data_len / channels as u64 * 8, block: 0, data_off, lsb_first: false, id3 })
}

fn rev_table() -> &'static [u8; 256] {
    static T: std::sync::OnceLock<[u8; 256]> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u8; 256];
        for (i, v) in t.iter_mut().enumerate() {
            *v = (i as u8).reverse_bits();
        }
        t
    })
}

/// DSD bayt akışı: kanal başına iki akış (sol, sağ). Çok kanallı dosyalarda ilk iki kanal kullanılır.
pub struct DsdSource {
    file: File,
    pub info: DsdInfo,
    /// kanal başına okunan bayt
    pos: u64,
}

impl DsdSource {
    pub fn open(path: &Path) -> Result<DsdSource> {
        let info = probe(path)?;
        Ok(DsdSource { file: File::open(path)?, info, pos: 0 })
    }

    pub fn seek_secs(&mut self, secs: f64) {
        let b = (secs.max(0.0) * self.info.fs as f64 / 8.0) as u64;
        self.pos = (b & !1).min(self.info.bytes_per_ch());
    }

    pub fn pos_secs(&self) -> f64 {
        self.pos as f64 * 8.0 / self.info.fs as f64
    }

    /// En fazla `want` bayt (kanal başına) okur; akış bittiyse `None`.
    pub fn read(&mut self, want: usize) -> Result<Option<(Vec<u8>, Vec<u8>)>> {
        let total = self.info.bytes_per_ch();
        let take = (total - self.pos).min(want as u64) as usize;
        if take == 0 {
            return Ok(None);
        }
        let ch = self.info.channels;
        let (mut l, mut r) = (Vec::with_capacity(take), Vec::with_capacity(take));
        match self.info.kind {
            Kind::Dff => {
                let mut raw = vec![0u8; take * ch];
                self.file.seek(SeekFrom::Start(self.info.data_off + self.pos * ch as u64))?;
                self.file.read_exact(&mut raw)?;
                for f in raw.chunks_exact(ch) {
                    l.push(f[0]);
                    r.push(if ch > 1 { f[1] } else { f[0] });
                }
            }
            Kind::Dsf => {
                let block = self.info.block;
                let group = block * ch;
                let g0 = self.pos as usize / block;
                let g1 = (self.pos as usize + take - 1) / block;
                let mut raw = vec![0u8; (g1 - g0 + 1) * group];
                self.file.seek(SeekFrom::Start(self.info.data_off + (g0 * group) as u64))?;
                self.file.read_exact(&mut raw)?;
                let start = self.pos as usize - g0 * block;
                let mut i = 0usize;
                while l.len() < take {
                    let (g, o) = ((start + i) / block, (start + i) % block);
                    let base = g * group;
                    let n = (block - o).min(take - l.len());
                    l.extend_from_slice(&raw[base + o..base + o + n]);
                    let rc = if ch > 1 { block } else { 0 };
                    r.extend_from_slice(&raw[base + rc + o..base + rc + o + n]);
                    i += n;
                }
            }
        }
        if self.info.lsb_first {
            let t = rev_table();
            l.iter_mut().for_each(|b| *b = t[*b as usize]);
            r.iter_mut().for_each(|b| *b = t[*b as usize]);
        }
        self.pos += take as u64;
        Ok(Some((l, r)))
    }
}

// ------------------------------------------------------------------ DSD -> PCM

/// DSD sessizlik deseni (her iki baytta 0x69): DC'siz, 0 genlik.
pub const DSD_SILENCE: u8 = 0x69;

/// Tek aşamalı kutu-tablolu (bayt başına) FIR seyreltici: DSD bit akışı -> 352.8 / 384 kHz PCM.
/// Süzgeç uzunluğu DSD hızıyla büyür (DSD64: 256 tap, DSD128: 512, ...), kesim ~250 kHz: 24 kHz altına katlanacak
/// bölge (çıkış hızı ± 22 kHz) güçlü bastırılır, işitilebilir bandın tamamı düz geçer.
pub struct Dsd2Pcm {
    adv: usize,
    s: usize,
    lut: Vec<[f32; 256]>,
    buf: [Vec<u8>; 2],
    next_end: usize,
}

fn blackman_harris(n: usize, m: usize) -> f64 {
    let x = 2.0 * std::f64::consts::PI * n as f64 / (m - 1) as f64;
    0.35875 - 0.48829 * x.cos() + 0.14128 * (2.0 * x).cos() - 0.01168 * (3.0 * x).cos()
}

impl Dsd2Pcm {
    pub fn new(info: &DsdInfo) -> Dsd2Pcm {
        let adv = info.adv();
        let s = 32 * adv;
        let m = 8 * s;
        let fcn = 250_000.0 / info.fs as f64; // kesim (döngü/örnek)
        let mid = (m - 1) as f64 / 2.0;
        let mut h: Vec<f64> = (0..m)
            .map(|k| {
                let x = k as f64 - mid;
                let sinc = if x.abs() < 1e-12 { 1.0 } else { (2.0 * std::f64::consts::PI * fcn * x).sin() / (std::f64::consts::PI * x) };
                sinc * blackman_harris(k, m)
            })
            .collect();
        let sum: f64 = h.iter().sum();
        h.iter_mut().for_each(|v| *v /= sum);
        // LUT_j[b] = Σ_t (±1) * h[8j + 7 - t]; t = 0 en eski bit (baytın MSB'si)
        let lut: Vec<[f32; 256]> = (0..s)
            .map(|j| {
                let mut t = [0f32; 256];
                for (b, v) in t.iter_mut().enumerate() {
                    let mut acc = 0f64;
                    for bit in 0..8 {
                        let x = if (b >> (7 - bit)) & 1 == 1 { 1.0 } else { -1.0 };
                        acc += x * h[8 * j + 7 - bit];
                    }
                    *v = acc as f32;
                }
                t
            })
            .collect();
        let mut c = Dsd2Pcm { adv, s, lut, buf: [vec![], vec![]], next_end: 0 };
        c.reset();
        c
    }

    /// Geçmişi sessizlikle doldurur (seek / başlangıç).
    pub fn reset(&mut self) {
        for b in self.buf.iter_mut() {
            *b = vec![DSD_SILENCE; self.s - 1];
        }
        self.next_end = self.s - 1;
    }

    /// Yeni baytlardan PCM üretir: interleaved stereo f32.
    pub fn process(&mut self, l: &[u8], r: &[u8]) -> Vec<f32> {
        self.buf[0].extend_from_slice(l);
        self.buf[1].extend_from_slice(r);
        let mut out = Vec::with_capacity((l.len() / self.adv + 1) * 2);
        let len = self.buf[0].len();
        while self.next_end < len {
            let e = self.next_end;
            for ch in 0..2 {
                let b = &self.buf[ch];
                let mut acc = 0f32;
                for j in 0..self.s {
                    acc += self.lut[j][b[e - j] as usize];
                }
                out.push(acc);
            }
            self.next_end += self.adv;
        }
        let keep_from = self.next_end + 1 - self.s;
        if keep_from > 0 {
            self.buf[0].drain(..keep_from);
            self.buf[1].drain(..keep_from);
            self.next_end -= keep_from;
        }
        out
    }
}

// ------------------------------------------------------------------ DoP

/// DoP işaret baytı: çerçeve başına dönüşümlü 0x05 / 0xFA.
pub fn dop_marker(odd_frame: bool) -> u32 {
    if odd_frame { 0xFA } else { 0x05 }
}

/// 24 bitlik DoP sözcüğü: üst bayt işaret, alt 16 bit DSD (en eski bit en üstte).
pub fn dop_word(odd_frame: bool, dsd16: u16) -> u32 {
    (dop_marker(odd_frame) << 16) | dsd16 as u32
}

/// Çift sayıda bayttan (kanal başına) DoP için 16 bitlik DSD değerleri üretir: interleaved stereo, değerler `f32` içinde
/// tam sayı (0..65535). İşaret baytını çıkış tarafı ekler, böylece duraklatma/arabellek boşalmasında işaret düzeni bozulmaz.
pub fn dop_pairs(l: &[u8], r: &[u8]) -> Vec<f32> {
    let n = l.len() / 2;
    let mut out = Vec::with_capacity(n * 2);
    for i in 0..n {
        out.push(((l[2 * i] as u16) << 8 | l[2 * i + 1] as u16) as f32);
        out.push(((r[2 * i] as u16) << 8 | r[2 * i + 1] as u16) as f32);
    }
    out
}

// ------------------------------------------------------------------ ID3v2 etiketleri

#[derive(Default, Clone, Debug)]
pub struct Tags {
    pub title: String,
    pub artist: String,
    pub album_artist: String,
    pub album: String,
    pub track_no: u32,
    pub disc_no: u32,
    pub cover: Option<Vec<u8>>,
}

fn syncsafe(b: &[u8]) -> usize {
    b.iter().fold(0usize, |a, x| (a << 7) | (*x as usize & 0x7f))
}

fn decode_text(enc: u8, b: &[u8]) -> String {
    let s = match enc {
        0 => b.iter().map(|c| *c as char).collect::<String>(),
        1 | 2 => {
            let (mut data, mut be) = (b, enc == 2);
            if enc == 1 && data.len() >= 2 {
                if data[0] == 0xFE && data[1] == 0xFF {
                    be = true;
                    data = &data[2..];
                } else if data[0] == 0xFF && data[1] == 0xFE {
                    data = &data[2..];
                }
            }
            let u: Vec<u16> = data.chunks_exact(2).map(|c| if be { u16::from_be_bytes([c[0], c[1]]) } else { u16::from_le_bytes([c[0], c[1]]) }).collect();
            String::from_utf16_lossy(&u)
        }
        _ => String::from_utf8_lossy(b).to_string(),
    };
    s.trim_matches('\0').trim().to_string()
}

fn leading_num(s: &str) -> u32 {
    s.trim().split(|c: char| !c.is_ascii_digit()).next().and_then(|x| x.parse().ok()).unwrap_or(0)
}

/// Bellekteki ID3v2.3 / 2.4 etiketini ayrıştırır.
pub fn parse_id3(d: &[u8]) -> Tags {
    let mut t = Tags::default();
    if d.len() < 10 || &d[..3] != b"ID3" {
        return t;
    }
    let major = d[3];
    if major != 3 && major != 4 {
        return t;
    }
    let flags = d[5];
    let total = (10 + syncsafe(&d[6..10])).min(d.len());
    let mut p = 10;
    if flags & 0x40 != 0 && p + 4 <= total {
        // genişletilmiş başlık
        p += if major == 4 { syncsafe(&d[p..p + 4]) } else { u32::from_be_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]]) as usize + 4 };
    }
    while p + 10 <= total {
        let id = &d[p..p + 4];
        if id[0] == 0 {
            break;
        }
        let size = if major == 4 { syncsafe(&d[p + 4..p + 8]) } else { u32::from_be_bytes([d[p + 4], d[p + 5], d[p + 6], d[p + 7]]) as usize };
        let body_start = p + 10;
        let body_end = (body_start + size).min(total);
        if body_start > body_end {
            break;
        }
        let body = &d[body_start..body_end];
        let text = || if body.is_empty() { String::new() } else { decode_text(body[0], &body[1..]) };
        match id {
            b"TIT2" => t.title = text(),
            b"TPE1" => t.artist = text(),
            b"TPE2" => t.album_artist = text(),
            b"TALB" => t.album = text(),
            b"TRCK" => t.track_no = leading_num(&text()),
            b"TPOS" => t.disc_no = leading_num(&text()),
            b"APIC" if t.cover.is_none() && body.len() > 4 => {
                let enc = body[0];
                if let Some(z) = body[1..].iter().position(|c| *c == 0) {
                    let mut q = 1 + z + 1 + 1; // mime\0 + resim türü
                    // açıklama (kodlamaya göre bir ya da iki baytlık sonlandırıcı)
                    if enc == 1 || enc == 2 {
                        while q + 1 < body.len() && !(body[q] == 0 && body[q + 1] == 0) {
                            q += 2;
                        }
                        q += 2;
                    } else {
                        while q < body.len() && body[q] != 0 {
                            q += 1;
                        }
                        q += 1;
                    }
                    if q < body.len() {
                        t.cover = Some(body[q..].to_vec());
                    }
                }
            }
            _ => {}
        }
        p = body_end;
    }
    t
}

/// Dosyanın ID3 etiketleri (yoksa boş).
pub fn read_tags(path: &Path, info: &DsdInfo) -> Tags {
    let Some((off, len)) = info.id3 else { return Tags::default() };
    let Ok(mut f) = File::open(path) else { return Tags::default() };
    if f.seek(SeekFrom::Start(off)).is_err() {
        return Tags::default();
    }
    let mut buf = vec![];
    let _ = f.by_ref().take(len.min(32 << 20)).read_to_end(&mut buf);
    parse_id3(&buf)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// 2. derece sigma-delta modülatör: [-1, 1] girişi 1 bit akışa çevirir (test sinyali üretmek için).
    pub fn sigma_delta(x: &[f64]) -> Vec<bool> {
        let (mut i1, mut i2, mut y) = (0.0f64, 0.0f64, 1.0f64);
        x.iter()
            .map(|v| {
                i1 += v - y;
                i2 += i1 - y;
                y = if i2 >= 0.0 { 1.0 } else { -1.0 };
                y > 0.0
            })
            .collect()
    }

    pub(crate) fn pack_msb(bits: &[bool]) -> Vec<u8> {
        bits.chunks(8).map(|c| c.iter().enumerate().fold(0u8, |a, (i, b)| a | ((*b as u8) << (7 - i)))).collect()
    }

    pub(crate) fn tone_bits(fs: f64, f: f64, amp: f64, secs: f64) -> Vec<bool> {
        let n = (fs * secs) as usize;
        let x: Vec<f64> = (0..n).map(|i| amp * (2.0 * std::f64::consts::PI * f * i as f64 / fs).sin()).collect();
        sigma_delta(&x)
    }

    pub(crate) fn write_dsf(path: &Path, fs: u32, l: &[u8], r: &[u8], id3: &[u8]) {
        let block = 4096usize;
        let groups = l.len().div_ceil(block);
        let mut data = vec![0u8; groups * block * 2];
        let rev = rev_table();
        for g in 0..groups {
            for i in 0..block {
                let idx = g * block + i;
                data[g * block * 2 + i] = rev[*l.get(idx).unwrap_or(&0x69) as usize]; // DSF: LSB önce
                data[g * block * 2 + block + i] = rev[*r.get(idx).unwrap_or(&0x69) as usize];
            }
        }
        let mut b = vec![];
        b.extend(b"DSD ");
        b.extend(28u64.to_le_bytes());
        let total = 28 + 52 + 12 + data.len() as u64 + id3.len() as u64;
        b.extend(total.to_le_bytes());
        let meta = if id3.is_empty() { 0u64 } else { 28 + 52 + 12 + data.len() as u64 };
        b.extend(meta.to_le_bytes());
        b.extend(b"fmt ");
        b.extend(52u64.to_le_bytes());
        for v in [1u32, 0, 2, 2, fs, 1] {
            b.extend(v.to_le_bytes());
        }
        b.extend((l.len() as u64 * 8).to_le_bytes());
        b.extend((block as u32).to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend(b"data");
        b.extend((12 + data.len() as u64).to_le_bytes());
        b.extend(&data);
        b.extend(id3);
        std::fs::write(path, b).unwrap();
    }

    fn write_dff(path: &Path, fs: u32, l: &[u8], r: &[u8]) {
        let mut prop = vec![];
        prop.extend(b"SND ");
        prop.extend(b"FS  ");
        prop.extend(4u64.to_be_bytes());
        prop.extend(fs.to_be_bytes());
        prop.extend(b"CHNL");
        prop.extend(10u64.to_be_bytes());
        prop.extend(2u16.to_be_bytes());
        prop.extend(b"SLFTSRGT");
        prop.extend(b"CMPR");
        prop.extend(4u64.to_be_bytes());
        prop.extend(b"DSD ");
        let mut data = vec![];
        for i in 0..l.len() {
            data.push(l[i]);
            data.push(r[i]);
        }
        let mut body = vec![];
        body.extend(b"DSD ");
        body.extend(b"FVER");
        body.extend(4u64.to_be_bytes());
        body.extend(0x01050000u32.to_be_bytes());
        body.extend(b"PROP");
        body.extend((prop.len() as u64).to_be_bytes());
        body.extend(&prop);
        body.extend(b"DSD ");
        body.extend((data.len() as u64).to_be_bytes());
        body.extend(&data);
        let mut b = vec![];
        b.extend(b"FRM8");
        b.extend((body.len() as u64).to_be_bytes());
        b.extend(&body);
        std::fs::write(path, b).unwrap();
    }

    /// Goertzel: `f` Hz bileşeninin genliği (tam sayıda periyot içeren `n` örnek üzerinde).
    fn tone_amp(pcm: &[f32], rate: f64, f: f64, skip: usize, n: usize) -> f64 {
        let x: Vec<f64> = pcm.chunks_exact(2).skip(skip).take(n).map(|fr| fr[0] as f64).collect();
        assert_eq!(x.len(), n);
        let (mut re, mut im) = (0.0, 0.0);
        for (i, v) in x.iter().enumerate() {
            let a = 2.0 * std::f64::consts::PI * f * i as f64 / rate;
            re += v * a.cos();
            im -= v * a.sin();
        }
        2.0 * (re * re + im * im).sqrt() / n as f64
    }

    fn convert_all(path: &Path) -> (DsdInfo, Vec<f32>) {
        let mut src = DsdSource::open(path).unwrap();
        let info = src.info.clone();
        let mut conv = Dsd2Pcm::new(&info);
        let mut pcm = vec![];
        while let Some((l, r)) = src.read(4096).unwrap() {
            pcm.extend(conv.process(&l, &r));
        }
        (info, pcm)
    }

    #[test]
    fn dsf_tone_converts_to_correct_level_and_frequency() {
        let dir = std::env::temp_dir().join("sw_dsd_test");
        std::fs::create_dir_all(&dir).unwrap();
        let fs = 2_822_400u32;
        let bits = tone_bits(fs as f64, 1000.0, 0.5, 0.5);
        let bytes = pack_msb(&bits);
        let p = dir.join("t.dsf");
        write_dsf(&p, fs, &bytes, &bytes, &[]);
        let (info, pcm) = convert_all(&p);
        assert_eq!((info.fs, info.channels, info.name().as_str(), info.pcm_rate(), info.dop_rate()), (fs, 2, "DSD64", 352_800, 176_400));
        assert!((info.secs() - 0.5).abs() < 0.01);
        // 0.5 genlikli 1 kHz sinüs: 100 periyot = 35280 örnek
        let a1 = tone_amp(&pcm, 352_800.0, 1000.0, 2000, 35_280);
        assert!((a1 - 0.5).abs() < 0.01, "1 kHz genliği {a1}");
        assert!(tone_amp(&pcm, 352_800.0, 3000.0, 2000, 35_280) < 0.005, "3 kHz sızıntısı");
        // çıkış uzunluğu ≈ bayt sayısı
        assert!((pcm.len() as i64 / 2 - bytes.len() as i64).abs() <= 2);
    }

    #[test]
    fn dff_matches_dsf_for_the_same_signal() {
        let dir = std::env::temp_dir().join("sw_dsd_test");
        std::fs::create_dir_all(&dir).unwrap();
        let fs = 2_822_400u32;
        let bytes = pack_msb(&tone_bits(fs as f64, 2000.0, 0.4, 0.2));
        let (a, b) = (dir.join("a.dsf"), dir.join("a.dff"));
        write_dsf(&a, fs, &bytes, &bytes, &[]);
        write_dff(&b, fs, &bytes, &bytes);
        let (_, pa) = convert_all(&a);
        let (ib, pb) = convert_all(&b);
        assert_eq!(ib.kind, Kind::Dff);
        let n = pa.len().min(pb.len()) - 400;
        for i in 0..n {
            assert!((pa[i] - pb[i]).abs() < 1e-6, "i={i}");
        }
    }

    #[test]
    fn dsd128_uses_longer_filter_and_same_pcm_rate() {
        let dir = std::env::temp_dir().join("sw_dsd_test");
        std::fs::create_dir_all(&dir).unwrap();
        let fs = 5_644_800u32;
        let bytes = pack_msb(&tone_bits(fs as f64, 1000.0, 0.5, 0.3));
        let p = dir.join("t128.dsf");
        write_dsf(&p, fs, &bytes, &bytes, &[]);
        let (info, pcm) = convert_all(&p);
        assert_eq!((info.name().as_str(), info.pcm_rate(), info.adv(), info.dop_rate()), ("DSD128", 352_800, 2, 352_800));
        let a1 = tone_amp(&pcm, 352_800.0, 1000.0, 2000, 35_280);
        assert!((a1 - 0.5).abs() < 0.01, "1 kHz genliği {a1}");
    }

    #[test]
    fn silence_pattern_converts_to_silence() {
        let info = DsdInfo { kind: Kind::Dsf, fs: 2_822_400, channels: 2, bits_per_ch: 0, block: 4096, data_off: 0, lsb_first: false, id3: None };
        let mut c = Dsd2Pcm::new(&info);
        let z = vec![DSD_SILENCE; 8192];
        let out = c.process(&z, &z);
        assert!(out.iter().all(|v| v.abs() < 2e-3), "sessizlik gürültülü");
    }

    #[test]
    fn seek_reads_from_the_right_position() {
        let dir = std::env::temp_dir().join("sw_dsd_test");
        std::fs::create_dir_all(&dir).unwrap();
        let fs = 2_822_400u32;
        let n = 20_000usize;
        let l: Vec<u8> = (0..n).map(|i| (i % 251) as u8).collect();
        let r: Vec<u8> = (0..n).map(|i| (i % 241) as u8).collect();
        for (name, dsf) in [("s.dsf", true), ("s.dff", false)] {
            let p = dir.join(name);
            if dsf { write_dsf(&p, fs, &l, &r, &[]) } else { write_dff(&p, fs, &l, &r) }
            let mut s = DsdSource::open(&p).unwrap();
            s.seek_secs(5000.0 * 8.0 / fs as f64); // bayt 5000
            let (a, b) = s.read(100).unwrap().unwrap();
            assert_eq!(a, l[5000..5100], "{name} L");
            assert_eq!(b, r[5000..5100], "{name} R");
            // blok sınırını aşan okuma ve dosya sonu
            s.seek_secs(4090.0 * 8.0 / fs as f64);
            let (a, _) = s.read(20).unwrap().unwrap();
            assert_eq!(a, l[4090..4110]);
            s.seek_secs((n as f64 - 4.0) * 8.0 / fs as f64);
            let (a, _) = s.read(100).unwrap().unwrap();
            assert_eq!(a.len(), 4);
            assert!(s.read(10).unwrap().is_none());
        }
    }

    #[test]
    fn dop_framing_roundtrip_is_bit_exact() {
        let fs = 2_822_400u32;
        let bits = tone_bits(fs as f64, 1000.0, 0.5, 0.05);
        let bytes = pack_msb(&bits);
        let bytes = &bytes[..bytes.len() & !1];
        let words = dop_pairs(bytes, bytes);
        assert_eq!(words.len(), bytes.len()); // 2 bayt/kanal -> 1 çerçeve (2 örnek)
        // çıkış tarafı gibi işaret ekle ve DSD'yi geri çöz
        let mut rebuilt = vec![];
        let mut odd = false;
        for f in words.chunks_exact(2) {
            let w = dop_word(odd, f[0] as u16);
            assert_eq!(w >> 16, if odd { 0xFA } else { 0x05 });
            rebuilt.push((w & 0xFFFF) as u16);
            odd = !odd;
        }
        let orig: Vec<u16> = bytes.chunks_exact(2).map(|c| (c[0] as u16) << 8 | c[1] as u16).collect();
        assert_eq!(rebuilt, orig);
        // f32 içinde 24 bitlik sözcük kayıpsız taşınır
        for w in [dop_word(true, 0xFFFF), dop_word(false, 0x0001), dop_word(true, 0x6969)] {
            assert_eq!((w as f32) as u32, w);
        }
        // DSD sessizliği 0x6969
        assert_eq!(dop_pairs(&[DSD_SILENCE, DSD_SILENCE], &[DSD_SILENCE, DSD_SILENCE]), vec![0x6969 as f32, 0x6969 as f32]);
    }

    #[test]
    fn id3_tags_are_read_from_dsf() {
        fn frame(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut f = vec![];
            f.extend(id);
            f.extend((body.len() as u32).to_be_bytes());
            f.extend([0, 0]);
            f.extend(body);
            f
        }
        let mut frames = vec![];
        frames.extend(frame(b"TIT2", &[&[3u8][..], "Parça Adı".as_bytes()].concat()));
        frames.extend(frame(b"TPE1", &[&[3u8][..], "Sanatçı".as_bytes()].concat()));
        frames.extend(frame(b"TALB", &[&[1u8, 0xFF, 0xFE][..], &"Alb".encode_utf16().flat_map(|u| u.to_le_bytes()).collect::<Vec<u8>>()].concat()));
        frames.extend(frame(b"TRCK", &[&[0u8][..], b"3/12"].concat()));
        frames.extend(frame(b"APIC", &[&[0u8][..], b"image/png\0", &[3u8][..], b"kapak\0", &[1, 2, 3, 4][..]].concat()));
        let n = frames.len();
        let mut id3 = vec![b'I', b'D', b'3', 3, 0, 0, ((n >> 21) & 0x7f) as u8, ((n >> 14) & 0x7f) as u8, ((n >> 7) & 0x7f) as u8, (n & 0x7f) as u8];
        id3.extend(frames);
        let t = parse_id3(&id3);
        assert_eq!((t.title.as_str(), t.artist.as_str(), t.album.as_str(), t.track_no), ("Parça Adı", "Sanatçı", "Alb", 3));
        assert_eq!(t.cover.as_deref(), Some(&[1u8, 2, 3, 4][..]));
        // dosyadan okuma
        let dir = std::env::temp_dir().join("sw_dsd_test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("tag.dsf");
        write_dsf(&p, 2_822_400, &[DSD_SILENCE; 100], &[DSD_SILENCE; 100], &id3);
        let info = probe(&p).unwrap();
        assert_eq!(read_tags(&p, &info).title, "Parça Adı");
    }

    #[test]
    fn rejects_non_dsd_files() {
        let dir = std::env::temp_dir().join("sw_dsd_test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x.dsf");
        std::fs::write(&p, b"RIFF....WAVE").unwrap();
        assert!(probe(&p).is_err());
    }
}
