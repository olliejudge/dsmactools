//! Firmware installation. This module is the only code that can send firmware-update reports.
//!
//! [`install`] is the single entry point. In order, it re-reads the open controller and refuses
//! if it no longer matches the validated image, requires exactly one USB controller and enough
//! charge, writes the complete Start → Write → Verify → Finalize sequence with bounded polling,
//! closes the handle, and verifies the installed version on a new handle. Nothing here retries
//! or reflashes after an error.

// An install must never panic on output: after the terminal window closes, writes fail and
// `println!` would unwind in the middle of the sequence. Use `say!` and `alert!` instead.
#![deny(clippy::print_stdout, clippy::print_stderr)]

use crate::{
    error::{AppError, Result},
    firmware::{ControllerIdentity, FIRMWARE_SIZE, FirmwareImage},
    hid::{self, Controller, DualSenseHid},
    input::{Cancellation, StopSignal},
    protocol::{self, FirmwareInfo, UpdateCommand},
};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::{
    cell::Cell,
    io::{IsTerminal, Write},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const REPORT_ID_UPDATE_COMMAND: u8 = 0xF4;
const REPORT_ID_UPDATE_STATUS: u8 = 0xF5;
const REPORT_LEN: usize = 64;
const STATUS_LEN: usize = 4;
/// Payload bytes per update report after its `[0xF4, command, length]` header.
const MAX_CHUNK: usize = 0x39;
/// Image bytes per write block. Packets never span two blocks.
const BLOCK_SIZE: usize = 0x8000;
/// StartUpdate carries the first 256 bytes of the image.
const HEADER_LEN: usize = 256;
const PHASE_TIMEOUT: Duration = Duration::from_secs(30);
const STATUS_POLL: Duration = Duration::from_millis(10);
const PACKET_DELAY: Duration = Duration::from_millis(10);
const START_HEADER_DELAY: Duration = Duration::from_millis(50);
const VERIFY_TIMEOUT: Duration = Duration::from_secs(30);
const VERIFY_POLL: Duration = Duration::from_millis(500);
/// Rough time per write packet, used only for the stop warning before the transfer has a
/// measured rate: the 10 ms pacing delay plus an assumed 10 ms of USB round trips.
const ESTIMATED_PACKET_TIME: Duration = Duration::from_millis(20);

// Unit tests discard install output, which `println!` capture used to hide.
#[cfg(test)]
use std::io::{sink as error_stream, sink as output_stream};
#[cfg(not(test))]
use std::io::{stderr as error_stream, stdout as output_stream};

/// Print a line to stdout, ignoring write errors (a closed terminal or pipe).
macro_rules! say {
    ($($arg:tt)*) => {{
        let _ = writeln!(output_stream(), $($arg)*);
    }};
}

/// Print a line to stderr, ignoring write errors (a closed terminal or pipe).
macro_rules! alert {
    ($($arg:tt)*) => {{
        let _ = writeln!(error_stream(), $($arg)*);
    }};
}

/// Proof of being inside this module. Its field is private, so only this module can create one,
/// and the HID layer requires it for firmware-update feature reports.
pub(crate) struct UpdateAccess(());

/// The result of an install whose new version was read back on a new handle.
#[derive(Debug)]
pub enum InstallOutcome {
    /// Every phase succeeded and the controller reports the new version.
    Verified(FirmwareInfo),
    /// A phase reported an error, but the controller rebooted and reports the new version.
    /// This remains a successful install; the error is shown to the user.
    VerifiedDespiteProtocolError {
        installed: FirmwareInfo,
        error: AppError,
    },
}

impl InstallOutcome {
    pub fn message(&self) -> String {
        match self {
            Self::Verified(current) => format!(
                "Verified installed firmware: 0x{:04X} ({} {}).",
                current.firmware_version, current.build_date, current.build_time
            ),
            Self::VerifiedDespiteProtocolError { installed, error } => format!(
                "Verified installed firmware: 0x{:04X}. Controller rebooted/committed despite a protocol error: {error}",
                installed.firmware_version
            ),
        }
    }
}

/// The result of [`install`], and the stop request (if any) held back while it ran.
#[derive(Debug)]
pub struct Installation {
    pub result: Result<InstallOutcome>,
    /// Set when Ctrl-C, SIGTERM or SIGHUP arrived during the install. The install still ran to
    /// its end; the caller reports `result` and then exits instead of carrying on.
    pub stop_requested: Option<StopSignal>,
}

/// Install a validated image on the open controller at `path`.
///
/// The caller's confirmation may have taken any amount of time, so the controller is read again
/// here before anything is written. Errors before the first write mean nothing was written.
pub fn install(dev: DualSenseHid, path: &str, image: &FirmwareImage) -> Installation {
    install_with(dev, path, image, &SystemHost)
}

/// Refuse an update unless exactly one USB DualSense/Edge is connected and it is `path`.
/// [`install`] repeats this check; calling it first avoids asking for a doomed confirmation.
pub fn require_single_controller(path: &str) -> Result<()> {
    single_usb_controller(&hid::controllers()?, path).map(|_| ())
}

fn install_with<D: Device, H: Host>(
    dev: D,
    path: &str,
    image: &FirmwareImage,
    host: &H,
) -> Installation {
    let mut stop_requested = None;
    let result = run_install(dev, path, image, host, &mut stop_requested);
    Installation {
        result,
        stop_requested,
    }
}

fn run_install<D: Device, H: Host>(
    dev: D,
    path: &str,
    image: &FirmwareImage,
    host: &H,
    stop_requested: &mut Option<StopSignal>,
) -> Result<InstallOutcome> {
    let _awake = host.keep_awake();
    let validated = image.validated_for();
    let info = dev.firmware_info()?;
    let connected = host.controllers()?;
    let controller = single_usb_controller(&connected, path)?;
    check_identity(&validated, &ControllerIdentity::of(&info, controller.pid))?;
    protocol::require_update_battery(dev.battery()?)?;
    let signals = host.defer_signals()?;
    // The updater owns the handle and is dropped at the end of this statement.
    let flashed =
        DualSenseUpdater::new(dev, host, &signals, host.interactive_progress()).flash(image);
    // The controller may re-enumerate with a new HID path; the old handle is closed.
    say!(
        "Checking installed firmware; waiting up to 30 seconds for the controller to reconnect..."
    );
    let verified = verify_installed(host, &signals, &validated, image.version());
    *stop_requested = signals.stop_signal();
    conclude(flashed, verified)
}

/// Combine the write sequence and the installed-version check. Only the new handle's version
/// proves success; a transfer status alone never does.
fn conclude(flashed: Result<()>, verified: Result<FirmwareInfo>) -> Result<InstallOutcome> {
    match (flashed, verified) {
        (Ok(()), Ok(current)) => Ok(InstallOutcome::Verified(current)),
        (Err(error), Ok(installed)) => {
            Ok(InstallOutcome::VerifiedDespiteProtocolError { installed, error })
        }
        (Err(phase_error), Err(_)) => Err(AppError::Validation(format!(
            "{phase_error}. Update not verified; reconnect and run --print-firmware-info before any retry"
        ))),
        (Ok(()), Err(error)) => Err(error),
    }
}

fn check_identity(validated: &ControllerIdentity, current: &ControllerIdentity) -> Result<()> {
    if validated == current {
        return Ok(());
    }
    let describe = |c: &ControllerIdentity| {
        format!(
            "PID {:04x}, series {:04X}, hardware 0x{:08X}, firmware 0x{:04X}",
            c.pid, c.software_series, c.hardware_info, c.firmware_version
        )
    };
    Err(AppError::Validation(format!(
        "Controller changed after the firmware check (expected {}; found {}); nothing was written. Check it again before updating",
        describe(validated),
        describe(current)
    )))
}

fn single_usb_controller<'a>(devices: &'a [Controller], path: &str) -> Result<&'a Controller> {
    match devices {
        [only] if only.path == path => Ok(only),
        [] => Err(AppError::Validation(
            "The controller is no longer connected over USB; nothing was written".into(),
        )),
        [_] => Err(AppError::Validation(
            "The connected controller is not the one that was checked; nothing was written. Check it again before updating".into(),
        )),
        _ => Err(AppError::Validation(
            "Disconnect other controllers before updating firmware".into(),
        )),
    }
}

