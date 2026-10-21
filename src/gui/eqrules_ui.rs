//! Otomatik EQ kuralları: belirli bir şarkı, albüm ya da sanatçı çalınca ekolayzır kendiliğinden seçilen ön ayara geçer.
//! Öncelik şarkı > albüm > sanatçı; kural olmayan parçalarda kullanıcının seçili ön ayarına dönülür.

use super::*;
use egui::FontId;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum RuleScope {
    Track,
    Album,
    Artist,
    /// çıkış cihazı: o cihaz seçiliyken (çalan parçadan bağımsız) uygulanır
    Device,
}

impl RuleScope {
    fn name(self) -> &'static str {
        match self {
            RuleScope::Track => "ŞARKI",
            RuleScope::Album => "ALBÜM",
            RuleScope::Artist => "SANATÇI",
            RuleScope::Device => "CİHAZ",
        }
    }
    fn lower(self) -> &'static str {
        match self {
            RuleScope::Track => "şarkı",
            RuleScope::Album => "albüm",
            RuleScope::Artist => "sanatçı",
            RuleScope::Device => "cihaz",
        }
    }
    fn color(self) -> Color32 {
        match self {
            RuleScope::Track => SKY,
            RuleScope::Album => GREEN,
            RuleScope::Artist => AMBER,
            RuleScope::Device => Color32::from_rgb(190, 150, 255),
        }
    }
    /// Öncelik: küçük sayı önce kazanır.
    fn rank(self) -> u8 {
        match self {
            RuleScope::Track => 0,
            RuleScope::Album => 1,
            RuleScope::Artist => 2,
            RuleScope::Device => 3,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub(super) struct EqRule {
    pub scope: RuleScope,
    /// şarkı: dosya yolu, albüm: kütüphane anahtarı, sanatçı: küçük harfli ad
    pub key: String,
    /// listede gösterilen ad
    pub label: String,
    /// ikinci satır (ör. albümün sanatçısı)
    #[serde(default)]
    pub sub: String,
    /// ön ayar dosya adı; `None` = EQ'yu düz bırak
    pub preset: Option<String>,
}

/// Çalan parçanın kimliği (kuralları eşleştirmek için).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Playing {
    pub path: String,
    pub title: String,
    pub album_key: Option<String>,
    pub album_title: String,
    pub artist: String,
}

pub(super) fn artist_key(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Çalan parçaya ve çıkış cihazına uyan kuralı seçer: şarkı > albüm > sanatçı > cihaz.
/// Cihaz kuralı çalan parça olmadan da uygulanır (ör. kulaklık seçilince o kulaklığın düzeltmesi).
pub(super) fn resolve_rule<'a>(rules: &'a [EqRule], now: Option<&Playing>, device: Option<&str>) -> Option<&'a EqRule> {
    let ak = now.map(|n| artist_key(&n.artist)).unwrap_or_default();
    rules
        .iter()
        .filter(|r| match r.scope {
            RuleScope::Track => now.map_or(false, |n| r.key == n.path),
            RuleScope::Album => now.map_or(false, |n| n.album_key.as_deref() == Some(r.key.as_str())),
            RuleScope::Artist => !ak.is_empty() && r.key == ak,
            RuleScope::Device => device == Some(r.key.as_str()),
        })
        .min_by_key(|r| r.scope.rank())
}

/// Kural ekler; aynı kapsam ve anahtar için olan varsa onu günceller.
pub(super) fn upsert_rule(rules: &mut Vec<EqRule>, rule: EqRule) {
    match rules.iter_mut().find(|r| r.scope == rule.scope && r.key == rule.key) {
        Some(r) => *r = rule,
        None => rules.push(rule),
    }
}

/// Ayarlar'daki "kural ekle" formunun durumu.
pub(super) struct RuleNew {
    scope: RuleScope,
    query: String,
    picked: Option<(String, String, String)>,
    preset: Option<String>,
}

fn short_preset(p: &Option<String>) -> String {
    p.as_deref().map(|n| n.trim_end_matches(".txt").to_string()).unwrap_or_else(|| "EQ yok (düz)".to_string())
}

