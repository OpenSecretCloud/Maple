//! Whole-recording transcription through the attested Continuum proxy.
//!
//! WAV duration is validated from actual frames before sending. Other supported
//! containers remain opaque to the enclave: the provider owns decoding and its
//! returned duration is checked before a successful response is accepted.
//! There is deliberately no provider fallback, chunking, or POST replay here.

use crate::provider_client::{ProviderClient, ProviderRequest, ProviderRequestError};
use crate::proxy_config::ProxyConfig;
use axum::http::StatusCode;
use base64::{engine::general_purpose, Engine};
use futures::StreamExt;
use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;
use uuid::Uuid;

pub(crate) const MODEL: &str = "whisper-large-v3";
// Twenty minutes of the application's 16 kHz mono PCM16 WAV is 38,400,044
// bytes. Its base64 representation, plus JSON, fits the existing 50 MiB V2 cap.
pub(crate) const MAX_AUDIO_BYTES: usize = 39_000_000;
pub(crate) const MAX_AUDIO_SECONDS: f64 = 1_200.0;
const REQUEST_DEADLINE: Duration = Duration::from_secs(120);
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_PROMPT_BYTES: usize = 4096;

#[derive(Clone, Deserialize)]
pub(crate) struct TranscriptionRequest {
    pub(crate) file: String,
    #[serde(default = "default_filename")]
    pub(crate) filename: String,
    #[serde(default = "default_content_type")]
    pub(crate) content_type: String,
    #[serde(default = "default_model")]
    pub(crate) model: String,
    pub(crate) language: Option<String>,
    pub(crate) prompt: Option<String>,
    #[serde(default = "default_response_format")]
    pub(crate) response_format: String,
    pub(crate) temperature: Option<f64>,
}

fn default_filename() -> String {
    "audio.mp3".to_owned()
}

fn default_content_type() -> String {
    "audio/mpeg".to_owned()
}

fn default_model() -> String {
    MODEL.to_owned()
}

fn default_response_format() -> String {
    "json".to_owned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TranscriptionError {
    #[error("Transcription is available through the Maple app only.")]
    AppOnly,
    #[error("A paid plan is required for transcription.")]
    PaidRequired,
    #[error("The audio file is empty, malformed, or incomplete.")]
    InvalidAudio,
    #[error("This audio format is not supported.")]
    UnsupportedAudio,
    #[error("The audio file exceeds the upload limit.")]
    AudioTooLarge,
    #[error("The recording exceeds the 20 minute transcription limit.")]
    AudioTooLong,
    #[error("The transcription parameters are invalid.")]
    InvalidParameters,
    #[error("The requested transcription model is not supported.")]
    UnsupportedModel,
    #[error("No speech was detected. Your recording was not transcribed.")]
    NoSpeech,
    #[error("The transcription provider returned an invalid or incomplete response.")]
    InvalidResponse,
    #[error("The transcription provider returned an oversized response.")]
    ResponseTooLarge,
    #[error("Transcription is temporarily unavailable. Please try again later.")]
    Unavailable,
    #[error("Transcription is at capacity. Please try again later.")]
    RateLimited,
    #[error("The transcription provider could not complete this recording.")]
    Upstream,
    #[error("Transcription timed out before the complete result was received.")]
    Timeout,
}

impl TranscriptionError {
    pub(crate) fn status(self) -> StatusCode {
        match self {
            Self::AppOnly | Self::PaidRequired => StatusCode::FORBIDDEN,
            Self::InvalidAudio | Self::InvalidParameters | Self::UnsupportedModel => {
                StatusCode::BAD_REQUEST
            }
            Self::UnsupportedAudio | Self::AudioTooLong | Self::NoSpeech => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            Self::AudioTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::Timeout => StatusCode::GATEWAY_TIMEOUT,
            Self::InvalidResponse | Self::ResponseTooLarge | Self::Upstream => {
                StatusCode::BAD_GATEWAY
            }
        }
    }

    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::AppOnly => "transcription_app_only",
            Self::PaidRequired => "transcription_paid_required",
            Self::InvalidAudio => "transcription_invalid_audio",
            Self::UnsupportedAudio => "transcription_unsupported_audio",
            Self::AudioTooLarge => "transcription_audio_too_large",
            Self::AudioTooLong => "transcription_audio_too_long",
            Self::InvalidParameters => "transcription_invalid_parameters",
            Self::UnsupportedModel => "transcription_unsupported_model",
            Self::NoSpeech => "transcription_no_speech",
            Self::InvalidResponse => "transcription_invalid_response",
            Self::ResponseTooLarge => "transcription_response_too_large",
            Self::Unavailable => "transcription_unavailable",
            Self::RateLimited => "transcription_rate_limited",
            Self::Upstream => "transcription_upstream_error",
            Self::Timeout => "transcription_timeout",
        }
    }
}

