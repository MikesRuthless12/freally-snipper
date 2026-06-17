//! Raster baking for the P4.2 markup tools — pure image processing, no egui.
//!
//! A freehand stroke is a list of **image-space** points with a pixel `radius`.
//! Baking happens once, when the stroke commits (mouse-up): we rasterize the
//! stroke to an anti-aliased **coverage** buffer over its bounding box, then
//! composite it into the working raster according to the tool's [`Paint`] mode.
//!
//! Coverage is **max-merged** across the stroke's segments (a union, not a sum),
//! so a translucent highlighter never double-darkens where it overlaps itself.
//! Keeping this egui-free makes the tricky parts — the text-aware mask and the
//! per-tool compositing the P4.2 acceptance turns on — directly unit-testable.

use freally_capture::image::{Rgba, RgbaImage};

/// How a baked stroke combines with the pixels underneath it.
pub enum Paint {
    /// Opaque colour — Pen / Brush.
    Solid([u8; 3]),
    /// Translucent highlight at `alpha` (0..=1). When `text_only`, the highlight
    /// is restricted to detected text pixels within the stroke band (P4.2's
    /// text-aware highlighter); the image background is left untouched.
    Highlight {
        color: [u8; 3],
        alpha: f32,
        text_only: bool,
    },
    /// Erase to opaque white.
    White,
    /// Erase markup only: restore the original captured (pristine) pixels.
    Restore,
}

/// An image-space rectangle of pixels (the area a stroke can touch).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Bbox {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// Bake `points` (image-space, `radius` px) into `image` using `paint`.
/// `pristine` is the original capture, used by [`Paint::Restore`]; it must match
/// `image`'s dimensions (restore is skipped per-pixel where it does not).
pub fn bake_stroke(
    image: &mut RgbaImage,
    pristine: &RgbaImage,
    points: &[(f32, f32)],
    radius: f32,
    paint: &Paint,
) {
    if points.is_empty() || radius <= 0.0 {
        return;
    }
    let Some(bbox) = stroke_bbox(points, radius, image.width(), image.height()) else {
        return;
    };
    let cov = rasterize_coverage(points, radius, bbox);
    let text = match paint {
        Paint::Highlight {
            text_only: true, ..
        } => Some(text_mask(image, bbox, &cov)),
        _ => None,
    };
    composite(image, Some(pristine), bbox, &cov, text.as_deref(), paint);
}

/// Bake an anti-aliased solid-colour path (a polyline with round caps) into
/// `image` — the shape-outline / line / arrow primitive for P4.3 objects.
pub fn bake_solid_path(image: &mut RgbaImage, points: &[(f32, f32)], radius: f32, color: [u8; 3]) {
    if points.is_empty() || radius <= 0.0 {
        return;
    }
    let Some(bbox) = stroke_bbox(points, radius, image.width(), image.height()) else {
        return;
    };
    let cov = rasterize_coverage(points, radius, bbox);
    composite(image, None, bbox, &cov, None, &Paint::Solid(color));
}

/// Alpha-composite (`src`-over) a solid `color` into the axis-aligned rectangle
/// spanning corners `a`–`b` (image space) — a filled rect / shape interior.
pub fn fill_rect(image: &mut RgbaImage, a: (f32, f32), b: (f32, f32), color: [u8; 4]) {
    let (w, h) = (image.width(), image.height());
    let x0 = a.0.min(b.0).floor().clamp(0.0, w as f32) as u32;
    let y0 = a.1.min(b.1).floor().clamp(0.0, h as f32) as u32;
    let x1 = a.0.max(b.0).ceil().clamp(0.0, w as f32) as u32;
    let y1 = a.1.max(b.1).ceil().clamp(0.0, h as f32) as u32;
    for y in y0..y1 {
        for x in x0..x1 {
            let out = blend_over(image.get_pixel(x, y).0, color);
            image.put_pixel(x, y, Rgba(out));
        }
    }
}

