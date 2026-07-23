import {
  createContext,
  useContext,
  useState,
  useCallback,
  useRef,
  useEffect,
  ReactNode
} from "react";
import { isTauriDesktop, isIOS, isTauri } from "@/utils/platform";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { prepareAndScheduleTTSChunks } from "./ttsPlayback";

export type TTSStatus =
  | "not_available"
  | "checking"
  | "not_downloaded"
  | "upgrade_available"
  | "downloading"
  | "loading"
  | "ready"
  | "deleting"
  | "error";

export type TTSModelVersion = "supertonic3" | "legacy";

interface TTSStatusResponse {
  models_downloaded: boolean;
  models_loaded: boolean;
  models_present_but_incompatible: boolean;
  upgrade_available: boolean;
  model_version: TTSModelVersion | null;
  total_size_mb: number;
}

interface TTSSynthesizeResponse {
  audio_base64: string;
  sample_rate: number;
  duration_seconds: number;
  skipped: boolean;
}

interface TTSChunkTextResponse {
  chunks: string[];
}

interface DownloadProgress {
  downloaded: number;
  total: number;
  file_name: string;
  percent: number;
}

interface DecodedTTSChunk {
  audioBuffer: AudioBuffer;
  chunkIndex: number;
}

interface AudioSessionLike {
  type: string;
}

type NavigatorWithAudioSession = Navigator & {
  audioSession?: AudioSessionLike;
};

type WindowWithWebkitAudioContext = Window &
  typeof globalThis & {
    webkitAudioContext?: typeof AudioContext;
  };

export const TTS_MIN_PLAYBACK_SPEED = 0.5;
export const TTS_MAX_PLAYBACK_SPEED = 2.0;
export const TTS_PLAYBACK_SPEED_STEP = 0.1;
export const TTS_LANGUAGE_OPTIONS = [
  { code: "na", label: "Auto" },
  { code: "ar", label: "Arabic" },
  { code: "bg", label: "Bulgarian" },
  { code: "hr", label: "Croatian" },
  { code: "cs", label: "Czech" },
  { code: "da", label: "Danish" },
  { code: "nl", label: "Dutch" },
  { code: "en", label: "English" },
  { code: "et", label: "Estonian" },
  { code: "fi", label: "Finnish" },
  { code: "fr", label: "French" },
  { code: "de", label: "German" },
  { code: "el", label: "Greek" },
  { code: "hi", label: "Hindi" },
  { code: "hu", label: "Hungarian" },
  { code: "id", label: "Indonesian" },
  { code: "it", label: "Italian" },
  { code: "ja", label: "Japanese" },
  { code: "ko", label: "Korean" },
  { code: "lv", label: "Latvian" },
  { code: "lt", label: "Lithuanian" },
  { code: "pl", label: "Polish" },
  { code: "pt", label: "Portuguese" },
  { code: "ro", label: "Romanian" },
  { code: "ru", label: "Russian" },
  { code: "sk", label: "Slovak" },
  { code: "sl", label: "Slovenian" },
  { code: "es", label: "Spanish" },
  { code: "sv", label: "Swedish" },
  { code: "tr", label: "Turkish" },
  { code: "uk", label: "Ukrainian" },
  { code: "vi", label: "Vietnamese" }
] as const;

export type TTSLanguage = (typeof TTS_LANGUAGE_OPTIONS)[number]["code"];

const SUPERTONIC3_DEFAULT_PLAYBACK_SPEED = 1.2;
const LEGACY_DEFAULT_PLAYBACK_SPEED = 1.2;
const TTS_PLAYBACK_SPEED_STORAGE_KEY = "ttsPlaybackSpeed";
const TTS_LANGUAGE_STORAGE_KEY = "ttsLanguage";
const DEFAULT_TTS_LANGUAGE: TTSLanguage = "na";

interface TTSContextValue {
  status: TTSStatus;
  error: string | null;
  playbackError: string | null;
  downloadProgress: number;
  downloadDetail: string;
  totalSizeMB: number;
  upgradeAvailable: boolean;
  modelsPresentButIncompatible: boolean;
  modelVersion: TTSModelVersion | null;
  isPreparing: boolean;
  isPlaying: boolean;
  currentPlayingId: string | null;
  playbackSpeed: number;
  hasCustomPlaybackSpeed: boolean;
  ttsLanguage: TTSLanguage;
  isTauriEnv: boolean;

