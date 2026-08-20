export type NativeDocumentType = "pdf" | "doc" | "docx";
export type SupportedDocumentType = NativeDocumentType | "txt" | "md";

export function getSupportedDocumentType(filename: string): SupportedDocumentType | null {
  const normalizedFilename = filename.toLowerCase();

  if (normalizedFilename.endsWith(".pdf")) return "pdf";
  if (normalizedFilename.endsWith(".docx")) return "docx";
  if (normalizedFilename.endsWith(".doc")) return "doc";
  if (normalizedFilename.endsWith(".txt")) return "txt";
  if (normalizedFilename.endsWith(".md")) return "md";

  return null;
}

export function isNativeDocumentType(
  documentType: SupportedDocumentType
): documentType is NativeDocumentType {
  return documentType === "pdf" || documentType === "doc" || documentType === "docx";
}

export function prepareExtractedDocumentText(text: string | undefined): string | null {
  const extractedText = text ?? "";
  return extractedText.trim() ? extractedText : null;
}

export function prepareExtractedPdfText(text: string | undefined): string | null {
  const cleanedText = (text ?? "").replace(/!\[Image\]\([^)]+\)/g, "");
  return prepareExtractedDocumentText(cleanedText);
}

export function getDocumentProcessingErrorMessage(error: unknown): string {
  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return "Failed to process document";
}
