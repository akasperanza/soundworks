//! Alttan çıkan ipucu/uyarı kartı ve Ayarlar > Uyarılar.

use super::*;
use crate::tips::{self, Facts, Tip, TipIcon};
use egui::Pos2;
use std::time::Duration;

fn tip_color(i: TipIcon) -> Color32 {
    match i {
        TipIcon::Warning => AMBER,
        TipIcon::Bluetooth | TipIcon::Info => SKY,
    }
}

/// Uyarı simgesi: yuvarlak zemin üstüne küçük çizim (ikon dosyası gerekmez).
fn tip_glyph(p: &egui::Painter, icon: TipIcon, c: Pos2, r: f32) {
    let col = tip_color(icon);
    p.circle_filled(c, r, alpha(col, 34));
    let st = Stroke::new(1.8, col);
    match icon {
        TipIcon::Bluetooth => {
            // Bluetooth rünü: 24'lük ızgarada 7,7 → 17,17 → 12,22 → 12,2 → 17,7 → 7,17
            let k = r * 1.05 / 12.0;
            let pts: Vec<Pos2> = [(7.0, 7.0), (17.0, 17.0), (12.0, 22.0), (12.0, 2.0), (17.0, 7.0), (7.0, 17.0)]
                .iter()
                .map(|(x, y)| pos2(c.x + (x - 12.0) * k * 0.9, c.y + (y - 12.0) * k * 0.9))
                .collect();
            p.add(Shape::line(pts, st));
        }
        TipIcon::Info => {
            p.circle_filled(pos2(c.x, c.y - r * 0.38), 1.8, col);
            p.line_segment([pos2(c.x, c.y - r * 0.05), pos2(c.x, c.y + r * 0.42)], st);
        }
        TipIcon::Warning => {
            p.line_segment([pos2(c.x, c.y - r * 0.42), pos2(c.x, c.y + r * 0.1)], st);
            p.circle_filled(pos2(c.x, c.y + r * 0.42), 1.8, col);
        }
    }
}

impl App {
    /// Anlık duruma göre uyarı kararlarının girdisi.
    fn tip_facts(&self) -> Facts {
        // hata ayıklama: SW_TIPS_DEMO=bt|clip|shared sahte durum üretir
        if let Ok(demo) = crate::devenv::dev_var("SW_TIPS_DEMO") {
            let mut f = Facts { running: true, exclusive: true, volume: 1.0, out_bits: 24, ..Default::default() };
            match demo.as_str() {
                "bt" => {
                    f.dev_kind = Some(crate::engine::DeviceKind::Bluetooth);
                    f.dev_name = "Kulaklıklar (MOMENTUM 4)".into();
                    f.out_bits = 16;
                    f.src_bits = 24;
                    f.resampled = true;
                    f.supported = vec![48000];
                }
                "clip" => f.clips = 500,
                _ => {
                    f.exclusive = false;
                    f.exclusive_pref_off = true;
                }
            }
            return f;
        }
        let out = self.running.as_ref().map(|r| r.ctl.out_info.lock().unwrap().clone());
        let live = out.as_ref().map(|o| o.rate > 0).unwrap_or(false);
        let o = out.unwrap_or_default();
        let (kind, name, id) = match &self.target_dev {
            Some((k, n, i)) => (Some(*k), n.clone(), i.clone()),
            None => (None, String::new(), String::new()),
        };
        Facts {
            running: live,
            loopback: self.cfg.use_loopback,
            exclusive: o.exclusive,
            exclusive_pref_off: self.cfg.exclusive_off,
            out_bits: o.bits,
            out_float: o.float,
            dev_kind: kind,
            dev_name: name,
            supported: self.rate_cache.get(&id).cloned().unwrap_or_default(),
            src_bits: self.meta.bits as u32,
            resampled: self.meta.resampled,
            volume: self.player.volume(),
            clips: self.viz.clips_total,
            has_limiter: self.cfg.chain.iter().any(|s| s.enabled && s.kind == crate::fx::FxKind::Limiter),
            rate_notice: self.rate_notice,
        }
    }

    /// Geçerli uyarıları hesapla; gösterilecek olanı seçer (kullanıcı kapatmadıysa, kararlı biçimde).
    fn tip_refresh(&mut self) {
        if self.tip_eval.elapsed() < Duration::from_millis(400) {
            return;
        }
        self.tip_eval = Instant::now();
        if !self.cfg.tips_on {
            self.tip_cands.clear();
            self.tip_cur = None;
            return;
        }
        let dismissed = &self.cfg.tips_dismissed;
        let seen = &self.tip_seen;
        let cands: Vec<Tip> = tips::evaluate(&self.tip_facts()).into_iter().filter(|t| !dismissed.iter().any(|d| d == t.id) && !seen.contains(t.id)).collect();
        // gösterilen uyarı hâlâ geçerliyse yerinde kalsın (sıra değişse bile titremesin)
        let keep = self.tip_cur.as_ref().and_then(|c| cands.iter().find(|t| t.id == c.id)).cloned();
        self.tip_cur = keep.or_else(|| cands.first().cloned());
        if self.tip_cur.is_some() && self.tip_cur_since.is_none() {
            self.tip_cur_since = Some(Instant::now());
        }
        if self.tip_cur.is_none() {
            self.tip_cur_since = None;
        }
        self.tip_cands = cands;
    }

