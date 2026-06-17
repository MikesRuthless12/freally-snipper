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
| [`serde`](https://serde.rs) / [`serde_json`](https://crates.io/crates/serde_json) | settings (de)serialization | MIT OR Apache-2.0 |
| [`directories`](https://crates.io/crates/directories) | OS config/data paths | MIT OR Apache-2.0 |
| [`log`](https://crates.io/crates/log) | logging facade (capture crate) | MIT OR Apache-2.0 |

Transitive Rust dependencies are MIT / Apache-2.0 / BSD / Zlib / MPL. Verify the full set with
`cargo about` or `cargo deny` before any release.

> **Linux note:** `rfd` uses the **XDG Desktop Portal** (D-Bus) for the folder picker and `xcap`
> uses **PipeWire** for capture, so a Linux build links `libpipewire`, `libwayland`, `libxcb`, and
> related system libraries (see `README.md` for the full `apt` list).

## Planned components (later phases — listed now for licensing clarity)

| Component | Role | License | Notes |
|-----------|------|---------|-------|
| [Noto fonts](https://fonts.google.com/noto) | multilingual text & captions | SIL OFL 1.1 | bundled *as Noto* with attribution; free for commercial use |
| [OpenAI Whisper](https://github.com/openai/whisper) + [whisper.cpp](https://github.com/ggerganov/whisper.cpp) / [`whisper-rs`](https://crates.io/crates/whisper-rs) | optional local speech-to-text | MIT | optional add-on; **manual captions are the owned default** |
| [Silero VAD](https://github.com/snakers4/silero-vad) | voice-activity detection | MIT | optional |
| Translation model ([M2M-100](https://github.com/facebookresearch/fairseq/tree/main/examples/m2m_100)) | translate captions to any language | MIT | optional; **avoid NLLB-200 (CC-BY-NC)** |
| TTS / voice-clone ([Piper](https://github.com/rhasspy/piper); [OpenVoice](https://github.com/myshell-ai/OpenVoice) + [MeloTTS](https://github.com/myshell-ai/MeloTTS)) | audio dubbing | **MIT** | optional; **avoid Coqui XTTS (CPML)** |
| [Tesseract OCR](https://github.com/tesseract-ocr/tesseract) | text extraction + text-aware highlight | **Apache-2.0** | optional; local; multilingual lang-data on demand |
| [ffmpeg](https://ffmpeg.org) via [`ffmpeg-sidecar`](https://crates.io/crates/ffmpeg-sidecar) | optional MP4/WebM export | **LGPL** (use the LGPL build; do **not** `--enable-gpl`) | optional convenience export; **`freally-video` is the default** |

## Codec / patent note

H.264 and AAC are **patent-encumbered**. Freally Snipper defaults to its own **`freally-video`**
codec, built only from expired-patent / public-domain techniques. The optional ffmpeg export
defaults to **royalty-free VP9 / Opus / WebM** (and VP8); H.264/MP4 is offered only where the
bundled ffmpeg build's license permits.

## Owned components (not third-party)

`freally-video` (codec), the `freally-*` image codecs, and the future `freally-font` typeface are
original works © Mike Weaver, covered by [`LICENSE`](LICENSE) — they are not third-party components.
