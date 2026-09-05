import { decode, decodeURLSafe, encode, encodeURLSafe } from "@stablelib/base64";
import { decodeUtf8, encodeUtf8, parseStrictJson, requireExactObject } from "./encoding";

const TOKEN_LIMIT_BYTES = 16 * 1024;
const AUTH_BUNDLE_LIMIT_BYTES = 64 * 1024;
const AUTH_STORAGE_PREFIX = "opensecret:transport-v2:auth:v1:";
const CACHE_ROOT_STORAGE_PREFIX = "opensecret:transport-v2:cache-root:v1:";

const USER_ACCESS_AUDIENCE = "urn:opensecret:internal:transport-v2:user:access-descriptor";
const USER_RESUMPTION_AUDIENCE = "urn:opensecret:internal:transport-v2:user:resumption";
const PLATFORM_ACCESS_AUDIENCE = "urn:opensecret:internal:transport-v2:platform:access-descriptor";
const PLATFORM_RESUMPTION_AUDIENCE = "urn:opensecret:internal:transport-v2:platform:resumption";
const TOKEN_ISSUER = "urn:opensecret:transport-v2";

export type TransportV2PrincipalKind = "user" | "platform";

export interface StoredTransportV2Credentials {
  kind: TransportV2PrincipalKind;
  principalId: string;
  apiOrigin: string;
  generation: number;
  accessToken: string;
  refreshToken: string;
  accessExpiresAtUnixSeconds: number;
}

/** A process-local compare-and-swap token for one persisted authority slot. */
export interface TransportV2AuthSnapshot {
  kind: TransportV2PrincipalKind;
  principalId: string | null;
  apiOrigin: string;
  generation: number;
}

export class TransportV2AuthorityChangedError extends Error {
  constructor() {
    super("Transport v2 authentication state changed while the operation was in progress.");
    this.name = "TransportV2AuthorityChangedError";
  }
}

interface StoredAuthMarker {
  version: 2;
  principal_kind: TransportV2PrincipalKind;
  principal_id: string;
  api_origin: string;
  generation: number;
  access_expires_at_unix_seconds: number;
}

interface TransportV2AuthBundle {
  version: 2;
  api_origin: string;
  access_token: string;
  refresh_token: string;
  cache_namespace_root_base64: string;
}

export interface PreparedTransportV2AuthBundleImport {
  apiOrigin: string;
  accessToken: string;
  refreshToken: string;
  cacheNamespaceRoot: Uint8Array;
}

interface TokenHints {
  kind: TransportV2PrincipalKind;
  principalId: string;
  expiresAtUnixSeconds: number;
}

const memoryStorage = new Map<string, string>();
const authInvalidationListeners = new Set<{
  apiOrigin: string;
  kind: TransportV2PrincipalKind;
  listener: () => void;
}>();

function storage(): Storage | undefined {
  try {
    return globalThis.localStorage;
  } catch {
    return undefined;
  }
}

function readStorage(key: string): string | null {
  try {
    return storage()?.getItem(key) ?? memoryStorage.get(key) ?? null;
  } catch {
    return memoryStorage.get(key) ?? null;
  }
}

function writeStorage(key: string, value: string): void {
  memoryStorage.set(key, value);
  try {
    storage()?.setItem(key, value);
  } catch {
    // Sandboxed browser contexts may not expose persistent storage. The
    // in-memory value remains valid for this process.
  }
}

function removeStorage(key: string): void {
  memoryStorage.delete(key);
  try {
    storage()?.removeItem(key);
  } catch {
    // Best-effort removal from unavailable storage; the in-memory copy is gone.
  }
}

function withoutBase64Padding(value: string): string {
  return value.replace(/=+$/u, "");
}

function paddedBase64Url(value: string): string {
  if (!/^[A-Za-z0-9_-]+$/u.test(value)) {
    throw new Error("Transport v2 value is not canonical base64url.");
  }
  const remainder = value.length % 4;
  if (remainder === 1) {
    throw new Error("Transport v2 value is not canonical base64url.");
  }
  return `${value}${"=".repeat((4 - remainder) % 4)}`;
}

