//! Windows media integration: the system media overlay (headphone and keyboard buttons, lock screen, volume flyout)
//! through SystemMediaTransportControls, and the play/pause/skip buttons in the taskbar thumbnail.

use crate::media::{push, MediaCmd};
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use windows::core::{HSTRING, PCWSTR};
use windows::Foundation::{TypedEventHandler, Uri};
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, SystemMediaTransportControls, SystemMediaTransportControlsButton, SystemMediaTransportControlsButtonPressedEventArgs,
};
use windows::Storage::Streams::RandomAccessStreamReference;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;
use windows::Win32::UI::Shell::{
    DefSubclassProc, ITaskbarList3, SetWindowSubclass, TaskbarList, THBF_DISABLED, THBF_ENABLED, THB_FLAGS, THB_ICON, THB_TOOLTIP, THUMBBUTTON,
};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, DestroyIcon, RegisterWindowMessageW, HICON, ICONINFO, WM_COMMAND};

const ID_PREV: u32 = 100;
const ID_PLAY: u32 = 101;
const ID_NEXT: u32 = 102;
const THBN_CLICKED: usize = 0x1800;

static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, PartialEq, Default)]
pub struct NowPlaying {
    pub active: bool,
    pub playing: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub cover: Option<PathBuf>,
    pub cover_id: u64,
}

struct Icons {
    prev: HICON,
    next: HICON,
    play: HICON,
    pause: HICON,
}

struct State {
    hwnd: HWND,
    smtc: Option<SystemMediaTransportControls>,
    taskbar: Option<ITaskbarList3>,
    buttons_added: bool,
    icons: Option<Icons>,
    last: Option<NowPlaying>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn glyph(kind: u8) -> Vec<u8> {
    // 32x32 straight-alpha BGRA: a light glyph with a thin dark halo so it reads on light and dark taskbars
    const N: usize = 32;
    let inside = |x: f32, y: f32| -> bool {
        let tri = |x0: f32, x1: f32, y0: f32, y1: f32, right: bool| {
            let (t, ym) = (if right { (x - x0) / (x1 - x0) } else { (x1 - x) / (x1 - x0) }, (y0 + y1) * 0.5);
            (0.0..=1.0).contains(&t) && (y - ym).abs() <= (y1 - y0) * 0.5 * (1.0 - t)
        };
        let bar = |x0: f32, x1: f32| x >= x0 && x <= x1 && y >= 8.0 && y <= 24.0;
        match kind {
            0 => bar(7.0, 10.5) || tri(11.0, 25.0, 8.0, 24.0, false),
            1 => bar(21.5, 25.0) || tri(7.0, 21.0, 8.0, 24.0, true),
            2 => tri(9.0, 25.0, 6.0, 26.0, true),
            _ => bar(9.0, 13.5) || bar(18.5, 23.0),
        }
    };
    let cover = |px: usize, py: usize, grow: f32| -> f32 {
        let mut hit = 0;
        for sy in 0..4 {
            for sx in 0..4 {
                let (x, y) = (px as f32 + (sx as f32 + 0.5) / 4.0, py as f32 + (sy as f32 + 0.5) / 4.0);
                let mut any = inside(x, y);
                if !any && grow > 0.0 {
                    any = [(grow, 0.0), (-grow, 0.0), (0.0, grow), (0.0, -grow)].iter().any(|(dx, dy)| inside(x + dx, y + dy));
                }
                hit += any as i32;
            }
        }
        hit as f32 / 16.0
    };
    let mut out = vec![0u8; N * N * 4];
    for py in 0..N {
        for px in 0..N {
            let (fg, halo) = (cover(px, py, 0.0), cover(px, py, 1.3));
            let i = (py * N + px) * 4;
            let a = fg + (halo - fg).max(0.0) * 0.55 * (1.0 - fg);
            if a <= 0.0 {
                continue;
            }
            let shade = 240.0 * fg / a.max(0.001) + 20.0 * (1.0 - fg / a.max(0.001));
            out[i] = shade as u8;
            out[i + 1] = shade as u8;
            out[i + 2] = shade as u8;
            out[i + 3] = (a.min(1.0) * 255.0) as u8;
        }
    }
    out
}

fn make_icon(kind: u8) -> Option<HICON> {
    let bgra = glyph(kind);
    let mask = vec![0u8; 32 * 4];
    unsafe {
        let color = CreateBitmap(32, 32, 1, 32, Some(bgra.as_ptr() as *const _));
        let mono = CreateBitmap(32, 32, 1, 1, Some(mask.as_ptr() as *const _));
        let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mono, hbmColor: color };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(color);
        let _ = DeleteObject(mono);
        icon
    }
}

