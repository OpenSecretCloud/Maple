import {
  TRANSPORT_V2_VERSION,
  TransportV2ProtocolError,
  decodeCanonicalBase64,
  decodeUtf8,
  encodeCanonicalBase64,
  encodeCanonicalOpaquePathSegment,
  encodeUtf8,
  parseStrictJson,
  requestIdToBytes,
  requireExactObject
} from "./encoding";

const KIB = 1024;
const MIB = KIB * KIB;

export const TRANSPORT_V2_LIMITS = Object.freeze({
  envelopeBytes: 50 * MIB,
  logicalBodyBytes: 28 * MIB,
  pathBytes: 4096,
  queryBytes: 8192,
  headerCount: 64,
  headerNameBytes: 128,
  headerValueBytes: 16 * KIB,
  aggregateHeaderBytes: 64 * KIB,
  credentialBytes: 16 * KIB,
  streamChunkBytes: 64 * KIB,
  streamErrorBytes: 16 * KIB
});

export type LogicalMethod = "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
export type ResponseMode = "unary" | "stream";
export type CredentialKind = "api_key" | "resumption";

export interface TransportV2Header {
  name: string;
  value: Uint8Array;
}

export interface TransportV2Credential {
  kind: CredentialKind;
  value: Uint8Array;
}

export interface TransportV2LogicalRequest {
  method: LogicalMethod;
  path: string;
  query: string | null;
  headers: readonly TransportV2Header[];
  body: Uint8Array | null;
}

export interface TransportV2RequestEnvelope {
  requestId: string;
  responseMode: ResponseMode;
  credential: TransportV2Credential | null;
  cacheNamespaceRoot: Uint8Array | null;
  request: TransportV2LogicalRequest;
}

export interface TransportV2UnaryResponse {
  requestId: string;
  status: number;
  headers: TransportV2Header[];
  body: Uint8Array | null;
}

export type TransportV2StreamRecord =
  | {
      kind: "start";
      requestId: string;
      sequence: number;
      status: number;
      headers: TransportV2Header[];
    }
  | {
      kind: "chunk";
      requestId: string;
      sequence: number;
      body: Uint8Array;
    }
  | { kind: "end"; requestId: string; sequence: number }
  | {
      kind: "error";
      requestId: string;
      sequence: number;
      status: number;
      body: Uint8Array;
    };

type WireHeader = { name: string; value_base64: string };

function checkByteLimit(value: string, limit: number, description: string): void {
  if (encodeUtf8(value).length > limit) {
    throw new TransportV2ProtocolError(`${description} exceeds its size limit.`);
  }
}

function isHexDigit(value: number): boolean {
  return (
    (value >= 0x30 && value <= 0x39) ||
    (value >= 0x41 && value <= 0x46) ||
    (value >= 0x61 && value <= 0x66)
  );
}

function decodePercentTriplet(value: string, index: number): number {
  if (
    value[index] !== "%" ||
    index + 2 >= value.length ||
    !isHexDigit(value.charCodeAt(index + 1)) ||
    !isHexDigit(value.charCodeAt(index + 2))
  ) {
    throw new TransportV2ProtocolError("Transport v2 URI contains invalid percent encoding.");
  }
  return Number.parseInt(value.slice(index + 1, index + 3), 16);
}

function decodedSegment(value: string): Uint8Array {
  const bytes: number[] = [];
  for (let index = 0; index < value.length;) {
    if (value[index] === "%") {
      bytes.push(decodePercentTriplet(value, index));
      index += 3;
    } else {
      bytes.push(value.charCodeAt(index));
      index += 1;
    }
  }
  return new Uint8Array(bytes);
}

function validateOpaquePath(path: string): boolean {
  for (const prefix of ["/protected/kv/", "/protected/api-keys/"] as const) {
    if (!path.startsWith(prefix)) continue;
    const segment = path.slice(prefix.length);
    if (!segment || segment.includes("/")) {
      throw new TransportV2ProtocolError("Transport v2 opaque path segment is invalid.");
    }
    const decoded = decodeUtf8(decodedSegment(segment));
    if (encodeCanonicalOpaquePathSegment(decoded) !== segment) {
      throw new TransportV2ProtocolError("Transport v2 opaque path segment is not canonical.");
    }
    return true;
  }
  return false;
}

