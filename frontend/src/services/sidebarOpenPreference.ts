const SIDEBAR_OPEN_VERSION = 1 as const;

type SidebarOpenStorage = Pick<Storage, "getItem" | "setItem">;

type StoredSidebarOpen = {
  version: typeof SIDEBAR_OPEN_VERSION;
  isOpen: boolean;
};

function sidebarOpenStorageKey(userId: string): string {
  if (typeof userId !== "string" || !userId.trim()) {
    throw new Error("Sidebar state requires an authenticated user");
  }
  return `maple:sidebar-open:v${SIDEBAR_OPEN_VERSION}:${encodeURIComponent(userId)}`;
}

export function loadSidebarOpenPreference(
  userId: string,
  storage: SidebarOpenStorage | null = browserStorage()
): boolean | null {
  if (!storage) return null;

  try {
    const stored = storage.getItem(sidebarOpenStorageKey(userId));
    if (!stored) return null;
    const parsed: unknown = JSON.parse(stored);
    return isStoredSidebarOpen(parsed) ? parsed.isOpen : null;
  } catch {
    return null;
  }
}

export function saveSidebarOpenPreference(
  userId: string,
  isOpen: boolean,
  storage: SidebarOpenStorage | null = browserStorage()
): boolean {
  if (!storage) return false;

  const value: StoredSidebarOpen = {
    version: SIDEBAR_OPEN_VERSION,
    isOpen
  };

  try {
    storage.setItem(sidebarOpenStorageKey(userId), JSON.stringify(value));
    return true;
  } catch {
    return false;
  }
}

function isStoredSidebarOpen(value: unknown): value is StoredSidebarOpen {
  return (
    value !== null &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    (value as Record<string, unknown>).version === SIDEBAR_OPEN_VERSION &&
    typeof (value as Record<string, unknown>).isOpen === "boolean"
  );
}

function browserStorage(): SidebarOpenStorage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}
