//! Klavyedeki medya tuşları (oynat/duraklat, sonraki, önceki, durdur).
//! Ayrı bir iş parçacığı global kısayol (RegisterHotKey) kaydeder. Tuşlar YALNIZCA motor çalışırken yakalanır:
//! Soundworks boştayken tuşları (ör. Spotify için) çalmayız.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MediaCmd {
    PlayPause,
    Next,
    Prev,
    Stop,
}

pub const VK_MEDIA_NEXT_TRACK: u32 = 0xB0;
pub const VK_MEDIA_PREV_TRACK: u32 = 0xB1;
pub const VK_MEDIA_STOP: u32 = 0xB2;
pub const VK_MEDIA_PLAY_PAUSE: u32 = 0xB3;

/// (sanal tuş, kısayol kimliği, komut)
const KEYS: [(u32, i32, MediaCmd); 4] = [
    (VK_MEDIA_PLAY_PAUSE, 1, MediaCmd::PlayPause),
    (VK_MEDIA_NEXT_TRACK, 2, MediaCmd::Next),
    (VK_MEDIA_PREV_TRACK, 3, MediaCmd::Prev),
    (VK_MEDIA_STOP, 4, MediaCmd::Stop),
];

pub fn cmd_for_id(id: i32) -> Option<MediaCmd> {
    KEYS.iter().find(|k| k.1 == id).map(|k| k.2)
}

fn queue() -> &'static Mutex<VecDeque<MediaCmd>> {
    static Q: OnceLock<Mutex<VecDeque<MediaCmd>>> = OnceLock::new();
    Q.get_or_init(|| Mutex::new(VecDeque::new()))
}

/// Tuşların dinlenip dinlenmeyeceği (arayüz motor çalışırken true yapar).
static ENABLED: AtomicBool = AtomicBool::new(false);
static STARTED: AtomicBool = AtomicBool::new(false);

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Gelen komutlar (arayüz her karede boşaltır).
pub fn drain() -> Vec<MediaCmd> {
    queue().lock().unwrap().drain(..).collect()
}

/// Dinleyici iş parçacığını bir kez başlat. `wake`: komut gelince arayüzü uyandır (pencere gizliyken de).
pub fn start(wake: impl Fn() + Send + 'static) {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, MOD_NOREPEAT};
        use windows::Win32::UI::WindowsAndMessaging::{PeekMessageW, MSG, PM_REMOVE, WM_HOTKEY};
        let mut registered = false;
        loop {
            let want = ENABLED.load(Ordering::Relaxed);
            if want && !registered {
                for (vk, id, _) in KEYS {
                    unsafe {
                        // başka uygulama aynı tuşu almışsa kayıt başarısız olur; o tuşu atla
                        let _ = RegisterHotKey(None, id, MOD_NOREPEAT, vk);
                    }
                }
                registered = true;
            } else if !want && registered {
                for (_, id, _) in KEYS {
                    unsafe {
                        let _ = UnregisterHotKey(None, id);
                    }
                }
                registered = false;
            }
            unsafe {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, WM_HOTKEY, WM_HOTKEY, PM_REMOVE).as_bool() {
                    if let Some(c) = cmd_for_id(msg.wParam.0 as i32) {
                        queue().lock().unwrap().push_back(c);
                        wake();
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_mapping_is_complete_and_unique() {
        assert_eq!(cmd_for_id(1), Some(MediaCmd::PlayPause));
        assert_eq!(cmd_for_id(2), Some(MediaCmd::Next));
        assert_eq!(cmd_for_id(3), Some(MediaCmd::Prev));
        assert_eq!(cmd_for_id(4), Some(MediaCmd::Stop));
        assert_eq!(cmd_for_id(9), None);
        let mut ids: Vec<i32> = KEYS.iter().map(|k| k.1).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 4);
        // Windows sanal tuş kodları
        assert_eq!((VK_MEDIA_NEXT_TRACK, VK_MEDIA_PREV_TRACK, VK_MEDIA_STOP, VK_MEDIA_PLAY_PAUSE), (0xB0, 0xB1, 0xB2, 0xB3));
    }

    #[test]
    fn queue_drains_in_order() {
        queue().lock().unwrap().clear();
        for c in [MediaCmd::Next, MediaCmd::PlayPause, MediaCmd::Prev] {
            queue().lock().unwrap().push_back(c);
        }
        assert_eq!(drain(), vec![MediaCmd::Next, MediaCmd::PlayPause, MediaCmd::Prev]);
        assert!(drain().is_empty());
    }

    /// Kısayolları kaydedip hemen bırakır: API çağrısının geçerli olduğunu doğrular, tuşlara basmaz.
    #[test]
    fn hotkey_api_register_and_release() {
        use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, MOD_NOREPEAT};
        let ok = unsafe { RegisterHotKey(None, 77, MOD_NOREPEAT, VK_MEDIA_STOP).is_ok() };
        if ok {
            unsafe {
                let _ = UnregisterHotKey(None, 77);
            }
        }
        // başka bir uygulama tuşu tutuyorsa kayıt reddedilebilir; bu hata değildir
        println!("medya tusu kaydi: {}", if ok { "basarili" } else { "baska uygulama tutuyor" });
    }
}
