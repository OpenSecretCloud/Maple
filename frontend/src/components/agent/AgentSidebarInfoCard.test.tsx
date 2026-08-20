import { afterEach, describe, expect, mock, test } from "bun:test";
import { Folder, MessageSquare } from "lucide-react";
import { renderToStaticMarkup } from "react-dom/server";
import { act, create, type ReactTestRenderer } from "react-test-renderer";

import { AgentSidebarInfoCard } from "./AgentSidebarInfoCard";
import { formatAgentSidebarDate, latestAgentSidebarUpdatedMs } from "./agentSidebarInfoCardDate";

const LONG_PROJECT_PATH = "/Users/admin/workspaces/agent-mode-sidebar/maple/frontend";

describe("AgentSidebarInfoCard", () => {
  let renderer: ReactTestRenderer | null = null;

  afterEach(() => {
    if (renderer) {
      act(() => renderer?.unmount());
      renderer = null;
    }
  });

  test("presents the title, metadata, progress, and a middle-truncated folder path", () => {
    const markup = renderToStaticMarkup(
      <AgentSidebarInfoCard
        folderPath={LONG_PROJECT_PATH}
        icon={Folder}
        isInProgress
        metadata="3 tasks · 2 unread"
        metadataIcon={MessageSquare}
        onDismiss={() => {}}
        onOpenProjectFolder={() => {}}
        progressLabel="2 tasks in progress"
        title="Maple sidebar"
      />
    );

    expect(markup).toContain(">Maple sidebar</p>");
    expect(markup).toContain(">3 tasks · 2 unread</span>");
    expect(markup).toContain(">2 tasks in progress</span>");
    expect(markup).toContain("motion-safe:animate-ping");
    expect(markup).toContain(`aria-label="Open project folder: ${LONG_PROJECT_PATH}"`);
    expect(markup).toContain(`<span class="sr-only">${LONG_PROJECT_PATH}</span>`);
    expect(markup).toContain('<span aria-hidden="true">/Users/admin/…/maple/frontend</span>');
    expect(markup).not.toContain("<time");
  });

  test("places a compact last-updated date in the top-right of the header", () => {
    const updatedMs = Date.UTC(2026, 7, 20, 14, 5);
    const dateLabel = formatAgentSidebarDate(updatedMs, Date.UTC(2026, 7, 20))!;
    const markup = renderToStaticMarkup(
      <AgentSidebarInfoCard
        folderPath={LONG_PROJECT_PATH}
        icon={Folder}
        isInProgress={false}
        metadata="1 task"
        metadataIcon={MessageSquare}
        onDismiss={() => {}}
        onOpenProjectFolder={() => {}}
        progressLabel="No tasks in progress"
        title="Maple sidebar"
        updatedMs={updatedMs}
      />
    );

    expect(markup).toContain(`dateTime="${new Date(updatedMs).toISOString()}"`);
    expect(markup).toContain(`>${dateLabel}</time>`);
    expect(markup).toContain("text-[11px] font-medium leading-4 text-muted-foreground");
  });

  test("isolates the folder interaction, dismisses the card, and opens the folder once", () => {
    const callOrder: string[] = [];
    const onDismiss = mock(() => callOrder.push("dismiss"));
    const onOpenProjectFolder = mock(() => callOrder.push("open"));

    act(() => {
      renderer = create(
        <AgentSidebarInfoCard
          folderPath="/Users/admin/workspaces/maple"
          icon={Folder}
          isInProgress={false}
          metadata="1 task"
          metadataIcon={MessageSquare}
          onDismiss={onDismiss}
          onOpenProjectFolder={onOpenProjectFolder}
          progressLabel="No tasks in progress"
          title="Maple"
        />
      );
    });

    if (!renderer) throw new Error("AgentSidebarInfoCard did not mount");
    const folderButton = renderer.root.findByType("button");
    expect(onDismiss).not.toHaveBeenCalled();
    expect(onOpenProjectFolder).not.toHaveBeenCalled();

    const preventDefault = mock(() => {});
    const stopClickPropagation = mock(() => {});

    act(() => {
      folderButton.props.onClick({
        preventDefault,
        stopPropagation: stopClickPropagation
      });
    });

    expect(preventDefault).toHaveBeenCalledTimes(1);
    expect(stopClickPropagation).toHaveBeenCalledTimes(1);
    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(onOpenProjectFolder).toHaveBeenCalledTimes(1);
    expect(callOrder).toEqual(["dismiss", "open"]);
  });
});

describe("formatAgentSidebarDate", () => {
  test("uses a compact same-year date and includes the year otherwise", () => {
    const nowMs = Date.UTC(2026, 7, 20);
    expect(formatAgentSidebarDate(Date.UTC(2026, 7, 3), nowMs)).toBe(
      new Date(Date.UTC(2026, 7, 3)).toLocaleDateString(undefined, {
        month: "short",
        day: "numeric"
      })
    );
    expect(formatAgentSidebarDate(Date.UTC(2025, 11, 31), nowMs)).toBe(
      new Date(Date.UTC(2025, 11, 31)).toLocaleDateString(undefined, {
        month: "short",
        day: "numeric",
        year: "numeric"
      })
    );
  });

  test("omits invalid timestamps", () => {
    expect(formatAgentSidebarDate(0)).toBeNull();
    expect(formatAgentSidebarDate(Number.NaN)).toBeNull();
    expect(formatAgentSidebarDate(-1)).toBeNull();
  });
});

describe("latestAgentSidebarUpdatedMs", () => {
  test("returns the newest valid timestamp", () => {
    expect(latestAgentSidebarUpdatedMs([100, 0, 250, Number.NaN, -3])).toBe(250);
    expect(latestAgentSidebarUpdatedMs([])).toBeUndefined();
    expect(latestAgentSidebarUpdatedMs([0, Number.NaN])).toBeUndefined();
  });
});
