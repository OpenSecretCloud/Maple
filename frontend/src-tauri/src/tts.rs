use anyhow::{Context, Result};
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
use std::io::{BufReader, Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
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
const HUGGINGFACE_REVISION: &str = "b6856d033f622c63ea29441795be266a1133e227";
const HUGGINGFACE_BASE_URL: &str = "https://huggingface.co/Supertone/supertonic/resolve";

// (file_name, url_path, expected_size_bytes, expected_sha256_hex)
const MODEL_FILES: &[(&str, &str, u64, &str)] = &[
    (
        "duration_predictor.onnx",
        "onnx/duration_predictor.onnx",
        1_500_789,
        "b861580c56a0cba2a2b82aa697ecb3c5a163c3240c60a0ddfac369d21d054092",
    ),
    (
        "text_encoder.onnx",
        "onnx/text_encoder.onnx",
        27_348_373,
        "ba0c8ea74aeb5df00d21a89b8d47c71317f47120232e3deef95024dba37dbd88",
    ),
    (
        "vector_estimator.onnx",
        "onnx/vector_estimator.onnx",
        132_471_364,
        "b3f82ecd2e9decc4e2236048b03628a1c1d5f14a792ba274a59b7325107aa6a6",
    ),
    (
        "vocoder.onnx",
        "onnx/vocoder.onnx",
        101_405_066,
        "19bd51f47a186069c752403518a40f7ea4c647455056d2511f7249691ecddf7c",
    ),
    (
        "tts.json",
        "onnx/tts.json",
        8_645,
        "4dac5f986698a3ace9a97ea2545d43f6c8ba120d25e005f8c905128281be9b6d",
    ),
    (
        "unicode_indexer.json",
        "onnx/unicode_indexer.json",
        262_134,
        "0c3800ba4fb1fc760c9070eb43a0ad5a68279ec165742591a68ea3edca452978",
    ),
    (
        "F1.json",
        "voice_styles/F1.json",
        420_622,
        "1450bcad84a2790eaf73f85e763dd5bae7c399f55d692c4835cf4f7686b5a10f",
    ),
    (
        "F2.json",
        "voice_styles/F2.json",
        420_905,
        "47c8d44445ef8ac8aae8ef5806feca21903483cbd4f1232e405184a40520a549",
    ),
    (
        "M1.json",
        "voice_styles/M1.json",
        421_053,
        "273c9ba6582d2e00383d8fbe2f5d660d86e8fba849c91ff695384d1a6e2e02f1",
    ),
    (
        "M2.json",
        "voice_styles/M2.json",
        421_027,
        "26898a9ec3de1b5bf8cc3f6cbf41930543ca0403f2201e12aad849691ff315dd",
    ),
];

const TOTAL_MODEL_SIZE: u64 = 264_679_978; // bytes

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
}

impl UnicodeProcessor {
    fn new(indexer: Vec<i64>) -> Self {
        UnicodeProcessor { indexer }
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
                    // Use 0 (padding token) for out-of-vocabulary characters
                    row[j] = 0;
                }
            }
            text_ids.push(row);
        }

        let text_mask = length_to_mask(&text_ids_lengths, Some(max_len));
        (text_ids, text_mask)
    }
}

fn preprocess_text(text: &str) -> String {
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
}

impl TTSState {
    pub fn new() -> Mutex<Self> {
        Mutex::new(TTSState {
            tts: None,
            style: None,
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

            let processed_chunk = preprocess_text(chunk);
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

fn get_tts_models_dir() -> Result<PathBuf> {
    let data_dir = dirs::data_local_dir()
        .context("Failed to get local data directory")?
        .join("cloud.opensecret.maple")
        .join("tts_models");
    Ok(data_dir)
}

fn load_voice_style(models_dir: &Path) -> Result<Style> {
    // TODO: Add voice selection API - currently hardcoded to F2
    // Available voices: F1, F2, M1, M2
    let style_path = models_dir.join("F2.json");
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

fn load_tts_engine(models_dir: &Path) -> Result<TextToSpeech> {
    let cfg_path = models_dir.join("tts.json");
    let file = File::open(&cfg_path)?;
    let reader = BufReader::new(file);
    let cfgs: Config = serde_json::from_reader(reader)?;

    let indexer_path = models_dir.join("unicode_indexer.json");
    let file = File::open(&indexer_path)?;
    let reader = BufReader::new(file);
    let indexer: Vec<i64> = serde_json::from_reader(reader)?;
    let text_processor = UnicodeProcessor::new(indexer);

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
    pub total_size_mb: f64,
}

#[tauri::command]
pub async fn tts_get_status(
    state: tauri::State<'_, Mutex<TTSState>>,
) -> Result<TTSStatusResponse, String> {
    let models_dir = get_tts_models_dir().map_err(|e| e.to_string())?;

    let models_downloaded =
        MODEL_FILES.iter().all(|(name, _, expected_size, _)| {
            match fs::metadata(models_dir.join(name)) {
                Ok(meta) => meta.len() == *expected_size,
                Err(_) => false,
            }
        });
    let models_loaded = {
        let guard = state.lock().map_err(|e| e.to_string())?;
        guard.tts.is_some() && guard.style.is_some()
    };

    Ok(TTSStatusResponse {
        models_downloaded,
        models_loaded,
        total_size_mb: TOTAL_MODEL_SIZE as f64 / 1024.0 / 1024.0,
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

    let models_dir = get_tts_models_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&models_dir)
        .map_err(|e| format!("Failed to create models directory: {e}"))?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;
    let mut total_downloaded: u64 = 0;

    for (file_name, url_path, expected_size, expected_sha256) in MODEL_FILES {
        let file_path = models_dir.join(file_name);
        let temp_path = models_dir.join(format!("{file_name}.part"));

        // Skip if already downloaded
        if file_path.exists() {
            if let Ok(meta) = fs::metadata(&file_path) {
                if meta.len() == *expected_size {
                    total_downloaded += expected_size;
                    let _ = app.emit(
                        "tts-download-progress",
                        DownloadProgress {
                            downloaded: total_downloaded,
                            total: TOTAL_MODEL_SIZE,
                            file_name: file_name.to_string(),
                            percent: (total_downloaded as f64 / TOTAL_MODEL_SIZE as f64) * 100.0,
                        },
                    );
                    continue;
                }
            }

            // Zero-byte or unreadable file: treat as invalid and re-download
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
                    total: TOTAL_MODEL_SIZE,
                    file_name: file_name.to_string(),
                    percent: (current_total as f64 / TOTAL_MODEL_SIZE as f64) * 100.0,
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

    Ok(())
}

#[tauri::command]
pub async fn tts_load_models(state: tauri::State<'_, Mutex<TTSState>>) -> Result<(), String> {
    let models_dir = get_tts_models_dir().map_err(|e| e.to_string())?;

    log::info!("Loading TTS models from {models_dir:?}");

    let tts =
        load_tts_engine(&models_dir).map_err(|e| format!("Failed to load TTS engine: {e}"))?;
    let style =
        load_voice_style(&models_dir).map_err(|e| format!("Failed to load voice style: {e}"))?;

    {
        let mut guard = state.lock().map_err(|e| e.to_string())?;
        guard.tts = Some(tts);
        guard.style = Some(style);
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
    }

    // Delete the models directory
    let models_dir = get_tts_models_dir().map_err(|e| e.to_string())?;
    if models_dir.exists() {
        fs::remove_dir_all(&models_dir).map_err(|e| format!("Failed to delete TTS models: {e}"))?;
    }

    log::info!("TTS models deleted");
    Ok(())
}
