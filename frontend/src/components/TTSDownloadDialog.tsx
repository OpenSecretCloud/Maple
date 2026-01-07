import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Volume2, Download, AlertCircle, Loader2, Check, Trash2 } from "lucide-react";
import { useTTS } from "@/services/tts/TTSContext";

interface TTSDownloadDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function TTSDownloadDialog({ open, onOpenChange }: TTSDownloadDialogProps) {
  const {
    status,
    error,
    downloadProgress,
    downloadDetail,
    totalSizeMB,
    startDownload,
    deleteModels
  } = useTTS();

  const handleDownload = async () => {
    await startDownload();
  };

  const handleDelete = async () => {
    await deleteModels();
  };

  const isChecking = status === "checking";
  const isDownloading = status === "downloading";
  const isLoading = status === "loading";
  const isDeleting = status === "deleting";
  const isReady = status === "ready";
  const hasError = status === "error";
  const isNotAvailable = status === "not_available";
  const isProcessing = isChecking || isDownloading || isLoading || isDeleting;

  return (
    <Dialog open={open} onOpenChange={isProcessing ? undefined : onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <div className="flex items-center gap-3 mb-2">
            <div className="p-2 rounded-lg bg-primary/10 text-primary">
              <Volume2 className="h-8 w-8" />
            </div>
            <DialogTitle>Text-to-Speech</DialogTitle>
          </div>
          <DialogDescription className="text-base">
            {isNotAvailable
              ? "TTS is only available in the desktop app."
              : isReady
                ? "TTS is ready! You can now listen to assistant messages."
                : hasError
                  ? "There was an error setting up TTS."
                  : isProcessing
                    ? "Setting up TTS. Please keep this window open."
                    : `Listen to assistant messages with natural-sounding speech. This requires a one-time download of ~${Math.round(totalSizeMB)} MB.`}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4">
          {isNotAvailable && (
            <div className="flex items-start gap-3 p-3 rounded-lg bg-muted text-muted-foreground">
              <AlertCircle className="h-5 w-5 mt-0.5 shrink-0" />
              <div className="space-y-1">
                <p className="text-sm font-medium">Desktop App Required</p>
                <p className="text-sm opacity-90">
                  Text-to-speech requires the Maple desktop app. Download it from our website to use
                  this feature.
                </p>
              </div>
            </div>
          )}

          {hasError && error && (
            <div className="flex items-start gap-3 p-3 rounded-lg bg-destructive/10 text-destructive">
              <AlertCircle className="h-5 w-5 mt-0.5 shrink-0" />
              <div className="space-y-1">
                <p className="text-sm font-medium">Setup Failed</p>
                <p className="text-sm opacity-90">{error}</p>
              </div>
            </div>
          )}

          {isProcessing && (
            <div className="space-y-3">
              <div className="flex items-center gap-2 text-sm text-muted-foreground">
                <Loader2 className="h-4 w-4 animate-spin" />
                <span>
                  {downloadDetail ||
                    (isChecking
                      ? "Checking TTS status..."
                      : isLoading
                        ? "Loading models..."
                        : "Downloading...")}
                </span>
              </div>
              {!isChecking && (
                <div className="w-full bg-secondary rounded-full h-2.5 overflow-hidden">
                  <div
                    className="bg-primary h-2.5 rounded-full transition-all duration-300 ease-out"
                    style={{ width: `${isLoading ? 100 : downloadProgress}%` }}
                  />
                </div>
              )}
              {isDownloading && (
                <p className="text-xs text-muted-foreground text-center">
                  {downloadProgress.toFixed(0)}% complete
                </p>
              )}
            </div>
          )}

          {isDeleting && (
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Loader2 className="h-4 w-4 animate-spin" />
              <span>Deleting TTS models...</span>
            </div>
          )}

          {isReady && (
            <div className="space-y-4">
              <div className="flex items-center gap-3 p-3 rounded-lg bg-green-500/10 text-green-600 dark:text-green-400">
                <Check className="h-5 w-5 shrink-0" />
                <p className="text-sm">
                  TTS is ready! Click the speaker icon on any assistant message to listen.
                </p>
              </div>
              <div className="border-t pt-4">
                <p className="text-sm text-muted-foreground mb-3">
                  Models are stored locally (~{Math.round(totalSizeMB)} MB). You can delete them to
                  free up space.
                </p>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleDelete}
                  className="text-destructive hover:text-destructive"
                >
                  <Trash2 className="h-4 w-4 mr-2" />
                  Delete TTS Models
                </Button>
              </div>
            </div>
          )}

          {!isProcessing && !isReady && !isNotAvailable && !hasError && (
            <div className="space-y-3">
              <div className="text-sm text-muted-foreground space-y-2">
                <p className="font-medium">What you need to know:</p>
                <ul className="space-y-1.5 ml-4 list-disc">
                  <li>One-time download of ~{Math.round(totalSizeMB)} MB</li>
                  <li>Models are stored locally for future use</li>
                  <li>All processing happens on your device</li>
                  <li>Powered by Supertonic TTS</li>
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
              <Loader2 className="h-4 w-4 animate-spin mr-2" />
              {isChecking
                ? "Checking..."
                : isLoading
                  ? "Loading..."
                  : isDeleting
                    ? "Deleting..."
                    : "Downloading..."}
            </Button>
          ) : isNotAvailable ? (
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              Close
            </Button>
          ) : (
            <>
              <Button variant="outline" onClick={() => onOpenChange(false)}>
                Maybe Later
              </Button>
              <Button onClick={handleDownload} className="gap-2">
                <Download className="h-4 w-4" />
                Download (~{Math.round(totalSizeMB)} MB)
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
