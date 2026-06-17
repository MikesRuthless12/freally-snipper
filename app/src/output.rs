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
/// losslessly as PNG for now (full WebP export lands with the editor in Phase 4).
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

    let path = unique_path(folder, ext);

    // JPEG has no alpha channel; drop it. PNG/BMP keep RGBA as-is.
    let result = if matches!(encoder, ::image::ImageFormat::Jpeg) {
        let rgb = ::image::DynamicImage::ImageRgba8(image.clone()).to_rgb8();
        rgb.save_with_format(&path, encoder)
    } else {
        image.save_with_format(&path, encoder)
    };

    result.map_err(std::io::Error::other)?;
    Ok(path)
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// A non-colliding path in `folder`: `Freally Snip <ms>.<ext>`, with a ` (n)`
/// suffix if two captures land in the same millisecond (so neither is lost).
fn unique_path(folder: &Path, ext: &str) -> PathBuf {
    let base = format!("Freally Snip {}", unix_millis());
    let mut path = folder.join(format!("{base}.{ext}"));
    let mut n = 1;
    while path.exists() {
        path = folder.join(format!("{base} ({n}).{ext}"));
        n += 1;
    }
    path
}
