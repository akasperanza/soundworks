//! AutoEQ mağazası: kulaklığa özel EQ profili ara, eğrisini gör, dinleyerek dene, kaydet.

use super::*;
use crate::autoeq::{self, Entry, IndexState, Profile};
use egui::{FontId, Pos2, Vec2};
use std::time::Duration;

/// Mağaza penceresinin durumu.
pub(super) struct AeUi {
    pub open: bool,
    pub query: String,
    results: Vec<usize>,
    shown_q: String,
    shown_n: usize,
    sel: Option<usize>,
    /// seçili profil çalan sese (kaydedilmeden) uygulanıyor
    pub listening: bool,
    /// dinlemede uygulanan profilin yolu
    applied: Option<String>,
    /// dinlemeden önceki EQ (geri dönmek için): (ön ayar, ad, değişmiş mi)
    backup: Option<(Preset, Option<String>, bool)>,
    focus: bool,
    t0: Instant,
    q_at: Instant,
    curve_for: String,
    curve: Vec<f64>,
}

impl AeUi {
    pub fn new() -> AeUi {
        AeUi {
            open: false,
            query: String::new(),
            results: vec![],
            shown_q: String::from("\u{0}"),
            shown_n: usize::MAX,
            sel: None,
            listening: false,
            applied: None,
            backup: None,
            focus: false,
            t0: Instant::now(),
            q_at: Instant::now(),
            curve_for: String::new(),
            curve: vec![],
        }
    }
}

enum Act {
    Device,
    Retry,
    Refresh,
    Listen(bool),
    EqOn,
    Save,
    Close,
}

/// Bir profilin frekans yanıtı (dB), 20 Hz–20 kHz arası logaritmik 200 nokta (48 kHz'de tasarlanır).
fn response_curve(p: &Preset) -> Vec<f64> {
    let fs = 48000.0;
    let coeffs: Vec<Coeffs> = p.bands.iter().filter(|b| b.freq < fs * 0.5).map(|b| Coeffs::design(b, fs)).collect();
    (0..200)
        .map(|i| {
            let f = 20.0 * 1000f64.powf(i as f64 / 199.0);
            coeffs.iter().map(|c| c.magnitude_db(f, fs)).sum::<f64>()
        })
        .collect()
}

/// Eğriyi ızgarayla çizer: 0 dB çizgisi, dB ve frekans işaretleri.
fn draw_curve(ui: &mut egui::Ui, size: Vec2, curve: &[f64]) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, Rounding::same(8.0), BG0);
    let peak = curve.iter().fold(0f64, |m, v| m.max(v.abs()));
    let range = ((peak / 6.0).ceil() * 6.0).max(12.0) as f32;
    let inner = rect.shrink2(vec2(34.0, 14.0));
    let y_of = |db: f32| inner.center().y - db / range * inner.height() * 0.5;
    for k in [-1.0f32, -0.5, 0.0, 0.5, 1.0] {
        let db = k * range;
        let y = y_of(db);
        let strong = k == 0.0;
        p.line_segment([pos2(inner.left(), y), pos2(inner.right(), y)], Stroke::new(1.0, alpha(Color32::WHITE, if strong { 36 } else { 12 })));
        p.text(pos2(rect.left() + 6.0, y), Align2::LEFT_CENTER, format!("{db:+.0}"), body(10.0), alpha(DIM, 200));
    }
    for (f, label) in [(100.0f32, "100"), (1000.0, "1k"), (10000.0, "10k")] {
        let x = inner.left() + (f / 20.0).log10() / 3.0 * inner.width();
        p.line_segment([pos2(x, inner.top()), pos2(x, inner.bottom())], Stroke::new(1.0, alpha(Color32::WHITE, 12)));
        p.text(pos2(x, rect.bottom() - 2.0), Align2::CENTER_BOTTOM, label, body(10.0), alpha(DIM, 200));
    }
    if curve.len() > 1 {
        let n = curve.len();
        let pts: Vec<Pos2> = curve
            .iter()
            .enumerate()
            .map(|(i, db)| pos2(inner.left() + i as f32 / (n - 1) as f32 * inner.width(), y_of((*db as f32).clamp(-range, range))))
            .collect();
        p.add(Shape::line(pts.clone(), Stroke::new(6.0, alpha(BLUE, 40))));
        p.add(Shape::line(pts, Stroke::new(2.0, SKY)));
    }
}

