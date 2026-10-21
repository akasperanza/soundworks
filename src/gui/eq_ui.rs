use super::*;

#[derive(Default)]
struct PresetActs {
    pick: Option<String>,
    props: Option<String>,
    copy: Option<String>,
    del: Option<String>,
}

impl App {
    pub(super) fn refresh_library(&mut self) {
        let mut v: Vec<String> = std::fs::read_dir(presets_dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.to_lowercase().ends_with(".txt"))
            .collect();
        v.sort_by_key(|n| n.to_lowercase());
        self.library = v
            .into_iter()
            .filter_map(|n| apo::load(&presets_dir().join(&n)).ok().map(|p| (n, p)))
            .collect();
    }

    pub(super) fn push_preset(&self) {
        if let Some(r) = &self.running {
            r.ctl.set_preset(self.preset.clone());
        }
    }

    fn create_button(&mut self, ui: &mut egui::Ui) {
        let btn = pill_button(ui, "Oluştur", Some(Icon::Plus), true);
        let id = btn.id.with("create_menu");
        if btn.clicked() {
            ui.memory_mut(|m| m.toggle_popup(id));
        }
        let mut pick = 0u8;
        egui::popup_below_widget(ui, id, &btn, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
            ui.set_min_width(260.0);
            if super::eqrules_ui::menu_row(ui, "AutoEQ ile seç", "Kulaklığına hazır profil", true).clicked() {
                pick = 1;
            }
            if super::eqrules_ui::menu_row(ui, "Sıfırdan oluştur", "Boş bir ön ayar", true).clicked() {
                pick = 2;
            }
        });
        if pick != 0 {
            ui.memory_mut(|m| m.close_popup());
        }
        match pick {
            1 => self.ae_open(true),
            2 => self.props_open(None),
            _ => {}
        }
    }

    pub(super) fn select(&mut self, name: &str, announce: bool) {
        match apo::load(&presets_dir().join(name)) {
            Ok(p) => {
                self.preset = p;
                self.preset_name = Some(name.to_string());
                self.cfg.preset = Some(name.to_string());
                self.dirty = false;
                self.sel_band = None;
                self.push_preset();
                self.save_cfg();
                self.message.clear();
                if announce {
                    self.say(format!("Preset uygulandı: {}", name.trim_end_matches(".txt")));
                }
            }
            Err(e) => self.message = format!("Preset okunamadı: {e}"),
        }
    }

    pub(super) fn import(&mut self) {
        let Some(files) = rfd::FileDialog::new().add_filter("AutoEQ / APO", &["txt"]).pick_files() else {
            return;
        };
        let _ = std::fs::create_dir_all(presets_dir());
        let mut last = None;
        for f in files {
            match apo::load(&f) {
                Ok(_) => {
                    let name = f.file_name().unwrap().to_string_lossy().to_string();
                    if std::fs::copy(&f, presets_dir().join(&name)).is_ok() {
                        last = Some(name);
                    }
                }
                Err(e) => self.message = format!("{}: {e}", f.display()),
            }
        }
        self.refresh_library();
        if let Some(n) = last {
            self.select(&n, true);
        }
    }

    pub(super) fn save_preset(&mut self) {
        let _ = std::fs::create_dir_all(presets_dir());
        let name = match &self.preset_name {
            Some(n) => n.clone(),
            None => {
                let Some(p) = rfd::FileDialog::new()
                    .set_directory(presets_dir())
                    .set_file_name("Preset.txt")
                    .add_filter("AutoEQ / APO", &["txt"])
                    .save_file()
                else {
                    return;
                };
                p.file_name().unwrap().to_string_lossy().to_string()
            }
        };
        if std::fs::write(presets_dir().join(&name), apo::to_text(&self.preset)).is_ok() {
            self.preset_name = Some(name.clone());
            self.cfg.preset = Some(name.clone());
            self.dirty = false;
            self.refresh_library();
            self.save_cfg();
            self.say(format!("Kaydedildi: {}", name.trim_end_matches(".txt")));
        }
    }

    pub(super) fn eq_curve_db(&mut self) -> Arc<Vec<f64>> {
        let rate = self
            .running
            .as_ref()
            .map(|r| r.ctl.out_info.lock().unwrap().rate)
            .filter(|r| *r > 0)
            .unwrap_or_else(|| self.player.rate());
        if let Some((p, r, c)) = &self.eq_cache {
            if *r == rate && *p == self.preset {
                return c.clone();
            }
        }
        let fs = rate as f64;
        let coeffs: Vec<Coeffs> = self.preset.bands.iter().filter(|b| b.freq < fs * 0.5).map(|b| Coeffs::design(b, fs)).collect();
        let curve: Vec<f64> = (0..EQ_POINTS)
            .map(|i| {
                let f = 20.0 * 1000f64.powf(i as f64 / (EQ_POINTS - 1) as f64);
                coeffs.iter().map(|c| c.magnitude_db(f, fs)).sum::<f64>()
            })
            .collect();
        let c = Arc::new(curve);
        self.eq_cache = Some((self.preset.clone(), rate, c.clone()));
        c
    }

    pub(super) fn peak_boost(&mut self) -> f64 {
        self.eq_curve_db().iter().copied().fold(0.0, f64::max)
    }

    pub(super) fn preset_chip(ui: &mut egui::Ui, text: &str, active: bool) -> egui::Response {
        let g = ui.painter().layout_no_wrap(text.to_string(), bold(12.5), TEXT);
        let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 22.0, 28.0), Sense::click());
        let h = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.12);
        let a = ui.ctx().animate_bool_with_time(resp.id.with("a"), active, 0.18);
        let bg = lerp_color(lerp_color(BG2, BG3, h), BLUE, a);
        ui.painter().rect_filled(rect, Rounding::same(7.0), bg);
        let col = lerp_color(lerp_color(DIM, TEXT, h), Color32::WHITE, a);
        ui.painter().galley(rect.center() - g.size() * 0.5, g, col);
        resp
    }

    pub(super) fn preset_strip(&mut self, ui: &mut egui::Ui) {
        if dev_skip("strip") {
            return;
        }
        let mut acts = PresetActs::default();
        ui.horizontal(|ui| {
            if pill_button(ui, "İçe aktar", Some(Icon::Import), false).clicked() {
                self.import();
            }
            self.create_button(ui);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(self.dirty && self.preset_name.is_some(), |ui| {
                    if pill_button(ui, "Geri al", Some(Icon::Undo), false).clicked() {
                        let n = self.preset_name.clone().unwrap();
                        self.select(&n, false);
                    }
                });
                ui.add_enabled_ui(self.dirty || self.preset_name.is_none(), |ui| {
                    if pill_button(ui, "Kaydet", Some(Icon::Save), false).clicked() {
                        self.save_preset();
                    }
                });
                self.auto_eq_button(ui);
            });
        });

        ui.add_space(8.0);
        let w = ui.available_width().clamp(200.0, 560.0);
        self.preset_picker(ui, w, &mut acts);
        if let Some(n) = acts.pick {
            self.select(&n, true);
        }
        if let Some(n) = acts.props {
            self.props_open(Some(&n));
        }
        if let Some(n) = acts.copy {
            self.duplicate_preset(&n);
        }
        if let Some(n) = acts.del {
            self.delete_preset(&n);
        }
    }

    fn preset_picker(&mut self, ui: &mut egui::Ui, w: f32, acts: &mut PresetActs) {
        let name = self.preset_name.as_deref().map(|n| n.trim_end_matches(".txt").to_string());
        let (rect, btn) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click());
        let h = ui.ctx().animate_bool_with_time(btn.id, btn.hovered(), 0.12);
        ui.painter().rect_filled(rect, Rounding::same(16.0), lerp_color(BG2, BG3, h));
        draw_icon(ui.painter(), Icon::Folder, pos2(rect.left() + 20.0, rect.center().y), 14.0, lerp_color(DIM, TEXT, h));
        let shown = match (&name, self.library.is_empty()) {
            (Some(n), _) => n.clone(),
            (None, true) => "Henüz ön ayar yok".to_string(),
            (None, false) => "Ön ayar seç".to_string(),
        };
        let txt_w = rect.width() - 20.0 - 18.0 - 34.0 - if self.dirty { 14.0 } else { 0.0 };
        let shown = super::eqrules_ui::fit(ui.painter(), &shown, bold(13.0), txt_w);
        let g = ui.painter().layout_no_wrap(shown, bold(13.0), if name.is_some() { Color32::WHITE } else { DIM });
        let gw = g.size().x;
        ui.painter().galley(pos2(rect.left() + 38.0, rect.center().y - g.size().y * 0.5), g, Color32::WHITE);
        if self.dirty && name.is_some() {
            ui.painter().circle_filled(pos2(rect.left() + 38.0 + gw + 10.0, rect.center().y), 3.5, AMBER);
        }
        draw_icon(ui.painter(), Icon::ChevronDown, pos2(rect.right() - 20.0, rect.center().y), 12.0, lerp_color(DIM, TEXT, h));
        let id = btn.id.with("preset_list");
        if btn.clone().on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.preset_q.clear();
            self.preset_q_focus = true;
            ui.memory_mut(|m| m.toggle_popup(id));
        }

        {
            static OPENED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if crate::devenv::dev_var("SW_PRESETLIST").is_ok() && !OPENED.swap(true, Relaxed) {
                ui.memory_mut(|m| m.open_popup(id));
            }
        }
        let mut q = std::mem::take(&mut self.preset_q);
        let mut focus = std::mem::take(&mut self.preset_q_focus);
        egui::popup_below_widget(ui, id, &btn, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
            ui.set_min_width(w.max(380.0));
            let te = ui.add(egui::TextEdit::singleline(&mut q).hint_text("Ön ayar ara…").desired_width(ui.available_width()).margin(vec2(10.0, 7.0)));
            if focus {
                te.request_focus();
                focus = false;
            }
            ui.add_space(6.0);
            let ql = q.trim().to_lowercase();

            let mut own: Vec<&String> = vec![];
            let mut auto: Vec<&String> = vec![];
            for (n, _) in &self.library {
                let meta = self.cfg.preset_meta.get(n);
                if !ql.is_empty() && !n.to_lowercase().contains(&ql) && !meta.map_or(false, |m| m.label.to_lowercase().contains(&ql)) {
                    continue;
                }
                if meta.map_or(false, |m| m.source.starts_with("AutoEQ")) {
                    auto.push(n);
                } else {
                    own.push(n);
                }
            }
            egui::ScrollArea::vertical().id_salt("preset_list").max_height(360.0).auto_shrink([true, true]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                if own.is_empty() && auto.is_empty() {
                    ui.label(RichText::new(if ql.is_empty() { "Henüz ön ayar yok" } else { "Sonuç yok" }).font(body(12.5)).color(DIM));
                }
                for (title, group) in [("Ön ayarlarım", &own), ("AutoEQ", &auto)] {
                    if group.is_empty() {
                        continue;
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add_space(4.0);
                        ui.label(RichText::new(format!("{title}  ·  {}", group.len())).font(bold(11.5)).color(DIM));
                    });
                    for n in group.iter() {
                        let meta = self.cfg.preset_meta.get(*n);
                        let active = self.preset_name.as_deref() == Some(n.as_str());
                        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
                        let hv = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1);
                        ui.painter().rect_filled(r, Rounding::same(9.0), if active { alpha(BLUE, 44) } else { alpha(BG3, (200.0 * hv) as u8) });
                        if active {
                            ui.painter().rect_filled(Rect::from_min_size(r.min + vec2(0.0, 8.0), vec2(3.0, r.height() - 16.0)), Rounding::same(2.0), BLUE);
                        }
                        let title = super::eqrules_ui::fit(ui.painter(), n.trim_end_matches(".txt"), bold(13.0), r.width() - 64.0);
                        let sub = meta.map(|m| if !m.label.is_empty() { m.label.clone() } else { m.source.clone() }).unwrap_or_default();

                        ui.painter().text(pos2(r.left() + 14.0, r.top() + if sub.is_empty() { 22.0 } else { 15.0 }), Align2::LEFT_CENTER, title, bold(13.0), Color32::WHITE);
                        if !sub.is_empty() {
                            let sub = super::eqrules_ui::fit(ui.painter(), &sub, body(11.5), r.width() - 64.0);
                            ui.painter().text(pos2(r.left() + 14.0, r.top() + 31.0), Align2::LEFT_CENTER, sub, body(11.5), DIM);
                        }

                        let gear = Rect::from_center_size(pos2(r.right() - 22.0, r.center().y), vec2(28.0, 28.0));
                        let gresp = ui.interact(gear, resp.id.with("gear"), Sense::click());
                        if hv > 0.01 || gresp.hovered() {
                            draw_icon(ui.painter(), Icon::Settings, gear.center(), 15.0, if gresp.hovered() { Color32::WHITE } else { alpha(DIM, 220) });
                        }
                        let gear_clicked = gresp.clicked();
                        gresp.on_hover_text("Özellikler");
                        if gear_clicked {
                            acts.props = Some((*n).clone());
                        } else if resp.clicked() {
                            acts.pick = Some((*n).clone());
                            ui.memory_mut(|m| m.close_popup());
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Özellikler…").clicked() {
                                acts.props = Some((*n).clone());
                                ui.close_menu();
                            }
                            if ui.button("Kopya oluştur").clicked() {
                                acts.copy = Some((*n).clone());
                                ui.close_menu();
                            }
                            if ui.button("Sil").clicked() {
                                acts.del = Some((*n).clone());
                                ui.close_menu();
                            }
                        });
                    }
                }
            });
        });
        self.preset_q = q;
        self.preset_q_focus = focus;
        if acts.props.is_some() || acts.copy.is_some() || acts.del.is_some() {
            ui.memory_mut(|m| m.close_popup());
        }
    }

    pub(super) fn delete_preset(&mut self, name: &str) {
        let path = presets_dir().join(name);
        let bytes = std::fs::read(&path).unwrap_or_default();
        let _ = std::fs::remove_file(&path);
        self.cfg.preset_meta.remove(name);
        if self.preset_name.as_deref() == Some(name) {
            self.preset_name = None;
            self.cfg.preset = None;
        }
        self.refresh_library();
        self.undo = Some((Undo::Preset(name.to_string(), bytes), format!("\"{}\" silindi", name.trim_end_matches(".txt")), Instant::now()));
    }

    pub(super) fn ab_button(ui: &mut egui::Ui, held_in: &mut bool) {
        let (rect, resp) = ui.allocate_exact_size(vec2(150.0, 28.0), Sense::click_and_drag());
        if resp.is_pointer_button_down_on() {
            *held_in = true;
        }
        let held = *held_in;
        let t = ui.ctx().animate_bool_with_time(resp.id, held, 0.1);
        let h = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.12);
        let bg = lerp_color(lerp_color(BG2, BG3, h), RED, t);
        ui.painter().rect_filled(rect, Rounding::same(8.0), bg);
        ui.painter().text(pos2(rect.left() + 12.0, rect.center().y), Align2::LEFT_CENTER, "A/B", bold(13.0), Color32::WHITE);
        ui.painter().text(
            pos2(rect.right() - 12.0, rect.center().y),
            Align2::RIGHT_CENTER,
            if held { "EQ kapalı" } else { "basılı tut" },
            body(11.5),
            lerp_color(DIM, Color32::WHITE, t),
        );
    }

    pub(super) fn eq_graph(&mut self, ui: &mut egui::Ui, size: egui::Vec2) {
        if dev_skip("graph") {
            return;
        }
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, Rounding::same(8.0), BG0);

        let fx = |f: f64| rect.left() + ((f / 20.0).log10() / 3.0) as f32 * rect.width();
        let xf = |x: f32| 20.0 * 10f64.powf(3.0 * ((x - rect.left()) / rect.width()) as f64);
        let half = rect.height() as f64 * 0.5 - 12.0;
        let gy = |d: f64| rect.center().y - (d / GAIN_RANGE * half) as f32;
        let y_to_gain = |y: f32| (rect.center().y - y) as f64 / half * GAIN_RANGE;

        viz::draw_freq_grid(&painter, rect);
        for d in (-3..=3).map(|i| i as f64 * 6.0) {
            painter.line_segment(
                [pos2(rect.left(), gy(d)), pos2(rect.right(), gy(d))],
                Stroke::new(1.0, if d == 0.0 { alpha(DIM, 90) } else { alpha(STROKE, 120) }),
            );
            painter.text(pos2(rect.right() - 6.0, gy(d) - 8.0), Align2::RIGHT_CENTER, format!("{d:+.0}"), body(10.5), DIM);
        }

        self.viz.draw_spectrum(&painter, rect, self.cfg.viz_bars);

        let curve = self.eq_curve_db();
        let pts: Vec<egui::Pos2> = curve
            .iter()
            .enumerate()
            .map(|(i, db)| {
                let f = 20.0 * 1000f64.powf(i as f64 / (EQ_POINTS - 1) as f64);
                pos2(fx(f), gy(db.clamp(-GAIN_RANGE, GAIN_RANGE)))
            })
            .collect();
        painter.add(Shape::line(pts, Stroke::new(2.0, Color32::WHITE)));

        let node_pos = |b: &Band| {
            let g = if b.kind.has_gain() { b.gain_db } else { 0.0 };
            pos2(fx(b.freq), gy(g.clamp(-GAIN_RANGE, GAIN_RANGE)))
        };
        let hover = resp.hover_pos();
        let hit = |p: egui::Pos2, bands: &[Band]| -> Option<usize> {
            bands
                .iter()
                .enumerate()
                .map(|(i, b)| (i, node_pos(b).distance(p)))
                .filter(|(_, d)| *d < 13.0)
                .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                .map(|(i, _)| i)
        };
        let before = self.preset.clone();
        if resp.drag_started() {
            if let Some(p) = resp.interact_pointer_pos() {
                self.drag_band = hit(p, &self.preset.bands);
                if self.drag_band.is_some() {
                    self.sel_band = self.drag_band;
                }
            }
        }
        if resp.dragged() {
            if let (Some(i), Some(p)) = (self.drag_band, resp.interact_pointer_pos()) {
                if let Some(b) = self.preset.bands.get_mut(i) {
                    b.freq = xf(p.x).clamp(20.0, 20000.0);
                    if b.kind.has_gain() {
                        b.gain_db = (y_to_gain(p.y).clamp(-GAIN_RANGE, GAIN_RANGE) * 10.0).round() / 10.0;
                    }
                }
            }
        }
        if resp.drag_stopped() {
            self.drag_band = None;
        }
        let mut hot_band = None;
        if let Some(p) = hover {
            if let Some(i) = hit(p, &self.preset.bands) {
                hot_band = Some(i);
                let scroll = ui.input(|inp| inp.raw_scroll_delta.y);
                if scroll != 0.0 {
                    let b = &mut self.preset.bands[i];
                    b.q = (b.q * 1.1f64.powf(scroll as f64 / 40.0)).clamp(0.1, 30.0);
                }
                if resp.secondary_clicked() {
                    self.preset.bands.remove(i);
                    self.sel_band = None;
                    hot_band = None;
                }
            }
        }
        if resp.double_clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                if hit(p, &self.preset.bands).is_none() && self.preset.bands.len() < 32 {
                    self.preset.bands.push(Band {
                        kind: FilterKind::Peak,
                        freq: xf(p.x).clamp(20.0, 20000.0),
                        gain_db: (y_to_gain(p.y).clamp(-GAIN_RANGE, GAIN_RANGE) * 10.0).round() / 10.0,
                        q: 1.0,
                    });
                    self.sel_band = Some(self.preset.bands.len() - 1);
                }
            }
        }

        let solo_key = ui.input(|inp| inp.key_down(egui::Key::S) && !inp.modifiers.command && !inp.modifiers.alt) && !ui.ctx().wants_keyboard_input();
        if solo_key || self.solo_latch {
            let target = match self.drag_band.or(self.sel_band).and_then(|i| self.preset.bands.get(i)) {
                Some(b) => Some((b.freq, b.q.clamp(0.5, 20.0))),
                None => hover.map(|p| (xf(p.x).clamp(20.0, 20000.0), 2.0)),
            };
            if let Some((f0, q)) = target {
                self.solo_target = Some((f0, q));

                let bw = f0 / q;
                let (lo, hi) = ((f0 - bw * 0.5).max(20.0), (f0 + bw * 0.5).min(20000.0));
                let band = Rect::from_min_max(pos2(fx(lo), rect.top()), pos2(fx(hi).max(fx(lo) + 3.0), rect.bottom()));
                painter.rect_filled(band, Rounding::ZERO, alpha(BLUE, 46));
                painter.line_segment([pos2(fx(f0), rect.top()), pos2(fx(f0), rect.bottom())], Stroke::new(1.5, SKY));
                let (note, cents) = crate::biquad::note_name(f0);
                let freq_txt = if f0 >= 1000.0 { format!("{:.2} kHz", f0 / 1000.0) } else { format!("{:.0} Hz", f0) };
                let lbl = format!("SOLO · {freq_txt} · {note}{}", if cents == 0 { String::new() } else { format!(" {cents:+}c") });
                let g = painter.layout_no_wrap(lbl, bold(13.0), Color32::WHITE);
                let x = (fx(f0) + 10.0).min(rect.right() - g.size().x - 14.0).max(rect.left() + 8.0);
                let box_r = Rect::from_min_size(pos2(x - 6.0, rect.top() + 34.0), g.size() + vec2(12.0, 8.0));
                painter.rect_filled(box_r, Rounding::same(6.0), alpha(BG0, 230));
                painter.galley(pos2(x, rect.top() + 38.0), g, Color32::WHITE);
            } else {
                painter.text(rect.center(), Align2::CENTER_CENTER, "Fareyi grafiğin üstüne getir ya da bir bant seç", body(12.5), alpha(TEXT, 200));
            }
        }
        for (i, b) in self.preset.bands.iter().enumerate() {
            let p = node_pos(b);
            let sel = self.sel_band == Some(i);
            let hot = hot_band == Some(i);
            let k = ui.ctx().animate_value_with_time(Id::new(("node", i)), if sel || hot { 1.0 } else { 0.0 }, 0.1);
            if sel {
                painter.line_segment([pos2(p.x, rect.top()), pos2(p.x, rect.bottom())], Stroke::new(1.0, alpha(BLUE, 70)));
            }
            let r = 5.0 + 2.0 * k;
            painter.circle_filled(p, r, if sel { BLUE } else { BG0 });
            painter.circle_stroke(p, r, Stroke::new(1.5, if sel { Color32::WHITE } else { SKY }));
        }
        if let Some(i) = hot_band.or(self.drag_band).or(self.sel_band) {
            if let Some(b) = self.preset.bands.get(i) {
                let txt = if b.kind.has_gain() {
                    format!("#{} {} · {:.0} Hz · {:+.1} dB · Q {:.2}", i + 1, b.kind.label(), b.freq, b.gain_db, b.q)
                } else {
                    format!("#{} {} · {:.0} Hz · Q {:.2}", i + 1, b.kind.label(), b.freq, b.q)
                };
                painter.text(rect.left_top() + vec2(12.0, 12.0), Align2::LEFT_TOP, txt, bold(12.0), TEXT);
            }
        } else {
            painter.text(
                rect.left_top() + vec2(12.0, 12.0),
                Align2::LEFT_TOP,
                "Sürükle: frekans/kazanç · Tekerlek: Q · Çift tık: ekle · Sağ tık: sil · S basılı: solo dinle",
                body(11.0),
                alpha(DIM, 170),
            );
        }
        if self.preset != before {
            self.dirty = true;
            self.push_preset();
        }
    }

    pub(super) fn band_faders(&mut self, ui: &mut egui::Ui) {
        if dev_skip("faders") {
            return;
        }
        let before = self.preset.clone();
        let mut remove = None;
        ui.horizontal(|ui| {
            section_label(ui, "Parametrik EQ");
            let boost = self.peak_boost();
            ui.label(RichText::new(format!("tepe boost {:+.1} dB", boost)).font(body(12.0)).color(if boost + self.preset.preamp_db > 0.05 { AMBER } else { DIM }));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if pill_button(ui, "Solo", Some(Icon::Wave), self.solo_latch)
                    .on_hover_text("S tuşunu basılı tutarak ya da bu düğmeyle: yalnızca seçili bandın bölgesini dinle (bant seçili değilse farenin üstündeki frekansı)")
                    .clicked()
                {
                    self.solo_latch = !self.solo_latch;
                }
                if pill_button(ui, "Düzleştir", None, false).on_hover_text("Tüm kazançları 0 yap").clicked() {
                    for b in &mut self.preset.bands {
                        b.gain_db = 0.0;
                    }
                }
                if pill_button(ui, "Oto preamp", None, false).on_hover_text("Kırpılmayı önleyen preamp").clicked() {
                    self.preset.preamp_db = -((boost * 10.0).ceil() / 10.0);
                }
            });
        });
        ui.add_space(2.0);
        egui::ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 4.0);
                ui.vertical(|ui| {
                    ui.set_width(70.0);
                    ui.label(RichText::new("Preamp").font(bold(12.0)).color(DIM));
                    ui.add_space(28.0);
                    ui.spacing_mut().slider_width = 80.0;
                    ui.add(egui::Slider::new(&mut self.preset.preamp_db, -20.0..=6.0).vertical().show_value(false));
                    ui.add(egui::DragValue::new(&mut self.preset.preamp_db).speed(0.1).range(-20.0..=6.0).suffix(" dB").fixed_decimals(1));
                });
                for (i, b) in self.preset.bands.iter_mut().enumerate() {
                    let sel = self.sel_band == Some(i);
                    let s = ui.ctx().animate_bool_with_time(Id::new(("colsel", i)), sel, 0.15);
                    let frame = Frame::none()
                        .fill(alpha(BLUE, (22.0 * s) as u8))
                        .rounding(Rounding::same(8.0))
                        .inner_margin(Margin::symmetric(6.0, 4.0));
                    frame.show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(80.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                if ui.add(egui::Label::new(RichText::new(format!("{}", i + 1)).font(bold(12.0)).color(lerp_color(DIM, SKY, s))).sense(Sense::click())).clicked() {
                                    self.sel_band = Some(i);
                                }
                                ui.add_space(46.0);
                                if icon_button(ui, Icon::Close, 18.0, false).clicked() {
                                    remove = Some(i);
                                }
                            });
                            egui::ComboBox::from_id_salt(("kind", i)).width(74.0).selected_text(b.kind.label()).show_ui(ui, |ui| {
                                for k in FilterKind::ALL {
                                    ui.selectable_value(&mut b.kind, k, k.label());
                                }
                            });
                            ui.spacing_mut().slider_width = 76.0;
                            ui.add_enabled(
                                b.kind.has_gain(),
                                egui::Slider::new(&mut b.gain_db, -GAIN_RANGE..=GAIN_RANGE).vertical().show_value(false),
                            );
                            ui.add_enabled(
                                b.kind.has_gain(),
                                egui::DragValue::new(&mut b.gain_db).speed(0.1).range(-GAIN_RANGE..=GAIN_RANGE).suffix(" dB").fixed_decimals(1),
                            );
                            let sp = (b.freq * 0.004).max(0.2);
                            ui.add(egui::DragValue::new(&mut b.freq).speed(sp).range(20.0..=20000.0).suffix(" Hz").fixed_decimals(0));
                            ui.add(egui::DragValue::new(&mut b.q).speed(0.02).range(0.1..=30.0).prefix("Q ").fixed_decimals(2));
                        });
                    });
                }
                if self.preset.bands.len() < 32 {
                    ui.vertical(|ui| {
                        ui.add_space(60.0);
                        if icon_button(ui, Icon::Plus, 36.0, false).on_hover_text("Bant ekle").clicked() {
                            self.preset.bands.push(Band { kind: FilterKind::Peak, freq: 1000.0, gain_db: 0.0, q: 1.0 });
                            self.sel_band = Some(self.preset.bands.len() - 1);
                        }
                    });
                }
            });
        });
        if let Some(i) = remove {
            self.preset.bands.remove(i);
            self.sel_band = None;
        }
        if self.preset != before {
            self.dirty = true;
            self.push_preset();
        }
    }
}
