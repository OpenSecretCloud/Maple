use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use pdf_inspector::{process_pdf_mem, PdfType};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentData {
    pub filename: String,
    pub text_content: String,
    /// Base64-encoded page images (data URLs) for scanned/image-based PDFs.
    /// Present when the PDF needs OCR -- the frontend should send these to a
    /// vision model (e.g. Qwen 3 VL) for text extraction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_images: Option<Vec<String>>,
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

    match file_type.as_str() {
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
                // For scanned/image-based PDFs, extract page images for vision-model OCR
                if matches!(result.pdf_type, PdfType::Scanned | PdfType::ImageBased) {
                    log::info!(
                        "PDF '{filename}' is scanned/image-based -- extracting page images for OCR"
                    );
                    let page_images = extract_page_images(&file_bytes)?;
                    if page_images.is_empty() {
                        return Err(
                            "This PDF appears to be scanned or image-based but no page images could be extracted."
                                .to_string(),
                        );
                    }
                    return Ok(DocumentResponse {
                        document: DocumentData {
                            filename,
                            text_content: String::new(),
                            page_images: Some(page_images),
                        },
                        status: "completed".to_string(),
                    });
                }
                return Err("No text content could be extracted from this PDF.".to_string());
            }

            Ok(DocumentResponse {
                document: DocumentData {
                    filename,
                    text_content: markdown,
                    page_images: None,
                },
                status: "completed".to_string(),
            })
        }
        "txt" | "text/plain" | "md" | "text/markdown" => {
            // For text files, just convert bytes to string
            let text_content = String::from_utf8(file_bytes)
                .map_err(|e| format!("Failed to decode text file: {e}"))?;
            Ok(DocumentResponse {
                document: DocumentData {
                    filename,
                    text_content,
                    page_images: None,
                },
                status: "completed".to_string(),
            })
        }
        _ => Err(format!("Unsupported file type: {file_type}")),
    }
}

// ---------------------------------------------------------------------------
// Scanned-PDF page image extraction
// ---------------------------------------------------------------------------

/// Extract embedded images from each page of a scanned PDF.
///
/// Returns a Vec of base64 data-URL strings (e.g. `data:image/jpeg;base64,...`).
/// Handles the most common image encodings found in scanned documents:
///   - DCTDecode (JPEG) -- raw bytes passed through
///   - CCITTFaxDecode (Group 3/4) -- wrapped in a minimal TIFF, decoded, re-encoded as JPEG
///   - FlateDecode (raw pixels) -- decompressed and encoded as JPEG
fn extract_page_images(pdf_bytes: &[u8]) -> Result<Vec<String>, String> {
    let doc = lopdf::Document::load_mem(pdf_bytes)
        .map_err(|e| format!("Failed to parse PDF for image extraction: {e}"))?;

    let mut images: Vec<String> = Vec::new();

    for (_page_num, page_id) in doc.get_pages() {
        let page = doc
            .get_object(page_id)
            .map_err(|e| format!("Failed to get page object: {e}"))?;
        let dict = match page.as_dict() {
            Ok(d) => d,
            Err(_) => continue,
        };

        let resources = match dict.get(b"Resources") {
            Ok(r) => match doc.dereference(r) {
                Ok((_, obj)) => obj.clone(),
                Err(_) => continue,
            },
            Err(_) => continue,
        };

        let xobjects = match resources.as_dict().and_then(|d| d.get(b"XObject")) {
            Ok(x) => match doc.dereference(x) {
                Ok((_, obj)) => obj.clone(),
                Err(_) => continue,
            },
            Err(_) => continue,
        };

        let xobj_dict = match xobjects.as_dict() {
            Ok(d) => d,
            Err(_) => continue,
        };

        for (_name, obj_ref) in xobj_dict.iter() {
            let obj = match doc.dereference(obj_ref) {
                Ok((_, o)) => o.clone(),
                Err(_) => continue,
            };

            let stream = match obj.as_stream() {
                Ok(s) => s,
                Err(_) => continue,
            };

            // Only process Image XObjects
            let subtype = stream
                .dict
                .get(b"Subtype")
                .and_then(|s| s.as_name())
                .unwrap_or(b"");
            if subtype != b"Image" {
                continue;
            }

            // Determine the filter (compression) type.
            // Per PDF spec §7.3.4.2, Filter can be a single Name or an Array of Names.
            // In an array, the last filter describes the final data format (e.g.
            // [/ASCII85Decode /DCTDecode] means the underlying data is JPEG).
            let filter_name: Vec<u8> = stream
                .dict
                .get(b"Filter")
                .ok()
                .and_then(|f| {
                    f.as_name().map(|n| n.to_vec()).ok().or_else(|| {
                        f.as_array()
                            .ok()
                            .and_then(|arr| arr.last())
                            .and_then(|last| last.as_name().ok())
                            .map(|n| n.to_vec())
                    })
                })
                .unwrap_or_default();

            let data_url = match filter_name.as_slice() {
                b"DCTDecode" => {
                    // Raw JPEG -- pass through directly
                    let b64 = BASE64.encode(&stream.content);
                    format!("data:image/jpeg;base64,{b64}")
                }
                b"CCITTFaxDecode" => {
                    // CCITT Group 3/4 fax data -- wrap in TIFF, decode, re-encode as JPEG
                    decode_ccitt_to_data_url(stream)?
                }
                b"FlateDecode" => {
                    // Raw pixel data compressed with zlib
                    decode_flate_to_data_url(stream)?
                }
                other => {
                    let name = String::from_utf8_lossy(other);
                    log::warn!("Unsupported PDF image filter '{name}' -- skipping");
                    continue;
                }
            };

            images.push(data_url);
        }
    }

    Ok(images)
}

