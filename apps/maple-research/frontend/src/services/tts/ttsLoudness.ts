export const TTS_ACTIVE_LOUDNESS_TARGET_DBFS = -21;
export const TTS_ACTIVE_LOUDNESS_GATE_DBFS = -50;
export const TTS_PEAK_CEILING_DBFS = -1;
export const TTS_MAX_NORMALIZATION_BOOST_DB = 24;
export const TTS_MAX_NORMALIZATION_ATTENUATION_DB = -24;

const TTS_LOUDNESS_WINDOW_SECONDS = 0.02;

export interface PCMBufferLike {
  length: number;
  numberOfChannels: number;
  sampleRate: number;
  getChannelData: (channel: number) => Float32Array;
}

export interface TTSLoudnessAdjustment {
  activeRmsDbfs: number | null;
  gain: number;
  gainDb: number;
  peakDbfs: number | null;
  peakLimited: boolean;
}

const UNITY_ADJUSTMENT: TTSLoudnessAdjustment = {
  activeRmsDbfs: null,
  gain: 1,
  gainDb: 0,
  peakDbfs: null,
  peakLimited: false
};

function dbToLinear(db: number): number {
  return 10 ** (db / 20);
}

function linearToDb(linear: number): number {
  return 20 * Math.log10(linear);
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}

/**
 * Computes one constant playback gain for a decoded TTS chunk.
 *
 * Average speech level is measured only across active 20 ms windows so pauses
 * do not make a chunk appear artificially quiet. The resulting correction is
 * capped by the chunk's highest sample, which keeps boosted audio below the
 * configured peak ceiling without changing the audio within the chunk.
 */
export function calculateTTSLoudnessAdjustment(buffer: PCMBufferLike): TTSLoudnessAdjustment {
  const channelCount = Math.trunc(buffer.numberOfChannels);
  const frameCount = Math.trunc(buffer.length);
  const sampleRate = buffer.sampleRate;
  if (channelCount <= 0 || frameCount <= 0 || !Number.isFinite(sampleRate) || sampleRate <= 0) {
    return { ...UNITY_ADJUSTMENT };
  }

  const channels: Float32Array[] = [];
  for (let channelIndex = 0; channelIndex < channelCount; channelIndex += 1) {
    try {
      channels.push(buffer.getChannelData(channelIndex));
    } catch {
      return { ...UNITY_ADJUSTMENT };
    }
  }

  const windowFrameCount = Math.max(1, Math.round(sampleRate * TTS_LOUDNESS_WINDOW_SECONDS));
  const gateLinear = dbToLinear(TTS_ACTIVE_LOUDNESS_GATE_DBFS);
  let activeSampleCount = 0;
  let activeSumSquares = 0;
  let peak = 0;

  for (let windowStart = 0; windowStart < frameCount; windowStart += windowFrameCount) {
    const windowEnd = Math.min(frameCount, windowStart + windowFrameCount);
    let windowSampleCount = 0;
    let windowSumSquares = 0;

    for (const channel of channels) {
      const channelEnd = Math.min(windowEnd, channel.length);
      for (let frameIndex = windowStart; frameIndex < channelEnd; frameIndex += 1) {
        const sample = channel[frameIndex];
        if (!Number.isFinite(sample)) {
          continue;
        }

        peak = Math.max(peak, Math.abs(sample));
        windowSumSquares += sample * sample;
        windowSampleCount += 1;
      }
    }

    if (windowSampleCount === 0) {
      continue;
    }

    const windowRms = Math.sqrt(windowSumSquares / windowSampleCount);
    if (Number.isFinite(windowRms) && windowRms >= gateLinear) {
      activeSumSquares += windowSumSquares;
      activeSampleCount += windowSampleCount;
    }
  }

  if (activeSampleCount === 0 || peak <= 0) {
    return { ...UNITY_ADJUSTMENT };
  }

  const activeRms = Math.sqrt(activeSumSquares / activeSampleCount);
  if (!Number.isFinite(activeRms) || activeRms <= 0 || !Number.isFinite(peak)) {
    return { ...UNITY_ADJUSTMENT };
  }

  const activeRmsDbfs = linearToDb(activeRms);
  const peakDbfs = linearToDb(peak);
  const targetGainDb = clamp(
    TTS_ACTIVE_LOUDNESS_TARGET_DBFS - activeRmsDbfs,
    TTS_MAX_NORMALIZATION_ATTENUATION_DB,
    TTS_MAX_NORMALIZATION_BOOST_DB
  );
  const peakSafeGainDb = TTS_PEAK_CEILING_DBFS - peakDbfs;
  const gainDb = Math.min(targetGainDb, peakSafeGainDb);

  return {
    activeRmsDbfs,
    gain: dbToLinear(gainDb),
    gainDb,
    peakDbfs,
    peakLimited: gainDb < targetGainDb
  };
}
