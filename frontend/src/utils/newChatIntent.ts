// Common-core contract for the "quick open a new chat" shortcut.
//
// Any platform's shortcut mechanism (iOS Siri App Intent today; Android App
// Shortcuts / macOS Shortcuts / desktop later) fires the same deep link:
//
//   cloud.opensecret.maple://new-chat?folder=<name>&web_search=on|off&message=<text>&auto_send=on|off&voice=on|off
//
// Every parameter is optional. Omitted parameters fall back to the user's
// persisted defaults (e.g. last-used model and web-search state), so a bare
// `cloud.opensecret.maple://new-chat` simply opens a fresh chat.
//
// The native/platform layer only has to build this URL — all routing and
// behavior lives in the frontend, keeping per-platform code minimal.

export const NEW_CHAT_DEEP_LINK_HOST = "new-chat";

/** sessionStorage key used to hand a parsed intent to the chat view on cold launch. */
export const PENDING_NEW_CHAT_KEY = "pendingNewChat";

/** Window event dispatched to apply an intent while the chat view is already mounted. */
export const NEW_CHAT_INTENT_EVENT = "newchatfromdeeplink";

export interface NewChatIntent {
  /** Folder (conversation project) name; matched case-insensitively, ignored if unknown. */
  folder?: string;
  /** Initial web-search toggle. Omitted => keep the user's persisted default. */
  webSearch?: boolean;
  /** Text to prefill the composer with. Sent immediately only when `autoSend` is set. */
  message?: string;
  /** Auto-submit `message` on open instead of just prefilling it. Requires `message`. */
  autoSend?: boolean;
  /** Start voice recording as soon as the chat opens. Takes precedence over `autoSend`. */
  voice?: boolean;
}

function parseBool(value: string | null): boolean | undefined {
  if (value === null) return undefined;
  const v = value.trim().toLowerCase();
  if (["on", "true", "1", "yes"].includes(v)) return true;
  if (["off", "false", "0", "no"].includes(v)) return false;
  return undefined;
}

/**
 * Parse a deep-link URL into a NewChatIntent, or return null if it is not a
 * new-chat link. Accepts the action as either the URL host
 * (`new-chat://...` style) or the first path segment, since custom-scheme URL
 * parsing can place it in either spot.
 */
export function parseNewChatDeepLink(url: URL): NewChatIntent | null {
  const firstPathPart = url.pathname.split("/").filter(Boolean)[0] || "";
  if (url.host !== NEW_CHAT_DEEP_LINK_HOST && firstPathPart !== NEW_CHAT_DEEP_LINK_HOST) {
    return null;
  }

  return {
    folder: url.searchParams.get("folder")?.trim() || undefined,
    webSearch: parseBool(url.searchParams.get("web_search")),
    message: url.searchParams.get("message") || undefined,
    autoSend: parseBool(url.searchParams.get("auto_send")),
    voice: parseBool(url.searchParams.get("voice"))
  };
}

/** Persist an intent so the chat view can apply it once it mounts (cold launch). */
export function stashNewChatIntent(intent: NewChatIntent): void {
  try {
    sessionStorage.setItem(PENDING_NEW_CHAT_KEY, JSON.stringify(intent));
  } catch (error) {
    console.error("[new-chat] Failed to stash intent:", error);
  }
}

/** Read and clear a pending intent, if one was stashed. */
export function consumeNewChatIntent(): NewChatIntent | null {
  try {
    const raw = sessionStorage.getItem(PENDING_NEW_CHAT_KEY);
    if (!raw) return null;
    sessionStorage.removeItem(PENDING_NEW_CHAT_KEY);
    return JSON.parse(raw) as NewChatIntent;
  } catch (error) {
    console.error("[new-chat] Failed to read pending intent:", error);
    return null;
  }
}
