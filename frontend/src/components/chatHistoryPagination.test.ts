import { describe, expect, test } from "bun:test";

import {
  ChatHistoryPaginationGate,
  chatHistoryCursorProgressed,
  requiredChatHistoryBottomCompensation,
  restoredChatHistoryAnchorScrollTop,
  restoredChatHistoryScrollTop,
  usesFirstCancelableWheelGestureStart
} from "./chatHistoryPagination";

const visibleBoundary = {
  canLoad: true,
  topBoundaryVisible: true
};

describe("usesFirstCancelableWheelGestureStart", () => {
  test("uses the cancelability boundary in macOS web browsers", () => {
    expect(
      usesFirstCancelableWheelGestureStart({
        isTauriEnvironment: false,
        browserPlatform: "MacIntel"
      })
    ).toBe(true);
  });

  test("keeps Tauri and non-macOS browser wheel listeners on the passive path", () => {
    expect(
      usesFirstCancelableWheelGestureStart({
        isTauriEnvironment: true,
        browserPlatform: "MacIntel"
      })
    ).toBe(false);
    expect(
      usesFirstCancelableWheelGestureStart({
        isTauriEnvironment: false,
        browserPlatform: "Win32"
      })
    ).toBe(false);
  });
});

describe("ChatHistoryPaginationGate", () => {
  test("does not load when the top boundary is visible without user intent", () => {
    const gate = new ChatHistoryPaginationGate();

    expect(gate.tryStartLoad(visibleBoundary)).toBe(false);
  });

  test("allows only one load for one gesture", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginGesture();

    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(false);
  });

  test("keeps a gesture armed until the top boundary becomes visible", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginGesture();

    expect(
      gate.tryStartLoad({
        canLoad: true,
        topBoundaryVisible: false
      })
    ).toBe(false);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(false);
  });

  test("does not load from a stale intersection after the gesture ends", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginGesture();
    expect(
      gate.tryStartLoad({
        canLoad: true,
        topBoundaryVisible: false
      })
    ).toBe(false);

    gate.endGesture();

    expect(gate.tryStartLoad(visibleBoundary)).toBe(false);
  });

  test("resetIntent clears intent armed for the previous conversation", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginGesture();
    gate.resetIntent();

    expect(gate.tryStartLoad(visibleBoundary)).toBe(false);
  });

  test("does not rearm from repeated intersection or momentum until the gesture ends", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginGesture();
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);

    gate.finishLoad();
    gate.beginGesture();

    expect(gate.tryStartLoad(visibleBoundary)).toBe(false);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(false);
  });

  test("does not rearm when the consumed gesture crosses a restored boundary again", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginGesture();
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);
    gate.finishLoad();

    gate.beginGesture();
    expect(
      gate.tryStartLoad({
        canLoad: true,
        topBoundaryVisible: false
      })
    ).toBe(false);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(false);

    gate.endGesture();
    gate.beginGesture();
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);
  });

  test("loads through page four from one completed gesture per page", () => {
    const gate = new ChatHistoryPaginationGate();
    let loadCount = 0;

    gate.beginGesture();
    if (gate.tryStartLoad(visibleBoundary)) loadCount += 1;
    gate.finishLoad();

    for (let page = 2; page <= 4; page += 1) {
      // Idle observer callbacks cannot start another page.
      expect(gate.tryStartLoad(visibleBoundary)).toBe(false);

      // Momentum in the completed gesture cannot rearm the next page.
      gate.beginGesture();
      expect(gate.tryStartLoad(visibleBoundary)).toBe(false);

      // A genuinely completed gesture allows exactly one new traversal.
      gate.endGesture();
      gate.beginGesture();
      expect(
        gate.tryStartLoad({
          canLoad: true,
          topBoundaryVisible: false
        })
      ).toBe(false);
      if (gate.tryStartLoad(visibleBoundary)) loadCount += 1;
      expect(gate.tryStartLoad(visibleBoundary)).toBe(false);
      gate.finishLoad();
    }

    expect(loadCount).toBe(4);
  });

  test("allows the next page after a second deliberate gesture", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginGesture();
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);
    gate.finishLoad();
    gate.endGesture();

    gate.beginGesture();

    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);
  });

  test("rearms immediately for the first event of a new wheel gesture", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginWheelGesture(true);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);
    gate.finishLoad();

    // Residual direct or momentum events do not create another gesture.
    gate.beginWheelGesture(false);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(false);

    // A new first wheel event can replace the still-active consumed gesture.
    gate.beginWheelGesture(true);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);
  });

  test("queues a new wheel gesture that reaches the top during an in-flight load", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginWheelGesture(true);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);

    gate.beginWheelGesture(true);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(false);

    // Reaching the boundary is the completed intent. Ending the physical
    // gesture before the network response must not discard that one request.
    gate.endGesture();
    gate.finishLoad({ preserveQueuedLoad: true });

    expect(gate.tryStartQueuedLoad({ canLoad: true })).toBe(true);
    expect(gate.tryStartQueuedLoad({ canLoad: true })).toBe(false);

    gate.finishLoad({ preserveQueuedLoad: true });
    expect(gate.tryStartQueuedLoad({ canLoad: true })).toBe(false);
  });

  test("queues a browser wheel gesture that starts after scrollend during an in-flight load", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginGesture();
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);

    // Chrome's scrollend completes the first physical gesture before the next
    // wheel event arrives, even if the first page request is still pending.
    gate.endGesture();
    gate.beginGesture();
    expect(gate.tryStartLoad(visibleBoundary)).toBe(false);

    gate.endGesture();
    gate.finishLoad({ preserveQueuedLoad: true });

    expect(gate.tryStartQueuedLoad({ canLoad: true })).toBe(true);
    expect(gate.tryStartQueuedLoad({ canLoad: true })).toBe(false);
  });

  test("clears unconsumed intent when an in-flight page progresses", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginWheelGesture(true);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);

    // A later gesture has started, but has not actually reached the boundary,
    // so it must not become an observer-driven request after the prepend.
    gate.beginWheelGesture(true);
    expect(
      gate.tryStartLoad({
        canLoad: true,
        topBoundaryVisible: false
      })
    ).toBe(false);

    gate.finishLoad({ preserveQueuedLoad: true });

    expect(gate.tryStartLoad(visibleBoundary)).toBe(false);
    expect(gate.tryStartQueuedLoad({ canLoad: true })).toBe(false);
  });

  test("does not queue residual wheel momentum during an in-flight load", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginWheelGesture(true);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);

    gate.beginWheelGesture(false);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(false);

    gate.finishLoad({ preserveQueuedLoad: true });

    expect(gate.tryStartQueuedLoad({ canLoad: true })).toBe(false);
  });

  test("clears a queued gesture when the active page fails to progress", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginWheelGesture(true);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);

    gate.beginWheelGesture(true);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(false);
    gate.finishLoad();

    expect(gate.tryStartQueuedLoad({ canLoad: true })).toBe(false);
  });

  test("clears newer armed intent when the active page fails to progress", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginWheelGesture(true);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);

    gate.beginWheelGesture(true);
    expect(
      gate.tryStartLoad({
        canLoad: true,
        topBoundaryVisible: false
      })
    ).toBe(false);
    gate.finishLoad();

    expect(gate.tryStartLoad(visibleBoundary)).toBe(false);
  });

  test("keeps a queued load when its drain races the active request", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginWheelGesture(true);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);
    gate.beginWheelGesture(true);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(false);

    expect(gate.tryStartQueuedLoad({ canLoad: true })).toBe(false);
    gate.finishLoad({ preserveQueuedLoad: true });

    expect(gate.tryStartQueuedLoad({ canLoad: true })).toBe(true);
  });

  test("coalesces multiple overlapping boundary attempts into one queued load", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginWheelGesture(true);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);

    for (let gesture = 0; gesture < 3; gesture += 1) {
      gate.beginWheelGesture(true);
      expect(gate.tryStartLoad(visibleBoundary)).toBe(false);
    }

    gate.finishLoad({ preserveQueuedLoad: true });
    expect(gate.tryStartQueuedLoad({ canLoad: true })).toBe(true);
    expect(gate.tryStartQueuedLoad({ canLoad: true })).toBe(false);
  });

  test("discards a queued load when there is no older page", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginWheelGesture(true);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);
    gate.beginWheelGesture(true);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(false);
    gate.finishLoad({ preserveQueuedLoad: true });

    expect(gate.tryStartQueuedLoad({ canLoad: false })).toBe(false);
    expect(gate.tryStartQueuedLoad({ canLoad: true })).toBe(false);
  });

  test("does not duplicate a request owned by a previous runtime projection", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginGesture();
    expect(
      gate.tryStartLoad({
        ...visibleBoundary,
        requestInFlight: true
      })
    ).toBe(false);

    // The discarded projection intent cannot turn into an automatic retry
    // when the request owned by the previous projection settles.
    expect(gate.tryStartLoad(visibleBoundary)).toBe(false);
  });

  test("keeps pagination lifecycles independent across runtime projections", () => {
    const previousRuntimeGate = new ChatHistoryPaginationGate();
    const nextRuntimeGate = new ChatHistoryPaginationGate();

    previousRuntimeGate.beginGesture();
    expect(previousRuntimeGate.tryStartLoad(visibleBoundary)).toBe(true);

    nextRuntimeGate.beginGesture();
    expect(nextRuntimeGate.tryStartLoad(visibleBoundary)).toBe(true);

    previousRuntimeGate.finishLoad();
    expect(nextRuntimeGate.tryStartLoad(visibleBoundary)).toBe(false);
  });

  test("loads through page four when each next wheel gesture reaches the top in flight", () => {
    const gate = new ChatHistoryPaginationGate();
    let loadCount = 0;

    gate.beginWheelGesture(true);
    if (gate.tryStartLoad(visibleBoundary)) loadCount += 1;

    for (let page = 2; page <= 4; page += 1) {
      gate.beginWheelGesture(true);

      // The new gesture is queued, but cannot overlap the active request.
      expect(gate.tryStartLoad(visibleBoundary)).toBe(false);
      gate.endGesture();

      gate.finishLoad({ preserveQueuedLoad: true });
      if (gate.tryStartQueuedLoad({ canLoad: true })) loadCount += 1;
      expect(gate.tryStartQueuedLoad({ canLoad: true })).toBe(false);
    }

    expect(loadCount).toBe(4);
  });

  test("resetIntent clears a queued load for the previous conversation", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginWheelGesture(true);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);
    gate.beginWheelGesture(true);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(false);

    gate.resetIntent();
    gate.finishLoad({ preserveQueuedLoad: true });

    expect(gate.tryStartQueuedLoad({ canLoad: true })).toBe(false);
  });

  test("requires fresh intent after a load finishes or fails", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginGesture();
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);

    // Callers settle both success and failure through finishLoad.
    gate.finishLoad();
    expect(gate.tryStartLoad(visibleBoundary)).toBe(false);

    gate.endGesture();
    gate.beginGesture();
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);
  });

  test("does not load when no older page can be loaded", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginGesture();

    expect(
      gate.tryStartLoad({
        canLoad: false,
        topBoundaryVisible: true
      })
    ).toBe(false);
  });
});

