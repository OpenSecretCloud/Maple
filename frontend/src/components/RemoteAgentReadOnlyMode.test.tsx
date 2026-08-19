import { afterEach, describe, expect, mock, test } from "bun:test";
import { act, create, type ReactTestInstance, type ReactTestRenderer } from "react-test-renderer";
import { RemoteAgentReadOnlyMode } from "@/components/RemoteAgentReadOnlyMode";
import { AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES } from "@/services/agentRemoteCapabilities";
import type {
  AgentRemoteReadOnlyClient,
  AgentRemoteRecordsPage,
  AgentRemoteSessionPage,
  AgentRemoteSessionSummary
} from "@/services/agentRemoteProviderBridge";

function session(id: string, title: string, updatedMs: number): AgentRemoteSessionSummary {
  return {
    id,
    title,
    createdMs: updatedMs - 1,
    updatedMs,
    pageSortMs: updatedMs,
    messageCount: 2
  };
}

const SESSION_A = session("session-a", "Task A", 20);
const SESSION_B = session("session-b", "Task B", 10);

function headHistoryPage(): AgentRemoteRecordsPage {
  return {
    // Native pages are newest-first; the cache must present record containers
    // chronologically without splitting their internal items.
    records: [
      {
        recordId: "record-new",
        role: "assistant",
        createdMs: 20,
        items: [
          {
            id: "permission-a",
            itemType: "permission",
            role: "system",
            title: "Tool permission",
            status: "completed",
            createdMs: 20,
            merge: "replace"
          },
          {
            id: "assistant-a",
            itemType: "message",
            role: "assistant",
            text: "<script>remoteCanary()</script>",
            createdMs: 21,
            merge: "replace"
          }
        ]
      },
      {
        recordId: "record-old",
        role: "user",
        createdMs: 10,
        items: [
          {
            id: "user-a",
            itemType: "message",
            role: "user",
            text: "Older user text",
            createdMs: 10,
            merge: "replace"
          }
        ]
      }
    ],
    nextCursor: "history-cursor-older",
    historyRevision: "history-revision-a"
  };
}

function client(overrides?: {
  readonly getRuntimeStatus?: AgentRemoteReadOnlyClient["getRuntimeStatus"];
  readonly listSessionSummariesPage?: AgentRemoteReadOnlyClient["listSessionSummariesPage"];
  readonly listPersistedRecordsPage?: AgentRemoteReadOnlyClient["listPersistedRecordsPage"];
}): AgentRemoteReadOnlyClient {
  return Object.freeze({
    binding: Object.freeze({
      accountId: "account-a",
      targetId: "target-a",
      targetLabel: "Office Mac"
    }),
    capabilities: AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES,
    getRuntimeStatus:
      overrides?.getRuntimeStatus ?? (async () => ({ running: true, activeRunCount: 1 })),
    listSessionSummariesPage:
      overrides?.listSessionSummariesPage ??
      (async () => ({ items: [SESSION_A], nextCursor: "session-cursor-older" })),
    listPersistedRecordsPage: overrides?.listPersistedRecordsPage ?? (async () => headHistoryPage())
  });
}

function nodeText(node: ReactTestInstance): string {
  return node.children
    .map((child) => (typeof child === "string" ? child : nodeText(child)))
    .join("");
}

function button(renderer: ReactTestRenderer, text: string): ReactTestInstance {
  const match = renderer.root
    .findAllByType("button")
    .find((candidate) => nodeText(candidate).includes(text));
  if (!match) throw new Error(`Button not found: ${text}`);
  return match;
}

