use super::*;
use std::time::Duration;

impl App {

    pub(super) fn tick_queue_and_resume(&mut self) {
        if let Some((paths, next)) = self.pending_queue.take() {
            let n = paths.len();
            if next {
                self.player.insert_next(paths);
            } else {
                self.player.add(paths);
            }
            self.sync_playlist();
            self.say(format!("{n} parça {}", if next { "sıradaki olarak eklendi" } else { "sıraya eklendi" }));
        }
        if self.resume_saved.elapsed() > Duration::from_secs(4) {
            self.resume_saved = Instant::now();
            self.update_resume();
        }
    }

    pub(super) fn update_resume(&mut self) {
        let new = match self.player.current() {
            Some(i) if self.player.duration() > 0.0 => Some((i, self.player.position().floor())),
            Some(_) => return,
            None if self.player.playlist.lock().unwrap().is_empty() => None,
            None => return,
        };
        if self.cfg.resume != new {
            self.cfg.resume = new;
            self.save_cfg();
        }
    }

    pub(super) fn queue_labels(&mut self) {
        let sig: usize = self.lib.albums.iter().map(|a| a.tracks.len()).sum::<usize>() + self.lib.albums.len() * 100_000;
        if sig != self.queue_sig || self.queue_map.is_empty() {
            self.queue_sig = sig;
            self.queue_map.clear();
            for a in &self.lib.albums {
                for t in &a.tracks {
                    self.queue_map.insert(t.path.clone(), (t.title.clone(), a.artist.clone()));
                }
            }
        }
    }

