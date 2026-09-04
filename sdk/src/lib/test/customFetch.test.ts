import { describe, expect, mock, test } from "bun:test";
import { createCustomFetchWithDependencies, type CustomFetchDependencies } from "../ai";
import type { PcrConfig } from "../pcr";
import type { StoredTransportV2Credentials } from "../transportV2/auth";
import type { TransportV2RuntimeRequest, TransportV2RuntimeResponse } from "../transportV2/runtime";

const apiUrl = "https://api.example.test/base";
const cacheRoot = new Uint8Array(32).fill(0x42);

function deferred<T>() {
  let resolve!: (value: T) => void;
  return {
    promise: new Promise<T>((fulfill) => {
      resolve = fulfill;
    }),
    resolve
  };
}

function userCredentials(): StoredTransportV2Credentials {
  return {
    kind: "user",
    principalId: "00112233-4455-6677-8899-aabbccddeeff",
    apiOrigin: "https://api.example.test",
    revision: 1,
    accessToken: "user-access-token",
    refreshToken: "user-refresh-token",
    accessExpiresAtUnixSeconds: 4_000_000_000,
    refreshExpiresAtUnixSeconds: 4_000_000_000
  };
}

function result(response: Response): TransportV2RuntimeResponse {
  return { response, rememberOAuthContinuation: () => {} };
}

function dependencies(
  implementation: (input: TransportV2RuntimeRequest) => Promise<TransportV2RuntimeResponse>,
  credentials: StoredTransportV2Credentials | null = null
): CustomFetchDependencies {
  return {
    auth: {
      authority: mock(async () => {
        if (!credentials) throw new Error("No access token available");
        return {
          credential: { kind: "bearer", value: credentials.accessToken },
          credentials,
          snapshot: {
            apiOrigin: credentials.apiOrigin,
            kind: "user",
            principalId: credentials.principalId,
            revision: credentials.revision
          },
          assertCurrent() {}
        };
      }),
      noteResponse: mock(() => {})
    },
    runtime: { request: mock(implementation) },
    getApiPcrConfig: () => ({ environment: "development" }),
    getApiUrl: () => apiUrl,
    getCacheRoot: () => new Uint8Array(cacheRoot),
    readUserCredentials: () => credentials
  };
}

function copyRequest(input: TransportV2RuntimeRequest): TransportV2RuntimeRequest {
  return {
    ...input,
    request: {
      ...input.request,
      headers: input.request.headers?.map((header) => ({ ...header })),
      body: input.request.body ? new Uint8Array(input.request.body) : input.request.body,
      cacheNamespaceRoot: input.request.cacheNamespaceRoot
        ? new Uint8Array(input.request.cacheNamespaceRoot)
        : undefined
    }
  };
}

