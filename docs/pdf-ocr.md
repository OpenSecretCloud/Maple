# PDF extraction and OCR

Maple uses [`pdf_oxide` 0.3.74](https://crates.io/crates/pdf_oxide/0.3.74) for native PDF text extraction and page classification. Pages classified as scanned or image-backed are OCR'd locally with PDFOxide's PaddleOCR integration. PDF bytes and rendered pages never leave the device.

## Inference backend

Maple enables PDFOxide's `ocr-tract` and `rendering` features on macOS, Windows, Linux, iOS, and Android. Tract executes the same ONNX models in pure Rust on every target.

PDFOxide's alternative `ocr` feature currently pins `ort = 2.0.0-rc.11` and unconditionally enables dynamic ONNX Runtime loading. Using it would conflict with Maple's statically linked iOS runtime, add an ONNX Runtime requirement to Android, and drop the official Intel macOS runtime that Maple's universal macOS 13.3 build still supports. The existing TTS ONNX Runtime therefore remains on its independently tested version; it does not participate in PDF OCR.

The application constructs and reuses one `OcrEngine`. Native-only PDFs bypass model setup entirely. OCR-routed pages are rendered once in full, bounded to a 2,000 by 2,000-pixel box, so tiled scans, inline images, and vector content are not lost by selecting a single embedded image. Mixed pages retain native text and append only OCR detection spans not already represented in the native layer. OCR is optional enrichment for native-readable hybrid pages: if models or OCR are unavailable, Maple keeps the native text instead of making an ordinary digital PDF depend on the network.

## Model supply and cache

The first scanned PDF downloads 12,577,821 bytes into the application cache under `ocr/models/paddleocr-en-v1`. Mobile operating systems may purge that cache, in which case Maple downloads and verifies it again.

Maple does not use PDFOxide's mutable `resolve/main` downloader. Every artifact is pinned to an immutable repository revision and checked for its exact length and SHA-256 before it is loaded:

| File | Immutable source | Bytes | SHA-256 |
| --- | --- | ---: | --- |
| `det.onnx` | [SWHL RapidOCR PaddleOCR detector](https://huggingface.co/SWHL/RapidOCR/blob/1cfba2e90fc938db55889873735088de210cc173/PP-OCRv4/ch_PP-OCRv4_det_infer.onnx) | 4,745,517 | `d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9` |
| `rec.onnx` | [monkt English recognizer](https://huggingface.co/monkt/paddleocr-onnx/blob/7b02d0a30a07ba2b92ad1ff5a8941ae2c633de65/languages/english/rec.onnx) | 7,830,888 | `4e16deb22c4da6468bdca539b2cd3c8687825538b67109177c47d359ab994cd7` |
| `en_dict.txt` | [monkt English dictionary](https://huggingface.co/monkt/paddleocr-onnx/blob/7b02d0a30a07ba2b92ad1ff5a8941ae2c633de65/languages/english/dict.txt) | 1,416 | `e025a66d31f327ba0c232e03f407ae8d105e1e709e7ccb3f408aa778c24e70d6` |

PaddleOCR, SWHL RapidOCR, and the monkt model repository declare Apache-2.0. Maple uses the SWHL detector rather than PDFOxide's default custom-notice mirror; the unforked backend accepts the model directly and the model-backed test verifies compatibility.

On Android, the model-only HTTP client uses rustls with the standard Mozilla WebPKI roots. This avoids reqwest's separate Android platform-verifier Kotlin/JNI setup; desktop and iOS retain their normal platform trust stores.

## Panic and error boundary

All PDF parsing, classification, rendering, and inference work runs in an isolated blocking task. An ordinary Rust unwind becomes a normal Tauri command error, so the frontend clears its busy state and remains usable. Process aborts and native stack exhaustion cannot be recovered by a Rust unwind boundary. Maple therefore serializes PDF jobs, bounds OCR renders to four megapixels, and rejects OCR pages above conservative source-image count and pixel budgets before PDFOxide decodes them.

The backend independently enforces 10 MiB limits on both the input document and extracted text, rejects locked PDFs with a specific message, and treats a document with no recognized text as an error rather than silently attaching an empty document.

## Known upstream opportunities

No PDFOxide fork is required. Potential upstream contributions that would simplify Maple's integration are:

- enable `AutoExtractor` OCR routing and model prefetch for `ocr-tract`, not only `ocr`;
- accept a reusable caller-owned `OcrEngine` in the whole-document auto extractor;
- expose the native/OCR merge helper;
- integrate the public full-page renderer as an OCR fallback;
- publish the inline-image decoding fix currently newer than 0.3.74.
- publish normal Rust library metadata separately from the `cdylib` and `staticlib` FFI artifacts, reducing multi-target build time and disk use.

Maple's current model pack recognizes English text. Additional language packs, password entry, and OCR download progress beyond the attachment spinner are separate product work.

## Model-backed test

The focused test can be run with a scanned fixture and the three verified model files:

```sh
MAPLE_OCR_TEST_PDF=/absolute/path/to/scanned.pdf \
MAPLE_OCR_MODEL_DIR=/absolute/path/to/models \
nix develop -c cargo test --release --locked --lib \
  extracts_scanned_pdf_with_real_ocr_models -- --ignored --nocapture
```

Use the release profile for this model-backed test. Tract performs expensive graph specialization in an unoptimized build and is not representative of the packaged application.
