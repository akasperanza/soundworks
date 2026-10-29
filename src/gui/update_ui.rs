use super::*;
use crate::update::{self, NoteLine};
use std::sync::mpsc::channel;

pub(super) enum UpdState {
    Idle,
    Checking,
    UpToDate,
    Available(update::Release),
    Downloading(update::Release),
    Failed(String),
}

pub(super) enum UpdMsg {
    Checked(bool, Result<Option<update::Release>, String>),
    Installed(Result<(), String>),
}

pub(super) struct WhatsNew {
    pub version: String,
    pub notes: Option<String>,
    pub failed: bool,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Mode {
    Available,
    Updated,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

const REMIND_CHOICES: [(&str, u64); 4] = [("1 saat sonra", 3600), ("Yarın", 24 * 3600), ("3 gün sonra", 3 * 24 * 3600), ("1 hafta sonra", 7 * 24 * 3600)];

fn paint_backdrop(p: &egui::Painter, rect: Rect, time: f32) {
    p.rect_filled(rect, 0.0, BG0);
    let mut mesh = egui::Mesh::default();
    let top = lerp_color(BG0, BLUE, 0.16);
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), BG0);
    mesh.colored_vertex(rect.left_bottom(), BG0);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(Shape::mesh(mesh));
    let m = rect.width().min(rect.height());
    let orbs = [
        (0.18 + 0.06 * (time * 0.23).sin(), 0.22 + 0.07 * (time * 0.19).cos(), 0.55, BLUE),
        (0.82 + 0.05 * (time * 0.17).cos(), 0.30 + 0.08 * (time * 0.21).sin(), 0.48, SKY),
        (0.55 + 0.08 * (time * 0.15).sin(), 0.92 + 0.04 * (time * 0.27).cos(), 0.6, lerp_color(BLUE, Color32::from_rgb(140, 90, 255), 0.6)),
    ];
    for (fx, fy, fr, color) in orbs {
        let c = pos2(rect.left() + rect.width() * fx, rect.top() + rect.height() * fy);
        radial_glow(p, c, m * fr, color, 44);
    }
}

fn radial_glow(p: &egui::Painter, c: egui::Pos2, radius: f32, color: Color32, a: u8) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(c, alpha(color, a));
    let n = 64;
    for i in 0..n {
        let ang = std::f32::consts::TAU * i as f32 / n as f32;
        mesh.colored_vertex(c + vec2(ang.cos(), ang.sin()) * radius, alpha(color, 0));
    }
    for i in 0..n as u32 {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n as u32);
    }
    p.add(Shape::mesh(mesh));
}

fn text_w(ui: &egui::Ui, text: &str, font: egui::FontId) -> f32 {
    ui.painter().layout_no_wrap(crate::i18n::tr(text), font, Color32::WHITE).size().x
}

fn pill_w(ui: &egui::Ui, text: &str, icon: bool) -> f32 {
    text_w(ui, text, body(13.5)) + if icon { 20.0 } else { 0.0 } + 28.0
}

fn link_w(ui: &egui::Ui, text: &str) -> f32 {
    text_w(ui, text, body(13.0)) + 20.0
}

fn center_by(ui: &mut egui::Ui, col_w: f32, total: f32) {
    ui.add_space(((col_w - total) * 0.5).max(0.0));
}

