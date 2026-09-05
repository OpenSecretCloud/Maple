import { describe, expect, test } from "bun:test";
import {
  MapleApiAuthService,
  type MapleApiAuthBridge,
  type MapleApiAuthChanged,
  type MapleApiAuthMetadata,
  type MapleApiAuthSnapshot
} from "./mapleApiAuthService";

function deferred(): { promise: Promise<void>; resolve: () => void } {
  let resolve = () => {};
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

class FakeAuthBridge implements MapleApiAuthBridge {
  browserBundle = "browser-bundle-one";
  metadata: MapleApiAuthMetadata | null = null;
  nativeSnapshot: MapleApiAuthSnapshot | null = null;
  setCalls = 0;
  getCalls = 0;
  clearCalls = 0;
  listenFailures = 0;
  commandOrder: string[] = [];
  setHook: (() => Promise<void>) | null = null;
  getHook: (() => Promise<void>) | null = null;
  private handler: ((event: MapleApiAuthChanged) => Promise<void>) | null = null;

  isDesktop(): boolean {
    return true;
  }

  apiUrl(): string {
    return "https://enclave.trymaple.ai";
  }

  async exportAuthBundle(): Promise<string> {
    return this.browserBundle;
  }

  async importAuthBundle(bundle: string): Promise<void> {
    this.browserBundle = bundle;
  }

  readMetadata(): MapleApiAuthMetadata | null {
    return this.metadata ? { ...this.metadata } : null;
  }

  writeMetadata(metadata: MapleApiAuthMetadata | null): void {
    this.metadata = metadata ? { ...metadata } : null;
  }

  async invoke<T>(command: string, args: Record<string, unknown>): Promise<T> {
    if (command === "maple_api_set_auth") {
      this.setCalls += 1;
      this.commandOrder.push("set:start");
      const request = args.request as {
        userId: string;
        authBundle: string;
      };
      await this.setHook?.();
      const prior = this.nativeSnapshot;
      const unchanged = prior?.userId === request.userId && prior.authBundle === request.authBundle;
      this.nativeSnapshot = {
        userId: request.userId,
        authBundle: request.authBundle,
        nativeInstanceId: "native-instance-1",
        revision: unchanged ? prior.revision : (prior?.revision ?? 0) + 1
      };
      this.commandOrder.push("set:finish");
      return { ...this.nativeSnapshot } as T;
    }
    if (command === "maple_api_get_auth") {
      this.getCalls += 1;
      this.commandOrder.push("get");
      await this.getHook?.();
      if (!this.nativeSnapshot) throw new Error("native auth missing");
      return { ...this.nativeSnapshot } as T;
    }
    if (command === "maple_api_clear_auth") {
      this.clearCalls += 1;
      this.commandOrder.push("clear");
      this.nativeSnapshot = null;
      return undefined as T;
    }
    throw new Error(`Unexpected command: ${command}`);
  }

  async listen(handler: (event: MapleApiAuthChanged) => Promise<void>): Promise<void> {
    if (this.listenFailures > 0) {
      this.listenFailures -= 1;
      throw new Error("listener unavailable");
    }
    this.handler = handler;
  }

  async emit(event: MapleApiAuthChanged): Promise<void> {
    if (!this.handler) throw new Error("listener missing");
    await this.handler(event);
  }

  setNativeRefresh(authBundle: string, revision: number): void {
    if (!this.nativeSnapshot) throw new Error("native auth missing");
    this.nativeSnapshot = {
      userId: this.nativeSnapshot.userId,
      authBundle,
      nativeInstanceId: this.nativeSnapshot.nativeInstanceId,
      revision
    };
  }
}

describe("MapleApiAuthService", () => {
  test("installs once and only pushes the browser bundle after it changes", async () => {
    const bridge = new FakeAuthBridge();
    const service = new MapleApiAuthService(bridge);

    await service.activate("user-a");
    await service.sync("user-a");
    expect(bridge.setCalls).toBe(1);

    bridge.browserBundle = "browser-bundle-refreshed";
    await service.sync("user-a");
    expect(bridge.setCalls).toBe(2);
    expect(bridge.nativeSnapshot?.authBundle).toBe("browser-bundle-refreshed");
    expect(bridge.metadata?.nativeRevision).toBe(2);
  });

  test("reconciles an SDK-refreshed opaque bundle back to the browser", async () => {
    const bridge = new FakeAuthBridge();
    const service = new MapleApiAuthService(bridge);
    await service.activate("user-a");

    bridge.setNativeRefresh("native-bundle-refreshed", 2);
    await bridge.emit({ userId: "user-a", revision: 2, authenticated: true });

    expect(bridge.browserBundle).toBe("native-bundle-refreshed");
    expect(bridge.metadata?.nativeRevision).toBe(2);
  });

  test("a new service recovers a missed native refresh using durable revision metadata", async () => {
    const bridge = new FakeAuthBridge();
    const firstService = new MapleApiAuthService(bridge);
    await firstService.activate("user-a");

    bridge.setNativeRefresh("native-bundle-refreshed", 2);
    const reloadedService = new MapleApiAuthService(bridge);
    await reloadedService.activate("user-a");

    expect(bridge.browserBundle).toBe("native-bundle-refreshed");
    expect(bridge.setCalls).toBe(1);
    expect(bridge.metadata?.nativeRevision).toBe(2);
  });

  test("does not overwrite a browser refresh with a late native notification", async () => {
    const bridge = new FakeAuthBridge();
    const service = new MapleApiAuthService(bridge);
    await service.activate("user-a");

    bridge.browserBundle = "browser-bundle-won";
    bridge.setNativeRefresh("late-native-bundle", 2);
    await bridge.emit({ userId: "user-a", revision: 2, authenticated: true });

    expect(bridge.setCalls).toBe(2);
    expect(bridge.browserBundle).toBe("browser-bundle-won");
    expect(bridge.nativeSnapshot?.authBundle).toBe("browser-bundle-won");
  });

  test("a browser refresh during get_auth is reinstalled instead of overwritten", async () => {
    const bridge = new FakeAuthBridge();
    const service = new MapleApiAuthService(bridge);
    await service.activate("user-a");
    bridge.setNativeRefresh("native-bundle-late", 2);
    bridge.getHook = async () => {
      bridge.getHook = null;
      bridge.browserBundle = "browser-bundle-new";
    };

    await bridge.emit({ userId: "user-a", revision: 2, authenticated: true });

    expect(bridge.browserBundle).toBe("browser-bundle-new");
    expect(bridge.nativeSnapshot?.authBundle).toBe("browser-bundle-new");
    expect(bridge.setCalls).toBe(2);
  });

  test("a browser refresh during set_auth is installed before sync resolves", async () => {
    const bridge = new FakeAuthBridge();
    const service = new MapleApiAuthService(bridge);
    await service.activate("user-a");
    bridge.browserBundle = "browser-bundle-second";
    bridge.setHook = async () => {
      bridge.setHook = null;
      bridge.browserBundle = "browser-bundle-third";
    };

    await service.sync("user-a");

    expect(bridge.setCalls).toBe(3);
    expect(bridge.nativeSnapshot?.authBundle).toBe("browser-bundle-third");
  });

  test("serialized clear cannot be undone by a delayed credential install", async () => {
    const bridge = new FakeAuthBridge();
    const service = new MapleApiAuthService(bridge);
    const setStarted = deferred();
    const releaseSet = deferred();
    bridge.setHook = async () => {
      setStarted.resolve();
      await releaseSet.promise;
    };

    const activation = service.activate("user-a");
    await setStarted.promise;
    const clearing = service.clear("user-a");
    releaseSet.resolve();
    await Promise.all([activation, clearing]);

    expect(bridge.commandOrder).toEqual(["get", "set:start", "set:finish", "clear"]);
    expect(bridge.nativeSnapshot).toBeNull();
    expect(bridge.metadata).toBeNull();

    bridge.setHook = null;
    bridge.browserBundle = "account-b-bundle";
    await service.activate("user-b");
    expect(bridge.nativeSnapshot?.userId).toBe("user-b");
  });

  test("clearing an account makes its late refresh notification inert", async () => {
    const bridge = new FakeAuthBridge();
    const service = new MapleApiAuthService(bridge);
    await service.activate("user-a");
    const original = bridge.browserBundle;

    await service.clear("user-a");
    await bridge.emit({ userId: "user-a", revision: 2, authenticated: true });

    expect(bridge.clearCalls).toBe(1);
    expect(bridge.browserBundle).toBe(original);
    expect(bridge.nativeSnapshot).toBeNull();
  });

  test("a transient listener failure can be retried without reloading", async () => {
    const bridge = new FakeAuthBridge();
    bridge.listenFailures = 1;
    const service = new MapleApiAuthService(bridge);

    await expect(service.activate("user-a")).rejects.toThrow("listener unavailable");
    await service.activate("user-a");

    expect(bridge.nativeSnapshot?.userId).toBe("user-a");
  });

  test("native credential rejection clears matching auth and notifies UI lifecycle", async () => {
    const bridge = new FakeAuthBridge();
    const service = new MapleApiAuthService(bridge);
    const invalidated: string[] = [];
    service.subscribeInvalidation(({ userId }) => invalidated.push(userId));
    await service.activate("user-a");

    await bridge.emit({ userId: "user-a", revision: 2, authenticated: false });

    expect(bridge.clearCalls).toBe(1);
    expect(bridge.nativeSnapshot).toBeNull();
    expect(bridge.metadata).toBeNull();
    expect(invalidated).toEqual(["user-a"]);
    await expect(service.sync("user-a")).rejects.toThrow("authentication changed");
  });

  test("an invalidation for another account cannot clear the active lifecycle", async () => {
    const bridge = new FakeAuthBridge();
    const service = new MapleApiAuthService(bridge);
    const invalidated: string[] = [];
    service.subscribeInvalidation(({ userId }) => invalidated.push(userId));
    await service.activate("user-a");

    await bridge.emit({ userId: "user-b", revision: 2, authenticated: false });

    expect(bridge.clearCalls).toBe(0);
    expect(bridge.nativeSnapshot?.userId).toBe("user-a");
    expect(invalidated).toEqual([]);
  });

  test("native rejection cannot clear a newer browser credential generation", async () => {
    const bridge = new FakeAuthBridge();
    const service = new MapleApiAuthService(bridge);
    const invalidated: string[] = [];
    service.subscribeInvalidation(({ userId }) => invalidated.push(userId));
    await service.activate("user-a");
    bridge.browserBundle = "browser-bundle-newer";

    await bridge.emit({ userId: "user-a", revision: 2, authenticated: false });

    expect(bridge.clearCalls).toBe(0);
    expect(bridge.setCalls).toBe(2);
    expect(bridge.nativeSnapshot?.authBundle).toBe("browser-bundle-newer");
    expect(invalidated).toEqual([]);
  });
});
