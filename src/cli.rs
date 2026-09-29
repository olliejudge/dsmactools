use clap::{ArgGroup, Parser};

pub const DEFAULT_VID: u16 = 0x054c;
pub const DEFAULT_PID: u16 = 0x0ce6;

#[derive(Parser, Debug, Clone)]
#[command(name = "dsmactools", version,
    about = "DS Mac Tools: a controller workbench for Mac game developers.",
    group(ArgGroup::new("update_action").args(["fw_image", "update_latest"])),
    group(ArgGroup::new("timed_action").args(["record", "feedback"])),
    group(ArgGroup::new("action").args(["print_firmware_info", "check", "download_latest", "update_latest", "fw_image", "list", "check_catalogue", "monitor", "record", "feedback", "diagnostics"]).multiple(false)))]
pub struct Args {
    #[arg(long, value_parser = parse_u16, default_value_t = DEFAULT_VID)]
    pub vid: u16,
    #[arg(long, value_parser = parse_u16, default_value_t = DEFAULT_PID)]
    pub pid: u16,
    #[arg(
        value_name = "FW_IMAGE",
        help = "Flash a local image after compatibility checks."
    )]
    pub fw_image: Option<std::path::PathBuf>,
    #[arg(long, help = "Read controller information without writing.")]
    pub print_firmware_info: bool,
    #[arg(
        long,
        help = "Compare the controller with Sony's live firmware catalogue (read-only)."
    )]
    pub check: bool,
    #[arg(
        long,
        help = "Check Sony's full catalogue against the release baseline; no controller needed."
    )]
    pub check_catalogue: bool,
    #[arg(
        long,
        help = "Download the latest compatible official firmware without flashing."
    )]
    pub download_latest: bool,
    #[arg(
        long,
        help = "Download and flash the latest compatible official firmware."
    )]
    pub update_latest: bool,
    #[arg(
        long,
        help = "List attached Sony controller HID interfaces without opening them."
    )]
    pub list: bool,
    #[arg(
        long,
        help = "Output controller information as JSON; read-only.",
        requires = "print_firmware_info",
        conflicts_with_all = ["check", "download_latest", "update_latest", "fw_image", "list", "check_catalogue", "monitor", "record", "feedback", "diagnostics"]
    )]
    pub json: bool,
    #[arg(
        long,
        help = "Confirm a firmware update without an interactive prompt.",
        requires = "update_action"
    )]
    pub yes: bool,
    #[arg(
        long,
        default_value = "",
        help = "Exact HID path; must match the selected VID/PID."
    )]
    pub path: String,
    #[arg(short = 'v', long, help = "Enable protocol debugging.")]
    pub verbose: bool,
    #[arg(long, help = "Live USB input viewer (terminal required).")]
    pub monitor: bool,
    #[arg(
        long,
        value_name = "FILE",
        help = "Record raw and decoded USB inputs as JSONL; never overwrites a file."
    )]
    pub record: Option<std::path::PathBuf>,
    #[arg(
        long,
        value_enum,
        help = "Run a timed rumble, lightbar or adaptive trigger test."
    )]
    pub feedback: Option<crate::feedback::Preset>,
    #[arg(
        long,
        requires = "feedback",
        conflicts_with = "cycle",
        help = "Lightbar color: name, #RRGGBB or R,G,B (0–255)."
    )]
    pub color: Option<crate::feedback::Rgb>,
    #[arg(
        long,
        requires = "feedback",
        help = "Lightbar brightness, 0–255 (default: 255)."
    )]
    pub brightness: Option<u8>,
    #[arg(
        long,
        requires = "feedback",
        help = "Cycle the lightbar smoothly through the rainbow."
    )]
    pub cycle: bool,
    #[arg(
        long,
        value_enum,
        requires = "feedback",
        help = "Trigger side: left (L2), right (R2), or both (default)."
    )]
    pub trigger: Option<crate::feedback::TriggerSide>,
    #[arg(
        long,
        value_enum,
        requires = "feedback",
        help = "Trigger effect (default: resistance)."
    )]
    pub effect: Option<crate::feedback::TriggerEffect>,
    #[arg(
        long,
        requires = "feedback",
        help = "Trigger start zone (default: 4); valid range depends on effect."
    )]
    pub start: Option<u8>,
    #[arg(
        long,
        requires = "feedback",
        help = "Weapon/bow/galloping/machine end zone, greater than start (default: 7)."
    )]
    pub end: Option<u8>,
    #[arg(
        long,
        requires = "feedback",
        help = "Trigger strength/amplitude, 1–8; machine uses 0–7 (default: 2)."
    )]
    pub strength: Option<u8>,
    #[arg(
        long,
        requires = "feedback",
        help = "Effect frequency setting, 1–255 (default: galloping 4; vibration/machine 25)."
    )]
    pub frequency: Option<u8>,
    #[arg(
        long,
        requires = "feedback",
        help = "Bow snap strength, 1–8 (default: 2)."
    )]
    pub snap_strength: Option<u8>,
    #[arg(
        long,
        requires = "feedback",
        help = "Galloping first foot, 0–6 (default: 1)."
    )]
    pub first_foot: Option<u8>,
    #[arg(
        long,
        requires = "feedback",
        help = "Galloping second foot, greater than first and at most 7 (default: 3)."
    )]
    pub second_foot: Option<u8>,
    #[arg(
        long,
        requires = "feedback",
        help = "Machine second strength, 0–7 (default: 4)."
    )]
    pub strength_b: Option<u8>,
    #[arg(
        long,
        requires = "feedback",
        help = "Machine strength-change period, 0–255 controller units (default: 20)."
    )]
    pub period: Option<u8>,
    #[arg(long, requires = "feedback", conflicts_with_all = ["start", "strength"], value_name = "L0,L1,...,L9", help = "Resistance/vibration profile: ten comma-separated levels 0–8; 0 disables a zone.")]
    pub zones: Option<crate::feedback::ZoneLevels>,
    #[arg(
        long,
        default_value_t = 10,
        help = "Capture duration (1–3600s) or feedback duration (1–30s).",
        requires = "timed_action"
    )]
    pub duration: u64,
    #[arg(
        long,
        help = "Print Mac toolchain and native GameController diagnostics as JSON."
    )]
    pub diagnostics: bool,
    #[arg(long, help = "Open the interactive menu (the default in a terminal).", conflicts_with_all = ["print_firmware_info", "check", "download_latest", "update_latest", "fw_image", "list", "json", "yes", "check_catalogue", "monitor", "record", "feedback", "diagnostics"])]
    pub interactive: bool,
    #[arg(
        long,
        help = "Firmware download directory (default: the macOS user cache)."
    )]
    pub cache_dir: Option<std::path::PathBuf>,
}

