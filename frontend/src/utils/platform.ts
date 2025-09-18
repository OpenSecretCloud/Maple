/**
 * Platform detection utilities for Tauri applications
 *
 * This module provides consistent platform detection across the application,
 * handling both Tauri native environments and web browsers.
 *
 * Uses dynamic imports to safely handle environments where Tauri APIs may not be available.
 */

/**
 * Platform types supported by the application
 */
export type PlatformType = "ios" | "android" | "macos" | "windows" | "linux" | "web";

/**
 * Platform information object returned by detection functions
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
}

/**
 * Cache for platform info to avoid repeated async calls
 */
let platformInfoCache: PlatformInfo | null = null;

/**
 * Gets comprehensive platform information
 *
 * @returns Promise resolving to platform information object
 *
 * @example
 * ```typescript
 * const platform = await getPlatformInfo();
 * if (platform.isMobile) {
 *   // Mobile-specific logic (iOS or Android)
 * }
 * if (platform.isIOS) {
 *   // iOS-specific logic
 * }
 * ```
 */
export async function getPlatformInfo(): Promise<PlatformInfo> {
  // Return cached value if available
  if (platformInfoCache) {
    return platformInfoCache;
  }

  try {
    // Safely check if we're in a Tauri environment
    const tauriEnv = await import("@tauri-apps/api/core")
      .then((m) => m.isTauri())
      .catch(() => false);

    if (tauriEnv) {
      try {
        // Safely get the platform type
        const { type } = await import("@tauri-apps/plugin-os");
        const platformType = await type();

        const info: PlatformInfo = {
          platform: platformType as PlatformType,
          isTauri: true,
          isIOS: platformType === "ios",
          isAndroid: platformType === "android",
          isMobile: platformType === "ios" || platformType === "android",
          isDesktop:
            platformType === "macos" || platformType === "windows" || platformType === "linux",
          isMacOS: platformType === "macos",
          isWindows: platformType === "windows",
          isLinux: platformType === "linux",
          isWeb: false
        };

        platformInfoCache = info;
        return info;
      } catch (error) {
        // If we can't get platform type, but we're in Tauri, assume desktop
        console.warn("Platform detection: Could not determine platform type", error);

        // Default to a generic desktop Tauri environment
        const info: PlatformInfo = {
          platform: "linux" as PlatformType, // Safe fallback
          isTauri: true,
          isIOS: false,
          isAndroid: false,
          isMobile: false,
          isDesktop: true,
          isMacOS: false,
          isWindows: false,
          isLinux: true,
          isWeb: false
        };

        platformInfoCache = info;
        return info;
      }
    }
  } catch (error) {
    // If Tauri APIs are not available, we're in a web environment
    console.debug("Platform detection: Not in Tauri environment", error);
  }

  // Web environment (or complete failure to detect)
  const info: PlatformInfo = {
    platform: "web",
    isTauri: false,
    isIOS: false,
    isAndroid: false,
    isMobile: false,
    isDesktop: false,
    isMacOS: false,
    isWindows: false,
    isLinux: false,
    isWeb: true
  };

  platformInfoCache = info;
  return info;
}

/**
 * Clears the platform info cache
 * Useful for testing or when platform might have changed
 */
export function clearPlatformCache(): void {
  platformInfoCache = null;
}

/**
 * Checks if the app is running in a Tauri environment
 *
 * @returns Promise resolving to true if in Tauri, false otherwise
 *
 * @example
 * ```typescript
 * if (await isTauri()) {
 *   // Tauri-specific logic
 * }
 * ```
 */
export async function isTauri(): Promise<boolean> {
  const info = await getPlatformInfo();
  return info.isTauri;
}

/**
 * Checks if the platform is iOS
 *
 * @returns Promise resolving to true if iOS, false otherwise
 *
 * @example
 * ```typescript
 * if (await isIOS()) {
 *   // iOS-specific logic
 * }
 * ```
 */
export async function isIOS(): Promise<boolean> {
  const info = await getPlatformInfo();
  return info.isIOS;
}

/**
 * Checks if the platform is Android
 *
 * @returns Promise resolving to true if Android, false otherwise
 *
 * @example
 * ```typescript
 * if (await isAndroid()) {
 *   // Android-specific logic
 * }
 * ```
 */
export async function isAndroid(): Promise<boolean> {
  const info = await getPlatformInfo();
  return info.isAndroid;
}

/**
 * Checks if the platform is any mobile platform (iOS or Android)
 *
 * @returns Promise resolving to true if mobile, false otherwise
 *
 * @example
 * ```typescript
 * if (await isMobile()) {
 *   // Mobile-specific logic (iOS or Android)
 * }
 * ```
 */
export async function isMobile(): Promise<boolean> {
  const info = await getPlatformInfo();
  return info.isMobile;
}

/**
 * Checks if the platform is any desktop platform (macOS, Windows, Linux)
 *
 * @returns Promise resolving to true if desktop, false otherwise
 *
 * @example
 * ```typescript
 * if (await isDesktop()) {
 *   // Desktop-specific logic
 * }
 * ```
 */
export async function isDesktop(): Promise<boolean> {
  const info = await getPlatformInfo();
  return info.isDesktop;
}

/**
 * Checks if the platform is macOS
 *
 * @returns Promise resolving to true if macOS, false otherwise
 *
 * @example
 * ```typescript
 * if (await isMacOS()) {
 *   // macOS-specific logic
 * }
 * ```
 */
export async function isMacOS(): Promise<boolean> {
  const info = await getPlatformInfo();
  return info.isMacOS;
}

/**
 * Checks if the platform is Windows
 *
 * @returns Promise resolving to true if Windows, false otherwise
 *
 * @example
 * ```typescript
 * if (await isWindows()) {
 *   // Windows-specific logic
 * }
 * ```
 */
export async function isWindows(): Promise<boolean> {
  const info = await getPlatformInfo();
  return info.isWindows;
}

/**
 * Checks if the platform is Linux
 *
 * @returns Promise resolving to true if Linux, false otherwise
 *
 * @example
 * ```typescript
 * if (await isLinux()) {
 *   // Linux-specific logic
 * }
 * ```
 */
export async function isLinux(): Promise<boolean> {
  const info = await getPlatformInfo();
  return info.isLinux;
}

/**
 * Checks if the app is running in a web browser (not Tauri)
 *
 * @returns Promise resolving to true if web, false otherwise
 *
 * @example
 * ```typescript
 * if (await isWeb()) {
 *   // Web-specific logic
 * }
 * ```
 */
export async function isWeb(): Promise<boolean> {
  const info = await getPlatformInfo();
  return info.isWeb;
}

/**
 * Checks if the platform is a Tauri desktop environment
 * Useful for determining if desktop-specific features like proxy should be enabled
 *
 * @returns Promise resolving to true if Tauri desktop, false otherwise
 *
 * @example
 * ```typescript
 * if (await isTauriDesktop()) {
 *   // Enable proxy configuration
 * }
 * ```
 */
export async function isTauriDesktop(): Promise<boolean> {
  const info = await getPlatformInfo();
  return info.isTauri && info.isDesktop;
}

/**
 * Checks if the platform is a Tauri mobile environment
 *
 * @returns Promise resolving to true if Tauri mobile, false otherwise
 *
 * @example
 * ```typescript
 * if (await isTauriMobile()) {
 *   // Mobile app-specific logic
 * }
 * ```
 */
export async function isTauriMobile(): Promise<boolean> {
  const info = await getPlatformInfo();
  return info.isTauri && info.isMobile;
}
