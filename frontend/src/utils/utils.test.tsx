import { afterEach, describe, expect, mock, test } from "bun:test";
import { act, create, type ReactTestRenderer } from "react-test-renderer";

import { useIsCoarsePointer } from "./utils";

const originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");

function CoarsePointerProbe() {
  return <span>{useIsCoarsePointer() ? "coarse" : "fine"}</span>;
}

describe("useIsCoarsePointer", () => {
  let renderer: ReactTestRenderer | null = null;

  afterEach(() => {
    if (renderer) act(() => renderer?.unmount());
    renderer = null;

    if (originalWindow) {
      Object.defineProperty(globalThis, "window", originalWindow);
    } else {
      Reflect.deleteProperty(globalThis, "window");
    }
  });

  test("detects a secondary coarse pointer on a hybrid device", () => {
    const matchMedia = mock((query: string) => ({
      matches: query.includes("(any-pointer: coarse)"),
      addEventListener: mock(() => {}),
      removeEventListener: mock(() => {})
    }));
    Object.defineProperty(globalThis, "window", {
      configurable: true,
      value: { matchMedia },
      writable: true
    });

    act(() => {
      renderer = create(<CoarsePointerProbe />);
    });

    expect(renderer?.toJSON()).toMatchObject({ children: ["coarse"] });
    expect(matchMedia).toHaveBeenCalledWith(
      "(pointer: coarse), (any-pointer: coarse), (hover: none)"
    );
  });
});
