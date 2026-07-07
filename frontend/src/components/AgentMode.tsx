import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useOpenSecret } from "@opensecret/react";
import {
  AlertCircle,
  Bot,
  ChevronRight,
  Check,
  Circle,
  FolderOpen,
  Loader2,
  Play,
  RotateCcw,
  Send,
  Square,
  Terminal,
  X
} from "lucide-react";
import {
  type ContentBlock,
  type PlanEntry,
  type SessionNotification,
  type ToolCall,
  type ToolCallContent,
  type ToolCallLocation,
  type ToolCallUpdate,
  type UsageUpdate
} from "@agentclientprotocol/sdk";
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
  AgentAcpClient,
  type PermissionDecision,
  type PermissionRequestHandle
} from "@/services/agentAcpClient";
import {
  agentRuntimeService,
  type AgentRuntimeStatus,
  type RecentProjectRoot
} from "@/services/agentRuntimeService";
import { proxyService } from "@/services/proxyService";
import { SIDEBAR_GRID_COLUMNS_CLASS, getSidebarLayoutStyle } from "@/constants/layout";
import { cn, useIsLandscapeMobile, useIsMobile } from "@/utils/utils";
import { isTauriDesktop } from "@/utils/platform";

type AgentMessageRole = "user" | "assistant" | "thought" | "system";

interface AgentMessageItem {
  type: "message";
  id: string;
  role: AgentMessageRole;
  text: string;
}

interface AgentToolItem {
  type: "tool";
  id: string;
  title: string;
  kind?: string;
  status?: string;
  input?: unknown;
  output?: unknown;
  content?: ToolCallContent[];
  locations?: ToolCallLocation[];
  meta?: Record<string, unknown> | null;
}

type AgentTimelineItem = AgentMessageItem | AgentToolItem;

const DEFAULT_MODEL = "auto:powerful";

