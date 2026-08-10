import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import { useState } from "react";
import { act, create, type ReactTestInstance, type ReactTestRenderer } from "react-test-renderer";

import { ResizableSidebarLayout } from "./ResizableSidebarLayout";

class FakeWindow extends EventTarget {
  innerWidth = 1_000;
  prefersReducedMotion = false;
  readonly storage = new Map<string, string>();
  readonly timers = new Map<number, TimerHandler>();
  private nextTimerId = 1;

  readonly localStorage = {
    getItem: (key: string) => this.storage.get(key) ?? null,
    setItem: (key: string, value: string) => this.storage.set(key, value)
  };

  clearTimeout = (timerId: number) => {
    this.timers.delete(timerId);
  };

  setTimeout = (handler: TimerHandler) => {
    const timerId = this.nextTimerId++;
    this.timers.set(timerId, handler);
    return timerId;
  };

  matchMedia = () =>
    ({
      addEventListener: () => {},
      matches: this.prefersReducedMotion,
      removeEventListener: () => {}
    }) as unknown as MediaQueryList;

  runTimer(timerId: number) {
    const handler = this.timers.get(timerId);
    this.timers.delete(timerId);
    if (typeof handler === "function") handler();
  }
}

class PointerCaptureTarget {
  readonly captured = new Set<number>();
  readonly releases: number[] = [];

  hasPointerCapture(pointerId: number): boolean {
    return this.captured.has(pointerId);
  }

  releasePointerCapture(pointerId: number): void {
    this.releases.push(pointerId);
    this.captured.delete(pointerId);
  }

  setPointerCapture(pointerId: number): void {
    this.captured.add(pointerId);
  }
}

const originalGlobals = {
  document: Object.getOwnPropertyDescriptor(globalThis, "document"),
  window: Object.getOwnPropertyDescriptor(globalThis, "window")
};

function setGlobal(name: "document" | "window", value: unknown): void {
  Object.defineProperty(globalThis, name, {
    configurable: true,
    value,
    writable: true
  });
}

function restoreGlobal(name: "document" | "window", descriptor: PropertyDescriptor | undefined) {
  if (descriptor) Object.defineProperty(globalThis, name, descriptor);
  else Reflect.deleteProperty(globalThis, name);
}

function pointerEvent(target: PointerCaptureTarget, pointerId: number, clientX: number) {
  return {
    button: 0,
    clientX,
    currentTarget: target,
    isPrimary: true,
    pointerId,
    preventDefault: mock(() => {})
  };
}

function FakeSidebar({ isOpen, onToggle }: { isOpen: boolean; onToggle: () => void }) {
  return <button data-sidebar-open={isOpen} onClick={onToggle} />;
}

function Harness({
  isCompactLayout = false,
  mode = "chat",
  userId
}: {
  isCompactLayout?: boolean;
  mode?: "agent" | "chat";
  userId?: string;
}) {
  const [isOpen, setIsOpen] = useState(true);
  return (
    <ResizableSidebarLayout
      data-testid="layout"
      isCompactLayout={isCompactLayout}
      isOpen={isOpen}
      mode={mode}
      onOpenChange={setIsOpen}
      sidebar={<FakeSidebar isOpen={isOpen} onToggle={() => setIsOpen((open) => !open)} />}
      userId={userId}
    >
      <main data-open={isOpen} />
    </ResizableSidebarLayout>
  );
}

