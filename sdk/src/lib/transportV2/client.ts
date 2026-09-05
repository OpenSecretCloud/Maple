import {
  decodeUtf8,
  encodeCanonicalBase64,
  encodeUtf8,
  MIN_ENCRYPTED_RECORD_BYTES,
  parseStrictJson,
  requireExactObject
} from "./encoding";
import type {
  LogicalMethod,
  ResponseMode,
  TransportV2Credential,
  TransportV2Header,
  TransportV2LogicalRequest,
  TransportV2StreamRecord
} from "./envelope";
import { TRANSPORT_V2_LIMITS } from "./envelope";
import { TransportV2Handshake } from "./handshake";
import {
  TransportV2Session,
  TransportV2SessionUnavailableError,
  type PreparedTransportV2Request,
  type SerializedTransportV2SessionState
} from "./session";
import { isLocalDevelopmentApiUrl, verifyAttestationDocument } from "../attestation";
import {
  requireTrustedPcr0,
  serializePcrConfig,
  snapshotPcrConfig,
  validatePcr0Hash
} from "../pcr";
import type { PcrConfig } from "../pcr";
import {
  canonicalizeTransportV2ApiUrl,
  clearLegacyTransportV1Credentials,
  clearTransportV2CacheRoot,
  clearTransportV2Credentials,
  clearTransportV2CredentialsIfCurrent,
  getOrCreateTransportV2CacheRoot,
  installTransportV2Credentials,
  isTransportV2AuthSnapshotCurrent,
  readTransportV2Credentials,
  snapshotTransportV2Auth,
  type StoredTransportV2Credentials,
  type TransportV2AuthSnapshot,
  TransportV2AuthorityChangedError,
  type TransportV2PrincipalKind
} from "./auth";

const MAX_HANDSHAKE_RESPONSE_BYTES = 4 * 1024;
const MAX_ATTESTATION_RESPONSE_BYTES = 2 * 1024 * 1024;
const MAX_OUTER_RESPONSE_BYTES =
  TRANSPORT_V2_LIMITS.responseEnvelopeBytes + MIN_ENCRYPTED_RECORD_BYTES;
const OAUTH_CONTINUATION_PREFIX = "opensecret:transport-v2:oauth:v1:";
const AUTH_RENEWAL_SKEW_SECONDS = 30;

export type TransportV2Authority =
  | { kind: "anonymous"; purpose: "public" | "user" | "platform" }
  | { kind: "user"; principalId: string; generation: number }
  | { kind: "platform"; principalId: string; generation: number }
  | { kind: "api_key"; value: string };

export interface TransportV2FetchInput {
  apiUrl: string;
  pcrConfig?: PcrConfig;
  url: string;
  method: LogicalMethod;
  headers?: HeadersInit;
  body: Uint8Array | null;
  responseMode: ResponseMode;
  authority: TransportV2Authority;
  signal?: AbortSignal | null;
}

export interface TransportV2SessionInfo {
  protocolVersion: 2;
  sessionId: string;
  expiresAtUnixSeconds: number;
  authority: "anonymous" | "user" | "platform" | "api_key";
}

interface OAuthContinuation {
  version: 2;
  api_origin: string;
  pcr_policy: string;
  provider: "github" | "google" | "apple";
  state: string;
  session: SerializedTransportV2SessionState;
}

interface ManagedSession {
  session: TransportV2Session;
  authority: TransportV2SessionInfo["authority"];
  principalId?: string;
  authGeneration?: number;
}

interface SendResult {
  response: Response;
  session: ManagedSession;
}

export interface TransportV2ClientDependencies {
  fetch: typeof globalThis.fetch;
  crypto: Crypto;
  verifyAttestationDocument: typeof verifyAttestationDocument;
  validatePcr0Hash: typeof validatePcr0Hash;
  /** @internal Deterministic capacity hook for session-manager tests. */
  sessionResponseRecordLimit?: number;
}

const defaultDependencies: TransportV2ClientDependencies = {
  fetch: (...args) => globalThis.fetch(...args),
  crypto: globalThis.crypto,
  verifyAttestationDocument,
  validatePcr0Hash
};

function sessionStorageOrUndefined(): Storage | undefined {
  try {
    return globalThis.sessionStorage;
  } catch {
    return undefined;
  }
}

function safeSessionStorageGet(key: string): string | null {
  try {
    return sessionStorageOrUndefined()?.getItem(key) ?? null;
  } catch {
    return null;
  }
}

function safeSessionStorageSet(key: string, value: string): void {
  try {
    const storage = sessionStorageOrUndefined();
    if (!storage) {
      throw new Error("same-tab session storage is unavailable");
    }
    storage.setItem(key, value);
  } catch {
    throw new Error("OAuth requires same-tab session storage for its attested continuation.");
  }
}

function safeSessionStorageRemove(key: string): void {
  try {
    sessionStorageOrUndefined()?.removeItem(key);
  } catch {
    // The in-memory session will still be disposed; unavailable browser
    // storage cannot be used to resume a continuation.
  }
}

