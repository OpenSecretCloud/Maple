import type { ChatRuntimeKey } from "./chatRuntimeStore";

export type ChatRecordingStopper = {
  stopRecording: (callback: () => void) => void;
};

export type ChatRecordingStream = {
  getTracks: () => ReadonlyArray<{ stop: () => void }>;
};

export type ChatRecordingNavigationCleanupResult = Readonly<{
  stopped: boolean;
  errors: readonly unknown[];
}>;

export type ChatRecordingResourceCleanupOptions = Readonly<{
  recorder: ChatRecordingStopper | null;
  stream: ChatRecordingStream | null;
  clearOwnership: () => void;
  clearRecorder: () => void;
  clearStream: () => void;
}>;

export type ChatRecordingNavigationCleanupOptions = ChatRecordingResourceCleanupOptions &
  Readonly<{
    /** Keys must be canonicalized through ChatRuntimeStore.resolveKey first. */
    ownerKey: ChatRuntimeKey | null;
    destinationKey: ChatRuntimeKey;
  }>;

export type ChatRecordingTeardownCleanupResult = Readonly<{
  errors: readonly unknown[];
}>;

/**
 * Keys must already be canonicalized through ChatRuntimeStore.resolveKey.
 * Pending microphone access, active recording, and transcription all retain the
 * same owner key, so none of them may have their destination adopted by a run.
 */
export function canAdoptRecordingDestination(
  destinationKey: ChatRuntimeKey,
  recordingOwnerKey: ChatRuntimeKey | null
): boolean {
  return recordingOwnerKey !== destinationKey;
}

function cleanupRecordingResources({
  recorder,
  stream,
  clearOwnership,
  clearRecorder,
  clearStream
}: ChatRecordingResourceCleanupOptions): unknown[] {
  const errors: unknown[] = [];
  const run = (action: () => void) => {
    try {
      action();
    } catch (error) {
      errors.push(error);
    }
  };

  // Ownership is fenced before any fallible cleanup, preventing late async
  // microphone or transcription work from committing after navigation/teardown.
  run(clearOwnership);
  if (recorder) run(() => recorder.stopRecording(() => {}));

  if (stream) {
    let tracks: ReadonlyArray<{ stop: () => void }> = [];
    run(() => {
      tracks = stream.getTracks();
    });
    for (const track of tracks) run(() => track.stop());
  }

  run(clearRecorder);
  run(clearStream);
  return errors;
}

/** Stops every local recording resource during unmount or account teardown. */
export function cleanupRecordingForTeardown(
  options: ChatRecordingResourceCleanupOptions
): ChatRecordingTeardownCleanupResult {
  return { errors: cleanupRecordingResources(options) };
}

/**
 * Guards commits after awaited microphone/transcription work. The runtime check
 * also fences account teardown, where the owner ref and store entry both vanish.
 */
export function isRecordingOwnershipCurrent(
  expectedOwnerKey: ChatRuntimeKey,
  expectedSessionToken: number,
  currentOwnerKey: ChatRuntimeKey | null,
  currentSessionToken: number,
  ownerRuntimeExists: boolean
): boolean {
  return (
    currentOwnerKey === expectedOwnerKey &&
    currentSessionToken === expectedSessionToken &&
    ownerRuntimeExists
  );
}

/**
 * Stops recording work owned by the chat being left. Recording UI and browser
 * media resources are component-local, so preserving them offscreen would make
 * the destination chat inherit an invisible, globally blocking microphone.
 * Ownership is cleared before resources are stopped, fencing pending
 * getUserMedia and transcription completions before the destination becomes
 * interactive.
 */
export function cleanupRecordingForNavigation({
  ownerKey,
  destinationKey,
  recorder,
  stream,
  clearOwnership,
  clearRecorder,
  clearStream
}: ChatRecordingNavigationCleanupOptions): ChatRecordingNavigationCleanupResult {
  if (!ownerKey || ownerKey === destinationKey) {
    return { stopped: false, errors: [] };
  }

  const errors = cleanupRecordingResources({
    recorder,
    stream,
    clearOwnership,
    clearRecorder,
    clearStream
  });

  return { stopped: true, errors };
}
