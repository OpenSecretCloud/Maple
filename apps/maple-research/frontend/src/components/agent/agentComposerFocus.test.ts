import { describe, expect, test } from "bun:test";
import {
  isDeliberateAgentComposerFocusTarget,
  settleAgentComposerFocusRequest,
  type AgentComposerFocusContext,
  type AgentComposerFocusRequest,
  type AgentComposerFocusTarget
} from "./agentComposerFocus";

function focusHarness(overrides: Partial<AgentComposerFocusContext> = {}) {
  const body = {};
  const calls: Array<FocusOptions | undefined> = [];
  const textarea: AgentComposerFocusTarget = {
    disabled: false,
    focus: (options) => calls.push(options)
  };
  const context: AgentComposerFocusContext = {
    currentInteractionGeneration: 3,
    isSubmitting: false,
    hasTimeline: true,
    textarea,
    activeElement: body,
    documentBody: body,
    documentRoot: {},
    ...overrides
  };
  const request: AgentComposerFocusRequest = {
    sendToken: 7,
    interactionGeneration: 3,
    waitForTimeline: false
  };
  return { body, calls, context, request, textarea };
}

describe("settleAgentComposerFocusRequest", () => {
  test("waits for submit unlock and the first accepted timeline row", () => {
    const { calls, context, request } = focusHarness({
      isSubmitting: true,
      hasTimeline: false
    });
    request.waitForTimeline = true;

    expect(settleAgentComposerFocusRequest(request, context)).toBe(false);
    context.isSubmitting = false;
    expect(settleAgentComposerFocusRequest(request, context)).toBe(false);
    context.hasTimeline = true;
    expect(settleAgentComposerFocusRequest(request, context)).toBe(true);
    expect(calls).toEqual([{ preventScroll: true }]);
  });

  test("waits while the current composer is absent or disabled", () => {
    const { context, request, textarea } = focusHarness({ textarea: null });

    expect(settleAgentComposerFocusRequest(request, context)).toBe(false);
    context.textarea = textarea;
    textarea.disabled = true;
    expect(settleAgentComposerFocusRequest(request, context)).toBe(false);
  });

  test("does not focus again when the composer already owns focus", () => {
    const { calls, context, request, textarea } = focusHarness();
    context.activeElement = textarea;

    expect(settleAgentComposerFocusRequest(request, context)).toBe(true);
    expect(calls).toEqual([]);
  });

  test("cancels after a task interaction or deliberate focus change", () => {
    const changedTask = focusHarness({ currentInteractionGeneration: 4 });
    expect(settleAgentComposerFocusRequest(changedTask.request, changedTask.context)).toBe(true);
    expect(changedTask.calls).toEqual([]);

    const changedFocus = focusHarness({ activeElement: {} });
    expect(settleAgentComposerFocusRequest(changedFocus.request, changedFocus.context)).toBe(true);
    expect(changedFocus.calls).toEqual([]);
  });

  test("retains a deliberate keyboard focus move after its control disappears", () => {
    const { body, calls, context, request, textarea } = focusHarness();
    const documentRoot = context.documentRoot;
    const otherControl = {};

    const cancelled = isDeliberateAgentComposerFocusTarget(
      otherControl,
      textarea,
      body,
      documentRoot
    );
    expect(cancelled).toBe(true);
    expect(isDeliberateAgentComposerFocusTarget(body, textarea, body, documentRoot)).toBe(false);
    expect(isDeliberateAgentComposerFocusTarget(documentRoot, textarea, body, documentRoot)).toBe(
      false
    );

    const currentRequest = cancelled ? null : request;
    context.activeElement = body;
    if (currentRequest) {
      settleAgentComposerFocusRequest(currentRequest, context);
    }
    expect(calls).toEqual([]);
  });

  test("a failed first send can release the timeline wait and refocus the empty composer", () => {
    const { calls, context, request } = focusHarness({ hasTimeline: false });
    request.waitForTimeline = true;

    expect(settleAgentComposerFocusRequest(request, context)).toBe(false);
    request.waitForTimeline = false;
    expect(settleAgentComposerFocusRequest(request, context)).toBe(true);
    expect(calls).toEqual([{ preventScroll: true }]);
  });
});
