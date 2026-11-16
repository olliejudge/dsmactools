# Installing and removing DS Mac Tools

Most people should install with Homebrew. If Homebrew starts building Rust from source on your Mac, use [Rust's installer](#install-with-rust-instead) instead.

## Which Macs get a quick install

Homebrew installs a ready-made package (a "bottle") on Macs running macOS 15 Sequoia or later, with either Apple Silicon or Intel.

On these Macs, installing takes a minute or two. On macOS 14 Sonoma or older, Homebrew has no ready-made package, so it builds DS Mac Tools from source instead. That needs the Rust compiler, and Homebrew may have to build Rust itself from source first. Building Rust can take several hours.

You can tell this is happening if Homebrew says it is installing or building `rust`. Press Control-C to stop it, then follow the steps under [Install with Rust instead](#install-with-rust-instead). Stopping it is safe. Afterwards, `brew autoremove` removes any build tools Homebrew had already installed for it.

Whichever way you install, you'll need a USB data cable to connect your controller. Charging-only cables will not work.

## Install with Homebrew

If you don't have Homebrew yet, follow the instructions at [brew.sh](https://brew.sh). Then run:

```sh
brew tap olliejudge/tap
brew trust --formula olliejudge/tap/dsmactools
brew install dsmactools
dsmactools
```

Older Homebrew versions without `brew trust` can skip that line. Already using this tap? You only need `brew install dsmactools`.

### Update

```sh
brew update && brew upgrade dsmactools
```

### Uninstall

```sh
brew uninstall dsmactools
rm -rf ~/Library/Caches/ds-mac-tools
```

The second line removes downloaded firmware. If you won't use anything else from this tap, you can also run `brew untap olliejudge/tap`.

If Homebrew built Rust for you during an earlier install, `brew autoremove` removes it as long as nothing else needs it.

## Install with Rust instead

This route uses rustup, Rust's official installer, to download a ready-made Rust compiler, then builds DS Mac Tools on your Mac. It usually takes a few minutes. Installing Rust is your choice; if you'd rather not, the Homebrew route still works but can take hours on older Macs.

Use one route or the other, not both. If you already installed with Homebrew, run `brew uninstall dsmactools` first.

1. Install Apple's Command Line Tools, which Rust needs to build DS Mac Tools. A window will open; follow its steps. If they are already installed, it will tell you.

   ```sh
   xcode-select --install
   ```

2. Install Rust:

   ```sh
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
   ```

   Then close and reopen Terminal so it can find Rust. If you already had Rust, run `rustup update` instead.

3. Install DS Mac Tools and open it:

   ```sh
   cargo install --locked --git https://github.com/olliejudge/dsmactools --tag v0.5.0
   dsmactools
   ```

### Update

Run the same `cargo install` command with the newest version from the [releases page](https://github.com/olliejudge/dsmactools/releases) in `--tag`.

### Uninstall

```sh
cargo uninstall ds-mac-tools
rm -rf ~/Library/Caches/ds-mac-tools
```

The second line removes downloaded firmware.

If you installed Rust only for DS Mac Tools, you can remove it too:

```sh
rustup self uninstall -y
```

This removes **all** of Rust, including anything else installed with `cargo`. Don't run it if you use Rust for anything else.

## Where your data is stored

DS Mac Tools does not collect or upload anything. It only saves:

- **Downloaded firmware**, in `~/Library/Caches/ds-mac-tools/firmware`. Each download sits in a folder named after its firmware target code and version, with a small `manifest.json` recording where it came from and its checksum. If a download for the same firmware is already there, the app reuses it, but only after checking that its checksum matches `manifest.json` and that it still passes every compatibility check; otherwise it downloads again. The menu's **Downloaded firmware** item shows this folder and its size and can delete the downloads (only the firmware files the app saved). It is also safe to delete the folder yourself at any time; the app downloads firmware again when it needs it.
- **Session recordings**, only when you make one, in the file you choose (for example `session.jsonl`).

To keep firmware downloads somewhere else, add `--cache-dir` with a folder of your choice:

```sh
dsmactools --download-latest --cache-dir ~/Documents/dualsense-firmware
```

With `--cache-dir`, the menu's delete option removes only the firmware files DS Mac Tools saved in that folder, and leaves the folder and anything else in it. Delete the folder yourself when you uninstall.