export function AgentMode() {
  const { createApiKey } = useOpenSecret();
  const isMobile = useIsMobile();
  const isLandscapeMobile = useIsLandscapeMobile();
  const isCompactLayout = isMobile || isLandscapeMobile;
  const [isSidebarOpen, setIsSidebarOpen] = useState(!isCompactLayout);
  const [runtimeStatus, setRuntimeStatus] = useState<AgentRuntimeStatus | null>(null);
  const [recentRoots, setRecentRoots] = useState<RecentProjectRoot[]>([]);
  const [projectRoot, setProjectRoot] = useState("");
  const [model, setModel] = useState(DEFAULT_MODEL);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [timelineItems, setTimelineItems] = useState<AgentTimelineItem[]>([]);
  const [planEntries, setPlanEntries] = useState<PlanEntry[]>([]);
  const [usage, setUsage] = useState<UsageUpdate | null>(null);
  const [pendingPermissions, setPendingPermissions] = useState<PermissionRequestHandle[]>([]);
  const [input, setInput] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [isStarting, setIsStarting] = useState(false);
  const [isSending, setIsSending] = useState(false);
  const clientRef = useRef<AgentAcpClient | null>(null);
  const sessionIdRef = useRef<string | null>(null);
  const didRunInitialProxyInitRef = useRef(false);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setIsSidebarOpen(!isCompactLayout);
  }, [isCompactLayout]);

  useEffect(() => {
    sessionIdRef.current = sessionId;
  }, [sessionId]);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ block: "end" });
  }, [timelineItems, pendingPermissions, planEntries]);

  const activeRootLabel = useMemo(() => {
    if (!projectRoot) return "Select folder";
    return recentRoots.find((root) => root.path === projectRoot)?.name || basename(projectRoot);
  }, [projectRoot, recentRoots]);

  const toggleSidebar = useCallback(() => setIsSidebarOpen((prev) => !prev), []);

  const appendRuntimeLog = useCallback((message: string) => {
    if (!isTauriDesktop()) return;
    void agentRuntimeService.appendRuntimeLog(message).catch(() => {
      // Logging must never block Agent Mode interaction.
    });
  }, []);

  const ensureMapleProxyReady = useCallback(async () => {
    appendRuntimeLog("Ensuring Maple proxy is ready for Agent Mode");
    const status = await proxyService.ensureProxyReady(async (name) => {
      appendRuntimeLog(`Creating Agent Mode proxy API key ${name}`);
      const response = await createApiKey(name);
      return response.key;
    });
    appendRuntimeLog(
      `Maple proxy ready for Agent Mode on ${status.config.host}:${status.config.port} backend=${status.config.backend_url || "default"}`
    );
    return status;
  }, [appendRuntimeLog, createApiKey]);

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
        setProjectRoot(root);
        setModel(status.model || config.defaultModel || DEFAULT_MODEL);

        if (!didRunInitialProxyInitRef.current) {
          didRunInitialProxyInitRef.current = true;
          await ensureMapleProxyReady();
        }
      } catch (loadError) {
        if (!cancelled) setError(errorMessage(loadError));
      }
    }
    void loadInitialState();
    return () => {
      cancelled = true;
      clientRef.current?.close();
      clientRef.current = null;
    };
  }, [ensureMapleProxyReady]);

  const handleSessionUpdate = useCallback(
    (notification: SessionNotification) => {
      const update = notification.update;
      void agentRuntimeService.appendSessionEvent(notification.sessionId, {
        type: "sessionUpdate",
        update
      });

      switch (update.sessionUpdate) {
        case "agent_message_chunk":
          appendMessageChunk(
            "assistant",
            messageIdFromUpdate(update),
            contentBlockText(update.content)
          );
          break;
        case "agent_thought_chunk":
          appendMessageChunk(
            "thought",
            messageIdFromUpdate(update),
            contentBlockText(update.content)
          );
          break;
        case "tool_call":
          mergeToolCall(update);
          if (update.status === "failed") {
            appendRuntimeLog(`[ACP tool failed] ${toolFailureSummary(update)}`);
          }
          break;
        case "tool_call_update":
          mergeToolCall(update);
          if (update.status === "failed") {
            appendRuntimeLog(`[ACP tool failed] ${toolFailureSummary(update)}`);
          }
          break;
        case "plan":
          setPlanEntries(update.entries);
          break;
        case "usage_update":
          setUsage(update);
          break;
        case "user_message_chunk":
        case "available_commands_update":
        case "current_mode_update":
        case "config_option_update":
        case "session_info_update":
          break;
      }
    },
    [appendRuntimeLog]
  );

  const createClient = useCallback(() => {
    return new AgentAcpClient({
      onSessionUpdate: handleSessionUpdate,
      onPermissionRequest: (request) => {
        setPendingPermissions((current) => [...current, request]);
      },
      onDiagnostic: (diagnostic) => {
        appendRuntimeLog(`[ACP ${diagnostic.phase}] ${diagnostic.message}`);
      },
      onClosed: () => {
        clientRef.current = null;
      }
    });
  }, [appendRuntimeLog, handleSessionUpdate]);

  const refreshRuntimeStatus = useCallback(async () => {
    if (!isTauriDesktop()) return;
    const status = await agentRuntimeService.getRuntimeStatus();
    setRuntimeStatus(status);
  }, []);

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
        const roots = await agentRuntimeService.saveRecentProjectRoot(selected);
        setRecentRoots(roots);
      }
    } catch (chooseError) {
      setError(errorMessage(chooseError));
    }
  }, []);

  const ensureConnectedSession = useCallback(async () => {
    if (!projectRoot) {
      throw new Error("Select a project folder first");
    }

    await ensureMapleProxyReady();

    let status = await agentRuntimeService.getRuntimeStatus();
    if (!status.running || !status.acpUrl) {
      setIsStarting(true);
      try {
        status = await agentRuntimeService.startRuntime({
          projectRoot,
          model: model || DEFAULT_MODEL,
          mode: "approve"
        });
        setRuntimeStatus(status);
        setRecentRoots(await agentRuntimeService.listRecentProjectRoots());
      } finally {
        setIsStarting(false);
      }
    }

    if (!status.acpUrl) {
      throw new Error("ACP endpoint is not available");
    }

    if (!clientRef.current) {
      const nextClient = createClient();
      await nextClient.connect(status.acpUrl);
      clientRef.current = nextClient;
    }

    let activeSessionId = sessionIdRef.current;
    if (!activeSessionId) {
      activeSessionId = await clientRef.current.newSession(projectRoot);
      setSessionId(activeSessionId);
      sessionIdRef.current = activeSessionId;
      await agentRuntimeService.appendSessionEvent(activeSessionId, {
        type: "sessionStarted",
        projectRoot,
        model: model || DEFAULT_MODEL
      });
    }

    return { client: clientRef.current, sessionId: activeSessionId };
  }, [createClient, ensureMapleProxyReady, model, projectRoot]);

  const startRuntime = useCallback(async () => {
    setError(null);
    setIsStarting(true);
    try {
      if (!projectRoot) {
        throw new Error("Select a project folder first");
      }
      await ensureMapleProxyReady();
      const status = await agentRuntimeService.startRuntime({
        projectRoot: projectRoot || null,
        model: model || DEFAULT_MODEL,
        mode: "approve"
      });
      setRuntimeStatus(status);
      setProjectRoot(status.projectRoot || projectRoot);
      setModel(status.model || model || DEFAULT_MODEL);
      setRecentRoots(await agentRuntimeService.listRecentProjectRoots());
    } catch (startError) {
      setError(errorMessage(startError));
    } finally {
      setIsStarting(false);
    }
  }, [ensureMapleProxyReady, model, projectRoot]);

  const restartRuntime = useCallback(async () => {
    setError(null);
    setIsStarting(true);
    clientRef.current?.close();
    clientRef.current = null;
    setSessionId(null);
    setTimelineItems([]);
    setPlanEntries([]);
    setUsage(null);
    setPendingPermissions([]);
    try {
      await ensureMapleProxyReady();
      const status = await agentRuntimeService.restartRuntime({
        projectRoot: projectRoot || null,
        model: model || DEFAULT_MODEL,
        mode: "approve"
      });
      setRuntimeStatus(status);
      setProjectRoot(status.projectRoot || projectRoot);
      setModel(status.model || model || DEFAULT_MODEL);
    } catch (restartError) {
      setError(errorMessage(restartError));
    } finally {
      setIsStarting(false);
    }
  }, [ensureMapleProxyReady, model, projectRoot]);

  const stopRuntime = useCallback(async () => {
    clientRef.current?.close();
    clientRef.current = null;
    setSessionId(null);
    setPendingPermissions([]);
    setRuntimeStatus(await agentRuntimeService.stopRuntime());
  }, []);

  const sendMessage = useCallback(async () => {
    const text = input.trim();
    if (!text || isSending) return;

    setError(null);
    setInput("");
    setIsSending(true);
    setPlanEntries([]);
    setUsage(null);
    setPendingPermissions([]);
    const userMessage: AgentMessageItem = {
      type: "message",
      id: crypto.randomUUID(),
      role: "user",
      text
    };
    setTimelineItems((current) => [...current, userMessage]);

    try {
      const active = await ensureConnectedSession();
      await agentRuntimeService.appendSessionEvent(active.sessionId, {
        type: "userPrompt",
        text
      });
      await active.client.prompt(active.sessionId, text);
    } catch (sendError) {
      setError(errorMessage(sendError));
      setTimelineItems((current) => [
        ...current,
        {
          type: "message",
          id: crypto.randomUUID(),
          role: "system",
          text: errorMessage(sendError)
        }
      ]);
    } finally {
      setIsSending(false);
    }
  }, [ensureConnectedSession, input, isSending]);

  const cancelPrompt = useCallback(async () => {
    if (!sessionId || !clientRef.current) return;
    try {
      await clientRef.current.cancel(sessionId);
    } catch (cancelError) {
      setError(errorMessage(cancelError));
    }
  }, [sessionId]);

  const respondToPermission = useCallback((id: string, decision: PermissionDecision) => {
    setPendingPermissions((current) => {
      const request = current.find((candidate) => candidate.id === id);
      request?.decide(decision);
      return current.filter((candidate) => candidate.id !== id);
    });
  }, []);

  const cancelPermission = useCallback((id: string) => {
    setPendingPermissions((current) => {
      const request = current.find((candidate) => candidate.id === id);
      request?.cancel();
      return current.filter((candidate) => candidate.id !== id);
    });
  }, []);

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
          <div className="fixed left-4 top-[9.5px] z-20 flex items-center gap-1.5">
            <SidebarToggle onToggle={toggleSidebar} />
            <MapleWordmark className="h-4 w-auto" aria-hidden />
          </div>
        )}

        <header className="flex h-14 shrink-0 items-center justify-between gap-3 border-b border-border/35 px-4">
          <div className={cn("flex min-w-0 items-center gap-3", !isSidebarOpen && "pl-20")}>
            <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md border border-border/50 bg-muted/60">
              <Bot className="h-4 w-4" />
            </div>
            <div className="min-w-0">
              <div className="flex items-center gap-2">
                <h1 className="truncate text-sm font-medium">Agent Mode</h1>
                <RuntimeBadge running={runtimeStatus?.running ?? false} />
              </div>
              <p className="truncate text-xs text-muted-foreground">
                {projectRoot ? projectRoot : "No project folder selected"}
              </p>
            </div>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <Button
              variant="outline"
              size="icon"
              className="h-8 w-8"
              onClick={() => void refreshRuntimeStatus()}
              aria-label="Refresh agent status"
            >
              <RotateCcw className="h-4 w-4" />
            </Button>
            {runtimeStatus?.running ? (
              <Button
                variant="outline"
                size="icon"
                className="h-8 w-8"
                onClick={() => void stopRuntime()}
                aria-label="Stop agent runtime"
              >
                <Square className="h-3.5 w-3.5" />
              </Button>
            ) : (
              <Button
                variant="outline"
                size="icon"
                className="h-8 w-8"
                onClick={() => void startRuntime()}
                disabled={isStarting || !projectRoot}
                aria-label="Start agent runtime"
              >
                {isStarting ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <Play className="h-4 w-4" />
                )}
              </Button>
            )}
          </div>
        </header>

        {error && (
          <div className="mx-auto mt-3 w-full max-w-4xl px-4">
            <div className="flex items-start gap-2 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive">
              <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
              <span className="min-w-0 break-words">{error}</span>
            </div>
          </div>
        )}

        <main className="flex min-h-0 flex-1 flex-col">
          <div className="min-h-0 flex-1 overflow-y-auto px-4 py-5">
            <div className="mx-auto flex w-full max-w-4xl flex-col gap-4">
              {timelineItems.length === 0 ? (
                <div className="flex min-h-[42vh] items-center justify-center">
                  <div className="flex w-full max-w-2xl flex-col items-center gap-5 text-center">
                    <h2 className="font-displayWide text-3xl font-normal brand-gradient-text">
                      Work in a folder...
                    </h2>
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
                      onRestartRuntime={restartRuntime}
                      onSendMessage={sendMessage}
                    />
                  </div>
                </div>
              ) : (
                <>
                  <AgentTimeline items={timelineItems} />
                  {planEntries.length > 0 && <PlanList entries={planEntries} />}
                  {pendingPermissions.length > 0 && (
                    <PermissionList
                      requests={pendingPermissions}
                      onCancel={cancelPermission}
                      onDecision={respondToPermission}
                    />
                  )}
                  {usage && <UsageLine usage={usage} />}
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
                  onRestartRuntime={restartRuntime}
                  onSendMessage={sendMessage}
                />
              </div>
            </div>
          ) : null}
        </main>
      </div>
    </div>
  );

  function appendMessageChunk(
    role: AgentMessageRole,
    messageId: string | null | undefined,
    text: string
  ) {
    if (!text) return;
    setTimelineItems((current) => {
      const timelineMessageId = messageId ? `${messageId}:${role}` : undefined;
      const index = messageId
        ? current.findIndex((item) => item.type === "message" && item.id === timelineMessageId)
        : current.length - 1;
      const existing = index >= 0 ? current[index] : null;
      if (!messageId && (existing?.type !== "message" || existing.role !== role)) {
        return [...current, { type: "message", id: `${role}-${crypto.randomUUID()}`, role, text }];
      }
      if (index >= 0) {
        const next = [...current];
        const message = next[index];
        if (message.type === "message") {
          next[index] = { ...message, text: message.text + text };
        }
        return next;
      }
      return [
        ...current,
        {
          type: "message",
          id: timelineMessageId || `${role}-${crypto.randomUUID()}`,
          role,
          text
        }
      ];
    });
  }

  function mergeToolCall(update: (ToolCall | ToolCallUpdate) & { sessionUpdate: string }) {
    setTimelineItems((current) => {
      const index = current.findIndex(
        (item) => item.type === "tool" && item.id === update.toolCallId
      );
      const previous = index >= 0 ? current[index] : null;
      const nextTool: AgentToolItem = {
        type: "tool",
        id: update.toolCallId,
        title:
          update.title ?? (previous?.type === "tool" ? previous.title : undefined) ?? "Tool call",
        kind: update.kind ?? (previous?.type === "tool" ? previous.kind : undefined),
        status: update.status ?? (previous?.type === "tool" ? previous.status : undefined),
        input: update.rawInput ?? (previous?.type === "tool" ? previous.input : undefined),
        output: update.rawOutput ?? (previous?.type === "tool" ? previous.output : undefined),
        content: update.content ?? (previous?.type === "tool" ? previous.content : undefined),
        locations: update.locations ?? (previous?.type === "tool" ? previous.locations : undefined),
        meta: update._meta ?? (previous?.type === "tool" ? previous.meta : undefined)
      };

      if (index >= 0) {
        const next = [...current];
        next[index] = nextTool;
        return next;
      }
      return [...current, nextTool];
    });
  }
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
          disabled={isStarting}
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

