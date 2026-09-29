use crate::{
    cli::Args,
    error::{AppError, Result},
    hid::DualSenseHid,
    input::{Cancellation, TerminalGuard, stop_requested},
};
use clap::ValueEnum;
use std::{
    fmt,
    io::{IsTerminal, Write},
    str::FromStr,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Preset {
    Rumble,
    Lightbar,
    Triggers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

pub const COLORS: &[(&str, Rgb)] = &[
    ("Red", Rgb(255, 0, 0)),
    ("Orange", Rgb(255, 80, 0)),
    ("Amber", Rgb(255, 160, 0)),
    ("Yellow", Rgb(255, 255, 0)),
    ("Lime", Rgb(128, 255, 0)),
    ("Green", Rgb(0, 255, 0)),
    ("Mint", Rgb(0, 255, 128)),
    ("Cyan", Rgb(0, 255, 255)),
    ("Sky blue", Rgb(30, 140, 255)),
    ("Blue", Rgb(0, 0, 255)),
    ("Indigo", Rgb(75, 0, 255)),
    ("Purple", Rgb(160, 0, 255)),
    ("Magenta", Rgb(255, 0, 255)),
    ("Pink", Rgb(255, 70, 140)),
    ("White", Rgb(255, 255, 255)),
    ("Off", Rgb(0, 0, 0)),
];

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02X}{:02X}{:02X}", self.0, self.1, self.2)
    }
}
impl FromStr for Rgb {
    type Err = String;
    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        let value = value.trim();
        let normalized = value.replace('-', " ");
        if value.eq_ignore_ascii_case("black") {
            return Ok(Self(0, 0, 0));
        }
        if let Some((_, rgb)) = COLORS
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(&normalized))
        {
            return Ok(*rgb);
        }
        let hex = value.strip_prefix('#').unwrap_or(value);
        if hex.len() == 6 && hex.bytes().all(|c| c.is_ascii_hexdigit()) {
            let packed = u32::from_str_radix(hex, 16).map_err(|e| e.to_string())?;
            return Ok(Self(
                (packed >> 16) as u8,
                (packed >> 8) as u8,
                packed as u8,
            ));
        }
        let channels: Vec<_> = value.split(',').map(|v| v.trim().parse::<u8>()).collect();
        if let [Ok(r), Ok(g), Ok(b)] = channels.as_slice() {
            return Ok(Self(*r, *g, *b));
        }
        Err("Use a color name, #RRGGBB, or R,G,B with channels 0–255".into())
    }
}
impl Rgb {
    fn dimmed(self, brightness: u8) -> Self {
        let scale = |v| (u16::from(v) * u16::from(brightness) / 255) as u8;
        Self(scale(self.0), scale(self.1), scale(self.2))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum TriggerSide {
    Both,
    Left,
    Right,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum TriggerEffect {
    Resistance,
    Weapon,
    Vibration,
    Off,
    Bow,
    Galloping,
    Machine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZoneLevels(pub [u8; 10]);
impl FromStr for ZoneLevels {
    type Err = String;
    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        let values = value
            .split(',')
            .map(|v| v.trim().parse::<u8>())
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| "Zones require ten comma-separated levels, each 0–8".to_string())?;
        let levels: [u8; 10] = values
            .try_into()
            .map_err(|_| "Zones require exactly ten levels".to_string())?;
        if levels.iter().any(|&v| v > 8) {
            return Err("Zone levels must be 0–8".into());
        }
        Ok(Self(levels))
    }
}
impl fmt::Display for ZoneLevels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, level) in self.0.iter().enumerate() {
            if index != 0 {
                write!(f, ",")?;
            }
            write!(f, "{level}")?;
        }
        Ok(())
    }
}

// The report payload is constructed only after validation; no later bit shifts or
// strength subtraction depend on unchecked CLI input.
#[derive(Clone, Copy, Debug)]
pub struct TriggerTest {
    payload: [u8; 11],
    mode: TriggerEffect,
    start: u8,
    end: u8,
    strength: u8,
    frequency: u8,
    secondary: u8,
    period: u8,
    zones: Option<ZoneLevels>,
}
#[derive(Clone, Copy, Debug)]
pub enum Effect {
    Rumble,
    Lightbar {
        color: Rgb,
        brightness: u8,
        cycle: bool,
    },
    Triggers {
        side: TriggerSide,
        test: TriggerTest,
    },
}

