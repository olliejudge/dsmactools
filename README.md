# DS Mac Tools

Update the firmware on your PlayStation DualSense controller from a Mac. It runs in Terminal and you control it with the keyboard. No coding knowledge needed.

## Install

You'll need [Homebrew](https://brew.sh), a USB data cable, and a Mac running macOS 15 Sequoia or later (Apple Silicon or Intel). Open Terminal and run:

```sh
brew tap olliejudge/tap
brew trust --formula olliejudge/tap/dsmactools
brew install dsmactools
dsmactools
```

Older Homebrew versions without `brew trust` can skip that line.

If Homebrew says it is installing or building `rust`, press Control-C and follow the [install guide](docs/install.md) instead. The guide also covers older Macs.

## Update your controller

1. Connect your DualSense with a USB data cable. Charging-only cables will not work.
2. Run `dsmactools`. The main screen shows your controller's firmware and whether an update is available.
3. Use the arrow keys and Enter to choose **Firmware & controller details**, then **Install latest firmware**.
4. Confirm, then keep the cable connected and your Mac awake until the app says the installed firmware is verified. This can take several minutes.

The app downloads the matching official firmware from Sony, checks your controller and its battery first, and reads back the new version at the end. If there is no newer firmware, it says so.

Firmware updates always carry some risk. If something goes wrong, reconnect the controller and check its version under **Firmware & controller details → Controller details** before trying again.

Standard DualSense updates are hardware-tested on Apple Silicon Macs. Updates on Intel Macs and on DualSense Edge controllers have not been tested yet. On Windows, use Sony's official [PlayStation Accessories](https://controller.dl.playstation.net/controller/lang/en/2100004.html) updater.

## Update or uninstall DS Mac Tools

```sh
brew update && brew upgrade dsmactools
```

To uninstall, run `brew uninstall dsmactools`, then `rm -rf ~/Library/Caches/ds-mac-tools` to remove downloaded firmware. Installed another way? See the [install guide](docs/install.md#install-with-rust-instead).

## Privacy

No telemetry or analytics. Controller inputs, recordings and diagnostic output stay on your Mac and are never uploaded. The app's only network requests are firmware checks and downloads directly from Sony.

## More

The menu also has tools for game developers and testers: a live input monitor, session recording, a lightbar and trigger feedback lab, and diagnostics.

[Install guide](docs/install.md) · [Developer tools](docs/tools.md) · [Lightbar and trigger testing](docs/feedback.md) · [Capture format](docs/captures.md) · [Build and protocol notes](docs/research.md) · [Release and Homebrew guide](docs/releasing.md)

## Credits

Derived from [dualsense-updater-rs](https://github.com/nchie/dualsense-updater-rs), preserving its MIT licence and history. Terminal interaction inspired by [Mole](https://github.com/tw93/Mole). Protocol references are in the research notes.

Independent, unofficial software. **Not affiliated with or endorsed by Sony or PlayStation.** Firmware belongs to Sony. MIT licensed; see [LICENSE](LICENSE).
