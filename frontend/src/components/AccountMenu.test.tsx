import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

describe("AccountMenu", () => {
  test("keeps Usage out of the shared compact and desktop menu control", () => {
    const accountMenuSource = readFileSync(new URL("./AccountMenu.tsx", import.meta.url), "utf8");
    const sidebarSource = readFileSync(new URL("./Sidebar.tsx", import.meta.url), "utf8");

    expect(accountMenuSource).not.toContain("CreditUsage");
    expect(accountMenuSource).not.toContain('to="/pricing"');
    expect(sidebarSource.match(/<AccountMenu(?:\s|\/|>)/g)).toHaveLength(2);
  });
});
