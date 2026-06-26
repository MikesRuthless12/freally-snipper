//! OCR / Extract-Text (P4.6b) — egui-free, pure-Rust.
//!
//! Uses **`ocrs` + `rten`** (MIT) — no Tesseract C++ system deps. The detection +
//! recognition models are **downloaded on demand** (see [`crate::models`]) and run
//! off the UI thread; the `on_progress` callback drives the download UI (P4.11).
//!
//! [`extract_text_auto_orient`] adds the P5 **auto-orient** polish: if the upright
//! pass reads poorly (sideways / upside-down text), it re-runs OCR at 90° / 180° /
//! 270° and keeps the most readable result — all local, no cloud.

use freally_capture::image::{imageops, RgbaImage};

use crate::download::Progress;
use crate::models;

/// Recognized word-characters above which the upright pass is trusted and the
/// rotations are skipped — the common, already-upright snip (so OCR runs once).
const CONFIDENT_SCORE: usize = 6;

/// Extract text from `image` (the whole raster), **auto-orienting** it: if the
/// upright pass reads poorly (sideways/upside-down text), retry at 90° / 180° /
/// 270° and keep the most readable result. The extra passes run **only** when the
/// upright read is weak, so a normal upright snip still costs a single OCR pass.
/// Downloads the models on first use, reporting progress via `on_progress`.
/// **Blocking + slow** — worker only.
pub fn extract_text(
    image: &RgbaImage,
    on_progress: impl FnMut(usize, Progress),
) -> Result<String, String> {
    let engine = build_engine(on_progress)?;

    let upright = run_ocr(&engine, image)?;
    let mut best_score = readability_score(&upright);
    if best_score >= CONFIDENT_SCORE {
        return Ok(upright); // already reads well — no rotation needed
    }

    // The selection may be rotated: try each turn and keep the best-scoring read.
    let mut best_text = upright;
    for angle in [90u32, 180, 270] {
        let rotated = match angle {
            90 => imageops::rotate90(image),
            180 => imageops::rotate180(image),
            _ => imageops::rotate270(image),
        };
        if let Ok(text) = run_ocr(&engine, &rotated) {
            let score = readability_score(&text);
            if score > best_score {
                best_score = score;
                best_text = text;
            }
        }
    }
    Ok(best_text)
}

/// Load the OCR models (downloading on first use) and build the engine.
fn build_engine(on_progress: impl FnMut(usize, Progress)) -> Result<ocrs::OcrEngine, String> {
    // `paths` is in `OCR.files` order: [detection, recognition].
    let paths = models::ensure(&models::OCR, on_progress)?;
    let detection_model =
        rten::Model::load_file(&paths[0]).map_err(|e| format!("load detection model: {e}"))?;
    let recognition_model =
        rten::Model::load_file(&paths[1]).map_err(|e| format!("load recognition model: {e}"))?;

    ocrs::OcrEngine::new(ocrs::OcrEngineParams {
        detection_model: Some(detection_model),
        recognition_model: Some(recognition_model),
        ..Default::default()
    })
    .map_err(|e| format!("init OCR engine: {e}"))
}

/// Run one OCR pass over `image` with an already-built `engine`.
fn run_ocr(engine: &ocrs::OcrEngine, image: &RgbaImage) -> Result<String, String> {
    // Flatten over opaque white (so transparent capture regions can't feed noise to
    // the recognizer) and hand ocrs 3-channel RGB — its documented input format.
    let (w, h) = (image.width(), image.height());
    let mut rgb = Vec::with_capacity(w as usize * h as usize * 3);
    for px in image.pixels() {
        let [r, g, b, a] = px.0;
        let a = a as u32;
        let over = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
        rgb.extend_from_slice(&[over(r), over(g), over(b)]);
    }
    let source =
        ocrs::ImageSource::from_bytes(&rgb, (w, h)).map_err(|e| format!("read image: {e}"))?;
    let input = engine
        .prepare_input(source)
        .map_err(|e| format!("prepare OCR input: {e}"))?;
    engine
        .get_text(&input)
        .map_err(|e| format!("recognize text: {e}"))
}

/// A crude readability score: the count of word-characters in tokens that are
/// mostly alphanumeric. Correctly-oriented text reads as many real word-characters;
/// a wrong orientation yields few characters / mostly symbol noise (scored 0), so a
/// higher score reliably picks the right turn.
fn readability_score(text: &str) -> usize {
    text.split_whitespace()
        .map(|word| {
            let total = word.chars().count();
            let alnum = word.chars().filter(|c| c.is_alphanumeric()).count();
            // Discount symbol-heavy garbage tokens; reward clean, longer words.
            if alnum * 2 >= total {
                alnum
            } else {
                0
            }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{self, FontFamily};
    use freally_capture::image::{Rgba, RgbaImage};

    #[test]
    fn readability_prefers_real_words_over_symbol_noise() {
        assert!(readability_score("Hello world 123") > readability_score("|~ ^^ <> #@"));
        assert_eq!(readability_score(""), 0);
        assert_eq!(readability_score("   \n  "), 0);
        // A single short word should still clear the symbol-noise floor.
        assert!(readability_score("Login") >= 5);
    }

    /// Diagnostic: confirm the OCR pipeline (models + rten + decode) reads clean,
    /// upright black-on-white text. Ignored by default — it needs the OCR models in
    /// the cache and runs real (slow in debug) inference. Run with:
    ///   cargo test -p freally-editor --release ocr -- --ignored --nocapture
    #[test]
    #[ignore = "needs downloaded OCR models in the cache; slow inference"]
    fn ocr_reads_clean_rendered_text() {
        let mut img = RgbaImage::from_pixel(720, 180, Rgba([255, 255, 255, 255]));
        let stamp = text::render("Hello OCR 12345", 72.0, FontFamily::Sans, [0, 0, 0, 255])
            .expect("render text");
        // Composite the black text stamp over the opaque white canvas (src-over).
        for y in 0..stamp.height().min(img.height() - 40) {
            for x in 0..stamp.width().min(img.width() - 30) {
                let s = stamp.get_pixel(x, y).0;
                let a = s[3] as u32;
                if a == 0 {
                    continue;
                }
                let d = img.get_pixel(x + 30, y + 40).0;
                let blend = |s: u8, d: u8| ((s as u32 * a + d as u32 * (255 - a)) / 255) as u8;
                img.put_pixel(
                    x + 30,
                    y + 40,
                    Rgba([blend(s[0], d[0]), blend(s[1], d[1]), blend(s[2], d[2]), 255]),
                );
            }
        }
        let out = extract_text(&img, |_, _| {}).expect("ocr run");
        eprintln!("OCR OUTPUT: {out:?}");
        assert!(
            out.to_lowercase().contains("hello"),
            "expected to read 'Hello', got: {out:?}"
        );
    }
}
