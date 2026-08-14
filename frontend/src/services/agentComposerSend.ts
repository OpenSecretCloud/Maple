export function canSubmitAgentComposerMessage({
  text,
  isSendLocked,
  isSessionSelectionPending,
  hasInFlightSend
}: {
  text: string;
  isSendLocked: boolean;
  isSessionSelectionPending: boolean;
  hasInFlightSend: boolean;
}): boolean {
  return Boolean(text.trim()) && !isSendLocked && !isSessionSelectionPending && !hasInFlightSend;
}

export function agentComposerShowsStop(isSending: boolean): boolean {
  return isSending;
}

export function agentComposerCanSend({
  text,
  isSendDisabled,
  projectRoot
}: {
  text: string;
  isSendDisabled: boolean;
  projectRoot: string;
}): boolean {
  return Boolean(text.trim()) && !isSendDisabled && Boolean(projectRoot);
}
