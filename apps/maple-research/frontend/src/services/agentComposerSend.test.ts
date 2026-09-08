import { describe, expect, test } from "bun:test";
import {
  agentComposerCanSend,
  agentComposerShowsStop,
  canSubmitAgentComposerMessage,
  isAgentComposerSendLocked,
  isAgentComposerSteerHotkey,
  planAgentComposerStop,
  shouldClearStoppingSendLock
} from "./agentComposerSend";

describe("agent composer send policy", () => {
  test("treats Command or Control Enter as a steer bypass", () => {
    expect(
      isAgentComposerSteerHotkey({
        key: "Enter",
        metaKey: true,
        ctrlKey: false,
        altKey: false,
        shiftKey: false
      })
    ).toBe(true);
    expect(
      isAgentComposerSteerHotkey({
        key: "Enter",
        metaKey: false,
        ctrlKey: true,
        altKey: false,
        shiftKey: false
      })
    ).toBe(true);
    expect(
      isAgentComposerSteerHotkey({
        key: "Enter",
        metaKey: false,
        ctrlKey: false,
        altKey: false,
        shiftKey: false
      })
    ).toBe(false);
  });

  test("accepts a mid-run submit so the native runtime can stage a follow-up", () => {
    expect(
      canSubmitAgentComposerMessage({
        text: "also check the tests",
        isSendLocked: false,
        isSessionSelectionPending: false,
        hasInFlightSend: false
      })
    ).toBe(true);
    expect(
      canSubmitAgentComposerMessage({
        text: "",
        hasAttachments: true,
        isSendLocked: false,
        isSessionSelectionPending: false,
        hasInFlightSend: false
      })
    ).toBe(true);
    expect(
      agentComposerCanSend({
        text: "",
        hasAttachments: true,
        isSendDisabled: false,
        projectRoot: "/tmp/project"
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
        hasQueuedMessages: true,
        hasActiveRun: true
      })
    ).toBe(false);
    expect(
      canSubmitAgentComposerMessage({
        text: "",
        isSendLocked: false,
        isSessionSelectionPending: false,
        hasInFlightSend: false,
        hasQueuedMessages: true,
        hasActiveRun: true,
        steerNow: true
      })
    ).toBe(true);
    expect(
      canSubmitAgentComposerMessage({
        text: "",
        isSendLocked: false,
        isSessionSelectionPending: false,
        hasInFlightSend: false,
        hasQueuedMessages: false,
        hasActiveRun: true,
        steerNow: true
      })
    ).toBe(false);
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
    expect(
      agentComposerCanSend({
        text: "",
        isSendDisabled: false,
        projectRoot: "/tmp/project",
        hasQueuedMessages: true,
        hasActiveRun: true
      })
    ).toBe(false);
  });
});