fn chip_text(e: &Entry) -> String {
    let mut s = format!("{} · {}", e.source, e.form);
    if !e.rig.is_empty() {
        s.push_str(&format!(" · {}", e.rig));
    }
    s
}

impl App {
    /// Çıkış cihazının adından kulaklık modeli ("Kulaklıklar (2- MOMENTUM 4)" -> "MOMENTUM 4").
    pub(super) fn ae_device_query(&self) -> String {
        let dev = match &self.cfg.to {
            Some(id) => self.devices.iter().find(|d| &d.id == id),
            None => self.devices.iter().find(|d| d.is_default),
        };
        dev.map(|d| autoeq::device_query(&d.name)).unwrap_or_default()
    }

    pub(super) fn ae_open(&mut self, from_device: bool) {
        self.ae.open = true;
        self.ae.focus = true;
        self.ae.t0 = Instant::now();
        if from_device {
            let q = self.ae_device_query();
            if q.is_empty() {
                self.say("Cihaz adından kulaklık modeli çıkarılamadı; adı elle yaz");
            } else {
                self.ae.query = q;
                self.ae.sel = None;
            }
        }
        self.autoeq.ensure_index();
    }

    /// Mağazayı kapatır; dinleme açıksa ve kaydedilmediyse önceki EQ'ya döner.
    pub(super) fn ae_close(&mut self, keep_applied: bool) {
        if !keep_applied {
            self.ae_revert_preview();
        } else {
            self.ae.backup = None;
        }
        self.ae.listening = false;
        self.ae.applied = None;
        self.ae.open = false;
    }

    fn ae_apply_preview(&mut self, p: Preset) {
        if self.ae.backup.is_none() {
            self.ae.backup = Some((self.preset.clone(), self.preset_name.clone(), self.dirty));
        }
        self.preset = p;
        self.preset_name = None;
        self.dirty = true;
        self.sel_band = None;
        self.push_preset();
    }

    fn ae_revert_preview(&mut self) {
        if let Some((p, n, d)) = self.ae.backup.take() {
            self.preset = p;
            self.preset_name = n;
            self.dirty = d;
            self.push_preset();
        }
        self.ae.applied = None;
    }

    pub(super) fn autoeq_overlay(&mut self, ctx: &egui::Context) {
        if !self.ae.open {
            return;
        }
        self.autoeq.ensure_index();
        if self.autoeq.poll() {
            ctx.request_repaint();
        }
        if self.autoeq.busy() {
            ctx.request_repaint_after(Duration::from_millis(120));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.ae_close(false);
            return;
        }

        // sonuçları sorgu ya da indeks değişince yeniden hesapla
        if self.ae.shown_q != self.ae.query || self.ae.shown_n != self.autoeq.entries.len() {
            self.ae.results = autoeq::search(&self.autoeq.entries, &self.ae.query, 60);
            self.ae.shown_q = self.ae.query.clone();
            self.ae.shown_n = self.autoeq.entries.len();
            self.ae.q_at = Instant::now();
            if !self.ae.sel.map_or(false, |s| self.ae.results.contains(&s)) {
                self.ae.sel = None;
            }
        }
        // sorgu durulduktan sonra ilk sonucu seç (her tuşta indirme başlamasın)
        if self.ae.sel.is_none() && !self.ae.results.is_empty() {
            if self.ae.q_at.elapsed() > Duration::from_millis(450) {
                self.ae.sel = self.ae.results.first().copied();
            } else {
                ctx.request_repaint_after(Duration::from_millis(120));
            }
        }
        // ok tuşlarıyla gezinme
        let n = self.ae.results.len();
        if n > 0 {
            let cur = self.ae.sel.and_then(|s| self.ae.results.iter().position(|r| *r == s));
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                self.ae.sel = Some(self.ae.results[cur.map_or(0, |c| (c + 1).min(n - 1))]);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                self.ae.sel = Some(self.ae.results[cur.map_or(0, |c| c.saturating_sub(1))]);
            }
        }

