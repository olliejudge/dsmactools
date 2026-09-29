use crate::{
    error::{AppError, Result},
    hid::DualSenseHid,
};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    io::{IsTerminal, Write},
    path::Path,
    time::{Duration, Instant},
};

pub fn decode(raw: &[u8]) -> Result<Value> {
    if raw.len() != 64 || raw[0] != 1 {
        return Err(AppError::Validation(
            "Expected a 64-byte DualSense USB input report (0x01)".into(),
        ));
    }
    let names = [
        "Square", "Cross", "Circle", "Triangle", "L1", "R1", "L2", "R2", "Create", "Options", "L3",
        "R3", "PS", "Touchpad", "Mute",
    ];
    let bits = u32::from(raw[8] >> 4) | (u32::from(raw[9]) << 4) | (u32::from(raw[10] & 7) << 12);
    let pressed: Vec<_> = names
        .iter()
        .enumerate()
        .filter_map(|(i, name)| (bits & (1 << i) != 0).then_some(*name))
        .collect();
    let axis = |offset| i16::from_le_bytes([raw[offset], raw[offset + 1]]);
    let touch = |offset| {
        json!({"active":raw[offset] & 0x80 == 0,"id":raw[offset] & 0x7f,
        "x":u16::from(raw[offset+1]) | (u16::from(raw[offset+2] & 15) << 8),
        "y":u16::from(raw[offset+2] >> 4) | (u16::from(raw[offset+3]) << 4)})
    };
    let sticks: Vec<_> = raw[1..5]
        .iter()
        .map(|v| (2.0 * f64::from(*v) / 255.0) - 1.0)
        .collect();
    Ok(
        json!({"sticks":sticks,"sticks_raw":&raw[1..5],"triggers_raw":&raw[5..7],"sequence":raw[7],
        "dpad":raw[8]&15,"buttons":pressed,"gyro_raw":[axis(16),axis(18),axis(20)],
        "accel_raw":[axis(22),axis(24),axis(26)],"sensor_timestamp":u32::from_le_bytes(raw[28..32].try_into().unwrap()),
        "touch":[touch(33),touch(37)],"battery_raw":raw[53]}),
    )
}

/// Catch process cancellation so timed effects can reset and captures can flush.
pub struct Cancellation {
    flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    registrations: Vec<signal_hook::SigId>,
}
impl Cancellation {
    pub fn new() -> Result<Self> {
        let mut this = Self {
            flag: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            registrations: Vec::new(),
        };
        for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
            this.registrations
                .push(signal_hook::flag::register(signal, this.flag.clone())?);
        }
        Ok(this)
    }
    pub fn requested(&self) -> bool {
        self.flag.load(std::sync::atomic::Ordering::Relaxed)
    }
}
impl Drop for Cancellation {
    fn drop(&mut self) {
        for id in self.registrations.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}

pub struct TerminalGuard;
impl TerminalGuard {
    pub fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        let guard = Self;
        execute!(std::io::stdout(), EnterAlternateScreen, cursor::Hide)?;
        Ok(guard)
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen, cursor::Show);
        let _ = terminal::disable_raw_mode();
    }
}
pub fn stop_requested() -> Result<bool> {
    while event::poll(Duration::ZERO)? {
        if let Event::Key(key) = event::read()?
            && (matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
                || (key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL)))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn monitor(dev: &DualSenseHid) -> Result<()> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(AppError::Validation(
            "The live monitor needs a terminal; use --record FILE for a capture".into(),
        ));
    }
    let cancellation = Cancellation::new()?;
    let _guard = TerminalGuard::enter()?;
    let start = Instant::now();
    let mut drawn = start - Duration::from_secs(1);
    let mut count = 0u64;
    loop {
        if cancellation.requested() || stop_requested()? {
            return Ok(());
        }
        if let Some(raw) = dev.read_input(20)? {
            let value = decode(&raw)?;
            count += 1;
            if drawn.elapsed() >= Duration::from_millis(33) {
                let text = format!(
                    "{}\n\nLeft stick    X {:+.3}   Y {:+.3}\nRight stick   X {:+.3}   Y {:+.3}\nTriggers      L {:3}     R {:3}   (0–255)\nD-pad         {} (8 = neutral)\nButtons       {}\n\nGyroscope     {}   (raw counts)\nAccelerometer {}   (raw counts)\nTouch 1       {}\nTouch 2       {}\n\n{} reports · {:.1} reports/s (host arrival rate)\nq / Esc / Ctrl-C to return",
                    console::style("DS MAC TOOLS · Live USB input")
                        .cyan()
                        .bold(),
                    value["sticks"][0].as_f64().unwrap(),
                    value["sticks"][1].as_f64().unwrap(),
                    value["sticks"][2].as_f64().unwrap(),
                    value["sticks"][3].as_f64().unwrap(),
                    raw[5],
                    raw[6],
                    value["dpad"],
                    value["buttons"],
                    value["gyro_raw"],
                    value["accel_raw"],
                    value["touch"][0],
                    value["touch"][1],
                    count,
                    count as f64 / start.elapsed().as_secs_f64()
                );
                execute!(
                    std::io::stdout(),
                    cursor::MoveTo(0, 0),
                    Clear(ClearType::All)
                )?;
                write!(std::io::stdout(), "{}", text.replace('\n', "\r\n"))?;
                std::io::stdout().flush()?;
                drawn = Instant::now();
            }
        } else if start.elapsed() > Duration::from_secs(3) && count == 0 {
            return Err(AppError::Validation(
                "No input reports received; check the USB connection".into(),
            ));
        }
    }
}

