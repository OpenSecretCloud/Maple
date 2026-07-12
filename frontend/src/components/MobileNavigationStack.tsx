import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type RefObject
} from "react";
import { flushSync } from "react-dom";
import { MainMenu } from "@/components/Sidebar";
import { ProjectDetailView } from "@/components/ProjectDetailView";
import { UnifiedChat } from "@/components/UnifiedChat";
import { useLocalState } from "@/state/useLocalState";
import { cn } from "@/utils/utils";
import { isTauriMobile } from "@/utils/platform";
import {
  activeMobilePage,
  createInitialMobileNavigation,
  createMobileHistoryState,
  mobilePageHref,
  promoteNewChatToConversation,
  pushMobilePage,
  readMobileHistoryState,
  type MobileNavigationPage,
  type MobileNavigationSnapshot
} from "@/utils/mobileNavigation";

const PAGE_TRANSITION_MS = 320;
let nativeMobileNavigationInitialized = false;

function currentHomeHref() {
  return `${window.location.pathname}${window.location.search}${window.location.hash}`;
}

function consumeNativeFreshLaunch() {
  if (!isTauriMobile() || nativeMobileNavigationInitialized) return false;
  nativeMobileNavigationInitialized = true;
  return true;
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

function NavigationLayer({
  active,
  children,
  className,
  layerRef
}: {
  active: boolean;
  children: ReactNode;
  className?: string;
  layerRef?: RefObject<HTMLDivElement>;
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

export function MobileNavigationStack() {
  const { setSelectedProjectId } = useLocalState();
  const nativeFreshLaunchRef = useRef(consumeNativeFreshLaunch());
  const [snapshot, setSnapshot] = useState<MobileNavigationSnapshot>(() =>
    createInitialMobileNavigation(currentHomeHref(), {
      nativeFreshLaunch: nativeFreshLaunchRef.current
    })
  );
  const snapshotRef = useRef(snapshot);
  const nextInstanceIdRef = useRef(maxInstanceId(snapshot) + 1);
  const [incomingSnapshot, setIncomingSnapshot] = useState<MobileNavigationSnapshot | null>(null);
  const [isExiting, setIsExiting] = useState(false);
  const [enteringInstanceId, setEnteringInstanceId] = useState<number | null>(
    activeMobilePage(snapshot).type === "menu" ? null : activeMobilePage(snapshot).instanceId
  );
  const transitionTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const updateSnapshot = useCallback((next: MobileNavigationSnapshot) => {
    snapshotRef.current = next;
    nextInstanceIdRef.current = Math.max(nextInstanceIdRef.current, maxInstanceId(next) + 1);
    setSnapshot(next);
  }, []);

  useLayoutEffect(() => {
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

  const completeBackwardNavigation = useCallback(
    (next: MobileNavigationSnapshot) => {
      if (transitionTimerRef.current) clearTimeout(transitionTimerRef.current);

      setIncomingSnapshot(next);
      setIsExiting(true);

      const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
      transitionTimerRef.current = setTimeout(
        () => {
          updateSnapshot(next);
          setIncomingSnapshot(null);
          setIsExiting(false);
          setEnteringInstanceId(null);
          transitionTimerRef.current = null;
        },
        reducedMotion ? 0 : PAGE_TRANSITION_MS
      );
    },
    [updateSnapshot]
  );

  useEffect(() => {
    const handlePopState = () => {
      const current = snapshotRef.current;
      const restored =
        readMobileHistoryState(window.history.state) ??
        createInitialMobileNavigation(currentHomeHref());

      if (restored.historyIndex < current.historyIndex) {
        completeBackwardNavigation(restored);
        return;
      }

      updateSnapshot(restored);
      const activePage = activeMobilePage(restored);
      setEnteringInstanceId(activePage.type === "menu" ? null : activePage.instanceId);
    };

    window.addEventListener("popstate", handlePopState);
    return () => window.removeEventListener("popstate", handlePopState);
  }, [completeBackwardNavigation, updateSnapshot]);

  useEffect(() => {
    return () => {
      if (transitionTimerRef.current) clearTimeout(transitionTimerRef.current);
    };
  }, []);

  const pushPage = useCallback(
    (page: Exclude<MobileNavigationPage, { type: "menu" }>) => {
      const next = pushMobilePage(snapshotRef.current, page);
      window.history.pushState(createMobileHistoryState(next), "", mobilePageHref(page));
      updateSnapshot(next);
      setEnteringInstanceId(page.instanceId);
    },
    [updateSnapshot]
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

  const handleConversationCreated = useCallback((conversationId: string) => {
    const next = promoteNewChatToConversation(snapshotRef.current, conversationId);
    if (next === snapshotRef.current) return;

    snapshotRef.current = next;
    setSnapshot(next);
    window.history.replaceState(
      createMobileHistoryState(next, window.history.state),
      "",
      mobilePageHref(activeMobilePage(next))
    );
  }, []);

  const goBack = useCallback(() => {
    if (isExiting) return;

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
  }, [completeBackwardNavigation, isExiting, setSelectedProjectId]);

  const baseActivePage = activeMobilePage(snapshot);
  const baseProjectPage = lastProjectPage(snapshot);
  const incomingActivePage = incomingSnapshot ? activeMobilePage(incomingSnapshot) : null;
  const isTransitioningBackward = isExiting && incomingSnapshot !== null;
  const targetActivePage = incomingActivePage ?? baseActivePage;
  const isMenuCovered = targetActivePage.type !== "menu";
  const visibleChatPages: Array<Extract<MobileNavigationPage, { type: "chat" | "new-chat" }>> = [];

  if (
    isTransitioningBackward &&
    incomingActivePage &&
    (incomingActivePage.type === "chat" || incomingActivePage.type === "new-chat")
  ) {
    visibleChatPages.push(incomingActivePage);
  }
  if (baseActivePage.type === "chat" || baseActivePage.type === "new-chat") {
    visibleChatPages.push(baseActivePage);
  }

  const renderChatPage = () => (
    <UnifiedChat
      standaloneMobile
      onMobileBack={goBack}
      onMobileOpenNewChat={openNewChat}
      onMobileConversationCreated={handleConversationCreated}
    />
  );

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
    <div className="relative h-dvh min-h-0 w-full overflow-hidden bg-background">
      <NavigationLayer
        active={!isTransitioningBackward && baseActivePage.type === "menu"}
        className={cn(
          "maple-navigation-page z-0",
          isMenuCovered && "maple-navigation-page-covered"
        )}
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
                : baseActivePage.instanceId === enteringInstanceId && "maple-navigation-page-enter"
          )}
        >
          {renderProjectPage(baseProjectPage)}
        </NavigationLayer>
      ) : null}

      {visibleChatPages.map((page) => {
        const isLeaving = isTransitioningBackward && page.instanceId === baseActivePage.instanceId;
        const isIncoming =
          isTransitioningBackward && page.instanceId === incomingActivePage?.instanceId;

        return (
          <NavigationLayer
            key={`chat-${page.instanceId}`}
            active={!isIncoming}
            className={cn(
              "maple-navigation-page shadow-[-12px_0_28px_rgba(0,0,0,0.12)]",
              isLeaving && "maple-navigation-page-pop z-20",
              isIncoming && "z-10",
              !isTransitioningBackward &&
                page.instanceId === enteringInstanceId &&
                "maple-navigation-page-enter"
            )}
          >
            {renderChatPage()}
          </NavigationLayer>
        );
      })}
    </div>
  );
}
