import { useCallback, useEffect, useRef, useState } from "react";
import { useOpenSecret } from "@mapleai/sdk";
import { isTauri } from "@/utils/platform";
import { listen } from "@tauri-apps/api/event";
import { getCurrent } from "@tauri-apps/plugin-deep-link";
import { getSafeInternalRedirect } from "@/utils/internalRedirect";
import {
  authorizeNativeOAuthCallback,
  isNativeOAuthHandoffGrant,
  redeemNativeOAuthGrant,
  type NativeOAuthAccount
} from "@/services/nativeOAuthAttempt";
import { useNotification } from "@/contexts/NotificationContext";
import { NativeOAuthAccountConfirmation } from "./NativeOAuthAccountConfirmation";
import {
  mapleAppVariant,
  nativeCallbackScheme,
  type MapleAppVariant
} from "@/config/mapleAppVariant";

// Rust forwards each deep link as a "deep-link-received" event. A link that
// launches the app arrives before this component is listening, so that event
// is dropped; the deep-link plugin keeps the launch URL, and we read it with
// getCurrent() once the listener is attached.
//
// getCurrent() keeps returning that URL for the whole app session, and
// handling a link reloads the page, so remember what's been handled. Auth
// links use their pending-attempt fence instead; never store credential-bearing URLs.
const HANDLED_DEEP_LINKS_KEY = "maple.handledDeepLinks";
const MAX_HANDLED_DEEP_LINKS = 20;
const deepLinksHandledThisPage = new Set<string>();

function deepLinkKey(url: string): string | null {
  try {
    const parsed = new URL(url);
    const firstPathPart = parsed.pathname.split("/").filter(Boolean)[0] || parsed.hostname;
    if (
      firstPathPart === "auth" ||
      parsed.username ||
      parsed.password ||
      parsed.hash ||
      ["handoff_grant", "access_token", "refresh_token", "cache_namespace_root"].some((name) =>
        parsed.searchParams.has(name)
      )
    ) {
      return null;
    }
    return parsed.toString();
  } catch {
    return null;
  }
}

function readHandledDeepLinks(): string[] {
  try {
    const parsed: unknown = JSON.parse(sessionStorage.getItem(HANDLED_DEEP_LINKS_KEY) ?? "[]");
    return Array.isArray(parsed) ? parsed.filter((k): k is string => typeof k === "string") : [];
  } catch {
    return [];
  }
}

function rememberHandledDeepLink(url: string): void {
  const key = deepLinkKey(url);
  if (key === null) return;
  deepLinksHandledThisPage.add(key);
  try {
    const handled = readHandledDeepLinks().filter((k) => k !== key);
    handled.push(key);
    sessionStorage.setItem(
      HANDLED_DEEP_LINKS_KEY,
      JSON.stringify(handled.slice(-MAX_HANDLED_DEEP_LINKS))
    );
  } catch {
    // Without storage, a reload may see the launch link again. The handlers
    // below still reject replays (auth needs a pending attempt).
  }
}

function wasDeepLinkHandled(url: string): boolean {
  const key = deepLinkKey(url);
  return (
    key !== null && (deepLinksHandledThisPage.has(key) || readHandledDeepLinks().includes(key))
  );
}

function navigatePaymentReturn(destination: string, launch: boolean): void {
  // If replay storage is unavailable, the launch URL is read after every
  // reload. Once at its destination, avoid starting another reload.
  // Credits and Team success routes replace their initial callback URL.
  if (
    launch &&
    ((destination === "/?credits_success=true" && window.location.pathname === "/settings/api") ||
      (destination === "/pricing?success=true" && window.location.pathname === "/settings/team"))
  ) {
    return;
  }
  if (`${window.location.pathname}${window.location.search}` !== destination) {
    window.location.href = destination;
  }
}

