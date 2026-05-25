use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use futures_util::StreamExt;
use hound::{SampleFormat, WavSpec, WavWriter};
use ndarray::{Array, Array3};
use once_cell::sync::Lazy;
use ort::{session::Session, value::Value};
use rand::thread_rng;
use rand_distr::{Distribution, Normal};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufReader, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};
use unicode_normalization::UnicodeNormalization;

#[cfg(target_os = "linux")]
static ORT_ENV_INITIALIZED: Lazy<Result<bool, String>> = Lazy::new(|| {
    ort::init_from(onnxruntime_dylib_path())
        .commit()
        .map_err(|e| e.to_string())
});

#[cfg(target_os = "linux")]
fn onnxruntime_dylib_path() -> String {
    if let Ok(path) = std::env::var("ORT_DYLIB_PATH") {
        if !path.is_empty() {
            return path;
        }
    }

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let bundled_path = exe_dir.join("../lib/maple/libonnxruntime.so");
            if bundled_path.exists() {
                return bundled_path.to_string_lossy().into_owned();
            }
        }
    }

    "libonnxruntime.so".to_string()
}

// Pre-compiled regexes for text preprocessing (compiled once, reused)
static RE_BOLD: Lazy<Regex> = Lazy::new(|| Regex::new(r"\*\*([^*]+)\*\*").unwrap());
static RE_BOLD2: Lazy<Regex> = Lazy::new(|| Regex::new(r"__([^_]+)__").unwrap());
static RE_ITALIC: Lazy<Regex> = Lazy::new(|| Regex::new(r"\*([^*]+)\*").unwrap());
static RE_ITALIC2: Lazy<Regex> = Lazy::new(|| Regex::new(r"_([^_\s][^_]*)_").unwrap());
static RE_STRIKE: Lazy<Regex> = Lazy::new(|| Regex::new(r"~~([^~]+)~~").unwrap());
static RE_CODE: Lazy<Regex> = Lazy::new(|| Regex::new(r"`([^`]+)`").unwrap());
static RE_CODEBLOCK: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)```[^`]*```").unwrap());
static RE_HEADER: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^#{1,6}\s*").unwrap());
static RE_XML_TAG: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"</?[A-Za-z][A-Za-z0-9_-]*(?:\s+[^>]*)?>").unwrap());
static RE_EMOJI: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[\x{1F600}-\x{1F64F}\x{1F300}-\x{1F5FF}\x{1F680}-\x{1F6FF}\x{1F700}-\x{1F77F}\x{1F780}-\x{1F7FF}\x{1F800}-\x{1F8FF}\x{1F900}-\x{1F9FF}\x{1FA00}-\x{1FA6F}\x{1FA70}-\x{1FAFF}\x{2600}-\x{26FF}\x{2700}-\x{27BF}\x{1F1E6}-\x{1F1FF}]+").unwrap()
});
static RE_DIACRITICS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[\u{0302}\u{0303}\u{0304}\u{0305}\u{0306}\u{0307}\u{0308}\u{030A}\u{030B}\u{030C}\u{0327}\u{0328}\u{0329}\u{032A}\u{032B}\u{032C}\u{032D}\u{032E}\u{032F}]").unwrap()
});
static RE_SPACE_COMMA: Lazy<Regex> = Lazy::new(|| Regex::new(r" ,").unwrap());
static RE_SPACE_PERIOD: Lazy<Regex> = Lazy::new(|| Regex::new(r" \.").unwrap());
static RE_SPACE_EXCL: Lazy<Regex> = Lazy::new(|| Regex::new(r" !").unwrap());
static RE_SPACE_QUEST: Lazy<Regex> = Lazy::new(|| Regex::new(r" \?").unwrap());
static RE_SPACE_SEMI: Lazy<Regex> = Lazy::new(|| Regex::new(r" ;").unwrap());
static RE_SPACE_COLON: Lazy<Regex> = Lazy::new(|| Regex::new(r" :").unwrap());
static RE_SPACE_APOS: Lazy<Regex> = Lazy::new(|| Regex::new(r" '").unwrap());
static RE_DUP_DQUOTE: Lazy<Regex> = Lazy::new(|| Regex::new(r#""{2,}"#).unwrap());
static RE_DUP_SQUOTE: Lazy<Regex> = Lazy::new(|| Regex::new(r"'{2,}").unwrap());
static RE_DUP_BTICK: Lazy<Regex> = Lazy::new(|| Regex::new(r"`{2,}").unwrap());
static RE_MULTI_SPACE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());
static RE_ENDS_PUNCT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"[.!?;:,'"\u{201C}\u{201D}\u{2018}\u{2019})\]}…。」』】〉》›»]$"#).unwrap()
});
static RE_SENTENCE: Lazy<Regex> = Lazy::new(|| Regex::new(r"([.!?])\s+").unwrap());

// Pin model downloads to a specific repo revision to ensure integrity and reproducibility.
const HUGGINGFACE_REVISION: &str = "3cadd1ee6394adea1bd021217a0e650ede09a323";
const HUGGINGFACE_BASE_URL: &str = "https://huggingface.co/Supertone/supertonic-3/resolve";
const MODEL_REVISION_FILE: &str = "supertonic_revision.txt";
const SUPERTONIC3_CACHE_DIR: &str = "supertonic-3";
const DEFAULT_TTS_LANGUAGE: &str = "en";
const DEFAULT_VOICE_STYLE: &str = "F2.json";

