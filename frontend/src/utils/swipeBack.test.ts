import { describe, expect, test } from "bun:test";
import {
  clampSwipeBackDistance,
  getSwipeBackDirection,
  getSwipeBackSettleDuration,
  shouldCompleteSwipeBack
} from "./swipeBack";

describe("iOS swipe-back gesture", () => {
  test("waits for enough movement before locking direction", () => {
    expect(getSwipeBackDirection(4, 2)).toBe("pending");
    expect(getSwipeBackDirection(10, 3)).toBe("track");
  });

  test("rejects leftward and primarily vertical movement", () => {
    expect(getSwipeBackDirection(-10, 1)).toBe("reject");
    expect(getSwipeBackDirection(8, 12)).toBe("reject");
  });

  test("clamps interactive movement to the viewport", () => {
    expect(clampSwipeBackDistance(-20, 390)).toBe(0);
    expect(clampSwipeBackDistance(180, 390)).toBe(180);
    expect(clampSwipeBackDistance(500, 390)).toBe(390);
  });

  test("completes by distance or rightward velocity", () => {
    expect(shouldCompleteSwipeBack({ distance: 140, width: 390, velocity: 0.1 })).toBe(true);
    expect(shouldCompleteSwipeBack({ distance: 60, width: 390, velocity: 0.7 })).toBe(true);
    expect(shouldCompleteSwipeBack({ distance: 60, width: 390, velocity: 0.2 })).toBe(false);
  });

  test("settles faster when less distance remains and skips motion when requested", () => {
    expect(
      getSwipeBackSettleDuration({ progress: 0.8, completing: true, reducedMotion: false })
    ).toBe(120);
    expect(
      getSwipeBackSettleDuration({ progress: 0.5, completing: false, reducedMotion: false })
    ).toBe(160);
    expect(
      getSwipeBackSettleDuration({ progress: 0.5, completing: true, reducedMotion: true })
    ).toBe(0);
  });
});