fn tip(text: &str) -> [u16; 260] {
    let mut out = [0u16; 260];
    for (i, c) in crate::i18n::tr(text).encode_utf16().take(259).enumerate() {
        out[i] = c;
    }
    out
}

fn button(id: u32, icon: HICON, text: &str, enabled: bool) -> THUMBBUTTON {
    THUMBBUTTON {
        dwMask: THB_ICON | THB_TOOLTIP | THB_FLAGS,
        iId: id,
        iBitmap: 0,
        hIcon: icon,
        szTip: tip(text),
        dwFlags: if enabled { THBF_ENABLED } else { THBF_DISABLED },
    }
}

unsafe extern "system" fn subclass_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, _data: usize) -> LRESULT {
    if msg == WM_COMMAND && (wparam.0 >> 16) & 0xffff == THBN_CLICKED {
        match (wparam.0 & 0xffff) as u32 {
            ID_PREV => push(MediaCmd::Prev),
            ID_PLAY => push(MediaCmd::PlayPause),
            ID_NEXT => push(MediaCmd::Next),
            _ => {}
        }
        return LRESULT(0);
    }
    let created = TASKBAR_CREATED.load(Ordering::Relaxed);
    if created != 0 && msg == created {
        STATE.with(|s| {
            if let Some(st) = s.try_borrow_mut().ok().as_mut().and_then(|b| b.as_mut()) {
                st.buttons_added = false;
                add_buttons(st);
            }
        });
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

fn add_buttons(st: &mut State) {
    if st.buttons_added {
        return;
    }
    let Some(tb) = st.taskbar.clone() else { return };
    let Some(icons) = &st.icons else { return };
    let enabled = st.last.as_ref().map_or(false, |n| n.active);
    let playing = st.last.as_ref().map_or(false, |n| n.playing);
    let list = [
        button(ID_PREV, icons.prev, "Önceki", enabled),
        button(ID_PLAY, if playing { icons.pause } else { icons.play }, if playing { "Duraklat" } else { "Çal" }, enabled),
        button(ID_NEXT, icons.next, "Sonraki", enabled),
    ];
    unsafe {
        if tb.ThumbBarAddButtons(st.hwnd, &list).is_ok() {
            st.buttons_added = true;
        }
    }
}

fn init_smtc(hwnd: HWND) -> Option<SystemMediaTransportControls> {
    let interop = windows::core::factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>().ok()?;
    let smtc: SystemMediaTransportControls = unsafe { interop.GetForWindow(hwnd).ok()? };
    smtc.SetIsEnabled(true).ok()?;
    for r in [smtc.SetIsPlayEnabled(true), smtc.SetIsPauseEnabled(true), smtc.SetIsNextEnabled(true), smtc.SetIsPreviousEnabled(true), smtc.SetIsStopEnabled(true)] {
        r.ok()?;
    }
    let handler = TypedEventHandler::<SystemMediaTransportControls, SystemMediaTransportControlsButtonPressedEventArgs>::new(|_, args| {
        if let Some(args) = args.as_ref() {
            match args.Button()? {
                SystemMediaTransportControlsButton::Play | SystemMediaTransportControlsButton::Pause => push(MediaCmd::PlayPause),
                SystemMediaTransportControlsButton::Next => push(MediaCmd::Next),
                SystemMediaTransportControlsButton::Previous => push(MediaCmd::Prev),
                SystemMediaTransportControlsButton::Stop => push(MediaCmd::Stop),
                _ => {}
            }
        }
        Ok(())
    });
    smtc.ButtonPressed(&handler).ok()?;
    Some(smtc)
}

fn install(hwnd: HWND) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let msg = RegisterWindowMessageW(PCWSTR(HSTRING::from("TaskbarButtonCreated").as_ptr()));
        TASKBAR_CREATED.store(msg, Ordering::Relaxed);
        let _ = SetWindowSubclass(hwnd, Some(subclass_proc), 0x5357, 0);
    }
    let taskbar: Option<ITaskbarList3> = unsafe {
        CoCreateInstance::<_, ITaskbarList3>(&TaskbarList, None, CLSCTX_INPROC_SERVER).ok().filter(|t| t.HrInit().is_ok())
    };
    let icons = match (make_icon(0), make_icon(1), make_icon(2), make_icon(3)) {
        (Some(prev), Some(next), Some(play), Some(pause)) => Some(Icons { prev, next, play, pause }),
        _ => None,
    };
    let mut st = State { hwnd, smtc: init_smtc(hwnd), taskbar, buttons_added: false, icons, last: None };
    add_buttons(&mut st);
    #[cfg(feature = "devtools")]
    if let Ok(p) = std::env::var("SW_WINMEDIA_LOG") {
        let _ = std::fs::write(p, format!("hwnd={:?} smtc={} taskbar={} icons={} buttons={}", hwnd.0, st.smtc.is_some(), st.taskbar.is_some(), st.icons.is_some(), st.buttons_added));
    }
    STATE.with(|s| *s.borrow_mut() = Some(st));
}