function oauthContinuationKey(
  apiOrigin: string,
  pcrPolicy: string,
  provider: OAuthContinuation["provider"]
): string {
  const scope = encodeCanonicalBase64(encodeUtf8(`${apiOrigin}\n${pcrPolicy}\n${provider}`))
    .replace(/=+$/u, "")
    .replace(/\+/gu, "-")
    .replace(/\//gu, "_");
  return `${OAUTH_CONTINUATION_PREFIX}${scope}`;
}

function parseOAuthContinuation(raw: string): OAuthContinuation {
  const value = requireExactObject(
    parseStrictJson(raw),
    ["version", "api_origin", "pcr_policy", "provider", "state", "session"],
    "Transport v2 OAuth continuation"
  );
  if (
    value.version !== 2 ||
    typeof value.api_origin !== "string" ||
    typeof value.pcr_policy !== "string" ||
    !(["github", "google", "apple"] as const).includes(
      value.provider as OAuthContinuation["provider"]
    ) ||
    typeof value.state !== "string" ||
    value.state.length === 0 ||
    typeof value.session !== "object" ||
    value.session === null
  ) {
    throw new Error("Transport v2 OAuth continuation is invalid.");
  }
  const session = requireExactObject(
    value.session,
    [
      "version",
      "sessionId",
      "expiresAtUnixSeconds",
      "requestKeyBase64",
      "responseKeyBase64",
      "requestRecords",
      "responseRecords"
    ],
    "Transport v2 OAuth session"
  );
  return { ...value, session } as unknown as OAuthContinuation;
}

function exactResponseHeaders(headers: readonly TransportV2Header[]): Headers {
  const result = new Headers();
  for (const header of headers) {
    let value = "";
    for (const byte of header.value) value += String.fromCharCode(byte);
    result.append(header.name, value);
  }
  return result;
}

function logicalResponse(response: {
  status: number;
  headers: readonly TransportV2Header[];
  body: Uint8Array | null;
}): Response {
  return new Response(response.body, {
    status: response.status,
    headers: exactResponseHeaders(response.headers)
  });
}

async function readBoundedBytes(response: Response, limit: number, description: string) {
  const contentLength = response.headers.get("content-length");
  if (contentLength && /^\d+$/u.test(contentLength) && Number(contentLength) > limit) {
    await response.body?.cancel().catch(() => {});
    throw new Error(`${description} exceeds its size limit.`);
  }
  if (!response.body) return new Uint8Array(0);
  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  try {
    while (true) {
      const next = await reader.read();
      if (next.done) break;
      total += next.value.length;
      if (!Number.isSafeInteger(total) || total > limit) {
        next.value.fill(0);
        throw new Error(`${description} exceeds its size limit.`);
      }
      chunks.push(next.value);
    }
    const bytes = new Uint8Array(total);
    let offset = 0;
    for (const chunk of chunks) {
      bytes.set(chunk, offset);
      offset += chunk.length;
      chunk.fill(0);
    }
    chunks.length = 0;
    return bytes;
  } catch (error) {
    await reader.cancel().catch(() => {});
    for (const chunk of chunks) chunk.fill(0);
    throw error;
  }
}

async function readBoundedText(response: Response, limit: number, description: string) {
  const bytes = await readBoundedBytes(response, limit, description);
  try {
    return decodeUtf8(bytes);
  } finally {
    bytes.fill(0);
  }
}

function hasExactContentType(response: Response, expected: string): boolean {
  return response.headers.get("content-type")?.trim().toLowerCase() === expected;
}

function requestParts(apiOrigin: string, requestUrl: string) {
  let request: URL;
  const base = new URL(apiOrigin);
  try {
    request = new URL(requestUrl);
  } catch {
    throw new Error("Transport v2 request URL is invalid.");
  }
  if (request.origin !== base.origin || request.username || request.password || request.hash) {
    throw new Error("Transport v2 request escaped its attested API origin.");
  }

  const basePath = base.pathname === "/" ? "" : base.pathname.replace(/\/+$/u, "");
  if (basePath && request.pathname !== basePath && !request.pathname.startsWith(`${basePath}/`)) {
    throw new Error("Transport v2 request escaped its configured API base path.");
  }
  const path = basePath ? request.pathname.slice(basePath.length) || "/" : request.pathname;
  return { path, query: request.search ? request.search.slice(1) : null };
}

function logicalHeaders(headers?: HeadersInit): TransportV2Header[] {
  if (!headers) return [];
  const logical: TransportV2Header[] = [];
  new Headers(headers).forEach((value, name) => {
    logical.push({ name: name.toLowerCase(), value: encodeUtf8(value) });
  });
  return logical;
}

function strictJsonObject(bytes: Uint8Array | null): Record<string, unknown> {
  if (!bytes) throw new Error("Transport v2 operation requires a JSON body.");
  const parsed = parseStrictJson(decodeUtf8(bytes));
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    throw new Error("Transport v2 operation requires a JSON object.");
  }
  return parsed as Record<string, unknown>;
}

function oauthProvider(path: string): OAuthContinuation["provider"] | null {
  if (path.startsWith("/auth/github")) return "github";
  if (path.startsWith("/auth/google")) return "google";
  if (path.startsWith("/auth/apple")) return "apple";
  return null;
}

function isOAuthInitiation(path: string): boolean {
  return path === "/auth/github" || path === "/auth/google" || path === "/auth/apple";
}

function isOAuthCallback(path: string): boolean {
  return (
    path === "/auth/github/callback" ||
    path === "/auth/google/callback" ||
    path === "/auth/apple/callback"
  );
}

function isUserBinding(path: string): boolean {
  return (
    path === "/login" ||
    path === "/register" ||
    isOAuthCallback(path) ||
    path === "/auth/apple/native"
  );
}

function isPlatformBinding(path: string): boolean {
  return path === "/platform/login" || path === "/platform/register";
}

function isTerminalUserOperation(path: string): boolean {
  return (
    path === "/logout" ||
    path === "/protected/change_password" ||
    path === "/protected/delete-account/confirm"
  );
}

function isTerminalPlatformOperation(path: string): boolean {
  return path === "/platform/logout" || path === "/platform/change-password";
}

async function apiKeyHash(apiKey: string, crypto: Crypto): Promise<string> {
  const bytes = encodeUtf8(apiKey);
  try {
    const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
    try {
      return Array.from(digest, (byte) => byte.toString(16).padStart(2, "0")).join("");
    } finally {
      digest.fill(0);
    }
  } finally {
    bytes.fill(0);
  }
}

function authSnapshotForAuthority(
  apiOrigin: string,
  authority: Exclude<TransportV2Authority, { kind: "api_key" }>
): TransportV2AuthSnapshot | null {
  if (authority.kind === "user" || authority.kind === "platform") {
    return {
      apiOrigin,
      kind: authority.kind,
      principalId: authority.principalId,
      generation: authority.generation
    };
  }
  if (authority.purpose === "user" || authority.purpose === "platform") {
    return snapshotTransportV2Auth(apiOrigin, authority.purpose);
  }
  return null;
}

function credentialsSnapshot(credentials: StoredTransportV2Credentials): TransportV2AuthSnapshot {
  return {
    apiOrigin: credentials.apiOrigin,
    kind: credentials.kind,
    principalId: credentials.principalId,
    generation: credentials.generation
  };
}

function isAuthenticatedSessionExhausted(status: number, body: Uint8Array | null): boolean {
  if (status !== 503 || body === null) return false;
  try {
    const value = requireExactObject(
      parseStrictJson(decodeUtf8(body)),
      ["error"],
      "Transport v2 session exhaustion response"
    );
    const error = requireExactObject(
      value.error,
      ["code", "message"],
      "Transport v2 session exhaustion error"
    );
    return (
      error.code === "session_exhausted" &&
      typeof error.message === "string" &&
      error.message.length > 0
    );
  } catch {
    return false;
  }
}

