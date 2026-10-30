use super::*;
use crate::analyze::{self, Report, Verdict};
use super::track_menu::dim_backdrop;
use std::path::Path;

enum State {
    Busy(Receiver<Result<Report, String>>),
    Ready(Box<Report>, TextureHandle),
    Failed(String),
}

pub(super) struct SpectrumView {
    name: String,
    state: State,
}

fn dashed_h(p: &egui::Painter, y: f32, x0: f32, x1: f32, color: Color32) {
    let mut x = x0;
    while x < x1 {
        p.line_segment([pos2(x, y), pos2((x + 5.0).min(x1), y)], Stroke::new(1.0_f32, color));
        x += 9.0;
    }
}

impl App {
    pub(super) fn open_spectrum(&mut self, ctx: &egui::Context, path: &str) {
        let p = PathBuf::from(path);
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if !p.is_file() {
            self.say("Dosya bulunamadı");
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let c = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(analyze::compute(Path::new(&p)).map_err(|e| e.to_string()));
            c.request_repaint();
        });
        self.spectrum = Some(SpectrumView { name, state: State::Busy(rx) });
    }

    pub(super) fn spectrum_ui(&mut self, ctx: &egui::Context) {
        let Some(view) = self.spectrum.as_mut() else { return };
        if let State::Busy(rx) = &view.state {
            if let Ok(res) = rx.try_recv() {
                view.state = match res {
                    Ok(r) => {
                        let img = ColorImage::from_rgb([analyze::COLS, analyze::ROWS], &r.rgb);
                        let tex = ctx.load_texture("spectrum_analysis", img, TextureOptions::LINEAR);
                        State::Ready(Box::new(r), tex)
                    }
                    Err(e) => State::Failed(e),
                };
            }
        }
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        dim_backdrop(ctx, "spectrum_dim");
        let view = self.spectrum.as_ref().unwrap();
        egui::Window::new(crate::i18n::tr("Spektrum analizi"))
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Tooltip)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .fixed_size(vec2(900.0, 0.0))
            .show(ctx, |ui| {
                ui.label(RichText::new(&view.name).font(bold(15.0)).color(Color32::WHITE));
                ui.add_space(4.0);
                match &view.state {
                    State::Busy(_) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new(crate::i18n::tr("Analiz ediliyor…")).font(body(13.0)).color(DIM));
                        });
                        ctx.request_repaint();
                    }
                    State::Failed(e) => {
                        ui.label(RichText::new(crate::i18n::tr("Dosya analiz edilemedi")).font(body(13.0)).color(RED));
                        ui.label(RichText::new(e).font(body(12.0)).color(DIM));
                    }
                    State::Ready(r, tex) => report_ui(ui, r, tex),
                }
                ui.add_space(10.0);
                if pill_button(ui, "Kapat", None, false).clicked() {
                    close = true;
                }
            });
        if close {
            self.spectrum = None;
        }
    }
}