function AgentTimeline({ items }: { items: AgentTimelineItem[] }) {
  return (
    <div className="flex flex-col gap-3">
      {items.map((item) =>
        item.type === "message" ? (
          <MessageBubble key={item.id} message={item} />
        ) : (
          <ToolCallList key={item.id} tools={[item]} />
        )
      )}
    </div>
  );
}

function MessageBubble({ message }: { message: AgentMessageItem }) {
  if (message.role === "thought") {
    return <ThinkingMessage text={message.text} />;
  }

  return (
    <div className={cn("flex", message.role === "user" ? "justify-end" : "justify-start")}>
      <div
        className={cn(
          "max-w-[82%] rounded-lg px-3 py-2 text-sm leading-6",
          message.role === "user"
            ? "bg-[hsl(var(--maple-primary))] text-primary-foreground"
            : "border border-border/45 bg-muted/45 text-foreground",
          message.role === "system" && "border-destructive/30 bg-destructive/5 text-destructive"
        )}
      >
        {message.role === "assistant" ? (
          <Markdown content={message.text} />
        ) : (
          <div className="whitespace-pre-wrap break-words">{message.text}</div>
        )}
      </div>
    </div>
  );
}

function ThinkingMessage({ text }: { text: string }) {
  return (
    <details className="group max-w-[82%] rounded-lg border border-border/35 bg-muted/25 px-3 py-2 text-sm text-muted-foreground">
      <summary className="flex cursor-pointer list-none items-center gap-2 font-medium text-foreground/80">
        <ChevronRight className="h-4 w-4 transition-transform group-open:rotate-90" />
        Thinking
      </summary>
      <div className="mt-2 whitespace-pre-wrap break-words text-xs leading-5">{text}</div>
    </details>
  );
}

