import { expect, test } from "bun:test";
import worker, { type Bucket } from "./worker";
import { fixture, encodeFixture, PAYLOAD } from "./fixtures";
import {
  MAX_ENVELOPE_BYTES,
  parseEnvelope,
  parseManifest,
  releasePath,
} from "./manifest";

class MemoryBucket implements Bucket {
  objects = new Map<string, Uint8Array<ArrayBuffer>>();
  calls: string[] = [];
  fail = false;
  async get(
    key: string,
    options?: { range: { offset: number; length: number } },
  ) {
    this.calls.push(`get:${key}`);
    if (this.fail) throw new Error("private fixture error");
    const bytes = this.objects.get(key);
    if (!bytes) return null;
    const range = options?.range;
    const body = range
      ? bytes.slice(range.offset, range.offset + range.length)
      : bytes;
    return {
      size: bytes.length,
      httpEtag: '"fixture"',
      body: new Response(body).body!,
    };
  }
  async head(key: string) {
    this.calls.push(`head:${key}`);
    if (this.fail) throw new Error("private fixture error");
    const bytes = this.objects.get(key);
    return bytes ? { size: bytes.length, httpEtag: '"fixture"' } : null;
  }
}
function setup(linux = false) {
  const bucket = new MemoryBucket();
  const current = fixture(linux);
  bucket.objects.set("stable/latest.json", current.bytes);
  bucket.objects.set("stable/releases/0.0.2/manifest.json", current.bytes);
  for (const artifact of [
    ...Object.values(current.manifest.artifacts),
    ...Object.values(current.manifest.downloads),
  ])
    bucket.objects.set(artifact.path.replace("/agent/", ""), PAYLOAD);
  const fetch = (path: string, init?: RequestInit) =>
    worker.fetch(new Request(`https://updates.example${path}`, init), {
      AGENT_RELEASES: bucket,
    });
  return { bucket, fetch, ...current };
}

