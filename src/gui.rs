use crate::apo::{self, Band, FilterKind, Preset};
use crate::biquad::Coeffs;
use crate::engine::{self, Control, DeviceInfo, Options};
use crate::artist;
use crate::library::{self, Library, ScanMsg};
use crate::lyrics::{self, Lyrics};
use crate::wave;
use crate::player::{Player, TrackMeta, AUDIO_EXTS};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::mpsc::Receiver;
use crate::ui::*;
use crate::viz::{self, VizMode, VizState};
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, ColorImage, Frame, Id, Margin, Rect, Rounding, Sense, Shape, Stroke,
    TextureHandle, TextureOptions,
};
use serde::{Deserialize, Serialize};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

static WINDOW_HIDDEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static QUIT_REQ: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static QUIT_ACK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(feature = "devtools")]
fn dev_trace(msg: &str) {
    use std::io::Write;
    static T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let t0 = T0.get_or_init(Instant::now);
    if let Ok(path) = crate::devenv::dev_var("SW_TRACE") {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{:8.3} {msg}", t0.elapsed().as_secs_f64());
        }
    }
}
#[cfg(not(feature = "devtools"))]
fn dev_trace(_: &str) {}

#[cfg(feature = "devtools")]
mod devlog {
    use std::collections::HashMap;
    use std::sync::Mutex;
    pub static COUNTS: Mutex<Option<HashMap<String, u32>>> = Mutex::new(None);
    struct L;
    impl log::Log for L {
        fn enabled(&self, _: &log::Metadata) -> bool {
            true
        }
        fn log(&self, r: &log::Record) {
            let msg = format!("{} {}", r.target(), r.args());
            let key: String = msg.chars().filter(|c| !c.is_ascii_digit()).take(70).collect();
            *COUNTS.lock().unwrap().get_or_insert_with(HashMap::new).entry(key).or_insert(0) += 1;
        }
        fn flush(&self) {}
    }
    pub fn init() {
        let _ = log::set_logger(&L);
        log::set_max_level(log::LevelFilter::Trace);
    }
    pub fn take_top(n: usize) -> String {
        let mut g = COUNTS.lock().unwrap();
        let m = g.get_or_insert_with(HashMap::new);
        let mut v: Vec<(String, u32)> = m.drain().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v.iter().take(n).map(|(k, c)| format!("{c}x[{k}]")).collect::<Vec<_>>().join(" ")
    }
}

#[cfg(feature = "devtools")]
fn dev_skip(name: &str) -> bool {
    crate::devenv::dev_var("SW_SKIP").map(|v| v.split(',').any(|x| x == name)).unwrap_or(false)
}
#[cfg(not(feature = "devtools"))]
fn dev_skip(_: &str) -> bool {
    false
}

#[cfg(feature = "devtools")]
static UPDATE_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[cfg(feature = "devtools")]
struct UpdateTimer(Instant);
#[cfg(feature = "devtools")]
impl Drop for UpdateTimer {
    fn drop(&mut self) {
        UPDATE_US.fetch_add(self.0.elapsed().as_micros() as u64, Relaxed);
    }
}

const GAIN_RANGE: f64 = 18.0;
const EQ_POINTS: usize = 600;
const RATES: [(u32, &str); 6] =
    [(44100, "44.1 kHz"), (48000, "48 kHz"), (88200, "88.2 kHz"), (96000, "96 kHz"), (176400, "176.4 kHz"), (192000, "192 kHz")];

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
struct Config {
    preset: Option<String>,
    from: Option<String>,
    to: Option<String>,
    use_loopback: bool,
    exclusive_off: bool,
    autostart: bool,

    show_engine_btn: bool,

    #[serde(skip_serializing)]
    playlist: Vec<String>,
    volume: Option<f32>,
    level_match_off: bool,
    repeat: bool,
    repeat_one: bool,
    shuffle: bool,
    player_rate: u32,

    match_rate: bool,

    resume: Option<(usize, f64)>,

    rg_mode: u32,
    rg_preamp: f32,
    rg_prevent: bool,

    tips_on: bool,

    tips_dismissed: Vec<String>,
    crossfeed: f32,
    balance: f32,
    mono: bool,
    viz_bars: bool,
    viz_mode: u8,

    auto_route: bool,

    restore_default: Option<String>,
    tray_on_close: bool,
    run_at_login: bool,
    fetch_genres: bool,
    group_by_genre: bool,

    scan_on_start: bool,

    dsd_prefs: BTreeMap<String, u8>,

    lyr_offsets: BTreeMap<String, i32>,

    ui_zoom: u32,
    ambient: bool,
    ambient_secs: u32,
    #[serde(default)]
    lang: Option<String>,

    dither: bool,

    chain: Vec<crate::fx::FxSpec>,

    eq_rules: Vec<eqrules_ui::EqRule>,

    preset_meta: BTreeMap<String, presetprops_ui::PresetMeta>,

    win: Option<[f32; 6]>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            preset: None,
            from: None,
            to: None,
            use_loopback: false,
            exclusive_off: false,
            autostart: false,
            show_engine_btn: true,
            playlist: vec![],
            volume: None,
            level_match_off: false,
            repeat: false,
            repeat_one: false,
            shuffle: false,
            player_rate: 48000,
            match_rate: true,
            resume: None,
            rg_mode: 0,
            rg_preamp: 0.0,
            rg_prevent: true,
            tips_on: true,
            tips_dismissed: Vec::new(),
            crossfeed: 0.0,
            balance: 0.0,
            mono: false,
            viz_bars: false,
            viz_mode: 0,
            auto_route: true,
            restore_default: None,
            tray_on_close: true,
            run_at_login: false,
            fetch_genres: true,
            group_by_genre: true,
            scan_on_start: true,
            dsd_prefs: BTreeMap::new(),
            lyr_offsets: BTreeMap::new(),
            ui_zoom: 0,
            ambient: true,
            ambient_secs: 60,
            lang: None,
            dither: true,
            chain: Vec::new(),
            eq_rules: Vec::new(),
            preset_meta: BTreeMap::new(),
            win: None,
        }
    }
}

fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Soundworks")
}
fn presets_dir() -> PathBuf {
    data_dir().join("presets")
}
fn queue_path() -> PathBuf {
    data_dir().join("queue.json")
}
fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

mod artist_ui;
mod dsd_ui;
mod engine_ctl;
mod eq_ui;
mod fx_ui;
mod library_ui;
mod lyrics_ui;
mod panels;
mod proof_ui;
mod ambient_ui;
mod queue_ui;
mod search_ui;
mod autoeq_ui;
mod eqrules_ui;
mod presetprops_ui;
mod settings_ui;
mod shell;
mod tips_ui;
pub use shell::run;
use shell::{hide_window, make_tray, set_run_at_login, style_titlebar};
#[cfg(feature = "devtools")]
use shell::request_quit;

enum Undo {
    Album(library::Album, usize),
    Preset(String, Vec<u8>),
    Rule(eqrules_ui::EqRule, usize),
}

struct Running {
    ctl: Arc<Control>,
    handle: JoinHandle<()>,

    dop: bool,
}

struct DsdDialog {
    files: Vec<PathBuf>,
    start: usize,
    dev_id: String,
    dev_name: String,
    dop_rate: u32,
    t0: Instant,
    probe: DsdProbe,
}

enum DsdProbe {
    Idle,
    Running(std::sync::mpsc::Receiver<Result<(), String>>),

    Ask,
    Failed(String),
}

const DOP_LOCK_SECS: f32 = 3.0;

#[derive(PartialEq, Clone, Copy)]
enum View {
    Motor,
    Library,
    Settings,
}

enum SearchItem {
    Artist(String),
    Album(String),
    Track(String, usize),
}

#[derive(PartialEq, Clone, Copy)]
enum RTab {
    Playing,
    List,
}

pub struct App {
    cfg: Config,
    devices: Vec<DeviceInfo>,
    library: Vec<(String, Preset)>,
    preset_name: Option<String>,
    preset: Preset,
    dirty: bool,
    running: Option<Running>,
    message: String,
    player: Arc<Player>,
    view: View,
    view_t0: Instant,
    rtab: RTab,
    sel_band: Option<usize>,

    solo_target: Option<(f64, f64)>,

    solo_latch: bool,
    drag_band: Option<usize>,
    viz: VizState,
    viz_mode: VizMode,
    last_frame: Instant,

    meta: TrackMeta,
    cover_id: u64,
    cover_tex: Option<TextureHandle>,

    wordmark_tex: Option<(TextureHandle, f32)>,
    cover_t0: Instant,
    ambient_on: bool,
    ambient_side: u8,
    ambient_ptr: Option<egui::Pos2>,
    ambient_since: Instant,
    ambient_t: f32,
    ambient_last: Instant,
    tint: Color32,
    tint_target: Color32,
    toast: Option<(String, Instant)>,
    frames: u32,
    _tray: Option<TrayIcon>,

    tray_eq: Option<CheckMenuItem>,
    start_hidden: bool,
    quitting: bool,
    lib: Library,
    lib_scans: Vec<(Receiver<ScanMsg>, usize, usize)>,

    lib_missing: HashSet<String>,
    missing_rx: Option<Receiver<HashSet<String>>>,
    lib_added: usize,
    lib_open: Option<String>,
    lib_query: String,
    lib_tex: HashMap<String, Option<TextureHandle>>,
    lib_tint: HashMap<String, Color32>,
    titlebar_done: bool,
    chain_sent: Vec<crate::fx::FxSpec>,
    rate_cache: HashMap<String, Vec<u32>>,

    recover: Option<(bool, Instant)>,
    dsd_dialog: Option<DsdDialog>,

    dop_rate_cache: HashMap<String, Vec<u32>>,

    last_vol: f32,

    eq_cache: Option<(Preset, u32, Arc<Vec<f64>>)>,

    stopping: Option<JoinHandle<()>>,

    idle_since: Option<Instant>,

    queued_start: Option<bool>,
    loud: Arc<crate::loudness::Scanner>,
    loud_started: bool,
    loud_cached: usize,
    loud_cached_t: Instant,
    target_dev: Option<(engine::DeviceKind, String, String)>,
    tip_seen: std::collections::HashSet<&'static str>,
    tip_cands: Vec<crate::tips::Tip>,
    tip_cur: Option<crate::tips::Tip>,
    tip_last: Option<crate::tips::Tip>,
    tip_cur_since: Option<Instant>,
    tip_eval: Instant,
    rate_checked: bool,
    start_rate: Option<u32>,

    rate_notice: Option<(u32, u32)>,
    motor_tab: u8,
    fx_drag: Option<fx_ui::FxDrag>,
    fx_sel: Option<u32>,
    fx_rects: Vec<Rect>,
    fx_zone: Rect,
    fx_eq_right: f32,
    frames_run: u32,
    backdrop: Color32,
    backdrop_a: f32,
    tex_budget: u8,
    fetcher: artist::Fetcher,
    lyr_fetcher: lyrics::LyricsFetcher,
    lyrics_open: bool,
    queue_open: bool,