  checkStatus: () => Promise<void>;
  startDownload: () => Promise<boolean>;
  deleteModels: () => Promise<boolean>;
  speak: (text: string, messageId: string) => Promise<void>;
  stop: () => void;
  setPlaybackSpeed: (speed: number) => void;
  resetPlaybackSpeed: () => void;
  setTTSLanguage: (language: TTSLanguage) => void;
  clearPlaybackError: () => void;
}

const TTSContext = createContext<TTSContextValue | null>(null);

function errorMessage(error: unknown, fallback: string): string {
  if (typeof error === "string") {
    return error;
  }
  if (error instanceof Error) {
    return error.message;
  }
  if (
    error &&
    typeof error === "object" &&
    "message" in error &&
    typeof (error as { message: unknown }).message === "string"
  ) {
    return (error as { message: string }).message;
  }
  return fallback;
}

function isNoSpeakableTextError(error: unknown): boolean {
  const message = errorMessage(error, "").toLowerCase();
  return message.includes("no speakable text") || message.includes("no text to synthesize");
}

function clampPlaybackSpeed(speed: number): number {
  if (!Number.isFinite(speed)) {
    return SUPERTONIC3_DEFAULT_PLAYBACK_SPEED;
  }
  const clamped = Math.min(TTS_MAX_PLAYBACK_SPEED, Math.max(TTS_MIN_PLAYBACK_SPEED, speed));
  return Number(clamped.toFixed(2));
}

function readPlaybackSpeedOverride(): number | null {
  if (typeof window === "undefined") {
    return null;
  }

  try {
    const stored = window.localStorage.getItem(TTS_PLAYBACK_SPEED_STORAGE_KEY);
    if (!stored) {
      return null;
    }
    const speed = Number(stored);
    return Number.isFinite(speed) ? clampPlaybackSpeed(speed) : null;
  } catch {
    return null;
  }
}

function isTTSLanguage(value: string): value is TTSLanguage {
  return TTS_LANGUAGE_OPTIONS.some((option) => option.code === value);
}

function readTTSLanguage(): TTSLanguage {
  if (typeof window === "undefined") {
    return DEFAULT_TTS_LANGUAGE;
  }

  try {
    const stored = window.localStorage.getItem(TTS_LANGUAGE_STORAGE_KEY);
    return stored && isTTSLanguage(stored) ? stored : DEFAULT_TTS_LANGUAGE;
  } catch {
    return DEFAULT_TTS_LANGUAGE;
  }
}

function defaultPlaybackSpeedForModel(modelVersion: TTSModelVersion | null): number {
  return modelVersion === "legacy"
    ? LEGACY_DEFAULT_PLAYBACK_SPEED
    : SUPERTONIC3_DEFAULT_PLAYBACK_SPEED;
}