function decodeUnpaddedBase64Url(value: string, limit: number): Uint8Array {
  if (encodeUtf8(value).length > Math.ceil(limit / 3) * 4) {
    throw new Error("Transport v2 encoded value exceeds its size limit.");
  }
  const decoded = decodeURLSafe(paddedBase64Url(value));
  if (decoded.length > limit || withoutBase64Padding(encodeURLSafe(decoded)) !== value) {
    decoded.fill(0);
    throw new Error("Transport v2 value is not canonical base64url.");
  }
  return decoded;
}

function storageScope(apiOrigin: string): string {
  return withoutBase64Padding(encodeURLSafe(encodeUtf8(apiOrigin)));
}

function authStorageKey(kind: TransportV2PrincipalKind, apiOrigin: string, field: string): string {
  return `${AUTH_STORAGE_PREFIX}${kind}:${storageScope(apiOrigin)}:${field}`;
}

function readAuthGeneration(apiOrigin: string, kind: TransportV2PrincipalKind): number {
  const raw = readStorage(authStorageKey(kind, apiOrigin, "generation"));
  if (raw === null) return 0;
  if (!/^(?:0|[1-9][0-9]*)$/u.test(raw)) return 0;
  const value = Number(raw);
  return Number.isSafeInteger(value) && value >= 0 ? value : 0;
}

function nextAuthGeneration(current: number): number {
  if (!Number.isSafeInteger(current) || current < 0 || current >= Number.MAX_SAFE_INTEGER) {
    throw new Error("Transport v2 authentication generation is exhausted.");
  }
  return current + 1;
}

function removeCredentialFields(apiOrigin: string, kind: TransportV2PrincipalKind): void {
  for (const field of ["marker", "access", "refresh"] as const) {
    removeStorage(authStorageKey(kind, apiOrigin, field));
  }
}

function clearCredentialFieldsAtGeneration(
  apiOrigin: string,
  kind: TransportV2PrincipalKind,
  generation: number,
  notify: boolean
): boolean {
  if (readAuthGeneration(apiOrigin, kind) !== generation) return false;
  removeCredentialFields(apiOrigin, kind);
  writeStorage(
    authStorageKey(kind, apiOrigin, "generation"),
    String(nextAuthGeneration(generation))
  );
  if (notify) notifyAuthInvalidated(apiOrigin, kind);
  return true;
}

function notifyAuthInvalidated(apiOrigin: string, kind: TransportV2PrincipalKind): void {
  for (const subscription of authInvalidationListeners) {
    if (subscription.apiOrigin === apiOrigin && subscription.kind === kind) {
      subscription.listener();
    }
  }
}

function cacheRootStorageKey(apiOrigin: string): string {
  return `${CACHE_ROOT_STORAGE_PREFIX}${storageScope(apiOrigin)}`;
}

export function canonicalizeTransportV2ApiUrl(apiUrl: string): string {
  let url: URL;
  try {
    url = new URL(apiUrl);
  } catch {
    throw new Error("Transport v2 requires a valid API URL.");
  }

  if (url.protocol !== "https:" && url.protocol !== "http:") {
    throw new Error("Transport v2 API URL must use HTTP or HTTPS.");
  }
  if (url.username || url.password || url.search || url.hash) {
    throw new Error("Transport v2 API URL must not contain credentials, a query, or a fragment.");
  }
  const localHosts = new Set(["127.0.0.1", "localhost", "0.0.0.0", "[::1]"]);
  if (url.protocol !== "https:" && !localHosts.has(url.hostname.toLowerCase())) {
    throw new Error("Transport v2 requires HTTPS outside exact loopback development.");
  }

  const path = url.pathname === "/" ? "" : url.pathname.replace(/\/+$/u, "");
  return `${url.origin}${path}`;
}

