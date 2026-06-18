//! Local machine translation (P4.9) — egui-free, pure-Rust.
//!
//! **MADLAD-400** (Google, **Apache-2.0** — sell-commercial, no attribution string)
//! is a T5 model covering ~400 languages, any→any, in **one** model. We run it on
//! CPU via **candle** (`candle-transformers::models::t5`). The weights (~3 GB),
//! tokenizer and config **download on demand** to the cache — not bundled — so the
//! build stays light. MADLAD's convention: prefix the source with a target-language
//! token, e.g. `"<2es> Hello"` → Spanish.
//!
//! **Honest:** a 3 B model on CPU takes a few seconds per translation, so the editor
//! debounces (translate on a pause, not per keystroke) and runs it off the UI thread.
//! Translation *quality* is verified interactively (no GPU/model here to self-test).

use std::fs::File;

use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::t5;
use tokenizers::Tokenizer;

use crate::download::Progress;
use crate::models;

/// Cap generated tokens so a runaway decode can't hang.
const MAX_NEW_TOKENS: usize = 512;

/// A loaded MADLAD translator. Lives on the editor's translate worker thread.
pub struct Translator {
    model: t5::T5ForConditionalGeneration,
    tokenizer: Tokenizer,
    config: t5::Config,
    device: Device,
}

impl Translator {
    /// Download (if needed) + load the model, reporting download progress.
    /// **Blocking + slow** — worker thread only.
    pub fn load(on_progress: impl FnMut(usize, Progress)) -> Result<Translator, String> {
        // `paths` is in TRANSLATE.files order: [config.json, tokenizer.json, weights].
        let paths = models::ensure(&models::TRANSLATE, on_progress)?;

        let config: t5::Config = serde_json::from_reader(
            File::open(&paths[0]).map_err(|e| format!("open config: {e}"))?,
        )
        .map_err(|e| format!("parse config: {e}"))?;
        let tokenizer =
            Tokenizer::from_file(&paths[1]).map_err(|e| format!("load tokenizer: {e}"))?;

        let device = Device::Cpu;
        // SAFETY: mmap of a file we just wrote into our private cache; candle
        // requires unsafe for mmap. Pinned to an immutable revision (see models.rs).
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[paths[2].clone()], DType::F32, &device)
                .map_err(|e| format!("load weights: {e}"))?
        };
        let model = t5::T5ForConditionalGeneration::load(vb, &config)
            .map_err(|e| format!("build model: {e}"))?;
        Ok(Translator {
            model,
            tokenizer,
            config,
            device,
        })
    }

    /// Translate `text` into `target` (an ISO code like "es", "ja", "ar").
    pub fn translate(&mut self, text: &str, target: &str) -> Result<String, String> {
        let prompt = format!("<2{target}> {text}");
        let encoding = self
            .tokenizer
            .encode(prompt, true)
            .map_err(|e| format!("tokenize: {e}"))?;
        let input_ids = Tensor::new(encoding.get_ids(), &self.device)
            .and_then(|t| t.unsqueeze(0))
            .map_err(|e| format!("input tensor: {e}"))?;

        self.model.clear_kv_cache();
        let encoder_output = self
            .model
            .encode(&input_ids)
            .map_err(|e| format!("encode: {e}"))?;

        let start = self
            .config
            .decoder_start_token_id
            .unwrap_or(self.config.pad_token_id) as u32;
        let mut tokens: Vec<u32> = vec![start];
        let mut generated: Vec<u32> = Vec::new();
        let mut logits_processor = LogitsProcessor::new(0, None, None); // greedy

        for index in 0..MAX_NEW_TOKENS {
            // With the kv-cache, feed the whole prefix once, then only the last token.
            let decoder_input = if index == 0 || !self.config.use_cache {
                Tensor::new(tokens.as_slice(), &self.device).and_then(|t| t.unsqueeze(0))
            } else {
                Tensor::new(&[*tokens.last().unwrap()], &self.device).and_then(|t| t.unsqueeze(0))
            }
            .map_err(|e| format!("decoder tensor: {e}"))?;

            let logits = self
                .model
                .decode(&decoder_input, &encoder_output)
                .map_err(|e| format!("decode: {e}"))?;
            let logits = logits.squeeze(0).map_err(|e| format!("logits: {e}"))?;
            let next = logits_processor
                .sample(&logits)
                .map_err(|e| format!("sample: {e}"))?;
            if next as usize == self.config.eos_token_id {
                break;
            }
            tokens.push(next);
            generated.push(next);
        }

        let out = self
            .tokenizer
            .decode(&generated, true)
            .map_err(|e| format!("detokenize: {e}"))?;
        // MADLAD/SentencePiece can prefix the output with a space marker (▁, U+2581)
        // or a no-break space; normalize ▁→space, then trim all surrounding spaces
        // (`trim` covers U+00A0). So "Yes!"→Welsh yields "Ie", not "\u{00a0}Ie".
        let out = out.replace('\u{2581}', " ");
        Ok(out.trim().to_owned())
    }
}

