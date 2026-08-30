import { ChaCha20Poly1305 } from "@stablelib/chacha20poly1305";
import {
  MIN_ENCRYPTED_RECORD_BYTES,
  RECORD_NONCE_BYTES,
  SESSION_KEY_BYTES,
  TRANSPORT_V2_VERSION,
  TransportV2ProtocolError,
  bytesToUuid,
  concatBytes,
  encodeUtf8,
  equalBytes,
  readSafeUint64,
  requestIdToBytes,
  sequenceToBytes,
  uuidToBytes
} from "./encoding";

const HANDSHAKE_KEY_INFO = encodeUtf8("opensecret/transport-v2/handshake-key");
const REQUEST_KEY_INFO = encodeUtf8("opensecret/transport-v2/client-request");
const RESPONSE_KEY_INFO = encodeUtf8("opensecret/transport-v2/enclave-response");

const KEY_EXCHANGE_AAD = encodeUtf8("opensecret/transport-v2/key-exchange");
const REQUEST_RECORD_AAD = encodeUtf8("opensecret/transport-v2/request-record");
const UNARY_RESPONSE_RECORD_AAD = encodeUtf8("opensecret/transport-v2/unary-response-record");
const STREAM_RESPONSE_RECORD_AAD = encodeUtf8("opensecret/transport-v2/stream-response-record");

const HANDSHAKE_PAYLOAD_BYTES = 1 + 16 + SESSION_KEY_BYTES + 8;
const HANDSHAKE_RECORD_BYTES = RECORD_NONCE_BYTES + HANDSHAKE_PAYLOAD_BYTES + 16;

export interface TransportV2DirectionalKeys {
  requestKey: Uint8Array;
  responseKey: Uint8Array;
}

export interface TransportV2HandshakeResult extends TransportV2DirectionalKeys {
  sessionId: string;
  expiresAtUnixSeconds: number;
}

async function hkdfSha256(
  inputKeyMaterial: Uint8Array,
  info: Uint8Array,
  subtle: SubtleCrypto = globalThis.crypto.subtle
): Promise<Uint8Array> {
  if (inputKeyMaterial.length === 0) {
    throw new TransportV2ProtocolError("Transport v2 key material is empty.");
  }
  try {
    const key = await subtle.importKey("raw", inputKeyMaterial, "HKDF", false, ["deriveBits"]);
    const bits = await subtle.deriveBits(
      { name: "HKDF", hash: "SHA-256", salt: new Uint8Array(0), info },
      key,
      SESSION_KEY_BYTES * 8
    );
    return new Uint8Array(bits);
  } catch {
    throw new TransportV2ProtocolError("Transport v2 key derivation failed.");
  }
}

export async function deriveTransportV2DirectionalKeys(
  sessionMaster: Uint8Array,
  subtle: SubtleCrypto = globalThis.crypto.subtle
): Promise<TransportV2DirectionalKeys> {
  if (sessionMaster.length !== SESSION_KEY_BYTES) {
    throw new TransportV2ProtocolError("Transport v2 session master has an invalid length.");
  }
  const requestKey = await hkdfSha256(sessionMaster, REQUEST_KEY_INFO, subtle);
  try {
    const responseKey = await hkdfSha256(sessionMaster, RESPONSE_KEY_INFO, subtle);
    return { requestKey, responseKey };
  } catch (error) {
    requestKey.fill(0);
    throw error;
  }
}

async function deriveHandshakeKey(
  sharedSecret: Uint8Array,
  subtle: SubtleCrypto = globalThis.crypto.subtle
): Promise<Uint8Array> {
  if (sharedSecret.length !== SESSION_KEY_BYTES || sharedSecret.every((byte) => byte === 0)) {
    throw new TransportV2ProtocolError("Transport v2 key exchange is non-contributory.");
  }
  return hkdfSha256(sharedSecret, HANDSHAKE_KEY_INFO, subtle);
}

function requireRecordKey(key: Uint8Array): void {
  if (key.length !== SESSION_KEY_BYTES) {
    throw new TransportV2ProtocolError("Transport v2 record key has an invalid length.");
  }
}

