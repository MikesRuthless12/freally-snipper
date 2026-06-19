# Freally Snipper

A **free**, **native**, **cross-platform** screen capture + image editor + light video editor
for **Windows, macOS, and Linux** — in the spirit of the Windows 11 Snipping Tool + Snagit +
ScreenToGif, but **free, local-first, and privacy-respecting** (no accounts, no cloud, no
telemetry).

> **Status:** Phase 5 (Video capture) — **screen recording** of a region / window / full screen to the
> **owned `freally-video` (`.fvid`) format**, with **attach-and-follow a window**, a recording bar
> (pause / resume / stop), **system + microphone audio**, and an optional **webcam picture-in-picture**.
> Recordings **play back in-app** through the owned decoder, and **export** to **GIF** (built-in),
> **WebM (VP9/Opus)**, or **MP4 (H.264/AAC)** via ffmpeg. Any resolution, **including 4K** — the
> streaming encoder keeps memory bounded. Builds on the Phase 4 image editor, the Phase 3 capture
> overlay, the Phase 2 home window, the Phase 1 capture core, and the Phase 0 foundation.

> **🔒 No bundled AI models — full transparency.** Capture and image/video editing work **100%
> offline**. The **optional** speech-to-text, translation, and dubbing features use third-party AI
> models (Whisper, M2M-100, Piper, …) that are **NOT bundled or redistributed** with Freally Snipper.
> When *you choose* to enable one of those features, the app **downloads the model you pick, on
> demand** (or lets you point it at one you already have) — and shows you exactly what it fetches.
> Nothing is downloaded or sent anywhere otherwise. See [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md).

## License (important)

Freally Snipper is **proprietary** software — **© 2026 Mike Weaver. All Rights Reserved.**
The pre-built application is free to download and use; the **source code is not** open source
and may not be copied, modified, or redistributed. See [`LICENSE`](LICENSE). Bundled third-party
components keep their own licenses — see [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md).

## Security

Freally Snipper is **local-first and offline by default** — no accounts, no cloud, no telemetry. To
report a vulnerability, see [`SECURITY.md`](SECURITY.md) (please report **privately**, not via a
public issue).

## Requirements

