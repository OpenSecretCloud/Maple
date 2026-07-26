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

export const TTS_MIN_PLAYBACK_SPEED = 0.5;
export const TTS_MAX_PLAYBACK_SPEED = 2;
export const TTS_PLAYBACK_SPEED_STEP = 0.1;
const DEFAULT_TTS_PLAYBACK_SPEED = 1.2;
const TTS_PLAYBACK_SPEED_STORAGE_KEY = "ttsPlaybackSpeed";

export type TTSStatus =
  | "not_available"
  | "checking"
  | "not_downloaded"
  | "downloading"
  | "loading"
  | "ready"
  | "deleting"
  | "error";

interface TTSStatusResponse {
  models_downloaded: boolean;
  models_loaded: boolean;
  total_size_mb: number;
}

interface TTSSynthesizeResponse {
  audio_base64: string;
  sample_rate: number;
  duration_seconds: number;
}

interface DownloadProgress {
  downloaded: number;
  total: number;
  file_name: string;
  percent: number;
}

interface TTSContextValue {
  status: TTSStatus;
  error: string | null;
  playbackError: string | null;
  downloadProgress: number;
  downloadDetail: string;
  totalSizeMB: number;
  playbackSpeed: number;
  hasCustomPlaybackSpeed: boolean;
  isPreparing: boolean;
  isPlaying: boolean;
  currentPlayingId: string | null;
  isTauriEnv: boolean;

  checkStatus: () => Promise<void>;
  startDownload: () => Promise<void>;
  deleteModels: () => Promise<void>;
  setPlaybackSpeed: (speed: number) => void;
  resetPlaybackSpeed: () => void;
  speak: (text: string, messageId: string) => Promise<void>;
  stop: () => void;
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
    return DEFAULT_TTS_PLAYBACK_SPEED;
  }
  return (
    Math.round(Math.min(TTS_MAX_PLAYBACK_SPEED, Math.max(TTS_MIN_PLAYBACK_SPEED, speed)) * 10) / 10
  );
}

function loadPlaybackSpeed(): number {
  if (typeof window === "undefined") {
    return DEFAULT_TTS_PLAYBACK_SPEED;
  }
  try {
    const stored = window.localStorage.getItem(TTS_PLAYBACK_SPEED_STORAGE_KEY);
    if (stored !== null) {
      return clampPlaybackSpeed(Number(stored));
    }
  } catch {
    // Storage is optional; use the model default when it is unavailable.
  }
  return DEFAULT_TTS_PLAYBACK_SPEED;
}

