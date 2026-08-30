import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { beginNativeOAuthAttempt } from "@/services/nativeOAuthAttempt";

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

interface DeepLinkEvent {
  payload: string;
}

let deepLinkListener: ((event: DeepLinkEvent) => void) | undefined;
let currentUser: { id: string } | undefined;
const unlisten = mock(() => {});
const importAuthBundle = mock(async (bundle: string, apiUrl: string) => {
  void bundle;
  void apiUrl;
});

mock.module("@opensecret/react", () => ({
  exportTransportV2AuthBundle: async () => "test-bundle",
  importTransportV2AuthBundle: importAuthBundle,
  useOpenSecret: () => ({ auth: { user: currentUser } })
}));

const realPlatform = await import("@/utils/platform");
mock.module("@/utils/platform", () => ({
  ...realPlatform,
  isTauri: () => true
}));

mock.module("@tauri-apps/api/event", () => ({
  listen: async (_eventName: string, listener: (event: DeepLinkEvent) => void) => {
    deepLinkListener = listener;
    return unlisten;
  }
}));

const { DeepLinkHandler } = await import("./DeepLinkHandler");

const originalGlobals = {
  localStorage: Object.getOwnPropertyDescriptor(globalThis, "localStorage"),
  window: Object.getOwnPropertyDescriptor(globalThis, "window")
};

function restoreGlobal(name: string, descriptor: PropertyDescriptor | undefined): void {
  if (descriptor) {
    Object.defineProperty(globalThis, name, descriptor);
  } else {
    Reflect.deleteProperty(globalThis, name);
  }
}

