import { describe, expect, test } from "bun:test";
import {
  AgentRuntimeStopCoordinator,
  AgentRuntimePartialStopError,
  AgentRuntimeService,
  LOCAL_AGENT_EXECUTION_TARGET,
  clearAgentDataForUser,
  clearAgentHistoryForUser,
  createRemoteAgentExecutionTarget,
  activateAgentRuntimeAccountResources,
  retireAgentAuthAccount,
  retireAgentRuntimeAccountResources,
  stopAgentRuntimeForUser,
  type AgentListSessionsPageRequest,
  type AgentListSessionRecordsPageRequest,
  type AgentBridgeEventHandler,
  type AgentBridgeLiveChannelResult,
  type AgentBeginSessionHistoryAttachResponse,
  type AgentExecutionLease,
  type AgentLiveChannelFrame,
  type AgentLiveEventCursor,
  type AgentRuntimeBridge,
  type AgentRuntimeInvocation,
  type AgentRuntimeStopBridge,
  type AgentRuntimeLifecycleOutcome,
  type AgentCreateSessionRequest,
  type AgentEventEnvelope,
  type AgentExecutionTarget,
  type AgentRenameSessionRequest,
  type AgentSessionRecordsPage,
  type AgentSessionSummary,
  type AgentSendMessageRequest
} from "./agentRuntimeService";
import {
  AgentOperationFence,
  AgentOperationsBlockedError,
  type AgentOperationBlock
} from "./agentOperationFence";
import { AgentAuthLifecycleCoordinator } from "./agentAuthLifecycle";
import { waitForPlatform } from "@/utils/platform";

const TEST_JOURNAL_ID = "0123456789abcdef0123456789abcdef";

function synchronizedAttachResult(): AgentBeginSessionHistoryAttachResponse {
  return {
    attachId: "attach-1",
    page: { records: [], historyRevision: "history-1" },
    liveSessionsComplete: true,
    liveSessionCount: 0,
    liveSessions: [],
    throughEventCursor: { journalId: TEST_JOURNAL_ID, sequence: 0 }
  };
}

function closedLiveItem(
  id = "live-1",
  overrides: Record<string, unknown> = {}
): Record<string, unknown> {
  return {
    id,
    itemType: "message",
    role: "assistant",
    text: "safe",
    createdMs: 1,
    merge: "replace",
    ...overrides
  };
}

function orderedLiveFrame(
  lease: AgentExecutionLease,
  target: AgentExecutionTarget,
  eventSequence: number,
  payload: Record<string, unknown>
): Record<string, unknown> {
  return {
    liveEventVersion: 1,
    targetId: target.id,
    hostEpoch: lease.hostEpoch,
    connectionGeneration: lease.connectionGeneration,
    eventEpoch: TEST_JOURNAL_ID,
    eventSequence,
    sessionId: "session",
    ...payload
  };
}

class RecordingBridge implements AgentRuntimeBridge {
  readonly events: string[] = [];
  lastArgs: Record<string, unknown> | undefined;
  response: unknown;
  invokeError: unknown;