/// Alpha-composite a solid `color` into the filled ellipse with `center` and
/// `radius` (rx, ry) in image space.
pub fn fill_ellipse(image: &mut RgbaImage, center: (f32, f32), radius: (f32, f32), color: [u8; 4]) {
    let (rx, ry) = radius;
    if rx <= 0.0 || ry <= 0.0 {
        return;
    }
    let (w, h) = (image.width(), image.height());
    let x0 = (center.0 - rx).floor().clamp(0.0, w as f32) as u32;
    let y0 = (center.1 - ry).floor().clamp(0.0, h as f32) as u32;
    let x1 = (center.0 + rx).ceil().clamp(0.0, w as f32) as u32;
    let y1 = (center.1 + ry).ceil().clamp(0.0, h as f32) as u32;
    for y in y0..y1 {
        for x in x0..x1 {
            let nx = (x as f32 + 0.5 - center.0) / rx;
            let ny = (y as f32 + 0.5 - center.1) / ry;
            if nx * nx + ny * ny <= 1.0 {
                let out = blend_over(image.get_pixel(x, y).0, color);
                image.put_pixel(x, y, Rgba(out));
            }
        }
    }
}

/// Alpha-composite the RGBA `stamp` onto `image` with its top-left at image pixel
/// `(x, y)` — used to bake a rendered text stamp (P4.4) into the raster.
pub fn blit_over(image: &mut RgbaImage, stamp: &RgbaImage, x: i32, y: i32) {
    for sy in 0..stamp.height() {
        let py = y + sy as i32;
        if py < 0 || py >= image.height() as i32 {
            continue;
        }
        for sx in 0..stamp.width() {
            let px = x + sx as i32;
            if px < 0 || px >= image.width() as i32 {
                continue;
            }
            let s = stamp.get_pixel(sx, sy).0;
            if s[3] == 0 {
                continue;
            }
            let out = blend_over(image.get_pixel(px as u32, py as u32).0, s);
            image.put_pixel(px as u32, py as u32, Rgba(out));
        }
    }
}

/// Straight alpha `src`-over composite of `color` onto `dst` (both RGBA bytes).
fn blend_over(dst: [u8; 4], color: [u8; 4]) -> [u8; 4] {
    let a = color[3] as f32 / 255.0;
    if a <= 0.0 {
        return dst;
    }
    if a >= 1.0 {
        return color;
    }
    let blend = |d: u8, s: u8| lerp(d, s, a);
    [
        blend(dst[0], color[0]),
        blend(dst[1], color[1]),
        blend(dst[2], color[2]),
        ((color[3] as f32) + (dst[3] as f32) * (1.0 - a)).round() as u8,
    ]
}

