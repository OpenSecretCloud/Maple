import { afterEach, describe, expect, test } from "bun:test";
import { decode, encode } from "@stablelib/base64";
import nacl from "tweetnacl";
import type { AttestationDocument } from "../attestation";
import {
  clearTransportV2CacheRoot,
  clearTransportV2Credentials,
  installTransportV2Credentials,
  readTransportV2Credentials,
  snapshotTransportV2Auth,
  subscribeTransportV2AuthInvalidation
} from "../transportV2/auth";
import {
  TransportV2Client,
  type TransportV2ClientDependencies,
  type TransportV2FetchInput
} from "../transportV2/client";
import {
  TRANSPORT_V2_LIMITS,
  decryptTransportV2Record,
  deriveTransportV2DirectionalKeys,
  encodeCanonicalBase64,
  encryptTransportV2Record,
  requestRecordAad,
  streamResponseRecordAad,
  unaryResponseRecordAad
} from "../transportV2";
import { encodeUtf8, uuidToBytes } from "../transportV2/encoding";

const API_URL = "http://127.0.0.1:3010/base";
const USER_ID = "00112233-4455-6677-8899-aabbccddeeff";
const HANDSHAKE_INFO = encodeUtf8("opensecret/transport-v2/handshake-key");
const HANDSHAKE_AAD = encodeUtf8("opensecret/transport-v2/key-exchange");

type WireRequest = {
  request_id: string;
  response_mode: "unary" | "stream";
  credential: { kind: "api_key" | "resumption"; value_base64: string } | null;
  cache_namespace_root_base64: string | null;
  request: {
    method: string;
    path: string;
    query: string | null;
    headers: Array<{ name: string; value_base64: string }>;
    body_base64: string | null;
  };
};

type ServerSession = {
  requestKey: Uint8Array;
  responseKey: Uint8Array;
  expiresAt: number;
  authority: "anonymous" | "user" | "api_key";
};

function writeU64(value: number): Uint8Array {
  const bytes = new Uint8Array(8);
  new DataView(bytes.buffer).setBigUint64(0, BigInt(value), false);
  return bytes;
}

async function handshakeKey(sharedSecret: Uint8Array): Promise<Uint8Array> {
  const material = await crypto.subtle.importKey("raw", sharedSecret, "HKDF", false, [
    "deriveBits"
  ]);
  return new Uint8Array(
    await crypto.subtle.deriveBits(
      { name: "HKDF", hash: "SHA-256", salt: new Uint8Array(0), info: HANDSHAKE_INFO },
      material,
      256
    )
  );
}

function token(kind: "access_descriptor" | "resumption", principal = USER_ID): string {
  const audience =
    kind === "access_descriptor"
      ? "urn:opensecret:internal:transport-v2:user:access-descriptor"
      : "urn:opensecret:internal:transport-v2:user:resumption";
  const payload = Buffer.from(
    JSON.stringify({
      iss: "urn:opensecret:transport-v2",
      aud: audience,
      tv: 2,
      tk: kind,
      pk: "user",
      sub: principal,
      exp: Math.floor(Date.now() / 1000) + 3600
    })
  ).toString("base64url");
  return `e30.${payload}.c2ln`;
}

class V2TestServer {
  readonly keyPair = nacl.box.keyPair();
  readonly sessions = new Map<string, ServerSession>();
  readonly requests: WireRequest[] = [];
  readonly outerUrls: string[] = [];
  attestationCount = 0;
  keyExchangeCount = 0;
  requestCount = 0;
  failKeyExchanges = 0;
  failAfterPath: string | null = null;
  unauthorizedOncePath: string | null = null;
  sessionExhaustedOncePath: string | null = null;
  preStartSessionExhaustedOncePath: string | null = null;
  streamSessionExhaustedOncePath: string | null = null;
  genericServiceUnavailableOncePath: string | null = null;
  appendStreamRecordAfterEnd = false;
  streamTrailingGate: Promise<void> | null = null;
  pauseAfterPath: string | null = null;
  pauseReached: (() => void) | null = null;
  pauseRelease: Promise<void> | null = null;
  sessionLifetimeSeconds = 3600;
  #nextSession = 1;

  dependencies(sessionResponseRecordLimit?: number): TransportV2ClientDependencies {
    return {
      fetch: this.fetch,
      crypto: globalThis.crypto,
      verifyAttestationDocument: async () =>
        ({
          module_id: "test-enclave",
          digest: "SHA384",
          timestamp: Date.now(),
          pcrs: new Map(),
          certificate: new Uint8Array(1),
          cabundle: [],
          public_key: new Uint8Array(this.keyPair.publicKey),
          user_data: null,
          nonce: null
        }) as AttestationDocument,
      validatePcr0Hash: async () => {
        throw new Error("loopback tests must not call remote PCR validation");
      },
      sessionResponseRecordLimit
    };
  }

