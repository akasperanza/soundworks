use super::*;
use crate::onlinemeta::{self, WriteReport};
use crate::tagwrite::{self, WriteLogEntry};
use std::time::Duration;

const BATCH: usize = 6;
const ERROR_PAUSE: Duration = Duration::from_secs(300);
const WRITE_MODE_ASK: u8 = 0;
const WRITE_MODE_ALWAYS: u8 = 1;
const WRITE_MODE_NEVER: u8 = 2;

fn epoch_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn merge_log(log: &mut Vec<WriteLogEntry>, add: Vec<WriteLogEntry>) {
    for e in add {
        match log.iter_mut().find(|x| x.path == e.path) {
            Some(x) => {
                for f in e.fields {
                    x.fields.retain(|(k, _)| *k != f.0);
                    x.fields.push(f);
                }
            }
            None => log.push(e),
        }
    }
}

impl App {
    pub(super) fn drive_online_meta(&mut self, ctx: &egui::Context) {
        let mut changed = false;
        for (path, res) in self.meta_fetcher.poll() {
            match res {
                Ok(Some(m)) => {
                    self.lib.online.insert(path.clone(), m);
                    self.lib.apply_online(&path);
                    if tagwrite::can_write(std::path::Path::new(&path)) {
                        self.meta_found.push(path);
                    }
                    changed = true;
                }
                Ok(None) => {
                    self.lib.online_tried.insert(path, epoch_now());
                    changed = true;
                }
                Err(_) => self.meta_pause = Some(Instant::now()),
            }
        }
        if changed {
            if self.lib_open.as_ref().map_or(false, |k| !self.lib.albums.iter().any(|a| a.key == *k)) {
                self.lib_open = None;
            }
            self.lib_tex.retain(|_, v| v.is_some());
            self.meta_unsaved += 1;
        }
        for (key, res) in self.cover_fetcher.poll() {
            match res {
                Ok(found) => {
                    self.lib.cover_tried.insert(key.clone(), epoch_now());
                    if found {
                        self.lib_tex.remove(&key);
                        self.bd_images.remove(&format!("album|{key}"));
                    }
                    if self.cover_manual.remove(&key) {
                        self.say(if found { "Kapak bulundu" } else { "Bu albüm için uygun bir kapak bulunamadı" });
                    }
                    self.meta_unsaved += 1;
                }
                Err(_) => {
                    if self.cover_manual.remove(&key) {
                        self.say("İnternete ulaşılamadı");
                    } else {
                        self.meta_pause = Some(Instant::now());
                    }
                }
            }
        }
        if self.meta_pause.map_or(false, |t| t.elapsed() >= ERROR_PAUSE) {
            self.meta_pause = None;
        }
        if !self.cfg.meta_online {
            self.meta_fetcher.cancel_all();
        } else if self.meta_pause.is_none() && self.lib_scans.is_empty() {
            let queued = self.meta_fetcher.queue_len();
            if queued < BATCH {
                let jobs = self.lib.online_jobs(self.cfg.meta_all, BATCH - queued, |p| self.meta_fetcher.is_pending(p));
                for (path, hint) in jobs {
                    self.meta_fetcher.request(path, hint);
                }
            }
        }
        if self.cfg.meta_online && self.cfg.meta_covers && self.meta_pause.is_none() && self.lib_scans.is_empty() && self.cover_fetcher.queue_len() < 2 && self.cover_check.elapsed() >= Duration::from_secs(2) {
            self.cover_check = Instant::now();
            for (key, artist, album) in self.lib.cover_jobs(2, |k| self.cover_fetcher.is_pending(k)) {
                self.cover_fetcher.request(key, artist, album);
            }
        }
        if self.meta_unsaved > 0 && (self.meta_unsaved >= 10 || (self.meta_fetcher.queue_len() == 0 && self.cover_fetcher.queue_len() == 0)) {
            self.lib.save();
            self.meta_unsaved = 0;
        }
        self.poll_meta_write();
        if self.meta_batch_ready() {
            match self.cfg.meta_write {
                WRITE_MODE_ALWAYS => self.write_found_to_files(),
                WRITE_MODE_NEVER => self.meta_found.clear(),
                _ => {}
            }
        }
        if self.meta_fetcher.queue_len() > 0 || self.cover_fetcher.queue_len() > 0 || self.meta_write_rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    fn meta_batch_ready(&self) -> bool {
        !self.meta_found.is_empty() && self.meta_write_rx.is_none() && (self.meta_fetcher.queue_len() == 0 || self.meta_found.len() >= 25)
    }

    fn poll_meta_write(&mut self) {
        let Some(rx) = &self.meta_write_rx else { return };
        let Ok(WriteReport { entries, failed, undone }) = rx.try_recv() else { return };
        self.meta_write_rx = None;
        let n = entries.len();
        if undone {
            self.meta_log.retain(|e| !entries.iter().any(|r| r.path == e.path));
        } else {
            merge_log(&mut self.meta_log, entries);
        }
        onlinemeta::save_log(&self.meta_log);
        let msg = match (undone, failed) {
            (false, 0) => format!("{n} dosyaya yazıldı"),
            (false, f) => format!("{n} dosyaya yazıldı, {f} dosyaya yazılamadı"),
            (true, 0) => format!("{n} dosyadaki değişiklik geri alındı"),
            (true, f) => format!("{n} dosyadaki değişiklik geri alındı, {f} dosya geri alınamadı"),
        };
        self.say(msg);
    }

    fn write_found_to_files(&mut self) {
        let paths = std::mem::take(&mut self.meta_found);
        let jobs: Vec<_> = paths.into_iter().filter_map(|p| self.lib.online.get(&p).cloned().map(|m| (p, m))).collect();
        if !jobs.is_empty() {
            self.meta_write_rx = Some(onlinemeta::spawn_write(jobs));
        }
    }

    fn write_all_online_to_files(&mut self) {
        if self.meta_write_rx.is_some() {
            return;
        }
        let jobs: Vec<_> = self.lib.online.iter().filter(|(p, _)| tagwrite::can_write(std::path::Path::new(p))).map(|(p, m)| (p.clone(), m.clone())).collect();
        self.meta_found.clear();
        if jobs.is_empty() {
            self.say("Dosyaya yazılacak bir şey yok");
        } else {
            self.meta_write_rx = Some(onlinemeta::spawn_write(jobs));
        }
    }

    pub(super) fn meta_banner(&mut self, ctx: &egui::Context) {
        let visible = self.cfg.meta_write == WRITE_MODE_ASK && self.meta_batch_ready();
        let t = ctx.animate_bool_with_time(Id::new("meta_banner_vis"), visible, 0.28);
        if visible {
            self.meta_banner_n = self.meta_found.len();
        }
        if t <= 0.01 {
            return;
        }
        let e = ease(t);
        let screen = ctx.screen_rect();
        let avail = ctx.available_rect();
        let w = (avail.width() - 60.0).clamp(320.0, 620.0);
        let n = self.meta_banner_n;
        let lift = if self.cfg.tips_on && self.tip_cur.is_some() { 130.0 } else { 0.0 };
        let (mut write, mut later, mut never) = (false, false, false);
        egui::Area::new(Id::new("meta_banner"))
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_BOTTOM, vec2(avail.center().x - screen.center().x, -(screen.bottom() - avail.bottom()) - 96.0 - lift + (1.0 - e) * 18.0))
            .show(ctx, |ui| {
                ui.set_opacity(e);
                Frame::none()
                    .fill(BG3)
                    .stroke(Stroke::new(1.0_f32, alpha(SKY, 90)))
                    .rounding(Rounding::same(16.0))
                    .inner_margin(Margin::same(18.0))
                    .shadow(egui::epaint::Shadow { offset: vec2(0.0, 10.0), blur: 28.0, spread: 0.0, color: Color32::from_black_alpha(110) })
                    .show(ui, |ui| {
                        ui.set_width(w - 36.0);
                        ui.spacing_mut().item_spacing.y = 5.0;
                        let title = if n == 1 { "1 parça için metadata bulundu".to_string() } else { format!("{n} parça için metadata bulundu") };
                        ui.label(RichText::new(crate::i18n::tr(title)).font(bold(14.5)).color(Color32::WHITE));
                        ui.add(
                            egui::Label::new(
                                RichText::new(crate::i18n::tr("Bilgiler kütüphanede zaten görünüyor. Dosyaların etiketlerine de yazalım mı? Yalnızca boş alanlar doldurulur, mevcut etiketlere dokunulmaz."))
                                    .font(body(13.0))
                                    .color(TEXT),
                            )
                            .wrap(),
                        );
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            write = pill_button(ui, "Dosyalara yaz", None, true).clicked();
                            later = pill_button(ui, "Şimdi değil", None, false).clicked();
                            never = pill_button(ui, "Hiç sorma", None, false).clicked();
                        });
                    });
            });
        if write {
            self.write_found_to_files();
        } else if later {
            self.meta_found.clear();
        } else if never {
            self.meta_found.clear();
            self.cfg.meta_write = WRITE_MODE_NEVER;
            self.save_cfg();
        }
    }

    pub(super) fn metadata_settings(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            section_label(ui, "Metadata");
            toggle_row(
                ui,
                "Eksik metadata'yı internetten tamamla",
                "Albüm, sanatçı, yıl ve parça numarası Deezer ve MusicBrainz'dan alınır",
                "Dosya adı ve süreyle eşleştirir; emin olmadığı sonucu kullanmaz. Etiketi dolu alanların yerine geçmez.",
                &mut self.cfg.meta_online,
            );
            row_sep(ui);
            toggle_row(
                ui,
                "Eksik albüm kapaklarını bul",
                "Kapağı olmayan albümlerin kapağı Deezer'dan indirilir",
                "Albüm ve sanatçı adı eşleşmezse kapak kullanılmaz. Dosyalara dokunulmaz, kapaklar yalnızca Soundworks'te görünür.",
                &mut self.cfg.meta_covers,
            );
            row_sep(ui);
            setting_row(ui, "Hangi parçalar", "Tüm şarkılarda etiketi dolu parçaların boş alanları (yıl, albüm sanatçısı gibi) tamamlanır", "", |ui| {
                segmented(ui, &mut self.cfg.meta_all, &[(false, "Yalnızca eksikler", None), (true, "Tüm şarkılar", None)]);
            });
            row_sep(ui);
            setting_row(ui, "Dosyalara yaz", "Bulunan bilgi dosya etiketine de işlenir. Yalnızca boş alanlar doldurulur, her şey geri alınabilir.", "", |ui| {
                segmented(ui, &mut self.cfg.meta_write, &[(WRITE_MODE_ASK, "Sor", None), (WRITE_MODE_ALWAYS, "Her zaman", None), (WRITE_MODE_NEVER, "Asla", None)]);
            });
            row_sep(ui);
            let (done, total) = self.lib.online_progress(self.cfg.meta_all);
            let found = self.lib.online.len();
            let status = format!("{done}/{total} parça işlendi · {found} bulundu");
            setting_row(ui, "Durum", &status, "", |ui| {
                if pill_button(ui, "Şimdi ara", Some(Icon::Undo), false).clicked() {
                    self.lib.online_tried.clear();
                    self.meta_pause = None;
                    self.lib.save();
                }
            });
            if self.cfg.meta_online && total > 0 && done < total {
                ui.add(egui::ProgressBar::new(done as f32 / total as f32).desired_height(6.0));
                ui.add_space(4.0);
            }
            if self.meta_pause.is_some() && self.cfg.meta_online {
                ui.label(RichText::new(crate::i18n::tr("İnternete ulaşılamadı, birkaç dakika sonra yeniden denenecek")).font(body(12.0)).color(AMBER));
                ui.add_space(4.0);
            }
            row_sep(ui);
            let busy = self.meta_write_rx.is_some();
            let desc = if busy { "Dosyalar işleniyor…".to_string() } else { format!("{} parçanın bulunan bilgisi var", found) };
            let can_undo = !self.meta_log.is_empty();
            let undo_n = self.meta_log.len();
            setting_row(ui, "Dosya işlemleri", &desc, "", |ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    if can_undo && pill_button(ui, &format!("Yazılanları geri al ({undo_n})"), Some(Icon::Undo), false).clicked() {
                        self.meta_write_rx = Some(onlinemeta::spawn_undo(self.meta_log.clone()));
                    }
                    if found > 0 && pill_button(ui, "Hepsini dosyalara yaz", None, false).clicked() {
                        self.write_all_online_to_files();
                    }
                });
            });
        });
        ui.add_space(12.0);
    }
}
