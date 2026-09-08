import { describe, expect, test } from "bun:test";

import {
  getDocumentProcessingErrorMessage,
  getSupportedDocumentType,
  isNativeDocumentType,
  prepareExtractedDocumentText,
  prepareExtractedPdfText
} from "./documentUpload";

describe("document upload helpers", () => {
  test("recognizes supported extensions case-insensitively", () => {
    expect(getSupportedDocumentType("report.PDF")).toBe("pdf");
    expect(getSupportedDocumentType("proposal.DoCx")).toBe("docx");
    expect(getSupportedDocumentType("legacy.DoC")).toBe("doc");
    expect(getSupportedDocumentType("notes.Txt")).toBe("txt");
    expect(getSupportedDocumentType("README.Md")).toBe("md");
    expect(getSupportedDocumentType("image.png")).toBeNull();
    expect(getSupportedDocumentType("macro.docm")).toBeNull();
    expect(getSupportedDocumentType("template.dot")).toBeNull();
    expect(getSupportedDocumentType("open-document.odt")).toBeNull();
    expect(getSupportedDocumentType("report.docx.exe")).toBeNull();
  });

  test("identifies document types that require native extraction", () => {
    expect(isNativeDocumentType("pdf")).toBe(true);
    expect(isNativeDocumentType("doc")).toBe(true);
    expect(isNativeDocumentType("docx")).toBe(true);
    expect(isNativeDocumentType("txt")).toBe(false);
    expect(isNativeDocumentType("md")).toBe(false);
  });

  test("rejects blank extracted text without applying PDF-specific cleanup", () => {
    expect(prepareExtractedDocumentText("Word text\n![Image](kept.png)")).toBe(
      "Word text\n![Image](kept.png)"
    );
    expect(prepareExtractedDocumentText("  \n")).toBeNull();
    expect(prepareExtractedDocumentText(undefined)).toBeNull();
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
