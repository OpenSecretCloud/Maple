import {
  cloneElement,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type HTMLAttributes,
  type KeyboardEvent,
  type PointerEvent,
  type ReactElement,
  type ReactNode
} from "react";

import { Sidebar } from "@/components/Sidebar";
import { SIDEBAR_GRID_COLUMNS_CLASS } from "@/constants/layout";
import {
  RESIZABLE_SIDEBAR_DEFAULT_WIDTH_PX,
  RESIZABLE_SIDEBAR_KEYBOARD_LARGE_STEP_PX,
  RESIZABLE_SIDEBAR_KEYBOARD_STEP_PX,
  RESIZABLE_SIDEBAR_MIN_WIDTH_PX,
  clampSidebarWidth,
  loadSidebarWidth,
  saveSidebarWidth,
  sidebarDragUpdate,
  sidebarMaximumWidth
} from "@/services/sidebarWidth";
import type { WorkspaceMode } from "@/services/workspaceModePreference";
import { cn } from "@/utils/utils";

const COLLAPSE_TRANSITION_MS = 150;
const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

type SidebarDragPresentation = {
  active: boolean;
  collapsed: boolean;
  panelWidthPx: number | null;
  transition: boolean;
};

type ResizableSidebarLayoutProps = Omit<HTMLAttributes<HTMLDivElement>, "children"> & {
  children: ReactNode;
  isCompactLayout: boolean;
  isOpen: boolean;
  mode: WorkspaceMode;
  onOpenChange: (isOpen: boolean) => void;
  onTransitionChange?: (isTransitioning: boolean) => void;
  sidebar: ReactElement<React.ComponentProps<typeof Sidebar>>;
  userId?: string | null;
};

type DragSession = {
  bodyCursor: string;
  bodyUserSelect: string;
  collapsed: boolean;
  hasToggledCollapse: boolean;
  panelWidthPx: number;
  pointerId: number;
  startPointerX: number;
  startWidthPx: number;
  target: HTMLDivElement;
  transitionUntil: number;
  widthPx: number;
};

type SidebarResizeHandleProps = {
  active: boolean;
  disabled: boolean;
  isOpen: boolean;
  isVisuallyCollapsed: boolean;
  layoutWidthPx: number;
  maximumWidthPx: number;
  mode: WorkspaceMode;
  onCommit: (widthPx: number) => void;
  onDragPresentationChange: (presentation: SidebarDragPresentation) => void;
  onOpenChange: (isOpen: boolean) => void;
  onWidthChange: (widthPx: number | null) => void;
  prefersReducedMotion: boolean;
  transition: boolean;
  widthPx: number;
};

function viewportMaximumWidth(): number {
  return sidebarMaximumWidth(typeof window === "undefined" ? 0 : window.innerWidth);
}

function usePrefersReducedMotion(): boolean {
  const [matches, setMatches] = useState(
    () =>
      typeof window !== "undefined" &&
      typeof window.matchMedia === "function" &&
      window.matchMedia(REDUCED_MOTION_QUERY).matches
  );

  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const mediaQuery = window.matchMedia(REDUCED_MOTION_QUERY);
    const handleChange = (event: MediaQueryListEvent) => setMatches(event.matches);
    if ("addEventListener" in mediaQuery) {
      mediaQuery.addEventListener("change", handleChange);
      return () => mediaQuery.removeEventListener("change", handleChange);
    }
    (mediaQuery as MediaQueryList).addListener(handleChange);
    return () => (mediaQuery as MediaQueryList).removeListener(handleChange);
  }, []);

  return matches;
}

