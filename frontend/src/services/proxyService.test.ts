import { describe, expect, it } from "bun:test";

import {
  deactivateAgentProxyKeyRegistry,
  manualProxyConfigsMatch,
  ProxyAuthenticationChangedError,
  ProxyService,
  removeAgentProxyKeyRecord,
  type AgentProxyKeyRegistry,
  type ProxyConfig
} from "./proxyService";

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

describe("manual proxy authentication fence", () => {
  it("does not start after logout while API-key creation is delayed", async () => {
    const keyCreationStarted = deferred<void>();
    const createdKey = deferred<string>();
    const commands: string[] = [];
    const service = new ProxyService(
      async <T>(command: string): Promise<T> => {
        commands.push(command);
        throw new Error(`Unexpected command: ${command}`);
      },
      () => true
    );
    service.observeAuthenticatedUser("user-a");

    const start = service.startManualProxy(
      "user-a",
      { ...desiredConfig, api_key: "" },
      {
        name: "maple-desktop-test",
        createApiKey: async () => {
          keyCreationStarted.resolve();
          return await createdKey.promise;
        }
      }
    );

    await keyCreationStarted.promise;
    service.observeAuthenticatedUser(null);
    createdKey.resolve("new-user-a-key");

    await expect(start).rejects.toBeInstanceOf(ProxyAuthenticationChangedError);
    expect(commands).toEqual([]);
  });

  it("scrubs a proxy that finishes starting after an account transition", async () => {
    const nativeStartInvoked = deferred<void>();
    const nativeStart = deferred<{ running: boolean; config: ProxyConfig }>();
    const commands: string[] = [];
    const service = new ProxyService(
      async <T>(command: string): Promise<T> => {
        commands.push(command);
        if (command === "start_proxy") {
          nativeStartInvoked.resolve();
          return (await nativeStart.promise) as T;
        }
        if (command === "stop_and_reset_proxy") {
          return { running: false, config: desiredConfig } as T;
        }
        throw new Error(`Unexpected command: ${command}`);
      },
      () => true
    );
    service.observeAuthenticatedUser("user-a");

    const start = service.startManualProxy("user-a", desiredConfig);
    await nativeStartInvoked.promise;
    service.observeAuthenticatedUser("user-b");
    nativeStart.resolve({ running: true, config: desiredConfig });

    await expect(start).rejects.toBeInstanceOf(ProxyAuthenticationChangedError);
    expect(commands).toEqual(["start_proxy", "stop_and_reset_proxy"]);
  });

  it("invalidates delayed startup before serialized logout cleanup", async () => {
    const keyCreationStarted = deferred<void>();
    const createdKey = deferred<string>();
    const commands: string[] = [];
    const service = new ProxyService(
      async <T>(command: string): Promise<T> => {
        commands.push(command);
        if (command === "stop_and_reset_proxy") {
          return { running: false, config: desiredConfig } as T;
        }
        throw new Error(`Unexpected command: ${command}`);
      },
      () => true
    );
    service.observeAuthenticatedUser("user-a");

    const start = service.startManualProxy(
      "user-a",
      { ...desiredConfig, api_key: "" },
      {
        name: "maple-desktop-test",
        createApiKey: async () => {
          keyCreationStarted.resolve();
          return await createdKey.promise;
        }
      }
    );
    await keyCreationStarted.promise;

    const reset = service.stopAndResetProxy();
    createdKey.resolve("new-user-a-key");

    await expect(start).rejects.toBeInstanceOf(ProxyAuthenticationChangedError);
    await reset;
    expect(commands).toEqual(["stop_and_reset_proxy"]);
  });

  it("observes login without reading or rewriting existing proxy settings", () => {
    const commands: string[] = [];
    const service = new ProxyService(async <T>(command: string): Promise<T> => {
      commands.push(command);
      throw new Error(`Unexpected command: ${command}`);
    });

    service.observeAuthenticatedUser("existing-user");

    expect(commands).toEqual([]);
  });
});
