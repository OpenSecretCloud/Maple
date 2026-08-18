export function canSubmitAgentComposerMessage({
  text,
  isSendLocked,
  isSessionSelectionPending,
  hasInFlightSend,
  hasQueuedMessages = false
}: {
  text: string;
  isSendLocked: boolean;
  isSessionSelectionPending: boolean;
  hasInFlightSend: boolean;
  hasQueuedMessages?: boolean;
}): boolean {
  return (
    (Boolean(text.trim()) || hasQueuedMessages) &&
    !isSendLocked &&
    !isSessionSelectionPending &&
    !hasInFlightSend
  );
}

export function isAgentComposerSendLocked({
  areSettingsLocked,
  isStopping
}: {
  areSettingsLocked: boolean;
  isStopping: boolean;
}): boolean {
  return areSettingsLocked || isStopping;
}

export function planAgentComposerStop({
  hasActiveRun,
  hasInFlightSend
}: {
  hasActiveRun: boolean;
  hasInFlightSend: boolean;
}): {
  markInFlightSendCancelled: boolean;
  cancelActiveRun: boolean;
  lockSendUntilRunFinished: boolean;
} {
  return {
    markInFlightSendCancelled: hasInFlightSend,
    cancelActiveRun: hasActiveRun,
    lockSendUntilRunFinished: hasActiveRun
  };
}

export function shouldClearStoppingSendLock({
  cancelledRunId,
  trackedRunId
}: {
  cancelledRunId: string;
  trackedRunId: string | undefined;
}): boolean {
  return trackedRunId !== cancelledRunId;
}

export function agentComposerShowsStop(isSending: boolean): boolean {
  return isSending;
}

export function agentComposerCanSend({
  text,
  isSendDisabled,
  projectRoot,
  hasQueuedMessages = false
}: {
  text: string;
  isSendDisabled: boolean;
  projectRoot: string;
  hasQueuedMessages?: boolean;
}): boolean {
  return (Boolean(text.trim()) || hasQueuedMessages) && !isSendDisabled && Boolean(projectRoot);
}