const AVAILABLE_LANGS: &[&str] = &[
    "en", "ko", "ja", "ar", "bg", "cs", "da", "de", "el", "es", "et", "fi", "fr", "hi", "hr", "hu",
    "id", "it", "lt", "lv", "nl", "pl", "pt", "ro", "ru", "sk", "sl", "sv", "tr", "uk", "vi", "na",
];

// (file_name, url_path, expected_size_bytes, expected_sha256_hex)
const SUPERTONIC3_MODEL_FILES: &[(&str, &str, u64, &str)] = &[
    (
        "duration_predictor.onnx",
        "onnx/duration_predictor.onnx",
        3_700_147,
        "c3eb91414d5ff8a7a239b7fe9e34e7e2bf8a8140d8375ffb14718b1c639325db",
    ),
    (
        "text_encoder.onnx",
        "onnx/text_encoder.onnx",
        36_416_150,
        "c7befd5ea8c3119769e8a6c1486c4edc6a3bc8365c67621c881bbb774b9902ff",
    ),
    (
        "vector_estimator.onnx",
        "onnx/vector_estimator.onnx",
        256_534_781,
        "883ac868ea0275ef0e991524dc64f16b3c0376efd7c320af6b53f5b780d7c61c",
    ),
    (
        "vocoder.onnx",
        "onnx/vocoder.onnx",
        101_424_195,
        "085de76dd8e8d5836d6ca66826601f615939218f90e519f70ee8a36ed2a4c4ba",
    ),
    (
        "tts.json",
        "onnx/tts.json",
        8_253,
        "42078d3aef1cd43ab43021f3c54f47d2d75ceb4e75f627f118890128b06a0d09",
    ),
    (
        "unicode_indexer.json",
        "onnx/unicode_indexer.json",
        277_676,
        "9bf7346e43883a81f8645c81224f786d43c5b57f3641f6e7671a7d6c493cb24f",
    ),
    (
        "F1.json",
        "voice_styles/F1.json",
        292_046,
        "bbdec6ee00231c2c742ad05483df5334cab3b52fda3ba38e6a07059c4563dbc2",
    ),
    (
        "F2.json",
        "voice_styles/F2.json",
        292_423,
        "7c722c6a72707b1a77f035d67f0d1351ba187738e06f7683e8c72b1df3477fc6",
    ),
    (
        "F3.json",
        "voice_styles/F3.json",
        290_794,
        "12f6ef2573baa2defa1128069cb59f203e3ab67c92af77b42df8a0e3a2f7c6ab",
    ),
    (
        "F4.json",
        "voice_styles/F4.json",
        291_808,
        "c2fa764c1225a76dfc3e2c73e8aa4f70d9ee48793860eb34c295fff01c2e032b",
    ),
    (
        "F5.json",
        "voice_styles/F5.json",
        291_479,
        "45966e73316415626cf41a7d1c6f3b4c70dbc1ba2bee5c1978ef0ce33244fc8d",
    ),
    (
        "M1.json",
        "voice_styles/M1.json",
        291_748,
        "e35604687f5d23694b8e91593a93eec0e4eca6c0b02bb8ed69139ab2ea6b0a5b",
    ),
    (
        "M2.json",
        "voice_styles/M2.json",
        292_055,
        "b76cbf62bac707c710cf0ae5aba5e31eea1a6339a9734bfae33ab98499534a50",
    ),
    (
        "M3.json",
        "voice_styles/M3.json",
        290_198,
        "ea1ac35ccb91b0d7ecad533a2fbd0eec10c91513d8951e3b25fbba99954e159b",
    ),
    (
        "M4.json",
        "voice_styles/M4.json",
        291_522,
        "ca8eefad4fcd989c9379032ff3e50738adc547eeb5e221b82593a6d7b3bac303",
    ),
    (
        "M5.json",
        "voice_styles/M5.json",
        291_469,
        "dd22b92740314321f8ae11c5e87f8dd60d060f15dd3a632b5adf77f471f77af2",
    ),
];

const SUPERTONIC3_TOTAL_MODEL_SIZE: u64 = 401_276_744; // bytes

