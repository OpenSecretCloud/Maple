use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use pdf_extract::extract_text_from_mem;
use serde::{Deserialize, Serialize};

const PDF_EXTRACTION_FAILED: &str = "Failed to extract text from PDF";

async fn run_pdf_extraction<F>(extract: F) -> Result<String, String>
where
    F: FnOnce() -> Result<String, String> + Send + 'static,
{
    match tokio::task::spawn_blocking(extract).await {
        Ok(result) => result,
        Err(error) => {
            if error.is_panic() {
                log::error!("PDF text extraction task panicked");
            } else {
                log::error!("PDF text extraction task was cancelled");
            }
            Err(format!(
                "{PDF_EXTRACTION_FAILED}: parser failed unexpectedly"
            ))
        }
    }
}

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
            run_pdf_extraction(move || {
                extract_text_from_mem(&file_bytes)
                    .map_err(|error| format!("{PDF_EXTRACTION_FAILED}: {error}"))
            })
            .await?
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
    use super::{extract_document_content, run_pdf_extraction, PDF_EXTRACTION_FAILED};
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};

    const TYPE4_PDF: &[u8] = include_bytes!("../tests/fixtures/type4-separation.pdf");
    const PAGE_FONT_COLLISION_PDF: &[u8] =
        include_bytes!("../tests/fixtures/page-font-name-collision.pdf");
    const XOBJECT_FONT_COLLISION_PDF: &[u8] =
        include_bytes!("../tests/fixtures/xobject-font-name-collision.pdf");
    const INVALID_PDF: &[u8] = include_bytes!("../tests/fixtures/invalid.pdf");

    async fn extract_pdf(pdf: &[u8], filename: &str) -> super::DocumentResponse {
        extract_document_content(
            BASE64.encode(pdf),
            filename.to_string(),
            "application/pdf".to_string(),
        )
        .await
        .expect("expected PDF extraction to succeed")
    }

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
    async fn extract_document_content_accepts_type4_separation_pdf() {
        let response = extract_pdf(TYPE4_PDF, "type4-separation.pdf").await;

        assert_eq!(response.status, "completed");
        assert_eq!(response.document.filename, "type4-separation.pdf");
        assert!(
            response.document.text_content.contains("Type 4 regression"),
            "unexpected text: {:?}",
            response.document.text_content
        );
    }

    #[tokio::test]
    async fn extract_document_content_keeps_page_font_resources_scoped() {
        let response = extract_pdf(PAGE_FONT_COLLISION_PDF, "page-fonts.pdf").await;
        let text: String = response
            .document
            .text_content
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();

        assert_eq!(text, "AB");
    }

    #[tokio::test]
    async fn extract_document_content_keeps_xobject_font_resources_scoped() {
        let response = extract_pdf(XOBJECT_FONT_COLLISION_PDF, "xobject-fonts.pdf").await;
        let text: String = response
            .document
            .text_content
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();

        assert_eq!(text, "AB");
    }

    #[tokio::test]
    async fn pdf_parser_panics_are_returned_as_errors() {
        let error = run_pdf_extraction(|| -> Result<String, String> {
            panic!("synthetic PDF parser panic")
        })
        .await
        .expect_err("expected a parser panic to become an error");

        assert_eq!(
            error,
            format!("{PDF_EXTRACTION_FAILED}: parser failed unexpectedly")
        );

        let response = extract_pdf(TYPE4_PDF, "after-parser-panic.pdf").await;
        assert!(response.document.text_content.contains("Type 4 regression"));
    }

    #[tokio::test]
    async fn extract_document_content_rejects_invalid_pdf() {
        let error = extract_document_content(
            BASE64.encode(INVALID_PDF),
            "invalid.pdf".to_string(),
            "application/pdf".to_string(),
        )
        .await
        .expect_err("expected invalid PDF extraction to fail");

        assert!(
            error.starts_with(PDF_EXTRACTION_FAILED),
            "unexpected error: {error}"
        );
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