describe("Transport V2 custom Fetch adapter", () => {
  test("encrypts the exact binary body, target, safe headers, API key, and cache root", async () => {
    const seen: TransportV2RuntimeRequest[] = [];
    const deps = dependencies(async (input) => {
      seen.push(copyRequest(input));
      return result(new Response("ok", { headers: { "x-authenticated": "yes" } }));
    });
    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "real-api-key", apiUrl, pcrConfig: { environment: "development" } },
      deps
    );
    const plaintext = new Uint8Array([0, 1, 2, 0xff]);

    const response = await customFetch(`${apiUrl}/v1/audio/transcriptions?b=2&a=1`, {
      method: "POST",
      body: plaintext,
      headers: {
        authorization: "Bearer caller-placeholder",
        "content-type": "application/octet-stream",
        "content-length": "4",
        "x-stainless-lang": "js",
        "x-stainless-retry-count": "0",
        "x-opensecret-routing-key": "caller-route",
        "x-session-id": "caller-session",
        "x-openai-api-key": "caller-key",
        accept: "application/json",
        "x-safe-metadata": "kept"
      }
    });

    expect(await response.text()).toBe("ok");
    expect(response.headers.get("x-authenticated")).toBe("yes");
    expect(seen).toHaveLength(1);
    expect(seen[0].apiUrl).toBe(apiUrl);
    expect(seen[0].pcrConfig).toMatchObject({ environment: "development" });
    expect(seen[0].request).toMatchObject({
      credential: { kind: "api_key", value: "real-api-key" },
      method: "POST",
      target: "/v1/audio/transcriptions?b=2&a=1"
    });
    expect(seen[0].request.body).toEqual(plaintext);
    expect(seen[0].request.cacheNamespaceRoot).toEqual(cacheRoot);
    expect(seen[0].request.headers).toEqual([
      { name: "accept", value: "application/json" },
      { name: "content-type", value: "application/octet-stream" },
      { name: "x-safe-metadata", value: "kept" }
    ]);
  });

  test("preserves an absent body versus an explicitly empty body", async () => {
    const seen: TransportV2RuntimeRequest[] = [];
    const deps = dependencies(async (input) => {
      seen.push(copyRequest(input));
      return result(Response.json({ ok: true }));
    });
    const customFetch = createCustomFetchWithDependencies({ apiKey: "key", apiUrl }, deps);

    await customFetch(`${apiUrl}/v1/models`, { method: "GET" });
    await customFetch(`${apiUrl}/v1/audio/transcriptions`, { method: "POST" });
    await customFetch(`${apiUrl}/v1/audio/transcriptions`, {
      method: "POST",
      body: undefined
    });
    await customFetch(`${apiUrl}/v1/audio/transcriptions`, { method: "POST", body: "" });

    expect(seen.map(({ request }) => request.body)).toEqual([
      undefined,
      undefined,
      undefined,
      new Uint8Array(0)
    ]);
  });

  test("puts the signed-in user bearer and stable cache root inside the envelope", async () => {
    let seen: TransportV2RuntimeRequest | undefined;
    const deps = dependencies(async (input) => {
      seen = copyRequest(input);
      return result(Response.json({ ok: true }));
    }, userCredentials());
    const customFetch = createCustomFetchWithDependencies({ apiUrl }, deps);

    await customFetch(`${apiUrl}/v1/responses`, { method: "POST", body: "{}" });

    expect(seen?.request.credential).toEqual({ kind: "bearer", value: "user-access-token" });
    expect(seen?.request.cacheNamespaceRoot).toEqual(cacheRoot);
    expect(deps.auth.authority).toHaveBeenCalledTimes(1);
    expect(deps.auth.noteResponse).toHaveBeenCalledTimes(1);
  });

  test("does not dispatch after the selected user authority changes in flight", async () => {
    const stored = userCredentials();
    let outerSends = 0;
    const deps = dependencies(async (input) => {
      input.beforeSend?.();
      outerSends += 1;
      return result(Response.json({ ok: true }));
    }, stored);
    deps.auth.authority = mock(async () => ({
      credential: { kind: "bearer", value: stored.accessToken },
      credentials: stored,
      snapshot: {
        apiOrigin: stored.apiOrigin,
        kind: "user",
        principalId: stored.principalId,
        revision: stored.revision
      },
      assertCurrent() {
        throw new Error("Transport v2 authentication state changed before send.");
      }
    }));
    const customFetch = createCustomFetchWithDependencies({ apiUrl }, deps);

    await expect(
      customFetch(`${apiUrl}/v1/responses`, { method: "POST", body: "{}" })
    ).rejects.toThrow("authentication state changed");
    expect(deps.runtime.request).toHaveBeenCalledTimes(1);
    expect(outerSends).toBe(0);
  });

  test("allows only the models endpoint to be anonymous", async () => {
    const seen: TransportV2RuntimeRequest[] = [];
    const deps = dependencies(async (input) => {
      seen.push(copyRequest(input));
      return result(Response.json({ object: "list", data: [] }));
    });
    const customFetch = createCustomFetchWithDependencies({ apiUrl }, deps);

    await customFetch(`${apiUrl}/v1/models?available=true`, { method: "GET" });
    expect(seen[0].request.credential).toBeUndefined();
    expect(seen[0].request.cacheNamespaceRoot).toBeUndefined();
    await expect(
      customFetch(`${apiUrl}/v1/responses`, { method: "POST", body: "{}" })
    ).rejects.toThrow("fresh transport v2 sign-in");
    expect(seen).toHaveLength(1);
  });

  test("treats an explicitly empty API key as invalid instead of falling back to the user", async () => {
    const request = mock(async () => result(Response.json({ ok: true })));
    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "", apiUrl },
      dependencies(request, userCredentials())
    );

    await expect(customFetch(`${apiUrl}/v1/models`)).rejects.toThrow("API key must not be empty");
    expect(request).toHaveBeenCalledTimes(0);
  });

  test("rejects automatic OpenAI retries before making another transport request", async () => {
    const request = mock(async () => result(Response.json({ ok: true })));
    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "key", apiUrl },
      dependencies(request)
    );

    await expect(
      customFetch(`${apiUrl}/v1/responses`, {
        method: "POST",
        body: "{}",
        headers: { "x-stainless-retry-count": "1" }
      })
    ).rejects.toThrow("Configure maxRetries: 0");
    expect(request).toHaveBeenCalledTimes(0);
  });

  test("never retries an ambiguous transport failure", async () => {
    const request = mock(async () => {
      throw new Error("connection dropped after send");
    });
    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "key", apiUrl },
      dependencies(request)
    );

    await expect(
      customFetch(`${apiUrl}/v1/responses`, { method: "POST", body: "{}" })
    ).rejects.toThrow("connection dropped after send");
    expect(request).toHaveBeenCalledTimes(1);
  });

  test("rejects cross-origin, outside-base, and fragment targets before transport", async () => {
    const request = mock(async () => result(Response.json({ ok: true })));
    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "key", apiUrl },
      dependencies(request)
    );

    for (const target of [
      "https://other.example.test/base/v1/models",
      "https://api.example.test/base-evil/v1/models",
      "https://api.example.test/base/v1/models#hidden"
    ]) {
      await expect(customFetch(target)).rejects.toThrow("attested API");
    }
    expect(request).toHaveBeenCalledTimes(0);
  });

  test("returns authenticated SSE and native binary responses without rewriting them", async () => {
    const encoder = new TextEncoder();
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(encoder.encode("data: first\n\n"));
        controller.enqueue(encoder.encode("data: [DONE]\n\n"));
        controller.close();
      }
    });
    const sse = new Response(stream, { headers: { "content-type": "text/event-stream" } });
    const audio = new Uint8Array([0, 1, 2, 0xff]);
    const responses = [sse, new Response(audio, { headers: { "content-type": "audio/mpeg" } })];
    const deps = dependencies(async () => result(responses.shift()!));
    const customFetch = createCustomFetchWithDependencies({ apiKey: "key", apiUrl }, deps);

    const returnedSse = await customFetch(`${apiUrl}/v1/responses`, {
      method: "POST",
      body: "{}"
    });
    expect(returnedSse).toBe(sse);
    expect(await returnedSse.text()).toBe("data: first\n\ndata: [DONE]\n\n");

    const returnedAudio = await customFetch(`${apiUrl}/v1/audio/speech`, {
      method: "POST",
      body: "{}"
    });
    expect(returnedAudio.headers.get("content-type")).toBe("audio/mpeg");
    expect(new Uint8Array(await returnedAudio.arrayBuffer())).toEqual(audio);
  });

  test("performs no transport work for a pre-aborted request", async () => {
    const request = mock(async () => result(Response.json({ ok: true })));
    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "key", apiUrl },
      dependencies(request)
    );
    const controller = new AbortController();
    controller.abort();

    await expect(
      customFetch(`${apiUrl}/v1/responses`, {
        method: "POST",
        body: "{}",
        signal: controller.signal
      })
    ).rejects.toMatchObject({ name: "AbortError" });
    expect(request).toHaveBeenCalledTimes(0);
  });

  test("an abort during authority preparation prevents the application send", async () => {
    const stored = userCredentials();
    const authorityReady = deferred<void>();
    const request = mock(async () => result(Response.json({ ok: true })));
    const deps = dependencies(request, stored);
    deps.auth.authority = mock(async () => {
      await authorityReady.promise;
      return {
        credential: { kind: "bearer", value: stored.accessToken },
        credentials: stored,
        snapshot: {
          apiOrigin: stored.apiOrigin,
          kind: "user",
          principalId: stored.principalId,
          revision: stored.revision
        },
        assertCurrent() {}
      };
    });
    const customFetch = createCustomFetchWithDependencies({ apiUrl }, deps);
    const controller = new AbortController();

    const pending = customFetch(`${apiUrl}/v1/responses`, {
      method: "POST",
      body: "{}",
      signal: controller.signal
    });
    expect(deps.auth.authority).toHaveBeenCalledTimes(1);
    controller.abort();
    authorityReady.resolve();

    await expect(pending).rejects.toMatchObject({ name: "AbortError" });
    expect(request).toHaveBeenCalledTimes(0);
  });

  test("preserves Fetch signal inheritance and explicit null detachment", async () => {
    const request = mock(async (input: TransportV2RuntimeRequest) => {
      expect(input.signal).toBeNull();
      return result(Response.json({ object: "list", data: [] }));
    });
    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "key", apiUrl },
      dependencies(request)
    );

    const inheritedController = new AbortController();
    inheritedController.abort();
    const inherited = new Request(`${apiUrl}/v1/models`, {
      signal: inheritedController.signal
    });
    await expect(customFetch(inherited, { signal: undefined })).rejects.toMatchObject({
      name: "AbortError"
    });
    expect(request).toHaveBeenCalledTimes(0);

    const detachedController = new AbortController();
    detachedController.abort();
    const detached = new Request(`${apiUrl}/v1/models`, {
      signal: detachedController.signal
    });
    await expect(customFetch(detached, { signal: null })).resolves.toBeInstanceOf(Response);
    expect(request).toHaveBeenCalledTimes(1);
  });

  test("pins API key, endpoint, and PCR policy when caller options mutate in flight", async () => {
    const mutablePcrConfig: PcrConfig = {
      environment: "development",
      remoteAttestation: false,
      pcr0DevValues: ["11".repeat(48)]
    };
    const options = {
      apiKey: "first-api-key",
      apiUrl,
      pcrConfig: mutablePcrConfig
    };
    let seen: TransportV2RuntimeRequest | undefined;
    const customFetch = createCustomFetchWithDependencies(
      options,
      dependencies(async (input) => {
        seen = copyRequest(input);
        return result(Response.json({ ok: true }));
      })
    );

    const pending = customFetch(`${apiUrl}/v1/responses`, { method: "POST", body: "{}" });
    options.apiKey = "second-api-key";
    options.apiUrl = "https://other.example.test";
    mutablePcrConfig.environment = "production";
    mutablePcrConfig.remoteAttestation = true;
    mutablePcrConfig.pcr0DevValues![0] = "22".repeat(48);

    await pending;
    expect(seen?.apiUrl).toBe(apiUrl);
    expect(seen?.request.credential).toEqual({ kind: "api_key", value: "first-api-key" });
    expect(seen?.pcrConfig).toMatchObject({
      environment: "development",
      remoteAttestation: false,
      pcr0DevValues: ["11".repeat(48)]
    });
  });
});
