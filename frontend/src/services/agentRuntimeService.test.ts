import { describe, expect, test } from "bun:test";
import {
  AgentRuntimeStopCoordinator,
  AgentRuntimePartialStopError,
  AgentRuntimeService,
  type AgentRuntimeBridge,
  type AgentRuntimeStopBridge,
  type AgentRuntimeLifecycleOutcome,
  type AgentCreateSessionRequest,
  type AgentRenameSessionRequest,
  type AgentSessionSummary,
  type AgentSendMessageRequest
} from "./agentRuntimeService";
import type { AgentOperationBlock } from "./agentOperationFence";

class RecordingBridge implements AgentRuntimeBridge {
  readonly events: string[] = [];
  lastArgs: Record<string, unknown> | undefined;
  response: unknown;
  invokeError: unknown;

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
    if (this.invokeError) throw this.invokeError;
    return this.response as T;
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

  test("queue control stays account-fenced without waiting for remote auth sync", async () => {
    const bridge = new RecordingBridge();
    const service = new AgentRuntimeService(bridge);
    const request = { sessionId: "session-1", queueId: "queue-1" };

    await service.cancelQueuedMessage("user-a", request);
    await service.unqueueMessageForEdit("user-a", request);
    await service.updateQueuedMessage("user-a", { ...request, text: "revised" });

    expect(bridge.events).toEqual([
      "fence:user-a",
      "invoke:agent_cancel_queued_message",
      "fence:user-a",
      "invoke:agent_unqueue_message_for_edit",
      "fence:user-a",
      "invoke:agent_update_queued_message"
    ]);
    expect(bridge.lastArgs).toEqual({
      userId: "user-a",
      request: { ...request, text: "revised" }
    });
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

  test("session rename forwards the request and returns the persisted summary", async () => {
    const bridge = new RecordingBridge();
    const service = new AgentRuntimeService(bridge);
    const request: AgentRenameSessionRequest = {
      sessionId: "session-1",
      title: "Renamed task"
    };
    const summary: AgentSessionSummary = {
      id: "session-1",
      title: "Renamed task",
      projectRoot: "/tmp/project",
      createdMs: 100,
      updatedMs: 200,
      messageCount: 3,
      model: "kimi-k2-6",
      mode: "auto"
    };
    bridge.response = summary;

    const result = await service.renameSession("user-a", request);

    expect(result).toBe(summary);
    expect(bridge.events).toEqual(["fence:user-a", "sync:user-a", "invoke:agent_rename_session"]);
    expect(bridge.lastArgs).toEqual({ userId: "user-a", request });
  });

  test("session rename propagates persistence failures", async () => {
    const bridge = new RecordingBridge();
    const service = new AgentRuntimeService(bridge);
    const persistenceError = new Error("database write failed");
    bridge.invokeError = persistenceError;

    await expect(
      service.renameSession("user-a", { sessionId: "session-1", title: "Renamed task" })
    ).rejects.toBe(persistenceError);
  });
});

class RecordingStopBridge implements AgentRuntimeStopBridge {
  readonly events: string[] = [];
  readonly block: AgentOperationBlock = {
    release: () => this.events.push("release"),
    retainUntilNextSession: () => this.events.push("retain")
  };
  stopError: Error | null = null;
  outcome: AgentRuntimeLifecycleOutcome = {
    status: { running: false, activeRuns: {} },
    acpShutdownError: null
  };

  async blockAndDrain(userId: string): Promise<AgentOperationBlock> {
    this.events.push(`block:${userId}`);
    return this.block;
  }

  async stopHost(userId: string): Promise<AgentRuntimeLifecycleOutcome> {
    this.events.push(`stop-host:${userId}`);
    if (this.stopError) throw this.stopError;
    return this.outcome;
  }
}

describe("AgentRuntimeStopCoordinator", () => {
  test("drains work and delegates one composite stop to the native host", async () => {
    const bridge = new RecordingStopBridge();
    const coordinator = new AgentRuntimeStopCoordinator(bridge);

    expect(await coordinator.stop("user-a")).toBe(bridge.block);
    expect(bridge.events).toEqual(["block:user-a", "stop-host:user-a"]);
  });

  test("a native host failure releases the cleanup lease", async () => {
    const bridge = new RecordingStopBridge();
    bridge.stopError = new Error("runtime still alive");
    const coordinator = new AgentRuntimeStopCoordinator(bridge);

    await expect(coordinator.stop("user-a")).rejects.toThrow("runtime still alive");
    expect(bridge.events).toEqual(["block:user-a", "stop-host:user-a", "release"]);
  });

  test("partial ACP cleanup releases the lease and preserves the stopped runtime status", async () => {
    const bridge = new RecordingStopBridge();
    bridge.outcome = {
      status: { running: false, activeRuns: {} },
      acpShutdownError: "ACP still alive"
    };
    const coordinator = new AgentRuntimeStopCoordinator(bridge);

    try {
      await coordinator.stop("user-a");
      throw new Error("expected partial stop failure");
    } catch (error) {
      expect(error).toBeInstanceOf(AgentRuntimePartialStopError);
      expect((error as AgentRuntimePartialStopError).outcome.status.running).toBe(false);
      expect(error).toHaveProperty(
        "message",
        "Agent runtime stopped, but ACP cleanup failed: ACP still alive"
      );
    }
    expect(bridge.events).toEqual(["block:user-a", "stop-host:user-a", "release"]);
  });
});
