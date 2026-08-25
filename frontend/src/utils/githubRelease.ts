/**
 * Utility for fetching the latest GitHub release information and matching
 * installable assets the same way the Maple marketing site does.
 */

import repositoryMetadata from "../../../repo.meta.json";

export const GITHUB_REPOSITORY = `${repositoryMetadata.github.owner}/${repositoryMetadata.github.repository}`;
export const GITHUB_REPOSITORY_URL = `https://github.com/${GITHUB_REPOSITORY}`;

export const GITHUB_RELEASES_API_URL = `https://api.github.com/repos/${GITHUB_REPOSITORY}/releases/latest`;
export const GITHUB_RELEASES_LATEST_URL = `${GITHUB_REPOSITORY_URL}/releases/latest`;

export type DownloadAssetKey =
  | "macOS"
  | "windowsExe"
  | "linuxAppImage"
  | "linuxDeb"
  | "linuxRpm"
  | "androidApk";

export type DownloadUrls = Record<DownloadAssetKey, string>;

export interface DownloadInfo {
  version: string;
  tagName: string;
  downloadUrls: DownloadUrls;
  releaseUrl: string;
}

interface GitHubReleaseAsset {
  name: string;
  browser_download_url: string;
}

interface GitHubRelease {
  tag_name: string;
  name: string;
  published_at: string;
  html_url: string;
  assets?: GitHubReleaseAsset[];
}

const DOWNLOAD_ASSET_MATCHERS: Record<DownloadAssetKey, (name: string) => boolean> = {
  macOS: (name) => name.endsWith("_universal.dmg"),
  windowsExe: (name) => name.endsWith("_x64-setup.exe"),
  linuxAppImage: (name) => name.endsWith("_amd64.AppImage"),
  linuxDeb: (name) => name.endsWith("_amd64.deb"),
  linuxRpm: (name) => name.endsWith("_x86_64.rpm") || name.endsWith(".x86_64.rpm"),
  androidApk: (name) => name === "app-universal-release.apk"
};

const DOWNLOAD_ASSET_KEYS = Object.keys(DOWNLOAD_ASSET_MATCHERS) as DownloadAssetKey[];

function isAbortError(error: unknown): boolean {
  return error instanceof Error && error.name === "AbortError";
}

function versionFromTag(tagName: string): string {
  return tagName.startsWith("v") ? tagName.slice(1) : tagName;
}

function tagFromVersion(version: string): string {
  return version.startsWith("v") ? version : `v${version}`;
}

function latestReleaseDownloadUrls(): DownloadUrls {
  return {
    macOS: GITHUB_RELEASES_LATEST_URL,
    windowsExe: GITHUB_RELEASES_LATEST_URL,
    linuxAppImage: GITHUB_RELEASES_LATEST_URL,
    linuxDeb: GITHUB_RELEASES_LATEST_URL,
    linuxRpm: GITHUB_RELEASES_LATEST_URL,
    androidApk: GITHUB_RELEASES_LATEST_URL
  };
}

export function buildFallbackDownloadInfo(version: string): DownloadInfo {
  const tagName = tagFromVersion(version);
  const normalizedVersion = versionFromTag(tagName);
  return {
    version: normalizedVersion,
    tagName,
    downloadUrls: latestReleaseDownloadUrls(),
    releaseUrl: GITHUB_RELEASES_LATEST_URL
  };
}

function isGitHubRelease(value: unknown): value is GitHubRelease {
  if (!value || typeof value !== "object") {
    return false;
  }

  const release = value as GitHubRelease;
  return (
    typeof release.tag_name === "string" &&
    typeof release.name === "string" &&
    typeof release.published_at === "string" &&
    typeof release.html_url === "string"
  );
}

export function resolveDownloadUrlsFromRelease(
  release: Pick<GitHubRelease, "assets">
): DownloadUrls {
  const resolved = latestReleaseDownloadUrls();
  const assets = Array.isArray(release.assets) ? release.assets : [];

  for (const key of DOWNLOAD_ASSET_KEYS) {
    const matcher = DOWNLOAD_ASSET_MATCHERS[key];
    const asset = assets.find(
      (candidate) =>
        typeof candidate?.name === "string" &&
        typeof candidate.browser_download_url === "string" &&
        matcher(candidate.name)
    );
    if (asset?.browser_download_url) {
      resolved[key] = asset.browser_download_url;
    }
  }

  return resolved;
}

/**
 * Fetches the latest release from GitHub
 */
export async function fetchLatestRelease(signal?: AbortSignal): Promise<GitHubRelease | null> {
  try {
    const response = await fetch(GITHUB_RELEASES_API_URL, {
      headers: { Accept: "application/vnd.github+json" },
      signal
    });

    if (!response.ok) {
      console.error("Failed to fetch latest release:", response.status);
      return null;
    }

    const data: unknown = await response.json();
    if (!isGitHubRelease(data)) {
      console.error("Invalid release data format from GitHub API");
      return null;
    }
    return data;
  } catch (error) {
    if (isAbortError(error)) {
      return null;
    }
    console.error("Error fetching latest release:", error);
    return null;
  }
}

/**
 * Gets download information for the latest release
 */
export async function getLatestDownloadInfo(signal?: AbortSignal): Promise<DownloadInfo | null> {
  const release = await fetchLatestRelease(signal);

  if (!release) {
    return null;
  }

  const version = versionFromTag(release.tag_name);

  return {
    version,
    tagName: release.tag_name,
    downloadUrls: resolveDownloadUrlsFromRelease(release),
    releaseUrl: release.html_url
  };
}