pub fn record(dev: &DualSenseHid, path: &Path, seconds: u64, pid: u16) -> Result<()> {
    if !(1..=3600).contains(&seconds) {
        return Err(AppError::Validation(
            "Capture duration must be 1–3600 seconds".into(),
        ));
    }
    let cancellation = Cancellation::new()?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    writeln!(
        file,
        "{}",
        json!({"kind":"metadata","schema":1,"tool":env!("CARGO_PKG_VERSION"),"transport":"USB","pid":pid,"requested_seconds":seconds,"timing":"host arrival; not end-to-end latency","motion_units":"raw sensor counts"})
    )?;
    println!(
        "Recording for {seconds}s to {}. Move sticks and press buttons.",
        path.display()
    );
    let start = Instant::now();
    let mut previous = None;
    let mut count = 0u64;
    let mut min = u128::MAX;
    let mut max = 0u128;
    let mut sum = 0u128;
    let mut axis_min = [255u8; 6];
    let mut axis_max = [0u8; 6];
    let mut buttons = std::collections::BTreeSet::new();
    // Raw mode allows cancellation without terminating before the summary is flushed.
    let guard = if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        Some(TerminalGuard::enter()?)
    } else {
        None
    };
    if guard.is_some() {
        print!(
            "DS MAC TOOLS · Recording input\r\n\r\n{}\r\nDuration: {seconds}s · q / Esc / Ctrl-C to finish early.\r\n",
            path.display()
        );
        std::io::stdout().flush()?;
    }
    let mut cancelled = false;
    let outcome = (|| -> Result<()> {
        while start.elapsed() < Duration::from_secs(seconds) {
            if cancellation.requested() || (guard.is_some() && stop_requested()?) {
                cancelled = true;
                break;
            }
            let Some(raw) = dev.read_input(20)? else {
                continue;
            };
            let input = decode(&raw)?;
            let elapsed = start.elapsed().as_micros();
            if let Some(prior) = previous {
                let interval = elapsed - prior;
                min = min.min(interval);
                max = max.max(interval);
                sum += interval;
            }
            previous = Some(elapsed);
            for i in 0..6 {
                axis_min[i] = axis_min[i].min(raw[i + 1]);
                axis_max[i] = axis_max[i].max(raw[i + 1]);
            }
            for button in input["buttons"].as_array().unwrap() {
                buttons.insert(button.as_str().unwrap().to_owned());
            }
            writeln!(
                file,
                "{}",
                json!({"kind":"sample","elapsed_us":elapsed,"raw":raw,"input":input})
            )?;
            count += 1;
        }
        Ok(())
    })();
    let summary = json!({"kind":"summary","samples":count,"elapsed_seconds":start.elapsed().as_secs_f64(),"reports_per_second":count as f64 / start.elapsed().as_secs_f64(),"arrival_interval_us": if count>1 {json!({"min":min,"mean":sum as f64/(count-1) as f64,"max":max})} else {Value::Null},"axis_min":axis_min,"axis_max":axis_max,"buttons_seen":buttons,"complete":outcome.is_ok(),"cancelled":cancelled});
    writeln!(file, "{summary}")?;
    file.flush()?;
    drop(guard);
    println!("{}", serde_json::to_string_pretty(&summary).unwrap());
    outcome?;
    if count == 0 {
        return Err(AppError::Validation(
            "Capture saved, but no input reports were received".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_buttons_signed_motion_and_touch() {
        let mut raw = [0u8; 64];
        raw[0] = 1;
        raw[1] = 255;
        raw[8] = 0x28;
        raw[9] = 0x81;
        raw[10] = 1;
        raw[16..18].copy_from_slice(&(-123i16).to_le_bytes());
        raw[33..37].copy_from_slice(&[7, 0xbc, 0x3a, 0x12]);
        let data = decode(&raw).unwrap();
        assert_eq!(data["buttons"], json!(["Cross", "L1", "R3", "PS"]));
        assert_eq!(data["gyro_raw"][0], -123);
        assert_eq!(data["touch"][0]["x"], 0xabc);
        assert_eq!(data["touch"][0]["y"], 0x123);
        assert_eq!(data["sticks"][0], 1.0);
        assert_eq!(data["dpad"], 8);
        assert!(decode(&raw[..63]).is_err());
        raw[0] = 0x31;
        assert!(decode(&raw).is_err());
    }
}
