import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { RefreshCw } from "lucide-react";
import { MapleWordmark } from "@/components/MapleWordmark";
import { Button } from "@/components/ui/button";
import {
  AgentHistoryPaginationCache,
  AgentHistoryProjectionLimitError,
  type AgentHistoryPageToken,
  type AgentHistorySnapshot
} from "@/services/agentHistoryPagination";
import {
  AgentRemoteSessionPaginationCache,
  AgentRemoteSessionWindowLimitError
} from "@/services/agentRemoteSessionPagination";
import { type AgentTimelineItem } from "@/services/agentRuntimeService";
import {
  type AgentRemoteReadOnlyClient,
  type AgentRemoteRuntimeSummary,
  type AgentRemoteSessionSummary
} from "@/services/agentRemoteProviderBridge";

const REMOTE_AGENT_PAGE_SIZE = 25;
const MAX_REMOTE_AGENT_SESSION_SUMMARIES = 200;

type RemoteAgentLoadError = "unavailable" | "windowLimit" | null;

export interface RemoteAgentReadOnlyModeProps {
  readonly client: AgentRemoteReadOnlyClient;
  readonly runtimeKey: string;
}

/**
 * Persisted transcript browser for an authenticated paired host. This surface
 * intentionally has no composer, permissions, mutations, settings, MCP,
 * administration, raw tool payloads, or live-event attachment.
 */
