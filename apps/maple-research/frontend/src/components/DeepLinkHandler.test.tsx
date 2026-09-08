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

mock.module("@opensecret/react", () => ({
  useOpenSecret: () => ({ auth: { user: currentUser } })
}));

mock.module("@/utils/platform", () => ({
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

  function emitAuthLink(accessToken = "incoming-access", refreshToken = "incoming-refresh"): void {
    const query = new URLSearchParams({
      access_token: accessToken,
      refresh_token: refreshToken,
      next: "/settings"
    });
    deepLinkListener?.({ payload: `cloud.opensecret.maple://auth?${query}` });
  }

  test("preserves an authenticated session and consumes any pending marker", () => {
    beginNativeOAuthAttempt();
    storage.setItem("access_token", "current-access");
    storage.setItem("refresh_token", "current-refresh");
    currentUser = { id: "current-user" };
    act(() => renderer?.update(<DeepLinkHandler />));

    emitAuthLink();

    expect(storage.getItem("access_token")).toBe("current-access");
    expect(storage.getItem("refresh_token")).toBe("current-refresh");
    expect(location.href).toBe("tauri://localhost/");

    storage.removeItem("access_token");
    storage.removeItem("refresh_token");
    emitAuthLink();
    expect(storage.getItem("access_token")).toBeNull();
  });

  test("rejects an unsolicited auth callback without a pending marker", () => {
    emitAuthLink();

    expect(storage.getItem("access_token")).toBeNull();
    expect(storage.getItem("refresh_token")).toBeNull();
    expect(location.href).toBe("tauri://localhost/");
  });

  test("accepts a pending auth callback once and preserves its safe redirect", () => {
    beginNativeOAuthAttempt();
    storage.setItem("access_token", "stale-access");
    storage.setItem("refresh_token", "stale-refresh");

    emitAuthLink();

    expect(storage.getItem("access_token")).toBe("incoming-access");
    expect(storage.getItem("refresh_token")).toBe("incoming-refresh");
    expect(location.href).toBe("/settings");

    storage.removeItem("access_token");
    storage.removeItem("refresh_token");
    emitAuthLink("replayed-access", "replayed-refresh");
    expect(storage.getItem("access_token")).toBeNull();
  });

  test("does not consume the pending marker for a callback missing required tokens", () => {
    beginNativeOAuthAttempt();
    deepLinkListener?.({
      payload: "cloud.opensecret.maple://auth?access_token=incomplete"
    });

    emitAuthLink();

    expect(storage.getItem("access_token")).toBe("incoming-access");
    expect(storage.getItem("refresh_token")).toBe("incoming-refresh");
  });
});
