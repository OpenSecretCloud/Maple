import {
  TTS_ACTIVE_LOUDNESS_GATE_DBFS,
  TTS_PEAK_CEILING_DBFS,
  type PCMBufferLike
} from "./ttsLoudness";

const ANALYSIS_WINDOW_SECONDS = 0.02;
const ANALYSIS_BIN_SECONDS = 3;
const MIN_ACTIVE_SECONDS_PER_BIN = 0.75;
const MIN_ANALYSIS_BIN_COUNT = 8;
const MIN_ANALYSIS_SPAN_SECONDS = 24;
const MIN_TREND_PAIR_SEPARATION_SECONDS = 6;

const MIN_FITTED_DROP_DB = 3.5;
const MIN_EARLY_LATE_DROP_DB = 3;
const MIN_DECLINING_PAIR_RATIO = 0.68;

const COMPENSATION_FRACTION = 0.8;
export const TTS_MAX_TAPER_CORRECTION_DB = 6;
export const TTS_MAX_TAPER_CORRECTION_RATE_DB_PER_SECOND = 0.1;
const MIN_USEFUL_CORRECTION_DB = 0.5;

interface LoudnessPoint {
  dbfs: number;
  timeSeconds: number;
}

export interface TTSTaperCorrection {
  analysisBinCount: number;
  appliedCorrectionDb: number;
  decliningPairRatio: number | null;
  detectedDropDb: number | null;
  earlyLateDropDb: number | null;
  endGain: number;
  endTimeSeconds: number;
  peakLimited: boolean;
  startGain: number;
  startTimeSeconds: number;
}

function createNoCorrection(
  diagnostics: Partial<
    Pick<
      TTSTaperCorrection,
      "analysisBinCount" | "decliningPairRatio" | "detectedDropDb" | "earlyLateDropDb"
    >
  > = {}
): TTSTaperCorrection {
  return {
    analysisBinCount: diagnostics.analysisBinCount ?? 0,
    appliedCorrectionDb: 0,
    decliningPairRatio: diagnostics.decliningPairRatio ?? null,
    detectedDropDb: diagnostics.detectedDropDb ?? null,
    earlyLateDropDb: diagnostics.earlyLateDropDb ?? null,
    endGain: 1,
    endTimeSeconds: 0,
    peakLimited: false,
    startGain: 1,
    startTimeSeconds: 0
  };
}

function dbToLinear(db: number): number {
  return 10 ** (db / 20);
}

function linearToDb(linear: number): number {
  return 20 * Math.log10(linear);
}

function median(values: number[]): number | null {
  if (values.length === 0) {
    return null;
  }

  values.sort((left, right) => left - right);
  const midpoint = Math.floor(values.length / 2);
  if (values.length % 2 === 1) {
    return values[midpoint];
  }
  return (values[midpoint - 1] + values[midpoint]) / 2;
}

function readChannels(buffer: PCMBufferLike): Float32Array[] | null {
  const channelCount = Math.trunc(buffer.numberOfChannels);
  if (!Number.isSafeInteger(channelCount) || channelCount <= 0) {
    return null;
  }

  const channels: Float32Array[] = [];
  for (let channelIndex = 0; channelIndex < channelCount; channelIndex += 1) {
    try {
      channels.push(buffer.getChannelData(channelIndex));
    } catch {
      return null;
    }
  }
  return channels;
}

