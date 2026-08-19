export interface AgentComposerFocusRequest {
  readonly sendToken: number;
  readonly interactionGeneration: number;
  waitForTimeline: boolean;
}

export interface AgentComposerFocusTarget {
  disabled: boolean;
  focus(options?: FocusOptions): void;
}

export interface AgentComposerFocusContext {
  currentInteractionGeneration: number;
  isSubmitting: boolean;
  hasTimeline: boolean;
  textarea: AgentComposerFocusTarget | null;
  activeElement: unknown;
  documentBody: unknown;
  documentRoot: unknown;
}

export function isDeliberateAgentComposerFocusTarget(
  target: unknown,
  textarea: AgentComposerFocusTarget | null,
  documentBody: unknown,
  documentRoot: unknown
): boolean {
  return (
    target !== null && target !== textarea && target !== documentBody && target !== documentRoot
  );
}

/** Returns true when the request has either been fulfilled or invalidated. */
export function settleAgentComposerFocusRequest(
  request: AgentComposerFocusRequest,
  context: AgentComposerFocusContext
): boolean {
  if (request.interactionGeneration !== context.currentInteractionGeneration) {
    return true;
  }

  if (
    isDeliberateAgentComposerFocusTarget(
      context.activeElement,
      context.textarea,
      context.documentBody,
      context.documentRoot
    )
  ) {
    return true;
  }

  if (
    context.isSubmitting ||
    (request.waitForTimeline && !context.hasTimeline) ||
    !context.textarea ||
    context.textarea.disabled
  ) {
    return false;
  }

  if (context.activeElement !== context.textarea) {
    context.textarea.focus({ preventScroll: true });
  }
  return true;
}
