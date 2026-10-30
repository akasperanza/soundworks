use eframe::egui::{
    self, epaint::Shadow, pos2, vec2, Color32, Context, FontData, FontDefinitions, FontFamily, FontId, Frame, Id, Margin,
    Mesh, Painter, Pos2, Rect, Response, Rounding, Sense, Shape, Stroke, Ui, Vec2,
};

pub const BG0: Color32 = Color32::from_rgb(9, 13, 22);
pub const BG1: Color32 = Color32::from_rgb(14, 20, 33);
pub const BG2: Color32 = Color32::from_rgb(21, 29, 46);
pub const BG3: Color32 = Color32::from_rgb(30, 41, 64);
pub const STROKE: Color32 = Color32::from_rgb(31, 42, 64);
pub const TEXT: Color32 = Color32::from_rgb(220, 228, 242);
pub const DIM: Color32 = Color32::from_rgb(122, 138, 166);
pub const BLUE: Color32 = Color32::from_rgb(76, 141, 255);
pub const SKY: Color32 = Color32::from_rgb(125, 178, 255);
pub const CYAN: Color32 = Color32::from_rgb(110, 170, 255);
pub const GREEN: Color32 = Color32::from_rgb(74, 196, 140);
pub const AMBER: Color32 = Color32::from_rgb(230, 176, 60);
pub const RED: Color32 = Color32::from_rgb(232, 84, 100);

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}
pub fn body(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn text_width(p: &Painter, s: &str, font: &FontId) -> f32 {
    p.layout_no_wrap(s.to_string(), font.clone(), Color32::WHITE).size().x
}

pub fn line_height(p: &Painter, font: &FontId) -> f32 {
    p.layout_no_wrap("Ag".to_string(), font.clone(), Color32::WHITE).size().y
}

pub fn wrap_lines(p: &Painter, s: &str, font: &FontId, max_w: f32) -> Vec<String> {
    let translated = crate::i18n::tr(s);
    let s = translated.trim();
    if s.is_empty() || text_width(p, s, font) <= max_w {
        return vec![s.to_string()];
    }
    let mut lines: Vec<String> = vec![];
    let mut cur = String::new();
    for word in s.split_whitespace() {
        let cand = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if text_width(p, &cand, font) <= max_w {
            cur = cand;
            continue;
        }
        if !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
        }

        let mut piece = String::new();
        for ch in word.chars() {
            piece.push(ch);
            if text_width(p, &piece, font) > max_w && piece.chars().count() > 1 {
                piece.pop();
                lines.push(std::mem::take(&mut piece));
                piece.push(ch);
            }
        }
        cur = piece;
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

pub fn draw_lines(p: &Painter, pos: Pos2, anchor: egui::Align2, lines: &[String], font: &FontId, color: Color32) -> f32 {
    let lh = line_height(p, font);
    for (i, l) in lines.iter().enumerate() {
        p.text(pos2(pos.x, pos.y + i as f32 * lh), anchor, l, font.clone(), color);
    }
    lh * lines.len() as f32
}

pub fn fit_wrap(p: &Painter, s: &str, mk: fn(f32) -> FontId, sizes: &[f32], max_w: f32, max_lines: usize) -> (FontId, Vec<String>) {
    for &sz in sizes {
        let f = mk(sz);
        let l = wrap_lines(p, s, &f, max_w);
        if l.len() <= max_lines {
            return (f, l);
        }
    }
    let f = mk(*sizes.last().unwrap_or(&12.0));
    let l = wrap_lines(p, s, &f, max_w);
    (f, l)
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgba_unmultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

pub fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

pub fn setup(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    let mut load = |key: &str, path: &str| -> bool {
        match std::fs::read(path) {
            Ok(b) => {
                fonts.font_data.insert(key.to_owned(), FontData::from_owned(b));
                true
            }
            Err(_) => false,
        }
    };
    let has_reg = load("segoe", "C:/Windows/Fonts/segoeui.ttf");
    let has_bold = load("segoe_b", "C:/Windows/Fonts/segoeuib.ttf");
    let has_sym = load("segoe_sym", "C:/Windows/Fonts/seguisym.ttf");
    {
        let prop = fonts.families.entry(FontFamily::Proportional).or_default();
        if has_reg {
            prop.insert(0, "segoe".to_owned());
        }
        if has_sym {
            prop.push("segoe_sym".to_owned());
        }
    }
    let mut bold_fam: Vec<String> = vec![];
    if has_bold {
        bold_fam.push("segoe_b".into());
    }
    bold_fam.extend(fonts.families[&FontFamily::Proportional].clone());
    fonts.families.insert(FontFamily::Name("bold".into()), bold_fam);
    fonts.font_data.insert("barlow_sb".to_owned(), FontData::from_static(include_bytes!("../assets/fonts/Barlow-SemiBold.ttf")));
    fonts.families.insert(FontFamily::Name("wordmark".into()), vec!["barlow_sb".to_owned()]);
    ctx.set_fonts(fonts);

    ctx.set_theme(egui::ThemePreference::Dark);
    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(TEXT);
    v.panel_fill = BG0;
    v.window_fill = BG1;
    v.extreme_bg_color = BG0;
    v.faint_bg_color = BG1;
    v.window_rounding = Rounding::same(10.0);
    v.menu_rounding = Rounding::same(10.0);
    v.window_stroke = Stroke::new(1.0_f32, STROKE);
    v.popup_shadow = Shadow { offset: vec2(0.0, 8.0), blur: 24.0, spread: 0.0, color: Color32::from_black_alpha(140) };
    v.selection.bg_fill = BLUE;
    v.selection.stroke = Stroke::new(1.0_f32, SKY);
    v.slider_trailing_fill = true;
    v.widgets.noninteractive.bg_fill = BG1;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, STROKE);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, DIM);
    v.widgets.noninteractive.rounding = Rounding::same(8.0);
    v.widgets.inactive.bg_fill = BG3;
    v.widgets.inactive.weak_bg_fill = BG2;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, STROKE);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    v.widgets.inactive.rounding = Rounding::same(8.0);
    v.widgets.hovered.bg_fill = BG3;
    v.widgets.hovered.weak_bg_fill = BG3;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, BLUE);
    v.widgets.hovered.fg_stroke = Stroke::new(1.5_f32, Color32::WHITE);
    v.widgets.hovered.rounding = Rounding::same(8.0);
    v.widgets.active.bg_fill = BLUE;
    v.widgets.active.weak_bg_fill = BLUE;
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, SKY);
    v.widgets.active.fg_stroke = Stroke::new(1.5_f32, Color32::WHITE);
    v.widgets.active.rounding = Rounding::same(8.0);
    v.widgets.open.bg_fill = BG2;
    v.widgets.open.weak_bg_fill = BG2;
    v.widgets.open.rounding = Rounding::same(10.0);

    v.widgets.noninteractive.bg_stroke = Stroke::NONE;
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.interaction.selectable_labels = false;
        s.interaction.multi_widget_text_select = false;
    });

    ctx.style_mut(|s| {
        s.spacing.item_spacing = vec2(12.0, 10.0);
        s.spacing.button_padding = vec2(14.0, 7.0);
        s.spacing.slider_rail_height = 6.0;
        s.spacing.interact_size.y = 28.0;
        s.spacing.scroll.bar_width = 6.0;
        use egui::TextStyle::*;
        s.text_styles.insert(Heading, bold(20.0));
        s.text_styles.insert(Body, body(14.0));
        s.text_styles.insert(Button, body(14.0));
        s.text_styles.insert(Small, body(11.5));
        s.text_styles.insert(Monospace, FontId::monospace(13.0));
    });
}

