import { describe, expect, test } from "bun:test";
import {
  AgentRuntimeService,
  type AgentRuntimeBridge,
  type AgentSendMessageRequest
} from "./agentRuntimeService";

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
      visionCapable: false
    };

    await service.sendMessage("user-a", request);

    expect(bridge.events).toEqual(["fence:user-a", "sync:user-a", "invoke:agent_send_message"]);
    expect(bridge.lastArgs).toEqual({ userId: "user-a", request });
  });
});
