use super::*;
use crate::fileops::{self, DeleteReport, EditJob, EditReport};
use crate::tagwrite;
use std::path::Path;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Act {
    Play,
    Shuffle,
    Next,
    Queue,
    Reveal,
    Spectrum,
    Edit,
    CopyPath,
    Cover,
    Remove,
    Delete,
}

struct EditRow {
    field: &'static str,
    label: &'static str,
    text: String,
    orig: String,
    mixed: bool,
}

pub(super) struct MetaEdit {
    heading: String,
    items: Vec<(String, String)>,
    album_mode: bool,
    rows: Vec<EditRow>,
    skipped: usize,
    saving: bool,
}

pub(super) struct DeleteConfirm {
    paths: Vec<String>,
    title: String,
    disk: bool,
    album_key: Option<String>,
}

fn menu_item(ui: &mut egui::Ui, act: &mut Option<Act>, a: Act, text: &str, danger: bool) {
    let label = crate::i18n::tr(text);
    let resp = if danger { ui.button(RichText::new(label).color(RED)) } else { ui.button(label) };
    if resp.clicked() {
        *act = Some(a);
        ui.close_menu();
    }
}

pub(super) fn track_menu(ui: &mut egui::Ui) -> Option<Act> {
    let mut act = None;
    ui.set_min_width(210.0);
    menu_item(ui, &mut act, Act::Play, "Çal", false);
    menu_item(ui, &mut act, Act::Next, "Sıradaki olarak ekle", false);
    menu_item(ui, &mut act, Act::Queue, "Sıraya ekle", false);
    ui.separator();
    menu_item(ui, &mut act, Act::Reveal, "Dosya gezgininde göster", false);
    menu_item(ui, &mut act, Act::Spectrum, "Spektrum analizi…", false);
    menu_item(ui, &mut act, Act::Edit, "Metadatayı düzenle…", false);
    menu_item(ui, &mut act, Act::CopyPath, "Dosya yolunu kopyala", false);
    ui.separator();
    menu_item(ui, &mut act, Act::Remove, "Soundworks'ten sil", true);
    menu_item(ui, &mut act, Act::Delete, "Dosyayı sil…", true);
    act
}

pub(super) fn album_menu(ui: &mut egui::Ui) -> Option<Act> {
    let mut act = None;
    ui.set_min_width(210.0);
    menu_item(ui, &mut act, Act::Play, "Çal", false);
    menu_item(ui, &mut act, Act::Shuffle, "Karıştırarak çal", false);
    menu_item(ui, &mut act, Act::Next, "Sıradaki olarak ekle", false);
    menu_item(ui, &mut act, Act::Queue, "Sıraya ekle", false);
    ui.separator();
    menu_item(ui, &mut act, Act::Reveal, "Dosya gezgininde göster", false);
    menu_item(ui, &mut act, Act::Edit, "Metadatayı düzenle…", false);
    menu_item(ui, &mut act, Act::Cover, "Kapağı internetten ara", false);
    ui.separator();
    menu_item(ui, &mut act, Act::Remove, "Soundworks'ten sil", true);
    menu_item(ui, &mut act, Act::Delete, "Dosyaları sil…", true);
    act
}

pub(super) fn dim_backdrop(ctx: &egui::Context, id: &str) {
    egui::Area::new(Id::new(id)).order(egui::Order::Foreground).fixed_pos(pos2(0.0, 0.0)).show(ctx, |ui| {
        let r = ctx.screen_rect();
        ui.allocate_rect(r, Sense::click_and_drag());
        ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha(150));
    });
}

impl App {
    fn album_of(&self, path: &str) -> Option<&library::Album> {
        self.lib.albums.iter().find(|a| a.tracks.iter().any(|t| t.path == path))
    }

    pub(super) fn run_track_act(&mut self, ctx: &egui::Context, act: Act, path: &str) {
        let title = self.album_of(path).and_then(|a| a.tracks.iter().find(|t| t.path == path)).map(|t| t.title.clone()).unwrap_or_default();
        match act {
            Act::Next | Act::Queue => self.pending_queue = Some((vec![PathBuf::from(path)], act == Act::Next)),
            Act::Reveal => shell::shell_select(Path::new(path)),
            Act::Spectrum => self.open_spectrum(ctx, path),
            Act::CopyPath => ctx.copy_text(path.to_string()),
            Act::Edit => self.open_meta_edit(vec![path.to_string()], false, title),
            Act::Remove => self.del_confirm = Some(DeleteConfirm { paths: vec![path.to_string()], title, disk: false, album_key: None }),
            Act::Delete => self.del_confirm = Some(DeleteConfirm { paths: vec![path.to_string()], title, disk: true, album_key: None }),
            _ => {}
        }
    }

