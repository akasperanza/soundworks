//! Kütüphane: tarama, albüm ızgarası ve albüm sayfası, klasör yönetimi.

use super::*;

impl App {
    /// Eksik dosya denetimini arka planda başlatır.
    pub(super) fn start_missing_check(&mut self) {
        if self.missing_rx.is_none() && self.lib.total_tracks() > 0 {
            self.missing_rx = Some(library::spawn_missing_check(self.lib.all_paths()));
        }
    }

    /// Kütüphaneyi yeniden denetler: eksik dosyaları bulur, `scan_new` ise izlenen klasörlerdeki yeni dosyaları da tarar.
    pub(super) fn rescan_library(&mut self, scan_new: bool) {
        self.start_missing_check();
        if scan_new && !self.lib.folders.is_empty() {
            let known: HashSet<String> = self.lib.all_paths().into_iter().collect();
            self.lib_scans.push((library::spawn_folder_scan(self.lib.folders.clone(), known), 0, 0));
        }
    }

    /// Klasörü kütüphaneye ekler ve izleme listesine alır; dosyalar arka planda taranır.
    pub(super) fn add_library_folder(&mut self, dir: PathBuf) {
        self.lib.add_folder(&dir.to_string_lossy());
        self.lib.save();
        let known: HashSet<String> = self.lib.all_paths().into_iter().collect();
        self.lib_scans.push((library::spawn_folder_scan(vec![dir.to_string_lossy().to_string()], known), 0, 0));
    }

    /// Bulunamayan parçaları kütüphaneden çıkarır.
    pub(super) fn clean_missing(&mut self) {
        let n = self.lib.remove_missing(&self.lib_missing);
        self.lib_missing.clear();
        self.lib.save();
        self.lib_tex.retain(|_, v| v.is_some());
        self.say(if n > 0 { format!("{n} eksik parça kütüphaneden çıkarıldı") } else { "Eksik parça yok".to_string() });
    }

    pub(super) fn lib_add_paths(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        let known: HashSet<String> =
            self.lib.albums.iter().flat_map(|a| a.tracks.iter().map(|t| t.path.clone())).collect();
        let total = paths.len();
        self.lib_scans.push((library::spawn_scan(paths, known), 0, total));
    }

    pub(super) fn poll_scans(&mut self) {
        let mut finished = vec![];
        let mut added = 0usize;
        for (i, (rx, done, total)) in self.lib_scans.iter_mut().enumerate() {
            while let Ok(m) = rx.try_recv() {
                match m {
                    ScanMsg::Track(t) => {
                        if self.lib.add(&t) {
                            added += 1;
                        }
                    }
                    ScanMsg::Progress(d, t) => {
                        *done = d;
                        *total = t;
                    }
                    ScanMsg::Done => finished.push(i),
                }
            }
        }
        if added > 0 {
            self.lib.sort();
            self.lib_added += added;
            self.lib_tex.retain(|_, v| v.is_some());
        }
        if !finished.is_empty() {
            for i in finished.into_iter().rev() {
                self.lib_scans.remove(i);
            }
            if self.lib_scans.is_empty() {
                self.lib.save();
                self.start_missing_check();
                let n = std::mem::take(&mut self.lib_added);
                self.lib_tex.retain(|_, v| v.is_some());
                self.say(if n > 0 { format!("{n} parça eklendi") } else { "Yeni parça yok".to_string() });
            }
        }
        if let Some(rx) = &self.missing_rx {
            if let Ok(m) = rx.try_recv() {
                self.lib_missing = m;
                self.missing_rx = None;
            }
        }
        // sanatçı bilgileri
        for (name, info) in self.fetcher.poll() {
            let k = artist::norm(&name);
            match info {
                Some(i) => {
                    self.artist_cache.insert(k.clone(), i);
                    self.artist_failed.remove(&k);
                    self.artist_small.remove(&k);
                    self.artist_big.remove(&k);
                }
                None => {
                    self.artist_failed.insert(k);
                }
            }
        }
    }

