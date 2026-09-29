use crate::protocol::UpdateCommand;
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("HID error: {0}")]
    Hid(#[from] hidapi::HidError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Validation(String),
    #[error("Device not found over USB for VID:PID {vid:04x}:{pid:04x}")]
    DeviceNotFound { vid: u16, pid: u16 },
    #[error("No USB device with the selected VID/PID matched {0}")]
    DevicePathNotMatched(String),
    #[error("Invalid firmware information report ({0} bytes); expected report 0x20, 64 bytes")]
    FirmwareInfoTooShort(usize),
    #[error("Update status report is empty")]
    UpdateStatusEmpty,
    #[error("Update status report malformed: {0} bytes")]
    UpdateStatusMalformed(usize),
    #[error("Unexpected update status command: {0:?} (expected {1:?})")]
    UnexpectedUpdateStatusCommand(UpdateCommand, UpdateCommand),
    #[error("Timed out waiting for {0:?}")]
    UpdateTimeout(UpdateCommand),
}
pub type Result<T> = std::result::Result<T, AppError>;