export function RemoteAgentReadOnlyMode({ client, runtimeKey }: RemoteAgentReadOnlyModeProps) {
  const accountId = client.binding.accountId;
  const targetId = client.binding.targetId;
  const ownerKey = JSON.stringify([accountId, targetId, runtimeKey]);
  const { sessionCache, historyCache } = useMemo(
    () => createRemoteAgentCaches(ownerKey, accountId, targetId),
    [accountId, ownerKey, targetId]
  );
  const [runtimeSummary, setRuntimeSummary] = useState<AgentRemoteRuntimeSummary | null>(null);
  const [runtimeLoading, setRuntimeLoading] = useState(true);
  const [runtimeError, setRuntimeError] = useState<RemoteAgentLoadError>(null);
  const [sessionSnapshot, setSessionSnapshot] = useState(() => sessionCache.snapshot());
  const [sessionError, setSessionError] = useState<RemoteAgentLoadError>(null);
  const [selectedSessionId, setSelectedSessionId] = useState<string | null>(null);
  const [historySnapshot, setHistorySnapshot] = useState<AgentHistorySnapshot | null>(null);
  const [historyError, setHistoryError] = useState<RemoteAgentLoadError>(null);
  const lifecycleGenerationRef = useRef(0);
  const statusRequestRef = useRef(0);
  const selectedSessionIdRef = useRef<string | null>(null);

  const loadRuntimeStatus = useCallback(
    async (generation: number) => {
      const requestId = ++statusRequestRef.current;
      setRuntimeLoading(true);
      setRuntimeError(null);
      try {
        const status = await client.getRuntimeStatus();
        if (
          lifecycleGenerationRef.current !== generation ||
          statusRequestRef.current !== requestId
        ) {
          return;
        }
        setRuntimeSummary(status);
      } catch {
        if (
          lifecycleGenerationRef.current === generation &&
          statusRequestRef.current === requestId
        ) {
          setRuntimeSummary(null);
          setRuntimeError("unavailable");
        }
      } finally {
        if (
          lifecycleGenerationRef.current === generation &&
          statusRequestRef.current === requestId
        ) {
          setRuntimeLoading(false);
        }
      }
    },
    [client]
  );

  const loadSessionPage = useCallback(
    async (kind: "head" | "older", generation: number) => {
      // Refresh replaces the bounded task window instead of retaining every
      // previously paged summary indefinitely.
      if (kind === "head") sessionCache.clear();
      const before = sessionCache.snapshot();
      const remaining = MAX_REMOTE_AGENT_SESSION_SUMMARIES - before.items.length;
      if (kind === "older" && remaining <= 0) return;
      const token = kind === "head" ? sessionCache.beginHead() : sessionCache.beginOlder();
      if (!token) return;
      const limit =
        kind === "head" ? REMOTE_AGENT_PAGE_SIZE : Math.min(REMOTE_AGENT_PAGE_SIZE, remaining);
      setSessionSnapshot(sessionCache.snapshot());
      setSessionError(null);
      try {
        const page = await client.listSessionSummariesPage({
          cursor: token.cursor,
          limit
        });
        if (lifecycleGenerationRef.current !== generation) {
          sessionCache.fail(token);
          return;
        }
        sessionCache.commit(token, page);
        setSessionSnapshot(sessionCache.snapshot());
      } catch (error) {
        sessionCache.fail(token);
        if (lifecycleGenerationRef.current === generation) {
          const snapshot = sessionCache.snapshot();
          setSessionSnapshot(snapshot);
          if (!snapshot.isLoading) {
            setSessionError(
              error instanceof AgentRemoteSessionWindowLimitError ? "windowLimit" : "unavailable"
            );
          }
        }
      }
    },
    [client, sessionCache]
  );

  const loadHistoryPage = useCallback(
    async (sessionId: string, kind: "head" | "older", generation: number) => {
      let token: AgentHistoryPageToken | null = null;
      try {
        token =
          kind === "head" ? historyCache.beginHead(sessionId) : historyCache.beginOlder(sessionId);
        if (!token) return;
        if (selectedSessionIdRef.current === sessionId) {
          setHistorySnapshot(historyCache.snapshot(sessionId));
          setHistoryError(null);
        }
        const page = await client.listPersistedRecordsPage({
          sessionId,
          cursor: token.cursor,
          limit: REMOTE_AGENT_PAGE_SIZE
        });
        if (lifecycleGenerationRef.current !== generation) {
          historyCache.fail(token);
          return;
        }
        const result = historyCache.commit(token, page);
        historyCache.reconcileRetention(
          new Set(selectedSessionIdRef.current ? [selectedSessionIdRef.current] : [])
        );
        if (selectedSessionIdRef.current === sessionId) {
          setHistorySnapshot(historyCache.snapshot(sessionId));
          setHistoryError(result === "history-replaced" ? "unavailable" : null);
        }
      } catch (error) {
        if (token) historyCache.fail(token);
        historyCache.reconcileRetention(
          new Set(selectedSessionIdRef.current ? [selectedSessionIdRef.current] : [])
        );
        if (
          lifecycleGenerationRef.current === generation &&
          selectedSessionIdRef.current === sessionId
        ) {
          const snapshot = historyCache.snapshot(sessionId);
          setHistorySnapshot(snapshot);
          if (!snapshot.isLoading) {
            setHistoryError(
              error instanceof AgentHistoryProjectionLimitError ? "windowLimit" : "unavailable"
            );
          }
        }
      }
    },
    [client, historyCache]
  );

  useEffect(() => {
    const generation = ++lifecycleGenerationRef.current;
    sessionCache.clear();
    historyCache.clear();
    selectedSessionIdRef.current = null;
    setSelectedSessionId(null);
    setHistorySnapshot(null);
    setHistoryError(null);
    setSessionSnapshot(sessionCache.snapshot());
    void loadRuntimeStatus(generation);
    void loadSessionPage("head", generation);
    return () => {
      if (lifecycleGenerationRef.current === generation) {
        lifecycleGenerationRef.current += 1;
        statusRequestRef.current += 1;
      }
    };
  }, [client, historyCache, loadRuntimeStatus, loadSessionPage, ownerKey, sessionCache]);

  const selectSession = useCallback(
    (sessionId: string) => {
      selectedSessionIdRef.current = sessionId;
      setSelectedSessionId(sessionId);
      setHistoryError(null);
      historyCache.reconcileRetention(new Set([sessionId]));
      const snapshot = historyCache.snapshot(sessionId);
      setHistorySnapshot(snapshot);
      if (!snapshot.headLoaded && !snapshot.isLoading) {
        void loadHistoryPage(sessionId, "head", lifecycleGenerationRef.current);
      }
    },
    [historyCache, loadHistoryPage]
  );

  const selectedSession = selectedSessionId
    ? (sessionSnapshot.items.find((session) => session.id === selectedSessionId) ?? null)
    : null;
  const reachedSessionWindow = sessionSnapshot.items.length >= MAX_REMOTE_AGENT_SESSION_SUMMARIES;

  return (
    <main className="flex min-h-dvh flex-col bg-background text-foreground">
      <header className="border-b border-border px-4 py-4 sm:px-6">
        <div className="mx-auto flex w-full max-w-6xl items-start justify-between gap-4">
          <div className="min-w-0 space-y-1">
            <a href="/" className="inline-flex text-xs text-muted-foreground hover:text-foreground">
              Back to chats
            </a>
            <MapleWordmark className="h-4 w-auto" />
            <h1 className="text-lg font-semibold">Paired host history</h1>
            <p className="text-sm text-muted-foreground">
              Read-only persisted transcripts from{" "}
              {client.binding.targetLabel ?? "your paired host"}.
            </p>
          </div>
          <RuntimeStatus
            summary={runtimeSummary}
            loading={runtimeLoading}
            error={runtimeError}
            onRetry={() => void loadRuntimeStatus(lifecycleGenerationRef.current)}
          />
        </div>
      </header>

      <div className="mx-auto grid w-full max-w-6xl flex-1 grid-cols-1 md:grid-cols-[minmax(15rem,20rem)_minmax(0,1fr)]">
        <aside
          className="border-b border-border p-4 md:border-b-0 md:border-r"
          aria-label="Agent tasks"
        >
          <div className="mb-3 flex items-center justify-between gap-2">
            <h2 className="text-sm font-semibold">Tasks</h2>
            <Button
              type="button"
              size="sm"
              variant="ghost"
              disabled={sessionSnapshot.isLoading}
              onClick={() => void loadSessionPage("head", lifecycleGenerationRef.current)}
              aria-label="Refresh paired host tasks"
            >
              <RefreshCw className="size-4" aria-hidden="true" />
            </Button>
          </div>

          {sessionError && (
            <LoadNotice
              onRetry={() => void loadSessionPage("head", lifecycleGenerationRef.current)}
            >
              Maple couldn’t load tasks from the paired host.
            </LoadNotice>
          )}
          {!sessionSnapshot.headLoaded && sessionSnapshot.isLoading && (
            <p role="status" className="text-sm text-muted-foreground">
              Loading tasks…
            </p>
          )}
          {sessionSnapshot.headLoaded && sessionSnapshot.items.length === 0 && (
            <p className="text-sm text-muted-foreground">No persisted Agent tasks are available.</p>
          )}

          <ul className="grid gap-1">
            {sessionSnapshot.items.map((session) => (
              <li key={session.id}>
                <button
                  type="button"
                  aria-pressed={selectedSessionId === session.id}
                  className={`w-full rounded-md px-3 py-2 text-left transition-colors ${
                    selectedSessionId === session.id
                      ? "bg-accent text-accent-foreground"
                      : "hover:bg-muted"
                  }`}
                  onClick={() => selectSession(session.id)}
                >
                  <span className="block truncate text-sm font-medium">{session.title}</span>
                  <span className="mt-0.5 block text-xs text-muted-foreground">
                    {formatAgentDate(session.updatedMs)} · {session.messageCount} messages
                  </span>
                </button>
              </li>
            ))}
          </ul>

          {sessionSnapshot.hasMore && !reachedSessionWindow && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="mt-3 w-full"
              disabled={sessionSnapshot.isLoading}
              onClick={() => void loadSessionPage("older", lifecycleGenerationRef.current)}
            >
              {sessionSnapshot.isLoading ? "Loading…" : "Load older tasks"}
            </Button>
          )}
          {reachedSessionWindow && sessionSnapshot.hasMore && (
            <p className="mt-3 text-xs text-muted-foreground">
              Showing the newest {MAX_REMOTE_AGENT_SESSION_SUMMARIES} tasks.
            </p>
          )}
        </aside>

        <section className="min-w-0 p-4 sm:p-6" aria-label="Selected Agent transcript">
          {!selectedSessionId ? (
            <div className="flex min-h-64 items-center justify-center text-center text-sm text-muted-foreground">
              Choose a task to browse its persisted transcript.
            </div>
          ) : (
            <TranscriptPanel
              session={selectedSession}
              snapshot={historySnapshot}
              error={historyError}
              onRefresh={() =>
                void loadHistoryPage(selectedSessionId, "head", lifecycleGenerationRef.current)
              }
              onLoadOlder={() =>
                void loadHistoryPage(selectedSessionId, "older", lifecycleGenerationRef.current)
              }
            />
          )}
        </section>
      </div>
    </main>
  );
}

