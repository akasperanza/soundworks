# Değişiklik günlüğü

Her sürümün notları aşağıda `## <sürüm numarası>` başlığı altında yazılır. Sürüm yayınlanırken ilgili bölüm GitHub Release'in açıklaması olur ve uygulamadaki "Yeni sürüm hazır" ve "Soundworks güncellendi" ekranlarında gösterilir.

Notlar çok dillidir: `<!-- tr -->`, `<!-- en -->` ve `<!-- de -->` satırları dil bloklarını ayırır, uygulama kullanıcının diline uyan bloğu gösterir (olmazsa İngilizce, sonra Türkçe). Yazım kuralları için [docs/RELEASING.md](docs/RELEASING.md) dosyasına bakın.

## 0.5.7

<!-- tr -->
- Waveform'a gelince üzerinde büyük bir ilerleme çubuğu açılır: oynatıcı çubuğunun tamamını kaplar, ikonların üstüne çıkar ve istediğin yere kolayca tıklayıp sürükleyebilirsin
- Fare ayrılınca çubuk eski haline döner
- Windows medya oynatıcısı olarak tanınır: kulaklık ve klavye medya tuşları, Windows ses/medya paneli ve kilit ekranı Soundworks'ü kontrol eder ve çalan parçayı gösterir
- Görev çubuğu önizlemesinde önceki / oynat-duraklat / sonraki düğmeleri çıkar
- Oynatıcıdaki parça adına ya da albüm kapağına tıklayınca albüme, sanatçı adına tıklayınca sanatçıya gidilir

<!-- en -->
- Hovering the waveform opens a large scrubber over the whole player bar, on top of the icons, so you can click and drag to any position easily
- It collapses back when the pointer leaves
- Soundworks is now recognized by Windows as a media player: headphone and keyboard media keys, the Windows volume/media flyout and the lock screen control it and show what's playing
- The taskbar thumbnail now has previous / play-pause / next buttons
- In the player, clicking the track title or the cover opens the album, clicking the artist name opens the artist

<!-- de -->
- Beim Überfahren der Waveform öffnet sich ein großer Positionsregler über der ganzen Player-Leiste, auch über den Symbolen, zum bequemen Klicken und Ziehen
- Er klappt wieder ein, sobald der Mauszeiger die Leiste verlässt
- Windows erkennt Soundworks jetzt als Mediaplayer: Kopfhörer- und Tastatur-Medientasten, das Windows-Lautstärke-/Medienfenster und der Sperrbildschirm steuern es und zeigen den laufenden Titel
- Die Taskleisten-Vorschau hat jetzt Schaltflächen für Zurück / Wiedergabe-Pause / Weiter
- Im Player öffnet ein Klick auf Titel oder Cover das Album, ein Klick auf den Künstlernamen den Künstler

## 0.5.6

<!-- tr -->
- Albümlere ve parçalara sağ tıklayınca menü açılır: çal, sıraya ekle, dosya gezgininde göster, dosya yolunu kopyala, metadatayı düzenle, Soundworks'ten sil ve dosyayı sil
- Silme işlemleri önce "emin misin?" diye sorar; dosyalar kalıcı silinmez, geri dönüşüm kutusuna taşınır
- Soundworks'ten silinen parçalar sonraki klasör taramalarında geri eklenmez
- Yeni metadata düzenleyici: bir parçanın ya da tüm albümün başlık, sanatçı, albüm, yıl ve parça numarası etiketlerini dosyalara yazar
- Kapağı olmayan albümlerin kapağı Deezer'dan bulunur (Ayarlar'dan kapatılabilir); albüm menüsünden elle de aranabilir
- Waveform üzerine gelince yükselir ve vuruş alanı genişler, ilerletmek ve seçim yapmak kolaylaşır

<!-- en -->
- Right-click albums and tracks for a menu: play, add to queue, show in File Explorer, copy file path, edit metadata, remove from Soundworks and delete file
- Deleting always asks "are you sure?" first; files are never erased permanently, they go to the recycle bin
- Tracks removed from Soundworks are not added back by later folder scans
- New metadata editor: writes title, artist, album, year and track number tags to the files for one track or a whole album
- Covers for albums without artwork are found on Deezer (can be turned off in Settings); you can also search for one by hand from the album menu
- The waveform grows taller and its hit area widens on hover, making it easy to scrub and pick a position

