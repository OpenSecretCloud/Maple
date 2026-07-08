import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useOpenSecret } from "@opensecret/react";
import {
  AlertCircle,
  Check,
  ChevronRight,
  Circle,
  FolderOpen,
  Loader2,
  MessageSquarePlus,
  RotateCcw,
  Send,
  Square,
  Terminal,
  X
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Badge } from "@/components/ui/badge";
import { Markdown } from "@/components/markdown";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue
} from "@/components/ui/select";
import { Input } from "@/components/ui/input";
import { Sidebar, SidebarToggle } from "@/components/Sidebar";
import { MapleWordmark } from "@/components/MapleWordmark";
import {
  agentRuntimeService,
  type AgentEventEnvelope,
  type AgentPermissionDecision,
  type AgentRuntimeStatus,
  type AgentSessionSummary,
  type AgentTimelineItem,
  type RecentProjectRoot
} from "@/services/agentRuntimeService";
import { proxyService } from "@/services/proxyService";
import { SIDEBAR_GRID_COLUMNS_CLASS, getSidebarLayoutStyle } from "@/constants/layout";
import { cn, useIsLandscapeMobile, useIsMobile } from "@/utils/utils";
import { isTauriDesktop } from "@/utils/platform";

const DEFAULT_MODEL = "auto:powerful";
const DEFAULT_MODE = "smart_approve";