test("latest exact envelope and HEAD are never cached", async () => {
  const { fetch, bytes } = setup();
  for (const method of ["GET", "HEAD"]) {
    const result = await fetch("/agent/stable/latest.json", { method });
    expect(result.status).toBe(200);
    expect(result.headers.get("cache-control")).toBe("no-store");
    expect(result.headers.get("cdn-cache-control")).toBe("no-store");
    expect(result.headers.get("content-length")).toBe(String(bytes.length));
    expect(new Uint8Array(await result.arrayBuffer())).toEqual(
      method === "HEAD" ? new Uint8Array() : bytes,
    );
  }
});
test("downloads are uncached origin-relative redirects; omitted Linux is isolated", async () => {
  for (const linux of [false, true]) {
    const { fetch } = setup(linux);
    for (const method of ["GET", "HEAD"]) {
      const mac = await fetch("/download/agent/stable/macos", { method });
      expect(mac.status).toBe(302);
      expect(mac.headers.get("location")).toBe(
        "/agent/stable/releases/0.0.2/Maple-Agent.dmg",
      );
      expect(mac.headers.get("cache-control")).toBe("no-store");
      expect(
        (await fetch("/download/agent/stable/linux-appimage", { method }))
          .status,
      ).toBe(linux ? 302 : 404);
    }
  }
});
test("immutable artifact GET, HEAD, ranges, validators and If-Range", async () => {
  const { fetch, bucket } = setup();
  const path = "/agent/stable/releases/0.0.2/Maple-Agent.app.tar.gz";
  const result = await fetch(path);
  expect(await result.text()).toBe(new TextDecoder().decode(PAYLOAD));
  expect(result.headers.get("cache-control")).toContain("immutable");
  for (const [range, expected] of [
    ["bytes=2-6", PAYLOAD.slice(2, 7)],
    ["bytes=2-", PAYLOAD.slice(2)],
    ["bytes=-4", PAYLOAD.slice(-4)],
    ["bytes=0-9999", PAYLOAD],
  ]) {
    const result = await fetch(path, { headers: { range: range as string } });
    expect(result.status).toBe(206);
    expect(new Uint8Array(await result.arrayBuffer())).toEqual(
      expected as Uint8Array<ArrayBuffer>,
    );
  }
  for (const range of [
    "bytes=9999-",
    "bytes=-0",
    "bytes=3-2",
    "bytes=0-1,4-5",
    "bytes=-",
    "items=0-1",
    "bytes=999999999999999999999-9999999999999999999999",
  ])
    expect((await fetch(path, { headers: { range } })).status).toBe(416);
  expect(
    (
      await fetch(path, {
        headers: { range: "bytes=0-2", "if-range": '"changed"' },
      })
    ).status,
  ).toBe(200);
  expect(
    (await fetch(path, { headers: { "if-none-match": '"fixture"' } })).status,
  ).toBe(304);
  bucket.calls = [];
  const head = await fetch(path, {
    method: "HEAD",
    headers: { range: "bytes=0-2" },
  });
  expect(head.status).toBe(200);
  expect(head.headers.get("content-length")).toBe(String(PAYLOAD.length));
  expect(head.headers.get("content-range")).toBeNull();
  expect(await head.text()).toBe("");
  expect(bucket.calls).not.toContain(
    "get:stable/releases/0.0.2/Maple-Agent.app.tar.gz",
  );
});
test("immutable manifest is exact, but unsigned auxiliary objects are private", async () => {
  const { fetch, bucket, bytes } = setup();
  const response = await fetch("/agent/stable/releases/0.0.2/manifest.json");
  expect(response.status).toBe(200);
  expect(new Uint8Array(await response.arrayBuffer())).toEqual(bytes);
  for (const file of [
    "private.json",
    "publication-manifest.json",
    "publication-metadata.json",
    "metadata/macos-build-info.json",
  ]) {
    bucket.objects.set(`stable/releases/0.0.2/${file}`, PAYLOAD);
    expect((await fetch(`/agent/stable/releases/0.0.2/${file}`)).status).toBe(
      404,
    );
  }
});
test("unknown paths, methods and ambiguous encodings cannot reach arbitrary bucket keys", async () => {
  const { fetch, bucket } = setup();
  for (const path of [
    "/latest.json",
    "/download/research/stable/macos",
    "/agent/stable/random",
    "/agent/stable/latest.json?secret=value",
    "/agent/stable/releases/0.0.2/x%2fy.dmg",
    "/agent/stable/releases/00.0.2/a.dmg",
    "/agent/stable//latest.json",
    "/agent/stable/releases/0.0.2/a..dmg",
    "/download/agent/stable/unknown",
  ])
    expect((await fetch(path)).status).toBe(404);
  expect(bucket.calls).toEqual([]);
  const post = await fetch("/agent/stable/latest.json", { method: "POST" });
  expect(post.status).toBe(405);
  expect(post.headers.get("allow")).toBe("GET, HEAD");
});
test("missing and malformed data and R2 failures fail only Agent with bounded errors", async () => {
  const { fetch, bucket } = setup();
  bucket.objects.delete("stable/latest.json");
  expect((await fetch("/agent/stable/latest.json")).status).toBe(404);
  for (const bad of [
    new Uint8Array(MAX_ENVELOPE_BYTES + 1),
    new TextEncoder().encode("{}"),
    new Uint8Array([0xff]),
  ]) {
    bucket.objects.set("stable/latest.json", bad);
    for (const path of [
      "/agent/stable/latest.json",
      "/download/agent/stable/macos",
    ]) {
      const result = await fetch(path);
      expect(result.status).toBe(503);
      expect(await result.text()).toBe("503\n");
    }
  }
  bucket.fail = true;
  expect((await fetch("/agent/stable/latest.json")).status).toBe(503);
  expect((await fetch("/latest.json")).status).toBe(404);
});
test("size mismatch and incomplete publication cannot stream corrupt or partial artifacts", async () => {
  const { fetch, bucket } = setup();
  const path = "/agent/stable/releases/0.0.2/Maple-Agent.dmg";
  bucket.objects.set(
    "stable/releases/0.0.2/Maple-Agent.dmg",
    new Uint8Array([1]),
  );
  expect((await fetch(path)).status).toBe(503);
  bucket.objects.delete("stable/releases/0.0.2/manifest.json");
  expect((await fetch(path)).status).toBe(404);
});
test("metadata stream size is enforced independently of the bucket's declared size", async () => {
  for (const oversized of [false, true]) {
    const { fetch, bucket, bytes } = setup();
    let cancelled = false;
    const stream = oversized
      ? new ReadableStream<Uint8Array<ArrayBuffer>>({
          start(controller) {
            controller.enqueue(new Uint8Array(MAX_ENVELOPE_BYTES + 1));
          },
          cancel() {
            cancelled = true;
          },
        })
      : new Response(bytes).body!;
    bucket.get = async () => ({
      size: oversized ? MAX_ENVELOPE_BYTES : bytes.length + 1,
      httpEtag: '"fixture"',
      body: stream,
    });
    const result = await fetch("/agent/stable/latest.json");
    expect(result.status).toBe(503);
    if (oversized) expect(cancelled).toBe(true);
  }
});
test("schema binds product, channel, version, target, path, size, hashes and signature comment", () => {
  const mutations = [
    (v: any) => {
      v.product = "research";
    },
    (v: any) => {
      v.channel = "dev";
    },
    (v: any) => {
      v.version = "0.0.2-beta";
    },
    (v: any) => {
      v.build = "01";
    },
    (v: any) => {
      v.build = "2\n";
    },
    (v: any) => {
      v.source_sha += "\n";
    },
    (v: any) => {
      v.downloads.macos.sha256 += "\n";
    },
    (v: any) => {
      v.version += "\n";
    },
    (v: any) => {
      v.pub_date = "2026-02-30T00:00:00Z";
    },
    (v: any) => {
      v.notes = "\u0000";
    },
    (v: any) => {
      v.notes = "\ud800";
    },
    (v: any) => {
      v.notes = "é".repeat(8193);
    },
    (v: any) => {
      v.extra = true;
    },
    (v: any) => {
      v.artifacts["windows-x86_64"] = v.artifacts["darwin-aarch64"];
    },
    (v: any) => {
      delete v.artifacts["darwin-aarch64"];
    },
    (v: any) => {
      v.downloads.macos.path = "https://evil.example/a.dmg";
    },
    (v: any) => {
      v.downloads.macos.path = "/agent/stable/releases/0.0.1/a.dmg";
    },
    (v: any) => {
      v.downloads.macos.path += "?x=y";
    },
    (v: any) => {
      v.downloads.macos.path = "/agent/stable/releases/0.0.2/../a.dmg";
    },
    (v: any) => {
      v.downloads.macos.path = "/agent/stable/releases/0.0.2/a%2fb.dmg";
    },
    (v: any) => {
      v.downloads.macos.size = 4 * 1024 ** 3 + 1;
    },
    (v: any) => {
      v.downloads.macos.sha256 = "A".repeat(64);
    },
    (v: any) => {
      v.artifacts["linux-x86_64"] = v.downloads.macos;
    },
  ];
  for (const change of mutations) {
    const { manifest } = fixture();
    change(manifest);
    expect(() => parseEnvelope(encodeFixture(manifest))).toThrow();
  }
  const envelope = JSON.parse(new TextDecoder().decode(fixture().bytes));
  envelope.signature = envelope.signature.replace(
    "version=0.0.2",
    "version=0.0.3",
  );
  expect(() =>
    parseEnvelope(new TextEncoder().encode(JSON.stringify(envelope))),
  ).toThrow();
  expect(releasePath("/agent/stable/releases/0.0.2/manifest.json")?.key).toBe(
    "stable/releases/0.0.2/manifest.json",
  );
  expect(
    releasePath("/agent/stable/releases/0.0.2\n/manifest.json"),
  ).toBeNull();
  const noncanonical = fixture().manifest;
  noncanonical.version += "\n";
  for (const artifact of [
    ...Object.values(noncanonical.artifacts),
    ...Object.values(noncanonical.downloads),
  ]) {
    artifact.path = artifact.path.replace("/0.0.2/", "/0.0.2\n/");
  }
  expect(() => parseManifest(noncanonical)).toThrow();
});
