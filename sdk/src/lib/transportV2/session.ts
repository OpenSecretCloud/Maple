import {
  MIN_ENCRYPTED_RECORD_BYTES,
  TransportV2ProtocolError,
  decodeCanonicalBase64,
  encodeCanonicalBase64,
  encodeUtf8,
  generateRequestId,
  parseStrictJson,
  requireExactObject,
  uuidToBytes
} from "./encoding";
import {
  TRANSPORT_V2_LIMITS,
  parseUnaryResponseEnvelope,
  serializeRequestEnvelope,
  type ResponseMode,
  type TransportV2RequestEnvelope,
  type TransportV2UnaryResponse
} from "./envelope";
import {
  decryptTransportV2Record,
  encryptTransportV2Record,
  requestRecordAad,
  streamResponseRecordAad,
  unaryResponseRecordAad,
  type TransportV2HandshakeResult
} from "./crypto";
import { TransportV2StreamDecoder } from "./stream";

const MAX_REQUEST_RECORDS = 65_536;
const MAX_RESPONSE_RECORDS = 65_536;
const OUTER_RESPONSE_OVERHEAD_BYTES = 32;
const MAX_OUTER_RESPONSE_BODY_BYTES =
  Math.ceil((TRANSPORT_V2_LIMITS.envelopeBytes + MIN_ENCRYPTED_RECORD_BYTES) / 3) * 4 +
  OUTER_RESPONSE_OVERHEAD_BYTES;
const MAX_OUTER_REQUEST_BODY_BYTES = 50 * 1024 * 1024;

export interface PrepareTransportV2Request extends Omit<TransportV2RequestEnvelope, "requestId"> {}

export interface TransportV2HttpRequest {
  path: "/v2/request";
  method: "POST";
  headers: Readonly<Record<"content-type" | "x-session-id", string>>;
  body: string;
}

export class PreparedTransportV2Request {
  readonly requestId: string;
  readonly responseMode: ResponseMode;

  #responseContext: TransportV2ResponseContext;
  #httpRequest: TransportV2HttpRequest | null;
  #responseSelected = false;

  constructor(
    responseContext: TransportV2ResponseContext,
    requestId: string,
    responseMode: ResponseMode,
    httpRequest: TransportV2HttpRequest
  ) {
    this.#responseContext = responseContext;
    this.requestId = requestId;
    this.responseMode = responseMode;
    this.#httpRequest = httpRequest;
  }

  /**
   * Returns the one network send owned by this logical request. A sent request
   * is never recreated with the same or a fresh request ID by this engine.
   */
  takeHttpRequest(): TransportV2HttpRequest {
    if (!this.#httpRequest) {
      throw new TransportV2ProtocolError("Transport v2 request has already been taken for send.");
    }
    const request = this.#httpRequest;
    this.#httpRequest = null;
    return request;
  }

  decryptUnaryResponse(outerBody: string): TransportV2UnaryResponse {
    if (this.responseMode !== "unary") {
      throw new TransportV2ProtocolError("Transport v2 request did not select a unary response.");
    }
    this.#selectResponse();
    return this.#responseContext.decryptUnaryResponse(outerBody);
  }

  decryptPreStartUnaryError(outerBody: string): TransportV2UnaryResponse {
    if (this.responseMode !== "stream") {
      throw new TransportV2ProtocolError("Transport v2 request did not select streaming.");
    }
    this.#selectResponse();
    return this.#responseContext.decryptPreStartUnaryError(outerBody);
  }

  createStreamDecoder(): TransportV2StreamDecoder {
    if (this.responseMode !== "stream") {
      throw new TransportV2ProtocolError("Transport v2 request did not select streaming.");
    }
    this.#selectResponse();
    return this.#responseContext.createStreamDecoder();
  }

  dispose(): void {
    this.#httpRequest = null;
    this.#responseSelected = true;
    this.#responseContext.dispose();
  }

  #selectResponse(): void {
    if (this.#responseSelected) {
      throw new TransportV2ProtocolError("Transport v2 request response was already selected.");
    }
    this.#responseSelected = true;
  }
}

class TransportV2ResponseContext {
  #sessionId: string;
  #requestId: string;
  #responseKey: Uint8Array | null;
  #reserveChunkResponseRecord: () => void;
  #releasePreStartTerminalRecord: () => void;

