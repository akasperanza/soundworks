//! Motor denetimi: başlat/durdur, hız isteği, kurtarma, oynat/duraklat.

use super::*;

impl App {
    pub(super) fn start(&mut self) {
        if self.running.is_some() {
            return;
        }
        if self.stopping.is_some() {
            // önceki motor henüz cihazı bırakmadı: kapanınca başlatılır
            self.queued_start.get_or_insert(false);
            return;
        }
        self.rate_checked = false;
        let mut from = None;
        let mut to = self.cfg.to.clone();
        if self.cfg.use_loopback {
            let virt = match &self.cfg.from {
                Some(n) => engine::resolve(Some(n)).ok(),
                None => engine::find_virtual(),
            };
            let Some(virt) = virt else {
                self.message = "Sanal ses cihazı bulunamadı. Ayarlar > Sistem DSP bölümüne bak.".into();
                self.say("Sanal ses cihazı bulunamadı (Ayarlar > Sistem DSP)");
                return;
            };
            let cur_default = engine::default_render_id().ok();
            if to.is_none() {
                // hedef: yönlendirmeden önceki varsayılan cihaz
                to = match &cur_default {
                    Some(d) if *d != virt.id => Some(d.clone()),
                    _ => self.cfg.restore_default.clone(),
                };
                if to.is_none() {
                    self.message = "Hedef cihaz belirlenemedi. Ayarlar'dan gerçek bir hedef cihaz seç.".into();
                    return;
                }
            }
            if let Some(t) = &to {
                if engine::resolve(Some(t)).map(|d| d.id == virt.id).unwrap_or(false) {
                    self.message = "Hedef cihaz sanal cihazın kendisi olamaz. Ayarlar'dan gerçek cihazı seç.".into();
                    return;
                }
            }
            if self.cfg.auto_route && cur_default.as_deref() != Some(virt.id.as_str()) {
                self.cfg.restore_default = cur_default;
                self.save_cfg();
                if let Err(e) = engine::set_default_render(&virt.id) {
                    self.cfg.restore_default = None;
                    self.message = format!("Varsayılan çıkış yönlendirilemedi: {e:#}");
                    return;
                }
            }
            from = Some(virt.id.clone());
        }
        self.target_dev = engine::resolve(to.as_deref()).ok().map(|d| (d.kind, d.name, d.id));
        self.rate_notice = None;
        self.refresh_dsd_policy();
        self.prepare_rate(to.as_deref());
        // DoP yalnızca oynatıcı kaynağında, exclusive ve hız eşleştirme açıkken
        let dop = self.player.want_dop() && !self.cfg.use_loopback && !self.cfg.exclusive_off && self.cfg.match_rate;
        self.player.set_engine_dop(dop);
        let ctl = Control::new(self.preset.clone());
        ctl.set_chain(self.cfg.chain.clone());
        self.chain_sent = self.cfg.chain.clone();
        let opts = Options {
            from,
            to,
            shared: self.cfg.exclusive_off,
            player: if self.cfg.use_loopback { None } else { Some(self.player.clone()) },
            dop,
        };
        let c2 = ctl.clone();
        let handle = std::thread::spawn(move || {
            if let Err(e) = engine::run(c2.clone(), opts) {
                if engine::is_gone(&e) {
                    c2.gone.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                *c2.status.lock().unwrap() = format!("Hata: {}", engine::explain_error(&e));
            }
        });
        dev_trace(&format!("engine_start rate={} dop={dop}", self.player.rate()));
        self.running = Some(Running { ctl, handle, dop });
        self.proof_id = u64::MAX; // yeni motorun sayaçları sıfırdan başlar: tabanı yeniden al
        self.message.clear();
    }

    /// Sistem DSP yönlendirmesini geri al: varsayılan çıkışı eski cihaza döndür.
    pub(super) fn restore_route(&mut self) {
        if let Some(id) = self.cfg.restore_default.take() {
            if engine::set_default_render(&id).is_err() {
                // eski cihaz yoksa (ör. kulaklık çıkarıldı) ilk gerçek cihaza dön
                if let Some(d) = engine::list_devices().ok().and_then(|v| {
                    let virt = engine::find_virtual().map(|d| d.id);
                    v.into_iter().find(|d| Some(&d.id) != virt.as_ref())
                }) {
                    let _ = engine::set_default_render(&d.id);
                }
            }
            self.save_cfg();
        }
    }

    /// Çıkış hızını hazırla: eşleştirme açıksa cihazın desteklediği hızlar sorgulanır ve başlangıç hızı yüklü parçaya göre seçilir.
    pub(super) fn prepare_rate(&mut self, to: Option<&str>) {
        // Hız parça değişiminden gelmiyorsa (ör. hedef cihaz değişti) açık parça eski hıza çevrilmiş çözülüyor; motor yeni hızda
        // çalarsa şarkı hızlanır/yavaşlar. Hız değiştiyse parçayı kaldığı yerden yeni hızda yeniden aç.
        let from_track_change = self.start_rate.is_some();
        let before = self.player.rate();
        self.prepare_rate_inner(to);
        if !from_track_change && self.player.rate() != before {
            self.player.reload_current();
        }
    }

    fn prepare_rate_inner(&mut self, to: Option<&str>) {
        if self.cfg.use_loopback {
            return;
        }
        let fixed = self.cfg.player_rate;
        if !self.cfg.match_rate || self.cfg.exclusive_off {
            self.player.set_follow(false, vec![], fixed);
            self.player.set_rate(self.start_rate.take().unwrap_or(fixed));
            return;
        }
        let mut sup: Vec<u32> = vec![];
        if let Ok(d) = engine::resolve(to) {
            // her başlatmada yeniden sor (yoklama ~20 ms): cihaz modu değişmiş olabilir (ör. USB-C takıldı). Cihaz o an meşgulse
            // ya da cevap vermezse birkaç kez dene; yine boşsa son bilinen listeye dön.
            for attempt in 0..3 {
                sup = engine::supported_rates(&d.id);
                if !sup.is_empty() {
                    break;
                }
                if attempt < 2 {
                    std::thread::sleep(std::time::Duration::from_millis(150));
                }
            }
            if sup.is_empty() {
                sup = self.rate_cache.get(&d.id).cloned().unwrap_or_default();
            } else {
                self.rate_cache.insert(d.id.clone(), sup.clone());
            }
        }
        self.probe_rates = sup.clone();
        if sup.is_empty() {
            self.say(format!("Cihazın kabul ettiği hızlar alınamadı; sabit {} kHz kullanılıyor", fixed as f32 / 1000.0));
        }
        self.player.set_follow(true, sup.clone(), fixed);
        let r = match self.start_rate.take() {
            Some(r) => r,
            None if self.meta.src_rate > 0 => engine::choose_rate(self.meta.src_rate, &sup, fixed),
            None => fixed,
        };
        self.player.set_rate(r);
    }

    /// Kütüphanedeki parçaların ses seviyesini arka planda ölç (önbellekte olanlar atlanır).
    pub(super) fn start_loudness(&mut self) {
        self.loud_started = true;
        let albums: Vec<Vec<String>> = self.lib.albums.iter().map(|a| a.tracks.iter().map(|t| t.path.clone()).collect()).collect();
        crate::loudness::spawn(self.loud.clone(), albums);
    }

    /// Oynatıcı yeni bir çıkış hızı istedi (parçanın hızı değişti): motoru yeni hızla yeniden başlat.
    pub(super) fn on_rate_request(&mut self, r: u32) {
        dev_trace(&format!("rate_request {r}"));
        if self.running.is_some() && !self.cfg.use_loopback {
            // arayüzü bekletmeden: motor arka planda kapanır, yeni hızla kendiliğinden yeniden başlar
            self.stop_async();
            self.start_rate = Some(r);
            self.queued_start = Some(true);
        } else {
            self.player.set_rate(r);
        }
    }

    /// Cihaz koptuysa: geri geldiğinde motoru yeniden başlat ve çalıyorduysa devam et.
    pub(super) fn tick_recover(&mut self) {
        let Some((was_playing, last)) = self.recover else { return };
        if self.running.is_some() {
            self.recover = None;
            return;
        }
        if last.elapsed() < std::time::Duration::from_millis(1500) {
            return;
        }
        let present = engine::resolve(self.cfg.to.as_deref()).is_ok();
        if crate::recover::should_retry(last.elapsed(), present) {
            self.recover = Some((was_playing, Instant::now()));
            self.start();
            if self.running.is_some() {
                if was_playing {
                    self.player.playing.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                self.recover = None;
                self.say("Cihaz geri geldi, devam ediliyor");
            }
        } else {
            self.recover = Some((was_playing, Instant::now()));
        }
    }

    /// Klavyedeki medya tuşları (yalnızca motor çalışırken dinlenir).
    pub(super) fn tick_media(&mut self) {
        crate::media::set_enabled(self.running.is_some() && !self.cfg.use_loopback);
        for c in crate::media::drain() {
            match c {
                crate::media::MediaCmd::PlayPause => self.play_pressed(),
                crate::media::MediaCmd::Next => self.player.next(),
                crate::media::MediaCmd::Prev => self.player.prev(),
                crate::media::MediaCmd::Stop => self.player.playing.store(false, std::sync::atomic::Ordering::SeqCst),
            }
        }
    }

    /// Motoru kapatır ve bitmesini bekler (uygulamadan çıkarken kullanılır).
    pub(super) fn stop(&mut self) {
        self.recover = None;
        self.queued_start = None;
        if let Some(r) = self.running.take() {
            r.ctl.request_stop();
            let _ = r.handle.join();
        }
        if let Some(h) = self.stopping.take() {
            let _ = h.join();
        }
        self.restore_route();
        self.player.playing.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Motoru arayüzü bekletmeden kapatır: kapanma arka planda bitirilir.
    pub(super) fn stop_async(&mut self) {
        self.recover = None;
        self.player.playing.store(false, std::sync::atomic::Ordering::SeqCst);
        match self.running.take() {
            Some(r) => {
                r.ctl.request_stop();
                let h = r.handle;
                self.stopping = Some(std::thread::spawn(move || {
                    let _ = h.join();
                }));
            }
            None => self.restore_route(),
        }
    }

    /// Oynatıcı modunda motor boşta (duraklatıldı / liste bitti) kalırsa bir süre sonra kendiliğinden kapanır ve
    /// (exclusive) cihazı diğer uygulamalara bırakır. Play'e basınca yeniden kendiliğinden başlar.
    pub(super) fn tick_idle_release(&mut self) {
        const IDLE_RELEASE: std::time::Duration = std::time::Duration::from_secs(30);
        let idle = self.running.is_some()
            && !self.cfg.use_loopback
            && !self.player.playing.load(std::sync::atomic::Ordering::Relaxed)
            && self.stopping.is_none()
            && self.queued_start.is_none();
        if !idle {
            self.idle_since = None;
            return;
        }
        let since = *self.idle_since.get_or_insert_with(Instant::now);
        if since.elapsed() >= IDLE_RELEASE {
            self.idle_since = None;
            dev_trace("idle_release");
            self.stop_async();
        }
    }

    /// Arka planda kapanan motor bitti mi? Bittiyse yönlendirmeyi geri alır, sıradaki başlatmayı yapar.
    pub(super) fn tick_stopping(&mut self) {
        if !self.stopping.as_ref().is_some_and(|h| h.is_finished()) {
            return;
        }
        dev_trace("engine_stopped (arka planda kapandı)");
        if let Some(h) = self.stopping.take() {
            let _ = h.join();
        }
        self.restore_route();
        if let Some(resume) = self.queued_start.take() {
            self.start();
            if resume && self.running.is_some() {
                self.player.playing.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
    }

    pub(super) fn sync_playlist(&mut self) {
        if crate::devenv::dev_var("SW_OFFSCREEN").is_ok() || crate::devenv::dev_var("SW_SHOT").is_ok() {
            return;
        }
        let q: Vec<String> =
            self.player.playlist.lock().unwrap().iter().map(|p| p.to_string_lossy().to_string()).collect();
        if let Ok(j) = serde_json::to_vec(&q) {
            let _ = crate::fsutil::write_atomic(&queue_path(), &j);
        }
    }

    pub(super) fn loopback_notice(&mut self) {
        self.say("Kaynak: sistem sesi. Oynatıcı için Ayarlar'dan Dahili oynatıcıyı seç.");
    }

    pub(super) fn play_pressed(&mut self) {
        if self.cfg.use_loopback {
            self.loopback_notice();
            return;
        }
        self.start();
        self.player.toggle();
    }

    pub(super) fn play_index(&mut self, i: usize) {
        if self.cfg.use_loopback {
            self.loopback_notice();
            return;
        }
        let list: Vec<PathBuf> = self.player.playlist.lock().unwrap().clone();
        if self.dsd_gate(&list, i) {
            return;
        }
        self.start();
        self.player.play_index(i);
    }
}