function measureLoudnessPoints(
  channels: Float32Array[],
  frameCount: number,
  sampleRate: number,
  playbackGain: number
): LoudnessPoint[] {
  const durationSeconds = frameCount / sampleRate;
  const binCount = Math.max(1, Math.ceil(durationSeconds / ANALYSIS_BIN_SECONDS));
  const binSumSquares = new Float64Array(binCount);
  const binSampleCounts = new Uint32Array(binCount);
  const binActiveFrameCounts = new Uint32Array(binCount);
  const windowFrameCount = Math.max(1, Math.round(sampleRate * ANALYSIS_WINDOW_SECONDS));
  const gateLinear = dbToLinear(TTS_ACTIVE_LOUDNESS_GATE_DBFS);

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
        windowSumSquares += sample * sample;
        windowSampleCount += 1;
      }
    }

    if (windowSampleCount === 0) {
      continue;
    }

    const windowRms = Math.sqrt(windowSumSquares / windowSampleCount);
    if (!Number.isFinite(windowRms) || windowRms * playbackGain < gateLinear) {
      continue;
    }

    const windowMidpointFrame = windowStart + (windowEnd - windowStart) / 2;
    const binIndex = Math.min(
      binCount - 1,
      Math.floor(windowMidpointFrame / sampleRate / ANALYSIS_BIN_SECONDS)
    );
    binSumSquares[binIndex] += windowSumSquares;
    binSampleCounts[binIndex] += windowSampleCount;
    binActiveFrameCounts[binIndex] += windowEnd - windowStart;
  }

  const points: LoudnessPoint[] = [];
  for (let binIndex = 0; binIndex < binCount; binIndex += 1) {
    if (binActiveFrameCounts[binIndex] / sampleRate < MIN_ACTIVE_SECONDS_PER_BIN) {
      continue;
    }

    const sampleCount = binSampleCounts[binIndex];
    if (sampleCount === 0) {
      continue;
    }

    const rms = Math.sqrt(binSumSquares[binIndex] / sampleCount);
    if (!Number.isFinite(rms) || rms <= 0) {
      continue;
    }

    points.push({
      dbfs: linearToDb(rms),
      timeSeconds: Math.min(durationSeconds, (binIndex + 0.5) * ANALYSIS_BIN_SECONDS)
    });
  }

  return points;
}

function analyzeTrend(points: LoudnessPoint[]): {
  decliningPairRatio: number;
  detectedDropDb: number;
  earlyLateDropDb: number;
} | null {
  const slopes: number[] = [];
  let decliningPairCount = 0;

  for (let leftIndex = 0; leftIndex < points.length - 1; leftIndex += 1) {
    for (let rightIndex = leftIndex + 1; rightIndex < points.length; rightIndex += 1) {
      const elapsedSeconds = points[rightIndex].timeSeconds - points[leftIndex].timeSeconds;
      if (elapsedSeconds < MIN_TREND_PAIR_SEPARATION_SECONDS) {
        continue;
      }

      const slope = (points[rightIndex].dbfs - points[leftIndex].dbfs) / elapsedSeconds;
      if (!Number.isFinite(slope)) {
        continue;
      }
      slopes.push(slope);
      if (slope < 0) {
        decliningPairCount += 1;
      }
    }
  }

  const medianSlope = median(slopes);
  if (medianSlope === null || slopes.length === 0) {
    return null;
  }

  const analysisSpanSeconds = points[points.length - 1].timeSeconds - points[0].timeSeconds;
  const regionSize = Math.max(2, Math.floor(points.length / 4));
  const earlyMedian = median(points.slice(0, regionSize).map((point) => point.dbfs));
  const lateMedian = median(points.slice(-regionSize).map((point) => point.dbfs));
  if (earlyMedian === null || lateMedian === null) {
    return null;
  }

  return {
    decliningPairRatio: decliningPairCount / slopes.length,
    detectedDropDb: -medianSlope * analysisSpanSeconds,
    earlyLateDropDb: earlyMedian - lateMedian
  };
}

function correctionPosition(
  timeSeconds: number,
  startTimeSeconds: number,
  endTimeSeconds: number
): number {
  if (timeSeconds <= startTimeSeconds) {
    return -0.5;
  }
  if (timeSeconds >= endTimeSeconds) {
    return 0.5;
  }
  return (timeSeconds - startTimeSeconds) / (endTimeSeconds - startTimeSeconds) - 0.5;
}

function calculatePeakSafeCorrectionDb(
  channels: Float32Array[],
  frameCount: number,
  sampleRate: number,
  baseGain: number,
  startTimeSeconds: number,
  endTimeSeconds: number,
  requestedCorrectionDb: number
): number {
  const baseGainDb = linearToDb(baseGain);
  let safeCorrectionDb = requestedCorrectionDb;

  for (const channel of channels) {
    const channelEnd = Math.min(frameCount, channel.length);
    for (let frameIndex = 0; frameIndex < channelEnd; frameIndex += 1) {
      const sample = channel[frameIndex];
      if (!Number.isFinite(sample) || sample === 0) {
        continue;
      }

      const position = correctionPosition(
        frameIndex / sampleRate,
        startTimeSeconds,
        endTimeSeconds
      );
      if (position <= 0) {
        continue;
      }

      const baseOutputDbfs = linearToDb(Math.abs(sample)) + baseGainDb;
      const headroomDb = TTS_PEAK_CEILING_DBFS - baseOutputDbfs;
      safeCorrectionDb = Math.min(safeCorrectionDb, Math.max(0, headroomDb / position));
      if (safeCorrectionDb === 0) {
        return 0;
      }
    }
  }

  return safeCorrectionDb;
}