  async syncLocalAuth(userId: string): Promise<void> {
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

class RecordingTargetBridge implements AgentRuntimeBridge {
  readonly invocations: Array<{
    lease: AgentExecutionLease;
    invocation: AgentRuntimeInvocation;
  }> = [];
  readonly preparedTargets: AgentExecutionTarget[] = [];
  readonly fencedTargets: AgentExecutionTarget[] = [];
  readonly subscriptions: Array<{
    lease: AgentExecutionLease | null;
    target: AgentExecutionTarget;
  }> = [];
  readonly listeners = new Map<string, Set<AgentBridgeEventHandler>>();
  readonly liveHandlers: AgentBridgeEventHandler[] = [];
  readonly liveResumes: Array<{
    lease: AgentExecutionLease | null;
    cursor: AgentLiveEventCursor;
  }> = [];
  readonly pendingAttachCancels: string[] = [];
  readonly liveStreamCancels: string[] = [];
  private readonly leases = new Map<
    string,
    {
      targetId: string;
      hostEpoch: string;
      connectionGeneration: number;
    }
  >();
  private nextGeneration = 1;
  runtimeStatusResult: unknown = { running: false };
  createSessionResult: unknown = null;
  sessionPageResult: unknown = { items: [], nextCursor: null };
  recordPageResult: unknown = {
    records: [],
    nextCursor: null,
    historyRevision: "history-1"
  };
  attachResult: unknown = synchronizedAttachResult();
  attachError: unknown = null;
  activateResult: unknown = {
    throughEventCursor: { journalId: "0123456789abcdef0123456789abcdef", sequence: 0 },
    liveStreamId: "attach-1"
  };
  activateError: unknown = null;
  resumeResult: unknown = {
    throughEventCursor: { journalId: "0123456789abcdef0123456789abcdef", sequence: 7 },
    liveStreamId: "stream-1"
  };
  resumeError: unknown = null;

  async prepareTarget(userId: string, target: AgentExecutionTarget): Promise<unknown> {
    this.preparedTargets.push(target);
    const key = `${userId}:${target.id as string}`;
    let lease = this.leases.get(key);
    if (!lease) {
      const generation = this.nextGeneration++;
      lease = {
        targetId: target.id,
        hostEpoch: String(generation),
        connectionGeneration: generation
      };
      this.leases.set(key, lease);
    }
    return lease;
  }

  async runForUser<T>(
    _userId: string,
    operation: () => Promise<T>,
    target?: AgentExecutionTarget
  ): Promise<T> {
    if (target) this.fencedTargets.push(target);
    return await operation();
  }

  async invokeTarget(
    lease: AgentExecutionLease,
    invocation: AgentRuntimeInvocation
  ): Promise<unknown> {
    this.invocations.push({ lease, invocation });
    switch (invocation.operation) {
      case "getRuntimeStatus":
        return this.runtimeStatusResult;
      case "createSession":
        return this.createSessionResult;
      case "listSessionsPage":
        return this.sessionPageResult;
      case "listSessionRecordsPage":
        return this.recordPageResult;
      case "stopRuntime":
        return {
          status: { running: false, activeRuns: {} },
          acpShutdownError: null
        };
      default:
        return null;
    }
  }

  async listenToEvents(
    lease: AgentExecutionLease | null,
    target: AgentExecutionTarget,
    handler: AgentBridgeEventHandler
  ): Promise<() => void> {
    this.subscriptions.push({ lease, target });
    const listeners = this.listeners.get(target.id) ?? new Set<AgentBridgeEventHandler>();
    listeners.add(handler);
    this.listeners.set(target.id, listeners);
    return () => listeners.delete(handler);
  }

  async beginSessionHistoryAttach(
    _userId: string,
    _lease: AgentExecutionLease | null,
    _target: AgentExecutionTarget,
    _request: AgentListSessionRecordsPageRequest,
    handler: AgentBridgeEventHandler
  ): Promise<AgentBridgeLiveChannelResult> {
    if (this.attachError) throw this.attachError;
    this.liveHandlers.push(handler);
    return { result: this.attachResult, keepAlive: {} };
  }

  async activateSessionHistoryAttach(): Promise<unknown> {
    if (this.activateError) throw this.activateError;
    return this.activateResult;
  }

  async cancelSessionHistoryAttach(
    _userId: string,
    _lease: AgentExecutionLease | null,
    _target: AgentExecutionTarget,
    attachId: string
  ): Promise<void> {
    this.pendingAttachCancels.push(attachId);
  }

  async resumeLiveEvents(
    _userId: string,
    lease: AgentExecutionLease | null,
    _target: AgentExecutionTarget,
    cursor: AgentLiveEventCursor,
    handler: AgentBridgeEventHandler
  ): Promise<AgentBridgeLiveChannelResult> {
    if (this.resumeError) throw this.resumeError;
    this.liveResumes.push({ lease, cursor });
    this.liveHandlers.push(handler);
    return { result: this.resumeResult, keepAlive: {} };
  }

  async cancelLiveEvents(
    _userId: string,
    _lease: AgentExecutionLease | null,
    _target: AgentExecutionTarget,
    liveStreamId: string
  ): Promise<void> {
    this.liveStreamCancels.push(liveStreamId);
  }

  emit(target: AgentExecutionTarget, event: unknown): void {
    for (const listener of this.listeners.get(target.id) ?? []) listener(event);
  }

  rotateLease(userId: string, target: AgentExecutionTarget): void {
    const generation = this.nextGeneration++;
    this.leases.set(`${userId}:${target.id as string}`, {
      targetId: target.id,
      hostEpoch: String(generation),
      connectionGeneration: generation
    });
  }
}

class ControlledPrepareBridge extends RecordingTargetBridge {
  readonly pendingPreparations: Array<{
    userId: string;
    target: AgentExecutionTarget;
    resolve: (value: unknown) => void;
    reject: (error: unknown) => void;
  }> = [];

  override async prepareTarget(userId: string, target: AgentExecutionTarget): Promise<unknown> {
    this.preparedTargets.push(target);
    return await new Promise<unknown>((resolve, reject) => {
      this.pendingPreparations.push({ userId, target, resolve, reject });
    });
  }

  resolvePreparation(index: number, generation: number): void {
    const preparation = this.pendingPreparations[index];
    preparation.resolve({
      targetId: preparation.target.id,
      hostEpoch: String(generation),
      connectionGeneration: generation
    });
  }
}

class ControlledEventBindBridge extends RecordingTargetBridge {
  readonly pendingBinds: Array<{
    lease: AgentExecutionLease;
    target: AgentExecutionTarget;
    handler: AgentBridgeEventHandler;
    resolve: (unlisten: () => void) => void;
    reject: (error: unknown) => void;
  }> = [];

  override async listenToEvents(
    lease: AgentExecutionLease | null,
    target: AgentExecutionTarget,
    handler: AgentBridgeEventHandler
  ): Promise<() => void> {
    if (!lease) return await super.listenToEvents(lease, target, handler);
    this.subscriptions.push({ lease, target });
    return await new Promise<() => void>((resolve, reject) => {
      this.pendingBinds.push({ lease, target, handler, resolve, reject });
    });
  }

  resolveBind(index: number): void {
    const bind = this.pendingBinds[index];
    const listeners = this.listeners.get(bind.target.id) ?? new Set<AgentBridgeEventHandler>();
    listeners.add(bind.handler);
    this.listeners.set(bind.target.id, listeners);
    bind.resolve(() => listeners.delete(bind.handler));
  }

  rejectBind(index: number, error: unknown): void {
    this.pendingBinds[index].reject(error);
  }
}

class FailingUnlistenBridge extends RecordingTargetBridge {
  unlistenCalls = 0;
  private failuresRemaining = 0;

  failNextUnlisten(): void {
    this.failuresRemaining += 1;
  }

  override async listenToEvents(
    lease: AgentExecutionLease | null,
    target: AgentExecutionTarget,
    handler: AgentBridgeEventHandler
  ): Promise<() => void> {
    this.subscriptions.push({ lease, target });
    const listeners = this.listeners.get(target.id) ?? new Set<AgentBridgeEventHandler>();
    listeners.add(handler);
    this.listeners.set(target.id, listeners);
    return () => {
      this.unlistenCalls += 1;
      if (this.failuresRemaining > 0) {
        this.failuresRemaining -= 1;
        throw new Error("subscription cleanup failed");
      }
      listeners.delete(handler);
    };
  }
}

class FencedControlledEventBridge extends ControlledEventBindBridge {
  constructor(private readonly fence: AgentOperationFence) {
    super();
  }

  override async runForUser<T>(
    userId: string,
    operation: () => Promise<T>,
    target?: AgentExecutionTarget
  ): Promise<T> {
    if (target) this.fencedTargets.push(target);
    return await this.fence.run(userId, operation);
  }
}

class ControlledAttachBridge extends RecordingTargetBridge {
  readonly pendingActivations: Array<{
    resolve: (value: unknown) => void;
    reject: (error: unknown) => void;
  }> = [];

  override async activateSessionHistoryAttach(): Promise<unknown> {
    return await new Promise<unknown>((resolve, reject) => {
      this.pendingActivations.push({ resolve, reject });
    });
  }
}

class ControlledBeginBridge extends RecordingTargetBridge {
  readonly pendingBegins: Array<{
    handler: AgentBridgeEventHandler;
    resolve: (value: AgentBridgeLiveChannelResult) => void;
    reject: (error: unknown) => void;
  }> = [];

  override async beginSessionHistoryAttach(
    _userId: string,
    _lease: AgentExecutionLease | null,
    _target: AgentExecutionTarget,
    _request: AgentListSessionRecordsPageRequest,
    handler: AgentBridgeEventHandler
  ): Promise<AgentBridgeLiveChannelResult> {
    return await new Promise<AgentBridgeLiveChannelResult>((resolve, reject) => {
      this.pendingBegins.push({ handler, resolve, reject });
    });
  }

  resolveBegin(index: number): void {
    const pending = this.pendingBegins[index];
    this.liveHandlers.push(pending.handler);
    pending.resolve({ result: this.attachResult, keepAlive: {} });
  }
}

class SelectiveFailingLiveCancelBridge extends RecordingTargetBridge {
  readonly cancellationFailures = new Map<string, unknown[]>();

  failNextCancellation(liveStreamId: string, error: unknown): void {
    const failures = this.cancellationFailures.get(liveStreamId) ?? [];
    failures.push(error);
    this.cancellationFailures.set(liveStreamId, failures);
  }

  override async cancelLiveEvents(
    userId: string,
    lease: AgentExecutionLease | null,
    target: AgentExecutionTarget,
    liveStreamId: string
  ): Promise<void> {
    await super.cancelLiveEvents(userId, lease, target, liveStreamId);
    const failures = this.cancellationFailures.get(liveStreamId);
    const failure = failures?.shift();
    if (failures?.length === 0) this.cancellationFailures.delete(liveStreamId);
    if (failure) throw failure;
  }
}

class ControlledLiveCancelBridge extends RecordingTargetBridge {
  readonly pendingLiveCancels: Array<{
    liveStreamId: string;
    resolve: () => void;
    reject: (error: unknown) => void;
  }> = [];

  override async cancelLiveEvents(
    userId: string,
    lease: AgentExecutionLease | null,
    target: AgentExecutionTarget,
    liveStreamId: string
  ): Promise<void> {
    await super.cancelLiveEvents(userId, lease, target, liveStreamId);
    await new Promise<void>((resolve, reject) => {
      this.pendingLiveCancels.push({ liveStreamId, resolve, reject });
    });
  }
}

class ControlledResumeBridge extends SelectiveFailingLiveCancelBridge {
  readonly pendingResumes: Array<{
    lease: AgentExecutionLease | null;
    cursor: AgentLiveEventCursor;
    handler: AgentBridgeEventHandler;
    resolve: (value: AgentBridgeLiveChannelResult) => void;
    reject: (error: unknown) => void;
  }> = [];

  override async resumeLiveEvents(
    _userId: string,
    lease: AgentExecutionLease | null,
    _target: AgentExecutionTarget,
    cursor: AgentLiveEventCursor,
    handler: AgentBridgeEventHandler
  ): Promise<AgentBridgeLiveChannelResult> {
    this.liveResumes.push({ lease, cursor });
    return await new Promise<AgentBridgeLiveChannelResult>((resolve, reject) => {
      this.pendingResumes.push({ lease, cursor, handler, resolve, reject });
    });
  }

  resolveResume(index: number, result: unknown): void {
    const pending = this.pendingResumes[index];
    this.liveHandlers.push(pending.handler);
    pending.resolve({ result, keepAlive: {} });
  }
}

class FailingLateStreamCancelBridge extends ControlledAttachBridge {
  cancelError: unknown = null;

  override async cancelLiveEvents(
    userId: string,
    lease: AgentExecutionLease | null,
    target: AgentExecutionTarget,
    liveStreamId: string
  ): Promise<void> {
    await super.cancelLiveEvents(userId, lease, target, liveStreamId);
    const error = this.cancelError;
    this.cancelError = null;
    if (error) throw error;
  }
}

async function waitFor(predicate: () => boolean): Promise<void> {
  for (let turn = 0; turn < 20; turn += 1) {
    if (predicate()) return;
    await Promise.resolve();
  }
  throw new Error("timed out waiting for test condition");
}

describe("AgentRuntimeService", () => {
  test("cancellation stays account-fenced without waiting for remote auth sync", async () => {
    const bridge = new RecordingBridge();
    const service = new AgentRuntimeService(bridge);

    await service.cancelRun("user-a", "run-1");

    expect(bridge.events).toEqual(["fence:user-a", "invoke:agent_cancel_run"]);
    expect(bridge.lastArgs).toEqual({ userId: "user-a", runId: "run-1" });
  });

  test("preserves embedded lifecycle command behavior through the semantic service", async () => {
    const stopBridge = new RecordingBridge();
    stopBridge.response = {
      status: { running: false, activeRuns: {} },
      acpShutdownError: null
    };
    const stopService = new AgentRuntimeService(stopBridge);

    await stopService.stopRuntime("user-a");
    expect(stopBridge.events).toEqual(["fence:user-a", "invoke:agent_stop_runtime"]);
    expect(stopBridge.lastArgs).toEqual({ userId: "user-a" });

    const clearBridge = new RecordingBridge();
    const clearService = new AgentRuntimeService(clearBridge);
    await clearService.clearUserData("user-a");
    await clearService.clearUserHistory("user-a");
    expect(clearBridge.events).toEqual([
      "fence:user-a",
      "sync:user-a",
      "invoke:agent_clear_user_data",
      "fence:user-a",
      "sync:user-a",
      "invoke:agent_clear_user_history"
    ]);
  });

  test("keeps implicit local cleanup helpers as no-ops outside Tauri Desktop", async () => {
    await waitForPlatform();
    const stopBlock = await stopAgentRuntimeForUser("user-a");
    const dataBlock = await clearAgentDataForUser("user-a");
    const historyBlock = await clearAgentHistoryForUser("user-a");

    expect(() => stopBlock.release()).not.toThrow();
    expect(() => dataBlock.retainUntilNextSession()).not.toThrow();
    expect(() => historyBlock.release()).not.toThrow();

    const anonymousStopBlock = await stopAgentRuntimeForUser(undefined);
    const anonymousDataBlock = await clearAgentDataForUser(undefined);
    const anonymousHistoryBlock = await clearAgentHistoryForUser(undefined);
    expect(() => anonymousStopBlock.release()).not.toThrow();
    expect(() => anonymousDataBlock.release()).not.toThrow();
    expect(() => anonymousHistoryBlock.retainUntilNextSession()).not.toThrow();
  });

  test("an explicit remote cleanup service still requires an authenticated user", async () => {
    const bridge = new RecordingTargetBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);

    await expect(stopAgentRuntimeForUser(undefined, service)).rejects.toThrow(
      "without an authenticated user"
    );
    await expect(clearAgentDataForUser(undefined, service)).rejects.toThrow(
      "without an authenticated user"
    );
    await expect(clearAgentHistoryForUser(undefined, service)).rejects.toThrow(
      "without an authenticated user"
    );
    expect(bridge.invocations).toEqual([]);
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
      pageSortMs: 200,
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

  test("keeps legacy command strings on the local bridge only", async () => {
    const bridge = new RecordingBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);

    await expect(service.cancelRun("user-a", "run-1")).rejects.toThrow(
      'Agent runtime bridge cannot prepare remote target "dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA"'
    );
    expect(bridge.events).toEqual(["fence:user-a"]);
  });

  test("routes authenticated operations through a target-aware semantic vocabulary", async () => {
    const bridge = new RecordingTargetBridge();
    const remote = createRemoteAgentExecutionTarget(
      "dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA",
      "MacBook Pro"
    );
    const service = new AgentRuntimeService(bridge, remote);

    await service.getRuntimeStatus("user-a");

    expect(bridge.fencedTargets).toEqual([remote]);
    expect(bridge.preparedTargets).toEqual([remote]);
    expect(remote.displayName).toBe("MacBook Pro");
    expect(bridge.invocations).toHaveLength(1);
    expect(bridge.invocations[0].lease).toMatchObject({
      accountId: "user-a",
      targetId: remote.id,
      hostEpoch: "1",
      connectionGeneration: 1
    });
    expect(Object.isFrozen(bridge.invocations[0].lease)).toBe(true);
    expect(bridge.invocations[0].invocation).toEqual({ operation: "getRuntimeStatus" });
    expect(JSON.stringify(bridge.invocations[0].invocation)).not.toContain("user-a");
  });

  test("rejects malformed results at the remote bridge boundary", async () => {
    const bridge = new RecordingTargetBridge();
    bridge.runtimeStatusResult = { running: "sometimes" };
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);

    await expect(service.getRuntimeStatus("user-a")).rejects.toThrow(
      'invalid result for "getRuntimeStatus"'
    );
  });

  test("rejects a zero-generation verified host lease", async () => {
    const bridge = new ControlledPrepareBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    const status = service.getRuntimeStatus("user-a");
    await waitFor(() => bridge.pendingPreparations.length === 1);
    bridge.resolvePreparation(0, 0);
    await expect(status).rejects.toThrow("invalid or mismatched execution lease");
  });

  test("accepts only the exact native execution lease shape", async () => {
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const invalidLeases: unknown[] = [
      { targetId: remote.id, connectionGeneration: 1 },
      { targetId: remote.id, hostEpoch: "1", connectionGeneration: 1, accountId: "user-a" },
      {
        targetId: remote.id,
        hostEpoch: "1",
        connectionGeneration: 1,
        leaseId: "legacy-lease"
      },
      { targetId: "another-target", hostEpoch: "1", connectionGeneration: 1 },
      { targetId: remote.id, hostEpoch: "", connectionGeneration: 1 },
      { targetId: remote.id, hostEpoch: "host:1", connectionGeneration: 1 },
      { targetId: remote.id, hostEpoch: "01", connectionGeneration: 1 },
      { targetId: remote.id, hostEpoch: "18446744073709551616", connectionGeneration: 1 },
      { targetId: remote.id, hostEpoch: "1", connectionGeneration: 0 }
    ];

    for (const invalidLease of invalidLeases) {
      const bridge = new ControlledPrepareBridge();
      const service = new AgentRuntimeService(bridge, remote);
      const status = service.getRuntimeStatus("user-a");
      await waitFor(() => bridge.pendingPreparations.length === 1);
      bridge.pendingPreparations[0].resolve(invalidLease);
      await expect(status).rejects.toThrow("invalid or mismatched execution lease");
    }
  });

  test("rejects an in-flight result after reconnect replaces its immutable lease", async () => {
    const bridge = new RecordingTargetBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    let resolveFirst: ((value: unknown) => void) | undefined;
    bridge.runtimeStatusResult = new Promise((resolve) => {
      resolveFirst = resolve;
    });

    const firstInvocation = service.getRuntimeStatus("user-a");
    await waitFor(() => bridge.invocations.length === 1);
    expect(bridge.invocations).toHaveLength(1);
    const firstLease = bridge.invocations[0].lease;

    bridge.rotateLease("user-a", remote);
    bridge.runtimeStatusResult = { running: true };
    await expect(service.getRuntimeStatus("user-a")).resolves.toEqual({ running: true });
    const replacementLease = bridge.invocations[1].lease;
    expect(replacementLease.hostEpoch).not.toBe(firstLease.hostEpoch);

    resolveFirst?.({ running: false });
    await expect(firstInvocation).rejects.toThrow(
      'execution lease changed while "getRuntimeStatus" was in flight'
    );
  });

  test("a late account A preparation cannot overwrite account B invocation authority", async () => {
    const bridge = new ControlledPrepareBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);

    const accountA = service.getRuntimeStatus("account-a");
    await waitFor(() => bridge.pendingPreparations.length === 1);
    const accountAOutcome = accountA.then(
      () => null,
      (error: unknown) => error
    );

    const accountB = service.getRuntimeStatus("account-b");
    await waitFor(() => bridge.pendingPreparations.length === 2);
    bridge.resolvePreparation(1, 22);
    await expect(accountB).resolves.toEqual({ running: false });

    bridge.resolvePreparation(0, 11);
    expect(await accountAOutcome).toHaveProperty(
      "message",
      "Remote Agent target preparation was superseded"
    );
    expect(bridge.invocations).toHaveLength(1);
    expect(bridge.invocations[0].lease).toMatchObject({
      accountId: "account-b",
      hostEpoch: "22",
      connectionGeneration: 22
    });
  });

  test("A to B to A handoff cannot insert or later revive A's retired subscription", async () => {
    const bridge = new ControlledPrepareBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    const retiredEvents: AgentEventEnvelope[] = [];

    const subscribeA = service.listenToEvents("account-a", (event) => retiredEvents.push(event));
    const subscribeAOutcome = subscribeA.then(
      () => null,
      (error: unknown) => error
    );
    await waitFor(() => bridge.pendingPreparations.length === 1);
    bridge.resolvePreparation(0, 51);

    // completeRemotePreparation installs A and queues the listenToEvents
    // continuation. Resume the test between those two handoff microtasks, then
    // synchronously let B retire A before A can enter remoteSubscriptions.
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
    const commandB = service.getRuntimeStatus("account-b");
    await waitFor(() => bridge.pendingPreparations.length === 2);
    bridge.resolvePreparation(1, 52);
    await expect(commandB).resolves.toEqual({ running: false });

    expect(await subscribeAOutcome).toHaveProperty(
      "message",
      "Remote Agent execution lease changed before event subscription handoff"
    );
    expect(bridge.subscriptions).toEqual([]);

    const commandA = service.getRuntimeStatus("account-a");
    await waitFor(() => bridge.pendingPreparations.length === 3);
    bridge.resolvePreparation(2, 53);
    await expect(commandA).resolves.toEqual({ running: false });

    // Returning to account A refreshes command authority only. The retired
    // late subscription was never inserted, so it cannot silently rebind.
    expect(bridge.subscriptions).toEqual([]);
    bridge.emit(remote, {
      eventType: "runStarted",
      targetId: remote.id,
      connectionGeneration: 53,
      sessionId: "session-1",
      runId: "must-not-deliver"
    });
    expect(retiredEvents).toEqual([]);
  });

  test("coalesces concurrent same-account status and paged-load preparation", async () => {
    const bridge = new ControlledPrepareBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);

    const status = service.getRuntimeStatus("user-a");
    const sessions = service.listSessionsPage("user-a", { limit: 25 });
    await waitFor(() => bridge.pendingPreparations.length === 1);
    bridge.resolvePreparation(0, 41);

    await expect(status).resolves.toEqual({ running: false });
    await expect(sessions).resolves.toEqual({ items: [], nextCursor: null });
    expect(bridge.preparedTargets).toEqual([remote]);
    expect(bridge.invocations.map(({ invocation }) => invocation.operation).sort()).toEqual([
      "getRuntimeStatus",
      "listSessionsPage"
    ]);
    expect(bridge.invocations[0].lease).toMatchObject({
      accountId: "user-a",
      hostEpoch: "41",
      connectionGeneration: 41
    });
    expect(bridge.invocations[1].lease).toMatchObject({
      accountId: "user-a",
      hostEpoch: "41",
      connectionGeneration: 41
    });
  });

