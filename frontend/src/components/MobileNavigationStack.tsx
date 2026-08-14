import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
  type RefObject
} from "react";
import { flushSync } from "react-dom";
import { useRouter } from "@tanstack/react-router";
import { MainMenu } from "@/components/Sidebar";
import { ProjectDetailView } from "@/components/ProjectDetailView";
import { UnifiedChat } from "@/components/UnifiedChat";
import { useIOSSwipeBack } from "@/components/useIOSSwipeBack";
import { useSelectedProjectState } from "@/state/useLocalState";
import { cn } from "@/utils/utils";
import { isTauriMobile } from "@/utils/platform";
import {
  activeMobilePage,
  createInitialMobileNavigation,
  createMobileHistoryState,
  createNativeMobileLaunchGate,
  mobileMenuHistoryDelta,
  mobilePageHref,
  mobilePageUsesMenuButton,
  pageFromHref,
  promoteNewChatToConversation,
  pushMobilePage,
  readMobileHistoryState,
  resolveMissingMobileConversation,
  type MobileNavigationPage,
  type MobileNavigationSnapshot
} from "@/utils/mobileNavigation";

const PAGE_TRANSITION_MS = 320;
const nativeMobileLaunchGate = createNativeMobileLaunchGate();

function currentHomeHref() {
  return `${window.location.pathname}${window.location.search}${window.location.hash}`;
}

function maxInstanceId(snapshot: MobileNavigationSnapshot) {
  return Math.max(...snapshot.stack.map((page) => page.instanceId));
}

function lastProjectPage(snapshot: MobileNavigationSnapshot) {
  for (let index = snapshot.stack.length - 1; index >= 0; index -= 1) {
    const page = snapshot.stack[index];
    if (page.type === "project") return page;
  }
  return null;
}

function parentSnapshotForSwipe(snapshot: MobileNavigationSnapshot) {
  if (snapshot.stack.length <= 1) return null;

  return {
    ...snapshot,
    stack: snapshot.stack.slice(0, -1),
    historyIndex: Math.max(0, snapshot.historyIndex - 1)
  };
}

function mobilePageMatchesHomeHref(page: MobileNavigationPage, href: string) {
  const hrefPage = pageFromHref(href, -1);
  if (hrefPage.type === "menu") {
    return page.type === "menu" || page.type === "new-chat";
  }
  if (hrefPage.type === "chat") {
    return page.type === "chat" && page.conversationId === hrefPage.conversationId;
  }
  return page.type === "project" && page.projectId === hrefPage.projectId;
}

function NavigationLayer({
  active,
  children,
  className,
  layerRef,
  style
}: {
  active: boolean;
  children: ReactNode;
  className?: string;
  layerRef?: RefObject<HTMLDivElement>;
  style?: CSSProperties;
}) {
  const fallbackRef = useRef<HTMLDivElement>(null);
  const ref = layerRef ?? fallbackRef;

  useEffect(() => {
    const element = ref.current;
    if (!element) return;

    if (active) {
      element.removeAttribute("inert");
      requestAnimationFrame(() => element.focus({ preventScroll: true }));
    } else {
      element.setAttribute("inert", "");
    }
  }, [active, ref]);

  return (
    <div
      ref={ref}
      tabIndex={-1}
      aria-hidden={active ? undefined : true}
      style={style}
      className={cn(
        "absolute inset-0 min-h-0 min-w-0 overflow-hidden bg-background outline-none",
        active ? "pointer-events-auto" : "pointer-events-none",
        className
      )}
    >
      {children}
    </div>
  );
}

