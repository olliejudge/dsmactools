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
    parse_version(version)
}

/// Parse a catalogue version, which must be exactly `0x` followed by four hex digits.
fn parse_version(version: &str) -> Result<u16> {
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
            // Must be first: ignore user curl config, including extra URLs/uploads.
            "--disable",
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
        .output()
        .map_err(|e| {
            log::debug!("Could not run curl: {e}");
            invalid(if e.kind() == std::io::ErrorKind::NotFound {
                "Couldn't find curl, which is needed to contact Sony's firmware server."
            } else {
                FETCH_FAILED
            })
        })?;
    if !output.status.success() {
        // The raw curl detail stays behind -v, like the other protocol debugging.
        log::debug!(
            "curl failed ({:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return Err(invalid(fetch_failure_message(output.status.code())));
    }
    Ok(output.stdout)
}

const FETCH_FAILED: &str =
    "Couldn't reach Sony's firmware server. Check your internet connection and try again.";

/// A plain-language reason for a failed curl run. `22` is an HTTP error answer (`--fail`),
/// which means the server was reached.
fn fetch_failure_message(exit_code: Option<i32>) -> &'static str {
    match exit_code {
        Some(22) => "Sony's firmware server didn't return the file. Try again later.",
        _ => FETCH_FAILED,
    }
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
            key.strip_prefix("FwUpdate")
                .and_then(|k| k.strip_suffix("LatestVersion"))
                .filter(|t| t.len() == 4 && t.bytes().all(|b| b.is_ascii_hexdigit()))
                .ok_or_else(|| invalid(format!("Unexpected firmware catalogue key: {key}")))?;
            let version = value
                .as_str()
                .ok_or_else(|| invalid(format!("Sony catalogue is missing {key}")))?;
            parse_version(version)?;
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

/// The controller state a firmware image was validated against. An install refuses to start
/// unless the controller still reports exactly this state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControllerIdentity {
    pub pid: u16,
    pub software_series: u16,
    pub hardware_info: u32,
    pub firmware_version: u16,
}

impl ControllerIdentity {
    pub fn of(info: &FirmwareInfo, pid: u16) -> Self {
        Self {
            pid,
            software_series: info.software_series,
            hardware_info: info.hardware_info,
            firmware_version: info.firmware_version,
        }
    }
}

/// A firmware image that passed every compatibility check for one controller state.
///
/// The fields are private, so an image can only come from [`FirmwareImage::validate`] (also
/// used by [`FirmwareImage::load`] and [`download`]).
pub struct FirmwareImage {
    data: Vec<u8>,
    version: u16,
    sha256: String,
    validated_for: ControllerIdentity,
}

impl FirmwareImage {
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn version(&self) -> u16 {
        self.version
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// The controller state the image was checked against.
    pub fn validated_for(&self) -> ControllerIdentity {
        self.validated_for
    }

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
            validated_for: ControllerIdentity::of(info, pid),
        })
    }

    pub fn load(path: &Path, info: &FirmwareInfo, pid: u16) -> Result<Self> {
        Self::validate(std::fs::read(path)?, info, pid, None)
    }
}

/// Where an image returned by [`download`] came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageSource {
    Downloaded,
    /// An earlier download, reused after its checksum and compatibility were verified again.
    Cached,
}

pub fn download(
    target: &str,
    version: u16,
    info: &FirmwareInfo,
    pid: u16,
    cache_dir: Option<&Path>,
) -> Result<(FirmwareImage, PathBuf, ImageSource)> {
    download_with(target, version, info, pid, cache_dir, fetch)
}

