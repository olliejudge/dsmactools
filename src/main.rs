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
    cli::{Args, DEFAULT_PID},
    error::{AppError, Result},
    hid::{Controller, DualSenseHid, find_first_device_path},
    protocol::FirmwareInfo,
};
use clap::Parser;
use console::style;
use std::io::{IsTerminal, Write};

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
        // A closed terminal makes `eprintln!` panic; the exit status still reports the failure.
        let _ = writeln!(std::io::stderr(), "{err}");
        std::process::exit(1);
    }
}

/// Report an install, then honour any stop request that was held back while it ran. The result
/// is printed first and the process then exits with the signal's conventional status, instead of
/// returning to the menu. Nothing here retries or reflashes.
fn finish_install(installation: update::Installation) -> Result<()> {
    let update::Installation {
        result,
        stop_requested,
    } = installation;
    let Some(code) = stop_exit_code(stop_requested) else {
        return result.map(|outcome| {
            let _ = writeln!(std::io::stdout(), "{}", outcome.message());
        });
    };
    match result {
        Ok(outcome) => {
            let _ = writeln!(std::io::stdout(), "{}", outcome.message());
        }
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "{error}");
        }
    }
    let _ = writeln!(std::io::stderr(), "Stopped as requested during the update.");
    std::process::exit(code)
}

/// The exit status for a stop request held during an install: 128 + the signal number. `None`
/// means carry on normally.
fn stop_exit_code(stop: Option<input::StopSignal>) -> Option<i32> {
    stop.map(input::StopSignal::exit_code)
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

/// Pick the product ID for command-line use. An explicit `--pid` always wins. Otherwise use the
/// controller named by `--path`, or the product ID shared by every connected USB controller (so
/// a lone DualSense Edge works without `--pid`). Mixed models, or none, keep the DualSense default
/// and the usual not-found or multiple-controller errors.
fn resolve_pid(explicit: Option<u16>, path: &str, connected: &[Controller]) -> u16 {
    if let Some(pid) = explicit {
        return pid;
    }
    if !path.is_empty()
        && let Some(controller) = connected.iter().find(|c| c.path == path)
    {
        return controller.pid;
    }
    match connected {
        [first, rest @ ..] if rest.iter().all(|c| c.pid == first.pid) => first.pid,
        _ => DEFAULT_PID,
    }
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
    let connected = if args.pid.is_none() {
        hid::controllers().unwrap_or_default()
    } else {
        Vec::new()
    };
    let pid = resolve_pid(args.pid, &args.path, &connected);
    if args.vid != 0x054c || ![0x0ce6, 0x0df2].contains(&pid) {
        return Err(AppError::Validation(
            "Only Sony DualSense and DualSense Edge VID/PIDs are supported".into(),
        ));
    }
    if args.list {
        return hid::print_devices(args.vid, pid);
    }
    let path = if args.path.is_empty() {
        find_first_device_path(args.vid, pid)?
    } else {
        args.path
    };
    let dev = DualSenseHid::open(args.vid, pid, &path)?;
    if args.monitor {
        return input::monitor(&dev);
    }
    if let Some(file) = args.record {
        return input::record(&dev, &file, args.duration, pid);
    }
    if let Some(effect) = effect {
        return feedback::run(&dev, effect, args.duration);
    }
    let info = dev.get_firmware_info()?;
    print_info(&info, &path, pid, args.json)?;
    if args.print_firmware_info {
        return Ok(());
    }
    let target = firmware::target_for(&info, pid)?;
    let image = if let Some(local) = args.fw_image {
        println!("Checking local firmware image...");
        firmware::FirmwareImage::load(&local, &info, pid)?
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
        println!("Preparing firmware...");
        let download_started = std::time::Instant::now();
        let (image, downloaded, source) =
            firmware::download(&target, latest, &info, pid, args.cache_dir.as_deref())?;
        match source {
            firmware::ImageSource::Downloaded => println!(
                "Download validated: {} bytes in {:.1}s.",
                image.data().len(),
                download_started.elapsed().as_secs_f64()
            ),
            firmware::ImageSource::Cached => println!(
                "Using the copy you already downloaded: {} bytes, checksum and compatibility verified.",
                image.data().len()
            ),
        }
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
    if pid == 0x0df2 {
        println!("{}", style(ui::EDGE_UNTESTED_WARNING).yellow());
    }
    println!(
        "Keep the USB cable connected and the Mac awake. Writing firmware can commit before finalization."
    );
    println!("Writing to the controller can take several minutes, even for this small file.");
    if !args.yes && !prompt_yes_no("Install this firmware?")? {
        println!("Update cancelled before writing.");
        return Ok(());
    }
    finish_install(update::install(dev, &path, &image))
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
    use crate::input::StopSignal;

    #[test]
    fn a_held_stop_request_exits_with_the_conventional_signal_status() {
        assert_eq!(stop_exit_code(Some(StopSignal::Interrupt)), Some(130));
        assert_eq!(stop_exit_code(Some(StopSignal::Terminate)), Some(143));
        assert_eq!(stop_exit_code(Some(StopSignal::Hangup)), Some(129));
    }

    #[test]
    fn no_stop_request_continues_normally() {
        assert_eq!(stop_exit_code(None), None);
    }

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

    fn usb(path: &str, pid: u16) -> Controller {
        Controller {
            path: path.into(),
            pid,
            name: "DualSense".into(),
        }
    }

    #[test]
    fn pid_is_detected_only_when_not_given() {
        let edge = [usb("a", 0x0df2)];
        assert_eq!(resolve_pid(None, "", &edge), 0x0df2);
        assert_eq!(resolve_pid(None, "", &[usb("a", 0x0ce6)]), 0x0ce6);
        // An explicit --pid is never overridden by what is connected.
        assert_eq!(resolve_pid(Some(0x0ce6), "", &edge), 0x0ce6);
        assert_eq!(resolve_pid(Some(0x0df2), "", &[]), 0x0df2);
        // Nothing connected keeps the default, so the usual not-found error follows.
        assert_eq!(resolve_pid(None, "", &[]), DEFAULT_PID);
    }

    #[test]
    fn mixed_controllers_keep_the_default_and_a_path_selects_its_own_model() {
        let mixed = [usb("a", 0x0ce6), usb("b", 0x0df2)];
        assert_eq!(resolve_pid(None, "", &mixed), DEFAULT_PID);
        assert_eq!(resolve_pid(None, "b", &mixed), 0x0df2);
        assert_eq!(resolve_pid(None, "missing", &mixed), DEFAULT_PID);
        // Two of the same model still report "multiple controllers" for that model.
        let two_edges = [usb("a", 0x0df2), usb("b", 0x0df2)];
        assert_eq!(resolve_pid(None, "", &two_edges), 0x0df2);
    }

    #[test]
    fn print_info_accepts_unsupported_series() {
        let info = info(0x9999);
        assert!(firmware::target_for(&info, 0x0ce6).is_err());
        print_info(&info, "p", 0x0ce6, false).unwrap();
        print_info(&info, "p", 0x0ce6, true).unwrap();
    }
}
