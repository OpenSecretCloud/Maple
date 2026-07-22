import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue
} from "@/components/ui/select";
import { Volume2, Download, AlertCircle, Loader2, Check, Trash2, RotateCcw } from "lucide-react";
import {
  useTTS,
  TTS_MIN_PLAYBACK_SPEED,
  TTS_MAX_PLAYBACK_SPEED,
  TTS_PLAYBACK_SPEED_STEP,
  TTS_LANGUAGE_OPTIONS,
  type TTSLanguage
} from "@/services/tts/TTSContext";

interface TTSDownloadDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  mode?: "manage" | "upgrade";
  onKeepCurrentVoice?: () => void;
}

function formatPlaybackSpeed(speed: number): string {
  return `${speed.toFixed(1)}x`;
}

export function TTSDownloadDialog({
  open,
  onOpenChange,
  mode = "manage",
  onKeepCurrentVoice
}: TTSDownloadDialogProps) {
  const {
    status,
    error,
    downloadProgress,
    downloadDetail,
    totalSizeMB,
    upgradeAvailable,
    modelsPresentButIncompatible,
    modelVersion,
    playbackSpeed,
    hasCustomPlaybackSpeed,
    ttsLanguage,
    setPlaybackSpeed,
    resetPlaybackSpeed,
    setTTSLanguage,
    startDownload,
    deleteModels
  } = useTTS();

  const isChecking = status === "checking";
  const isDownloading = status === "downloading";
  const isLoading = status === "loading";
  const isDeleting = status === "deleting";
  const isReady = status === "ready";
  const hasError = status === "error";
  const isNotAvailable = status === "not_available";
  const isProcessing = isChecking || isDownloading || isLoading || isDeleting;
  const isIncompatibleInstall = status === "upgrade_available";
  const isLegacyReady = isReady && modelVersion === "legacy" && upgradeAvailable;
  const isSupertonic3Ready = isReady && modelVersion === "supertonic3";
  const canKeepCurrentVoice = isLegacyReady && onKeepCurrentVoice !== undefined;
  const canRetryDelete = hasError && (modelVersion !== null || modelsPresentButIncompatible);
  const showDeleteRecovery = isIncompatibleInstall || canRetryDelete;
  const upgradeRequiresDelete =
    modelVersion === "legacy" || modelsPresentButIncompatible || isIncompatibleInstall;

  const handleDownload = async () => {
    if (isProcessing || upgradeAvailable) {
      return;
    }
    await startDownload();
  };

  const handleDelete = async () => {
    if (isProcessing) {
      return;
    }
    await deleteModels();
  };

  const handleUpgrade = async () => {
    if (isProcessing) {
      return;
    }

    if (upgradeRequiresDelete && !(await deleteModels())) {
      return;
    }

    await startDownload();
  };

  const phaseLabel = isChecking
    ? "Checking text-to-speech status..."
    : isLoading
      ? "Loading text-to-speech models..."
      : isDeleting
        ? "Deleting text-to-speech models..."
        : downloadDetail || "Downloading text-to-speech models...";

  if (mode === "upgrade") {
    const upgradeDescription = isSupertonic3Ready
      ? "Supertonic 3 is installed and ready to use."
      : hasError
        ? "The upgrade did not finish. You can safely retry it."
        : isProcessing
          ? "Upgrading your local voice. Please keep this window open."
          : isLegacyReady
            ? "A newer local voice is available. You can keep using your current voice or upgrade now."
            : "Maple found an older local voice model that needs to be upgraded before read-aloud can continue.";

    return (
      <Dialog open={open} onOpenChange={isProcessing ? undefined : onOpenChange}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <div className="mb-2 flex items-center gap-3">
              <div className="rounded-lg bg-primary/10 p-2 text-primary">
                <Volume2 className="h-8 w-8" aria-hidden="true" />
              </div>
              <DialogTitle>
                {isSupertonic3Ready ? "Supertonic 3 is ready" : "Supertonic 3 is available"}
              </DialogTitle>
            </div>
            <DialogDescription className="text-base">{upgradeDescription}</DialogDescription>
          </DialogHeader>

          <div className="space-y-4 py-4">
            {hasError && error && (
              <div
                className="flex items-start gap-3 rounded-lg bg-destructive/10 p-3 text-destructive"
                role="alert"
              >
                <AlertCircle className="mt-0.5 h-5 w-5 shrink-0" aria-hidden="true" />
                <div className="space-y-1">
                  <p className="text-sm font-medium">Upgrade Failed</p>
                  <p className="text-sm opacity-90">{error}</p>
                </div>
              </div>
            )}

            {isProcessing && (
              <div className="space-y-3" aria-live="polite" aria-atomic="true">
                <div
                  className="flex items-center gap-2 text-sm text-muted-foreground"
                  role="status"
                >
                  <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
                  <span>{phaseLabel}</span>
                </div>
                {(isDownloading || isLoading) && (
                  <>
                    <div
                      className="h-2.5 w-full overflow-hidden rounded-full bg-primary/20"
                      role="progressbar"
                      aria-label={isLoading ? "Loading text-to-speech models" : "Download progress"}
                      aria-valuemin={0}
                      aria-valuemax={100}
                      aria-valuenow={isLoading ? 100 : Math.round(downloadProgress)}
                    >
                      <div
                        className="h-2.5 rounded-full bg-primary transition-all duration-300 ease-out"
                        style={{ width: `${isLoading ? 100 : downloadProgress}%` }}
                      />
                    </div>
                    {isDownloading && (
                      <p className="text-center text-xs text-muted-foreground">
                        {downloadProgress.toFixed(0)}% complete
                      </p>
                    )}
                  </>
                )}
              </div>
            )}

            {!isProcessing && !isSupertonic3Ready && (
              <div className="flex items-start gap-3 rounded-lg border p-3">
                <Download className="mt-0.5 h-5 w-5 shrink-0 text-primary" aria-hidden="true" />
                <div className="space-y-1">
                  <p className="text-sm font-medium">Upgrade the local voice model</p>
                  <p className="text-sm text-muted-foreground">
                    {upgradeRequiresDelete
                      ? "Maple will remove only the older text-to-speech model files, then download"
                      : "Maple will continue downloading"}{" "}
                    Supertonic 3 (~{Math.round(totalSizeMB)} MB). Speech remains private and runs on
                    this device.
                  </p>
                </div>
              </div>
            )}

            {isSupertonic3Ready && (
              <div className="flex items-center gap-3 rounded-lg bg-maple-success/10 p-3 text-maple-success">
                <Check className="h-5 w-5 shrink-0" aria-hidden="true" />
                <p className="text-sm">Supertonic 3 is ready. Click any speaker to listen.</p>
              </div>
            )}
          </div>

          <DialogFooter className="gap-2 sm:gap-0">
            {isSupertonic3Ready ? (
              <Button onClick={() => onOpenChange(false)}>Done</Button>
            ) : isProcessing ? (
              <Button disabled variant="outline">
                <Loader2 className="mr-2 h-4 w-4 animate-spin" aria-hidden="true" />
                {isDeleting ? "Deleting..." : isLoading ? "Loading..." : "Downloading..."}
              </Button>
            ) : (
              <>
                <Button
                  variant="outline"
                  onClick={canKeepCurrentVoice ? onKeepCurrentVoice : () => onOpenChange(false)}
                >
                  {canKeepCurrentVoice ? "Keep Current Voice" : "Maybe Later"}
                </Button>
                <Button onClick={handleUpgrade} className="gap-2">
                  <Download className="h-4 w-4" aria-hidden="true" />
                  {hasError
                    ? upgradeRequiresDelete
                      ? "Try Upgrade Again"
                      : "Try Download Again"
                    : "Upgrade to Supertonic 3"}
                </Button>
              </>
            )}
          </DialogFooter>
        </DialogContent>
      </Dialog>
    );
  }

  const description = isNotAvailable
    ? "Local text-to-speech is available in the Maple desktop and iOS apps."
    : isReady && isLegacyReady
      ? "Your current local voice is ready. Supertonic 3 is available when you want to upgrade."
      : isReady
        ? "Supertonic 3 is installed and ready. You can listen to any assistant message."
        : hasError
          ? "There was an error setting up text-to-speech."
          : isProcessing
            ? "Setting up text-to-speech. Please keep this window open."
            : isIncompatibleInstall
              ? `An older local voice model is installed. Upgrade it to download the new ~${Math.round(totalSizeMB)} MB Supertonic 3 model set.`
              : `Listen to assistant messages with a one-time local download of ~${Math.round(totalSizeMB)} MB.`;

  return (
    <Dialog open={open} onOpenChange={isProcessing ? undefined : onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <div className="mb-2 flex items-center gap-3">
            <div className="rounded-lg bg-primary/10 p-2 text-primary">
              <Volume2 className="h-8 w-8" aria-hidden="true" />
            </div>
            <DialogTitle>Text-to-Speech</DialogTitle>
          </div>
          <DialogDescription className="text-base">{description}</DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4">
          {isNotAvailable && (
            <div className="flex items-start gap-3 rounded-lg bg-muted p-3 text-muted-foreground">
              <AlertCircle className="mt-0.5 h-5 w-5 shrink-0" aria-hidden="true" />
              <div className="space-y-1">
                <p className="text-sm font-medium">Maple App Required</p>
                <p className="text-sm opacity-90">
                  Use Maple on desktop or iOS to run private text-to-speech locally on your device.
                </p>
              </div>
            </div>
          )}

          {hasError && error && (
            <div
              className="flex items-start gap-3 rounded-lg bg-destructive/10 p-3 text-destructive"
              role="alert"
            >
              <AlertCircle className="mt-0.5 h-5 w-5 shrink-0" aria-hidden="true" />
              <div className="space-y-1">
                <p className="text-sm font-medium">Setup Failed</p>
                <p className="text-sm opacity-90">{error}</p>
              </div>
            </div>
          )}

          {showDeleteRecovery && (
            <div className="space-y-3 rounded-lg border p-3">
              <div className="flex items-start gap-3">
                <AlertCircle className="mt-0.5 h-5 w-5 shrink-0 text-primary" aria-hidden="true" />
                <div className="space-y-1">
                  <p className="text-sm font-medium">
                    {modelVersion === "supertonic3"
                      ? "Remove Existing Voice Files"
                      : "Supertonic 3 Upgrade Available"}
                  </p>
                  <p className="text-sm text-muted-foreground">
                    {modelVersion === "supertonic3"
                      ? "Remove the local voice files so you can download a fresh copy."
                      : "Maple found an older local voice model. Upgrade to Supertonic 3 to keep using read-aloud."}{" "}
                    This only changes text-to-speech models stored on this device.
                  </p>
                </div>
              </div>
              <Button
                variant="outline"
                size="sm"
                onClick={modelVersion === "supertonic3" ? handleDelete : handleUpgrade}
                disabled={isProcessing}
                className={
                  modelVersion === "supertonic3"
                    ? "text-destructive hover:text-destructive"
                    : "gap-2"
                }
              >
                {modelVersion === "supertonic3" ? (
                  <Trash2 className="mr-2 h-4 w-4" aria-hidden="true" />
                ) : (
                  <Download className="h-4 w-4" aria-hidden="true" />
                )}
                {modelVersion === "supertonic3" ? "Delete TTS Models" : "Upgrade to Supertonic 3"}
              </Button>
            </div>
          )}

          {isProcessing && (
            <div className="space-y-3" aria-live="polite" aria-atomic="true">
              <div className="flex items-center gap-2 text-sm text-muted-foreground" role="status">
                <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
                <span>{phaseLabel}</span>
              </div>

              {(isDownloading || isLoading) && (
                <>
                  <div
                    className="h-2.5 w-full overflow-hidden rounded-full bg-primary/20"
                    role="progressbar"
                    aria-label={isLoading ? "Loading text-to-speech models" : "Download progress"}
                    aria-valuemin={0}
                    aria-valuemax={100}
                    aria-valuenow={isLoading ? 100 : Math.round(downloadProgress)}
                  >
                    <div
                      className="h-2.5 rounded-full bg-primary transition-all duration-300 ease-out"
                      style={{ width: `${isLoading ? 100 : downloadProgress}%` }}
                    />
                  </div>
                  {isDownloading && (
                    <p className="text-center text-xs text-muted-foreground">
                      {downloadProgress.toFixed(0)}% complete
                    </p>
                  )}
                </>
              )}
            </div>
          )}

          {isReady && (
            <div className="space-y-4">
              <div className="flex items-center gap-3 rounded-lg bg-maple-success/10 p-3 text-maple-success">
                <Check className="h-5 w-5 shrink-0" aria-hidden="true" />
                <p className="text-sm">
                  {modelVersion === "supertonic3"
                    ? "Supertonic 3 is ready. Click the speaker on any assistant message to listen."
                    : "Your current voice is ready. Click the speaker on any assistant message to listen."}
                </p>
              </div>

              {modelVersion === "supertonic3" && (
                <div className="space-y-2 rounded-lg border p-3">
                  <label htmlFor="tts-language" className="text-sm font-medium">
                    Language
                  </label>
                  <Select
                    value={ttsLanguage}
                    onValueChange={(value) => setTTSLanguage(value as TTSLanguage)}
                  >
                    <SelectTrigger id="tts-language" aria-label="Text-to-speech language">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {TTS_LANGUAGE_OPTIONS.map((option) => (
                        <SelectItem key={option.code} value={option.code}>
                          {option.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <p className="text-xs text-muted-foreground">
                    Auto lets Supertonic handle the text without a specified language. Choose a
                    language to guide pronunciation when you know it.
                  </p>
                </div>
              )}

              <div className="space-y-3 rounded-lg border p-3">
                <div className="flex items-center justify-between gap-3">
                  <label htmlFor="tts-speech-speed" className="text-sm font-medium">
                    Speech speed
                  </label>
                  <output
                    htmlFor="tts-speech-speed"
                    className="text-sm tabular-nums text-muted-foreground"
                  >
                    {formatPlaybackSpeed(playbackSpeed)}
                  </output>
                </div>
                <input
                  id="tts-speech-speed"
                  type="range"
                  min={TTS_MIN_PLAYBACK_SPEED}
                  max={TTS_MAX_PLAYBACK_SPEED}
                  step={TTS_PLAYBACK_SPEED_STEP}
                  value={playbackSpeed}
                  onChange={(event) => setPlaybackSpeed(Number(event.currentTarget.value))}
                  className="h-2 w-full cursor-pointer accent-primary"
                  aria-valuetext={formatPlaybackSpeed(playbackSpeed)}
                />
                <div className="flex items-center justify-between text-xs text-muted-foreground">
                  <span>{formatPlaybackSpeed(TTS_MIN_PLAYBACK_SPEED)}</span>
                  <span>{formatPlaybackSpeed(TTS_MAX_PLAYBACK_SPEED)}</span>
                </div>
                <div className="flex justify-end">
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={resetPlaybackSpeed}
                    disabled={!hasCustomPlaybackSpeed}
                    className="gap-2"
                  >
                    <RotateCcw className="h-4 w-4" aria-hidden="true" />
                    Reset
                  </Button>
                </div>
              </div>

              {isLegacyReady ? (
                <div className="space-y-3 rounded-lg border p-3">
                  <div className="flex items-start gap-3">
                    <AlertCircle
                      className="mt-0.5 h-5 w-5 shrink-0 text-primary"
                      aria-hidden="true"
                    />
                    <div className="space-y-1">
                      <p className="text-sm font-medium">Supertonic 3 Upgrade Available</p>
                      <p className="text-sm text-muted-foreground">
                        Your current voice will keep working. Delete it when you are ready to
                        download the new ~{Math.round(totalSizeMB)} MB model set.
                      </p>
                    </div>
                  </div>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={handleUpgrade}
                    disabled={isProcessing}
                    className="gap-2"
                  >
                    <Download className="h-4 w-4" aria-hidden="true" />
                    Upgrade to Supertonic 3
                  </Button>
                </div>
              ) : (
                <div className="border-t pt-4">
                  <p className="mb-3 text-sm text-muted-foreground">
                    Models are stored locally (~{Math.round(totalSizeMB)} MB). Delete them any time
                    to free up space.
                  </p>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={handleDelete}
                    disabled={isProcessing}
                    className="text-destructive hover:text-destructive"
                  >
                    <Trash2 className="mr-2 h-4 w-4" aria-hidden="true" />
                    Delete TTS Models
                  </Button>
                </div>
              )}
            </div>
          )}

          {!isProcessing &&
            !isReady &&
            !isNotAvailable &&
            !isIncompatibleInstall &&
            !canRetryDelete && (
              <div className="space-y-3">
                <div className="space-y-2 text-sm text-muted-foreground">
                  <p className="font-medium">What you need to know:</p>
                  <ul className="ml-4 list-disc space-y-1.5">
                    <li>One-time download of ~{Math.round(totalSizeMB)} MB</li>
                    <li>Models are stored locally for future use</li>
                    <li>All processing happens on your device</li>
                    <li>Powered by Supertonic 3</li>
                  </ul>
                </div>
              </div>
            )}
        </div>

        <DialogFooter className="gap-2 sm:gap-0">
          {isReady ? (
            <Button onClick={() => onOpenChange(false)}>Close</Button>
          ) : isProcessing ? (
            <Button disabled variant="outline">
              <Loader2 className="mr-2 h-4 w-4 animate-spin" aria-hidden="true" />
              {isChecking
                ? "Checking..."
                : isLoading
                  ? "Loading..."
                  : isDeleting
                    ? "Deleting..."
                    : "Downloading..."}
            </Button>
          ) : isNotAvailable || isIncompatibleInstall || canRetryDelete ? (
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              Close
            </Button>
          ) : (
            <>
              <Button variant="outline" onClick={() => onOpenChange(false)}>
                {hasError ? "Close" : "Maybe Later"}
              </Button>
              <Button onClick={handleDownload} className="gap-2">
                <Download className="h-4 w-4" aria-hidden="true" />
                {hasError ? "Try Download Again" : `Download (~${Math.round(totalSizeMB)} MB)`}
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