fn invalid(message: &str) -> AppError {
    AppError::Validation(message.into())
}

impl Effect {
    pub fn from_args(args: &Args) -> Result<Self> {
        if !(1..=30).contains(&args.duration) {
            return Err(invalid("Feedback duration must be 1–30 seconds"));
        }
        let has_color = args.color.is_some() || args.brightness.is_some() || args.cycle;
        let has_trigger = args.trigger.is_some()
            || args.effect.is_some()
            || args.start.is_some()
            || args.end.is_some()
            || args.strength.is_some()
            || args.frequency.is_some()
            || args.snap_strength.is_some()
            || args.first_foot.is_some()
            || args.second_foot.is_some()
            || args.strength_b.is_some()
            || args.period.is_some()
            || args.zones.is_some();
        let preset = args
            .feedback
            .ok_or_else(|| invalid("Choose a feedback test"))?;
        if has_color && preset != Preset::Lightbar {
            return Err(invalid(
                "Color, brightness and cycle options require --feedback lightbar",
            ));
        }
        if has_trigger && preset != Preset::Triggers {
            return Err(invalid("Trigger options require --feedback triggers"));
        }
        match preset {
            Preset::Rumble => Ok(Self::Rumble),
            Preset::Lightbar => {
                if args.cycle && args.color.is_some() {
                    return Err(invalid("Choose a fixed color or a rainbow cycle"));
                }
                Ok(Self::Lightbar {
                    color: args.color.unwrap_or(Rgb(30, 140, 255)),
                    brightness: args.brightness.unwrap_or(255),
                    cycle: args.cycle,
                })
            }
            Preset::Triggers => Ok(Self::Triggers {
                side: args.trigger.unwrap_or(TriggerSide::Both),
                test: TriggerTest::from_args(args)?,
            }),
        }
    }

