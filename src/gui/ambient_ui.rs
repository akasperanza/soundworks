use super::*;
use egui::text::LayoutJob;
use std::time::Duration;

fn galley(p: &egui::Painter, text: String, font: egui::FontId, color: Color32, wrap: f32, rows: usize) -> Arc<egui::Galley> {
    let mut job = LayoutJob::simple(text, font, color, wrap);
    job.wrap.max_rows = rows;
    job.wrap.break_anywhere = false;
    p.layout_job(job)
}

fn accent_of(c: Color32) -> Color32 {
    let mut h = egui::ecolor::Hsva::from(c);
    h.s = (h.s * 1.5).clamp(0.5, 0.9);
    h.v = 0.95;
    Color32::from(h)
}

impl App {
    fn ambient_blocked(&self) -> bool {
        self.search_open || self.props.is_some() || self.ae.open || self.dsd_dialog.is_some() || self.upload_open
    }

    pub(super) fn ambient_tick(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let last_ptr = &mut self.ambient_ptr;
        let (moved, button, wheel, keyed, f11, tab) = ctx.input(|i| {
            let mut moved = false;
            for e in &i.events {
                match e {
                    egui::Event::PointerMoved(pos) => {
                        if last_ptr.map_or(true, |l| l.distance(*pos) >= 2.0) {
                            moved = true;
                        }
                        *last_ptr = Some(*pos);
                    }
                    egui::Event::PointerGone => *last_ptr = None,
                    _ => {}
                }
            }
            let any = |f: &dyn Fn(&egui::Event) -> bool| i.events.iter().any(|e| f(e));
            let button = any(&|e| matches!(e, egui::Event::PointerButton { .. }));
            let wheel = any(&|e| matches!(e, egui::Event::MouseWheel { .. }));
            let keyed = any(&|e| matches!(e, egui::Event::Text(_) | egui::Event::Touch { .. } | egui::Event::Key { pressed: true, .. }));
            (moved, button, wheel, keyed, i.key_pressed(egui::Key::F11), i.key_pressed(egui::Key::Tab))
        });
        let active = moved || button || wheel || keyed;
        let swallow = |ctx: &egui::Context| {
            ctx.input_mut(|i| {
                i.events.retain(|e| {
                    !matches!(
                        e,
                        egui::Event::PointerMoved(_)
                            | egui::Event::PointerButton { .. }
                            | egui::Event::MouseWheel { .. }
                            | egui::Event::Text(_)
                            | egui::Event::Touch { .. }
                            | egui::Event::Key { .. }
                            | egui::Event::Paste(_)
                            | egui::Event::Copy
                            | egui::Event::Cut
                    )
                })
            });
        };
        if self.ambient_on {
            if tab {
                self.ambient_side = (self.ambient_side + 1) % 3;
                self.ambient_ctl = now;
                swallow(ctx);
            } else if f11 || keyed {
                self.ambient_on = false;
                swallow(ctx);
            } else if moved || wheel || button {
                self.ambient_ctl = now;
            }
            self.ambient_last = now;
            return;
        }
        if f11 && !self.ambient_on && !self.ambient_blocked() {
            self.ambient_on = true;
            self.ambient_since = now;
            self.ambient_last = now;
            swallow(ctx);
            return;
        }
        if active {
            self.ambient_last = now;
            return;
        }
        if !self.cfg.ambient {
            return;
        }
        let hidden = WINDOW_HIDDEN.load(Relaxed) || ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        let wait = Duration::from_secs(self.cfg.ambient_secs.clamp(10, 3600) as u64);
        if !hidden && self.player.playing.load(Relaxed) && !self.meta.title.is_empty() && !self.ambient_blocked() && now.duration_since(self.ambient_last) >= wait {
            self.ambient_on = true;
            self.ambient_since = now;
        }
    }

