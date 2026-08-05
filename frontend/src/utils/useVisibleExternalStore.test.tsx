import { afterEach, describe, expect, test } from "bun:test";
import { Profiler } from "react";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { useVisibleExternalStore } from "./useVisibleExternalStore";

class NumberStore {
  private value = 0;
  private readonly listeners = new Set<() => void>();

  readonly subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  readonly getSnapshot = (): number => this.value;

  get subscriberCount(): number {
    return this.listeners.size;
  }

  set(value: number): void {
    this.value = value;
    for (const listener of this.listeners) listener();
  }
}

function StoreProbe({ isVisible, store }: { isVisible: boolean; store: NumberStore }) {
  const value = useVisibleExternalStore(
    isVisible,
    store.subscribe,
    store.getSnapshot,
    store.getSnapshot
  );
  return <span>{value}</span>;
}

describe("useVisibleExternalStore", () => {
  let renderer: ReactTestRenderer | null = null;

  afterEach(() => {
    if (renderer) {
      act(() => renderer?.unmount());
      renderer = null;
    }
  });

  test("renders store notifications only while visible and catches up on return", () => {
    const store = new NumberStore();
    let renderCount = 0;
    const probe = (isVisible: boolean) => (
      <Profiler id="visible-store" onRender={() => renderCount++}>
        <StoreProbe isVisible={isVisible} store={store} />
      </Profiler>
    );

    act(() => {
      renderer = create(probe(true));
    });
    expect(store.subscriberCount).toBe(1);

    act(() => store.set(1));
    expect(renderer?.toJSON()).toMatchObject({ children: ["1"] });

    act(() => renderer?.update(probe(false)));
    expect(store.subscriberCount).toBe(0);
    const hiddenRenderCount = renderCount;

    act(() => store.set(2));
    expect(renderCount).toBe(hiddenRenderCount);
    expect(renderer?.toJSON()).toMatchObject({ children: ["1"] });

    act(() => renderer?.update(probe(true)));
    expect(store.subscriberCount).toBe(1);
    expect(renderer?.toJSON()).toMatchObject({ children: ["2"] });
  });
});
