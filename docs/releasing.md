# Releases and Homebrew

The GitHub repository is `olliejudge/dsmactools`; the package and executable are `ds-mac-tools`. The Homebrew formula is `Formula/ds-mac-tools.rb` in `olliejudge/homebrew-tap`. It does not replace or edit the tap's Orion cask.

1. Update the version in `Cargo.toml`, regenerate `Cargo.lock`, and update `CHANGELOG.md`.
2. Run formatting, tests, Clippy and a release build. Exercise read-only menu navigation on a real terminal. A release does not require reflashing a current controller.
3. Commit and push `main`, then wait for CI to pass.
4. Tag the reviewed commit, push the tag, and create a GitHub release with its changes.
5. Download `https://github.com/olliejudge/dsmactools/archive/refs/tags/v<VERSION>.tar.gz` and compute its SHA-256. Use the downloaded GitHub archive rather than a locally generated archive; compressed bytes may differ.
6. Update the formula's version URL and checksum in the existing tap, then install it from source and run `brew test olliejudge/tap/ds-mac-tools`. Preserve unrelated tap files.
7. Commit and push the formula. Existing installations receive it through `brew update` and `brew upgrade olliejudge/tap/ds-mac-tools`.

The formula builds with `cargo install --locked --path . --root <prefix>`. Rust is a build dependency; the resulting tool uses native IOKit HID access on macOS. This source-build distribution does not require Apple Developer ID signing or notarization. Revisit signing and notarization if adding downloadable executable/app distribution.

Homebrew livecheck detects stable GitHub releases. Updating the tool and checking Sony controller firmware are separate operations. The daily Sony catalogue workflow only reports metadata changes for review.
