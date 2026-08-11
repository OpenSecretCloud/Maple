import { beforeEach, describe, expect, test } from "bun:test";
import { createCustomFetchWithDependencies, type CustomFetchDependencies } from "../ai";
import { getApiPcrConfig, getApiUrl, setApiUrl } from "../api";
import type { Attestation } from "../getAttestation";
import type { PcrConfig } from "../pcr";

const staleKey = new Uint8Array(32).fill(1);
const freshKey = new Uint8Array(32).fill(2);
const staleAttestation: Attestation = { sessionKey: staleKey, sessionId: "stale-session" };
const freshAttestation: Attestation = { sessionKey: freshKey, sessionId: "fresh-session" };

interface RecordedRequest {
  authorization: string | null;
  encryptedBody: string | undefined;
  sessionId: string | null;
}

function recordRequest(init?: RequestInit): RecordedRequest {
  const headers = new Headers(init?.headers);
  const body = init?.body ? (JSON.parse(init.body as string) as { encrypted?: string }) : undefined;

  return {
    authorization: headers.get("Authorization"),
    encryptedBody: body?.encrypted,
    sessionId: headers.get("x-session-id")
  };
}

function encryptForTest(sessionKey: Uint8Array, plaintext: string): string {
  return `${sessionKey[0]}:${plaintext}`;
}

function decryptForTest(sessionKey: Uint8Array, ciphertext: string): string {
  const expectedPrefix = `${sessionKey[0]}:`;
  if (!ciphertext.startsWith(expectedPrefix)) {
    throw new Error(`Ciphertext was not encrypted for key ${sessionKey[0]}`);
  }
  return ciphertext.slice(expectedPrefix.length);
}

function dependencies(overrides: Partial<CustomFetchDependencies>): CustomFetchDependencies {
  return {
    decryptMessage: decryptForTest,
    encryptMessage: encryptForTest,
    fetch: async () => new Response(null, { status: 500 }),
    getAttestation: async () => staleAttestation,
    refreshToken: async () => ({
      access_token: "refreshed-access-token",
      refresh_token: "refreshed-refresh-token"
    }),
    ...overrides
  };
}

