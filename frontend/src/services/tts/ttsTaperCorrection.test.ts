import { describe, expect, test } from "bun:test";
import { TTS_PEAK_CEILING_DBFS, type PCMBufferLike } from "./ttsLoudness";
import {
  calculateTTSTaperCorrection,
  TTS_MAX_TAPER_CORRECTION_DB,
  TTS_MAX_TAPER_CORRECTION_RATE_DB_PER_SECOND,
  type TTSTaperCorrection
} from "./ttsTaperCorrection";

const SAMPLE_RATE = 1_000;

function dbToLinear(db: number): number {
  return 10 ** (db / 20);
}

function linearToDb(linear: number): number {
  return 20 * Math.log10(linear);
}

function createBuffer(...channels: Float32Array[]): PCMBufferLike {
  return {
    length: Math.max(0, ...channels.map((channel) => channel.length)),
    numberOfChannels: channels.length,
    sampleRate: SAMPLE_RATE,
    getChannelData: (channel) => channels[channel]
  };
}

function createEnvelope(
  durationSeconds: number,
  dbAtTime: (timeSeconds: number) => number,
  isSilent: (timeSeconds: number) => boolean = () => false
): Float32Array {
  const frameCount = Math.round(durationSeconds * SAMPLE_RATE);
  const samples = new Float32Array(frameCount);
  for (let frameIndex = 0; frameIndex < frameCount; frameIndex += 1) {
    const timeSeconds = frameIndex / SAMPLE_RATE;
    if (!isSilent(timeSeconds)) {
      const amplitude = dbToLinear(dbAtTime(timeSeconds));
      samples[frameIndex] = frameIndex % 2 === 0 ? amplitude : -amplitude;
    }
  }
  return samples;
}

function correctionGainAtTime(correction: TTSTaperCorrection, timeSeconds: number): number {
  if (correction.appliedCorrectionDb === 0 || timeSeconds <= correction.startTimeSeconds) {
    return correction.startGain;
  }
  if (timeSeconds >= correction.endTimeSeconds) {
    return correction.endGain;
  }

  const progress =
    (timeSeconds - correction.startTimeSeconds) /
    (correction.endTimeSeconds - correction.startTimeSeconds);
  return correction.startGain * (correction.endGain / correction.startGain) ** progress;
}

function maximumCorrectedPeakDbfs(
  samples: Float32Array,
  correction: TTSTaperCorrection,
  baseGain = 1
): number {
  let peak = 0;
  for (let frameIndex = 0; frameIndex < samples.length; frameIndex += 1) {
    const gain = correctionGainAtTime(correction, frameIndex / SAMPLE_RATE);
    peak = Math.max(peak, Math.abs(samples[frameIndex] * baseGain * gain));
  }
  return peak > 0 ? linearToDb(peak) : Number.NEGATIVE_INFINITY;
}

function seededRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (Math.imul(state, 1_664_525) + 1_013_904_223) >>> 0;
    return state / 0x1_0000_0000;
  };
}

