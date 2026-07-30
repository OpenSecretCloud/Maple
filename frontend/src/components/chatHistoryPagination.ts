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

export function usesFirstCancelableWheelGestureStart({
  isTauriEnvironment,
  isMacOSPlatform,
  browserPlatform
}: {
  isTauriEnvironment: boolean;
  isMacOSPlatform: boolean;
  browserPlatform: string;
}): boolean {
  return isTauriEnvironment ? isMacOSPlatform : browserPlatform.startsWith("Mac");
}

export class ChatHistoryPaginationGate {
  private intentArmed = false;
  private gestureActive = false;
  private loadInFlight = false;
  private queuedLoad = false;

  beginGesture(): void {
    if (this.gestureActive) return;

    this.gestureActive = true;
    this.intentArmed = true;
  }

  beginWheelGesture(isNewGesture: boolean): void {
    if (!isNewGesture) return;

    // macOS WKWebView and Chrome make the first wheel event in a hardware
    // gesture cancelable and later direct/momentum events non-cancelable when
    // that first event is not prevented. A new first event is therefore
    // stronger evidence than an arbitrary quiet-period timer and may replace
    // a still active, already-consumed gesture.
    this.gestureActive = true;
    this.intentArmed = true;
  }

  endGesture(): void {
    this.gestureActive = false;
    this.intentArmed = false;
  }

  resetIntent(): void {
    this.gestureActive = false;
    this.intentArmed = false;
    this.queuedLoad = false;
  }

  tryStartLoad({
    canLoad,
    topBoundaryVisible,
    requestInFlight = false
  }: {
    canLoad: boolean;
    topBoundaryVisible: boolean;
    requestInFlight?: boolean;
  }): boolean {
    if (!canLoad || !topBoundaryVisible || !this.intentArmed) {
      return false;
    }

    this.intentArmed = false;

    if (this.loadInFlight) {
      // The new gesture already reached the boundary, so preserve exactly one
      // later page even if that gesture ends before the active prepend settles.
      this.queuedLoad = true;
      return false;
    }

    // A runtime can be projected again while a request owned by its previous
    // projection is still pending. Do not issue the same cursor twice from the
    // new gate, and do not turn that discarded intent into a post-failure retry.
    if (requestInFlight) return false;

    this.loadInFlight = true;
    return true;
  }

  finishLoad({ preserveQueuedLoad = false }: { preserveQueuedLoad?: boolean } = {}): void {
    this.loadInFlight = false;
    if (!preserveQueuedLoad) {
      this.queuedLoad = false;
      this.intentArmed = false;
    }
  }

  tryStartQueuedLoad({ canLoad }: { canLoad: boolean }): boolean {
    if (!this.queuedLoad) return false;
    if (this.loadInFlight) return false;

    this.queuedLoad = false;
    if (!canLoad) {
      this.intentArmed = false;
      return false;
    }

    this.loadInFlight = true;
    return true;
  }
}
