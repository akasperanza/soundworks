use super::*;
use crate::fx::{self, FxKind, FxSpec};
use egui::Pos2;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum FxDrag {

    New(FxKind),

    Move(usize),
}

fn fx_glyph(p: &egui::Painter, kind: FxKind, c: Pos2, s: f32, col: Color32) {
    let st = Stroke::new((s * 0.075).max(1.5), col);
    let h = s * 0.5;
    match kind {
        FxKind::Limiter => {

            let ceil = c.y - h * 0.45;
            p.line_segment([pos2(c.x - h, ceil), pos2(c.x + h, ceil)], Stroke::new(st.width, alpha(col, 120)));
            let pts: Vec<Pos2> = (0..=40)
                .map(|i| {
                    let t = i as f32 / 40.0;
                    let y = -(t * std::f32::consts::TAU * 1.25).sin() * h * 0.9;
                    pos2(c.x - h + t * 2.0 * h, c.y + y.max(-h * 0.45))
                })
                .collect();
            p.add(Shape::line(pts, st));
        }
        FxKind::Compressor => {

            let pts: Vec<Pos2> = (0..=40)
                .map(|i| {
                    let t = i as f32 / 40.0;
                    let env = 1.0 - 0.65 * (1.0 - (t * 2.0 - 1.0).abs());
                    let y = (t * std::f32::consts::TAU * 3.0).sin() * h * 0.8 * env;
                    pos2(c.x - h + t * 2.0 * h, c.y + y)
                })
                .collect();
            p.add(Shape::line(pts, st));
        }
        FxKind::Width => {
            p.circle_filled(c, s * 0.07, col);
            for sgn in [-1.0f32, 1.0] {
                for k in 1..=2 {
                    let r = s * 0.17 * k as f32 + s * 0.12;
                    let pts: Vec<Pos2> = (0..=16)
                        .map(|i| {
                            let a = -0.9 + 1.8 * i as f32 / 16.0;
                            pos2(c.x + sgn * a.cos() * r, c.y + a.sin() * r)
                        })
                        .collect();
                    p.add(Shape::line(pts, Stroke::new(st.width, alpha(col, if k == 1 { 255 } else { 150 }))));
                }
            }
        }
        FxKind::Filter => {

            let pts = [(-1.0, 0.6), (-0.7, 0.6), (-0.45, -0.3), (-0.3, -0.5), (0.3, -0.5), (0.45, -0.3), (0.7, 0.6), (1.0, 0.6)];
            let pts: Vec<Pos2> = pts.iter().map(|(x, y)| pos2(c.x + x * h, c.y + y * h)).collect();
            p.add(Shape::line(pts, st));
        }
        FxKind::Tone => {

            let pts: Vec<Pos2> = (0..=40)
                .map(|i| {
                    let t = i as f32 / 40.0;
                    let y = 0.45 * ((1.0 - t * 5.0).max(0.0).powi(2) + ((t - 0.8) * 5.0).max(0.0).powi(2)) - 0.1;
                    pos2(c.x - h + t * 2.0 * h, c.y - y.min(0.9) * h * 0.9 + h * 0.2)
                })
                .collect();
            p.add(Shape::line(pts, st));
        }
        FxKind::Loudness => {

            for (i, hh) in [0.9f32, 0.45, 0.25, 0.45, 0.8].iter().enumerate() {
                let x = c.x + (i as f32 - 2.0) * h * 0.42;
                p.line_segment([pos2(x, c.y - hh * h * 0.8), pos2(x, c.y + hh * h * 0.8)], Stroke::new(st.width * 1.4, col));
            }
        }
        FxKind::Crossfeed => {

            let (a, b) = (h * 0.8, h * 0.45);
            p.line_segment([pos2(c.x - a, c.y - b), pos2(c.x + a, c.y - b)], st);
            p.line_segment([pos2(c.x - a, c.y + b), pos2(c.x + a, c.y + b)], st);
            p.line_segment([pos2(c.x - a * 0.6, c.y - b), pos2(c.x + a * 0.6, c.y + b)], Stroke::new(st.width, alpha(col, 140)));
            p.line_segment([pos2(c.x - a * 0.6, c.y + b), pos2(c.x + a * 0.6, c.y - b)], Stroke::new(st.width, alpha(col, 140)));
        }
        FxKind::Gain => {
            p.line_segment([pos2(c.x, c.y + h * 0.7), pos2(c.x, c.y - h * 0.7)], st);
            p.line_segment([pos2(c.x - h * 0.5, c.y - h * 0.2), pos2(c.x, c.y - h * 0.7)], st);
            p.line_segment([pos2(c.x + h * 0.5, c.y - h * 0.2), pos2(c.x, c.y - h * 0.7)], st);
            p.line_segment([pos2(c.x - h * 0.5, c.y + h * 0.7), pos2(c.x + h * 0.5, c.y + h * 0.7)], Stroke::new(st.width, alpha(col, 120)));
        }
    }
}