export function ResizableSidebarLayout({
  children,
  className,
  isCompactLayout,
  isOpen,
  mode,
  onOpenChange,
  onTransitionChange,
  sidebar,
  style,
  userId,
  ...rootProps
}: ResizableSidebarLayoutProps) {
  const [preferredWidthPx, setPreferredWidthPx] = useState(() =>
    userId ? loadSidebarWidth(userId) : RESIZABLE_SIDEBAR_DEFAULT_WIDTH_PX
  );
  const [transientWidthPx, setTransientWidthPx] = useState<number | null>(null);
  const [maximumWidthPx, setMaximumWidthPx] = useState(viewportMaximumWidth);
  const prefersReducedMotion = usePrefersReducedMotion();
  const [dragPresentation, setDragPresentation] = useState<SidebarDragPresentation>(() => ({
    active: false,
    collapsed: !isOpen,
    panelWidthPx: null,
    transition: false
  }));
  const previousIsOpenRef = useRef(isOpen);

  useEffect(() => {
    setPreferredWidthPx(userId ? loadSidebarWidth(userId) : RESIZABLE_SIDEBAR_DEFAULT_WIDTH_PX);
    setTransientWidthPx(null);
  }, [userId]);

  useEffect(() => {
    if (typeof window === "undefined") return;
    const updateMaximumWidth = () => setMaximumWidthPx(viewportMaximumWidth());
    window.addEventListener("resize", updateMaximumWidth);
    return () => window.removeEventListener("resize", updateMaximumWidth);
  }, []);

  useLayoutEffect(() => {
    if (isCompactLayout) {
      previousIsOpenRef.current = isOpen;
      setDragPresentation({
        active: false,
        collapsed: !isOpen,
        panelWidthPx: null,
        transition: false
      });
      return;
    }
    if (previousIsOpenRef.current === isOpen) return;
    previousIsOpenRef.current = isOpen;
    setDragPresentation((current) =>
      current.active || (current.collapsed === !isOpen && current.transition)
        ? current
        : {
            active: false,
            collapsed: !isOpen,
            panelWidthPx: null,
            transition: !prefersReducedMotion
          }
    );
  }, [isCompactLayout, isOpen, prefersReducedMotion]);

  useLayoutEffect(() => {
    if (!prefersReducedMotion || !dragPresentation.transition) return;
    setDragPresentation((current) => ({
      ...current,
      panelWidthPx: null,
      transition: false
    }));
    if (!dragPresentation.active) setTransientWidthPx(null);
  }, [dragPresentation.active, dragPresentation.transition, prefersReducedMotion]);

  useLayoutEffect(() => {
    onTransitionChange?.(
      !isCompactLayout && (dragPresentation.active || dragPresentation.transition)
    );
  }, [dragPresentation.active, dragPresentation.transition, isCompactLayout, onTransitionChange]);

  useEffect(() => {
    if (!dragPresentation.transition) return;
    const timeout = window.setTimeout(() => {
      setDragPresentation((current) => ({
        ...current,
        panelWidthPx: null,
        transition: false
      }));
    }, COLLAPSE_TRANSITION_MS);
    return () => window.clearTimeout(timeout);
  }, [dragPresentation.collapsed, dragPresentation.transition]);

  const handleCommit = useCallback(
    (widthPx: number) => {
      const nextWidthPx = clampSidebarWidth(widthPx, maximumWidthPx);
      setPreferredWidthPx(nextWidthPx);
      setTransientWidthPx(null);
      if (userId) saveSidebarWidth(userId, nextWidthPx);
    },
    [maximumWidthPx, userId]
  );
  const preferredEffectiveWidthPx = clampSidebarWidth(preferredWidthPx, maximumWidthPx);
  const effectiveWidthPx = isCompactLayout
    ? RESIZABLE_SIDEBAR_DEFAULT_WIDTH_PX
    : transientWidthPx === null
      ? preferredEffectiveWidthPx
      : clampSidebarWidth(transientWidthPx, maximumWidthPx);
  const panelWidthPx =
    dragPresentation.panelWidthPx === null
      ? effectiveWidthPx
      : Math.max(effectiveWidthPx, dragPresentation.panelWidthPx);
  const retainSidebarShell = dragPresentation.active || dragPresentation.transition;
  const renderSidebarOpen = isOpen || retainSidebarShell;
  const layoutWidthPx =
    isCompactLayout || dragPresentation.collapsed || !renderSidebarOpen ? 0 : effectiveWidthPx;
  const layoutStyle = {
    "--sidebar-width": `${panelWidthPx}px`,
    "--sidebar-grid-template": `${layoutWidthPx}px minmax(0, 1fr)`
  } as CSSProperties;
  const renderedSidebar = useMemo(
    () => cloneElement(sidebar, { isOpen: renderSidebarOpen }),
    [renderSidebarOpen, sidebar]
  );

  return (
    <div
      {...rootProps}
      style={{ ...style, ...layoutStyle }}
      className={cn(
        "relative grid h-dvh min-h-0 w-full grid-cols-1 overflow-hidden bg-background",
        !isCompactLayout && SIDEBAR_GRID_COLUMNS_CLASS,
        dragPresentation.transition &&
          "md:transition-[grid-template-columns] md:duration-150 md:ease-out motion-reduce:transition-none",
        className
      )}
    >
      <div className={isCompactLayout ? "contents" : "relative min-h-0 min-w-0 overflow-hidden"}>
        {renderedSidebar}
      </div>
      <SidebarResizeHandle
        active={dragPresentation.active}
        disabled={isCompactLayout}
        isOpen={isOpen}
        isVisuallyCollapsed={dragPresentation.collapsed}
        layoutWidthPx={layoutWidthPx}
        maximumWidthPx={maximumWidthPx}
        mode={mode}
        onCommit={handleCommit}
        onDragPresentationChange={setDragPresentation}
        onOpenChange={onOpenChange}
        onWidthChange={setTransientWidthPx}
        prefersReducedMotion={prefersReducedMotion}
        transition={dragPresentation.transition}
        widthPx={effectiveWidthPx}
      />
      {children}
    </div>
  );
}

