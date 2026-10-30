use super::*;

impl App {
    pub(super) fn sync_window(&mut self, ctx: &egui::Context) {
        let native = ctx.native_pixels_per_point().unwrap_or(1.0);
        let zoom = if self.cfg.ui_zoom == 0 {

            if native >= 1.45 {
                0.85
            } else if native >= 1.2 {
                0.92
            } else {
                1.0
            }
        } else {
            self.cfg.ui_zoom.clamp(60, 140) as f32 / 100.0
        };
        if (ctx.zoom_factor() - zoom).abs() > 0.001 {
            ctx.set_zoom_factor(zoom);
        }
        if QUIT_REQ.load(Relaxed) && !QUIT_ACK.swap(true, Relaxed) {
            dev_trace("quit_ack (arayüz kapanış isteğini gördü)");
            self.quitting = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.player.set_replaygain(self.cfg.rg_mode, self.cfg.rg_preamp, self.cfg.rg_prevent);
        if self.cfg.rg_mode != 0 && !self.loud_started && !self.lib.albums.is_empty() && crate::devenv::dev_var("SW_OFFSCREEN").is_err() {
            self.start_loudness();
        }
        if let Some(r) = self.player.take_rate_req() {
            self.on_rate_request(r);
        }
        if !self.cfg.use_loopback && !self.rate_checked {
            if let Some(run) = &self.running {
                let o = run.ctl.out_info.lock().unwrap().clone();
                if o.rate > 0 {
                    self.rate_checked = true;
                    if o.rate != self.player.rate() {

                        self.rate_notice = Some((self.player.rate(), o.rate));
                        self.player.force_rate(o.rate);
                    }
                }
            }
        }
        if !self.titlebar_done && self.frames_run > 2 {
            self.titlebar_done = style_titlebar();
        }
        self.frames_run = self.frames_run.saturating_add(1);

        if crate::devenv::dev_var("SW_OFFSCREEN").is_err() && !WINDOW_HIDDEN.load(Relaxed) {
            let ppp = ctx.pixels_per_point();
            ctx.input(|i| {
                let v = i.viewport();
                let maxed = v.maximized.unwrap_or(false);
                if v.minimized.unwrap_or(false) {
                    return;
                }
                let k = ppp / native.max(0.1);
                if maxed {
                    if let Some(mut w) = self.cfg.win {
                        w[5] = 1.0;
                        self.cfg.win = Some(w);
                    }
                } else if let (Some(inner), Some(outer)) = (v.inner_rect, v.outer_rect) {
                    let (sz, p) = (inner.size() * k, outer.min.to_vec2() * k);
                    if sz.x >= 400.0 && sz.y >= 300.0 {
                        self.cfg.win = Some([p.x, p.y, sz.x, sz.y, native, 0.0]);
                    }
                }
            });
        }
    }

    pub(super) fn handle_keys(&mut self, ctx: &egui::Context) -> bool {
        let kb = ctx.wants_keyboard_input();
        let mut ab_held = false;
        if !kb {
            ab_held = ctx.input(|i| i.key_down(egui::Key::B));
            let (space, left, right, up, down) = ctx.input(|i| {
                (
                    i.key_pressed(egui::Key::Space),
                    i.key_pressed(egui::Key::ArrowLeft),
                    i.key_pressed(egui::Key::ArrowRight),
                    i.key_pressed(egui::Key::ArrowUp),
                    i.key_pressed(egui::Key::ArrowDown),
                )
            });

            if self.view != View::Settings && !self.search_open && !self.cfg.use_loopback {
                if space {
                    self.play_pressed();
                }
                if left {
                    self.player.seek((self.player.position() - 5.0).max(0.0));
                }
                if right {
                    self.player.seek(self.player.position() + 5.0);
                }
                if up || down {
                    let v = (self.player.volume() + if up { 0.05 } else { -0.05 }).clamp(0.0, 1.0);
                    self.player.set_volume(v);
                    self.cfg.volume = Some(v);
                }
            }
        }

        self.solo_target = None;
        let f8 = ctx.input(|i| i.key_pressed(egui::Key::F8));
        let shift_k = !kb && ctx.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::K));
        if f8 {
            if self.search_open {
                self.search_open = false;
            } else {
                self.open_search();
            }
        } else if shift_k && !self.search_open {
            self.open_search();
        }

        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Comma)) {
            self.search_open = false;
            self.upload_open = false;
            self.set_view(View::Settings);
        }
        ab_held
    }

    pub(super) fn update_backdrop(&mut self, ctx: &egui::Context) {
        let opened = match (&self.view, &self.lib_artist, self.lib_open.clone()) {
            (View::Library, None, Some(k)) if self.lib.albums.iter().any(|a| a.key == k) => Some(k),
            _ => None,
        };
        let key = match &opened {
            Some(k) => format!("album|{k}"),
            None if !self.meta.title.is_empty() && self.play_bd.is_some() => format!("play|{}", self.cover_id),
            None => "default".to_string(),
        };
        let dt = ctx.input(|i| i.unstable_dt).clamp(0.001, 0.1);
        if self.bd_layers.last().map(|l| &l.0) != Some(&key) {
            let cached = self.bd_images.get(&key).cloned();
            let image = match cached {
                Some(img) => img,
                None => {
                    let cover = match &opened {
                        Some(k) => image::open(library::cover_path(k)).ok().and_then(|img| {
                            let rgba = img.to_rgba8();
                            blurred_backdrop(rgba.width() as usize, rgba.height() as usize, rgba.as_raw())
                        }),
                        None if key != "default" => self.play_bd.clone(),
                        None => None,
                    };
                    let img = std::sync::Arc::new(cover.unwrap_or_else(default_backdrop));
                    if self.bd_images.len() > 24 {
                        self.bd_images.clear();
                    }
                    self.bd_images.insert(key.clone(), img.clone());
                    img
                }
            };
            let tex = ctx.load_texture(format!("backdrop{key}"), (*image).clone(), TextureOptions::LINEAR);
            self.bd_layers.push((key, tex, 0.0));
        }
        let mut animating = false;
        if let Some(top) = self.bd_layers.last_mut() {
            if top.2 < 1.0 {
                top.2 = (top.2 + dt / 0.9).min(1.0);
                animating = true;
            }
            if top.2 >= 1.0 {
                let keep = self.bd_layers.len() - 1;
                self.bd_layers.drain(..keep);
            }
        }
        if animating {
            ctx.request_repaint();
        }
        let screen = ctx.screen_rect();
        let painter = ctx.layer_painter(egui::LayerId::background());
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        for (i, (_, tex, a)) in self.bd_layers.iter().enumerate() {
            let a = if i == 0 { 1.0 } else { ease(*a) };
            painter.image(tex.id(), screen, uv, alpha(Color32::WHITE, (255.0 * a) as u8));
        }
    }

    pub(super) fn show_main_views(&mut self, ctx: &egui::Context, fade: f32, ab_held: &mut bool) {
        if !self.ambient_on && !self.update_overlay_active() {
            self.header(ctx);
            self.bottom_bar(ctx);
            self.lyrics_sidebar(ctx);
            self.queue_sidebar(ctx);
            self.plugin_sidebar(ctx);
            match self.view {
                View::Motor => {
                    self.side_panel(ctx, fade);
                    self.motor_view(ctx, fade, ab_held);
                }
                View::Library => self.library_view(ctx, fade),
                View::Settings => self.settings_view(ctx, fade),
                View::Extensions => self.extensions_view(ctx, fade),
            }
        }
    }

    pub(super) fn push_engine_controls(&mut self, ab_held: bool) {
        if let Some(r) = &self.running {
            let c = &r.ctl;
            c.bypass.store(ab_held, Relaxed);
            c.level_match.store(!self.cfg.level_match_off, Relaxed);
            c.balance.store(self.cfg.balance.to_bits(), Relaxed);
            c.mono.store(self.cfg.mono, Relaxed);
            c.dither.store(self.cfg.dither, Relaxed);
            match self.solo_target {
                Some((f, q)) => {
                    c.solo_freq.store((f as f32).to_bits(), Relaxed);
                    c.solo_q.store((q as f32).to_bits(), Relaxed);
                    c.solo.store(true, Relaxed);
                }
                None => c.solo.store(false, Relaxed),
            }
            if self.cfg.chain != self.chain_sent {
                c.set_chain(self.cfg.chain.clone());
                self.chain_sent = self.cfg.chain.clone();
            }
        }
    }

    pub(super) fn show_overlays(&mut self, ctx: &egui::Context) {
        if !self.ambient_on {
            self.search_overlay(ctx);
            self.autoeq_overlay(ctx);
            self.props_overlay(ctx);
            self.tip_banner(ctx);
            self.meta_banner(ctx);
            self.dsd_dialog_ui(ctx);
            self.track_dialogs(ctx);
        }
    }
}
