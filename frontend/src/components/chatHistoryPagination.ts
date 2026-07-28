export const CHAT_HISTORY_TOP_MARGIN_PX = 100;

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

export function chatHistoryScrolledBackward(
  previousScrollTop: number,
  nextScrollTop: number
): boolean {
  return nextScrollTop < previousScrollTop;
}

export class ChatHistoryPaginationGate {
  private intentArmed = false;
  private gestureActive = false;
  private loadInFlight = false;

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
  }

  resetIntent(): void {
    this.gestureActive = false;
    this.intentArmed = false;
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
