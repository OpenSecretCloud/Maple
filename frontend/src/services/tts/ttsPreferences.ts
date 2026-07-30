export const TTS_MIN_PLAYBACK_SPEED = 0.5;
export const TTS_MAX_PLAYBACK_SPEED = 2;
export const TTS_PLAYBACK_SPEED_STEP = 0.1;
export const DEFAULT_TTS_PLAYBACK_SPEED = 1.2;
export const TTS_PLAYBACK_SPEED_STORAGE_KEY = "ttsPlaybackSpeed";
export const TTS_VOICE_STORAGE_KEY = "ttsVoice";

export const VOXTRAL_TTS_VOICES = [
  "neutral_female",
  "neutral_male",
  "casual_female",
  "casual_male",
  "cheerful_female",
  "ar_male",
  "de_female",
  "de_male",
  "es_female",
  "es_male",
  "fr_female",
  "fr_male",
  "hi_female",
  "hi_male",
  "it_female",
  "it_male",
  "nl_female",
  "nl_male",
  "pt_female",
  "pt_male"
] as const;

export type VoxtralTTSVoice = (typeof VOXTRAL_TTS_VOICES)[number];

export const DEFAULT_VOXTRAL_TTS_VOICE: VoxtralTTSVoice = "casual_female";

export const VOXTRAL_TTS_VOICE_OPTIONS: ReadonlyArray<{
  value: VoxtralTTSVoice;
  label: string;
  group: "Default voices" | "Reference accents";
}> = [
  { value: "neutral_female", label: "Neutral — Female", group: "Default voices" },
  { value: "neutral_male", label: "Neutral — Male", group: "Default voices" },
  { value: "casual_female", label: "Casual — Female", group: "Default voices" },
  { value: "casual_male", label: "Casual — Male", group: "Default voices" },
  { value: "cheerful_female", label: "Cheerful — Female", group: "Default voices" },
  { value: "ar_male", label: "Arabic-accented — Male", group: "Reference accents" },
  { value: "de_female", label: "German-accented — Female", group: "Reference accents" },
  { value: "de_male", label: "German-accented — Male", group: "Reference accents" },
  { value: "es_female", label: "Spanish-accented — Female", group: "Reference accents" },
  { value: "es_male", label: "Spanish-accented — Male", group: "Reference accents" },
  { value: "fr_female", label: "French-accented — Female", group: "Reference accents" },
  { value: "fr_male", label: "French-accented — Male", group: "Reference accents" },
  { value: "hi_female", label: "Hindi-accented — Female", group: "Reference accents" },
  { value: "hi_male", label: "Hindi-accented — Male", group: "Reference accents" },
  { value: "it_female", label: "Italian-accented — Female", group: "Reference accents" },
  { value: "it_male", label: "Italian-accented — Male", group: "Reference accents" },
  { value: "nl_female", label: "Dutch-accented — Female", group: "Reference accents" },
  { value: "nl_male", label: "Dutch-accented — Male", group: "Reference accents" },
  {
    value: "pt_female",
    label: "Portuguese-accented — Female",
    group: "Reference accents"
  },
  { value: "pt_male", label: "Portuguese-accented — Male", group: "Reference accents" }
];

type TTSPreferenceStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

function getBrowserStorage(): TTSPreferenceStorage | null {
  if (typeof window === "undefined") return null;

  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function clampTTSPlaybackSpeed(speed: number): number {
  if (!Number.isFinite(speed)) {
    return DEFAULT_TTS_PLAYBACK_SPEED;
  }

  const clamped = Math.min(TTS_MAX_PLAYBACK_SPEED, Math.max(TTS_MIN_PLAYBACK_SPEED, speed));
  return Number(
    (Math.round(clamped / TTS_PLAYBACK_SPEED_STEP) * TTS_PLAYBACK_SPEED_STEP).toFixed(1)
  );
}

export function getStoredTTSPlaybackSpeed(
  storage: TTSPreferenceStorage | null = getBrowserStorage()
): number {
  if (!storage) return DEFAULT_TTS_PLAYBACK_SPEED;

  try {
    const stored = storage.getItem(TTS_PLAYBACK_SPEED_STORAGE_KEY);
    return stored === null || stored.trim() === ""
      ? DEFAULT_TTS_PLAYBACK_SPEED
      : clampTTSPlaybackSpeed(Number(stored));
  } catch {
    return DEFAULT_TTS_PLAYBACK_SPEED;
  }
}

export function rememberTTSPlaybackSpeed(
  speed: number,
  storage: TTSPreferenceStorage | null = getBrowserStorage()
): number {
  const clamped = clampTTSPlaybackSpeed(speed);
  if (!storage) return clamped;

  try {
    storage.setItem(TTS_PLAYBACK_SPEED_STORAGE_KEY, String(clamped));
  } catch {
    // A storage failure should not prevent changing the in-memory preference.
  }
  return clamped;
}

export function resetTTSPlaybackSpeed(
  storage: TTSPreferenceStorage | null = getBrowserStorage()
): void {
  if (!storage) return;

  try {
    storage.removeItem(TTS_PLAYBACK_SPEED_STORAGE_KEY);
  } catch {
    // A storage failure should not prevent resetting the in-memory preference.
  }
}

export function isVoxtralTTSVoice(value: string): value is VoxtralTTSVoice {
  return (VOXTRAL_TTS_VOICES as readonly string[]).includes(value);
}

export function getStoredTTSVoice(
  storage: TTSPreferenceStorage | null = getBrowserStorage()
): VoxtralTTSVoice {
  if (!storage) return DEFAULT_VOXTRAL_TTS_VOICE;

  try {
    const stored = storage.getItem(TTS_VOICE_STORAGE_KEY);
    return stored !== null && isVoxtralTTSVoice(stored) ? stored : DEFAULT_VOXTRAL_TTS_VOICE;
  } catch {
    return DEFAULT_VOXTRAL_TTS_VOICE;
  }
}

export function rememberTTSVoice(
  voice: VoxtralTTSVoice,
  storage: TTSPreferenceStorage | null = getBrowserStorage()
): void {
  if (!storage) return;

  try {
    storage.setItem(TTS_VOICE_STORAGE_KEY, voice);
  } catch {
    // A storage failure should not prevent changing the in-memory preference.
  }
}