pub fn radial_glow(painter: &Painter, center: Pos2, radius: f32, color: Color32) {
    let mut mesh = Mesh::default();
    let n = 48;
    mesh.colored_vertex(center, color);
    for i in 0..=n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        mesh.colored_vertex(center + vec2(a.cos(), a.sin()) * radius, alpha(color, 0));
    }
    for i in 0..n {
        mesh.add_triangle(0, 1 + i as u32, 2 + i as u32);
    }
    painter.add(Shape::mesh(mesh));
}

pub fn rounded_image(painter: &Painter, tex: egui::TextureId, rect: Rect, radius: f32, tint: Color32) {
    rounded_image_uv(painter, tex, rect, radius, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
}

pub fn cover_uv(img_w: f32, img_h: f32, rect: Rect) -> Rect {
    let ra = rect.width() / rect.height();
    let ia = img_w / img_h;
    if ia > ra {
        let w = ra / ia;
        Rect::from_min_max(pos2((1.0 - w) * 0.5, 0.0), pos2((1.0 + w) * 0.5, 1.0))
    } else {
        let h = ia / ra;
        Rect::from_min_max(pos2(0.0, (1.0 - h) * 0.5), pos2(1.0, (1.0 + h) * 0.5))
    }
}

pub fn rounded_image_uv(painter: &Painter, tex: egui::TextureId, rect: Rect, radius: f32, uvr: Rect, tint: Color32) {
    let mut mesh = Mesh::with_texture(tex);
    let uv = |p: Pos2| {
        pos2(
            uvr.left() + (p.x - rect.left()) / rect.width() * uvr.width(),
            uvr.top() + (p.y - rect.top()) / rect.height() * uvr.height(),
        )
    };
    let c = rect.center();
    mesh.vertices.push(egui::epaint::Vertex { pos: c, uv: uv(c), color: tint });
    let r = radius.min(rect.width() * 0.5).min(rect.height() * 0.5);
    let corners = [
        (pos2(rect.right() - r, rect.top() + r), -90.0f32),
        (pos2(rect.right() - r, rect.bottom() - r), 0.0),
        (pos2(rect.left() + r, rect.bottom() - r), 90.0),
        (pos2(rect.left() + r, rect.top() + r), 180.0),
    ];
    let steps = 8;
    for (cc, start) in corners {
        for s in 0..=steps {
            let a = (start + 90.0 * s as f32 / steps as f32).to_radians();
            let p = cc + vec2(a.cos(), a.sin()) * r;
            mesh.vertices.push(egui::epaint::Vertex { pos: p, uv: uv(p), color: tint });
        }
    }
    let n = (mesh.vertices.len() - 1) as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    painter.add(Shape::mesh(mesh));
}

pub fn circle_image(painter: &Painter, tex: egui::TextureId, center: Pos2, radius: f32, uvr: Rect, tint: Color32) {
    let mut mesh = Mesh::with_texture(tex);
    let rect = Rect::from_center_size(center, Vec2::splat(radius * 2.0));
    let uv = |p: Pos2| {
        pos2(
            uvr.left() + (p.x - rect.left()) / rect.width() * uvr.width(),
            uvr.top() + (p.y - rect.top()) / rect.height() * uvr.height(),
        )
    };
    mesh.vertices.push(egui::epaint::Vertex { pos: center, uv: uv(center), color: tint });
    let n = 64u32;
    for i in 0..=n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        let p = center + vec2(a.cos(), a.sin()) * radius;
        mesh.vertices.push(egui::epaint::Vertex { pos: p, uv: uv(p), color: tint });
    }
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 2 + i);
    }
    painter.add(Shape::mesh(mesh));
}