/// The pixel rectangle the stroke can touch, clamped to the image. `None` if it
/// falls entirely outside.
fn stroke_bbox(points: &[(f32, f32)], radius: f32, img_w: u32, img_h: u32) -> Option<Bbox> {
    if img_w == 0 || img_h == 0 {
        return None;
    }
    let pad = radius + 1.0; // +1 for the anti-alias falloff
    let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
    let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
    for &(x, y) in points {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    let x0 = ((min_x - pad).floor()).clamp(0.0, img_w as f32) as u32;
    let y0 = ((min_y - pad).floor()).clamp(0.0, img_h as f32) as u32;
    let x1 = ((max_x + pad).ceil()).clamp(0.0, img_w as f32) as u32;
    let y1 = ((max_y + pad).ceil()).clamp(0.0, img_h as f32) as u32;
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(Bbox {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    })
}

/// Anti-aliased coverage (0..=1) of the stroke over `bbox`, max-merged across
/// segments so self-overlap counts once.
fn rasterize_coverage(points: &[(f32, f32)], radius: f32, bbox: Bbox) -> Vec<f32> {
    let mut cov = vec![0.0f32; (bbox.w * bbox.h) as usize];
    let seg = |a: (f32, f32), b: (f32, f32), cov: &mut [f32]| {
        // Local bbox of this capsule, clamped to `bbox`.
        let pad = radius + 1.0;
        let lx0 = ((a.0.min(b.0) - pad).floor()).max(bbox.x as f32) as u32;
        let ly0 = ((a.1.min(b.1) - pad).floor()).max(bbox.y as f32) as u32;
        let lx1 = ((a.0.max(b.0) + pad).ceil()).min((bbox.x + bbox.w) as f32) as u32;
        let ly1 = ((a.1.max(b.1) + pad).ceil()).min((bbox.y + bbox.h) as f32) as u32;
        for y in ly0..ly1 {
            for x in lx0..lx1 {
                let d = dist_point_to_segment((x as f32 + 0.5, y as f32 + 0.5), a, b);
                let c = (radius + 0.5 - d).clamp(0.0, 1.0);
                if c > 0.0 {
                    let idx = ((y - bbox.y) * bbox.w + (x - bbox.x)) as usize;
                    if c > cov[idx] {
                        cov[idx] = c;
                    }
                }
            }
        }
    };
    if points.len() == 1 {
        seg(points[0], points[0], &mut cov);
    } else {
        for pair in points.windows(2) {
            seg(pair[0], pair[1], &mut cov);
        }
    }
    cov
}

/// Distance from point `p` to segment `a`–`b` (to the point itself if `a == b`).
fn dist_point_to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let (apx, apy) = (p.0 - a.0, p.1 - a.1);
    let len2 = abx * abx + aby * aby;
    let t = if len2 <= f32::EPSILON {
        0.0
    } else {
        ((apx * abx + apy * aby) / len2).clamp(0.0, 1.0)
    };
    let (cx, cy) = (a.0 + abx * t, a.1 + aby * t);
    ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt()
}

/// Detect text pixels within the stroke band for the text-aware highlighter.
///
/// Own heuristic (no OCR — Tesseract is the optional P4.6 path): over the band
/// pixels (coverage > 0), take the **most common luminance** as the background
/// (text is the minority), then mark a pixel as text when its luminance differs
/// from that background by more than [`TEXT_CONTRAST`]. Reliable on the common
/// snip — screenshots and documents (dark text on light, or the reverse).
fn text_mask(image: &RgbaImage, bbox: Bbox, cov: &[f32]) -> Vec<bool> {
    /// Luminance gap (0..=255) a pixel must clear from the background to count as text.
    const TEXT_CONTRAST: i32 = 64;

    let mut hist = [0u32; 256];
    for y in 0..bbox.h {
        for x in 0..bbox.w {
            let idx = (y * bbox.w + x) as usize;
            if cov[idx] > 0.0 {
                let px = image.get_pixel(bbox.x + x, bbox.y + y);
                hist[luma(px) as usize] += 1;
            }
        }
    }
    // Background = the dominant luminance among the covered pixels.
    let bg = hist
        .iter()
        .enumerate()
        .max_by_key(|(_, &c)| c)
        .map(|(l, _)| l as i32)
        .unwrap_or(255);

    let mut mask = vec![false; (bbox.w * bbox.h) as usize];
    for y in 0..bbox.h {
        for x in 0..bbox.w {
            let idx = (y * bbox.w + x) as usize;
            if cov[idx] > 0.0 {
                let px = image.get_pixel(bbox.x + x, bbox.y + y);
                mask[idx] = (luma(px) as i32 - bg).abs() > TEXT_CONTRAST;
            }
        }
    }
    mask
}

/// Rec. 601 luma of an RGBA pixel (alpha ignored).
fn luma(px: &Rgba<u8>) -> u8 {
    let [r, g, b, _] = px.0;
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32).round() as u8
}