describe("chatHistoryCursorProgressed", () => {
  test("requires a defined next cursor that differs from the current cursor", () => {
    expect(chatHistoryCursorProgressed("oldest-2", undefined)).toBe(false);
    expect(chatHistoryCursorProgressed("oldest-2", "oldest-2")).toBe(false);
    expect(chatHistoryCursorProgressed("oldest-2", "oldest-1")).toBe(true);
  });
});

describe("restoredChatHistoryScrollTop", () => {
  test("preserves the visible anchor when prepending at scrollTop zero", () => {
    expect(
      restoredChatHistoryScrollTop(
        {
          scrollTop: 0,
          scrollHeight: 800
        },
        1100
      )
    ).toBe(300);
  });

  test("preserves a nonzero scroll offset across a prepend", () => {
    expect(
      restoredChatHistoryScrollTop(
        {
          scrollTop: 120,
          scrollHeight: 800
        },
        1100
      )
    ).toBe(420);
  });

  test("uses the net height delta when rendered content changes", () => {
    expect(
      restoredChatHistoryScrollTop(
        {
          scrollTop: 75,
          scrollHeight: 640
        },
        910
      )
    ).toBe(345);
  });

  test("clamps restoration to the top when the rendered height shrinks", () => {
    expect(
      restoredChatHistoryScrollTop(
        {
          scrollTop: 40,
          scrollHeight: 700
        },
        620
      )
    ).toBe(0);
  });
});

describe("restoredChatHistoryAnchorScrollTop", () => {
  test("uses the rendered anchor movement instead of unrelated height changes", () => {
    expect(restoredChatHistoryAnchorScrollTop(953, 561, 584.5)).toBe(976.5);
  });

  test("clamps a backward anchor correction to the top", () => {
    expect(restoredChatHistoryAnchorScrollTop(10, 80, 20)).toBe(0);
  });
});

describe("requiredChatHistoryBottomCompensation", () => {
  test("fills only the scroll range missing from an anchor restoration", () => {
    expect(requiredChatHistoryBottomCompensation(976.5, 1972, 1019)).toBe(23.5);
  });

  test("does not add space when the natural scroll range reaches the anchor", () => {
    expect(requiredChatHistoryBottomCompensation(420, 1500, 800)).toBe(0);
  });
});
