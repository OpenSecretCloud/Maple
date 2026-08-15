export const NATIVE_IOS_COMPACT_VIEWPORT_CLASS = "maple-native-ios-compact-viewport";
export const NATIVE_IOS_SAFE_AREA_BOUNDARY_ID = "maple-native-ios-safe-area-boundary";

export const MOBILE_VIEWPORT_QUERY = "(max-width: 767px)";
export const SHORT_LANDSCAPE_VIEWPORT_QUERY = "(orientation: landscape) and (max-height: 500px)";

type MediaQueryListLike = {
  matches: boolean;
  addEventListener(type: "change", listener: () => void): void;
  removeEventListener(type: "change", listener: () => void): void;
};

type ViewportMetaLike = {
  getAttribute(name: "content"): string | null;
  setAttribute(name: "content", value: string): void;
  removeAttribute(name: "content"): void;
};

type ViewportDocumentLike = {
  documentElement: {
    classList: {
      add(token: string): void;
      remove(token: string): void;
    };
  };
  querySelector(selector: 'meta[name="viewport"]'): ViewportMetaLike | null;
};

type NativeIOSViewportEnvironment = {
  document: ViewportDocumentLike;
  matchMedia(query: string): MediaQueryListLike;
};

const cleanupByDocument = new WeakMap<ViewportDocumentLike, () => void>();
const compactViewportSubscribers = new Set<() => void>();
let nativeIOSViewportEnabled = false;
let nativeIOSCompactViewportActive = false;

function setNativeIOSCompactViewportActive(active: boolean) {
  if (nativeIOSCompactViewportActive === active) return;

  nativeIOSCompactViewportActive = active;
  for (const subscriber of compactViewportSubscribers) subscriber();
}

export function nativeIOSCompactViewportSnapshot() {
  return nativeIOSCompactViewportActive;
}

export function subscribeToNativeIOSCompactViewport(subscriber: () => void) {
  if (!nativeIOSViewportEnabled) return () => {};

  compactViewportSubscribers.add(subscriber);
  return () => compactViewportSubscribers.delete(subscriber);
}

function viewportContentWithCover(content: string) {
  const tokens = content
    .split(",")
    .map((token) => token.trim())
    .filter(Boolean);
  const retainedTokens = tokens.filter((token) => !/^viewport-fit\s*=/i.test(token));

  return [...retainedTokens, "viewport-fit=cover"].join(", ");
}

function restoreViewportContent(meta: ViewportMetaLike, content: string | null) {
  if (content === null) {
    meta.removeAttribute("content");
  } else {
    meta.setAttribute("content", content);
  }
}

export function initializeNativeIOSCompactViewport(
  enabled: boolean,
  environment: NativeIOSViewportEnvironment = {
    document,
    matchMedia: window.matchMedia.bind(window)
  }
) {
  cleanupByDocument.get(environment.document)?.();

  if (!enabled) {
    nativeIOSViewportEnabled = false;
    setNativeIOSCompactViewportActive(false);
    return () => {};
  }

  const viewportMeta = environment.document.querySelector('meta[name="viewport"]');
  if (!viewportMeta) {
    nativeIOSViewportEnabled = false;
    setNativeIOSCompactViewportActive(false);
    return () => {};
  }

  nativeIOSViewportEnabled = true;
  const originalContent = viewportMeta.getAttribute("content");
  const compactWidth = environment.matchMedia(MOBILE_VIEWPORT_QUERY);
  const shortLandscape = environment.matchMedia(SHORT_LANDSCAPE_VIEWPORT_QUERY);
  let disposed = false;

  const update = () => {
    if (disposed) return;

    if (compactWidth.matches || shortLandscape.matches) {
      viewportMeta.setAttribute("content", viewportContentWithCover(originalContent ?? ""));
      environment.document.documentElement.classList.add(NATIVE_IOS_COMPACT_VIEWPORT_CLASS);
      setNativeIOSCompactViewportActive(true);
    } else {
      restoreViewportContent(viewportMeta, originalContent);
      environment.document.documentElement.classList.remove(NATIVE_IOS_COMPACT_VIEWPORT_CLASS);
      setNativeIOSCompactViewportActive(false);
    }
  };

  compactWidth.addEventListener("change", update);
  shortLandscape.addEventListener("change", update);
  update();

  const cleanup = () => {
    if (disposed) return;
    disposed = true;
    compactWidth.removeEventListener("change", update);
    shortLandscape.removeEventListener("change", update);
    restoreViewportContent(viewportMeta, originalContent);
    environment.document.documentElement.classList.remove(NATIVE_IOS_COMPACT_VIEWPORT_CLASS);
    nativeIOSViewportEnabled = false;
    setNativeIOSCompactViewportActive(false);
    if (cleanupByDocument.get(environment.document) === cleanup) {
      cleanupByDocument.delete(environment.document);
    }
  };

  cleanupByDocument.set(environment.document, cleanup);
  return cleanup;
}
