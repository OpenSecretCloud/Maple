import { describe, expect, test } from "bun:test";
import vectors from "../../../testdata/transport-v2-golden-vectors.json";
import {
  TransportV2ProtocolError,
  TRANSPORT_V2_LIMITS,
  decodeCanonicalBase64,
  decryptTransportV2Handshake,
  decryptTransportV2Record,
  deriveTransportV2DirectionalKeys,
  encodeCanonicalBase64,
  encodeCanonicalOpaquePathSegment,
  encryptTransportV2Record,
  parseUnaryResponseEnvelope,
  requestRecordAad,
  serializeRequestEnvelope,
  streamResponseRecordAad,
  TransportV2Handshake,
  TransportV2StreamDecoder,
  TransportV2Session,
  unaryResponseRecordAad
} from "../transportV2";
import { encodeUtf8, hexToBytes } from "../transportV2/encoding";

const encoder = new TextEncoder();

function fixedRequestIdRandom(...requestIds: Uint8Array[]): Crypto {
  let index = 0;
  return {
    getRandomValues<T extends ArrayBufferView | null>(array: T): T {
      if (!array || !(array instanceof Uint8Array)) {
        throw new Error("unexpected random request");
      }
      if (array.length === 12) {
        return globalThis.crypto.getRandomValues(array) as T;
      }
      if (array.length !== 16 || index >= requestIds.length) {
        throw new Error("unexpected random request");
      }
      const value = requestIds[index];
      index += 1;
      if (value.length !== 16) throw new Error("unexpected request ID length");
      array.set(value);
      return array;
    }
  } as Crypto;
}

async function vectorKeys() {
  return deriveTransportV2DirectionalKeys(hexToBytes(vectors.session_master_hex, 32));
}

