use crate::library::OnlineMeta;
use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey, Tag};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const WRITABLE_EXTS: [&str; 10] = ["flac", "mp3", "m4a", "mp4", "ogg", "opus", "wav", "aiff", "aif", "ape"];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct WriteLogEntry {
    pub path: String,
    pub fields: Vec<(String, String)>,
}

pub fn can_write(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).map_or(false, |e| WRITABLE_EXTS.contains(&e.to_lowercase().as_str()))
}

fn field_value(tag: &Tag, field: &str) -> Option<String> {
    let v = match field {
        "title" => tag.title().map(|c| c.to_string()),
        "artist" => tag.artist().map(|c| c.to_string()),
        "album" => tag.album().map(|c| c.to_string()),
        "album_artist" => tag.get_string(&ItemKey::AlbumArtist).map(String::from),
        "year" => tag.get_string(&ItemKey::Year).or_else(|| tag.get_string(&ItemKey::RecordingDate)).map(|s| s.chars().take(4).collect()),
        "track" => tag.track().map(|n| n.to_string()),
        _ => None,
    };
    v.filter(|s| !s.trim().is_empty())
}

fn set_field(tag: &mut Tag, field: &str, value: &str) {
    match field {
        "title" => tag.set_title(value.to_string()),
        "artist" => tag.set_artist(value.to_string()),
        "album" => tag.set_album(value.to_string()),
        "album_artist" => {
            tag.insert_text(ItemKey::AlbumArtist, value.to_string());
        }
        "year" => {
            if !tag.insert_text(ItemKey::Year, value.to_string()) {
                tag.insert_text(ItemKey::RecordingDate, value.to_string());
            }
        }
        "track" => {
            if let Ok(n) = value.parse::<u32>() {
                tag.set_track(n);
            }
        }
        _ => {}
    }
}

fn clear_field(tag: &mut Tag, field: &str) {
    match field {
        "title" => tag.remove_title(),
        "artist" => tag.remove_artist(),
        "album" => tag.remove_album(),
        "album_artist" => tag.remove_key(&ItemKey::AlbumArtist),
        "year" => {
            tag.remove_key(&ItemKey::Year);
            tag.remove_key(&ItemKey::RecordingDate);
        }
        "track" => tag.remove_track(),
        _ => {}
    }
}

fn wanted(meta: &OnlineMeta) -> Vec<(&'static str, String)> {
    let mut v = vec![];
    if !meta.title.is_empty() {
        v.push(("title", meta.title.clone()));
    }
    if !meta.artist.is_empty() {
        v.push(("artist", meta.artist.clone()));
    }
    if !meta.album.is_empty() {
        v.push(("album", meta.album.clone()));
    }
    if !meta.album_artist.is_empty() {
        v.push(("album_artist", meta.album_artist.clone()));
    }
    if meta.year > 0 {
        v.push(("year", meta.year.to_string()));
    }
    if meta.track_no > 0 {
        v.push(("track", meta.track_no.to_string()));
    }
    v
}

fn open(path: &Path) -> Result<TaggedFile, String> {
    Probe::open(path).map_err(|e| e.to_string())?.guess_file_type().map_err(|e| e.to_string())?.read().map_err(|e| e.to_string())
}

fn primary_tag(file: &mut TaggedFile) -> &mut Tag {
    if file.primary_tag().is_none() {
        let kind = file.primary_tag_type();
        file.insert_tag(Tag::new(kind));
    }
    file.primary_tag_mut().expect("tag was just inserted")
}

fn temp_path(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    path.with_file_name(format!(".{name}.swtmp"))
}

