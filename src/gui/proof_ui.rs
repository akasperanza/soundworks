//! Bit-perfect ışığı ve makbuzu: alt çubukta kuyruk düğmesinin yanındaki ışık, üzerine gelince ölçüm makbuzu.

use super::*;
use crate::proof::{self, Counters, Light, LightIn};

/// Makbuzda gösterilen her şey önceden hesaplanır (ipucu penceresi `self`'e dokunmadan çizilebilsin).
pub(super) struct Receipt {
    light: Light,
    title: &'static str,
    source: String,
    device: String,
    rows: Vec<(bool, String)>,
    counts: Option<String>,
}

impl App {
    /// Her karede: parça başı sayaç tabanını ayarlar, son saniyedeki değişiklikleri ve kesintileri izler.
    pub(super) fn tick_proof(&mut self) {
        let Some(ctl) = self.running.as_ref().map(|r| r.ctl.clone()) else {
            self.proof_view = Counters::default();
            return;
        };
        let cur = ctl.proof.read();
        let id = self.player.meta_id.load(Relaxed);
        let old = std::time::Duration::from_secs(10);
        if id != self.proof_id {
            self.proof_id = id;
            self.proof_base = cur;
            self.proof_dry_shown = 0;
            self.proof_alt_seen = cur.alt;
            self.proof_cause_at = [Instant::now() - old; 6];
            self.proof_cause_mask = 0;
            self.proof_frames_seen = cur.ok + cur.alt;
            ctl.proof.causes.store(0, Relaxed);
        }
        // nedenler "yapışmaz": her neden son görüldüğü andan itibaren 1,5 sn geçerlidir. Yalnızca çerçeve yazılırken (çalarken) süre
        // işler; duraklatınca son bilinen durum korunur.
        let fresh = ctl.proof.causes.swap(0, Relaxed);
        let now = Instant::now();
        for (i, t) in self.proof_cause_at.iter_mut().enumerate() {
            if fresh >> i & 1 == 1 {
                *t = now;
            }
        }
        let frames = cur.ok + cur.alt;
        if frames != self.proof_frames_seen {
            self.proof_frames_seen = frames;
            self.proof_cause_mask = (0..6).filter(|&i| self.proof_cause_at[i].elapsed() < std::time::Duration::from_millis(1500)).fold(0, |m, i| m | 1 << i);
        }
        let causes = self.proof_cause_mask;
        let b = self.proof_base;
        let v = Counters { ok: cur.ok.saturating_sub(b.ok), ramp: cur.ramp.saturating_sub(b.ramp), alt: cur.alt.saturating_sub(b.alt), dry: cur.dry.saturating_sub(b.dry), causes };
        // parça sonunda halka doğal olarak boşalır: son saniyedeki boşalmalar kesinti sayılmaz
        let dur = self.player.duration();
        if dur <= 0.0 || self.player.position() < dur - 1.0 {
            self.proof_dry_shown = v.dry;
        }
        if cur.alt != self.proof_alt_seen {
            self.proof_alt_seen = cur.alt;
            self.proof_alt_at = Instant::now();
        }
        self.proof_view = v;
    }

    fn proof_light_state(&self) -> Light {
        let Some(r) = &self.running else { return Light::Idle };
        let exclusive = r.ctl.out_info.lock().unwrap().exclusive;
        proof::light(&LightIn {
            playing: self.player.playing.load(Relaxed) && !self.meta.title.is_empty(),
            dop: r.dop,
            loopback: self.cfg.use_loopback,
            exclusive,
            resampled: self.meta.resampled,
            rg: self.player.rg_applied_db().is_some(),
            recent_alt: self.proof_alt_at.elapsed() < std::time::Duration::from_secs(1),
            dry: self.proof_dry_shown,
        })
    }

