import { describe, expect, test } from "bun:test";
import {
  appDownloadActions,
  APP_STORE_URL,
  detectAppDownloadTarget,
  GOOGLE_PLAY_URL,
  TESTFLIGHT_URL
} from "./appDownloads";
import { buildFallbackDownloadInfo } from "./githubRelease";

describe("detectAppDownloadTarget", () => {
  test("detects iPhone Safari as iOS", () => {
    expect(
      detectAppDownloadTarget({
        userAgent:
          "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1"
      })
    ).toBe("ios");
  });

  test("detects iPadOS desktop UA as iOS when the device has a touch screen", () => {
    expect(
      detectAppDownloadTarget({
        userAgent:
          "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15",
        platform: "MacIntel",
        maxTouchPoints: 5
      })
    ).toBe("ios");
  });

  test("detects macOS Safari as macOS", () => {
    expect(
      detectAppDownloadTarget({
        userAgent:
          "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15",
        platform: "MacIntel",
        maxTouchPoints: 0
      })
    ).toBe("macos");
  });

  test("detects Windows Chrome as Windows", () => {
    expect(
      detectAppDownloadTarget({
        userAgent:
          "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        platform: "Win32"
      })
    ).toBe("windows");
  });

  test("detects Android Chrome as Android rather than Linux", () => {
    expect(
      detectAppDownloadTarget({
        userAgent:
          "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36"
      })
    ).toBe("android");
  });

  test("detects Ubuntu Chrome as Linux", () => {
    expect(
      detectAppDownloadTarget({
        userAgent:
          "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        platform: "Linux x86_64"
      })
    ).toBe("linux");
  });

  test("returns null for an unrecognized user agent", () => {
    expect(detectAppDownloadTarget({ userAgent: "UnknownClient/1.0" })).toBeNull();
  });
});

describe("appDownloadActions", () => {
  const urls = buildFallbackDownloadInfo("3.3.8").downloadUrls;

  test("offers the macOS dmg as the primary action", () => {
    expect(appDownloadActions("macos", urls)).toEqual([
      { label: "Download for macOS", href: urls.macOS, variant: "primary" }
    ]);
  });

  test("offers Linux package choices", () => {
    expect(appDownloadActions("linux", urls).map((action) => action.label)).toEqual([
      "Download AppImage",
      ".deb",
      ".rpm"
    ]);
  });

  test("uses store URLs for mobile platforms", () => {
    expect(appDownloadActions("ios", urls).map((action) => action.href)).toEqual([
      APP_STORE_URL,
      TESTFLIGHT_URL
    ]);
    expect(appDownloadActions("android", urls)[0]?.href).toBe(GOOGLE_PLAY_URL);
    expect(appDownloadActions("android", urls)[1]?.href).toBe(urls.androidApk);
  });
});
