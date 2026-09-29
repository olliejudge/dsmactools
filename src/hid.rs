use std::ffi::CStr;

use hidapi::{BusType, HidApi, HidDevice};

use crate::error::{AppError, Result};
use crate::protocol::{
    FirmwareInfo, REPORT_ID_FIRMWARE_INFO, REPORT_ID_UPDATE_COMMAND, REPORT_ID_UPDATE_STATUS,
    UpdateCommand, UpdateStatus,
};

pub struct DualSenseHid {
    _api: HidApi,
    dev: HidDevice,
}

pub fn find_first_device_path(vid: u16, pid: u16) -> Result<String> {
    let api = HidApi::new()?;
    let mut devices = api.device_list().filter(|d| {
        d.vendor_id() == vid && d.product_id() == pid && matches!(d.bus_type(), BusType::Usb)
    });
    let device = devices
        .next()
        .ok_or(AppError::DeviceNotFound { vid, pid })?;
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

    pub fn get_firmware_info(&self) -> Result<FirmwareInfo> {
        let raw = self.get_feature_report(REPORT_ID_FIRMWARE_INFO, 64)?;
        log::debug!("Firmware report: {:02x?}", raw);
        FirmwareInfo::parse(raw)
    }

    pub fn preflight_battery(&self) -> Result<u8> {
        let mut raw = [0u8; 64];
        let size = self.dev.read_timeout(&mut raw, 2000)?;
        if size != 64 || raw[0] != 0x01 {
            return Err(AppError::Validation(
                "No valid USB input report; check cable and close other controller apps".into(),
            ));
        }
        let status = raw[53];
        let percent = match status >> 4 {
            0 | 1 => ((status & 0x0f) * 10 + 5).min(100),
            2 => 100,
            _ => {
                return Err(AppError::Validation(
                    "Controller reports a battery/charging fault".into(),
                ));
            }
        };
        if percent < 10 {
            return Err(AppError::Validation(
                "Charge the controller to at least 10% before updating".into(),
            ));
        }
        Ok(percent)
    }

    pub fn send_update_command(&self, command: UpdateCommand, payload: &[u8]) -> Result<()> {
        let max_chunk = 0x39usize;
        let offsets: Vec<usize> = if payload.is_empty() {
            vec![0]
        } else {
            (0..payload.len()).step_by(max_chunk).collect()
        };
        for off in offsets {
            let chunk = &payload[off..payload.len().min(off + max_chunk)];
            let data_len = chunk.len() as u8;
            let mut data = [0u8; 64];
            data[..3].copy_from_slice(&[REPORT_ID_UPDATE_COMMAND, command as u8, data_len]);
            data[3..3 + chunk.len()].copy_from_slice(chunk);
            let preview = chunk
                .iter()
                .take(4)
                .map(|b| format!("{:02x}", b))
                .collect::<Vec<_>>()
                .join(" ");
            log::debug!(
                "F4 chunk off={} len={} first4={}",
                off,
                chunk.len(),
                preview
            );
            self.send_feature_report_raw(&data)?;
            if command == UpdateCommand::StartUpdate && off == 0 {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
        Ok(())
    }

    pub fn get_update_status(&self, length: usize) -> Result<UpdateStatus> {
        let raw = self.get_feature_report(REPORT_ID_UPDATE_STATUS, length)?;
        let dump = raw
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<_>>()
            .join(" ");
        log::debug!("F5 status raw: {}", dump);
        if raw.is_empty() {
            return Err(AppError::UpdateStatusEmpty);
        }
        if raw[0] != REPORT_ID_UPDATE_STATUS || raw.len() != 4 {
            return Err(AppError::UpdateStatusMalformed(raw.len()));
        }
        let command = UpdateCommand::from_int(raw[1]);
        Ok(UpdateStatus {
            report_id: raw[0],
            command,
            status_raw: raw[2],
            raw,
        })
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
