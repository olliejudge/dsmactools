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

fn print_info(info: &FirmwareInfo, path: &str, pid: u16, json: bool) -> Result<()> {
    let target = firmware::target_for(info, pid)?;
    if json {
        println!(
            "{}",
            serde_json::json!({"path":path,"pid":pid,"target":target,
            "version":info.firmware_version,"version_hex":format!("0x{:04X}",info.firmware_version),
            "build_date":info.build_date,"build_time":info.build_time,
            "firmware_type":info.firmware_type,"image_type":info.image_type,"hardware_info":info.hardware_info,"software_series":info.software_series})
        );
    } else {
        println!("Controller: {path} (USB, 054c:{pid:04x})");
        println!(
            "Firmware: 0x{:04X} | Sony target: {target}",
            info.firmware_version
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
        firmware::FirmwareImage::load(&local, &info, args.pid)?
    } else {
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
        let (image, downloaded) =
            firmware::download(&target, latest, &info, args.pid, args.cache_dir.as_deref())?;
        println!("Downloaded: {}", downloaded.display());
        if args.download_latest {
            println!(
                "Validated {} bytes | SHA-256: {}",
                image.data.len(),
                image.sha256
            );
            return Ok(());
        }
        image
    };
    let battery = dev.preflight_battery()?;
    println!(
        "Update: 0x{:04X} -> 0x{:04X} | Battery: approximately {battery}%",
        info.firmware_version, image.version
    );
    println!(
        "Validated {} bytes | SHA-256: {}",
        image.data.len(),
        image.sha256
    );
    println!(
        "Keep the USB cable connected and the Mac awake. Writing firmware can commit before finalization."
    );
    if !args.yes && !prompt_yes_no("Install this firmware?")? {
        println!("Update cancelled before writing.");
        return Ok(());
    }
    let updater = update::DualSenseUpdater::new(dev);
    let outcome = updater.flash(&image.data);
    drop(updater);
    // The old macOS HID path may change when the controller re-enumerates.
    let verified = verify_installed(args.vid, args.pid, &info, image.version);
    match (outcome, verified) {
        (Ok(()), Ok(current)) => println!(
            "Verified installed firmware: 0x{:04X} ({} {}).",
            current.firmware_version, current.build_date, current.build_time
        ),
        (Err(phase_error), Ok(current)) => println!(
            "Verified installed firmware: 0x{:04X}. Controller rebooted/committed despite a protocol error: {phase_error}",
            current.firmware_version
        ),
        (Err(phase_error), Err(_)) => {
            return Err(AppError::Validation(format!(
                "{phase_error}. Update not verified; reconnect and run --print-firmware-info before any retry"
            )));
        }
        (Ok(()), Err(e)) => return Err(e),
    }
    Ok(())
}

fn verify_installed(
    vid: u16,
    pid: u16,
    before: &FirmwareInfo,
    expected: u16,
) -> Result<FirmwareInfo> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if let Ok(path) = find_first_device_path(vid, pid)
            && let Ok(dev) = DualSenseHid::open(vid, pid, &path)
            && let Ok(info) = dev.get_firmware_info()
            && info.software_series == before.software_series
            && info.hardware_info == before.hardware_info
            && info.firmware_version == expected
        {
            return Ok(info);
        }
        if std::time::Instant::now() >= deadline {
            return Err(AppError::Validation(format!(
                "Transfer finished but installed firmware 0x{expected:04X} could not be verified. Reconnect and run --print-firmware-info; do not assume the update succeeded"
            )));
        }
    }
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
