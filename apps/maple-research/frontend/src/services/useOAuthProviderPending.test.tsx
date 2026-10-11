import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { useOAuthProviderPending } from "./useOAuthProviderPending";

type PendingProvider = ReturnType<typeof useOAuthProviderPending>;
const originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");

describe("OAuth provider pending feedback", () => {
  let renderer: ReactTestRenderer | undefined;
  let pending: PendingProvider;
  let events: EventTarget;

  function Probe() {
    pending = useOAuthProviderPending();
    return <span>{pending.pendingProvider}</span>;
  }

  beforeEach(() => {
    events = new EventTarget();
    Object.defineProperty(globalThis, "window", { configurable: true, value: events });
    act(() => {
      renderer = create(<Probe />);
    });
  });

  afterEach(() => {
    act(() => renderer?.unmount());
    if (originalWindow) Object.defineProperty(globalThis, "window", originalWindow);
    else Reflect.deleteProperty(globalThis, "window");
  });

  test("claims a provider synchronously before React can render another tap", () => {
    const begin = pending.beginProvider;
    act(() => {
      expect(begin("github")).toBe(true);
      expect(begin("github")).toBe(false);
      expect(begin("google")).toBe(false);
    });
    expect(pending.pendingProvider).toBe("github");
    expect(renderer?.root.findByType("span").children).toEqual(["github"]);
  });

  test("clearing feedback permits a fresh provider attempt", () => {
    act(() => {
      pending.beginProvider("github");
    });
    act(() => pending.clearPendingProvider());
    expect(pending.pendingProvider).toBeNull();
    act(() => {
      expect(pending.beginProvider("google")).toBe(true);
    });
    expect(pending.pendingProvider).toBe("google");
  });

  test("web feedback resets only when restoring the page from the back-forward cache", () => {
    act(() => {
      pending.beginProvider("google");
      events.dispatchEvent(Object.assign(new Event("pageshow"), { persisted: false }));
    });
    expect(pending.pendingProvider).toBe("google");
    act(() => {
      events.dispatchEvent(Object.assign(new Event("pageshow"), { persisted: true }));
    });
    expect(pending.pendingProvider).toBeNull();
    act(() => {
      expect(pending.beginProvider("github")).toBe(true);
    });
  });
});
