use crate::engine::Control;
use crate::ui::*;
use eframe::egui::{
    self, epaint::Vertex, pos2, vec2, Color32, ColorImage, Context, Mesh, Painter, Pos2, Rect, Rounding, Shape, Stroke,
    TextureHandle, TextureOptions, Ui,
};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::collections::VecDeque;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;

pub const FFT_N: usize = 4096;
const GRAM_W: usize = 420;
const GRAM_H: usize = 180;
const SCOPE_N: usize = 900;

#[derive(PartialEq, Clone, Copy)]
pub enum VizMode {
    Spectrum,
    Spectrogram,
    Scope,
}

pub struct VizState {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    pub spec: Vec<f32>,
    pub peaks: Vec<f32>,
    last_seq: u64,
    stale: bool,
    bins_db: Option<Vec<f32>>,
    rate: f32,
    scope: Vec<(f32, f32)>,
    pub corr: f32,
    gram: VecDeque<Vec<f32>>,
    gram_tex: Option<TextureHandle>,

    pub lvl: [f32; 2],
    pub hold: [f32; 2],
    hold_t: [f32; 2],
    pub clip_flash: f32,
    pub clips_total: u32,
}

impl VizState {
    pub fn new() -> VizState {
        let fft = FftPlanner::<f32>::new().plan_fft_forward(FFT_N);
        let window = (0..FFT_N)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / FFT_N as f32).cos())
            .collect();
        VizState {
            fft,
            window,
            spec: vec![],
            peaks: vec![],
            last_seq: 0,
            stale: true,
            bins_db: None,
            rate: 48000.0,
            scope: vec![],
            corr: 0.0,
            gram: VecDeque::new(),
            gram_tex: None,
            lvl: [-90.0; 2],
            hold: [-90.0; 2],
            hold_t: [0.0; 2],
            clip_flash: 0.0,
            clips_total: 0,
        }
    }

    pub fn feed(&mut self, ctl: Option<&Control>, dt: f32) {
        let mut pk = [0f32; 2];
        self.stale = true;
        if let Some(c) = ctl {
            self.rate = c.viz_rate.load(Relaxed) as f32;
            let seq = c.viz_seq.load(Relaxed);
            if seq != self.last_seq {
                self.last_seq = seq;
                let q = c.viz.lock().unwrap();
                if q.len() >= FFT_N * 2 {
                    let start = q.len() - FFT_N * 2;
                    let mut buf: Vec<Complex<f32>> = Vec::with_capacity(FFT_N);
                    let mut it = q.iter().skip(start);
                    let mut tail: Vec<(f32, f32)> = Vec::with_capacity(SCOPE_N);
                    for i in 0..FFT_N {
                        let l = *it.next().unwrap_or(&0.0);
                        let r = *it.next().unwrap_or(&0.0);
                        buf.push(Complex::new((l + r) * 0.5 * self.window[i], 0.0));
                        if i >= FFT_N - SCOPE_N {
                            tail.push((l, r));
                        }
                    }
                    drop(q);
                    self.scope = tail;
                    let (mut sl, mut sr, mut slr) = (0f32, 0f32, 0f32);
                    for (l, r) in &self.scope {
                        sl += l * l;
                        sr += r * r;
                        slr += l * r;
                    }
                    let d = (sl * sr).sqrt();
                    let corr = if d > 1e-9 { slr / d } else { 0.0 };
                    self.corr += (corr - self.corr) * 0.2;
                    self.fft.process(&mut buf);
                    let db: Vec<f32> = buf[..FFT_N / 2]
                        .iter()
                        .map(|c| 20.0 * (c.norm() / (FFT_N as f32 * 0.25)).max(1e-6).log10())
                        .collect();
                    self.push_gram(&db);
                    self.bins_db = Some(db);
                    self.stale = false;
                }
            }
            pk[0] = f32::from_bits(c.peak_l.swap(0, Relaxed));
            pk[1] = f32::from_bits(c.peak_r.swap(0, Relaxed));
            let clips = c.clips.swap(0, Relaxed);
            if clips > 0 {
                self.clip_flash = 1.0;
                self.clips_total += clips;
            }
        } else {
            self.bins_db = None;
        }
        for i in 0..2 {
            let db = if pk[i] > 1e-5 { 20.0 * pk[i].log10() } else { -90.0 };
            if db > self.lvl[i] {
                self.lvl[i] = db;
            } else {
                self.lvl[i] = (self.lvl[i] - 45.0 * dt).max(db).max(-90.0);
            }
            if db >= self.hold[i] {
                self.hold[i] = db;
                self.hold_t[i] = 1.3;
            } else {
                self.hold_t[i] -= dt;
                if self.hold_t[i] < 0.0 {
                    self.hold[i] = (self.hold[i] - 25.0 * dt).max(-90.0);
                }
            }
        }
        self.clip_flash = (self.clip_flash - dt * 0.4).max(0.0);
        if self.stale {

            if self.gram.len() >= GRAM_W {
                self.gram.pop_front();
            }
            self.gram.push_back(vec![-120.0; GRAM_H]);
            self.scope.clear();
        }
    }

    fn push_gram(&mut self, db: &[f32]) {
        let bin_hz = self.rate / FFT_N as f32;
        let col: Vec<f32> = (0..GRAM_H)
            .map(|r| {
                let f0 = 20.0 * 1000f32.powf(r as f32 / GRAM_H as f32);
                let f1 = 20.0 * 1000f32.powf((r + 1) as f32 / GRAM_H as f32);
                sample_db(db, bin_hz, f0, f1)
            })
            .collect();
        if self.gram.len() >= GRAM_W {
            self.gram.pop_front();
        }
        self.gram.push_back(col);
    }

    fn smooth(&mut self, cols: usize) {
        if self.spec.len() != cols {
            self.spec = vec![-120.0; cols];
            self.peaks = vec![-120.0; cols];
        }
        let bin_hz = self.rate / FFT_N as f32;
        for c in 0..cols {
            let target = match (&self.bins_db, self.stale) {
                (Some(b), false) => {
                    let f_lo = 20.0 * 1000f32.powf(c as f32 / cols as f32);
                    let f_hi = 20.0 * 1000f32.powf((c + 1) as f32 / cols as f32);
                    sample_db(b, bin_hz, f_lo, f_hi)
                }
                _ => -120.0,
            };
            let cur = self.spec[c];
            self.spec[c] = if target > cur { cur + (target - cur) * 0.55 } else { cur + (target - cur) * 0.09 };
            self.peaks[c] = (self.peaks[c] - 0.30).max(self.spec[c]);
        }
    }

    pub fn draw_spectrum(&mut self, painter: &Painter, rect: Rect, bars: bool) {
        self.draw_spectrum_with(painter, rect, bars, BLUE, SKY);
    }

    pub fn draw_spectrum_with(&mut self, painter: &Painter, rect: Rect, bars: bool, base: Color32, tip: Color32) {
        let cols = (rect.width() / 2.0).max(60.0) as usize;
        self.smooth(cols);
        let sy = |db: f32| rect.bottom() - ((db.clamp(-100.0, -12.0) + 100.0) / 88.0) * rect.height() * 0.94;
        let w = rect.width() / cols as f32;
        if bars {

            let mut f = 25.0f32;
            let mut centers = vec![];
            while f < 20000.0 {
                centers.push(f);
                f *= 2f32.powf(1.0 / 3.0);
            }
            let n = centers.len();
            let bw = rect.width() / n as f32;
            for (i, fc) in centers.iter().enumerate() {
                let c_lo = ((fc / 2f32.powf(1.0 / 6.0) / 20.0).log10() / 3.0 * cols as f32).max(0.0) as usize;
                let c_hi = (((fc * 2f32.powf(1.0 / 6.0) / 20.0).log10() / 3.0 * cols as f32).ceil() as usize).min(cols);
                if c_lo >= c_hi {
                    continue;
                }
                let v = self.spec[c_lo..c_hi].iter().cloned().fold(-120.0, f32::max);
                let pk = self.peaks[c_lo..c_hi].iter().cloned().fold(-120.0, f32::max);
                let top = sy(v);
                let x0 = rect.left() + i as f32 * bw + bw * 0.14;
                let r = Rect::from_min_max(pos2(x0, top), pos2(x0 + bw * 0.72, rect.bottom()));
                if r.height() > 1.0 {
                    let t = ((v + 100.0) / 88.0).clamp(0.0, 1.0);
                    let mut mesh = Mesh::default();
                    let top_c = lerp_color(base, tip, t);
                    let bot_c = alpha(base, 30);
                    mesh.colored_vertex(r.left_top(), top_c);
                    mesh.colored_vertex(r.right_top(), top_c);
                    mesh.colored_vertex(r.right_bottom(), bot_c);
                    mesh.colored_vertex(r.left_bottom(), bot_c);
                    mesh.add_triangle(0, 1, 2);
                    mesh.add_triangle(0, 2, 3);
                    painter.add(Shape::mesh(mesh));
                }
                let py = sy(pk);
                painter.rect_filled(
                    Rect::from_min_size(pos2(x0, py - 3.0), vec2(bw * 0.72, 2.0)),
                    Rounding::same(1.0),
                    alpha(Color32::WHITE, 170),
                );
            }
        } else {
            let mut mesh = Mesh::default();
            for c in 0..cols {
                let x = rect.left() + (c as f32 + 0.5) * w;
                let t = ((self.spec[c] + 100.0) / 88.0).clamp(0.0, 1.0);
                mesh.colored_vertex(pos2(x, sy(self.spec[c])), alpha(base, (60.0 + 70.0 * t) as u8));
                mesh.colored_vertex(pos2(x, rect.bottom()), alpha(base, 4));
            }
            for c in 0..cols.saturating_sub(1) {
                let i = (c * 2) as u32;
                mesh.add_triangle(i, i + 1, i + 2);
                mesh.add_triangle(i + 1, i + 3, i + 2);
            }
            painter.add(Shape::mesh(mesh));
            let top: Vec<Pos2> =
                (0..cols).map(|c| pos2(rect.left() + (c as f32 + 0.5) * w, sy(self.spec[c]))).collect();
            painter.add(Shape::line(top, Stroke::new(1.3, alpha(tip, 220))));
            let pk: Vec<Pos2> =
                (0..cols).map(|c| pos2(rect.left() + (c as f32 + 0.5) * w, sy(self.peaks[c]))).collect();
            painter.add(Shape::line(pk, Stroke::new(1.0, alpha(Color32::WHITE, 70))));
        }
    }

    pub fn draw_spectrogram(&mut self, ctx: &Context, painter: &Painter, rect: Rect) {
        let mut img = ColorImage::new([GRAM_W, GRAM_H], BG0);
        let n = self.gram.len();
        let off = GRAM_W - n;
        for (x, col) in self.gram.iter().enumerate() {
            for (r, db) in col.iter().enumerate() {
                let t = ((db + 100.0) / 70.0).clamp(0.0, 1.0);
                img.pixels[(GRAM_H - 1 - r) * GRAM_W + off + x] = heat(t);
            }
        }
        match &mut self.gram_tex {
            Some(t) => t.set(img, TextureOptions::LINEAR),
            None => self.gram_tex = Some(ctx.load_texture("gram", img, TextureOptions::LINEAR)),
        }
        if let Some(t) = &self.gram_tex {
            rounded_image(painter, t.id(), rect, 10.0, Color32::WHITE);
        }
    }

    pub fn draw_scope(&self, painter: &Painter, rect: Rect) {
        let c = pos2(rect.center().x, rect.center().y - 10.0);
        let r = (rect.width().min(rect.height() - 30.0)) * 0.5 * 0.92;
        for k in [0.33f32, 0.66, 1.0] {
            painter.circle_stroke(c, r * k, Stroke::new(1.0, alpha(STROKE, if k == 1.0 { 255 } else { 150 })));
        }
        let d = r * 0.7071;
        let guide = Stroke::new(1.0, alpha(STROKE, 200));
        painter.line_segment([c + vec2(-d, d), c + vec2(d, -d)], guide);
        painter.line_segment([c + vec2(-d, -d), c + vec2(d, d)], guide);
        painter.line_segment([c + vec2(0.0, -r), c + vec2(0.0, r)], guide);
        painter.line_segment([c + vec2(-r, 0.0), c + vec2(r, 0.0)], guide);
        for (t, p) in [("L", vec2(-d - 10.0, -d - 8.0)), ("R", vec2(d + 10.0, -d - 8.0)), ("M", vec2(0.0, -r - 10.0))] {
            painter.text_tr(c + p, egui::Align2::CENTER_CENTER, t, bold(12.0), DIM);
        }
        let max = self.scope.iter().fold(0.2f32, |m, (l, r)| m.max(l.abs()).max(r.abs()));
        let g = 0.9 / max.max(0.2);
        let n = self.scope.len();
        let mut mesh = Mesh::default();
        for (i, (l, rr)) in self.scope.iter().enumerate() {
            let x = (rr - l) * 0.7071 * g;
            let y = (l + rr) * 0.7071 * g;
            let p = c + vec2(x, -y) * r;
            let a = 30.0 + 150.0 * (i as f32 / n.max(1) as f32);
            let col = alpha(lerp_color(BLUE, CYAN, i as f32 / n.max(1) as f32), a as u8);
            let s = 1.6;
            let base = mesh.vertices.len() as u32;
            for (dx, dy) in [(-s, -s), (s, -s), (s, s), (-s, s)] {
                mesh.vertices.push(Vertex { pos: p + vec2(dx, dy), uv: egui::epaint::WHITE_UV, color: col });
            }
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base, base + 2, base + 3);
        }
        painter.add(Shape::mesh(mesh));

        let bar = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 14.0), vec2(r * 1.6, 8.0));
        painter.rect_filled(bar, Rounding::same(4.0), BG2);
        painter.line_segment([pos2(bar.center().x, bar.top() - 2.0), pos2(bar.center().x, bar.bottom() + 2.0)], Stroke::new(1.0, DIM));
        let x = bar.center().x + self.corr.clamp(-1.0, 1.0) * bar.width() * 0.5;
        let col = if self.corr > 0.3 { GREEN } else if self.corr > -0.1 { AMBER } else { RED };
        painter.circle_filled(pos2(x, bar.center().y), 6.0, col);
        painter.text_tr(pos2(bar.left() - 8.0, bar.center().y), egui::Align2::RIGHT_CENTER, "-1", body(11.0), DIM);
        painter.text_tr(pos2(bar.right() + 8.0, bar.center().y), egui::Align2::LEFT_CENTER, "+1", body(11.0), DIM);
        painter.text_tr(
            pos2(bar.center().x, bar.top() - 9.0),
            egui::Align2::CENTER_CENTER,
            format!("Faz korelasyonu {:+.2}", self.corr),
            body(11.0),
            DIM,
        );
    }

    pub fn draw_meters(&self, ui: &mut Ui, size: egui::Vec2) {
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let painter = ui.painter_at(rect);
        let top = rect.top() + 22.0;
        let bottom = rect.bottom() - 6.0;
        let bar_w = ((rect.width() - 56.0) / 2.0 - 8.0).clamp(14.0, 40.0);
        let segs = 34;
        let seg_h = (bottom - top) / segs as f32;
        let x0 = rect.left() + 34.0;
        for (ch, name) in ["L", "R"].iter().enumerate() {
            let x = x0 + ch as f32 * (bar_w + 10.0);
            painter.text_tr(pos2(x + bar_w * 0.5, rect.top() + 6.0), egui::Align2::CENTER_CENTER, *name, bold(11.5), DIM);
            for s in 0..segs {
                let db = -48.0 + 48.0 * (s as f32 + 0.5) / segs as f32;
                let y1 = bottom - s as f32 * seg_h;
                let r = Rect::from_min_max(pos2(x, y1 - seg_h + 1.5), pos2(x + bar_w, y1));
                let base = if db > -1.0 { RED } else if db > -6.0 { AMBER } else { lerp_color(BLUE, SKY, ((db + 48.0) / 42.0).clamp(0.0, 1.0)) };
                let lit = db <= self.lvl[ch];
                let is_hold = (db - self.hold[ch]).abs() < 48.0 / segs as f32 * 0.5 && self.hold[ch] > -48.0;
                let col = if is_hold { Color32::WHITE } else if lit { base } else { alpha(base, 24) };
                painter.rect_filled(r, Rounding::same(1.5), col);
            }
        }

        for db in [0, -6, -12, -24, -48] {
            let y = bottom - (db as f32 + 48.0) / 48.0 * (bottom - top);
            painter.text_tr(pos2(x0 - 6.0, y), egui::Align2::RIGHT_CENTER, format!("{db}"), body(10.0), DIM);
        }

        let led = pos2(rect.right() - 12.0, rect.top() + 6.0);
        if self.clip_flash > 0.0 {
            radial_glow(&painter, led, 12.0, alpha(RED, (180.0 * self.clip_flash) as u8));
        }
        painter.circle_filled(led, 4.0, lerp_color(alpha(RED, 60), RED, self.clip_flash));
        painter.text_tr(led + vec2(-8.0, 0.0), egui::Align2::RIGHT_CENTER, "CLIP", bold(9.5), lerp_color(DIM, RED, self.clip_flash));
    }
}

