import { describe, expect, test } from "bun:test";
import {
  applyAgentDesktopQueueSnapshot,
  emptyAgentDesktopQueueSnapshot,
  queueSnapshotWithoutItem,
  restoreQueuedMessageToComposer,
  shouldPrepareThoughtAfterAgentSend,
  type AgentQueuedMessage
} from "./agentComposerQueue";

function queued(queueId: string, text: string): AgentQueuedMessage {
  return {
    queueId,
    messageId: `msg-${queueId}`,
    sessionId: "session-1",
    text,
    createdMs: 1
  };
}

describe("agent composer queue projection", () => {
  test("ignores stale snapshots and accepts equal or newer revisions", () => {
    const first = { revision: 2, items: [queued("q1", "later")] };
    expect(
      applyAgentDesktopQueueSnapshot(first, { revision: 1, items: [queued("q0", "stale")] })
    ).toBe(first);
    expect(
      applyAgentDesktopQueueSnapshot(first, { revision: 2, items: [queued("q1", "same")] }).items[0]
        ?.text
    ).toBe("same");
    expect(
      applyAgentDesktopQueueSnapshot(undefined, emptyAgentDesktopQueueSnapshot()).items
    ).toEqual([]);
  });

  test("optimistically drops a chip without advancing the native revision", () => {
    const snapshot = {
      revision: 4,
      items: [queued("q1", "keep"), queued("q2", "drop")]
    };
    expect(queueSnapshotWithoutItem(snapshot, "q2")).toEqual({
      revision: 4,
      items: [queued("q1", "keep")]
    });
    expect(queueSnapshotWithoutItem(undefined, "missing")).toEqual(
      emptyAgentDesktopQueueSnapshot()
    );
  });

  test("restores an unqueued message without discarding a composer draft", () => {
    expect(restoreQueuedMessageToComposer("", "queued")).toBe("queued");
    expect(restoreQueuedMessageToComposer("   ", "queued")).toBe("queued");
    expect(restoreQueuedMessageToComposer("draft", "queued")).toBe("queued\ndraft");
  });

  test("does not seed thought tracking for a staged follow-up", () => {
    expect(shouldPrepareThoughtAfterAgentSend(undefined)).toBe(true);
    expect(shouldPrepareThoughtAfterAgentSend(null)).toBe(true);
    expect(shouldPrepareThoughtAfterAgentSend(queued("q1", "later"))).toBe(false);
  });
});
