import {
  MAX_ENVELOPE_BYTES,
  parseEnvelope,
  releasePath,
  type Artifact,
  type Manifest,
} from "./manifest";

interface StoredObject {
  size: number;
  httpEtag: string;
  body?: ReadableStream<Uint8Array>;
}
export interface Bucket {
  get(
    key: string,
    options?: { range: { offset: number; length: number } },
  ): Promise<StoredObject | null>;
  head(key: string): Promise<StoredObject | null>;
}
export interface Env {
  AGENT_RELEASES: Bucket;
}

const NO_STORE = {
  "cache-control": "no-store",
  "cdn-cache-control": "no-store",
  "x-content-type-options": "nosniff",
};
function response(
  status: number,
  method: string,
  extra?: HeadersInit,
): Response {
  return new Response(method === "HEAD" ? null : `${status}\n`, {
    status,
    headers: {
      ...NO_STORE,
      "content-type": "text/plain; charset=utf-8",
      ...extra,
    },
  });
}
async function metadata(
  bucket: Bucket,
  key: string,
): Promise<{ bytes: Uint8Array<ArrayBuffer>; manifest: Manifest } | null> {
  const object = await bucket.get(key);
  if (!object) return null;
  if (object.size === 0 || object.size > MAX_ENVELOPE_BYTES || !object.body) {
    await object.body?.cancel().catch(() => {});
    throw new Error("metadata size");
  }
  const reader = object.body.getReader();
  let length = 0;
  const chunks: Uint8Array[] = [];
  try {
    while (true) {
      const part = await reader.read();
      if (part.done) break;
      length += part.value.length;
      if (length > MAX_ENVELOPE_BYTES || length > object.size)
        throw new Error("metadata size");
      chunks.push(part.value);
    }
  } catch (error) {
    await reader.cancel().catch(() => {});
    throw error;
  }
  if (length !== object.size) throw new Error("truncated metadata");
  const bytes = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.length;
  }
  return { bytes, manifest: parseEnvelope(bytes).manifest };
}

function requestedRange(
  header: string,
  size: number,
): { offset: number; length: number } | null {
  const match = /^bytes=(\d*)-(\d*)$/.exec(header);
  if (!match || (!match[1] && !match[2])) return null;
  if (!match[1]) {
    const suffix = Number(match[2]);
    return Number.isSafeInteger(suffix) && suffix > 0
      ? { offset: Math.max(0, size - suffix), length: Math.min(size, suffix) }
      : null;
  }
  const start = Number(match[1]);
  const end = match[2] ? Number(match[2]) : size - 1;
  if (
    !Number.isSafeInteger(start) ||
    !Number.isSafeInteger(end) ||
    start >= size ||
    end < start
  )
    return null;
  return { offset: start, length: Math.min(end, size - 1) - start + 1 };
}

function contentType(path: string): string {
  if (path.endsWith(".dmg")) return "application/x-apple-diskimage";
  if (path.endsWith(".tar.gz")) return "application/gzip";
  return "application/octet-stream";
}
async function artifactResponse(
  request: Request,
  bucket: Bucket,
  key: string,
  artifact: Artifact,
): Promise<Response> {
  const object = await bucket.head(key);
  if (!object) return response(404, request.method);
  if (object.size !== artifact.size) throw new Error("artifact size mismatch");
  const headers = new Headers({
    "cache-control": "public, max-age=31536000, immutable",
    "content-type": contentType(artifact.path),
    "x-content-type-options": "nosniff",
    "accept-ranges": "bytes",
    etag: object.httpEtag,
    "content-length": String(object.size),
  });
  if (
    request.headers
      .get("if-none-match")
      ?.split(",")
      .some((value) => value.trim() === object.httpEtag || value.trim() === "*")
  )
    return new Response(null, { status: 304, headers });
  // RFC 9110: Range modifies GET only; HEAD describes the full representation.
  const rangeHeader =
    request.method === "GET" ? request.headers.get("range") : null;
  const ifRange = request.headers.get("if-range");
  const range =
    rangeHeader && (!ifRange || ifRange === object.httpEtag)
      ? requestedRange(rangeHeader, object.size)
      : undefined;
  if (range === null)
    return response(416, request.method, {
      "content-range": `bytes */${object.size}`,
      "accept-ranges": "bytes",
    });
  if (range) {
    headers.set(
      "content-range",
      `bytes ${range.offset}-${range.offset + range.length - 1}/${object.size}`,
    );
    headers.set("content-length", String(range.length));
  }
  const status = range ? 206 : 200;
  if (request.method === "HEAD") return new Response(null, { status, headers });
  const full = await bucket.get(key, range ? { range } : undefined);
  if (
    !full ||
    !full.body ||
    full.size !== object.size ||
    full.httpEtag !== object.httpEtag
  ) {
    await full?.body?.cancel().catch(() => {});
    throw new Error("artifact changed");
  }
  return new Response(full.body, { status, headers });
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    const path = url.pathname;
    // No origin fetch: missing data and exceptions remain inside this Worker.
    if (
      !path.startsWith("/agent/stable/") &&
      !path.startsWith("/download/agent/stable/")
    )
      return response(404, request.method);
    if (!["GET", "HEAD"].includes(request.method))
      return response(405, request.method, { allow: "GET, HEAD" });
    if (
      url.search ||
      path.includes("%") ||
      path.includes("\\") ||
      path.includes("//")
    )
      return response(404, request.method);
    try {
      if (
        path === "/agent/stable/latest.json" ||
        path.startsWith("/download/agent/stable/")
      ) {
        if (
          path.startsWith("/download/") &&
          ![
            "/download/agent/stable/macos",
            "/download/agent/stable/linux-appimage",
          ].includes(path)
        )
          return response(404, request.method);
        const current = await metadata(
          env.AGENT_RELEASES,
          "stable/latest.json",
        );
        if (!current) return response(404, request.method);
        if (path.startsWith("/download/")) {
          const target = path.endsWith("/macos")
            ? current.manifest.downloads.macos
            : current.manifest.downloads["linux-appimage"];
          return target
            ? new Response(null, {
                status: 302,
                headers: { ...NO_STORE, location: target.path },
              })
            : response(404, request.method);
        }
        return new Response(request.method === "HEAD" ? null : current.bytes, {
          headers: {
            ...NO_STORE,
            "content-type": "application/json; charset=utf-8",
            "content-length": String(current.bytes.length),
          },
        });
      }
      const release = releasePath(path);
      if (!release) return response(404, request.method);
      const published = await metadata(
        env.AGENT_RELEASES,
        `stable/releases/${release.version}/manifest.json`,
      );
      if (!published) return response(404, request.method);
      if (published.manifest.version !== release.version)
        throw new Error("version mismatch");
      if (release.file === "manifest.json")
        return new Response(
          request.method === "HEAD" ? null : published.bytes,
          {
            headers: {
              "cache-control": "public, max-age=31536000, immutable",
              "content-type": "application/json; charset=utf-8",
              "content-length": String(published.bytes.length),
              "x-content-type-options": "nosniff",
            },
          },
        );
      const artifact = [
        ...Object.values(published.manifest.artifacts),
        ...Object.values(published.manifest.downloads),
      ].find((value) => value.path === path);
      return artifact
        ? await artifactResponse(
            request,
            env.AGENT_RELEASES,
            release.key,
            artifact,
          )
        : response(404, request.method);
    } catch {
      return response(503, request.method);
    }
  },
};
