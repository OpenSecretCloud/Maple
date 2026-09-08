import { describe, expect, test } from "bun:test";
import {
  MapleApiAuthService,
  type BrowserTokenPair,
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
  browserTokens: BrowserTokenPair = {
    accessToken: "access-one",
    refreshToken: "refresh-one"
  };
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

  readTokens(): BrowserTokenPair {
    return { ...this.browserTokens };
  }

  writeTokens(tokens: BrowserTokenPair): void {
    this.browserTokens = { ...tokens };
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
        accessToken: string;
        refreshToken: string | null;
      };
      await this.setHook?.();
      const prior = this.nativeSnapshot;
      const unchanged =
        prior?.userId === request.userId &&
        prior.accessToken === request.accessToken &&
        (prior.refreshToken || null) === request.refreshToken;
      this.nativeSnapshot = {
        userId: request.userId,
        accessToken: request.accessToken,
        refreshToken: request.refreshToken,
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

  setNativeRefresh(tokens: BrowserTokenPair, revision: number): void {
    if (!this.nativeSnapshot) throw new Error("native auth missing");
    this.nativeSnapshot = {
      userId: this.nativeSnapshot.userId,
      accessToken: tokens.accessToken,
      refreshToken: tokens.refreshToken,
      nativeInstanceId: this.nativeSnapshot.nativeInstanceId,
      revision
    };
  }
}

describe("MapleApiAuthService", () => {
  test("installs once and only pushes browser credentials after they change", async () => {
    const bridge = new FakeAuthBridge();
    const service = new MapleApiAuthService(bridge);

    await service.activate("user-a");
    await service.sync("user-a");
    expect(bridge.setCalls).toBe(1);

    bridge.browserTokens = {
      accessToken: "browser-refreshed",
      refreshToken: "browser-refresh-token"
    };
    await service.sync("user-a");
    expect(bridge.setCalls).toBe(2);
    expect(bridge.nativeSnapshot?.accessToken).toBe("browser-refreshed");
    expect(bridge.metadata?.nativeRevision).toBe(2);
  });

  test("reconciles an SDK-refreshed token pair back to the browser", async () => {
    const bridge = new FakeAuthBridge();
    const service = new MapleApiAuthService(bridge);
    await service.activate("user-a");

    bridge.setNativeRefresh(
      { accessToken: "native-refreshed", refreshToken: "native-refresh-token" },
      2
    );
    await bridge.emit({ userId: "user-a", revision: 2 });

    expect(bridge.browserTokens).toEqual({
      accessToken: "native-refreshed",
      refreshToken: "native-refresh-token"
    });
    expect(bridge.metadata?.nativeRevision).toBe(2);
  });

  test("a new service recovers a missed native refresh using durable revision metadata", async () => {
    const bridge = new FakeAuthBridge();
    const firstService = new MapleApiAuthService(bridge);
    await firstService.activate("user-a");

    bridge.setNativeRefresh(
      { accessToken: "native-refreshed", refreshToken: "native-refresh-token" },
      2
    );
    const reloadedService = new MapleApiAuthService(bridge);
    await reloadedService.activate("user-a");

    expect(bridge.browserTokens).toEqual({
      accessToken: "native-refreshed",
      refreshToken: "native-refresh-token"
    });
    expect(bridge.setCalls).toBe(1);
    expect(bridge.metadata?.nativeRevision).toBe(2);
  });

  test("does not overwrite a browser refresh with a late native notification", async () => {
    const bridge = new FakeAuthBridge();
    const service = new MapleApiAuthService(bridge);
    await service.activate("user-a");

    bridge.browserTokens = {
      accessToken: "browser-won",
      refreshToken: "browser-won-refresh"
    };
    bridge.setNativeRefresh({ accessToken: "late-native", refreshToken: "late-native-refresh" }, 2);
    await bridge.emit({ userId: "user-a", revision: 2 });

    expect(bridge.setCalls).toBe(2);
    expect(bridge.browserTokens.accessToken).toBe("browser-won");
    expect(bridge.nativeSnapshot?.accessToken).toBe("browser-won");
  });

  test("a browser refresh during get_auth is reinstalled instead of overwritten", async () => {
    const bridge = new FakeAuthBridge();
    const service = new MapleApiAuthService(bridge);
    await service.activate("user-a");
    bridge.setNativeRefresh({ accessToken: "native-late", refreshToken: "native-late-refresh" }, 2);
    bridge.getHook = async () => {
      bridge.getHook = null;
      bridge.browserTokens = {
        accessToken: "browser-new",
        refreshToken: "browser-new-refresh"
      };
    };

    await bridge.emit({ userId: "user-a", revision: 2 });

    expect(bridge.browserTokens.accessToken).toBe("browser-new");
    expect(bridge.nativeSnapshot?.accessToken).toBe("browser-new");
    expect(bridge.setCalls).toBe(2);
  });

  test("a browser refresh during set_auth is installed before sync resolves", async () => {
    const bridge = new FakeAuthBridge();
    const service = new MapleApiAuthService(bridge);
    await service.activate("user-a");
    bridge.browserTokens = {
      accessToken: "browser-second",
      refreshToken: "browser-second-refresh"
    };
    bridge.setHook = async () => {
      bridge.setHook = null;
      bridge.browserTokens = {
        accessToken: "browser-third",
        refreshToken: "browser-third-refresh"
      };
    };

    await service.sync("user-a");

    expect(bridge.setCalls).toBe(3);
    expect(bridge.nativeSnapshot?.accessToken).toBe("browser-third");
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
    bridge.browserTokens = { accessToken: "account-b", refreshToken: "account-b-refresh" };
    await service.activate("user-b");
    expect(bridge.nativeSnapshot?.userId).toBe("user-b");
  });

  test("clearing an account makes its late refresh notification inert", async () => {
    const bridge = new FakeAuthBridge();
    const service = new MapleApiAuthService(bridge);
    await service.activate("user-a");
    const original = { ...bridge.browserTokens };

    await service.clear("user-a");
    await bridge.emit({ userId: "user-a", revision: 2 });

    expect(bridge.clearCalls).toBe(1);
    expect(bridge.browserTokens).toEqual(original);
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
});
