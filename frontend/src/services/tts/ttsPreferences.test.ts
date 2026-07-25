import { describe, expect, test } from "bun:test";
import {
  clampTTSPlaybackSpeed,
  DEFAULT_TTS_PLAYBACK_SPEED,
  DEFAULT_VOXTRAL_TTS_VOICE,
  getStoredTTSPlaybackSpeed,
  getStoredTTSVoice,
  rememberTTSPlaybackSpeed,
  rememberTTSVoice,
  resetTTSPlaybackSpeed,
  TTS_MAX_PLAYBACK_SPEED,
  TTS_MIN_PLAYBACK_SPEED,
  TTS_PLAYBACK_SPEED_STEP,
  TTS_PLAYBACK_SPEED_STORAGE_KEY,
  TTS_VOICE_STORAGE_KEY,
  VOXTRAL_TTS_VOICE_OPTIONS,
  VOXTRAL_TTS_VOICES
} from "./ttsPreferences";

class MemoryStorage {
  private readonly values = new Map<string, string>();

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.values.set(key, value);
  }

  removeItem(key: string): void {
    this.values.delete(key);
  }
}

describe("TTS preferences", () => {
  test("uses Casual Female at 1.2x as the product defaults and preserves storage keys", () => {
    const storage = new MemoryStorage();

    expect(DEFAULT_TTS_PLAYBACK_SPEED).toBe(1.2);
    expect(DEFAULT_VOXTRAL_TTS_VOICE).toBe("casual_female");
    expect(TTS_MIN_PLAYBACK_SPEED).toBe(0.5);
    expect(TTS_MAX_PLAYBACK_SPEED).toBe(2);
    expect(TTS_PLAYBACK_SPEED_STEP).toBe(0.1);
    expect(TTS_PLAYBACK_SPEED_STORAGE_KEY).toBe("ttsPlaybackSpeed");
    expect(TTS_VOICE_STORAGE_KEY).toBe("ttsVoice");
    expect(getStoredTTSPlaybackSpeed(storage)).toBe(1.2);
    expect(getStoredTTSVoice(storage)).toBe("casual_female");
  });

  test("preserves previously selected values when the product defaults change", () => {
    const storage = new MemoryStorage();
    storage.setItem(TTS_PLAYBACK_SPEED_STORAGE_KEY, "1.2");
    storage.setItem(TTS_VOICE_STORAGE_KEY, "neutral_female");

    expect(getStoredTTSPlaybackSpeed(storage)).toBe(1.2);
    expect(getStoredTTSVoice(storage)).toBe("neutral_female");
  });

  test("clamps, rounds, persists, and resets playback speed", () => {
    const storage = new MemoryStorage();

    expect(clampTTSPlaybackSpeed(0.1)).toBe(TTS_MIN_PLAYBACK_SPEED);
    expect(clampTTSPlaybackSpeed(2.4)).toBe(TTS_MAX_PLAYBACK_SPEED);
    expect(rememberTTSPlaybackSpeed(1.26, storage)).toBe(1.3);
    expect(getStoredTTSPlaybackSpeed(storage)).toBe(1.3);

    resetTTSPlaybackSpeed(storage);
    expect(storage.getItem(TTS_PLAYBACK_SPEED_STORAGE_KEY)).toBeNull();
    expect(getStoredTTSPlaybackSpeed(storage)).toBe(DEFAULT_TTS_PLAYBACK_SPEED);
  });

  test("offers every provider voice and persists a valid selection", () => {
    const storage = new MemoryStorage();

    expect(VOXTRAL_TTS_VOICES).toHaveLength(20);
    expect(new Set(VOXTRAL_TTS_VOICES).size).toBe(20);
    expect(VOXTRAL_TTS_VOICE_OPTIONS.map(({ value }) => value).sort()).toEqual(
      [...VOXTRAL_TTS_VOICES].sort()
    );
    expect(getStoredTTSVoice(storage)).toBe(DEFAULT_VOXTRAL_TTS_VOICE);

    rememberTTSVoice("fr_female", storage);
    expect(getStoredTTSVoice(storage)).toBe("fr_female");
  });

  test("falls back from invalid or unavailable stored preferences", () => {
    const storage = new MemoryStorage();
    storage.setItem(TTS_PLAYBACK_SPEED_STORAGE_KEY, "not-a-number");
    storage.setItem(TTS_VOICE_STORAGE_KEY, "not-a-voice");

    expect(getStoredTTSPlaybackSpeed(storage)).toBe(DEFAULT_TTS_PLAYBACK_SPEED);
    expect(getStoredTTSVoice(storage)).toBe(DEFAULT_VOXTRAL_TTS_VOICE);

    const unavailableStorage = {
      getItem: () => {
        throw new Error("unavailable");
      },
      setItem: () => {
        throw new Error("unavailable");
      },
      removeItem: () => {
        throw new Error("unavailable");
      }
    };
    expect(getStoredTTSPlaybackSpeed(unavailableStorage)).toBe(DEFAULT_TTS_PLAYBACK_SPEED);
    expect(getStoredTTSVoice(unavailableStorage)).toBe(DEFAULT_VOXTRAL_TTS_VOICE);
    expect(() => rememberTTSPlaybackSpeed(1.4, unavailableStorage)).not.toThrow();
    expect(() => rememberTTSVoice("neutral_male", unavailableStorage)).not.toThrow();
    expect(() => resetTTSPlaybackSpeed(unavailableStorage)).not.toThrow();
  });
});
