import { beforeEach, describe, expect, test } from "bun:test";
import {
  authenticatedApiCallWithDependencies,
  encryptedApiCallWithDependencies,
  openAiAuthenticatedApiCallWithDependencies,
  type EncryptedApiDependencies
} from "../encryptedApi";
import type { Attestation } from "../getAttestation";
import { snapshotPcrConfig } from "../pcr";
import { ERROR_CODE_HEADER, ERROR_CONTRACT_HEADER } from "../recovery";
import { ACCOUNT_CREDENTIAL_MISMATCH_CODE } from "../credentialIdentity";

const staleKey = new Uint8Array(32).fill(1);
const freshKey = new Uint8Array(32).fill(2);
const staleAttestation: Attestation = { sessionKey: staleKey, sessionId: "stale-session" };
const freshAttestation: Attestation = { sessionKey: freshKey, sessionId: "fresh-session" };

function encryptForTest(sessionKey: Uint8Array, plaintext: string): string {
  return `${sessionKey[0]}:${plaintext}`;
}

function decryptForTest(sessionKey: Uint8Array, ciphertext: string): string {
  const prefix = `${sessionKey[0]}:`;
  if (!ciphertext.startsWith(prefix)) throw new Error(`wrong key ${sessionKey[0]}`);
  return ciphertext.slice(prefix.length);
}

function contractError(status: number, message: string, code?: string): Response {
  const headers = new Headers({ [ERROR_CONTRACT_HEADER]: "1" });
  if (code) headers.set(ERROR_CODE_HEADER, code);
  return Response.json({ status, message }, { status, headers });
}

function encryptedSuccess(sessionKey: Uint8Array, value: unknown): Response {
  return Response.json(
    { encrypted: encryptForTest(sessionKey, JSON.stringify(value)) },
    { headers: { [ERROR_CONTRACT_HEADER]: "1" } }
  );
}