fn link_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let g = ui.painter().layout_no_wrap(crate::i18n::tr(text), body(13.0), DIM);
    let (rect, resp) = ui.allocate_exact_size(g.size() + vec2(20.0, 14.0), Sense::click());
    let h = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.12);
    let col = lerp_color(alpha(Color32::WHITE, 150), Color32::WHITE, h);
    let pos = rect.center() - g.size() * 0.5;
    ui.painter().galley(pos, g, col);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn big_button(ui: &mut egui::Ui, text: &str, icon: Icon) -> egui::Response {
    let g = ui.painter().layout_no_wrap(crate::i18n::tr(text), bold(15.0), Color32::WHITE);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 78.0, 50.0), Sense::click());
    let h = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.14);
    let lift = -2.0 * h;
    let r = rect.translate(vec2(0.0, lift));
    for k in 0..6 {
        let g = k as f32 * 4.0 * (0.6 + h * 0.4);
        ui.painter().rect_filled(r.translate(vec2(0.0, 6.0)).expand(g), Rounding::same(25.0 + g), alpha(BLUE, 10));
    }
    let top = lerp_color(BLUE, SKY, 0.35 + 0.3 * h);
    let mut mesh = egui::Mesh::default();
    let steps = 12;
    let rad = 25.0f32;
    let mut pts = vec![];
    for i in 0..=steps {
        let a = std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / steps as f32;
        pts.push(pos2(r.left() + rad + rad * a.cos(), r.center().y + rad * a.sin()));
    }
    for i in 0..=steps {
        let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / steps as f32;
        pts.push(pos2(r.right() - rad + rad * a.cos(), r.center().y + rad * a.sin()));
    }
    for q in &pts {
        let f = ((q.y - r.top()) / r.height()).clamp(0.0, 1.0);
        mesh.colored_vertex(*q, lerp_color(top, BLUE, f));
    }
    for i in 1..pts.len() as u32 - 1 {
        mesh.add_triangle(0, i, i + 1);
    }
    ui.painter().add(Shape::mesh(mesh));
    draw_icon(ui.painter(), icon, pos2(r.left() + 30.0, r.center().y), 17.0, Color32::WHITE);
    ui.painter().galley(pos2(r.left() + 52.0, r.center().y - g.size().y * 0.5), g, Color32::WHITE);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn version_pill(ui: &mut egui::Ui, text: &str, filled: bool) {
    let g = ui.painter().layout_no_wrap(text.to_string(), bold(15.0), if filled { Color32::WHITE } else { TEXT });
    let (rect, _) = ui.allocate_exact_size(g.size() + vec2(28.0, 14.0), Sense::hover());
    if filled {
        ui.painter().rect_filled(rect, Rounding::same(18.0), alpha(BLUE, 200));
    } else {
        ui.painter().rect_filled(rect, Rounding::same(18.0), alpha(Color32::WHITE, 14));
        ui.painter().rect_stroke(rect, Rounding::same(18.0), Stroke::new(1.0, alpha(Color32::WHITE, 36)));
    }
    ui.painter().galley(rect.center() - g.size() * 0.5, g, Color32::WHITE);
}

fn notes_card(ui: &mut egui::Ui, notes: &str, max_h: f32, a: f32) {
    let lines = update::notes_to_lines(&update::pick_language(notes, crate::i18n::get().code()));
    Frame::none().fill(alpha(BG2, (210.0 * a) as u8)).rounding(Rounding::same(16.0)).inner_margin(Margin::symmetric(24.0, 18.0)).stroke(Stroke::new(1.0, alpha(Color32::WHITE, (16.0 * a) as u8))).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.label(egui::RichText::new(crate::i18n::tr("YENİLİKLER")).font(bold(11.0)).color(alpha(SKY, (220.0 * a) as u8)));
        ui.add_space(10.0);
        if lines.is_empty() {
            ui.label(egui::RichText::new(crate::i18n::tr("Bu sürüm için not yok")).font(body(14.0)).color(alpha(DIM, (255.0 * a) as u8)));
            return;
        }
        egui::ScrollArea::vertical().max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
            ui.set_width(ui.available_width() - 8.0);
            for (i, l) in lines.iter().enumerate() {
                let t = ((ui.ctx().input(|x| x.time) as f32 * 0.0) + a).clamp(0.0, 1.0);
                let _ = i;
                match l {
                    NoteLine::Heading(h) => {
                        ui.add_space(if i == 0 { 0.0 } else { 10.0 });
                        ui.label(egui::RichText::new(h).font(bold(15.0)).color(alpha(Color32::WHITE, (255.0 * t) as u8)));
                        ui.add_space(4.0);
                    }
                    NoteLine::Bullet(b) => {
                        ui.horizontal_top(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(14.0, 20.0), Sense::hover());
                            ui.painter().circle_filled(pos2(r.left() + 4.0, r.top() + 11.0), 2.6, alpha(BLUE, (255.0 * t) as u8));
                            ui.add(egui::Label::new(egui::RichText::new(b).font(body(14.0)).color(alpha(TEXT, (255.0 * t) as u8))).wrap());
                        });
                        ui.add_space(3.0);
                    }
                    NoteLine::Text(x) => {
                        ui.add(egui::Label::new(egui::RichText::new(x).font(body(13.5)).color(alpha(DIM, (255.0 * t) as u8))).wrap());
                        ui.add_space(3.0);
                    }
                }
            }
        });
        });
    });
}

