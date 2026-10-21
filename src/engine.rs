//! WASAPI: kaynak cihazın loopback'ini yakala -> EQ -> hedef cihaza (tercihen exclusive) yaz.

use crate::apo::Preset;
use crate::biquad::Equalizer;
use anyhow::{anyhow, bail, Context, Result};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use windows::core::{GUID, PCWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::*;
use windows::Win32::System::Threading::*;

pub static STOP: AtomicBool = AtomicBool::new(false); // Ctrl+C (CLI)
/// Tepsi menüsünden: EQ'yu kalıcı olarak devre dışı bırak.
pub static EQ_OFF: AtomicBool = AtomicBool::new(false);

/// Çalışan motoru dışarıdan yönetmek için ortak durum.
pub struct Control {
    pub stop: AtomicBool,
    version: std::sync::atomic::AtomicU64,
    preset: Mutex<Preset>,
    pub status: Mutex<String>,
    /// A/B: true iken EQ atlanır.
    pub bypass: AtomicBool,
    /// A/B'de EQ'süz sesi preamp kadar kıs (adil karşılaştırma).
    pub level_match: AtomicBool,
    /// Görselleştirici için son çıkış örnekleri (mono, çıkış hızında).
    pub viz: Mutex<VecDeque<f32>>,
    pub viz_seq: std::sync::atomic::AtomicU64,
    pub viz_rate: std::sync::atomic::AtomicU32,
    /// -1..1 (f32 bit)
    pub balance: std::sync::atomic::AtomicU32,
    /// 0..1 (f32 bit)
    pub crossfeed: std::sync::atomic::AtomicU32,
    pub mono: AtomicBool,
    /// Son okumadan beri tepe değerler (f32 bit, >=0 olduğu için bit sırası = değer sırası)
    pub peak_l: std::sync::atomic::AtomicU32,
    pub peak_r: std::sync::atomic::AtomicU32,
    pub clips: std::sync::atomic::AtomicU32,
    pub out_info: Mutex<OutInfo>,
    /// bit-perfect kanıtı: cihaza yazılan çerçevelerin ölçümü
    pub proof: crate::proof::Proof,
    /// Kaynak (sanal) cihazın Windows ses seviyesi/mute'u; loopback bunu içermediği için biz uygularız.
    pub src_gain: std::sync::atomic::AtomicU32,
    /// Arayüzde gösterilecek tek seferlik bildirim.
    pub notice: Mutex<Option<String>>,
    /// 16 bit çıkışta TPDF dither (yalnızca sinyale dokunulmuşsa uygulanır).
    pub dither: AtomicBool,
    /// Sinyal zinciri modülleri (EQ'dan sonra, ses seviyesinden önce).
    chain: Mutex<Vec<crate::fx::FxSpec>>,
    chain_ver: std::sync::atomic::AtomicU64,
    /// Band solo: açıkken yalnızca `solo_freq` (Hz) çevresi, `solo_q` bant genişliğiyle çalar.
    pub solo: AtomicBool,
    /// motor, çıkış cihazı kaybolduğu için (çıkarıldı/bağlantı koptu) sonlandı
    pub gone: AtomicBool,
    /// Canlı ölçümler (f32 bit; < -150 = ölçülemedi): anlık/kısa vadeli/bütünleşik LUFS ve gerçek tepe dBTP.
    pub m_lufs: std::sync::atomic::AtomicU32,
    pub s_lufs: std::sync::atomic::AtomicU32,
    pub i_lufs: std::sync::atomic::AtomicU32,
    pub tp_db: std::sync::atomic::AtomicU32,
    /// arayüz isteği: ölçümleri sıfırla
    pub meter_reset: AtomicBool,
    pub solo_freq: std::sync::atomic::AtomicU32,
    pub solo_q: std::sync::atomic::AtomicU32,
}

#[derive(Default, Clone)]
pub struct OutInfo {
    pub exclusive: bool,
    pub rate: u32,
    pub bits: u16,
    pub float: bool,
    pub device: String,
    /// DoP (DSD over PCM) çıkışı: ses seviyesi, EQ, dither ve modüller devre dışıdır
    pub dop: bool,
}

impl Control {
    pub fn new(preset: Preset) -> Arc<Control> {
        Arc::new(Control {
            stop: AtomicBool::new(false),
            version: std::sync::atomic::AtomicU64::new(0),
            preset: Mutex::new(preset),
            status: Mutex::new(String::new()),
            bypass: AtomicBool::new(false),
            level_match: AtomicBool::new(true),
            viz: Mutex::new(VecDeque::new()),
            viz_seq: std::sync::atomic::AtomicU64::new(0),
            viz_rate: std::sync::atomic::AtomicU32::new(48000),
            balance: std::sync::atomic::AtomicU32::new(0f32.to_bits()),
            crossfeed: std::sync::atomic::AtomicU32::new(0f32.to_bits()),
            mono: AtomicBool::new(false),
            peak_l: std::sync::atomic::AtomicU32::new(0),
            peak_r: std::sync::atomic::AtomicU32::new(0),
            clips: std::sync::atomic::AtomicU32::new(0),
            out_info: Mutex::new(OutInfo::default()),
            proof: Default::default(),
            src_gain: std::sync::atomic::AtomicU32::new(1f32.to_bits()),
            notice: Mutex::new(None),
            dither: AtomicBool::new(true),
            chain: Mutex::new(Vec::new()),
            chain_ver: std::sync::atomic::AtomicU64::new(0),
            solo: AtomicBool::new(false),
            gone: AtomicBool::new(false),
            m_lufs: std::sync::atomic::AtomicU32::new((-200f32).to_bits()),
            s_lufs: std::sync::atomic::AtomicU32::new((-200f32).to_bits()),
            i_lufs: std::sync::atomic::AtomicU32::new((-200f32).to_bits()),
            tp_db: std::sync::atomic::AtomicU32::new((-200f32).to_bits()),
            meter_reset: AtomicBool::new(false),
            solo_freq: std::sync::atomic::AtomicU32::new(1000f32.to_bits()),
            solo_q: std::sync::atomic::AtomicU32::new(2f32.to_bits()),
        })
    }
    /// Çalışırken preseti değiştir (EQ yeniden kurulur).
    pub fn set_preset(&self, p: Preset) {
        *self.preset.lock().unwrap() = p;
        self.version.fetch_add(1, Ordering::SeqCst);
    }
    /// Modül zincirini değiştir (yalnızca parametre değiştiyse durum korunur, yapı değiştiyse çapraz geçiş yapılır).
    pub fn set_chain(&self, v: Vec<crate::fx::FxSpec>) {
        *self.chain.lock().unwrap() = v;
        self.chain_ver.fetch_add(1, Ordering::SeqCst);
    }
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed) || STOP.load(Ordering::Relaxed)
    }
    fn say(&self, s: impl Into<String>) {
        let s = s.into();
        println!("{s}");
        *self.status.lock().unwrap() = s;
    }
}

const GUID_FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
const GUID_PCM: GUID = GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);
const E_NOT_ALIGNED: i32 = 0x88890019u32 as i32;

// ---------------------------------------------------------------- formatlar

#[derive(Debug, Clone, Copy)]
struct Fmt {
    rate: u32,
    channels: u16,
    container_bits: u16,
    valid_bits: u16,
    float: bool,
    mask: u32,
}

impl Fmt {
    fn frame_bytes(&self) -> usize {
        self.channels as usize * (self.container_bits as usize / 8)
    }

    unsafe fn from_ptr(p: *const WAVEFORMATEX) -> Fmt {
        let w = p.read_unaligned();
        if w.wFormatTag == 0xFFFE {
            let e = (p as *const WAVEFORMATEXTENSIBLE).read_unaligned();
            let valid = e.Samples.wValidBitsPerSample;
            let sub = e.SubFormat;
            Fmt {
                rate: w.nSamplesPerSec,
                channels: w.nChannels,
                container_bits: w.wBitsPerSample,
                valid_bits: if valid == 0 { w.wBitsPerSample } else { valid },
                float: sub == GUID_FLOAT,
                mask: e.dwChannelMask,
            }
        } else {
            Fmt {
                rate: w.nSamplesPerSec,
                channels: w.nChannels,
                container_bits: w.wBitsPerSample,
                valid_bits: w.wBitsPerSample,
                float: w.wFormatTag == 3,
                mask: 0,
            }
        }
    }

    fn to_wfx(&self) -> WAVEFORMATEXTENSIBLE {
        let align = self.frame_bytes() as u16;
        WAVEFORMATEXTENSIBLE {
            Format: WAVEFORMATEX {
                wFormatTag: 0xFFFE,
                nChannels: self.channels,
                nSamplesPerSec: self.rate,
                nAvgBytesPerSec: self.rate * align as u32,
                nBlockAlign: align,
                wBitsPerSample: self.container_bits,
                cbSize: 22,
            },
            Samples: WAVEFORMATEXTENSIBLE_0 { wValidBitsPerSample: self.valid_bits },
            dwChannelMask: self.mask,
            SubFormat: if self.float { GUID_FLOAT } else { GUID_PCM },
        }
    }
}

fn read_sample(b: &[u8], f: &Fmt) -> f32 {
    if f.float {
        return f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    }
    let bytes = f.container_bits as usize / 8;
    let mut v: i32 = 0;
    for (i, x) in b[..bytes].iter().enumerate() {
        v |= (*x as i32) << (8 * (4 - bytes + i));
    }
    v as f32 / 2147483648.0
}

