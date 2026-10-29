use super::*;

impl App {
    pub(super) fn detach_motor(&mut self) {
        self.motor_detached = true;
        self.motor_titlebar = None;
        if self.view == View::Motor {
            self.set_view(View::Library);
            self.lib_artist = None;
            self.lib_open = None;
        }
    }

    pub(super) fn motor_window(&mut self, ctx: &egui::Context, ab_held: &mut bool) {
        if !self.motor_detached {
            return;
        }
        let title = format!("Soundworks \u{b7} {}", crate::i18n::tr("Motor"));
        let builder = egui::ViewportBuilder::default()
            .with_title(title.clone())
            .with_inner_size([1000.0, 780.0])
            .with_min_inner_size([720.0, 540.0])
            .with_icon(Arc::new(crate::appicon::window_icon()));
        let mut closed = false;
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("motor_window"), builder, |ctx, _| {
            ctx.layer_painter(egui::LayerId::background()).rect_filled(ctx.screen_rect(), 0.0, BG0);
            if ctx.input(|i| i.key_down(egui::Key::B)) {
                *ab_held = true;
            }
            self.side_panel(ctx, 1.0);
            self.motor_view(ctx, 1.0, ab_held);
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            closed = ctx.input(|i| i.viewport().close_requested());
            if crate::devenv::dev_var("SW_CHILDCLOSE").ok().and_then(|v| v.parse::<u32>().ok()) == Some(self.frames) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            self.dev_capture(ctx, true);
        });
        if self.motor_titlebar.as_deref() != Some(title.as_str()) && shell::style_titlebar_for(&title) {
            self.motor_titlebar = Some(title);
        }
        if closed {
            self.motor_detached = false;
            self.motor_titlebar = None;
        }
    }
}
