# Changelog

All notable changes to Freally Snipper are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.30.0] — 2026-06-17 — Capture overlay action bar

A top-center action bar on the capture overlay, with live mode switching and a hand-off to the editor.

### Added
- An action bar (Camera · Video · Snippet ▾ · Markup · Text Extractor · Color · 🗑) sits at the top of the capture overlay.
- Snippet ▾ switches the selection shape (rectangle / window / freeform / full screen) live, mid-capture.
- Markup hands a finished snip to an editor preview centered below the selection (Save / Discard); the bar hides while you drag.
- Color sets the markup colour on the overlay itself, and the 🗑 button (or Esc) cancels.

### Changed
- The capture hint moved to the bottom of the overlay so it never sits under the action bar.
- Video and Text Extractor show on the bar but are disabled, each labelled with the phase it arrives in (5 and 4).
- Version bumped to 0.30.0 — the Phase 3 step on the ladder to v1.0.0.

## [0.19.85] — 2026-06-17 — Windows release polish

The downloaded Windows build now carries its icon and runs without a console window.

### Fixed
- The `.exe` shows the Freally Snipper icon in Explorer and the taskbar (embedded as a Win32 resource).
- Launching the release no longer opens a console window, and closing a terminal can't quit the app — it's a GUI app now.

## [0.19.84] — 2026-06-17 — Home window

The Windows-11-style home window, plus a system tray, an on-screen capture timer, a recent-captures gallery, full settings, an About panel, and the opt-in Print Screen takeover.

### Added
- A toolbar (`+ New` · Camera · Video · Snippet ▾ · Timer ▾ · Color) starts a capture in the chosen mode.
- With a Timer, you select the region first, then a center-screen countdown runs and the live screen is grabbed.
- A recent-captures strip shows uniform thumbnails of the whole image, each with its date and time.
- A system tray (Windows/macOS) keeps the hotkey working while the window is closed; click the tray icon to reopen.
- Settings cover the hotkey, save folder, format, theme, default mode, timer, colour, an 18-language picker, a "show editor" toggle, and minimize-to-tray.
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
