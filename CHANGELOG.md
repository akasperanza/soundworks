# Değişiklik günlüğü

Her sürümün notları aşağıda `## <sürüm numarası>` başlığı altında yazılır. Sürüm yayınlanırken ilgili bölüm GitHub Release'in açıklaması olur ve uygulamadaki "Yeni sürüm hazır" ve "Soundworks güncellendi" ekranlarında gösterilir.

Notlar çok dillidir: `<!-- tr -->`, `<!-- en -->` ve `<!-- de -->` satırları dil bloklarını ayırır, uygulama kullanıcının diline uyan bloğu gösterir (olmazsa İngilizce, sonra Türkçe). Yazım kuralları için [docs/RELEASING.md](docs/RELEASING.md) dosyasına bakın.

## 0.5.2

<!-- tr -->
- Eksik metadata internetten tamamlanıyor: albüm, sanatçı, yıl ve parça numarası Deezer ve MusicBrainz'dan alınır (Ayarlar > Kütüphane > Metadata)
- İstersen bulunan bilgi dosya etiketlerine de yazılır: önce sorar, yalnızca boş alanları doldurur, yazılanlar geri alınabilir
- Güncelleme ekranı yeniden tasarlandı: sade, iki sütunlu, hareketli süsler yok

<!-- en -->
- Missing metadata is now filled in from the internet: album, artist, year and track number come from Deezer and MusicBrainz (Settings > Library > Metadata)
- Optionally, found info can also be written into the file tags: it asks first, fills only empty fields, and can be undone
- The update screen was redesigned: plain, two columns, no animated decorations

<!-- de -->
- Fehlende Metadaten werden jetzt aus dem Internet ergänzt: Album, Künstler, Jahr und Tracknummer von Deezer und MusicBrainz (Einstellungen > Bibliothek > Metadaten)
- Optional werden gefundene Infos auch in die Datei-Tags geschrieben: fragt vorher, füllt nur leere Felder und lässt sich rückgängig machen
- Der Update-Bildschirm wurde neu gestaltet: schlicht, zwei Spalten, keine animierten Verzierungen

## 0.5.1

<!-- tr -->
- Uyarılar ve ipuçları artık varsayılan olarak kapalı (Ayarlar > Uygulama'dan açılabilir)
- Düzeltme: zincirde tek modül varken onu sürüklemek uygulamayı çökertiyordu
- Kod temizliği ve iç iyileştirmeler

<!-- en -->
- Tips and warnings are now off by default (turn them on in Settings > App)
- Fix: dragging the only module in the chain crashed the app
- Code cleanup and internal improvements

<!-- de -->
- Hinweise und Warnungen sind jetzt standardmäßig aus (in Einstellungen > App einschaltbar)
- Korrektur: Das Ziehen des einzigen Moduls in der Kette brachte die App zum Absturz
- Code-Bereinigung und interne Verbesserungen

## 0.5.0

<!-- tr -->
- Üç dil: Türkçe, İngilizce, Almanca
- Bekleme ekranı: müzik çalarken fare ve klavye kullanılmazsa kapak, sıradakiler ve canlı spektrum tam ekran görünür
- Eklenti sistemi: JavaScript ile düğme, panel, komut, ağ ve indirme (Discord Rich Presence ve Sleep Timer örnekleri)
- Ayarlar yeniden tasarlandı: sol kategori listesi ve sade satırlar
- Ctrl + Motor sekmesi ile Motor ayrı pencerede açılır
- Otomatik güncelleme: yeni sürüm tam ekran bildirilir, tek tıkla indirilip kurulur
- Düzeltme: başlıktaki yazının bulanıklığı, seçim listelerinde çökme, metinlerin seçilebilmesi

<!-- en -->
- Three languages: Turkish, English, German
- Idle screen: when you stop interacting while music plays, the cover, up-next list and live spectrum fill the window
- Extension system: buttons, panels, commands, networking and downloads in JavaScript (Discord Rich Presence and Sleep Timer examples)
- Redesigned Settings: category list on the left and simpler rows
- Ctrl + click the Engine tab to open the Engine in a separate window
- Automatic updates: new versions are announced full screen and install with one click
- Fixes: blurry wordmark in the header, crash when opening selection lists, text no longer selectable

<!-- de -->
- Drei Sprachen: Türkisch, Englisch, Deutsch
- Ruhebildschirm: Wenn bei laufender Musik nichts bedient wird, füllen Cover, Warteschlange und Live-Spektrum das Fenster
- Erweiterungssystem: Schaltflächen, Panels, Befehle, Netzwerk und Downloads in JavaScript (Beispiele: Discord Rich Presence und Sleep Timer)
- Neu gestaltete Einstellungen: Kategorieliste links und schlichtere Zeilen
- Strg + Klick auf den Engine-Tab öffnet die Engine in einem eigenen Fenster
- Automatische Updates: Neue Versionen werden im Vollbild angekündigt und mit einem Klick installiert
- Korrekturen: unscharfer Schriftzug in der Kopfzeile, Absturz beim Öffnen von Auswahllisten, Text ist nicht mehr markierbar
