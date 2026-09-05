import { beforeEach, describe, expect, test } from "bun:test";
import OpenAI from "openai";
import {
  createCustomFetchWithDependencies,
  findOpenSecretInferenceCapacityError,
  OPEN_SECRET_INFERENCE_SEND_LIMIT_HEADER,
  OpenSecretInferenceCapacityError,
  type CustomFetchDependencies
} from "../ai";
import { getApiPcrConfig, getApiUrl, setApiUrl } from "../api";
import type { Attestation } from "../getAttestation";
import type { PcrConfig } from "../pcr";
import { ERROR_CODE_HEADER, ERROR_CONTRACT_HEADER } from "../recovery";

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

function contractError(status: number, body: string, code?: string): Response {
  const headers = new Headers({ [ERROR_CONTRACT_HEADER]: "1" });
  if (code) headers.set(ERROR_CODE_HEADER, code);
  return new Response(body, { status, headers });
}

function capacityContractError(
  status: number,
  options?: {
    contract?: string | null;
    code?: string | null;
    replay?: string | null;
    retryAfter?: string | null;
  }
): Response {
  const headers = new Headers();
  if (options?.contract !== null) {
    headers.set("x-opensecret-error-contract", options?.contract ?? "1");
  }
  if (options?.code !== null) {
    headers.set("x-opensecret-error-code", options?.code ?? "inference_capacity");
  }
  if (options?.replay !== null) {
    headers.set("x-opensecret-client-replay", options?.replay ?? "safe");
  }
  if (options?.retryAfter !== undefined && options.retryAfter !== null) {
    headers.set("retry-after", options.retryAfter);
  }
  return new Response("private upstream capacity detail", { status, headers });
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

async function withRequestBodyUnavailable<T>(callback: () => Promise<T>): Promise<T> {
  const OriginalRequest = globalThis.Request;
  const plaintextBodies = new WeakMap<Request, string>();
  class RequestWithoutBody extends OriginalRequest {
    constructor(input: RequestInfo | URL, init?: RequestInit) {
      const plaintextBody =
        typeof init?.body === "string"
          ? init.body
          : input instanceof RequestWithoutBody
            ? plaintextBodies.get(input)
            : undefined;
      super(
        input,
        plaintextBody !== undefined && init?.body == null ? { ...init, body: plaintextBody } : init
      );
      if (plaintextBody !== undefined) plaintextBodies.set(this, plaintextBody);
    }

    get body() {
      return undefined;
    }

    async text(): Promise<string> {
      const storedPlaintextBody = plaintextBodies.get(this);
      const plaintextBody = await super.text();
      return storedPlaintextBody ?? plaintextBody;
    }
  }

  globalThis.Request = RequestWithoutBody as unknown as typeof Request;
  try {
    return await callback();
  } finally {
    globalThis.Request = OriginalRequest;
  }
}

describe("createCustomFetch inference-capacity contract", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
  });

  for (const status of [429, 503] as const) {
    test(`classifies exact ${status} without consuming or exposing its body`, async () => {
      let bodyRead = false;
      let bodyCancelled = false;
      const capacityResponse = capacityContractError(status, { retryAfter: "7" });
      const responseBody = capacityResponse.body;
      if (!responseBody) throw new Error("capacity test response must have a body");
      const cancelBody = responseBody.cancel.bind(responseBody);
      responseBody.cancel = async (reason?: unknown) => {
        bodyCancelled = true;
        return cancelBody(reason);
      };
      capacityResponse.text = async () => {
        bodyRead = true;
        return "private upstream capacity detail";
      };
      const customFetch = createCustomFetchWithDependencies(
        { apiKey: "test-api-key" },
        dependencies({
          fetch: async () => capacityResponse
        })
      );

      let error: unknown;
      try {
        await customFetch("https://example.test/v1/responses", {
          method: "POST",
          body: '{"prompt":"hello"}'
        });
      } catch (caught) {
        error = caught;
      }

      expect(error).toBeInstanceOf(OpenSecretInferenceCapacityError);
      expect(error).toMatchObject({
        name: "OpenSecretInferenceCapacityError",
        message: "Inference capacity is temporarily unavailable.",
        status,
        retryDelayMs: 7_000,
        inferenceSendCount: 1
      });
      expect(String(error)).not.toContain("private upstream");
      expect(bodyRead).toBe(false);
      expect(bodyCancelled).toBe(true);
    });
  }

  test("uses strict bounded delta-seconds retry hints", async () => {
    const cases: Array<[string | undefined, number | null]> = [
      [undefined, 1_000],
      ["0", 0],
      ["7", 7_000],
      ["60", 60_000],
      ["61", null],
      ["01", 1_000],
      ["-1", 1_000],
      ["1.5", 1_000],
      ["1e2", 1_000],
      ["Wed, 21 Oct 2015 07:28:00 GMT", 1_000],
      ["7, 9", 1_000],
      ["999999999999999999999999999999999999999999", null]
    ];

    for (const [retryAfter, expectedDelay] of cases) {
      const customFetch = createCustomFetchWithDependencies(
        { apiKey: "test-api-key" },
        dependencies({
          fetch: async () => capacityContractError(503, { retryAfter })
        })
      );

      let error: unknown;
      try {
        await customFetch("https://example.test/v1/responses");
      } catch (caught) {
        error = caught;
      }
      expect(error).toBeInstanceOf(OpenSecretInferenceCapacityError);
      expect((error as OpenSecretInferenceCapacityError).retryDelayMs).toBe(expectedDelay);
    }
  });

  test("rejects missing, future, duplicated, and status-mismatched required headers", async () => {
    const invalid = [
      capacityContractError(429, { contract: null }),
      capacityContractError(429, { contract: "2" }),
      capacityContractError(429, { contract: "1, 1" }),
      capacityContractError(429, { code: null }),
      capacityContractError(429, { code: "inference_capacity_v2" }),
      capacityContractError(429, { code: "inference_capacity, inference_capacity" }),
      capacityContractError(429, { replay: null }),
      capacityContractError(429, { replay: "true" }),
      capacityContractError(429, { replay: "safe, safe" }),
      capacityContractError(529),
      capacityContractError(500)
    ];

    const duplicateContract = capacityContractError(503);
    duplicateContract.headers.append("x-opensecret-error-contract", "1");
    invalid.push(duplicateContract);
    const duplicateCode = capacityContractError(503);
    duplicateCode.headers.append("x-opensecret-error-code", "inference_capacity");
    invalid.push(duplicateCode);
    const duplicateReplay = capacityContractError(503);
    duplicateReplay.headers.append("x-opensecret-client-replay", "safe");
    invalid.push(duplicateReplay);

    for (const response of invalid) {
      const customFetch = createCustomFetchWithDependencies(
        { apiKey: "test-api-key" },
        dependencies({ fetch: async () => response.clone() })
      );

      let error: unknown;
      try {
        await customFetch("https://example.test/v1/responses");
      } catch (caught) {
        error = caught;
      }
      expect(findOpenSecretInferenceCapacityError(error)).toBeNull();
      expect(String(error)).toContain(`Request failed with status ${response.status}`);
    }
  });

  test("finds only the SDK-owned typed error through a bounded cause chain", () => {
    const capacity = new OpenSecretInferenceCapacityError(503, 1_000);
    const wrapped = new Error("outer", { cause: new Error("middle", { cause: capacity }) });
    expect(findOpenSecretInferenceCapacityError(wrapped)).toBe(capacity);
    expect(
      findOpenSecretInferenceCapacityError({
        name: "OpenSecretInferenceCapacityError",
        status: 503,
        retryDelayMs: 1_000
      })
    ).toBeNull();

    const cycle: { cause?: unknown } = {};
    cycle.cause = cycle;
    expect(findOpenSecretInferenceCapacityError(cycle)).toBeNull();
  });

  test("survives the real OpenAI wrapper with its transport retries disabled", async () => {
    let sends = 0;
    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "test-api-key" },
      dependencies({
        fetch: async () => {
          sends += 1;
          return capacityContractError(503, { retryAfter: "0" });
        }
      })
    );
    const openai = new OpenAI({
      apiKey: "not-a-real-api-key",
      baseURL: "https://example.test/v1/",
      dangerouslyAllowBrowser: true,
      fetch: customFetch,
      maxRetries: 0
    });

    let error: unknown;
    try {
      await openai.responses.create({ model: "kimi-k3", input: "hello" });
    } catch (caught) {
      error = caught;
    }

    expect(sends).toBe(1);
    expect(error).toMatchObject({ name: "Error" });
    const capacity = findOpenSecretInferenceCapacityError(error);
    expect(capacity).toBeInstanceOf(OpenSecretInferenceCapacityError);
    expect(capacity).toMatchObject({ status: 503, retryDelayMs: 0 });
    expect((error as { cause?: unknown }).cause).toBe(capacity);
  });

  test("shares a two-send ceiling across stale-session repair and capacity", async () => {
    let currentAttestation = staleAttestation;
    let forcedAttestations = 0;
    let sends = 0;
    const forwardedLimits: Array<string | null> = [];
    const redirects: Array<RequestRedirect | undefined> = [];
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
        fetch: async (_input, init) => {
          sends += 1;
          forwardedLimits.push(
            new Headers(init?.headers).get(OPEN_SECRET_INFERENCE_SEND_LIMIT_HEADER)
          );
          redirects.push(init?.redirect);
          if (recordRequest(init).sessionId === staleAttestation.sessionId) {
            return contractError(400, "stale session", "session_not_found");
          }
          return capacityContractError(503, { retryAfter: "0" });
        }
      })
    );

    let error: unknown;
    try {
      await customFetch("https://example.test/v1/responses", {
        headers: { [OPEN_SECRET_INFERENCE_SEND_LIMIT_HEADER]: "2" }
      });
    } catch (caught) {
      error = caught;
    }

    expect(findOpenSecretInferenceCapacityError(error)).toMatchObject({
      status: 503,
      retryDelayMs: 0,
      inferenceSendCount: 2
    });
    expect(sends).toBe(2);
    expect(forcedAttestations).toBe(1);
    expect(forwardedLimits).toEqual([null, null]);
    expect(redirects).toEqual(["manual", "manual"]);
  });

  test("repairs a stale session but does not exceed a one-send ceiling", async () => {
    let currentAttestation = staleAttestation;
    let forcedAttestations = 0;
    let sends = 0;
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
          sends += 1;
          if (sends === 1) return contractError(400, "stale session", "session_not_found");
          return Response.json({ encrypted: '2:{"unexpected":true}' });
        }
      })
    );

    await expect(
      customFetch("https://example.test/v1/responses", {
        headers: { [OPEN_SECRET_INFERENCE_SEND_LIMIT_HEADER]: "1" }
      })
    ).rejects.toMatchObject({ status: 400 });

    expect(sends).toBe(1);
    expect(forcedAttestations).toBe(1);
    expect(currentAttestation).toBe(freshAttestation);
  });
});

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
            return contractError(
              400,
              '{"status":400,"message":"Bad Request"}',
              "session_not_found"
            );
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

  for (const method of ["GET", "HEAD", "POST"] as const) {
    test(`keeps Firefox ${method} requests bodyless across stale-session recovery`, async () => {
      await withRequestBodyUnavailable(async () => {
        let currentAttestation = staleAttestation;
        let forcedAttestations = 0;
        let encryptions = 0;
        const requests: Array<{
          body: BodyInit | null | undefined;
          method: string | undefined;
          safeHeader: string | null;
          sessionId: string | null;
          url: string;
        }> = [];

        const customFetch = createCustomFetchWithDependencies(
          { apiKey: "test-api-key" },
          dependencies({
            encryptMessage: (sessionKey, plaintext) => {
              encryptions += 1;
              return encryptForTest(sessionKey, plaintext);
            },
            getAttestation: async (forceRefresh) => {
              if (forceRefresh) {
                forcedAttestations += 1;
                currentAttestation = freshAttestation;
              }
              return currentAttestation;
            },
            fetch: async (input, init) => {
              const headers = new Headers(init?.headers);
              requests.push({
                body: init?.body,
                method: init?.method,
                safeHeader: headers.get("x-safe-provider-header"),
                sessionId: headers.get("x-session-id"),
                url: String(input)
              });

              return requests.length === 1
                ? contractError(400, "stale", "session_not_found")
                : Response.json({ encrypted: '2:{"ok":true}' });
            }
          })
        );
        const sourceRequest = new Request(
          "https://example.test/v1/conversations/conversation-1?limit=20",
          {
            method,
            headers: { "x-safe-provider-header": "preserve-me" }
          }
        );

        expect(await (await customFetch(sourceRequest)).json()).toEqual({ ok: true });
        expect(forcedAttestations).toBe(1);
        expect(encryptions).toBe(0);
        expect(requests).toEqual([
          {
            body: undefined,
            method,
            safeHeader: "preserve-me",
            sessionId: "stale-session",
            url: "https://example.test/v1/conversations/conversation-1?limit=20"
          },
          {
            body: undefined,
            method,
            safeHeader: "preserve-me",
            sessionId: "fresh-session",
            url: "https://example.test/v1/conversations/conversation-1?limit=20"
          }
        ]);
      });
    });
  }

  test("keeps Firefox POST plaintext encryption and replay intact", async () => {
    await withRequestBodyUnavailable(async () => {
      let currentAttestation = staleAttestation;
      let forcedAttestations = 0;
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
          fetch: async (_input, init) => {
            const request = recordRequest(init);
            requests.push(request);
            return requests.length === 1
              ? contractError(400, "stale", "session_not_found")
              : Response.json({ encrypted: '2:{"ok":true}' });
          }
        })
      );
      const sourceRequest = new Request("https://example.test/v1/responses", {
        method: "POST",
        body: '{"prompt":"preserve this"}'
      });

      expect(await (await customFetch(sourceRequest)).json()).toEqual({ ok: true });
      expect(forcedAttestations).toBe(1);
      expect(requests).toEqual([
        {
          authorization: "Bearer test-api-key",
          encryptedBody: '1:{"prompt":"preserve this"}',
          sessionId: "stale-session"
        },
        {
          authorization: "Bearer test-api-key",
          encryptedBody: '2:{"prompt":"preserve this"}',
          sessionId: "fresh-session"
        }
      ]);
    });
  });

  for (const source of ["RequestInit", "Request"] as const) {
    test(`preserves an explicitly empty Firefox POST body from ${source}`, async () => {
      await withRequestBodyUnavailable(async () => {
        let currentAttestation = staleAttestation;
        const requests: RecordedRequest[] = [];

        const customFetch = createCustomFetchWithDependencies(
          { apiKey: "test-api-key" },
          dependencies({
            getAttestation: async (forceRefresh) => {
              if (forceRefresh) currentAttestation = freshAttestation;
              return currentAttestation;
            },
            fetch: async (_input, init) => {
              const request = recordRequest(init);
              requests.push(request);
              return requests.length === 1
                ? contractError(400, "stale", "session_not_found")
                : Response.json({ encrypted: '2:{"ok":true}' });
            }
          })
        );
        const url = "https://example.test/v1/responses";

        const response =
          source === "RequestInit"
            ? await customFetch(url, { method: "POST", body: "" })
            : await customFetch(new Request(url, { method: "POST", body: "" }));

        expect(await response.json()).toEqual({ ok: true });
        expect(requests).toEqual([
          {
            authorization: "Bearer test-api-key",
            encryptedBody: "1:",
            sessionId: "stale-session"
          },
          {
            authorization: "Bearer test-api-key",
            encryptedBody: "2:",
            sessionId: "fresh-session"
          }
        ]);
      });
    });
  }

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
          return contractError(400, "still bad", "session_not_found");
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

  test("keeps legacy headerless 400 session recovery", async () => {
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
          return requests === 1
            ? new Response("legacy stale session", { status: 400 })
            : Response.json({ encrypted: '2:{"ok":true}' });
        }
      })
    );

    expect(
      await (
        await customFetch("https://example.test/v1/responses", {
          method: "POST",
          body: '{"prompt":"hello"}'
        })
      ).json()
    ).toEqual({ ok: true });
    expect(requests).toBe(2);
    expect(forcedAttestations).toBe(1);
  });

  test("keeps legacy headerless 401 JWT recovery", async () => {
    window.localStorage.setItem("access_token", "expired-access-token");
    let requests = 0;
    let tokenRefreshes = 0;
    const customFetch = createCustomFetchWithDependencies(
      undefined,
      dependencies({
        refreshToken: async () => {
          tokenRefreshes += 1;
          window.localStorage.setItem("access_token", "fresh-access-token");
          return {
            access_token: "fresh-access-token",
            refresh_token: "fresh-refresh-token"
          };
        },
        fetch: async () => {
          requests += 1;
          return requests === 1
            ? new Response("legacy expired JWT", { status: 401 })
            : Response.json({ encrypted: '1:{"ok":true}' });
        }
      })
    );

    expect(await (await customFetch("https://example.test/v1/responses")).json()).toEqual({
      ok: true
    });
    expect(requests).toBe(2);
    expect(tokenRefreshes).toBe(1);
  });

  for (const ordinaryError of [
    { status: 400, code: undefined, name: "ordinary v1 400" },
    { status: 401, code: "invalid_jwt", name: "ordinary v1 401" }
  ]) {
    test(`${ordinaryError.name} fails closed without replay`, async () => {
      window.localStorage.setItem("access_token", "access-token");
      let requests = 0;
      let forcedAttestations = 0;
      let tokenRefreshes = 0;
      const customFetch = createCustomFetchWithDependencies(
        undefined,
        dependencies({
          getAttestation: async (forceRefresh) => {
            if (forceRefresh) forcedAttestations += 1;
            return staleAttestation;
          },
          refreshToken: async () => {
            tokenRefreshes += 1;
            return {
              access_token: "unused-access-token",
              refresh_token: "unused-refresh-token"
            };
          },
          fetch: async () => {
            requests += 1;
            return contractError(
              ordinaryError.status,
              `ordinary-${ordinaryError.status}`,
              ordinaryError.code
            );
          }
        })
      );

      await expect(customFetch("https://example.test/v1/responses")).rejects.toThrow(
        `Request failed with status ${ordinaryError.status}: ordinary-${ordinaryError.status}`
      );
      expect(requests).toBe(1);
      expect(forcedAttestations).toBe(0);
      expect(tokenRefreshes).toBe(0);
    });
  }

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
          if (requests.length === 1) {
            return contractError(401, "expired JWT", "access_token_expired");
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

  test("uses one target replay budget when recoverable reasons alternate", async () => {
    window.localStorage.setItem("access_token", "expired-access-token");
    let requests = 0;
    let tokenRefreshes = 0;
    let forcedAttestations = 0;
    const customFetch = createCustomFetchWithDependencies(
      undefined,
      dependencies({
        getAttestation: async (forceRefresh) => {
          if (forceRefresh) forcedAttestations += 1;
          return staleAttestation;
        },
        refreshToken: async () => {
          tokenRefreshes += 1;
          window.localStorage.setItem("access_token", "fresh-access-token");
          return {
            access_token: "fresh-access-token",
            refresh_token: "fresh-refresh-token"
          };
        },
        fetch: async () => {
          requests += 1;
          return requests === 1
            ? contractError(401, "expired", "access_token_expired")
            : contractError(400, "stale", "session_not_found");
        }
      })
    );

    await expect(customFetch("https://example.test/v1/responses")).rejects.toThrow(
      "Request failed with status 400: stale"
    );
    expect(requests).toBe(2);
    expect(tokenRefreshes).toBe(1);
    expect(forcedAttestations).toBe(0);
  });

  test("never refreshes a JWT for an API-key 401", async () => {
    let requests = 0;
    let tokenRefreshes = 0;
    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "invalid-api-key" },
      dependencies({
        refreshToken: async () => {
          tokenRefreshes += 1;
          return {
            access_token: "unused-access-token",
            refresh_token: "unused-refresh-token"
          };
        },
        fetch: async () => {
          requests += 1;
          return contractError(401, "expired", "access_token_expired");
        }
      })
    );

    await expect(customFetch("https://example.test/v1/responses")).rejects.toThrow(
      "Request failed with status 401: expired"
    );
    expect(requests).toBe(1);
    expect(tokenRefreshes).toBe(0);
  });

  test("keeps API-key authentication pinned when options mutate in flight", async () => {
    const options = { apiKey: "original-api-key" };
    let requests = 0;
    let tokenRefreshes = 0;
    const customFetch = createCustomFetchWithDependencies(
      options,
      dependencies({
        refreshToken: async () => {
          tokenRefreshes += 1;
          return {
            access_token: "unused-access-token",
            refresh_token: "unused-refresh-token"
          };
        },
        fetch: async (_input, init) => {
          requests += 1;
          expect(new Headers(init?.headers).get("Authorization")).toBe("Bearer original-api-key");
          options.apiKey = "";
          return contractError(401, "expired", "access_token_expired");
        }
      })
    );

    await expect(customFetch("https://example.test/v1/responses")).rejects.toThrow(
      "Request failed with status 401: expired"
    );
    expect(requests).toBe(1);
    expect(tokenRefreshes).toBe(0);
  });

  test("keeps JWT authentication pinned when options mutate in flight", async () => {
    const options: { apiKey?: string } = {};
    window.localStorage.setItem("access_token", "original-access-token");
    let requests = 0;
    let tokenRefreshes = 0;
    const customFetch = createCustomFetchWithDependencies(
      options,
      dependencies({
        refreshToken: async () => {
          tokenRefreshes += 1;
          window.localStorage.setItem("access_token", "fresh-access-token");
          return {
            access_token: "fresh-access-token",
            refresh_token: "fresh-refresh-token"
          };
        },
        fetch: async (_input, init) => {
          requests += 1;
          const authorization = new Headers(init?.headers).get("Authorization");
          if (requests === 1) {
            expect(authorization).toBe("Bearer original-access-token");
            options.apiKey = "late-api-key";
            return contractError(401, "expired", "access_token_expired");
          }

          expect(authorization).toBe("Bearer fresh-access-token");
          return Response.json({ encrypted: '1:{"ok":true}' });
        }
      })
    );

    const response = await customFetch("https://example.test/v1/responses");
    expect(await response.json()).toEqual({ ok: true });
    expect(requests).toBe(2);
    expect(tokenRefreshes).toBe(1);
  });

  test("snapshots and preserves the complete logical request across recovery", async () => {
    let currentAttestation = staleAttestation;
    const controller = new AbortController();
    const url = new URL("https://example.test/v1/responses?model=private&stream=true");
    const sourceHeaders = new Headers({
      "content-type": "application/json",
      "x-safe-provider-header": "preserve-me"
    });
    const sourceInit: RequestInit = {
      method: "POST",
      headers: sourceHeaders,
      body: '{"prompt":"original"}',
      cache: "no-store",
      credentials: "include",
      redirect: "manual",
      referrerPolicy: "no-referrer",
      signal: controller.signal
    };
    const requests: Array<{
      url: string;
      method: string | undefined;
      safeHeader: string | null;
      plaintext: string;
      cache: RequestCache | undefined;
      credentials: RequestCredentials | undefined;
      redirect: RequestRedirect | undefined;
      referrerPolicy: ReferrerPolicy | undefined;
      sameSignal: boolean;
    }> = [];

    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "test-api-key" },
      dependencies({
        getAttestation: async (forceRefresh) => {
          if (forceRefresh) currentAttestation = freshAttestation;
          return currentAttestation;
        },
        fetch: async (input, init) => {
          const recorded = recordRequest(init);
          const key = recorded.sessionId === "stale-session" ? staleKey : freshKey;
          requests.push({
            url: String(input),
            method: init?.method,
            safeHeader: new Headers(init?.headers).get("x-safe-provider-header"),
            plaintext: decryptForTest(key, recorded.encryptedBody!),
            cache: init?.cache,
            credentials: init?.credentials,
            redirect: init?.redirect,
            referrerPolicy: init?.referrerPolicy,
            sameSignal: init?.signal === controller.signal
          });

          if (requests.length === 1) {
            url.searchParams.set("model", "mutated");
            sourceHeaders.set("x-safe-provider-header", "mutated");
            sourceInit.method = "PUT";
            sourceInit.body = '{"prompt":"mutated"}';
            sourceInit.credentials = "omit";
            return contractError(400, "stale", "session_not_found");
          }
          return Response.json({ encrypted: '2:{"ok":true}' });
        }
      })
    );

    const responsePromise = customFetch(url, sourceInit);
    // snapshotRequest reads the body asynchronously; mutations during that
    // yield must not leak into either transport attempt.
    sourceInit.cache = "reload";
    sourceInit.referrerPolicy = "origin";

    expect(await (await responsePromise).json()).toEqual({ ok: true });
    expect(requests).toEqual([
      {
        url: "https://example.test/v1/responses?model=private&stream=true",
        method: "POST",
        safeHeader: "preserve-me",
        plaintext: '{"prompt":"original"}',
        cache: "no-store",
        credentials: "include",
        redirect: "manual",
        referrerPolicy: "no-referrer",
        sameSignal: true
      },
      {
        url: "https://example.test/v1/responses?model=private&stream=true",
        method: "POST",
        safeHeader: "preserve-me",
        plaintext: '{"prompt":"original"}',
        cache: "no-store",
        credentials: "include",
        redirect: "manual",
        referrerPolicy: "no-referrer",
        sameSignal: true
      }
    ]);
  });

  test("an abort after the first response prevents recovery and replay", async () => {
    const controller = new AbortController();
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
          controller.abort();
          return contractError(400, "stale", "session_not_found");
        }
      })
    );

    await expect(
      customFetch("https://example.test/v1/responses", { signal: controller.signal })
    ).rejects.toMatchObject({ name: "AbortError" });
    expect(requests).toBe(1);
    expect(forcedAttestations).toBe(0);
  });

  test("a pre-aborted request performs no attestation or transport work", async () => {
    const controller = new AbortController();
    controller.abort();
    let attestations = 0;
    let requests = 0;
    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "test-api-key" },
      dependencies({
        getAttestation: async () => {
          attestations += 1;
          return staleAttestation;
        },
        fetch: async () => {
          requests += 1;
          return Response.json({});
        }
      })
    );

    await expect(
      customFetch("https://example.test/v1/responses", { signal: controller.signal })
    ).rejects.toMatchObject({ name: "AbortError" });
    expect(attestations).toBe(0);
    expect(requests).toBe(0);
  });

  test("an abort during initial attestation prevents the first transport send", async () => {
    const controller = new AbortController();
    let attestations = 0;
    let requests = 0;
    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "test-api-key" },
      dependencies({
        getAttestation: async () => {
          attestations += 1;
          controller.abort();
          return staleAttestation;
        },
        fetch: async () => {
          requests += 1;
          return Response.json({});
        }
      })
    );

    await expect(
      customFetch("https://example.test/v1/responses", { signal: controller.signal })
    ).rejects.toMatchObject({ name: "AbortError" });
    expect(attestations).toBe(1);
    expect(requests).toBe(0);
  });

  test("an explicit null signal detaches from a Request source across recovery", async () => {
    const sourceController = new AbortController();
    const sourceRequest = new Request("https://example.test/v1/responses", {
      method: "POST",
      body: '{"prompt":"detached"}',
      signal: sourceController.signal
    });
    let currentAttestation = staleAttestation;
    let forcedAttestations = 0;
    const signals: Array<AbortSignal | null | undefined> = [];
    let requests = 0;
    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "test-api-key" },
      dependencies({
        getAttestation: async (forceRefresh) => {
          if (forceRefresh) {
            forcedAttestations += 1;
            sourceController.abort();
            currentAttestation = freshAttestation;
          }
          return currentAttestation;
        },
        fetch: async (_input, init) => {
          requests += 1;
          signals.push(init?.signal);
          return requests === 1
            ? contractError(400, "stale", "session_not_found")
            : Response.json({ encrypted: '2:{"ok":true}' });
        }
      })
    );

    const response = await customFetch(sourceRequest, { signal: null });
    expect(await response.json()).toEqual({ ok: true });
    expect(sourceController.signal.aborted).toBe(true);
    expect(forcedAttestations).toBe(1);
    expect(requests).toBe(2);
    expect(signals).toEqual([null, null]);
  });

  test("an explicit undefined signal inherits a Request source signal", async () => {
    const sourceController = new AbortController();
    const sourceRequest = new Request("https://example.test/v1/responses", {
      signal: sourceController.signal
    });
    let attestations = 0;
    let requests = 0;
    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "test-api-key" },
      dependencies({
        getAttestation: async () => {
          attestations += 1;
          sourceController.abort();
          return staleAttestation;
        },
        fetch: async () => {
          requests += 1;
          return Response.json({});
        }
      })
    );

    await expect(customFetch(sourceRequest, { signal: undefined })).rejects.toMatchObject({
      name: "AbortError"
    });
    expect(attestations).toBe(1);
    expect(requests).toBe(0);
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
            return contractError(400, "stale", "session_not_found");
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
            return contractError(400, "stale", "session_not_found");
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
            return contractError(400, "stale", "session_not_found");
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

  test("a staggered stale response joins renewal after the leader evicts the cache", async () => {
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
    const requests: RecordedRequest[] = [];

    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "test-api-key" },
      dependencies({
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

          // This is the cache-miss handshake the old compare-before-map
          // ordering allowed the staggered caller to start.
          fullHandshakes += 1;
          currentAttestation = lateAttestation;
          return lateAttestation;
        },
        fetch: async (_input, init) => {
          const request = recordRequest(init);
          requests.push(request);
          if (request.sessionId === staleAttestation.sessionId) {
            staleSends += 1;
            if (staleSends === 2) {
              await cacheCleared;
              releaseLateResponse();
            }
            return contractError(400, "stale", "session_not_found");
          }

          const key = request.sessionId === freshAttestation.sessionId ? freshKey : lateKey;
          return Response.json({ encrypted: encryptForTest(key, JSON.stringify({ ok: true })) });
        }
      })
    );

    const results = await Promise.all(
      ["first", "late"].map((prompt) =>
        customFetch("https://example.test/v1/responses", {
          method: "POST",
          body: JSON.stringify({ prompt })
        }).then((response) => response.json())
      )
    );

    expect(results).toEqual([{ ok: true }, { ok: true }]);
    expect(forcedAttestations).toBe(1);
    expect(fullHandshakes).toBe(1);
    expect(requests.filter(({ sessionId }) => sessionId === "stale-session")).toHaveLength(2);
    expect(requests.filter(({ sessionId }) => sessionId === "fresh-session")).toHaveLength(2);
    expect(requests.some(({ sessionId }) => sessionId === "late-extra-session")).toBe(false);
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
          return contractError(422, "invalid request");
        }
      })
    );

    await expect(customFetch("https://example.test/v1/responses")).rejects.toThrow(
      "Request failed with status 422: invalid request"
    );
    expect(requests).toBe(1);
    expect(forcedAttestations).toBe(0);
  });

  test("preserves coded application error metadata without replay", async () => {
    let requests = 0;
    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "test-api-key" },
      dependencies({
        fetch: async () => {
          requests += 1;
          return contractError(
            503,
            '{"status":503,"message":"Upstream provider temporarily unavailable"}',
            "image_description_unavailable"
          );
        }
      })
    );

    let thrown: unknown;
    try {
      await customFetch("https://example.test/v1/responses");
    } catch (error) {
      thrown = error;
    }

    expect(requests).toBe(1);
    expect(thrown).toBeInstanceOf(Error);
    const requestError = thrown as Error & { status?: number; headers?: Headers };
    expect(requestError.message).toBe(
      'Request failed with status 503: {"status":503,"message":"Upstream provider temporarily unavailable"}'
    );
    expect(requestError.status).toBe(503);
    expect(requestError.headers?.get(ERROR_CONTRACT_HEADER)).toBe("1");
    expect(requestError.headers?.get(ERROR_CODE_HEADER)).toBe("image_description_unavailable");
  });

  test("forwards one endpoint-bound PCR policy through lookup and renewal", async () => {
    const apiUrl = "https://enclave.example.test/base";
    const pcrConfig: PcrConfig = {
      environment: "development",
      pcr0DevValues: ["2a".repeat(48)],
      remoteAttestation: false
    };
    const expectedPcrConfig: PcrConfig = {
      environment: "development",
      pcr0Values: [],
      pcr0DevValues: ["2a".repeat(48)],
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
            pcrConfig.environment = "production";
            pcrConfig.pcr0DevValues = ["4c".repeat(48)];
            pcrConfig.remoteAttestation = true;
            return contractError(400, "stale", "session_not_found");
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
      expect(calls[0][2]?.environment).toBe("production");
    } finally {
      setApiUrl(originalApiUrl, originalPcrConfig);
    }
  });
});
