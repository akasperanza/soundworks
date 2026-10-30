use super::*;

const WORDMARK_PT: f32 = 19.0;

impl App {

    pub(super) fn format_badge(&self) -> Option<String> {
        let m = &self.meta;
        if m.title.is_empty() || m.codec.is_empty() || m.src_rate == 0 {
            return None;
        }
        let codec = m.codec.to_uppercase();
        let codec = if codec.starts_with("PCM") { "WAV".to_string() } else { codec.replace("VORBIS", "Vorbis") };
        let khz = m.src_rate as f32 / 1000.0;
        let khz = if (khz - khz.round()).abs() < 0.01 { format!("{khz:.0}") } else { format!("{khz:.1}") };
        Some(if m.bits > 0 { format!("{codec} {}/{khz}", m.bits) } else { format!("{codec} {khz} kHz") })
    }

    pub(super) fn sync_cover(&mut self, ctx: &egui::Context, dt: f32) {
        let id = self.player.meta_id.load(Relaxed);
        if id != self.cover_id {
            self.cover_id = id;
            self.cover_t0 = Instant::now();
            let mut m = self.player.meta.lock().unwrap().clone();
            self.cover_tex = m.cover.as_ref().map(|(w, h, d)| {
                ctx.load_texture(
                    format!("cover{id}"),
                    ColorImage::from_rgba_unmultiplied([*w as usize, *h as usize], d),
                    TextureOptions::LINEAR,
                )
            });
            self.tint_target = if m.cover.is_some() {
                let t = m.tint;
                lerp_color(Color32::from_rgb(t[0], t[1], t[2]), Color32::WHITE, 0.12)
            } else {
                BLUE
            };
            self.np_cover = m.cover.as_ref().and_then(|(w, h, d)| {
                let path = library::data_dir().join("now_playing.png");
                let _ = std::fs::create_dir_all(path.parent()?);
                let img = image::RgbaImage::from_raw(*w, *h, d.clone())?;
                img.save_with_format(&path, image::ImageFormat::Png).ok()?;
                Some(path)
            });
            self.play_bd = m.cover.as_ref().and_then(|(w, h, d)| blurred_backdrop(*w as usize, *h as usize, d));
            m.cover = None;
            self.meta = m;
        }
        self.tint = lerp_color(self.tint, self.tint_target, (dt * 3.0).min(1.0));
    }

