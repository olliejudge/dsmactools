use clap::{ArgGroup, Parser};

pub const DEFAULT_VID: u16 = 0x054c;
pub const DEFAULT_PID: u16 = 0x0ce6;

#[derive(Parser, Debug)]
#[command(name = "dualsense-updater", version,
    about = "Check and update DualSense firmware from Sony over USB.",
    group(ArgGroup::new("action").args(["print_firmware_info", "check", "download_latest", "update_latest", "fw_image", "list"]).multiple(false)))]
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
        help = "Compare the controller with Sony's live firmware catalogue (default)."
    )]
    pub check: bool,
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
        conflicts_with_all = ["check", "download_latest", "update_latest", "fw_image", "list"]
    )]
    pub json: bool,
    #[arg(
        long,
        help = "Confirm a firmware update without an interactive prompt."
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
    }
}
