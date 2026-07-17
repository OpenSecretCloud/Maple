import { createContext, useCallback, useContext, type Dispatch, type SetStateAction } from "react";
import type { AgentSessionSelectionMemory } from "@/services/agentSessionSelection";

export type PersistentHomeNavigation = {
  returnToHome: (options?: { replace?: boolean }) => void;
  sidebarOpen: boolean | null;
  setSidebarOpen: Dispatch<SetStateAction<boolean | null>>;
  agentSessionSelection: AgentSessionSelectionMemory;
};

export const PersistentHomeNavigationContext = createContext<PersistentHomeNavigation | null>(null);

export function usePersistentHomeNavigation() {
  const context = useContext(PersistentHomeNavigationContext);
  if (!context) {
    throw new Error(
      "usePersistentHomeNavigation must be used within PersistentHomeNavigationProvider"
    );
  }
  return context;
}

export function usePersistentSidebarState(
  isCompactLayout: boolean
): readonly [boolean, Dispatch<SetStateAction<boolean>>] {
  const { sidebarOpen, setSidebarOpen } = usePersistentHomeNavigation();
  const isOpen = sidebarOpen ?? !isCompactLayout;
  const setIsOpen = useCallback<Dispatch<SetStateAction<boolean>>>(
    (nextValue) => {
      setSidebarOpen((currentValue) => {
        const currentIsOpen = currentValue ?? !isCompactLayout;
        return typeof nextValue === "function" ? nextValue(currentIsOpen) : nextValue;
      });
    },
    [isCompactLayout, setSidebarOpen]
  );

  return [isOpen, setIsOpen] as const;
}
