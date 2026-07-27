import { describe, expect, test } from "bun:test";
import { createDeferredDisposalLifecycle } from "./deferredDisposalLifecycle";

describe("createDeferredDisposalLifecycle", () => {
  test("ignores Strict Mode effect cleanup but disposes after the real unmount", () => {
    const queuedTasks: Array<() => void> = [];
    let disposeCount = 0;
    const lifecycle = createDeferredDisposalLifecycle(
      () => {
        disposeCount += 1;
      },
      (task) => queuedTasks.push(task)
    );

    const strictModeCleanup = lifecycle.activate();
    strictModeCleanup();
    const realUnmountCleanup = lifecycle.activate();

    queuedTasks.shift()?.();
    expect(disposeCount).toBe(0);

    realUnmountCleanup();
    queuedTasks.shift()?.();
    expect(disposeCount).toBe(1);
  });
});