fn fit(painter: &egui::Painter, s: &str, font: FontId, max_w: f32) -> String {
    let w = |t: &str| painter.layout_no_wrap(t.to_string(), font.clone(), Color32::WHITE).size().x;
    if w(s) <= max_w {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        let t: String = chars[..mid].iter().collect::<String>() + "…";
        if w(&t) <= max_w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    chars[..lo].iter().collect::<String>() + "…"
}

/// Kapsam rozeti (ŞARKI / ALBÜM / SANATÇI).
pub(super) fn scope_badge(ui: &mut egui::Ui, scope: RuleScope) {
    let (rect, _) = ui.allocate_exact_size(vec2(64.0, 22.0), Sense::hover());
    ui.painter().rect_filled(rect, Rounding::same(6.0), alpha(scope.color(), 28));
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, scope.name(), bold(10.5), scope.color());
}

/// İki satırlı, tam genişlikte menü satırı.
pub(super) fn menu_row(ui: &mut egui::Ui, title: &str, sub: &str, enabled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().max(280.0), 42.0), if enabled { Sense::click() } else { Sense::hover() });
    let h = ui.ctx().animate_bool_with_time(resp.id, resp.hovered() && enabled, 0.1);
    ui.painter().rect_filled(rect, Rounding::same(8.0), alpha(BG3, (220.0 * h) as u8));
    let (c1, c2) = if enabled { (Color32::WHITE, DIM) } else { (alpha(DIM, 150), alpha(DIM, 110)) };
    ui.painter().text(pos2(rect.left() + 12.0, rect.top() + 14.0), Align2::LEFT_CENTER, title, bold(13.0), c1);
    let sub = fit(ui.painter(), sub, body(11.5), rect.width() - 24.0);
    ui.painter().text(pos2(rect.left() + 12.0, rect.top() + 30.0), Align2::LEFT_CENTER, sub, body(11.5), c2);
    resp
}

impl App {
    /// Çalan parçanın kimliği; kütüphanede yoksa etiketlerden (albüm anahtarı olmadan).
    pub(super) fn playing_identity(&self) -> Option<Playing> {
        let path = self.current_path()?;
        for a in &self.lib.albums {
            if let Some(t) = a.tracks.iter().find(|t| t.path == path) {
                return Some(Playing { path, title: t.title.clone(), album_key: Some(a.key.clone()), album_title: a.title.clone(), artist: a.artist.clone() });
            }
        }
        Some(Playing { path, title: self.meta.title.clone(), album_key: None, album_title: self.meta.album.clone(), artist: self.meta.artist.clone() })
    }

    /// Ön ayarı kalıcı seçimi (cfg.preset) değiştirmeden yükler: kural geçişi kullanıcının varsayılanını bozmaz.
    fn load_preset_quiet(&mut self, name: Option<&str>) -> bool {
        match name {
            None => {
                self.preset = Preset::default();
                self.preset_name = None;
            }
            Some(n) => match apo::load(&presets_dir().join(n)) {
                Ok(p) => {
                    self.preset = p;
                    self.preset_name = Some(n.to_string());
                }
                Err(_) => return false,
            },
        }
        self.dirty = false;
        self.sel_band = None;
        self.push_preset();
        true
    }

    /// Parça değişince (ya da kurallar değişince) uygun kuralı uygular; kural yoksa varsayılan ön ayara döner.
    pub(super) fn tick_eq_rules(&mut self) {
        let id = self.player.meta_id.load(Relaxed);
        let dev = self.current_device_id();
        if id == self.eq_rule_seen && dev == self.eq_rule_dev && !self.eq_rule_dirty {
            return;
        }
        self.eq_rule_seen = id;
        self.eq_rule_dev = dev.clone();
        self.eq_rule_dirty = false;
        if self.cfg.eq_rules.is_empty() && self.rule_active.is_none() {
            return;
        }
        let now = self.playing_identity();
        let rule = resolve_rule(&self.cfg.eq_rules, now.as_ref(), dev.as_deref()).cloned();
        match rule {
            Some(r) => {
                if self.rule_active.as_ref() == Some(&r) && self.preset_name == r.preset {
                    return;
                }
                if self.load_preset_quiet(r.preset.as_deref()) {
                    self.say(format!("Otomatik EQ: {}", short_preset(&r.preset)));
                    self.rule_active = Some(r);
                }
            }
            None => {
                if self.rule_active.take().is_some() {
                    let def = self.cfg.preset.clone();
                    if !self.load_preset_quiet(def.as_deref()) {
                        self.load_preset_quiet(None);
                    }
                    self.say("Otomatik EQ bitti: seçili ön ayara dönüldü");
                }
            }
        }
    }

