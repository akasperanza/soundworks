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

fn hero_font(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name("wordmark".into()))
}

fn fade_in(age: f32, delay: f32) -> f32 {
    ease(((age - delay) / 0.45).clamp(0.0, 1.0))
}

fn solid_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let g = ui.painter().layout_no_wrap(crate::i18n::tr(text), bold(14.0), Color32::WHITE);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 48.0, 46.0), Sense::click());
    let h = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.12);
    ui.painter().rect_filled(rect, Rounding::same(7.0), lerp_color(BLUE, SKY, 0.25 * h));
    ui.painter().galley(rect.center() - g.size() * 0.5, g, Color32::WHITE);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn text_link(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let g = ui.painter().layout_no_wrap(crate::i18n::tr(text), body(13.5), DIM);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 4.0, 30.0), Sense::click());
    let h = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.12);
    let col = lerp_color(alpha(Color32::WHITE, 150), Color32::WHITE, h);
    ui.painter().galley(pos2(rect.left(), rect.center().y - g.size().y * 0.5), g, col);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn hairline(ui: &mut egui::Ui, a: f32) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, alpha(Color32::WHITE, (22.0 * a) as u8));
}

struct Screen<'a> {
    mode: Mode,
    downloading: bool,
    failed: Option<&'a str>,
    notes: Option<String>,
    version: String,
    remind_menu: bool,
    progress: (f32, f32),
    age: f32,
}

fn draw_notes(ui: &mut egui::Ui, s: &Screen, max_h: f32) {
    ui.label(egui::RichText::new(crate::i18n::tr("YENİLİKLER")).font(bold(11.0)).color(alpha(TEXT, (140.0 * fade_in(s.age, 0.15)) as u8)));
    ui.add_space(10.0);
    hairline(ui, fade_in(s.age, 0.15));
    let Some(notes) = &s.notes else {
        ui.add_space(14.0);
        ui.label(egui::RichText::new(crate::i18n::tr("Sürüm notları alınıyor…")).font(body(14.0)).color(DIM));
        return;
    };
    let lines = update::notes_to_lines(&update::pick_language(notes, crate::i18n::get().code()));
    if lines.is_empty() {
        ui.add_space(14.0);
        ui.label(egui::RichText::new(crate::i18n::tr("Bu sürüm için not yok")).font(body(14.0)).color(DIM));
        return;
    }
    egui::ScrollArea::vertical().max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
        ui.set_width(ui.available_width());
        for (i, l) in lines.iter().enumerate() {
            let a = fade_in(s.age, 0.22 + 0.05 * i as f32);
            match l {
                NoteLine::Heading(h) => {
                    ui.add_space(16.0);
                    ui.label(egui::RichText::new(h.to_uppercase()).font(bold(11.0)).color(alpha(SKY, (200.0 * a) as u8)));
                    ui.add_space(6.0);
                }
                NoteLine::Bullet(b) => {
                    ui.add_space(11.0);
                    ui.add(egui::Label::new(egui::RichText::new(b).font(body(15.0)).color(alpha(Color32::WHITE, (235.0 * a) as u8))).wrap());
                    ui.add_space(11.0);
                    hairline(ui, a);
                }
                NoteLine::Text(x) => {
                    ui.add_space(10.0);
                    ui.add(egui::Label::new(egui::RichText::new(x).font(body(13.5)).color(alpha(DIM, (255.0 * a) as u8))).wrap());
                    ui.add_space(6.0);
                }
            }
        }
    });
}

