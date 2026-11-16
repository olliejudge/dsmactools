# DS Mac Tools

An easy way to update your DualSense from a Mac, with tools for game developers and controller testing.

Plug in your controller, open the app, and use the keyboard menu. No coding knowledge needed.

## Install

You'll need [Homebrew](https://brew.sh), a USB data cable, and a Mac running macOS 15 Sequoia or later (Apple Silicon or Intel). Older Macs can use the [install guide](docs/install.md).

```sh
brew tap olliejudge/tap
brew trust --formula olliejudge/tap/dsmactools
brew install dsmactools
dsmactools
```

Older Homebrew versions without `brew trust` can skip that line. Already using this tap? You only need `brew install dsmactools`.

If Homebrew says it is installing or building `rust`, your Mac doesn't have a ready-made package and the build can take hours. Press Control-C and follow the [install guide](docs/install.md) instead.

## Update your controller

1. Connect your DualSense with a **USB data cable**.
2. Choose **Firmware & controller details → Install latest firmware**.
3. Confirm the update, then keep the cable connected and your Mac awake until verification finishes.

The app finds the matching official Sony firmware, checks compatibility and battery level, and reads back the installed version. If you're already up to date, it tells you. Firmware installation carries risk; after an error, reconnect and check the installed version before retrying.

Writing firmware to the controller over USB can take several minutes, even though the download is small. The progress bar shows elapsed time and estimated transfer time remaining; keep the controller connected through the final verification.

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

## Privacy

**No telemetry or analytics.** Controller inputs, recordings and diagnostic output stay on your Mac and are never uploaded by the app. Its only network requests are firmware catalogue checks and downloads directly from Sony.

## Support

Standard DualSense USB operation is hardware-tested, including a Type `000B` firmware update from `0580` to `0630`. Edge is detected but not hardware-tested. Direct input, feedback and firmware tools require USB; native macOS diagnostics can also see other supported controllers and Bluetooth devices.

Apple Silicon is hardware-tested. Intel Macs are supported, but controller updates on Intel have not yet been tested by the maintainer. Firmware downloads are saved in `~/Library/Caches/ds-mac-tools` and are not bundled with the app. Sony's official [PlayStation Accessories](https://controller.dl.playstation.net/controller/lang/en/2100004.html) updater is available for Windows.

To update DS Mac Tools itself: `brew update && brew upgrade dsmactools`.

[Install guide](docs/install.md) · [Build and protocol notes](docs/research.md) · [Release and Homebrew guide](docs/releasing.md)

## Uninstall

```sh
brew uninstall dsmactools
rm -rf ~/Library/Caches/ds-mac-tools
```

The second line removes downloaded firmware. You can also run `brew untap olliejudge/tap` if you don't use anything else from the tap. Installed another way? See the [install guide](docs/install.md#install-with-rust-instead).

## Credits

Derived from [dualsense-updater-rs](https://github.com/nchie/dualsense-updater-rs), preserving its MIT license and history. Terminal interaction inspired by [Mole](https://github.com/tw93/Mole). Protocol references are in the research notes.

Independent, unofficial software. **Not affiliated with or endorsed by Sony or PlayStation.** Firmware belongs to Sony. MIT licensed; see [LICENSE](LICENSE).
