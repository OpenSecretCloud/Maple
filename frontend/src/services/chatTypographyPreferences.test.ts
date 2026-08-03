import { describe, expect, test } from "bun:test";

import {
  applyChatTypography,
  CHAT_FONT_FAMILIES,
  CHAT_FONT_FAMILY_STORAGE_KEY,
  CHAT_FONT_OPTIONS,
  CHAT_FONT_SIZE_MAX,
  CHAT_FONT_SIZE_MIN,
  CHAT_FONT_SIZE_STEP,
  CHAT_FONT_SIZE_STORAGE_KEY,
  clampChatFontSize,
  DEFAULT_CHAT_FONT_FAMILY,
  DEFAULT_CHAT_FONT_SIZE,
  getStoredChatFontFamily,
  getStoredChatFontSize,
  rememberChatFontFamily,
  rememberChatFontSize,
  resetChatTypographyPreferences,
  restoreChatTypographyAtLaunch
} from "./chatTypographyPreferences";

class MemoryStorage {
  private readonly values = new Map<string, string>();

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.values.set(key, value);
  }

  removeItem(key: string): void {
    this.values.delete(key);
  }
}

class MemoryStyle {
  readonly values = new Map<string, string>();

  setProperty(property: string, value: string): void {
    this.values.set(property, value);
  }
}

describe("chat typography preferences", () => {
  test("defines the restrained product defaults and allowlisted options", () => {
    const storage = new MemoryStorage();

    expect(DEFAULT_CHAT_FONT_FAMILY).toBe("manrope");
    expect(DEFAULT_CHAT_FONT_SIZE).toBe(15);
    expect(CHAT_FONT_SIZE_MIN).toBe(13);
    expect(CHAT_FONT_SIZE_MAX).toBe(18);
    expect(CHAT_FONT_SIZE_STEP).toBe(1);
    expect(CHAT_FONT_FAMILY_STORAGE_KEY).toBe("chatFontFamily");
    expect(CHAT_FONT_SIZE_STORAGE_KEY).toBe("chatFontSize");
    expect(CHAT_FONT_OPTIONS.map(({ value }) => value)).toEqual([...CHAT_FONT_FAMILIES]);
    expect(new Set(CHAT_FONT_OPTIONS.map(({ cssFontFamily }) => cssFontFamily)).size).toBe(
      CHAT_FONT_FAMILIES.length
    );
    expect(getStoredChatFontFamily(storage)).toBe("manrope");
    expect(getStoredChatFontSize(storage)).toBe(15);
  });

  test("persists valid font choices and clamps sizes to whole steps", () => {
    const storage = new MemoryStorage();

    expect(rememberChatFontFamily("system", storage)).toBe("system");
    expect(rememberChatFontSize(16.6, storage)).toBe(17);
    expect(getStoredChatFontFamily(storage)).toBe("system");
    expect(getStoredChatFontSize(storage)).toBe(17);

    expect(clampChatFontSize(4)).toBe(13);
    expect(clampChatFontSize(100)).toBe(18);
    expect(clampChatFontSize(Number.NaN)).toBe(15);
  });

  test("falls back safely from malformed and unavailable storage", () => {
    const storage = new MemoryStorage();
    storage.setItem(CHAT_FONT_FAMILY_STORAGE_KEY, "comic-sans");
    storage.setItem(CHAT_FONT_SIZE_STORAGE_KEY, "huge");

    expect(getStoredChatFontFamily(storage)).toBe(DEFAULT_CHAT_FONT_FAMILY);
    expect(getStoredChatFontSize(storage)).toBe(DEFAULT_CHAT_FONT_SIZE);

    const unavailableStorage = {
      getItem: () => {
        throw new Error("unavailable");
      },
      setItem: () => {
        throw new Error("unavailable");
      },
      removeItem: () => {
        throw new Error("unavailable");
      }
    };

    expect(getStoredChatFontFamily(unavailableStorage)).toBe(DEFAULT_CHAT_FONT_FAMILY);
    expect(getStoredChatFontSize(unavailableStorage)).toBe(DEFAULT_CHAT_FONT_SIZE);
    expect(() => rememberChatFontFamily("system", unavailableStorage)).not.toThrow();
    expect(() => rememberChatFontSize(18, unavailableStorage)).not.toThrow();
    expect(() => resetChatTypographyPreferences(unavailableStorage)).not.toThrow();
  });

  test("removes both keys on reset", () => {
    const storage = new MemoryStorage();
    rememberChatFontFamily("serif", storage);
    rememberChatFontSize(18, storage);

    resetChatTypographyPreferences(storage);

    expect(storage.getItem(CHAT_FONT_FAMILY_STORAGE_KEY)).toBeNull();
    expect(storage.getItem(CHAT_FONT_SIZE_STORAGE_KEY)).toBeNull();
  });

  test("applies safe CSS variables and restores stored values before launch", () => {
    const storage = new MemoryStorage();
    const style = new MemoryStyle();
    rememberChatFontFamily("system", storage);
    rememberChatFontSize(16, storage);

    expect(restoreChatTypographyAtLaunch(storage, style)).toEqual({
      fontFamily: "system",
      fontSize: 16
    });
    expect(style.values.get("--chat-font-family")).toContain("-apple-system");
    expect(style.values.get("--chat-font-size")).toBe("16px");

    expect(
      applyChatTypography(
        { fontFamily: "invalid" as never, fontSize: Number.POSITIVE_INFINITY },
        style
      )
    ).toEqual({ fontFamily: "manrope", fontSize: 15 });
    expect(style.values.get("--chat-font-family")).toContain("Manrope");
    expect(style.values.get("--chat-font-size")).toBe("15px");
  });
});