function parseTokenClaims(token: string): Record<string, unknown> {
  if (encodeUtf8(token).length === 0 || encodeUtf8(token).length > TOKEN_LIMIT_BYTES) {
    throw new Error("Transport v2 credential has an invalid length.");
  }
  const parts = token.split(".");
  if (parts.length !== 3 || parts.some((part) => part.length === 0)) {
    throw new Error("Transport v2 credential is not a JWT.");
  }
  const payload = decodeUnpaddedBase64Url(parts[1], TOKEN_LIMIT_BYTES);
  try {
    const parsed = parseStrictJson(decodeUtf8(payload));
    if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
      throw new Error("Transport v2 credential claims are invalid.");
    }
    return parsed as Record<string, unknown>;
  } finally {
    payload.fill(0);
  }
}

function tokenHints(
  token: string,
  expectedKind: TransportV2PrincipalKind,
  expectedTokenKind: "access_descriptor" | "resumption"
): TokenHints {
  const claims = parseTokenClaims(token);
  const expectedAudience =
    expectedKind === "user"
      ? expectedTokenKind === "access_descriptor"
        ? USER_ACCESS_AUDIENCE
        : USER_RESUMPTION_AUDIENCE
      : expectedTokenKind === "access_descriptor"
        ? PLATFORM_ACCESS_AUDIENCE
        : PLATFORM_RESUMPTION_AUDIENCE;
  if (
    claims.iss !== TOKEN_ISSUER ||
    claims.aud !== expectedAudience ||
    claims.tv !== 2 ||
    claims.tk !== expectedTokenKind ||
    claims.pk !== expectedKind ||
    typeof claims.sub !== "string" ||
    claims.sub.length === 0 ||
    typeof claims.exp !== "number" ||
    !Number.isSafeInteger(claims.exp) ||
    claims.exp <= 0
  ) {
    throw new Error("Transport v2 credential descriptor is invalid.");
  }
  return {
    kind: expectedKind,
    principalId: claims.sub,
    expiresAtUnixSeconds: claims.exp
  };
}

function parseStoredMarker(value: string): StoredAuthMarker {
  const marker = requireExactObject(
    parseStrictJson(value),
    [
      "version",
      "principal_kind",
      "principal_id",
      "api_origin",
      "generation",
      "access_expires_at_unix_seconds"
    ],
    "Transport v2 auth marker"
  );
  if (
    marker.version !== 2 ||
    (marker.principal_kind !== "user" && marker.principal_kind !== "platform") ||
    typeof marker.principal_id !== "string" ||
    marker.principal_id.length === 0 ||
    typeof marker.api_origin !== "string" ||
    typeof marker.generation !== "number" ||
    !Number.isSafeInteger(marker.generation) ||
    marker.generation <= 0 ||
    typeof marker.access_expires_at_unix_seconds !== "number" ||
    !Number.isSafeInteger(marker.access_expires_at_unix_seconds)
  ) {
    throw new Error("Transport v2 auth marker is invalid.");
  }
  return marker as unknown as StoredAuthMarker;
}

