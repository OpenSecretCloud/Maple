/**
 * Cross-platform utility for opening external links
 * Handles iOS-specific behavior where links need to be opened using Tauri's opener plugin
 */

interface OpenExternalLinkOptions {
  fallbackBehavior?: "window.open" | "location.href";
  windowFeatures?: string;
}

export async function openExternalLink(
  url: string,
  options: OpenExternalLinkOptions = {}
): Promise<void> {
  const { fallbackBehavior = "window.open", windowFeatures } = options;

  try {
    // Dynamic import to avoid issues in non-Tauri environments
    const { isTauri } = await import("@tauri-apps/api/core").catch(() => ({
      isTauri: () => false
    }));

    if (isTauri()) {
      try {
        const { type } = await import("@tauri-apps/plugin-os");
        const platform = await type();

        // On iOS, we must use the Tauri opener plugin
        if (platform === "ios") {
          const { invoke } = await import("@tauri-apps/api/core");
          await invoke("plugin:opener|open_url", { url });
          return;
        }
      } catch (error) {
        console.error("Failed to open link with Tauri opener:", error);
        // Fall through to default behavior
      }
    }

    // Default behavior for non-iOS platforms or if Tauri is not available
    if (fallbackBehavior === "location.href") {
      window.location.href = url;
    } else {
      window.open(url, "_blank", windowFeatures);
    }
  } catch (error) {
    console.error("Failed to open external link:", error);
    // Final fallback
    window.open(url, "_blank", windowFeatures);
  }
}
