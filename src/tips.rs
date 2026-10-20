//! İpuçları ve uyarılar: bir odyofilin unutabileceği ya da bilmesi gereken durumlar.
//! `evaluate` saf bir fonksiyondur: durum bilgisinden (Facts) o an geçerli uyarıları öncelik sırasıyla üretir.

use crate::engine::DeviceKind;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TipIcon {
    Bluetooth,
    Info,
    Warning,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Tip {
    /// kalıcı gizleme anahtarı
    pub id: &'static str,
    pub icon: TipIcon,
    pub title: String,
    pub body: String,
}

/// Uyarıların karar verdiği anlık durum.
#[derive(Clone, Default, Debug)]
pub struct Facts {
    pub running: bool,
    pub loopback: bool,
    pub exclusive: bool,
    /// kullanıcı Exclusive'i bilerek kapattı mı (Ayarlar)
    pub exclusive_pref_off: bool,
    pub out_bits: u16,
    pub out_float: bool,
    pub dev_kind: Option<DeviceKindOpt>,
    pub dev_name: String,
    /// cihazın exclusive modda kabul ettiği hızlar (bilinmiyorsa boş)
    pub supported: Vec<u32>,
    pub src_bits: u32,
    pub resampled: bool,
    /// 0..1 yazılım ses seviyesi
    pub volume: f32,
    /// oturumda 0 dBFS'e çarpan örnek sayısı
    pub clips: u32,
    pub has_limiter: bool,
    /// cihazın istenen hızı kabul etmeyip başka hızda açıldığı: (istenen, açılan)
    pub rate_notice: Option<(u32, u32)>,
}

/// `Option<DeviceKind>` Default için sarmalayıcı.
pub type DeviceKindOpt = DeviceKind;

fn khz(r: u32) -> String {
    let k = r as f32 / 1000.0;
    if (k - k.round()).abs() < 0.001 {
        format!("{k:.0}")
    } else {
        format!("{k:.1}")
    }
}

/// O an geçerli uyarılar, en önemlisi başta.
pub fn evaluate(f: &Facts) -> Vec<Tip> {
    let mut out: Vec<Tip> = vec![];
    if !f.running {
        return out;
    }
    let bt = f.dev_kind == Some(DeviceKind::Bluetooth);
    let name = f.dev_name.to_lowercase();

    if bt && (name.contains("hands-free") || name.contains("handsfree")) {
        out.push(Tip {
            id: "bt_handsfree",
            icon: TipIcon::Bluetooth,
            title: "Kulaklık telefon kalitesinde bağlı (Hands-Free)".into(),
            body: "Bu cihaz konuşma profilinde (HFP) çalışıyor: ses mono ve çok düşük çözünürlüğe iniyor. Windows Ses ayarlarından aynı kulaklığın \"Stereo\" (Kulaklıklar) cihazını seç.".into(),
        });
    }
    if let Some((want, got)) = f.rate_notice {
        out.push(Tip {
            id: "rate_mismatch",
            icon: TipIcon::Warning,
            title: format!("Cihaz {} kHz'i kabul etmedi", khz(want)),
            body: format!(
                "{} kHz'de çalıyor; parçalar bu hıza yeniden örnekleniyor. Cihazın sürücüsü ya da Windows Ses > Gelişmiş ayarlarındaki varsayılan biçim bu hızları desteklemiyor olabilir.",
                khz(got)
            ),
        });
    }
    if bt {
        out.push(Tip {
            id: "bluetooth",
            icon: TipIcon::Bluetooth,
            title: "Bluetooth ile dinliyorsun".into(),
            body: "Windows, ne kadar iyi bir kulaklık kullanırsan kullan Bluetooth'ta varsayılan olarak sınırlı bir codec (genellikle SBC ya da AAC) kullanır; LDAC ve aptX gibi yüksek kaliteli codec'ler çoğu zaman devreye girmez. Kabloyla dinleme şansın varsa kabloyla dinle.".into(),
        });
    }
    if !f.exclusive {
        if f.exclusive_pref_off {
            out.push(Tip {
                id: "shared_mode",
                icon: TipIcon::Info,
                title: "Exclusive mod kapalı".into(),
                body: "Shared modda ses Windows karıştırıcısından geçer: yeniden örneklenir ve Dolby Atmos gibi ses efektleri devrede kalabilir. Bit-perfect için Ayarlar'dan Exclusive'i aç.".into(),
            });
        } else {
            out.push(Tip {
                id: "shared_fallback",
                icon: TipIcon::Warning,
                title: "Exclusive mod başlatılamadı, Shared'a düşüldü".into(),
                body: "Cihaz başka bir uygulamada kullanımda olabilir ya da bu biçimi kabul etmiyor. Diğer ses uygulamalarını kapatıp Soundworks'ü yeniden başlat. Şu an ses Windows karıştırıcısından geçiyor.".into(),
            });
        }
    }
    if f.clips >= 30 && !f.has_limiter {
        out.push(Tip {
            id: "clipping",
            icon: TipIcon::Warning,
            title: "Ses sık sık 0 dBFS'e çarpıyor".into(),
            body: "Tepeler tam ölçeğe dayanıyor, kırpılma bozulması duyabilirsin. EQ preamp'ini düşür ya da Motor > Modüller'den bir Limiter ekle.".into(),
        });
    }
    if f.loopback {
        out.push(Tip {
            id: "loopback_format",
            icon: TipIcon::Info,
            title: "Sistem DSP: formatı Windows belirler".into(),
            body: "Sistem sesi sanal kabloya Windows'un seçtiği formatta (genellikle 48 kHz) gider; kaynak daha yüksek çözünürlüklü olsa bile ona düşer. Ses ayarlarında sanal kablonun formatını en yüksek kaliteye (ör. 24 bit, 192 kHz) ayarla.".into(),
        });
    }
    if !f.loopback && !f.out_float && f.out_bits > 0 && f.out_bits <= 16 && f.src_bits > 16 {
        out.push(Tip {
            id: "bit16",
            icon: TipIcon::Info,
            title: "Çıkış 16 bit".into(),
            body: format!(
                "{} bit kaynak 16 bit'e indiriliyor (dither uygulanıyor). Cihazın ya da sürücünün 24 bit desteği varsa onu kullan; çoğu Bluetooth cihaz yalnızca 16 bit çalışır.",
                f.src_bits
            ),
        });
    }
    if !f.loopback && f.resampled && f.supported.len() == 1 {
        out.push(Tip {
            id: "single_rate",
            icon: TipIcon::Info,
            title: "Cihaz tek bir örnekleme hızı kabul ediyor".into(),
            body: format!(
                "Çıkış cihazı exclusive modda yalnızca {} kHz çalıyor; farklı hızdaki parçalar bu hıza yeniden örnekleniyor. Bu cihazın sınırı, Soundworks'ün değil.",
                khz(f.supported[0])
            ),
        });
    }
    if !f.loopback && f.volume < 0.7 {
        out.push(Tip {
            id: "soft_volume",
            icon: TipIcon::Info,
            title: "Ses seviyesi yazılımda düşürülüyor".into(),
            body: "Yazılımla kısmak her 6 dB'de yaklaşık 1 bit çözünürlük götürür (dither bunu hafifletir). Mümkünse DAC'ın ya da amfinin kendi ses kontrolünü kullanıp burada %100'e yakın bırak.".into(),
        });
    }
    if !f.loopback && f.exclusive {
        out.push(Tip {
            id: "exclusive_note",
            icon: TipIcon::Info,
            title: "Exclusive mod açık".into(),
            body: "Exclusive açıkken diğer uygulamalar (tarayıcı, Discord, oyunlar) bu cihazdan ses çıkaramaz. Başka bir uygulamadan ses gelmiyorsa Soundworks'ü durdur.".into(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Facts {
        Facts { running: true, exclusive: true, volume: 1.0, out_bits: 24, ..Default::default() }
    }
    fn ids(f: &Facts) -> Vec<&'static str> {
        evaluate(f).iter().map(|t| t.id).collect()
    }

    #[test]
    fn nothing_when_stopped() {
        let mut f = base();
        f.running = false;
        f.dev_kind = Some(DeviceKind::Bluetooth);
        assert!(ids(&f).is_empty());
    }

    #[test]
    fn bluetooth_and_handsfree() {
        let mut f = base();
        f.dev_kind = Some(DeviceKind::Bluetooth);
        f.dev_name = "Kulaklıklar (MOMENTUM 4)".into();
        assert!(ids(&f).contains(&"bluetooth"));
        assert!(!ids(&f).contains(&"bt_handsfree"));
        f.dev_name = "Hands-Free AG Audio (MOMENTUM 4)".into();
        let v = ids(&f);
        assert_eq!(v[0], "bt_handsfree"); // en önemlisi başta
        assert!(v.contains(&"bluetooth"));
        // Bluetooth olmayan cihazda yok
        f.dev_kind = Some(DeviceKind::Usb);
        assert!(!ids(&f).contains(&"bluetooth") && !ids(&f).contains(&"bt_handsfree"));
    }

    #[test]
    fn exclusive_variants() {
        let mut f = base();
        assert!(ids(&f).contains(&"exclusive_note"));
        f.exclusive = false;
        f.exclusive_pref_off = true;
        assert!(ids(&f).contains(&"shared_mode") && !ids(&f).contains(&"shared_fallback"));
        f.exclusive_pref_off = false;
        assert!(ids(&f).contains(&"shared_fallback") && !ids(&f).contains(&"exclusive_note"));
    }

    #[test]
    fn clipping_needs_enough_clips_and_no_limiter() {
        let mut f = base();
        f.clips = 5;
        assert!(!ids(&f).contains(&"clipping"));
        f.clips = 400;
        assert!(ids(&f).contains(&"clipping"));
        f.has_limiter = true;
        assert!(!ids(&f).contains(&"clipping"));
    }

    #[test]
    fn bit_depth_rate_and_volume_tips() {
        let mut f = base();
        f.out_bits = 16;
        f.src_bits = 24;
        f.resampled = true;
        f.supported = vec![48000];
        f.volume = 0.5;
        let v = ids(&f);
        assert!(v.contains(&"bit16") && v.contains(&"single_rate") && v.contains(&"soft_volume"));
        // 16 bit kaynak 16 bit'e inerken uyarı gerekmez
        f.src_bits = 16;
        assert!(!ids(&f).contains(&"bit16"));
        // float çıkışta yok
        f.src_bits = 24;
        f.out_float = true;
        assert!(!ids(&f).contains(&"bit16"));
    }

    #[test]
    fn loopback_and_rate_mismatch() {
        let mut f = base();
        f.loopback = true;
        f.volume = 0.2;
        let v = ids(&f);
        assert!(v.contains(&"loopback_format"));
        assert!(!v.contains(&"soft_volume") && !v.contains(&"exclusive_note")); // dahili oynatıcıya özgü
        let mut g = base();
        g.rate_notice = Some((96000, 48000));
        let t = evaluate(&g);
        let m = t.iter().find(|t| t.id == "rate_mismatch").unwrap();
        assert!(m.title.contains("96") && m.body.contains("48"));
    }

    #[test]
    fn all_tips_have_text_and_unique_ids() {
        let f = Facts {
            running: true,
            loopback: false,
            exclusive: false,
            exclusive_pref_off: true,
            out_bits: 16,
            dev_kind: Some(DeviceKind::Bluetooth),
            dev_name: "Hands-Free".into(),
            supported: vec![48000],
            src_bits: 24,
            resampled: true,
            volume: 0.3,
            clips: 999,
            rate_notice: Some((44100, 48000)),
            ..Default::default()
        };
        let t = evaluate(&f);
        let mut seen = std::collections::HashSet::new();
        for x in &t {
            assert!(!x.title.is_empty() && x.body.len() > 40, "{}", x.id);
            assert!(seen.insert(x.id), "tekrarlayan id {}", x.id);
        }
        assert!(t.len() >= 7);
    }
}