/// 4 noktalı kübik (Catmull-Rom) interpolasyon: p1 ile p2 arasında, t in [0, 1]. t=0'da tam p1.
fn catmull_rom(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * (2.0 * p1 + (-p0 + p2) * t + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2 + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

/// TPDF dither üreteci: iki düzgün dağılımın farkı, [-1, 1) LSB.
struct Dither {
    s: u32,
}

impl Dither {
    fn new(seed: u32) -> Dither {
        Dither { s: seed.max(1) }
    }
    fn next_u(&mut self) -> f32 {
        // xorshift32
        let mut x = self.s;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.s = x;
        (x >> 8) as f32 / 16777216.0
    }
    fn tpdf(&mut self) -> f32 {
        self.next_u() - self.next_u()
    }
}

/// Üstel yaklaşım; hedefe yeterince yaklaşınca tam hedefe oturur. f32'de `cur += (tgt - cur) * k` hedefin
/// ~4e-5 altında takılır (artım yarım ULP'nin altına düşer); bu da %100 seviyede bile bit-perfect yolu bozardı.
fn approach(cur: f32, tgt: f32, k: f32) -> f32 {
    let n = cur + (tgt - cur) * k;
    if (tgt - n).abs() < 1.0e-4 * tgt.max(1.0e-3) {
        tgt
    } else {
        n
    }
}

/// Sinyale dokunulmuşsa (EQ/DSP, ses ≠ %100, fade) dither uygulanır; bit-perfect yolda uygulanmaz.
fn needs_dither(dither_on: bool, chain_touched: bool, gain_cur: f32, gate_cur: f32) -> bool {
    dither_on && (chain_touched || (gain_cur - 1.0).abs() > 1e-6 || gate_cur < 1.0)
}

/// `dither`: LSB biriminde eklenecek gürültü (0 = yok).
fn write_sample(b: &mut [u8], s: f32, f: &Fmt, dither: f32) {
    let s = s.clamp(-1.0, 1.0);
    if f.float {
        b[..4].copy_from_slice(&s.to_le_bytes());
        return;
    }
    let max = (1i64 << (f.valid_bits - 1)) - 1;
    let v = (s as f64 * max as f64 + dither as f64).round() as i64;
    let v = v.clamp(-max - 1, max);
    // sol hizalı 32 bit değer; container kadar üst baytı yaz
    let left = ((v << (32 - f.valid_bits)) as i32).to_le_bytes();
    let bytes = f.container_bits as usize / 8;
    b[..bytes].copy_from_slice(&left[4 - bytes..]);
}

// ----------------------------------------------------------------- cihazlar

/// Çıkış cihazının bağlantı türü (uyarılar ve hız eşleştirme için).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceKind {
    Bluetooth,
    Usb,
    Hdmi,
    /// anakart / dahili ses (HD Audio, Intel SST...)
    Builtin,
    /// sanal kablo, yazılım cihazı
    Virtual,
    Unknown,
}

pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub kind: DeviceKind,
}

/// Cihaz türünü bus adından (PKEY_Device_EnumeratorName), form faktöründen ve addan çıkarır.
pub fn kind_from(enumerator: &str, form_factor: Option<u32>, name: &str) -> DeviceKind {
    let en = enumerator.to_uppercase();
    let nm = name.to_lowercase();
    if en.contains("BTH") || nm.contains("bluetooth") || nm.contains("hands-free") || nm.contains("handsfree") {
        return DeviceKind::Bluetooth;
    }
    // form faktör 9 = HDMI, 8 = S/PDIF, 10 = DisplayPort...
    if form_factor == Some(9) || nm.contains("hdmi") || nm.contains("displayport") || nm.contains("nvidia high definition") {
        return DeviceKind::Hdmi;
    }
    if nm.contains("cable") || nm.contains("vb-audio") || nm.contains("virtual") || nm.contains("voicemeeter") {
        return DeviceKind::Virtual;
    }
    if en.starts_with("USB") {
        return DeviceKind::Usb;
    }
    if en.contains("HDAUDIO") || en.contains("INTELAUDIO") || en.contains("ACPI") || en.starts_with("PCI") {
        return DeviceKind::Builtin;
    }
    if en == "ROOT" || en == "SWD" || en.contains("VIRTUAL") {
        return DeviceKind::Virtual;
    }
    DeviceKind::Unknown
}

impl DeviceKind {
    pub fn label(self) -> &'static str {
        match self {
            DeviceKind::Bluetooth => "Bluetooth",
            DeviceKind::Usb => "USB",
            DeviceKind::Hdmi => "HDMI/DisplayPort",
            DeviceKind::Builtin => "Dahili ses",
            DeviceKind::Virtual => "Sanal cihaz",
            DeviceKind::Unknown => "?",
        }
    }
}

pub fn com_init() -> Result<()> {
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
    Ok(())
}

fn enumerator() -> Result<IMMDeviceEnumerator> {
    Ok(unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? })
}

unsafe fn device_id(d: &IMMDevice) -> Result<String> {
    let p = d.GetId()?;
    let s = p.to_string()?;
    CoTaskMemFree(Some(p.0 as _));
    Ok(s)
}

unsafe fn device_name(d: &IMMDevice) -> Result<String> {
    let store = d.OpenPropertyStore(STGM_READ)?;
    let pv = store.GetValue(&PKEY_Device_FriendlyName)?;
    let p = PropVariantToStringAlloc(&pv)?;
    let s = p.to_string()?;
    CoTaskMemFree(Some(p.0 as _));
    Ok(s)
}

unsafe fn prop_string(store: &windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore, key: &windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY) -> String {
    match store.GetValue(key) {
        Ok(pv) => match PropVariantToStringAlloc(&pv) {
            Ok(p) => {
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as _));
                s
            }
            Err(_) => String::new(),
        },
        Err(_) => String::new(),
    }
}

unsafe fn device_kind(d: &IMMDevice, name: &str) -> DeviceKind {
    use windows::Win32::System::Com::StructuredStorage::PropVariantToUInt32;
    use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
    let Ok(store) = d.OpenPropertyStore(STGM_READ) else { return kind_from("", None, name) };
    let enum_key = PROPERTYKEY { fmtid: GUID::from_values(0xA45C254E, 0xDF1C, 0x4EFD, [0x80, 0x20, 0x67, 0xD1, 0x46, 0xA8, 0x50, 0xE0]), pid: 24 };
    let form_key = PROPERTYKEY { fmtid: GUID::from_values(0x1DA5D803, 0xD492, 0x4EDD, [0x8C, 0x23, 0xE0, 0xC0, 0xFF, 0xEE, 0x7F, 0x0E]), pid: 0 };
    let en = prop_string(&store, &enum_key);
    let ff = store.GetValue(&form_key).ok().and_then(|pv| PropVariantToUInt32(&pv).ok());
    kind_from(&en, ff, name)
}

pub fn list_devices() -> Result<Vec<DeviceInfo>> {
    unsafe {
        let en = enumerator()?;
        let def = device_id(&en.GetDefaultAudioEndpoint(eRender, eConsole)?)?;
        let coll = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        let mut out = vec![];
        for i in 0..coll.GetCount()? {
            let d = coll.Item(i)?;
            let id = device_id(&d)?;
            let name = device_name(&d)?;
            let kind = device_kind(&d, &name);
            out.push(DeviceInfo { is_default: id == def, kind, name, id });
        }
        Ok(out)
    }
}

/// Aramayla eşleşen cihaz: önce tam kimlik, sonra tam ad, en son ad parçası (ilk eşleşen). Saf; test edilebilir.
fn pick_device(devs: Vec<DeviceInfo>, q: &str) -> Option<DeviceInfo> {
    let ql = q.to_lowercase();
    let idx = devs
        .iter()
        .position(|d| d.id == q)
        .or_else(|| devs.iter().position(|d| d.name.to_lowercase() == ql))
        .or_else(|| devs.iter().position(|d| d.name.to_lowercase().contains(&ql)))?;
    devs.into_iter().nth(idx)
}

/// `None` -> varsayılan; aksi halde cihaz kimliği (arayüz) ya da ad / ad parçası (komut satırı).
pub fn resolve(query: Option<&str>) -> Result<DeviceInfo> {
    let devs = list_devices()?;
    let found = match query {
        None => devs.into_iter().find(|d| d.is_default),
        Some(q) => pick_device(devs, q),
    };
    found.ok_or_else(|| anyhow!("cihaz bulunamadı: {}", query.unwrap_or("<varsayılan>")))
}

/// Eski yapılandırmalarda cihaz adla saklanıyordu: ad bağlı bir cihaza denk geliyorsa kimliğine çevirir.
/// Cihaz şu an bağlı değilse (ad belirsiz) olduğu gibi bırakır.
pub fn migrate_device_ref(devs: &[DeviceInfo], stored: &mut Option<String>) -> bool {
    let Some(s) = stored.as_ref() else { return false };
    if devs.iter().any(|d| &d.id == s) {
        return false;
    }
    let sl = s.to_lowercase();
    let exact: Vec<&DeviceInfo> = devs.iter().filter(|d| d.name.to_lowercase() == sl).collect();
    let cand = if exact.len() == 1 {
        exact
    } else {
        let part: Vec<&DeviceInfo> = devs.iter().filter(|d| d.name.to_lowercase().contains(&sl)).collect();
        if part.len() == 1 { part } else { vec![] }
    };
    match cand.first() {
        Some(d) => {
            *stored = Some(d.id.clone());
            true
        }
        None => false,
    }
}

unsafe fn open(id: &str) -> Result<IMMDevice> {
    let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
    Ok(enumerator()?.GetDevice(PCWSTR(wide.as_ptr()))?)
}

// ------------------------------------------------------------------ halka

pub struct Ring {
    pub q: Mutex<VecDeque<f32>>, // interleaved stereo
    /// false: motor sesi birkaç ms'de sıfıra indirir ve halkayı tüketmeden bekler (duraklatma).
    pub gate: Arc<AtomicBool>,
    /// true: seek/parça değişiminde halka temizlenirken geçici kapı.
    pub hold: AtomicBool,
    /// Motor tamamen sessizliğe indiğinde true (halka güvenle temizlenebilir).
    pub muted: AtomicBool,
    /// Doğrusal ses kazancı (f32 bit); motor yumuşatarak uygular.
    pub vol: std::sync::atomic::AtomicU32,
}

impl Ring {
    pub fn new() -> Arc<Ring> {
        Ring::with_gate(Arc::new(AtomicBool::new(true)))
    }
    pub fn with_gate(gate: Arc<AtomicBool>) -> Arc<Ring> {
        Arc::new(Ring {
            q: Mutex::new(VecDeque::new()),
            gate,
            hold: AtomicBool::new(false),
            muted: AtomicBool::new(false),
            vol: std::sync::atomic::AtomicU32::new(1f32.to_bits()),
        })
    }
}

// ---------------------------------------------------------------- yakalama

