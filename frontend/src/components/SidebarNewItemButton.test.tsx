import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import { SidebarNewItemButton } from "./SidebarNewItemButton";

function renderButton({
  hasAction,
  isAgentMode = true,
  isTemporarilyDisabled = false
}: {
  hasAction: boolean;
  isAgentMode?: boolean;
  isTemporarilyDisabled?: boolean;
}): string {
  return renderToStaticMarkup(
    <SidebarNewItemButton
      hasAction={hasAction}
      isAgentMode={isAgentMode}
      isTemporarilyDisabled={isTemporarilyDisabled}
      onClick={() => {}}
    >
      {isAgentMode ? "New Task" : "New Chat"}
    </SidebarNewItemButton>
  );
}

describe("SidebarNewItemButton", () => {
  test("keeps the New Chat presentation unchanged without an Agent action", () => {
    const markup = renderButton({ hasAction: false, isAgentMode: false });

    expect(markup).toContain("New Chat");
    expect(markup).not.toContain('disabled=""');
    expect(markup).not.toContain("opacity-50");
  });

  test("keeps an available New Task button enabled and at full opacity", () => {
    const markup = renderButton({ hasAction: true });

    expect(markup).not.toContain('disabled=""');
    expect(markup).not.toContain("opacity-50");
  });

  test("blocks task creation without dimming during an existing-task selection", () => {
    const markup = renderButton({ hasAction: true, isTemporarilyDisabled: true });

    expect(markup).toContain('disabled=""');
    expect(markup).not.toContain("opacity-50");
  });

  test("keeps unavailable New Task buttons disabled and dimmed", () => {
    const markup = renderButton({ hasAction: false });

    expect(markup).toContain('disabled=""');
    expect(markup).toContain("opacity-50");
  });

  test("keeps another unavailable state dimmed during task selection", () => {
    const markup = renderButton({ hasAction: false, isTemporarilyDisabled: true });

    expect(markup).toContain('disabled=""');
    expect(markup).toContain("opacity-50");
  });
});