    queue_map: HashMap<String, (String, String)>,
    queue_sig: usize,
    resume_saved: Instant,

    pending_queue: Option<(Vec<PathBuf>, bool)>,
    undo: Option<(Undo, String, Instant)>,

    lyr_hold: Option<(Instant, Instant, i32)>,

    proof_id: u64,
    proof_base: crate::proof::Counters,
    proof_view: crate::proof::Counters,
    proof_dry_shown: u64,
    proof_alt_seen: u64,
    proof_alt_at: Instant,

    probe_rates: Vec<u32>,

    settings_tab: u8,

    preset_q: String,
    preset_q_focus: bool,

    proof_cause_at: [Instant; 6],
    proof_cause_mask: u32,
    proof_frames_seen: u64,
    wave_fetcher: wave::WaveFetcher,
    wave_cache: HashMap<String, Arc<Vec<f32>>>,
    wave_drag: Option<f32>,
    lyr_cache: HashMap<String, Lyrics>,
    lyr_failed: HashSet<String>,
    lyr_layout: Option<LyrLayout>,
    lyr_scroll: f32,
    lyr_vel: f32,
    lyr_pos: f64,
    lyr_manual: f32,
    lyr_manual_t: Instant,
    lyr_key: String,
    view_epoch: u32,
    nav_t0: Instant,
    nav_sig: (Option<String>, Option<String>, u8),
    search_t0: Instant,
    vol_open: bool,
    vol_t: Instant,
    vol_popup_rect: Rect,
    vol_prev: f32,
    upload_open: bool,
    upload_sub: bool,
    genre_fetcher: artist::GenreFetcher,
    genre_retry: HashMap<String, Instant>,
    genre_unsaved: u32,
    artist_cache: HashMap<String, artist::ArtistInfo>,
    artist_failed: HashSet<String>,
    artist_small: HashMap<String, Option<(TextureHandle, f32, f32)>>,
    artist_big: HashMap<String, Option<(TextureHandle, f32, f32)>>,
    lib_tab: u8,

    lib_q: String,
    lib_genre: Option<String>,
    lib_sort: [u8; 2],
    lib_artist: Option<String>,
    bio_open: bool,
    search_open: bool,

    autoeq: crate::autoeq::Client,
    ae: autoeq_ui::AeUi,

    eq_rule_seen: u64,
    eq_rule_dev: Option<String>,
    eq_rule_dirty: bool,
    rule_active: Option<eqrules_ui::EqRule>,
    rule_new: Option<eqrules_ui::RuleNew>,

    props: Option<presetprops_ui::PropsUi>,

    props_inline: Option<presetprops_ui::PropsUi>,
    search_query: String,
    search_sel: usize,
    search_focus: bool,
}

