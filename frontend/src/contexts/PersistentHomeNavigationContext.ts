import { createContext, useContext } from "react";

export type PersistentHomeNavigation = {
  homeHref: string;
  returnToHome: (options?: { replace?: boolean }) => void;
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
