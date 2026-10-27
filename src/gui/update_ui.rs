use super::*;
use crate::update;
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

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl App {
    pub(super) fn update_available(&self) -> Option<&update::Release> {
        match &self.upd {
            UpdState::Available(r) | UpdState::Downloading(r) => Some(r),
            _ => None,
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
                        if manual {
                            self.upd_dialog = true;
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
        if matches!(self.upd, UpdState::Checking | UpdState::Downloading(_)) {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        let due = now_secs().saturating_sub(self.cfg.update_checked) > 12 * 3600;
        if self.cfg.update_check && !self.upd_auto_done && matches!(self.upd, UpdState::Idle) && due && self.upd_started.elapsed().as_secs() > 25 && crate::devenv::dev_var("SW_OFFSCREEN").is_err() && crate::devenv::dev_var("SW_SHOT").is_err() {
            self.upd_auto_done = true;
            self.update_check_async(false);
        }
    }

    pub(super) fn update_pill(&mut self, ui: &mut egui::Ui) {
        let Some(rel) = self.update_available() else { return };
        if rel.version == self.cfg.update_skip && !self.upd_dialog {
            return;
        }
        let label = format!("Yeni sürüm {}", rel.version);
        if pill_button(ui, &label, Some(Icon::Import), true).on_hover_text_tr("Yeni sürümün ayrıntılarını gör").clicked() {
            self.upd_dialog = true;
        }
    }

    pub(super) fn update_dialog(&mut self, ctx: &egui::Context) {
        if !self.upd_dialog {
            return;
        }
        let (rel, downloading) = match &self.upd {
            UpdState::Available(r) => (Some(r.clone()), false),
            UpdState::Downloading(r) => (Some(r.clone()), true),
            _ => (None, false),
        };
        let failed = if let UpdState::Failed(e) = &self.upd { Some(e.clone()) } else { None };
        if rel.is_none() && failed.is_none() {
            self.upd_dialog = false;
            return;
        }
        let mut open = true;
        let (mut install, mut later, mut skip, mut retry) = (false, false, false, false);
        egui::Window::new(crate::i18n::tr("Yeni sürüm hazır"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .fixed_size(vec2(460.0, 0.0))
            .show(ctx, |ui| {
                if let Some(rel) = &rel {
                    ui.label(RichText::new(format!("Soundworks {} → {}", env!("CARGO_PKG_VERSION"), rel.version)).font(bold(16.0)).color(Color32::WHITE));
                    ui.add_space(8.0);
                    if !rel.notes.trim().is_empty() {
                        egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                            ui.add(egui::Label::new(egui::RichText::new(rel.notes.trim()).font(body(12.5)).color(DIM)).wrap());
                        });
                        ui.add_space(10.0);
                    }
                    if downloading {
                        let done = self.upd_progress.load(Relaxed) as f32;
                        let total = (rel.size as f32).max(1.0);
                        ui.add(egui::ProgressBar::new((done / total).clamp(0.0, 1.0)).desired_height(8.0));
                        ui.add_space(4.0);
                        ui.label(RichText::new(format!("İndiriliyor… {:.1} / {:.1} MB", done / 1_048_576.0, total / 1_048_576.0)).font(body(12.0)).color(DIM));
                        ui.label(RichText::new("İndirme bitince uygulama kendiliğinden yeniden başlar").font(body(12.0)).color(DIM));
                    } else {
                        ui.horizontal(|ui| {
                            if pill_button(ui, "Şimdi güncelle", Some(Icon::Import), true).clicked() {
                                install = true;
                            }
                            if pill_button(ui, "Daha sonra", None, false).clicked() {
                                later = true;
                            }
                            if pill_button(ui, "Bu sürümü atla", None, false).clicked() {
                                skip = true;
                            }
                        });
                    }
                } else if let Some(e) = &failed {
                    ui.label(RichText::new("Güncelleme tamamlanamadı").font(bold(15.0)).color(Color32::WHITE));
                    ui.add_space(6.0);
                    ui.add(egui::Label::new(egui::RichText::new(e).font(body(12.5)).color(AMBER)).wrap());
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if pill_button(ui, "Tekrar dene", Some(Icon::Undo), false).clicked() {
                            retry = true;
                        }
                        if pill_button(ui, "Kapat", None, false).clicked() {
                            later = true;
                        }
                    });
                }
            });
        if !open || later {
            self.upd_dialog = false;
            if failed.is_some() {
                self.upd = UpdState::Idle;
            }
        }
        if skip {
            if let Some(r) = &rel {
                self.cfg.update_skip = r.version.clone();
                self.save_cfg();
            }
            self.upd_dialog = false;
        }
        if install {
            if let Some(r) = rel.clone() {
                self.update_install_async(r);
            }
        }
        if retry {
            self.upd = UpdState::Idle;
            self.update_check_async(true);
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
            }
        });
        ui.add_space(12.0);
    }
}
