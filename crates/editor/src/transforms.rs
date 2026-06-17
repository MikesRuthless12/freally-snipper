//! Live transforms (P4.6) — pure `RgbaImage` → `RgbaImage`, egui-free + tested.
//!
//! Rotate (90°), flip, bevel and crop. Geometric transforms change the image and
//! its coordinate space, so the editor flattens overlay objects before applying
//! them (objects are baked into the new pixels); bevel is a same-size raster
//! effect that needs no flattening.

use freally_capture::image::{Rgba, RgbaImage};

/// Rotate 90° clockwise (dimensions swap).
pub fn rotate_cw(img: &RgbaImage) -> RgbaImage {
    let (w, h) = (img.width(), img.height());
    let mut out = RgbaImage::new(h, w);
    for dy in 0..w {
        for dx in 0..h {
            // out(dx, dy) ← in(dy, h - 1 - dx)
            let px = *img.get_pixel(dy, h - 1 - dx);
            out.put_pixel(dx, dy, px);
        }
    }
    out
}

/// Rotate 90° counter-clockwise (dimensions swap).
pub fn rotate_ccw(img: &RgbaImage) -> RgbaImage {
    let (w, h) = (img.width(), img.height());
    let mut out = RgbaImage::new(h, w);
    for dy in 0..w {
        for dx in 0..h {
            // out(dx, dy) ← in(w - 1 - dy, dx)
            let px = *img.get_pixel(w - 1 - dy, dx);
            out.put_pixel(dx, dy, px);
        }
    }
    out
}

/// Mirror horizontally (left↔right).
pub fn flip_h(img: &RgbaImage) -> RgbaImage {
    let (w, h) = (img.width(), img.height());
    let mut out = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            out.put_pixel(x, y, *img.get_pixel(w - 1 - x, y));
        }
    }
    out
}

/// Mirror vertically (top↔bottom).
pub fn flip_v(img: &RgbaImage) -> RgbaImage {
    let (w, h) = (img.width(), img.height());
    let mut out = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            out.put_pixel(x, y, *img.get_pixel(x, h - 1 - y));
        }
    }
    out
}

/// A framed 3-D bevel of `width` pixels: top/left edges are lit, bottom/right
/// edges shadowed, fading inward (same dimensions).
pub fn bevel(img: &RgbaImage, width: u32) -> RgbaImage {
    if width == 0 {
        return img.clone();
    }
    const STRENGTH: f32 = 0.7;
    let (w, h) = (img.width() as i32, img.height() as i32);
    let bw = width as i32;
    let mut out = img.clone();
    for y in 0..h {
        for x in 0..w {
            let (dl, dt, dr, db) = (x, y, w - 1 - x, h - 1 - y);
            let dmin = dl.min(dt).min(dr).min(db);
            if dmin >= bw {
                continue;
            }
            let f = (1.0 - dmin as f32 / bw as f32) * STRENGTH;
            let lit = dmin == dl || dmin == dt; // ties favour the lit (top/left) edge
            let p = img.get_pixel(x as u32, y as u32).0;
            let adj = |c: u8| {
                if lit {
                    (c as f32 + (255.0 - c as f32) * f).round() as u8
                } else {
                    (c as f32 * (1.0 - f)).round() as u8
                }
            };
            out.put_pixel(
                x as u32,
                y as u32,
                Rgba([adj(p[0]), adj(p[1]), adj(p[2]), p[3]]),
            );
        }
    }
    out
}

/// Crop to the pixel rectangle `(x, y, w, h)`, clamped to the image. Returns the
/// original if the rectangle is empty or fully outside.
pub fn crop(img: &RgbaImage, x: i32, y: i32, w: u32, h: u32) -> RgbaImage {
    let (iw, ih) = (img.width() as i32, img.height() as i32);
    let x0 = x.clamp(0, iw);
    let y0 = y.clamp(0, ih);
    let x1 = (x + w as i32).clamp(0, iw);
    let y1 = (y + h as i32).clamp(0, ih);
    if x1 <= x0 || y1 <= y0 {
        return img.clone();
    }
    let (cw, ch) = ((x1 - x0) as u32, (y1 - y0) as u32);
    let mut out = RgbaImage::new(cw, ch);
    for dy in 0..ch {
        for dx in 0..cw {
            out.put_pixel(dx, dy, *img.get_pixel(x0 as u32 + dx, y0 as u32 + dy));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2×3 image with a unique value per pixel (R channel = index).
    fn ramp(w: u32, h: u32) -> RgbaImage {
        let mut im = RgbaImage::new(w, h);
        let mut i = 0u8;
        for y in 0..h {
            for x in 0..w {
                im.put_pixel(x, y, Rgba([i, 0, 0, 255]));
                i += 1;
            }
        }
        im
    }

    #[test]
    fn rotate_cw_swaps_dims_and_corners() {
        let src = ramp(2, 3); // top-left=0, top-right=1, bottom-left=4, bottom-right=5
        let r = rotate_cw(&src);
        assert_eq!((r.width(), r.height()), (3, 2));
        // top-left of src (0) lands at top-right of dst.
        assert_eq!(r.get_pixel(r.width() - 1, 0).0[0], 0);
        // bottom-left of src (4) lands at top-left of dst.
        assert_eq!(r.get_pixel(0, 0).0[0], 4);
    }

    #[test]
    fn rotate_ccw_is_inverse_of_cw() {
        let src = ramp(4, 3);
        let back = rotate_ccw(&rotate_cw(&src));
        assert_eq!(back, src);
    }

    #[test]
    fn flips_mirror() {
        let src = ramp(2, 1); // [0,1]
        assert_eq!(flip_h(&src).get_pixel(0, 0).0[0], 1);
        let v = ramp(1, 2); // [0;1]
        assert_eq!(flip_v(&v).get_pixel(0, 0).0[0], 1);
    }

    #[test]
    fn crop_extracts_region_and_clamps() {
        let src = ramp(4, 4);
        let c = crop(&src, 1, 1, 2, 2);
        assert_eq!((c.width(), c.height()), (2, 2));
        assert_eq!(c.get_pixel(0, 0).0[0], 5); // src(1,1) = 1*4+1
                                               // Over-large request clamps to the image.
        let full = crop(&src, 0, 0, 99, 99);
        assert_eq!((full.width(), full.height()), (4, 4));
        // Empty request returns the original.
        assert_eq!(crop(&src, 0, 0, 0, 5), src);
    }

    #[test]
    fn bevel_lightens_top_left_darkens_bottom_right() {
        let src = RgbaImage::from_pixel(10, 10, Rgba([128, 128, 128, 255]));
        let b = bevel(&src, 3);
        assert!(b.get_pixel(0, 0).0[0] > 128, "top-left lit");
        assert!(b.get_pixel(9, 9).0[0] < 128, "bottom-right shadowed");
        assert_eq!(b.get_pixel(5, 5).0[0], 128, "interior unchanged");
        assert_eq!((b.width(), b.height()), (10, 10));
    }
}
