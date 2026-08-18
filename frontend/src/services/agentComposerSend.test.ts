import { describe, expect, test } from "bun:test";
import {
  agentComposerCanSend,
  agentComposerShowsStop,
  canSubmitAgentComposerMessage,
  isAgentComposerSendLocked,
  planAgentComposerStop,
  shouldClearStoppingSendLock
} from "./agentComposerSend";

describe("agent composer send policy", () => {
  test("accepts a mid-run submit so the native runtime can stage a follow-up", () => {
    expect(
      canSubmitAgentComposerMessage({
        text: "also check the tests",
        isSendLocked: false,
        isSessionSelectionPending: false,
        hasInFlightSend: false
      })
    ).toBe(true);
  });

  test("still blocks empty, locked, in-flight, and selection-pending submits", () => {
    expect(
      canSubmitAgentComposerMessage({
        text: "   ",
        isSendLocked: false,
        isSessionSelectionPending: false,
        hasInFlightSend: false
      })
    ).toBe(false);
    expect(
      canSubmitAgentComposerMessage({
        text: "hello",
        isSendLocked: true,
        isSessionSelectionPending: false,
        hasInFlightSend: false
      })
    ).toBe(false);
    expect(
      canSubmitAgentComposerMessage({
        text: "hello",
        isSendLocked: false,
        isSessionSelectionPending: true,
        hasInFlightSend: false
      })
    ).toBe(false);
    expect(
      canSubmitAgentComposerMessage({
        text: "hello",
        isSendLocked: false,
        isSessionSelectionPending: false,
        hasInFlightSend: true
      })
    ).toBe(false);
    expect(
      canSubmitAgentComposerMessage({
        text: "",
        isSendLocked: false,
        isSessionSelectionPending: false,
        hasInFlightSend: false,
        hasQueuedMessages: true
      })
    ).toBe(true);
    expect(
      canSubmitAgentComposerMessage({
        text: "",
        isSendLocked: false,
        isSessionSelectionPending: false,
        hasInFlightSend: false,
        hasQueuedMessages: false
      })
    ).toBe(false);
  });

  test("locks send while a run is stopping so Stop cannot race a new start", () => {
    expect(
      isAgentComposerSendLocked({
        areSettingsLocked: false,
        isStopping: false
      })
    ).toBe(false);
    expect(
      isAgentComposerSendLocked({
        areSettingsLocked: false,
        isStopping: true
      })
    ).toBe(true);
    expect(
      isAgentComposerSendLocked({
        areSettingsLocked: true,
        isStopping: false
      })
    ).toBe(true);
  });

  test("Stop cancels an in-flight send even when a run is already active", () => {
    expect(
      planAgentComposerStop({
        hasActiveRun: true,
        hasInFlightSend: true
      })
    ).toEqual({
      markInFlightSendCancelled: true,
      cancelActiveRun: true,
      lockSendUntilRunFinished: true
    });
    expect(
      planAgentComposerStop({
        hasActiveRun: true,
        hasInFlightSend: false
      })
    ).toEqual({
      markInFlightSendCancelled: false,
      cancelActiveRun: true,
      lockSendUntilRunFinished: true
    });
    expect(
      planAgentComposerStop({
        hasActiveRun: false,
        hasInFlightSend: true
      })
    ).toEqual({
      markInFlightSendCancelled: true,
      cancelActiveRun: false,
      lockSendUntilRunFinished: false
    });
  });

  test("clears the stopping lock if the cancelled run is already gone", () => {
    expect(
      shouldClearStoppingSendLock({
        cancelledRunId: "run-1",
        trackedRunId: "run-1"
      })
    ).toBe(false);
    expect(
      shouldClearStoppingSendLock({
        cancelledRunId: "run-1",
        trackedRunId: undefined
      })
    ).toBe(true);
    expect(
      shouldClearStoppingSendLock({
        cancelledRunId: "run-1",
        trackedRunId: "run-2"
      })
    ).toBe(true);
  });

  test("keeps stop visible while a run is active and send enabled when the composer has text", () => {
    expect(agentComposerShowsStop(true)).toBe(true);
    expect(agentComposerShowsStop(false)).toBe(false);
    expect(
      agentComposerCanSend({
        text: "keep going",
        isSendDisabled: false,
        projectRoot: "/tmp/project"
      })
    ).toBe(true);
    expect(
      agentComposerCanSend({
        text: "keep going",
        isSendDisabled: false,
        projectRoot: ""
      })
    ).toBe(false);
    expect(
      agentComposerCanSend({
        text: "",
        isSendDisabled: false,
        projectRoot: "/tmp/project",
        hasQueuedMessages: true
      })
    ).toBe(true);
  });
});