fn insert_index(rects: &[Rect], skip: Option<usize>, p: Pos2) -> usize {
    rects
        .iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != skip)
        .filter(|(_, r)| {
            let (top, bottom) = (r.top(), r.bottom());
            if p.y > bottom {
                true
            } else if p.y < top {
                false
            } else {
                r.center().x < p.x
            }
        })
        .count()
}

fn reorder(chain: &mut Vec<FxSpec>, from: usize, idx: usize) {
    if from >= chain.len() {
        return;
    }
    let s = chain.remove(from);
    let at = idx.min(chain.len());
    chain.insert(at, s);
}

fn summary(s: &FxSpec) -> String {
    match s.kind {
        FxKind::Limiter => format!("{:.1} dB", s.param(0)),
        FxKind::Compressor => format!("{:.0} dB · {}:1", s.param(0), format!("{:.1}", s.param(1)).trim_end_matches(".0")),
        FxKind::Width => format!("%{:.0}", s.param(0)),
        FxKind::Gain => format!("{:+.1} dB", s.param(0)),
        FxKind::Filter => {
            let (h, l) = (s.param(0), s.param(2));
            match (h > 0.5, l < 23999.5) {
                (true, true) => format!("{:.0} Hz – {:.1} kHz", h, l / 1000.0),
                (true, false) => format!("HPF {:.0} Hz", h),
                (false, true) => format!("LPF {:.1} kHz", l / 1000.0),
                (false, false) => "Kapalı".to_string(),
            }
        }
        FxKind::Tone => format!("Bas {:+.0} · Tiz {:+.0}", s.param(0), s.param(2)),
        FxKind::Loudness => format!("Güç %{:.0}", s.param(0)),
        FxKind::Crossfeed => format!("{:.0} dB", s.param(0)),
    }
}

const NODE_H: f32 = 96.0;

fn text_w(p: &egui::Painter, s: &str, font: egui::FontId) -> f32 {
    p.layout_no_wrap(crate::i18n::tr(s), font, Color32::WHITE).size().x
}

fn fit(p: &egui::Painter, s: &str, font: egui::FontId, max_w: f32) -> String {
    let s = &crate::i18n::tr(s);
    if text_w(p, s, font.clone()) <= max_w {
        return s.to_string();
    }
    let mut t: String = s.chars().collect();
    while t.chars().count() > 1 {
        t.pop();
        let cand = format!("{}…", t.trim_end());
        if text_w(p, &cand, font.clone()) <= max_w {
            return cand;
        }
    }
    "…".to_string()
}

fn node_width(p: &egui::Painter, spec: &FxSpec) -> f32 {
    let info = fx::info(spec.kind);
    let head = text_w(p, info.name, bold(13.5)).max(text_w(p, info.tagline, body(11.5)));
    let foot = text_w(p, &summary(spec), body(12.5)) + 16.0 + 14.0 + 52.0;
    (50.0 + head + 18.0).max(foot).max(138.0).ceil()
}

impl App {

