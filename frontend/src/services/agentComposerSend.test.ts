import { describe, expect, test } from "bun:test";
import {
  agentComposerCanSend,
  agentComposerShowsStop,
  canSubmitAgentComposerMessage
} from "./agentComposerSend";

describe("agent composer send policy", () => {
  test("accepts a mid-run submit so the native runtime can queue a Goose steer", () => {
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
  });
});
