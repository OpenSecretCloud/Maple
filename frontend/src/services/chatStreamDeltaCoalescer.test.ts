import { describe, expect, test } from "bun:test";
import {
  classifyChatStreamEof,
  createChatStreamDeltaCoalescer,
  flushRegisteredChatStreamDeltas,
  isTerminalChatStreamErrorEvent,
  registerChatStreamDeltaCoalescer,
  removeOwnedChatStreamAttemptItems,
  unregisterChatStreamDeltaCoalescer,
  type ChatStreamDeltaScheduler,
  type ChatStreamTextDelta
} from "./chatStreamDeltaCoalescer";

function createManualScheduler() {
  let nextHandle = 0;
  const tasks = new Map<number, () => void>();
  const scheduler: ChatStreamDeltaScheduler = {
    schedule: (task) => {
      const handle = ++nextHandle;
      tasks.set(handle, task);
      return handle;
    },
    cancel: (handle) => tasks.delete(handle as number)
  };

  return {
    scheduler,
    pendingCount: () => tasks.size,
    runNext: () => {
      const next = tasks.entries().next().value as [number, () => void] | undefined;
      if (!next) return false;
      tasks.delete(next[0]);
      next[1]();
      return true;
    }
  };
}

function delta(text: string, overrides: Partial<ChatStreamTextDelta> = {}): ChatStreamTextDelta {
  return {
    kind: "message",
    itemId: "assistant",
    contentIndex: 0,
    delta: text,
    ...overrides
  };
}