  fetch = async (input: string | URL | Request, init?: RequestInit): Promise<Response> => {
    const url = input.toString();
    this.outerUrls.push(url);
    const headers = new Headers(init?.headers);
    expect(headers.has("authorization")).toBe(false);
    if (url.includes("/v2/attestation/")) {
      this.attestationCount += 1;
      expect(init?.method).toBe("GET");
      return Response.json({ attestation_document: "test-document" });
    }
    if (url.endsWith("/v2/key_exchange")) {
      this.keyExchangeCount += 1;
      if (this.failKeyExchanges > 0) {
        this.failKeyExchanges -= 1;
        throw new Error("key exchange unavailable");
      }
      expect(headers.get("content-type")).toBe("application/json");
      return this.#keyExchange(String(init?.body));
    }
    if (url.endsWith("/v2/request")) {
      this.requestCount += 1;
      expect(headers.get("content-type")).toBe("application/octet-stream");
      expect(init?.body).toBeInstanceOf(Uint8Array);
      const body = new Uint8Array(init?.body as Uint8Array);
      return this.#request(headers.get("x-session-id") ?? "", body);
    }
    throw new Error(`unexpected outer URL: ${url}`);
  };

  async #keyExchange(body: string): Promise<Response> {
    const request = JSON.parse(body) as { client_public_key: string };
    const clientPublicKey = decode(request.client_public_key);
    const sharedSecret = nacl.scalarMult(this.keyPair.secretKey, clientPublicKey);
    const key = await handshakeKey(sharedSecret);
    const sessionId = `00000000-0000-4000-8000-${String(this.#nextSession).padStart(12, "0")}`;
    this.#nextSession += 1;
    const expiresAt = Math.floor(Date.now() / 1000) + this.sessionLifetimeSeconds;
    const master = new Uint8Array(32).fill(this.#nextSession & 0xff);
    const payload = new Uint8Array(57);
    payload[0] = 2;
    payload.set(uuidToBytes(sessionId), 1);
    payload.set(master, 17);
    payload.set(writeU64(expiresAt), 49);
    const encrypted = encryptTransportV2Record(key, payload, HANDSHAKE_AAD);
    const directional = await deriveTransportV2DirectionalKeys(master);
    this.sessions.set(sessionId, {
      ...directional,
      expiresAt,
      authority: "anonymous"
    });
    clientPublicKey.fill(0);
    sharedSecret.fill(0);
    key.fill(0);
    master.fill(0);
    payload.fill(0);
    return Response.json({
      session_id: sessionId,
      encrypted_session_key: encode(encrypted)
    });
  }

  async #request(sessionId: string, encrypted: Uint8Array): Promise<Response> {
    const session = this.sessions.get(sessionId);
    if (!session) throw new Error("unknown test session");
    const plaintext = decryptTransportV2Record(
      session.requestKey,
      encrypted,
      requestRecordAad(sessionId),
      TRANSPORT_V2_LIMITS.requestEnvelopeBytes
    );
    const request = JSON.parse(new TextDecoder().decode(plaintext)) as WireRequest;
    plaintext.fill(0);
    encrypted.fill(0);
    this.requests.push(request);

