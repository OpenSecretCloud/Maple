import { createContext, useContext, useLayoutEffect } from "react";
import { useLazyRef } from "@/utils/useLazyRef";

export type SettingsNavigationLockContextValue = {
  isNavigationLocked: boolean;
  setLock: (id: symbol, locked: boolean) => void;
};

export const SettingsNavigationLockContext =
  createContext<SettingsNavigationLockContextValue | null>(null);

export function useSettingsNavigationLock(locked: boolean) {
  const context = useContext(SettingsNavigationLockContext);
  const lockId = useLazyRef(() => Symbol("settings-navigation-lock"));
  const setLock = context?.setLock;

  useLayoutEffect(() => {
    if (!setLock) return;
    const id = lockId.current;
    setLock(id, locked);
    return () => setLock(id, false);
  }, [lockId, locked, setLock]);

  if (!context) {
    throw new Error("useSettingsNavigationLock must be used within settings");
  }
}

export function useSettingsNavigationLockState() {
  const context = useContext(SettingsNavigationLockContext);

  if (!context) {
    throw new Error("useSettingsNavigationLockState must be used within settings");
  }

  return context.isNavigationLocked;
}
