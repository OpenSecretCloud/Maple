import { describe, expect, test } from "bun:test";
import {
  consumeReplacedMenuFocusRestore,
  conversationMenuKey,
  nextExclusiveMenuKey,
  projectMenuKey,
  trackExclusiveMenuFocusChange
} from "./exclusiveMenu";

describe("exclusive mobile overflow menus", () => {
  test("keeps only the most recently opened menu", () => {
    expect(nextExclusiveMenuKey(null, "chat:a", true)).toBe("chat:a");
    expect(nextExclusiveMenuKey("chat:a", "project:b", true)).toBe("project:b");
  });

  test("ignores a stale close from the menu that was replaced", () => {
    expect(nextExclusiveMenuKey("project:b", "chat:a", false)).toBe("project:b");
    expect(nextExclusiveMenuKey("project:b", "project:b", false)).toBeNull();
  });

  test("uses conversation IDs rather than display titles for menu identity", () => {
    expect(conversationMenuKey("conversation-a")).toBe("chat:conversation-a");
    expect(conversationMenuKey("conversation-a")).not.toBe(conversationMenuKey("conversation-b"));
    expect(projectMenuKey("project-a")).toBe("project-menu:project-a");
  });

  test("suppresses only replaced-menu focus restoration across touch event orderings", () => {
    const closingKeys = new Set<string>();
    const replacedKeys = new Set<string>();

    // Touch can close A before the click that opens B reaches the new trigger.
    trackExclusiveMenuFocusChange(closingKeys, replacedKeys, "chat:a", "chat:a", false);
    trackExclusiveMenuFocusChange(closingKeys, replacedKeys, null, "chat:b", true);
    // B may finish its own action before A's exit animation unmounts.
    trackExclusiveMenuFocusChange(closingKeys, replacedKeys, "chat:b", "chat:b", false);

    expect(consumeReplacedMenuFocusRestore(closingKeys, replacedKeys, "chat:a")).toBe(true);
    expect(consumeReplacedMenuFocusRestore(closingKeys, replacedKeys, "chat:b")).toBe(false);
  });

  test("also tracks replacement when the new trigger opens before the old close callback", () => {
    const closingKeys = new Set<string>();
    const replacedKeys = new Set<string>();

    trackExclusiveMenuFocusChange(closingKeys, replacedKeys, "chat:a", "chat:b", true);
    trackExclusiveMenuFocusChange(closingKeys, replacedKeys, "chat:b", "chat:a", false);

    expect(consumeReplacedMenuFocusRestore(closingKeys, replacedKeys, "chat:a")).toBe(true);
  });

  test("clears a canceled close when the same menu reopens", () => {
    const closingKeys = new Set<string>();
    const replacedKeys = new Set<string>();

    trackExclusiveMenuFocusChange(closingKeys, replacedKeys, "chat:a", "chat:a", false);
    trackExclusiveMenuFocusChange(closingKeys, replacedKeys, null, "chat:a", true);
    trackExclusiveMenuFocusChange(closingKeys, replacedKeys, "chat:a", "chat:a", false);

    expect(consumeReplacedMenuFocusRestore(closingKeys, replacedKeys, "chat:a")).toBe(false);
  });

  test("keeps only the still-closing menu replaced across A to B to A", () => {
    const closingKeys = new Set<string>();
    const replacedKeys = new Set<string>();

    trackExclusiveMenuFocusChange(closingKeys, replacedKeys, "chat:a", "chat:b", true);
    trackExclusiveMenuFocusChange(closingKeys, replacedKeys, "chat:b", "chat:a", false);
    trackExclusiveMenuFocusChange(closingKeys, replacedKeys, "chat:b", "chat:a", true);
    trackExclusiveMenuFocusChange(closingKeys, replacedKeys, "chat:a", "chat:b", false);

    expect(consumeReplacedMenuFocusRestore(closingKeys, replacedKeys, "chat:b")).toBe(true);
    expect(consumeReplacedMenuFocusRestore(closingKeys, replacedKeys, "chat:a")).toBe(false);
  });
});
