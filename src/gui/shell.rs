use super::*;

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
        opts.vsync = false;
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

    let icon = crate::appicon::tray_icon()?;
    let menu = Menu::new();
    let show = MenuItem::new(crate::i18n::tr("Soundworks'ü göster"), true, None);
    let eq = CheckMenuItem::new(crate::i18n::tr("EQ açık"), true, true, None);
    let quit = MenuItem::new(match crate::i18n::get() { crate::i18n::Lang::Tr => "Çıkış", crate::i18n::Lang::En => "Quit", crate::i18n::Lang::De => "Beenden" }, true, None);
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

pub(super) fn request_quit(ctx: &egui::Context) {
    dev_trace("quit_request");
    QUIT_REQ.store(true, Relaxed);
    ctx.request_repaint();
    std::thread::spawn(|| {
        for _ in 0..30 {
            if QUIT_ACK.load(Relaxed) {

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

    px + pw > vx + 120.0 && px < vx + vw - 120.0 && py >= vy - 10.0 && py < vy + vh - 80.0 && h > 0.0
}

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

pub(super) static HIDDEN_AT: std::sync::Mutex<Option<(i32, i32, bool)>> = std::sync::Mutex::new(None);

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

        let _ = ShowWindow(h, SW_HIDE);
        let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
        SetWindowLongW(h, GWL_EXSTYLE, ((ex | WS_EX_TOOLWINDOW.0) & !WS_EX_APPWINDOW.0) as i32);
        let _ = SetWindowPos(h, windows::Win32::Foundation::HWND::default(), -32000, -32000, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
}

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
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Registry::*;
    unsafe {
        let mut key = HKEY::default();
        let opened = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        );
        if opened.is_err() {
            return;
        }
        if on {
            if let Ok(exe) = std::env::current_exe() {
                let value: Vec<u16> = format!("\"{}\" --tray", exe.display()).encode_utf16().chain(Some(0)).collect();
                let bytes = std::slice::from_raw_parts(value.as_ptr() as *const u8, value.len() * 2);
                let _ = RegSetValueExW(key, w!("Soundworks"), 0, REG_SZ, Some(bytes));
            }
        } else {
            let _ = RegDeleteValueW(key, w!("Soundworks"));
        }
        let _ = RegCloseKey(key);
    }
}

pub(crate) fn shell_open(target: &str) {
    shell_run("open", target, None);
}

pub(crate) fn shell_select(file: &std::path::Path) {
    shell_run("open", "explorer.exe", Some(&format!("/select,\"{}\"", file.display())));
}

fn shell_run(verb: &str, file: &str, params: Option<&str>) {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() };
    let (verb, file) = (wide(verb), wide(file));
    let params = params.map(wide);
    unsafe {
        ShellExecuteW(
            windows::Win32::Foundation::HWND::default(),
            PCWSTR(verb.as_ptr()),
            PCWSTR(file.as_ptr()),
            params.as_ref().map_or(PCWSTR::null(), |p| PCWSTR(p.as_ptr())),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_value() -> String {
        let out = std::process::Command::new("reg")
            .args(["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "Soundworks"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    #[test]
    #[ignore = "writes the real HKCU Run key"]
    fn run_at_login_writes_and_removes_the_registry_value() {
        if run_value().contains("Soundworks") {
            return;
        }
        set_run_at_login(true);
        let on = run_value();
        set_run_at_login(false);
        let off = run_value();
        assert!(on.contains("--tray") && on.contains("REG_SZ"), "{on}");
        assert!(!off.contains("--tray"), "{off}");
    }
}
