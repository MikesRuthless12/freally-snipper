# Third-Party Notices

Freally Snipper is proprietary software (© 2026 Mike Weaver — All Rights Reserved; see
[`LICENSE`](LICENSE)). It incorporates the third-party components listed below, each of which
remains licensed under its own terms. This file provides the attribution those licenses
require. Trademarks belong to their respective owners; listing here does not imply endorsement.

> Third-party components are kept **behind interfaces** so an owned implementation can replace them
> later. This list grows as later phases add dependencies.

## Currently bundled / linked (through Phase 2)

| Component | Role | License |
|-----------|------|---------|
| [`egui` / `eframe`](https://github.com/emilk/egui) | GUI framework | MIT OR Apache-2.0 |
| [`xcap`](https://crates.io/crates/xcap) | multi-monitor screen capture + window enumeration | Apache-2.0 |
| [`image`](https://crates.io/crates/image) | image encode/decode (capture, save, icon) | MIT OR Apache-2.0 |
| [`global-hotkey`](https://crates.io/crates/global-hotkey) | system-wide capture hotkey | Apache-2.0 OR MIT |
| [`arboard`](https://crates.io/crates/arboard) | clipboard image copy | MIT OR Apache-2.0 |
| [`rfd`](https://crates.io/crates/rfd) | native "save folder" picker + Print-Screen consent dialog | MIT |
| [`opener`](https://crates.io/crates/opener) | open a saved capture / its folder in the OS default app | MIT OR Apache-2.0 |
| [`winreg`](https://crates.io/crates/winreg) *(Windows only)* | opt-in Print Screen key takeover via the registry (P1.5) | MIT |
| [`tray-icon`](https://crates.io/crates/tray-icon) *(Windows/macOS only)* | system-tray icon + menu (minimize-to-tray) | MIT OR Apache-2.0 |
| [`chrono`](https://crates.io/crates/chrono) | local date/time formatting (recent-capture timestamps) | MIT OR Apache-2.0 |
| [`serde`](https://serde.rs) / [`serde_json`](https://crates.io/crates/serde_json) | settings (de)serialization | MIT OR Apache-2.0 |
| [`directories`](https://crates.io/crates/directories) | OS config/data paths | MIT OR Apache-2.0 |
| [`log`](https://crates.io/crates/log) | logging facade (capture crate) | MIT OR Apache-2.0 |

Transitive Rust dependencies are MIT / Apache-2.0 / BSD / Zlib / MPL. Verify the full set with
`cargo about` or `cargo deny` before any release.

> **Linux note:** `rfd` uses the **XDG Desktop Portal** (D-Bus) for the folder picker and `xcap`
> uses **PipeWire** for capture, so a Linux build links `libpipewire`, `libwayland`, `libxcb`, and
> related system libraries (see `README.md` for the full `apt` list).

## Phase 4 — image editor (Toolbar 2)

**Bundled (compiled in):**

| Component | Role | License |
|-----------|------|---------|
| [`rustybuzz`](https://crates.io/crates/rustybuzz) | text shaping (incl. Arabic joining) | MIT |
| [`ab_glyph`](https://crates.io/crates/ab_glyph) | glyph rasterization | Apache-2.0 OR MIT |
| [Noto Sans / Serif / Mono / Sans Arabic](https://fonts.google.com/noto) | bundled text-object fonts | SIL OFL 1.1 |
| [`swash`](https://crates.io/crates/swash) | colour-glyph (COLR/CBDT) rasterization for emoji | MIT OR Apache-2.0 |
| [`emojis`](https://crates.io/crates/emojis) | emoji database for the searchable picker | MIT OR Apache-2.0 |
| [`ocrs`](https://crates.io/crates/ocrs) + [`rten`](https://crates.io/crates/rten) | OCR engine ("Extract Text") — pure-Rust | MIT |
| [`ureq`](https://crates.io/crates/ureq) | on-demand model/font downloads (rustls TLS) | MIT OR Apache-2.0 |

**Downloaded on demand** (not bundled; fetched to the OS cache on first use):

| Component | Role | License |
|-----------|------|---------|
| [ocrs models](https://github.com/robertknight/ocrs-models) (detection / recognition) | OCR (P4.6b) | Apache-2.0 |
| [Noto Color Emoji](https://github.com/googlefonts/noto-emoji) | colour emoji rendering (P4.7) | SIL OFL 1.1 |

Downloads are over TLS from the hosts above. See [`SECURITY.md`](SECURITY.md) for the
download-integrity posture.

## Phase 5 — video capture (recording / audio / webcam / export)

**Bundled (compiled in):**

| Component | Role | License |
|-----------|------|---------|
| [`cpal`](https://crates.io/crates/cpal) | system-loopback + microphone audio capture (recording) | Apache-2.0 |
| [`nokhwa`](https://crates.io/crates/nokhwa) | webcam capture (recording picture-in-picture); native per-OS backend | Apache-2.0 OR MIT |
| [`image`](https://crates.io/crates/image) (`gif` feature) | in-process animated-GIF export | MIT OR Apache-2.0 |
| [`ffmpeg-sidecar`](https://crates.io/crates/ffmpeg-sidecar) | locate / download + drive ffmpeg as a subprocess (export only) | MIT OR Apache-2.0 |

**Run as a separate process, downloaded on demand (NOT bundled, NOT linked):**

| Component | Role | License |
|-----------|------|---------|
| [ffmpeg](https://ffmpeg.org) | optional **MP4 / WebM** video export | LGPL / GPL (the standalone binary's own license) |

`freally-video` (`.fvid`) is the **owned, default** record format, and **GIF** export is in-process.
ffmpeg is used **only** for optional MP4/WebM export — it is **not linked** into Freally Snipper but
**invoked as a standalone subprocess** (fetched on first use to a per-user cache), so its GPL/LGPL
stays with that separate binary and does not affect Freally Snipper's proprietary license. See
[`SECURITY.md`](SECURITY.md) for the ffmpeg-download trust note.

## Planned components (later phases — listed now for licensing clarity)

| Component | Role | License | Notes |
|-----------|------|---------|-------|
| [Noto fonts](https://fonts.google.com/noto) | multilingual text objects | SIL OFL 1.1 | bundled *as Noto* with attribution; free for commercial use |
| [Tesseract OCR](https://github.com/tesseract-ocr/tesseract) | text extraction + text-aware highlight | **Apache-2.0** | optional; local; multilingual lang-data on demand |
| [ffmpeg](https://ffmpeg.org) via [`ffmpeg-sidecar`](https://crates.io/crates/ffmpeg-sidecar) | optional MP4/WebM export | **LGPL** (use the LGPL build; do **not** `--enable-gpl`) | optional convenience export; **`freally-video` is the default** |

## Codec / patent note

H.264 (AVC) and AAC are **patent-pooled** (Via LA). Freally Snipper's owned **`freally-video`** codec
uses only expired-patent / public-domain techniques, and its **GIF** and **WebM (VP9 / Opus)** exports
are **royalty-free**. **MP4 (H.264/AAC)** export is offered for compatibility; those codecs' patents
are the owner's accepted responsibility as the distributor. All MP4/WebM encoding is performed by
**ffmpeg as a separate subprocess** (not linked into the app), downloaded on demand rather than
bundled. (A from-scratch H.264 encoder would **not** avoid these patents — they cover the format's
techniques, not the code — so it is revisited only once the AVC patents fully expire.)

## Owned components (not third-party)

`freally-video` (codec), the `freally-*` image codecs, and the future `freally-font` typeface are
original works © Mike Weaver, covered by [`LICENSE`](LICENSE) — they are not third-party components.
