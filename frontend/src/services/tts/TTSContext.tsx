import { useOpenSecret } from "@opensecret/react";
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
  type ReactNode
} from "react";
import { isIOS } from "@/utils/platform";
import { useLocalState } from "@/state/useLocalState";
import { isKnownFreePlan } from "@/billing/billingAccess";
import {
  INITIAL_TTS_PLAYBACK_STATE,
  prepareAndScheduleTTSChunks,
  reduceTTSPlaybackState
} from "./ttsPlayback";
import {
  DEFAULT_TTS_PLAYBACK_SPEED,
  getStoredTTSPlaybackSpeed,
  getStoredTTSVoice,
  rememberTTSPlaybackSpeed,
  rememberTTSVoice,
  resetTTSPlaybackSpeed,
  type VoxtralTTSVoice
} from "./ttsPreferences";
import { calculateTTSLoudnessAdjustment } from "./ttsLoudness";
import { isPaidTTSAccessError, synthesizeTTSChunk } from "./ttsSynthesis";
import { calculateTTSTaperCorrection, type TTSTaperCorrection } from "./ttsTaperCorrection";
import { scheduleTTSTaperCorrection } from "./ttsTaperPlayback";
import { chunkTextForTTS } from "./ttsText";

interface DecodedTTSChunk {
  audioBuffer: AudioBuffer;
  chunkIndex: number;
  playbackGain: number;
  taperCorrection: TTSTaperCorrection;
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

interface TTSContextValue {
  playbackError: string | null;
  upgradeRequired: boolean;
  playbackSpeed: number;
  hasCustomPlaybackSpeed: boolean;
  voice: VoxtralTTSVoice;
  isPreparing: boolean;
  isPlaying: boolean;
  currentPlayingId: string | null;
  setPlaybackSpeed: (speed: number) => void;
  resetPlaybackSpeed: () => void;
  setVoice: (voice: VoxtralTTSVoice) => void;
  speak: (text: string, messageId: string) => Promise<void>;
  stop: () => void;
  clearPlaybackError: () => void;
  clearUpgradeRequired: () => void;
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
    typeof error.message === "string"
  ) {
    return error.message;
  }
  return fallback;
}

