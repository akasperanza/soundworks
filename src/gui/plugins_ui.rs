use super::*;
use crate::plugins;

impl App {
    pub(super) fn plugins_refresh(&mut self) {
        self.plugin_list = plugins::discover(&plugins::plugins_dir());
        let enabled = self.cfg.plugins.clone();
        let known: Vec<String> = self.plugin_list.iter().map(|d| d.id.clone()).collect();
        for d in self.plugin_list.clone() {
            let want = enabled.contains(&d.id) && d.error.is_none();
            if want && !self.plugin_host.is_running(&d.id) {
                let _ = self.plugin_host.start(&d);
                self.plug_id = u64::MAX;
            } else if !want && self.plugin_host.is_running(&d.id) {
                self.plugin_host.stop(&d.id);
            }
        }
        for id in enabled.iter().filter(|id| !known.contains(id)) {
            self.plugin_host.stop(id);
        }
    }

    fn plugin_state_json(&self, pos: f64) -> String {
        serde_json::json!({
            "state": {
                "title": self.meta.title,
                "artist": self.meta.artist,
                "album": self.meta.album,
                "year": self.meta.year,
                "duration": self.player.duration(),
                "position": pos,
                "playing": self.player.playing.load(Relaxed),
            }
        })
        .to_string()
    }

    pub(super) fn plugins_tick(&mut self) {
        if !self.plugin_list.iter().any(|d| self.plugin_host.is_running(&d.id)) {
            return;
        }
        for t in self.plugin_host.drain_toasts() {
            self.say(t);
        }
        let id = self.cover_id;
        let playing = self.player.playing.load(Relaxed);
        let pos = self.player.position();
        let expected = self.plug_pos + if self.plug_playing { self.plug_at.elapsed().as_secs_f64() } else { 0.0 };
        let event = if id != self.plug_id {
            Some("trackchange")
        } else if playing != self.plug_playing {
            Some(if playing { "play" } else { "pause" })
        } else if (pos - expected).abs() > 1.5 {
            Some("seek")
        } else {
            None
        };
        if let Some(ev) = event {
            let json = self.plugin_state_json(pos);
            self.plugin_host.broadcast(ev, &json);
            self.plug_id = id;
            self.plug_playing = playing;
            self.plug_pos = pos;
            self.plug_at = Instant::now();
        }
    }

    pub(super) fn plugins_settings(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            section_label(ui, "Eklentiler");
            ui.add_space(2.0);
            if self.plugin_list.is_empty() {
                ui.label(RichText::new("Eklenti yok. Bir eklenti klasörünü eklentiler klasörüne kopyala, sonra Yenile'ye bas.").font(body(12.5)).color(DIM));
            }
            let mut toggled: Option<(String, bool)> = None;
            for d in self.plugin_list.clone() {
                ui.add_space(6.0);
                let (name, version, author, desc) = match &d.manifest {
                    Some(m) => (
                        if m.name.is_empty() { d.id.clone() } else { m.name.clone() },
                        m.version.clone(),
                        m.author.clone(),
                        m.description.clone(),
                    ),
                    None => (d.id.clone(), String::new(), String::new(), String::new()),
                };
                let mut on = self.cfg.plugins.contains(&d.id) && d.error.is_none();
                ui.add_enabled_ui(d.error.is_none(), |ui| {
                    let title = if version.is_empty() { name.clone() } else { format!("{name}  ·  {version}") };
                    if switch_row(ui, &title, &desc, &mut on) {
                        toggled = Some((d.id.clone(), on));
                    }
                });
                if !author.is_empty() {
                    ui.label(RichText::new(author).font(body(12.0)).color(alpha(TEXT, 130)));
                }
                if let Some(m) = &d.manifest {
                    for perm in &m.permissions {
                        if let Some(p) = perm.strip_prefix("pipe:") {
                            ui.label(RichText::new(format!("Yerel kanal erişimi: {p}")).font(body(12.0)).color(AMBER));
                        }
                    }
                }
                if let Some(e) = &d.error {
                    ui.label(RichText::new(format!("Yüklenemedi: {e}")).font(body(12.0)).color(AMBER));
                } else if let Some(e) = self.plugin_host.last_error(&d.id) {
                    ui.label(RichText::new(format!("Hata: {e}")).font(body(12.0)).color(AMBER));
                }
            }
            if let Some((id, on)) = toggled {
                if on {
                    if !self.cfg.plugins.contains(&id) {
                        self.cfg.plugins.push(id);
                    }
                } else {
                    self.cfg.plugins.retain(|p| p != &id);
                }
                self.plugins_refresh();
                self.save_cfg();
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if pill_button(ui, "Eklentiler klasörünü aç", Some(Icon::Folder), false).clicked() {
                    let dir = plugins::plugins_dir();
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = std::process::Command::new("explorer").arg(dir).spawn();
                }
                if pill_button(ui, "Yenile", Some(Icon::Undo), false).clicked() {
                    self.plugins_refresh();
                }
            });
        });
    }
}