impl App {
    fn new(ctx: &egui::Context) -> App {
        let mut cfg: Config = crate::fsutil::read_json(&config_path()).unwrap_or_default();
        if cfg.from.as_deref() == Some("__player__") {
            cfg.from = None;
        }
        crate::i18n::set(cfg.lang.as_deref().and_then(crate::i18n::Lang::from_code).unwrap_or_else(crate::i18n::detect));
        let devices = engine::list_devices().unwrap_or_default();

        let migrated = engine::migrate_device_ref(&devices, &mut cfg.to) | engine::migrate_device_ref(&devices, &mut cfg.from);
        let (tray, tray_eq) = match make_tray(ctx) {
            Some((t, e)) => (Some(t), Some(e)),
            None => (None, None),
        };
        let mut app = App {
            viz_mode: match cfg.viz_mode {
                1 => VizMode::Spectrogram,
                2 => VizMode::Scope,
                _ => VizMode::Spectrum,
            },
            cfg,
            devices,
            library: vec![],
            preset_name: None,
            preset: Preset::default(),
            dirty: false,
            running: None,
            message: String::new(),
            player: Player::new(),
            view: View::Motor,
            view_t0: Instant::now(),
            rtab: RTab::Playing,
            sel_band: None,
            solo_target: None,
            solo_latch: false,
            drag_band: None,
            viz: VizState::new(),
            last_frame: Instant::now(),
            meta: TrackMeta::default(),
            cover_id: 0,
            cover_tex: None,
            wordmark_tex: {
                let (px, w, h) = crate::appicon::wordmark_rgba(72);
                (w > 0).then(|| (ctx.load_texture("wordmark", ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px), TextureOptions::LINEAR), w as f32 / h as f32))
            },
            cover_t0: Instant::now(),
            ambient_on: false,
            ambient_side: 0,
            ambient_ptr: None,
            ambient_since: Instant::now(),
            ambient_t: 0.0,
            ambient_last: Instant::now(),
            tint: BLUE,
            tint_target: BLUE,
            toast: None,
            frames: 0,
            _tray: tray,
            tray_eq,
            start_hidden: false,
            quitting: false,
            lib: Library::load(),
            lib_scans: vec![],
            lib_missing: HashSet::new(),
            missing_rx: None,
            lib_added: 0,
            lib_open: None,
            lib_query: String::new(),
            lib_tex: HashMap::new(),
            lib_tint: HashMap::new(),
            titlebar_done: false,
            chain_sent: Vec::new(),
            rate_cache: HashMap::new(),
            recover: None,
            dsd_dialog: None,
            dop_rate_cache: HashMap::new(),
            last_vol: -1.0,
            eq_cache: None,
            stopping: None,
            queued_start: None,
            idle_since: None,
            loud: Arc::new(crate::loudness::Scanner::default()),
            loud_started: false,
            loud_cached: 0,
            loud_cached_t: Instant::now() - std::time::Duration::from_secs(10),
            target_dev: None,
            tip_seen: std::collections::HashSet::new(),
            tip_cands: Vec::new(),
            tip_cur: None,
            tip_last: None,
            tip_cur_since: None,
            tip_eval: Instant::now(),
            rate_checked: false,
            start_rate: None,
            rate_notice: None,
            motor_tab: 0,
            fx_drag: None,
            fx_sel: None,
            fx_rects: Vec::new(),
            fx_zone: Rect::NOTHING,
            fx_eq_right: 0.0,
            frames_run: 0,
            backdrop: BLUE,
            backdrop_a: 0.0,
            tex_budget: 4,
            fetcher: artist::Fetcher::new(),
            lyr_fetcher: lyrics::LyricsFetcher::new(),
            lyrics_open: false,
            queue_open: false,
            queue_map: HashMap::new(),
            queue_sig: 0,
            resume_saved: Instant::now(),
            pending_queue: None,
            undo: None,
            lyr_hold: None,
            proof_id: u64::MAX,
            proof_base: Default::default(),
            proof_view: Default::default(),
            proof_dry_shown: 0,
            proof_alt_seen: 0,
            proof_alt_at: Instant::now() - std::time::Duration::from_secs(10),
            probe_rates: Vec::new(),
            settings_tab: 0,
            preset_q: String::new(),
            preset_q_focus: false,
            proof_cause_at: [Instant::now() - std::time::Duration::from_secs(10); 6],
            proof_cause_mask: 0,
            proof_frames_seen: 0,
            wave_fetcher: wave::WaveFetcher::new(),
            wave_cache: HashMap::new(),
            wave_drag: None,
            lyr_cache: HashMap::new(),
            lyr_failed: HashSet::new(),
            lyr_layout: None,
            lyr_scroll: 0.0,
            lyr_vel: 0.0,
            lyr_pos: 0.0,
            lyr_manual: 0.0,
            lyr_manual_t: Instant::now(),
            lyr_key: String::new(),
            view_epoch: 0,
            nav_t0: Instant::now(),
            nav_sig: (None, None, 0),
            search_t0: Instant::now(),
            vol_open: false,
            vol_t: Instant::now(),
            vol_popup_rect: Rect::NOTHING,
            vol_prev: 0.6,
            upload_open: false,
            upload_sub: false,
            genre_fetcher: artist::GenreFetcher::new(),
            genre_retry: HashMap::new(),
            genre_unsaved: 0,
            artist_cache: HashMap::new(),
            artist_failed: HashSet::new(),
            artist_small: HashMap::new(),
            artist_big: HashMap::new(),
            lib_tab: 0,
            lib_q: String::new(),
            lib_genre: None,
            lib_sort: [0, 0],
            lib_artist: None,
            bio_open: false,
            search_open: false,
            autoeq: crate::autoeq::Client::new(data_dir().join("autoeq_index.md")),
            ae: autoeq_ui::AeUi::new(),
            eq_rule_seen: u64::MAX,
            eq_rule_dev: None,
            eq_rule_dirty: false,
            rule_active: None,
            rule_new: None,
            props: None,
            props_inline: None,
            search_query: String::new(),
            search_sel: 0,
            search_focus: false,
        };
        if migrated {
            app.save_cfg();
        }

        app.restore_route();
        {
            let c = ctx.clone();
            crate::media::start(move || c.request_repaint());
        }
        let _ = ctx;
        match crate::devenv::dev_var("SW_VIEW").as_deref() {
            Ok("settings") => app.view = View::Settings,
            Ok("library") => app.view = View::Library,
            _ => {}
        }
        let queue: Vec<String> = if app.cfg.playlist.is_empty() {
            crate::fsutil::read_json(&queue_path()).unwrap_or_default()
        } else {
            let old = std::mem::take(&mut app.cfg.playlist);
            if crate::devenv::dev_var("SW_OFFSCREEN").is_err() && crate::devenv::dev_var("SW_SHOT").is_err() {
                if let Ok(j) = serde_json::to_vec(&old) {
                    let _ = crate::fsutil::write_atomic(&queue_path(), &j);
                }
            }
            old
        };
        app.player.add(queue.iter().map(PathBuf::from).collect());
        if let Some((i, pos)) = app.cfg.resume {
            if i < queue.len() && crate::devenv::dev_var("SW_OFFSCREEN").is_err() {
                app.player.load_paused(i, pos);
            }
        }
        app.player.set_volume(crate::devenv::dev_var("SW_VOL").ok().and_then(|v| v.parse().ok()).unwrap_or(app.cfg.volume.unwrap_or(0.8)));
        app.player.set_rate(app.cfg.player_rate);
        app.player.repeat.store(app.cfg.repeat, Relaxed);
        app.player.repeat_one.store(app.cfg.repeat_one, Relaxed);
        app.player.shuffle.store(app.cfg.shuffle, Relaxed);
        app.refresh_library();

        app.rescan_library(app.cfg.scan_on_start && crate::devenv::dev_var("SW_OFFSCREEN").is_err());
        if let Some(name) = app.cfg.preset.clone() {
            app.select(&name, false);
        }
        if app.cfg.autostart {
            app.start();
        }
        if let Ok(m) = crate::devenv::dev_var("SW_MODE") {
            app.viz_mode = match m.as_str() {
                "gram" => VizMode::Spectrogram,
                "scope" => VizMode::Scope,
                _ => VizMode::Spectrum,
            };
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
        app
    }

