import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { SettingsAccountArea, SETTINGS_USAGE_LINK_CLASS } from "./SettingsAccountArea";

function renderAccountArea(compact: boolean) {
  return renderToStaticMarkup(
    <SettingsAccountArea
      compact={compact}
      email="long.account@example.com"
      planLabel="Maple Professional Workspace Plan"
      signOutError={null}
      isSigningOut={false}
      signOutDisabled={false}
      onSignOut={() => {}}
      usage={
        <a
          href="/pricing"
          className={SETTINGS_USAGE_LINK_CLASS}
          aria-label="Maple Professional Workspace plan"
        >
          Usage meter
        </a>
      }
    />
  );
}

describe("SettingsAccountArea", () => {
  for (const compact of [true, false]) {
    test(`orders account, Log out, and Usage in ${compact ? "compact" : "desktop"} Settings`, () => {
      const markup = renderAccountArea(compact);
      const accountIndex = markup.indexOf("long.account@example.com");
      const planIndex = markup.indexOf("Maple Professional Workspace Plan");
      const logoutIndex = markup.indexOf("Log out");
      const usageIndex = markup.indexOf('href="/pricing"');

      expect(accountIndex).toBeGreaterThan(-1);
      expect(planIndex).toBeGreaterThan(accountIndex);
      expect(logoutIndex).toBeGreaterThan(planIndex);
      expect(usageIndex).toBeGreaterThan(logoutIndex);
      expect(markup).toContain("min-h-11");
      expect(markup).toContain("Usage meter");
    });
  }
});
