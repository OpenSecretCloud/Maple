interface TTSChunkPlaybackOptions<T> {
  chunkCount: number;
  prebufferBeforePlayback: boolean;
  prepareChunk: (chunkIndex: number) => Promise<T | null>;
  scheduleChunk: (chunk: T) => void;
  isActive: () => boolean;
  beforeBufferedSchedule?: () => Promise<void>;
}

export interface TTSPlaybackState {
  isPreparing: boolean;
  isPlaying: boolean;
  currentPlayingId: string | null;
}

export type TTSPlaybackAction =
  | { type: "prepare"; messageId: string }
  | { type: "play" }
  | { type: "idle" };

export const INITIAL_TTS_PLAYBACK_STATE: TTSPlaybackState = {
  isPreparing: false,
  isPlaying: false,
  currentPlayingId: null
};

export function reduceTTSPlaybackState(
  state: TTSPlaybackState,
  action: TTSPlaybackAction
): TTSPlaybackState {
  switch (action.type) {
    case "prepare":
      return {
        isPreparing: true,
        isPlaying: false,
        currentPlayingId: action.messageId
      };
    case "play":
      return state.currentPlayingId
        ? {
            isPreparing: false,
            isPlaying: true,
            currentPlayingId: state.currentPlayingId
          }
        : state;
    case "idle":
      return INITIAL_TTS_PLAYBACK_STATE;
  }
}

/**
 * Prepares chunks sequentially and either schedules each one immediately or
 * waits until every chunk is ready. iOS uses the buffered mode so locking the
 * device after playback starts cannot suspend later inference or audio decoding.
 */
export async function prepareAndScheduleTTSChunks<T>({
  chunkCount,
  prebufferBeforePlayback,
  prepareChunk,
  scheduleChunk,
  isActive,
  beforeBufferedSchedule
}: TTSChunkPlaybackOptions<T>): Promise<boolean> {
  const bufferedChunks: T[] = [];

  for (let chunkIndex = 0; chunkIndex < chunkCount; chunkIndex += 1) {
    let chunk: T | null;
    try {
      chunk = await prepareChunk(chunkIndex);
    } catch (error) {
      if (!isActive()) {
        return false;
      }
      throw error;
    }
    if (!isActive()) {
      return false;
    }
    if (chunk === null) {
      continue;
    }

    if (prebufferBeforePlayback) {
      bufferedChunks.push(chunk);
    } else {
      scheduleChunk(chunk);
    }
  }

  if (!prebufferBeforePlayback || bufferedChunks.length === 0) {
    return true;
  }

  if (beforeBufferedSchedule) {
    await beforeBufferedSchedule();
    if (!isActive()) {
      return false;
    }
  }

  for (const chunk of bufferedChunks) {
    scheduleChunk(chunk);
  }
  bufferedChunks.length = 0;

  return true;
}
