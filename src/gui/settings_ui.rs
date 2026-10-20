//! Ayarlar görünümü.

use super::*;

impl App {
    pub(super) fn settings_view(&mut self, ctx: &egui::Context, fade: f32) {
        let inner = Margin { left: 40.0, right: 40.0, top: 20.0, bottom: 20.0 };
        egui::CentralPanel::default()
            .frame(panel_frame(inner, Margin { left: 12.0, right: 12.0, top: 2.0, bottom: 2.0 }))
            .show(ctx, |ui| {
                self.paint_glow(ui, inner);
                ui.set_opacity(fade);
                let running = self.running.is_some();
                let cfg_before = self.cfg.clone();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Ayarlar").font(bold(20.0)));
                        if running {
                            ui.add_space(10.0);
                            ui.label(RichText::new("Motor çalışırken kaynak ve çıkış ayarları kilitli").font(body(12.5)).color(AMBER));
                        }
                    });
                    ui.set_max_width(ui.available_width().min(780.0));
                    category(ui, "Ses");
                    self.settings_left(ui, running);
                    self.settings_right(ui);
                    ui.add_space(12.0);
                });
                if self.cfg != cfg_before {
                    self.save_cfg();
                }
            });
    }

    pub(super) fn settings_left(&mut self, ui: &mut egui::Ui, running: bool) {
        ui.add_enabled_ui(!running, |ui| {
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                section_label(ui, "Çıkış");
                ui.add_space(2.0);
                ui.label(RichText::new("Hedef cihaz").font(body(12.5)).color(DIM));
                ui.horizontal(|ui| {
                    Self::device_combo(ui, "to", &self.devices, &mut self.cfg.to, "Windows varsayılan çıkışı", 44.0);
                    if icon_button(ui, Icon::Undo, 28.0, false).on_hover_text("Cihazları yenile").clicked() {
                        self.devices = engine::list_devices().unwrap_or_default();
                    }
                });
                ui.add_space(2.0);
                let mut excl = !self.cfg.exclusive_off;
                if switch_row(ui, "Exclusive mod", "Dolby Atmos ve Windows ses efektlerini atlar", &mut excl) {
                    self.cfg.exclusive_off = !excl;
                }
            });
            ui.add_space(10.0);
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                section_label(ui, "Kaynak");
                ui.add_space(2.0);
                let mut lb = self.cfg.use_loopback;
                segmented(ui, &mut lb, &[(false, "Dahili oynatıcı", None), (true, "Sistem DSP", None)]);
                self.cfg.use_loopback = lb;
                if lb {
                    ui.add_space(6.0);
                    let virt = engine::find_virtual();
                    ui.horizontal(|ui| match &virt {
                        Some(d) => {
                            chip(ui, "SANAL CİHAZ HAZIR", GREEN);
                            ui.label(RichText::new(&d.name).font(body(12.5)));
                        }
                        None => {
                            chip(ui, "SANAL CİHAZ YOK", AMBER);
                            if pill_button(ui, "VB-Cable indir", Some(Icon::Import), true).clicked() {
                                let _ = std::process::Command::new("explorer").arg("https://vb-audio.com/Cable/").spawn();
                            }
                        }
                    });
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(
                            "Windows varsayılan çıkışı sanal cihaza yönlenir; tüm uygulamaların sesi oraya gider. \
                             Soundworks bunu yakalar, EQ uygular ve gerçek cihaza exclusive verir.",
                        )
                        .font(body(12.0))
                        .color(DIM),
                    );
                    ui.add_space(4.0);
                    switch_row(ui, "Çıkışı otomatik yönlendir", "Motor kapanınca eski cihaz geri yüklenir", &mut self.cfg.auto_route);
                    ui.add_space(2.0);
                    ui.label(RichText::new("Yakalanacak cihaz").font(body(12.5)).color(DIM));
                    Self::device_combo(ui, "from", &self.devices, &mut self.cfg.from, "Otomatik (sanal cihaz)", 0.0);
                } else {
                    ui.add_space(4.0);
                    ui.label(RichText::new("Kendi çalarını kullanırsın: dosya ya da klasör aç.").font(body(12.0)).color(DIM));
                }
            });
            ui.add_space(10.0);
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                section_label(ui, "Ses kalitesi");
                ui.add_space(2.0);
                switch_row(
                    ui,
                    "Hızı parçaya göre ayarla",
                    "Cihaz destekliyorsa çıkış hızı parçanın hızına çevrilir, yeniden örnekleme olmaz. Hızı farklı iki parça arasında kısa bir ara olur.",
                    &mut self.cfg.match_rate,
                );
                ui.add_space(10.0);
                ui.label(RichText::new(if self.cfg.match_rate { "Sabit çıkış hızı (cihaz parça hızını desteklemezse)" } else { "Sabit çıkış hızı" }).font(body(12.5)).color(DIM));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for (r, n) in RATES {
                        if tab(ui, n, self.cfg.player_rate == r).clicked() {
                            self.cfg.player_rate = r;
                        }
                    }
                });
                ui.label(RichText::new("Dosya hızıyla eşleşirse yeniden örnekleme yapılmaz.").font(body(12.0)).color(DIM));
            });
            ui.add_space(10.0);
            switch_row(ui, "Dither (16 bit çıkış)", "Yalnızca sinyale dokunulduysa (EQ, ses seviyesi) uygulanır; bit-perfect yolda eklenmez", &mut self.cfg.dither);
        });
    }

    pub(super) fn settings_right(&mut self, ui: &mut egui::Ui) {
        self.dsd_card(ui);
        ui.add_space(10.0);
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            section_label(ui, "Ses dengeleme (ReplayGain)");
            ui.add_space(2.0);
            ui.label(RichText::new("Dosya etiketlerindeki ReplayGain/R128 değerine göre albümler arası ses farkını dengeler. Etiketsiz parçaya dokunmaz. Kapalıyken sinyal olduğu gibi geçer.").font(body(12.0)).color(DIM));
            ui.add_space(8.0);
            segmented(ui, &mut self.cfg.rg_mode, &[(0u32, "Kapalı", None), (1u32, "Parça", None), (2u32, "Albüm", None)]);
            if self.cfg.rg_mode != 0 {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Ön kazanç").font(body(13.5)).color(TEXT));
                    ui.spacing_mut().slider_width = 200.0;
                    ui.add(egui::Slider::new(&mut self.cfg.rg_preamp, -12.0..=6.0).fixed_decimals(1).suffix(" dB"));
                });
                ui.add_space(6.0);
                switch_row(ui, "Kırpılmayı önle", "Etiketteki tepe değerine bakıp kazancı, sinyal 0 dBFS'i aşmayacak kadarla sınırlar.", &mut self.cfg.rg_prevent);
                ui.add_space(8.0);
                let total: usize = self.lib.albums.iter().map(|a| a.tracks.len()).sum();
                let running = self.loud.running.load(std::sync::atomic::Ordering::Relaxed);
                if !running && self.loud_cached_t.elapsed() > std::time::Duration::from_secs(2) {
                    let paths: Vec<String> = self.lib.albums.iter().flat_map(|a| a.tracks.iter().map(|t| t.path.clone())).collect();
                    self.loud_cached = crate::loudness::cached_count(&paths);
                    self.loud_cached_t = Instant::now();
                }
                if running {
                    let (d, t) = (self.loud.done.load(std::sync::atomic::Ordering::Relaxed), self.loud.total.load(std::sync::atomic::Ordering::Relaxed).max(1));
                    ui.label(RichText::new(format!("Etiketi olmayan parçaların ses seviyesi ölçülüyor… {d}/{t}")).font(body(12.0)).color(DIM));
                    ui.add_space(4.0);
                    ui.add(egui::ProgressBar::new(d as f32 / t as f32).desired_height(6.0));
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
                } else {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{}/{} parçanın ses seviyesi biliniyor", self.loud_cached, total)).font(body(12.0)).color(DIM));
                        if self.loud_cached < total {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if pill_button(ui, "Şimdi ölç", Some(Icon::Wave), false).clicked() {
                                    self.start_loudness();
                                }
                            });
                        }
                    });
                }
            }
        });
        ui.add_space(10.0);
        category(ui, "Kütüphane");
        self.library_folders_card(ui);
        ui.add_space(10.0);
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            section_label(ui, "Albüm türleri");
            ui.add_space(2.0);
            switch_row(
                ui,
                "Türleri internetten çek",
                "MusicBrainz ve Deezer'dan; dosya etiketleri yanlışken daha doğru sonuç verir",
                &mut self.cfg.fetch_genres,
            );
            ui.add_space(4.0);
            switch_row(ui, "Kütüphaneyi türe göre grupla", "Albümler tür başlıkları altında listelenir", &mut self.cfg.group_by_genre);
            let total = self.lib.albums.len();
            let done = self.lib.albums.iter().filter(|a| a.genre_checked).count();
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{done}/{total} albüm işlendi")).font(body(12.5)).color(DIM));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if pill_button(ui, "Yeniden çek", Some(Icon::Undo), false).on_hover_text("Tüm albümlerin türünü silip baştan alır").clicked() {
                        for a in self.lib.albums.iter_mut() {
                            a.genre.clear();
                            a.genre_checked = false;
                        }
                        self.genre_retry.clear();
                        self.lib.save();
                    }
                });
            });
            if self.cfg.fetch_genres && total > 0 && done < total {
                ui.add_space(4.0);
                ui.add(egui::ProgressBar::new(done as f32 / total as f32).desired_height(6.0));
            }
        });
        ui.add_space(10.0);
        category(ui, "Ekolayzır");
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                section_label(ui, "Presetler");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if pill_button(ui, "Klasörü aç", Some(Icon::Folder), false).clicked() {
                        let _ = std::fs::create_dir_all(presets_dir());
                        let _ = std::process::Command::new("explorer").arg(presets_dir()).spawn();
                    }
                });
            });
            ui.add_space(2.0);
            if self.library.is_empty() {
                ui.label(RichText::new("Henüz preset yok").font(body(12.5)).color(DIM));
            }
            let mut del = None;
            for (n, p) in &self.library {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(n.trim_end_matches(".txt")).font(body(13.0)));
                    ui.label(RichText::new(format!("{} bant", p.bands.len())).font(body(12.0)).color(DIM));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_button(ui, Icon::Trash, 26.0, false).on_hover_text("Sil").clicked() {
                            del = Some(n.clone());
                        }
                    });
                });
            }
            if let Some(n) = del {
                let _ = std::fs::remove_file(presets_dir().join(&n));
                if self.preset_name.as_ref() == Some(&n) {
                    self.preset_name = None;
                    self.cfg.preset = None;
                }
                self.refresh_library();
            }
        });
        ui.add_space(10.0);
        category(ui, "Uygulama");
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            section_label(ui, "Genel");
            ui.add_space(2.0);
            let mut lm = !self.cfg.level_match_off;
            if switch_row(ui, "A/B'de seviye eşle", "EQ kapalı ses preamp kadar kısılır", &mut lm) {
                self.cfg.level_match_off = !lm;
            }
            ui.add_space(4.0);
            switch_row(ui, "Açılışta motoru başlat", "", &mut self.cfg.autostart);
            ui.add_space(4.0);
            switch_row(ui, "Kapatınca tepside çalışmaya devam et", "Sistem DSP için önerilir", &mut self.cfg.tray_on_close);
            ui.add_space(4.0);
            let mut login = self.cfg.run_at_login;
            if switch_row(ui, "Windows ile birlikte başlat", "Tepside, arka planda açılır", &mut login) {
                self.cfg.run_at_login = login;
                set_run_at_login(login);
                if login {
                    self.cfg.autostart = true;
                }
            }
        });        ui.add_space(10.0);
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            section_label(ui, "Görünüm");
            ui.add_space(4.0);
            ui.label(RichText::new("Arayüz boyutu").font(body(14.5)).color(Color32::WHITE));
            ui.label(RichText::new("Otomatik, Windows ekran ölçeğine göre seçer").font(body(12.0)).color(DIM));
            ui.add_space(4.0);
            segmented(ui, &mut self.cfg.ui_zoom, &[(0, "Otomatik", None), (70, "%70", None), (80, "%80", None), (90, "%90", None), (100, "%100", None), (115, "%115", None)]);
        });
        ui.add_space(10.0);
        self.tips_settings(ui);
    }

    pub(super) fn device_combo(ui: &mut egui::Ui, id: &str, devices: &[DeviceInfo], value: &mut Option<String>, none_label: &str, reserve: f32) {
        let shown = match value.as_ref() {
            None => none_label.to_string(),
            Some(v) => devices
                .iter()
                .find(|d| &d.id == v)
                .map(|d| d.name.clone())
                .unwrap_or_else(|| "Kayıtlı cihaz (bağlı değil)".to_string()),
        };
        let w = (ui.available_width() - reserve - 8.0).clamp(180.0, 620.0);
        egui::ComboBox::from_id_salt(id).width(w).selected_text(shown).show_ui(ui, |ui| {
            ui.selectable_value(value, None, none_label);
            for d in devices {
                ui.selectable_value(value, Some(d.id.clone()), &d.name);
            }
        });
    }
}

/// Ayarlar sayfasında kategori başlığı: kalın başlık ve altında ince çizgi.
fn category(ui: &mut egui::Ui, text: &str) {
    ui.add_space(14.0);
    ui.label(RichText::new(text).font(bold(18.0)).color(Color32::WHITE));
    ui.add_space(2.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, alpha(DIM, 70));
    ui.add_space(8.0);
}
