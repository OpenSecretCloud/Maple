import { describe, expect, it } from "bun:test";

import { AgentOperationFence, AgentOperationsBlockedError } from "./agentOperationFence";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((nextResolve) => {
    resolve = nextResolve;
  });
  return { promise, resolve };
}

describe("AgentOperationFence", () => {
  it("blocks new work and waits for in-flight work before granting cleanup", async () => {
    const fence = new AgentOperationFence();
    const pending = deferred<void>();
    const operation = fence.run("user-a", async () => await pending.promise);

    await Promise.resolve();
    let blockGranted = false;
    const blockPromise = fence.blockAndDrain("user-a").then((block) => {
      blockGranted = true;
      return block;
    });

    await expect(fence.run("user-a", async () => "late work")).rejects.toBeInstanceOf(
      AgentOperationsBlockedError
    );
    expect(blockGranted).toBe(false);

    pending.resolve();
    await operation;
    const block = await blockPromise;
    expect(blockGranted).toBe(true);

    block.release();
    await expect(fence.run("user-a", async () => "resumed")).resolves.toBe("resumed");
  });

  it("invalidates queued work when cleanup wins the generation race", async () => {
    const fence = new AgentOperationFence();
    let ran = false;
    const operation = fence.run("user-a", async () => {
      ran = true;
    });
    const block = await fence.blockAndDrain("user-a");

    await expect(operation).rejects.toBeInstanceOf(AgentOperationsBlockedError);
    expect(ran).toBe(false);
    block.release();
  });

  it("drains an external await and rejects its late nested operation", async () => {
    const fence = new AgentOperationFence();
    const external = deferred<void>();
    let nestedRan = false;
    const workflow = fence.run("user-a", async () => {
      await external.promise;
      await fence.run("user-a", async () => {
        nestedRan = true;
      });
    });
    await Promise.resolve();

    const blockPromise = fence.blockAndDrain("user-a");
    external.resolve();
    const block = await blockPromise;

    await expect(workflow).rejects.toBeInstanceOf(AgentOperationsBlockedError);
    expect(nestedRan).toBe(false);
    block.release();
  });

  it("isolates accounts and keeps blocking until every lease releases", async () => {
    const fence = new AgentOperationFence();
    const first = await fence.blockAndDrain("user-a");
    const second = await fence.blockAndDrain("user-a");

    await expect(fence.run("user-b", async () => "ok")).resolves.toBe("ok");
    first.release();
    await expect(fence.run("user-a", async () => "blocked")).rejects.toBeInstanceOf(
      AgentOperationsBlockedError
    );

    second.release();
    await expect(fence.run("user-a", async () => "ok")).resolves.toBe("ok");
  });

  it("keeps a completed logout blocked until a new authenticated session activates", async () => {
    const fence = new AgentOperationFence();
    const block = await fence.blockAndDrain("user-a");
    block.retainUntilNextSession();

    await expect(fence.run("user-a", async () => "stale")).rejects.toBeInstanceOf(
      AgentOperationsBlockedError
    );
    fence.activateUserSession("user-a");
    await expect(fence.run("user-a", async () => "new session")).resolves.toBe("new session");
  });

  it("does not release an active cleanup lease when a component activates", async () => {
    const fence = new AgentOperationFence();
    const block = await fence.blockAndDrain("user-a");

    fence.activateUserSession("user-a");
    await expect(fence.run("user-a", async () => "blocked")).rejects.toBeInstanceOf(
      AgentOperationsBlockedError
    );
    block.release();
  });
});