- [Rust](https://rustup.rs) (stable; pinned via `rust-toolchain.toml`).
- **Linux only** — system libraries for the GUI, screen capture (`xcap`), and the
  folder picker (`rfd`):
  ```sh
  sudo apt-get install -y \
    pkg-config libgtk-3-dev \
    libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libxcb1-dev libxrandr-dev \
    libxkbcommon-dev libxkbcommon-x11-dev \
    libssl-dev libasound2-dev libv4l-dev libdbus-1-dev \
    libclang-dev libpipewire-0.3-dev libwayland-dev libegl-dev
  ```
  (`libasound2-dev` + `libv4l-dev` are for recording audio + webcam in Phase 5.)

## Build & run

```sh
cargo run                 # prints the version banner, then opens the home window
cargo build --release     # optimized build -> target/release/freally-snipper
```

## Capturing

Press the global hotkey (**`Ctrl+Shift+S`** by default) from anywhere, or click **`+ New`**
on the home window. The window hides and the screen freezes under a dimmed selection overlay:

- **Rectangle** — drag a box.
- **Window** — hover to highlight the window under the cursor, then click to grab it.
- **Freeform** — draw a lasso; everything outside the path becomes transparent.
- **Full screen** — captures every monitor at once.
- **Esc** cancels.

Pick the mode from **Snippet ▾** — on the home window, or live from the overlay's top-center **action
bar**, which also sets the markup **Color** and (via **Markup**) opens a finished snip in an editor
preview centered below the selection (**Save** / **Discard**) instead of saving straight away; the bar
hides while you drag so it's never in the shot. With **Timer ▾** (3 / 5 / 10 s) you select the region
first, then a center-screen countdown runs and the **live** screen is grabbed — so you can arrange the
screen during the delay (Timer Off captures immediately). Each capture is **copied to the clipboard** and
**saved** to your save folder (default `Pictures/Freally Snipper`), and appears as a dated **thumbnail**
on the home window — click it to open in your OS viewer. Turn on **minimize to system tray** to keep
the hotkey working while the window is closed.

## Editing

Turn on **Open the editor after capture** (in Settings, or **Markup** on the overlay) and each snip
opens in the **image editor** (Toolbar 2) — its own window on a zoom/pan canvas where **Save writes
exactly what you see** (Save / Copy / Discard, Undo / Redo):

- **Draw** — pen, brush, highlighter (free or text-aware), and a two-mode eraser, each with an adjustable size.
- **Objects** — rectangles / ovals / lines / arrows, **text** + **watermark** (size / font / opacity / colour), placed **image files**, and colour **emoji** — moved, resized, and flattened only on Save.
- **Filters & transforms** — grayscale / sepia / invert / blur / sharpen / brightness / contrast / posterize / cartoonize, and rotate / flip / bevel / crop, plus an eyedropper.
- **Extract Text** (OCR → clipboard) and **translate** any text object on-device into ~400 languages, with a type-to-filter language picker.

The OCR, colour-emoji, and translation models aren't bundled — the **Models** panel downloads each on
demand with a live progress bar (% · amount of total · MB/s). See
[`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md).

## Recording video

Click **Video** on the home toolbar (or **Video** on the capture overlay's action bar), then pick a
**region**, a **window**, or the **full screen** — the same selection modes as a snip. Recording
starts and a small always-on-top bar shows **● REC**, the elapsed time, and **Pause / Resume / Stop**;
on **Stop** the clip is saved as a **`.fvid`** in your save folder and appears in **Recent captures**.

- **Attach to a window** — record one app window and **follow it** as it moves or resizes.
- **Audio** — capture **system audio** ("what you hear") and/or your **microphone** (Settings →
  Recording). System audio is WASAPI loopback on Windows and a PipeWire/PulseAudio monitor on Linux;
  on macOS it needs a virtual device (e.g. BlackHole). Best-effort — recording continues silently if a
  source can't open.
- **Webcam PiP** — optionally overlay your camera in the corner of the recording.
- **Frame rate** — 15 / 24 / 30 / 60 fps. Any resolution, **including 4K**; the owned codec is
  lossless, so very high frame rates at 4K may not sustain on every machine, but playback always
  stays the right length.

**Play back** a recording by clicking it in **Recent captures** — it opens in the in-app player
(Play / Pause / Loop), decoded entirely through the owned `freally-video` path. From the player,
**Export ▾** writes an **animated GIF** (built-in, no dependency) or, via **ffmpeg** (fetched on first
use and run as a separate process), **WebM (VP9/Opus)** or **MP4 (H.264/AAC)**.

> **Owned vs. optional.** `freally-video` (`.fvid`) is the **owned, default** record format. GIF and
> WebM/VP9/Opus export are **royalty-free**; **MP4 (H.264/AAC)** uses patent-pooled codecs. ffmpeg is
> **not bundled** — it's downloaded on demand and run as a standalone subprocess. See
> [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md) and [`EULA.md`](EULA.md).

## Develop

```sh
cargo fmt --all                              # format
cargo fmt --all -- --check                   # CI format check
cargo clippy --all-targets -- -D warnings    # lint (warnings = errors)
cargo test                                   # run tests
```

These mirror exactly what CI runs (`.github/workflows/ci.yml`) on Windows, macOS, and Linux.

## Packaging (per-OS installable artifact)

Packaging uses [`cargo-bundle`](https://github.com/burtonageo/cargo-bundle), configured in
[`app/Cargo.toml`](app/Cargo.toml) under `[package.metadata.bundle]`:

```sh
cargo install cargo-bundle
cargo bundle --release        # run on each target OS
```

| OS | Produces | Notes |
|----|----------|-------|
| Windows | `.msi` | needs the [WiX Toolset](https://wixtoolset.org); a `.ico` is produced at packaging time |
| macOS | `.app` / `.dmg` | `.icns` generated from `assets/Freally_Snipper_Icon_Light.png`; notarization comes in Phase 7 |
| Linux | `.deb` | AppImage / `.rpm` / Flatpak come in Phase 7 |

### Releases

Pushing a version tag triggers [`.github/workflows/release.yml`](.github/workflows/release.yml),
which builds the app on all three OSes, **zips each**, and opens a **draft GitHub Release** with the
downloadable zips (you review, then publish):

```sh
git tag v0.45.0 && git push origin v0.45.0
```

Signed/notarized installers (MSI / .dmg / AppImage) and auto-update arrive in
**Phase 7 — Distribution & polish**.

A **Releases &amp; Updates** web page lives in [`docs/`](docs/) (a static site, not yet deployed).
Publish it via **Settings → Pages → Deploy from a branch → `main` / `docs`** to serve it at
`https://mikesruthless12.github.io/freally-snipper/`.

## Workspace layout

```
.
├── app/                 # `freally-snipper` binary — eframe home window, settings, banner
├── crates/
│   ├── capture/         # `freally-capture` — screen capture           (Phase 1)
│   ├── editor/          # `freally-editor`  — image editor             (Phase 4)
│   ├── freally-font/    # `freally-font`    — Freally's own typeface (English; in progress) (Phase 4b)
│   ├── asr/             # `freally-asr`     — optional local speech-to-text (Phase 6)
│   └── video/           # `freally-video`   — owned video codec + editor   (Phase 5/6)
└── .github/workflows/ci.yml
```

## Settings

On first run, settings are written as JSON to your OS configuration directory (resolved via the
[`directories`](https://crates.io/crates/directories) crate) and persist across runs. They hold
the hotkey, save folder, image format, theme, default snippet mode, capture timer, markup colour,
UI language, a "show capture editor" toggle, minimize-to-tray, and the opt-in Print Screen takeover.
The exact path is shown at the bottom of the in-app **Settings** view.