fn report_ui(ui: &mut egui::Ui, r: &Report, tex: &TextureHandle) {
    let bits = r.bits.map(|b| format!("{b} bit")).unwrap_or_else(|| "? bit".to_string());
    let info = format!("{} Hz · {} · {} {} · {}", r.rate, bits, r.chans, crate::i18n::tr("kanal"), fmt_time(r.secs));
    ui.label(RichText::new(info).font(body(12.5)).color(DIM));
    ui.add_space(8.0);

    let khz = r.cutoff_hz.map(|f| format!("{:.1}", f / 1000.0)).unwrap_or_default();
    let (color, text) = match r.verdict {
        Verdict::Full => (GREEN, "Tam bant: içerik üst frekanslara kadar uzanıyor, gerçek kayıpsız gibi görünüyor.".to_string()),
        Verdict::Upsampled => (AMBER, format!("Yükseltilmiş olabilir: {khz} kHz üstünde içerik yok, düşük örnekleme hızından büyütülmüş gibi.")),
        Verdict::LossySuspect => (RED, format!("Şüpheli: {khz} kHz'de keskin kesim var. MP3/AAC gibi kayıplı bir kaynaktan dönüştürülmüş olabilir.")),
        Verdict::LossyFormat if r.cutoff_hz.is_some() => (DIM, format!("Kayıplı biçim: {khz} kHz üstünün kesilmesi normaldir.")),
        Verdict::LossyFormat => (DIM, "Kayıplı biçim.".to_string()),
        Verdict::Silent => (DIM, "Analiz için ses çok sessiz.".to_string()),
    };
    Frame::none().fill(alpha(color, 36)).stroke(Stroke::new(1.0_f32, alpha(color, 120))).rounding(Rounding::same(10.0)).inner_margin(Margin::symmetric(12.0, 8.0)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.add(egui::Label::new(RichText::new(crate::i18n::tr(text)).font(bold(13.0)).color(Color32::WHITE)).wrap());
    });
    ui.add_space(10.0);

    let (left, bottom, right) = (46.0, 22.0, 54.0);
    let size = vec2(ui.available_width(), 340.0 + bottom);
    let (outer, _) = ui.allocate_exact_size(size, Sense::hover());
    let plot = Rect::from_min_max(outer.min + vec2(left, 0.0), outer.max - vec2(right, bottom));
    let p = ui.painter();
    p.image(tex.id(), plot, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    p.rect_stroke(plot, 0.0, Stroke::new(1.0_f32, alpha(Color32::WHITE, 40)));

    let nyq = r.nyquist();
    let step = if nyq > 30_000.0 { 4_000.0 } else { 2_000.0 };
    let y_of = |f: f64| plot.bottom() - (f / nyq) as f32 * plot.height();
    let mut f = 0.0;
    while f <= nyq + 1.0 {
        let y = y_of(f);
        p.text(pos2(plot.left() - 6.0, y), Align2::RIGHT_CENTER, format!("{:.0}k", f / 1000.0), body(11.0), DIM);
        if f > 0.0 && f < nyq - 500.0 {
            dashed_h(p, y, plot.left(), plot.right(), alpha(Color32::WHITE, 46));
        }
        f += step;
    }
    for i in 0..=5 {
        let x = plot.left() + plot.width() * i as f32 / 5.0;
        p.text(pos2(x, plot.bottom() + 4.0), if i == 0 { Align2::LEFT_TOP } else if i == 5 { Align2::RIGHT_TOP } else { Align2::CENTER_TOP }, fmt_time(r.secs * i as f64 / 5.0), body(11.0), DIM);
    }
    if let Some(c) = r.cutoff_hz {
        let y = y_of(c);
        dashed_h(p, y, plot.left(), plot.right(), Color32::WHITE);
        p.text(pos2(plot.right() - 6.0, y - 3.0), Align2::RIGHT_BOTTOM, format!("{:.1} kHz", c / 1000.0), bold(12.0), Color32::WHITE);
    }

    let bar = Rect::from_min_max(pos2(plot.right() + 14.0, plot.top()), pos2(plot.right() + 24.0, plot.bottom()));
    let n = 48;
    for i in 0..n {
        let t = 1.0 - (i as f64 + 0.5) / n as f64;
        let (cr, cg, cb) = analyze::heat(t);
        let y0 = bar.top() + bar.height() * i as f32 / n as f32;
        p.rect_filled(Rect::from_min_max(pos2(bar.left(), y0), pos2(bar.right(), y0 + bar.height() / n as f32 + 0.5)), 0.0, Color32::from_rgb(cr, cg, cb));
    }
    p.text(pos2(bar.right() + 4.0, bar.top()), Align2::LEFT_TOP, "-10", body(10.5), DIM);
    p.text(pos2(bar.right() + 4.0, bar.bottom()), Align2::LEFT_BOTTOM, "-130", body(10.5), DIM);
    p.text(pos2(bar.right() + 4.0, bar.center().y), Align2::LEFT_CENTER, "dB", body(10.5), DIM);

    ui.add_space(6.0);
    ui.add(egui::Label::new(RichText::new(crate::i18n::tr("Kesin kanıt değildir: eski kayıtlar ve bazı masterlar da yüksek frekansları doğal olarak keser.")).font(body(11.5)).color(DIM)).wrap());
}
