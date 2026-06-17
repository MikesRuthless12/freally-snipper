//! Live image filters (P4.5) — pure `RgbaImage` → `RgbaImage`, egui-free + tested.
//!
//! Each filter returns a new raster; the editor snapshots the old one for undo
//! before applying, so every filter is undoable and Save bakes the result. Alpha
//! is preserved (so a freeform-masked capture keeps its transparency).

use freally_capture::image::{Rgba, RgbaImage};

/// Map each pixel's RGB through `f`, preserving alpha.
fn map_rgb(img: &RgbaImage, f: impl Fn([u8; 3]) -> [u8; 3]) -> RgbaImage {
    let mut out = img.clone();
    for px in out.pixels_mut() {
        let [r, g, b, a] = px.0;
        let [nr, ng, nb] = f([r, g, b]);
        px.0 = [nr, ng, nb, a];
    }
    out
}

/// Rec. 601 luma.
fn luma(r: u8, g: u8, b: u8) -> u8 {
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32).round() as u8
}

pub fn grayscale(img: &RgbaImage) -> RgbaImage {
    map_rgb(img, |[r, g, b]| {
        let l = luma(r, g, b);
        [l, l, l]
    })
}

pub fn invert(img: &RgbaImage) -> RgbaImage {
    map_rgb(img, |[r, g, b]| [255 - r, 255 - g, 255 - b])
}

pub fn sepia(img: &RgbaImage) -> RgbaImage {
    map_rgb(img, |[r, g, b]| {
        let (rf, gf, bf) = (r as f32, g as f32, b as f32);
        let c = |v: f32| v.min(255.0) as u8;
        [
            c(0.393 * rf + 0.769 * gf + 0.189 * bf),
            c(0.349 * rf + 0.686 * gf + 0.168 * bf),
            c(0.272 * rf + 0.534 * gf + 0.131 * bf),
        ]
    })
}

/// Add `delta` to each channel (clamped). Positive brightens, negative darkens.
pub fn brightness(img: &RgbaImage, delta: i32) -> RgbaImage {
    let adj = |c: u8| (c as i32 + delta).clamp(0, 255) as u8;
    map_rgb(img, |[r, g, b]| [adj(r), adj(g), adj(b)])
}

