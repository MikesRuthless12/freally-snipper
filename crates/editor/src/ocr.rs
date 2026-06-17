//! OCR / Extract-Text (P4.6b) — egui-free, pure-Rust.
//!
//! Uses **`ocrs` + `rten`** (MIT) — no Tesseract C++ system deps. The detection +
//! recognition models are **downloaded on demand** (see [`crate::models`]) and run
//! off the UI thread; the `on_progress` callback drives the download UI (P4.11).

use freally_capture::image::RgbaImage;

use crate::download::Progress;
use crate::models;

/// Extract text from `image` (the whole raster). Downloads the models on first use,
/// reporting download progress via `on_progress`. **Blocking + slow** — worker only.
pub fn extract_text(
    image: &RgbaImage,
    on_progress: impl FnMut(usize, Progress),
) -> Result<String, String> {
    // `paths` is in `OCR.files` order: [detection, recognition].
    let paths = models::ensure(&models::OCR, on_progress)?;
    let detection_model =
        rten::Model::load_file(&paths[0]).map_err(|e| format!("load detection model: {e}"))?;
    let recognition_model =
        rten::Model::load_file(&paths[1]).map_err(|e| format!("load recognition model: {e}"))?;

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
