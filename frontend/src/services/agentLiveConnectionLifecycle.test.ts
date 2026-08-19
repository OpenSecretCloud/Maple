import { describe, expect, test } from "bun:test";

import {
  AgentLiveConnectionError,
  AgentLiveConnectionRegistry,
  recoverAgentLiveConnectionAfterReplacementFailure
} from "./agentLiveConnectionLifecycle";
import type { AgentActiveLiveStream, AgentPendingHistoryAttach } from "./agentRuntimeService";

function pending(cancel: () => Promise<void>): AgentPendingHistoryAttach {
  return {
    response: {} as AgentPendingHistoryAttach["response"],
    activate: async () => {
      throw new Error("not used by retirement tests");
    },
    cancel
  };
}

function active(cancel: () => Promise<void>): AgentActiveLiveStream {
  return {
    throughEventCursor: { journalId: "00112233445566778899aabbccddeeff", sequence: 7 },
    liveStreamId: "live-1",
    cancel
  };
}

describe("AgentLiveConnectionRegistry", () => {
  test("publishes cancellation intent before retirement yields", async () => {
    let cancelStarted = false;
    let finishCancel: (() => void) | undefined;
    const registry = new AgentLiveConnectionRegistry();
    registry.trackActive(
      active(async () => {
        cancelStarted = true;
        await new Promise<void>((resolve) => {
          finishCancel = resolve;
        });
      })
    );

    const retirement = registry.retire();
    expect(cancelStarted).toBe(true);
    finishCancel?.();
    await retirement;
  });

  test("clears every handle after both cancellations succeed", async () => {
    const calls: string[] = [];
    const registry = new AgentLiveConnectionRegistry();
    registry.trackPending(
      pending(async () => {
        calls.push("pending");
      })
    );
    registry.trackActive(
      active(async () => {
        calls.push("active");
      })
    );

    await registry.retire();

    expect(calls.sort()).toEqual(["active", "pending"]);
    expect(registry.pendingCount).toBe(0);
    expect(registry.activeCount).toBe(0);
  });

  test("propagates a failed cancellation and retains only its handle for retry", async () => {
    const cancellationError = new Error("host refused live cancellation");
    let activeAttempts = 0;
    const registry = new AgentLiveConnectionRegistry();
    registry.trackPending(pending(async () => {}));
    registry.trackActive(
      active(async () => {
        activeAttempts += 1;
        if (activeAttempts === 1) throw cancellationError;
      })
    );

    let caught: unknown;
    try {
      await registry.retire();
    } catch (error) {
      caught = error;
    }

    expect(caught).toBe(cancellationError);
    expect(registry.pendingCount).toBe(0);
    expect(registry.activeCount).toBe(1);

    await registry.retire();
    expect(activeAttempts).toBe(2);
    expect(registry.activeCount).toBe(0);
  });

  test("attempts both cancellations even when one throws synchronously", async () => {
    const pendingError = new Error("pending cancellation failed");
    const activeError = new Error("active cancellation failed");
    let activeAttempted = false;
    const registry = new AgentLiveConnectionRegistry();
    registry.trackPending(
      pending(() => {
        throw pendingError;
      })
    );
    registry.trackActive(
      active(async () => {
        activeAttempted = true;
        throw activeError;
      })
    );

    let caught: unknown;
    try {
      await registry.retire();
    } catch (error) {
      caught = error;
    }

    expect(activeAttempted).toBe(true);
    expect(caught).toBeInstanceOf(AgentLiveConnectionError);
    expect((caught as AgentLiveConnectionError).errors).toEqual([pendingError, activeError]);
    expect(registry.pendingCount).toBe(1);
    expect(registry.activeCount).toBe(1);
  });

  test("retains a late stale stream until its direct cancellation succeeds", async () => {
    const cancellationError = new Error("late stream cancellation failed");
    let attempts = 0;
    const registry = new AgentLiveConnectionRegistry();
    const stream = active(async () => {
      attempts += 1;
      if (attempts === 1) throw cancellationError;
    });

    await expect(registry.cancelActive(stream)).rejects.toBe(cancellationError);
    expect(registry.activeCount).toBe(1);

    await registry.retire();
    expect(attempts).toBe(2);
    expect(registry.activeCount).toBe(0);
  });
});

describe("recoverAgentLiveConnectionAfterReplacementFailure", () => {
  test("retires the failed replacement and resumes from the retained cursor", async () => {
    const replacementError = new Error("replacement attach failed");
    const cursor = { journalId: "00112233445566778899aabbccddeeff", sequence: 11 };
    const calls: string[] = [];
    let resumedCursor: unknown = null;

    let caught: unknown;
    try {
      await recoverAgentLiveConnectionAfterReplacementFailure({
        replacementError,
        cursor,
        retire: async () => {
          calls.push("retire");
        },
        resume: async (retainedCursor) => {
          calls.push("resume");
          resumedCursor = retainedCursor;
        }
      });
    } catch (error) {
      caught = error;
    }

    expect(calls).toEqual(["retire", "resume"]);
    expect(resumedCursor).toEqual(cursor);
    expect(caught).toBe(replacementError);
  });

  test("propagates retirement failure without opening a replacement stream", async () => {
    const replacementError = new Error("replacement attach failed");
    const retirementError = new Error("cancel failed");
    let resumeCount = 0;

    let caught: unknown;
    try {
      await recoverAgentLiveConnectionAfterReplacementFailure({
        replacementError,
        cursor: "event-cursor",
        retire: async () => {
          throw retirementError;
        },
        resume: async () => {
          resumeCount += 1;
        }
      });
    } catch (error) {
      caught = error;
    }

    expect(resumeCount).toBe(0);
    expect(caught).toBeInstanceOf(AgentLiveConnectionError);
    expect((caught as AgentLiveConnectionError).errors).toEqual([
      replacementError,
      retirementError
    ]);
  });

  test("reports both the replacement and cursor-resume failures", async () => {
    const replacementError = new Error("replacement attach failed");
    const resumeError = new Error("cursor resume failed");

    let caught: unknown;
    try {
      await recoverAgentLiveConnectionAfterReplacementFailure({
        replacementError,
        cursor: "event-cursor",
        retire: async () => {},
        resume: async () => {
          throw resumeError;
        }
      });
    } catch (error) {
      caught = error;
    }

    expect(caught).toBeInstanceOf(AgentLiveConnectionError);
    expect((caught as AgentLiveConnectionError).errors).toEqual([replacementError, resumeError]);
  });
});
