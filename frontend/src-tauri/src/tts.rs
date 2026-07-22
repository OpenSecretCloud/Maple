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
use std::time::Instant;
use tauri::{AppHandle, Emitter};
use unicode_normalization::UnicodeNormalization;

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
static RE_LEGACY_DIACRITICS: Lazy<Regex> = Lazy::new(|| {
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

// Model downloads are pinned to an immutable Hugging Face revision. Keep the
// sizes and hashes in sync with that revision so partial or substituted files
// never become loadable sessions.
const HUGGINGFACE_REVISION: &str = "3cadd1ee6394adea1bd021217a0e650ede09a323";
const HUGGINGFACE_BASE_URL: &str = "https://huggingface.co/Supertone/supertonic-3/resolve";
const MODEL_REVISION_FILE: &str = "supertonic_revision.txt";
const SUPERTONIC3_CACHE_DIR: &str = "supertonic-3";
const TTS_MODELS_DIR_ENV: &str = "MAPLE_TTS_MODELS_DIR";
const DEFAULT_LANGUAGE: &str = "na";
const DEFAULT_VOICE_STYLE: &str = "F2.json";
const SUPERTONIC3_TOTAL_STEPS: usize = 8;
const LEGACY_TOTAL_STEPS: usize = 10;
const DEFAULT_CHUNK_CHARS: usize = 300;
const CJK_CHUNK_CHARS: usize = 120;
const SUPERTONIC3_DEFAULT_SPEED: f32 = 1.2;
const LEGACY_DEFAULT_SPEED: f32 = 1.2;
const MIN_TTS_SPEED: f32 = 0.5;
const MAX_TTS_SPEED: f32 = 2.0;

const AVAILABLE_LANGUAGES: &[&str] = &[
    "en", "ko", "ja", "ar", "bg", "cs", "da", "de", "el", "es", "et", "fi", "fr", "hi", "hr", "hu",
    "id", "it", "lt", "lv", "nl", "pl", "pt", "ro", "ru", "sk", "sl", "sv", "tr", "uk", "vi", "na",
];

#[derive(Clone, Copy, Debug)]
struct ModelFile {
    name: &'static str,
    url_path: &'static str,
    size: u64,
    sha256: &'static str,
}

const SUPERTONIC3_MODEL_FILES: &[ModelFile] = &[
    ModelFile {
        name: "duration_predictor.onnx",
        url_path: "onnx/duration_predictor.onnx",
        size: 3_700_147,
        sha256: "c3eb91414d5ff8a7a239b7fe9e34e7e2bf8a8140d8375ffb14718b1c639325db",
    },
    ModelFile {
        name: "text_encoder.onnx",
        url_path: "onnx/text_encoder.onnx",
        size: 36_416_150,
        sha256: "c7befd5ea8c3119769e8a6c1486c4edc6a3bc8365c67621c881bbb774b9902ff",
    },
    ModelFile {
        name: "vector_estimator.onnx",
        url_path: "onnx/vector_estimator.onnx",
        size: 256_534_781,
        sha256: "883ac868ea0275ef0e991524dc64f16b3c0376efd7c320af6b53f5b780d7c61c",
    },
    ModelFile {
        name: "vocoder.onnx",
        url_path: "onnx/vocoder.onnx",
        size: 101_424_195,
        sha256: "085de76dd8e8d5836d6ca66826601f615939218f90e519f70ee8a36ed2a4c4ba",
    },
    ModelFile {
        name: "tts.json",
        url_path: "onnx/tts.json",
        size: 8_253,
        sha256: "42078d3aef1cd43ab43021f3c54f47d2d75ceb4e75f627f118890128b06a0d09",
    },
    ModelFile {
        name: "unicode_indexer.json",
        url_path: "onnx/unicode_indexer.json",
        size: 277_676,
        sha256: "9bf7346e43883a81f8645c81224f786d43c5b57f3641f6e7671a7d6c493cb24f",
    },
    ModelFile {
        name: "F1.json",
        url_path: "voice_styles/F1.json",
        size: 292_046,
        sha256: "bbdec6ee00231c2c742ad05483df5334cab3b52fda3ba38e6a07059c4563dbc2",
    },
    ModelFile {
        name: "F2.json",
        url_path: "voice_styles/F2.json",
        size: 292_423,
        sha256: "7c722c6a72707b1a77f035d67f0d1351ba187738e06f7683e8c72b1df3477fc6",
    },
    ModelFile {
        name: "F3.json",
        url_path: "voice_styles/F3.json",
        size: 290_794,
        sha256: "12f6ef2573baa2defa1128069cb59f203e3ab67c92af77b42df8a0e3a2f7c6ab",
    },
    ModelFile {
        name: "F4.json",
        url_path: "voice_styles/F4.json",
        size: 291_808,
        sha256: "c2fa764c1225a76dfc3e2c73e8aa4f70d9ee48793860eb34c295fff01c2e032b",
    },
    ModelFile {
        name: "F5.json",
        url_path: "voice_styles/F5.json",
        size: 291_479,
        sha256: "45966e73316415626cf41a7d1c6f3b4c70dbc1ba2bee5c1978ef0ce33244fc8d",
    },
    ModelFile {
        name: "M1.json",
        url_path: "voice_styles/M1.json",
        size: 291_748,
        sha256: "e35604687f5d23694b8e91593a93eec0e4eca6c0b02bb8ed69139ab2ea6b0a5b",
    },
    ModelFile {
        name: "M2.json",
        url_path: "voice_styles/M2.json",
        size: 292_055,
        sha256: "b76cbf62bac707c710cf0ae5aba5e31eea1a6339a9734bfae33ab98499534a50",
    },
    ModelFile {
        name: "M3.json",
        url_path: "voice_styles/M3.json",
        size: 290_198,
        sha256: "ea1ac35ccb91b0d7ecad533a2fbd0eec10c91513d8951e3b25fbba99954e159b",
    },
    ModelFile {
        name: "M4.json",
        url_path: "voice_styles/M4.json",
        size: 291_522,
        sha256: "ca8eefad4fcd989c9379032ff3e50738adc547eeb5e221b82593a6d7b3bac303",
    },
    ModelFile {
        name: "M5.json",
        url_path: "voice_styles/M5.json",
        size: 291_469,
        sha256: "dd22b92740314321f8ae11c5e87f8dd60d060f15dd3a632b5adf77f471f77af2",
    },
];

const SUPERTONIC3_TOTAL_MODEL_SIZE: u64 = 401_276_744;

// Supertonic 1 stored its assets directly in the root directory. Existing
// users can keep using those models until they explicitly choose to upgrade.
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

fn default_tts_speed(model_version: ModelVersion) -> f32 {
    match model_version {
        ModelVersion::Supertonic3 => SUPERTONIC3_DEFAULT_SPEED,
        ModelVersion::Legacy => LEGACY_DEFAULT_SPEED,
    }
}

fn resolve_tts_speed(model_version: ModelVersion, requested: Option<f32>) -> f32 {
    match requested {
        Some(speed) if speed.is_finite() && speed > 0.0 => {
            speed.clamp(MIN_TTS_SPEED, MAX_TTS_SPEED)
        }
        _ => default_tts_speed(model_version),
    }
}

fn total_steps_for_model(model_version: ModelVersion) -> usize {
    match model_version {
        ModelVersion::Supertonic3 => SUPERTONIC3_TOTAL_STEPS,
        ModelVersion::Legacy => LEGACY_TOTAL_STEPS,
    }
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

fn normalize_text_for_tts(text: &str, strip_legacy_diacritics: bool) -> String {
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
    // Expression tags are deliberately not exposed in Maple's first
    // Supertonic 3 iteration. Strip arbitrary XML-like input before adding the
    // trusted language wrapper below.
    text = RE_XML_TAG.replace_all(&text, " ").to_string();
    text = RE_EMOJI.replace_all(&text, "").to_string();

    // Replace various dashes and symbols
    let replacements = [
        ("–", "-"),
        ("‑", "-"),
        ("—", "-"),
        ("¯", " "),
        ("_", " "),
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

    if strip_legacy_diacritics {
        text = RE_LEGACY_DIACRITICS.replace_all(&text, "").to_string();
    }

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

fn normalize_language(language: Option<&str>) -> Result<&str> {
    let language = language.unwrap_or(DEFAULT_LANGUAGE).trim();
    if AVAILABLE_LANGUAGES.contains(&language) {
        Ok(language)
    } else {
        bail!("Invalid TTS language: {language}. Available languages: {AVAILABLE_LANGUAGES:?}")
    }
}

fn prepare_text(
    text: &str,
    model_version: ModelVersion,
    language: Option<&str>,
) -> Result<Option<String>> {
    let normalized = normalize_text_for_tts(text, model_version == ModelVersion::Legacy);
    if normalized.is_empty() {
        return Ok(None);
    }

    if model_version == ModelVersion::Legacy {
        return Ok(Some(normalized));
    }

    let language = normalize_language(language)?;
    Ok(Some(format!("<{language}>{normalized}</{language}>")))
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

fn hard_split_chars(text: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        if current.chars().count() == max_chars {
            chunks.push(std::mem::take(&mut current));
        }
        current.push(character);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn split_by_words(text: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        let word_chars = word.chars().count();
        if word_chars > max_chars {
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
            chunks.extend(hard_split_chars(word, max_chars));
            continue;
        }

        let separator = usize::from(!current.is_empty());
        if current.chars().count() + separator + word_chars > max_chars {
            chunks.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }

    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn push_chunk_unit(chunks: &mut Vec<String>, current: &mut String, unit: &str, max_chars: usize) {
    let unit = unit.trim();
    if unit.is_empty() {
        return;
    }

    if unit.chars().count() > max_chars {
        if !current.is_empty() {
            chunks.push(std::mem::take(current));
        }
        chunks.extend(split_by_words(unit, max_chars));
        return;
    }

    let separator = usize::from(!current.is_empty());
    if current.chars().count() + separator + unit.chars().count() > max_chars {
        chunks.push(std::mem::take(current));
    }
    if !current.is_empty() {
        current.push(' ');
    }
    current.push_str(unit);
}

fn chunk_text(text: &str, max_chars: usize) -> Vec<String> {
    if max_chars == 0 {
        return Vec::new();
    }
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }

    static RE_PARA: Lazy<Regex> = Lazy::new(|| Regex::new(r"\n\s*\n").unwrap());
    let paragraphs: Vec<&str> = RE_PARA.split(text).collect();
    let mut chunks = Vec::new();
    let mut current = String::new();

    for para in paragraphs {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }

        if para.chars().count() <= max_chars {
            push_chunk_unit(&mut chunks, &mut current, para, max_chars);
            continue;
        }

        let mut last_end = 0;

        for m in RE_SENTENCE.find_iter(para) {
            let sentence = para[last_end..m.start() + 1].trim();
            last_end = m.end();
            push_chunk_unit(&mut chunks, &mut current, sentence, max_chars);
        }

        let remaining = para[last_end..].trim();
        push_chunk_unit(&mut chunks, &mut current, remaining, max_chars);
    }

    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn chunk_size_for_language(language: &str) -> usize {
    if matches!(language, "ko" | "ja") {
        CJK_CHUNK_CHARS
    } else {
        DEFAULT_CHUNK_CHARS
    }
}

pub struct TTSState {
    loaded: Option<LoadedTts>,
}

impl TTSState {
    pub fn new() -> Mutex<Self> {
        Mutex::new(TTSState { loaded: None })
    }
}

struct LoadedTts {
    engine: TextToSpeech,
    style: Style,
    version: ModelVersion,
}

struct TextToSpeech {
    cfgs: Config,
    text_processor: UnicodeProcessor,
    dp_ort: Session,
    text_enc_ort: Session,
    vector_est_ort: Session,
    vocoder_ort: Session,
    sample_rate: i32,
}

impl TextToSpeech {
    fn synthesize(
        &mut self,
        text: &str,
        style: &Style,
        model_version: ModelVersion,
        language: Option<&str>,
        total_step: usize,
        speed: f32,
    ) -> Result<Vec<f32>> {
        let language = if model_version == ModelVersion::Supertonic3 {
            normalize_language(language)?
        } else {
            DEFAULT_LANGUAGE
        };
        let chunks = chunk_text(text, chunk_size_for_language(language));
        let mut wav_cat: Vec<f32> = Vec::new();
        let silence_duration = 0.05;

        for chunk in chunks {
            let Some(wav_chunk) = self.synthesize_one(
                &chunk,
                style,
                model_version,
                Some(language),
                total_step,
                speed,
            )?
            else {
                continue;
            };

            if !wav_cat.is_empty() {
                let silence_len = (silence_duration * self.sample_rate as f32) as usize;
                wav_cat.resize(wav_cat.len() + silence_len, 0.0);
            }
            wav_cat.extend_from_slice(&wav_chunk);
        }
        Ok(wav_cat)
    }

    fn synthesize_one(
        &mut self,
        text: &str,
        style: &Style,
        model_version: ModelVersion,
        language: Option<&str>,
        total_step: usize,
        speed: f32,
    ) -> Result<Option<Vec<f32>>> {
        let Some(processed_text) = prepare_text(text, model_version, language)? else {
            return Ok(None);
        };

        let (wav, duration) = self.infer(&[processed_text], style, total_step, speed)?;
        let duration = duration
            .first()
            .copied()
            .context("TTS model returned no duration")?;
        let wav_len = (self.sample_rate as f32 * duration) as usize;
        Ok(Some(wav[..wav_len.min(wav.len())].to_vec()))
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
        if duration.len() != bsz {
            bail!(
                "TTS model returned {} durations for a batch of {bsz}",
                duration.len()
            );
        }
        for dur in duration.iter_mut() {
            *dur /= speed;
        }
        if duration
            .iter()
            .any(|duration| !duration.is_finite() || *duration <= 0.0)
        {
            bail!("TTS model returned an invalid duration");
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
        if wav_data.iter().any(|sample| !sample.is_finite()) {
            bail!("TTS model returned non-finite audio samples");
        }
        Ok((wav_data.to_vec(), duration))
    }
}

fn get_tts_models_root_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os(TTS_MODELS_DIR_ENV).filter(|path| !path.is_empty()) {
        return validate_tts_models_root(path.into());
    }

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
        return validate_tts_models_root(data_dir);
    }

    #[cfg(not(target_os = "ios"))]
    {
        let data_dir = dirs::data_local_dir()
            .context("Failed to get local data directory")?
            .join("cloud.opensecret.maple")
            .join("tts_models");
        validate_tts_models_root(data_dir)
    }
}

fn validate_tts_models_root(path: PathBuf) -> Result<PathBuf> {
    if !path.is_absolute() {
        bail!("TTS model directory must be an absolute path");
    }
    if path.file_name().and_then(|name| name.to_str()) != Some("tts_models") {
        bail!("TTS model directory must end in 'tts_models'");
    }
    let parent = path.parent().context("TTS model directory has no parent")?;
    if parent.parent().is_none() {
        bail!("TTS model directory is too broad");
    }
    Ok(path)
}

fn get_supertonic3_models_dir() -> Result<PathBuf> {
    Ok(get_tts_models_root_dir()?.join(SUPERTONIC3_CACHE_DIR))
}

fn load_voice_style(models_dir: &Path) -> Result<Style> {
    // F2 preserves Maple's existing bright/friendly female default. All ten
    // published styles are pinned so a future selector does not require a new
    // model download.
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
    crate::onnxruntime::ensure_initialized().map_err(anyhow::Error::msg)?;

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
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Installation {
    None,
    Current(PathBuf),
    Legacy(PathBuf),
    Incompatible,
}

impl Installation {
    fn installed(&self) -> Option<(ModelVersion, &Path)> {
        match self {
            Self::Current(path) => Some((ModelVersion::Supertonic3, path)),
            Self::Legacy(path) => Some((ModelVersion::Legacy, path)),
            Self::None | Self::Incompatible => None,
        }
    }
}

fn revision_matches(models_dir: &Path) -> bool {
    fs::read_to_string(models_dir.join(MODEL_REVISION_FILE))
        .map(|revision| revision.trim() == HUGGINGFACE_REVISION)
        .unwrap_or(false)
}

fn files_match_sizes(models_dir: &Path, files: &[(&str, u64)]) -> bool {
    files.iter().all(|(name, expected_size)| {
        fs::metadata(models_dir.join(name))
            .map(|metadata| metadata.is_file() && metadata.len() == *expected_size)
            .unwrap_or(false)
    })
}

fn supertonic3_files_match_sizes(models_dir: &Path) -> bool {
    SUPERTONIC3_MODEL_FILES.iter().all(|file| {
        fs::metadata(models_dir.join(file.name))
            .map(|metadata| metadata.is_file() && metadata.len() == file.size)
            .unwrap_or(false)
    })
}

fn legacy_artifacts_present(root_dir: &Path) -> bool {
    LEGACY_MODEL_FILES
        .iter()
        .any(|(name, _)| root_dir.join(name).exists())
}

fn detect_installation(root_dir: &Path) -> Installation {
    let supertonic3_dir = root_dir.join(SUPERTONIC3_CACHE_DIR);
    if revision_matches(&supertonic3_dir) && supertonic3_files_match_sizes(&supertonic3_dir) {
        return Installation::Current(supertonic3_dir);
    }
    if files_match_sizes(root_dir, LEGACY_MODEL_FILES) {
        return Installation::Legacy(root_dir.to_path_buf());
    }
    if legacy_artifacts_present(root_dir) {
        return Installation::Incompatible;
    }
    Installation::None
}

fn current_installation() -> Result<Installation> {
    Ok(detect_installation(&get_tts_models_root_dir()?))
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
    let installation = current_installation().map_err(|error| error.to_string())?;
    let installed_version = installation.installed().map(|(version, _)| version);
    let (models_loaded, loaded_version) = {
        let guard = state.lock().map_err(|e| e.to_string())?;
        (
            guard.loaded.is_some(),
            guard.loaded.as_ref().map(|loaded| loaded.version),
        )
    };
    let model_version = loaded_version.or(installed_version);

    Ok(TTSStatusResponse {
        models_downloaded: installation.installed().is_some(),
        models_loaded,
        models_present_but_incompatible: installation == Installation::Incompatible,
        upgrade_available: model_version == Some(ModelVersion::Legacy),
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
    // Log the exact cause of any failure (each error string names the file and
    // the underlying error). Without this, a failed download produced no log
    // line after "Downloading TTS model: ...", and the frontend swallowed the
    // plain-string error behind a generic message. See PR #520 review.
    tts_download_models_impl(&app).await.inspect_err(|e| {
        log::error!("TTS model download failed: {e}");
    })
}

async fn tts_download_models_impl(app: &AppHandle) -> Result<(), String> {
    use std::time::Duration;

    let root_dir = get_tts_models_root_dir().map_err(|error| error.to_string())?;
    match detect_installation(&root_dir) {
        Installation::Legacy(_) => {
            return Err(
                "Your existing local TTS model must be deleted before downloading Supertonic 3."
                    .to_string(),
            );
        }
        Installation::Incompatible => {
            return Err(
                "Incompatible local TTS files must be deleted before downloading Supertonic 3."
                    .to_string(),
            );
        }
        Installation::None | Installation::Current(_) => {}
    }

    let models_dir = get_supertonic3_models_dir().map_err(|error| error.to_string())?;
    fs::create_dir_all(&models_dir)
        .map_err(|e| format!("Failed to create models directory: {e}"))?;
    log::info!("Using TTS model directory: {}", models_dir.display());

    let client = reqwest::Client::builder()
        // The largest v3 graph is about 257 MB. Five minutes was too short on
        // slower mobile connections, while retries safely reuse verified files.
        .timeout(Duration::from_secs(30 * 60))
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;
    let mut total_downloaded: u64 = 0;

    for model_file in SUPERTONIC3_MODEL_FILES {
        let file_path = models_dir.join(model_file.name);
        let temp_path = models_dir.join(format!("{}.part", model_file.name));

        // Reuse only files whose size and hash both match the pinned manifest.
        if file_path.exists() {
            if fs::metadata(&file_path)
                .map(|metadata| metadata.is_file() && metadata.len() == model_file.size)
                .unwrap_or(false)
            {
                match sha256_file(&file_path) {
                    Ok(actual_sha256) if actual_sha256 == model_file.sha256 => {
                        total_downloaded += model_file.size;
                        let _ = app.emit(
                            "tts-download-progress",
                            DownloadProgress {
                                downloaded: total_downloaded,
                                total: SUPERTONIC3_TOTAL_MODEL_SIZE,
                                file_name: model_file.name.to_string(),
                                percent: (total_downloaded as f64
                                    / SUPERTONIC3_TOTAL_MODEL_SIZE as f64)
                                    * 100.0,
                            },
                        );
                        continue;
                    }
                    Ok(actual_sha256) => log::info!(
                        "Existing TTS file {} has checksum {}, expected {}; re-downloading",
                        model_file.name,
                        actual_sha256,
                        model_file.sha256
                    ),
                    Err(error) => log::info!(
                        "Could not verify existing TTS file {}; re-downloading: {error}",
                        model_file.name
                    ),
                }
            }
            let _ = fs::remove_file(&file_path);
        }

        let _ = fs::remove_file(&temp_path);

        let url = format!(
            "{HUGGINGFACE_BASE_URL}/{HUGGINGFACE_REVISION}/{}",
            model_file.url_path
        );
        log::info!("Downloading TTS model: {}", model_file.name);

        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to download {} from {url}: {e}", model_file.name))?;

        if !response.status().is_success() {
            return Err(format!(
                "Failed to download {} from {}: HTTP {}",
                model_file.name,
                url,
                response.status()
            ));
        }

        let expected_len = response.content_length();
        let mut hasher = Sha256::new();

        let mut file = File::create(&temp_path)
            .map_err(|e| format!("Failed to create file {}: {e}", model_file.name))?;

        let mut stream = response.bytes_stream();
        let mut file_downloaded: u64 = 0;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk
                .map_err(|e| format!("Download error for {} from {url}: {e}", model_file.name))?;
            file.write_all(&chunk)
                .map_err(|e| format!("Write error for {}: {e}", model_file.name))?;

            hasher.update(&chunk);

            file_downloaded += chunk.len() as u64;
            let current_total = total_downloaded + file_downloaded;

            let _ = app.emit(
                "tts-download-progress",
                DownloadProgress {
                    downloaded: current_total,
                    total: SUPERTONIC3_TOTAL_MODEL_SIZE,
                    file_name: model_file.name.to_string(),
                    percent: (current_total as f64 / SUPERTONIC3_TOTAL_MODEL_SIZE as f64) * 100.0,
                },
            );
        }

        if let Some(expected_len) = expected_len {
            if file_downloaded != expected_len {
                drop(file);
                let _ = fs::remove_file(&temp_path);
                return Err(format!(
                    "Incomplete download for {}: expected {expected_len} bytes, got {file_downloaded}",
                    model_file.name
                ));
            }
        }

        if file_downloaded != model_file.size {
            drop(file);
            let _ = fs::remove_file(&temp_path);
            return Err(format!(
                "Unexpected download size for {}: expected {} bytes, got {file_downloaded}",
                model_file.name, model_file.size
            ));
        }

        let actual_sha256 = bytes_to_hex(hasher.finalize().as_ref());
        if actual_sha256 != model_file.sha256 {
            drop(file);
            let _ = fs::remove_file(&temp_path);
            return Err(format!(
                "Checksum mismatch for {}: expected {}, got {actual_sha256}",
                model_file.name, model_file.sha256
            ));
        }

        file.flush()
            .map_err(|e| format!("Failed to flush file {}: {e}", model_file.name))?;
        file.sync_all()
            .map_err(|e| format!("Failed to sync file {}: {e}", model_file.name))?;
        drop(file);
        fs::rename(&temp_path, &file_path)
            .map_err(|e| format!("Failed to finalize {}: {e}", model_file.name))?;

        total_downloaded += model_file.size;
        log::info!("Downloaded TTS model: {}", model_file.name);
    }

    let marker_path = models_dir.join(MODEL_REVISION_FILE);
    let marker_temp_path = models_dir.join(format!("{MODEL_REVISION_FILE}.part"));
    fs::write(&marker_temp_path, format!("{HUGGINGFACE_REVISION}\n"))
        .map_err(|e| format!("Failed to write TTS model revision marker: {e}"))?;
    let _ = fs::remove_file(&marker_path);
    fs::rename(&marker_temp_path, &marker_path)
        .map_err(|e| format!("Failed to finalize TTS model revision marker: {e}"))?;

    Ok(())
}

#[tauri::command]
pub async fn tts_load_models(state: tauri::State<'_, Mutex<TTSState>>) -> Result<(), String> {
    let installation = current_installation().map_err(|error| error.to_string())?;
    let (model_version, models_dir) = match installation {
        Installation::Current(path) => (ModelVersion::Supertonic3, path),
        Installation::Legacy(path) => (ModelVersion::Legacy, path),
        Installation::Incompatible => {
            return Err(
                "Local TTS files are incompatible and must be deleted before setup.".to_string(),
            );
        }
        Installation::None => return Err("TTS models are not downloaded.".to_string()),
    };

    log::info!(
        "Loading {model_version:?} TTS models from {}",
        models_dir.display()
    );

    let engine = load_tts_engine(&models_dir, model_version)
        .map_err(|e| format!("Failed to load TTS engine: {e}"))?;
    let style =
        load_voice_style(&models_dir).map_err(|e| format!("Failed to load voice style: {e}"))?;

    {
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        guard.loaded = Some(LoadedTts {
            engine,
            style,
            version: model_version,
        });
    }

    log::info!("TTS models loaded successfully");
    Ok(())
}

#[derive(Serialize)]
pub struct TTSSynthesizeResponse {
    pub audio_base64: String,
    pub sample_rate: i32,
    pub duration_seconds: f32,
    pub skipped: bool,
}

#[derive(Serialize)]
pub struct TTSChunkTextResponse {
    pub chunks: Vec<String>,
}

fn encode_synthesis_response(audio: Vec<f32>, sample_rate: i32) -> Result<TTSSynthesizeResponse> {
    if audio.is_empty() {
        return Ok(TTSSynthesizeResponse {
            audio_base64: String::new(),
            sample_rate,
            duration_seconds: 0.0,
            skipped: true,
        });
    }

    let duration_seconds = audio.len() as f32 / sample_rate as f32;
    let audio_base64 = wav_to_base64(&audio, sample_rate)?;
    Ok(TTSSynthesizeResponse {
        audio_base64,
        sample_rate,
        duration_seconds,
        skipped: false,
    })
}

#[tauri::command]
pub async fn tts_chunk_text(
    text: String,
    language: Option<String>,
) -> Result<TTSChunkTextResponse, String> {
    let language = normalize_language(language.as_deref()).map_err(|error| error.to_string())?;
    let chunks = chunk_text(&text, chunk_size_for_language(language))
        .into_iter()
        .filter(|chunk| !normalize_text_for_tts(chunk, false).is_empty())
        .collect::<Vec<_>>();

    log::info!(
        "TTS chunk plan: input_chars={}, chunks={}, language={language}",
        text.chars().count(),
        chunks.len()
    );
    Ok(TTSChunkTextResponse { chunks })
}

#[tauri::command]
pub async fn tts_synthesize(
    text: String,
    language: Option<String>,
    speed: Option<f32>,
    state: tauri::State<'_, Mutex<TTSState>>,
) -> Result<TTSSynthesizeResponse, String> {
    if text.trim().is_empty() {
        return Err("No text to synthesize".to_string());
    }

    let started = Instant::now();
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    let loaded = guard.loaded.as_mut().ok_or("TTS engine not loaded")?;
    let style = loaded.style.clone();
    let speed = resolve_tts_speed(loaded.version, speed);
    let total_steps = total_steps_for_model(loaded.version);
    let sample_rate = loaded.engine.sample_rate;
    let audio = loaded
        .engine
        .synthesize(
            &text,
            &style,
            loaded.version,
            language.as_deref(),
            total_steps,
            speed,
        )
        .map_err(|e| format!("TTS synthesis failed: {e}"))?;
    if audio.is_empty() {
        return Err("No speakable text after preprocessing".to_string());
    }
    drop(guard);

    let response = encode_synthesis_response(audio, sample_rate)
        .map_err(|e| format!("Failed to encode audio: {e}"))?;
    log::info!(
        "TTS synthesis complete: {:.2}s audio in {}ms",
        response.duration_seconds,
        started.elapsed().as_millis()
    );
    Ok(response)
}

#[tauri::command]
pub async fn tts_synthesize_chunk(
    text: String,
    chunk_index: usize,
    chunk_count: usize,
    language: Option<String>,
    speed: Option<f32>,
    state: tauri::State<'_, Mutex<TTSState>>,
) -> Result<TTSSynthesizeResponse, String> {
    if chunk_index == 0 || chunk_count == 0 || chunk_index > chunk_count {
        return Err("Invalid TTS chunk position".to_string());
    }

    let started = Instant::now();
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    let loaded = guard.loaded.as_mut().ok_or("TTS engine not loaded")?;
    let style = loaded.style.clone();
    let speed = resolve_tts_speed(loaded.version, speed);
    let total_steps = total_steps_for_model(loaded.version);
    let sample_rate = loaded.engine.sample_rate;
    let audio = loaded
        .engine
        .synthesize_one(
            &text,
            &style,
            loaded.version,
            language.as_deref(),
            total_steps,
            speed,
        )
        .map_err(|e| format!("TTS synthesis failed: {e}"))?
        .unwrap_or_default();
    drop(guard);

    let response = encode_synthesis_response(audio, sample_rate)
        .map_err(|e| format!("Failed to encode audio: {e}"))?;
    log::info!(
        "TTS chunk {chunk_index}/{chunk_count} complete: skipped={}, audio_seconds={:.2}, elapsed_ms={}",
        response.skipped,
        response.duration_seconds,
        started.elapsed().as_millis()
    );
    Ok(response)
}

#[tauri::command]
pub async fn tts_unload_models(state: tauri::State<'_, Mutex<TTSState>>) -> Result<(), String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    guard.loaded = None;
    log::info!("TTS models unloaded");
    Ok(())
}

#[tauri::command]
pub async fn tts_delete_models(state: tauri::State<'_, Mutex<TTSState>>) -> Result<(), String> {
    let loaded = {
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        guard.loaded.take()
    };
    // Drop ONNX sessions before removing their backing files, which is
    // required on Windows and makes deletion deterministic everywhere.
    drop(loaded);

    let root_dir = get_tts_models_root_dir().map_err(|e| e.to_string())?;
    log::info!("Deleting TTS model directory: {}", root_dir.display());
    if root_dir.exists() {
        fs::remove_dir_all(&root_dir).map_err(|e| format!("Failed to delete TTS models: {e}"))?;
    }

    log::info!("TTS models deleted");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use unicode_normalization::UnicodeNormalization;

    fn create_sized_file(path: &Path, size: u64) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        File::create(path).unwrap().set_len(size).unwrap();
    }

    fn seed_legacy(root: &Path) {
        for (name, size) in LEGACY_MODEL_FILES {
            create_sized_file(&root.join(name), *size);
        }
    }

    fn seed_supertonic3(root: &Path, revision: &str) {
        let models_dir = root.join(SUPERTONIC3_CACHE_DIR);
        for model_file in SUPERTONIC3_MODEL_FILES {
            create_sized_file(&models_dir.join(model_file.name), model_file.size);
        }
        fs::write(models_dir.join(MODEL_REVISION_FILE), revision).unwrap();
    }

    #[test]
    fn bytes_to_hex_is_lowercase_and_zero_padded() {
        assert_eq!(bytes_to_hex(&[0x00, 0xab, 0xff]), "00abff");
    }

    #[test]
    fn normalization_strips_markdown_emoji_and_expression_tags() {
        assert_eq!(
            normalize_text_for_tts("**Hello** _world_ <laugh> 😊", false),
            "Hello world."
        );
    }

    #[test]
    fn supertonic3_preserves_diacritics_and_wraps_language() {
        let prepared = prepare_text("Grüße déjà vu", ModelVersion::Supertonic3, Some("de"))
            .unwrap()
            .unwrap();
        assert_eq!(
            prepared.nfc().collect::<String>(),
            "<de>Grüße déjà vu.</de>"
        );
    }

    #[test]
    fn legacy_normalization_retains_existing_diacritic_behavior() {
        let prepared = prepare_text("Grüße", ModelVersion::Legacy, None)
            .unwrap()
            .unwrap();
        assert_eq!(prepared, "Gruße.");
    }

    #[test]
    fn language_defaults_to_language_agnostic_and_validates_codes() {
        assert_eq!(normalize_language(None).unwrap(), "na");
        for language in AVAILABLE_LANGUAGES {
            assert_eq!(normalize_language(Some(language)).unwrap(), *language);
        }
        assert!(normalize_language(Some("xx")).is_err());
    }

    #[test]
    fn emoji_only_text_is_a_successful_skip() {
        assert_eq!(
            prepare_text("😊 🎉", ModelVersion::Supertonic3, None).unwrap(),
            None
        );
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
    fn chunk_text_packs_short_paragraphs() {
        assert_eq!(
            chunk_text("One.\n\nTwo. Three.", 20),
            vec!["One. Two. Three.".to_string()]
        );
    }

    #[test]
    fn chunk_text_hard_splits_unbroken_unicode_without_recursion() {
        let chunks = chunk_text("ééééééééééé", 4);
        assert_eq!(
            chunks
                .iter()
                .map(|chunk| chunk.chars().count())
                .collect::<Vec<_>>(),
            vec![4, 4, 3]
        );
    }

    #[test]
    fn chunk_text_respects_cjk_character_bounds() {
        let chunks = chunk_text("日本語の長い文章です日本語の長い文章です", 6);
        assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 6));
        assert_eq!(chunks.concat(), "日本語の長い文章です日本語の長い文章です");
    }

    #[test]
    fn chunk_text_returns_no_chunks_for_empty_input_or_zero_limit() {
        assert!(chunk_text("   ", 10).is_empty());
        assert!(chunk_text("hello", 0).is_empty());
    }

    #[test]
    fn speed_defaults_and_clamps_are_model_specific() {
        assert_eq!(resolve_tts_speed(ModelVersion::Supertonic3, None), 1.2);
        assert_eq!(resolve_tts_speed(ModelVersion::Legacy, None), 1.2);
        assert_eq!(resolve_tts_speed(ModelVersion::Supertonic3, Some(0.1)), 0.5);
        assert_eq!(resolve_tts_speed(ModelVersion::Supertonic3, Some(3.0)), 2.0);
        assert_eq!(resolve_tts_speed(ModelVersion::Legacy, Some(f32::NAN)), 1.2);
        assert_eq!(total_steps_for_model(ModelVersion::Supertonic3), 8);
        assert_eq!(total_steps_for_model(ModelVersion::Legacy), 10);
    }

    #[test]
    fn manifest_total_and_hash_shapes_match_the_pin() {
        assert_eq!(
            SUPERTONIC3_MODEL_FILES
                .iter()
                .map(|model_file| model_file.size)
                .sum::<u64>(),
            SUPERTONIC3_TOTAL_MODEL_SIZE
        );
        assert!(SUPERTONIC3_MODEL_FILES.iter().all(|model_file| {
            model_file.sha256.len() == 64
                && model_file
                    .sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        }));
    }

    #[test]
    fn detects_none_legacy_current_partial_and_incompatible_models() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("tts_models");
        assert_eq!(detect_installation(&root), Installation::None);

        seed_legacy(&root);
        assert_eq!(
            detect_installation(&root),
            Installation::Legacy(root.clone())
        );

        fs::remove_dir_all(&root).unwrap();
        create_sized_file(&root.join("duration_predictor.onnx"), 10);
        assert_eq!(detect_installation(&root), Installation::Incompatible);

        fs::remove_dir_all(&root).unwrap();
        create_sized_file(
            &root
                .join(SUPERTONIC3_CACHE_DIR)
                .join("duration_predictor.onnx"),
            SUPERTONIC3_MODEL_FILES[0].size,
        );
        assert_eq!(detect_installation(&root), Installation::None);

        fs::remove_dir_all(&root).unwrap();
        seed_supertonic3(&root, "wrong-revision");
        assert_eq!(detect_installation(&root), Installation::None);

        fs::write(
            root.join(SUPERTONIC3_CACHE_DIR).join(MODEL_REVISION_FILE),
            HUGGINGFACE_REVISION,
        )
        .unwrap();
        assert_eq!(
            detect_installation(&root),
            Installation::Current(root.join(SUPERTONIC3_CACHE_DIR))
        );
    }

    #[test]
    fn checksum_helper_reads_files_incrementally() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("fixture");
        fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn model_root_override_validation_is_fail_closed() {
        assert!(validate_tts_models_root(PathBuf::from("relative/tts_models")).is_err());
        let temp_root = std::env::temp_dir().join("maple");
        assert!(validate_tts_models_root(temp_root.join("not-models")).is_err());
        let valid_root = temp_root.join("tts_models");
        assert_eq!(
            validate_tts_models_root(valid_root.clone()).unwrap(),
            valid_root
        );
    }

    #[test]
    fn wav_response_is_mono_pcm16_and_skip_is_explicit() {
        let skipped = encode_synthesis_response(Vec::new(), 44_100).unwrap();
        assert!(skipped.skipped);
        assert!(skipped.audio_base64.is_empty());

        let response = encode_synthesis_response(vec![0.0, 0.5, -0.5], 44_100).unwrap();
        assert!(!response.skipped);
        let bytes = BASE64.decode(response.audio_base64).unwrap();
        let reader = hound::WavReader::new(Cursor::new(bytes)).unwrap();
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.spec().sample_rate, 44_100);
        assert_eq!(reader.spec().bits_per_sample, 16);
    }

    #[test]
    #[ignore = "requires downloaded Supertonic 3 model files"]
    fn supertonic3_smoke_produces_finite_non_silent_audio_and_valid_wav() {
        let models_dir = PathBuf::from(
            std::env::var("MAPLE_SUPERTONIC3_SMOKE_DIR")
                .expect("MAPLE_SUPERTONIC3_SMOKE_DIR must point to Supertonic 3 model files"),
        );
        let mut engine = load_tts_engine(&models_dir, ModelVersion::Supertonic3).unwrap();
        let style = load_voice_style(&models_dir).unwrap();
        let audio = engine
            .synthesize(
                "Hello from Maple. This is a real Supertonic 3 smoke test.",
                &style,
                ModelVersion::Supertonic3,
                Some("en"),
                SUPERTONIC3_TOTAL_STEPS,
                SUPERTONIC3_DEFAULT_SPEED,
            )
            .unwrap();

        assert_eq!(engine.sample_rate, 44_100);
        assert!(audio.len() > 44_100 / 4);
        assert!(audio.iter().all(|sample| sample.is_finite()));
        let peak = audio
            .iter()
            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
        let rms =
            (audio.iter().map(|sample| sample * sample).sum::<f32>() / audio.len() as f32).sqrt();
        assert!(peak > 0.001, "audio peak was {peak}");
        assert!(rms > 0.0001, "audio RMS was {rms}");

        let response = encode_synthesis_response(audio, engine.sample_rate).unwrap();
        let wav = BASE64.decode(response.audio_base64).unwrap();
        let reader = hound::WavReader::new(Cursor::new(wav)).unwrap();
        assert_eq!(reader.spec().sample_rate, 44_100);
        assert_eq!(reader.spec().channels, 1);
    }
}
