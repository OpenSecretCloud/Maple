import { describe, expect, test } from "bun:test";
import {
  getSettingsBackTarget,
  isSettingsRootPath,
  shouldAnimateSettingsPop
} from "./settingsNavigation";

describe("compact settings navigation", () => {
  test("recognizes both settings root path forms", () => {
    expect(isSettingsRootPath("/settings")).toBe(true);
    expect(isSettingsRootPath("/settings/")).toBe(true);
    expect(isSettingsRootPath("/settings/account")).toBe(false);
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
});
