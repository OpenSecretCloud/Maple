import { useEffect, useRef } from "react";
import { importTransportV2AuthBundle, useOpenSecret } from "@opensecret/react";
import { isTauri } from "@/utils/platform";
import { listen } from "@tauri-apps/api/event";
import { getSafeInternalRedirect } from "@/utils/internalRedirect";
import {
  authorizeNativeOAuthCallback,
  isNativeOAuthAttemptId
} from "@/services/nativeOAuthAttempt";
import { TRANSPORT_V2_NATIVE_ATTEMPT_QUERY } from "@/services/desktopOAuthTransport";

// For direct deep link handling, we'll listen to our custom event
// If we had the types installed, we would use:
// import { onOpenUrl } from '@tauri-apps/plugin-deep-link';

export function DeepLinkHandler() {
  const os = useOpenSecret();
  const isAuthenticatedRef = useRef(false);
  isAuthenticatedRef.current = Boolean(os.auth.user);

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    const setupDeepLinkHandling = async () => {
      try {
        if (isTauri()) {
          console.log("[Deep Link] Setting up handler for Tauri app");

          // Listen for the custom event we emit from Rust
          unlisten = await listen<string>("deep-link-received", (event) => {
            const url = event.payload;
            console.log("[Deep Link] Received callback");

            void (async () => {
              try {
                // Parse the URL to extract parameters
                const urlObj = new URL(url);
                // The URL path structure will be: cloud.opensecret.maple://path?params
                const pathParts = urlObj.pathname.split("/").filter(Boolean);
                // Custom-scheme links normally encode the action as the host
                // (`cloud.opensecret.maple://auth`). Retain path support for
                // the existing triple-slash form without treating every empty
                // path as an authentication callback.
                const firstPathPart = pathParts[0] || urlObj.hostname;

                // Handle different types of deep links
                if (firstPathPart === "auth") {
                  // Handle auth deep links
                  const authBundle = urlObj.searchParams.get("auth_bundle");
                  const nativeOAuthAttemptId = urlObj.searchParams.get(
                    TRANSPORT_V2_NATIVE_ATTEMPT_QUERY
                  );
                  const next = urlObj.searchParams.get("next");
                  const safeNext = getSafeInternalRedirect(next) ?? "/";

                  if (authBundle && isNativeOAuthAttemptId(nativeOAuthAttemptId)) {
                    const authorization = authorizeNativeOAuthCallback(
                      isAuthenticatedRef.current,
                      nativeOAuthAttemptId
                    );
                    if (authorization === "already_authenticated") {
                      console.warn("[Deep Link] Ignoring auth callback for an existing session");
                      return;
                    }
                    if (authorization === "attempt_mismatch") {
                      console.warn("[Deep Link] Ignoring auth callback with mismatched state");
                      return;
                    }
                    if (authorization === "missing_or_expired_attempt") {
                      console.warn("[Deep Link] Ignoring unsolicited or expired auth callback");
                      return;
                    }

                    await importTransportV2AuthBundle(
                      authBundle,
                      import.meta.env.VITE_OPEN_SECRET_API_URL
                    );
                    console.log("[Deep Link] Authentication bundle accepted");

                    // Refresh the app state to reflect the logged-in status
                    window.location.href = safeNext; // Reload the app at the requested internal route
                  } else {
                    // Check required shape before consuming the one-time callback
                    // marker so a truncated URL cannot invalidate a later retry.
                    console.error("[Deep Link] Authentication callback is missing required state");
                  }
                } else if (
                  firstPathPart === "payment" ||
                  firstPathPart === "payment-success" ||
                  firstPathPart === "payment-success-credits" ||
                  firstPathPart === "payment-canceled" ||
                  urlObj.searchParams.has("payment_success") ||
                  urlObj.searchParams.has("success") ||
                  urlObj.searchParams.has("canceled") ||
                  urlObj.searchParams.has("payment_canceled")
                ) {
                  // Handle payment deep links from various sources
                  const isSuccess =
                    firstPathPart === "payment-success" ||
                    firstPathPart === "payment-success-credits" ||
                    urlObj.searchParams.get("success") === "true" ||
                    urlObj.searchParams.get("payment_success") === "true";

                  const isCreditSuccess = firstPathPart === "payment-success-credits";

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
                  if (isCreditSuccess) {
                    // Keep the established root callback contract; the home route bridges it into
                    // the dedicated API credits settings page.
                    window.location.href = "/?credits_success=true";
                  } else if (isSuccess) {
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
            })();
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