export function AgentMode() {
  const { createApiKey } = useOpenSecret();
  const isMobile = useIsMobile();
  const isLandscapeMobile = useIsLandscapeMobile();
  const isCompactLayout = isMobile || isLandscapeMobile;
  const [isSidebarOpen, setIsSidebarOpen] = useState(false);
  const [runtimeStatus, setRuntimeStatus] = useState<AgentRuntimeStatus | null>(null);
  const [recentRoots, setRecentRoots] = useState<RecentProjectRoot[]>([]);
  const [sessions, setSessions] = useState<AgentSessionSummary[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [projectRoot, setProjectRoot] = useState("");
  const [model, setModel] = useState(DEFAULT_MODEL);
  const [timelineItems, setTimelineItems] = useState<AgentTimelineItem[]>([]);
  const [input, setInput] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [isStarting, setIsStarting] = useState(false);
  const [isSending, setIsSending] = useState(false);
  const [activeRunId, setActiveRunId] = useState<string | null>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const projectRootRef = useRef(projectRoot);
  const activeSessionIdRef = useRef(activeSessionId);

  useEffect(() => {
    if (isCompactLayout) {
      setIsSidebarOpen(false);
    }
  }, [isCompactLayout]);

  useEffect(() => {
    projectRootRef.current = projectRoot;
  }, [projectRoot]);

  useEffect(() => {
    activeSessionIdRef.current = activeSessionId;
  }, [activeSessionId]);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ block: "end" });
  }, [timelineItems]);

  const activeRootLabel = useMemo(() => {
    if (!projectRoot) return "Select folder";
    return recentRoots.find((root) => root.path === projectRoot)?.name || basename(projectRoot);
  }, [projectRoot, recentRoots]);

  const activeSession = useMemo(
    () => sessions.find((session) => session.id === activeSessionId) ?? null,
    [activeSessionId, sessions]
  );

  const toggleSidebar = useCallback(() => setIsSidebarOpen((prev) => !prev), []);

  const appendRuntimeLog = useCallback((message: string) => {
    if (!isTauriDesktop()) return;
    void agentRuntimeService.appendRuntimeLog(message).catch(() => {
      // Logging must not block Agent Mode interaction.
    });
  }, []);

  const ensureMapleProxyReady = useCallback(async () => {
    appendRuntimeLog("Ensuring Maple proxy is ready for direct Agent Mode");
    return await proxyService.ensureProxyReady(async (name) => {
      appendRuntimeLog(`Creating Agent Mode proxy API key ${name}`);
      const response = await createApiKey(name);
      return response.key;
    });
  }, [appendRuntimeLog, createApiKey]);

  const refreshSessions = useCallback(async (root = projectRootRef.current) => {
    if (!isTauriDesktop()) return;
    const status = await agentRuntimeService.getRuntimeStatus();
    setRuntimeStatus(status);
    if (!status.running) {
      setSessions([]);
      return;
    }
    const nextSessions = await agentRuntimeService.listSessions(root || status.projectRoot || null);
    setSessions(nextSessions);
    if (!activeSessionIdRef.current && nextSessions.length > 0) {
      const first = nextSessions[0];
      setActiveSessionId(first.id);
      const detail = await agentRuntimeService.loadSession(first.id);
      setTimelineItems(detail.timeline);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    async function loadInitialState() {
      if (!isTauriDesktop()) return;
      try {
        const [status, config, roots] = await Promise.all([
          agentRuntimeService.getRuntimeStatus(),
          agentRuntimeService.loadConfig(),
          agentRuntimeService.listRecentProjectRoots()
        ]);
        if (cancelled) return;

        setRuntimeStatus(status);
        setRecentRoots(roots);
        const root = status.projectRoot || config.defaultProjectRoot || roots[0]?.path || "";
        const nextModel = status.model || config.defaultModel || DEFAULT_MODEL;
        setProjectRoot(root);
        setModel(nextModel);

        await ensureMapleProxyReady();
        if (status.running) {
          await refreshSessions(root);
        } else if (root) {
          const startedStatus = await agentRuntimeService.startRuntime({
            projectRoot: root,
            model: nextModel,
            mode: DEFAULT_MODE
          });
          if (cancelled) return;
          setRuntimeStatus(startedStatus);
          await refreshSessions(startedStatus.projectRoot || root);
        }
      } catch (loadError) {
        if (!cancelled) setError(errorMessage(loadError));
      }
    }
    void loadInitialState();
    return () => {
      cancelled = true;
    };
  }, [ensureMapleProxyReady, refreshSessions]);

  const chooseProjectRoot = useCallback(async () => {
    if (!isTauriDesktop()) return;
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Select project folder"
      });
      if (typeof selected === "string") {
        setProjectRoot(selected);
        setActiveSessionId(null);
        setTimelineItems([]);
        const roots = await agentRuntimeService.saveRecentProjectRoot(selected);
        setRecentRoots(roots);
      }
    } catch (chooseError) {
      setError(errorMessage(chooseError));
    }
  }, []);

  const startRuntime = useCallback(
    async (restart = false) => {
      setError(null);
      setIsStarting(true);
      try {
        if (!projectRoot) {
          throw new Error("Select a project folder first");
        }
        await ensureMapleProxyReady();
        const request = { projectRoot, model: model || DEFAULT_MODEL, mode: DEFAULT_MODE };
        const status = restart
          ? await agentRuntimeService.restartRuntime(request)
          : await agentRuntimeService.startRuntime(request);
        setRuntimeStatus(status);
        setProjectRoot(status.projectRoot || projectRoot);
        setModel(status.model || model || DEFAULT_MODEL);
        setRecentRoots(await agentRuntimeService.listRecentProjectRoots());
        await refreshSessions(status.projectRoot || projectRoot);
        return status;
      } catch (startError) {
        setError(errorMessage(startError));
        throw startError;
      } finally {
        setIsStarting(false);
      }
    },
    [ensureMapleProxyReady, model, projectRoot, refreshSessions]
  );

  const ensureRuntimeAndSession = useCallback(async () => {
    if (!projectRoot) {
      throw new Error("Select a project folder first");
    }

    let status = await agentRuntimeService.getRuntimeStatus();
    if (!status.running) {
      status = await startRuntime(false);
    } else if (status.projectRoot && status.projectRoot !== projectRoot) {
      status = await startRuntime(true);
    }

    let sessionId = activeSessionIdRef.current;
    if (!sessionId) {
      const detail = await agentRuntimeService.createSession({
        projectRoot,
        title: "New agent session",
        model: model || DEFAULT_MODEL,
        mode: DEFAULT_MODE
      });
      sessionId = detail.session.id;
      setActiveSessionId(sessionId);
      setSessions((current) => [
        detail.session,
        ...current.filter((item) => item.id !== sessionId)
      ]);
      setTimelineItems(detail.timeline);
    }

    return sessionId;
  }, [model, projectRoot, startRuntime]);

  const createSession = useCallback(async () => {
    setError(null);
    try {
      if (!runtimeStatus?.running) {
        await startRuntime(false);
      }
      const detail = await agentRuntimeService.createSession({
        projectRoot,
        title: "New agent session",
        model: model || DEFAULT_MODEL,
        mode: DEFAULT_MODE
      });
      setActiveSessionId(detail.session.id);
      setSessions((current) => [
        detail.session,
        ...current.filter((session) => session.id !== detail.session.id)
      ]);
      setTimelineItems(detail.timeline);
    } catch (createError) {
      setError(errorMessage(createError));
    }
  }, [model, projectRoot, runtimeStatus?.running, startRuntime]);

  const loadSession = useCallback(async (sessionId: string) => {
    setError(null);
    try {
      const detail = await agentRuntimeService.loadSession(sessionId);
      setActiveSessionId(detail.session.id);
      setProjectRoot(detail.session.projectRoot);
      setTimelineItems(detail.timeline);
    } catch (loadError) {
      setError(errorMessage(loadError));
    }
  }, []);

  const sendMessage = useCallback(async () => {
    const text = input.trim();
    if (!text || isSending) return;

    setError(null);
    setInput("");
    setIsSending(true);
    try {
      const sessionId = await ensureRuntimeAndSession();
      const response = await agentRuntimeService.sendMessage({
        sessionId,
        text,
        model: model || DEFAULT_MODEL,
        mode: DEFAULT_MODE
      });
      setActiveRunId(response.runId);
    } catch (sendError) {
      const message = errorMessage(sendError);
      setError(message);
      setTimelineItems((current) => [
        ...current,
        {
          id: `error-${Date.now()}`,
          itemType: "error",
          role: "system",
          title: "Agent error",
          text: message,
          status: "failed",
          createdMs: Date.now(),
          merge: "replace"
        }
      ]);
      setIsSending(false);
    }
  }, [ensureRuntimeAndSession, input, isSending, model]);

  const cancelPrompt = useCallback(async () => {
    if (!activeRunId) return;
    try {
      await agentRuntimeService.cancelRun(activeRunId);
      setActiveRunId(null);
      setIsSending(false);
    } catch (cancelError) {
      setError(errorMessage(cancelError));
    }
  }, [activeRunId]);

  const respondToPermission = useCallback(
    async (item: AgentTimelineItem, decision: AgentPermissionDecision) => {
      try {
        await agentRuntimeService.respondToPermission(permissionRequestId(item), decision);
        setTimelineItems((current) =>
          current.map((candidate) =>
            candidate.id === item.id ? { ...candidate, status: decision } : candidate
          )
        );
      } catch (permissionError) {
        setError(errorMessage(permissionError));
      }
    },
    []
  );

  const handleKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (event.key === "Enter" && !event.shiftKey && !isCompactLayout) {
        event.preventDefault();
        void sendMessage();
      }
    },
    [isCompactLayout, sendMessage]
  );

  const sidebarLayoutStyle = getSidebarLayoutStyle({ offsetContent: isSidebarOpen });

  const handleAgentEvent = useCallback(
    (event: AgentEventEnvelope) => {
      switch (event.eventType) {
        case "runtimeStatus":
          if (event.status) setRuntimeStatus(event.status);
          break;
        case "sessionCreated":
          if (event.session) {
            setSessions((current) => [
              event.session!,
              ...current.filter((session) => session.id !== event.session!.id)
            ]);
            setActiveSessionId(event.session.id);
          }
          break;
        case "runStarted":
          setIsSending(true);
          setActiveRunId(event.runId ?? null);
          break;
        case "timelineItem":
          if (event.sessionId && event.sessionId !== activeSessionIdRef.current) {
            setActiveSessionId(event.sessionId);
          }
          if (event.item) {
            setTimelineItems((current) => mergeTimelineItem(current, event.item!));
          }
          break;
        case "runFinished":
          setIsSending(false);
          setActiveRunId(null);
          void refreshSessions(projectRootRef.current).catch(() => {});
          break;
        case "error":
          setIsSending(false);
          setActiveRunId(null);
          if (event.message) setError(event.message);
          if (event.item) setTimelineItems((current) => mergeTimelineItem(current, event.item!));
          break;
        case "historyReplaced":
          void (async () => {
            const id = activeSessionIdRef.current;
            if (!id) return;
            const detail = await agentRuntimeService.loadSession(id);
            setTimelineItems(detail.timeline);
          })().catch((historyError) => setError(errorMessage(historyError)));
          break;
      }
    },
    [refreshSessions]
  );

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    void agentRuntimeService
      .listenToEvents((event) => {
        if (!cancelled) handleAgentEvent(event);
      })
      .then((nextUnlisten) => {
        if (cancelled) {
          nextUnlisten();
        } else {
          unlisten = nextUnlisten;
        }
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [handleAgentEvent]);

  if (!isTauriDesktop()) {
    return (
      <div className="flex h-dvh items-center justify-center bg-background p-6 text-center">
        <div className="max-w-sm space-y-3">
          <MapleWordmark className="mx-auto h-4 w-auto" />
          <p className="text-sm text-muted-foreground">Agent Mode is available in Maple Desktop.</p>
        </div>
      </div>
    );
  }

  return (
    <div
      style={sidebarLayoutStyle}
      className={cn(
        "grid h-dvh min-h-0 w-full grid-cols-1 overflow-hidden bg-background",
        isSidebarOpen ? SIDEBAR_GRID_COLUMNS_CLASS : ""
      )}
    >
      <Sidebar isOpen={isSidebarOpen} onToggle={toggleSidebar} />

      <div className="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
        {!isSidebarOpen && (
          <div className="fixed left-4 top-[9.5px] z-20">
            <SidebarToggle onToggle={toggleSidebar} />
          </div>
        )}

        <header className="h-14 shrink-0 border-b border-border/35" aria-label="Agent Mode" />

        {error && (
          <div className="mx-auto mt-3 w-full max-w-6xl px-4">
            <div className="flex items-start gap-2 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive">
              <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
              <span className="min-w-0 break-words">{error}</span>
            </div>
          </div>
        )}

        <main className="flex min-h-0 flex-1 overflow-hidden">
          <AgentProjectPanel
            activeSessionId={activeSessionId}
            projectRoot={projectRoot}
            recentRoots={recentRoots}
            runtimeRunning={runtimeStatus?.running ?? false}
            sessions={sessions}
            onChooseProjectRoot={chooseProjectRoot}
            onCreateSession={() => void createSession()}
            onProjectRootChange={setProjectRoot}
            onSessionSelect={(sessionId) => void loadSession(sessionId)}
          />

          <section className="flex min-w-0 flex-1 flex-col overflow-hidden">
            <div className="min-h-0 flex-1 overflow-y-auto px-4 py-5">
              <div className="mx-auto flex w-full max-w-4xl flex-col gap-4">
                {timelineItems.length === 0 ? (
                  <EmptyAgentState
                    activeRootLabel={activeRootLabel}
                    input={input}
                    isSending={isSending}
                    isStarting={isStarting}
                    model={model}
                    projectRoot={projectRoot}
                    recentRoots={recentRoots}
                    onCancelPrompt={cancelPrompt}
                    onChooseProjectRoot={chooseProjectRoot}
                    onInputChange={setInput}
                    onKeyDown={handleKeyDown}
                    onModelChange={setModel}
                    onProjectRootChange={setProjectRoot}
                    onRestartRuntime={() => void startRuntime(true)}
                    onSendMessage={() => void sendMessage()}
                  />
                ) : (
                  <>
                    <SessionHeader session={activeSession} />
                    <AgentTimeline
                      items={timelineItems}
                      onPermissionDecision={respondToPermission}
                    />
                  </>
                )}
                <div ref={messagesEndRef} />
              </div>
            </div>

            {timelineItems.length > 0 ? (
              <div className="shrink-0 border-t border-border/35 bg-background px-4 py-3">
                <div className="mx-auto max-w-4xl">
                  <AgentComposer
                    activeRootLabel={activeRootLabel}
                    input={input}
                    isSending={isSending}
                    isStarting={isStarting}
                    model={model}
                    projectRoot={projectRoot}
                    recentRoots={recentRoots}
                    onCancelPrompt={cancelPrompt}
                    onChooseProjectRoot={chooseProjectRoot}
                    onInputChange={setInput}
                    onKeyDown={handleKeyDown}
                    onModelChange={setModel}
                    onProjectRootChange={setProjectRoot}
                    onRestartRuntime={() => void startRuntime(true)}
                    onSendMessage={() => void sendMessage()}
                  />
                </div>
              </div>
            ) : null}
          </section>
        </main>
      </div>
    </div>
  );
}

