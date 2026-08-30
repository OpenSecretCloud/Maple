import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import { act, create, type ReactTestRenderer } from "react-test-renderer";

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
const nativeOAuthAttemptId = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
const nativeSessionId = "abcdef12-2222-3333-4444-555555555555";
const nativeInvoke = mock(async (command: string) => {
  if (command === "native_oauth_begin") {
    return { nativeOAuthAttempt: nativeOAuthAttemptId, sessionId: nativeSessionId };
  }
  if (command === "native_oauth_redeem") {
    return { userId: "user-one", authBundle: "opaque-redeemed-bundle" };
  }
  return undefined;
});

mock.module("@tauri-apps/api/core", () => ({ invoke: nativeInvoke }));

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

const { beginNativeOAuthAttempt } = await import("@/services/nativeOAuthAttempt");
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
    nativeInvoke.mockClear();

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

  async function emitAuthLink(handoffGrant = "header.payload.signature"): Promise<void> {
    const query = new URLSearchParams({
      handoff_grant: handoffGrant,
      native_session_id: nativeSessionId,
      next: "/settings"
    });
    await act(async () => {
      deepLinkListener?.({ payload: `cloud.opensecret.maple://auth?${query}` });
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });
  }

  async function mirrorNativeAttempt(): Promise<string> {
    return (await beginNativeOAuthAttempt("https://api.example.test")).nativeOAuthAttempt;
  }

  test("preserves an authenticated session and consumes any pending marker", async () => {
    await mirrorNativeAttempt();
    currentUser = { id: "current-user" };
    act(() => renderer?.update(<DeepLinkHandler />));

    await emitAuthLink("header.payload.signature");

    expect(importAuthBundle).not.toHaveBeenCalled();
    expect(location.href).toBe("tauri://localhost/");

    currentUser = undefined;
    act(() => renderer?.update(<DeepLinkHandler />));
    await emitAuthLink("header.second.signature");
    expect(importAuthBundle).not.toHaveBeenCalled();
  });

  test("rejects an unsolicited auth callback without a pending marker", async () => {
    await emitAuthLink();

    expect(importAuthBundle).not.toHaveBeenCalled();
    expect(location.href).toBe("tauri://localhost/");
  });

  test("accepts a pending auth callback once and preserves its safe redirect", async () => {
    await mirrorNativeAttempt();

    await emitAuthLink("header.payload.signature");

    expect(importAuthBundle).toHaveBeenCalledTimes(1);
    expect(importAuthBundle.mock.calls[0]?.[0]).toBe("opaque-redeemed-bundle");
    expect(nativeInvoke).toHaveBeenCalledWith("native_oauth_redeem", {
      request: {
        handoffGrant: "header.payload.signature",
        nativeSessionId
      }
    });
    expect(location.href).toBe("/settings");

    location.href = "tauri://localhost/";
    await emitAuthLink("header.payload.signature");
    expect(importAuthBundle).toHaveBeenCalledTimes(1);
    expect(
      nativeInvoke.mock.calls.filter(([command]) => command === "native_oauth_redeem")
    ).toHaveLength(1);
    expect(location.href).toBe("tauri://localhost/");
  });

  test("does not consume the pending marker for a callback missing the grant", async () => {
    await mirrorNativeAttempt();
    await act(async () => {
      deepLinkListener?.({
        payload: "cloud.opensecret.maple://auth"
      });
      await Promise.resolve();
    });

    await emitAuthLink("header.payload.signature");

    expect(importAuthBundle).toHaveBeenCalledTimes(1);
    expect(importAuthBundle.mock.calls[0]?.[0]).toBe("opaque-redeemed-bundle");
  });

  test("does not treat an unrelated custom-scheme host as an auth callback", async () => {
    await mirrorNativeAttempt();
    await act(async () => {
      deepLinkListener?.({
        payload: "cloud.opensecret.maple://unrelated?handoff_grant=header.payload.signature"
      });
      await Promise.resolve();
    });

    expect(importAuthBundle).not.toHaveBeenCalled();
    await emitAuthLink("header.payload.signature");
    expect(importAuthBundle).toHaveBeenCalledTimes(1);
  });
});