describe("DeepLinkHandler native auth callbacks", () => {
  let location: { href: string };
  let renderer: ReactTestRenderer | null;
  let storage: MemoryStorage;
  let originalConsoleError: typeof console.error;
  let originalConsoleLog: typeof console.log;
  let originalConsoleWarn: typeof console.warn;

  beforeEach(async () => {
    deepLinkListener = undefined;
    currentUser = undefined;
    renderer = null;
    storage = new MemoryStorage();
    location = { href: "tauri://localhost/" };
    importAuthBundle.mockClear();

    Object.defineProperty(globalThis, "localStorage", {
      configurable: true,
      value: storage,
      writable: true
    });
    Object.defineProperty(globalThis, "window", {
      configurable: true,
      value: { localStorage: storage, location },
      writable: true
    });

    originalConsoleError = console.error;
    originalConsoleLog = console.log;
    originalConsoleWarn = console.warn;
    console.error = mock(() => {});
    console.log = mock(() => {});
    console.warn = mock(() => {});

    await act(async () => {
      renderer = create(<DeepLinkHandler />);
      await Promise.resolve();
    });
    if (!deepLinkListener) throw new Error("Deep-link listener was not registered");
  });

  afterEach(() => {
    if (renderer) {
      act(() => renderer?.unmount());
    }
    restoreGlobal("localStorage", originalGlobals.localStorage);
    restoreGlobal("window", originalGlobals.window);
    console.error = originalConsoleError;
    console.log = originalConsoleLog;
    console.warn = originalConsoleWarn;
  });

  async function emitAuthLink(
    authBundle = "opaque-incoming-bundle",
    nativeOAuthAttemptId = "00000000-0000-4000-8000-000000000000"
  ): Promise<void> {
    const query = new URLSearchParams({
      auth_bundle: authBundle,
      native_oauth_attempt: nativeOAuthAttemptId,
      next: "/settings"
    });
    await act(async () => {
      deepLinkListener?.({ payload: `cloud.opensecret.maple://auth?${query}` });
      await Promise.resolve();
      await Promise.resolve();
    });
  }

  test("preserves an authenticated session and consumes any pending marker", async () => {
    const nativeOAuthAttemptId = beginNativeOAuthAttempt();
    currentUser = { id: "current-user" };
    act(() => renderer?.update(<DeepLinkHandler />));

    await emitAuthLink("opaque-incoming-bundle", nativeOAuthAttemptId);

    expect(importAuthBundle).not.toHaveBeenCalled();
    expect(location.href).toBe("tauri://localhost/");

    currentUser = undefined;
    act(() => renderer?.update(<DeepLinkHandler />));
    await emitAuthLink("second-bundle", nativeOAuthAttemptId);
    expect(importAuthBundle).not.toHaveBeenCalled();
  });

  test("rejects an unsolicited auth callback without a pending marker", async () => {
    await emitAuthLink();

    expect(importAuthBundle).not.toHaveBeenCalled();
    expect(location.href).toBe("tauri://localhost/");
  });

  test("accepts a pending auth callback once and preserves its safe redirect", async () => {
    const nativeOAuthAttemptId = beginNativeOAuthAttempt();

    await emitAuthLink("opaque-incoming-bundle", nativeOAuthAttemptId);

    expect(importAuthBundle).toHaveBeenCalledTimes(1);
    expect(importAuthBundle.mock.calls[0]?.[0]).toBe("opaque-incoming-bundle");
    expect(location.href).toBe("/settings");

    location.href = "tauri://localhost/";
    await emitAuthLink("replayed-bundle", nativeOAuthAttemptId);
    expect(importAuthBundle).toHaveBeenCalledTimes(1);
    expect(location.href).toBe("tauri://localhost/");
  });

  test("does not consume the pending marker for a callback missing the bundle", async () => {
    const nativeOAuthAttemptId = beginNativeOAuthAttempt();
    await act(async () => {
      deepLinkListener?.({
        payload: `cloud.opensecret.maple://auth?native_oauth_attempt=${encodeURIComponent(nativeOAuthAttemptId)}`
      });
      await Promise.resolve();
    });

    await emitAuthLink("opaque-incoming-bundle", nativeOAuthAttemptId);

    expect(importAuthBundle).toHaveBeenCalledTimes(1);
    expect(importAuthBundle.mock.calls[0]?.[0]).toBe("opaque-incoming-bundle");
  });

  test("does not treat an unrelated custom-scheme host as an auth callback", async () => {
    const nativeOAuthAttemptId = beginNativeOAuthAttempt();
    await act(async () => {
      deepLinkListener?.({
        payload: "cloud.opensecret.maple://unrelated?auth_bundle=opaque-incoming-bundle"
      });
      await Promise.resolve();
    });

    expect(importAuthBundle).not.toHaveBeenCalled();
    await emitAuthLink("opaque-incoming-bundle", nativeOAuthAttemptId);
    expect(importAuthBundle).toHaveBeenCalledTimes(1);
  });

  test("rejects mismatched state without consuming the genuine callback", async () => {
    const nativeOAuthAttemptId = beginNativeOAuthAttempt();

    await emitAuthLink("attacker-account-bundle", "00000000-0000-4000-8000-000000000000");
    expect(importAuthBundle).not.toHaveBeenCalled();
    expect(location.href).toBe("tauri://localhost/");

    await emitAuthLink("opaque-incoming-bundle", nativeOAuthAttemptId);
    expect(importAuthBundle).toHaveBeenCalledTimes(1);
    expect(importAuthBundle.mock.calls[0]?.[0]).toBe("opaque-incoming-bundle");
  });

  test("rejects a callback missing state without consuming the genuine callback", async () => {
    const nativeOAuthAttemptId = beginNativeOAuthAttempt();
    await act(async () => {
      deepLinkListener?.({
        payload: "cloud.opensecret.maple://auth?auth_bundle=bundle-without-state"
      });
      await Promise.resolve();
    });

    expect(importAuthBundle).not.toHaveBeenCalled();
    await emitAuthLink("opaque-incoming-bundle", nativeOAuthAttemptId);
    expect(importAuthBundle).toHaveBeenCalledTimes(1);
  });
});
