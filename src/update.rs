use crate::{
    error::{AppError, Result},
    hid::DualSenseHid,
    protocol::{UpdateCommand, UpdateStatus},
};
use indicatif::{ProgressBar, ProgressStyle};
use std::io::IsTerminal;
use std::{
    thread,
    time::{Duration, Instant},
};

pub trait Transport {
    fn send(&self, command: UpdateCommand, data: &[u8]) -> Result<()>;
    fn status(&self) -> Result<UpdateStatus>;
}
impl Transport for DualSenseHid {
    fn send(&self, command: UpdateCommand, data: &[u8]) -> Result<()> {
        self.send_update_command(command, data)
    }
    fn status(&self) -> Result<UpdateStatus> {
        self.get_update_status(4)
    }
}

pub struct DualSenseUpdater<T: Transport> {
    dev: T,
    timeout: Duration,
}
impl<T: Transport> DualSenseUpdater<T> {
    pub fn new(dev: T) -> Self {
        Self {
            dev,
            timeout: Duration::from_secs(30),
        }
    }
    pub fn flash(&self, data: &[u8]) -> Result<()> {
        if data.len() != crate::firmware::FIRMWARE_SIZE {
            return Err(AppError::Validation(
                "Invalid image size before StartUpdate".into(),
            ));
        }
        self.dev.send(UpdateCommand::StartUpdate, &data[..256])?;
        self.wait(UpdateCommand::StartUpdate)?;
        println!("Header accepted.");
        let progress = ProgressBar::new(data.len() as u64);
        progress.set_style(
            ProgressStyle::with_template(
                "  {bar:32.cyan/blue} {percent:>3}%  {bytes}/{total_bytes}",
            )
            .unwrap()
            .progress_chars("━╸─"),
        );
        let transfer = (|| -> Result<()> {
            let mut sent = 0;
            for (idx, block) in data.chunks(0x8000).enumerate() {
                for packet in block.chunks(57) {
                    self.dev.send(UpdateCommand::WriteUpdateImage, packet)?;
                    self.wait(UpdateCommand::WriteUpdateImage)?;
                    sent += packet.len();
                    progress.set_position(sent as u64);
                    thread::sleep(Duration::from_millis(10));
                }
                if !std::io::stdout().is_terminal() {
                    println!(
                        "Writing: {}/{} blocks",
                        idx + 1,
                        data.len().div_ceil(0x8000)
                    );
                }
            }
            Ok(())
        })();
        if let Err(error) = transfer {
            progress.abandon_with_message(
                "Transfer stopped. Read controller firmware before retrying.",
            );
            return Err(error);
        }
        progress.finish_and_clear();
        println!("Transfer complete. Verifying image...");
        self.dev.send(UpdateCommand::VerifyUpdateImage, &[])?;
        self.wait(UpdateCommand::VerifyUpdateImage)?;
        println!("Image verification accepted.");
        self.dev.send(UpdateCommand::FinalizeUpdate, &[])?;
        println!("Finalization sent; checking installed firmware.");
        Ok(())
    }

    fn wait(&self, command: UpdateCommand) -> Result<()> {
        let deadline = Instant::now() + self.timeout;
        loop {
            let status = self.dev.status()?;
            if status.command != command {
                return Err(AppError::UnexpectedUpdateStatusCommand(
                    status.command,
                    command,
                ));
            }
            match (command, status.status_raw) {
                (_, 0x00) | (UpdateCommand::WriteUpdateImage, 0x03) => return Ok(()),
                (UpdateCommand::StartUpdate, 0x04 | 0x10)
                | (UpdateCommand::WriteUpdateImage, 0x01 | 0x10)
                | (UpdateCommand::VerifyUpdateImage, 0x10) => {}
                _ => {
                    return Err(AppError::Validation(format!(
                        "Controller rejected {command:?} with status 0x{:02X}",
                        status.status_raw
                    )));
                }
            }
            if Instant::now() >= deadline {
                return Err(AppError::UpdateTimeout(command));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    struct Fake {
        command: UpdateCommand,
        status: u8,
        writes: Cell<usize>,
    }
    impl Transport for Fake {
        fn send(&self, _: UpdateCommand, _: &[u8]) -> Result<()> {
            self.writes.set(self.writes.get() + 1);
            Ok(())
        }
        fn status(&self) -> Result<UpdateStatus> {
            Ok(UpdateStatus {
                report_id: 0xf5,
                command: self.command,
                status_raw: self.status,
                raw: vec![],
            })
        }
    }
    fn fake(command: UpdateCommand, status: u8) -> DualSenseUpdater<Fake> {
        let mut u = DualSenseUpdater::new(Fake {
            command,
            status,
            writes: Cell::new(0),
        });
        u.timeout = Duration::ZERO;
        u
    }
    #[test]
    fn retry_is_never_success_and_cannot_hang() {
        for (c, s) in [
            (UpdateCommand::StartUpdate, 0x10),
            (UpdateCommand::StartUpdate, 0x04),
            (UpdateCommand::WriteUpdateImage, 1),
            (UpdateCommand::VerifyUpdateImage, 0x10),
        ] {
            assert!(matches!(
                fake(c, s).wait(c),
                Err(AppError::UpdateTimeout(_))
            ));
        }
    }
    #[test]
    fn errors_and_wrong_phases_fail() {
        assert!(
            fake(UpdateCommand::StartUpdate, 1)
                .wait(UpdateCommand::StartUpdate)
                .is_err()
        );
        assert!(
            fake(UpdateCommand::StartUpdate, 0)
                .wait(UpdateCommand::WriteUpdateImage)
                .is_err()
        );
        assert!(
            fake(UpdateCommand::WriteUpdateImage, 3)
                .wait(UpdateCommand::WriteUpdateImage)
                .is_ok()
        );
    }
    #[test]
    fn invalid_image_never_sends_start() {
        let u = fake(UpdateCommand::StartUpdate, 0);
        assert!(u.flash(&[0; 256]).is_err());
        assert_eq!(u.dev.writes.get(), 0);
    }
}
