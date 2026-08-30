import { beforeEach, describe, expect, test } from "bun:test";
import {
  buildTransportV2DesktopAuthUrl,
  buildTransportV2NativeAuthDeepLink,
  claimTransportV2DesktopOAuthInitiation,
  clearDesktopOAuthTransport,
  markDesktopOAuthTransport,
  markTransportV2DesktopOAuth,
  readDesktopOAuthTransport,
  readTransportV2DesktopOAuthAttempt,
  readTransportV2DesktopOAuthSession,
  shouldLoadLegacyDesktopOAuth
} from "./desktopOAuthTransport";

class MemoryStorage implements Storage {
  private readonly values = new Map<string, string>();

  get length(): number {
    return this.values.size;
  }

  clear(): void {
    this.values.clear();
  }

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  key(index: number): string | null {
    return [...this.values.keys()][index] ?? null;
  }

  removeItem(key: string): void {
    this.values.delete(key);
  }

  setItem(key: string, value: string): void {
    this.values.set(key, value);
  }
}

function location(pathname: string, search = ""): Pick<Location, "pathname" | "search"> {
  return { pathname, search };
}

beforeEach(() => {
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: new MemoryStorage(),
    writable: true
  });
  Object.defineProperty(globalThis, "sessionStorage", {
    configurable: true,
    value: new MemoryStorage(),
    writable: true
  });
});

describe("desktop OAuth transport selection", () => {
  const nativeOAuthAttemptId = "00000000-0000-4000-8000-000000000001";
  const nativeSessionId = "11111111-2222-3333-4444-555555555555";

  test("keeps the native attempt out of the HTTPS request and carries it in the fragment", () => {
    const authUrl = buildTransportV2DesktopAuthUrl({
      provider: "github",
      nativeOAuthAttemptId,
      nativeSessionId,
      selectedPlan: "pro",
      code: "redemption",
      next: "/settings"
    });
    const parsed = new URL(authUrl);

    expect(parsed.origin).toBe("https://trymaple.ai");
    expect(parsed.pathname).toBe("/desktop-auth");
    expect(parsed.searchParams.get("provider")).toBe("github");
    expect(parsed.searchParams.get("transport")).toBe("v2");
    expect(parsed.searchParams.has("native_oauth_attempt")).toBe(false);
    expect(new URLSearchParams(parsed.hash.slice(1)).get("native_oauth_attempt")).toBe(
      nativeOAuthAttemptId
    );
    expect(parsed.searchParams.get("native_session_id")).toBe(nativeSessionId);
    expect(parsed.searchParams.get("selected_plan")).toBe("pro");
    expect(parsed.searchParams.get("code")).toBe("redemption");
    expect(parsed.searchParams.get("next")).toBe("/settings");
  });

  test("stores only validated v2 attempt state across the hosted redirect", () => {
    markTransportV2DesktopOAuth(nativeOAuthAttemptId, nativeSessionId);

    expect(readDesktopOAuthTransport()).toBe("v2");
    expect(readTransportV2DesktopOAuthAttempt()).toBe(nativeOAuthAttemptId);
    expect(readTransportV2DesktopOAuthSession()).toBe(nativeSessionId);
    expect(() => markTransportV2DesktopOAuth("not-state", nativeSessionId)).toThrow();
  });

  test("claims GitHub or Google initiation exactly once for one native attempt", () => {
    markTransportV2DesktopOAuth(nativeOAuthAttemptId, nativeSessionId);

    expect(claimTransportV2DesktopOAuthInitiation(nativeOAuthAttemptId)).toBe(true);
    expect(claimTransportV2DesktopOAuthInitiation(nativeOAuthAttemptId)).toBe(false);

    const nextAttemptId = "00000000-0000-4000-8000-000000000002";
    markTransportV2DesktopOAuth(nextAttemptId, nativeSessionId);
    expect(claimTransportV2DesktopOAuthInitiation(nextAttemptId)).toBe(true);
  });

  test("cannot claim initiation for state other than the active hosted attempt", () => {
    markTransportV2DesktopOAuth(nativeOAuthAttemptId, nativeSessionId);

    expect(() =>
      claimTransportV2DesktopOAuthInitiation("00000000-0000-4000-8000-000000000002")
    ).toThrow("state changed");
  });

  test("builds a v2 handoff with only the grant and public session correlation", () => {
    const deepLink = buildTransportV2NativeAuthDeepLink(
      "header.payload.signature",
      nativeSessionId,
      "/settings"
    );
    const parsed = new URL(deepLink);

    expect(parsed.searchParams.get("handoff_grant")).toBe("header.payload.signature");
    expect(parsed.searchParams.has("auth_bundle")).toBe(false);
    expect(parsed.searchParams.get("native_session_id")).toBe(nativeSessionId);
    expect(parsed.searchParams.has("native_oauth_attempt")).toBe(false);
    expect(parsed.searchParams.get("next")).toBe("/settings");
    expect(parsed.searchParams.has("access_token")).toBe(false);
    expect(parsed.searchParams.has("refresh_token")).toBe(false);
  });

  test("keeps an unversioned desktop-auth request on the published v1 bridge", () => {
    expect(shouldLoadLegacyDesktopOAuth(location("/desktop-auth", "?provider=github"))).toBe(true);
  });

  test("routes explicit v2 and invalid selectors away from the legacy bundle", () => {
    expect(
      shouldLoadLegacyDesktopOAuth(location("/desktop-auth", "?provider=github&transport=v2"))
    ).toBe(false);
    expect(
      shouldLoadLegacyDesktopOAuth(location("/desktop-auth", "?provider=github&transport=invalid"))
    ).toBe(false);
  });

  test("preserves callbacks from already-running old-client flows", () => {
    localStorage.setItem("redirect-to-native", "true");
    expect(shouldLoadLegacyDesktopOAuth(location("/auth/github/callback"))).toBe(true);
  });

  test("never loads v1 for an explicitly marked v2 callback", () => {
    markDesktopOAuthTransport("v2");
    expect(readDesktopOAuthTransport()).toBe("v2");
    expect(shouldLoadLegacyDesktopOAuth(location("/auth/google/callback"))).toBe(false);
  });

  test("does not load the legacy bridge for ordinary web callbacks", () => {
    expect(shouldLoadLegacyDesktopOAuth(location("/auth/apple/callback"))).toBe(false);
  });

  test("clears both compatibility markers after a completed handoff", () => {
    markDesktopOAuthTransport("v1");
    clearDesktopOAuthTransport();
    expect(readDesktopOAuthTransport()).toBeNull();
    expect(readTransportV2DesktopOAuthAttempt()).toBeNull();
    expect(readTransportV2DesktopOAuthSession()).toBeNull();
    expect(localStorage.getItem("redirect-to-native")).toBeNull();
  });
});