export function TTSProvider({ children }: { children: ReactNode }) {
  // Check Tauri environment - TTS is available on desktop and iOS (not Android)
  const isTauriEnv = isTauriDesktop() || (isTauri() && isIOS());

  // Initial status depends on whether we're in Tauri
  const [status, setStatus] = useState<TTSStatus>(isTauriEnv ? "checking" : "not_available");
  const [error, setError] = useState<string | null>(null);
  const [downloadProgress, setDownloadProgress] = useState(0);
  const [downloadDetail, setDownloadDetail] = useState("");
  const [totalSizeMB, setTotalSizeMB] = useState(264);
  const [playbackSpeed, setPlaybackSpeedState] = useState(loadPlaybackSpeed);
  const [isPreparing, setIsPreparing] = useState(false);
  const [isPlaying, setIsPlaying] = useState(false);
  const [currentPlayingId, setCurrentPlayingId] = useState<string | null>(null);
  const [playbackError, setPlaybackError] = useState<string | null>(null);

  const audioContextRef = useRef<AudioContext | null>(null);
  const sourceNodeRef = useRef<AudioBufferSourceNode | null>(null);
  const audioUrlRef = useRef<string | null>(null);
  const audioSessionPrevTypeRef = useRef<string | null>(null);
  const mediaSessionPrevStateRef = useRef<{
    metadata: MediaMetadata | null;
    playbackState: MediaSessionPlaybackState;
  } | null>(null);
  const unlistenRef = useRef<(() => void) | null>(null);
  const playbackRequestRef = useRef(0);
  const mountedRef = useRef(true);

  const setPlaybackSpeed = useCallback((speed: number) => {
    const clamped = clampPlaybackSpeed(speed);
    setPlaybackSpeedState(clamped);
    try {
      window.localStorage.setItem(TTS_PLAYBACK_SPEED_STORAGE_KEY, String(clamped));
    } catch {
      // Keep the in-memory preference if storage is unavailable.
    }
  }, []);

  const resetPlaybackSpeed = useCallback(() => {
    setPlaybackSpeedState(DEFAULT_TTS_PLAYBACK_SPEED);
    try {
      window.localStorage.removeItem(TTS_PLAYBACK_SPEED_STORAGE_KEY);
    } catch {
      // Keep the in-memory default if storage is unavailable.
    }
  }, []);

  const cleanupDownloadListener = useCallback(() => {
    if (unlistenRef.current) {
      unlistenRef.current();
      unlistenRef.current = null;
    }
  }, []);

  // Check TTS status from Rust backend
  const checkStatus = useCallback(async () => {
    if (!isTauriEnv) {
      setStatus("not_available");
      return;
    }

    try {
      const result = await invoke<TTSStatusResponse>("tts_get_status");
      setTotalSizeMB(result.total_size_mb);

      if (result.models_loaded) {
        setStatus("ready");
      } else if (result.models_downloaded) {
        // Models downloaded but not loaded - load them
        setStatus("loading");
        try {
          await invoke("tts_load_models");
          setStatus("ready");
        } catch (loadErr) {
          console.error("Failed to load TTS models:", loadErr);
          setStatus("error");
          setError(errorMessage(loadErr, "Failed to load TTS models"));
        }
      } else {
        setStatus("not_downloaded");
      }
    } catch (err) {
      console.error("Failed to check TTS status:", err);
      setStatus("error");
      setError(errorMessage(err, "Failed to check TTS status"));
    }
  }, [isTauriEnv]);

  // Auto-check status on mount if in Tauri
  useEffect(() => {
    if (isTauriEnv) {
      checkStatus();
    }
  }, [isTauriEnv, checkStatus]);

  const startDownload = useCallback(async () => {
    if (!isTauriEnv) return;

    try {
      setStatus("downloading");
      setDownloadProgress(0);
      setDownloadDetail("Starting download...");
      setError(null);

      cleanupDownloadListener();

      // Set up event listener for progress
      const unlisten = await listen<DownloadProgress>("tts-download-progress", (event) => {
        const { percent, file_name } = event.payload;
        setDownloadProgress(percent);
        setDownloadDetail(`Downloading ${file_name}...`);
      });
      unlistenRef.current = unlisten;

      // Start the download
      await invoke("tts_download_models");

      // Load the models after download
      setStatus("loading");
      setDownloadDetail("Loading models...");
      await invoke("tts_load_models");

      setStatus("ready");
      setDownloadDetail("");
    } catch (err) {
      console.error("TTS download failed:", err);
      setStatus("error");
      // Tauri commands returning Result<_, String> reject with a plain string,
      // so `err instanceof Error` is false — surface the string directly instead
      // of hiding the real cause behind a generic message. See PR #520 review.
      setError(errorMessage(err, "Failed to download TTS models"));
    } finally {
      cleanupDownloadListener();
    }
  }, [isTauriEnv, cleanupDownloadListener]);

  const stop = useCallback(() => {
    playbackRequestRef.current += 1;
    if (audioUrlRef.current) {
      URL.revokeObjectURL(audioUrlRef.current);
      audioUrlRef.current = null;
    }
    if (sourceNodeRef.current) {
      try {
        sourceNodeRef.current.stop();
      } catch {
        // Ignore error if already stopped
      }
      sourceNodeRef.current = null;
    }
    if (audioContextRef.current) {
      void audioContextRef.current.close().catch(() => {
        // Ignore
      });
      audioContextRef.current = null;
    }

    if (audioSessionPrevTypeRef.current) {
      try {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const nav = navigator as any;
        if (nav.audioSession && typeof nav.audioSession.type === "string") {
          nav.audioSession.type = audioSessionPrevTypeRef.current;
        }
      } catch {
        // Ignore
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
        // Ignore
      }
      mediaSessionPrevStateRef.current = null;
    }
    if (mountedRef.current) {
      setIsPreparing(false);
      setIsPlaying(false);
      setCurrentPlayingId(null);
    }
  }, []);

  const deleteModels = useCallback(async () => {
    if (!isTauriEnv) return;

    try {
      setStatus("deleting");
      setError(null);

      // Stop any playing audio first
      stop();

      await invoke("tts_delete_models");
      setStatus("not_downloaded");
    } catch (err) {
      console.error("Failed to delete TTS models:", err);
      setStatus("error");
      setError(errorMessage(err, "Failed to delete TTS models"));
    }
  }, [isTauriEnv, stop]);

  const speak = useCallback(
    async (text: string, messageId: string) => {
      if (!isTauriEnv || status !== "ready") return;

      // Stop any currently playing audio
      stop();

      // Preprocess text to remove think blocks and other non-speakable content
      const processedText = preprocessTextForTTS(text);
      if (!processedText) {
        return;
      }

      const requestId = playbackRequestRef.current;
      try {
        setIsPreparing(true);
        setIsPlaying(false);
        setCurrentPlayingId(messageId);

        const result = await invoke<TTSSynthesizeResponse>("tts_synthesize", {
          text: processedText,
          speed: playbackSpeed
        });
        if (!mountedRef.current || requestId !== playbackRequestRef.current) {
          return;
        }

        // Create audio from base64
        const audioBlob = base64ToBlob(result.audio_base64, "audio/wav");
        const audioUrl = URL.createObjectURL(audioBlob);
        audioUrlRef.current = audioUrl;

        // iOS: set Now Playing metadata so the audio UI shows Maple instead of the origin hostname.
        // This is iOS-only and should not affect desktop media controls.
        try {
          if (isIOS() && "mediaSession" in navigator && typeof MediaMetadata !== "undefined") {
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
          // Ignore
        }

        // Use Web Audio API instead of HTMLAudioElement to avoid hijacking media controls
        // iOS Safari requires webkitAudioContext fallback
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const AudioContextClass = window.AudioContext || (window as any).webkitAudioContext;
        if (!AudioContextClass) {
          throw new Error(
            "Audio playback is not available. If you have Lockdown Mode enabled, TTS will not work."
          );
        }

        // iOS: try to force media playback routing (speaker) for Web Audio.
        // This helps avoid “only works with headphones / earpiece” routing issues.
        try {
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          const nav = navigator as any;
          if (nav.audioSession && typeof nav.audioSession.type === "string") {
            audioSessionPrevTypeRef.current = nav.audioSession.type;
            nav.audioSession.type = "playback";
          }
        } catch {
          // Ignore
        }

        const audioContext = new AudioContextClass() as AudioContext;

        // iOS requires user interaction to start audio - resume if suspended
        if (audioContext.state === "suspended") {
          await audioContext.resume();
        }

        const arrayBuffer = await audioBlob.arrayBuffer();
        const audioBuffer = await audioContext.decodeAudioData(arrayBuffer);
        if (!mountedRef.current || requestId !== playbackRequestRef.current) {
          void audioContext.close().catch(() => {
            // Ignore
          });
          return;
        }

        const source = audioContext.createBufferSource();
        source.buffer = audioBuffer;
        source.connect(audioContext.destination);

        // Store context and source for stop functionality
        audioContextRef.current = audioContext;
        sourceNodeRef.current = source;

        source.onended = () => {
          if (
            sourceNodeRef.current !== source ||
            requestId !== playbackRequestRef.current ||
            !mountedRef.current
          ) {
            return;
          }
          setIsPreparing(false);
          setIsPlaying(false);
          setCurrentPlayingId(null);

          if (audioUrlRef.current === audioUrl) {
            URL.revokeObjectURL(audioUrlRef.current);
            audioUrlRef.current = null;
          }
          void audioContext.close().catch(() => {
            // Ignore
          });
          audioContextRef.current = null;
          sourceNodeRef.current = null;

          if (audioSessionPrevTypeRef.current) {
            try {
              // eslint-disable-next-line @typescript-eslint/no-explicit-any
              const nav = navigator as any;
              if (nav.audioSession && typeof nav.audioSession.type === "string") {
                nav.audioSession.type = audioSessionPrevTypeRef.current;
              }
            } catch {
              // Ignore
            }
            audioSessionPrevTypeRef.current = null;
          }

          if (mediaSessionPrevStateRef.current) {
            try {
              if ("mediaSession" in navigator) {
                navigator.mediaSession.metadata = mediaSessionPrevStateRef.current.metadata;
                navigator.mediaSession.playbackState =
                  mediaSessionPrevStateRef.current.playbackState;
              }
            } catch {
              // Ignore
            }
            mediaSessionPrevStateRef.current = null;
          }
        };

        setIsPreparing(false);
        setIsPlaying(true);
        source.start(0);
      } catch (err) {
        if (!mountedRef.current || playbackRequestRef.current !== requestId) {
          return;
        }
        if (isNoSpeakableTextError(err)) {
          stop();
          return;
        }
        console.error("TTS playback failed:", err);
        setPlaybackError(errorMessage(err, "TTS playback failed"));
        stop();
      }
    },
    [isTauriEnv, playbackSpeed, status, stop]
  );

  const clearPlaybackError = useCallback(() => {
    setPlaybackError(null);
  }, []);

  // Clean up on unmount
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      playbackRequestRef.current += 1;
      if (unlistenRef.current) {
        unlistenRef.current();
      }
      if (sourceNodeRef.current) {
        try {
          sourceNodeRef.current.stop();
        } catch {
          // Ignore
        }
      }
      if (audioContextRef.current) {
        void audioContextRef.current.close().catch(() => {
          // Ignore
        });
      }
      if (audioUrlRef.current) {
        URL.revokeObjectURL(audioUrlRef.current);
        audioUrlRef.current = null;
      }

      if (audioSessionPrevTypeRef.current) {
        try {
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          const nav = navigator as any;
          if (nav.audioSession && typeof nav.audioSession.type === "string") {
            nav.audioSession.type = audioSessionPrevTypeRef.current;
          }
        } catch {
          // Ignore
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
          // Ignore
        }
        mediaSessionPrevStateRef.current = null;
      }
    };
  }, []);

  return (
    <TTSContext.Provider
      value={{
        status,
        error,
        playbackError,
        downloadProgress,
        downloadDetail,
        totalSizeMB,
        playbackSpeed,
        hasCustomPlaybackSpeed: playbackSpeed !== DEFAULT_TTS_PLAYBACK_SPEED,
        isPreparing,
        isPlaying,
        currentPlayingId,
        isTauriEnv,
        checkStatus,
        startDownload,
        deleteModels,
        setPlaybackSpeed,
        resetPlaybackSpeed,
        speak,
        stop,
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
 * Preprocess text for TTS by removing think blocks and other non-speakable content
 */
function preprocessTextForTTS(text: string): string {
  let processed = text;

  // Remove fenced code blocks (```lang\n...\n```), including optional language tags
  processed = stripFencedCodeBlocks(processed);

  // Remove <think>...</think> blocks (chain of thought reasoning)
  processed = processed.replace(/<think>[\s\S]*?<\/think>/g, "");

  // Remove unclosed <think> tags (streaming edge case)
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
  for (let i = 0; i < byteCharacters.length; i++) {
    byteNumbers[i] = byteCharacters.charCodeAt(i);
  }
  const byteArray = new Uint8Array(byteNumbers);
  return new Blob([byteArray], { type: mimeType });
}
