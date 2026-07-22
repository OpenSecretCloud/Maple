use futures_util::StreamExt;
use once_cell::sync::{Lazy, OnceCell};
use pdf_oxide::ocr::{OcrConfig, OcrEngine};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const OCR_MODEL_PACK_VERSION: &str = "paddleocr-en-v1";
const TOTAL_MODEL_SIZE: u64 = 12_577_821;

struct ModelFile {
    name: &'static str,
    url: &'static str,
    size: u64,
    sha256: &'static str,
}

// These immutable revisions are intentionally independent from PDFOxide's
// mutable `resolve/main` model downloader. See docs/pdf-ocr.md.
const MODEL_FILES: &[ModelFile] = &[
    ModelFile {
        name: "det.onnx",
        url: "https://huggingface.co/SWHL/RapidOCR/resolve/1cfba2e90fc938db55889873735088de210cc173/PP-OCRv4/ch_PP-OCRv4_det_infer.onnx",
        size: 4_745_517,
        sha256: "d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9",
    },
    ModelFile {
        name: "rec.onnx",
        url: "https://huggingface.co/monkt/paddleocr-onnx/resolve/7b02d0a30a07ba2b92ad1ff5a8941ae2c633de65/languages/english/rec.onnx",
        size: 7_830_888,
        sha256: "4e16deb22c4da6468bdca539b2cd3c8687825538b67109177c47d359ab994cd7",
    },
    ModelFile {
        name: "en_dict.txt",
        url: "https://huggingface.co/monkt/paddleocr-onnx/resolve/7b02d0a30a07ba2b92ad1ff5a8941ae2c633de65/languages/english/dict.txt",
        size: 1_416,
        sha256: "e025a66d31f327ba0c232e03f407ae8d105e1e709e7ccb3f408aa778c24e70d6",
    },
];

static OCR_ENGINE: OnceCell<Arc<OcrEngine>> = OnceCell::new();
static OCR_SETUP_LOCK: Lazy<tokio::sync::Mutex<()>> = Lazy::new(|| tokio::sync::Mutex::new(()));

#[derive(Clone, Serialize)]
struct OcrDownloadProgress {
    downloaded: u64,
    total: u64,
    file_name: String,
    percent: f64,
}

pub async fn get_or_prepare_engine(app: &AppHandle) -> Result<Arc<OcrEngine>, String> {
    if let Some(engine) = OCR_ENGINE.get() {
        return Ok(engine.clone());
    }

    let _setup_guard = OCR_SETUP_LOCK.lock().await;
    if let Some(engine) = OCR_ENGINE.get() {
        return Ok(engine.clone());
    }

    let models_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("Failed to locate Maple's OCR cache: {e}"))?
        .join("ocr")
        .join("models")
        .join(OCR_MODEL_PACK_VERSION);

    ensure_models(app, &models_dir).await.map_err(|e| {
        log::error!("OCR model setup failed: {e}");
        format!(
            "Maple couldn't download its on-device OCR models. Check your connection and try the PDF again. ({e})"
        )
    })?;

    crate::onnxruntime::ensure_initialized().map_err(|error| {
        log::error!("ONNX Runtime setup failed before OCR initialization: {error}");
        format!("Maple couldn't start its on-device OCR engine. Try the PDF again. ({error})")
    })?;

    let det_path = models_dir.join("det.onnx");
    let rec_path = models_dir.join("rec.onnx");
    let dict_path = models_dir.join("en_dict.txt");
    let engine = tokio::task::spawn_blocking(move || {
        OcrEngine::new(det_path, rec_path, dict_path, OcrConfig::default())
            .map(Arc::new)
            .map_err(|e| format!("Failed to initialize the on-device OCR engine: {e}"))
    })
    .await
    .map_err(|e| format!("The on-device OCR engine stopped unexpectedly: {e}"))??;

    // Another task cannot race this initialization because OCR_SETUP_LOCK is held.
    let _ = OCR_ENGINE.set(engine.clone());
    Ok(engine)
}

async fn ensure_models(app: &AppHandle, models_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(models_dir)
        .map_err(|e| format!("Failed to create the OCR model cache: {e}"))?;

    let client = configure_download_tls(reqwest::Client::builder())?
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| format!("Failed to create the OCR download client: {e}"))?;

    let mut completed = 0;
    for model in MODEL_FILES {
        let path = models_dir.join(model.name);
        if verify_model_file(&path, model)? {
            completed += model.size;
            emit_progress(app, model.name, completed);
            continue;
        }

        if path.exists() {
            fs::remove_file(&path)
                .map_err(|e| format!("Failed to replace invalid {}: {e}", model.name))?;
        }

        download_model(app, &client, models_dir, model, completed).await?;
        completed += model.size;
    }

    Ok(())
}