fn capture_thread(src_id: String, ctl: Arc<Control>, ring: Arc<Ring>, tx: mpsc::Sender<Result<u32>>) {
    let run = || -> Result<()> {
        com_init()?;
        unsafe {
            let dev = open(&src_id)?;
            let src_vol: Option<IAudioEndpointVolume> = dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok();
            let mut last_vol_poll = std::time::Instant::now() - Duration::from_secs(1);
            let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
            let mix = client.GetMixFormat()?;
            let fmt = Fmt::from_ptr(mix);
            client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 2_000_000, 0, mix, None)?;
            CoTaskMemFree(Some(mix as _));
            let cap: IAudioCaptureClient = client.GetService()?;
            let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
            client.Start()?;
            let _ = tx.send(Ok(fmt.rate));

            let ch = fmt.channels as usize;
            let sb = fmt.container_bits as usize / 8;
            let cap_limit = fmt.rate as usize * 2 * 2; // ~2 sn sınır
            let mut tmp: Vec<f32> = Vec::new();

            while !ctl.stopped() {
                std::thread::sleep(Duration::from_millis(4));
                if last_vol_poll.elapsed() >= Duration::from_millis(80) {
                    last_vol_poll = std::time::Instant::now();
                    if let Some(v) = &src_vol {
                        let muted = v.GetMute().map(|m| m.as_bool()).unwrap_or(false);
                        let db = v.GetMasterVolumeLevel().unwrap_or(0.0).min(0.0);
                        let g = if muted { 0.0 } else { 10f32.powf(db / 20.0) };
                        ctl.src_gain.store(g.to_bits(), Ordering::Relaxed);
                    }
                }
                loop {
                    let n = cap.GetNextPacketSize()?;
                    if n == 0 {
                        break;
                    }
                    let mut data: *mut u8 = std::ptr::null_mut();
                    let mut frames = 0u32;
                    let mut flags = 0u32;
                    cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                    tmp.clear();
                    tmp.resize(frames as usize * 2, 0.0);
                    if flags & 2 == 0 {
                        let bytes = std::slice::from_raw_parts(data, frames as usize * ch * sb);
                        for i in 0..frames as usize {
                            let base = i * ch * sb;
                            let l = read_sample(&bytes[base..], &fmt);
                            let r = if ch > 1 { read_sample(&bytes[base + sb..], &fmt) } else { l };
                            tmp[i * 2] = l;
                            tmp[i * 2 + 1] = r;
                        }
                    }
                    cap.ReleaseBuffer(frames)?;
                    let mut q = ring.q.lock().unwrap();
                    q.extend(tmp.iter());
                    while q.len() > cap_limit {
                        q.pop_front();
                    }
                }
            }
            client.Stop()?;
        }
        Ok(())
    };
    if let Err(e) = run() {
        let _ = tx.send(Err(e));
        ctl.request_stop();
    }
}

// ------------------------------------------------------------------ çıkış

struct RenderStream {
    client: IAudioClient,
    fmt: Fmt,
    exclusive: bool,
}

unsafe fn init_render(dev: &IMMDevice, src_rate: u32, allow_exclusive: bool) -> Result<RenderStream> {
    let probe: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
    let mix = probe.GetMixFormat()?;
    let mix_fmt = Fmt::from_ptr(mix);
    CoTaskMemFree(Some(mix as _));

    if allow_exclusive {
        let mut rates = vec![src_rate, mix_fmt.rate, 48000, 44100, 96000, 192000, 88200];
        let mut seen = std::collections::HashSet::new();
        rates.retain(|r| seen.insert(*r));
        // (container, valid, float)
        let formats = [(32u16, 32u16, true), (32, 32, false), (32, 24, false), (24, 24, false), (16, 16, false)];
        for rate in rates {
            for &(container, valid, float) in &formats {
                let f = Fmt { rate, channels: 2, container_bits: container, valid_bits: valid, float, mask: 3 };
                let w = f.to_wfx();
                let hr = probe.IsFormatSupported(
                    AUDCLNT_SHAREMODE_EXCLUSIVE,
                    &w as *const _ as *const WAVEFORMATEX,
                    None,
                );
                if hr.0 != 0 {
                    continue;
                }
                if let Ok(client) = try_init_exclusive(dev, &f) {
                    return Ok(RenderStream { client, fmt: f, exclusive: true });
                }
            }
        }
        eprintln!("! Exclusive mod başlatılamadı, shared moda düşülüyor (Dolby/Atmos devrede kalabilir).");
    }

    let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
    let mix = client.GetMixFormat()?;
    let fmt = Fmt::from_ptr(mix);
    client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, 1_000_000, 0, mix, None)?;
    CoTaskMemFree(Some(mix as _));
    Ok(RenderStream { client, fmt, exclusive: false })
}

unsafe fn try_init_exclusive(dev: &IMMDevice, f: &Fmt) -> Result<IAudioClient> {
    let w = f.to_wfx();
    let wp = &w as *const _ as *const WAVEFORMATEX;
    let mut client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
    let mut period = 0i64;
    client.GetDevicePeriod(Some(&mut period), None)?;
    let mut r = client.Initialize(AUDCLNT_SHAREMODE_EXCLUSIVE, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, period, period, wp, None);
    if let Err(e) = &r {
        if e.code().0 == E_NOT_ALIGNED {
            let frames = client.GetBufferSize()?;
            period = (10_000_000.0 / f.rate as f64 * frames as f64 + 0.5) as i64;
            client = dev.Activate(CLSCTX_ALL, None)?;
            r = client.Initialize(AUDCLNT_SHAREMODE_EXCLUSIVE, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, period, period, wp, None);
        }
    }
    r?;
    Ok(client)
}

/// Standart hızlar (cihaz desteği sorgusu).
pub const STD_RATES: &[u32] = &[44100, 48000, 88200, 96000, 176400, 192000, 352800, 384000];

/// Cihazın exclusive modda kabul ettiği hızlar. Cihaz başka bir uygulamada kullanımdaysa ya da sorgulanamazsa boş döner
/// (çağıran sabit hıza düşer). COM için ayrı bir iş parçacığı kullanır; arayüz thread'inden güvenle çağrılabilir.
pub fn supported_rates(dev_id: &str) -> Vec<u32> {
    let id = dev_id.to_string();
    std::thread::spawn(move || -> Vec<u32> {
        if com_init().is_err() {
            return vec![];
        }
        unsafe {
            let Ok(dev) = open(&id) else { return vec![] };
            let Ok(probe) = dev.Activate::<IAudioClient>(CLSCTX_ALL, None) else { return vec![] };
            let formats = [(32u16, 32u16, true), (32, 32, false), (32, 24, false), (24, 24, false), (16, 16, false)];
            let mut out = vec![];
            for &rate in STD_RATES {
                for &(container, valid, float) in &formats {
                    let f = Fmt { rate, channels: 2, container_bits: container, valid_bits: valid, float, mask: 3 };
                    let w = f.to_wfx();
                    let hr = probe.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, &w as *const _ as *const WAVEFORMATEX, None);
                    if hr.0 == 0 {
                        out.push(rate);
                        break;
                    }
                }
            }
            out
        }
    })
    .join()
    .unwrap_or_default()
}

/// Parça hızına en uygun çıkış hızı: cihaz destekliyorsa aynısı; yoksa aynı aileden (44.1k/48k) en yakın üst kat;
/// o da yoksa herhangi bir üst hız; hiçbiri yoksa en yüksek desteklenen. Liste boşsa `fallback`.
pub fn choose_rate(src: u32, supported: &[u32], fallback: u32) -> u32 {
    if supported.is_empty() {
        return fallback;
    }
    if supported.contains(&src) {
        return src;
    }
    let family = |r: u32| -> u32 {
        if r % 44100 == 0 { 44100 } else if r % 48000 == 0 { 48000 } else { 0 }
    };
    let mut sorted: Vec<u32> = supported.to_vec();
    sorted.sort_unstable();
    // aynı ailede tam kat, kaynağın üstünde, en düşük
    if let Some(r) = sorted.iter().copied().find(|r| *r > src && family(*r) != 0 && family(*r) == family(src)) {
        return r;
    }
    // aynı ailede kaynağın altı (ör. 192k dosya, cihaz 96k'ya kadar): en yüksek
    if let Some(r) = sorted.iter().rev().copied().find(|r| *r < src && family(*r) != 0 && family(*r) == family(src)) {
        return r;
    }
    // başka aile: kaynağın üstündeki en yakın, yoksa en yüksek
    sorted.iter().copied().find(|r| *r > src).unwrap_or(*sorted.last().unwrap())
}

pub struct Options {
    pub from: Option<String>,
    pub to: Option<String>,
    pub shared: bool,
    /// Some ise kaynak loopback değil, dahili oynatıcıdır.
    pub player: Option<Arc<crate::player::Player>>,
    /// DSD'yi DoP olarak çıkar (yalnızca exclusive, oynatıcı kaynağıyla)
    pub dop: bool,
}

pub fn run(ctl: Arc<Control>, opts: Options) -> Result<()> {
    com_init()?;
    let dst = resolve(opts.to.as_deref())?;
    println!("Hedef            : {}", dst.name);

    let (ring, src_rate, cap) = if let Some(p) = &opts.player {
        ctl.say("Kaynak: dahili oynatıcı");
        (p.ring.clone(), p.rate(), None)
    } else {
        let src = resolve(opts.from.as_deref())?;
        if src.id == dst.id {
            bail!("Kaynak ve hedef aynı cihaz ('{}'): geri besleme olur. Farklı bir hedef seç.", dst.name);
        }
        println!("Kaynak (loopback): {}", src.name);
        let ring = Ring::new();
        let (tx, rx) = mpsc::channel();
        let cap = {
            let (ring, id, ctl) = (ring.clone(), src.id.clone(), ctl.clone());
            std::thread::spawn(move || capture_thread(id, ctl, ring, tx))
        };
        let rate = rx.recv().context("yakalama thread'i başlamadı")?.context("loopback başlatılamadı")?;
        (ring, rate, Some(cap))
    };

    let result = unsafe {
        if opts.dop {
            render_loop_dop(&ctl, &dst.id, &dst.name, src_rate, &ring)
        } else {
            render_loop(&ctl, &dst.id, &dst.name, src_rate, &ring, !opts.shared, opts.player.is_none())
        }
    };
    ctl.request_stop();
    if let Some(c) = cap {
        let _ = c.join();
    }
    result
}