export function TTSProvider({ children }: { children: ReactNode }) {
  // Local TTS is available on desktop and iOS. Android remains intentionally excluded.
  const isTauriEnv = isTauriDesktop() || (isTauri() && isIOS());

  const [status, setStatus] = useState<TTSStatus>(isTauriEnv ? "checking" : "not_available");
  const [error, setError] = useState<string | null>(null);
  const [downloadProgress, setDownloadProgress] = useState(0);
  const [downloadDetail, setDownloadDetail] = useState("");
  const [totalSizeMB, setTotalSizeMB] = useState(383);
  const [modelVersion, setModelVersion] = useState<TTSModelVersion | null>(null);
  const [modelsPresentButIncompatible, setModelsPresentButIncompatible] = useState(false);
  const [isPreparing, setIsPreparing] = useState(false);
  const [isPlaying, setIsPlaying] = useState(false);
  const [currentPlayingId, setCurrentPlayingId] = useState<string | null>(null);
  const [playbackError, setPlaybackError] = useState<string | null>(null);
  const [playbackSpeedOverride, setPlaybackSpeedOverride] = useState<number | null>(() =>
    readPlaybackSpeedOverride()
  );
  const [ttsLanguage, setTTSLanguageState] = useState<TTSLanguage>(() => readTTSLanguage());

  const playbackSpeed =
    playbackSpeedOverride ?? clampPlaybackSpeed(defaultPlaybackSpeedForModel(modelVersion));
  const hasCustomPlaybackSpeed = playbackSpeedOverride !== null;
  const upgradeAvailable =
    modelVersion === "legacy" || modelsPresentButIncompatible || status === "upgrade_available";

  const mountedRef = useRef(true);
  const statusRequestIdRef = useRef(0);
  const playbackRequestIdRef = useRef(0);
  const modelOperationRef = useRef<"download" | "delete" | null>(null);
  const audioContextRef = useRef<AudioContext | null>(null);
  const scheduledSourceNodesRef = useRef<Set<AudioBufferSourceNode>>(new Set());
  const audioSessionPrevTypeRef = useRef<string | null>(null);
  const mediaSessionPrevStateRef = useRef<{
    metadata: MediaMetadata | null;
    playbackState: MediaSessionPlaybackState;
  } | null>(null);
  const unlistenRef = useRef<(() => void) | null>(null);

  const cleanupDownloadListener = useCallback(() => {
    if (unlistenRef.current) {
      unlistenRef.current();
      unlistenRef.current = null;
    }
  }, []);

  const restorePlatformAudioSession = useCallback(() => {
    if (audioSessionPrevTypeRef.current !== null) {
      try {
        const audioSession = (navigator as NavigatorWithAudioSession).audioSession;
        if (audioSession && typeof audioSession.type === "string") {
          audioSession.type = audioSessionPrevTypeRef.current;
        }
      } catch {
        // Ignore optional platform API failures.
      }
      audioSessionPrevTypeRef.current = null;
    }

    if (mediaSessionPrevStateRef.current) {
      try {
        if ("mediaSession" in navigator) {
          navigator.mediaSession.metadata = mediaSessionPrevStateRef.current.metadata;
          navigator.mediaSession.playbackState = mediaSessionPrevStateRef.current.playbackState;
        }
      } catch {
        // Ignore optional platform API failures.
      }
      mediaSessionPrevStateRef.current = null;
    }
  }, []);

  const cleanupPlaybackResources = useCallback(() => {
    playbackRequestIdRef.current += 1;

    for (const source of scheduledSourceNodesRef.current) {
      try {
        source.stop();
      } catch {
        // Ignore sources that have already ended.
      }
    }
    scheduledSourceNodesRef.current.clear();

    const audioContext = audioContextRef.current;
    audioContextRef.current = null;
    if (audioContext) {
      void audioContext.close().catch(() => {
        // Ignore contexts that have already closed.
      });
    }

    restorePlatformAudioSession();
  }, [restorePlatformAudioSession]);

  const stop = useCallback(() => {
    cleanupPlaybackResources();
    setIsPreparing(false);
    setIsPlaying(false);
    setCurrentPlayingId(null);
  }, [cleanupPlaybackResources]);

  const setPlaybackSpeed = useCallback((speed: number) => {
    const clamped = clampPlaybackSpeed(speed);
    setPlaybackSpeedOverride(clamped);

    try {
      window.localStorage.setItem(TTS_PLAYBACK_SPEED_STORAGE_KEY, clamped.toString());
    } catch {
      // A storage failure should not prevent using the preference for this session.
    }
  }, []);

  const resetPlaybackSpeed = useCallback(() => {
    setPlaybackSpeedOverride(null);

    try {
      window.localStorage.removeItem(TTS_PLAYBACK_SPEED_STORAGE_KEY);
    } catch {
      // Ignore storage failures.
    }
  }, []);

  const setTTSLanguage = useCallback((language: TTSLanguage) => {
    setTTSLanguageState(language);

    try {
      if (language === DEFAULT_TTS_LANGUAGE) {
        window.localStorage.removeItem(TTS_LANGUAGE_STORAGE_KEY);
      } else {
        window.localStorage.setItem(TTS_LANGUAGE_STORAGE_KEY, language);
      }
    } catch {
      // A storage failure should not prevent using the preference for this session.
    }
  }, []);

  const checkStatus = useCallback(async () => {
    if (!isTauriEnv) {
      if (mountedRef.current) {
        setStatus("not_available");
      }
      return;
    }

    const requestId = statusRequestIdRef.current + 1;
    statusRequestIdRef.current = requestId;

    try {
      const result = await invoke<TTSStatusResponse>("tts_get_status");
      if (!mountedRef.current || statusRequestIdRef.current !== requestId) {
        return;
      }

      setError(null);
      setTotalSizeMB(result.total_size_mb);
      setModelVersion(result.model_version);
      setModelsPresentButIncompatible(result.models_present_but_incompatible);

      if (result.models_loaded) {
        setStatus("ready");
        return;
      }

      if (result.models_downloaded) {
        setStatus("loading");
        try {
          await invoke("tts_load_models");
          if (!mountedRef.current || statusRequestIdRef.current !== requestId) {
            return;
          }
          setStatus("ready");
        } catch (loadError) {
          if (!mountedRef.current || statusRequestIdRef.current !== requestId) {
            return;
          }
          console.error("Failed to load TTS models:", loadError);
          setStatus("error");
          setError(errorMessage(loadError, "Failed to load TTS models"));
        }
        return;
      }

      setStatus(result.models_present_but_incompatible ? "upgrade_available" : "not_downloaded");
    } catch (statusError) {
      if (!mountedRef.current || statusRequestIdRef.current !== requestId) {
        return;
      }
      console.error("Failed to check TTS status:", statusError);
      setStatus("error");
      setError(errorMessage(statusError, "Failed to check TTS status"));
    }
  }, [isTauriEnv]);

  useEffect(() => {
    if (isTauriEnv) {
      void checkStatus();
    }
  }, [isTauriEnv, checkStatus]);

  const startDownload = useCallback(async () => {
    if (!isTauriEnv || modelOperationRef.current) {
      return false;
    }

    modelOperationRef.current = "download";
    statusRequestIdRef.current += 1;

    try {
      setStatus("downloading");
      setDownloadProgress(0);
      setDownloadDetail("Starting download...");
      setError(null);
      cleanupDownloadListener();

      const unlisten = await listen<DownloadProgress>("tts-download-progress", (event) => {
        if (!mountedRef.current) {
          return;
        }
        setDownloadProgress(event.payload.percent);
        setDownloadDetail(`Downloading ${event.payload.file_name}...`);
      });

      if (!mountedRef.current) {
        unlisten();
        return false;
      }
      unlistenRef.current = unlisten;

      await invoke("tts_download_models");
      if (!mountedRef.current) {
        return false;
      }

      setStatus("loading");
      setDownloadDetail("Loading models...");
      await invoke("tts_load_models");
      if (!mountedRef.current) {
        return false;
      }

      setModelVersion("supertonic3");
      setModelsPresentButIncompatible(false);
      setStatus("ready");
      setDownloadDetail("");
      setDownloadProgress(100);
      return true;
    } catch (downloadError) {
      if (!mountedRef.current) {
        return false;
      }
      console.error("TTS download failed:", downloadError);
      setStatus("error");
      setDownloadDetail("");
      setError(errorMessage(downloadError, "Failed to download TTS models"));
      return false;
    } finally {
      cleanupDownloadListener();
      modelOperationRef.current = null;
    }
  }, [isTauriEnv, cleanupDownloadListener]);

  const deleteModels = useCallback(async () => {
    if (
      !isTauriEnv ||
      modelOperationRef.current ||
      status === "checking" ||
      status === "downloading" ||
      status === "loading" ||
      status === "deleting"
    ) {
      return false;
    }

    modelOperationRef.current = "delete";
    statusRequestIdRef.current += 1;

    try {
      setStatus("deleting");
      setDownloadDetail("");
      setError(null);
      stop();

      await invoke("tts_delete_models");
      if (!mountedRef.current) {
        return false;
      }

      setModelVersion(null);
      setModelsPresentButIncompatible(false);
      setDownloadProgress(0);
      setStatus("not_downloaded");
      return true;
    } catch (deleteError) {
      if (!mountedRef.current) {
        return false;
      }
      console.error("Failed to delete TTS models:", deleteError);
      setStatus("error");
      setError(errorMessage(deleteError, "Failed to delete TTS models"));
      return false;
    } finally {
      modelOperationRef.current = null;
    }
  }, [isTauriEnv, status, stop]);

  const speak = useCallback(
    async (text: string, messageId: string) => {
      if (!isTauriEnv || status !== "ready") {
        return;
      }

      stop();
      setPlaybackError(null);

      const processedText = preprocessTextForTTS(text);
      if (!processedText) {
        return;
      }

      const requestId = playbackRequestIdRef.current + 1;
      playbackRequestIdRef.current = requestId;
      const isActiveRequest = () =>
        mountedRef.current && playbackRequestIdRef.current === requestId;

      try {
        setIsPreparing(true);
        setIsPlaying(false);
        setCurrentPlayingId(messageId);

        // Language conditioning is a Supertonic 3 feature. Keep legacy chunking
        // and synthesis behavior unchanged even if a v3 preference is stored.
        const synthesisLanguage =
          modelVersion === "supertonic3" ? ttsLanguage : DEFAULT_TTS_LANGUAGE;

        const { chunks } = await invoke<TTSChunkTextResponse>("tts_chunk_text", {
          text: processedText,
          language: synthesisLanguage
        });
        if (!isActiveRequest()) {
          return;
        }

        if (chunks.length === 0) {
          stop();
          return;
        }

        const audioWindow = window as WindowWithWebkitAudioContext;
        const AudioContextClass = audioWindow.AudioContext ?? audioWindow.webkitAudioContext;
        if (!AudioContextClass) {
          throw new Error(
            "Audio playback is not available. If you have Lockdown Mode enabled, TTS will not work."
          );
        }

        const prebufferBeforePlayback = isIOS();

        try {
          if (
            prebufferBeforePlayback &&
            "mediaSession" in navigator &&
            typeof MediaMetadata !== "undefined"
          ) {
            if (!mediaSessionPrevStateRef.current) {
              mediaSessionPrevStateRef.current = {
                metadata: navigator.mediaSession.metadata,
                playbackState: navigator.mediaSession.playbackState
              };
            }
            navigator.mediaSession.metadata = new MediaMetadata({
              title: "Maple AI",
              artist: "Text to Speech",
              artwork: [
                {
                  src: "/apple-touch-icon.png",
                  sizes: "180x180",
                  type: "image/png"
                },
                { src: "/favicon.png", sizes: "32x32", type: "image/png" }
              ]
            });
            navigator.mediaSession.playbackState = "playing";
          }
        } catch {
          // Ignore optional Media Session failures.
        }

        try {
          const audioSession = (navigator as NavigatorWithAudioSession).audioSession;
          if (audioSession && typeof audioSession.type === "string") {
            audioSessionPrevTypeRef.current = audioSession.type;
            audioSession.type = "playback";
          }
        } catch {
          // Ignore optional Audio Session failures.
        }

        // Preserve the existing iOS routing order: select the playback audio-session
        // category before constructing Web Audio so output uses the speaker path.
        const audioContext = new AudioContextClass();
        audioContextRef.current = audioContext;

        if (audioContext.state === "suspended") {
          await audioContext.resume();
          if (!isActiveRequest()) {
            return;
          }
        }

        const synthesizeAndDecodeChunk = async (
          chunkIndex: number
        ): Promise<DecodedTTSChunk | null> => {
          const result = await invoke<TTSSynthesizeResponse>("tts_synthesize_chunk", {
            text: chunks[chunkIndex],
            chunkIndex: chunkIndex + 1,
            chunkCount: chunks.length,
            language: synthesisLanguage,
            speed: playbackSpeed
          });
          if (!isActiveRequest()) {
            return null;
          }

          if (result.skipped || !result.audio_base64 || result.duration_seconds <= 0) {
            return null;
          }

          const audioBlob = base64ToBlob(result.audio_base64, "audio/wav");
          const audioBuffer = await audioContext.decodeAudioData(await audioBlob.arrayBuffer());
          if (!isActiveRequest()) {
            return null;
          }

          return { audioBuffer, chunkIndex };
        };

        let scheduledEndTime = audioContext.currentTime;
        let lastPlaybackEnded: Promise<void> | null = null;
        let startedPlayback = false;

        const scheduleDecodedChunk = (decoded: DecodedTTSChunk) => {
          const source = audioContext.createBufferSource();
          source.buffer = decoded.audioBuffer;
          source.connect(audioContext.destination);
          scheduledSourceNodesRef.current.add(source);

          const playbackEnded = new Promise<void>((resolve) => {
            source.onended = () => {
              scheduledSourceNodesRef.current.delete(source);
              resolve();
            };
          });

          const startAt = Math.max(scheduledEndTime, audioContext.currentTime);
          scheduledEndTime = startAt + decoded.audioBuffer.duration;
          try {
            source.start(startAt);
          } catch (sourceError) {
            scheduledSourceNodesRef.current.delete(source);
            throw sourceError;
          }

          lastPlaybackEnded = playbackEnded;
          if (!startedPlayback) {
            startedPlayback = true;
            setIsPreparing(false);
            setIsPlaying(true);
          }
        };

        const completedPreparation = await prepareAndScheduleTTSChunks({
          chunkCount: chunks.length,
          prebufferBeforePlayback,
          prepareChunk: synthesizeAndDecodeChunk,
          scheduleChunk: scheduleDecodedChunk,
          isActive: isActiveRequest,
          beforeBufferedSchedule: async () => {
            if (audioContext.state === "suspended") {
              await audioContext.resume();
            }
          }
        });
        if (!completedPreparation || !isActiveRequest()) {
          return;
        }

        if (!lastPlaybackEnded) {
          stop();
          return;
        }

        await lastPlaybackEnded;
        if (!isActiveRequest()) {
          return;
        }

        cleanupPlaybackResources();
        if (mountedRef.current) {
          setIsPreparing(false);
          setIsPlaying(false);
          setCurrentPlayingId(null);
        }
      } catch (playbackFailure) {
        if (!isActiveRequest()) {
          return;
        }

        if (isNoSpeakableTextError(playbackFailure)) {
          stop();
          return;
        }

        console.error("TTS playback failed:", playbackFailure);
        setPlaybackError(errorMessage(playbackFailure, "TTS playback failed"));
        stop();
      }
    },
    [isTauriEnv, modelVersion, playbackSpeed, status, stop, cleanupPlaybackResources, ttsLanguage]
  );

  const clearPlaybackError = useCallback(() => {
    setPlaybackError(null);
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    const scheduledSources = scheduledSourceNodesRef.current;

    return () => {
      mountedRef.current = false;
      statusRequestIdRef.current += 1;
      cleanupDownloadListener();
      cleanupPlaybackResources();
      scheduledSources.clear();
    };
  }, [cleanupDownloadListener, cleanupPlaybackResources]);

  return (
    <TTSContext.Provider
      value={{
        status,
        error,
        playbackError,
        downloadProgress,
        downloadDetail,
        totalSizeMB,
        upgradeAvailable,
        modelsPresentButIncompatible,
        modelVersion,
        isPreparing,
        isPlaying,
        currentPlayingId,
        playbackSpeed,
        hasCustomPlaybackSpeed,
        ttsLanguage,
        isTauriEnv,
        checkStatus,
        startDownload,
        deleteModels,
        speak,
        stop,
        setPlaybackSpeed,
        resetPlaybackSpeed,
        setTTSLanguage,
        clearPlaybackError
      }}
    >
      {children}
    </TTSContext.Provider>
  );
}

