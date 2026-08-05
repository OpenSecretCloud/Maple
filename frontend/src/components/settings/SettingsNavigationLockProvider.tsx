import { useCallback, useMemo, useState, type ReactNode } from "react";
import {
  SettingsNavigationLockContext,
  type SettingsNavigationLockContextValue
} from "@/contexts/SettingsNavigationLockContext";
import { useLazyRef } from "@/utils/useLazyRef";

export function SettingsNavigationLockProvider({ children }: { children: ReactNode }) {
  const locksRef = useLazyRef(() => new Set<symbol>());
  const [lockCount, setLockCount] = useState(0);

  const setLock = useCallback(
    (id: symbol, locked: boolean) => {
      const locks = locksRef.current;
      const wasLocked = locks.has(id);

      if (locked && !wasLocked) {
        locks.add(id);
        setLockCount(locks.size);
      } else if (!locked && wasLocked) {
        locks.delete(id);
        setLockCount(locks.size);
      }
    },
    [locksRef]
  );

  const value = useMemo<SettingsNavigationLockContextValue>(
    () => ({ isNavigationLocked: lockCount > 0, setLock }),
    [lockCount, setLock]
  );

  return (
    <SettingsNavigationLockContext.Provider value={value}>
      {children}
    </SettingsNavigationLockContext.Provider>
  );
}