    const path = request.request.path;
    let status = 200;
    let body: unknown = { ok: true, path };
    if (path === "/login") {
      expect(session.authority).toBe("anonymous");
      expect(request.cache_namespace_root_base64).not.toBeNull();
      session.authority = "user";
      body = {
        id: USER_ID,
        access_token: token("access_descriptor"),
        refresh_token: token("resumption")
      };
    } else if (path === "/refresh") {
      expect(session.authority).toBe("anonymous");
      expect(request.credential?.kind).toBe("resumption");
      expect(request.cache_namespace_root_base64).not.toBeNull();
      session.authority = "user";
      body = {
        access_token: token("access_descriptor"),
        refresh_token: token("resumption")
      };
    } else if (path === "/v1/models" && request.credential?.kind === "api_key") {
      expect(request.cache_namespace_root_base64).not.toBeNull();
      session.authority = "api_key";
      body = { object: "list", data: [] };
    } else if (path === "/v1/models") {
      body = { object: "list", data: [] };
    } else if (path.startsWith("/auth/") && !path.endsWith("/callback")) {
      const provider = path.split("/")[2];
      body = { auth_url: `https://oauth.example/${provider}`, state: `${provider}-state` };
    } else if (path.endsWith("/callback")) {
      const provider = path.split("/")[2];
      const callback = JSON.parse(
        new TextDecoder().decode(decode(request.request.body_base64 ?? ""))
      ) as { state: string };
      expect(callback.state).toBe(`${provider}-state`);
      expect(session.authority).toBe("anonymous");
      session.authority = "user";
      body = {
        id: USER_ID,
        access_token: token("access_descriptor"),
        refresh_token: token("resumption")
      };
    } else if (path === "/protected/user") {
      expect(session.authority).toBe("user");
      body = { user: { id: USER_ID } };
    } else if (path === "/protected/change_password") {
      expect(session.authority).toBe("user");
      body = {
        message: "Password changed",
        access_token: token("access_descriptor"),
        refresh_token: token("resumption")
      };
    } else if (path === "/logout") {
      expect(session.authority).toBe("user");
      body = null;
    } else if (path === "/v1/responses") {
      expect(session.authority === "user" || session.authority === "api_key").toBe(true);
      if (request.response_mode === "stream") {
        if (this.preStartSessionExhaustedOncePath === path) {
          this.preStartSessionExhaustedOncePath = null;
          return this.#unaryResponse(sessionId, session, request.request_id, 503, {
            error: {
              code: "session_exhausted",
              message: "Session response capacity is exhausted"
            }
          });
        }
        if (this.streamSessionExhaustedOncePath === path) {
          this.streamSessionExhaustedOncePath = null;
          return this.#streamErrorResponse(sessionId, session, request.request_id, 503, {
            error: {
              code: "session_exhausted",
              message: "Session response capacity is exhausted"
            }
          });
        }
        return this.#streamResponse(sessionId, session, request.request_id);
      }
    } else {
      status = 404;
      body = { message: "unsupported" };
    }

    if (this.unauthorizedOncePath === path) {
      this.unauthorizedOncePath = null;
      status = 401;
      body = { message: "authority is no longer valid" };
    }
    if (this.sessionExhaustedOncePath === path) {
      this.sessionExhaustedOncePath = null;
      status = 503;
      body = {
        error: {
          code: "session_exhausted",
          message: "Session request capacity is exhausted"
        }
      };
    }
    if (this.genericServiceUnavailableOncePath === path) {
      this.genericServiceUnavailableOncePath = null;
      status = 503;
      body = { error: { code: "provider_unavailable", message: "Try again later" } };
    }

    const response = this.#unaryResponse(sessionId, session, request.request_id, status, body);
    if (this.pauseAfterPath === path) {
      this.pauseReached?.();
      await this.pauseRelease;
    }
    if (this.failAfterPath === path) {
      this.failAfterPath = null;
      if (path === "/logout") this.sessions.delete(sessionId);
      throw new Error(`ambiguous ${path} failure`);
    }
    if (path === "/logout") this.sessions.delete(sessionId);
    if (status === 401 && session.authority !== "anonymous") this.sessions.delete(sessionId);
    return response;
  }

  #unaryResponse(
    sessionId: string,
    session: ServerSession,
    requestId: string,
    status: number,
    body: unknown
  ): Response {
    const plaintext = encodeUtf8(
      JSON.stringify({
        version: 2,
        request_id: requestId,
        status,
        headers: [{ name: "content-type", value_base64: encode(encodeUtf8("application/json")) }],
        body_base64: body === undefined ? null : encode(encodeUtf8(JSON.stringify(body)))
      })
    );
    const record = encryptTransportV2Record(
      session.responseKey,
      plaintext,
      unaryResponseRecordAad(sessionId, requestId)
    );
    plaintext.fill(0);
    return new Response(record, {
      status: 200,
      headers: { "content-type": "application/octet-stream" }
    });
  }

  #streamResponse(sessionId: string, session: ServerSession, requestId: string): Response {
    const records = [
      {
        version: 2,
        request_id: requestId,
        sequence: 0,
        kind: "start",
        status: 200,
        headers: [{ name: "content-type", value_base64: encode(encodeUtf8("text/event-stream")) }]
      },
      {
        version: 2,
        request_id: requestId,
        sequence: 1,
        kind: "chunk",
        body_base64: encode(encodeUtf8("event: response.completed\ndata: {}\n\n"))
      },
      { version: 2, request_id: requestId, sequence: 2, kind: "end" }
    ];
    const frames = records.map((value, sequence) => {
      const plaintext = encodeUtf8(JSON.stringify(value));
      const encrypted = encryptTransportV2Record(
        session.responseKey,
        plaintext,
        streamResponseRecordAad(sessionId, requestId, sequence)
      );
      plaintext.fill(0);
      return `data: ${encodeCanonicalBase64(encrypted)}\n\n`;
    });
    const carrier = frames.join("");
    if (this.appendStreamRecordAfterEnd) {
      const plaintext = encodeUtf8(
        JSON.stringify({
          version: 2,
          request_id: requestId,
          sequence: 3,
          kind: "chunk",
          body_base64: encode(encodeUtf8("trailing"))
        })
      );
      const encrypted = encryptTransportV2Record(
        session.responseKey,
        plaintext,
        streamResponseRecordAad(sessionId, requestId, 3)
      );
      plaintext.fill(0);
      const trailing = `data: ${encodeCanonicalBase64(encrypted)}\n\n`;
      const trailingGate = this.streamTrailingGate;
      let emittedTrailing = false;
      return new Response(
        new ReadableStream<Uint8Array>({
          start(controller) {
            controller.enqueue(encodeUtf8(carrier));
          },
          async pull(controller) {
            if (!emittedTrailing) {
              emittedTrailing = true;
              await trailingGate;
              controller.enqueue(encodeUtf8(trailing));
              return;
            }
            controller.close();
          }
        }),
        { headers: { "content-type": "text/event-stream" } }
      );
    }
    return new Response(carrier, { headers: { "content-type": "text/event-stream" } });
  }

  #streamErrorResponse(
    sessionId: string,
    session: ServerSession,
    requestId: string,
    status: number,
    body: unknown
  ): Response {
    const records = [
      {
        version: 2,
        request_id: requestId,
        sequence: 0,
        kind: "start",
        status: 200,
        headers: [{ name: "content-type", value_base64: encode(encodeUtf8("text/event-stream")) }]
      },
      {
        version: 2,
        request_id: requestId,
        sequence: 1,
        kind: "error",
        status,
        body_base64: encode(encodeUtf8(JSON.stringify(body)))
      }
    ];
    const carrier = records
      .map((value, sequence) => {
        const plaintext = encodeUtf8(JSON.stringify(value));
        const encrypted = encryptTransportV2Record(
          session.responseKey,
          plaintext,
          streamResponseRecordAad(sessionId, requestId, sequence)
        );
        plaintext.fill(0);
        return `data: ${encodeCanonicalBase64(encrypted)}\n\n`;
      })
      .join("");
    return new Response(carrier, { headers: { "content-type": "text/event-stream" } });
  }
}

