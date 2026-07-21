export type SupportedDocumentType = "pdf" | "txt" | "md";

export function getSupportedDocumentType(filename: string): SupportedDocumentType | null {
  const normalizedFilename = filename.toLowerCase();

  if (normalizedFilename.endsWith(".pdf")) return "pdf";
  if (normalizedFilename.endsWith(".txt")) return "txt";
  if (normalizedFilename.endsWith(".md")) return "md";

  return null;
}

export function prepareExtractedPdfText(text: string | undefined): string | null {
  const cleanedText = (text ?? "").replace(/!\[Image\]\([^)]+\)/g, "");
  return cleanedText.trim() ? cleanedText : null;
}

export function getDocumentProcessingErrorMessage(error: unknown): string {
  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return "Failed to process document";
}
