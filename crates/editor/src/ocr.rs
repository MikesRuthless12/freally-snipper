//! OCR / Extract-Text (P4.6b) — egui-free, pure-Rust.
//!
//! Uses **`ocrs` + `rten`** (MIT) — no Tesseract C++ system deps. The detection +
//! recognition models (~6 MB + ~12 MB) are **downloaded on demand** to the OS
//! cache dir on first use (not bundled), so the build stays light. Everything
//! here is blocking + slow — the editor runs it on a worker thread.

use std::fs;
use std::path::{Path, PathBuf};

use freally_capture::image::RgbaImage;

/// Official ocrs models (Apache-2.0; see THIRD-PARTY-NOTICES.md).
const DETECTION_URL: &str = "https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten";
const RECOGNITION_URL: &str =
    "https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.rten";

/// Cache directory for the downloaded OCR models.
fn models_dir() -> Result<PathBuf, String> {
    directories::ProjectDirs::from("com", "Havoc Software", "Freally Snipper")
        .map(|d| d.cache_dir().join("ocr-models"))
        .ok_or_else(|| "no cache directory available".to_owned())
}

/// Download `url` to `path` if it is not already cached (atomic via a temp file).
fn ensure_model(url: &str, path: &Path) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create cache dir: {e}"))?;
    }
    let mut response = ureq::get(url)
        .call()
        .map_err(|e| format!("download {url}: {e}"))?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(128 * 1024 * 1024) // models are ~6/12 MB; generous headroom
        .read_to_vec()
        .map_err(|e| format!("read {url}: {e}"))?;
    let tmp = path.with_extension("part");
    fs::write(&tmp, &bytes).map_err(|e| format!("write model: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| format!("finalize model: {e}"))?;
    Ok(())
}

/// Extract text from `image` (the whole raster). Downloads the models on first
/// use. **Blocking + slow** — call off the UI thread.
pub fn extract_text(image: &RgbaImage) -> Result<String, String> {
    let dir = models_dir()?;
    let detection = dir.join("text-detection.rten");
    let recognition = dir.join("text-recognition.rten");
    ensure_model(DETECTION_URL, &detection)?;
    ensure_model(RECOGNITION_URL, &recognition)?;

    let detection_model =
        rten::Model::load_file(&detection).map_err(|e| format!("load detection model: {e}"))?;
    let recognition_model =
        rten::Model::load_file(&recognition).map_err(|e| format!("load recognition model: {e}"))?;

    let engine = ocrs::OcrEngine::new(ocrs::OcrEngineParams {
        detection_model: Some(detection_model),
        recognition_model: Some(recognition_model),
        ..Default::default()
    })
    .map_err(|e| format!("init OCR engine: {e}"))?;

    let source = ocrs::ImageSource::from_bytes(image.as_raw(), (image.width(), image.height()))
        .map_err(|e| format!("read image: {e}"))?;
    let input = engine
        .prepare_input(source)
        .map_err(|e| format!("prepare OCR input: {e}"))?;
    engine
        .get_text(&input)
        .map_err(|e| format!("recognize text: {e}"))
}
