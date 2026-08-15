import { afterEach, describe, expect, test } from "bun:test";
import { act, create, type ReactTestRenderer } from "react-test-renderer";

import {
  getDialogVisualViewportLayout,
  type DialogSafeAreaInsets,
  type DialogVisualViewport,
  type DialogVisualViewportLayout,
  useDialogVisualViewportLayout
} from "./useDialogVisualViewport";

class FakeVisualViewport {
  height: number;
  offsetTop: number;
  private listeners = {
    resize: new Set<() => void>(),
    scroll: new Set<() => void>()
  };

  constructor({ height, offsetTop }: { height: number; offsetTop: number }) {
    this.height = height;
    this.offsetTop = offsetTop;
  }

  addEventListener(type: "resize" | "scroll", listener: () => void) {
    this.listeners[type].add(listener);
  }

  removeEventListener(type: "resize" | "scroll", listener: () => void) {
    this.listeners[type].delete(listener);
  }

  dispatch(type: "resize" | "scroll") {
    for (const listener of this.listeners[type]) listener();
  }

  listenerCount(type: "resize" | "scroll") {
    return this.listeners[type].size;
  }
}

let latestLayout: DialogVisualViewportLayout | null = null;
let renderedLayouts: Array<DialogVisualViewportLayout | null> = [];
let renderer: ReactTestRenderer | null = null;

function Probe({
  enabled,
  safeAreaInsets = { bottom: 0, top: 0 },
  viewport
}: {
  enabled: boolean;
  safeAreaInsets?: DialogSafeAreaInsets;
  viewport: DialogVisualViewport;
}) {
  latestLayout = useDialogVisualViewportLayout(enabled, viewport, () => safeAreaInsets);
  renderedLayouts.push(latestLayout);
  return null;
}

afterEach(() => {
  if (renderer) act(() => renderer?.unmount());
  renderer = null;
  latestLayout = null;
  renderedLayouts = [];
});

describe("dialog visual viewport layout", () => {
  test("centers within the visible keyboard-adjusted area with breathing room", () => {
    expect(getDialogVisualViewportLayout({ height: 320, offsetTop: 160 })).toEqual({
      centerY: 320,
      maxHeight: 288
    });
  });

  test("centers inside the native safe area when its insets exceed the margin", () => {
    expect(
      getDialogVisualViewportLayout({ height: 384, offsetTop: 256 }, { bottom: 34, top: 45 })
    ).toEqual({
      centerY: 453.5,
      maxHeight: 305
    });
  });

  test("uses the bounded placement on the first render", () => {
    const viewport = new FakeVisualViewport({ height: 384, offsetTop: 256 });

    act(() => {
      renderer = create(
        <Probe
          enabled
          safeAreaInsets={{ bottom: 34, top: 45 }}
          viewport={viewport as unknown as DialogVisualViewport}
        />
      );
    });

    expect(renderedLayouts[0]).toEqual({ centerY: 453.5, maxHeight: 305 });
  });

  test("updates for both visual viewport resize and scroll events", () => {
    const viewport = new FakeVisualViewport({ height: 640, offsetTop: 0 });

    act(() => {
      renderer = create(<Probe enabled viewport={viewport as unknown as DialogVisualViewport} />);
    });
    expect(latestLayout).toEqual({ centerY: 320, maxHeight: 608 });

    act(() => {
      viewport.height = 320;
      viewport.dispatch("resize");
    });
    expect(latestLayout).toEqual({ centerY: 160, maxHeight: 288 });

    act(() => {
      viewport.offsetTop = 120;
      viewport.dispatch("scroll");
    });
    expect(latestLayout).toEqual({ centerY: 280, maxHeight: 288 });
  });

  test("does not subscribe when disabled and cleans up both listeners", () => {
    const viewport = new FakeVisualViewport({ height: 320, offsetTop: 120 });

    act(() => {
      renderer = create(
        <Probe enabled={false} viewport={viewport as unknown as DialogVisualViewport} />
      );
    });
    expect(latestLayout).toBeNull();
    expect(viewport.listenerCount("resize")).toBe(0);
    expect(viewport.listenerCount("scroll")).toBe(0);

    act(() => {
      renderer?.update(<Probe enabled viewport={viewport as unknown as DialogVisualViewport} />);
    });
    expect(viewport.listenerCount("resize")).toBe(1);
    expect(viewport.listenerCount("scroll")).toBe(1);

    act(() => renderer?.unmount());
    renderer = null;
    expect(viewport.listenerCount("resize")).toBe(0);
    expect(viewport.listenerCount("scroll")).toBe(0);
  });
});