/// Target languages offered in the editor's translate picker — English first, then
/// alphabetical by English name (the roadmap's reusable selector convention). The
/// code is MADLAD's `<2xx>` token; this is a common-language subset of MADLAD's
/// ~400, kept searchable via the autocomplete combo (the full set lands with the
/// shared language picker). Codes are ISO 639-1 where possible.
pub const TARGETS: &[(&str, &str)] = &[
    ("en", "English"),
    ("af", "Afrikaans"),
    ("sq", "Albanian"),
    ("am", "Amharic"),
    ("ar", "Arabic"),
    ("hy", "Armenian"),
    ("az", "Azerbaijani"),
    ("eu", "Basque"),
    ("bn", "Bengali"),
    ("bg", "Bulgarian"),
    ("my", "Burmese"),
    ("ca", "Catalan"),
    ("zh", "Chinese"),
    ("hr", "Croatian"),
    ("cs", "Czech"),
    ("da", "Danish"),
    ("nl", "Dutch"),
    ("et", "Estonian"),
    ("fi", "Finnish"),
    ("fr", "French"),
    ("gl", "Galician"),
    ("ka", "Georgian"),
    ("de", "German"),
    ("el", "Greek"),
    ("gu", "Gujarati"),
    ("he", "Hebrew"),
    ("hi", "Hindi"),
    ("hu", "Hungarian"),
    ("is", "Icelandic"),
    ("id", "Indonesian"),
    ("ga", "Irish"),
    ("it", "Italian"),
    ("ja", "Japanese"),
    ("kn", "Kannada"),
    ("kk", "Kazakh"),
    ("km", "Khmer"),
    ("ko", "Korean"),
    ("lo", "Lao"),
    ("lv", "Latvian"),
    ("lt", "Lithuanian"),
    ("mk", "Macedonian"),
    ("ms", "Malay"),
    ("ml", "Malayalam"),
    ("mt", "Maltese"),
    ("mr", "Marathi"),
    ("mn", "Mongolian"),
    ("ne", "Nepali"),
    ("no", "Norwegian"),
    ("fa", "Persian"),
    ("pl", "Polish"),
    ("pt", "Portuguese"),
    ("pa", "Punjabi"),
    ("ro", "Romanian"),
    ("ru", "Russian"),
    ("sr", "Serbian"),
    ("si", "Sinhala"),
    ("sk", "Slovak"),
    ("sl", "Slovenian"),
    ("so", "Somali"),
    ("es", "Spanish"),
    ("sw", "Swahili"),
    ("sv", "Swedish"),
    ("tl", "Tagalog"),
    ("ta", "Tamil"),
    ("te", "Telugu"),
    ("th", "Thai"),
    ("tr", "Turkish"),
    ("uk", "Ukrainian"),
    ("ur", "Urdu"),
    ("uz", "Uzbek"),
    ("vi", "Vietnamese"),
    ("cy", "Welsh"),
    ("yi", "Yiddish"),
    ("zu", "Zulu"),
];

/// English label for a target code (falls back to the code).
pub fn target_label(code: &str) -> &str {
    TARGETS
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, name)| *name)
        .unwrap_or(code)
}
