import nacl from "tweetnacl";
import {
  SESSION_KEY_BYTES,
  TransportV2ProtocolError,
  decodeCanonicalBase64,
  encodeCanonicalBase64,
  encodeUtf8,
  parseStrictJson,
  requireExactObject,
  uuidToBytes
} from "./encoding";
import { decryptTransportV2Handshake } from "./crypto";
import { TransportV2Session } from "./session";

const MAX_ATTESTATION_NONCE_BYTES = 512;
const MAX_KEY_EXCHANGE_BODY_BYTES = 4 * 1024;
const HANDSHAKE_ENCRYPTED_RECORD_BYTES = 85;

export interface TransportV2KeyExchangeRequest {
  path: "/v2/key_exchange";
  method: "POST";
  headers: Readonly<Record<"content-type", "application/json">>;
  body: string;
}

export class TransportV2Handshake {
  #nonce: string;
  #clientPublicKey: Uint8Array;
  #clientSecretKey: Uint8Array;
  #requestTaken = false;
  #used = false;

  constructor(nonce: string) {
    const nonceBytes = encodeUtf8(nonce).length;
    if (nonceBytes === 0 || nonceBytes > MAX_ATTESTATION_NONCE_BYTES) {
      throw new TransportV2ProtocolError("Transport v2 attestation nonce has an invalid length.");
    }
    const keyPair = nacl.box.keyPair();
    this.#nonce = nonce;
    this.#clientPublicKey = new Uint8Array(keyPair.publicKey);
    this.#clientSecretKey = new Uint8Array(keyPair.secretKey);
    keyPair.secretKey.fill(0);
  }

  get clientPublicKey(): Uint8Array {
    return new Uint8Array(this.#clientPublicKey);
  }

  keyExchangeRequest(): TransportV2KeyExchangeRequest {
    if (this.#used || this.#requestTaken) {
      throw new TransportV2ProtocolError("Transport v2 handshake is already consumed.");
    }
    this.#requestTaken = true;
    const body = JSON.stringify({
      nonce: this.#nonce,
      client_public_key: encodeCanonicalBase64(this.#clientPublicKey)
    });
    if (encodeUtf8(body).length > MAX_KEY_EXCHANGE_BODY_BYTES) {
      throw new TransportV2ProtocolError("Transport v2 key exchange request is too large.");
    }
    return {
      path: "/v2/key_exchange",
      method: "POST",
      headers: { "content-type": "application/json" },
      body
    };
  }

  async complete(
    attestedServerPublicKey: Uint8Array,
    keyExchangeResponseBody: string,
    subtle: SubtleCrypto = globalThis.crypto.subtle,
    responseRecordLimit?: number
  ): Promise<TransportV2Session> {
    if (this.#used) {
      throw new TransportV2ProtocolError("Transport v2 handshake is already consumed.");
    }
    if (!this.#requestTaken) {
      throw new TransportV2ProtocolError("Transport v2 key exchange request was not taken.");
    }
    this.#used = true;
    this.#clientPublicKey.fill(0);
    if (
      attestedServerPublicKey.length !== SESSION_KEY_BYTES ||
      encodeUtf8(keyExchangeResponseBody).length > MAX_KEY_EXCHANGE_BODY_BYTES
    ) {
      this.dispose();
      throw new TransportV2ProtocolError("Transport v2 key exchange response is invalid.");
    }

    let sharedSecret: Uint8Array | undefined;
    try {
      const response = requireExactObject(
        parseStrictJson(keyExchangeResponseBody),
        ["session_id", "encrypted_session_key"],
        "Transport v2 key exchange response"
      );
      if (typeof response.session_id !== "string") {
        throw new TransportV2ProtocolError("Transport v2 key exchange session ID is invalid.");
      }
      uuidToBytes(response.session_id);
      const encrypted = decodeCanonicalBase64(
        typeof response.encrypted_session_key === "string" ? response.encrypted_session_key : "",
        HANDSHAKE_ENCRYPTED_RECORD_BYTES
      );
      try {
        if (encrypted.length !== HANDSHAKE_ENCRYPTED_RECORD_BYTES) {
          throw new TransportV2ProtocolError("Transport v2 key exchange record is invalid.");
        }
        sharedSecret = nacl.scalarMult(this.#clientSecretKey, attestedServerPublicKey);
        const handshake = await decryptTransportV2Handshake(
          sharedSecret,
          response.session_id,
          encrypted,
          subtle
        );
        try {
          return new TransportV2Session(handshake, responseRecordLimit);
        } finally {
          handshake.requestKey.fill(0);
          handshake.responseKey.fill(0);
        }
      } finally {
        encrypted.fill(0);
      }
    } finally {
      sharedSecret?.fill(0);
      this.dispose();
    }
  }

  dispose(): void {
    this.#used = true;
    this.#clientPublicKey.fill(0);
    this.#clientSecretKey.fill(0);
    this.#nonce = "";
  }
}
