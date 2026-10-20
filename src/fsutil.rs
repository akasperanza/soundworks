//! Kalıcı JSON dosyaları için güvenli yazma/okuma: yazma sırasında çökme dosyayı bozmaz.

use serde::de::DeserializeOwned;
use std::path::{Path, PathBuf};

fn sibling(path: &Path, ext: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(ext);
    PathBuf::from(s)
}

/// Önce geçici dosyaya yazar, sonra yerine taşır (atomik). Eski sürüm `.bak` olarak saklanır.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = sibling(path, ".tmp");
    std::fs::write(&tmp, bytes)?;
    if path.exists() {
        let _ = std::fs::copy(path, sibling(path, ".bak"));
    }
    std::fs::rename(&tmp, path)
}

/// JSON'u okur; ana dosya yok ya da bozuksa `.bak` yedeğine düşer.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let try_read = |p: &Path| std::fs::read(p).ok().and_then(|b| serde_json::from_slice::<T>(&b).ok());
    try_read(path).or_else(|| try_read(&sibling(path, ".bak")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_keeps_backup_and_recovers_from_corruption() {
        let dir = std::env::temp_dir().join("sw_fsutil_test");
        let _ = std::fs::remove_dir_all(&dir);
        let p = dir.join("a.json");
        write_atomic(&p, b"[1,2]").unwrap();
        write_atomic(&p, b"[3,4]").unwrap();
        assert_eq!(read_json::<Vec<u32>>(&p), Some(vec![3, 4]));
        // ana dosya bozulursa yedekten okunur
        std::fs::write(&p, b"[3,").unwrap();
        assert_eq!(read_json::<Vec<u32>>(&p), Some(vec![1, 2]));
        assert!(!sibling(&p, ".tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