export function installTransportV2Credentials(
  apiUrl: string,
  kind: TransportV2PrincipalKind,
  accessToken: string,
  refreshToken: string,
  expected?: TransportV2AuthSnapshot
): StoredTransportV2Credentials {
  const apiOrigin = canonicalizeTransportV2ApiUrl(apiUrl);
  const access = tokenHints(accessToken, kind, "access_descriptor");
  const resumption = tokenHints(refreshToken, kind, "resumption");
  if (access.principalId !== resumption.principalId) {
    throw new Error("Transport v2 credential principals do not match.");
  }

  const current = snapshotTransportV2Auth(apiOrigin, kind);
  if (
    expected &&
    (expected.apiOrigin !== apiOrigin ||
      expected.kind !== kind ||
      !isTransportV2AuthSnapshotCurrent(expected))
  ) {
    throw new TransportV2AuthorityChangedError();
  }
  const generation = nextAuthGeneration(current.generation);

  const marker: StoredAuthMarker = {
    version: 2,
    principal_kind: kind,
    principal_id: access.principalId,
    api_origin: apiOrigin,
    generation,
    access_expires_at_unix_seconds: access.expiresAtUnixSeconds
  };
  // The marker is the commit record. JavaScript storage calls do not yield, so
  // readers in this process observe either the old generation or this complete
  // new generation, never a partially installed identity.
  writeStorage(authStorageKey(kind, apiOrigin, "access"), accessToken);
  writeStorage(authStorageKey(kind, apiOrigin, "refresh"), refreshToken);
  writeStorage(authStorageKey(kind, apiOrigin, "marker"), JSON.stringify(marker));
  writeStorage(authStorageKey(kind, apiOrigin, "generation"), String(generation));
  // React identity is invalidated only when the principal changes. Ordinary
  // same-principal refresh/import rotations are generation-fenced by the
  // transport manager and do not make the already-rendered identity false.
  if (current.principalId !== null && current.principalId !== access.principalId) {
    notifyAuthInvalidated(apiOrigin, kind);
  }

  return {
    kind,
    principalId: access.principalId,
    apiOrigin,
    generation,
    accessToken,
    refreshToken,
    accessExpiresAtUnixSeconds: access.expiresAtUnixSeconds
  };
}

export function readTransportV2Credentials(
  apiUrl: string,
  kind: TransportV2PrincipalKind
): StoredTransportV2Credentials | null {
  const apiOrigin = canonicalizeTransportV2ApiUrl(apiUrl);
  const markerKey = authStorageKey(kind, apiOrigin, "marker");
  const accessKey = authStorageKey(kind, apiOrigin, "access");
  const refreshKey = authStorageKey(kind, apiOrigin, "refresh");
  const markerValue = readStorage(markerKey);
  const accessToken = readStorage(accessKey);
  const refreshToken = readStorage(refreshKey);
  const generation = readAuthGeneration(apiOrigin, kind);
  if (!markerValue && !accessToken && !refreshToken) return null;

  try {
    if (!markerValue || !accessToken || !refreshToken) {
      throw new Error("Transport v2 stored credentials are incomplete.");
    }
    const marker = parseStoredMarker(markerValue);
    const access = tokenHints(accessToken, kind, "access_descriptor");
    const resumption = tokenHints(refreshToken, kind, "resumption");
    if (
      marker.api_origin !== apiOrigin ||
      marker.principal_kind !== kind ||
      marker.generation !== generation ||
      marker.principal_id !== access.principalId ||
      marker.principal_id !== resumption.principalId ||
      marker.access_expires_at_unix_seconds !== access.expiresAtUnixSeconds
    ) {
      throw new Error("Transport v2 stored credential binding is invalid.");
    }
    return {
      kind,
      principalId: access.principalId,
      apiOrigin,
      generation,
      accessToken,
      refreshToken,
      accessExpiresAtUnixSeconds: access.expiresAtUnixSeconds
    };
  } catch {
    const principalId = (() => {
      try {
        return markerValue ? parseStoredMarker(markerValue).principal_id : null;
      } catch {
        return null;
      }
    })();
    clearCredentialFieldsAtGeneration(apiOrigin, kind, generation, principalId !== null);
    return null;
  }
}

export function snapshotTransportV2Auth(
  apiUrl: string,
  kind: TransportV2PrincipalKind
): TransportV2AuthSnapshot {
  const apiOrigin = canonicalizeTransportV2ApiUrl(apiUrl);
  const credentials = readTransportV2Credentials(apiOrigin, kind);
  return {
    kind,
    principalId: credentials?.principalId ?? null,
    apiOrigin,
    generation: credentials?.generation ?? readAuthGeneration(apiOrigin, kind)
  };
}

