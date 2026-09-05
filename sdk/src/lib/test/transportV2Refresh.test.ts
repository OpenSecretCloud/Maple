import { afterEach, beforeEach, expect, mock, spyOn, test } from "bun:test";
import { getApiPcrConfig, getApiUrl, refreshToken, setApiUrl } from "../api";
import {
  getPlatformApiUrl,
  getPlatformPcrConfig,
  platformRefreshToken,
  setPlatformApiUrl
} from "../platformApi";
import { clearTransportV2Credentials, installTransportV2Credentials } from "../transportV2/auth";
import { transportV2Client } from "../transportV2/client";

const USER_API_URL = "https://user-refresh.example.test/base";
const PLATFORM_API_URL = "https://platform-refresh.example.test/base";
const originalUserUrl = getApiUrl();
const originalUserPcr = getApiPcrConfig();
const originalPlatformUrl = getPlatformApiUrl();
const originalPlatformPcr = getPlatformPcrConfig();

function token(
  principalKind: "user" | "platform",
  tokenKind: "access_descriptor" | "resumption",
  principalId: string
): string {
  const audience = `urn:opensecret:internal:transport-v2:${principalKind}:${
    tokenKind === "access_descriptor" ? "access-descriptor" : "resumption"
  }`;
  const payload = Buffer.from(
    JSON.stringify({
      iss: "urn:opensecret:transport-v2",
      aud: audience,
      tv: 2,
      tk: tokenKind,
      pk: principalKind,
      sub: principalId,
      exp: 2_000_000_000
    })
  ).toString("base64url");
  return `e30.${payload}.c2ln`;
}

function credentials(kind: "user" | "platform", principalId: string) {
  return {
    access_token: token(kind, "access_descriptor", principalId),
    refresh_token: token(kind, "resumption", principalId)
  };
}

beforeEach(() => {
  localStorage.clear();
  setApiUrl(USER_API_URL, { environment: "development", remoteAttestation: false });
  setPlatformApiUrl(PLATFORM_API_URL, {
    environment: "production",
    remoteAttestation: false
  });
});

afterEach(() => {
  mock.restore();
  clearTransportV2Credentials(USER_API_URL);
  clearTransportV2Credentials(PLATFORM_API_URL);
  localStorage.clear();
  setApiUrl(originalUserUrl, originalUserPcr);
  setPlatformApiUrl(originalPlatformUrl, originalPlatformPcr);
});

test("user refresh uses the anonymous v2 resumption transition without generic tokens", async () => {
  const current = credentials("user", "user-123");
  const next = credentials("user", "user-123");
  installTransportV2Credentials(USER_API_URL, "user", current.access_token, current.refresh_token);
  const refresh = spyOn(transportV2Client, "refresh").mockImplementation(
    async (apiUrl, kind, pcrConfig) => {
      expect(apiUrl).toBe(USER_API_URL);
      expect(kind).toBe("user");
      expect(pcrConfig).toMatchObject({ environment: "development", remoteAttestation: false });
      return Response.json(next);
    }
  );

  await expect(refreshToken()).resolves.toEqual(next);
  expect(refresh).toHaveBeenCalledTimes(1);
  expect(localStorage.getItem("access_token")).toBeNull();
  expect(localStorage.getItem("refresh_token")).toBeNull();
});

test("platform refresh uses the separate anonymous v2 resumption transition", async () => {
  const current = credentials("platform", "platform-123");
  const next = credentials("platform", "platform-123");
  installTransportV2Credentials(
    PLATFORM_API_URL,
    "platform",
    current.access_token,
    current.refresh_token
  );
  const refresh = spyOn(transportV2Client, "refresh").mockImplementation(
    async (apiUrl, kind, pcrConfig) => {
      expect(apiUrl).toBe(PLATFORM_API_URL);
      expect(kind).toBe("platform");
      expect(pcrConfig).toMatchObject({ environment: "production", remoteAttestation: false });
      return Response.json(next);
    }
  );

  await expect(platformRefreshToken()).resolves.toEqual(next);
  expect(refresh).toHaveBeenCalledTimes(1);
  expect(localStorage.getItem("access_token")).toBeNull();
  expect(localStorage.getItem("refresh_token")).toBeNull();
});