/// Halkadan okuma adımı (1.0 = her çıkış çerçevesi tam bir kaynak çerçevesi). Telafi yalnızca kaynağın kendi saati varsa
/// (loopback) ve doluluk hedeften çok saptıysa uygulanır; aksi halde adım sabittir ve örnekler birebir geçer.
fn read_step(drift_comp: bool, base_step: f64, dev_ratio: f64) -> f64 {
    if !drift_comp || (dev_ratio.abs() < 0.5 && (base_step - 1.0).abs() < 1e-9) {
        base_step
    } else {
        base_step * (1.0 + dev_ratio.clamp(-1.0, 1.0) * 0.002)
    }
}

/// `drift_comp`: kaynağın kendi saati varsa (loopback yakalama) halka doluluğuna göre hız telafisi yapılır. Dahili oynatıcıda kaynak
/// saati yoktur (çözücü halkayı ihtiyaca göre doldurur), telafi yapılmaz: örnekler birebir geçer.
unsafe fn render_loop(ctl: &Control, dst_id: &str, dst_name: &str, src_rate: u32, ring: &Arc<Ring>, exclusive: bool, drift_comp: bool) -> Result<()> {
    let dev = open(dst_id)?;
    let mut tries = 0;
    let rs = loop {
        match init_render(&dev, src_rate, exclusive) {
            Ok(r) => break r,
            Err(err) if is_in_use(&err) && tries < 4 => {
                tries += 1;
                ctl.say("Cihaz meşgul, yeniden deneniyor…");
                std::thread::sleep(Duration::from_millis(600));
            }
            Err(err) => return Err(err),
        }
    };
    // Hedef cihaz Windows'ta sessize alınmışsa (özellikle exclusive'de) hiç ses duyulmaz: otomatik aç.
    if let Ok(v) = dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) {
        if v.GetMute().map(|m| m.as_bool()).unwrap_or(false) {
            if v.SetMute(false, std::ptr::null()).is_ok() {
                *ctl.notice.lock().unwrap() = Some("Hedef cihaz sessizdeydi, sesi açıldı".to_string());
            }
        }
        if let Ok(sc) = v.GetMasterVolumeLevelScalar() {
            if sc < 0.03 {
                *ctl.notice.lock().unwrap() = Some("Hedef cihazın Windows sesi çok düşük".to_string());
            }
        }
    }
    let fmt = rs.fmt;
    // Exclusive modda Windows ses seviyesi uygulanmaz; donanım sesi yoksa yazılımla biz uygularız.
    let vol: Option<IAudioEndpointVolume> = if rs.exclusive {
        match dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) {
            Ok(v) if v.QueryHardwareSupport().map(|h| h & 1 == 0).unwrap_or(true) => Some(v),
            _ => None,
        }
    } else {
        None
    };
    ctl.say(format!(
        "Çıkış modu       : {} | {} Hz, {} kanal, {}{} bit",
        if rs.exclusive { "EXCLUSIVE (Dolby/Atmos/APO bypass)" } else { "SHARED (Windows efektleri devrede olabilir)" },
        fmt.rate,
        fmt.channels,
        if fmt.float { "float" } else { "int" },
        fmt.valid_bits
    ));
    println!("Çalışıyor. Durdurmak için Ctrl+C.");

    let event: HANDLE = CreateEventW(None, false, false, None)?;
    rs.client.SetEventHandle(event)?;
    let render: IAudioRenderClient = rs.client.GetService()?;
    let buf_frames = rs.client.GetBufferSize()?;
    let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);

    // ilk buffer sessiz
    let p = render.GetBuffer(buf_frames)?;
    std::ptr::write_bytes(p, 0, buf_frames as usize * fmt.frame_bytes());
    render.ReleaseBuffer(buf_frames, 0)?;
    rs.client.Start()?;

    let target = (src_rate as usize * 40 / 1000).max(buf_frames as usize * 2); // kaynak frame
    let base_step = src_rate as f64 / fmt.rate as f64;
    let mut frac = 0.0f64;
    let mut prev = (0f32, 0f32); // kübik interpolasyon için bir önceki çerçeve
    let mut primed = false;
    let fb = fmt.frame_bytes();
    let sb = fmt.container_bits as usize / 8;
    let mut gain_cur = 0f32; // sessizden yumuşak giriş (ani yüksek ses olmasın)
    let mut gain_tgt = 1f32;
    let mut last_poll = std::time::Instant::now() - Duration::from_secs(1);
    let mut ver = ctl.version.load(Ordering::SeqCst);
    let mut eq = Equalizer::new(&ctl.preset.lock().unwrap(), fmt.rate as f64, 2);
    ctl.viz_rate.store(fmt.rate, Ordering::Relaxed);
    *ctl.out_info.lock().unwrap() = OutInfo {
        exclusive: rs.exclusive,
        rate: fmt.rate,
        bits: fmt.valid_bits,
        float: fmt.float,
        device: dst_name.to_string(),
        dop: false,
    };
    let mut cf = Crossfeed::new(fmt.rate as f32);
    let mut vbuf: Vec<f32> = Vec::with_capacity(4096);
    let mut gate_cur = 0f32; // duraklatma/seek kapısı (0..1, ~6 ms rampa)
    let gate_step = 1.0 / (0.006 * fmt.rate as f32);
    let mut muted_flag = false;
    let mut dith = [Dither::new(0x9E37_79B9), Dither::new(0x85EB_CA6B)];
    let mut old_eq: Option<(Equalizer, u32)> = None; // yapı değişiminde eski EQ ile kısa çapraz geçiş
    let xf_len = ((fmt.rate as f32 * 0.03) as u32).max(1);
    let mut chain = crate::fx::FxChain::new(&ctl.chain.lock().unwrap(), fmt.rate as f32);
    let mut chain_ver = ctl.chain_ver.load(Ordering::SeqCst);
    let mut old_chain: Option<(crate::fx::FxChain, u32)> = None;
    let mut meter = crate::loudness::LiveMeter::new(fmt.rate);
    let mut solo = crate::biquad::BandSolo::new();
    let mut solo_cur = 0f32; // 0..1 yumuşak geçiş (~10 ms)
    let mut solo_last = (0f32, 0f32);
    const SOLO_MAKEUP: f32 = 2.0; // +6 dB: dar bir bant kısık duyulur
    let mut mix_cur = 1f32; // 1 = EQ'lu, 0 = EQ'süz (yumuşak geçiş)

    while !ctl.stopped() {
        if WaitForSingleObject(event, 200) != WAIT_OBJECT_0 {
            continue;
        }
        if last_poll.elapsed() >= Duration::from_millis(100) {
            last_poll = std::time::Instant::now();
            if let Some(v) = &vol {
                let muted = v.GetMute().map(|m| m.as_bool()).unwrap_or(false);
                let db = v.GetMasterVolumeLevel().unwrap_or(0.0);
                gain_tgt = if muted { 0.0 } else { 10f32.powf(db.min(0.0) / 20.0) };
            }
        }
        let v = ctl.version.load(Ordering::SeqCst);
        if v != ver {
            ver = v;
            let pr = ctl.preset.lock().unwrap();
            // bant sayısı aynıysa filtre durumu korunur ve katsayılar yumuşakça kayar (tık yok)
            if !eq.retarget(&pr, fmt.rate as f64) {
                let new = Equalizer::new(&pr, fmt.rate as f64, 2);
                old_eq = Some((std::mem::replace(&mut eq, new), xf_len));
            }
        }
        let cv = ctl.chain_ver.load(Ordering::SeqCst);
        if cv != chain_ver {
            chain_ver = cv;
            let specs = ctl.chain.lock().unwrap();
            if chain.same_structure(&specs) {
                chain.update(&specs);
            } else {
                // modül eklendi/çıkarıldı/sırası değişti: eski zincirle kısa çapraz geçiş (tık olmasın)
                let new = crate::fx::FxChain::new(&specs, fmt.rate as f32);
                old_chain = Some((std::mem::replace(&mut chain, new), xf_len));
            }
        }
        if ctl.meter_reset.swap(false, Ordering::Relaxed) {
            meter.reset();
        }
        let solo_on = ctl.solo.load(Ordering::Relaxed);
        let solo_target = if solo_on { 1f32 } else { 0f32 };
        if solo_on {
            let (sf, sq) = (f32::from_bits(ctl.solo_freq.load(Ordering::Relaxed)), f32::from_bits(ctl.solo_q.load(Ordering::Relaxed)));
            if (sf, sq) != solo_last {
                solo.set(sf as f64, sq as f64, fmt.rate as f64);
                solo_last = (sf, sq);
            }
            if solo_cur < 0.0005 {
                solo.reset(); // başlarken eski durumdan geçiş sesi gelmesin
            }
        }
        let gate_tgt = if ring.gate.load(Ordering::Relaxed) && !ring.hold.load(Ordering::Relaxed) { 1f32 } else { 0f32 };
        let vol_g = f32::from_bits(ring.vol.load(Ordering::Relaxed)).clamp(0.0, 1.0);
        let dither_on = ctl.dither.load(Ordering::Relaxed) && !fmt.float && fmt.valid_bits <= 16;
        let src_gain = f32::from_bits(ctl.src_gain.load(Ordering::Relaxed)).clamp(0.0, 1.0);
        // seviyeye bağlı modüller (Loudness) çalar ses seviyesini bilir
        let level = gain_tgt * src_gain * vol_g;
        chain.set_level(level);
        if let Some((o, _)) = old_chain.as_mut() {
            o.set_level(level);
        }
        let cf_amt = f32::from_bits(ctl.crossfeed.load(Ordering::Relaxed)).clamp(0.0, 1.0);
        let mono = ctl.mono.load(Ordering::Relaxed);
        let bal = f32::from_bits(ctl.balance.load(Ordering::Relaxed)).clamp(-1.0, 1.0);
        let (bal_l, bal_r) = (if bal > 0.0 { 1.0 - bal } else { 1.0 }, if bal < 0.0 { 1.0 + bal } else { 1.0 });
        let (mut pk_l, mut pk_r, mut clips) = (0f32, 0f32, 0u32);
        let mut pf = crate::proof::Counters::default();
        let mix_tgt = if ctl.bypass.load(Ordering::Relaxed) || EQ_OFF.load(Ordering::Relaxed) { 0f32 } else { 1f32 };
        // seviye eşleme yalnızca A/B karşılaştırması içindir (basılı tut); EQ kalıcı kapatılınca sinyale dokunulmaz
        let dry_gain = if ctl.level_match.load(Ordering::Relaxed) && ctl.bypass.load(Ordering::Relaxed) { eq.preamp_gain() } else { 1.0 };
        // sinyale dokunulmuyorsa (bit-perfect yol) dither eklenmez
        let chain_touched = (!eq.is_identity() && mix_tgt > 0.0)
            || chain.any_enabled()
            || old_chain.is_some()
            || solo_on
            || solo_cur > 0.0005
            || old_eq.is_some()
            || (mix_tgt < 1.0 && dry_gain != 1.0)
            || cf_amt > 0.0
            || mono
            || bal_l != 1.0
            || bal_r != 1.0;
        let padding = if rs.exclusive { 0 } else { rs.client.GetCurrentPadding()? };
        let n = buf_frames - padding;
        if n == 0 {
            continue;
        }
        let p = render.GetBuffer(n)?;
        let out = std::slice::from_raw_parts_mut(p, n as usize * fb);
        out.fill(0);

        let mut q = ring.q.lock().unwrap();
        let fill = q.len() / 2;
        if !primed && fill >= target {
            primed = true;
            if q.len() >= 2 {
                prev = (q[0], q[1]);
            }
        }
        if primed {
            // clock drift telafisi: doluluk hedeften saptıkça çok küçük hız ayarı
            let dev_ratio = (fill as f64 - target as f64) / target as f64;
            let step = read_step(drift_comp, base_step, dev_ratio);
            if step == 1.0 {
                frac = 0.0;
            }
            for i in 0..n as usize {
                gate_cur = if gate_cur < gate_tgt { (gate_cur + gate_step).min(gate_tgt) } else { (gate_cur - gate_step).max(gate_tgt) };
                if gate_cur <= 0.0 && gate_tgt == 0.0 {
                    // tam sessiz: halka tüketilmez (duraklatma sonrası kaldığı yerden devam eder)
                    if !muted_flag {
                        muted_flag = true;
                        ring.muted.store(true, Ordering::SeqCst);
                    }
                    break;
                }
                if muted_flag {
                    muted_flag = false;
                    ring.muted.store(false, Ordering::SeqCst);
                }
                if q.len() < 4 {
                    primed = false;
                    gate_cur = 0.0; // sonraki başlangıçta yumuşak giriş
                    pf.dry += 1;
                    break;
                }
                let (l0, r0, l1, r1) = (q[0], q[1], q[2], q[3]);
                let gain_goal = gain_tgt * src_gain * vol_g;
                gain_cur = approach(gain_cur, gain_goal, 0.0008);
                let t = frac as f32;
                // kesirli konumda (hız farkı/saat kayması) kübik, yoksa birebir örnek: bit-perfect yol bozulmaz
                let (dl, dr) = if t == 0.0 {
                    (l0, r0)
                } else if q.len() >= 6 {
                    (catmull_rom(prev.0, l0, l1, q[4], t), catmull_rom(prev.1, r0, r1, q[5], t))
                } else {
                    (l0 + (l1 - l0) * t, r0 + (r1 - r0) * t)
                };
                let (wl, wr) = eq.process_frame(dl, dr);
                let (wl, wr) = match old_eq.as_mut() {
                    Some((o, rem)) => {
                        let (ol, or_) = o.process_frame(dl, dr);
                        let t = *rem as f32 / xf_len as f32;
                        *rem = rem.saturating_sub(1);
                        (wl * (1.0 - t) + ol * t, wr * (1.0 - t) + or_ * t)
                    }
                    None => (wl, wr),
                };
                if matches!(old_eq, Some((_, 0))) {
                    old_eq = None;
                }
                mix_cur = approach(mix_cur, mix_tgt, 0.004);
                let mut l = wl * mix_cur + dl * dry_gain * (1.0 - mix_cur);
                let mut r = wr * mix_cur + dr * dry_gain * (1.0 - mix_cur);
                solo_cur += (solo_target - solo_cur) * 0.002;
                if !chain.is_empty() || old_chain.is_some() {
                    let (cl, cr) = chain.process(l, r);
                    let (cl, cr) = match old_chain.as_mut() {
                        Some((o, rem)) => {
                            let (ol, or_) = o.process(l, r);
                            let t = *rem as f32 / xf_len as f32;
                            *rem = rem.saturating_sub(1);
                            (cl * (1.0 - t) + ol * t, cr * (1.0 - t) + or_ * t)
                        }
                        None => (cl, cr),
                    };
                    l = cl;
                    r = cr;
                    if matches!(old_chain, Some((_, 0))) {
                        old_chain = None;
                    }
                }
                if cf_amt > 0.0 {
                    let (a, b) = cf.process(l, r, cf_amt);
                    l = a;
                    r = b;
                }
                if mono {
                    let m = (l + r) * 0.5;
                    l = m;
                    r = m;
                }
                // Solo yalnızca dinlenen sesi değiştirir: karışım zincirden SONRA yapılır ve analizör (grafik arka planı) solosuz
                // sinyali (vl, vr) görmeye devam eder; yani solo açılınca grafik değişmez.
                let (mut vl, mut vr) = (l, r);
                if solo_cur > 0.0005 {
                    // EQ yerine, ham (EQ'suz) sinyalin yalnızca seçili bölgesi
                    let (sl, sr) = (solo.process(0, dl) * SOLO_MAKEUP, solo.process(1, dr) * SOLO_MAKEUP);
                    // zincir (limiter) atlandığı için kırpılma güvenliği: tam ölçeğe sıkıştır
                    l = (l * (1.0 - solo_cur) + sl * solo_cur).clamp(-1.0, 1.0);
                    r = (r * (1.0 - solo_cur) + sr * solo_cur).clamp(-1.0, 1.0);
                }
                let (k_l, k_r) = (bal_l * gain_cur * gate_cur, bal_r * gain_cur * gate_cur);
                l *= k_l;
                r *= k_r;
                vl *= k_l;
                vr *= k_r;
                if meter.feed(l, r) {
                    let put = |a: &std::sync::atomic::AtomicU32, v: Option<f32>| a.store(v.unwrap_or(-200.0).to_bits(), Ordering::Relaxed);
                    put(&ctl.m_lufs, meter.momentary());
                    put(&ctl.s_lufs, meter.short_term());
                    put(&ctl.i_lufs, meter.integrated());
                    put(&ctl.tp_db, meter.true_peak_db());
                }
                pk_l = pk_l.max(l.abs());
                pk_r = pk_r.max(r.abs());
                if l.abs() >= 0.999 || r.abs() >= 0.999 {
                    clips += 1;
                }
                vbuf.push(vl);
                vbuf.push(vr);
                let o = &mut out[i * fb..];
                let dith_on = needs_dither(dither_on, chain_touched, gain_cur, gate_cur);
                let (dl_, dr_) = if dith_on { (dith[0].tpdf(), dith[1].tpdf()) } else { (0.0, 0.0) };
                // kanıt: yumuşak geçişler dışında, yazılan değer halkadan gelen örnekle aynı mı?
                if gate_cur < 1.0 || gain_cur != gain_goal {
                    pf.ramp += 1;
                } else if crate::proof::frame_exact(dl, dr, l, r, t == 0.0, dith_on) {
                    pf.ok += 1;
                } else {
                    pf.alt += 1;
                    pf.causes |= crate::proof::causes(t == 0.0, (gain_cur - 1.0).abs() > 1e-6 || bal_l != 1.0 || bal_r != 1.0, chain_touched, dith_on, l.abs() > 1.0 || r.abs() > 1.0);
                }
                write_sample(o, l, &fmt, dl_);
                if fmt.channels > 1 {
                    write_sample(&mut o[sb..], r, &fmt, dr_);
                }
                frac += step;
                while frac >= 1.0 && q.len() >= 4 {
                    prev = (q[0], q[1]);
                    q.pop_front();
                    q.pop_front();
                    frac -= 1.0;
                }
            }
        }
        drop(q);
        if !primed && gate_tgt == 0.0 && !muted_flag {
            muted_flag = true;
            ring.muted.store(true, Ordering::SeqCst);
        }
        if !vbuf.is_empty() {
            let mut v = ctl.viz.lock().unwrap();
            v.extend(vbuf.drain(..));
            while v.len() > 16384 {
                v.pop_front();
            }
            ctl.viz_seq.fetch_add(1, Ordering::Relaxed);
        }
        ctl.peak_l.fetch_max(pk_l.to_bits(), Ordering::Relaxed);
        ctl.peak_r.fetch_max(pk_r.to_bits(), Ordering::Relaxed);
        if clips > 0 {
            ctl.clips.fetch_add(clips, Ordering::Relaxed);
        }
        ctl.proof.add(&pf);
        render.ReleaseBuffer(n, 0)?;
    }
    rs.client.Stop()?;
    Ok(())
}


