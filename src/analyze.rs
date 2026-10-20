//! Spektrum analizi: dosyayı yerel örnekleme hızında çözer, spektrogramı PNG olarak yazar.

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
const COLS: usize = 1200;
const ROWS: usize = 512;

pub fn run(path: &Path, png_out: &Path) -> Result<()> {
    let mss = MediaSourceStream::new(Box::new(File::open(path)?), Default::default());
    let mut hint = Hint::new();
    if let Some(e) = path.extension().and_then(|e| e.to_str()) {
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
    println!("Dosya      : {}", path.display());
    println!("Hız/derinlik: {} Hz, {} bit, {} kanal, {:.1} sn", rate, bits.map(|b| b.to_string()).unwrap_or("?".into()), chans, mono.len() as f64 / rate as f64);
    println!("Tepe seviye: {:.2} dBFS", 20.0 * (peak.max(1e-9) as f64).log10());

    let win: Vec<f32> = (0..FFT).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / FFT as f32).cos()).collect();
    let fft = FftPlanner::<f32>::new().plan_fft_forward(FFT);
    let bins = FFT / 2;
    let hop = (mono.len() - FFT) as f64 / (COLS - 1) as f64;
    let mut spec_cols: Vec<Vec<f32>> = Vec::with_capacity(COLS);
    let mut avg = vec![0f64; bins];
    let mut maxs = vec![-200f32; bins];
    for c in 0..COLS {
        let start = (c as f64 * hop) as usize;
        let mut buf: Vec<Complex<f32>> = (0..FFT).map(|i| Complex::new(mono[start + i] * win[i], 0.0)).collect();
        fft.process(&mut buf);
        let col: Vec<f32> = buf[..bins]
            .iter()
            .map(|c| 20.0 * ((c.norm() / (FFT as f32 * 0.25)).max(1e-9)).log10())
            .collect();
        for (k, v) in col.iter().enumerate() {
            avg[k] += 10f64.powf(*v as f64 / 10.0);
            maxs[k] = maxs[k].max(*v);
        }
        spec_cols.push(col);
    }
    let avg_db: Vec<f64> = avg.iter().map(|v| 10.0 * (v / COLS as f64).max(1e-20).log10()).collect();

    println!("\nOrtalama seviye (dBFS) ve en yüksek anlık seviye, 1 kHz dilimleri:");
    let nyq = rate as f64 / 2.0;
    let hz_per_bin = nyq / bins as f64;
    let mut khz = 0.0;
    while khz < nyq / 1000.0 {
        let lo = (khz * 1000.0 / hz_per_bin) as usize;
        let hi = (((khz + 1.0) * 1000.0 / hz_per_bin) as usize).min(bins);
        if hi > lo {
            let a = avg_db[lo..hi].iter().cloned().fold(f64::MIN, f64::max);
            let m = maxs[lo..hi].iter().cloned().fold(-200f32, f32::max);
            println!("{:>5.0}-{:<5.0} kHz  ort {:>7.1}   tepe {:>7.1}", khz, (khz + 1.0).min(nyq / 1000.0), a, m);
        }
        khz += 1.0;
    }

    // PNG
    let (w, h) = (COLS as u32, ROWS as u32);
    let mut img = vec![0u8; (w * h * 3) as usize];
    for x in 0..COLS {
        for y in 0..ROWS {
            // y=0 üst = Nyquist
            let f = (ROWS - 1 - y) as f64 / (ROWS - 1) as f64 * nyq;
            let k = ((f / hz_per_bin) as usize).min(bins - 1);
            let db = spec_cols[x][k].clamp(-130.0, -10.0);
            let t = (db + 130.0) / 120.0;
            let (r, g, b) = heat(t as f64);
            let o = (y * COLS + x) * 3;
            img[o] = r;
            img[o + 1] = g;
            img[o + 2] = b;
        }
    }
    // 16/18/20/22 kHz çizgileri
    for khz in [16.0, 18.0, 20.0, 22.0] {
        if khz * 1000.0 < nyq {
            let y = ROWS - 1 - ((khz * 1000.0 / nyq) * (ROWS - 1) as f64) as usize;
            for x in 0..COLS {
                let o = (y * COLS + x) * 3;
                if x % 6 < 3 {
                    img[o] = 255;
                    img[o + 1] = 255;
                    img[o + 2] = 255;
                }
            }
        }
    }
    let file = File::create(png_out)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&img)?;
    println!("\nSpektrogram: {} (beyaz kesikli çizgiler 16/18/20/22 kHz)", png_out.display());
    Ok(())
}

fn heat(t: f64) -> (u8, u8, u8) {
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
