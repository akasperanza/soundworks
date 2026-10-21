//! Pencere kabuğu: tek örnek denetimi, tepsi simgesi, gizleme/gösterme, açılışta başlatma, kapanış.

use super::*;

/// Aynı anda yalnızca bir Soundworks: ikincisi açılırsa mevcut pencereyi öne getirip çıkar.
pub(super) fn single_instance_ok() -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;
    if crate::devenv::dev_var("SW_SHOT").is_ok() || crate::devenv::dev_var("SW_MULTI").is_ok() {
        return true;
    }
    unsafe {
        match CreateMutexW(None, true, w!("Local\\Soundworks.SingleInstance")) {
            Ok(h) => {
                let exists = GetLastError() == ERROR_ALREADY_EXISTS;
                if !exists {
                    // tutamağı işlem boyunca açık tut
                    std::mem::forget(h);
                }
                !exists
            }
            Err(_) => true,
        }
    }
}

pub fn run(start_hidden: bool) -> anyhow::Result<()> {
    #[cfg(feature = "devtools")]
    if crate::devenv::dev_var("SW_LOGS").is_ok() {
        devlog::init();
    }
    if !single_instance_ok() {
        show_window();
        return Ok(());
    }
    let mut vp = egui::ViewportBuilder::default()
        .with_inner_size([1100.0, 690.0])
        .with_min_inner_size([820.0, 520.0])
        .with_title("Soundworks")
        .with_icon(std::sync::Arc::new(crate::appicon::window_icon()));
    if let Some(w) = crate::fsutil::read_json::<Config>(&config_path()).and_then(|c| c.win) {
        let [x, y, ww, hh, scale, maxed] = w;
        if ww >= 820.0 && hh >= 520.0 && window_on_screen(x, y, ww, hh, scale) {
            vp = vp.with_inner_size([ww, hh]).with_position([x, y]);
        }
        if maxed > 0.5 {
            vp = vp.with_maximized(true);
        }
    }
    #[allow(unused_mut)]
    let mut opts = eframe::NativeOptions { viewport: vp, ..Default::default() };
    #[cfg(feature = "devtools")]
    if crate::devenv::dev_var("SW_NOVSYNC").is_ok() {
        opts.vsync = false; // doğrulama: CPU yükü sürücünün dikey eşitleme beklemesinden mi?
    }
    let opts = if crate::devenv::dev_var("SW_OFFSCREEN").is_ok() {
        eframe::NativeOptions { viewport: opts.viewport.with_position([-6000.0, 100.0]), ..opts }
    } else {
        opts
    };
    eframe::run_native(
        "Soundworks",
        opts,
        Box::new(|cc| {
            setup(&cc.egui_ctx);
            let mut app = App::new(&cc.egui_ctx);
            app.start_hidden = start_hidden;
            if start_hidden && app.running.is_none() && app.cfg.use_loopback {
                app.start();
            }
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

pub(super) fn make_tray(ctx: &egui::Context) -> Option<(TrayIcon, CheckMenuItem)> {
    // uygulama simgesiyle aynı çizim (SW monogramı)
    let icon = crate::appicon::tray_icon()?;
    let menu = Menu::new();
    let show = MenuItem::new("Soundworks'ü göster", true, None);
    let eq = CheckMenuItem::new("EQ açık", true, true, None);
    let quit = MenuItem::new("Çıkış", true, None);
    menu.append_items(&[&show, &eq, &PredefinedMenuItem::separator(), &quit]).ok()?;
    let (show_id, eq_id, quit_id) = (show.id().clone(), eq.id().clone(), quit.id().clone());
    let tray = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("Soundworks").with_icon(icon).build().ok()?;
    let c2 = ctx.clone();
    MenuEvent::set_event_handler(Some(move |ev: MenuEvent| {
        if ev.id == show_id {
            show_window();
            c2.request_repaint();
        } else if ev.id == eq_id {
            let off = !engine::EQ_OFF.load(Relaxed);
            engine::EQ_OFF.store(off, Relaxed);
        } else if ev.id == quit_id {
            request_quit(&c2);
        }
    }));
    let c3 = ctx.clone();
    TrayIconEvent::set_event_handler(Some(move |ev: TrayIconEvent| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
            show_window();
            c3.request_repaint();
        }
    }));
    Some((tray, eq))
}

/// Tepsiden "Çıkış": önce temiz kapanış isteği (arayüz kapanışı görür, `Drop` ayarları kaydedip motoru durdurur);
/// arayüz (gizli pencere vb.) 3 sn içinde yanıt vermezse eski yoldan zorla çıkılır.
pub(super) fn request_quit(ctx: &egui::Context) {
    dev_trace("quit_request");
    QUIT_REQ.store(true, Relaxed);
    ctx.request_repaint();
    std::thread::spawn(|| {
        for _ in 0..30 {
            if QUIT_ACK.load(Relaxed) {
                // kapanış başladı; takılırsa 8 sn sonra zorla bitir
                std::thread::sleep(std::time::Duration::from_secs(8));
                dev_trace("force_exit_after_ack");
                std::process::exit(0);
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        dev_trace("force_quit_fallback");
        force_quit();
    });
}

/// Temiz kapanış çalışmazsa: varsayılan çıkışı geri alıp süreci sonlandırır.
pub(super) fn force_quit() -> ! {
    if let Some(mut c) = crate::fsutil::read_json::<Config>(&config_path()) {
        if let Some(id) = c.restore_default.take() {
            let _ = engine::set_default_render(&id);
            if let Ok(j) = serde_json::to_vec_pretty(&c) {
                let _ = crate::fsutil::write_atomic(&config_path(), &j);
            }
        }
    }
    std::process::exit(0)
}

/// Kayıtlı pencere konumu şu an bağlı ekranların içinde mi (monitör çıkarıldıysa görünmez pencere olmasın).
pub(super) fn window_on_screen(x: f32, y: f32, w: f32, h: f32, scale: f32) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN};
    let s = if scale > 0.1 { scale } else { 1.0 };
    let (vx, vy, vw, vh) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN) as f32,
            GetSystemMetrics(SM_YVIRTUALSCREEN) as f32,
            GetSystemMetrics(SM_CXVIRTUALSCREEN) as f32,
            GetSystemMetrics(SM_CYVIRTUALSCREEN) as f32,
        )
    };
    if vw <= 0.0 || vh <= 0.0 {
        return true;
    }
    let (px, py, pw) = (x * s, y * s, w * s);
    // başlık çubuğunun en az bir parçası görünür olmalı
    px + pw > vx + 120.0 && px < vx + vw - 120.0 && py >= vy - 10.0 && py < vy + vh - 80.0 && h > 0.0
}

