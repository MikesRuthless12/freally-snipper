//! Where a finished capture goes: saved to a file (and, from Phase 1 P1.4, the
//! clipboard). Kept separate so the editor can reuse it in later phases.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use freally_capture::image::RgbaImage;

use crate::settings::ImageFormat;

/// Copy an RGBA capture to the system clipboard via a caller-owned clipboard.
///
/// The caller keeps the [`arboard::Clipboard`] alive (see `delivery`): on
/// X11/Wayland the clipboard is served by the owning context, so the image is
/// available to other apps only while that context lives.
pub fn set_clipboard_image(
    clipboard: &mut arboard::Clipboard,
    image: &RgbaImage,
) -> Result<(), arboard::Error> {
    clipboard.set_image(arboard::ImageData {
        width: image.width() as usize,
        height: image.height() as usize,
        bytes: Cow::Borrowed(image.as_raw()),
    })
}

/// Save a capture into `folder` using `format`, returning the written path.
///
/// WebP encoding is not available in `image` 0.25, so a WebP selection is saved
/// losslessly as PNG for now (until a WebP encoder is wired in).
pub fn save_capture(
    image: &RgbaImage,
    folder: &Path,
    format: ImageFormat,
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(folder)?;

    let (ext, encoder) = match format {
        ImageFormat::Png | ImageFormat::WebP => ("png", ::image::ImageFormat::Png),
        ImageFormat::Jpg => ("jpg", ::image::ImageFormat::Jpeg),
        ImageFormat::Bmp => ("bmp", ::image::ImageFormat::Bmp),
    };

    let path = unique_path(folder, "Freally Snip", ext);

    // JPEG has no alpha channel. Don't just drop alpha (that would leak the
    // masked-out pixels of a Freeform crop back in as a full rectangle) — instead
    // composite over white, so transparent areas become white like the Win11
    // Snipping Tool. PNG/BMP keep the RGBA (and its transparency) as-is.
    let result = if matches!(encoder, ::image::ImageFormat::Jpeg) {
        flatten_over_white(image).save_with_format(&path, encoder)
    } else {
        image.save_with_format(&path, encoder)
    };

    result.map_err(std::io::Error::other)?;
    Ok(path)
}

/// Composite an RGBA image over a white background, returning opaque RGB.
/// Transparent (e.g. freeform-masked) pixels become white instead of revealing
/// the pixels left underneath the mask.
fn flatten_over_white(image: &RgbaImage) -> ::image::RgbImage {
    let mut rgb = ::image::RgbImage::new(image.width(), image.height());
    for (dst, src) in rgb.pixels_mut().zip(image.pixels()) {
        let a = src[3] as u32;
        // out = src·(a/255) + 255·(1 - a/255)
        let blend = |c: u8| ((c as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
        *dst = ::image::Rgb([blend(src[0]), blend(src[1]), blend(src[2])]);
    }
    rgb
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// A non-colliding `.fvid` recording path in `folder`
/// (`Freally Recording <ms>.fvid`).
pub fn recording_path(folder: &Path) -> PathBuf {
    unique_path(folder, "Freally Recording", "fvid")
}

/// A non-colliding path in `folder`: `<prefix> <ms>.<ext>`, with a ` (n)` suffix
/// if two outputs land in the same millisecond (so neither is lost).
fn unique_path(folder: &Path, prefix: &str, ext: &str) -> PathBuf {
    let base = format!("{prefix} {}", unix_millis());
    let mut path = folder.join(format!("{base}.{ext}"));
    let mut n = 1;
    while path.exists() {
        path = folder.join(format!("{base} ({n}).{ext}"));
        n += 1;
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::image::Rgba;

    #[test]
    fn flatten_over_white_composites_alpha() {
        let mut img = RgbaImage::new(3, 1);
        img.put_pixel(0, 0, Rgba([10, 20, 30, 255])); // opaque → unchanged
        img.put_pixel(1, 0, Rgba([10, 20, 30, 0])); // transparent → white
        img.put_pixel(2, 0, Rgba([0, 0, 0, 128])); // half → ~mid gray
        let rgb = flatten_over_white(&img);
        assert_eq!(rgb.get_pixel(0, 0).0, [10, 20, 30]);
        assert_eq!(rgb.get_pixel(1, 0).0, [255, 255, 255]);
        assert!((126..=129).contains(&rgb.get_pixel(2, 0).0[0]));
    }
}