fn apply(st: &mut State, np: &NowPlaying) {
    let prev = st.last.clone();
    if let Some(smtc) = &st.smtc {
        let status = if !np.active {
            MediaPlaybackStatus::Closed
        } else if np.playing {
            MediaPlaybackStatus::Playing
        } else {
            MediaPlaybackStatus::Paused
        };
        let _ = smtc.SetPlaybackStatus(status);
        let meta_changed = prev.as_ref().map_or(true, |p| (&p.title, &p.artist, &p.album, p.cover_id, p.active) != (&np.title, &np.artist, &np.album, np.cover_id, np.active));
        if np.active && meta_changed {
            if let Ok(du) = smtc.DisplayUpdater() {
                let _ = du.SetType(MediaPlaybackType::Music);
                if let Ok(mp) = du.MusicProperties() {
                    let _ = mp.SetTitle(&HSTRING::from(np.title.as_str()));
                    let _ = mp.SetArtist(&HSTRING::from(np.artist.as_str()));
                    let _ = mp.SetAlbumTitle(&HSTRING::from(np.album.as_str()));
                }
                let thumb = np.cover.as_ref().and_then(|p| {
                    let url = format!("file:///{}", p.to_string_lossy().replace('\\', "/"));
                    Uri::CreateUri(&HSTRING::from(url)).ok().and_then(|u| RandomAccessStreamReference::CreateFromUri(&u).ok())
                });
                let _ = du.SetThumbnail(thumb.as_ref());
                let _ = du.Update();
            }
        }
    }
    if let (Some(tb), Some(icons)) = (&st.taskbar, &st.icons) {
        if st.buttons_added && prev.as_ref().map_or(true, |p| (p.active, p.playing) != (np.active, np.playing)) {
            let list = [
                button(ID_PREV, icons.prev, "Önceki", np.active),
                button(ID_PLAY, if np.playing { icons.pause } else { icons.play }, if np.playing { "Duraklat" } else { "Çal" }, np.active),
                button(ID_NEXT, icons.next, "Sonraki", np.active),
            ];
            unsafe {
                let _ = tb.ThumbBarUpdateButtons(st.hwnd, &list);
            }
        }
    }
    st.last = Some(np.clone());
}

/// The app's own top-level window on this thread (another running copy of the app must never be picked up).
pub fn own_window() -> Option<HWND> {
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible};
    unsafe extern "system" fn each(h: HWND, found: LPARAM) -> BOOL {
        let mut title = [0u16; 64];
        let n = GetWindowTextW(h, &mut title) as usize;
        if GetWindowThreadProcessId(h, None) == GetCurrentThreadId() && String::from_utf16_lossy(&title[..n]) == "Soundworks" && IsWindowVisible(h).as_bool() {
            *(found.0 as *mut Option<HWND>) = Some(h);
            return false.into();
        }
        true.into()
    }
    let mut found: Option<HWND> = None;
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut found as *mut _ as isize));
    }
    found
}

/// Call once per frame on the UI thread. Installs itself as soon as the window exists.
pub fn sync(hwnd: Option<HWND>, np: &NowPlaying) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.is_none() {
            let Some(h) = hwnd else { return };
            drop(s);
            install(h);
            return;
        }
        if let Some(st) = s.as_mut() {
            if st.last.as_ref() != Some(np) {
                apply(st, np);
            }
        }
    });
}

pub fn installed() -> bool {
    STATE.with(|s| s.try_borrow().map_or(true, |b| b.is_some()))
}

#[allow(dead_code)]
fn release_icons(icons: Icons) {
    for i in [icons.prev, icons.next, icons.play, icons.pause] {
        unsafe {
            let _ = DestroyIcon(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs_have_visible_pixels_and_distinct_shapes() {
        let all: Vec<Vec<u8>> = (0..4).map(glyph).collect();
        for g in &all {
            assert_eq!(g.len(), 32 * 32 * 4);
            assert!(g.chunks(4).filter(|p| p[3] > 200).count() > 40);
        }
        assert_ne!(all[2], all[3]);
        assert_ne!(all[0], all[1]);
    }

    #[test]
    fn tooltips_fit_the_fixed_buffer() {
        let t = tip("Önceki");
        assert_eq!(t[259], 0);
        assert!(t[0] != 0);
    }
}