/**
 * Detects and partially corrects Voxtral's occasional gradual volume taper.
 *
 * This intentionally is not a compressor or adaptive gain controller. It only
 * returns a single slow, centered dB ramp when long-form active-speech bins
 * show a robust, sustained decline. Amplitude alone cannot distinguish model
 * taper from deliberately quieter delivery, so ambiguous audio remains
 * unchanged and the correction is deliberately capped.
 */
export function calculateTTSTaperCorrection(
  buffer: PCMBufferLike,
  baseGain: number
): TTSTaperCorrection {
  const frameCount = Math.trunc(buffer.length);
  const sampleRate = buffer.sampleRate;
  if (
    !Number.isSafeInteger(frameCount) ||
    frameCount <= 0 ||
    !Number.isFinite(sampleRate) ||
    sampleRate <= 0 ||
    !Number.isFinite(baseGain) ||
    baseGain <= 0
  ) {
    return createNoCorrection();
  }

  const channels = readChannels(buffer);
  if (channels === null) {
    return createNoCorrection();
  }

  const points = measureLoudnessPoints(channels, frameCount, sampleRate, baseGain);
  const analysisBinCount = points.length;
  if (analysisBinCount < MIN_ANALYSIS_BIN_COUNT) {
    return createNoCorrection({ analysisBinCount });
  }

  const startTimeSeconds = points[0].timeSeconds;
  const endTimeSeconds = points[points.length - 1].timeSeconds;
  const analysisSpanSeconds = endTimeSeconds - startTimeSeconds;
  if (analysisSpanSeconds < MIN_ANALYSIS_SPAN_SECONDS) {
    return createNoCorrection({ analysisBinCount });
  }

  const trend = analyzeTrend(points);
  if (trend === null) {
    return createNoCorrection({ analysisBinCount });
  }

  const diagnostics = {
    analysisBinCount,
    decliningPairRatio: trend.decliningPairRatio,
    detectedDropDb: trend.detectedDropDb,
    earlyLateDropDb: trend.earlyLateDropDb
  };
  if (
    trend.detectedDropDb < MIN_FITTED_DROP_DB ||
    trend.earlyLateDropDb < MIN_EARLY_LATE_DROP_DB ||
    trend.decliningPairRatio < MIN_DECLINING_PAIR_RATIO
  ) {
    return createNoCorrection(diagnostics);
  }

  const requestedCorrectionDb = Math.min(
    trend.detectedDropDb * COMPENSATION_FRACTION,
    TTS_MAX_TAPER_CORRECTION_DB,
    analysisSpanSeconds * TTS_MAX_TAPER_CORRECTION_RATE_DB_PER_SECOND
  );
  const peakSafeCorrectionDb = calculatePeakSafeCorrectionDb(
    channels,
    frameCount,
    sampleRate,
    baseGain,
    startTimeSeconds,
    endTimeSeconds,
    requestedCorrectionDb
  );
  const appliedCorrectionDb = Math.max(0, Math.min(requestedCorrectionDb, peakSafeCorrectionDb));
  if (appliedCorrectionDb < MIN_USEFUL_CORRECTION_DB) {
    return {
      ...createNoCorrection(diagnostics),
      peakLimited: peakSafeCorrectionDb < requestedCorrectionDb
    };
  }

  return {
    ...diagnostics,
    appliedCorrectionDb,
    endGain: dbToLinear(appliedCorrectionDb / 2),
    endTimeSeconds,
    peakLimited: peakSafeCorrectionDb < requestedCorrectionDb,
    startGain: dbToLinear(-appliedCorrectionDb / 2),
    startTimeSeconds
  };
}