function SidebarResizeHandle({
  active,
  disabled,
  isOpen,
  isVisuallyCollapsed,
  layoutWidthPx,
  maximumWidthPx,
  mode,
  onCommit,
  onDragPresentationChange,
  onOpenChange,
  onWidthChange,
  prefersReducedMotion,
  transition,
  widthPx
}: SidebarResizeHandleProps) {
  const dragSessionRef = useRef<DragSession | null>(null);

  const clearDragSession = useCallback((): DragSession | null => {
    const session = dragSessionRef.current;
    if (!session) return null;
    dragSessionRef.current = null;
    if (session.target.hasPointerCapture(session.pointerId)) {
      session.target.releasePointerCapture(session.pointerId);
    }
    if (typeof document !== "undefined") {
      document.body.style.cursor = session.bodyCursor;
      document.body.style.userSelect = session.bodyUserSelect;
    }
    return session;
  }, []);

  const cancelDrag = useCallback(() => {
    const session = clearDragSession();
    if (!session) return;
    if (session.collapsed) onOpenChange(true);
    onWidthChange(null);
    onDragPresentationChange({
      active: false,
      collapsed: false,
      panelWidthPx: session.hasToggledCollapse
        ? Math.max(session.startWidthPx, session.widthPx)
        : null,
      transition: session.hasToggledCollapse && !prefersReducedMotion
    });
  }, [
    clearDragSession,
    onDragPresentationChange,
    onOpenChange,
    onWidthChange,
    prefersReducedMotion
  ]);

  useEffect(() => {
    if (disabled) {
      cancelDrag();
      return;
    }
    if (typeof window === "undefined") return;
    window.addEventListener("blur", cancelDrag);
    return () => window.removeEventListener("blur", cancelDrag);
  }, [cancelDrag, disabled]);

  useEffect(
    () => () => {
      const session = clearDragSession();
      if (session?.collapsed) onOpenChange(true);
    },
    [clearDragSession, onOpenChange]
  );

  if (disabled) return null;

  const updateDrag = (session: DragSession, pointerX: number) => {
    const previousWidthPx = session.widthPx;
    const update = sidebarDragUpdate(
      session.startWidthPx,
      session.startPointerX,
      pointerX,
      maximumWidthPx
    );
    session.widthPx = update.widthPx;
    const collapsedChanged = session.collapsed !== update.isCollapsed;
    if (collapsedChanged) {
      session.collapsed = update.isCollapsed;
      session.hasToggledCollapse = true;
      session.panelWidthPx = Math.max(session.panelWidthPx, previousWidthPx, update.widthPx);
      session.transitionUntil = prefersReducedMotion ? 0 : Date.now() + COLLAPSE_TRANSITION_MS;
      onOpenChange(!update.isCollapsed);
    }
    const transition = session.transitionUntil > Date.now();
    if (!transition) session.panelWidthPx = update.widthPx;
    onWidthChange(update.widthPx);
    onDragPresentationChange({
      active: true,
      collapsed: update.isCollapsed,
      panelWidthPx: transition ? Math.max(session.panelWidthPx, update.widthPx) : null,
      transition
    });
    return { ...update, transition };
  };

  const handlePointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (
      !isOpen ||
      transition ||
      dragSessionRef.current ||
      event.button !== 0 ||
      event.isPrimary === false
    )
      return;
    event.preventDefault();
    try {
      event.currentTarget.setPointerCapture(event.pointerId);
    } catch {
      return;
    }
    dragSessionRef.current = {
      bodyCursor: typeof document === "undefined" ? "" : document.body.style.cursor,
      bodyUserSelect: typeof document === "undefined" ? "" : document.body.style.userSelect,
      collapsed: false,
      hasToggledCollapse: false,
      panelWidthPx: widthPx,
      pointerId: event.pointerId,
      startPointerX: event.clientX,
      startWidthPx: widthPx,
      target: event.currentTarget,
      transitionUntil: 0,
      widthPx
    };
    onDragPresentationChange({
      active: true,
      collapsed: false,
      panelWidthPx: null,
      transition: false
    });
    if (typeof document !== "undefined") {
      document.body.style.cursor = "col-resize";
      document.body.style.userSelect = "none";
    }
  };

  const handlePointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const session = dragSessionRef.current;
    if (!session || session.pointerId !== event.pointerId) return;
    updateDrag(session, event.clientX);
  };

  const handlePointerUp = (event: PointerEvent<HTMLDivElement>) => {
    const session = dragSessionRef.current;
    if (!session || session.pointerId !== event.pointerId) return;
    const update = updateDrag(session, event.clientX);
    clearDragSession();
    if (update.isCollapsed) {
      onWidthChange(null);
      onDragPresentationChange({
        active: false,
        collapsed: true,
        panelWidthPx: update.transition ? session.panelWidthPx : null,
        transition: update.transition
      });
      return;
    }
    onCommit(update.widthPx);
    onDragPresentationChange({
      active: false,
      collapsed: false,
      panelWidthPx: update.transition ? Math.max(session.panelWidthPx, update.widthPx) : null,
      transition: update.transition
    });
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Enter") {
      event.preventDefault();
      if (dragSessionRef.current) return;
      onOpenChange(!isOpen);
      return;
    }
    if (event.key === "Escape" && dragSessionRef.current) {
      event.preventDefault();
      cancelDrag();
      return;
    }
    if (!isOpen) return;

    let nextWidthPx: number | null = null;
    const stepPx = event.shiftKey
      ? RESIZABLE_SIDEBAR_KEYBOARD_LARGE_STEP_PX
      : RESIZABLE_SIDEBAR_KEYBOARD_STEP_PX;
    if (event.key === "ArrowLeft") nextWidthPx = widthPx - stepPx;
    if (event.key === "ArrowRight") nextWidthPx = widthPx + stepPx;
    if (event.key === "Home") nextWidthPx = RESIZABLE_SIDEBAR_MIN_WIDTH_PX;
    if (event.key === "End") nextWidthPx = maximumWidthPx;
    if (nextWidthPx === null) return;

    event.preventDefault();
    onCommit(clampSidebarWidth(nextWidthPx, maximumWidthPx));
  };

  const sidebarName = mode === "agent" ? "Agent sidebar" : "Chat sidebar";

  return (
    <div
      role="separator"
      aria-label={`Resize ${sidebarName}`}
      aria-orientation="vertical"
      aria-valuemax={maximumWidthPx}
      aria-valuemin={RESIZABLE_SIDEBAR_MIN_WIDTH_PX}
      aria-valuenow={widthPx}
      aria-valuetext={
        isVisuallyCollapsed
          ? `Collapsed ${sidebarName}; drag right to reopen`
          : isOpen
            ? `${widthPx} pixels`
            : `Collapsed; last width ${widthPx} pixels`
      }
      tabIndex={0}
      className={cn(
        "absolute inset-y-0 z-30 w-2 -translate-x-1 touch-none outline-none after:absolute after:inset-y-0 after:left-1/2 after:w-px after:-translate-x-1/2 after:bg-border/40 after:transition-colors hover:after:bg-border focus-visible:after:w-0.5 focus-visible:after:bg-ring motion-reduce:after:transition-none",
        isOpen || active ? "cursor-col-resize" : "pointer-events-none",
        transition &&
          "transition-[left] duration-150 ease-out motion-reduce:transition-none after:w-0.5 after:bg-[hsl(var(--maple-primary))]"
      )}
      style={{ left: isOpen || active || transition ? layoutWidthPx : 4 }}
      onDoubleClick={() => {
        if (isOpen) onCommit(RESIZABLE_SIDEBAR_DEFAULT_WIDTH_PX);
      }}
      onKeyDown={handleKeyDown}
      onLostPointerCapture={cancelDrag}
      onPointerCancel={cancelDrag}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
    />
  );
}
