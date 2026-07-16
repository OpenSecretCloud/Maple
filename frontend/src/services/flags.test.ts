import { describe, expect, test } from "bun:test";
import { FlagsClient, type FlagsFetch } from "./flags";

const USER_A = "00000000-0000-0000-0000-000000000001";
const USER_B = "00000000-0000-0000-0000-000000000002";

function jsonResponse(userId: string, flags: Record<string, unknown>, status = 200): Response {
  return new Response(JSON.stringify({ user_uuid: userId, flags }), {
    status,
    headers: { "content-type": "application/json" }
  });
}

function client(fetchFn: FlagsFetch, options: { now?: () => number; cacheTtlMs?: number } = {}) {
  return new FlagsClient({
    baseUrl: "https://flags.example.test/base",
    requestTimeoutMs: 1_000,
    fetchFn,
    ...options
  });
}

describe("FlagsClient", () => {
  test("uses the public endpoint with normalized keys and no credentials", async () => {
    let requestUrl: URL | undefined;
    let requestInit: RequestInit | undefined;
    const flags = client(async (input, init) => {
      requestUrl = new URL(input.toString());
      requestInit = init;
      return jsonResponse(USER_A, { alpha: true, beta: false });
    });

    await expect(flags.getFlags(USER_A, [" beta ", "alpha", "beta"])).resolves.toEqual({
      alpha: true,
      beta: false
    });
    expect(requestUrl?.pathname).toBe(`/base/v1/users/${USER_A}/flags`);
    expect(requestUrl?.searchParams.get("keys")).toBe("alpha,beta");
    expect(requestInit?.credentials).toBe("omit");
    expect(requestInit?.cache).toBe("no-store");
    expect(new Headers(requestInit?.headers).has("authorization")).toBe(false);
  });

  test("treats a missing flag as disabled", async () => {
    const flags = client(async () => jsonResponse(USER_A, {}));
    expect(flags.peekIsEnabled(USER_A, "missing")).toBeUndefined();
    await expect(flags.isEnabled(USER_A, "missing")).resolves.toBe(false);
    expect(flags.peekIsEnabled(USER_A, "missing")).toBe(false);
  });

  test("only exposes settled cached values synchronously", async () => {
    let resolveRequest: ((response: Response) => void) | undefined;
    const flags = client(
      () =>
        new Promise<Response>((resolve) => {
          resolveRequest = resolve;
        })
    );

    const pending = flags.isEnabled(USER_A, " enabled ");
    expect(flags.peekIsEnabled(USER_A, "enabled")).toBeUndefined();

    resolveRequest?.(jsonResponse(USER_A, { enabled: true }));
    await expect(pending).resolves.toBe(true);
    expect(flags.peekIsEnabled(` ${USER_A} `, " enabled ")).toBe(true);
  });

  test("coalesces concurrent and normalized equivalent lookups", async () => {
    let calls = 0;
    let resolveRequest: ((response: Response) => void) | undefined;
    const flags = client(
      () =>
        new Promise<Response>((resolve) => {
          calls += 1;
          resolveRequest = resolve;
        })
    );

    const first = flags.getFlags(USER_A, ["beta", "alpha"]);
    const second = flags.getFlags(USER_A, ["alpha", "beta", "alpha"]);
    expect(first).toBe(second);
    expect(calls).toBe(1);

    resolveRequest?.(jsonResponse(USER_A, { alpha: true, beta: false }));
    await expect(Promise.all([first, second])).resolves.toHaveLength(2);
    expect(calls).toBe(1);
  });

  test("caches successful responses for the configured TTL", async () => {
    let now = 1_000;
    let calls = 0;
    const flags = client(
      async () => {
        calls += 1;
        return jsonResponse(USER_A, { enabled: calls === 1 });
      },
      { now: () => now, cacheTtlMs: 600_000 }
    );

    await expect(flags.isEnabled(USER_A, "enabled")).resolves.toBe(true);
    now = 600_999;
    expect(flags.peekIsEnabled(USER_A, "enabled")).toBe(true);
    await expect(flags.isEnabled(USER_A, "enabled")).resolves.toBe(true);
    expect(calls).toBe(1);

    now = 601_000;
    expect(flags.peekIsEnabled(USER_A, "enabled")).toBeUndefined();
    await expect(flags.isEnabled(USER_A, "enabled")).resolves.toBe(false);
    expect(flags.peekIsEnabled(USER_A, "enabled")).toBe(false);
    expect(calls).toBe(2);
  });

  test("isolates cache entries by user", async () => {
    let calls = 0;
    const flags = client(async (input) => {
      calls += 1;
      const userId = new URL(input.toString()).pathname.includes(USER_A) ? USER_A : USER_B;
      return jsonResponse(userId, { enabled: userId === USER_A });
    });

    await expect(flags.isEnabled(USER_A, "enabled")).resolves.toBe(true);
    expect(flags.peekIsEnabled(USER_A, "enabled")).toBe(true);
    expect(flags.peekIsEnabled(USER_B, "enabled")).toBeUndefined();
    await expect(flags.isEnabled(USER_B, "enabled")).resolves.toBe(false);
    expect(flags.peekIsEnabled(USER_B, "enabled")).toBe(false);
    expect(calls).toBe(2);
  });

  test("does not cache HTTP failures", async () => {
    let calls = 0;
    const flags = client(async () => {
      calls += 1;
      if (calls === 1) return jsonResponse(USER_A, {}, 503);
      return jsonResponse(USER_A, { enabled: true });
    });

    await expect(flags.isEnabled(USER_A, "enabled")).rejects.toThrow("status 503");
    expect(flags.peekIsEnabled(USER_A, "enabled")).toBeUndefined();
    await expect(flags.isEnabled(USER_A, "enabled")).resolves.toBe(true);
    expect(calls).toBe(2);
  });

  test("rejects invalid responses without caching them", async () => {
    let calls = 0;
    const flags = client(async () => {
      calls += 1;
      if (calls === 1) return jsonResponse(USER_A, { enabled: "yes" });
      return jsonResponse(USER_A, { enabled: true });
    });

    await expect(flags.isEnabled(USER_A, "enabled")).rejects.toThrow("non-boolean");
    await expect(flags.isEnabled(USER_A, "enabled")).resolves.toBe(true);
    expect(calls).toBe(2);
  });
});