    /// Hedef çıkış cihazının kimliği (Windows varsayılanı seçiliyse o an varsayılan olan cihaz).
    pub(super) fn current_device_id(&self) -> Option<String> {
        match &self.cfg.to {
            Some(id) => Some(id.clone()),
            None => self.devices.iter().find(|d| d.is_default).map(|d| d.id.clone()),
        }
    }

    fn add_rule_for_playing(&mut self, scope: RuleScope) {
        let Some(now) = self.playing_identity() else { return };
        let Some(preset) = self.preset_name.clone() else { return };
        let Some((key, label, sub)) = super::presetprops_ui::playing_target(&now, scope) else { return };
        upsert_rule(&mut self.cfg.eq_rules, EqRule { scope, key, label: label.clone(), sub, preset: Some(preset.clone()) });
        self.eq_rule_dirty = true;
        self.save_cfg();
        self.say(format!("Kural eklendi: {} {} → {}", scope.lower(), label, short_preset(&Some(preset))));
    }

    /// Motor > Ekolayzır'daki "Otomatik" düğmesi: şu anki ön ayarı çalan şarkı/albüm/sanatçı için otomatik kullan.
    pub(super) fn auto_eq_button(&mut self, ui: &mut egui::Ui) {
        let now = self.playing_identity();
        let active = self.rule_active.is_some();
        let btn = pill_button(ui, "Otomatik", Some(Icon::Wave), active).on_hover_text("Bu EQ'yu çalan şarkı, albüm ya da sanatçı için otomatik kullan");
        let id = btn.id.with("auto_eq");
        if btn.clicked() {
            ui.memory_mut(|m| m.toggle_popup(id));
        }
        let mut pick: Option<RuleScope> = None;
        let mut remove: Option<EqRule> = None;
        let preset = self.preset_name.clone();
        let saved = preset.is_some() && !self.dirty;
        let current_rule = resolve_rule(&self.cfg.eq_rules, now.as_ref(), self.current_device_id().as_deref()).cloned();
        egui::popup_below_widget(ui, id, &btn, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
            ui.set_min_width(300.0);
            ui.label(RichText::new("Otomatik EQ").font(bold(14.0)).color(Color32::WHITE));
            match (&now, saved) {
                (None, _) => {
                    ui.label(RichText::new("Bir şarkı çalarken kullanılabilir.").font(body(12.0)).color(DIM));
                }
                (Some(_), false) => {
                    ui.label(RichText::new("Önce EQ'yu kaydet ya da hazır bir ön ayar seç; kural kayıtlı ön ayarlarla çalışır.").font(body(12.0)).color(AMBER));
                }
                (Some(n), true) => {
                    ui.label(RichText::new(format!("\"{}\" ön ayarı şu durumda otomatik uygulansın:", short_preset(&preset))).font(body(12.0)).color(DIM));
                    ui.add_space(6.0);
                    if menu_row(ui, "Bu şarkı çalınca", &n.title, true).clicked() {
                        pick = Some(RuleScope::Track);
                    }
                    if menu_row(ui, "Bu albüm çalınca", &n.album_title, n.album_key.is_some()).on_disabled_hover_text("Albüm kütüphanede değil").clicked() {
                        pick = Some(RuleScope::Album);
                    }
                    if menu_row(ui, "Bu sanatçı çalınca", &n.artist, !n.artist.trim().is_empty()).clicked() {
                        pick = Some(RuleScope::Artist);
                    }
                }
            }
            if let Some(r) = &current_rule {
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(4.0);
                ui.label(RichText::new(format!("Şu an geçerli kural: {} → {}", r.scope.lower(), short_preset(&r.preset))).font(body(12.0)).color(GREEN));
                if pill_button(ui, "Bu kuralı kaldır", Some(Icon::Trash), false).clicked() {
                    remove = Some(r.clone());
                }
            }
        });
        if let Some(s) = pick {
            self.add_rule_for_playing(s);
            ui.memory_mut(|m| m.close_popup());
        }
        if let Some(r) = remove {
            self.remove_rule(&r);
            ui.memory_mut(|m| m.close_popup());
        }
    }

    /// Kuralı siler ("Geri al" ile geri getirilebilir).
    pub(super) fn remove_rule(&mut self, r: &EqRule) {
        if let Some(i) = self.cfg.eq_rules.iter().position(|x| x == r) {
            let rule = self.cfg.eq_rules.remove(i);
            let msg = format!("{} kuralı kaldırıldı: {}", rule.scope.lower(), rule.label);
            self.undo = Some((Undo::Rule(rule, i), msg, Instant::now()));
            self.eq_rule_dirty = true;
            self.save_cfg();
        }
    }

    /// Eşleşen kural varsa Motor sekmesinde gösterilen küçük bilgi çipi.
    pub(super) fn auto_eq_chip(&self, ui: &mut egui::Ui) {
        if let Some(r) = &self.rule_active {
            chip(ui, &format!("OTOMATİK EQ · {}", r.scope.name()), GREEN).on_hover_text(format!("{} \"{}\" için {} ön ayarı uygulanıyor", r.scope.lower(), r.label, short_preset(&r.preset)));
        }
    }

    pub(super) fn rule_candidates(&self, scope: RuleScope, q: &str) -> Vec<(String, String, String)> {
        let q = q.trim().to_lowercase();
        // cihaz listesi kısa: boş aramada hepsi gösterilir
        if q.chars().count() < 2 && scope != RuleScope::Device {
            return vec![];
        }
        let has = |s: &str| s.to_lowercase().contains(&q);
        let mut out = vec![];
        match scope {
            RuleScope::Artist => {
                for (name, n_albums, _) in self.lib.artists() {
                    if has(&name) {
                        out.push((artist_key(&name), name, format!("{n_albums} albüm")));
                    }
                }
            }
            RuleScope::Album => {
                for a in &self.lib.albums {
                    if has(&a.title) || has(&a.artist) {
                        out.push((a.key.clone(), a.title.clone(), a.artist.clone()));
                    }
                }
            }
            RuleScope::Track => {
                for a in &self.lib.albums {
                    for t in &a.tracks {
                        if has(&t.title) || has(&a.artist) {
                            out.push((t.path.clone(), t.title.clone(), format!("{} · {}", a.artist, a.title)));
                        }
                    }
                }
            }
            RuleScope::Device => {
                for d in &self.devices {
                    if has(&d.name) {
                        out.push((d.id.clone(), d.name.clone(), String::new()));
                    }
                }
            }
        }
        out.truncate(8);
        out
    }

    /// Ayarlar > Ekolayzır: kural listesi ve kural ekleme formu.
    pub(super) fn eq_rules_card(&mut self, ui: &mut egui::Ui) {
        let choices: Vec<(Option<String>, String)> = std::iter::once((None, "EQ yok (düz)".to_string()))
            .chain(self.library.iter().map(|(n, _)| (Some(n.clone()), n.trim_end_matches(".txt").to_string())))
            .collect();
        let mut remove: Option<usize> = None;
        let mut change: Option<(usize, Option<String>)> = None;
        let mut open_form = false;
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            section_label(ui, "Otomatik EQ kuralları");
            ui.add_space(2.0);
            ui.add(
                egui::Label::new(
                    RichText::new("Belirli bir şarkı, albüm ya da sanatçı çalınca ekolayzır kendiliğinden seçtiğin ön ayara geçer. Birden çok kural uyarsa şarkı, albümü; albüm de sanatçıyı geçer. Kuralı olmayan parçalarda seçili ön ayarına dönülür.")
                        .font(body(12.0))
                        .color(DIM),
                )
                .wrap(),
            );
            ui.add_space(10.0);
            if self.cfg.eq_rules.is_empty() {
                ui.label(RichText::new("Henüz kural yok. Motor > Ekolayzır'daki \"Otomatik\" düğmesinden ya da aşağıdan ekleyebilirsin.").font(body(12.5)).color(DIM));
            }
            for (i, r) in self.cfg.eq_rules.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let (rect, _) = ui.allocate_exact_size(vec2(64.0, 22.0), Sense::hover());
                    ui.painter().rect_filled(rect, Rounding::same(6.0), alpha(r.scope.color(), 28));
                    ui.painter().text(rect.center(), Align2::CENTER_CENTER, r.scope.name(), bold(10.5), r.scope.color());
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.set_width((ui.available_width() - 250.0).max(120.0));
                        ui.label(RichText::new(&r.label).font(bold(13.0)).color(Color32::WHITE));
                        if !r.sub.is_empty() {
                            ui.label(RichText::new(&r.sub).font(body(11.5)).color(DIM));
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_button(ui, Icon::Trash, 26.0, false).on_hover_text("Kuralı kaldır").clicked() {
                            remove = Some(i);
                        }
                        let mut cur = r.preset.clone();
                        egui::ComboBox::from_id_salt(("rule_preset", i)).width(190.0).selected_text(short_preset(&cur)).show_ui(ui, |ui| {
                            for (val, name) in &choices {
                                ui.selectable_value(&mut cur, val.clone(), name);
                            }
                        });
                        if cur != r.preset {
                            change = Some((i, cur));
                        }
                    });
                });
                ui.add_space(6.0);
            }
            ui.add_space(4.0);
            if self.rule_new.is_none() {
                if pill_button(ui, "Kural ekle", Some(Icon::Plus), false).clicked() {
                    open_form = true;
                }
            } else {
                self.rule_form(ui, &choices);
            }
        });
        if open_form {
            self.rule_new = Some(RuleNew { scope: RuleScope::Album, query: String::new(), picked: None, preset: self.preset_name.clone() });
        }
        if let Some((i, p)) = change {
            if let Some(r) = self.cfg.eq_rules.get_mut(i) {
                r.preset = p;
                self.eq_rule_dirty = true;
            }
        }
        if let Some(i) = remove {
            let r = self.cfg.eq_rules[i].clone();
            self.remove_rule(&r);
        }
    }

    fn rule_form(&mut self, ui: &mut egui::Ui, choices: &[(Option<String>, String)]) {
        let Some(mut f) = self.rule_new.take() else { return };
        let mut done = false;
        let mut add = false;
        Frame::none().fill(BG3).rounding(Rounding::same(10.0)).inner_margin(Margin::same(14.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Yeni kural").font(bold(13.5)).color(Color32::WHITE));
            ui.add_space(8.0);
            let before = f.scope;
            segmented(ui, &mut f.scope, &[(RuleScope::Track, "Şarkı", None), (RuleScope::Album, "Albüm", None), (RuleScope::Artist, "Sanatçı", None)]);
            if f.scope != before {
                f.query.clear();
                f.picked = None;
            }
            ui.add_space(8.0);
            let mut unpick = false;
            match f.picked.clone() {
                Some((_, label, sub)) => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(label).font(bold(13.0)).color(Color32::WHITE));
                        if !sub.is_empty() {
                            ui.label(RichText::new(sub).font(body(12.0)).color(DIM));
                        }
                        if ui.add(egui::Label::new(RichText::new("değiştir").font(body(12.0)).color(SKY)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            unpick = true;
                        }
                    });
                }
                None => {
                    ui.add(
                        egui::TextEdit::singleline(&mut f.query)
                            .hint_text(format!("Kütüphanede {} ara…", f.scope.lower()))
                            .desired_width(ui.available_width())
                            .margin(vec2(10.0, 7.0)),
                    );
                    let cands = self.rule_candidates(f.scope, &f.query);
                    if cands.is_empty() && f.query.trim().chars().count() >= 2 {
                        ui.label(RichText::new("Eşleşen kayıt yok").font(body(12.0)).color(DIM));
                    }
                    for (k, l, s) in cands {
                        if menu_row(ui, &l, &s, true).clicked() {
                            f.picked = Some((k, l, s));
                            break;
                        }
                    }
                }
            }
            if unpick {
                f.picked = None;
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Kullanılacak EQ").font(body(12.5)).color(DIM));
                egui::ComboBox::from_id_salt("rule_new_preset").width(200.0).selected_text(short_preset(&f.preset)).show_ui(ui, |ui| {
                    for (val, name) in choices {
                        ui.selectable_value(&mut f.preset, val.clone(), name);
                    }
                });
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.add_enabled_ui(f.picked.is_some(), |ui| {
                    if pill_button(ui, "Ekle", Some(Icon::Plus), true).clicked() {
                        add = true;
                    }
                });
                if pill_button(ui, "Vazgeç", Some(Icon::Close), false).clicked() {
                    done = true;
                }
            });
        });
        if add {
            if let Some((key, label, sub)) = f.picked.clone() {
                upsert_rule(&mut self.cfg.eq_rules, EqRule { scope: f.scope, key, label, sub, preset: f.preset.clone() });
                self.eq_rule_dirty = true;
                done = true;
            }
        }
        if !done {
            self.rule_new = Some(f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(scope: RuleScope, key: &str, preset: &str) -> EqRule {
        EqRule { scope, key: key.into(), label: key.into(), sub: String::new(), preset: Some(preset.into()) }
    }
    fn now() -> Playing {
        Playing { path: "C:\\m\\a.flac".into(), title: "A".into(), album_key: Some("k1".into()), album_title: "Alb".into(), artist: "  Teddy Picker ".into() }
    }

    #[test]
    fn track_beats_album_beats_artist() {
        let rules = vec![rule(RuleScope::Artist, "teddy picker", "art.txt"), rule(RuleScope::Album, "k1", "alb.txt"), rule(RuleScope::Track, "C:\\m\\a.flac", "trk.txt")];
        let n = now();
        assert_eq!(resolve_rule(&rules, Some(&n), None).unwrap().preset.as_deref(), Some("trk.txt"));
        assert_eq!(resolve_rule(&rules[..2], Some(&n), None).unwrap().preset.as_deref(), Some("alb.txt"));
        assert_eq!(resolve_rule(&rules[..1], Some(&n), None).unwrap().preset.as_deref(), Some("art.txt"));
        assert!(resolve_rule(&[], Some(&n), None).is_none());
    }

    #[test]
    fn rules_only_match_their_own_target() {
        let rules = vec![rule(RuleScope::Track, "başka.flac", "x.txt"), rule(RuleScope::Album, "k2", "y.txt"), rule(RuleScope::Artist, "başka sanatçı", "z.txt")];
        assert!(resolve_rule(&rules, Some(&now()), None).is_none());
        // albüm anahtarı olmayan (kütüphanede olmayan) parçaya albüm kuralı uygulanmaz
        let mut n = now();
        n.album_key = None;
        assert!(resolve_rule(&[rule(RuleScope::Album, "k1", "y.txt")], Some(&n), None).is_none());
        // sanatçı adı boşsa eşleşme olmaz
        n.artist = "  ".into();
        assert!(resolve_rule(&[rule(RuleScope::Artist, "", "z.txt")], Some(&n), None).is_none());
    }

    #[test]
    fn artist_keys_ignore_case_and_padding() {
        assert_eq!(artist_key("  Teddy PICKER "), "teddy picker");
        let rules = vec![rule(RuleScope::Artist, "teddy picker", "art.txt")];
        assert!(resolve_rule(&rules, Some(&now()), None).is_some());
    }

    #[test]
    fn device_rule_is_lowest_priority_and_needs_no_track() {
        let rules = vec![rule(RuleScope::Device, "dev1", "kulaklik.txt"), rule(RuleScope::Artist, "teddy picker", "art.txt")];
        // çalan parça yokken cihaz kuralı uygulanır
        assert_eq!(resolve_rule(&rules, None, Some("dev1")).unwrap().preset.as_deref(), Some("kulaklik.txt"));
        // parça kuralı cihaz kuralını geçer
        assert_eq!(resolve_rule(&rules, Some(&now()), Some("dev1")).unwrap().preset.as_deref(), Some("art.txt"));
        // başka cihazda cihaz kuralı yok
        assert!(resolve_rule(&rules[..1], Some(&now()), Some("dev2")).is_none());
        assert!(resolve_rule(&rules[..1], None, None).is_none());
    }

    #[test]
    fn upsert_replaces_same_target_instead_of_duplicating() {
        let mut rules = vec![rule(RuleScope::Album, "k1", "a.txt")];
        upsert_rule(&mut rules, rule(RuleScope::Album, "k1", "b.txt"));
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].preset.as_deref(), Some("b.txt"));
        upsert_rule(&mut rules, rule(RuleScope::Artist, "k1", "c.txt"));
        assert_eq!(rules.len(), 2, "farklı kapsam ayrı kural");
    }

    #[test]
    fn rules_roundtrip_through_json() {
        let r = vec![rule(RuleScope::Track, "p", "x.txt"), EqRule { scope: RuleScope::Artist, key: "a".into(), label: "A".into(), sub: "".into(), preset: None }];
        let s = serde_json::to_string(&r).unwrap();
        let back: Vec<EqRule> = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
    }
}