fn download_with(
    target: &str,
    version: u16,
    info: &FirmwareInfo,
    pid: u16,
    cache_dir: Option<&Path>,
    fetch: impl FnOnce(&str) -> Result<Vec<u8>>,
) -> Result<(FirmwareImage, PathBuf, ImageSource)> {
    let url = format!("{SONY_BASE}/fwupdate{target}/0x{version:04X}/FWUPDATE{target}.bin");
    let dir = cache_directory(cache_dir)?
        .join(target)
        .join(format!("0x{version:04X}"));
    let path = dir.join(format!("FWUPDATE{target}.bin"));
    // A cached copy is only a shortcut for the network fetch: it must match its recorded
    // checksum and then passes the same validation as a fresh download.
    if let Some(data) = verified_cached_image(&dir, target, version)
        && let Ok(image) = FirmwareImage::validate(data, info, pid, Some(version))
    {
        return Ok((image, path, ImageSource::Cached));
    }
    let image = FirmwareImage::validate(fetch(&url)?, info, pid, Some(version))?;
    std::fs::create_dir_all(&dir)?;
    // A partial file cannot become the cached firmware image.
    let temporary = dir.join(format!(".download-{}", std::process::id()));
    std::fs::write(&temporary, image.data())?;
    std::fs::rename(&temporary, &path)?;
    let manifest = serde_json::json!({"source":url,"target":target,"version":format!("0x{version:04X}"),
        "bytes":image.data().len(),"sha256":image.sha256()});
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).map_err(|e| invalid(e.to_string()))?,
    )?;
    Ok((image, path, ImageSource::Downloaded))
}

/// The cached image for exactly this target and version, if `manifest.json` names the same
/// target, version, size and SHA-256 as the file's actual contents. Anything else is `None`.
fn verified_cached_image(dir: &Path, target: &str, version: u16) -> Option<Vec<u8>> {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).ok()?).ok()?;
    if manifest.get("target")?.as_str()? != target
        || manifest.get("version")?.as_str()? != format!("0x{version:04X}")
        || manifest.get("bytes")?.as_u64()? != FIRMWARE_SIZE as u64
    {
        return None;
    }
    let recorded = manifest.get("sha256")?.as_str()?;
    let file = dir.join(format!("FWUPDATE{target}.bin"));
    // Never follow a symlink to some other file.
    let meta = std::fs::symlink_metadata(&file).ok()?;
    if !meta.is_file() || meta.len() != FIRMWARE_SIZE as u64 {
        return None;
    }
    let data = std::fs::read(&file).ok()?;
    (format!("{:x}", Sha256::digest(&data)) == recorded.to_ascii_lowercase()).then_some(data)
}

/// The count and total size of downloaded firmware files.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CacheUsage {
    pub files: u64,
    pub bytes: u64,
}

/// A downloaded-firmware file this app writes: an image, its manifest or a partial download.
fn is_app_file(name: &str) -> bool {
    name == "manifest.json"
        || name.starts_with(".download-")
        || (name.starts_with("FWUPDATE") && name.ends_with(".bin"))
}

/// Every directory level this app creates: `<target>/0x<version>` under the cache.
fn is_target_dir(name: &str) -> bool {
    name.len() == 4 && name.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_version_dir(name: &str) -> bool {
    name.strip_prefix("0x")
        .is_some_and(|v| v.len() == 4 && v.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn subdirs(dir: &Path, accept: fn(&str) -> bool) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        // `file_type` does not follow symlinks, so a link can never lead outside the cache.
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| e.file_name().to_str().is_some_and(accept))
        .map(|e| e.path())
        .collect()
}

fn app_files(dir: &Path) -> Vec<(PathBuf, u64)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.file_name().to_str().is_some_and(is_app_file))
        .filter_map(|e| {
            let meta = std::fs::symlink_metadata(e.path()).ok()?;
            meta.is_file().then(|| (e.path(), meta.len()))
        })
        .collect()
}

fn app_downloads(root: &Path) -> Vec<(PathBuf, u64)> {
    subdirs(root, is_target_dir)
        .iter()
        .flat_map(|t| subdirs(t, is_version_dir))
        .flat_map(|v| app_files(&v))
        .collect()
}

/// The firmware this app has downloaded under `root`: file count and total size.
pub fn cache_usage(root: &Path) -> CacheUsage {
    app_downloads(root)
        .iter()
        .fold(CacheUsage::default(), |usage, (_, len)| CacheUsage {
            files: usage.files + 1,
            bytes: usage.bytes + len,
        })
}

