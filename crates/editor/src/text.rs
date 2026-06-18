//! Text rendering for P4.4 — no egui, fully testable.
//!
//! A text object is rendered to a CPU **stamp** ([`RgbaImage`]) at its font-pixel
//! size: **`rustybuzz`** shapes each line (so Arabic joins correctly and Latin
//! kerns/ligates), then **`ab_glyph`** rasterizes the shaped glyph outlines. The
//! same stamp is uploaded for the on-screen preview *and* composited on Save, so
//! "what you see is what you get" holds exactly.
//!
//! P4.4 bundles **Noto Sans / Serif / Mono** (Latin · Greek · Cyrillic) as the
//! family choices, plus **Noto Sans Arabic** used automatically for Arabic text
//! (RTL + joining). Full multi-script coverage (CJK, Indic, …) + a pluggable
//! font-pack with per-glyph fallback land with **P6.3b**.

use ab_glyph::{Font, FontRef, Glyph, GlyphId, PxScale, ScaleFont};
use freally_capture::image::{Rgba, RgbaImage};
use rustybuzz::{Direction, Face, UnicodeBuffer};

/// Bundled Noto fonts (OFL) — see `THIRD-PARTY-NOTICES.md`.
static SANS: &[u8] = include_bytes!("../fonts/NotoSans-Regular.ttf");
static SERIF: &[u8] = include_bytes!("../fonts/NotoSerif-Regular.ttf");
static MONO: &[u8] = include_bytes!("../fonts/NotoSansMono-Regular.ttf");
static ARABIC: &[u8] = include_bytes!("../fonts/NotoSansArabic-Regular.ttf");

/// Largest stamp side (px), so an extreme font size can't blow up memory.
const MAX_STAMP: u32 = 8192;

/// Selectable text typeface (Latin · Greek · Cyrillic). Arabic always renders via
/// the bundled Noto Sans Arabic regardless of this choice (P4.4 fallback).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontFamily {
    Sans,
    Serif,
    Mono,
}

impl FontFamily {
    pub const ALL: [FontFamily; 3] = [Self::Sans, Self::Serif, Self::Mono];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sans => "Sans",
            Self::Serif => "Serif",
            Self::Mono => "Mono",
        }
    }

    fn bytes(self) -> &'static [u8] {
        match self {
            Self::Sans => SANS,
            Self::Serif => SERIF,
            Self::Mono => MONO,
        }
    }
}

/// Render `text` at `font_px` in `family`, tinted `color` (RGBA; the alpha is the
/// object opacity), to an RGBA stamp. Returns `None` for empty/degenerate input.
pub fn render(text: &str, font_px: f32, family: FontFamily, color: [u8; 4]) -> Option<RgbaImage> {
    let font_px = font_px.clamp(4.0, 512.0);
    if text.trim().is_empty() {
        return None;
    }

    // Lay out each line, collecting placed glyphs (in stamp pixel space).
    let lines: Vec<&str> = text.split('\n').collect();
    let mut placed: Vec<Placed> = Vec::new();
    let mut max_width = 0.0f32;
    let mut line_height = font_px * 1.3; // overwritten from real metrics below

    for (row, line) in lines.iter().enumerate() {
        let rtl = is_rtl(line);
        let primary = if rtl { ARABIC } else { family.bytes() };
        // Fall back to an installed system font for scripts the bundled fonts don't
        // cover (e.g. translated Tamil / Telugu / Thai / Hebrew / CJK).
        let (bytes, index) = font_for_line(primary, line);
        let (Some(face), Ok(ab)) = (
            Face::from_slice(bytes, index),
            FontRef::try_from_slice_and_index(bytes, index),
        ) else {
            continue;
        };
        let scaled = ab.as_scaled(PxScale::from(font_px));
        line_height = scaled.height() + scaled.line_gap();
        let baseline = row as f32 * line_height + scaled.ascent();

        let upem = face.units_per_em().max(1) as f32;
        let scale = font_px / upem;

        // Shape the (non-empty) line.
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(line);
        buffer.set_direction(if rtl {
            Direction::RightToLeft
        } else {
            Direction::LeftToRight
        });
        buffer.guess_segment_properties();
        let glyphs = rustybuzz::shape(&face, &[], buffer);

        let mut pen = 0.0f32;
        for (info, pos) in glyphs
            .glyph_infos()
            .iter()
            .zip(glyphs.glyph_positions().iter())
        {
            let gx = pen + pos.x_offset as f32 * scale;
            let gy = baseline - pos.y_offset as f32 * scale;
            placed.push(Placed {
                id: GlyphId(info.glyph_id as u16),
                bytes,
                index,
                x: gx,
                y: gy,
            });
            pen += pos.x_advance as f32 * scale;
        }
        max_width = max_width.max(pen);
    }

    let width = (max_width.ceil() as u32 + 2).min(MAX_STAMP);
    let height = ((lines.len() as f32 * line_height).ceil() as u32 + 2).min(MAX_STAMP);
    if width == 0 || height == 0 || placed.is_empty() {
        return None;
    }

    let mut stamp = RgbaImage::new(width, height);
    let [cr, cg, cb, ca] = color;
    let color_a = ca as f32 / 255.0;
    for g in &placed {
        // Re-parse is cheap and avoids threading a borrowed font through `Placed`.
        let Ok(font) = FontRef::try_from_slice_and_index(g.bytes, g.index) else {
            continue;
        };
        let glyph = Glyph {
            id: g.id,
            scale: PxScale::from(font_px),
            position: ab_glyph::point(g.x, g.y),
        };
        if let Some(outline) = font.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            let ox = bounds.min.x.round() as i32;
            let oy = bounds.min.y.round() as i32;
            outline.draw(|dx, dy, coverage| {
                let px = ox + dx as i32;
                let py = oy + dy as i32;
                if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                    return;
                }
                let sa = coverage.clamp(0.0, 1.0) * color_a;
                if sa <= 0.0 {
                    return;
                }
                let dst = stamp.get_pixel(px as u32, py as u32).0;
                stamp.put_pixel(px as u32, py as u32, Rgba(over(dst, [cr, cg, cb], sa)));
            });
        }
    }
    Some(stamp)
}

