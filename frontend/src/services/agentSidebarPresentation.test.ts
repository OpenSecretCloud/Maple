import { describe, expect, test } from "bun:test";

import {
  agentProjectProgressLabel,
  agentProjectTaskSummaryLabel,
  agentSidebarToggleLabel,
  agentSidebarVisualStatus,
  agentTaskAccessibleLabel,
  aggregateAgentSidebarStatus,
  type AgentSidebarAggregateStatus
} from "./agentSidebarPresentation";

describe("Agent sidebar aggregate presentation", () => {
  test("counts running and unread tasks independently", () => {
    expect(
      aggregateAgentSidebarStatus(new Set(["running-a", "running-b"]), new Set(["unread-a"]))
    ).toEqual({ runningCount: 2, unreadCount: 1 });
  });

  test.each([
    [{ runningCount: 0, unreadCount: 0 }, "idle"],
    [{ runningCount: 2, unreadCount: 0 }, "running"],
    [{ runningCount: 0, unreadCount: 2 }, "unread"],
    [{ runningCount: 1, unreadCount: 3 }, "running"]
  ] as const)("uses visual priority for %o", (status, expected) => {
    expect(agentSidebarVisualStatus(status)).toBe(expected);
  });

  test("uses the idle priority when aggregate Agent status is absent", () => {
    expect(agentSidebarVisualStatus()).toBe("idle");
  });

  test.each([
    [{ runningCount: 0, unreadCount: 0 }, "Open Agent sidebar"],
    [{ runningCount: 1, unreadCount: 0 }, "Open Agent sidebar, 1 task running"],
    [{ runningCount: 2, unreadCount: 0 }, "Open Agent sidebar, 2 tasks running"],
    [{ runningCount: 0, unreadCount: 1 }, "Open Agent sidebar, 1 completed task unread"],
    [{ runningCount: 0, unreadCount: 2 }, "Open Agent sidebar, 2 completed tasks unread"],
    [
      { runningCount: 1, unreadCount: 2 },
      "Open Agent sidebar, 1 task running, 2 completed tasks unread"
    ]
  ] satisfies ReadonlyArray<readonly [AgentSidebarAggregateStatus, string]>)(
    "builds a counted accessible label for %o",
    (status, expected) => {
      expect(agentSidebarToggleLabel(status)).toBe(expected);
    }
  );

  test("keeps the shared sidebar label when Agent status is absent", () => {
    expect(agentSidebarToggleLabel()).toBe("Open sidebar");
  });
});

describe("Agent task accessible labels", () => {
  test.each([
    [{ running: false, unread: false }, "Investigate login"],
    [{ running: true, unread: false }, "Investigate login, running"],
    [{ running: false, unread: true }, "Investigate login, completed, unread"],
    [{ running: true, unread: true }, "Investigate login, running, completed, unread"]
  ] as const)("preserves status text for %o", (status, expected) => {
    expect(agentTaskAccessibleLabel("Investigate login", status)).toBe(expected);
  });
});

describe("Agent sidebar info-card labels", () => {
  test.each([
    [0, 0, "0 tasks"],
    [1, 0, "1 task"],
    [1, 1, "1 task · 1 unread"],
    [3, 2, "3 tasks · 2 unread"]
  ] as const)("formats %i project tasks with %i unread", (taskCount, unreadCount, expected) => {
    expect(agentProjectTaskSummaryLabel(taskCount, unreadCount)).toBe(expected);
  });

  test.each([
    [0, "No tasks in progress"],
    [1, "1 task in progress"],
    [3, "3 tasks in progress"]
  ] as const)("formats an in-progress project count of %i", (count, expected) => {
    expect(agentProjectProgressLabel(count)).toBe(expected);
  });
});
