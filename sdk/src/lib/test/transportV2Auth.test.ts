import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import {
  canonicalizeTransportV2ApiUrl,
  clearLegacyTransportV1Credentials,
  clearTransportV2CacheRoot,
  clearTransportV2Credentials,
  clearTransportV2CredentialsIfCurrent,
  commitTransportV2AuthBundleImport,
  exportTransportV2AuthBundle,
  getOrCreateTransportV2CacheRoot,
  importTransportV2AuthBundle,
  installTransportV2Credentials,
  prepareTransportV2AuthBundleImport,
  readTransportV2Credentials,
  setTransportV2CacheRoot,
  snapshotTransportV2Auth,
  subscribeTransportV2AuthInvalidation
} from "../transportV2/auth";

const API_URL = "https://auth.example.test/service";
const OTHER_API_URL = "https://other.example.test/service";
const CACHE_ROOT_PREFIX = "opensecret:transport-v2:cache-root:v1:";
const TOKEN_ISSUER = "urn:opensecret:transport-v2";
const USER_ACCESS_AUDIENCE = "urn:opensecret:internal:transport-v2:user:access-descriptor";
const USER_RESUMPTION_AUDIENCE = "urn:opensecret:internal:transport-v2:user:resumption";

function storageMock(): Storage {
  const values = new Map<string, string>();
  return {
    setItem(key: string, value: string) {
      values.set(key, value);
    },
    getItem(key: string) {
      return values.get(key) ?? null;
    },
    removeItem(key: string) {
      values.delete(key);
    },
    clear() {
      values.clear();
    },
    get length() {
      return values.size;
    },
    key(index: number) {
      return Array.from(values.keys())[index] ?? null;
    }
  } as Storage;
}

if (!globalThis.localStorage) {
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: storageMock()
  });
}

function unpaddedBase64Url(value: string | Uint8Array): string {
  return Buffer.from(value).toString("base64url");
}

function credential(
  tokenKind: "access_descriptor" | "resumption",
  principalId = "user-123",
  expiresAtUnixSeconds = 2_000_000_000
): string {
  const audience =
    tokenKind === "access_descriptor" ? USER_ACCESS_AUDIENCE : USER_RESUMPTION_AUDIENCE;
  const header = unpaddedBase64Url(JSON.stringify({ alg: "EdDSA", typ: "JWT" }));
  const claims = unpaddedBase64Url(
    JSON.stringify({
      iss: TOKEN_ISSUER,
      aud: audience,
      tv: 2,
      tk: tokenKind,
      pk: "user",
      sub: principalId,
      exp: expiresAtUnixSeconds
    })
  );
  const signature = unpaddedBase64Url(new Uint8Array(64).fill(0x5a));
  return `${header}.${claims}.${signature}`;
}

function installUser(apiUrl = API_URL) {
  const accessToken = credential("access_descriptor");
  const refreshToken = credential("resumption", "user-123", 2_000_003_600);
  const credentials = installTransportV2Credentials(apiUrl, "user", accessToken, refreshToken);
  return { accessToken, refreshToken, credentials };
}

function decodeBundle(bundle: string): Record<string, unknown> {
  return JSON.parse(Buffer.from(bundle, "base64url").toString("utf8")) as Record<string, unknown>;
}

function encodeBundle(bundle: Record<string, unknown>): string {
  return unpaddedBase64Url(JSON.stringify(bundle));
}

function storageKeys(): string[] {
  return Array.from({ length: globalThis.localStorage.length }, (_, index) =>
    globalThis.localStorage.key(index)
  )
    .filter((key): key is string => key !== null)
    .sort();
}

function fixedRandom(value: Uint8Array) {
  let calls = 0;
  const random = {
    getRandomValues<T extends ArrayBufferView | null>(array: T): T {
      calls += 1;
      if (!(array instanceof Uint8Array) || array.length !== value.length) {
        throw new Error("unexpected random request");
      }
      array.set(value);
      return array;
    }
  } as Crypto;
  return { random, calls: () => calls };
}

function cleanup(): void {
  for (const apiUrl of [API_URL, OTHER_API_URL, "https://api.example.test/base"]) {
    clearTransportV2Credentials(apiUrl);
    clearTransportV2CacheRoot(apiUrl);
  }
  clearLegacyTransportV1Credentials();
  globalThis.localStorage.clear();
}

beforeEach(cleanup);
afterEach(cleanup);

