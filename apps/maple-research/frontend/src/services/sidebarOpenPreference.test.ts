import { describe, expect, test } from "bun:test";

import { loadSidebarOpenPreference, saveSidebarOpenPreference } from "./sidebarOpenPreference";

type SidebarOpenStorage = Pick<Storage, "getItem" | "setItem">;

class MemoryStorage implements SidebarOpenStorage {
  readonly values = new Map<string, string>();

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.values.set(key, value);
  }
}

describe("sidebar open preference", () => {
  test("round-trips one open state per account", () => {
    const storage = new MemoryStorage();

    expect(saveSidebarOpenPreference("account/a", false, storage)).toBe(true);
    expect(saveSidebarOpenPreference("account/b", true, storage)).toBe(true);
    expect([...storage.values.keys()]).toEqual([
      "maple:sidebar-open:v1:account%2Fa",
      "maple:sidebar-open:v1:account%2Fb"
    ]);
    expect(loadSidebarOpenPreference("account/a", storage)).toBe(false);
    expect(loadSidebarOpenPreference("account/b", storage)).toBe(true);
  });

  test("recovers from invalid and unavailable storage", () => {
    const storage = new MemoryStorage();

    expect(loadSidebarOpenPreference("account", storage)).toBeNull();
    saveSidebarOpenPreference("account", false, storage);
    const key = [...storage.values.keys()][0];
    storage.values.set(key, "not json");
    expect(loadSidebarOpenPreference("account", storage)).toBeNull();
    storage.values.set(key, JSON.stringify({ version: 2, isOpen: false }));
    expect(loadSidebarOpenPreference("account", storage)).toBeNull();

    const unavailable: SidebarOpenStorage = {
      getItem: () => {
        throw new Error("unavailable");
      },
      setItem: () => {
        throw new Error("unavailable");
      }
    };
    expect(loadSidebarOpenPreference("account", unavailable)).toBeNull();
    expect(saveSidebarOpenPreference("account", false, unavailable)).toBe(false);
  });
});
