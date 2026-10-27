# Sürüm yayınlama

Sürümler GitHub Actions ile derlenir ve GitHub Release olarak yayınlanır. Uygulama yeni sürümleri bu Release'lerden bulur.

## Yeni sürüm çıkarmak

1. `Cargo.toml` içindeki `version` değerini yükselt (ör. `0.6.0`).
2. Değişikliği commit'le ve pushla.
3. Aynı numarayla bir etiket oluştur ve pushla:

   ```bash
   git tag v0.6.0
   git push origin v0.6.0
   ```

4. `Release` iş akışı çalışır: etiketin `Cargo.toml` sürümüyle aynı olduğunu denetler, uygulamayı derler ve Release'e iki dosya yükler:
   - `Soundworks-windows-x64.exe`
   - `Soundworks-windows-x64.exe.sha256`

   Sürüm notları, son sürümden beri yapılan commit'lerden ve birleştirilen değişikliklerden otomatik üretilir. Release sayfasından düzenleyebilirsin.

Dosya adları değişmemeli: uygulama bu iki adı arar. Etiket ile `Cargo.toml` uyuşmazsa iş akışı hata verip durur, yani yanlışlıkla eski numarayla yeni sürüm yayınlanmaz.

## Uygulama güncellemeyi nasıl bulur

- Açıldıktan kısa süre sonra ve en fazla 12 saatte bir `https://api.github.com/repos/akasperanza/soundworks/releases/latest` adresine bakar. Kullanıcı Ayarlar > Uygulama > Güncellemeler'den otomatik denetimi kapatabilir ya da "Şimdi denetle" diyebilir.
- Release'teki etiket, çalışan sürümden büyükse üst çubukta "Yeni sürüm x.y.z" düğmesi çıkar. Taslak (draft) ve ön sürüm (prerelease) olarak işaretlenen Release'ler yok sayılır.
- Kullanıcı "Şimdi güncelle" derse exe indirilir, `.sha256` dosyasındaki değerle doğrulanır, çalışan exe `Soundworks.exe.old` olarak kenara alınır, yenisi yerine konur ve uygulama kendini yeniden başlatır. Eski dosya sonraki açılışta silinir.
- İndirme yalnızca `github.com` ve GitHub'ın dosya sunucularından kabul edilir. Doğrulama başarısız olursa hiçbir şey değiştirilmez.

## Notlar

- Depo **public** olmalı. Private depoda bu adres giriş istediği için uygulama güncelleme bulamaz (sessizce "güncel" görünür).
- `.sha256` dosyası bozulmayı ve eksik indirmeyi yakalar. Birisi Release'e yazma yetkisini ele geçirirse hem exe'yi hem de özeti değiştirebilir. Bu risk kod imzası eklenince kapanır.
