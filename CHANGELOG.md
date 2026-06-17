# Changelog

All notable changes to Freally Snipper are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added — Phase 1: Capture core (image)
- `freally-capture` crate: multi-monitor, DPI/scale-aware screen capture via `xcap`
  (`capture_all`, `composite`/`crop_composite`, `capture_rect`, `list_windows`),
  all in virtual-desktop pixel coordinates (negative origins supported).
- **Selection overlay**: the home window hides, then becomes a borderless,
  always-on-top, full-virtual-desktop overlay showing a frozen, dimmed snapshot
  with a crosshair, live rubber-band rectangle, and size readout. **Esc** cancels.
- **Four capture modes**: Rectangle (drag), Window (hover-highlight a window and
  click to grab its exact, desktop-clamped bounds), Freeform (lasso → bounding-box
  crop with everything outside the path made transparent), and Full screen.
- **Global hotkey** (default `Ctrl+Shift+S`) opens a capture from anywhere; the
  hotkey is chosen from a validated preset list so an invalid binding can't lock
  the user out, and `+ New` / `Camera` start a capture from the toolbar.
- On capture, the image is **copied to the clipboard** (`arboard`) **and saved**
  to the configured folder; filenames are collision-safe within the same millisecond.
  Clipboard copy + file save run on a **background worker thread**, so committing a
  capture restores the window instantly (no UI stall while a full-screen PNG encodes).
- **Save folder picker**: a native "Change…" folder dialog (`rfd`) in Settings,
  opening at the current folder / Pictures by default.
- Hide-during-capture guarantees no Freally Snipper chrome appears in the shot.
- App split into modules (`settings`, `app`, `overlay`, `output`, `hotkey`).

### Changed
- Capture hotkey setting is now a preset dropdown (was free-text); a rejected
  hotkey keeps the previous working binding.
- CI/release workflows install the Linux system deps `xcap`/`rfd` need
  (`libclang-dev`, `libpipewire-0.3-dev`, `libwayland-dev`, `libxcb1-dev`,
  `libxrandr-dev`, `libegl-dev`).

## [0.1.0] — 2026-06-16 — Phase 0: Foundation

### Added
- Cargo workspace `freally-snipper`: binary crate `app/` plus library crates
  `crates/{capture,editor,asr,video}` (placeholders wired up for later phases).
- `rust-toolchain.toml` pinning stable Rust with `rustfmt` + `clippy`; `rustfmt.toml`.
- Proprietary `LICENSE` (© Mike Weaver — All Rights Reserved), `README.md`,
  `THIRD-PARTY-NOTICES.md`, `SECURITY.md` (security policy + private vulnerability reporting),
  `.gitattributes`, and this changelog.
- eframe home window (900×600, titled "Freally Snipper") with a light/dark theme
  toggle and an embedded app icon.
- JSON settings store in the OS config directory — capture hotkey, save folder,
  default image format, theme, and default snippet mode — that persists across runs.
- Version banner printed to stdout on launch.
- GitHub Actions CI matrix (windows-latest / macos-latest / ubuntu-latest):
  build, test, `clippy -D warnings`, and `fmt --check`.
- Packaging scaffold via `cargo-bundle` (`[package.metadata.bundle]` in `app/Cargo.toml`).
- `.github/workflows/release.yml` — tag-triggered (`v*`) per-OS build → zip → draft GitHub Release.
- README: up-front transparency note that optional AI models are **downloaded on demand, not bundled**.
- Icon strategy: owned custom SVGs are the primary approach (permissive set as fallback).
