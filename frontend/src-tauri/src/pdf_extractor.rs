use crate::pdf_ocr;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use once_cell::sync::Lazy;
use pdf_oxide::extractors::auto::PageKind;
use pdf_oxide::ocr::OcrEngine;
use pdf_oxide::rendering::{self, RenderOptions};
use pdf_oxide::PdfDocument;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::Semaphore;

const MAX_DOCUMENT_BYTES: usize = 10 * 1024 * 1024;
const MAX_EXTRACTED_TEXT_BYTES: usize = MAX_DOCUMENT_BYTES;
const OCR_RENDER_MAX_DIMENSION: u32 = 2_000;
const MAX_OCR_SOURCE_IMAGE_PIXELS: u64 = 24_000_000;
const MAX_OCR_PAGE_SOURCE_PIXELS: u64 = 64_000_000;
const MAX_OCR_PAGE_IMAGES: usize = 256;
const PDF_PANIC_MESSAGE: &str = "Maple couldn't process this PDF because its parser stopped unexpectedly. The app is still running; try a different PDF.";
static PDF_JOB_SEMAPHORE: Lazy<Semaphore> = Lazy::new(|| Semaphore::new(1));

#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentData {
    pub filename: String,
    pub text_content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentResponse {
    pub document: DocumentData,
    pub status: String,
}

enum PdfPreflight {
    Complete(String),
    NeedsOcr {
        native_fallback: String,
        has_scanned_pages: bool,
    },
}

#[tauri::command]
pub async fn extract_document_content(
    app: AppHandle,
    file_base64: String,
    filename: String,
    file_type: String,
) -> Result<DocumentResponse, String> {
    extract_document_content_impl(Some(&app), file_base64, filename, file_type).await
}

async fn extract_document_content_impl(
    app: Option<&AppHandle>,
    file_base64: String,
    filename: String,
    file_type: String,
) -> Result<DocumentResponse, String> {
    // Reject obviously oversized base64 before allocating the decoded buffer.
    let max_encoded_size = MAX_DOCUMENT_BYTES.div_ceil(3) * 4 + 4;
    if file_base64.len() > max_encoded_size {
        return Err("Document too large (max 10MB)".to_string());
    }

    let file_bytes = BASE64
        .decode(&file_base64)
        .map_err(|e| format!("Failed to decode base64 file: {e}"))?;
    if file_bytes.len() > MAX_DOCUMENT_BYTES {
        return Err("Document too large (max 10MB)".to_string());
    }

    let text_content = match file_type.to_ascii_lowercase().as_str() {
        "pdf" | "application/pdf" => extract_pdf(app, file_bytes).await?,
        "txt" | "text/plain" | "md" | "text/markdown" => {
            String::from_utf8(file_bytes).map_err(|e| format!("Failed to decode text file: {e}"))?
        }
        _ => return Err(format!("Unsupported file type: {file_type}")),
    };

    Ok(DocumentResponse {
        document: DocumentData {
            filename,
            text_content,
        },
        status: "completed".to_string(),
    })
}

async fn extract_pdf(app: Option<&AppHandle>, file_bytes: Vec<u8>) -> Result<String, String> {
    // PDF rendering and OCR can each use substantial memory. Serializing these
    // user-initiated jobs keeps concurrent invokes from multiplying that peak,
    // particularly on iOS and Android.
    let _job_permit = PDF_JOB_SEMAPHORE
        .acquire()
        .await
        .map_err(|_| "Maple's PDF processor is unavailable. Please try again.".to_string())?;

    let preflight_bytes = file_bytes.clone();
    match run_pdf_job(move || extract_native_or_request_ocr(preflight_bytes)).await? {
        PdfPreflight::Complete(text) => ensure_document_has_text(text),
        PdfPreflight::NeedsOcr {
            native_fallback,
            has_scanned_pages,
        } => {
            let Some(app) = app else {
                if has_scanned_pages {
                    return Err("This scanned PDF needs Maple's on-device OCR models.".to_string());
                }
                return ensure_document_has_text(native_fallback);
            };
            let engine = match pdf_ocr::get_or_prepare_engine(app).await {
                Ok(engine) => engine,
                Err(error) if !has_scanned_pages => {
                    log::warn!(
                        "Optional PDF OCR enrichment is unavailable; using native text: {error}"
                    );
                    return ensure_document_has_text(native_fallback);
                }
                Err(error) => return Err(error),
            };
            match run_pdf_job(move || extract_pdf_with_ocr(file_bytes, engine)).await {
                Ok(text) => ensure_document_has_text(text),
                Err(error) if !has_scanned_pages => {
                    log::warn!("Optional PDF OCR enrichment failed; using native text: {error}");
                    ensure_document_has_text(native_fallback)
                }
                Err(error) => Err(error),
            }
        }
    }
}

