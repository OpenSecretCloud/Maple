import type { VoxtralTTSVoice } from "./ttsPreferences";

export const VOXTRAL_TTS_MODEL = "voxtral-tts" as const;

export type AiCustomFetch = (
  input: string | URL | Request,
  init?: RequestInit
) => Promise<Response>;

export interface TTSSynthesisPreferences {
  voice: VoxtralTTSVoice;
  speed: number;
}

export class TTSSynthesisHttpError extends Error {
  readonly status: number;

  constructor(status: number) {
    super(`Text-to-speech request failed with HTTP ${status}`);
    this.name = "TTSSynthesisHttpError";
    this.status = status;
  }
}

export class TTSSynthesisProviderError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "TTSSynthesisProviderError";
  }
}

const MAX_PROVIDER_ERROR_DETAIL_LENGTH = 500;

function responseMediaType(response: Response): string | null {
  return response.headers.get("content-type")?.split(";", 1)[0]?.trim().toLowerCase() || null;
}

function isJsonMediaType(mediaType: string | null): boolean {
  return mediaType === "application/json" || mediaType?.endsWith("+json") === true;
}

function providerErrorDetail(value: unknown): string | null {
  if (typeof value === "string") {
    return value.trim() || null;
  }
  if (!value || typeof value !== "object") {
    return null;
  }

  const record = value as Record<string, unknown>;
  return (
    providerErrorDetail(record.message) ??
    providerErrorDetail(record.detail) ??
    providerErrorDetail(record.error)
  );
}

function providerErrorMessage(body: string): string {
  let detail: string | null = null;
  try {
    detail = providerErrorDetail(JSON.parse(body));
  } catch {
    // The MIME type is sufficient to classify this as a provider error.
  }

  if (!detail) {
    return "Text-to-speech provider returned an error response";
  }

  const compactDetail = detail.replace(/\s+/g, " ").slice(0, MAX_PROVIDER_ERROR_DETAIL_LENGTH);
  return `Text-to-speech provider returned an error: ${compactDetail}`;
}

function abortError(signal: AbortSignal): Error {
  if (signal.reason instanceof Error) {
    return signal.reason;
  }
  const error = new Error("Text-to-speech request was canceled");
  error.name = "AbortError";
  return error;
}

function throwIfAborted(signal: AbortSignal) {
  if (signal.aborted) {
    throw abortError(signal);
  }
}

export async function synthesizeTTSChunk(
  aiCustomFetch: AiCustomFetch,
  apiUrl: string,
  input: string,
  preferences: TTSSynthesisPreferences,
  signal: AbortSignal
): Promise<ArrayBuffer> {
  throwIfAborted(signal);
  const response = await aiCustomFetch(`${apiUrl.replace(/\/+$/, "")}/v1/audio/speech`, {
    method: "POST",
    headers: {
      Accept: "audio/wav",
      "Content-Type": "application/json"
    },
    body: JSON.stringify({
      input,
      model: VOXTRAL_TTS_MODEL,
      voice: preferences.voice,
      speed: preferences.speed
    }),
    signal
  });
  throwIfAborted(signal);

  if (!response.ok) {
    throw new TTSSynthesisHttpError(response.status);
  }

  const mediaType = responseMediaType(response);
  if (isJsonMediaType(mediaType)) {
    const providerErrorBody = await response.text();
    throwIfAborted(signal);
    throw new TTSSynthesisProviderError(providerErrorMessage(providerErrorBody));
  }
  if (mediaType && !mediaType.startsWith("audio/") && mediaType !== "application/octet-stream") {
    throw new TTSSynthesisProviderError(
      `Text-to-speech provider returned unexpected content type: ${mediaType}`
    );
  }

  const audio = await response.arrayBuffer();
  throwIfAborted(signal);
  if (audio.byteLength === 0) {
    throw new Error("Text-to-speech returned an empty audio file");
  }
  return audio;
}

function statusFromUnknownError(error: unknown): number | null {
  if (!error || typeof error !== "object") {
    return null;
  }

  if ("status" in error && typeof error.status === "number") {
    return error.status;
  }
  if (
    "response" in error &&
    error.response &&
    typeof error.response === "object" &&
    "status" in error.response &&
    typeof error.response.status === "number"
  ) {
    return error.response.status;
  }
  return null;
}

export function isPaidTTSAccessError(error: unknown): boolean {
  const status = statusFromUnknownError(error);
  if (status === 402 || status === 403) {
    return true;
  }

  const message = error instanceof Error ? error.message : typeof error === "string" ? error : "";
  return /\b(?:402|403)\b|forbidden|paid (?:plan|subscription)/i.test(message);
}
