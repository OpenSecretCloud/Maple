import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import {
  authenticatedApiCallWithDependencies,
  encryptedApiCallWithDependencies,
  openAiAuthenticatedApiCallWithDependencies,
  type EncryptedApiDependencies
} from "../encryptedApi";
import type { TransportV2FetchInput } from "../transportV2/client";
import { clearTransportV2Credentials, installTransportV2Credentials } from "../transportV2/auth";

const appUrl = "https://app.example.test";
const platformUrl = "https://platform.example.test";

function token(
  kind: "user" | "platform",
  tokenKind: "access_descriptor" | "resumption",
  principalId: string
): string {
  const audience = `urn:opensecret:internal:transport-v2:${kind}:${
    tokenKind === "access_descriptor" ? "access-descriptor" : "resumption"
  }`;
  const payload = Buffer.from(
    JSON.stringify({
      iss: "urn:opensecret:transport-v2",
      aud: audience,
      tv: 2,
      tk: tokenKind,
      pk: kind,
      sub: principalId,
      exp: 2_000_000_000
    })
  ).toString("base64url");
  return `e30.${payload}.c2ln`;
}

function install(kind: "user" | "platform", apiUrl: string, principalId: string): void {
  installTransportV2Credentials(
    apiUrl,
    kind,
    token(kind, "access_descriptor", principalId),
    token(kind, "resumption", principalId)
  );
}

beforeEach(() => {
  install("user", appUrl, "user-123");
  install("platform", platformUrl, "platform-123");
});

afterEach(() => {
  clearTransportV2Credentials(appUrl);
  clearTransportV2Credentials(platformUrl);
  localStorage.clear();
});

function dependencies(
  implementation: (input: TransportV2FetchInput) => Promise<Response>
): EncryptedApiDependencies {
  return {
    client: { fetch: mock(implementation) },
    getApiUrl: () => appUrl,
    getApiPcrConfig: () => ({ environment: "development" }),
    getPlatformApiUrl: () => platformUrl,
    getPlatformPcrConfig: () => ({ environment: "development" })
  };
}

describe("typed transport v2 API adapter", () => {
  test("binds login to an anonymous user session with no outer Authorization", async () => {
    const deps = dependencies(async (input) => {
      expect(input).toMatchObject({
        apiUrl: appUrl,
        url: `${appUrl}/login`,
        method: "POST",
        responseMode: "unary",
        authority: { kind: "anonymous", purpose: "user" }
      });
      expect(new Headers(input.headers).get("content-type")).toBe("application/json");
      expect(new Headers(input.headers).has("authorization")).toBe(false);
      expect(JSON.parse(new TextDecoder().decode(input.body!))).toEqual({
        email: "person@example.test",
        password: "secret"
      });
      return Response.json({ access_token: "descriptor", refresh_token: "resumption" });
    });

    await expect(
      encryptedApiCallWithDependencies(
        `${appUrl}/login`,
        "POST",
        { email: "person@example.test", password: "secret" },
        undefined,
        undefined,
        deps
      )
    ).resolves.toEqual({ access_token: "descriptor", refresh_token: "resumption" });
  });

  test("keeps app and platform bound authorities separate", async () => {
    const seen: TransportV2FetchInput[] = [];
    const deps = dependencies(async (input) => {
      seen.push(input);
      return Response.json({ ok: true });
    });

    await authenticatedApiCallWithDependencies(
      `${appUrl}/protected/user`,
      "GET",
      undefined,
      undefined,
      deps
    );
    await authenticatedApiCallWithDependencies(
      `${platformUrl}/platform/me`,
      "GET",
      undefined,
      undefined,
      deps
    );

    expect(seen.map((input) => input.authority)).toEqual([
      expect.objectContaining({ kind: "user", principalId: "user-123" }),
      expect.objectContaining({ kind: "platform", principalId: "platform-123" })
    ]);
    expect(seen.every((input) => input.body === null)).toBe(true);
  });

  test("hands an explicit API key only to the v2 authority binder", async () => {
    const deps = dependencies(async (input) => {
      expect(input.authority).toEqual({ kind: "api_key", value: "raw-api-key" });
      expect(new Headers(input.headers).has("authorization")).toBe(false);
      return Response.json({ ok: true });
    });

    await expect(
      openAiAuthenticatedApiCallWithDependencies(
        `${appUrl}/v1/audio/transcriptions`,
        "POST",
        { audio: "bytes" },
        undefined,
        "raw-api-key",
        deps
      )
    ).resolves.toEqual({ ok: true });
  });

  test("never transparently retries after the v2 manager may have sent", async () => {
    let sends = 0;
    const deps = dependencies(async () => {
      sends += 1;
      throw new Error("ambiguous network failure");
    });

    await expect(
      authenticatedApiCallWithDependencies(
        `${appUrl}/protected/kv`,
        "DELETE",
        undefined,
        undefined,
        deps
      )
    ).rejects.toThrow("ambiguous network failure");
    expect(sends).toBe(1);
  });

  test("preserves authenticated error text and successful empty bodies", async () => {
    const rejected = dependencies(async () =>
      Response.json({ message: "bound request rejected" }, { status: 409 })
    );
    await expect(
      authenticatedApiCallWithDependencies(
        `${appUrl}/protected/kv`,
        "GET",
        undefined,
        undefined,
        rejected
      )
    ).rejects.toThrow("bound request rejected");

    const empty = dependencies(async () => new Response(null, { status: 204 }));
    await expect(
      authenticatedApiCallWithDependencies(
        `${appUrl}/protected/kv`,
        "DELETE",
        undefined,
        undefined,
        empty
      )
    ).resolves.toBeUndefined();
  });

  test("consumes the authenticated Responses stream to its completed object", async () => {
    const completed = {
      id: "response-id",
      object: "response",
      created_at: 1,
      status: "completed",
      model: "test-model"
    };
    const deps = dependencies(async (input) => {
      expect(input.responseMode).toBe("stream");
      return new Response(
        `event: response.created\ndata: {"type":"response.created"}\n\n` +
          `event: response.completed\ndata: ${JSON.stringify({
            type: "response.completed",
            response: completed
          })}\n\n`,
        { headers: { "content-type": "text/event-stream" } }
      );
    });

    await expect(
      authenticatedApiCallWithDependencies(
        `${appUrl}/v1/responses`,
        "POST",
        { model: "test-model", input: "hello" },
        undefined,
        deps
      )
    ).resolves.toEqual(completed);
  });

  test("maps a terminal Responses error to the typed thrown-error surface", async () => {
    const deps = dependencies(
      async () =>
        new Response(
          `event: response.error\ndata: ${JSON.stringify({
            type: "response.error",
            error: { message: "provider failed" }
          })}\n\n`,
          { headers: { "content-type": "text/event-stream" } }
        )
    );

    await expect(
      authenticatedApiCallWithDependencies(
        `${appUrl}/v1/responses`,
        "POST",
        { model: "test-model", input: "hello" },
        undefined,
        deps
      )
    ).rejects.toThrow("provider failed");
  });
});