/// Decode a CCITTFaxDecode image stream by wrapping it in a minimal TIFF file,
/// then decoding with the `image` crate and re-encoding as JPEG.
fn decode_ccitt_to_data_url(stream: &lopdf::Stream) -> Result<String, String> {
    let width =
        get_stream_int(&stream.dict, b"Width").ok_or("CCITTFaxDecode image missing Width")?;
    let height =
        get_stream_int(&stream.dict, b"Height").ok_or("CCITTFaxDecode image missing Height")?;

    // K < 0 = Group 4, K = 0 = Group 3 1-D, K > 0 = Group 3 2-D
    let k = get_decode_parm_int(&stream.dict, b"K").unwrap_or(-1);

    let tiff_bytes = wrap_ccitt_as_tiff(&stream.content, width as u32, height as u32, k as i32);

    let img = image::load_from_memory(&tiff_bytes)
        .map_err(|e| format!("Failed to decode CCITT image via TIFF wrapper: {e}"))?;

    encode_image_to_jpeg_data_url(&img)
}

/// Decode a FlateDecode (zlib-compressed raw pixels) image stream.
fn decode_flate_to_data_url(stream: &lopdf::Stream) -> Result<String, String> {
    let width =
        get_stream_int(&stream.dict, b"Width").ok_or("FlateDecode image missing Width")? as u32;
    let height =
        get_stream_int(&stream.dict, b"Height").ok_or("FlateDecode image missing Height")? as u32;
    let bpc = get_stream_int(&stream.dict, b"BitsPerComponent").unwrap_or(8) as u32;

    // Decompress zlib data
    use std::io::Read;
    let mut decoder = flate2::read::ZlibDecoder::new(&stream.content[..]);
    let mut raw_pixels = Vec::new();
    decoder
        .read_to_end(&mut raw_pixels)
        .map_err(|e| format!("Failed to decompress FlateDecode image: {e}"))?;

    // Determine color space
    let cs_name = stream
        .dict
        .get(b"ColorSpace")
        .and_then(|c| c.as_name().map(|n| n.to_vec()))
        .unwrap_or_else(|_| b"DeviceGray".to_vec());

    let img: image::DynamicImage = match (cs_name.as_slice(), bpc) {
        (b"DeviceGray", 8) => {
            let gray = image::GrayImage::from_raw(width, height, raw_pixels)
                .ok_or("Failed to construct grayscale image from raw pixels")?;
            image::DynamicImage::ImageLuma8(gray)
        }
        (b"DeviceRGB", 8) => {
            let rgb = image::RgbImage::from_raw(width, height, raw_pixels)
                .ok_or("Failed to construct RGB image from raw pixels")?;
            image::DynamicImage::ImageRgb8(rgb)
        }
        (b"DeviceGray", 1) => {
            // 1-bit grayscale -- expand to 8-bit
            let expanded = expand_1bit_to_8bit(&raw_pixels, width, height);
            let gray = image::GrayImage::from_raw(width, height, expanded)
                .ok_or("Failed to construct 1-bit grayscale image")?;
            image::DynamicImage::ImageLuma8(gray)
        }
        _ => {
            let name = String::from_utf8_lossy(&cs_name);
            return Err(format!(
                "Unsupported FlateDecode color space / bpc: {name}/{bpc}"
            ));
        }
    };

    encode_image_to_jpeg_data_url(&img)
}