export function useTTS() {
  const context = useContext(TTSContext);
  if (!context) {
    throw new Error("useTTS must be used within a TTSProvider");
  }
  return context;
}

/**
 * Preprocess text for TTS by removing think blocks and other non-speakable content.
 */
function preprocessTextForTTS(text: string): string {
  let processed = stripFencedCodeBlocks(text);

  processed = processed.replace(/<think>[\s\S]*?<\/think>/g, "");
  processed = processed.replace(/<think>[\s\S]*$/g, "");

  return processed.trim();
}

function stripFencedCodeBlocks(text: string): string {
  const lines = text.split(/\r?\n/);
  const output: string[] = [];

  let inFence = false;
  let fenceChar: "`" | "~" | null = null;
  let fenceLen = 0;

  for (const line of lines) {
    if (!inFence) {
      const openMatch = line.match(/^\s*(?:>+\s*)?([`~]{3,})[^\n]*$/);
      if (openMatch) {
        inFence = true;
        fenceChar = openMatch[1][0] as "`" | "~";
        fenceLen = openMatch[1].length;
        continue;
      }

      output.push(line);
      continue;
    }

    const closeMatch = line.match(/^\s*(?:>+\s*)?([`~]{3,})\s*$/);
    if (closeMatch) {
      const fence = closeMatch[1];
      if (fenceChar && fence[0] === fenceChar && fence.length >= fenceLen) {
        inFence = false;
        fenceChar = null;
        fenceLen = 0;
      }
    }
  }

  return output.join("\n");
}

function base64ToBlob(base64: string, mimeType: string): Blob {
  const byteCharacters = atob(base64);
  const byteNumbers = new Array(byteCharacters.length);
  for (let index = 0; index < byteCharacters.length; index += 1) {
    byteNumbers[index] = byteCharacters.charCodeAt(index);
  }
  return new Blob([new Uint8Array(byteNumbers)], { type: mimeType });
}
