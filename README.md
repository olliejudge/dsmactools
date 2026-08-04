# DualSense Updater

## Disclaimer

This is not an official Sony application. If you can, use Sony’s official updater
for Windows instead. If you don’t have convenient access to a Windows machine,
you can try this tool.

Use at your own risk. I got it working, but discovered the hard way that upgrades
are auto-committed even if the verify and finalize steps are never run. That
means I no longer have a controller to test changes on. There are no
guarantees this won’t brick your controller, but I doubt it: the device appears
to verify the image before committing. The worst case I would expect is a failed
update that leaves the current firmware intact. This has only been tested on the
standard DualSense, not the DualSense Edge.

## Build

```sh
cargo build --release
```

## Usage

Build the binary and run it:

```sh
cargo build --release
cd target/release/
```

```sh
./dualsense-updater --print-firmware-info
```

```sh
./dualsense-updater FWUPDATE000B.bin
```

Note: you can replace `./dualsense-updater` with `cargo run -- ` and run it from the project directory if you prefer.

## Options

- `--vid` / `--pid`: USB VID/PID (default `0x054c:0x0ce6`).
- `--path`: exact HID device path from the device listing.
- `FW_IMAGE`: firmware image path (required for update commands).
- `--verbose` / `-v`: print extra update chunk/status debug output.

## Usage Instructions

The latest versions for each target are listed here:

```
https://fwupdater.dl.playstation.net/fwupdater/info.json
```

I’m not 100% sure how targets map to revisions, but so far I’ve found:

- DualSense BDM-020 -> 0004
- DualSense BDM-030 -> 0004
- DualSense BDM-050 -> 000B
- DualSense BDM-060 -> 000E
  (seems to depend on which SoC a revision uses)
- DualSense Edge -> 0044

In my experience, attempting to flash the wrong firmware fails verification, so it should not brick anything.

Firmware files can be downloaded from:

```
https://fwupdater.dl.playstation.net/fwupdater/fwupdate<target>/<version>/FWUPDATE<target>.bin
```

Example: to get version `0x0630` for DualSense BDM-050 (`000B`), use:

```
https://fwupdater.dl.playstation.net/fwupdater/fwupdate000B/0x0630/FWUPDATE000B.bin
```

## Notes

- A controller already on the latest firmware may not return success codes past
  `--start-update`; this is expected.
- You may need OS-specific permissions to access HID devices.

## License

MIT. See `LICENSE`.
