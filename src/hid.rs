use std::ffi::CStr;

use hidapi::{BusType, HidApi, HidDevice};

use crate::error::{AppError, Result};
use crate::protocol::{self, FirmwareInfo, REPORT_ID_FIRMWARE_INFO};
use crate::update::UpdateAccess;

const SONY_VENDOR_ID: u16 = 0x054c;
/// DualSense and DualSense Edge.
const SUPPORTED_PRODUCT_IDS: [u16; 2] = [0x0ce6, 0x0df2];

/// Shown when a controller is paired over Bluetooth but none is connected over USB.
pub const BLUETOOTH_HINT: &str = "Your controller is connected over Bluetooth. Connect it with a USB data cable to update or test it.";

fn is_supported_controller(vid: u16, pid: u16) -> bool {
    vid == SONY_VENDOR_ID && SUPPORTED_PRODUCT_IDS.contains(&pid)
}

fn is_usb_controller(vid: u16, pid: u16, bus: BusType) -> bool {
    is_supported_controller(vid, pid) && matches!(bus, BusType::Usb)
}

fn is_bluetooth_controller(vid: u16, pid: u16, bus: BusType) -> bool {
    is_supported_controller(vid, pid) && matches!(bus, BusType::Bluetooth)
}

/// Whether a DualSense or Edge is visible over Bluetooth. This only enumerates; a Bluetooth
/// device is never opened. Enumeration failures count as "no".
pub fn bluetooth_controller_present() -> bool {
    HidApi::new().is_ok_and(|api| {
        api.device_list()
            .any(|d| is_bluetooth_controller(d.vendor_id(), d.product_id(), d.bus_type()))
    })
}

pub struct DualSenseHid {
    _api: HidApi,
    dev: HidDevice,
}

pub fn find_first_device_path(vid: u16, pid: u16) -> Result<String> {
    let api = HidApi::new()?;
    let mut devices = api.device_list().filter(|d| {
        d.vendor_id() == vid && d.product_id() == pid && matches!(d.bus_type(), BusType::Usb)
    });
    let Some(device) = devices.next() else {
        let bluetooth = api
            .device_list()
            .any(|d| is_bluetooth_controller(d.vendor_id(), d.product_id(), d.bus_type()));
        return Err(if bluetooth {
            AppError::Validation(BLUETOOTH_HINT.into())
        } else {
            AppError::DeviceNotFound { vid, pid }
        });
    };
    if devices.next().is_some() {
        return Err(AppError::Validation(
            "Multiple USB interfaces/controllers found; select --path from --list".into(),
        ));
    }
    Ok(device.path().to_string_lossy().to_string())
}

impl DualSenseHid {
    pub fn open(vid: u16, pid: u16, path: &str) -> Result<Self> {
        let api = HidApi::new()?;
        let device_path = find_path(&api, vid, pid, path)?;
        let dev = api.open_path(device_path)?;
        Ok(Self { _api: api, dev })
    }

    pub fn read_input(&self, timeout_ms: i32) -> Result<Option<Vec<u8>>> {
        let mut raw = [0u8; 64];
        let size = self.dev.read_timeout(&mut raw, timeout_ms)?;
        if size == 0 {
            return Ok(None);
        }
        Ok(Some(raw[..size].to_vec()))
    }

    pub fn write_output(&self, report: &[u8]) -> Result<()> {
        let sent = self.dev.write(report)?;
        if sent != report.len() {
            return Err(AppError::Validation(format!(
                "Short output report: {sent}/{}",
                report.len()
            )));
        }
        Ok(())
    }

    pub fn get_firmware_info(&self) -> Result<FirmwareInfo> {
        let raw = self.get_feature_report(REPORT_ID_FIRMWARE_INFO, 64)?;
        log::debug!("Firmware report: {:02x?}", raw);
        FirmwareInfo::parse(raw)
    }