describe("createChatStreamDeltaCoalescer", () => {
  test("removes only a failed attempt's response items before retry", () => {
    const userMessage = { id: "local-user", kind: "user" };
    const previousAssistant = { id: "previous-assistant", kind: "assistant" };
    const failedAssistant = { id: "failed-assistant", kind: "assistant" };
    const failedReasoning = { id: "failed-reasoning", kind: "reasoning" };
    const otherRuntimeItem = { id: "other-runtime-item", kind: "assistant" };

    expect(
      removeOwnedChatStreamAttemptItems(
        [userMessage, previousAssistant, failedAssistant, failedReasoning, otherRuntimeItem],
        new Set([failedAssistant.id, failedReasoning.id])
      )
    ).toEqual([userMessage, previousAssistant, otherRuntimeItem]);
  });

  test("distinguishes completed, truncated, and stale-cancel iterator EOF", () => {
    expect(classifyChatStreamEof("completed", true)).toBe("completed");
    expect(classifyChatStreamEof(null, true)).toBe("truncated");
    expect(classifyChatStreamEof(null, false)).toBe("stale");
    expect(classifyChatStreamEof("cancelled", true)).toBe("terminal");
    expect(classifyChatStreamEof("error", true)).toBe("terminal");
  });

  test("recognizes OpenAI and OpenSecret terminal stream-error event names", () => {
    expect(isTerminalChatStreamErrorEvent("error")).toBe(true);
    expect(isTerminalChatStreamErrorEvent("response.error")).toBe(true);
    expect(isTerminalChatStreamErrorEvent("response.failed")).toBe(true);
    expect(isTerminalChatStreamErrorEvent("response.cancelled")).toBe(false);
    expect(isTerminalChatStreamErrorEvent("response.completed")).toBe(false);
  });

  test("preserves chunk order while publishing only once per scheduled batch", () => {
    const manual = createManualScheduler();
    const batches: ChatStreamTextDelta[][] = [];
    const coalescer = createChatStreamDeltaCoalescer({
      scheduler: manual.scheduler,
      isCurrent: () => true,
      onFlush: (batch) => batches.push(batch)
    });

    coalescer.enqueue(delta("one "));
    coalescer.enqueue(delta("two "));
    coalescer.enqueue(delta("three"));

    expect(manual.pendingCount()).toBe(1);
    expect(batches).toEqual([]);
    manual.runNext();
    expect(batches).toEqual([[delta("one two three")]]);
  });

  test("keeps message, reasoning, item, and content-index targets independent", () => {
    const manual = createManualScheduler();
    const batches: ChatStreamTextDelta[][] = [];
    const coalescer = createChatStreamDeltaCoalescer({
      scheduler: manual.scheduler,
      isCurrent: () => true,
      onFlush: (batch) => batches.push(batch)
    });

    coalescer.enqueue(delta("m0-a"));
    coalescer.enqueue(delta("r0", { kind: "reasoning" }));
    coalescer.enqueue(delta("m1", { contentIndex: 1 }));
    coalescer.enqueue(delta("m0-b"));

    manual.runNext();
    expect(batches).toEqual([
      [delta("m0-am0-b"), delta("r0", { kind: "reasoning" }), delta("m1", { contentIndex: 1 })]
    ]);
  });

  test("keeps concurrent run queues and timers independent", () => {
    const manual = createManualScheduler();
    const batchesA: ChatStreamTextDelta[][] = [];
    const batchesB: ChatStreamTextDelta[][] = [];
    const coalescerA = createChatStreamDeltaCoalescer({
      scheduler: manual.scheduler,
      isCurrent: () => true,
      onFlush: (batch) => batchesA.push(batch)
    });
    const coalescerB = createChatStreamDeltaCoalescer({
      scheduler: manual.scheduler,
      isCurrent: () => true,
      onFlush: (batch) => batchesB.push(batch)
    });

    coalescerA.enqueue(delta("A"));
    coalescerB.enqueue(delta("B"));
    expect(manual.pendingCount()).toBe(2);

    manual.runNext();
    expect(batchesA).toEqual([[delta("A")]]);
    expect(batchesB).toEqual([]);
    manual.runNext();
    expect(batchesB).toEqual([[delta("B")]]);
  });

  test("flushes synchronously for terminal processing and cancels the timer", () => {
    const manual = createManualScheduler();
    const batches: ChatStreamTextDelta[][] = [];
    const coalescer = createChatStreamDeltaCoalescer({
      scheduler: manual.scheduler,
      isCurrent: () => true,
      onFlush: (batch) => batches.push(batch)
    });

    coalescer.enqueue(delta("terminal text"));
    expect(coalescer.flush()).toBe(true);
    expect(batches).toEqual([[delta("terminal text")]]);
    expect(manual.pendingCount()).toBe(0);
    expect(manual.runNext()).toBe(false);
  });

  test("can flush the last partial frame before run ownership is cancelled", () => {
    const manual = createManualScheduler();
    const batches: ChatStreamTextDelta[][] = [];
    let isCurrent = true;
    const coalescer = createChatStreamDeltaCoalescer({
      scheduler: manual.scheduler,
      isCurrent: () => isCurrent,
      onFlush: (batch) => batches.push(batch)
    });

    coalescer.enqueue(delta("visible before stop"));
    coalescer.flush();
    isCurrent = false;

    expect(batches).toEqual([[delta("visible before stop")]]);
    expect(manual.pendingCount()).toBe(0);
  });

  test("finish flushes synchronously, closes the queue, and rejects later deltas", () => {
    const manual = createManualScheduler();
    const batches: ChatStreamTextDelta[][] = [];
    const coalescer = createChatStreamDeltaCoalescer({
      scheduler: manual.scheduler,
      isCurrent: () => true,
      onFlush: (batch) => batches.push(batch)
    });

    coalescer.enqueue(delta("last"));
    expect(coalescer.finish()).toBe(true);
    coalescer.enqueue(delta("too late"));

    expect(coalescer.isClosed()).toBe(true);
    expect(batches).toEqual([[delta("last")]]);
    expect(manual.pendingCount()).toBe(0);
  });

  test("a thrown stream can flush its final partial frame before terminal status", () => {
    const manual = createManualScheduler();
    const rendered = { text: "", status: "streaming" };
    const coalescer = createChatStreamDeltaCoalescer({
      scheduler: manual.scheduler,
      isCurrent: () => true,
      onFlush: (batch) => {
        rendered.text += batch.map((item) => item.delta).join("");
      }
    });

    coalescer.enqueue(delta("final partial"));
    coalescer.finish();
    rendered.status = "error";

    expect(rendered).toEqual({ text: "final partial", status: "error" });
    expect(manual.pendingCount()).toBe(0);
  });

  test("drops delayed deltas after the owning run becomes stale", () => {
    const manual = createManualScheduler();
    const batches: ChatStreamTextDelta[][] = [];
    let isCurrent = true;
    const coalescer = createChatStreamDeltaCoalescer({
      scheduler: manual.scheduler,
      isCurrent: () => isCurrent,
      onFlush: (batch) => batches.push(batch)
    });

    coalescer.enqueue(delta("stale"));
    isCurrent = false;
    manual.runNext();

    expect(batches).toEqual([]);
    expect(coalescer.hasPending()).toBe(false);
    expect(coalescer.isClosed()).toBe(true);
  });

  test("discard prevents a cancelled run's queued task from firing", () => {
    const manual = createManualScheduler();
    const batches: ChatStreamTextDelta[][] = [];
    const coalescer = createChatStreamDeltaCoalescer({
      scheduler: manual.scheduler,
      isCurrent: () => true,
      onFlush: (batch) => batches.push(batch)
    });

    coalescer.enqueue(delta("cancelled"));
    coalescer.discard();

    expect(manual.pendingCount()).toBe(0);
    expect(manual.runNext()).toBe(false);
    expect(batches).toEqual([]);
  });

  test("a remounted consumer can preflush a run registered by the previous mount", () => {
    const persistentRuntimeStore = {};
    let flushCount = 0;
    const mountACoalescer = {
      flush: () => {
        flushCount += 1;
        return true;
      }
    };

    registerChatStreamDeltaCoalescer(persistentRuntimeStore, 17, mountACoalescer);
    // A new UnifiedChat mount has only the persistent store and run token.
    expect(flushRegisteredChatStreamDeltas(persistentRuntimeStore, 17)).toBe(true);
    expect(flushCount).toBe(1);
    expect(unregisterChatStreamDeltaCoalescer(persistentRuntimeStore, 17, mountACoalescer)).toBe(
      true
    );
    expect(flushRegisteredChatStreamDeltas(persistentRuntimeStore, 17)).toBe(false);
  });

  test("identity-safe unregister cannot remove a replacement for the same token", () => {
    const persistentRuntimeStore = {};
    let replacementFlushes = 0;
    const obsolete = { flush: () => true };
    const replacement = {
      flush: () => {
        replacementFlushes += 1;
        return true;
      }
    };

    registerChatStreamDeltaCoalescer(persistentRuntimeStore, 23, obsolete);
    registerChatStreamDeltaCoalescer(persistentRuntimeStore, 23, replacement);

    expect(unregisterChatStreamDeltaCoalescer(persistentRuntimeStore, 23, obsolete)).toBe(false);
    expect(flushRegisteredChatStreamDeltas(persistentRuntimeStore, 23)).toBe(true);
    expect(replacementFlushes).toBe(1);
    expect(unregisterChatStreamDeltaCoalescer(persistentRuntimeStore, 23, replacement)).toBe(true);
  });
});
