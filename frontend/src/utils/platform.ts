/**
 * Platform detection utilities for Tauri applications
 *
 * This module provides crash-proof, reliable platform detection that is
 * guaranteed to be correct before the app renders. Platform is detected
 * ONCE at startup and cached for instant, synchronous access thereafter.
 *
 * The platform MUST be initialized via waitForPlatform() in main.tsx
 * before rendering the React app.
 */

/**
 * Platform types supported by the application
 */
export type PlatformType = "ios" | "android" | "macos" | "windows" | "linux" | "web";

/**
 * Comprehensive platform information
 */
export interface PlatformInfo {
  /** The specific platform type */
  platform: PlatformType;
  /** Whether the app is running in a Tauri environment */
  isTauri: boolean;
  /** Whether the platform is iOS */
  isIOS: boolean;
  /** Whether the platform is Android */
  isAndroid: boolean;
  /** Whether the platform is any mobile platform (iOS or Android) */
  isMobile: boolean;
  /** Whether the platform is any desktop platform (macOS, Windows, Linux) */
  isDesktop: boolean;
  /** Whether the platform is macOS */
  isMacOS: boolean;
  /** Whether the platform is Windows */
  isWindows: boolean;
  /** Whether the platform is Linux */
  isLinux: boolean;
  /** Whether the app is running in a web browser (not Tauri) */
  isWeb: boolean;
  /** Whether the app is running in Tauri desktop environment */
  isTauriDesktop: boolean;
  /** Whether the app is running in Tauri mobile environment */
  isTauriMobile: boolean;
}

/**
 * Platform info singleton - ALWAYS set before app renders
 * Never null after initialization
 */
let platformInfo: PlatformInfo | null = null;

/**
 * Promise that resolves when platform detection is complete
 * This runs immediately when the module loads
 */
const platformReady = (async () => {
  try {
    // Try to detect if we're in a Tauri environment
    const tauriEnv = await import("@tauri-apps/api/core")
      .then((m) => m.isTauri())
      .catch(() => false);

    if (tauriEnv) {
      // We're in Tauri - get the actual platform type
      try {
        const { type } = await import("@tauri-apps/plugin-os");
        const platform = await type();

        // Set comprehensive platform info for Tauri environment
        platformInfo = {
          platform: platform as PlatformType,
          isTauri: true,
          isIOS: platform === "ios",
          isAndroid: platform === "android",
          isMobile: platform === "ios" || platform === "android",
          isDesktop: platform === "macos" || platform === "windows" || platform === "linux",
          isMacOS: platform === "macos",
          isWindows: platform === "windows",
          isLinux: platform === "linux",
          isWeb: false,
          isTauriDesktop: platform === "macos" || platform === "windows" || platform === "linux",
          isTauriMobile: platform === "ios" || platform === "android"
        };

        console.log("[Platform] Detected Tauri environment:", platform);
      } catch (error) {
        // Tauri is available but we couldn't get the platform type
        // This shouldn't happen in practice, but we handle it gracefully
        console.error("[Platform] Failed to get Tauri platform type:", error);

        // Default to desktop Linux as a safe fallback for Tauri environments
        platformInfo = {
          platform: "linux",
          isTauri: true,
          isIOS: false,
          isAndroid: false,
          isMobile: false,
          isDesktop: true,
          isMacOS: false,
          isWindows: false,
          isLinux: true,
          isWeb: false,
          isTauriDesktop: true,
          isTauriMobile: false
        };
      }
    } else {
      // We're in a web browser
      platformInfo = {
        platform: "web",
        isTauri: false,
        isIOS: false,
        isAndroid: false,
        isMobile: false,
        isDesktop: false,
        isMacOS: false,
        isWindows: false,
        isLinux: false,
        isWeb: true,
        isTauriDesktop: false,
        isTauriMobile: false
      };

      console.log("[Platform] Detected web environment");
    }
  } catch (error) {
    // Complete failure - default to web as the safest option
    console.error("[Platform] Critical error during platform detection:", error);

    platformInfo = {
      platform: "web",
      isTauri: false,
      isIOS: false,
      isAndroid: false,
      isMobile: false,
      isDesktop: false,
      isMacOS: false,
      isWindows: false,
      isLinux: false,
      isWeb: true,
      isTauriDesktop: false,
      isTauriMobile: false
    };
  }

  // Freeze the platform info to prevent accidental modification
  Object.freeze(platformInfo);
})();