    fn fx_normalize(&mut self) {
        let mut seen: Vec<u32> = vec![];
        let max = self.cfg.chain.iter().map(|s| s.id).max().unwrap_or(0);
        let mut next = max + 1;
        for s in self.cfg.chain.iter_mut() {
            if seen.contains(&s.id) {
                s.id = next;
                next += 1;
            }
            seen.push(s.id);
        }
        for s in self.cfg.chain.iter_mut() {
            let info = fx::info(s.kind);
            while s.params.len() < info.params.len() {
                s.params.push(info.params[s.params.len()].default);
            }
        }
    }

    fn fx_next_id(&self) -> u32 {
        self.cfg.chain.iter().map(|s| s.id).max().unwrap_or(0) + 1
    }

    fn fx_add(&mut self, kind: FxKind, at: Option<usize>) {
        let id = self.fx_next_id();
        let spec = FxSpec::new(id, kind);
        let at = at.unwrap_or(self.cfg.chain.len()).min(self.cfg.chain.len());
        self.cfg.chain.insert(at, spec);
        self.fx_sel = Some(id);
    }

    fn fx_insert_index(&self, p: Pos2) -> usize {
        let skip = match self.fx_drag {
            Some(FxDrag::Move(i)) => Some(i),
            _ => None,
        };
        insert_index(&self.fx_rects, skip, p)
    }

