import { describe, expect, test } from "bun:test";
import {
  AgentRuntimeStopCoordinator,
  AgentRuntimeService,
  type AgentRuntimeBridge,
  type AgentRuntimeStopBridge,
  type AgentCreateSessionRequest,
  type AgentSendMessageRequest
} from "./agentRuntimeService";
import type { AgentOperationBlock } from "./agentOperationFence";

class RecordingBridge implements AgentRuntimeBridge {
  readonly events: string[] = [];
  lastArgs: Record<string, unknown> | undefined;

  async syncAuth(userId: string): Promise<void> {
    this.events.push(`sync:${userId}`);
  }

  async runForUser<T>(userId: string, operation: () => Promise<T>): Promise<T> {
    this.events.push(`fence:${userId}`);
    return await operation();
  }

  async invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    this.events.push(`invoke:${command}`);
    this.lastArgs = args;
    return undefined as T;
  }
}

describe("AgentRuntimeService", () => {
  test("cancellation stays account-fenced without waiting for remote auth sync", async () => {
    const bridge = new RecordingBridge();
    const service = new AgentRuntimeService(bridge);

    await service.cancelRun("user-a", "run-1");

    expect(bridge.events).toEqual(["fence:user-a", "invoke:agent_cancel_run"]);
    expect(bridge.lastArgs).toEqual({ userId: "user-a", runId: "run-1" });
  });

  test("backend-dependent operations still synchronize credentials inside the fence", async () => {
    const bridge = new RecordingBridge();
    const service = new AgentRuntimeService(bridge);
    const request: AgentSendMessageRequest = {
      sessionId: "session-1",
      text: "hello",
      contextLimit: 384_000,
      visionCapable: false
    };

    await service.sendMessage("user-a", request);

    expect(bridge.events).toEqual(["fence:user-a", "sync:user-a", "invoke:agent_send_message"]);
    expect(bridge.lastArgs).toEqual({ userId: "user-a", request });
  });

  test("session creation forwards the selected model context limit", async () => {
    const bridge = new RecordingBridge();
    const service = new AgentRuntimeService(bridge);
    const request: AgentCreateSessionRequest = {
      projectRoot: "/tmp/project",
      model: "kimi-k2-6",
      contextLimit: 256_000
    };

    await service.createSession("user-a", request);

    expect(bridge.events).toEqual(["fence:user-a", "sync:user-a", "invoke:agent_create_session"]);
    expect(bridge.lastArgs).toEqual({ userId: "user-a", request });
  });
});

class RecordingStopBridge implements AgentRuntimeStopBridge {
  readonly events: string[] = [];
  readonly block: AgentOperationBlock = {
    release: () => this.events.push("release"),
    retainUntilNextSession: () => this.events.push("retain")
  };
  acpError: Error | null = null;

  async blockAndDrain(userId: string): Promise<AgentOperationBlock> {
    this.events.push(`block:${userId}`);
    return this.block;
  }

  async stopAcp(userId: string): Promise<void> {
    this.events.push(`stop-acp:${userId}`);
    if (this.acpError) throw this.acpError;
  }

  async stopRuntime(userId: string): Promise<void> {
    this.events.push(`stop-runtime:${userId}`);
  }
}

describe("AgentRuntimeStopCoordinator", () => {
  test("drains work and stops ACP before the Agent runtime", async () => {
    const bridge = new RecordingStopBridge();
    const coordinator = new AgentRuntimeStopCoordinator(bridge);

    expect(await coordinator.stop("user-a")).toBe(bridge.block);
    expect(bridge.events).toEqual(["block:user-a", "stop-acp:user-a", "stop-runtime:user-a"]);
  });

  test("an ACP stop failure prevents runtime shutdown and releases the cleanup lease", async () => {
    const bridge = new RecordingStopBridge();
    bridge.acpError = new Error("ACP still alive");
    const coordinator = new AgentRuntimeStopCoordinator(bridge);

    await expect(coordinator.stop("user-a")).rejects.toThrow("ACP still alive");
    expect(bridge.events).toEqual(["block:user-a", "stop-acp:user-a", "release"]);
  });
});
