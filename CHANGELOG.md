# Changelog

All notable changes to Freally Snipper are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