async fn run_pdf_job<T, F>(operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    match tokio::task::spawn_blocking(operation).await {
        Ok(result) => result,
        Err(error) if error.is_panic() => {
            log::error!("PDF processing panicked inside its isolated worker: {error}");
            Err(PDF_PANIC_MESSAGE.to_string())
        }
        Err(error) => {
            log::error!("PDF processing worker could not complete: {error}");
            Err("Maple couldn't finish processing this PDF. Please try again.".to_string())
        }
    }
}

fn open_pdf(file_bytes: Vec<u8>) -> Result<PdfDocument, String> {
    let document = PdfDocument::from_bytes(file_bytes)
        .map_err(|e| format!("Maple couldn't read this PDF: {e}"))?;
    if document.is_encrypted() && !document.is_authenticated() {
        return Err(
            "This PDF is password-protected. Maple cannot read protected PDFs yet.".to_string(),
        );
    }
    Ok(document)
}

fn extract_native_or_request_ocr(file_bytes: Vec<u8>) -> Result<PdfPreflight, String> {
    let document = open_pdf(file_bytes)?;
    let page_count = document
        .page_count()
        .map_err(|e| format!("Maple couldn't read the PDF's page list: {e}"))?;

    let mut pages = Vec::with_capacity(page_count);
    let mut extracted_bytes = 0;
    let mut needs_ocr = false;
    let mut has_scanned_pages = false;
    for page in 0..page_count {
        let classification = document
            .classify_page(page)
            .map_err(|e| format!("Maple couldn't inspect PDF page {}: {e}", page + 1))?;
        let text = match classification.kind {
            PageKind::TextLayer => document
                .extract_text(page)
                .map_err(|e| format!("Maple couldn't extract PDF page {}: {e}", page + 1))?,
            PageKind::Empty => String::new(),
            PageKind::Scanned => {
                needs_ocr = true;
                has_scanned_pages = true;
                extract_native_best_effort(&document, page)
            }
            PageKind::ImageText | PageKind::Mixed => {
                needs_ocr = true;
                extract_native_best_effort(&document, page)
            }
            _ => {
                return Err(format!(
                    "PDF page {} uses a page type this Maple version does not support.",
                    page + 1
                ));
            }
        };
        push_page_with_budget(&mut pages, &mut extracted_bytes, text)?;
    }

    let native_text = join_pages(pages);
    if needs_ocr {
        Ok(PdfPreflight::NeedsOcr {
            native_fallback: native_text,
            has_scanned_pages,
        })
    } else {
        Ok(PdfPreflight::Complete(native_text))
    }
}

