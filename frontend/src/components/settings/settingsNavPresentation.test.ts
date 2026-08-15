import { describe, expect, test } from "bun:test";
import { getSettingsNavLinkPresentation } from "./settingsNavPresentation";

describe("Settings navigation row presentation", () => {
  test("uses one semantic selected color without touch-sticky hover in compact Settings", () => {
    const presentation = getSettingsNavLinkPresentation(true);

    expect(presentation.activeClassName).toContain("--sidebar-row-selected");
    expect(presentation.activeClassName).not.toContain("dark:");
    expect(presentation.inactiveClassName).not.toContain("hover:");
    expect(presentation.containerClassName).toContain("-mx-2");
    expect(presentation.containerClassName).toContain("px-2");
  });

  test("preserves desktop Settings selection, hover, and row sizing", () => {
    const presentation = getSettingsNavLinkPresentation(false);

    expect(presentation.activeClassName).toContain("--sidebar-chrome");
    expect(presentation.activeClassName).toContain("dark:bg");
    expect(presentation.inactiveClassName).toContain("hover:bg-background/70");
    expect(presentation.containerClassName).toBe("gap-3 rounded-lg px-3 py-2 text-sm");
  });
});
