# Soundworks

**A lightweight, hi-res music player for Windows, built in Rust.** Soundworks brings your local music library, playback controls, and a flexible audio-processing engine together in one desktop app.

[Download the latest release](https://github.com/akasperanza/soundworks/releases/latest) · [Changelog](CHANGELOG.md) · [Extension guide](docs/EXTENSIONS.md)

## Features

- **Local music library** — scan folders, browse albums and artists, and manage tracks from the player.
- **Hi-res playback and audio processing** — build a configurable signal chain, apply EQ presets, and route audio to your chosen output device.
- **Library tools** — view and edit track metadata, find missing album artwork and metadata online, and inspect audio with waveform and spectrum analysis.
- **Lyrics and playback controls** — follow along with lyrics, manage the queue, and use Windows media keys and media controls.
- **Extensible with JavaScript** — add buttons, panels, commands, playback integrations, and more. See the [extension examples](examples) and [documentation](docs/EXTENSIONS.md).
- **English, Turkish, and German interface**.

## Getting started

1. Download `Soundworks-windows-x64.exe` from the [latest release](https://github.com/akasperanza/soundworks/releases/latest).
2. Run the application and add the folder containing your music to the library.
3. Browse your albums and start listening. Use **Settings** to configure your library and audio setup.

Soundworks is currently distributed as a Windows x64 application.

## Build from source

You need a Rust toolchain installed. From a Windows command prompt or PowerShell:

```powershell
git clone https://github.com/akasperanza/soundworks.git
cd soundworks
cargo run --release
```

To build without launching the app:

```powershell
cargo build --release
```

## Extensions

Extensions are small JavaScript programs that run in an isolated runtime inside Soundworks. They can respond to playback events and, with user-granted permissions, add interface elements, control playback, make network requests, or download audio.

- [Extension API and quick start](docs/EXTENSIONS.md)
- [Discord Rich Presence example](examples/discord-rich-presence)
- [Sleep Timer example](examples/sleep-timer)
- [Download from URL example](examples/download-from-url)

## License

Soundworks is licensed under the [GNU General Public License v3.0 or later](LICENSE).
