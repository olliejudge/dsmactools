# dsmactools

A maintained macOS-first fork of [nchie/dualsense-updater-rs](https://github.com/nchie/dualsense-updater-rs), based on `c1ef2ca`. Checks the connected controller against Sony's live firmware catalogue and downloads official firmware on demand.

## Build

Install Rust if needed (`brew install rust`), then:

```sh
cargo build --release --locked
```

macOS uses HIDAPI's native IOKit backend. No libusb driver replacement or root access is required. A sandboxed terminal/agent may require permission to access USB HID devices.

## Use

Connect a standard DualSense using a USB data cable.

```sh
# Read-only controller information
./target/release/dualsense-updater --print-firmware-info
./target/release/dualsense-updater --print-firmware-info --json

# Compare with Sony's current catalogue; also the default with no arguments
./target/release/dualsense-updater --check

# Download and validate without flashing
./target/release/dualsense-updater --download-latest

# Download, validate, ask for confirmation, install, and read back the version
./target/release/dualsense-updater --update-latest

# Explicitly confirm installation without a prompt
./target/release/dualsense-updater --update-latest --yes
```

Downloaded images and their source URL, byte count and SHA-256 manifest are kept under `firmware/`, excluded from Git. Each check fetches Sony's current catalogue; the version is not hard-coded. A local image can be supplied as a positional path and passes the same compatibility checks. Already installed versions and downgrades are refused before writing.

Use `--list` to enumerate HID paths and `--path PATH` when more than one controller/interface is attached. The selected path must match the VID/PID and be USB. Edge enumeration is available with `--pid 0x0df2`; **Edge flashing has not been tested in this project**.

## Update behavior

This is an unofficial updater using Sony's firmware. Sony's official PC application currently requires Windows. Keep the Mac awake, close games and other controller tools, and leave the cable connected throughout installation. The upstream author observed that the device can commit an update while writing, before verify/finalize commands are run.

Before writing, the tool checks USB transport, unambiguous selection, battery state, a supported controller software series, the firmware header's PID/series, its version and the known 950,272-byte image format. Unknown targets and changed image formats stop for review. SHA-256 records the downloaded bytes; it is not an independent authenticity signature. The controller performs its own firmware authentication.

Packets use padded 64-byte HID reports and bounded status polling. Retry/busy states never count as success. The image is read once before transfer. Individual destructive debug phases from upstream have been removed. After transfer, the tool closes the old handle and waits up to 30 seconds for the controller to reappear with the expected firmware, software series and hardware information. Transfer completion alone is not reported as a verified update.

If an update returns an error, reconnect and read `--print-firmware-info` before retrying. The controller may already have committed the image. The updater does not automatically retry the entire flash.

## Maintenance

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked -- -D warnings
python3 scripts/check_catalogue.py
```

The catalogue check compares Sony's latest versions with `catalogue-baseline.json` and fails when versions or target keys change, prompting review. GitHub Actions runs that check daily and tests on macOS and Linux. These workflows become active after publishing this repository to GitHub. They never flash hardware or silently accept new firmware formats. Review a new catalogue entry before updating the baseline and compatibility checks.

Research, upstream provenance, protocol references and hardware validation are in [docs/research.md](docs/research.md).

MIT. The original upstream license is preserved in `LICENSE`. Sony firmware is downloaded separately and is not licensed by this repository.
