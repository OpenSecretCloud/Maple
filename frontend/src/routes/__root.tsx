import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useOpenSecret } from "@opensecret/react";
import { OpenSecretContextType } from "@opensecret/react";
import { createRootRouteWithContext, Outlet, useLocation } from "@tanstack/react-router";
import {
  AuthenticatedHomeContent,
  PersistentHomeNavigationProvider
} from "@/components/AuthenticatedHomeContent";
import { ExternalUrlConfirmHandler } from "@/components/ExternalUrlConfirmHandler";
import { TeamSeatMismatchAlert } from "@/components/team/TeamSeatMismatchAlert";
import { VerificationModal } from "@/components/VerificationModal";
import { transitionAgentAuthUser } from "@/services/agentRuntimeService";
import { proxyService } from "@/services/proxyService";
import { getSafeInternalRedirect } from "@/utils/internalRedirect";

interface RootRouterContext {
  os: OpenSecretContextType;
}

export type RootSearchParams = {
  login?: string;
  next?: string;
  selected_plan?: string;
  success?: boolean;
  canceled?: boolean;
  provider?: string;
};

export const Route = createRootRouteWithContext<RootRouterContext>()({
  component: Root,
  validateSearch: (search: Record<string, unknown>): RootSearchParams => ({
    login: typeof search.login === "string" ? search.login : undefined,
    next: getSafeInternalRedirect(search.next),
    selected_plan: typeof search.selected_plan === "string" ? search.selected_plan : undefined,
    success: typeof search.success === "boolean" ? search.success : undefined,
    canceled: typeof search.canceled === "boolean" ? search.canceled : undefined,
    provider: typeof search.provider === "string" ? search.provider : undefined
  })
});

function Root() {
  const { auth } = useOpenSecret();
  const userId = auth.user?.user.id || null;
  const location = useLocation();
  const persistentHomeRef = useRef<HTMLDivElement>(null);
  const [proxyReadyUserId, setProxyReadyUserId] = useState<string | null | undefined>();

  const isHomeRoute = location.pathname === "/";
  const isSettingsRoute =
    location.pathname === "/settings" || location.pathname.startsWith("/settings/");
  const keepAuthenticatedHomeMounted = !!auth.user && (isHomeRoute || isSettingsRoute);

  useLayoutEffect(() => {
    if (auth.loading) return;
    let cancelled = false;
    let retryTimer: number | undefined;
    setProxyReadyUserId(undefined);

    // Update both account coordinators synchronously before route-level passive
    // effects can initialize account-bound work. Proxy readiness gates the UI;
    // a failed native scrub is retried without activating the next account.
    const proxyTransition = proxyService.transitionAuthenticatedUser(userId);
    void transitionAgentAuthUser(userId).catch(() => {});

    const waitForProxyTransition = async (transition: Promise<void>) => {
      try {
        await transition;
        if (!cancelled) setProxyReadyUserId(userId);
      } catch {
        if (!cancelled) {
          retryTimer = window.setTimeout(() => {
            void waitForProxyTransition(proxyService.transitionAuthenticatedUser(userId));
          }, 500);
        }
      }
    };
    void waitForProxyTransition(proxyTransition);

    return () => {
      cancelled = true;
      if (retryTimer !== undefined) window.clearTimeout(retryTimer);
    };
  }, [auth.loading, userId]);

  useEffect(() => {
    const persistentHome = persistentHomeRef.current;
    if (!persistentHome) return;

    if (isSettingsRoute) {
      persistentHome.setAttribute("inert", "");
    } else {
      persistentHome.removeAttribute("inert");
    }
  }, [isSettingsRoute, keepAuthenticatedHomeMounted]);

  // Never strand a user on a blank signed-out screen while a redundant cleanup
  // retry is in progress. A newly authenticated account still waits for the
  // previous account's proxy state to be scrubbed.
  if (auth.loading || (userId !== null && proxyReadyUserId !== userId)) {
    return <></>;
  }

  return (
    <PersistentHomeNavigationProvider>
      {keepAuthenticatedHomeMounted && (
        <div
          ref={persistentHomeRef}
          aria-hidden={isSettingsRoute || undefined}
          className={isSettingsRoute ? "pointer-events-none fixed inset-0 invisible" : "contents"}
        >
          <AuthenticatedHomeContent homeLocationHref={isHomeRoute ? location.href : null} />
        </div>
      )}

      <div
        className={
          isSettingsRoute ? "fixed inset-0 z-50 overflow-hidden bg-background" : "contents"
        }
      >
        <Outlet />
      </div>
      {(isHomeRoute || isSettingsRoute) && <VerificationModal />}
      {!isSettingsRoute && <TeamSeatMismatchAlert />}
      <ExternalUrlConfirmHandler />
    </PersistentHomeNavigationProvider>
  );
}