/// Windows 11 başlık çubuğunu uygulamanın rengine boya (koyu mod + başlık/metin/kenar rengi).
pub(super) fn style_titlebar() -> bool {
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
    let Some(h) = find_window() else { return false };
    let bgr = |r: u32, g: u32, b: u32| -> u32 { r | (g << 8) | (b << 16) };
    let dark: i32 = 1;
    let caption = bgr(9, 13, 22);
    let text = bgr(205, 214, 232);
    unsafe {
        let _ = DwmSetWindowAttribute(h, DWMWINDOWATTRIBUTE(20), &dark as *const i32 as *const _, 4);
        let _ = DwmSetWindowAttribute(h, DWMWINDOWATTRIBUTE(35), &caption as *const u32 as *const _, 4);
        let _ = DwmSetWindowAttribute(h, DWMWINDOWATTRIBUTE(36), &text as *const u32 as *const _, 4);
        let _ = DwmSetWindowAttribute(h, DWMWINDOWATTRIBUTE(34), &caption as *const u32 as *const _, 4);
    }
    true
}

pub(super) fn find_window() -> Option<windows::Win32::Foundation::HWND> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
    unsafe { FindWindowW(PCWSTR::null(), w!("Soundworks")).ok() }
}

/// Tepsiye gizlenirken pencerenin eski konumu ve durumu: (x, y, büyütülmüş mü).
pub(super) static HIDDEN_AT: std::sync::Mutex<Option<(i32, i32, bool)>> = std::sync::Mutex::new(None);

