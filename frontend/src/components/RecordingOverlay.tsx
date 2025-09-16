import { useEffect, useState, useRef, useMemo } from "react";
import { X, CornerRightUp, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/utils/utils";

interface RecordingOverlayProps {
  isRecording: boolean;
  isProcessing?: boolean;
  onSend: () => void;
  onCancel: () => void;
  isCompact?: boolean;
  className?: string;
}

export function RecordingOverlay({
  isRecording,
  isProcessing = false,
  onSend,
  onCancel,
  isCompact = false,
  className
}: RecordingOverlayProps) {
  const [duration, setDuration] = useState(0);
  const startTimeRef = useRef<number>(0);
  const animationFrameRef = useRef<number>();

  useEffect(() => {
    if (isRecording && !isProcessing) {
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
  }, [isRecording, isProcessing]);

  const formatTime = (seconds: number) => {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, "0")}`;
  };

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

  const renderWaveformBars = () => {
    return waveformBars.map((bar, i) => (
      <div
        key={i}
        className="flex-shrink-0 bg-primary/40 rounded-full"
        style={{
          width: "2px",
          height: `${bar.height}%`,
          animation: isRecording
            ? `pulse ${bar.animationDuration}s ease-in-out ${bar.animationDelay}s infinite`
            : "none",
          transition: "height 0.3s ease-out"
        }}
      />
    ));
  };

  if (!isRecording) return null;

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
        `}
      </style>

      <div className="w-full h-full rounded-lg bg-background/95 backdrop-blur-sm border border-primary/20 relative overflow-hidden flex flex-col items-center justify-center p-4">
        {/* Top buttons - Cancel on left, Send on right */}
        <div className="absolute top-3 left-3 right-3 flex justify-between">
          <Button
            onClick={onCancel}
            variant="ghost"
            size="icon"
            className="rounded-full hover:bg-muted"
            aria-label="Cancel recording"
            disabled={isProcessing}
          >
            <X className="h-4 w-4" />
          </Button>

          <Button
            onClick={onSend}
            size={isCompact ? "icon" : "sm"}
            className={cn(isCompact ? "rounded-full" : "gap-1.5")}
            aria-label="Send recording"
            disabled={isProcessing}
          >
            {isProcessing ? (
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
        </div>

        <div className="flex flex-col items-center gap-6 max-w-md w-full">
          {/* Waveform visualization - only show when not compact */}
          {!isCompact && (
            <div className="flex items-center justify-center h-12 w-full gap-0.5 px-4">
              {renderWaveformBars()}
            </div>
          )}

          {/* Timer */}
          <div className="text-2xl font-mono text-muted-foreground">{formatTime(duration)}</div>

          {/* Status indicator - only show when not compact */}
          {!isCompact && (
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              {isProcessing ? (
                <>
                  <Loader2 className="w-4 h-4 animate-spin" />
                  Processing...
                </>
              ) : (
                <>
                  <div className="w-2 h-2 bg-destructive rounded-full animate-pulse" />
                  Recording
                </>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
