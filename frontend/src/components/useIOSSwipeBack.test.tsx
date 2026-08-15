import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import type { CSSProperties, PointerEvent as ReactPointerEvent } from "react";
import { act, create, type ReactTestRenderer } from "react-test-renderer";

mock.module("@/utils/platform", () => ({
  isIOS: () => true,
  isTauriMobile: () => true
}));

const { useIOSSwipeBack } = await import("./useIOSSwipeBack");

class FakeNode {}

class FakeElement extends FakeNode {
  closest() {
    return null;
  }
}

class FakeStyle {
  readonly values = new Map<string, string>();
  onRemove: (() => void) | null = null;

  setProperty(name: string, value: string) {
    this.values.set(name, value);
  }

  removeProperty(name: string) {
    this.onRemove?.();
    this.values.delete(name);
  }
}

class FakeSurface {
  readonly clientWidth = 294;
  readonly ownerDocument: { documentElement: { style: FakeStyle } };

  constructor(style: FakeStyle) {
    this.ownerDocument = { documentElement: { style } };
  }

  contains(target: unknown) {
    return target instanceof FakeNode;
  }

  releasePointerCapture() {}

  setPointerCapture() {}
}

class FakeWindow {
  readonly innerWidth = 294;
  readonly animationFrames = new Map<number, FrameRequestCallback>();
  private nextAnimationFrame = 1;

  cancelAnimationFrame = (frameId: number) => {
    this.animationFrames.delete(frameId);
  };

  matchMedia = () =>
    ({
      addEventListener: () => {},
      matches: false,
      removeEventListener: () => {}
    }) as unknown as MediaQueryList;

  requestAnimationFrame = (callback: FrameRequestCallback) => {
    const frameId = this.nextAnimationFrame++;
    this.animationFrames.set(frameId, callback);
    return frameId;
  };

  runAnimationFrames() {
    const frames = [...this.animationFrames.values()];
    this.animationFrames.clear();
    for (const frame of frames) frame(performance.now());
  }
}

type SwipeHookResult = {
  parentStyle: CSSProperties | undefined;
  pointerHandlers: {
    onPointerDownCapture: (event: ReactPointerEvent<HTMLDivElement>) => void;
    onPointerMoveCapture: (event: ReactPointerEvent<HTMLDivElement>) => void;
    onPointerUpCapture: (event: ReactPointerEvent<HTMLDivElement>) => void;
  };
  reset: () => void;
};

let latestHook: SwipeHookResult | null = null;
let onComplete = mock(() => {});

function Probe() {
  latestHook = useIOSSwipeBack({
    getContext: () => "menu",
    onComplete
  });
  return null;
}

function currentHook(): SwipeHookResult {
  if (!latestHook) throw new Error("Swipe hook has not rendered");
  return latestHook;
}

function pointerEvent(
  surface: FakeSurface,
  target: FakeElement,
  clientX: number
): ReactPointerEvent<HTMLDivElement> {
  return {
    clientX,
    clientY: 100,
    currentTarget: surface,
    isPrimary: true,
    pointerId: 1,
    pointerType: "touch",
    preventDefault: mock(() => {}),
    target
  } as unknown as ReactPointerEvent<HTMLDivElement>;
}

const originalGlobals = {
  Element: Object.getOwnPropertyDescriptor(globalThis, "Element"),
  Node: Object.getOwnPropertyDescriptor(globalThis, "Node"),
  window: Object.getOwnPropertyDescriptor(globalThis, "window")
};

function setGlobal(name: "Element" | "Node" | "window", value: unknown) {
  Object.defineProperty(globalThis, name, { configurable: true, value, writable: true });
}

function restoreGlobal(name: "Element" | "Node" | "window", descriptor?: PropertyDescriptor) {
  if (descriptor) Object.defineProperty(globalThis, name, descriptor);
  else Reflect.deleteProperty(globalThis, name);
}

describe("useIOSSwipeBack", () => {
  let fakeWindow: FakeWindow;
  let renderer: ReactTestRenderer | null;

  beforeEach(() => {
    fakeWindow = new FakeWindow();
    renderer = null;
    latestHook = null;
    onComplete = mock(() => {});
    setGlobal("Element", FakeElement);
    setGlobal("Node", FakeNode);
    setGlobal("window", fakeWindow);
  });

  afterEach(() => {
    if (renderer) act(() => renderer?.unmount());
    renderer = null;
    restoreGlobal("Element", originalGlobals.Element);
    restoreGlobal("Node", originalGlobals.Node);
    restoreGlobal("window", originalGlobals.window);
  });

  test("settles from the last painted frame when the release move is still queued", () => {
    const style = new FakeStyle();
    const surface = new FakeSurface(style);
    const target = new FakeElement();

    act(() => {
      renderer = create(<Probe />);
    });
    act(() => {
      currentHook().pointerHandlers.onPointerDownCapture(pointerEvent(surface, target, 0));
      currentHook().pointerHandlers.onPointerMoveCapture(pointerEvent(surface, target, 30));
    });
    fakeWindow.runAnimationFrames();

    act(() => {
      currentHook().pointerHandlers.onPointerMoveCapture(pointerEvent(surface, target, 177));
      currentHook().pointerHandlers.onPointerUpCapture(pointerEvent(surface, target, 0));
    });

    const transition = [...style.values.entries()].find(([name]) =>
      name.includes("transition")
    )?.[1];
    expect(transition).toContain("260ms");
    expect(fakeWindow.animationFrames.size).toBe(0);
    expect(onComplete).not.toHaveBeenCalled();
  });

  test("removes swipe variables only after React removes their inline consumers", () => {
    const style = new FakeStyle();
    const surface = new FakeSurface(style);
    const target = new FakeElement();

    act(() => {
      renderer = create(<Probe />);
    });
    act(() => {
      currentHook().pointerHandlers.onPointerDownCapture(pointerEvent(surface, target, 0));
      currentHook().pointerHandlers.onPointerMoveCapture(pointerEvent(surface, target, 120));
    });
    fakeWindow.runAnimationFrames();

    const parentStylesDuringRemoval: Array<CSSProperties | undefined> = [];
    style.onRemove = () => parentStylesDuringRemoval.push(currentHook().parentStyle);

    act(() => currentHook().reset());

    expect(parentStylesDuringRemoval.length).toBeGreaterThan(0);
    expect(parentStylesDuringRemoval.every((style) => style === undefined)).toBe(true);
    expect(style.values.size).toBe(0);
  });
});
