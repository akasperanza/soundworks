//! Ön ayar özellikleri: Windows "Özellikler" penceresi gibi sekmeli düzenleme (Genel, Bantlar, Otomatik, Bilgi).
//! Hem "Oluştur > Sıfırdan oluştur"da, ön ayara sağ tık > Özellikler'de hem de Ayarlar > Ekolayzır > Ön ayarlar'da kullanılır.

use super::eqrules_ui::{artist_key, menu_row, scope_badge, upsert_rule, EqRule, Playing, RuleScope};
use super::*;
use std::time::SystemTime;

/// Ön ayarın dosyası dışında tutulan bilgileri (APO dosya biçimine dokunmadan).
#[derive(Serialize, Deserialize, Clone, PartialEq, Default, Debug)]
#[serde(default)]
pub(super) struct PresetMeta {
    /// hangi kulaklık / hoparlör için (serbest etiket)
    pub label: String,
    /// serbest not
    pub note: String,
    /// nereden geldi (ör. "AutoEQ · oratory1990")
    pub source: String,
}

impl PresetMeta {
    fn is_empty(&self) -> bool {
        self.label.is_empty() && self.note.is_empty() && self.source.is_empty()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PropsTab {
    General,
    Bands,
    Auto,
    Info,
}

/// Pencerede düzenlenen değerler; `base` ile karşılaştırılarak değişiklik olup olmadığı anlaşılır.
#[derive(Clone, PartialEq)]
struct PropsData {
    name: String,
    preamp: f64,
    bands: Vec<Band>,
    meta: PresetMeta,
    rules: Vec<EqRule>,
}

/// Açık özellikler düzenlemesinin durumu. Değişiklikler "Kaydet"e kadar yalnızca burada tutulur.
pub(super) struct PropsUi {
    /// düzenlenen ön ayarın dosya adı; `None` = yeni ön ayar oluşturuluyor
    pub orig: Option<String>,
    data: PropsData,
    base: PropsData,
    tab: PropsTab,
    scope: RuleScope,
    query: String,
    focus: bool,
    error: String,
    /// dosyanın son değişme zamanı (Bilgi sekmesi için)
    modified: Option<SystemTime>,
    t0: Instant,
}

impl PropsUi {
    /// doğrulama kancası: sekmeyi adından seçer
    pub fn dev_set_tab(&mut self, t: &str) {
        self.tab = match t {
            "bantlar" => PropsTab::Bands,
            "otomatik" => PropsTab::Auto,
            "bilgi" => PropsTab::Info,
            _ => PropsTab::General,
        };
    }

    pub fn is_dirty(&self) -> bool {
        self.data != self.base
    }
}

/// Eylem: sekme gövdesinden dışarıya (pencere ya da Ayarlar) iletilen istek.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum PropsAct {
    None,
    Save,
    Cancel,
    Delete,
    Duplicate,
    Export,
    Reveal,
}

/// Kullanıcının yazdığı adı dosya adına uygun hale getirir; geçersizse `None`.
pub(super) fn clean_name(raw: &str) -> Option<String> {
    let s = raw.trim().trim_end_matches(".txt").trim();
    let s: String = s.chars().map(|c| if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() { '-' } else { c }).collect();
    let s = s.trim().trim_end_matches('.').trim().to_string();
    if s.is_empty() || s.chars().count() > 80 {
        None
    } else {
        Some(s)
    }
}

/// Çalan parçadan verilen kapsam için (anahtar, ad, alt satır); albüm kütüphanede değilse `None`.
pub(super) fn playing_target(now: &Playing, scope: RuleScope) -> Option<(String, String, String)> {
    match scope {
        RuleScope::Track => Some((now.path.clone(), now.title.clone(), format!("{} · {}", now.artist, now.album_title))),
        RuleScope::Album => now.album_key.as_ref().map(|k| (k.clone(), now.album_title.clone(), now.artist.clone())),
        RuleScope::Artist if !now.artist.trim().is_empty() => Some((artist_key(&now.artist), now.artist.trim().to_string(), String::new())),
        _ => None,
    }
}

/// Kırpılmayı önleyen ön kazanç (dB): bantların toplam yanıtındaki en yüksek tepe kadar kıs (0,1 dB adımla).
pub(super) fn auto_preamp(bands: &[Band]) -> f64 {
    let fs = 48000.0;
    let coeffs: Vec<Coeffs> = bands.iter().filter(|b| b.freq < fs * 0.5).map(|b| Coeffs::design(b, fs)).collect();
    let peak = (0..240)
        .map(|i| {
            let f = 20.0 * 1000f64.powf(i as f64 / 239.0);
            coeffs.iter().map(|c| c.magnitude_db(f, fs)).sum::<f64>()
        })
        .fold(0.0, f64::max);
    -((peak * 10.0).ceil() / 10.0)
}

fn kind_tr(k: FilterKind) -> &'static str {
    match k {
        FilterKind::Peak => "Tepe (bell)",
        FilterKind::LowShelf => "Alçak raf",
        FilterKind::HighShelf => "Yüksek raf",
        FilterKind::LowPass => "Alçak geçiren",
        FilterKind::HighPass => "Yüksek geçiren",
        FilterKind::BandPass => "Bant geçiren",
        FilterKind::Notch => "Çentik",
        FilterKind::AllPass => "Tüm geçiren",
    }
}