    fn say(&mut self, s: impl Into<String>) {
        self.toast = Some((s.into(), Instant::now()));
    }

    fn set_view(&mut self, v: View) {
        if self.view != v {
            self.view_epoch += 1;
            self.view = v;
            self.view_t0 = Instant::now();
        }
    }

    fn fade(&self, ctx: &egui::Context) -> f32 {
        let t = self.view_t0.elapsed().as_secs_f32() / 0.28;
        if t < 1.0 {
            ctx.request_repaint();
        }
        ease(t)
    }

    fn save_cfg(&self) {
        if crate::devenv::dev_var("SW_OFFSCREEN").is_ok() || crate::devenv::dev_var("SW_SHOT").is_ok() {
            return;
        }
        if let Ok(j) = serde_json::to_vec_pretty(&self.cfg) {
            let _ = crate::fsutil::write_atomic(&config_path(), &j);
        }
    }

    #[cfg(feature = "devtools")]
    fn dev_hooks(&mut self, ctx: &egui::Context, gap_ms: f64, busy: bool) {
        static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        static LAST: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
        static ONCE: std::sync::Mutex<(bool, bool)> = std::sync::Mutex::new((false, false));
        let t0 = START.get_or_init(Instant::now);
        static FRAMES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        FRAMES.fetch_add(1, Relaxed);
        let busy_limit = if busy { 100.0 } else { 400.0 };
        if gap_ms > busy_limit {
            dev_trace(&format!("kare_aralığı {gap_ms:.0} ms (busy={busy})"));
        }
        let mut last = LAST.lock().unwrap();
        if last.map(|t| t.elapsed().as_millis() >= 500).unwrap_or(true) {
            *last = Some(Instant::now());
            dev_trace(&format!(
                "durum kare={} update_ms={} playing={} pos={:.1} vol={:.2} view={} engine={} idx={:?} kanıt={:?} ön_ayar={:?} kural={} viz_rms={:.4} bant={} eq_off={} solo={:?}",
                FRAMES.swap(0, Relaxed),
                UPDATE_US.swap(0, Relaxed) / 1000,
                self.player.playing.load(Relaxed),
                self.player.position(),
                self.player.volume(),
                match self.view {
                    View::Motor => "motor",
                    View::Library => "kütüphane",
                    View::Settings => "ayarlar",
                },
                self.running.is_some(),
                self.player.current(),
                self.proof_view,
                self.preset_name,
                self.rule_active.is_some(),
                self.running.as_ref().map_or(0.0, |r| {
                    let v = r.ctl.viz.lock().unwrap();
                    let n = v.len().min(4096);
                    if n == 0 { 0.0 } else { (v.iter().rev().take(n).map(|x| x * x).sum::<f32>() / n as f32).sqrt() }
                }),
                self.preset.bands.len(),
                engine::EQ_OFF.load(Relaxed),
                self.solo_target
            ));
        }
        if crate::devenv::dev_var("SW_LOGS").is_ok() && last.map(|t| t.elapsed().as_millis() < 5).unwrap_or(false) {
            dev_trace(&format!("günlük {}", devlog::take_top(6)));
        }

        if crate::devenv::dev_var("SW_CAUSES").is_ok() {
            static EV: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let (n, first) = ctx.input(|i| (i.events.len() as u32, i.events.first().map(|e| format!("{e:?}"))));
            EV.fetch_add(n, Relaxed);
            if n > 0 && EV.load(Relaxed) < 40 {
                dev_trace(&format!("olay n={n} ilk={}", first.unwrap_or_default().chars().take(80).collect::<String>()));
            }
        }

        if crate::devenv::dev_var("SW_CAUSES").is_ok() && last.map(|t| t.elapsed().as_millis() < 5).unwrap_or(false) {
            let mut c: Vec<String> = ctx.repaint_causes().iter().map(|c| format!("{c}")).collect();
            c.sort();
            c.dedup();
            dev_trace(&format!("neden {}", c.join(" | ")));
        }
        let el = t0.elapsed().as_secs_f32();
        let mut once = ONCE.lock().unwrap();

        if let Some(h) = crate::devenv::dev_var("SW_HIDE").ok().and_then(|v| v.parse::<f32>().ok()) {
            if !once.0 && el >= h {
                once.0 = true;
                dev_trace("pencere gizlendi");
                hide_window();
            }
        }

        if let Some(q) = crate::devenv::dev_var("SW_EQOFF").ok().and_then(|v| v.parse::<f32>().ok()) {
            if el >= q {
                engine::EQ_OFF.store(true, Relaxed);
            }
        }

        if let Some(q) = crate::devenv::dev_var("SW_AUTOQUIT").ok().and_then(|v| v.parse::<f32>().ok()) {
            if !once.1 && el >= q {
                once.1 = true;
                request_quit(ctx);
            }
        }
    }

}

