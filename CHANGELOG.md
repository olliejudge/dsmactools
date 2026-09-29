# Changelog

## 0.3.0

- Rename the application to DS Mac Tools and the executable to `ds-mac-tools`.
- Add an interactive terminal menu with controller status, battery level, live update checks, controller selection, downloads and installation.
- Show progress during firmware transfer and keep installation confirmation enabled in the menu.
- Store downloaded firmware in the user's cache, with an optional directory override.
- Add Homebrew installation through `olliejudge/tap`.

## 0.2.0

- Add live Sony catalogue checks, automatic firmware branch selection, validated downloads, USB/battery preflight and installed-version verification.
- Fix firmware-report parsing and retry handling; remove unguarded per-phase update commands.
- Verify a standard Type 000B DualSense update from `0580` to `0630` on macOS.

## Upstream

Derived from nchie/dualsense-updater-rs at `c1ef2ca`, under the MIT license.
