/**
 * Opens an external URL using the Tauri opener plugin when available,
 * with fallback to window.open for web environments.
 *
 * @param url - The external URL to open
 */
export const openExternalLink = async (url: string): Promise<void> => {
  try {
    // Use Tauri opener plugin to open external URLs in the device's default browser
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("plugin:opener|open_url", { url });
  } catch (error) {
    // Fallback for non-Tauri environments (e.g., web)
    console.warn("Failed to open URL with Tauri opener, falling back to window.open:", error);
    try {
      window.open(url, "_blank", "noopener,noreferrer");
    } catch (windowError) {
      console.error("Failed to open URL with window.open:", windowError);
    }
  }
};
