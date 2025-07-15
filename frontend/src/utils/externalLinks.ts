/**
 * Opens an external URL using the Tauri opener plugin when running on iOS in a Tauri environment,
 * with fallback to window.open for web environments and other platforms.
 *
 * @param url - The external URL to open
 */
export const openExternalLink = async (url: string): Promise<void> => {
  try {
    // Check if we're in a Tauri environment first
    const { isTauri } = await import("@tauri-apps/api/core");
    const tauriEnv = await isTauri();

    if (tauriEnv) {
      // Check if we're on iOS
      const { type } = await import("@tauri-apps/plugin-os");
      const platform = await type();

      if (platform === "ios") {
        // Use Tauri opener plugin for iOS
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("plugin:opener|open_url", { url });
        return;
      }
    }

    // Fallback for non-iOS or non-Tauri environments
    window.open(url, "_blank", "noopener,noreferrer");
  } catch (error) {
    // Final fallback if everything fails
    console.warn("Failed to open URL with Tauri opener, falling back to window.open:", error);
    try {
      window.open(url, "_blank", "noopener,noreferrer");
    } catch (windowError) {
      console.error("Failed to open URL with window.open:", windowError);
    }
  }
};
