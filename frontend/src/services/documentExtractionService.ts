import { isTauri } from "@/utils/platform";
import type { NativeDocumentType } from "@/utils/documentUpload";

export interface ExtractedDocumentResponse {
  document: {
    filename: string;
    text_content: string;
  };
  status: string;
}

export interface DocumentExtractionBridge {
  isTauri(): boolean;
  invoke<T>(command: string, args: Record<string, unknown>): Promise<T>;
}

const defaultBridge: DocumentExtractionBridge = {
  isTauri,
  async invoke<T>(command: string, args: Record<string, unknown>): Promise<T> {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<T>(command, args);
  }
};

function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error ?? new Error("Failed to read document"));
    reader.onload = () => {
      if (typeof reader.result !== "string") {
        reject(new Error("Failed to encode document"));
        return;
      }
      const separator = reader.result.indexOf(",");
      if (separator < 0) {
        reject(new Error("Failed to encode document"));
        return;
      }
      resolve(reader.result.slice(separator + 1));
    };
    reader.readAsDataURL(file);
  });
}

export async function extractDocumentContent(
  file: File,
  fileType: NativeDocumentType,
  bridge: DocumentExtractionBridge = defaultBridge
): Promise<ExtractedDocumentResponse> {
  if (!bridge.isTauri()) {
    throw new Error("Document extraction is only available in the Maple app");
  }

  const fileBase64 = await fileToBase64(file);
  return await bridge.invoke<ExtractedDocumentResponse>("extract_document_content", {
    fileBase64,
    filename: file.name,
    fileType
  });
}
