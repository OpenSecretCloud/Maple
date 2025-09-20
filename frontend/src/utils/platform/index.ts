/**
 * Platform detection utilities and hooks
 *
 * This module provides a unified API for platform detection across the application.
 *
 * @example
 * ```typescript
 * // Import utilities for async functions
 * import { isIOS, isAndroid, isMobile, isDesktop } from '@/utils/platform';
 *
 * // Import hooks for React components
 * import { useIsIOS, useIsMobile, usePlatform } from '@/utils/platform';
 *
 * // Use in async functions
 * if (await isMobile()) {
 *   // Mobile-specific logic
 * }
 *
 * // Use in React components
 * function MyComponent() {
 *   const { isMobile } = useIsMobile();
 *   return isMobile ? <MobileView /> : <DesktopView />;
 * }
 * ```
 */

// Export all utility functions
export {
  getPlatformInfo,
  clearPlatformCache,
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

// Export all React hooks
export {
  usePlatform,
  useIsIOS,
  useIsAndroid,
  useIsMobile,
  useIsDesktop,
  useIsTauri,
  useIsWeb,
  useIsTauriDesktop,
  useIsTauriMobile
} from "@/hooks/usePlatform";
