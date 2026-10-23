import { expect, test } from "bun:test";
import { createApiKey, deleteApiKey, fetchLogin, fetchModels } from "../../api";
import { createCustomFetch } from "../../ai";

// Explicitly opt in: consumes the fixture account's local admission window.
// No request in this test may reach an inference provider.
test.skipIf(process.env.RUN_RATE_LIMIT_SMOKE !== "1")(
  "local V2 inference admission shares accounts, endpoints and keys, then recovers",
  async () => {
    const apiUrl = process.env.VITE_OPEN_SECRET_API_URL!;
    const endpoint = new URL(apiUrl);
    expect(endpoint.protocol).toBe("http:");
    expect(["localhost", "127.0.0.1", "[::1]"]).toContain(endpoint.hostname);
    await fetchLogin(
      process.env.VITE_TEST_EMAIL!,
      process.env.VITE_TEST_PASSWORD!,
      process.env.VITE_TEST_CLIENT_ID!
    );
    const prefix = `rate-limit-${crypto.randomUUID()}`;
    const key1 = await createApiKey(`${prefix}-1`);
    const key2 = await createApiKey(`${prefix}-2`);
    const jwtFetch = createCustomFetch();
    const keyFetches = [key1, key2].map((key) => createCustomFetch({ apiKey: key.key }));
    const requests = [
      { path: "/v1/chat/completions", body: {}, status: 400 },
      { path: "/v1/embeddings", body: { input: [] }, status: 400 },
      { path: "/v1/systemone", body: {}, status: 422 },
      {
        path: "/v1/responses",
        body: {
          model: "__rate_limit_smoke_invalid_model__",
          input: "local validation",
          conversation: crypto.randomUUID()
        },
        status: 400
      }
    ];
    const send = (fetcher: typeof jwtFetch, request: (typeof requests)[number]) =>
      fetcher(`${apiUrl}${request.path}`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(request.body)
      });
    try {
      const started = Date.now();
      for (let index = 0; index < 100; index += 1) {
        const request = requests[index % requests.length];
        // Responses is JWT-only; the other routes alternate keys and JWT.
        const fetcher =
          request.path === "/v1/responses" ? jwtFetch : [jwtFetch, ...keyFetches][index % 3];
        const response = await send(fetcher, request);
        expect(response.status).toBe(request.status);
        await response.arrayBuffer();
      }
      expect(Date.now() - started).toBeLessThan(60_000);
      for (const request of requests) {
        const response = await send(jwtFetch, request);
        expect(response.status).toBe(429);
        expect(response.headers.get("x-opensecret-error-code")).toBe("rate_limit_exceeded");
        expect(response.headers.get("x-opensecret-client-replay")).toBeNull();
        expect(Number(response.headers.get("retry-after"))).toBeGreaterThan(0);
        expect(Number(response.headers.get("retry-after"))).toBeLessThanOrEqual(60);
        expect((await response.json()).error.code).toBe("rate_limit_exceeded");
      }
      // Changing keys cannot bypass the account cooldown.
      for (const keyFetch of keyFetches) {
        const response = await send(keyFetch, requests[0]);
        expect(response.status).toBe(429);
        await response.arrayBuffer();
      }
      // Non-inference reads remain available during the cooldown.
      expect(await fetchModels()).toBeDefined();
      const unauthenticated = await send(
        createCustomFetch({ apiKey: crypto.randomUUID() }),
        requests[0]
      );
      expect(unauthenticated.status).toBe(401);
      await unauthenticated.arrayBuffer();

      await fetchLogin(
        process.env.VITE_TEST_OTHER_EMAIL!,
        process.env.VITE_TEST_OTHER_PASSWORD!,
        process.env.VITE_TEST_CLIENT_ID!
      );
      const independent = await send(createCustomFetch(), requests[1]);
      expect(independent.status).toBe(400);
      await independent.arrayBuffer();
      await fetchLogin(
        process.env.VITE_TEST_EMAIL!,
        process.env.VITE_TEST_PASSWORD!,
        process.env.VITE_TEST_CLIENT_ID!
      );
      await Bun.sleep(1_100);
      const retry = await send(jwtFetch, requests[0]);
      const remaining = Number(retry.headers.get("retry-after"));
      expect(retry.status).toBe(429);
      expect(remaining).toBeLessThan(60);
      await retry.arrayBuffer();
      await Bun.sleep(remaining * 1_000 + 100);
      const recovered = await send(keyFetches[0], requests[1]);
      expect(recovered.status).toBe(400);
      expect(recovered.headers.get("retry-after")).toBeNull();
      await recovered.arrayBuffer();
    } finally {
      await deleteApiKey(key1.name);
      await deleteApiKey(key2.name);
    }
  },
  120_000
);
