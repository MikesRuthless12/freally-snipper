//! Optional downloadable assets (P4.11) — the models/fonts the editor's add-on
//! features need, **where** to fetch them (pinned, immutable refs where possible),
//! and **where** to install them. Centralised so every URL + filename lives in one
//! audited place (filenames are hardcoded literals — no caller-supplied paths, so
//! no traversal), and so the Models panel + the on-use download share one source.
//!
//! Integrity: downloads are over TLS from known hosts. Per-file SHA-256 pinning is
//! a tracked hardening item (see SECURITY.md).

use std::path::PathBuf;

use crate::download::{self, Progress};

/// One file of an [`Asset`]: where to download it from and what to save it as.
pub struct AssetFile {
    pub url: &'static str,
    pub name: &'static str,
}

/// A downloadable add-on (model or font) with user-facing copy + a size estimate.
pub struct Asset {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    /// Cache subdirectory the files install into.
    pub subdir: &'static str,
    pub files: &'static [AssetFile],
}

// OCR — the ocrs project's models (Apache-2.0); S3 paths are project-versioned.
const OCR_DETECTION: &str = "https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten";
const OCR_RECOGNITION: &str =
    "https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.rten";
// Noto Color Emoji (OFL).
const EMOJI_FONT: &str =
    "https://github.com/googlefonts/noto-emoji/raw/main/fonts/NotoColorEmoji.ttf";

pub static OCR: Asset = Asset {
    id: "ocr",
    title: "Text recognition (OCR)",
    description: "Lets \"Extract Text\" read the words out of an image. Pure-Rust, runs on CPU.",
    subdir: "ocr-models",
    files: &[
        AssetFile {
            url: OCR_DETECTION,
            name: "text-detection.rten",
        },
        AssetFile {
            url: OCR_RECOGNITION,
            name: "text-recognition.rten",
        },
    ],
};

pub static EMOJI: Asset = Asset {
    id: "emoji",
    title: "Colour emoji font",
    description: "Noto Color Emoji — renders the emoji you place in full colour.",
    subdir: "emoji",
    files: &[AssetFile {
        url: EMOJI_FONT,
        name: "NotoColorEmoji.ttf",
    }],
};

/// All assets, for the Models panel.
pub static ALL: &[&Asset] = &[&OCR, &EMOJI];

fn cache_root() -> Result<PathBuf, String> {
    directories::ProjectDirs::from("com", "Havoc Software", "Freally Snipper")
        .map(|d| d.cache_dir().to_path_buf())
        .ok_or_else(|| "no cache directory available".to_owned())
}

/// The install directory for `asset`.
pub fn dir(asset: &Asset) -> Result<PathBuf, String> {
    Ok(cache_root()?.join(asset.subdir))
}

/// Whether every file of `asset` is already on disk.
pub fn is_installed(asset: &Asset) -> bool {
    match dir(asset) {
        Ok(d) => asset.files.iter().all(|f| d.join(f.name).exists()),
        Err(_) => false,
    }
}

/// Exact total size of the installed files on disk (0 if any are missing). No
/// network — just `metadata().len()`.
pub fn installed_size(asset: &Asset) -> u64 {
    let Ok(d) = dir(asset) else {
        return 0;
    };
    asset
        .files
        .iter()
        .filter_map(|f| std::fs::metadata(d.join(f.name)).ok())
        .map(|m| m.len())
        .sum()
}

/// Exact total download size via HTTP `HEAD` (`Content-Length`) across the files.
/// **Blocking** — call off the UI thread.
pub fn remote_size(asset: &Asset) -> Result<u64, String> {
    let mut total = 0u64;
    for f in asset.files {
        let response = ureq::head(f.url)
            .call()
            .map_err(|e| format!("size of {}: {e}", f.name))?;
        let len = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| format!("no size for {}", f.name))?;
        total += len;
    }
    Ok(total)
}

/// Download any missing files for `asset`, reporting `(file_index, progress)` as it
/// streams. Returns the local paths in `asset.files` order. Blocking — worker only.
pub fn ensure(
    asset: &Asset,
    mut on_progress: impl FnMut(usize, Progress),
) -> Result<Vec<PathBuf>, String> {
    let d = dir(asset)?;
    let mut paths = Vec::with_capacity(asset.files.len());
    for (i, file) in asset.files.iter().enumerate() {
        let path = d.join(file.name);
        if !path.exists() {
            download::download_with_progress(file.url, &path, |p| on_progress(i, p))?;
        }
        paths.push(path);
    }
    Ok(paths)
}