pub fn vgradient(painter: &Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

pub fn panel_frame(inner: Margin, outer: Margin) -> Frame {
    Frame::none().fill(alpha(BG1, 150)).rounding(Rounding::same(14.0)).inner_margin(inner).outer_margin(outer)
}

pub fn raised_frame(inner: Margin) -> Frame {
    Frame::none().fill(alpha(BG2, 200)).rounding(Rounding::same(14.0)).inner_margin(inner)
}

pub fn raised_shadow(ui: &Ui) {
    let rect = ui.max_rect();
    let painter = ui.painter().with_clip_rect(ui.ctx().screen_rect());
    let ambient = egui::epaint::Shadow { offset: vec2(0.0, 18.0), blur: 44.0, spread: 0.0, color: Color32::from_black_alpha(190) };
    let contact = egui::epaint::Shadow { offset: vec2(0.0, 2.0), blur: 8.0, spread: 0.0, color: Color32::from_black_alpha(120) };
    painter.add(ambient.as_shape(rect, Rounding::same(14.0)));
    painter.add(contact.as_shape(rect, Rounding::same(14.0)));
}

pub fn raised_surface(ui: &Ui, inner: Margin) {
    let r = ui.max_rect();
    let panel = Rect::from_min_max(pos2(r.left() - inner.left, r.top() - inner.top), pos2(r.right() + inner.right, r.bottom() + inner.bottom));
    ui.painter().rect_stroke(panel.shrink(0.5), Rounding::same(14.0), Stroke::new(1.0_f32, alpha(Color32::WHITE, 16)));
    let (x0, x1, y) = (panel.left() + 14.0, panel.right() - 14.0, panel.top() + 1.0);
    let mid = (x0 + x1) * 0.5;
    let mut mesh = Mesh::default();
    for (x, a) in [(x0, 0u8), (mid, 78), (x1, 0)] {
        mesh.colored_vertex(pos2(x, y - 0.5), alpha(Color32::WHITE, a));
        mesh.colored_vertex(pos2(x, y + 0.5), alpha(Color32::WHITE, a));
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    mesh.add_triangle(2, 3, 4);
    mesh.add_triangle(3, 5, 4);
    ui.painter().add(Shape::mesh(mesh));
}

pub fn default_backdrop() -> egui::ColorImage {
    let n = 64usize;
    let mut px = Vec::with_capacity(n * n * 4);
    for y in 0..n {
        for x in 0..n {
            let (fx, fy) = (x as f32 / n as f32, y as f32 / n as f32);
            let glow = |cx: f32, cy: f32, r: f32| (1.0 - ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt() / r).clamp(0.0, 1.0).powi(2);
            let g1 = glow(0.15, -0.05, 0.95);
            let g2 = glow(0.95, 0.35, 0.7);
            let r = 9.0 + 6.0 * g1 + 18.0 * g2;
            let g = 13.0 + 26.0 * g1 + 8.0 * g2;
            let b = 22.0 + 70.0 * g1 + 40.0 * g2;
            px.extend_from_slice(&[r as u8, g as u8, b as u8, 255]);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([n, n], &px)
}

pub fn blurred_backdrop(w: usize, h: usize, rgba: &[u8]) -> Option<egui::ColorImage> {
    let img = image::RgbaImage::from_raw(w as u32, h as u32, rgba.to_vec())?;
    let small = image::imageops::resize(&img, 64, 64, image::imageops::FilterType::Triangle);
    let mut soft = image::imageops::blur(&small, 3.5);
    for p in soft.pixels_mut() {
        let lum = 0.3 * p[0] as f32 + 0.59 * p[1] as f32 + 0.11 * p[2] as f32;
        for c in 0..3 {
            p[c] = (lum + (p[c] as f32 - lum) * 1.35).clamp(0.0, 255.0) as u8;
        }
        p[3] = 255;
    }
    Some(egui::ColorImage::from_rgba_unmultiplied([64, 64], soft.as_raw()))
}

pub fn panel_glow(painter: &Painter, panel: Rect, radius: f32, height: f32, color: Color32, a: f32) {
    if a < 1.0 {
        return;
    }
    let h = height.min(panel.height());
    let mut ys: Vec<f32> = (0..=10).map(|k| radius * k as f32 / 10.0).collect();
    ys.extend((1..=24).map(|k| radius + (h - radius).max(0.0) * k as f32 / 24.0));
    let mut mesh = Mesh::default();
    for (k, dy) in ys.iter().enumerate() {
        let dy = *dy;
        let f = (dy / h).clamp(0.0, 1.0);
        let y = panel.top() + dy;
        let inset = if dy < radius { radius - (radius * radius - (radius - dy) * (radius - dy)).max(0.0).sqrt() } else { 0.0 };
        let col = alpha(color, (a * (1.0 - f) * (1.0 - f * 0.35)) as u8);
        mesh.colored_vertex(pos2(panel.left() + inset, y), col);
        mesh.colored_vertex(pos2(panel.right() - inset, y), col);
        if k > 0 {
            let i = (k * 2) as u32;
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i - 1, i + 1, i);
        }
    }
    painter.add(Shape::mesh(mesh));
}

pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    Frame::none()
        .fill(BG2)
        .rounding(Rounding::same(12.0))
        .inner_margin(Margin::same(22.0))
        .show(ui, add)
}

pub fn section_label(ui: &mut Ui, text: &str) {
    let text = &crate::i18n::tr(text);
    ui.label(egui::RichText::new(text.to_uppercase()).font(bold(11.0)).color(DIM));
}

pub fn chip(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let text = &crate::i18n::tr(text);
    let g = ui.painter().layout_no_wrap(text.to_string(), bold(11.0), color);
    let size = g.size() + vec2(12.0, 6.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, Rounding::same(5.0), alpha(color, 26));
    ui.painter().galley(rect.center() - g.size() * 0.5, g, color);
    resp
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    Play,
    Pause,
    Prev,
    Next,
    Repeat,
    Volume,
    Plus,
    Close,
    Trash,
    Power,
    List,
    Import,
    Undo,
    Save,
    Folder,
    Spectrum,
    Spectrogram,
    Scope,
    Wave,
    Search,
    User,
    ChevronDown,
    ChevronUp,
    ChevronRight,
    Shuffle,
    Repeat1,
    VolumeX,
    Volume1,
    Track,
    Upload,
    Lyrics,
    Settings,
    Headphones,
    Maximize,
    Puzzle,
}

fn icon_svg(i: Icon) -> &'static str {
    match i {
        Icon::Play => include_str!("../assets/icons/play.svg"),
        Icon::Pause => include_str!("../assets/icons/pause.svg"),
        Icon::Prev => include_str!("../assets/icons/skip-back.svg"),
        Icon::Next => include_str!("../assets/icons/skip-forward.svg"),
        Icon::Repeat => include_str!("../assets/icons/repeat.svg"),
        Icon::Volume => include_str!("../assets/icons/volume-2.svg"),
        Icon::Plus => include_str!("../assets/icons/plus.svg"),
        Icon::Close => include_str!("../assets/icons/x.svg"),
        Icon::Trash => include_str!("../assets/icons/trash.svg"),
        Icon::Power => include_str!("../assets/icons/power.svg"),
        Icon::List => include_str!("../assets/icons/list.svg"),
        Icon::Import => include_str!("../assets/icons/download.svg"),
        Icon::Undo => include_str!("../assets/icons/undo-2.svg"),
        Icon::Save => include_str!("../assets/icons/save.svg"),
        Icon::Folder => include_str!("../assets/icons/folder-open.svg"),
        Icon::Spectrum => include_str!("../assets/icons/chart-no-axes-column.svg"),
        Icon::Spectrogram => include_str!("../assets/icons/grid-3x3.svg"),
        Icon::Scope => include_str!("../assets/icons/radar.svg"),
        Icon::Wave => include_str!("../assets/icons/audio-lines.svg"),
        Icon::Search => include_str!("../assets/icons/search.svg"),
        Icon::User => include_str!("../assets/icons/user-round.svg"),
        Icon::ChevronDown => include_str!("../assets/icons/chevron-down.svg"),
        Icon::ChevronUp => include_str!("../assets/icons/chevron-up.svg"),
        Icon::ChevronRight => include_str!("../assets/icons/chevron-right.svg"),
        Icon::Shuffle => include_str!("../assets/icons/shuffle.svg"),
        Icon::Repeat1 => include_str!("../assets/icons/repeat-1.svg"),
        Icon::VolumeX => include_str!("../assets/icons/volume-x.svg"),
        Icon::Volume1 => include_str!("../assets/icons/volume-1.svg"),
        Icon::Track => include_str!("../assets/icons/file-music.svg"),
        Icon::Upload => include_str!("../assets/icons/upload.svg"),
        Icon::Lyrics => include_str!("../assets/icons/mic-vocal.svg"),
        Icon::Settings => include_str!("../assets/icons/settings.svg"),
        Icon::Headphones => include_str!("../assets/icons/headphones.svg"),
        Icon::Maximize => include_str!("../assets/icons/maximize.svg"),
        Icon::Puzzle => include_str!("../assets/icons/puzzle.svg"),
    }
}

static ICON_CACHE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<Icon, egui::TextureHandle>>> =
    std::sync::OnceLock::new();

fn icon_texture(ctx: &Context, icon: Icon) -> egui::TextureId {
    let cache = ICON_CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut map = cache.lock().unwrap();
    if let Some(t) = map.get(&icon) {
        return t.id();
    }
    let mut svg = icon_svg(icon).replace("currentColor", "#ffffff");
    if matches!(icon, Icon::Play | Icon::Pause | Icon::Prev | Icon::Next) {
        svg = svg.replace("fill=\"none\"", "fill=\"#ffffff\"");
    }
    let px = 96u32;
    let mut pix = resvg::tiny_skia::Pixmap::new(px, px).expect("pixmap");
    if let Ok(tree) = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()) {
        let k = px as f32 / 24.0;
        resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(k, k), &mut pix.as_mut());
    }
    let img = egui::ColorImage::from_rgba_premultiplied([px as usize, px as usize], pix.data());
    let tex = ctx.load_texture(format!("lucide_{}", map.len()), img, egui::TextureOptions::LINEAR);
    let id = tex.id();
    map.insert(icon, tex);
    id
}

