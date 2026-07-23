interface TTSChunkPlaybackOptions<T> {
  chunkCount: number;
  prebufferBeforePlayback: boolean;
  prepareChunk: (chunkIndex: number) => Promise<T | null>;
  scheduleChunk: (chunk: T) => void;
  isActive: () => boolean;
  beforeBufferedSchedule?: () => Promise<void>;
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
    const chunk = await prepareChunk(chunkIndex);
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
