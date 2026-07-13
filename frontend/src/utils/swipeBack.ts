export const SWIPE_BACK_EDGE_WIDTH = 28;
export const SWIPE_BACK_DIRECTION_LOCK = 8;
export const SWIPE_BACK_DISTANCE_THRESHOLD = 0.35;
export const SWIPE_BACK_VELOCITY_THRESHOLD = 0.5;

export type SwipeBackDirection = "pending" | "track" | "reject";

export function getSwipeBackDirection(deltaX: number, deltaY: number): SwipeBackDirection {
  if (Math.max(Math.abs(deltaX), Math.abs(deltaY)) < SWIPE_BACK_DIRECTION_LOCK) {
    return "pending";
  }

  if (deltaX <= 0 || Math.abs(deltaY) >= Math.abs(deltaX)) {
    return "reject";
  }

  return "track";
}

export function clampSwipeBackDistance(distance: number, width: number) {
  if (!Number.isFinite(width) || width <= 0) return 0;
  return Math.min(Math.max(distance, 0), width);
}

export function shouldCompleteSwipeBack({
  distance,
  width,
  velocity
}: {
  distance: number;
  width: number;
  velocity: number;
}) {
  if (!Number.isFinite(width) || width <= 0) return false;
  return (
    distance / width >= SWIPE_BACK_DISTANCE_THRESHOLD || velocity >= SWIPE_BACK_VELOCITY_THRESHOLD
  );
}

export function getSwipeBackSettleDuration({
  progress,
  completing,
  reducedMotion
}: {
  progress: number;
  completing: boolean;
  reducedMotion: boolean;
}) {
  if (reducedMotion) return 0;

  const clampedProgress = Math.min(Math.max(progress, 0), 1);
  const remaining = completing ? 1 - clampedProgress : clampedProgress;
  return Math.round(Math.min(260, Math.max(120, 320 * remaining)));
}