function operation(
  path: string,
  authority: TransportV2FetchInput["authority"],
  body: unknown = undefined,
  responseMode: "unary" | "stream" = "unary"
): TransportV2FetchInput {
  return {
    apiUrl: API_URL,
    pcrConfig: { environment: "development" },
    url: `${API_URL}${path}`,
    method: "POST",
    headers: body === undefined ? undefined : { "content-type": "application/json" },
    body: body === undefined ? null : encodeUtf8(JSON.stringify(body)),
    responseMode,
    authority
  };
}

function userAuthority(): Extract<TransportV2FetchInput["authority"], { kind: "user" }> {
  const credentials = readTransportV2Credentials(API_URL, "user");
  if (!credentials) throw new Error("test user credentials are not installed");
  return {
    kind: "user",
    principalId: credentials.principalId,
    generation: credentials.generation
  };
}

function installUserCredentials(principal: string) {
  return installTransportV2Credentials(
    API_URL,
    "user",
    token("access_descriptor", principal),
    token("resumption", principal)
  );
}

function pausePath(server: V2TestServer, path: string) {
  let markReached!: () => void;
  let release!: () => void;
  const reached = new Promise<void>((resolve) => {
    markReached = resolve;
  });
  server.pauseAfterPath = path;
  server.pauseReached = markReached;
  server.pauseRelease = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { reached, release };
}

afterEach(() => {
  clearTransportV2Credentials(API_URL);
  clearTransportV2CacheRoot(API_URL);
  localStorage.clear();
  sessionStorage.clear();
});

