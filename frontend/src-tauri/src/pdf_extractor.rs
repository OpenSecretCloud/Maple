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
    const DECLARED_ENCODING_CFF_PDF: &[u8] =
        include_bytes!("../tests/fixtures/declared-encoding-overrides-builtin.pdf");
    const TYPE1_OUT_OF_RANGE_ENCODING_PDF: &[u8] =
        include_bytes!("../tests/fixtures/type1-encoding-code-300.pdf");
    const DIFFERENCES_ONLY_CFF_PDF: &[u8] =
        include_bytes!("../tests/fixtures/differences-only-embedded-cff.pdf");
    const NO_ENCODING_MISSING_SLOT_CFF_PDF: &[u8] =
        include_bytes!("../tests/fixtures/no-encoding-missing-slot-cff.pdf");
    const ABSENT_CFF_CHARSET_GLYPH_PDF: &[u8] =
        include_bytes!("../tests/fixtures/absent-cff-charset-glyph.pdf");
    const NOTDEF_DIFFERENCE_CFF_PDF: &[u8] =
        include_bytes!("../tests/fixtures/notdef-difference-embedded-cff.pdf");
    const SYMBOL_DIFFERENCES_PDF: &[u8] =
        include_bytes!("../tests/fixtures/symbol-differences.pdf");
    const ZAPF_DINGBATS_DIFFERENCES_PDF: &[u8] =
        include_bytes!("../tests/fixtures/zapf-dingbats-differences.pdf");
    const NON_ZAPF_DIFFERENCES_PDF: &[u8] =
        include_bytes!("../tests/fixtures/non-zapf-differences.pdf");
    const EMBEDDED_NON_ZAPF_A1_PDF: &[u8] =
        include_bytes!("../tests/fixtures/embedded-non-zapf-a1.pdf");
    const FONTAWESOME_TOUNICODE_PDF: &[u8] =
        include_bytes!("../tests/fixtures/fontawesome-tounicode-occupied.pdf");
    const FINITE_XOBJECT_REENTRY_PDF: &[u8] =
        include_bytes!("../tests/fixtures/finite-form-reentry.pdf");
    const RECURSIVE_XOBJECT_PDF: &[u8] =
        include_bytes!("../tests/fixtures/recursive-form-xobject.pdf");
    const DEEP_XOBJECT_PDF: &[u8] = include_bytes!("../tests/fixtures/xobject-depth-101.pdf");
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

    fn compact_text(text: &str) -> String {
        text.chars()
            .filter(|character| !character.is_whitespace())
            .collect()
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
        let text = compact_text(&response.document.text_content);

        assert_eq!(text, "AB");
    }

    #[tokio::test]
    async fn extract_document_content_keeps_xobject_font_resources_scoped() {
        let response = extract_pdf(XOBJECT_FONT_COLLISION_PDF, "xobject-fonts.pdf").await;
        let text = compact_text(&response.document.text_content);

        assert_eq!(text, "AB");
    }

    #[tokio::test]
    async fn declared_encoding_overrides_embedded_cff_encoding() {
        let response = extract_pdf(DECLARED_ENCODING_CFF_PDF, "declared-encoding.pdf").await;

        assert_eq!(response.document.text_content.trim(), "Mooré");
    }

    #[tokio::test]
    async fn declared_encoding_ignores_out_of_range_type1_builtin_codes() {
        let response = extract_pdf(TYPE1_OUT_OF_RANGE_ENCODING_PDF, "type1-code-300.pdf").await;

        assert_eq!(response.document.text_content.trim(), "A");
    }

    #[tokio::test]
    async fn differences_only_encoding_uses_embedded_cff_base() {
        let response = extract_pdf(DIFFERENCES_ONLY_CFF_PDF, "differences-only.pdf").await;

        assert_eq!(response.document.text_content.trim(), "éMoorØ");
    }

    #[tokio::test]
    async fn embedded_cff_without_declared_encoding_skips_unmapped_slots() {
        let response = extract_pdf(NO_ENCODING_MISSING_SLOT_CFF_PDF, "no-encoding.pdf").await;

        assert_eq!(response.document.text_content.trim(), "MoorØ");
    }

    #[tokio::test]
    async fn embedded_cff_skips_encoding_entries_absent_from_its_charset() {
        let response = extract_pdf(ABSENT_CFF_CHARSET_GLYPH_PDF, "absent-cff-glyph.pdf").await;

        assert_eq!(response.document.text_content.trim(), "Moor");
    }

    #[tokio::test]
    async fn notdef_difference_clears_the_embedded_cff_base_slot() {
        let response = extract_pdf(NOTDEF_DIFFERENCE_CFF_PDF, "notdef-difference.pdf").await;

        assert_eq!(response.document.text_content.trim(), "MoorØ");
    }

    #[tokio::test]
    async fn differences_only_encoding_uses_symbol_font_base() {
        let response = extract_pdf(SYMBOL_DIFFERENCES_PDF, "symbol-differences.pdf").await;

        assert_eq!(response.document.text_content.trim(), "ΓΒ");
    }

    #[tokio::test]
    async fn differences_only_encoding_uses_zapf_dingbats_font_base() {
        let response = extract_pdf(
            ZAPF_DINGBATS_DIFFERENCES_PDF,
            "zapf-dingbats-differences.pdf",
        )
        .await;

        assert_eq!(response.document.text_content.trim(), "✁✁");
    }

    #[tokio::test]
    async fn zapf_glyph_names_do_not_apply_to_other_fonts() {
        let response = extract_pdf(NON_ZAPF_DIFFERENCES_PDF, "non-zapf-differences.pdf").await;

        assert_eq!(response.document.text_content.trim(), "B");
    }

    #[tokio::test]
    async fn embedded_zapf_glyph_names_do_not_apply_to_other_fonts() {
        let response = extract_pdf(EMBEDDED_NON_ZAPF_A1_PDF, "embedded-non-zapf-a1.pdf").await;

        assert!(response.document.text_content.trim().is_empty());
    }

    #[tokio::test]
    async fn unknown_fontawesome_difference_preserves_tounicode_mapping() {
        let response = extract_pdf(FONTAWESOME_TOUNICODE_PDF, "fontawesome-tounicode.pdf").await;

        assert_eq!(response.document.text_content.trim(), "✉");
    }

    #[tokio::test]
    async fn form_xobjects_may_reenter_with_different_resources_and_repeat() {
        let response = extract_pdf(FINITE_XOBJECT_REENTRY_PDF, "finite-reentry.pdf").await;

        assert_eq!(response.status, "completed");
    }

    #[tokio::test]
    async fn excessive_form_xobject_nesting_returns_an_error() {
        let error = extract_document_content(
            BASE64.encode(DEEP_XOBJECT_PDF),
            "deep-xobjects.pdf".to_string(),
            "application/pdf".to_string(),
        )
        .await
        .expect_err("expected excessive Form XObject nesting to fail extraction");

        assert!(
            error.starts_with(PDF_EXTRACTION_FAILED),
            "unexpected error: {error}"
        );
        assert!(
            error.contains("Form XObject nesting exceeds 100 levels"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn recursive_form_xobjects_return_an_error_without_aborting() {
        let error = extract_document_content(
            BASE64.encode(RECURSIVE_XOBJECT_PDF),
            "recursive-xobject.pdf".to_string(),
            "application/pdf".to_string(),
        )
        .await
        .expect_err("expected recursive Form XObjects to fail extraction");

        assert!(
            error.starts_with(PDF_EXTRACTION_FAILED),
            "unexpected error: {error}"
        );
        assert!(
            error.contains("recursive Form XObject reference"),
            "unexpected error: {error}"
        );

        let response = extract_pdf(TYPE4_PDF, "after-recursive-xobject.pdf").await;
        assert!(response.document.text_content.contains("Type 4 regression"));
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
