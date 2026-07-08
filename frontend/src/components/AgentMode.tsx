import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useOpenSecret } from "@opensecret/react";
import {
  AlertCircle,
  ArrowUp,
  Brain,
  Camera,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Circle,
  FolderOpen,
  Loader2,
  Lock,
  MessageSquarePlus,
  RotateCcw,
  Terminal,
  X,
  Zap
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
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger
} from "@/components/ui/dropdown-menu";
import { Sidebar, SidebarToggle } from "@/components/Sidebar";
import { MapleWordmark } from "@/components/MapleWordmark";
import { UpgradePromptDialog } from "@/components/UpgradePromptDialog";
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
import {
  cn,
  POWERFUL_MODEL_ALIAS,
  QUICK_MODEL_ALIAS,
  useIsLandscapeMobile,
  useIsMobile
} from "@/utils/utils";
import { isTauriDesktop } from "@/utils/platform";
import { useLocalState } from "@/state/useLocalState";
import type {
  ModelAccessTier,
  OpenSecretModel,
  OpenSecretModelAlias,
  OpenSecretModelCatalog
} from "@/state/LocalStateContextDef";

const DEFAULT_MODEL = POWERFUL_MODEL_ALIAS;
const DEFAULT_MODE = "smart_approve";
const AUTO_SCROLL_BOTTOM_THRESHOLD_PX = 100;

const PRIMARY_AGENT_MODELS = [
  {
    id: QUICK_MODEL_ALIAS,
    label: "Quick",
    icon: Zap,
    description: "Fast, everyday responses",
    access: "free" as ModelAccessTier,
    capabilities: { vision: false, reasoning: true }
  },
  {
    id: POWERFUL_MODEL_ALIAS,
    label: "Powerful",
    icon: Brain,
    description: "Deeper thinking & analysis",
    access: "pro" as ModelAccessTier,
    capabilities: { vision: true, reasoning: true }
  }
] as const;

const FALLBACK_ALIAS_TARGETS = {
  [QUICK_MODEL_ALIAS]: "gpt-oss-120b",
  [POWERFUL_MODEL_ALIAS]: "kimi-k2-6"
} as const;

type ModelCatalogClient = {
  fetchModelCatalog?: () => Promise<OpenSecretModelCatalog>;
  fetchModels?: () => Promise<OpenSecretModel[]>;
};

function isAutoModelAlias(modelId: string): boolean {
  return modelId === QUICK_MODEL_ALIAS || modelId === POWERFUL_MODEL_ALIAS;
}

function isSelectableChatModel(model: OpenSecretModel): boolean {
  return model.enabled !== false && model.deprecated !== true && model.capabilities?.chat !== false;
}

function buildFallbackModelAliases(models: OpenSecretModel[]): OpenSecretModelAlias[] {
  const modelById = new Map(models.map((availableModel) => [availableModel.id, availableModel]));

  return PRIMARY_AGENT_MODELS.map((primaryModel) => {
    const targetModel = modelById.get(FALLBACK_ALIAS_TARGETS[primaryModel.id]);

    return {
      id: primaryModel.id,
      label: primaryModel.label,
      short_name: primaryModel.label,
      description: primaryModel.description,
      target_model: targetModel?.id || "",
      access: targetModel?.access || primaryModel.access,
      capabilities: targetModel?.capabilities || primaryModel.capabilities
    };
  });
}

