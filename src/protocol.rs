use crate::error::{AppError, Result};

pub const REPORT_ID_FIRMWARE_INFO: u8 = 0x20;
pub const REPORT_ID_USB_INPUT: u8 = 0x01;
pub const USB_INPUT_REPORT_LEN: usize = 64;
/// Byte in the USB input report holding the charging state (high nibble) and level (low nibble).
const BATTERY_STATUS_OFFSET: usize = 53;
/// Firmware updates are refused below this approximate charge.
pub const MIN_UPDATE_BATTERY_PERCENT: u8 = 10;

/// Approximate battery percentage from a USB input report (report 0x01, 64 bytes).
///
/// Discharging (0) and charging (1) report a level nibble 0–10 as roughly level × 10 + 5;
/// charge complete (2) is 100%. Every other charging state is a battery or charging fault.
pub fn battery_percent(report: &[u8]) -> Result<u8> {
    if report.len() != USB_INPUT_REPORT_LEN || report[0] != REPORT_ID_USB_INPUT {
        return Err(AppError::Validation(
            "No valid USB input report; check cable and close other controller apps".into(),
        ));
    }
    let status = report[BATTERY_STATUS_OFFSET];
    match status >> 4 {
        0 | 1 => Ok(((status & 0x0f) * 10 + 5).min(100)),
        2 => Ok(100),
        _ => Err(AppError::Validation(
            "Controller reports a battery/charging fault".into(),
        )),
    }
}

/// Refuse a firmware update when the controller's charge is below the update threshold.
pub fn require_update_battery(percent: u8) -> Result<u8> {
    if percent < MIN_UPDATE_BATTERY_PERCENT {
        return Err(AppError::Validation(format!(
            "Battery is at {percent}%. Charge the controller to at least {MIN_UPDATE_BATTERY_PERCENT}% (it charges over the USB cable), then refresh and try again."
        )));
    }
    Ok(percent)
}

#[derive(Debug, Clone)]
pub struct FirmwareInfo {
    pub build_date: String,
    pub build_time: String,
    pub firmware_version: u16,
    pub firmware_type: u16,
    pub software_series: u16,
    pub hardware_info: u32,
    pub image_type: u8,
    #[allow(dead_code)]
    pub unknown: Vec<u8>,
    #[allow(dead_code)]
    pub raw: Vec<u8>,
}

impl FirmwareInfo {
    pub fn parse(raw: Vec<u8>) -> Result<Self> {
        if raw.len() != 64 || raw[0] != REPORT_ID_FIRMWARE_INFO {
            return Err(AppError::FirmwareInfoTooShort(raw.len()));
        }
        Ok(Self {
            build_date: decode_ascii(&raw[1..12]),
            build_time: decode_ascii(&raw[12..20]),
            firmware_type: u16::from_le_bytes(raw[20..22].try_into().unwrap()),
            software_series: u16::from_le_bytes(raw[22..24].try_into().unwrap()),
            hardware_info: u32::from_le_bytes(raw[24..28].try_into().unwrap()),
            firmware_version: u16::from_le_bytes(raw[44..46].try_into().unwrap()),
            image_type: raw[46],
            unknown: raw[20..].to_vec(),
            raw,
        })
    }
}

pub fn decode_ascii(data: &[u8]) -> String {
    let trimmed = data
        .iter()
        .copied()
        .take_while(|b| *b != 0)
        .collect::<Vec<u8>>();
    String::from_utf8_lossy(&trimmed).to_string()
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum UpdateCommand {
    StartUpdate = 0x00,
    WriteUpdateImage = 0x01,
    VerifyUpdateImage = 0x02,
    FinalizeUpdate = 0x03,
    Unknown = 0xFF,
}

impl UpdateCommand {
    pub fn from_int(value: u8) -> Self {
        match value {
            0x00 => Self::StartUpdate,
            0x01 => Self::WriteUpdateImage,
            0x02 => Self::VerifyUpdateImage,
            0x03 => Self::FinalizeUpdate,
            _ => Self::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn feature_report_includes_id_and_uses_little_endian_fields() {
        let mut raw = vec![0; 64];
        raw[0] = 0x20;
        raw[1..12].copy_from_slice(b"Nov  1 2024");
        raw[12..20].copy_from_slice(b"17:49:16");
        raw[20..22].copy_from_slice(&2u16.to_le_bytes());
        raw[22..24].copy_from_slice(&0xbu16.to_le_bytes());
        raw[24..28].copy_from_slice(&0x1107u32.to_le_bytes());
        raw[44..46].copy_from_slice(&0x0580u16.to_le_bytes());
        let i = FirmwareInfo::parse(raw).unwrap();
        assert_eq!(i.build_date, "Nov  1 2024");
        assert_eq!(i.build_time, "17:49:16");
        assert_eq!(i.firmware_version, 0x580);
        assert_eq!(i.software_series, 0xb);
        assert_eq!(i.hardware_info, 0x1107);
        assert_eq!(i.firmware_type, 2);
    }
    #[test]
    fn malformed_reports_are_rejected_before_field_reads() {
        for size in [0, 20, 63, 65] {
            assert!(FirmwareInfo::parse(vec![0x20; size]).is_err());
        }
        assert!(FirmwareInfo::parse(vec![0; 64]).is_err());
    }

    fn input_report(battery_status: u8) -> Vec<u8> {
        let mut raw = vec![0; 64];
        raw[0] = 0x01;
        raw[53] = battery_status;
        raw
    }

    #[test]
    fn battery_levels_map_to_percentages_and_the_update_gate() {
        for (status, percent, allowed) in [
            (0x00, 5, false), // discharging, level 0: below the 10% gate
            (0x01, 15, true), // discharging, level 1
            (0x05, 55, true),
            (0x0a, 100, true),
            (0x0f, 100, true), // out-of-range level is capped, not wrapped
            (0x10, 5, false),  // charging, level 0
            (0x11, 15, true),  // charging, level 1
            (0x20, 100, true), // charge complete ignores the level nibble
            (0x2f, 100, true),
        ] {
            let decoded = battery_percent(&input_report(status)).unwrap();
            assert_eq!(decoded, percent, "status 0x{status:02x}");
            assert_eq!(
                require_update_battery(decoded).is_ok(),
                allowed,
                "status 0x{status:02x}"
            );
        }
        assert_eq!(require_update_battery(10).unwrap(), 10);
        assert_eq!(
            require_update_battery(9).unwrap_err().to_string(),
            "Battery is at 9%. Charge the controller to at least 10% (it charges over the USB cable), then refresh and try again."
        );
    }

    #[test]
    fn battery_faults_and_invalid_reports_are_rejected() {
        for status in [0x30, 0x3a, 0x40, 0x80, 0xa0, 0xf0, 0xff] {
            assert_eq!(
                battery_percent(&input_report(status))
                    .unwrap_err()
                    .to_string(),
                "Controller reports a battery/charging fault",
                "status 0x{status:02x}"
            );
        }
        let mut bluetooth = input_report(0x20);
        bluetooth[0] = 0x31;
        let full = input_report(0x20);
        for report in [
            &[][..],
            &full[..63],
            &bluetooth[..],
            &[full.clone(), vec![0]].concat(),
        ] {
            assert_eq!(
                battery_percent(report).unwrap_err().to_string(),
                "No valid USB input report; check cable and close other controller apps"
            );
        }
    }
}