impl Args {
    pub fn has_action(&self) -> bool {
        self.print_firmware_info
            || self.check
            || self.download_latest
            || self.update_latest
            || self.fw_image.is_some()
            || self.list
            || self.check_catalogue
            || self.monitor
            || self.record.is_some()
            || self.feedback.is_some()
            || self.diagnostics
    }
}

fn parse_u16(value: &str) -> Result<u16, String> {
    if let Some(hex) = value.strip_prefix("0x") {
        u16::from_str_radix(hex, 16).map_err(|e| e.to_string())
    } else {
        value.parse::<u16>().map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn read_and_write_actions_cannot_be_combined() {
        assert!(
            Args::try_parse_from(["tool", "--print-firmware-info", "--update-latest"]).is_err()
        );
        assert!(Args::try_parse_from(["tool", "--download-latest", "--update-latest"]).is_err());
        assert!(Args::try_parse_from(["tool", "--json", "--update-latest"]).is_err());
        assert!(Args::try_parse_from(["tool", "--start-update-only"]).is_err());
        assert!(Args::try_parse_from(["tool", "--invalid"]).is_err());
        assert!(Args::try_parse_from(["tool", "--monitor", "--update-latest"]).is_err());
        assert!(Args::try_parse_from(["tool", "--feedback", "rumble", "--yes"]).is_err());
        assert!(Args::try_parse_from(["tool", "--duration", "5"]).is_err());
        assert!(
            Args::try_parse_from(["tool", "--record", "test.jsonl", "--duration", "5"]).is_ok()
        );
    }
    #[test]
    fn interactive_mode_cannot_bypass_update_confirmation() {
        assert!(Args::try_parse_from(["tool", "--interactive", "--yes"]).is_err());
        assert!(Args::try_parse_from(["tool", "--interactive", "--update-latest"]).is_err());
        let default = Args::try_parse_from(["tool"]).unwrap();
        assert!(!default.has_action());
        assert!(!default.yes);
        let check = Args::try_parse_from(["tool", "--check"]).unwrap();
        assert!(check.has_action());
        assert!(!check.update_latest);
        assert!(!check.yes);
    }
    #[test]
    fn feedback_flags_require_an_action_and_reject_conflicts() {
        for flags in [
            vec!["--color", "red"],
            vec!["--brightness", "128"],
            vec!["--cycle"],
            vec!["--trigger", "left"],
            vec!["--effect", "bow"],
            vec!["--start", "4"],
            vec!["--end", "7"],
            vec!["--strength", "2"],
            vec!["--frequency", "25"],
            vec!["--snap-strength", "2"],
            vec!["--first-foot", "1"],
            vec!["--second-foot", "3"],
            vec!["--strength-b", "4"],
            vec!["--period", "20"],
            vec!["--zones", "0,0,0,0,2,2,2,2,2,2"],
        ] {
            let mut command = vec!["tool"];
            command.extend(flags);
            assert!(Args::try_parse_from(command).is_err());
        }
        for flags in [
            vec!["--feedback", "lightbar", "--color", "red", "--cycle"],
            vec![
                "--feedback",
                "triggers",
                "--zones",
                "0,0,0,0,2,2,2,2,2,2",
                "--start",
                "4",
            ],
            vec![
                "--feedback",
                "triggers",
                "--zones",
                "0,0,0,0,2,2,2,2,2,2",
                "--strength",
                "2",
            ],
            vec!["--feedback", "lightbar", "--brightness", "256"],
            vec!["--feedback", "triggers", "--period", "256"],
        ] {
            let mut command = vec!["tool"];
            command.extend(flags);
            assert!(Args::try_parse_from(command).is_err());
        }
    }
}
