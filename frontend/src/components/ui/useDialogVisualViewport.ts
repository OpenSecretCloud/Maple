import { useEffect, useState, useSyncExternalStore } from "react";

import {
  nativeIOSCompactViewportSnapshot,
  NATIVE_IOS_SAFE_AREA_BOUNDARY_ID,
  subscribeToNativeIOSCompactViewport
} from "@/utils/nativeIOSViewport";

const DIALOG_VIEWPORT_MARGIN = 16;

export type DialogVisualViewport = Pick<
  VisualViewport,
  "addEventListener" | "height" | "offsetTop" | "removeEventListener"
>;

export type DialogVisualViewportLayout = {
  centerY: number;
  maxHeight: number;
};

export type DialogSafeAreaInsets = {
  bottom: number;
  top: number;
};

function resolvedInset(value: string) {
  const inset = Number.parseFloat(value);
  return Number.isFinite(inset) ? Math.max(0, inset) : 0;
}

export function readDialogSafeAreaInsets(): DialogSafeAreaInsets {
  if (typeof document === "undefined" || typeof window === "undefined") {
    return { bottom: 0, top: 0 };
  }

  const boundary = document.getElementById(NATIVE_IOS_SAFE_AREA_BOUNDARY_ID);
  if (!boundary) return { bottom: 0, top: 0 };

  const style = window.getComputedStyle(boundary);
  return {
    bottom: resolvedInset(style.bottom),
    top: resolvedInset(style.top)
  };
}

export function getDialogVisualViewportLayout(
  viewport: Pick<VisualViewport, "height" | "offsetTop">,
  safeAreaInsets: DialogSafeAreaInsets = { bottom: 0, top: 0 }
): DialogVisualViewportLayout {
  const safeTop = Math.max(DIALOG_VIEWPORT_MARGIN, safeAreaInsets.top);
  const safeBottom = Math.max(DIALOG_VIEWPORT_MARGIN, safeAreaInsets.bottom);
  const maxHeight = Math.max(0, viewport.height - safeTop - safeBottom);

  return {
    centerY: viewport.offsetTop + safeTop + maxHeight / 2,
    maxHeight
  };
}

export function useDialogVisualViewportLayout(
  enabled: boolean,
  viewport: DialogVisualViewport | null,
  getSafeAreaInsets: () => DialogSafeAreaInsets = readDialogSafeAreaInsets
): DialogVisualViewportLayout | null {
  const [layout, setLayout] = useState<DialogVisualViewportLayout | null>(() =>
    enabled && viewport ? getDialogVisualViewportLayout(viewport, getSafeAreaInsets()) : null
  );

  useEffect(() => {
    if (!enabled || !viewport) {
      setLayout(null);
      return;
    }

    let animationFrame: number | null = null;
    let disposed = false;

    const update = () => {
      if (disposed) return;
      const next = getDialogVisualViewportLayout(viewport, getSafeAreaInsets());
      setLayout((current) =>
        current?.centerY === next.centerY && current.maxHeight === next.maxHeight ? current : next
      );
    };
    const scheduleUpdate = () => {
      if (typeof window === "undefined" || typeof window.requestAnimationFrame !== "function") {
        update();
        return;
      }
      if (animationFrame !== null) return;

      animationFrame = window.requestAnimationFrame(() => {
        animationFrame = null;
        update();
      });
    };

    update();
    viewport.addEventListener("resize", scheduleUpdate);
    viewport.addEventListener("scroll", scheduleUpdate);

    return () => {
      disposed = true;
      viewport.removeEventListener("resize", scheduleUpdate);
      viewport.removeEventListener("scroll", scheduleUpdate);
      if (animationFrame !== null && typeof window !== "undefined") {
        window.cancelAnimationFrame(animationFrame);
      }
    };
  }, [enabled, getSafeAreaInsets, viewport]);

  if (!enabled || !viewport) return null;
  return layout ?? getDialogVisualViewportLayout(viewport, getSafeAreaInsets());
}

export function useNativeDialogVisualViewportLayout() {
  const enabled = useSyncExternalStore(
    subscribeToNativeIOSCompactViewport,
    nativeIOSCompactViewportSnapshot,
    () => false
  );
  const viewport = typeof window === "undefined" ? null : window.visualViewport;

  return useDialogVisualViewportLayout(enabled, viewport);
}
