export const CHAT_FONT_FAMILIES = ["manrope", "system", "serif"] as const;

export type ChatFontFamily = (typeof CHAT_FONT_FAMILIES)[number];

export interface ChatFontOption {
  value: ChatFontFamily;
  label: string;
  description: string;
  cssFontFamily: string;
}

export interface ChatTypographyPreferences {
  fontFamily: ChatFontFamily;
  fontSize: number;
}

export type ChatTypographyStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

export type ChatTypographyStyle = Pick<CSSStyleDeclaration, "setProperty">;

export const CHAT_FONT_OPTIONS: readonly ChatFontOption[] = [
  {
    value: "manrope",
    label: "Manrope",
    description: "Maple's default, with a compact modern feel.",
    cssFontFamily: '"Manrope", sans-serif'
  },
  {
    value: "system",
    label: "System",
    description: "The familiar interface font for your device.",
    cssFontFamily: 'system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
  },
  {
    value: "serif",
    label: "Serif",
    description: "A traditional reading style with distinct letterforms.",
    cssFontFamily: 'Georgia, "Times New Roman", serif'
  }
];

export const DEFAULT_CHAT_FONT_FAMILY: ChatFontFamily = "manrope";
export const DEFAULT_CHAT_FONT_SIZE = 15;
export const CHAT_FONT_SIZE_MIN = 13;
export const CHAT_FONT_SIZE_MAX = 18;
export const CHAT_FONT_SIZE_STEP = 1;

export const CHAT_FONT_FAMILY_STORAGE_KEY = "chatFontFamily";
export const CHAT_FONT_SIZE_STORAGE_KEY = "chatFontSize";

const CHAT_FONT_OPTION_BY_FAMILY = new Map(
  CHAT_FONT_OPTIONS.map((option) => [option.value, option] as const)
);

function getBrowserStorage(): ChatTypographyStorage | null {
  if (typeof window === "undefined") return null;

  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

function getDocumentRootStyle(): ChatTypographyStyle | null {
  if (typeof document === "undefined") return null;

  try {
    return document.documentElement.style;
  } catch {
    return null;
  }
}

export function isChatFontFamily(value: unknown): value is ChatFontFamily {
  return typeof value === "string" && CHAT_FONT_OPTION_BY_FAMILY.has(value as ChatFontFamily);
}

export function getChatFontOption(fontFamily: ChatFontFamily): ChatFontOption {
  return (
    CHAT_FONT_OPTION_BY_FAMILY.get(fontFamily) ??
    CHAT_FONT_OPTION_BY_FAMILY.get(DEFAULT_CHAT_FONT_FAMILY)!
  );
}

export function clampChatFontSize(fontSize: number): number {
  if (!Number.isFinite(fontSize)) {
    return DEFAULT_CHAT_FONT_SIZE;
  }

  const clamped = Math.min(CHAT_FONT_SIZE_MAX, Math.max(CHAT_FONT_SIZE_MIN, fontSize));
  return (
    Math.round((clamped - CHAT_FONT_SIZE_MIN) / CHAT_FONT_SIZE_STEP) * CHAT_FONT_SIZE_STEP +
    CHAT_FONT_SIZE_MIN
  );
}

export function getStoredChatFontFamily(
  storage: ChatTypographyStorage | null = getBrowserStorage()
): ChatFontFamily {
  if (!storage) return DEFAULT_CHAT_FONT_FAMILY;

  try {
    const stored = storage.getItem(CHAT_FONT_FAMILY_STORAGE_KEY);
    return isChatFontFamily(stored) ? stored : DEFAULT_CHAT_FONT_FAMILY;
  } catch {
    return DEFAULT_CHAT_FONT_FAMILY;
  }
}

export function getStoredChatFontSize(
  storage: ChatTypographyStorage | null = getBrowserStorage()
): number {
  if (!storage) return DEFAULT_CHAT_FONT_SIZE;

  try {
    const stored = storage.getItem(CHAT_FONT_SIZE_STORAGE_KEY);
    if (stored === null || stored.trim() === "") {
      return DEFAULT_CHAT_FONT_SIZE;
    }
    return clampChatFontSize(Number(stored));
  } catch {
    return DEFAULT_CHAT_FONT_SIZE;
  }
}

export function rememberChatFontFamily(
  fontFamily: ChatFontFamily,
  storage: ChatTypographyStorage | null = getBrowserStorage()
): ChatFontFamily {
  const safeFontFamily = isChatFontFamily(fontFamily) ? fontFamily : DEFAULT_CHAT_FONT_FAMILY;
  if (!storage) return safeFontFamily;

  try {
    storage.setItem(CHAT_FONT_FAMILY_STORAGE_KEY, safeFontFamily);
  } catch {
    // Storage failures should not prevent an in-memory preference change.
  }
  return safeFontFamily;
}

export function rememberChatFontSize(
  fontSize: number,
  storage: ChatTypographyStorage | null = getBrowserStorage()
): number {
  const safeFontSize = clampChatFontSize(fontSize);
  if (!storage) return safeFontSize;

  try {
    storage.setItem(CHAT_FONT_SIZE_STORAGE_KEY, String(safeFontSize));
  } catch {
    // Storage failures should not prevent an in-memory preference change.
  }
  return safeFontSize;
}

export function resetChatTypographyPreferences(
  storage: ChatTypographyStorage | null = getBrowserStorage()
): void {
  if (!storage) return;

  try {
    storage.removeItem(CHAT_FONT_FAMILY_STORAGE_KEY);
  } catch {
    // Continue so one unavailable key does not block the other reset.
  }

  try {
    storage.removeItem(CHAT_FONT_SIZE_STORAGE_KEY);
  } catch {
    // Storage failures should not prevent an in-memory preference reset.
  }
}

export function applyChatTypography(
  preferences: ChatTypographyPreferences,
  style: ChatTypographyStyle | null = getDocumentRootStyle()
): ChatTypographyPreferences {
  const safePreferences: ChatTypographyPreferences = {
    fontFamily: isChatFontFamily(preferences.fontFamily)
      ? preferences.fontFamily
      : DEFAULT_CHAT_FONT_FAMILY,
    fontSize: clampChatFontSize(preferences.fontSize)
  };
  if (!style) return safePreferences;

  try {
    style.setProperty(
      "--chat-font-family",
      getChatFontOption(safePreferences.fontFamily).cssFontFamily
    );
    style.setProperty("--chat-font-size", `${safePreferences.fontSize}px`);
  } catch {
    // CSS application failures should not prevent the app from launching.
  }

  return safePreferences;
}

export function restoreChatTypographyAtLaunch(
  storage: ChatTypographyStorage | null = getBrowserStorage(),
  style: ChatTypographyStyle | null = getDocumentRootStyle()
): ChatTypographyPreferences {
  return applyChatTypography(
    {
      fontFamily: getStoredChatFontFamily(storage),
      fontSize: getStoredChatFontSize(storage)
    },
    style
  );
}
