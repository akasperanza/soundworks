const SVG: &str = include_str!("../assets/icon/icon.svg");

pub fn rgba(size: u32) -> Vec<u8> {
    let mut pix = resvg::tiny_skia::Pixmap::new(size, size).expect("pixmap");
    if let Ok(tree) = resvg::usvg::Tree::from_str(SVG, &resvg::usvg::Options::default()) {
        let k = size as f32 / 256.0;
        resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(k, k), &mut pix.as_mut());
    }

    pix.pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect()
}

pub fn window_icon() -> eframe::egui::IconData {
    eframe::egui::IconData { rgba: rgba(256), width: 256, height: 256 }
}

pub fn tray_icon() -> Option<tray_icon::Icon> {
    tray_icon::Icon::from_rgba(rgba(64), 64, 64).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_renders_dark_navy_tile_with_light_letters_and_transparent_corners() {
        let n = 128u32;
        let px = rgba(n);
        assert_eq!(px.len(), (n * n * 4) as usize);
        let at = |x: u32, y: u32| {
            let o = ((y * n + x) * 4) as usize;
            [px[o], px[o + 1], px[o + 2], px[o + 3]]
        };
        assert_eq!(at(0, 0)[3], 0, "köşe şeffaf (yuvarlak kare)");
        let bg = at(10, n / 2);
        assert!(bg[3] == 255 && bg[2] >= bg[0] && (bg[0] as u32 + bg[1] as u32 + bg[2] as u32) < 330, "zemin koyu lacivert: {bg:?}");

        let light = (0..n).any(|x| (0..n).any(|y| { let c = at(x, y); c[3] == 255 && c[0] > 200 && c[1] > 200 && c[2] > 220 }));
        assert!(light, "açık renkli harf pikseli bulunamadı");

        let left_edge_light = (0..n).any(|y| { let c = at(8, y); c[0] > 200 && c[1] > 200 });
        assert!(!left_edge_light, "harfler kenara taşmamalı");
    }

    #[test]
    #[ignore]
    fn dump_icons() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("icon");
        std::fs::create_dir_all(&dir).unwrap();
        for size in [16u32, 24, 32, 48, 64, 128, 256] {
            let px = rgba(size);
            let img = image::RgbaImage::from_raw(size, size, px).unwrap();
            img.save(dir.join(format!("icon_{size}.png"))).unwrap();
        }
    }

    fn glyph_path(font_path: &str, text: &str, em: f32) -> (String, f32, f32) {
        use ab_glyph::{Font, FontVec, OutlineCurve, ScaleFont};
        let data = std::fs::read(font_path).unwrap();
        let font = FontVec::try_from_vec(data).unwrap();
        let sf = font.as_scaled(ab_glyph::PxScale::from(em));
        let mut x = 0.0f32;
        let mut d = String::new();
        let (mut top, mut bottom) = (f32::MAX, f32::MIN);
        let mut prev: Option<ab_glyph::GlyphId> = None;
        for ch in text.chars() {
            let id = font.glyph_id(ch);
            if let Some(p) = prev {
                x += sf.kern(p, id);
            }
            prev = Some(id);
            if let Some(o) = font.outline(id) {
                let k = em / font.height_unscaled();
                let pt = |p: ab_glyph::Point| (x + p.x * k, -p.y * k);
                let mut cur: Option<(f32, f32)> = None;
                let start_of = |c: &OutlineCurve| match c {
                    OutlineCurve::Line(a, _) => *a,
                    OutlineCurve::Quad(a, _, _) => *a,
                    OutlineCurve::Cubic(a, _, _, _) => *a,
                };
                for c in &o.curves {
                    let s = pt(start_of(c));
                    if cur.map_or(true, |p| (p.0 - s.0).abs() > 0.01 || (p.1 - s.1).abs() > 0.01) {
                        if cur.is_some() {
                            d.push('Z');
                        }
                        d.push_str(&format!("M{:.2} {:.2}", s.0, s.1));
                    }
                    match c {
                        OutlineCurve::Line(_, b) => {
                            let b = pt(*b);
                            d.push_str(&format!("L{:.2} {:.2}", b.0, b.1));
                            cur = Some(b);
                        }
                        OutlineCurve::Quad(_, b, c2) => {
                            let (b, c2) = (pt(*b), pt(*c2));
                            d.push_str(&format!("Q{:.2} {:.2} {:.2} {:.2}", b.0, b.1, c2.0, c2.1));
                            cur = Some(c2);
                        }
                        OutlineCurve::Cubic(_, b, c2, e) => {
                            let (b, c2, e) = (pt(*b), pt(*c2), pt(*e));
                            d.push_str(&format!("C{:.2} {:.2} {:.2} {:.2} {:.2} {:.2}", b.0, b.1, c2.0, c2.1, e.0, e.1));
                            cur = Some(e);
                        }
                    }
                }
                d.push('Z');
            }
            x += sf.h_advance(id);
        }
        let _ = (&mut top, &mut bottom);
        (d, x, em)
    }

    #[test]
    #[ignore]
    fn build_icon_svg() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("icon");
        let em = 168.0;
        let (path, w, _h) = glyph_path(&std::env::var("ICON_FONT").expect("ICON_FONT=<Barlow-SemiBold.ttf yolu>"), "SW", em);
        let tx = (256.0 - w) / 2.0;
        let ty = 128.0 + em * 0.35;
        let svg = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256"><defs><linearGradient id="g" x1="0" y1="0" x2="0.9" y2="1"><stop offset="0" stop-color="#2B3F72"/><stop offset="0.55" stop-color="#121B34"/><stop offset="1" stop-color="#090D1A"/></linearGradient><linearGradient id="t" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#FFFFFF"/><stop offset="1" stop-color="#9DB9F2"/></linearGradient><linearGradient id="h" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#FFFFFF" stop-opacity="0.22"/><stop offset="0.5" stop-color="#FFFFFF" stop-opacity="0"/></linearGradient></defs><rect width="256" height="256" rx="58" fill="url(#g)"/><rect x="1.5" y="1.5" width="253" height="253" rx="56.5" fill="none" stroke="url(#h)" stroke-width="3"/><path transform="translate({tx:.2} {ty:.2})" d="{path}" fill="url(#t)" stroke="url(#t)" stroke-width="1.5" stroke-linejoin="round"/></svg>"##
        );
        std::fs::write(dir.join("icon.svg"), svg).unwrap();
    }
}