/// Refuse to delete from anywhere but this app's own cache: the default
/// `.../ds-mac-tools/firmware` folder or the exact `--cache-dir` in effect. A symlinked root is
/// refused too, as are the filesystem root and the home folder.
fn check_cache_root(root: &Path, explicit: Option<&Path>, home: Option<&Path>) -> Result<()> {
    let expected = root.ends_with("ds-mac-tools/firmware") || explicit == Some(root);
    let dangerous = root.parent().is_none() || home == Some(root);
    if !expected || dangerous {
        return Err(invalid(format!(
            "Refusing to delete from {}: it is not DS Mac Tools' firmware folder",
            root.display()
        )));
    }
    match std::fs::symlink_metadata(root) {
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(invalid(format!(
            "Refusing to delete from {}: it is not a plain folder",
            root.display()
        ))),
        // Nothing there, so nothing to delete.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Delete downloaded firmware from the cache at `root` (see [`check_cache_root`]). Only the
/// files and folders this app creates are removed, so a `--cache-dir` holding anything else
/// keeps it. Folders left empty are removed, and so is the default cache folder itself.
pub fn delete_downloads(root: &Path, explicit: Option<&Path>) -> Result<CacheUsage> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    check_cache_root(root, explicit, home.as_deref())?;
    let mut removed = CacheUsage::default();
    for (file, len) in app_downloads(root) {
        std::fs::remove_file(&file)?;
        removed.files += 1;
        removed.bytes += len;
    }
    for target in subdirs(root, is_target_dir) {
        for version in subdirs(&target, is_version_dir) {
            let _ = std::fs::remove_dir(version);
        }
        let _ = std::fs::remove_dir(target);
    }
    // Only the default cache folder is ours to remove; a chosen --cache-dir stays.
    if root.ends_with("ds-mac-tools/firmware") {
        let _ = std::fs::remove_dir(root);
    }
    Ok(removed)
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

/// Image bytes that pass validation for `pid`/`series`, with a known header layout.
#[cfg(test)]
pub(crate) fn sample_image(pid: u16, series: u16, version: u16) -> Vec<u8> {
    let mut d = vec![0; FIRMWARE_SIZE];
    let text = b"Copyright (C) 2022 Sony Interactive Entertainment Inc.";
    d[..text.len()].copy_from_slice(text);
    d[0x60] = 2;
    d[0x62..0x64].copy_from_slice(&pid.to_le_bytes());
    d[0x74..0x76].copy_from_slice(&series.to_le_bytes());
    d[0x78..0x7a].copy_from_slice(&version.to_le_bytes());
    // Distinct bytes after the header so ordering mistakes in a transfer are detectable.
    for (i, byte) in d.iter_mut().enumerate().skip(0x100) {
        *byte = (i % 251) as u8;
    }
    d
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
        sample_image(0x0ce6, 0x000b, 0x0630)
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
    fn version_parser_requires_prefix_four_hex_digits_and_no_extras() {
        assert_eq!(parse_version("0x0630").unwrap(), 0x630);
        assert_eq!(parse_version("0xFfFf").unwrap(), 0xffff);
        for bad in [
            "", "0x", "0630", "0X0630", "0x630", "0x00630", "0x063g", "0x+630", " 0x0630",
            "0x0630 ",
        ] {
            assert!(parse_version(bad).is_err(), "{bad:?}");
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
    fn validated_image_records_the_controller_it_was_checked_for() {
        let mut i = info();
        i.hardware_info = 0x1107;
        let image = FirmwareImage::validate(image(), &i, 0x0ce6, Some(0x630)).unwrap();
        assert_eq!(
            image.validated_for(),
            ControllerIdentity {
                pid: 0x0ce6,
                software_series: 0x000b,
                hardware_info: 0x1107,
                firmware_version: 0x0580,
            }
        );
        assert_eq!(image.version(), 0x0630);
        assert_eq!(image.data().len(), FIRMWARE_SIZE);
        assert_eq!(image.sha256().len(), 64);
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
    /// A fresh, empty directory under the system temp folder, removed on drop.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "dsmactools-test-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fetch_ok(data: Vec<u8>) -> impl FnOnce(&str) -> Result<Vec<u8>> {
        move |_| Ok(data)
    }
    fn fetch_forbidden(_: &str) -> Result<Vec<u8>> {
        Err(invalid("must not download"))
    }
    fn run_download(
        cache: &Path,
        fetch: impl FnOnce(&str) -> Result<Vec<u8>>,
    ) -> Result<(FirmwareImage, PathBuf, ImageSource)> {
        download_with("000B", 0x0630, &info(), 0x0ce6, Some(cache), fetch)
    }
    fn image_sha() -> String {
        format!("{:x}", Sha256::digest(image()))
    }

    #[test]
    fn download_is_cached_and_reused_without_the_network() {
        let tmp = TempDir::new("reuse");
        let (first, path, source) = run_download(&tmp.0, fetch_ok(image())).unwrap();
        assert_eq!(source, ImageSource::Downloaded);
        assert!(path.ends_with("000B/0x0630/FWUPDATE000B.bin"));
        let (second, path2, source) = run_download(&tmp.0, fetch_forbidden).unwrap();
        assert_eq!(source, ImageSource::Cached);
        assert_eq!(path2, path);
        assert_eq!(second.sha256(), first.sha256());
        assert_eq!(second.version(), 0x0630);
    }

    #[test]
    fn cached_image_still_goes_through_full_validation() {
        let tmp = TempDir::new("revalidate");
        run_download(&tmp.0, fetch_ok(image())).unwrap();
        // A controller that is already up to date refuses the cached image, as a fresh one.
        let mut current = info();
        current.firmware_version = 0x0630;
        let result = download_with(
            "000B",
            0x0630,
            &current,
            0x0ce6,
            Some(&tmp.0),
            fetch_forbidden,
        );
        assert!(result.is_err());
    }

    #[test]
    fn tampered_cache_is_rejected_and_downloaded_again() {
        let tmp = TempDir::new("tampered");
        let (_, path, _) = run_download(&tmp.0, fetch_ok(image())).unwrap();
        let mut bad = image();
        bad[0x2000] ^= 0xff; // same size and header, different contents
        std::fs::write(&path, &bad).unwrap();
        let (fresh, _, source) = run_download(&tmp.0, fetch_ok(image())).unwrap();
        assert_eq!(source, ImageSource::Downloaded);
        assert_eq!(fresh.data(), image().as_slice());
        // The tampered file is replaced and the next call can reuse the good copy.
        assert_eq!(std::fs::read(&path).unwrap(), image());
        assert_eq!(
            run_download(&tmp.0, fetch_forbidden).unwrap().2,
            ImageSource::Cached
        );
    }

    #[test]
    fn cache_without_a_matching_manifest_is_ignored() {
        let tmp = TempDir::new("manifest");
        let (_, path, _) = run_download(&tmp.0, fetch_ok(image())).unwrap();
        let manifest = path.with_file_name("manifest.json");
        let good = std::fs::read(&manifest).unwrap();
        let text = String::from_utf8(good.clone()).unwrap();
        for broken in [
            None,
            Some(b"not json".to_vec()),
            Some(text.replace("0x0630", "0x0631").into_bytes()),
            Some(text.replace("000B", "0004").into_bytes()),
            Some(text.replacen(&image_sha(), &"0".repeat(64), 1).into_bytes()),
        ] {
            match broken {
                None => std::fs::remove_file(&manifest).unwrap(),
                Some(bytes) => std::fs::write(&manifest, bytes).unwrap(),
            }
            let source = run_download(&tmp.0, fetch_ok(image())).unwrap().2;
            assert_eq!(source, ImageSource::Downloaded);
            std::fs::write(&manifest, &good).unwrap();
        }
    }

    #[test]
    fn failed_download_does_not_touch_the_cache() {
        let tmp = TempDir::new("failed");
        assert!(run_download(&tmp.0, fetch_forbidden).is_err());
        assert!(!tmp.0.join("000B").exists());
        // A bad download is refused before anything is cached.
        let mut wrong = image();
        wrong[0x62] ^= 1;
        assert!(run_download(&tmp.0, fetch_ok(wrong)).is_err());
        assert!(!tmp.0.join("000B").exists());
    }

    #[test]
    fn network_failures_get_plain_messages() {
        assert!(
            fetch_failure_message(Some(6)).starts_with("Couldn't reach Sony's firmware server.")
        );
        assert_eq!(fetch_failure_message(None), FETCH_FAILED);
        assert!(fetch_failure_message(Some(22)).contains("didn't return the file"));
    }

    #[test]
    fn delete_guard_accepts_only_the_app_cache_or_the_explicit_directory() {
        let home = Path::new("/Users/someone");
        let check = |root: &str, explicit: Option<&str>| {
            check_cache_root(Path::new(root), explicit.map(Path::new), Some(home)).is_ok()
        };
        assert!(check(
            "/Users/someone/Library/Caches/ds-mac-tools/firmware",
            None
        ));
        assert!(check("/x/ds-mac-tools/firmware", None));
        assert!(check("/data/fw", Some("/data/fw")));
        for refused in ["/", "/Users/someone", "/Users/someone/Documents", "/tmp"] {
            assert!(!check(refused, None), "{refused}");
        }
        // Nearly right names, or an explicit directory that differs, are refused.
        assert!(!check("/x/ds-mac-tools/firmware-old", None));
        assert!(!check("/x/not-ds-mac-tools/firmware", None));
        assert!(!check("/Users/someone/Documents", Some("/data/fw")));
        // Even an explicit --cache-dir cannot be the root or the home folder.
        assert!(!check("/", Some("/")));
        assert!(!check("/Users/someone", Some("/Users/someone")));
    }

    #[test]
    fn delete_removes_only_app_files_and_never_follows_symlinks() {
        let tmp = TempDir::new("delete");
        let outside = TempDir::new("delete-outside");
        std::fs::write(outside.0.join("precious.txt"), b"keep").unwrap();
        let root = tmp.0.join("cache");
        run_download(&root, fetch_ok(image())).unwrap();
        std::fs::write(root.join("notes.txt"), b"mine").unwrap();
        let version_dir = root.join("000B/0x0630");
        std::fs::write(version_dir.join("unrelated.txt"), b"mine").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside.0, root.join("0004")).unwrap();
            std::os::unix::fs::symlink(&outside.0, version_dir.join(".download-1")).unwrap();
        }
        let usage = cache_usage(&root);
        assert_eq!(usage.files, 2); // image and manifest
        assert!(usage.bytes > FIRMWARE_SIZE as u64);
        let removed = delete_downloads(&root, Some(&root)).unwrap();
        assert_eq!(removed, usage);
        assert_eq!(cache_usage(&root), CacheUsage::default());
        assert!(!version_dir.join("FWUPDATE000B.bin").exists());
        assert!(root.join("notes.txt").exists());
        assert!(version_dir.join("unrelated.txt").exists());
        assert!(outside.0.join("precious.txt").exists());
    }

    #[test]
    fn delete_refuses_a_symlinked_or_unexpected_cache_root() {
        let tmp = TempDir::new("root");
        let outside = TempDir::new("root-outside");
        std::fs::write(outside.0.join("precious.txt"), b"keep").unwrap();
        assert!(delete_downloads(&tmp.0.join("other"), None).is_err());
        #[cfg(unix)]
        {
            let link = tmp.0.join("ds-mac-tools-link");
            std::os::unix::fs::symlink(&outside.0, &link).unwrap();
            assert!(delete_downloads(&link, Some(&link)).is_err());
        }
        assert!(outside.0.join("precious.txt").exists());
        // A missing cache folder is simply empty.
        let missing = tmp.0.join("nothing-here");
        assert_eq!(
            delete_downloads(&missing, Some(&missing)).unwrap(),
            CacheUsage::default()
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
            br#"{"FwUpdate000BLatestVersion":1537}"#,
            br#"{"FwUpdate../../LatestVersion":"0x0701"}"#,
        ] {
            assert!(catalogue_report(data).is_err());
        }
    }
}
