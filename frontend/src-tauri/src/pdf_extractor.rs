use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use pdf_inspector::{process_pdf_mem, PdfType};
use serde::{Deserialize, Serialize};

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

#[tauri::command]
pub async fn extract_document_content(
    file_base64: String,
    filename: String,
    file_type: String,
) -> Result<DocumentResponse, String> {
    // Decode base64 file data
    let file_bytes = BASE64
        .decode(&file_base64)
        .map_err(|e| format!("Failed to decode base64 file: {e}"))?;

    let text_content = match file_type.as_str() {
        "pdf" | "application/pdf" => {
            // Use pdf-inspector for full PDF processing (detect + extract + markdown)
            let result = process_pdf_mem(&file_bytes)
                .map_err(|e| format!("Failed to extract text from PDF: {e}"))?;

            // For scanned or image-based PDFs, pdf-inspector detects the type
            // but cannot extract text (needs OCR which is outside scope)
            if matches!(result.pdf_type, PdfType::Scanned | PdfType::ImageBased) {
                return Err(
                    "This PDF appears to be scanned or image-based. Text extraction is not available for this type of PDF.".to_string()
                );
            }

            // If encoding issues were detected, warn but still return what we have
            let markdown = result.markdown.unwrap_or_default();

            if markdown.trim().is_empty() {
                return Err("No text content could be extracted from this PDF.".to_string());
            }

            if result.has_encoding_issues {
                log::warn!(
                    "PDF '{filename}' has encoding issues - extracted text may be incomplete or garbled"
                );
            }

            log::info!(
                "PDF '{filename}': type={:?}, pages={}, confidence={:.2}, time={}ms",
                result.pdf_type,
                result.page_count,
                result.confidence,
                result.processing_time_ms
            );

            markdown
        }
        "txt" | "text/plain" | "md" | "text/markdown" => {
            // For text files, just convert bytes to string
            String::from_utf8(file_bytes).map_err(|e| format!("Failed to decode text file: {e}"))?
        }
        _ => {
            return Err(format!("Unsupported file type: {file_type}"));
        }
    };

    Ok(DocumentResponse {
        document: DocumentData {
            filename,
            text_content,
        },
        status: "completed".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::extract_document_content;
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};

    #[tokio::test]
    async fn extract_document_content_text_plain_success() {
        let file_base64 = BASE64.encode(b"Hello, Maple!");

        let resp = extract_document_content(
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
    async fn extract_document_content_rejects_unsupported_file_type() {
        let file_base64 = BASE64.encode(b"whatever");

        let err = extract_document_content(
            file_base64,
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
        let err = extract_document_content(
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
        let file_base64 = BASE64.encode([0xff, 0xfe, 0xfd]);

        let err = extract_document_content(file_base64, "bad.txt".to_string(), "txt".to_string())
            .await
            .expect_err("expected invalid utf-8 to error");

        assert!(
            err.contains("Failed to decode text file"),
            "unexpected error: {err}"
        );
    }
}
