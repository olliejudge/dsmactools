use crate::{
    cli::Args,
    error::{AppError, Result},
    firmware,
    hid::{self, Controller, DualSenseHid},
    protocol::FirmwareInfo,
};
use console::{Term, style};
use dialoguer::{Confirm, Select, theme::ColorfulTheme};
use indicatif::{ProgressBar, ProgressStyle};
use std::{io::IsTerminal, time::Duration};

fn ui_error(e: impl std::fmt::Display) -> AppError {
    AppError::Validation(e.to_string())
}

struct Status {
    controller: Controller,
    info: FirmwareInfo,
    battery: Option<u8>,
    latest: std::result::Result<u16, String>,
}

fn loading(message: &str) -> ProgressBar {
    let bar = ProgressBar::new_spinner();
    bar.set_style(ProgressStyle::with_template("  {spinner:.cyan} {msg}").unwrap());
    bar.set_message(message.to_owned());
    bar.enable_steady_tick(Duration::from_millis(90));
    bar
}

fn read_status(controller: Controller) -> Result<Status> {
    let spinner = loading("Reading controller and checking Sony's firmware catalogue...");
    let result = (|| {
        let dev = DualSenseHid::open(0x054c, controller.pid, &controller.path)?;
        let info = dev.get_firmware_info()?;
        let battery = dev.battery().ok();
        let target = firmware::target_for(&info, controller.pid)?;
        let latest = firmware::latest(&target).map_err(|e| e.to_string());
        Ok(Status {
            controller,
            info,
            battery,
            latest,
        })
    })();
    spinner.finish_and_clear();
    result
}

fn select_controller(term: &Term, devices: &[Controller]) -> Result<Option<Controller>> {
    if devices.is_empty() {
        return Ok(None);
    }
    if devices.len() == 1 {
        return Ok(Some(devices[0].clone()));
    }
    let labels = devices
        .iter()
        .map(|d| format!("{}  ({})", d.name, d.path))
        .collect::<Vec<_>>();
    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Choose a USB controller")
        .items(&labels)
        .default(0)
        .interact_on_opt(term)
        .map_err(ui_error)?;
    Ok(selection.map(|index| devices[index].clone()))
}

fn heading(term: &Term) -> Result<()> {
    term.clear_screen()?;
    term.write_line("")?;
    term.write_line(&format!(
        "  {}  {}",
        style("DS MAC TOOLS").cyan().bold(),
        style(env!("CARGO_PKG_VERSION")).dim()
    ))?;
    term.write_line(&format!("  {}", style("Your DualSense, up to date.").dim()))?;
    term.write_line("")?;
    Ok(())
}

fn dashboard(term: &Term, status: Option<&Status>, error: Option<&str>) -> Result<()> {
    heading(term)?;
    if let Some(s) = status {
        term.write_line(&format!("  {}  {}", style("●").green(), s.controller.name))?;
        let battery = s
            .battery
            .map(|b| format!("{b}% battery"))
            .unwrap_or_else(|| "Battery unavailable".into());
        term.write_line(&format!("     USB connected  ·  {battery}"))?;
        match s.latest {
            Ok(v) if v > s.info.firmware_version => term.write_line(&format!(
                "     {}  {:04X} → {v:04X}",
                style("Update available").yellow(),
                s.info.firmware_version
            ))?,
            Ok(_) => term.write_line(&format!(
                "     {}  ·  Firmware {:04X}",
                style("Up to date").green(),
                s.info.firmware_version
            ))?,
            Err(_) => term.write_line(&format!(
                "     Firmware {:04X}  ·  {}",
                s.info.firmware_version,
                style("Could not check online").yellow()
            ))?,
        }
        if s.controller.pid == 0x0df2 {
            term.write_line(&format!(
                "     {}",
                style("DualSense Edge firmware installation is not hardware-tested.").yellow()
            ))?;
        }
    } else {
        term.write_line(&format!("  {}  No controller ready", style("○").yellow()))?;
        term.write_line("     Connect a DualSense with a USB data cable, then refresh.")?;
    }
    if let Some(error) = error {
        term.write_line(&format!(
            "
  {}",
            style(error).yellow()
        ))?;
    }
    term.write_line("")?;
    Ok(())
}

