use super::*;

impl App {
    pub(super) fn lyrics_identity(&self) -> Option<(String, String, String, f64)> {
        if self.cfg.use_loopback || self.meta.title.trim().is_empty() {
            return None;
        }
        Some((self.meta.title.clone(), self.meta.artist.clone(), self.meta.album.clone(), self.player.duration()))
    }

    pub(super) fn poll_lyrics(&mut self) {
        for (k, l) in self.lyr_fetcher.poll() {
            match l {
                Some(x) => {
                    self.lyr_failed.remove(&k);
                    self.lyr_cache.insert(k, x);
                }
                None => {
                    self.lyr_failed.insert(k);
                }
            }
        }
    }

    pub(super) fn ensure_lyrics(&mut self) {
        let Some((title, artist, album, dur)) = self.lyrics_identity() else { return };
        let k = lyrics::key(&artist, &title);
        if k != self.lyr_key {
            self.lyr_key = k.clone();
            self.lyr_scroll = 0.0;
            self.lyr_manual = 0.0;
            self.lyr_layout = None;
        }
        if self.lyr_cache.contains_key(&k) {
            return;
        }
        if let Some(l) = lyrics::load_cached(&k) {
            self.lyr_cache.insert(k, l);
            return;
        }
        if dur <= 0.0 {
            return;
        }
        if !self.lyr_failed.contains(&k) && !self.lyr_fetcher.is_pending(&k) {
            self.lyr_fetcher.request(k, title, artist, album, dur);
        }
    }

    pub(super) fn build_lyr_layout(&self, ctx: &egui::Context, key: &str, l: &Lyrics, width: f32) -> LyrLayout {
        let font = bold(25.0);
        let font_bg = bold(16.0);
        let space = ctx.fonts(|f| f.glyph_width(&font, ' '));
        let space_bg = ctx.fonts(|f| f.glyph_width(&font_bg, ' '));
        let mut lines = vec![];
        let mut y = 0.0f32;
        for line in &l.lines {

            let raw: Vec<(String, Option<usize>, bool, bool)> = if !line.words.is_empty() {
                line.words.iter().enumerate().map(|(i, w)| (w.text.clone(), Some(i), w.sp, w.bg)).collect()
            } else {
                line.text.split_whitespace().map(|s| (s.to_string(), None, true, false)).collect()
            };
            let mut toks_in: Vec<(String, Option<usize>, bool, bool)> = vec![];
            let mut in_paren = false;
            for (text, word, sp, forced_bg) in raw {
                let starts = text.starts_with('(');
                if starts {
                    in_paren = true;
                }
                let bg = in_paren || forced_bg;
                if text.ends_with(')') {
                    in_paren = false;
                }
                let shown = text.trim_matches(|c| c == '(' || c == ')').to_string();
                if !shown.is_empty() {
                    toks_in.push((shown, word, sp, bg));
                }
            }
            let mut toks = vec![];
            let mut ly = 0.0f32;
            for pass_bg in [false, true] {
                let (f, sp_w) = if pass_bg { (font_bg.clone(), space_bg) } else { (font.clone(), space) };
                let (mut x, mut row_h) = (0.0f32, 0.0f32);
                let mut any = false;
                for (text, word, sp, bg) in toks_in.iter().filter(|t| t.3 == pass_bg) {
                    let galley = ctx.fonts(|fo| fo.layout_no_wrap(text.clone(), f.clone(), Color32::WHITE));
                    let (w, h) = (galley.size().x, galley.size().y);
                    if x > 0.0 && x + w > width {
                        x = 0.0;
                        ly += row_h + 5.0;
                        row_h = 0.0;
                    }
                    row_h = row_h.max(h);
                    toks.push(LyrTok { bg: *bg, x, y: ly, galley, word: *word });
                    x += w + if *sp { sp_w } else { 0.0 };
                    any = true;
                }
                if any {
                    ly += row_h + if pass_bg { 0.0 } else { 6.0 };
                }
            }
            let min_h = if toks.iter().all(|t| t.bg) { 24.0 } else { 36.0 };
            let h = ly.max(min_h);
            lines.push(LyrLine { y, h, toks });
            y += h + 28.0;
        }
        LyrLayout { key: key.to_string(), width: width as u32, lines }
    }

