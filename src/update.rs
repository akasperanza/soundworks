use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub const REPO: &str = "akasperanza/soundworks";
pub const EXE_ASSET: &str = "Soundworks-windows-x64.exe";
pub const SUM_ASSET: &str = "Soundworks-windows-x64.exe.sha256";
const MIN_EXE_BYTES: u64 = 1 << 20;
const MAX_EXE_BYTES: u64 = 256 << 20;
const MAX_NOTES: usize = 4000;
const TRUSTED_HOSTS: [&str; 5] = ["github.com", "api.github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com", "github-releases.githubusercontent.com"];

pub fn api_url() -> String {
    format!("https://api.github.com/repos/{REPO}/releases/latest")
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub page: String,
    pub exe_url: String,
    pub sum_url: String,
    pub size: u64,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let core = s.split(['-', '+']).next()?;
    let mut it = core.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next().unwrap_or("0").parse().ok()?;
    let patch = it.next().unwrap_or("0").parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

pub fn parse_release(json: &str, current: &str) -> Result<Option<Release>, String> {
    let r: ApiRelease = serde_json::from_str(json).map_err(|e| format!("invalid release data: {e}"))?;
    if r.draft || r.prerelease || !is_newer(&r.tag_name, current) {
        return Ok(None);
    }
    let find = |name: &str| r.assets.iter().find(|a| a.name == name);
    let (Some(exe), Some(sum)) = (find(EXE_ASSET), find(SUM_ASSET)) else { return Ok(None) };
    let mut notes = r.body.unwrap_or_default();
    if notes.chars().count() > MAX_NOTES {
        notes = notes.chars().take(MAX_NOTES).collect::<String>() + "…";
    }
    Ok(Some(Release {
        version: r.tag_name.trim().trim_start_matches(['v', 'V']).to_string(),
        notes,
        page: r.html_url,
        exe_url: exe.browser_download_url.clone(),
        sum_url: sum.browser_download_url.clone(),
        size: exe.size,
    }))
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(timeout).user_agent(&format!("Soundworks/{}", env!("CARGO_PKG_VERSION"))).build()
}

pub fn check(url: &str, current: &str) -> Result<Option<Release>, String> {
    match agent(Duration::from_secs(15)).get(url).set("Accept", "application/vnd.github+json").call() {
        Ok(resp) => {
            let body = resp.into_string().map_err(|e| e.to_string())?;
            parse_release(&body, current)
        }
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(ureq::Error::Status(code, _)) => Err(format!("HTTP {code}")),
        Err(e) => Err(e.to_string()),
    }
}

pub fn trusted_host(url: &str) -> bool {
    let Ok(u) = url::Url::parse(url) else { return false };
    u.scheme() == "https" && u.host_str().map_or(false, |h| TRUSTED_HOSTS.contains(&h.to_lowercase().as_str()))
}

pub fn parse_checksum(text: &str) -> Option<String> {
    let first = text.split_whitespace().next()?.to_lowercase();
    (first.len() == 64 && first.chars().all(|c| c.is_ascii_hexdigit())).then_some(first)
}

pub fn download(rel: &Release, dir: &Path, progress: &AtomicU64, allow: &dyn Fn(&str) -> bool) -> Result<PathBuf, String> {
    if !allow(&rel.exe_url) || !allow(&rel.sum_url) {
        return Err("the update is not hosted on GitHub".into());
    }
    let sum_text = agent(Duration::from_secs(20)).get(&rel.sum_url).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
    let expected = parse_checksum(&sum_text).ok_or("the checksum file is invalid")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let part = dir.join("Soundworks-update.exe.part");
    let done = dir.join("Soundworks-update.exe");
    let _ = std::fs::remove_file(&part);
    let _ = std::fs::remove_file(&done);
    let resp = agent(Duration::from_secs(900)).get(&rel.exe_url).call().map_err(|e| e.to_string())?;
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buf = vec![0u8; 128 * 1024];
    let mut head = [0u8; 2];
    loop {
        let n = reader.read(&mut buf).map_err(|e| {
            let _ = std::fs::remove_file(&part);
            e.to_string()
        })?;
        if n == 0 {
            break;
        }
        if total == 0 && n >= 2 {
            head.copy_from_slice(&buf[..2]);
        }
        total += n as u64;
        if total > MAX_EXE_BYTES {
            let _ = std::fs::remove_file(&part);
            return Err("the update file is too large".into());
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        progress.store(total, Ordering::Relaxed);
    }
    drop(file);
    let actual: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if actual != expected {
        let _ = std::fs::remove_file(&part);
        return Err("the downloaded file does not match its checksum".into());
    }
    if total < MIN_EXE_BYTES || &head != b"MZ" {
        let _ = std::fs::remove_file(&part);
        return Err("the downloaded file is not a Windows program".into());
    }
    std::fs::rename(&part, &done).map_err(|e| e.to_string())?;
    Ok(done)
}

pub fn old_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".old");
    target.with_file_name(name)
}

