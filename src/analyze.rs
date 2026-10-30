use anyhow::{anyhow, Result};
use rustfft::{num_complex::Complex, FftPlanner};
use std::fs::File;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

const FFT: usize = 4096;
pub const COLS: usize = 1200;
pub const ROWS: usize = 512;
const DB_FLOOR: f32 = -130.0;
const DB_CEIL: f32 = -10.0;
const LOSSY_EXTS: &[&str] = &["mp3", "aac", "ogg", "opus", "wma", "mp2"];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Verdict {
    /// Content reaches the top of the band: looks like genuine lossless.
    Full,
    /// Sharp brick-wall at the Nyquist of a lower rate: resampled up from 44.1/48 kHz.
    Upsampled,
    /// Sharp low-pass inside the audible range: typical of MP3/AAC transcodes.
    LossySuspect,
    /// The file itself is a lossy format, so a cutoff says nothing about fakery.
    LossyFormat,
    /// Too quiet to judge.
    Silent,
}

pub struct Report {
    pub rate: u32,
    pub bits: Option<u32>,
    pub chans: usize,
    pub secs: f64,
    pub peak_db: f64,
    pub rgb: Vec<u8>,
    pub avg_db: Vec<f64>,
    pub max_db: Vec<f32>,
    pub hz_per_bin: f64,
    pub cutoff_hz: Option<f64>,
    pub verdict: Verdict,
}

impl Report {
    pub fn nyquist(&self) -> f64 {
        self.rate as f64 / 2.0
    }
}

pub fn compute(path: &Path) -> Result<Report> {
    let mss = MediaSourceStream::new(Box::new(File::open(path)?), Default::default());
    let mut hint = Hint::new();
    let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase());
    if let Some(e) = &ext {
        hint.with_extension(e);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| anyhow!("{e}"))?;
    let mut format = probed.format;
    let t = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow!("ses yok"))?;
    let id = t.id;
    let rate = t.codec_params.sample_rate.ok_or_else(|| anyhow!("hız yok"))?;
    let bits = t.codec_params.bits_per_sample;
    let chans = t.codec_params.channels.map(|c| c.count()).unwrap_or(2);
    let mut dec = symphonia::default::get_codecs()
        .make(&t.codec_params, &DecoderOptions::default())
        .map_err(|e| anyhow!("{e}"))?;

    let mut mono: Vec<f32> = vec![];
    let mut peak = 0f32;
    while let Ok(p) = format.next_packet() {
        if p.track_id() != id {
            continue;
        }
        let Ok(d) = dec.decode(&p) else { continue };
        let spec = *d.spec();
        let mut sb = SampleBuffer::<f32>::new(d.capacity() as u64, spec);
        sb.copy_interleaved_ref(d);
        let ch = spec.channels.count();
        for f in sb.samples().chunks_exact(ch) {
            let m = if ch > 1 { (f[0] + f[1]) * 0.5 } else { f[0] };
            peak = peak.max(f.iter().fold(0f32, |a, v| a.max(v.abs())));
            mono.push(m);
        }
    }
    if mono.len() < FFT * 2 {
        return Err(anyhow!("dosya çok kısa"));
    }

    let win: Vec<f32> = (0..FFT).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / FFT as f32).cos()).collect();
    let fft = FftPlanner::<f32>::new().plan_fft_forward(FFT);
    let bins = FFT / 2;
    let hop = (mono.len() - FFT) as f64 / (COLS - 1) as f64;
    let mut spec_cols: Vec<Vec<f32>> = Vec::with_capacity(COLS);
    let mut avg = vec![0f64; bins];
    let mut max_db = vec![-200f32; bins];
    for c in 0..COLS {
        let start = (c as f64 * hop) as usize;
        let mut buf: Vec<Complex<f32>> = (0..FFT).map(|i| Complex::new(mono[start + i] * win[i], 0.0)).collect();
        fft.process(&mut buf);
        let col: Vec<f32> = buf[..bins].iter().map(|c| 20.0 * ((c.norm() / (FFT as f32 * 0.25)).max(1e-9)).log10()).collect();
        for (k, v) in col.iter().enumerate() {
            avg[k] += 10f64.powf(*v as f64 / 10.0);
            max_db[k] = max_db[k].max(*v);
        }
        spec_cols.push(col);
    }
    let avg_db: Vec<f64> = avg.iter().map(|v| 10.0 * (v / COLS as f64).max(1e-20).log10()).collect();
    let nyq = rate as f64 / 2.0;
    let hz_per_bin = nyq / bins as f64;
    let peak_db = 20.0 * (peak.max(1e-9) as f64).log10();
    let lossy_format = ext.as_deref().map_or(false, |e| LOSSY_EXTS.contains(&e));
    let (cutoff_hz, verdict) = judge(&avg_db, hz_per_bin, nyq, peak_db, lossy_format);

    let mut rgb = vec![0u8; COLS * ROWS * 3];
    for x in 0..COLS {
        for y in 0..ROWS {
            let f = (ROWS - 1 - y) as f64 / (ROWS - 1) as f64 * nyq;
            let k = ((f / hz_per_bin) as usize).min(bins - 1);
            let db = spec_cols[x][k].clamp(DB_FLOOR, DB_CEIL);
            let (r, g, b) = heat(((db - DB_FLOOR) / (DB_CEIL - DB_FLOOR)) as f64);
            let o = (y * COLS + x) * 3;
            rgb[o] = r;
            rgb[o + 1] = g;
            rgb[o + 2] = b;
        }
    }
    Ok(Report { rate, bits, chans, secs: mono.len() as f64 / rate as f64, peak_db, rgb, avg_db, max_db, hz_per_bin, cutoff_hz, verdict })
}

