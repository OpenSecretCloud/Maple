import { describe, expect, test } from "bun:test";
import { createChatDraftKey, createConversationChatKey } from "./chatRuntimeStore";
import {
  canAdoptRecordingDestination,
  cleanupRecordingForNavigation,
  cleanupRecordingForTeardown,
  isRecordingOwnershipCurrent
} from "./chatRecordingNavigation";

describe("cleanupRecordingForNavigation", () => {
  test("blocks adoption for the runtime that owns pending or active voice work", () => {
    const destination = createConversationChatKey("voice-destination");

    expect(canAdoptRecordingDestination(destination, destination)).toBe(false);
    expect(
      canAdoptRecordingDestination(destination, createConversationChatKey("other-recording"))
    ).toBe(true);
    expect(canAdoptRecordingDestination(destination, null)).toBe(true);
  });

  test("clears a pending draft owner before the destination becomes interactive", () => {
    const draftKey = createChatDraftKey("pending-recording");
    const actions: string[] = [];

    const result = cleanupRecordingForNavigation({
      ownerKey: draftKey,
      destinationKey: createConversationChatKey("destination"),
      recorder: null,
      stream: null,
      clearOwnership: () => actions.push("clear-owner"),
      clearRecorder: () => actions.push("clear-recorder"),
      clearStream: () => actions.push("clear-stream")
    });

    expect(result).toEqual({ stopped: true, errors: [] });
    expect(actions).toEqual(["clear-owner", "clear-recorder", "clear-stream"]);
  });

  test("stops every active conversation recording resource even when one cleanup throws", () => {
    const ownerKey = createConversationChatKey("active-recording");
    const actions: string[] = [];

    const result = cleanupRecordingForNavigation({
      ownerKey,
      destinationKey: createChatDraftKey("next-draft"),
      recorder: {
        stopRecording: () => {
          actions.push("stop-recorder");
          throw new Error("recorder stop failed");
        }
      },
      stream: {
        getTracks: () => [
          { stop: () => actions.push("stop-track-1") },
          { stop: () => actions.push("stop-track-2") }
        ]
      },
      clearOwnership: () => actions.push("clear-owner"),
      clearRecorder: () => actions.push("clear-recorder"),
      clearStream: () => actions.push("clear-stream")
    });

    expect(result.stopped).toBe(true);
    expect(result.errors).toHaveLength(1);
    expect(actions).toEqual([
      "clear-owner",
      "stop-recorder",
      "stop-track-1",
      "stop-track-2",
      "clear-recorder",
      "clear-stream"
    ]);
  });

  test("preserves recording resources only when the owner remains selected", () => {
    const ownerKey = createConversationChatKey("recording-owner");
    let cleanupCalls = 0;

    const result = cleanupRecordingForNavigation({
      ownerKey,
      destinationKey: ownerKey,
      recorder: { stopRecording: () => (cleanupCalls += 1) },
      stream: { getTracks: () => [{ stop: () => (cleanupCalls += 1) }] },
      clearOwnership: () => (cleanupCalls += 1),
      clearRecorder: () => (cleanupCalls += 1),
      clearStream: () => (cleanupCalls += 1)
    });

    expect(result).toEqual({ stopped: false, errors: [] });
    expect(cleanupCalls).toBe(0);
  });

  test("does nothing when there is no recording owner", () => {
    let cleanupCalls = 0;

    const result = cleanupRecordingForNavigation({
      ownerKey: null,
      destinationKey: createChatDraftKey("destination"),
      recorder: null,
      stream: null,
      clearOwnership: () => (cleanupCalls += 1),
      clearRecorder: () => (cleanupCalls += 1),
      clearStream: () => (cleanupCalls += 1)
    });

    expect(result.stopped).toBe(false);
    expect(cleanupCalls).toBe(0);
  });

  test("full teardown clears ownership before stopping conversation resources", () => {
    const ownerKey = createConversationChatKey("teardown-owner");
    let currentOwner: string | null = ownerKey;
    const actions: string[] = [];

    const result = cleanupRecordingForTeardown({
      recorder: {
        stopRecording: () => actions.push(`stop-recorder:${currentOwner ?? "cleared"}`)
      },
      stream: {
        getTracks: () => [{ stop: () => actions.push("stop-track") }]
      },
      clearOwnership: () => {
        currentOwner = null;
        actions.push("clear-owner");
      },
      clearRecorder: () => actions.push("clear-recorder"),
      clearStream: () => actions.push("clear-stream")
    });

    expect(result).toEqual({ errors: [] });
    expect(actions).toEqual([
      "clear-owner",
      "stop-recorder:cleared",
      "stop-track",
      "clear-recorder",
      "clear-stream"
    ]);
  });

  test("late microphone and transcription completions cannot commit after teardown", () => {
    const ownerKey = createChatDraftKey("late-async-owner");
    let currentOwner: string | null = ownerKey;
    let currentSessionToken = 1;
    let lateRecorderCreations = 0;
    let lateSends = 0;
    const commitLateMicrophone = () => {
      if (isRecordingOwnershipCurrent(ownerKey, 1, currentOwner, currentSessionToken, true)) {
        lateRecorderCreations += 1;
      }
    };
    const commitLateTranscription = () => {
      if (isRecordingOwnershipCurrent(ownerKey, 1, currentOwner, currentSessionToken, true)) {
        lateSends += 1;
      }
    };

    expect(isRecordingOwnershipCurrent(ownerKey, 1, currentOwner, currentSessionToken, true)).toBe(
      true
    );
    cleanupRecordingForTeardown({
      recorder: null,
      stream: null,
      clearOwnership: () => {
        currentOwner = null;
        currentSessionToken += 1;
      },
      clearRecorder: () => {},
      clearStream: () => {}
    });
    commitLateMicrophone();
    commitLateTranscription();

    expect(lateRecorderCreations).toBe(0);
    expect(lateSends).toBe(0);
    expect(isRecordingOwnershipCurrent(ownerKey, 1, ownerKey, 1, false)).toBe(false);
  });

  test("a late session cannot commit after returning to the same chat", () => {
    const ownerKey = createConversationChatKey("same-chat");

    expect(isRecordingOwnershipCurrent(ownerKey, 1, ownerKey, 2, true)).toBe(false);
    expect(isRecordingOwnershipCurrent(ownerKey, 2, ownerKey, 2, true)).toBe(true);
  });

  test("a delayed A stop callback releases only captured A resources after B takes ownership", () => {
    const ownerA = createConversationChatKey("owner-a");
    const ownerB = createConversationChatKey("owner-b");
    let stoppedATracks = 0;
    let stoppedBTracks = 0;
    const streamA = { getTracks: () => [{ stop: () => (stoppedATracks += 1) }] };
    const streamB = { getTracks: () => [{ stop: () => (stoppedBTracks += 1) }] };
    let currentOwner: string | null = ownerA;
    let currentStream: typeof streamA | null = streamA;
    const capturedAStream = currentStream;
    let transcriptionStarts = 0;

    // B replaces both refs before RecordRTC invokes A's delayed stop callback.
    currentOwner = ownerB;
    currentStream = streamB;

    if (!isRecordingOwnershipCurrent(ownerA, 1, currentOwner, 2, true)) {
      capturedAStream.getTracks().forEach((track) => track.stop());
      if (currentStream === capturedAStream) currentStream = null;
    } else {
      transcriptionStarts += 1;
    }

    expect(stoppedATracks).toBe(1);
    expect(stoppedBTracks).toBe(0);
    expect(currentOwner).toBe(ownerB);
    expect(currentStream).toBe(streamB);
    expect(transcriptionStarts).toBe(0);
  });
});