/// Poll new handles until the flashed controller reports the expected version, for at most
/// [`VERIFY_TIMEOUT`].
fn verify_installed<H: Host>(
    host: &H,
    signals: &SignalDeferral,
    expected: &ControllerIdentity,
    version: u16,
) -> Result<FirmwareInfo> {
    let deadline = host.now() + VERIFY_TIMEOUT;
    loop {
        host.sleep(VERIFY_POLL);
        if let Some(warning) = signals.warning(Stage::Checking) {
            alert!("{warning}");
        }
        if let Ok(devices) = host.controllers()
            && let Some(controller) = sole_controller(&devices, expected.pid)
            && let Ok(info) = host.read_new_handle(controller)
            && is_expected_install(&info, expected, version)
        {
            return Ok(info);
        }
        if host.now() >= deadline {
            return Err(AppError::Validation(format!(
                "Transfer finished but installed firmware 0x{version:04X} could not be verified. Reconnect and run --print-firmware-info; do not assume the update succeeded"
            )));
        }
    }
}

/// The controller to verify after re-enumeration. With several connected, a different
/// controller already on the new version could be mistaken for the flashed one, so verification
/// only reads a controller when it is the only USB DualSense/Edge and has the flashed PID.
fn sole_controller(devices: &[Controller], pid: u16) -> Option<&Controller> {
    match devices {
        [only] if only.pid == pid => Some(only),
        _ => None,
    }
}

fn is_expected_install(info: &FirmwareInfo, expected: &ControllerIdentity, version: u16) -> bool {
    info.software_series == expected.software_series
        && info.hardware_info == expected.hardware_info
        && info.firmware_version == version
}

/// A parsed 0xF5 update status report.
#[derive(Debug, Clone, Copy)]
struct UpdateStatus {
    command: UpdateCommand,
    status_raw: u8,
}

/// Parse a status report: exactly `[0xF5, command, status, _]`.
fn parse_status(raw: &[u8]) -> Result<UpdateStatus> {
    if raw.is_empty() {
        return Err(AppError::UpdateStatusEmpty);
    }
    if raw[0] != REPORT_ID_UPDATE_STATUS || raw.len() != STATUS_LEN {
        return Err(AppError::UpdateStatusMalformed(raw.len()));
    }
    Ok(UpdateStatus {
        command: UpdateCommand::from_int(raw[1]),
        status_raw: raw[2],
    })
}

/// Frame an update command as padded 64-byte reports `[0xF4, command, length, payload…]`, each
/// carrying at most 57 payload bytes. An empty payload is sent as one zero-length report.
fn frames(command: UpdateCommand, payload: &[u8]) -> Vec<[u8; REPORT_LEN]> {
    let chunks: Vec<&[u8]> = if payload.is_empty() {
        vec![&[]]
    } else {
        payload.chunks(MAX_CHUNK).collect()
    };
    chunks
        .into_iter()
        .map(|chunk| {
            let mut report = [0u8; REPORT_LEN];
            report[..3].copy_from_slice(&[
                REPORT_ID_UPDATE_COMMAND,
                command as u8,
                chunk.len() as u8,
            ]);
            report[3..3 + chunk.len()].copy_from_slice(chunk);
            report
        })
        .collect()
}

/// How a status report answers the command that was just sent.
#[derive(Debug)]
enum Verdict {
    Done,
    /// Still working; poll again until the phase timeout.
    Pending,
    Rejected(AppError),
}

/// Phase-specific status meanings. A retry/busy status is never treated as success.
fn classify(command: UpdateCommand, status: &UpdateStatus) -> Verdict {
    if status.command != command {
        return Verdict::Rejected(AppError::UnexpectedUpdateStatusCommand(
            status.command,
            command,
        ));
    }
    match (command, status.status_raw) {
        (_, 0x00) | (UpdateCommand::WriteUpdateImage, 0x03) => Verdict::Done,
        (UpdateCommand::StartUpdate, 0x04 | 0x10)
        | (UpdateCommand::WriteUpdateImage, 0x01 | 0x10)
        | (UpdateCommand::VerifyUpdateImage, 0x10) => Verdict::Pending,
        _ => Verdict::Rejected(AppError::Validation(format!(
            "Controller rejected {command:?} with status 0x{:02X}",
            status.status_raw
        ))),
    }
}

/// Raw firmware-update reports on one open controller.
trait Transport {
    fn send_report(&self, report: &[u8; REPORT_LEN]) -> Result<()>;
    fn read_status_report(&self) -> Result<Vec<u8>>;
}

/// The open controller as the install preflight sees it.
trait Device: Transport {
    fn firmware_info(&self) -> Result<FirmwareInfo>;
    fn battery(&self) -> Result<u8>;
}

impl Transport for DualSenseHid {
    fn send_report(&self, report: &[u8; REPORT_LEN]) -> Result<()> {
        self.send_update_report(&UpdateAccess(()), report)
    }
    fn read_status_report(&self) -> Result<Vec<u8>> {
        self.read_update_report(&UpdateAccess(()), REPORT_ID_UPDATE_STATUS, STATUS_LEN)
    }
}

impl Device for DualSenseHid {
    fn firmware_info(&self) -> Result<FirmwareInfo> {
        self.get_firmware_info()
    }
    fn battery(&self) -> Result<u8> {
        DualSenseHid::battery(self)
    }
}

/// Time for polling and pacing, so tests can run the full sequence on a virtual clock.
trait Pacer {
    fn now(&self) -> Instant;
    fn sleep(&self, duration: Duration);
}

/// What an install needs around the open controller.
trait Host: Pacer {
    /// Every USB DualSense/Edge currently enumerated.
    fn controllers(&self) -> Result<Vec<Controller>>;
    /// Open a new handle to `controller` and read its firmware information.
    fn read_new_handle(&self, controller: &Controller) -> Result<FirmwareInfo>;
    /// Defer SIGINT/SIGTERM/SIGHUP until the returned value is dropped.
    fn defer_signals(&self) -> Result<SignalDeferral>;
    /// Keep the Mac from idle-sleeping until the returned value is dropped.
    fn keep_awake(&self) -> KeepAwake;
    /// Draw an animated progress bar rather than one line per block.
    fn interactive_progress(&self) -> bool;
}

struct SystemHost;

impl Pacer for SystemHost {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn sleep(&self, duration: Duration) {
        thread::sleep(duration);
    }
}

impl Host for SystemHost {
    fn controllers(&self) -> Result<Vec<Controller>> {
        hid::controllers()
    }
    fn read_new_handle(&self, controller: &Controller) -> Result<FirmwareInfo> {
        DualSenseHid::open(0x054c, controller.pid, &controller.path)?.get_firmware_info()
    }
    fn defer_signals(&self) -> Result<SignalDeferral> {
        SignalDeferral::process()
    }
    fn keep_awake(&self) -> KeepAwake {
        KeepAwake::start()
    }
    fn interactive_progress(&self) -> bool {
        std::io::stdout().is_terminal()
            && std::io::stderr().is_terminal()
            && std::env::var("TERM").as_deref() != Ok("dumb")
    }
}

/// Holds Ctrl-C, SIGTERM and SIGHUP (the terminal window closing) while firmware is written and
/// checked, so they cannot stop the process in the middle of an install. The first request
/// produces one warning and the update carries on; it is never aborted. Dropping this restores
/// default signal behaviour.
struct SignalDeferral {
    requested: Box<dyn Fn() -> Option<StopSignal>>,
    warned: Cell<Option<Stage>>,
}

/// Where an install is when a stop is requested.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Stage {
    /// The update sequence is running; `remaining` estimates the time to the end of the check.
    Writing { remaining: Duration },
    /// The installed-version check on a new handle.
    Checking,
}

impl SignalDeferral {
    /// Use the process-wide registration shared with monitor, record and feedback.
    fn process() -> Result<Self> {
        let cancellation = Cancellation::including_hangup()?;
        Ok(Self::with_signal(move || cancellation.stop_signal()))
    }

