import { useSyncExternalStore } from "react";
import {
  nativeIOSCompactViewportSnapshot,
  NATIVE_IOS_SAFE_AREA_BOUNDARY_ID,
  subscribeToNativeIOSCompactViewport
} from "@/utils/nativeIOSViewport";

export function useNativeIOSCollisionBoundary() {
  const active = useSyncExternalStore(
    subscribeToNativeIOSCompactViewport,
    nativeIOSCompactViewportSnapshot,
    () => false
  );

  if (!active || typeof document === "undefined") return undefined;
  return document.getElementById(NATIVE_IOS_SAFE_AREA_BOUNDARY_ID) ?? undefined;
}