// ------------------------------------------------------------------ DoP (DSD over PCM)

/// DoP'ta denenen biçimler (container, geçerli bit): 24 bitlik DoP sözcüğü sola yaslı yazılır.
const DOP_FORMATS: [(u16, u16); 3] = [(32, 24), (24, 24), (32, 32)];

/// DoP sözcüğünü (24 bit) biçime göre bayt dizisine yazar. Sola yaslı: 32 bit kapta üst 24 bit.
fn write_dop(b: &mut [u8], word: u32, f: &Fmt) {
    if f.container_bits == 32 {
        b[..4].copy_from_slice(&(word << 8).to_le_bytes());
    } else {
        b[..3].copy_from_slice(&word.to_le_bytes()[..3]);
    }
}

/// `n` çerçeve DoP yazar: işaret baytı çerçeve başına dönüşümlü, (sol, sağ) 16 bitlik DSD değerleri `next`'ten gelir.
fn fill_dop(out: &mut [u8], n: usize, f: &Fmt, odd: &mut bool, mut next: impl FnMut() -> (u16, u16)) {
    let (fb, sb) = (f.frame_bytes(), f.container_bits as usize / 8);
    for i in 0..n {
        let (l, r) = next();
        write_dop(&mut out[i * fb..], crate::dsd::dop_word(*odd, l), f);
        write_dop(&mut out[i * fb + sb..], crate::dsd::dop_word(*odd, r), f);
        *odd = !*odd;
    }
}