describe("transport v2 cross-language vectors", () => {
  test("derives and authenticates the frozen handshake and directional records", async () => {
    const sharedSecret = hexToBytes(vectors.shared_secret_hex, 32);
    const handshakeRecord = hexToBytes(vectors.handshake.record_hex, 85);
    const handshake = await decryptTransportV2Handshake(
      sharedSecret,
      vectors.session_id,
      handshakeRecord
    );
    expect(handshake.sessionId).toBe(vectors.session_id);
    expect(handshake.expiresAtUnixSeconds).toBe(vectors.expires_at_unix_seconds);
    expect(Buffer.from(handshake.requestKey).toString("hex")).toBe(vectors.request.derived_key_hex);
    expect(Buffer.from(handshake.responseKey).toString("hex")).toBe(
      vectors.unary_response.derived_key_hex
    );

    const requestPlaintext = encoder.encode(vectors.request.plaintext_utf8);
    const requestRecord = encryptTransportV2Record(
      handshake.requestKey,
      requestPlaintext,
      requestRecordAad(vectors.session_id),
      hexToBytes(vectors.request.nonce_hex, 12)
    );
    expect(Buffer.from(requestRecord).toString("hex")).toBe(vectors.request.record_hex);
    expect(encodeCanonicalBase64(requestRecord)).toBe(vectors.request.record_base64);

    const unaryRecord = encryptTransportV2Record(
      handshake.responseKey,
      encoder.encode(vectors.unary_response.plaintext_utf8),
      unaryResponseRecordAad(vectors.session_id, vectors.request_id_hex),
      hexToBytes(vectors.unary_response.nonce_hex, 12)
    );
    expect(Buffer.from(unaryRecord).toString("hex")).toBe(vectors.unary_response.record_hex);

    const streamRecord = encryptTransportV2Record(
      handshake.responseKey,
      encoder.encode(vectors.stream_response.plaintext_utf8),
      streamResponseRecordAad(vectors.session_id, vectors.request_id_hex, vectors.stream_sequence),
      hexToBytes(vectors.stream_response.nonce_hex, 12)
    );
    expect(Buffer.from(streamRecord).toString("hex")).toBe(vectors.stream_response.record_hex);

    const opened = decryptTransportV2Record(
      handshake.responseKey,
      streamRecord,
      streamResponseRecordAad(vectors.session_id, vectors.request_id_hex, vectors.stream_sequence),
      1024
    );
    expect(new TextDecoder().decode(opened)).toBe(vectors.stream_response.plaintext_utf8);
  });

  test("serializes the frozen absent-body and explicit-empty-body envelopes", () => {
    const withoutBody = serializeRequestEnvelope({
      requestId: vectors.request_id_hex,
      responseMode: "unary",
      credential: null,
      cacheNamespaceRoot: null,
      request: {
        method: "GET",
        path: "/v1/models",
        query: "limit=10",
        headers: [{ name: "x-provider-beta", value: encoder.encode("beta") }],
        body: null
      }
    });
    expect(new TextDecoder().decode(withoutBody)).toBe(vectors.request_without_body_json);

    const withEmptyBody = serializeRequestEnvelope({
      requestId: vectors.request_id_hex,
      responseMode: "unary",
      credential: null,
      cacheNamespaceRoot: null,
      request: {
        method: "POST",
        path: "/v1/responses",
        query: null,
        headers: [{ name: "content-type", value: encoder.encode("application/json") }],
        body: new Uint8Array(0)
      }
    });
    expect(new TextDecoder().decode(withEmptyBody)).toBe(vectors.request_with_empty_body_json);
  });

  test("serializes non-null credentials and cache roots without an outer credential", () => {
    const cacheNamespaceRoot = hexToBytes(
      "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
      32
    );
    for (const kind of ["api_key", "resumption"] as const) {
      const encoded = serializeRequestEnvelope({
        requestId: vectors.request_id_hex,
        responseMode: "unary",
        credential: { kind, value: encoder.encode("sk-test") },
        cacheNamespaceRoot,
        request: {
          method: "POST",
          path: "/v1/chat/completions",
          query: null,
          headers: [],
          body: null
        }
      });
      expect(new TextDecoder().decode(encoded)).toBe(
        `{"version":2,"request_id":"${vectors.request_id_hex}","response_mode":"unary","credential":{"kind":"${kind}","value_base64":"c2stdGVzdA=="},"cache_namespace_root_base64":"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=","request":{"method":"POST","path":"/v1/chat/completions","query":null,"headers":[],"body_base64":null}}`
      );
    }
  });

  test("rejects alternate base64 text and changed response bindings", async () => {
    const unpadded = vectors.request.record_base64.replace(/=+$/, "");
    expect(() => decodeCanonicalBase64(unpadded, 1024)).toThrow(TransportV2ProtocolError);

    const keys = await vectorKeys();
    expect(() =>
      decryptTransportV2Record(
        keys.responseKey,
        hexToBytes(vectors.unary_response.record_hex, 62),
        unaryResponseRecordAad(vectors.session_id, "00112233445566778899aabbccddeeff"),
        1024
      )
    ).toThrow(TransportV2ProtocolError);

    const requestRecord = hexToBytes(vectors.request.record_hex, 55);
    const unaryRecord = hexToBytes(vectors.unary_response.record_hex, 62);
    const streamRecord = hexToBytes(vectors.stream_response.record_hex, 61);
    const otherSessionId = "00112233-4455-6677-8899-aabbccddeefe";
    const otherRequestId = "00112233445566778899aabbccddeefe";
    const tampered = new Uint8Array(unaryRecord);
    tampered[tampered.length - 1] ^= 1;

    const rejected = [
      () =>
        decryptTransportV2Record(
          keys.responseKey,
          requestRecord,
          requestRecordAad(vectors.session_id),
          1024
        ),
      () =>
        decryptTransportV2Record(
          keys.requestKey,
          unaryRecord,
          unaryResponseRecordAad(vectors.session_id, vectors.request_id_hex),
          1024
        ),
      () =>
        decryptTransportV2Record(
          keys.responseKey,
          unaryRecord,
          unaryResponseRecordAad(otherSessionId, vectors.request_id_hex),
          1024
        ),
      () =>
        decryptTransportV2Record(
          keys.responseKey,
          unaryRecord,
          unaryResponseRecordAad(vectors.session_id, otherRequestId),
          1024
        ),
      () =>
        decryptTransportV2Record(
          keys.responseKey,
          streamRecord,
          streamResponseRecordAad(
            vectors.session_id,
            vectors.request_id_hex,
            vectors.stream_sequence + 1
          ),
          1024
        ),
      () =>
        decryptTransportV2Record(
          keys.responseKey,
          tampered,
          unaryResponseRecordAad(vectors.session_id, vectors.request_id_hex),
          1024
        ),
      () => decryptTransportV2Record(keys.responseKey, new Uint8Array(27), new Uint8Array(0), 1024)
    ];
    for (const reject of rejected) expect(reject).toThrow(TransportV2ProtocolError);
    await expect(
      decryptTransportV2Handshake(new Uint8Array(32), vectors.session_id, new Uint8Array(85))
    ).rejects.toBeInstanceOf(TransportV2ProtocolError);
  });
});

