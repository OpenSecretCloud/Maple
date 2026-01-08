import {
  createContext,
  useContext,
  useState,
  useCallback,
  useRef,
  useEffect,
  ReactNode
} from "react";
import { isTauriDesktop } from "@/utils/platform";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

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
  downloadProgress: number;
  downloadDetail: string;
  totalSizeMB: number;
  isPlaying: boolean;
  currentPlayingId: string | null;
  isTauriEnv: boolean;

  checkStatus: () => Promise<void>;
  startDownload: () => Promise<void>;
  deleteModels: () => Promise<void>;
  speak: (text: string, messageId: string) => Promise<void>;
  stop: () => void;
}

const TTSContext = createContext<TTSContextValue | null>(null);

export function TTSProvider({ children }: { children: ReactNode }) {
  // Check Tauri desktop environment once at mount - TTS is desktop-only
  const isTauriEnv = isTauriDesktop();

  // Initial status depends on whether we're in Tauri
  const [status, setStatus] = useState<TTSStatus>(isTauriEnv ? "checking" : "not_available");
  const [error, setError] = useState<string | null>(null);
  const [downloadProgress, setDownloadProgress] = useState(0);
  const [downloadDetail, setDownloadDetail] = useState("");
  const [totalSizeMB, setTotalSizeMB] = useState(264);
  const [isPlaying, setIsPlaying] = useState(false);
  const [currentPlayingId, setCurrentPlayingId] = useState<string | null>(null);

  const audioContextRef = useRef<AudioContext | null>(null);
  const sourceNodeRef = useRef<AudioBufferSourceNode | null>(null);
  const audioUrlRef = useRef<string | null>(null);
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
          setError(loadErr instanceof Error ? loadErr.message : "Failed to load TTS models");
        }
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
      setStatus("not_downloaded");
    } catch (err) {
      console.error("Failed to delete TTS models:", err);
      setStatus("error");
      setError(err instanceof Error ? err.message : "Failed to delete TTS models");
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

      try {
        setIsPlaying(true);
        setCurrentPlayingId(messageId);

        const result = await invoke<TTSSynthesizeResponse>("tts_synthesize", {
          text: processedText
        });

        // Create audio from base64
        const audioBlob = base64ToBlob(result.audio_base64, "audio/wav");
        const audioUrl = URL.createObjectURL(audioBlob);
        audioUrlRef.current = audioUrl;

        // Use Web Audio API instead of HTMLAudioElement to avoid hijacking media controls
        const audioContext = new AudioContext();
        const arrayBuffer = await audioBlob.arrayBuffer();
        const audioBuffer = await audioContext.decodeAudioData(arrayBuffer);

        const source = audioContext.createBufferSource();
        source.buffer = audioBuffer;
        source.connect(audioContext.destination);

        // Store context and source for stop functionality
        audioContextRef.current = audioContext;
        sourceNodeRef.current = source;

        source.onended = () => {
          if (sourceNodeRef.current !== source) {
            return;
          }
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
        };

        source.start(0);
      } catch (err) {
        console.error("TTS synthesis failed:", err);
        stop();
      }
    },
    [isTauriEnv, status, stop]
  );

  // Clean up on unmount
  useEffect(() => {
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
      if (audioContextRef.current) {
        void audioContextRef.current.close().catch(() => {
          // Ignore
        });
      }
      if (audioUrlRef.current) {
        URL.revokeObjectURL(audioUrlRef.current);
        audioUrlRef.current = null;
      }
    };
  }, []);

  return (
    <TTSContext.Provider
      value={{
        status,
        error,
        downloadProgress,
        downloadDetail,
        totalSizeMB,
        isPlaying,
        currentPlayingId,
        isTauriEnv,
        checkStatus,
        startDownload,
        deleteModels,
        speak,
        stop
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