    pub(super) fn ambient_overlay(&mut self, ctx: &egui::Context) {
        let t = ctx.animate_bool_with_time(Id::new("ambient_fade"), self.ambient_on, 0.5);
        self.ambient_t = t;
        if t <= 0.003 {
            return;
        }
        let ctl_age = self.ambient_ctl.elapsed().as_secs_f32();
        let ctl_a = (1.0 - (ctl_age - 2.6) / 0.6).clamp(0.0, 1.0);
        if self.ambient_on && ctl_a <= 0.0 {
            ctx.set_cursor_icon(egui::CursorIcon::None);
        }
        let e = ease(t);
        let rect = ctx.screen_rect();
        let mut p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new("ambient_layer")));
        p.set_opacity(e);
        p.rect_filled(rect, 0.0, BG0);

        let accent = accent_of(self.tint);
        {
            let top = lerp_color(BG0, self.tint, 0.34);
            let mut mesh = egui::Mesh::default();
            mesh.colored_vertex(rect.left_top(), top);
            mesh.colored_vertex(rect.right_top(), top);
            mesh.colored_vertex(rect.right_bottom(), BG0);
            mesh.colored_vertex(rect.left_bottom(), BG0);
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            p.add(Shape::mesh(mesh));
        }

        let (w, h) = (rect.width(), rect.height());
        let margin = (w * 0.07).clamp(36.0, 120.0);
        let band_h = (h * 0.26).clamp(110.0, 260.0);
        let spec_rect = Rect::from_min_max(pos2(rect.left() + margin, rect.bottom() - band_h - 26.0), pos2(rect.right() - margin, rect.bottom() - 26.0));
        let content = Rect::from_min_max(pos2(rect.left() + margin, rect.top() + margin * 0.6), pos2(rect.right() - margin, spec_rect.top() - 28.0));

        let side = if w >= 900.0 { self.ambient_side } else { 0 };
        let s = content.height().min(w * if side != 0 { 0.26 } else { 0.32 }).clamp(150.0, 560.0);
        let cover = Rect::from_min_size(pos2(content.left(), content.center().y - s * 0.5), vec2(s, s));
        for i in 0..8 {
            let g = i as f32 * 5.0;
            p.rect_filled(cover.translate(vec2(0.0, 14.0)).expand(g), Rounding::same(14.0 + g), alpha(Color32::BLACK, 16));
        }
        match &self.cover_tex {
            Some(tex) => rounded_image(&p, tex.id(), cover, 12.0, Color32::WHITE),
            None => {
                p.rect_filled(cover, Rounding::same(12.0), BG2);
                draw_icon(&p, Icon::Wave, cover.center(), s * 0.3, DIM);
            }
        }

        let cur = self.player.current();
        let shuffled = self.player.shuffle.load(Relaxed);
        let upcoming: Vec<PathBuf> = if shuffled || side != 2 {
            vec![]
        } else {
            let list = self.player.playlist.lock().unwrap();
            match cur {
                Some(i) => list.iter().skip(i + 1).take(7).cloned().collect(),
                None => vec![],
            }
        };
        let qw = (w * 0.30).clamp(300.0, 460.0);
        let qx = content.right() - qw;
        let x0 = cover.right() + 48.0;
        let tw = ((if side != 0 { qx - 40.0 } else { content.right() }) - x0).max(160.0);

        let tsize = (h * 0.058).clamp(26.0, 52.0);
        let (title, artist, album) = (self.meta.title.clone(), self.meta.artist.clone(), self.meta.album.clone());
        let line3 = match (album.is_empty(), self.meta.year.is_empty()) {
            (false, false) => format!("{album} · {}", self.meta.year),
            (false, true) => album,
            (true, false) => self.meta.year.clone(),
            (true, true) => String::new(),
        };
        let g_title = galley(&p, title, bold(tsize), Color32::WHITE, tw, 2);
        let g_artist = galley(&p, artist, body(tsize * 0.52), TEXT, tw, 2);
        let g_album = galley(&p, line3, body(tsize * 0.4), DIM, tw, 1);

        let pos = self.player.position();
        let dur = self.player.duration();
        let g_pos = galley(&p, fmt_time(pos), body(14.0), DIM, 200.0, 1);
        let g_dur = galley(&p, fmt_time(dur), body(14.0), DIM, 200.0, 1);
        let badge = self.format_badge().map(|b| galley(&p, b, body(13.0), DIM, 400.0, 1));
        let row_w = tw.min(760.0);
        let time_w = 46.0f32;
        let badge_w = badge.as_ref().map(|b| b.size().x + 18.0).unwrap_or(0.0);
        let inline_badge = badge.is_some() && row_w - 2.0 * time_w - badge_w - 24.0 >= 150.0;
        let row_h = 22.0;

        let mut total_h = g_title.size().y + 14.0 + g_artist.size().y;
        if !g_album.rows.is_empty() && g_album.size().x > 0.0 {
            total_h += 8.0 + g_album.size().y;
        }
        total_h += 34.0 + row_h;
        if badge.is_some() && !inline_badge {
            total_h += 12.0 + 18.0;
        }
        let mut y = (cover.center().y - total_h * 0.5).max(content.top());
        let mut put = |g: &Arc<egui::Galley>, y: &mut f32, gap: f32| {
            p.galley(pos2(x0, *y), g.clone(), Color32::WHITE);
            *y += g.size().y + gap;
        };
        put(&g_title, &mut y, 14.0);
        put(&g_artist, &mut y, if g_album.size().x > 0.0 { 8.0 } else { 0.0 });
        if g_album.size().x > 0.0 {
            put(&g_album, &mut y, 0.0);
        }
        y += 34.0;

        let cy = y + row_h * 0.5;
        p.galley(pos2(x0, cy - g_pos.size().y * 0.5), g_pos.clone(), DIM);
        let end_x = x0 + row_w - if inline_badge { badge_w + 16.0 } else { 0.0 };
        let bar_l = x0 + time_w;
        let bar_r = end_x - time_w;
        let bar = Rect::from_min_max(pos2(bar_l, cy - 2.0), pos2(bar_r.max(bar_l + 40.0), cy + 2.0));
        p.rect_filled(bar, Rounding::same(2.0), alpha(Color32::WHITE, 36));
        let f = if dur > 0.0 { (pos / dur).clamp(0.0, 1.0) as f32 } else { 0.0 };
        let filled = Rect::from_min_max(bar.left_top(), pos2(bar.left() + bar.width() * f, bar.bottom()));
        p.rect_filled(filled, Rounding::same(2.0), accent);
        p.circle_filled(pos2(filled.right(), cy), 5.5, Color32::WHITE);
        p.galley(pos2(bar.right() + 12.0, cy - g_dur.size().y * 0.5), g_dur.clone(), DIM);
        if let Some(b) = &badge {
            let (bx, by) = if inline_badge { (end_x + 16.0, cy) } else { (x0, cy + row_h * 0.5 + 12.0 + 9.0) };
            let br = Rect::from_min_size(pos2(bx, by - 11.0), vec2(b.size().x + 18.0, 22.0));
            p.rect_stroke(br, Rounding::same(11.0), Stroke::new(1.0, alpha(Color32::WHITE, 46)));
            p.galley(pos2(br.left() + 9.0, by - b.size().y * 0.5), b.clone(), DIM);
        }

        if side == 1 {
            self.ensure_lyrics();
            let identity = self.lyrics_identity();
            let key = identity.as_ref().map(|(t, a, _, _)| lyrics::key(a, t));
            let cached = key.as_ref().and_then(|k| self.lyr_cache.get(k)).cloned();
            let lr = Rect::from_min_max(pos2(qx, content.top() + 44.0), pos2(content.right(), content.bottom()));
            match (&cached, &key) {
                (Some(l), Some(k)) if l.found() => {
                    let (l, k) = (l.clone(), k.clone());
                    egui::Area::new(Id::new("ambient_lyrics")).order(egui::Order::Tooltip).fixed_pos(lr.min).interactable(false).show(ctx, |ui| {
                        ui.set_opacity(e);
                        let (rect, resp) = ui.allocate_exact_size(lr.size(), Sense::hover());
                        let painter = ui.painter_at(rect);
                        self.draw_lyrics(ctx, ui, &painter, rect, &resp, &l, &k);
                    });
                }
                (Some(_), _) => {
                    p.text_tr(lr.center(), Align2::CENTER_CENTER, "Bu şarkı için söz bulunamadı", bold(16.0), DIM);
                }
                (None, k) => {
                    let failed = k.as_ref().map(|k| self.lyr_failed.contains(k)).unwrap_or(false);
                    if failed {
                        p.text_tr(lr.center(), Align2::CENTER_CENTER, "Sözler alınamadı", bold(15.0), AMBER);
                    } else {
                        p.text_tr(lr.center(), Align2::CENTER_CENTER, "Sözler aranıyor…", body(14.0), DIM);
                    }
                }
            }
        }
        if side == 2 {
            self.queue_labels();
            let head = galley(&p, crate::i18n::tr("SIRADA"), bold(12.0), DIM, qw, 1);
            let top = cover.top() + 4.0;
            p.galley(pos2(qx, top), head, DIM);
            p.line_segment([pos2(qx, top + 26.0), pos2(qx + qw, top + 26.0)], Stroke::new(1.0, alpha(Color32::WHITE, 28)));
            if upcoming.is_empty() {
                let msg = if shuffled { "Karıştırma açık: sıradaki parça belli değil" } else { "Kuyruk boş" };
                p.text_tr(pos2(qx, top + 44.0), Align2::LEFT_TOP, msg, body(14.0), DIM);
            }
            let row = 52.0f32;
            let fit_n = (((cover.bottom() - top - 40.0) / row).floor() as usize).clamp(1, 7);
            for (i, path) in upcoming.iter().take(fit_n).enumerate() {
                let key = path.to_string_lossy().to_string();
                let (t, a) = self.queue_map.get(&key).cloned().unwrap_or_else(|| (path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), String::new()));
                let fade_a = 1.0 - i as f32 / (fit_n as f32 + 1.5);
                let ry = top + 38.0 + i as f32 * row;
                let tcol = alpha(TEXT, (255.0 * fade_a) as u8);
                let dcol = alpha(DIM, (255.0 * fade_a) as u8);
                p.text(pos2(qx, ry), Align2::LEFT_TOP, (i + 1).to_string(), body(12.5), dcol);
                let tx = qx + 26.0;
                let t = eqrules_ui::fit(&p, &t, bold(15.0), qw - 26.0);
                p.text(pos2(tx, ry - 2.0), Align2::LEFT_TOP, t, bold(15.0), tcol);
                if !a.is_empty() {
                    let a = eqrules_ui::fit(&p, &a, body(12.5), qw - 26.0);
                    p.text(pos2(tx, ry + 18.0), Align2::LEFT_TOP, a, body(12.5), dcol);
                }
            }
        }

        p.line_segment([pos2(spec_rect.left(), spec_rect.bottom() + 1.0), pos2(spec_rect.right(), spec_rect.bottom() + 1.0)], Stroke::new(1.0, alpha(Color32::WHITE, 30)));
        self.viz.draw_spectrum_with(&p, spec_rect, false, alpha(accent, 150), lerp_color(accent, Color32::WHITE, 0.35));
        let hint_a = (1.0 - (self.ambient_since.elapsed().as_secs_f32() - 4.0) / 1.5).clamp(0.0, 1.0);
        if hint_a > 0.0 && self.ambient_on {
            p.text_tr(pos2(rect.right() - margin, rect.bottom() - 10.0), Align2::RIGHT_BOTTOM, "Tab: sözler / sıradakiler", body(12.0), alpha(DIM, (160.0 * hint_a) as u8));
        }
        if self.ambient_on && ctl_a > 0.0 {
            let icons = [Icon::Lyrics, Icon::List, Icon::Close];
            let h = 30.0;
            let mut x = rect.right() - margin;
            let y0 = rect.top() + (margin * 0.35).max(14.0);
            let ptr = ctx.input(|i| i.pointer.hover_pos());
            let clicked = ctx.input(|i| i.pointer.primary_clicked());
            let mut hit: Option<usize> = None;
            let mut ps = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new("ambient_layer")));
            ps.set_opacity(e * ctl_a);
            for (i, icon) in icons.iter().enumerate().rev() {
                let r = Rect::from_min_max(pos2(x - h, y0), pos2(x, y0 + h));
                x = r.left() - 4.0;
                let hov = ptr.map_or(false, |q| r.contains(q));
                let on = (i == 0 && side == 1) || (i == 1 && side == 2);
                if hov {
                    ps.rect_filled(r, Rounding::same(8.0), alpha(Color32::WHITE, 20));
                }
                let col = if on { accent } else if hov { Color32::WHITE } else { alpha(TEXT, 170) };
                draw_icon(&ps, *icon, r.center(), 16.0, col);
                if on {
                    ps.circle_filled(pos2(r.center().x, r.bottom() - 1.5), 1.8, accent);
                }
                if clicked && hov {
                    hit = Some(i);
                }
            }
            match hit {
                Some(0) => self.ambient_side = if self.ambient_side == 1 { 0 } else { 1 },
                Some(1) => self.ambient_side = if self.ambient_side == 2 { 0 } else { 2 },
                Some(_) => self.ambient_on = false,
                None if clicked => self.ambient_on = false,
                None => {}
            }
        } else if self.ambient_on && ctx.input(|i| i.pointer.primary_clicked()) {
            self.ambient_on = false;
        }
        ctx.request_repaint_after(Duration::from_millis(33));
    }
}