        let sel_entry: Option<Entry> = self.ae.sel.map(|i| self.autoeq.entries[i].clone());
        let profile: Option<Profile> = sel_entry.as_ref().map(|e| self.autoeq.profile(e).clone());
        if let (Some(e), Some(Profile::Ready(p))) = (&sel_entry, &profile) {
            if self.ae.curve_for != e.path {
                self.ae.curve = response_curve(p);
                self.ae.curve_for = e.path.clone();
            }
        }
        let rows: Vec<(usize, Entry)> = self.ae.results.iter().map(|&i| (i, self.autoeq.entries[i].clone())).collect();
        let state = self.autoeq.state.clone();
        let refreshing = self.autoeq.is_refreshing();
        let total = self.autoeq.entries.len();
        let running = self.running.is_some();
        let eq_off = engine::EQ_OFF.load(Relaxed);
        let listening = self.ae.listening;
        let screen = ctx.screen_rect();
        let w = 920.0f32.min(screen.width() - 60.0);
        let h = 640.0f32.min(screen.height() - 90.0).max(380.0);
        let e = ease(self.ae.t0.elapsed().as_secs_f32() / 0.18);
        if e < 1.0 {
            ctx.request_repaint();
        }
        let panel_rect = Rect::from_min_size(pos2(screen.center().x - w * 0.5, (screen.height() - h) * 0.5 - (1.0 - e) * 14.0), vec2(w, h));
        let mut act: Option<Act> = None;
        let mut clicked: Option<usize> = None;