function RuntimeStatus({
  summary,
  loading,
  error,
  onRetry
}: {
  readonly summary: AgentRemoteRuntimeSummary | null;
  readonly loading: boolean;
  readonly error: RemoteAgentLoadError;
  readonly onRetry: () => void;
}) {
  if (loading) {
    return (
      <span role="status" className="text-xs text-muted-foreground">
        Checking host…
      </span>
    );
  }
  if (error || !summary) {
    return (
      <Button type="button" variant="outline" size="sm" onClick={onRetry}>
        Retry host status
      </Button>
    );
  }
  return (
    <div className="shrink-0 text-right text-xs text-muted-foreground">
      <span className="block font-medium text-foreground">
        Host {summary.running ? "active" : "idle"}
      </span>
      {summary.activeRunCount > 0 && <span>{summary.activeRunCount} active runs</span>}
    </div>
  );
}

function TranscriptPanel({
  session,
  snapshot,
  error,
  onRefresh,
  onLoadOlder
}: {
  readonly session: AgentRemoteSessionSummary | null;
  readonly snapshot: AgentHistorySnapshot | null;
  readonly error: RemoteAgentLoadError;
  readonly onRefresh: () => void;
  readonly onLoadOlder: () => void;
}) {
  return (
    <div className="mx-auto w-full max-w-3xl">
      <div className="mb-5 flex items-start justify-between gap-3 border-b border-border pb-4">
        <div className="min-w-0">
          <h2 className="truncate text-base font-semibold">{session?.title ?? "Agent task"}</h2>
          <p className="text-xs text-muted-foreground">
            Persisted history only. Live updates and Agent actions are unavailable here.
          </p>
        </div>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          disabled={snapshot?.isLoading ?? true}
          onClick={onRefresh}
          aria-label="Refresh persisted Agent transcript"
        >
          <RefreshCw className="size-4" aria-hidden="true" />
        </Button>
      </div>

      {error && (
        <LoadNotice onRetry={onRefresh}>
          {error === "windowLimit"
            ? "This transcript reached Maple’s bounded history window."
            : "Maple couldn’t load this persisted transcript."}
        </LoadNotice>
      )}
      {!snapshot?.headLoaded && snapshot?.isLoading && (
        <p role="status" className="text-sm text-muted-foreground">
          Loading transcript…
        </p>
      )}
      {snapshot?.headLoaded && snapshot.timeline.length === 0 && (
        <p className="text-sm text-muted-foreground">This task has no persisted transcript.</p>
      )}

      {snapshot && snapshot.timeline.length > 0 && (
        <ol className="grid gap-3" aria-label="Persisted transcript items">
          {snapshot.timeline.map((item) => (
            <TranscriptItem key={item.id} item={item} />
          ))}
        </ol>
      )}

      {snapshot?.hasMore && (
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="mt-5"
          disabled={snapshot.isLoading}
          onClick={onLoadOlder}
        >
          {snapshot.isLoading ? "Loading…" : "Load older transcript"}
        </Button>
      )}
    </div>
  );
}

