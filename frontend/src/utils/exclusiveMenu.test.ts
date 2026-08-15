import { describe, expect, test } from "bun:test";
import { applyExclusiveMenuChange, consumeReplacedMenuFocusRestore } from "./exclusiveMenu";

describe("exclusive mobile overflow menus", () => {
  test("suppresses only replaced-menu focus restoration across touch event orderings", () => {
    const closingKeys = new Set<string>();
    const replacedKeys = new Set<string>();

    // Touch can close A before the click that opens B reaches the new trigger.
    expect(
      applyExclusiveMenuChange(closingKeys, replacedKeys, "chat:a", "chat:a", false)
    ).toBeNull();
    expect(applyExclusiveMenuChange(closingKeys, replacedKeys, null, "chat:b", true)).toBe(
      "chat:b"
    );
    // B may finish its own action before A's exit animation unmounts.
    applyExclusiveMenuChange(closingKeys, replacedKeys, "chat:b", "chat:b", false);

    expect(consumeReplacedMenuFocusRestore(closingKeys, replacedKeys, "chat:a")).toBe(true);
    expect(consumeReplacedMenuFocusRestore(closingKeys, replacedKeys, "chat:b")).toBe(false);
  });

  test("also tracks replacement when the new trigger opens before the old close callback", () => {
    const closingKeys = new Set<string>();
    const replacedKeys = new Set<string>();

    expect(applyExclusiveMenuChange(closingKeys, replacedKeys, "chat:a", "chat:b", true)).toBe(
      "chat:b"
    );
    expect(applyExclusiveMenuChange(closingKeys, replacedKeys, "chat:b", "chat:a", false)).toBe(
      "chat:b"
    );

    expect(consumeReplacedMenuFocusRestore(closingKeys, replacedKeys, "chat:a")).toBe(true);
  });

  test("clears a canceled close when the same menu reopens", () => {
    const closingKeys = new Set<string>();
    const replacedKeys = new Set<string>();

    applyExclusiveMenuChange(closingKeys, replacedKeys, "chat:a", "chat:a", false);
    applyExclusiveMenuChange(closingKeys, replacedKeys, null, "chat:a", true);
    applyExclusiveMenuChange(closingKeys, replacedKeys, "chat:a", "chat:a", false);

    expect(consumeReplacedMenuFocusRestore(closingKeys, replacedKeys, "chat:a")).toBe(false);
  });

  test("keeps only the still-closing menu replaced across A to B to A", () => {
    const closingKeys = new Set<string>();
    const replacedKeys = new Set<string>();

    applyExclusiveMenuChange(closingKeys, replacedKeys, "chat:a", "chat:b", true);
    applyExclusiveMenuChange(closingKeys, replacedKeys, "chat:b", "chat:a", false);
    applyExclusiveMenuChange(closingKeys, replacedKeys, "chat:b", "chat:a", true);
    applyExclusiveMenuChange(closingKeys, replacedKeys, "chat:a", "chat:b", false);

    expect(consumeReplacedMenuFocusRestore(closingKeys, replacedKeys, "chat:b")).toBe(true);
    expect(consumeReplacedMenuFocusRestore(closingKeys, replacedKeys, "chat:a")).toBe(false);
  });
});
