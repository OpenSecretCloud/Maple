import { afterAll, beforeAll, describe, expect, test } from "bun:test";

import {
  extractDocumentContent,
  type DocumentExtractionBridge,
  type ExtractedDocumentResponse
} from "./documentExtractionService";

class RecordingBridge implements DocumentExtractionBridge {
  isNative = true;
  calls: Array<{ command: string; args: Record<string, unknown> }> = [];
  response: ExtractedDocumentResponse = {
    document: {
      filename: "report.docx",
      text_content: "Extracted text"
    },
    status: "completed"
  };

  isTauri(): boolean {
    return this.isNative;
  }

  async invoke<T>(command: string, args: Record<string, unknown>): Promise<T> {
    this.calls.push({ command, args });
    return this.response as T;
  }
}

const browserFileReader = globalThis.FileReader;

class TestFileReader {
  error: DOMException | null = null;
  onerror: ((event: ProgressEvent<FileReader>) => void) | null = null;
  onload: ((event: ProgressEvent<FileReader>) => void) | null = null;
  result: string | ArrayBuffer | null = null;

  readAsDataURL(file: File): void {
    void file.arrayBuffer().then(
      (buffer) => {
        const binary = String.fromCharCode(...new Uint8Array(buffer));
        this.result = `data:${file.type};base64,${btoa(binary)}`;
        this.onload?.call(this as unknown as FileReader, {} as ProgressEvent<FileReader>);
      },
      (error) => {
        this.error = error instanceof DOMException ? error : new DOMException(String(error));
        this.onerror?.call(this as unknown as FileReader, {} as ProgressEvent<FileReader>);
      }
    );
  }
}

beforeAll(() => {
  globalThis.FileReader = TestFileReader as unknown as typeof FileReader;
});

afterAll(() => {
  globalThis.FileReader = browserFileReader;
});

describe("document extraction service", () => {
  test("base64-encodes the file and invokes the typed native command", async () => {
    const bridge = new RecordingBridge();
    const file = new File([new Uint8Array([0, 255, 16])], "report.docx", {
      type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    });

    const result = await extractDocumentContent(file, "docx", bridge);

    expect(result).toEqual(bridge.response);
    expect(bridge.calls).toEqual([
      {
        command: "extract_document_content",
        args: {
          fileBase64: "AP8Q",
          filename: "report.docx",
          fileType: "docx"
        }
      }
    ]);
  });

  test("forwards every supported native document type", async () => {
    const bridge = new RecordingBridge();
    const file = new File(["document"], "document.bin");

    for (const fileType of ["pdf", "doc", "docx"] as const) {
      await extractDocumentContent(file, fileType, bridge);
    }

    expect(bridge.calls.map(({ args }) => args.fileType)).toEqual(["pdf", "doc", "docx"]);
  });

  test("fails closed outside Tauri before invoking the native command", async () => {
    const bridge = new RecordingBridge();
    bridge.isNative = false;

    await expect(
      extractDocumentContent(new File(["document"], "report.doc"), "doc", bridge)
    ).rejects.toThrow("Document extraction is only available in the Maple app");
    expect(bridge.calls).toHaveLength(0);
  });
});