/// Looks for the steepest fall in the long-term average spectrum above 10 kHz. A lossy encoder's
/// low-pass drops tens of dB within a few hundred Hz and leaves nothing above it; natural roll-off is gradual.
fn judge(avg_db: &[f64], hz_per_bin: f64, nyq: f64, peak_db: f64, lossy_format: bool) -> (Option<f64>, Verdict) {
    if peak_db < -70.0 {
        return (None, Verdict::Silent);
    }
    let span = ((600.0 / hz_per_bin) as usize).max(4);
    let lo = (10_000.0 / hz_per_bin) as usize + span;
    let hi = avg_db.len().saturating_sub(span);
    let mean = |a: usize, b: usize| avg_db[a..b].iter().sum::<f64>() / (b - a) as f64;
    let mut best: Option<(usize, f64)> = None;
    for k in lo..hi {
        let drop = mean(k - span, k) - mean(k, k + span);
        if best.map_or(true, |(_, d)| drop > d) {
            best = Some((k, drop));
        }
    }
    let cliff = best.filter(|&(k, d)| d >= 25.0 && mean(k + span / 2, (k + 2 * span).min(avg_db.len())) < -95.0).map(|(k, _)| k as f64 * hz_per_bin);
    let verdict = match cliff {
        None => {
            if lossy_format { Verdict::LossyFormat } else { Verdict::Full }
        }
        Some(_) if lossy_format => Verdict::LossyFormat,
        Some(f) if nyq > 24_000.0 && (20_500.0..=24_500.0).contains(&f) => Verdict::Upsampled,
        Some(_) => Verdict::LossySuspect,
    };
    (cliff, verdict)
}

