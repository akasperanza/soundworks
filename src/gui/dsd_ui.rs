use super::*;

impl App {

    pub(super) fn dsd_eligible(&self, kind: engine::DeviceKind) -> bool {
        kind == engine::DeviceKind::Usb && !self.cfg.exclusive_off && self.cfg.match_rate && !self.cfg.use_loopback
    }

    pub(super) fn refresh_dsd_policy(&mut self) {
        let mut rates: Vec<u32> = vec![];
        let allowed = match engine::resolve(self.cfg.to.as_deref()) {
            Ok(d) if self.dsd_eligible(d.kind) && self.cfg.dsd_prefs.get(&d.id) == Some(&1) => {
                rates = self.dop_rates_for(&d.id);

                !rates.is_empty()
            }
            _ => false,
        };
        self.player.set_dop_rates(rates);
        self.player.set_dop_allowed(allowed);
    }

    pub(super) fn dop_rates_for(&mut self, dev_id: &str) -> Vec<u32> {
        if let Some(r) = self.dop_rate_cache.get(dev_id) {
            return r.clone();
        }
        let r = engine::dop_rates(dev_id);
        self.dop_rate_cache.insert(dev_id.to_string(), r.clone());
        r
    }

    pub(super) fn dsd_gate(&mut self, files: &[PathBuf], start: usize) -> bool {
        let Some(first) = files.get(start) else { return false };
        if !crate::dsd::is_dsd(first) || self.dsd_dialog.is_some() {
            return false;
        }
        let Ok(dev) = engine::resolve(self.cfg.to.as_deref()) else { return false };
        if !self.dsd_eligible(dev.kind) {
            return false;
        }
        if self.cfg.dsd_prefs.contains_key(&dev.id) {
            self.refresh_dsd_policy();
            return false;
        }
        let Ok(info) = crate::dsd::probe(first) else { return false };
        let rate = info.dop_rate();
        if !self.dop_rates_for(&dev.id).contains(&rate) {

            return false;
        }
        self.dsd_dialog = Some(DsdDialog { files: files.to_vec(), start, dev_id: dev.id, dev_name: dev.name, dop_rate: rate, t0: Instant::now(), probe: DsdProbe::Idle });
        true
    }

    pub(super) fn dsd_choose(&mut self, dop: bool) {
        let Some(d) = self.dsd_dialog.take() else { return };
        self.cfg.dsd_prefs.insert(d.dev_id.clone(), dop as u8);
        self.save_cfg();
        self.refresh_dsd_policy();
        if d.files.len() == self.player.playlist.lock().unwrap().len() && self.player.playlist.lock().unwrap().iter().eq(d.files.iter()) {
            self.play_index(d.start);
        } else {
            self.play_paths(d.files, d.start);
        }
    }

    pub(super) fn tick_dsd_volume_note(&mut self) {
        let v = self.player.volume();
        let dop = self.running.as_ref().is_some_and(|r| r.ctl.out_info.lock().unwrap().dop);
        if dop && self.last_vol >= 0.0 && (v - self.last_vol).abs() > 1e-4 {
            self.say("DoP'ta yazılım ses seviyesi çalışmaz; sesi DAC'ından ayarla");
        }
        self.last_vol = v;
    }

    pub(super) fn dsd_dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(d) = self.dsd_dialog.as_mut() else { return };

        if let DsdProbe::Running(rx) = &d.probe {
            match rx.try_recv() {
                Ok(Ok(())) => d.probe = DsdProbe::Ask,
                Ok(Err(e)) => d.probe = DsdProbe::Failed(e),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => d.probe = DsdProbe::Failed("test tamamlanamadı".into()),
                Err(_) => {}
            }
            ctx.request_repaint();
        }
        let elapsed = d.t0.elapsed().as_secs_f32();
        let left = (DOP_LOCK_SECS - elapsed).max(0.0);
        if left > 0.0 {
            ctx.request_repaint();
        }
        let dev_name = d.dev_name.clone();
        let rate = d.dop_rate;
        let dev_id = d.dev_id.clone();
        let mut action: Option<u8> = None;
        let busy = matches!(d.probe, DsdProbe::Running(_));
        let ask = matches!(d.probe, DsdProbe::Ask);
        let failed = if let DsdProbe::Failed(e) = &d.probe { Some(e.clone()) } else { None };