  constructor(
    sessionId: string,
    requestId: string,
    responseKey: Uint8Array,
    reserveChunkResponseRecord: () => void,
    releasePreStartTerminalRecord: () => void
  ) {
    this.#sessionId = sessionId;
    this.#requestId = requestId;
    this.#responseKey = responseKey;
    this.#reserveChunkResponseRecord = reserveChunkResponseRecord;
    this.#releasePreStartTerminalRecord = releasePreStartTerminalRecord;
  }

  decryptUnaryResponse(outerBody: string): TransportV2UnaryResponse {
    return this.#decryptUnaryOuter(outerBody, false);
  }

  decryptPreStartUnaryError(outerBody: string): TransportV2UnaryResponse {
    return this.#decryptUnaryOuter(outerBody, true);
  }

  createStreamDecoder(): TransportV2StreamDecoder {
    const responseKey = this.#takeResponseKey();
    try {
      return new TransportV2StreamDecoder(
        this.#requestId,
        (encrypted, sequence) => {
          return decryptTransportV2Record(
            responseKey,
            encrypted,
            streamResponseRecordAad(this.#sessionId, this.#requestId, sequence),
            TRANSPORT_V2_LIMITS.envelopeBytes
          );
        },
        undefined,
        () => responseKey.fill(0),
        this.#reserveChunkResponseRecord
      );
    } catch (error) {
      responseKey.fill(0);
      throw error;
    }
  }

  dispose(): void {
    this.#responseKey?.fill(0);
    this.#responseKey = null;
  }

  #decryptUnaryOuter(outerBody: string, requireError: boolean): TransportV2UnaryResponse {
    const responseKey = this.#takeResponseKey();
    let encrypted: Uint8Array | undefined;
    let plaintext: Uint8Array | undefined;
    try {
      if (encodeUtf8(outerBody).length > MAX_OUTER_RESPONSE_BODY_BYTES) {
        throw new TransportV2ProtocolError("Transport v2 outer response exceeds its size limit.");
      }
      const outer = requireExactObject(
        parseStrictJson(outerBody),
        ["encrypted"],
        "Transport v2 outer response"
      );
      encrypted = decodeCanonicalBase64(
        typeof outer.encrypted === "string" ? outer.encrypted : "",
        TRANSPORT_V2_LIMITS.envelopeBytes + MIN_ENCRYPTED_RECORD_BYTES
      );
      plaintext = decryptTransportV2Record(
        responseKey,
        encrypted,
        unaryResponseRecordAad(this.#sessionId, this.#requestId),
        TRANSPORT_V2_LIMITS.envelopeBytes
      );
      const response = parseUnaryResponseEnvelope(plaintext);
      if (response.requestId !== this.#requestId) {
        zeroUnaryResponse(response);
        throw new TransportV2ProtocolError("Transport v2 unary response binding is invalid.");
      }
      if (requireError && (response.status < 400 || response.status > 599)) {
        zeroUnaryResponse(response);
        throw new TransportV2ProtocolError(
          "Transport v2 pre-stream unary response is not an error."
        );
      }
      if (requireError) this.#releasePreStartTerminalRecord();
      return response;
    } finally {
      encrypted?.fill(0);
      plaintext?.fill(0);
      responseKey.fill(0);
    }
  }

  #takeResponseKey(): Uint8Array {
    if (!this.#responseKey) {
      throw new TransportV2ProtocolError("Transport v2 response context is no longer available.");
    }
    const responseKey = this.#responseKey;
    this.#responseKey = null;
    return responseKey;
  }
}

export class TransportV2Session {
  readonly sessionId: string;
  readonly expiresAtUnixSeconds: number;

  #requestKey: Uint8Array;
  #responseKey: Uint8Array;
  #requestIds = new Set<string>();
  #requestRecords = 0;
  #responseRecords = 0;
  #responseRecordLimit: number;
  #disposed = false;

  constructor(handshake: TransportV2HandshakeResult, responseRecordLimit = MAX_RESPONSE_RECORDS) {
    uuidToBytes(handshake.sessionId);
    if (
      handshake.requestKey.length !== 32 ||
      handshake.responseKey.length !== 32 ||
      !Number.isSafeInteger(handshake.expiresAtUnixSeconds) ||
      handshake.expiresAtUnixSeconds < 0 ||
      !Number.isSafeInteger(responseRecordLimit) ||
      responseRecordLimit < 0 ||
      responseRecordLimit > MAX_RESPONSE_RECORDS
    ) {
      throw new TransportV2ProtocolError("Transport v2 handshake result is invalid.");
    }
    this.sessionId = handshake.sessionId;
    this.expiresAtUnixSeconds = handshake.expiresAtUnixSeconds;
    this.#requestKey = new Uint8Array(handshake.requestKey);
    this.#responseKey = new Uint8Array(handshake.responseKey);
    this.#responseRecordLimit = responseRecordLimit;
  }

