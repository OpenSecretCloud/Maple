import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { MARKETING_DOWNLOADS_URL } from "@/config/domains";
import { useNotification } from "@/contexts/NotificationContext";
import { updateService, type PreparedUpdate, type UpdateService } from "@/services/updateService";
import { openExternalUrl } from "@/utils/openUrl";
import { isTauriDesktop } from "@/utils/platform";

interface UpdateFailedPayload {
  version: string;
}

interface UpdateEventListenerProps {
  service?: UpdateService;
  isDesktop?: boolean;
  listenEvent?: typeof listen;
}

export function UpdateEventListener({
  service = updateService,
  isDesktop = isTauriDesktop(),
  listenEvent = listen
}: UpdateEventListenerProps = {}) {
  const { showNotification } = useNotification();

  useEffect(() => {
    if (!isDesktop) {
      return;
    }

    let disposed = false;
    let preparedUpdateGeneration = 0;
    let unlistenPreparedUpdate: (() => void) | null = null;
    let unlistenUpdateFailed: (() => void) | null = null;
    let unlistenManualCheckUpToDate: (() => void) | null = null;
    let unlistenManualCheckFailed: (() => void) | null = null;

    const installPendingUpdate = async (version: string) => {
      try {
        const prepared = await service.installPreparedUpdate(version);
        if (!disposed) {
          if (prepared.status === "ready_to_restart") {
            showPreparedUpdate(prepared);
          } else {
            // A coalesced caller can observe the first install's retryable
            // failure as the still-prepared native update.
            showUpdateInstallFailed(prepared.version);
          }
        }
      } catch (error) {
        console.error("Failed to install pending update:", error);
        if (disposed) return;

        const reconciliationGeneration = preparedUpdateGeneration;
        try {
          const currentPrepared = await service.getPreparedUpdate();
          if (disposed || preparedUpdateGeneration !== reconciliationGeneration) return;

          if (currentPrepared?.status === "ready_to_install") {
            showUpdateInstallFailed(currentPrepared.version);
          } else if (currentPrepared?.status === "ready_to_restart") {
            showUpdateReady(currentPrepared.version);
          } else {
            showNotification({
              type: "error",
              title: "Update Action Unavailable",
              message: "Open Settings and check for updates again.",
              duration: 8000
            });
          }
        } catch (preparedError) {
          console.error("Failed to reconcile updater state:", preparedError);
          if (!disposed && preparedUpdateGeneration === reconciliationGeneration) {
            showNotification({
              type: "error",
              title: "Couldn't Confirm the Update",
              message: "Open Settings and check for updates again.",
              duration: 8000
            });
          }
        }
      }
    };

    const restartForInstalledUpdate = async (version: string) => {
      try {
        await service.restartForUpdate();
      } catch (error) {
        console.error("Failed to restart for update:", error);
        showNotification({
          type: "error",
          title: "Couldn't Restart Maple",
          message: `Version ${version} is installed. Quit and reopen Maple to finish updating, or try again.`,
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
              label: "Try Again",
              variant: "primary",
              onClick: () => {
                void restartForInstalledUpdate(version);
              }
            }
          ]
        });
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
              void openExternalUrl(MARKETING_DOWNLOADS_URL);
            }
          },
          {
            label: "Try Again",
            variant: "primary",
            onClick: () => {
              void installPendingUpdate(version);
            }
          }
        ]
      });
    };

    const showUpdateAvailable = (version: string, requiresSystemApproval: boolean) => {
      showNotification({
        type: "update",
        title: "Update Ready",
        message: requiresSystemApproval
          ? `Maple downloaded and verified version ${version}. Your system will ask for approval to install it.`
          : `Maple downloaded and verified version ${version}. Install it when you're ready.`,
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
              void installPendingUpdate(version);
            }
          }
        ]
      });
    };

    const showUpdateReady = (version: string) => {
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
            onClick: () => {
              void restartForInstalledUpdate(version);
            }
          }
        ]
      });
    };

    const showPreparedUpdate = (prepared: PreparedUpdate) => {
      preparedUpdateGeneration += 1;
      if (prepared.status === "ready_to_install") {
        showUpdateAvailable(prepared.version, prepared.requires_system_approval);
      } else {
        showUpdateReady(prepared.version);
      }
    };

    const setupListeners = async () => {
      try {
        const unlistenPrepared = await service.subscribePreparedUpdates(showPreparedUpdate);
        if (disposed) {
          unlistenPrepared();
          return;
        }
        unlistenPreparedUpdate = unlistenPrepared;
      } catch (error) {
        console.error("Failed to subscribe to prepared updates:", error);
      }
      if (disposed) return;

      try {
        const unlistenFailed = await listenEvent<UpdateFailedPayload>("update-failed", (event) => {
          showUpdateInstallFailed(event.payload.version);
        });
        if (disposed) {
          unlistenFailed();
          return;
        }
        unlistenUpdateFailed = unlistenFailed;
      } catch (error) {
        console.error("Failed to subscribe to update install failures:", error);
      }
      if (disposed) return;

      try {
        const unlistenUpToDate = await listenEvent("manual-update-check-up-to-date", () => {
          showNotification({
            type: "success",
            title: "Maple Is Up to Date",
            message: "You're running the latest available version.",
            duration: 5000
          });
        });
        if (disposed) {
          unlistenUpToDate();
          return;
        }
        unlistenManualCheckUpToDate = unlistenUpToDate;
      } catch (error) {
        console.error("Failed to subscribe to manual update success:", error);
      }
      if (disposed) return;

      try {
        const unlistenCheckFailed = await listenEvent("manual-update-check-failed", () => {
          showNotification({
            type: "error",
            title: "Couldn't Check for Updates",
            message: "Try again; if it keeps failing, use the latest installer.",
            duration: 8000
          });
        });
        if (disposed) {
          unlistenCheckFailed();
          return;
        }
        unlistenManualCheckFailed = unlistenCheckFailed;
      } catch (error) {
        console.error("Failed to subscribe to manual update failures:", error);
      }
      if (disposed) return;

      try {
        const queryGeneration = preparedUpdateGeneration;
        const prepared = await service.getPreparedUpdate();
        if (!disposed && prepared && preparedUpdateGeneration === queryGeneration) {
          showPreparedUpdate(prepared);
        }
      } catch (error) {
        console.error("Failed to rehydrate prepared update state:", error);
      }
    };

    setupListeners();

    return () => {
      disposed = true;
      if (unlistenPreparedUpdate) unlistenPreparedUpdate();
      if (unlistenUpdateFailed) unlistenUpdateFailed();
      if (unlistenManualCheckUpToDate) unlistenManualCheckUpToDate();
      if (unlistenManualCheckFailed) unlistenManualCheckFailed();
    };
  }, [isDesktop, listenEvent, service, showNotification]);

  return null;
}
