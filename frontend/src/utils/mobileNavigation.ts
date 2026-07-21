export const MOBILE_NAVIGATION_HISTORY_KEY = "__mapleMobileNavigation";

export type MobileNavigationPage =
  | { type: "menu"; instanceId: number }
  | { type: "new-chat"; instanceId: number; projectId: string | null }
  | {
      type: "chat";
      instanceId: number;
      conversationId: string;
      openedFromNewChat?: true;
    }
  | { type: "project"; instanceId: number; projectId: string };

export type MobileNavigationSnapshot = {
  version: 1;
  stack: MobileNavigationPage[];
  hasInAppParent: boolean;
  historyIndex: number;
};

function menuPage(): MobileNavigationPage {
  return { type: "menu", instanceId: 0 };
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.length > 0;
}

function isPage(value: unknown): value is MobileNavigationPage {
  if (!value || typeof value !== "object") return false;

  const page = value as Record<string, unknown>;
  if (typeof page.instanceId !== "number" || !Number.isFinite(page.instanceId)) return false;

  switch (page.type) {
    case "menu":
      return true;
    case "new-chat":
      return page.projectId === null || isNonEmptyString(page.projectId);
    case "chat":
      return (
        isNonEmptyString(page.conversationId) &&
        (page.openedFromNewChat === undefined || page.openedFromNewChat === true)
      );
    case "project":
      return isNonEmptyString(page.projectId);
    default:
      return false;
  }
}

export function pageFromHref(href: string, instanceId: number): MobileNavigationPage {
  const url = new URL(href, "https://maple.local");
  const conversationId = url.searchParams.get("conversation_id");
  if (conversationId) {
    return { type: "chat", instanceId, conversationId };
  }

  const projectId = url.searchParams.get("project_id");
  if (projectId) {
    return { type: "project", instanceId, projectId };
  }

  return menuPage();
}

export function createInitialMobileNavigation(
  href: string,
  { nativeFreshLaunch = false }: { nativeFreshLaunch?: boolean } = {}
): MobileNavigationSnapshot {
  if (nativeFreshLaunch) {
    return {
      version: 1,
      stack: [menuPage(), { type: "new-chat", instanceId: 1, projectId: null }],
      hasInAppParent: false,
      historyIndex: 0
    };
  }

  const page = pageFromHref(href, 1);
  return {
    version: 1,
    stack: page.type === "menu" ? [page] : [menuPage(), page],
    hasInAppParent: false,
    historyIndex: 0
  };
}

export function createMobileHistoryState(
  snapshot: MobileNavigationSnapshot,
  existingState: unknown = null
): Record<string, unknown> {
  const base =
    existingState && typeof existingState === "object"
      ? { ...(existingState as Record<string, unknown>) }
      : {};

  return {
    ...base,
    [MOBILE_NAVIGATION_HISTORY_KEY]: snapshot
  };
}

export function readMobileHistoryState(state: unknown): MobileNavigationSnapshot | null {
  if (!state || typeof state !== "object") return null;

  const snapshot = (state as Record<string, unknown>)[MOBILE_NAVIGATION_HISTORY_KEY];
  if (!snapshot || typeof snapshot !== "object") return null;

  const candidate = snapshot as Record<string, unknown>;
  if (
    candidate.version !== 1 ||
    typeof candidate.hasInAppParent !== "boolean" ||
    typeof candidate.historyIndex !== "number" ||
    !Number.isFinite(candidate.historyIndex) ||
    !Array.isArray(candidate.stack) ||
    candidate.stack.length === 0 ||
    !candidate.stack.every(isPage) ||
    candidate.stack[0].type !== "menu"
  ) {
    return null;
  }

  return candidate as MobileNavigationSnapshot;
}

export function pushMobilePage(
  snapshot: MobileNavigationSnapshot,
  page: Exclude<MobileNavigationPage, { type: "menu" }>
): MobileNavigationSnapshot {
  return {
    version: 1,
    stack: [...snapshot.stack, page],
    hasInAppParent: true,
    historyIndex: snapshot.historyIndex + 1
  };
}

export function promoteNewChatToConversation(
  snapshot: MobileNavigationSnapshot,
  newChatInstanceId: number,
  conversationId: string
): MobileNavigationSnapshot {
  const activePage = snapshot.stack[snapshot.stack.length - 1];
  if (activePage.type !== "new-chat" || activePage.instanceId !== newChatInstanceId)
    return snapshot;

  return {
    ...snapshot,
    stack: [
      ...snapshot.stack.slice(0, -1),
      {
        type: "chat",
        instanceId: activePage.instanceId,
        conversationId,
        openedFromNewChat: true
      }
    ]
  };
}

export function mobilePageUsesMenuButton(page: MobileNavigationPage) {
  return page.type === "new-chat" || (page.type === "chat" && page.openedFromNewChat === true);
}

export function mobileMenuHistoryDelta(snapshot: MobileNavigationSnapshot) {
  return snapshot.hasInAppParent && snapshot.historyIndex > 0 ? -snapshot.historyIndex : null;
}

export function mobilePageHref(page: MobileNavigationPage): string {
  switch (page.type) {
    case "chat":
      return `/?conversation_id=${encodeURIComponent(page.conversationId)}`;
    case "project":
      return `/?project_id=${encodeURIComponent(page.projectId)}`;
    case "menu":
    case "new-chat":
      return "/";
  }
}

export function activeMobilePage(snapshot: MobileNavigationSnapshot): MobileNavigationPage {
  return snapshot.stack[snapshot.stack.length - 1];
}
