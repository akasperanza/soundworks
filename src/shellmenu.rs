//! Explorer right-click entries for audio files, and the hand-off from a second process to the running app.
//!
//! The entries live under `HKCU\Software\Classes\SystemFileAssociations\.<ext>\shell\Soundworks`, so no admin rights
//! are needed and nothing outside the user's own hive is touched. Explorer starts `Soundworks.exe --add <file>` or
//! `--spectrum <file>`; that process drops a request file into the data directory and the running instance picks it up.
use std::path::{Path, PathBuf};
use windows::core::{w, PCWSTR};
use windows::Win32::System::Registry::*;

const ROOT: &str = "Software\\Classes\\SystemFileAssociations";

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Add,
    Spectrum,
}

pub struct Request {
    pub kind: Kind,
    pub paths: Vec<PathBuf>,
}

fn inbox_dir() -> PathBuf {
    crate::library::data_dir().join("inbox")
}

pub fn send(kind: Kind, paths: &[PathBuf]) -> std::io::Result<()> {
    let dir = inbox_dir();
    std::fs::create_dir_all(&dir)?;
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let mut body = String::from(if kind == Kind::Add { "add\n" } else { "spectrum\n" });
    for p in paths {
        body.push_str(&p.to_string_lossy());
        body.push('\n');
    }
    let name = format!("{nanos}-{}", std::process::id());
    let tmp = dir.join(format!("{name}.tmp"));
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, dir.join(format!("{name}.req")))
}

pub fn drain() -> Vec<Request> {
    let Ok(rd) = std::fs::read_dir(inbox_dir()) else { return vec![] };
    let mut files: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map_or(false, |e| e == "req")).collect();
    files.sort();
    let mut out = vec![];
    for f in files {
        let text = std::fs::read_to_string(&f);
        let _ = std::fs::remove_file(&f);
        let Ok(text) = text else { continue };
        let mut lines = text.lines();
        let kind = match lines.next() {
            Some("add") => Kind::Add,
            Some("spectrum") => Kind::Spectrum,
            _ => continue,
        };
        let paths: Vec<PathBuf> = lines.filter(|l| !l.is_empty()).map(PathBuf::from).collect();
        if !paths.is_empty() {
            out.push(Request { kind, paths });
        }
    }
    out
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

unsafe fn set_str(key: HKEY, name: PCWSTR, value: &str) {
    let v = wide(value);
    let bytes = std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 2);
    let _ = RegSetValueExW(key, name, 0, REG_SZ, Some(bytes));
}

unsafe fn create(path: &str) -> Option<HKEY> {
    let mut key = HKEY::default();
    let p = wide(path);
    RegCreateKeyExW(HKEY_CURRENT_USER, PCWSTR(p.as_ptr()), 0, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut key, None).is_ok().then_some(key)
}

fn ext_key(ext: &str) -> String {
    format!("{ROOT}\\.{ext}\\shell\\Soundworks")
}

pub fn register(exe: &Path) {
    let exe = exe.display().to_string();
    let labels = (crate::i18n::tr("Soundworks Kütüphanesine Ekle"), crate::i18n::tr("Spektrum Analizi"));
    unsafe {
        for ext in crate::player::AUDIO_EXTS {
            let base = ext_key(ext);
            if let Some(k) = create(&base) {
                set_str(k, w!("MUIVerb"), "Soundworks");
                set_str(k, w!("Icon"), &format!("\"{exe}\",0"));
                set_str(k, w!("SubCommands"), "");
                let _ = RegCloseKey(k);
            }
            for (id, label, flag) in [("1add", &labels.0, "--add"), ("2spectrum", &labels.1, "--spectrum")] {
                if let Some(k) = create(&format!("{base}\\shell\\{id}")) {
                    set_str(k, PCWSTR::null(), label);
                    let _ = RegCloseKey(k);
                }
                if let Some(k) = create(&format!("{base}\\shell\\{id}\\command")) {
                    set_str(k, PCWSTR::null(), &format!("\"{exe}\" {flag} \"%1\""));
                    let _ = RegCloseKey(k);
                }
            }
        }
    }
}

pub fn unregister() {
    for ext in crate::player::AUDIO_EXTS {
        let p = wide(&ext_key(ext));
        unsafe {
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(p.as_ptr()));
        }
    }
}

/// The executable the entries currently point at, if they are registered.
pub fn registered_exe() -> Option<PathBuf> {
    let sub = wide(&format!("{}\\shell\\1add\\command", ext_key(crate::player::AUDIO_EXTS[0])));
    let mut buf = vec![0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    unsafe {
        RegGetValueW(HKEY_CURRENT_USER, PCWSTR(sub.as_ptr()), PCWSTR::null(), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut len)).ok().ok()?;
    }
    let s = String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]);
    let rest = s.strip_prefix('"')?;
    Some(PathBuf::from(&rest[..rest.find('"')?]))
}

/// Keeps the entries pointing at a live executable: registers when missing or when the old target is gone,
/// but leaves alone a registration that points at another copy that still exists.
pub fn refresh(exe: &Path) {
    match registered_exe() {
        Some(old) if old != exe && old.exists() => {}
        Some(old) if old == exe => {}
        _ => register(exe),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip_through_the_inbox() {
        let a = PathBuf::from("C:\\Müzik\\a b.flac");
        send(Kind::Spectrum, &[a.clone()]).unwrap();
        send(Kind::Add, &[a.clone(), PathBuf::from("D:\\x.mp3")]).unwrap();
        let got = drain();
        let mine: Vec<_> = got.iter().filter(|r| r.paths.contains(&a)).collect();
        assert_eq!(mine.len(), 2);
        assert_eq!(mine[0].kind, Kind::Spectrum);
        assert_eq!(mine[1].paths.len(), 2);
    }

    #[test]
    #[ignore = "writes the real HKCU file-association keys"]
    fn registration_writes_and_removes_the_entries() {
        if registered_exe().is_some() {
            return;
        }
        let exe = Path::new("C:\\Program Files\\Soundworks\\Soundworks.exe");
        register(exe);
        let got = registered_exe();
        unregister();
        assert_eq!(got.as_deref(), Some(exe));
        assert!(registered_exe().is_none());
    }
}
