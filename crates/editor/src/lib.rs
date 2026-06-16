//! `freally-editor` — WYSIWYG image editor (Toolbar 2) for Freally Snipper.
//!
//! Placeholder for **Phase 4 — Image editor**: raster tools (pen / brush /
//! highlighter / two-mode eraser), movable & selectable text / shape / watermark
//! objects, live filters and transforms, eyedropper, undo/redo, and
//! save / copy / share. "Save writes exactly what you see." For now this crate
//! only exposes its name so the workspace builds and is testable.

/// Identifier for this crate, surfaced in version banners and logs.
pub const CRATE_NAME: &str = "freally-editor";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_set() {
        assert_eq!(CRATE_NAME, "freally-editor");
    }
}
