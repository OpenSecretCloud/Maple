import { describe, expect, test } from "bun:test";

import {
  RESIZABLE_SIDEBAR_DEFAULT_WIDTH_PX,
  clampSidebarWidth,
  loadSidebarWidth,
  saveSidebarWidth,
  sidebarDragUpdate,
  sidebarMaximumWidth
} from "./sidebarWidth";

type SidebarWidthStorage = Pick<Storage, "getItem" | "setItem">;

class MemoryStorage implements SidebarWidthStorage {
  readonly values = new Map<string, string>();

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.values.set(key, value);
  }
}

describe("sidebar width persistence", () => {
  test("uses one versioned width per account", () => {
    const storage = new MemoryStorage();

    expect(saveSidebarWidth("account/a", 360, storage)).toBe(true);
    expect(saveSidebarWidth("account/b", 420, storage)).toBe(true);
    expect([...storage.values.keys()]).toEqual([
      "maple:sidebar-width:v1:account%2Fa",
      "maple:sidebar-width:v1:account%2Fb"
    ]);
    expect(loadSidebarWidth("account/a", storage)).toBe(360);
    expect(loadSidebarWidth("account/b", storage)).toBe(420);
  });

  test("recovers from invalid and unavailable storage", () => {
    const storage = new MemoryStorage();

    expect(loadSidebarWidth("account", storage)).toBe(RESIZABLE_SIDEBAR_DEFAULT_WIDTH_PX);
    saveSidebarWidth("account", 400, storage);
    const key = [...storage.values.keys()][0];
    storage.values.set(key, "not json");
    expect(loadSidebarWidth("account", storage)).toBe(RESIZABLE_SIDEBAR_DEFAULT_WIDTH_PX);
    storage.values.set(key, JSON.stringify({ version: 2, widthPx: 400 }));
    expect(loadSidebarWidth("account", storage)).toBe(RESIZABLE_SIDEBAR_DEFAULT_WIDTH_PX);

    const unavailable: SidebarWidthStorage = {
      getItem: () => {
        throw new Error("unavailable");
      },
      setItem: () => {
        throw new Error("unavailable");
      }
    };
    expect(loadSidebarWidth("account", unavailable)).toBe(RESIZABLE_SIDEBAR_DEFAULT_WIDTH_PX);
    expect(saveSidebarWidth("account", 400, unavailable)).toBe(false);
  });
});

describe("sidebar width constraints", () => {
  test("calculates the responsive maximum and clamps restored widths", () => {
    expect(sidebarMaximumWidth(800)).toBe(320);
    expect(sidebarMaximumWidth(1_200)).toBe(480);
    expect(sidebarMaximumWidth(2_000)).toBe(480);
    expect(clampSidebarWidth(480, sidebarMaximumWidth(800))).toBe(320);
    expect(clampSidebarWidth(200, sidebarMaximumWidth(800))).toBe(240);
  });

  test("holds at the minimum and toggles at the midpoint in both directions", () => {
    expect(sidebarDragUpdate(320, 320, 240, 480)).toEqual({
      widthPx: 240,
      isCollapsed: false
    });
    expect(sidebarDragUpdate(320, 320, 121, 480)).toEqual({
      widthPx: 240,
      isCollapsed: false
    });
    expect(sidebarDragUpdate(320, 320, 120, 480)).toEqual({
      widthPx: 240,
      isCollapsed: true
    });
  });
});
