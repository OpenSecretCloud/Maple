import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useNotification } from "@/contexts/NotificationContext";
import { isTauri } from "@/utils/platform";

interface UpdateReadyPayload {
  version: string;
}

export function UpdateEventListener() {
  const { showNotification } = useNotification();

  useEffect(() => {
    if (!isTauri()) {
      return;
    }

    let unlistenUpdateReady: (() => void) | null = null;

    const setupListeners = async () => {
      try {
        unlistenUpdateReady = await listen<UpdateReadyPayload>("update-ready", (event) => {
          const { version } = event.payload;
          showNotification({
            type: "update",
            title: "Update Ready",
            message: `Version ${version} has been downloaded and is ready to install.`,
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
      } catch (error) {
        console.error("Failed to setup update event listeners:", error);
      }
    };

    setupListeners();

    return () => {
      if (unlistenUpdateReady) unlistenUpdateReady();
    };
  }, [showNotification]);

  return null;
}