pub(crate) struct PreparedTranscription {
    audio: Vec<u8>,
    audio_seconds: Option<f64>,
    format: AudioFormat,
    language: Option<String>,
    prompt: Option<String>,
    response_format: String,
    temperature: Option<f64>,
}

impl PreparedTranscription {
    pub(crate) fn audio_seconds(&self) -> Option<f64> {
        self.audio_seconds
    }

    pub(crate) fn file_size(&self) -> usize {
        self.audio.len()
    }
}

#[derive(Clone, Copy)]
struct AudioFormat {
    content_type: &'static str,
    filename: &'static str,
    is_wav: bool,
}

fn audio_format(content_type: &str) -> Result<AudioFormat, TranscriptionError> {
    // Only these canonical strings reach a multipart header. Caller metadata,
    // including the filename and MIME parameters, never becomes a header.
    let mime = content_type.split(';').next().unwrap_or("").trim();
    let (content_type, filename, is_wav) = match mime.to_ascii_lowercase().as_str() {
        "audio/wav" | "audio/wave" | "audio/x-wav" | "audio/vnd.wave" => {
            ("audio/wav", "audio.wav", true)
        }
        "audio/mpeg" | "audio/mp3" | "audio/mpga" => ("audio/mpeg", "audio.mp3", false),
        "audio/mp4" | "audio/m4a" | "audio/x-m4a" | "video/mp4" => {
            ("audio/mp4", "audio.m4a", false)
        }
        "audio/flac" | "audio/x-flac" => ("audio/flac", "audio.flac", false),
        "audio/ogg" | "application/ogg" => ("audio/ogg", "audio.ogg", false),
        "audio/webm" | "video/webm" => ("audio/webm", "audio.webm", false),
        _ => return Err(TranscriptionError::UnsupportedAudio),
    };
    Ok(AudioFormat {
        content_type,
        filename,
        is_wav,
    })
}

pub(crate) fn prepare(
    request: TranscriptionRequest,
) -> Result<PreparedTranscription, TranscriptionError> {
    if !matches!(request.model.as_str(), MODEL | "whisper-large-v3-turbo") {
        return Err(TranscriptionError::UnsupportedModel);
    }
    let language = match request.language {
        None => None,
        Some(value) if value.is_empty() || value == "auto" => None,
        Some(value) if value.len() == 2 && value.bytes().all(|b| b.is_ascii_lowercase()) => {
            Some(value)
        }
        _ => return Err(TranscriptionError::InvalidParameters),
    };
    if !matches!(request.response_format.as_str(), "json" | "verbose_json")
        || (request.response_format == "verbose_json" && language.is_none())
        || request
            .temperature
            .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        || request
            .prompt
            .as_ref()
            .is_some_and(|value| value.len() > MAX_PROMPT_BYTES)
        || request.filename.len() > 256
        || request.content_type.len() > 256
    {
        return Err(TranscriptionError::InvalidParameters);
    }
    let format = audio_format(&request.content_type)?;
    // Reject oversized base64 before allocating its decoded representation.
    let max_encoded_bytes = MAX_AUDIO_BYTES.div_ceil(3) * 4;
    if request.file.len() > max_encoded_bytes {
        return Err(TranscriptionError::AudioTooLarge);
    }
    let audio = general_purpose::STANDARD
        .decode(request.file)
        .map_err(|_| TranscriptionError::InvalidAudio)?;
    if audio.is_empty() {
        return Err(TranscriptionError::InvalidAudio);
    }
    if audio.len() > MAX_AUDIO_BYTES {
        return Err(TranscriptionError::AudioTooLarge);
    }
    // A WAV signature must not bypass local validation by claiming to be MP3.
    let looks_like_wav = audio.starts_with(b"RIFF") && audio.get(8..12) == Some(b"WAVE");
    let audio_seconds = if format.is_wav || looks_like_wav {
        if !format.is_wav {
            return Err(TranscriptionError::InvalidAudio);
        }
        let wav = validate_wav(&audio)?;
        if wav.digital_silence {
            return Err(TranscriptionError::NoSpeech);
        }
        Some(wav.duration_seconds)
    } else {
        None
    };
    Ok(PreparedTranscription {
        audio,
        audio_seconds,
        format,
        language,
        prompt: request.prompt,
        response_format: request.response_format,
        temperature: request.temperature,
    })
}