    pub(super) fn album_tex(&mut self, ctx: &egui::Context, key: &str) -> Option<egui::TextureId> {
        if let Some(e) = self.lib_tex.get(key) {
            return e.as_ref().map(|t| t.id());
        }
        if self.tex_budget == 0 {
            ctx.request_repaint();
            return None;
        }
        self.tex_budget -= 1;
        let tex = image::open(library::cover_path(key)).ok().map(|img| {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            ctx.load_texture(
                format!("album_{}", self.lib_tex.len()),
                ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw()),
                TextureOptions::LINEAR,
            )
        });
        let id = tex.as_ref().map(|t| t.id());
        self.lib_tex.insert(key.to_string(), tex);
        id
    }

    pub(super) fn play_paths(&mut self, files: Vec<PathBuf>, start: usize) {
        if files.is_empty() {
            return;
        }
        if self.cfg.use_loopback {
            self.loopback_notice();
            return;
        }
        if self.dsd_gate(&files, start) {
            return;
        }
        // diskte olmayan dosyalar kuyruğa girmesin; başlangıç sırası buna göre kayar
        let total = files.len();
        let skipped_before_start = files.iter().take(start).filter(|f| self.lib_missing.contains(&f.to_string_lossy().to_string())).count();
        let files: Vec<PathBuf> = files.into_iter().filter(|f| !self.lib_missing.contains(&f.to_string_lossy().to_string())).collect();
        if files.is_empty() {
            self.say("Dosyalar bulunamadı: kütüphaneyi yeniden tara ya da eksikleri temizle");
            return;
        }
        let skipped = total - files.len();
        let start = start.saturating_sub(skipped_before_start).min(files.len() - 1);
        self.player.clear();
        self.player.add(files);
        self.sync_playlist();
        self.play_index(start);
        if skipped > 0 {
            self.say(format!("{skipped} eksik parça atlandı"));
        }
    }

    pub(super) fn play_album(&mut self, key: &str, start: usize) {
        let Some(al) = self.lib.albums.iter().find(|a| a.key == key) else { return };
        let files = al.paths();
        self.play_paths(files, start);
    }

    pub(super) fn current_path(&self) -> Option<String> {
        let i = self.player.current()?;
        self.player.playlist.lock().unwrap().get(i).map(|p| p.to_string_lossy().to_string())
    }

    /// Panelin üstünden inen kapak rengi.
    pub(super) fn paint_glow(&self, ui: &egui::Ui, inner: Margin) {
        let r = ui.max_rect();
        let panel = Rect::from_min_max(pos2(r.left() - inner.left, r.top() - inner.top), pos2(r.right() + inner.right, r.bottom() + inner.bottom));
        panel_glow(ui.painter(), panel, 14.0, 420.0, self.backdrop, self.backdrop_a * 0.7);
    }

    pub(super) fn library_view(&mut self, ctx: &egui::Context, fade: f32) {
        let inner = Margin { left: 40.0, right: 40.0, top: 20.0, bottom: 20.0 };
        egui::CentralPanel::default()
            .frame(panel_frame(inner, Margin { left: 12.0, right: 12.0, top: 2.0, bottom: 2.0 }))
            .show(ctx, |ui| {
                self.paint_glow(ui, inner);
                let sig = (self.lib_open.clone(), self.lib_artist.clone(), self.lib_tab);
                if sig != self.nav_sig {
                    self.nav_sig = sig;
                    self.nav_t0 = Instant::now();
                    self.view_epoch += 1;
                }
                let nf = ease(self.nav_t0.elapsed().as_secs_f32() / 0.24);
                if nf < 1.0 {
                    ctx.request_repaint();
                }
                ui.set_opacity(fade * nf);
                if let Some(key) = self.lib_open.clone() {
                    if self.lib.albums.iter().any(|a| a.key == key) {
                        self.album_detail(ui, ctx, &key);
                        return;
                    }
                    self.lib_open = None;
                }
                if let Some(name) = self.lib_artist.clone() {
                    self.artist_page(ui, ctx, &name);
                    return;
                }
                self.library_home(ui, ctx);
            });
    }

    pub(super) fn library_home(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let scanning: Option<(usize, usize)> = if self.lib_scans.is_empty() {
            None
        } else {
            Some((self.lib_scans.iter().map(|s| s.1).sum(), self.lib_scans.iter().map(|s| s.2).sum()))
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            ui.label(
                RichText::new(format!("{} albüm · {} parça", self.lib.albums.len(), self.lib.total_tracks()))
                    .font(body(13.5))
                    .color(DIM),
            );
            if let Some((d, t)) = scanning {
                ui.label(RichText::new(format!("Taranıyor {d}/{t}")).font(body(12.5)).color(AMBER));
            }
            if self.cfg.fetch_genres {
                let total = self.lib.albums.len();
                let done = self.lib.albums.iter().filter(|a| a.genre_checked).count();
                if done < total {
                    ui.label(RichText::new(format!("Türler alınıyor {done}/{total}")).font(body(12.5)).color(AMBER));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if scanning.is_none() && pill_button(ui, "Yeniden tara", Some(Icon::Undo), false)
                    .on_hover_text("Dosyaların hâlâ yerinde olup olmadığına bakar ve izlenen klasörlerdeki yeni dosyaları ekler")
                    .clicked()
                {
                    self.rescan_library(true);
                    self.say("Kütüphane denetleniyor");
                }
                if !self.lib_missing.is_empty() {
                    let label = format!("Eksikleri temizle ({})", self.lib_missing.len());
                    if pill_button(ui, &label, Some(Icon::Trash), false).on_hover_text("Diskte bulunamayan parçaları kütüphaneden çıkarır").clicked() {
                        self.clean_missing();
                    }
                }
            });
        });
        ui.add_space(12.0);
        if !self.lib.albums.is_empty() {
            self.library_filter_bar(ui);
        }
        ui.add_space(14.0);

        if self.lib.albums.is_empty() {
            ui.add_space(70.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("Kütüphane boş").font(bold(17.0)).color(DIM));
                ui.label(RichText::new("Sol üstteki ok düğmesinden Yükle > Klasör ya da Parça seç; dosyaları buraya da sürükleyebilirsin.").font(body(13.0)).color(DIM));
                ui.label(RichText::new("Aynı albümün parçaları otomatik olarak aynı albümde toplanır.").font(body(12.5)).color(DIM));
            });
            return;
        }
        if self.lib_tab == 0 {
            self.album_grid(ui, ctx);
        } else {
            self.artist_grid(ui, ctx);
        }
    }

    /// Filtre çubuğu: metin, tür ve sıralama. Albümler ve sanatçılar aynı filtreyi paylaşır; sıralama sekmeye göre değişir.
    fn library_filter_bar(&mut self, ui: &mut egui::Ui) {
        const ALBUM_SORTS: [&str; 4] = ["A → Z", "Z → A", "Sanatçıya göre", "Süreye göre"];
        const ARTIST_SORTS: [&str; 4] = ["A → Z", "Z → A", "Çok albümlü önce", "Çok parçalı önce"];
        let mut genres: Vec<String> = self.lib.albums.iter().filter(|a| !a.genre.is_empty()).map(|a| a.genre.clone()).collect();
        genres.sort();
        genres.dedup();
        if let Some(g) = &self.lib_genre {
            if !genres.contains(g) {
                self.lib_genre = None;
            }
        }
        let tab = self.lib_tab.min(1) as usize;
        let active = !self.lib_q.trim().is_empty() || self.lib_genre.is_some();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.spacing_mut().interact_size.y = 34.0; // kutu, menüler ve anahtar aynı yükseklikte
            let hint = if tab == 0 { "Albüm ya da sanatçı filtrele…" } else { "Sanatçı filtrele…" };
            ui.add(egui::TextEdit::singleline(&mut self.lib_q).hint_text(hint).desired_width(230.0).margin(vec2(10.0, 7.0)));
            if !genres.is_empty() {
                let shown = self.lib_genre.clone().unwrap_or_else(|| "Tüm türler".to_string());
                egui::ComboBox::from_id_salt("lib_genre").width(150.0).selected_text(shown).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.lib_genre, None, "Tüm türler");
                    for g in &genres {
                        ui.selectable_value(&mut self.lib_genre, Some(g.clone()), g);
                    }
                });
            }
            let names = if tab == 0 { ALBUM_SORTS } else { ARTIST_SORTS };
            let mut sort = self.lib_sort[tab];
            egui::ComboBox::from_id_salt(("lib_sort", tab)).width(150.0).selected_text(format!("Sırala: {}", names[sort as usize % 4])).show_ui(ui, |ui| {
                for (i, n) in names.iter().enumerate() {
                    ui.selectable_value(&mut sort, i as u8, *n);
                }
            });
            self.lib_sort[tab] = sort;
            if active && pill_button(ui, "Filtreyi temizle", Some(Icon::Close), false).clicked() {
                self.lib_q.clear();
                self.lib_genre = None;
            }
            // görünüm anahtarı satırın sağ ucunda: filtre/tür/sıralama solda gruplu kalır
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut tab_v = self.lib_tab;
                view_switch(ui, &mut tab_v, &[(0u8, "Albümler"), (1u8, "Sanatçılar")]);
                self.lib_tab = tab_v;
            });
        });
    }

    /// Filtreden geçen albümlerin indeksleri, seçili sıralamayla.
    fn visible_albums(&self) -> Vec<usize> {
        let q = self.lib_q.trim().to_lowercase();
        let mut v: Vec<usize> = (0..self.lib.albums.len())
            .filter(|&i| {
                let a = &self.lib.albums[i];
                (q.is_empty() || a.title.to_lowercase().contains(&q) || a.artist.to_lowercase().contains(&q))
                    && self.lib_genre.as_ref().map_or(true, |g| &a.genre == g)
            })
            .collect();
        let al = &self.lib.albums;
        match self.lib_sort[0] {
            1 => v.sort_by_key(|&i| std::cmp::Reverse(al[i].title.to_lowercase())),
            2 => v.sort_by_key(|&i| (al[i].artist.to_lowercase(), al[i].title.to_lowercase())),
            3 => v.sort_by_key(|&i| std::cmp::Reverse(al[i].total_secs() as u64)),
            _ => v.sort_by_key(|&i| al[i].title.to_lowercase()),
        }
        v
    }

    /// Filtreden geçen sanatçılar (ad, albüm sayısı, parça sayısı), seçili sıralamayla.
    pub(super) fn visible_artists(&self) -> Vec<(String, usize, usize)> {
        let q = self.lib_q.trim().to_lowercase();
        let mut v: Vec<(String, usize, usize)> = self
            .lib
            .artists()
            .into_iter()
            .filter(|(name, _, _)| {
                (q.is_empty() || name.to_lowercase().contains(&q))
                    && self.lib_genre.as_ref().map_or(true, |g| {
                        self.lib.albums.iter().any(|a| a.artist.trim().eq_ignore_ascii_case(name.trim()) && &a.genre == g)
                    })
            })
            .collect();
        match self.lib_sort[1] {
            1 => v.sort_by_key(|x| std::cmp::Reverse(x.0.to_lowercase())),
            2 => v.sort_by_key(|x| (std::cmp::Reverse(x.1), x.0.to_lowercase())),
            3 => v.sort_by_key(|x| (std::cmp::Reverse(x.2), x.0.to_lowercase())),
            _ => v.sort_by_key(|x| x.0.to_lowercase()),
        }
        v
    }

    /// Filtre hiçbir şeyle eşleşmediğinde gösterilen boş durum.
    pub(super) fn no_match(&mut self, ui: &mut egui::Ui, what: &str) {
        ui.add_space(60.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(format!("Filtreyle eşleşen {what} yok")).font(bold(16.0)).color(DIM));
            ui.add_space(8.0);
            if pill_button(ui, "Filtreyi temizle", Some(Icon::Close), false).clicked() {
                self.lib_q.clear();
                self.lib_genre = None;
            }
        });
    }

    /// Albüm kartı. (aç, çal, kaldır)
    pub(super) fn album_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, i: usize, cw: f32, cur: &Option<String>) -> (Option<String>, Option<String>, Option<String>) {
        let (key, title, artist, n, is_cur) = {
            let a = &self.lib.albums[i];
            (
                a.key.clone(),
                a.title.clone(),
                a.artist.clone(),
                a.tracks.len(),
                cur.as_ref().map(|c| a.tracks.iter().any(|t| &t.path == c)).unwrap_or(false),
            )
        };
        let tex = self.album_tex(ctx, &key);
        let (name_f, sub_f) = (body(14.0), body(11.5));
        let miss = self.lib.albums[i].tracks.iter().filter(|t| self.lib_missing.contains(&t.path)).count();
        let sub_txt = if miss == 0 {
            format!("{artist} · {n}")
        } else if miss == n {
            format!("{artist} · dosyalar bulunamadı")
        } else {
            format!("{artist} · {n} ({miss} eksik)")
        };
        let (name_l, sub_l, lh_n, lh_s) = {
            let p = ui.painter().clone();
            (wrap_lines(&p, &title, &name_f, cw), wrap_lines(&p, &sub_txt, &sub_f, cw), line_height(&p, &name_f), line_height(&p, &sub_f))
        };
        let text_h = (8.0 + name_l.len() as f32 * lh_n + 3.0 + sub_l.len() as f32 * lh_s + 6.0).max(46.0);
        let (rect0, resp) = ui.allocate_exact_size(vec2(cw, cw + text_h), Sense::click());
        // düğme kartın üstüne binince kartın kendi `hovered()` değeri düşer ve animasyon titrer: imleç dikdörtgenin içinde mi diye bak
        let hov = ui.rect_contains_pointer(rect0);
        let h = ui.ctx().animate_bool_with_time(resp.id, hov, 0.12);
        let ap = appear(ctx, Id::new(("card", &key, self.view_epoch)), (i % 10) as f32 * 0.03);
        let rect = rect0.translate(vec2(0.0, (1.0 - ap) * 16.0 - 3.0 * h));
        let prev_op = ui.opacity();
        ui.set_opacity(prev_op * ap);
        let cover = Rect::from_min_size(rect.min, vec2(cw, cw));
        let painter = ui.painter();
        // Surface 2: üzerine gelince kart, panelden (BG1) bir kat yukarı kalkar
        if h > 0.0 {
            painter.rect_filled(rect.expand(10.0), Rounding::same(16.0), alpha(BG2, (255.0 * h) as u8));
        }
        match tex {
            Some(t) => rounded_image(painter, t, cover, 12.0, Color32::from_white_alpha((255.0 * ap) as u8)),
            None => {
                painter.rect_filled(cover, Rounding::same(12.0), BG2);
                draw_icon(painter, Icon::Wave, cover.center(), 34.0, alpha(DIM, 160));
            }
        }
        if is_cur {
            painter.rect_stroke(cover, Rounding::same(12.0), Stroke::new(2.0, BLUE));
        }
        if miss == n {
            painter.rect_filled(cover, Rounding::same(12.0), Color32::from_black_alpha(150));
        }
        if h > 0.0 {
            painter.rect_filled(cover, Rounding::same(12.0), Color32::from_black_alpha((70.0 * h) as u8));
        }
        let used = draw_lines(painter, pos2(rect.left(), rect.top() + cw + 8.0), Align2::LEFT_TOP, &name_l, &name_f, if miss == n { DIM } else { Color32::WHITE });
        draw_lines(painter, pos2(rect.left(), rect.top() + cw + 8.0 + used + 3.0), Align2::LEFT_TOP, &sub_l, &sub_f, DIM);
        let pr = Rect::from_center_size(cover.right_bottom() - vec2(26.0, 26.0), vec2(36.0, 36.0));
        let mut over_play = false;
        let (mut open, mut play, mut remove) = (None, None, None);
        if hov || h > 0.01 {
            let presp = ui.interact(pr, resp.id.with("play"), Sense::click());
            over_play = presp.hovered();
            let pc = pr.center();
            ui.painter().circle_filled(pc, 18.0 * (0.85 + 0.15 * h), lerp_color(BLUE, SKY, over_play as i32 as f32 * 0.3));
            draw_icon(ui.painter(), Icon::Play, pc + vec2(1.0, 0.0), 13.0, Color32::WHITE);
            if presp.clicked() {
                play = Some(key.clone());
            }
        }
        if resp.clicked() && !over_play {
            open = Some(key.clone());
        }
        let album_paths = self.lib.albums[i].paths();
        let mut queue_req: Option<bool> = None;
        resp.context_menu(|ui| {
            if ui.button("Sıradaki olarak ekle").clicked() {
                queue_req = Some(true);
                ui.close_menu();
            }
            if ui.button("Sıraya ekle").clicked() {
                queue_req = Some(false);
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Kütüphaneden kaldır").clicked() {
                remove = Some(key.clone());
                ui.close_menu();
            }
        });
        if let Some(next) = queue_req {
            self.pending_queue = Some((album_paths, next));
        }
        ui.set_opacity(prev_op);
        (open, play, remove)
    }

    pub(super) fn apply_album_actions(&mut self, open: Option<String>, play: Option<String>, remove: Option<String>) {
        if let Some(k) = open {
            self.lib_open = Some(k);
        }
        if let Some(k) = play {
            self.play_album(&k, 0);
        }
        if let Some(k) = remove {
            self.remove_album_undoable(&k);
        }
    }

    /// Albümü kütüphaneden çıkarır; dosyalara dokunmaz ve birkaç saniye boyunca geri alınabilir.
    pub(super) fn remove_album_undoable(&mut self, key: &str) {
        if let Some(i) = self.lib.albums.iter().position(|a| a.key == key) {
            let album = self.lib.albums[i].clone();
            let msg = format!("\"{}\" kütüphaneden kaldırıldı", album.title);
            self.lib.remove_album(key);
            self.lib.save();
            self.undo = Some((Undo::Album(album, i), msg, Instant::now()));
        }
    }

    /// Geri alınabilir silmelerin bildirimi: mesaj ve "Geri al" düğmesi, 6 sn sonra kapanır.
    pub(super) fn undo_toast(&mut self, ctx: &egui::Context) {
        let Some((_, msg, t0)) = &self.undo else { return };
        let age = t0.elapsed().as_secs_f32();
        if age > 6.0 {
            self.undo = None;
            return;
        }
        let msg = msg.clone();
        let mut clicked = false;
        egui::Area::new(Id::new("undo_toast")).anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -76.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            Frame::none().fill(BG3).stroke(Stroke::new(1.0, STROKE)).rounding(Rounding::same(8.0)).inner_margin(Margin::symmetric(14.0, 8.0)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(msg).font(bold(13.0)));
                    ui.add_space(8.0);
                    if pill_button(ui, "Geri al", Some(Icon::Undo), false).clicked() {
                        clicked = true;
                    }
                });
            });
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        if clicked {
            if let Some((u, _, _)) = self.undo.take() {
                match u {
                    Undo::Album(a, i) => {
                        if !self.lib.albums.iter().any(|x| x.key == a.key) {
                            let at = i.min(self.lib.albums.len());
                            self.lib.albums.insert(at, a);
                            self.lib.save();
                        }
                    }
                    Undo::Preset(name, bytes) => {
                        let _ = std::fs::create_dir_all(presets_dir());
                        let _ = std::fs::write(presets_dir().join(&name), bytes);
                        self.refresh_library();
                    }
                }
            }
        }
    }

    pub(super) fn album_grid(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let cur = self.current_path();
        let (mut open, mut play, mut remove) = (None, None, None);
        let vis = self.visible_albums();
        // bölümler: tür -> albüm indeksleri
        let mut sections: Vec<(String, Vec<usize>)> = vec![];
        if self.cfg.group_by_genre {
            let mut map: std::collections::BTreeMap<String, Vec<usize>> = std::collections::BTreeMap::new();
            for &i in &vis {
                let a = &self.lib.albums[i];
                let name = if a.genre.is_empty() { "Diğer".to_string() } else { a.genre.clone() };
                map.entry(name).or_default().push(i);
            }
            sections = map.into_iter().collect();
            sections.sort_by(|a, b| {
                let (ao, bo) = (a.0 == "Diğer", b.0 == "Diğer");
                ao.cmp(&bo).then(b.1.len().cmp(&a.1.len())).then(a.0.cmp(&b.0))
            });
        } else {
            sections.push((String::new(), vis.clone()));
        }
        if vis.is_empty() {
            self.no_match(ui, "albüm");
            return;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let gap = 26.0;
            let avail = ui.available_width() - 6.0;
            let cols = ((avail + gap) / (180.0 + gap)).floor().max(1.0);
            let cw = ((avail - gap * (cols - 1.0)) / cols).floor();
            for (name, idxs) in &sections {
                if !name.is_empty() {
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(name).font(bold(18.0)));
                        ui.label(RichText::new(format!("{} albüm", idxs.len())).font(body(12.5)).color(DIM));
                    });
                    ui.add_space(6.0);
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
                    ui.spacing_mut().item_spacing = vec2(gap, gap);
                    for &i in idxs {
                        let (o, p, r) = self.album_card(ui, ctx, i, cw, &cur);
                        open = open.take().or(o);
                        play = play.take().or(p);
                        remove = remove.take().or(r);
                    }
                });
            }
            ui.add_space(16.0);
        });
        self.apply_album_actions(open, play, remove);
    }

    /// Kapağın baskın rengi (küçültülmüş görüntünün doygunluk ağırlıklı ortalaması).
    pub(super) fn album_tint(&mut self, key: &str) -> Color32 {
        if let Some(c) = self.lib_tint.get(key) {
            return *c;
        }
        let c = image::open(library::cover_path(key))
            .ok()
            .map(|img| {
                let small = image::imageops::thumbnail(&img.to_rgba8(), 16, 16);
                let (mut r, mut gc, mut b, mut wsum) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
                for p in small.pixels() {
                    let (pr, pg, pb) = (p[0] as f32, p[1] as f32, p[2] as f32);
                    let mx = pr.max(pg).max(pb);
                    let mn = pr.min(pg).min(pb);
                    let w = 0.15 + (mx - mn) / 255.0;
                    r += pr * w;
                    gc += pg * w;
                    b += pb * w;
                    wsum += w;
                }
                let (r, gc, b) = (r / wsum, gc / wsum, b / wsum);
                // açık kapaklarda zemin grileşmesin: parlaklığı sınırla
                let k = (120.0 / r.max(gc).max(b).max(1.0)).min(1.0);
                Color32::from_rgb((r * k) as u8, (gc * k) as u8, (b * k) as u8)
            })
            .unwrap_or(BLUE);
        self.lib_tint.insert(key.to_string(), c);
        c
    }

    pub(super) fn album_detail(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, key: &str) {
        let Some(album) = self.lib.albums.iter().find(|a| a.key == key).cloned() else { return };
        let tex = self.album_tex(ctx, key);
        let tint = self.album_tint(key);
        let cur = self.current_path();
        let playing = self.player.playing.load(Relaxed);
        let mut back = false;
        let mut play_from: Option<usize> = None;
        let mut shuffle_play = false;
        let mut remove = false;
        let mut goto_artist = false;

        let _ = tint;
        // pencere genişliğiyle ölçeklen: geniş pencerede liste ve yazılar büyür
        let full_w = ui.available_width();
        let col_w = (full_w - 14.0).max(300.0);
        let sc = (full_w / 1100.0).clamp(1.0, 1.25);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(full_w);
            ui.add_space(6.0);
            let label = if self.lib_artist.is_some() { "Geri" } else { "Albümler" };
            if pill_button(ui, label, Some(Icon::Undo), false).clicked() {
                back = true;
            }
            ui.add_space(34.0);

            // ---- kahraman alan
            let cover = 224.0 * sc;
            ui.horizontal_top(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(cover, cover), Sense::hover());
                for i in 1..=7 {
                    let e = i as f32 * 3.5;
                    ui.painter().rect_filled(rect.expand(e).translate(vec2(0.0, 10.0)), Rounding::same(10.0 + e), Color32::from_black_alpha(14));
                }
                match tex {
                    Some(t) => rounded_image(ui.painter(), t, rect, 10.0, Color32::WHITE),
                    None => {
                        ui.painter().rect_filled(rect, Rounding::same(10.0), BG2);
                        draw_icon(ui.painter(), Icon::Wave, rect.center(), 56.0, alpha(DIM, 160));
                    }
                }
                ui.add_space(26.0);
                ui.vertical(|ui| {
                    ui.set_width((col_w - cover - 26.0).max(240.0));
                    ui.add_space(30.0 * sc);
                    let kind = if album.genre.is_empty() { "Albüm".to_string() } else { format!("Albüm · {}", album.genre) };
                    ui.label(RichText::new(kind).font(body(12.5)).color(alpha(TEXT, 170)));
                    ui.add_space(2.0);
                    ui.add(egui::Label::new(RichText::new(&album.title).font(bold(34.0 * sc)).color(Color32::WHITE)).wrap());
                    ui.add_space(2.0);
                    let artist = ui.add(egui::Label::new(RichText::new(&album.artist).font(bold(16.0 * sc)).color(alpha(Color32::WHITE, 225))).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand);
                    if artist.hovered() {
                        let y = artist.rect.bottom() + 1.0;
                        ui.painter().line_segment([pos2(artist.rect.left(), y), pos2(artist.rect.right(), y)], Stroke::new(1.0, alpha(Color32::WHITE, 160)));
                    }
                    if artist.clicked() {
                        goto_artist = true;
                    }
                    let secs = album.total_secs() as u64;
                    let dur = if secs >= 3600 { format!("{} sa {} dk", secs / 3600, (secs / 60) % 60) } else { format!("{} dk", (secs / 60).max(1)) };
                    ui.add_space(1.0);
                    ui.label(RichText::new(format!("{} parça · {}", album.tracks.len(), dur)).font(body(13.0)).color(DIM));
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        if pill_button(ui, "Çal", Some(Icon::Play), true).clicked() {
                            play_from = Some(0);
                        }
                        if pill_button(ui, "Karıştır", Some(Icon::Shuffle), false).clicked() {
                            shuffle_play = true;
                        }
                        // yıkıcı eylem ana düğmelerden ayrı, "···" menüsünün içinde
                        let more = pill_button(ui, "···", None, false).on_hover_text("Diğer işlemler");
                        let pid = more.id.with("more");
                        if more.clicked() {
                            ui.memory_mut(|m| m.toggle_popup(pid));
                        }
                        egui::popup_below_widget(ui, pid, &more, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                            ui.set_min_width(220.0);
                            if ui.button("Kütüphaneden kaldır (dosyalar silinmez)").clicked() {
                                remove = true;
                            }
                        });
                    });
                });
            });
            ui.add_space(44.0);

            // ---- parça listesi
            let multi_disc = album.tracks.iter().any(|t| t.disc_no > 1);
            let mut last_disc = 0u32;
            let base_row_h = 52.0 * sc;
            for (i, t) in album.tracks.iter().enumerate() {
                if multi_disc && t.disc_no != last_disc {
                    last_disc = t.disc_no;
                    ui.add_space(8.0);
                    ui.label(RichText::new(format!("Disk {}", t.disc_no.max(1))).font(bold(13.0)).color(DIM));
                    ui.add_space(4.0);
                }
                let tfont = if cur.as_ref() == Some(&t.path) { bold(14.5 * sc) } else { body(14.5 * sc) };
                let is_missing = self.lib_missing.contains(&t.path);
                let title_txt = if is_missing { format!("{} · dosya yok", t.title) } else { t.title.clone() };
                let (ttl_l, lh_t) = {
                    let p = ui.painter().clone();
                    (wrap_lines(&p, &title_txt, &tfont, (col_w - 60.0 * sc - 18.0 - 56.0 * sc).max(100.0)), line_height(&p, &tfont))
                };
                let row_h = base_row_h.max(ttl_l.len() as f32 * lh_t + 18.0);
                let (rect0, resp) = ui.allocate_exact_size(vec2(col_w, row_h), Sense::click());
                let ap = appear(ctx, Id::new(("trow", &t.path, self.view_epoch)), i.min(14) as f32 * 0.025);
                let rect = rect0.translate(vec2((1.0 - ap) * -12.0, 0.0));
                let prev_op = ui.opacity();
                ui.set_opacity(prev_op * ap);
                let h = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1);
                let is_cur = cur.as_ref() == Some(&t.path);
                let p = ui.painter();
                if is_cur {
                    p.rect_filled(rect.shrink2(vec2(0.0, 1.0)), Rounding::same(8.0), alpha(BLUE, 30));
                } else if h > 0.0 {
                    p.rect_filled(rect.shrink2(vec2(0.0, 1.0)), Rounding::same(8.0), alpha(Color32::WHITE, (14.0 * h) as u8));
                }
                if !is_cur && h < 0.5 && i + 1 < album.tracks.len() {
                    p.hline(rect.x_range().shrink(14.0), rect.bottom(), Stroke::new(1.0, alpha(STROKE, 150)));
                }
                let num = if t.track_no > 0 { t.track_no.to_string() } else { (i + 1).to_string() };
                let nc = pos2(rect.left() + 28.0 * sc, rect.center().y);
                if is_cur {
                    let r = Rect::from_center_size(nc, vec2(14.0, 16.0));
                    let mut child = ui.child_ui(r, egui::Layout::left_to_right(egui::Align::Center), None);
                    playing_bars(&mut child, playing, vec2(14.0, 16.0));
                } else if h > 0.0 {
                    draw_icon(ui.painter(), Icon::Play, nc, 13.0, alpha(Color32::WHITE, (255.0 * h) as u8));
                } else {
                    ui.painter().text(nc, Align2::CENTER_CENTER, num, body(13.0), DIM);
                }
                let ty = rect.center().y - ttl_l.len() as f32 * lh_t * 0.5;
                draw_lines(ui.painter(), pos2(rect.left() + 60.0 * sc, ty), Align2::LEFT_TOP, &ttl_l, &tfont, if is_missing { DIM } else if is_cur { Color32::WHITE } else { TEXT });
                let d = t.duration as u64;
                ui.painter().text(pos2(rect.right() - 18.0, rect.center().y), Align2::RIGHT_CENTER, format!("{}:{:02}", d / 60, d % 60), body(13.0 * sc), DIM);
                ui.set_opacity(prev_op);
                if resp.clicked() {
                    play_from = Some(i);
                }
            }
            ui.add_space(24.0);
        });
        if back {
            self.lib_open = None;
        }
        if goto_artist {
            let a = album.artist.clone();
            self.open_artist(&a);
        }
        if shuffle_play && !album.tracks.is_empty() {
            self.player.shuffle.store(true, Relaxed);
            self.cfg.shuffle = true;
            let n = album.tracks.len();
            let start = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos() as usize).unwrap_or(0)) % n;
            self.play_album(key, start);
        } else if let Some(i) = play_from {
            self.play_album(key, i);
        }
        if remove {
            self.remove_album_undoable(key);
            self.lib_open = None;
        }
    }

    pub(super) fn library_folders_card(&mut self, ui: &mut egui::Ui) {
        let mut remove: Option<String> = None;
        let mut add = false;
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                section_label(ui, "Kütüphane klasörleri");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if pill_button(ui, "Klasör ekle", Some(Icon::Folder), false).clicked() {
                        add = true;
                    }
                });
            });
            ui.add_space(2.0);
            ui.add(egui::Label::new(RichText::new("İzlenen klasörlerdeki yeni dosyalar yeniden taramada kütüphaneye eklenir. Klasörü listeden çıkarmak mevcut parçaları silmez.").font(body(12.0)).color(DIM)).wrap());
            ui.add_space(6.0);
            if self.lib.folders.is_empty() {
                ui.label(RichText::new("İzlenen klasör yok").font(body(12.5)).color(DIM));
            }
            for f in self.lib.folders.clone() {
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 40.0).max(100.0);
                    ui.allocate_ui(vec2(w, 0.0), |ui| {
                        ui.add(egui::Label::new(RichText::new(&f).font(body(12.5)).color(TEXT)).wrap());
                    });
                    if icon_button(ui, Icon::Trash, 26.0, false).on_hover_text("Listeden çıkar").clicked() {
                        remove = Some(f.clone());
                    }
                });
            }
            ui.add_space(6.0);
            switch_row(ui, "Açılışta yeni dosyaları tara", "", &mut self.cfg.scan_on_start);
        });
        if add {
            if let Some(d) = rfd::FileDialog::new().pick_folder() {
                self.add_library_folder(d);
            }
        }
        if let Some(f) = remove {
            self.lib.remove_folder(&f);
            self.lib.save();
        }
    }
}
