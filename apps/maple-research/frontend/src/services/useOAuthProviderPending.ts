import { useCallback, useEffect, useRef, useState } from "react";

type RedirectProvider = "github" | "google";

export function useOAuthProviderPending() {
  const pending = useRef<RedirectProvider | null>(null);
  const [pendingProvider, setPendingProvider] = useState<RedirectProvider | null>(null);
  const clearPendingProvider = useCallback(() => {
    pending.current = null;
    setPendingProvider(null);
  }, []);

  const beginProvider = (provider: RedirectProvider): boolean => {
    // React state alone cannot fence another tap before the next render.
    if (pending.current !== null) return false;
    pending.current = provider;
    setPendingProvider(provider);
    return true;
  };

  useEffect(() => {
    // Web provider redirects can leave this page in the back-forward cache.
    const restoreLogin = (event: PageTransitionEvent) => {
      if (event.persisted) clearPendingProvider();
    };
    window.addEventListener("pageshow", restoreLogin);
    return () => window.removeEventListener("pageshow", restoreLogin);
  }, [clearPendingProvider]);

  return { pendingProvider, beginProvider, clearPendingProvider };
}