fn draw_actions(ui: &mut egui::Ui, s: &Screen, act: &mut Option<&'static str>, remind: &mut u64) {
    let a = fade_in(s.age, 0.3);
    ui.scope(|ui| {
        ui.set_opacity(a);
        match s.mode {
            Mode::Updated => {
                if solid_button(ui, "Devam et").clicked() {
                    *act = Some("continue");
                }
            }
            Mode::Available if s.downloading => {
                let (done, total) = s.progress;
                let frac = (done / total.max(1.0)).clamp(0.0, 1.0);
                ui.label(egui::RichText::new(format!("{:.0}%", frac * 100.0)).font(hero_font(44.0)).color(Color32::WHITE));
                ui.add_space(10.0);
                let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 3.0), Sense::hover());
                ui.painter().rect_filled(bar, 0.0, alpha(Color32::WHITE, 28));
                ui.painter().rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * frac, bar.height())), 0.0, BLUE);
                ui.add_space(10.0);
                ui.label(egui::RichText::new(format!("{:.1} / {:.1} MB", done / 1_048_576.0, total / 1_048_576.0)).font(body(13.0)).color(DIM));
                ui.add_space(14.0);
                ui.add(egui::Label::new(egui::RichText::new(crate::i18n::tr("İndirme bitince Soundworks kendiliğinden yeniden başlar")).font(body(13.0)).color(alpha(DIM, 200))).wrap());
            }
            Mode::Available => {
                if let Some(e) = s.failed {
                    ui.label(egui::RichText::new(crate::i18n::tr("Güncelleme tamamlanamadı")).font(bold(16.0)).color(Color32::WHITE));
                    ui.add_space(6.0);
                    ui.add(egui::Label::new(egui::RichText::new(e).font(body(13.0)).color(AMBER)).wrap());
                    ui.add_space(16.0);
                    if solid_button(ui, "Tekrar dene").clicked() {
                        *act = Some("retry");
                    }
                    ui.add_space(8.0);
                    if text_link(ui, "Kapat").clicked() {
                        *act = Some("close");
                    }
                } else {
                    if solid_button(ui, "Şimdi güncelle").clicked() {
                        *act = Some("install");
                    }
                    ui.add_space(18.0);
                    if s.remind_menu {
                        for (label, secs) in REMIND_CHOICES {
                            if text_link(ui, label).clicked() {
                                *remind = secs;
                                *act = Some("remind");
                            }
                        }
                        if text_link(ui, "← Geri").clicked() {
                            *act = Some("back");
                        }
                    } else {
                        if text_link(ui, "Sonra hatırlat").clicked() {
                            *act = Some("remind_menu");
                        }
                        if text_link(ui, "Bu sürümü atla").clicked() {
                            *act = Some("skip");
                        }
                        if text_link(ui, "Güncellemeleri hatırlatma").clicked() {
                            *act = Some("never");
                        }
                    }
                }
            }
        }
    });
}

