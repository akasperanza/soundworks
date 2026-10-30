use super::*;

pub(super) fn apply(app: &mut App) {
    if let Ok(v) = crate::devenv::dev_var("SW_PLUGPANEL") {
        if let Some((p, id)) = v.split_once('/') {
            app.plugin_panel = Some((p.to_string(), id.to_string()));
        }
    }
    match crate::devenv::dev_var("SW_VIEW").as_deref() {
        Ok("settings") => app.view = View::Settings,
        Ok("extensions") => app.view = View::Extensions,
        Ok("library") => app.view = View::Library,
        _ => {}
    }
    if let Ok(m) = crate::devenv::dev_var("SW_MODE") {
        app.viz_mode = match m.as_str() {
            "gram" => VizMode::Spectrogram,
            "scope" => VizMode::Scope,
            _ => VizMode::Spectrum,
        };
    }
    if let Ok(v) = crate::devenv::dev_var("SW_UPDATE_FAKE") {
        app.upd = update_ui::UpdState::Available(crate::update::Release {
            version: v,
            notes: "- Bekleme ekranı: kapak, sıradakiler ve canlı spektrum\n- Eklenti sistemi: düğme, panel ve komutlar\n- Ayarlar yeniden tasarlandı\n- **Düzeltme:** seçim listelerinde çökme".into(),
            page: String::new(),
            exe_url: String::new(),
            sum_url: String::new(),
            size: 18_000_000,
        });
        app.upd_dialog = true;
        if crate::devenv::dev_var("SW_UPDATE_DL").is_ok() {
            if let update_ui::UpdState::Available(r) = std::mem::replace(&mut app.upd, update_ui::UpdState::Idle) {
                app.upd_progress.store(7_200_000, Relaxed);
                app.upd = update_ui::UpdState::Downloading(r);
            }
        }
    }
    if crate::devenv::dev_var("SW_UPDATE_NOW").is_ok() {
        app.update_check_async(true);
    }
    if let Ok(v) = crate::devenv::dev_var("SW_WHATSNEW") {
        app.cfg.update_notes = "- Bekleme ekranı: kapak, sıradakiler ve canlı spektrum\n- Eklenti sistemi: düğme, panel ve komutlar\n- Ayarlar yeniden tasarlandı\n- **Düzeltme:** seçim listelerinde çökme\n\n**Full Changelog**: https://github.com/x/y/compare/a...b".into();
        app.cfg.update_notes_version = v.clone();
        app.show_whats_new(v);
    }
    if crate::devenv::dev_var("SW_DETACH").is_ok() {
        app.detach_motor();
    }
    if crate::devenv::dev_var("SW_AMBIENT").is_ok() {
        app.ambient_on = true;
        app.ambient_side = crate::devenv::dev_var("SW_AMBSIDE").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        let extra: Vec<PathBuf> = app.lib.albums.iter().flat_map(|a| a.tracks.iter().map(|t| PathBuf::from(&t.path))).take(7).collect();
        app.player.add(extra);
    }
    if crate::devenv::dev_var("SW_QUEUE").is_ok() {
        app.queue_open = true;
    }

    if let Ok(v) = crate::devenv::dev_var("SW_SOLO") {
        app.solo_latch = true;
        app.sel_band = Some(v.parse().unwrap_or(4));
    }
    if let Some(m) = crate::devenv::dev_var("SW_RG").ok().and_then(|v| v.parse::<u32>().ok()) {
        app.cfg.rg_mode = m;
    }
    if crate::devenv::dev_var("SW_FXTAB").is_ok() {
        app.motor_tab = 1;
    }
    if let Ok(list) = crate::devenv::dev_var("SW_FX") {
        use crate::fx::{FxKind, FxSpec};
        for (i, n) in list.split(',').enumerate() {
            let k = match n.trim() {
                "limiter" => FxKind::Limiter,
                "comp" => FxKind::Compressor,
                "width" => FxKind::Width,
                "filter" => FxKind::Filter,
                "tone" => FxKind::Tone,
                "loud" => FxKind::Loudness,
                "cross" => FxKind::Crossfeed,
                _ => FxKind::Gain,
            };
            app.cfg.chain.push(FxSpec::new(i as u32 + 1, k));
        }
        app.fx_sel = app.cfg.chain.first().map(|s| s.id);
    }
    if let Some(w) = crate::devenv::dev_var("SW_FXW").ok().and_then(|v| v.parse::<f32>().ok()) {
        for s in app.cfg.chain.iter_mut().filter(|s| s.kind == crate::fx::FxKind::Width) {
            s.params = vec![w];
        }
    }
    if let Some(l) = crate::devenv::dev_var("SW_LANG").ok().and_then(|v| crate::i18n::Lang::from_code(&v)) {
        crate::i18n::set(l);
    }
    if let Some(z) = crate::devenv::dev_var("SW_ZOOM").ok().and_then(|v| v.parse::<u32>().ok()) {
        app.cfg.ui_zoom = z;
    }
    if let Ok(n) = crate::devenv::dev_var("SW_ALBUM") {
        app.view = View::Library;
        let idx: usize = n.parse().unwrap_or(0);
        app.lib_open = app.lib.albums.get(idx).map(|a| a.key.clone());
    }
    if let Ok(n) = crate::devenv::dev_var("SW_ARTIST") {
        app.view = View::Library;
        app.open_artist(&n);
    }
    if crate::devenv::dev_var("SW_DSD_DIALOG").is_ok() {

        app.dsd_dialog = Some(DsdDialog {
            files: vec![],
            start: 0,
            dev_id: "test".into(),
            dev_name: "FiiO K5 Pro (USB DAC)".into(),
            dop_rate: 176_400,
            t0: Instant::now(),
            probe: DsdProbe::Idle,
        });
    }
    if let Ok(v) = crate::devenv::dev_var("SW_EDIT_META") {
        app.dev_meta_edit(v == "album");
    }
    if let Ok(v) = crate::devenv::dev_var("SW_CONFIRM") {
        app.dev_confirm(v == "delete");
    }
    if crate::devenv::dev_var("SW_LYRICS").is_ok() {
        app.lyrics_open = true;
    }

    if let Ok(path) = crate::devenv::dev_var("SW_RULE_TRACK") {
        app.cfg.eq_rules.push(eqrules_ui::EqRule { scope: eqrules_ui::RuleScope::Track, key: path, label: "Test".into(), sub: String::new(), preset: Some("test.txt".into()) });
    }

    if let Some(t) = crate::devenv::dev_var("SW_SETTAB").ok().and_then(|v| v.parse::<u8>().ok()) {
        app.settings_tab = t;
    }

    if let Ok(v) = crate::devenv::dev_var("SW_PROPS") {
        app.props_open(if v == "yeni" { None } else { Some(v.as_str()) });
        if let (Some(p), Ok(t)) = (app.props.as_mut(), crate::devenv::dev_var("SW_PROPSTAB")) {
            p.dev_set_tab(&t);
        }
    }
    if let Ok(q) = crate::devenv::dev_var("SW_AUTOEQ") {
        app.ae_open(false);
        app.ae.query = q;
    }
    if let Ok(q) = crate::devenv::dev_var("SW_SEARCH") {
        app.open_search();
        app.search_query = q;
    }
    if crate::devenv::dev_var("SW_ARTISTS").is_ok() {
        app.lib_tab = 1;
    }
    if let Ok(i) = crate::devenv::dev_var("SW_OPEN") {
        app.lib_open = i.parse::<usize>().ok().and_then(|i| app.lib.albums.get(i)).map(|a| a.key.clone());
    }
    if let Ok(p) = crate::devenv::dev_var("SW_ADD") {
        let mut files = vec![];
        library::collect_audio(std::path::Path::new(&p), &mut files, 0);
        app.lib_add_paths(files);
    }
    if crate::devenv::dev_var("SW_UPLOAD").is_ok() {
        app.upload_open = true;
        app.upload_sub = crate::devenv::dev_var("SW_UPLOAD").map(|v| v == "sub").unwrap_or(false);
    }
    if crate::devenv::dev_var("SW_VOLPOP").is_ok() {
        app.vol_open = true;
    }
    if crate::devenv::dev_var("SW_LOOP").is_ok() {
        app.cfg.use_loopback = true;
    }
    if crate::devenv::dev_var("SW_PLAYER").is_ok() {
        app.cfg.use_loopback = false;
    }
    if let Ok(t) = crate::devenv::dev_var("SW_TO") {
        app.cfg.to = Some(t);
    }
    if let Ok(p) = crate::devenv::dev_var("SW_PLAYFILE") {
        app.player.clear();
        app.player.add(vec![PathBuf::from(p)]);
        app.play_pressed();
    }
    if crate::devenv::dev_var("SW_PLAY").is_ok() {
        app.play_pressed();
    }
}
