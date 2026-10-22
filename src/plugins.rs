use rquickjs::{Context, Ctx, Error as JsError, Function, Object, Runtime};
use serde::Deserialize;
use std::collections::{HashMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Pipes::PeekNamedPipe;

pub const API_VERSION: u32 = 2;
const MAX_UI_JSON: usize = 256 * 1024;
const MAX_BUTTONS: usize = 8;
const MAX_PANELS: usize = 4;
const MAX_COMMANDS: usize = 50;
const MAX_NODES: usize = 300;
const MAX_ITEMS: usize = 500;
const MAX_BODY: usize = 16 << 20;
const MAX_DOWNLOAD: u64 = 512 << 20;
const MAX_INFLIGHT: usize = 4;
const MAX_REDIRECTS: usize = 5;
const MAX_KEPT_BODIES: usize = 32;
const AUDIO_EXTENSIONS: [&str; 14] = ["flac", "wav", "mp3", "m4a", "aac", "ogg", "opus", "wma", "aiff", "aif", "alac", "dsf", "dff", "ape"];
const CONTROL_COMMANDS: [&str; 7] = ["play", "pause", "toggle", "next", "prev", "seek", "volume"];
const PRELUDE: &str = include_str!("plugins_prelude.js");
const CALL_LIMIT: Duration = Duration::from_secs(2);
const MEMORY_LIMIT: usize = 64 << 20;
const STORAGE_LIMIT: usize = 1 << 20;
const MAX_PENDING_TOASTS: usize = 20;
const MAX_LOG_LINES: usize = 200;
const LOG_FILE_LIMIT: u64 = 256 * 1024;

pub fn plugins_dir() -> PathBuf {
    app_dir().join("plugins")
}

pub fn data_dir() -> PathBuf {
    app_dir().join("plugin-data")
}

fn app_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Soundworks")
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub main: String,
    pub api: u32,
    pub permissions: Vec<String>,
    pub config: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone, Debug)]
pub struct Discovered {
    pub id: String,
    pub dir: PathBuf,
    pub manifest: Option<Manifest>,
    pub error: Option<String>,
}

pub fn validate(m: &Manifest, dir: &Path) -> Result<(), String> {
    let id_ok = !m.id.is_empty()
        && m.id.chars().next().map_or(false, |c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && m.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.' || c == '_');
    if !id_ok {
        return Err("id must use lowercase letters, digits, '-', '.' or '_'".into());
    }
    if m.name.trim().is_empty() {
        return Err("name is missing".into());
    }
    if m.api == 0 || m.api > API_VERSION {
        return Err(format!("needs plugin API {} but this Soundworks provides {}", m.api, API_VERSION));
    }
    let main = if m.main.is_empty() { "main.js" } else { m.main.as_str() };
    let p = Path::new(main);
    if p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir | std::path::Component::Prefix(_))) {
        return Err("main must be a relative path inside the plugin folder".into());
    }
    if !dir.join(main).is_file() {
        return Err(format!("{main} not found"));
    }
    for perm in &m.permissions {
        let known = perm == "ui"
            || perm == "playback"
            || perm == "download"
            || perm == "library"
            || perm.strip_prefix("network:").map_or(false, |h| !h.is_empty() && !h.contains(['/', ':', ' ', '\\']))
            || perm.strip_prefix("pipe:").map_or(false, |n| !n.is_empty() && !n.contains(['\\', '/']) && !n.contains(".."));
        if !known {
            return Err(format!("unknown permission '{perm}'"));
        }
    }
    Ok(())
}

