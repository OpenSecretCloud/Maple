import { isNativeOAuthAttemptId } from "./nativeOAuthAttempt";

export type DesktopOAuthTransport = "v1" | "v2";
export type DesktopOAuthProvider = "github" | "google" | "apple";

const DESKTOP_OAUTH_TRANSPORT_KEY = "maple_desktop_oauth_transport_v1";
const REDIRECT_TO_NATIVE_KEY = "redirect-to-native";
const TRANSPORT_V2_NATIVE_ATTEMPT_KEY = "maple_desktop_oauth_native_attempt_v2";
const TRANSPORT_V2_INITIATION_CLAIM_KEY = "maple_desktop_oauth_initiation_claim_v2";
export const TRANSPORT_V2_NATIVE_ATTEMPT_QUERY = "native_oauth_attempt";

interface TransportV2DesktopAuthUrlOptions {
  provider: DesktopOAuthProvider;
  nativeOAuthAttemptId: string;
  selectedPlan?: string;
  code?: string;
  next?: string;
}

export function buildTransportV2DesktopAuthUrl({
  provider,
  nativeOAuthAttemptId,
  selectedPlan,
  code,
  next
}: TransportV2DesktopAuthUrlOptions): string {
  if (!isNativeOAuthAttemptId(nativeOAuthAttemptId)) {
    throw new Error("Cannot start desktop authentication without valid state");
  }

  const url = new URL("https://trymaple.ai/desktop-auth");
  url.searchParams.set("provider", provider);
  url.searchParams.set("transport", "v2");
  url.searchParams.set(TRANSPORT_V2_NATIVE_ATTEMPT_QUERY, nativeOAuthAttemptId);
  if (selectedPlan) url.searchParams.set("selected_plan", selectedPlan);
  if (code) url.searchParams.set("code", code);
  if (next) url.searchParams.set("next", next);
  return url.toString();
}

export function markDesktopOAuthTransport(transport: DesktopOAuthTransport): void {
  localStorage.setItem(DESKTOP_OAUTH_TRANSPORT_KEY, transport);
  localStorage.setItem(REDIRECT_TO_NATIVE_KEY, "true");
}

export function markTransportV2DesktopOAuth(nativeOAuthAttemptId: string): void {
  if (!isNativeOAuthAttemptId(nativeOAuthAttemptId)) {
    throw new Error("Desktop authentication state is missing or invalid");
  }
  sessionStorage.setItem(TRANSPORT_V2_NATIVE_ATTEMPT_KEY, nativeOAuthAttemptId);
  markDesktopOAuthTransport("v2");
}

export function readTransportV2DesktopOAuthAttempt(): string | null {
  const attemptId = sessionStorage.getItem(TRANSPORT_V2_NATIVE_ATTEMPT_KEY);
  if (isNativeOAuthAttemptId(attemptId)) return attemptId;
  sessionStorage.removeItem(TRANSPORT_V2_NATIVE_ATTEMPT_KEY);
  return null;
}

export function claimTransportV2DesktopOAuthInitiation(nativeOAuthAttemptId: string): boolean {
  if (!isNativeOAuthAttemptId(nativeOAuthAttemptId)) {
    throw new Error("Desktop authentication state is missing or invalid");
  }
  if (readTransportV2DesktopOAuthAttempt() !== nativeOAuthAttemptId) {
    throw new Error("Desktop authentication state changed before initiation");
  }

  if (sessionStorage.getItem(TRANSPORT_V2_INITIATION_CLAIM_KEY) === nativeOAuthAttemptId) {
    return false;
  }
  sessionStorage.setItem(TRANSPORT_V2_INITIATION_CLAIM_KEY, nativeOAuthAttemptId);
  return true;
}

export function readDesktopOAuthTransport(): DesktopOAuthTransport | null {
  const transport = localStorage.getItem(DESKTOP_OAUTH_TRANSPORT_KEY);
  return transport === "v1" || transport === "v2" ? transport : null;
}

export function clearDesktopOAuthTransport(): void {
  localStorage.removeItem(DESKTOP_OAUTH_TRANSPORT_KEY);
  localStorage.removeItem(REDIRECT_TO_NATIVE_KEY);
  sessionStorage.removeItem(TRANSPORT_V2_NATIVE_ATTEMPT_KEY);
  sessionStorage.removeItem(TRANSPORT_V2_INITIATION_CLAIM_KEY);
}

export function isNativeOAuthRedirect(): boolean {
  return localStorage.getItem(REDIRECT_TO_NATIVE_KEY) === "true";
}

export function buildTransportV2NativeAuthDeepLink(
  authBundle: string,
  nativeOAuthAttemptId: string,
  next?: string | null
): string {
  if (!authBundle.trim()) {
    throw new Error("The desktop authentication bundle is missing");
  }
  if (!isNativeOAuthAttemptId(nativeOAuthAttemptId)) {
    throw new Error("The desktop authentication state is missing or invalid");
  }
  const query = new URLSearchParams({
    auth_bundle: authBundle,
    [TRANSPORT_V2_NATIVE_ATTEMPT_QUERY]: nativeOAuthAttemptId
  });
  if (next) query.set("next", next);
  return `cloud.opensecret.maple://auth?${query.toString()}`;
}

export function shouldLoadLegacyDesktopOAuth(
  location: Pick<Location, "pathname" | "search">
): boolean {
  if (location.pathname === "/desktop-auth") {
    // Released clients did not send a transport selector. New clients always
    // opt in explicitly, so removing `transport=v2` cannot downgrade the app
    // that eventually receives the incompatible credential shape.
    return new URLSearchParams(location.search).get("transport") === null;
  }

  if (!/^\/auth\/(github|google|apple)\/callback$/.test(location.pathname)) {
    return false;
  }

  if (!isNativeOAuthRedirect()) return false;

  const transport = readDesktopOAuthTransport();
  // The missing marker is the compatibility case for an already-running OAuth
  // attempt started by the previously deployed web app.
  return transport === "v1" || transport === null;
}
