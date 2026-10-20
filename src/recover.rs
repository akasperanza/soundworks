//! Cihaz koparsa (Bluetooth bağlantısı gitti, USB DAC çıkarıldı) otomatik yeniden bağlanma kararları.
//! Saf fonksiyonlar: arayüz yalnızca bunların sonucunu uygular.

use std::time::Duration;

/// Motor beklenmedik şekilde bittiğinde ne yapılacağı.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OnExit {
    /// kullanıcıya hata göster, otomatik bir şey yapma
    ShowError,
    /// cihaz geri gelince kendiliğinden devam et
    Reconnect,
}

/// - `gone`: hata, cihazın kaybolduğu türden mi (invalidated / bulunamadı)
/// - `explicit_target`: kullanıcı belirli bir hedef cihaz seçmiş mi. Seçmemişse Windows varsayılanı kaybolunca
///   başka (ör. hoparlör) cihaza geçip sesi beklenmedik yerden vermemek için otomatik bağlanma YAPILMAZ.
/// - `loopback`: sistem DSP modunda yönlendirme karmaşık olduğu için otomatik bağlanma yok.
pub fn on_unexpected_exit(gone: bool, explicit_target: bool, loopback: bool) -> OnExit {
    if gone && explicit_target && !loopback {
        OnExit::Reconnect
    } else {
        OnExit::ShowError
    }
}

/// Yeniden bağlanmayı ne zaman denemeli: cihaz listede yeniden göründüyse ve son denemeden yeterince zaman geçtiyse.
pub fn should_retry(since_last_try: Duration, device_present: bool) -> bool {
    device_present && since_last_try >= Duration::from_millis(1500)
}

/// Hata metninden "cihaz kayboldu" sınıfı mı (HRESULT yoksa, ör. cihaz bulunamadı).
pub fn text_means_gone(msg: &str) -> bool {
    let m = msg.to_lowercase();
    m.contains("cihaz bulunamadı") || m.contains("çıkış cihazı çıkarıldı")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnect_only_for_explicit_device_loss() {
        assert_eq!(on_unexpected_exit(true, true, false), OnExit::Reconnect);
        // varsayılan cihaz seçiliyse: başka cihaza kayma riski, otomatik bağlanma yok
        assert_eq!(on_unexpected_exit(true, false, false), OnExit::ShowError);
        // başka türden hata (ör. cihaz meşgul): sonsuz denemeye girme
        assert_eq!(on_unexpected_exit(false, true, false), OnExit::ShowError);
        assert_eq!(on_unexpected_exit(true, true, true), OnExit::ShowError);
    }

    #[test]
    fn retry_waits_for_device_and_spacing() {
        assert!(!should_retry(Duration::from_secs(10), false)); // cihaz yok: bekle
        assert!(!should_retry(Duration::from_millis(200), true)); // çok erken
        assert!(should_retry(Duration::from_secs(2), true));
    }

    #[test]
    fn gone_text_detection() {
        assert!(text_means_gone("cihaz bulunamadı: Kulaklıklar"));
        assert!(text_means_gone("Hata: Çıkış cihazı çıkarıldı ya da değişti."));
        assert!(!text_means_gone("Hata: Cihaz başka bir uygulamada özel kullanımda"));
    }
}