fn draw_screen(ui: &mut egui::Ui, rect: Rect, s: &Screen) -> (Option<&'static str>, u64) {
    let (mut act, mut remind) = (None, 0u64);
    ui.painter().rect_filled(rect, 0.0, BG0);
    let wide = rect.width() >= 820.0;
    let total_w = (rect.width() - 120.0).clamp(320.0, 1040.0);
    let left_w = if wide { total_w * 0.40 } else { total_w };
    let gap = 72.0;
    let x0 = rect.center().x - total_w * 0.5;
    let line_count = s.notes.as_deref().map_or(0, |n| update::notes_to_lines(&update::pick_language(n, crate::i18n::get().code())).len());
    let right_est = 60.0 + line_count as f32 * 46.0;
    let left_est: f32 = 380.0;
    let block_h = if wide { left_est.max(right_est.min(rect.height() - 160.0)) } else { left_est + 40.0 + right_est.min(260.0) };
    let top = (rect.top() + (rect.height() - block_h) * 0.5).max(rect.top() + 48.0);
    let bottom = rect.bottom() - 56.0;

    let left_rect = Rect::from_min_max(pos2(x0, top), pos2(x0 + left_w, bottom));
    let mut lui = ui.new_child(egui::UiBuilder::new().max_rect(left_rect).layout(egui::Layout::top_down(egui::Align::Min)));
    let l = &mut lui;
    let a0 = fade_in(s.age, 0.0);
    let kicker = match s.mode {
        Mode::Updated => "GÜNCELLENDİ",
        Mode::Available => "YENİ SÜRÜM",
    };
    l.label(egui::RichText::new(crate::i18n::tr(kicker)).font(bold(11.5)).color(alpha(SKY, (230.0 * a0) as u8)));
    let a1 = fade_in(s.age, 0.05);
    l.add_space(8.0 + (1.0 - a1) * 14.0);
    l.label(egui::RichText::new(&s.version).font(hero_font(112.0)).color(alpha(Color32::WHITE, (255.0 * a1) as u8)));
    l.add_space(-6.0 - (1.0 - a1) * 14.0);
    let a2 = fade_in(s.age, 0.12);
    let (title, sub) = match s.mode {
        Mode::Updated => (crate::i18n::tr("Soundworks güncellendi"), String::new()),
        Mode::Available if s.downloading => (crate::i18n::tr("Güncelleniyor…"), format!("v{} → v{}", env!("CARGO_PKG_VERSION"), s.version)),
        Mode::Available => (crate::i18n::tr("Yeni sürüm hazır"), format!("v{} → v{}", env!("CARGO_PKG_VERSION"), s.version)),
    };
    l.label(egui::RichText::new(title).font(bold(22.0)).color(alpha(Color32::WHITE, (255.0 * a2) as u8)));
    if !sub.is_empty() {
        l.add_space(4.0);
        l.label(egui::RichText::new(sub).font(body(14.0)).color(alpha(DIM, (255.0 * a2) as u8)));
    }
    l.add_space(30.0);
    draw_actions(l, s, &mut act, &mut remind);

    let right_rect = if wide {
        Rect::from_min_max(pos2(x0 + left_w + gap, top), pos2(x0 + total_w, bottom))
    } else {
        let y = lui.min_rect().bottom() + 36.0;
        Rect::from_min_max(pos2(x0, y), pos2(x0 + total_w, bottom.max(y + 80.0)))
    };
    let mut rui = ui.new_child(egui::UiBuilder::new().max_rect(right_rect).layout(egui::Layout::top_down(egui::Align::Min)));
    let max_h = (right_rect.height() - 40.0).max(80.0);
    draw_notes(&mut rui, s, max_h);

    ui.painter().text(pos2(x0, rect.bottom() - 30.0), Align2::LEFT_CENTER, "Soundworks", hero_font(15.0), alpha(DIM, 150));
    (act, remind)
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
                        if crate::devenv::dev_var("SW_UPDATE_AUTOINSTALL").is_ok() {
                            self.update_install_async(rel.clone());
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
        let age = self.upd_t0.elapsed().as_secs_f32();
        ctx.request_repaint_after(std::time::Duration::from_millis(16));

        let (rel, downloading) = match &self.upd {
            UpdState::Available(r) => (Some(r.clone()), false),
            UpdState::Downloading(r) => (Some(r.clone()), true),
            _ => (None, false),
        };
        let failed = if let UpdState::Failed(e) = &self.upd { Some(e.clone()) } else { None };
        let mut act = None::<&'static str>;
        let mut remind_secs = 0u64;

        let version = match mode {
            Mode::Updated => self.whats_new.as_ref().map(|w| w.version.clone()).unwrap_or_default(),
            Mode::Available => rel.as_ref().map(|r| r.version.clone()).unwrap_or_default(),
        };
        let notes = match mode {
            Mode::Updated => self.whats_new.as_ref().and_then(|w| w.notes.clone()),
            Mode::Available => Some(rel.as_ref().map(|r| r.notes.clone()).unwrap_or_default()),
        };
        let screen_data = Screen {
            mode,
            downloading,
            failed: failed.as_deref(),
            notes,
            version,
            remind_menu: self.upd_remind_menu,
            progress: (self.upd_progress.load(Relaxed) as f32, rel.as_ref().map_or(1.0, |r| r.size as f32)),
            age,
        };
        egui::Area::new(Id::new("upd_full")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            ui.set_opacity(ease(t));
            let (rect, _) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
            let (a, r) = draw_screen(ui, rect, &screen_data);
            act = a;
            remind_secs = r;
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
            (Mode::Available, Some("back")) => self.upd_remind_menu = false,
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
