import React, { useEffect, useRef } from "react";
import { exportTransportV2AuthBundle, useOpenSecret } from "@opensecret/react";
import { v4 as uuidv4 } from "uuid";
import { sha256 } from "@noble/hashes/sha256";
import { bytesToHex } from "@noble/hashes/utils";
import { Button, type ButtonProps } from "./ui/button";
import { Apple } from "./icons/Apple";
import { getBillingService } from "@/billing/billingService";
import { getSafeInternalRedirect } from "@/utils/internalRedirect";
import {
  buildTransportV2NativeAuthDeepLink,
  clearDesktopOAuthTransport,
  isNativeOAuthRedirect,
  readTransportV2DesktopOAuthAttempt
} from "@/services/desktopOAuthTransport";

interface AppleAuthProviderProps {
  onSuccess?: () => void;
  onError?: (error: Error) => void;
  inviteCode?: string;
  redirectAfterLogin?: (plan?: string) => void;
  selectedPlan?: string;
  className?: string;
  buttonLabel?: string;
  buttonVariant?: ButtonProps["variant"];
  children?: React.ReactNode;
}

export interface AppleAuthorization {
  code: string;
  state: string;
  id_token?: string;
}

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
          authorization: AppleAuthorization;
        }>;
      };
    };
  }
}

function getAppleAuthError(value: unknown): Error {
  if (value instanceof Error) return value;
  if (value && typeof value === "object") {
    const error = (value as Record<string, unknown>).error;
    if (typeof error === "string" && error) return new Error(error);
  }

  return new Error("Apple authentication failed");
}

function isAppleAuthCancellation(error: Error): boolean {
  return error.message === "user_cancelled_authorize" || error.message === "popup_closed_by_user";
}

export function AppleAuthProvider({
  onSuccess,
  onError,
  inviteCode = "",
  redirectAfterLogin,
  selectedPlan,
  className,
  buttonLabel = "Log in with Apple",
  buttonVariant,
  children
}: AppleAuthProviderProps) {
  const os = useOpenSecret();
  const appleScriptLoaded = useRef(false);
  const rawNonceRef = useRef<string>("");
  const isSignInPending = useRef(false);

  useEffect(() => {
    if (appleScriptLoaded.current) return;
    if (window.location.protocol === "tauri:") return;

    const script = document.createElement("script");
    script.src =
      "https://appleid.cdn-apple.com/appleauth/static/jsapi/appleid/1/en_US/appleid.auth.js";
    script.async = true;
    document.head.appendChild(script);
    appleScriptLoaded.current = true;

    return () => {
      if (script.parentNode) {
        script.parentNode.removeChild(script);
      }
      appleScriptLoaded.current = false;
    };
  }, []);

  const initializeAppleAuth = async () => {
    if (!window.AppleID) {
      throw new Error("Apple Sign In SDK not loaded");
    }

    // A retry is a new authorization attempt, so it gets a fresh backend state and nonce.
    const initiateResult = await os.initiateAppleAuth(inviteCode || "");
    rawNonceRef.current = uuidv4();
    const hashedNonce = bytesToHex(sha256(new TextEncoder().encode(rawNonceRef.current)));

    sessionStorage.setItem("apple_auth_nonce", rawNonceRef.current);
    const state = initiateResult.state || "";
    sessionStorage.setItem("apple_auth_state", state);

    if (selectedPlan) {
      sessionStorage.setItem("selected_plan", selectedPlan);
    }

    window.AppleID.auth.init({
      clientId: "cloud.opensecret.maple.services",
      scope: "name email",
      redirectURI: window.location.origin + "/auth/apple/callback",
      state,
      nonce: hashedNonce,
      usePopup: true
    });
  };

  const completeAuthorization = async (authorization: AppleAuthorization) => {
    sessionStorage.removeItem("apple_auth_state");
    await os.handleAppleCallback(authorization.code, authorization.state, inviteCode || "");

    try {
      getBillingService().clearToken();
    } catch (billingError) {
      console.warn("Failed to clear billing token:", billingError);
    }

    const isTauriAuth = isNativeOAuthRedirect();
    if (isTauriAuth) {
      const authBundle = await exportTransportV2AuthBundle(
        import.meta.env.VITE_OPEN_SECRET_API_URL
      );
      const nativeOAuthAttemptId = readTransportV2DesktopOAuthAttempt();
      if (!nativeOAuthAttemptId) {
        throw new Error("Desktop authentication state is missing or expired; please restart login");
      }

      const postAuthRedirect = sessionStorage.getItem("post_auth_redirect");
      sessionStorage.removeItem("post_auth_redirect");
      const safePostAuthRedirect = getSafeInternalRedirect(postAuthRedirect);

      const deepLinkUrl = buildTransportV2NativeAuthDeepLink(
        authBundle,
        nativeOAuthAttemptId,
        !selectedPlan ? safePostAuthRedirect : null
      );
      clearDesktopOAuthTransport();

      setTimeout(() => {
        window.location.href = deepLinkUrl;
      }, 1000);
      return;
    }

    onSuccess?.();
    redirectAfterLogin?.(selectedPlan);
  };

  const handleAppleSignIn = async () => {
    if (isSignInPending.current) return;
    isSignInPending.current = true;

    try {
      await initializeAppleAuth();

      // Programmatic Apple sign-in returns one promise that resolves on success and rejects on
      // failure. It is the only completion channel; document events are intentionally unused.
      const authResult = await window.AppleID.auth.signIn();
      const authorization = authResult?.authorization;
      if (!authorization?.code || !authorization.state) {
        throw new Error("Missing required authentication data");
      }

      await completeAuthorization(authorization);
    } catch (error) {
      const signInError = getAppleAuthError(error);
      console.error("[Apple Auth] Sign In failed:", signInError);

      if (!isAppleAuthCancellation(signInError)) {
        onError?.(signInError);
      }
    } finally {
      isSignInPending.current = false;
    }
  };

  if (window.location.protocol === "tauri:") {
    return null;
  }

  return children ? (
    <div onClick={handleAppleSignIn} className={className}>
      {children}
    </div>
  ) : (
    <Button
      type="button"
      onClick={handleAppleSignIn}
      variant={buttonVariant}
      className={className || "w-full"}
    >
      <Apple className="mr-2 h-4 w-4" />
      {buttonLabel}
    </Button>
  );
}
