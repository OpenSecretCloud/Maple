import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type PointerEvent as ReactPointerEvent
} from "react";
import { isIOS, isTauriMobile } from "@/utils/platform";
import {
  clampSwipeBackDistance,
  getSwipeBackDirection,
  getSwipeBackSettleDuration,
  shouldCompleteSwipeBackFromLastSample,
  SWIPE_BACK_EDGE_WIDTH
} from "@/utils/swipeBack";

const NAVIGATION_EASING = "cubic-bezier(0.32, 0.72, 0, 1)";

type SwipeGesture<Context> = {
  context: Context;
  pointerId: number;
  surface: HTMLDivElement;
  startX: number;
  startY: number;
  lastX: number;
  lastTime: number;
  velocity: number;
  width: number;
  tracking: boolean;
};

export type IOSSwipeBackVisual<Context> = {
  context: Context;
};

type IOSSwipeBackInteractiveVisual<Context> = IOSSwipeBackVisual<Context> & {
  offset: number;
  surface: HTMLDivElement;
  width: number;
  transitionMs: number;
};

type IOSSwipeBackCSSProperties = {
  currentOffset: string;
  parentOffset: string;
  transition: string;
};

let nextSwipeBackPropertyId = 0;

function createVisualProperties(): IOSSwipeBackCSSProperties {
  const id = nextSwipeBackPropertyId++;
  return {
    currentOffset: `--maple-swipe-back-current-offset-${id}`,
    parentOffset: `--maple-swipe-back-parent-offset-${id}`,
    transition: `--maple-swipe-back-transition-${id}`
  };
}

function applyVisual<Context>(
  visual: IOSSwipeBackInteractiveVisual<Context>,
  properties: IOSSwipeBackCSSProperties
) {
  const progress = visual.offset / visual.width;
  const root = visual.surface.ownerDocument.documentElement;
  root.style.setProperty(properties.currentOffset, `${visual.offset}px`);
  root.style.setProperty(properties.parentOffset, `${-24 * (1 - progress)}%`);
  root.style.setProperty(
    properties.transition,
    visual.transitionMs ? `transform ${visual.transitionMs}ms ${NAVIGATION_EASING}` : "none"
  );
}

function clearVisual(surface: HTMLDivElement, properties: IOSSwipeBackCSSProperties) {
  const root = surface.ownerDocument.documentElement;
  root.style.removeProperty(properties.currentOffset);
  root.style.removeProperty(properties.parentOffset);
  root.style.removeProperty(properties.transition);
}