export function MobileNavigationStack({
  homeLocationHref = null,
  suspendChats = false
}: {
  homeLocationHref?: string | null;
  suspendChats?: boolean;
}) {
  const router = useRouter();
  const { setSelectedProjectId } = useSelectedProjectState();
  const nativeFreshLaunchRef = useRef(nativeMobileLaunchGate.peek(isTauriMobile()));
  const [snapshot, setSnapshot] = useState<MobileNavigationSnapshot>(() =>
    createInitialMobileNavigation(currentHomeHref(), {
      nativeFreshLaunch: nativeFreshLaunchRef.current
    })
  );
  const snapshotRef = useRef(snapshot);
  const nextInstanceIdRef = useRef(maxInstanceId(snapshot) + 1);
  const [incomingSnapshot, setIncomingSnapshot] = useState<MobileNavigationSnapshot | null>(null);
  const [outgoingChatPage, setOutgoingChatPage] = useState<Extract<
    MobileNavigationPage,
    { type: "chat" | "new-chat" }
  > | null>(null);
  const [isExiting, setIsExiting] = useState(false);
  const [enteringInstanceId, setEnteringInstanceId] = useState<number | null>(
    nativeFreshLaunchRef.current || activeMobilePage(snapshot).type === "menu"
      ? null
      : activeMobilePage(snapshot).instanceId
  );
  const transitionTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const forwardTransitionTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const enteringTransitionTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const previousHomeLocationHrefRef = useRef(homeLocationHref);
  const skipNextBackwardAnimationRef = useRef(false);
  const pendingMenuNavigationRef = useRef<"animated" | "interactive" | null>(null);
  const backwardTransitionActiveRef = useRef(false);

  const updateSnapshot = useCallback((next: MobileNavigationSnapshot) => {
    snapshotRef.current = next;
    nextInstanceIdRef.current = Math.max(nextInstanceIdRef.current, maxInstanceId(next) + 1);
    setSnapshot(next);
  }, []);

  const syncSelectedProjectFromPage = useCallback(
    (page: MobileNavigationPage) => {
      if (page.type === "project") {
        setSelectedProjectId(page.projectId);
      } else if (page.type === "new-chat") {
        setSelectedProjectId(page.projectId);
      }
    },
    [setSelectedProjectId]
  );

  const getSwipeParentSnapshot = useCallback(() => {
    const activePage = activeMobilePage(snapshotRef.current);
    if (activePage.type === "menu") return null;
    if (mobilePageUsesMenuButton(activePage)) return createInitialMobileNavigation("/");
    return parentSnapshotForSwipe(snapshotRef.current);
  }, []);

  const commitSwipeBack = useCallback(
    (_parentSnapshot: MobileNavigationSnapshot, resetSwipe: () => void) => {
      const current = snapshotRef.current;
      const activePage = activeMobilePage(current);
      const opensMenu = mobilePageUsesMenuButton(activePage);
      const historyDelta = opensMenu ? mobileMenuHistoryDelta(current) : null;

      if (historyDelta !== null) {
        pendingMenuNavigationRef.current = "interactive";
        window.history.go(historyDelta);
        return;
      }

      if (!opensMenu && current.hasInAppParent) {
        skipNextBackwardAnimationRef.current = true;
        window.history.back();
        return;
      }

      const menu = createInitialMobileNavigation("/");
      window.history.replaceState(createMobileHistoryState(menu, window.history.state), "", "/");
      window.dispatchEvent(new CustomEvent("newchat", { detail: { projectId: null } }));
      flushSync(() => {
        setSelectedProjectId(null);
        updateSnapshot(menu);
        setIncomingSnapshot(null);
        setIsExiting(false);
        setEnteringInstanceId(null);
      });
      resetSwipe();
    },
    [setSelectedProjectId, updateSnapshot]
  );

  const {
    active: isSwipeBackActive,
    currentStyle: swipeCurrentStyle,
    parentStyle: swipeParentStyle,
    platformEnabled: isIOSSwipeBackEnabled,
    pointerHandlers: swipeBackPointerHandlers,
    reset: resetSwipeBack,
    visual: swipeVisual
  } = useIOSSwipeBack({
    blocked: isExiting || enteringInstanceId !== null,
    getContext: getSwipeParentSnapshot,
    onComplete: commitSwipeBack
  });

  useLayoutEffect(() => {
    if (nativeFreshLaunchRef.current) nativeMobileLaunchGate.commit();

    const href = nativeFreshLaunchRef.current ? "/" : currentHomeHref();
    window.history.replaceState(
      createMobileHistoryState(snapshotRef.current, window.history.state),
      "",
      href
    );
  }, []);

  useEffect(() => {
    if (!nativeFreshLaunchRef.current) return;

    // Let the persistent-home provider record the normalized native launch URL after its event
    // listeners have mounted, so a settings round-trip cannot restore the stale pre-launch URL.
    const timeout = setTimeout(() => {
      window.dispatchEvent(new CustomEvent("newchat", { detail: { projectId: null } }));
    }, 0);

    return () => clearTimeout(timeout);
  }, []);

  useEffect(() => {
    if (enteringTransitionTimerRef.current) {
      clearTimeout(enteringTransitionTimerRef.current);
      enteringTransitionTimerRef.current = null;
    }
    if (enteringInstanceId === null) return;

    const instanceId = enteringInstanceId;
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    enteringTransitionTimerRef.current = setTimeout(
      () => {
        enteringTransitionTimerRef.current = null;
        setEnteringInstanceId((current) => (current === instanceId ? null : current));
      },
      reducedMotion ? 0 : PAGE_TRANSITION_MS
    );

    return () => {
      if (enteringTransitionTimerRef.current) {
        clearTimeout(enteringTransitionTimerRef.current);
        enteringTransitionTimerRef.current = null;
      }
    };
  }, [enteringInstanceId]);

  useEffect(() => {
    const previousHomeLocationHref = previousHomeLocationHrefRef.current;
    previousHomeLocationHrefRef.current = homeLocationHref;
    if (previousHomeLocationHref !== null || homeLocationHref === null) return;

    const current = snapshotRef.current;
    if (mobilePageMatchesHomeHref(activeMobilePage(current), homeLocationHref)) return;

    const restoredFromHistory = readMobileHistoryState(window.history.state);
    const next =
      restoredFromHistory &&
      mobilePageMatchesHomeHref(activeMobilePage(restoredFromHistory), homeLocationHref)
        ? restoredFromHistory
        : createInitialMobileNavigation(homeLocationHref);
    const activePage = activeMobilePage(next);

    if (transitionTimerRef.current) {
      clearTimeout(transitionTimerRef.current);
      transitionTimerRef.current = null;
    }
    if (forwardTransitionTimerRef.current) {
      clearTimeout(forwardTransitionTimerRef.current);
      forwardTransitionTimerRef.current = null;
    }
    backwardTransitionActiveRef.current = false;
    pendingMenuNavigationRef.current = null;
    skipNextBackwardAnimationRef.current = false;

    window.history.replaceState(
      createMobileHistoryState(next, window.history.state),
      "",
      homeLocationHref
    );
    syncSelectedProjectFromPage(activePage);
    updateSnapshot(next);
    setIncomingSnapshot(null);
    setOutgoingChatPage(null);
    setIsExiting(false);
    setEnteringInstanceId(null);
    resetSwipeBack();
  }, [homeLocationHref, resetSwipeBack, syncSelectedProjectFromPage, updateSnapshot]);

  const completeBackwardNavigation = useCallback(
    (next: MobileNavigationSnapshot) => {
      if (transitionTimerRef.current) clearTimeout(transitionTimerRef.current);
      if (forwardTransitionTimerRef.current) {
        clearTimeout(forwardTransitionTimerRef.current);
        forwardTransitionTimerRef.current = null;
      }
      backwardTransitionActiveRef.current = true;
      resetSwipeBack();

      setOutgoingChatPage(null);
      setIncomingSnapshot(next);
      setIsExiting(true);

      const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
      transitionTimerRef.current = setTimeout(
        () => {
          backwardTransitionActiveRef.current = false;
          updateSnapshot(next);
          setIncomingSnapshot(null);
          setIsExiting(false);
          setEnteringInstanceId(null);
          transitionTimerRef.current = null;
        },
        reducedMotion ? 0 : PAGE_TRANSITION_MS
      );
    },
    [resetSwipeBack, updateSnapshot]
  );

  useEffect(() => {
    const handlePopState = () => {
      if (forwardTransitionTimerRef.current) {
        clearTimeout(forwardTransitionTimerRef.current);
        forwardTransitionTimerRef.current = null;
      }
      setOutgoingChatPage(null);

      const current = snapshotRef.current;
      const pendingMenuNavigation = pendingMenuNavigationRef.current;

      if (pendingMenuNavigation !== null) {
        pendingMenuNavigationRef.current = null;
        const menu = createInitialMobileNavigation("/");
        window.history.replaceState(createMobileHistoryState(menu, window.history.state), "", "/");
        window.dispatchEvent(new CustomEvent("newchat", { detail: { projectId: null } }));
        flushSync(() => setSelectedProjectId(null));

        if (pendingMenuNavigation === "interactive") {
          backwardTransitionActiveRef.current = false;
          updateSnapshot(menu);
          setIncomingSnapshot(null);
          setIsExiting(false);
          setEnteringInstanceId(null);
          resetSwipeBack();
        } else {
          completeBackwardNavigation(menu);
        }
        return;
      }

      const restored =
        readMobileHistoryState(window.history.state) ??
        createInitialMobileNavigation(currentHomeHref());
      syncSelectedProjectFromPage(activeMobilePage(restored));

      if (restored.historyIndex < current.historyIndex) {
        if (skipNextBackwardAnimationRef.current) {
          skipNextBackwardAnimationRef.current = false;
          backwardTransitionActiveRef.current = false;
          updateSnapshot(restored);
          setIncomingSnapshot(null);
          setIsExiting(false);
          setEnteringInstanceId(null);
          resetSwipeBack();
          return;
        }

        completeBackwardNavigation(restored);
        return;
      }

      updateSnapshot(restored);
      backwardTransitionActiveRef.current = false;
      const activePage = activeMobilePage(restored);
      setEnteringInstanceId(activePage.type === "menu" ? null : activePage.instanceId);
    };

    window.addEventListener("popstate", handlePopState);
    return () => window.removeEventListener("popstate", handlePopState);
  }, [
    completeBackwardNavigation,
    resetSwipeBack,
    setSelectedProjectId,
    syncSelectedProjectFromPage,
    updateSnapshot
  ]);

  useEffect(() => {
    return () => {
      if (transitionTimerRef.current) clearTimeout(transitionTimerRef.current);
      if (forwardTransitionTimerRef.current) clearTimeout(forwardTransitionTimerRef.current);
      if (enteringTransitionTimerRef.current) clearTimeout(enteringTransitionTimerRef.current);
      backwardTransitionActiveRef.current = false;
    };
  }, []);

  const pushPage = useCallback(
    (page: Exclude<MobileNavigationPage, { type: "menu" }>) => {
      const current = snapshotRef.current;
      const currentPage = activeMobilePage(current);
      const next = pushMobilePage(current, page);

      if (forwardTransitionTimerRef.current) clearTimeout(forwardTransitionTimerRef.current);
      forwardTransitionTimerRef.current = null;
      if (currentPage.type === "chat" || currentPage.type === "new-chat") {
        setOutgoingChatPage(currentPage);
        const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
        forwardTransitionTimerRef.current = setTimeout(
          () => {
            setOutgoingChatPage(null);
            forwardTransitionTimerRef.current = null;
          },
          reducedMotion ? 0 : PAGE_TRANSITION_MS
        );
      } else {
        setOutgoingChatPage(null);
      }

      router.history.push(
        mobilePageHref(page),
        createMobileHistoryState(next, window.history.state)
      );
      updateSnapshot(next);
      setEnteringInstanceId(page.instanceId);
    },
    [router.history, updateSnapshot]
  );

  const openConversation = useCallback(
    (conversationId: string) => {
      const page: MobileNavigationPage = {
        type: "chat",
        instanceId: nextInstanceIdRef.current++,
        conversationId
      };
      pushPage(page);
      window.dispatchEvent(new CustomEvent("conversationselected", { detail: { conversationId } }));
    },
    [pushPage]
  );

  const openProject = useCallback(
    (projectId: string) => {
      const page: MobileNavigationPage = {
        type: "project",
        instanceId: nextInstanceIdRef.current++,
        projectId
      };
      pushPage(page);
      window.dispatchEvent(new Event("projectselected"));
    },
    [pushPage]
  );

  const openNewChat = useCallback(
    (projectId: string | null) => {
      flushSync(() => setSelectedProjectId(projectId));
      const page: MobileNavigationPage = {
        type: "new-chat",
        instanceId: nextInstanceIdRef.current++,
        projectId
      };
      pushPage(page);
      window.dispatchEvent(new CustomEvent("newchat", { detail: { projectId } }));
    },
    [pushPage, setSelectedProjectId]
  );

  const handleConversationCreated = useCallback(
    (newChatInstanceId: number, conversationId: string) => {
      if (pendingMenuNavigationRef.current !== null || backwardTransitionActiveRef.current) {
        return false;
      }

      const next = promoteNewChatToConversation(
        snapshotRef.current,
        newChatInstanceId,
        conversationId
      );
      if (next === snapshotRef.current) return false;

      snapshotRef.current = next;
      setSnapshot(next);
      window.history.replaceState(
        createMobileHistoryState(next, window.history.state),
        "",
        mobilePageHref(activeMobilePage(next))
      );
      return true;
    },
    []
  );

  const handleConversationNotFound = useCallback(
    (instanceId: number) => {
      const resolution = resolveMissingMobileConversation(snapshotRef.current, instanceId);
      if (!resolution) return;

      const targetPage = activeMobilePage(resolution.targetSnapshot);
      window.history.replaceState(
        createMobileHistoryState(resolution.sanitizedSnapshot, window.history.state),
        "",
        mobilePageHref(targetPage)
      );

      if (targetPage.type === "project") {
        setSelectedProjectId(targetPage.projectId);
        window.dispatchEvent(new Event("projectselected"));
      } else if (targetPage.type === "new-chat") {
        setSelectedProjectId(targetPage.projectId);
        window.dispatchEvent(
          new CustomEvent("newchat", { detail: { projectId: targetPage.projectId } })
        );
      } else if (targetPage.type === "menu") {
        setSelectedProjectId(null);
        window.dispatchEvent(new CustomEvent("newchat", { detail: { projectId: null } }));
      } else {
        window.dispatchEvent(
          new CustomEvent("conversationselected", {
            detail: { conversationId: targetPage.conversationId }
          })
        );
      }

      if (resolution.historyDelta !== null) {
        if (targetPage.type === "menu") {
          pendingMenuNavigationRef.current = "animated";
        }
        window.history.go(resolution.historyDelta);
        return;
      }

      completeBackwardNavigation(resolution.targetSnapshot);
    },
    [completeBackwardNavigation, setSelectedProjectId]
  );

  const goBack = useCallback(() => {
    if (isExiting || isSwipeBackActive) return;

    const current = snapshotRef.current;
    if (current.hasInAppParent) {
      window.history.back();
      return;
    }

    const menu = createInitialMobileNavigation("/");
    flushSync(() => setSelectedProjectId(null));
    window.history.replaceState(createMobileHistoryState(menu, window.history.state), "", "/");
    window.dispatchEvent(new CustomEvent("newchat", { detail: { projectId: null } }));
    completeBackwardNavigation(menu);
  }, [completeBackwardNavigation, isExiting, isSwipeBackActive, setSelectedProjectId]);

  const showMenu = useCallback(() => {
    if (isExiting || isSwipeBackActive || pendingMenuNavigationRef.current !== null) return;

    const current = snapshotRef.current;
    const historyDelta = mobileMenuHistoryDelta(current);
    if (historyDelta !== null) {
      pendingMenuNavigationRef.current = "animated";
      window.history.go(historyDelta);
      return;
    }

    const menu = createInitialMobileNavigation("/");
    flushSync(() => setSelectedProjectId(null));
    window.history.replaceState(createMobileHistoryState(menu, window.history.state), "", "/");
    window.dispatchEvent(new CustomEvent("newchat", { detail: { projectId: null } }));
    completeBackwardNavigation(menu);
  }, [completeBackwardNavigation, isExiting, isSwipeBackActive, setSelectedProjectId]);

  const baseActivePage = activeMobilePage(snapshot);
  const baseProjectPage = lastProjectPage(snapshot);
  const incomingActivePage = incomingSnapshot ? activeMobilePage(incomingSnapshot) : null;
  const isTransitioningBackward = isExiting && incomingSnapshot !== null;
  const targetActivePage = incomingActivePage ?? baseActivePage;
  const isMenuCovered = targetActivePage.type !== "menu";
  const swipeParentPage = swipeVisual ? activeMobilePage(swipeVisual.context) : null;
  const isRevealingMenuDirectly =
    (swipeParentPage?.type === "menu" && mobilePageUsesMenuButton(baseActivePage)) ||
    (isTransitioningBackward &&
      incomingActivePage?.type === "menu" &&
      mobilePageUsesMenuButton(baseActivePage));
  const visibleChatPages: Array<Extract<MobileNavigationPage, { type: "chat" | "new-chat" }>> = [];

  if (!suspendChats) {
    if (outgoingChatPage) {
      visibleChatPages.push(outgoingChatPage);
    }
    if (swipeParentPage?.type === "chat" || swipeParentPage?.type === "new-chat") {
      if (!visibleChatPages.some((page) => page.instanceId === swipeParentPage.instanceId)) {
        visibleChatPages.push(swipeParentPage);
      }
    }
    if (
      isTransitioningBackward &&
      incomingActivePage &&
      (incomingActivePage.type === "chat" || incomingActivePage.type === "new-chat")
    ) {
      if (!visibleChatPages.some((page) => page.instanceId === incomingActivePage.instanceId)) {
        visibleChatPages.push(incomingActivePage);
      }
    }
    if (baseActivePage.type === "chat" || baseActivePage.type === "new-chat") {
      if (!visibleChatPages.some((page) => page.instanceId === baseActivePage.instanceId)) {
        visibleChatPages.push(baseActivePage);
      }
    }
  }

  const renderChatPage = (page: Extract<MobileNavigationPage, { type: "chat" | "new-chat" }>) => {
    const usesMenuButton = mobilePageUsesMenuButton(page);

    return (
      <UnifiedChat
        standaloneMobile
        standaloneMobileConversationId={page.type === "chat" ? page.conversationId : null}
        standaloneMobileProjectId={page.type === "new-chat" ? page.projectId : undefined}
        standaloneMobileNavigationInstanceId={page.instanceId}
        mobileNavigationControl={usesMenuButton ? "menu" : "back"}
        onMobileNavigation={usesMenuButton ? showMenu : goBack}
        onMobileOpenNewChat={openNewChat}
        onMobileConversationCreated={(conversationId) =>
          handleConversationCreated(page.instanceId, conversationId)
        }
        onMobileConversationNotFound={handleConversationNotFound}
      />
    );
  };

  const renderProjectPage = (page: Extract<MobileNavigationPage, { type: "project" }>) => (
    <ProjectDetailView
      projectId={page.projectId}
      standaloneMobile
      onMobileBack={goBack}
      onMobileOpenConversation={openConversation}
      onMobileOpenNewChat={openNewChat}
      onMobileProjectDeleted={goBack}
    />
  );

  return (
    <div
      data-ios-swipe-back-surface={isIOSSwipeBackEnabled ? "" : undefined}
      className={cn(
        "relative h-dvh min-h-0 w-full overflow-hidden bg-background",
        isIOSSwipeBackEnabled && "touch-pan-y"
      )}
      {...swipeBackPointerHandlers}
    >
      <NavigationLayer
        active={!isTransitioningBackward && baseActivePage.type === "menu"}
        className={cn(
          "maple-navigation-page z-0",
          isMenuCovered && "maple-navigation-page-covered",
          swipeParentPage?.type === "menu" && "maple-navigation-page-interactive"
        )}
        style={swipeParentPage?.type === "menu" ? swipeParentStyle : undefined}
      >
        <MainMenu
          presentation="page"
          chatId={baseActivePage.type === "chat" ? baseActivePage.conversationId : undefined}
          onOpenConversation={openConversation}
          onOpenProject={openProject}
          onOpenNewChat={openNewChat}
        />
      </NavigationLayer>

      {baseProjectPage ? (
        <NavigationLayer
          key={`project-${baseProjectPage.instanceId}`}
          active={baseActivePage.type === "project"}
          className={cn(
            "maple-navigation-page z-10 shadow-[-12px_0_28px_rgba(0,0,0,0.12)]",
            isTransitioningBackward && baseActivePage.type === "project"
              ? "maple-navigation-page-pop z-20"
              : targetActivePage.type === "chat" || targetActivePage.type === "new-chat"
                ? "maple-navigation-page-covered"
                : baseActivePage.instanceId === enteringInstanceId && "maple-navigation-page-enter",
            swipeVisual &&
              (swipeParentPage?.instanceId === baseProjectPage.instanceId ||
                baseActivePage.instanceId === baseProjectPage.instanceId) &&
              "maple-navigation-page-interactive",
            swipeVisual && baseActivePage.instanceId === baseProjectPage.instanceId && "z-20",
            isRevealingMenuDirectly && "invisible"
          )}
          style={
            swipeParentPage?.instanceId === baseProjectPage.instanceId
              ? swipeParentStyle
              : swipeVisual && baseActivePage.instanceId === baseProjectPage.instanceId
                ? swipeCurrentStyle
                : undefined
          }
        >
          {renderProjectPage(baseProjectPage)}
        </NavigationLayer>
      ) : null}

      {visibleChatPages.map((page) => {
        const isLeaving = isTransitioningBackward && page.instanceId === baseActivePage.instanceId;
        const isIncoming =
          isTransitioningBackward && page.instanceId === incomingActivePage?.instanceId;
        const isSwipeParent = swipeParentPage?.instanceId === page.instanceId;
        const isSwipeCurrent = !!swipeVisual && baseActivePage.instanceId === page.instanceId;
        const isForwardOutgoing = outgoingChatPage?.instanceId === page.instanceId;

        return (
          <NavigationLayer
            key={`chat-${page.instanceId}`}
            active={!isIncoming && !isSwipeParent && !isForwardOutgoing}
            className={cn(
              "maple-navigation-page shadow-[-12px_0_28px_rgba(0,0,0,0.12)]",
              isForwardOutgoing ? "maple-navigation-page-covered z-10" : "z-20",
              isLeaving && "maple-navigation-page-pop z-20",
              isIncoming && "z-10",
              !isTransitioningBackward &&
                page.instanceId === enteringInstanceId &&
                "maple-navigation-page-enter",
              (isSwipeParent || isSwipeCurrent) && "maple-navigation-page-interactive",
              isSwipeParent && "z-10",
              isForwardOutgoing && isRevealingMenuDirectly && "invisible"
            )}
            style={
              isSwipeParent ? swipeParentStyle : isSwipeCurrent ? swipeCurrentStyle : undefined
            }
          >
            {renderChatPage(page)}
          </NavigationLayer>
        );
      })}
    </div>
  );
}