/// The font (bytes + face index) to render `line` with: the `primary` bundled font
/// if it covers every character, otherwise an installed system font that covers the
/// first uncovered one (so translated Indic / Thai / Hebrew / CJK text isn't tofu).
fn font_for_line(primary: &'static [u8], line: &str) -> (&'static [u8], u32) {
    let Ok(font) = FontRef::try_from_slice(primary) else {
        return (primary, 0);
    };
    let uncovered = line
        .chars()
        .find(|&c| !c.is_whitespace() && font.glyph_id(c).0 == 0);
    match uncovered {
        Some(c) => crate::fonts::fallback_for(c).unwrap_or((primary, 0)),
        None => (primary, 0),
    }
}

/// A shaped glyph placed in stamp space.
struct Placed {
    id: GlyphId,
    bytes: &'static [u8],
    index: u32,
    x: f32,
    y: f32,
}

/// Straight-alpha `src`-over: composite `rgb` at coverage-alpha `sa` over `dst`.
fn over(dst: [u8; 4], rgb: [u8; 3], sa: f32) -> [u8; 4] {
    let da = dst[3] as f32 / 255.0;
    let oa = sa + da * (1.0 - sa);
    if oa <= 0.0 {
        return [0, 0, 0, 0];
    }
    let blend = |s: u8, d: u8| {
        let v = (s as f32 * sa + d as f32 * da * (1.0 - sa)) / oa;
        v.round().clamp(0.0, 255.0) as u8
    };
    [
        blend(rgb[0], dst[0]),
        blend(rgb[1], dst[1]),
        blend(rgb[2], dst[2]),
        (oa * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

/// Whether a line is right-to-left (contains an Arabic-block character). Hebrew &
/// other RTL scripts get full coverage with the font-pack (P6.3b).
fn is_rtl(line: &str) -> bool {
    line.chars().any(|c| {
        matches!(c as u32,
            0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_cover_three_choices() {
        assert_eq!(FontFamily::ALL.len(), 3);
        assert_eq!(FontFamily::Serif.label(), "Serif");
    }

    #[test]
    fn empty_text_renders_nothing() {
        assert!(render("", 32.0, FontFamily::Sans, [0, 0, 0, 255]).is_none());
        assert!(render("   ", 32.0, FontFamily::Sans, [0, 0, 0, 255]).is_none());
    }

    #[test]
    fn latin_text_renders_a_stamp_with_ink() {
        let img = render("Hi", 48.0, FontFamily::Sans, [255, 0, 0, 255]).expect("stamp");
        assert!(img.width() > 0 && img.height() > 0);
        // Some pixels are inked (non-zero alpha) and tinted red.
        let inked = img.pixels().filter(|p| p.0[3] > 0).count();
        assert!(inked > 0, "expected glyph coverage");
        assert!(
            img.pixels().any(|p| p.0[3] > 0 && p.0[0] > p.0[2]),
            "red ink"
        );
    }

    #[test]
    fn arabic_text_shapes_and_renders() {
        // "مرحبا" (hello) — exercises the Arabic font + RTL joining path.
        let img = render("مرحبا", 48.0, FontFamily::Sans, [0, 0, 0, 255]).expect("stamp");
        assert!(img.width() > 0 && img.height() > 0);
        assert!(img.pixels().filter(|p| p.0[3] > 0).count() > 0);
    }

    #[test]
    fn is_rtl_detects_arabic_only() {
        assert!(is_rtl("مرحبا"));
        assert!(!is_rtl("hello"));
        assert!(!is_rtl("123"));
    }
}
