import React, { useEffect, useRef } from "react";
import { useOpenSecret } from "@opensecret/react";
import { v4 as uuidv4 } from "uuid";
import { sha256 } from "@noble/hashes/sha256";
import { bytesToHex } from "@noble/hashes/utils";
import { Button } from "./ui/button";
import { Apple } from "./icons/Apple";
import { getBillingService } from "@/billing/billingService";

// Define the props interface
interface AppleAuthProviderProps {
  onSuccess?: () => void;
  onError?: (error: Error) => void;
  inviteCode?: string;
  redirectAfterLogin?: (plan?: string) => void;
  selectedPlan?: string;
  className?: string;
  children?: React.ReactNode;
}

// Define AppleID interface which will be added to window
declare global {
  interface Window {
    AppleID: {
      auth: {
        init: (config: {
          clientId: string;
          scope: string;
          redirectURI: string;
          state: string;
          nonce: string;
          usePopup: boolean;
        }) => void;
        signIn: () => Promise<{
          authorization: {
            code: string;
            state: string;
            id_token?: string;
          };
        }>;
      };
    };
  }
}

// Define Apple Sign In event types
interface AppleSignInAuthorizationData {
  code: string;
  state: string;
  id_token?: string;
}

interface AppleSignInSuccessEventDetail {
  data?: {
    authorization?: AppleSignInAuthorizationData;
  };
  authorization?: AppleSignInAuthorizationData;
  code?: string;
  state?: string;
  id_token?: string;
}

interface AppleSignInSuccessEvent extends Event {
  detail: AppleSignInSuccessEventDetail | string;
  authorization?: AppleSignInAuthorizationData;
}

interface AppleSignInFailureEvent extends Event {
  detail: {
    error: string;
  };
}