export function isTransportV2AuthSnapshotCurrent(snapshot: TransportV2AuthSnapshot): boolean {
  const apiOrigin = canonicalizeTransportV2ApiUrl(snapshot.apiOrigin);
  if (apiOrigin !== snapshot.apiOrigin) return false;
  if (readAuthGeneration(apiOrigin, snapshot.kind) !== snapshot.generation) return false;
  const markerValue = readStorage(authStorageKey(snapshot.kind, apiOrigin, "marker"));
  if (!markerValue) return snapshot.principalId === null;
  try {
    const marker = parseStoredMarker(markerValue);
    return (
      marker.api_origin === apiOrigin &&
      marker.principal_kind === snapshot.kind &&
      marker.generation === snapshot.generation &&
      marker.principal_id === snapshot.principalId
    );
  } catch {
    return false;
  }
}

export function clearTransportV2CredentialsIfCurrent(expected: TransportV2AuthSnapshot): boolean {
  const apiOrigin = canonicalizeTransportV2ApiUrl(expected.apiOrigin);
  if (apiOrigin !== expected.apiOrigin || !isTransportV2AuthSnapshotCurrent(expected)) return false;
  return clearCredentialFieldsAtGeneration(
    apiOrigin,
    expected.kind,
    expected.generation,
    expected.principalId !== null
  );
}

export function clearTransportV2Credentials(apiUrl: string, kind?: TransportV2PrincipalKind): void {
  const apiOrigin = canonicalizeTransportV2ApiUrl(apiUrl);
  for (const selectedKind of kind ? [kind] : (["user", "platform"] as const)) {
    clearTransportV2CredentialsIfCurrent(snapshotTransportV2Auth(apiOrigin, selectedKind));
  }
  removeStorage("access_token");
  removeStorage("refresh_token");
}

/** @internal Used by React providers to mirror exact current credential invalidation. */
export function subscribeTransportV2AuthInvalidation(
  apiUrl: string,
  kind: TransportV2PrincipalKind,
  listener: () => void
): () => void {
  const subscription = {
    apiOrigin: canonicalizeTransportV2ApiUrl(apiUrl),
    kind,
    listener
  };
  authInvalidationListeners.add(subscription);
  return () => authInvalidationListeners.delete(subscription);
}

export function clearLegacyTransportV1Credentials(): void {
  removeStorage("access_token");
  removeStorage("refresh_token");
}

export function getOrCreateTransportV2CacheRoot(
  apiUrl: string,
  random: Crypto = globalThis.crypto
): Uint8Array {
  // One client-held root is intentionally stable per canonical API base. The
  // enclave combines it with the authenticated owner, so user and API-key
  // sessions for the same owner can share a provider-cache namespace while
  // the same root cannot collide across different verified owners.
  const apiOrigin = canonicalizeTransportV2ApiUrl(apiUrl);
  const key = cacheRootStorageKey(apiOrigin);
  const existing = readStorage(key);
  if (existing) {
    try {
      const decoded = decode(existing);
      if (decoded.length === 32 && encode(decoded) === existing) return decoded;
      decoded.fill(0);
    } catch {
      // Replace malformed local state with a fresh client-held root.
    }
    removeStorage(key);
  }
  const root = new Uint8Array(32);
  random.getRandomValues(root);
  writeStorage(key, encode(root));
  return root;
}

export function setTransportV2CacheRoot(apiUrl: string, root: Uint8Array): void {
  if (root.length !== 32) throw new Error("Transport v2 cache namespace root must be 32 bytes.");
  const apiOrigin = canonicalizeTransportV2ApiUrl(apiUrl);
  writeStorage(cacheRootStorageKey(apiOrigin), encode(root));
}

export function clearTransportV2CacheRoot(apiUrl: string): void {
  removeStorage(cacheRootStorageKey(canonicalizeTransportV2ApiUrl(apiUrl)));
}