export function AgentMode() {
  const { createApiKey } = useOpenSecret();
  const isMobile = useIsMobile();
  const isLandscapeMobile = useIsLandscapeMobile();
  const isCompactLayout = isMobile || isLandscapeMobile;
  const [isSidebarOpen, setIsSidebarOpen] = useState(!isCompactLayout);
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
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [activeRunsBySession, setActiveRunsBySession] = useState<Record<string, string>>({});
  const chatContainerRef = useRef<HTMLDivElement>(null);
  const activeSessionIdRef = useRef(activeSessionId);
  const shouldAutoScrollRef = useRef(true);

  useEffect(() => {
    if (isCompactLayout) {
      setIsSidebarOpen(false);
    }
  }, [isCompactLayout]);

  useEffect(() => {
    activeSessionIdRef.current = activeSessionId;
  }, [activeSessionId]);

  const updateAutoScrollFromPosition = useCallback(() => {
    const container = chatContainerRef.current;
    if (!container) return;

    const distanceFromBottom =
      container.scrollHeight - container.scrollTop - container.clientHeight;
    shouldAutoScrollRef.current = distanceFromBottom < AUTO_SCROLL_BOTTOM_THRESHOLD_PX;
  }, []);

  const scrollTimelineToBottom = useCallback((behavior: ScrollBehavior = "auto") => {
    const container = chatContainerRef.current;
    if (!container) return;

    container.scrollTo({
      top: container.scrollHeight,
      behavior
    });
  }, []);

  useEffect(() => {
    if (!shouldAutoScrollRef.current) return;

    const frame = requestAnimationFrame(() => {
      if (shouldAutoScrollRef.current) {
        scrollTimelineToBottom("auto");
      }
    });

    return () => cancelAnimationFrame(frame);
  }, [scrollTimelineToBottom, timelineItems]);

  const activeRootLabel = useMemo(() => {
    if (!projectRoot) return "Select folder";
    return recentRoots.find((root) => root.path === projectRoot)?.name || basename(projectRoot);
  }, [projectRoot, recentRoots]);
  const activeRunId = activeSessionId ? (activeRunsBySession[activeSessionId] ?? null) : null;
  const isSending = Boolean(activeRunId) || isSubmitting;
  const runningSessionIds = useMemo(
    () => new Set(Object.keys(activeRunsBySession)),
    [activeRunsBySession]
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

  const refreshSessions = useCallback(async () => {
    if (!isTauriDesktop()) return;
    const status = await agentRuntimeService.getRuntimeStatus();
    setRuntimeStatus(status);
    if (!status.running) {
      setSessions([]);
      return;
    }
    const nextSessions = await agentRuntimeService.listSessions(null);
    setSessions(nextSessions);
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
          await refreshSessions();
        } else if (root) {
          const startedStatus = await agentRuntimeService.startRuntime({
            projectRoot: root,
            model: nextModel,
            mode: DEFAULT_MODE
          });
          if (cancelled) return;
          setRuntimeStatus(startedStatus);
          await refreshSessions();
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
        shouldAutoScrollRef.current = true;
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

  const selectProjectRoot = useCallback(
    (value: string) => {
      setProjectRoot(value);
      setActiveSessionId(null);
      activeSessionIdRef.current = null;
      setTimelineItems([]);
      shouldAutoScrollRef.current = true;
      void refreshSessions().catch((selectError) => setError(errorMessage(selectError)));
    },
    [refreshSessions]
  );

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
        await refreshSessions();
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

    const status = await agentRuntimeService.getRuntimeStatus();
    if (!status.running) {
      await startRuntime(false);
    }

    let sessionId = activeSessionIdRef.current;
    if (!sessionId) {
      const detail = await agentRuntimeService.createSession({
        projectRoot,
        title: "New agent session",
        model: model || DEFAULT_MODEL,
        mode: DEFAULT_MODE
      });
      shouldAutoScrollRef.current = true;
      sessionId = detail.session.id;
      activeSessionIdRef.current = sessionId;
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
      shouldAutoScrollRef.current = true;
      activeSessionIdRef.current = detail.session.id;
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
      shouldAutoScrollRef.current = true;
      activeSessionIdRef.current = detail.session.id;
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
    setIsSubmitting(true);
    shouldAutoScrollRef.current = true;
    requestAnimationFrame(() => scrollTimelineToBottom("smooth"));
    try {
      const sessionId = await ensureRuntimeAndSession();
      const response = await agentRuntimeService.sendMessage({
        sessionId,
        text,
        model: model || DEFAULT_MODEL,
        mode: DEFAULT_MODE
      });
      setActiveRunsBySession((current) => ({
        ...current,
        [sessionId]: response.runId
      }));
      setIsSubmitting(false);
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
      setIsSubmitting(false);
    }
  }, [ensureRuntimeAndSession, input, isSending, model, scrollTimelineToBottom]);

  const cancelPrompt = useCallback(async () => {
    if (!activeRunId) return;
    try {
      await agentRuntimeService.cancelRun(activeRunId);
      if (activeSessionIdRef.current) {
        setActiveRunsBySession((current) => {
          const next = { ...current };
          delete next[activeSessionIdRef.current!];
          return next;
        });
      }
      setIsSubmitting(false);
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
          }
          break;
        case "runStarted":
          if (event.sessionId && event.runId) {
            setActiveRunsBySession((current) => ({
              ...current,
              [event.sessionId!]: event.runId!
            }));
          }
          setIsSubmitting(false);
          break;
        case "timelineItem":
          if (event.item && event.sessionId === activeSessionIdRef.current) {
            setTimelineItems((current) => mergeTimelineItem(current, event.item!));
          }
          break;
        case "runFinished":
          if (event.sessionId) {
            setActiveRunsBySession((current) => {
              const next = { ...current };
              delete next[event.sessionId!];
              return next;
            });
          }
          setIsSubmitting(false);
          void refreshSessions().catch(() => {});
          break;
        case "error":
          if (event.sessionId) {
            setActiveRunsBySession((current) => {
              const next = { ...current };
              delete next[event.sessionId!];
              return next;
            });
          }
          setIsSubmitting(false);
          if (event.message) setError(event.message);
          if (event.item && event.sessionId === activeSessionIdRef.current) {
            setTimelineItems((current) => mergeTimelineItem(current, event.item!));
          }
          break;
        case "historyReplaced":
          void (async () => {
            const id = event.sessionId || activeSessionIdRef.current;
            if (event.sessionId && event.sessionId !== activeSessionIdRef.current) return;
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
      <Sidebar
        isOpen={isSidebarOpen}
        mode="agent"
        navigationContent={
          <AgentSidebarContent
            activeSessionId={activeSessionId}
            projectRoot={projectRoot}
            recentRoots={recentRoots}
            runtimeRunning={runtimeStatus?.running ?? false}
            runningSessionIds={runningSessionIds}
            sessions={sessions}
            onChooseProjectRoot={chooseProjectRoot}
            onCreateSession={() => void createSession()}
            onProjectRootChange={selectProjectRoot}
            onSessionSelect={(sessionId) => void loadSession(sessionId)}
          />
        }
        onToggle={toggleSidebar}
      />

      <div className="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
        {!isSidebarOpen && (
          <div className="fixed left-4 top-[9.5px] z-20 flex items-center gap-1.5">
            <SidebarToggle onToggle={toggleSidebar} />
            <MapleWordmark
              className="h-4 w-auto animate-in fade-in-0 slide-in-from-left-1 duration-300"
              aria-hidden
            />
          </div>
        )}

        {error && (
          <div className="mx-auto mt-3 w-full max-w-6xl px-4">
            <div className="flex items-start gap-2 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive">
              <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
              <span className="min-w-0 break-words">{error}</span>
            </div>
          </div>
        )}

        <section className="flex min-h-0 flex-1 flex-col overflow-hidden">
          <div
            ref={chatContainerRef}
            className="min-h-0 flex-1 overflow-y-auto px-4 py-5"
            onScroll={updateAutoScrollFromPosition}
          >
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
                  onProjectRootChange={selectProjectRoot}
                  onRestartRuntime={() => void startRuntime(true)}
                  onSendMessage={() => void sendMessage()}
                />
              ) : (
                <AgentTimeline items={timelineItems} onPermissionDecision={respondToPermission} />
              )}
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
                  onProjectRootChange={selectProjectRoot}
                  onRestartRuntime={() => void startRuntime(true)}
                  onSendMessage={() => void sendMessage()}
                />
              </div>
            </div>
          ) : null}
        </section>
      </div>
    </div>
  );
}

function EmptyAgentState(props: AgentComposerProps) {
  return (
    <div className="flex min-h-[52vh] items-center justify-center">
      <div className="flex w-full max-w-4xl flex-col items-center gap-5 text-center">
        <h2 className="font-displayWide text-3xl font-normal brand-gradient-text">
          Work in a folder...
        </h2>
        <AgentComposer {...props} />
      </div>
    </div>
  );
}

interface AgentSidebarContentProps {
  activeSessionId: string | null;
  projectRoot: string;
  recentRoots: RecentProjectRoot[];
  runtimeRunning: boolean;
  runningSessionIds: Set<string>;
  sessions: AgentSessionSummary[];
  onChooseProjectRoot: () => void;
  onCreateSession: () => void;
  onProjectRootChange: (value: string) => void;
  onSessionSelect: (sessionId: string) => void;
}

function AgentSidebarContent({
  activeSessionId,
  projectRoot,
  recentRoots,
  runtimeRunning,
  runningSessionIds,
  sessions,
  onChooseProjectRoot,
  onCreateSession,
  onProjectRootChange,
  onSessionSelect
}: AgentSidebarContentProps) {
  const { projectRows, sessionsByRoot } = useMemo(() => {
    const rootsByPath = new Map<string, RecentProjectRoot>();
    const sessionsByProjectRoot = new Map<string, AgentSessionSummary[]>();

    recentRoots.forEach((root) => rootsByPath.set(root.path, root));
    if (projectRoot && !rootsByPath.has(projectRoot)) {
      rootsByPath.set(projectRoot, {
        path: projectRoot,
        name: basename(projectRoot),
        lastUsedMs: Date.now()
      });
    }

    sessions.forEach((session) => {
      const rootSessions = sessionsByProjectRoot.get(session.projectRoot) || [];
      rootSessions.push(session);
      sessionsByProjectRoot.set(session.projectRoot, rootSessions);

      const existingRoot = rootsByPath.get(session.projectRoot);
      if (!existingRoot || existingRoot.lastUsedMs < session.updatedMs) {
        rootsByPath.set(session.projectRoot, {
          path: session.projectRoot,
          name: basename(session.projectRoot),
          lastUsedMs: session.updatedMs
        });
      }
    });

    sessionsByProjectRoot.forEach((rootSessions) => {
      rootSessions.sort((a, b) => b.updatedMs - a.updatedMs);
    });

    const rows = [...rootsByPath.values()].sort((a, b) => {
      if (a.path === projectRoot) return -1;
      if (b.path === projectRoot) return 1;
      return b.lastUsedMs - a.lastUsedMs;
    });

    return { projectRows: rows, sessionsByRoot: sessionsByProjectRoot };
  }, [projectRoot, recentRoots, sessions]);

  return (
    <>
      <div className="mb-4 flex items-center justify-between gap-3">
        <p className="text-sm font-medium text-muted-foreground">Projects</p>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-8 w-8 text-muted-foreground hover:bg-background/60 hover:text-foreground"
          onClick={onChooseProjectRoot}
          aria-label="Choose project folder"
        >
          <FolderOpen className="h-4 w-4" />
        </Button>
      </div>

      {projectRows.length === 0 ? (
        <button
          type="button"
          className="flex w-full items-center gap-3 rounded-md px-2 py-2 text-left text-sm text-muted-foreground transition-colors hover:bg-background/60 hover:text-foreground"
          onClick={onChooseProjectRoot}
        >
          <FolderOpen className="h-4 w-4 shrink-0" />
          Select a folder
        </button>
      ) : (
        <div className="space-y-1">
            {projectRows.map((root) => {
              const isActive = root.path === projectRoot;
              const projectSessions = sessionsByRoot.get(root.path) || [];
              const hasRunningSession = projectSessions.some((session) =>
                runningSessionIds.has(session.id)
              );

              return (
                <div key={root.path} className="space-y-1.5">
                  <div
                    className={cn(
                      "flex items-center gap-1 rounded-md transition-colors",
                      isActive ? "bg-background/70 text-foreground" : "text-muted-foreground"
                    )}
                  >
                    <button
                      type="button"
                      className={cn(
                        "flex min-w-0 flex-1 items-center gap-3 rounded-md px-2 py-2 text-left text-sm transition-colors",
                        !isActive && "hover:bg-background/60 hover:text-foreground"
                      )}
                      onClick={() => onProjectRootChange(root.path)}
                      title={root.path}
                    >
                      <FolderOpen className="h-4 w-4 shrink-0" />
                      <span className="min-w-0 flex-1 truncate">{root.name}</span>
                      {hasRunningSession ? (
                        <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin text-[hsl(var(--maple-primary))]" />
                      ) : isActive && runtimeRunning ? (
                        <span className="h-2 w-2 shrink-0 rounded-full bg-maple-success" />
                      ) : null}
                      {isActive ? <ChevronDown className="h-4 w-4 shrink-0" /> : null}
                    </button>
                    {isActive ? (
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        className="mr-1 h-7 w-7 shrink-0 text-muted-foreground hover:bg-background/70 hover:text-foreground"
                        onClick={onCreateSession}
                        disabled={!projectRoot}
                        aria-label="New agent session"
                      >
                        <MessageSquarePlus className="h-4 w-4" />
                      </Button>
                    ) : null}
                  </div>

                  <div className="space-y-0.5 pl-9">
                    {projectSessions.length === 0 ? (
                      isActive ? (
                        <p className="px-2 py-1 text-xs text-muted-foreground/75">
                          No sessions yet
                        </p>
                      ) : null
                    ) : (
                      projectSessions.slice(0, 6).map((session) => {
                        const isRunning = runningSessionIds.has(session.id);

                        return (
                          <button
                            key={session.id}
                            type="button"
                            className={cn(
                              "grid w-full grid-cols-[minmax(0,1fr)_auto] items-center gap-3 rounded-md px-2 py-1.5 text-left transition-colors",
                              session.id === activeSessionId
                                ? "bg-background/80 text-foreground"
                                : "text-muted-foreground hover:bg-background/60 hover:text-foreground"
                            )}
                            onClick={() => onSessionSelect(session.id)}
                          >
                            <span className="truncate text-sm">{sessionTitle(session)}</span>
                            <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
                              {isRunning ? (
                                <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-[hsl(var(--maple-primary))]" />
                              ) : null}
                              {formatDate(session.updatedMs)}
                            </span>
                          </button>
                        );
                      })
                    )}
                    {projectSessions.length > 6 ? (
                      <p className="px-2 py-1 text-xs text-muted-foreground/75">
                        {projectSessions.length - 6} more
                      </p>
                    ) : null}
                  </div>
                </div>
              );
            })}
        </div>
      )}

      <div className="mt-7">
        <p className="mb-3 text-sm font-medium text-muted-foreground">Chats</p>
        <p className="px-2 text-xs text-muted-foreground/75">
          Folderless agent chats are not available yet.
        </p>
      </div>
    </>
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
  const rootOptions = recentRoots.some((root) => root.path === projectRoot)
    ? recentRoots
    : projectRoot
      ? [{ path: projectRoot, name: activeRootLabel, lastUsedMs: Date.now() }, ...recentRoots]
      : recentRoots;

  return (
    <div className="relative w-full overflow-hidden rounded-3xl border border-[hsl(var(--maple-secondary-container))] bg-background transition-colors focus-within:border-[hsl(var(--maple-primary))]">
      <Textarea
        id="agent-message"
        value={input}
        onChange={(event) => onInputChange(event.target.value)}
        onKeyDown={onKeyDown}
        placeholder="Ask Goose to work in this folder..."
        className="w-full max-h-[200px] min-h-[52px] resize-none border-0 bg-transparent py-3.5 pl-4 pr-2 leading-6 focus-visible:ring-0 focus-visible:ring-offset-0 placeholder:text-muted-foreground/60"
        rows={1}
      />

      <div className="grid grid-cols-[minmax(0,1fr)_auto] items-end gap-x-2 gap-y-2 px-2 pb-2 pt-1">
        <div className="flex min-w-0 flex-wrap items-center gap-1.5 sm:gap-2">
          <AgentModelSelector model={model} onModelChange={onModelChange} />

          <Select value={projectRoot || undefined} onValueChange={onProjectRootChange}>
            <SelectTrigger className="h-8 w-auto max-w-[12rem] gap-1 border-0 bg-transparent px-2 text-[hsl(var(--maple-secondary-700))] hover:bg-[hsl(var(--maple-primary-container))] hover:text-[hsl(var(--maple-secondary-700))] focus:ring-0 focus:ring-offset-0">
              <FolderOpen className="h-4 w-4 shrink-0" />
              <SelectValue placeholder={activeRootLabel} />
            </SelectTrigger>
            <SelectContent>
              {rootOptions.map((root) => (
                <SelectItem key={root.path} value={root.path}>
                  {root.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>

          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-8 w-8 p-0 text-[hsl(var(--maple-secondary-700))] hover:bg-[hsl(var(--maple-primary-container))] hover:text-[hsl(var(--maple-secondary-700))]"
            onClick={onChooseProjectRoot}
            aria-label="Choose project folder"
          >
            <FolderOpen className="h-4 w-4" />
          </Button>

          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-8 w-8 p-0 text-[hsl(var(--maple-secondary-700))] hover:bg-[hsl(var(--maple-primary-container))] hover:text-[hsl(var(--maple-secondary-700))]"
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

        <div className="flex shrink-0 items-center self-end gap-1.5 sm:gap-2">
          {isSending ? (
            <Button
              type="button"
              size="icon"
              variant="destructive"
              className="h-8 w-8 rounded-xl"
              onClick={onCancelPrompt}
              aria-label="Cancel prompt"
            >
              <div className="h-3 w-3 rounded-md bg-current" />
            </Button>
          ) : (
            <button
              type="button"
              className="flex h-8 w-8 items-center justify-center rounded-full bg-gradient-to-b from-[hsl(var(--maple-primary))] to-[hsl(var(--maple-primary-strong))] text-[hsl(var(--maple-on-primary))]/90 transition-all duration-200 ease-out active:scale-[0.95] disabled:pointer-events-none disabled:opacity-40"
              onClick={onSendMessage}
              disabled={!input.trim() || isStarting || !projectRoot}
              aria-label="Send agent message"
            >
              {isStarting ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <ArrowUp className="h-4 w-4" />
              )}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function AgentModelSelector({
  model,
  onModelChange
}: {
  model: string;
  onModelChange: (value: string) => void;
}) {
  const {
    availableModels,
    setAvailableModels,
    modelAliases,
    setModelAliases,
    billingStatus,
    setHasWhisperModel
  } = useLocalState();
  const os = useOpenSecret();
  const isFetching = useRef(false);
  const hasFetched = useRef(false);
  const currentModelRef = useRef(model);
  const [upgradeDialogOpen, setUpgradeDialogOpen] = useState(false);
  const [selectedModelName, setSelectedModelName] = useState("");
  const [showAdvanced, setShowAdvanced] = useState(false);

  useEffect(() => {
    currentModelRef.current = model;
  }, [model]);

  const modelById = useMemo(() => {
    return new Map(availableModels.map((availableModel) => [availableModel.id, availableModel]));
  }, [availableModels]);

  const aliasById = useMemo(() => {
    return new Map(modelAliases.map((alias) => [alias.id, alias]));
  }, [modelAliases]);

  const reconcileSelectedConcreteModel = useCallback(
    (models: OpenSecretModel[]) => {
      const currentModel = currentModelRef.current;
      if (!currentModel || isAutoModelAlias(currentModel)) return;

      const selectedModel = models.find((availableModel) => availableModel.id === currentModel);
      if (!selectedModel) {
        onModelChange(DEFAULT_MODEL);
      }
    },
    [onModelChange]
  );

  const fetchCatalog = useCallback(async () => {
    if (hasFetched.current || isFetching.current) return;

    if (availableModels.length > 0 && modelAliases.length > 0) {
      hasFetched.current = true;
      return;
    }

    isFetching.current = true;

    try {
      const modelClient = os as unknown as ModelCatalogClient;

      if (modelClient.fetchModelCatalog) {
        try {
          const catalog = await modelClient.fetchModelCatalog();
          const selectableModels = catalog.data.filter(isSelectableChatModel);
          const hasCatalogWhisperModel = catalog.data.some(
            (catalogModel) => catalogModel.id === "whisper-large-v3"
          );
          hasFetched.current = true;
          setAvailableModels(selectableModels);
          setModelAliases(catalog.aliases);
          setHasWhisperModel(catalog.audio?.transcription?.available ?? hasCatalogWhisperModel);
          reconcileSelectedConcreteModel(selectableModels);

          return;
        } catch (fetchCatalogError) {
          if (import.meta.env.DEV) {
            console.warn(
              "Failed to fetch model catalog, falling back to fetchModels:",
              fetchCatalogError
            );
          }
        }
      }

      if (modelClient.fetchModels) {
        const models = await modelClient.fetchModels();
        const availableGenerateModels = models.filter((availableModel) => {
          const tasks = availableModel.tasks || [];
          if (tasks.length > 0) return tasks.includes("generate");
          const id = availableModel.id.toLowerCase();
          return !id.includes("whisper") && !id.includes("embed");
        });
        hasFetched.current = true;
        setHasWhisperModel(
          models.some((availableModel) => availableModel.id === "whisper-large-v3")
        );
        setAvailableModels(availableGenerateModels);
        setModelAliases(buildFallbackModelAliases(availableGenerateModels));
        reconcileSelectedConcreteModel(availableGenerateModels);
      }
    } catch (fetchError) {
      if (import.meta.env.DEV) {
        console.warn("Failed to fetch model metadata:", fetchError);
      }
    } finally {
      isFetching.current = false;
    }
  }, [
    availableModels.length,
    modelAliases.length,
    os,
    reconcileSelectedConcreteModel,
    setAvailableModels,
    setHasWhisperModel,
    setModelAliases
  ]);

  useEffect(() => {
    void fetchCatalog();
  }, [fetchCatalog]);

  const getAlias = useCallback(
    (modelId: string): OpenSecretModelAlias | undefined => {
      const alias = aliasById.get(modelId as OpenSecretModelAlias["id"]);
      if (alias) return alias;

      const fallback = PRIMARY_AGENT_MODELS.find((primaryModel) => primaryModel.id === modelId);
      if (!fallback) return undefined;

      return {
        id: fallback.id,
        label: fallback.label,
        short_name: fallback.label,
        description: fallback.description,
        target_model: "",
        access: fallback.access,
        capabilities: fallback.capabilities
      };
    },
    [aliasById]
  );

  const getTargetModel = useCallback(
    (alias: OpenSecretModelAlias | undefined) => {
      if (!alias?.target_model) return undefined;
      return modelById.get(alias.target_model);
    },
    [modelById]
  );

  const getAccess = useCallback(
    (modelId: string): ModelAccessTier => {
      const alias = getAlias(modelId);
      if (alias) {
        return getTargetModel(alias)?.access || alias.access || "free";
      }
      return modelById.get(modelId)?.access || "free";
    },
    [getAlias, getTargetModel, modelById]
  );

  const hasAccessToModel = useCallback(
    (modelId: string) => {
      const access = getAccess(modelId);
      if (access === "free") return true;

      const planName = billingStatus?.product_name?.toLowerCase() || "";

      if (access === "pro") {
        return planName.includes("pro") || planName.includes("max") || planName.includes("team");
      }

      if (access === "starter") {
        return (
          planName.includes("starter") ||
          planName.includes("pro") ||
          planName.includes("max") ||
          planName.includes("team")
        );
      }

      return true;
    },
    [billingStatus?.product_name, getAccess]
  );

  const getDisplayLabel = (modelId: string): string => {
    const alias = getAlias(modelId);
    if (alias) return alias.short_name || alias.label;

    const selectedModel = modelById.get(modelId);
    return selectedModel?.short_name || selectedModel?.display_name || modelId;
  };

  const getDisplayNameText = (modelId: string): string => {
    const alias = getAlias(modelId);
    if (alias) return alias.label;

    const selectedModel = modelById.get(modelId);
    return selectedModel?.display_name || selectedModel?.short_name || modelId;
  };

  const handlePrimarySelect = (targetModel: string) => {
    if (!hasAccessToModel(targetModel)) {
      setSelectedModelName(getDisplayNameText(targetModel));
      setUpgradeDialogOpen(true);
      return;
    }

    onModelChange(targetModel);
  };

  const getModelBadges = (modelId: string): string[] => {
    const badges = modelById.get(modelId)?.badges || [];
    return badges.filter((badge) => badge !== "Pro" && badge !== "Starter");
  };

  const getDisplayName = (modelId: string, showLock = false) => {
    const selectedModel = modelById.get(modelId);
    const elements: React.ReactNode[] = [];

    if (selectedModel) {
      elements.push(selectedModel.display_name || selectedModel.short_name || modelId);

      const badges = getModelBadges(modelId);
      badges.forEach((badge, index) => {
        let badgeClass = "rounded-md px-1.5 py-0.5 text-[10px] font-medium";

        if (badge === "Coming Soon") {
          badgeClass += " bg-muted text-muted-foreground";
        } else if (badge === "New") {
          badgeClass += " bg-maple-info/10 text-maple-info";
        } else if (badge === "Reasoning") {
          badgeClass += " bg-maple-error/10 text-maple-error";
        } else if (badge === "Beta") {
          badgeClass += " bg-maple-warning/10 text-maple-warning";
        } else {
          badgeClass += " bg-[hsl(var(--maple-primary))]/10 text-[hsl(var(--maple-primary))]";
        }

        elements.push(
          <span key={`badge-${index}`} className={badgeClass}>
            {badge}
          </span>
        );
      });

      if (showLock && !hasAccessToModel(modelId)) {
        elements.push(<Lock key="lock" className="h-3 w-3 opacity-50" />);
      }

      if (selectedModel.capabilities?.vision) {
        elements.push(<Camera key="cam" className="h-3 w-3 opacity-50" />);
      }
    } else {
      elements.push(getDisplayNameText(modelId));
    }

    return <span className="flex items-center gap-1">{elements}</span>;
  };

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant="ghost"
            size="sm"
            className="h-8 gap-1 px-2 text-[hsl(var(--maple-secondary-700))] hover:bg-[hsl(var(--maple-primary-container))] hover:text-[hsl(var(--maple-secondary-700))]"
            aria-label={`Current agent model: ${getDisplayNameText(model)}. Click to change model.`}
          >
            <span className="text-xs font-medium">{getDisplayLabel(model)}</span>
            <ChevronDown className="h-3 w-3" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-64 p-0">
          {!showAdvanced ? (
            <div className="flex flex-col p-1">
              {PRIMARY_AGENT_MODELS.map((primaryModel) => {
                const alias = getAlias(primaryModel.id);
                const Icon = primaryModel.icon;
                const targetModel = primaryModel.id;
                const isActive = model === targetModel;
                const requiresUpgrade = !hasAccessToModel(targetModel);

                return (
                  <DropdownMenuItem
                    key={targetModel}
                    onClick={() => handlePrimarySelect(targetModel)}
                    className={cn(
                      "flex cursor-pointer items-center gap-2 px-3 py-1.5",
                      requiresUpgrade &&
                        "hover:bg-[hsl(var(--maple-primary-container))] dark:hover:bg-[hsl(var(--maple-primary))]/10"
                    )}
                  >
                    <Icon className="h-4 w-4 opacity-70" />
                    <div className="flex-1">
                      <div className="flex items-center gap-1.5">
                        <span className="text-sm font-medium">
                          {alias?.label || primaryModel.label}
                        </span>
                        {requiresUpgrade && <Lock className="h-3 w-3 opacity-50" />}
                      </div>
                      <div className="text-xs text-muted-foreground">
                        {alias?.description || primaryModel.description}
                      </div>
                    </div>
                    {isActive && <Check className="h-4 w-4" />}
                  </DropdownMenuItem>
                );
              })}

              <DropdownMenuSeparator />

              <DropdownMenuItem
                onSelect={(event) => {
                  event.preventDefault();
                  void fetchCatalog();
                  setShowAdvanced(true);
                }}
                className="flex cursor-pointer items-center gap-2 px-3 py-1.5"
              >
                <ChevronLeft className="h-4 w-4 rotate-180 opacity-70" />
                <div className="flex-1">
                  <span className="text-sm font-medium">More models</span>
                  <div className="text-xs text-muted-foreground">All models</div>
                </div>
              </DropdownMenuItem>
            </div>
          ) : (
            <div className="flex flex-col p-1">
              <DropdownMenuItem
                onSelect={(event) => {
                  event.preventDefault();
                  setShowAdvanced(false);
                }}
                className="mb-1 flex cursor-pointer items-center gap-2 px-3 py-1.5"
              >
                <ChevronLeft className="h-4 w-4" />
                <span className="text-sm font-medium">Back</span>
              </DropdownMenuItem>

              <DropdownMenuSeparator />

              <div className="max-h-80 overflow-y-auto">
                {availableModels.length === 0 ? (
                  <DropdownMenuItem disabled className="px-3 py-2 text-sm text-muted-foreground">
                    Loading models...
                  </DropdownMenuItem>
                ) : (
                  [...availableModels]
                    .filter(isSelectableChatModel)
                    .filter(
                      (availableModel, index, self) =>
                        self.findIndex((candidate) => candidate.id === availableModel.id) === index
                    )
                    .sort((a, b) => {
                      const aDisabled = a.enabled === false;
                      const bDisabled = b.enabled === false;
                      const aRestricted = !hasAccessToModel(a.id);
                      const bRestricted = !hasAccessToModel(b.id);

                      if (aDisabled && !bDisabled) return 1;
                      if (!aDisabled && bDisabled) return -1;
                      if (aRestricted && !bRestricted) return 1;
                      if (!aRestricted && bRestricted) return -1;

                      return (a.sort_order ?? 999) - (b.sort_order ?? 999);
                    })
                    .map((availableModel) => {
                      const isDisabled = availableModel.enabled === false;
                      const isRestricted = !hasAccessToModel(availableModel.id);
                      const selectedAliasTarget = getAlias(model)?.target_model;
                      const isActive =
                        model === availableModel.id || selectedAliasTarget === availableModel.id;

                      return (
                        <DropdownMenuItem
                          key={`agent-model-${availableModel.id}`}
                          onClick={() => {
                            if (isDisabled) return;
                            if (isRestricted) {
                              setSelectedModelName(
                                availableModel.display_name || availableModel.id
                              );
                              setUpgradeDialogOpen(true);
                            } else {
                              onModelChange(availableModel.id);
                              setShowAdvanced(false);
                            }
                          }}
                          className={cn(
                            "group flex items-center justify-between",
                            isDisabled && "cursor-not-allowed opacity-50",
                            isRestricted &&
                              "hover:bg-[hsl(var(--maple-primary-container))] dark:hover:bg-[hsl(var(--maple-primary))]/10"
                          )}
                          disabled={isDisabled}
                        >
                          <div className="flex flex-1 items-center gap-2">
                            <div className="text-sm">{getDisplayName(availableModel.id, true)}</div>
                          </div>
                          {isActive && <Check className="h-4 w-4" />}
                        </DropdownMenuItem>
                      );
                    })
                )}
              </div>
            </div>
          )}
        </DropdownMenuContent>
      </DropdownMenu>

      <UpgradePromptDialog
        open={upgradeDialogOpen}
        onOpenChange={setUpgradeDialogOpen}
        feature="model"
        modelName={selectedModelName}
      />
    </>
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
