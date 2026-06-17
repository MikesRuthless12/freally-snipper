# Changelog

All notable changes to Freally Snipper are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.19.84] — 2026-06-17 — Home window

The Windows-11-style home window, plus a system tray, an on-screen capture timer, a recent-captures gallery, full settings, an About panel, and the opt-in Print Screen takeover.

### Added
- A toolbar (`+ New` · Camera · Video · Snippet ▾ · Timer ▾ · Color) starts a capture in the chosen mode.
- With a Timer set, you select the region first, then a center-screen 5→1 countdown runs and the *live* screen is grabbed — so you can open a menu or arrange things during the delay.
- A recent-captures strip shows uniform square thumbnails (whole image, decoded off-thread) with each capture's date/time, that reopen or reveal in their folder.
- A system tray (Windows/macOS) keeps the app resident so the hotkey / Print Screen still capture while the window is closed; double-click or the menu reopens it, Quit exits.
- Settings cover the hotkey, save folder, image format, theme, default mode, capture timer, markup colour, an 18-language UI picker (English first), a "show capture editor" toggle, and minimize-to-tray.
- An About panel shows the version, ownership, project-start date, and the embedded license + third-party notices.
- Opt-in "Open Freally Snipper with Print Screen" (P1.5): Windows frees the key via the registry and restores it on disable; macOS/Linux get guided remap steps.
- First public milestone, so it also ships the Phase 1 capture core (rectangle / window / freeform / full-screen snips, global hotkey, clipboard + save).

### Changed
- App/window icon is `Freally_Snipper_Icon_Light.png`, auto-trimmed to fill the canvas; the tray icon is pre-scaled for a crisp fit.
- Version set to 0.19.84 — the first step on the release ladder to v1.0.0.

### Fixed
- Freeform saved to JPG composites the outside over white instead of leaking the masked-out pixels into a full rectangle (PNG keeps it transparent).
- The Freeform outline draws in the toolbar's active colour.

## [0.1.0] — 2026-06-16 — Foundation

The workspace, app shell, settings store, CI matrix, and packaging scaffold.

### Added
- Cargo workspace (`app/` + `crates/{capture,editor,asr,video}`) on a pinned stable toolchain.
- An eframe home window (900×600) with a light/dark theme toggle and an embedded icon.
- A JSON settings store in the OS config directory that persists across runs.
- A CI matrix (Windows/macOS/Linux): build, test, clippy `-D warnings`, and fmt.
- Packaging via `cargo-bundle` plus a tag-triggered release workflow (per-OS zip → draft Release).
