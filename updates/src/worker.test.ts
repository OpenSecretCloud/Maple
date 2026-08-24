import { describe, expect, test } from "bun:test";

import { handleRequest, isLatestRelease, type Env } from "./worker";

function releaseUrl(version: string, name: string): string {
  return `https://github.com/OpenSecretCloud/Maple/releases/download/v${version}/${name}`;
}

function validRelease(version = "3.3.8") {
  const appImage = {
    signature: "RWQ-test-appimage-signature",
    url: releaseUrl(version, `Maple_${version}_amd64.AppImage`),
  };

  return {
    notes: `See the release notes for v${version}`,
    platforms: {
      "darwin-aarch64": {
        signature: "RWQ-test-macos-signature",
        url: releaseUrl(version, `Maple_${version}_universal.app.tar.gz`),
      },
      "darwin-x86_64": {
        signature: "RWQ-test-macos-signature",
        url: releaseUrl(version, `Maple_${version}_universal.app.tar.gz`),
      },
      "linux-x86_64": appImage,
      "linux-x86_64-appimage": appImage,
      "linux-x86_64-deb": {
        signature: "RWQ-test-deb-signature",
        url: releaseUrl(version, `Maple_${version}_amd64.deb`),
      },
      "linux-x86_64-rpm": {
        signature: "RWQ-test-rpm-signature",
        url: releaseUrl(version, `Maple-${version}-1.x86_64.rpm`),
      },
      "windows-x86_64": {
        signature: "RWQ-test-windows-signature",
        url: releaseUrl(version, `Maple_${version}_x64-setup.exe`),
      },
    },
    pub_date: "2026-08-24T20:00:00Z",
    version,
  };
}

function envReturning(response: Response, requests: Request[] = []): Env {
  return {
    ASSETS: {
      async fetch(request) {
        requests.push(request);
        return response.clone();
      },
    },
  };
}

describe("latest.json validation", () => {
  test("accepts the Maple release schema", () => {
    expect(isLatestRelease(validRelease())).toBe(true);
  });

  test("rejects mismatched tags and non-GitHub artifact URLs", () => {
    const wrongTag = validRelease();
    wrongTag.platforms["windows-x86_64"].url = releaseUrl(
      "3.3.7",
      "Maple_3.3.7_x64-setup.exe",
    );
    expect(isLatestRelease(wrongTag)).toBe(false);

    const wrongHost = validRelease();
    wrongHost.platforms["windows-x86_64"].url =
      "https://downloads.example.com/Maple_3.3.8_x64-setup.exe";
    expect(isLatestRelease(wrongHost)).toBe(false);

    const nonDefaultPort = validRelease();
    nonDefaultPort.platforms["windows-x86_64"].url =
      "https://github.com:8443/OpenSecretCloud/Maple/releases/download/v3.3.8/Maple_3.3.8_x64-setup.exe";
    expect(isLatestRelease(nonDefaultPort)).toBe(false);
  });

  test("rejects malformed timestamps and invalid extra platforms", () => {
    const dateOnly = validRelease();
    dateOnly.pub_date = "2026-08-24";
    expect(isLatestRelease(dateOnly)).toBe(false);

    const impossibleDate = validRelease();
    impossibleDate.pub_date = "2026-02-30T20:00:00Z";
    expect(isLatestRelease(impossibleDate)).toBe(false);

    const invalidExtraPlatform = validRelease() as ReturnType<
      typeof validRelease
    > & {
      platforms: Record<string, { signature: string; url: string }>;
    };
    invalidExtraPlatform.platforms["future-target"] = {
      signature: "",
      url: "https://downloads.example.com/untrusted",
    };
    expect(isLatestRelease(invalidExtraPlatform)).toBe(false);
  });
});

describe("updates Worker", () => {
  test("returns 404 for every path except latest.json without reading assets", async () => {
    let assetFetches = 0;
    const response = await handleRequest(
      new Request("https://updates.trymaple.ai/"),
      {
        ASSETS: {
          async fetch() {
            assetFetches += 1;
            return new Response(null, { status: 404 });
          },
        },
      },
    );

    expect(response.status).toBe(404);
    expect(response.headers.get("cache-control")).toBe("no-store");
    expect(assetFetches).toBe(0);
  });

  test("allows only GET and HEAD for latest.json", async () => {
    const response = await handleRequest(
      new Request("https://updates.trymaple.ai/latest.json", {
        method: "POST",
      }),
      envReturning(new Response(null, { status: 404 })),
    );

    expect(response.status).toBe(405);
    expect(response.headers.get("allow")).toBe("GET, HEAD");
  });

  test("returns 404 while latest.json is absent", async () => {
    const response = await handleRequest(
      new Request("https://updates.trymaple.ai/latest.json"),
      envReturning(new Response(null, { status: 404 })),
    );

    expect(response.status).toBe(404);
    expect(response.headers.get("content-type")).toBe(
      "text/plain; charset=utf-8",
    );
  });

  test("serves validated JSON with the public cache contract", async () => {
    const requests: Request[] = [];
    const metadata = JSON.stringify(validRelease());
    const response = await handleRequest(
      new Request("https://updates.trymaple.ai/latest.json", {
        headers: { authorization: "Bearer should-not-be-forwarded" },
      }),
      envReturning(
        new Response(metadata, {
          headers: {
            etag: '"release-etag"',
            "last-modified": "Mon, 24 Aug 2026 20:00:00 GMT",
          },
        }),
        requests,
      ),
    );

    expect(response.status).toBe(200);
    expect(await response.json()).toEqual(validRelease());
    expect(response.headers.get("content-type")).toBe(
      "application/json; charset=utf-8",
    );
    expect(response.headers.get("cache-control")).toBe(
      "public, max-age=0, must-revalidate, no-transform",
    );
    expect(response.headers.get("cdn-cache-control")).toBe("no-store");
    expect(response.headers.get("etag")).toBe('"release-etag"');
    expect(requests).toHaveLength(1);
    expect(requests[0].headers.get("accept")).toBe("application/json");
    expect(requests[0].headers.has("authorization")).toBe(false);
  });

  test("serves HEAD with GET headers and no body", async () => {
    const metadata = JSON.stringify(validRelease());
    const response = await handleRequest(
      new Request("https://updates.trymaple.ai/latest.json", {
        method: "HEAD",
      }),
      envReturning(new Response(metadata)),
    );

    expect(response.status).toBe(200);
    expect(response.headers.get("content-type")).toBe(
      "application/json; charset=utf-8",
    );
    expect(await response.text()).toBe("");
  });

  test("returns 503 for invalid, oversized, or unavailable metadata", async () => {
    const invalid = await handleRequest(
      new Request("https://updates.trymaple.ai/latest.json"),
      envReturning(new Response("<html>challenge</html>")),
    );
    expect(invalid.status).toBe(503);

    const oversized = await handleRequest(
      new Request("https://updates.trymaple.ai/latest.json"),
      envReturning(
        new Response("{}", {
          headers: { "content-length": String(64 * 1024 + 1) },
        }),
      ),
    );
    expect(oversized.status).toBe(503);

    const unavailable = await handleRequest(
      new Request("https://updates.trymaple.ai/latest.json"),
      envReturning(new Response(null, { status: 503 })),
    );
    expect(unavailable.status).toBe(503);
  });
});
