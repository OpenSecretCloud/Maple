import { useIsLandscapeMobile, useIsMobile } from "@/utils/utils";

export function useCompactSettingsLayout() {
  const isMobile = useIsMobile();
  const isLandscapeMobile = useIsLandscapeMobile();

  return isMobile || isLandscapeMobile;
}