    pub(super) fn tip_banner(&mut self, ctx: &egui::Context) {
        self.tip_refresh();
        // motor yeni başladıysa ve ilk saniyelerde değilse göster (açılışta üst üste binmesin)
        let ready = self.tip_cur_since.map(|t| t.elapsed() > Duration::from_millis(1800)).unwrap_or(false);
        let visible = self.tip_cur.is_some() && ready && self.cfg.tips_on;
        if let Some(t) = &self.tip_cur {
            self.tip_last = Some(t.clone());
        }
        let t = ctx.animate_bool_with_time(Id::new("tip_vis"), visible, 0.28);
        if !visible && self.tip_cur.is_some() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        let Some(tip) = self.tip_last.clone() else { return };
        if t <= 0.01 {
            return;
        }
        let e = ease(t);
        let screen = ctx.screen_rect();
        let avail = ctx.available_rect();
        let w = (avail.width() - 60.0).clamp(320.0, 660.0);
        let n = self.tip_cands.len();
        let (mut understood, mut hide_forever) = (false, false);
        egui::Area::new(Id::new("tip_banner"))
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_BOTTOM, vec2((avail.center().x - screen.center().x), -(screen.bottom() - avail.bottom()) - 14.0 + (1.0 - e) * 18.0))
            .show(ctx, |ui| {
                ui.set_opacity(e);
                Frame::none()
                    .fill(BG3)
                    .stroke(Stroke::new(1.0, alpha(tip_color(tip.icon), 90)))
                    .rounding(Rounding::same(16.0))
                    .inner_margin(Margin::same(18.0))
                    .shadow(egui::epaint::Shadow { offset: vec2(0.0, 10.0), blur: 28.0, spread: 0.0, color: Color32::from_black_alpha(110) })
                    .show(ui, |ui| {
                        ui.set_width(w - 36.0);
                        ui.horizontal_top(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
                            tip_glyph(ui.painter(), tip.icon, r.center(), 17.0);
                            ui.add_space(8.0);
                            ui.vertical(|ui| {
                                ui.set_max_width(w - 36.0 - 34.0 - 16.0);
                                ui.spacing_mut().item_spacing.y = 5.0;
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&tip.title).font(bold(14.5)).color(Color32::WHITE));
                                    if n > 1 {
                                        ui.label(RichText::new(format!("+{} uyarı daha", n - 1)).font(body(11.5)).color(DIM));
                                    }
                                });
                                ui.add(egui::Label::new(RichText::new(&tip.body).font(body(13.0)).color(TEXT)).wrap());
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    if pill_button(ui, "Anladım", None, true).clicked() {
                                        understood = true;
                                    }
                                    if pill_button(ui, "Bir daha gösterme", None, false).clicked() {
                                        hide_forever = true;
                                    }
                                });
                            });
                        });
                    });
            });
        if understood || hide_forever {
            if hide_forever && !self.cfg.tips_dismissed.iter().any(|d| d == tip.id) {
                self.cfg.tips_dismissed.push(tip.id.to_string());
                self.save_cfg();
            }
            self.tip_seen.insert(tip.id);
            self.tip_cur = None;
            self.tip_cur_since = None;
            self.tip_eval = Instant::now() - Duration::from_secs(1); // sıradakini hemen seç
        }
    }

    /// Ayarlar > Uyarılar kartı.
    pub(super) fn tips_settings(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            section_label(ui, "İpuçları ve uyarılar");
            ui.add_space(2.0);
            switch_row(
                ui,
                "Uyarıları göster",
                "Bluetooth, exclusive, bit derinliği, kırpılma gibi unutulabilecek durumlarda alttan kısa bir not çıkar.",
                &mut self.cfg.tips_on,
            );
            let n = self.cfg.tips_dismissed.len();
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(if n == 0 { "Gizlenen uyarı yok".to_string() } else { format!("{n} uyarı gizlendi") }).font(body(12.5)).color(DIM));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_enabled_ui(n > 0 || !self.tip_seen.is_empty(), |ui| {
                        if pill_button(ui, "Hepsini yeniden göster", Some(Icon::Undo), false).clicked() {
                            self.cfg.tips_dismissed.clear();
                            self.tip_seen.clear();
                            self.save_cfg();
                        }
                    });
                });
            });
        });
    }
}