// Supertonic 1 assets were downloaded directly into the root tts_models
// directory. Keep detecting them so existing users can keep read-aloud working
// until they explicitly delete and download Supertonic 3.
const LEGACY_MODEL_FILES: &[(&str, u64)] = &[
    ("duration_predictor.onnx", 1_500_789),
    ("text_encoder.onnx", 27_348_373),
    ("vector_estimator.onnx", 132_471_364),
    ("vocoder.onnx", 101_405_066),
    ("tts.json", 8_645),
    ("unicode_indexer.json", 262_134),
    ("F1.json", 420_622),
    ("F2.json", 420_905),
    ("M1.json", 421_053),
    ("M2.json", 421_027),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelVersion {
    Supertonic3,
    Legacy,
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub ae: AEConfig,
    pub ttl: TTLConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AEConfig {
    pub sample_rate: i32,
    pub base_chunk_size: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TTLConfig {
    pub chunk_compress_factor: i32,
    pub latent_dim: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceStyleData {
    pub style_ttl: StyleComponent,
    pub style_dp: StyleComponent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleComponent {
    pub data: Vec<Vec<Vec<f32>>>,
    pub dims: Vec<usize>,
    #[serde(rename = "type")]
    pub dtype: String,
}

#[derive(Clone)]
pub struct Style {
    pub ttl: Array3<f32>,
    pub dp: Array3<f32>,
}

struct UnicodeProcessor {
    indexer: Vec<i64>,
    unknown_token_id: i64,
}

impl UnicodeProcessor {
    fn new(indexer: Vec<i64>, unknown_token_id: i64) -> Self {
        UnicodeProcessor {
            indexer,
            unknown_token_id,
        }
    }

    fn call(&self, text_list: &[String]) -> (Vec<Vec<i64>>, Array3<f32>) {
        // Text should already be preprocessed before reaching here
        let text_ids_lengths: Vec<usize> = text_list.iter().map(|t| t.chars().count()).collect();
        let max_len = *text_ids_lengths.iter().max().unwrap_or(&0);

        let mut text_ids = Vec::new();
        for text in text_list {
            let mut row = vec![0i64; max_len];
            let unicode_vals: Vec<usize> = text.chars().map(|c| c as usize).collect();
            for (j, &val) in unicode_vals.iter().enumerate() {
                if val < self.indexer.len() {
                    row[j] = self.indexer[val];
                } else {
                    row[j] = self.unknown_token_id;
                }
            }
            text_ids.push(row);
        }

        let text_mask = length_to_mask(&text_ids_lengths, Some(max_len));
        (text_ids, text_mask)
    }
}

fn is_valid_lang(lang: &str) -> bool {
    AVAILABLE_LANGS.contains(&lang)
}

fn normalize_text_for_tts(text: &str) -> String {
    let mut text: String = text.nfkd().collect();

    // Remove markdown formatting (using pre-compiled regexes)
    text = RE_BOLD.replace_all(&text, "$1").to_string();
    text = RE_BOLD2.replace_all(&text, "$1").to_string();
    text = RE_ITALIC.replace_all(&text, "$1").to_string();
    text = RE_ITALIC2.replace_all(&text, "$1").to_string();
    text = RE_STRIKE.replace_all(&text, "$1").to_string();
    text = RE_CODE.replace_all(&text, "$1").to_string();
    text = RE_CODEBLOCK.replace_all(&text, "").to_string();
    text = RE_HEADER.replace_all(&text, "").to_string();
    text = RE_XML_TAG.replace_all(&text, " ").to_string();
    text = RE_EMOJI.replace_all(&text, "").to_string();

    // Replace various dashes and symbols
    let replacements = [
        ("–", "-"),
        ("‑", "-"),
        ("—", "-"),
        ("¯", " "),
        ("\u{201C}", "\""),
        ("\u{201D}", "\""),
        ("\u{2018}", "'"),
        ("\u{2019}", "'"),
        ("´", "'"),
        ("`", "'"),
        ("[", " "),
        ("]", " "),
        ("|", " "),
        ("/", " "),
        ("#", " "),
        ("→", " "),
        ("←", " "),
    ];
    for (from, to) in &replacements {
        text = text.replace(from, to);
    }

    text = RE_DIACRITICS.replace_all(&text, "").to_string();

    // Remove special symbols
    for symbol in &["♥", "☆", "♡", "©", "\\"] {
        text = text.replace(symbol, "");
    }

    // Replace known expressions
    text = text.replace("@", " at ");
    text = text.replace("e.g.,", "for example, ");
    text = text.replace("i.e.,", "that is, ");

    // Fix spacing around punctuation
    text = RE_SPACE_COMMA.replace_all(&text, ",").to_string();
    text = RE_SPACE_PERIOD.replace_all(&text, ".").to_string();
    text = RE_SPACE_EXCL.replace_all(&text, "!").to_string();
    text = RE_SPACE_QUEST.replace_all(&text, "?").to_string();
    text = RE_SPACE_SEMI.replace_all(&text, ";").to_string();
    text = RE_SPACE_COLON.replace_all(&text, ":").to_string();
    text = RE_SPACE_APOS.replace_all(&text, "'").to_string();

    // Remove duplicate quotes (single regex pass instead of while loop)
    text = RE_DUP_DQUOTE.replace_all(&text, "\"").to_string();
    text = RE_DUP_SQUOTE.replace_all(&text, "'").to_string();
    text = RE_DUP_BTICK.replace_all(&text, "`").to_string();

    // Remove extra spaces
    text = RE_MULTI_SPACE.replace_all(&text, " ").to_string();
    text = text.trim().to_string();

    // Add period if no ending punctuation
    if !text.is_empty() && !RE_ENDS_PUNCT.is_match(&text) {
        text.push('.');
    }
    text
}

fn preprocess_text(text: &str, lang: &str) -> Result<String> {
    let text = normalize_text_for_tts(text);
    if text.is_empty() {
        return Ok(text);
    }

    if !is_valid_lang(lang) {
        bail!("Invalid TTS language: {lang}. Available: {AVAILABLE_LANGS:?}");
    }

    Ok(format!("<{lang}>{text}</{lang}>"))
}

fn length_to_mask(lengths: &[usize], max_len: Option<usize>) -> Array3<f32> {
    let bsz = lengths.len();
    let max_len = max_len.unwrap_or_else(|| *lengths.iter().max().unwrap_or(&0));
    let mut mask = Array3::<f32>::zeros((bsz, 1, max_len));
    for (i, &len) in lengths.iter().enumerate() {
        for j in 0..len.min(max_len) {
            mask[[i, 0, j]] = 1.0;
        }
    }
    mask
}

fn sample_noisy_latent(
    duration: &[f32],
    sample_rate: i32,
    base_chunk_size: i32,
    chunk_compress: i32,
    latent_dim: i32,
) -> (Array3<f32>, Array3<f32>) {
    let bsz = duration.len();
    let max_dur = duration.iter().fold(0.0f32, |a, &b| a.max(b));
    let wav_len_max = (max_dur * sample_rate as f32) as usize;
    let wav_lengths: Vec<usize> = duration
        .iter()
        .map(|&d| (d * sample_rate as f32) as usize)
        .collect();

    let chunk_size = (base_chunk_size * chunk_compress) as usize;
    let latent_len = wav_len_max.div_ceil(chunk_size);
    let latent_dim_val = (latent_dim * chunk_compress) as usize;

    let mut noisy_latent = Array3::<f32>::zeros((bsz, latent_dim_val, latent_len));
    let normal = Normal::new(0.0, 1.0).unwrap();
    let mut rng = thread_rng();

    for b in 0..bsz {
        for d in 0..latent_dim_val {
            for t in 0..latent_len {
                noisy_latent[[b, d, t]] = normal.sample(&mut rng);
            }
        }
    }

    let latent_lengths: Vec<usize> = wav_lengths
        .iter()
        .map(|&len| len.div_ceil(chunk_size))
        .collect();
    let latent_mask = length_to_mask(&latent_lengths, Some(latent_len));

    // Apply mask
    for b in 0..bsz {
        for d in 0..latent_dim_val {
            for t in 0..latent_len {
                noisy_latent[[b, d, t]] *= latent_mask[[b, 0, t]];
            }
        }
    }
    (noisy_latent, latent_mask)
}

/// Split text by words when it exceeds max_len
fn split_by_words(text: &str, max_len: usize) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        if current.len() + word.len() + 1 > max_len && !current.is_empty() {
            result.push(current.trim().to_string());
            current.clear();
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }

    if !current.is_empty() {
        result.push(current.trim().to_string());
    }
    result
}

fn chunk_text(text: &str, max_len: usize) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return vec![String::new()];
    }

    static RE_PARA: Lazy<Regex> = Lazy::new(|| Regex::new(r"\n\s*\n").unwrap());
    let paragraphs: Vec<&str> = RE_PARA.split(text).collect();
    let mut chunks = Vec::new();

    for para in paragraphs {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }

        if para.len() <= max_len {
            chunks.push(para.to_string());
            continue;
        }

        // Split by sentence boundaries, keeping punctuation
        let mut current = String::new();
        let mut last_end = 0;

        for m in RE_SENTENCE.find_iter(para) {
            let sentence = para[last_end..m.start() + 1].trim(); // +1 to include punctuation
            last_end = m.end();

            if sentence.is_empty() {
                continue;
            }

            // If single sentence exceeds max_len, split by words
            if sentence.len() > max_len {
                if !current.is_empty() {
                    chunks.push(current.trim().to_string());
                    current.clear();
                }
                chunks.extend(split_by_words(sentence, max_len));
                continue;
            }

            if current.len() + sentence.len() + 1 > max_len && !current.is_empty() {
                chunks.push(current.trim().to_string());
                current.clear();
            }

            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(sentence);
        }

        // Remaining text after last sentence boundary
        let remaining = para[last_end..].trim();
        if !remaining.is_empty() {
            // If remaining exceeds max_len, split by words
            if remaining.len() > max_len {
                if !current.is_empty() {
                    chunks.push(current.trim().to_string());
                }
                chunks.extend(split_by_words(remaining, max_len));
            } else if current.len() + remaining.len() + 1 > max_len && !current.is_empty() {
                chunks.push(current.trim().to_string());
                chunks.push(remaining.to_string());
            } else {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(remaining);
                chunks.push(current.trim().to_string());
            }
        } else if !current.is_empty() {
            chunks.push(current.trim().to_string());
        }
    }

    if chunks.is_empty() {
        vec![String::new()]
    } else {
        chunks
    }
}

pub struct TTSState {
    tts: Option<TextToSpeech>,
    style: Option<Style>,
    model_version: Option<ModelVersion>,
}

impl TTSState {
    pub fn new() -> Mutex<Self> {
        Mutex::new(TTSState {
            tts: None,
            style: None,
            model_version: None,
        })
    }
}

struct TextToSpeech {
    cfgs: Config,
    text_processor: UnicodeProcessor,
    dp_ort: Session,
    text_enc_ort: Session,
    vector_est_ort: Session,
    vocoder_ort: Session,
    sample_rate: i32,
    model_version: ModelVersion,
}

impl TextToSpeech {
    fn synthesize(
        &mut self,
        text: &str,
        style: &Style,
        total_step: usize,
        speed: f32,
    ) -> Result<Vec<f32>> {
        let chunks = chunk_text(text, 300);
        let mut wav_cat: Vec<f32> = Vec::new();
        let silence_duration = 0.05;

        for (i, chunk) in chunks.iter().enumerate() {
            if chunk.is_empty() {
                continue;
            }

            let processed_chunk = match self.model_version {
                ModelVersion::Supertonic3 => preprocess_text(chunk, DEFAULT_TTS_LANGUAGE)?,
                ModelVersion::Legacy => normalize_text_for_tts(chunk),
            };
            if processed_chunk.trim().is_empty() {
                continue;
            }

            let (wav, duration) = self.infer(&[processed_chunk], style, total_step, speed)?;
            let dur = duration[0];
            let wav_len = (self.sample_rate as f32 * dur) as usize;
            let wav_chunk = &wav[..wav_len.min(wav.len())];

            if i > 0 {
                let silence_len = (silence_duration * self.sample_rate as f32) as usize;
                wav_cat.extend(vec![0.0f32; silence_len]);
            }
            wav_cat.extend_from_slice(wav_chunk);
        }
        Ok(wav_cat)
    }

    fn infer(
        &mut self,
        text_list: &[String],
        style: &Style,
        total_step: usize,
        speed: f32,
    ) -> Result<(Vec<f32>, Vec<f32>)> {
        let bsz = text_list.len();
        let (text_ids, text_mask) = self.text_processor.call(text_list);

        if text_ids.is_empty() || text_ids[0].is_empty() {
            return Err(anyhow::anyhow!("Empty text input"));
        }

        let text_ids_array = {
            let text_ids_shape = (bsz, text_ids[0].len());
            let flat: Vec<i64> = text_ids.into_iter().flatten().collect();
            Array::from_shape_vec(text_ids_shape, flat)?
        };

        let text_ids_value = Value::from_array(text_ids_array)?;
        let text_mask_value = Value::from_array(text_mask)?;
        let style_dp_value = Value::from_array(style.dp.clone())?;

        // Predict duration
        let dp_outputs = self.dp_ort.run(ort::inputs! {
            "text_ids" => &text_ids_value,
            "style_dp" => &style_dp_value,
            "text_mask" => &text_mask_value
        })?;

        let (_, duration_data) = dp_outputs["duration"].try_extract_tensor::<f32>()?;
        let mut duration: Vec<f32> = duration_data.to_vec();
        for dur in duration.iter_mut() {
            *dur /= speed;
        }

        // Encode text
        let style_ttl_value = Value::from_array(style.ttl.clone())?;
        let text_enc_outputs = self.text_enc_ort.run(ort::inputs! {
            "text_ids" => &text_ids_value,
            "style_ttl" => &style_ttl_value,
            "text_mask" => &text_mask_value
        })?;

        let (text_emb_shape, text_emb_data) =
            text_enc_outputs["text_emb"].try_extract_tensor::<f32>()?;
        let text_emb = Array3::from_shape_vec(
            (
                text_emb_shape[0] as usize,
                text_emb_shape[1] as usize,
                text_emb_shape[2] as usize,
            ),
            text_emb_data.to_vec(),
        )?;
        let text_emb_value = Value::from_array(text_emb)?;

        // Sample noisy latent
        let (mut xt, latent_mask) = sample_noisy_latent(
            &duration,
            self.sample_rate,
            self.cfgs.ae.base_chunk_size,
            self.cfgs.ttl.chunk_compress_factor,
            self.cfgs.ttl.latent_dim,
        );
        let latent_mask_value = Value::from_array(latent_mask)?;

        let total_step_array = Array::from_elem(bsz, total_step as f32);
        let total_step_value = Value::from_array(total_step_array)?;

        // Denoising loop
        for step in 0..total_step {
            let current_step_array = Array::from_elem(bsz, step as f32);
            let xt_value = Value::from_array(xt)?;
            let current_step_value = Value::from_array(current_step_array)?;

            let vector_est_outputs = self.vector_est_ort.run(ort::inputs! {
                "noisy_latent" => &xt_value,
                "text_emb" => &text_emb_value,
                "style_ttl" => &style_ttl_value,
                "latent_mask" => &latent_mask_value,
                "text_mask" => &text_mask_value,
                "current_step" => &current_step_value,
                "total_step" => &total_step_value
            })?;

            let (denoised_shape, denoised_data) =
                vector_est_outputs["denoised_latent"].try_extract_tensor::<f32>()?;
            xt = Array3::from_shape_vec(
                (
                    denoised_shape[0] as usize,
                    denoised_shape[1] as usize,
                    denoised_shape[2] as usize,
                ),
                denoised_data.to_vec(),
            )?;
        }

        // Generate waveform
        let final_latent_value = Value::from_array(xt)?;
        let vocoder_outputs = self.vocoder_ort.run(ort::inputs! {
            "latent" => &final_latent_value
        })?;

        let (_, wav_data) = vocoder_outputs["wav_tts"].try_extract_tensor::<f32>()?;
        Ok((wav_data.to_vec(), duration))
    }
}

fn get_tts_models_root_dir() -> Result<PathBuf> {
    // On iOS, we need to use a different approach since dirs::data_local_dir() may not work
    #[cfg(target_os = "ios")]
    {
        // On iOS, store models under Library/Caches so they're not user-visible (Files app)
        // and won't be iCloud-backed.
        let home = std::env::var("HOME").context("Failed to get HOME directory on iOS")?;
        let data_dir = PathBuf::from(home)
            .join("Library")
            .join("Caches")
            .join("cloud.opensecret.maple")
            .join("tts_models");
        return Ok(data_dir);
    }

    #[cfg(not(target_os = "ios"))]
    {
        let data_dir = dirs::data_local_dir()
            .context("Failed to get local data directory")?
            .join("cloud.opensecret.maple")
            .join("tts_models");
        Ok(data_dir)
    }
}

fn get_supertonic3_models_dir() -> Result<PathBuf> {
    Ok(get_tts_models_root_dir()?.join(SUPERTONIC3_CACHE_DIR))
}

fn load_voice_style(models_dir: &Path) -> Result<Style> {
    // TODO: Add voice selection API. F2 is the closest match to the existing
    // bright/friendly female default.
    let style_path = models_dir.join(DEFAULT_VOICE_STYLE);
    let file = File::open(&style_path).context("Failed to open voice style file")?;
    let reader = BufReader::new(file);
    let data: VoiceStyleData = serde_json::from_reader(reader)?;

    let ttl_dims = &data.style_ttl.dims;
    let dp_dims = &data.style_dp.dims;

    let mut ttl_flat = Vec::new();
    for batch in &data.style_ttl.data {
        for row in batch {
            ttl_flat.extend(row);
        }
    }

    let mut dp_flat = Vec::new();
    for batch in &data.style_dp.data {
        for row in batch {
            dp_flat.extend(row);
        }
    }

    let ttl_style = Array3::from_shape_vec((1, ttl_dims[1], ttl_dims[2]), ttl_flat)?;
    let dp_style = Array3::from_shape_vec((1, dp_dims[1], dp_dims[2]), dp_flat)?;

    Ok(Style {
        ttl: ttl_style,
        dp: dp_style,
    })
}

fn load_tts_engine(models_dir: &Path, model_version: ModelVersion) -> Result<TextToSpeech> {
    #[cfg(target_os = "linux")]
    ORT_ENV_INITIALIZED
        .as_ref()
        .map_err(|e| anyhow::anyhow!(e.clone()))?;

    let cfg_path = models_dir.join("tts.json");
    let file = File::open(&cfg_path)?;
    let reader = BufReader::new(file);
    let cfgs: Config = serde_json::from_reader(reader)?;

    let indexer_path = models_dir.join("unicode_indexer.json");
    let file = File::open(&indexer_path)?;
    let reader = BufReader::new(file);
    let indexer: Vec<i64> = serde_json::from_reader(reader)?;
    let unknown_token_id = match model_version {
        ModelVersion::Supertonic3 => -1,
        ModelVersion::Legacy => 0,
    };
    let text_processor = UnicodeProcessor::new(indexer, unknown_token_id);

    let dp_ort =
        Session::builder()?.commit_from_file(models_dir.join("duration_predictor.onnx"))?;
    let text_enc_ort =
        Session::builder()?.commit_from_file(models_dir.join("text_encoder.onnx"))?;
    let vector_est_ort =
        Session::builder()?.commit_from_file(models_dir.join("vector_estimator.onnx"))?;
    let vocoder_ort = Session::builder()?.commit_from_file(models_dir.join("vocoder.onnx"))?;

    let sample_rate = cfgs.ae.sample_rate;
    Ok(TextToSpeech {
        cfgs,
        text_processor,
        dp_ort,
        text_enc_ort,
        vector_est_ort,
        vocoder_ort,
        sample_rate,
        model_version,
    })
}

fn wav_to_base64(audio_data: &[f32], sample_rate: i32) -> Result<String> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: sample_rate as u32,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };

    let mut buffer = Cursor::new(Vec::new());
    {
        let mut writer = WavWriter::new(&mut buffer, spec)?;
        for &sample in audio_data {
            let clamped = sample.clamp(-1.0, 1.0);
            let val = (clamped * 32767.0) as i16;
            writer.write_sample(val)?;
        }
        writer.finalize()?;
    }

    Ok(BASE64.encode(buffer.into_inner()))
}

