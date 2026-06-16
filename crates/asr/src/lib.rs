//! `freally-asr` — optional, local speech-to-text & caption translation.
//!
//! Placeholder for **Phase 6**. This is an *optional, free, non-owned add-on* —
//! manual captions are the owned default. Planned here:
//!
//! - Transcribe recorded audio with local **Whisper** (`whisper-rs`) → segment +
//!   word-level timestamps; **VAD**-gated (Silero) so cues exist only during speech.
//! - **Translate captions into any language** via a local, commercially-licensed
//!   MT model (OPUS-MT / M2M-100 — never NLLB, which is non-commercial), since
//!   Whisper alone only outputs the source language or English.
//! - A reusable **language selector** listing every language with **English first,
//!   then alphabetical**, plus a **"translate to my system language"** default that
//!   reads the OS display language so foreign-audio video is captioned natively.
//!
//! For now this crate only exposes its name so the workspace builds and is testable.

/// Identifier for this crate, surfaced in version banners and logs.
pub const CRATE_NAME: &str = "freally-asr";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_set() {
        assert_eq!(CRATE_NAME, "freally-asr");
    }
}