export function AppleAuthProvider({
  onSuccess,
  onError,
  inviteCode = "",
  redirectAfterLogin,
  selectedPlan,
  className,
  children
}: AppleAuthProviderProps) {
  const os = useOpenSecret();
  const appleScriptLoaded = useRef(false);
  const appleAuthInitialized = useRef(false);
  const rawNonceRef = useRef<string>("");
  const isInitializing = useRef(false);

  // Load Apple Sign In JS SDK on mount (but don't initialize auth yet)
  useEffect(() => {
    // Don't load the script multiple times
    if (appleScriptLoaded.current) return;

    // Skip if we're in a Tauri environment (will use native flow on iOS)
    if (window.location.protocol === "tauri:") return;

    // Load Apple Sign In JS SDK
    const script = document.createElement("script");
    script.src =
      "https://appleid.cdn-apple.com/appleauth/static/jsapi/appleid/1/en_US/appleid.auth.js";
    script.async = true;
    script.onload = () => {
      // SDK loaded successfully
    };
    document.head.appendChild(script);

    appleScriptLoaded.current = true;

    return () => {
      // Clean up script on unmount if needed
      if (script.parentNode) {
        script.parentNode.removeChild(script);
      }
    };
  }, []);

  const initializeAppleAuth = async () => {
    if (!window.AppleID) {
      console.error("[Apple Auth] AppleID JS SDK not loaded");
      return;
    }

    // Prevent double initialization
    if (isInitializing.current || appleAuthInitialized.current) {
      return;
    }
    isInitializing.current = true;

    try {
      try {
        // First we need to get the proper state and auth URL from the backend
        const initiateResult = await os.initiateAppleAuth(inviteCode || "");

        // Generate the nonce for Apple
        rawNonceRef.current = uuidv4();
        const hashedNonce = bytesToHex(sha256(new TextEncoder().encode(rawNonceRef.current)));

        // Store the raw nonce in sessionStorage to access it during callback
        sessionStorage.setItem("apple_auth_nonce", rawNonceRef.current);

        // Store the state from the backend for CSRF validation
        const state = initiateResult.state || "";
        sessionStorage.setItem("apple_auth_state", state);

        // Store selected plan if present
        if (selectedPlan) {
          sessionStorage.setItem("selected_plan", selectedPlan);
        }

        // Initialize Apple auth with required parameters
        window.AppleID.auth.init({
          clientId: "cloud.opensecret.maple.services", // Apple Services ID
          scope: "name email",
          redirectURI: window.location.origin + "/auth/apple/callback",
          state: state, // Use the state from the backend
          nonce: hashedNonce,
          usePopup: true // Using popup to capture authentication on client side
        });
      } catch (error) {
        console.error("[Apple Auth] Failed to initialize:", error);
        if (onError && error instanceof Error) {
          onError(error);
        }
      }

      // Add event listeners for Apple Sign In response
      document.addEventListener("AppleIDSignInOnSuccess", async (event) => {
        // Handle successful response
        try {
          // Cast event to AppleSignInSuccessEvent
          const appleEvent = event as AppleSignInSuccessEvent;

          // Access the data - the structure might vary
          let code, state;

          // Different versions of Apple Sign In JS SDK might structure the data differently
          if (
            typeof appleEvent.detail === "object" &&
            appleEvent.detail.data?.authorization?.code
          ) {
            // Standard structure
            code = appleEvent.detail.data.authorization.code;
            state = appleEvent.detail.data.authorization.state;
          } else if (
            typeof appleEvent.detail === "object" &&
            appleEvent.detail.authorization?.code
          ) {
            // Alternative structure
            code = appleEvent.detail.authorization.code;
            state = appleEvent.detail.authorization.state;
          } else if (typeof appleEvent.detail === "object" && appleEvent.detail.code) {
            // Simplified structure
            code = appleEvent.detail.code;
            state = appleEvent.detail.state;
          } else if (appleEvent.authorization?.code) {
            // Another possible structure
            code = appleEvent.authorization.code;
            state = appleEvent.authorization.state;
          } else if (typeof appleEvent.detail === "string") {
            // Sometimes the data might be a stringified JSON
            try {
              const parsedData = JSON.parse(appleEvent.detail);
              code = parsedData.code || parsedData.authorization?.code;
              state = parsedData.state || parsedData.authorization?.state;
            } catch (e) {
              console.error("[Apple Auth] Failed to parse string data:", e);
            }
          }

          if (code && state) {
            // Validate state for CSRF protection
            // TODO: Fix state validation later
            // const storedState = sessionStorage.getItem("apple_auth_state");
            // if (state !== storedState) {
            //   throw new Error("Invalid state parameter - potential CSRF attack");
            // }

            // Clear the stored state after validation
            sessionStorage.removeItem("apple_auth_state");

            // Call the OpenSecret SDK to handle the authentication
            await os.handleAppleCallback(code, state, inviteCode || "");

            // Clear any existing billing token to prevent session mixing
            try {
              getBillingService().clearToken();
            } catch (billingError) {
              console.warn("Failed to clear billing token:", billingError);
            }

            // Check if this is a Tauri app auth flow (desktop or mobile)
            const isTauriAuth = localStorage.getItem("redirect-to-native") === "true";

            if (isTauriAuth) {
              // Clear the flag
              localStorage.removeItem("redirect-to-native");

              // Handle Tauri redirect - redirect back to desktop app
              const accessToken = localStorage.getItem("access_token") || "";
              const refreshToken = localStorage.getItem("refresh_token");

              let deepLinkUrl = `cloud.opensecret.maple://auth?access_token=${encodeURIComponent(accessToken)}`;

              if (refreshToken) {
                deepLinkUrl += `&refresh_token=${encodeURIComponent(refreshToken)}`;
              }

              setTimeout(() => {
                window.location.href = deepLinkUrl;
              }, 1000);

              return;
            }

            // Handle web flow - regular navigation
            if (onSuccess) {
              onSuccess();
            }

            if (redirectAfterLogin) {
              redirectAfterLogin(selectedPlan);
            }
          } else {
            throw new Error("Missing required authentication data");
          }
        } catch (error) {
          console.error("[Apple Auth] Error processing authentication:", error);
          if (onError && error instanceof Error) {
            onError(error);
          }
        }
      });

      // Listen for authorization failures
      document.addEventListener("AppleIDSignInOnFailure", (event) => {
        const failureEvent = event as AppleSignInFailureEvent;
        const errorMessage = failureEvent.detail.error;
        console.error("[Apple Auth] Sign In failed:", errorMessage);

        // Don't show error for user closing the popup - they already know
        if (errorMessage === "popup_closed_by_user") {
          return;
        }

        if (onError) {
          onError(new Error(errorMessage || "Apple authentication failed"));
        }
      });

      appleAuthInitialized.current = true;
      isInitializing.current = false;
    } catch (error) {
      console.error("[Apple Auth] Failed to initialize Apple Sign In:", error);
      isInitializing.current = false;
      if (onError && error instanceof Error) {
        onError(error);
      }
    }
  };

  const handleAppleSignIn = async () => {
    try {
      if (!window.AppleID) {
        throw new Error("Apple Sign In SDK not loaded");
      }

      // Initialize Apple Auth if not already initialized
      if (!appleAuthInitialized.current) {
        await initializeAppleAuth();
        appleAuthInitialized.current = true;
      }

      // This will open a popup for Apple authentication
      // Add additional handling to be more robust
      const authResult = await window.AppleID.auth.signIn();

      // Some implementations might return the data directly
      if (authResult && authResult.authorization && authResult.authorization.code) {
        const code = authResult.authorization.code;
        const state = authResult.authorization.state;

        if (code && state) {
          // Validate state for CSRF protection
          // TODO: Fix state validation later
          // const storedState = sessionStorage.getItem("apple_auth_state");
          // if (state !== storedState) {
          //   throw new Error("Invalid state parameter - potential CSRF attack");
          // }

          // Clear the stored state after validation
          sessionStorage.removeItem("apple_auth_state");

          // Call the OpenSecret SDK to handle the authentication
          await os.handleAppleCallback(code, state, inviteCode || "");

          // Clear any existing billing token to prevent session mixing
          try {
            getBillingService().clearToken();
          } catch (billingError) {
            console.warn("Failed to clear billing token:", billingError);
          }

          // Check if this is a Tauri app auth flow (desktop or mobile)
          const isTauriAuth = localStorage.getItem("redirect-to-native") === "true";

          if (isTauriAuth) {
            // Clear the flag
            localStorage.removeItem("redirect-to-native");

            // Handle Tauri redirect - redirect back to desktop app
            const accessToken = localStorage.getItem("access_token") || "";
            const refreshToken = localStorage.getItem("refresh_token");

            let deepLinkUrl = `cloud.opensecret.maple://auth?access_token=${encodeURIComponent(accessToken)}`;

            if (refreshToken) {
              deepLinkUrl += `&refresh_token=${encodeURIComponent(refreshToken)}`;
            }

            setTimeout(() => {
              window.location.href = deepLinkUrl;
            }, 1000);

            return;
          }

          // Handle web flow - regular navigation
          if (onSuccess) {
            onSuccess();
          }

          if (redirectAfterLogin) {
            redirectAfterLogin(selectedPlan);
          }
        }
      }
      // If not returned directly, it will be handled by the event listener
    } catch (error) {
      console.error("[Apple Auth] Sign In failed:", error);

      // Don't show error for user closing the popup
      if (error instanceof Error && error.message === "popup_closed_by_user") {
        return;
      }

      if (onError && error instanceof Error) {
        onError(error);
      }
    }
  };

  // Skip rendering on Tauri (iOS will use native button)
  if (window.location.protocol === "tauri:") {
    return null;
  }

  // Render Apple Sign In button for web
  return children ? (
    <div onClick={handleAppleSignIn} className={className}>
      {children}
    </div>
  ) : (
    <Button onClick={handleAppleSignIn} className={className || "w-full"}>
      <Apple className="mr-2 h-4 w-4" />
      Log in with Apple
    </Button>
  );
}
