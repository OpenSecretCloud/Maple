import { useEffect, useState, useRef, useMemo } from "react";
import { X, CornerRightUp, Loader2, RotateCcw, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/utils/utils";

/** The current phase shown by the overlay. */
export type VoiceOverlayState =
  | "recording"
  | "processing"
  | "error"
  | "waiting"
  | "generating"
  | "playing";

interface RecordingOverlayProps {
  isRecording: boolean;
  isProcessing?: boolean;
  onSend: () => void;
  onCancel: () => void;
  isCompact?: boolean;
  className?: string;

  // Voice-mode extensions
  /** Current voice-mode state (defaults to recording/processing based on isRecording/isProcessing) */
  voiceState?: VoiceOverlayState;
  /** Error message to display in error state */
  errorMessage?: string;
  /** Duration of the recording that failed (shown in error state) */
  savedDuration?: number;
  /** Called when user taps Retry in error state */
  onRetry?: () => void;
  /** Called when user taps Discard in error state */
  onDiscard?: () => void;
}

export function RecordingOverlay({
  isRecording,
  isProcessing = false,
  onSend,
  onCancel,
  isCompact = false,
  className,
  voiceState: voiceStateProp,
  errorMessage,
  savedDuration,
  onRetry,
  onDiscard
}: RecordingOverlayProps) {
  // Derive the effective state: use voiceState prop if provided, otherwise fall back
  const effectiveState: VoiceOverlayState = voiceStateProp
    ? voiceStateProp
    : isProcessing
      ? "processing"
      : "recording";

  const [duration, setDuration] = useState(0);
  const startTimeRef = useRef<number>(0);
  const animationFrameRef = useRef<number>();

  useEffect(() => {
    if (effectiveState === "recording") {
      setDuration(0);
      startTimeRef.current = Date.now();

      const updateTimer = () => {
        const elapsed = Math.floor((Date.now() - startTimeRef.current) / 1000);
        setDuration(elapsed);
        animationFrameRef.current = requestAnimationFrame(updateTimer);
      };

      animationFrameRef.current = requestAnimationFrame(updateTimer);

      return () => {
        if (animationFrameRef.current) {
          cancelAnimationFrame(animationFrameRef.current);
        }
      };
    }
  }, [effectiveState]);

  const formatTime = (seconds: number) => {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, "0")}`;
  };

  // Determine color scheme based on state
  const isPlaybackStyle = effectiveState === "generating" || effectiveState === "playing";

  // Generate stable bar configurations once when component mounts
  const waveformBars = useMemo(() => {
    const barCount = 30;
    const bars = [];

    // Create a pseudo-random but stable pattern
    const seed = 12345; // Fixed seed for consistency
    let rand = seed;
    const pseudoRandom = () => {
      rand = (rand * 9301 + 49297) % 233280;
      return rand / 233280;
    };

    for (let i = 0; i < barCount; i++) {
      // Create a wave-like pattern with some randomness
      const baseHeight = 35 + Math.sin((i / barCount) * Math.PI * 2) * 20;
      const randomVariation = pseudoRandom() * 15;
      const height = baseHeight + randomVariation;

      // Stagger animations for a more natural flow
      const animationDuration = 0.8 + pseudoRandom() * 0.4; // 0.8-1.2s
      const animationDelay = (i / barCount) * 0.3; // Progressive delay

      bars.push({
        height,
        animationDuration,
        animationDelay
      });
    }

    return bars;
  }, []); // Empty deps = generated once

  const shouldAnimate =
    effectiveState === "recording" ||
    effectiveState === "generating" ||
    effectiveState === "playing";

  const renderWaveformBars = () => {
    const barColorClass = isPlaybackStyle ? "bg-blue-400/50" : "bg-primary/40";
    const animName = isPlaybackStyle ? "pulse-blue" : "pulse";

    return waveformBars.map((bar, i) => (
      <div
        key={i}
        className={cn("flex-shrink-0 rounded-full", barColorClass)}
        style={{
          width: "2px",
          height: `${bar.height}%`,
          animation: shouldAnimate
            ? `${animName} ${bar.animationDuration}s ease-in-out ${bar.animationDelay}s infinite`
            : "none",
          transition: "height 0.3s ease-out"
        }}
      />
    ));
  };

  // Show the overlay when recording OR when in any voice-mode state
  const isVisible = isRecording || !!voiceStateProp;
  if (!isVisible) return null;

  // Whether the top-right send button should be shown (only in recording state)
  const showSendButton = effectiveState === "recording" || effectiveState === "processing";

  const renderStatusContent = () => {
    switch (effectiveState) {
      case "recording":
        return (
          <>
            <div className="w-2 h-2 bg-destructive rounded-full animate-pulse" />
            Recording
          </>
        );
      case "processing":
        return (
          <>
            <Loader2 className="w-4 h-4 animate-spin" />
            Processing...
          </>
        );
      case "error":
        return (
          <div className="flex flex-col items-center gap-3">
            <div className="text-destructive text-sm text-center max-w-xs">
              {errorMessage || "Transcription failed"}
            </div>
            {savedDuration !== undefined && (
              <div className="text-xs text-muted-foreground">
                Recording: {formatTime(savedDuration)}
              </div>
            )}
            <div className="flex items-center gap-2">
              {onRetry && (
                <Button onClick={onRetry} variant="outline" size="sm" className="gap-1.5">
                  <RotateCcw className="h-3.5 w-3.5" />
                  Retry
                </Button>
              )}
              {onDiscard && (
                <Button
                  onClick={onDiscard}
                  variant="ghost"
                  size="sm"
                  className="gap-1.5 text-muted-foreground"
                >
                  <Trash2 className="h-3.5 w-3.5" />
                  Discard
                </Button>
              )}
            </div>
          </div>
        );
      case "waiting":
        return (
          <div className="flex items-center gap-2">
            <div className="w-2 h-2 bg-blue-400 rounded-full animate-[breathing_2s_ease-in-out_infinite]" />
            Waiting for response...
          </div>
        );
      case "generating":
        return (
          <div className="flex items-center gap-2 text-blue-400">
            <Loader2 className="w-4 h-4 animate-spin" />
            Generating audio...
          </div>
        );
      case "playing":
        return (
          <div className="flex items-center gap-2 text-blue-400">
            <div className="w-2 h-2 bg-blue-400 rounded-full animate-pulse" />
            Playing
          </div>
        );
    }
  };

  return (
    <div
      className={cn(
        "absolute inset-0 z-40 flex items-center justify-center",
        "animate-in fade-in duration-200",
        className
      )}
    >
      <style>
        {`
          @keyframes pulse {
            0%, 100% { transform: scaleY(0.5); opacity: 0.6; }
            50% { transform: scaleY(1); opacity: 1; }
          }
          @keyframes pulse-blue {
            0%, 100% { transform: scaleY(0.5); opacity: 0.5; }
            50% { transform: scaleY(1); opacity: 0.9; }
          }
          @keyframes breathing {
            0%, 100% { opacity: 0.4; transform: scale(0.9); }
            50% { opacity: 1; transform: scale(1.1); }
          }
        `}
      </style>

      <div
        className={cn(
          "w-full h-full rounded-lg bg-background/95 backdrop-blur-sm border relative overflow-hidden flex flex-col items-center justify-center p-4",
          isPlaybackStyle ? "border-blue-400/30" : "border-primary/20"
        )}
      >
        {/* Top buttons */}
        <div className="absolute top-3 left-3 right-3 flex justify-between">
          <Button
            onClick={onCancel}
            variant="ghost"
            size="icon"
            className="rounded-full hover:bg-muted"
            aria-label={effectiveState === "recording" ? "Cancel recording" : "Exit voice mode"}
            disabled={effectiveState === "processing"}
          >
            <X className="h-4 w-4" />
          </Button>

          {showSendButton && (
            <Button
              onClick={onSend}
              size={isCompact ? "icon" : "sm"}
              className={cn(isCompact ? "rounded-full" : "gap-1.5")}
              aria-label="Send recording"
              disabled={effectiveState === "processing"}
            >
              {effectiveState === "processing" ? (
                <Loader2 className={cn(isCompact ? "h-4 w-4" : "h-3.5 w-3.5", "animate-spin")} />
              ) : isCompact ? (
                <CornerRightUp className="h-4 w-4" />
              ) : (
                <>
                  <CornerRightUp className="h-3.5 w-3.5" />
                  Send
                </>
              )}
            </Button>
          )}
        </div>

        <div className="flex flex-col items-center gap-6 max-w-md w-full">
          {/* Waveform visualization - show for recording (non-compact), generating, and playing (always) */}
          {((!isCompact && effectiveState === "recording") ||
            effectiveState === "generating" ||
            effectiveState === "playing") && (
            <div className="flex items-center justify-center h-12 w-full gap-0.5 px-4">
              {renderWaveformBars()}
            </div>
          )}

          {/* Timer - show during recording */}
          {(effectiveState === "recording" || effectiveState === "processing") && (
            <div className="text-2xl font-mono text-muted-foreground">{formatTime(duration)}</div>
          )}

          {/* Status indicator - show in all modes for voice states, only non-compact for recording */}
          {(!isCompact || (effectiveState !== "recording" && effectiveState !== "processing")) && (
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              {renderStatusContent()}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
