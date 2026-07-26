import { describe, expect, it } from "bun:test";
import { ResponseLifecycleFence } from "./responseLifecycle";

describe("ResponseLifecycleFence", () => {
  it("allows ordinary request failures to use existing recovery", () => {
    const fence = new ResponseLifecycleFence();
    expect(fence.beginResponse()).toBe(true);
    expect(fence.shouldIgnoreErrors()).toBe(false);
    expect(fence.canUpdateState()).toBe(true);
  });

  it("rejects a duplicate submission until the active response finishes", () => {
    const fence = new ResponseLifecycleFence();
    expect(fence.beginResponse()).toBe(true);
    expect(fence.beginResponse()).toBe(false);
    fence.finishResponse();
    expect(fence.beginResponse()).toBe(true);
  });

  it("suppresses retry and recovery after a deliberate client abort", () => {
    const fence = new ResponseLifecycleFence();
    fence.beginResponse();
    fence.abortResponse();
    expect(fence.shouldIgnoreErrors()).toBe(true);
  });

  it("allows a later request after an in-place user cancellation", () => {
    const fence = new ResponseLifecycleFence();
    expect(fence.beginResponse()).toBe(true);
    fence.abortResponse();
    expect(fence.beginResponse()).toBe(false);
    fence.finishResponse();
    expect(fence.beginResponse()).toBe(true);
    expect(fence.shouldIgnoreErrors()).toBe(false);
  });

  it("permanently suppresses retries and state updates after unmount", () => {
    const fence = new ResponseLifecycleFence();
    fence.unmount();
    expect(fence.beginResponse()).toBe(false);
    expect(fence.shouldIgnoreErrors()).toBe(true);
    expect(fence.canUpdateState()).toBe(false);
  });
});
