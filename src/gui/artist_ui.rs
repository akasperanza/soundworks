use super::*;

impl App {

    pub(super) fn ensure_artist(&mut self, name: &str, front: bool) {
        let k = artist::norm(name);
        if k == "bilinmeyen sanatçı" {
            return;
        }
        if self.artist_cache.contains_key(&k) {
            return;
        }
        if let Some(i) = artist::load_cached(name) {
            self.artist_cache.insert(k, i);
            return;
        }
        if !self.artist_failed.contains(&k) && !self.fetcher.is_pending(name) {
            self.fetcher.request(name, front);
        }
    }

    pub(super) fn artist_texture(&mut self, ctx: &egui::Context, name: &str, big: bool) -> Option<(egui::TextureId, f32, f32)> {
        let k = artist::norm(name);
        {
            let map = if big { &self.artist_big } else { &self.artist_small };
            if let Some(e) = map.get(&k) {
                return e.as_ref().map(|(t, w, h)| (t.id(), *w, *h));
            }
        }
        let photo = self.artist_cache.get(&k)?.photo.clone();
        if self.tex_budget == 0 {
            ctx.request_repaint();
            return None;
        }
        self.tex_budget -= 1;
        let tex = photo.and_then(|p| image::open(&p).ok()).map(|img| {
            let img = if big { img.thumbnail(1400, 800) } else { img.thumbnail(360, 360) };
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            (
                ctx.load_texture(
                    format!("artist_{}_{}", big as u8, k),
                    ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw()),
                    TextureOptions::LINEAR,
                ),
                w as f32,
                h as f32,
            )
        });
        let out = tex.as_ref().map(|(t, w, h)| (t.id(), *w, *h));
        if big {
            self.artist_big.insert(k, tex);
        } else {
            self.artist_small.insert(k, tex);
        }
        out
    }

    pub(super) fn artist_tracks(&self, name: &str) -> Vec<(String, String, library::LibTrack)> {
        let k = artist::norm(name);
        let mut out = vec![];
        for a in self.lib.albums.iter().filter(|a| artist::norm(&a.artist) == k) {
            for t in &a.tracks {
                out.push((a.key.clone(), a.title.clone(), t.clone()));
            }
        }
        out
    }

    pub(super) fn open_artist(&mut self, name: &str) {
        self.view = View::Library;
        self.view_t0 = Instant::now();
        self.lib_tab = 1;
        self.lib_artist = Some(name.to_string());
        self.lib_open = None;
        self.bio_open = false;
        self.artist_big.clear();
        self.ensure_artist(name, true);
    }

    pub(super) fn drive_genres(&mut self) {
        let mut changed = false;
        for (key, res) in self.genre_fetcher.poll() {
            match res {
                Some(g) => {
                    if let Some(a) = self.lib.albums.iter_mut().find(|a| a.key == key) {
                        a.genre = g;
                        a.genre_checked = true;
                        changed = true;
                    }
                }
                None => {
                    self.genre_retry.insert(key, Instant::now());
                }
            }
        }
        if self.cfg.fetch_genres {
            let queued = self.genre_fetcher.queue_len();
            if queued < 6 {
                let now = Instant::now();
                let mut jobs = vec![];
                for a in self.lib.albums.iter_mut() {
                    if a.genre_checked {
                        continue;
                    }
                    if a.artist.trim().to_lowercase() == "bilinmeyen sanatçı" || a.key.starts_with("dir|") {
                        a.genre_checked = true;
                        changed = true;
                        continue;
                    }
                    if self.genre_fetcher.is_pending(&a.key) {
                        continue;
                    }
                    if self.genre_retry.get(&a.key).map(|t| now.duration_since(*t) < std::time::Duration::from_secs(90)).unwrap_or(false) {
                        continue;
                    }
                    jobs.push((a.key.clone(), a.artist.clone(), a.title.clone()));
                    if jobs.len() + queued >= 6 {
                        break;
                    }
                }
                for (k, ar, al) in jobs {
                    self.genre_fetcher.request(k, ar, al);
                }
            }
        }
        if changed {
            self.genre_unsaved += 1;
            if self.genre_unsaved >= 8 || self.genre_fetcher.queue_len() == 0 {
                self.lib.save();
                self.genre_unsaved = 0;
            }
        }
    }

    pub(super) fn artist_grid(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let artists = self.visible_artists();
        let mut open: Option<String> = None;
        if let Some(e) = self.fetcher.last_error.lock().unwrap().clone() {
            ui.label(RichText::new(format!("Sanatçı bilgisi alınamıyor: {e}")).font(body(12.0)).color(AMBER));
        }
        if artists.is_empty() {
            self.no_match(ui, "sanatçı");
            return;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let gap = 22.0;
            let avail = ui.available_width() - 6.0;
            let cols = ((avail + gap) / (185.0 + gap)).floor().max(1.0);
            let cw = ((avail - gap * (cols - 1.0)) / cols).floor();
            let pad = 16.0;
            let rad = ((cw - 2.0 * pad) * 0.5).clamp(50.0, 96.0);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
                ui.spacing_mut().item_spacing = vec2(gap, gap);
                for (name, n_albums, n_tracks) in &artists {
                    let (nm_f, sb_f) = (bold(14.5), body(12.0));
                    let sb_txt = format!("{n_albums} albüm · {n_tracks} parça");
                    let (nm_l, sb_l, lh_n, lh_s) = {
                        let p = ui.painter().clone();
                        (wrap_lines(&p, name, &nm_f, cw - 2.0 * pad), wrap_lines(&p, &sb_txt, &sb_f, cw - 2.0 * pad), line_height(&p, &nm_f), line_height(&p, &sb_f))
                    };
                    let text_top = pad + 2.0 * rad + 14.0;
                    let card_h = text_top + nm_l.len() as f32 * lh_n + 4.0 + sb_l.len() as f32 * lh_s + pad;
                    let (rect0, resp) = ui.allocate_exact_size(vec2(cw, card_h), Sense::click());
                    let visible = ui.is_rect_visible(rect0);
                    let ap = if visible { appear(ctx, Id::new(("acard", name, self.view_epoch)), 0.0) } else { 0.0 };
                    let rect = rect0.translate(vec2(0.0, (1.0 - ap) * 14.0));
                    let prev_op = ui.opacity();
                    ui.set_opacity(prev_op * ap);
                    if visible {
                        self.ensure_artist(name, false);
                    }
                    let h = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);

                    ui.painter().rect_filled(rect, Rounding::same(16.0), lerp_color(alpha(BG2, 200), BG3, h));
                    let c = pos2(rect.center().x, rect.top() + pad + rad);
                    let r = rad + 2.0 * h;
                    let tex = if visible { self.artist_texture(ctx, name, false) } else { None };
                    match tex {
                        Some((t, w, hh)) => circle_image(ui.painter(), t, c, r, cover_uv(w, hh, Rect::from_center_size(c, egui::Vec2::splat(r * 2.0))), Color32::from_white_alpha((255.0 * ap) as u8)),
                        None => {
                            ui.painter().circle_filled(c, r, BG2);
                            let initial: String = name.chars().next().map(|ch| ch.to_uppercase().to_string()).unwrap_or_default();
                            ui.painter().text_tr(c, Align2::CENTER_CENTER, initial, bold(40.0), alpha(DIM, 170));
                        }
                    }
                    if h > 0.0 {
                        ui.painter().circle_stroke(c, r, Stroke::new(2.0_f32, alpha(BLUE, (200.0 * h) as u8)));
                    }
                    let used = draw_lines(ui.painter(), pos2(rect.center().x, rect.top() + text_top), Align2::CENTER_TOP, &nm_l, &nm_f, Color32::WHITE);
                    draw_lines(ui.painter(), pos2(rect.center().x, rect.top() + text_top + used + 4.0), Align2::CENTER_TOP, &sb_l, &sb_f, DIM);
                    ui.set_opacity(prev_op);
                    if resp.clicked() {
                        open = Some(name.clone());
                    }
                }
            });
        ui.add_space(96.0);
});
        if let Some(n) = open {
            self.open_artist(&n);
        }
    }

    pub(super) fn artist_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, name: &str) {
        self.ensure_artist(name, true);
        let key = artist::norm(name);
        let info = self.artist_cache.get(&key).cloned();
        let pending = self.fetcher.is_pending(name);
        let failed = self.artist_failed.contains(&key);
        let tracks = self.artist_tracks(name);
        let cur = self.current_path();
        let playing = self.player.playing.load(Relaxed);
        let mut back = false;
        let mut play_from: Option<usize> = None;
        let (mut open, mut play, mut remove) = (None, None, None);
        let album_ids: Vec<usize> = self
            .lib
            .albums
            .iter()
            .enumerate()
            .filter(|(_, a)| artist::norm(&a.artist) == key)
            .map(|(i, _)| i)
            .collect();
        let display = info.as_ref().map(|i| i.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| name.to_string());

        ui.horizontal(|ui| {
            if pill_button(ui, "Sanatçılar", Some(Icon::Undo), false).clicked() {
                back = true;
            }
        });
        ui.add_space(8.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {

            let w = ui.available_width() - 6.0;
            let (rect, _) = ui.allocate_exact_size(vec2(w, 280.0), Sense::hover());
            let tex = self.artist_texture(ctx, name, true);
            let ba = appear(ctx, Id::new(("banner", &key, tex.is_some(), self.view_epoch)), 0.0);
            let painter = ui.painter();
            match tex {
                Some((t, iw, ih)) => rounded_image_uv(painter, t, rect, 12.0, cover_uv(iw, ih, rect), Color32::from_white_alpha((255.0 * ba) as u8)),
                None => {
                    painter.rect_filled(rect, Rounding::same(12.0), BG2);
                    let initial: String = display.chars().next().map(|ch| ch.to_uppercase().to_string()).unwrap_or_default();
                    painter.text_tr(rect.center(), Align2::CENTER_CENTER, initial, bold(120.0), alpha(DIM, 60));
                }
            }

            vgradient(
                painter,
                Rect::from_min_max(pos2(rect.left(), rect.top() + 90.0), rect.right_bottom()),
                Color32::from_rgba_unmultiplied(9, 13, 22, 0),
                Color32::from_rgba_unmultiplied(9, 13, 22, 235),
            );
            painter.text_tr(pos2(rect.left() + 24.0, rect.bottom() - 62.0), Align2::LEFT_CENTER, &display, bold(40.0), Color32::WHITE);
            painter.text_tr(
                pos2(rect.left() + 26.0, rect.bottom() - 26.0),
                Align2::LEFT_CENTER,
                format!("{} albüm · {} parça", album_ids.len(), tracks.len()),
                body(13.0),
                alpha(Color32::WHITE, 190),
            );
            if tex.is_none() {
                let msg = if pending {
                    "Fotoğraf yükleniyor…"
                } else if failed {
                    "Fotoğraf alınamadı"
                } else {
                    ""
                };
                if !msg.is_empty() {
                    painter.text_tr(rect.left_top() + vec2(20.0, 20.0), Align2::LEFT_CENTER, msg, body(12.0), DIM);
                }
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let (pr, presp) = ui.allocate_exact_size(vec2(52.0, 52.0), Sense::click());
                let ph = ui.ctx().animate_bool_with_time(presp.id, presp.hovered(), 0.12);
                ui.painter().circle_filled(pr.center(), 26.0 * (1.0 + 0.04 * ph), lerp_color(BLUE, SKY, 0.25 * ph));
                draw_icon(ui.painter(), Icon::Play, pr.center() + vec2(1.5, 0.0), 20.0, Color32::WHITE);
                if presp.clicked() {
                    play_from = Some(0);
                }
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.add_space(8.0);
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(RichText::new("Tümünü çal").font(bold(14.0)));
                    ui.label(RichText::new(format!("{} parça", tracks.len())).font(body(12.0)).color(DIM));
                });
            });
            ui.add_space(14.0);

            section_label(ui, "Hakkında");
            ui.add_space(2.0);
            match &info {
                Some(i) if !i.description.trim().is_empty() => {
                    let text = i.description.trim();
                    let long = text.chars().count() > 420;
                    let shown: String = if long && !self.bio_open { text.chars().take(420).collect::<String>() + "…" } else { text.to_string() };
                    ui.add(egui::Label::new(RichText::new(shown).font(body(14.0)).color(TEXT)).wrap());
                    if long && ui.add(egui::Label::new(RichText::new(if self.bio_open { "Daha az göster" } else { "Devamını oku" }).font(bold(12.5)).color(SKY)).sense(Sense::click())).clicked() {
                        self.bio_open = !self.bio_open;
                    }
                }
                Some(_) => {
                    ui.label(RichText::new("Bu sanatçı için açıklama bulunamadı.").font(body(13.0)).color(DIM));
                }
                None if pending => {
                    ui.label(RichText::new("Bilgi alınıyor…").font(body(13.0)).color(DIM));
                }
                None => {
                    let err = self.fetcher.last_error.lock().unwrap().clone().unwrap_or_default();
                    ui.label(RichText::new(format!("Bilgi alınamadı. {err}")).font(body(13.0)).color(AMBER));
                }
            }

            ui.add_space(14.0);
            section_label(ui, "Parçalar");
            ui.add_space(4.0);
            for (i, (_, album_title, t)) in tracks.iter().enumerate() {
                let (tf, af) = (body(13.5), body(12.0));
                let (tf_cur, row_w) = (bold(13.5), w - 6.0);
                let (alb_l, ttl_l, lh_t, lh_a) = {
                    let p = ui.painter().clone();
                    let alb_l = wrap_lines(&p, album_title, &af, (row_w * 0.30).max(120.0));
                    let aw = alb_l.iter().map(|l| text_width(&p, l, &af)).fold(0.0, f32::max);
                    let title_w = (row_w - 40.0 - 90.0 - aw - 16.0).max(100.0);
                    let tfont = if cur.as_ref() == Some(&t.path) { &tf_cur } else { &tf };
                    (alb_l, wrap_lines(&p, &t.title, tfont, title_w), line_height(&p, &tf), line_height(&p, &af))
                };
                let row_h = (ttl_l.len() as f32 * lh_t).max(alb_l.len() as f32 * lh_a) + 16.0;
                let (rect0, resp) = ui.allocate_exact_size(vec2(row_w, row_h.max(34.0)), Sense::click());
                let ap = appear(ctx, Id::new(("trow", &t.path, self.view_epoch)), i.min(14) as f32 * 0.025);
                let rect = rect0.translate(vec2((1.0 - ap) * -12.0, 0.0));
                let prev_op = ui.opacity();
                ui.set_opacity(prev_op * ap);
                let h = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1);
                let is_cur = cur.as_ref() == Some(&t.path);
                let p = ui.painter();
                if is_cur {
                    p.rect_filled(rect, Rounding::same(7.0), alpha(BLUE, 34));
                } else if h > 0.0 {
                    p.rect_filled(rect, Rounding::same(7.0), alpha(BG3, (110.0 * h) as u8));
                }
                if is_cur {
                    let r = Rect::from_min_size(pos2(rect.left() + 10.0, rect.center().y - 8.0), vec2(14.0, 16.0));
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::left_to_right(egui::Align::Center)));
                    playing_bars(&mut child, playing, vec2(14.0, 16.0));
                } else {
                    ui.painter().text_tr(pos2(rect.left() + 16.0, rect.center().y), Align2::CENTER_CENTER, (i + 1).to_string(), body(12.5), DIM);
                }
                let tfont = if is_cur { bold(13.5) } else { body(13.5) };
                let ty = rect.center().y - ttl_l.len() as f32 * lh_t * 0.5;
                draw_lines(ui.painter(), pos2(rect.left() + 40.0, ty), Align2::LEFT_TOP, &ttl_l, &tfont, if is_cur { Color32::WHITE } else { TEXT });
                let ay = rect.center().y - alb_l.len() as f32 * lh_a * 0.5;
                draw_lines(ui.painter(), pos2(rect.right() - 90.0, ay), Align2::RIGHT_TOP, &alb_l, &af, DIM);
                let d = t.duration as u64;
                ui.painter().text_tr(pos2(rect.right() - 12.0, rect.center().y), Align2::RIGHT_CENTER, format!("{}:{:02}", d / 60, d % 60), body(12.5), DIM);
                ui.set_opacity(prev_op);
                let tpath = PathBuf::from(&t.path);
                let mut qreq: Option<bool> = None;
                resp.context_menu(|ui| {
                    if ui.button_tr("Sıradaki olarak ekle").clicked() {
                        qreq = Some(true);
                        ui.close_menu();
                    }
                    if ui.button_tr("Sıraya ekle").clicked() {
                        qreq = Some(false);
                        ui.close_menu();
                    }
                });
                if let Some(next) = qreq {
                    self.pending_queue = Some((vec![tpath], next));
                }
                if resp.clicked() {
                    play_from = Some(i);
                }
            }
            if tracks.is_empty() {
                ui.label(RichText::new("Bu sanatçıdan kütüphanede parça yok.").font(body(13.0)).color(DIM));
            }

            if !album_ids.is_empty() {
                ui.add_space(16.0);
                section_label(ui, "Albümler");
                ui.add_space(6.0);
                let gap = 26.0;
                let avail = ui.available_width() - 6.0;
                let cols = ((avail + gap) / (170.0 + gap)).floor().max(1.0);
                let cw = ((avail - gap * (cols - 1.0)) / cols).floor();
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(gap, gap);
                    for i in &album_ids {
                        let (o, p, r) = self.album_card(ui, ctx, *i, cw, &cur);
                        open = open.take().or(o);
                        play = play.take().or(p);
                        remove = remove.take().or(r);
                    }
                });
            }
            ui.add_space(20.0);
        ui.add_space(96.0);
});
        if back {
            self.lib_artist = None;
            self.artist_big.clear();
        }
        if let Some(i) = play_from {
            let files: Vec<PathBuf> = tracks.iter().map(|(_, _, t)| PathBuf::from(&t.path)).collect();
            self.play_paths(files, i);
        }
        self.apply_album_actions(open, play, remove);
    }
}
