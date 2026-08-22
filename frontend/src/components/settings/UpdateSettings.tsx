import { useEffect, useRef, useState } from "react";
import { RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  updateService,
  type UpdateCheckResult,
  type UpdaterPreferences,
  type UpdateService
} from "@/services/updateService";
import { cn } from "@/utils/utils";
import { SettingsSection } from "./SettingsPage";

function updateCheckMessage(result: UpdateCheckResult): string {
  switch (result.status) {
    case "up_to_date":
      return "Maple is up to date.";
    case "ready_to_restart":
      return `Version ${result.version} is installed. Restart Maple to finish updating.`;
    case "ready_to_install":
      return `Version ${result.version} is downloaded and ready to install.`;
    case "install_failed":
      return `Maple couldn't install version ${result.version}. Try checking again or use the latest installer.`;
    case "automatic_updates_disabled":
      return "Automatic updates are off. You can still check for updates manually.";
  }
}

export function UpdateSettings({ service = updateService }: { service?: UpdateService }) {
  const mountedRef = useRef(false);
  const [preferences, setPreferences] = useState<UpdaterPreferences | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [isChecking, setIsChecking] = useState(false);
  const [preferenceError, setPreferenceError] = useState<string | null>(null);
  const [preferenceStatus, setPreferenceStatus] = useState<string | null>(null);
  const [checkError, setCheckError] = useState<string | null>(null);
  const [checkStatus, setCheckStatus] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const [loadAttempt, setLoadAttempt] = useState(0);

  useEffect(() => {
    mountedRef.current = true;

    const loadPreferences = async () => {
      try {
        const loaded = await service.loadPreferences();
        if (mountedRef.current) {
          setPreferences(loaded);
          setLoadFailed(false);
          setLoadError(null);
        }
      } catch (loadError) {
        console.error("Failed to load updater preferences:", loadError);
        if (mountedRef.current) {
          setPreferences(null);
          setLoadFailed(true);
          setLoadError(
            "Maple couldn't confirm your update preference. Automatic update behavior is unchanged. Try again, or turn automatic updates off now."
          );
        }
      } finally {
        if (mountedRef.current) setIsLoading(false);
      }
    };

    void loadPreferences();
    return () => {
      mountedRef.current = false;
    };
  }, [service, loadAttempt]);

  const setAutomaticUpdates = async (automaticUpdates: boolean) => {
    if (isSaving) return;

    const next = { automatic_updates: automaticUpdates };
    setIsSaving(true);
    setPreferenceError(null);
    setPreferenceStatus(null);

    try {
      await service.savePreferences(next);
      if (mountedRef.current) {
        setPreferences(next);
        setLoadFailed(false);
        setLoadError(null);
        setPreferenceStatus(
          automaticUpdates
            ? "Automatic updates are on. Maple will check shortly after launch and every hour."
            : "Automatic updates are off. You can still check for updates manually."
        );
      }
    } catch (saveError) {
      console.error("Failed to save updater preferences:", saveError);
      if (mountedRef.current) {
        setPreferenceError("Maple couldn't save your update preference. Please try again.");
      }
    } finally {
      if (mountedRef.current) setIsSaving(false);
    }
  };

  const retryLoadingPreferences = () => {
    setIsLoading(true);
    setLoadError(null);
    setPreferenceStatus(null);
    setLoadAttempt((attempt) => attempt + 1);
  };

  const checkForUpdates = async () => {
    if (isChecking) return;

    setIsChecking(true);
    setCheckError(null);
    setCheckStatus(null);

    try {
      const result = await service.checkForUpdates();
      if (mountedRef.current) {
        if (result.status === "install_failed") {
          setCheckError(updateCheckMessage(result));
        } else {
          setCheckStatus(updateCheckMessage(result));
        }
      }
    } catch (checkError) {
      console.error("Failed to check for updates:", checkError);
      if (mountedRef.current) {
        setCheckError(
          "Maple couldn't complete the update check. Try again; if it keeps failing, use the latest installer."
        );
      }
    } finally {
      if (mountedRef.current) setIsChecking(false);
    }
  };

  return (
    <SettingsSection
      title="Updates"
      description="Choose whether Maple checks for updates in the background, or check whenever you want."
    >
      <div className="space-y-5">
        <div className="flex items-start justify-between gap-4">
          <div>
            <Label htmlFor="automatic-updates">Automatic updates</Label>
            <p
              id="automatic-updates-description"
              className="mt-1 text-xs leading-relaxed text-muted-foreground"
            >
              With this on, Maple checks shortly after launch and every hour. On macOS, Windows, and
              AppImage, available updates download and install automatically. Linux deb/rpm packages
              keep their current install prompt.
            </p>
          </div>
          <Switch
            id="automatic-updates"
            checked={preferences?.automatic_updates ?? false}
            onCheckedChange={(checked) => void setAutomaticUpdates(checked)}
            disabled={isLoading || isSaving || !preferences}
            aria-describedby="automatic-updates-description"
            aria-busy={isSaving}
          />
        </div>

        <div className="flex flex-col gap-3 border-t border-border/70 pt-4 sm:flex-row sm:items-center sm:justify-between">
          <p className="text-xs leading-relaxed text-muted-foreground">
            A manual check can download and install an available update immediately on macOS,
            Windows, and AppImage, even when automatic updates are off. Linux deb/rpm packages keep
            their install prompt.
          </p>
          <Button
            type="button"
            variant="outline"
            onClick={() => void checkForUpdates()}
            disabled={isChecking}
            aria-busy={isChecking}
          >
            <RefreshCw
              aria-hidden="true"
              className={cn(
                "mr-2 h-4 w-4",
                isChecking && "animate-spin motion-reduce:animate-none"
              )}
            />
            {isChecking ? "Checking..." : "Check for updates"}
          </Button>
        </div>

        {isLoading && (
          <p className="text-xs text-muted-foreground" role="status">
            Loading update preference...
          </p>
        )}
        {preferenceStatus && (
          <p className="text-xs leading-relaxed text-muted-foreground" role="status">
            {preferenceStatus}
          </p>
        )}
        {checkStatus && (
          <p className="text-xs leading-relaxed text-muted-foreground" role="status">
            {checkStatus}
          </p>
        )}
        {loadError && (
          <div className="space-y-3" role="alert">
            <p className="text-xs leading-relaxed text-destructive">{loadError}</p>
            {loadFailed && (
              <div className="flex flex-wrap gap-2">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={retryLoadingPreferences}
                  disabled={isLoading || isSaving}
                >
                  Retry preference
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => void setAutomaticUpdates(false)}
                  disabled={isSaving}
                >
                  Turn automatic updates off
                </Button>
              </div>
            )}
          </div>
        )}
        {preferenceError && (
          <p className="text-xs leading-relaxed text-destructive" role="alert">
            {preferenceError}
          </p>
        )}
        {checkError && (
          <p className="text-xs leading-relaxed text-destructive" role="alert">
            {checkError}
          </p>
        )}
      </div>
    </SettingsSection>
  );
}