fn extract_pdf_with_ocr(file_bytes: Vec<u8>, engine: Arc<OcrEngine>) -> Result<String, String> {
    let document = open_pdf(file_bytes)?;
    let page_count = document
        .page_count()
        .map_err(|e| format!("Maple couldn't read the PDF's page list: {e}"))?;
    let mut pages = Vec::with_capacity(page_count);
    let mut extracted_bytes = 0;

    for page in 0..page_count {
        let classification = document
            .classify_page(page)
            .map_err(|e| format!("Maple couldn't inspect PDF page {}: {e}", page + 1))?;
        let text = match classification.kind {
            PageKind::TextLayer => document
                .extract_text(page)
                .map_err(|e| format!("Maple couldn't extract PDF page {}: {e}", page + 1))?,
            PageKind::Empty => String::new(),
            PageKind::Scanned => {
                let native = extract_native_best_effort(&document, page);
                match ocr_page(&document, page, &engine) {
                    Ok(fragments) => merge_native_and_ocr(&native, &fragments),
                    Err(error) => {
                        log::warn!("Required OCR failed on PDF page {}: {error}", page + 1);
                        return Err(error);
                    }
                }
            }
            PageKind::ImageText | PageKind::Mixed => {
                let native_result = document
                    .extract_text(page)
                    .map_err(|e| format!("Maple couldn't extract PDF page {}: {e}", page + 1));
                let ocr_result = ocr_page(&document, page, &engine);
                match (native_result, ocr_result) {
                    (Ok(native), Ok(fragments)) => merge_native_and_ocr(&native, &fragments),
                    (Ok(native), Err(error)) => {
                        log::warn!(
                            "Optional OCR enrichment failed on PDF page {}: {error}",
                            page + 1
                        );
                        native
                    }
                    (Err(native_error), Ok(fragments)) if !fragments.is_empty() => {
                        log::warn!(
                            "Native extraction failed on PDF page {}; using OCR: {native_error}",
                            page + 1
                        );
                        merge_native_and_ocr("", &fragments)
                    }
                    (Err(native_error), Ok(_)) => return Err(native_error),
                    (Err(native_error), Err(ocr_error)) => {
                        return Err(format!(
                            "{native_error}; on-device OCR also failed: {ocr_error}"
                        ));
                    }
                }
            }
            _ => {
                return Err(format!(
                    "PDF page {} uses a page type this Maple version does not support.",
                    page + 1
                ));
            }
        };
        push_page_with_budget(&mut pages, &mut extracted_bytes, text)?;
    }

    Ok(join_pages(pages))
}

fn ocr_page(
    document: &PdfDocument,
    page: usize,
    engine: &OcrEngine,
) -> Result<Vec<String>, String> {
    validate_ocr_page_resources(document, page)?;

    // Always OCR one bounded rendering of the complete page. This captures
    // tiled scans, inline images, vector text, and multiple image regions
    // without PDFOxide's eager all-image decode or largest-image truncation.
    let rendered = rendering::render_page_fit(
        document,
        page,
        OCR_RENDER_MAX_DIMENSION,
        OCR_RENDER_MAX_DIMENSION,
        &RenderOptions::default().as_raw(),
    )
    .map_err(|e| format!("Maple couldn't render PDF page {} for OCR: {e}", page + 1))?;
    let rgba = image::RgbaImage::from_raw(rendered.width, rendered.height, rendered.data)
        .ok_or_else(|| format!("Maple couldn't decode PDF page {} for OCR.", page + 1))?;
    let image = image::DynamicImage::ImageRgba8(rgba);
    let output = engine
        .ocr_image(&image)
        .map_err(|e| format!("On-device OCR failed on PDF page {}: {e}", page + 1))?;

    let mut spans = output.spans;
    spans.sort_by(|left, right| {
        const Y_BAND: f32 = 10.0;
        let left_band = (left.polygon[0][1] / Y_BAND).round() as i64;
        let right_band = (right.polygon[0][1] / Y_BAND).round() as i64;
        left_band
            .cmp(&right_band)
            .then_with(|| left.polygon[0][0].total_cmp(&right.polygon[0][0]))
            .then_with(|| left.polygon[0][1].total_cmp(&right.polygon[0][1]))
    });
    Ok(spans
        .into_iter()
        .map(|span| span.text.trim().to_string())
        .filter(|text| !text.is_empty())
        .collect())
}

fn validate_ocr_page_resources(document: &PdfDocument, page: usize) -> Result<(), String> {
    let handles = document.page_image_handles(page).map_err(|e| {
        format!(
            "Maple couldn't inspect images on PDF page {}: {e}",
            page + 1
        )
    })?;
    validate_ocr_source_budget(
        page,
        handles
            .iter()
            .map(|image| (image.width, image.height, image.byte_size_compressed)),
    )
}

fn validate_ocr_source_budget(
    page: usize,
    images: impl IntoIterator<Item = (u32, u32, u64)>,
) -> Result<(), String> {
    let mut count = 0_usize;
    let mut total_pixels = 0_u64;
    for (width, height, compressed_bytes) in images {
        count += 1;
        let pixels = u64::from(width)
            .checked_mul(u64::from(height))
            .ok_or_else(|| safe_ocr_complexity_error(page))?;
        total_pixels = total_pixels
            .checked_add(pixels)
            .ok_or_else(|| safe_ocr_complexity_error(page))?;
        if count > MAX_OCR_PAGE_IMAGES
            || pixels > MAX_OCR_SOURCE_IMAGE_PIXELS
            || total_pixels > MAX_OCR_PAGE_SOURCE_PIXELS
            || compressed_bytes > MAX_DOCUMENT_BYTES as u64
        {
            return Err(safe_ocr_complexity_error(page));
        }
    }
    Ok(())
}