async function settle(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

describe("RemoteAgentReadOnlyMode", () => {
  let renderer: ReactTestRenderer | null = null;

  afterEach(() => {
    if (renderer) act(() => renderer?.unmount());
    renderer = null;
  });

  test("loads only bounded read pages and renders persisted transcript as React text", async () => {
    const statusCalls: unknown[][] = [];
    const sessionCalls: Parameters<AgentRemoteReadOnlyClient["listSessionSummariesPage"]>[] = [];
    const historyCalls: Parameters<AgentRemoteReadOnlyClient["listPersistedRecordsPage"]>[] = [];
    const readOnlyClient = client({
      getRuntimeStatus: mock(async (...args: unknown[]) => {
        statusCalls.push(args);
        return { running: true, activeRunCount: 1 };
      }),
      listSessionSummariesPage: mock(async (request) => {
        sessionCalls.push([request]);
        return request.cursor
          ? { items: [SESSION_B], nextCursor: null }
          : { items: [SESSION_A], nextCursor: "session-cursor-older" };
      }),
      listPersistedRecordsPage: mock(async (request): Promise<AgentRemoteRecordsPage> => {
        historyCalls.push([request]);
        return request.cursor
          ? {
              records: [
                {
                  recordId: "record-oldest",
                  role: "system",
                  createdMs: 1,
                  items: [
                    {
                      id: "system-oldest",
                      itemType: "system",
                      role: "system",
                      text: "Oldest persisted text",
                      createdMs: 1,
                      merge: "replace"
                    }
                  ]
                }
              ],
              nextCursor: null,
              historyRevision: "history-revision-a"
            }
          : headHistoryPage();
      })
    });

    await act(async () => {
      renderer = create(<RemoteAgentReadOnlyMode client={readOnlyClient} runtimeKey="binding-a" />);
      await settle();
    });
    if (!renderer) throw new Error("Remote Agent transcript browser did not mount");

    expect(statusCalls).toEqual([[]]);
    expect(sessionCalls).toEqual([[{ cursor: null, limit: 25 }]]);
    expect(historyCalls).toEqual([]);
    const initialText = nodeText(renderer.root);
    expect(initialText).toContain("Read-only persisted transcripts from Office Mac");
    for (const prohibited of [
      "New task",
      "Send message",
      "Rename",
      "Delete",
      "Allow once",
      "Deny once",
      "MCP",
      "Start runtime"
    ]) {
      expect(initialText).not.toContain(prohibited);
    }

    await act(async () => {
      button(renderer!, "Task A").props.onClick();
      await settle();
    });
    expect(historyCalls).toEqual([[{ sessionId: "session-a", cursor: null, limit: 25 }]]);
    // A next cursor is presented as one explicit button, never followed in an
    // automatic full-history loop.
    expect(button(renderer, "Load older transcript")).toBeDefined();
    expect(historyCalls).toHaveLength(1);

    const transcriptItems = renderer.root.findAll(
      (node) => typeof node.props["data-item-type"] === "string"
    );
    expect(transcriptItems.map(nodeText)).toEqual([
      "YouOlder user text",
      "Tool permissioncompleted",
      "Assistant<script>remoteCanary()</script>"
    ]);
    expect(renderer.root.findAllByType("script")).toHaveLength(0);
    const renderedButtons = renderer.root.findAllByType("button").map(nodeText);
    expect(renderedButtons).not.toContain("Allow once");
    expect(renderedButtons).not.toContain("Deny once");
    expect(renderedButtons).not.toContain("Cancel");

    await act(async () => {
      button(renderer!, "Load older transcript").props.onClick();
      await settle();
    });
    expect(historyCalls[1]).toEqual([
      { sessionId: "session-a", cursor: "history-cursor-older", limit: 25 }
    ]);

    await act(async () => {
      button(renderer!, "Load older tasks").props.onClick();
      await settle();
    });
    expect(sessionCalls[1]).toEqual([{ cursor: "session-cursor-older", limit: 25 }]);
  });

  test("does not overlap repeated history-head selection for the same task", async () => {
    let resolveHistory!: (page: AgentRemoteRecordsPage) => void;
    const historyPromise = new Promise<AgentRemoteRecordsPage>((resolve) => {
      resolveHistory = resolve;
    });
    const historyCalls: Parameters<AgentRemoteReadOnlyClient["listPersistedRecordsPage"]>[] = [];
    const readOnlyClient = client({
      listPersistedRecordsPage: mock(async (request) => {
        historyCalls.push([request]);
        return await historyPromise;
      })
    });

    await act(async () => {
      renderer = create(<RemoteAgentReadOnlyMode client={readOnlyClient} runtimeKey="binding-a" />);
      await settle();
    });
    if (!renderer) throw new Error("Remote Agent transcript browser did not mount");
    const taskButton = button(renderer, "Task A");
    act(() => {
      taskButton.props.onClick();
      taskButton.props.onClick();
    });
    expect(historyCalls).toHaveLength(1);

    await act(async () => {
      resolveHistory(headHistoryPage());
      await settle();
    });
  });

  test("settles a 129th stalled task at the bounded history-state limit", async () => {
    const sessions = Array.from({ length: 129 }, (_, index) =>
      session(
        `session-${index.toString().padStart(3, "0")}`,
        `Task ${index.toString().padStart(3, "0")}`,
        1_000 - index
      )
    );
    const historyCalls: Parameters<AgentRemoteReadOnlyClient["listPersistedRecordsPage"]>[] = [];
    const readOnlyClient = client({
      listSessionSummariesPage: mock(async (request) => {
        const start = request.cursor ? Number(request.cursor) : 0;
        const end = Math.min(start + request.limit, sessions.length);
        return {
          items: sessions.slice(start, end),
          nextCursor: end < sessions.length ? String(end) : null
        };
      }),
      listPersistedRecordsPage: mock(async (request) => {
        historyCalls.push([request]);
        return await new Promise<AgentRemoteRecordsPage>(() => {});
      })
    });

    await act(async () => {
      renderer = create(<RemoteAgentReadOnlyMode client={readOnlyClient} runtimeKey="binding-a" />);
      await settle();
    });
    if (!renderer) throw new Error("Remote Agent transcript browser did not mount");

    for (let page = 1; page < 6; page += 1) {
      await act(async () => {
        button(renderer!, "Load older tasks").props.onClick();
        await settle();
      });
    }
    expect(renderer.root.findAllByType("li")).toHaveLength(129);

    for (let index = 0; index < 128; index += 1) {
      act(() => {
        button(renderer!, `Task ${index.toString().padStart(3, "0")}`).props.onClick();
      });
    }
    expect(historyCalls).toHaveLength(128);

    await act(async () => {
      button(renderer!, "Task 128").props.onClick();
      await settle();
    });
    expect(historyCalls).toHaveLength(128);
    expect(nodeText(renderer.root)).toContain(
      "This transcript reached Maple’s bounded history window."
    );

    act(() => {
      button(renderer!, "Task 000").props.onClick();
    });
    expect(historyCalls).toHaveLength(128);
    expect(nodeText(renderer.root)).toContain("Loading transcript…");
  });

  test("renders generic failure copy without native or transport diagnostics", async () => {
    const diagnostic = "access-token-canary remote-transport-stack";
    const rejectingSessions = mock(
      async (): Promise<AgentRemoteSessionPage> => await Promise.reject(new Error(diagnostic))
    );
    const readOnlyClient = client({
      getRuntimeStatus: mock(async () => await Promise.reject(new Error(diagnostic))),
      listSessionSummariesPage: rejectingSessions
    });

    await act(async () => {
      renderer = create(<RemoteAgentReadOnlyMode client={readOnlyClient} runtimeKey="binding-a" />);
      await settle();
    });
    if (!renderer) throw new Error("Remote Agent transcript browser did not mount");
    const rendered = nodeText(renderer.root);
    expect(rendered).toContain("Maple couldn’t load tasks from the paired host.");
    expect(rendered).toContain("Retry host status");
    expect(rendered).not.toContain(diagnostic);
  });
});