/// Composite the coverage buffer into `image` per the tool's [`Paint`] mode.
/// `pristine` (for [`Paint::Restore`]) is optional — shape baking passes `None`.
fn composite(
    image: &mut RgbaImage,
    pristine: Option<&RgbaImage>,
    bbox: Bbox,
    cov: &[f32],
    text: Option<&[bool]>,
    paint: &Paint,
) {
    // Pristine is only usable for Restore when it matches the image dimensions.
    let restore = pristine.filter(|p| p.width() == image.width() && p.height() == image.height());
    for y in 0..bbox.h {
        for x in 0..bbox.w {
            let idx = (y * bbox.w + x) as usize;
            let c = cov[idx];
            if c <= 0.0 {
                continue;
            }
            // Text-aware highlighter: only paint detected text pixels.
            if let Some(text) = text {
                if !text[idx] {
                    continue;
                }
            }
            let (px, py) = (bbox.x + x, bbox.y + y);
            let src = image.get_pixel(px, py).0;
            let out = match paint {
                Paint::Solid(rgb) => [
                    lerp(src[0], rgb[0], c),
                    lerp(src[1], rgb[1], c),
                    lerp(src[2], rgb[2], c),
                    lerp(src[3], 255, c),
                ],
                Paint::Highlight { color, alpha, .. } => {
                    let eff = c * alpha.clamp(0.0, 1.0);
                    [
                        lerp(src[0], color[0], eff),
                        lerp(src[1], color[1], eff),
                        lerp(src[2], color[2], eff),
                        src[3],
                    ]
                }
                Paint::White => [
                    lerp(src[0], 255, c),
                    lerp(src[1], 255, c),
                    lerp(src[2], 255, c),
                    lerp(src[3], 255, c),
                ],
                Paint::Restore => {
                    let Some(p) = restore else {
                        continue;
                    };
                    let p = p.get_pixel(px, py).0;
                    [
                        lerp(src[0], p[0], c),
                        lerp(src[1], p[1], c),
                        lerp(src[2], p[2], c),
                        lerp(src[3], p[3], c),
                    ]
                }
            };
            image.put_pixel(px, py, Rgba(out));
        }
    }
}

