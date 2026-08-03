import { afterEach, describe, expect, test } from "bun:test";
import { act, create, type ReactTestRenderer } from "react-test-renderer";

import {
  CHAT_FONT_FAMILY_STORAGE_KEY,
  CHAT_FONT_SIZE_STORAGE_KEY
} from "@/services/chatTypographyPreferences";
import { ChatTypographyProvider, useChatTypography } from "./ChatTypographyContext";

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

type ChatTypographySnapshot = ReturnType<typeof useChatTypography>;

function ChatTypographyProbe({ onRender }: { onRender: (value: ChatTypographySnapshot) => void }) {
  onRender(useChatTypography());
  return null;
}

describe("ChatTypographyProvider", () => {
  let renderer: ReactTestRenderer | null = null;

  afterEach(() => {
    if (renderer) {
      act(() => renderer?.unmount());
      renderer = null;
    }
  });

  test("restores, persists, applies, and resets typography immediately", () => {
    const storage = new MemoryStorage();
    const style = new MemoryStyle();
    const renderedContext: { current?: ChatTypographySnapshot } = {};
    const currentContext = () => {
      if (!renderedContext.current) throw new Error("Typography context did not render");
      return renderedContext.current;
    };
    storage.setItem(CHAT_FONT_FAMILY_STORAGE_KEY, "serif");
    storage.setItem(CHAT_FONT_SIZE_STORAGE_KEY, "17");

    act(() => {
      renderer = create(
        <ChatTypographyProvider storage={storage} rootStyle={style}>
          <ChatTypographyProbe onRender={(value) => (renderedContext.current = value)} />
        </ChatTypographyProvider>
      );
    });

    expect(currentContext().fontFamily).toBe("serif");
    expect(currentContext().fontSize).toBe(17);
    expect(currentContext().hasCustomTypography).toBe(true);
    expect(style.values.get("--chat-font-family")).toContain("Georgia");
    expect(style.values.get("--chat-font-size")).toBe("17px");

    act(() => currentContext().setFontFamily("system"));
    expect(storage.getItem(CHAT_FONT_FAMILY_STORAGE_KEY)).toBe("system");
    expect(style.values.get("--chat-font-family")).toContain("system-ui");

    act(() => currentContext().setFontSize(100));
    expect(currentContext().fontSize).toBe(18);
    expect(storage.getItem(CHAT_FONT_SIZE_STORAGE_KEY)).toBe("18");
    expect(style.values.get("--chat-font-size")).toBe("18px");

    act(() => currentContext().resetTypography());
    expect(currentContext().fontFamily).toBe("manrope");
    expect(currentContext().fontSize).toBe(15);
    expect(currentContext().hasCustomTypography).toBe(false);
    expect(storage.getItem(CHAT_FONT_FAMILY_STORAGE_KEY)).toBeNull();
    expect(storage.getItem(CHAT_FONT_SIZE_STORAGE_KEY)).toBeNull();
    expect(style.values.get("--chat-font-family")).toContain("Manrope");
    expect(style.values.get("--chat-font-size")).toBe("15px");
  });
});