unsafe fn init_render_dop(dev: &IMMDevice, rate: u32) -> Result<RenderStream> {
    let probe: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
    for &(container, valid) in &DOP_FORMATS {
        let f = Fmt { rate, channels: 2, container_bits: container, valid_bits: valid, float: false, mask: 3 };
        let w = f.to_wfx();
        if probe.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, &w as *const _ as *const WAVEFORMATEX, None).0 != 0 {
            continue;
        }
        if let Ok(client) = try_init_exclusive(dev, &f) {
            return Ok(RenderStream { client, fmt: f, exclusive: true });
        }
    }
    bail!("Cihaz {rate} Hz / 24 bit exclusive biçimini kabul etmiyor (DoP için gerekli)")
}

/// DoP için denenen PCM hızları: 44.1 ailesinde DSD64/128/256 = 176.4/352.8/705.6 kHz, 48 ailesinde 192/384/768 kHz.
pub const DOP_RATES: &[u32] = &[176_400, 192_000, 352_800, 384_000, 705_600, 768_000];

/// Cihazın DoP için gereken biçimi (24 bit, exclusive) kabul ettiği hızlar. En düşük DoP hızı 176.4 kHz olduğundan cihaz
/// 96 kHz'e kadar destekliyorsa liste boş döner (DoP imkânsız). Cihazın DoP'u *anladığını* göstermez, yalnızca biçimi kabul ettiğini.
pub fn dop_rates(dev_id: &str) -> Vec<u32> {
    let id = dev_id.to_string();
    std::thread::spawn(move || -> Vec<u32> {
        if com_init().is_err() {
            return vec![];
        }
        unsafe {
            let Ok(dev) = open(&id) else { return vec![] };
            let Ok(probe) = dev.Activate::<IAudioClient>(CLSCTX_ALL, None) else { return vec![] };
            DOP_RATES
                .iter()
                .copied()
                .filter(|&rate| {
                    DOP_FORMATS.iter().any(|&(container, valid)| {
                        let f = Fmt { rate, channels: 2, container_bits: container, valid_bits: valid, float: false, mask: 3 };
                        let w = f.to_wfx();
                        probe.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, &w as *const _ as *const WAVEFORMATEX, None).0 == 0
                    })
                })
                .collect()
        }
    })
    .join()
    .unwrap_or_default()
}

/// DAC'a `secs` saniye boyunca yalnızca DoP **sessizliği** (DSD sessizlik deseni) gönderir: müzik içermez. DAC DoP'u
/// tanıyorsa DSD moduna geçer (ekranında/ışığında görünür); tanımıyorsa yalnızca çok kısık bir ton duyulabilir.
/// Çağıran, aynı cihazı kullanan motoru önce durdurmalıdır.
pub fn dop_probe(dev_id: &str, rate: u32, secs: f32) -> Result<()> {
    com_init()?;
    unsafe {
        let dev = open(dev_id)?;
        let rs = init_render_dop(&dev, rate)?;
        let event: HANDLE = CreateEventW(None, false, false, None)?;
        rs.client.SetEventHandle(event)?;
        let render: IAudioRenderClient = rs.client.GetService()?;
        let buf_frames = rs.client.GetBufferSize()?;
        let fmt = rs.fmt;
        let mut odd = false;
        let silence = (crate::dsd::DSD_SILENCE as u16) << 8 | crate::dsd::DSD_SILENCE as u16;
        let p = render.GetBuffer(buf_frames)?;
        fill_dop(std::slice::from_raw_parts_mut(p, buf_frames as usize * fmt.frame_bytes()), buf_frames as usize, &fmt, &mut odd, || (silence, silence));
        render.ReleaseBuffer(buf_frames, 0)?;
        rs.client.Start()?;
        let t0 = std::time::Instant::now();
        while t0.elapsed().as_secs_f32() < secs {
            if WaitForSingleObject(event, 200) != WAIT_OBJECT_0 {
                continue;
            }
            let p = render.GetBuffer(buf_frames)?;
            fill_dop(std::slice::from_raw_parts_mut(p, buf_frames as usize * fmt.frame_bytes()), buf_frames as usize, &fmt, &mut odd, || (silence, silence));
            render.ReleaseBuffer(buf_frames, 0)?;
        }
        rs.client.Stop()?;
    }
    Ok(())
}

/// DoP render döngüsü: halka yalnızca 16 bitlik DSD değerleri (f32 içinde tam sayı) taşır; işaret baytını burada ekleriz,
/// böylece duraklatma / arabellek boşalması / parça geçişinde DSD sessizliği (0x69) gönderilir ve işaret düzeni hiç bozulmaz.
/// Ses seviyesi, EQ, dither, modüller ve ölçümler uygulanmaz (bit-exact).
unsafe fn render_loop_dop(ctl: &Control, dst_id: &str, dst_name: &str, rate: u32, ring: &Arc<Ring>) -> Result<()> {
    let dev = open(dst_id)?;
    let mut tries = 0;
    let rs = loop {
        match init_render_dop(&dev, rate) {
            Ok(r) => break r,
            Err(err) if is_in_use(&err) && tries < 4 => {
                tries += 1;
                ctl.say("Cihaz meşgul, yeniden deneniyor…");
                std::thread::sleep(Duration::from_millis(600));
            }
            Err(err) => return Err(err),
        }
    };
    let fmt = rs.fmt;
    ctl.say(format!("Çıkış modu       : EXCLUSIVE DoP | {} Hz, {} bit kap", fmt.rate, fmt.container_bits));
    *ctl.out_info.lock().unwrap() =
        OutInfo { exclusive: true, rate: fmt.rate, bits: 24, float: false, device: dst_name.to_string(), dop: true };

    let event: HANDLE = CreateEventW(None, false, false, None)?;
    rs.client.SetEventHandle(event)?;
    let render: IAudioRenderClient = rs.client.GetService()?;
    let buf_frames = rs.client.GetBufferSize()? as usize;
    let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
    let silence = (crate::dsd::DSD_SILENCE as u16) << 8 | crate::dsd::DSD_SILENCE as u16;
    let mut odd = false;
    // ilk arabellek DSD sessizliği (sıfır PCM değil: DAC DoP kilidini kaybetmesin)
    let p = render.GetBuffer(buf_frames as u32)?;
    fill_dop(std::slice::from_raw_parts_mut(p, buf_frames * fmt.frame_bytes()), buf_frames, &fmt, &mut odd, || (silence, silence));
    render.ReleaseBuffer(buf_frames as u32, 0)?;
    rs.client.Start()?;

    let target = (rate as usize * 40 / 1000).max(buf_frames * 2);
    let mut primed = false;
    while !ctl.stopped() {
        if WaitForSingleObject(event, 200) != WAIT_OBJECT_0 {
            continue;
        }
        let p = render.GetBuffer(buf_frames as u32)?;
        let out = std::slice::from_raw_parts_mut(p, buf_frames * fmt.frame_bytes());
        let open = ring.gate.load(Ordering::Relaxed) && !ring.hold.load(Ordering::Relaxed);
        let mut q = ring.q.lock().unwrap();
        if !primed && open && q.len() / 2 >= target {
            primed = true;
        }
        let mut sent_data = false;
        fill_dop(out, buf_frames, &fmt, &mut odd, || {
            if primed && open && q.len() >= 2 {
                let (l, r) = (q.pop_front().unwrap_or(0.0), q.pop_front().unwrap_or(0.0));
                sent_data = true;
                (l as u16, r as u16)
            } else {
                primed = false; // boşaldı ya da duraklatıldı: yeniden dolana kadar sessizlik
                (silence, silence)
            }
        });
        drop(q);
        ring.muted.store(!sent_data, Ordering::SeqCst);
        render.ReleaseBuffer(buf_frames as u32, 0)?;
    }
    rs.client.Stop()?;
    Ok(())
}

pub fn install_ctrlc() {
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::System::Console::SetConsoleCtrlHandler;
    unsafe extern "system" fn handler(_: u32) -> BOOL {
        STOP.store(true, Ordering::Relaxed);
        BOOL(1)
    }
    unsafe {
        let _ = SetConsoleCtrlHandler(Some(handler), true);
    }
}

/// Bir cihazın loopback'ini `secs` saniye dinleyip RMS (dBFS, sol kanal) döndürür.
pub fn measure(dev_query: Option<&str>, secs: u64) -> Result<f64> {
    com_init()?;
    let d = resolve(dev_query)?;
    unsafe {
        let dev = open(&d.id)?;
        let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
        let mix = client.GetMixFormat()?;
        let fmt = Fmt::from_ptr(mix);
        client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 2_000_000, 0, mix, None)?;
        CoTaskMemFree(Some(mix as _));
        let cap: IAudioCaptureClient = client.GetService()?;
        client.Start()?;
        let (ch, sb) = (fmt.channels as usize, fmt.container_bits as usize / 8);
        let (mut sum, mut cnt) = (0f64, 0u64);
        let (mut pk, mut sil) = (0u32, 0u32);
        let t0 = std::time::Instant::now();
        while t0.elapsed().as_secs() < secs {
            std::thread::sleep(Duration::from_millis(5));
            while cap.GetNextPacketSize()? > 0 {
                let mut data: *mut u8 = std::ptr::null_mut();
                let (mut frames, mut flags) = (0u32, 0u32);
                cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                pk += 1;
                if flags & 2 != 0 {
                    sil += 1;
                }
                if flags & 2 == 0 && t0.elapsed().as_millis() > 800 {
                    let bytes = std::slice::from_raw_parts(data, frames as usize * ch * sb);
                    for i in 0..frames as usize {
                        let v = read_sample(&bytes[i * ch * sb..], &fmt) as f64;
                        sum += v * v;
                        cnt += 1;
                    }
                }
                cap.ReleaseBuffer(frames)?;
            }
        }
        client.Stop()?;
        if cnt == 0 {
            bail!("hiç ses yakalanmadı (paket {pk}, sessiz {sil})");
        }
        Ok(10.0 * (sum / cnt as f64).max(1e-20).log10())
    }
}


/// Basit kulaklık crossfeed'i: karşı kanalın alçak geçirilmiş, hafif geciktirilmiş kısmı.
struct Crossfeed {
    dl: Vec<f32>,
    dr: Vec<f32>,
    pos: usize,
    lp_l: f32,
    lp_r: f32,
    a: f32,
}

