import { afterEach, describe, expect, mock, test } from "bun:test";
import OpenAI from "openai";
import { createCustomFetchWithDependencies, type CustomFetchDependencies } from "../ai";
import { clearTransportV2Credentials, installTransportV2Credentials } from "../transportV2/auth";
import type { TransportV2FetchInput } from "../transportV2/client";

const apiUrl = "https://api.example.test/base";
const userId = "00112233-4455-6677-8899-aabbccddeeff";

function token(kind: "access_descriptor" | "resumption"): string {
  const audience =
    kind === "access_descriptor"
      ? "urn:opensecret:internal:transport-v2:user:access-descriptor"
      : "urn:opensecret:internal:transport-v2:user:resumption";
  const claims = {
    iss: "urn:opensecret:transport-v2",
    aud: audience,
    tv: 2,
    tk: kind,
    pk: "user",
    sub: userId,
    exp: Math.floor(Date.now() / 1000) + 3600
  };
  return `e30.${Buffer.from(JSON.stringify(claims)).toString("base64url")}.c2ln`;
}

function dependencies(
  implementation: (input: TransportV2FetchInput) => Promise<Response>
): CustomFetchDependencies {
  return {
    client: { fetch: mock(implementation) },
    getApiUrl: () => apiUrl,
    getApiPcrConfig: () => ({ environment: "development" })
  };
}

afterEach(() => {
  clearTransportV2Credentials(apiUrl);
  localStorage.clear();
  sessionStorage.clear();
});