    pub(super) fn queue_sidebar(&mut self, ctx: &egui::Context) {
        let t = ctx.animate_bool_with_time(Id::new("queue_side"), self.queue_open, 0.24);
        if t <= 0.005 {
            return;
        }
        let e = ease(t);
        let full_w = 400.0f32;
        self.queue_labels();
        let list: Vec<PathBuf> = self.player.playlist.lock().unwrap().clone();
        let cur = self.player.current();
        let playing = self.player.playing.load(Relaxed);
        let (mut play, mut up, mut down, mut remove, mut clear, mut close): (Option<usize>, Option<usize>, Option<usize>, Option<usize>, bool, bool) = (None, None, None, None, false, false);

        egui::SidePanel::right("queue_side")
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
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Kuyruk").font(bold(18.0)).color(Color32::WHITE));
                        ui.label(RichText::new(format!("{} parça", list.len())).font(body(12.5)).color(DIM));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icon_button(ui, Icon::Close, 30.0, false).on_hover_text_tr("Kapat").clicked() {
                                close = true;
                            }
                            if !list.is_empty() && pill_button(ui, "Temizle", Some(Icon::Trash), false).clicked() {
                                clear = true;
                            }
                        });
                    });
                    ui.add_space(10.0);
                    if list.is_empty() {
                        ui.add_space(30.0);
                        ui.label(RichText::new("Kuyruk boş").font(bold(14.5)).color(TEXT));
                        ui.label(RichText::new("Kütüphanede bir albüme sağ tıklayıp \"Sıraya ekle\" de, ya da bir albüm çal.").font(body(12.5)).color(DIM));
                        return;
                    }
                    let row_h = 64.0;
                    egui::ScrollArea::vertical().id_salt("queue_scroll").auto_shrink([false, false]).show_rows(ui, row_h, list.len(), |ui, range| {
                        for i in range {
                            let path = &list[i];
                            let key = path.to_string_lossy().to_string();
                            let (title, artist) = self.queue_map.get(&key).cloned().unwrap_or_else(|| {
                                (path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), String::new())
                            });
                            let (rect, resp) = ui.allocate_exact_size(vec2(w - 8.0, row_h), Sense::click());
                            let hov = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1);
                            let is_cur = cur == Some(i);
                            let p = ui.painter();
                            if is_cur {
                                p.rect_filled(rect.shrink2(vec2(0.0, 2.0)), Rounding::same(9.0), alpha(BLUE, 30));
                            } else if hov > 0.0 {
                                p.rect_filled(rect.shrink2(vec2(0.0, 2.0)), Rounding::same(9.0), alpha(Color32::WHITE, (14.0 * hov) as u8));
                            }
                            let nc = pos2(rect.left() + 22.0, rect.center().y);
                            if is_cur {
                                let r = Rect::from_center_size(nc, vec2(14.0, 16.0));
                                let mut child = ui.child_ui(r, egui::Layout::left_to_right(egui::Align::Center), None);
                                playing_bars(&mut child, playing, vec2(14.0, 16.0));
                            } else if hov > 0.3 {
                                draw_icon(ui.painter(), Icon::Play, nc, 12.0, alpha(Color32::WHITE, (255.0 * hov) as u8));
                            } else {
                                ui.painter().text_tr(nc, Align2::CENTER_CENTER, (i + 1).to_string(), body(12.5), DIM);
                            }
                            let btn_w = if hov > 0.05 { 84.0 } else { 8.0 };
                            let tw = rect.width() - 46.0 - btn_w;
                            let p = ui.painter().clone();
                            let (tfont, tl) = fit_wrap(&p, &title, if is_cur { bold } else { body }, &[13.5, 12.5, 11.5], tw, 2);
                            let (afont, al) = if artist.is_empty() { (body(11.5), vec![]) } else { fit_wrap(&p, &artist, body, &[11.5, 10.5], tw, 1) };
                            let (lh_t, lh_a) = (line_height(&p, &tfont), line_height(&p, &afont));
                            let block = tl.len() as f32 * lh_t + if al.is_empty() { 0.0 } else { 2.0 + al.len() as f32 * lh_a };
                            let top = rect.center().y - block * 0.5;
                            draw_lines(&p, pos2(rect.left() + 44.0, top), Align2::LEFT_TOP, &tl, &tfont, if is_cur { Color32::WHITE } else { TEXT });
                            draw_lines(&p, pos2(rect.left() + 44.0, top + tl.len() as f32 * lh_t + 2.0), Align2::LEFT_TOP, &al, &afont, DIM);
                            let mut over_btn = false;
                            if hov > 0.05 {
                                let bx = rect.right() - 12.0;
                                for (k, (icon, act)) in [(Icon::Close, 3u8), (Icon::ChevronDown, 2), (Icon::ChevronUp, 1)].iter().enumerate() {
                                    let c = pos2(bx - 14.0 - k as f32 * 26.0, rect.center().y);
                                    let br = Rect::from_center_size(c, vec2(24.0, 24.0));
                                    let r = ui.interact(br, resp.id.with(("qb", *act)), Sense::click());
                                    over_btn |= r.hovered();
                                    let h = ui.ctx().animate_bool_with_time(r.id, r.hovered(), 0.1);
                                    draw_icon(ui.painter(), *icon, c, 11.0, lerp_color(DIM, if *act == 3 { RED } else { Color32::WHITE }, h));
                                    if r.clicked() {
                                        match act {
                                            1 if i > 0 => up = Some(i),
                                            2 if i + 1 < list.len() => down = Some(i),
                                            3 => remove = Some(i),
                                            _ => {}
                                        }
                                    }
                                }
                            }
                            if resp.clicked() && !over_btn {
                                play = Some(i);
                            }
                        }
                    });
                });
            });

        let mut changed = false;
        if let Some(i) = up {
            self.player.move_item(i, i - 1);
            changed = true;
        } else if let Some(i) = down {
            self.player.move_item(i, i + 1);
            changed = true;
        } else if let Some(i) = remove {
            self.player.remove(i);
            changed = true;
        }
        if clear {
            self.player.clear();
            changed = true;
        }
        if changed {
            self.sync_playlist();
        }
        if let Some(i) = play {
            self.play_index(i);
        }
        if close {
            self.queue_open = false;
        }
    }
}
