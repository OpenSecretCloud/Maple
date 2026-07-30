import type { TTSTaperCorrection } from "./ttsTaperCorrection";

export interface TaperGainParamLike {
  exponentialRampToValueAtTime: (value: number, endTime: number) => unknown;
  setValueAtTime: (value: number, startTime: number) => unknown;
}

/** Schedules the experiment's one monotonic, linear-in-dB gain ramp. */
export function scheduleTTSTaperCorrection(
  gain: TaperGainParamLike,
  correction: TTSTaperCorrection,
  playbackStartTime: number,
  durationSeconds: number
): boolean {
  if (
    correction.appliedCorrectionDb <= 0 ||
    !Number.isFinite(playbackStartTime) ||
    !Number.isFinite(durationSeconds) ||
    durationSeconds <= 0 ||
    !Number.isFinite(correction.startGain) ||
    correction.startGain <= 0 ||
    !Number.isFinite(correction.endGain) ||
    correction.endGain <= 0 ||
    !Number.isFinite(correction.startTimeSeconds) ||
    !Number.isFinite(correction.endTimeSeconds)
  ) {
    return false;
  }

  const rampStartOffset = Math.min(durationSeconds, Math.max(0, correction.startTimeSeconds));
  const rampEndOffset = Math.min(
    durationSeconds,
    Math.max(rampStartOffset, correction.endTimeSeconds)
  );
  if (rampEndOffset <= rampStartOffset) {
    return false;
  }

  gain.setValueAtTime(correction.startGain, playbackStartTime);
  if (rampStartOffset > 0) {
    gain.setValueAtTime(correction.startGain, playbackStartTime + rampStartOffset);
  }
  gain.exponentialRampToValueAtTime(correction.endGain, playbackStartTime + rampEndOffset);
  if (rampEndOffset < durationSeconds) {
    gain.setValueAtTime(correction.endGain, playbackStartTime + durationSeconds);
  }
  return true;
}