describe("Transport V2 custom Fetch adapter", () => {
  test("captures exact binary body/query and strips credential, framing, and provider headers", async () => {
    const plaintext = new Uint8Array([0, 1, 2, 0xff]);
    const deps = dependencies(async (input) => {
      expect(input.url).toBe(`${apiUrl}/v1/audio/transcriptions?b=2&a=1`);
      expect(input.method).toBe("POST");
      expect(input.body).toEqual(plaintext);
      expect(input.authority).toEqual({ kind: "api_key", value: "real-api-key" });
      const headers = new Headers(input.headers);
      expect(headers.get("x-safe-metadata")).toBe("kept");
      expect(headers.get("content-type")).toBe("application/octet-stream");
      for (const forbidden of [
        "authorization",
        "user-agent",
        "accept",
        "accept-encoding",
        "content-length",
        "content-md5",
        "digest",
        "x-stainless-lang",
        "x-stainless-retry-count",
        "x-session-id",
        "x-openai-api-key",
        "openai-project"
      ]) {
        expect(headers.has(forbidden)).toBe(false);
      }
      return new Response("ok", { headers: { "x-authenticated": "yes" } });
    });
    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "real-api-key", apiUrl, pcrConfig: { environment: "development" } },
      deps
    );

    const response = await customFetch(`${apiUrl}/v1/audio/transcriptions?b=2&a=1`, {
      method: "POST",
      body: plaintext,
      headers: {
        authorization: "Bearer fake-openai-key",
        "user-agent": "OpenAI/JS test",
        accept: "application/json",
        "accept-encoding": "identity",
        "content-type": "application/octet-stream",
        "content-length": "4",
        "content-md5": "attacker-controlled",
        digest: "sha-256=attacker-controlled",
        "x-stainless-lang": "js",
        "x-stainless-retry-count": "0",
        "x-session-id": "attacker-value",
        "x-openai-api-key": "attacker-value",
        "openai-project": "attacker-value",
        "x-safe-metadata": "kept"
      }
    });
    expect(await response.text()).toBe("ok");
    expect(response.headers.get("x-authenticated")).toBe("yes");
  });

  test("preserves no-body versus explicitly empty body", async () => {
    const bodies: Array<Uint8Array | null> = [];
    const deps = dependencies(async (input) => {
      bodies.push(input.body);
      return Response.json({ ok: true });
    });
    const customFetch = createCustomFetchWithDependencies({ apiKey: "key", apiUrl }, deps);

    await customFetch(`${apiUrl}/v1/models`, { method: "GET" });
    await customFetch(`${apiUrl}/v1/audio/transcriptions`, { method: "POST", body: "" });
    expect(bodies[0]).toBeNull();
    expect(bodies[1]).toEqual(new Uint8Array(0));
  });

  test("selects streaming only for Responses create or strict chat stream true", async () => {
    const modes: string[] = [];
    const deps = dependencies(async (input) => {
      modes.push(input.responseMode);
      return new Response("stream-or-json");
    });
    const customFetch = createCustomFetchWithDependencies({ apiKey: "key", apiUrl }, deps);

    await customFetch(`${apiUrl}/v1/responses`, { method: "POST", body: "{}" });
    await customFetch(`${apiUrl}/v1/chat/completions`, {
      method: "POST",
      body: JSON.stringify({ stream: true })
    });
    await customFetch(`${apiUrl}/v1/chat/completions`, {
      method: "POST",
      body: JSON.stringify({ stream: "true" })
    });
    expect(modes).toEqual(["stream", "stream", "unary"]);
  });

  test("uses the installed user authority and never exposes its descriptor as a header", async () => {
    installTransportV2Credentials(apiUrl, "user", token("access_descriptor"), token("resumption"));
    const deps = dependencies(async (input) => {
      expect(input.authority).toMatchObject({
        kind: "user",
        principalId: "00112233-4455-6677-8899-aabbccddeeff"
      });
      expect(input.authority).toHaveProperty("generation");
      expect(new Headers(input.headers).has("authorization")).toBe(false);
      return Response.json({ ok: true });
    });
    const customFetch = createCustomFetchWithDependencies({ apiUrl }, deps);
    await customFetch(`${apiUrl}/v1/conversations?limit=20`, { method: "GET" });
  });

  test("allows only models to use an anonymous public authority", async () => {
    const seen: TransportV2FetchInput[] = [];
    const deps = dependencies(async (input) => {
      seen.push(input);
      return Response.json({ object: "list", data: [] });
    });
    const customFetch = createCustomFetchWithDependencies({ apiUrl }, deps);

    await customFetch(`${apiUrl}/v1/models`, { method: "GET" });
    expect(seen[0].authority).toEqual({ kind: "anonymous", purpose: "public" });
    await expect(
      customFetch(`${apiUrl}/v1/responses`, { method: "POST", body: "{}" })
    ).rejects.toThrow("fresh transport v2 sign-in");
    expect(seen).toHaveLength(1);
  });

  test("does not retry an ambiguous post-send failure", async () => {
    let sends = 0;
    const deps = dependencies(async () => {
      sends += 1;
      throw new Error("connection dropped after send");
    });
    const customFetch = createCustomFetchWithDependencies({ apiKey: "key", apiUrl }, deps);

    await expect(
      customFetch(`${apiUrl}/v1/responses/response-id/cancel`, { method: "POST" })
    ).rejects.toThrow("connection dropped after send");
    expect(sends).toBe(1);
  });

  test("blocks an OpenAI automatic retry before a second transport send", async () => {
    let transportSends = 0;
    const deps = dependencies(async () => {
      transportSends += 1;
      throw new Error("connection dropped after send");
    });
    const customFetch = createCustomFetchWithDependencies({ apiKey: "key", apiUrl }, deps);
    let customFetchInvocations = 0;
    const openai = new OpenAI({
      apiKey: "not-a-real-api-key",
      baseURL: `${apiUrl}/v1`,
      dangerouslyAllowBrowser: true,
      fetch: (...args) => {
        customFetchInvocations += 1;
        return customFetch(...args);
      },
      maxRetries: 1
    });

    let rejected: unknown;
    try {
      await openai.chat.completions.create({
        model: "test-model",
        messages: [{ role: "user", content: "hello" }]
      });
    } catch (error) {
      rejected = error;
    }
    expect(rejected).toBeInstanceOf(Error);
    expect((rejected as Error).message).toContain("Connection error");
    expect(customFetchInvocations).toBe(2);
    expect(transportSends).toBe(1);
  });

  test("performs no transport work for a pre-aborted request", async () => {
    const clientFetch = mock(async () => Response.json({ ok: true }));
    const customFetch = createCustomFetchWithDependencies(
      { apiKey: "key", apiUrl },
      dependencies(clientFetch)
    );
    const controller = new AbortController();
    controller.abort();

    await expect(
      customFetch(`${apiUrl}/v1/responses`, { method: "POST", signal: controller.signal })
    ).rejects.toMatchObject({ name: "AbortError" });
    expect(clientFetch).toHaveBeenCalledTimes(0);
  });

  test("snapshots API-key authority before asynchronous body capture", async () => {
    const options = { apiKey: "first-key", apiUrl };
    const deps = dependencies(async (input) => {
      expect(input.authority).toEqual({ kind: "api_key", value: "first-key" });
      return Response.json({ ok: true });
    });
    const customFetch = createCustomFetchWithDependencies(options, deps);
    const request = customFetch(`${apiUrl}/v1/responses`, { method: "POST", body: "{}" });
    options.apiKey = "second-key";
    await request;
  });

  test("snapshots the exact user generation before asynchronous body capture", async () => {
    const first = installTransportV2Credentials(
      apiUrl,
      "user",
      token("access_descriptor"),
      token("resumption")
    );
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const body = new ReadableStream<Uint8Array>({
      async start(controller) {
        await gate;
        controller.enqueue(new TextEncoder().encode("{}"));
        controller.close();
      }
    });
    const deps = dependencies(async (input) => {
      expect(input.authority).toEqual({
        kind: "user",
        principalId: first.principalId,
        generation: first.generation
      });
      return Response.json({ ok: true });
    });
    const customFetch = createCustomFetchWithDependencies({ apiUrl }, deps);
    const request = new Request(`${apiUrl}/v1/responses`, {
      method: "POST",
      body,
      duplex: "half"
    } as RequestInit & { duplex: "half" });
    const pending = customFetch(request);
    installTransportV2Credentials(apiUrl, "user", token("access_descriptor"), token("resumption"));
    release();
    await pending;
  });

  test("restores the existing binary TTS response contract", async () => {
    const deps = dependencies(async () =>
      Response.json({ content_base64: "AAEC/w==", content_type: "audio/mpeg" })
    );
    const customFetch = createCustomFetchWithDependencies({ apiKey: "key", apiUrl }, deps);
    const response = await customFetch(`${apiUrl}/v1/audio/speech`, {
      method: "POST",
      body: "{}"
    });

    expect(response.headers.get("content-type")).toBe("audio/mpeg");
    expect(new Uint8Array(await response.arrayBuffer())).toEqual(new Uint8Array([0, 1, 2, 0xff]));
  });
});