<!-- de -->
- Rechtsklick auf Alben und Titel öffnet ein Menü: abspielen, zur Warteschlange, im Explorer zeigen, Dateipfad kopieren, Metadaten bearbeiten, aus Soundworks entfernen und Datei löschen
- Löschen fragt immer vorher nach; Dateien werden nie endgültig gelöscht, sondern in den Papierkorb verschoben
- Aus Soundworks entfernte Titel werden von späteren Ordner-Scans nicht wieder hinzugefügt
- Neuer Metadaten-Editor: schreibt Titel, Künstler, Album, Jahr und Titelnummer für einen Titel oder ein ganzes Album in die Dateien
- Cover für Alben ohne Artwork werden bei Deezer gefunden (in den Einstellungen abschaltbar); über das Albummenü lässt sich auch manuell suchen
- Die Waveform wird beim Überfahren höher und ihre Trefferfläche größer, so lässt sich leicht eine Position wählen

## 0.5.5

<!-- tr -->
- Oynatıcı çubuğu cam gibi içeriğin üstünde yüzer, altında yumuşak bir karartma okunurluğu korur
- Sağ paneller açıkken çubuk onların yanında ortalanır; Sözler ve Kuyruk açıkken Motor'daki seviye paneli gizlenir
- Albüm değiştirirken arka plan yumuşakça geçiş yapar
- Dar pencerede kütüphane filtre satırı iki satıra bölünür, üst üste binmez
- Panel gölgeleri hafifletildi; arka plan renkleri daha dengeli

<!-- en -->
- The player bar floats over the content like glass, with a soft fade underneath to keep it readable
- With right panels open, the bar centers beside them; the level panel in Engine hides while Lyrics or Queue is open
- The backdrop now transitions smoothly when switching albums
- In narrow windows the library filter row wraps to two lines instead of overlapping
- Panel shadows were softened; backdrop colors are more balanced

<!-- de -->
- Die Player-Leiste schwebt wie Glas über dem Inhalt, ein sanfter Verlauf darunter sorgt für Lesbarkeit
- Bei geöffneten rechten Panels wird die Leiste daneben zentriert; das Pegel-Panel in Engine wird bei Songtexten oder Warteschlange ausgeblendet
- Der Hintergrund wechselt beim Albumwechsel jetzt sanft
- In schmalen Fenstern bricht die Filterzeile der Bibliothek in zwei Zeilen um, statt sich zu überlagern
- Panel-Schatten wurden abgeschwächt; die Hintergrundfarben sind ausgewogener

## 0.5.4

<!-- tr -->
- Yeni arayüz: çalan albümün kapağı bulanık bir arka plan olarak tüm pencereye yayılır, paneller yarı saydamdır
- Alttaki oynatıcı yüzen bir çubuğa dönüştü (waveform ilerleme çubuğu korundu)
- Sağ paneller (Motor, Sözler, Kuyruk, eklentiler) gölge ve ışıkla yükseltilmiş yüzey olarak çizilir
- Parça çalmıyorken hafif bir varsayılan arka plan gösterilir

<!-- en -->
- New look: the playing album's cover spreads across the window as a blurred backdrop, and panels are translucent
- The bottom player is now a floating bar (the waveform progress bar is kept)
- Right panels (Engine, Lyrics, Queue, plugins) are drawn as raised surfaces with shadow and light
- A subtle default backdrop is shown when nothing is playing

<!-- de -->
- Neues Design: Das Cover des laufenden Albums breitet sich als unscharfer Hintergrund über das Fenster aus, die Panels sind halbtransparent
- Der Player unten ist jetzt eine schwebende Leiste (die Waveform-Fortschrittsleiste bleibt erhalten)
- Rechte Panels (Engine, Songtexte, Warteschlange, Plugins) werden als erhabene Flächen mit Schatten und Licht dargestellt
- Wenn nichts läuft, wird ein dezenter Standardhintergrund angezeigt

## 0.5.3

<!-- tr -->
- Güncelleme artık her açılışta denetleniyor
- Güncelleme ekranında indirme ilerlemesi dalga formu şeklinde gösteriliyor

<!-- en -->
- Updates are now checked on every launch
- Download progress on the update screen is shown as a waveform

<!-- de -->
- Updates werden jetzt bei jedem Start geprüft
- Der Download-Fortschritt im Update-Bildschirm wird als Wellenform angezeigt

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