    pub(super) fn lyrics_sidebar(&mut self, ctx: &egui::Context) {
        let t = ctx.animate_bool_with_time(Id::new("lyr_side"), self.lyrics_open && !self.cfg.use_loopback, 0.24);
        if t <= 0.005 {
            return;
        }
        let e = ease(t);
        let full_w = 450.0f32;
        self.ensure_lyrics();
        let identity = self.lyrics_identity();
        let key = identity.as_ref().map(|(t, a, _, _)| lyrics::key(a, t));
        let cached = key.as_ref().and_then(|k| self.lyr_cache.get(k)).cloned();
        let pending = key.as_ref().map(|k| self.lyr_fetcher.is_pending(k)).unwrap_or(false);
        let known = identity.is_some();
        let (mut retry, mut close) = (false, false);

        egui::SidePanel::right("lyrics_side")
            .exact_width(full_w * e + 12.0)
            .resizable(false)
            .frame(Frame::none().inner_margin(Margin { left: 0.0, right: 12.0, top: 2.0, bottom: 2.0 }))
            .show(ctx, |ui| {
                let inner = Margin { left: 30.0, right: 30.0, top: 22.0, bottom: 12.0 };
                let avail = ui.available_size();
                panel_frame(inner, Margin::ZERO).show(ui, |ui| {
                ui.set_min_size(vec2(avail.x - inner.left - inner.right, avail.y - inner.top - inner.bottom));
                self.paint_glow(ui, inner);
                ui.set_opacity(e);
                let cw = full_w - 60.0;
                ui.set_min_width(cw);
                ui.allocate_ui(vec2(cw, 50.0), |ui| {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(46.0, 46.0), Sense::hover());
                        match &self.cover_tex {
                            Some(tx) if known => rounded_image(ui.painter(), tx.id(), r, 8.0, Color32::WHITE),
                            _ => {
                                ui.painter().rect_filled(r, Rounding::same(8.0), BG2);
                                draw_icon(ui.painter(), Icon::Wave, r.center(), 24.0, DIM);
                            }
                        }
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.set_max_width((cw - 46.0 - 110.0).max(120.0));
                            let title = if known { self.meta.title.clone() } else { "Çalan şarkı yok".to_string() };
                            ui.add(egui::Label::new(RichText::new(title).font(bold(16.0))).wrap());
                            if known {
                                ui.add(egui::Label::new(RichText::new(self.meta.artist.clone()).font(body(13.0)).color(SKY)).wrap());
                            }
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icon_button(ui, Icon::Close, 30.0, false).on_hover_text_tr("Kapat").clicked() {
                                close = true;
                            }
                            if cached.is_some() && icon_button(ui, Icon::Undo, 30.0, false).on_hover_text_tr("Yeniden dene").clicked() {
                                retry = true;
                            }
                            if let (Some(l), Some(k)) = (&cached, &key) {
                                if l.found() && l.kind != "plain" {
                                    let shifted = self.cfg.lyr_offsets.contains_key(k);
                                    let gear = icon_button(ui, Icon::Settings, 30.0, shifted).on_hover_text_tr("Söz kayması");
                                    self.lyric_offset_popup(ui, &gear, k);
                                }
                            }
                        });
                    });
                });
                if let Some(l) = &cached {
                    if l.found() {
                        let kind = match l.kind.as_str() {
                            "word" => "kelime senkronlu",
                            "synced" => "satır senkronlu",
                            _ => "senkronsuz",
                        };
                        ui.add_space(6.0);
                        ui.label(RichText::new(format!("{} · {}", l.provider, kind)).font(body(12.0)).color(alpha(TEXT, 110)));
                    }
                }
                ui.add_space(4.0);
                let (rect, resp) = ui.allocate_exact_size(vec2(cw, ui.available_height().max(120.0)), Sense::click());
                let painter = ui.painter_at(rect);
                match (&cached, known) {
                    (_, false) => {
                        painter.text_tr(rect.center(), Align2::CENTER_CENTER, "Bir şarkı çal; sözleri burada görünür.", body(14.0), DIM);
                    }
                    (None, true) => {
                        let failed = key.as_ref().map(|k| self.lyr_failed.contains(k)).unwrap_or(false);
                        if pending || !failed {
                            painter.text_tr(rect.center(), Align2::CENTER_CENTER, "Sözler aranıyor…", body(14.0), DIM);
                            ctx.request_repaint();
                        } else {
                            painter.text_tr(rect.center(), Align2::CENTER_CENTER, "Sözler alınamadı", bold(15.0), AMBER);
                            painter.text_tr(rect.center() + vec2(0.0, 24.0), Align2::CENTER_CENTER, "İnternet bağlantını kontrol et.", body(12.0), DIM);
                        }
                    }
                    (Some(l), true) if !l.found() => {
                        painter.text_tr(rect.center(), Align2::CENTER_CENTER, "Bu şarkı için söz bulunamadı", bold(16.0), DIM);
                        painter.text_tr(rect.center() + vec2(0.0, 26.0), Align2::CENTER_CENTER, "Apple Music, BiniLyrics ve LRCLib arandı.", body(12.0), DIM);
                    }
                    (Some(l), true) => {
                        self.draw_lyrics(ctx, ui, &painter, rect, &resp, l, key.as_deref().unwrap_or(""));
                    }
                }
                });
            });
        if retry {
            if let Some(k) = &key {
                lyrics::forget(k);
                self.lyr_cache.remove(k);
                self.lyr_failed.remove(k);
                self.lyr_layout = None;
            }
        }
        if close {
            self.lyrics_open = false;
        }
    }

    fn lyric_offset_popup(&mut self, ui: &mut egui::Ui, gear: &egui::Response, key: &str) {
        const STEP: i32 = 100;
        let id = gear.id.with("lyr_off");
        if gear.clicked() {
            ui.memory_mut(|m| m.toggle_popup(id));
        }
        let cur = self.cfg.lyr_offsets.get(key).copied().unwrap_or(0);
        let mut next = cur;
        let mut dir: Option<i32> = None;
        egui::popup_below_widget(ui, id, gear, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
            ui.set_min_width(230.0);
            ui.label(RichText::new("Söz kayması").font(bold(13.5)).color(Color32::WHITE));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if pill_button(ui, "−", None, false).is_pointer_button_down_on() {
                    dir = Some(-1);
                }
                let mut secs = cur as f64 / 1000.0;
                let edit = egui::DragValue::new(&mut secs)
                    .range(-10.0..=10.0)
                    .speed(0.01)
                    .custom_formatter(|v, _| format!("{v:+.2} {}", crate::i18n::tr("sn")))
                    .custom_parser(|s| s.trim().trim_end_matches(['s', 'n']).trim().replace(',', ".").parse::<f64>().ok());
                if ui.add(edit).on_hover_text_tr("Tıklayıp yaz (ör. -2,35) ya da sürükle").changed() {
                    next = (secs * 1000.0).round() as i32;
                }
                if pill_button(ui, "+", None, false).is_pointer_button_down_on() {
                    dir = Some(1);
                }
                if cur != 0 && pill_button(ui, "Sıfırla", None, false).clicked() {
                    next = 0;
                }
            });
        });
        match dir {
            Some(d) => {
                let now = Instant::now();
                match self.lyr_hold {
                    Some((t0, last, hd)) if hd == d => {
                        if let Some(step) = hold_repeat(t0.elapsed(), last.elapsed()) {
                            next += d * step;
                            self.lyr_hold = Some((t0, now, d));
                        }
                    }
                    _ => {
                        next += d * STEP;
                        self.lyr_hold = Some((now, now, d));
                    }
                }
                ui.ctx().request_repaint();
            }
            None => self.lyr_hold = None,
        }
        if next != cur {
            if next == 0 {
                self.cfg.lyr_offsets.remove(key);
            } else {
                self.cfg.lyr_offsets.insert(key.to_string(), next.clamp(-10_000, 10_000));
            }
        }
    }

    pub(super) fn draw_lyrics(&mut self, ctx: &egui::Context, ui: &mut egui::Ui, painter: &egui::Painter, rect: Rect, resp: &egui::Response, l: &Lyrics, key: &str) {
        let width = (rect.width() - 16.0).max(200.0);
        let need = match &self.lyr_layout {
            Some(x) => x.key != key || (x.width as f32 - width).abs() > 24.0,
            None => true,
        };
        if need {
            self.lyr_layout = Some(self.build_lyr_layout(ctx, key, l, width));
        }
        let synced = l.kind != "plain";
        let dt = ctx.input(|i| i.unstable_dt).clamp(0.001, 0.1);

        let actual = self.player.position();
        if self.player.playing.load(Relaxed) {
            let est = self.lyr_pos + dt as f64;
            let err = actual - est;
            self.lyr_pos = if err.abs() > 0.35 { actual } else { est + err * 0.12 };
        } else {
            self.lyr_pos = actual;
        }
        let pos_ms = ((self.lyr_pos + 0.04) * 1000.0) as i64 + self.cfg.lyr_offsets.get(key).copied().unwrap_or(0) as i64;
        let cur: Option<usize> = if synced { l.lines.iter().rposition(|x| x.t as i64 <= pos_ms) } else { None };

        let lay = self.lyr_layout.as_ref().unwrap();

        let anchor = rect.height() * 0.32;
        let target = match cur {
            Some(i) => lay.lines[i].y + lay.lines[i].h * 0.5,
            None => {
                if synced {
                    lay.lines.first().map(|x| x.y + x.h * 0.5).unwrap_or(0.0)
                } else {
                    anchor
                }
            }
        };
        let (s, v) = smooth_damp(self.lyr_scroll, target, self.lyr_vel, 0.5, dt);
        self.lyr_scroll = s;
        self.lyr_vel = v;

        if resp.hovered() {
            let sc = ui.input(|i| i.raw_scroll_delta.y);
            if sc != 0.0 {
                self.lyr_manual -= sc;
                self.lyr_manual_t = Instant::now();
            }
        }
        if self.lyr_manual_t.elapsed().as_secs_f32() > 3.0 {
            self.lyr_manual *= 1.0 - (dt * 4.0).min(1.0);
        }
        let center = rect.top() + anchor - self.lyr_scroll - self.lyr_manual;

        let mut clicked_line: Option<usize> = None;
        let pointer = resp.interact_pointer_pos();
        for (i, line) in lay.lines.iter().enumerate() {
            let top = center + line.y;
            if top + line.h < rect.top() - 14.0 || top > rect.bottom() + 14.0 {
                continue;
            }
            let dist = cur.map(|c| (i as i32 - c as i32).abs()).unwrap_or(1);

            let mid_y = top + line.h * 0.5;
            let edge = smoothstep((mid_y - rect.top()) / 90.0) * smoothstep((rect.bottom() - mid_y) / 120.0);
            let base = if !synced { 0.8 } else if Some(i) == cur { 1.0 } else { (0.36 - 0.06 * (dist as f32 - 1.0)).max(0.14) };
            let a = ctx.animate_value_with_time(Id::new(("lyr", i)), base, 0.45) * edge;
            let is_cur = Some(i) == cur;
            let data = &l.lines[i];

            let word_fill = is_cur && l.kind == "word";
            let dim_a = if word_fill { 0.36 * edge } else { a };

            for tok in &line.toks {
                let pos = pos2(rect.left() + tok.x, top + tok.y);
                let scale_a = if tok.bg { if is_cur { 0.85 } else { 0.7 } } else { 1.0 };
                let scale_a_full = scale_a * edge;
                if word_fill {
                    let prog = match tok.word.and_then(|wi| data.words.get(wi)) {
                        Some(wd) => smoothstep(((pos_ms - wd.t as i64) as f32 / (wd.d.max(220)) as f32).clamp(0.0, 1.0)),
                        None => 1.0,
                    };
                    let bright = alpha(Color32::WHITE, (255.0 * scale_a_full) as u8);
                    draw_word(painter, pos, &tok.galley, prog, alpha(TEXT, (255.0 * dim_a * scale_a) as u8), bright);
                } else {
                    painter.galley_with_override_text_color(pos, tok.galley.clone(), alpha(if is_cur { Color32::WHITE } else { TEXT }, (255.0 * a * scale_a) as u8));
                }
            }
            if let Some(p) = pointer {
                if resp.clicked() && synced && p.y >= top - 12.0 && p.y <= top + line.h + 12.0 {
                    clicked_line = Some(i);
                }
            }
        }
        if let Some(i) = clicked_line {
            self.player.seek(l.lines[i].t as f64 / 1000.0);
            self.lyr_manual = 0.0;
        }
        if synced {

            ctx.request_repaint_after(std::time::Duration::from_millis(40));
        }
    }
}

fn hold_repeat(held: std::time::Duration, since_last: std::time::Duration) -> Option<i32> {
    use std::time::Duration as D;
    let (interval, step) = if held < D::from_millis(400) {
        return None;
    } else if held < D::from_millis(1200) {
        (D::from_millis(100), 100)
    } else if held < D::from_millis(2500) {
        (D::from_millis(40), 100)
    } else {
        (D::from_millis(40), 500)
    };
    (since_last >= interval).then_some(step)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration as D;

    #[test]
    fn hold_waits_then_accelerates() {
        assert_eq!(hold_repeat(D::from_millis(200), D::from_millis(200)), None, "ilk 0,4 sn tekrar yok");
        assert_eq!(hold_repeat(D::from_millis(600), D::from_millis(50)), None, "aralık dolmadan adım yok");
        assert_eq!(hold_repeat(D::from_millis(600), D::from_millis(100)), Some(100));
        assert_eq!(hold_repeat(D::from_millis(1500), D::from_millis(40)), Some(100), "daha sık");
        assert_eq!(hold_repeat(D::from_millis(3000), D::from_millis(40)), Some(500), "uzun basışta büyük adım");
    }
}