fn u16_le(bytes: &[u8], offset: usize) -> Result<u16, TranscriptionError> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or(TranscriptionError::InvalidAudio)?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn u32_le(bytes: &[u8], offset: usize) -> Result<u32, TranscriptionError> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or(TranscriptionError::InvalidAudio)?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

struct ValidatedWav {
    duration_seconds: f64,
    digital_silence: bool,
}

fn validate_wav(audio: &[u8]) -> Result<ValidatedWav, TranscriptionError> {
    if audio.len() < 44
        || !audio.starts_with(b"RIFF")
        || audio.get(8..12) != Some(b"WAVE")
        || u32_le(audio, 4)? as usize + 8 != audio.len()
    {
        return Err(TranscriptionError::InvalidAudio);
    }
    let mut offset = 12usize;
    let mut format = None;
    let mut data_bytes = None;
    while offset < audio.len() {
        let header = audio
            .get(offset..offset + 8)
            .ok_or(TranscriptionError::InvalidAudio)?;
        let chunk_len = u32_le(header, 4)? as usize;
        let start = offset + 8;
        let end = start
            .checked_add(chunk_len)
            .filter(|end| *end <= audio.len())
            .ok_or(TranscriptionError::InvalidAudio)?;
        match &header[..4] {
            b"fmt " => {
                if format.is_some() || data_bytes.is_some() || chunk_len < 16 {
                    return Err(TranscriptionError::InvalidAudio);
                }
                let fmt = &audio[start..end];
                let encoding = u16_le(fmt, 0)?;
                let channels = u16_le(fmt, 2)?;
                let sample_rate = u32_le(fmt, 4)?;
                let byte_rate = u32_le(fmt, 8)?;
                let block_align = u16_le(fmt, 12)?;
                let bits = u16_le(fmt, 14)?;
                let valid_bits = match encoding {
                    1 => matches!(bits, 8 | 16 | 24 | 32),
                    3 => matches!(bits, 32 | 64),
                    _ => false,
                };
                if !valid_bits
                    || !(1..=2).contains(&channels)
                    || !(8_000..=192_000).contains(&sample_rate)
                    || block_align as u32 != channels as u32 * bits as u32 / 8
                    || byte_rate != sample_rate * block_align as u32
                {
                    return Err(TranscriptionError::UnsupportedAudio);
                }
                // PCM and float may use WAVEFORMATEX; unsupported extensions
                // must not silently change the interpretation of frame sizes.
                if chunk_len != 16 && (chunk_len != 18 || u16_le(fmt, 16)? != 0) {
                    return Err(TranscriptionError::UnsupportedAudio);
                }
                format = Some((sample_rate, block_align, encoding, bits));
            }
            b"data" => {
                if format.is_none() || data_bytes.is_some() || chunk_len == 0 {
                    return Err(TranscriptionError::InvalidAudio);
                }
                data_bytes = Some((start, end));
            }
            _ => {}
        }
        offset = end
            .checked_add(chunk_len % 2)
            .filter(|end| *end <= audio.len())
            .ok_or(TranscriptionError::InvalidAudio)?;
    }
    let (sample_rate, block_align, encoding, bits) =
        format.ok_or(TranscriptionError::InvalidAudio)?;
    let (data_start, data_end) = data_bytes.ok_or(TranscriptionError::InvalidAudio)?;
    let samples = &audio[data_start..data_end];
    if !samples.len().is_multiple_of(block_align as usize) {
        return Err(TranscriptionError::InvalidAudio);
    }
    let frames = samples.len() / block_align as usize;
    if frames > sample_rate as usize * MAX_AUDIO_SECONDS as usize {
        return Err(TranscriptionError::AudioTooLong);
    }
    // Detect only exact digital zero in the validated data chunk. This is not
    // an energy or speech threshold: even one nonzero least significant bit
    // keeps a quiet recording eligible for transcription. Unsigned PCM8 zero
    // is its midpoint; IEEE float positive and negative zero are both silent.
    let digital_silence = match (encoding, bits) {
        (1, 8) => samples.iter().all(|byte| *byte == 128),
        (1, 16 | 24 | 32) => samples.iter().all(|byte| *byte == 0),
        (3, 32 | 64) => {
            let width = bits as usize / 8;
            samples.chunks_exact(width).all(|sample| {
                sample[..width - 1].iter().all(|byte| *byte == 0) && sample[width - 1] & 0x7f == 0
            })
        }
        _ => return Err(TranscriptionError::UnsupportedAudio),
    };
    Ok(ValidatedWav {
        duration_seconds: frames as f64 / sample_rate as f64,
        digital_silence,
    })
}

#[cfg(test)]
fn wav_duration(audio: &[u8]) -> Result<f64, TranscriptionError> {
    validate_wav(audio).map(|wav| wav.duration_seconds)
}