pub fn draw_icon(p: &Painter, icon: Icon, c: Pos2, s: f32, col: Color32) {
    let tex = icon_texture(p.ctx(), icon);
    let rect = Rect::from_center_size(c, Vec2::splat(s * 1.3));
    p.image(tex, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), col);
}

pub fn icon_button(ui: &mut Ui, icon: Icon, size: f32, active: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let t = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.12);
    let a = ui.ctx().animate_bool_with_time(resp.id.with("a"), active, 0.18);
    ui.painter().rect_filled(rect, Rounding::same(7.0), lerp_color(alpha(BG3, 0), BG3, t.max(a * 0.7)));
    let col = lerp_color(lerp_color(DIM, TEXT, t), BLUE, a);
    draw_icon(ui.painter(), icon, rect.center(), size * 0.5, col);
    resp
}

pub fn play_button(ui: &mut Ui, playing: bool, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let t = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.12);
    let c = rect.center();
    ui.painter().circle_filled(c, size * 0.5, lerp_color(BLUE, SKY, 0.25 * t));
    draw_icon(ui.painter(), if playing { Icon::Pause } else { Icon::Play }, c, size * 0.46, Color32::WHITE);
    resp
}

pub fn pill_button(ui: &mut Ui, text: &str, icon: Option<Icon>, primary: bool) -> Response {
    let text = &crate::i18n::tr(text);
    let g = ui.painter().layout_no_wrap(text.to_string(), if primary { bold(13.0) } else { body(13.5) }, TEXT);
    let icon_w = if icon.is_some() { 20.0 } else { 0.0 };
    let size = vec2(g.size().x + icon_w + 28.0, 32.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let en = ui.is_enabled();
    let t = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered() && en, 0.12);
    let (bg, fg, stroke) = if primary {
        (lerp_color(BLUE, SKY, 0.2 * t), Color32::WHITE, Color32::TRANSPARENT)
    } else {
        (lerp_color(BG2, BG3, t), lerp_color(TEXT, Color32::WHITE, t), Color32::TRANSPARENT)
    };
    let (bg, fg) = if en { (bg, fg) } else { (alpha(bg, 110), alpha(fg, 100)) };
    ui.painter().rect_filled(rect, Rounding::same(16.0), bg);
    if stroke != Color32::TRANSPARENT {
        ui.painter().rect_stroke(rect, Rounding::same(16.0), Stroke::new(1.0_f32, stroke));
    }
    let mut x = rect.left() + 14.0;
    if let Some(ic) = icon {
        draw_icon(ui.painter(), ic, pos2(x + 7.0, rect.center().y), 12.5, fg);
        x += icon_w;
    }
    ui.painter().galley(pos2(x, rect.center().y - g.size().y * 0.5), g, fg);
    resp
}

