import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import {
  authorizeNativeOAuthCallback,
  beginNativeOAuthAttempt,
  cancelNativeOAuthAttempt,
  PENDING_NATIVE_OAUTH_ATTEMPT_TTL_MS
} from "./nativeOAuthAttempt";

class MemoryStorage implements Storage {
  private readonly values = new Map<string, string>();

  get length(): number {
    return this.values.size;
  }

  clear(): void {
    this.values.clear();
  }

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  key(index: number): string | null {
    return [...this.values.keys()][index] ?? null;
  }

  removeItem(key: string): void {
    this.values.delete(key);
  }

  setItem(key: string, value: string): void {
    this.values.set(key, value);
  }
}

const originalLocalStorage = Object.getOwnPropertyDescriptor(globalThis, "localStorage");

function restoreLocalStorage(): void {
  if (originalLocalStorage) {
    Object.defineProperty(globalThis, "localStorage", originalLocalStorage);
  } else {
    Reflect.deleteProperty(globalThis, "localStorage");
  }
}

describe("native OAuth attempt authorization", () => {
  let storage: MemoryStorage;

  beforeEach(() => {
    storage = new MemoryStorage();
    Object.defineProperty(globalThis, "localStorage", {
      configurable: true,
      value: storage,
      writable: true
    });
  });

  afterEach(() => {
    restoreLocalStorage();
  });

  test("a retry replaces the marker and an older failed attempt cannot cancel it", () => {
    const firstAttemptId = beginNativeOAuthAttempt(1_000);
    const secondAttemptId = beginNativeOAuthAttempt(2_000);

    expect(secondAttemptId).not.toBe(firstAttemptId);
    cancelNativeOAuthAttempt(firstAttemptId);
    expect(authorizeNativeOAuthCallback(false, 2_001)).toBe("accepted");
    expect(authorizeNativeOAuthCallback(false, 2_001)).toBe("missing_or_expired_attempt");
  });

  test("canceling the current browser-open attempt removes its marker", () => {
    const attemptId = beginNativeOAuthAttempt(1_000);

    cancelNativeOAuthAttempt(attemptId);

    expect(authorizeNativeOAuthCallback(false, 1_001)).toBe("missing_or_expired_attempt");
  });

  test("rejects and clears an expired or future-dated marker", () => {
    beginNativeOAuthAttempt(1_000);
    expect(
      authorizeNativeOAuthCallback(false, 1_000 + PENDING_NATIVE_OAUTH_ATTEMPT_TTL_MS + 1)
    ).toBe("missing_or_expired_attempt");

    beginNativeOAuthAttempt(2_000);
    expect(authorizeNativeOAuthCallback(false, 1_999)).toBe("missing_or_expired_attempt");
    expect(authorizeNativeOAuthCallback(false, 2_001)).toBe("missing_or_expired_attempt");
  });

  test("rejects a callback for an authenticated user and clears the marker", () => {
    beginNativeOAuthAttempt(1_000);

    expect(authorizeNativeOAuthCallback(true, 1_001)).toBe("already_authenticated");
    expect(authorizeNativeOAuthCallback(false, 1_001)).toBe("missing_or_expired_attempt");
  });

  test("rejects an unsolicited callback without a marker", () => {
    expect(authorizeNativeOAuthCallback(false, 1_000)).toBe("missing_or_expired_attempt");
  });

  test("rejects and removes malformed marker data", () => {
    beginNativeOAuthAttempt(1_000);
    const markerKey = storage.key(0);
    if (!markerKey) throw new Error("Expected the pending marker to be stored");
    storage.setItem(markerKey, "not-json");

    expect(authorizeNativeOAuthCallback(false, 1_001)).toBe("missing_or_expired_attempt");
    expect(storage.getItem(markerKey)).toBeNull();
  });
});
