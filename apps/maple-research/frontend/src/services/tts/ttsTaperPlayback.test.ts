import { describe, expect, test } from "bun:test";
import { scheduleTTSTaperCorrection, type TaperGainParamLike } from "./ttsTaperPlayback";
import type { TTSTaperCorrection } from "./ttsTaperCorrection";

function correction(overrides: Partial<TTSTaperCorrection> = {}): TTSTaperCorrection {
  return {
    analysisBinCount: 30,
    appliedCorrectionDb: 4,
    decliningPairRatio: 0.9,
    detectedDropDb: 5,
    earlyLateDropDb: 4,
    endGain: 1.2589254118,
    endTimeSeconds: 88.5,
    peakLimited: false,
    startGain: 0.7943282347,
    startTimeSeconds: 1.5,
    ...overrides
  };
}

function recordingGainParam() {
  const calls: Array<{ kind: "set" | "ramp"; time: number; value: number }> = [];
  const gain: TaperGainParamLike = {
    setValueAtTime: (value, time) => calls.push({ kind: "set", time, value }),
    exponentialRampToValueAtTime: (value, time) => calls.push({ kind: "ramp", time, value })
  };
  return { calls, gain };
}

describe("scheduleTTSTaperCorrection", () => {
  test("schedules one future exponential ramp on the chunk timeline", () => {
    const { calls, gain } = recordingGainParam();

    expect(scheduleTTSTaperCorrection(gain, correction(), 12, 90)).toBe(true);
    expect(calls).toEqual([
      { kind: "set", time: 12, value: 0.7943282347 },
      { kind: "set", time: 13.5, value: 0.7943282347 },
      { kind: "ramp", time: 100.5, value: 1.2589254118 },
      { kind: "set", time: 102, value: 1.2589254118 }
    ]);
  });

  test("does nothing when the detector did not activate", () => {
    const { calls, gain } = recordingGainParam();

    expect(
      scheduleTTSTaperCorrection(
        gain,
        correction({ appliedCorrectionDb: 0, startGain: 1, endGain: 1 }),
        12,
        90
      )
    ).toBe(false);
    expect(calls).toEqual([]);
  });

  test("clamps analysis offsets to the decoded buffer duration", () => {
    const { calls, gain } = recordingGainParam();

    expect(
      scheduleTTSTaperCorrection(
        gain,
        correction({ startTimeSeconds: -2, endTimeSeconds: 120 }),
        4,
        90
      )
    ).toBe(true);
    expect(calls).toEqual([
      { kind: "set", time: 4, value: 0.7943282347 },
      { kind: "ramp", time: 94, value: 1.2589254118 }
    ]);
  });

  test("rejects non-finite detector offsets without scheduling automation", () => {
    const { calls, gain } = recordingGainParam();

    expect(
      scheduleTTSTaperCorrection(gain, correction({ startTimeSeconds: Number.NaN }), 4, 90)
    ).toBe(false);
    expect(calls).toEqual([]);
  });
});
