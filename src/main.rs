mod cli;
mod diagnostics;
mod error;
mod feedback;
mod firmware;
mod hid;
mod input;
mod protocol;
mod ui;
mod update;

use crate::{
    cli::Args,
    error::{AppError, Result},
    hid::{DualSenseHid, find_first_device_path},
    protocol::FirmwareInfo,
};
use clap::Parser;
use std::io::IsTerminal;

fn main() {
    let args = Args::parse();
    env_logger::Builder::from_default_env()
        .filter_level(if args.verbose {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        })
        .format_timestamp(None)
        .init();
    let menu = args.interactive
        || (std::io::stdin().is_terminal()
            && std::io::stdout().is_terminal()
            && !args.has_action()
            && !args.yes);
    let result = if menu { ui::run(args) } else { run(args) };
    if let Err(err) = result {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

/// Describe the connected controller. An unsupported series is reported, not rejected, so
/// details stay available; update commands still validate the target themselves.
fn info_json(info: &FirmwareInfo, path: &str, pid: u16, target: Option<&str>) -> serde_json::Value {
    serde_json::json!({"path":path,"pid":pid,"target":target,
        "version":info.firmware_version,"version_hex":format!("0x{:04X}",info.firmware_version),
        "build_date":info.build_date,"build_time":info.build_time,
        "firmware_type":info.firmware_type,"image_type":info.image_type,"hardware_info":info.hardware_info,"software_series":info.software_series})
}

fn print_info(info: &FirmwareInfo, path: &str, pid: u16, json: bool) -> Result<()> {
    let target = firmware::target_for(info, pid).ok();
    if json {
        println!("{}", info_json(info, path, pid, target.as_deref()));
    } else {
        println!("Controller: {path} (USB, 054c:{pid:04x})");
        println!(
            "Firmware: 0x{:04X} | Sony target: {}",
            info.firmware_version,
            target
                .unwrap_or_else(|| format!("unsupported (series 0x{:04X})", info.software_series))
        );
        println!(
            "Built: {} {} | Hardware: 0x{:08X}",
            info.build_date, info.build_time, info.hardware_info
        );
    }
    Ok(())
}

fn run(args: Args) -> Result<()> {
    if args.check_catalogue {
        return firmware::check_catalogue();
    }
    if args.diagnostics {
        return diagnostics::run();
    }
    // Validate feedback settings before opening hardware or sending any output.
    let effect = args
        .feedback
        .map(|_| feedback::Effect::from_args(&args))
        .transpose()?;
    if args.vid != 0x054c || ![0x0ce6, 0x0df2].contains(&args.pid) {
        return Err(AppError::Validation(
            "Only Sony DualSense and DualSense Edge VID/PIDs are supported".into(),
        ));
    }
    if args.list {
        return hid::print_devices(args.vid, args.pid);
    }
    let path = if args.path.is_empty() {
        find_first_device_path(args.vid, args.pid)?
    } else {
        args.path
    };
    let dev = DualSenseHid::open(args.vid, args.pid, &path)?;
    if args.monitor {
        return input::monitor(&dev);
    }
    if let Some(file) = args.record {
        return input::record(&dev, &file, args.duration, args.pid);
    }
    if let Some(effect) = effect {
        return feedback::run(&dev, effect, args.duration);
    }
    let info = dev.get_firmware_info()?;
    print_info(&info, &path, args.pid, args.json)?;
    if args.print_firmware_info {
        return Ok(());
    }
    let target = firmware::target_for(&info, args.pid)?;
    let image = if let Some(local) = args.fw_image {
        println!("Checking local firmware image...");
        firmware::FirmwareImage::load(&local, &info, args.pid)?
    } else {
        println!("Checking Sony's firmware catalogue...");
        let latest = firmware::latest(&target)?;
        println!("Latest on Sony's server: 0x{latest:04X}");
        if latest <= info.firmware_version {
            println!("No newer firmware is available for this controller's series.");
            return Ok(());
        }
        if !args.download_latest && !args.update_latest {
            println!(
                "Update available. Run with --download-latest to prepare it or --update-latest to install it."
            );
            return Ok(());
        }
        println!("Downloading firmware from Sony...");
        let download_started = std::time::Instant::now();
        let (image, downloaded) =
            firmware::download(&target, latest, &info, args.pid, args.cache_dir.as_deref())?;
        println!(
            "Download validated: {} bytes in {:.1}s.",
            image.data().len(),
            download_started.elapsed().as_secs_f64()
        );
        println!("Downloaded: {}", downloaded.display());
        if args.download_latest {
            println!(
                "Validated {} bytes | SHA-256: {}",
                image.data().len(),
                image.sha256()
            );
            println!("Firmware saved only; nothing has been written to the controller.");
            return Ok(());
        }
        image
    };
    update::require_single_controller(&path)?;
    let battery = dev.preflight_battery()?;
    println!(
        "Update: 0x{:04X} -> 0x{:04X} | Battery: approximately {battery}%",
        info.firmware_version,
        image.version()
    );
    println!(
        "Validated {} bytes | SHA-256: {}",
        image.data().len(),
        image.sha256()
    );
    println!(
        "Keep the USB cable connected and the Mac awake. Writing firmware can commit before finalization."
    );
    println!("Writing to the controller can take several minutes, even for this small file.");
    if !args.yes && !prompt_yes_no("Install this firmware?")? {
        println!("Update cancelled before writing.");
        return Ok(());
    }
    let outcome = update::install(dev, &path, &image)?;
    println!("{}", outcome.message());
    Ok(())
}

fn prompt_yes_no(prompt: &str) -> Result<bool> {
    use std::io::{self, Write};
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        return ui::confirm_update();
    }
    print!("{prompt} [y/N] ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(matches!(
        input.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(series: u16) -> FirmwareInfo {
        let mut r = vec![0; 64];
        r[0] = 0x20;
        r[20] = 2;
        r[22..24].copy_from_slice(&series.to_le_bytes());
        r[44..46].copy_from_slice(&0x0630u16.to_le_bytes());
        FirmwareInfo::parse(r).unwrap()
    }

    #[test]
    fn info_json_keeps_its_keys_and_nulls_only_an_unsupported_target() {
        let supported = info_json(&info(0x000b), "p", 0x0ce6, Some("000B"));
        let unsupported = info_json(&info(0x9999), "p", 0x0ce6, None);
        assert_eq!(supported["target"], "000B");
        assert!(unsupported["target"].is_null());
        assert_eq!(unsupported["software_series"], 0x9999);
        assert_eq!(unsupported["version_hex"], "0x0630");
        let keys =
            |v: &serde_json::Value| v.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
        assert_eq!(keys(&supported), keys(&unsupported));
    }

    #[test]
    fn print_info_accepts_unsupported_series() {
        let info = info(0x9999);
        assert!(firmware::target_for(&info, 0x0ce6).is_err());
        print_info(&info, "p", 0x0ce6, false).unwrap();
        print_info(&info, "p", 0x0ce6, true).unwrap();
    }
}
