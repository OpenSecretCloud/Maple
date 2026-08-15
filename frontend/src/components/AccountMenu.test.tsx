import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { getAccountMenuPresentation } from "./accountMenuPresentation";

describe("AccountMenu", () => {
  test("uses the shared 44px Settings control and root route in a compact page header", () => {
    expect(
      getAccountMenuPresentation({ compactSettingsLayout: true, pagePresentation: true })
    ).toEqual({
      settingsPath: "/settings",
      controlSizeClass: "h-11 w-11",
      iconSizeClass: "h-5 w-5"
    });
  });

  test("preserves the existing dense Settings control and detail route on desktop", () => {
    expect(
      getAccountMenuPresentation({ compactSettingsLayout: false, pagePresentation: false })
    ).toEqual({
      settingsPath: "/settings/account",
      controlSizeClass: "h-9 w-9",
      iconSizeClass: "h-4 w-4"
    });
  });

  test("keeps Usage out of the shared compact and desktop menu control", () => {
    const accountMenuSource = readFileSync(new URL("./AccountMenu.tsx", import.meta.url), "utf8");
    const sidebarSource = readFileSync(new URL("./Sidebar.tsx", import.meta.url), "utf8");

    expect(accountMenuSource).not.toContain("CreditUsage");
    expect(accountMenuSource).not.toContain('to="/pricing"');
    expect(sidebarSource.match(/<AccountMenu(?:\s|\/|>)/g)).toHaveLength(2);
  });
});
