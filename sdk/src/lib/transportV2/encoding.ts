import { decode, encode } from "@stablelib/base64";

export const TRANSPORT_V2_VERSION = 2 as const;
export const SESSION_ID_BYTES = 16;
export const REQUEST_ID_BYTES = 16;
export const SESSION_KEY_BYTES = 32;
export const RECORD_NONCE_BYTES = 12;
export const RECORD_TAG_BYTES = 16;
export const MIN_ENCRYPTED_RECORD_BYTES = RECORD_NONCE_BYTES + RECORD_TAG_BYTES;

const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const REQUEST_ID_PATTERN = /^[0-9a-f]{32}$/;
const JSON_WHITESPACE = new Set([" ", "\t", "\n", "\r"]);
const MAX_JSON_DEPTH = 64;

export class TransportV2ProtocolError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "TransportV2ProtocolError";
  }
}

export function concatBytes(...parts: readonly Uint8Array[]): Uint8Array {
  const length = parts.reduce((total, part) => total + part.length, 0);
  const output = new Uint8Array(length);
  let offset = 0;
  for (const part of parts) {
    output.set(part, offset);
    offset += part.length;
  }
  return output;
}

export function encodeUtf8(value: string): Uint8Array {
  return new TextEncoder().encode(value);
}

export function decodeUtf8(value: Uint8Array): string {
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(value);
  } catch {
    throw new TransportV2ProtocolError("Transport v2 record is not valid UTF-8.");
  }
}

export function encodeCanonicalBase64(value: Uint8Array): string {
  return encode(value);
}

export function decodeCanonicalBase64(value: string, maxDecodedBytes: number): Uint8Array {
  if (typeof value !== "string") {
    throw new TransportV2ProtocolError("Transport v2 field is not base64 text.");
  }

  const maximumEncodedLength = Math.ceil(maxDecodedBytes / 3) * 4;
  if (value.length > maximumEncodedLength) {
    throw new TransportV2ProtocolError("Transport v2 base64 field exceeds its size limit.");
  }

  let decoded: Uint8Array;
  try {
    decoded = decode(value);
  } catch {
    throw new TransportV2ProtocolError("Transport v2 field is not valid standard base64.");
  }
  if (decoded.length > maxDecodedBytes || encode(decoded) !== value) {
    decoded.fill(0);
    throw new TransportV2ProtocolError("Transport v2 field is not canonical padded base64.");
  }
  return decoded;
}