function credentialSessionLabel(credentials: StoredTransportV2Credentials): string {
  return `${credentials.kind}:${credentials.principalId}:${credentials.generation}`;
}

function bindingKind(path: string): TransportV2PrincipalKind | null {
  if (isUserBinding(path)) return "user";
  if (isPlatformBinding(path)) return "platform";
  return null;
}

export class TransportV2Client {
  #dependencies: TransportV2ClientDependencies;
  #sessions = new Map<string, ManagedSession>();
  #establishing = new Map<string, Promise<ManagedSession>>();
  #refreshing = new Map<string, Promise<Response>>();
  #apiKeyBindings = new Map<string, Promise<void>>();
  #bindingTransitions = new Set<string>();
  #apiKeyGenerations = new Map<string, number>();
  #refreshSuccessors = new Map<string, TransportV2AuthSnapshot>();

  constructor(dependencies: TransportV2ClientDependencies = defaultDependencies) {
    this.#dependencies = dependencies;
  }

  async fetch(input: TransportV2FetchInput): Promise<Response> {
    input.signal?.throwIfAborted();
    const apiOrigin = canonicalizeTransportV2ApiUrl(input.apiUrl);
    const pcrConfig = snapshotPcrConfig(input.pcrConfig);
    const pcrPolicy = serializePcrConfig(pcrConfig);
    const parts = requestParts(apiOrigin, input.url);
    const request: TransportV2LogicalRequest = {
      method: input.method,
      path: parts.path,
      query: parts.query,
      headers: logicalHeaders(input.headers),
      body: input.body
    };

    if (input.authority.kind === "api_key") {
      return this.#fetchWithApiKey(apiOrigin, pcrConfig, pcrPolicy, request, input);
    }

    const authSnapshot = authSnapshotForAuthority(apiOrigin, input.authority);
    if (authSnapshot && !isTransportV2AuthSnapshotCurrent(authSnapshot)) {
      throw new TransportV2AuthorityChangedError();
    }

    const transitionKind = bindingKind(parts.path);
    const transitionSlot = transitionKind
      ? this.#slot(apiOrigin, pcrPolicy, `binding:${transitionKind}`)
      : null;
    if (transitionSlot) {
      if (this.#bindingTransitions.has(transitionSlot)) {
        throw new Error(`A transport v2 ${transitionKind} authentication is already in progress.`);
      }
      this.#bindingTransitions.add(transitionSlot);
    }

    try {
      if (parts.path === "/refresh") {
        return this.refresh(apiOrigin, "user", pcrConfig);
      }
      if (parts.path === "/platform/refresh") {
        return this.refresh(apiOrigin, "platform", pcrConfig);
      }

      let managed: ManagedSession;
      if (isOAuthCallback(parts.path)) {
        const provider = oauthProvider(parts.path)!;
        const state = strictJsonObject(input.body).state;
        if (typeof state !== "string" || state.length === 0) {
          throw new Error("Transport v2 OAuth callback is missing its state.");
        }
        managed = this.#consumeOAuthContinuation(apiOrigin, pcrPolicy, provider, state);
      } else {
        managed = await this.#sessionForAuthority(apiOrigin, pcrConfig, pcrPolicy, input.authority);
      }

      const cacheRoot =
        isUserBinding(parts.path) && !isOAuthInitiation(parts.path)
          ? getOrCreateTransportV2CacheRoot(apiOrigin, this.#dependencies.crypto)
          : null;
      try {
        // Session establishment and request-body capture can both yield. Fence
        // the exact identity again immediately before the one network send.
        if (authSnapshot && !this.#isExpectedOrManagedAuthCurrent(authSnapshot, managed)) {
          throw new TransportV2AuthorityChangedError();
        }
        const result = await this.#send(
          managed,
          apiOrigin,
          pcrPolicy,
          request,
          input.responseMode,
          null,
          cacheRoot,
          input.signal
        );
        if (authSnapshot && !this.#isExpectedOrManagedAuthCurrent(authSnapshot, managed)) {
          this.#retireManagedSession(apiOrigin, pcrPolicy, managed);
          throw new TransportV2AuthorityChangedError();
        }
        if (
          result.response.status === 401 &&
          (managed.authority === "user" || managed.authority === "platform")
        ) {
          this.#retireManagedSession(apiOrigin, pcrPolicy, managed);
        }
        await this.#afterSuccessfulOperation(
          result,
          apiOrigin,
          pcrPolicy,
          parts.path,
          authSnapshot
        );
        return result.response;
      } catch (error) {
        if (isUserBinding(parts.path) || isPlatformBinding(parts.path)) {
          this.#retireManagedSession(apiOrigin, pcrPolicy, managed);
        }
        if (isTerminalUserOperation(parts.path) || isTerminalPlatformOperation(parts.path)) {
          this.#retireManagedSession(apiOrigin, pcrPolicy, managed);
        }
        throw error;
      } finally {
        cacheRoot?.fill(0);
      }
    } finally {
      if (transitionSlot) this.#bindingTransitions.delete(transitionSlot);
    }
  }

  async refresh(
    apiUrl: string,
    kind: TransportV2PrincipalKind,
    pcrConfig?: PcrConfig
  ): Promise<Response> {
    const apiOrigin = canonicalizeTransportV2ApiUrl(apiUrl);
    const policy = snapshotPcrConfig(pcrConfig);
    const pcrPolicy = serializePcrConfig(policy);
    const credentials = readTransportV2Credentials(apiOrigin, kind);
    if (!credentials) {
      clearLegacyTransportV1Credentials();
      throw new Error("A fresh transport v2 sign-in is required.");
    }
    const renewalSlot = this.#slot(
      apiOrigin,
      pcrPolicy,
      `resume:${kind}:${credentials.principalId}:${credentials.generation}`
    );
    let renewal = this.#refreshing.get(renewalSlot);
    if (!renewal) {
      renewal = this.#performRefresh(apiOrigin, kind, policy, pcrPolicy, credentials);
      this.#refreshing.set(renewalSlot, renewal);
    }
    try {
      return (await renewal).clone();
    } finally {
      if (this.#refreshing.get(renewalSlot) === renewal) {
        this.#refreshing.delete(renewalSlot);
      }
    }
  }

  async #performRefresh(
    apiOrigin: string,
    kind: TransportV2PrincipalKind,
    policy: PcrConfig,
    pcrPolicy: string,
    credentials: StoredTransportV2Credentials
  ): Promise<Response> {
    const expected = credentialsSnapshot(credentials);
    if (!isTransportV2AuthSnapshotCurrent(expected)) {
      throw new TransportV2AuthorityChangedError();
    }
    this.#retireCredentialSessions(apiOrigin, pcrPolicy, expected);
    const managed = await this.#establish(apiOrigin, policy, pcrPolicy, `resume:${kind}`);
    let root: Uint8Array | null | undefined;
    let credentialBytes: Uint8Array | undefined;
    try {
      root =
        kind === "user"
          ? getOrCreateTransportV2CacheRoot(apiOrigin, this.#dependencies.crypto)
          : null;
      const path = kind === "user" ? "/refresh" : "/platform/refresh";
      credentialBytes = encodeUtf8(credentials.refreshToken);
      if (!isTransportV2AuthSnapshotCurrent(expected)) {
        throw new TransportV2AuthorityChangedError();
      }
      const response = await this.#send(
        managed,
        apiOrigin,
        pcrPolicy,
        { method: "POST", path, query: null, headers: [], body: null },
        "unary",
        { kind: "resumption", value: credentialBytes },
        root,
        undefined
      );
      if (!response.response.ok) {
        managed.session.dispose();
        if (!clearTransportV2CredentialsIfCurrent(expected)) {
          throw new TransportV2AuthorityChangedError();
        }
        return response.response;
      }
      const installed = await this.#installBindingResponse(
        response.response,
        apiOrigin,
        kind,
        expected
      );
      this.#refreshSuccessors.set(this.#authSnapshotKey(expected), credentialsSnapshot(installed));
      response.session.authority = kind;
      response.session.principalId = installed.principalId;
      response.session.authGeneration = installed.generation;
      this.#sessions.set(
        this.#slot(apiOrigin, pcrPolicy, credentialSessionLabel(installed)),
        response.session
      );
      return response.response;
    } catch (error) {
      managed.session.dispose();
      throw error;
    } finally {
      // The descriptor itself remains persisted, but no temporary plaintext
      // credential bytes survive the encrypted binding operation.
      credentialBytes?.fill(0);
      root?.fill(0);
    }
  }

  async sessionInfo(
    apiUrl: string,
    pcrConfig: PcrConfig | undefined,
    authority: TransportV2Authority = { kind: "anonymous", purpose: "public" }
  ): Promise<TransportV2SessionInfo> {
    const apiOrigin = canonicalizeTransportV2ApiUrl(apiUrl);
    const policy = snapshotPcrConfig(pcrConfig);
    const pcrPolicy = serializePcrConfig(policy);
    if (authority.kind === "api_key") {
      throw new Error("API-key readiness is established by its first encrypted operation.");
    }
    const managed = await this.#sessionForAuthority(apiOrigin, policy, pcrPolicy, authority);
    return {
      protocolVersion: 2,
      sessionId: managed.session.sessionId,
      expiresAtUnixSeconds: managed.session.expiresAtUnixSeconds,
      authority: managed.authority
    };
  }

  clear(
    apiUrl: string,
    kind?: TransportV2PrincipalKind,
    purgeCacheRoot = false,
    expected?: TransportV2AuthSnapshot
  ): boolean {
    const apiOrigin = canonicalizeTransportV2ApiUrl(apiUrl);
    if (!kind) {
      for (const managed of this.#sessions.values()) managed.session.dispose();
      this.#sessions.clear();
      clearTransportV2Credentials(apiOrigin);
      this.#clearOAuthContinuations(apiOrigin);
      if (purgeCacheRoot) clearTransportV2CacheRoot(apiOrigin);
      return true;
    }

    let target = expected ?? snapshotTransportV2Auth(apiOrigin, kind);
    if (target.apiOrigin !== apiOrigin || target.kind !== kind) return false;
    if (!isTransportV2AuthSnapshotCurrent(target)) {
      const successor = this.#refreshSuccessors.get(this.#authSnapshotKey(target));
      if (!successor || !isTransportV2AuthSnapshotCurrent(successor)) return false;
      target = successor;
    }
    if (!clearTransportV2CredentialsIfCurrent(target)) {
      return false;
    }
    const anonymousLabel = kind === "user" ? "anonymous:user" : "anonymous:platform";
    for (const [key, managed] of this.#sessions) {
      if (!key.startsWith(`${apiOrigin}\n`)) continue;
      if (
        !(
          managed.authority === kind &&
          managed.principalId === target.principalId &&
          managed.authGeneration === target.generation
        ) &&
        (managed.authority !== "anonymous" || !key.endsWith(`\n${anonymousLabel}`))
      ) {
        continue;
      }
      managed.session.dispose();
      this.#sessions.delete(key);
    }
    if (kind === "user") this.#clearOAuthContinuations(apiOrigin);
    if (purgeCacheRoot) clearTransportV2CacheRoot(apiOrigin);
    return true;
  }

  retireAuthenticationState(apiUrl: string, kind: TransportV2PrincipalKind): void {
    const apiOrigin = canonicalizeTransportV2ApiUrl(apiUrl);
    const anonymousLabel = kind === "user" ? "anonymous:user" : "anonymous:platform";
    for (const [key, managed] of this.#sessions) {
      if (!key.startsWith(`${apiOrigin}\n`)) continue;
      if (
        managed.authority !== kind &&
        (managed.authority !== "anonymous" || !key.endsWith(`\n${anonymousLabel}`))
      ) {
        continue;
      }
      managed.session.dispose();
      this.#sessions.delete(key);
    }
    if (kind === "user") this.#clearOAuthContinuations(apiOrigin);
  }

  async retireApiKey(
    apiUrl: string,
    pcrConfig: PcrConfig | undefined,
    apiKey: string
  ): Promise<void> {
    const apiOrigin = canonicalizeTransportV2ApiUrl(apiUrl);
    const pcrPolicy = serializePcrConfig(snapshotPcrConfig(pcrConfig));
    const hash = await apiKeyHash(apiKey, this.#dependencies.crypto);
    const slot = this.#slot(apiOrigin, pcrPolicy, `api-key:${hash}`);
    this.#apiKeyGenerations.set(slot, (this.#apiKeyGenerations.get(slot) ?? 0) + 1);
    this.#sessions.get(slot)?.session.dispose();
    this.#sessions.delete(slot);
  }

  async #sessionForAuthority(
    apiOrigin: string,
    pcrConfig: PcrConfig,
    pcrPolicy: string,
    authority: Exclude<TransportV2Authority, { kind: "api_key" }>
  ): Promise<ManagedSession> {
    if (authority.kind === "anonymous") {
      const label = `anonymous:${authority.purpose}`;
      return this.#getOrEstablish(apiOrigin, pcrConfig, pcrPolicy, label);
    }

    const credentials = readTransportV2Credentials(apiOrigin, authority.kind);
    if (!credentials) {
      clearLegacyTransportV1Credentials();
      throw new Error("A fresh transport v2 sign-in is required.");
    }
    if (
      credentials.principalId !== authority.principalId ||
      credentials.generation !== authority.generation
    ) {
      throw new TransportV2AuthorityChangedError();
    }
    const label = credentialSessionLabel(credentials);
    const key = this.#slot(apiOrigin, pcrPolicy, label);
    const current = this.#sessions.get(key);
    const now = Math.floor(Date.now() / 1000);
    if (
      current &&
      !current.session.isDisposed &&
      current.session.expiresAtUnixSeconds > now &&
      current.principalId === credentials.principalId &&
      current.authGeneration === credentials.generation &&
      credentials.accessExpiresAtUnixSeconds > now + AUTH_RENEWAL_SKEW_SECONDS
    ) {
      return current;
    }
    current?.session.dispose();
    this.#sessions.delete(key);

    const inFlight = this.#establishing.get(key);
    if (inFlight) return inFlight;
    const renewal = (async () => {
      const response = await this.refresh(apiOrigin, authority.kind, pcrConfig);
      if (!response.ok) {
        throw Object.assign(new Error("Transport v2 session resumption was rejected."), {
          status: response.status,
          headers: new Headers(response.headers)
        });
      }
      const refreshed = readTransportV2Credentials(apiOrigin, authority.kind);
      if (!refreshed) throw new Error("Transport v2 session resumption returned no credentials.");
      if (refreshed.principalId !== credentials.principalId) {
        throw new TransportV2AuthorityChangedError();
      }
      if (!isTransportV2AuthSnapshotCurrent(credentialsSnapshot(refreshed))) {
        throw new TransportV2AuthorityChangedError();
      }
      const renewed = this.#sessions.get(
        this.#slot(apiOrigin, pcrPolicy, credentialSessionLabel(refreshed))
      );
      if (!renewed) throw new Error("Transport v2 session resumption did not bind a session.");
      return renewed;
    })();
    this.#establishing.set(key, renewal);
    try {
      return await renewal;
    } finally {
      if (this.#establishing.get(key) === renewal) this.#establishing.delete(key);
    }
  }

  async #fetchWithApiKey(
    apiOrigin: string,
    pcrConfig: PcrConfig,
    pcrPolicy: string,
    request: TransportV2LogicalRequest,
    input: TransportV2FetchInput
  ): Promise<Response> {
    if (input.authority.kind !== "api_key") {
      throw new Error("Transport v2 API-key authority is invalid.");
    }
    const rawApiKey = input.authority.value;
    const hash = await apiKeyHash(rawApiKey, this.#dependencies.crypto);
    const label = `api-key:${hash}`;
    const key = this.#slot(apiOrigin, pcrPolicy, label);
    const generation = this.#apiKeyGenerations.get(key) ?? 0;
    const current = this.#sessions.get(key);
    const now = Math.floor(Date.now() / 1000);
    if (current && !current.session.isDisposed && current.session.expiresAtUnixSeconds > now) {
      const result = await this.#send(
        current,
        apiOrigin,
        pcrPolicy,
        request,
        input.responseMode,
        null,
        null,
        input.signal
      );
      if (result.response.status === 401) {
        this.#retireManagedSession(apiOrigin, pcrPolicy, current);
      }
      return result.response;
    }
    current?.session.dispose();
    this.#sessions.delete(key);

    const binding = this.#apiKeyBindings.get(key);
    if (binding) {
      await binding;
      if ((this.#apiKeyGenerations.get(key) ?? 0) !== generation) {
        throw new TransportV2AuthorityChangedError();
      }
      return this.#fetchWithApiKey(apiOrigin, pcrConfig, pcrPolicy, request, input);
    }

    let resolveBinding!: () => void;
    const gate = new Promise<void>((resolve) => {
      resolveBinding = resolve;
    });
    this.#apiKeyBindings.set(key, gate);
    let managed: ManagedSession | undefined;
    let root: Uint8Array | undefined;
    let credentialBytes: Uint8Array | undefined;
    try {
      managed = await this.#establish(apiOrigin, pcrConfig, pcrPolicy, label);
      if ((this.#apiKeyGenerations.get(key) ?? 0) !== generation) {
        throw new TransportV2AuthorityChangedError();
      }
      root = getOrCreateTransportV2CacheRoot(apiOrigin, this.#dependencies.crypto);
      credentialBytes = encodeUtf8(rawApiKey);
      const credential: TransportV2Credential = {
        kind: "api_key",
        value: credentialBytes
      };
      const result = await this.#send(
        managed,
        apiOrigin,
        pcrPolicy,
        request,
        input.responseMode,
        credential,
        root,
        input.signal
      );
      if (result.response.ok) {
        if ((this.#apiKeyGenerations.get(key) ?? 0) !== generation) {
          throw new TransportV2AuthorityChangedError();
        }
        managed.authority = "api_key";
        this.#sessions.set(key, managed);
        resolveBinding();
      } else {
        managed.session.dispose();
        resolveBinding();
      }
      return result.response;
    } catch (error) {
      managed?.session.dispose();
      resolveBinding();
      throw error;
    } finally {
      credentialBytes?.fill(0);
      root?.fill(0);
      if (this.#apiKeyBindings.get(key) === gate) this.#apiKeyBindings.delete(key);
    }
  }

  async #getOrEstablish(
    apiOrigin: string,
    pcrConfig: PcrConfig,
    pcrPolicy: string,
    label: string
  ): Promise<ManagedSession> {
    const key = this.#slot(apiOrigin, pcrPolicy, label);
    const current = this.#sessions.get(key);
    const now = Math.floor(Date.now() / 1000);
    if (current && !current.session.isDisposed && current.session.expiresAtUnixSeconds > now) {
      return current;
    }
    current?.session.dispose();
    this.#sessions.delete(key);
    const inFlight = this.#establishing.get(key);
    if (inFlight) return inFlight;
    const establishing = this.#establish(apiOrigin, pcrConfig, pcrPolicy, label);
    this.#establishing.set(key, establishing);
    try {
      const managed = await establishing;
      this.#sessions.set(key, managed);
      return managed;
    } finally {
      if (this.#establishing.get(key) === establishing) this.#establishing.delete(key);
    }
  }

  async #establish(
    apiOrigin: string,
    pcrConfig: PcrConfig,
    _pcrPolicy: string,
    _label: string
  ): Promise<ManagedSession> {
    const nonce = this.#dependencies.crypto.randomUUID();
    const handshake = new TransportV2Handshake(nonce);
    try {
      const attestationResponse = await this.#dependencies.fetch(
        `${apiOrigin}/v2/attestation/${encodeURIComponent(nonce)}`,
        { method: "GET", credentials: "omit", cache: "no-store", redirect: "error" }
      );
      if (!attestationResponse.ok) {
        throw new Error(
          `Transport v2 attestation failed with status ${attestationResponse.status}.`
        );
      }
      const attestationBody = await readBoundedText(
        attestationResponse,
        MAX_ATTESTATION_RESPONSE_BYTES,
        "Transport v2 attestation response"
      );
      const parsed = parseStrictJson(attestationBody);
      if (
        typeof parsed !== "object" ||
        parsed === null ||
        Array.isArray(parsed) ||
        typeof (parsed as Record<string, unknown>).attestation_document !== "string"
      ) {
        throw new Error("Transport v2 attestation response is invalid.");
      }
      const document = await this.#dependencies.verifyAttestationDocument(
        (parsed as Record<string, string>).attestation_document,
        nonce,
        apiOrigin
      );
      if (!document.public_key || document.public_key.length !== 32) {
        throw new Error("Transport v2 attestation document has no valid public key.");
      }
      if (!isLocalDevelopmentApiUrl(apiOrigin)) {
        await requireTrustedPcr0(document.pcrs, pcrConfig, this.#dependencies.validatePcr0Hash);
      }

      const keyRequest = handshake.keyExchangeRequest();
      const keyResponse = await this.#dependencies.fetch(`${apiOrigin}${keyRequest.path}`, {
        method: keyRequest.method,
        headers: keyRequest.headers,
        body: keyRequest.body,
        credentials: "omit",
        redirect: "error"
      });
      if (!keyResponse.ok) {
        throw new Error(`Transport v2 key exchange failed with status ${keyResponse.status}.`);
      }
      const keyBody = await readBoundedText(
        keyResponse,
        MAX_HANDSHAKE_RESPONSE_BYTES,
        "Transport v2 key exchange response"
      );
      const session = await handshake.complete(
        new Uint8Array(document.public_key),
        keyBody,
        this.#dependencies.crypto.subtle,
        this.#dependencies.sessionResponseRecordLimit
      );
      return { session, authority: "anonymous" };
    } catch (error) {
      handshake.dispose();
      throw error;
    }
  }

  async #send(
    managed: ManagedSession,
    apiOrigin: string,
    pcrPolicy: string,
    request: TransportV2LogicalRequest,
    responseMode: ResponseMode,
    credential: TransportV2Credential | null,
    cacheNamespaceRoot: Uint8Array | null,
    signal?: AbortSignal | null
  ): Promise<SendResult> {
    signal?.throwIfAborted();
    let prepared: PreparedTransportV2Request;
    try {
      prepared = managed.session.prepareRequest({
        responseMode,
        credential,
        cacheNamespaceRoot,
        request
      });
    } catch (error) {
      if (error instanceof TransportV2SessionUnavailableError) {
        this.#retireManagedSession(apiOrigin, pcrPolicy, managed);
      }
      throw error;
    }
    const outer = prepared.takeHttpRequest();
    let response: Response;
    try {
      response = await this.#dependencies.fetch(`${apiOrigin}${outer.path}`, {
        method: outer.method,
        headers: outer.headers,
        body: outer.body,
        credentials: "omit",
        redirect: "error",
        signal
      });
    } catch (error) {
      prepared.dispose();
      this.#retireManagedSession(apiOrigin, pcrPolicy, managed);
      throw error;
    } finally {
      outer.body.fill(0);
    }

    if (responseMode === "unary") {
      if (response.status !== 200 || !hasExactContentType(response, "application/octet-stream")) {
        prepared.dispose();
        this.#retireManagedSession(apiOrigin, pcrPolicy, managed);
        throw new Error("Transport v2 returned an unauthenticated outer response.");
      }
      try {
        const body = await readBoundedBytes(
          response,
          MAX_OUTER_RESPONSE_BYTES,
          "Transport v2 outer response"
        );
        const logical = prepared.decryptUnaryResponse(body);
        if (isAuthenticatedSessionExhausted(logical.status, logical.body)) {
          this.#retireManagedSession(apiOrigin, pcrPolicy, managed);
        }
        return { response: logicalResponse(logical), session: managed };
      } catch (error) {
        prepared.dispose();
        this.#retireManagedSession(apiOrigin, pcrPolicy, managed);
        throw error;
      }
    }
    return {
      response: await this.#decodeStreamResponse(prepared, response, () =>
        this.#retireManagedSession(apiOrigin, pcrPolicy, managed)
      ),
      session: managed
    };
  }

  async #decodeStreamResponse(
    prepared: PreparedTransportV2Request,
    response: Response,
    retireSession: () => void
  ): Promise<Response> {
    if (!hasExactContentType(response, "text/event-stream")) {
      if (response.status !== 200 || !hasExactContentType(response, "application/octet-stream")) {
        prepared.dispose();
        retireSession();
        throw new Error("Transport v2 returned an unauthenticated outer stream response.");
      }
      try {
        const body = await readBoundedBytes(
          response,
          MAX_OUTER_RESPONSE_BYTES,
          "Transport v2 outer response"
        );
        const logical = prepared.decryptPreStartUnaryError(body);
        if (isAuthenticatedSessionExhausted(logical.status, logical.body)) retireSession();
        return logicalResponse(logical);
      } catch (error) {
        prepared.dispose();
        retireSession();
        throw error;
      }
    }
    if (response.status !== 200 || !response.body) {
      prepared.dispose();
      retireSession();
      throw new Error("Transport v2 stream response is invalid.");
    }

    const decoder = prepared.createStreamDecoder();
    const reader = response.body.getReader();
    let queued: TransportV2StreamRecord[] = [];
    let start: Extract<TransportV2StreamRecord, { kind: "start" }> | undefined;
    try {
      while (!start) {
        const next = await reader.read();
        if (next.done) {
          decoder.finish();
          throw new Error("Transport v2 stream ended before Start.");
        }
        queued.push(...decoder.push(next.value));
        const candidate = queued.shift();
        if (candidate?.kind === "start") start = candidate;
        else if (candidate) throw new Error("Transport v2 stream did not begin with Start.");
      }
    } catch (error) {
      decoder.dispose();
      await reader.cancel().catch(() => {});
      retireSession();
      throw error;
    }

    const body = new ReadableStream<Uint8Array>({
      async pull(controller) {
        try {
          while (true) {
            const record = queued.shift();
            if (record) {
              if (record.kind === "chunk") {
                controller.enqueue(record.body);
                return;
              }
              if (record.kind === "end") {
                // End authenticates logical finality, but the outer HTTP body
                // must also end there. Keep reading until EOF so a forwarding
                // host cannot hide bytes in a later carrier chunk after the
                // authenticated terminal record.
                while (true) {
                  const trailing = await reader.read();
                  if (trailing.done) break;
                  decoder.push(trailing.value);
                }
                decoder.finish();
                controller.close();
                return;
              }
              if (record.kind === "error") {
                const exhausted = isAuthenticatedSessionExhausted(record.status, record.body);
                const message = decodeUtf8(record.body);
                record.body.fill(0);
                decoder.finish();
                if (exhausted) retireSession();
                controller.error(Object.assign(new Error(message), { status: record.status }));
                return;
              }
              throw new Error("Transport v2 stream contains a duplicate Start.");
            }
            const next = await reader.read();
            if (next.done) {
              decoder.finish();
              controller.close();
              return;
            }
            queued.push(...decoder.push(next.value));
          }
        } catch (error) {
          decoder.dispose();
          retireSession();
          controller.error(error);
        }
      },
      async cancel() {
        decoder.dispose();
        await reader.cancel();
      }
    });
    return new Response(body, {
      status: start.status,
      headers: exactResponseHeaders(start.headers)
    });
  }

  async #afterSuccessfulOperation(
    result: SendResult,
    apiOrigin: string,
    pcrPolicy: string,
    path: string,
    expected: TransportV2AuthSnapshot | null
  ): Promise<void> {
    if (!result.response.ok) return;
    if (isOAuthInitiation(path)) {
      if (!expected || !isTransportV2AuthSnapshotCurrent(expected)) {
        throw new TransportV2AuthorityChangedError();
      }
      const provider = oauthProvider(path)!;
      const value = (await result.response.clone().json()) as Record<string, unknown>;
      const state = value.state;
      if (typeof state !== "string" || state.length === 0) {
        throw new Error("Transport v2 OAuth initiation returned no state.");
      }
      this.#persistOAuthContinuation(apiOrigin, pcrPolicy, provider, state, result.session.session);
      return;
    }
    if (isUserBinding(path)) {
      if (!expected || expected.kind !== "user") throw new TransportV2AuthorityChangedError();
      const credentials = await this.#installBindingResponse(
        result.response,
        apiOrigin,
        "user",
        expected
      );
      this.#removeManagedSessionReference(
        this.#slot(apiOrigin, pcrPolicy, "anonymous:user"),
        result.session
      );
      this.#retireCredentialSessions(apiOrigin, pcrPolicy, expected);
      result.session.authority = "user";
      result.session.principalId = credentials.principalId;
      result.session.authGeneration = credentials.generation;
      this.#sessions.set(
        this.#slot(apiOrigin, pcrPolicy, credentialSessionLabel(credentials)),
        result.session
      );
      return;
    }
    if (isPlatformBinding(path)) {
      if (!expected || expected.kind !== "platform") {
        throw new TransportV2AuthorityChangedError();
      }
      const credentials = await this.#installBindingResponse(
        result.response,
        apiOrigin,
        "platform",
        expected
      );
      this.#removeManagedSessionReference(
        this.#slot(apiOrigin, pcrPolicy, "anonymous:platform"),
        result.session
      );
      this.#retireCredentialSessions(apiOrigin, pcrPolicy, expected);
      result.session.authority = "platform";
      result.session.principalId = credentials.principalId;
      result.session.authGeneration = credentials.generation;
      this.#sessions.set(
        this.#slot(apiOrigin, pcrPolicy, credentialSessionLabel(credentials)),
        result.session
      );
      return;
    }

    if (path === "/protected/change_password" || path === "/platform/change-password") {
      const kind = path.startsWith("/platform/") ? "platform" : "user";
      if (!expected || expected.kind !== kind) throw new TransportV2AuthorityChangedError();
      const current = this.#currentAuthSnapshotForManaged(expected, result.session);
      try {
        await this.#installBindingResponse(result.response, apiOrigin, kind, current);
      } finally {
        this.#retireCredentialSessions(apiOrigin, pcrPolicy, current);
        this.#retireManagedSession(apiOrigin, pcrPolicy, result.session);
      }
      return;
    }
    if (isTerminalUserOperation(path)) {
      this.#retireManagedSession(apiOrigin, pcrPolicy, result.session);
      if (expected?.kind === "user") {
        const current = this.#currentAuthSnapshotForManaged(expected, result.session);
        this.#retireCredentialSessions(apiOrigin, pcrPolicy, current);
        const cleared = clearTransportV2CredentialsIfCurrent(current);
        if (cleared && path === "/protected/delete-account/confirm") {
          clearTransportV2CacheRoot(apiOrigin);
        }
      }
      return;
    }
    if (isTerminalPlatformOperation(path)) {
      this.#retireManagedSession(apiOrigin, pcrPolicy, result.session);
      if (expected?.kind === "platform") {
        const current = this.#currentAuthSnapshotForManaged(expected, result.session);
        this.#retireCredentialSessions(apiOrigin, pcrPolicy, current);
        clearTransportV2CredentialsIfCurrent(current);
      }
    }
  }

  async #installBindingResponse(
    response: Response,
    apiOrigin: string,
    kind: TransportV2PrincipalKind,
    expected: TransportV2AuthSnapshot
  ): Promise<StoredTransportV2Credentials> {
    const value = (await response.clone().json()) as Record<string, unknown>;
    if (typeof value.access_token !== "string" || typeof value.refresh_token !== "string") {
      throw new Error("Transport v2 authentication response returned no credentials.");
    }
    return installTransportV2Credentials(
      apiOrigin,
      kind,
      value.access_token,
      value.refresh_token,
      expected
    );
  }

  #persistOAuthContinuation(
    apiOrigin: string,
    pcrPolicy: string,
    provider: OAuthContinuation["provider"],
    state: string,
    session: TransportV2Session
  ): void {
    const continuation: OAuthContinuation = {
      version: 2,
      api_origin: apiOrigin,
      pcr_policy: pcrPolicy,
      provider,
      state,
      session: session.serialize()
    };
    safeSessionStorageSet(
      oauthContinuationKey(apiOrigin, pcrPolicy, provider),
      JSON.stringify(continuation)
    );
  }

  #consumeOAuthContinuation(
    apiOrigin: string,
    pcrPolicy: string,
    provider: OAuthContinuation["provider"],
    state: string
  ): ManagedSession {
    const key = oauthContinuationKey(apiOrigin, pcrPolicy, provider);
    const raw = safeSessionStorageGet(key);
    if (!raw) {
      throw new Error("OAuth attested session is unavailable; restart sign-in.");
    }
    let continuation: OAuthContinuation;
    try {
      try {
        continuation = parseOAuthContinuation(raw);
      } catch {
        safeSessionStorageRemove(key);
        throw new Error("Transport v2 OAuth continuation is invalid.");
      }
      if (
        continuation.api_origin !== apiOrigin ||
        continuation.pcr_policy !== pcrPolicy ||
        continuation.provider !== provider ||
        continuation.state !== state
      ) {
        throw new Error("OAuth callback does not match its attested session.");
      }
      if (continuation.session.expiresAtUnixSeconds <= Math.floor(Date.now() / 1000)) {
        safeSessionStorageRemove(key);
        throw new Error("OAuth attested session expired; restart sign-in.");
      }
      // Consume before network transmission. A callback is never transplanted
      // or retried under a replacement session.
      safeSessionStorageRemove(key);
      return { session: TransportV2Session.restore(continuation.session), authority: "anonymous" };
    } catch (error) {
      if (error instanceof Error) throw error;
      throw new Error("Transport v2 OAuth continuation is invalid.");
    }
  }

  #clearOAuthContinuations(apiOrigin: string): void {
    const storage = sessionStorageOrUndefined();
    if (!storage) return;
    const keys: string[] = [];
    try {
      for (let index = 0; index < storage.length; index += 1) {
        const key = storage.key(index);
        if (!key?.startsWith(OAUTH_CONTINUATION_PREFIX)) continue;
        const raw = storage.getItem(key);
        if (raw && raw.includes(`\"api_origin\":\"${apiOrigin}\"`)) keys.push(key);
      }
    } catch {
      return;
    }
    for (const key of keys) safeSessionStorageRemove(key);
  }

  #retireCredentialSessions(
    apiOrigin: string,
    pcrPolicy: string,
    expected: TransportV2AuthSnapshot
  ): void {
    for (const [key, managed] of this.#sessions) {
      if (!key.startsWith(`${apiOrigin}\n${pcrPolicy}\n`)) continue;
      if (
        managed.authority !== expected.kind ||
        managed.principalId !== expected.principalId ||
        managed.authGeneration !== expected.generation
      ) {
        continue;
      }
      managed.session.dispose();
      this.#sessions.delete(key);
    }
  }

  #removeManagedSessionReference(key: string, expected: ManagedSession): void {
    if (this.#sessions.get(key) === expected) this.#sessions.delete(key);
  }

  #isExpectedOrManagedAuthCurrent(
    expected: TransportV2AuthSnapshot,
    managed: ManagedSession
  ): boolean {
    if (isTransportV2AuthSnapshotCurrent(expected)) return true;
    if (
      managed.authority !== expected.kind ||
      managed.principalId !== expected.principalId ||
      managed.authGeneration === undefined
    ) {
      return false;
    }
    return isTransportV2AuthSnapshotCurrent({
      apiOrigin: expected.apiOrigin,
      kind: expected.kind,
      principalId: expected.principalId,
      generation: managed.authGeneration
    });
  }

  #currentAuthSnapshotForManaged(
    expected: TransportV2AuthSnapshot,
    managed: ManagedSession
  ): TransportV2AuthSnapshot {
    if (isTransportV2AuthSnapshotCurrent(expected)) return expected;
    if (
      managed.authority !== expected.kind ||
      managed.principalId !== expected.principalId ||
      managed.authGeneration === undefined
    ) {
      throw new TransportV2AuthorityChangedError();
    }
    const refreshed = {
      apiOrigin: expected.apiOrigin,
      kind: expected.kind,
      principalId: expected.principalId,
      generation: managed.authGeneration
    };
    if (!isTransportV2AuthSnapshotCurrent(refreshed)) {
      throw new TransportV2AuthorityChangedError();
    }
    return refreshed;
  }

  #retireManagedSession(apiOrigin: string, pcrPolicy: string, failed: ManagedSession): void {
    failed.session.dispose();
    const prefix = `${apiOrigin}\n${pcrPolicy}\n`;
    for (const [key, current] of this.#sessions) {
      if (key.startsWith(prefix) && current === failed) this.#sessions.delete(key);
    }
  }

  #slot(apiOrigin: string, pcrPolicy: string, label: string): string {
    return `${apiOrigin}\n${pcrPolicy}\n${label}`;
  }

  #authSnapshotKey(snapshot: TransportV2AuthSnapshot): string {
    return `${snapshot.apiOrigin}\n${snapshot.kind}\n${snapshot.principalId ?? ""}\n${snapshot.generation}`;
  }
}

export const transportV2Client = new TransportV2Client();
