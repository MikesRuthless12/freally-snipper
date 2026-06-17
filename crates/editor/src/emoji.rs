//! Colour emoji (P4.7) — egui-free, pure-Rust.
//!
//! A picked emoji is rasterized to a **full-colour** [`RgbaImage`] via **`swash`**
//! (COLR/CBDT colour glyphs) from **Noto Color Emoji** (OFL), then dropped onto
//! the canvas as an **Image object** (so it reuses P4.8's place/resize/opacity/
//! bake). The font (~24 MB) is **downloaded on demand** to the cache — not bundled
//! — so the build stays light. `rustybuzz` shapes the emoji string first, so
//! multi-codepoint ZWJ sequences (e.g. 👨‍👩‍👧) resolve to the right ligature glyph.

use freally_capture::image::RgbaImage;
use rustybuzz::{Face, UnicodeBuffer};
use swash::scale::{ScaleContext, StrikeWith};
use swash::FontRef;

use crate::download::Progress;
use crate::models;

/// Strike size requested from the bitmap font (Noto's strike is ~128 px).
const EMOJI_PX: f32 = 128.0;

/// Download the emoji font on demand (see [`crate::models`]) and return its bytes,
/// reporting download progress. **Blocking + slow** (~24 MB) — call off the UI thread.
pub fn ensure_font(on_progress: impl FnMut(usize, Progress)) -> Result<Vec<u8>, String> {
    let paths = models::ensure(&models::EMOJI, on_progress)?;
    std::fs::read(&paths[0]).map_err(|e| format!("read emoji font: {e}"))
}

/// Rasterize `emoji` to a colour [`RgbaImage`] using `font_bytes`. Fast + sync.
/// Returns `None` if the font lacks the glyph.
pub fn rasterize(font_bytes: &[u8], emoji: &str) -> Option<RgbaImage> {
    // Shape with rustybuzz so ZWJ sequences map to the right ligature glyph.
    let face = Face::from_slice(font_bytes, 0)?;
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(emoji);
    buffer.guess_segment_properties();
    let shaped = rustybuzz::shape(&face, &[], buffer);
    let glyph_id = shaped.glyph_infos().first()?.glyph_id as u16;
    if glyph_id == 0 {
        return None; // .notdef — unsupported by the font
    }

    // Rasterize the colour bitmap with swash.
    let font = FontRef::from_index(font_bytes, 0)?;
    let mut context = ScaleContext::new();
    let mut scaler = context.builder(font).size(EMOJI_PX).build();
    let image = scaler.scale_color_bitmap(glyph_id, StrikeWith::BestFit)?;
    let (w, h) = (image.placement.width, image.placement.height);
    if w == 0 || h == 0 || image.data.len() < (w as usize * h as usize * 4) {
        return None;
    }

    // swash colour-bitmap data is straight-alpha RGBA, row-major.
    let mut out = RgbaImage::new(w, h);
    for (i, px) in out.pixels_mut().enumerate() {
        let o = i * 4;
        px.0 = [
            image.data[o],
            image.data[o + 1],
            image.data[o + 2],
            image.data[o + 3],
        ];
    }
    Some(out)
}

/// Emoji matching `query` (by name), capped at `limit`. An empty query returns the
/// first `limit` emoji (a sensible default grid).
pub fn search(query: &str, limit: usize) -> Vec<(&'static str, &'static str)> {
    let q = query.trim().to_lowercase();
    emojis::iter()
        .filter(|e| q.is_empty() || e.name().to_lowercase().contains(&q))
        .take(limit)
        .map(|e| (e.as_str(), e.name()))
        .collect()
}