    fn label(self) -> String {
        match self {
            Self::Rumble => "Gentle rumble".into(),
            Self::Lightbar {
                color,
                brightness,
                cycle,
            } => format!(
                "Lightbar · {} · brightness {brightness}/255",
                if cycle {
                    "Rainbow cycle".into()
                } else {
                    color.to_string()
                }
            ),
            Self::Triggers { side, test } => {
                let side = match side {
                    TriggerSide::Both => "L2 + R2",
                    TriggerSide::Left => "L2",
                    TriggerSide::Right => "R2",
                };
                format!("{side} · {}", test.label())
            }
        }
    }
}
impl TriggerTest {
    fn from_args(args: &Args) -> Result<Self> {
        let mode = args.effect.unwrap_or(TriggerEffect::Resistance);
        let uniform = matches!(mode, TriggerEffect::Resistance | TriggerEffect::Vibration);
        let bounded = matches!(
            mode,
            TriggerEffect::Weapon
                | TriggerEffect::Bow
                | TriggerEffect::Galloping
                | TriggerEffect::Machine
        );
        let rhythmic = matches!(
            mode,
            TriggerEffect::Vibration | TriggerEffect::Galloping | TriggerEffect::Machine
        );
        for (present, allowed, name) in [
            (
                args.start.is_some(),
                mode != TriggerEffect::Off && args.zones.is_none(),
                "--start",
            ),
            (args.end.is_some(), bounded, "--end"),
            (
                args.strength.is_some(),
                mode != TriggerEffect::Off
                    && mode != TriggerEffect::Galloping
                    && args.zones.is_none(),
                "--strength",
            ),
            (args.frequency.is_some(), rhythmic, "--frequency"),
            (
                args.snap_strength.is_some(),
                mode == TriggerEffect::Bow,
                "--snap-strength",
            ),
            (
                args.first_foot.is_some(),
                mode == TriggerEffect::Galloping,
                "--first-foot",
            ),
            (
                args.second_foot.is_some(),
                mode == TriggerEffect::Galloping,
                "--second-foot",
            ),
            (
                args.strength_b.is_some(),
                mode == TriggerEffect::Machine,
                "--strength-b",
            ),
            (
                args.period.is_some(),
                mode == TriggerEffect::Machine,
                "--period",
            ),
            (args.zones.is_some(), uniform, "--zones"),
        ] {
            if present && !allowed {
                return Err(invalid(&format!(
                    "{name} is not used by the selected effect/profile"
                )));
            }
        }
        let start = args.start.unwrap_or(4);
        let end = args.end.unwrap_or(7);
        let strength = args.strength.unwrap_or(2);
        let frequency = args
            .frequency
            .unwrap_or(if mode == TriggerEffect::Galloping {
                4
            } else {
                25
            });
        let secondary = match mode {
            TriggerEffect::Bow => args.snap_strength.unwrap_or(2),
            TriggerEffect::Galloping => args.second_foot.unwrap_or(3),
            TriggerEffect::Machine => args.strength_b.unwrap_or(4),
            _ => 0,
        };
        let period = args.period.unwrap_or(20);
        let mut payload = [0u8; 11];
        match mode {
            TriggerEffect::Off => payload[0] = 0x05,
            TriggerEffect::Resistance | TriggerEffect::Vibration => {
                let levels = if let Some(zones) = args.zones {
                    if zones.0.iter().any(|&v| v > 8) {
                        return Err(invalid("Zone levels must be 0–8"));
                    }
                    zones
                } else {
                    if start > 9 || !(1..=8).contains(&strength) {
                        return Err(invalid("Start must be 0–9 and strength 1–8"));
                    }
                    ZoneLevels(std::array::from_fn(|zone| {
                        if zone >= usize::from(start) {
                            strength
                        } else {
                            0
                        }
                    }))
                };
                if mode == TriggerEffect::Vibration && frequency == 0 {
                    return Err(invalid("Frequency must be 1–255"));
                }
                payload[0] = if mode == TriggerEffect::Resistance {
                    0x21
                } else {
                    0x26
                };
                let mut enabled = 0u16;
                let mut packed = 0u32;
                for (zone, level) in levels.0.into_iter().enumerate() {
                    if level != 0 {
                        enabled |= 1 << zone;
                        packed |= u32::from(level - 1) << (zone * 3);
                    }
                }
                payload[1..3].copy_from_slice(&enabled.to_le_bytes());
                payload[3..7].copy_from_slice(&packed.to_le_bytes());
                if mode == TriggerEffect::Vibration {
                    payload[9] = frequency;
                }
            }
            TriggerEffect::Weapon
            | TriggerEffect::Bow
            | TriggerEffect::Galloping
            | TriggerEffect::Machine => {
                let (min_start, max_start, max_end) = match mode {
                    TriggerEffect::Weapon => (2, 7, 8),
                    TriggerEffect::Bow => (1, 7, 8),
                    TriggerEffect::Galloping => (0, 8, 9),
                    TriggerEffect::Machine => (1, 8, 9),
                    _ => unreachable!(),
                };
                if !(min_start..=max_start).contains(&start) || end <= start || end > max_end {
                    return Err(invalid(&format!(
                        "Start must be {min_start}–{max_start}; end must exceed start and be at most {max_end}"
                    )));
                }
                let boundaries = (1u16 << start) | (1u16 << end);
                payload[1..3].copy_from_slice(&boundaries.to_le_bytes());
                match mode {
                    TriggerEffect::Weapon | TriggerEffect::Bow => {
                        if !(1..=8).contains(&strength) {
                            return Err(invalid("Strength must be 1–8"));
                        }
                        payload[0] = if mode == TriggerEffect::Weapon {
                            0x25
                        } else {
                            0x22
                        };
                        payload[3] = strength - 1;
                        if mode == TriggerEffect::Bow {
                            if !(1..=8).contains(&secondary) {
                                return Err(invalid("Bow snap strength must be 1–8"));
                            }
                            payload[3] |= (secondary - 1) << 3;
                        }
                    }
                    TriggerEffect::Galloping => {
                        let first = args.first_foot.unwrap_or(1);
                        if first > 6 || secondary <= first || secondary > 7 {
                            return Err(invalid(
                                "First foot must be 0–6; second foot must exceed first and be at most 7",
                            ));
                        }
                        if frequency == 0 {
                            return Err(invalid("Frequency must be 1–255"));
                        }
                        payload[0] = 0x23;
                        payload[3] = (first << 3) | secondary;
                        payload[4] = frequency;
                    }
                    TriggerEffect::Machine => {
                        if strength > 7 || secondary > 7 {
                            return Err(invalid("Machine strengths must be 0–7"));
                        }
                        if frequency == 0 {
                            return Err(invalid("Frequency must be 1–255"));
                        }
                        payload[0] = 0x27;
                        payload[3] = strength | (secondary << 3);
                        payload[4] = frequency;
                        payload[5] = period;
                    }
                    _ => unreachable!(),
                }
            }
        }
        Ok(Self {
            payload,
            mode,
            start,
            end,
            strength,
            frequency,
            secondary,
            period,
            zones: args.zones,
        })
    }
    fn label(self) -> String {
        match self.mode {
            TriggerEffect::Off => "Off baseline".into(),
            TriggerEffect::Resistance | TriggerEffect::Vibration => {
                let profile = self
                    .zones
                    .map(|z| format!("zones [{z}]"))
                    .unwrap_or_else(|| {
                        format!("zone {} · strength {}/8", self.start, self.strength)
                    });
                if self.mode == TriggerEffect::Resistance {
                    format!("Resistance · {profile}")
                } else {
                    format!("Vibration · {profile} · frequency {}", self.frequency)
                }
            }
            TriggerEffect::Weapon => format!(
                "Weapon break · zones {}–{} · strength {}/8",
                self.start, self.end, self.strength
            ),
            TriggerEffect::Bow => format!(
                "Bow · zones {}–{} · strength {}/8 · snap {}/8",
                self.start, self.end, self.strength, self.secondary
            ),
            TriggerEffect::Galloping => format!(
                "Galloping · zones {}–{} · feet {}/{} · frequency {}",
                self.start,
                self.end,
                self.payload[3] >> 3,
                self.secondary,
                self.frequency
            ),
            TriggerEffect::Machine => format!(
                "Machine · zones {}–{} · strengths {}/{} · frequency {} · period {}",
                self.start, self.end, self.strength, self.secondary, self.frequency, self.period
            ),
        }
    }
}

