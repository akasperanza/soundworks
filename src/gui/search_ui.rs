//! Genel arama katmanı.

use super::*;

impl App {
    pub(super) fn open_search(&mut self) {
        self.search_open = true;
        self.search_t0 = Instant::now();
        self.search_focus = true;
        self.search_query.clear();
        self.search_sel = 0;
    }

    pub(super) fn search_items(&self) -> Vec<SearchItem> {
        let q = self.search_query.trim().to_lowercase();
        let mut out = vec![];
        if q.is_empty() {
            return out;
        }
        for (name, _, _) in self.lib.artists().into_iter().filter(|(n, _, _)| n.to_lowercase().contains(&q)).take(4) {
            out.push(SearchItem::Artist(name));
        }
        for a in self
            .lib
            .albums
            .iter()
            .filter(|a| a.title.to_lowercase().contains(&q) || a.artist.to_lowercase().contains(&q))
            .take(6)
        {
            out.push(SearchItem::Album(a.key.clone()));
        }
        let mut n = 0;
        'outer: for a in &self.lib.albums {
            for (i, t) in a.tracks.iter().enumerate() {
                if t.title.to_lowercase().contains(&q) {
                    out.push(SearchItem::Track(a.key.clone(), i));
                    n += 1;
                    if n >= 8 {
                        break 'outer;
                    }
                }
            }
        }
        out
    }

    /// Arama sonucu satırının (başlık, alt başlık) metni.
    pub(super) fn search_texts(&self, it: &SearchItem) -> (String, String) {
        match it {
            SearchItem::Artist(name) => (name.clone(), "Sanatçı".to_string()),
            SearchItem::Album(key) => {
                let (t, a) = self.lib.albums.iter().find(|a| &a.key == key).map(|a| (a.title.clone(), a.artist.clone())).unwrap_or_default();
                (t, format!("Albüm · {a}"))
            }
            SearchItem::Track(key, i) => self
                .lib
                .albums
                .iter()
                .find(|a| &a.key == key)
                .map(|a| (a.tracks.get(*i).map(|t| t.title.clone()).unwrap_or_default(), format!("{} · {}", a.artist, a.title)))
                .unwrap_or_default(),
        }
    }

    pub(super) fn search_activate(&mut self, item: SearchItem) {
        match item {
            SearchItem::Artist(name) => self.open_artist(&name),
            SearchItem::Album(key) => {
                self.view = View::Library;
                self.view_t0 = Instant::now();
                self.lib_artist = None;
                self.lib_open = Some(key);
            }
            SearchItem::Track(key, idx) => self.play_album(&key, idx),
        }
        self.search_open = false;
    }

    pub(super) fn search_overlay(&mut self, ctx: &egui::Context) {
        if !self.search_open {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.search_open = false;
            return;
        }
        let items = self.search_items();
        let n = items.len();
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) && n > 0 {
            self.search_sel = (self.search_sel + 1) % n;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) && n > 0 {
            self.search_sel = (self.search_sel + n - 1) % n;
        }
        if self.search_sel >= n.max(1) {
            self.search_sel = 0;
        }
        let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
        let screen = ctx.screen_rect();
        let mut chosen: Option<usize> = None;

        // arka plan ve panel TEK katmanda: panel her zaman gri örtünün üstünde kalır
        let mut close = false;
        let w = 660.0f32.min(screen.width() - 60.0);
        let e = ease(self.search_t0.elapsed().as_secs_f32() / 0.18);
        if e < 1.0 {
            ctx.request_repaint();
        }
        let panel_rect = Rect::from_min_size(pos2(screen.center().x - w * 0.5, 70.0 - (1.0 - e) * 14.0), vec2(w, (screen.height() - 100.0).max(220.0)));
        egui::Area::new(Id::new("search_overlay"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                ui.set_opacity(e);
                let bd = ui.allocate_rect(screen, Sense::click());
                ui.painter().rect_filled(screen, 0.0, Color32::from_rgba_unmultiplied(78, 82, 92, 214));
                let mut child = ui.child_ui(panel_rect, egui::Layout::top_down(egui::Align::Min), None);
                let ui = &mut child;
                let inner = Frame::none()
                    .fill(BG1)
                    .stroke(Stroke::new(1.0, STROKE))
                    .rounding(Rounding::same(12.0))
                    .inner_margin(Margin::same(14.0))
                    .shadow(egui::epaint::Shadow { offset: vec2(0.0, 12.0), blur: 30.0, spread: 0.0, color: Color32::from_black_alpha(120) })
                    .show(ui, |ui| {
                        ui.set_width(w - 28.0);
                        ui.horizontal(|ui| {
                            let (ir, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                            draw_icon(ui.painter(), Icon::Search, ir.center(), 16.0, DIM);
                            let te = ui.add(
                                egui::TextEdit::singleline(&mut self.search_query)
                                    .hint_text("Sanatçı, albüm ya da parça ara…")
                                    .font(body(18.0))
                                    .frame(false)
                                    .desired_width((w - 28.0 - 44.0).max(120.0)),
                            );
                            if self.search_focus {
                                te.request_focus();
                                self.search_focus = false;
                            }
                            if te.changed() {
                                self.search_sel = 0;
                            }
                        });
                        ui.add_space(6.0);
                        ui.separator();
                        if self.search_query.trim().is_empty() {
                            ui.add_space(6.0);
                            ui.label(RichText::new("Yazmaya başla · Enter: aç · Esc: kapat").font(body(12.5)).color(DIM));
                        } else if items.is_empty() {
                            ui.add_space(6.0);
                            ui.label(RichText::new("Sonuç yok").font(body(13.0)).color(DIM));
                        }
                        egui::ScrollArea::vertical().max_height((screen.height() - 220.0).max(160.0)).auto_shrink([true, true]).show(ui, |ui| {
                            let mut last_kind = 9;
                            for (idx, it) in items.iter().enumerate() {
                                let kind = match it {
                                    SearchItem::Artist(_) => 0,
                                    SearchItem::Album(_) => 1,
                                    SearchItem::Track(..) => 2,
                                };
                                if kind != last_kind {
                                    last_kind = kind;
                                    ui.add_space(6.0);
                                    section_label(ui, ["Sanatçılar", "Albümler", "Parçalar"][kind]);
                                    ui.add_space(2.0);
                                }
                                let (title0, sub0) = self.search_texts(it);
                                let row_w = ui.available_width();
                                let (tfont, tl, sfont, sl, lh_t, lh_s) = {
                                    let p = ui.painter().clone();
                                    let tw = (row_w - 54.0 - 12.0).max(100.0);
                                    let (tfont, tl) = fit_wrap(&p, &title0, bold, &[13.5, 12.5], tw, 2);
                                    let (sfont, sl) = fit_wrap(&p, &sub0, body, &[12.0, 11.0], tw, 2);
                                    let (lh_t, lh_s) = (line_height(&p, &tfont), line_height(&p, &sfont));
                                    (tfont, tl, sfont, sl, lh_t, lh_s)
                                };
                                let row_h = (12.0 + tl.len() as f32 * lh_t + 2.0 + sl.len() as f32 * lh_s).max(44.0);
                                let (rect, resp) = ui.allocate_exact_size(vec2(row_w, row_h), Sense::click());
                                let sel = self.search_sel == idx;
                                if resp.hovered() && ui.input(|i| i.pointer.delta().length() > 0.0) {
                                    self.search_sel = idx;
                                }
                                if sel {
                                    ui.painter().rect_filled(rect, Rounding::same(8.0), alpha(BLUE, 40));
                                }
                                let thumb = Rect::from_center_size(pos2(rect.left() + 26.0, rect.center().y), vec2(34.0, 34.0));
                                match it {
                                    SearchItem::Artist(name) => {
                                        self.ensure_artist(name, false);
                                        match self.artist_texture(ctx, name, false) {
                                            Some((t, iw, ih)) => circle_image(ui.painter(), t, thumb.center(), 17.0, cover_uv(iw, ih, thumb), Color32::WHITE),
                                            None => {
                                                ui.painter().circle_filled(thumb.center(), 17.0, BG2);
                                                draw_icon(ui.painter(), Icon::User, thumb.center(), 14.0, DIM);
                                            }
                                        }
                                    }
                                    SearchItem::Album(key) => {
                                        match self.album_tex(ctx, key) {
                                            Some(tx) => rounded_image(ui.painter(), tx, thumb, 5.0, Color32::WHITE),
                                            None => {
                                                ui.painter().rect_filled(thumb, Rounding::same(5.0), BG2);
                                                draw_icon(ui.painter(), Icon::Wave, thumb.center(), 14.0, DIM);
                                            }
                                        }
                                    }
                                    SearchItem::Track(key, i) => {
                                        let _ = i;
                                        match self.album_tex(ctx, key) {
                                            Some(tx) => rounded_image(ui.painter(), tx, thumb, 5.0, Color32::WHITE),
                                            None => {
                                                ui.painter().rect_filled(thumb, Rounding::same(5.0), BG2);
                                                draw_icon(ui.painter(), Icon::Wave, thumb.center(), 14.0, DIM);
                                            }
                                        }
                                    }
                                };
                                let top = rect.center().y - (tl.len() as f32 * lh_t + 2.0 + sl.len() as f32 * lh_s) * 0.5;
                                draw_lines(ui.painter(), pos2(rect.left() + 54.0, top), Align2::LEFT_TOP, &tl, &tfont, TEXT);
                                draw_lines(ui.painter(), pos2(rect.left() + 54.0, top + tl.len() as f32 * lh_t + 2.0), Align2::LEFT_TOP, &sl, &sfont, DIM);
                                if resp.clicked() {
                                    chosen = Some(idx);
                                }
                            }
                        });
                    });
                let fr = inner.response.rect;
                if bd.clicked() {
                    if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                        if !fr.contains(p) {
                            close = true;
                        }
                    }
                }
            });
        if enter && n > 0 && chosen.is_none() {
            chosen = Some(self.search_sel.min(n - 1));
        }
        if let Some(i) = chosen {
            if let Some(it) = items.into_iter().nth(i) {
                self.search_activate(it);
            }
        }
        if close {
            self.search_open = false;
        }
    }
}
