import React, { useEffect, useRef } from "react";
import { useOpenSecret } from "@opensecret/react";
import { v4 as uuidv4 } from "uuid";
import { sha256 } from "@noble/hashes/sha256";
import { bytesToHex } from "@noble/hashes/utils";
import { Button } from "./ui/button";
import { Apple } from "./icons/Apple";

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
  const rawNonceRef = useRef<string>("");

  useEffect(() => {
    // Don't load the script multiple times
    if (appleScriptLoaded.current) return;

    // Skip if we're in a Tauri environment (will use native flow on iOS)
    if (window.location.protocol === "tauri:") return;

    const initializeAppleAuth = async () => {
      if (!window.AppleID) {
        console.error("[Apple Auth] AppleID JS SDK not loaded");
        return;
      }

      try {
        try {
          // First we need to get the proper state and auth URL from the backend
          const initiateResult = await os.initiateAppleAuth(inviteCode || "");
          console.log("[Apple Auth] Initiate result:", initiateResult);

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
            redirectURI: "https://trymaple.ai/auth/apple/callback",
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

        console.log(
          "[Apple Auth] Using redirectURI:",
          window.location.origin + "/auth/apple/callback"
        );

        // Add event listeners for Apple Sign In response
        document.addEventListener("AppleIDSignInOnSuccess", async (event) => {
          // Handle successful response
          try {
            // Cast event to AppleSignInSuccessEvent
            const appleEvent = event as AppleSignInSuccessEvent;

            // Log the entire event for debugging
            console.log("[Apple Auth] Success Event:", appleEvent);

            // Access the data - the structure might vary
            let code, state, identityToken;

            // Different versions of Apple Sign In JS SDK might structure the data differently
            if (
              typeof appleEvent.detail === "object" &&
              appleEvent.detail.data?.authorization?.code
            ) {
              // Standard structure
              code = appleEvent.detail.data.authorization.code;
              state = appleEvent.detail.data.authorization.state;
              identityToken = appleEvent.detail.data.authorization.id_token;
            } else if (
              typeof appleEvent.detail === "object" &&
              appleEvent.detail.authorization?.code
            ) {
              // Alternative structure
              code = appleEvent.detail.authorization.code;
              state = appleEvent.detail.authorization.state;
              identityToken = appleEvent.detail.authorization.id_token;
            } else if (typeof appleEvent.detail === "object" && appleEvent.detail.code) {
              // Simplified structure
              code = appleEvent.detail.code;
              state = appleEvent.detail.state;
              identityToken = appleEvent.detail.id_token;
            } else if (appleEvent.authorization?.code) {
              // Another possible structure
              code = appleEvent.authorization.code;
              state = appleEvent.authorization.state;
              identityToken = appleEvent.authorization.id_token;
            } else if (typeof appleEvent.detail === "string") {
              // Sometimes the data might be a stringified JSON
              try {
                const parsedData = JSON.parse(appleEvent.detail);
                code = parsedData.code || parsedData.authorization?.code;
                state = parsedData.state || parsedData.authorization?.state;
                identityToken = parsedData.id_token || parsedData.authorization?.id_token;
              } catch (e) {
                console.error("[Apple Auth] Failed to parse string data:", e);
              }
            }

            console.log("[Apple Auth] Parsed data:", { code, state, identityToken });

            if (code && state) {
              // Call the OpenSecret SDK to handle the authentication
              await os.handleAppleCallback(code, state, inviteCode || "");

              // Handle successful login redirection
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
          console.error("[Apple Auth] Sign In failed:", failureEvent.detail.error);
          if (onError) {
            onError(new Error(failureEvent.detail.error || "Apple authentication failed"));
          }
        });
      } catch (error) {
        console.error("[Apple Auth] Failed to initialize Apple Sign In:", error);
        if (onError && error instanceof Error) {
          onError(error);
        }
      }
    };

    // Load Apple Sign In JS SDK
    const script = document.createElement("script");
    script.src =
      "https://appleid.cdn-apple.com/appleauth/static/jsapi/appleid/1/en_US/appleid.auth.js";
    script.async = true;
    script.onload = () => {
      initializeAppleAuth().catch((error) => {
        console.error("[Apple Auth] Initialization failed:", error);
        if (onError && error instanceof Error) {
          onError(error);
        }
      });
    };
    document.head.appendChild(script);

    appleScriptLoaded.current = true;

    return () => {
      // Clean up script on unmount if needed
      if (script.parentNode) {
        script.parentNode.removeChild(script);
      }
    };
  }, [os, inviteCode, selectedPlan, onSuccess, redirectAfterLogin, onError]);

  const handleAppleSignIn = async () => {
    try {
      if (!window.AppleID) {
        throw new Error("Apple Sign In SDK not loaded");
      }

      // This will open a popup for Apple authentication
      // Add additional handling to be more robust
      const authResult = await window.AppleID.auth.signIn();

      // Log the direct result from signIn (might contain data in some cases)
      console.log("[Apple Auth] Direct signIn result:", authResult);

      // Some implementations might return the data directly
      if (authResult && authResult.authorization && authResult.authorization.code) {
        const code = authResult.authorization.code;
        const state = authResult.authorization.state;

        console.log("[Apple Auth] Found authorization in direct result", { code, state });

        if (code && state) {
          // Call the OpenSecret SDK to handle the authentication
          await os.handleAppleCallback(code, state, inviteCode || "");

          // Handle successful login redirection
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
      if (onError && error instanceof Error) {
        onError(error);
      }
    }
  };

  // Use provided children or default Apple sign-in button
  const renderButton = () => {
    if (children) {
      return <div onClick={handleAppleSignIn}>{children}</div>;
    }

    return (
      <Button onClick={handleAppleSignIn} className={className || "w-full"}>
        <Apple className="mr-2 h-4 w-4" />
        Sign in with Apple
      </Button>
    );
  };

  return renderButton();
}
