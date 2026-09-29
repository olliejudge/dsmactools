use crate::{
    error::{AppError, Result},
    protocol::FirmwareInfo,
};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

pub const SONY_BASE: &str = "https://fwupdater.dl.playstation.net/fwupdater";
pub const FIRMWARE_SIZE: usize = 950_272;

fn invalid(message: impl Into<String>) -> AppError {
    AppError::Validation(message.into())
}

pub fn target_for(info: &FirmwareInfo, pid: u16) -> Result<String> {
    match (pid, info.software_series) {
        (0x0ce6, 0x0004 | 0x000b | 0x000e) | (0x0df2, 0x0044) => {
            Ok(format!("{:04X}", info.software_series))
        }
        _ => Err(invalid(format!(
            "Unsupported PID/firmware series: {pid:04x}/{:04x}; no image will be selected",
            info.software_series
        ))),
    }
}

pub fn parse_catalogue(data: &[u8], target: &str) -> Result<u16> {
    let json: serde_json::Value = serde_json::from_slice(data)
        .map_err(|e| invalid(format!("Invalid Sony catalogue: {e}")))?;
    let key = format!("FwUpdate{target}LatestVersion");
    let version = json
        .get(&key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| invalid(format!("Sony catalogue is missing {key}")))?;
    let hex = version
        .strip_prefix("0x")
        .filter(|s| s.len() == 4 && s.bytes().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(|| invalid(format!("Invalid Sony version: {version}")))?;
    u16::from_str_radix(hex, 16).map_err(|e| invalid(e.to_string()))
}

fn fetch(url: &str) -> Result<Vec<u8>> {
    // curl uses macOS's certificate store; firmware never comes from mirrors.
    let output = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--connect-timeout",
            "10",
            "--max-time",
            "60",
            "--max-filesize",
            "4194304",
            url,
        ])
        .output()?;
    if !output.status.success() {
        return Err(invalid(format!(
            "Sony download failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output.stdout)
}

pub fn latest(target: &str) -> Result<u16> {
    parse_catalogue(&fetch(&format!("{SONY_BASE}/info.json"))?, target)
}

pub struct FirmwareImage {
    pub data: Vec<u8>,
    pub version: u16,
    pub sha256: String,
}

impl FirmwareImage {
    pub fn validate(
        data: Vec<u8>,
        info: &FirmwareInfo,
        pid: u16,
        expected: Option<u16>,
    ) -> Result<Self> {
        target_for(info, pid)?;
        if data.len() != FIRMWARE_SIZE {
            return Err(invalid(format!(
                "Unexpected image size: {}; expected {FIRMWARE_SIZE}. Review new image formats before flashing",
                data.len()
            )));
        }
        if !data.starts_with(b"Copyright (C) ")
            || !data[..64]
                .windows(30)
                .any(|s| s == b"Sony Interactive Entertainment")
        {
            return Err(invalid("Image is missing the Sony firmware header"));
        }
        let image_pid = u16::from_le_bytes(data[0x62..0x64].try_into().unwrap());
        let series = u32::from_le_bytes(data[0x74..0x78].try_into().unwrap());
        let fw_type = u16::from_le_bytes(data[0x60..0x62].try_into().unwrap());
        if image_pid != pid
            || series != u32::from(info.software_series)
            || fw_type != info.firmware_type
        {
            return Err(invalid(format!(
                "Image/controller mismatch: PID {image_pid:04x}/{pid:04x}, series {series:04x}/{:04x}, type {fw_type}/{}",
                info.software_series, info.firmware_type
            )));
        }
        let version = u16::from_le_bytes(data[0x78..0x7a].try_into().unwrap());
        if expected.is_some_and(|v| v != version) {
            return Err(invalid("Image version does not match Sony's catalogue"));
        }
        if version <= info.firmware_version {
            return Err(invalid(
                "Image is not newer than installed firmware; reflashing and downgrades are disabled",
            ));
        }
        let sha256 = format!("{:x}", Sha256::digest(&data));
        Ok(Self {
            data,
            version,
            sha256,
        })
    }

    pub fn load(path: &Path, info: &FirmwareInfo, pid: u16) -> Result<Self> {
        Self::validate(std::fs::read(path)?, info, pid, None)
    }
}

pub fn download(
    target: &str,
    version: u16,
    info: &FirmwareInfo,
    pid: u16,
) -> Result<(FirmwareImage, PathBuf)> {
    let url = format!("{SONY_BASE}/fwupdate{target}/0x{version:04X}/FWUPDATE{target}.bin");
    let image = FirmwareImage::validate(fetch(&url)?, info, pid, Some(version))?;
    let dir = PathBuf::from("firmware")
        .join(target)
        .join(format!("0x{version:04X}"));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("FWUPDATE{target}.bin"));
    // A partial file cannot become the cached firmware image.
    let temporary = dir.join(format!(".download-{}", std::process::id()));
    std::fs::write(&temporary, &image.data)?;
    std::fs::rename(&temporary, &path)?;
    let manifest = serde_json::json!({"source":url,"target":target,"version":format!("0x{version:04X}"),
        "bytes":image.data.len(),"sha256":image.sha256});
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).map_err(|e| invalid(e.to_string()))?,
    )?;
    Ok((image, path))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn info() -> FirmwareInfo {
        let mut r = vec![0; 64];
        r[0] = 0x20;
        r[20] = 2;
        r[22] = 0xb;
        r[44..46].copy_from_slice(&0x0580u16.to_le_bytes());
        FirmwareInfo::parse(r).unwrap()
    }
    fn image() -> Vec<u8> {
        let mut d = vec![0; FIRMWARE_SIZE];
        let text = b"Copyright (C) 2022 Sony Interactive Entertainment Inc.";
        d[..text.len()].copy_from_slice(text);
        d[0x60] = 2;
        d[0x62..0x64].copy_from_slice(&0x0ce6u16.to_le_bytes());
        d[0x74] = 0xb;
        d[0x78..0x7a].copy_from_slice(&0x0630u16.to_le_bytes());
        d
    }
    #[test]
    fn selects_actual_series_and_rejects_unknown_or_wrong_pid() {
        let mut i = info();
        assert_eq!(target_for(&i, 0x0ce6).unwrap(), "000B");
        assert!(target_for(&i, 0x0df2).is_err());
        i.software_series = 0x9999;
        assert!(target_for(&i, 0x0ce6).is_err());
    }
    #[test]
    fn catalogue_is_strict_and_allows_new_versions() {
        assert_eq!(
            parse_catalogue(br#"{"FwUpdate000BLatestVersion":"0x0701"}"#, "000B").unwrap(),
            0x701
        );
        for d in [
            br#"{}"#.as_slice(),
            br#"{"FwUpdate000BLatestVersion":"../../bad"}"#,
            b"not json",
        ] {
            assert!(parse_catalogue(d, "000B").is_err());
        }
    }
    #[test]
    fn checks_size_pid_series_type_catalogue_and_upgrade() {
        assert!(FirmwareImage::validate(image(), &info(), 0x0ce6, Some(0x630)).is_ok());
        for offset in [0, 0x60, 0x62, 0x74] {
            let mut d = image();
            d[offset] ^= 1;
            assert!(FirmwareImage::validate(d, &info(), 0x0ce6, None).is_err());
        }
        assert!(FirmwareImage::validate(vec![0; 256], &info(), 0x0ce6, None).is_err());
        assert!(FirmwareImage::validate(image(), &info(), 0x0ce6, Some(0x641)).is_err());
        let mut i = info();
        i.firmware_version = 0x630;
        assert!(FirmwareImage::validate(image(), &i, 0x0ce6, None).is_err());
    }
}
