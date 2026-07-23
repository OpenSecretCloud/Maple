import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, Outlet, useLocation, useRouter } from "@tanstack/react-router";
import { useOpenSecret } from "@opensecret/react";
import {
  ArrowLeft,
  BookOpen,
  CreditCard,
  Database,
  KeyRound,
  LogOut,
  MessageSquareText,
  Settings,
  ShieldCheck,
  UserRound,
  UsersRound
} from "lucide-react";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ComponentType
} from "react";
import { getBillingService } from "@/billing/billingService";
import { MapleWordmark } from "@/components/MapleWordmark";
import { SettingsNavigationLockProvider } from "@/components/settings/SettingsNavigationLockProvider";
import { useCompactSettingsLayout } from "@/components/settings/useCompactSettingsLayout";
import { useIOSSwipeBack } from "@/components/useIOSSwipeBack";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { usePersistentHomeNavigation } from "@/contexts/PersistentHomeNavigationContext";
import {
  useSettingsNavigationLock,
  useSettingsNavigationLockState
} from "@/contexts/SettingsNavigationLockContext";
import { stopAgentRuntimeForUser } from "@/services/agentRuntimeService";
import { useLocalState } from "@/state/useLocalState";
import type { TeamStatus } from "@/types/team";
import { isIOS } from "@/utils/platform";
import {
  getSettingsBackTarget,
  hasSettingsHomeParent,
  isSettingsPath,
  isSettingsRootPath,
  SETTINGS_SHELL_POP_EVENT,
  SETTINGS_SHELL_SWIPE_BACK_EVENT,
  shouldAnimateSettingsPop
} from "@/utils/settingsNavigation";
import { getTeamSeatMismatch } from "@/utils/teamSeats";
import { cn } from "@/utils/utils";
import packageJson from "../../../package.json";

type SettingsNavItem = {
  label: string;
  to:
    | "/settings/account"
    | "/settings/preferences"
    | "/settings/security"
    | "/settings/billing"
    | "/settings/team"
    | "/settings/api"
    | "/settings/history"
    | "/settings/about";
  icon: ComponentType<{ className?: string }>;
  badge?: string;
  badgeTone?: "warning" | "danger";
};

function SettingsNavLink({ item }: { item: SettingsNavItem }) {
  const Icon = item.icon;
  const isNavigationLocked = useSettingsNavigationLockState();
  const attentionLabel =
    item.badge === "Paused"
      ? "Team usage paused"
      : item.badge === "Setup"
        ? "Team setup required"
        : undefined;

  return (
    <Link
      to={item.to}
      title={item.label}
      aria-label={attentionLabel ? `${item.label}, ${attentionLabel}` : item.label}
      aria-disabled={isNavigationLocked || undefined}
      onClick={(event) => {
        if (isNavigationLocked) {
          event.preventDefault();
          return;
        }
      }}
      activeProps={{
        className:
          "bg-[hsl(var(--sidebar-chrome))] text-foreground shadow-sm dark:bg-[hsl(var(--sidebar-chrome-hover))]"
      }}
      inactiveProps={{
        className: "text-muted-foreground hover:bg-background/70 hover:text-foreground"
      }}
      className={cn(
        "group flex min-h-11 items-center justify-start gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors",
        isNavigationLocked && "cursor-not-allowed opacity-50"
      )}
    >
      <span className="shrink-0">
        <Icon className="h-4 w-4" />
      </span>
      <span className="min-w-0 flex-1 truncate">{item.label}</span>
      {item.badge && (
        <Badge
          variant="secondary"
          className={cn(
            "h-5 px-1.5 text-[10px]",
            item.badgeTone === "warning" &&
              "bg-maple-warning text-maple-onWarning hover:bg-maple-warning",
            item.badgeTone === "danger" &&
              "bg-destructive text-destructive-onFilled hover:bg-destructive"
          )}
        >
          {item.badge}
        </Badge>
      )}
    </Link>
  );
}