    /// A deferral whose stop request, once true, counts as Ctrl-C.
    #[cfg(test)]
    fn new(requested: impl Fn() -> bool + 'static) -> Self {
        Self::with_signal(move || requested().then_some(StopSignal::Interrupt))
    }

    fn with_signal(requested: impl Fn() -> Option<StopSignal> + 'static) -> Self {
        Self {
            requested: Box::new(requested),
            warned: Cell::new(None),
        }
    }

    /// The signal that asked to stop, if one has arrived.
    fn stop_signal(&self) -> Option<StopSignal> {
        (self.requested)()
    }

    /// The warning to show the first time a stop has been requested.
    fn warning(&self, stage: Stage) -> Option<String> {
        if self.warned.get().is_some() || self.stop_signal().is_none() {
            return None;
        }
        self.warned.set(Some(stage));
        Some(stop_warning(stage))
    }
}

fn stop_warning(stage: Stage) -> String {
    let Stage::Writing { remaining } = stage else {
        return "Checking the installed firmware. Leave the controller connected; this takes up to 30 seconds.".into();
    };
    let minutes = remaining.as_secs().div_ceil(60).max(1);
    let unit = if minutes == 1 { "minute" } else { "minutes" };
    format!(
        "Firmware is being written. Stopping now could leave the controller unusable; the update will finish in about {minutes} {unit}."
    )
}

/// An idle-sleep assertion held by `/usr/bin/caffeinate -i -w <this process>` until dropped.
/// `-w` also releases it if this process exits without running the destructor.
struct KeepAwake(Option<Child>);

impl KeepAwake {
    fn start() -> Self {
        let mut command = Command::new("/usr/bin/caffeinate");
        command.args(["-i", "-w", &std::process::id().to_string()]);
        Self::spawn(command)
    }

    fn spawn(mut command: Command) -> Self {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // Its own process group keeps Ctrl-C in the terminal from ending the assertion.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        match command.spawn() {
            Ok(child) => Self(Some(child)),
            Err(error) => {
                alert!("{}", keep_awake_warning(&error));
                Self(None)
            }
        }
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn keep_awake_warning(error: &std::io::Error) -> String {
    format!("Could not keep the Mac awake ({error}); keep it awake until the update finishes.")
}

/// Runs the update sequence on one open controller.
struct DualSenseUpdater<'a, T: Transport, P: Pacer> {
    dev: T,
    pacer: &'a P,
    signals: &'a SignalDeferral,
    timeout: Duration,
    interactive: bool,
    progress: ProgressBar,
    sent: Cell<usize>,
    transfer_started: Cell<Option<Instant>>,
}

impl<'a, T: Transport, P: Pacer> DualSenseUpdater<'a, T, P> {
    fn new(dev: T, pacer: &'a P, signals: &'a SignalDeferral, interactive: bool) -> Self {
        Self {
            dev,
            pacer,
            signals,
            timeout: PHASE_TIMEOUT,
            interactive,
            progress: ProgressBar::hidden(),
            sent: Cell::new(0),
            transfer_started: Cell::new(None),
        }
    }

