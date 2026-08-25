const LATEST_JSON_PATH = "/latest.json";
const MAX_LATEST_JSON_BYTES = 64 * 1024;
const STABLE_VERSION = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const GENERATED_PUB_DATE = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/;
const REQUIRED_PLATFORMS = [
  "darwin-aarch64",
  "darwin-x86_64",
  "linux-x86_64",
  "linux-x86_64-appimage",
  "linux-x86_64-deb",
  "linux-x86_64-rpm",
  "windows-x86_64",
] as const;

interface AssetFetcher {
  fetch(request: Request): Promise<Response>;
}

export interface Env {
  ASSETS: AssetFetcher;
}

interface PlatformRelease {
  signature: string;
  url: string;
}

interface LatestRelease {
  notes: string;
  platforms: Record<string, PlatformRelease>;
  pub_date: string;
  version: string;
}

function textResponse(
  body: string,
  status: number,
  extraHeaders?: HeadersInit,
): Response {
  return new Response(body, {
    status,
    headers: {
      "cache-control": "no-store",
      "cdn-cache-control": "no-store",
      "content-type": "text/plain; charset=utf-8",
      "x-content-type-options": "nosniff",
      ...extraHeaders,
    },
  });
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isExpectedArtifactUrl(value: string, version: string): boolean {
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return false;
  }

  if (
    url.protocol !== "https:" ||
    url.hostname !== "github.com" ||
    url.port !== "" ||
    url.username !== "" ||
    url.password !== "" ||
    url.search !== "" ||
    url.hash !== ""
  ) {
    return false;
  }

  const segments = url.pathname.split("/").filter(Boolean);
  return (
    segments.length === 6 &&
    segments[0] === "OpenSecretCloud" &&
    segments[1] === "Maple" &&
    segments[2] === "releases" &&
    segments[3] === "download" &&
    segments[4] === `v${version}` &&
    segments[5].length > 0
  );
}

function isPlatformRelease(
  value: unknown,
  version: string,
): value is PlatformRelease {
  return (
    isRecord(value) &&
    typeof value.signature === "string" &&
    value.signature.trim().length > 0 &&
    value.signature.length <= 4096 &&
    typeof value.url === "string" &&
    isExpectedArtifactUrl(value.url, version)
  );
}

function isGeneratedPubDate(value: string): boolean {
  if (!GENERATED_PUB_DATE.test(value)) {
    return false;
  }

  const parsed = new Date(value);
  return (
    Number.isFinite(parsed.getTime()) &&
    parsed.toISOString().replace(".000Z", "Z") === value
  );
}

export function isLatestRelease(value: unknown): value is LatestRelease {
  if (
    !isRecord(value) ||
    typeof value.version !== "string" ||
    !STABLE_VERSION.test(value.version) ||
    typeof value.notes !== "string" ||
    typeof value.pub_date !== "string" ||
    !isGeneratedPubDate(value.pub_date) ||
    !isRecord(value.platforms)
  ) {
    return false;
  }

  for (const platform of REQUIRED_PLATFORMS) {
    if (!isPlatformRelease(value.platforms[platform], value.version)) {
      return false;
    }
  }

  for (const platform of Object.values(value.platforms)) {
    if (!isPlatformRelease(platform, value.version)) {
      return false;
    }
  }

  const platforms = value.platforms as Record<string, PlatformRelease>;
  const linuxDefault = platforms["linux-x86_64"];
  const linuxAppImage = platforms["linux-x86_64-appimage"];
  return (
    linuxDefault.url === linuxAppImage.url &&
    linuxDefault.signature === linuxAppImage.signature
  );
}

async function readValidatedAsset(
  response: Response,
): Promise<ArrayBuffer | null> {
  const declaredLength = response.headers.get("content-length");
  if (declaredLength !== null) {
    const parsedLength = Number(declaredLength);
    if (
      !Number.isSafeInteger(parsedLength) ||
      parsedLength > MAX_LATEST_JSON_BYTES
    ) {
      return null;
    }
  }

  const body = await response.arrayBuffer();
  if (body.byteLength === 0 || body.byteLength > MAX_LATEST_JSON_BYTES) {
    return null;
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(body));
  } catch {
    return null;
  }

  return isLatestRelease(parsed) ? body : null;
}

export async function handleRequest(
  request: Request,
  env: Env,
): Promise<Response> {
  const url = new URL(request.url);
  if (url.pathname !== LATEST_JSON_PATH) {
    return textResponse("Not found\n", 404);
  }

  if (request.method !== "GET" && request.method !== "HEAD") {
    return textResponse("Method not allowed\n", 405, { allow: "GET, HEAD" });
  }

  const assetRequest = new Request(new URL(LATEST_JSON_PATH, url.origin), {
    method: "GET",
    headers: { accept: "application/json" },
  });

  let asset: Response;
  try {
    asset = await env.ASSETS.fetch(assetRequest);
  } catch {
    return textResponse("Updater metadata unavailable\n", 503);
  }

  if (asset.status === 404) {
    return textResponse("Not found\n", 404);
  }
  if (!asset.ok) {
    return textResponse("Updater metadata unavailable\n", 503);
  }

  let body: ArrayBuffer | null;
  try {
    body = await readValidatedAsset(asset);
  } catch {
    body = null;
  }
  if (body === null) {
    return textResponse("Updater metadata unavailable\n", 503);
  }

  const headers = new Headers({
    "cache-control": "public, max-age=0, must-revalidate, no-transform",
    "cdn-cache-control": "no-store",
    "content-type": "application/json; charset=utf-8",
    "x-content-type-options": "nosniff",
  });
  for (const name of ["etag", "last-modified"] as const) {
    const value = asset.headers.get(name);
    if (value !== null) {
      headers.set(name, value);
    }
  }

  return new Response(request.method === "HEAD" ? null : body, {
    status: 200,
    headers,
  });
}

export default {
  fetch: handleRequest,
};
