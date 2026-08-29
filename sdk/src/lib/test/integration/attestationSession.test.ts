import { afterEach, expect, mock, test } from "bun:test";
import { encode } from "@stablelib/base64";
import {
  cacheAttestationSessionForTesting,
  getAttestation,
  getAttestationSessionStorageKey
} from "../../getAttestation";

const originalFetch = globalThis.fetch;

afterEach(() => {
  window.sessionStorage.clear();
  globalThis.fetch = originalFetch;
});

test("session cache keys include the full normalized API base path and environment", async () => {
  const prodPath = await getAttestationSessionStorageKey("https://custom.example/prod/", {
    environment: "prod"
  });
  const devPath = await getAttestationSessionStorageKey("https://custom.example/dev", {
    environment: "prod"
  });
  const devEnvironment = await getAttestationSessionStorageKey("https://custom.example/prod", {
    environment: "dev"
  });

  expect(prodPath).not.toBe(devPath);
  expect(prodPath).not.toBe(devEnvironment);
  expect(prodPath).toBe(
    await getAttestationSessionStorageKey("https://custom.example/prod", { environment: "prod" })
  );
});

test("reads only a valid policy-scoped unexpired cached session", async () => {
  const apiUrl = "https://custom.example/prod";
  const policy = { environment: "prod" as const };
  const cacheKey = await getAttestationSessionStorageKey(apiUrl, policy);
  const sessionKey = new Uint8Array(32).fill(7);
  await cacheAttestationSessionForTesting(
    apiUrl,
    policy,
    { sessionKey, sessionId: "session-id" },
    "11".repeat(48)
  );
  globalThis.fetch = mock(async () => {
    throw new Error("cached sessions must not fetch");
  }) as typeof fetch;

  const result = await getAttestation(false, apiUrl, policy);
  expect(result.sessionKey).toEqual(sessionKey);
  expect(result.sessionId).toBe("session-id");
  expect(globalThis.fetch).not.toHaveBeenCalled();
});

test("never reads legacy unversioned session keys", async () => {
  window.sessionStorage.setItem("sessionKey", encode(new Uint8Array(32).fill(8)));
  window.sessionStorage.setItem("sessionId", "legacy-session");
  globalThis.fetch = mock(async () => {
    throw new Error("fresh attestation required");
  }) as typeof fetch;

  await expect(getAttestation(false, "http://localhost:31110")).rejects.toThrow(
    "fresh attestation required"
  );
  expect(globalThis.fetch).toHaveBeenCalled();
  expect(window.sessionStorage.getItem("sessionKey")).toBeNull();
  expect(window.sessionStorage.getItem("sessionId")).toBeNull();
});

test("rejects stale cached sessions before requesting a fresh attestation document", async () => {
  const apiUrl = "https://custom.example/prod";
  const policy = { environment: "prod" as const };
  const cacheKey = await getAttestationSessionStorageKey(apiUrl, policy);
  window.sessionStorage.setItem(
    cacheKey,
    JSON.stringify({
      sessionKey: encode(new Uint8Array(32).fill(7)),
      sessionId: "stale-cache-shape"
    })
  );
  globalThis.fetch = mock(async () => {
    throw new Error("fresh attestation required");
  }) as typeof fetch;

  await expect(getAttestation(false, apiUrl, policy)).rejects.toThrow(
    "TUF root has not been bootstrapped"
  );
  expect(globalThis.fetch).not.toHaveBeenCalled();
  expect(window.sessionStorage.getItem(cacheKey)).toBeNull();
});