pub fn install(new_exe: &Path, target: &Path) -> Result<(), String> {
    let old = old_path(target);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(target, &old).map_err(|e| format!("could not move the current program aside: {e}"))?;
    let moved = std::fs::rename(new_exe, target).or_else(|_| std::fs::copy(new_exe, target).map(|_| ()).and_then(|_| std::fs::remove_file(new_exe)));
    if let Err(e) = moved {
        let _ = std::fs::remove_file(target);
        let _ = std::fs::rename(&old, target);
        return Err(format!("could not put the new program in place: {e}"));
    }
    Ok(())
}

pub fn cleanup_old(target: &Path) {
    let _ = std::fs::remove_file(old_path(target));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::net::TcpListener;

    fn release_json(tag: &str, extra: &str) -> String {
        format!(
            r#"{{"tag_name":"{tag}","body":"notes","html_url":"https://github.com/x/y/releases/{tag}","draft":false,"prerelease":false,
            "assets":[{{"name":"{EXE_ASSET}","browser_download_url":"https://github.com/x/y/releases/download/{tag}/{EXE_ASSET}","size":123}},
                      {{"name":"{SUM_ASSET}","browser_download_url":"https://github.com/x/y/releases/download/{tag}/{SUM_ASSET}","size":80}}{extra}]}}"#
        )
    }

    #[test]
    fn versions_parse_and_compare() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.5"), Some((0, 5, 0)));
        assert_eq!(parse_version("2.0.0-beta.1"), Some((2, 0, 0)));
        assert_eq!(parse_version("abc"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert!(is_newer("v0.6.0", "0.5.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(!is_newer("0.5.0", "0.5.0"));
        assert!(!is_newer("0.4.9", "0.5.0"));
        assert!(!is_newer("garbage", "0.5.0"));
    }

    #[test]
    fn release_parsing_picks_the_assets_and_ignores_old_or_incomplete_releases() {
        let r = parse_release(&release_json("v0.6.0", ""), "0.5.0").unwrap().unwrap();
        assert_eq!(r.version, "0.6.0");
        assert!(r.exe_url.ends_with(EXE_ASSET) && r.sum_url.ends_with(SUM_ASSET));
        assert_eq!(r.notes, "notes");
        assert_eq!(parse_release(&release_json("v0.5.0", ""), "0.5.0").unwrap(), None);
        let no_sum = release_json("v0.6.0", "").replace(SUM_ASSET, "other.txt");
        assert_eq!(parse_release(&no_sum, "0.5.0").unwrap(), None);
        let draft = release_json("v0.6.0", "").replace("\"draft\":false", "\"draft\":true");
        assert_eq!(parse_release(&draft, "0.5.0").unwrap(), None);
        let pre = release_json("v0.6.0", "").replace("\"prerelease\":false", "\"prerelease\":true");
        assert_eq!(parse_release(&pre, "0.5.0").unwrap(), None);
        assert!(parse_release("not json", "0.5.0").is_err());
    }

    #[test]
    fn only_github_https_hosts_are_trusted() {
        assert!(trusted_host("https://github.com/a/b/releases/download/v1/x.exe"));
        assert!(trusted_host("https://objects.githubusercontent.com/abc"));
        assert!(!trusted_host("http://github.com/a"));
        assert!(!trusted_host("https://evil.example.com/github.com"));
        assert!(!trusted_host("https://github.com.evil.com/x"));
        assert!(!trusted_host("not a url"));
    }

    #[test]
    fn checksum_files_are_parsed_strictly() {
        let h = "a".repeat(64);
        assert_eq!(parse_checksum(&format!("{h}  Soundworks.exe\n")), Some(h.clone()));
        assert_eq!(parse_checksum(&h.to_uppercase()), Some(h));
        assert_eq!(parse_checksum("short"), None);
        assert_eq!(parse_checksum(&"z".repeat(64)), None);
        assert_eq!(parse_checksum(""), None);
    }

    fn serve(payloads: Vec<(&'static str, Vec<u8>)>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let payloads = payloads.clone();
                std::thread::spawn(move || {
                    let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                    let mut stream = stream;
                    let mut line = String::new();
                    let _ = reader.read_line(&mut line);
                    let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                    loop {
                        let mut h = String::new();
                        if reader.read_line(&mut h).is_err() || h.trim().is_empty() {
                            break;
                        }
                    }
                    let body = payloads.iter().find(|(p, _)| *p == path).map(|(_, b)| b.clone());
                    let (status, data) = match body {
                        Some(b) => ("200 OK", b),
                        None => ("404 Not Found", vec![]),
                    };
                    let head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len());
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(&data);
                });
            }
        });
        port
    }

    fn fake_exe() -> Vec<u8> {
        let mut v = b"MZ".to_vec();
        v.extend((0..(MIN_EXE_BYTES + 5000)).map(|i| (i % 251) as u8));
        v
    }

    fn sum_of(data: &[u8]) -> String {
        Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
    }

    fn rel(port: u16) -> Release {
        Release {
            version: "9.9.9".into(),
            notes: String::new(),
            page: String::new(),
            exe_url: format!("http://127.0.0.1:{port}/exe"),
            sum_url: format!("http://127.0.0.1:{port}/sum"),
            size: 0,
        }
    }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sw_update_test_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn download_verifies_the_checksum_and_reports_progress() {
        let exe = fake_exe();
        let port = serve(vec![("/exe", exe.clone()), ("/sum", format!("{}  {EXE_ASSET}\n", sum_of(&exe)).into_bytes())]);
        let dir = temp("ok");
        let progress = AtomicU64::new(0);
        let path = download(&rel(port), &dir, &progress, &|_| true).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), exe);
        assert_eq!(progress.load(Ordering::Relaxed), exe.len() as u64);
        assert!(!dir.join("Soundworks-update.exe.part").exists());
    }

    #[test]
    fn download_rejects_a_wrong_checksum_untrusted_hosts_and_non_programs() {
        let exe = fake_exe();
        let dir = temp("bad");
        let progress = AtomicU64::new(0);
        let bad_sum = serve(vec![("/exe", exe.clone()), ("/sum", format!("{}\n", "0".repeat(64)).into_bytes())]);
        let err = download(&rel(bad_sum), &dir, &progress, &|_| true).unwrap_err();
        assert!(err.contains("checksum"), "{err}");
        assert!(!dir.join("Soundworks-update.exe").exists() && !dir.join("Soundworks-update.exe.part").exists());

        let port = serve(vec![("/exe", exe.clone()), ("/sum", sum_of(&exe).into_bytes())]);
        let err = download(&rel(port), &dir, &progress, &|u| trusted_host(u)).unwrap_err();
        assert!(err.contains("GitHub"), "{err}");

        let junk = vec![b'X'; (MIN_EXE_BYTES + 10) as usize];
        let port = serve(vec![("/exe", junk.clone()), ("/sum", sum_of(&junk).into_bytes())]);
        let err = download(&rel(port), &dir, &progress, &|_| true).unwrap_err();
        assert!(err.contains("not a Windows program"), "{err}");

        let tiny = b"MZ tiny".to_vec();
        let port = serve(vec![("/exe", tiny.clone()), ("/sum", sum_of(&tiny).into_bytes())]);
        assert!(download(&rel(port), &dir, &progress, &|_| true).is_err());
    }

    #[test]
    fn install_swaps_the_program_and_keeps_the_old_one_until_cleanup() {
        let dir = temp("install");
        let target = dir.join("Soundworks.exe");
        let new = dir.join("update.exe");
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(&new, b"new").unwrap();
        install(&new, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert_eq!(std::fs::read(old_path(&target)).unwrap(), b"old");
        assert!(!new.exists());
        cleanup_old(&target);
        assert!(!old_path(&target).exists());
    }

    #[test]
    fn install_restores_the_old_program_when_the_new_one_is_missing() {
        let dir = temp("rollback");
        let target = dir.join("Soundworks.exe");
        std::fs::write(&target, b"old").unwrap();
        let err = install(&dir.join("does-not-exist.exe"), &target).unwrap_err();
        assert!(err.contains("new program"), "{err}");
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        assert!(!old_path(&target).exists());
    }

    #[test]
    fn check_treats_a_missing_repository_as_no_update() {
        let port = serve(vec![("/latest", release_json("v0.6.0", "").into_bytes())]);
        let r = check(&format!("http://127.0.0.1:{port}/latest"), "0.5.0").unwrap().unwrap();
        assert_eq!(r.version, "0.6.0");
        assert_eq!(check(&format!("http://127.0.0.1:{port}/nope"), "0.5.0").unwrap(), None);
        assert_eq!(check(&format!("http://127.0.0.1:{port}/latest"), "0.6.0").unwrap(), None);
    }
}