fn model_revision_matches(models_dir: &Path) -> bool {
    fs::read_to_string(models_dir.join(MODEL_REVISION_FILE))
        .map(|revision| revision.trim() == HUGGINGFACE_REVISION)
        .unwrap_or(false)
}

fn models_dir_has_entries(models_dir: &Path) -> bool {
    fs::read_dir(models_dir)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false)
}

fn model_files_match(models_dir: &Path, files: &[(&str, u64)]) -> bool {
    files.iter().all(
        |(name, expected_size)| match fs::metadata(models_dir.join(name)) {
            Ok(meta) => meta.len() == *expected_size,
            Err(_) => false,
        },
    )
}

fn supertonic3_model_files_match(models_dir: &Path) -> bool {
    SUPERTONIC3_MODEL_FILES
        .iter()
        .all(
            |(name, _, expected_size, _)| match fs::metadata(models_dir.join(name)) {
                Ok(meta) => meta.len() == *expected_size,
                Err(_) => false,
            },
        )
}

fn installed_model_version() -> Result<Option<(ModelVersion, PathBuf)>> {
    let root_dir = get_tts_models_root_dir()?;
    let supertonic3_dir = root_dir.join(SUPERTONIC3_CACHE_DIR);

    if supertonic3_model_files_match(&supertonic3_dir) && model_revision_matches(&supertonic3_dir) {
        return Ok(Some((ModelVersion::Supertonic3, supertonic3_dir)));
    }

    if model_files_match(&root_dir, LEGACY_MODEL_FILES) {
        return Ok(Some((ModelVersion::Legacy, root_dir)));
    }

    Ok(None)
}

