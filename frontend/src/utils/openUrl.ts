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

// State management for confirmation dialog
let urlConfirmationCallback: ((url: string) => void) | null = null;

export function setUrlConfirmationCallback(callback: ((url: string) => void) | null) {
  urlConfirmationCallback = callback;
}

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
      const popup = window.open(url, "_blank", "noopener,noreferrer");
      if (!popup) {
        throw new Error("Failed to open URL - popup may be blocked");
      }
    }
  } catch (error) {
    console.error("Failed to open URL:", url, error);
    // Fallback: try window.open anyway
    try {
      const popup = window.open(url, "_blank", "noopener,noreferrer");
      if (!popup) {
        console.error("Fallback window.open was also blocked");
      }
    } catch (fallbackError) {
      console.error("Fallback also failed:", fallbackError);
    }
  }
}

/**
 * Opens an external URL with user confirmation
 *
 * Shows a confirmation dialog to the user before opening the URL.
 * Requires ExternalUrlConfirmHandler to be mounted in the app.
 *
 * @param url - The URL to open
 *
 * @example
 * ```typescript
 * // In a component
 * import { openExternalUrlWithConfirmation } from '@/utils/openUrl';
 *
 * <a
 *   href={url}
 *   onClick={(e) => {
 *     e.preventDefault();
 *     openExternalUrlWithConfirmation(url);
 *   }}
 * >
 *   Click me
 * </a>
 * ```
 */
export function openExternalUrlWithConfirmation(url: string): void {
  if (urlConfirmationCallback) {
    urlConfirmationCallback(url);
  } else {
    console.warn("URL confirmation callback not set, opening directly");
    openExternalUrl(url);
  }
}