        egui::Area::new(Id::new("dsd_dim")).order(egui::Order::Foreground).fixed_pos(pos2(0.0, 0.0)).show(ctx, |ui| {
            let r = ctx.screen_rect();
            ui.allocate_rect(r, Sense::click_and_drag());
            ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha(170));
        });
        egui::Window::new(crate::i18n::tr("DSD dosyası"))
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Tooltip)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .fixed_size(vec2(540.0, 0.0))
            .show(ctx, |ui| {
                ui.add(egui::Label::new(RichText::new(format!("{dev_name} cihazında DSD nasıl çalınsın?")).font(bold(15.0))).wrap());
                ui.add_space(10.0);

                ui.add(egui::Label::new(RichText::new("DoP nedir?").font(bold(13.5)).color(Color32::WHITE)).wrap());
                ui.add_space(2.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(format!(
                            "DoP (DSD over PCM), DSD müziğini hiç dönüştürmeden DAC'a taşımanın bir yoludur. DSD'nin 1 bitlik verisi, sıradan bir PCM ses akışının ({:.1} kHz, 24 bit) içine paketlenir ve DAC onu tekrar DSD olarak açar. Böylece ses, yazılımda PCM'e çevrilmeden doğrudan DAC'ın DSD çözücüsüne gider.",
                            rate as f32 / 1000.0
                        ))
                        .font(body(12.5))
                        .color(TEXT),
                    )
                    .wrap(),
                );
                ui.add_space(10.0);

                ui.add(egui::Label::new(RichText::new("DoP ne zaman, nasıl kullanılmalı?").font(bold(13.5)).color(Color32::WHITE)).wrap());
                ui.add_space(2.0);
                ui.add(
                    egui::Label::new(
                        RichText::new("DAC'ının DoP'u desteklediğinden eminsen (kılavuzunda ya da üreticinin sayfasında \"DoP\" yazar) ve DSD'yi hiç işlenmeden, olduğu gibi dinlemek istiyorsan. Ses seviyesini Soundworks'ten değil, DAC'ından ya da amfinden ayarlarsın. Emin değilsen önce aşağıdaki \"DoP desteğini kontrol et\" düğmesini kullan. Başka durumlarda \"PCM'e çevirerek çal\" seçeneği her cihazda sorunsuz çalışır ve EQ ile ses seviyesi de kullanılabilir.")
                            .font(body(12.5))
                            .color(TEXT),
                    )
                    .wrap(),
                );
                ui.add_space(10.0);

                Frame::none().fill(alpha(RED, 45)).stroke(Stroke::new(1.0, alpha(RED, 160))).rounding(Rounding::same(8.0)).inner_margin(Margin::same(12.0)).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new("DoP'U NEDEN HER ZAMAN KULLANMAMALISIN?").font(bold(13.5)).color(Color32::WHITE)).wrap());
                    ui.add_space(4.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new("DAC'ın DoP'u bilmiyorsa, DSD verisini normal müzik sanır ve çok yüksek seviyeli bir gürültü çalar. Bu gürültü kulağına, kulaklığına ve hoparlörüne zarar verebilir.")
                                .font(bold(12.5))
                                .color(Color32::WHITE),
                        )
                        .wrap(),
                    );
                    ui.add_space(4.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new("DoP'ta Soundworks'ün ses seviyesi, EQ, ReplayGain ve limiter çalışmaz; bu yüzden yanlışlıkla çok yüksek sesle başlatmak kolaydır. DoP seçeceksen önce DAC'ının ve amfinin sesini en aza indir.")
                                .font(bold(12.5))
                                .color(Color32::WHITE),
                        )
                        .wrap(),
                    );
                });
                ui.add_space(12.0);

                if busy {
                    ui.add(egui::Label::new(RichText::new("DAC'a DoP sessizliği gönderiliyor (yaklaşık 3 sn)… DAC'ının ekranına / DSD ışığına bak.").font(body(12.5)).color(AMBER)).wrap());
                } else if ask {
                    ui.add(egui::Label::new(RichText::new("DAC'ın ekranında DSD (ör. DSD64) göründü ya da DSD ışığı yandı mı?").font(bold(13.0))).wrap());
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        if pill_button(ui, "Evet, tanıdı: DoP ile çal", None, true).clicked() {
                            action = Some(1);
                        }
                        if pill_button(ui, "Hayır: PCM'e çevirerek çal", None, false).clicked() {
                            action = Some(0);
                        }
                    });
                } else {
                    if let Some(e) = &failed {
                        ui.add(egui::Label::new(RichText::new(format!("DoP testi yapılamadı: {e}")).font(body(12.5)).color(AMBER)).wrap());
                        ui.add_space(6.0);
                    }
                    ui.horizontal_wrapped(|ui| {
                        if pill_button(ui, "PCM'e çevirerek çal (güvenli)", None, true).clicked() {
                            action = Some(0);
                        }
                        if failed.is_none() && locked_pill_button(ui, "DoP ile çal", &format!("{:.0}", left.ceil()), left / DOP_LOCK_SECS).clicked() {
                            action = Some(1);
                        }
                        if failed.is_none() && pill_button(ui, "DoP desteğini kontrol et", None, false).on_hover_text_tr("DAC'a yalnızca DSD sessizliği gönderir (müzik yok)").clicked() {
                            action = Some(2);
                        }
                    });
                }
                ui.add_space(4.0);
                if !busy && ui.add(egui::Label::new(RichText::new("Vazgeç").font(body(12.0)).color(SKY)).sense(Sense::click())).clicked() {
                    action = Some(3);
                }
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !busy {
            action = Some(3);
        }
        match action {
            Some(0) => self.dsd_choose(false),
            Some(1) => self.dsd_choose(true),
            Some(2) => {

                self.stop();
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(engine::dop_probe(&dev_id, rate, 3.0).map_err(|e| engine::explain_error(&e)));
                });
                if let Some(d) = self.dsd_dialog.as_mut() {
                    d.probe = DsdProbe::Running(rx);
                }
            }
            Some(_) => self.dsd_dialog = None,
            None => {}
        }
    }

    pub(super) fn dsd_card(&mut self, ui: &mut egui::Ui) {
        let mut reset: Option<String> = None;
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            section_label(ui, "DSD çalma");
            ui.add_space(2.0);
            ui.add(egui::Label::new(RichText::new("DSF ve DFF dosyaları varsayılan olarak PCM'e çevrilerek çalınır. DoP (bit-perfect DSD) yalnızca USB DAC'larda, ilk DSD dosyasında sorulduktan sonra ve senin onayınla kullanılır; Bluetooth, HDMI ve dahili ses cihazlarında hiç sunulmaz.").font(body(12.0)).color(DIM)).wrap());
            ui.add_space(6.0);
            if self.cfg.dsd_prefs.is_empty() {
                ui.label(RichText::new("Henüz kayıtlı bir tercih yok").font(body(12.5)).color(DIM));
            }
            for (id, mode) in self.cfg.dsd_prefs.clone() {
                let name = self.devices.iter().find(|d| d.id == id).map(|d| d.name.clone()).unwrap_or_else(|| "Bağlı olmayan cihaz".to_string());
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 150.0).max(100.0);
                    ui.allocate_ui(vec2(w, 0.0), |ui| {
                        ui.add(egui::Label::new(RichText::new(format!("{name} · {}", if mode == 1 { "DoP" } else { "PCM'e çevir" })).font(body(12.5)).color(TEXT)).wrap());
                    });
                    if pill_button(ui, "Yeniden sor", Some(Icon::Undo), false).on_hover_text_tr("Bu cihaz için bir sonraki DSD dosyasında tekrar sorulur").clicked() {
                        reset = Some(id.clone());
                    }
                });
            }
        });
        if let Some(id) = reset {
            self.cfg.dsd_prefs.remove(&id);
            self.refresh_dsd_policy();
        }
    }
}