pub fn locked_pill_button(ui: &mut Ui, text: &str, suffix: &str, locked: f32) -> Response {
    let text = &crate::i18n::tr(text);
    let font = body(13.5);

    let wide = ui.painter().layout_no_wrap(format!("{text}  {suffix}"), font.clone(), TEXT);
    let size = vec2(wide.size().x + 28.0, 32.0);
    let open = locked <= 0.0;
    let (rect, resp) = ui.allocate_exact_size(size, if open { Sense::click() } else { Sense::hover() });
    let t = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered() && open, 0.12);
    let rounding = Rounding::same(16.0);
    ui.painter().rect_filled(rect, rounding, lerp_color(BG2, BG3, t));
    if !open {
        let fill = Rect::from_min_size(rect.min, vec2(rect.width() * locked.clamp(0.0, 1.0), rect.height()));
        ui.painter().with_clip_rect(rect).rect_filled(fill, rounding, alpha(RED, 150));
    }
    let label = if open { text.to_string() } else { format!("{text}  {suffix}") };
    let g = ui.painter().layout_no_wrap(label, font, if open { lerp_color(TEXT, Color32::WHITE, t) } else { alpha(Color32::WHITE, 235) });
    ui.painter().galley(rect.center() - g.size() * 0.5, g, Color32::WHITE);
    resp
}

pub fn tab(ui: &mut Ui, text: &str, selected: bool) -> Response {
    let text = &crate::i18n::tr(text);
    let g = ui.painter().layout_no_wrap(text.to_string(), body(14.5), TEXT);
    let size = vec2(g.size().x + 24.0, 32.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let t = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.12);
    let s = ui.ctx().animate_bool_with_time(resp.id.with("s"), selected, 0.2);
    let col = lerp_color(lerp_color(DIM, TEXT, t), Color32::WHITE, s);
    ui.painter().galley(rect.center() - g.size() * 0.5, g, col);
    let w = (rect.width() - 24.0) * ease(s);
    ui.painter().rect_filled(
        Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 1.0), vec2(w, 2.0)),
        Rounding::same(1.0),
        BLUE,
    );
    resp
}

pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let size = vec2(42.0, 24.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.18);
    let en = ui.is_enabled();
    let bg = lerp_color(BG3, BLUE, t);
    let bg = if en { bg } else { alpha(bg, 110) };
    ui.painter().rect_filled(rect, Rounding::same(12.0), bg);
    let x = rect.left() + 12.0 + t * (rect.width() - 24.0);
    ui.painter().circle_filled(pos2(x, rect.center().y), 9.0 + resp.hovered() as i32 as f32 * 0.8, Color32::WHITE);
    resp
}

pub fn switch_row(ui: &mut Ui, label: &str, hint: &str, on: &mut bool) -> bool {
    let label = &crate::i18n::tr(label);
    let hint = &crate::i18n::tr(hint);
    let mut changed = false;
    ui.horizontal(|ui| {
        changed = switch(ui, on).changed();
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(egui::RichText::new(label).font(body(14.5)).color(Color32::WHITE));
            if !hint.is_empty() {
                ui.label(egui::RichText::new(hint).font(body(12.0)).color(DIM));
            }
        });
    });
    changed
}

pub fn view_switch<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, items: &[(T, &str)]) {
    let gs: Vec<_> = items.iter().map(|(_, t)| ui.painter().layout_no_wrap(crate::i18n::tr(t), bold(13.5), TEXT)).collect();
    let ws: Vec<f32> = gs.iter().map(|g| g.size().x + 30.0).collect();
    let (rect, _) = ui.allocate_exact_size(vec2(ws.iter().sum::<f32>() + 6.0, 34.0), Sense::hover());

    ui.painter().rect_filled(rect, Rounding::same(9.0), BG2);
    let mut x = rect.left() + 3.0;
    let sel = items.iter().position(|(v, _)| *v == *value).unwrap_or(0);
    let (mut tx, mut tw) = (rect.left() + 3.0, ws[0]);
    for (i, w) in ws.iter().enumerate() {
        if i == sel {
            tw = *w;
            break;
        }
        tx += *w;
    }
    let id = ui.id().with("vswitch");
    let ix = ui.ctx().animate_value_with_time(id.with("x"), tx, 0.2);
    let iw = ui.ctx().animate_value_with_time(id.with("w"), tw, 0.2);
    let ind = Rect::from_min_size(pos2(ix, rect.top() + 3.0), vec2(iw, rect.height() - 6.0));
    ui.painter().rect_filled(ind, Rounding::same(7.0), BG3);
    ui.painter().rect_stroke(ind, Rounding::same(7.0), Stroke::new(1.0_f32, alpha(Color32::WHITE, 20)));
    for (i, g) in gs.into_iter().enumerate() {
        let r = Rect::from_min_size(pos2(x, rect.top() + 3.0), vec2(ws[i], rect.height() - 6.0));
        let resp = ui.interact(r, id.with(("t", i)), Sense::click());
        let h = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.12);
        let on = i == sel;
        let col = if on { Color32::WHITE } else { lerp_color(DIM, TEXT, h) };
        ui.painter().galley(r.center() - g.size() * 0.5, g, col);
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            *value = items[i].0;
        }
        x += ws[i];
    }
}

pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, items: &[(T, &str, Option<Icon>)]) {
    let mut total = 4.0;
    let gs: Vec<_> = items.iter().map(|(_, t, _)| ui.painter().layout_no_wrap(crate::i18n::tr(t), bold(12.0), TEXT)).collect();
    for (g, (_, _, ic)) in gs.iter().zip(items) {
        total += g.size().x + 22.0 + if ic.is_some() { 18.0 } else { 0.0 };
    }
    let (rect, _) = ui.allocate_exact_size(vec2(total, 28.0), Sense::hover());
    ui.painter().rect_filled(rect, Rounding::same(8.0), BG2);
    let mut x = rect.left() + 2.0;

    let base = ui.id().with(("seg", items.iter().map(|(_, t, _)| *t).collect::<Vec<_>>().join("|")));
    for (g, (v, _, ic)) in gs.into_iter().zip(items) {
        let w = g.size().x + 22.0 + if ic.is_some() { 18.0 } else { 0.0 };
        let r = Rect::from_min_size(pos2(x, rect.top() + 2.0), vec2(w, rect.height() - 4.0));
        let resp = ui.interact(r, base.with(x as i32), Sense::click());
        let sel = *value == *v;
        let s = ui.ctx().animate_bool_with_time(resp.id.with("s"), sel, 0.18);
        let h = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.12);
        ui.painter().rect_filled(r, Rounding::same(6.0), lerp_color(alpha(BG3, (70.0 * h) as u8), BG3, s));
        let col = lerp_color(lerp_color(DIM, TEXT, h), Color32::WHITE, s);
        let mut tx = r.left() + 11.0;
        if let Some(i) = ic {
            draw_icon(ui.painter(), *i, pos2(tx + 7.0, r.center().y), 14.0, col);
            tx += 18.0;
        }
        ui.painter().galley(pos2(tx, r.center().y - g.size().y * 0.5), g, col);
        if resp.clicked() {
            *value = *v;
        }
        x += w;
    }
}

pub fn status_dot(ui: &mut Ui, on: bool, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, if on { color } else { alpha(DIM, 110) });
}

pub fn playing_bars(ui: &mut Ui, playing: bool, size: Vec2) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let time = ui.input(|i| i.time) as f32;
    let n = 4;
    let w = rect.width() / (n as f32 * 1.6);
    for i in 0..n {
        let phase = i as f32 * 1.7;
        let h = if playing {
            0.25 + 0.75 * (0.5 + 0.5 * (time * (4.0 + i as f32 * 1.3) + phase).sin())
        } else {
            0.2
        };
        let x = rect.left() + i as f32 * w * 1.6 + w * 0.3;
        let r = Rect::from_min_max(pos2(x, rect.bottom() - rect.height() * h), pos2(x + w, rect.bottom()));
        ui.painter().rect_filled(r, Rounding::same(w * 0.5), lerp_color(DIM, SKY, if playing { 1.0 } else { 0.0 }));
    }
    if playing {

        ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
    }
}

pub fn toast(ctx: &Context, text: &str, age: f32) {
    let text = &crate::i18n::tr(text);
    let a = if age < 0.15 { age / 0.15 } else if age > 2.4 { (2.8 - age) / 0.4 } else { 1.0 }.clamp(0.0, 1.0);
    egui::Area::new(Id::new("toast")).anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -70.0 + 8.0 * (1.0 - a))).interactable(false).show(ctx, |ui| {
        ui.set_opacity(a);
        Frame::none()
            .fill(BG3)
            .stroke(Stroke::new(1.0_f32, STROKE))
            .rounding(Rounding::same(8.0))
            .inner_margin(Margin::symmetric(14.0, 8.0))
            .show(ui, |ui| {
                ui.label(egui::RichText::new(text).font(bold(13.0)));
            });
    });
}

pub fn appear(ctx: &Context, id: Id, delay: f32) -> f32 {
    let key = id.with("seen_at");
    let now = ctx.input(|i| i.time);
    let t0 = ctx.data_mut(|d| *d.get_temp_mut_or_insert_with(key, || now));
    let t = ((now - t0 - delay as f64) / 0.38).clamp(0.0, 1.0) as f32;
    if t < 1.0 {
        ctx.request_repaint();
    }
    ease(t)
}

pub struct RichText;

#[allow(clippy::new_ret_no_self)]
impl RichText {
    pub fn new(text: impl Into<String>) -> egui::RichText {
        egui::RichText::new(crate::i18n::tr(text.into()))
    }
}

pub trait ResponseTr {
    fn on_hover_text_tr(self, text: impl AsRef<str>) -> Self;
    fn on_disabled_hover_text_tr(self, text: impl AsRef<str>) -> Self;
}

impl ResponseTr for Response {
    fn on_hover_text_tr(self, text: impl AsRef<str>) -> Self {
        self.on_hover_text(crate::i18n::tr(text))
    }
    fn on_disabled_hover_text_tr(self, text: impl AsRef<str>) -> Self {
        self.on_disabled_hover_text(crate::i18n::tr(text))
    }
}

pub trait TextEditTr {
    fn hint_text_tr(self, text: impl AsRef<str>) -> Self;
}

