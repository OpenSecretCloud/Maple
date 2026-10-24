import { describe, expect, test } from "bun:test";
import { isCompactionMetadataUnsupported } from "./chatCompactionApi";
import {
  startChatCompactionPolling,
  type ChatCompactionPollScheduler
} from "./chatCompactionPolling";

function manualScheduler() {
  let nextHandle = 0;
  const pending = new Map<number, { callback: () => void; delayMs: number }>();
  const scheduler: ChatCompactionPollScheduler = {
    schedule: (callback, delayMs) => {
      const handle = ++nextHandle;
      pending.set(handle, { callback, delayMs });
      return handle;
    },
    cancel: (handle) => {
      pending.delete(handle as number);
    }
  };
  return {
    scheduler,
    pending,
    next() {
      const first = pending.entries().next().value;
      if (!first) throw new Error("No scheduled compaction poll");
      const [handle, task] = first;
      pending.delete(handle);
      task.callback();
      return task.delayMs;
    },
    delay() {
      return pending.values().next().value?.delayMs;
    }
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

async function settle() {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

describe("advisory compaction polling", () => {
  test("waits for POST registration, then polls at a short interval", async () => {
    const clock = manualScheduler();
    const controller = new AbortController();
    const statuses: string[] = [];
    let reads = 0;
    const stop = startChatCompactionPolling({
      read: async () => {
        reads += 1;
        return "compacting";
      },
      onStatus: (status) => statuses.push(status),
      isCurrent: () => true,
      signal: controller.signal,
      isUnsupported: isCompactionMetadataUnsupported,
      scheduler: clock.scheduler
    });
    expect(reads).toBe(0);
    expect(clock.next()).toBe(500);
    await settle();
    expect(reads).toBe(1);
    expect(statuses).toEqual(["compacting"]);
    expect(clock.delay()).toBe(1000);
    stop();
    expect(clock.pending.size).toBe(0);
  });

  test("never overlaps slow reads", async () => {
    const clock = manualScheduler();
    const pending = deferred<string>();
    const controller = new AbortController();
    let reads = 0;
    const statuses: string[] = [];
    const stop = startChatCompactionPolling({
      read: () => {
        reads += 1;
        return pending.promise;
      },
      onStatus: (status) => statuses.push(status),
      isCurrent: () => true,
      signal: controller.signal,
      isUnsupported: isCompactionMetadataUnsupported,
      scheduler: clock.scheduler
    });
    clock.next();
    await settle();
    expect(reads).toBe(1);
    expect(clock.pending.size).toBe(0);
    pending.resolve("idle");
    await settle();
    expect(statuses).toEqual(["idle"]);
    expect(clock.pending.size).toBe(1);
    stop();
  });

  test("aborting during a read rejects its late result and cancels future polling", async () => {
    const clock = manualScheduler();
    const pending = deferred<string>();
    const controller = new AbortController();
    const statuses: string[] = [];
    let readSignal: AbortSignal | undefined;
    startChatCompactionPolling({
      read: (signal) => {
        readSignal = signal;
        return pending.promise;
      },
      onStatus: (status) => statuses.push(status),
      isCurrent: () => true,
      signal: controller.signal,
      isUnsupported: isCompactionMetadataUnsupported,
      scheduler: clock.scheduler
    });
    clock.next();
    controller.abort();
    expect(readSignal?.aborted).toBe(true);
    pending.resolve("compacting");
    await settle();
    expect(statuses).toEqual([]);
    expect(clock.pending.size).toBe(0);
  });

  test("a stale run cannot start a read or publish a late result", async () => {
    for (const staleBeforeRead of [true, false]) {
      const clock = manualScheduler();
      const pending = deferred<string>();
      const controller = new AbortController();
      const statuses: string[] = [];
      let current = !staleBeforeRead;
      let reads = 0;
      startChatCompactionPolling({
        read: () => {
          reads += 1;
          return pending.promise;
        },
        onStatus: (status) => statuses.push(status),
        isCurrent: () => current,
        signal: controller.signal,
        isUnsupported: isCompactionMetadataUnsupported,
        scheduler: clock.scheduler
      });
      clock.next();
      current = false;
      pending.resolve("compacting");
      await settle();
      expect(reads).toBe(staleBeforeRead ? 0 : 1);
      expect(statuses).toEqual([]);
      expect(clock.pending.size).toBe(0);
    }
  });

  test("old servers stop optional polling on 404 without emitting a false status", async () => {
    const clock = manualScheduler();
    const controller = new AbortController();
    const statuses: string[] = [];
    let unavailable = 0;
    startChatCompactionPolling({
      read: async () => {
        throw { status: 404, body: "private" };
      },
      onStatus: (status: string) => statuses.push(status),
      onUnavailable: () => {
        unavailable += 1;
      },
      isCurrent: () => true,
      signal: controller.signal,
      isUnsupported: isCompactionMetadataUnsupported,
      scheduler: clock.scheduler
    });
    clock.next();
    await settle();
    expect(statuses).toEqual([]);
    expect(unavailable).toBe(1);
    expect(clock.pending.size).toBe(0);
    expect(controller.signal.aborted).toBe(false);
  });

  test("read failures back off with a cap, emit no status, and recover to normal pacing", async () => {
    const clock = manualScheduler();
    const controller = new AbortController();
    const statuses: string[] = [];
    let unavailable = 0;
    let failures = 5;
    const stop = startChatCompactionPolling({
      read: async () => {
        if (failures-- > 0) throw new Error("private read failure");
        return "idle";
      },
      onStatus: (status) => statuses.push(status),
      onUnavailable: () => {
        unavailable += 1;
      },
      isCurrent: () => true,
      signal: controller.signal,
      isUnsupported: isCompactionMetadataUnsupported,
      scheduler: clock.scheduler
    });
    for (const delay of [2000, 4000, 8000, 10000, 10000]) {
      clock.next();
      await settle();
      expect(clock.delay()).toBe(delay);
      expect(statuses).toEqual([]);
    }
    expect(unavailable).toBe(5);
    clock.next();
    await settle();
    expect(statuses).toEqual(["idle"]);
    expect(clock.delay()).toBe(1000);
    stop();
  });

  test("a read failure clears observed progress without claiming completion", async () => {
    const clock = manualScheduler();
    const controller = new AbortController();
    const events: string[] = [];
    let reads = 0;
    const stop = startChatCompactionPolling({
      read: async () => {
        if (reads++ === 0) return "compacting";
        throw new Error("private read failure");
      },
      onStatus: (status) => events.push(status),
      onUnavailable: () => events.push("unavailable"),
      isCurrent: () => true,
      signal: controller.signal,
      isUnsupported: isCompactionMetadataUnsupported,
      scheduler: clock.scheduler
    });
    clock.next();
    await settle();
    clock.next();
    await settle();
    expect(events).toEqual(["compacting", "unavailable"]);
    expect(clock.delay()).toBe(2000);
    stop();
  });

  test("late read failures after abort or run replacement cannot update the UI", async () => {
    for (const cancellation of ["abort", "stale"] as const) {
      const clock = manualScheduler();
      const pending = deferred<string>();
      const controller = new AbortController();
      const events: string[] = [];
      let current = true;
      startChatCompactionPolling({
        read: () => pending.promise,
        onStatus: (status) => events.push(status),
        onUnavailable: () => events.push("unavailable"),
        isCurrent: () => current,
        signal: controller.signal,
        isUnsupported: isCompactionMetadataUnsupported,
        scheduler: clock.scheduler
      });
      clock.next();
      if (cancellation === "abort") controller.abort();
      else current = false;
      pending.reject(new Error("private late failure"));
      await settle();
      expect(events).toEqual([]);
      expect(clock.pending.size).toBe(0);
    }
  });

  test("an already-aborted run schedules no reads and stopping twice is harmless", () => {
    const clock = manualScheduler();
    const controller = new AbortController();
    controller.abort();
    let reads = 0;
    const stop = startChatCompactionPolling({
      read: async () => {
        reads += 1;
      },
      onStatus: () => {
        throw new Error("Unexpected status");
      },
      isCurrent: () => true,
      signal: controller.signal,
      isUnsupported: isCompactionMetadataUnsupported,
      scheduler: clock.scheduler
    });
    stop();
    stop();
    expect(reads).toBe(0);
    expect(clock.pending.size).toBe(0);
  });
});
