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
}
