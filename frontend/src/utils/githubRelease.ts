/**
 * Utility for fetching the latest GitHub release information
 */

interface GitHubRelease {
  tag_name: string;
  name: string;
  published_at: string;
  html_url: string;
}

interface DownloadInfo {
  version: string;
  tagName: string;
  downloadUrls: {
    macOS: string;
    linuxAppImage: string;
    linuxDeb: string;
    linuxRpm: string;
  };
  releaseUrl: string;
}

/**
 * Fetches the latest release from GitHub
 */
export async function fetchLatestRelease(): Promise<GitHubRelease | null> {
  try {
    const response = await fetch(
      "https://api.github.com/repos/OpenSecretCloud/Maple/releases/latest"
    );
    
    if (!response.ok) {
      console.error("Failed to fetch latest release:", response.status);
      return null;
    }
    
    const release: GitHubRelease = await response.json();
    return release;
  } catch (error) {
    console.error("Error fetching latest release:", error);
    return null;
  }
}

/**
 * Gets download information for the latest release
 */
export async function getLatestDownloadInfo(): Promise<DownloadInfo | null> {
  const release = await fetchLatestRelease();
  
  if (!release) {
    return null;
  }
  
  // Extract version number from tag (remove 'v' prefix if present)
  const version = release.tag_name.startsWith("v") 
    ? release.tag_name.slice(1) 
    : release.tag_name;
  
  const baseDownloadUrl = `https://github.com/OpenSecretCloud/Maple/releases/download/${release.tag_name}`;
  
  return {
    version,
    tagName: release.tag_name,
    downloadUrls: {
      macOS: `${baseDownloadUrl}/Maple_${version}_universal.dmg`,
      linuxAppImage: `${baseDownloadUrl}/Maple_${version}_amd64.AppImage`,
      linuxDeb: `${baseDownloadUrl}/Maple_${version}_amd64.deb`,
      linuxRpm: `${baseDownloadUrl}/Maple-${version}-1.x86_64.rpm`,
    },
    releaseUrl: release.html_url,
  };
}