/// Encode a DynamicImage as a JPEG data URL.
fn encode_image_to_jpeg_data_url(img: &image::DynamicImage) -> Result<String, String> {
    let mut jpeg_buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut jpeg_buf);
    img.write_to(&mut cursor, image::ImageFormat::Jpeg)
        .map_err(|e| format!("Failed to encode image as JPEG: {e}"))?;
    let b64 = BASE64.encode(&jpeg_buf);
    Ok(format!("data:image/jpeg;base64,{b64}"))
}

/// Expand 1-bit-per-pixel data to 8-bit grayscale.
fn expand_1bit_to_8bit(data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let row_bytes = (width as usize).div_ceil(8);
    let mut out = Vec::with_capacity((width * height) as usize);
    for y in 0..height as usize {
        for x in 0..width as usize {
            let byte_idx = y * row_bytes + x / 8;
            let bit_idx = 7 - (x % 8);
            let bit = if byte_idx < data.len() {
                (data[byte_idx] >> bit_idx) & 1
            } else {
                0
            };
            // In PDF, 0 is typically black for 1-bit images
            out.push(if bit == 0 { 0 } else { 255 });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// TIFF wrapper for CCITT Group 3/4 data
// ---------------------------------------------------------------------------

/// Wrap raw CCITT Group 3/4 data in a minimal TIFF container so the `image`
/// crate (via its `tiff` backend) can decode it.
fn wrap_ccitt_as_tiff(ccitt_data: &[u8], width: u32, height: u32, k: i32) -> Vec<u8> {
    // TIFF compression tag: 3 = CCITT Group 3, 4 = CCITT Group 4
    let compression: u16 = if k < 0 { 4 } else { 3 };

    let num_entries: u16 = 8;
    let ifd_size = 2 + (num_entries as u32) * 12 + 4;
    let data_offset: u32 = 8 + ifd_size;

    let mut buf = Vec::with_capacity(data_offset as usize + ccitt_data.len());

    // TIFF header (8 bytes)
    buf.extend_from_slice(b"II"); // little-endian
    buf.extend_from_slice(&42u16.to_le_bytes()); // magic
    buf.extend_from_slice(&8u32.to_le_bytes()); // offset to first IFD

    // IFD entry count
    buf.extend_from_slice(&num_entries.to_le_bytes());

    // IFD entries (must be sorted by tag)
    write_ifd_long(&mut buf, 256, width); // ImageWidth
    write_ifd_long(&mut buf, 257, height); // ImageLength
    write_ifd_short(&mut buf, 258, 1); // BitsPerSample
    write_ifd_short(&mut buf, 259, compression); // Compression
    write_ifd_short(&mut buf, 262, 0); // PhotometricInterpretation (WhiteIsZero)
    write_ifd_long(&mut buf, 273, data_offset); // StripOffsets
    write_ifd_long(&mut buf, 278, height); // RowsPerStrip
    write_ifd_long(&mut buf, 279, ccitt_data.len() as u32); // StripByteCounts

    // Next IFD offset (0 = no more IFDs)
    buf.extend_from_slice(&0u32.to_le_bytes());

    // Image data
    buf.extend_from_slice(ccitt_data);

    buf
}

fn write_ifd_short(buf: &mut Vec<u8>, tag: u16, value: u16) {
    buf.extend_from_slice(&tag.to_le_bytes());
    buf.extend_from_slice(&3u16.to_le_bytes()); // type = SHORT
    buf.extend_from_slice(&1u32.to_le_bytes()); // count
    buf.extend_from_slice(&(value as u32).to_le_bytes());
}

fn write_ifd_long(buf: &mut Vec<u8>, tag: u16, value: u32) {
    buf.extend_from_slice(&tag.to_le_bytes());
    buf.extend_from_slice(&4u16.to_le_bytes()); // type = LONG
    buf.extend_from_slice(&1u32.to_le_bytes()); // count
    buf.extend_from_slice(&value.to_le_bytes());
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read an integer value from a PDF stream dictionary.
fn get_stream_int(dict: &lopdf::Dictionary, key: &[u8]) -> Option<i64> {
    dict.get(key).ok().and_then(|v| v.as_i64().ok())
}

/// Read an integer from the DecodeParms sub-dictionary of a stream.
fn get_decode_parm_int(dict: &lopdf::Dictionary, key: &[u8]) -> Option<i64> {
    dict.get(b"DecodeParms")
        .ok()
        .and_then(|v| v.as_dict().ok())
        .and_then(|d| d.get(key).ok())
        .and_then(|v| v.as_i64().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let pdf_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_fixtures/bitcoin_whitepaper.pdf");
        let pdf_bytes = std::fs::read(&pdf_path).unwrap_or_else(|e| {
            panic!("failed to read test fixture at {}: {e}", pdf_path.display())
        });

        let file_base64 = BASE64.encode(&pdf_bytes);

        let resp =
            extract_document_content(file_base64, "bitcoin.pdf".to_string(), "pdf".to_string())
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
    async fn extract_scanned_pdf_returns_page_images() {
        // Read a real scanned PDF (a scanned letter) from the test fixtures directory.
        // Since it's a scanned document, pdf-inspector won't extract text -- instead
        // we should get page images back for vision-model OCR.
        let pdf_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_fixtures/scanned_letter.pdf");
        let pdf_bytes = std::fs::read(&pdf_path).unwrap_or_else(|e| {
            panic!("failed to read test fixture at {}: {e}", pdf_path.display())
        });

        let file_base64 = BASE64.encode(&pdf_bytes);

        let resp = extract_document_content(
            file_base64,
            "scanned_letter.pdf".to_string(),
            "pdf".to_string(),
        )
        .await
        .expect("expected scanned PDF extraction to succeed (with page images)");

        assert_eq!(resp.status, "completed");
        assert_eq!(resp.document.filename, "scanned_letter.pdf");

        // Text content should be empty for a scanned PDF
        assert!(
            resp.document.text_content.is_empty(),
            "scanned PDF should have empty text_content"
        );

        // Should have page images for OCR
        let page_images = resp
            .document
            .page_images
            .expect("scanned PDF should have page_images");

        assert_eq!(
            page_images.len(),
            1,
            "scanned letter has 1 page, expected 1 image"
        );

        // Each image should be a valid JPEG data URL
        assert!(
            page_images[0].starts_with("data:image/jpeg;base64,"),
            "page image should be a JPEG data URL"
        );

        // Verify the image has reasonable size (not empty/corrupt)
        let b64_data = page_images[0]
            .strip_prefix("data:image/jpeg;base64,")
            .unwrap();
        let decoded_size = BASE64.decode(b64_data).unwrap().len();
        assert!(
            decoded_size > 1000,
            "JPEG image should be >1KB, got {} bytes",
            decoded_size
        );
    }

    #[test]
    fn test_wrap_ccitt_as_tiff_structure() {
        let fake_data = vec![0u8; 100];
        let tiff = wrap_ccitt_as_tiff(&fake_data, 100, 200, -1);

        // Check TIFF header
        assert_eq!(&tiff[0..2], b"II"); // little-endian
        assert_eq!(u16::from_le_bytes([tiff[2], tiff[3]]), 42); // magic
        assert_eq!(u32::from_le_bytes([tiff[4], tiff[5], tiff[6], tiff[7]]), 8); // IFD offset

        // Should end with our image data
        assert_eq!(&tiff[tiff.len() - 100..], &fake_data[..]);
    }
}