export function useIOSSwipeBack<Context>({
  blocked = false,
  enabled = true,
  getContext,
  onComplete
}: {
  blocked?: boolean;
  enabled?: boolean;
  getContext: () => Context | null;
  onComplete: (context: Context, reset: () => void) => void;
}) {
  const platformEnabledRef = useRef(isTauriMobile() && isIOS());
  const gestureRef = useRef<SwipeGesture<Context> | null>(null);
  const visualRef = useRef<IOSSwipeBackInteractiveVisual<Context> | null>(null);
  const visualFrameRef = useRef<number | null>(null);
  const settleTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [visualProperties] = useState(createVisualProperties);
  const [visual, setVisual] = useState<IOSSwipeBackVisual<Context> | null>(null);
  const platformEnabled = platformEnabledRef.current;

  const activateVisual = useCallback(
    (next: IOSSwipeBackInteractiveVisual<Context>) => {
      visualRef.current = next;
      applyVisual(next, visualProperties);
      setVisual({ context: next.context });
    },
    [visualProperties]
  );

  const cancelVisualFrame = useCallback(() => {
    if (visualFrameRef.current === null) return;
    window.cancelAnimationFrame(visualFrameRef.current);
    visualFrameRef.current = null;
  }, []);

  const scheduleVisual = useCallback(
    (next: IOSSwipeBackInteractiveVisual<Context>) => {
      visualRef.current = next;
      if (visualFrameRef.current !== null) return;

      visualFrameRef.current = window.requestAnimationFrame(() => {
        visualFrameRef.current = null;
        const latest = visualRef.current;
        if (latest?.transitionMs === 0) applyVisual(latest, visualProperties);
      });
    },
    [visualProperties]
  );

  const reset = useCallback(() => {
    cancelVisualFrame();
    if (settleTimerRef.current) clearTimeout(settleTimerRef.current);
    settleTimerRef.current = null;
    const gesture = gestureRef.current;
    gestureRef.current = null;
    if (gesture) {
      try {
        gesture.surface.releasePointerCapture(gesture.pointerId);
      } catch {
        // The pointer may already have ended or been released by this WebView.
      }
    }

    const current = visualRef.current;
    visualRef.current = null;
    if (current) clearVisual(current.surface, visualProperties);
    setVisual(null);
  }, [cancelVisualFrame, visualProperties]);

  useEffect(() => {
    return () => {
      if (visualFrameRef.current !== null) {
        window.cancelAnimationFrame(visualFrameRef.current);
        visualFrameRef.current = null;
      }
      if (settleTimerRef.current) clearTimeout(settleTimerRef.current);
      settleTimerRef.current = null;
      gestureRef.current = null;
      const current = visualRef.current;
      visualRef.current = null;
      if (current) clearVisual(current.surface, visualProperties);
    };
  }, [visualProperties]);

  const settle = useCallback(
    (completing: boolean) => {
      const current = visualRef.current;
      if (!current) return;
      cancelVisualFrame();

      const transitionMs = getSwipeBackSettleDuration({
        progress: current.offset / current.width,
        completing,
        reducedMotion: window.matchMedia("(prefers-reduced-motion: reduce)").matches
      });
      const next = {
        ...current,
        offset: completing ? current.width : 0,
        transitionMs
      };
      visualRef.current = next;
      applyVisual(next, visualProperties);

      if (settleTimerRef.current) clearTimeout(settleTimerRef.current);
      settleTimerRef.current = setTimeout(() => {
        settleTimerRef.current = null;
        if (completing) {
          onComplete(current.context, reset);
        } else {
          reset();
        }
      }, transitionMs);
    },
    [cancelVisualFrame, onComplete, reset, visualProperties]
  );

  const settleFromLastSample = useCallback(
    (gesture: SwipeGesture<Context>, includeVelocity = true) => {
      const current = visualRef.current;
      if (!current) return;

      const elapsedSinceMove = Math.max(performance.now() - gesture.lastTime, 1);
      settle(
        shouldCompleteSwipeBackFromLastSample({
          distance: current.offset,
          elapsedSinceMove,
          velocity: includeVelocity ? gesture.velocity : 0,
          width: current.width
        })
      );
    },
    [settle]
  );

  const onPointerDownCapture = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      if (
        !platformEnabled ||
        !enabled ||
        blocked ||
        event.pointerType !== "touch" ||
        !event.isPrimary ||
        event.clientX > SWIPE_BACK_EDGE_WIDTH ||
        gestureRef.current ||
        visualRef.current
      ) {
        return;
      }

      const target = event.target;
      if (!(target instanceof Node) || !event.currentTarget.contains(target)) return;
      if (target instanceof Element && target.closest("[data-swipe-back-ignore]")) return;

      const width = event.currentTarget.clientWidth || window.innerWidth;
      const context = getContext();
      if (width <= 0 || context === null) return;

      gestureRef.current = {
        context,
        pointerId: event.pointerId,
        surface: event.currentTarget,
        startX: event.clientX,
        startY: event.clientY,
        lastX: event.clientX,
        lastTime: performance.now(),
        velocity: 0,
        width,
        tracking: false
      };

      try {
        event.currentTarget.setPointerCapture(event.pointerId);
      } catch {
        // Pointer capture is an enhancement; WebKit still sends the active touch to this surface.
      }
    },
    [blocked, enabled, getContext, platformEnabled]
  );

  const onPointerMoveCapture = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      const gesture = gestureRef.current;
      if (!gesture || gesture.pointerId !== event.pointerId) return;

      const deltaX = event.clientX - gesture.startX;
      const deltaY = event.clientY - gesture.startY;
      if (!gesture.tracking) {
        const direction = getSwipeBackDirection(deltaX, deltaY);
        if (direction === "pending") return;
        if (direction === "reject") {
          gestureRef.current = null;
          try {
            event.currentTarget.releasePointerCapture(event.pointerId);
          } catch {
            // The pointer may not have been captured by this WebView.
          }
          return;
        }

        gesture.tracking = true;
        activateVisual({
          context: gesture.context,
          offset: clampSwipeBackDistance(deltaX, gesture.width),
          surface: gesture.surface,
          width: gesture.width,
          transitionMs: 0
        });
      }

      event.preventDefault();
      const now = performance.now();
      const elapsed = Math.max(now - gesture.lastTime, 1);
      const instantaneousVelocity = (event.clientX - gesture.lastX) / elapsed;
      gesture.velocity = gesture.velocity * 0.35 + instantaneousVelocity * 0.65;
      gesture.lastX = event.clientX;
      gesture.lastTime = now;

      const current = visualRef.current;
      if (!current) return;
      const next = {
        ...current,
        offset: clampSwipeBackDistance(deltaX, gesture.width),
        transitionMs: 0
      };
      scheduleVisual(next);
    },
    [activateVisual, scheduleVisual]
  );

  const onPointerUpCapture = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      const gesture = gestureRef.current;
      if (!gesture || gesture.pointerId !== event.pointerId) return;

      gestureRef.current = null;
      try {
        event.currentTarget.releasePointerCapture(event.pointerId);
      } catch {
        // The pointer may not have been captured by this WebView.
      }
      if (!gesture.tracking) return;

      event.preventDefault();
      // WKWebView can report a reset clientX on touch pointerup. The last move sample is the
      // position the user actually saw, so use it for both distance and velocity settlement.
      settleFromLastSample(gesture);
    },
    [settleFromLastSample]
  );

  const onPointerCancelCapture = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      const gesture = gestureRef.current;
      if (!gesture || gesture.pointerId !== event.pointerId) return;
      gestureRef.current = null;
      // WKWebView may finish a captured horizontal touch with pointercancel after it has already
      // delivered the full drag. Complete only when the visible distance itself crossed the normal
      // threshold; do not turn an interrupted short flick into navigation based on velocity alone.
      if (gesture.tracking) settleFromLastSample(gesture, false);
    },
    [settleFromLastSample]
  );

  const onLostPointerCaptureCapture = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      const gesture = gestureRef.current;
      if (!gesture || gesture.pointerId !== event.pointerId) return;
      gestureRef.current = null;
      if (gesture.tracking) settle(false);
    },
    [settle]
  );

  const currentStyle: CSSProperties | undefined = visual
    ? {
        transform: `translate3d(var(${visualProperties.currentOffset}, 0px), 0, 0)`,
        transition: `var(${visualProperties.transition}, none)`
      }
    : undefined;
  const parentStyle: CSSProperties | undefined = visual
    ? {
        transform: `translate3d(var(${visualProperties.parentOffset}, -24%), 0, 0)`,
        transition: `var(${visualProperties.transition}, none)`
      }
    : undefined;

  return {
    active: visual !== null,
    currentStyle,
    parentStyle,
    platformEnabled,
    reset,
    visual,
    pointerHandlers: {
      onPointerDownCapture,
      onPointerMoveCapture,
      onPointerUpCapture,
      onPointerCancelCapture,
      onLostPointerCaptureCapture
    }
  };
}
