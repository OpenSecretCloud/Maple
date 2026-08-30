import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import type { MapleApiAuthInvalidated } from "@/services/mapleApiAuthService";

let currentUserId: string | null = null;
let releaseSignOut: (() => void) | null = null;
const signOut = mock(
  async () =>
    await new Promise<void>((resolve) => {
      releaseSignOut = resolve;
    })
);

mock.module("@opensecret/react", () => ({
  exportTransportV2AuthBundle: async () => "test-bundle",
  importTransportV2AuthBundle: async () => {},
  useOpenSecret: () => ({
    auth: { user: currentUserId ? { user: { id: currentUserId } } : undefined },
    signOut
  })
}));

const { MapleApiAuthInvalidationHandler } = await import("./MapleApiAuthInvalidationHandler");

class FakeInvalidationSource {
  handler: ((event: MapleApiAuthInvalidated) => void) | null = null;

  subscribeInvalidation(handler: (event: MapleApiAuthInvalidated) => void): () => void {
    this.handler = handler;
    return () => {
      if (this.handler === handler) this.handler = null;
    };
  }

  emit(userId: string): void {
    this.handler?.({ userId });
  }
}

describe("MapleApiAuthInvalidationHandler", () => {
  let renderer: ReactTestRenderer | null = null;
  let source: FakeInvalidationSource;

  beforeEach(() => {
    currentUserId = "user-a";
    releaseSignOut = null;
    signOut.mockClear();
    source = new FakeInvalidationSource();
    act(() => {
      renderer = create(<MapleApiAuthInvalidationHandler source={source} />);
    });
  });

  afterEach(() => {
    releaseSignOut?.();
    act(() => renderer?.unmount());
    renderer = null;
  });

  test("signs out only the matching UI account and coalesces duplicate native events", async () => {
    source.emit("user-b");
    source.emit("USER-A");
    source.emit("user-a");

    expect(signOut).toHaveBeenCalledTimes(1);
    releaseSignOut?.();
    await act(async () => await Promise.resolve());
  });

  test("uses the latest rendered account when a native event arrives", () => {
    currentUserId = "user-b";
    act(() => renderer?.update(<MapleApiAuthInvalidationHandler source={source} />));

    source.emit("user-a");
    expect(signOut).not.toHaveBeenCalled();
    source.emit("user-b");
    expect(signOut).toHaveBeenCalledTimes(1);
  });

  test("unsubscribes when the application boundary unmounts", () => {
    act(() => renderer?.unmount());
    renderer = null;

    source.emit("user-a");
    expect(signOut).not.toHaveBeenCalled();
  });
});