fn path_row(ui: &mut egui::Ui, title: &str, detail: &str, last: bool) {
    let w = ui.available_width();
    let df = body(11.5);
    let (dl, lh) = {
        let p = ui.painter().clone();
        (wrap_lines(&p, detail, &df, (w - 5.0 - 14.0).max(80.0)), line_height(&p, &df))
    };
    let h = (19.0 + dl.len() as f32 * lh + 8.0).max(36.0);
    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let p = ui.painter();
    let x = rect.left() + 5.0;
    if !last {
        p.line_segment([pos2(x, rect.top() + 14.0), pos2(x, rect.bottom() + 4.0)], Stroke::new(1.0, STROKE));
    }
    p.circle_filled(pos2(x, rect.top() + 9.0), 3.0, SKY);
    p.text_tr(pos2(x + 14.0, rect.top() + 9.0), Align2::LEFT_CENTER, title, bold(12.0), TEXT);
    draw_lines(p, pos2(x + 14.0, rect.top() + 19.0), Align2::LEFT_TOP, &dl, &df, DIM);
}

fn fmt_time(s: f64) -> String {
    let s = s as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        #[cfg(feature = "devtools")]
        let _timer = UpdateTimer(Instant::now());
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

        let hidden = WINDOW_HIDDEN.load(Relaxed) || ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        let busy = self.player.playing.load(Relaxed) || (self.view == View::Motor && self.running.is_some());
        #[allow(unused_mut)]
        let mut every = if hidden {
            500
        } else if busy {
            if self.view == View::Motor { 50 } else { 83 }
        } else {
            400
        };
        #[cfg(feature = "devtools")]
        if let Some(e) = crate::devenv::dev_var("SW_EVERY").ok().and_then(|v| v.parse::<u64>().ok()) {
            every = e;
        }
        if self.ambient_on || self.ambient_t > 0.0 {
            every = every.min(33);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(every));
        let gap_ms = self.last_frame.elapsed().as_secs_f64() * 1000.0;
        let dt = self.last_frame.elapsed().as_secs_f32().clamp(0.001, 0.1);
        self.last_frame = Instant::now();
        #[cfg(feature = "devtools")]
        self.dev_hooks(ctx, gap_ms, busy);
        #[cfg(not(feature = "devtools"))]
        let _ = gap_ms;

        if let Some(r) = &self.running {
            if r.handle.is_finished() {
                let was_dop = r.dop;
                let gone = r.ctl.gone.load(std::sync::atomic::Ordering::SeqCst);
                let was_playing = self.player.playing.load(std::sync::atomic::Ordering::SeqCst);
                if crate::recover::on_unexpected_exit(gone, self.cfg.to.is_some(), self.cfg.use_loopback) == crate::recover::OnExit::Reconnect {
                    self.recover = Some((was_playing, Instant::now() - std::time::Duration::from_secs(5)));
                }
                self.message = r.ctl.status.lock().unwrap().clone();
                if self.message.starts_with("Hata") {
                    let m = self.message.clone();
                    self.say(m);
                }
                self.running = None;
                self.restore_route();
                self.player.playing.store(false, std::sync::atomic::Ordering::SeqCst);
                if self.recover.is_some() {
                    self.message = "Cihaz bağlantısı koptu; geri gelince kendiliğinden devam edecek".into();
                    self.say("Cihaz bağlantısı koptu, geri gelince devam edilecek");
                } else if was_dop && self.message.starts_with("Hata") {

                    if let Some((_, _, id)) = self.target_dev.clone() {
                        self.cfg.dsd_prefs.insert(id, 0);
                        self.save_cfg();
                    }
                    self.player.set_dop_allowed(false);
                    self.player.set_engine_dop(false);
                    self.player.reload_current();
                    self.say("Cihaz DoP'u açamadı; DSD PCM'e çevrilerek çalınacak");
                    self.start();
                    if self.running.is_some() && was_playing {
                        self.player.playing.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                }
            }
        }
        self.tick_stopping();
        self.tick_idle_release();
        self.tick_proof();
        self.tick_eq_rules();
        if let Some(item) = &self.tray_eq {
            let on = !engine::EQ_OFF.load(Relaxed);
            if item.is_checked() != on {
                item.set_checked(on);
            }
        }
        self.tick_dsd_volume_note();
        let note = self.player.note.lock().unwrap().take();
        if let Some(n) = note {
            self.say(n);
        }
        self.tick_recover();
        self.tick_queue_and_resume();
        self.tick_media();
        if self.start_hidden {
            self.start_hidden = false;
            hide_window();
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting && self.cfg.tray_on_close && self._tray.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            hide_window();
        }
        let ctl = self.running.as_ref().map(|r| r.ctl.clone());
        if let Some(c) = &ctl {
            if let Some(n) = c.notice.lock().unwrap().take() {
                self.say(n);
            }
        }
        self.ambient_tick(ctx);
        self.viz.feed(ctl.as_deref(), dt);
        self.tex_budget = 4;
        self.poll_scans();
        self.poll_lyrics();
        for (path, w) in self.wave_fetcher.poll() {
            self.wave_cache.insert(path, Arc::new(w.unwrap_or_default()));
        }
        self.drive_genres();
        if !self.lib_scans.is_empty() {
            ctx.request_repaint();
        }
        if self.view == View::Library {
            let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
            let mut files = vec![];
            for p in dropped {
                if p.is_dir() {
                    self.add_library_folder(p);
                } else {
                    files.push(p);
                }
            }
            self.lib_add_paths(files);
        }
        self.sync_cover(ctx, dt);

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

        ctx.layer_painter(egui::LayerId::background()).rect_filled(ctx.screen_rect(), 0.0, BG0);

        {
            let album_tint = match (&self.view, &self.lib_artist, self.lib_open.clone()) {
                (View::Library, None, Some(k)) if self.lib.albums.iter().any(|a| a.key == k) => Some(self.album_tint(&k)),
                _ => None,
            };
            let (tc, ta) = match album_tint {
                Some(c) => (c, 105.0),
                None if !self.meta.title.is_empty() => {
                    let c = self.tint;
                    let k = (130.0 / (c.r().max(c.g()).max(c.b()) as f32).max(1.0)).min(1.0);
                    (Color32::from_rgb((c.r() as f32 * k) as u8, (c.g() as f32 * k) as u8, (c.b() as f32 * k) as u8), 78.0)
                }
                None => (BLUE, 26.0),
            };
            let dt = ctx.input(|i| i.unstable_dt).clamp(0.001, 0.1);
            let s = (dt * 4.0).min(1.0);
            self.backdrop = lerp_color(self.backdrop, tc, s);
            self.backdrop_a += (ta - self.backdrop_a) * s;
            if (self.backdrop_a - ta).abs() > 0.5 {
                ctx.request_repaint();
            }
        }

        let fade = self.fade(ctx);
        if !self.ambient_on {
            self.header(ctx);
            self.bottom_bar(ctx);
            self.lyrics_sidebar(ctx);
            self.queue_sidebar(ctx);
            match self.view {
                View::Motor => {
                    self.side_panel(ctx, fade);
                    self.motor_view(ctx, fade, &mut ab_held);
                }
                View::Library => self.library_view(ctx, fade),
                View::Settings => self.settings_view(ctx, fade),
            }
        }
        if let Some(r) = &self.running {
            let c = &r.ctl;
            c.bypass.store(ab_held, Relaxed);
            c.level_match.store(!self.cfg.level_match_off, Relaxed);
            c.crossfeed.store(self.cfg.crossfeed.to_bits(), Relaxed);
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

        if !self.ambient_on {
            self.search_overlay(ctx);
            self.autoeq_overlay(ctx);
            self.props_overlay(ctx);
            self.tip_banner(ctx);
            self.dsd_dialog_ui(ctx);
        }
        self.ambient_overlay(ctx);

        if let Ok(path) = crate::devenv::dev_var("SW_SHOT") {
            self.frames += 1;
            if self.frames == 90 {
                if let Some(s) = crate::devenv::dev_var("SW_SEEK").ok().and_then(|v| v.parse::<f64>().ok()) {
                    self.player.seek(s);
                }
            }
            let wait = crate::devenv::dev_var("SW_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(120);
            if self.frames == wait {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
            }
            let shot = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(img) = shot {
                let (w, h) = (img.width() as u32, img.height() as u32);
                let mut raw = Vec::with_capacity((w * h * 4) as usize);
                for p in &img.pixels {
                    raw.extend_from_slice(&[p.r(), p.g(), p.b(), p.a()]);
                }
                if let Ok(f) = std::fs::File::create(&path) {
                    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w, h);
                    enc.set_color(png::ColorType::Rgba);
                    enc.set_depth(png::BitDepth::Eight);
                    if let Ok(mut wr) = enc.write_header() {
                        let _ = wr.write_image_data(&raw);
                    }
                }
                self.stop();
                self.restore_route();
                std::process::exit(0);
            }
        }

        self.undo_toast(ctx);
        if let Some((msg, t0)) = &self.toast {
            let age = t0.elapsed().as_secs_f32();
            if age < 2.8 {
                toast(ctx, msg, age);
            } else {
                self.toast = None;
            }
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        dev_trace("drop_begin");
        self.update_resume();
        self.save_cfg();
        self.stop();
        self.restore_route();
        self.player.shutdown();
        dev_trace("drop_end (temiz kapanış tamam)");
    }
}

struct LyrTok {
    sp: bool,

    bg: bool,
    x: f32,
    y: f32,
    galley: Arc<egui::Galley>,

    word: Option<usize>,
}

struct LyrLine {
    y: f32,
    h: f32,
    toks: Vec<LyrTok>,
}

struct LyrLayout {
    key: String,
    width: u32,
    lines: Vec<LyrLine>,
}

fn waveform_seek(ui: &mut egui::Ui, size: egui::Vec2, peaks: Option<&[f32]>, progress: f32, active: bool) -> (egui::Response, Option<f32>, Option<f32>) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let (bar, gap) = (2.0f32, 1.6f32);
    let step = bar + gap;
    let n = (rect.width() / step).floor().max(1.0) as usize;
    let mid = rect.center().y;
    let half = rect.height() * 0.5 - 1.0;
    let time = ui.input(|i| i.time) as f32;

    let dbg_hover: Option<f32> = crate::devenv::dev_var("SW_WAVEHOVER").ok().and_then(|v| v.parse::<f32>().ok()).map(|f| rect.left() + f * rect.width());
    let hovering = resp.hover_pos().is_some() || resp.dragged() || dbg_hover.is_some();
    let pres = ui.ctx().animate_bool_with_time(resp.id.with("pres"), hovering && (active || dbg_hover.is_some()), 0.2);

    let target_x = resp.hover_pos().or(resp.interact_pointer_pos()).map(|p| p.x).or(dbg_hover).map(|x| x.clamp(rect.left(), rect.right()));
    let sx = ui.ctx().animate_value_with_time(resp.id.with("sx"), target_x.unwrap_or(rect.center().x), if pres > 0.01 && pres < 0.99 { 0.0 } else { 0.07 });
    let hover_x = if pres > 0.003 { Some(sx) } else { None };
    let radius = 70.0f32;
    let have = peaks.map(|p| !p.is_empty()).unwrap_or(false);
    let gray = Color32::from_rgb(58, 69, 94);
    let gray_hi = Color32::from_rgb(92, 106, 138);
    for i in 0..n {
        let f0 = i as f32 / n as f32;
        let f1 = (i + 1) as f32 / n as f32;
        let amp = if have {
            let p = peaks.unwrap();
            let a = ((f0 * p.len() as f32) as usize).min(p.len() - 1);
            let b = ((f1 * p.len() as f32).ceil() as usize).clamp(a + 1, p.len());
            p[a..b].iter().cloned().fold(0.0, f32::max)
        } else if active && peaks.is_none() {

            0.12 + 0.14 * ((time * 3.0 + i as f32 * 0.4).sin() * 0.5 + 0.5)
        } else {
            0.08
        };
        let x = rect.left() + i as f32 * step;
        let played = (i as f32 + 0.5) / n as f32 <= progress;

        let m = match hover_x {
            Some(hx) => {
                let d = ((hx - (x + bar * 0.5)).abs() / radius).min(1.0);
                let c = (d * std::f32::consts::FRAC_PI_2).cos();
                c * c * pres
            }
            None => 0.0,
        };
        let hh = ((amp * half * (1.0 - 0.34 * pres)).max(1.6) * (1.0 + 1.1 * m)).min(half + 1.0);
        let base = if played { BLUE } else { gray };
        let hi = if played { SKY } else { gray_hi };
        let col = lerp_color(base, hi, (m * 1.3).min(1.0));
        if m < 0.03 {
            painter.rect_filled(Rect::from_min_max(pos2(x, mid - hh), pos2(x + bar, mid + hh)), Rounding::same(1.0), col);
        } else {

            let w = bar * (1.0 + 0.9 * m);
            let cx = x + bar * 0.5;
            let r = Rect::from_min_max(pos2(cx - w * 0.5, mid - hh), pos2(cx + w * 0.5, mid + hh));
            let rim = lerp_color(col, Color32::BLACK, 0.45 * m);
            let shine = lerp_color(col, Color32::WHITE, 0.35 * m);
            vgradient(&painter, Rect::from_min_max(r.min, pos2(r.max.x, mid)), rim, shine);
            vgradient(&painter, Rect::from_min_max(pos2(r.min.x, mid), r.max), shine, rim);
            if m > 0.25 {
                let core = Rect::from_center_size(pos2(cx - w * 0.18, mid), vec2((w * 0.3).max(0.8), hh * 1.5));
                painter.rect_filled(core, Rounding::same(0.5), alpha(Color32::WHITE, (70.0 * m) as u8));
            }
        }
    }

    if active && peaks.is_none() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
    }
    if pres > 0.01 {
        ui.ctx().request_repaint();
    }

    if active && have {
        let x = rect.left() + progress.clamp(0.0, 1.0) * rect.width();
        painter.rect_filled(Rect::from_min_max(pos2(x - 0.5, rect.top()), pos2(x + 0.5, rect.bottom())), Rounding::ZERO, alpha(Color32::WHITE, 150));
    }
    let frac_of = |p: egui::Pos2| ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
    let mut preview = None;
    let mut commit = None;
    if active {
        if resp.dragged() {
            if let Some(p) = resp.interact_pointer_pos() {
                preview = Some(frac_of(p));
            }
        }
        if resp.drag_stopped() {
            if let Some(p) = resp.interact_pointer_pos() {
                commit = Some(frac_of(p));
            }
        } else if resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                commit = Some(frac_of(p));
            }
        }
    }
    (resp, preview, commit)
}

fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn smooth_damp(current: f32, target: f32, vel: f32, smooth_time: f32, dt: f32) -> (f32, f32) {
    let st = smooth_time.max(0.0001);
    let omega = 2.0 / st;
    let x = omega * dt;
    let exp = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let change = current - target;
    let temp = (vel + omega * change) * dt;
    let new_vel = (vel - omega * temp) * exp;
    let out = target + (change + temp) * exp;
    (out, new_vel)
}

fn draw_word(painter: &egui::Painter, pos: egui::Pos2, galley: &Arc<egui::Galley>, prog: f32, dim: Color32, bright: Color32) {
    let (w, h) = (galley.size().x, galley.size().y);
    painter.galley_with_override_text_color(pos, galley.clone(), dim);
    if prog <= 0.0 {
        return;
    }
    if prog >= 0.999 {
        painter.galley_with_override_text_color(pos, galley.clone(), bright);
        return;
    }
    let fill = w * prog;
    let edge = (w * 0.5).min(20.0);
    let solid_end = (fill - edge).max(0.0);
    if solid_end > 0.0 {
        let clip = Rect::from_min_max(pos2(pos.x - 2.0, pos.y - 6.0), pos2(pos.x + solid_end, pos.y + h + 6.0));
        painter.with_clip_rect(clip).galley_with_override_text_color(pos, galley.clone(), bright);
    }
    let slices = 10;
    for s in 0..slices {
        let x0 = solid_end + (fill - solid_end) * s as f32 / slices as f32;
        let x1 = solid_end + (fill - solid_end) * (s + 1) as f32 / slices as f32;
        if x1 <= x0 {
            continue;
        }
        let t = 1.0 - (s as f32 + 0.5) / slices as f32;
        let col = lerp_color(dim, bright, smoothstep(t));
        let clip = Rect::from_min_max(pos2(pos.x + x0, pos.y - 6.0), pos2(pos.x + x1, pos.y + h + 6.0));
        painter.with_clip_rect(clip).galley_with_override_text_color(pos, galley.clone(), col);
    }
}
