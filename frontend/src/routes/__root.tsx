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
import { getSafeInternalRedirect } from "@/utils/internalRedirect";
import { SETTINGS_SHELL_POP_EVENT } from "@/utils/settingsNavigation";
import { cn, useIsLandscapeMobile, useIsMobile } from "@/utils/utils";

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
  const routedSurfaceRef = useRef<HTMLDivElement>(null);
  const isMobile = useIsMobile();
  const isLandscapeMobile = useIsLandscapeMobile();
  const isCompactLayout = isMobile || isLandscapeMobile;
  const [isSettingsPopping, setIsSettingsPopping] = useState(false);

  const isHomeRoute = location.pathname === "/";
  const isSettingsRoute =
    location.pathname === "/settings" || location.pathname.startsWith("/settings/");
  const keepAuthenticatedHomeMounted = !!auth.user && (isHomeRoute || isSettingsRoute);

  useLayoutEffect(() => {
    // Queue cleanup before route-level passive effects initialize Agent Mode.
    // A failed transition is surfaced by Agent Mode's matching wait gate.
    void transitionAgentAuthUser(userId).catch(() => {});
  }, [userId]);

  useEffect(() => {
    const persistentHome = persistentHomeRef.current;
    if (!persistentHome) return;

    if (isSettingsRoute) {
      persistentHome.setAttribute("inert", "");
    } else {
      persistentHome.removeAttribute("inert");
    }
  }, [isSettingsRoute, keepAuthenticatedHomeMounted]);

  useEffect(() => {
    const handleSettingsPop = () => setIsSettingsPopping(true);
    window.addEventListener(SETTINGS_SHELL_POP_EVENT, handleSettingsPop);
    return () => window.removeEventListener(SETTINGS_SHELL_POP_EVENT, handleSettingsPop);
  }, []);

  useLayoutEffect(() => {
    if (!isSettingsRoute) setIsSettingsPopping(false);
  }, [isSettingsRoute]);

  useEffect(() => {
    const routedSurface = routedSurfaceRef.current;
    if (!routedSurface) return;

    routedSurface.inert = isSettingsRoute && isSettingsPopping;
    return () => {
      routedSurface.inert = false;
    };
  }, [isSettingsPopping, isSettingsRoute]);

  // TODO... put something here, but showing nothing looks nicer than "Loading..."
  if (auth.loading) {
    return <></>;
  }

  return (
    <PersistentHomeNavigationProvider>
      {keepAuthenticatedHomeMounted && (
        <div
          ref={persistentHomeRef}
          aria-hidden={isSettingsRoute || undefined}
          className={cn(
            isCompactLayout
              ? [
                  "maple-navigation-page fixed inset-0",
                  isSettingsRoute && [
                    "pointer-events-none",
                    !isSettingsPopping && "maple-navigation-page-covered"
                  ]
                ]
              : isSettingsRoute
                ? "pointer-events-none fixed inset-0 invisible"
                : "contents"
          )}
        >
          <AuthenticatedHomeContent homeLocationHref={isHomeRoute ? location.href : null} />
        </div>
      )}

      <div
        ref={routedSurfaceRef}
        aria-hidden={isSettingsRoute && isSettingsPopping ? true : undefined}
        className={cn(
          isSettingsRoute
            ? isCompactLayout
              ? [
                  "maple-navigation-page fixed inset-0 z-50 overflow-hidden bg-background shadow-[-12px_0_28px_rgba(0,0,0,0.12)]",
                  isSettingsPopping
                    ? "maple-navigation-page-pop pointer-events-none"
                    : "maple-navigation-page-enter"
                ]
              : "fixed inset-0 z-50 overflow-hidden bg-background"
            : "contents"
        )}
      >
        <Outlet />
      </div>
      {(isHomeRoute || isSettingsRoute) && <VerificationModal />}
      {!isSettingsRoute && <TeamSeatMismatchAlert />}
      <ExternalUrlConfirmHandler />
    </PersistentHomeNavigationProvider>
  );
}
