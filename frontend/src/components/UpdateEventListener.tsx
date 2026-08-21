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
  // True when the user approved the install and it did not complete (for
  // example a cancelled password prompt). The download is kept for a retry.
  retryable?: boolean;
}

interface UpdateAvailablePayload {
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
    let unlistenUpdateAvailable: (() => void) | null = null;

    const installPendingUpdate = async () => {
      try {
        await invoke("install_pending_update");
      } catch (error) {
        // The backend emits update-failed for install errors.
        console.error("Failed to install pending update:", error);
      }
    };

    const showUpdateInstallFailed = (version: string) => {
      showNotification({
        type: "error",
        title: "Update Not Installed",
        message: `Maple couldn't install version ${version}. Try again, or download the latest installer to update manually.`,
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
            variant: "secondary",
            onClick: () => {
              void openExternalUrl("https://trymaple.ai/downloads");
            }
          },
          {
            label: "Try Again",
            variant: "primary",
            onClick: () => {
              void installPendingUpdate();
            }
          }
        ]
      });
    };

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

    // Linux deb/rpm installs open a system password prompt, so the backend
    // downloads the update and waits for the user to approve the install.
    const showUpdateAvailable = (version: string) => {
      showNotification({
        type: "update",
        title: "Update Available",
        message: `Maple downloaded version ${version}. Your system will ask for your password to install it.`,
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
            label: "Install Now",
            variant: "primary",
            onClick: () => {
              void installPendingUpdate();
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
          const { version, retryable } = event.payload;
          if (retryable) {
            showUpdateInstallFailed(version);
          } else {
            showUpdateFailed(version);
          }
        });
        if (disposed) {
          unlistenFailed();
          return;
        }
        unlistenUpdateFailed = unlistenFailed;

        const unlistenAvailable = await listen<UpdateAvailablePayload>(
          "update-available",
          (event) => {
            const { version } = event.payload;
            showUpdateAvailable(version);
          }
        );
        if (disposed) {
          unlistenAvailable();
          return;
        }
        unlistenUpdateAvailable = unlistenAvailable;

        const pendingFailure = await invoke<string | null>("get_pending_update_failure");
        if (!disposed && pendingFailure) {
          showUpdateFailed(pendingFailure);
          return;
        }

        const pendingInstall = await invoke<string | null>("get_pending_update_install");
        if (!disposed && pendingInstall) {
          showUpdateAvailable(pendingInstall);
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
      if (unlistenUpdateAvailable) unlistenUpdateAvailable();
    };
  }, [showNotification]);

  return null;
}
