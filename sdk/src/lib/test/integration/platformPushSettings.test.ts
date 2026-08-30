import { afterEach, beforeEach, expect, mock, spyOn, test } from "bun:test";
import {
  getPlatformApiUrl,
  getPlatformPcrConfig,
  getPushSettings,
  setPlatformApiUrl,
  updatePushSettings,
  type PushSettings
} from "../../platformApi";
import type { PcrConfig } from "../../pcr";
import { clearTransportV2Credentials, installTransportV2Credentials } from "../../transportV2/auth";
import { transportV2Client, type TransportV2FetchInput } from "../../transportV2/client";

const platformApiUrl = "https://platform.example.com";
const pcrConfig: PcrConfig = { environment: "production", remoteAttestation: false };
const originalPlatformApiUrl = getPlatformApiUrl();
const originalPlatformPcrConfig = getPlatformPcrConfig();

function platformToken(kind: "access_descriptor" | "resumption"): string {
  const audience =
    kind === "access_descriptor"
      ? "urn:opensecret:internal:transport-v2:platform:access-descriptor"
      : "urn:opensecret:internal:transport-v2:platform:resumption";
  const payload = Buffer.from(
    JSON.stringify({
      iss: "urn:opensecret:transport-v2",
      aud: audience,
      tv: 2,
      tk: kind,
      pk: "platform",
      sub: "platform-user-123",
      exp: 2_000_000_000
    })
  ).toString("base64url");
  return `e30.${payload}.c2ln`;
}

function expectPlatformRequest(input: TransportV2FetchInput, method: "GET" | "PUT"): void {
  expect(input.url).toBe(
    `${platformApiUrl}/platform/orgs/org-123/projects/project-456/settings/push`
  );
  expect(input.method).toBe(method);
  expect(input.authority).toMatchObject({ kind: "platform", principalId: "platform-user-123" });
  expect(input.authority).toHaveProperty("generation");
  expect(new Headers(input.headers).has("authorization")).toBe(false);
}

beforeEach(() => {
  localStorage.clear();
  sessionStorage.clear();
  setPlatformApiUrl(platformApiUrl, pcrConfig);
  installTransportV2Credentials(
    platformApiUrl,
    "platform",
    platformToken("access_descriptor"),
    platformToken("resumption")
  );
});

afterEach(() => {
  mock.restore();
  clearTransportV2Credentials(platformApiUrl);
  setPlatformApiUrl(originalPlatformApiUrl, originalPlatformPcrConfig);
  localStorage.clear();
  sessionStorage.clear();
});

test("getPushSettings calls the project push settings endpoint", async () => {
  const responseSettings: PushSettings = {
    encrypted_preview_enabled: true,
    ios: {
      enabled: true,
      bundle_id: "ai.trymaple.ios",
      apns_environment: "prod",
      team_id: "TEAM123",
      key_id: "KEY123"
    },
    android: {
      enabled: true,
      firebase_project_id: "firebase-project",
      package_name: "ai.trymaple.android"
    }
  };
  const fetch = spyOn(transportV2Client, "fetch").mockImplementation(async (input) => {
    expectPlatformRequest(input, "GET");
    expect(input.body).toBeNull();
    return Response.json(responseSettings);
  });

  await expect(getPushSettings("org-123", "project-456")).resolves.toEqual(responseSettings);
  expect(fetch).toHaveBeenCalledTimes(1);
});

test("updatePushSettings sends encrypted push settings to the project endpoint", async () => {
  const requestSettings: PushSettings = {
    encrypted_preview_enabled: true,
    ios: {
      enabled: true,
      bundle_id: "ai.trymaple.ios",
      apns_environment: "dev",
      team_id: "TEAM456",
      key_id: "KEY456"
    },
    android: {
      enabled: false,
      firebase_project_id: "firebase-project",
      package_name: "ai.trymaple.android"
    }
  };
  const fetch = spyOn(transportV2Client, "fetch").mockImplementation(async (input) => {
    expectPlatformRequest(input, "PUT");
    expect(JSON.parse(new TextDecoder().decode(input.body!))).toEqual(requestSettings);
    return Response.json(requestSettings);
  });

  await expect(updatePushSettings("org-123", "project-456", requestSettings)).resolves.toEqual(
    requestSettings
  );
  expect(fetch).toHaveBeenCalledTimes(1);
});