    pub fn battery(&self) -> Result<u8> {
        let mut raw = [0u8; 64];
        let size = self.dev.read_timeout(&mut raw, 2000)?;
        protocol::battery_percent(&raw[..size])
    }

    pub fn preflight_battery(&self) -> Result<u8> {
        protocol::require_update_battery(self.battery()?)
    }

    /// Send one firmware-update feature report. Only the update module can create the
    /// [`UpdateAccess`] this requires, so no other code can write firmware commands.
    pub(crate) fn send_update_report(&self, _: &UpdateAccess, report: &[u8]) -> Result<()> {
        self.send_feature_report_raw(report)
    }

    /// Read a firmware-update feature report; restricted like [`Self::send_update_report`].
    pub(crate) fn read_update_report(
        &self,
        _: &UpdateAccess,
        report_id: u8,
        length: usize,
    ) -> Result<Vec<u8>> {
        self.get_feature_report(report_id, length)
    }

    fn get_feature_report(&self, report_id: u8, length: usize) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; length];
        if !buf.is_empty() {
            buf[0] = report_id;
        }
        let size = self.dev.get_feature_report(&mut buf)?;
        buf.truncate(size);
        Ok(buf)
    }

    fn send_feature_report_raw(&self, data: &[u8]) -> Result<()> {
        self.dev.send_feature_report(data)?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Controller {
    pub path: String,
    pub pid: u16,
    pub name: String,
}

pub fn controllers() -> Result<Vec<Controller>> {
    let api = HidApi::new()?;
    Ok(api
        .device_list()
        .filter(|d| is_usb_controller(d.vendor_id(), d.product_id(), d.bus_type()))
        .map(|d| Controller {
            path: d.path().to_string_lossy().into_owned(),
            pid: d.product_id(),
            name: d
                .product_string()
                .unwrap_or("DualSense controller")
                .to_owned(),
        })
        .collect())
}

pub fn print_devices(vid: u16, pid: u16) -> Result<()> {
    let api = HidApi::new()?;
    for d in api
        .device_list()
        .filter(|d| d.vendor_id() == vid && d.product_id() == pid)
    {
        println!(
            "{} {:04x}:{:04x} {:?} usage={:04x}:{:04x} {}",
            d.path().to_string_lossy(),
            d.vendor_id(),
            d.product_id(),
            d.bus_type(),
            d.usage_page(),
            d.usage(),
            d.product_string().unwrap_or("")
        );
    }
    Ok(())
}

fn find_path<'a>(api: &'a HidApi, vid: u16, pid: u16, path_str: &str) -> Result<&'a CStr> {
    let mut matches = api.device_list().filter(|d| {
        d.vendor_id() == vid && d.product_id() == pid && matches!(d.bus_type(), BusType::Usb)
    });
    for device in matches.by_ref() {
        let path = device.path();
        if path.to_string_lossy() == path_str {
            return Ok(path);
        }
    }
    Err(AppError::DevicePathNotMatched(path_str.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bluetooth_hint_only_covers_supported_sony_controllers_over_bluetooth() {
        assert!(is_bluetooth_controller(0x054c, 0x0ce6, BusType::Bluetooth));
        assert!(is_bluetooth_controller(0x054c, 0x0df2, BusType::Bluetooth));
        assert!(!is_bluetooth_controller(0x054c, 0x0ce6, BusType::Usb));
        assert!(!is_bluetooth_controller(0x054c, 0x09cc, BusType::Bluetooth));
        assert!(!is_bluetooth_controller(0x045e, 0x0ce6, BusType::Bluetooth));
    }

    #[test]
    fn usb_listing_excludes_bluetooth_and_other_devices() {
        assert!(is_usb_controller(0x054c, 0x0ce6, BusType::Usb));
        assert!(is_usb_controller(0x054c, 0x0df2, BusType::Usb));
        assert!(!is_usb_controller(0x054c, 0x0ce6, BusType::Bluetooth));
        assert!(!is_usb_controller(0x054c, 0x0ba0, BusType::Usb));
    }
}
