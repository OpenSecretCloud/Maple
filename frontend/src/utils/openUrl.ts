/**
 * Utility for opening external URLs
 *
 * This module provides a unified way to open external URLs that works
 * correctly in both web browsers and Tauri environments.
 *
 * In Tauri apps, regular <a> tags with target="_blank" don't work properly.
 * We need to use the Tauri opener plugin to open URLs in the system browser.
 */

import { isTauri } from "@/utils/platform";

/**
 * Opens an external URL in the appropriate way for the current environment
 *
 * - In web browsers: uses window.open()
 * - In Tauri apps: uses the opener plugin to open in system browser
 *
 * @param url - The URL to open
 * @returns Promise that resolves when the URL is opened (or rejects on error)
 *
 * @example
 * ```typescript
 * // In a component
 * import { openExternalUrl } from '@/utils/openUrl';
 *
 * <a
 *   href={url}
 *   onClick={(e) => {
 *     e.preventDefault();
 *     openExternalUrl(url);
 *   }}
 * >
 *   Click me
 * </a>
 * ```
 */
export async function openExternalUrl(url: string): Promise<void> {
  try {
    if (isTauri()) {
      // In Tauri, use the opener plugin
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(url);
    } else {
      // In web browsers, use window.open
      window.open(url, "_blank", "noopener,noreferrer");
    }
  } catch (error) {
    console.error("Failed to open URL:", url, error);
    // Fallback: try window.open anyway
    try {
      window.open(url, "_blank", "noopener,noreferrer");
    } catch (fallbackError) {
      console.error("Fallback also failed:", fallbackError);
    }
  }
}