function isUriPchar(byte: number): boolean {
  return (
    (byte >= 0x30 && byte <= 0x39) ||
    (byte >= 0x41 && byte <= 0x5a) ||
    (byte >= 0x61 && byte <= 0x7a) ||
    "-._~!$&'()*+,;=:@".includes(String.fromCharCode(byte))
  );
}

function validatePath(path: string): void {
  checkByteLimit(path, TRANSPORT_V2_LIMITS.pathBytes, "Transport v2 path");
  if (!path.startsWith("/") || path.startsWith("//")) {
    throw new TransportV2ProtocolError("Transport v2 path is not origin-relative.");
  }
  if (path.includes("?") || path.includes("#") || path.includes("\\")) {
    throw new TransportV2ProtocolError("Transport v2 path contains a forbidden delimiter.");
  }
  if (validateOpaquePath(path)) return;

  for (let index = 0; index < path.length;) {
    const byte = path.charCodeAt(index);
    if (byte === 0x25) {
      const decoded = decodePercentTriplet(path, index);
      if (decoded === 0x2f || decoded === 0x5c) {
        throw new TransportV2ProtocolError("Transport v2 path contains an encoded separator.");
      }
      index += 3;
      continue;
    }
    if (byte !== 0x2f && !isUriPchar(byte)) {
      throw new TransportV2ProtocolError("Transport v2 path contains an invalid character.");
    }
    index += 1;
  }

  for (const segment of path.split("/")) {
    const decoded = decodedSegment(segment);
    if (
      (decoded.length === 1 && decoded[0] === 0x2e) ||
      (decoded.length === 2 && decoded[0] === 0x2e && decoded[1] === 0x2e)
    ) {
      throw new TransportV2ProtocolError("Transport v2 path contains a dot-segment.");
    }
  }
}

function validateQuery(query: string): void {
  checkByteLimit(query, TRANSPORT_V2_LIMITS.queryBytes, "Transport v2 query");
  if (query.startsWith("?") || query.startsWith("#") || query.includes("#")) {
    throw new TransportV2ProtocolError("Transport v2 query contains a forbidden delimiter.");
  }
  for (let index = 0; index < query.length;) {
    const byte = query.charCodeAt(index);
    if (byte === 0x25) {
      decodePercentTriplet(query, index);
      index += 3;
      continue;
    }
    if (byte !== 0x2f && byte !== 0x3f && !isUriPchar(byte)) {
      throw new TransportV2ProtocolError("Transport v2 query contains an invalid character.");
    }
    index += 1;
  }
}