  test("a generation refresh supersedes an initial async bind without closing the logical subscription", async () => {
    const bridge = new ControlledEventBindBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    const events: AgentEventEnvelope[] = [];

    const subscription = service.listenToEvents("user-a", (event) => events.push(event));
    await waitFor(() => bridge.pendingBinds.length === 1);
    const initialLease = bridge.pendingBinds[0].lease;

    bridge.rotateLease("user-a", remote);
    const refreshCommand = service.getRuntimeStatus("user-a");
    await waitFor(() => bridge.pendingBinds.length === 2);
    const refreshedLease = bridge.pendingBinds[1].lease;
    expect(refreshedLease.connectionGeneration).not.toBe(initialLease.connectionGeneration);

    bridge.resolveBind(1);
    await expect(refreshCommand).resolves.toEqual({ running: false });
    bridge.resolveBind(0);
    const unlisten = await subscription;

    bridge.emit(remote, {
      eventType: "runStarted",
      targetId: remote.id,
      connectionGeneration: initialLease.connectionGeneration,
      sessionId: "session-1",
      runId: "old-run"
    });
    expect(events).toEqual([]);
    bridge.emit(remote, {
      eventType: "runStarted",
      targetId: remote.id,
      hostEpoch: refreshedLease.hostEpoch,
      connectionGeneration: refreshedLease.connectionGeneration,
      eventEpoch: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      eventSequence: 1,
      sessionId: "session-1",
      runId: "new-run"
    });
    expect(events).toHaveLength(1);
    expect(events[0]).toMatchObject({ runId: "new-run" });

    unlisten();
  });

  test("a stale initial native bind rejection cannot close its successful generation replacement", async () => {
    const bridge = new ControlledEventBindBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    const events: AgentEventEnvelope[] = [];

    const subscription = service.listenToEvents("user-a", (event) => events.push(event));
    await waitFor(() => bridge.pendingBinds.length === 1);
    const initialLease = bridge.pendingBinds[0].lease;

    bridge.rotateLease("user-a", remote);
    const refreshCommand = service.getRuntimeStatus("user-a");
    await waitFor(() => bridge.pendingBinds.length === 2);
    const replacementLease = bridge.pendingBinds[1].lease;

    bridge.resolveBind(1);
    await expect(refreshCommand).resolves.toEqual({ running: false });
    bridge.rejectBind(0, new Error("generation 1 socket closed"));
    const unlisten = await subscription;

    bridge.emit(remote, {
      eventType: "runStarted",
      targetId: remote.id,
      connectionGeneration: initialLease.connectionGeneration,
      sessionId: "session-1",
      runId: "old-run"
    });
    expect(events).toEqual([]);
    bridge.emit(remote, {
      eventType: "runStarted",
      targetId: remote.id,
      hostEpoch: replacementLease.hostEpoch,
      connectionGeneration: replacementLease.connectionGeneration,
      eventEpoch: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      eventSequence: 1,
      sessionId: "session-1",
      runId: "new-run"
    });
    expect(events).toHaveLength(1);
    expect(events[0]).toMatchObject({ runId: "new-run" });

    unlisten();
  });