impl TextEditTr for egui::TextEdit<'_> {
    fn hint_text_tr(self, text: impl AsRef<str>) -> Self {
        self.hint_text(crate::i18n::tr(text))
    }
}

pub trait ComboBoxTr {
    fn selected_text_tr(self, text: impl AsRef<str>) -> Self;
}

impl ComboBoxTr for egui::ComboBox {
    fn selected_text_tr(self, text: impl AsRef<str>) -> Self {
        self.selected_text(crate::i18n::tr(text))
    }
}

pub trait UiTr {
    fn button_tr(&mut self, text: impl AsRef<str>) -> Response;
    fn selectable_value_tr<V: PartialEq>(&mut self, current: &mut V, selected: V, text: impl AsRef<str>) -> Response;
}

impl UiTr for Ui {
    fn button_tr(&mut self, text: impl AsRef<str>) -> Response {
        self.button(crate::i18n::tr(text))
    }
    fn selectable_value_tr<V: PartialEq>(&mut self, current: &mut V, selected: V, text: impl AsRef<str>) -> Response {
        self.selectable_value(current, selected, crate::i18n::tr(text))
    }
}

pub trait PainterTr {
    fn text_tr(&self, pos: Pos2, anchor: egui::Align2, text: impl ToString, font: egui::FontId, color: Color32) -> Rect;
}

impl PainterTr for Painter {
    fn text_tr(&self, pos: Pos2, anchor: egui::Align2, text: impl ToString, font: egui::FontId, color: Color32) -> Rect {
        self.text(pos, anchor, crate::i18n::tr(text.to_string()), font, color)
    }
}


pub fn row_sep(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, alpha(Color32::WHITE, 12));
}

pub fn setting_row(ui: &mut Ui, title: &str, desc: &str, info: &str, control: impl FnOnce(&mut Ui)) {
    let (title, desc, info) = (crate::i18n::tr(title), crate::i18n::tr(desc), crate::i18n::tr(info));
    let w = ui.available_width();
    let left_w = (w * 0.56).clamp(160.0, 440.0);
    let info_w = if info.is_empty() { 0.0 } else { 24.0 };
    let gt = ui.painter().layout(title.clone(), bold(14.0), Color32::WHITE, left_w - info_w);
    let gd = (!desc.is_empty()).then(|| ui.painter().layout(desc, body(12.0), DIM, left_w));
    let text_h = gt.size().y + gd.as_ref().map_or(0.0, |g| 3.0 + g.size().y);
    let h = (text_h + 18.0).max(46.0);
    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let top = rect.center().y - text_h * 0.5;
    let title_w = gt.size().x;
    let title_h = gt.size().y;
    let first_row_h = gt.rows.first().map_or(18.0, |r| r.rect.height());
    ui.painter().galley(pos2(rect.left(), top), gt, Color32::WHITE);
    if let Some(g) = gd {
        let y = top + title_h + 3.0;
        ui.painter().galley(pos2(rect.left(), y), g, DIM);
    }
    if !info.is_empty() {
        let c = pos2(rect.left() + title_w + 16.0, top + first_row_h * 0.5);
        let ir = Rect::from_center_size(c, vec2(20.0, 20.0));
        let resp = ui.interact(ir, ui.id().with(("setting_info", &title)), Sense::hover());
        let a = if resp.hovered() { 230 } else { 120 };
        ui.painter().circle_stroke(c, 7.5, Stroke::new(1.0_f32, alpha(DIM, a)));
        ui.painter().text(c, egui::Align2::CENTER_CENTER, "i", bold(10.0), alpha(DIM, a));
        resp.on_hover_text(info);
    }
    let cr = Rect::from_min_max(pos2((rect.left() + left_w + 12.0).min(rect.right() - 40.0), rect.top()), rect.right_bottom());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(cr).layout(egui::Layout::right_to_left(egui::Align::Center)));
    control(&mut child);
}

pub fn toggle_row(ui: &mut Ui, title: &str, desc: &str, info: &str, on: &mut bool) -> bool {
    let mut changed = false;
    setting_row(ui, title, desc, info, |ui| {
        changed = switch(ui, on).changed();
    });
    changed
}

pub fn setting_block(ui: &mut Ui, title: &str, desc: &str, add: impl FnOnce(&mut Ui)) {
    ui.add_space(12.0);
    ui.label(egui::RichText::new(crate::i18n::tr(title)).font(bold(14.0)).color(Color32::WHITE));
    if !desc.is_empty() {
        ui.add_space(2.0);
        ui.add(egui::Label::new(egui::RichText::new(crate::i18n::tr(desc)).font(body(12.0)).color(DIM)).wrap());
    }
    ui.add_space(8.0);
    add(ui);
    ui.add_space(12.0);
}

pub fn nav_item(ui: &mut Ui, text: &str, icon: Icon, selected: bool) -> Response {
    let g = ui.painter().layout_no_wrap(crate::i18n::tr(text), bold(14.0), TEXT);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    let h = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.12);
    let s = ui.ctx().animate_bool_with_time(resp.id.with("s"), selected, 0.18);
    if s > 0.01 {
        ui.painter().rect_filled(rect, Rounding::same(10.0), alpha(BLUE, (34.0 * s) as u8));
    } else if h > 0.01 {
        ui.painter().rect_filled(rect, Rounding::same(10.0), alpha(Color32::WHITE, (12.0 * h) as u8));
    }
    let col = lerp_color(lerp_color(DIM, TEXT, h), Color32::WHITE, s);
    draw_icon(ui.painter(), icon, pos2(rect.left() + 22.0, rect.center().y), 14.0, lerp_color(col, SKY, s));
    ui.painter().galley(pos2(rect.left() + 42.0, rect.center().y - g.size().y * 0.5), g, col);
    resp
}