impl Crossfeed {
    fn new(rate: f32) -> Crossfeed {
        let n = ((0.00027 * rate) as usize).max(1);
        Crossfeed {
            dl: vec![0.0; n],
            dr: vec![0.0; n],
            pos: 0,
            lp_l: 0.0,
            lp_r: 0.0,
            a: 1.0 - (-2.0 * std::f32::consts::PI * 700.0 / rate).exp(),
        }
    }

    fn process(&mut self, l: f32, r: f32, amt: f32) -> (f32, f32) {
        let (old_l, old_r) = (self.dl[self.pos], self.dr[self.pos]);
        self.dl[self.pos] = l;
        self.dr[self.pos] = r;
        self.pos = (self.pos + 1) % self.dl.len();
        self.lp_l += self.a * (old_l - self.lp_l);
        self.lp_r += self.a * (old_r - self.lp_r);
        let k = 0.5 * amt;
        let norm = 1.0 / (1.0 + k * 0.6);
        ((l + k * self.lp_r) * norm, (r + k * self.lp_l) * norm)
    }
}


// ------------------------------------------------- sanal cihaz ve varsayılan çıkış

#[windows::core::interface("F8679F50-850A-41CF-9C72-430F290290C8")]
unsafe trait IPolicyConfig: windows::core::IUnknown {
    fn GetMixFormat(&self, a: PCWSTR, b: *mut *mut WAVEFORMATEX) -> windows::core::HRESULT;
    fn GetDeviceFormat(&self, a: PCWSTR, b: i32, c: *mut *mut WAVEFORMATEX) -> windows::core::HRESULT;
    fn ResetDeviceFormat(&self, a: PCWSTR) -> windows::core::HRESULT;
    fn SetDeviceFormat(&self, a: PCWSTR, b: *mut WAVEFORMATEX, c: *mut WAVEFORMATEX) -> windows::core::HRESULT;
    fn GetProcessingPeriod(&self, a: PCWSTR, b: i32, c: *mut i64, d: *mut i64) -> windows::core::HRESULT;
    fn SetProcessingPeriod(&self, a: PCWSTR, b: *mut i64) -> windows::core::HRESULT;
    fn GetShareMode(&self, a: PCWSTR, b: *mut core::ffi::c_void) -> windows::core::HRESULT;
    fn SetShareMode(&self, a: PCWSTR, b: *mut core::ffi::c_void) -> windows::core::HRESULT;
    fn GetPropertyValue(&self, a: PCWSTR, b: i32, c: *const core::ffi::c_void, d: *mut core::ffi::c_void) -> windows::core::HRESULT;
    fn SetPropertyValue(&self, a: PCWSTR, b: i32, c: *const core::ffi::c_void, d: *mut core::ffi::c_void) -> windows::core::HRESULT;
    fn SetDefaultEndpoint(&self, a: PCWSTR, role: ERole) -> windows::core::HRESULT;
    fn SetEndpointVisibility(&self, a: PCWSTR, b: i32) -> windows::core::HRESULT;
}

const VIRTUAL_HINTS: &[&str] = &["cable input", "vb-audio", "voicemeeter", "virtual", "soundworks", "hi-fi cable"];

/// Bilinen sanal ses kablosu/cihazı (render ucu) varsa döndürür.
pub fn find_virtual() -> Option<DeviceInfo> {
    let devs = list_devices().ok()?;
    let is_virtual = |d: &DeviceInfo| {
        let n = d.name.to_lowercase();
        VIRTUAL_HINTS.iter().any(|h| n.contains(h))
    };
    // önce klasik 2 kanallı kablo, sonra diğer sanal cihazlar (16 kanallı sürüm en sona)
    let rank = |d: &DeviceInfo| {
        let n = d.name.to_lowercase();
        if n.contains("cable input") {
            0
        } else if n.contains("16ch") {
            2
        } else {
            1
        }
    };
    devs.into_iter().filter(|d| is_virtual(d)).min_by_key(|d| rank(d))
}

pub fn default_render_id() -> Result<String> {
    unsafe {
        let en = enumerator()?;
        device_id(&en.GetDefaultAudioEndpoint(eRender, eConsole)?)
    }
}

/// Windows varsayılan çıkış cihazını (tüm roller) değiştirir.
pub fn set_default_render(id: &str) -> Result<()> {
    unsafe {
        let policy: IPolicyConfig =
            CoCreateInstance(&GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9), None, CLSCTX_ALL)
                .context("IPolicyConfig oluşturulamadı")?;
        let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
        for role in [eConsole, eMultimedia, eCommunications] {
            policy.SetDefaultEndpoint(PCWSTR(wide.as_ptr()), role).ok().context("varsayılan cihaz değiştirilemedi")?;
        }
        Ok(())
    }
}


/// Hata ayıklama: her çıkış cihazının endpoint ses durumunu yazdır.
pub fn volumes() -> Result<()> {
    com_init()?;
    unsafe {
        let en = enumerator()?;
        let coll = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        for i in 0..coll.GetCount()? {
            let d = coll.Item(i)?;
            let v: IAudioEndpointVolume = d.Activate(CLSCTX_ALL, None)?;
            let hw = v.QueryHardwareSupport().unwrap_or(0);
            let mute = v.GetMute().map(|m| m.as_bool()).unwrap_or(false);
            let db = v.GetMasterVolumeLevel().unwrap_or(-999.0);
            let sc = v.GetMasterVolumeLevelScalar().unwrap_or(-1.0);
            println!("{:<50} hw={} mute={} {:.1} dB scalar={:.2}", device_name(&d)?, hw, mute, db, sc);
        }
    }
    Ok(())
}


/// Hata ayıklama: bir çıkış cihazının Windows mute durumunu ayarla.
pub fn set_mute(query: &str, mute: bool) -> Result<()> {
    com_init()?;
    let d = resolve(Some(query))?;
    unsafe {
        let dev = open(&d.id)?;
        let v: IAudioEndpointVolume = dev.Activate(CLSCTX_ALL, None)?;
        v.SetMute(mute, std::ptr::null())?;
    }
    Ok(())
}


fn hresult_of(e: &anyhow::Error) -> Option<u32> {
    e.chain().find_map(|c| c.downcast_ref::<windows::core::Error>()).map(|w| w.code().0 as u32)
}

/// Hata, çıkış cihazının kaybolduğu türden mi (bağlantı koptu, cihaz çıkarıldı/değişti, bulunamadı)?
pub fn is_gone(e: &anyhow::Error) -> bool {
    matches!(hresult_of(e), Some(0x88890004) | Some(0x88890026)) || crate::recover::text_means_gone(&format!("{e:#}"))
}

pub fn is_in_use(e: &anyhow::Error) -> bool {
    hresult_of(e) == Some(0x8889000A)
}

/// WASAPI hata kodlarını anlaşılır Türkçe'ye çevir.
pub fn explain_error(e: &anyhow::Error) -> String {
    match hresult_of(e) {
        Some(0x8889000A) => "Cihaz başka bir uygulamada özel (exclusive) kullanımda. Tepsideki başka bir Soundworks ya da cihazı kilitleyen bir oynatıcıyı kapat. (0x8889000A)".to_string(),
        Some(0x88890004) | Some(0x88890026) => "Çıkış cihazı çıkarıldı ya da değişti. Cihazı seçip yeniden başlat.".to_string(),
        Some(0x88890008) => "Cihaz bu ses biçimini desteklemiyor. Ayarlar'dan çıkış hızını değiştir.".to_string(),
        Some(0x88890003) => "Cihaz ses motoru tarafından kapatıldı. Yeniden başlat. (0x88890003)".to_string(),
        Some(0x8889000F) | Some(0x88890021) => "Exclusive kullanım izni yok. Windows Ses Ayarları > cihaz özellikleri > Gelişmiş'te 'uygulamaların özel denetim almasına izin ver' açık olmalı.".to_string(),
        Some(c) => format!("{e:#} (0x{c:08X})"),
        None => format!("{e:#}"),
    }
}

#[cfg(test)]
mod dsp_tests {
    use super::*;

    #[test]
    fn player_mode_never_nudges_the_read_step() {
        // dahili oynatıcıda halka hedefin kat kat üstünde dolu olsa bile adım tam 1.0 kalır (aksi halde ara değer = bit-perfect değil)
        for ratio in [-1.0, -0.4, 0.0, 0.4, 2.75, 10.0] {
            assert_eq!(read_step(false, 1.0, ratio), 1.0, "doluluk sapması {ratio}");
        }
        // loopback'te küçük sapma sabit, büyük sapma telafi edilir
        assert_eq!(read_step(true, 1.0, 0.3), 1.0);
        assert!(read_step(true, 1.0, 2.75) > 1.0);
        assert!(read_step(true, 1.0, -2.0) < 1.0);
    }

    fn fmt16() -> Fmt {
        Fmt { rate: 48000, channels: 2, container_bits: 16, valid_bits: 16, float: false, mask: 3 }
    }

    fn pcm16(f: &Fmt, s: f32, d: f32) -> i16 {
        let mut b = [0u8; 2];
        write_sample(&mut b, s, f, d);
        i16::from_le_bytes(b)
    }

    #[test]
    fn undithered_write_is_exact_for_representable_samples() {
        let f = fmt16();
        for v in [-32768i32, -12345, -1, 0, 1, 777, 32767] {
            let s = v as f32 / 32767.0;
            assert_eq!(pcm16(&f, s, 0.0) as i32, v.max(-32767).min(32767), "v={v}");
        }
    }

    #[test]
    fn tpdf_is_bounded_zero_mean_and_triangular() {
        let mut d = Dither::new(12345);
        let n = 400_000;
        let (mut sum, mut sq, mut mx) = (0f64, 0f64, 0f32);
        for _ in 0..n {
            let x = d.tpdf();
            sum += x as f64;
            sq += (x * x) as f64;
            mx = mx.max(x.abs());
        }
        assert!(mx <= 1.0);
        assert!((sum / n as f64).abs() < 0.01, "ortalama {}", sum / n as f64);
        // üçgensel dağılımın varyansı 1/6
        assert!((sq / n as f64 - 1.0 / 6.0).abs() < 0.01, "varyans {}", sq / n as f64);
    }