impl App {
    pub(super) fn update_available(&self) -> Option<&update::Release> {
        match &self.upd {
            UpdState::Available(r) | UpdState::Downloading(r) => Some(r),
            _ => None,
        }
    }

    pub(super) fn update_overlay_active(&self) -> bool {
        self.whats_new.is_some() || (self.upd_dialog && !matches!(self.upd, UpdState::Idle | UpdState::Checking | UpdState::UpToDate))
    }

    pub(super) fn whats_new_init(&mut self) {
        let current = env!("CARGO_PKG_VERSION");
        if self.cfg.last_run_version == current {
            return;
        }
        let previous = std::mem::replace(&mut self.cfg.last_run_version, current.to_string());
        let upgraded = !previous.is_empty() && update::is_newer(current, &previous);
        if upgraded {
            self.show_whats_new(current.to_string());
        }
        self.save_cfg();
    }

    pub(super) fn show_whats_new(&mut self, version: String) {
        let saved = (self.cfg.update_notes_version == version && !self.cfg.update_notes.trim().is_empty()).then(|| self.cfg.update_notes.clone());
        let need_fetch = saved.is_none();
        self.whats_new = Some(WhatsNew { version: version.clone(), notes: saved, failed: false });
        self.upd_t0 = Instant::now();
        if need_fetch {
            let (tx, rx) = channel();
            self.notes_rx = Some(rx);
            let url = crate::devenv::dev_var("SW_NOTES_API").unwrap_or_else(|_| update::notes_url(&version));
            std::thread::spawn(move || {
                let _ = tx.send(update::fetch_notes(&url));
            });
        }
    }

    pub(super) fn update_check_async(&mut self, manual: bool) {
        if matches!(self.upd, UpdState::Checking | UpdState::Downloading(_)) {
            return;
        }
        self.upd = UpdState::Checking;
        let (tx, rx) = channel();
        self.upd_rx = Some(rx);
        let url = crate::devenv::dev_var("SW_UPDATE_API").unwrap_or_else(|_| update::api_url());
        std::thread::spawn(move || {
            let r = update::check(&url, env!("CARGO_PKG_VERSION"));
            let _ = tx.send(UpdMsg::Checked(manual, r));
        });
    }

    pub(super) fn update_install_async(&mut self, rel: update::Release) {
        self.upd_progress.store(0, Relaxed);
        self.cfg.update_notes = rel.notes.clone();
        self.cfg.update_notes_version = rel.version.clone();
        self.save_cfg();
        self.upd = UpdState::Downloading(rel.clone());
        let (tx, rx) = channel();
        self.upd_rx = Some(rx);
        let progress = self.upd_progress.clone();
        let dev_open = crate::devenv::dev_var("SW_UPDATE_API").is_ok();
        std::thread::spawn(move || {
            let dir = config_path().parent().map(|p| p.join("update")).unwrap_or_else(|| PathBuf::from("update"));
            let result = (|| -> Result<(), String> {
                let exe = std::env::current_exe().map_err(|e| e.to_string())?;
                let allow = |u: &str| dev_open || update::trusted_host(u);
                let file = update::download(&rel, &dir, &progress, &allow)?;
                update::install(&file, &exe)
            })();
            let _ = tx.send(UpdMsg::Installed(result));
        });
    }

    fn restart_into_new_version(&mut self, ctx: &egui::Context) {
        let Ok(exe) = std::env::current_exe() else { return };
        self.save_cfg();
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("--wait-pid").arg(std::process::id().to_string());
        if WINDOW_HIDDEN.load(Relaxed) {
            cmd.arg("--tray");
        }
        if cmd.spawn().is_ok() {
            shell::request_quit(ctx);
        } else {
            self.upd = UpdState::Failed("could not restart".into());
        }
    }

