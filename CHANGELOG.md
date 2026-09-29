# Changelog

## 0.4.0

- Expand the lightbar lab with named colors, custom RGB/hex values, brightness and a smooth rainbow cycle.
- Add configurable resistance, weapon, bow, galloping, machine and vibration effects, plus an off baseline, for L2, R2 or both.
- Add per-zone resistance/vibration profiles and live trigger pull values during tests.
- Keep every feedback test bounded to 1–30 seconds with cancellation and automatic reset.
- State the privacy policy in the README and app: no telemetry or analytics; inputs, recordings and diagnostics stay local.
- Ignore local curl configuration during firmware requests.

## 0.3.1

- Simplify the README for everyday firmware updates and developer testing.
- Add color to the live input display.
- Ignore local captures, diagnostics and environment files.
- Add reproducible Homebrew bottle builds for Apple Silicon and Intel.

## 0.3.0

- Rename the application to DS Mac Tools and the executable to `dsmactools`.
- Add an interactive terminal menu with controller status, battery level, live update checks, controller selection, downloads and installation.
- Show progress during firmware transfer and keep installation confirmation enabled in the menu.
- Store downloaded firmware in the user's cache, with an optional directory override.
- Add the live USB input monitor and JSONL session recording with arrival timing and axis/button summaries.
- Add timed rumble, lightbar and adaptive trigger presets with cancellation cleanup.
- Add Mac developer diagnostics and native GameController capability inspection through Rust bindings.
- Move the Sony catalogue checker into Rust and remove the Python helper.
- Add Homebrew installation as `dsmactools` through `olliejudge/tap`.

## 0.2.0

- Add live Sony catalogue checks, automatic firmware branch selection, validated downloads, USB/battery preflight and installed-version verification.
- Fix firmware-report parsing and retry handling; remove unguarded per-phase update commands.
- Verify a standard Type 000B DualSense update from `0580` to `0630` on macOS.

## Upstream

Derived from nchie/dualsense-updater-rs at `c1ef2ca`, under the MIT license.