pub fn discover(root: &Path) -> Vec<Discovered> {
    let mut out = vec![];
    let Ok(rd) = std::fs::read_dir(root) else { return out };
    for e in rd.flatten() {
        let dir = e.path();
        if !dir.is_dir() {
            continue;
        }
        let folder = e.file_name().to_string_lossy().to_string();
        let file = dir.join("soundworks.json");
        if !file.is_file() {
            continue;
        }
        let parsed = std::fs::read(&file)
            .map_err(|e| e.to_string())
            .and_then(|b| serde_json::from_slice::<Manifest>(&b).map_err(|e| e.to_string()));
        match parsed {
            Ok(m) => {
                let err = validate(&m, &dir).err();
                out.push(Discovered { id: if m.id.is_empty() { folder } else { m.id.clone() }, dir, manifest: Some(m), error: err });
            }
            Err(e) => out.push(Discovered { id: folder, dir, manifest: None, error: Some(e) }),
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

pub fn allows_pipe(perms: &[String], name: &str) -> bool {
    if name.is_empty() || name.contains(['\\', '/']) || name.contains("..") {
        return false;
    }
    perms.iter().filter_map(|p| p.strip_prefix("pipe:")).any(|pat| match pat.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => name == pat,
    })
}

enum Msg {
    Net(i32, NetDone),
    Event(String, String),
    Action(String, String, String),
    Stop,
}

struct NetOk {
    status: u16,
    status_text: String,
    headers: Vec<(String, String)>,
    url: String,
    body: Vec<u8>,
    path: Option<PathBuf>,
    size: u64,
}

type NetDone = Result<NetOk, String>;

pub fn is_ip_or_local(host: &str) -> bool {
    let h = host.trim_matches(|c| c == '[' || c == ']').to_lowercase();
    h.parse::<std::net::IpAddr>().is_ok() || h == "localhost" || h.ends_with(".localhost") || h.ends_with(".local") || h.ends_with(".internal") || !h.contains('.')
}

pub fn allows_host(perms: &[String], host: &str) -> bool {
    let host = host.to_lowercase();
    perms.iter().filter_map(|p| p.strip_prefix("network:")).any(|pat| {
        let pat = pat.to_lowercase();
        if pat == "*" {
            !is_ip_or_local(&host)
        } else if let Some(suffix) = pat.strip_prefix("*.") {
            !is_ip_or_local(&host) && host.len() > suffix.len() + 1 && host.ends_with(&format!(".{suffix}"))
        } else {
            host == pat
        }
    })
}

fn safe_file_name(name: &str) -> String {
    let cleaned: String = name.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    let cleaned: String = cleaned.chars().take(150).collect();
    if cleaned.is_empty() {
        "download".to_string()
    } else {
        cleaned
    }
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    for n in 1..1000 {
        let p = dir.join(format!("{stem} ({n}){ext}"));
        if !p.exists() {
            return p;
        }
    }
    dir.join(format!("{stem}-{}{ext}", epoch_ms() as u64))
}

struct NetRequest {
    perms: Vec<String>,
    url: String,
    method: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    timeout: Duration,
    download: Option<(PathBuf, String)>,
}

fn run_request(r: NetRequest) -> NetDone {
    let mut url = url::Url::parse(&r.url).map_err(|e| format!("invalid url: {e}"))?;
    let mut method = r.method.to_uppercase();
    let mut body = r.body;
    let agent = ureq::AgentBuilder::new().redirects(0).timeout(r.timeout).build();
    for _hop in 0..=MAX_REDIRECTS {
        if url.scheme() != "http" && url.scheme() != "https" {
            return Err("only http and https are allowed".into());
        }
        let host = url.host_str().unwrap_or("").to_string();
        if !allows_host(&r.perms, &host) {
            return Err(format!("host '{host}' is not allowed by the manifest permissions"));
        }
        let mut req = agent.request(&method, url.as_str());
        for (k, v) in &r.headers {
            if !k.eq_ignore_ascii_case("host") {
                req = req.set(k, v);
            }
        }
        let res = if body.is_empty() { req.call() } else { req.send_bytes(&body) };
        let resp = match res {
            Ok(resp) => resp,
            Err(ureq::Error::Status(_, resp)) => resp,
            Err(e) => return Err(e.to_string()),
        };
        let status = resp.status();
        if (301..=308).contains(&status) && status != 304 {
            if let Some(loc) = resp.header("location") {
                url = url.join(loc).map_err(|e| format!("bad redirect: {e}"))?;
                if status != 307 && status != 308 {
                    method = "GET".into();
                    body.clear();
                }
                continue;
            }
        }
        let status_text = resp.status_text().to_string();
        let headers: Vec<(String, String)> = resp
            .headers_names()
            .into_iter()
            .filter_map(|n| resp.header(&n).map(|v| (n.to_lowercase(), v.to_string())))
            .collect();
        let final_url = url.to_string();
        let mut reader = resp.into_reader();
        if let Some((dir, name)) = &r.download {
            if !(200..300).contains(&status) {
                return Err(format!("HTTP {status} {status_text}"));
            }
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            let target = unique_path(dir, &safe_file_name(name));
            let part = target.with_extension("part");
            let mut file = File::create(&part).map_err(|e| e.to_string())?;
            let mut size = 0u64;
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let n = reader.read(&mut buf).map_err(|e| {
                    let _ = std::fs::remove_file(&part);
                    e.to_string()
                })?;
                if n == 0 {
                    break;
                }
                size += n as u64;
                if size > MAX_DOWNLOAD {
                    let _ = std::fs::remove_file(&part);
                    return Err("download is too large".into());
                }
                file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            }
            drop(file);
            std::fs::rename(&part, &target).map_err(|e| e.to_string())?;
            return Ok(NetOk { status, status_text, headers, url: final_url, body: vec![], path: Some(target), size });
        }
        let mut data = Vec::new();
        reader.take(MAX_BODY as u64 + 1).read_to_end(&mut data).map_err(|e| e.to_string())?;
        if data.len() > MAX_BODY {
            return Err("response is too large".into());
        }
        let size = data.len() as u64;
        return Ok(NetOk { status, status_text, headers, url: final_url, body: data, path: None, size });
    }
    Err("too many redirects".into())
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct UiButton {
    pub id: String,
    pub icon: String,
    pub tooltip: String,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct UiCommand {
    pub id: String,
    pub title: String,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct UiItem {
    pub id: String,
    pub title: String,
    pub subtitle: String,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum UiNode {
    Text {
        #[serde(default)]
        text: String,
        #[serde(default)]
        style: String,
    },
    Button {
        id: String,
        #[serde(default)]
        label: String,
        #[serde(default)]
        primary: bool,
    },
    List {
        id: String,
        #[serde(default)]
        items: Vec<UiItem>,
    },
    Input {
        id: String,
        #[serde(default)]
        value: String,
        #[serde(default)]
        placeholder: String,
    },
    Switch {
        id: String,
        #[serde(default)]
        label: String,
        #[serde(default)]
        value: bool,
    },
    Divider,
    Space,
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct UiPanel {
    pub id: String,
    pub title: String,
    pub icon: String,
    pub content: Vec<UiNode>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct UiSnapshot {
    pub buttons: Vec<UiButton>,
    pub panels: Vec<UiPanel>,
    pub commands: Vec<UiCommand>,
}

fn clip(s: &mut String, max: usize) {
    if s.chars().count() > max {
        *s = s.chars().take(max).collect();
    }
}

fn sanitize(ui: &mut UiSnapshot) {
    ui.buttons.truncate(MAX_BUTTONS);
    ui.panels.truncate(MAX_PANELS);
    ui.commands.truncate(MAX_COMMANDS);
    for b in &mut ui.buttons {
        clip(&mut b.id, 64);
        clip(&mut b.icon, 32);
        clip(&mut b.tooltip, 120);
    }
    for c in &mut ui.commands {
        clip(&mut c.id, 64);
        clip(&mut c.title, 120);
    }
    for p in &mut ui.panels {
        clip(&mut p.id, 64);
        clip(&mut p.title, 80);
        clip(&mut p.icon, 32);
        p.content.truncate(MAX_NODES);
        for n in &mut p.content {
            match n {
                UiNode::Text { text, style } => {
                    clip(text, 4000);
                    clip(style, 16);
                }
                UiNode::Button { id, label, .. } | UiNode::Switch { id, label, .. } => {
                    clip(id, 64);
                    clip(label, 120);
                }
                UiNode::List { id, items } => {
                    clip(id, 64);
                    items.truncate(MAX_ITEMS);
                    for i in items {
                        clip(&mut i.id, 64);
                        clip(&mut i.title, 200);
                        clip(&mut i.subtitle, 200);
                    }
                }
                UiNode::Input { id, value, placeholder } => {
                    clip(id, 64);
                    clip(value, 1000);
                    clip(placeholder, 120);
                }
                _ => {}
            }
        }
    }
}

#[derive(Default)]
struct Shared {
    toasts: Mutex<Vec<String>>,
    logs: Mutex<VecDeque<String>>,
    error: Mutex<Option<String>>,
    ui: Mutex<UiSnapshot>,
    control: Mutex<Vec<(String, f64)>>,
    library: Mutex<Vec<PathBuf>>,
    file: Option<PathBuf>,
}

fn append_log(path: &Path, level: &str, msg: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(path).map_or(false, |m| m.len() > LOG_FILE_LIMIT) {
        let _ = std::fs::rename(path, path.with_extension("log.old"));
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(f, "{secs} [{level}] {}", msg.replace('\n', " | "));
    }
}

impl Shared {
    fn log(&self, level: &str, msg: &str) {
        let mut l = self.logs.lock().unwrap();
        if l.len() >= MAX_LOG_LINES {
            l.pop_front();
        }
        l.push_back(format!("[{level}] {msg}"));
        if let Some(path) = &self.file {
            append_log(path, level, msg);
        }
        if level == "error" {
            *self.error.lock().unwrap() = Some(msg.lines().next().unwrap_or("").to_string());
        }
    }
}

struct Running {
    tx: Sender<Msg>,
    shared: Arc<Shared>,
}

pub struct PluginHost {
    data_dir: PathBuf,
    downloads_dir: PathBuf,
    running: HashMap<String, Running>,
}

impl PluginHost {
    pub fn new() -> PluginHost {
        let music = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")).join("Music").join("Soundworks");
        PluginHost::with_dirs(data_dir(), music)
    }

    pub fn with_dirs(data_dir: PathBuf, downloads_dir: PathBuf) -> PluginHost {
        PluginHost { data_dir, downloads_dir, running: HashMap::new() }
    }

    pub fn with_data_dir(data_dir: PathBuf) -> PluginHost {
        let downloads = data_dir.join("downloads");
        PluginHost::with_dirs(data_dir, downloads)
    }

    pub fn downloads_dir(&self) -> &Path {
        &self.downloads_dir
    }

    pub fn drain_library(&self) -> Vec<PathBuf> {
        let mut out = vec![];
        for r in self.running.values() {
            out.append(&mut r.shared.library.lock().unwrap());
        }
        out
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.running.contains_key(id)
    }

    pub fn start(&mut self, d: &Discovered) -> Result<(), String> {
        let m = d.manifest.clone().ok_or_else(|| d.error.clone().unwrap_or_default())?;
        if let Some(e) = &d.error {
            return Err(e.clone());
        }
        self.stop(&m.id);
        let main = if m.main.is_empty() { "main.js".to_string() } else { m.main.clone() };
        let script = std::fs::read_to_string(d.dir.join(&main)).map_err(|e| e.to_string())?;
        let mut config = m.config.clone();
        if let Ok(b) = std::fs::read(d.dir.join("config.json")) {
            if let Ok(serde_json::Value::Object(over)) = serde_json::from_slice::<serde_json::Value>(&b) {
                for (k, v) in over {
                    config.insert(k, v);
                }
            }
        }
        let shared = Arc::new(Shared { file: Some(self.data_dir.join(format!("{}.log", m.id))), ..Default::default() });
        let (tx, rx) = channel::<Msg>();
        let job = Job {
            id: m.id.clone(),
            permissions: m.permissions.clone(),
            config: serde_json::Value::Object(config).to_string(),
            script,
            data_dir: self.data_dir.clone(),
            downloads_dir: self.downloads_dir.clone(),
            shared: shared.clone(),
            tx: tx.clone(),
        };
        std::thread::Builder::new()
            .name(format!("plugin-{}", m.id))
            .spawn(move || run(job, rx))
            .map_err(|e| e.to_string())?;
        self.running.insert(m.id.clone(), Running { tx, shared });
        Ok(())
    }

    pub fn stop(&mut self, id: &str) {
        if let Some(r) = self.running.remove(id) {
            let _ = r.tx.send(Msg::Stop);
        }
    }

    pub fn stop_all(&mut self) {
        let ids: Vec<String> = self.running.keys().cloned().collect();
        for id in ids {
            self.stop(&id);
        }
    }

    pub fn broadcast(&self, event: &str, state_json: &str) {
        for r in self.running.values() {
            let _ = r.tx.send(Msg::Event(event.to_string(), state_json.to_string()));
        }
    }

    pub fn ui_snapshots(&self) -> Vec<(String, UiSnapshot)> {
        let mut out: Vec<(String, UiSnapshot)> = self.running.iter().map(|(id, r)| (id.clone(), r.shared.ui.lock().unwrap().clone())).collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    pub fn drain_control(&self) -> Vec<(String, f64)> {
        let mut out = vec![];
        for r in self.running.values() {
            out.append(&mut r.shared.control.lock().unwrap());
        }
        out
    }

    pub fn send_action(&self, plugin: &str, kind: &str, id: &str, payload: &str) {
        if let Some(r) = self.running.get(plugin) {
            let _ = r.tx.send(Msg::Action(kind.to_string(), id.to_string(), payload.to_string()));
        }
    }

    pub fn drain_toasts(&self) -> Vec<String> {
        let mut out = vec![];
        for r in self.running.values() {
            out.append(&mut r.shared.toasts.lock().unwrap());
        }
        out
    }

    pub fn last_error(&self, id: &str) -> Option<String> {
        self.running.get(id).and_then(|r| r.shared.error.lock().unwrap().clone())
    }

    pub fn logs(&self, id: &str) -> Vec<String> {
        self.running.get(id).map(|r| r.shared.logs.lock().unwrap().iter().cloned().collect()).unwrap_or_default()
    }
}

impl Drop for PluginHost {
    fn drop(&mut self) {
        self.stop_all();
    }
}

struct Job {
    id: String,
    permissions: Vec<String>,
    config: String,
    script: String,
    data_dir: PathBuf,
    downloads_dir: PathBuf,
    shared: Arc<Shared>,
    tx: Sender<Msg>,
}

fn epoch_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
}

fn pipe_available(f: &File) -> Option<u32> {
    let mut avail = 0u32;
    unsafe { PeekNamedPipe(HANDLE(f.as_raw_handle()), None, 0, None, Some(&mut avail), None).ok()? };
    Some(avail)
}

fn error_text(ctx: &Ctx, e: JsError) -> String {
    if matches!(e, JsError::Exception) {
        let v = ctx.catch();
        if let Some(ex) = v.as_exception() {
            let msg = ex.message().unwrap_or_default();
            let stack = ex.stack().unwrap_or_default();
            return format!("{msg}\n{stack}");
        }
        return format!("{v:?}");
    }
    e.to_string()
}

struct Store {
    path: PathBuf,
    map: HashMap<String, String>,
}

impl Store {
    fn load(path: PathBuf) -> Store {
        let map = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Store { path, map }
    }

    fn set(&mut self, k: String, v: String) -> bool {
        let before = self.map.insert(k.clone(), v);
        let total: usize = self.map.iter().map(|(k, v)| k.len() + v.len()).sum();
        if total > STORAGE_LIMIT {
            match before {
                Some(old) => self.map.insert(k, old),
                None => self.map.remove(&k),
            };
            return false;
        }
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        serde_json::to_vec(&self.map).ok().map_or(false, |j| std::fs::write(&self.path, j).is_ok())
    }
}

fn put<'js>(o: &Object<'js>, name: &str, f: Function<'js>) -> Result<(), String> {
    o.set(name, f).map_err(|e| e.to_string())
}

fn run(job: Job, rx: std::sync::mpsc::Receiver<Msg>) {
    let shared = job.shared.clone();
    let fail = |msg: String| {
        shared.log("error", &msg);
    };
    let Ok(rt) = Runtime::new() else { return fail("could not start the JavaScript runtime".into()) };
    rt.set_memory_limit(MEMORY_LIMIT);
    rt.set_max_stack_size(512 * 1024);
    let started = Instant::now();
    let deadline = Arc::new(AtomicU64::new(u64::MAX));
    {
        let d = deadline.clone();
        rt.set_interrupt_handler(Some(Box::new(move || started.elapsed().as_millis() as u64 > d.load(Ordering::Relaxed))));
    }
    let Ok(ctx) = Context::full(&rt) else { return fail("could not create the JavaScript context".into()) };

    let pipes: Arc<Mutex<HashMap<i32, File>>> = Arc::new(Mutex::new(HashMap::new()));
    let next_pipe = Arc::new(Mutex::new(1i32));
    let body_store: Arc<Mutex<VecDeque<(i32, Vec<u8>)>>> = Arc::new(Mutex::new(VecDeque::new()));
    let store = Arc::new(Mutex::new(Store::load(job.data_dir.join(format!("{}.json", job.id)))));

    let setup = ctx.with(|ctx| -> Result<(), String> {
        let native = Object::new(ctx.clone()).map_err(|e| e.to_string())?;

        let sh = shared.clone();
        put(&native, "log", Function::new(ctx.clone(), move |level: String, msg: String| sh.log(&level, &msg)).map_err(|e| e.to_string())?)?;

        let sh = shared.clone();
        put(&native, 
            "toast",
            Function::new(ctx.clone(), move |msg: String| {
                let mut t = sh.toasts.lock().unwrap();
                if t.len() < MAX_PENDING_TOASTS {
                    t.push(msg.chars().take(200).collect());
                }
            })
            .map_err(|e| e.to_string())?,
        )?;

        put(&native, "now", Function::new(ctx.clone(), || epoch_ms()).map_err(|e| e.to_string())?)?;
        put(&native, "pid", Function::new(ctx.clone(), || std::process::id() as f64).map_err(|e| e.to_string())?)?;
        put(&native, "version", Function::new(ctx.clone(), || env!("CARGO_PKG_VERSION").to_string()).map_err(|e| e.to_string())?)?;
        let cfg = job.config.clone();
        put(&native, "config", Function::new(ctx.clone(), move || cfg.clone()).map_err(|e| e.to_string())?)?;

        let st = store.clone();
        put(&native, "storage_get", Function::new(ctx.clone(), move |k: String| st.lock().unwrap().map.get(&k).cloned()).map_err(|e| e.to_string())?)?;
        let st = store.clone();
        put(&native, "storage_set", Function::new(ctx.clone(), move |k: String, v: String| st.lock().unwrap().set(k, v)).map_err(|e| e.to_string())?)?;

        let (perms, tbl, counter, sh) = (job.permissions.clone(), pipes.clone(), next_pipe.clone(), shared.clone());
        put(&native, 
            "pipe_open",
            Function::new(ctx.clone(), move |name: String| -> i32 {
                if !allows_pipe(&perms, &name) {
                    sh.log("error", &format!("pipe '{name}' is not allowed by the manifest permissions"));
                    return -1;
                }
                match OpenOptions::new().read(true).write(true).open(format!(r"\\.\pipe\{name}")) {
                    Ok(f) => {
                        let mut c = counter.lock().unwrap();
                        let id = *c;
                        *c += 1;
                        tbl.lock().unwrap().insert(id, f);
                        id
                    }
                    Err(_) => -1,
                }
            })
            .map_err(|e| e.to_string())?,
        )?;

        let tbl = pipes.clone();
        put(&native, 
            "pipe_write",
            Function::new(ctx.clone(), move |id: i32, bytes: Vec<u8>| -> bool {
                let mut t = tbl.lock().unwrap();
                match t.get_mut(&id) {
                    Some(f) => {
                        let ok = f.write_all(&bytes).and_then(|_| f.flush()).is_ok();
                        if !ok {
                            t.remove(&id);
                        }
                        ok
                    }
                    None => false,
                }
            })
            .map_err(|e| e.to_string())?,
        )?;

        let tbl = pipes.clone();
        put(&native, 
            "pipe_read",
            Function::new(ctx.clone(), move |id: i32| -> Option<Vec<u8>> {
                let mut t = tbl.lock().unwrap();
                let f = t.get_mut(&id)?;
                let avail = match pipe_available(f) {
                    Some(a) => a as usize,
                    None => {
                        t.remove(&id);
                        return None;
                    }
                };
                let mut buf = vec![0u8; avail.min(1 << 16)];
                if buf.is_empty() {
                    return Some(buf);
                }
                match f.read(&mut buf) {
                    Ok(n) => {
                        buf.truncate(n);
                        Some(buf)
                    }
                    Err(_) => {
                        t.remove(&id);
                        None
                    }
                }
            })
            .map_err(|e| e.to_string())?,
        )?;

        let (perms, sh) = (job.permissions.clone(), shared.clone());
        put(
            &native,
            "ui_publish",
            Function::new(ctx.clone(), move |json: String| -> bool {
                if !perms.iter().any(|p| p == "ui") {
                    sh.log("error", "sw.ui and sw.commands need the 'ui' permission");
                    return false;
                }
                if json.len() > MAX_UI_JSON {
                    sh.log("error", "interface description is too large");
                    return false;
                }
                match serde_json::from_str::<UiSnapshot>(&json) {
                    Ok(mut ui) => {
                        sanitize(&mut ui);
                        *sh.ui.lock().unwrap() = ui;
                        true
                    }
                    Err(e) => {
                        sh.log("error", &format!("invalid interface description: {e}"));
                        false
                    }
                }
            })
            .map_err(|e| e.to_string())?,
        )?;

        let (perms, sh) = (job.permissions.clone(), shared.clone());
        put(
            &native,
            "player_cmd",
            Function::new(ctx.clone(), move |name: String, arg: f64| -> bool {
                if !perms.iter().any(|p| p == "playback") {
                    sh.log("error", "controlling playback needs the 'playback' permission");
                    return false;
                }
                if !CONTROL_COMMANDS.contains(&name.as_str()) {
                    return false;
                }
                let mut q = sh.control.lock().unwrap();
                if q.len() < MAX_PENDING_TOASTS {
                    q.push((name, arg));
                }
                true
            })
            .map_err(|e| e.to_string())?,
        )?;

        let (perms, sh, tx, inflight) = (job.permissions.clone(), shared.clone(), job.tx.clone(), Arc::new(AtomicUsize::new(0)));
        let downloads = job.downloads_dir.clone();
        put(
            &native,
            "net_start",
            Function::new(ctx.clone(), move |id: i32, kind: String, url: String, opts_json: String, body: Vec<u8>| -> bool {
                let opts: serde_json::Value = serde_json::from_str(&opts_json).unwrap_or_default();
                let method = opts["method"].as_str().unwrap_or("GET").to_string();
                let timeout_ms = opts["timeout"].as_f64().unwrap_or(0.0);
                let filename = opts["filename"].as_str().unwrap_or("").to_string();
                let headers_json = opts["headers"].to_string();
                let need = if kind == "download" { "download" } else { "" };
                if !need.is_empty() && !perms.iter().any(|p| p == need) {
                    sh.log("error", "sw.download needs the 'download' permission");
                    return false;
                }
                if !perms.iter().any(|p| p.starts_with("network:")) {
                    sh.log("error", "network access needs a 'network:<host>' permission");
                    return false;
                }
                if inflight.load(Ordering::Relaxed) >= MAX_INFLIGHT {
                    sh.log("error", "too many requests in flight");
                    return false;
                }
                let headers: Vec<(String, String)> = serde_json::from_str::<HashMap<String, String>>(&headers_json).unwrap_or_default().into_iter().collect();
                let default_timeout = if kind == "download" { 600.0 } else { 30.0 };
                let secs = if timeout_ms > 0.0 { timeout_ms / 1000.0 } else { default_timeout };
                let req = NetRequest {
                    perms: perms.clone(),
                    url,
                    method: if method.is_empty() { "GET".into() } else { method },
                    headers,
                    body,
                    timeout: Duration::from_secs_f64(secs.clamp(1.0, 900.0)),
                    download: (kind == "download").then(|| (downloads.clone(), filename)),
                };
                inflight.fetch_add(1, Ordering::Relaxed);
                let (tx, counter) = (tx.clone(), inflight.clone());
                std::thread::spawn(move || {
                    let r = run_request(req);
                    counter.fetch_sub(1, Ordering::Relaxed);
                    let _ = tx.send(Msg::Net(id, r));
                });
                true
            })
            .map_err(|e| e.to_string())?,
        )?;

        let bodies = body_store.clone();
        put(
            &native,
            "net_text",
            Function::new(ctx.clone(), move |id: i32| -> String {
                bodies.lock().unwrap().iter().find(|(i, _)| *i == id).map(|(_, b)| String::from_utf8_lossy(b).to_string()).unwrap_or_default()
            })
            .map_err(|e| e.to_string())?,
        )?;
        let bodies = body_store.clone();
        put(
            &native,
            "net_bytes",
            Function::new(ctx.clone(), move |id: i32| -> Vec<u8> { bodies.lock().unwrap().iter().find(|(i, _)| *i == id).map(|(_, b)| b.clone()).unwrap_or_default() }).map_err(|e| e.to_string())?,
        )?;

        let (perms, sh, dl) = (job.permissions.clone(), shared.clone(), job.downloads_dir.clone());
        put(
            &native,
            "library_add",
            Function::new(ctx.clone(), move |json: String| -> bool {
                if !perms.iter().any(|p| p == "library") {
                    sh.log("error", "sw.library.add needs the 'library' permission");
                    return false;
                }
                let Ok(paths) = serde_json::from_str::<Vec<String>>(&json) else { return false };
                let root = std::fs::canonicalize(&dl).unwrap_or(dl.clone());
                let mut ok = false;
                for p in paths.into_iter().take(500) {
                    let Ok(full) = std::fs::canonicalize(&p) else { continue };
                    let audio = full.extension().and_then(|e| e.to_str()).map_or(false, |e| AUDIO_EXTENSIONS.contains(&e.to_lowercase().as_str()));
                    if full.starts_with(&root) && full.is_file() && audio {
                        sh.library.lock().unwrap().push(full);
                        ok = true;
                    } else {
                        sh.log("error", &format!("library.add only accepts audio files inside the downloads folder: {p}"));
                    }
                }
                ok
            })
            .map_err(|e| e.to_string())?,
        )?;

        let tbl = pipes.clone();
        put(&native, "pipe_close", Function::new(ctx.clone(), move |id: i32| { tbl.lock().unwrap().remove(&id); }).map_err(|e| e.to_string())?)?;

        ctx.globals().set("__native", native).map_err(|e| e.to_string())?;
        ctx.eval::<(), _>(PRELUDE).map_err(|e| error_text(&ctx, e))?;
        Ok(())
    });
    if let Err(e) = setup {
        return fail(e);
    }

    {
        let sh = shared.clone();
        rt.set_host_promise_rejection_tracker(Some(Box::new(move |ctx, _promise, reason, handled| {
            if !handled {
                let text = reason.as_exception().map(|e| e.message().unwrap_or_default()).unwrap_or_else(|| format!("{reason:?}"));
                let _ = ctx;
                sh.log("error", &format!("unhandled promise rejection: {text}"));
            }
        })));
    }

    let guarded = |f: &dyn Fn(&Ctx) -> Result<(), String>| {
        deadline.store(started.elapsed().as_millis() as u64 + CALL_LIMIT.as_millis() as u64, Ordering::Relaxed);
        let r = ctx.with(|c| f(&c));
        while rt.execute_pending_job().unwrap_or(false) {}
        deadline.store(u64::MAX, Ordering::Relaxed);
        if let Err(e) = r {
            shared.log("error", &e);
        }
    };

    guarded(&|c| c.eval::<(), _>(job.script.as_str()).map_err(|e| error_text(c, e)));

    let call = |c: &Ctx, name: &str, args: Vec<String>| -> Result<(), String> {
        let f: Function = c.globals().get(name).map_err(|e| error_text(c, e))?;
        match args.len() {
            0 => f.call::<_, ()>(()),
            2 => f.call::<_, ()>((args[0].clone(), args[1].clone())),
            _ => f.call::<_, ()>((args[0].clone(), args[1].clone(), args[2].clone())),
        }
        .map_err(|e| error_text(c, e))
    };

    loop {
        match rx.recv_timeout(Duration::from_millis(40)) {
            Ok(Msg::Net(id, result)) => {
                let meta = match result {
                    Ok(ok) => {
                        let mut b = body_store.lock().unwrap();
                        b.push_back((id, ok.body));
                        while b.len() > MAX_KEPT_BODIES {
                            b.pop_front();
                        }
                        serde_json::json!({
                            "ok": true,
                            "status": ok.status,
                            "statusText": ok.status_text,
                            "headers": ok.headers.into_iter().collect::<HashMap<String, String>>(),
                            "url": ok.url,
                            "path": ok.path.map(|p| p.to_string_lossy().to_string()),
                            "size": ok.size,
                        })
                    }
                    Err(e) => serde_json::json!({ "ok": false, "error": e }),
                };
                let text = meta.to_string();
                guarded(&|c| call(c, "__net_done", vec![id.to_string(), text.clone()]));
            }
            Ok(Msg::Event(name, json)) => guarded(&|c| call(c, "__dispatch", vec![name.clone(), json.clone()])),
            Ok(Msg::Action(kind, id, payload)) => guarded(&|c| call(c, "__action", vec![kind.clone(), id.clone(), payload.clone()])),
            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => {
                guarded(&|c| call(c, "__dispatch", vec!["stop".into(), "{}".into()]));
                break;
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        guarded(&|c| call(c, "__tick", vec![]));
    }
    pipes.lock().unwrap().clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sw_plugin_test_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn make_plugin(root: &Path, id: &str, manifest_extra: &str, script: &str) -> Discovered {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("soundworks.json"), format!(r#"{{"id":"{id}","name":"T","version":"1","api":1{manifest_extra}}}"#)).unwrap();
        std::fs::write(dir.join("main.js"), script).unwrap();
        discover(root).into_iter().find(|d| d.id == id).unwrap()
    }

    fn wait_for<T>(mut f: impl FnMut() -> Option<T>) -> Option<T> {
        for _ in 0..150 {
            if let Some(v) = f() {
                return Some(v);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }

    fn first_toast(host: &PluginHost) -> Option<String> {
        wait_for(|| host.drain_toasts().into_iter().next())
    }

    #[test]
    fn manifest_validation_rejects_bad_input() {
        let root = temp_dir("validate");
        let good = make_plugin(&root, "good-one", "", "");
        assert!(good.error.is_none());
        let dir = root.join("bad");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("main.js"), "").unwrap();
        for (manifest, needle) in [
            (r#"{"id":"Bad Id","name":"x","api":1}"#, "id must"),
            (r#"{"id":"x","name":"","api":1}"#, "name"),
            (r#"{"id":"x","name":"x","api":9}"#, "API"),
            (r#"{"id":"x","name":"x","api":1,"main":"../evil.js"}"#, "relative"),
            (r#"{"id":"x","name":"x","api":1,"main":"nope.js"}"#, "not found"),
            (r#"{"id":"x","name":"x","api":1,"permissions":["fs:all"]}"#, "unknown permission"),
        ] {
            std::fs::write(dir.join("soundworks.json"), manifest).unwrap();
            let found = discover(&root).into_iter().find(|d| d.dir == dir).unwrap();
            let err = found.error.unwrap_or_default();
            assert!(err.contains(needle), "{manifest} -> {err}");
        }
        std::fs::write(dir.join("soundworks.json"), "not json").unwrap();
        assert!(discover(&root).into_iter().find(|d| d.dir == dir).unwrap().manifest.is_none());
    }

    #[test]
    fn pipe_permissions_match_by_prefix_or_exact_name() {
        let p = vec!["pipe:discord-ipc-*".to_string(), "pipe:exact".to_string()];
        assert!(allows_pipe(&p, "discord-ipc-0"));
        assert!(allows_pipe(&p, "exact"));
        assert!(!allows_pipe(&p, "exact2"));
        assert!(!allows_pipe(&p, "other"));
        assert!(!allows_pipe(&p, r"discord-ipc-0\..\x"));
        assert!(!allows_pipe(&p, ""));
        assert!(!allows_pipe(&[], "discord-ipc-0"));
    }

    #[test]
    fn events_reach_the_plugin_with_player_state() {
        let root = temp_dir("events");
        let d = make_plugin(&root, "ev", "", r#"sw.on("trackchange", s => sw.toast(s.title + " / " + s.artist + " / " + s.playing));"#);
        let mut host = PluginHost::with_data_dir(root.join("data"));
        host.start(&d).unwrap();
        host.broadcast("trackchange", r#"{"state":{"title":"Song","artist":"Band","playing":true}}"#);
        assert_eq!(first_toast(&host).as_deref(), Some("Song / Band / true"));
    }

    #[test]
    fn timers_fire_and_clear() {
        let root = temp_dir("timers");
        let d = make_plugin(
            &root,
            "tm",
            "",
            r#"var n = 0; var id = setInterval(function(){ n++; if (n == 3) { sw.toast("three"); clearInterval(id); } }, 20);
               setTimeout(function(){ sw.toast("once"); }, 30);"#,
        );
        let mut host = PluginHost::with_data_dir(root.join("data"));
        host.start(&d).unwrap();
        let mut seen = vec![];
        wait_for(|| {
            seen.extend(host.drain_toasts());
            (seen.len() >= 2).then_some(())
        })
        .unwrap();
        assert!(seen.contains(&"once".to_string()) && seen.contains(&"three".to_string()));
    }

    #[test]
    fn runaway_plugin_is_interrupted_and_stays_isolated() {
        let root = temp_dir("loop");
        let bad = make_plugin(&root, "bad", "", r#"sw.on("play", function(){ while(true){} });"#);
        let good = make_plugin(&root, "good", "", r#"sw.on("pause", function(){ sw.toast("alive"); });"#);
        let mut host = PluginHost::with_data_dir(root.join("data"));
        host.start(&bad).unwrap();
        host.start(&good).unwrap();
        host.broadcast("play", r#"{"state":{}}"#);
        host.broadcast("pause", r#"{"state":{}}"#);
        assert_eq!(first_toast(&host).as_deref(), Some("alive"));
        let err = wait_for(|| host.last_error("bad")).expect("interrupt should be reported");
        assert!(err.to_lowercase().contains("interrupt"), "{err}");
        host.broadcast("pause", r#"{"state":{}}"#);
        assert!(host.is_running("bad"));
    }

    #[test]
    fn script_errors_are_reported_not_fatal() {
        let root = temp_dir("err");
        let d = make_plugin(&root, "er", "", r#"sw.on("play", function(){ throw new Error("boom"); }); sw.on("pause", function(){ sw.toast("ok"); });"#);
        let mut host = PluginHost::with_data_dir(root.join("data"));
        host.start(&d).unwrap();
        host.broadcast("play", r#"{"state":{}}"#);
        host.broadcast("pause", r#"{"state":{}}"#);
        assert_eq!(first_toast(&host).as_deref(), Some("ok"));
        assert!(host.last_error("er").unwrap().contains("boom"));
    }

    #[test]
    fn disallowed_pipe_is_denied() {
        let root = temp_dir("pipe");
        let d = make_plugin(&root, "pp", "", r#"var id = sw.pipe.open("not-allowed"); sw.toast(String(id));"#);
        let mut host = PluginHost::with_data_dir(root.join("data"));
        host.start(&d).unwrap();
        assert_eq!(first_toast(&host).as_deref(), Some("null"));
        assert!(host.logs("pp").iter().any(|l| l.contains("not allowed")));
    }

    #[test]
    fn storage_and_config_persist() {
        let root = temp_dir("store");
        let d = make_plugin(
            &root,
            "st",
            r#","config":{"greeting":"hi"}"#,
            r#"var n = sw.storage.get("runs", 0) + 1; sw.storage.set("runs", n); sw.toast(sw.config.greeting + n);"#,
        );
        std::fs::write(d.dir.join("config.json"), r#"{"greeting":"yo"}"#).unwrap();
        let data = root.join("data");
        let mut host = PluginHost::with_data_dir(data.clone());
        host.start(&d).unwrap();
        assert_eq!(first_toast(&host).as_deref(), Some("yo1"));
        host.stop("st");
        host.start(&d).unwrap();
        assert_eq!(first_toast(&host).as_deref(), Some("yo2"));
    }

    #[test]
    fn utf8_helpers_roundtrip_including_astral_characters() {
        let root = temp_dir("utf8");
        let d = make_plugin(
            &root,
            "u8",
            "",
            r#"var s = "şarkı ♪ 🎵 ok"; var b = sw.utf8.encode(s); sw.toast(sw.utf8.decode(b) === s ? "same:" + b.length : "diff");"#,
        );
        let mut host = PluginHost::with_data_dir(root.join("data"));
        host.start(&d).unwrap();
        let expected = "şarkı ♪ 🎵 ok".len();
        assert_eq!(first_toast(&host), Some(format!("same:{expected}")));
    }

    #[test]
    fn player_state_position_advances_while_playing() {
        let root = temp_dir("pos");
        let d = make_plugin(&root, "ps", "", r#"sw.on("play", function(s){ setTimeout(function(){ sw.toast(String(sw.player.getState().position >= 10.05)); }, 120); });"#);
        let mut host = PluginHost::with_data_dir(root.join("data"));
        host.start(&d).unwrap();
        host.broadcast("play", r#"{"state":{"position":10,"duration":100,"playing":true}}"#);
        assert_eq!(first_toast(&host).as_deref(), Some("true"));
    }

    #[test]
    fn restarting_replaces_the_running_plugin() {
        let root = temp_dir("restart");
        let d = make_plugin(&root, "rs", "", r#"sw.toast("start");"#);
        let mut host = PluginHost::with_data_dir(root.join("data"));
        host.start(&d).unwrap();
        assert_eq!(first_toast(&host).as_deref(), Some("start"));
        host.start(&d).unwrap();
        assert_eq!(first_toast(&host).as_deref(), Some("start"));
        host.stop_all();
        assert!(!host.is_running("rs"));
    }

    struct MockDiscord {
        frames: Arc<Mutex<Vec<(u32, String)>>>,
    }

    fn read_frame(f: &mut File) -> Option<(u32, String)> {
        let mut head = [0u8; 8];
        f.read_exact(&mut head).ok()?;
        let op = u32::from_le_bytes(head[0..4].try_into().unwrap());
        let len = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
        let mut body = vec![0u8; len];
        f.read_exact(&mut body).ok()?;
        Some((op, String::from_utf8(body).ok()?))
    }

    fn start_mock_discord(pipe_name: &str) -> MockDiscord {
        use std::os::windows::io::FromRawHandle;
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
        use windows::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT};
        let wide: Vec<u16> = format!(r"\\.\pipe\{pipe_name}").encode_utf16().chain(Some(0)).collect();
        let handle = unsafe { CreateNamedPipeW(PCWSTR(wide.as_ptr()), PIPE_ACCESS_DUPLEX, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT, 1, 65536, 65536, 0, None) };
        let frames: Arc<Mutex<Vec<(u32, String)>>> = Arc::new(Mutex::new(vec![]));
        let sink = frames.clone();
        let raw = handle.0 as usize;
        std::thread::spawn(move || {
            let handle = HANDLE(raw as *mut std::ffi::c_void);
            let _ = unsafe { ConnectNamedPipe(handle, None) };
            let mut f = unsafe { File::from_raw_handle(handle.0) };
            let Some(hello) = read_frame(&mut f) else { return };
            sink.lock().unwrap().push(hello);
            let ready = br#"{"cmd":"DISPATCH","evt":"READY","data":{}}"#;
            let mut out = 1u32.to_le_bytes().to_vec();
            out.extend((ready.len() as u32).to_le_bytes());
            out.extend(ready);
            if f.write_all(&out).is_err() {
                return;
            }
            while let Some(fr) = read_frame(&mut f) {
                sink.lock().unwrap().push(fr);
            }
        });
        MockDiscord { frames }
    }

    fn wait_frame(m: &MockDiscord, from: usize, pred: impl Fn(&serde_json::Value) -> bool) -> Option<serde_json::Value> {
        wait_for(|| {
            let frames = m.frames.lock().unwrap();
            frames.iter().skip(from).filter_map(|(_, t)| serde_json::from_str::<serde_json::Value>(t).ok()).find(|v| pred(v))
        })
    }

    #[test]
    fn discord_example_talks_the_ipc_protocol() {
        let root = temp_dir("discord");
        let dir = root.join("discord-rich-presence");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("soundworks.json"), include_str!("../examples/discord-rich-presence/soundworks.json")).unwrap();
        std::fs::write(dir.join("main.js"), include_str!("../examples/discord-rich-presence/main.js")).unwrap();
        let base = format!("discord-ipc-t{}-", std::process::id());
        std::fs::write(dir.join("config.json"), format!(r#"{{"clientId":"123456789","pipeBase":"{base}"}}"#)).unwrap();
        let mock = start_mock_discord(&format!("{base}0"));
        let d = discover(&root).into_iter().next().unwrap();
        assert!(d.error.is_none(), "{:?}", d.error);
        let mut host = PluginHost::with_data_dir(root.join("data"));
        host.start(&d).unwrap();

        let hello = wait_frame(&mock, 0, |v| v.get("client_id").is_some()).expect("handshake");
        assert_eq!(hello["v"], 1);
        assert_eq!(hello["client_id"], "123456789");

        host.broadcast("trackchange", r#"{"state":{"title":"Şarkı 🎵","artist":"Sanatçı","album":"Albüm","duration":200,"position":10,"playing":true}}"#);
        let set = wait_frame(&mock, 0, |v| v["cmd"] == "SET_ACTIVITY" && !v["args"]["activity"].is_null()).expect("activity");
        let act = &set["args"]["activity"];
        assert_eq!(set["args"]["pid"], std::process::id());
        assert_eq!(act["type"], 2);
        assert_eq!(act["details"], "Şarkı 🎵");
        assert_eq!(act["state"], "Sanatçı");
        assert_eq!(act["assets"]["large_text"], "Albüm");
        let start = act["timestamps"]["start"].as_i64().unwrap();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
        assert!((now - 10 - start).abs() <= 3, "start {start} now {now}");
        assert_eq!(act["timestamps"]["end"].as_i64().unwrap(), start + 200);

        let seen = mock.frames.lock().unwrap().len();
        host.broadcast("pause", r#"{"state":{"playing":false,"position":12}}"#);
        wait_frame(&mock, seen, |v| v["cmd"] == "SET_ACTIVITY" && v["args"]["activity"].is_null()).expect("cleared on pause");

        let seen = mock.frames.lock().unwrap().len();
        host.broadcast("play", r#"{"state":{"playing":true,"position":12}}"#);
        wait_frame(&mock, seen, |v| v["cmd"] == "SET_ACTIVITY" && !v["args"]["activity"].is_null()).expect("activity again");

        let seen = mock.frames.lock().unwrap().len();
        host.stop("discord-rich-presence");
        wait_frame(&mock, seen, |v| v["cmd"] == "SET_ACTIVITY" && v["args"]["activity"].is_null()).expect("cleared on stop");
    }

    #[test]
    fn interface_calls_need_the_ui_permission() {
        let root = temp_dir("noui");
        let d = make_plugin(&root, "nu", "", r#"sw.ui.addButton({id:"b", icon:"plus", onClick:function(){}}); sw.toast("done");"#);
        let mut host = PluginHost::with_data_dir(root.join("data"));
        host.start(&d).unwrap();
        assert_eq!(first_toast(&host).as_deref(), Some("done"));
        wait_for(|| host.logs("nu").iter().any(|l| l.contains("'ui' permission")).then_some(())).expect("permission error logged");
        assert!(host.ui_snapshots()[0].1.buttons.is_empty());
    }

    #[test]
    fn buttons_panels_and_commands_round_trip() {
        let root = temp_dir("ui");
        let d = make_plugin(
            &root,
            "ui",
            r#","permissions":["ui"]"#,
            r#"
            sw.ui.addButton({ id: "go", icon: "lyrics", tooltip: "Go", onClick: function () { sw.toast("button"); } });
            sw.ui.addPanel({
              id: "main", title: "Main", icon: "list",
              content: [
                { type: "text", text: "Hello", style: "title" },
                { type: "button", id: "b1", label: "Press", primary: true },
                { type: "list", id: "l1", items: [{ id: "i1", title: "One", subtitle: "first" }] },
                { type: "input", id: "in", value: "x", placeholder: "type" },
                { type: "switch", id: "sw", label: "On", value: true },
                { type: "divider" },
                { type: "from-the-future", foo: 1 }
              ],
              onAction: function (a) { sw.toast("action:" + a.type + ":" + a.id + ":" + a.value); },
              onOpen: function () { sw.toast("opened"); },
            });
            sw.commands.register({ id: "cmd", title: "Do it", run: function () { sw.toast("ran"); } });
            "#,
        );
        let mut host = PluginHost::with_data_dir(root.join("data"));
        host.start(&d).unwrap();
        let snap = wait_for(|| {
            let s = host.ui_snapshots().into_iter().next().unwrap().1;
            (!s.panels.is_empty() && !s.commands.is_empty() && !s.buttons.is_empty()).then_some(s)
        })
        .expect("snapshot published");
        assert_eq!(snap.buttons[0].icon, "lyrics");
        assert_eq!(snap.panels[0].content.len(), 7);
        assert!(matches!(snap.panels[0].content[1], UiNode::Button { primary: true, .. }));
        assert!(matches!(snap.panels[0].content[6], UiNode::Unknown));
        assert_eq!(snap.commands[0].title, "Do it");

        host.send_action("ui", "button", "go", "");
        assert_eq!(first_toast(&host).as_deref(), Some("button"));
        host.send_action("ui", "command", "cmd", "");
        assert_eq!(first_toast(&host).as_deref(), Some("ran"));
        host.send_action("ui", "open", "main", "");
        assert_eq!(first_toast(&host).as_deref(), Some("opened"));
        host.send_action("ui", "panel", "main", r#"{"type":"input","id":"in","value":"hi"}"#);
        assert_eq!(first_toast(&host).as_deref(), Some("action:input:in:hi"));
    }

    #[test]
    fn panel_content_can_be_updated_and_is_limited() {
        let root = temp_dir("uilimit");
        let d = make_plugin(
            &root,
            "ul",
            r#","permissions":["ui"]"#,
            r#"
            sw.ui.addPanel({ id: "p", title: "P", content: [] });
            var nodes = [];
            for (var i = 0; i < 1000; i++) nodes.push({ type: "text", text: "line " + i });
            sw.ui.setContent("p", nodes);
            for (var i = 0; i < 20; i++) sw.ui.addButton({ id: "b" + i, icon: "plus" });
            "#,
        );
        let mut host = PluginHost::with_data_dir(root.join("data"));
        host.start(&d).unwrap();
        let snap = wait_for(|| {
            let s = host.ui_snapshots().into_iter().next().unwrap().1;
            (s.panels.first().map_or(0, |p| p.content.len()) > 0).then_some(s)
        })
        .unwrap();
        assert_eq!(snap.panels[0].content.len(), MAX_NODES);
        assert_eq!(snap.buttons.len(), MAX_BUTTONS);
    }

    #[test]
    fn playback_control_needs_its_permission_and_known_commands() {
        let root = temp_dir("ctl");
        let denied = make_plugin(&root, "denied", "", r#"sw.player.pause(); sw.toast("tried");"#);
        let allowed = make_plugin(
            &root,
            "allowed",
            r#","permissions":["playback"]"#,
            r#"sw.player.pause(); sw.player.seek(42); sw.player.setVolume(5); sw.toast("sent");"#,
        );
        let mut host = PluginHost::with_data_dir(root.join("data"));
        host.start(&denied).unwrap();
        assert_eq!(first_toast(&host).as_deref(), Some("tried"));
        assert!(host.drain_control().is_empty());
        host.stop_all();
        host.start(&allowed).unwrap();
        assert_eq!(first_toast(&host).as_deref(), Some("sent"));
        let cmds = host.drain_control();
        assert_eq!(cmds, vec![("pause".to_string(), 0.0), ("seek".to_string(), 42.0), ("volume".to_string(), 1.0)]);
    }

    #[test]
    fn sleep_timer_example_fades_pauses_and_restores_volume() {
        let root = temp_dir("sleep");
        let dir = root.join("sleep-timer");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("soundworks.json"), include_str!("../examples/sleep-timer/soundworks.json")).unwrap();
        std::fs::write(dir.join("main.js"), include_str!("../examples/sleep-timer/main.js")).unwrap();
        let d = discover(&root).into_iter().next().unwrap();
        assert!(d.error.is_none(), "{:?}", d.error);
        let mut host = PluginHost::with_data_dir(root.join("data"));
        host.start(&d).unwrap();
        host.broadcast("play", r#"{"state":{"playing":true,"volume":0.8,"title":"x"}}"#);
        let snap = wait_for(|| {
            let s = host.ui_snapshots().into_iter().next().unwrap().1;
            (!s.panels.is_empty() && !s.commands.is_empty()).then_some(s)
        })
        .expect("ui published");
        assert_eq!(snap.commands.len(), 4);
        host.send_action("sleep-timer", "panel", "sleep", r#"{"type":"click","id":"start:0.03"}"#);
        let mut sent: Vec<(String, f64)> = vec![];
        wait_for(|| {
            sent.extend(host.drain_control());
            sent.iter().any(|(c, _)| c == "pause").then_some(())
        })
        .expect("pauses when the time is up");
        std::thread::sleep(Duration::from_millis(300));
        sent.extend(host.drain_control());
        let vols: Vec<f64> = sent.iter().filter(|(c, _)| c == "volume").map(|(_, v)| *v).collect();
        assert!(vols.len() >= 2, "{sent:?}");
        assert!(vols[0] <= 0.8 + 1e-9, "fade starts at or below the start volume: {vols:?}");
        assert!((vols.last().unwrap() - 0.8).abs() < 1e-9, "volume restored: {vols:?}");
        let toasts = host.drain_toasts();
        assert!(toasts.is_empty() || toasts.iter().any(|t| t.contains("Sleep timer")));
    }

    fn serve_http() -> u16 {
        use std::io::{BufRead, BufReader};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut stream = stream;
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() {
                        return;
                    }
                    let mut parts = line.split_whitespace();
                    let (method, path) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string());
                    let mut len = 0usize;
                    let mut token = String::new();
                    loop {
                        let mut h = String::new();
                        if reader.read_line(&mut h).is_err() || h.trim().is_empty() {
                            break;
                        }
                        let lower = h.to_lowercase();
                        if let Some(v) = lower.strip_prefix("content-length:") {
                            len = v.trim().parse().unwrap_or(0);
                        }
                        if let Some(v) = h.to_lowercase().strip_prefix("x-token:") {
                            token = v.trim().to_string();
                        }
                    }
                    let mut body = vec![0u8; len];
                    let _ = reader.read_exact(&mut body);
                    let (status, extra, payload): (&str, String, Vec<u8>) = match path.as_str() {
                        "/json" => ("200 OK", "Content-Type: application/json\r\nX-Test: yes\r\n".into(), br#"{"a":1}"#.to_vec()),
                        "/redirect" => ("302 Found", format!("Location: http://localhost:{port}/json\r\n"), vec![]),
                        "/missing" => ("404 Not Found", String::new(), b"nope".to_vec()),
                        "/echo" => ("200 OK", String::new(), format!("{method}|{token}|{}", String::from_utf8_lossy(&body)).into_bytes()),
                        "/audio.flac" => ("200 OK", String::new(), b"fLaC-test-bytes".to_vec()),
                        _ => ("404 Not Found", String::new(), vec![]),
                    };
                    let head = format!("HTTP/1.1 {status}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n", payload.len());
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(&payload);
                });
            }
        });
        port
    }

    fn net_plugin(root: &Path, id: &str, perms: &str, script: &str, port: u16) -> Discovered {
        make_plugin(root, id, &format!(r#","permissions":[{perms}],"config":{{"base":"http://127.0.0.1:{port}"}}"#), script)
    }

    #[test]
    fn host_patterns_never_match_local_addresses_through_wildcards() {
        let any = vec!["network:*".to_string()];
        assert!(allows_host(&any, "example.com"));
        assert!(!allows_host(&any, "127.0.0.1"));
        assert!(!allows_host(&any, "localhost"));
        assert!(!allows_host(&any, "192.168.1.5"));
        assert!(!allows_host(&any, "printer.local"));
        let sub = vec!["network:*.example.com".to_string()];
        assert!(allows_host(&sub, "a.example.com"));
        assert!(allows_host(&sub, "A.B.Example.com"));
        assert!(!allows_host(&sub, "example.com"));
        assert!(!allows_host(&sub, "evilexample.com"));
        let exact = vec!["network:127.0.0.1".to_string(), "network:api.site.org".to_string()];
        assert!(allows_host(&exact, "127.0.0.1"));
        assert!(allows_host(&exact, "API.site.org"));
        assert!(!allows_host(&exact, "site.org"));
        assert!(!allows_host(&[], "example.com"));
    }

    #[test]
    fn fetch_returns_status_headers_and_json() {
        let port = serve_http();
        let root = temp_dir("fetch1");
        let d = net_plugin(
            &root,
            "f1",
            r#""network:127.0.0.1""#,
            r#"sw.fetch(sw.config.base + "/json").then(function (r) {
                 return r.json().then(function (j) { sw.toast(r.status + ":" + r.ok + ":" + j.a + ":" + r.headers.get("X-Test")); });
               });
               sw.fetch(sw.config.base + "/missing").then(function (r) { return r.text().then(function (t) { sw.toast("m:" + r.status + ":" + r.ok + ":" + t); }); });"#,
            port,
        );
        let mut host = PluginHost::with_data_dir(root.join("data"));
        host.start(&d).unwrap();
        let mut seen = vec![];
        wait_for(|| {
            seen.extend(host.drain_toasts());
            (seen.len() >= 2).then_some(())
        })
        .unwrap();
        assert!(seen.contains(&"200:true:1:yes".to_string()), "{seen:?}");
        assert!(seen.contains(&"m:404:false:nope".to_string()), "{seen:?}");
    }

    #[test]
    fn fetch_sends_method_headers_and_body() {
        let port = serve_http();
        let root = temp_dir("fetch2");
        let d = net_plugin(
            &root,
            "f2",
            r#""network:127.0.0.1""#,
            r#"sw.fetch(sw.config.base + "/echo", { method: "POST", headers: { "X-Token": "abc" }, body: "şarkı" })
                 .then(function (r) { return r.text(); }).then(function (t) { sw.toast(t); });"#,
            port,
        );
        let mut host = PluginHost::with_data_dir(root.join("data"));
        host.start(&d).unwrap();
        assert_eq!(first_toast(&host).as_deref(), Some("POST|abc|şarkı"));
    }

    #[test]
    fn fetch_refuses_hosts_outside_the_permissions_including_redirects() {
        let port = serve_http();
        let root = temp_dir("fetch3");
        let d = net_plugin(
            &root,
            "f3",
            r#""network:127.0.0.1""#,
            r#"sw.fetch("http://example.com/").catch(function (e) { sw.toast("a:" + e.message); });
               sw.fetch(sw.config.base + "/redirect").catch(function (e) { sw.toast("b:" + e.message); });"#,
            port,
        );
        let mut host = PluginHost::with_data_dir(root.join("data"));
        host.start(&d).unwrap();
        let mut seen = vec![];
        wait_for(|| {
            seen.extend(host.drain_toasts());
            (seen.len() >= 2).then_some(())
        })
        .unwrap();
        assert!(seen.iter().any(|t| t.starts_with("a:") && t.contains("not allowed")), "{seen:?}");
        assert!(seen.iter().any(|t| t.starts_with("b:") && t.contains("localhost") && t.contains("not allowed")), "{seen:?}");
    }

    #[test]
    fn fetch_needs_a_network_permission_and_logs_unhandled_rejections() {
        let root = temp_dir("fetch4");
        let d = make_plugin(&root, "f4", "", r#"sw.fetch("http://example.com/");"#);
        let mut host = PluginHost::with_data_dir(root.join("data"));
        host.start(&d).unwrap();
        wait_for(|| {
            let logs = host.logs("f4");
            (logs.iter().any(|l| l.contains("'network:<host>' permission")) && logs.iter().any(|l| l.contains("unhandled promise rejection"))).then_some(())
        })
        .expect("permission error and unhandled rejection logged");
    }

    #[test]
    fn download_saves_into_the_downloads_folder_and_library_add_is_confined_to_it() {
        let port = serve_http();
        let root = temp_dir("download");
        let outside = root.join("outside.flac");
        std::fs::write(&outside, b"x").unwrap();
        let outside_js = outside.to_string_lossy().replace('\\', "\\\\");
        let d = net_plugin(
            &root,
            "dl",
            r#""network:127.0.0.1","download","library""#,
            &format!(
                r#"sw.download(sw.config.base + "/audio.flac", {{ filename: "a/b:c.flac" }}).then(function (r) {{
                     sw.toast("saved:" + r.size);
                     sw.toast("add:" + sw.library.add(r.path));
                     sw.toast("outside:" + sw.library.add("{outside_js}"));
                   }});"#
            ),
            port,
        );
        let mut host = PluginHost::with_data_dir(root.join("data"));
        host.start(&d).unwrap();
        let mut seen = vec![];
        wait_for(|| {
            seen.extend(host.drain_toasts());
            (seen.len() >= 3).then_some(())
        })
        .unwrap();
        assert_eq!(seen, vec!["saved:15".to_string(), "add:true".to_string(), "outside:false".to_string()]);
        let saved = host.downloads_dir().join("a_b_c.flac");
        assert_eq!(std::fs::read(&saved).unwrap(), b"fLaC-test-bytes");
        let added = host.drain_library();
        assert_eq!(added.len(), 1);
        assert_eq!(std::fs::canonicalize(&saved).unwrap(), added[0]);
        assert!(!host.downloads_dir().join("a_b_c.part").exists());
    }

    #[test]
    fn download_picks_a_free_name_instead_of_overwriting() {
        let port = serve_http();
        let root = temp_dir("download2");
        let d = net_plugin(
            &root,
            "d2",
            r#""network:127.0.0.1","download""#,
            r#"sw.download(sw.config.base + "/audio.flac", { filename: "t.flac" }).then(function () {
                 return sw.download(sw.config.base + "/audio.flac", { filename: "t.flac" });
               }).then(function (r) { sw.toast(r.path); });"#,
            port,
        );
        let mut host = PluginHost::with_data_dir(root.join("data"));
        host.start(&d).unwrap();
        let path = first_toast(&host).unwrap();
        assert!(path.ends_with("t (1).flac"), "{path}");
        assert!(host.downloads_dir().join("t.flac").exists());
    }

    #[test]
    fn download_from_url_example_downloads_and_adds_to_the_library() {
        let port = serve_http();
        let root = temp_dir("dlurl");
        let dir = root.join("download-from-url");
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = include_str!("../examples/download-from-url/soundworks.json").replace("network:*", "network:127.0.0.1");
        std::fs::write(dir.join("soundworks.json"), manifest).unwrap();
        std::fs::write(dir.join("main.js"), include_str!("../examples/download-from-url/main.js")).unwrap();
        let d = discover(&root).into_iter().next().unwrap();
        assert!(d.error.is_none(), "{:?}", d.error);
        let mut host = PluginHost::with_data_dir(root.join("data"));
        host.start(&d).unwrap();
        wait_for(|| host.ui_snapshots().into_iter().next().filter(|(_, s)| !s.panels.is_empty()).map(|_| ())).expect("panel published");
        let url = format!("http://127.0.0.1:{port}/audio.flac");
        host.send_action("download-from-url", "panel", "download", &serde_json::json!({"type":"input","id":"url","value":url}).to_string());
        host.send_action("download-from-url", "panel", "download", r#"{"type":"click","id":"go"}"#);
        let status = wait_for(|| {
            let snap = host.ui_snapshots().into_iter().next()?.1;
            match snap.panels[0].content.first() {
                Some(UiNode::Text { text, .. }) if text.starts_with("Added to your library") => Some(text.clone()),
                _ => None,
            }
        })
        .expect("download finished");
        assert!(status.ends_with("audio.flac"), "{status}");
        assert_eq!(host.drain_library().len(), 1);
        assert_eq!(std::fs::read(host.downloads_dir().join("audio.flac")).unwrap(), b"fLaC-test-bytes");
    }
}