fn since_text(t: SystemTime) -> String {
    let days = SystemTime::now().duration_since(t).map(|d| d.as_secs() / 86400).unwrap_or(0);
    match days {
        0 => "bugün".to_string(),
        1 => "dün".to_string(),
        n => format!("{n} gün önce"),
    }
}

impl App {
    /// Dosyadan bir düzenleme durumu kurar. Seçili ön ayarda kaydedilmemiş grafik düzenlemesi varsa onu başlangıç alır.
    pub(super) fn props_load(&self, file: &str) -> Option<PropsUi> {
        let preset = if self.preset_name.as_deref() == Some(file) && self.dirty { self.preset.clone() } else { apo::load(&presets_dir().join(file)).ok()? };
        let data = PropsData {
            name: file.trim_end_matches(".txt").to_string(),
            preamp: preset.preamp_db,
            bands: preset.bands.clone(),
            meta: self.cfg.preset_meta.get(file).cloned().unwrap_or_default(),
            rules: self.cfg.eq_rules.iter().filter(|r| r.preset.as_deref() == Some(file)).cloned().collect(),
        };
        let modified = std::fs::metadata(presets_dir().join(file)).ok().and_then(|m| m.modified().ok());
        Some(PropsUi { orig: Some(file.to_string()), base: data.clone(), data, tab: PropsTab::General, scope: RuleScope::Album, query: String::new(), focus: false, error: String::new(), modified, t0: Instant::now() })
    }

    fn props_new(&self) -> PropsUi {
        let mut n = 1;
        let name = loop {
            let name = if n == 1 { "Yeni ön ayar".to_string() } else { format!("Yeni ön ayar {n}") };
            if !presets_dir().join(format!("{name}.txt")).exists() {
                break name;
            }
            n += 1;
        };
        let data = PropsData { name, preamp: 0.0, bands: vec![Band { kind: FilterKind::Peak, freq: 1000.0, gain_db: 0.0, q: 1.0 }], meta: PresetMeta::default(), rules: vec![] };
        // yeni ön ayarda her şey "değişmiş" sayılır (Oluştur her zaman etkin)
        let mut base = data.clone();
        base.name.clear();
        PropsUi { orig: None, data, base, tab: PropsTab::General, scope: RuleScope::Album, query: String::new(), focus: true, error: String::new(), modified: None, t0: Instant::now() }
    }

    /// Özellikler penceresini açar: `Some(dosya adı)` mevcut ön ayar, `None` yeni ön ayar.
    pub(super) fn props_open(&mut self, preset: Option<&str>) {
        self.props = match preset {
            Some(f) => self.props_load(f),
            None => Some(self.props_new()),
        };
    }