export async function exportTransportV2AuthBundle(apiUrl: string): Promise<string> {
  const apiOrigin = canonicalizeTransportV2ApiUrl(apiUrl);
  const credentials = readTransportV2Credentials(apiOrigin, "user");
  if (!credentials) throw new Error("No transport v2 user credentials are available to export.");
  const root = getOrCreateTransportV2CacheRoot(apiOrigin);
  try {
    const bundle: TransportV2AuthBundle = {
      version: 2,
      api_origin: apiOrigin,
      access_token: credentials.accessToken,
      refresh_token: credentials.refreshToken,
      cache_namespace_root_base64: encode(root)
    };
    return withoutBase64Padding(encodeURLSafe(encodeUtf8(JSON.stringify(bundle))));
  } finally {
    root.fill(0);
  }
}

export function prepareTransportV2AuthBundleImport(
  bundle: string,
  expectedApiUrl: string
): PreparedTransportV2AuthBundleImport {
  const bytes = decodeUnpaddedBase64Url(bundle, AUTH_BUNDLE_LIMIT_BYTES);
  let root: Uint8Array | undefined;
  try {
    const value = requireExactObject(
      parseStrictJson(decodeUtf8(bytes)),
      ["version", "api_origin", "access_token", "refresh_token", "cache_namespace_root_base64"],
      "Transport v2 auth bundle"
    );
    if (
      value.version !== 2 ||
      typeof value.api_origin !== "string" ||
      typeof value.access_token !== "string" ||
      typeof value.refresh_token !== "string" ||
      typeof value.cache_namespace_root_base64 !== "string"
    ) {
      throw new Error("Transport v2 auth bundle is invalid.");
    }
    const apiOrigin = canonicalizeTransportV2ApiUrl(value.api_origin);
    if (apiOrigin !== canonicalizeTransportV2ApiUrl(expectedApiUrl)) {
      throw new Error("Transport v2 auth bundle belongs to a different API origin.");
    }
    root = decode(value.cache_namespace_root_base64);
    if (root.length !== 32 || encode(root) !== value.cache_namespace_root_base64) {
      throw new Error("Transport v2 auth bundle cache root is invalid.");
    }
    const canonicalBundle: TransportV2AuthBundle = {
      version: 2,
      api_origin: apiOrigin,
      access_token: value.access_token,
      refresh_token: value.refresh_token,
      cache_namespace_root_base64: value.cache_namespace_root_base64
    };
    const canonicalEncoding = withoutBase64Padding(
      encodeURLSafe(encodeUtf8(JSON.stringify(canonicalBundle)))
    );
    if (canonicalEncoding !== bundle) {
      throw new Error("Transport v2 auth bundle is not canonically encoded.");
    }
    // Validate both descriptors before the caller retires a currently usable
    // bound session. The backend remains authoritative when this resumption
    // credential is actually presented.
    const access = tokenHints(value.access_token, "user", "access_descriptor");
    const resumption = tokenHints(value.refresh_token, "user", "resumption");
    if (access.principalId !== resumption.principalId) {
      throw new Error("Transport v2 credential principals do not match.");
    }
    const prepared = {
      apiOrigin,
      accessToken: value.access_token,
      refreshToken: value.refresh_token,
      cacheNamespaceRoot: root
    };
    root = undefined;
    return prepared;
  } finally {
    root?.fill(0);
    bytes.fill(0);
  }
}

export function commitTransportV2AuthBundleImport(
  prepared: PreparedTransportV2AuthBundleImport,
  expected = snapshotTransportV2Auth(prepared.apiOrigin, "user")
): void {
  try {
    installTransportV2Credentials(
      prepared.apiOrigin,
      "user",
      prepared.accessToken,
      prepared.refreshToken,
      expected
    );
    setTransportV2CacheRoot(prepared.apiOrigin, prepared.cacheNamespaceRoot);
  } finally {
    prepared.cacheNamespaceRoot.fill(0);
  }
}

export async function importTransportV2AuthBundle(
  bundle: string,
  expectedApiUrl: string
): Promise<void> {
  const expected = snapshotTransportV2Auth(expectedApiUrl, "user");
  commitTransportV2AuthBundleImport(
    prepareTransportV2AuthBundleImport(bundle, expectedApiUrl),
    expected
  );
}