describe("resizable sidebar layout", () => {
  let renderer: ReactTestRenderer | null = null;
  let fakeWindow: FakeWindow;

  const layout = (): ReactTestInstance => {
    if (!renderer) throw new Error("layout did not mount");
    const root = renderer.root
      .findAllByProps({ "data-testid": "layout" })
      .find((node) => node.type === "div");
    if (!root) throw new Error("layout root did not mount");
    return root;
  };

  const resizeHandle = (): ReactTestInstance => renderer!.root.findByProps({ role: "separator" });
  const main = (): ReactTestInstance => renderer!.root.findByType("main");
  const sidebarButton = (): ReactTestInstance => renderer!.root.findByType("button");
  const keyEvent = (key: string, shiftKey = false) => ({
    key,
    preventDefault: mock(() => {}),
    shiftKey
  });

  beforeEach(() => {
    fakeWindow = new FakeWindow();
    setGlobal("window", fakeWindow);
    setGlobal("document", { body: { style: { cursor: "", userSelect: "" } } });
  });

  afterEach(() => {
    if (renderer) act(() => renderer?.unmount());
    renderer = null;
    restoreGlobal("document", originalGlobals.document);
    restoreGlobal("window", originalGlobals.window);
  });

  test("exposes an accessible separator and supports keyboard resizing", () => {
    act(() => {
      renderer = create(<Harness />);
    });

    expect(resizeHandle().props).toMatchObject({
      "aria-label": "Resize Chat sidebar",
      "aria-orientation": "vertical",
      "aria-valuemax": 400,
      "aria-valuemin": 240,
      "aria-valuenow": 296,
      tabIndex: 0
    });

    act(() => resizeHandle().props.onKeyDown(keyEvent("ArrowLeft")));
    expect(layout().props.style["--sidebar-width"]).toBe("288px");
    act(() => resizeHandle().props.onKeyDown(keyEvent("ArrowRight", true)));
    expect(layout().props.style["--sidebar-width"]).toBe("320px");
    act(() => resizeHandle().props.onKeyDown(keyEvent("Home")));
    expect(layout().props.style["--sidebar-width"]).toBe("240px");
    act(() => resizeHandle().props.onKeyDown(keyEvent("End")));
    expect(layout().props.style["--sidebar-width"]).toBe("400px");
    act(() => resizeHandle().props.onDoubleClick());
    expect(layout().props.style["--sidebar-width"]).toBe("296px");
    act(() => resizeHandle().props.onKeyDown(keyEvent("Enter")));
    expect(main().props["data-open"]).toBe(false);
  });

  test("keeps the panel, divider, and grid edge together through live collapse and reversal", () => {
    act(() => {
      renderer = create(<Harness />);
    });
    const target = new PointerCaptureTarget();

    act(() => resizeHandle().props.onPointerDown(pointerEvent(target, 1, 296)));
    act(() => resizeHandle().props.onPointerMove(pointerEvent(target, 1, 200)));
    expect(layout().props.style["--sidebar-width"]).toBe("240px");
    expect(layout().props.style["--sidebar-grid-template"]).toBe("240px minmax(0, 1fr)");
    expect(layout().props.className).not.toContain("transition-[grid-template-columns]");

    act(() => resizeHandle().props.onPointerUp(pointerEvent(target, 1, 296)));
    act(() => resizeHandle().props.onPointerDown(pointerEvent(target, 2, 296)));
    act(() => resizeHandle().props.onPointerMove(pointerEvent(target, 2, 120)));
    expect(main().props["data-open"]).toBe(false);
    expect(target.captured.has(2)).toBe(true);
    expect(layout().props.style["--sidebar-width"]).toBe("296px");
    expect(layout().props.style["--sidebar-grid-template"]).toBe("0px minmax(0, 1fr)");

    act(() => resizeHandle().props.onPointerMove(pointerEvent(target, 2, 121)));
    expect(main().props["data-open"]).toBe(true);
    expect(target.captured.has(2)).toBe(true);
    expect(layout().props.style["--sidebar-width"]).toBe("296px");
    expect(layout().props.style["--sidebar-grid-template"]).toBe("240px minmax(0, 1fr)");

    act(() => resizeHandle().props.onPointerMove(pointerEvent(target, 2, 280)));
    expect(layout().props.style["--sidebar-width"]).toBe("296px");
    expect(layout().props.style["--sidebar-grid-template"]).toBe("280px minmax(0, 1fr)");
    expect(layout().props.className).toContain("transition-[grid-template-columns]");
  });

  test("uses the final release coordinate in both directions", () => {
    act(() => {
      renderer = create(<Harness />);
    });
    const target = new PointerCaptureTarget();

    act(() => resizeHandle().props.onPointerDown(pointerEvent(target, 3, 296)));
    act(() => resizeHandle().props.onPointerMove(pointerEvent(target, 3, 120)));
    act(() => resizeHandle().props.onPointerUp(pointerEvent(target, 3, 121)));
    expect(main().props["data-open"]).toBe(true);
    expect(layout().props.style["--sidebar-width"]).toBe("296px");
    expect(layout().props.style["--sidebar-grid-template"]).toBe("240px minmax(0, 1fr)");
    expect(target.releases).toEqual([3]);

    const timer = [...fakeWindow.timers.keys()][0];
    act(() => fakeWindow.runTimer(timer));
    act(() => resizeHandle().props.onPointerDown(pointerEvent(target, 4, 240)));
    act(() => resizeHandle().props.onPointerMove(pointerEvent(target, 4, 121)));
    act(() => resizeHandle().props.onPointerUp(pointerEvent(target, 4, 120)));
    expect(main().props["data-open"]).toBe(false);
    expect(target.releases).toEqual([3, 4]);
  });

  test("restores the open state after cancellation and window blur", () => {
    act(() => {
      renderer = create(<Harness />);
    });
    const target = new PointerCaptureTarget();

    act(() => resizeHandle().props.onPointerDown(pointerEvent(target, 5, 296)));
    act(() => resizeHandle().props.onPointerMove(pointerEvent(target, 5, 120)));
    act(() => resizeHandle().props.onPointerCancel(pointerEvent(target, 5, 120)));
    expect(main().props["data-open"]).toBe(true);
    expect(target.releases).toEqual([5]);

    const transitionTimer = [...fakeWindow.timers.keys()][0];
    act(() => fakeWindow.runTimer(transitionTimer));
    act(() => resizeHandle().props.onPointerDown(pointerEvent(target, 6, 296)));
    act(() => resizeHandle().props.onPointerMove(pointerEvent(target, 6, 120)));
    act(() => {
      fakeWindow.dispatchEvent(new Event("blur"));
    });
    expect(main().props["data-open"]).toBe(true);
    expect(target.releases).toEqual([5, 6]);
  });

  test("finishes a collapse on its original animation timer", () => {
    act(() => {
      renderer = create(<Harness />);
    });
    const target = new PointerCaptureTarget();

    act(() => resizeHandle().props.onPointerDown(pointerEvent(target, 7, 296)));
    act(() => resizeHandle().props.onPointerMove(pointerEvent(target, 7, 120)));
    const transitionTimer = [...fakeWindow.timers.keys()][0];
    act(() => resizeHandle().props.onPointerUp(pointerEvent(target, 7, 120)));
    expect([...fakeWindow.timers.keys()]).toEqual([transitionTimer]);

    act(() => fakeWindow.runTimer(transitionTimer));
    expect(layout().props.className).not.toContain("transition-[grid-template-columns]");
    expect(sidebarButton().props["data-sidebar-open"]).toBe(false);
  });

  test("skips retained transitions when motion is reduced", () => {
    fakeWindow.prefersReducedMotion = true;
    act(() => {
      renderer = create(<Harness />);
    });

    act(() => sidebarButton().props.onClick());
    expect(layout().props.style["--sidebar-grid-template"]).toBe("0px minmax(0, 1fr)");
    expect(layout().props.className).not.toContain("transition-[grid-template-columns]");
    expect(sidebarButton().props["data-sidebar-open"]).toBe(false);
  });

  test("does not render desktop resize controls in compact layouts", () => {
    act(() => {
      renderer = create(<Harness isCompactLayout />);
    });

    expect(renderer!.root.findAllByProps({ role: "separator" })).toHaveLength(0);
    expect(layout().props.className).not.toContain("grid-cols-[var(--sidebar-grid-template)]");
  });

  test("shares the persisted width between Agent and Chat", () => {
    act(() => {
      renderer = create(<Harness mode="chat" userId="account/a" />);
    });
    const target = new PointerCaptureTarget();

    act(() => resizeHandle().props.onPointerDown(pointerEvent(target, 8, 296)));
    act(() => resizeHandle().props.onPointerUp(pointerEvent(target, 8, 360)));
    expect(layout().props.style["--sidebar-width"]).toBe("360px");

    act(() => renderer?.unmount());
    renderer = null;
    act(() => {
      renderer = create(<Harness mode="agent" userId="account/a" />);
    });
    expect(layout().props.style["--sidebar-width"]).toBe("360px");
  });
});