export function bytesToHex(value: Uint8Array): string {
  return Array.from(value, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

export function hexToBytes(value: string, expectedBytes: number): Uint8Array {
  if (
    value.length !== expectedBytes * 2 ||
    !Array.from(value).every((character) => /[0-9a-f]/.test(character))
  ) {
    throw new TransportV2ProtocolError("Transport v2 field is not canonical lowercase hex.");
  }
  const bytes = new Uint8Array(expectedBytes);
  for (let index = 0; index < expectedBytes; index += 1) {
    bytes[index] = Number.parseInt(value.slice(index * 2, index * 2 + 2), 16);
  }
  return bytes;
}

export function requestIdToBytes(requestId: string): Uint8Array {
  if (!REQUEST_ID_PATTERN.test(requestId)) {
    throw new TransportV2ProtocolError("Transport v2 request ID is not canonical.");
  }
  return hexToBytes(requestId, REQUEST_ID_BYTES);
}

export function generateRequestId(random = globalThis.crypto): string {
  if (!random?.getRandomValues) {
    throw new TransportV2ProtocolError("Secure randomness is unavailable.");
  }
  const bytes = new Uint8Array(REQUEST_ID_BYTES);
  random.getRandomValues(bytes);
  return bytesToHex(bytes);
}

export function uuidToBytes(uuid: string): Uint8Array {
  if (!UUID_PATTERN.test(uuid)) {
    throw new TransportV2ProtocolError("Transport v2 session ID is not canonical.");
  }
  return hexToBytes(uuid.replaceAll("-", ""), SESSION_ID_BYTES);
}

export function bytesToUuid(bytes: Uint8Array): string {
  if (bytes.length !== SESSION_ID_BYTES) {
    throw new TransportV2ProtocolError("Transport v2 session ID has an invalid length.");
  }
  const hex = bytesToHex(bytes);
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(
    16,
    20
  )}-${hex.slice(20)}`;
}

export function sequenceToBytes(sequence: number): Uint8Array {
  if (!Number.isSafeInteger(sequence) || sequence < 0) {
    throw new TransportV2ProtocolError("Transport v2 stream sequence is invalid.");
  }
  const bytes = new Uint8Array(8);
  new DataView(bytes.buffer).setBigUint64(0, BigInt(sequence), false);
  return bytes;
}

export function readSafeUint64(bytes: Uint8Array): number {
  if (bytes.length !== 8) {
    throw new TransportV2ProtocolError("Transport v2 integer has an invalid length.");
  }
  const value = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getBigUint64(
    0,
    false
  );
  if (value > BigInt(Number.MAX_SAFE_INTEGER)) {
    throw new TransportV2ProtocolError("Transport v2 integer exceeds the client range.");
  }
  return Number(value);
}

export function equalBytes(left: Uint8Array, right: Uint8Array): boolean {
  if (left.length !== right.length) return false;
  let difference = 0;
  for (let index = 0; index < left.length; index += 1) {
    difference |= left[index] ^ right[index];
  }
  return difference === 0;
}

export function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function requireExactObject(
  value: unknown,
  expectedKeys: readonly string[],
  description: string
): Record<string, unknown> {
  if (!isPlainObject(value)) {
    throw new TransportV2ProtocolError(`${description} is not an object.`);
  }
  const actualKeys = Object.keys(value).sort();
  const sortedExpected = [...expectedKeys].sort();
  if (
    actualKeys.length !== sortedExpected.length ||
    actualKeys.some((key, index) => key !== sortedExpected[index])
  ) {
    throw new TransportV2ProtocolError(`${description} has an unexpected shape.`);
  }
  return value;
}

/**
 * Parses JSON while rejecting duplicate object members before JSON.parse can
 * collapse them. The protocol's schemas validate the returned value next.
 */
export function parseStrictJson(input: string): unknown {
  let index = 0;

  const skipWhitespace = () => {
    while (index < input.length && JSON_WHITESPACE.has(input[index])) index += 1;
  };

  const parseString = (): string => {
    const start = index;
    if (input[index] !== '"') throw new TransportV2ProtocolError("Invalid transport v2 JSON.");
    index += 1;
    while (index < input.length) {
      const character = input[index];
      if (character === '"') {
        index += 1;
        try {
          return JSON.parse(input.slice(start, index)) as string;
        } catch {
          throw new TransportV2ProtocolError("Invalid transport v2 JSON string.");
        }
      }
      if (character === "\\") {
        index += 1;
        if (index >= input.length) break;
        if (input[index] === "u") {
          const unicode = input.slice(index + 1, index + 5);
          if (!/^[0-9a-fA-F]{4}$/.test(unicode)) {
            throw new TransportV2ProtocolError("Invalid transport v2 JSON escape.");
          }
          index += 5;
          continue;
        }
        if (!'"\\/bfnrt'.includes(input[index])) {
          throw new TransportV2ProtocolError("Invalid transport v2 JSON escape.");
        }
        index += 1;
        continue;
      }
      if (character.charCodeAt(0) < 0x20) {
        throw new TransportV2ProtocolError("Invalid transport v2 JSON string.");
      }
      index += 1;
    }
    throw new TransportV2ProtocolError("Unterminated transport v2 JSON string.");
  };

  const parseNumber = () => {
    const rest = input.slice(index);
    const match = /^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?/.exec(rest);
    if (!match) throw new TransportV2ProtocolError("Invalid transport v2 JSON number.");
    index += match[0].length;
  };

  const parseValue = (depth: number): void => {
    if (depth > MAX_JSON_DEPTH) {
      throw new TransportV2ProtocolError("Transport v2 JSON nesting is too deep.");
    }
    skipWhitespace();
    const character = input[index];
    if (character === "{") {
      index += 1;
      skipWhitespace();
      if (input[index] === "}") {
        index += 1;
        return;
      }
      const keys = new Set<string>();
      while (true) {
        skipWhitespace();
        const key = parseString();
        if (keys.has(key)) {
          throw new TransportV2ProtocolError("Transport v2 JSON contains a duplicate field.");
        }
        keys.add(key);
        skipWhitespace();
        if (input[index] !== ":") {
          throw new TransportV2ProtocolError("Invalid transport v2 JSON object.");
        }
        index += 1;
        parseValue(depth + 1);
        skipWhitespace();
        if (input[index] === "}") {
          index += 1;
          return;
        }
        if (input[index] !== ",") {
          throw new TransportV2ProtocolError("Invalid transport v2 JSON object.");
        }
        index += 1;
      }
    }
    if (character === "[") {
      index += 1;
      skipWhitespace();
      if (input[index] === "]") {
        index += 1;
        return;
      }
      while (true) {
        parseValue(depth + 1);
        skipWhitespace();
        if (input[index] === "]") {
          index += 1;
          return;
        }
        if (input[index] !== ",") {
          throw new TransportV2ProtocolError("Invalid transport v2 JSON array.");
        }
        index += 1;
      }
    }
    if (character === '"') {
      parseString();
      return;
    }
    for (const literal of ["true", "false", "null"] as const) {
      if (input.startsWith(literal, index)) {
        index += literal.length;
        return;
      }
    }
    parseNumber();
  };

  parseValue(0);
  skipWhitespace();
  if (index !== input.length) {
    throw new TransportV2ProtocolError("Invalid trailing transport v2 JSON data.");
  }
  try {
    return JSON.parse(input) as unknown;
  } catch {
    throw new TransportV2ProtocolError("Invalid transport v2 JSON.");
  }
}

export function encodeCanonicalOpaquePathSegment(value: string): string {
  const bytes = encodeUtf8(value);
  let result = "";
  for (const byte of bytes) {
    if (
      (byte >= 0x30 && byte <= 0x39) ||
      (byte >= 0x41 && byte <= 0x5a) ||
      (byte >= 0x61 && byte <= 0x7a)
    ) {
      result += String.fromCharCode(byte);
    } else {
      result += `%${byte.toString(16).toUpperCase().padStart(2, "0")}`;
    }
  }
  return result;
}