/// Linear blend from `a` to `b` by `t` (0..=1), rounded to the nearest byte.
fn lerp(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t.clamp(0.0, 1.0)).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba(rgba))
    }

    #[test]
    fn pen_paints_opaque_color_under_the_stroke() {
        let mut img = solid(20, 20, [255, 255, 255, 255]);
        let pristine = img.clone();
        bake_stroke(
            &mut img,
            &pristine,
            &[(10.0, 10.0)],
            3.0,
            &Paint::Solid([255, 0, 0]),
        );
        // Centre is fully painted red; a far corner is untouched white.
        assert_eq!(img.get_pixel(10, 10).0, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(0, 0).0, [255, 255, 255, 255]);
    }

    #[test]
    fn erase_to_white_paints_white() {
        let mut img = solid(10, 10, [10, 20, 30, 255]);
        let pristine = img.clone();
        bake_stroke(&mut img, &pristine, &[(5.0, 5.0)], 2.0, &Paint::White);
        assert_eq!(img.get_pixel(5, 5).0, [255, 255, 255, 255]);
    }

    #[test]
    fn erase_markup_only_restores_pristine_pixels() {
        // Pristine is blue; current was scribbled red. Restore brings blue back.
        let pristine = solid(10, 10, [0, 0, 200, 255]);
        let mut img = solid(10, 10, [200, 0, 0, 255]);
        bake_stroke(&mut img, &pristine, &[(5.0, 5.0)], 2.0, &Paint::Restore);
        assert_eq!(img.get_pixel(5, 5).0, [0, 0, 200, 255]);
        // Outside the stroke stays the scribbled colour.
        assert_eq!(img.get_pixel(0, 0).0, [200, 0, 0, 255]);
    }

    #[test]
    fn free_highlight_tints_everything_under_the_band() {
        // White background + one black "text" column at x = 5.
        let mut img = solid(12, 12, [255, 255, 255, 255]);
        for y in 0..12 {
            img.put_pixel(5, y, Rgba([0, 0, 0, 255]));
        }
        let pristine = img.clone();
        // A wide horizontal free highlight across the middle row.
        let pts: Vec<(f32, f32)> = (1..11).map(|x| (x as f32, 6.0)).collect();
        bake_stroke(
            &mut img,
            &pristine,
            &pts,
            2.0,
            &Paint::Highlight {
                color: [255, 255, 0],
                alpha: 0.5,
                text_only: false,
            },
        );
        // Free mode tints the white background too (it's no longer pure white).
        let bg = img.get_pixel(2, 6).0;
        assert!(
            bg != [255, 255, 255, 255],
            "free highlight should tint background"
        );
    }

    #[test]
    fn text_aware_highlight_spares_the_background() {
        // Same scene: white background + a black text column.
        let mut img = solid(12, 12, [255, 255, 255, 255]);
        for y in 0..12 {
            img.put_pixel(5, y, Rgba([0, 0, 0, 255]));
        }
        let pristine = img.clone();
        let pts: Vec<(f32, f32)> = (1..11).map(|x| (x as f32, 6.0)).collect();
        bake_stroke(
            &mut img,
            &pristine,
            &pts,
            2.0,
            &Paint::Highlight {
                color: [255, 255, 0],
                alpha: 0.5,
                text_only: true,
            },
        );
        // The white background under the band is left untouched...
        assert_eq!(img.get_pixel(2, 6).0, [255, 255, 255, 255]);
        // ...while the dark text pixel is tinted toward yellow.
        let text_px = img.get_pixel(5, 6).0;
        assert!(
            text_px != [0, 0, 0, 255],
            "text pixel should be highlighted"
        );
    }

    #[test]
    fn stroke_outside_the_image_is_a_no_op() {
        let mut img = solid(8, 8, [1, 2, 3, 255]);
        let pristine = img.clone();
        bake_stroke(
            &mut img,
            &pristine,
            &[(100.0, 100.0)],
            2.0,
            &Paint::Solid([9, 9, 9]),
        );
        assert!(img.pixels().all(|p| p.0 == [1, 2, 3, 255]));
    }

    #[test]
    fn fill_rect_paints_the_interior_only() {
        let mut img = solid(10, 10, [255, 255, 255, 255]);
        fill_rect(&mut img, (2.0, 2.0), (8.0, 8.0), [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(5, 5).0, [255, 0, 0, 255]); // inside → red
        assert_eq!(img.get_pixel(0, 0).0, [255, 255, 255, 255]); // outside → white
    }

    #[test]
    fn fill_rect_alpha_blends_when_translucent() {
        let mut img = solid(4, 4, [0, 0, 0, 255]);
        fill_rect(&mut img, (0.0, 0.0), (4.0, 4.0), [255, 255, 255, 128]);
        // Half-opacity white over black ≈ mid grey.
        let g = img.get_pixel(1, 1).0[0];
        assert!((120..=136).contains(&g), "got {g}");
    }

    #[test]
    fn fill_ellipse_fills_center_not_corner() {
        let mut img = solid(20, 20, [255, 255, 255, 255]);
        fill_ellipse(&mut img, (10.0, 10.0), (8.0, 8.0), [0, 128, 0, 255]);
        assert_eq!(img.get_pixel(10, 10).0, [0, 128, 0, 255]); // centre filled
        assert_eq!(img.get_pixel(0, 0).0, [255, 255, 255, 255]); // corner spared
    }

    #[test]
    fn solid_path_draws_a_line() {
        let mut img = solid(20, 5, [255, 255, 255, 255]);
        bake_solid_path(&mut img, &[(2.0, 2.0), (18.0, 2.0)], 1.0, [0, 0, 0]);
        assert_eq!(img.get_pixel(10, 2).0, [0, 0, 0, 255]); // on the line
        assert_eq!(img.get_pixel(10, 4).0, [255, 255, 255, 255]); // off the line
    }
}