fn legacy_models_present() -> Result<bool> {
    let root_dir = get_tts_models_root_dir()?;
    Ok(model_files_match(&root_dir, LEGACY_MODEL_FILES))
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)
        .with_context(|| format!("Failed to open {} for checksum", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];

    loop {
        let bytes_read = file
            .read(&mut buffer)
            .with_context(|| format!("Failed to read {} for checksum", path.display()))?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(bytes_to_hex(hasher.finalize().as_ref()))
}

// ============================================================================
// Tauri Commands
// ============================================================================

#[derive(Serialize)]
pub struct TTSStatusResponse {
    pub models_downloaded: bool,
    pub models_loaded: bool,
    pub models_present_but_incompatible: bool,
    pub upgrade_available: bool,
    pub model_version: Option<ModelVersion>,
    pub total_size_mb: f64,
}

#[tauri::command]
pub async fn tts_get_status(
    state: tauri::State<'_, Mutex<TTSState>>,
) -> Result<TTSStatusResponse, String> {
    let installed = installed_model_version().map_err(|e| e.to_string())?;
    let models_downloaded = installed.is_some();
    let model_version = installed.map(|(version, _)| version);
    let upgrade_available = model_version == Some(ModelVersion::Legacy);

    let root_dir = get_tts_models_root_dir().map_err(|e| e.to_string())?;
    let supertonic3_dir = root_dir.join(SUPERTONIC3_CACHE_DIR);
    let models_present_but_incompatible = !models_downloaded
        && models_dir_has_entries(&root_dir)
        && !models_dir_has_entries(&supertonic3_dir);
    let models_loaded = {
        let guard = state.lock().map_err(|e| e.to_string())?;
        guard.tts.is_some() && guard.style.is_some() && guard.model_version.is_some()
    };

    Ok(TTSStatusResponse {
        models_downloaded,
        models_loaded,
        models_present_but_incompatible,
        upgrade_available,
        model_version,
        total_size_mb: SUPERTONIC3_TOTAL_MODEL_SIZE as f64 / 1024.0 / 1024.0,
    })
}

#[derive(Clone, Serialize)]
struct DownloadProgress {
    downloaded: u64,
    total: u64,
    file_name: String,
    percent: f64,
}

#[tauri::command]
pub async fn tts_download_models(app: AppHandle) -> Result<(), String> {
    use std::time::Duration;

    if legacy_models_present().map_err(|e| e.to_string())? {
        return Err(
            "Supertonic 1 local TTS models are installed. Delete them before downloading Supertonic 3."
                .to_string(),
        );
    }

    let models_dir = get_supertonic3_models_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&models_dir)
        .map_err(|e| format!("Failed to create models directory: {e}"))?;

    fs::write(
        models_dir.join(MODEL_REVISION_FILE),
        format!("{HUGGINGFACE_REVISION}\n"),
    )
    .map_err(|e| format!("Failed to write TTS model revision marker: {e}"))?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;
    let mut total_downloaded: u64 = 0;

    for (file_name, url_path, expected_size, expected_sha256) in SUPERTONIC3_MODEL_FILES {
        let file_path = models_dir.join(file_name);
        let temp_path = models_dir.join(format!("{file_name}.part"));

        // Skip only if the existing file matches both size and checksum.
        if file_path.exists() {
            if let Ok(meta) = fs::metadata(&file_path) {
                if meta.len() == *expected_size {
                    match sha256_file(&file_path) {
                        Ok(actual_sha256) if actual_sha256 == *expected_sha256 => {
                            total_downloaded += expected_size;
                            let _ = app.emit(
                                "tts-download-progress",
                                DownloadProgress {
                                    downloaded: total_downloaded,
                                    total: SUPERTONIC3_TOTAL_MODEL_SIZE,
                                    file_name: file_name.to_string(),
                                    percent: (total_downloaded as f64
                                        / SUPERTONIC3_TOTAL_MODEL_SIZE as f64)
                                        * 100.0,
                                },
                            );
                            continue;
                        }
                        Ok(actual_sha256) => {
                            log::warn!(
                                "Existing TTS model checksum mismatch for {file_name}: expected {expected_sha256}, got {actual_sha256}; re-downloading"
                            );
                        }
                        Err(err) => {
                            log::warn!(
                                "Failed to verify existing TTS model {file_name}: {err}; re-downloading"
                            );
                        }
                    }
                }
            }

            // Wrong-size, corrupted, or unreadable file: treat as invalid and re-download.
            let _ = fs::remove_file(&file_path);
        }

        // Clean up any partial download from previous attempt
        let _ = fs::remove_file(&temp_path);

        let url = format!("{HUGGINGFACE_BASE_URL}/{HUGGINGFACE_REVISION}/{url_path}");
        log::info!("Downloading TTS model: {file_name}");

        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to download {file_name}: {e}"))?;

        if !response.status().is_success() {
            return Err(format!(
                "Failed to download {}: HTTP {}",
                file_name,
                response.status()
            ));
        }

        let expected_len = response.content_length();
        let mut hasher = Sha256::new();

        let mut file = File::create(&temp_path)
            .map_err(|e| format!("Failed to create file {file_name}: {e}"))?;

        let mut stream = response.bytes_stream();
        let mut file_downloaded: u64 = 0;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("Download error: {e}"))?;
            file.write_all(&chunk)
                .map_err(|e| format!("Write error: {e}"))?;

            hasher.update(&chunk);

            file_downloaded += chunk.len() as u64;
            let current_total = total_downloaded + file_downloaded;

            let _ = app.emit(
                "tts-download-progress",
                DownloadProgress {
                    downloaded: current_total,
                    total: SUPERTONIC3_TOTAL_MODEL_SIZE,
                    file_name: file_name.to_string(),
                    percent: (current_total as f64 / SUPERTONIC3_TOTAL_MODEL_SIZE as f64) * 100.0,
                },
            );
        }

        if let Some(expected_len) = expected_len {
            if file_downloaded != expected_len {
                drop(file);
                let _ = fs::remove_file(&temp_path);
                return Err(format!(
                    "Incomplete download for {file_name}: expected {expected_len} bytes, got {file_downloaded}"
                ));
            }
        }

        if file_downloaded != *expected_size {
            drop(file);
            let _ = fs::remove_file(&temp_path);
            return Err(format!(
                "Unexpected download size for {file_name}: expected {expected_size} bytes, got {file_downloaded}"
            ));
        }

        let actual_sha256 = bytes_to_hex(hasher.finalize().as_ref());
        if actual_sha256 != *expected_sha256 {
            drop(file);
            let _ = fs::remove_file(&temp_path);
            return Err(format!(
                "Checksum mismatch for {file_name}: expected {expected_sha256}, got {actual_sha256}"
            ));
        }

        // Flush and rename temp file to final path
        file.flush()
            .map_err(|e| format!("Failed to flush file {file_name}: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("Failed to sync file {file_name}: {e}"))?;
        drop(file);
        fs::rename(&temp_path, &file_path)
            .map_err(|e| format!("Failed to finalize {file_name}: {e}"))?;

        total_downloaded += expected_size;
        log::info!("Downloaded TTS model: {file_name}");
    }

    fs::write(
        models_dir.join(MODEL_REVISION_FILE),
        format!("{HUGGINGFACE_REVISION}\n"),
    )
    .map_err(|e| format!("Failed to write TTS model revision marker: {e}"))?;

    Ok(())
}

