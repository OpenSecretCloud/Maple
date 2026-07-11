import { describe, expect, it } from "bun:test";
import { ResponseLifecycleFence } from "./responseLifecycle";

describe("ResponseLifecycleFence", () => {
  it("allows ordinary request failures to use existing recovery", () => {
    const fence = new ResponseLifecycleFence();
    fence.beginResponse();
    expect(fence.shouldIgnoreErrors()).toBe(false);
    expect(fence.canUpdateState()).toBe(true);
  });

  it("suppresses retry and recovery after a deliberate client abort", () => {
    const fence = new ResponseLifecycleFence();
    fence.beginResponse();
    fence.abortResponse();
    expect(fence.shouldIgnoreErrors()).toBe(true);
  });

  it("allows a later request after an in-place user cancellation", () => {
    const fence = new ResponseLifecycleFence();
    fence.abortResponse();
    fence.beginResponse();
    expect(fence.shouldIgnoreErrors()).toBe(false);
  });

  it("permanently suppresses retries and state updates after unmount", () => {
    const fence = new ResponseLifecycleFence();
    fence.unmount();
    fence.beginResponse();
    expect(fence.shouldIgnoreErrors()).toBe(true);
    expect(fence.canUpdateState()).toBe(false);
  });
});
