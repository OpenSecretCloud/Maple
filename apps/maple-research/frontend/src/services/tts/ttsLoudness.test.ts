import { describe, expect, test } from "bun:test";
import {
  calculateTTSLoudnessAdjustment,
  TTS_ACTIVE_LOUDNESS_TARGET_DBFS,
  TTS_MAX_NORMALIZATION_BOOST_DB,
  TTS_PEAK_CEILING_DBFS,
  type PCMBufferLike
} from "./ttsLoudness";

const SAMPLE_RATE = 24_000;
const WINDOW_FRAME_COUNT = 480;

function dbToLinear(db: number): number {
  return 10 ** (db / 20);
}

function createBuffer(...channels: number[][]): PCMBufferLike {
  const data = channels.map((channel) => Float32Array.from(channel));
  return {
    length: Math.max(0, ...data.map((channel) => channel.length)),
    numberOfChannels: data.length,
    sampleRate: SAMPLE_RATE,
    getChannelData: (channel) => data[channel]
  };
}

function constantWindows(dbfs: number, windowCount = 2): number[] {
  const amplitude = dbToLinear(dbfs);
  return Array.from({ length: WINDOW_FRAME_COUNT * windowCount }, (_, index) =>
    index % 2 === 0 ? amplitude : -amplitude
  );
}

describe("calculateTTSLoudnessAdjustment", () => {
  test("raises quiet speech and lowers loud speech toward one average target", () => {
    const quiet = calculateTTSLoudnessAdjustment(createBuffer(constantWindows(-35)));
    const loud = calculateTTSLoudnessAdjustment(createBuffer(constantWindows(-10)));

    expect(quiet.gainDb).toBeCloseTo(14, 4);
    expect(loud.gainDb).toBeCloseTo(-11, 4);
    expect((quiet.activeRmsDbfs ?? 0) + quiet.gainDb).toBeCloseTo(
      TTS_ACTIVE_LOUDNESS_TARGET_DBFS,
      4
    );
    expect((loud.activeRmsDbfs ?? 0) + loud.gainDb).toBeCloseTo(TTS_ACTIVE_LOUDNESS_TARGET_DBFS, 4);
  });

  test("does not let a requested boost exceed the peak ceiling", () => {
    const samples = constantWindows(-35, 2);
    samples[0] = 1;
    const adjustment = calculateTTSLoudnessAdjustment(createBuffer(samples));

    expect(adjustment.peakLimited).toBe(true);
    expect(adjustment.gainDb).toBeCloseTo(TTS_PEAK_CEILING_DBFS, 4);
    expect((adjustment.peakDbfs ?? 0) + adjustment.gainDb).toBeLessThanOrEqual(
      TTS_PEAK_CEILING_DBFS
    );
  });

  test("ignores silent windows when measuring average speech level", () => {
    const activeSpeech = constantWindows(-30, 2);
    const withSilence = [
      ...Array<number>(WINDOW_FRAME_COUNT).fill(0),
      ...activeSpeech,
      ...Array<number>(WINDOW_FRAME_COUNT).fill(0)
    ];

    const activeAdjustment = calculateTTSLoudnessAdjustment(createBuffer(activeSpeech));
    const paddedAdjustment = calculateTTSLoudnessAdjustment(createBuffer(withSilence));

    expect(paddedAdjustment.activeRmsDbfs).toBeCloseTo(activeAdjustment.activeRmsDbfs ?? 0, 4);
    expect(paddedAdjustment.gainDb).toBeCloseTo(activeAdjustment.gainDb, 4);
  });

  test("leaves silence and below-gate noise unchanged", () => {
    const silence = calculateTTSLoudnessAdjustment(
      createBuffer(Array<number>(WINDOW_FRAME_COUNT * 2).fill(0))
    );
    const belowGate = calculateTTSLoudnessAdjustment(createBuffer(constantWindows(-60)));

    expect(silence).toEqual({
      activeRmsDbfs: null,
      gain: 1,
      gainDb: 0,
      peakDbfs: null,
      peakLimited: false
    });
    expect(belowGate.gain).toBe(1);
    expect(belowGate.gainDb).toBe(0);
  });

  test("caps extreme boosts even when the peak ceiling has ample headroom", () => {
    const adjustment = calculateTTSLoudnessAdjustment(createBuffer(constantWindows(-49)));

    expect(adjustment.gainDb).toBe(TTS_MAX_NORMALIZATION_BOOST_DB);
    expect(adjustment.peakLimited).toBe(false);
  });

  test("uses one gain for every channel and preserves their relative balance", () => {
    const left = constantWindows(-30);
    const right = left.map((sample) => sample / 2);
    const adjustment = calculateTTSLoudnessAdjustment(createBuffer(left, right));

    const outputLeft = Math.abs(left[0] * adjustment.gain);
    const outputRight = Math.abs(right[0] * adjustment.gain);
    expect(outputLeft / outputRight).toBeCloseTo(2, 8);
  });

  test("never returns a non-finite gain for invalid samples or empty buffers", () => {
    const invalid = calculateTTSLoudnessAdjustment(
      createBuffer([Number.NaN, Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY])
    );
    const empty = calculateTTSLoudnessAdjustment(createBuffer([]));

    expect(Number.isFinite(invalid.gain)).toBe(true);
    expect(Number.isFinite(invalid.gainDb)).toBe(true);
    expect(Number.isFinite(empty.gain)).toBe(true);
    expect(Number.isFinite(empty.gainDb)).toBe(true);
  });
});
