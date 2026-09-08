import { useCallback, useSyncExternalStore } from "react";

type ExternalStoreSubscribe = (listener: () => void) => () => void;

const unsubscribeNoop = () => {};

/**
 * Suppresses store-driven renders while retained UI is hidden without pausing
 * the store itself. React reads the current snapshot when the UI becomes
 * visible and re-subscribes, so updates made while hidden are observed then.
 */
export function useVisibleExternalStore<T>(
  isVisible: boolean,
  subscribe: ExternalStoreSubscribe,
  getSnapshot: () => T,
  getServerSnapshot: () => T = getSnapshot
): T {
  const subscribeWhileVisible = useCallback(
    (listener: () => void) => {
      if (!isVisible) return unsubscribeNoop;
      return subscribe(listener);
    },
    [isVisible, subscribe]
  );

  return useSyncExternalStore(subscribeWhileVisible, getSnapshot, getServerSnapshot);
}
