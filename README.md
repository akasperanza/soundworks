# Soundworks

Windows için, odyofiller düşünülerek yazılmış bir masaüstü müzik çalar ve ses işleme motoru. Rust ve egui ile yazıldı.

Soundworks'ün çıkış noktası şu soru: *"Bit-perfect çalıyorum"* demek kolay, ama bunu kim ölçüyor? Çoğu oynatıcı yalnızca ayarlara bakıp karar verir. Soundworks cihaza yazılan her çerçeveyi ölçer ve sonucu alt çubuktaki ışıkta gösterir.

## Öne çıkanlar

**Bit-perfect, ölçülerek**
- WASAPI exclusive modda çıkış, cihaz bazında seçim (cihaz kimliğiyle, adı değişse de bulunur).
- Alt çubukta kuyruk düğmesinin yanındaki ışık: yeşil (ölçüldü, birebir), sarı (bilinen bir nedenle değiştiriliyor), kırmızı (kesinti), gri (çalmıyor). Üzerine gelince makbuz açılır: kaynak biçimi, çıkış, satır satır kontroller, doğrulanan çerçeve sayısı.
- Motor, cihaza yazılan değerin halkadan gelen örnekle aynı olup olmadığına her çerçevede bakar. Ölçüm yazılımdan cihaz tamponuna kadar olan yolu kapsar; sürücü ve DAC içindeki işlem doğrulanamaz.
- Hız parçaya göre ayarlanabilir (yeniden örnekleme olmadan), 16 bit çıkışta yalnızca sinyale dokunulduysa dither uygulanır.

**DSD**
- DSF ve DFF dosyaları: varsayılan olarak PCM'e çevrilerek çalınır (güvenli yol).
- İsteğe bağlı DoP (DSD over PCM): yalnızca USB cihazlarda sunulur, cihaz başına hatırlanır, DAC'ın gerekli hızı kabul edip etmediği önceden denenir. DoP'un yanlış cihazda yüksek sesli gürültü çıkarabileceği, seçmeden önce uyarı diyaloğunda açıkça anlatılır.

**Ses işleme**
- Parametrik EQ (APO uyumlu ön ayarlar), A/B karşılaştırma, bant solo.
- Modül zinciri, true-peak limiter, crossfeed, denge, mono.
- ReplayGain (parça/albüm), canlı LUFS (momentary, short-term, integrated) ve gerçek tepe ölçümü.
- Sistem DSP kipi: sanal bir kablo (ör. VB-Cable) üzerinden tüm uygulamaların sesini yakalayıp işleyebilir.

**Kütüphane ve arayüz**
- Albüm ve sanatçı görünümü, tür/ad filtresi ve sıralama, izlenen klasörler, eksik dosya denetimi.
- Senkronlu şarkı sözleri (kelime ve satır düzeyinde), şarkıya özel söz kayması ayarı.
- Dalga formu arama çubuğu, spektrum/spektrogram/osiloskop görselleştirmeleri.
- Tepsi simgesi, genel medya tuşları ve kısayollar, kaldığın yerden devam.

**Desteklenen biçimler:** mp3, flac, wav, aiff/aif, caf, ogg, m4a/aac/mp4/alac, mka, dsf, dff.

## Derleme

Yalnızca Windows 10/11. [Rust](https://rustup.rs) gerekli.

```bash
cargo build --release
```

Çıktı: `target/release/soundworks.exe`.

Testler:

```bash
cargo test --release
```

Doğrulama kancaları (durum izi, ekran görüntüsü, otomatik kapatma vb.) normal derlemeye girmez; yalnızca şu özellikle açılır:

```bash
cargo build --release --features devtools
```

## Proje yapısı

| Yol | İçerik |
|---|---|
| `src/engine.rs` | WASAPI çıkış motoru, halka, DSP zinciri, DoP |
| `src/player.rs` | Çözücü iş parçacığı, çalma kuyruğu, hız ve mod geçişleri |
| `src/proof.rs` | Bit-perfect ölçümü (sayaçlar, ışık kararı) |
| `src/dsd.rs` | DSF/DFF okuyucu, DSD→PCM dönüştürücü, DoP çerçeveleme |
| `src/fx.rs`, `src/biquad.rs`, `src/loudness.rs` | Modüller, filtreler, LUFS/true-peak |
| `src/library.rs`, `src/web.rs` | Kütüphane; söz, sanatçı ve tür sorguları |
| `src/gui.rs`, `src/gui/` | Arayüz (konuya göre bölünmüş görünümler) |

## Durum ve bilinen sınırlar

Proje aktif geliştirme altında.

- DoP yolu yalnızca sentetik dosyalarla ve sanal cihazla denendi; gerçek bir DAC'ta henüz test edilmedi. İlk denemede sesi en aza indir.
- DSD için gerçek (üçüncü taraf) dosyalarla doğrulama yapılmadı.
- Opus şu an desteklenmiyor.

## Lisans

Henüz belirtilmedi. `vendor/` altındaki üçüncü taraf kodlar kendi lisanslarına tabidir. İkonlar [Lucide](https://lucide.dev) (ISC lisansı).
