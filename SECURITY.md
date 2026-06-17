# Security Policy

Freally Snipper is proprietary software (© 2026 Mike Weaver — All Rights Reserved; see
[`LICENSE`](LICENSE)). Protecting your data is a core design goal: the app is **local-first and
offline by default** — capture, editing, and the optional transcription/translation/dubbing all run
**on your machine**, with **no accounts, no cloud, and no telemetry**.

## Supported versions

Freally Snipper is pre-1.0 and under active development. Security fixes target the **latest** commit
on the default branch; older snapshots are not maintained.

| Version | Supported |
|---------|-----------|
| latest (`main`) | ✅ |
| older | ❌ |

## Reporting a vulnerability

Please report security issues **privately — do not open a public issue or PR**.

- **Email:** mythodikalone@gmail.com (subject: `Freally Snipper security`), **or**
- **GitHub:** use **Security ▸ Report a vulnerability** (private vulnerability reporting) on this repo.

Include the affected version/commit, your OS, reproduction steps, impact, and any proof-of-concept.
You'll get an acknowledgement and status updates through to the fix. Please allow reasonable time to
remediate before any public disclosure.

## Scope & notes

- **Local-first:** the core never transmits your data. The only network actions are *optional and
  explicit* — model downloads (Whisper / translation / TTS) and user-initiated "Share"/export.
- **Capture surface (Phase 1):** screen capture, the clipboard copy, and the saved image file all
  stay **on your machine**. Captures are written only to the folder you choose (default
  `Pictures/Freally Snipper`); filenames are program-generated (no path-traversal input). The
  global capture hotkey is registered with the OS and chosen from a fixed preset list. No `unsafe`
  code is used (`#![forbid(unsafe_code)]`).
- **Home window (Phase 2):** the recent-captures gallery opens a saved file or its folder only via
  the OS default handler (`opener`), at your click. The opt-in **Print Screen** takeover changes a
  single **per-user** registry value (`HKCU\Control Panel\Keyboard\PrintScreenKeyForSnippingEnabled`)
  — only after an explicit consent dialog — and restores the prior value when disabled; it never
  touches machine-wide (`HKLM`) settings. That registry access uses the safe `winreg` wrapper, so
  the app stays `#![forbid(unsafe_code)]`. The UI-language setting is stored locally and sends
  nothing anywhere. The opt-in **system tray** (Windows/macOS) only keeps the app running locally so
  the capture hotkey works while the window is closed — no network activity; timestamps shown in the
  gallery read only the local clock.
- **Capture overlay (Phase 3):** the top-center action bar and the post-capture **editor preview**
  run entirely **locally and in-memory**. Saving from the preview reuses the same path as before — a
  clipboard copy plus a file written only to the folder you chose, with program-generated filenames
  (no path-traversal input). The **Video** and **Text Extractor (OCR)** buttons are inert
  placeholders (no capture, no recognition, no network) until their phases land. No new dependencies
  were added, and the app stays `#![forbid(unsafe_code)]`.
- **Image editor (Phase 4):** all editing — markup, text, shapes, emoji, filters, transforms, OCR,
  and translation — runs **locally and in-memory**; nothing is uploaded. The only network actions are
  **optional, explicit model downloads** for the OCR (ocrs), colour-emoji (Noto Color Emoji), and
  translation (MADLAD-400) add-ons, fetched on first use (or from the in-app **Models** panel) into
  your per-user cache. **Integrity:** downloads are over **TLS** from fixed, hardcoded hosts; target
  filenames are **hardcoded literals** (no path-traversal input); each file is streamed to a temp path
  and **atomically renamed**; the ~3 GB **MADLAD weights are pinned to an immutable revision** so they
  can't silently change. **Tracked hardening:** per-file **SHA-256 pinning** — TLS authenticates the
  host, not the bytes. The translate add-on loads weights via one **`unsafe` memory-map** (required by
  `candle`) of a file the app just wrote into its own cache; the rest of the editor crate, and the
  whole app binary, remain `#![forbid(unsafe_code)]`.
- **Third-party components** (see [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md)) carry their own
  advisories; we track and update them, and intend to run `cargo audit` / `cargo deny` in CI as the
  project matures.
- **No secrets** are bundled or logged; `.env` and config files are treated as sensitive.

Thank you for helping keep Freally Snipper and its users safe.
