//! `freally-video` — the OWNED video codec + light video editor for Freally Snipper.
//!
//! Placeholder for **Phase 5 (codec + recording)** and **Phase 6 (timeline,
//! captions, export)**. `freally-video` is built only from expired-patent /
//! public-domain techniques (intra frames via the owned image codecs +
//! inter-frame delta + RLE/Huffman) → 100% owned + patent-safe, and is the
//! default record/project format. Recording supports region / full screen and
//! **attach-to-window** (record only a chosen app window, following it as it
//! moves/resizes). For now this crate only exposes its name so the workspace
//! builds and is testable.

/// Identifier for this crate, surfaced in version banners and logs.
pub const CRATE_NAME: &str = "freally-video";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_set() {
        assert_eq!(CRATE_NAME, "freally-video");
    }
}