pub fn heat(t: f32) -> Color32 {
    let stops = [
        (0.0, Color32::from_rgb(5, 9, 20)),
        (0.18, Color32::from_rgb(14, 30, 90)),
        (0.45, Color32::from_rgb(40, 100, 230)),
        (0.72, Color32::from_rgb(70, 190, 255)),
        (1.0, Color32::from_rgb(235, 250, 255)),
    ];
    for w in stops.windows(2) {
        let (a, ca) = w[0];
        let (b, cb) = w[1];
        if t <= b {
            return lerp_color(ca, cb, (t - a) / (b - a));
        }
    }
    stops[4].1
}

pub fn draw_freq_grid(painter: &Painter, rect: Rect) {
    let fx = |f: f32| rect.left() + ((f / 20.0).log10() / 3.0) * rect.width();
    for f in [30.0, 50.0, 100.0, 200.0, 300.0, 500.0, 1000.0, 2000.0, 3000.0, 5000.0, 10000.0, 20000.0] {
        painter.line_segment([pos2(fx(f), rect.top()), pos2(fx(f), rect.bottom())], Stroke::new(1.0, alpha(STROKE, 120)));
        if [50.0, 100.0, 500.0, 1000.0, 5000.0, 10000.0, 20000.0].contains(&f) {
            let l = if f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{f}") };
            painter.text_tr(pos2(fx(f) + 4.0, rect.bottom() - 10.0), egui::Align2::LEFT_CENTER, l, body(10.5), DIM);
        }
    }
}

fn sample_db(b: &[f32], bin_hz: f32, f_lo: f32, f_hi: f32) -> f32 {
    let lo = f_lo / bin_hz;
    let hi = f_hi / bin_hz;
    let last = b.len() - 1;
    if hi - lo < 1.0 {
        let c = ((lo + hi) * 0.5).min(last as f32);
        let i = (c.floor() as usize).min(last);
        let j = (i + 1).min(last);
        let t = c - i as f32;
        b[i] + (b[j] - b[i]) * t
    } else {
        let l = (lo as usize).min(last);
        let h = (hi.ceil() as usize).max(l + 1).min(b.len());
        b[l..h].iter().cloned().fold(-120.0, f32::max)
    }
}
