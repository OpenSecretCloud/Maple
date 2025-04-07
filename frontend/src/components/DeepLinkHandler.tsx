import { useEffect } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// For direct deep link handling, we'll listen to our custom event
// If we had the types installed, we would use:
// import { onOpenUrl } from '@tauri-apps/plugin-deep-link';

export function DeepLinkHandler() {
  useEffect(() => {
    let unlisten: (() => void) | undefined;

    const setupDeepLinkHandling = async () => {
      try {
        if (await isTauri()) {
          // Listen for the custom event we emit from Rust
          unlisten = await listen<string>("deep-link-received", (event) => {
            const url = event.payload;
            console.log("[Deep Link] Received URL:", url);

            try {
              // Parse the URL to extract the tokens
              const urlObj = new URL(url);
              // The URL will look like: cloud.opensecret.maple://auth?access_token=...&refresh_token=...
              const accessToken = urlObj.searchParams.get("access_token");
              const refreshToken = urlObj.searchParams.get("refresh_token");

              if (accessToken && refreshToken) {
                console.log("[Deep Link] Auth tokens received");

                // Store the tokens in localStorage with consistent naming
                localStorage.setItem("access_token", accessToken);
                localStorage.setItem("refresh_token", refreshToken);

                // Refresh the app state to reflect the logged-in status
                window.location.href = "/"; // Reload the app
              } else {
                console.error("[Deep Link] Missing tokens in deep link");
              }
            } catch (error) {
              console.error("[Deep Link] Failed to process deep link:", error);
            }
          });

          console.log("[Deep Link] Handler setup complete");
        }
      } catch (error) {
        console.error("[Deep Link] Setup failed:", error);
      }
    };

    setupDeepLinkHandling();

    // Return cleanup function
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  return null; // This component doesn't render anything
}
