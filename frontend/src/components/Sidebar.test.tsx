import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import { SidebarToggle } from "./Sidebar";

function renderToggle(agentStatus?: { runningCount: number; unreadCount: number }): string {
  return renderToStaticMarkup(<SidebarToggle onToggle={() => {}} agentStatus={agentStatus} />);
}

describe("SidebarToggle", () => {
  test("keeps the default Chat presentation unchanged", () => {
    const markup = renderToggle();

    expect(markup).toContain('aria-label="Open sidebar"');
    expect(markup).toContain("lucide-menu");
    expect(markup).not.toContain("data-agent-sidebar-status");
  });

  test("renders a motion-safe running indicator and reports every aggregate count", () => {
    const markup = renderToggle({ runningCount: 1, unreadCount: 2 });

    expect(markup).toContain(
      'aria-label="Open Agent sidebar, 1 task running, 2 completed tasks unread"'
    );
    expect(markup).toContain('data-agent-sidebar-status="running"');
    expect(markup).toContain("motion-safe:animate-spin");
    expect(markup).toContain('aria-hidden="true"');
    expect(markup).not.toContain('data-agent-sidebar-status="unread"');
  });

  test("renders the unread indicator only when no task is running", () => {
    const markup = renderToggle({ runningCount: 0, unreadCount: 2 });

    expect(markup).toContain('aria-label="Open Agent sidebar, 2 completed tasks unread"');
    expect(markup).toContain('data-agent-sidebar-status="unread"');
    expect(markup).not.toContain('data-agent-sidebar-status="running"');
  });

  test("keeps an idle Agent toggle free of a visual status marker", () => {
    const markup = renderToggle({ runningCount: 0, unreadCount: 0 });

    expect(markup).toContain('aria-label="Open Agent sidebar"');
    expect(markup).not.toContain("data-agent-sidebar-status");
  });
});
