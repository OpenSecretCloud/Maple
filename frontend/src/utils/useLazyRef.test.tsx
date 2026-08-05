import { afterEach, describe, expect, test } from "bun:test";
import { createRef, forwardRef, useImperativeHandle, type MutableRefObject } from "react";
import { act, create, type ReactTestRenderer } from "react-test-renderer";

import { useLazyRef } from "./useLazyRef";

interface LazyValue {
  readonly sequence: number;
}

interface LazyRefProbeHandle {
  getRef: () => MutableRefObject<LazyValue>;
}

const LazyRefProbe = forwardRef<LazyRefProbeHandle, { createValue: () => LazyValue }>(
  function LazyRefProbe({ createValue }, forwardedRef) {
    const valueRef = useLazyRef(createValue);
    useImperativeHandle(forwardedRef, () => ({ getRef: () => valueRef }), [valueRef]);
    return null;
  }
);

function getCurrentRef(probeRef: React.RefObject<LazyRefProbeHandle>) {
  const handle = probeRef.current;
  if (!handle) throw new Error("LazyRefProbe is not mounted");
  return handle.getRef();
}

function mountProbe(probeRef: React.RefObject<LazyRefProbeHandle>, createValue: () => LazyValue) {
  return create(<LazyRefProbe ref={probeRef} createValue={createValue} />);
}

function updateProbe(
  renderer: ReactTestRenderer,
  probeRef: React.RefObject<LazyRefProbeHandle>,
  createValue: () => LazyValue
) {
  renderer.update(<LazyRefProbe ref={probeRef} createValue={createValue} />);
  return null;
}

describe("useLazyRef", () => {
  let renderer: ReactTestRenderer | null = null;

  afterEach(() => {
    if (renderer) {
      act(() => renderer?.unmount());
      renderer = null;
    }
  });

  test("calls the initializer once per mount across rerenders", () => {
    let initializerCalls = 0;
    const createValue = () => ({ sequence: ++initializerCalls });
    const probeRef = createRef<LazyRefProbeHandle>();

    act(() => {
      renderer = mountProbe(probeRef, createValue);
    });
    const firstRef = getCurrentRef(probeRef);
    act(() => {
      if (!renderer) throw new Error("LazyRefProbe renderer is missing");
      updateProbe(renderer, probeRef, createValue);
      updateProbe(renderer, probeRef, createValue);
    });

    expect(initializerCalls).toBe(1);
    expect(getCurrentRef(probeRef)).toBe(firstRef);
    expect(getCurrentRef(probeRef).current.sequence).toBe(1);

    act(() => renderer?.unmount());
    renderer = null;

    act(() => {
      renderer = mountProbe(probeRef, createValue);
    });

    expect(initializerCalls).toBe(2);
    expect(getCurrentRef(probeRef).current.sequence).toBe(2);
  });

  test("keeps the ref and value identities stable across rerenders", () => {
    const value: LazyValue = { sequence: 1 };
    const createValue = () => value;
    const probeRef = createRef<LazyRefProbeHandle>();

    act(() => {
      renderer = mountProbe(probeRef, createValue);
    });
    const firstRef = getCurrentRef(probeRef);
    act(() => {
      if (!renderer) throw new Error("LazyRefProbe renderer is missing");
      updateProbe(renderer, probeRef, createValue);
    });

    expect(getCurrentRef(probeRef)).toBe(firstRef);
    expect(getCurrentRef(probeRef).current).toBe(value);
  });
});
