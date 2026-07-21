import { describe, expect, test } from "bun:test";

import {
  getDocumentProcessingErrorMessage,
  getSupportedDocumentType,
  prepareExtractedPdfText
} from "./documentUpload";

describe("document upload helpers", () => {
  test("recognizes supported extensions case-insensitively", () => {
    expect(getSupportedDocumentType("report.PDF")).toBe("pdf");
    expect(getSupportedDocumentType("notes.Txt")).toBe("txt");
    expect(getSupportedDocumentType("README.Md")).toBe("md");
    expect(getSupportedDocumentType("image.png")).toBeNull();
  });

  test("cleans extracted image references and rejects PDFs without readable text", () => {
    expect(prepareExtractedPdfText("Hello\n![Image](image-1.png)\nworld")).toBe("Hello\n\nworld");
    expect(prepareExtractedPdfText("![Image](image-1.png)\n")).toBeNull();
    expect(prepareExtractedPdfText("  \n")).toBeNull();
    expect(prepareExtractedPdfText(undefined)).toBeNull();
  });

  test("surfaces non-empty backend string errors and hides unknown errors", () => {
    expect(getDocumentProcessingErrorMessage("  This PDF is password-protected  ")).toBe(
      "This PDF is password-protected"
    );
    expect(getDocumentProcessingErrorMessage("")).toBe("Failed to process document");
    expect(getDocumentProcessingErrorMessage(new Error("internal detail"))).toBe(
      "Failed to process document"
    );
  });
});
