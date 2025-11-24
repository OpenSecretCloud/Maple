import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useNotification } from "@/contexts/NotificationContext";
import { Server } from "lucide-react";
import { isTauri } from "@/utils/platform";

export function ProxyEventListener() {
  const { showNotification } = useNotification();

  useEffect(() => {
    // Only setup listeners if running on Tauri (not web)
    if (!isTauri()) {
      return;
    }

    let unlistenAutoStarted: (() => void) | null = null;
    let unlistenAutoStartFailed: (() => void) | null = null;

    const setupListeners = async () => {
      try {
        // Listen for proxy auto-start success
        unlistenAutoStarted = await listen("proxy-autostarted", (event) => {
          const config = event.payload as { host: string; port: number };
          showNotification({
            type: "success",
            title: "Proxy Started",
            message: `Local proxy is running on ${config.host}:${config.port}`,
            icon: <Server className="h-5 w-5 text-green-600 dark:text-green-500" />,
            duration: 5000
          });
        });

        // Listen for proxy auto-start failure
        unlistenAutoStartFailed = await listen("proxy-autostart-failed", (event) => {
          const error = event.payload as string;
          showNotification({
            type: "error",
            title: "Proxy Auto-Start Failed",
            message: error || "Failed to start the proxy automatically",
            duration: 7000
          });
        });
      } catch (error) {
        console.error("Failed to setup proxy event listeners:", error);
      }
    };

    setupListeners();

    // Cleanup listeners on unmount
    return () => {
      if (unlistenAutoStarted) unlistenAutoStarted();
      if (unlistenAutoStartFailed) unlistenAutoStartFailed();
    };
  }, [showNotification]);

  return null; // This component doesn't render anything
}
