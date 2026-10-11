import { useEffect, useRef, useState } from "react";
import { useOpenSecret } from "@mapleai/sdk";
import { HostedNativeSignInConfirmation } from "@/components/HostedNativeSignInConfirmation";
import {
  isCurrentDesktopOAuthTarget,
  isNativeOAuthRedirect,
  readTransportV2DesktopOAuth
} from "@/services/desktopOAuthTransport";
import type { AuthSiteRoute } from "./route";
import { ApplePopupRecovery, CallbackRecovery } from "./CallbackRecovery";

// Fixed stage codes only. Never render SDK errors, callback parameters, or stored state.
// SDK 4.1.1 groups its internal failures, so the SDK code does not imply a network request.
type CallbackStatus =
  | "processing"
  | "confirm"
  | "apple-redirect"
  | "AUTH-CB-TARGET"
  | "AUTH-CB-SDK"
  | "AUTH-CB-REPLACED";

export function HostedCallback({ route }: { route: Extract<AuthSiteRoute, { kind: "callback" }> }) {
  const os = useOpenSecret();
  const active = useRef(true);
  const processed = useRef(false);
  const [target] = useState(() =>
    isNativeOAuthRedirect() ? readTransportV2DesktopOAuth(route.provider) : null
  );
  const [status, setStatus] = useState<CallbackStatus>("processing");

  useEffect(() => {
    active.current = true;
    if (!processed.current) {
      processed.current = true;
      void (async () => {
        // Browser Apple sign-in completes only in HostedAppleSignIn's popup owner.
        // This registered callback path is a passive recovery page, not a redirect fallback.
        if (route.provider === "apple") {
          setStatus("apple-redirect");
          return;
        }
        // A hosted callback must still own its native target. Leave its address untouched.
        if (!target || !isCurrentDesktopOAuthTarget(target)) {
          setStatus("AUTH-CB-TARGET");
          return;
        }
        try {
          const callback = {
            github: os.handleGitHubCallback,
            google: os.handleGoogleCallback
          }[route.provider];
          await callback(route.code, route.state, "");
          if (active.current) {
            setStatus(isCurrentDesktopOAuthTarget(target) ? "confirm" : "AUTH-CB-REPLACED");
          }
        } catch {
          // Do not clear the address or another attempt's pending state on a stale callback.
          // Browser credentials retain the SDK's existing persistence behavior.
          if (active.current) setStatus("AUTH-CB-SDK");
        }
      })();
    }
    return () => {
      active.current = false;
    };
  }, [os.handleGitHubCallback, os.handleGoogleCallback, route, target]);

  if (status === "confirm" && target) return <HostedNativeSignInConfirmation target={target} />;
  if (status !== "processing" && status !== "confirm") {
    return (
      <div className="space-y-4">
        {status === "apple-redirect" ? (
          <ApplePopupRecovery />
        ) : (
          <>
            <p role="alert">This browser sign-in could not be completed.</p>
            <CallbackRecovery />
            <p className="text-xs text-muted-foreground">Error code: {status}</p>
          </>
        )}
      </div>
    );
  }
  return <p role="status">Completing sign-in…</p>;
}
