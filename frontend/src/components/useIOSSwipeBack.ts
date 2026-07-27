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
  shouldCompleteSwipeBack,
  SWIPE_BACK_EDGE_WIDTH
} from "@/utils/swipeBack";

const NAVIGATION_EASING = "cubic-bezier(0.32, 0.72, 0, 1)";

type SwipeGesture<Context> = {
  context: Context;
  pointerId: number;
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
  offset: number;
  width: number;
  transitionMs: number;
};

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
  const visualRef = useRef<IOSSwipeBackVisual<Context> | null>(null);
  const settleTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [visual, setVisual] = useState<IOSSwipeBackVisual<Context> | null>(null);
  const platformEnabled = platformEnabledRef.current;

  const updateVisual = useCallback((next: IOSSwipeBackVisual<Context> | null) => {
    visualRef.current = next;
    setVisual(next);
  }, []);

  const reset = useCallback(() => {
    if (settleTimerRef.current) clearTimeout(settleTimerRef.current);
    settleTimerRef.current = null;
    gestureRef.current = null;
    updateVisual(null);
  }, [updateVisual]);

  useEffect(() => {
    return () => {
      if (settleTimerRef.current) clearTimeout(settleTimerRef.current);
      settleTimerRef.current = null;
      gestureRef.current = null;
      visualRef.current = null;
    };
  }, []);

  const settle = useCallback(
    (completing: boolean) => {
      const current = visualRef.current;
      if (!current) return;

      const transitionMs = getSwipeBackSettleDuration({
        progress: current.offset / current.width,
        completing,
        reducedMotion: window.matchMedia("(prefers-reduced-motion: reduce)").matches
      });
      updateVisual({
        ...current,
        offset: completing ? current.width : 0,
        transitionMs
      });

      if (settleTimerRef.current) clearTimeout(settleTimerRef.current);
      settleTimerRef.current = setTimeout(() => {
        settleTimerRef.current = null;
        if (completing) {
          onComplete(current.context, reset);
        } else {
          updateVisual(null);
        }
      }, transitionMs);
    },
    [onComplete, reset, updateVisual]
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
      if (target instanceof Element && target.closest("[data-swipe-back-ignore]")) return;

      const width = event.currentTarget.clientWidth || window.innerWidth;
      const context = getContext();
      if (width <= 0 || context === null) return;

      gestureRef.current = {
        context,
        pointerId: event.pointerId,
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
        updateVisual({
          context: gesture.context,
          offset: clampSwipeBackDistance(deltaX, gesture.width),
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
      updateVisual({
        ...current,
        offset: clampSwipeBackDistance(deltaX, gesture.width),
        transitionMs: 0
      });
    },
    [updateVisual]
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
      const current = visualRef.current;
      if (!current) return;
      const releaseDistance = clampSwipeBackDistance(event.clientX - gesture.startX, gesture.width);
      const elapsedSinceMove = Math.max(performance.now() - gesture.lastTime, 1);
      const releaseVelocity = (event.clientX - gesture.lastX) / elapsedSinceMove;
      const recentVelocity = elapsedSinceMove <= 80 ? gesture.velocity : 0;
      updateVisual({ ...current, offset: releaseDistance, transitionMs: 0 });
      settle(
        shouldCompleteSwipeBack({
          distance: releaseDistance,
          width: current.width,
          velocity: Math.max(recentVelocity, releaseVelocity)
        })
      );
    },
    [settle, updateVisual]
  );

  const onPointerCancelCapture = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      const gesture = gestureRef.current;
      if (!gesture || gesture.pointerId !== event.pointerId) return;
      gestureRef.current = null;
      if (gesture.tracking) settle(false);
    },
    [settle]
  );

  const progress = visual ? visual.offset / visual.width : 0;
  const transition = visual?.transitionMs
    ? `transform ${visual.transitionMs}ms ${NAVIGATION_EASING}`
    : "none";
  const currentStyle: CSSProperties | undefined = visual
    ? {
        transform: `translate3d(${visual.offset}px, 0, 0)`,
        transition
      }
    : undefined;
  const parentStyle: CSSProperties | undefined = visual
    ? {
        transform: `translate3d(${-24 * (1 - progress)}%, 0, 0)`,
        transition
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
      onPointerCancelCapture
    }
  };
}
