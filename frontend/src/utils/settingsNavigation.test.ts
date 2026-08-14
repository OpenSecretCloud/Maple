import { describe, expect, test } from "bun:test";
import {
  closeCompactSettings,
  getSettingsBackTarget,
  hasSettingsHomeParent,
  isSettingsPath,
  isSettingsRootPath,
  SETTINGS_HOME_PARENT_STATE_KEY,
  settingsMenuOwnsDocumentCanvas,
  shouldAnimateSettingsPop,
  shouldSuspendCoveredHomeChats
} from "./settingsNavigation";

describe("compact settings navigation", () => {
  test("recognizes both settings root path forms", () => {
    expect(isSettingsRootPath("/settings")).toBe(true);
    expect(isSettingsRootPath("/settings/")).toBe(true);
    expect(isSettingsRootPath("/settings/account")).toBe(false);
  });

  test("recognizes the complete settings route family", () => {
    expect(isSettingsPath("/settings")).toBe(true);
    expect(isSettingsPath("/settings/api/keys")).toBe(true);
    expect(isSettingsPath("/settings-and-more")).toBe(false);
  });

  test("recognizes settings entries pushed from the mounted home surface", () => {
    expect(hasSettingsHomeParent({ [SETTINGS_HOME_PARENT_STATE_KEY]: true })).toBe(true);
    expect(hasSettingsHomeParent({ [SETTINGS_HOME_PARENT_STATE_KEY]: false })).toBe(false);
    expect(hasSettingsHomeParent(null)).toBe(false);
  });

  test("animates before returning to the marked home entry", async () => {
    const operations: string[] = [];

    await closeCompactSettings({
      interactive: false,
      hasHomeParent: true,
      animate: async () => {
        operations.push("animate");
      },
      canCommit: () => true,
      goBack: () => {
        operations.push("back");
      },
      replaceWithMenu: () => {
        operations.push("replace-menu");
      }
    });

    expect(operations).toEqual(["animate", "back"]);
  });

  test("does not replay the shell animation after an interactive swipe", async () => {
    const operations: string[] = [];

    await closeCompactSettings({
      interactive: true,
      hasHomeParent: true,
      animate: async () => {
        operations.push("animate");
      },
      canCommit: () => true,
      goBack: () => {
        operations.push("back");
      },
      replaceWithMenu: () => {
        operations.push("replace-menu");
      }
    });

    expect(operations).toEqual(["back"]);
  });

  test("replaces a markerless or direct Settings entry with the menu", async () => {
    const operations: string[] = [];

    await closeCompactSettings({
      interactive: false,
      hasHomeParent: false,
      animate: async () => {
        operations.push("animate");
      },
      canCommit: () => true,
      goBack: () => {
        operations.push("back");
      },
      replaceWithMenu: async () => {
        operations.push("replace-menu");
      }
    });

    expect(operations).toEqual(["animate", "replace-menu"]);
  });

  test("does not commit a deferred close after history changes", async () => {
    const operations: string[] = [];

    const committed = await closeCompactSettings({
      interactive: false,
      hasHomeParent: true,
      animate: async () => {
        operations.push("animate");
      },
      canCommit: () => false,
      goBack: () => {
        operations.push("back");
      },
      replaceWithMenu: () => {
        operations.push("replace-menu");
      }
    });

    expect(committed).toBe(false);
    expect(operations).toEqual(["animate"]);
  });

  test("a markerless interactive close goes straight to the menu", async () => {
    const operations: string[] = [];

    await closeCompactSettings({
      interactive: true,
      hasHomeParent: false,
      animate: async () => {
        operations.push("animate");
      },
      canCommit: () => true,
      goBack: () => {
        operations.push("back");
      },
      replaceWithMenu: () => {
        operations.push("replace-menu");
      }
    });

    expect(operations).toEqual(["replace-menu"]);
  });

  test("returns to the recorded menu entry across nested detail history", () => {
    expect(getSettingsBackTarget(14, 11)).toEqual({ type: "history", delta: -3 });
  });

  test("uses a root replacement for a directly loaded detail", () => {
    expect(getSettingsBackTarget(14, null)).toEqual({ type: "root" });
    expect(getSettingsBackTarget(undefined, 11)).toEqual({ type: "root" });
  });

  test("animates only compact backward navigation from detail to the menu", () => {
    expect(
      shouldAnimateSettingsPop({
        compact: true,
        currentPathname: "/settings/api/keys",
        nextPathname: "/settings",
        action: "GO"
      })
    ).toBe(true);
    expect(
      shouldAnimateSettingsPop({
        compact: true,
        currentPathname: "/settings/api/keys",
        nextPathname: "/settings/api",
        action: "BACK"
      })
    ).toBe(false);
    expect(
      shouldAnimateSettingsPop({
        compact: false,
        currentPathname: "/settings/account",
        nextPathname: "/settings",
        action: "BACK"
      })
    ).toBe(false);
  });

  test("unmounts a covered chat only after Settings has fully entered", () => {
    expect(
      shouldSuspendCoveredHomeChats({
        isSettingsRoute: true,
        hasSettingsShellEntered: true,
        isSettingsPopping: false,
        isSettingsShellSwipeActive: false
      })
    ).toBe(true);

    for (const override of [
      { isSettingsRoute: false },
      { hasSettingsShellEntered: false },
      { isSettingsPopping: true },
      { isSettingsShellSwipeActive: true }
    ]) {
      expect(
        shouldSuspendCoveredHomeChats({
          isSettingsRoute: true,
          hasSettingsShellEntered: true,
          isSettingsPopping: false,
          isSettingsShellSwipeActive: false,
          ...override
        })
      ).toBe(false);
    }
  });

  test("keeps the source Settings canvas through compact page transitions", () => {
    expect(
      settingsMenuOwnsDocumentCanvas({
        compact: true,
        isSettingsRoot: true,
        isSettingsDetailEntering: false
      })
    ).toBe(true);
    expect(
      settingsMenuOwnsDocumentCanvas({
        compact: true,
        isSettingsRoot: false,
        isSettingsDetailEntering: true
      })
    ).toBe(true);
    expect(
      settingsMenuOwnsDocumentCanvas({
        compact: true,
        isSettingsRoot: false,
        isSettingsDetailEntering: false
      })
    ).toBe(false);
    expect(
      settingsMenuOwnsDocumentCanvas({
        compact: false,
        isSettingsRoot: true,
        isSettingsDetailEntering: false
      })
    ).toBe(false);
  });
});
