export function isAgentComposerSteerHotkey(event: {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}): boolean {
  return (
    event.key === "Enter" && (event.metaKey || event.ctrlKey) && !event.altKey && !event.shiftKey
  );
}

export function canSubmitAgentComposerMessage({
  text,
  isSendLocked,
  isSessionSelectionPending,
  hasInFlightSend,
  hasQueuedMessages = false,
  hasActiveRun = false,
  steerNow = false
}: {
  text: string;
  isSendLocked: boolean;
  isSessionSelectionPending: boolean;
  hasInFlightSend: boolean;
  hasQueuedMessages?: boolean;
  hasActiveRun?: boolean;
  steerNow?: boolean;
}): boolean {
  const hasSendableText = Boolean(text.trim());
  const canFlushQueuedOnly = hasQueuedMessages && !hasActiveRun;
  const canSteerQueuedStack = steerNow && hasQueuedMessages && !hasSendableText;
  return (
    (hasSendableText || canFlushQueuedOnly || canSteerQueuedStack) &&
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
  hasQueuedMessages = false,
  hasActiveRun = false
}: {
  text: string;
  isSendDisabled: boolean;
  projectRoot: string;
  hasQueuedMessages?: boolean;
  hasActiveRun?: boolean;
}): boolean {
  const hasSendableText = Boolean(text.trim());
  const canFlushQueuedOnly = hasQueuedMessages && !hasActiveRun;
  return (hasSendableText || canFlushQueuedOnly) && !isSendDisabled && Boolean(projectRoot);
}
