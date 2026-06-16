//! `freally-capture` — multi-monitor screen capture (image) for Freally Snipper.
//!
//! Placeholder for **Phase 1 — Capture core**. The real APIs land there:
//! monitor enumeration (`capture_all`), region capture (`capture_rect`), the
//! transparent always-on-top selection overlay, and the capture modes —
//! Rectangle, **Window (attach to an app window and grab exactly its bounds)**,
//! Freeform, and Full screen. For now this crate only exposes its name so the
//! workspace builds and is testable.

/// Identifier for this crate, surfaced in version banners and logs.
pub const CRATE_NAME: &str = "freally-capture";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_set() {
        assert_eq!(CRATE_NAME, "freally-capture");
    }
}
