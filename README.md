# DS Mac Tools

**Keep your DualSense up to date, from your Mac.**

DS Mac Tools is a small, interactive terminal app for checking and updating PlayStation DualSense controller firmware over USB. Plug in your controller, open the menu, and it checks Sony's latest firmware for your hardware.

```text
  DS MAC TOOLS
  Your DualSense, up to date.

  ●  DualSense Wireless Controller
     USB connected  ·  100% battery
     Up to date  ·  Firmware 0630

  ↑/↓ to move  ·  Enter to select  ·  Esc to quit
  ❯ Check for updates
    Install latest firmware
    Controller details
    Download firmware
    Refresh / choose controller
    About DS Mac Tools
    Quit
```

## Install

With [Homebrew](https://brew.sh):

```sh
brew install olliejudge/tap/ds-mac-tools
```

Then connect your controller with a **USB data cable** and run:

```sh
ds-mac-tools
```

The menu shows your controller's firmware, battery level, and whether an update is available. Use the arrow keys and Enter to choose an action. Installation asks for confirmation before writing firmware.

The first Homebrew release builds locally from a checksum-verified source archive, with Rust installed as a build dependency. It supports Apple Silicon and Intel Macs. No Apple Developer account, driver replacement, or administrator access is required to run the tool.

To update DS Mac Tools itself:

```sh
brew update
brew upgrade olliejudge/tap/ds-mac-tools
```

## What it does

- Detects USB-connected controllers and lets you choose between them.
- Reads the controller's software series to select its matching firmware branch.
- Checks Sony's live catalogue each time you refresh or request an update.
- Downloads firmware directly from Sony and validates its model, branch, size and version.
- Checks the controller's battery before installation and shows transfer progress.
- Reopens the controller after installation and reads back the installed version.

Already-current firmware, reflashing and downgrades are skipped or refused. A higher version for another hardware branch does not mean your controller is out of date.

## Command-line use

For scripts, troubleshooting, or a quick check:

```sh
ds-mac-tools --check                       # Check for an update without installing
ds-mac-tools --print-firmware-info         # Read controller details
ds-mac-tools --print-firmware-info --json  # Machine-readable details
ds-mac-tools --download-latest             # Download and validate without installing
ds-mac-tools --update-latest               # Install after confirmation
ds-mac-tools --list                        # List available HID interfaces
ds-mac-tools --help
```

A terminal opens the interactive menu by default. Non-interactive runs default to a read-only update check. `--interactive` explicitly opens the menu and requires a terminal. `--yes` skips installation confirmation only when paired with an update command. Use `--path PATH` to select an exact USB HID interface for command-line actions.

Firmware and source manifests are saved under `~/Library/Caches/ds-mac-tools/firmware`, with their URL, size and SHA-256 checksum. Use `--cache-dir DIRECTORY` to choose a different location. Firmware files are not bundled with the app or committed to this repository.

## Controller support

| Controller | Status |
| --- | --- |
| Standard DualSense | USB update tested on Type `000B`, from `0580` to `0630` |
| Standard DualSense Types `0004` and `000E` | Firmware branch recognized; not hardware-tested here |
| DualSense Edge | Detected; update path available, but not hardware-tested here |
| Bluetooth connections | Firmware installation not supported |

The interactive menu detects both standard and Edge controllers. For Edge command-line actions, add `--pid 0x0df2`.

## Before installing firmware

Keep the USB cable connected and your Mac awake until the installed version is verified. Close games and other software using the controller. The tool requires at least 10% battery and refuses unknown firmware branches or image formats.

Controller firmware writes carry a risk of leaving the device unusable. The original updater's author observed that writing can commit an update before the verify/finalize steps. If an installation reports an error, reconnect and check the installed version before retrying; the update may already have committed. SHA-256 records the downloaded file and is not an independent authenticity signature. The controller performs its own firmware authentication.

Sony's official updater, [PlayStation Accessories](https://controller.dl.playstation.net/controller/lang/en/2100004.html), is available for Windows.

## Build and contribute

```sh
git clone https://github.com/olliejudge/dsmactools.git
cd dsmactools
cargo build --release --locked
./target/release/ds-mac-tools
```

Before submitting changes:

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked -- -D warnings
python3 scripts/check_catalogue.py
```

GitHub Actions tests the project on macOS and Linux. A daily check flags changes to Sony's firmware catalogue for review. It never flashes hardware or silently accepts a new firmware format. Protocol sources, implementation notes and real hardware validation are in [the research log](docs/research.md). Release and Homebrew maintenance are documented in [the release guide](docs/releasing.md).

## Credits and independence

DS Mac Tools is inspired by and derived from [nchie/dualsense-updater-rs](https://github.com/nchie/dualsense-updater-rs). Its original MIT license and commit history are preserved. Firmware protocol research also draws on [nowrep/dualsensectl](https://github.com/nowrep/dualsensectl) and [daidr/dualsense-tester](https://github.com/daidr/dualsense-tester). The keyboard-driven terminal experience is inspired by [Mole](https://github.com/tw93/Mole).

**This is an independent, unofficial project. It is not affiliated with, sponsored by, approved by, or endorsed by Sony Interactive Entertainment or PlayStation.** DualSense and PlayStation are trademarks of their respective owners. Firmware remains Sony's property and is downloaded separately from Sony's servers.

MIT licensed. See [LICENSE](LICENSE).
