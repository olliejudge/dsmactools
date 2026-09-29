# DS Mac Tools

This is a macOS-first fork of nchie/dualsense-updater-rs. Preserve the MIT license and upstream history. `upstream` is the source repository; `origin` is `olliejudge/dsmactools`. The Homebrew formula lives in `olliejudge/homebrew-tap`, separate from its Orion cask.

- Default controller operations are read-only. Use Sony's live catalogue rather than a remembered firmware version.
- Select firmware using the controller report's software series, checked against the image header. Never guess from retail color, product ID alone, or the highest available version across targets.
- Keep image validation, USB/battery checks, complete update sequencing, bounded polling and installed-version verification together. Do not restore unguarded per-phase flash commands.
- A transfer or finalize status is not proof of an installed update. Read the controller after re-enumeration; avoid automatic reflashing on errors.
- Firmware images, manifests and private diagnostics remain ignored. Do not add Sony binaries to Git.
- Read `docs/research.md` for tested hardware and protocol sources. New controller targets or image formats need source review before support is added.
- Validate changes with `cargo fmt --check`, `cargo test --locked`, and `cargo clippy --locked -- -D warnings`. Use `cargo run --locked -- --check-catalogue` for a live metadata check without opening hardware.
- Update the research log after hardware tests. Only advance `catalogue-baseline.json` after reviewing a detected change.

- The executable and Homebrew formula are named `dsmactools`. The repository remains `dsmactools`.
- The default terminal experience is interactive; scripted runs remain read-only by default. Route menu actions through the shared validated update path.
- Follow `docs/releasing.md` for tagged source releases and tap updates. Preserve existing packages in the shared Homebrew tap.