#[cfg(target_os = "android")]
fn configure_download_tls(
    builder: reqwest::ClientBuilder,
) -> Result<reqwest::ClientBuilder, String> {
    // Reqwest's platform verifier needs separate Kotlin/JNI initialization on
    // Android. This download-only client instead uses the standard Mozilla root
    // set already supported by rustls, keeping the mobile integration in Rust.
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("Failed to configure Android OCR download TLS: {e}"))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(builder.tls_backend_preconfigured(tls))
}

#[cfg(not(target_os = "android"))]
fn configure_download_tls(
    builder: reqwest::ClientBuilder,
) -> Result<reqwest::ClientBuilder, String> {
    Ok(builder)
}

async fn download_model(
    app: &AppHandle,
    client: &reqwest::Client,
    models_dir: &Path,
    model: &ModelFile,
    already_downloaded: u64,
) -> Result<(), String> {
    let final_path = models_dir.join(model.name);
    let temp_path = models_dir.join(format!("{}.part", model.name));
    let _ = fs::remove_file(&temp_path);

    log::info!("Downloading pinned OCR model {}", model.name);
    let result = async {
        let response = client
            .get(model.url)
            .send()
            .await
            .map_err(|e| format!("Failed to download {}: {e}", model.name))?;
        if !response.status().is_success() {
            return Err(format!(
                "Failed to download {}: HTTP {}",
                model.name,
                response.status()
            ));
        }
        if let Some(length) = response.content_length() {
            if length != model.size {
                return Err(format!(
                    "Unexpected Content-Length for {}: expected {}, got {length}",
                    model.name, model.size
                ));
            }
        }

        let mut file = File::create(&temp_path)
            .map_err(|e| format!("Failed to create {}: {e}", model.name))?;
        let mut stream = response.bytes_stream();
        let mut downloaded = 0_u64;
        let mut hasher = Sha256::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("Download failed for {}: {e}", model.name))?;
            downloaded = downloaded
                .checked_add(chunk.len() as u64)
                .ok_or_else(|| format!("Download size overflow for {}", model.name))?;
            if downloaded > model.size {
                return Err(format!(
                    "Download for {} exceeded its pinned size",
                    model.name
                ));
            }
            file.write_all(&chunk)
                .map_err(|e| format!("Failed to write {}: {e}", model.name))?;
            hasher.update(&chunk);
            emit_progress(app, model.name, already_downloaded + downloaded);
        }

        if downloaded != model.size {
            return Err(format!(
                "Incomplete download for {}: expected {}, got {downloaded}",
                model.name, model.size
            ));
        }
        let actual_hash = format!("{:x}", hasher.finalize());
        if actual_hash != model.sha256 {
            return Err(format!("Checksum verification failed for {}", model.name));
        }

        file.flush()
            .map_err(|e| format!("Failed to flush {}: {e}", model.name))?;
        file.sync_all()
            .map_err(|e| format!("Failed to sync {}: {e}", model.name))?;
        drop(file);
        fs::rename(&temp_path, &final_path)
            .map_err(|e| format!("Failed to finalize {}: {e}", model.name))?;
        Ok(())
    }
    .await;

    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn verify_model_file(path: &Path, model: &ModelFile) -> Result<bool, String> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!(
                "Failed to inspect cached model {}: {error}",
                model.name
            ));
        }
    };
    if !metadata.is_file() || metadata.len() != model.size {
        return Ok(false);
    }

    let actual_hash = sha256_file(path)
        .map_err(|e| format!("Failed to verify cached model {}: {e}", model.name))?;
    Ok(actual_hash == model.sha256)
}

fn sha256_file(path: &Path) -> Result<String, std::io::Error> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn emit_progress(app: &AppHandle, file_name: &str, downloaded: u64) {
    let downloaded = downloaded.min(TOTAL_MODEL_SIZE);
    let _ = app.emit(
        "ocr-download-progress",
        OcrDownloadProgress {
            downloaded,
            total: TOTAL_MODEL_SIZE,
            file_name: file_name.to_string(),
            percent: downloaded as f64 / TOTAL_MODEL_SIZE as f64 * 100.0,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::{sha256_file, verify_model_file, ModelFile};
    use std::fs;

    #[test]
    fn model_verification_checks_size_and_sha256() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("tiny-model");
        fs::write(&path, b"maple").expect("write fixture");
        let model = ModelFile {
            name: "tiny-model",
            url: "https://example.invalid/tiny-model",
            size: 5,
            sha256: "49ead2b1066bda1e127f6ae0bd163778d08587e3e97d9ed58e8cc99972460a1c",
        };

        assert!(verify_model_file(&path, &model).expect("verify model"));

        fs::write(&path, b"Maple").expect("replace fixture");
        assert!(!verify_model_file(&path, &model).expect("reject bad hash"));
    }

    #[test]
    fn sha256_file_uses_lowercase_hex() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("hash-input");
        fs::write(&path, b"maple").expect("write fixture");

        assert_eq!(
            sha256_file(&path).expect("hash fixture"),
            "49ead2b1066bda1e127f6ae0bd163778d08587e3e97d9ed58e8cc99972460a1c"
        );
    }
}