/// Scale contrast around mid-grey by `factor` (>1 more, <1 less).
pub fn contrast(img: &RgbaImage, factor: f32) -> RgbaImage {
    let adj = |c: u8| {
        (128.0 + (c as f32 - 128.0) * factor)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    map_rgb(img, |[r, g, b]| [adj(r), adj(g), adj(b)])
}

/// Quantise each channel to `levels` (≥ 2) steps.
pub fn posterize(img: &RgbaImage, levels: u8) -> RgbaImage {
    let l = (levels.max(2) - 1) as f32;
    let adj = |c: u8| {
        ((c as f32 / 255.0 * l).round() / l * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    map_rgb(img, |[r, g, b]| [adj(r), adj(g), adj(b)])
}

/// Separable box blur of the given pixel `radius` (RGB; alpha preserved).
pub fn box_blur(img: &RgbaImage, radius: u32) -> RgbaImage {
    if radius == 0 {
        return img.clone();
    }
    let (w, h) = (img.width(), img.height());
    let r = radius as i32;
    let avg = |samples: &[[u8; 3]]| -> [u8; 3] {
        let n = samples.len().max(1) as u32;
        let mut s = [0u32; 3];
        for p in samples {
            for c in 0..3 {
                s[c] += p[c] as u32;
            }
        }
        [(s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8]
    };

    // Horizontal pass.
    let mut tmp = img.clone();
    let mut window: Vec<[u8; 3]> = Vec::with_capacity((2 * r + 1) as usize);
    for y in 0..h {
        for x in 0..w {
            window.clear();
            for dx in -r..=r {
                let xx = x as i32 + dx;
                if xx >= 0 && xx < w as i32 {
                    let p = img.get_pixel(xx as u32, y).0;
                    window.push([p[0], p[1], p[2]]);
                }
            }
            let [nr, ng, nb] = avg(&window);
            let a = img.get_pixel(x, y).0[3];
            tmp.put_pixel(x, y, Rgba([nr, ng, nb, a]));
        }
    }
    // Vertical pass.
    let mut out = tmp.clone();
    for y in 0..h {
        for x in 0..w {
            window.clear();
            for dy in -r..=r {
                let yy = y as i32 + dy;
                if yy >= 0 && yy < h as i32 {
                    let p = tmp.get_pixel(x, yy as u32).0;
                    window.push([p[0], p[1], p[2]]);
                }
            }
            let [nr, ng, nb] = avg(&window);
            let a = tmp.get_pixel(x, y).0[3];
            out.put_pixel(x, y, Rgba([nr, ng, nb, a]));
        }
    }
    out
}

/// Unsharp mask: `out = 2·orig − blurred` per channel (a crisp sharpen).
pub fn sharpen(img: &RgbaImage) -> RgbaImage {
    let blurred = box_blur(img, 1);
    let mut out = img.clone();
    for (o, b) in out.pixels_mut().zip(blurred.pixels()) {
        let s = |oc: u8, bc: u8| (oc as i32 * 2 - bc as i32).clamp(0, 255) as u8;
        o.0 = [
            s(o.0[0], b.0[0]),
            s(o.0[1], b.0[1]),
            s(o.0[2], b.0[2]),
            o.0[3],
        ];
    }
    out
}

/// Cartoonize: posterize the colours, then darken Sobel edges into outlines.
pub fn cartoonize(img: &RgbaImage) -> RgbaImage {
    let mut out = posterize(img, 6);
    let (w, h) = (img.width(), img.height());
    if w < 3 || h < 3 {
        return out;
    }
    let lum = |x: u32, y: u32| {
        let p = img.get_pixel(x, y).0;
        luma(p[0], p[1], p[2]) as i32
    };
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            // Sobel gradient magnitude of luma.
            let gx = (lum(x + 1, y - 1) + 2 * lum(x + 1, y) + lum(x + 1, y + 1))
                - (lum(x - 1, y - 1) + 2 * lum(x - 1, y) + lum(x - 1, y + 1));
            let gy = (lum(x - 1, y + 1) + 2 * lum(x, y + 1) + lum(x + 1, y + 1))
                - (lum(x - 1, y - 1) + 2 * lum(x, y - 1) + lum(x + 1, y - 1));
            let mag = ((gx * gx + gy * gy) as f32).sqrt();
            if mag > 80.0 {
                let p = out.get_pixel(x, y).0;
                // Darken toward black for the cartoon outline.
                out.put_pixel(x, y, Rgba([p[0] / 5, p[1] / 5, p[2] / 5, p[3]]));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(pixels: &[[u8; 4]], w: u32, h: u32) -> RgbaImage {
        let mut im = RgbaImage::new(w, h);
        for (i, px) in pixels.iter().enumerate() {
            im.put_pixel(i as u32 % w, i as u32 / w, Rgba(*px));
        }
        im
    }

    #[test]
    fn grayscale_equalizes_channels_and_keeps_alpha() {
        let g = grayscale(&img(&[[255, 0, 0, 200]], 1, 1));
        let p = g.get_pixel(0, 0).0;
        assert_eq!(p[0], p[1]);
        assert_eq!(p[1], p[2]);
        assert_eq!(p[3], 200); // alpha preserved
    }

    #[test]
    fn invert_inverts_rgb_only() {
        let p = invert(&img(&[[10, 20, 30, 128]], 1, 1)).get_pixel(0, 0).0;
        assert_eq!(p, [245, 235, 225, 128]);
    }

    #[test]
    fn brightness_clamps() {
        assert_eq!(
            brightness(&img(&[[250, 0, 5, 255]], 1, 1), 25)
                .get_pixel(0, 0)
                .0,
            [255, 25, 30, 255]
        );
        assert_eq!(
            brightness(&img(&[[10, 0, 5, 255]], 1, 1), -25)
                .get_pixel(0, 0)
                .0,
            [0, 0, 0, 255]
        );
    }

    #[test]
    fn posterize_two_levels_snaps_to_extremes() {
        let p = posterize(&img(&[[10, 130, 245, 255]], 1, 1), 2)
            .get_pixel(0, 0)
            .0;
        assert_eq!(p, [0, 255, 255, 255]);
    }

    #[test]
    fn box_blur_zero_radius_is_identity() {
        let src = img(&[[1, 2, 3, 4], [5, 6, 7, 8]], 2, 1);
        assert_eq!(box_blur(&src, 0), src);
    }

    #[test]
    fn sharpen_of_uniform_is_unchanged() {
        // 2·c − c == c on a flat region.
        let src = RgbaImage::from_pixel(4, 4, Rgba([100, 110, 120, 255]));
        let out = sharpen(&src);
        assert!(out.pixels().all(|p| p.0 == [100, 110, 120, 255]));
    }

    #[test]
    fn filters_preserve_dimensions() {
        let src = RgbaImage::from_pixel(8, 6, Rgba([40, 80, 120, 255]));
        for f in [
            grayscale(&src),
            sepia(&src),
            invert(&src),
            box_blur(&src, 2),
            sharpen(&src),
            contrast(&src, 1.2),
            posterize(&src, 4),
            cartoonize(&src),
        ] {
            assert_eq!((f.width(), f.height()), (8, 6));
        }
    }
}