fn safe_ocr_complexity_error(page: usize) -> String {
    format!(
        "PDF page {} is too complex for Maple to OCR safely. Try a lower-resolution PDF.",
        page + 1
    )
}

fn extract_native_best_effort(document: &PdfDocument, page: usize) -> String {
    document.extract_text(page).unwrap_or_else(|error| {
        log::warn!(
            "Native text extraction failed on OCR-routed PDF page {}: {error}",
            page + 1
        );
        String::new()
    })
}

// Compare individual OCR detection spans, rather than the engine's single
// space-joined page string, so one novel image caption does not duplicate an
// otherwise native-readable page.
fn merge_native_and_ocr(native: &str, ocr_fragments: &[String]) -> String {
    let native_trimmed = native.trim_end();
    if ocr_fragments.is_empty() {
        return native.to_string();
    }
    if native_trimmed.trim().is_empty() {
        return ocr_fragments.join("\n");
    }

    let normalize = |value: &str| {
        value
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let native_normalized = normalize(native_trimmed);
    let mut extras: Vec<&str> = Vec::new();
    for fragment in ocr_fragments {
        let fragment = fragment.trim();
        let normalized = normalize(fragment);
        if normalized.is_empty()
            || native_normalized.contains(&normalized)
            || extras.iter().any(|extra| normalize(extra) == normalized)
        {
            continue;
        }
        extras.push(fragment);
    }

    if extras.is_empty() {
        native.to_string()
    } else {
        format!("{native_trimmed}\n{}", extras.join("\n"))
    }
}

fn push_page_with_budget(
    pages: &mut Vec<String>,
    extracted_bytes: &mut usize,
    text: String,
) -> Result<(), String> {
    *extracted_bytes = extracted_bytes
        .checked_add(text.len().saturating_add(2))
        .ok_or_else(extracted_text_too_large)?;
    if *extracted_bytes > MAX_EXTRACTED_TEXT_BYTES {
        return Err(extracted_text_too_large());
    }
    pages.push(text);
    Ok(())
}

fn extracted_text_too_large() -> String {
    "The extracted text from this PDF is too large for Maple (max 10MB).".to_string()
}

fn join_pages(pages: Vec<String>) -> String {
    pages
        .into_iter()
        .map(|page| page.trim().to_string())
        .collect::<Vec<_>>()
        .join("\n\n")
        .trim()
        .to_string()
}

fn ensure_document_has_text(text: String) -> Result<String, String> {
    if text.trim().is_empty() {
        Err("This PDF does not contain text Maple can read.".to_string())
    } else {
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        extract_document_content_impl, extract_pdf_with_ocr, merge_native_and_ocr, run_pdf_job,
        validate_ocr_source_budget, PDF_PANIC_MESSAGE,
    };
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
    use pdf_oxide::ocr::{OcrConfig, OcrEngine};
    use std::path::PathBuf;
    use std::sync::Arc;

    #[tokio::test]
    async fn extract_document_content_text_plain_success() {
        let file_base64 = BASE64.encode(b"Hello, Maple!");

        let resp = extract_document_content_impl(
            None,
            file_base64,
            "hello.txt".to_string(),
            "text/plain".to_string(),
        )
        .await
        .expect("expected text/plain extraction to succeed");

        assert_eq!(resp.status, "completed");
        assert_eq!(resp.document.filename, "hello.txt");
        assert_eq!(resp.document.text_content, "Hello, Maple!");
    }

    #[tokio::test]
    async fn extracts_native_pdf_without_ocr_models() {
        let file_base64 = BASE64.encode(native_text_pdf("MAPLE NATIVE PDF"));

        let resp = extract_document_content_impl(
            None,
            file_base64,
            "native.pdf".to_string(),
            "application/pdf".to_string(),
        )
        .await
        .expect("native PDF should not require OCR models");

        assert!(resp.document.text_content.contains("MAPLE NATIVE PDF"));
    }

    #[tokio::test]
    async fn native_readable_hybrid_pdf_does_not_require_ocr_models() {
        let pdf = native_text_and_image_pdf("MAPLE HYBRID PDF KEEPS ITS NATIVE TEXT OFFLINE");
        let document = pdf_oxide::PdfDocument::from_bytes(pdf.clone()).expect("open hybrid PDF");
        let classification = document.classify_page(0).expect("classify hybrid PDF");
        assert!(matches!(
            classification.kind,
            pdf_oxide::extractors::auto::PageKind::ImageText
                | pdf_oxide::extractors::auto::PageKind::Mixed
        ));

        let response = extract_document_content_impl(
            None,
            BASE64.encode(pdf),
            "hybrid.pdf".to_string(),
            "application/pdf".to_string(),
        )
        .await
        .expect("native-readable hybrid PDF should work without OCR models");

        assert!(response.document.text_content.contains("MAPLE HYBRID PDF"));
    }

    #[tokio::test]
    async fn invalid_pdf_returns_a_normal_error() {
        let error = extract_document_content_impl(
            None,
            BASE64.encode(b"not a PDF"),
            "invalid.pdf".to_string(),
            "pdf".to_string(),
        )
        .await
        .expect_err("invalid PDF should fail");

        assert!(error.contains("couldn't read this PDF"), "{error}");
    }

    #[tokio::test]
    async fn type4_tint_transform_reproducer_returns_without_a_parser_panic() {
        // Minimal public reproducer for the pdf-extract FunctionType 4 crash
        // that prompted this migration. It contains no text, so a normal
        // empty-document error is the expected result.
        const TYPE4_PDF: &str = "JVBERi0xLjcKMSAwIG9iago8PC9UeXBlL1BhZ2VzL0tpZHNbNCAwIFJdL0NvdW50IDE+PgplbmRvYmoKMiAwIG9iago8PC9GdW5jdGlvblR5cGUgNC9Eb21haW5bMCAxXS9SYW5nZVswIDEgMCAxIDAgMV0vTGVuZ3RoIDExPj5zdHJlYW0KeyBkdXAgZHVwIH0KZW5kc3RyZWFtIAplbmRvYmoKMyAwIG9iago8PC9MZW5ndGggOD4+c3RyZWFtCi9DUzEgY3MKCmVuZHN0cmVhbSAKZW5kb2JqCjQgMCBvYmoKPDwvVHlwZS9QYWdlL1BhcmVudCAxIDAgUi9NZWRpYUJveFswIDAgMjAwIDIwMF0vUmVzb3VyY2VzPDwvQ29sb3JTcGFjZTw8L0NTMVsvU2VwYXJhdGlvbi9TcG90L0RldmljZVJHQiAyIDAgUl0+Pj4+L0NvbnRlbnRzIDMgMCBSPj4KZW5kb2JqCjUgMCBvYmoKPDwvVHlwZS9DYXRhbG9nL1BhZ2VzIDEgMCBSPj4KZW5kb2JqCjYgMCBvYmoKPDwvUm9vdCA1IDAgUi9UeXBlL1hSZWYvU2l6ZSA3L1dbMSA0IDJdL0luZGV4WzEgNl0vTGVuZ3RoIDQyPj5zdHJlYW0KAQAAAAkAAAEAAAA8AAABAAAApQAAAQAAANwAAAEAAAFvAAABAAABnAAACmVuZHN0cmVhbSAKZW5kb2JqCgpzdGFydHhyZWYKNDEyCiUlRU9G";

        let error = extract_document_content_impl(
            None,
            TYPE4_PDF.to_string(),
            "type4.pdf".to_string(),
            "application/pdf".to_string(),
        )
        .await
        .expect_err("text-free reproducer should return a normal error");

        assert_ne!(error, PDF_PANIC_MESSAGE);
    }

    #[tokio::test]
    async fn parser_panic_is_contained_and_a_later_job_still_runs() {
        let panic_error = run_pdf_job::<(), _>(|| panic!("synthetic parser panic"))
            .await
            .expect_err("panic should become an ordinary error");
        assert_eq!(panic_error, PDF_PANIC_MESSAGE);

        let result = run_pdf_job(|| Ok::<_, String>("worker recovered".to_string()))
            .await
            .expect("a later worker should still run");
        assert_eq!(result, "worker recovered");
    }

    #[tokio::test]
    async fn extract_document_content_rejects_unsupported_file_type() {
        let err = extract_document_content_impl(
            None,
            BASE64.encode(b"whatever"),
            "file.bin".to_string(),
            "application/octet-stream".to_string(),
        )
        .await
        .expect_err("expected unsupported file type to error");

        assert!(
            err.contains("Unsupported file type"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn extract_document_content_rejects_invalid_base64() {
        let err = extract_document_content_impl(
            None,
            "not base64".to_string(),
            "file.txt".to_string(),
            "txt".to_string(),
        )
        .await
        .expect_err("expected invalid base64 to error");

        assert!(
            err.contains("Failed to decode base64 file"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn extract_document_content_rejects_invalid_utf8_for_text_files() {
        let err = extract_document_content_impl(
            None,
            BASE64.encode([0xff, 0xfe, 0xfd]),
            "bad.txt".to_string(),
            "txt".to_string(),
        )
        .await
        .expect_err("expected invalid utf-8 to error");

        assert!(
            err.contains("Failed to decode text file"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn hybrid_merge_deduplicates_native_lines_and_keeps_image_text() {
        assert_eq!(
            merge_native_and_ocr(
                "CONFIDENTIAL QUARTERLY MEMO 2026\nNative line",
                &[
                    "confidential quarterly memo 2026".to_string(),
                    "IMAGE CAPTION".to_string(),
                ]
            ),
            "CONFIDENTIAL QUARTERLY MEMO 2026\nNative line\nIMAGE CAPTION"
        );
    }

    #[test]
    fn ocr_source_budget_rejects_unsafe_page_allocations() {
        assert!(validate_ocr_source_budget(0, [(4_000, 6_000, 1_000)]).is_ok());
        assert!(validate_ocr_source_budget(0, [(6_000, 6_000, 1_000)]).is_err());
        assert!(validate_ocr_source_budget(
            0,
            [
                (4_000, 4_000, 1_000),
                (4_000, 4_000, 1_000),
                (4_000, 4_000, 1_000),
                (4_000, 4_000, 1_000),
                (1, 1, 1_000)
            ]
        )
        .is_err());
    }

    #[tokio::test]
    #[ignore = "set MAPLE_OCR_TEST_PDF and MAPLE_OCR_MODEL_DIR to run model-backed OCR"]
    async fn extracts_scanned_pdf_with_real_ocr_models() {
        let pdf_path = PathBuf::from(
            std::env::var("MAPLE_OCR_TEST_PDF").expect("MAPLE_OCR_TEST_PDF is required"),
        );
        let model_dir = PathBuf::from(
            std::env::var("MAPLE_OCR_MODEL_DIR").expect("MAPLE_OCR_MODEL_DIR is required"),
        );
        let pdf = std::fs::read(pdf_path).expect("read OCR PDF fixture");
        crate::onnxruntime::ensure_initialized().expect("initialize Maple's ONNX Runtime");
        let engine = OcrEngine::new(
            model_dir.join("det.onnx"),
            model_dir.join("rec.onnx"),
            model_dir.join("en_dict.txt"),
            OcrConfig::default(),
        )
        .map(Arc::new)
        .expect("load OCR models");

        let text = run_pdf_job(move || extract_pdf_with_ocr(pdf, engine))
            .await
            .expect("OCR extraction should succeed");
        let uppercase = text.to_uppercase();
        assert!(!text.trim().is_empty());
        assert!(
            uppercase.contains("OCR") || uppercase.contains("TEST") || uppercase.contains("HELLO"),
            "unexpected OCR output: {text:?}"
        );
    }

    fn native_text_pdf(text: &str) -> Vec<u8> {
        let content = format!("BT /F1 18 Tf 72 720 Td ({text}) Tj ET");
        build_pdf(&[
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_string(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        ])
    }

    fn native_text_and_image_pdf(text: &str) -> Vec<u8> {
        let content =
            format!("q 200 0 0 200 72 400 cm /Im1 Do Q BT /F1 18 Tf 72 720 Td ({text}) Tj ET");
        build_pdf(&[
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> /XObject << /Im1 6 0 R >> >> /Contents 5 0 R >>".to_string(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
            "<< /Type /XObject /Subtype /Image /Width 300 /Height 300 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /ASCIIHexDecode /Length 7 >>\nstream\nFF0000>\nendstream".to_string(),
        ])
    }

    fn build_pdf(objects: &[String]) -> Vec<u8> {
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref_offset = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }
}