        egui::Area::new(Id::new("autoeq_overlay")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            ui.set_opacity(e);
            let bd = ui.allocate_rect(screen, Sense::click());
            ui.painter().rect_filled(screen, 0.0, Color32::from_rgba_unmultiplied(78, 82, 92, 214));
            if bd.clicked() && ctx.pointer_interact_pos().map_or(true, |p| !panel_rect.contains(p)) {
                act = Some(Act::Close);
            }
            let mut child = ui.child_ui(panel_rect, egui::Layout::top_down(egui::Align::Min), None);
            let ui = &mut child;
            Frame::none()
                .fill(BG1)
                .stroke(Stroke::new(1.0, STROKE))
                .rounding(Rounding::same(14.0))
                .inner_margin(Margin::same(20.0))
                .shadow(egui::epaint::Shadow { offset: vec2(0.0, 14.0), blur: 34.0, spread: 0.0, color: Color32::from_black_alpha(130) })
                .show(ui, |ui| {
                    ui.set_width(w - 40.0);
                    ui.set_height(h - 40.0);

                    // ---- başlık
                    ui.horizontal(|ui| {
                        let (ir, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
                        draw_icon(ui.painter(), Icon::Headphones, ir.center(), 20.0, SKY);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.label(RichText::new("AutoEQ").font(bold(18.0)).color(Color32::WHITE));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icon_button(ui, Icon::Close, 30.0, false).on_hover_text("Kapat (Esc)").clicked() {
                                act = Some(Act::Close);
                            }
                        });
                    });
                    ui.add_space(14.0);

                    // ---- arama satırı
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let field_w = ui.available_width() - 44.0;
                        Frame::none().fill(BG2).rounding(Rounding::same(10.0)).inner_margin(Margin::symmetric(12.0, 7.0)).show(ui, |ui| {
                            ui.set_width(field_w - 24.0);
                            ui.horizontal(|ui| {
                                let (ir, _) = ui.allocate_exact_size(vec2(18.0, 20.0), Sense::hover());
                                draw_icon(ui.painter(), Icon::Search, ir.center(), 15.0, DIM);
                                let te = ui.add(
                                    egui::TextEdit::singleline(&mut self.ae.query)
                                        .hint_text("Kulaklık adı (ör. MOMENTUM 4, HD 600, WH-1000XM5)")
                                        .font(body(15.0))
                                        .frame(false)
                                        .desired_width((field_w - 24.0 - 60.0).max(80.0)),
                                );
                                if self.ae.focus {
                                    te.request_focus();
                                    self.ae.focus = false;
                                }
                                if !self.ae.query.is_empty() {
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if icon_button(ui, Icon::Close, 22.0, false).on_hover_text("Temizle").clicked() {
                                            self.ae.query.clear();
                                            self.ae.focus = true;
                                        }
                                    });
                                }
                            });
                        });
                        let dq = self.ae_device_query();
                        let tip = if dq.is_empty() { "Çıkış cihazının adıyla ara".to_string() } else { format!("Çıkış cihazımın adını yaz: {dq}") };
                        if icon_button(ui, Icon::Headphones, 36.0, false).on_hover_text(tip).clicked() {
                            act = Some(Act::Device);
                        }
                    });
                    ui.add_space(12.0);

                    // ---- gövde: sonuçlar | önizleme
                    let body_h = ui.available_height() - 34.0;
                    ui.horizontal_top(|ui| {
                        let list_w = ((w - 40.0) * 0.46).floor();
                        let prev_w = w - 40.0 - list_w - 16.0;
                        ui.allocate_ui_with_layout(vec2(list_w, body_h), egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_width(list_w);
                            match &state {
                                IndexState::Idle | IndexState::Loading => {
                                    ui.add_space(60.0);
                                    ui.vertical_centered(|ui| {
                                        let dots = ".".repeat(1 + (self.ae.t0.elapsed().as_secs_f32() * 2.5) as usize % 3);
                                        ui.label(RichText::new(format!("Profil listesi indiriliyor{dots}")).font(bold(14.0)).color(TEXT));
                                    });
                                }
                                IndexState::Failed(msg) => {
                                    ui.add_space(50.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(RichText::new("Profil listesi alınamadı").font(bold(14.0)).color(AMBER));
                                        ui.label(RichText::new(msg).font(body(12.0)).color(DIM));
                                        ui.add_space(8.0);
                                        if pill_button(ui, "Yeniden dene", Some(Icon::Undo), true).clicked() {
                                            act = Some(Act::Retry);
                                        }
                                    });
                                }
                                IndexState::Ready { .. } => {
                                    if self.ae.query.trim().is_empty() {
                                        // boş: yazmaya davet eden metin yok, alan kendiliğinden anlaşılır
                                    } else if rows.is_empty() {
                                        ui.add_space(50.0);
                                        ui.vertical_centered(|ui| {
                                            ui.label(RichText::new("Sonuç bulunamadı").font(bold(14.0)).color(DIM));
                                        });
                                    } else {
                                        ui.label(RichText::new(format!("{} sonuç", rows.len())).font(body(12.0)).color(DIM));
                                        ui.add_space(4.0);
                                        egui::ScrollArea::vertical().id_salt("ae_list").auto_shrink([false, false]).show(ui, |ui| {
                                            ui.spacing_mut().item_spacing.y = 3.0;
                                            for (idx, en) in &rows {
                                                let selected = self.ae.sel == Some(*idx);
                                                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width() - 8.0, 52.0), Sense::click());
                                                let hv = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1);
                                                let bg = if selected { alpha(BLUE, 44) } else { alpha(BG3, (200.0 * hv) as u8) };
                                                ui.painter().rect_filled(rect, Rounding::same(9.0), bg);
                                                if selected {
                                                    ui.painter().rect_filled(Rect::from_min_size(rect.min + vec2(0.0, 8.0), vec2(3.0, rect.height() - 16.0)), Rounding::same(2.0), BLUE);
                                                }
                                                let name = fit_text(ui.painter(), &en.name, bold(13.5), rect.width() - 28.0);
                                                ui.painter().text(pos2(rect.left() + 14.0, rect.top() + 17.0), Align2::LEFT_CENTER, name, bold(13.5), Color32::WHITE);
                                                let sub = fit_text(ui.painter(), &chip_text(en), body(11.5), rect.width() - 28.0);
                                                ui.painter().text(pos2(rect.left() + 14.0, rect.top() + 36.0), Align2::LEFT_CENTER, sub, body(11.5), DIM);
                                                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                                    clicked = Some(*idx);
                                                }
                                            }
                                        });
                                    }
                                }
                            }
                        });
                        ui.add_space(16.0);
                        ui.allocate_ui_with_layout(vec2(prev_w, body_h), egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_width(prev_w);
                            Frame::none().fill(BG2).rounding(Rounding::same(12.0)).inner_margin(Margin::same(16.0)).show(ui, |ui| {
                                ui.set_width(prev_w - 32.0);
                                ui.set_min_height(body_h - 32.0);
                                // içerik pencereye sığmazsa kart içinde kayar (pencere taşmasın)
                                egui::ScrollArea::vertical().id_salt("ae_prev").max_height(body_h - 32.0).auto_shrink([false, false]).show(ui, |ui| {
                                let (Some(en), Some(pr)) = (&sel_entry, &profile) else {
                                    return;
                                };
                                ui.add(egui::Label::new(RichText::new(&en.name).font(bold(16.0)).color(Color32::WHITE)).wrap());
                                ui.label(RichText::new(chip_text(en)).font(body(12.0)).color(DIM));
                                ui.add_space(10.0);
                                match pr {
                                    Profile::Loading => {
                                        ui.add_space(40.0);
                                        ui.vertical_centered(|ui| {
                                            ui.label(RichText::new("Profil indiriliyor…").font(body(13.0)).color(DIM));
                                        });
                                    }
                                    Profile::Failed(msg) => {
                                        ui.add_space(24.0);
                                        ui.label(RichText::new("Profil indirilemedi").font(bold(13.5)).color(AMBER));
                                        ui.label(RichText::new(msg).font(body(12.0)).color(DIM));
                                        ui.add_space(8.0);
                                        if pill_button(ui, "Yeniden dene", Some(Icon::Undo), false).clicked() {
                                            act = Some(Act::Refresh);
                                        }
                                    }
                                    Profile::Ready(p) => {
                                        draw_curve(ui, vec2(prev_w - 48.0, 130.0), &self.ae.curve);
                                        ui.add_space(8.0);
                                        let maxg = p.bands.iter().map(|b| b.gain_db).fold(f64::MIN, f64::max);
                                        let ming = p.bands.iter().map(|b| b.gain_db).fold(f64::MAX, f64::min);
                                        ui.label(
                                            RichText::new(format!("{} bant · ön kazanç {:+.1} dB · en fazla {:+.1} / {:+.1} dB", p.bands.len(), p.preamp_db, maxg, ming))
                                                .font(body(12.0))
                                                .color(TEXT),
                                        );
                                        ui.add_space(14.0);
                                        // dinleyerek dene
                                        if listening {
                                            ui.horizontal(|ui| {
                                                chip(ui, "ŞU AN DİNLİYORSUN", GREEN);
                                                ui.label(RichText::new("kaydedilmedi").font(body(12.0)).color(DIM));
                                            });
                                            ui.add_space(6.0);
                                            if eq_off {
                                                ui.label(RichText::new("Ekolayzır kapalı olduğu için farkı duymazsın.").font(body(12.0)).color(AMBER));
                                                if pill_button(ui, "Ekolayzırı aç", Some(Icon::Power), false).clicked() {
                                                    act = Some(Act::EqOn);
                                                }
                                                ui.add_space(6.0);
                                            }
                                            if pill_button(ui, "Dinlemeyi bırak", Some(Icon::Close), false).clicked() {
                                                act = Some(Act::Listen(false));
                                            }
                                        } else if running {
                                            if pill_button(ui, "Dinleyerek dene", Some(Icon::Headphones), false).on_hover_text("Profili çalan müziğe geçici uygular; kaydetmeden geri alabilirsin").clicked() {
                                                act = Some(Act::Listen(true));
                                            }
                                        } else {
                                            ui.label(RichText::new("Motoru başlatınca dinleyerek deneyebilirsin.").font(body(12.0)).color(DIM));
                                        }
                                        ui.add_space(10.0);
                                        if pill_button(ui, "Kaydet ve kullan", Some(Icon::Save), true).clicked() {
                                            act = Some(Act::Save);
                                        }
                                    }
                                }
                                });
                            });
                        });
                    });

                    // ---- alt bilgi
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let status = match &state {
                            IndexState::Ready { cached } => {
                                let src = if *cached { "yerel kopya" } else { "güncel" };
                                format!("{} profil · {}{}", crate::proof::group(total as u64), src, if refreshing { " · yenileniyor…" } else { "" })
                            }
                            _ => String::new(),
                        };
                        ui.label(RichText::new(format!("AutoEq (MIT)  ·  {status}")).font(body(11.0)).color(alpha(DIM, 200)));
                        if matches!(state, IndexState::Ready { .. }) && !refreshing {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add(egui::Label::new(RichText::new("Listeyi yenile").font(body(11.0)).color(SKY)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                    act = Some(Act::Retry);
                                }
                            });
                        }
                    });
                });
        });

        if let Some(i) = clicked {
            self.ae.sel = Some(i);
        }
        // dinleme açıksa seçili profili uygula (seçim değişince yenisine geç)
        if self.ae.listening {
            if let (Some(e), Some(Profile::Ready(p))) = (&sel_entry, &profile) {
                if self.ae.applied.as_deref() != Some(e.path.as_str()) {
                    self.ae_apply_preview(p.clone());
                    self.ae.applied = Some(e.path.clone());
                }
            }
        }
        match act {
            Some(Act::Close) => self.ae_close(false),
            Some(Act::Device) => {
                let q = self.ae_device_query();
                if q.is_empty() {
                    self.say("Cihaz adından kulaklık modeli çıkarılamadı; adı elle yaz");
                } else {
                    self.ae.query = q;
                    self.ae.sel = None;
                }
            }
            Some(Act::Retry) => self.autoeq.retry(),
            Some(Act::Refresh) => {
                if let Some(e) = &sel_entry {
                    self.autoeq.forget_profile(e);
                }
            }
            Some(Act::Listen(on)) => {
                self.ae.listening = on;
                if !on {
                    self.ae_revert_preview();
                }
            }
            Some(Act::EqOn) => engine::EQ_OFF.store(false, Relaxed),
            Some(Act::Save) => {
                if let (Some(e), Some(Profile::Ready(p))) = (&sel_entry, &profile) {
                    let _ = std::fs::create_dir_all(presets_dir());
                    let name = e.file_name();
                    if std::fs::write(presets_dir().join(&name), apo::to_text(p)).is_ok() {
                        // geçici dinleme zaten uygulandıysa geri dönmeden kalıcı seçilir
                        self.ae_close(true);
                        self.refresh_library();
                        self.select(&name, false);
                        self.say(format!("AutoEQ uygulandı: {}", e.name));
                    } else {
                        self.say("Profil kaydedilemedi");
                    }
                }
            }
            None => {}
        }
    }
}

/// Metni verilen genişliğe sığacak şekilde sonuna "…" koyarak kısaltır.
fn fit_text(painter: &egui::Painter, s: &str, font: FontId, max_w: f32) -> String {
    let w = |t: &str| painter.layout_no_wrap(t.to_string(), font.clone(), Color32::WHITE).size().x;
    if w(s) <= max_w {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        let t: String = chars[..mid].iter().collect::<String>() + "…";
        if w(&t) <= max_w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    chars[..lo].iter().collect::<String>() + "…"
}