    pub(super) fn run_album_act(&mut self, act: Act, key: &str) {
        let Some(album) = self.lib.albums.iter().find(|a| a.key == key).cloned() else { return };
        let paths: Vec<String> = album.tracks.iter().map(|t| t.path.clone()).collect();
        match act {
            Act::Play => self.play_album(key, 0),
            Act::Shuffle => {
                if !paths.is_empty() {
                    self.player.shuffle.store(true, Relaxed);
                    self.cfg.shuffle = true;
                    let start = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos() as usize).unwrap_or(0) % paths.len();
                    self.play_album(key, start);
                }
            }
            Act::Next | Act::Queue => self.pending_queue = Some((album.paths(), act == Act::Next)),
            Act::Reveal => {
                if let Some(p) = paths.first() {
                    shell::shell_select(Path::new(p));
                }
            }
            Act::Edit => self.open_meta_edit(paths, true, album.title.clone()),
            Act::Cover => {
                self.lib.cover_tried.remove(key);
                self.cover_manual.insert(key.to_string());
                self.cover_fetcher.request(key.to_string(), album.artist.clone(), album.title.clone());
                self.say("Kapak aranıyor…");
            }
            Act::Remove => self.del_confirm = Some(DeleteConfirm { paths, title: album.title.clone(), disk: false, album_key: Some(key.to_string()) }),
            Act::Delete => self.del_confirm = Some(DeleteConfirm { paths, title: album.title.clone(), disk: true, album_key: Some(key.to_string()) }),
            Act::CopyPath | Act::Spectrum => {}
        }
    }

    fn open_meta_edit(&mut self, paths: Vec<String>, album_mode: bool, heading: String) {
        let writable: Vec<String> = paths.iter().filter(|p| tagwrite::can_write(Path::new(p))).cloned().collect();
        if writable.is_empty() {
            self.say("Bu dosya biçiminin etiketleri düzenlenemiyor");
            return;
        }
        let skipped = paths.len() - writable.len();
        let mut tags: Vec<Vec<(&'static str, String)>> = vec![];
        for p in &writable {
            match tagwrite::read_fields(Path::new(p)) {
                Ok(t) => tags.push(t),
                Err(_) => {
                    self.say("Dosyanın etiketleri okunamadı");
                    return;
                }
            }
        }
        let value = |t: &Vec<(&'static str, String)>, f: &str| t.iter().find(|(k, _)| *k == f).map(|(_, v)| v.clone()).unwrap_or_default();
        let spec: &[(&'static str, &'static str)] = if album_mode {
            &[("album", "Albüm"), ("album_artist", "Albüm sanatçısı"), ("artist", "Sanatçı"), ("year", "Yıl")]
        } else {
            &[("title", "Başlık"), ("artist", "Sanatçı"), ("album", "Albüm"), ("album_artist", "Albüm sanatçısı"), ("year", "Yıl"), ("track", "Parça no")]
        };
        let rows = spec
            .iter()
            .map(|(field, label)| {
                let first = value(&tags[0], field);
                let same = tags.iter().all(|t| value(t, field) == first);
                let v = if same { first } else { String::new() };
                EditRow { field, label, text: v.clone(), orig: v, mixed: !same }
            })
            .collect();
        let items = writable.iter().map(|p| (p.clone(), self.album_of(p).map(|a| a.key.clone()).unwrap_or_default())).collect();
        self.meta_edit = Some(MetaEdit { heading, items, album_mode, rows, skipped, saving: false });
    }

    #[cfg(feature = "devtools")]
    pub(super) fn dev_meta_edit(&mut self, album: bool) {
        let Some(a) = self.lib.albums.first().cloned() else { return };
        if album {
            self.open_meta_edit(a.tracks.iter().map(|t| t.path.clone()).collect(), true, a.title);
        } else if let Some(t) = a.tracks.first() {
            self.open_meta_edit(vec![t.path.clone()], false, t.title.clone());
        }
    }

    #[cfg(feature = "devtools")]
    pub(super) fn dev_confirm(&mut self, disk: bool) {
        let Some(a) = self.lib.albums.first().cloned() else { return };
        self.del_confirm = Some(DeleteConfirm { paths: a.tracks.iter().map(|t| t.path.clone()).collect(), title: a.title, disk, album_key: Some(a.key) });
    }

    pub(super) fn track_dialogs(&mut self, ctx: &egui::Context) {
        self.meta_edit_ui(ctx);
        self.confirm_ui(ctx);
        self.spectrum_ui(ctx);
    }

    fn meta_edit_ui(&mut self, ctx: &egui::Context) {
        let Some(ed) = self.meta_edit.as_mut() else { return };
        let (mut save, mut cancel) = (false, false);
        let changed = ed.rows.iter().any(|r| if ed.album_mode { !r.text.trim().is_empty() && r.text.trim() != r.orig.trim() } else { r.text.trim() != r.orig.trim() });
        dim_backdrop(ctx, "meta_edit_dim");
        egui::Window::new(crate::i18n::tr("Metadatayı düzenle"))
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Tooltip)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .fixed_size(vec2(440.0, 0.0))
            .show(ctx, |ui| {
                ui.add(egui::Label::new(RichText::new(&ed.heading).font(bold(15.0)).color(Color32::WHITE)).wrap());
                ui.add_space(2.0);
                let note = if ed.album_mode {
                    format!("{} parçanın etiketi değişir. Boş bıraktığın alanlar olduğu gibi kalır.", ed.items.len())
                } else {
                    "Boş bıraktığın alan dosyanın etiketinden silinir.".to_string()
                };
                ui.add(egui::Label::new(RichText::new(crate::i18n::tr(note)).font(body(12.5)).color(DIM)).wrap());
                ui.add_space(10.0);
                egui::Grid::new("meta_edit_grid").num_columns(2).spacing(vec2(12.0, 8.0)).show(ui, |ui| {
                    for r in ed.rows.iter_mut() {
                        ui.label(RichText::new(crate::i18n::tr(r.label)).font(body(13.0)).color(TEXT));
                        let hint = if r.mixed { "Farklı değerler" } else { "" };
                        ui.add_enabled(!ed.saving, egui::TextEdit::singleline(&mut r.text).hint_text_tr(hint).desired_width(300.0).margin(vec2(8.0, 6.0)));
                        ui.end_row();
                    }
                });
                if ed.skipped > 0 {
                    ui.add_space(8.0);
                    ui.add(egui::Label::new(RichText::new(crate::i18n::tr(format!("{} dosyanın biçimi etiket yazmayı desteklemiyor, atlanacak", ed.skipped))).font(body(12.0)).color(AMBER)).wrap());
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ed.saving {
                        ui.label(RichText::new(crate::i18n::tr("Dosyalara yazılıyor…")).font(body(13.0)).color(DIM));
                    } else {
                        ui.add_enabled_ui(changed, |ui| {
                            if pill_button(ui, "Kaydet", None, true).clicked() {
                                save = true;
                            }
                        });
                        if pill_button(ui, "Vazgeç", None, false).clicked() {
                            cancel = true;
                        }
                    }
                });
            });
        if ed.saving {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        } else if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        if save && !ed.saving {
            let edits: Vec<(&'static str, String)> = ed
                .rows
                .iter()
                .filter(|r| if ed.album_mode { !r.text.trim().is_empty() && r.text.trim() != r.orig.trim() } else { r.text.trim() != r.orig.trim() })
                .map(|r| (r.field, r.text.trim().to_string()))
                .collect();
            let jobs: Vec<(EditJob, String)> = ed.items.iter().map(|(p, k)| (EditJob { path: p.clone(), edits: edits.clone() }, k.clone())).collect();
            ed.saving = true;
            self.edit_rx = Some(fileops::spawn_edit(jobs));
        }
        if cancel {
            self.meta_edit = None;
        }
    }

    fn confirm_ui(&mut self, ctx: &egui::Context) {
        let Some(c) = self.del_confirm.as_ref() else { return };
        let (mut yes, mut no) = (false, false);
        let n = c.paths.len();
        let (head, text, button) = if c.disk {
            (
                format!("\"{}\" silinsin mi?", c.title),
                if n == 1 { "Dosya geri dönüşüm kutusuna taşınır ve kütüphaneden çıkarılır.".to_string() } else { format!("{n} dosya geri dönüşüm kutusuna taşınır ve kütüphaneden çıkarılır.") },
                "Dosyaları sil",
            )
        } else {
            (
                format!("\"{}\" Soundworks'ten silinsin mi?", c.title),
                "Dosyalar diskte kalır. Klasör taramaları bu parçaları yeniden eklemez.".to_string(),
                "Soundworks'ten sil",
            )
        };
        let disk = c.disk;
        dim_backdrop(ctx, "del_confirm_dim");
        egui::Window::new(crate::i18n::tr(if disk { "Emin misin?" } else { "Kütüphaneden sil" }))
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Tooltip)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .fixed_size(vec2(420.0, 0.0))
            .show(ctx, |ui| {
                ui.add(egui::Label::new(RichText::new(crate::i18n::tr(head)).font(bold(15.0)).color(Color32::WHITE)).wrap());
                ui.add_space(6.0);
                ui.add(egui::Label::new(RichText::new(crate::i18n::tr(text)).font(body(13.0)).color(TEXT)).wrap());
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if disk {
                        let b = egui::Button::new(RichText::new(crate::i18n::tr(button)).font(bold(13.0)).color(Color32::WHITE)).fill(alpha(RED, 200)).rounding(Rounding::same(16.0)).min_size(vec2(0.0, 30.0));
                        yes = ui.add(b).clicked();
                    } else {
                        yes = pill_button(ui, button, None, true).clicked();
                    }
                    no = pill_button(ui, "Vazgeç", None, false).clicked();
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            no = true;
        }
        if yes {
            if let Some(c) = self.del_confirm.take() {
                self.apply_delete(c);
            }
        } else if no {
            self.del_confirm = None;
        }
    }

    fn apply_delete(&mut self, c: DeleteConfirm) {
        let set: HashSet<String> = c.paths.iter().cloned().collect();
        if c.disk {
            let playing: Vec<usize> = {
                let pl = self.player.playlist.lock().unwrap();
                pl.iter().enumerate().filter(|(_, p)| set.contains(&p.to_string_lossy().to_string())).map(|(i, _)| i).collect()
            };
            for i in playing.into_iter().rev() {
                self.player.remove(i);
            }
            self.sync_playlist();
            self.lib.forget_tracks(&set, false);
            self.del_rx = Some(fileops::spawn_recycle(c.paths));
            self.say("Dosyalar geri dönüşüm kutusuna taşınıyor…");
        } else if let Some(key) = &c.album_key {
            self.remove_album_undoable(key);
            self.lib.forget_tracks(&set, true);
        } else {
            self.lib.forget_tracks(&set, true);
            self.say(format!("\"{}\" Soundworks'ten silindi", c.title));
        }
        self.lib.save();
        self.lib_tex.retain(|_, v| v.is_some());
        self.queue_sig = usize::MAX;
    }

    pub(super) fn poll_inbox(&mut self, ctx: &egui::Context) {
        if self.inbox_check.elapsed() < std::time::Duration::from_millis(400) {
            return;
        }
        self.inbox_check = Instant::now();
        for req in crate::shellmenu::drain() {
            match req.kind {
                crate::shellmenu::Kind::Add => self.lib_add_paths(req.paths),
                crate::shellmenu::Kind::Spectrum => {
                    if let Some(p) = req.paths.first() {
                        self.open_spectrum(ctx, &p.to_string_lossy());
                    }
                }
            }
        }
    }

    pub(super) fn poll_file_jobs(&mut self) {
        if let Some(rx) = &self.edit_rx {
            if let Ok(EditReport { done, failed }) = rx.try_recv() {
                self.edit_rx = None;
                self.meta_edit = None;
                let set: HashSet<String> = done.iter().map(|(p, _, _)| p.clone()).collect();
                for (_, old, new) in &done {
                    let (from, to) = (library::cover_path(old), library::cover_path(new));
                    if old != new && from.exists() && !to.exists() {
                        let _ = std::fs::copy(&from, &to);
                    }
                }
                self.lib.forget_tracks(&set, false);
                self.lib.save();
                self.lib_tex.retain(|_, v| v.is_some());
                self.queue_sig = usize::MAX;
                self.lib_add_paths(done.iter().map(|(p, _, _)| PathBuf::from(p)).collect());
                let msg = match (done.len(), failed.len()) {
                    (n, 0) => format!("{n} dosyanın etiketi güncellendi"),
                    (n, f) => format!("{n} dosyanın etiketi güncellendi, {f} dosyaya yazılamadı (kullanımda ya da salt okunur olabilir)"),
                };
                self.say(msg);
            }
        }
        if let Some(rx) = &self.del_rx {
            if let Ok(DeleteReport { failed }) = rx.try_recv() {
                self.del_rx = None;
                if failed > 0 {
                    self.say(format!("{failed} dosya silinemedi"));
                    self.rescan_library(true);
                } else {
                    self.say("Dosyalar geri dönüşüm kutusuna taşındı");
                }
            }
        }
    }
}
