#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod analyze;
mod artist;
mod autoeq;
mod apo;
mod appicon;
mod biquad;
mod devenv;
mod dsd;
mod engine;
mod fileops;
mod fsutil;
mod fx;
mod gui;
mod i18n;
mod plugins;
mod onlinemeta;
mod tagwrite;
mod update;
mod library;
mod loudness;
mod lyrics;
mod tips;
mod ui;
mod viz;
mod wave;
mod web;
mod player;
mod proof;
mod media;
mod winmedia;
mod recover;
mod shellmenu;

use anyhow::{bail, Result};
use std::path::Path;

fn usage() -> ! {
    eprintln!(
        "Soundworks\n\
         \n  soundworks list                       çıkış cihazlarını listele\
         \n  soundworks check <preset.txt>         preseti çözümle ve göster\
         \n  soundworks run <preset.txt> [--from AD] [--to AD] [--shared]\
         \n\n  --from    loopback kaynağı (varsayılan: Windows varsayılan çıkışı)\
         \n  --to      EQ'lu sesin gideceği gerçek cihaz\
         \n  --shared  exclusive yerine shared mod (Dolby/Atmos devrede kalır)"
    );
    std::process::exit(2)
}

fn wait_for_exit(pid: u32) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            WaitForSingleObject(h, 20_000);
            let _ = CloseHandle(h);
        }
    }
}

fn main() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(i) = args.iter().position(|a| a == "--wait-pid") {
        let pid = args.get(i + 1).and_then(|v| v.parse::<u32>().ok());
        args.drain(i..(i + 2).min(args.len()));
        if let Some(pid) = pid {
            wait_for_exit(pid);
        }
    }
    if !args.is_empty() {

        unsafe {
            let _ = windows::Win32::System::Console::AttachConsole(u32::MAX);
        }
    }
    match args.first().map(String::as_str) {
        Some("list") => {
            engine::com_init()?;
            for d in engine::list_devices()? {
                println!("{} {} [{}]\n    {}", if d.is_default { "*" } else { " " }, d.name, d.kind.label(), d.id);
            }
        }
        Some("rates") => {
            engine::com_init()?;
            let d = engine::resolve(args.get(1).map(String::as_str))?;
            println!("{} [{}]: {:?}", d.name, d.kind.label(), engine::supported_rates(&d.id));
        }
        Some("check") => {
            let p = apo::load(Path::new(args.get(1).map(String::as_str).unwrap_or_else(|| usage())))?;
            println!("Preamp: {} dB, {} bant", p.preamp_db, p.bands.len());
            for (i, b) in p.bands.iter().enumerate() {
                println!("{:>2}: {:?} {} Hz {} dB Q {}", i + 1, b.kind, b.freq, b.gain_db, b.q);
            }
            for f in [100.0, 1000.0, 2400.0, 5000.0] {
                let db: f64 = p.preamp_db
                    + p.bands.iter().map(|b| biquad::Coeffs::design(b, 48000.0).magnitude_db(f, 48000.0)).sum::<f64>();
                println!("toplam yanıt @ {f} Hz: {db:.2} dB");
            }
        }
        Some("analyze") => {
            let f = args.get(1).map(String::as_str).unwrap_or_else(|| usage());
            let out = args.get(2).map(String::as_str).unwrap_or("spectrogram.png");
            analyze::run(Path::new(f), Path::new(out))?;
        }
        Some("default") => {
            engine::com_init()?;
            match args.get(1) {
                Some(id) => {
                    engine::set_default_render(id)?;
                    println!("varsayılan çıkış ayarlandı");
                }
                None => println!("{}", engine::default_render_id()?),
            }
        }
        Some("volumes") => engine::volumes()?,
        Some("mute") => {
            let q = args.get(1).map(String::as_str).unwrap_or_else(|| usage());
            engine::set_mute(q, args.get(2).map(String::as_str) != Some("0"))?;
        }
        Some("measure") => {
            let dev = args.get(1).map(String::as_str);
            println!("{:.2}", engine::measure(dev, 3)?);
        }
        Some("run") => {
            let file = args.get(1).map(String::as_str).unwrap_or_else(|| usage());
            let preset = apo::load(Path::new(file))?;
            let mut opts = engine::Options { from: None, to: None, shared: false, player: None, dop: false };
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--from" => {
                        i += 1;
                        opts.from = args.get(i).cloned();
                    }
                    "--to" => {
                        i += 1;
                        opts.to = args.get(i).cloned();
                    }
                    "--shared" => opts.shared = true,
                    o => bail!("bilinmeyen seçenek: {o}"),
                }
                i += 1;
            }
            engine::install_ctrlc();
            if let Err(e) = engine::run(engine::Control::new(preset), opts) {
                bail!("{}", engine::explain_error(&e));
            }
        }
        None => gui::run(false)?,
        Some("--tray") => gui::run(true)?,
        Some(flag @ ("--add" | "--spectrum")) => {
            let kind = if flag == "--add" { shellmenu::Kind::Add } else { shellmenu::Kind::Spectrum };
            let paths: Vec<std::path::PathBuf> = args[1..].iter().map(std::path::PathBuf::from).collect();
            if !paths.is_empty() {
                let _ = shellmenu::send(kind, &paths);
            }
            gui::run(false)?
        }
        _ => usage(),
    }
    Ok(())
}