fn pause(term: &Term) -> Result<()> {
    term.write_line(&format!(
        "
  {}",
        style("Press Enter to return to the menu.").dim()
    ))?;
    term.read_line()?;
    Ok(())
}

pub fn confirm_update() -> Result<bool> {
    Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Install this firmware?")
        .default(false)
        .interact()
        .map_err(ui_error)
}

pub fn run(mut args: Args) -> Result<()> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(ui_error(
            "The interactive menu needs a terminal. Use --check or --print-firmware-info for scripts.",
        ));
    }
    let term = Term::stdout();
    let theme = ColorfulTheme::default();
    let mut selected = None;
    let mut status = None;
    let mut error = None;
    let mut refresh = true;
    loop {
        if refresh {
            refresh = false;
            heading(&term)?;
            match hid::controllers() {
                Ok(devices) => {
                    // Refresh the ephemeral HID path after a controller reboot.
                    selected = if !args.path.is_empty() {
                        devices
                            .iter()
                            .find(|d| d.path == args.path && d.pid == args.pid)
                            .cloned()
                    } else if let Some(old) = selected.as_ref() {
                        let old: &Controller = old;
                        devices
                            .iter()
                            .find(|d| d.path == old.path)
                            .cloned()
                            .or_else(|| {
                                if devices.len() == 1 {
                                    Some(devices[0].clone())
                                } else {
                                    None
                                }
                            })
                    } else {
                        select_controller(&term, &devices)?
                    };
                    match selected.clone().map(read_status).transpose() {
                        Ok(value) => {
                            status = value;
                            error = None;
                        }
                        Err(e) => {
                            status = None;
                            error = Some(e.to_string());
                        }
                    }
                }
                Err(e) => {
                    status = None;
                    error = Some(e.to_string());
                }
            }
        }
        dashboard(&term, status.as_ref(), error.as_deref())?;
        let items = [
            "Check for updates",
            "Install latest firmware",
            "Controller details",
            "Download firmware",
            "Refresh / choose controller",
            "About DS Mac Tools",
            "Quit",
        ];
        term.write_line(&format!(
            "  {}",
            style("↑/↓ to move  ·  Enter to select  ·  Esc to quit").dim()
        ))?;
        let Some(choice) = Select::with_theme(&theme)
            .items(items)
            .default(0)
            .interact_on_opt(&term)
            .map_err(ui_error)?
        else {
            break;
        };
        match choice {
            0 => {
                refresh = true;
            }
            4 => {
                selected = None;
                args.path.clear();
                refresh = true;
            }
            5 => {
                heading(&term)?;
                term.write_line(
                    "  A macOS toolkit for DualSense firmware, inspired by dualsense-updater-rs.",
                )?;
                term.write_line("  Terminal interaction inspired by Mole.")?;
                term.write_line(
                    "  Independent, unofficial software. Not endorsed by Sony or PlayStation.",
                )?;
                term.write_line("  https://github.com/olliejudge/dsmactools")?;
                pause(&term)?;
            }
            6 => break,
            _ => {
                let Some(s) = status.as_ref() else {
                    term.write_line(
                        "
  No controller is ready. Connect it and refresh first.",
                    )?;
                    pause(&term)?;
                    continue;
                };
                // UI actions always use the validated shared CLI path and its preflight.
                let mut action = args.clone();
                action.interactive = false;
                action.yes = false;
                action.pid = s.controller.pid;
                action.path = s.controller.path.clone();
                action.print_firmware_info = choice == 2;
                action.update_latest = choice == 1;
                action.download_latest = choice == 3;
                heading(&term)?;
                if let Err(e) = crate::run(action) {
                    term.write_line(&format!(
                        "
  {}",
                        style(e).red()
                    ))?;
                }
                pause(&term)?;
                refresh = true;
            }
        }
    }
    term.write_line(
        "
  See you next time.
",
    )?;
    Ok(())
}
