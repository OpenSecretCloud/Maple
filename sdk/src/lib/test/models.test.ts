import { afterEach, beforeEach, expect, mock, spyOn, test } from "bun:test";
import type { TransportV2FetchInput } from "../transportV2/client";
import { transportV2Client } from "../transportV2/client";
import { installTransportV2Credentials } from "../transportV2/auth";
import { fetchModelCatalog, fetchModels, getApiPcrConfig, getApiUrl, setApiUrl } from "../api";

const apiUrl = "https://models.example.com";
const modelsResponse = {
  object: "list" as const,
  data: [{ id: "test-model", object: "model" as const, created: 0, owned_by: "opensecret" }]
};
const originalApiUrl = getApiUrl();
const originalApiPcrConfig = getApiPcrConfig();

function token(kind: "access_descriptor" | "resumption"): string {
  const audience =
    kind === "access_descriptor"
      ? "urn:opensecret:internal:transport-v2:user:access-descriptor"
      : "urn:opensecret:internal:transport-v2:user:resumption";
  const payload = Buffer.from(
    JSON.stringify({
      iss: "urn:opensecret:transport-v2",
      aud: audience,
      tv: 2,
      tk: kind,
      pk: "user",
      sub: "user-123",
      exp: 2_000_000_000
    })
  ).toString("base64url");
  return `e30.${payload}.c2ln`;
}

beforeEach(() => {
  localStorage.clear();
  sessionStorage.clear();
  setApiUrl(apiUrl, { environment: "development" });
});

afterEach(() => {
  mock.restore();
  setApiUrl(originalApiUrl, originalApiPcrConfig);
  localStorage.clear();
  sessionStorage.clear();
});

function successfulModels(input: TransportV2FetchInput): Promise<Response> {
  expect(input.url).toBe(`${apiUrl}/v1/models`);
  expect(input.method).toBe("GET");
  expect(input.body).toBeNull();
  return Promise.resolve(Response.json(modelsResponse));
}

test("fetchModels uses an anonymous v2 authority before sign-in", async () => {
  const fetch = spyOn(transportV2Client, "fetch").mockImplementation(async (input) => {
    expect(input.authority).toEqual({ kind: "anonymous", purpose: "public" });
    return successfulModels(input);
  });

  await expect(fetchModels()).resolves.toEqual(modelsResponse.data);
  expect(fetch).toHaveBeenCalledTimes(1);
});

test("fetchModels preserves a stored user authority without an outer token", async () => {
  installTransportV2Credentials(apiUrl, "user", token("access_descriptor"), token("resumption"));
  const fetch = spyOn(transportV2Client, "fetch").mockImplementation(async (input) => {
    expect(input.authority).toMatchObject({ kind: "user", principalId: "user-123" });
    expect(input.authority).toHaveProperty("generation");
    expect(new Headers(input.headers).has("authorization")).toBe(false);
    return successfulModels(input);
  });

  await expect(fetchModels()).resolves.toEqual(modelsResponse.data);
  expect(fetch).toHaveBeenCalledTimes(1);
});

test("fetchModels binds an explicit API key and never retries anonymously", async () => {
  const fetch = spyOn(transportV2Client, "fetch").mockImplementation(async (input) => {
    expect(input.authority).toEqual({ kind: "api_key", value: "invalid-api-key" });
    return Response.json({ message: "Invalid API key" }, { status: 401 });
  });

  await expect(fetchModels("invalid-api-key")).rejects.toThrow("Invalid API key");
  expect(fetch).toHaveBeenCalledTimes(1);
});

test("fetchModelCatalog remains user-authenticated", async () => {
  const catalog = {
    ...modelsResponse,
    aliases: [],
    defaults: { quick: "auto:quick" as const, powerful: "auto:powerful" as const }
  };
  const fetch = spyOn(transportV2Client, "fetch").mockImplementation(async (input) => {
    expect(input.authority).toMatchObject({ kind: "user", principalId: "user-123" });
    expect(input.authority).toHaveProperty("generation");
    return Response.json(catalog);
  });

  await expect(fetchModelCatalog()).resolves.toEqual(catalog);
  expect(fetch).toHaveBeenCalledTimes(1);
});
