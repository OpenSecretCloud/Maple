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

const inactiveUserAConfig: ProxyConfig = {
  ...desiredConfig,
  api_key: "",
  enabled: false,
  owner_user_id: "user-a"
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
    expect(
      manualProxyConfigsMatch(
        { ...desiredConfig, owner_user_id: "user-a" },
        { ...desiredConfig, owner_user_id: "user-b" }
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
    const commands: string[] = [];
    let retainedKey = "";
    const service = new ProxyService(
      async <T>(command: string) => {
        commands.push(command);
        if (command === "load_proxy_config") return inactiveUserAConfig as T;
        if (command === "stop_and_reset_proxy") {
          return { running: false, config: { ...desiredConfig, api_key: "", enabled: false } } as T;
        }
        throw new Error(`Unexpected proxy command: ${command}`);
      },
      () => true
    );

    await service.transitionAuthenticatedUser(null);
    await service.transitionAuthenticatedUser("user-a");
    commands.length = 0;

    const start = service.startManualProxy(
      "user-a",
      { ...desiredConfig, api_key: "" },
      {
        name: "maple-desktop-delayed",
        createApiKey: async () => {
          createEntered.resolve();
          return await createdKey.promise;
        },
        refreshApiKeys: async () => {},
        onApiKeyCreated: (apiKey) => {
          retainedKey = apiKey;
        }
      }
    );

    await createEntered.promise;
    await service.stopAndResetProxy("user-a");
    await service.transitionAuthenticatedUser(null);
    createdKey.resolve("new-secret-key");

    await expect(start).rejects.toThrow(
      "The authenticated Maple account changed before the local proxy finished starting"
    );
    expect(retainedKey).toBe("new-secret-key");
    expect(commands).not.toContain("start_proxy");
    expect(commands.filter((command) => command === "stop_and_reset_proxy")).toHaveLength(2);
  });

  it("does not start after the account changes during the API-key refresh", async () => {
    const refreshEntered = deferred<void>();
    const refreshFinished = deferred<void>();
    const commands: string[] = [];
    const service = new ProxyService(
      async <T>(command: string) => {
        commands.push(command);
        if (command === "load_proxy_config") return inactiveUserAConfig as T;
        if (command === "stop_and_reset_proxy") {
          return { running: false, config: { ...desiredConfig, api_key: "", enabled: false } } as T;
        }
        throw new Error(`Unexpected proxy command: ${command}`);
      },
      () => true
    );

    await service.transitionAuthenticatedUser("user-a");
    commands.length = 0;
    const start = service.startManualProxy(
      "user-a",
      { ...desiredConfig, api_key: "" },
      {
        name: "maple-desktop-refresh",
        createApiKey: async () => "new-secret-key",
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
  });

  it("resets a proxy that finishes starting after its account becomes stale", async () => {
    const nativeStartEntered = deferred<void>();
    const nativeStartFinished = deferred<void>();
    const commands: string[] = [];
    const service = new ProxyService(
      async <T>(command: string, args?: Record<string, unknown>) => {
        commands.push(command);
        if (command === "load_proxy_config") return inactiveUserAConfig as T;
        if (command === "start_proxy") {
          nativeStartEntered.resolve();
          await nativeStartFinished.promise;
          return { running: true, config: args?.config as ProxyConfig } as T;
        }
        if (command === "stop_and_reset_proxy") {
          return { running: false, config: { ...desiredConfig, api_key: "", enabled: false } } as T;
        }
        throw new Error(`Unexpected proxy command: ${command}`);
      },
      () => true
    );

    await service.transitionAuthenticatedUser("user-a");
    commands.length = 0;
    const start = service.startManualProxy("user-a", desiredConfig);

    await nativeStartEntered.promise;
    const accountTransition = service.transitionAuthenticatedUser("user-b");
    nativeStartFinished.resolve();

    await expect(start).rejects.toThrow(
      "The authenticated Maple account changed before the local proxy finished starting"
    );
    await accountTransition;
    expect(commands.filter((command) => command === "start_proxy")).toHaveLength(1);
    expect(commands.filter((command) => command === "stop_and_reset_proxy")).toHaveLength(2);
  });

  it("preserves a released ownerless config byte-for-byte through login", async () => {
    const commands: string[] = [];
    const legacyConfig: ProxyConfig = {
      ...desiredConfig,
      host: "127.0.0.2",
      port: 38721,
      api_key: "pre-owner-secret",
      enabled: true,
      enable_cors: false,
      backend_url: "https://legacy.example.invalid",
      auto_start: true
    };
    const originalJson = JSON.stringify(legacyConfig);
    const service = new ProxyService(
      async <T>(command: string, args?: Record<string, unknown>) => {
        commands.push(command);
        if (command === "load_proxy_config") return JSON.parse(originalJson) as T;
        if (command === "get_proxy_status") {
          return { running: false, config: JSON.parse(originalJson) } as T;
        }
        if (command === "start_proxy") {
          expect(args?.config).toEqual(legacyConfig);
          return { running: true, config: args?.config as ProxyConfig } as T;
        }
        throw new Error(`Unexpected proxy command: ${command}`);
      },
      () => true
    );

    await service.transitionAuthenticatedUser(null);
    expect(commands).toEqual([]);
    await service.transitionAuthenticatedUser("user-a");

    expect(commands).toEqual(["load_proxy_config", "get_proxy_status", "start_proxy"]);
    expect(commands).not.toContain("save_proxy_settings");
    expect(commands).not.toContain("stop_and_reset_proxy");
    expect(JSON.stringify(legacyConfig)).toBe(originalJson);
    expect(legacyConfig.owner_user_id).toBeUndefined();
  });

  it("preserves a released ownerless config when auth restores before signed-out setup settles", async () => {
    const commands: string[] = [];
    const releasedConfig: ProxyConfig = {
      ...desiredConfig,
      api_key: "released-secret",
      auto_start: true
    };
    const service = new ProxyService(
      async <T>(command: string, args?: Record<string, unknown>) => {
        commands.push(command);
        if (command === "load_proxy_config") return releasedConfig as T;
        if (command === "get_proxy_status") {
          return { running: false, config: releasedConfig } as T;
        }
        if (command === "start_proxy") {
          return { running: true, config: args?.config as ProxyConfig } as T;
        }
        throw new Error(`Unexpected proxy command: ${command}`);
      },
      () => true
    );

    const signedOutSetup = service.transitionAuthenticatedUser(null);
    const restoredAuthSetup = service.transitionAuthenticatedUser("user-a");

    await expect(signedOutSetup).rejects.toThrow(
      "The authenticated Maple account changed before the local proxy finished starting"
    );
    await restoredAuthSetup;
    expect(commands).toEqual(["load_proxy_config", "get_proxy_status", "start_proxy"]);
    expect(commands).not.toContain("stop_and_reset_proxy");
    expect(releasedConfig.owner_user_id).toBeUndefined();
  });

  it("does not duplicate native startup for an already-running ownerless config", async () => {
    const commands: string[] = [];
    const legacyConfig: ProxyConfig = {
      ...desiredConfig,
      api_key: "released-secret",
      auto_start: true
    };
    const service = new ProxyService(
      async <T>(command: string) => {
        commands.push(command);
        if (command === "load_proxy_config") return legacyConfig as T;
        if (command === "get_proxy_status") return { running: true, config: legacyConfig } as T;
        throw new Error(`Unexpected proxy command: ${command}`);
      },
      () => true
    );

    await service.transitionAuthenticatedUser("user-a");
    expect(commands).toEqual(["load_proxy_config", "get_proxy_status"]);
    expect(legacyConfig.owner_user_id).toBeUndefined();
  });

  it("leaves an account-owned config untouched while signed out", async () => {
    const commands: string[] = [];
    const service = new ProxyService(
      async (command: string) => {
        commands.push(command);
        throw new Error(`Unexpected proxy command: ${command}`);
      },
      () => true
    );

    await service.transitionAuthenticatedUser(null);
    expect(commands).toEqual([]);
  });

  it("auto-starts an owned config only for its matching account", async () => {
    const commands: string[] = [];
    const ownedConfig: ProxyConfig = {
      ...desiredConfig,
      api_key: "owned-secret",
      auto_start: true,
      owner_user_id: "user-a"
    };
    const service = new ProxyService(
      async <T>(command: string, args?: Record<string, unknown>) => {
        commands.push(command);
        if (command === "load_proxy_config") return ownedConfig as T;
        if (command === "get_proxy_status") return { running: false, config: ownedConfig } as T;
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

  it("scrubs a foreign owned config and retries only the failed reset", async () => {
    const commands: string[] = [];
    let resetAttempts = 0;
    const foreignConfig: ProxyConfig = {
      ...desiredConfig,
      api_key: "user-a-secret",
      owner_user_id: "user-a"
    };
    const service = new ProxyService(
      async <T>(command: string) => {
        commands.push(command);
        if (command === "load_proxy_config") return foreignConfig as T;
        if (command === "stop_and_reset_proxy") {
          resetAttempts += 1;
          if (resetAttempts === 1) throw new Error("keyring unavailable");
          return { running: false, config: { ...desiredConfig, api_key: "", enabled: false } } as T;
        }
        throw new Error(`Unexpected proxy command: ${command}`);
      },
      () => true
    );

    await expect(service.transitionAuthenticatedUser("user-b")).rejects.toThrow(
      "keyring unavailable"
    );
    await expect(service.awaitAuthenticatedUser("user-b")).resolves.toBeUndefined();

    expect(commands).toEqual(["load_proxy_config", "stop_and_reset_proxy", "stop_and_reset_proxy"]);
  });

  it("finishes a failed signed-out reset before activating the next account", async () => {
    const commands: string[] = [];
    let resetAttempts = 0;
    const releasedConfig: ProxyConfig = {
      ...desiredConfig,
      api_key: "user-a-secret"
    };
    const service = new ProxyService(
      async <T>(command: string) => {
        commands.push(command);
        if (command === "load_proxy_config") return releasedConfig as T;
        if (command === "stop_and_reset_proxy") {
          resetAttempts += 1;
          if (resetAttempts === 1) throw new Error("keyring unavailable");
          return { running: false, config: { ...desiredConfig, api_key: "", enabled: false } } as T;
        }
        throw new Error(`Unexpected proxy command: ${command}`);
      },
      () => true
    );

    await service.transitionAuthenticatedUser("user-a");
    await expect(service.transitionAuthenticatedUser(null)).rejects.toThrow("keyring unavailable");
    await expect(service.transitionAuthenticatedUser("user-b")).resolves.toBeUndefined();

    expect(commands).toEqual(["load_proxy_config", "stop_and_reset_proxy", "stop_and_reset_proxy"]);
    expect(commands).not.toContain("start_proxy");
  });

  it("fails open for the UI on a config read error but keeps proxy operations fenced", async () => {
    const commands: string[] = [];
    const service = new ProxyService(
      async (command: string) => {
        commands.push(command);
        if (command === "load_proxy_config") throw new Error("keyring unavailable");
        throw new Error(`Unexpected proxy command: ${command}`);
      },
      () => true
    );

    await expect(service.transitionAuthenticatedUser("user-a")).resolves.toBeUndefined();
    await expect(service.awaitAuthenticatedUser("user-a")).rejects.toThrow(
      "The authenticated Maple account changed before the local proxy finished starting"
    );
    expect(commands).toEqual(["load_proxy_config", "load_proxy_config"]);
    expect(commands).not.toContain("stop_and_reset_proxy");
  });

  it("reconciles a foreign config after a transient ownership read failure", async () => {
    const commands: string[] = [];
    let loadAttempts = 0;
    let nativeConfig: ProxyConfig = {
      ...desiredConfig,
      api_key: "user-b-secret",
      owner_user_id: "user-b"
    };
    const service = new ProxyService(
      async <T>(command: string) => {
        commands.push(command);
        if (command === "load_proxy_config") {
          loadAttempts += 1;
          if (loadAttempts === 1) throw new Error("transient keyring failure");
          return nativeConfig as T;
        }
        if (command === "stop_and_reset_proxy") {
          nativeConfig = { ...desiredConfig, api_key: "", enabled: false };
          return { running: false, config: nativeConfig } as T;
        }
        if (command === "get_proxy_status") {
          return { running: false, config: nativeConfig } as T;
        }
        throw new Error(`Unexpected proxy command: ${command}`);
      },
      () => true
    );

    await expect(service.transitionAuthenticatedUser("user-a")).resolves.toBeUndefined();
    const state = await service.loadManualProxyState("user-a");

    expect(state.config.api_key).toBe("");
    expect(state.config.owner_user_id).toBeUndefined();
    expect(commands).toEqual([
      "load_proxy_config",
      "load_proxy_config",
      "stop_and_reset_proxy",
      "load_proxy_config",
      "get_proxy_status"
    ]);
    expect(commands).not.toContain("start_proxy");
  });

  it("stamps ownership only on an explicit authenticated manual start", async () => {
    const commands: string[] = [];
    const service = new ProxyService(
      async <T>(command: string, args?: Record<string, unknown>) => {
        commands.push(command);
        if (command === "load_proxy_config") return inactiveUserAConfig as T;
        if (command === "start_proxy") {
          const config = args?.config as ProxyConfig;
          expect(config.owner_user_id).toBe("user-a");
          return { running: true, config } as T;
        }
        throw new Error(`Unexpected proxy command: ${command}`);
      },
      () => true
    );

    await service.transitionAuthenticatedUser("user-a");
    const status = await service.startManualProxy("user-a", desiredConfig);

    expect(status.config.owner_user_id).toBe("user-a");
    expect(commands).toEqual(["load_proxy_config", "start_proxy"]);
  });

  it("keeps a successful durable start when legacy WebView metadata is unavailable", async () => {
    const commands: string[] = [];
    const unavailableStorage = new MemoryStorage();
    unavailableStorage.removeItem = () => {
      throw new Error("local storage unavailable");
    };
    Object.defineProperty(globalThis, "localStorage", {
      configurable: true,
      value: unavailableStorage
    });

    try {
      const service = new ProxyService(
        async <T>(command: string, args?: Record<string, unknown>) => {
          commands.push(command);
          if (command === "load_proxy_config") return inactiveUserAConfig as T;
          if (command === "start_proxy") {
            return { running: true, config: args?.config as ProxyConfig } as T;
          }
          throw new Error(`Unexpected proxy command: ${command}`);
        },
        () => true
      );

      await service.transitionAuthenticatedUser("user-a");
      await expect(service.startManualProxy("user-a", desiredConfig)).resolves.toMatchObject({
        running: true
      });
      expect(commands).toEqual(["load_proxy_config", "start_proxy"]);
      expect(commands).not.toContain("stop_and_reset_proxy");
    } finally {
      Object.defineProperty(globalThis, "localStorage", {
        configurable: true,
        value: testStorage
      });
    }
  });

  it("retains a newly-created key for retry when refresh fails", async () => {
    let retainedKey = "";
    const commands: string[] = [];
    const service = new ProxyService(
      async <T>(command: string) => {
        commands.push(command);
        if (command === "load_proxy_config") return inactiveUserAConfig as T;
        throw new Error(`Unexpected proxy command: ${command}`);
      },
      () => true
    );

    await service.transitionAuthenticatedUser("user-a");
    await expect(
      service.startManualProxy(
        "user-a",
        { ...desiredConfig, api_key: "" },
        {
          name: "maple-desktop-20260728",
          createApiKey: async () => "new-secret-key",
          refreshApiKeys: async () => {
            throw new Error("refresh failed");
          },
          onApiKeyCreated: (apiKey) => {
            retainedKey = apiKey;
          }
        }
      )
    ).rejects.toThrow("refresh failed");

    expect(retainedKey).toBe("new-secret-key");
    expect(commands).toEqual(["load_proxy_config"]);
  });

  it("stamps ownership only on an explicit authenticated settings save", async () => {
    const commands: string[] = [];
    const service = new ProxyService(
      async <T>(command: string, args?: Record<string, unknown>) => {
        commands.push(command);
        if (command === "load_proxy_config") return inactiveUserAConfig as T;
        if (command === "save_proxy_settings") {
          expect((args?.config as ProxyConfig).owner_user_id).toBe("user-a");
          return undefined as T;
        }
        throw new Error(`Unexpected proxy command: ${command}`);
      },
      () => true
    );

    await service.transitionAuthenticatedUser("user-a");
    await service.saveManualProxySettings("user-a", desiredConfig);
    expect(commands).toEqual(["load_proxy_config", "save_proxy_settings"]);
  });

  it("retries a failed explicit reset without reinitializing the account", async () => {
    const commands: string[] = [];
    let resetAttempts = 0;
    const service = new ProxyService(
      async <T>(command: string) => {
        commands.push(command);
        if (command === "load_proxy_config") return inactiveUserAConfig as T;
        if (command === "stop_and_reset_proxy") {
          resetAttempts += 1;
          if (resetAttempts === 1) throw new Error("keyring unavailable");
          return { running: false, config: { ...desiredConfig, api_key: "", enabled: false } } as T;
        }
        throw new Error(`Unexpected proxy command: ${command}`);
      },
      () => true
    );

    await service.transitionAuthenticatedUser("user-a");
    await expect(service.stopAndResetProxy("user-a")).rejects.toThrow("keyring unavailable");
    await expect(service.awaitAuthenticatedUser("user-a")).resolves.toBeUndefined();

    expect(resetAttempts).toBe(2);
    expect(commands).toEqual(["load_proxy_config", "stop_and_reset_proxy", "stop_and_reset_proxy"]);
  });

  it("preserves released Agent API-key cleanup on logout", async () => {
    localStorage.setItem(
      "maple-agent-proxy-keys-v1",
      JSON.stringify({
        keys: [
          { userId: "user-a", name: "maple-agent-user-a" },
          { userId: "user-b", name: "maple-agent-user-b" }
        ],
        activeName: "maple-agent-user-a"
      })
    );
    const deletedNames: string[] = [];
    const service = new ProxyService(
      async <T>(command: string) => {
        if (command === "load_proxy_config") return inactiveUserAConfig as T;
        if (command === "stop_and_reset_proxy") {
          return { running: false, config: { ...desiredConfig, api_key: "", enabled: false } } as T;
        }
        throw new Error(`Unexpected proxy command: ${command}`);
      },
      () => true
    );

    await service.transitionAuthenticatedUser("user-a");
    await service.stopAndResetProxy("user-a", async (name) => {
      deletedNames.push(name);
    });
    await Promise.resolve();

    expect(deletedNames).toEqual(["maple-agent-user-a"]);
    expect(JSON.parse(localStorage.getItem("maple-agent-proxy-keys-v1") || "{}")).toEqual({
      keys: [{ userId: "user-b", name: "maple-agent-user-b" }]
    });
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
        if (command === "load_proxy_config") return nativeConfig as T;
        if (command === "stop_and_reset_proxy") {
          resetEntered.resolve();
          await resetFinished.promise;
          nativeConfig = { ...desiredConfig, api_key: "", enabled: false };
          return { running: false, config: nativeConfig } as T;
        }
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
    expect(state.status.running).toBe(false);
  });
});