    fn receipt(&self, light: Light) -> Receipt {
        let v = self.proof_view;
        let title = match light {
            Light::Idle => "Doğrulama bekliyor",
            Light::Perfect => "Bit-perfect · doğrulandı",
            Light::Altered => "Bit-perfect değil",
            Light::Glitch => "Kesinti yaşandı",
        };
        let mut rows: Vec<(bool, String)> = vec![];
        let mut device = String::new();
        let mut counts = None;
        if let Some(r) = &self.running {
            let o = r.ctl.out_info.lock().unwrap().clone();
            device = format!("{} · {} · {:.1} kHz / {} bit", o.device, if o.exclusive { "Exclusive" } else { "Paylaşımlı" }, o.rate as f32 / 1000.0, o.bits);
            if self.cfg.use_loopback {
                rows.push((false, "Sistem DSP kaynağı: ses Windows'tan geçiyor".to_string()));
            } else {
                rows.push((o.exclusive, if o.exclusive { "Exclusive mod: Windows mikseri atlanıyor".into() } else { "Paylaşımlı mod: Windows mikseri sesi işliyor".into() }));
                if r.dop {
                    rows.push((true, "DSD, PCM'e çevrilmeden DoP ile gidiyor".into()));
                } else {
                    let drift = v.causes & proof::DRIFT != 0;
                    rows.push(if self.meta.resampled || drift {
                        let why = if self.probe_rates.is_empty() {
                            "cihazın kabul ettiği hızlar alınamadı".to_string()
                        } else {
                            let list: Vec<String> = self.probe_rates.iter().map(|r| format!("{:.1}", *r as f32 / 1000.0)).collect();
                            format!("cihazın kabul ettiği: {} kHz", list.join(" · "))
                        };
                        (false, format!("Hız dönüştürülüyor ({:.1} → {:.1} kHz); {why}", self.meta.src_rate as f32 / 1000.0, o.rate as f32 / 1000.0))
                    } else {
                        (true, "Hız dönüşümü yok".into())
                    });
                    rows.push(match self.player.rg_applied_db() {
                        Some(db) => (false, format!("ReplayGain uygulanıyor ({db:+.1} dB)")),
                        None => (true, "ReplayGain uygulanmıyor".into()),
                    });
                    let gain_ok = v.causes & proof::GAIN == 0 && self.player.volume() >= 0.999;
                    rows.push((gain_ok, if gain_ok { "Ses seviyesi %100".into() } else { "Ses seviyesi ya da dengeleme değiştirilmiş".into() }));
                    let dsp = v.causes & proof::DSP != 0;
                    rows.push((!dsp, if dsp { "EQ / modüller sinyali değiştiriyor".into() } else { "EQ ve modüller sinyale dokunmuyor".into() }));
                    let dith = v.causes & proof::DITHER != 0;
                    rows.push((!dith, if dith { "Dither ekleniyor".into() } else { "Dither eklenmiyor".into() }));
                }
                rows.push(if self.proof_dry_shown == 0 { (true, "Kesinti yok".into()) } else { (false, format!("{} kesinti (tampon boşaldı)", self.proof_dry_shown)) });
            }
            if !r.dop && (v.ok + v.alt) > 0 {
                counts = Some(format!("Doğrulanan çerçeve {}  ·  Fark {}  ·  Geçiş {}", proof::group(v.ok), proof::group(v.alt), proof::group(v.ramp)));
            }
        }
        Receipt { light, title, source: self.format_badge().unwrap_or_default(), device, rows, counts }
    }

    /// Alt çubuktaki ışık. Üzerine gelince makbuz açılır.
    pub(super) fn proof_light(&mut self, ui: &mut egui::Ui) {
        let light = self.proof_light_state();
        let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 32.0), Sense::hover());
        let col = match light {
            Light::Idle => alpha(DIM, 110),
            Light::Perfect => GREEN,
            Light::Altered => AMBER,
            Light::Glitch => RED,
        };
        let c = rect.center();
        ui.painter().circle_filled(c, 9.0, alpha(col, 38));
        ui.painter().circle_filled(c, 5.0, col);
        let receipt = self.receipt(light);
        resp.on_hover_ui(|ui| receipt_ui(ui, &receipt));
    }
}

fn receipt_ui(ui: &mut egui::Ui, r: &Receipt) {
    ui.set_max_width(330.0);
    let col = match r.light {
        Light::Idle => DIM,
        Light::Perfect => GREEN,
        Light::Altered => AMBER,
        Light::Glitch => RED,
    };
    ui.horizontal(|ui| {
        let (d, _) = ui.allocate_exact_size(vec2(12.0, 16.0), Sense::hover());
        ui.painter().circle_filled(d.center(), 5.0, col);
        ui.label(RichText::new(r.title).font(bold(14.5)).color(Color32::WHITE));
    });
    if !r.source.is_empty() {
        ui.label(RichText::new(format!("Kaynak: {}", r.source)).font(body(12.0)).color(TEXT));
    }
    if !r.device.is_empty() {
        ui.label(RichText::new(format!("Çıkış: {}", r.device)).font(body(12.0)).color(TEXT));
    }
    if r.rows.is_empty() {
        ui.add_space(4.0);
        ui.label(RichText::new("Çalmaya başlayınca ölçüm burada görünür.").font(body(12.0)).color(DIM));
    } else {
        ui.add_space(6.0);
        for (ok, text) in &r.rows {
            ui.horizontal(|ui| {
                let (d, _) = ui.allocate_exact_size(vec2(12.0, 16.0), Sense::hover());
                ui.painter().circle_filled(d.center(), 3.5, if *ok { GREEN } else { AMBER });
                ui.label(RichText::new(text).font(body(12.0)).color(if *ok { TEXT } else { Color32::WHITE }));
            });
        }
    }
    if let Some(c) = &r.counts {
        ui.add_space(6.0);
        ui.label(RichText::new(c).font(body(11.5)).color(TEXT));
    }
    ui.add_space(6.0);
    ui.label(RichText::new("Ölçüm yazılımdan cihaz tamponuna kadar olan yolu kapsar; sürücü ve DAC içindeki işlem doğrulanamaz.").font(body(10.5)).color(DIM));
}
