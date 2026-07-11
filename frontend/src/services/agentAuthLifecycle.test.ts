import { describe, expect, test } from "bun:test";
import { AgentAuthLifecycleCoordinator } from "./agentAuthLifecycle";

describe("AgentAuthLifecycleCoordinator", () => {
  test("cleans the previous account before activating the next one", async () => {
    const events: string[] = [];
    const coordinator = new AgentAuthLifecycleCoordinator(
      async (userId) => {
        events.push(`cleanup:${userId}`);
      },
      (userId) => events.push(`activate:${userId}`)
    );

    await coordinator.transitionTo("user-a");
    events.length = 0;
    await coordinator.transitionTo("user-b");
    await coordinator.waitForUser("user-b");

    expect(events).toEqual(["cleanup:user-a", "activate:user-b"]);
  });

  test("drains every skipped account during rapid transitions", async () => {
    const events: string[] = [];
    let releaseCleanup: (() => void) | undefined;
    const cleanupGate = new Promise<void>((resolve) => {
      releaseCleanup = resolve;
    });
    const coordinator = new AgentAuthLifecycleCoordinator(
      async (userId) => {
        events.push(`cleanup:${userId}`);
        if (userId === "user-a") await cleanupGate;
      },
      (userId) => events.push(`activate:${userId}`)
    );

    await coordinator.transitionTo("user-a");
    events.length = 0;
    const toB = coordinator.transitionTo("user-b");
    const toC = coordinator.transitionTo("user-c");
    releaseCleanup?.();
    await Promise.all([toB, toC]);

    expect(events.slice(0, 2)).toEqual(["cleanup:user-a", "cleanup:user-b"]);
    expect(events.at(-1)).toBe("activate:user-c");
    expect(events).not.toContain("activate:user-b");
  });

  test("retains a failed cleanup target for the next transition retry", async () => {
    const attempts: string[] = [];
    let shouldFail = true;
    const coordinator = new AgentAuthLifecycleCoordinator(
      async (userId) => {
        attempts.push(userId);
        if (shouldFail) throw new Error("offline");
      },
      () => {}
    );

    await coordinator.transitionTo("user-a");
    await expect(coordinator.transitionTo("user-b")).rejects.toThrow("offline");
    shouldFail = false;
    await coordinator.transitionTo("user-c");

    expect(attempts).toEqual(["user-a", "user-a", "user-b"]);
    await coordinator.waitForUser("user-c");
  });
});
