import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useNotification } from "@/contexts/NotificationContext";
import { openExternalUrl } from "@/utils/openUrl";
import { isTauri } from "@/utils/platform";

interface UpdateReadyPayload {
  version: string;
}

interface UpdateFailedPayload {
  version: string;
}

export function UpdateEventListener() {
  const { showNotification } = useNotification();

  useEffect(() => {
    if (!isTauri()) {
      return;
    }

    let disposed = false;
    let unlistenUpdateReady: (() => void) | null = null;
    let unlistenUpdateFailed: (() => void) | null = null;

    const showUpdateFailed = (version: string) => {
      showNotification({
        type: "error",
        title: "Update Failed",
        message: `Maple couldn't install version ${version} automatically. Download the latest installer to update manually.`,
        duration: 0,
        actions: [
          {
            label: "Later",
            variant: "secondary",
            onClick: () => {
              // Just dismiss - the notification will close automatically
            }
          },
          {
            label: "Download Manually",
            variant: "primary",
            onClick: () => {
              void openExternalUrl("https://trymaple.ai/downloads");
            }
          }
        ]
      });
    };

    const setupListeners = async () => {
      try {
        const unlistenReady = await listen<UpdateReadyPayload>("update-ready", (event) => {
          const { version } = event.payload;
          showNotification({
            type: "update",
            title: "Update Installed",
            message: `Version ${version} has been installed. Restart Maple to finish updating.`,
            duration: 0,
            actions: [
              {
                label: "Later",
                variant: "secondary",
                onClick: () => {
                  // Just dismiss - the notification will close automatically
                }
              },
              {
                label: "Restart Now",
                variant: "primary",
                onClick: async () => {
                  try {
                    await invoke("restart_for_update");
                  } catch (error) {
                    console.error("Failed to restart for update:", error);
                  }
                }
              }
            ]
          });
        });
        if (disposed) {
          unlistenReady();
          return;
        }
        unlistenUpdateReady = unlistenReady;

        const unlistenFailed = await listen<UpdateFailedPayload>("update-failed", (event) => {
          const { version } = event.payload;
          showUpdateFailed(version);
        });
        if (disposed) {
          unlistenFailed();
          return;
        }
        unlistenUpdateFailed = unlistenFailed;

        const pendingFailure = await invoke<string | null>("get_pending_update_failure");
        if (!disposed && pendingFailure) {
          showUpdateFailed(pendingFailure);
        }
      } catch (error) {
        console.error("Failed to setup update event listeners:", error);
      }
    };

    setupListeners();

    return () => {
      disposed = true;
      if (unlistenUpdateReady) unlistenUpdateReady();
      if (unlistenUpdateFailed) unlistenUpdateFailed();
    };
  }, [showNotification]);

  return null;
}
