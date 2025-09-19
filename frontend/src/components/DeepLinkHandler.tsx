import { useEffect } from "react";
import { isTauri } from "@/utils/platform";
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
          console.log("[Deep Link] Setting up handler for Tauri app");

          // Listen for the custom event we emit from Rust
          unlisten = await listen<string>("deep-link-received", (event) => {
            const url = event.payload;
            console.log("[Deep Link] Received URL:", url);

            try {
              // Parse the URL to extract parameters
              const urlObj = new URL(url);
              // The URL path structure will be: cloud.opensecret.maple://path?params
              const pathParts = urlObj.pathname.split("/").filter(Boolean);
              const firstPathPart = pathParts[0] || "";

              // Handle different types of deep links
              if (firstPathPart === "auth" || firstPathPart === "") {
                // Handle auth deep links
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
                  console.error("[Deep Link] Missing tokens in auth deep link");
                }
              } else if (
                firstPathPart === "payment" ||
                firstPathPart === "payment-success" ||
                firstPathPart === "payment-canceled" ||
                urlObj.searchParams.has("payment_success") ||
                urlObj.searchParams.has("success")
              ) {
                // Handle payment deep links from various sources
                const isSuccess =
                  firstPathPart === "payment-success" ||
                  urlObj.searchParams.get("success") === "true" ||
                  urlObj.searchParams.get("payment_success") === "true";

                const isCanceled =
                  firstPathPart === "payment-canceled" ||
                  urlObj.searchParams.get("canceled") === "true" ||
                  urlObj.searchParams.has("payment_canceled");

                console.log("[Deep Link] Payment callback received:", {
                  isSuccess,
                  isCanceled,
                  path: firstPathPart,
                  source: urlObj.searchParams.get("source")
                });

                // Use window.location instead of navigate
                if (isSuccess) {
                  // Navigate to the success page or show a success message
                  window.location.href = "/pricing?success=true";
                } else if (isCanceled) {
                  // Navigate to the canceled page or show a canceled message
                  window.location.href = "/pricing?canceled=true";
                } else {
                  // Handle unknown payment status
                  console.warn("[Deep Link] Unknown payment status in callback");
                  window.location.href = "/pricing";
                }
              } else {
                console.warn("[Deep Link] Unknown deep link type:", firstPathPart);
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