export function encryptTransportV2Record(
  key: Uint8Array,
  plaintext: Uint8Array,
  aad: Uint8Array,
  nonce?: Uint8Array,
  random = globalThis.crypto
): Uint8Array {
  requireRecordKey(key);
  const recordNonce = nonce ? new Uint8Array(nonce) : new Uint8Array(RECORD_NONCE_BYTES);
  if (recordNonce.length !== RECORD_NONCE_BYTES) {
    throw new TransportV2ProtocolError("Transport v2 record nonce has an invalid length.");
  }
  if (!nonce) {
    if (!random?.getRandomValues) {
      throw new TransportV2ProtocolError("Secure randomness is unavailable.");
    }
    random.getRandomValues(recordNonce);
  }

  const cipher = new ChaCha20Poly1305(key);
  try {
    return concatBytes(recordNonce, cipher.seal(recordNonce, plaintext, aad));
  } catch {
    throw new TransportV2ProtocolError("Transport v2 record encryption failed.");
  } finally {
    cipher.clean();
  }
}

export function decryptTransportV2Record(
  key: Uint8Array,
  record: Uint8Array,
  aad: Uint8Array,
  maxPlaintextBytes: number
): Uint8Array {
  requireRecordKey(key);
  if (
    record.length < MIN_ENCRYPTED_RECORD_BYTES ||
    record.length > maxPlaintextBytes + MIN_ENCRYPTED_RECORD_BYTES
  ) {
    throw new TransportV2ProtocolError("Transport v2 encrypted record has an invalid length.");
  }
  const nonce = record.subarray(0, RECORD_NONCE_BYTES);
  const ciphertext = record.subarray(RECORD_NONCE_BYTES);
  const cipher = new ChaCha20Poly1305(key);
  try {
    const plaintext = cipher.open(nonce, ciphertext, aad);
    if (!plaintext || plaintext.length > maxPlaintextBytes) {
      plaintext?.fill(0);
      throw new TransportV2ProtocolError("Transport v2 record authentication failed.");
    }
    return plaintext;
  } catch (error) {
    if (error instanceof TransportV2ProtocolError) throw error;
    throw new TransportV2ProtocolError("Transport v2 record authentication failed.");
  } finally {
    cipher.clean();
  }
}

export function requestRecordAad(sessionId: string): Uint8Array {
  return concatBytes(REQUEST_RECORD_AAD, new Uint8Array([0]), uuidToBytes(sessionId));
}

export function unaryResponseRecordAad(sessionId: string, requestId: string): Uint8Array {
  return concatBytes(
    UNARY_RESPONSE_RECORD_AAD,
    new Uint8Array([0]),
    uuidToBytes(sessionId),
    requestIdToBytes(requestId)
  );
}

export function streamResponseRecordAad(
  sessionId: string,
  requestId: string,
  sequence: number
): Uint8Array {
  return concatBytes(
    STREAM_RESPONSE_RECORD_AAD,
    new Uint8Array([0]),
    uuidToBytes(sessionId),
    requestIdToBytes(requestId),
    sequenceToBytes(sequence)
  );
}

export async function decryptTransportV2Handshake(
  sharedSecret: Uint8Array,
  outerSessionId: string,
  encryptedRecord: Uint8Array,
  subtle: SubtleCrypto = globalThis.crypto.subtle
): Promise<TransportV2HandshakeResult> {
  const outerSessionBytes = uuidToBytes(outerSessionId);
  if (encryptedRecord.length !== HANDSHAKE_RECORD_BYTES) {
    throw new TransportV2ProtocolError("Transport v2 handshake record has an invalid length.");
  }
  const handshakeKey = await deriveHandshakeKey(sharedSecret, subtle);
  let payload: Uint8Array | undefined;
  try {
    payload = decryptTransportV2Record(
      handshakeKey,
      encryptedRecord,
      KEY_EXCHANGE_AAD,
      HANDSHAKE_PAYLOAD_BYTES
    );
    if (payload.length !== HANDSHAKE_PAYLOAD_BYTES || payload[0] !== TRANSPORT_V2_VERSION) {
      throw new TransportV2ProtocolError("Transport v2 handshake payload is invalid.");
    }
    const innerSessionBytes = payload.subarray(1, 17);
    if (!equalBytes(outerSessionBytes, innerSessionBytes)) {
      throw new TransportV2ProtocolError("Transport v2 handshake session IDs do not match.");
    }
    const sessionId = bytesToUuid(innerSessionBytes);
    const sessionMaster = new Uint8Array(payload.subarray(17, 49));
    const expiresAtUnixSeconds = readSafeUint64(payload.subarray(49, 57));
    try {
      const keys = await deriveTransportV2DirectionalKeys(sessionMaster, subtle);
      return { sessionId, expiresAtUnixSeconds, ...keys };
    } finally {
      sessionMaster.fill(0);
    }
  } finally {
    payload?.fill(0);
    handshakeKey.fill(0);
  }
}
