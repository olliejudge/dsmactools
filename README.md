# DS Mac Tools

**A controller workbench for Mac game developers and testers.**

Inspect live inputs, record test sessions, try controller feedback, check your Mac development environment, and keep DualSense firmware current. One Rust terminal application, with a keyboard menu and scriptable commands.

```text
  DS MAC TOOLS
  A controller workbench for Mac developers.

  ●  DualSense Wireless Controller
     USB connected  ·  100% battery
     Up to date  ·  Firmware 0630

  ❯ Live input monitor
    Record a test session
    Feedback lab
    Developer diagnostics
    Firmware & controller details
    Refresh / choose controller
    About DS Mac Tools
    Quit
```

## Install

Add the tap once, then install:

```sh
brew tap olliejudge/tap
brew install dsmactools
```

Or use the fully qualified command:

```sh
brew install olliejudge/tap/dsmactools
```

Connect a DualSense with a **USB data cable** and run `dsmactools`. Use arrow keys and Enter; Esc returns or exits. The existing Orion cask in this tap is independent and unchanged.

The source formula installs Rust as a build dependency. Running the tool needs no Apple Developer account, driver replacement or administrator access. Apple Silicon is hardware-tested; Intel builds use the same source formula. This release is distributed through our tap, not Homebrew core. On a fresh machine, bare `brew install dsmactools` requires the tap setup above.

```sh
brew update
brew upgrade dsmactools
```

## The workbench

| Tool | Use |
| --- | --- |
| Live input monitor | Sticks, buttons, triggers, touch contacts, raw gyro and accelerometer values; host report rate |
| Session capture | JSONL metadata, raw USB packets, decoded inputs, timing, axis ranges and buttons seen |
| Feedback lab | Gentle rumble, blue lightbar and mild adaptive trigger resistance, with timed cleanup |
| Developer diagnostics | macOS/architecture, Xcode/Swift/Metal tool availability, USB controllers and Apple's native GameController profiles |
| Firmware | Live Sony catalogue, validated downloads, battery preflight, installation and installed-version verification |

Input tools are read-only. Feedback tests change volatile controller effects, stop after the chosen duration, and attempt cleanup on cancellation or errors. Cleanup disables rumble/resistance and releases lightbar control; it does not reconstruct another app's previous effects. Close games and other controller tools before USB tests. Abrupt process termination or cable loss can prevent cleanup; reconnect the controller if needed.

Capture timing measures **host report arrival**, not game latency or button-to-screen latency. Motion values are raw sensor counts, without calibration or conversion to physical units. Stick values are normalized to −1…1 without a dead zone. The native GameController snapshot is distinct from direct USB HID access: a device visible over USB may not appear in the framework snapshot. Developer diagnostics also work without a DualSense attached and can list other controllers exposed by macOS.

## Commands

```sh
dsmactools --monitor
dsmactools --record session.jsonl --duration 10
dsmactools --feedback rumble --duration 3
dsmactools --feedback lightbar --duration 5
dsmactools --feedback triggers --duration 5
dsmactools --diagnostics > diagnostics.json

dsmactools --check
dsmactools --print-firmware-info --json
dsmactools --download-latest
dsmactools --update-latest
dsmactools --list
dsmactools --check-catalogue
dsmactools --help
```

Capture files are never overwritten. Durations are 1–3600 seconds for recordings and 1–30 seconds for feedback; the menu uses five-second feedback tests. Press q, Esc or Ctrl-C to stop a live tool. Captures contain one metadata row, sample rows with `elapsed_us`, `raw` and `input`, then a summary row. `cancelled` indicates an early stop and `complete` indicates whether the recording finished without an error. Capture schema version is 1.

A terminal opens the menu by default. Non-interactive runs default to a read-only firmware check. `--interactive` requires a terminal. `--yes` is accepted only with a firmware write command. Use `--path PATH` to select an exact USB interface; ambiguous device selections are refused. For Edge CLI actions, add `--pid 0x0df2`.

Firmware and source manifests are cached in `~/Library/Caches/ds-mac-tools/firmware`, including the URL, size and SHA-256 checksum. Use `--cache-dir DIRECTORY` to override it. Sony firmware is downloaded separately and never bundled in this project.

## Hardware support

| Controller / connection | Status |
| --- | --- |
| Standard DualSense, USB | Input monitor/capture and output transfers tested; Type `000B` firmware update tested from `0580` to `0630` |
| Standard DualSense Types `0004` / `000E` | Firmware branches recognized; not hardware-tested here |
| DualSense Edge, USB | Detected and common protocol supported; not hardware-tested here; extra Edge controls are not decoded |
| Bluetooth | May appear in native GameController diagnostics; direct input/feedback/firmware tools require USB |
| Other controllers | Native GameController diagnostics only |

## Firmware installation

The updater selects firmware using the controller's actual software series. It validates the image's model, branch, size and version, requires at least 10% battery, keeps all update phases together, and reopens the controller to verify the installed version. It refuses unknown branches, reflashing and downgrades.

Keep the USB cable connected and your Mac awake until verification finishes. Firmware writes carry a risk of leaving a device unusable. Writing can commit before verify/finalize completes. If an update reports an error, reconnect and check the installed version before retrying. A recorded SHA-256 is not an independent authenticity signature; the controller performs firmware authentication.

Sony's official updater, [PlayStation Accessories](https://controller.dl.playstation.net/controller/lang/en/2100004.html), is available for Windows.

## Build and maintain

```sh
git clone https://github.com/olliejudge/dsmactools.git
cd dsmactools
cargo build --release --locked
./target/release/dsmactools
```

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked -- -D warnings
cargo run --locked -- --check-catalogue
```

GitHub Actions tests macOS and Linux. A daily **Rust** catalogue check flags changed, added or removed Sony metadata for review; it never flashes hardware or silently approves new firmware formats. There is no Python runtime or catalogue helper. See the [research log](docs/research.md), [capture format](docs/captures.md), and [release guide](docs/releasing.md).

## Credits and independence

Inspired by and derived from [nchie/dualsense-updater-rs](https://github.com/nchie/dualsense-updater-rs), with its original MIT license and history preserved. Protocol research references [nowrep/dualsensectl](https://github.com/nowrep/dualsensectl), [daidr/dualsense-tester](https://github.com/daidr/dualsense-tester) and [Apple's GameController documentation](https://developer.apple.com/documentation/gamecontroller). The keyboard terminal experience is inspired by [Mole](https://github.com/tw93/Mole).

**Independent, unofficial software. Not affiliated with, sponsored by, approved by or endorsed by Sony Interactive Entertainment or PlayStation.** DualSense and PlayStation are trademarks of their respective owners. Firmware remains Sony's property.

MIT licensed. See [LICENSE](LICENSE).