pub fn run(path: &Path, png_out: &Path) -> Result<()> {
    let r = compute(path)?;
    println!("Dosya      : {}", path.display());
    println!("Hız/derinlik: {} Hz, {} bit, {} kanal, {:.1} sn", r.rate, r.bits.map(|b| b.to_string()).unwrap_or("?".into()), r.chans, r.secs);
    println!("Tepe seviye: {:.2} dBFS", r.peak_db);

    println!("\nOrtalama seviye (dBFS) ve en yüksek anlık seviye, 1 kHz dilimleri:");
    let (nyq, bins) = (r.nyquist(), r.avg_db.len());
    let mut khz = 0.0;
    while khz < nyq / 1000.0 {
        let lo = (khz * 1000.0 / r.hz_per_bin) as usize;
        let hi = (((khz + 1.0) * 1000.0 / r.hz_per_bin) as usize).min(bins);
        if hi > lo {
            let a = r.avg_db[lo..hi].iter().cloned().fold(f64::MIN, f64::max);
            let m = r.max_db[lo..hi].iter().cloned().fold(-200f32, f32::max);
            println!("{:>5.0}-{:<5.0} kHz  ort {:>7.1}   tepe {:>7.1}", khz, (khz + 1.0).min(nyq / 1000.0), a, m);
        }
        khz += 1.0;
    }
    match r.cutoff_hz {
        Some(f) => println!("\nKesim      : {:.1} kHz ({:?})", f / 1000.0, r.verdict),
        None => println!("\nKesim      : yok ({:?})", r.verdict),
    }

    let mut img = r.rgb.clone();
    for khz in [16.0, 18.0, 20.0, 22.0] {
        if khz * 1000.0 < nyq {
            let y = ROWS - 1 - ((khz * 1000.0 / nyq) * (ROWS - 1) as f64) as usize;
            for x in (0..COLS).filter(|x| x % 6 < 3) {
                let o = (y * COLS + x) * 3;
                img[o..o + 3].copy_from_slice(&[255, 255, 255]);
            }
        }
    }
    let file = File::create(png_out)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), COLS as u32, ROWS as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&img)?;
    println!("Spektrogram: {} (beyaz kesikli çizgiler 16/18/20/22 kHz)", png_out.display());
    Ok(())
}

pub fn heat(t: f64) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    let (r, g, b) = if t < 0.25 {
        (0.0, 0.0, t / 0.25 * 0.6)
    } else if t < 0.5 {
        let u = (t - 0.25) / 0.25;
        (u * 0.6, 0.0, 0.6 + u * 0.1)
    } else if t < 0.75 {
        let u = (t - 0.5) / 0.25;
        (0.6 + u * 0.4, u * 0.6, 0.7 - u * 0.7)
    } else {
        let u = (t - 0.75) / 0.25;
        (1.0, 0.6 + u * 0.4, u)
    };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spectrum(nyq: f64, cut: Option<f64>) -> (Vec<f64>, f64) {
        let bins = 2048;
        let hz = nyq / bins as f64;
        let v = (0..bins)
            .map(|k| {
                let f = k as f64 * hz;
                match cut {
                    Some(c) if f > c => -125.0,
                    _ => -50.0 - f / 1000.0 * 2.0,
                }
            })
            .collect();
        (v, hz)
    }

    #[test]
    fn lowpass_inside_the_band_is_flagged_as_transcode() {
        let (v, hz) = spectrum(22_050.0, Some(16_000.0));
        let (cut, verdict) = judge(&v, hz, 22_050.0, -1.0, false);
        assert_eq!(verdict, Verdict::LossySuspect);
        assert!((cut.unwrap() - 16_000.0).abs() < 400.0);
    }

    #[test]
    fn full_band_content_passes() {
        let (v, hz) = spectrum(22_050.0, None);
        assert_eq!(judge(&v, hz, 22_050.0, -1.0, false).1, Verdict::Full);
    }

    #[test]
    fn cut_at_44k_nyquist_inside_a_hires_file_is_an_upsample() {
        let (v, hz) = spectrum(48_000.0, Some(22_050.0));
        assert_eq!(judge(&v, hz, 48_000.0, -1.0, false).1, Verdict::Upsampled);
    }

    #[test]
    fn lossy_formats_are_not_accused() {
        let (v, hz) = spectrum(22_050.0, Some(16_000.0));
        assert_eq!(judge(&v, hz, 22_050.0, -1.0, true).1, Verdict::LossyFormat);
    }

    #[test]
    fn near_silence_is_not_judged() {
        let (v, hz) = spectrum(22_050.0, Some(16_000.0));
        assert_eq!(judge(&v, hz, 22_050.0, -90.0, false).1, Verdict::Silent);
    }
}