    pub(super) fn fx_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.fx_normalize();
        egui::ScrollArea::vertical().id_salt("fxview").auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(4.0);
            ui.label(RichText::new("Sinyal zinciri").font(bold(17.0)).color(Color32::WHITE));
            ui.label(RichText::new("Marketten bir modülü zincire sürükle. Sırayı değiştirmek için zincirdeki modülü tut ve taşı.").font(body(12.5)).color(DIM));
            ui.add_space(12.0);
            self.fx_chain_row(ui, ctx);
            ui.add_space(14.0);
            self.fx_params(ui);
            ui.add_space(22.0);
            self.fx_market(ui, ctx);
            ui.add_space(20.0);
        });
        self.fx_finish_drag(ctx);
    }

    fn fx_chain_row(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        #[derive(Clone, Copy, PartialEq)]
        enum Item {
            Source,
            Eq,
            Out,
            Module(usize),
            Placeholder,
        }
        const ARROW_W: f32 = 22.0;
        const ROW_GAP: f32 = 14.0;

        let chain = self.cfg.chain.clone();
        let dragging = self.fx_drag.is_some();
        let ptr = ctx.pointer_latest_pos();
        let bands = self.preset.bands.len();
        let mut rects: Vec<Rect> = vec![];
        let mut eq_rect = Rect::NOTHING;
        let (mut remove, mut toggle, mut select, mut drag_start): (Option<usize>, Option<usize>, Option<u32>, Option<usize>) = (None, None, None, None);
        let mut goto_eq = false;

        let mut items: Vec<(Item, f32)> = vec![(Item::Source, 94.0), (Item::Eq, 94.0)];
        for (i, s) in chain.iter().enumerate() {
            items.push((Item::Module(i), node_width(ui.painter(), s)));
        }
        if chain.is_empty() {
            items.push((Item::Placeholder, 210.0));
        }
        items.push((Item::Out, 94.0));

        let inner = Frame::none().fill(BG2).rounding(Rounding::same(14.0)).inner_margin(Margin::symmetric(18.0, 16.0)).show(ui, |ui| {
            let avail = ui.available_width();

            let mut place: Vec<(f32, usize)> = vec![];
            let (mut x, mut row) = (0.0f32, 0usize);
            for (_, w) in &items {
                if x > 0.0 && x + *w > avail {
                    row += 1;
                    x = 0.0;
                }
                place.push((x, row));
                x += *w + ARROW_W;
            }
            let rows = row + 1;
            let total_h = rows as f32 * NODE_H + (rows - 1) as f32 * ROW_GAP;
            let (area, _) = ui.allocate_exact_size(vec2(avail, total_h), Sense::hover());

            for (k, (item, w)) in items.iter().enumerate() {
                let (x, row) = place[k];
                let rect = Rect::from_min_size(area.min + vec2(x, row as f32 * (NODE_H + ROW_GAP)), vec2(*w, NODE_H));
                match item {
                    Item::Source | Item::Eq | Item::Out => {
                        let (title, sub, icon, clickable) = match item {
                            Item::Source => ("Kaynak".to_string(), if self.cfg.use_loopback { "Sistem sesi" } else { "Oynatıcı" }.to_string(), Icon::Wave, false),
                            Item::Eq => ("Ekolayzır".to_string(), format!("{bands} bant"), Icon::Wave, true),
                            _ => ("Çıkış".to_string(), "Ses → cihaz".to_string(), Icon::Volume, false),
                        };
                        let resp = ui.interact(rect, Id::new(("fxfixed", k)), if clickable { Sense::click() } else { Sense::hover() });
                        let h = ui.ctx().animate_bool_with_time(resp.id, resp.hovered() && clickable, 0.1);
                        let p = ui.painter();
                        p.rect_filled(rect, Rounding::same(12.0), lerp_color(BG1, BG3, h * 0.6));
                        draw_icon(p, icon, pos2(rect.center().x, rect.top() + 28.0), 17.0, SKY);
                        p.text_tr(pos2(rect.center().x, rect.top() + 56.0), Align2::CENTER_CENTER, fit(p, &title, bold(13.0), rect.width() - 12.0), bold(13.0), Color32::WHITE);
                        p.text_tr(pos2(rect.center().x, rect.top() + 75.0), Align2::CENTER_CENTER, fit(p, &sub, body(11.5), rect.width() - 12.0), body(11.5), DIM);
                        if *item == Item::Eq {
                            eq_rect = rect;
                            if resp.clicked() {
                                goto_eq = true;
                            }
                        }
                    }
                    Item::Placeholder => {
                        let on = dragging && ptr.map(|p| rect.expand(40.0).contains(p)).unwrap_or(false);
                        let p = ui.painter();
                        p.rect_stroke(rect, Rounding::same(12.0), Stroke::new(1.4_f32, if on { BLUE } else { alpha(DIM, 110) }));
                        p.rect_filled(rect, Rounding::same(12.0), if on { alpha(BLUE, 28) } else { Color32::TRANSPARENT });
                        p.text_tr(rect.center(), Align2::CENTER_CENTER, "Modülleri buraya sürükle", body(12.5), if on { SKY } else { DIM });
                    }
                    Item::Module(i) => {
                        let i = *i;
                        let spec = &chain[i];
                        let info = fx::info(spec.kind);
                        let nw = *w;
                        let resp = ui.interact(rect, Id::new(("fxnode", spec.id)), Sense::click_and_drag());
                        rects.push(rect);
                        let sel = self.fx_sel == Some(spec.id);
                        let being_dragged = self.fx_drag == Some(FxDrag::Move(i));
                        let h = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1);
                        let dim = if spec.enabled { 1.0 } else { 0.5 } * if being_dragged { 0.4 } else { 1.0 };
                        let p = ui.painter();
                        p.rect_filled(rect, Rounding::same(12.0), if sel { lerp_color(BG3, BLUE, 0.28) } else { BG3 });
                        if h > 0.0 && !sel {
                            p.rect_filled(rect, Rounding::same(12.0), alpha(Color32::WHITE, (10.0 * h) as u8));
                        }
                        p.rect_stroke(rect, Rounding::same(12.0), Stroke::new(1.2_f32, if sel { BLUE } else { alpha(STROKE, 200) }));
                        fx_glyph(p, spec.kind, pos2(rect.left() + 28.0, rect.top() + 30.0), 26.0, alpha(if spec.enabled { SKY } else { DIM }, (255.0 * dim) as u8));
                        p.text_tr(pos2(rect.left() + 50.0, rect.top() + 24.0), Align2::LEFT_CENTER, fit(p, info.name, bold(13.5), nw - 60.0), bold(13.5), alpha(Color32::WHITE, (255.0 * dim) as u8));
                        p.text_tr(pos2(rect.left() + 50.0, rect.top() + 42.0), Align2::LEFT_CENTER, fit(p, info.tagline, body(11.5), nw - 60.0), body(11.5), alpha(DIM, (255.0 * dim) as u8));
                        p.text_tr(pos2(rect.left() + 16.0, rect.bottom() - 20.0), Align2::LEFT_CENTER, fit(p, &summary(spec), body(12.5), nw - 16.0 - 62.0), body(12.5), alpha(TEXT, (255.0 * dim) as u8));

                        let tr = Rect::from_center_size(pos2(rect.right() - 42.0, rect.bottom() - 20.0), vec2(26.0, 26.0));
                        let rr = Rect::from_center_size(pos2(rect.right() - 17.0, rect.bottom() - 20.0), vec2(26.0, 26.0));
                        let tresp = ui.interact(tr, Id::new(("fxnode_t", spec.id)), Sense::click());
                        let rresp = ui.interact(rr, Id::new(("fxnode_r", spec.id)), Sense::click());
                        let th = ui.ctx().animate_bool_with_time(tresp.id, tresp.hovered(), 0.1);
                        let rh = ui.ctx().animate_bool_with_time(rresp.id, rresp.hovered(), 0.1);
                        draw_icon(ui.painter(), Icon::Power, tr.center(), 11.5, lerp_color(if spec.enabled { SKY } else { DIM }, Color32::WHITE, th));
                        draw_icon(ui.painter(), Icon::Close, rr.center(), 10.5, lerp_color(DIM, RED, rh));
                        tresp.clone().on_hover_text_tr(if spec.enabled { "Devre dışı bırak" } else { "Etkinleştir" });
                        if tresp.clicked() {
                            toggle = Some(i);
                        } else if rresp.clicked() {
                            remove = Some(i);
                        } else if resp.clicked() {
                            select = Some(spec.id);
                        }
                        if resp.drag_started() {
                            drag_start = Some(i);
                        }
                    }
                }

                if k + 1 < items.len() && place[k + 1].1 == row {
                    let c = pos2(rect.right() + ARROW_W * 0.5, rect.center().y);
                    let col = alpha(DIM, 150);
                    let p = ui.painter();
                    p.line_segment([pos2(c.x - 6.0, c.y), pos2(c.x + 6.0, c.y)], Stroke::new(1.5_f32, col));
                    p.line_segment([pos2(c.x + 2.0, c.y - 4.0), pos2(c.x + 6.0, c.y)], Stroke::new(1.5_f32, col));
                    p.line_segment([pos2(c.x + 2.0, c.y + 4.0), pos2(c.x + 6.0, c.y)], Stroke::new(1.5_f32, col));
                }
            }
        });
        self.fx_zone = inner.response.rect;
        self.fx_rects = rects.clone();
        self.fx_eq_right = eq_rect.right();

        if dragging {
            if let Some(p) = ptr {
                if self.fx_zone.contains(p) && !rects.is_empty() {
                    let skip = match self.fx_drag {
                        Some(FxDrag::Move(i)) => Some(i),
                        _ => None,
                    };
                    let idx = insert_index(&rects, skip, p);
                    let others: Vec<Rect> = rects.iter().enumerate().filter(|(i, _)| Some(*i) != skip).map(|(_, r)| *r).collect();
                    let (x, top) = match (others.get(idx), others.last().or(rects.first())) {
                        (Some(r), _) => (r.left() - 13.0, r.top()),
                        (None, Some(l)) => (l.right() + 13.0, l.top()),
                        (None, None) => (self.fx_zone.left(), self.fx_zone.top()),
                    };
                    ui.painter().rect_filled(Rect::from_min_max(pos2(x - 1.5, top), pos2(x + 1.5, top + NODE_H)), Rounding::same(2.0), BLUE);
                    ui.painter().rect_filled(Rect::from_min_max(pos2(x - 5.0, top), pos2(x + 5.0, top + NODE_H)), Rounding::same(5.0), alpha(BLUE, 40));
                }
            }
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        }

        if let Some(i) = drag_start {
            self.fx_drag = Some(FxDrag::Move(i));
        }
        if let Some(i) = toggle {
            self.cfg.chain[i].enabled = !self.cfg.chain[i].enabled;
        }
        if let Some(i) = remove {
            let id = self.cfg.chain[i].id;
            self.cfg.chain.remove(i);
            if self.fx_sel == Some(id) {
                self.fx_sel = None;
            }
        }
        if let Some(id) = select {
            self.fx_sel = Some(id);
        }
        if goto_eq {
            self.motor_tab = 0;
        }
    }

    fn fx_params(&mut self, ui: &mut egui::Ui) {
        let Some(idx) = self.fx_sel.and_then(|id| self.cfg.chain.iter().position(|s| s.id == id)) else {
            if self.cfg.chain.is_empty() {
                return;
            }
            ui.label(RichText::new("Ayarlarını görmek için zincirden bir modüle tıkla.").font(body(12.5)).color(DIM));
            return;
        };
        let kind = self.cfg.chain[idx].kind;
        let info = fx::info(kind);
        let mut reset = false;
        let mut remove = false;
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::hover());
                ui.painter().rect_filled(r, Rounding::same(9.0), BG3);
                fx_glyph(ui.painter(), kind, r.center(), 24.0, SKY);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(RichText::new(info.name).font(bold(15.0)).color(Color32::WHITE));
                    ui.label(RichText::new(info.tagline).font(body(12.0)).color(DIM));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if pill_button(ui, "Kaldır", Some(Icon::Trash), false).clicked() {
                        remove = true;
                    }
                    if pill_button(ui, "Varsayılan", Some(Icon::Undo), false).clicked() {
                        reset = true;
                    }
                    let mut on = self.cfg.chain[idx].enabled;
                    if switch(ui, &mut on).changed() {
                        self.cfg.chain[idx].enabled = on;
                    }
                });
            });
            ui.add_space(12.0);
            ui.spacing_mut().slider_width = (ui.available_width() - 200.0).clamp(140.0, 420.0);
            egui::Grid::new(("fxparams", self.cfg.chain[idx].id)).num_columns(2).spacing([22.0, 12.0]).show(ui, |ui| {
                for (pi, pinfo) in info.params.iter().enumerate() {
                    ui.label(RichText::new(pinfo.name).font(body(13.5)).color(TEXT));
                    let mut v = self.cfg.chain[idx].params[pi].clamp(pinfo.min, pinfo.max);
                    let suffix = if pinfo.unit == "%" { "%".to_string() } else { format!(" {}", crate::i18n::tr(pinfo.unit)) };
                    let mut slider = egui::Slider::new(&mut v, pinfo.min..=pinfo.max).fixed_decimals(pinfo.decimals).suffix(suffix);

                    let (min, max, dec, off) = (pinfo.min, pinfo.max, pinfo.decimals, pinfo.off);
                    let unit = crate::i18n::tr(pinfo.unit);
                    if off != crate::fx::Off::No {
                        slider = slider.custom_formatter(move |x, _| match off {
                            crate::fx::Off::AtMin(l) if x <= min as f64 => crate::i18n::tr(l),
                            crate::fx::Off::AtMax(l) if x >= max as f64 => crate::i18n::tr(l),
                            _ => format!("{:.*} {}", dec, x, unit),
                        });
                    }
                    if ui.add(slider).changed() {
                        self.cfg.chain[idx].params[pi] = v;
                    }
                    ui.end_row();
                }
            });
            ui.add_space(8.0);
            ui.label(RichText::new(info.desc).font(body(12.0)).color(DIM));
        });
        if reset {
            self.cfg.chain[idx].params = info.params.iter().map(|p| p.default).collect();
        }
        if remove {
            let id = self.cfg.chain[idx].id;
            self.cfg.chain.remove(idx);
            if self.fx_sel == Some(id) {
                self.fx_sel = None;
            }
        }
    }

    fn fx_market(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Market").font(bold(17.0)).color(Color32::WHITE));
            ui.label(RichText::new(format!("{} modül", fx::CATALOG.len())).font(body(12.5)).color(DIM));
        });
        ui.add_space(8.0);
        let gap = 14.0;
        let avail = ui.available_width() - 4.0;
        let cols = ((avail + gap) / (290.0 + gap)).floor().max(1.0);
        let cw = ((avail - gap * (cols - 1.0)) / cols).floor();
        let mut add: Option<FxKind> = None;
        let mut start: Option<FxKind> = None;

        let desc_h = fx::CATALOG
            .iter()
            .map(|i| ui.painter().layout(crate::i18n::tr(i.desc), body(11.8), DIM, cw - 32.0).size().y)
            .fold(0.0f32, f32::max);
        let ch = (72.0 + desc_h + 18.0).ceil().max(132.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(gap, gap);
            for info in fx::CATALOG {
                let (rect, resp) = ui.allocate_exact_size(vec2(cw, ch), Sense::click_and_drag());
                let h = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1);
                let count = self.cfg.chain.iter().filter(|s| s.kind == info.kind).count();
                let p = ui.painter().with_clip_rect(rect);
                p.rect_filled(rect, Rounding::same(14.0), lerp_color(BG2, BG3, h * 0.7));
                if resp.dragged() {
                    p.rect_stroke(rect, Rounding::same(14.0), Stroke::new(1.2_f32, BLUE));
                }
                let ir = Rect::from_min_size(rect.min + vec2(16.0, 16.0), vec2(46.0, 46.0));
                p.rect_filled(ir, Rounding::same(11.0), BG1);
                fx_glyph(&p, info.kind, ir.center(), 30.0, SKY);

                let tw = (rect.right() - 16.0 - 34.0 - 36.0) - (ir.right() + 14.0);
                p.text_tr(pos2(ir.right() + 14.0, rect.top() + 28.0), Align2::LEFT_CENTER, fit(&p, info.name, bold(14.5), tw), bold(14.5), Color32::WHITE);
                p.text_tr(pos2(ir.right() + 14.0, rect.top() + 47.0), Align2::LEFT_CENTER, fit(&p, info.tagline, body(12.0), tw), body(12.0), SKY);
                let g = p.layout(crate::i18n::tr(info.desc), body(11.8), DIM, rect.width() - 32.0);
                p.galley(pos2(rect.left() + 16.0, rect.top() + 72.0), g, DIM);

                let br = Rect::from_center_size(pos2(rect.right() - 30.0, rect.top() + 28.0), vec2(34.0, 28.0));
                let bresp = ui.interact(br, resp.id.with("add"), Sense::click());
                let bh = ui.ctx().animate_bool_with_time(bresp.id, bresp.hovered(), 0.1);
                ui.painter().rect_filled(br, Rounding::same(14.0), lerp_color(alpha(BLUE, 70), BLUE, bh));
                draw_icon(ui.painter(), Icon::Plus, br.center(), 12.0, Color32::WHITE);
                bresp.clone().on_hover_text_tr("Zincirin sonuna ekle");
                if count > 0 {
                    ui.painter().text_tr(pos2(br.left() - 10.0, rect.top() + 28.0), Align2::RIGHT_CENTER, format!("×{count}"), body(11.5), DIM);
                }
                if bresp.clicked() {
                    add = Some(info.kind);
                } else if resp.drag_started() {
                    start = Some(info.kind);
                }
            }
        });
        if let Some(k) = add {
            self.fx_add(k, None);
        }
        if let Some(k) = start {
            self.fx_drag = Some(FxDrag::New(k));
        }
        let _ = ctx;
    }

    fn fx_finish_drag(&mut self, ctx: &egui::Context) {
        let Some(d) = self.fx_drag else { return };
        let ptr = ctx.pointer_latest_pos();
        if let Some(p) = ptr {

            let kind = match d {
                FxDrag::New(k) => Some(k),
                FxDrag::Move(i) => self.cfg.chain.get(i).map(|s| s.kind),
            };
            if let Some(k) = kind {
                let info = fx::info(k);
                let layer = egui::LayerId::new(egui::Order::Tooltip, Id::new("fx_ghost"));
                let pa = ctx.layer_painter(layer);
                let r = Rect::from_center_size(p + vec2(0.0, 8.0), vec2(150.0, 54.0));
                pa.rect_filled(r.translate(vec2(0.0, 5.0)).expand(2.0), Rounding::same(14.0), Color32::from_black_alpha(70));
                pa.rect_filled(r, Rounding::same(12.0), alpha(BG3, 245));
                pa.rect_stroke(r, Rounding::same(12.0), Stroke::new(1.4_f32, BLUE));
                fx_glyph(&pa, k, pos2(r.left() + 26.0, r.center().y), 24.0, SKY);
                pa.text_tr(pos2(r.left() + 48.0, r.center().y - 7.0), Align2::LEFT_CENTER, info.name, bold(13.0), Color32::WHITE);
                pa.text_tr(pos2(r.left() + 48.0, r.center().y + 10.0), Align2::LEFT_CENTER, info.tagline, body(11.0), DIM);
            }
        }
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        ctx.request_repaint();
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.fx_drag = None;
            return;
        }
        if ctx.input(|i| i.pointer.any_released()) {
            if let Some(p) = ptr {
                if self.fx_zone.expand2(vec2(0.0, 30.0)).contains(p) {
                    let idx = self.fx_insert_index(p);
                    match d {
                        FxDrag::New(k) => self.fx_add(k, Some(idx)),
                        FxDrag::Move(i) => reorder(&mut self.cfg.chain, i, idx),
                    }
                }
            }
            self.fx_drag = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain(ids: &[u32]) -> Vec<FxSpec> {
        ids.iter().map(|i| FxSpec::new(*i, FxKind::Gain)).collect()
    }
    fn ids(c: &[FxSpec]) -> Vec<u32> {
        c.iter().map(|s| s.id).collect()
    }

    fn r(x: f32, y: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(100.0, 96.0))
    }

    #[test]
    fn insert_index_single_row() {

        let rects = [r(100.0, 0.0), r(250.0, 0.0), r(400.0, 0.0)];
        let y = 40.0;
        assert_eq!(insert_index(&rects, None, pos2(50.0, y)), 0);
        assert_eq!(insert_index(&rects, None, pos2(200.0, y)), 1);
        assert_eq!(insert_index(&rects, None, pos2(340.0, y)), 2);
        assert_eq!(insert_index(&rects, None, pos2(900.0, y)), 3);

        assert_eq!(insert_index(&rects, Some(1), pos2(340.0, y)), 1);
        assert_eq!(insert_index(&rects, Some(1), pos2(900.0, y)), 2);
    }

    #[test]
    fn insert_index_wrapped_rows() {

        let rects = [r(0.0, 0.0), r(150.0, 0.0), r(0.0, 110.0), r(150.0, 110.0)];

        assert_eq!(insert_index(&rects, None, pos2(10.0, 150.0)), 2);

        assert_eq!(insert_index(&rects, None, pos2(140.0, 150.0)), 3);

        assert_eq!(insert_index(&rects, None, pos2(500.0, 150.0)), 4);

        assert_eq!(insert_index(&rects, None, pos2(500.0, 40.0)), 2);

        assert_eq!(insert_index(&rects, None, pos2(10.0, 103.0)), 2);
    }

    #[test]
    fn reorder_moves_without_losing_or_duplicating() {
        let mut c = chain(&[1, 2, 3, 4]);
        reorder(&mut c, 0, 3);
        assert_eq!(ids(&c), vec![2, 3, 4, 1]);
        reorder(&mut c, 3, 0);
        assert_eq!(ids(&c), vec![1, 2, 3, 4]);
        reorder(&mut c, 1, 1);
        assert_eq!(ids(&c), vec![1, 2, 3, 4]);
        reorder(&mut c, 9, 0);
        assert_eq!(ids(&c), vec![1, 2, 3, 4]);
        reorder(&mut c, 0, 99);
        assert_eq!(ids(&c), vec![2, 3, 4, 1]);
    }

    #[test]
    fn drag_roundtrip_matches_pointer_position() {

        let rects = [r(100.0, 0.0), r(250.0, 0.0), r(400.0, 0.0)];
        let mut c = chain(&[1, 2, 3]);
        let idx = insert_index(&rects, Some(1), pos2(900.0, 40.0));
        reorder(&mut c, 1, idx);
        assert_eq!(ids(&c), vec![1, 3, 2]);
    }
}