function ToolCallList({ tools }: { tools: AgentToolItem[] }) {
  return (
    <div className="space-y-2">
      {tools.map((tool) => {
        const isFailed = tool.status === "failed";
        const hasDetails = toolHasDetails(tool);

        return (
          <details
            key={tool.id}
            open={isFailed}
            className={cn(
              "group rounded-lg border bg-muted/30 px-3 py-2",
              isFailed ? "border-destructive/35" : "border-border/45"
            )}
          >
            <summary className="flex cursor-pointer list-none items-center justify-between gap-3">
              <div className="flex min-w-0 items-center gap-2">
                <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground transition-transform group-open:rotate-90" />
                <Terminal className="h-4 w-4 shrink-0 text-muted-foreground" />
                <span className="truncate text-sm font-medium">{tool.title}</span>
              </div>
              <Badge
                variant="secondary"
                className={cn(
                  "shrink-0 capitalize",
                  isFailed && "border-destructive/40 bg-destructive/15 text-destructive"
                )}
              >
                {tool.status || "pending"}
              </Badge>
            </summary>
            <div className="mt-2 space-y-2 pl-6">
              <ToolDetail label="Summary" value={toolSummary(tool)} />
              {tool.kind && <p className="text-xs text-muted-foreground">{tool.kind}</p>}
              {tool.locations?.length ? (
                <ToolDetail
                  label="Locations"
                  value={tool.locations.map((item) => item.path).join("\n")}
                />
              ) : null}
              {tool.content?.length ? (
                <ToolDetail
                  label="Content"
                  value={tool.content.map(toolContentText).filter(Boolean).join("\n")}
                />
              ) : null}
              {tool.input !== undefined ? (
                <ToolDetail label="Input" value={formatUnknown(tool.input)} />
              ) : null}
              {tool.output !== undefined ? (
                <ToolDetail label="Output" value={formatUnknown(tool.output)} />
              ) : null}
              {tool.meta ? <ToolDetail label="Metadata" value={formatUnknown(tool.meta)} /> : null}
              {!hasDetails && isFailed ? (
                <ToolDetail
                  label="Missing Details"
                  value="ACP reported this tool call as failed but did not include input, output, content, locations, or metadata. Check the Agent Mode runtime log and session JSONL for the raw event."
                />
              ) : null}
            </div>
          </details>
        );
      })}
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

function toolHasDetails(tool: AgentToolItem): boolean {
  return Boolean(
    tool.kind ||
    tool.input !== undefined ||
    tool.output !== undefined ||
    tool.content?.length ||
    tool.locations?.length ||
    tool.meta
  );
}

function toolSummary(tool: AgentToolItem): string {
  const parts = [`id: ${tool.id}`, `status: ${tool.status || "pending"}`];
  const toolName = gooseToolName(tool.meta);
  if (toolName) parts.push(`tool: ${toolName}`);
  if (tool.kind) parts.push(`kind: ${tool.kind}`);
  return parts.join("\n");
}

function gooseToolName(meta: Record<string, unknown> | null | undefined): string | null {
  const goose = meta?.goose;
  if (!goose || typeof goose !== "object") return null;
  const toolCall = (goose as Record<string, unknown>).toolCall;
  if (!toolCall || typeof toolCall !== "object") return null;
  const name = (toolCall as Record<string, unknown>).toolName;
  return typeof name === "string" ? name : null;
}

function PermissionList({
  requests,
  onCancel,
  onDecision
}: {
  requests: PermissionRequestHandle[];
  onCancel: (id: string) => void;
  onDecision: (id: string, decision: PermissionDecision) => void;
}) {
  return (
    <div className="space-y-2">
      {requests.map((permission) => (
        <div
          key={permission.id}
          className="rounded-lg border border-[hsl(var(--maple-primary)/0.45)] bg-[hsl(var(--maple-primary)/0.06)] px-3 py-3"
        >
          <div className="flex items-start justify-between gap-3">
            <div className="min-w-0">
              <p className="text-sm font-medium">{permission.request.toolCall.title}</p>
              <p className="mt-1 break-words text-xs text-muted-foreground">
                {permission.request.toolCall.content
                  ?.map(toolContentText)
                  .filter(Boolean)
                  .join(" ")}
              </p>
            </div>
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7 shrink-0"
              onClick={() => onCancel(permission.id)}
              aria-label="Cancel permission request"
            >
              <X className="h-4 w-4" />
            </Button>
          </div>
          <div className="mt-3 flex flex-wrap gap-2">
            {permission.request.options.map((option) => (
              <Button
                key={option.optionId}
                size="sm"
                variant={option.kind.startsWith("allow") ? "default" : "outline"}
                className="h-8"
                onClick={() => onDecision(permission.id, option.kind)}
              >
                {option.kind.startsWith("allow") ? (
                  <Check className="mr-1 h-4 w-4" />
                ) : (
                  <X className="mr-1 h-4 w-4" />
                )}
                {formatPermission(option.name)}
              </Button>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

function PlanList({ entries }: { entries: PlanEntry[] }) {
  return (
    <div className="rounded-lg border border-border/45 bg-muted/25 px-3 py-2">
      <div className="space-y-1.5">
        {entries.map((entry, index) => (
          <div key={`${entry.content}-${index}`} className="flex items-start gap-2 text-sm">
            <Circle className="mt-1.5 h-2 w-2 shrink-0 fill-muted-foreground text-muted-foreground" />
            <span
              className={entry.status === "completed" ? "text-muted-foreground line-through" : ""}
            >
              {entry.content}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

function UsageLine({ usage }: { usage: UsageUpdate }) {
  return (
    <p className="text-right text-xs text-muted-foreground">
      {usage.used.toLocaleString()} / {usage.size.toLocaleString()} tokens
    </p>
  );
}

function RuntimeBadge({ running }: { running: boolean }) {
  return (
    <Badge
      variant={running ? "default" : "secondary"}
      className="h-5 rounded-md px-1.5 text-[11px]"
    >
      {running ? "running" : "stopped"}
    </Badge>
  );
}

function contentBlockText(block: ContentBlock): string {
  switch (block.type) {
    case "text":
      return block.text;
    case "resource_link":
      return block.uri;
    case "resource":
      return "resource" in block.resource && "text" in block.resource ? block.resource.text : "";
    case "image":
      return "[image]";
    case "audio":
      return "[audio]";
  }
}

function toolContentText(content: ToolCallContent): string {
  if (content.type === "diff") {
    const oldLines = content.oldText?.split("\n").length ?? 0;
    const newLines = content.newText.split("\n").length;
    return `${content.path}: ${oldLines} -> ${newLines} lines`;
  }
  if (content.type === "terminal") {
    return content.terminalId;
  }
  return contentBlockText(content.content);
}

function formatUnknown(value: unknown): string {
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function basename(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || path;
}

function formatPermission(value: string): string {
  return value.replace(/_/g, " ");
}

function errorMessage(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  const details = errorDetails(error);
  return details ? `${message}: ${details}` : message;
}

function errorDetails(error: unknown): string | null {
  if (!error || typeof error !== "object") return null;
  const record = error as Record<string, unknown>;
  const parts: string[] = [];
  if (typeof record.code === "number") {
    parts.push(`code=${record.code}`);
  }
  if ("data" in record && record.data !== undefined) {
    parts.push(`data=${formatUnknown(record.data)}`);
  }
  return parts.join(" ");
}

function messageIdFromUpdate(update: { messageId?: string | null; _meta?: unknown }) {
  if (update.messageId) return update.messageId;
  const goose = gooseMeta(update);
  return typeof goose?.messageId === "string" ? goose.messageId : undefined;
}

function toolFailureSummary(update: ToolCall | ToolCallUpdate): string {
  const input = update.rawInput !== undefined ? formatUnknown(update.rawInput).trim() : "";
  const output =
    update.rawOutput !== undefined
      ? formatUnknown(update.rawOutput).trim()
      : update.content?.map(toolContentText).filter(Boolean).join(" ").trim() || "";
  return [
    `id=${update.toolCallId}`,
    update.title ? `title=${update.title}` : null,
    update.kind ? `kind=${update.kind}` : null,
    input ? `input=${truncate(input, 500)}` : null,
    output ? `output=${truncate(output, 500)}` : "output=no ACP error details"
  ]
    .filter(Boolean)
    .join(" ");
}

function gooseMeta(update: { _meta?: unknown }): Record<string, unknown> | null {
  if (!update._meta || typeof update._meta !== "object") return null;
  const goose = (update._meta as Record<string, unknown>).goose;
  return goose && typeof goose === "object" ? (goose as Record<string, unknown>) : null;
}

function truncate(value: string, maxLength: number): string {
  return value.length > maxLength ? `${value.slice(0, maxLength)}...` : value;
}