#[tauri::command]
pub async fn tts_load_models(state: tauri::State<'_, Mutex<TTSState>>) -> Result<(), String> {
    let (model_version, models_dir) = installed_model_version()
        .map_err(|e| e.to_string())?
        .ok_or("TTS models are not downloaded")?;

    log::info!("Loading {model_version:?} TTS models from {models_dir:?}");

    let tts = load_tts_engine(&models_dir, model_version)
        .map_err(|e| format!("Failed to load TTS engine: {e}"))?;
    let style =
        load_voice_style(&models_dir).map_err(|e| format!("Failed to load voice style: {e}"))?;

    {
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        guard.tts = Some(tts);
        guard.style = Some(style);
        guard.model_version = Some(model_version);
    }

    log::info!("TTS models loaded successfully");
    Ok(())
}

#[derive(Serialize)]
pub struct TTSSynthesizeResponse {
    pub audio_base64: String,
    pub sample_rate: i32,
    pub duration_seconds: f32,
}

#[tauri::command]
pub async fn tts_synthesize(
    text: String,
    state: tauri::State<'_, Mutex<TTSState>>,
) -> Result<TTSSynthesizeResponse, String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;

    // Clone the style to avoid borrow conflicts
    let style = guard
        .style
        .as_ref()
        .ok_or("Voice style not loaded")?
        .clone();
    let tts = guard.tts.as_mut().ok_or("TTS engine not loaded")?;

    if text.trim().is_empty() {
        return Err("No text to synthesize".to_string());
    }

    let audio = tts
        .synthesize(&text, &style, 10, 1.2)
        .map_err(|e| format!("TTS synthesis failed: {e}"))?;

    if audio.is_empty() {
        return Err("No speakable text after preprocessing".to_string());
    }

    let sample_rate = tts.sample_rate;
    let duration_seconds = audio.len() as f32 / sample_rate as f32;

    // Drop the guard before encoding to release the lock
    drop(guard);

    let audio_base64 =
        wav_to_base64(&audio, sample_rate).map_err(|e| format!("Failed to encode audio: {e}"))?;

    log::info!("TTS synthesis complete: {duration_seconds:.2}s audio");

    Ok(TTSSynthesizeResponse {
        audio_base64,
        sample_rate,
        duration_seconds,
    })
}