  test("account B retirement rejects account A when its pending initial bind later resolves", async () => {
    const bridge = new ControlledEventBindBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);

    const subscriptionA = service.listenToEvents("account-a", () => {});
    const subscriptionAOutcome = subscriptionA.then(
      () => null,
      (error: unknown) => error
    );
    await waitFor(() => bridge.pendingBinds.length === 1);

    let accountBSettled = false;
    const accountB = service.getRuntimeStatus("account-b").finally(() => {
      accountBSettled = true;
    });
    await Promise.resolve();
    expect(accountBSettled).toBe(false);
    bridge.resolveBind(0);

    expect(await subscriptionAOutcome).toHaveProperty(
      "message",
      "Remote Agent event subscription was retired before its initial bind completed"
    );
    await expect(accountB).resolves.toEqual({ running: false });
    expect(bridge.listeners.get(remote.id)?.size ?? 0).toBe(0);
  });

  test("account B retirement rejects account A when its pending initial bind later rejects", async () => {
    const bridge = new ControlledEventBindBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);

    const subscriptionA = service.listenToEvents("account-a", () => {});
    const subscriptionAOutcome = subscriptionA.then(
      () => null,
      (error: unknown) => error
    );
    await waitFor(() => bridge.pendingBinds.length === 1);

    let accountBSettled = false;
    const accountB = service.getRuntimeStatus("account-b").finally(() => {
      accountBSettled = true;
    });
    await Promise.resolve();
    expect(accountBSettled).toBe(false);
    bridge.rejectBind(0, new Error("retired account socket closed"));

    expect(await subscriptionAOutcome).toHaveProperty(
      "message",
      "Remote Agent event subscription was retired before its initial bind completed"
    );
    await expect(accountB).resolves.toEqual({ running: false });
  });

  test("account B preparation synchronously retires account A's subscription", async () => {
    const bridge = new ControlledPrepareBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    const accountAEvents: AgentEventEnvelope[] = [];
    const accountBEvents: AgentEventEnvelope[] = [];

    const subscribeA = service.listenToEvents("account-a", (event) => accountAEvents.push(event));
    await waitFor(() => bridge.pendingPreparations.length === 1);
    bridge.resolvePreparation(0, 31);
    await subscribeA;
    const accountALease = bridge.subscriptions[0].lease!;

    const subscribeB = service.listenToEvents("account-b", (event) => accountBEvents.push(event));
    await waitFor(() => bridge.pendingPreparations.length === 2);
    bridge.emit(remote, {
      eventType: "runStarted",
      targetId: remote.id,
      connectionGeneration: accountALease.connectionGeneration,
      sessionId: "session-1",
      runId: "run-a"
    });
    expect(accountAEvents).toEqual([]);

    bridge.resolvePreparation(1, 32);
    await subscribeB;
    const accountBLease = bridge.subscriptions[1].lease!;
    bridge.emit(remote, {
      eventType: "runStarted",
      targetId: remote.id,
      hostEpoch: accountBLease.hostEpoch,
      connectionGeneration: accountBLease.connectionGeneration,
      eventEpoch: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      eventSequence: 1,
      sessionId: "session-1",
      runId: "run-b"
    });
    expect(accountAEvents).toEqual([]);
    expect(accountBEvents).toHaveLength(1);
  });

  test("a non-desktop A to B transition drains delayed operations and subscription binds", async () => {
    const fence = new AgentOperationFence();
    const bridge = new FencedControlledEventBridge(fence);
    const baseService = new AgentRuntimeService(bridge);
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = baseService.forTarget(remote);
    const activations: string[] = [];
    const desktopCalls: string[] = [];
    const coordinator = new AgentAuthLifecycleCoordinator(
      async (userId) =>
        await retireAgentAuthAccount(userId, {
          blockAndDrain: async (accountId) => await fence.blockAndDrain(accountId),
          retireRemoteAccount: async (accountId) => await baseService.retireAccount(accountId),
          isDesktop: () => false,
          stopLocalHost: async (accountId) => {
            desktopCalls.push(`stop:${accountId}`);
            return { status: { running: false }, acpShutdownError: null };
          },
          clearLocalAuth: async (accountId) => {
            desktopCalls.push(`clear:${accountId}`);
          },
          stopLocalProxy: async () => {
            desktopCalls.push("proxy");
          }
        }),
      async (userId) => {
        activations.push(userId);
        fence.activateUserSession(userId);
      }
    );
    await coordinator.transitionTo("account-a");

    let finishStatus: ((value: unknown) => void) | undefined;
    bridge.runtimeStatusResult = new Promise<unknown>((resolve) => {
      finishStatus = resolve;
    });
    const delayedStatus = service.getRuntimeStatus("account-a");
    const delayedSubscription = service.listenToEvents("account-a", () => {});
    await waitFor(() => bridge.invocations.length === 1 && bridge.pendingBinds.length === 1);

    let transitionSettled = false;
    const transitionB = coordinator.transitionTo("account-b").finally(() => {
      transitionSettled = true;
    });
    await Promise.resolve();
    await Promise.resolve();
    expect(transitionSettled).toBe(false);
    expect(activations).toEqual(["account-a"]);

    finishStatus?.({ running: false });
    bridge.resolveBind(0);
    await expect(delayedStatus).resolves.toEqual({ running: false });
    const lateUnlisten = await delayedSubscription;
    await transitionB;

    expect(activations).toEqual(["account-a", "account-b"]);
    expect(desktopCalls).toEqual([]);
    expect(bridge.listeners.get(remote.id)?.size ?? 0).toBe(0);
    lateUnlisten();
    await expect(service.getRuntimeStatus("account-a")).rejects.toBeInstanceOf(
      AgentOperationsBlockedError
    );
  });

  test("auth retirement drains a custom-bridge scope with a delayed initial bind", async () => {
    const accountId = "patch3-custom-bind-account";
    activateAgentRuntimeAccountResources(accountId);
    const bridge = new ControlledEventBindBridge();
    const remote = createRemoteAgentExecutionTarget("dev_patch3_custom_bind");
    const service = new AgentRuntimeService(bridge, remote);
    try {
      const subscription = service.listenToEvents(accountId, () => {});
      const subscriptionOutcome = subscription.then(
        () => null,
        (error: unknown) => error
      );
      await waitFor(() => bridge.pendingBinds.length === 1);

      let retired = false;
      const retirement = retireAgentRuntimeAccountResources(accountId).finally(() => {
        retired = true;
      });
      await Promise.resolve();
      expect(retired).toBe(false);

      bridge.resolveBind(0);
      expect(await subscriptionOutcome).toHaveProperty(
        "message",
        "Remote Agent event subscription was retired before its initial bind completed"
      );
      await retirement;
      expect(bridge.listeners.get(remote.id)?.size ?? 0).toBe(0);
    } finally {
      activateAgentRuntimeAccountResources(accountId);
    }
  });

  test("auth retirement owns a custom-bridge attach before its native ID arrives", async () => {
    const accountId = "patch3-pending-attach-account";
    activateAgentRuntimeAccountResources(accountId);
    const bridge = new ControlledBeginBridge();
    const remote = createRemoteAgentExecutionTarget("dev_patch3_pending_attach");
    const service = new AgentRuntimeService(bridge, remote);
    try {
      const attach = service.beginSessionHistoryAttach(
        accountId,
        { sessionId: "session" },
        () => {}
      );
      const attachOutcome = attach.then(
        () => null,
        (error: unknown) => error
      );
      await waitFor(() => bridge.pendingBegins.length === 1);

      let retired = false;
      const retirement = retireAgentRuntimeAccountResources(accountId).finally(() => {
        retired = true;
      });
      await Promise.resolve();
      expect(retired).toBe(false);

      bridge.resolveBegin(0);
      expect(await attachOutcome).toHaveProperty(
        "message",
        "Agent history attachment owner retired while opening"
      );
      await retirement;
      expect(bridge.pendingAttachCancels).toEqual(["attach-1"]);
    } finally {
      activateAgentRuntimeAccountResources(accountId);
    }
  });

  test("auth retirement waits for a replacement opened by an ordinary invocation", async () => {
    const accountId = "patch3-ordinary-rebind-account";
    activateAgentRuntimeAccountResources(accountId);
    const bridge = new ControlledResumeBridge();
    const remote = createRemoteAgentExecutionTarget("dev_patch3_ordinary_rebind");
    const service = new AgentRuntimeService(bridge, remote);
    try {
      const pending = await service.beginSessionHistoryAttach(
        accountId,
        { sessionId: "session" },
        () => {}
      );
      await pending.activate();
      bridge.rotateLease(accountId, remote);
      const refresh = service.getRuntimeStatus(accountId);
      await waitFor(() => bridge.pendingResumes.length === 1);

      let retired = false;
      const retirement = retireAgentRuntimeAccountResources(accountId).finally(() => {
        retired = true;
      });
      await Promise.resolve();
      expect(retired).toBe(false);

      bridge.resolveResume(0, {
        throughEventCursor: { journalId: TEST_JOURNAL_ID, sequence: 0 },
        liveStreamId: "retired-replacement"
      });
      await expect(refresh).rejects.toThrow("superseded");
      await retirement;
      expect(bridge.liveStreamCancels).toEqual(["attach-1", "retired-replacement"]);
    } finally {
      activateAgentRuntimeAccountResources(accountId);
    }
  });

  test("a fresh service retries a failed subscription cleanup before rebinding", async () => {
    const bridge = new FailingUnlistenBridge();
    const remote = createRemoteAgentExecutionTarget("dev_patch3_subscription_remount");
    const firstService = new AgentRuntimeService(bridge, remote);
    const unlisten = await firstService.listenToEvents(
      "patch3-subscription-remount-account",
      () => {}
    );
    bridge.failNextUnlisten();

    unlisten();
    await Promise.resolve();
    await Promise.resolve();
    expect(bridge.unlistenCalls).toBe(1);
    expect(bridge.listeners.get(remote.id)?.size ?? 0).toBe(1);

    const replacementService = new AgentRuntimeService(bridge, remote);
    const replacementUnlisten = await replacementService.listenToEvents(
      "patch3-subscription-remount-account",
      () => {}
    );
    expect(bridge.unlistenCalls).toBe(2);
    expect(bridge.listeners.get(remote.id)?.size ?? 0).toBe(1);

    replacementUnlisten();
    await Promise.resolve();
    expect(bridge.listeners.get(remote.id)?.size ?? 0).toBe(0);
  });

  test("lifecycle and destructive operations use the target lease and semantic host adapter", async () => {
    const bridge = new RecordingTargetBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);

    await expect(service.stopRuntime("user-a")).resolves.toMatchObject({
      status: { running: false },
      acpShutdownError: null
    });
    await service.clearUserData("user-a");
    await service.clearUserHistory("user-a");

    expect(bridge.invocations.map(({ invocation }) => invocation)).toEqual([
      { operation: "stopRuntime" },
      { operation: "clearUserData" },
      { operation: "clearUserHistory" }
    ]);
    expect(bridge.invocations.every(({ lease }) => lease.accountId === "user-a")).toBe(true);
    expect(JSON.stringify(bridge.invocations)).not.toContain('"userId"');
  });

  test("remote clear helper stops the host before clearing through target authority", async () => {
    const bridge = new RecordingTargetBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);

    const block = await clearAgentDataForUser("user-a", service);

    expect(bridge.invocations.map(({ invocation }) => invocation.operation)).toEqual([
      "stopRuntime",
      "clearUserData"
    ]);
    expect(() => block.release()).not.toThrow();
  });

  test("validates opaque remote IDs independently from display names", () => {
    const target = createRemoteAgentExecutionTarget(
      "dev:01J4Z3N9Y5K7QX2P8B6C0R1TWA",
      "Ada's MacBook Pro"
    );

    expect(String(target.id)).toBe("dev:01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    expect(target.displayName).toBe("Ada's MacBook Pro");
    for (const invalidId of [
      123,
      "",
      "local",
      "human readable name",
      "unicode-🪿",
      "a".repeat(129)
    ]) {
      expect(() => createRemoteAgentExecutionTarget(invalidId)).toThrow();
    }
    expect(() => createRemoteAgentExecutionTarget("device-1", 123)).toThrow(
      "display name must be a string"
    );
    expect(String(createRemoteAgentExecutionTarget("a".repeat(128)).id)).toBe("a".repeat(128));
  });

  test("constructs a closed remote create response and rejects hostile timeline authority", async () => {
    const bridge = new RecordingTargetBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    const rawSummary = {
      id: "session-1",
      title: "New task",
      projectRoot: "/workspace",
      createdMs: 1,
      updatedMs: 1,
      pageSortMs: 1,
      messageCount: 0,
      model: null,
      mode: "smart_approve"
    };
    const rawDetail = { session: rawSummary, timeline: [], mcpErrors: [] };
    bridge.createSessionResult = rawDetail;

    const detail = await service.createSession("user-a");
    expect(detail).toEqual(rawDetail);
    expect(detail).not.toBe(rawDetail);
    expect(detail.session).not.toBe(rawSummary);

    for (const hostile of [
      { ...rawDetail, providerExtension: "secret" },
      { ...rawDetail, timeline: [closedLiveItem("input", { input: { secret: true } })] },
      { ...rawDetail, timeline: [closedLiveItem("output", { output: "secret" })] },
      { ...rawDetail, timeline: [closedLiveItem("unknown", { providerExtension: true })] }
    ]) {
      bridge.createSessionResult = hostile;
      await expect(service.createSession("user-a")).rejects.toThrow(
        'invalid result for "createSession"'
      );
    }
  });

  test("exposes paged remote history operations and rejects unpaged compatibility calls", async () => {
    const bridge = new RecordingTargetBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);

    await service.listSessionsPage("user-a", {
      projectRoot: "/workspace",
      cursor: "sessions:50",
      limit: 50
    });
    await service.listSessionRecordsPage("user-a", {
      sessionId: "session-1",
      cursor: "timeline:100",
      limit: 50
    });

    expect(bridge.invocations.map(({ invocation }) => invocation)).toEqual([
      {
        operation: "listSessionsPage",
        request: {
          request: { projectRoot: "/workspace", cursor: "sessions:50", limit: 50 }
        }
      },
      {
        operation: "listSessionRecordsPage",
        request: {
          request: { sessionId: "session-1", cursor: "timeline:100", limit: 50 }
        }
      }
    ]);
    await expect(service.listSessions("user-a")).rejects.toThrow(
      '"listSessions" is embedded-only compatibility'
    );
    await expect(service.loadSession("user-a", "session-1")).rejects.toThrow(
      '"loadSession" is embedded-only compatibility'
    );
    await expect(service.listSessionsPage("user-a", { limit: 51 })).rejects.toThrow(
      "page limit must be between 1 and 50"
    );
    await expect(
      service.listSessionsPage("user-a", {
        cursor: 7
      } as unknown as AgentListSessionsPageRequest)
    ).rejects.toThrow("cursor must be non-empty bounded ASCII");
    await expect(
      service.listSessionsPage("user-a", {
        projectRoot: 7
      } as unknown as AgentListSessionsPageRequest)
    ).rejects.toThrow("project root must be a string or null");
    await expect(
      service.listSessionRecordsPage("user-a", {
        sessionId: 7
      } as unknown as AgentListSessionRecordsPageRequest)
    ).rejects.toThrow("session ID must be a non-empty string");

    bridge.sessionPageResult = { items: [], nextCursor: 7 };
    await expect(service.listSessionsPage("user-a")).rejects.toThrow(
      'invalid result for "listSessionsPage"'
    );
    bridge.sessionPageResult = {
      items: [
        {
          id: "session-1",
          title: "Session",
          projectRoot: 7,
          createdMs: 1,
          updatedMs: 2,
          pageSortMs: 2,
          messageCount: 0,
          mode: "auto"
        }
      ],
      nextCursor: null
    };
    await expect(service.listSessionsPage("user-a")).rejects.toThrow(
      'invalid result for "listSessionsPage"'
    );
  });

  test("constructs bounded closed session pages and rejects unsafe summaries", async () => {
    const bridge = new RecordingTargetBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    const rawSummary = {
      id: "session-1",
      title: "Task",
      projectRoot: "/workspace",
      createdMs: 1,
      updatedMs: 2,
      pageSortMs: 2,
      messageCount: 3,
      model: "model-1",
      mode: "smart_approve"
    };
    const rawPage = { items: [rawSummary], nextCursor: "next" };
    bridge.sessionPageResult = rawPage;

    const decoded = await service.listSessionsPage("user-a");
    expect(decoded).toEqual(rawPage);
    expect(decoded).not.toBe(rawPage);
    expect(decoded.items[0]).not.toBe(rawSummary);

    const hostileSummaries = [
      { ...rawSummary, providerExtension: true },
      { ...rawSummary, messageCount: Number.MAX_SAFE_INTEGER + 1 },
      { ...rawSummary, createdMs: -1 },
      { ...rawSummary, title: "unsafe\ncontrol" },
      { ...rawSummary, title: "spoof\u202Etxt" },
      { ...rawSummary, mode: "\u061Cauto" },
      { ...rawSummary, title: "x".repeat(1_025) }
    ];
    for (const summary of hostileSummaries) {
      bridge.sessionPageResult = { items: [summary], nextCursor: null };
      await expect(service.listSessionsPage("user-a")).rejects.toThrow(
        'invalid result for "listSessionsPage"'
      );
    }
    bridge.sessionPageResult = { ...rawPage, unknown: true };
    await expect(service.listSessionsPage("user-a")).rejects.toThrow(
      'invalid result for "listSessionsPage"'
    );
  });

  test("enforces the requested page limit and cursor progress on returned pages", async () => {
    const bridge = new RecordingTargetBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    const summary = (id: string): AgentSessionSummary => ({
      id,
      title: id,
      projectRoot: "/workspace",
      createdMs: 1,
      updatedMs: 1,
      pageSortMs: 1,
      messageCount: 0,
      mode: "smart_approve"
    });

    bridge.sessionPageResult = { items: [summary("one"), summary("two")], nextCursor: null };
    await expect(service.listSessionsPage("user-a", { limit: 1 })).rejects.toThrow(
      "exceeded the requested record limit"
    );

    bridge.sessionPageResult = { items: [summary("one")], nextCursor: "same" };
    await expect(service.listSessionsPage("user-a", { cursor: "same", limit: 1 })).rejects.toThrow(
      "cursor did not progress"
    );

    bridge.recordPageResult = {
      records: [
        { recordId: "r1", role: "user", createdMs: 1, items: [] },
        { recordId: "r2", role: "assistant", createdMs: 2, items: [] }
      ],
      nextCursor: null,
      historyRevision: "history-1"
    };
    await expect(
      service.listSessionRecordsPage("user-a", { sessionId: "session", limit: 1 })
    ).rejects.toThrow("exceeded the requested record limit");

    bridge.recordPageResult = {
      records: [],
      nextCursor: "later",
      historyRevision: "history-1"
    };
    await expect(
      service.listSessionRecordsPage("user-a", { sessionId: "session", limit: 1 })
    ).rejects.toThrow("cursor without records");
  });

  test("keeps ordinary history pages strictly unsynchronized", async () => {
    const bridge = new RecordingTargetBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    const liveItem = {
      id: "live-1",
      itemType: "message",
      role: "assistant",
      text: "live",
      createdMs: 1,
      merge: "replace"
    } as const;

    bridge.recordPageResult = {
      records: [],
      nextCursor: null,
      historyRevision: "history-1",
      liveItems: [liveItem]
    };
    await expect(
      service.listSessionRecordsPage("user-a", { sessionId: "session" })
    ).rejects.toThrow('invalid result for "listSessionRecordsPage"');

    bridge.recordPageResult = {
      records: [],
      nextCursor: null,
      historyRevision: "history-1",
      throughEventCursor: { journalId: "journal", sequence: 0 }
    };
    await expect(
      service.listSessionRecordsPage("user-a", { sessionId: "session" })
    ).rejects.toThrow('invalid result for "listSessionRecordsPage"');

    bridge.recordPageResult = {
      records: [],
      nextCursor: null,
      historyRevision: "history-1",
      liveItems: [liveItem],
      throughEventCursor: { journalId: "journal", sequence: 0 }
    };
    await expect(
      service.listSessionRecordsPage("user-a", { sessionId: "session" })
    ).rejects.toThrow('invalid result for "listSessionRecordsPage"');

    bridge.recordPageResult = {
      records: [],
      nextCursor: null,
      historyRevision: "history-1",
      liveItems: [{ ...liveItem, merge: "append" }],
      throughEventCursor: { journalId: "journal", sequence: 0 }
    };
    await expect(
      service.listSessionRecordsPage("user-a", { sessionId: "session" })
    ).rejects.toThrow('invalid result for "listSessionRecordsPage"');
  });

  test("preserves hidden native message rows with non-chat roles", async () => {
    const bridge = new RecordingTargetBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    const expectedPage: AgentSessionRecordsPage = {
      records: [{ recordId: "provider-row", role: "developer", createdMs: 1, items: [] }],
      nextCursor: "older",
      historyRevision: "history-1"
    };
    bridge.recordPageResult = expectedPage;

    await expect(
      service.listSessionRecordsPage("user-a", { sessionId: "session" })
    ).resolves.toEqual(expectedPage);

    for (const role of ["developer\nspoof", "developer\u202espoof"]) {
      bridge.recordPageResult = {
        ...expectedPage,
        records: [{ ...expectedPage.records[0], role }]
      };
      await expect(
        service.listSessionRecordsPage("user-a", { sessionId: "session" })
      ).rejects.toThrow('invalid result for "listSessionRecordsPage"');
    }
  });

  test("begins, activates, and cancels a fully validated synchronized history stream", async () => {
    const bridge = new RecordingTargetBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    const frames: AgentLiveChannelFrame[] = [];

    const pending = await service.beginSessionHistoryAttach(
      "user-a",
      { sessionId: "session", limit: 25 },
      (frame) => frames.push(frame)
    );
    expect(pending.response).toEqual(synchronizedAttachResult());
    // begin does not use the ordinary invocation list; inspect the bridge's
    // current verified lease through the next fenced operation.
    await service.getRuntimeStatus("user-a");
    const currentLease = bridge.invocations.at(-1)!.lease;

    bridge.liveHandlers[0]({
      liveEventVersion: 1,
      eventType: "runStarted",
      targetId: remote.id,
      hostEpoch: `${currentLease.hostEpoch}:stale`,
      connectionGeneration: currentLease.connectionGeneration,
      eventEpoch: TEST_JOURNAL_ID,
      eventSequence: 1,
      sessionId: "session",
      runId: "stale"
    });
    expect(frames).toEqual([]);
    bridge.liveHandlers[0]({
      liveEventVersion: 1,
      eventType: "runStarted",
      targetId: createRemoteAgentExecutionTarget("other-target").id,
      hostEpoch: currentLease.hostEpoch,
      connectionGeneration: currentLease.connectionGeneration,
      eventEpoch: TEST_JOURNAL_ID,
      eventSequence: 1,
      sessionId: "session",
      runId: "wrong-target"
    });
    bridge.liveHandlers[0]({
      liveEventVersion: 1,
      eventType: "runStarted",
      targetId: remote.id,
      hostEpoch: currentLease.hostEpoch,
      connectionGeneration: currentLease.connectionGeneration + 1,
      eventEpoch: TEST_JOURNAL_ID,
      eventSequence: 1,
      sessionId: "session",
      runId: "wrong-generation"
    });
    bridge.liveHandlers[0]({
      eventType: "notAnAgentEvent",
      targetId: remote.id,
      hostEpoch: currentLease.hostEpoch,
      connectionGeneration: currentLease.connectionGeneration
    });
    expect(frames).toEqual([]);
    bridge.liveHandlers[0]({
      liveEventVersion: 1,
      eventType: "runStarted",
      targetId: remote.id,
      hostEpoch: currentLease.hostEpoch,
      connectionGeneration: currentLease.connectionGeneration,
      eventEpoch: TEST_JOURNAL_ID,
      eventSequence: 1,
      sessionId: "session",
      runId: "run"
    });
    expect(frames).toMatchObject([{ eventType: "runStarted", runId: "run" }]);

    const active = await pending.activate();
    expect(active.liveStreamId).toBe("attach-1");
    await active.cancel();
    await active.cancel();
    expect(bridge.liveStreamCancels).toEqual(["attach-1"]);
  });

  test("rejects malformed complete snapshots and cancels their paused lease", async () => {
    const bridge = new RecordingTargetBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    bridge.attachResult = {
      ...(synchronizedAttachResult() as unknown as Record<string, unknown>),
      liveSessionCount: 1,
      liveSessions: []
    };

    await expect(
      service.beginSessionHistoryAttach("user-a", { sessionId: "session" }, () => {})
    ).rejects.toThrow("invalid synchronized history attachment");
    expect(bridge.pendingAttachCancels).toEqual(["attach-1"]);
  });

  test("validates synchronized session ordering by native UTF-8 byte order", async () => {
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const correctlyOrdered = [
      { sessionId: "session-\ue000", liveItems: [closedLiveItem("bmp")] },
      { sessionId: "session-🪿", liveItems: [closedLiveItem("supplementary")] }
    ];
    const bridge = new RecordingTargetBridge();
    bridge.attachResult = {
      ...synchronizedAttachResult(),
      liveSessionCount: 2,
      liveSessions: correctlyOrdered
    };
    const service = new AgentRuntimeService(bridge, remote);
    const pending = await service.beginSessionHistoryAttach(
      "user-a",
      { sessionId: "session" },
      () => {}
    );
    expect(pending.response.liveSessions.map(({ sessionId }) => sessionId)).toEqual(
      correctlyOrdered.map(({ sessionId }) => sessionId)
    );
    await pending.cancel();

    const reversedBridge = new RecordingTargetBridge();
    reversedBridge.attachResult = {
      ...synchronizedAttachResult(),
      liveSessionCount: 2,
      liveSessions: [...correctlyOrdered].reverse()
    };
    const reversedService = new AgentRuntimeService(reversedBridge, remote);
    await expect(
      reversedService.beginSessionHistoryAttach("user-a", { sessionId: "session" }, () => {})
    ).rejects.toThrow("invalid synchronized live sessions");
    expect(reversedBridge.pendingAttachCancels).toEqual(["attach-1"]);
  });

  test("rejects unsafe synchronized history and live items before installing the snapshot", async () => {
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const unsafeItems = [
      closedLiveItem("provider-json", { input: { token: "secret" } }),
      closedLiveItem("extra-field", { providerExtension: "secret" }),
      closedLiveItem("raw-tool", {
        itemType: "tool",
        role: "assistant",
        title: "curl --header Authorization: secret",
        text: "/Users/alice/.env",
        status: "failed"
      }),
      closedLiveItem("actionable-permission", {
        itemType: "permission",
        role: "system",
        title: "Tool permission",
        text: undefined,
        status: "pending"
      }),
      closedLiveItem("oversized-title", { title: "🪿".repeat(300) }),
      closedLiveItem("bidi-\u202espoof")
    ];

    for (const [index, item] of unsafeItems.entries()) {
      const bridge = new RecordingTargetBridge();
      const service = new AgentRuntimeService(bridge, remote);
      bridge.attachResult = {
        ...synchronizedAttachResult(),
        attachId: `attach-${index}`,
        page: {
          records: [{ recordId: `row-${index}`, role: "assistant", createdMs: 1, items: [item] }],
          historyRevision: "history-1"
        }
      };
      await expect(
        service.beginSessionHistoryAttach("user-a", { sessionId: "session" }, () => {})
      ).rejects.toThrow("invalid synchronized history attachment");
      expect(bridge.pendingAttachCancels).toEqual([`attach-${index}`]);
    }

    const bridge = new RecordingTargetBridge();
    const service = new AgentRuntimeService(bridge, remote);
    bridge.attachResult = {
      ...synchronizedAttachResult(),
      liveSessionCount: 1,
      liveSessions: [
        { sessionId: "session", liveItems: [closedLiveItem("unsafe-live", { output: "secret" })] }
      ]
    };
    await expect(
      service.beginSessionHistoryAttach("user-a", { sessionId: "session" }, () => {})
    ).rejects.toThrow("invalid synchronized live suffix");
    expect(bridge.pendingAttachCancels).toEqual(["attach-1"]);

    const oversizedBridge = new RecordingTargetBridge();
    const oversizedService = new AgentRuntimeService(oversizedBridge, remote);
    oversizedBridge.attachResult = {
      ...synchronizedAttachResult(),
      page: {
        records: [
          {
            recordId: "oversized-row",
            role: "assistant",
            createdMs: 1,
            items: Array.from({ length: 6 }, (_, index) =>
              closedLiveItem(`large-${index}`, { text: "x".repeat(192 * 1024) })
            )
          }
        ],
        historyRevision: "history-1"
      }
    };
    await expect(
      oversizedService.beginSessionHistoryAttach("user-a", { sessionId: "session" }, () => {})
    ).rejects.toThrow("invalid synchronized history attachment");
    expect(oversizedBridge.pendingAttachCancels).toEqual(["attach-1"]);
  });

  test("decodes every exact closed v1 live variant and rejects compatibility or unsafe frames", async () => {
    const bridge = new RecordingTargetBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    const frames: AgentLiveChannelFrame[] = [];
    await service.beginSessionHistoryAttach("user-a", { sessionId: "session" }, (frame) =>
      frames.push(frame)
    );
    await service.getRuntimeStatus("user-a");
    const lease = bridge.invocations.at(-1)!.lease;
    const summary = {
      id: "session",
      title: "Task",
      projectRoot: "/workspace",
      createdMs: 1,
      updatedMs: 2,
      pageSortMs: 3,
      messageCount: 4,
      mode: "smart_approve"
    };
    const variants = [
      { eventType: "runStarted", runId: "run" },
      { eventType: "timelineUpsert", runId: "run", item: closedLiveItem("message") },
      { eventType: "timelineCleared", runId: "run", reason: "run_started" },
      { eventType: "historyReplaced", runId: "run" },
      { eventType: "cursorAdvanced" },
      { eventType: "sessionUpdated", session: summary },
      { eventType: "runFinished", runId: "run", terminal: "completed" },
      { eventType: "sessionDeleted" },
      {
        eventType: "userFacingError",
        runId: "run",
        item: closedLiveItem("safe-error", {
          itemType: "error",
          role: "system",
          title: "Agent error",
          text: "The Agent task failed. Open the host for additional diagnostic details.",
          status: "failed"
        })
      },
      { eventType: "timelineCleared", reason: "explicit_reload" }
    ];
    variants.forEach((variant, index) =>
      bridge.liveHandlers[0](orderedLiveFrame(lease, remote, index + 1, variant))
    );
    expect(frames.map((frame) => frame.eventType)).toEqual([
      "runStarted",
      "timelineUpsert",
      "timelineCleared",
      "historyReplaced",
      "cursorAdvanced",
      "sessionUpdated",
      "runFinished",
      "sessionDeleted",
      "userFacingError",
      "timelineCleared"
    ]);

    const invalidFrames = [
      orderedLiveFrame(lease, remote, 11, { eventType: "timelineItem", item: closedLiveItem() }),
      orderedLiveFrame(lease, remote, 11, {
        eventType: "runFinished",
        runId: "run",
        message: "completed"
      }),
      orderedLiveFrame(lease, remote, 11, { eventType: "cursorAdvanced", runId: "run" }),
      orderedLiveFrame(lease, remote, 11, {
        eventType: "timelineCleared",
        runId: "run",
        reason: "explicit_reload"
      }),
      orderedLiveFrame(lease, remote, 11, {
        eventType: "sessionUpdated",
        session: { ...summary, id: "other" }
      }),
      orderedLiveFrame(lease, remote, 11, {
        eventType: "timelineUpsert",
        item: closedLiveItem("unsafe", { input: { token: "secret" } })
      }),
      { ...orderedLiveFrame(lease, remote, 11, { eventType: "cursorAdvanced" }), extra: true },
      {
        ...orderedLiveFrame(lease, remote, 11, { eventType: "cursorAdvanced" }),
        liveEventVersion: 2
      }
    ];
    invalidFrames.forEach((frame) => bridge.liveHandlers[0](frame));
    expect(frames).toHaveLength(10);
  });

  test("cancels and resumes an active synchronized stream from its retained cursor on lease rotation", async () => {
    const bridge = new RecordingTargetBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    const frames: AgentLiveChannelFrame[] = [];
    const pending = await service.beginSessionHistoryAttach(
      "user-a",
      { sessionId: "session" },
      (frame) => frames.push(frame)
    );
    const active = await pending.activate();
    await service.getRuntimeStatus("user-a");
    const oldLease = bridge.invocations.at(-1)!.lease;
    bridge.liveHandlers[0](orderedLiveFrame(oldLease, remote, 1, { eventType: "cursorAdvanced" }));
    expect(frames).toHaveLength(1);

    bridge.rotateLease("user-a", remote);
    bridge.resumeResult = {
      throughEventCursor: { journalId: TEST_JOURNAL_ID, sequence: 1 },
      liveStreamId: "replacement-stream"
    };
    await service.getRuntimeStatus("user-a");
    const replacementLease = bridge.liveResumes[0].lease!;

    expect(bridge.liveStreamCancels).toEqual(["attach-1"]);
    expect(bridge.liveResumes).toMatchObject([
      {
        cursor: { journalId: TEST_JOURNAL_ID, sequence: 1 },
        lease: {
          hostEpoch: replacementLease.hostEpoch,
          connectionGeneration: replacementLease.connectionGeneration
        }
      }
    ]);

    bridge.liveHandlers[0](orderedLiveFrame(oldLease, remote, 2, { eventType: "cursorAdvanced" }));
    expect(frames).toHaveLength(1);
    bridge.liveHandlers[1](
      orderedLiveFrame(replacementLease, remote, 2, { eventType: "cursorAdvanced" })
    );
    expect(frames).toHaveLength(2);
    expect(active.throughEventCursor).toEqual({ journalId: TEST_JOURNAL_ID, sequence: 2 });

    await active.cancel();
    expect(bridge.liveStreamCancels).toEqual(["attach-1", "replacement-stream"]);
  });

  test("notifies an active stream when lease replacement cannot resume", async () => {
    const bridge = new RecordingTargetBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    const frames: AgentLiveChannelFrame[] = [];
    const pending = await service.beginSessionHistoryAttach(
      "user-a",
      { sessionId: "session" },
      (frame) => frames.push(frame)
    );
    const active = await pending.activate();
    bridge.rotateLease("user-a", remote);
    bridge.resumeError = new Error("replacement socket unavailable");

    await expect(service.getRuntimeStatus("user-a")).rejects.toThrow(
      "replacement socket unavailable"
    );
    expect(bridge.liveStreamCancels).toEqual(["attach-1"]);
    expect(frames).toEqual([
      expect.objectContaining({
        eventType: "snapshotRequired",
        reason: "ordering_lost",
        lastEventCursor: { journalId: TEST_JOURNAL_ID, sequence: 0 }
      })
    ]);
    await expect(active.cancel()).resolves.toBeUndefined();
  });

  test("retains a malformed replacement ID when its cleanup fails and retries it exactly", async () => {
    const bridge = new SelectiveFailingLiveCancelBridge();
    const remote = createRemoteAgentExecutionTarget("dev_patch3_malformed_replacement");
    const service = new AgentRuntimeService(bridge, remote);
    const pending = await service.beginSessionHistoryAttach(
      "patch3-malformed-account",
      { sessionId: "session" },
      () => {}
    );
    const active = await pending.activate();
    bridge.rotateLease("patch3-malformed-account", remote);
    bridge.resumeResult = {
      throughEventCursor: { journalId: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", sequence: 0 },
      liveStreamId: "malformed-replacement"
    };
    const cleanupError = new Error("replacement cleanup failed");
    bridge.failNextCancellation("malformed-replacement", cleanupError);

    await expect(service.getRuntimeStatus("patch3-malformed-account")).rejects.toBe(cleanupError);
    expect(bridge.liveStreamCancels).toEqual(["attach-1", "malformed-replacement"]);

    bridge.resumeResult = {
      throughEventCursor: { journalId: TEST_JOURNAL_ID, sequence: 0 },
      liveStreamId: "recovered-replacement"
    };
    await expect(service.getRuntimeStatus("patch3-malformed-account")).resolves.toEqual({
      running: false
    });
    expect(bridge.liveStreamCancels).toEqual([
      "attach-1",
      "malformed-replacement",
      "malformed-replacement"
    ]);
    await active.cancel();
    expect(bridge.liveStreamCancels.at(-1)).toBe("recovered-replacement");
  });

  test("retains a stale late-opened replacement ID when exact cleanup fails", async () => {
    const bridge = new ControlledResumeBridge();
    const remote = createRemoteAgentExecutionTarget("dev_patch3_stale_replacement");
    const service = new AgentRuntimeService(bridge, remote);
    const pending = await service.beginSessionHistoryAttach(
      "patch3-stale-account-a",
      { sessionId: "session" },
      () => {}
    );
    await pending.activate();
    bridge.rotateLease("patch3-stale-account-a", remote);

    const refreshA = service.getRuntimeStatus("patch3-stale-account-a");
    await waitFor(() => bridge.pendingResumes.length === 1);
    let accountBSettled = false;
    const accountB = service.getRuntimeStatus("patch3-stale-account-b").finally(() => {
      accountBSettled = true;
    });
    await Promise.resolve();
    expect(accountBSettled).toBe(false);

    const cleanupError = new Error("stale replacement cleanup failed");
    bridge.failNextCancellation("stale-replacement", cleanupError);
    bridge.resolveResume(0, {
      throughEventCursor: { journalId: TEST_JOURNAL_ID, sequence: 0 },
      liveStreamId: "stale-replacement"
    });
    await expect(refreshA).rejects.toBe(cleanupError);
    await expect(accountB).resolves.toEqual({ running: false });
    expect(bridge.liveStreamCancels).toEqual([
      "attach-1",
      "stale-replacement",
      "stale-replacement"
    ]);

    const replacement = await service.beginSessionHistoryAttach(
      "patch3-stale-account-a",
      { sessionId: "session" },
      () => {}
    );
    expect(bridge.liveStreamCancels).toEqual([
      "attach-1",
      "stale-replacement",
      "stale-replacement"
    ]);
    await replacement.cancel();
  });

  test("retries an old binding cancellation failure before a later lease replacement", async () => {
    const bridge = new SelectiveFailingLiveCancelBridge();
    const remote = createRemoteAgentExecutionTarget("dev_patch3_old_binding");
    const service = new AgentRuntimeService(bridge, remote);
    const pending = await service.beginSessionHistoryAttach(
      "patch3-old-binding-account",
      { sessionId: "session" },
      () => {}
    );
    const active = await pending.activate();
    const cleanupError = new Error("old binding cleanup failed");
    bridge.failNextCancellation("attach-1", cleanupError);
    bridge.rotateLease("patch3-old-binding-account", remote);
    bridge.resumeResult = {
      throughEventCursor: { journalId: TEST_JOURNAL_ID, sequence: 0 },
      liveStreamId: "replacement-after-retry"
    };

    await expect(service.getRuntimeStatus("patch3-old-binding-account")).rejects.toBe(cleanupError);
    expect(bridge.liveResumes).toHaveLength(0);
    await expect(service.getRuntimeStatus("patch3-old-binding-account")).resolves.toEqual({
      running: false
    });
    expect(bridge.liveStreamCancels.slice(0, 2)).toEqual(["attach-1", "attach-1"]);
    expect(bridge.liveResumes).toHaveLength(1);
    await active.cancel();
  });

  test("shares one exact native result across double cancel and account retirement", async () => {
    const bridge = new ControlledLiveCancelBridge();
    const remote = createRemoteAgentExecutionTarget("dev_patch3_concurrent_cancel");
    const service = new AgentRuntimeService(bridge, remote);
    const active = await service.resumeLiveEvents(
      "patch3-concurrent-cancel-account",
      { journalId: TEST_JOURNAL_ID, sequence: 7 },
      () => {}
    );

    const firstCancel = active.cancel();
    await waitFor(() => bridge.pendingLiveCancels.length === 1);
    const secondCancel = active.cancel();
    const retirement = service.retireAccount("patch3-concurrent-cancel-account");
    await Promise.resolve();
    expect(bridge.pendingLiveCancels).toHaveLength(1);
    expect(bridge.liveStreamCancels).toEqual(["stream-1"]);

    bridge.pendingLiveCancels[0].resolve();
    await expect(Promise.all([firstCancel, secondCancel, retirement])).resolves.toEqual([
      undefined,
      undefined,
      undefined
    ]);
    await expect(active.cancel()).resolves.toBeUndefined();
    expect(bridge.liveStreamCancels).toEqual(["stream-1"]);
  });

  test("active cancel waits for an in-flight replacement and retries its exact ID", async () => {
    const bridge = new ControlledResumeBridge();
    const remote = createRemoteAgentExecutionTarget("dev_patch3_cancel_during_replacement");
    const service = new AgentRuntimeService(bridge, remote);
    const pending = await service.beginSessionHistoryAttach(
      "patch3-cancel-during-replacement-account",
      { sessionId: "session" },
      () => {}
    );
    const active = await pending.activate();
    bridge.rotateLease("patch3-cancel-during-replacement-account", remote);

    const refresh = service.getRuntimeStatus("patch3-cancel-during-replacement-account");
    const refreshOutcome = refresh.then(
      () => null,
      (error: unknown) => error
    );
    await waitFor(() => bridge.pendingResumes.length === 1);
    let cancelSettled = false;
    const cancelOutcome = active
      .cancel()
      .then(
        () => null,
        (error: unknown) => error
      )
      .finally(() => {
        cancelSettled = true;
      });
    await Promise.resolve();
    expect(cancelSettled).toBe(false);

    const cleanupError = new Error("late replacement cleanup failed");
    bridge.failNextCancellation("late-replacement", cleanupError);
    bridge.failNextCancellation("late-replacement", cleanupError);
    bridge.resolveResume(0, {
      throughEventCursor: { journalId: TEST_JOURNAL_ID, sequence: 0 },
      liveStreamId: "late-replacement"
    });
    expect(await refreshOutcome).toBe(cleanupError);
    expect(await cancelOutcome).toBe(cleanupError);
    expect(bridge.liveStreamCancels).toEqual(["attach-1", "late-replacement", "late-replacement"]);

    await expect(active.cancel()).resolves.toBeUndefined();
    expect(bridge.liveStreamCancels.at(-1)).toBe("late-replacement");
  });

  test("same-lease remount retries a failed final unmount cleanup before opening", async () => {
    const bridge = new SelectiveFailingLiveCancelBridge();
    const remote = createRemoteAgentExecutionTarget("dev_patch3_same_lease_remount");
    const service = new AgentRuntimeService(bridge, remote);
    const active = await service.resumeLiveEvents(
      "patch3-remount-account",
      { journalId: TEST_JOURNAL_ID, sequence: 7 },
      () => {}
    );
    const cleanupError = new Error("unmount cleanup failed");
    bridge.failNextCancellation("stream-1", cleanupError);

    await expect(active.cancel()).rejects.toBe(cleanupError);
    const replacementService = new AgentRuntimeService(bridge, remote);
    const replacement = await replacementService.beginSessionHistoryAttach(
      "patch3-remount-account",
      { sessionId: "session" },
      () => {}
    );
    expect(bridge.liveStreamCancels).toEqual(["stream-1", "stream-1"]);
    await replacement.cancel();
  });

  test("retains activation rejection cleanup after an earlier pending cancel", async () => {
    const bridge = new FailingLateStreamCancelBridge();
    const remote = createRemoteAgentExecutionTarget("dev_patch3_rejected_activation_cleanup");
    const service = new AgentRuntimeService(bridge, remote);
    const pending = await service.beginSessionHistoryAttach(
      "patch3-rejected-activation-account",
      { sessionId: "session" },
      () => {}
    );
    const activation = pending.activate();
    await waitFor(() => bridge.pendingActivations.length === 1);
    await pending.cancel();

    const cleanupError = new Error("rejected activation cleanup failed");
    bridge.cancelError = cleanupError;
    bridge.pendingActivations[0].reject(new Error("activation transport failed"));
    await expect(activation).rejects.toBe(cleanupError);
    expect(bridge.liveStreamCancels).toEqual(["attach-1"]);

    await expect(pending.cancel()).resolves.toBeUndefined();
    expect(bridge.liveStreamCancels).toEqual(["attach-1", "attach-1"]);
  });

  test("an ambiguous activation rejection drains both pending and live exact IDs", async () => {
    const bridge = new ControlledAttachBridge();
    const remote = createRemoteAgentExecutionTarget("dev_patch3_ambiguous_activation");
    const service = new AgentRuntimeService(bridge, remote);
    const pending = await service.beginSessionHistoryAttach(
      "patch3-ambiguous-activation-account",
      { sessionId: "session" },
      () => {}
    );
    const activation = pending.activate();
    await waitFor(() => bridge.pendingActivations.length === 1);

    bridge.pendingActivations[0].reject(new Error("activation transport failed"));
    await expect(activation).rejects.toThrow("activation transport failed");
    expect(bridge.pendingAttachCancels).toEqual(["attach-1"]);
    expect(bridge.liveStreamCancels).toEqual(["attach-1"]);
  });

  test("cancels a stream whose activation resolves after owner teardown", async () => {
    const bridge = new ControlledAttachBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    const pending = await service.beginSessionHistoryAttach(
      "user-a",
      { sessionId: "session" },
      () => {}
    );
    const activation = pending.activate();
    await waitFor(() => bridge.pendingActivations.length === 1);

    await pending.cancel();
    bridge.pendingActivations[0].resolve(bridge.activateResult);
    await expect(activation).rejects.toThrow("cancelled during activation");
    expect(bridge.pendingAttachCancels).toEqual(["attach-1"]);
    expect(bridge.liveStreamCancels).toEqual(["attach-1"]);
  });

  test("retains and propagates a late activated stream whose cancellation fails", async () => {
    const bridge = new FailingLateStreamCancelBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    const pending = await service.beginSessionHistoryAttach(
      "user-a",
      { sessionId: "session" },
      () => {}
    );
    const activation = pending.activate();
    await waitFor(() => bridge.pendingActivations.length === 1);

    await pending.cancel();
    const cancellationError = new Error("live cleanup failed");
    bridge.cancelError = cancellationError;
    bridge.pendingActivations[0].resolve(bridge.activateResult);
    await expect(activation).rejects.toBe(cancellationError);

    await pending.cancel();
    expect(bridge.pendingAttachCancels).toEqual(["attach-1"]);
    expect(bridge.liveStreamCancels).toEqual(["attach-1", "attach-1"]);
  });

  test("retains malformed activation cleanup until its non-benign failure is retried", async () => {
    const bridge = new FailingLateStreamCancelBridge();
    bridge.activateResult = {
      throughEventCursor: { journalId: TEST_JOURNAL_ID, sequence: 0 },
      liveStreamId: "wrong-stream"
    };
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    const pending = await service.beginSessionHistoryAttach(
      "user-a",
      { sessionId: "session" },
      () => {}
    );
    const activation = pending.activate();
    await waitFor(() => bridge.pendingActivations.length === 1);

    const cancellationError = new Error("malformed activation cleanup failed");
    bridge.cancelError = cancellationError;
    bridge.pendingActivations[0].resolve(bridge.activateResult);
    await expect(activation).rejects.toBe(cancellationError);

    await pending.cancel();
    expect(bridge.liveStreamCancels).toEqual(["attach-1", "attach-1"]);
  });

  test("decodes only exact stamped snapshot-required control frames", async () => {
    const bridge = new RecordingTargetBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    const frames: AgentLiveChannelFrame[] = [];
    await service.beginSessionHistoryAttach("user-a", { sessionId: "session" }, (frame) =>
      frames.push(frame)
    );
    await service.getRuntimeStatus("user-a");
    const lease = bridge.invocations.at(-1)!.lease;
    const base = {
      liveEventVersion: 1,
      eventType: "snapshotRequired",
      targetId: remote.id,
      hostEpoch: lease.hostEpoch,
      connectionGeneration: lease.connectionGeneration,
      lastEventCursor: { journalId: TEST_JOURNAL_ID, sequence: 7 }
    } as const;

    bridge.liveHandlers[0]({ ...base, reason: "pausedSubscriberOverflow" });
    bridge.liveHandlers[0]({ ...base, reason: "paused_overflow", hostEpoch: "wrong" });
    bridge.liveHandlers[0]({ ...base, reason: "paused_overflow", liveEventVersion: 2 });
    const missingVersion = { ...base } as Record<string, unknown>;
    delete missingVersion.liveEventVersion;
    bridge.liveHandlers[0]({ ...missingVersion, reason: "paused_overflow" });
    bridge.liveHandlers[0]({ ...base, reason: "paused_overflow", extra: true });
    expect(frames).toEqual([]);
    bridge.liveHandlers[0]({ ...base, reason: "paused_overflow" });
    expect(frames).toEqual([{ ...base, reason: "paused_overflow" }]);
  });

  test("fails a pending attach closed by the native TTL without inventing a stream", async () => {
    const bridge = new RecordingTargetBridge();
    bridge.activateError = { code: "attach_not_found" };
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    const pending = await service.beginSessionHistoryAttach(
      "user-a",
      { sessionId: "session" },
      () => {}
    );

    await expect(pending.activate()).rejects.toEqual({ code: "attach_not_found" });
    // Activation may consume or expire the pending token; exact active cleanup
    // remains safe because activated attaches use attachId as liveStreamId.
    expect(bridge.liveStreamCancels).toEqual(["attach-1"]);
  });

  test("preserves the typed oversized-history-record failure", async () => {
    const bridge = new RecordingTargetBridge();
    bridge.attachError = { code: "history_record_too_large" };
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);

    await expect(
      service.beginSessionHistoryAttach("user-a", { sessionId: "session" }, () => {})
    ).rejects.toMatchObject({
      name: "AgentHistoryRecordTooLargeError",
      message: "An Agent history record is too large to present safely"
    });
  });

  test("resumes from an exact cursor and normalizes snapshot-required errors", async () => {
    const bridge = new RecordingTargetBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    const frames: AgentLiveChannelFrame[] = [];
    const active = await service.resumeLiveEvents(
      "user-a",
      { journalId: TEST_JOURNAL_ID, sequence: 7 },
      (frame) => frames.push(frame)
    );
    expect(active).toMatchObject({
      liveStreamId: "stream-1",
      throughEventCursor: { journalId: TEST_JOURNAL_ID, sequence: 7 }
    });
    await service.getRuntimeStatus("user-a");
    const lease = bridge.invocations.at(-1)!.lease;
    bridge.liveHandlers[0]({
      liveEventVersion: 1,
      eventType: "runStarted",
      targetId: remote.id,
      hostEpoch: lease.hostEpoch,
      connectionGeneration: lease.connectionGeneration,
      eventEpoch: TEST_JOURNAL_ID,
      eventSequence: 8,
      sessionId: "session",
      runId: "resumed"
    });
    expect(frames).toMatchObject([{ eventType: "runStarted", runId: "resumed" }]);
    await active.cancel();
    expect(bridge.liveStreamCancels).toContain("stream-1");

    bridge.resumeError = {
      code: "snapshot_required",
      reason: "retention_gap",
      lastEventCursor: { journalId: TEST_JOURNAL_ID, sequence: 7 }
    };
    await expect(
      service.resumeLiveEvents("user-a", { journalId: TEST_JOURNAL_ID, sequence: 7 }, () => {})
    ).rejects.toMatchObject({
      name: "AgentLiveSnapshotRequiredError",
      reason: "retention_gap",
      lastEventCursor: { journalId: TEST_JOURNAL_ID, sequence: 7 }
    });
    await expect(
      service.resumeLiveEvents("user-a", { journalId: "unsafe", sequence: 7 }, () => {})
    ).rejects.toThrow("cursor is invalid");
  });

  test("keeps synchronized attach unavailable on the unverified embedded compatibility path", async () => {
    const service = new AgentRuntimeService(new RecordingBridge());
    await expect(
      service.beginSessionHistoryAttach("user-a", { sessionId: "session" }, () => {})
    ).rejects.toThrow("requires a verified remote host connection stamp");
  });

  test("normalizes targetless legacy events only for the embedded target", async () => {
    const bridge = new RecordingTargetBridge();
    const service = new AgentRuntimeService(bridge);
    const events: AgentEventEnvelope[] = [];

    await service.listenToEvents((event) => events.push(event));
    bridge.emit(LOCAL_AGENT_EXECUTION_TARGET, {
      eventType: "runStarted",
      sessionId: "session-1",
      runId: "run-1"
    });

    expect(events).toEqual([
      {
        eventType: "runStarted",
        targetId: LOCAL_AGENT_EXECUTION_TARGET.id,
        connectionGeneration: 0,
        sessionId: "session-1",
        runId: "run-1"
      }
    ]);
    expect(bridge.subscriptions).toEqual([{ lease: null, target: LOCAL_AGENT_EXECUTION_TARGET }]);
  });

  test("requires an account and event-capable bridge for remote subscriptions", async () => {
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const eventlessService = new AgentRuntimeService(new RecordingBridge(), remote);

    await expect(eventlessService.listenToEvents(() => {})).rejects.toThrow(
      "Remote Agent event subscription requires an authenticated user"
    );
    await expect(eventlessService.listenToEvents("user-a", () => {})).rejects.toThrow(
      "does not support events for remote target"
    );
  });

  test("keeps one logical subscription alive when its connection generation changes", async () => {
    const bridge = new RecordingTargetBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    const events: AgentEventEnvelope[] = [];

    const unlisten = await service.listenToEvents("user-a", (event) => events.push(event));
    const oldLease = bridge.subscriptions[0].lease!;
    expect(Object.isFrozen(oldLease)).toBe(true);

    bridge.rotateLease("user-a", remote);
    await service.getRuntimeStatus("user-a");
    const resumedLease = bridge.invocations[bridge.invocations.length - 1].lease;
    expect(resumedLease.connectionGeneration).not.toBe(oldLease.connectionGeneration);

    bridge.emit(remote, {
      eventType: "runStarted",
      targetId: remote.id,
      connectionGeneration: oldLease.connectionGeneration,
      sessionId: "session-1",
      runId: "run-1"
    });
    expect(events).toEqual([]);

    const subscribedResumedLease = bridge.subscriptions[bridge.subscriptions.length - 1].lease!;
    expect(subscribedResumedLease).toMatchObject({
      accountId: "user-a",
      targetId: remote.id,
      hostEpoch: resumedLease.hostEpoch,
      connectionGeneration: resumedLease.connectionGeneration
    });
    bridge.emit(remote, {
      eventType: "runStarted",
      targetId: remote.id,
      hostEpoch: subscribedResumedLease.hostEpoch,
      connectionGeneration: subscribedResumedLease.connectionGeneration,
      eventEpoch: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      eventSequence: 1,
      sessionId: "session-1",
      runId: "run-2"
    });
    expect(events).toHaveLength(1);
    expect(events[0]).toMatchObject({
      connectionGeneration: subscribedResumedLease.connectionGeneration,
      runId: "run-2"
    });

    unlisten();
    bridge.emit(remote, {
      eventType: "runStarted",
      targetId: remote.id,
      connectionGeneration: subscribedResumedLease.connectionGeneration,
      sessionId: "session-1",
      runId: "run-3"
    });
    expect(events).toHaveLength(1);
  });

  test("retires account A's lease when the same target service switches to account B", async () => {
    const bridge = new RecordingTargetBridge();
    const remote = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const service = new AgentRuntimeService(bridge, remote);
    const accountAEvents: AgentEventEnvelope[] = [];
    const accountBEvents: AgentEventEnvelope[] = [];

    await service.listenToEvents("account-a", (event) => accountAEvents.push(event));
    const accountALease = bridge.subscriptions[bridge.subscriptions.length - 1].lease!;
    await service.listenToEvents("account-b", (event) => accountBEvents.push(event));
    const accountBLease = bridge.subscriptions[bridge.subscriptions.length - 1].lease!;

    expect(accountALease.accountId).toBe("account-a");
    expect(accountBLease.accountId).toBe("account-b");
    expect(accountBLease.hostEpoch).not.toBe(accountALease.hostEpoch);

    bridge.emit(remote, {
      eventType: "runStarted",
      targetId: remote.id,
      connectionGeneration: accountALease.connectionGeneration,
      sessionId: "same-session",
      runId: "same-run"
    });
    expect(accountAEvents).toEqual([]);
    expect(accountBEvents).toEqual([]);

    bridge.emit(remote, {
      eventType: "runStarted",
      targetId: remote.id,
      hostEpoch: accountBLease.hostEpoch,
      connectionGeneration: accountBLease.connectionGeneration,
      eventEpoch: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      eventSequence: 1,
      sessionId: "same-session",
      runId: "same-run"
    });
    expect(accountAEvents).toEqual([]);
    expect(accountBEvents).toHaveLength(1);
  });

  test("namespaces identical session and run IDs by execution target", async () => {
    const bridge = new RecordingTargetBridge();
    const macbook = createRemoteAgentExecutionTarget("dev_01J4Z3N9Y5K7QX2P8B6C0R1TWA");
    const studio = createRemoteAgentExecutionTarget("dev_01J4Z3PBVAD2S6M8H0FQ9C7XKE");
    const baseService = new AgentRuntimeService(bridge);
    const macbookService = baseService.forTarget(macbook);
    const studioService = baseService.forTarget(studio);
    const macbookEvents: AgentEventEnvelope[] = [];
    const studioEvents: AgentEventEnvelope[] = [];

    await macbookService.listenToEvents("user-a", (event) => macbookEvents.push(event));
    await studioService.listenToEvents("user-a", (event) => studioEvents.push(event));
    const macbookLease = bridge.subscriptions[0].lease!;
    const studioLease = bridge.subscriptions[1].lease!;
    await macbookService.cancelRun("user-a", "shared-run");
    await studioService.cancelRun("user-a", "shared-run");

    // Targetless legacy events must never be inferred onto a remote target.
    bridge.emit(macbook, {
      eventType: "runStarted",
      sessionId: "shared-session",
      runId: "shared-run"
    });
    expect(macbookEvents).toEqual([]);

    bridge.emit(macbook, {
      eventType: "runStarted",
      targetId: macbook.id,
      hostEpoch: macbookLease.hostEpoch,
      connectionGeneration: macbookLease.connectionGeneration,
      sessionId: "shared-session"
    });
    expect(macbookEvents).toEqual([]);

    bridge.emit(macbook, {
      eventType: "runStarted",
      targetId: macbook.id,
      connectionGeneration: macbookLease.connectionGeneration,
      sessionId: 7,
      runId: "shared-run"
    });
    expect(macbookEvents).toEqual([]);

    bridge.emit(macbook, {
      eventType: "runStarted",
      targetId: macbook.id,
      hostEpoch: macbookLease.hostEpoch,
      connectionGeneration: macbookLease.connectionGeneration,
      eventEpoch: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      eventSequence: 1,
      sessionId: "shared-session",
      runId: "shared-run"
    });
    expect(macbookEvents).toEqual([
      {
        eventType: "runStarted",
        targetId: macbook.id,
        hostEpoch: macbookLease.hostEpoch,
        connectionGeneration: macbookLease.connectionGeneration,
        eventEpoch: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        eventSequence: 1,
        sessionId: "shared-session",
        runId: "shared-run"
      }
    ]);
    expect(studioEvents).toEqual([]);

    bridge.emit(studio, {
      eventType: "runStarted",
      targetId: studio.id,
      hostEpoch: studioLease.hostEpoch,
      connectionGeneration: studioLease.connectionGeneration,
      eventEpoch: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
      eventSequence: 1,
      sessionId: "shared-session",
      runId: "shared-run"
    });
    expect(studioEvents).toEqual([
      {
        eventType: "runStarted",
        targetId: studio.id,
        hostEpoch: studioLease.hostEpoch,
        connectionGeneration: studioLease.connectionGeneration,
        eventEpoch: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        eventSequence: 1,
        sessionId: "shared-session",
        runId: "shared-run"
      }
    ]);

    // Even if a bridge delivers an explicitly mis-tagged event on the wrong
    // subscription, the target-bound service rejects it.
    bridge.emit(macbook, {
      eventType: "runFinished",
      targetId: studio.id,
      connectionGeneration: studioLease.connectionGeneration,
      sessionId: "shared-session",
      runId: "shared-run"
    });
    expect(macbookEvents).toHaveLength(1);

    bridge.emit(macbook, {
      eventType: "runFinished",
      targetId: macbook.id,
      hostEpoch: macbookLease.hostEpoch,
      connectionGeneration: macbookLease.connectionGeneration,
      eventEpoch: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      eventSequence: 2,
      sessionId: "shared-session",
      runId: "shared-run",
      message: "completed"
    });
    expect(macbookEvents[1]).toEqual({
      eventType: "runFinished",
      targetId: macbook.id,
      hostEpoch: macbookLease.hostEpoch,
      connectionGeneration: macbookLease.connectionGeneration,
      eventEpoch: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      eventSequence: 2,
      sessionId: "shared-session",
      runId: "shared-run",
      message: "completed"
    });

    expect(bridge.invocations).toEqual([
      {
        lease: macbookLease,
        invocation: {
          operation: "cancelRun",
          request: { runId: "shared-run" }
        }
      },
      {
        lease: studioLease,
        invocation: {
          operation: "cancelRun",
          request: { runId: "shared-run" }
        }
      }
    ]);
    expect(bridge.fencedTargets).toEqual([macbook, studio, macbook, studio]);
    expect(bridge.subscriptions).toEqual([
      { lease: macbookLease, target: macbook },
      { lease: studioLease, target: studio }
    ]);
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
