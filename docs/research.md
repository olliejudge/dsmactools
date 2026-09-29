# Research and validation, 2026-09-29

## Existing projects

- [nchie/dualsense-updater-rs](https://github.com/nchie/dualsense-updater-rs): Rust CLI with a native macOS HIDAPI path. Initial implementation `a7ec8ad`, 2026-01-27. Latest inspected commit `c1ef2ca`, 2026-08-04, only adds a BDM-060 README mapping. MIT license, retained here with Git history.
- [Open PR #4](https://github.com/nchie/dualsense-updater-rs/pull/4) adds BDM-040 -> 0004 documentation and reports a successful update. It does not change the protocol.
- [Issue #1](https://github.com/nchie/dualsense-updater-rs/issues/1) reports an SFEATURE error on Linux at the first header packet. This is an unresolved report, not a confirmed permissions diagnosis.
- [Issue #5](https://github.com/nchie/dualsense-updater-rs/issues/5) illustrates the problem with manually choosing firmware. Its reported future-dated build is not evidence of a real newer Sony image.
- [nowrep/dualsensectl](https://github.com/nowrep/dualsensectl/blob/main/main.c) now implements firmware updates, but the full tool depends on Linux/libudev. Its source uses 64-byte padded update reports, a delay after the first header packet, a delay between writes, image PID/version header checks and a battery check. These informed this fork; its phase-status meanings differ in some places, so this fork retains the Rust upstream's phase-specific status interpretation.
- [daidr/dualsense-tester](https://github.com/daidr/dualsense-tester/blob/main/src/router/DualSense/views/_ConnectPanel/FactoryInfo.vue) and dualsensectl agree on feature report 0x20 offsets. Reports include the report ID at byte 0, date at 1..12, time at 12..20, firmware type at 20..22, software series at 22..24, hardware info at 24..28 and update version at 44..46.
- [Paliverse firmware archive](https://github.com/Paliverse/DualSense-List-of-Firmwares) provides independent recorded hashes and a maintained catalogue watcher. Images used here come directly from Sony, not the archive.

## Official sources

[PlayStation Accessories](https://controller.dl.playstation.net/controller/lang/en/2100004.html) supports Windows 10/11. No official native Mac updater was found.

Sony's [live catalogue](https://fwupdater.dl.playstation.net/fwupdater/info.json), fetched directly on 2026-09-29, advertised:

| Target | Latest |
| --- | --- |
| 0004 | 0x0630 |
| 000B | 0x0630 |
| 000E | 0x0641 |
| 0044 | 0x0217 |

Official URL pattern: `https://fwupdater.dl.playstation.net/fwupdater/fwupdate<TARGET>/0x<VERSION>/FWUPDATE<TARGET>.bin`.

## Selection and implementation findings

The connected controller's report contained PID `0x0ce6`, software series `0x000b`, firmware type `2`, hardware info `0x00001107`, update version `0x0580` and build `Nov 1 2024 17:49:16`.

The 000B/0x0630 image header independently contained PID `0x0ce6` at 0x62, header value `2` at 0x60, series `0x000b` at 0x74 and version `0x0630` at 0x78. Its size was 950,272 bytes. The controller/image series agreement is the basis for selection; no retail-color or BDM guess is used. Firmware header interpretation remains reverse-engineered, with unsupported combinations refused.

Upstream's date parser included the report ID in the date, exposed individual write phases, selected the first device without rejecting ambiguity, used unbounded polling, could accept StartUpdate retry as success, and reported no installed-version verification. This fork addresses those issues and adds live catalogue downloads, validation and regression tests.

## Hardware validation

On 2026-09-29 this controller was successfully updated over USB from `0x0580` to `0x0630` using this fork on Apple Silicon/macOS 27.0. Its battery report indicated 100%. All 29 image blocks were written, VerifyUpdate returned success, and FinalizeUpdate was sent. A newly opened HID handle then reported `0x0630`, build `Jul 4 2025 10:38:40`, with the same software series and hardware information.

Official image source: `https://fwupdater.dl.playstation.net/fwupdater/fwupdate000B/0x0630/FWUPDATE000B.bin`.

SHA-256: `b66528795683d214ca06fe4672d298de75bf019bd5046442cbec63fc02974f86`, also matching the Paliverse archive entry. Byte count: 950,272.

This is one standard DualSense hardware test. Edge and other hardware revisions are not hardware-tested by this project. Protocol tests cannot substitute for hardware coverage.

At the initial firmware validation, ten regression tests, Rust formatting, Clippy with warnings denied and the release build passed. The live Sony catalogue watcher reported no changes against the saved baseline. Subsequent workbench and release validation is recorded below.


The final read from the finished build independently confirmed version `0x0630` and a new macOS HID path after re-enumeration. Its firmware report type had changed from `2` to `3`, while software series stayed `000B` and hardware info stayed `0x00001107`. The image header field at 0x60 (`2`) therefore must not be equated to the firmware report type. The final validator checks PID and software series, and a regression test covers this transition so later updates are not incorrectly blocked.


## DS Mac Tools 0.3.0

The executable was renamed to `ds-mac-tools` and given an interactive menu inspired by Mole. Read-only menu navigation was exercised in a real pseudo-terminal against the updated controller: automatic USB detection, 100% battery, live current-version status, Controller details, return to menu, and Escape to exit. JSON controller inspection remains available. No second hardware flash was needed for the terminal presentation changes. Regression tests cover the shared update guards, CLI mode selection and cache override.

Homebrew source installation is provided in the existing `olliejudge/homebrew-tap` as an independent formula, alongside the untouched Orion cask. Source installation avoids a requirement for distributing a signed/notarized executable. The release source archive is checksum-pinned and builds with Cargo's committed lockfile.


The catalogue watcher is now implemented in Rust as `dsmactools --check-catalogue`. It shares the application’s HTTPS/JSON path and compares all published keys with an embedded release baseline. This command does not enumerate or open HID devices, and returns a failure when versions, keys or metadata shape change. The earlier standalone Python helper was removed.

## Developer workbench validation (2026-09-29)

The 0.3.0 executable/formula is `dsmactools`. The workbench adds live USB input inspection, JSONL captures, timed output presets, and Mac environment/native GameController diagnostics, all implemented in Rust. USB report layouts and trigger effect fields were checked against the documented structures in [dualsensectl](https://github.com/nowrep/dualsensectl/blob/master/main.c); the implementation sends independent 0x02 output reports rather than firmware feature commands. Motion remains raw rather than assuming calibrated units.

On the same standard Type 000B controller with firmware 0630:

- A two-second capture recorded 502 correctly sized raw/decoded reports, about 250 reports/s. Metadata, every sample and the final count were parsed and checked. This is host arrival rate, not game latency.
- Terminal menu navigation, live input rendering and q/Esc return paths were exercised using a real PTY.
- One-second rumble, blue-lightbar and mild adaptive-trigger output transfers completed, followed by reset transfers. Physical sensation/color were not independently measured; successful HID transfer is the tested result.
- Native GameController discovery exposed the DualSense's extended gamepad, motion, haptics, light, battery, 10 axis aliases and 34 button aliases. Discovery must initialize before pumping the main run loop; querying for the first time after waiting gave an empty snapshot.
- macOS 27.0, arm64, Xcode 27.1 beta, Swift 6.4 and the Metal compiler were detected. Missing tools remain explicit availability failures rather than being installed by the app.
- Concurrent HID clients can cause exclusive-access errors. Tests were then run sequentially. Users should close other controller tools.

The 16 Rust tests cover malformed reports, button/touch/signed-motion decoding, output report flags/effect reset, CLI action separation, catalogue changes, firmware compatibility and update sequencing. No additional firmware flash was performed for these UI/developer checks. Edge feedback/input and Intel execution are not hardware-tested.

## Release and Homebrew validation (0.3.1)

The stable v0.3.1 release is published with checksummed source and separate Apple Silicon and Intel/macOS 26 bottles. Both clean runner jobs passed strict online formula audit, source build, hardware-free formula tests and bottle publication. The final workflow commit also passed macOS and Linux Rust CI. Intel package execution was tested on the hosted Intel runner; Intel controller hardware remains untested.

On this Apple Silicon Mac, `brew install dsmactools` poured the published bottle, installed version 0.3.1 with no Homebrew runtime dependencies, and passed direct version/help, live catalogue and USB firmware inspection checks. The colored menu was exercised in a PTY, reporting firmware 0630 and exiting cleanly with Esc. Terminal colors respect `NO_COLOR`.

Local Homebrew audit/test commands were blocked by the separately installed outdated Command Line Tools, despite current full Xcode. Those checks passed on the clean release runners; system developer tools were not modified.

Independent security reviews covered tracked files and Git history, the tagged source archive, tap changes, and both actual bottle archives/binaries. No credentials, personal machine paths, device captures or firmware images were found in the published artifacts. Bottle build paths and receipts refer to generic hosted runners and Homebrew staging. The local working instructions and audit reports are excluded from Git.

## Expanded feedback lab (0.4.0)

The feedback lab covers full 24-bit RGB, sixteen named choices including off, brightness scaling and a twelve-second rainbow cycle. Adaptive triggers expose resistance, weapon, vibration, bow, galloping, machine and an off baseline, with independent L2/R2 selection and ten-zone resistance/vibration profiles. Report fields and parameter bounds were independently reviewed against [dualsensectl's primary implementation](https://github.com/nowrep/dualsensectl/blob/main/main.c). Protocol facts inform an independent Rust implementation; no unknown raw effect modes are exposed.

All 24 Rust tests, formatting, Clippy with warnings denied and the locked release build passed. Tests include known report vectors, side flags, per-zone packing, mode-specific bounds, incompatible options, full RGB parsing and reset reports. Feedback settings and duration are validated before hardware opens.

On the same USB-connected standard Type 000B controller at firmware 0630, short transfers completed for all seven modes, a custom zone profile, custom RGB/brightness, off and rainbow cycling, followed by successful reset transfers. The terminal menu accepted defaults, retried invalid custom color input, cancelled a text prompt with Esc and displayed live L2/R2 input percentages. A thirty-second off-baseline test was stopped early with q and reset successfully. No firmware flash was performed.

These checks verify USB transfer and input/terminal behavior, not measured physical color, force or oscillation quality. Frequency, period, zone and strength values remain protocol settings. Edge hardware remains untested. [Feedback settings and examples](feedback.md).

The privacy review found no telemetry/analytics dependency or application upload path. Controller inputs, captures and diagnostics use local interfaces, files and standard output; firmware requests fetch only fixed Sony catalogue/image URLs. Curl's first argument is `--disable` so local curl configuration cannot add unrelated requests or uploads. A live catalogue check succeeded with a temporary curl config containing an extra URL and an output marker that would invalidate the JSON if inherited.
