use crate::{
    cli::Args,
    error::{AppError, Result},
    feedback::{COLORS, Preset, Rgb, TriggerEffect, TriggerSide, ZoneLevels},
    firmware,
    hid::{self, Controller, DualSenseHid},
    protocol::FirmwareInfo,
};
use console::{Key, Term, style};
use dialoguer::{Confirm, Input, Select, theme::ColorfulTheme};
use indicatif::{ProgressBar, ProgressStyle};
use std::{io::IsTerminal, str::FromStr, time::Duration};

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
    term.write_line(&format!(
        "  {}",
        style("A controller workbench for Mac developers.").dim()
    ))?;
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

// Feedback settings use individual key reads so Escape also cancels text prompts.
fn feedback_input<T: FromStr + std::fmt::Display>(
    term: &Term,
    prompt: &str,
    default: T,
    validate: impl Fn(&T) -> std::result::Result<(), String>,
) -> Result<Option<T>>
where
    T::Err: std::fmt::Display,
{
    let mut value = String::new();
    loop {
        term.clear_line()?;
        term.write_str(&format!("  {} [{default}]: {value}", style(prompt).cyan()))?;
        term.flush()?;
        match term.read_key_raw()? {
            Key::Escape | Key::CtrlC => {
                term.write_line("")?;
                return Ok(None);
            }
            Key::Enter => {
                let text = if value.trim().is_empty() {
                    default.to_string()
                } else {
                    value.clone()
                };
                match text.trim().parse::<T>() {
                    Ok(parsed) => match validate(&parsed) {
                        Ok(()) => {
                            term.clear_line()?;
                            term.write_line(&format!("  {prompt}: {parsed}"))?;
                            return Ok(Some(parsed));
                        }
                        Err(error) => term.write_line(&format!("\n  {}", style(error).yellow()))?,
                    },
                    Err(error) => term.write_line(&format!("\n  {}", style(error).yellow()))?,
                }
            }
            Key::Backspace => {
                value.pop();
            }
            Key::Char(c) if !c.is_control() && value.len() < 128 => value.push(c),
            _ => {}
        }
    }
}

fn feedback_number(term: &Term, prompt: &str, default: u8, min: u8, max: u8) -> Result<Option<u8>> {
    feedback_input(
        term,
        &format!("{prompt} ({min}–{max})"),
        default,
        |value| {
            if (min..=max).contains(value) {
                Ok(())
            } else {
                Err(format!("Choose {min}–{max}."))
            }
        },
    )
}