function EmptyAgentState(props: AgentComposerProps) {
  return (
    <div className="flex min-h-[52vh] items-center justify-center">
      <div className="flex w-full max-w-2xl flex-col items-center gap-5 text-center">
        <h2 className="font-displayWide text-3xl font-normal brand-gradient-text">
          Work in a folder...
        </h2>
        <AgentComposer {...props} />
      </div>
    </div>
  );
}

interface AgentProjectPanelProps {
  activeSessionId: string | null;
  projectRoot: string;
  recentRoots: RecentProjectRoot[];
  runtimeRunning: boolean;
  sessions: AgentSessionSummary[];
  onChooseProjectRoot: () => void;
  onCreateSession: () => void;
  onProjectRootChange: (value: string) => void;
  onSessionSelect: (sessionId: string) => void;
}

function AgentProjectPanel({
  activeSessionId,
  projectRoot,
  recentRoots,
  runtimeRunning,
  sessions,
  onChooseProjectRoot,
  onCreateSession,
  onProjectRootChange,
  onSessionSelect
}: AgentProjectPanelProps) {
  return (
    <aside className="hidden w-72 shrink-0 flex-col overflow-hidden border-r border-border/35 bg-muted/20 lg:flex">
      <div className="border-b border-border/35 p-3">
        <div className="mb-2 flex items-center justify-between gap-2">
          <p className="truncate text-xs font-medium uppercase text-muted-foreground">Project</p>
          <Badge variant="outline" className="h-5 rounded-md px-1.5 text-[11px]">
            {runtimeRunning ? "running" : "stopped"}
          </Badge>
        </div>
        <div className="flex items-center gap-2">
          <Select value={projectRoot || undefined} onValueChange={onProjectRootChange}>
            <SelectTrigger className="h-8 min-w-0 flex-1 rounded-md border-border/45 bg-background px-2 text-xs">
              <SelectValue placeholder={projectRoot ? basename(projectRoot) : "Select folder"} />
            </SelectTrigger>
            <SelectContent>
              {recentRoots.map((root) => (
                <SelectItem key={root.path} value={root.path}>
                  {root.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button
            type="button"
            variant="outline"
            size="icon"
            className="h-8 w-8 shrink-0"
            onClick={onChooseProjectRoot}
            aria-label="Choose project folder"
          >
            <FolderOpen className="h-4 w-4" />
          </Button>
        </div>
        {projectRoot ? (
          <p className="mt-2 truncate text-[11px] text-muted-foreground">{projectRoot}</p>
        ) : null}
      </div>
      <div className="flex items-center justify-between gap-2 px-3 py-2">
        <p className="text-xs font-medium uppercase text-muted-foreground">Sessions</p>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-7 w-7"
          onClick={onCreateSession}
          disabled={!projectRoot}
          aria-label="New agent session"
        >
          <MessageSquarePlus className="h-4 w-4" />
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
        {sessions.length === 0 ? (
          <div className="rounded-md border border-border/35 bg-background/60 px-3 py-2 text-xs text-muted-foreground">
            No agent sessions yet.
          </div>
        ) : (
          <div className="space-y-1">
            {sessions.map((session) => (
              <button
                key={session.id}
                type="button"
                className={cn(
                  "w-full rounded-md px-2 py-2 text-left transition-colors",
                  session.id === activeSessionId
                    ? "bg-background text-foreground"
                    : "text-muted-foreground hover:bg-background/60 hover:text-foreground"
                )}
                onClick={() => onSessionSelect(session.id)}
              >
                <span className="block truncate text-sm">{sessionTitle(session)}</span>
                <span className="mt-0.5 block truncate text-[11px]">
                  {formatDate(session.updatedMs)}
                </span>
              </button>
            ))}
          </div>
        )}
      </div>
    </aside>
  );
}

function SessionHeader({ session }: { session: AgentSessionSummary | null }) {
  if (!session) return null;
  return (
    <div className="rounded-md border border-border/35 bg-muted/20 px-3 py-2">
      <div className="flex items-center justify-between gap-3">
        <div className="min-w-0">
          <p className="truncate text-sm font-medium">{sessionTitle(session)}</p>
          <p className="truncate text-xs text-muted-foreground">{session.projectRoot}</p>
        </div>
        <Badge variant="outline" className="shrink-0 rounded-md capitalize">
          {session.mode}
        </Badge>
      </div>
    </div>
  );
}

interface AgentComposerProps {
  activeRootLabel: string;
  input: string;
  isSending: boolean;
  isStarting: boolean;
  model: string;
  projectRoot: string;
  recentRoots: RecentProjectRoot[];
  onCancelPrompt: () => void;
  onChooseProjectRoot: () => void;
  onInputChange: (value: string) => void;
  onKeyDown: (event: React.KeyboardEvent<HTMLTextAreaElement>) => void;
  onModelChange: (value: string) => void;
  onProjectRootChange: (value: string) => void;
  onRestartRuntime: () => void;
  onSendMessage: () => void;
}

function AgentComposer({
  activeRootLabel,
  input,
  isSending,
  isStarting,
  model,
  projectRoot,
  recentRoots,
  onCancelPrompt,
  onChooseProjectRoot,
  onInputChange,
  onKeyDown,
  onModelChange,
  onProjectRootChange,
  onRestartRuntime,
  onSendMessage
}: AgentComposerProps) {
  return (
    <div className="w-full rounded-lg border border-border/45 bg-background shadow-sm">
      <div className="flex flex-wrap items-center gap-2 border-b border-border/35 px-2 py-2">
        <Select value={projectRoot || undefined} onValueChange={onProjectRootChange}>
          <SelectTrigger className="h-8 min-w-[12rem] flex-1 rounded-md border-0 bg-muted/60 px-2">
            <FolderOpen className="mr-2 h-4 w-4 shrink-0" />
            <SelectValue placeholder={activeRootLabel} />
          </SelectTrigger>
          <SelectContent>
            {recentRoots.map((root) => (
              <SelectItem key={root.path} value={root.path}>
                {root.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Button
          type="button"
          variant="outline"
          size="icon"
          className="h-8 w-8 shrink-0"
          onClick={onChooseProjectRoot}
          aria-label="Choose project folder"
        >
          <FolderOpen className="h-4 w-4" />
        </Button>
        <Input
          value={model}
          onChange={(event) => onModelChange(event.target.value)}
          className="h-8 w-[11rem] rounded-md border-0 bg-muted/60 text-xs"
          aria-label="Agent model"
        />
        <Button
          type="button"
          variant="outline"
          size="icon"
          className="h-8 w-8 shrink-0"
          onClick={onRestartRuntime}
          disabled={isStarting || !projectRoot}
          aria-label="Restart agent runtime"
        >
          {isStarting ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <RotateCcw className="h-4 w-4" />
          )}
        </Button>
      </div>
      <div className="flex items-end gap-2 p-2">
        <Textarea
          id="agent-message"
          value={input}
          onChange={(event) => onInputChange(event.target.value)}
          onKeyDown={onKeyDown}
          placeholder="Ask Goose to work in this folder..."
          className="max-h-44 min-h-[4.5rem] resize-none rounded-md border-0 bg-transparent px-2 py-2 focus-visible:ring-0 focus-visible:ring-offset-0"
        />
        {isSending ? (
          <Button
            type="button"
            size="icon"
            variant="outline"
            className="h-9 w-9 shrink-0"
            onClick={onCancelPrompt}
            aria-label="Cancel prompt"
          >
            <Square className="h-3.5 w-3.5" />
          </Button>
        ) : (
          <Button
            type="button"
            size="icon"
            className="h-9 w-9 shrink-0"
            onClick={onSendMessage}
            disabled={!input.trim() || isStarting || !projectRoot}
            aria-label="Send agent message"
          >
            {isStarting ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Send className="h-4 w-4" />
            )}
          </Button>
        )}
      </div>
    </div>
  );
}

function AgentTimeline({
  items,
  onPermissionDecision
}: {
  items: AgentTimelineItem[];
  onPermissionDecision: (item: AgentTimelineItem, decision: AgentPermissionDecision) => void;
}) {
  const visibleItems = items.filter(isRenderableTimelineItem);
  return (
    <div className="flex flex-col gap-3">
      {visibleItems.map((item) => {
        if (item.itemType === "message") return <MessageBubble key={item.id} item={item} />;
        if (item.itemType === "thinking") return <ThinkingMessage key={item.id} item={item} />;
        if (item.itemType === "tool") return <ToolCallRow key={item.id} item={item} />;
        if (item.itemType === "usage") return <UsageRow key={item.id} item={item} />;
        if (item.itemType === "permission") {
          return (
            <PermissionRow key={item.id} item={item} onPermissionDecision={onPermissionDecision} />
          );
        }
        return <SystemRow key={item.id} item={item} />;
      })}
    </div>
  );
}

function MessageBubble({ item }: { item: AgentTimelineItem }) {
  const role = item.role || "assistant";
  const text = item.text || "";
  if (!text.trim()) return null;
  return (
    <div className={cn("flex", role === "user" ? "justify-end" : "justify-start")}>
      <div
        className={cn(
          "max-w-[82%] rounded-lg px-3 py-2 text-sm leading-6",
          role === "user"
            ? "bg-[hsl(var(--maple-primary))] text-primary-foreground"
            : "border border-border/45 bg-muted/45 text-foreground"
        )}
      >
        {role === "assistant" ? (
          <Markdown content={text} />
        ) : (
          <div className="whitespace-pre-wrap break-words">{text}</div>
        )}
      </div>
    </div>
  );
}

function ThinkingMessage({ item }: { item: AgentTimelineItem }) {
  return (
    <details className="group rounded-lg border border-border/35 bg-muted/25 px-3 py-2 text-sm text-muted-foreground">
      <summary className="flex cursor-pointer list-none items-center gap-2 font-medium text-foreground/80">
        <ChevronRight className="h-4 w-4 transition-transform group-open:rotate-90" />
        Thinking
      </summary>
      <div className="mt-2 whitespace-pre-wrap break-words text-xs leading-5">
        {item.text || ""}
      </div>
    </details>
  );
}

function ToolCallRow({ item }: { item: AgentTimelineItem }) {
  const failed = item.status === "failed";
  return (
    <details
      open={failed}
      className={cn(
        "group rounded-lg border bg-muted/30 px-3 py-2",
        failed ? "border-destructive/35" : "border-border/45"
      )}
    >
      <summary className="flex cursor-pointer list-none items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-2">
          <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground transition-transform group-open:rotate-90" />
          <Terminal className="h-4 w-4 shrink-0 text-muted-foreground" />
          <span className="truncate text-sm font-medium">{toolTitle(item)}</span>
        </div>
        <Badge
          variant="secondary"
          className={cn(
            "shrink-0 capitalize",
            failed && "border-destructive/40 bg-destructive/15 text-destructive"
          )}
        >
          {formatStatus(item.status || "running")}
        </Badge>
      </summary>
      <div className="mt-2 space-y-2 pl-6">
        {item.text ? <ToolDetail label="Summary" value={item.text} /> : null}
        {item.input !== undefined ? (
          <ToolDetail label="Input" value={formatUnknown(item.input)} />
        ) : null}
        {item.output !== undefined ? (
          <ToolDetail label="Output" value={formatUnknown(item.output)} />
        ) : null}
        {item.raw !== undefined ? <ToolDetail label="Raw" value={formatUnknown(item.raw)} /> : null}
      </div>
    </details>
  );
}

function PermissionRow({
  item,
  onPermissionDecision
}: {
  item: AgentTimelineItem;
  onPermissionDecision: (item: AgentTimelineItem, decision: AgentPermissionDecision) => void;
}) {
  const resolved = Boolean(item.status && item.status !== "pending");
  return (
    <div
      className={cn(
        "rounded-lg border px-3 py-3",
        resolved
          ? "border-border/45 bg-muted/20"
          : "border-[hsl(var(--maple-primary)/0.45)] bg-[hsl(var(--maple-primary)/0.08)]"
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <p className="text-sm font-medium">{item.title || "Permission requested"}</p>
          {item.text ? (
            <p className="mt-1 whitespace-pre-wrap break-words text-xs text-muted-foreground">
              {item.text}
            </p>
          ) : null}
        </div>
        <Badge variant={resolved ? "secondary" : "outline"} className="shrink-0 rounded-md">
          {formatPermissionStatus(item.status || "pending")}
        </Badge>
      </div>
      {item.input !== undefined ? (
        <div className="mt-2">
          <ToolDetail label="Input" value={formatUnknown(item.input)} />
        </div>
      ) : null}
      {!resolved ? (
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <Button
            size="sm"
            className="h-8"
            onClick={() => onPermissionDecision(item, "allow_once")}
          >
            <Check className="mr-1 h-4 w-4" />
            Allow once
          </Button>
          <Button
            size="sm"
            variant="outline"
            className="h-8"
            onClick={() => onPermissionDecision(item, "deny_once")}
          >
            Deny
          </Button>
          <Button
            size="sm"
            variant="ghost"
            className="h-8"
            onClick={() => onPermissionDecision(item, "cancel")}
          >
            <X className="mr-1 h-4 w-4" />
            Cancel
          </Button>
        </div>
      ) : null}
    </div>
  );
}

function UsageRow({ item }: { item: AgentTimelineItem }) {
  const usage = summarizeUsage(item.raw);
  if (!usage) return null;

  return (
    <div className="flex justify-end">
      <div className="rounded-md border border-border/35 bg-muted/20 px-2 py-1 text-[11px] text-muted-foreground">
        {usage}
      </div>
    </div>
  );
}

function SystemRow({ item }: { item: AgentTimelineItem }) {
  const failed = item.itemType === "error" || item.status === "failed";
  return (
    <div
      className={cn(
        "rounded-lg border px-3 py-2 text-sm",
        failed
          ? "border-destructive/35 bg-destructive/5 text-destructive"
          : "border-border/35 bg-muted/25 text-muted-foreground"
      )}
    >
      <div className="flex items-start gap-2">
        {failed ? (
          <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
        ) : (
          <Circle className="mt-1 h-3 w-3 shrink-0" />
        )}
        <div className="min-w-0">
          <p className="font-medium text-foreground">{item.title || "Agent event"}</p>
          {item.text ? <p className="mt-1 whitespace-pre-wrap break-words">{item.text}</p> : null}
          {item.raw !== undefined ? (
            <div className="mt-2">
              <ToolDetail label="Raw" value={formatUnknown(item.raw)} />
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}

function ToolDetail({ label, value }: { label: string; value: string }) {
  if (!value.trim()) return null;
  return (
    <div>
      <p className="mb-1 text-[11px] font-medium uppercase text-muted-foreground">{label}</p>
      <pre className="max-h-56 overflow-auto whitespace-pre-wrap break-words rounded-md bg-background/70 px-2 py-1.5 text-xs text-muted-foreground">
        {value}
      </pre>
    </div>
  );
}

function mergeTimelineItem(
  current: AgentTimelineItem[],
  incoming: AgentTimelineItem
): AgentTimelineItem[] {
  const index = current.findIndex((item) => item.id === incoming.id);
  if (index < 0) return [...current, incoming];

  const next = [...current];
  const previous = next[index];
  const appendText =
    incoming.merge === "append" &&
    (incoming.itemType === "message" || incoming.itemType === "thinking") &&
    incoming.text;

  next[index] = {
    ...previous,
    ...incoming,
    title: incoming.title ?? previous.title,
    input: incoming.input ?? previous.input,
    output: incoming.output ?? previous.output,
    raw: incoming.raw ?? previous.raw,
    text: appendText
      ? `${previous.text || ""}${incoming.text || ""}`
      : (incoming.text ?? previous.text)
  };

  return next;
}

function isRenderableTimelineItem(item: AgentTimelineItem): boolean {
  if (item.itemType === "message") return Boolean(item.text?.trim());
  if (item.itemType === "usage") return Boolean(summarizeUsage(item.raw));
  if (item.itemType === "system" || item.itemType === "error") {
    return Boolean(item.title?.trim() || item.text?.trim() || item.raw !== undefined);
  }
  return true;
}

function permissionRequestId(item: AgentTimelineItem): string {
  return item.id.startsWith("permission-") ? item.id.slice("permission-".length) : item.id;
}

function sessionTitle(session: AgentSessionSummary): string {
  return session.title || "Agent session";
}

function toolTitle(item: AgentTimelineItem): string {
  return item.title || "Tool call";
}

function formatUnknown(value: unknown): string {
  if (value === undefined || value === null) return "";
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function formatStatus(status: string): string {
  return status.replace(/_/g, " ").replace(/\b\w/g, (letter) => letter.toUpperCase());
}

function formatPermissionStatus(status: string): string {
  switch (status) {
    case "allow_once":
      return "Allowed once";
    case "always_allow":
      return "Always allowed";
    case "deny_once":
      return "Denied";
    case "always_deny":
      return "Always denied";
    case "cancel":
      return "Cancelled";
    default:
      return formatStatus(status);
  }
}

function summarizeUsage(raw: unknown): string | null {
  if (!raw || typeof raw !== "object") return null;
  const value = raw as {
    model?: unknown;
    usage?: {
      input_tokens?: unknown;
      output_tokens?: unknown;
      total_tokens?: unknown;
    };
  };
  const usage = value.usage;
  if (!usage || typeof usage !== "object") return null;

  const inputTokens = typeof usage.input_tokens === "number" ? usage.input_tokens : null;
  const outputTokens = typeof usage.output_tokens === "number" ? usage.output_tokens : null;
  const totalTokens = typeof usage.total_tokens === "number" ? usage.total_tokens : null;
  const model = typeof value.model === "string" ? value.model : null;
  const parts = [
    model,
    totalTokens !== null ? `${totalTokens.toLocaleString()} tokens` : null,
    inputTokens !== null && outputTokens !== null
      ? `${inputTokens.toLocaleString()} in / ${outputTokens.toLocaleString()} out`
      : null
  ].filter(Boolean);

  return parts.length ? parts.join(" · ") : null;
}

function basename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() || path;
}

function formatDate(ms: number): string {
  if (!ms) return "";
  return new Date(ms).toLocaleString([], {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit"
  });
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return "Agent Mode failed";
}
