# DS Mac Tools

An easy way to update your DualSense from a Mac, with tools for game developers and controller testing.

Plug in your controller, open the app, and use the keyboard menu. No coding knowledge needed.

## Install

```sh
brew tap olliejudge/tap
brew trust --formula olliejudge/tap/dsmactools
brew install dsmactools
dsmactools
```

Older Homebrew versions without `brew trust` can skip that line. Already using this tap? You only need `brew install dsmactools`.

## Update your controller

1. Connect your DualSense with a **USB data cable**.
2. Choose **Firmware & controller details → Install latest firmware**.
3. Confirm the update, then keep the cable connected and your Mac awake until verification finishes.

The app finds the matching official Sony firmware, checks compatibility and battery level, and reads back the installed version. If you're already up to date, it tells you. Firmware installation carries risk; after an error, reconnect and check the installed version before retrying.

## Test and develop

| Tool | What it does |
| --- | --- |
| Live input monitor | Sticks, buttons, triggers, touch and raw motion data |
| Session recording | Raw and decoded USB inputs saved as JSONL, with timing and axis summaries |
| Feedback lab | Every RGB lightbar color, rainbow cycling and seven adaptive trigger modes |
| Developer diagnostics | Xcode, Swift, Metal and native macOS GameController capabilities |
| Firmware | Check, download or install official updates |

The menu uses arrow keys and Enter; Esc returns or exits. Live tools stop with q, Esc or Ctrl-C. Feedback tests reset their effects when they finish. Close games and other controller tools before USB tests.

```sh
dsmactools --monitor
dsmactools --record session.jsonl --duration 10
dsmactools --feedback rumble --duration 3
dsmactools --diagnostics
dsmactools --check
dsmactools --print-firmware-info --json
dsmactools --help
```

Capture timing measures host report arrival, not game latency. [Capture format](docs/captures.md) · [Lightbar and trigger testing](docs/feedback.md).

## Support

Standard DualSense USB operation is hardware-tested, including a Type `000B` firmware update from `0580` to `0630`. Edge is detected but not hardware-tested. Direct input, feedback and firmware tools require USB; native macOS diagnostics can also see other supported controllers and Bluetooth devices.

Apple Silicon is hardware-tested. Firmware downloads are cached locally and are not bundled with the app. Sony's official [PlayStation Accessories](https://controller.dl.playstation.net/controller/lang/en/2100004.html) updater is available for Windows.

To update DS Mac Tools itself: `brew update && brew upgrade dsmactools`.

[Build and protocol notes](docs/research.md) · [Release and Homebrew guide](docs/releasing.md)

## Credits

Derived from [dualsense-updater-rs](https://github.com/nchie/dualsense-updater-rs), preserving its MIT license and history. Terminal interaction inspired by [Mole](https://github.com/tw93/Mole). Protocol references are in the research notes.

Independent, unofficial software. **Not affiliated with or endorsed by Sony or PlayStation.** Firmware belongs to Sony. MIT licensed; see [LICENSE](LICENSE).
