use crate::{
    error::{AppError, Result},
    hid::DualSenseHid,
    input::{Cancellation, TerminalGuard, stop_requested},
};
use clap::ValueEnum;
use std::{
    io::IsTerminal,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Preset {
    Rumble,
    Lightbar,
    Triggers,
}

// USB output report 0x02. These are volatile effects, separate from firmware reports.
fn report(preset: Preset) -> [u8; 63] {
    let mut data = [0u8; 63];
    data[0] = 2;
    match preset {
        Preset::Rumble => {
            data[1] = 3;
            data[3] = 40;
            data[4] = 40;
        }
        Preset::Lightbar => {
            data[2] = 4;
            data[45] = 30;
            data[46] = 140;
            data[47] = 255;
        }
        Preset::Triggers => {
            data[1] = 12;
            // Mild resistance from zone 4 onwards. Six zones at strength 2.
            for offset in [11, 22] {
                data[offset] = 0x21;
                data[offset + 1..offset + 3].copy_from_slice(&0x03f0u16.to_le_bytes());
                let mut strength = 0u32;
                for zone in 4..10 {
                    strength |= 1 << (zone * 3);
                }
                data[offset + 3..offset + 7].copy_from_slice(&strength.to_le_bytes());
            }
        }
    }
    data
}
fn reset_report() -> [u8; 63] {
    let mut data = [0u8; 63];
    data[0] = 2;
    data[1] = 15;
    data[2] = 8;
    data[11] = 5;
    data[22] = 5;
    data
}
struct Reset<'a>(&'a DualSenseHid);
impl Drop for Reset<'_> {
    fn drop(&mut self) {
        let _ = self.0.write_output(&reset_report());
    }
}

pub fn run(dev: &DualSenseHid, preset: Preset, seconds: u64) -> Result<()> {
    if !(1..=30).contains(&seconds) {
        return Err(AppError::Validation(
            "Feedback duration must be 1–30 seconds".into(),
        ));
    }
    println!("{preset:?} test for {seconds}s. q / Esc / Ctrl-C stops the test in a terminal.");
    let cancellation = Cancellation::new()?;
    let terminal = if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        Some(TerminalGuard::enter()?)
    } else {
        None
    };
    let reset = Reset(dev);
    dev.write_output(&report(preset))?;
    if terminal.is_some() {
        print!(
            "DS MAC TOOLS · {preset:?} test\r\n\r\nEnds after {seconds}s. q / Esc / Ctrl-C to stop.\r\n"
        );
    }
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(seconds) {
        if cancellation.requested() || (terminal.is_some() && stop_requested()?) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // An explicit reset reports failures; Drop also attempts cleanup on error paths.
    dev.write_output(&reset_report())?;
    drop(reset);
    drop(terminal);
    println!("Effects stopped; trigger resistance disabled and lightbar control released.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn effects_and_cleanup_use_only_volatile_output_reports() {
        let rumble = report(Preset::Rumble);
        assert_eq!(rumble[0], 2);
        assert_eq!(rumble.len(), 63);
        assert_eq!(&rumble[1..5], &[3, 0, 40, 40]);
        let light = report(Preset::Lightbar);
        assert_eq!(light[2], 4);
        assert_eq!(&light[45..48], &[30, 140, 255]);
        let triggers = report(Preset::Triggers);
        assert_eq!(triggers[1], 12);
        assert_eq!(triggers[11], 0x21);
        assert_eq!(triggers[22], 0x21);
        assert_eq!(&triggers[12..14], &[0xf0, 3]);
        assert_eq!(&triggers[11..22], &triggers[22..33]);
        let reset = reset_report();
        assert_eq!(reset[1], 15);
        assert_eq!(reset[2], 8);
        assert_eq!(reset[11], 5);
        assert_eq!(reset[22], 5);
        assert_eq!(reset[3], 0);
        assert_eq!(reset[4], 0);
    }
}
