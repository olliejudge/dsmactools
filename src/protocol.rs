pub const REPORT_ID_FIRMWARE_INFO: u8 = 0x20;
pub const REPORT_ID_UPDATE_COMMAND: u8 = 0xF4;
pub const REPORT_ID_UPDATE_STATUS: u8 = 0xF5;

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
    pub fn parse(raw: Vec<u8>) -> crate::error::Result<Self> {
        if raw.len() != 64 || raw[0] != REPORT_ID_FIRMWARE_INFO {
            return Err(crate::error::AppError::FirmwareInfoTooShort(raw.len()));
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

#[derive(Debug, Clone)]
pub struct UpdateStatus {
    #[allow(dead_code)]
    pub report_id: u8,
    pub command: UpdateCommand,
    pub status_raw: u8,
    #[allow(dead_code)]
    pub raw: Vec<u8>,
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
}