function TranscriptItem({ item }: { readonly item: AgentTimelineItem }) {
  const label = transcriptItemLabel(item);
  return (
    <li
      className={`rounded-lg border border-border px-4 py-3 ${
        item.role === "user" ? "bg-muted/60" : "bg-card"
      }`}
      data-item-type={item.itemType}
    >
      <div className="mb-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
        <span className="font-medium text-foreground">{label}</span>
        {item.status && <span>{item.status}</span>}
      </div>
      {item.title && item.title !== label && <p className="text-sm font-medium">{item.title}</p>}
      {item.text && <p className="whitespace-pre-wrap break-words text-sm">{item.text}</p>}
    </li>
  );
}

function transcriptItemLabel(item: AgentTimelineItem): string {
  if (item.itemType === "message") {
    if (item.role === "user") return "You";
    if (item.role === "assistant") return "Assistant";
  }
  switch (item.itemType) {
    case "thinking":
      return "Thinking";
    case "tool":
      return "Tool activity";
    case "permission":
      return "Tool permission";
    case "error":
      return "Agent error";
    case "system":
      return "System";
    default:
      return "Message";
  }
}

function LoadNotice({
  children,
  onRetry
}: {
  readonly children: React.ReactNode;
  onRetry: () => void;
}) {
  return (
    <div className="mb-3 rounded-md border border-border bg-muted/40 p-3 text-sm">
      <p>{children}</p>
      <Button type="button" variant="link" size="sm" className="h-auto px-0" onClick={onRetry}>
        Try again
      </Button>
    </div>
  );
}

function formatAgentDate(timestamp: number): string {
  const date = new Date(timestamp);
  return Number.isNaN(date.getTime()) ? "Unknown date" : date.toLocaleDateString();
}

function createRemoteAgentCaches(ownerKey: string, accountId: string, targetId: string) {
  // The opaque lifecycle key deliberately participates in cache identity even
  // though it is not authorization and is never forwarded to the bridge.
  if (!ownerKey) throw new Error("Remote Agent transcript cache owner is unavailable");
  return {
    sessionCache: new AgentRemoteSessionPaginationCache(MAX_REMOTE_AGENT_SESSION_SUMMARIES),
    historyCache: new AgentHistoryPaginationCache({ accountId, targetId })
  };
}
