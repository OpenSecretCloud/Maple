import { describe, expect, test } from "bun:test";
import { AgentAuthLifecycleCoordinator } from "./agentAuthLifecycle";

describe("AgentAuthLifecycleCoordinator", () => {
  test("cleans the previous account before activating the next one", async () => {
    const events: string[] = [];
    const coordinator = new AgentAuthLifecycleCoordinator(
      async (userId) => {
        events.push(`cleanup:${userId}`);
      },
      async (userId) => {
        events.push(`activate:${userId}`);
      }
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
      async (userId) => {
        events.push(`activate:${userId}`);
      }
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
      async () => {}
    );

    await coordinator.transitionTo("user-a");
    await expect(coordinator.transitionTo("user-b")).rejects.toThrow("offline");
    shouldFail = false;
    await coordinator.transitionTo("user-c");

    expect(attempts).toEqual(["user-a", "user-a", "user-b"]);
    await coordinator.waitForUser("user-c");
  });

  test("waits for credential installation before activating an account", async () => {
    const events: string[] = [];
    let finishInstall: (() => void) | undefined;
    let markInstallStarted: (() => void) | undefined;
    const installGate = new Promise<void>((resolve) => {
      finishInstall = resolve;
    });
    const installStarted = new Promise<void>((resolve) => {
      markInstallStarted = resolve;
    });
    const coordinator = new AgentAuthLifecycleCoordinator(
      async () => {},
      async (userId) => {
        events.push(`install:${userId}`);
        markInstallStarted?.();
        await installGate;
        events.push(`activate:${userId}`);
      }
    );

    const transition = coordinator.transitionTo("user-a");
    await installStarted;
    expect(events).toEqual(["install:user-a"]);
    finishInstall?.();
    await transition;
    expect(events).toEqual(["install:user-a", "activate:user-a"]);
  });

  test("retries a transient same-user activation failure", async () => {
    let attempts = 0;
    const coordinator = new AgentAuthLifecycleCoordinator(
      async () => {},
      async () => {
        attempts += 1;
        if (attempts === 1) throw new Error("temporary auth failure");
      }
    );

    await expect(coordinator.transitionTo("user-a")).rejects.toThrow("temporary auth failure");
    await coordinator.transitionTo("user-a");
    await coordinator.waitForUser("user-a");

    expect(attempts).toBe(2);
  });

  test("Agent initialization retries a failed initial activation through the serialized lane", async () => {
    let attempts = 0;
    const coordinator = new AgentAuthLifecycleCoordinator(
      async () => {},
      async () => {
        attempts += 1;
        if (attempts === 1) throw new Error("temporary auth failure");
      }
    );

    const initialTransition = coordinator.transitionTo("user-a");
    const firstInitialization = coordinator.ensureCurrentUser("user-a");
    const secondInitialization = coordinator.ensureCurrentUser("user-a");

    await expect(initialTransition).rejects.toThrow("temporary auth failure");
    await Promise.all([firstInitialization, secondInitialization]);
    await coordinator.waitForUser("user-a");

    expect(attempts).toBe(2);
  });

  test("an initialization retry cannot restore an account replaced while activation was pending", async () => {
    let failUserA: (() => void) | undefined;
    let markUserAStarted: (() => void) | undefined;
    const firstUserA = new Promise<void>((_, reject) => {
      failUserA = () => reject(new Error("temporary auth failure"));
    });
    const userAStarted = new Promise<void>((resolve) => {
      markUserAStarted = resolve;
    });
    const activations: string[] = [];
    const coordinator = new AgentAuthLifecycleCoordinator(
      async () => {},
      async (userId) => {
        activations.push(userId);
        if (userId === "user-a" && activations.length === 1) {
          markUserAStarted?.();
          await firstUserA;
        }
      }
    );

    const initialA = coordinator.transitionTo("user-a");
    const observedInitialA = initialA.then(
      () => null,
      (error: unknown) => error
    );
    await userAStarted;
    const ensureA = coordinator.ensureCurrentUser("user-a");
    const observedEnsureA = ensureA.then(
      () => null,
      (error: unknown) => error
    );
    const transitionB = coordinator.transitionTo("user-b");
    failUserA?.();

    expect(await observedInitialA).toEqual(new Error("temporary auth failure"));
    expect(await observedEnsureA).toEqual(
      new Error("Agent Mode authentication changed before initialization completed")
    );
    await transitionB;
    await coordinator.waitForUser("user-b");

    expect(activations).toEqual(["user-a", "user-b"]);
  });
});