function tokenForSubject(subject: string, generation = 1): string {
  const encode = (value: object) =>
    btoa(JSON.stringify(value)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  return `${encode({ alg: "ES256K", typ: "JWT" })}.${encode({ sub: subject, generation })}.sig`;
}

function dependencies(overrides: Partial<EncryptedApiDependencies> = {}): EncryptedApiDependencies {
  return {
    decryptMessage: decryptForTest,
    encryptMessage: encryptForTest,
    fetch: async () => new Response(null, { status: 500 }),
    getAttestation: async () => staleAttestation,
    getApiPcrConfig: () => snapshotPcrConfig({ environment: "development" }),
    getApiUrl: () => "https://api.example.test",
    getPlatformApiUrl: () => "https://platform.example.test",
    getPlatformPcrConfig: () => snapshotPcrConfig({ environment: "development" }),
    getAccessToken: () => window.localStorage.getItem("access_token"),
    refreshAccessToken: async () => {},
    resolveEndpoint: (url) => ({
      baseUrl: "https://api.example.test",
      context: url.includes("/platform/") ? "platform" : "app"
    }),
    ...overrides
  };
}

function recordedRequest(init: RequestInit | undefined, sessionKey: Uint8Array) {
  const headers = new Headers(init?.headers);
  const envelope = init?.body
    ? (JSON.parse(String(init.body)) as { encrypted: string })
    : undefined;
  return {
    authorization: headers.get("Authorization"),
    method: init?.method,
    plaintext: envelope ? decryptForTest(sessionKey, envelope.encrypted) : undefined,
    sessionId: headers.get("x-session-id")
  };
}

describe("encrypted API recovery", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
  });

  test("v1 session recovery re-encrypts one exact typed request", async () => {
    let currentAttestation = staleAttestation;
    let forcedAttestations = 0;
    const data = { operation: "original" };
    const urls: string[] = [];
    const requests: ReturnType<typeof recordedRequest>[] = [];
    const deps = dependencies({
      getAttestation: async (forceRefresh) => {
        if (forceRefresh) {
          forcedAttestations += 1;
          currentAttestation = freshAttestation;
        }
        return currentAttestation;
      },
      fetch: async (input, init) => {
        urls.push(String(input));
        const key =
          new Headers(init?.headers).get("x-session-id") === "stale-session" ? staleKey : freshKey;
        requests.push(recordedRequest(init, key));
        if (requests.length === 1) {
          data.operation = "mutated";
          return contractError(400, "Bad Request", "session_not_found");
        }
        return encryptedSuccess(freshKey, { ok: true });
      }
    });

    const result = await encryptedApiCallWithDependencies<{ operation: string }, { ok: boolean }>(
      "https://api.example.test/protected/action?mode=exact",
      "PATCH",
      data,
      "api-key",
      undefined,
      deps
    );

    expect(result).toEqual({ ok: true });
    expect(forcedAttestations).toBe(1);
    expect(urls).toEqual([
      "https://api.example.test/protected/action?mode=exact",
      "https://api.example.test/protected/action?mode=exact"
    ]);
    expect(requests).toEqual([
      {
        authorization: "Bearer api-key",
        method: "PATCH",
        plaintext: '{"operation":"original"}',
        sessionId: "stale-session"
      },
      {
        authorization: "Bearer api-key",
        method: "PATCH",
        plaintext: '{"operation":"original"}',
        sessionId: "fresh-session"
      }
    ]);
  });

  test("shares one session renewal across concurrent typed stale requests", async () => {
    const callCount = 8;
    let currentAttestation = staleAttestation;
    let forcedAttestations = 0;
    let attestationDocuments = 0;
    let keyExchanges = 0;
    let staleSends = 0;
    let releaseStaleResponses!: () => void;
    const allStaleRequestsStarted = new Promise<void>((resolve) => {
      releaseStaleResponses = resolve;
    });
    const requests: Array<
      ReturnType<typeof recordedRequest> & {
        ciphertext: string | undefined;
      }
    > = [];

    const deps = dependencies({
      getAttestation: async (forceRefresh) => {
        if (forceRefresh) {
          forcedAttestations += 1;
          attestationDocuments += 1;
          await new Promise((resolve) => setTimeout(resolve, 10));
          keyExchanges += 1;
          currentAttestation = freshAttestation;
        }
        return currentAttestation;
      },
      fetch: async (_input, init) => {
        const sessionId = new Headers(init?.headers).get("x-session-id");
        const key = sessionId === freshAttestation.sessionId ? freshKey : staleKey;
        const envelope = init?.body
          ? (JSON.parse(String(init.body)) as { encrypted: string })
          : undefined;
        const request = {
          ...recordedRequest(init, key),
          ciphertext: envelope?.encrypted
        };
        requests.push(request);

        if (sessionId === staleAttestation.sessionId) {
          staleSends += 1;
          if (staleSends === callCount) releaseStaleResponses();
          await allStaleRequestsStarted;
          return contractError(400, "Bad Request", "session_not_found");
        }

        const payload = JSON.parse(request.plaintext || "{}") as { operation?: string };
        return encryptedSuccess(freshKey, { operation: payload.operation });
      }
    });

    const results = await Promise.all(
      Array.from({ length: callCount }, (_, index) =>
        encryptedApiCallWithDependencies<{ operation: string }, { operation: string }>(
          "https://api.example.test/protected/action",
          "POST",
          { operation: `call-${index}` },
          "api-key",
          undefined,
          deps
        )
      )
    );

    expect(results).toEqual(
      Array.from({ length: callCount }, (_, index) => ({ operation: `call-${index}` }))
    );
    expect(forcedAttestations).toBe(1);
    expect(attestationDocuments).toBe(1);
    expect(keyExchanges).toBe(1);
    expect(requests.filter(({ sessionId }) => sessionId === "stale-session")).toHaveLength(
      callCount
    );
    const freshRequests = requests.filter(({ sessionId }) => sessionId === "fresh-session");
    expect(freshRequests).toHaveLength(callCount);
    expect(freshRequests.every(({ ciphertext }) => ciphertext?.startsWith("2:") === true)).toBe(
      true
    );
    expect(freshRequests.map(({ plaintext }) => plaintext).sort()).toEqual(
      Array.from({ length: callCount }, (_, index) => `{"operation":"call-${index}"}`).sort()
    );
  });

  test("a staggered typed stale response joins after the leader clears the cache", async () => {
    const lateKey = new Uint8Array(32).fill(3);
    const lateAttestation: Attestation = {
      sessionKey: lateKey,
      sessionId: "late-extra-session"
    };
    let currentAttestation: Attestation | null = staleAttestation;
    let forcedAttestations = 0;
    let fullHandshakes = 0;
    let staleSends = 0;
    let releaseCacheCleared!: () => void;
    const cacheCleared = new Promise<void>((resolve) => {
      releaseCacheCleared = resolve;
    });
    let releaseLateResponse!: () => void;
    const lateResponseReturned = new Promise<void>((resolve) => {
      releaseLateResponse = resolve;
    });
    const sessionIds: Array<string | null> = [];

    const deps = dependencies({
      getAttestation: async (forceRefresh) => {
        if (forceRefresh) {
          forcedAttestations += 1;
          fullHandshakes += 1;
          currentAttestation = null;
          releaseCacheCleared();
          await lateResponseReturned;
          await new Promise((resolve) => setTimeout(resolve, 10));
          currentAttestation = freshAttestation;
          return freshAttestation;
        }
        if (currentAttestation) return currentAttestation;

        fullHandshakes += 1;
        currentAttestation = lateAttestation;
        return lateAttestation;
      },
      fetch: async (_input, init) => {
        const sessionId = new Headers(init?.headers).get("x-session-id");
        sessionIds.push(sessionId);
        if (sessionId === staleAttestation.sessionId) {
          staleSends += 1;
          if (staleSends === 2) {
            await cacheCleared;
            releaseLateResponse();
          }
          return contractError(400, "Bad Request", "session_not_found");
        }

        const key = sessionId === freshAttestation.sessionId ? freshKey : lateKey;
        return encryptedSuccess(key, { ok: true });
      }
    });

    const results = await Promise.all(
      ["first", "late"].map((operation) =>
        encryptedApiCallWithDependencies<{ operation: string }, { ok: boolean }>(
          "https://api.example.test/protected/action",
          "POST",
          { operation },
          "api-key",
          undefined,
          deps
        )
      )
    );

    expect(results).toEqual([{ ok: true }, { ok: true }]);
    expect(forcedAttestations).toBe(1);
    expect(fullHandshakes).toBe(1);
    expect(sessionIds.filter((sessionId) => sessionId === "stale-session")).toHaveLength(2);
    expect(sessionIds.filter((sessionId) => sessionId === "fresh-session")).toHaveLength(2);
    expect(sessionIds).not.toContain("late-extra-session");
  });

  test("an authenticated request fails before transport if attestation yields to another account", async () => {
    window.localStorage.setItem("access_token", tokenForSubject("user-a"));
    let sends = 0;
    let refreshes = 0;
    const deps = dependencies({
      getAttestation: async () => {
        window.localStorage.setItem("access_token", tokenForSubject("user-b"));
        return staleAttestation;
      },
      refreshAccessToken: async () => {
        refreshes += 1;
      },
      fetch: async () => {
        sends += 1;
        return encryptedSuccess(staleKey, { ok: true });
      }
    });

    await expect(
      authenticatedApiCallWithDependencies<undefined, { ok: boolean }>(
        "https://api.example.test/protected/destructive-action",
        "DELETE",
        undefined,
        undefined,
        deps
      )
    ).rejects.toMatchObject({ code: ACCOUNT_CREDENTIAL_MISMATCH_CODE });
    expect(sends).toBe(0);
    expect(refreshes).toBe(0);
  });

  test("a provider-bound request rejects a replacement token before attestation", async () => {
    window.localStorage.setItem("access_token", tokenForSubject("user-b"));
    let attestations = 0;
    let sends = 0;
    const deps = dependencies({
      getAttestation: async () => {
        attestations += 1;
        return staleAttestation;
      },
      fetch: async () => {
        sends += 1;
        return encryptedSuccess(staleKey, { ok: true });
      }
    });

    await expect(
      authenticatedApiCallWithDependencies<undefined, { ok: boolean }>(
        "https://api.example.test/protected/destructive-action",
        "DELETE",
        undefined,
        undefined,
        deps,
        "user-a"
      )
    ).rejects.toMatchObject({ code: ACCOUNT_CREDENTIAL_MISMATCH_CODE });
    expect(attestations).toBe(0);
    expect(sends).toBe(0);
  });

  test("does not publish a successful response after the account changes in flight", async () => {
    window.localStorage.setItem("access_token", tokenForSubject("user-a"));
    let sends = 0;
    const deps = dependencies({
      fetch: async () => {
        sends += 1;
        window.localStorage.setItem("access_token", tokenForSubject("user-b"));
        return encryptedSuccess(staleKey, { private: "user-a" });
      }
    });

    await expect(
      authenticatedApiCallWithDependencies<undefined, { private: string }>(
        "https://api.example.test/protected/user",
        "GET",
        undefined,
        undefined,
        deps
      )
    ).rejects.toMatchObject({ code: ACCOUNT_CREDENTIAL_MISMATCH_CODE });
    expect(sends).toBe(1);
  });

  test("does not publish an encrypted result if the account changes while reading its body", async () => {
    window.localStorage.setItem("access_token", tokenForSubject("user-a"));
    let decryptions = 0;
    const deps = dependencies({
      decryptMessage: (sessionKey, ciphertext) => {
        decryptions += 1;
        return decryptForTest(sessionKey, ciphertext);
      },
      fetch: async () => {
        const response = encryptedSuccess(staleKey, { private: "user-a" });
        Object.defineProperty(response, "json", {
          value: async () => {
            await Promise.resolve();
            window.localStorage.setItem("access_token", tokenForSubject("user-b"));
            return { encrypted: encryptForTest(staleKey, '{"private":"user-a"}') };
          }
        });
        return response;
      }
    });

    await expect(
      authenticatedApiCallWithDependencies<undefined, { private: string }>(
        "https://api.example.test/protected/user",
        "GET",
        undefined,
        undefined,
        deps
      )
    ).rejects.toMatchObject({ code: ACCOUNT_CREDENTIAL_MISMATCH_CODE });
    expect(decryptions).toBe(0);
  });

  test("does not downgrade an account change during an error-body read to an HTTP error", async () => {
    window.localStorage.setItem("access_token", tokenForSubject("user-a"));
    const deps = dependencies({
      fetch: async () => {
        const response = contractError(403, "Forbidden");
        Object.defineProperty(response, "json", {
          value: async () => {
            await Promise.resolve();
            window.localStorage.setItem("access_token", tokenForSubject("user-b"));
            return { message: "Forbidden" };
          }
        });
        return response;
      }
    });

    await expect(
      authenticatedApiCallWithDependencies<undefined, never>(
        "https://api.example.test/protected/user",
        "DELETE",
        undefined,
        undefined,
        deps
      )
    ).rejects.toMatchObject({ code: ACCOUNT_CREDENTIAL_MISMATCH_CODE });
  });

  test("a 401 response cannot refresh or replay under a replacement account", async () => {
    const accessTokenA = tokenForSubject("user-a");
    const accessTokenB = tokenForSubject("user-b");
    window.localStorage.setItem("access_token", accessTokenA);
    let sends = 0;
    let refreshes = 0;
    const authorizations: Array<string | null> = [];
    const deps = dependencies({
      refreshAccessToken: async () => {
        refreshes += 1;
      },
      fetch: async (_input, init) => {
        sends += 1;
        authorizations.push(new Headers(init?.headers).get("Authorization"));
        window.localStorage.setItem("access_token", accessTokenB);
        return contractError(401, "Invalid JWT", "access_token_expired");
      }
    });

    await expect(
      authenticatedApiCallWithDependencies<undefined, never>(
        "https://api.example.test/protected/destructive-action",
        "DELETE",
        undefined,
        undefined,
        deps
      )
    ).rejects.toMatchObject({ code: ACCOUNT_CREDENTIAL_MISMATCH_CODE });
    expect(sends).toBe(1);
    expect(refreshes).toBe(0);
    expect(authorizations).toEqual([`Bearer ${accessTokenA}`]);
  });

  test("an account replacement during refresh cannot replay the retained request", async () => {
    const accessTokenA = tokenForSubject("user-a");
    const accessTokenB = tokenForSubject("user-b");
    window.localStorage.setItem("access_token", accessTokenA);
    let sends = 0;
    let refreshes = 0;
    const deps = dependencies({
      refreshAccessToken: async () => {
        refreshes += 1;
        window.localStorage.setItem("access_token", accessTokenB);
      },
      fetch: async () => {
        sends += 1;
        return sends === 1
          ? contractError(401, "Invalid JWT", "access_token_expired")
          : encryptedSuccess(staleKey, { ok: true });
      }
    });

    await expect(
      authenticatedApiCallWithDependencies<undefined, { ok: boolean }>(
        "https://api.example.test/protected/destructive-action",
        "DELETE",
        undefined,
        undefined,
        deps
      )
    ).rejects.toMatchObject({ code: ACCOUNT_CREDENTIAL_MISMATCH_CODE });
    expect(sends).toBe(1);
    expect(refreshes).toBe(1);
  });

  test("v1 access-token expiry refreshes once and replays with the new token", async () => {
    const expiredAccessToken = tokenForSubject("user-a", 1);
    const freshAccessToken = tokenForSubject("user-a", 2);
    window.localStorage.setItem("access_token", expiredAccessToken);
    let tokenRefreshes = 0;
    const authorizations: Array<string | null> = [];
    const deps = dependencies({
      refreshAccessToken: async () => {
        tokenRefreshes += 1;
        window.localStorage.setItem("access_token", freshAccessToken);
      },
      fetch: async (_input, init) => {
        authorizations.push(new Headers(init?.headers).get("Authorization"));
        return authorizations.length === 1
          ? contractError(401, "Invalid JWT", "access_token_expired")
          : encryptedSuccess(staleKey, { ok: true });
      }
    });

    expect(
      await authenticatedApiCallWithDependencies<undefined, { ok: boolean }>(
        "https://api.example.test/protected/user",
        "GET",
        undefined,
        undefined,
        deps
      )
    ).toEqual({ ok: true });
    expect(tokenRefreshes).toBe(1);
    expect(authorizations).toEqual([`Bearer ${expiredAccessToken}`, `Bearer ${freshAccessToken}`]);
  });

  for (const ordinary of [
    { status: 400, code: undefined, message: "Encryption error" },
    { status: 401, code: "invalid_jwt", message: "Invalid JWT" }
  ]) {
    test(`v1 ordinary ${ordinary.status} fails closed`, async () => {
      window.localStorage.setItem("access_token", tokenForSubject("user-a"));
      let sends = 0;
      let forcedAttestations = 0;
      let tokenRefreshes = 0;
      const deps = dependencies({
        getAttestation: async (forceRefresh) => {
          if (forceRefresh) forcedAttestations += 1;
          return staleAttestation;
        },
        refreshAccessToken: async () => {
          tokenRefreshes += 1;
        },
        fetch: async () => {
          sends += 1;
          return contractError(ordinary.status, ordinary.message, ordinary.code);
        }
      });

      await expect(
        authenticatedApiCallWithDependencies<undefined, never>(
          "https://api.example.test/protected/user",
          "GET",
          undefined,
          undefined,
          deps
        )
      ).rejects.toThrow(ordinary.message);
      expect(sends).toBe(1);
      expect(forcedAttestations).toBe(0);
      expect(tokenRefreshes).toBe(0);
    });
  }

  test("headerless 400 and 401 retain legacy recovery", async () => {
    const expiredAccessToken = tokenForSubject("user-a", 1);
    const freshAccessToken = tokenForSubject("user-a", 2);
    window.localStorage.setItem("access_token", expiredAccessToken);
    let currentAttestation = staleAttestation;
    let sessionSends = 0;
    let authSends = 0;
    let forcedAttestations = 0;
    let tokenRefreshes = 0;
    const deps = dependencies({
      getAttestation: async (forceRefresh) => {
        if (forceRefresh) {
          forcedAttestations += 1;
          currentAttestation = freshAttestation;
        }
        return currentAttestation;
      },
      refreshAccessToken: async () => {
        tokenRefreshes += 1;
        window.localStorage.setItem("access_token", freshAccessToken);
      },
      fetch: async (input) => {
        if (String(input).endsWith("/legacy-session")) {
          sessionSends += 1;
          return sessionSends === 1
            ? Response.json({ message: "Bad Request" }, { status: 400 })
            : encryptedSuccess(freshKey, { ok: "session" });
        }
        authSends += 1;
        return authSends === 1
          ? Response.json({ message: "Invalid JWT" }, { status: 401 })
          : encryptedSuccess(freshKey, { ok: "auth" });
      }
    });

    expect(
      await encryptedApiCallWithDependencies<undefined, { ok: string }>(
        "https://api.example.test/legacy-session",
        "GET",
        undefined,
        undefined,
        undefined,
        deps
      )
    ).toEqual({ ok: "session" });
    expect(
      await authenticatedApiCallWithDependencies<undefined, { ok: string }>(
        "https://api.example.test/protected/legacy-auth",
        "GET",
        undefined,
        undefined,
        deps
      )
    ).toEqual({ ok: "auth" });
    expect(sessionSends).toBe(2);
    expect(authSends).toBe(2);
    expect(forcedAttestations).toBe(1);
    expect(tokenRefreshes).toBe(1);
  });

  test("one target replay budget stops alternating recovery reasons", async () => {
    window.localStorage.setItem("access_token", tokenForSubject("user-a"));
    let currentAttestation = staleAttestation;
    let sends = 0;
    let forcedAttestations = 0;
    let tokenRefreshes = 0;
    const deps = dependencies({
      getAttestation: async (forceRefresh) => {
        if (forceRefresh) {
          forcedAttestations += 1;
          currentAttestation = freshAttestation;
        }
        return currentAttestation;
      },
      refreshAccessToken: async () => {
        tokenRefreshes += 1;
      },
      fetch: async () => {
        sends += 1;
        return sends === 1
          ? contractError(400, "Bad Request", "session_not_found")
          : contractError(401, "Invalid JWT", "access_token_expired");
      }
    });

    await expect(
      authenticatedApiCallWithDependencies<undefined, never>(
        "https://api.example.test/protected/action",
        "POST",
        undefined,
        undefined,
        deps
      )
    ).rejects.toThrow("Invalid JWT");
    expect(sends).toBe(2);
    expect(forcedAttestations).toBe(1);
    expect(tokenRefreshes).toBe(0);
  });

  test("expired target JWT can refresh through one stale-session repair", async () => {
    const expiredAccessToken = tokenForSubject("user-a", 1);
    const freshAccessToken = tokenForSubject("user-a", 2);
    window.localStorage.setItem("access_token", expiredAccessToken);
    let currentAttestation = staleAttestation;
    let forcedAttestations = 0;
    let targetSends = 0;
    let refreshSends = 0;
    const targetRequests: ReturnType<typeof recordedRequest>[] = [];
    const refreshRequests: ReturnType<typeof recordedRequest>[] = [];
    const deps = dependencies({
      getAttestation: async (forceRefresh) => {
        if (forceRefresh) {
          forcedAttestations += 1;
          currentAttestation = freshAttestation;
        }
        return currentAttestation;
      },
      refreshAccessToken: async () => {
        const tokens = await encryptedApiCallWithDependencies<
          { refresh_token: string },
          { access_token: string; refresh_token: string }
        >(
          "https://api.example.test/refresh",
          "POST",
          { refresh_token: "refresh-token" },
          undefined,
          undefined,
          deps
        );
        window.localStorage.setItem("access_token", tokens.access_token);
      },
      fetch: async (input, init) => {
        const url = String(input);
        const sessionId = new Headers(init?.headers).get("x-session-id");
        const key = sessionId === "fresh-session" ? freshKey : staleKey;
        if (url.includes("/refresh")) {
          refreshSends += 1;
          refreshRequests.push(recordedRequest(init, key));
          return refreshSends === 1
            ? contractError(400, "Bad Request", "session_not_found")
            : encryptedSuccess(freshKey, {
                access_token: freshAccessToken,
                refresh_token: "fresh-refresh-token"
              });
        }

        targetSends += 1;
        targetRequests.push(recordedRequest(init, key));
        return targetSends === 1
          ? contractError(401, "Invalid JWT", "access_token_expired")
          : encryptedSuccess(freshKey, { ok: true });
      }
    });

    expect(
      await authenticatedApiCallWithDependencies<{ prompt: string }, { ok: boolean }>(
        "https://api.example.test/v1/chat/completions?stream=false",
        "POST",
        { prompt: "same prompt" },
        undefined,
        deps
      )
    ).toEqual({ ok: true });
    expect(targetSends).toBe(2);
    expect(refreshSends).toBe(2);
    expect(forcedAttestations).toBe(1);
    expect(refreshRequests.map(({ plaintext }) => plaintext)).toEqual([
      '{"refresh_token":"refresh-token"}',
      '{"refresh_token":"refresh-token"}'
    ]);
    expect(targetRequests).toEqual([
      {
        authorization: `Bearer ${expiredAccessToken}`,
        method: "POST",
        plaintext: '{"prompt":"same prompt"}',
        sessionId: "stale-session"
      },
      {
        authorization: `Bearer ${freshAccessToken}`,
        method: "POST",
        plaintext: '{"prompt":"same prompt"}',
        sessionId: "fresh-session"
      }
    ]);
  });

  test("a successful response decryption failure never replays", async () => {
    let sends = 0;
    let forcedAttestations = 0;
    const deps = dependencies({
      getAttestation: async (forceRefresh) => {
        if (forceRefresh) forcedAttestations += 1;
        return staleAttestation;
      },
      fetch: async () => {
        sends += 1;
        return Response.json({ encrypted: '2:{"ok":true}' });
      }
    });

    await expect(
      encryptedApiCallWithDependencies<undefined, never>(
        "https://api.example.test/action",
        "POST",
        undefined,
        undefined,
        undefined,
        deps
      )
    ).rejects.toThrow("Failed to decrypt or parse the response");
    expect(sends).toBe(1);
    expect(forcedAttestations).toBe(0);
  });

  test("API-key 401 never invokes access-token refresh", async () => {
    let sends = 0;
    let tokenRefreshes = 0;
    const deps = dependencies({
      refreshAccessToken: async () => {
        tokenRefreshes += 1;
      },
      fetch: async () => {
        sends += 1;
        return contractError(401, "Invalid JWT", "access_token_expired");
      }
    });

    await expect(
      openAiAuthenticatedApiCallWithDependencies<undefined, never>(
        "https://api.example.test/v1/models",
        "GET",
        undefined,
        undefined,
        "api-key",
        deps
      )
    ).rejects.toThrow("Invalid JWT");
    expect(sends).toBe(1);
    expect(tokenRefreshes).toBe(0);
  });
});
