/**
 * React hooks for platform detection
 *
 * These hooks provide reactive platform detection for React components,
 * with caching and proper state management.
 */

import { useEffect, useState } from "react";
import { getPlatformInfo, type PlatformInfo } from "@/utils/platform";

/**
 * React hook for getting complete platform information
 *
 * @returns Platform information object with loading state
 *
 * @example
 * ```tsx
 * function MyComponent() {
 *   const { platform, loading } = usePlatform();
 *
 *   if (loading) return <Spinner />;
 *
 *   if (platform.isMobile) {
 *     return <MobileView />;
 *   }
 *
 *   return <DesktopView />;
 * }
 * ```
 */
export function usePlatform() {
  const [platform, setPlatform] = useState<PlatformInfo | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let mounted = true;

    getPlatformInfo()
      .then((info) => {
        if (mounted) {
          setPlatform(info);
          setLoading(false);
        }
      })
      .catch((error) => {
        console.error("Failed to get platform info:", error);
        if (mounted) {
          // Set default web platform on error
          setPlatform({
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
          });
          setLoading(false);
        }
      });

    return () => {
      mounted = false;
    };
  }, []);

  return { platform, loading };
}

/**
 * Hook for checking if the platform is iOS
 *
 * @returns Object with isIOS boolean and loading state
 *
 * @example
 * ```tsx
 * function MyComponent() {
 *   const { isIOS, loading } = useIsIOS();
 *
 *   if (isIOS) {
 *     return <IOSSpecificComponent />;
 *   }
 *   return <DefaultComponent />;
 * }
 * ```
 */
export function useIsIOS() {
  const { platform, loading } = usePlatform();
  return { isIOS: platform?.isIOS ?? false, loading };
}

/**
 * Hook for checking if the platform is Android
 *
 * @returns Object with isAndroid boolean and loading state
 *
 * @example
 * ```tsx
 * function MyComponent() {
 *   const { isAndroid, loading } = useIsAndroid();
 *
 *   if (isAndroid) {
 *     return <AndroidSpecificComponent />;
 *   }
 *   return <DefaultComponent />;
 * }
 * ```
 */
export function useIsAndroid() {
  const { platform, loading } = usePlatform();
  return { isAndroid: platform?.isAndroid ?? false, loading };
}

/**
 * Hook for checking if the platform is any mobile platform
 *
 * @returns Object with isMobile boolean and loading state
 *
 * @example
 * ```tsx
 * function MyComponent() {
 *   const { isMobile, loading } = useIsMobile();
 *
 *   return (
 *     <div className={isMobile ? "mobile-layout" : "desktop-layout"}>
 *       {content}
 *     </div>
 *   );
 * }
 * ```
 */
export function useIsMobile() {
  const { platform, loading } = usePlatform();
  return { isMobile: platform?.isMobile ?? false, loading };
}

/**
 * Hook for checking if the platform is any desktop platform
 *
 * @returns Object with isDesktop boolean and loading state
 *
 * @example
 * ```tsx
 * function MyComponent() {
 *   const { isDesktop } = useIsDesktop();
 *
 *   if (isDesktop) {
 *     // Show desktop-specific features like proxy configuration
 *     return <ProxySettings />;
 *   }
 *   return null;
 * }
 * ```
 */
export function useIsDesktop() {
  const { platform, loading } = usePlatform();
  return { isDesktop: platform?.isDesktop ?? false, loading };
}

/**
 * Hook for checking if the app is running in Tauri
 *
 * @returns Object with isTauri boolean and loading state
 *
 * @example
 * ```tsx
 * function MyComponent() {
 *   const { isTauri } = useIsTauri();
 *
 *   if (isTauri) {
 *     // Use native features
 *     return <NativeFeatures />;
 *   }
 *   return <WebFeatures />;
 * }
 * ```
 */
export function useIsTauri() {
  const { platform, loading } = usePlatform();
  return { isTauri: platform?.isTauri ?? false, loading };
}

/**
 * Hook for checking if the app is running in a web browser
 *
 * @returns Object with isWeb boolean and loading state
 *
 * @example
 * ```tsx
 * function MyComponent() {
 *   const { isWeb } = useIsWeb();
 *
 *   if (isWeb) {
 *     // Show web-specific UI
 *     return <WebOnlyFeature />;
 *   }
 *   return null;
 * }
 * ```
 */
export function useIsWeb() {
  const { platform, loading } = usePlatform();
  return { isWeb: platform?.isWeb ?? false, loading };
}

/**
 * Hook for checking if the app is running in Tauri desktop
 *
 * @returns Object with isTauriDesktop boolean and loading state
 *
 * @example
 * ```tsx
 * function ProxyConfig() {
 *   const { isTauriDesktop } = useIsTauriDesktop();
 *
 *   // Only show proxy config on Tauri desktop
 *   if (!isTauriDesktop) return null;
 *
 *   return <ProxySettings />;
 * }
 * ```
 */
export function useIsTauriDesktop() {
  const { platform, loading } = usePlatform();
  return {
    isTauriDesktop: platform ? platform.isTauri && platform.isDesktop : false,
    loading
  };
}

/**
 * Hook for checking if the app is running in Tauri mobile
 *
 * @returns Object with isTauriMobile boolean and loading state
 *
 * @example
 * ```tsx
 * function MobileApp() {
 *   const { isTauriMobile } = useIsTauriMobile();
 *
 *   if (isTauriMobile) {
 *     // Mobile app-specific features
 *     return <MobileAppFeatures />;
 *   }
 *   return <WebMobileFeatures />;
 * }
 * ```
 */
export function useIsTauriMobile() {
  const { platform, loading } = usePlatform();
  return {
    isTauriMobile: platform ? platform.isTauri && platform.isMobile : false,
    loading
  };
}
