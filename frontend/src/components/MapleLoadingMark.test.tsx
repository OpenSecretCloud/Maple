import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import { act, create, type ReactTestRenderer } from "react-test-renderer";

import { MapleLoadingMark } from "./MapleLoadingMark";

/** Minimal stand-in for the SVGPathElement the component writes `d` to. */
class FakePath {
  readonly writes: string[] = [];
  setAttribute(name: string, value: string): void {
    if (name === "d") this.writes.push(value);
  }
  getAttribute(name: string): string | null {
    return name === "d" ? (this.writes.at(-1) ?? null) : null;
  }
}

function points(d: string): Array<[number, number]> {
  return d
    .slice(0, -1)
    .split(/(?=[ML])/)
    .filter(Boolean)
    .map((seg) => {
      const [x, y] = seg.slice(1).split(" ").map(Number);
      return [x, y] as [number, number];
    });
}

function bbox(pts: Array<[number, number]>) {
  const xs = pts.map((p) => p[0]);
  const ys = pts.map((p) => p[1]);
  return {
    minX: Math.min(...xs),
    maxX: Math.max(...xs),
    minY: Math.min(...ys),
    maxY: Math.max(...ys)
  };
}

const originalNow = globalThis.performance.now;
const originalRaf = globalThis.requestAnimationFrame;
const originalCaf = globalThis.cancelAnimationFrame;
const originalMatchMedia = globalThis.matchMedia;
const originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");

describe("MapleLoadingMark", () => {
  let node: FakePath;
  let pending: FrameRequestCallback | null;
  let reduceMotion: boolean;
  let clock: number;
  let lastHandle: number;
  let cancelled: number[];

  beforeEach(() => {
    node = new FakePath();
    pending = null;
    reduceMotion = false;
    // The component times itself off performance.now(), so the test drives that
    // clock and the rAF timestamp together rather than passing bare numbers.
    clock = 0;
    lastHandle = 0;
    cancelled = [];
    globalThis.performance.now = () => clock;
    globalThis.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      pending = cb;
      lastHandle += 1;
      return lastHandle;
    }) as typeof requestAnimationFrame;
    globalThis.cancelAnimationFrame = ((handle: number) => {
      cancelled.push(handle);
    }) as typeof cancelAnimationFrame;
    globalThis.matchMedia = ((query: string) => ({
      matches: reduceMotion && query.includes("reduce")
    })) as unknown as typeof matchMedia;
    Object.defineProperty(globalThis, "window", {
      configurable: true,
      value: { matchMedia: globalThis.matchMedia },
      writable: true
    });
  });

  afterEach(() => {
    globalThis.performance.now = originalNow;
    globalThis.requestAnimationFrame = originalRaf;
    globalThis.cancelAnimationFrame = originalCaf;
    globalThis.matchMedia = originalMatchMedia;
    if (originalWindow) Object.defineProperty(globalThis, "window", originalWindow);
    else Reflect.deleteProperty(globalThis, "window");
  });

  function render(): ReactTestRenderer {
    let renderer!: ReactTestRenderer;
    act(() => {
      renderer = create(<MapleLoadingMark />, { createNodeMock: () => node });
    });
    return renderer;
  }

  /** Move the shared clock to `ms` and run the frame the component scheduled. */
  function advanceTo(ms: number) {
    clock = ms;
    act(() => {
      const cb = pending;
      pending = null;
      cb?.(ms);
    });
  }

  test("draws a closed 64-point ring", () => {
    render();
    advanceTo(0);
    const d = node.writes.at(-1)!;
    expect(d.startsWith("M")).toBe(true);
    expect(d.endsWith("Z")).toBe(true);
    expect(points(d)).toHaveLength(64);
  });

  test("the first letter is the wordmark's M, centred in the 32-unit box", () => {
    render();
    advanceTo(0);
    const box = bbox(points(node.writes.at(-1)!));
    // The M of the mark is 27.02 x 23.99; centred leaves ~2.49 / ~4.0 of margin.
    expect(box.maxX - box.minX).toBeCloseTo(27.02, 1);
    expect(box.maxY - box.minY).toBeCloseTo(23.99, 1);
    expect(box.minX).toBeCloseTo((32 - 27.02) / 2, 1);
    expect(box.minY).toBeCloseTo((32 - 23.99) / 2, 1);
  });

  test("holds the letter, then morphs away from it", () => {
    render();
    advanceTo(0);
    const atRest = node.writes.at(-1)!;
    advanceTo(100); // still inside the 140ms hold
    expect(node.writes.at(-1)).toBe(atRest);
    advanceTo(140); // hold elapses, morph begins
    advanceTo(340); // ~halfway through the 400ms morph
    expect(node.writes.at(-1)).not.toBe(atRest);
  });

  test("never renders the letter it just left on the frame a morph completes", () => {
    // Regression: deriving the letter pair before advancing the state made the
    // completing frame draw the source letter for one frame — a visible flash.
    render();
    advanceTo(0);
    const first = points(node.writes.at(-1)!);
    advanceTo(140); // hold elapses, morph begins
    advanceTo(540); // 400ms later the morph completes and the state advances
    const after = points(node.writes.at(-1)!);
    let maxDrift = 0;
    for (let i = 0; i < first.length; i++)
      maxDrift = Math.max(
        maxDrift,
        Math.hypot(after[i][0] - first[i][0], after[i][1] - first[i][1])
      );
    // it must have landed on the NEXT letter, not snapped back to the first
    expect(maxDrift).toBeGreaterThan(5);
  });

  test("stops its animation frame when unmounted", () => {
    // The auth screens unmount this the moment the callback resolves. Without the
    // cleanup the loop keeps running against a detached node for the life of the page.
    const renderer = render();
    advanceTo(0);
    advanceTo(200);
    const scheduled = lastHandle;

    act(() => renderer.unmount());

    expect(cancelled).toContain(scheduled);
  });

  test("honours prefers-reduced-motion with a static mark", () => {
    reduceMotion = true;
    render();
    expect(node.writes).toHaveLength(1);
    expect(pending).toBeNull();
    expect(points(node.writes[0])).toHaveLength(64);
  });
});