/// Pencereyi tepsiye gizler. `SW_HIDE` KULLANILMAZ: gizli bir pencere için Windows çizim isteği iletmez ve arayüz döngüsü
/// durur (tepsiden çıkış, medya tuşları, hız değişimi gibi arayüze bağlı işler çalışmaz). Bunun yerine pencere ekran dışına
/// taşınır ve araç penceresi stiline alınır (görev çubuğunda / Alt+Tab'da görünmez) ama "görünür" kalır, döngü sürer.
pub(super) fn hide_window() {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongW, GetWindowRect, IsIconic, IsZoomed, SetWindowLongW, SetWindowPos, ShowWindow, GWL_EXSTYLE, SWP_NOACTIVATE, SWP_NOSIZE,
        SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE, SW_RESTORE, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    };
    let Some(h) = find_window() else { return };
    if WINDOW_HIDDEN.swap(true, Relaxed) {
        return;
    }
    unsafe {
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        let mut r = RECT::default();
        let _ = GetWindowRect(h, &mut r);
        *HIDDEN_AT.lock().unwrap() = Some((r.left, r.top, IsZoomed(h).as_bool()));
        // stil değişikliğinin görev çubuğuna yansıması için önce gizle, sonra ekran dışında yeniden göster
        let _ = ShowWindow(h, SW_HIDE);
        let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
        SetWindowLongW(h, GWL_EXSTYLE, ((ex | WS_EX_TOOLWINDOW.0) & !WS_EX_APPWINDOW.0) as i32);
        let _ = SetWindowPos(h, windows::Win32::Foundation::HWND::default(), -32000, -32000, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
}

/// Tepsideki pencereyi eski konumunda geri getirir.
pub(super) fn show_window() {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongW, IsIconic, SetForegroundWindow, SetWindowLongW, SetWindowPos, ShowWindow, GWL_EXSTYLE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
        SW_HIDE, SW_MAXIMIZE, SW_RESTORE, SW_SHOW, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    };
    let Some(h) = find_window() else { return };
    unsafe {
        if WINDOW_HIDDEN.swap(false, Relaxed) {
            let (x, y, zoomed) = HIDDEN_AT.lock().unwrap().take().unwrap_or((100, 100, false));
            let _ = ShowWindow(h, SW_HIDE);
            let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
            SetWindowLongW(h, GWL_EXSTYLE, ((ex | WS_EX_APPWINDOW.0) & !WS_EX_TOOLWINDOW.0) as i32);
            let _ = SetWindowPos(h, windows::Win32::Foundation::HWND::default(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_SHOWWINDOW);
            let _ = ShowWindow(h, if zoomed { SW_MAXIMIZE } else { SW_SHOW });
        } else if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        } else {
            let _ = ShowWindow(h, SW_SHOW);
        }
        let _ = SetForegroundWindow(h);
    }
}

pub(super) fn set_run_at_login(on: bool) {
    use std::os::windows::process::CommandExt;
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let mut cmd = std::process::Command::new("reg");
    if on {
        if let Ok(exe) = std::env::current_exe() {
            cmd.args(["add", key, "/v", "Soundworks", "/t", "REG_SZ", "/d", &format!("\"{}\" --tray", exe.display()), "/f"]);
        } else {
            return;
        }
    } else {
        cmd.args(["delete", key, "/v", "Soundworks", "/f"]);
    }
    let _ = cmd.creation_flags(0x0800_0000).output();
}