describe("calculateTTSTaperCorrection", () => {
  test("leaves flat, rising, and short clips unchanged", () => {
    const flat = calculateTTSTaperCorrection(createBuffer(createEnvelope(90, () => -24)), 1);
    const rising = calculateTTSTaperCorrection(
      createBuffer(createEnvelope(90, (time) => -28 + (8 * time) / 90)),
      1
    );
    const shortFade = calculateTTSTaperCorrection(
      createBuffer(createEnvelope(20, (time) => -20 - (8 * time) / 20)),
      1
    );

    expect(flat.appliedCorrectionDb).toBe(0);
    expect(flat.startGain).toBe(1);
    expect(flat.endGain).toBe(1);
    expect(rising.appliedCorrectionDb).toBe(0);
    expect(shortFade.appliedCorrectionDb).toBe(0);
  });

  test("does not mistake recurring dynamics or an abrupt level step for a gradual taper", () => {
    const recurring = calculateTTSTaperCorrection(
      createBuffer(
        createEnvelope(96, (time) => {
          const offsets = [2, -2, 1, -1];
          return -24 + offsets[Math.floor(time / 3) % offsets.length];
        })
      ),
      1
    );
    const stepDown = calculateTTSTaperCorrection(
      createBuffer(createEnvelope(90, (time) => (time < 45 ? -21 : -27))),
      1
    );

    expect(recurring.appliedCorrectionDb).toBe(0);
    expect(stepDown.appliedCorrectionDb).toBe(0);
  });

  test("partially reverses a clear long linear fade with one centered ramp", () => {
    const correction = calculateTTSTaperCorrection(
      createBuffer(createEnvelope(90, (time) => -20 - (8 * time) / 90)),
      1
    );

    expect(correction.detectedDropDb).toBeCloseTo(7.73, 1);
    expect(correction.earlyLateDropDb).toBeGreaterThan(5);
    expect(correction.decliningPairRatio).toBeGreaterThan(0.99);
    expect(correction.appliedCorrectionDb).toBe(TTS_MAX_TAPER_CORRECTION_DB);
    expect(linearToDb(correction.endGain / correction.startGain)).toBeCloseTo(
      correction.appliedCorrectionDb,
      6
    );
    expect(linearToDb(correction.startGain)).toBeCloseTo(-correction.appliedCorrectionDb / 2, 6);
    expect(linearToDb(correction.endGain)).toBeCloseTo(correction.appliedCorrectionDb / 2, 6);
  });

  test("limits correction by both the configured total and rate caps", () => {
    const fastFade = calculateTTSTaperCorrection(
      createBuffer(createEnvelope(30, (time) => -20 - (12 * time) / 30)),
      1
    );
    const longFade = calculateTTSTaperCorrection(
      createBuffer(createEnvelope(120, (time) => -20 - (14 * time) / 120)),
      1
    );

    const fastSpan = fastFade.endTimeSeconds - fastFade.startTimeSeconds;
    expect(fastFade.appliedCorrectionDb).toBeLessThanOrEqual(
      fastSpan * TTS_MAX_TAPER_CORRECTION_RATE_DB_PER_SECOND + 1e-8
    );
    expect(longFade.appliedCorrectionDb).toBe(TTS_MAX_TAPER_CORRECTION_DB);
  });

  test("ignores pauses while keeping the correction aligned with active speech", () => {
    const fade = (time: number) => -20 - (8 * time) / 96;
    const silence = (time: number) => time % 6 >= 4.5;
    const correction = calculateTTSTaperCorrection(
      createBuffer(createEnvelope(96, fade, silence)),
      1
    );

    expect(correction.appliedCorrectionDb).toBeGreaterThan(4);
    expect(correction.startTimeSeconds).toBeGreaterThan(0);
    expect(correction.endTimeSeconds).toBeLessThan(96);
    expect(correction.decliningPairRatio).toBeGreaterThan(0.95);
  });

  test("measures activity at the normalized playback level for quiet provider audio", () => {
    const quietFade = createEnvelope(90, (time) => -48 - (10 * time) / 90);
    const withoutNormalization = calculateTTSTaperCorrection(createBuffer(quietFade), 1);
    const withNormalization = calculateTTSTaperCorrection(createBuffer(quietFade), dbToLinear(18));

    expect(withoutNormalization.appliedCorrectionDb).toBe(0);
    expect(withNormalization.analysisBinCount).toBeGreaterThan(25);
    expect(withNormalization.appliedCorrectionDb).toBeGreaterThan(5);
  });

  test("caps a late boost against the sample peak ceiling", () => {
    const samples = createEnvelope(90, (time) => -18 - (10 * time) / 90);
    samples[samples.length - 1] = dbToLinear(-2.5);
    const correction = calculateTTSTaperCorrection(createBuffer(samples), 1);

    expect(correction.peakLimited).toBe(true);
    expect(correction.appliedCorrectionDb).toBeCloseTo(3, 2);
    expect(maximumCorrectedPeakDbfs(samples, correction)).toBeLessThanOrEqual(
      TTS_PEAK_CEILING_DBFS + 1e-5
    );
  });

  test("an early full-scale transient does not suppress a safe late correction", () => {
    const samples = createEnvelope(90, (time) => -18 - (8 * time) / 90);
    samples[0] = dbToLinear(TTS_PEAK_CEILING_DBFS);
    const correction = calculateTTSTaperCorrection(createBuffer(samples), 1);

    expect(correction.appliedCorrectionDb).toBeGreaterThan(5);
    expect(correction.peakLimited).toBe(false);
    expect(maximumCorrectedPeakDbfs(samples, correction)).toBeLessThanOrEqual(
      TTS_PEAK_CEILING_DBFS + 1e-5
    );
  });

  test("uses one shared correction for stereo energy without phase cancellation", () => {
    const left = createEnvelope(90, (time) => -20 - (8 * time) / 90);
    const right = Float32Array.from(left, (sample) => -sample);
    const correction = calculateTTSTaperCorrection(createBuffer(left, right), 1);

    expect(correction.appliedCorrectionDb).toBeGreaterThan(5);
    const time = 72;
    const gain = correctionGainAtTime(correction, time);
    const frame = time * SAMPLE_RATE;
    expect((left[frame] * gain) / (right[frame] * gain)).toBeCloseTo(-1, 8);
  });

  test("fails closed to unity for invalid metadata, channel access, or gain", () => {
    const samples = createEnvelope(90, (time) => -20 - (8 * time) / 90);
    const invalidRate = { ...createBuffer(samples), sampleRate: Number.NaN };
    const invalidLength = { ...createBuffer(samples), length: Number.POSITIVE_INFINITY };
    const invalidChannelCount = {
      ...createBuffer(samples),
      numberOfChannels: Number.POSITIVE_INFINITY
    };
    const throwing: PCMBufferLike = {
      length: samples.length,
      numberOfChannels: 1,
      sampleRate: SAMPLE_RATE,
      getChannelData: () => {
        throw new Error("unavailable");
      }
    };

    for (const correction of [
      calculateTTSTaperCorrection(invalidRate, 1),
      calculateTTSTaperCorrection(invalidLength, 1),
      calculateTTSTaperCorrection(invalidChannelCount, 1),
      calculateTTSTaperCorrection(throwing, 1),
      calculateTTSTaperCorrection(createBuffer(samples), Number.POSITIVE_INFINITY),
      calculateTTSTaperCorrection(createBuffer(samples), 0)
    ]) {
      expect(correction.appliedCorrectionDb).toBe(0);
      expect(correction.startGain).toBe(1);
      expect(correction.endGain).toBe(1);
    }
  });

  test("remains finite when channel data contains invalid samples", () => {
    const samples = createEnvelope(90, (time) => -20 - (8 * time) / 90);
    samples[100] = Number.NaN;
    samples[10_000] = Number.POSITIVE_INFINITY;
    samples[50_000] = Number.NEGATIVE_INFINITY;
    const correction = calculateTTSTaperCorrection(createBuffer(samples), 1);

    expect(Number.isFinite(correction.appliedCorrectionDb)).toBe(true);
    expect(Number.isFinite(correction.startGain)).toBe(true);
    expect(Number.isFinite(correction.endGain)).toBe(true);
    expect(correction.appliedCorrectionDb).toBeGreaterThan(5);
  });

  test("rejects many zero-trend dynamics while detecting many genuine fades", () => {
    let falseActivations = 0;
    let detectedFades = 0;

    for (let batch = 0; batch < 100; batch += 1) {
      const random = seededRandom(batch + 1);
      const offsets = Array.from({ length: 30 }, () => (random() - 0.5) * 5);
      const noTrend = createEnvelope(
        90,
        (time) => -24 + offsets[Math.min(offsets.length - 1, Math.floor(time / 3))]
      );
      if (calculateTTSTaperCorrection(createBuffer(noTrend), 1).appliedCorrectionDb > 0) {
        falseActivations += 1;
      }

      const fadeRandom = seededRandom(batch + 10_001);
      const fadeOffsets = Array.from({ length: 30 }, () => (fadeRandom() - 0.5) * 3);
      const fade = createEnvelope(90, (time) => {
        const local = fadeOffsets[Math.min(fadeOffsets.length - 1, Math.floor(time / 3))];
        return -20 - (8 * time) / 90 + local;
      });
      if (calculateTTSTaperCorrection(createBuffer(fade), 1).appliedCorrectionDb > 0) {
        detectedFades += 1;
      }
    }

    expect(falseActivations).toBe(0);
    expect(detectedFades).toBeGreaterThanOrEqual(95);
  });
});
