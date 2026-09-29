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

Final local validation: ten regression tests passed, Rust formatting and Clippy with warnings denied passed, the release build succeeded, and the live Sony catalogue watcher reported no changes against the saved baseline. GitHub CI has been configured but has not run remotely.


The final read from the finished build independently confirmed version `0x0630` and a new macOS HID path after re-enumeration. Its firmware report type had changed from `2` to `3`, while software series stayed `000B` and hardware info stayed `0x00001107`. The image header field at 0x60 (`2`) therefore must not be equated to the firmware report type. The final validator checks PID and software series, and a regression test covers this transition so later updates are not incorrectly blocked.
