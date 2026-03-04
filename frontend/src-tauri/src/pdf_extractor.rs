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

            // Log PDF classification info for debugging
            log::info!(
                "PDF '{filename}': type={:?}, pages={}, confidence={:.2}, has_encoding_issues={}, time={}ms",
                result.pdf_type,
                result.page_count,
                result.confidence,
                result.has_encoding_issues,
                result.processing_time_ms
            );

            if result.has_encoding_issues {
                log::warn!(
                    "PDF '{filename}' has encoding issues - extracted text may be incomplete or garbled"
                );
            }

            // pdf-inspector extracts what it can from all PDF types.
            // For scanned/image-based PDFs it may return limited or no text.
            let markdown = result.markdown.unwrap_or_default();

            if markdown.trim().is_empty() {
                // Provide a more helpful error for scanned/image-based PDFs
                if matches!(result.pdf_type, PdfType::Scanned | PdfType::ImageBased) {
                    return Err(
                        "This PDF appears to be scanned or image-based. No text could be extracted.".to_string()
                    );
                }
                return Err("No text content could be extracted from this PDF.".to_string());
            }

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

    #[tokio::test]
    async fn extract_bitcoin_whitepaper_pdf() {
        // Read the Bitcoin whitepaper PDF from the test fixtures directory (not embedded in binary)
        let pdf_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test_fixtures/bitcoin_whitepaper.pdf");
        let pdf_bytes = std::fs::read(&pdf_path).unwrap_or_else(|e| {
            panic!(
                "failed to read test fixture at {}: {e}",
                pdf_path.display()
            )
        });

        let file_base64 = BASE64.encode(&pdf_bytes);

        let resp = extract_document_content(
            file_base64,
            "bitcoin.pdf".to_string(),
            "pdf".to_string(),
        )
        .await
        .expect("expected Bitcoin PDF extraction to succeed");

        assert_eq!(resp.status, "completed");
        assert_eq!(resp.document.filename, "bitcoin.pdf");

        let content = &resp.document.text_content;

        // Verify meaningful content was extracted (whitepaper is ~9 pages)
        assert!(
            content.len() > 1000,
            "expected substantial content from Bitcoin whitepaper, got {} chars",
            content.len()
        );

        // Verify key content from the Bitcoin whitepaper is present
        assert!(
            content.contains("Bitcoin") || content.contains("bitcoin"),
            "expected 'Bitcoin' in extracted text"
        );
        assert!(
            content.contains("peer-to-peer") || content.contains("peer to peer"),
            "expected 'peer-to-peer' in extracted text"
        );
        assert!(
            content.contains("Satoshi") || content.contains("Nakamoto"),
            "expected author name in extracted text"
        );
    }

    #[tokio::test]
    async fn extract_scanned_pdf_processes_without_early_rejection() {
        // Read a real scanned PDF (a scanned letter) from the test fixtures directory.
        // pdf-inspector should attempt extraction on all PDF types rather than
        // rejecting scanned PDFs outright — it may still extract useful content.
        let pdf_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test_fixtures/scanned_letter.pdf");
        let pdf_bytes = std::fs::read(&pdf_path).unwrap_or_else(|e| {
            panic!(
                "failed to read test fixture at {}: {e}",
                pdf_path.display()
            )
        });

        let file_base64 = BASE64.encode(&pdf_bytes);

        let result = extract_document_content(
            file_base64,
            "scanned_letter.pdf".to_string(),
            "pdf".to_string(),
        )
        .await;

        // The scanned PDF should be processed (not rejected early).
        // It may succeed with extracted content or fail with "No text" / "scanned or image-based"
        // error — but it should NOT be rejected before even attempting extraction.
        match result {
            Ok(resp) => {
                assert_eq!(resp.status, "completed");
                assert_eq!(resp.document.filename, "scanned_letter.pdf");
                // If content was extracted, verify it's non-empty
                assert!(
                    !resp.document.text_content.trim().is_empty(),
                    "expected non-empty content from scanned PDF"
                );
            }
            Err(err) => {
                // If extraction failed, it should be because no text was found
                // (not because we refused to try)
                assert!(
                    err.contains("No text content") || err.contains("scanned or image-based"),
                    "unexpected error: {err}"
                );
            }
        }
    }
}