describe("transport v2 dormant session engine", () => {
  test("accepts the exact 50 MiB logical request boundary and rejects one byte more", () => {
    const body = new Uint8Array(TRANSPORT_V2_LIMITS.requestLogicalBodyBytes);
    const envelope = serializeRequestEnvelope({
      requestId: vectors.request_id_hex,
      responseMode: "unary",
      credential: null,
      cacheNamespaceRoot: null,
      request: {
        method: "POST",
        path: "/v1/responses",
        query: null,
        headers: [],
        body
      }
    });
    expect(envelope.length).toBeLessThanOrEqual(TRANSPORT_V2_LIMITS.requestEnvelopeBytes);
    body.fill(0);
    envelope.fill(0);

    const oversized = new Uint8Array(TRANSPORT_V2_LIMITS.requestLogicalBodyBytes + 1);
    expect(() =>
      serializeRequestEnvelope({
        requestId: vectors.request_id_hex,
        responseMode: "unary",
        credential: null,
        cacheNamespaceRoot: null,
        request: {
          method: "POST",
          path: "/v1/responses",
          query: null,
          headers: [],
          body: oversized
        }
      })
    ).toThrow("body exceeds its size limit");
    oversized.fill(0);
  });

  test("rejects an empty attestation nonce before generating key material", () => {
    expect(() => new TransportV2Handshake("")).toThrow("invalid length");

    const handshake = new TransportV2Handshake("nonce");
    const publicKey = handshake.clientPublicKey;
    publicKey.fill(0);
    const request = handshake.keyExchangeRequest();
    const requestPublicKey = JSON.parse(request.body) as { client_public_key: string };
    expect(requestPublicKey.client_public_key).not.toBe(encodeCanonicalBase64(publicKey));
    expect(() => handshake.keyExchangeRequest()).toThrow("already consumed");
    handshake.dispose();
  });

  test("owns exactly one send and authenticates one matching unary response", async () => {
    const keys = await vectorKeys();
    const session = new TransportV2Session(
      {
        sessionId: vectors.session_id,
        expiresAtUnixSeconds: vectors.expires_at_unix_seconds,
        ...keys
      },
      1
    );
    const requestIdBytes = hexToBytes(vectors.request_id_hex, 16);
    const prepared = session.prepareRequest(
      {
        responseMode: "unary",
        credential: null,
        cacheNamespaceRoot: null,
        request: {
          method: "GET",
          path: "/v1/models",
          query: null,
          headers: [],
          body: null
        }
      },
      fixedRequestIdRandom(requestIdBytes),
      vectors.expires_at_unix_seconds - 1
    );
    const outbound = prepared.takeHttpRequest();
    expect(outbound.path).toBe("/v2/request");
    expect(outbound.headers).toEqual({
      "content-type": "application/octet-stream",
      "x-session-id": vectors.session_id
    });
    expect(new TextDecoder().decode(outbound.body)).not.toContain("/v1/models");
    expect(() => prepared.takeHttpRequest()).toThrow("already been taken");
    expect(() => prepared.createStreamDecoder()).toThrow("did not select streaming");

    const responsePlaintext = encodeUtf8(
      JSON.stringify({
        version: 2,
        request_id: vectors.request_id_hex,
        status: 200,
        headers: [{ name: "content-type", value_base64: "YXBwbGljYXRpb24vanNvbg==" }],
        body_base64: "eyJvayI6dHJ1ZX0="
      })
    );
    const responseRecord = encryptTransportV2Record(
      keys.responseKey,
      responsePlaintext,
      unaryResponseRecordAad(vectors.session_id, vectors.request_id_hex)
    );
    session.dispose();
    expect(session.isDisposed).toBe(true);
    const response = prepared.decryptUnaryResponse(responseRecord);
    expect(response.status).toBe(200);
    expect(JSON.parse(new TextDecoder().decode(response.body!))).toEqual({ ok: true });
    expect(() => prepared.decryptUnaryResponse(new Uint8Array(0))).toThrow("already selected");
  });

  test("decodes arbitrary carrier splits and requires ordered authenticated finality", async () => {
    const keys = await vectorKeys();
    const session = new TransportV2Session(
      {
        sessionId: vectors.session_id,
        expiresAtUnixSeconds: vectors.expires_at_unix_seconds,
        ...keys
      },
      3
    );
    const prepared = session.prepareRequest(
      {
        responseMode: "stream",
        credential: null,
        cacheNamespaceRoot: null,
        request: {
          method: "POST",
          path: "/v1/responses",
          query: null,
          headers: [{ name: "content-type", value: encoder.encode("application/json") }],
          body: encoder.encode("{}")
        }
      },
      fixedRequestIdRandom(hexToBytes(vectors.request_id_hex, 16)),
      vectors.expires_at_unix_seconds - 1
    );
    prepared.takeHttpRequest();
    expect(() => prepared.decryptUnaryResponse(new Uint8Array(0))).toThrow(
      "did not select a unary"
    );
    const decoder = prepared.createStreamDecoder();
    session.dispose();

    const plaintexts = [
      JSON.stringify({
        version: 2,
        request_id: vectors.request_id_hex,
        sequence: 0,
        kind: "start",
        status: 200,
        headers: [{ name: "content-type", value_base64: "dGV4dC9ldmVudC1zdHJlYW0=" }]
      }),
      JSON.stringify({
        version: 2,
        request_id: vectors.request_id_hex,
        sequence: 1,
        kind: "chunk",
        body_base64: "ZGF0YTogaGkKCg=="
      }),
      JSON.stringify({
        version: 2,
        request_id: vectors.request_id_hex,
        sequence: 2,
        kind: "end"
      })
    ];
    const carrier = plaintexts
      .map((plaintext, sequence) => {
        const encrypted = encryptTransportV2Record(
          keys.responseKey,
          encodeUtf8(plaintext),
          streamResponseRecordAad(vectors.session_id, vectors.request_id_hex, sequence)
        );
        return `data: ${encodeCanonicalBase64(encrypted)}\n\n`;
      })
      .join("");
    const carrierBytes = encodeUtf8(carrier);
    const records = [
      ...decoder.push(carrierBytes.slice(0, 7)),
      ...decoder.push(carrierBytes.slice(7, 131)),
      ...decoder.push(carrierBytes.slice(131))
    ];
    expect(records.map((record) => record.kind)).toEqual(["start", "chunk", "end"]);
    expect(records[1].kind === "chunk" && new TextDecoder().decode(records[1].body)).toBe(
      "data: hi\n\n"
    );
    decoder.finish();
  });

  test("authenticates a streaming request's explicit pre-Start unary error", async () => {
    const keys = await vectorKeys();
    const session = new TransportV2Session(
      {
        sessionId: vectors.session_id,
        expiresAtUnixSeconds: vectors.expires_at_unix_seconds,
        ...keys
      },
      2
    );
    const prepared = session.prepareRequest(
      {
        responseMode: "stream",
        credential: null,
        cacheNamespaceRoot: null,
        request: {
          method: "POST",
          path: "/v1/responses",
          query: null,
          headers: [],
          body: encoder.encode("{}")
        }
      },
      fixedRequestIdRandom(hexToBytes(vectors.request_id_hex, 16)),
      vectors.expires_at_unix_seconds - 1
    );
    prepared.takeHttpRequest();
    const plaintext = encodeUtf8(
      JSON.stringify({
        version: 2,
        request_id: vectors.request_id_hex,
        status: 409,
        headers: [],
        body_base64: "eyJlcnJvciI6eyJjb2RlIjoicmVwbGF5X2RldGVjdGVkIn19"
      })
    );
    const encrypted = encryptTransportV2Record(
      keys.responseKey,
      plaintext,
      unaryResponseRecordAad(vectors.session_id, vectors.request_id_hex)
    );
    const afterErrorRandom = fixedRequestIdRandom(new Uint8Array(16).fill(0x5b));
    const unaryInput = {
      responseMode: "unary" as const,
      credential: null,
      cacheNamespaceRoot: null,
      request: {
        method: "GET" as const,
        path: "/v1/models",
        query: null,
        headers: [],
        body: null
      }
    };
    expect(() =>
      session.prepareRequest(unaryInput, afterErrorRandom, vectors.expires_at_unix_seconds - 1)
    ).toThrow("response record budget");
    const response = prepared.decryptPreStartUnaryError(encrypted);
    expect(response.status).toBe(409);
    expect(() => prepared.createStreamDecoder()).toThrow("already selected");
    const afterError = session.prepareRequest(
      unaryInput,
      afterErrorRandom,
      vectors.expires_at_unix_seconds - 1
    );
    expect(afterError.takeHttpRequest().path).toBe("/v2/request");
    afterError.dispose();
  });

  test("atomically reserves unary or stream response capacity before exposing a request", async () => {
    const keys = await vectorKeys();
    const session = new TransportV2Session(
      {
        sessionId: vectors.session_id,
        expiresAtUnixSeconds: vectors.expires_at_unix_seconds,
        ...keys
      },
      2
    );
    const unaryInput = {
      responseMode: "unary" as const,
      credential: null,
      cacheNamespaceRoot: null,
      request: {
        method: "GET" as const,
        path: "/v1/models",
        query: null,
        headers: [],
        body: null
      }
    };
    const streamInput = {
      ...unaryInput,
      responseMode: "stream" as const,
      request: {
        ...unaryInput.request,
        method: "POST" as const,
        path: "/v1/responses",
        body: encoder.encode("{}")
      }
    };

    const first = session.prepareRequest(
      unaryInput,
      fixedRequestIdRandom(new Uint8Array(16).fill(0x81)),
      vectors.expires_at_unix_seconds - 1
    );
    expect(first.takeHttpRequest().path).toBe("/v2/request");

    const finalSlotRandom = fixedRequestIdRandom(new Uint8Array(16).fill(0x83));
    expect(() =>
      session.prepareRequest(streamInput, finalSlotRandom, vectors.expires_at_unix_seconds - 1)
    ).toThrow("response record budget");
    const final = session.prepareRequest(
      unaryInput,
      finalSlotRandom,
      vectors.expires_at_unix_seconds - 1
    );
    expect(final.takeHttpRequest().path).toBe("/v2/request");
    expect(() =>
      session.prepareRequest(
        unaryInput,
        fixedRequestIdRandom(new Uint8Array(16).fill(0x85)),
        vectors.expires_at_unix_seconds - 1
      )
    ).toThrow("response record budget");
    first.dispose();
    final.dispose();
    expect(() =>
      session.prepareRequest(
        unaryInput,
        fixedRequestIdRandom(new Uint8Array(16).fill(0x87)),
        vectors.expires_at_unix_seconds - 1
      )
    ).toThrow("response record budget");
  });

  test("rolls back initial response capacity when request preparation fails", async () => {
    const keys = await vectorKeys();
    const session = new TransportV2Session(
      {
        sessionId: vectors.session_id,
        expiresAtUnixSeconds: vectors.expires_at_unix_seconds,
        ...keys
      },
      1
    );
    const requestId = new Uint8Array(16).fill(0x89);
    const invalidInput = {
      responseMode: "unary" as const,
      credential: null,
      cacheNamespaceRoot: null,
      request: {
        method: "GET" as const,
        path: "not-origin-relative",
        query: null,
        headers: [],
        body: null
      }
    };
    expect(() =>
      session.prepareRequest(
        invalidInput,
        fixedRequestIdRandom(requestId),
        vectors.expires_at_unix_seconds - 1
      )
    ).toThrow("origin-relative");

    const prepared = session.prepareRequest(
      {
        ...invalidInput,
        request: { ...invalidInput.request, path: "/v1/models" }
      },
      fixedRequestIdRandom(requestId),
      vectors.expires_at_unix_seconds - 1
    );
    expect(prepared.takeHttpRequest().path).toBe("/v2/request");
    prepared.dispose();
  });

  test("allows exactly one concurrent request to reserve the final response slot", async () => {
    const keys = await vectorKeys();
    const session = new TransportV2Session(
      {
        sessionId: vectors.session_id,
        expiresAtUnixSeconds: vectors.expires_at_unix_seconds,
        ...keys
      },
      1
    );
    const input = {
      responseMode: "unary" as const,
      credential: null,
      cacheNamespaceRoot: null,
      request: {
        method: "GET" as const,
        path: "/v1/models",
        query: null,
        headers: [],
        body: null
      }
    };
    const attempts = await Promise.allSettled([
      Promise.resolve().then(() =>
        session.prepareRequest(
          input,
          fixedRequestIdRandom(new Uint8Array(16).fill(0x91)),
          vectors.expires_at_unix_seconds - 1
        )
      ),
      Promise.resolve().then(() =>
        session.prepareRequest(
          input,
          fixedRequestIdRandom(new Uint8Array(16).fill(0x93)),
          vectors.expires_at_unix_seconds - 1
        )
      )
    ]);
    const fulfilled = attempts.filter(
      (attempt): attempt is PromiseFulfilledResult<ReturnType<typeof session.prepareRequest>> =>
        attempt.status === "fulfilled"
    );
    const rejected = attempts.filter(
      (attempt): attempt is PromiseRejectedResult => attempt.status === "rejected"
    );
    expect(fulfilled).toHaveLength(1);
    expect(rejected).toHaveLength(1);
    expect(rejected[0].reason).toBeInstanceOf(TransportV2ProtocolError);
    expect((rejected[0].reason as Error).message).toContain("response record budget");
    expect(fulfilled[0].value.takeHttpRequest().path).toBe("/v2/request");
    fulfilled[0].value.dispose();
  });

  test("releases explicitly abandoned prepared and streaming response contexts", async () => {
    const keys = await vectorKeys();
    const session = new TransportV2Session({
      sessionId: vectors.session_id,
      expiresAtUnixSeconds: vectors.expires_at_unix_seconds,
      ...keys
    });
    const prepared = session.prepareRequest(
      {
        responseMode: "unary",
        credential: null,
        cacheNamespaceRoot: null,
        request: {
          method: "GET",
          path: "/v1/models",
          query: null,
          headers: [],
          body: null
        }
      },
      fixedRequestIdRandom(hexToBytes(vectors.request_id_hex, 16)),
      vectors.expires_at_unix_seconds - 1
    );
    prepared.dispose();
    expect(() => prepared.takeHttpRequest()).toThrow("already been taken");
    expect(() => prepared.decryptUnaryResponse(new Uint8Array(0))).toThrow("already selected");

    let releases = 0;
    const decoder = new TransportV2StreamDecoder(
      vectors.request_id_hex,
      () => new Uint8Array(0),
      undefined,
      () => {
        releases += 1;
      }
    );
    decoder.dispose();
    decoder.dispose();
    expect(releases).toBe(1);
    expect(() => decoder.push(encoder.encode("x"))).toThrow("failed closed");
  });

  test("fails closed on duplicate JSON fields and stream EOF before terminal", async () => {
    const duplicate = encodeUtf8(
      `{"version":2,"request_id":"${vectors.request_id_hex}","status":200,"status":201,"headers":[],"body_base64":null}`
    );
    expect(() => parseUnaryResponseEnvelope(duplicate)).toThrow("duplicate field");

    const keys = await vectorKeys();
    const session = new TransportV2Session({
      sessionId: vectors.session_id,
      expiresAtUnixSeconds: vectors.expires_at_unix_seconds,
      ...keys
    });
    const prepared = session.prepareRequest(
      {
        responseMode: "stream",
        credential: null,
        cacheNamespaceRoot: null,
        request: {
          method: "POST",
          path: "/v1/responses",
          query: null,
          headers: [],
          body: encoder.encode("{}")
        }
      },
      fixedRequestIdRandom(hexToBytes(vectors.request_id_hex, 16)),
      vectors.expires_at_unix_seconds - 1
    );
    prepared.takeHttpRequest();
    const decoder = prepared.createStreamDecoder();
    const start = encodeUtf8(
      JSON.stringify({
        version: 2,
        request_id: vectors.request_id_hex,
        sequence: 0,
        kind: "start",
        status: 200,
        headers: []
      })
    );
    const encrypted = encryptTransportV2Record(
      keys.responseKey,
      start,
      streamResponseRecordAad(vectors.session_id, vectors.request_id_hex, 0)
    );
    decoder.push(encodeUtf8(`data: ${encodeCanonicalBase64(encrypted)}\n\n`));
    expect(() => decoder.finish()).toThrow("without an authenticated terminal");
  });

  test("enforces the cumulative logical stream bound independent of chunk boundaries", async () => {
    const keys = await vectorKeys();
    const plaintexts = [
      JSON.stringify({
        version: 2,
        request_id: vectors.request_id_hex,
        sequence: 0,
        kind: "start",
        status: 200,
        headers: []
      }),
      JSON.stringify({
        version: 2,
        request_id: vectors.request_id_hex,
        sequence: 1,
        kind: "chunk",
        body_base64: "YWJj"
      }),
      JSON.stringify({
        version: 2,
        request_id: vectors.request_id_hex,
        sequence: 2,
        kind: "chunk",
        body_base64: "ZGU="
      }),
      JSON.stringify({
        version: 2,
        request_id: vectors.request_id_hex,
        sequence: 3,
        kind: "end"
      })
    ];
    const frames = plaintexts.map((plaintext, sequence) => {
      const encrypted = encryptTransportV2Record(
        keys.responseKey,
        encodeUtf8(plaintext),
        streamResponseRecordAad(vectors.session_id, vectors.request_id_hex, sequence)
      );
      return encodeUtf8(`data: ${encodeCanonicalBase64(encrypted)}\n\n`);
    });
    const decrypt = (encrypted: Uint8Array, sequence: number) =>
      decryptTransportV2Record(
        keys.responseKey,
        encrypted,
        streamResponseRecordAad(vectors.session_id, vectors.request_id_hex, sequence),
        1024
      );

    const boundary = new TransportV2StreamDecoder(vectors.request_id_hex, decrypt, 5);
    for (const frame of frames) boundary.push(frame);
    boundary.finish();

    let reservedChunks = 0;
    const oversized = new TransportV2StreamDecoder(
      vectors.request_id_hex,
      decrypt,
      4,
      () => {},
      () => {
        reservedChunks += 1;
      }
    );
    oversized.push(frames[0]);
    oversized.push(frames[1]);
    expect(() => oversized.push(frames[2])).toThrow("logical stream exceeds");
    expect(reservedChunks).toBe(2);
    expect(() => oversized.push(frames[3])).toThrow("failed closed");
  });

  test("accepts authenticated Error as the sole stream terminal", async () => {
    const keys = await vectorKeys();
    const records = [
      JSON.stringify({
        version: 2,
        request_id: vectors.request_id_hex,
        sequence: 0,
        kind: "start",
        status: 200,
        headers: []
      }),
      JSON.stringify({
        version: 2,
        request_id: vectors.request_id_hex,
        sequence: 1,
        kind: "error",
        status: 500,
        body_base64: "eyJlcnJvciI6eyJjb2RlIjoic3RyZWFtX2ZhaWxlZCJ9fQ=="
      })
    ];
    const decrypt = (encrypted: Uint8Array, sequence: number) =>
      decryptTransportV2Record(
        keys.responseKey,
        encrypted,
        streamResponseRecordAad(vectors.session_id, vectors.request_id_hex, sequence),
        1024
      );
    const decoder = new TransportV2StreamDecoder(vectors.request_id_hex, decrypt);
    for (const [sequence, plaintext] of records.entries()) {
      const encrypted = encryptTransportV2Record(
        keys.responseKey,
        encodeUtf8(plaintext),
        streamResponseRecordAad(vectors.session_id, vectors.request_id_hex, sequence)
      );
      decoder.push(encodeUtf8(`data: ${encodeCanonicalBase64(encrypted)}\n\n`));
    }
    decoder.finish();
    expect(() => decoder.push(encodeUtf8("data: AA==\n\n"))).toThrow("data after");
    expect(() => decoder.finish()).toThrow("failed closed");
  });

  test("uses the backend's byte-exact opaque segment codec", () => {
    expect(encodeCanonicalOpaquePathSegment("Production Key-1_test/é")).toBe(
      "Production%20Key%2D1%5Ftest%2F%C3%A9"
    );
  });
});
