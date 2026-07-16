import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import { WorkspaceModeSwitch, type WorkspaceMode } from "./WorkspaceModeSwitch";

function renderSwitch(mode: WorkspaceMode): string {
  return renderToStaticMarkup(<WorkspaceModeSwitch mode={mode} onModeChange={() => {}} />);
}

function radioMarkup(markup: string, mode: WorkspaceMode): string {
  const match = markup.match(new RegExp(`<input[^>]*id="workspace-mode-${mode}"[^>]*>`));
  if (!match) throw new Error(`Missing ${mode} radio`);
  return match[0];
}

describe("WorkspaceModeSwitch", () => {
  test("renders an accessible radio group with persistent Chat and Agent options", () => {
    const markup = renderSwitch("chat");

    expect(markup).toContain("<fieldset");
    expect(markup).toContain('<legend class="sr-only">Workspace mode</legend>');
    expect(markup.match(/type="radio"/g)).toHaveLength(2);
    expect(markup.match(/name="workspace-mode"/g)).toHaveLength(2);

    for (const mode of ["chat", "agent"] as const) {
      expect(radioMarkup(markup, mode)).toContain(`value="${mode}"`);
      expect(markup).toContain(`<label for="workspace-mode-${mode}"`);
    }

    expect(markup).toContain(">Chat</span>");
    expect(markup).toContain(">Agent</span>");
    expect(markup).toContain("lucide-message-circle");
    expect(markup).toContain("lucide-bot");
  });

  test.each(["chat", "agent"] as const)("marks only %s as selected", (activeMode) => {
    const markup = renderSwitch(activeMode);
    const inactiveMode: WorkspaceMode = activeMode === "chat" ? "agent" : "chat";

    expect(radioMarkup(markup, activeMode)).toContain('checked=""');
    expect(radioMarkup(markup, inactiveMode)).not.toContain("checked");
  });
});