fn form_field(body: &mut Vec<u8>, boundary: &str, name: &str, value: &str) {
    body.extend_from_slice(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(value.as_bytes());
    body.extend_from_slice(b"\r\n");
}

fn multipart(prepared: PreparedTranscription) -> (String, Vec<u8>) {
    let boundary = format!("maple-transcription-{}", Uuid::new_v4().simple());
    let mut body = Vec::with_capacity(prepared.audio.len() + 8192);
    form_field(&mut body, &boundary, "model", MODEL);
    form_field(
        &mut body,
        &boundary,
        "response_format",
        &prepared.response_format,
    );
    if let Some(language) = prepared.language {
        form_field(&mut body, &boundary, "language", &language);
    }
    if let Some(prompt) = prepared.prompt {
        form_field(&mut body, &boundary, "prompt", &prompt);
    }
    if let Some(temperature) = prepared.temperature {
        form_field(
            &mut body,
            &boundary,
            "temperature",
            &temperature.to_string(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",
            prepared.format.filename, prepared.format.content_type
        )
        .as_bytes(),
    );
    body.extend_from_slice(&prepared.audio);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

fn provider_error(error: ProviderRequestError) -> TranscriptionError {
    match error {
        ProviderRequestError::Timeout(_) => TranscriptionError::Timeout,
        ProviderRequestError::TinfoilUnavailable | ProviderRequestError::Build(_) => {
            TranscriptionError::Unavailable
        }
        ProviderRequestError::Connect(_) | ProviderRequestError::Send(_) => {
            TranscriptionError::Upstream
        }
        ProviderRequestError::Upstream(error) => upstream_status(error.status),
    }
}

fn upstream_status(status: u16) -> TranscriptionError {
    match status {
        429 => TranscriptionError::RateLimited,
        503 | 529 => TranscriptionError::Unavailable,
        408 | 504 => TranscriptionError::Timeout,
        _ => TranscriptionError::Upstream,
    }
}

fn validate_response(
    bytes: &[u8],
    expected_seconds: Option<f64>,
    response_format: &str,
) -> Result<Value, TranscriptionError> {
    let response: Value =
        serde_json::from_slice(bytes).map_err(|_| TranscriptionError::InvalidResponse)?;
    let object = response
        .as_object()
        .ok_or(TranscriptionError::InvalidResponse)?;
    let text = object
        .get("text")
        .and_then(Value::as_str)
        .ok_or(TranscriptionError::InvalidResponse)?;
    let usage_seconds = match object.get("usage") {
        Some(value) => {
            let usage = value
                .as_object()
                .ok_or(TranscriptionError::InvalidResponse)?;
            if usage.get("type").and_then(Value::as_str) != Some("duration") {
                return Err(TranscriptionError::InvalidResponse);
            }
            let seconds = usage
                .get("seconds")
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite() && *value > 0.0)
                .ok_or(TranscriptionError::InvalidResponse)?;
            validate_duration(seconds, expected_seconds)?;
            Some(seconds)
        }
        None => None,
    };
    // The pinned Privatemode adapter accepts verbose_json with top-level
    // duration (including a numeric string) and no usage. Duration describes
    // the recording; it is not fabricated into provider-billed usage here.
    // See privatemode-public/internal/oss/openai/vllm.go's
    // TranscriptionUsageResponse.UnmarshalJSON.
    let verbose_seconds = if response_format == "verbose_json" {
        object
            .get("duration")
            .map(|value| {
                let seconds = value
                    .as_f64()
                    .or_else(|| value.as_str().and_then(|value| value.parse::<f64>().ok()))
                    .filter(|value| value.is_finite() && *value > 0.0)
                    .ok_or(TranscriptionError::InvalidResponse)?;
                validate_duration(seconds, expected_seconds)?;
                Ok::<_, TranscriptionError>(seconds)
            })
            .transpose()?
    } else {
        None
    };
    if usage_seconds.is_none() && verbose_seconds.is_none() {
        return Err(TranscriptionError::InvalidResponse);
    }
    if text.trim().is_empty() {
        return Err(TranscriptionError::NoSpeech);
    }
    Ok(response)
}

fn validate_duration(
    seconds: f64,
    expected_seconds: Option<f64>,
) -> Result<(), TranscriptionError> {
    // Opaque compressed inputs are bounded after the provider decodes them.
    // They must never escape the same product duration limit as local WAVs.
    if seconds > MAX_AUDIO_SECONDS {
        return Err(if expected_seconds.is_some() {
            TranscriptionError::InvalidResponse
        } else {
            TranscriptionError::AudioTooLong
        });
    }
    // Duration billing uses whole seconds. Accept either rounding direction,
    // but never treat a provider's short partial recording as a full success.
    if expected_seconds.is_some_and(|expected| (seconds - expected).abs() > 1.0) {
        return Err(TranscriptionError::InvalidResponse);
    }
    Ok(())
}

pub(crate) async fn execute(
    client: &ProviderClient,
    provider: &ProxyConfig,
    prepared: PreparedTranscription,
) -> Result<Value, TranscriptionError> {
    execute_with_deadline(client, provider, prepared, REQUEST_DEADLINE).await
}

async fn execute_with_deadline(
    client: &ProviderClient,
    provider: &ProxyConfig,
    prepared: PreparedTranscription,
    deadline: Duration,
) -> Result<Value, TranscriptionError> {
    // Fail closed if a caller accidentally passes a different provider. The
    // configured Continuum proxy retains the existing attestation boundary.
    if provider.provider_name != "continuum" {
        return Err(TranscriptionError::Unavailable);
    }
    tokio::time::timeout(deadline, async {
        let expected_seconds = prepared.audio_seconds;
        let response_format = prepared.response_format.clone();
        let (content_type, body) = multipart(prepared);
        let response = client
            .send(
                provider,
                ProviderRequest::new(Method::POST, "/v1/audio/transcriptions", deadline)
                    .content_type(&content_type)
                    .body(body),
            )
            .await
            .map_err(provider_error)?;
        if !response.is_success() {
            // Discard arbitrary provider error bodies without reading/logging
            // them. A timeout or upstream error never causes a second POST.
            return Err(upstream_status(response.status_code()));
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| TranscriptionError::Upstream)?;
            if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(bytes.len()) {
                return Err(TranscriptionError::ResponseTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        validate_response(&bytes, expected_seconds, &response_format)
    })
    .await
    .map_err(|_| TranscriptionError::Timeout)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, Bytes};
    use axum::extract::{Multipart, State};
    use axum::http::Response;
    use axum::routing::post;
    use axum::{Json, Router};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::task::JoinHandle;

    fn wav(sample_rate: u32, channels: u16, bits: u16, frames: usize) -> Vec<u8> {
        let block_align = channels * bits / 8;
        let len = frames * block_align as usize;
        let mut bytes = Vec::with_capacity(44 + len + len % 2);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&((36 + len + len % 2) as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * block_align as u32).to_le_bytes());
        bytes.extend_from_slice(&block_align.to_le_bytes());
        bytes.extend_from_slice(&bits.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(len as u32).to_le_bytes());
        bytes.resize(44 + len + len % 2, 0);
        if bits == 8 {
            bytes[44..44 + len].fill(128);
            bytes[44] = 129;
        } else {
            bytes[44] = 1;
        }
        bytes
    }

    fn silent_wav(sample_rate: u32, channels: u16, bits: u16, frames: usize) -> Vec<u8> {
        let mut audio = wav(sample_rate, channels, bits, frames);
        let data_len = frames * (channels * bits / 8) as usize;
        audio[44..44 + data_len].fill(if bits == 8 { 128 } else { 0 });
        audio
    }

    fn request(audio: &[u8]) -> TranscriptionRequest {
        TranscriptionRequest {
            file: general_purpose::STANDARD.encode(audio),
            filename: "recording.wav".to_owned(),
            content_type: "audio/wav".to_owned(),
            model: MODEL.to_owned(),
            language: None,
            prompt: None,
            response_format: "json".to_owned(),
            temperature: None,
        }
    }

    fn prepared() -> PreparedTranscription {
        prepare(request(&wav(16_000, 1, 16, 16_000))).unwrap_or_else(|error| panic!("{error}"))
    }

    #[test]
    fn validates_actual_frames_at_ten_and_twenty_minutes() {
        // Smallest supported PCM frame size keeps boundary tests economical.
        for seconds in [600, 1200] {
            let audio = wav(8_000, 1, 8, 8_000 * seconds);
            assert_eq!(wav_duration(&audio), Ok(seconds as f64));
        }
        assert_eq!(
            wav_duration(&wav(8_000, 1, 8, 8_000 * 1200 + 1)),
            Err(TranscriptionError::AudioTooLong)
        );
    }

    #[test]
    fn exact_integer_digital_silence_is_rejected_but_one_lsb_is_kept() {
        for bits in [8, 16, 24, 32] {
            for channels in [1, 2] {
                // Three PCM8 mono frames also exercise an odd data chunk
                // whose padding is zero rather than the unsigned midpoint.
                let mut audio = silent_wav(16_000, channels, bits, 3);
                assert!(matches!(
                    prepare(request(&audio)),
                    Err(TranscriptionError::NoSpeech)
                ));
                let last_sample = 44 + (3 * channels as usize - 1) * bits as usize / 8;
                audio[last_sample] = if bits == 8 { 129 } else { 1 };
                assert!(prepare(request(&audio)).is_ok());
            }
        }
        // PCM8 value zero is full negative amplitude, not digital silence.
        let mut unsigned = silent_wav(8_000, 1, 8, 3);
        unsigned[44] = 0;
        assert!(prepare(request(&unsigned)).is_ok());
    }

    #[test]
    fn exact_float_positive_and_negative_zero_are_silent() {
        for bits in [32, 64] {
            let mut audio = silent_wav(16_000, 2, bits, 2);
            audio[20..22].copy_from_slice(&3u16.to_le_bytes());
            let width = bits as usize / 8;
            audio[44 + width - 1] = 0x80; // One negative-zero sample.
            assert!(matches!(
                prepare(request(&audio)),
                Err(TranscriptionError::NoSpeech)
            ));
            // The smallest positive subnormal stays eligible; no VAD or
            // amplitude threshold can discard very quiet speech here.
            audio[44 + width] = 1;
            assert!(prepare(request(&audio)).is_ok());
        }
    }

    #[test]
    fn digital_silence_check_respects_validated_twenty_minute_boundary() {
        let mut audio = silent_wav(8_000, 1, 8, 8_000 * 1200);
        let metadata = validate_wav(&audio).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(metadata.duration_seconds, 1200.0);
        assert!(metadata.digital_silence);
        let last_sample = audio.len() - 1;
        audio[last_sample] = 129;
        assert!(
            !validate_wav(&audio)
                .unwrap_or_else(|error| panic!("{error}"))
                .digital_silence
        );
        assert_eq!(
            wav_duration(&silent_wav(8_000, 1, 8, 8_000 * 1200 + 1)),
            Err(TranscriptionError::AudioTooLong)
        );
    }

    #[test]
    fn rejects_truncation_fake_duration_and_incomplete_frames() {
        let audio = wav(16_000, 1, 16, 16_000);
        for length in [0, 12, 43, audio.len() - 1] {
            assert_eq!(
                wav_duration(&audio[..length]),
                Err(TranscriptionError::InvalidAudio)
            );
        }
        let mut fake_rate = audio.clone();
        fake_rate[28..32].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(
            wav_duration(&fake_rate),
            Err(TranscriptionError::UnsupportedAudio)
        );
        let mut odd_frame = wav(16_000, 1, 16, 1);
        odd_frame[40..44].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(
            wav_duration(&odd_frame),
            Err(TranscriptionError::InvalidAudio)
        );
        let mut huge_chunk = audio;
        huge_chunk[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            wav_duration(&huge_chunk),
            Err(TranscriptionError::InvalidAudio)
        );
    }

    #[test]
    fn accepts_metadata_chunks_but_rejects_ambiguous_wav_layouts() {
        let mut audio = wav(16_000, 1, 16, 100);
        audio.splice(36..36, *b"JUNK\x01\0\0\0x\0");
        let riff_size = (audio.len() - 8) as u32;
        audio[4..8].copy_from_slice(&riff_size.to_le_bytes());
        assert_eq!(wav_duration(&audio), Ok(100.0 / 16_000.0));
        let duplicate_fmt = audio[12..36].to_vec();
        audio.extend_from_slice(&duplicate_fmt);
        let riff_size = (audio.len() - 8) as u32;
        audio[4..8].copy_from_slice(&riff_size.to_le_bytes());
        assert_eq!(wav_duration(&audio), Err(TranscriptionError::InvalidAudio));
    }

    #[test]
    fn keeps_legacy_model_and_compressed_containers_without_trusting_duration() {
        let mut request = request(b"opaque compressed file");
        request.content_type = "audio/mpeg".to_owned();
        request.model = "whisper-large-v3-turbo".to_owned();
        let prepared = prepare(request).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(prepared.audio_seconds(), None);
        assert_eq!(prepared.file_size(), 22);
        let legacy: TranscriptionRequest = serde_json::from_value(json!({"file":"YWJj"})).unwrap();
        assert_eq!(legacy.content_type, "audio/mpeg");
        assert_eq!(legacy.response_format, "json");
    }

    #[test]
    fn validates_model_and_parameters_before_sending() {
        let mut invalid = request(&wav(16_000, 1, 16, 1));
        invalid.model = "arbitrary-upstream-model".to_owned();
        assert!(matches!(
            prepare(invalid),
            Err(TranscriptionError::UnsupportedModel)
        ));
        for language in ["english", "en\r\n", "../../", "EN"] {
            let mut invalid = request(&wav(16_000, 1, 16, 1));
            invalid.language = Some(language.to_owned());
            assert!(matches!(
                prepare(invalid),
                Err(TranscriptionError::InvalidParameters)
            ));
        }
        let mut invalid = request(&wav(16_000, 1, 16, 1));
        invalid.temperature = Some(f64::NAN);
        assert!(matches!(
            prepare(invalid),
            Err(TranscriptionError::InvalidParameters)
        ));
        let mut invalid = request(&wav(16_000, 1, 16, 1));
        invalid.response_format = "verbose_json".to_owned();
        assert!(matches!(
            prepare(invalid),
            Err(TranscriptionError::InvalidParameters)
        ));
        let mut disguised = request(&wav(16_000, 1, 16, 1));
        disguised.content_type = "audio/mpeg".to_owned();
        assert!(matches!(
            prepare(disguised),
            Err(TranscriptionError::InvalidAudio)
        ));
    }

    #[test]
    fn rejects_empty_invalid_base64_and_oversized_encoded_uploads() {
        let empty = request(&[]);
        assert!(matches!(
            prepare(empty),
            Err(TranscriptionError::InvalidAudio)
        ));
        let mut invalid = request(b"x");
        invalid.file = "this is not base64".to_owned();
        assert!(matches!(
            prepare(invalid),
            Err(TranscriptionError::InvalidAudio)
        ));
        // An invalid oversized encoded file must hit the size gate before the
        // decoder, rather than allocating according to attacker input.
        let mut oversized = request(b"x");
        oversized.file = "!".repeat(MAX_AUDIO_BYTES.div_ceil(3) * 4 + 1);
        assert!(matches!(
            prepare(oversized),
            Err(TranscriptionError::AudioTooLarge)
        ));
    }

    #[test]
    fn response_requires_text_and_actual_duration_usage() {
        for invalid in [
            json!({}),
            json!({"text": 7, "usage":{"type":"duration","seconds":1}}),
            json!({"text":"words"}),
            json!({"text":"words", "usage":{"type":"tokens","seconds":1}}),
            json!({"text":"words", "usage":{"type":"duration","seconds":0}}),
            json!({"text":"words", "usage":{"type":"duration","seconds":"1"}}),
        ] {
            assert_eq!(
                validate_response(invalid.to_string().as_bytes(), None, "json"),
                Err(TranscriptionError::InvalidResponse)
            );
        }
        assert_eq!(
            validate_response(b"{\"text\":", None, "json"),
            Err(TranscriptionError::InvalidResponse)
        );
        let empty = json!({"text":" \n ", "usage":{"type":"duration","seconds":1}});
        assert_eq!(
            validate_response(empty.to_string().as_bytes(), None, "json"),
            Err(TranscriptionError::NoSpeech)
        );
        let too_long = json!({"text":"words", "usage":{"type":"duration","seconds":1201}});
        assert_eq!(
            validate_response(too_long.to_string().as_bytes(), None, "json"),
            Err(TranscriptionError::AudioTooLong)
        );
        assert_eq!(
            validate_response(too_long.to_string().as_bytes(), Some(1.0), "json"),
            Err(TranscriptionError::InvalidResponse)
        );
    }

    #[test]
    fn rejects_partial_duration_even_when_transcript_is_nonempty() {
        let partial = json!({"text":"Beginning only.", "usage":{"type":"duration","seconds":30}});
        assert_eq!(
            validate_response(partial.to_string().as_bytes(), Some(600.0), "json"),
            Err(TranscriptionError::InvalidResponse)
        );
        for billed in [600, 601] {
            let complete = json!({"text":"Beginning, middle, end.", "usage":{"type":"duration","seconds":billed}});
            assert!(
                validate_response(complete.to_string().as_bytes(), Some(600.5), "json").is_ok()
            );
        }
    }

    #[test]
    fn verbose_duration_without_usage_is_validated_without_inventing_usage() {
        for duration in [json!("600.5"), json!(600.5)] {
            let verbose =
                json!({"text":"Beginning, middle, end.", "duration":duration, "segments":[]});
            let response =
                validate_response(verbose.to_string().as_bytes(), Some(600.5), "verbose_json")
                    .unwrap();
            assert_eq!(response, verbose);
            assert!(response.get("usage").is_none());
            assert_eq!(
                validate_response(verbose.to_string().as_bytes(), Some(600.5), "json"),
                Err(TranscriptionError::InvalidResponse)
            );
        }
        for duration in [
            json!("NaN"),
            json!("inf"),
            json!("bad"),
            json!(0),
            json!(-1),
            json!(true),
        ] {
            let invalid = json!({"text":"words", "duration":duration});
            assert_eq!(
                validate_response(invalid.to_string().as_bytes(), None, "verbose_json"),
                Err(TranscriptionError::InvalidResponse)
            );
        }
        let invalid_usage =
            json!({"text":"words", "duration":"1.0", "usage":{"type":"duration","seconds":"1"}});
        assert_eq!(
            validate_response(
                invalid_usage.to_string().as_bytes(),
                Some(1.0),
                "verbose_json"
            ),
            Err(TranscriptionError::InvalidResponse)
        );
        let partial = json!({"text":"Beginning only.", "duration":"30"});
        assert_eq!(
            validate_response(partial.to_string().as_bytes(), Some(600.0), "verbose_json"),
            Err(TranscriptionError::InvalidResponse)
        );
    }

    async fn server(router: Router) -> (ProviderClient, ProxyConfig, JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let client = ProviderClient::for_test(base_url.clone()).unwrap();
        let provider = ProxyConfig {
            base_url,
            api_key: None,
            provider_name: "continuum".to_owned(),
        };
        (client, provider, task)
    }

    #[tokio::test]
    async fn sends_one_whole_recording_with_fixed_model_and_safe_multipart() {
        async fn transcribe(mut multipart: Multipart) -> Json<Value> {
            let mut fields = std::collections::HashMap::new();
            while let Some(field) = multipart.next_field().await.unwrap() {
                let name = field.name().unwrap().to_owned();
                if name == "file" {
                    assert_eq!(field.file_name(), Some("audio.wav"));
                    assert_eq!(field.content_type(), Some("audio/wav"));
                    assert_eq!(
                        field.bytes().await.unwrap().as_ref(),
                        wav(16_000, 1, 16, 16_000)
                    );
                } else {
                    fields.insert(name, field.text().await.unwrap());
                }
            }
            assert_eq!(fields["model"], MODEL);
            assert_eq!(fields["prompt"], "Names: Maple.\nKeep punctuation.");
            assert_eq!(fields["response_format"], "json");
            assert!(!fields.contains_key("language"));
            Json(
                json!({"text":"Beginning, middle, and end.", "usage":{"type":"duration","seconds":1}}),
            )
        }
        let (client, provider, task) =
            server(Router::new().route("/v1/audio/transcriptions", post(transcribe))).await;
        let mut request = request(&wav(16_000, 1, 16, 16_000));
        request.filename = "evil\"\r\nX-Injected: yes.wav".to_owned();
        request.prompt = Some("Names: Maple.\nKeep punctuation.".to_owned());
        request.language = Some("auto".to_owned());
        let response = execute(
            &client,
            &provider,
            prepare(request).unwrap_or_else(|e| panic!("{e}")),
        )
        .await
        .unwrap();
        assert_eq!(response["text"], "Beginning, middle, and end.");
        task.abort();
    }

    #[tokio::test]
    async fn bounds_stalled_response_body_and_never_replays_post() {
        async fn stalled(State(calls): State<Arc<AtomicUsize>>) -> Response<Body> {
            calls.fetch_add(1, Ordering::SeqCst);
            let body = futures::stream::once(async {
                Ok::<_, std::io::Error>(Bytes::from_static(b"{\"text\":"))
            })
            .chain(futures::stream::pending());
            Response::new(Body::from_stream(body))
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let router = Router::new()
            .route("/v1/audio/transcriptions", post(stalled))
            .with_state(Arc::clone(&calls));
        let (client, provider, task) = server(router).await;
        let response =
            execute_with_deadline(&client, &provider, prepared(), Duration::from_millis(100)).await;
        assert_eq!(response, Err(TranscriptionError::Timeout));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        task.abort();
    }

    #[tokio::test]
    async fn bounds_response_and_sanitizes_provider_failures_without_retry() {
        async fn oversized() -> String {
            "x".repeat(MAX_RESPONSE_BYTES + 1)
        }
        let (client, provider, task) =
            server(Router::new().route("/v1/audio/transcriptions", post(oversized))).await;
        assert_eq!(
            execute(&client, &provider, prepared()).await,
            Err(TranscriptionError::ResponseTooLarge)
        );
        task.abort();

        async fn unavailable(State(calls): State<Arc<AtomicUsize>>) -> (StatusCode, &'static str) {
            calls.fetch_add(1, Ordering::SeqCst);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "secret upstream body must never escape",
            )
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let router = Router::new()
            .route("/v1/audio/transcriptions", post(unavailable))
            .with_state(Arc::clone(&calls));
        let (client, provider, task) = server(router).await;
        assert_eq!(
            execute(&client, &provider, prepared()).await,
            Err(TranscriptionError::Unavailable)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        task.abort();
    }
}
