import { describe, expect, mock, test } from "bun:test";
import { mintNativeHandoffGrantWithDependencies } from "../api";

const API_URL = "https://api.example.test";
const NATIVE_SESSION_ID = "abcdef12-2222-3333-4444-555555555555";
const NATIVE_ATTEMPT_ID = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
const GRANT = "header.payload.signature";

describe("native handoff grant minting", () => {
  test("sends the exact authenticated request and returns the validated response", async () => {
    const call = mock(async () => ({ grant: GRANT, expires_at: 1_800_000_000 }));

    await expect(
      mintNativeHandoffGrantWithDependencies(NATIVE_SESSION_ID, NATIVE_ATTEMPT_ID, API_URL, call)
    ).resolves.toEqual({ grant: GRANT, expires_at: 1_800_000_000 });
    expect(call).toHaveBeenCalledTimes(1);
    expect(call).toHaveBeenCalledWith(
      `${API_URL}/auth/native-handoff/grant`,
      "POST",
      {
        native_session_id: NATIVE_SESSION_ID,
        native_attempt_id: NATIVE_ATTEMPT_ID
      },
      "Failed to mint native handoff grant"
    );
  });

  test("rejects non-canonical identifiers before sending", async () => {
    const call = mock(async () => ({ grant: GRANT, expires_at: 1 }));

    await expect(
      mintNativeHandoffGrantWithDependencies(
        NATIVE_SESSION_ID.toUpperCase(),
        NATIVE_ATTEMPT_ID,
        API_URL,
        call
      )
    ).rejects.toThrow("nativeSessionId must be a non-nil canonical lowercase UUID");
    await expect(
      mintNativeHandoffGrantWithDependencies(NATIVE_SESSION_ID, "not-an-attempt", API_URL, call)
    ).rejects.toThrow("nativeAttemptId must be a non-nil canonical lowercase UUID");
    await expect(
      mintNativeHandoffGrantWithDependencies(
        "00000000-0000-0000-0000-000000000000",
        NATIVE_ATTEMPT_ID,
        API_URL,
        call
      )
    ).rejects.toThrow("nativeSessionId must be a non-nil canonical lowercase UUID");
    expect(call).not.toHaveBeenCalled();
  });

  test("requires an exact compact JWT response shape", async () => {
    const invalidResponses: unknown[] = [
      null,
      { grant: GRANT },
      { grant: GRANT, expires_at: 1, extra: true },
      { grant: "", expires_at: 1 },
      { grant: "two.segments", expires_at: 1 },
      { grant: "padded=.payload.signature", expires_at: 1 },
      { grant: `a.${"b".repeat(4093)}.c`, expires_at: 1 },
      { grant: GRANT, expires_at: -1 },
      { grant: GRANT, expires_at: 1.5 },
      { grant: GRANT, expires_at: Number.MAX_SAFE_INTEGER + 1 }
    ];

    for (const response of invalidResponses) {
      await expect(
        mintNativeHandoffGrantWithDependencies(
          NATIVE_SESSION_ID,
          NATIVE_ATTEMPT_ID,
          API_URL,
          async () => response
        )
      ).rejects.toThrow("Native handoff grant response");
    }
  });

  test("accepts the maximum grant length and a zero Unix expiry", async () => {
    const grant = `${"a".repeat(1364)}.${"b".repeat(1364)}.${"c".repeat(1366)}`;
    expect(grant.length).toBe(4096);

    await expect(
      mintNativeHandoffGrantWithDependencies(
        NATIVE_SESSION_ID,
        NATIVE_ATTEMPT_ID,
        API_URL,
        async () => ({ grant, expires_at: 0 })
      )
    ).resolves.toEqual({ grant, expires_at: 0 });
  });
});
