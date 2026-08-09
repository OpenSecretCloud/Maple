export type AgentSidebarAggregateStatus = Readonly<{
  runningCount: number;
  unreadCount: number;
}>;

export type AgentSidebarAggregateVisualStatus = "idle" | "running" | "unread";

export interface AgentTaskAccessibleStatus {
  running: boolean;
  unread: boolean;
}

export function aggregateAgentSidebarStatus(
  runningSessionIds: ReadonlySet<string>,
  completedUnreadSessionIds: ReadonlySet<string>
): AgentSidebarAggregateStatus {
  return {
    runningCount: runningSessionIds.size,
    unreadCount: completedUnreadSessionIds.size
  };
}

export function agentSidebarVisualStatus(
  status?: AgentSidebarAggregateStatus
): AgentSidebarAggregateVisualStatus {
  if (!status) return "idle";
  if (status.runningCount > 0) return "running";
  if (status.unreadCount > 0) return "unread";
  return "idle";
}

export function agentSidebarToggleLabel(status?: AgentSidebarAggregateStatus): string {
  if (!status) return "Open sidebar";

  const details: string[] = [];

  if (status.runningCount > 0) {
    details.push(`${status.runningCount} ${pluralize("task", status.runningCount)} running`);
  }
  if (status.unreadCount > 0) {
    details.push(`${status.unreadCount} completed ${pluralize("task", status.unreadCount)} unread`);
  }

  return details.length > 0 ? `Open Agent sidebar, ${details.join(", ")}` : "Open Agent sidebar";
}

export function agentTaskAccessibleLabel(title: string, status: AgentTaskAccessibleStatus): string {
  const details: string[] = [];
  if (status.running) details.push("running");
  if (status.unread) details.push("completed, unread");
  return details.length > 0 ? `${title}, ${details.join(", ")}` : title;
}

export function agentProjectTaskSummaryLabel(taskCount: number, unreadCount: number): string {
  const taskCountLabel = `${taskCount} ${pluralize("task", taskCount)}`;
  return unreadCount > 0 ? `${taskCountLabel} · ${unreadCount} unread` : taskCountLabel;
}

export function agentProjectProgressLabel(inProgressCount: number): string {
  return inProgressCount > 0
    ? `${inProgressCount} ${pluralize("task", inProgressCount)} in progress`
    : "No tasks in progress";
}

function pluralize(noun: string, count: number): string {
  return count === 1 ? noun : `${noun}s`;
}
