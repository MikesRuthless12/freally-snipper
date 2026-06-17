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
  nothing anywhere.
- **Third-party components** (see [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md)) carry their own
  advisories; we track and update them, and intend to run `cargo audit` / `cargo deny` in CI as the
  project matures.
- **No secrets** are bundled or logged; `.env` and config files are treated as sensitive.

Thank you for helping keep Freally Snipper and its users safe.
