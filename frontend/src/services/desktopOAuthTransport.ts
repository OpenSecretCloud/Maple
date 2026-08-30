import { isNativeOAuthAttemptId, isNativeOAuthSessionId } from "./nativeOAuthAttempt";

export type DesktopOAuthTransport = "v1" | "v2";
export type DesktopOAuthProvider = "github" | "google" | "apple";

const DESKTOP_OAUTH_TRANSPORT_KEY = "maple_desktop_oauth_transport_v1";
const REDIRECT_TO_NATIVE_KEY = "redirect-to-native";
const TRANSPORT_V2_NATIVE_ATTEMPT_KEY = "maple_desktop_oauth_native_attempt_v2";
const TRANSPORT_V2_NATIVE_SESSION_KEY = "maple_desktop_oauth_native_session_v2";
const TRANSPORT_V2_INITIATION_CLAIM_KEY = "maple_desktop_oauth_initiation_claim_v2";
export const TRANSPORT_V2_NATIVE_ATTEMPT_QUERY = "native_oauth_attempt";
export const TRANSPORT_V2_NATIVE_SESSION_QUERY = "native_session_id";

interface TransportV2DesktopAuthUrlOptions {
  provider: DesktopOAuthProvider;
  nativeOAuthAttemptId: string;
  nativeSessionId: string;
  selectedPlan?: string;
  code?: string;
  next?: string;
}

export function buildTransportV2DesktopAuthUrl({
  provider,
  nativeOAuthAttemptId,
  nativeSessionId,
  selectedPlan,
  code,
  next
}: TransportV2DesktopAuthUrlOptions): string {
  if (!isNativeOAuthAttemptId(nativeOAuthAttemptId)) {
    throw new Error("Cannot start desktop authentication without valid state");
  }
  if (!isNativeOAuthSessionId(nativeSessionId)) {
    throw new Error("Cannot start desktop authentication without a valid native session");
  }

  const url = new URL("https://trymaple.ai/desktop-auth");
  url.searchParams.set("provider", provider);
  url.searchParams.set("transport", "v2");
  url.searchParams.set(TRANSPORT_V2_NATIVE_SESSION_QUERY, nativeSessionId);
  if (selectedPlan) url.searchParams.set("selected_plan", selectedPlan);
  if (code) url.searchParams.set("code", code);
  if (next) url.searchParams.set("next", next);
  url.hash = new URLSearchParams({
    [TRANSPORT_V2_NATIVE_ATTEMPT_QUERY]: nativeOAuthAttemptId
  }).toString();
  return url.toString();
}

export function readTransportV2DesktopOAuthAttemptFromFragment(hash: string): string | null {
  const fragment = hash.startsWith("#") ? hash.slice(1) : hash;
  const attemptId = new URLSearchParams(fragment).get(TRANSPORT_V2_NATIVE_ATTEMPT_QUERY);
  return isNativeOAuthAttemptId(attemptId) ? attemptId : null;
}

export function markDesktopOAuthTransport(transport: DesktopOAuthTransport): void {
  localStorage.setItem(DESKTOP_OAUTH_TRANSPORT_KEY, transport);
  localStorage.setItem(REDIRECT_TO_NATIVE_KEY, "true");
}

export function markTransportV2DesktopOAuth(
  nativeOAuthAttemptId: string,
  nativeSessionId: string
): void {
  if (!isNativeOAuthAttemptId(nativeOAuthAttemptId)) {
    throw new Error("Desktop authentication state is missing or invalid");
  }
  if (!isNativeOAuthSessionId(nativeSessionId)) {
    throw new Error("Desktop authentication native session is missing or invalid");
  }
  sessionStorage.setItem(TRANSPORT_V2_NATIVE_ATTEMPT_KEY, nativeOAuthAttemptId);
  sessionStorage.setItem(TRANSPORT_V2_NATIVE_SESSION_KEY, nativeSessionId);
  markDesktopOAuthTransport("v2");
}

export function readTransportV2DesktopOAuthAttempt(): string | null {
  const attemptId = sessionStorage.getItem(TRANSPORT_V2_NATIVE_ATTEMPT_KEY);
  if (isNativeOAuthAttemptId(attemptId)) return attemptId;
  sessionStorage.removeItem(TRANSPORT_V2_NATIVE_ATTEMPT_KEY);
  return null;
}

export function readTransportV2DesktopOAuthSession(): string | null {
  const sessionId = sessionStorage.getItem(TRANSPORT_V2_NATIVE_SESSION_KEY);
  if (isNativeOAuthSessionId(sessionId)) return sessionId;
  sessionStorage.removeItem(TRANSPORT_V2_NATIVE_SESSION_KEY);
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
  sessionStorage.removeItem(TRANSPORT_V2_NATIVE_SESSION_KEY);
  sessionStorage.removeItem(TRANSPORT_V2_INITIATION_CLAIM_KEY);
}

export function isNativeOAuthRedirect(): boolean {
  return localStorage.getItem(REDIRECT_TO_NATIVE_KEY) === "true";
}

export function buildTransportV2NativeAuthDeepLink(
  handoffGrant: string,
  nativeSessionId: string,
  next?: string | null
): string {
  if (!handoffGrant.trim()) {
    throw new Error("The desktop authentication grant is missing");
  }
  if (!isNativeOAuthSessionId(nativeSessionId)) {
    throw new Error("The desktop authentication native session is missing or invalid");
  }
  const query = new URLSearchParams({
    handoff_grant: handoffGrant,
    [TRANSPORT_V2_NATIVE_SESSION_QUERY]: nativeSessionId
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
