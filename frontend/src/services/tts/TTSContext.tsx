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

interface TTSStatusResponse {
  models_downloaded: boolean;
  models_loaded: boolean;
  models_present_but_incompatible: boolean;
  upgrade_available: boolean;
  model_version: "supertonic3" | "legacy" | null;
  total_size_mb: number;
}

interface TTSSynthesizeResponse {
  audio_base64: string;
  sample_rate: number;
  duration_seconds: number;
}

interface TTSChunkTextResponse {
  chunks: string[];
}

interface DecodedTTSChunk {
  audioBuffer: AudioBuffer;
  chunkIndex: number;
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
  upgradeAvailable: boolean;
  modelVersion: "supertonic3" | "legacy" | null;
  isPreparing: boolean;
  isPlaying: boolean;
  currentPlayingId: string | null;
  isTauriEnv: boolean;

  checkStatus: () => Promise<void>;
  startDownload: () => Promise<void>;
  deleteModels: () => Promise<void>;
  speak: (text: string, messageId: string) => Promise<void>;
  stop: () => void;
  clearPlaybackError: () => void;
}

const TTSContext = createContext<TTSContextValue | null>(null);

function debugTTS(event: string, details?: Record<string, unknown>) {
  if (import.meta.env.DEV) {
    console.info(`[TTS] ${event}`, details ?? {});
  }
}

function errorMessage(err: unknown, fallback: string): string {
  if (err instanceof Error) {
    return err.message;
  }
  if (typeof err === "string") {
    return err;
  }
  if (
    err &&
    typeof err === "object" &&
    "message" in err &&
    typeof (err as { message: unknown }).message === "string"
  ) {
    return (err as { message: string }).message;
  }
  return fallback;
}

