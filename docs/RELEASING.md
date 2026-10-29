# Sürüm yayınlama

Sürümler GitHub Actions ile derlenir ve GitHub Release olarak yayınlanır. Uygulama yeni sürümleri bu Release'lerden bulur.

## Yeni sürüm çıkarmak

1. `Cargo.toml` içindeki `version` değerini yükselt (ör. `0.6.0`).
2. `CHANGELOG.md` dosyasının en üstüne, giriş paragrafının altına yeni sürümün bölümünü ekle (nasıl yazılacağı aşağıda).
3. Değişiklikleri commit'le ve pushla.
4. Aynı numarayla bir etiket oluştur ve pushla:

   ```bash
   git tag v0.6.0
   git push origin v0.6.0
   ```

5. `Release` iş akışı çalışır: etiketin `Cargo.toml` sürümüyle aynı olduğunu ve `CHANGELOG.md` içinde o sürüm için not bulunduğunu denetler, uygulamayı derler ve Release'e şunları yükler:
   - `Soundworks-windows-x64.exe`
   - `Soundworks-windows-x64.exe.sha256`

   Release'in açıklaması, `CHANGELOG.md` içindeki o sürümün bölümüdür.

Dosya adları değişmemeli: uygulama bu iki adı arar. Etiket ile `Cargo.toml` uyuşmazsa ya da değişiklik günlüğünde o sürüm yoksa iş akışı hata verip durur, yani yanlışlıkla eski numarayla ya da notsuz sürüm yayınlanmaz.

## Değişiklik günlüğü nasıl yazılır

Uygulamadaki "Yeni sürüm hazır" ve güncelleme sonrası "Soundworks güncellendi" ekranları sürüm notlarını Release açıklamasından alır, yani yazdığın metin doğrudan kullanıcıya gösterilir.

```markdown
## 0.6.0

<!-- tr -->
- Yeni: şu özellik eklendi
- Düzeltme: şu hata giderildi

<!-- en -->
- New: this feature was added
- Fix: this bug was fixed

<!-- de -->
- Neu: diese Funktion wurde hinzugefügt
- Korrektur: dieser Fehler wurde behoben
```

- Başlık tam olarak `## <sürüm>` biçiminde olmalı (`v` olmadan, yalnızca sürüm numarası). Bölüm bir sonraki `## ` başlığına kadar sürer.
- `<!-- tr -->`, `<!-- en -->`, `<!-- de -->` satırları dil bloklarını ayırır. Uygulama kullanıcının diline uyan bloğu gösterir. O dil yoksa İngilizce, o da yoksa Türkçe bloğa düşer. Hiç dil satırı yoksa bütün metin herkese gösterilir. GitHub bu satırları sayfada göstermez.
- Gösterilen biçimler: `##` başlıklar, `-` ya da `*` ile madde işaretleri ve düz paragraflar. `**kalın**`, `` `kod` `` ve `[bağlantı](url)` işaretleri temizlenir. En fazla 40 satır gösterilir, satırlar 220 karaktere kısaltılır, bu yüzden maddeleri kısa tut.
- Release yayınlandıktan sonra GitHub'da açıklamayı düzenlersen bir sonraki denetimde yeni metin görünür. Güncellemeyi uygulama yaptıysa o anda kaydedilmiş metin, elle yaptıysan GitHub'daki güncel metin kullanılır.

## Uygulama güncellemeyi nasıl bulur

- Açıldıktan kısa süre sonra ve en fazla 12 saatte bir `https://api.github.com/repos/akasperanza/soundworks/releases/latest` adresine bakar. Kullanıcı Ayarlar > Uygulama > Güncellemeler'den otomatik denetimi kapatabilir ya da "Şimdi denetle" diyebilir.
- Release'teki etiket, çalışan sürümden büyükse üst çubukta "Yeni sürüm x.y.z" düğmesi çıkar. Taslak (draft) ve ön sürüm (prerelease) olarak işaretlenen Release'ler yok sayılır.
- Kullanıcı "Şimdi güncelle" derse exe indirilir, `.sha256` dosyasındaki değerle doğrulanır, çalışan exe `Soundworks.exe.old` olarak kenara alınır, yenisi yerine konur ve uygulama kendini yeniden başlatır. Eski dosya sonraki açılışta silinir.
- İndirme yalnızca `github.com` ve GitHub'ın dosya sunucularından kabul edilir. Doğrulama başarısız olursa hiçbir şey değiştirilmez.

## Notlar

- Depo **public** olmalı. Private depoda bu adres giriş istediği için uygulama güncelleme bulamaz (sessizce "güncel" görünür).
- `.sha256` dosyası bozulmayı ve eksik indirmeyi yakalar. Birisi Release'e yazma yetkisini ele geçirirse hem exe'yi hem de özeti değiştirebilir. Bu risk kod imzası eklenince kapanır.
