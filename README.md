# Freally Snipper

A **free**, **native**, **cross-platform** screen capture + image editor + light video editor
for **Windows, macOS, and Linux** — in the spirit of the Windows 11 Snipping Tool + Snagit +
ScreenToGif, but **free, local-first, and privacy-respecting** (no accounts, no cloud, no
telemetry).

> **Status:** Phase 0 (Foundation) — workspace, app shell, settings, CI, and packaging scaffold.

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
- **Linux only** — system libraries for the GUI/window stack:
  ```sh
  sudo apt-get install -y \
    libgtk-3-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    pkg-config libgtk-3-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libxkbcommon-dev libxkbcommon-x11-dev libssl-dev libasound2-dev libdbus-1-dev
  ```

## Build & run

```sh
cargo run                 # prints the version banner, then opens the home window
cargo build --release     # optimized build -> target/release/freally-snipper
```

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
| macOS | `.app` / `.dmg` | `.icns` generated from `assets/icon.png`; notarization comes in Phase 7 |
| Linux | `.deb` | AppImage / `.rpm` / Flatpak come in Phase 7 |

### Releases

Pushing a version tag triggers [`.github/workflows/release.yml`](.github/workflows/release.yml),
which builds the app on all three OSes, **zips each**, and opens a **draft GitHub Release** with the
downloadable zips (you review, then publish):

```sh
git tag v0.1.0 && git push origin v0.1.0
```

Signed/notarized installers (MSI / .dmg / AppImage) and auto-update arrive in
**Phase 7 — Distribution & polish**.

## Workspace layout

```
.
├── app/                 # `freally-snipper` binary — eframe home window, settings, banner
├── crates/
│   ├── capture/         # `freally-capture` — screen capture           (Phase 1)
│   ├── editor/          # `freally-editor`  — image editor             (Phase 4)
│   ├── asr/             # `freally-asr`     — optional local speech-to-text (Phase 6)
│   └── video/           # `freally-video`   — owned video codec + editor   (Phase 5/6)
└── .github/workflows/ci.yml
```

## Settings

On first run, settings are written as JSON to your OS configuration directory (resolved via the
[`directories`](https://crates.io/crates/directories) crate). They hold the capture hotkey, save
folder, default image format, theme, and default snippet mode, and persist across runs. The exact
path is shown at the bottom of the in-app **Settings** section.