export function TTSProvider({ children }: { children: ReactNode }) {
  // Check Tauri environment - TTS is available on desktop and iOS (not Android)
  const isTauriEnv = isTauriDesktop() || (isTauri() && isIOS());

  // Initial status depends on whether we're in Tauri
  const [status, setStatus] = useState<TTSStatus>(isTauriEnv ? "checking" : "not_available");
  const [error, setError] = useState<string | null>(null);
  const [downloadProgress, setDownloadProgress] = useState(0);
  const [downloadDetail, setDownloadDetail] = useState("");
  const [totalSizeMB, setTotalSizeMB] = useState(383);
  const [upgradeAvailable, setUpgradeAvailable] = useState(false);
  const [modelVersion, setModelVersion] = useState<"supertonic3" | "legacy" | null>(null);
  const [isPreparing, setIsPreparing] = useState(false);
  const [isPlaying, setIsPlaying] = useState(false);
  const [currentPlayingId, setCurrentPlayingId] = useState<string | null>(null);
  const [playbackError, setPlaybackError] = useState<string | null>(null);

  const requestIdRef = useRef(0);
  const synthesisInFlightRef = useRef(false);
  const audioContextRef = useRef<AudioContext | null>(null);
  const sourceNodeRef = useRef<AudioBufferSourceNode | null>(null);
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

  // Check TTS status from Rust backend
  const checkStatus = useCallback(async () => {
    if (!isTauriEnv) {
      setStatus("not_available");
      return;
    }

    try {
      const result = await invoke<TTSStatusResponse>("tts_get_status");
      setTotalSizeMB(result.total_size_mb);
      setUpgradeAvailable(result.upgrade_available);
      setModelVersion(result.model_version);

      if (result.models_loaded) {
        setStatus("ready");
      } else if (result.models_downloaded) {
        // Models downloaded but not loaded - load them
        setStatus("loading");
        try {
          await invoke("tts_load_models");
          setUpgradeAvailable(result.upgrade_available);
          setModelVersion(result.model_version);
          setStatus("ready");
        } catch (loadErr) {
          console.error("Failed to load TTS models:", loadErr);
          setStatus("error");
          setError(loadErr instanceof Error ? loadErr.message : "Failed to load TTS models");
        }
      } else if (result.models_present_but_incompatible) {
        setStatus("upgrade_available");
      } else {
        setStatus("not_downloaded");
      }
    } catch (err) {
      console.error("Failed to check TTS status:", err);
      setStatus("error");
      setError(err instanceof Error ? err.message : "Failed to check TTS status");
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

      setUpgradeAvailable(false);
      setModelVersion("supertonic3");
      setStatus("ready");
      setDownloadDetail("");
    } catch (err) {
      console.error("TTS download failed:", err);
      setStatus("error");
      setError(err instanceof Error ? err.message : "Failed to download TTS models");
    } finally {
      cleanupDownloadListener();
    }
  }, [isTauriEnv, cleanupDownloadListener]);

  const stop = useCallback(() => {
    requestIdRef.current += 1;
    synthesisInFlightRef.current = false;

    for (const source of scheduledSourceNodesRef.current) {
      try {
        source.stop();
      } catch {
        // Ignore error if already stopped
      }
    }
    scheduledSourceNodesRef.current.clear();
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
    setIsPreparing(false);
    setIsPlaying(false);
    setCurrentPlayingId(null);
  }, []);

  const deleteModels = useCallback(async () => {
    if (!isTauriEnv) return;

    try {
      setStatus("deleting");
      setError(null);

      // Stop any playing audio first
      stop();

      await invoke("tts_delete_models");
      setUpgradeAvailable(false);
      setModelVersion(null);
      setStatus("not_downloaded");
    } catch (err) {
      console.error("Failed to delete TTS models:", err);
      setStatus("error");
      setError(err instanceof Error ? err.message : "Failed to delete TTS models");
    }
  }, [isTauriEnv, stop]);

  const speak = useCallback(
    async (text: string, messageId: string) => {
      if (!isTauriEnv || status !== "ready") {
        debugTTS("speak skipped", { isTauriEnv, status, messageId });
        return;
      }

      if (synthesisInFlightRef.current) {
        debugTTS("speak skipped while synthesis is in flight", { messageId });
        return;
      }

      // Stop any currently playing audio
      stop();
      setPlaybackError(null);

      // Preprocess text to remove think blocks and other non-speakable content
      const processedText = preprocessTextForTTS(text);
      if (!processedText) {
        debugTTS("speak skipped after preprocessing", {
          messageId,
          rawChars: text.length
        });
        return;
      }

      const requestId = requestIdRef.current + 1;
      requestIdRef.current = requestId;
      const startedAt = performance.now();
      try {
        debugTTS("speak start", {
          messageId,
          rawChars: text.length,
          processedChars: processedText.length
        });
        synthesisInFlightRef.current = true;
        setIsPreparing(true);
        setIsPlaying(false);
        setCurrentPlayingId(messageId);

        const { chunks } = await invoke<TTSChunkTextResponse>("tts_chunk_text", {
          text: processedText
        });

        if (requestIdRef.current !== requestId) {
          debugTTS("chunk plan ignored", {
            messageId,
            elapsedMs: Math.round(performance.now() - startedAt)
          });
          return;
        }

        debugTTS("chunk plan ready", {
          messageId,
          chunks: chunks.length,
          elapsedMs: Math.round(performance.now() - startedAt)
        });

        // Use Web Audio API instead of HTMLAudioElement to avoid hijacking media controls
        // iOS Safari requires webkitAudioContext fallback
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const AudioContextClass = window.AudioContext || (window as any).webkitAudioContext;
        if (!AudioContextClass) {
          throw new Error(
            "Audio playback is not available. If you have Lockdown Mode enabled, TTS will not work."
          );
        }

        const audioContext = new AudioContextClass() as AudioContext;
        audioContextRef.current = audioContext;

        // iOS requires user interaction to start audio - resume if suspended
        if (audioContext.state === "suspended") {
          debugTTS("audio context resume start", { messageId });
          await audioContext.resume();
          debugTTS("audio context resumed", {
            messageId,
            state: audioContext.state
          });
        }

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

        const finishPlaybackSession = async () => {
          if (audioContextRef.current === audioContext) {
            await audioContext.close().catch(() => {
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
                navigator.mediaSession.playbackState =
                  mediaSessionPrevStateRef.current.playbackState;
              }
            } catch {
              // Ignore
            }
            mediaSessionPrevStateRef.current = null;
          }
        };

        const synthesizeAndDecodeChunk = async (chunkIndex: number): Promise<DecodedTTSChunk> => {
          const chunkStartedAt = performance.now();
          debugTTS("chunk synth start", {
            messageId,
            chunk: chunkIndex + 1,
            chunks: chunks.length,
            chars: chunks[chunkIndex].length
          });

          const result = await invoke<TTSSynthesizeResponse>("tts_synthesize_chunk", {
            text: chunks[chunkIndex],
            chunkIndex: chunkIndex + 1,
            chunkCount: chunks.length
          });

          if (requestIdRef.current !== requestId) {
            throw new Error("TTS playback cancelled");
          }

          debugTTS("chunk synth response", {
            messageId,
            chunk: chunkIndex + 1,
            chunks: chunks.length,
            elapsedMs: Math.round(performance.now() - chunkStartedAt),
            durationSeconds: result.duration_seconds,
            sampleRate: result.sample_rate,
            base64Chars: result.audio_base64.length
          });

          const audioBlob = base64ToBlob(result.audio_base64, "audio/wav");
          const arrayBuffer = await audioBlob.arrayBuffer();
          const audioBuffer = await audioContext.decodeAudioData(arrayBuffer);

          if (requestIdRef.current !== requestId) {
            throw new Error("TTS playback cancelled");
          }

          debugTTS("chunk decoded", {
            messageId,
            chunk: chunkIndex + 1,
            chunks: chunks.length,
            durationSeconds: audioBuffer.duration,
            sampleRate: audioBuffer.sampleRate
          });

          return {
            audioBuffer,
            chunkIndex
          };
        };

        let scheduledEndTime = 0;
        const scheduleAudioBuffer = (decoded: DecodedTTSChunk): Promise<void> => {
          if (requestIdRef.current !== requestId) {
            return Promise.resolve();
          }

          const source = audioContext.createBufferSource();
          source.buffer = decoded.audioBuffer;
          source.connect(audioContext.destination);
          scheduledSourceNodesRef.current.add(source);

          const ended = new Promise<void>((resolve) => {
            source.onended = () => {
              scheduledSourceNodesRef.current.delete(source);
              if (sourceNodeRef.current === source) {
                sourceNodeRef.current = null;
              }
              debugTTS("chunk playback ended", {
                messageId,
                chunk: decoded.chunkIndex + 1,
                chunks: chunks.length,
                elapsedMs: Math.round(performance.now() - startedAt)
              });
              resolve();
            };
          });

          sourceNodeRef.current = source;
          setIsPreparing(false);
          setIsPlaying(true);
          setCurrentPlayingId(messageId);

          const currentTime = audioContext.currentTime;
          const startAt = Math.max(scheduledEndTime, currentTime);
          scheduledEndTime = startAt + decoded.audioBuffer.duration;

          try {
            source.start(startAt);
            synthesisInFlightRef.current = false;
            debugTTS("chunk playback scheduled", {
              messageId,
              chunk: decoded.chunkIndex + 1,
              chunks: chunks.length,
              startDelayMs: Math.round((startAt - currentTime) * 1000),
              durationSeconds: decoded.audioBuffer.duration,
              elapsedMs: Math.round(performance.now() - startedAt)
            });
          } catch (err) {
            scheduledSourceNodesRef.current.delete(source);
            if (sourceNodeRef.current === source) {
              sourceNodeRef.current = null;
            }
            throw err;
          }

          return ended;
        };

        let nextChunkPromise: Promise<DecodedTTSChunk> | null = synthesizeAndDecodeChunk(0);
        let finalPlaybackPromise: Promise<void> | null = null;

        for (let chunkIndex = 0; chunkIndex < chunks.length; chunkIndex++) {
          if (!nextChunkPromise || requestIdRef.current !== requestId) {
            break;
          }

          setIsPreparing(!sourceNodeRef.current);
          const decoded = await nextChunkPromise;
          if (requestIdRef.current !== requestId) {
            break;
          }

          finalPlaybackPromise = scheduleAudioBuffer(decoded);

          nextChunkPromise =
            chunkIndex + 1 < chunks.length ? synthesizeAndDecodeChunk(chunkIndex + 1) : null;
        }

        if (finalPlaybackPromise && requestIdRef.current === requestId) {
          await finalPlaybackPromise;
        }

        if (requestIdRef.current === requestId) {
          synthesisInFlightRef.current = false;
          setIsPreparing(false);
          setIsPlaying(false);
          setCurrentPlayingId(null);
          sourceNodeRef.current = null;
          await finishPlaybackSession();
          debugTTS("speak complete", {
            messageId,
            chunks: chunks.length,
            elapsedMs: Math.round(performance.now() - startedAt)
          });
        }
      } catch (err) {
        if (requestIdRef.current !== requestId) {
          debugTTS("speak error ignored after cancellation", {
            messageId,
            elapsedMs: Math.round(performance.now() - startedAt)
          });
          return;
        }

        synthesisInFlightRef.current = false;
        setIsPreparing(false);
        const message = errorMessage(err, "TTS playback failed");
        console.error("TTS playback failed:", err, {
          messageId,
          elapsedMs: Math.round(performance.now() - startedAt)
        });
        setPlaybackError(message);
        stop();
      }
    },
    [isTauriEnv, status, stop]
  );

  const clearPlaybackError = useCallback(() => {
    setPlaybackError(null);
  }, []);

  // Clean up on unmount
  useEffect(() => {
    const scheduledSourceNodes = scheduledSourceNodesRef.current;

    return () => {
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
      for (const source of scheduledSourceNodes) {
        try {
          source.stop();
        } catch {
          // Ignore
        }
      }
      scheduledSourceNodes.clear();
      if (audioContextRef.current) {
        void audioContextRef.current.close().catch(() => {
          // Ignore
        });
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
        upgradeAvailable,
        modelVersion,
        isPreparing,
        isPlaying,
        currentPlayingId,
        isTauriEnv,
        checkStatus,
        startDownload,
        deleteModels,
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