#[tauri::command]
pub async fn tts_unload_models(state: tauri::State<'_, Mutex<TTSState>>) -> Result<(), String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    guard.tts = None;
    guard.style = None;
    guard.model_version = None;
    log::info!("TTS models unloaded");
    Ok(())
}

#[tauri::command]
pub async fn tts_delete_models(state: tauri::State<'_, Mutex<TTSState>>) -> Result<(), String> {
    // First unload models from memory
    {
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        guard.tts = None;
        guard.style = None;
        guard.model_version = None;
    }

    // Delete the models directory
    let root_dir = get_tts_models_root_dir().map_err(|e| e.to_string())?;
    if root_dir.exists() {
        fs::remove_dir_all(&root_dir).map_err(|e| format!("Failed to delete TTS models: {e}"))?;
    }

    log::info!("TTS models deleted");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_to_hex_is_lowercase_and_zero_padded() {
        assert_eq!(bytes_to_hex(&[0x00, 0xab, 0xff]), "00abff");
    }

    #[test]
    fn preprocess_text_strips_markdown_and_emoji_and_adds_period() {
        assert_eq!(
            normalize_text_for_tts("**Hello** _world_ 😊"),
            "Hello world."
        );
    }

    #[test]
    fn preprocess_text_does_not_add_punctuation_if_already_present() {
        assert_eq!(normalize_text_for_tts("Hi!"), "Hi!");
    }

    #[test]
    fn preprocess_text_wraps_with_language_tags_for_supertonic3() {
        assert_eq!(
            preprocess_text("Hello", "en").unwrap(),
            "<en>Hello.</en>".to_string()
        );
    }

    #[test]
    fn normalize_text_strips_expression_tags_from_user_text() {
        assert_eq!(
            normalize_text_for_tts("Hello <laugh> world"),
            "Hello world."
        );
    }

    #[test]
    #[ignore = "requires downloaded Supertonic 3 model files"]
    fn supertonic3_smoke_synthesizes_from_model_dir() {
        let models_dir = std::env::var("MAPLE_SUPERTONIC3_SMOKE_DIR")
            .expect("MAPLE_SUPERTONIC3_SMOKE_DIR must point at Supertonic 3 model files");
        let models_dir = PathBuf::from(models_dir);

        let mut tts = load_tts_engine(&models_dir, ModelVersion::Supertonic3).unwrap();
        let style = load_voice_style(&models_dir).unwrap();
        let audio = tts.synthesize("Hello from Maple.", &style, 2, 1.2).unwrap();

        assert!(!audio.is_empty());
    }

    #[test]
    fn chunk_text_splits_long_sentence_by_words_when_needed() {
        let chunks = chunk_text("Hello world. Bye.", 10);
        assert_eq!(
            chunks,
            vec![
                "Hello".to_string(),
                "world.".to_string(),
                "Bye.".to_string()
            ]
        );
    }

    #[test]
    fn chunk_text_returns_single_empty_chunk_for_empty_input() {
        assert_eq!(chunk_text("   ", 10), vec![String::new()]);
    }
}
