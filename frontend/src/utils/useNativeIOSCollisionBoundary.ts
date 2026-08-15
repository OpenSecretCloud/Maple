import { useSyncExternalStore } from "react";
import {
  NATIVE_IOS_COMPACT_VIEWPORT_CLASS,
  NATIVE_IOS_SAFE_AREA_BOUNDARY_ID
} from "@/utils/nativeIOSViewport";
import { isIOS, isTauriMobile } from "@/utils/platform";

type CollisionBoundaryDocument = {
  documentElement: { classList: { contains(token: string): boolean } };
  getElementById(id: string): Element | null;
};

const boundarySubscribers = new Set<() => void>();
let boundaryClassObserver: MutationObserver | null = null;

function notifyBoundarySubscribers() {
  for (const subscriber of boundarySubscribers) subscriber();
}

function subscribeToNativeIOSCollisionBoundary(subscriber: () => void) {
  if (
    typeof window === "undefined" ||
    typeof document === "undefined" ||
    !isTauriMobile() ||
    !isIOS()
  ) {
    return () => {};
  }

  boundarySubscribers.add(subscriber);
  if (boundarySubscribers.size === 1) {
    if (typeof MutationObserver !== "undefined") {
      boundaryClassObserver = new MutationObserver(notifyBoundarySubscribers);
      boundaryClassObserver.observe(document.documentElement, {
        attributes: true,
        attributeFilter: ["class"]
      });
    }
  }

  return () => {
    boundarySubscribers.delete(subscriber);
    if (boundarySubscribers.size > 0) return;

    boundaryClassObserver?.disconnect();
    boundaryClassObserver = null;
  };
}

export function nativeIOSCollisionBoundary(
  collisionDocument: CollisionBoundaryDocument | undefined = typeof document === "undefined"
    ? undefined
    : document
) {
  if (
    !collisionDocument ||
    !collisionDocument.documentElement.classList.contains(NATIVE_IOS_COMPACT_VIEWPORT_CLASS)
  ) {
    return null;
  }

  return collisionDocument.getElementById(NATIVE_IOS_SAFE_AREA_BOUNDARY_ID);
}

export function useNativeIOSCollisionBoundary() {
  return (
    useSyncExternalStore(
      subscribeToNativeIOSCollisionBoundary,
      nativeIOSCollisionBoundary,
      () => null
    ) ?? undefined
  );
}