    /// Değişiklikleri uygular: ad (yeniden adlandırma / yeni dosya), bantlar, ön kazanç, meta ve kurallar.
    pub(super) fn props_save(&mut self, st: &mut PropsUi) -> Result<(), String> {
        let name = clean_name(&st.data.name).ok_or("Geçerli bir ad yaz")?;
        if st.data.bands.is_empty() {
            return Err("En az bir bant olmalı".into());
        }
        let file = format!("{name}.txt");
        let target = presets_dir().join(&file);
        let _ = std::fs::create_dir_all(presets_dir());
        let renamed_from: Option<String> = match &st.orig {
            Some(old) if *old != file => {
                if target.exists() {
                    return Err("Bu adda bir ön ayar zaten var".into());
                }
                Some(old.clone())
            }
            None if target.exists() => return Err("Bu adda bir ön ayar zaten var".into()),
            _ => None,
        };
        // önce yeni içerik yazılır; yeniden adlandırmada eski dosya ancak yazma başarılıysa silinir
        let content = Preset { preamp_db: st.data.preamp, bands: st.data.bands.clone() };
        std::fs::write(&target, apo::to_text(&content)).map_err(|_| "Dosya yazılamadı".to_string())?;
        if let Some(old) = &renamed_from {
            let _ = std::fs::remove_file(presets_dir().join(old));
            // eski ada bağlı her şey yeni ada taşınır
            for r in self.cfg.eq_rules.iter_mut().filter(|r| r.preset.as_deref() == Some(old.as_str())) {
                r.preset = Some(file.clone());
            }
            if self.cfg.preset.as_deref() == Some(old.as_str()) {
                self.cfg.preset = Some(file.clone());
            }
            if self.preset_name.as_deref() == Some(old.as_str()) {
                self.preset_name = Some(file.clone());
            }
            self.cfg.preset_meta.remove(old);
        }
        if st.data.meta.is_empty() {
            self.cfg.preset_meta.remove(&file);
        } else {
            self.cfg.preset_meta.insert(file.clone(), st.data.meta.clone());
        }
        // bu ön ayarın kuralları pencerede gösterilenlerle değiştirilir
        self.cfg.eq_rules.retain(|r| r.preset.as_deref() != Some(file.as_str()));
        for mut r in st.data.rules.iter().cloned() {
            r.preset = Some(file.clone());
            upsert_rule(&mut self.cfg.eq_rules, r);
        }
        self.refresh_library();
        // çalan EQ bu ön ayarsa (ya da yenisi seçilecekse) değişiklik hemen duyulur
        if st.orig.is_none() || self.preset_name.as_deref() == Some(file.as_str()) {
            self.select(&file, false);
        }
        self.eq_rule_dirty = true;
        self.save_cfg();
        st.orig = Some(file.clone());
        st.data.name = name;
        st.base = st.data.clone();
        st.modified = Some(SystemTime::now());
        Ok(())
    }

    /// Ön ayarın bir kopyasını oluşturur ("Ad (kopya).txt").
    pub(super) fn duplicate_preset(&mut self, file: &str) {
        let stem = file.trim_end_matches(".txt");
        let mut n = 1;
        let copy = loop {
            let c = if n == 1 { format!("{stem} (kopya).txt") } else { format!("{stem} (kopya {n}).txt") };
            if !presets_dir().join(&c).exists() {
                break c;
            }
            n += 1;
        };
        if std::fs::copy(presets_dir().join(file), presets_dir().join(&copy)).is_ok() {
            if let Some(m) = self.cfg.preset_meta.get(file).cloned() {
                self.cfg.preset_meta.insert(copy.clone(), m);
            }
            self.refresh_library();
            self.save_cfg();
            self.say(format!("Kopya oluşturuldu: {}", copy.trim_end_matches(".txt")));
        }
    }

