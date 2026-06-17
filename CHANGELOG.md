# Changelog

All notable changes to Freally Snipper are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.19.84] — 2026-06-17 — Home window

The Windows-11-style home window: a capture toolbar, a recent-captures gallery, full settings, an About panel, and an opt-in Print Screen takeover.

### Added
- A toolbar (`+ New` · Camera · Video · Snippet ▾ · Timer ▾ · Color) starts a capture in the chosen mode after an optional 3/5/10 s timer.
- A recent-captures strip shows thumbnails — decoded off-thread for no UI stutter — that reopen or reveal in their folder.
- Settings cover the hotkey, save folder, image format, theme, default mode, and an 18-language UI picker (English first).
- An About panel shows the version, ownership, project-start date, and the embedded license + third-party notices.
- Opt-in "Open Freally Snipper with Print Screen" (P1.5): Windows frees the key via the registry and restores it on disable; macOS/Linux get guided remap steps.
- First public release, so it also ships the Phase 1 capture core (rectangle / window / freeform / full-screen snips, global hotkey, clipboard + save).

### Changed
- The app/window icon is now `Freally_Snipper_Icon_Dark.png`.
- Version set to 0.19.84 — the first step on the release ladder to v1.0.0.

## [0.1.0] — 2026-06-16 — Foundation

The workspace, app shell, settings store, CI matrix, and packaging scaffold.

### Added
- Cargo workspace (`app/` + `crates/{capture,editor,asr,video}`) on a pinned stable toolchain.
- An eframe home window (900×600) with a light/dark theme toggle and an embedded icon.
- A JSON settings store in the OS config directory that persists across runs.
- A CI matrix (Windows/macOS/Linux): build, test, clippy `-D warnings`, and fmt.
- Packaging via `cargo-bundle` plus a tag-triggered release workflow (per-OS zip → draft Release).