export function TTSProvider({ children }: { children: ReactNode }) {
  const { aiCustomFetch, apiUrl } = useOpenSecret();
  const { billingStatus } = useLocalState();
  const shouldShowTTSUpgrade = isKnownFreePlan(billingStatus);

  const [{ isPreparing, isPlaying, currentPlayingId }, dispatchPlayback] = useReducer(
    reduceTTSPlaybackState,
    INITIAL_TTS_PLAYBACK_STATE
  );
  const [playbackError, setPlaybackError] = useState<string | null>(null);
  const [upgradeRequired, setUpgradeRequired] = useState(false);
  const [playbackSpeed, setPlaybackSpeedState] = useState(getStoredTTSPlaybackSpeed);
  const [voice, setVoiceState] = useState(getStoredTTSVoice);

  const mountedRef = useRef(true);
  const playbackRequestIdRef = useRef(0);
  const abortControllerRef = useRef<AbortController | null>(null);
  const audioContextRef = useRef<AudioContext | null>(null);
  const scheduledSourceNodesRef = useRef<Set<AudioBufferSourceNode>>(new Set());
  const audioSessionPrevTypeRef = useRef<string | null>(null);
  const mediaSessionPrevStateRef = useRef<{
    metadata: MediaMetadata | null;
    playbackState: MediaSessionPlaybackState;
  } | null>(null);

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

    abortControllerRef.current?.abort();
    abortControllerRef.current = null;

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
    dispatchPlayback({ type: "idle" });
  }, [cleanupPlaybackResources]);

  const setPlaybackSpeed = useCallback((speed: number) => {
    setPlaybackSpeedState(rememberTTSPlaybackSpeed(speed));
  }, []);

  const resetPlaybackSpeed = useCallback(() => {
    resetTTSPlaybackSpeed();
    setPlaybackSpeedState(DEFAULT_TTS_PLAYBACK_SPEED);
  }, []);

  const setVoice = useCallback((selectedVoice: VoxtralTTSVoice) => {
    rememberTTSVoice(selectedVoice);
    setVoiceState(selectedVoice);
  }, []);

  const speak = useCallback(
    async (text: string, messageId: string) => {
      stop();
      setPlaybackError(null);
      setUpgradeRequired(false);

      if (shouldShowTTSUpgrade) {
        setUpgradeRequired(true);
        return;
      }

      const chunks = chunkTextForTTS(text);
      if (chunks.length === 0) {
        return;
      }

      const requestId = playbackRequestIdRef.current + 1;
      playbackRequestIdRef.current = requestId;
      const abortController = new AbortController();
      abortControllerRef.current = abortController;
      const isActiveRequest = () =>
        mountedRef.current &&
        playbackRequestIdRef.current === requestId &&
        !abortController.signal.aborted;

      try {
        dispatchPlayback({ type: "prepare", messageId });

        const audioWindow = window as WindowWithWebkitAudioContext;
        const AudioContextClass = audioWindow.AudioContext ?? audioWindow.webkitAudioContext;
        if (!AudioContextClass) {
          throw new Error(
            "Audio playback is not available. If you have Lockdown Mode enabled, text-to-speech will not work."
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

        // Preserve the established iOS routing order: select the playback
        // audio-session category before constructing Web Audio.
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
          const audioBytes = await synthesizeTTSChunk(
            aiCustomFetch,
            apiUrl,
            chunks[chunkIndex],
            { voice, speed: playbackSpeed },
            abortController.signal
          );
          if (!isActiveRequest()) {
            return null;
          }

          const audioBuffer = await audioContext.decodeAudioData(audioBytes);
          if (!isActiveRequest()) {
            return null;
          }

          const loudnessAdjustment = calculateTTSLoudnessAdjustment(audioBuffer);
          const taperCorrection = calculateTTSTaperCorrection(audioBuffer, loudnessAdjustment.gain);
          const taperSummary =
            taperCorrection.appliedCorrectionDb > 0
              ? `, taper=+${taperCorrection.appliedCorrectionDb.toFixed(1)} dB` +
                `${taperCorrection.peakLimited ? " (peak-limited)" : ""}`
              : "";
          console.debug(
            `[TTS] Prepared chunk ${chunkIndex + 1}/${chunks.length}: ` +
              `active=${loudnessAdjustment.activeRmsDbfs?.toFixed(1) ?? "silent"} dBFS, ` +
              `peak=${loudnessAdjustment.peakDbfs?.toFixed(1) ?? "silent"} dBFS, ` +
              `gain=${loudnessAdjustment.gainDb >= 0 ? "+" : ""}${loudnessAdjustment.gainDb.toFixed(1)} dB` +
              taperSummary
          );
          return {
            audioBuffer,
            chunkIndex,
            playbackGain: loudnessAdjustment.gain,
            taperCorrection
          };
        };

        let scheduledEndTime = audioContext.currentTime;
        let lastPlaybackEnded: Promise<void> | null = null;
        let startedPlayback = false;

        const scheduleDecodedChunk = (decoded: DecodedTTSChunk) => {
          const source = audioContext.createBufferSource();
          const normalizationGainNode = audioContext.createGain();
          const taperGainNode =
            decoded.taperCorrection.appliedCorrectionDb > 0 ? audioContext.createGain() : null;
          source.buffer = decoded.audioBuffer;
          normalizationGainNode.gain.value = decoded.playbackGain;
          source.connect(normalizationGainNode);
          if (taperGainNode) {
            normalizationGainNode.connect(taperGainNode);
            taperGainNode.connect(audioContext.destination);
          } else {
            normalizationGainNode.connect(audioContext.destination);
          }
          scheduledSourceNodesRef.current.add(source);

          const playbackEnded = new Promise<void>((resolve) => {
            source.onended = () => {
              normalizationGainNode.disconnect();
              taperGainNode?.disconnect();
              scheduledSourceNodesRef.current.delete(source);
              if (isActiveRequest()) {
                console.debug(`[TTS] Finished chunk ${decoded.chunkIndex + 1}/${chunks.length}`);
              }
              resolve();
            };
          });

          const startAt = Math.max(scheduledEndTime, audioContext.currentTime);
          scheduledEndTime = startAt + decoded.audioBuffer.duration;
          try {
            if (taperGainNode) {
              scheduleTTSTaperCorrection(
                taperGainNode.gain,
                decoded.taperCorrection,
                startAt,
                decoded.audioBuffer.duration
              );
            }
            source.start(startAt);
          } catch (sourceError) {
            normalizationGainNode.disconnect();
            taperGainNode?.disconnect();
            scheduledSourceNodesRef.current.delete(source);
            throw sourceError;
          }

          console.debug(
            `[TTS] Scheduled chunk ${decoded.chunkIndex + 1}/${chunks.length} ` +
              `at ${startAt.toFixed(3)}s for ${decoded.audioBuffer.duration.toFixed(3)}s`
          );
          lastPlaybackEnded = playbackEnded;
          if (!startedPlayback) {
            startedPlayback = true;
            dispatchPlayback({ type: "play" });
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
          dispatchPlayback({ type: "idle" });
        }
      } catch (playbackFailure) {
        if (!isActiveRequest()) {
          return;
        }

        console.error("TTS playback failed:", playbackFailure);
        if (isPaidTTSAccessError(playbackFailure)) {
          setUpgradeRequired(true);
        } else {
          setPlaybackError(errorMessage(playbackFailure, "Text-to-speech playback failed"));
        }
        stop();
      }
    },
    [
      aiCustomFetch,
      apiUrl,
      cleanupPlaybackResources,
      playbackSpeed,
      shouldShowTTSUpgrade,
      stop,
      voice
    ]
  );

  const clearPlaybackError = useCallback(() => {
    setPlaybackError(null);
  }, []);

  const clearUpgradeRequired = useCallback(() => {
    setUpgradeRequired(false);
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    const scheduledSources = scheduledSourceNodesRef.current;

    return () => {
      mountedRef.current = false;
      cleanupPlaybackResources();
      scheduledSources.clear();
    };
  }, [cleanupPlaybackResources]);

  const contextValue = useMemo<TTSContextValue>(
    () => ({
      playbackError,
      upgradeRequired,
      playbackSpeed,
      hasCustomPlaybackSpeed: playbackSpeed !== DEFAULT_TTS_PLAYBACK_SPEED,
      voice,
      isPreparing,
      isPlaying,
      currentPlayingId,
      setPlaybackSpeed,
      resetPlaybackSpeed,
      setVoice,
      speak,
      stop,
      clearPlaybackError,
      clearUpgradeRequired
    }),
    [
      clearPlaybackError,
      clearUpgradeRequired,
      currentPlayingId,
      isPlaying,
      isPreparing,
      playbackError,
      playbackSpeed,
      resetPlaybackSpeed,
      setPlaybackSpeed,
      setVoice,
      speak,
      stop,
      upgradeRequired,
      voice
    ]
  );

  return <TTSContext.Provider value={contextValue}>{children}</TTSContext.Provider>;
}

export function useTTS() {
  const context = useContext(TTSContext);
  if (!context) {
    throw new Error("useTTS must be used within a TTSProvider");
  }
  return context;
}
