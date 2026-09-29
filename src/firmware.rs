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

/// Compare every published catalogue entry with this release's embedded baseline.
/// This command never enumerates or opens a controller.
pub fn catalogue_report(data: &[u8]) -> Result<serde_json::Value> {
    let current: serde_json::Value = serde_json::from_slice(data)
        .map_err(|e| invalid(format!("Invalid Sony catalogue: {e}")))?;
    let current = current
        .as_object()
        .filter(|c| !c.is_empty())
        .ok_or_else(|| invalid("Empty or malformed Sony catalogue"))?;
    for (key, value) in current {
        if key.starts_with("FwUpdate") {
            let target = key
                .strip_prefix("FwUpdate")
                .and_then(|k| k.strip_suffix("LatestVersion"))
                .filter(|t| t.len() == 4 && t.bytes().all(|b| b.is_ascii_hexdigit()));
            let target = target
                .ok_or_else(|| invalid(format!("Unexpected firmware catalogue key: {key}")))?;
            parse_catalogue(data, target)?;
        }
        if !value.is_string() {
            return Err(invalid(format!("Invalid catalogue value for {key}")));
        }
    }
    let baseline: serde_json::Value =
        serde_json::from_slice(include_bytes!("../catalogue-baseline.json"))
            .map_err(|e| invalid(format!("Invalid embedded catalogue baseline: {e}")))?;
    let baseline = baseline
        .as_object()
        .ok_or_else(|| invalid("Invalid embedded catalogue baseline"))?;
    let keys = baseline
        .keys()
        .chain(current.keys())
        .collect::<std::collections::BTreeSet<_>>();
    let changes=keys.into_iter().filter(|key|baseline.get(*key)!=current.get(*key))
        .map(|key|serde_json::json!({"key":key,"previous":baseline.get(key),"current":current.get(key)})).collect::<Vec<_>>();
    Ok(
        serde_json::json!({"source":format!("{SONY_BASE}/info.json"),"catalogue":current,"changes":changes}),
    )
}

pub fn check_catalogue() -> Result<()> {
    let report = catalogue_report(&fetch(&format!("{SONY_BASE}/info.json"))?)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|e| invalid(e.to_string()))?
    );
    if report["changes"]
        .as_array()
        .is_some_and(|changes| !changes.is_empty())
    {
        return Err(invalid(
            "Sony's catalogue changed. Review the changes before updating catalogue-baseline.json.",
        ));
    }
    Ok(())
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
        // The firmware report's type changes after upgrades (2 -> 3 on our
        // tested controller). Header offset 0x60 is not that device identity.
        if image_pid != pid || series != u32::from(info.software_series) {
            return Err(invalid(format!(
                "Image/controller mismatch: PID {image_pid:04x}/{pid:04x}, series {series:04x}/{:04x}",
                info.software_series
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
    cache_dir: Option<&Path>,
) -> Result<(FirmwareImage, PathBuf)> {
    let url = format!("{SONY_BASE}/fwupdate{target}/0x{version:04X}/FWUPDATE{target}.bin");
    let image = FirmwareImage::validate(fetch(&url)?, info, pid, Some(version))?;
    let dir = cache_directory(cache_dir)?
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

pub fn cache_directory(override_path: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = override_path {
        return Ok(path.to_path_buf());
    }
    let user_home = std::env::var_os("HOME")
        .ok_or_else(|| invalid("Cannot locate user cache; provide --cache-dir"))?;
    #[cfg(target_os = "macos")]
    let cache = PathBuf::from(user_home).join("Library/Caches/ds-mac-tools/firmware");
    #[cfg(not(target_os = "macos"))]
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(user_home).join(".cache"))
        .join("ds-mac-tools/firmware");
    Ok(cache)
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
    fn checks_size_pid_series_catalogue_and_upgrade() {
        assert!(FirmwareImage::validate(image(), &info(), 0x0ce6, Some(0x630)).is_ok());
        for offset in [0, 0x62, 0x74] {
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
    #[test]
    fn firmware_type_can_change_across_an_upgrade() {
        let mut current = info();
        current.firmware_type = 3;
        current.firmware_version = 0x0630;
        let mut next = image();
        next[0x78..0x7a].copy_from_slice(&0x0701u16.to_le_bytes());
        assert!(FirmwareImage::validate(next, &current, 0x0ce6, Some(0x0701)).is_ok());
    }
    #[test]
    fn cache_override_is_independent_of_working_directory() {
        assert_eq!(
            cache_directory(Some(Path::new("/tmp/ds-test-cache"))).unwrap(),
            PathBuf::from("/tmp/ds-test-cache")
        );
    }
    #[test]
    fn full_catalogue_detects_versions_new_targets_and_removed_entries() {
        let baseline: serde_json::Value =
            serde_json::from_slice(include_bytes!("../catalogue-baseline.json")).unwrap();
        let report = catalogue_report(&serde_json::to_vec(&baseline).unwrap()).unwrap();
        assert!(report["changes"].as_array().unwrap().is_empty());
        let mut changed = baseline.clone();
        changed["FwUpdate000BLatestVersion"] = serde_json::json!("0x0701");
        changed["FwUpdate0099LatestVersion"] = serde_json::json!("0x0001");
        changed
            .as_object_mut()
            .unwrap()
            .remove("FwUpdate0044LatestVersion");
        let report = catalogue_report(&serde_json::to_vec(&changed).unwrap()).unwrap();
        assert_eq!(report["changes"].as_array().unwrap().len(), 3);
    }
    #[test]
    fn full_catalogue_rejects_empty_or_malformed_metadata() {
        for data in [
            b"{}".as_slice(),
            b"[]",
            br#"{"FwUpdate000BLatestVersion":"garbage"}"#,
            br#"{"FwUpdate../../LatestVersion":"0x0701"}"#,
        ] {
            assert!(catalogue_report(data).is_err());
        }
    }
}
