import { describe, expect, test } from "bun:test";

import {
  ChatHistoryPaginationGate,
  chatHistoryCursorProgressed,
  chatHistoryScrolledBackward,
  requiredChatHistoryBottomCompensation,
  restoredChatHistoryAnchorScrollTop,
  restoredChatHistoryScrollTop
} from "./chatHistoryPagination";

const visibleBoundary = {
  canLoad: true,
  topBoundaryVisible: true
};

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

  test("does not queue a gesture that begins while a load is in flight", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginGesture();
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);

    gate.endGesture();
    gate.beginGesture();
    gate.finishLoad();

    expect(gate.tryStartLoad(visibleBoundary)).toBe(false);

    gate.endGesture();
    gate.beginGesture();
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);
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

describe("chatHistoryScrolledBackward", () => {
  test("rechecks the boundary after native upward scrolling applies", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginGesture();
    expect(
      gate.tryStartLoad({
        canLoad: true,
        topBoundaryVisible: false
      })
    ).toBe(false);

    expect(chatHistoryScrolledBackward(240, 80)).toBe(true);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);
  });

  test("does not recheck on stationary or forward scroll changes", () => {
    expect(chatHistoryScrolledBackward(80, 80)).toBe(false);
    expect(chatHistoryScrolledBackward(80, 240)).toBe(false);
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
