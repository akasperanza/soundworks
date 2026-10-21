use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OnExit {

    ShowError,

    Reconnect,
}

pub fn on_unexpected_exit(gone: bool, explicit_target: bool, loopback: bool) -> OnExit {
    if gone && explicit_target && !loopback {
        OnExit::Reconnect
    } else {
        OnExit::ShowError
    }
}

pub fn should_retry(since_last_try: Duration, device_present: bool) -> bool {
    device_present && since_last_try >= Duration::from_millis(1500)
}

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

        assert_eq!(on_unexpected_exit(true, false, false), OnExit::ShowError);

        assert_eq!(on_unexpected_exit(false, true, false), OnExit::ShowError);
        assert_eq!(on_unexpected_exit(true, true, true), OnExit::ShowError);
    }

    #[test]
    fn retry_waits_for_device_and_spacing() {
        assert!(!should_retry(Duration::from_secs(10), false));
        assert!(!should_retry(Duration::from_millis(200), true));
        assert!(should_retry(Duration::from_secs(2), true));
    }

    #[test]
    fn gone_text_detection() {
        assert!(text_means_gone("cihaz bulunamadı: Kulaklıklar"));
        assert!(text_means_gone("Hata: Çıkış cihazı çıkarıldı ya da değişti."));
        assert!(!text_means_gone("Hata: Cihaz başka bir uygulamada özel kullanımda"));
    }
}