  prepareRequest(
    input: PrepareTransportV2Request,
    random: Crypto = globalThis.crypto,
    nowUnixSeconds = Math.floor(Date.now() / 1000)
  ): PreparedTransportV2Request {
    this.#requireActive();
    if (nowUnixSeconds >= this.expiresAtUnixSeconds) {
      throw new TransportV2ProtocolError("Transport v2 session has expired.");
    }
    if (this.#requestRecords >= MAX_REQUEST_RECORDS) {
      throw new TransportV2ProtocolError("Transport v2 request record budget is exhausted.");
    }

    const expectedResponseRecords = input.responseMode === "stream" ? 2 : 1;
    this.#reserveResponseRecords(expectedResponseRecords);

    let requestId: string | undefined;
    try {
      for (let attempt = 0; attempt < 16; attempt += 1) {
        const candidate = generateRequestId(random);
        if (!this.#requestIds.has(candidate)) {
          requestId = candidate;
          break;
        }
      }
      if (!requestId) {
        throw new TransportV2ProtocolError("Secure request ID generation repeatedly collided.");
      }
      this.#requestIds.add(requestId);

      let plaintext: Uint8Array | undefined;
      let encrypted: Uint8Array | undefined;
      try {
        plaintext = serializeRequestEnvelope({ ...input, requestId });
        encrypted = encryptTransportV2Record(
          this.#requestKey,
          plaintext,
          requestRecordAad(this.sessionId),
          undefined,
          random
        );
        const outerBody = JSON.stringify({ encrypted: encodeCanonicalBase64(encrypted) });
        if (encodeUtf8(outerBody).length > MAX_OUTER_REQUEST_BODY_BYTES) {
          throw new TransportV2ProtocolError("Transport v2 outer request exceeds its size limit.");
        }
        this.#requestRecords += 1;
        return new PreparedTransportV2Request(
          new TransportV2ResponseContext(
            this.sessionId,
            requestId,
            new Uint8Array(this.#responseKey),
            () => this.#reserveResponseRecords(1),
            () => this.#releaseResponseRecords(1)
          ),
          requestId,
          input.responseMode,
          {
            path: "/v2/request",
            method: "POST",
            headers: { "content-type": "application/json", "x-session-id": this.sessionId },
            body: outerBody
          }
        );
      } catch (error) {
        this.#requestIds.delete(requestId);
        throw error;
      } finally {
        plaintext?.fill(0);
        encrypted?.fill(0);
      }
    } catch (error) {
      this.#releaseResponseRecords(expectedResponseRecords);
      throw error;
    }
  }

  get isDisposed(): boolean {
    return this.#disposed;
  }

  dispose(): void {
    if (this.#disposed) return;
    this.#disposed = true;
    this.#requestKey.fill(0);
    this.#responseKey.fill(0);
    this.#requestIds.clear();
  }

  #reserveResponseRecords(records: number): void {
    const nextResponseRecords = this.#responseRecords + records;
    if (
      !Number.isSafeInteger(records) ||
      records <= 0 ||
      !Number.isSafeInteger(nextResponseRecords) ||
      nextResponseRecords > this.#responseRecordLimit
    ) {
      throw new TransportV2ProtocolError("Transport v2 response record budget is exhausted.");
    }
    // This method contains no asynchronous boundary. The check and increment
    // therefore form one atomic reservation for all requests sharing this
    // JavaScript session object, including a stream's Start + terminal pair.
    this.#responseRecords = nextResponseRecords;
  }

  #releaseResponseRecords(records: number): void {
    if (!Number.isSafeInteger(records) || records <= 0 || records > this.#responseRecords) {
      throw new TransportV2ProtocolError("Transport v2 response reservation is invalid.");
    }
    this.#responseRecords -= records;
  }

  #requireActive(): void {
    if (this.#disposed) {
      throw new TransportV2ProtocolError("Transport v2 session is disposed.");
    }
  }
}

function zeroUnaryResponse(response: TransportV2UnaryResponse): void {
  response.body?.fill(0);
  for (const header of response.headers) header.value.fill(0);
}