/**
 * Wait for platform detection to complete
 * MUST be called in main.tsx before rendering the app
 *
 * @example
 * ```typescript
 * // In main.tsx
 * import { waitForPlatform } from '@/utils/platform';
 *
 * await waitForPlatform();
 *
 * // Now safe to render - platform is guaranteed to be correct
 * createRoot(document.getElementById("root")!).render(<App />);
 * ```
 */
export async function waitForPlatform(): Promise<void> {
  await platformReady;

  if (!platformInfo) {
    throw new Error("[Platform] Fatal: Platform detection failed completely");
  }
}

/**
 * Get comprehensive platform information
 *
 * @returns The platform information object (never null after initialization)
 * @throws Error if called before platform initialization
 */
export function getPlatformInfo(): PlatformInfo {
  if (!platformInfo) {
    throw new Error(
      "[Platform] Platform not initialized. Ensure waitForPlatform() is called in main.tsx before rendering."
    );
  }
  return platformInfo;
}

/**
 * Check if the app is running in a Tauri environment
 *
 * @returns true if in Tauri, false otherwise
 */
export function isTauri(): boolean {
  if (!platformInfo) {
    throw new Error("[Platform] Platform not initialized");
  }
  return platformInfo.isTauri;
}

/**
 * Check if the platform is iOS
 *
 * @returns true if iOS, false otherwise
 */
export function isIOS(): boolean {
  if (!platformInfo) {
    throw new Error("[Platform] Platform not initialized");
  }
  return platformInfo.isIOS;
}

/**
 * Check if the platform is Android
 *
 * @returns true if Android, false otherwise
 */
export function isAndroid(): boolean {
  if (!platformInfo) {
    throw new Error("[Platform] Platform not initialized");
  }
  return platformInfo.isAndroid;
}

/**
 * Check if the platform is any mobile platform (iOS or Android)
 *
 * @returns true if mobile, false otherwise
 */
export function isMobile(): boolean {
  if (!platformInfo) {
    throw new Error("[Platform] Platform not initialized");
  }
  return platformInfo.isMobile;
}

/**
 * Check if the platform is any desktop platform
 *
 * @returns true if desktop, false otherwise
 */
export function isDesktop(): boolean {
  if (!platformInfo) {
    throw new Error("[Platform] Platform not initialized");
  }
  return platformInfo.isDesktop;
}

/**
 * Check if the platform is macOS
 *
 * @returns true if macOS, false otherwise
 */
export function isMacOS(): boolean {
  if (!platformInfo) {
    throw new Error("[Platform] Platform not initialized");
  }
  return platformInfo.isMacOS;
}

/**
 * Check if the platform is Windows
 *
 * @returns true if Windows, false otherwise
 */
export function isWindows(): boolean {
  if (!platformInfo) {
    throw new Error("[Platform] Platform not initialized");
  }
  return platformInfo.isWindows;
}

/**
 * Check if the platform is Linux
 *
 * @returns true if Linux, false otherwise
 */
export function isLinux(): boolean {
  if (!platformInfo) {
    throw new Error("[Platform] Platform not initialized");
  }
  return platformInfo.isLinux;
}

/**
 * Check if the app is running in a web browser
 *
 * @returns true if web, false otherwise
 */
export function isWeb(): boolean {
  if (!platformInfo) {
    throw new Error("[Platform] Platform not initialized");
  }
  return platformInfo.isWeb;
}

/**
 * Check if the app is running in Tauri desktop
 *
 * @returns true if Tauri desktop, false otherwise
 */
export function isTauriDesktop(): boolean {
  if (!platformInfo) {
    throw new Error("[Platform] Platform not initialized");
  }
  return platformInfo.isTauriDesktop;
}

/**
 * Check if the app is running in Tauri mobile
 *
 * @returns true if Tauri mobile, false otherwise
 */
export function isTauriMobile(): boolean {
  if (!platformInfo) {
    throw new Error("[Platform] Platform not initialized");
  }
  return platformInfo.isTauriMobile;
}
