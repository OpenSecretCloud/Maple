export const CHAT_HISTORY_TOP_MARGIN_PX = 100;
export const CHAT_HISTORY_WHEEL_GESTURE_QUIET_MS = 180;

export type ChatHistoryScrollSnapshot = {
  scrollTop: number;
  scrollHeight: number;
  anchorId?: string;
  anchorOffset?: number;
};

export function restoredChatHistoryScrollTop(
  snapshot: ChatHistoryScrollSnapshot,
  nextScrollHeight: number
): number {
  return Math.max(0, snapshot.scrollTop + nextScrollHeight - snapshot.scrollHeight);
}

export function restoredChatHistoryAnchorScrollTop(
  currentScrollTop: number,
  previousAnchorOffset: number,
  nextAnchorOffset: number
): number {
  return Math.max(0, currentScrollTop + nextAnchorOffset - previousAnchorOffset);
}

export function requiredChatHistoryBottomCompensation(
  restoredScrollTop: number,
  scrollHeight: number,
  clientHeight: number
): number {
  const maxScrollTop = Math.max(0, scrollHeight - clientHeight);
  return Math.max(0, restoredScrollTop - maxScrollTop);
}

export function chatHistoryCursorProgressed(
  currentOldestItemId: string,
  nextOldestItemId: string | undefined
): boolean {
  return Boolean(nextOldestItemId && nextOldestItemId !== currentOldestItemId);
}

export class ChatHistoryPaginationGate {
  private intentArmed = false;
  private gestureActive = false;
  private loadInFlight = false;
  private lastWheelInputAt: number | null = null;

  beginWheelGesture(eventTimestamp: number): void {
    if (
      this.lastWheelInputAt !== null &&
      eventTimestamp - this.lastWheelInputAt >= CHAT_HISTORY_WHEEL_GESTURE_QUIET_MS
    ) {
      // A busy prepend/render can delay the compatibility timeout past the next
      // physical swipe. Preserve the same quiet-period boundary synchronously
      // from the browser event timestamps so that swipe is not consumed as
      // momentum from the previous page.
      this.endGesture();
    }

    this.lastWheelInputAt = eventTimestamp;
    this.beginGesture();
  }

  beginGesture(): void {
    if (this.gestureActive) return;

    this.gestureActive = true;
    if (!this.loadInFlight) {
      this.intentArmed = true;
    }
  }

  endGesture(): void {
    this.gestureActive = false;
    this.intentArmed = false;
    this.lastWheelInputAt = null;
  }

  resetIntent(): void {
    this.gestureActive = false;
    this.intentArmed = false;
    this.lastWheelInputAt = null;
  }

  tryStartLoad({
    canLoad,
    topBoundaryVisible
  }: {
    canLoad: boolean;
    topBoundaryVisible: boolean;
  }): boolean {
    if (!canLoad || !topBoundaryVisible || !this.intentArmed || this.loadInFlight) {
      return false;
    }

    this.intentArmed = false;
    this.loadInFlight = true;
    return true;
  }

  finishLoad(): void {
    this.loadInFlight = false;
    this.intentArmed = false;
  }
}