    #[test]
    fn dither_preserves_sub_lsb_signal_on_average() {
        // 0.25 LSB'lik DC: dither'sız her örnek 0'a yuvarlanır (bilgi kaybolur), dither'lı ortalama 0.25'e yaklaşır
        let f = fmt16();
        let s = 0.25f32 / 32767.0;
        assert_eq!(pcm16(&f, s, 0.0), 0);
        let mut d = Dither::new(987);
        let n = 200_000;
        let mean: f64 = (0..n).map(|_| pcm16(&f, s, d.tpdf()) as f64).sum::<f64>() / n as f64;
        assert!((mean - 0.25).abs() < 0.02, "ortalama {mean}");
    }

    #[test]
    fn dithered_output_never_overflows() {
        let f = fmt16();
        let mut d = Dither::new(5);
        for _ in 0..10_000 {
            let _ = pcm16(&f, 1.0, d.tpdf());
            let _ = pcm16(&f, -1.0, d.tpdf());
        }
    }

    #[test]
    fn cubic_interpolation_is_exact_at_nodes_and_beats_linear() {
        assert_eq!(catmull_rom(0.1, 0.2, 0.3, 0.4, 0.0), 0.2);
        assert!((catmull_rom(0.1, 0.2, 0.3, 0.4, 1.0) - 0.3).abs() < 1e-7);
        // 5 kHz sinüsü 48 kHz'den kesirli konumlarda yeniden örnekle: kübik hata doğrusaldan çok küçük
        let fs = 48000.0f64;
        let f = 5000.0f64;
        let s = |n: f64| (2.0 * std::f64::consts::PI * f * n / fs).sin() as f32;
        let (mut e_lin, mut e_cub) = (0f64, 0f64);
        let mut cnt = 0;
        for n in 10..2000 {
            for k in 1..10 {
                let t = k as f32 / 10.0;
                let truth = s(n as f64 + t as f64) as f64;
                let lin = (s(n as f64) + (s(n as f64 + 1.0) - s(n as f64)) * t) as f64;
                let cub = catmull_rom(s(n as f64 - 1.0), s(n as f64), s(n as f64 + 1.0), s(n as f64 + 2.0), t) as f64;
                e_lin += (lin - truth).powi(2);
                e_cub += (cub - truth).powi(2);
                cnt += 1;
            }
        }
        let (r_lin, r_cub) = ((e_lin / cnt as f64).sqrt(), (e_cub / cnt as f64).sqrt());
        assert!(r_cub < r_lin / 8.0, "kübik {r_cub} vs doğrusal {r_lin}");
    }


    #[test]
    fn gain_smoothing_reaches_unity_exactly() {
        let mut g = 0f32;
        for _ in 0..2_000_000 {
            g = approach(g, 1.0, 0.0008);
        }
        assert_eq!(g, 1.0);
        // 24 bit tam ölçek örnek değişmeden geçer
        let s = 8388607f32 / 8388608.0;
        assert_eq!((s * g * 8388608.0).round(), 8388607.0);
        // kıs ve geri aç
        let mut m = 1f32;
        for _ in 0..200_000 {
            m = approach(m, 0.0, 0.004);
        }
        assert!(m < 1e-6);
        for _ in 0..200_000 {
            m = approach(m, 1.0, 0.004);
        }
        assert_eq!(m, 1.0);
        assert!(!needs_dither(true, false, g, 1.0));
    }


    #[test]
    fn dop_samples_are_written_left_aligned_with_alternating_markers() {
        let f32c = Fmt { rate: 176400, channels: 2, container_bits: 32, valid_bits: 24, float: false, mask: 3 };
        let f24c = Fmt { rate: 176400, channels: 2, container_bits: 24, valid_bits: 24, float: false, mask: 3 };
        let mut odd = false;
        let mut buf = vec![0u8; 4 * 2 * 2];
        let data = [(0x1234u16, 0xABCDu16), (0xFFFF, 0x0001)];
        let mut it = data.iter();
        fill_dop(&mut buf, 2, &f32c, &mut odd, || *it.next().unwrap());
        // çerçeve 0 (işaret 0x05): 0x051234 << 8 ; çerçeve 1 (işaret 0xFA): 0xFAFFFF << 8
        assert_eq!(&buf[0..4], &(0x0512_3400u32).to_le_bytes());
        assert_eq!(&buf[4..8], &(0x05AB_CD00u32).to_le_bytes());
        assert_eq!(&buf[8..12], &(0xFAFF_FF00u32).to_le_bytes());
        assert_eq!(&buf[12..16], &(0xFA00_0100u32).to_le_bytes());
        assert!(odd == false); // iki çerçeve sonra başa döner
        // 24 bit kapta üç bayt
        let mut b24 = vec![0u8; 3 * 2];
        let mut o2 = true;
        fill_dop(&mut b24, 1, &f24c, &mut o2, || (0x6969, 0x6969));
        assert_eq!(b24, vec![0x69, 0x69, 0xFA, 0x69, 0x69, 0xFA]);
        // işaret düzeni çağrılar arasında korunur
        let mut o3 = false;
        let mut x = vec![0u8; 8];
        for _ in 0..3 {
            fill_dop(&mut x, 1, &f32c, &mut o3, || (0, 0));
        }
        assert!(o3); // 3 çerçeve: tek
    }

    #[test]
    fn dither_decision() {
        // bit-perfect yol: ses %100, EQ/DSP yok, kapı açık -> dither yok
        assert!(!needs_dither(true, false, 1.0, 1.0));
        // ses kısık, EQ açık, fade sırasında -> var
        assert!(needs_dither(true, false, 0.64, 1.0));
        assert!(needs_dither(true, true, 1.0, 1.0));
        assert!(needs_dither(true, false, 1.0, 0.5));
        // ayar kapalı -> hiç
        assert!(!needs_dither(false, true, 0.5, 0.5));
    }
}

#[cfg(test)]
mod device_tests {
    use super::*;

    /// Gerçek cihaz yoklaması (ses çalmaz, yalnızca biçim sorar): `SW_PROBE_ID=<cihaz kimliği> cargo test probe_real -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn probe_real_device() {
        com_init().unwrap();
        for d in list_devices().unwrap() {
            println!("cihaz: {} | {:?} | varsayılan={} | {}", d.name, d.kind, d.is_default, d.id);
        }
        let id = std::env::var("SW_PROBE_ID").expect("SW_PROBE_ID");
        let rates = supported_rates(&id);
        println!("exclusive kabul edilen hızlar: {rates:?}");
        println!("44.1 kHz parça için seçilen: {}", choose_rate(44100, &rates, 48000));
    }


    fn dev(id: &str, name: &str) -> DeviceInfo {
        DeviceInfo { id: id.into(), name: name.into(), is_default: false, kind: DeviceKind::Unknown }
    }

    #[test]
    fn device_lookup_prefers_id_then_exact_name() {
        let d = || vec![dev("{1}", "Speakers (Realtek)"), dev("{2}", "Speakers (Realtek) 2"), dev("{3}", "FiiO K5")];
        assert_eq!(pick_device(d(), "{2}").unwrap().id, "{2}");
        // tam ad, daha uzun addan önce gelir
        assert_eq!(pick_device(d(), "speakers (realtek)").unwrap().id, "{1}");
        assert_eq!(pick_device(d(), "fiio").unwrap().id, "{3}");
        assert!(pick_device(d(), "yok").is_none());
    }

    #[test]
    fn old_name_settings_migrate_to_ids() {
        let devs = vec![dev("{1}", "Speakers (Realtek)"), dev("{2}", "Speakers (Realtek) 2"), dev("{3}", "FiiO K5 Pro")];
        let mut s = Some("FiiO K5 Pro".to_string());
        assert!(migrate_device_ref(&devs, &mut s));
        assert_eq!(s.as_deref(), Some("{3}"));
        // zaten kimlik: dokunma
        assert!(!migrate_device_ref(&devs, &mut s));
        // belirsiz ad parçası ("speakers" iki cihaza uyar): olduğu gibi kalır
        let mut amb = Some("speakers".to_string());
        assert!(!migrate_device_ref(&devs, &mut amb));
        assert_eq!(amb.as_deref(), Some("speakers"));
        // bağlı olmayan cihaz
        let mut gone = Some("Başka".to_string());
        assert!(!migrate_device_ref(&devs, &mut gone));
        let mut none: Option<String> = None;
        assert!(!migrate_device_ref(&devs, &mut none));
    }

    #[test]
    fn kind_detection() {
        assert_eq!(kind_from("BTHENUM", None, "Kulaklıklar (MOMENTUM 4)"), DeviceKind::Bluetooth);
        assert_eq!(kind_from("BTHHFENUM", None, "Hands-Free AG Audio"), DeviceKind::Bluetooth);
        assert_eq!(kind_from("", None, "Headphones (WH-1000XM5 Bluetooth)"), DeviceKind::Bluetooth);
        assert_eq!(kind_from("USB", None, "Speakers (FiiO K5 Pro)"), DeviceKind::Usb);
        assert_eq!(kind_from("HDAUDIO", None, "Hoparlörler (Realtek)"), DeviceKind::Builtin);
        assert_eq!(kind_from("HDAUDIO", Some(9), "BenQ GW2480 (NVIDIA High Definition Audio)"), DeviceKind::Hdmi);
        assert_eq!(kind_from("ROOT", None, "CABLE Input (VB-Audio Virtual Cable)"), DeviceKind::Virtual);
        assert_eq!(kind_from("XYZ", None, "Bir cihaz"), DeviceKind::Unknown);
    }

    #[test]
    fn rate_choice() {
        let bt = [44100, 48000];
        assert_eq!(choose_rate(44100, &bt, 48000), 44100); // aynısı
        assert_eq!(choose_rate(96000, &bt, 48000), 48000); // aynı aile, desteklenen en yüksek altı
        assert_eq!(choose_rate(88200, &bt, 48000), 44100); // 44.1 ailesi
        assert_eq!(choose_rate(192000, &bt, 48000), 48000);
        let dac = [44100, 48000, 88200, 96000, 176400, 192000];
        assert_eq!(choose_rate(88200, &dac, 48000), 88200);
        assert_eq!(choose_rate(176400, &dac, 48000), 176400);
        // 44.1k ailesi: cihaz yalnız 48k ailesini destekliyorsa üstteki en yakın
        let only48 = [48000, 96000];
        assert_eq!(choose_rate(44100, &only48, 48000), 48000);
        assert_eq!(choose_rate(88200, &only48, 48000), 96000);
        // sorgulanamadı: sabit hıza düş
        assert_eq!(choose_rate(44100, &[], 48000), 48000);
    }
}
