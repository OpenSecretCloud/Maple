import { beforeEach, describe, expect, it } from "bun:test";

import {
  deactivateAgentProxyKeyRegistry,
  manualProxyConfigsMatch,
  ProxyService,
  removeAgentProxyKeyRecord,
  type AgentProxyKeyRegistry,
  type ProxyConfig
} from "./proxyService";

class MemoryStorage implements Storage {
  private values = new Map<string, string>();

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

const testStorage = new MemoryStorage();
Object.defineProperty(globalThis, "localStorage", {
  configurable: true,
  value: testStorage
});

beforeEach(() => {
  testStorage.clear();
});

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

const desiredConfig: ProxyConfig = {
  host: "127.0.0.1",
  port: 37615,
  api_key: "workspace-key",
  enabled: true,
  enable_cors: true,
  backend_url: "http://127.0.0.1:31938",
  auto_start: false
};

describe("manualProxyConfigsMatch", () => {
  it("requires the native process to be running with the requested durable config", () => {
    expect(
      manualProxyConfigsMatch(
        {
          ...desiredConfig,
          host: "127.0.0.1",
          api_key: " workspace-key ",
          backend_url: "http://127.0.0.1:31938/"
        },
        desiredConfig
      )
    ).toBe(true);
    expect(manualProxyConfigsMatch({ ...desiredConfig, auto_start: true }, desiredConfig)).toBe(
      false
    );
    expect(manualProxyConfigsMatch({ ...desiredConfig, port: 8080 }, desiredConfig)).toBe(false);
    expect(
      manualProxyConfigsMatch({ ...desiredConfig, api_key: "another-key" }, desiredConfig)
    ).toBe(false);
    expect(
      manualProxyConfigsMatch(
        { ...desiredConfig, backend_url: "https://enclave.trymaple.ai" },
        desiredConfig
      )
    ).toBe(false);
  });

  it("treats an omitted CORS setting as the secure disabled default", () => {
    expect(
      manualProxyConfigsMatch(
        { ...desiredConfig, enable_cors: undefined },
        { ...desiredConfig, enable_cors: false }
      )
    ).toBe(true);
    expect(
      manualProxyConfigsMatch(
        { ...desiredConfig, enable_cors: undefined },
        { ...desiredConfig, enable_cors: true }
      )
    ).toBe(false);
  });
});

describe("Agent proxy key registry", () => {
  it("removes only the exact revoked key and preserves other devices/accounts", () => {
    const registry: AgentProxyKeyRegistry = {
      keys: [
        { userId: "user-a", name: "maple-agent-local" },
        { userId: "user-a", name: "maple-agent-other-device" },
        { userId: "user-b", name: "maple-agent-user-b" }
      ],
      activeName: "maple-agent-local"
    };

    expect(removeAgentProxyKeyRecord(registry, "maple-agent-local")).toEqual({
      keys: [
        { userId: "user-a", name: "maple-agent-other-device" },
        { userId: "user-b", name: "maple-agent-user-b" }
      ],
      activeName: undefined
    });
  });

  it("detaches a manual proxy config without forgetting the tracked key", () => {
    const registry: AgentProxyKeyRegistry = {
      keys: [{ userId: "user-a", name: "maple-agent-local" }],
      activeName: "maple-agent-local"
    };

    expect(deactivateAgentProxyKeyRegistry(registry)).toEqual({
      keys: [{ userId: "user-a", name: "maple-agent-local" }],
      activeName: undefined
    });
  });
});

describe("manual proxy authentication lifecycle", () => {
  it("lets logout scrub native state while API key creation is still delayed", async () => {
    const createEntered = deferred<void>();
    const createdKey = deferred<string>();
    const deletedNames: string[] = [];
    const commands: string[] = [];
    const service = new ProxyService(
      async <T>(command: string, args?: Record<string, unknown>) => {
        commands.push(command);
        if (command === "start_proxy") {
          const config = args?.config as ProxyConfig;
          return { running: true, config } as T;
        }
        if (command === "stop_and_reset_proxy") {
          return { running: false, config: { ...desiredConfig, api_key: "", enabled: false } } as T;
        }
        throw new Error(`Unexpected proxy command: ${command}`);
      },
      () => true
    );

    await service.transitionAuthenticatedUser(null);
    commands.length = 0;
    await service.transitionAuthenticatedUser("user-a");
    const start = service.startManualProxy(
      "user-a",
      { ...desiredConfig, api_key: "" },
      {
        name: "maple-desktop-delayed",
        createApiKey: async () => {
          createEntered.resolve();
          return await createdKey.promise;
        },
        deleteApiKey: async (name) => {
          deletedNames.push(name);
        },
        refreshApiKeys: async () => {}
      }
    );

    await createEntered.promise;
    await service.stopAndResetProxy("user-a", async (name) => {
      deletedNames.push(name);
    });
    expect(commands.filter((command) => command === "stop_and_reset_proxy")).toHaveLength(1);

    await service.transitionAuthenticatedUser(null);
    createdKey.resolve("new-secret-key");

    await expect(start).rejects.toThrow(
      "The authenticated Maple account changed before the local proxy finished starting"
    );

    expect(commands).not.toContain("start_proxy");
    expect(commands.filter((command) => command === "stop_and_reset_proxy")).toHaveLength(2);
    expect(deletedNames).toEqual([]);
    expect(JSON.parse(localStorage.getItem("maple-pending-manual-proxy-keys-v1") || "[]")).toEqual([
      { userId: "user-a", name: "maple-desktop-delayed" }
    ]);

    await service.transitionAuthenticatedUser("user-a");
    await service.cleanupPendingManualProxyKeys("user-a", async (name) => {
      deletedNames.push(name);
    });
    expect(deletedNames).toEqual(["maple-desktop-delayed"]);
    expect(localStorage.getItem("maple-pending-manual-proxy-keys-v1")).toBeNull();
  });

  it("revokes a delayed key while logout still owns the initiating account", async () => {
    const createEntered = deferred<void>();
    const createdKey = deferred<string>();
    const deletedNames: string[] = [];
    const service = new ProxyService(
      async <T>(command: string) => {
        if (command === "stop_and_reset_proxy") {
          return { running: false, config: { ...desiredConfig, api_key: "", enabled: false } } as T;
        }
        throw new Error(`Unexpected proxy command: ${command}`);
      },
      () => true
    );

    await service.transitionAuthenticatedUser(null);
    await service.transitionAuthenticatedUser("user-a");
    const start = service.startManualProxy(
      "user-a",
      { ...desiredConfig, api_key: "" },
      {
        name: "maple-desktop-same-session",
        createApiKey: async () => {
          createEntered.resolve();
          return await createdKey.promise;
        },
        deleteApiKey: async (name) => {
          deletedNames.push(name);
        },
        refreshApiKeys: async () => {}
      }
    );

    await createEntered.promise;
    await service.stopAndResetProxy("user-a");
    createdKey.resolve("new-secret-key");

    await expect(start).rejects.toThrow(
      "The authenticated Maple account changed before the local proxy finished starting"
    );
    expect(deletedNames).toEqual(["maple-desktop-same-session"]);
    expect(localStorage.getItem("maple-pending-manual-proxy-keys-v1")).toBeNull();
  });

  it("preserves an old account's cleanup record instead of deleting through the new account", async () => {
    const refreshEntered = deferred<void>();
    const refreshFinished = deferred<void>();
    const deletedNames: string[] = [];
    const commands: string[] = [];
    const service = new ProxyService(
      async <T>(command: string, args?: Record<string, unknown>) => {
        commands.push(command);
        if (command === "start_proxy") {
          const config = args?.config as ProxyConfig;
          return { running: true, config } as T;
        }
        if (command === "stop_and_reset_proxy") {
          return { running: false, config: { ...desiredConfig, api_key: "", enabled: false } } as T;
        }
        throw new Error(`Unexpected proxy command: ${command}`);
      },
      () => true
    );

    await service.transitionAuthenticatedUser(null);
    commands.length = 0;
    await service.transitionAuthenticatedUser("user-a");
    const start = service.startManualProxy(
      "user-a",
      { ...desiredConfig, api_key: "" },
      {
        name: "maple-desktop-refresh",
        createApiKey: async () => "new-secret-key",
        deleteApiKey: async (name) => {
          deletedNames.push(name);
        },
        refreshApiKeys: async () => {
          refreshEntered.resolve();
          await refreshFinished.promise;
        }
      }
    );

    await refreshEntered.promise;
    const accountTransition = service.transitionAuthenticatedUser("user-b");
    refreshFinished.resolve();

    await expect(start).rejects.toThrow(
      "The authenticated Maple account changed before the local proxy finished starting"
    );
    await accountTransition;

    expect(commands).not.toContain("start_proxy");
    expect(commands.filter((command) => command === "stop_and_reset_proxy")).toHaveLength(1);
    expect(deletedNames).toEqual([]);
    expect(JSON.parse(localStorage.getItem("maple-pending-manual-proxy-keys-v1") || "[]")).toEqual([
      { userId: "user-a", name: "maple-desktop-refresh" }
    ]);
  });

  it("resets a proxy that finishes starting after its account becomes stale", async () => {
    const nativeStartEntered = deferred<void>();
    const nativeStartFinished = deferred<void>();
    const deletedNames: string[] = [];
    const commands: string[] = [];
    const service = new ProxyService(
      async <T>(command: string, args?: Record<string, unknown>) => {
        commands.push(command);
        if (command === "start_proxy") {
          nativeStartEntered.resolve();
          await nativeStartFinished.promise;
          const config = args?.config as ProxyConfig;
          return { running: true, config } as T;
        }
        if (command === "stop_and_reset_proxy") {
          return { running: false, config: { ...desiredConfig, api_key: "", enabled: false } } as T;
        }
        throw new Error(`Unexpected proxy command: ${command}`);
      },
      () => true
    );

    await service.transitionAuthenticatedUser(null);
    commands.length = 0;
    await service.transitionAuthenticatedUser("user-a");
    const start = service.startManualProxy(
      "user-a",
      { ...desiredConfig, api_key: "" },
      {
        name: "maple-desktop-native-start",
        createApiKey: async () => "new-secret-key",
        deleteApiKey: async (name) => {
          deletedNames.push(name);
        },
        refreshApiKeys: async () => {}
      }
    );

    await nativeStartEntered.promise;
    const accountTransition = service.transitionAuthenticatedUser("user-b");
    nativeStartFinished.resolve();

    await expect(start).rejects.toThrow(
      "The authenticated Maple account changed before the local proxy finished starting"
    );
    await accountTransition;

    expect(commands.filter((command) => command === "start_proxy")).toHaveLength(1);
    expect(commands.filter((command) => command === "stop_and_reset_proxy")).toHaveLength(2);
    expect(deletedNames).toEqual([]);
    expect(JSON.parse(localStorage.getItem("maple-pending-manual-proxy-keys-v1") || "[]")).toEqual([
      { userId: "user-a", name: "maple-desktop-native-start" }
    ]);
  });

  it("retries a failed account-transition reset before the new account becomes ready", async () => {
    let resetAttempts = 0;
    const service = new ProxyService(
      async <T>(command: string) => {
        if (command === "load_proxy_config") {
          return { ...desiredConfig, owner_user_id: "user-a" } as T;
        }
        if (command !== "stop_and_reset_proxy") {
          throw new Error(`Unexpected proxy command: ${command}`);
        }
        resetAttempts += 1;
        if (resetAttempts === 1) throw new Error("keyring unavailable");
        return { running: false, config: { ...desiredConfig, api_key: "", enabled: false } } as T;
      },
      () => true
    );

    await service.transitionAuthenticatedUser("user-a");
    await expect(service.transitionAuthenticatedUser("user-b")).rejects.toThrow(
      "keyring unavailable"
    );
    await expect(service.awaitAuthenticatedUser("user-b")).resolves.toBeUndefined();
    expect(resetAttempts).toBe(2);
  });

  it("keeps a matching native owner but scrubs a foreign cold-start owner", async () => {
    const commands: string[] = [];
    const invokeCommand = async <T>(command: string) => {
      commands.push(command);
      if (command === "load_proxy_config") {
        return { ...desiredConfig, owner_user_id: "user-a" } as T;
      }
      return { running: false, config: { ...desiredConfig, api_key: "", enabled: false } } as T;
    };

    const sameOwnerService = new ProxyService(invokeCommand, () => true);
    await sameOwnerService.transitionAuthenticatedUser("user-a");
    expect(commands).toEqual(["load_proxy_config"]);

    commands.length = 0;
    const foreignOwnerService = new ProxyService(invokeCommand, () => true);
    await foreignOwnerService.transitionAuthenticatedUser("user-b");
    expect(commands).toEqual(["load_proxy_config", "stop_and_reset_proxy"]);
  });

  it("auto-starts only after the saved owner is authenticated", async () => {
    const commands: string[] = [];
    const ownedConfig: ProxyConfig = {
      ...desiredConfig,
      api_key: "owned-secret",
      enabled: true,
      auto_start: true,
      owner_user_id: "user-a"
    };
    const service = new ProxyService(
      async <T>(command: string, args?: Record<string, unknown>) => {
        commands.push(command);
        if (command === "load_proxy_config") return ownedConfig as T;
        if (command === "get_proxy_status") {
          return { running: false, config: ownedConfig } as T;
        }
        if (command === "start_proxy") {
          expect(args?.config).toEqual(ownedConfig);
          return { running: true, config: ownedConfig } as T;
        }
        throw new Error(`Unexpected proxy command: ${command}`);
      },
      () => true
    );

    await service.transitionAuthenticatedUser("user-a");
    expect(commands).toEqual(["load_proxy_config", "get_proxy_status", "start_proxy"]);
  });

  it("loads the new account's scrubbed config only after transition cleanup", async () => {
    const resetEntered = deferred<void>();
    const resetFinished = deferred<void>();
    let nativeConfig: ProxyConfig = {
      ...desiredConfig,
      api_key: "user-a-secret",
      enabled: true,
      owner_user_id: "user-a"
    };
    const service = new ProxyService(
      async <T>(command: string) => {
        if (command === "stop_and_reset_proxy") {
          resetEntered.resolve();
          await resetFinished.promise;
          nativeConfig = { ...desiredConfig, api_key: "", enabled: false };
          return { running: false, config: nativeConfig } as T;
        }
        if (command === "load_proxy_config") return nativeConfig as T;
        if (command === "get_proxy_status") {
          return { running: nativeConfig.enabled, config: nativeConfig } as T;
        }
        throw new Error(`Unexpected proxy command: ${command}`);
      },
      () => true
    );

    await service.transitionAuthenticatedUser("user-a");
    const transition = service.transitionAuthenticatedUser("user-b");
    await resetEntered.promise;
    const load = service.loadManualProxyState("user-b");
    resetFinished.resolve();

    await transition;
    const state = await load;
    expect(state.config.api_key).toBe("");
    expect(state.status.running).toBeFalse();
  });
});