describe("createCustomFetch stale-session recovery", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
  });

  test("renews once and rebuilds an API-key request with the fresh session", async () => {
    let currentAttestation = staleAttestation;
    let forcedAttestations = 0;
    let tokenRefreshes = 0;
    const requests: RecordedRequest[] = [];

    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "test-api-key" },
      dependencies({
        getAttestation: async (forceRefresh) => {
          if (forceRefresh) {
            forcedAttestations += 1;
            currentAttestation = freshAttestation;
          }
          return currentAttestation;
        },
        refreshToken: async () => {
          tokenRefreshes += 1;
          return {
            access_token: "unused-access-token",
            refresh_token: "unused-refresh-token"
          };
        },
        fetch: async (_input, init) => {
          const request = recordRequest(init);
          requests.push(request);

          if (request.sessionId === staleAttestation.sessionId) {
            return new Response('{"status":400,"message":"Bad Request"}', { status: 400 });
          }

          return Response.json({ encrypted: '2:{"ok":true}' });
        }
      })
    );

    const response = await customFetch("https://example.test/v1/responses", {
      method: "POST",
      body: '{"prompt":"hello"}'
    });

    expect(await response.json()).toEqual({ ok: true });
    expect(forcedAttestations).toBe(1);
    expect(tokenRefreshes).toBe(0);
    expect(requests).toEqual([
      {
        authorization: "Bearer test-api-key",
        encryptedBody: '1:{"prompt":"hello"}',
        sessionId: "stale-session"
      },
      {
        authorization: "Bearer test-api-key",
        encryptedBody: '2:{"prompt":"hello"}',
        sessionId: "fresh-session"
      }
    ]);
  });

  test("stops after one attestation retry when 400 persists", async () => {
    let currentAttestation = staleAttestation;
    let forcedAttestations = 0;
    let requests = 0;

    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "test-api-key" },
      dependencies({
        getAttestation: async (forceRefresh) => {
          if (forceRefresh) {
            forcedAttestations += 1;
            currentAttestation = freshAttestation;
          }
          return currentAttestation;
        },
        fetch: async () => {
          requests += 1;
          return new Response("still bad", { status: 400 });
        }
      })
    );

    await expect(
      customFetch("https://example.test/v1/responses", {
        method: "POST",
        body: '{"prompt":"hello"}'
      })
    ).rejects.toThrow("Request failed with status 400: still bad");

    expect(forcedAttestations).toBe(1);
    expect(requests).toBe(2);
  });

  test("rebuilds the request after a JWT refresh replaces the attestation", async () => {
    window.localStorage.setItem("access_token", "expired-access-token");
    let currentAttestation = staleAttestation;
    let tokenRefreshes = 0;
    let forcedAttestations = 0;
    const requests: RecordedRequest[] = [];

    const customFetch = createCustomFetchWithDependencies(
      undefined,
      dependencies({
        getAttestation: async (forceRefresh) => {
          if (forceRefresh) forcedAttestations += 1;
          return currentAttestation;
        },
        refreshToken: async () => {
          tokenRefreshes += 1;
          window.localStorage.setItem("access_token", "fresh-access-token");
          currentAttestation = freshAttestation;
          return {
            access_token: "fresh-access-token",
            refresh_token: "fresh-refresh-token"
          };
        },
        fetch: async (_input, init) => {
          requests.push(recordRequest(init));
          if (requests.length === 1) return new Response("expired JWT", { status: 401 });
          return Response.json({ encrypted: '2:{"ok":true}' });
        }
      })
    );

    const response = await customFetch("https://example.test/v1/responses", {
      method: "POST",
      body: '{"prompt":"hello"}'
    });

    expect(await response.json()).toEqual({ ok: true });
    expect(tokenRefreshes).toBe(1);
    expect(forcedAttestations).toBe(0);
    expect(requests).toEqual([
      {
        authorization: "Bearer expired-access-token",
        encryptedBody: '1:{"prompt":"hello"}',
        sessionId: "stale-session"
      },
      {
        authorization: "Bearer fresh-access-token",
        encryptedBody: '2:{"prompt":"hello"}',
        sessionId: "fresh-session"
      }
    ]);
  });

  test("keeps a stale retry bound to the identity that initiated the request", async () => {
    window.localStorage.setItem("access_token", "initiating-account-token");
    let currentAttestation = staleAttestation;
    const requests: RecordedRequest[] = [];

    const customFetch = createCustomFetchWithDependencies(
      undefined,
      dependencies({
        getAttestation: async (forceRefresh) => {
          if (forceRefresh) {
            // Simulate an unrelated account change while re-attestation is in
            // flight. The pending operation must retain its original token.
            window.localStorage.setItem("access_token", "different-account-token");
            currentAttestation = freshAttestation;
          }
          return currentAttestation;
        },
        fetch: async (_input, init) => {
          const request = recordRequest(init);
          requests.push(request);
          if (request.sessionId === staleAttestation.sessionId) {
            return new Response("stale", { status: 400 });
          }
          return Response.json({ encrypted: '2:{"ok":true}' });
        }
      })
    );

    const response = await customFetch("https://example.test/v1/responses", {
      method: "POST",
      body: '{"prompt":"hello"}'
    });

    expect(await response.json()).toEqual({ ok: true });
    expect(requests.map(({ authorization }) => authorization)).toEqual([
      "Bearer initiating-account-token",
      "Bearer initiating-account-token"
    ]);
    expect(window.localStorage.getItem("access_token")).toBe("different-account-token");
  });

  test("decrypts a retried SSE response with the fresh session key", async () => {
    let currentAttestation = staleAttestation;

    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "test-api-key" },
      dependencies({
        getAttestation: async (forceRefresh) => {
          if (forceRefresh) currentAttestation = freshAttestation;
          return currentAttestation;
        },
        fetch: async (_input, init) => {
          if (recordRequest(init).sessionId === staleAttestation.sessionId) {
            return new Response("stale", { status: 400 });
          }

          return new Response(
            'event: response.output_text.delta\ndata: 2:{"delta":"hello"}\n\ndata: [DONE]\n\n',
            { headers: { "content-type": "text/event-stream" } }
          );
        }
      })
    );

    const response = await customFetch("https://example.test/v1/responses", {
      method: "POST",
      body: '{"prompt":"hello"}'
    });

    const responseText = await response.text();
    expect(responseText).toContain('data: {"delta":"hello"}');
    expect(responseText).toContain("data: [DONE]");
    expect(responseText).not.toContain('data: 2:{"delta":"hello"}');
  });

  test("shares one attestation renewal across concurrent stale requests", async () => {
    let currentAttestation = staleAttestation;
    let forcedAttestations = 0;
    const requests: RecordedRequest[] = [];

    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "test-api-key" },
      dependencies({
        getAttestation: async (forceRefresh) => {
          if (forceRefresh) {
            forcedAttestations += 1;
            await new Promise((resolve) => setTimeout(resolve, 10));
            currentAttestation = freshAttestation;
          }
          return currentAttestation;
        },
        fetch: async (_input, init) => {
          const request = recordRequest(init);
          requests.push(request);
          if (request.sessionId === staleAttestation.sessionId) {
            return new Response("stale", { status: 400 });
          }
          return Response.json({ encrypted: '2:{"ok":true}' });
        }
      })
    );

    const results = await Promise.all(
      Array.from({ length: 8 }, (_, index) =>
        customFetch("https://example.test/v1/responses", {
          method: "POST",
          body: JSON.stringify({ prompt: `hello-${index}` })
        }).then((response) => response.json())
      )
    );

    expect(results).toEqual(Array.from({ length: 8 }, () => ({ ok: true })));
    expect(forcedAttestations).toBe(1);
    expect(requests.filter(({ sessionId }) => sessionId === "stale-session")).toHaveLength(8);
    expect(requests.filter(({ sessionId }) => sessionId === "fresh-session")).toHaveLength(8);
    expect(
      requests
        .filter(({ sessionId }) => sessionId === "fresh-session")
        .every(({ encryptedBody }) => encryptedBody?.startsWith("2:") === true)
    ).toBe(true);
  });

  test("does not replay non-400 application errors", async () => {
    let requests = 0;
    let forcedAttestations = 0;

    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "test-api-key" },
      dependencies({
        getAttestation: async (forceRefresh) => {
          if (forceRefresh) forcedAttestations += 1;
          return staleAttestation;
        },
        fetch: async () => {
          requests += 1;
          return new Response("invalid request", { status: 422 });
        }
      })
    );

    await expect(customFetch("https://example.test/v1/responses")).rejects.toThrow(
      "Request failed with status 422: invalid request"
    );
    expect(requests).toBe(1);
    expect(forcedAttestations).toBe(0);
  });

  test("forwards one endpoint-bound PCR policy through lookup and renewal", async () => {
    const apiUrl = "https://enclave.example.test/base";
    const pcrConfig: PcrConfig = {
      pcr0Values: ["2a".repeat(48)],
      remoteAttestation: false
    };
    const expectedPcrConfig: PcrConfig = {
      pcr0Values: ["2a".repeat(48)],
      pcr0DevValues: [],
      remoteAttestation: false,
      remoteAttestationUrls: {
        prod: "https://raw.githubusercontent.com/OpenSecretCloud/opensecret/master/pcrProdHistory.json",
        dev: "https://raw.githubusercontent.com/OpenSecretCloud/opensecret/master/pcrDevHistory.json"
      }
    };
    const calls: Array<[boolean | undefined, string | undefined, PcrConfig | undefined]> = [];
    let currentAttestation = staleAttestation;

    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "test-api-key", apiUrl, pcrConfig },
      dependencies({
        getAttestation: async (forceRefresh, explicitApiUrl, policy) => {
          calls.push([forceRefresh, explicitApiUrl, policy]);
          if (forceRefresh) currentAttestation = freshAttestation;
          return currentAttestation;
        },
        fetch: async (_input, init) => {
          if (recordRequest(init).sessionId === staleAttestation.sessionId) {
            pcrConfig.pcr0Values = ["4c".repeat(48)];
            pcrConfig.remoteAttestation = true;
            return new Response("stale", { status: 400 });
          }
          return Response.json({ encrypted: '2:{"ok":true}' });
        }
      })
    );

    const response = await customFetch("https://example.test/v1/responses");

    expect(await response.json()).toEqual({ ok: true });
    expect(calls).toHaveLength(3);
    expect(calls.map(([forceRefresh]) => forceRefresh)).toEqual([false, false, true]);
    expect(calls.every(([, endpoint]) => endpoint === apiUrl)).toBe(true);
    expect(calls.every(([, , policy]) => policy !== pcrConfig)).toBe(true);
    expect(calls.map(([, , policy]) => policy)).toEqual([
      expectedPcrConfig,
      expectedPcrConfig,
      expectedPcrConfig
    ]);
    expect(calls[0][2]).toBe(calls[1][2]);
    expect(calls[1][2]).toBe(calls[2][2]);
  });

  test("inherits the provider's global endpoint and PCR policy when options omit them", async () => {
    const originalApiUrl = getApiUrl();
    const originalPcrConfig = getApiPcrConfig();
    const apiUrl = "https://provider.example.test";
    const pcrConfig: PcrConfig = {
      pcr0Values: ["3b".repeat(48)],
      remoteAttestation: false
    };
    const calls: Array<[boolean | undefined, string | undefined, PcrConfig | undefined]> = [];

    try {
      setApiUrl(apiUrl, pcrConfig);
      const customFetch = createCustomFetchWithDependencies(
        { apiKey: "test-api-key" },
        dependencies({
          getAttestation: async (forceRefresh, explicitApiUrl, policy) => {
            calls.push([forceRefresh, explicitApiUrl, policy]);
            return freshAttestation;
          },
          fetch: async () => Response.json({ encrypted: '2:{"ok":true}' })
        })
      );

      expect(await (await customFetch("https://example.test/v1/responses")).json()).toEqual({
        ok: true
      });
      expect(calls).toHaveLength(1);
      expect(calls[0][0]).toBe(false);
      expect(calls[0][1]).toBe(apiUrl);
      expect(calls[0][2]).toEqual(expect.objectContaining(pcrConfig));
    } finally {
      setApiUrl(originalApiUrl, originalPcrConfig);
    }
  });
});
