use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use pdf_extract::extract_text_from_mem;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentData {
    pub filename: String,
    pub text_content: String,
    pub page_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentResponse {
    pub document: DocumentData,
    pub status: String,
    pub processing_time: f64,
}

#[tauri::command]
pub async fn extract_document_content(
    file_base64: String, 
    filename: String,
    file_type: String
) -> Result<DocumentResponse, String> {
    let start_time = std::time::Instant::now();
    
    // Decode base64 file data
    let file_bytes = BASE64
        .decode(&file_base64)
        .map_err(|e| format!("Failed to decode base64 file: {}", e))?;
    
    let text_content = match file_type.as_str() {
        "pdf" | "application/pdf" => {
            // Extract text from PDF
            extract_text_from_mem(&file_bytes)
                .map_err(|e| format!("Failed to extract text from PDF: {}", e))?
        },
        "txt" | "text/plain" | "md" | "text/markdown" => {
            // For text files, just convert bytes to string
            String::from_utf8(file_bytes)
                .map_err(|e| format!("Failed to decode text file: {}", e))?
        },
        _ => {
            return Err(format!("Unsupported file type: {}", file_type));
        }
    };
    
    // Count pages (for PDFs, count page breaks; for text files, estimate based on lines)
    let page_count = if file_type.contains("pdf") {
        text_content.matches('\x0C').count() + 1
    } else {
        // Rough estimate: ~50 lines per page
        (text_content.lines().count() / 50).max(1)
    };
    
    let processing_time = start_time.elapsed().as_secs_f64();
    
    Ok(DocumentResponse {
        document: DocumentData {
            filename,
            text_content,
            page_count,
        },
        status: "completed".to_string(),
        processing_time,
    })
}