    pub(super) fn header(&mut self, ctx: &egui::Context) {
        let upload_rect = egui::TopBottomPanel::top("header")
            .frame(Frame::none().inner_margin(Margin { left: 36.0, right: 36.0, top: 18.0, bottom: 14.0 }))
            .show(ctx, |ui| {
                let narrow = ctx.screen_rect().width() < 1180.0;
                let mut left_end = 0.0f32;
                let mut upload_btn = Rect::NOTHING;
                let mut right_start = f32::MAX;
                let row = ui.horizontal(|ui| {
                    let ub = icon_button(ui, if self.upload_open { Icon::ChevronUp } else { Icon::ChevronDown }, 30.0, self.upload_open);
                    upload_btn = ub.rect;
                    if ub.clicked() {
                        self.upload_open = !self.upload_open;
                    }
                    ui.add_space(8.0);
                    for (v, n) in [(View::Library, "Kütüphane"), (View::Motor, "Motor")] {
                        if v == View::Motor && self.motor_detached {
                            continue;
                        }
                        let resp = tab(ui, n, self.view == v);
                        if v == View::Motor {
                            resp.clone().on_hover_text_tr("Ctrl + tıklama: ayrı pencerede aç");
                        }
                        if resp.clicked() {
                            if v == View::Motor && ui.input(|i| i.modifiers.command) {
                                self.detach_motor();
                            } else {
                                self.set_view(v);
                                if v == View::Library {
                                    self.lib_artist = None;
                                    self.lib_open = None;
                                }
                            }
                        }
                    }
                    left_end = ui.min_rect().right();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {

                        ui.label(egui::RichText::new("Soundworks").font(egui::FontId::new(WORDMARK_PT, egui::FontFamily::Name("wordmark".into()))).color(Color32::WHITE));
                        ui.add_space(8.0);
                        let on = self.running.is_some();

                        if self.cfg.show_engine_btn {
                            let what = if self.cfg.use_loopback { "Sistem DSP" } else { "Ses motoru" };
                            let tip = if on { format!("{what} açık: durdur (cihazı serbest bırakır)") } else { format!("{what} kapalı: başlat") };
                            if icon_button(ui, Icon::Power, 34.0, on).on_hover_text_tr(tip).clicked() {
                                if on {
                                    self.stop_async();
                                } else {
                                    self.start();
                                }
                            }
                        }
                        if self.cfg.use_loopback {
                            let (r, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
                            draw_icon(ui.painter(), Icon::Wave, r.center(), 15.0, if on { BLUE } else { DIM });
                            resp.on_hover_text_tr(if on { "Sistem sesi açık" } else { "Sistem sesi kapalı (motor durdu)" });
                        }
                        let status = match &self.running {
                            Some(r) => {
                                let o = r.ctl.out_info.lock().unwrap().clone();
                                let s = r.ctl.status.lock().unwrap().clone();
                                if s.starts_with("Hata") {
                                    s
                                } else if o.rate > 0 {
                                    format!("{} · {:.1} kHz / {} bit", if o.exclusive { "Exclusive" } else { "Shared" }, o.rate as f32 / 1000.0, o.bits)
                                } else {
                                    "Başlatılıyor…".to_string()
                                }
                            }
                            None => self.message.clone(),
                        };
                        let err = status.starts_with("Hata");

                        if err {
                            status_dot(ui, true, RED);
                        }
                        if err && !narrow {
                            ui.scope(|ui| {
                                ui.set_max_width(300.0);
                                ui.add(egui::Label::new(RichText::new(&status).font(body(12.0)).color(if err { RED } else { DIM })).wrap());
                            });
                        }
                        right_start = ui.min_rect().left();
                    });
                });

                let rr = row.response.rect;
                let cx = rr.center().x;
                let half = (cx - left_end - 24.0).min(right_start - cx - 24.0).min(170.0);
                if half >= 62.0 {
                    let w = half * 2.0;
                    let r = Rect::from_center_size(pos2(cx, rr.center().y), vec2(w, 32.0));
                    let resp = ui.interact(r, Id::new("search_box"), Sense::click());
                    let h = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.12);
                    let a = ui.ctx().animate_bool_with_time(resp.id.with("a"), self.search_open, 0.15);
                    let p = ui.painter();
                    p.rect_filled(r, Rounding::same(16.0), lerp_color(BG2, BG3, h));
                    p.rect_stroke(r, Rounding::same(16.0), Stroke::new(1.0_f32, lerp_color(STROKE, BLUE, a.max(h * 0.5))));
                    draw_icon(p, Icon::Search, pos2(r.left() + 20.0, r.center().y), 12.5, lerp_color(DIM, TEXT, h));
                    p.text_tr(pos2(r.left() + 38.0, r.center().y), Align2::LEFT_CENTER, if w >= 300.0 { "Sanatçı, albüm ya da parça ara" } else { "Ara…" }, body(13.0), lerp_color(DIM, TEXT, h * 0.6));
                    if w > 280.0 {
                        let kr = Rect::from_center_size(pos2(r.right() - 24.0, r.center().y), vec2(30.0, 20.0));
                        p.rect_filled(kr, Rounding::same(5.0), alpha(BG3, 220));
                        p.text_tr(kr.center(), Align2::CENTER_CENTER, "F8", bold(10.5), DIM);
                    }
                    if resp.on_hover_text_tr("Shift+K  ·  F8").clicked() {
                        if self.search_open {
                            self.search_open = false;
                        } else {
                            self.open_search();
                        }
                    }
                } else {

                    let r = Rect::from_center_size(pos2(cx, rr.center().y), vec2(32.0, 32.0));
                    let resp = ui.interact(r, Id::new("search_box"), Sense::click());
                    draw_icon(ui.painter(), Icon::Search, r.center(), 14.0, if resp.hovered() { TEXT } else { DIM });
                    if resp.on_hover_text_tr("Ara (Shift+K · F8)").clicked() {
                        self.open_search();
                    }
                }
                upload_btn
            })
            .inner;
        self.upload_menu(ctx, upload_rect);
    }

    pub(super) fn upload_menu(&mut self, ctx: &egui::Context, btn: Rect) {
        let t = ctx.animate_bool_with_time(Id::new("upload_anim"), self.upload_open, 0.16);
        if t <= 0.001 {
            self.upload_sub = false;
            return;
        }
        let e = ease(t);
        let s = ctx.animate_bool_with_time(Id::new("upload_sub_anim"), self.upload_open && self.upload_sub, 0.18);
        let se = ease(s);
        let (mw, mh) = (176.0f32, 144.0f32);
        let main_pos = pos2(btn.left() - 4.0, btn.bottom() + 6.0);
        let main = Rect::from_min_size(main_pos, vec2(mw, mh));
        let (sw, sh) = (214.0f32, 100.0f32);
        let sub_full = Rect::from_min_size(pos2(main.right() + 6.0, main.top()), vec2(sw, sh));
        let mut action: Option<u8> = None;
        let mut toggle_sub = false;
        let mut hover_sub = false;
        let mut hover_row2 = false;
        let mut open_settings = false;
        let mut open_extensions = false;
        let mut hover_row3 = false;

        egui::Area::new(Id::new("upload_main")).order(egui::Order::Foreground).fixed_pos(main_pos + vec2(0.0, (1.0 - e) * -6.0)).show(ctx, |ui| {
            ui.set_opacity(e);
            let (rect, _) = ui.allocate_exact_size(vec2(mw, mh), Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(rect, Rounding::same(10.0), BG2);
            painter.rect_stroke(rect, Rounding::same(10.0), Stroke::new(1.0_f32, STROKE));
            let row = Rect::from_min_size(rect.min + vec2(6.0, 6.0), vec2(mw - 12.0, 44.0));
            let resp = ui.interact(row, Id::new("upload_row"), Sense::click());
            if resp.clicked() {
                toggle_sub = true;
            }
            if resp.hovered() {
                hover_sub = true;
            }
            let hv = ui.ctx().animate_bool_with_time(resp.id, resp.hovered() || self.upload_sub, 0.1);
            ui.painter().rect_filled(row, Rounding::same(8.0), alpha(BLUE, (70.0 * hv) as u8));
            draw_icon(ui.painter(), Icon::Upload, pos2(row.left() + 20.0, row.center().y), 13.0, lerp_color(TEXT, Color32::WHITE, hv));
            ui.painter().text_tr(pos2(row.left() + 40.0, row.center().y), Align2::LEFT_CENTER, "Yükle", bold(14.0), Color32::WHITE);
            draw_icon(ui.painter(), Icon::ChevronRight, pos2(row.right() - 16.0, row.center().y), 12.0, lerp_color(DIM, Color32::WHITE, hv));

            let row3 = Rect::from_min_size(rect.min + vec2(6.0, 50.0), vec2(mw - 12.0, 44.0));
            let resp3 = ui.interact(row3, Id::new("extensions_row"), Sense::click());
            if resp3.clicked() {
                open_extensions = true;
            }
            if resp3.hovered() {
                hover_row3 = true;
            }
            let hv3 = ui.ctx().animate_bool_with_time(resp3.id, resp3.hovered(), 0.1);
            ui.painter().rect_filled(row3, Rounding::same(8.0), alpha(BLUE, (70.0 * hv3) as u8));
            draw_icon(ui.painter(), Icon::Puzzle, pos2(row3.left() + 20.0, row3.center().y), 13.0, lerp_color(TEXT, Color32::WHITE, hv3));
            ui.painter().text_tr(pos2(row3.left() + 40.0, row3.center().y), Align2::LEFT_CENTER, "Eklentiler", bold(14.0), Color32::WHITE);

            let row2 = Rect::from_min_size(rect.min + vec2(6.0, 94.0), vec2(mw - 12.0, 44.0));
            let resp2 = ui.interact(row2, Id::new("settings_row"), Sense::click());
            if resp2.clicked() {
                open_settings = true;
            }
            if resp2.hovered() {
                hover_row2 = true;
            }
            let hv2 = ui.ctx().animate_bool_with_time(resp2.id, resp2.hovered(), 0.1);
            ui.painter().rect_filled(row2, Rounding::same(8.0), alpha(BLUE, (70.0 * hv2) as u8));
            draw_icon(ui.painter(), Icon::Settings, pos2(row2.left() + 20.0, row2.center().y), 13.0, lerp_color(TEXT, Color32::WHITE, hv2));
            ui.painter().text_tr(pos2(row2.left() + 40.0, row2.center().y), Align2::LEFT_CENTER, "Ayarlar", bold(14.0), Color32::WHITE);
            ui.painter().text_tr(pos2(row2.right() - 12.0, row2.center().y), Align2::RIGHT_CENTER, "Ctrl+,", body(11.5), DIM);
        });

        if s > 0.001 {
            egui::Area::new(Id::new("upload_sub"))
                .order(egui::Order::Foreground)
                .fixed_pos(sub_full.min + vec2((1.0 - se) * -10.0, 0.0))
                .show(ctx, |ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(sw * (0.55 + 0.45 * se), sh), Sense::hover());
                    let painter = ui.painter().with_clip_rect(rect.expand(1.0));
                    let a = (255.0 * se) as u8;
                    let full = Rect::from_min_size(rect.min, vec2(sw, sh));
                    painter.rect_filled(full, Rounding::same(10.0), alpha(BG2, a));
                    painter.rect_stroke(full, Rounding::same(10.0), Stroke::new(1.0_f32, alpha(STROKE, a)));
                    let items = [
                        ("Klasör", "Albüm olarak yükler", Icon::Folder, 0u8),
                        ("Parça", "Varsa albümüne ekler", Icon::Track, 1u8),
                    ];
                    for (i, (name, sub, icon, id)) in items.iter().enumerate() {
                        let r = Rect::from_min_size(pos2(full.left() + 6.0, full.top() + 6.0 + i as f32 * 44.0), vec2(sw - 12.0, 40.0));
                        let hv = if self.upload_sub && s > 0.8 {
                            let resp = ui.interact(r.intersect(rect), Id::new(("upload_opt", *id)), Sense::click());
                            if resp.clicked() {
                                action = Some(*id);
                            }
                            ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1)
                        } else {
                            0.0
                        };
                        painter.rect_filled(r, Rounding::same(8.0), alpha(BLUE, (70.0 * hv) as u8));
                        draw_icon(&painter, *icon, pos2(r.left() + 18.0, r.center().y), 13.0, alpha(lerp_color(TEXT, Color32::WHITE, hv), a));
                        painter.text_tr(pos2(r.left() + 38.0, r.center().y - 7.0), Align2::LEFT_CENTER, *name, bold(13.5), alpha(Color32::WHITE, a));
                        painter.text_tr(pos2(r.left() + 38.0, r.center().y + 9.0), Align2::LEFT_CENTER, *sub, body(11.0), alpha(DIM, a));
                    }
                });
        }

        if toggle_sub {
            self.upload_sub = !self.upload_sub;
        } else if hover_sub {
            self.upload_sub = true;
        } else if hover_row2 || hover_row3 {
            self.upload_sub = false;
        }
        if open_extensions {
            self.upload_open = false;
            self.upload_sub = false;
            self.set_view(View::Extensions);
        }
        if open_settings {
            self.upload_open = false;
            self.upload_sub = false;
            self.set_view(View::Settings);
        }

        if ctx.input(|i| i.pointer.any_click()) {
            if let Some(p) = ctx.input(|i| i.pointer.interact_pos()) {
                let in_sub = self.upload_sub && sub_full.contains(p);
                if !main.contains(p) && !in_sub && !btn.contains(p) {
                    self.upload_open = false;
                    self.upload_sub = false;
                }
            }
        }
        match action {
            Some(0) => {
                self.upload_open = false;
                self.upload_sub = false;
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    self.add_library_folder(d);
                    self.set_view(View::Library);
                }
            }
            Some(_) => {
                self.upload_open = false;
                self.upload_sub = false;
                if let Some(f) = rfd::FileDialog::new().add_filter("Ses dosyaları", AUDIO_EXTS).pick_files() {
                    self.lib_add_paths(f);
                    self.set_view(View::Library);
                }
            }
            None => {}
        }
        ctx.request_repaint();
    }

    pub(super) fn bottom_bar(&mut self, ctx: &egui::Context) {

        if self.cfg.use_loopback {
            return;
        }
        let mut vol_btn_rect = Rect::NOTHING;
        let mut bar_rect = Rect::NOTHING;
        let playing_album: Option<(String, String)> = self
            .current_path()
            .and_then(|p| self.lib.albums.iter().find(|a| a.tracks.iter().any(|t| t.path == p)).map(|a| (a.key.clone(), a.artist.clone())));
        let (mut goto_album, mut goto_artist) = (false, false);
        let screen = ctx.screen_rect();
        let right_w = self.right_panels_width(ctx);
        let free_w = (screen.width() - right_w).max(0.0);
        let bar_w = (free_w * 0.72).clamp(780.0, 860.0).min(screen.width() - 32.0);
        let shift = -(right_w * 0.5).min((screen.width() - bar_w) * 0.5 - 40.0).max(0.0);
        egui::Area::new(Id::new("player_bar"))
            .order(egui::Order::Middle)
            .anchor(Align2::CENTER_BOTTOM, vec2(shift, -14.0))
            .show(ctx, |ui| {
                let scrim_h = 150.0;
                vgradient(
                    &ui.painter().with_clip_rect(screen),
                    Rect::from_min_max(pos2(screen.left(), screen.bottom() - scrim_h), screen.right_bottom()),
                    alpha(Color32::BLACK, 0),
                    alpha(Color32::BLACK, 205),
                );
                ui.set_width(bar_w);
                let glass = Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(16, 22, 36, 172))
                    .stroke(Stroke::new(1.0_f32, alpha(Color32::WHITE, 38)))
                    .rounding(Rounding::same(24.0))
                    .inner_margin(Margin::symmetric(18.0, 9.0))
                    .shadow(egui::epaint::Shadow { offset: vec2(0.0, 10.0), blur: 30.0, spread: 0.0, color: Color32::from_black_alpha(120) })
                    .show(ui, |ui| {
                ui.horizontal(|ui| {

                    let (r, cover_resp) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
                    if playing_album.is_some() {
                        goto_album |= cover_resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text_tr("Albüme git").clicked();
                    }
                    match &self.cover_tex {
                        Some(t) => rounded_image(ui.painter(), t.id(), r, 6.0, Color32::WHITE),
                        None => {
                            ui.painter().rect_filled(r, Rounding::same(6.0), BG2);
                            draw_icon(ui.painter(), Icon::Wave, r.center(), 22.0, DIM);
                        }
                    }
                    ui.allocate_ui(vec2(210.0, 40.0), |ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            let (t, a) = if self.meta.title.is_empty() {
                                ("Çalan yok".to_string(), "Kütüphaneden bir albüm seç".to_string())
                            } else {
                                (self.meta.title.clone(), self.meta.artist.clone())
                            };
                            let link = |ui: &mut egui::Ui, text: String, font: egui::FontId, color: Color32, enabled: bool| -> bool {
                                let r = ui.add(egui::Label::new(RichText::new(text).font(font).color(color)).wrap().sense(if enabled { Sense::click() } else { Sense::hover() }));
                                if enabled && r.hovered() {
                                    ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                                    let y = r.rect.bottom() - 1.0;
                                    ui.painter().line_segment([pos2(r.rect.left(), y), pos2(r.rect.right(), y)], Stroke::new(1.0_f32, alpha(color, 170)));
                                }
                                enabled && r.clicked()
                            };
                            let has = playing_album.is_some();
                            goto_album |= link(ui, t, bold(13.5), Color32::WHITE, has);
                            goto_artist |= link(ui, a, body(12.0), DIM, has);
                        });
                    });
                    let playing = self.player.playing.load(Relaxed);
                    ui.add_space(8.0);

                    let sh = self.player.shuffle.load(Relaxed);
                    if icon_button(ui, Icon::Shuffle, 30.0, sh).on_hover_text_tr("Karıştır").clicked() {
                        self.player.shuffle.store(!sh, Relaxed);
                        self.cfg.shuffle = !sh;
                    }
                    if icon_button(ui, Icon::Prev, 30.0, false).clicked() {
                        self.player.prev();
                    }
                    if play_button(ui, playing, 34.0).clicked() {
                        self.play_pressed();
                    }
                    if icon_button(ui, Icon::Next, 30.0, false).clicked() {
                        self.player.next();
                    }

                    let all = self.player.repeat.load(Relaxed);
                    let one = self.player.repeat_one.load(Relaxed);
                    let (ric, tip) = if one { (Icon::Repeat1, "Tek parçayı tekrarla") } else if all { (Icon::Repeat, "Listeyi tekrarla") } else { (Icon::Repeat, "Tekrar kapalı") };
                    if icon_button(ui, ric, 30.0, all || one).on_hover_text_tr(tip).clicked() {
                        let (na, no) = if !all && !one { (true, false) } else if all && !one { (false, true) } else { (false, false) };
                        self.player.repeat.store(na, Relaxed);
                        self.player.repeat_one.store(no, Relaxed);
                        self.cfg.repeat = na;
                        self.cfg.repeat_one = no;
                    }
                    ui.add_space(8.0);
                    let dur = self.player.duration();
                    let pos = self.player.position();
                    ui.label(RichText::new(fmt_time(pos)).font(body(12.0)).color(DIM));
                    let seek_w = (ui.available_width() - 266.0 - 36.0 * self.plugin_bar_count() as f32).max(120.0);
                    let cur = self.current_path();
                    if let Some(p) = &cur {
                        if !self.wave_cache.contains_key(p) && !self.wave_fetcher.is_pending(p) {
                            self.wave_fetcher.request(p);
                        }
                    }
                    let wave = cur.as_ref().and_then(|p| self.wave_cache.get(p).cloned());
                    let progress = self.wave_drag.unwrap_or(if dur > 0.0 { (pos / dur) as f32 } else { 0.0 });

                    let (wresp, preview, commit) =
                        waveform_seek(ui, vec2(seek_w, 36.0), wave.as_ref().map(|v| v.as_slice()), progress, dur > 0.0 && cur.is_some(), true);
                    self.wave_rect = wresp.rect;
                    if !self.wave_big {
                        self.wave_drag = preview;
                    }
                    if let Some(f) = commit {
                        self.player.seek(f as f64 * dur);
                        self.wave_drag = None;
                    }
                    if dur > 0.0 {
                        if let Some(hp) = wresp.hover_pos() {
                            let f = ((hp.x - wresp.rect.left()) / wresp.rect.width()).clamp(0.0, 1.0);
                            wresp.on_hover_ui_at_pointer(|ui| {
                                ui.label(RichText::new(fmt_time(f as f64 * dur)).font(bold(12.0)));
                            });
                        }
                    }
                    ui.label(RichText::new(fmt_time(dur)).font(body(12.0)).color(DIM));
                    ui.add_space(4.0);

                    let vol = self.player.volume();
                    let vic = if vol <= 0.001 { Icon::VolumeX } else if vol < 0.5 { Icon::Volume1 } else { Icon::Volume };
                    let vresp = icon_button(ui, vic, 32.0, self.vol_open);
                    vol_btn_rect = vresp.rect;
                    if vresp.clicked() {
                        if vol > 0.001 {
                            self.vol_prev = vol;
                            self.player.set_volume(0.0);
                            self.cfg.volume = Some(0.0);
                        } else {
                            let v = self.vol_prev.max(0.2);
                            self.player.set_volume(v);
                            self.cfg.volume = Some(v);
                        }
                    }

                    let lb = icon_button(ui, Icon::Lyrics, 32.0, self.lyrics_open).on_hover_text_tr("Sözler");
                    if lb.clicked() {
                        self.lyrics_open = !self.lyrics_open;
                        if self.lyrics_open {
                            self.queue_open = false;
                            self.plugin_set_panel(None);
                        }
                    }
                    self.proof_light(ui);
                    let qb = icon_button(ui, Icon::List, 32.0, self.queue_open).on_hover_text_tr("Kuyruk");
                    if qb.clicked() {
                        self.queue_open = !self.queue_open;
                        if self.queue_open {
                            self.lyrics_open = false;
                            self.plugin_set_panel(None);
                        }
                    }
                    self.plugin_bar_buttons(ui);
                    let fb = icon_button(ui, Icon::Maximize, 32.0, false).on_hover_text_tr("Tam ekran (F11)");
                    if fb.clicked() && !self.meta.title.is_empty() {
                        self.ambient_on = true;
                        self.ambient_since = Instant::now();
                        self.ambient_last = Instant::now();
                        self.ambient_ctl = Instant::now();
                    }
                });
                    });
                bar_rect = glass.response.rect;
                edge_highlight(ui.painter(), glass.response.rect, 22.0, 120);
            });

        let pointer = ctx.pointer_hover_pos();
        let in_btn = pointer.map(|p| vol_btn_rect.expand(3.0).contains(p)).unwrap_or(false);
        let in_pop = pointer.map(|p| self.vol_popup_rect.contains(p)).unwrap_or(false);
        if in_btn || (self.vol_open && in_pop) {
            self.vol_open = true;
            self.vol_t = Instant::now();
        } else if self.vol_open && self.vol_t.elapsed() > std::time::Duration::from_millis(250) && crate::devenv::dev_var("SW_VOLPOP").is_err() {
            self.vol_open = false;
        }
        let t = ctx.animate_bool_with_time(Id::new("vol_anim"), self.vol_open, 0.14);
        if t > 0.01 {
            let (w, h) = (46.0, 150.0);
            let pos = pos2(vol_btn_rect.center().x - w * 0.5, vol_btn_rect.top() - h - 6.0 + (1.0 - ease(t)) * 8.0);
            let area = egui::Area::new(Id::new("vol_popup")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
                ui.set_opacity(t);
                Frame::none()
                    .fill(BG2)
                    .stroke(Stroke::new(1.0_f32, STROKE))
                    .rounding(Rounding::same(10.0))
                    .inner_margin(Margin::symmetric(10.0, 10.0))
                    .show(ui, |ui| {
                        ui.set_width(w - 20.0);
                        ui.vertical_centered(|ui| {
                            let mut v = self.player.volume();
                            let db = if v < 0.001 { "sessiz".to_string() } else { format!("{:.0} dB", 40.0 * v.log10()) };
                            ui.label(RichText::new(db).font(bold(12.0)).color(DIM));
                            ui.spacing_mut().slider_width = 100.0;
                            if ui.add(egui::Slider::new(&mut v, 0.0..=1.0).vertical().show_value(false)).changed() {
                                self.player.set_volume(v);
                                self.cfg.volume = Some(v);
                            }
                        });
                    });
            });

            self.vol_popup_rect = area.response.rect.union(vol_btn_rect);
        } else {
            self.vol_popup_rect = Rect::NOTHING;
        }
        if let Some((key, artist)) = playing_album {
            if goto_album {
                self.set_view(View::Library);
                self.lib_artist = None;
                self.lib_open = Some(key);
            } else if goto_artist {
                self.open_artist(&artist);
            }
        }
        self.big_waveform(ctx, bar_rect);
    }

    /// While the pointer is over the waveform, a large scrubber covers the whole bar (icons included).
    fn big_waveform(&mut self, ctx: &egui::Context, bar: Rect) {
        let dur = self.player.duration();
        let cur = self.current_path();
        let active = dur > 0.0 && cur.is_some();
        let modal = self.meta_edit.is_some() || self.del_confirm.is_some() || self.dsd_dialog.is_some() || self.search_open;
        let pointer = ctx.pointer_hover_pos();
        let over = pointer.map_or(false, |p| self.wave_rect.contains(p) || (self.wave_big && self.wave_big_rect.contains(p)));
        let dragging = self.wave_big && ctx.input(|i| i.pointer.primary_down());
        self.wave_big = active && !modal && bar.is_positive() && (over || dragging || crate::devenv::dev_var("SW_BIGWAVE").is_ok());
        let t = ctx.animate_bool_with_time(Id::new("wave_big_t"), self.wave_big, 0.12);
        if t <= 0.01 || !bar.is_positive() {
            if !self.wave_big {
                self.wave_big_rect = Rect::NOTHING;
            }
            return;
        }
        let big = Rect::from_min_max(pos2(bar.left(), bar.top() - 40.0), bar.right_bottom());
        let wave = cur.as_ref().and_then(|p| self.wave_cache.get(p).cloned());
        let pos = self.player.position();
        let progress = self.wave_drag.unwrap_or(if dur > 0.0 { (pos / dur) as f32 } else { 0.0 });
        let mut result = (None, None);
        let area = egui::Area::new(Id::new("wave_big")).order(egui::Order::Foreground).fixed_pos(big.min + vec2(0.0, (1.0 - ease(t)) * 10.0)).show(ctx, |ui| {
            ui.set_opacity(t);
            Frame::none()
                .fill(Color32::from_rgba_unmultiplied(12, 17, 30, 252))
                .stroke(Stroke::new(1.0_f32, alpha(Color32::WHITE, 46)))
                .rounding(Rounding::same(26.0))
                .inner_margin(Margin::symmetric(24.0, 14.0))
                .shadow(egui::epaint::Shadow { offset: vec2(0.0, 10.0), blur: 30.0, spread: 0.0, color: Color32::from_black_alpha(130) })
                .show(ui, |ui| {
                    ui.set_width(big.width() - 48.0);
                    ui.set_height(big.height() - 28.0);
                    ui.horizontal_centered(|ui| {
                        ui.spacing_mut().item_spacing.x = 14.0;
                        let shown = if dur > 0.0 { progress as f64 * dur } else { pos };
                        ui.label(RichText::new(fmt_time(shown)).font(bold(13.0)).color(Color32::WHITE));
                        let w = (ui.available_width() - 56.0).max(100.0);
                        let (resp, preview, commit) = waveform_seek(ui, vec2(w, big.height() - 28.0), wave.as_ref().map(|v| v.as_slice()), progress, true, false);
                        if let Some(hp) = resp.hover_pos() {
                            let f = ((hp.x - resp.rect.left()) / resp.rect.width()).clamp(0.0, 1.0);
                            resp.on_hover_ui_at_pointer(|ui| {
                                ui.label(RichText::new(fmt_time(f as f64 * dur)).font(bold(12.0)));
                            });
                        }
                        result = (preview, commit);
                        ui.label(RichText::new(fmt_time(dur)).font(body(13.0)).color(DIM));
                    });
                });
        });
        self.wave_big_rect = area.response.rect;
        self.wave_drag = result.0;
        if let Some(f) = result.1 {
            self.player.seek(f as f64 * dur);
            self.wave_drag = None;
        }
        ctx.request_repaint();
    }

    fn side_hide(&self, ctx: &egui::Context) -> f32 {
        let other = (self.lyrics_open && !self.cfg.use_loopback) || self.queue_open || self.plugin_panel.is_some();
        ease(ctx.animate_bool_with_time(Id::new("side_hide"), other, 0.24))
    }

    fn right_panels_width(&self, ctx: &egui::Context) -> f32 {
        let anim = |id: &str, on: bool| ease(ctx.animate_bool_with_time(Id::new(id), on, 0.24));
        let mut w = 0.0;
        if self.view == View::Motor {
            w += 318.0 * (1.0 - self.side_hide(ctx));
        }
        w += 450.0 * anim("lyr_side", self.lyrics_open && !self.cfg.use_loopback);
        w += 400.0 * anim("queue_side", self.queue_open);
        w += 380.0 * anim("plug_side", self.plugin_panel.is_some());
        w
    }

    pub(super) fn side_panel(&mut self, ctx: &egui::Context, fade: f32) {
        if dev_skip("side") {
            return;
        }
        let keep = 1.0 - self.side_hide(ctx);
        if keep < 0.2 {
            return;
        }
        let inner = Margin { left: 22.0, right: 22.0, top: 20.0, bottom: 18.0 };
        egui::SidePanel::right("side")
            .exact_width(318.0 * keep)
            .resizable(false)
            .frame(Frame::none().inner_margin(Margin { left: 12.0, right: 12.0, top: 2.0, bottom: 2.0 }))
            .show(ctx, |ui| {
                let avail = ui.available_size();
                raised_shadow(ui);
                raised_frame(inner).show(ui, |ui| {
                ui.set_min_size(vec2(avail.x - inner.left - inner.right, avail.y - inner.top - inner.bottom));
                raised_surface(ui, inner);
                ui.set_opacity(fade);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_width(ui.available_width());

                    ui.horizontal(|ui| {
                        section_label(ui, "Seviye");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let pk = self.viz.hold[0].max(self.viz.hold[1]);
                            ui.label(RichText::new(if pk > -89.0 { format!("{pk:+.1} dBFS") } else { "—".into() }).font(body(11.5)).color(DIM));
                        });
                    });
                    self.viz.draw_meters(ui, vec2(ui.available_width(), 130.0));
                    ui.add_space(6.0);
                    self.loudness_readout(ui);
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    section_label(ui, "Ses işleme");
                    ui.add_space(2.0);
                    ui.label(RichText::new("Crossfeed").font(body(12.5)).color(DIM));
                    ui.spacing_mut().slider_width = ui.available_width() - 64.0;
                    let mut cf = self.cfg.crossfeed * 100.0;
                    if ui.add(egui::Slider::new(&mut cf, 0.0..=100.0).suffix("%")).changed() {
                        self.cfg.crossfeed = cf / 100.0;
                    }
                    ui.label(RichText::new("Balans").font(body(12.5)).color(DIM));
                    let mut b = self.cfg.balance * 100.0;
                    let r = ui.add(egui::Slider::new(&mut b, -100.0..=100.0).custom_formatter(|v, _| {
                        if v.abs() < 1.0 { "C".into() } else if v < 0.0 { format!("L {:.0}", -v) } else { format!("R {:.0}", v) }
                    }));
                    if r.changed() {
                        self.cfg.balance = b / 100.0;
                    }
                    if r.double_clicked() {
                        self.cfg.balance = 0.0;
                    }
                    ui.add_space(4.0);
                    switch_row(ui, "Mono", "", &mut self.cfg.mono);
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);
                    self.signal_path(ui);
                });
                });
            });
    }

    pub(super) fn loudness_readout(&mut self, ui: &mut egui::Ui) {
        let get = |a: &std::sync::atomic::AtomicU32| -> Option<f32> {
            let v = f32::from_bits(a.load(std::sync::atomic::Ordering::Relaxed));
            (v > -150.0).then_some(v)
        };
        let ctl = self.running.as_ref().map(|r| r.ctl.clone());
        let (tp, m, s, i) = match &ctl {
            Some(c) => (get(&c.tp_db), get(&c.m_lufs), get(&c.s_lufs), get(&c.i_lufs)),
            None => (None, None, None, None),
        };
        let row = |ui: &mut egui::Ui, label: &str, unit: &str, v: Option<f32>, color: Color32, hint: &str| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).font(body(12.5)).color(DIM)).on_hover_text_tr(hint);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let txt = match v {
                        Some(x) => format!("{x:+.1} {unit}"),
                        None => "—".to_string(),
                    };
                    ui.label(RichText::new(txt).font(bold(12.5)).color(if v.is_some() { color } else { DIM }));
                });
            });
        };
        let tp_color = match tp {
            Some(x) if x > 0.0 => RED,
            Some(x) if x > -1.0 => AMBER,
            _ => TEXT,
        };
        ui.horizontal(|ui| {
            section_label(ui, "Ses şiddeti");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ctl.is_some() && ui.add(egui::Label::new(RichText::new("sıfırla").font(body(11.5)).color(SKY)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    if let Some(c) = &ctl {
                        c.meter_reset.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            });
        });
        ui.add_space(2.0);
        row(ui, "Gerçek tepe", "dBTP", tp, tp_color, "Örnekler arası (inter-sample) tepe dahil. 0 dBTP üstü kırpılma riski; -1 dBTP üstü sarı uyarı.");
        row(ui, "Anlık (400 ms)", "LUFS", m, TEXT, "Son 400 ms'nin K-ağırlıklı ses şiddeti");
        row(ui, "Kısa vadeli (3 sn)", "LUFS", s, TEXT, "Son 3 saniye");
        row(ui, "Bütünleşik", "LUFS", i, TEXT, "Başlangıçtan beri, sessizlik kapılanarak (ITU-R BS.1770 / EBU R128)");
    }

    pub(super) fn signal_path(&mut self, ui: &mut egui::Ui) {
        let out = self.running.as_ref().map(|r| r.ctl.out_info.lock().unwrap().clone());
        let eq_active = !self.preset.bands.is_empty();
        let mut reasons: Vec<&str> = vec![];
        let exclusive = out.as_ref().map(|o| o.exclusive).unwrap_or(false);
        let dop = out.as_ref().is_some_and(|o| o.dop);
        if self.cfg.use_loopback {
            reasons.push("kaynak sistem sesi");
        }
        if !exclusive {
            reasons.push("exclusive kapalı");
        }
        if self.meta.resampled && !self.cfg.use_loopback {
            reasons.push("yeniden örnekleme");
        }
        if eq_active {
            reasons.push("EQ açık");
        }
        if !self.cfg.use_loopback && self.player.volume() < 0.999 {
            reasons.push("ses < %100");
        }
        if self.cfg.crossfeed > 0.0 || self.cfg.mono || self.cfg.balance != 0.0 {
            reasons.push("DSP açık");
        }
        if self.player.rg_applied_db().is_some() && !self.cfg.use_loopback {
            reasons.push("ReplayGain");
        }
        if dop {

            reasons.clear();
        }
        let active_fx: Vec<&str> = self.cfg.chain.iter().filter(|s| s.enabled).map(|s| crate::fx::info(s.kind).name).collect();
        if !active_fx.is_empty() {
            reasons.push("modül zinciri");
        }
        ui.horizontal(|ui| {
            section_label(ui, "Sinyal yolu");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if reasons.is_empty() {
                    chip(ui, "BIT-PERFECT", GREEN);
                }
            });
        });
        ui.add_space(4.0);
        let src = if self.cfg.use_loopback {
            "Sistem sesi (loopback)".to_string()
        } else if self.meta.codec.is_empty() {
            "—".to_string()
        } else {
            format!("{} · {:.1} kHz / {} bit", self.meta.codec.to_uppercase(), self.meta.src_rate as f32 / 1000.0, self.meta.bits)
        };
        path_row(ui, "Kaynak", &src, false);
        if self.meta.resampled && !self.cfg.use_loopback {
            path_row(ui, "Yeniden örnekleme", &format!("→ {:.1} kHz", self.player.rate() as f32 / 1000.0), false);
        }
        path_row(
            ui,
            "İşleme",
            &if dop {
                "DoP: EQ, seviye ve modüller devre dışı".to_string()
            } else if eq_active {
                format!("EQ · {} bant", self.preset.bands.len())
            } else {
                "EQ yok".into()
            },
            false,
        );
        if !active_fx.is_empty() && !dop {
            path_row(ui, "Modüller", &active_fx.iter().map(crate::i18n::tr).collect::<Vec<_>>().join(" → "), false);
        }
        if let (Some(db), false) = (self.player.rg_applied_db(), self.cfg.use_loopback) {
            path_row(ui, "ReplayGain", &format!("{db:+.1} dB · {}", if self.cfg.rg_mode == 2 { "albüm" } else { "parça" }), false);
        }
        let outs = match &out {
            Some(o) => format!(
                "{} · {:.1} kHz / {}{}",
                if o.exclusive { "Exclusive" } else { "Shared" },
                o.rate as f32 / 1000.0,
                o.bits,
                if o.float { " float" } else if o.dop { " bit · DoP" } else { " bit" }
            ),
            None => "Motor kapalı".into(),
        };
        path_row(ui, "Çıkış", &outs, true);
        if !reasons.is_empty() {
            ui.label(RichText::new(format!("Bit-perfect değil: {}", reasons.join(", "))).font(body(11.0)).color(DIM));
        }
    }

    pub(super) fn motor_view(&mut self, ctx: &egui::Context, fade: f32, ab_held: &mut bool) {
        let inner = Margin { left: 28.0, right: 28.0, top: 20.0, bottom: 18.0 };
        egui::CentralPanel::default()
            .frame(panel_frame(inner, Margin { left: 12.0, right: 0.0, top: 2.0, bottom: 2.0 }))
            .show(ctx, |ui| {
                ui.set_opacity(fade);
                ui.horizontal(|ui| {
                    let mut t = self.motor_tab;
                    segmented(ui, &mut t, &[(0u8, "Ekolayzır", None), (1u8, "Modüller", None)]);
                    self.motor_tab = t;
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let on = !engine::EQ_OFF.load(Relaxed);
                        let tip = if on { "Ekolayzırı devre dışı bırakır; sinyal EQ'dan geçmez" } else { "Ekolayzırı yeniden açar" };
                        if pill_button(ui, if on { "Ekolayzır açık" } else { "Ekolayzır kapalı" }, Some(Icon::Power), on).on_hover_text_tr(tip).clicked() {
                            engine::EQ_OFF.store(on, Relaxed);
                        }

                        self.auto_eq_chip(ui);
                    });
                });
                ui.add_space(10.0);
                if self.motor_tab == 1 {
                    self.fx_view(ui, ctx);
                    return;
                }
                self.preset_strip(ui);
                ui.add_space(8.0);
                let total = ui.available_height();
                let fader_h = 340.0;
                let viz_h = (total - fader_h - 10.0).max(190.0);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.allocate_ui(vec2(ui.available_width(), viz_h), |ui| {
                        card(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.set_height(viz_h - 26.0);
                            ui.horizontal(|ui| {
                                let before = self.viz_mode;
                                segmented(
                                    ui,
                                    &mut self.viz_mode,
                                    &[
                                        (VizMode::Spectrum, "Spektrum", None),
                                        (VizMode::Spectrogram, "Spektrogram", None),
                                        (VizMode::Scope, "Vektörskop", None),
                                    ],
                                );
                                if before != self.viz_mode {
                                    self.cfg.viz_mode = match self.viz_mode {
                                        VizMode::Spectrum => 0,
                                        VizMode::Spectrogram => 1,
                                        VizMode::Scope => 2,
                                    };
                                }
                                if self.viz_mode == VizMode::Spectrum {
                                    let mut bars = self.cfg.viz_bars;
                                    segmented(ui, &mut bars, &[(false, "Dalga", None), (true, "1/3 oktav", None)]);
                                    self.cfg.viz_bars = bars;
                                }
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    Self::ab_button(ui, ab_held);
                                });
                            });
                            ui.add_space(6.0);
                            let size = ui.available_size();
                            match self.viz_mode {
                                VizMode::Spectrum => self.eq_graph(ui, size),
                                VizMode::Spectrogram => {
                                    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                                    let painter = ui.painter_at(rect);
                                    self.viz.draw_spectrogram(ui.ctx(), &painter, rect);
                                    viz::draw_freq_grid(&painter, rect);
                                }
                                VizMode::Scope => {
                                    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                                    let painter = ui.painter_at(rect);
                                    painter.rect_filled(rect, Rounding::same(8.0), BG0);
                                    self.viz.draw_scope(&painter, rect);
                                }
                            }
                        });
                    });
                    ui.add_space(10.0);
                    card(ui, |ui| {
                        ui.set_width(ui.available_width());
                        self.band_faders(ui);
                    });
                ui.add_space(96.0);
});
            });
    }
}
