//! Colour emoji (P4.7) — egui-free, pure-Rust.
//!
//! A picked emoji is rasterized to a **full-colour** [`RgbaImage`] via **`swash`**
//! (COLR/CBDT colour glyphs) from **Noto Color Emoji** (OFL), then dropped onto
//! the canvas as an **Image object** (so it reuses P4.8's place/resize/opacity/
//! bake). The font (~24 MB) is **downloaded on demand** to the cache — not bundled
//! — so the build stays light. `rustybuzz` shapes the emoji string first, so
//! multi-codepoint ZWJ sequences (e.g. 👨‍👩‍👧) resolve to the right ligature glyph.

use std::fs;
use std::path::PathBuf;

use freally_capture::image::RgbaImage;
use rustybuzz::{Face, UnicodeBuffer};
use swash::scale::{ScaleContext, StrikeWith};
use swash::FontRef;

/// Noto Color Emoji (OFL; see THIRD-PARTY-NOTICES.md). CBDT bitmap colour font.
const FONT_URL: &str =
    "https://github.com/googlefonts/noto-emoji/raw/main/fonts/NotoColorEmoji.ttf";
/// Strike size requested from the bitmap font (Noto's strike is ~128 px).
const EMOJI_PX: f32 = 128.0;

fn font_path() -> Result<PathBuf, String> {
    directories::ProjectDirs::from("com", "Havoc Software", "Freally Snipper")
        .map(|d| d.cache_dir().join("emoji").join("NotoColorEmoji.ttf"))
        .ok_or_else(|| "no cache directory available".to_owned())
}

/// Download the emoji font if missing, then return its bytes. **Blocking + slow**
/// (the font is ~24 MB) — call off the UI thread.
pub fn ensure_font() -> Result<Vec<u8>, String> {
    let path = font_path()?;
    if !path.exists() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("create cache dir: {e}"))?;
        }
        let mut response = ureq::get(FONT_URL)
            .call()
            .map_err(|e| format!("download emoji font: {e}"))?;
        let bytes = response
            .body_mut()
            .with_config()
            .limit(128 * 1024 * 1024)
            .read_to_vec()
            .map_err(|e| format!("read emoji font: {e}"))?;
        let tmp = path.with_extension("part");
        fs::write(&tmp, &bytes).map_err(|e| format!("write emoji font: {e}"))?;
        fs::rename(&tmp, &path).map_err(|e| format!("finalize emoji font: {e}"))?;
    }
    fs::read(&path).map_err(|e| format!("read emoji font: {e}"))
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