    pub(super) fn update_tick(&mut self, ctx: &egui::Context) {
        let msg = self.upd_rx.as_ref().and_then(|rx| rx.try_recv().ok());
        match msg {
            Some(UpdMsg::Checked(manual, r)) => {
                self.cfg.update_checked = now_secs();
                self.save_cfg();
                self.upd = match r {
                    Ok(Some(rel)) => {
                        let fresh = rel.version != self.cfg.update_skip && now_secs() >= self.cfg.update_remind_at;
                        if manual || (fresh && !self.ambient_on && self.whats_new.is_none()) {
                            self.upd_dialog = true;
                            self.upd_t0 = Instant::now();
                        }
                        UpdState::Available(rel)
                    }
                    Ok(None) => UpdState::UpToDate,
                    Err(e) => UpdState::Failed(e),
                };
            }
            Some(UpdMsg::Installed(r)) => match r {
                Ok(()) => self.restart_into_new_version(ctx),
                Err(e) => self.upd = UpdState::Failed(e),
            },
            None => {}
        }
        if let Some(r) = self.notes_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            if let Some(w) = self.whats_new.as_mut() {
                match r {
                    Ok(text) => w.notes = Some(text),
                    Err(_) => {
                        w.notes = Some(String::new());
                        w.failed = true;
                    }
                }
            }
            self.notes_rx = None;
        }
        if matches!(self.upd, UpdState::Checking | UpdState::Downloading(_)) {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        if self.update_overlay_active() {
            self.upd_esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
            ctx.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Key { .. } | egui::Event::Text(_) | egui::Event::Paste(_) | egui::Event::Copy | egui::Event::Cut)));
        } else {
            self.upd_esc = false;
        }
        let due = now_secs().saturating_sub(self.cfg.update_checked) > 12 * 3600;
        if self.cfg.update_check && !self.upd_auto_done && matches!(self.upd, UpdState::Idle) && due && self.upd_started.elapsed().as_secs() > 25 && crate::devenv::dev_var("SW_OFFSCREEN").is_err() && crate::devenv::dev_var("SW_SHOT").is_err() {
            self.upd_auto_done = true;
            self.update_check_async(false);
        }
    }

    pub(super) fn update_overlay(&mut self, ctx: &egui::Context) {
        let mode = if self.whats_new.is_some() {
            Some(Mode::Updated)
        } else if self.update_overlay_active() {
            Some(Mode::Available)
        } else {
            None
        };
        let t = ctx.animate_bool_with_time(Id::new("upd_overlay"), mode.is_some(), 0.45);
        if mode.is_none() && t <= 0.003 {
            return;
        }
        let Some(mode) = mode.or(self.upd_last_mode) else { return };
        let screen = ctx.screen_rect();
        let time = ctx.input(|i| i.time) as f32;
        let age = self.upd_t0.elapsed().as_secs_f32();
        let item = |delay: f32| ease(((age - delay) / 0.5).clamp(0.0, 1.0));
        ctx.request_repaint_after(std::time::Duration::from_millis(16));

        let (rel, downloading) = match &self.upd {
            UpdState::Available(r) => (Some(r.clone()), false),
            UpdState::Downloading(r) => (Some(r.clone()), true),
            _ => (None, false),
        };
        let failed = if let UpdState::Failed(e) = &self.upd { Some(e.clone()) } else { None };
        let mut act = None::<&'static str>;
        let mut remind_secs = 0u64;

        egui::Area::new(Id::new("upd_full")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            ui.set_opacity(ease(t));
            let (rect, _) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
            paint_backdrop(ui.painter(), rect, time);
            let col_w = (rect.width() - 80.0).clamp(320.0, 700.0);
            let note_lines = update::notes_to_lines(&match mode {
                Mode::Updated => self.whats_new.as_ref().and_then(|w| w.notes.clone()).unwrap_or_default(),
                Mode::Available => rel.as_ref().map(|r| r.notes.clone()).unwrap_or_default(),
            })
            .len() as f32;
            let card_h = if downloading { 120.0 } else { (note_lines * 30.0 + 76.0).clamp(110.0, (rect.height() * 0.34).clamp(120.0, 300.0) + 76.0) };
            let block_h = 30.0 + 18.0 + 56.0 + 14.0 + 36.0 + 26.0 + card_h + 26.0 + 100.0;
            let top = rect.top() + ((rect.height() - block_h) * 0.5).max(28.0) + (1.0 - ease(t)) * 24.0;
            let col = Rect::from_min_size(pos2(rect.center().x - col_w * 0.5, top), vec2(col_w, rect.height() - (top - rect.top()) - 24.0));
            let mut cui = ui.child_ui(col, egui::Layout::top_down(egui::Align::Center), None);
            let ui = &mut cui;
            ui.set_width(col_w);

            let a0 = item(0.0);
            let badge = crate::i18n::tr(if mode == Mode::Updated { "GÜNCELLENDİ" } else { "YENİ SÜRÜM" });
            let g = ui.painter().layout_no_wrap(badge, bold(11.0), alpha(SKY, (255.0 * a0) as u8));
            let (br, _) = ui.allocate_exact_size(g.size() + vec2(26.0, 12.0), Sense::hover());
            ui.painter().rect_stroke(br, Rounding::same(14.0), Stroke::new(1.0, alpha(SKY, (120.0 * a0) as u8)));
            ui.painter().galley(br.center() - g.size() * 0.5, g, alpha(SKY, (255.0 * a0) as u8));
            ui.add_space(18.0 + (1.0 - a0) * 10.0);

            let a1 = item(0.08);
            let title = match mode {
                Mode::Updated => crate::i18n::tr("Soundworks güncellendi"),
                Mode::Available => crate::i18n::tr(if downloading { "Güncelleniyor…" } else { "Yeni sürüm hazır" }),
            };
            ui.label(egui::RichText::new(title).font(bold(42.0)).color(alpha(Color32::WHITE, (255.0 * a1) as u8)));
            ui.add_space(14.0 + (1.0 - a1) * 12.0);

            let a2 = item(0.16);
            ui.scope(|ui| {
                ui.set_opacity(a2);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    match mode {
                        Mode::Updated => {
                            let v = self.whats_new.as_ref().map(|w| w.version.clone()).unwrap_or_default();
                            let label = format!("v{v}");
                            center_by(ui, col_w, ui.painter().layout_no_wrap(label.clone(), bold(15.0), TEXT).size().x + 28.0);
                            version_pill(ui, &label, true);
                        }
                        Mode::Available => {
                            let new_v = rel.as_ref().map(|r| r.version.clone()).unwrap_or_default();
                            let cur = format!("v{}", env!("CARGO_PKG_VERSION"));
                            let pw = |ui: &egui::Ui, t: &str| ui.painter().layout_no_wrap(t.to_string(), bold(15.0), TEXT).size().x + 28.0;
                            let new_label = format!("v{new_v}");
                            let total = pw(ui, &cur) + 28.0 + pw(ui, &new_label) + 24.0;
                            center_by(ui, col_w, total);
                            version_pill(ui, &cur, false);
                            let (ar, _) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
                            let slide = (time * 1.6).sin() * 2.5;
                            ui.painter().text(ar.center() + vec2(slide, 0.0), Align2::CENTER_CENTER, "→", bold(20.0), alpha(SKY, 230));
                            version_pill(ui, &new_label, true);
                        }
                    }
                });
            });
            ui.add_space(26.0 + (1.0 - a2) * 12.0);

            let a3 = item(0.28);
            let max_h = (rect.height() * 0.34).clamp(120.0, 300.0);
            let notes = match mode {
                Mode::Updated => self.whats_new.as_ref().and_then(|w| w.notes.clone()),
                Mode::Available => Some(rel.as_ref().map(|r| r.notes.clone()).unwrap_or_default()),
            };
            if !downloading || mode == Mode::Updated {
                match notes {
                    Some(n) if failed.is_none() || mode == Mode::Updated => notes_card(ui, &n, max_h, a3),
                    None => {
                        ui.label(egui::RichText::new(crate::i18n::tr("Sürüm notları alınıyor…")).font(body(14.0)).color(alpha(DIM, (255.0 * a3) as u8)));
                    }
                    _ => {}
                }
            }
            ui.add_space(26.0);

            let a4 = item(0.42);
            ui.scope(|ui| {
                ui.set_opacity(a4);
                match mode {
                    Mode::Updated => {
                        if big_button(ui, "Devam et", Icon::ChevronRight).clicked() {
                            act = Some("continue");
                        }
                    }
                    Mode::Available => {
                        if let (Some(rel), true) = (&rel, downloading) {
                            let done = self.upd_progress.load(Relaxed) as f32;
                            let total = (rel.size as f32).max(1.0);
                            let frac = (done / total).clamp(0.0, 1.0);
                            ui.label(egui::RichText::new(format!("{:.0}%", frac * 100.0)).font(bold(34.0)).color(Color32::WHITE));
                            ui.add_space(10.0);
                            let (bar, _) = ui.allocate_exact_size(vec2(col_w.min(460.0), 10.0), Sense::hover());
                            ui.painter().rect_filled(bar, Rounding::same(5.0), alpha(Color32::WHITE, 22));
                            let fill = Rect::from_min_size(bar.min, vec2((bar.width() * frac).max(10.0), bar.height()));
                            ui.painter().rect_filled(fill, Rounding::same(5.0), lerp_color(BLUE, SKY, frac));
                            let sh = ((time * 0.9) % 1.4) / 1.4;
                            let sx = fill.left() + fill.width() * sh;
                            ui.painter().with_clip_rect(fill).rect_filled(Rect::from_center_size(pos2(sx, fill.center().y), vec2(40.0, fill.height())), Rounding::same(5.0), alpha(Color32::WHITE, 70));
                            ui.add_space(12.0);
                            ui.label(egui::RichText::new(format!("{} {:.1} / {:.1} MB", crate::i18n::tr("İndiriliyor…"), done / 1_048_576.0, total / 1_048_576.0)).font(body(13.0)).color(DIM));
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(crate::i18n::tr("İndirme bitince Soundworks kendiliğinden yeniden başlar")).font(body(12.5)).color(alpha(DIM, 200)));
                        } else if let Some(e) = &failed {
                            ui.label(egui::RichText::new(crate::i18n::tr("Güncelleme tamamlanamadı")).font(bold(16.0)).color(Color32::WHITE));
                            ui.add_space(6.0);
                            ui.add(egui::Label::new(egui::RichText::new(e).font(body(13.0)).color(AMBER)).wrap());
                            ui.add_space(14.0);
                            ui.horizontal(|ui| {
                                center_by(ui, col_w, pill_w(ui, "Tekrar dene", true) + pill_w(ui, "Kapat", false) + 8.0);
                                if pill_button(ui, "Tekrar dene", Some(Icon::Undo), true).clicked() {
                                    act = Some("retry");
                                }
                                if pill_button(ui, "Kapat", None, false).clicked() {
                                    act = Some("close");
                                }
                            });
                        } else {
                            if big_button(ui, "Şimdi güncelle", Icon::Import).clicked() {
                                act = Some("install");
                            }
                            ui.add_space(14.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                let total = if self.upd_remind_menu {
                                    REMIND_CHOICES.iter().map(|(l, _)| pill_w(ui, l, false)).sum::<f32>() + 4.0 * (REMIND_CHOICES.len() as f32 - 1.0)
                                } else {
                                    link_w(ui, "Sonra hatırlat") + link_w(ui, "Bu sürümü atla") + link_w(ui, "Güncellemeleri hatırlatma") + 8.0
                                };
                                center_by(ui, col_w, total);
                                if self.upd_remind_menu {
                                    for (label, secs) in REMIND_CHOICES {
                                        if pill_button(ui, label, None, false).clicked() {
                                            remind_secs = secs;
                                            act = Some("remind");
                                        }
                                    }
                                } else {
                                    if link_button(ui, "Sonra hatırlat").clicked() {
                                        act = Some("remind_menu");
                                    }
                                    if link_button(ui, "Bu sürümü atla").clicked() {
                                        act = Some("skip");
                                    }
                                    if link_button(ui, "Güncellemeleri hatırlatma").clicked() {
                                        act = Some("never");
                                    }
                                }
                            });
                        }
                    }
                }
            });
        });

        self.upd_last_mode = Some(mode);
        let esc = self.upd_esc;
        match (mode, act) {
            (Mode::Updated, Some("continue")) => self.finish_whats_new(),
            (Mode::Updated, None) if esc => self.finish_whats_new(),
            (Mode::Available, Some("install")) => {
                if let Some(r) = rel {
                    self.update_install_async(r);
                }
            }
            (Mode::Available, Some("retry")) => {
                self.upd = UpdState::Idle;
                self.update_check_async(true);
            }
            (Mode::Available, Some("close")) => {
                self.upd_dialog = false;
                self.upd = UpdState::Idle;
            }
            (Mode::Available, Some("remind_menu")) => self.upd_remind_menu = true,
            (Mode::Available, Some("remind")) => {
                self.cfg.update_remind_at = now_secs() + remind_secs;
                self.upd_dialog = false;
                self.upd_remind_menu = false;
                self.save_cfg();
            }
            (Mode::Available, Some("skip")) => {
                if let Some(r) = &rel {
                    self.cfg.update_skip = r.version.clone();
                }
                self.upd_dialog = false;
                self.save_cfg();
            }
            (Mode::Available, Some("never")) => {
                self.cfg.update_check = false;
                self.upd_dialog = false;
                self.save_cfg();
                self.say("Otomatik denetim kapatıldı; Ayarlar'dan yeniden açabilirsin");
            }
            (Mode::Available, None) if esc && !downloading => {
                if self.upd_remind_menu {
                    self.upd_remind_menu = false;
                } else {
                    self.upd_dialog = false;
                }
            }
            _ => {}
        }
    }

    fn finish_whats_new(&mut self) {
        self.whats_new = None;
        self.notes_rx = None;
        if !self.cfg.update_notes.is_empty() {
            self.cfg.update_notes.clear();
            self.cfg.update_notes_version.clear();
            self.save_cfg();
        }
    }

    pub(super) fn update_settings(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            section_label(ui, "Güncellemeler");
            let version = env!("CARGO_PKG_VERSION");
            let status = match &self.upd {
                UpdState::Idle => {
                    if self.cfg.update_checked == 0 {
                        "Henüz denetlenmedi".to_string()
                    } else {
                        "Güncel sürümü kullanıyorsun".to_string()
                    }
                }
                UpdState::Checking => "Denetleniyor…".to_string(),
                UpdState::UpToDate => "Güncel sürümü kullanıyorsun".to_string(),
                UpdState::Available(r) | UpdState::Downloading(r) => format!("Yeni sürüm {} hazır", r.version),
                UpdState::Failed(e) => format!("Denetlenemedi: {e}"),
            };
            let busy = matches!(self.upd, UpdState::Checking | UpdState::Downloading(_));
            let title = format!("Soundworks {version}");
            let mut check = false;
            let mut show = false;
            let has_new = self.update_available().is_some();
            setting_row(ui, &title, &status, "", |ui| {
                if has_new {
                    if pill_button(ui, "Ayrıntılar", Some(Icon::Import), true).clicked() {
                        show = true;
                    }
                } else {
                    ui.add_enabled_ui(!busy, |ui| {
                        if pill_button(ui, "Şimdi denetle", Some(Icon::Undo), false).clicked() {
                            check = true;
                        }
                    });
                }
            });
            row_sep(ui);
            toggle_row(ui, "Güncellemeleri otomatik denetle", "Günde en fazla bir kez GitHub'a bakar, sormadan yüklemez", "", &mut self.cfg.update_check);
            if check {
                self.update_check_async(true);
            }
            if show {
                self.upd_dialog = true;
                self.upd_t0 = Instant::now();
            }
        });
        ui.add_space(12.0);
    }
}
