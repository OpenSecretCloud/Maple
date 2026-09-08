import { SIDEBAR_WIDTH_PX } from "@/constants/layout";

const RESIZABLE_SIDEBAR_WIDTH_VERSION = 1 as const;
export const RESIZABLE_SIDEBAR_DEFAULT_WIDTH_PX = SIDEBAR_WIDTH_PX;
export const RESIZABLE_SIDEBAR_MIN_WIDTH_PX = 240;
export const RESIZABLE_SIDEBAR_MAX_WIDTH_PX = 480;
const RESIZABLE_SIDEBAR_COLLAPSE_WIDTH_PX = RESIZABLE_SIDEBAR_MIN_WIDTH_PX / 2;
export const RESIZABLE_SIDEBAR_KEYBOARD_STEP_PX = 8;
export const RESIZABLE_SIDEBAR_KEYBOARD_LARGE_STEP_PX = 32;

type SidebarWidthStorage = Pick<Storage, "getItem" | "setItem">;

type StoredSidebarWidth = {
  version: typeof RESIZABLE_SIDEBAR_WIDTH_VERSION;
  widthPx: number;
};

type SidebarDragUpdate = {
  widthPx: number;
  isCollapsed: boolean;
};

function sidebarWidthStorageKey(userId: string): string {
  if (typeof userId !== "string" || !userId.trim()) {
    throw new Error("Sidebar width requires an authenticated user");
  }
  return `maple:sidebar-width:v${RESIZABLE_SIDEBAR_WIDTH_VERSION}:${encodeURIComponent(userId)}`;
}

export function loadSidebarWidth(
  userId: string,
  storage: SidebarWidthStorage | null = browserStorage()
): number {
  if (!storage) return RESIZABLE_SIDEBAR_DEFAULT_WIDTH_PX;

  let stored: string | null;
  try {
    stored = storage.getItem(sidebarWidthStorageKey(userId));
  } catch {
    return RESIZABLE_SIDEBAR_DEFAULT_WIDTH_PX;
  }
  if (!stored) return RESIZABLE_SIDEBAR_DEFAULT_WIDTH_PX;

  let parsed: unknown;
  try {
    parsed = JSON.parse(stored);
  } catch {
    return RESIZABLE_SIDEBAR_DEFAULT_WIDTH_PX;
  }
  if (
    !isRecord(parsed) ||
    parsed.version !== RESIZABLE_SIDEBAR_WIDTH_VERSION ||
    typeof parsed.widthPx !== "number" ||
    !Number.isFinite(parsed.widthPx)
  ) {
    return RESIZABLE_SIDEBAR_DEFAULT_WIDTH_PX;
  }

  return clampSidebarWidth(parsed.widthPx, RESIZABLE_SIDEBAR_MAX_WIDTH_PX);
}

export function saveSidebarWidth(
  userId: string,
  widthPx: number,
  storage: SidebarWidthStorage | null = browserStorage()
): boolean {
  if (!storage) return false;

  const value: StoredSidebarWidth = {
    version: RESIZABLE_SIDEBAR_WIDTH_VERSION,
    widthPx: clampSidebarWidth(widthPx, RESIZABLE_SIDEBAR_MAX_WIDTH_PX)
  };

  try {
    storage.setItem(sidebarWidthStorageKey(userId), JSON.stringify(value));
    return true;
  } catch {
    return false;
  }
}

export function sidebarMaximumWidth(viewportWidthPx: number): number {
  if (!Number.isFinite(viewportWidthPx) || viewportWidthPx <= 0) {
    return RESIZABLE_SIDEBAR_DEFAULT_WIDTH_PX;
  }
  return Math.max(
    RESIZABLE_SIDEBAR_MIN_WIDTH_PX,
    Math.min(RESIZABLE_SIDEBAR_MAX_WIDTH_PX, Math.floor(viewportWidthPx * 0.4))
  );
}

export function clampSidebarWidth(widthPx: number, maximumWidthPx: number): number {
  const maximum = Number.isFinite(maximumWidthPx)
    ? Math.max(
        RESIZABLE_SIDEBAR_MIN_WIDTH_PX,
        Math.min(RESIZABLE_SIDEBAR_MAX_WIDTH_PX, maximumWidthPx)
      )
    : RESIZABLE_SIDEBAR_DEFAULT_WIDTH_PX;
  const width = Number.isFinite(widthPx) ? widthPx : RESIZABLE_SIDEBAR_DEFAULT_WIDTH_PX;
  return Math.round(Math.max(RESIZABLE_SIDEBAR_MIN_WIDTH_PX, Math.min(maximum, width)));
}

export function sidebarDragUpdate(
  startWidthPx: number,
  startPointerX: number,
  pointerX: number,
  maximumWidthPx: number
): SidebarDragUpdate {
  const rawWidthPx = startWidthPx + (pointerX - startPointerX);
  const hasFiniteWidth = Number.isFinite(rawWidthPx);
  return {
    widthPx: clampSidebarWidth(rawWidthPx, maximumWidthPx),
    isCollapsed: hasFiniteWidth && rawWidthPx <= RESIZABLE_SIDEBAR_COLLAPSE_WIDTH_PX
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function browserStorage(): SidebarWidthStorage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}
