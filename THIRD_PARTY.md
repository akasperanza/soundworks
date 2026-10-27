# Üçüncü taraf bileşenler

Soundworks, [GPL-3.0-or-later](LICENSE) ile lisanslıdır. Aşağıdaki bileşenler kendi lisanslarıyla birlikte dağıtılır.

## Uygulamaya gömülü varlıklar

| Bileşen | Kullanım | Lisans |
|---|---|---|
| [Barlow](https://github.com/jpt/barlow) SemiBold, © 2017 The Barlow Project Authors | Başlıktaki "Soundworks" yazısı, uygulama simgesindeki harfler | SIL Open Font License 1.1: [assets/fonts/OFL.txt](assets/fonts/OFL.txt) |
| [Lucide](https://lucide.dev) simgeleri, © Lucide Icons and Contributors (bir kısmı © Cole Bemis, Feather) | Arayüz simgeleri (`assets/icons`) | ISC ve MIT: [assets/icons/LICENSE](assets/icons/LICENSE) |

## Çalışma sırasında kullanılan veri kaynakları

Aşağıdaki veriler Soundworks ile birlikte dağıtılmaz, kullanıcı istediğinde internetten alınır:

- [AutoEq](https://github.com/jaakkopasanen/AutoEq) kulaklık profilleri (MIT).
- MusicBrainz, Deezer, Last.fm ve Wikipedia: albüm türü, sanatçı fotoğrafı ve özeti.
- LRCLib, Apple Music ve BiniLyrics: şarkı sözleri.
- Windows ile gelen Segoe UI yazı tipi uygulamaya gömülü değildir, sistemden okunur.

## Rust kütüphaneleri

Uygulama aşağıdaki Rust kütüphanelerini kullanır. Her birinin lisans metni ve telif bildirimi kendi kaynak deposunda ve [crates.io](https://crates.io) sayfasındadır. Tam sürüm listesi için `Cargo.lock` dosyasına bakın.

### MIT OR Apache-2.0 (125)

accesskit 0.16.3, accesskit_consumer 0.24.3, accesskit_windows 0.23.2, ahash 0.8.12, anyhow 1.0.104, arboard 3.7.0, arrayvec 0.7.8, ash 0.38.0+1.3.281, base64 0.22.1, bitflags 2.13.2, cc 1.8.0, cfg-if 1.0.5, crc32fast 1.5.2, crossbeam-channel 0.5.17, crossbeam-utils 0.8.23, data-url 0.3.2, displaydoc 0.2.7, document-features 0.2.12, ecolor 0.29.1, eframe 0.29.1, egui 0.29.1, egui-wgpu 0.29.1, egui-winit 0.29.1, egui_glow 0.29.1, emath 0.29.1, epaint 0.29.1, euclid 0.22.14, fdeflate 0.3.7, find-msvc-tools 0.1.14, flate2 1.1.10, form_urlencoded 1.2.2, getrandom 0.2.17, getrandom 0.3.4, getrandom 0.4.3, gpu-alloc 0.6.2, gpu-alloc-types 0.3.1, gpu-allocator 0.26.0, gpu-descriptor 0.3.2, gpu-descriptor-types 0.2.0, hashbrown 0.15.5, hashbrown 0.17.1, idna 1.1.0, image 0.25.10, itoa 1.0.18, jobserver 0.1.35, keyboard-types 0.7.0, lazy_static 1.5.1, libc 0.2.190, litrs 1.0.1, lock_api 0.4.14, log 0.4.34, naga 22.1.0, num-complex 0.4.6, num-integer 0.1.47, num-traits 0.2.19, once_cell 1.21.4, parking_lot 0.12.5, parking_lot_core 0.9.12, paste 1.0.15, percent-encoding 2.3.2, pkg-config 0.3.34, png 0.17.16, png 0.18.1, presser 0.3.1, primal-check 0.3.4, proc-macro2 1.0.107, profiling 1.0.18, quote 1.0.47, regex 1.13.1, regex-automata 0.4.18, regex-syntax 0.8.11, renderdoc-sys 1.1.0, roxmltree 0.20.0, rustfft 6.4.1, rustls-pki-types 1.15.1, rustversion 1.0.23, scopeguard 1.2.0, serde 1.0.229, serde_core 1.0.229, serde_derive 1.0.229, serde_json 1.0.154, shlex 2.0.1, simdutf8 0.1.5, siphasher 1.0.4, smallvec 1.16.3, smol_str 0.2.2, stable_deref_trait 1.2.1, static_assertions 1.1.0, strength_reduce 0.2.4, syn 1.0.109, syn 2.0.119, syn 3.0.9, thiserror 1.0.69, thiserror 2.0.21, thiserror-impl 1.0.69, thiserror-impl 2.0.21, transpose 0.2.3, tray-icon 0.19.3, ttf-parser 0.25.1, unicode-segmentation 1.13.3, unicode-width 0.1.14, unicode-xid 0.2.6, ureq 2.12.1, url 2.5.8, web-time 1.1.0, webbrowser 1.2.4, wgpu 22.1.0, wgpu-core 22.1.0, wgpu-hal 22.0.0, wgpu-types 22.0.0, widestring 1.2.1, windows 0.52.0, windows 0.58.0, windows-core 0.52.0, windows-core 0.58.0, windows-implement 0.58.0, windows-interface 0.58.0, windows-link 0.2.1, windows-result 0.2.0, windows-strings 0.1.0, windows-sys 0.52.0, windows-sys 0.59.0, windows-sys 0.61.2, windows-targets 0.52.6, windows_x86_64_msvc 0.52.6

### MIT (29)

cfg_aliases 0.1.1, cfg_aliases 0.2.2, com 0.6.0, com_macros 0.6.0, com_macros_support 0.6.0, extended 0.1.0, float-cmp 0.9.0, glutin-winit 0.5.0, hassle-rs 0.11.0, imagesize 0.13.0, memoffset 0.9.1, pico-args 0.5.0, realfft 3.5.0, rfd 0.15.4, rgb 0.8.53, rquickjs 0.9.0, rquickjs-core 0.9.0, rquickjs-sys 0.9.0, rubato 0.15.0, simd-adler32 0.3.10, strict-num 0.1.1, synstructure 0.14.0, tracing 0.1.44, tracing-attributes 0.1.31, tracing-core 0.1.36, winres 0.1.12, xml-rs 0.8.29, xmlwriter 0.1.0, zmij 1.0.23

### MPL-2.0 (18)

resvg 0.44.0, symphonia 0.5.5, symphonia-bundle-flac 0.5.5, symphonia-bundle-mp3 0.5.5, symphonia-codec-aac 0.5.5, symphonia-codec-adpcm 0.5.5, symphonia-codec-alac 0.5.5, symphonia-codec-pcm 0.5.5, symphonia-codec-vorbis 0.5.5, symphonia-core 0.5.5, symphonia-format-caf 0.5.5, symphonia-format-isomp4 0.5.5, symphonia-format-mkv 0.5.5, symphonia-format-ogg 0.5.5, symphonia-format-riff 0.5.5, symphonia-metadata 0.5.5, symphonia-utils-xiph 0.5.5, usvg 0.44.0

### Unicode-3.0 (18)

icu_collections 2.3.0, icu_locale_core 2.3.0, icu_normalizer 2.3.0, icu_normalizer_data 2.3.0, icu_properties 2.3.0, icu_properties_data 2.3.0, icu_provider 2.3.1, litemap 0.8.3, potential_utf 0.1.6, tinystr 0.8.4, writeable 0.6.4, yoke 0.8.3, yoke-derive 0.8.4, zerofrom 0.1.8, zerofrom-derive 0.1.8, zerotrie 0.2.5, zerovec 0.11.8, zerovec-derive 0.11.6

### Apache-2.0 OR MIT (15)

autocfg 1.5.1, equivalent 1.0.3, idna_adapter 1.2.2, immutable-chunkmap 2.1.4, indexmap 2.14.2, kurbo 0.11.3, muda 0.15.3, multiversion_no_op 1.0.0, nohash-hasher 0.2.0, pin-project-lite 0.2.17, rustc-hash 2.1.3, simplecss 0.2.2, svgtypes 0.15.3, utf8_iter 1.0.4, zeroize 1.9.1

### Apache-2.0 (12)

ab_glyph 0.2.32, ab_glyph_rasterizer 0.1.10, accesskit_winit 0.22.4, codespan-reporting 0.11.1, gl_generator 0.14.0, glutin 0.32.3, glutin_egl_sys 0.7.1, glutin_wgl_sys 0.6.1, khronos_api 3.1.0, owned_ttf_parser 0.25.1, spirv 0.3.0+sdk-1.3.268.0, winit 0.30.13

### MIT/Apache-2.0 (9)

bit-set 0.6.0, bit-vec 0.7.0, bitflags 1.3.2, core_detect 1.0.0, khronos-egl 6.0.0, toml 0.5.11, type-map 0.5.1, version_check 0.9.5, winapi 0.3.9

### MIT OR Apache-2.0 OR Zlib (6)

cursor-icon 1.2.0, glow 0.13.1, glow 0.14.2, raw-window-handle 0.6.2, zune-core 0.5.3, zune-jpeg 0.5.15

### Unlicense OR MIT (5)

aho-corasick 1.1.5, byteorder-lite 0.1.0, memchr 2.8.3, termcolor 1.4.1, winapi-util 0.1.11

### BSD-3-Clause (3)

subtle 2.6.1, tiny-skia 0.11.4, tiny-skia-path 0.11.4

### ISC (3)

libloading 0.8.9, rustls-webpki 0.103.15, untrusted 0.9.0

### BSD-3-Clause OR Apache-2.0 (2)

moxcms 0.8.1, pxfm 0.1.30

### BSL-1.0 (2)

clipboard-win 5.4.1, error-code 3.4.0

### CDLA-Permissive-2.0 (2)

webpki-roots 0.26.11, webpki-roots 1.0.9

### MIT OR Zlib OR Apache-2.0 (2)

miniz_oxide 0.8.9, miniz_oxide 0.9.1

### Zlib (2)

foldhash 0.1.5, zlib-rs 0.6.8

### Zlib OR Apache-2.0 OR MIT (2)

bytemuck 1.25.2, bytemuck_derive 1.12.1

### (Apache-2.0 OR MIT) AND BSD-3-Clause (1)

encoding_rs 0.8.42

### (MIT OR Apache-2.0) AND OFL-1.1 AND LicenseRef-UFL-1.0 (1)

epaint_default_fonts 0.29.1

### (MIT OR Apache-2.0) AND Unicode-3.0 (1)

unicode-ident 1.0.27

### 0BSD OR MIT OR Apache-2.0 (1)

adler2 2.0.1

### Apache-2.0 AND ISC (1)

ring 0.17.14

### Apache-2.0 AND MIT (1)

dpi 0.1.2

### Apache-2.0 OR ISC OR MIT (1)

rustls 0.23.46

### Apache-2.0/MIT (1)

rustc-hash 1.1.0

### BSD-2-Clause (1)

arrayref 0.3.9

### BSD-2-Clause OR Apache-2.0 OR MIT (1)

zerocopy 0.8.63

### CC0-1.0 (1)

hexf-parse 0.2.1
