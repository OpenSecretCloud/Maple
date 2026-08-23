import { afterEach, describe, expect, spyOn, test } from "bun:test";
import {
  buildFallbackDownloadInfo,
  fetchLatestRelease,
  getLatestDownloadInfo,
  GITHUB_RELEASES_API_URL,
  resolveDownloadUrlsFromRelease
} from "./githubRelease";

let fetchSpy: ReturnType<typeof spyOn<typeof globalThis, "fetch">> | null = null;

afterEach(() => {
  fetchSpy?.mockRestore();
  fetchSpy = null;
});

describe("buildFallbackDownloadInfo", () => {
  test("builds GitHub tag URLs from the packaged version", () => {
    expect(buildFallbackDownloadInfo("3.3.8")).toEqual({
      version: "3.3.8",
      tagName: "v3.3.8",
      downloadUrls: {
        macOS:
          "https://github.com/OpenSecretCloud/Maple/releases/download/v3.3.8/Maple_3.3.8_universal.dmg",
        windowsExe:
          "https://github.com/OpenSecretCloud/Maple/releases/download/v3.3.8/Maple_3.3.8_x64-setup.exe",
        linuxAppImage:
          "https://github.com/OpenSecretCloud/Maple/releases/download/v3.3.8/Maple_3.3.8_amd64.AppImage",
        linuxDeb:
          "https://github.com/OpenSecretCloud/Maple/releases/download/v3.3.8/Maple_3.3.8_amd64.deb",
        linuxRpm:
          "https://github.com/OpenSecretCloud/Maple/releases/download/v3.3.8/Maple_3.3.8_x86_64.rpm",
        androidApk:
          "https://github.com/OpenSecretCloud/Maple/releases/download/v3.3.8/app-universal-release.apk"
      },
      releaseUrl: "https://github.com/OpenSecretCloud/Maple/releases/latest"
    });
  });
});

describe("resolveDownloadUrlsFromRelease", () => {
  test("prefers matched GitHub assets, including the rpmbuild Linux name", () => {
    const urls = resolveDownloadUrlsFromRelease({
      tag_name: "v3.3.7",
      assets: [
        {
          name: "Maple_3.3.7_universal.dmg",
          browser_download_url: "https://example.test/Maple.dmg"
        },
        {
          name: "Maple_3.3.7_x64-setup.exe",
          browser_download_url: "https://example.test/Maple.exe"
        },
        {
          name: "Maple_3.3.7_amd64.AppImage",
          browser_download_url: "https://example.test/Maple.AppImage"
        },
        {
          name: "Maple_3.3.7_amd64.deb",
          browser_download_url: "https://example.test/Maple.deb"
        },
        {
          name: "Maple-3.3.7-1.x86_64.rpm",
          browser_download_url: "https://example.test/Maple.rpm"
        },
        {
          name: "Maple-3.3.7-1.x86_64.rpm.sig",
          browser_download_url: "https://example.test/Maple.rpm.sig"
        },
        {
          name: "app-universal-release.apk",
          browser_download_url: "https://example.test/Maple.apk"
        }
      ]
    });

    expect(urls).toEqual({
      macOS: "https://example.test/Maple.dmg",
      windowsExe: "https://example.test/Maple.exe",
      linuxAppImage: "https://example.test/Maple.AppImage",
      linuxDeb: "https://example.test/Maple.deb",
      linuxRpm: "https://example.test/Maple.rpm",
      androidApk: "https://example.test/Maple.apk"
    });
  });

  test("keeps constructed URLs when an asset is missing", () => {
    const urls = resolveDownloadUrlsFromRelease({
      tag_name: "v3.3.8",
      assets: [
        {
          name: "Maple_3.3.8_universal.dmg",
          browser_download_url: "https://example.test/Maple.dmg"
        }
      ]
    });

    expect(urls.macOS).toBe("https://example.test/Maple.dmg");
    expect(urls.windowsExe).toBe(
      "https://github.com/OpenSecretCloud/Maple/releases/download/v3.3.8/Maple_3.3.8_x64-setup.exe"
    );
  });
});

describe("getLatestDownloadInfo", () => {
  test("returns matched download URLs from the GitHub latest release", async () => {
    fetchSpy = spyOn(globalThis, "fetch").mockImplementation((async (input) => {
      expect(String(input)).toBe(GITHUB_RELEASES_API_URL);
      return new Response(
        JSON.stringify({
          tag_name: "v9.9.9",
          name: "v9.9.9",
          published_at: "2026-08-23T00:00:00Z",
          html_url: "https://github.com/OpenSecretCloud/Maple/releases/tag/v9.9.9",
          assets: [
            {
              name: "Maple_9.9.9_universal.dmg",
              browser_download_url: "https://example.test/Maple_9.9.9_universal.dmg"
            },
            {
              name: "Maple_9.9.9_x64-setup.exe",
              browser_download_url: "https://example.test/Maple_9.9.9_x64-setup.exe"
            }
          ]
        }),
        { status: 200, headers: { "Content-Type": "application/json" } }
      );
    }) as typeof fetch);

    await expect(getLatestDownloadInfo()).resolves.toEqual({
      version: "9.9.9",
      tagName: "v9.9.9",
      downloadUrls: {
        macOS: "https://example.test/Maple_9.9.9_universal.dmg",
        windowsExe: "https://example.test/Maple_9.9.9_x64-setup.exe",
        linuxAppImage:
          "https://github.com/OpenSecretCloud/Maple/releases/download/v9.9.9/Maple_9.9.9_amd64.AppImage",
        linuxDeb:
          "https://github.com/OpenSecretCloud/Maple/releases/download/v9.9.9/Maple_9.9.9_amd64.deb",
        linuxRpm:
          "https://github.com/OpenSecretCloud/Maple/releases/download/v9.9.9/Maple_9.9.9_x86_64.rpm",
        androidApk:
          "https://github.com/OpenSecretCloud/Maple/releases/download/v9.9.9/app-universal-release.apk"
      },
      releaseUrl: "https://github.com/OpenSecretCloud/Maple/releases/tag/v9.9.9"
    });
  });

  test("returns null without logging when the request is aborted", async () => {
    const consoleError = spyOn(console, "error").mockImplementation(() => {});
    const controller = new AbortController();
    controller.abort();
    await expect(fetchLatestRelease(controller.signal)).resolves.toBeNull();
    expect(consoleError).not.toHaveBeenCalled();
    consoleError.mockRestore();
  });
});