    pub(super) fn export_preset(&mut self, file: &str) {
        let Some(dest) = rfd::FileDialog::new().set_file_name(file).add_filter("Equalizer APO / AutoEQ", &["txt"]).save_file() else { return };
        match std::fs::copy(presets_dir().join(file), &dest) {
            Ok(_) => self.say("Dışa aktarıldı"),
            Err(_) => self.say("Dışa aktarılamadı"),
        }
    }

    pub(super) fn reveal_preset(&self, file: &str) {
        let _ = std::process::Command::new("explorer").arg(format!("/select,{}", presets_dir().join(file).display())).spawn();
    }

    /// Sekmeli gövde ve (yalnızca bilgi sekmesinde) dosya eylemleri. Pencerede ve Ayarlar'da aynı çizilir.
    pub(super) fn props_body(&self, ui: &mut egui::Ui, st: &mut PropsUi, now: &Option<Playing>) -> PropsAct {
        let mut act = PropsAct::None;
        segmented(
            ui,
            &mut st.tab,
            &[(PropsTab::General, "Genel", None), (PropsTab::Bands, "Bantlar", None), (PropsTab::Auto, "Otomatik", None), (PropsTab::Info, "Bilgi", None)],
        );
        ui.add_space(12.0);
        match st.tab {
            PropsTab::General => {
                ui.label(RichText::new("Ad").font(body(12.5)).color(DIM));
                let te = ui.add(egui::TextEdit::singleline(&mut st.data.name).desired_width(ui.available_width()).margin(vec2(10.0, 8.0)).font(body(15.0)));
                if st.focus {
                    te.request_focus();
                    st.focus = false;
                }
                if te.changed() {
                    st.error.clear();
                }
                ui.add_space(10.0);
                ui.label(RichText::new("Ön kazanç").font(body(12.5)).color(DIM));
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut st.data.preamp).range(-30.0..=10.0).speed(0.05).fixed_decimals(1).suffix(" dB"));
                    if pill_button(ui, "Otomatik", None, false).on_hover_text("Kırpılmayı önleyecek kadar kıs").clicked() {
                        st.data.preamp = auto_preamp(&st.data.bands);
                    }
                });
                ui.add_space(10.0);
                ui.label(RichText::new("Kulaklık / hoparlör").font(body(12.5)).color(DIM));
                ui.add(egui::TextEdit::singleline(&mut st.data.meta.label).hint_text("ör. Sennheiser HD 600").desired_width(ui.available_width()).margin(vec2(10.0, 7.0)));
                ui.add_space(10.0);
                ui.label(RichText::new("Not").font(body(12.5)).color(DIM));
                ui.add(egui::TextEdit::multiline(&mut st.data.meta.note).desired_rows(3).desired_width(ui.available_width()).margin(vec2(10.0, 7.0)));
            }
            PropsTab::Bands => {
                let mut remove: Option<usize> = None;
                // pencere yüksekliği içerikten kurulduğu için tablo yüksekliği açıkça verilir (aksi halde tek satıra sıkışır)
                let table_h = (st.data.bands.len() as f32 * 38.0 + 34.0).min(300.0);
                egui::ScrollArea::vertical().id_salt("props_bands").min_scrolled_height(table_h).max_height(table_h).auto_shrink([false, false]).show(ui, |ui| {
                    egui::Grid::new("props_band_grid").num_columns(6).spacing([10.0, 8.0]).show(ui, |ui| {
                        for h in ["#", "Tür", "Frekans", "Kazanç", "Q", ""] {
                            ui.label(RichText::new(h).font(body(11.5)).color(DIM));
                        }
                        ui.end_row();
                        for (i, b) in st.data.bands.iter_mut().enumerate() {
                            ui.label(RichText::new(format!("{}", i + 1)).font(body(12.5)).color(DIM));
                            egui::ComboBox::from_id_salt(("props_kind", i)).width(120.0).selected_text(kind_tr(b.kind)).show_ui(ui, |ui| {
                                for k in FilterKind::ALL {
                                    ui.selectable_value(&mut b.kind, k, kind_tr(k));
                                }
                            });
                            let sp = (b.freq * 0.01).max(0.5);
                            ui.add(egui::DragValue::new(&mut b.freq).range(10.0..=24000.0).speed(sp).fixed_decimals(0).suffix(" Hz"));
                            ui.add_enabled(b.kind.has_gain(), egui::DragValue::new(&mut b.gain_db).range(-24.0..=24.0).speed(0.05).fixed_decimals(1).suffix(" dB"));
                            ui.add(egui::DragValue::new(&mut b.q).range(0.1..=20.0).speed(0.01).fixed_decimals(2));
                            if icon_button(ui, Icon::Trash, 24.0, false).on_hover_text("Bandı sil").clicked() {
                                remove = Some(i);
                            }
                            ui.end_row();
                        }
                    });
                });
                if let Some(i) = remove {
                    st.data.bands.remove(i);
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(st.data.bands.len() < 31, |ui| {
                        if pill_button(ui, "Bant ekle", Some(Icon::Plus), false).clicked() {
                            st.data.bands.push(Band { kind: FilterKind::Peak, freq: 1000.0, gain_db: 0.0, q: 1.0 });
                        }
                    });
                    ui.label(RichText::new(format!("{} bant", st.data.bands.len())).font(body(12.0)).color(DIM));
                });
            }
            PropsTab::Auto => {
                ui.label(RichText::new("Şunlar çalınca ya da seçilince otomatik kullan").font(bold(13.5)).color(Color32::WHITE));
                ui.add_space(6.0);
                if st.data.rules.is_empty() {
                    ui.label(RichText::new("Kural yok").font(body(12.5)).color(DIM));
                }
                let mut remove: Option<usize> = None;
                for (i, r) in st.data.rules.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        scope_badge(ui, r.scope);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.set_width((ui.available_width() - 40.0).max(100.0));
                            ui.label(RichText::new(&r.label).font(bold(13.0)).color(Color32::WHITE));
                            if !r.sub.is_empty() {
                                ui.label(RichText::new(&r.sub).font(body(11.5)).color(DIM));
                            }
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icon_button(ui, Icon::Close, 24.0, false).on_hover_text("Kaldır").clicked() {
                                remove = Some(i);
                            }
                        });
                    });
                    ui.add_space(4.0);
                }
                if let Some(i) = remove {
                    st.data.rules.remove(i);
                }
                ui.add_space(8.0);
                let mut add: Option<(RuleScope, String, String, String)> = None;
                // çalan parçadan ve geçerli cihazdan hızlı ekleme
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
                    if let Some(n) = now {
                        for (scope, text) in [(RuleScope::Track, "Çalan şarkı"), (RuleScope::Album, "Çalan albüm"), (RuleScope::Artist, "Çalan sanatçı")] {
                            let target = playing_target(n, scope);
                            let already = target.as_ref().map_or(false, |(k, _, _)| st.data.rules.iter().any(|r| r.scope == scope && &r.key == k));
                            ui.add_enabled_ui(target.is_some() && !already, |ui| {
                                if pill_button(ui, text, Some(Icon::Plus), false).clicked() {
                                    add = target.clone().map(|(k, l, s)| (scope, k, l, s));
                                }
                            });
                        }
                    }
                    if let Some(id) = self.current_device_id() {
                        let name = self.devices.iter().find(|d| d.id == id).map(|d| d.name.clone()).unwrap_or_default();
                        let already = st.data.rules.iter().any(|r| r.scope == RuleScope::Device && r.key == id);
                        ui.add_enabled_ui(!name.is_empty() && !already, |ui| {
                            if pill_button(ui, "Bu çıkış cihazı", Some(Icon::Plus), false).on_hover_text(name.clone()).clicked() {
                                add = Some((RuleScope::Device, id.clone(), name.clone(), String::new()));
                            }
                        });
                    }
                });
                ui.add_space(8.0);
                segmented(
                    ui,
                    &mut st.scope,
                    &[(RuleScope::Track, "Şarkı", None), (RuleScope::Album, "Albüm", None), (RuleScope::Artist, "Sanatçı", None), (RuleScope::Device, "Cihaz", None)],
                );
                ui.add_space(6.0);
                ui.add(egui::TextEdit::singleline(&mut st.query).hint_text(if st.scope == RuleScope::Device { "Cihaz ara…" } else { "Kütüphanede ara…" }).desired_width(ui.available_width()).margin(vec2(10.0, 7.0)));
                for (k, l, s) in self.rule_candidates(st.scope, &st.query) {
                    if menu_row(ui, &l, &s, true).clicked() {
                        add = Some((st.scope, k, l, s));
                    }
                }
                if let Some((scope, key, label, sub)) = add {
                    upsert_rule(&mut st.data.rules, EqRule { scope, key, label, sub, preset: None });
                    st.query.clear();
                }
            }
            PropsTab::Info => {
                let file = st.orig.clone();
                // etiket üstte, değer altta ve satır kırılır: uzun dosya yolu pencereyi taşırmaz
                let row = |ui: &mut egui::Ui, k: &str, v: &str| {
                    ui.label(RichText::new(k).font(body(11.5)).color(DIM));
                    ui.add(egui::Label::new(RichText::new(v).font(body(12.5)).color(TEXT)).wrap());
                    ui.add_space(6.0);
                };
                match &file {
                    Some(f) => {
                        row(ui, "Dosya", &presets_dir().join(f).display().to_string());
                        row(ui, "Bant", &format!("{}", st.data.bands.len()));
                        if let Some(m) = st.modified {
                            row(ui, "Son değişiklik", &since_text(m));
                        }
                        if !st.data.meta.source.is_empty() {
                            row(ui, "Kaynak", &st.data.meta.source);
                        }
                        ui.add_space(12.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
                            if pill_button(ui, "Klasörde göster", Some(Icon::Folder), false).clicked() {
                                act = PropsAct::Reveal;
                            }
                            if pill_button(ui, "Çoğalt", Some(Icon::Plus), false).clicked() {
                                act = PropsAct::Duplicate;
                            }
                            if pill_button(ui, "Dışa aktar", Some(Icon::Upload), false).clicked() {
                                act = PropsAct::Export;
                            }
                            if pill_button(ui, "Sil", Some(Icon::Trash), false).clicked() {
                                act = PropsAct::Delete;
                            }
                        });
                    }
                    None => {
                        ui.label(RichText::new("Oluşturulunca dosya bilgisi burada görünür.").font(body(12.5)).color(DIM));
                    }
                }
            }
        }
        if !st.error.is_empty() {
            ui.add_space(8.0);
            ui.label(RichText::new(&st.error).font(body(12.0)).color(AMBER));
        }
        act
    }

    /// Dosya eylemleri (çoğalt, dışa aktar, klasörde göster, sil). `true` = ön ayar silindi (düzenleme kapanmalı).
    pub(super) fn props_file_act(&mut self, act: PropsAct, st: &PropsUi) -> bool {
        let Some(file) = st.orig.clone() else { return false };
        match act {
            PropsAct::Duplicate => self.duplicate_preset(&file),
            PropsAct::Export => self.export_preset(&file),
            PropsAct::Reveal => self.reveal_preset(&file),
            PropsAct::Delete => {
                self.delete_preset(&file);
                return true;
            }
            _ => {}
        }
        false
    }

    pub(super) fn props_overlay(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.props.take() else { return };
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return;
        }
        let now = self.playing_identity();
        let is_new = st.orig.is_none();
        let mut save = false;
        let mut cancel = false;
        let mut file_act = PropsAct::None;

        egui::Area::new(Id::new("props_dim")).order(egui::Order::Foreground).fixed_pos(pos2(0.0, 0.0)).show(ctx, |ui| {
            let r = ctx.screen_rect();
            ui.allocate_rect(r, Sense::click_and_drag());
            ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha(170));
        });
        egui::Window::new(if is_new { "Yeni ön ayar" } else { "Ön ayar özellikleri" })
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Tooltip)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .fixed_size(vec2(580.0, 0.0))
            .show(ctx, |ui| {
                file_act = self.props_body(ui, &mut st, &now);
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if pill_button(ui, if is_new { "Oluştur" } else { "Kaydet" }, Some(Icon::Save), true).clicked() {
                        save = true;
                    }
                    if pill_button(ui, if is_new { "Vazgeç" } else { "Kapat" }, Some(Icon::Close), false).clicked() {
                        cancel = true;
                    }
                    if st.is_dirty() && !is_new {
                        ui.label(RichText::new("kaydedilmemiş değişiklik").font(body(12.0)).color(AMBER));
                    }
                });
            });
        if st.t0.elapsed().as_millis() < 200 {
            ctx.request_repaint();
        }
        if matches!(file_act, PropsAct::Delete | PropsAct::Duplicate | PropsAct::Export | PropsAct::Reveal) {
            if self.props_file_act(file_act, &st) {
                return;
            }
        }
        if cancel {
            return;
        }
        let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter)) && st.tab == PropsTab::General && !st.data.name.trim().is_empty();
        if save || enter {
            match self.props_save(&mut st) {
                Ok(()) => {
                    self.say(if is_new { "Yeni ön ayar oluşturuldu" } else { "Kaydedildi" });
                    if is_new {
                        return;
                    }
                }
                Err(e) => st.error = e,
            }
        }
        self.props = Some(st);
    }

    /// Ayarlar > Ekolayzır > Ön ayarlar: aynı sekmeli düzenleme, ön ayar seçerek.
    pub(super) fn presets_manager_card(&mut self, ui: &mut egui::Ui) {
        let names: Vec<String> = self.library.iter().map(|(n, _)| n.clone()).collect();
        // seçili düzenleme yoksa ya da silindiyse çalan ön ayarla (yoksa ilkiyle) başla
        let valid = self.props_inline.as_ref().map_or(false, |s| s.orig.as_ref().map_or(false, |o| names.contains(o)));
        if !valid {
            let pick = self.preset_name.clone().filter(|n| names.contains(n)).or_else(|| names.first().cloned());
            self.props_inline = pick.and_then(|f| self.props_load(&f));
        }
        let mut switch_to: Option<String> = None;
        let mut act = PropsAct::None;
        let mut save = false;
        let mut revert = false;
        let mut create = false;
        let now = self.playing_identity();
        let mut st = self.props_inline.take();
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                section_label(ui, "Ön ayarlar");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if pill_button(ui, "Klasörü aç", Some(Icon::Folder), false).clicked() {
                        let _ = std::fs::create_dir_all(presets_dir());
                        let _ = std::process::Command::new("explorer").arg(presets_dir()).spawn();
                    }
                    if pill_button(ui, "Yeni", Some(Icon::Plus), false).clicked() {
                        create = true;
                    }
                });
            });
            ui.add_space(6.0);
            if names.is_empty() {
                ui.label(RichText::new("Henüz ön ayar yok").font(body(12.5)).color(DIM));
                return;
            }
            let cur = st.as_ref().and_then(|s| s.orig.clone());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for n in &names {
                    if Self::preset_chip(ui, n.trim_end_matches(".txt"), cur.as_deref() == Some(n.as_str())).clicked() && cur.as_deref() != Some(n.as_str()) {
                        switch_to = Some(n.clone());
                    }
                }
            });
            if let Some(s) = st.as_mut() {
                ui.add_space(12.0);
                Frame::none().fill(BG3).rounding(Rounding::same(10.0)).inner_margin(Margin::same(14.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    act = self.props_body(ui, s, &now);
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.add_enabled_ui(s.is_dirty(), |ui| {
                            if pill_button(ui, "Kaydet", Some(Icon::Save), true).clicked() {
                                save = true;
                            }
                            if pill_button(ui, "Geri al", Some(Icon::Undo), false).clicked() {
                                revert = true;
                            }
                        });
                        if s.is_dirty() {
                            ui.label(RichText::new("kaydedilmemiş değişiklik").font(body(12.0)).color(AMBER));
                        }
                    });
                });
            }
        });
        if create {
            self.props_inline = st;
            self.props_open(None);
            return;
        }
        if let Some(f) = switch_to {
            self.props_inline = self.props_load(&f);
            return;
        }
        if let Some(s) = st.as_mut() {
            if save {
                if let Err(e) = self.props_save(s) {
                    s.error = e;
                } else {
                    self.say("Kaydedildi");
                }
            }
        }
        if revert {
            if let Some(f) = st.as_ref().and_then(|s| s.orig.clone()) {
                self.props_inline = self.props_load(&f);
                return;
            }
        }
        if matches!(act, PropsAct::Delete | PropsAct::Duplicate | PropsAct::Export | PropsAct::Reveal) {
            if let Some(s) = &st {
                if self.props_file_act(act, s) {
                    self.props_inline = None;
                    return;
                }
            }
        }
        self.props_inline = st;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_cleaned_for_the_file_system() {
        assert_eq!(clean_name("  Rock  ").as_deref(), Some("Rock"));
        assert_eq!(clean_name("Rock.txt").as_deref(), Some("Rock"));
        assert_eq!(clean_name("A/B: \"x\"?").as_deref(), Some("A-B- -x--"));
        assert_eq!(clean_name("Bitti...").as_deref(), Some("Bitti"));
        assert_eq!(clean_name("   "), None);
        assert_eq!(clean_name(".txt"), None);
        assert_eq!(clean_name(&"x".repeat(81)), None);
        assert!(clean_name("Çay & Şeker").is_some(), "Türkçe karakterler kalır");
    }

    #[test]
    fn playing_target_per_scope() {
        let n = Playing { path: "p.flac".into(), title: "T".into(), album_key: Some("k".into()), album_title: "Alb".into(), artist: " Sanatçı ".into() };
        assert_eq!(playing_target(&n, RuleScope::Track).unwrap().0, "p.flac");
        assert_eq!(playing_target(&n, RuleScope::Album).unwrap().0, "k");
        assert_eq!(playing_target(&n, RuleScope::Artist).unwrap(), ("sanatçı".into(), "Sanatçı".into(), String::new()));
        assert!(playing_target(&n, RuleScope::Device).is_none(), "cihaz çalan parçadan türemez");
        let mut m = n.clone();
        m.album_key = None;
        m.artist = " ".into();
        assert!(playing_target(&m, RuleScope::Album).is_none(), "kütüphanede olmayan albüm");
        assert!(playing_target(&m, RuleScope::Artist).is_none(), "adsız sanatçı");
    }

    #[test]
    fn auto_preamp_cancels_the_boost_and_is_zero_for_cuts() {
        // +6 dB tepe: ön kazanç ≈ -6 dB (0,1 dB'ye yukarı yuvarlanır)
        let boost = vec![Band { kind: FilterKind::Peak, freq: 1000.0, gain_db: 6.0, q: 1.0 }];
        let p = auto_preamp(&boost);
        assert!(p <= -5.9 && p >= -6.3, "{p}");
        // yalnızca kesme: kırpılma yok, ön kazanç 0
        let cut = vec![Band { kind: FilterKind::Peak, freq: 1000.0, gain_db: -6.0, q: 1.0 }];
        assert_eq!(auto_preamp(&cut), 0.0);
        assert_eq!(auto_preamp(&[]), 0.0);
    }

    #[test]
    fn empty_meta_is_not_stored() {
        assert!(PresetMeta::default().is_empty());
        assert!(!PresetMeta { note: "x".into(), ..Default::default() }.is_empty());
    }

    #[test]
    fn meta_roundtrips_and_tolerates_missing_fields() {
        let m = PresetMeta { label: "HD 600".into(), note: "".into(), source: "AutoEQ · oratory1990".into() };
        let s = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<PresetMeta>(&s).unwrap(), m);
        assert_eq!(serde_json::from_str::<PresetMeta>("{}").unwrap(), PresetMeta::default());
    }
}