function SettingsLayoutContent() {
  const os = useOpenSecret();
  const router = useRouter();
  const location = useLocation();
  const queryClient = useQueryClient();
  const { returnToHome } = usePersistentHomeNavigation();
  const { billingStatus, setBillingStatus } = useLocalState();
  const isNavigationLocked = useSettingsNavigationLockState();
  const isCompactViewport = useCompactSettingsLayout();
  const isSettingsRoot = isSettingsRootPath(location.pathname);
  const isAuthReady = !os.auth.loading && !!os.auth.user;
  const menuRef = useRef<HTMLElement>(null);
  const menuBackButtonRef = useRef<HTMLButtonElement>(null);
  const detailBackButtonRef = useRef<HTMLButtonElement>(null);
  const mainRef = useRef<HTMLElement>(null);
  const rootHistoryIndexRef = useRef<number | null>(null);
  const popAnimationRef = useRef<Promise<void> | null>(null);
  const popTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const popResolveRef = useRef<(() => void) | null>(null);
  const settingsShellPopRef = useRef<Promise<void> | null>(null);
  const settingsShellPopTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const settingsShellPopResolveRef = useRef<(() => void) | null>(null);
  const skipNextDetailPopAnimationRef = useRef(false);
  const skipNextSettingsShellPopAnimationRef = useRef(false);
  const [isPopping, setIsPopping] = useState(false);
  const [isClosingSettings, setIsClosingSettings] = useState(false);
  const [isSigningOut, setIsSigningOut] = useState(false);
  const [signOutError, setSignOutError] = useState<string | null>(null);

  useSettingsNavigationLock(isSigningOut);

  useEffect(() => {
    if (!os.auth.loading && !os.auth.user) {
      void router.navigate({
        to: "/login",
        search: { next: location.href },
        replace: true
      });
    }
  }, [location.href, os.auth.loading, os.auth.user, router]);

  useLayoutEffect(() => {
    if (!isCompactViewport) {
      rootHistoryIndexRef.current = null;
      setIsPopping(false);
      return;
    }

    if (isSettingsRoot) {
      const historyIndex = window.history.state?.__TSR_index;
      rootHistoryIndexRef.current =
        typeof historyIndex === "number" && Number.isFinite(historyIndex) ? historyIndex : null;
      setIsPopping(false);
    }
  }, [isCompactViewport, isSettingsRoot]);

  useEffect(() => {
    if (!isAuthReady) return;

    const menu = menuRef.current;
    const main = mainRef.current;

    if (menu) {
      menu.inert = isCompactViewport && !isSettingsRoot;
    }
    if (main) {
      main.inert = isCompactViewport && (isSettingsRoot || isPopping);
    }

    return () => {
      if (menu) menu.inert = false;
      if (main) main.inert = false;
    };
  }, [isAuthReady, isCompactViewport, isPopping, isSettingsRoot]);

  useEffect(() => {
    if (!isAuthReady || !isCompactViewport) return;

    const frame = window.requestAnimationFrame(() => {
      if (isSettingsRoot) {
        menuBackButtonRef.current?.focus({ preventScroll: true });
      } else if (!isPopping) {
        detailBackButtonRef.current?.focus({ preventScroll: true });
      }
    });
    return () => window.cancelAnimationFrame(frame);
  }, [isAuthReady, isCompactViewport, isPopping, isSettingsRoot]);

  const runPopAnimation = useCallback(() => {
    if (popAnimationRef.current) return popAnimationRef.current;

    setIsPopping(true);
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const animation = new Promise<void>((resolve) => {
      popResolveRef.current = resolve;
      popTimerRef.current = setTimeout(resolve, reducedMotion ? 0 : 320);
    }).finally(() => {
      popAnimationRef.current = null;
      popTimerRef.current = null;
      popResolveRef.current = null;
    });
    popAnimationRef.current = animation;
    return animation;
  }, []);

  const runSettingsShellPop = useCallback(() => {
    if (settingsShellPopRef.current) return settingsShellPopRef.current;

    setIsClosingSettings(true);
    window.dispatchEvent(new Event(SETTINGS_SHELL_POP_EVENT));
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const animation = new Promise<void>((resolve) => {
      settingsShellPopResolveRef.current = resolve;
      settingsShellPopTimerRef.current = setTimeout(resolve, reducedMotion ? 0 : 320);
    }).finally(() => {
      settingsShellPopRef.current = null;
      settingsShellPopTimerRef.current = null;
      settingsShellPopResolveRef.current = null;
    });
    settingsShellPopRef.current = animation;
    return animation;
  }, []);

  const getSettingsDetailSwipeContext = useCallback(() => {
    if (
      !isCompactViewport ||
      isSettingsRoot ||
      isNavigationLocked ||
      isPopping ||
      isClosingSettings
    ) {
      return null;
    }

    return location.pathname;
  }, [
    isClosingSettings,
    isCompactViewport,
    isNavigationLocked,
    isPopping,
    isSettingsRoot,
    location.pathname
  ]);

  const commitSettingsDetailSwipe = useCallback(
    (_pathname: string, resetSwipe: () => void) => {
      const target = getSettingsBackTarget(
        window.history.state?.__TSR_index,
        rootHistoryIndexRef.current
      );
      if (target.type === "history") {
        skipNextDetailPopAnimationRef.current = true;
        router.history.go(target.delta);
        return;
      }

      void router
        .navigate({ to: "/settings", replace: true, ignoreBlocker: true })
        .finally(resetSwipe);
    },
    [router]
  );

  const {
    active: isSettingsDetailSwipeActive,
    currentStyle: settingsDetailSwipeStyle,
    parentStyle: settingsMenuSwipeStyle,
    platformEnabled: isIOSSwipeBackEnabled,
    pointerHandlers: settingsSwipePointerHandlers,
    reset: resetSettingsDetailSwipe
  } = useIOSSwipeBack({
    enabled: isCompactViewport && !isSettingsRoot,
    getContext: getSettingsDetailSwipeContext,
    onComplete: commitSettingsDetailSwipe
  });

  useLayoutEffect(() => {
    if (isSettingsRoot) resetSettingsDetailSwipe();
  }, [isSettingsRoot, resetSettingsDetailSwipe]);

  const closeSettings = useCallback(
    async (interactive = false) => {
      if (isNavigationLocked || isSigningOut || isClosingSettings) return;

      if (isCompactViewport) {
        if (hasSettingsHomeParent(window.history.state)) {
          if (interactive) skipNextSettingsShellPopAnimationRef.current = true;
          router.history.back();
          return;
        }

        if (!interactive) await runSettingsShellPop();
      }

      returnToHome();
    },
    [
      isClosingSettings,
      isCompactViewport,
      isNavigationLocked,
      isSigningOut,
      returnToHome,
      router.history,
      runSettingsShellPop
    ]
  );

  useEffect(() => {
    const handleInteractiveSettingsClose = (event: Event) => {
      if (isNavigationLocked || isSigningOut || isClosingSettings) {
        event.preventDefault();
        return;
      }

      void closeSettings(true);
    };

    window.addEventListener(SETTINGS_SHELL_SWIPE_BACK_EVENT, handleInteractiveSettingsClose);
    return () =>
      window.removeEventListener(SETTINGS_SHELL_SWIPE_BACK_EVENT, handleInteractiveSettingsClose);
  }, [closeSettings, isClosingSettings, isNavigationLocked, isSigningOut]);

  useEffect(() => {
    return () => {
      if (popTimerRef.current) clearTimeout(popTimerRef.current);
      popResolveRef.current?.();
      if (settingsShellPopTimerRef.current) clearTimeout(settingsShellPopTimerRef.current);
      settingsShellPopResolveRef.current?.();
    };
  }, []);

  useEffect(() => {
    if (!isCompactViewport) return;

    return router.history.block({
      enableBeforeUnload: false,
      blockerFn: async ({ currentLocation, nextLocation, action }) => {
        const shouldPopDetail = shouldAnimateSettingsPop({
          compact: true,
          currentPathname: currentLocation.pathname,
          nextPathname: nextLocation.pathname,
          action
        });

        if (skipNextDetailPopAnimationRef.current && shouldPopDetail) {
          skipNextDetailPopAnimationRef.current = false;
          if (isNavigationLocked) {
            resetSettingsDetailSwipe();
            return true;
          }
          return false;
        }

        if (isNavigationLocked) return true;

        const isBackwardAction = action === "BACK" || action === "GO";
        if (
          isSettingsRootPath(currentLocation.pathname) &&
          !isSettingsPath(nextLocation.pathname) &&
          isBackwardAction
        ) {
          if (skipNextSettingsShellPopAnimationRef.current) {
            skipNextSettingsShellPopAnimationRef.current = false;
            return false;
          }
          await runSettingsShellPop();
          return false;
        }

        if (!shouldPopDetail) return false;

        await runPopAnimation();
        return false;
      }
    });
  }, [
    isCompactViewport,
    isNavigationLocked,
    resetSettingsDetailSwipe,
    router.history,
    runPopAnimation,
    runSettingsShellPop
  ]);

  const showSettingsMenu = useCallback(async () => {
    if (isNavigationLocked || isPopping || isSettingsDetailSwipeActive) return;

    const target = getSettingsBackTarget(
      window.history.state?.__TSR_index,
      rootHistoryIndexRef.current
    );
    if (target.type === "history") {
      router.history.go(target.delta);
      return;
    }

    await runPopAnimation();
    await router.navigate({ to: "/settings", replace: true, ignoreBlocker: true });
  }, [isNavigationLocked, isPopping, isSettingsDetailSwipeActive, router, runPopAnimation]);

  useEffect(() => {
    if (!isCompactViewport || isSettingsRoot) return;

    const dismissOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      void showSettingsMenu();
    };

    window.addEventListener("keydown", dismissOnEscape);
    return () => window.removeEventListener("keydown", dismissOnEscape);
  }, [isCompactViewport, isSettingsRoot, showSettingsMenu]);

  const { data: currentBillingStatus } = useQuery({
    queryKey: ["billingStatus"],
    queryFn: async () => {
      const status = await getBillingService().getBillingStatus();
      setBillingStatus(status);
      return status;
    },
    enabled: !!os.auth.user
  });

  const resolvedBillingStatus = currentBillingStatus ?? billingStatus;
  const productName = resolvedBillingStatus?.product_name?.toLowerCase() ?? "";
  const isTeamPlan = productName.includes("team");

  const { data: teamStatus } = useQuery<TeamStatus>({
    queryKey: ["teamStatus"],
    queryFn: () => getBillingService().getTeamStatus(),
    enabled: !!os.auth.user && isTeamPlan
  });

  const isIOSPlatform = isIOS();
  const { data: products, isError: productsError } = useQuery({
    queryKey: ["products-version-check", isIOSPlatform],
    queryFn: () => getBillingService().getProducts(`v${packageJson.version}`),
    enabled: isIOSPlatform && !!os.auth.user
  });

  if (os.auth.loading || !os.auth.user) {
    return null;
  }

  const teamSeatMismatch = getTeamSeatMismatch(teamStatus);
  const needsTeamSetup = !!teamStatus?.has_team_subscription && teamStatus.team_created === false;
  const showApiManagement =
    !isIOSPlatform ||
    productsError ||
    !!products?.some((product) => product.is_available !== false);

  const sections: Array<{ label: string; items: SettingsNavItem[] }> = [
    {
      label: "Personal",
      items: [
        { label: "Account", to: "/settings/account", icon: UserRound },
        { label: "Preferences", to: "/settings/preferences", icon: MessageSquareText },
        { label: "Security", to: "/settings/security", icon: ShieldCheck }
      ]
    },
    {
      label: "Plan",
      items: [
        { label: "Billing", to: "/settings/billing", icon: CreditCard },
        ...(isTeamPlan
          ? [
              {
                label: "Team",
                to: "/settings/team" as const,
                icon: UsersRound,
                badge: teamSeatMismatch ? "Paused" : needsTeamSetup ? "Setup" : undefined,
                badgeTone: teamSeatMismatch ? ("danger" as const) : ("warning" as const)
              }
            ]
          : [])
      ]
    },
    {
      label: "Developer",
      items: showApiManagement
        ? [{ label: "API & credits", to: "/settings/api", icon: KeyRound }]
        : []
    },
    {
      label: "Data",
      items: [{ label: "Chat and task history", to: "/settings/history", icon: Database }]
    },
    {
      label: "Maple",
      items: [{ label: "About", to: "/settings/about", icon: BookOpen }]
    }
  ];

  const signOut = async () => {
    if (isNavigationLocked || isSigningOut) return;

    setSignOutError(null);
    setIsSigningOut(true);
    let operationBlock: Awaited<ReturnType<typeof stopAgentRuntimeForUser>> | null = null;
    let signedOut = false;

    // Never sign out while this account may still have Agent tools executing.
    try {
      operationBlock = await stopAgentRuntimeForUser(os.auth.user?.user.id);
    } catch (error) {
      console.error("Error stopping Agent Mode:", error);
      setSignOutError("Maple could not stop Agent Mode. Please try logging out again.");
      setIsSigningOut(false);
      return;
    }

    try {
      // Credential reset is required before logout so the next account cannot
      // inherit this user's local proxy key.
      const { proxyService } = await import("@/services/proxyService");
      await proxyService.stopAndResetProxy(os.auth.user?.user.id, os.deleteApiKey);

      try {
        getBillingService().clearToken();
      } catch (error) {
        console.error("Error clearing billing token:", error);
        sessionStorage.removeItem("maple_billing_token");
      }

      await os.signOut();
      signedOut = true;
      queryClient.clear();
      await router.invalidate();
      await router.navigate({ to: "/" });
    } catch (error) {
      console.error("Error during sign out:", error);
      if (signedOut) {
        window.location.href = "/";
        return;
      }
      setSignOutError(
        "Maple could not securely reset Agent Mode or finish logging out. Please try again."
      );
    } finally {
      if (!signedOut) {
        operationBlock.release();
        setIsSigningOut(false);
      } else {
        operationBlock.retainUntilNextSession();
      }
    }
  };

  return (
    <div
      className={cn(
        "relative grid h-dvh min-h-0 w-full grid-cols-1 overflow-hidden bg-background sm:grid-cols-[16rem_minmax(0,1fr)]",
        isIOSSwipeBackEnabled && isCompactViewport && "touch-pan-y"
      )}
      data-swipe-back-ignore={
        isNavigationLocked || isSigningOut || isClosingSettings ? "" : undefined
      }
      style={isCompactViewport ? { gridTemplateColumns: "minmax(0, 1fr)" } : undefined}
      {...settingsSwipePointerHandlers}
    >
      <aside
        ref={menuRef}
        aria-label="Settings navigation"
        aria-hidden={isCompactViewport && !isSettingsRoot}
        className={cn(
          "flex min-h-0 flex-col overflow-hidden border-r border-border/40 bg-muted dark:bg-[hsl(var(--sidebar))]",
          isCompactViewport
            ? [
                "maple-navigation-page fixed inset-0 z-10 w-full border-r-0",
                !isSettingsRoot && !isPopping && "maple-navigation-page-covered",
                isSettingsDetailSwipeActive && "maple-navigation-page-interactive"
              ]
            : "z-auto transition-none"
        )}
        style={isSettingsDetailSwipeActive ? settingsMenuSwipeStyle : undefined}
      >
        <div className="flex h-16 shrink-0 items-center border-b border-border/30 px-3 sm:px-4">
          <button
            ref={menuBackButtonRef}
            type="button"
            onClick={() => void closeSettings()}
            disabled={isNavigationLocked || isClosingSettings}
            className="flex h-10 min-w-0 flex-1 items-center justify-start gap-2 rounded-lg px-2 text-foreground transition-colors hover:bg-background/70 disabled:cursor-not-allowed disabled:opacity-50"
            aria-label="Back to chats"
          >
            <ArrowLeft className="h-4 w-4 shrink-0" />
            <MapleWordmark className="h-4 min-w-0 w-auto" aria-hidden />
          </button>
        </div>

        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto px-3 py-3">
          <div className="mb-3 flex items-center gap-2 px-3">
            <Settings className="h-4 w-4 text-muted-foreground" />
            <p className="text-sm font-semibold">Settings</p>
          </div>

          <nav className="space-y-4" aria-label="Settings categories">
            {sections
              .filter((section) => section.items.length > 0)
              .map((section) => (
                <div key={section.label}>
                  <p className="mb-1 px-3 text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground/80">
                    {section.label}
                  </p>
                  <div className="space-y-0.5">
                    {section.items.map((item) => (
                      <SettingsNavLink key={item.to} item={item} />
                    ))}
                  </div>
                </div>
              ))}
          </nav>
        </div>

        <div className="shrink-0 border-t border-border/30 p-3">
          <div className="mb-2 min-w-0 px-3">
            <p className="truncate text-xs font-medium">
              {os.auth.user.user.email || "Maple user"}
            </p>
            <p className="truncate text-[11px] text-muted-foreground">
              {resolvedBillingStatus?.product_name
                ? `${resolvedBillingStatus.product_name} Plan`
                : "Loading plan..."}
            </p>
          </div>
          {signOutError && (
            <p role="alert" className="mb-2 px-3 text-xs leading-relaxed text-destructive">
              {signOutError}
            </p>
          )}
          <Button
            type="button"
            variant="ghost"
            onClick={signOut}
            disabled={isNavigationLocked || isSigningOut}
            title="Log out"
            className="h-10 w-full justify-start px-3 text-muted-foreground hover:text-foreground"
          >
            <LogOut className="mr-3 h-4 w-4 shrink-0" />
            <span>{isSigningOut ? "Logging out..." : "Log out"}</span>
          </Button>
        </div>
      </aside>

      <main
        ref={mainRef}
        aria-hidden={isCompactViewport && (isSettingsRoot || isPopping)}
        className={cn(
          "min-h-0 min-w-0 overflow-y-auto overscroll-y-contain bg-background",
          isCompactViewport && [
            "maple-navigation-page fixed inset-0 z-20 shadow-[-12px_0_28px_rgba(0,0,0,0.12)]",
            isSettingsDetailSwipeActive && "maple-navigation-page-interactive",
            isSettingsRoot
              ? "maple-navigation-page-pop"
              : isPopping
                ? "maple-navigation-page-pop"
                : "maple-navigation-page-enter"
          ]
        )}
        style={isSettingsDetailSwipeActive ? settingsDetailSwipeStyle : undefined}
      >
        {isCompactViewport && (
          <div className="sticky top-0 z-30 flex h-14 items-center gap-3 border-b border-border/50 bg-background/95 px-3 backdrop-blur supports-[backdrop-filter]:bg-background/80">
            <button
              ref={detailBackButtonRef}
              type="button"
              onClick={() => void showSettingsMenu()}
              disabled={isNavigationLocked || isPopping || isSettingsDetailSwipeActive}
              className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg text-foreground transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              aria-label="Back to settings"
            >
              <ArrowLeft className="h-5 w-5" />
            </button>
            <p className="text-sm font-semibold">Settings</p>
          </div>
        )}
        <Outlet />
      </main>
    </div>
  );
}

export function SettingsLayout() {
  return (
    <SettingsNavigationLockProvider>
      <SettingsLayoutContent />
    </SettingsNavigationLockProvider>
  );
}