// Smooth RGB wheel, one revolution every 12 seconds. All colors remain available as custom RGB.
fn rainbow(seconds: f64) -> Rgb {
    let hue = seconds.rem_euclid(12.0) / 2.0;
    let rising = ((hue.fract() * 255.0).round()) as u8;
    let falling = 255 - rising;
    match hue as u8 {
        0 => Rgb(255, rising, 0),
        1 => Rgb(falling, 255, 0),
        2 => Rgb(0, 255, rising),
        3 => Rgb(0, falling, 255),
        4 => Rgb(rising, 0, 255),
        _ => Rgb(255, 0, falling),
    }
}

// USB output report 0x02. Volatile effects, separate from firmware feature reports.
fn report(effect: Effect, elapsed: f64) -> [u8; 63] {
    let mut data = [0u8; 63];
    data[0] = 2;
    match effect {
        Effect::Rumble => {
            data[1] = 3;
            data[3] = 40;
            data[4] = 40;
        }
        Effect::Lightbar {
            color,
            brightness,
            cycle,
        } => {
            data[2] = 4;
            let Rgb(r, g, b) = if cycle { rainbow(elapsed) } else { color }.dimmed(brightness);
            data[45..48].copy_from_slice(&[r, g, b]);
        }
        Effect::Triggers { side, test } => {
            // Right effect starts at byte 11, left at 22. Flags target only the chosen side.
            let offsets: &[usize] = match side {
                TriggerSide::Both => {
                    data[1] = 12;
                    &[11, 22]
                }
                TriggerSide::Left => {
                    data[1] = 8;
                    &[22]
                }
                TriggerSide::Right => {
                    data[1] = 4;
                    &[11]
                }
            };
            for offset in offsets {
                data[*offset..*offset + 11].copy_from_slice(&test.payload);
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

pub fn run(dev: &DualSenseHid, effect: Effect, seconds: u64) -> Result<()> {
    if !(1..=30).contains(&seconds) {
        return Err(invalid("Feedback duration must be 1–30 seconds"));
    }
    let label = effect.label();
    println!("{label} for {seconds}s. q / Esc / Ctrl-C stops the test in a terminal.");
    let cancellation = Cancellation::new()?;
    let terminal = if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        Some(TerminalGuard::enter()?)
    } else {
        None
    };
    let reset = Reset(dev);
    dev.write_output(&report(effect, 0.0))?;
    if terminal.is_some() {
        print!(
            "DS MAC TOOLS · {label}\r\n\r\nEnds after {seconds}s. q / Esc / Ctrl-C to stop.\r\n\r\n"
        );
        if matches!(effect, Effect::Triggers { .. }) {
            print!("Pull the selected trigger(s) to feel the effect.\r\n");
        }
        std::io::stdout().flush()?;
    }
    let start = Instant::now();
    let mut frame = Instant::now();
    let mut pulls = None;
    while start.elapsed() < Duration::from_secs(seconds) {
        if cancellation.requested() || (terminal.is_some() && stop_requested()?) {
            break;
        }
        if terminal.is_some() && matches!(effect, Effect::Triggers { .. }) {
            if let Some(raw) = dev.read_input(20)? {
                crate::input::decode(&raw)?;
                pulls = Some((raw[5], raw[6]));
            }
        } else {
            std::thread::sleep(Duration::from_millis(20));
        }
        if frame.elapsed() >= Duration::from_millis(33) {
            if matches!(effect, Effect::Lightbar { cycle: true, .. }) {
                dev.write_output(&report(effect, start.elapsed().as_secs_f64()))?;
            }
            if terminal.is_some() {
                let remaining = (seconds as f64 - start.elapsed().as_secs_f64()).max(0.0);
                print!("\r\x1b[2K{remaining:.1}s remaining");
                if let Some((left, right)) = pulls {
                    print!(
                        "   L2 {:3.0}%   R2 {:3.0}%",
                        f64::from(left) * 100.0 / 255.0,
                        f64::from(right) * 100.0 / 255.0
                    );
                }
                std::io::stdout().flush()?;
            }
            frame = Instant::now();
        }
    }
    // Explicit reset reports failures; Drop also attempts cleanup on every error path.
    dev.write_output(&reset_report())?;
    drop(reset);
    drop(terminal);
    println!("Effects stopped; trigger resistance disabled and lightbar control released.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    fn configured(options: &[&str]) -> Result<Effect> {
        let mut args = vec!["tool", "--feedback", "triggers"];
        args.extend_from_slice(options);
        Effect::from_args(&Args::try_parse_from(args).unwrap())
    }
    #[test]
    fn color_parsing_covers_full_rgb_and_rejects_bad_channels() {
        for value in ["#FF0080", "ff0080", "255,0,128", "255, 0, 128"] {
            assert_eq!(value.parse::<Rgb>().unwrap(), Rgb(255, 0, 128));
        }
        assert_eq!("sky blue".parse::<Rgb>().unwrap(), Rgb(30, 140, 255));
        assert_eq!("sky-blue".parse::<Rgb>().unwrap(), Rgb(30, 140, 255));
        assert_eq!("black".parse::<Rgb>().unwrap(), Rgb(0, 0, 0));
        for value in [
            "#FFF", "#GG0080", "256,0,0", "-1,0,0", "1,2", "1,2,3,4", "rainbow",
        ] {
            assert!(value.parse::<Rgb>().is_err());
        }
        assert_eq!(Rgb(255, 128, 0).dimmed(128), Rgb(128, 64, 0));
        assert_eq!(Rgb(255, 128, 0).dimmed(0), Rgb(0, 0, 0));
        for &(name, rgb) in COLORS {
            assert_eq!(name.parse::<Rgb>().unwrap(), rgb);
        }
    }
    #[test]
    fn rgb_and_cycle_reports_preserve_output_layout() {
        let light = report(
            Effect::Lightbar {
                color: Rgb(255, 128, 0),
                brightness: 128,
                cycle: false,
            },
            0.0,
        );
        assert_eq!(light[0], 2);
        assert_eq!(light.len(), 63);
        assert_eq!(light[1], 0);
        assert_eq!(light[2], 4);
        assert_eq!(&light[45..48], &[128, 64, 0]);
        for (seconds, rgb) in [
            (0.0, Rgb(255, 0, 0)),
            (2.0, Rgb(255, 255, 0)),
            (4.0, Rgb(0, 255, 0)),
            (6.0, Rgb(0, 255, 255)),
            (8.0, Rgb(0, 0, 255)),
            (10.0, Rgb(255, 0, 255)),
            (12.0, Rgb(255, 0, 0)),
        ] {
            assert_eq!(rainbow(seconds), rgb);
        }
    }
    #[test]
    fn trigger_reports_match_known_zone_vectors_and_target_sides() {
        let resistance = report(
            configured(&["--start", "4", "--strength", "2"]).unwrap(),
            0.0,
        );
        assert_eq!(resistance[1], 12);
        assert_eq!(
            &resistance[11..22],
            &[0x21, 0xf0, 0x03, 0x00, 0x90, 0x24, 0x09, 0, 0, 0, 0]
        );
        assert_eq!(&resistance[11..22], &resistance[22..33]);
        let weapon = report(
            configured(&[
                "--effect",
                "weapon",
                "--trigger",
                "left",
                "--start",
                "2",
                "--end",
                "8",
                "--strength",
                "8",
            ])
            .unwrap(),
            0.0,
        );
        assert_eq!(weapon[1], 8);
        assert_eq!(&weapon[11..22], &[0; 11]);
        assert_eq!(&weapon[22..33], &[0x25, 4, 1, 7, 0, 0, 0, 0, 0, 0, 0]);
        let vibration = report(
            configured(&[
                "--effect",
                "vibration",
                "--trigger",
                "right",
                "--start",
                "9",
                "--strength",
                "8",
                "--frequency",
                "255",
            ])
            .unwrap(),
            0.0,
        );
        assert_eq!(vibration[1], 4);
        assert_eq!(&vibration[22..33], &[0; 11]);
        assert_eq!(
            &vibration[11..22],
            &[0x26, 0, 2, 0, 0, 0, 0x38, 0, 0, 255, 0]
        );
        let off = report(configured(&["--effect", "off"]).unwrap(), 0.0);
        assert_eq!(off[11], 5);
        assert_eq!(off[22], 5);
    }
    #[test]
    fn invalid_effect_parameters_are_rejected_before_device_writes() {
        for flags in [
            vec!["--strength", "0"],
            vec!["--strength", "9"],
            vec!["--start", "10"],
            vec!["--end", "8"],
            vec!["--frequency", "25"],
            vec!["--effect", "vibration", "--frequency", "0"],
            vec!["--effect", "weapon", "--start", "1"],
            vec!["--effect", "weapon", "--start", "7", "--end", "7"],
            vec!["--effect", "off", "--strength", "2"],
        ] {
            assert!(configured(&flags).is_err());
        }
        let args =
            Args::try_parse_from(["tool", "--feedback", "rumble", "--color", "red"]).unwrap();
        assert!(Effect::from_args(&args).is_err());
        let args =
            Args::try_parse_from(["tool", "--feedback", "lightbar", "--effect", "weapon"]).unwrap();
        assert!(Effect::from_args(&args).is_err());
    }
    #[test]
    fn additional_modes_match_boundary_vectors() {
        let vectors: &[(&[&str], [u8; 11])] = &[
            (
                &[
                    "--effect",
                    "bow",
                    "--start",
                    "1",
                    "--end",
                    "8",
                    "--strength",
                    "3",
                    "--snap-strength",
                    "8",
                ],
                [0x22, 2, 1, 58, 0, 0, 0, 0, 0, 0, 0],
            ),
            (
                &[
                    "--effect",
                    "galloping",
                    "--start",
                    "0",
                    "--end",
                    "9",
                    "--first-foot",
                    "6",
                    "--second-foot",
                    "7",
                    "--frequency",
                    "4",
                ],
                [0x23, 1, 2, 55, 4, 0, 0, 0, 0, 0, 0],
            ),
            (
                &[
                    "--effect",
                    "machine",
                    "--start",
                    "1",
                    "--end",
                    "9",
                    "--strength",
                    "0",
                    "--strength-b",
                    "7",
                    "--frequency",
                    "255",
                    "--period",
                    "0",
                ],
                [0x27, 2, 2, 56, 255, 0, 0, 0, 0, 0, 0],
            ),
        ];
        for (flags, expected) in vectors {
            let actual = report(configured(flags).unwrap(), 0.0);
            assert_eq!(&actual[11..22], expected);
            assert_eq!(&actual[22..33], expected);
            assert!(actual[33..].iter().all(|v| *v == 0));
        }
    }
    #[test]
    fn per_zone_profiles_pack_active_bits_and_levels_independently() {
        let levels = "0,1,2,3,4,5,6,7,8,0";
        let resistance = report(configured(&["--zones", levels]).unwrap(), 0.0);
        assert_eq!(
            &resistance[11..22],
            &[0x21, 0xfe, 1, 0x40, 0x34, 0xd6, 0x07, 0, 0, 0, 0]
        );
        let vibration = report(
            configured(&[
                "--effect",
                "vibration",
                "--zones",
                levels,
                "--frequency",
                "1",
            ])
            .unwrap(),
            0.0,
        );
        assert_eq!(
            &vibration[11..22],
            &[0x26, 0xfe, 1, 0x40, 0x34, 0xd6, 0x07, 0, 0, 1, 0]
        );
        let disabled = report(
            configured(&["--zones", "0,0,0,0,0,0,0,0,0,0"]).unwrap(),
            0.0,
        );
        assert_eq!(&disabled[12..22], &[0; 10]);
        assert_eq!(levels.parse::<ZoneLevels>().unwrap().to_string(), levels);
        for invalid in [
            "",
            "1,2",
            "0,0,0,0,0,0,0,0,0",
            "0,0,0,0,0,0,0,0,0,0,0",
            "0,0,0,0,0,0,0,0,0,9",
            "0,0,0,0,0,0,0,0,0,-1",
            "0,0,0,0,0,0,0,0,0,x",
        ] {
            assert!(invalid.parse::<ZoneLevels>().is_err());
        }
    }
    #[test]
    fn mode_boundaries_and_unused_options_are_rejected() {
        let cases: &[&[&str]] = &[
            &["--effect", "bow", "--start", "0"],
            &["--effect", "bow", "--start", "8", "--end", "9"],
            &["--effect", "bow", "--snap-strength", "0"],
            &["--effect", "bow", "--snap-strength", "9"],
            &["--effect", "bow", "--end", "4"],
            &["--effect", "galloping", "--start", "9"],
            &["--effect", "galloping", "--end", "10"],
            &["--effect", "galloping", "--first-foot", "7"],
            &[
                "--effect",
                "galloping",
                "--first-foot",
                "3",
                "--second-foot",
                "3",
            ],
            &["--effect", "galloping", "--second-foot", "8"],
            &["--effect", "galloping", "--frequency", "0"],
            &["--effect", "machine", "--start", "0"],
            &["--effect", "machine", "--strength", "8"],
            &["--effect", "machine", "--strength-b", "8"],
            &["--effect", "machine", "--frequency", "0"],
            &["--effect", "machine", "--end", "10"],
            &["--effect", "off", "--start", "4"],
            &["--effect", "weapon", "--snap-strength", "2"],
            &["--effect", "bow", "--frequency", "4"],
            &["--effect", "galloping", "--strength", "2"],
            &["--effect", "vibration", "--period", "1"],
            &["--effect", "resistance", "--first-foot", "1"],
            &["--effect", "bow", "--strength-b", "2"],
            &["--effect", "machine", "--second-foot", "3"],
            &["--effect", "weapon", "--zones", "0,0,0,0,0,0,0,0,0,0"],
            &["--duration", "0"],
            &["--duration", "31"],
        ];
        for flags in cases {
            assert!(
                configured(flags).is_err(),
                "unexpectedly accepted: {flags:?}"
            );
        }
        // Directly constructed UI Args must also reject combinations that Clap rejects.
        let mut args = Args::try_parse_from(["tool", "--feedback", "triggers"]).unwrap();
        args.zones = Some(ZoneLevels([2; 10]));
        args.start = Some(4);
        assert!(Effect::from_args(&args).is_err());
        args.start = None;
        args.zones = Some(ZoneLevels([9; 10]));
        assert!(Effect::from_args(&args).is_err());
        args.zones = None;
        for mode in [
            TriggerEffect::Resistance,
            TriggerEffect::Weapon,
            TriggerEffect::Vibration,
            TriggerEffect::Off,
            TriggerEffect::Bow,
            TriggerEffect::Galloping,
            TriggerEffect::Machine,
        ] {
            args.effect = Some(mode);
            assert!(
                Effect::from_args(&args).is_ok(),
                "invalid defaults for {mode:?}"
            );
        }
    }
    #[test]
    fn cleanup_turns_off_both_triggers_rumble_and_releases_led_control() {
        let reset = reset_report();
        assert_eq!(&reset[0..5], &[2, 15, 8, 0, 0]);
        assert_eq!(reset[11], 5);
        assert_eq!(reset[22], 5);
        assert_eq!(&reset[12..22], &[0; 10]);
        assert_eq!(&reset[23..33], &[0; 10]);
        let rumble = report(Effect::Rumble, 0.0);
        assert_eq!(&rumble[1..5], &[3, 0, 40, 40]);
    }
}
