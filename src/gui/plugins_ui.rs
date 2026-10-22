use super::*;
use crate::plugins::{self, UiNode};

fn icon_by_name(name: &str) -> Icon {
    match name {
        "play" => Icon::Play,
        "pause" => Icon::Pause,
        "prev" => Icon::Prev,
        "next" => Icon::Next,
        "repeat" => Icon::Repeat,
        "volume" => Icon::Volume,
        "close" => Icon::Close,
        "trash" => Icon::Trash,
        "power" => Icon::Power,
        "list" => Icon::List,
        "import" => Icon::Import,
        "undo" => Icon::Undo,
        "save" => Icon::Save,
        "folder" => Icon::Folder,
        "spectrum" => Icon::Spectrum,
        "spectrogram" => Icon::Spectrogram,
        "scope" => Icon::Scope,
        "wave" => Icon::Wave,
        "search" => Icon::Search,
        "user" => Icon::User,
        "shuffle" => Icon::Shuffle,
        "track" => Icon::Track,
        "upload" => Icon::Upload,
        "lyrics" => Icon::Lyrics,
        "settings" => Icon::Settings,
        "headphones" => Icon::Headphones,
        "maximize" => Icon::Maximize,
        _ => Icon::Plus,
    }
}

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
                "volume": self.player.volume(),
            }
        })
        .to_string()
    }

    pub(super) fn plugins_tick(&mut self) {
        if !self.plugin_list.iter().any(|d| self.plugin_host.is_running(&d.id)) {
            if !self.plug_ui.is_empty() {
                self.plug_ui.clear();
                self.plugin_panel = None;
            }
            return;
        }
        for t in self.plugin_host.drain_toasts() {
            self.say(t);
        }
        let added = self.plugin_host.drain_library();
        if !added.is_empty() {
            let n = added.len();
            self.lib_add_paths(added);
            self.say(format!("{n} parça kütüphaneye eklendi"));
        }
        for (cmd, arg) in self.plugin_host.drain_control() {
            let playing = self.player.playing.load(Relaxed);
            match cmd.as_str() {
                "play" if !playing => self.play_pressed(),
                "pause" if playing => self.play_pressed(),
                "toggle" => self.play_pressed(),
                "next" => self.player.next(),
                "prev" => self.player.prev(),
                "seek" => self.player.seek(arg.max(0.0)),
                "volume" => {
                    let v = arg.clamp(0.0, 1.0) as f32;
                    self.player.set_volume(v);
                    self.cfg.volume = Some(v);
                }
                _ => {}
            }
        }
        self.plug_ui = self.plugin_host.ui_snapshots();
        if let Some((p, id)) = self.plugin_panel.clone() {
            let alive = self.plug_ui.iter().any(|(pid, s)| *pid == p && (s.panels.is_empty() || s.panels.iter().any(|x| x.id == id)));
            if !alive {
                self.plugin_panel = None;
            }
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

    pub(super) fn plugin_set_panel(&mut self, new: Option<(String, String)>) {
        if self.plugin_panel == new {
            return;
        }
        if let Some((p, id)) = &self.plugin_panel {
            self.plugin_host.send_action(p, "close", id, "");
        }
        if let Some((p, id)) = &new {
            self.plugin_host.send_action(p, "open", id, "");
            self.lyrics_open = false;
            self.queue_open = false;
        }
        self.plugin_panel = new;
    }

    pub(super) fn plugin_bar_count(&self) -> usize {
        self.plug_ui.iter().map(|(_, s)| s.buttons.len() + s.panels.len()).sum()
    }

    pub(super) fn plugin_bar_buttons(&mut self, ui: &mut egui::Ui) {
        for (pid, snap) in self.plug_ui.clone() {
            for b in &snap.buttons {
                let r = icon_button(ui, icon_by_name(&b.icon), 32.0, false);
                let r = if b.tooltip.is_empty() { r } else { r.on_hover_text(b.tooltip.clone()) };
                if r.clicked() {
                    self.plugin_host.send_action(&pid, "button", &b.id, "");
                }
            }
            for p in &snap.panels {
                let open = self.plugin_panel.as_ref() == Some(&(pid.clone(), p.id.clone()));
                let r = icon_button(ui, icon_by_name(&p.icon), 32.0, open).on_hover_text(p.title.clone());
                if r.clicked() {
                    self.plugin_set_panel(if open { None } else { Some((pid.clone(), p.id.clone())) });
                }
            }
        }
    }

    pub(super) fn plugin_sidebar(&mut self, ctx: &egui::Context) {
        let active = self.plugin_panel.clone();
        if active.is_some() {
            self.plug_shown = active.clone();
        }
        let t = ctx.animate_bool_with_time(Id::new("plug_side"), active.is_some(), 0.24);
        if t <= 0.005 {
            return;
        }
        let Some((pid, panel_id)) = self.plug_shown.clone() else { return };
        let Some(panel) = self.plug_ui.iter().find(|(p, _)| *p == pid).and_then(|(_, s)| s.panels.iter().find(|x| x.id == panel_id)).cloned() else { return };
        let e = ease(t);
        let full_w = 380.0f32;
        let mut actions: Vec<(String, String, serde_json::Value)> = vec![];
        let mut close = false;

        egui::SidePanel::right("plugin_side")
            .exact_width(full_w * e + 12.0)
            .resizable(false)
            .frame(Frame::none().inner_margin(Margin { left: 0.0, right: 12.0, top: 2.0, bottom: 2.0 }))
            .show(ctx, |ui| {
                let inner = Margin { left: 22.0, right: 22.0, top: 22.0, bottom: 12.0 };
                let avail = ui.available_size();
                panel_frame(inner, Margin::ZERO).show(ui, |ui| {
                    ui.set_min_size(vec2(avail.x - inner.left - inner.right, avail.y - inner.top - inner.bottom));
                    self.paint_glow(ui, inner);
                    ui.set_opacity(e);
                    let w = full_w - 44.0;
                    ui.set_min_width(w);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(panel.title.clone()).font(bold(18.0)).color(Color32::WHITE));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icon_button(ui, Icon::Close, 30.0, false).on_hover_text_tr("Kapat").clicked() {
                                close = true;
                            }
                        });
                    });
                    ui.add_space(10.0);
                    egui::ScrollArea::vertical().id_salt("plugin_scroll").auto_shrink([false, false]).show(ui, |ui| {
                        ui.set_width(w - 8.0);
                        for (n, node) in panel.content.iter().enumerate() {
                            match node {
                                UiNode::Text { text, style } => {
                                    let (font, color) = match style.as_str() {
                                        "title" => (bold(18.0), Color32::WHITE),
                                        "heading" => (bold(14.0), TEXT),
                                        "dim" => (body(12.5), DIM),
                                        _ => (body(14.0), TEXT),
                                    };
                                    ui.add(egui::Label::new(egui::RichText::new(text.clone()).font(font).color(color)).wrap());
                                }
                                UiNode::Button { id, label, primary } => {
                                    if pill_button(ui, label, None, *primary).clicked() {
                                        actions.push(("click".into(), id.clone(), serde_json::Value::Null));
                                    }
                                }
                                UiNode::List { id, items } => {
                                    for it in items {
                                        let h = if it.subtitle.is_empty() { 36.0 } else { 48.0 };
                                        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
                                        if resp.hovered() {
                                            ui.painter().rect_filled(rect, Rounding::same(8.0), alpha(Color32::WHITE, 14));
                                        }
                                        let p = ui.painter().with_clip_rect(rect);
                                        let ty = if it.subtitle.is_empty() { rect.center().y } else { rect.top() + 15.0 };
                                        p.text(pos2(rect.left() + 10.0, ty), Align2::LEFT_CENTER, it.title.clone(), bold(13.5), TEXT);
                                        if !it.subtitle.is_empty() {
                                            p.text(pos2(rect.left() + 10.0, rect.top() + 34.0), Align2::LEFT_CENTER, it.subtitle.clone(), body(12.0), DIM);
                                        }
                                        if resp.clicked() {
                                            actions.push(("select".into(), id.clone(), serde_json::json!(it.id)));
                                        }
                                    }
                                }
                                UiNode::Input { id, value, placeholder } => {
                                    let key = format!("{pid}/{panel_id}/{id}/{n}");
                                    let entry = self.plug_inputs.entry(key).or_insert_with(|| (value.clone(), value.clone()));
                                    if entry.1 != *value {
                                        *entry = (value.clone(), value.clone());
                                    }
                                    let te = ui.add(egui::TextEdit::singleline(&mut entry.0).hint_text(placeholder.clone()).desired_width(ui.available_width()));
                                    if te.lost_focus() && entry.0 != entry.1 {
                                        entry.1 = entry.0.clone();
                                        actions.push(("input".into(), id.clone(), serde_json::json!(entry.0)));
                                    }
                                }
                                UiNode::Switch { id, label, value } => {
                                    let key = format!("{pid}/{panel_id}/{id}/{n}/sw");
                                    let entry = self.plug_inputs.entry(key).or_insert_with(|| (value.to_string(), value.to_string()));
                                    if entry.1 != value.to_string() {
                                        *entry = (value.to_string(), value.to_string());
                                    }
                                    let mut on = entry.0 == "true";
                                    if switch_row(ui, label, "", &mut on) {
                                        entry.0 = on.to_string();
                                        actions.push(("switch".into(), id.clone(), serde_json::json!(on)));
                                    }
                                }
                                UiNode::Divider => {
                                    ui.add_space(4.0);
                                    ui.separator();
                                    ui.add_space(4.0);
                                }
                                UiNode::Space => ui.add_space(10.0),
                                UiNode::Unknown => {}
                            }
                            ui.add_space(4.0);
                        }
                    });
                });
            });
        for (kind, id, value) in actions {
            let payload = serde_json::json!({ "type": kind, "id": id, "value": value }).to_string();
            self.plugin_host.send_action(&pid, "panel", &panel_id, &payload);
        }
        if close {
            self.plugin_set_panel(None);
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
                        let line = if let Some(p) = perm.strip_prefix("pipe:") {
                            format!("Yerel kanal erişimi: {p}")
                        } else if perm == "ui" {
                            "Arayüze düğme, panel ve komut ekler".to_string()
                        } else if perm == "playback" {
                            "Çalmayı denetler (oynat, duraklat, atla)".to_string()
                        } else if perm == "download" {
                            "Müzik klasörüne dosya indirir".to_string()
                        } else if perm == "library" {
                            "İndirdiği ses dosyalarını kütüphaneye ekler".to_string()
                        } else if perm == "network:*" {
                            "Herhangi bir internet sitesine bağlanabilir".to_string()
                        } else if let Some(h) = perm.strip_prefix("network:") {
                            format!("Şu sunucuya bağlanır: {h}")
                        } else {
                            continue;
                        };
                        ui.label(RichText::new(line).font(body(12.0)).color(AMBER));
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