describe("Transport V2 authority/session manager", () => {
  test("singleflights one anonymous handshake and never uses a v1 transport URL", async () => {
    const server = new V2TestServer();
    const client = new TransportV2Client(server.dependencies());
    const [first, second] = await Promise.all([
      client.fetch(operation("/v1/models", { kind: "anonymous", purpose: "public" })),
      client.fetch(operation("/v1/models", { kind: "anonymous", purpose: "public" }))
    ]);
    expect(first.ok).toBe(true);
    expect(second.ok).toBe(true);
    expect(server.attestationCount).toBe(1);
    expect(server.keyExchangeCount).toBe(1);
    expect(server.outerUrls.every((url) => url.includes("/v2/"))).toBe(true);
  });

  test("binds login and serves steady user requests on that exact session", async () => {
    const server = new V2TestServer();
    const client = new TransportV2Client(server.dependencies());
    await client.fetch(
      operation("/login", { kind: "anonymous", purpose: "user" }, { email: "a", password: "b" })
    );
    const response = await client.fetch({
      ...operation("/protected/user", userAuthority()),
      method: "GET"
    });
    expect(response.ok).toBe(true);
    expect(server.keyExchangeCount).toBe(1);
    expect(server.requests[0].credential).toBeNull();
    expect(server.requests[0].cache_namespace_root_base64).not.toBeNull();
    expect(server.requests[1].credential).toBeNull();
    expect(server.requests[1].cache_namespace_root_base64).toBeNull();
  });

  test("never sends a body captured under an older user generation", async () => {
    const server = new V2TestServer();
    const client = new TransportV2Client(server.dependencies());
    await client.fetch(
      operation("/login", { kind: "anonymous", purpose: "user" }, { email: "a", password: "b" })
    );
    const stale = userAuthority();
    installUserCredentials("11112233-4455-6677-8899-aabbccddeeff");
    const sent = server.requestCount;

    await expect(
      client.fetch({ ...operation("/protected/user", stale), method: "GET" })
    ).rejects.toThrow("authentication state changed");
    expect(server.requestCount).toBe(sent);
  });

  test("disposes an ambiguously bound login session before another attempt", async () => {
    const server = new V2TestServer();
    server.failAfterPath = "/login";
    const client = new TransportV2Client(server.dependencies());
    await expect(
      client.fetch(
        operation("/login", { kind: "anonymous", purpose: "user" }, { email: "a", password: "b" })
      )
    ).rejects.toThrow("ambiguous /login failure");
    await expect(
      client.fetch(
        operation("/login", { kind: "anonymous", purpose: "user" }, { email: "a", password: "b" })
      )
    ).resolves.toBeInstanceOf(Response);
    expect(server.keyExchangeCount).toBe(2);
  });

  test("a delayed login cannot overwrite credentials installed while it was in flight", async () => {
    const server = new V2TestServer();
    const paused = pausePath(server, "/login");
    const client = new TransportV2Client(server.dependencies());
    const login = client.fetch(
      operation("/login", { kind: "anonymous", purpose: "user" }, { email: "a", password: "b" })
    );
    await paused.reached;
    const newer = installUserCredentials("11112233-4455-6677-8899-aabbccddeeff");
    paused.release();

    await expect(login).rejects.toThrow("authentication state changed");
    expect(readTransportV2Credentials(API_URL, "user")).toEqual(newer);
  });

  test("a delayed successful refresh cannot overwrite a newer principal", async () => {
    const server = new V2TestServer();
    const client = new TransportV2Client(server.dependencies());
    await client.fetch(
      operation("/login", { kind: "anonymous", purpose: "user" }, { email: "a", password: "b" })
    );
    client.retireAuthenticationState(API_URL, "user");
    const paused = pausePath(server, "/refresh");
    const pending = client.fetch({
      ...operation("/protected/user", userAuthority()),
      method: "GET"
    });
    await paused.reached;
    const newer = installUserCredentials("11112233-4455-6677-8899-aabbccddeeff");
    paused.release();

    await expect(pending).rejects.toThrow("authentication state changed");
    expect(readTransportV2Credentials(API_URL, "user")).toEqual(newer);
  });

  test("a delayed rejected refresh cannot clear a newer principal", async () => {
    const server = new V2TestServer();
    const client = new TransportV2Client(server.dependencies());
    await client.fetch(
      operation("/login", { kind: "anonymous", purpose: "user" }, { email: "a", password: "b" })
    );
    client.retireAuthenticationState(API_URL, "user");
    server.unauthorizedOncePath = "/refresh";
    const paused = pausePath(server, "/refresh");
    const pending = client.fetch({
      ...operation("/protected/user", userAuthority()),
      method: "GET"
    });
    await paused.reached;
    const newer = installUserCredentials("11112233-4455-6677-8899-aabbccddeeff");
    paused.release();

    await expect(pending).rejects.toThrow("authentication state changed");
    expect(readTransportV2Credentials(API_URL, "user")).toEqual(newer);
  });

  test("a current rejected refresh clears credentials and emits React invalidation", async () => {
    const server = new V2TestServer();
    const client = new TransportV2Client(server.dependencies());
    await client.fetch(
      operation("/login", { kind: "anonymous", purpose: "user" }, { email: "a", password: "b" })
    );
    client.retireAuthenticationState(API_URL, "user");
    server.unauthorizedOncePath = "/refresh";
    let invalidations = 0;
    const unsubscribe = subscribeTransportV2AuthInvalidation(API_URL, "user", () => {
      invalidations += 1;
    });
    try {
      await expect(
        client.fetch({ ...operation("/protected/user", userAuthority()), method: "GET" })
      ).rejects.toThrow("resumption was rejected");
      expect(readTransportV2Credentials(API_URL, "user")).toBeNull();
      expect(invalidations).toBe(1);
    } finally {
      unsubscribe();
    }
  });

  test("an ordinary request adopts the exact generation produced by its own resumption", async () => {
    const server = new V2TestServer();
    const client = new TransportV2Client(server.dependencies());
    await client.fetch(
      operation("/login", { kind: "anonymous", purpose: "user" }, { email: "a", password: "b" })
    );
    const before = userAuthority();
    client.retireAuthenticationState(API_URL, "user");

    const response = await client.fetch({
      ...operation("/protected/user", before),
      method: "GET"
    });

    expect(response.ok).toBe(true);
    expect(userAuthority().generation).toBe(before.generation + 1);
    expect(server.requests.slice(-2).map((request) => request.request.path)).toEqual([
      "/refresh",
      "/protected/user"
    ]);
  });

  test("terminal operations use the exact generation produced by their own resumption", async () => {
    const server = new V2TestServer();
    const client = new TransportV2Client(server.dependencies());
    await client.fetch(
      operation("/login", { kind: "anonymous", purpose: "user" }, { email: "a", password: "b" })
    );
    const before = userAuthority();
    client.retireAuthenticationState(API_URL, "user");

    const changed = await client.fetch(
      operation("/protected/change_password", before, {
        current_password: "old",
        new_password: "new"
      })
    );
    expect(changed.ok).toBe(true);
    const afterChange = userAuthority();
    expect(afterChange.generation).toBe(before.generation + 2);

    const loggedOut = await client.fetch(operation("/logout", afterChange, {}));
    expect(loggedOut.ok).toBe(true);
    expect(readTransportV2Credentials(API_URL, "user")).toBeNull();
  });

  test("a delayed logout cannot clear or retire a newer principal", async () => {
    const server = new V2TestServer();
    const client = new TransportV2Client(server.dependencies());
    await client.fetch(
      operation("/login", { kind: "anonymous", purpose: "user" }, { email: "a", password: "b" })
    );
    const paused = pausePath(server, "/logout");
    const pending = client.fetch(operation("/logout", userAuthority(), {}));
    await paused.reached;
    const newer = installUserCredentials("11112233-4455-6677-8899-aabbccddeeff");
    paused.release();

    await expect(pending).rejects.toThrow("authentication state changed");
    expect(readTransportV2Credentials(API_URL, "user")).toEqual(newer);
  });

  test("does not replay after an ambiguous ordinary request send", async () => {
    const server = new V2TestServer();
    const client = new TransportV2Client(server.dependencies());
    server.failAfterPath = "/v1/models";
    await expect(
      client.fetch(operation("/v1/models", { kind: "anonymous", purpose: "public" }))
    ).rejects.toThrow("ambiguous /v1/models failure");
    expect(server.requestCount).toBe(1);
    await expect(
      client.fetch(operation("/v1/models", { kind: "anonymous", purpose: "public" }))
    ).resolves.toBeInstanceOf(Response);
    expect(server.keyExchangeCount).toBe(2);
  });

  test("retires an exhausted bound session without retrying and replaces it next request", async () => {
    const server = new V2TestServer();
    const client = new TransportV2Client(server.dependencies(2));
    await client.fetch(
      operation("/login", { kind: "anonymous", purpose: "user" }, { email: "a", password: "b" })
    );
    await client.fetch({ ...operation("/protected/user", userAuthority()), method: "GET" });
    const sentBeforeExhaustion = server.requestCount;

    await expect(
      client.fetch({ ...operation("/protected/user", userAuthority()), method: "GET" })
    ).rejects.toThrow("response record budget is exhausted");
    expect(server.requestCount).toBe(sentBeforeExhaustion);

    await expect(
      client.fetch({ ...operation("/protected/user", userAuthority()), method: "GET" })
    ).resolves.toBeInstanceOf(Response);
    expect(server.keyExchangeCount).toBe(2);
    expect(server.requestCount).toBe(sentBeforeExhaustion + 2);
  });

  test("retires only an authenticated unary session_exhausted response for the next request", async () => {
    const server = new V2TestServer();
    const client = new TransportV2Client(server.dependencies());
    await client.fetch(
      operation("/login", { kind: "anonymous", purpose: "user" }, { email: "a", password: "b" })
    );
    server.sessionExhaustedOncePath = "/protected/user";
    const sentBefore = server.requestCount;

    const exhausted = await client.fetch({
      ...operation("/protected/user", userAuthority()),
      method: "GET"
    });
    expect(exhausted.status).toBe(503);
    expect(await exhausted.json()).toEqual({
      error: {
        code: "session_exhausted",
        message: "Session request capacity is exhausted"
      }
    });
    expect(server.requestCount).toBe(sentBefore + 1);

    await client.fetch({ ...operation("/protected/user", userAuthority()), method: "GET" });
    expect(server.keyExchangeCount).toBe(2);
    expect(server.requestCount).toBe(sentBefore + 3);
  });

  test("does not retire a generic authenticated 503 application response", async () => {
    const server = new V2TestServer();
    const client = new TransportV2Client(server.dependencies());
    await client.fetch(
      operation("/login", { kind: "anonymous", purpose: "user" }, { email: "a", password: "b" })
    );
    server.genericServiceUnavailableOncePath = "/protected/user";

    const unavailable = await client.fetch({
      ...operation("/protected/user", userAuthority()),
      method: "GET"
    });
    expect(unavailable.status).toBe(503);
    await client.fetch({ ...operation("/protected/user", userAuthority()), method: "GET" });
    expect(server.keyExchangeCount).toBe(1);
  });

  test("retires an authenticated pre-start stream session_exhausted response", async () => {
    const server = new V2TestServer();
    const client = new TransportV2Client(server.dependencies());
    await client.fetch(
      operation("/login", { kind: "anonymous", purpose: "user" }, { email: "a", password: "b" })
    );
    server.preStartSessionExhaustedOncePath = "/v1/responses";

    const exhausted = await client.fetch(
      operation("/v1/responses", userAuthority(), { model: "test" }, "stream")
    );
    expect(exhausted.status).toBe(503);
    expect((await exhausted.json()).error.code).toBe("session_exhausted");
    await client.fetch({ ...operation("/protected/user", userAuthority()), method: "GET" });
    expect(server.keyExchangeCount).toBe(2);
  });

  test("retires an authenticated late stream session_exhausted terminal", async () => {
    const server = new V2TestServer();
    const client = new TransportV2Client(server.dependencies());
    await client.fetch(
      operation("/login", { kind: "anonymous", purpose: "user" }, { email: "a", password: "b" })
    );
    server.streamSessionExhaustedOncePath = "/v1/responses";

    const exhausted = await client.fetch(
      operation("/v1/responses", userAuthority(), { model: "test" }, "stream")
    );
    await expect(exhausted.text()).rejects.toThrow('"code":"session_exhausted"');
    await client.fetch({ ...operation("/protected/user", userAuthority()), method: "GET" });
    expect(server.keyExchangeCount).toBe(2);
  });

  test("reconstructs an authenticated SSE stream through its exact terminal record", async () => {
    const server = new V2TestServer();
    const client = new TransportV2Client(server.dependencies());
    await client.fetch(
      operation("/login", { kind: "anonymous", purpose: "user" }, { email: "a", password: "b" })
    );
    const response = await client.fetch(
      operation("/v1/responses", userAuthority(), { model: "test" }, "stream")
    );
    expect(response.status).toBe(200);
    expect(response.headers.get("content-type")).toBe("text/event-stream");
    expect(await response.text()).toBe("event: response.completed\ndata: {}\n\n");
  });

  test("rejects a carrier chunk arriving after the authenticated stream terminal", async () => {
    const server = new V2TestServer();
    server.appendStreamRecordAfterEnd = true;
    const client = new TransportV2Client(server.dependencies());
    await client.fetch(
      operation("/login", { kind: "anonymous", purpose: "user" }, { email: "a", password: "b" })
    );
    const response = await client.fetch(
      operation("/v1/responses", userAuthority(), { model: "test" }, "stream")
    );
    await expect(response.text()).rejects.toThrow(
      "Transport v2 stream contains data after its terminal record"
    );
  });

  test("a late failure from an old stream cannot retire its replacement session", async () => {
    const server = new V2TestServer();
    server.appendStreamRecordAfterEnd = true;
    let releaseTrailing!: () => void;
    server.streamTrailingGate = new Promise<void>((resolve) => {
      releaseTrailing = resolve;
    });
    const client = new TransportV2Client(server.dependencies());
    await client.fetch(
      operation("/login", { kind: "anonymous", purpose: "user" }, { email: "a", password: "b" })
    );
    const oldResponse = await client.fetch(
      operation("/v1/responses", userAuthority(), { model: "test" }, "stream")
    );

    client.retireAuthenticationState(API_URL, "user");
    await client.fetch({
      ...operation("/protected/user", userAuthority()),
      method: "GET"
    });
    expect(server.keyExchangeCount).toBe(2);

    releaseTrailing();
    await expect(oldResponse.text()).rejects.toThrow(
      "Transport v2 stream contains data after its terminal record"
    );
    await client.fetch({
      ...operation("/protected/user", userAuthority()),
      method: "GET"
    });
    expect(server.keyExchangeCount).toBe(2);
  });

  test("retires a bound user session after an ambiguous logout", async () => {
    const server = new V2TestServer();
    const client = new TransportV2Client(server.dependencies());
    await client.fetch(
      operation("/login", { kind: "anonymous", purpose: "user" }, { email: "a", password: "b" })
    );
    server.failAfterPath = "/logout";
    await expect(client.fetch(operation("/logout", userAuthority(), {}))).rejects.toThrow(
      "ambiguous /logout failure"
    );
    await expect(
      client.fetch({ ...operation("/protected/user", userAuthority()), method: "GET" })
    ).resolves.toBeInstanceOf(Response);
    expect(server.keyExchangeCount).toBe(2);
  });

  test("local logout cleanup follows only the exact refresh produced by that logout", async () => {
    const server = new V2TestServer();
    const client = new TransportV2Client(server.dependencies());
    await client.fetch(
      operation("/login", { kind: "anonymous", purpose: "user" }, { email: "a", password: "b" })
    );
    const expected = snapshotTransportV2Auth(API_URL, "user");
    client.retireAuthenticationState(API_URL, "user");
    server.failAfterPath = "/logout";

    await expect(client.fetch(operation("/logout", userAuthority(), {}))).rejects.toThrow(
      "ambiguous /logout failure"
    );
    expect(readTransportV2Credentials(API_URL, "user")?.generation).toBe(expected.generation + 1);
    expect(client.clear(API_URL, "user", false, expected)).toBe(true);
    expect(readTransportV2Credentials(API_URL, "user")).toBeNull();
  });

  test("refresh-successor logout cleanup cannot clear a newer login", async () => {
    const server = new V2TestServer();
    const client = new TransportV2Client(server.dependencies());
    await client.fetch(
      operation("/login", { kind: "anonymous", purpose: "user" }, { email: "a", password: "b" })
    );
    const expected = snapshotTransportV2Auth(API_URL, "user");
    client.retireAuthenticationState(API_URL, "user");
    server.failAfterPath = "/logout";
    await expect(client.fetch(operation("/logout", userAuthority(), {}))).rejects.toThrow(
      "ambiguous /logout failure"
    );

    const newer = installUserCredentials("11112233-4455-6677-8899-aabbccddeeff");
    expect(client.clear(API_URL, "user", false, expected)).toBe(false);
    expect(readTransportV2Credentials(API_URL, "user")).toEqual(newer);
  });

  for (const provider of ["github", "google", "apple"] as const) {
    test(`restores and consumes the exact ${provider} OAuth session after reload`, async () => {
      const server = new V2TestServer();
      const initiator = new TransportV2Client(server.dependencies());
      const initiated = await initiator.fetch(
        operation(`/auth/${provider}`, { kind: "anonymous", purpose: "user" }, { client_id: "c" })
      );
      const state = ((await initiated.json()) as { state: string }).state;
      expect(state).toBe(`${provider}-state`);
      expect(sessionStorage.length).toBe(1);

      const callback = new TransportV2Client(server.dependencies());
      const completed = await callback.fetch(
        operation(
          `/auth/${provider}/callback`,
          { kind: "anonymous", purpose: "user" },
          {
            code: "code",
            state
          }
        )
      );
      expect(completed.ok).toBe(true);
      expect(sessionStorage.length).toBe(0);
      expect(server.keyExchangeCount).toBe(1);
      await expect(
        new TransportV2Client(server.dependencies()).fetch(
          operation(
            `/auth/${provider}/callback`,
            { kind: "anonymous", purpose: "user" },
            {
              code: "code",
              state
            }
          )
        )
      ).rejects.toThrow("unavailable");
    });
  }

  test("does not consume an OAuth continuation for a wrong state, policy, or provider", async () => {
    const server = new V2TestServer();
    const client = new TransportV2Client(server.dependencies());
    await client.fetch(
      operation("/auth/github", { kind: "anonymous", purpose: "user" }, { client_id: "c" })
    );
    const callback = new TransportV2Client(server.dependencies());
    await expect(
      callback.fetch(
        operation(
          "/auth/github/callback",
          { kind: "anonymous", purpose: "user" },
          {
            code: "code",
            state: "wrong"
          }
        )
      )
    ).rejects.toThrow("does not match");
    await expect(
      callback.fetch({
        ...operation(
          "/auth/github/callback",
          { kind: "anonymous", purpose: "user" },
          {
            code: "code",
            state: "github-state"
          }
        ),
        pcrConfig: { pcr0Values: ["a".repeat(96)], remoteAttestation: false }
      })
    ).rejects.toThrow("unavailable");
    await expect(
      callback.fetch(
        operation(
          "/auth/google/callback",
          { kind: "anonymous", purpose: "user" },
          {
            code: "code",
            state: "github-state"
          }
        )
      )
    ).rejects.toThrow("unavailable");
    const otherApiUrl = "http://127.0.0.1:3010/other";
    await expect(
      callback.fetch({
        ...operation(
          "/auth/github/callback",
          { kind: "anonymous", purpose: "user" },
          {
            code: "code",
            state: "github-state"
          }
        ),
        apiUrl: otherApiUrl,
        url: `${otherApiUrl}/auth/github/callback`
      })
    ).rejects.toThrow("unavailable");
    expect(sessionStorage.length).toBe(1);
  });

  test("removes an expired OAuth continuation", async () => {
    const server = new V2TestServer();
    const client = new TransportV2Client(server.dependencies());
    await client.fetch(
      operation("/auth/github", { kind: "anonymous", purpose: "user" }, { client_id: "c" })
    );
    const key = sessionStorage.key(0)!;
    const continuation = JSON.parse(sessionStorage.getItem(key)!) as {
      session: { expiresAtUnixSeconds: number };
    };
    continuation.session.expiresAtUnixSeconds = 0;
    sessionStorage.setItem(key, JSON.stringify(continuation));
    await expect(
      new TransportV2Client(server.dependencies()).fetch(
        operation(
          "/auth/github/callback",
          { kind: "anonymous", purpose: "user" },
          {
            code: "code",
            state: "github-state"
          }
        )
      )
    ).rejects.toThrow("expired");
    expect(sessionStorage.length).toBe(0);
  });

  test("cleans a failed API-key establishment gate for the next concurrent caller", async () => {
    const server = new V2TestServer();
    server.failKeyExchanges = 1;
    const client = new TransportV2Client(server.dependencies());
    const authority = { kind: "api_key" as const, value: "api-key" };
    const [first, second] = await Promise.allSettled([
      client.fetch(operation("/v1/models", authority)),
      client.fetch(operation("/v1/models", authority))
    ]);
    expect(first.status).toBe("rejected");
    expect(second.status).toBe("fulfilled");
    expect(server.keyExchangeCount).toBe(2);
    await expect(client.fetch(operation("/v1/models", authority))).resolves.toBeInstanceOf(
      Response
    );
    expect(server.keyExchangeCount).toBe(2);
  });

  test("retiring an in-flight API-key bind prevents the old completion from repopulating it", async () => {
    const server = new V2TestServer();
    const paused = pausePath(server, "/v1/models");
    const client = new TransportV2Client(server.dependencies());
    const authority = { kind: "api_key" as const, value: "api-key" };
    const pending = client.fetch(operation("/v1/models", authority));
    await paused.reached;
    await client.retireApiKey(API_URL, { environment: "development" }, "api-key");
    paused.release();

    await expect(pending).rejects.toThrow("authentication state changed");
    await expect(client.fetch(operation("/v1/models", authority))).resolves.toBeInstanceOf(
      Response
    );
    expect(server.keyExchangeCount).toBe(2);
  });

  test("re-establishes an API-key session at its exact expiry", async () => {
    const originalNow = Date.now;
    let now = originalNow();
    Date.now = () => now;
    try {
      const server = new V2TestServer();
      server.sessionLifetimeSeconds = 1;
      const client = new TransportV2Client(server.dependencies());
      const authority = { kind: "api_key" as const, value: "api-key" };
      await client.fetch(operation("/v1/models", authority));
      now += 1000;
      await client.fetch(operation("/v1/models", authority));
      expect(server.keyExchangeCount).toBe(2);
    } finally {
      Date.now = originalNow;
    }
  });

  test("retires bound user and API-key sessions after an authenticated unauthorized result", async () => {
    const server = new V2TestServer();
    const client = new TransportV2Client(server.dependencies());
    await client.fetch(
      operation("/login", { kind: "anonymous", purpose: "user" }, { email: "a", password: "b" })
    );
    server.unauthorizedOncePath = "/protected/user";
    const unauthorizedUser = await client.fetch({
      ...operation("/protected/user", userAuthority()),
      method: "GET"
    });
    expect(unauthorizedUser.status).toBe(401);
    await client.fetch({ ...operation("/protected/user", userAuthority()), method: "GET" });
    expect(server.keyExchangeCount).toBe(2);

    const authority = { kind: "api_key" as const, value: "api-key" };
    await client.fetch(operation("/v1/models", authority));
    server.unauthorizedOncePath = "/v1/models";
    const unauthorizedApiKey = await client.fetch(operation("/v1/models", authority));
    expect(unauthorizedApiKey.status).toBe(401);
    await client.fetch(operation("/v1/models", authority));
    expect(server.keyExchangeCount).toBe(4);
  });
});
