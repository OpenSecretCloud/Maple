import { describe, expect, test } from "bun:test";

import {
  CHAT_HISTORY_WHEEL_GESTURE_QUIET_MS,
  ChatHistoryPaginationGate,
  chatHistoryCursorProgressed,
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

  test("uses wheel event timestamps when a busy render delays the quiet timer", () => {
    const gate = new ChatHistoryPaginationGate();

    gate.beginWheelGesture(1_000);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);
    gate.finishLoad();

    // Residual input inside the quiet period still belongs to the consumed gesture.
    gate.beginWheelGesture(1_100);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(false);

    // The timeout callback has not run, but the next event proves that the
    // configured quiet period elapsed after the final residual input.
    gate.beginWheelGesture(1_100 + CHAT_HISTORY_WHEEL_GESTURE_QUIET_MS);
    expect(gate.tryStartLoad(visibleBoundary)).toBe(true);
  });

  test("loads later pages from fresh wheel bursts without waiting for timer callbacks", () => {
    const gate = new ChatHistoryPaginationGate();
    let loadCount = 0;
    let eventTimestamp = 0;

    gate.beginWheelGesture(eventTimestamp);
    if (gate.tryStartLoad(visibleBoundary)) loadCount += 1;
    gate.finishLoad();

    for (let page = 2; page <= 4; page += 1) {
      // Closely spaced momentum cannot rearm the consumed page.
      eventTimestamp += 60;
      gate.beginWheelGesture(eventTimestamp);
      expect(gate.tryStartLoad(visibleBoundary)).toBe(false);

      // A new burst after the same quiet period used by the fallback timer can.
      eventTimestamp += CHAT_HISTORY_WHEEL_GESTURE_QUIET_MS;
      gate.beginWheelGesture(eventTimestamp);
      if (gate.tryStartLoad(visibleBoundary)) loadCount += 1;
      expect(gate.tryStartLoad(visibleBoundary)).toBe(false);
      gate.finishLoad();
    }

    expect(loadCount).toBe(4);
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