fn through_temp(path: &Path, edit: impl FnOnce(&Path) -> Result<Vec<(String, String)>, String>) -> Result<Vec<(String, String)>, String> {
    let tmp = temp_path(path);
    std::fs::copy(path, &tmp).map_err(|e| e.to_string())?;
    let done = edit(&tmp);
    match done {
        Ok(fields) if !fields.is_empty() => match std::fs::rename(&tmp, path) {
            Ok(()) => Ok(fields),
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                Err(format!("file is in use or read-only: {e}"))
            }
        },
        Ok(fields) => {
            let _ = std::fs::remove_file(&tmp);
            Ok(fields)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

pub fn write_missing(path: &Path, meta: &OnlineMeta) -> Result<Vec<(String, String)>, String> {
    if !can_write(path) {
        return Err("this file format cannot be tagged".into());
    }
    through_temp(path, |tmp| {
        let mut file = open(tmp)?;
        let mut tried = vec![];
        {
            let tag = primary_tag(&mut file);
            for (field, value) in wanted(meta) {
                if field_value(tag, field).is_none() {
                    set_field(tag, field, &value);
                    tried.push((field.to_string(), value));
                }
            }
        }
        if tried.is_empty() {
            return Ok(vec![]);
        }
        file.save_to_path(tmp, WriteOptions::default()).map_err(|e| e.to_string())?;
        let check = open(tmp)?;
        let landed: Vec<(String, String)> = tried
            .into_iter()
            .filter(|(f, v)| check.primary_tag().or_else(|| check.first_tag()).and_then(|t| field_value(t, f)).map_or(false, |got| got == *v))
            .collect();
        Ok(landed)
    })
}

pub fn undo(entry: &WriteLogEntry) -> Result<usize, String> {
    let path = Path::new(&entry.path);
    let removed = through_temp(path, |tmp| {
        let mut file = open(tmp)?;
        let mut removed = vec![];
        {
            let tag = primary_tag(&mut file);
            for (field, value) in &entry.fields {
                if field_value(tag, field).as_deref() == Some(value.as_str()) {
                    clear_field(tag, field);
                    removed.push((field.clone(), value.clone()));
                }
            }
        }
        if removed.is_empty() {
            return Ok(vec![]);
        }
        file.save_to_path(tmp, WriteOptions::default()).map_err(|e| e.to_string())?;
        Ok(removed)
    })?;
    Ok(removed.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn silent_wav(path: &Path) {
        let (rate, secs) = (8000u32, 1u32);
        let data = vec![0u8; (rate * secs * 2) as usize];
        let mut out = Vec::new();
        out.extend(b"RIFF");
        out.extend((36 + data.len() as u32).to_le_bytes());
        out.extend(b"WAVEfmt ");
        out.extend(16u32.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(rate.to_le_bytes());
        out.extend((rate * 2).to_le_bytes());
        out.extend(2u16.to_le_bytes());
        out.extend(16u16.to_le_bytes());
        out.extend(b"data");
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        std::fs::write(path, out).unwrap();
    }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sw_tagwrite_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn meta() -> OnlineMeta {
        OnlineMeta { title: "Şarkı Adı".into(), artist: "Sanatçı".into(), album: "Albüm".into(), album_artist: String::new(), year: 2015, track_no: 3, source: "test".into() }
    }

    fn tags_of(path: &Path) -> (Option<String>, Option<String>, Option<String>) {
        let f = open(path).unwrap();
        let t = f.primary_tag().or_else(|| f.first_tag()).cloned();
        match t {
            Some(t) => (field_value(&t, "title"), field_value(&t, "artist"), field_value(&t, "album")),
            None => (None, None, None),
        }
    }

    #[test]
    fn missing_fields_are_written_and_reported() {
        let dir = temp("write");
        let file = dir.join("a.wav");
        silent_wav(&file);
        let written = write_missing(&file, &meta()).unwrap();
        assert!(written.iter().any(|(f, v)| f == "title" && v == "Şarkı Adı"), "{written:?}");
        assert!(written.iter().any(|(f, _)| f == "artist"));
        let (title, artist, album) = tags_of(&file);
        assert_eq!(title.as_deref(), Some("Şarkı Adı"));
        assert_eq!(artist.as_deref(), Some("Sanatçı"));
        assert_eq!(album.as_deref(), Some("Albüm"));
        assert!(!temp_path(&file).exists());
    }

    #[test]
    fn existing_tags_are_never_overwritten() {
        let dir = temp("keep");
        let file = dir.join("b.wav");
        silent_wav(&file);
        let first = OnlineMeta { title: "Kendi Başlığım".into(), artist: String::new(), album: String::new(), album_artist: String::new(), year: 0, track_no: 0, source: "x".into() };
        write_missing(&file, &first).unwrap();
        let written = write_missing(&file, &meta()).unwrap();
        assert!(!written.iter().any(|(f, _)| f == "title"), "{written:?}");
        let (title, artist, _) = tags_of(&file);
        assert_eq!(title.as_deref(), Some("Kendi Başlığım"));
        assert_eq!(artist.as_deref(), Some("Sanatçı"));
    }

    #[test]
    fn nothing_is_touched_when_there_is_nothing_to_fill() {
        let dir = temp("noop");
        let file = dir.join("c.wav");
        silent_wav(&file);
        write_missing(&file, &meta()).unwrap();
        let before = std::fs::read(&file).unwrap();
        let again = write_missing(&file, &meta()).unwrap();
        assert!(again.is_empty());
        assert_eq!(std::fs::read(&file).unwrap(), before);
        assert!(!temp_path(&file).exists());
    }

    #[test]
    fn undo_removes_only_what_was_written() {
        let dir = temp("undo");
        let file = dir.join("d.wav");
        silent_wav(&file);
        let manual = OnlineMeta { title: "Elle Yazılmış".into(), artist: String::new(), album: String::new(), album_artist: String::new(), year: 0, track_no: 0, source: "x".into() };
        write_missing(&file, &manual).unwrap();
        let written = write_missing(&file, &meta()).unwrap();
        let entry = WriteLogEntry { path: file.to_string_lossy().to_string(), fields: written };
        let n = undo(&entry).unwrap();
        assert!(n >= 2);
        let (title, artist, album) = tags_of(&file);
        assert_eq!(title.as_deref(), Some("Elle Yazılmış"));
        assert_eq!(artist, None);
        assert_eq!(album, None);
    }

    #[test]
    fn unsupported_or_missing_files_fail_cleanly() {
        let dir = temp("bad");
        let dsd = dir.join("x.dsf");
        std::fs::write(&dsd, b"DSD ").unwrap();
        assert!(write_missing(&dsd, &meta()).is_err());
        assert!(write_missing(&dir.join("none.wav"), &meta()).is_err());
        let junk = dir.join("junk.wav");
        std::fs::write(&junk, b"not audio").unwrap();
        assert!(write_missing(&junk, &meta()).is_err());
        assert_eq!(std::fs::read(&junk).unwrap(), b"not audio");
        assert!(!temp_path(&junk).exists());
    }

    #[test]
    fn writable_formats_are_recognised_case_insensitively() {
        assert!(can_write(Path::new("a.FLAC")) && can_write(Path::new("b.mp3")));
        assert!(!can_write(Path::new("c.dsf")) && !can_write(Path::new("d.txt")));
    }

    #[test]
    #[ignore]
    fn real_files_roundtrip() {
        let dir = temp("real");
        for src in std::env::var("SW_TEST_AUDIO").unwrap_or_default().split(';').filter(|s| !s.is_empty()) {
            let file = dir.join(Path::new(src).file_name().unwrap());
            std::fs::copy(src, &file).unwrap();
            let mut f = open(&file).unwrap();
            if let Some(t) = f.primary_tag_mut() {
                for f in ["title", "artist", "album", "album_artist", "year", "track"] {
                    clear_field(t, f);
                }
            }
            f.save_to_path(&file, WriteOptions::default()).unwrap();
            let blank = OnlineMeta { year: 1999, track_no: 9, album_artist: "AA".into(), ..meta() };
            let first = write_missing(&file, &blank).unwrap();
            println!("{src}: wrote {first:?}");
            let again = write_missing(&file, &blank).unwrap();
            assert!(again.is_empty(), "second write must be a no-op: {again:?}");
            let n = undo(&WriteLogEntry { path: file.to_string_lossy().to_string(), fields: first.clone() }).unwrap();
            assert_eq!(n, first.len());
            assert!(open(&file).is_ok());
        }
    }
}