pub fn collapsible(ui: &mut Ui, id: &str, label: &str, add: impl FnOnce(&mut Ui)) {
    let key = ui.id().with(("collapsible", id));
    let open = ui.ctx().data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
    ui.add_space(4.0);
    let g = ui.painter().layout_no_wrap(crate::i18n::tr(label), bold(13.0), DIM);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
    let h = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.12);
    let col = lerp_color(DIM, TEXT, h);
    draw_icon(ui.painter(), if open { Icon::ChevronUp } else { Icon::ChevronDown }, pos2(rect.left() + 10.0, rect.center().y), 12.0, col);
    ui.painter().galley(pos2(rect.left() + 28.0, rect.center().y - g.size().y * 0.5), g, col);
    if resp.clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(key, !open));
    }
    if open {
        add(ui);
    }
}

#[cfg(test)]
mod wrap_tests {
    use super::*;

    fn painter() -> Painter {
        let ctx = egui::Context::default();

        let _ = ctx.run(Default::default(), |_| {});
        Painter::new(ctx, egui::LayerId::background(), Rect::EVERYTHING)
    }

    #[test]
    fn wrapping_never_loses_text_and_respects_width() {
        let p = painter();
        let f = body(14.0);
        let title = "Symphony No. 5 in C Minor, Op. 67: I. Allegro con brio (Live at the Musikverein, Wien)";
        let w = 180.0;
        let lines = wrap_lines(&p, title, &f, w);
        assert!(lines.len() > 1);
        assert_eq!(lines.join(" "), title);
        for l in &lines {
            assert!(text_width(&p, l, &f) <= w + 0.5, "{l}");
        }

        assert_eq!(wrap_lines(&p, "Kısa", &f, w), vec!["Kısa".to_string()]);
    }

    #[test]
    fn overlong_word_is_split_not_dropped() {
        let p = painter();
        let f = body(14.0);
        let word = "Pneumonoultramicroscopicsilicovolcanoconiosis";
        let lines = wrap_lines(&p, word, &f, 90.0);
        assert!(lines.len() > 1);
        assert_eq!(lines.concat(), word);
    }

    #[test]
    fn fit_wrap_prefers_smaller_font_before_more_lines() {
        let p = painter();
        let s = "Bir iki üç dört beş altı yedi sekiz dokuz on";
        let (f, l) = fit_wrap(&p, s, body, &[14.0, 12.0, 10.0], 150.0, 2);
        assert!(l.len() <= 2 || f.size == 10.0);
        assert_eq!(l.join(" "), s);
    }

    #[test]
    fn segmented_in_separate_cards_do_not_share_clicks() {
        use egui::{Event, PointerButton, Pos2, RawInput};
        let ctx = egui::Context::default();
        setup(&ctx);
        let (mut a, mut b) = (5u32, 0u32);
        let mut frame = |ctx: &egui::Context, a: &mut u32, b: &mut u32, events: Vec<Event>| {
            let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 500.0))), events, ..Default::default() };
            let mut rects = vec![];
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    card(ui, |ui| {
                        ui.set_width(ui.available_width());
                        segmented(ui, a, &[(0u32, "Otomatik", None), (5u32, "%70", None), (7u32, "%80", None)]);
                    });
                    ui.add_space(10.0);
                    card(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let before = ui.min_rect().top();
                        let _ = before;
                        segmented(ui, b, &[(0u32, "Kapalı", None), (1u32, "Parça", None), (2u32, "Albüm", None)]);
                        rects.push(ui.min_rect());
                    });
                });
            });
            rects
        };

        let rects = frame(&ctx, &mut a, &mut b, vec![]);
        let _ = frame(&ctx, &mut a, &mut b, vec![]);
        let r = rects[0];

        let click = |ctx: &egui::Context, a: &mut u32, b: &mut u32, p: Pos2| {
            frame(ctx, a, b, vec![Event::PointerMoved(p)]);
            frame(ctx, a, b, vec![Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
            frame(ctx, a, b, vec![Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
        };
        let y = r.center().y;
        click(&ctx, &mut a, &mut b, pos2(r.left() + 80.0, y));
        assert_eq!(b, 1, "ikinci karttaki 'Parça' seçilmeli");
        assert_eq!(a, 5, "birinci kart etkilenmemeli");

        click(&ctx, &mut a, &mut b, pos2(r.left() + 20.0, y));
        assert_eq!(b, 0, "'Kapalı' geri seçilebilmeli");
        assert_eq!(a, 5, "birinci kartın 'Otomatik'i tetiklenmemeli");
    }
}

#[cfg(test)]
mod translation_helper_tests {
    use super::*;

    #[test]
    fn translating_widget_helpers_terminate_and_return_widgets() {
        crate::i18n::set(crate::i18n::Lang::En);
        let ctx = Context::default();
        let mut picked: Option<String> = None;
        let mut text = String::new();
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.selectable_value_tr(&mut picked, None, "Tüm türler");
                ui.selectable_value_tr(&mut picked, Some("Rock".to_string()), "Rock");
                ui.button_tr("Kaydet").on_hover_text_tr("Kaydet").on_disabled_hover_text_tr("Kaydet");
                ui.add(egui::TextEdit::singleline(&mut text).hint_text_tr("Sonuç yok"));
                egui::ComboBox::from_id_salt("t").selected_text_tr("Tüm türler").show_ui(ui, |ui| {
                    ui.selectable_value_tr(&mut picked, None, "Tüm türler");
                });
                ui.label(RichText::new("Kaydet"));
                ui.painter().text_tr(pos2(0.0, 0.0), egui::Align2::LEFT_TOP, "Kaydet", egui::FontId::default(), Color32::WHITE);
            });
        });
        crate::i18n::set(crate::i18n::Lang::Tr);
        assert!(!out.shapes.is_empty());
    }
}
