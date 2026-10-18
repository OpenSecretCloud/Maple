/** Within this distance the transcript counts as being at the bottom. */
export const CHAT_BOTTOM_ATTACH_THRESHOLD_PX = 4;

export type ChatScrollMetrics = Readonly<{
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
}>;

export function chatDistanceFromBottom({
  scrollTop,
  scrollHeight,
  clientHeight
}: ChatScrollMetrics): number {
  return Math.max(0, scrollHeight - scrollTop - clientHeight);
}

export function readChatScrollMetrics(element: ChatScrollMetrics): ChatScrollMetrics {
  return {
    scrollTop: element.scrollTop,
    scrollHeight: element.scrollHeight,
    clientHeight: element.clientHeight
  };
}

/** Keys that scroll a focused transcript toward older content. */
export function isChatBackwardScrollKey(
  event: Readonly<{ key: string; shiftKey: boolean }>
): boolean {
  return (
    event.key === "ArrowUp" ||
    event.key === "PageUp" ||
    event.key === "Home" ||
    (event.shiftKey && (event.key === " " || event.key === "Spacebar"))
  );
}

function isChatForwardScrollKey(event: Readonly<{ key: string; shiftKey: boolean }>): boolean {
  return (
    event.key === "ArrowDown" ||
    event.key === "PageDown" ||
    event.key === "End" ||
    (!event.shiftKey && (event.key === " " || event.key === "Spacebar"))
  );
}

/** Rubber-band overscroll can report offsets outside the scrollable range. */
function clampToScrollRange(metrics: ChatScrollMetrics): ChatScrollMetrics {
  const maxScrollTop = Math.max(0, metrics.scrollHeight - metrics.clientHeight);
  return { ...metrics, scrollTop: Math.min(Math.max(metrics.scrollTop, 0), maxScrollTop) };
}

/**
 * Tracks whether the transcript should keep following the bottom while a
 * response streams.
 *
 * Upward wheel, touch and key input detaches immediately, without a distance
 * threshold; an upward scroll position change (for example a scrollbar drag)
 * detaches when the next scroll event is seen. Following resumes only after a
 * real downward return to the bottom, or when the caller attaches it (on send
 * or a session switch). Content shrinking and overscroll bounce are not
 * treated as user movement.
 */
export class ChatScrollFollow {
  private following = true;
  private previous: ChatScrollMetrics | null = null;
  // Upward input was seen but its scroll position has not been observed yet.
  // Scroll notifications queued before the gesture must not re-attach.
  private upwardInputPending = false;

  get isFollowing(): boolean {
    return this.following;
  }

  /** Follow the bottom. Pass the metrics after any programmatic scroll write. */
  attach(metrics?: ChatScrollMetrics): void {
    this.following = true;
    this.upwardInputPending = false;
    if (metrics) this.previous = clampToScrollRange(metrics);
  }

  detach(metrics?: ChatScrollMetrics): void {
    this.following = false;
    if (metrics) this.previous = clampToScrollRange(metrics);
  }

  /** Wheel or trackpad input; negative deltaY scrolls toward older content. */
  handleWheel(deltaY: number): void {
    if (deltaY < 0) this.detachForUpwardInput();
    else if (deltaY > 0) this.upwardInputPending = false;
  }

  /** Finger movement since the last touch event; positive drags content down. */
  handleTouchMove(fingerDeltaY: number): void {
    if (fingerDeltaY > 0) this.detachForUpwardInput();
    else if (fingerDeltaY < 0) this.upwardInputPending = false;
  }

  handleKey(event: Readonly<{ key: string; shiftKey: boolean }>): void {
    if (isChatBackwardScrollKey(event)) this.detachForUpwardInput();
    else if (isChatForwardScrollKey(event)) this.upwardInputPending = false;
  }

  handleScroll(rawMetrics: ChatScrollMetrics): void {
    const metrics = clampToScrollRange(rawMetrics);
    const previous = this.previous;
    this.previous = metrics;

    const atBottom = chatDistanceFromBottom(metrics) <= CHAT_BOTTOM_ATTACH_THRESHOLD_PX;
    if (!previous) {
      this.following = atBottom && !this.upwardInputPending;
      return;
    }

    const previousMaxScrollTop = previous.scrollHeight - previous.clientHeight;
    const maxScrollTop = metrics.scrollHeight - metrics.clientHeight;
    const clampedByShrink =
      maxScrollTop < previousMaxScrollTop - 0.5 && metrics.scrollTop >= maxScrollTop - 0.5;
    const movedUp = metrics.scrollTop < previous.scrollTop - 0.5 && !clampedByShrink;
    const movedDown = metrics.scrollTop > previous.scrollTop + 0.5;

    if (movedUp) {
      this.following = false;
      this.upwardInputPending = false;
    } else if (movedDown && atBottom && !this.upwardInputPending) {
      this.following = true;
    }
  }

  private detachForUpwardInput(): void {
    this.following = false;
    this.upwardInputPending = true;
  }
}

const TEXT_ENTRY_SELECTOR =
  "input, textarea, select, button, a, [contenteditable='true'], [role='button'], [role='textbox']";

/** Feeds wheel, touch and key scroll input into `follow`. Returns a cleanup function. */
export function listenForChatScrollIntent(
  container: HTMLElement,
  follow: ChatScrollFollow
): () => void {
  let previousTouchY: number | null = null;

  const handleWheel = (event: WheelEvent) => {
    // Ctrl+wheel is pinch-to-zoom, not scrolling.
    if (!event.ctrlKey) follow.handleWheel(event.deltaY);
  };
  const handleTouchStart = (event: TouchEvent) => {
    previousTouchY = event.touches[0]?.clientY ?? null;
  };
  const handleTouchMove = (event: TouchEvent) => {
    const touchY = event.touches[0]?.clientY;
    if (touchY === undefined) return;
    if (previousTouchY !== null) follow.handleTouchMove(touchY - previousTouchY);
    previousTouchY = touchY;
  };
  const handleKeyDown = (event: KeyboardEvent) => {
    const target = event.target;
    if (target instanceof Element && target.closest(TEXT_ENTRY_SELECTOR)) return;
    follow.handleKey(event);
  };

  container.addEventListener("wheel", handleWheel, { passive: true });
  container.addEventListener("touchstart", handleTouchStart, { passive: true });
  container.addEventListener("touchmove", handleTouchMove, { passive: true });
  window.addEventListener("keydown", handleKeyDown);
  return () => {
    container.removeEventListener("wheel", handleWheel);
    container.removeEventListener("touchstart", handleTouchStart);
    container.removeEventListener("touchmove", handleTouchMove);
    window.removeEventListener("keydown", handleKeyDown);
  };
}
