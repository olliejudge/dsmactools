# Releases and Homebrew

The GitHub repository is `olliejudge/dsmactools`; the display name is DS Mac Tools, and the executable/formula is `dsmactools`. The Cargo package is `ds-mac-tools`. `Formula/dsmactools.rb` lives in `olliejudge/homebrew-tap` alongside the independent Orion cask. Never replace unrelated tap packages.

## A release

1. Update `Cargo.toml`, `Cargo.lock` and `CHANGELOG.md`. Do not alter an existing release tag.
2. Run `cargo fmt --check`, `cargo test --locked`, `cargo clippy --locked -- -D warnings` and `cargo build --release --locked`. Exercise the menu in a real terminal. Read-only controller checks and short feedback tests are sufficient; do not reflash a current controller just to test a release.
3. Commit and push `main`. Wait for the macOS and Linux CI checks to pass for that exact commit.
4. Tag the reviewed commit `v<VERSION>`, push the tag, and create a stable GitHub release with its changes and tested hardware limitations.
5. Download `https://github.com/olliejudge/dsmactools/archive/refs/tags/v<VERSION>.tar.gz`; compute `shasum -a 256` on that downloaded archive. GitHub's compressed archive bytes can differ from a locally generated archive.
6. Update the formula's version URL and SHA-256. It uses `cargo install --locked` via `std_cargo_args`, with Rust as a build dependency and macOS 11.3+ required for native diagnostics.
7. Run `brew style Formula/dsmactools.rb` and `brew audit --strict --online olliejudge/tap/dsmactools`, install from the release archive, and run `brew test olliejudge/tap/dsmactools`. Audit can have environment/network limitations; record any unresolved result accurately.
8. Commit and push the formula and release notes to the tap. Verify the remote formula URL/checksum and an installed `dsmactools --version`. Existing users receive releases with `brew update` and `brew upgrade dsmactools`.

A source-built Homebrew CLI does not require Apple Developer ID signing or notarization. Revisit signing/notarization if distributing downloadable executable binaries or an app bundle. Firmware is downloaded separately at runtime; never include Sony images in release archives.

Homebrew's GitHub livecheck can find stable releases. Updating this app and updating controller firmware are separate operations. The daily Rust Sony catalogue workflow reports metadata changes for review; it does not advance the baseline or flash hardware automatically.

## The install command

Today, either command sequence works:

```sh
brew tap olliejudge/tap
brew install dsmactools
```

```sh
brew install olliejudge/tap/dsmactools
```

The qualified command adds the tap as part of installation. Once tapped, users can use the short package name. Adding this formula does not collide with the tap's `orion` cask. The formula installs only `dsmactools`; it does not replace upstream updater executables.

## Homebrew core

For **a fresh Homebrew installation with no tap setup**, `brew install dsmactools` requires acceptance into `homebrew/core`. A release in our own tap cannot provide that globally.

As reviewed on 2026-09-29, Homebrew's [package acceptance policy](https://docs.brew.sh/Package-Acceptance-Policy) normally requires repositories to be at least 30 days old and demonstrate interest beyond the author. The published GitHub thresholds are 30 forks, 30 watchers or 75 stars; an owner self-submission has higher thresholds of 90 forks, 90 watchers or 225 stars. Exceptions are discretionary. This new project should use its tap while it develops a public user base.

The [formula requirements](https://docs.brew.sh/Acceptable-Formulae) also call for a stable immutable release, checksummed sources, an acceptable open-source license, reproducible dependencies, current compiler support and successful tests on supported platforms. A separately named fork can qualify. A documented macOS restriction can be eligible; native GameController functionality explains our macOS formula restriction, while the Rust core also builds in Linux CI.

When the project meets the current policy, refresh the rules, prepare a `homebrew/core` formula PR using the same stable source/checksum, include hardware-free formula tests, and work through Homebrew's review and bottle CI. Do not claim core availability before that PR is merged. Core acceptance is a maintainer decision, not a release step we can guarantee.