describe("transport v2 auth storage and transfer bundle", () => {
  test("canonicalizes one API scope before storing or reading credentials", () => {
    const configuredUrl = "HTTPS://API.EXAMPLE.TEST:443/base///";
    const canonicalUrl = "https://api.example.test/base";

    expect(canonicalizeTransportV2ApiUrl(configuredUrl)).toBe(canonicalUrl);
    expect(canonicalizeTransportV2ApiUrl("http://LOCALHOST:80/")).toBe("http://localhost");
    expect(() => canonicalizeTransportV2ApiUrl("http://api.example.test")).toThrow(
      "requires HTTPS"
    );
    expect(() => canonicalizeTransportV2ApiUrl("https://api.example.test/?query=1")).toThrow(
      "must not contain credentials, a query, or a fragment"
    );

    const installed = installUser(configuredUrl);
    expect(installed.credentials.apiOrigin).toBe(canonicalUrl);
    expect(readTransportV2Credentials(`${canonicalUrl}/`, "user")).toEqual(installed.credentials);
  });

  test("exports and imports an unpadded base64url bundle with exactly five fields", async () => {
    const { credentials } = installUser(API_URL);
    setTransportV2CacheRoot(
      API_URL,
      Uint8Array.from({ length: 32 }, (_, index) => index)
    );

    const exported = await exportTransportV2AuthBundle(`${API_URL}/`);
    expect(exported).toMatch(/^[A-Za-z0-9_-]+$/);
    expect(exported).not.toContain("=");

    const decoded = decodeBundle(exported);
    expect(Object.keys(decoded).sort()).toEqual(
      [
        "version",
        "api_origin",
        "access_token",
        "refresh_token",
        "cache_namespace_root_base64"
      ].sort()
    );
    expect(decoded.version).toBe(2);
    expect(decoded.api_origin).toBe(API_URL);

    await expect(importTransportV2AuthBundle(`${exported}=`, API_URL)).rejects.toThrow(
      "not canonical base64url"
    );
    await expect(
      importTransportV2AuthBundle(encodeBundle({ ...decoded, extra: true }), API_URL)
    ).rejects.toThrow("unexpected shape");
    const { refresh_token: _removed, ...missingField } = decoded;
    await expect(importTransportV2AuthBundle(encodeBundle(missingField), API_URL)).rejects.toThrow(
      "unexpected shape"
    );

    clearTransportV2Credentials(API_URL);
    clearTransportV2CacheRoot(API_URL);
    await importTransportV2AuthBundle(exported, API_URL);
    const imported = readTransportV2Credentials(API_URL, "user");
    expect(imported).toMatchObject({
      kind: credentials.kind,
      principalId: credentials.principalId,
      apiOrigin: credentials.apiOrigin,
      accessToken: credentials.accessToken,
      refreshToken: credentials.refreshToken,
      accessExpiresAtUnixSeconds: credentials.accessExpiresAtUnixSeconds
    });
    expect(imported!.generation).toBeGreaterThan(credentials.generation);
  });

  test("uses an exact 32-byte padded standard-base64 cache root", async () => {
    installUser(API_URL);
    const root = new Uint8Array(32).fill(0xff);
    const expectedBase64 = Buffer.from(root).toString("base64");
    setTransportV2CacheRoot(API_URL, root);

    const decoded = decodeBundle(await exportTransportV2AuthBundle(API_URL));
    expect(decoded.cache_namespace_root_base64).toBe(expectedBase64);
    expect(expectedBase64).toMatch(/^[A-Za-z0-9+/]+=$/);
    expect(Buffer.from(expectedBase64, "base64")).toHaveLength(32);

    const unpadded = {
      ...decoded,
      cache_namespace_root_base64: expectedBase64.replace(/=+$/u, "")
    };
    await expect(importTransportV2AuthBundle(encodeBundle(unpadded), API_URL)).rejects.toThrow(
      "cache root is invalid"
    );

    const urlSafe = {
      ...decoded,
      cache_namespace_root_base64: expectedBase64.replaceAll("/", "_")
    };
    await expect(importTransportV2AuthBundle(encodeBundle(urlSafe), API_URL)).rejects.toThrow();

    const shortRoot = {
      ...decoded,
      cache_namespace_root_base64: Buffer.from(new Uint8Array(31)).toString("base64")
    };
    await expect(importTransportV2AuthBundle(encodeBundle(shortRoot), API_URL)).rejects.toThrow(
      "cache root is invalid"
    );
  });

  test("rejects a bundle bound to another canonical API origin without side effects", async () => {
    installUser(API_URL);
    setTransportV2CacheRoot(API_URL, new Uint8Array(32).fill(0x33));
    const exported = await exportTransportV2AuthBundle(API_URL);
    const keysBefore = storageKeys();

    await expect(importTransportV2AuthBundle(exported, OTHER_API_URL)).rejects.toThrow(
      "belongs to a different API origin"
    );

    expect(readTransportV2Credentials(OTHER_API_URL, "user")).toBeNull();
    expect(storageKeys()).toEqual(keysBefore);
  });

  test("ignores legacy compatibility credentials and clears them without clearing v2 state", () => {
    globalThis.localStorage.setItem("access_token", credential("access_descriptor", "legacy-user"));
    globalThis.localStorage.setItem("refresh_token", credential("resumption", "legacy-user"));

    expect(readTransportV2Credentials(API_URL, "user")).toBeNull();
    clearLegacyTransportV1Credentials();
    expect(globalThis.localStorage.getItem("access_token")).toBeNull();
    expect(globalThis.localStorage.getItem("refresh_token")).toBeNull();

    const installed = installUser(API_URL);
    clearLegacyTransportV1Credentials();
    expect(globalThis.localStorage.getItem("access_token")).toBeNull();
    expect(globalThis.localStorage.getItem("refresh_token")).toBeNull();
    expect(readTransportV2Credentials(API_URL, "user")).toEqual(installed.credentials);
  });

  test("creates one stable cache root for canonically equivalent API URLs", () => {
    const expected = Uint8Array.from({ length: 32 }, (_, index) => 0xa0 + index);
    const source = fixedRandom(expected);
    const first = getOrCreateTransportV2CacheRoot(`${API_URL}///`, source.random);
    expect(first).toEqual(expected);
    expect(source.calls()).toBe(1);

    first.fill(0);
    const neverRandom = {
      getRandomValues(): never {
        throw new Error("stored cache root should be reused");
      }
    } as unknown as Crypto;
    const second = getOrCreateTransportV2CacheRoot(API_URL, neverRandom);
    expect(second).toEqual(expected);

    const cacheRootKey = storageKeys().find((key) => key.startsWith(CACHE_ROOT_PREFIX));
    expect(cacheRootKey).toBeDefined();
    expect(globalThis.localStorage.getItem(cacheRootKey!)).toBe(
      Buffer.from(expected).toString("base64")
    );
  });

  test("stale installs and clears cannot overwrite or remove a newer principal", () => {
    installUser(API_URL);
    const stale = snapshotTransportV2Auth(API_URL, "user");
    const newer = installTransportV2Credentials(
      API_URL,
      "user",
      credential("access_descriptor", "user-456"),
      credential("resumption", "user-456", 2_000_003_600)
    );

    expect(() =>
      installTransportV2Credentials(
        API_URL,
        "user",
        credential("access_descriptor", "user-123"),
        credential("resumption", "user-123", 2_000_003_600),
        stale
      )
    ).toThrow("authentication state changed");
    expect(clearTransportV2CredentialsIfCurrent(stale)).toBe(false);
    expect(readTransportV2Credentials(API_URL, "user")).toEqual(newer);
  });

  test("notifies React auth for principal replacement and exact current invalidation", () => {
    installUser(API_URL);
    const current = snapshotTransportV2Auth(API_URL, "user");
    let invalidations = 0;
    const unsubscribe = subscribeTransportV2AuthInvalidation(API_URL, "user", () => {
      invalidations += 1;
    });
    try {
      const newer = installTransportV2Credentials(
        API_URL,
        "user",
        credential("access_descriptor", "user-456"),
        credential("resumption", "user-456", 2_000_003_600)
      );
      expect(clearTransportV2CredentialsIfCurrent(current)).toBe(false);
      expect(invalidations).toBe(1);
      expect(clearTransportV2CredentialsIfCurrent(snapshotTransportV2Auth(API_URL, "user"))).toBe(
        true
      );
      expect(invalidations).toBe(2);
      expect(newer.principalId).toBe("user-456");
    } finally {
      unsubscribe();
    }
  });

  test("a prepared bundle import cannot replace credentials installed after its snapshot", async () => {
    installUser(API_URL);
    setTransportV2CacheRoot(API_URL, new Uint8Array(32).fill(0x44));
    const bundle = await exportTransportV2AuthBundle(API_URL);
    const expected = snapshotTransportV2Auth(API_URL, "user");
    const prepared = prepareTransportV2AuthBundleImport(bundle, API_URL);
    const newer = installTransportV2Credentials(
      API_URL,
      "user",
      credential("access_descriptor", "user-456"),
      credential("resumption", "user-456", 2_000_003_600)
    );

    expect(() => commitTransportV2AuthBundleImport(prepared, expected)).toThrow(
      "authentication state changed"
    );
    expect(readTransportV2Credentials(API_URL, "user")).toEqual(newer);
  });
});