fn configure_feedback(term: &Term, action: &mut Args) -> Result<bool> {
    let theme = ColorfulTheme::default();
    term.write_line("  Tests are timed and reset automatically. Esc cancels any setting.")?;
    let Some(test) = Select::with_theme(&theme)
        .with_prompt("Feedback lab")
        .items(["Lightbar colors", "Adaptive triggers", "Gentle rumble"])
        .default(0)
        .interact_on_opt(term)
        .map_err(ui_error)?
    else {
        return Ok(false);
    };
    action.feedback = Some([Preset::Lightbar, Preset::Triggers, Preset::Rumble][test]);
    if test == 0 {
        let mut colors = COLORS
            .iter()
            .map(|(name, Rgb(r, g, b))| format!("{}  {name}", style("●").true_color(*r, *g, *b)))
            .collect::<Vec<_>>();
        colors.extend(["Custom RGB / hex".into(), "Rainbow cycle".into()]);
        let Some(color) = Select::with_theme(&theme)
            .with_prompt("Lightbar color")
            .items(&colors)
            .default(8)
            .interact_on_opt(term)
            .map_err(ui_error)?
        else {
            return Ok(false);
        };
        if color < COLORS.len() {
            action.color = Some(COLORS[color].1);
        } else if color == COLORS.len() {
            let Some(rgb) =
                feedback_input(term, "Color (#RRGGBB or R,G,B)", Rgb(30, 140, 255), |_| {
                    Ok(())
                })?
            else {
                return Ok(false);
            };
            action.color = Some(rgb);
        } else {
            action.cycle = true;
        }
        let Some(brightness) = feedback_number(term, "Brightness", 255, 0, 255)? else {
            return Ok(false);
        };
        action.brightness = Some(brightness);
    } else if test == 1 {
        let Some(mode) = Select::with_theme(&theme)
            .with_prompt("Adaptive trigger effect")
            .items([
                "Resistance · sustained force",
                "Weapon · break / release point",
                "Vibration · pulsing force",
                "Bow · draw and snap",
                "Galloping · alternating pulses",
                "Machine · changing vibration",
                "Off · free trigger baseline",
                "Advanced · resistance by zone",
                "Advanced · vibration by zone",
            ])
            .default(0)
            .interact_on_opt(term)
            .map_err(ui_error)?
        else {
            return Ok(false);
        };
        let effects = [
            TriggerEffect::Resistance,
            TriggerEffect::Weapon,
            TriggerEffect::Vibration,
            TriggerEffect::Bow,
            TriggerEffect::Galloping,
            TriggerEffect::Machine,
            TriggerEffect::Off,
            TriggerEffect::Resistance,
            TriggerEffect::Vibration,
        ];
        action.effect = Some(effects[mode]);
        let Some(side) = Select::with_theme(&theme)
            .with_prompt("Which trigger?")
            .items(["Both · L2 + R2", "Left · L2", "Right · R2"])
            .default(0)
            .interact_on_opt(term)
            .map_err(ui_error)?
        else {
            return Ok(false);
        };
        action.trigger = Some([TriggerSide::Both, TriggerSide::Left, TriggerSide::Right][side]);
        macro_rules! ask {
            ($field:ident, $prompt:expr, $default:expr, $min:expr, $max:expr) => {
                let Some(value) = feedback_number(term, $prompt, $default, $min, $max)? else {
                    return Ok(false);
                };
                action.$field = Some(value);
            };
        }
        if mode >= 7 {
            term.write_line("  Ten levels from released zone 0 to fully pulled zone 9. 0 disables a zone; 1–8 sets force.")?;
            let Some(zones) = feedback_input(
                term,
                "Zone levels (ten comma-separated values)",
                ZoneLevels([0, 0, 0, 0, 2, 2, 2, 2, 2, 2]),
                |_| Ok(()),
            )?
            else {
                return Ok(false);
            };
            action.zones = Some(zones);
        } else if mode != 6 {
            let (min_start, max_start, max_end) = match mode {
                1 => (2, 7, 8),
                3 => (1, 7, 8),
                4 => (0, 8, 9),
                5 => (1, 8, 9),
                _ => (0, 9, 9),
            };
            ask!(start, "Start zone", 4, min_start, max_start);
            if matches!(mode, 1 | 3 | 4 | 5) {
                let start = action.start.unwrap();
                ask!(end, "End zone", 7u8.max(start + 1), start + 1, max_end);
            }
            if mode != 4 {
                ask!(
                    strength,
                    "Strength",
                    2,
                    if mode == 5 { 0 } else { 1 },
                    if mode == 5 { 7 } else { 8 }
                );
            }
            if mode == 3 {
                ask!(snap_strength, "Snap strength", 2, 1, 8);
            }
            if mode == 4 {
                ask!(first_foot, "First pulse position", 1, 0, 6);
                let first = action.first_foot.unwrap();
                ask!(
                    second_foot,
                    "Second pulse position",
                    3u8.max(first + 1),
                    first + 1,
                    7
                );
            }
            if mode == 5 {
                ask!(strength_b, "Second strength", 4, 0, 7);
                ask!(period, "Strength change period", 20, 0, 255);
            }
        }
        if matches!(mode, 2 | 4 | 5 | 8) {
            ask!(
                frequency,
                "Frequency",
                if mode == 4 { 4 } else { 25 },
                1,
                255
            );
        }
    }
    let Some(duration) = feedback_input(term, "Duration in seconds (1–30)", 5u64, |v| {
        if (1..=30).contains(v) {
            Ok(())
        } else {
            Err("Choose 1–30 seconds.".into())
        }
    })?
    else {
        return Ok(false);
    };
    action.duration = duration;
    // Keep menu and command-line validation identical before opening the controller.
    crate::feedback::Effect::from_args(action)?;
    Ok(true)
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
            "Live input monitor",
            "Record a test session",
            "Feedback lab",
            "Developer diagnostics",
            "Firmware & controller details",
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
            5 => {
                selected = None;
                args.path.clear();
                refresh = true;
            }
            6 => {
                heading(&term)?;
                term.write_line("  A controller workbench for Mac game developers and testers.")?;
                term.write_line("  Firmware inspired by dualsense-updater-rs; terminal interaction inspired by Mole.")?;
                term.write_line("  Independent software. Not endorsed by Sony or PlayStation.")?;
                term.write_line("  https://github.com/olliejudge/dsmactools")?;
                pause(&term)?;
            }
            7 => break,
            3 => {
                heading(&term)?;
                if let Err(e) = crate::diagnostics::run() {
                    term.write_line(&format!("  {}", style(e).red()))?;
                }
                pause(&term)?;
            }
            _ => {
                let Some(s) = status.as_ref() else {
                    term.write_line("  No controller is ready. Connect it and refresh first.")?;
                    pause(&term)?;
                    continue;
                };
                let mut action = args.clone();
                action.interactive = false;
                action.yes = false;
                action.pid = s.controller.pid;
                action.path = s.controller.path.clone();
                heading(&term)?;
                match choice {
                    0 => action.monitor = true,
                    1 => {
                        let stamp = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_err(ui_error)?
                            .as_secs();
                        let path: String = Input::with_theme(&theme)
                            .with_prompt("Capture file")
                            .default(format!("dsmactools-capture-{stamp}.jsonl"))
                            .interact_text()
                            .map_err(ui_error)?;
                        action.duration = Input::with_theme(&theme)
                            .with_prompt("Duration in seconds (1–3600)")
                            .default(10u64)
                            .validate_with(|v: &u64| {
                                if (1..=3600).contains(v) {
                                    Ok(())
                                } else {
                                    Err("Choose 1–3600 seconds")
                                }
                            })
                            .interact_text()
                            .map_err(ui_error)?;
                        action.record = Some(path.into());
                    }
                    2 => {
                        if !configure_feedback(&term, &mut action)? {
                            continue;
                        }
                    }
                    4 => {
                        let Some(firmware_action) = Select::with_theme(&theme)
                            .with_prompt("Firmware & controller details")
                            .items([
                                "Check for updates",
                                "Controller details",
                                "Download latest firmware",
                                "Install latest firmware",
                            ])
                            .interact_on_opt(&term)
                            .map_err(ui_error)?
                        else {
                            continue;
                        };
                        action.check = firmware_action == 0;
                        action.print_firmware_info = firmware_action == 1;
                        action.download_latest = firmware_action == 2;
                        action.update_latest = firmware_action == 3;
                    }
                    _ => unreachable!(),
                }
                if let Err(e) = crate::run(action) {
                    term.write_line(&format!("\n  {}", style(e).red()))?;
                }
                pause(&term)?;
                if choice == 4 {
                    refresh = true;
                }
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