export function DeepLinkHandler({
  tauri = isTauri(),
  appVariant = mapleAppVariant(),
  desktopDev = import.meta.env.VITE_MAPLE_DESKTOP_VARIANT === "dev"
}: { tauri?: boolean; appVariant?: MapleAppVariant; desktopDev?: boolean } = {}) {
  const os = useOpenSecret();
  const { showNotification } = useNotification();
  const isAuthenticatedRef = useRef(false);
  const redeemingRef = useRef(false);
  const activeRedemptionRef = useRef<AbortController | null>(null);
  const confirmationRef = useRef<((approved: boolean) => void) | null>(null);
  const [confirmation, setConfirmation] = useState<NativeOAuthAccount | null>(null);
  isAuthenticatedRef.current = Boolean(os.auth.user);

  const resolveConfirmation = useCallback((approved: boolean) => {
    const resolve = confirmationRef.current;
    confirmationRef.current = null;
    setConfirmation(null);
    resolve?.(approved);
  }, []);

  const confirmAccount = useCallback(
    (account: NativeOAuthAccount) =>
      new Promise<boolean>((resolve) => {
        resolveConfirmation(false);
        if (isAuthenticatedRef.current) {
          resolve(false);
          return;
        }
        confirmationRef.current = resolve;
        setConfirmation(account);
      }),
    [resolveConfirmation]
  );

  useEffect(() => {
    if (os.auth.user && confirmationRef.current) {
      activeRedemptionRef.current?.abort();
      resolveConfirmation(false);
    }
  }, [os.auth.user, resolveConfirmation]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;

    const setupDeepLinkHandling = async () => {
      try {
        if (tauri) {
          console.log("[Deep Link] Setting up handler for Tauri app");

          const handleDeepLink = async (url: string, launch = false) => {
            if (disposed) return;
            let isAuthenticationCallback = false;
            console.log("[Deep Link] Received callback");

            try {
              // Parse the URL to extract parameters
              const urlObj = new URL(url);
              const callbackScheme = `${nativeCallbackScheme(appVariant)}:`;
              // The dev app owns only its dev scheme, including payment returns.
              if (appVariant === "dev" && urlObj.protocol !== callbackScheme) return;
              if (
                appVariant === "production" &&
                urlObj.protocol === `${nativeCallbackScheme("dev")}:`
              ) {
                return;
              }
              // The URL path structure will be: cloud.opensecret.maple://path?params
              const pathParts = urlObj.pathname.split("/").filter(Boolean);
              const firstPathPart = pathParts[0] || urlObj.hostname;

              // Handle different types of deep links
              if (firstPathPart === "auth") {
                isAuthenticationCallback = true;
                const parameterNames = [...urlObj.searchParams.keys()];
                const grantValues = urlObj.searchParams.getAll("handoff_grant");
                const validEnvelope =
                  urlObj.protocol === callbackScheme &&
                  urlObj.hostname === "auth" &&
                  (urlObj.pathname === "" || urlObj.pathname === "/") &&
                  urlObj.username === "" &&
                  urlObj.password === "" &&
                  urlObj.port === "" &&
                  urlObj.hash === "" &&
                  parameterNames.length === 1 &&
                  parameterNames[0] === "handoff_grant" &&
                  grantValues.length === 1 &&
                  isNativeOAuthHandoffGrant(grantValues[0]);
                if (!validEnvelope) {
                  console.error("[Deep Link] Authentication callback is malformed");
                  return;
                }

                const authorization = authorizeNativeOAuthCallback(isAuthenticatedRef.current);
                if (authorization === "already_authenticated") {
                  console.warn("[Deep Link] Ignoring auth callback for an existing session");
                  return;
                }
                if (authorization === "missing_or_expired_attempt") {
                  console.warn("[Deep Link] Ignoring unsolicited or expired auth callback");
                  return;
                }
                if (redeemingRef.current) {
                  console.warn("[Deep Link] Authentication completion is already in progress");
                  return;
                }

                redeemingRef.current = true;
                const controller = new AbortController();
                activeRedemptionRef.current = controller;
                try {
                  const pending = await redeemNativeOAuthGrant(grantValues[0], {
                    confirmAccount,
                    signal: controller.signal
                  });
                  if (!pending || disposed || controller.signal.aborted) return;
                  console.log("[Deep Link] Authentication grant accepted");
                  if (pending.selectedPlan) {
                    window.location.href = `/pricing?selected_plan=${encodeURIComponent(pending.selectedPlan)}`;
                  } else if (pending.next === "/redeem" && pending.redemptionCode) {
                    window.location.href = `/redeem?code=${encodeURIComponent(pending.redemptionCode)}`;
                  } else {
                    window.location.href = getSafeInternalRedirect(pending.next) ?? "/";
                  }
                } finally {
                  if (activeRedemptionRef.current === controller) {
                    activeRedemptionRef.current = null;
                    redeemingRef.current = false;
                    resolveConfirmation(false);
                  }
                }
              } else if (
                urlObj.username ||
                urlObj.password ||
                urlObj.hash ||
                ["handoff_grant", "access_token", "refresh_token", "cache_namespace_root"].some(
                  (name) => urlObj.searchParams.has(name)
                )
              ) {
                return;
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
                  navigatePaymentReturn("/?credits_success=true", launch);
                } else if (isSuccess) {
                  // Navigate to the success page or show a success message
                  navigatePaymentReturn("/pricing?success=true", launch);
                } else if (isCanceled) {
                  // Navigate to the canceled page or show a canceled message
                  navigatePaymentReturn("/pricing?canceled=true", launch);
                } else {
                  // Handle unknown payment status
                  console.warn("[Deep Link] Unknown payment status in callback");
                  navigatePaymentReturn("/pricing", launch);
                }
              } else {
                console.warn("[Deep Link] Unknown deep link type:", firstPathPart);
              }
            } catch (error) {
              if (disposed) return;
              if (isAuthenticationCallback) {
                console.error("[Deep Link] Failed to complete authentication");
                showNotification({
                  type: "error",
                  title: "Sign-in could not be completed",
                  message: "Please restart sign-in from Maple.",
                  duration: 0
                });
              } else {
                console.error("[Deep Link] Failed to process deep link", error);
              }
            }
          };

          // Listen for the custom event we emit from Rust
          unlisten = await listen<string>("deep-link-received", async (event) => {
            if (disposed) return;
            rememberHandledDeepLink(event.payload);
            await handleDeepLink(event.payload);
          });

          if (disposed) {
            unlisten();
            return;
          }

          const launchUrls = await getCurrent().catch((error: unknown) => {
            console.warn("[Deep Link] Could not read the launch link:", error);
            return null;
          });
          for (const url of launchUrls ?? []) {
            if (disposed) return;
            if (wasDeepLinkHandled(url)) continue;
            rememberHandledDeepLink(url);
            await handleDeepLink(url, true);
          }

          console.log("[Deep Link] Handler setup complete");
        }
      } catch (error) {
        console.error("[Deep Link] Setup failed:", error);
      }
    };

    setupDeepLinkHandling();

    // Return cleanup function
    return () => {
      disposed = true;
      activeRedemptionRef.current?.abort();
      activeRedemptionRef.current = null;
      redeemingRef.current = false;
      resolveConfirmation(false);
      if (unlisten) unlisten();
    };
  }, [appVariant, confirmAccount, desktopDev, resolveConfirmation, showNotification, tauri]);

  return confirmation ? (
    <NativeOAuthAccountConfirmation account={confirmation} onDecision={resolveConfirmation} />
  ) : null;
}
