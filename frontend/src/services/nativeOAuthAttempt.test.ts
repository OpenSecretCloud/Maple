import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";

class MemoryStorage implements Storage {
  private readonly values = new Map<string, string>();
  get length() {
    return this.values.size;
  }
  clear() {
    this.values.clear();
  }
  getItem(key: string) {
    return this.values.get(key) ?? null;
  }
  key(index: number) {
    return [...this.values.keys()][index] ?? null;
  }
  removeItem(key: string) {
    this.values.delete(key);
  }
  setItem(key: string, value: string) {
    this.values.set(key, value);
  }
}

const ATTEMPT_ONE = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
const ATTEMPT_TWO = "11111111-2222-4333-8444-555555555555";
const SESSION_ONE = "abcdef12-2222-3333-4444-555555555555";
const SESSION_TWO = "98765432-2222-3333-4444-555555555555";
const defaultInvoke = async (command: string) => {
  if (command === "native_oauth_begin") {
    return { nativeOAuthAttempt: ATTEMPT_ONE, sessionId: SESSION_ONE };
  }
  if (command === "native_oauth_redeem") {
    return { userId: "user-one", authBundle: "opaque-bundle" };
  }
  return undefined;
};
const invoke = mock(defaultInvoke);

const {
  authorizeNativeOAuthCallback,
  beginNativeOAuthAttempt,
  cancelNativeOAuthAttempt,
  consumeNativeOAuthAttempt,
  isNativeOAuthAttemptId,
  isNativeOAuthSessionId,
  PENDING_NATIVE_OAUTH_ATTEMPT_TTL_MS,
  redeemNativeOAuthGrant
} = await import("./nativeOAuthAttempt");

const originalLocalStorage = Object.getOwnPropertyDescriptor(globalThis, "localStorage");

describe("native OAuth attempt authorization", () => {
  beforeEach(() => {
    invoke.mockClear();
    invoke.mockImplementation(defaultInvoke);
    Object.defineProperty(globalThis, "localStorage", {
      configurable: true,
      value: new MemoryStorage(),
      writable: true
    });
  });

  afterEach(() => {
    if (originalLocalStorage) {
      Object.defineProperty(globalThis, "localStorage", originalLocalStorage);
    } else {
      Reflect.deleteProperty(globalThis, "localStorage");
    }
  });

  test("begins natively and mirrors the returned attempt and session", async () => {
    const result = await beginNativeOAuthAttempt("https://api.example.test", 1_000, invoke);

    expect(result).toEqual({ nativeOAuthAttempt: ATTEMPT_ONE, sessionId: SESSION_ONE });
    expect(invoke).toHaveBeenCalledWith("native_oauth_begin", {
      request: { apiUrl: "https://api.example.test" }
    });
    expect(authorizeNativeOAuthCallback(false, 1_001)).toBe("accepted");
    expect(authorizeNativeOAuthCallback(false, 1_002)).toBe("accepted");
    expect(consumeNativeOAuthAttempt(ATTEMPT_ONE)).toBe(true);
    expect(authorizeNativeOAuthCallback(false, 1_003)).toBe("missing_or_expired_attempt");
  });

  test("rejects the nil UUID for both native attempt and session identifiers", async () => {
    const nilUuid = "00000000-0000-0000-0000-000000000000";
    expect(isNativeOAuthAttemptId(nilUuid)).toBe(false);
    expect(isNativeOAuthSessionId(nilUuid)).toBe(false);

    invoke.mockImplementation(async () => ({
      nativeOAuthAttempt: nilUuid,
      sessionId: SESSION_ONE
    }));
    await expect(
      beginNativeOAuthAttempt("https://api.example.test", 1_000, invoke)
    ).rejects.toThrow("Native OAuth initiation returned invalid state");

    invoke.mockImplementation(async () => ({
      nativeOAuthAttempt: ATTEMPT_ONE,
      sessionId: nilUuid
    }));
    await expect(
      beginNativeOAuthAttempt("https://api.example.test", 1_000, invoke)
    ).rejects.toThrow("Native OAuth initiation returned invalid state");
  });

  test("cancel invokes native for the exact attempt and cannot clear a replacement", async () => {
    await beginNativeOAuthAttempt("https://api.example.test", 1_000, invoke);
    invoke.mockImplementation(async (command: string) => {
      if (command === "native_oauth_begin") {
        return { nativeOAuthAttempt: ATTEMPT_TWO, sessionId: SESSION_TWO };
      }
      return undefined;
    });
    await beginNativeOAuthAttempt("https://api.example.test", 2_000, invoke);

    await cancelNativeOAuthAttempt(ATTEMPT_ONE, invoke);
    expect(invoke).toHaveBeenLastCalledWith("native_oauth_cancel", {
      request: { nativeOAuthAttempt: ATTEMPT_ONE }
    });
    expect(authorizeNativeOAuthCallback(false, 2_001)).toBe("accepted");
  });

  test("authorization preserves the pending attempt until consumption and expiry fails closed", async () => {
    await beginNativeOAuthAttempt("https://api.example.test", 1_000, invoke);

    expect(authorizeNativeOAuthCallback(false, 1_001)).toBe("accepted");
    expect(authorizeNativeOAuthCallback(false, 1_002)).toBe("accepted");
    expect(
      authorizeNativeOAuthCallback(false, 1_000 + PENDING_NATIVE_OAUTH_ATTEMPT_TTL_MS + 1)
    ).toBe("missing_or_expired_attempt");
  });

  test("redeems through local IPC with the exact grant request", async () => {
    const result = await redeemNativeOAuthGrant("header.payload.signature", SESSION_ONE, invoke);

    expect(result).toEqual({ userId: "user-one", authBundle: "opaque-bundle" });
    expect(invoke).toHaveBeenCalledWith("native_oauth_redeem", {
      request: {
        handoffGrant: "header.payload.signature",
        nativeSessionId: SESSION_ONE
      }
    });
  });
});
