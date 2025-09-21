/**
 * Platform detection utilities
 *
 * This module provides a unified API for reliable, crash-proof platform detection.
 * Platform must be initialized in main.tsx via waitForPlatform() before rendering.
 *
 * @example
 * ```typescript
 * // Import utilities for synchronous platform checks
 * import { isIOS, isAndroid, isMobile, isDesktop } from '@/utils/platform';
 *
 * // Use directly in components - always correct, never wrong
 * function MyComponent() {
 *   if (isMobile()) {
 *     return <MobileView />;
 *   }
 *
 *   if (isTauri()) {
 *     // Safe to use Tauri APIs
 *     const { invoke } = await import("@tauri-apps/api/core");
 *     await invoke("some_command");
 *   }
 *
 *   return <DesktopView />;
 * }
 * ```
 */

// Export all synchronous utility functions
export {
  waitForPlatform,
  getPlatformInfo,
  isTauri,
  isIOS,
  isAndroid,
  isMobile,
  isDesktop,
  isMacOS,
  isWindows,
  isLinux,
  isWeb,
  isTauriDesktop,
  isTauriMobile,
  type PlatformType,
  type PlatformInfo
} from "../platform";