    fn flash(&self, image: &FirmwareImage) -> Result<()> {
        let data = image.data();
        if data.len() != FIRMWARE_SIZE {
            return Err(AppError::Validation(
                "Invalid image size before StartUpdate".into(),
            ));
        }
        say!("Preparing controller for firmware installation...");
        self.send(UpdateCommand::StartUpdate, &data[..HEADER_LEN])?;
        self.wait(UpdateCommand::StartUpdate)?;
        say!(
            "Writing firmware to controller over USB. Keep it connected until verification finishes."
        );
        let progress = &self.progress;
        progress.set_length(data.len() as u64);
        progress.set_style(
            ProgressStyle::with_template(
                "  {bar:24.cyan/blue} {percent:>3}%  {bytes}/{total_bytes}\n  Elapsed: {elapsed_precise} | Est. transfer remaining: {eta_precise}",
            )
            .unwrap()
            .progress_chars("━╸─"),
        );
        if self.interactive {
            progress.set_draw_target(ProgressDrawTarget::stderr());
            progress.reset_elapsed();
            progress.reset_eta();
        }
        let transfer_started = self.pacer.now();
        self.transfer_started.set(Some(transfer_started));
        let transfer = (|| -> Result<()> {
            let blocks = data.len().div_ceil(BLOCK_SIZE);
            for (idx, block) in data.chunks(BLOCK_SIZE).enumerate() {
                for packet in block.chunks(MAX_CHUNK) {
                    self.send(UpdateCommand::WriteUpdateImage, packet)?;
                    self.wait(UpdateCommand::WriteUpdateImage)?;
                    let sent = self.sent.get() + packet.len();
                    self.sent.set(sent);
                    progress.set_position(sent as u64);
                    self.pacer.sleep(PACKET_DELAY);
                }
                if !self.interactive {
                    say!(
                        "Writing: {}/{blocks} blocks | Elapsed: {}s",
                        idx + 1,
                        (self.pacer.now() - transfer_started).as_secs()
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
        say!(
            "USB transfer complete in {:.1}s. Verifying firmware on controller...",
            (self.pacer.now() - transfer_started).as_secs_f64()
        );
        self.send(UpdateCommand::VerifyUpdateImage, &[])?;
        self.wait(UpdateCommand::VerifyUpdateImage)?;
        say!("Image verification accepted.");
        self.send(UpdateCommand::FinalizeUpdate, &[])?;
        say!("Finalization sent. Keep the controller connected for the installed-version check.");
        Ok(())
    }

    fn send(&self, command: UpdateCommand, payload: &[u8]) -> Result<()> {
        for (index, report) in frames(command, payload).iter().enumerate() {
            let len = usize::from(report[2]);
            log::debug!(
                "F4 chunk off={} len={len} first4={}",
                index * MAX_CHUNK,
                hex(&report[3..3 + len.min(4)])
            );
            self.dev.send_report(report)?;
            if command == UpdateCommand::StartUpdate && index == 0 {
                self.pacer.sleep(START_HEADER_DELAY);
            }
        }
        Ok(())
    }

    fn wait(&self, command: UpdateCommand) -> Result<()> {
        let deadline = self.pacer.now() + self.timeout;
        loop {
            self.warn_if_stop_requested();
            let raw = self.dev.read_status_report()?;
            log::debug!("F5 status raw: {}", hex(&raw));
            match classify(command, &parse_status(&raw)?) {
                Verdict::Done => return Ok(()),
                Verdict::Pending => {}
                Verdict::Rejected(error) => return Err(error),
            }
            if self.pacer.now() >= deadline {
                return Err(AppError::UpdateTimeout(command));
            }
            self.pacer.sleep(STATUS_POLL);
        }
    }

    fn warn_if_stop_requested(&self) {
        if let Some(warning) = self.signals.warning(Stage::Writing {
            remaining: self.remaining(),
        }) {
            self.progress.suspend(|| alert!("{warning}"));
        }
    }

    fn remaining(&self) -> Duration {
        let elapsed = self
            .transfer_started
            .get()
            .map(|started| self.pacer.now() - started);
        estimate_remaining(elapsed, self.sent.get())
    }
}

/// Estimated time until the installed-version check finishes, from the transfer rate so far.
/// Before any image bytes are written, assume [`ESTIMATED_PACKET_TIME`] per packet.
fn estimate_remaining(transfer_elapsed: Option<Duration>, sent: usize) -> Duration {
    let transfer = match transfer_elapsed {
        Some(elapsed) if sent > 0 => {
            elapsed.mul_f64(FIRMWARE_SIZE.saturating_sub(sent) as f64 / sent as f64)
        }
        _ => ESTIMATED_PACKET_TIME * write_packet_count(FIRMWARE_SIZE) as u32,
    };
    transfer + VERIFY_TIMEOUT
}

/// Write packets for an image of `len` bytes: each block is split separately.
fn write_packet_count(len: usize) -> usize {
    let full_blocks = len / BLOCK_SIZE;
    full_blocks * BLOCK_SIZE.div_ceil(MAX_CHUNK) + (len % BLOCK_SIZE).div_ceil(MAX_CHUNK)
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::firmware::sample_image;
    use std::{cell::RefCell, rc::Rc};

    const PID: u16 = 0x0ce6;
    const SERIES: u16 = 0x000b;
    const HARDWARE: u32 = 0x1107;
    const INSTALLED: u16 = 0x0580;
    const NEW: u16 = 0x0630;
    const PATH: &str = "DevSrvsID:1";

    fn info(series: u16, hardware: u32, version: u16) -> FirmwareInfo {
        let mut r = vec![0; 64];
        r[0] = 0x20;
        r[1..12].copy_from_slice(b"Jul  4 2025");
        r[12..20].copy_from_slice(b"10:38:40");
        r[20] = 2;
        r[22..24].copy_from_slice(&series.to_le_bytes());
        r[24..28].copy_from_slice(&hardware.to_le_bytes());
        r[44..46].copy_from_slice(&version.to_le_bytes());
        FirmwareInfo::parse(r).unwrap()
    }

    fn image() -> FirmwareImage {
        FirmwareImage::validate(
            sample_image(PID, SERIES, NEW),
            &info(SERIES, HARDWARE, INSTALLED),
            PID,
            Some(NEW),
        )
        .unwrap()
    }

    fn usb(path: &str, pid: u16) -> Controller {
        Controller {
            path: path.into(),
            pid,
            name: "DualSense".into(),
        }
    }

    fn status(command: UpdateCommand, code: u8) -> Result<Vec<u8>> {
        Ok(vec![REPORT_ID_UPDATE_STATUS, command as u8, code, 0])
    }

    /// Status script: (last command sent, status reads since that send) -> status report.
    type Script = Box<dyn Fn(UpdateCommand, usize) -> Result<Vec<u8>>>;

    /// A scripted controller that records every update report it receives.
    struct FakeController {
        info: FirmwareInfo,
        battery: u8,
        reports: RefCell<Vec<[u8; REPORT_LEN]>>,
        sent: Rc<Cell<usize>>,
        status_reads: Cell<usize>,
        since_send: Cell<usize>,
        fail_send_at: Option<usize>,
        script: Script,
        closed: Rc<Cell<bool>>,
    }

    impl FakeController {
        fn new() -> Self {
            Self {
                info: info(SERIES, HARDWARE, INSTALLED),
                battery: 100,
                reports: RefCell::new(Vec::new()),
                sent: Rc::new(Cell::new(0)),
                status_reads: Cell::new(0),
                since_send: Cell::new(0),
                fail_send_at: None,
                script: Box::new(|command, _| status(command, 0x00)),
                closed: Rc::new(Cell::new(false)),
            }
        }
        fn with_script(script: impl Fn(UpdateCommand, usize) -> Result<Vec<u8>> + 'static) -> Self {
            Self {
                script: Box::new(script),
                ..Self::new()
            }
        }
        fn commands(&self) -> Vec<UpdateCommand> {
            self.reports
                .borrow()
                .iter()
                .map(|r| UpdateCommand::from_int(r[1]))
                .collect()
        }
        fn count(&self, command: UpdateCommand) -> usize {
            self.commands().iter().filter(|c| **c == command).count()
        }
    }

    /// The open handle. Dropping it marks the fake controller's handle as closed.
    struct Handle<'a>(&'a FakeController);

    impl Drop for Handle<'_> {
        fn drop(&mut self) {
            self.0.closed.set(true);
        }
    }

    impl Transport for Handle<'_> {
        fn send_report(&self, report: &[u8; REPORT_LEN]) -> Result<()> {
            let fake = self.0;
            assert!(!fake.closed.get());
            if fake.fail_send_at == Some(fake.sent.get()) {
                return Err(AppError::Io(std::io::Error::other("USB write failed")));
            }
            fake.reports.borrow_mut().push(*report);
            fake.sent.set(fake.sent.get() + 1);
            fake.since_send.set(0);
            Ok(())
        }
        fn read_status_report(&self) -> Result<Vec<u8>> {
            let fake = self.0;
            let last = fake
                .reports
                .borrow()
                .last()
                .map_or(UpdateCommand::Unknown, |r| UpdateCommand::from_int(r[1]));
            let nth = fake.since_send.get();
            fake.since_send.set(nth + 1);
            fake.status_reads.set(fake.status_reads.get() + 1);
            (fake.script)(last, nth)
        }
    }

    impl Device for Handle<'_> {
        fn firmware_info(&self) -> Result<FirmwareInfo> {
            Ok(self.0.info.clone())
        }
        fn battery(&self) -> Result<u8> {
            Ok(self.0.battery)
        }
    }

    /// A virtual clock: sleeping advances time instantly.
    struct VirtualClock {
        origin: Instant,
        elapsed: Cell<Duration>,
        sleeps: RefCell<Vec<Duration>>,
    }

    impl VirtualClock {
        fn new() -> Self {
            Self {
                origin: Instant::now(),
                elapsed: Cell::new(Duration::ZERO),
                sleeps: RefCell::new(Vec::new()),
            }
        }
    }

    impl Pacer for VirtualClock {
        fn now(&self) -> Instant {
            self.origin + self.elapsed.get()
        }
        fn sleep(&self, duration: Duration) {
            self.elapsed.set(self.elapsed.get() + duration);
            self.sleeps.borrow_mut().push(duration);
        }
    }

    type Enumeration = Box<dyn Fn(usize) -> Vec<Controller>>;
    type NewHandle = Box<dyn Fn(usize, &Controller) -> Result<FirmwareInfo>>;

    /// Scripted enumeration and new-handle reads on a virtual clock.
    struct FakeHost {
        clock: VirtualClock,
        enumerate: Enumeration,
        open: NewHandle,
        enumerations: Cell<usize>,
        reads: Cell<usize>,
        stop_requested: Rc<Cell<bool>>,
        deferred: Cell<bool>,
        /// Use the real process-wide signal deferral rather than `stop_requested`.
        process_signals: bool,
        /// Set by an install: the original handle must be closed before a new one is opened.
        original_closed: RefCell<Option<Rc<Cell<bool>>>>,
    }

    impl FakeHost {
        fn new(
            enumerate: impl Fn(usize) -> Vec<Controller> + 'static,
            open: impl Fn(usize, &Controller) -> Result<FirmwareInfo> + 'static,
        ) -> Self {
            Self {
                clock: VirtualClock::new(),
                enumerate: Box::new(enumerate),
                open: Box::new(open),
                enumerations: Cell::new(0),
                reads: Cell::new(0),
                stop_requested: Rc::new(Cell::new(false)),
                deferred: Cell::new(false),
                process_signals: false,
                original_closed: RefCell::new(None),
            }
        }
        /// One USB controller at `PATH` that reports the new version once reopened.
        fn updated() -> Self {
            Self::new(
                |_| vec![usb(PATH, PID)],
                |_, _| Ok(info(SERIES, HARDWARE, NEW)),
            )
        }
    }

    impl Pacer for FakeHost {
        fn now(&self) -> Instant {
            self.clock.now()
        }
        fn sleep(&self, duration: Duration) {
            self.clock.sleep(duration);
        }
    }

    impl Host for FakeHost {
        fn controllers(&self) -> Result<Vec<Controller>> {
            let n = self.enumerations.get();
            self.enumerations.set(n + 1);
            Ok((self.enumerate)(n))
        }
        fn read_new_handle(&self, controller: &Controller) -> Result<FirmwareInfo> {
            if let Some(closed) = self.original_closed.borrow().as_ref() {
                assert!(closed.get(), "original handle still open");
            }
            let n = self.reads.get();
            self.reads.set(n + 1);
            (self.open)(n, controller)
        }
        fn defer_signals(&self) -> Result<SignalDeferral> {
            self.deferred.set(true);
            if self.process_signals {
                return SignalDeferral::process();
            }
            let requested = self.stop_requested.clone();
            Ok(SignalDeferral::new(move || requested.get()))
        }
        fn keep_awake(&self) -> KeepAwake {
            KeepAwake(None)
        }
        fn interactive_progress(&self) -> bool {
            false
        }
    }

    fn quiet_signals() -> SignalDeferral {
        SignalDeferral::new(|| false)
    }

    fn flash_with(fake: &FakeController, clock: &VirtualClock) -> Result<()> {
        let signals = quiet_signals();
        DualSenseUpdater::new(Handle(fake), clock, &signals, false).flash(&image())
    }

    fn install_on(fake: &FakeController, host: &FakeHost) -> Result<InstallOutcome> {
        install_stopping_on(fake, host).result
    }

    fn install_stopping_on(fake: &FakeController, host: &FakeHost) -> Installation {
        *host.original_closed.borrow_mut() = Some(fake.closed.clone());
        install_with(Handle(fake), PATH, &image(), host)
    }

    fn payload(report: &[u8; REPORT_LEN]) -> &[u8] {
        &report[3..3 + usize::from(report[2])]
    }

    // Pure protocol pieces.

    #[test]
    fn status_reports_need_f5_and_exactly_four_bytes() {
        let ok = parse_status(&[0xF5, 0x01, 0x03, 0x00]).unwrap();
        assert_eq!(ok.command, UpdateCommand::WriteUpdateImage);
        assert_eq!(ok.status_raw, 0x03);
        assert!(matches!(
            parse_status(&[]),
            Err(AppError::UpdateStatusEmpty)
        ));
        for raw in [
            &[0xF4, 0x00, 0x00, 0x00][..],
            &[0x20, 0x00, 0x00, 0x00],
            &[0xF5],
            &[0xF5, 0x00, 0x00],
            &[0xF5, 0x00, 0x00, 0x00, 0x00],
            &[0xF5; 64],
        ] {
            assert!(
                matches!(parse_status(raw), Err(AppError::UpdateStatusMalformed(n)) if n == raw.len()),
                "{raw:02x?}"
            );
        }
        let unknown = parse_status(&[0xF5, 0x7f, 0x00, 0x00]).unwrap();
        assert_eq!(unknown.command, UpdateCommand::Unknown);
    }

    #[test]
    fn frames_are_padded_f4_reports_with_at_most_57_payload_bytes() {
        let header: Vec<u8> = (0..=255).collect();
        let reports = frames(UpdateCommand::StartUpdate, &header);
        assert_eq!(
            reports.iter().map(|r| r[2]).collect::<Vec<_>>(),
            [57, 57, 57, 57, 28]
        );
        for report in &reports {
            assert_eq!(report[..2], [0xF4, 0x00]);
            assert!(report[3 + usize::from(report[2])..].iter().all(|b| *b == 0));
        }
        assert_eq!(
            reports
                .iter()
                .flat_map(payload)
                .copied()
                .collect::<Vec<_>>(),
            header
        );
        assert_eq!(reports[1][3], 57);

        let exact = frames(UpdateCommand::WriteUpdateImage, &[0xAA; 57]);
        assert_eq!(exact.len(), 1);
        assert_eq!(exact[0][..3], [0xF4, 0x01, 57]);
        assert_eq!(exact[0][3..60], [0xAA; 57]);
        assert_eq!(exact[0][60..], [0; 4]);
        assert_eq!(frames(UpdateCommand::WriteUpdateImage, &[1; 58]).len(), 2);

        for command in [
            UpdateCommand::VerifyUpdateImage,
            UpdateCommand::FinalizeUpdate,
        ] {
            let empty = frames(command, &[]);
            assert_eq!(empty.len(), 1);
            let mut expected = [0u8; REPORT_LEN];
            expected[..3].copy_from_slice(&[0xF4, command as u8, 0]);
            assert_eq!(empty[0], expected);
        }
    }

    #[test]
    fn classify_uses_phase_specific_status_meanings() {
        use UpdateCommand::*;
        for command in [StartUpdate, WriteUpdateImage, VerifyUpdateImage] {
            for code in 0..=u8::MAX {
                let expected = match (command, code) {
                    (_, 0x00) | (WriteUpdateImage, 0x03) => "done",
                    (StartUpdate, 0x04 | 0x10)
                    | (WriteUpdateImage, 0x01 | 0x10)
                    | (VerifyUpdateImage, 0x10) => "pending",
                    _ => "rejected",
                };
                let verdict = classify(
                    command,
                    &UpdateStatus {
                        command,
                        status_raw: code,
                    },
                );
                let actual = match &verdict {
                    Verdict::Done => "done",
                    Verdict::Pending => "pending",
                    Verdict::Rejected(e) => {
                        assert_eq!(
                            e.to_string(),
                            format!("Controller rejected {command:?} with status 0x{code:02X}")
                        );
                        "rejected"
                    }
                };
                assert_eq!(actual, expected, "{command:?} 0x{code:02x}");
            }
        }
    }

    #[test]
    fn status_for_another_or_unknown_command_is_rejected() {
        use UpdateCommand::*;
        for (reported, expected) in [
            (Unknown, StartUpdate),
            (StartUpdate, WriteUpdateImage),
            (WriteUpdateImage, VerifyUpdateImage),
        ] {
            let verdict = classify(
                expected,
                &UpdateStatus {
                    command: reported,
                    status_raw: 0x00,
                },
            );
            assert!(matches!(
                verdict,
                Verdict::Rejected(AppError::UnexpectedUpdateStatusCommand(r, e))
                    if r == reported && e == expected
            ));
        }
    }

    #[test]
    fn retry_is_never_success_and_cannot_hang() {
        for (c, s) in [
            (UpdateCommand::StartUpdate, 0x10),
            (UpdateCommand::StartUpdate, 0x04),
            (UpdateCommand::WriteUpdateImage, 1),
            (UpdateCommand::VerifyUpdateImage, 0x10),
        ] {
            let fake = FakeController::with_script(move |_, _| status(c, s));
            let clock = VirtualClock::new();
            let signals = quiet_signals();
            let updater = DualSenseUpdater::new(Handle(&fake), &clock, &signals, false);
            assert!(matches!(updater.wait(c), Err(AppError::UpdateTimeout(t)) if t == c));
            // Bounded by the 30 s phase timeout, polled every 10 ms.
            assert!(clock.elapsed.get() >= PHASE_TIMEOUT);
            assert!(clock.elapsed.get() <= PHASE_TIMEOUT + STATUS_POLL);
        }
    }

    #[test]
    fn errors_and_wrong_phases_fail() {
        let wait = |reported: UpdateCommand, code: u8, expected: UpdateCommand| {
            let fake = FakeController::with_script(move |_, _| status(reported, code));
            let clock = VirtualClock::new();
            let signals = quiet_signals();
            DualSenseUpdater::new(Handle(&fake), &clock, &signals, false).wait(expected)
        };
        assert!(wait(UpdateCommand::StartUpdate, 1, UpdateCommand::StartUpdate).is_err());
        assert!(
            wait(
                UpdateCommand::StartUpdate,
                0,
                UpdateCommand::WriteUpdateImage
            )
            .is_err()
        );
        assert!(
            wait(
                UpdateCommand::WriteUpdateImage,
                3,
                UpdateCommand::WriteUpdateImage
            )
            .is_ok()
        );
    }

    // The full sequence on a scripted transport.

    #[test]
    fn full_image_sends_start_writes_verify_and_finalize_in_order() {
        let fake = FakeController::new();
        let clock = VirtualClock::new();
        flash_with(&fake, &clock).unwrap();
        let image = image();
        let reports = fake.reports.borrow();
        let packets = (FIRMWARE_SIZE / BLOCK_SIZE) * BLOCK_SIZE.div_ceil(MAX_CHUNK);
        assert_eq!(FIRMWARE_SIZE % BLOCK_SIZE, 0);
        assert_eq!(write_packet_count(FIRMWARE_SIZE), packets);
        assert_eq!(packets, 16_675);
        assert_eq!(reports.len(), 5 + packets + 2);

        // StartUpdate: the 256-byte header as five reports.
        let start = &reports[..5];
        assert!(
            start
                .iter()
                .all(|r| r[1] == UpdateCommand::StartUpdate as u8)
        );
        let header: Vec<u8> = start.iter().flat_map(payload).copied().collect();
        assert_eq!(header, image.data()[..HEADER_LEN]);

        // WriteUpdateImage: the whole image in order, no packet spanning a 32 KiB block.
        let writes = &reports[5..5 + packets];
        assert!(
            writes
                .iter()
                .all(|r| r[1] == UpdateCommand::WriteUpdateImage as u8)
        );
        let mut offset = 0;
        for report in writes {
            let len = usize::from(report[2]);
            assert!((1..=MAX_CHUNK).contains(&len));
            assert_eq!(offset / BLOCK_SIZE, (offset + len - 1) / BLOCK_SIZE);
            assert_eq!(payload(report), &image.data()[offset..offset + len]);
            offset += len;
        }
        assert_eq!(offset, FIRMWARE_SIZE);

        assert_eq!(
            fake.commands()[5 + packets..],
            [
                UpdateCommand::VerifyUpdateImage,
                UpdateCommand::FinalizeUpdate
            ]
        );
        assert_eq!(payload(&reports[5 + packets]), &[] as &[u8]);
        // One status read each for StartUpdate, every write and VerifyUpdate; none after Finalize.
        assert_eq!(fake.status_reads.get(), 1 + packets + 1);
        // Pacing: 50 ms after the first header report, 10 ms after every write packet.
        let sleeps = clock.sleeps.borrow();
        assert_eq!(
            sleeps.iter().filter(|d| **d == START_HEADER_DELAY).count(),
            1
        );
        assert_eq!(sleeps[0], START_HEADER_DELAY);
        assert_eq!(
            sleeps.iter().filter(|d| **d == PACKET_DELAY).count(),
            packets
        );
    }

    #[test]
    fn write_failure_mid_stream_never_sends_verify_or_finalize() {
        let fake = FakeController {
            fail_send_at: Some(5 + 1000),
            ..FakeController::new()
        };
        let error = flash_with(&fake, &VirtualClock::new()).unwrap_err();
        assert!(error.to_string().contains("USB write failed"));
        assert_eq!(fake.count(UpdateCommand::WriteUpdateImage), 1000);
        assert_eq!(fake.count(UpdateCommand::VerifyUpdateImage), 0);
        assert_eq!(fake.count(UpdateCommand::FinalizeUpdate), 0);
    }

    #[test]
    fn rejected_write_status_stops_the_transfer() {
        let fake = FakeController::with_script(|command, _| match command {
            UpdateCommand::WriteUpdateImage => status(command, 0x02),
            _ => status(command, 0x00),
        });
        let error = flash_with(&fake, &VirtualClock::new()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Controller rejected WriteUpdateImage with status 0x02"
        );
        assert_eq!(fake.count(UpdateCommand::WriteUpdateImage), 1);
        assert_eq!(fake.count(UpdateCommand::VerifyUpdateImage), 0);
    }

    #[test]
    fn rejected_verify_never_sends_finalize() {
        let fake = FakeController::with_script(|command, _| match command {
            UpdateCommand::VerifyUpdateImage => status(command, 0x01),
            _ => status(command, 0x00),
        });
        let error = flash_with(&fake, &VirtualClock::new()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Controller rejected VerifyUpdateImage with status 0x01"
        );
        assert_eq!(fake.count(UpdateCommand::VerifyUpdateImage), 1);
        assert_eq!(fake.count(UpdateCommand::FinalizeUpdate), 0);
    }

    #[test]
    fn pending_statuses_are_polled_until_done() {
        let fake = FakeController::with_script(|command, nth| match (command, nth) {
            (UpdateCommand::StartUpdate, 0) => status(command, 0x10),
            (UpdateCommand::StartUpdate, 1) => status(command, 0x04),
            (UpdateCommand::WriteUpdateImage, 0) => status(command, 0x01),
            (UpdateCommand::WriteUpdateImage, 1) => status(command, 0x10),
            (UpdateCommand::WriteUpdateImage, _) => status(command, 0x03),
            (UpdateCommand::VerifyUpdateImage, 0..=2) => status(command, 0x10),
            _ => status(command, 0x00),
        });
        flash_with(&fake, &VirtualClock::new()).unwrap();
        let packets = write_packet_count(FIRMWARE_SIZE);
        assert_eq!(fake.status_reads.get(), 3 + packets * 3 + 4);
        assert_eq!(fake.count(UpdateCommand::FinalizeUpdate), 1);
    }

    #[test]
    fn a_phase_that_stays_pending_times_out_without_continuing() {
        let fake = FakeController::with_script(|command, _| match command {
            UpdateCommand::VerifyUpdateImage => status(command, 0x10),
            _ => status(command, 0x00),
        });
        let error = flash_with(&fake, &VirtualClock::new()).unwrap_err();
        assert!(matches!(
            error,
            AppError::UpdateTimeout(UpdateCommand::VerifyUpdateImage)
        ));
        assert_eq!(fake.count(UpdateCommand::FinalizeUpdate), 0);
    }

    #[test]
    fn status_read_errors_propagate_and_stop_the_sequence() {
        let fake = FakeController::with_script(|command, _| match command {
            UpdateCommand::WriteUpdateImage => {
                Err(AppError::Io(std::io::Error::other("status read failed")))
            }
            _ => status(command, 0x00),
        });
        let error = flash_with(&fake, &VirtualClock::new()).unwrap_err();
        assert!(matches!(error, AppError::Io(_)));
        assert_eq!(fake.count(UpdateCommand::WriteUpdateImage), 1);
        assert_eq!(fake.count(UpdateCommand::VerifyUpdateImage), 0);
    }

    #[test]
    fn unknown_status_command_is_rejected_before_any_write() {
        let fake = FakeController::with_script(|_, _| Ok(vec![0xF5, 0x7f, 0x00, 0x00]));
        let error = flash_with(&fake, &VirtualClock::new()).unwrap_err();
        assert!(matches!(
            error,
            AppError::UnexpectedUpdateStatusCommand(
                UpdateCommand::Unknown,
                UpdateCommand::StartUpdate
            )
        ));
        assert_eq!(fake.commands(), [UpdateCommand::StartUpdate; 5]);
    }

    #[test]
    fn malformed_status_report_stops_the_sequence() {
        let fake = FakeController::with_script(|command, _| {
            Ok(vec![REPORT_ID_UPDATE_STATUS, command as u8, 0x00])
        });
        let error = flash_with(&fake, &VirtualClock::new()).unwrap_err();
        assert!(matches!(error, AppError::UpdateStatusMalformed(3)));
        assert_eq!(fake.count(UpdateCommand::WriteUpdateImage), 0);
    }

    // Signals and keeping the Mac awake.

    #[test]
    fn a_stop_request_during_the_write_warns_once_and_never_aborts() {
        let fake = FakeController::new();
        let sent = fake.sent.clone();
        let signals = SignalDeferral::new(move || sent.get() > 1000);
        let clock = VirtualClock::new();
        DualSenseUpdater::new(Handle(&fake), &clock, &signals, false)
            .flash(&image())
            .unwrap();
        assert!(matches!(signals.warned.get(), Some(Stage::Writing { .. })));
        assert!(signals.warning(Stage::Checking).is_none());
        assert_eq!(fake.commands().last(), Some(&UpdateCommand::FinalizeUpdate));
        assert_eq!(fake.count(UpdateCommand::StartUpdate), 5);
    }

    #[test]
    fn a_stop_request_during_the_installed_version_check_says_so() {
        let signals = SignalDeferral::new(|| true);
        let expected = ControllerIdentity::of(&info(SERIES, HARDWARE, INSTALLED), PID);
        let installed = verify_installed(&FakeHost::updated(), &signals, &expected, NEW).unwrap();
        assert_eq!(installed.firmware_version, NEW);
        assert_eq!(signals.warned.get(), Some(Stage::Checking));
        assert!(signals.warning(Stage::Checking).is_none());
    }

    #[test]
    fn stop_warning_matches_the_stage() {
        let writing = |secs| {
            stop_warning(Stage::Writing {
                remaining: Duration::from_secs(secs),
            })
        };
        assert_eq!(
            writing(150),
            "Firmware is being written. Stopping now could leave the controller unusable; the update will finish in about 3 minutes."
        );
        assert!(writing(20).ends_with("about 1 minute."));
        assert!(writing(60).ends_with("about 1 minute."));
        assert!(writing(61).ends_with("about 2 minutes."));
        assert_eq!(
            stop_warning(Stage::Checking),
            "Checking the installed firmware. Leave the controller connected; this takes up to 30 seconds."
        );
        let deferral = SignalDeferral::new(|| false);
        assert!(deferral.warning(Stage::Checking).is_none());
        assert!(deferral.warned.get().is_none());
    }

    #[test]
    fn remaining_time_uses_the_measured_transfer_rate() {
        let packets = write_packet_count(FIRMWARE_SIZE) as u32;
        assert_eq!(
            estimate_remaining(None, 0),
            ESTIMATED_PACKET_TIME * packets + VERIFY_TIMEOUT
        );
        assert_eq!(
            estimate_remaining(Some(Duration::from_secs(60)), FIRMWARE_SIZE / 2),
            Duration::from_secs(90)
        );
        assert_eq!(
            estimate_remaining(Some(Duration::from_secs(240)), FIRMWARE_SIZE),
            VERIFY_TIMEOUT
        );
    }

    /// Re-run this test binary with `mode` in `DSMAC_SIGNAL_MODE` and return how the child ended.
    #[cfg(unix)]
    fn signal_child(test: &str, mode: &str) -> std::process::ExitStatus {
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--test-threads=1"])
            .env("DSMAC_SIGNAL_MODE", mode)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
    }

    #[cfg(unix)]
    fn signal_named(name: &str) -> i32 {
        use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
        match name {
            "SIGINT" => SIGINT,
            "SIGTERM" => SIGTERM,
            "SIGHUP" => SIGHUP,
            _ => unreachable!("{name}"),
        }
    }

    // Re-run this binary: the child holds a deferral, raises all three signals, then drops it and
    // must die from the named signal, proving default behaviour is restored for each.
    #[cfg(unix)]
    #[test]
    fn signal_deferral_holds_signals_then_restores_default_termination() {
        use signal_hook::{consts, low_level::raise};
        use std::os::unix::process::ExitStatusExt;
        if let Ok(name) = std::env::var("DSMAC_SIGNAL_MODE") {
            let deferral = SignalDeferral::process().unwrap();
            raise(consts::SIGINT).unwrap();
            raise(consts::SIGTERM).unwrap();
            raise(consts::SIGHUP).unwrap();
            let remaining = Duration::from_secs(150);
            let warning = deferral.warning(Stage::Writing { remaining }).unwrap();
            assert!(warning.ends_with("about 3 minutes."));
            assert!(deferral.warning(Stage::Writing { remaining }).is_none());
            drop(deferral);
            raise(signal_named(&name)).unwrap();
            thread::sleep(Duration::from_secs(5));
            std::process::exit(0);
        }
        for name in ["SIGINT", "SIGTERM", "SIGHUP"] {
            let status = signal_child(
                "update::tests::signal_deferral_holds_signals_then_restores_default_termination",
                name,
            );
            assert_eq!(status.signal(), Some(signal_named(name)), "{name}");
        }
    }

    // Re-run this binary: the child runs a whole install with the process-wide deferral while
    // all three signals arrive during the installed-version check. The install must finish, and
    // once it returns (verified or failed) the named signal must terminate the child.
    #[cfg(unix)]
    #[test]
    fn signals_during_an_install_are_held_and_default_termination_returns_after() {
        use signal_hook::{consts, low_level::raise};
        use std::os::unix::process::ExitStatusExt;
        if let Ok(mode) = std::env::var("DSMAC_SIGNAL_MODE") {
            let (outcome, name) = mode.split_once(':').unwrap();
            let fake = FakeController {
                fail_send_at: (outcome == "error").then_some(10),
                ..FakeController::new()
            };
            let version = if outcome == "error" { INSTALLED } else { NEW };
            let host = FakeHost {
                process_signals: true,
                ..FakeHost::new(
                    |n| {
                        // n = 0 is the preflight; later enumerations are the installed-version check.
                        if n == 1 {
                            for signal in [consts::SIGINT, consts::SIGTERM, consts::SIGHUP] {
                                raise(signal).unwrap();
                            }
                        }
                        vec![usb(PATH, PID)]
                    },
                    move |_, _| Ok(info(SERIES, HARDWARE, version)),
                )
            };
            let installation = install_stopping_on(&fake, &host);
            let result = installation.result;
            assert_eq!(result.is_ok(), outcome == "ok", "{result:?}");
            // The last signal raised is the one reported.
            assert_eq!(installation.stop_requested, Some(StopSignal::Hangup));
            assert!(host.enumerations.get() > 1);
            raise(signal_named(name)).unwrap();
            thread::sleep(Duration::from_secs(5));
            std::process::exit(0);
        }
        for outcome in ["ok", "error"] {
            for name in ["SIGINT", "SIGTERM", "SIGHUP"] {
                let status = signal_child(
                    "update::tests::signals_during_an_install_are_held_and_default_termination_returns_after",
                    &format!("{outcome}:{name}"),
                );
                assert_eq!(
                    status.signal(),
                    Some(signal_named(name)),
                    "{outcome} {name}"
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn keep_awake_helper_runs_in_its_own_group_and_ends_when_dropped() {
        let alive = |pid: u32| {
            Command::new("sh")
                .args(["-c", &format!("kill -0 {pid} 2>/dev/null")])
                .status()
                .unwrap()
                .success()
        };
        let mut command = Command::new("sleep");
        command.arg("30");
        let awake = KeepAwake::spawn(command);
        let pid = awake.0.as_ref().expect("sleep starts").id();
        assert!(alive(pid));
        // Its own process group: Ctrl-C in the terminal does not reach it.
        let group = Command::new("ps")
            .args(["-o", "pgid=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&group.stdout).trim(),
            pid.to_string()
        );
        drop(awake);
        assert!(!alive(pid));
        // A helper that cannot start does not stop the update.
        assert!(
            KeepAwake::spawn(Command::new("/nonexistent/caffeinate"))
                .0
                .is_none()
        );
        let error = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert_eq!(
            keep_awake_warning(&error),
            format!(
                "Could not keep the Mac awake ({error}); keep it awake until the update finishes."
            )
        );
    }

    // The guarded install.

    #[test]
    fn install_reports_a_held_stop_request_with_a_success() {
        let fake = FakeController::new();
        let host = FakeHost::updated();
        host.stop_requested.set(true);
        let installation = install_stopping_on(&fake, &host);
        assert!(matches!(
            installation.result,
            Ok(InstallOutcome::Verified(_))
        ));
        assert_eq!(installation.stop_requested, Some(StopSignal::Interrupt));
        assert_eq!(fake.count(UpdateCommand::FinalizeUpdate), 1);
    }

    #[test]
    fn install_reports_a_held_stop_request_with_a_failure_and_never_retries() {
        let fake = FakeController::new();
        // The controller never reports the new version, so the install fails verification.
        let host = FakeHost::new(
            |_| vec![usb(PATH, PID)],
            |_, _| Ok(info(SERIES, HARDWARE, INSTALLED)),
        );
        host.stop_requested.set(true);
        let installation = install_stopping_on(&fake, &host);
        assert!(installation.result.is_err());
        assert_eq!(installation.stop_requested, Some(StopSignal::Interrupt));
        assert_eq!(fake.count(UpdateCommand::StartUpdate), 5);
    }

    #[test]
    fn install_reports_no_stop_when_none_was_requested() {
        let installation = install_stopping_on(&FakeController::new(), &FakeHost::updated());
        assert!(installation.result.is_ok());
        assert_eq!(installation.stop_requested, None);
    }

    #[test]
    fn install_verifies_on_a_new_handle_after_the_full_sequence() {
        let fake = FakeController::new();
        let host = FakeHost::updated();
        let outcome = install_on(&fake, &host).unwrap();
        assert!(matches!(&outcome, InstallOutcome::Verified(i) if i.firmware_version == NEW));
        assert_eq!(
            outcome.message(),
            "Verified installed firmware: 0x0630 (Jul  4 2025 10:38:40)."
        );
        assert!(host.deferred.get());
        assert!(host.reads.get() >= 1);
        assert_eq!(fake.commands().last(), Some(&UpdateCommand::FinalizeUpdate));
    }

    #[test]
    fn protocol_error_with_a_verified_version_stays_a_verified_install() {
        let fake = FakeController::with_script(|command, _| match command {
            UpdateCommand::VerifyUpdateImage => status(command, 0x01),
            _ => status(command, 0x00),
        });
        let host = FakeHost::updated();
        let outcome = install_on(&fake, &host).unwrap();
        assert!(matches!(
            &outcome,
            InstallOutcome::VerifiedDespiteProtocolError { installed, .. }
                if installed.firmware_version == NEW
        ));
        assert_eq!(
            outcome.message(),
            "Verified installed firmware: 0x0630. Controller rebooted/committed despite a protocol error: Controller rejected VerifyUpdateImage with status 0x01"
        );
        // Never retried or reflashed.
        assert_eq!(fake.count(UpdateCommand::StartUpdate), 5);
        assert_eq!(fake.count(UpdateCommand::FinalizeUpdate), 0);
    }

    #[test]
    fn unverified_installs_are_errors() {
        let still_old = || {
            FakeHost::new(
                |_| vec![usb(PATH, PID)],
                |_, _| Ok(info(SERIES, HARDWARE, INSTALLED)),
            )
        };
        let error = install_on(&FakeController::new(), &still_old()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Transfer finished but installed firmware 0x0630 could not be verified. Reconnect and run --print-firmware-info; do not assume the update succeeded"
        );
        let failing = FakeController {
            fail_send_at: Some(10),
            ..FakeController::new()
        };
        let error = install_on(&failing, &still_old()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "IO error: USB write failed. Update not verified; reconnect and run --print-firmware-info before any retry"
        );
        assert_eq!(failing.count(UpdateCommand::VerifyUpdateImage), 0);
    }

    #[test]
    fn conclude_keeps_the_outcome_matrix() {
        let rejected = || AppError::Validation("Controller rejected X".into());
        let unverified = || AppError::Validation("not verified".into());
        let new = || info(SERIES, HARDWARE, NEW);
        assert!(matches!(
            conclude(Ok(()), Ok(new())),
            Ok(InstallOutcome::Verified(_))
        ));
        assert!(matches!(
            conclude(Err(rejected()), Ok(new())),
            Ok(InstallOutcome::VerifiedDespiteProtocolError { .. })
        ));
        assert_eq!(
            conclude(Err(rejected()), Err(unverified()))
                .unwrap_err()
                .to_string(),
            "Controller rejected X. Update not verified; reconnect and run --print-firmware-info before any retry"
        );
        assert_eq!(
            conclude(Ok(()), Err(unverified())).unwrap_err().to_string(),
            "not verified"
        );
    }

    #[test]
    fn install_refuses_before_writing_when_the_controller_changed() {
        let changed = [
            (info(0x000e, HARDWARE, INSTALLED), PID),
            (info(SERIES, 0x1108, INSTALLED), PID),
            (info(SERIES, HARDWARE, NEW), PID),
            (info(SERIES, HARDWARE, INSTALLED), 0x0df2),
        ];
        for (current, pid) in changed {
            let fake = FakeController {
                info: current,
                ..FakeController::new()
            };
            let host = FakeHost::new(move |_| vec![usb(PATH, pid)], |_, _| unreachable!());
            let error = install_on(&fake, &host).unwrap_err().to_string();
            assert!(
                error.starts_with("Controller changed after the firmware check")
                    && error.ends_with("nothing was written. Check it again before updating"),
                "{error}"
            );
            assert!(fake.reports.borrow().is_empty());
            assert!(!host.deferred.get());
            assert_eq!(host.reads.get(), 0);
        }
    }

    #[test]
    fn install_requires_exactly_one_usb_controller() {
        for (devices, message) in [
            (
                vec![usb(PATH, PID), usb("DevSrvsID:2", PID)],
                "Disconnect other controllers before updating firmware",
            ),
            (
                vec![usb(PATH, PID), usb("DevSrvsID:2", 0x0df2)],
                "Disconnect other controllers before updating firmware",
            ),
            (
                vec![],
                "The controller is no longer connected over USB; nothing was written",
            ),
            (
                vec![usb("DevSrvsID:2", PID)],
                "The connected controller is not the one that was checked; nothing was written. Check it again before updating",
            ),
        ] {
            let fake = FakeController::new();
            let host = FakeHost::new(move |_| devices.clone(), |_, _| unreachable!());
            assert_eq!(install_on(&fake, &host).unwrap_err().to_string(), message);
            assert!(fake.reports.borrow().is_empty());
            assert!(!host.deferred.get());
        }
    }

    #[test]
    fn install_refuses_low_battery_before_writing() {
        let fake = FakeController {
            battery: 5,
            ..FakeController::new()
        };
        let host = FakeHost::updated();
        assert_eq!(
            install_on(&fake, &host).unwrap_err().to_string(),
            "Battery is at 5%. Charge the controller to at least 10% (it charges over the USB cable), then refresh and try again."
        );
        assert!(fake.reports.borrow().is_empty());
        assert!(!host.deferred.get());
    }

    // Installed-version verification.

    fn verify(host: &FakeHost) -> Result<FirmwareInfo> {
        let expected = ControllerIdentity::of(&info(SERIES, HARDWARE, INSTALLED), PID);
        verify_installed(host, &quiet_signals(), &expected, NEW)
    }

    fn assert_times_out(host: &FakeHost) {
        assert!(verify(host).is_err());
        assert!(host.clock.elapsed.get() >= VERIFY_TIMEOUT);
        assert!(host.clock.elapsed.get() <= VERIFY_TIMEOUT + VERIFY_POLL);
    }

    #[test]
    fn verification_accepts_only_a_single_controller_matching_the_update() {
        // Several controllers: never read, even if one looks updated.
        let host = FakeHost::new(
            |_| vec![usb("a", PID), usb("b", PID)],
            |_, _| Ok(info(SERIES, HARDWARE, NEW)),
        );
        assert_times_out(&host);
        assert_eq!(host.reads.get(), 0);
        // A different model in place of the flashed one is never read.
        let host = FakeHost::new(
            |_| vec![usb("a", 0x0df2)],
            |_, _| Ok(info(0x0044, HARDWARE, NEW)),
        );
        assert_times_out(&host);
        assert_eq!(host.reads.get(), 0);
        // Wrong series, wrong hardware or the old version keep polling until the deadline.
        for wrong in [
            info(0x0004, HARDWARE, NEW),
            info(SERIES, 0x2001, NEW),
            info(SERIES, HARDWARE, INSTALLED),
        ] {
            let host = FakeHost::new(|_| vec![usb("new", PID)], move |_, _| Ok(wrong.clone()));
            assert_times_out(&host);
            assert!(host.reads.get() >= 59);
        }
    }

    #[test]
    fn verification_waits_for_reconnection_and_a_single_controller() {
        // Absent while rebooting, then briefly alongside another controller, then alone.
        let host = FakeHost::new(
            |n| match n {
                0..=3 => vec![],
                4..=6 => vec![usb("new", PID), usb("other", PID)],
                _ => vec![usb("new", PID)],
            },
            |n, c| {
                assert_eq!(c.path, "new");
                if n == 0 {
                    Err(AppError::Validation("not ready".into()))
                } else {
                    Ok(info(SERIES, HARDWARE, NEW))
                }
            },
        );
        let installed = verify(&host).unwrap();
        assert_eq!(installed.firmware_version, NEW);
        assert_eq!(host.enumerations.get(), 9);
        assert_eq!(host.reads.get(), 2);
        assert_eq!(host.clock.elapsed.get(), VERIFY_POLL * 9);
    }

    #[test]
    fn sole_controller_requires_one_device_with_the_flashed_pid() {
        let one = [usb("a", PID)];
        assert_eq!(
            sole_controller(&one, PID).map(|c| c.path.as_str()),
            Some("a")
        );
        assert!(sole_controller(&one, 0x0df2).is_none());
        assert!(sole_controller(&[], PID).is_none());
        assert!(sole_controller(&[usb("a", PID), usb("b", 0x0df2)], PID).is_none());
        let expected = ControllerIdentity::of(&info(SERIES, HARDWARE, INSTALLED), PID);
        assert!(is_expected_install(
            &info(SERIES, HARDWARE, NEW),
            &expected,
            NEW
        ));
        assert!(!is_expected_install(
            &info(SERIES, HARDWARE, INSTALLED),
            &expected,
            NEW
        ));
    }
}