function isLowercaseHttpToken(name: string): boolean {
  return /^[a-z0-9!#$%&'*+.^_`|~-]+$/.test(name);
}

function validateAndEncodeHeaders(headers: readonly TransportV2Header[]): WireHeader[] {
  if (headers.length > TRANSPORT_V2_LIMITS.headerCount) {
    throw new TransportV2ProtocolError("Transport v2 has too many headers.");
  }
  let aggregateBytes = 0;
  return headers.map((header) => {
    const nameBytes = encodeUtf8(header.name).length;
    if (
      nameBytes === 0 ||
      nameBytes > TRANSPORT_V2_LIMITS.headerNameBytes ||
      !isLowercaseHttpToken(header.name)
    ) {
      throw new TransportV2ProtocolError("Transport v2 header name is invalid.");
    }
    if (header.value.length > TRANSPORT_V2_LIMITS.headerValueBytes) {
      throw new TransportV2ProtocolError("Transport v2 header value exceeds its size limit.");
    }
    if (header.value.some((byte) => byte === 0 || byte === 0x0a || byte === 0x0d)) {
      throw new TransportV2ProtocolError("Transport v2 header value is invalid.");
    }
    aggregateBytes += nameBytes + header.value.length;
    if (aggregateBytes > TRANSPORT_V2_LIMITS.aggregateHeaderBytes) {
      throw new TransportV2ProtocolError("Transport v2 headers exceed their aggregate limit.");
    }
    return { name: header.name, value_base64: encodeCanonicalBase64(header.value) };
  });
}

function parseHeaders(value: unknown): TransportV2Header[] {
  if (!Array.isArray(value) || value.length > TRANSPORT_V2_LIMITS.headerCount) {
    throw new TransportV2ProtocolError("Transport v2 response headers are invalid.");
  }
  const decoded: TransportV2Header[] = [];
  let aggregateBytes = 0;
  for (const candidate of value) {
    const header = requireExactObject(candidate, ["name", "value_base64"], "Transport v2 header");
    if (typeof header.name !== "string" || !isLowercaseHttpToken(header.name)) {
      throw new TransportV2ProtocolError("Transport v2 header name is invalid.");
    }
    const nameBytes = encodeUtf8(header.name).length;
    if (nameBytes > TRANSPORT_V2_LIMITS.headerNameBytes) {
      throw new TransportV2ProtocolError("Transport v2 header name exceeds its size limit.");
    }
    const bytes = decodeCanonicalBase64(
      requireString(header.value_base64, "Transport v2 header value"),
      TRANSPORT_V2_LIMITS.headerValueBytes
    );
    if (bytes.some((byte) => byte === 0 || byte === 0x0a || byte === 0x0d)) {
      bytes.fill(0);
      throw new TransportV2ProtocolError("Transport v2 header value is invalid.");
    }
    aggregateBytes += nameBytes + bytes.length;
    if (aggregateBytes > TRANSPORT_V2_LIMITS.aggregateHeaderBytes) {
      bytes.fill(0);
      throw new TransportV2ProtocolError("Transport v2 headers exceed their aggregate limit.");
    }
    decoded.push({ name: header.name, value: bytes });
  }
  return decoded;
}

function requireString(value: unknown, description: string): string {
  if (typeof value !== "string") {
    throw new TransportV2ProtocolError(`${description} is not text.`);
  }
  return value;
}

function requireSafeInteger(value: unknown, description: string): number {
  if (!Number.isSafeInteger(value) || (value as number) < 0) {
    throw new TransportV2ProtocolError(`${description} is not a safe non-negative integer.`);
  }
  return value as number;
}

function requireStatus(value: unknown, minimum: number, maximum: number): number {
  const status = requireSafeInteger(value, "Transport v2 response status");
  if (status < minimum || status > maximum) {
    throw new TransportV2ProtocolError("Transport v2 response status is invalid.");
  }
  return status;
}

function requireVersion(value: unknown): void {
  if (value !== TRANSPORT_V2_VERSION) {
    throw new TransportV2ProtocolError("Transport v2 record has the wrong version.");
  }
}

function requireRequestId(value: unknown): string {
  const requestId = requireString(value, "Transport v2 request ID");
  requestIdToBytes(requestId);
  return requestId;
}

function parseBody(value: unknown, limit: number, nullable: boolean): Uint8Array | null {
  if (value === null && nullable) return null;
  return decodeCanonicalBase64(requireString(value, "Transport v2 body"), limit);
}

export function serializeRequestEnvelope(envelope: TransportV2RequestEnvelope): Uint8Array {
  requestIdToBytes(envelope.requestId);
  if (!(["unary", "stream"] as const).includes(envelope.responseMode)) {
    throw new TransportV2ProtocolError("Transport v2 response mode is invalid.");
  }
  validatePath(envelope.request.path);
  if (!(["GET", "POST", "PUT", "PATCH", "DELETE"] as const).includes(envelope.request.method)) {
    throw new TransportV2ProtocolError("Transport v2 logical method is invalid.");
  }
  if (envelope.request.query !== null) validateQuery(envelope.request.query);
  const headers = validateAndEncodeHeaders(envelope.request.headers);
  if (
    envelope.request.body &&
    envelope.request.body.length > TRANSPORT_V2_LIMITS.logicalBodyBytes
  ) {
    throw new TransportV2ProtocolError("Transport v2 body exceeds its size limit.");
  }

  let credential: { kind: CredentialKind; value_base64: string } | null = null;
  if (envelope.credential) {
    if (!(["api_key", "resumption"] as const).includes(envelope.credential.kind)) {
      throw new TransportV2ProtocolError("Transport v2 credential kind is invalid.");
    }
    if (envelope.credential.value.length > TRANSPORT_V2_LIMITS.credentialBytes) {
      throw new TransportV2ProtocolError("Transport v2 credential exceeds its size limit.");
    }
    credential = {
      kind: envelope.credential.kind,
      value_base64: encodeCanonicalBase64(envelope.credential.value)
    };
  }
  if (envelope.cacheNamespaceRoot && envelope.cacheNamespaceRoot.length !== 32) {
    throw new TransportV2ProtocolError("Transport v2 cache namespace root must be 32 bytes.");
  }

  const wire = {
    version: TRANSPORT_V2_VERSION,
    request_id: envelope.requestId,
    response_mode: envelope.responseMode,
    credential,
    cache_namespace_root_base64: envelope.cacheNamespaceRoot
      ? encodeCanonicalBase64(envelope.cacheNamespaceRoot)
      : null,
    request: {
      method: envelope.request.method,
      path: envelope.request.path,
      query: envelope.request.query,
      headers,
      body_base64:
        envelope.request.body === null ? null : encodeCanonicalBase64(envelope.request.body)
    }
  };
  const bytes = encodeUtf8(JSON.stringify(wire));
  if (bytes.length > TRANSPORT_V2_LIMITS.envelopeBytes) {
    throw new TransportV2ProtocolError("Transport v2 envelope exceeds its size limit.");
  }
  return bytes;
}

export function parseUnaryResponseEnvelope(plaintext: Uint8Array): TransportV2UnaryResponse {
  if (plaintext.length > TRANSPORT_V2_LIMITS.envelopeBytes) {
    throw new TransportV2ProtocolError("Transport v2 response exceeds its size limit.");
  }
  const value = requireExactObject(
    parseStrictJson(decodeUtf8(plaintext)),
    ["version", "request_id", "status", "headers", "body_base64"],
    "Transport v2 unary response"
  );
  requireVersion(value.version);
  return {
    requestId: requireRequestId(value.request_id),
    status: requireStatus(value.status, 100, 599),
    headers: parseHeaders(value.headers),
    body: parseBody(value.body_base64, TRANSPORT_V2_LIMITS.logicalBodyBytes, true)
  };
}

export function parseStreamRecord(plaintext: Uint8Array): TransportV2StreamRecord {
  if (plaintext.length > TRANSPORT_V2_LIMITS.envelopeBytes) {
    throw new TransportV2ProtocolError("Transport v2 stream record exceeds its size limit.");
  }
  const parsed = parseStrictJson(decodeUtf8(plaintext));
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    throw new TransportV2ProtocolError("Transport v2 stream record is not an object.");
  }
  const kind = requireString((parsed as Record<string, unknown>).kind, "Transport v2 record kind");

  if (kind === "start") {
    const value = requireExactObject(
      parsed,
      ["version", "request_id", "sequence", "kind", "status", "headers"],
      "Transport v2 stream start"
    );
    requireVersion(value.version);
    const sequence = requireSafeInteger(value.sequence, "Transport v2 stream sequence");
    if (sequence !== 0) {
      throw new TransportV2ProtocolError("Transport v2 stream start sequence is invalid.");
    }
    return {
      kind,
      requestId: requireRequestId(value.request_id),
      sequence,
      status: requireStatus(value.status, 200, 299),
      headers: parseHeaders(value.headers)
    };
  }
  if (kind === "chunk") {
    const value = requireExactObject(
      parsed,
      ["version", "request_id", "sequence", "kind", "body_base64"],
      "Transport v2 stream chunk"
    );
    requireVersion(value.version);
    const sequence = requireSafeInteger(value.sequence, "Transport v2 stream sequence");
    if (sequence === 0) {
      throw new TransportV2ProtocolError("Transport v2 stream chunk sequence is invalid.");
    }
    return {
      kind,
      requestId: requireRequestId(value.request_id),
      sequence,
      body: parseBody(value.body_base64, TRANSPORT_V2_LIMITS.streamChunkBytes, false)!
    };
  }
  if (kind === "end") {
    const value = requireExactObject(
      parsed,
      ["version", "request_id", "sequence", "kind"],
      "Transport v2 stream end"
    );
    requireVersion(value.version);
    const sequence = requireSafeInteger(value.sequence, "Transport v2 stream sequence");
    if (sequence === 0) {
      throw new TransportV2ProtocolError("Transport v2 stream end sequence is invalid.");
    }
    return { kind, requestId: requireRequestId(value.request_id), sequence };
  }
  if (kind === "error") {
    const value = requireExactObject(
      parsed,
      ["version", "request_id", "sequence", "kind", "status", "body_base64"],
      "Transport v2 stream error"
    );
    requireVersion(value.version);
    const sequence = requireSafeInteger(value.sequence, "Transport v2 stream sequence");
    if (sequence === 0) {
      throw new TransportV2ProtocolError("Transport v2 stream error sequence is invalid.");
    }
    return {
      kind,
      requestId: requireRequestId(value.request_id),
      sequence,
      status: requireStatus(value.status, 400, 599),
      body: parseBody(value.body_base64, TRANSPORT_V2_LIMITS.streamErrorBytes, false)!
    };
  }
  throw new TransportV2ProtocolError("Transport v2 stream record kind is invalid.");
}
