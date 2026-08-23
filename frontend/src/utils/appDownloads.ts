import type { DownloadUrls } from "./githubRelease";

export type AppDownloadTarget = "macos" | "windows" | "linux" | "ios" | "android";

export type AppDownloadEnvironment = {
  userAgent: string;
  platform?: string;
  maxTouchPoints?: number;
};

export type AppDownloadActionVariant = "primary" | "outline";

export type AppDownloadAction = {
  label: string;
  href: string;
  variant: AppDownloadActionVariant;
};

export const APP_STORE_URL = "https://apps.apple.com/us/app/id6743764835";
export const TESTFLIGHT_URL = "https://testflight.apple.com/join/zjgtyAeD";
export const GOOGLE_PLAY_URL =
  "https://play.google.com/store/apps/details?id=cloud.opensecret.maple";
export const GOOGLE_PLAY_BETA_URL = "https://play.google.com/apps/testing/cloud.opensecret.maple";

export const APP_DOWNLOAD_TARGETS: readonly AppDownloadTarget[] = [
  "macos",
  "windows",
  "linux",
  "ios",
  "android"
];

const TARGET_LABELS: Record<AppDownloadTarget, string> = {
  macos: "macOS",
  windows: "Windows",
  linux: "Linux",
  ios: "iOS",
  android: "Android"
};

const TARGET_COPY: Record<AppDownloadTarget, { title: string; description: string }> = {
  macos: {
    title: "macOS",
    description: "Universal installer for Apple Silicon and Intel Macs running macOS 11.0 or later."
  },
  windows: {
    title: "Windows",
    description: "Installer for Windows PCs."
  },
  linux: {
    title: "Linux",
    description:
      "Ubuntu 24.04+ is the officially supported target. Other distributions are not officially supported."
  },
  ios: {
    title: "iOS",
    description: "Native app for iPhone and iPad."
  },
  android: {
    title: "Android",
    description: "Native app for phones and tablets, or a direct APK from GitHub releases."
  }
};

export function appDownloadTargetLabel(target: AppDownloadTarget): string {
  return TARGET_LABELS[target];
}

export function appDownloadCopy(target: AppDownloadTarget): {
  title: string;
  description: string;
} {
  return TARGET_COPY[target];
}

export function detectAppDownloadTarget(
  environment: AppDownloadEnvironment
): AppDownloadTarget | null {
  const userAgent = environment.userAgent;
  const platform = environment.platform ?? "";
  const maxTouchPoints = environment.maxTouchPoints ?? 0;

  if (/iPhone|iPod/i.test(userAgent) || /iPad/i.test(userAgent)) {
    return "ios";
  }
  if ((/Macintosh|Mac OS X/i.test(userAgent) || /Mac/i.test(platform)) && maxTouchPoints > 1) {
    return "ios";
  }
  if (/Android/i.test(userAgent)) {
    return "android";
  }
  if (/Windows/i.test(userAgent) || /Win32|Win64|Windows/i.test(platform)) {
    return "windows";
  }
  if (/Macintosh|Mac OS X|MacIntel/i.test(userAgent) || /Mac/i.test(platform)) {
    return "macos";
  }
  if (/CrOS/i.test(userAgent) || /Linux/i.test(userAgent) || /Linux/i.test(platform)) {
    return "linux";
  }
  return null;
}

export function readAppDownloadEnvironment(): AppDownloadEnvironment {
  if (typeof navigator === "undefined") {
    return { userAgent: "" };
  }

  return {
    userAgent: navigator.userAgent,
    platform: navigator.platform,
    maxTouchPoints: navigator.maxTouchPoints
  };
}

export function appDownloadActions(
  target: AppDownloadTarget,
  downloadUrls: DownloadUrls
): AppDownloadAction[] {
  switch (target) {
    case "macos":
      return [{ label: "Download for macOS", href: downloadUrls.macOS, variant: "primary" }];
    case "windows":
      return [{ label: "Download for Windows", href: downloadUrls.windowsExe, variant: "primary" }];
    case "linux":
      return [
        { label: "Download AppImage", href: downloadUrls.linuxAppImage, variant: "primary" },
        { label: ".deb", href: downloadUrls.linuxDeb, variant: "outline" },
        { label: ".rpm", href: downloadUrls.linuxRpm, variant: "outline" }
      ];
    case "ios":
      return [
        { label: "App Store", href: APP_STORE_URL, variant: "primary" },
        { label: "TestFlight", href: TESTFLIGHT_URL, variant: "outline" }
      ];
    case "android":
      return [
        { label: "Google Play", href: GOOGLE_PLAY_URL, variant: "primary" },
        { label: "Download APK", href: downloadUrls.androidApk, variant: "outline" },
        { label: "Play Beta", href: GOOGLE_PLAY_BETA_URL, variant: "outline" }
      ];
  }
}
