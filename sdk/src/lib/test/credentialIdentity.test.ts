import { beforeEach, describe, expect, test } from "bun:test";
import {
  ACCOUNT_CREDENTIAL_MISMATCH_CODE,
  accessTokenSubject,
  assertExpectedAccessTokenSubject,
  captureExpectedUserCredentials,
  clearCapturedUserCredentials,
  commitRefreshedUserTokensIfCurrent,
  isAccountCredentialMismatchError,
  revokeAndClearUserCredentials
} from "../credentialIdentity";

function tokenForSubject(subject: string): string {
  const encode = (value: object) =>
    btoa(JSON.stringify(value)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  return `${encode({ alg: "ES256K", typ: "JWT" })}.${encode({ sub: subject })}.signature`;
}

describe("credential identity", () => {
  beforeEach(() => window.localStorage.clear());

  test("extracts the stable subject from refreshed JWTs", () => {
    expect(accessTokenSubject(tokenForSubject("user-a"))).toBe("user-a");
    expect(accessTokenSubject("invalid")).toBeNull();
  });

  test("asserts that the current access token belongs to the expected user", () => {
    window.localStorage.setItem("access_token", tokenForSubject("user-a"));

    expect(() => assertExpectedAccessTokenSubject("user-a")).not.toThrow();
    expect(() => assertExpectedAccessTokenSubject("user-b")).toThrow();
  });

  test("recognizes credential mismatch errors without matching arbitrary errors", () => {
    expect(isAccountCredentialMismatchError({ code: ACCOUNT_CREDENTIAL_MISMATCH_CODE })).toBe(true);
    expect(isAccountCredentialMismatchError(new Error("network"))).toBe(false);
  });

  test("captures and clears an unchanged same-account credential pair", () => {
    const accessToken = tokenForSubject("user-a");
    window.localStorage.setItem("access_token", accessToken);
    window.localStorage.setItem("refresh_token", "refresh-a");

    const snapshot = captureExpectedUserCredentials("user-a");
    expect(snapshot).toEqual({
      userId: "user-a",
      accessToken,
      refreshToken: "refresh-a"
    });

    clearCapturedUserCredentials(snapshot);

    expect(window.localStorage.getItem("access_token")).toBeNull();
    expect(window.localStorage.getItem("refresh_token")).toBeNull();
  });

  test("rejects a stale provider before it can capture another account's refresh token", () => {
    window.localStorage.setItem("access_token", tokenForSubject("user-b"));
    window.localStorage.setItem("refresh_token", "refresh-b");

    expect(() => captureExpectedUserCredentials("user-a")).toThrow();
    expect(window.localStorage.getItem("access_token")).toBe(tokenForSubject("user-b"));
    expect(window.localStorage.getItem("refresh_token")).toBe("refresh-b");
  });

  test("does not clear another account's credentials after an in-flight logout", () => {
    window.localStorage.setItem("access_token", tokenForSubject("user-a"));
    window.localStorage.setItem("refresh_token", "refresh-a");
    const snapshot = captureExpectedUserCredentials("user-a");

    window.localStorage.setItem("access_token", tokenForSubject("user-b"));
    window.localStorage.setItem("refresh_token", "refresh-b");

    expect(() => clearCapturedUserCredentials(snapshot)).toThrow();
    expect(window.localStorage.getItem("access_token")).toBe(tokenForSubject("user-b"));
    expect(window.localStorage.getItem("refresh_token")).toBe("refresh-b");
  });

  test("does not clear replacement credentials for the same account", () => {
    window.localStorage.setItem("access_token", tokenForSubject("user-a"));
    window.localStorage.setItem("refresh_token", "refresh-a");
    const snapshot = captureExpectedUserCredentials("user-a");

    const refreshedAccessToken = `${tokenForSubject("user-a")}-refreshed`;
    window.localStorage.setItem("access_token", refreshedAccessToken);
    window.localStorage.setItem("refresh_token", "refresh-a-new");

    expect(() => clearCapturedUserCredentials(snapshot)).toThrow();
    expect(window.localStorage.getItem("access_token")).toBe(refreshedAccessToken);
    expect(window.localStorage.getItem("refresh_token")).toBe("refresh-a-new");
  });

  test("does not revoke credentials when the provider is already stale", async () => {
    window.localStorage.setItem("access_token", tokenForSubject("user-b"));
    window.localStorage.setItem("refresh_token", "refresh-b");
    const revokedTokens: string[] = [];

    await expect(
      revokeAndClearUserCredentials({
        expectedUserId: "user-a",
        revokeRefreshToken: async (refreshToken) => {
          revokedTokens.push(refreshToken);
        }
      })
    ).rejects.toMatchObject({ code: ACCOUNT_CREDENTIAL_MISMATCH_CODE });

    expect(revokedTokens).toEqual([]);
    expect(window.localStorage.getItem("access_token")).toBe(tokenForSubject("user-b"));
    expect(window.localStorage.getItem("refresh_token")).toBe("refresh-b");
  });

  test("revokes only the captured account and preserves a replacement during logout", async () => {
    window.localStorage.setItem("access_token", tokenForSubject("user-a"));
    window.localStorage.setItem("refresh_token", "refresh-a");
    let releaseLogout!: () => void;
    const revokedTokens: string[] = [];

    const logout = revokeAndClearUserCredentials({
      expectedUserId: "user-a",
      revokeRefreshToken: async (refreshToken) => {
        revokedTokens.push(refreshToken);
        await new Promise<void>((resolve) => {
          releaseLogout = resolve;
        });
      }
    });

    expect(revokedTokens).toEqual(["refresh-a"]);
    window.localStorage.setItem("access_token", tokenForSubject("user-b"));
    window.localStorage.setItem("refresh_token", "refresh-b");
    releaseLogout();

    await expect(logout).rejects.toMatchObject({ code: ACCOUNT_CREDENTIAL_MISMATCH_CODE });
    expect(window.localStorage.getItem("access_token")).toBe(tokenForSubject("user-b"));
    expect(window.localStorage.getItem("refresh_token")).toBe("refresh-b");
  });

  test("preserves same-account logout and unauthenticated cleanup behavior", async () => {
    window.localStorage.setItem("access_token", tokenForSubject("user-a"));
    window.localStorage.setItem("refresh_token", "refresh-a");
    const revokedTokens: string[] = [];

    await revokeAndClearUserCredentials({
      expectedUserId: "user-a",
      revokeRefreshToken: async (refreshToken) => {
        revokedTokens.push(refreshToken);
      }
    });

    expect(revokedTokens).toEqual(["refresh-a"]);
    expect(window.localStorage.getItem("access_token")).toBeNull();
    expect(window.localStorage.getItem("refresh_token")).toBeNull();

    window.localStorage.setItem("access_token", "unverified-access");
    window.localStorage.setItem("refresh_token", "unverified-refresh");
    await revokeAndClearUserCredentials({
      revokeRefreshToken: async (refreshToken) => {
        revokedTokens.push(refreshToken);
      }
    });

    expect(revokedTokens).toEqual(["refresh-a", "unverified-refresh"]);
    expect(window.localStorage.getItem("access_token")).toBeNull();
    expect(window.localStorage.getItem("refresh_token")).toBeNull();
  });

  test("commits a refresh only while its initiating credential still owns storage", () => {
    window.localStorage.setItem("refresh_token", "refresh-a");

    commitRefreshedUserTokensIfCurrent({
      initiatingRefreshToken: "refresh-a",
      accessToken: "access-a-new",
      refreshToken: "refresh-a-new"
    });

    expect(window.localStorage.getItem("access_token")).toBe("access-a-new");
    expect(window.localStorage.getItem("refresh_token")).toBe("refresh-a-new");
  });

  test("does not overwrite credentials replaced while refresh was in flight", () => {
    window.localStorage.setItem("access_token", "access-b");
    window.localStorage.setItem("refresh_token", "refresh-b");

    expect(() =>
      commitRefreshedUserTokensIfCurrent({
        initiatingRefreshToken: "refresh-a",
        accessToken: "late-access-a",
        refreshToken: "late-refresh-a"
      })
    ).toThrow();

    try {
      commitRefreshedUserTokensIfCurrent({
        initiatingRefreshToken: "refresh-a",
        accessToken: "late-access-a",
        refreshToken: "late-refresh-a"
      });
    } catch (error) {
      expect((error as { code?: string }).code).toBe(ACCOUNT_CREDENTIAL_MISMATCH_CODE);
    }
    expect(window.localStorage.getItem("access_token")).toBe("access-b");
    expect(window.localStorage.getItem("refresh_token")).toBe("refresh-b");
  });
});
