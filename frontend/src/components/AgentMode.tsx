import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useReducer,
  useRef,
  useState
} from "react";
import { useOpenSecret } from "@opensecret/react";
import { useOpenAI } from "@/ai/useOpenAi";
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
  Expand,
  FolderOpen,
  Loader2,
  Lock,
  MessageSquarePlus,
  MoreHorizontal,
  ShieldCheck,
  Shrink,
  Trash,
  X,
  Zap
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle
} from "@/components/ui/alert-dialog";
import { Textarea } from "@/components/ui/textarea";
import { Markdown, ThinkingBlock } from "@/components/markdown";
import {
  CHAT_COMPOSER_TEXTAREA_CLASS,
  ChatAssistantPendingTurn,
  ChatAssistantTurn,
  ChatComposerSurface,
  ChatDesktopConversationHeader,
  ChatUserTurn
} from "@/components/chat/ChatTurn";
import { ChatCopyButton } from "@/components/chat/ChatCopyButton";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectSeparator,
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
import { DeleteChatDialog } from "@/components/DeleteChatDialog";
import { UpgradePromptDialog } from "@/components/UpgradePromptDialog";
import { AgentMcpMenu, AgentMcpServersDialog } from "@/components/agent/AgentMcpControls";
import {
  agentRuntimeService,
  awaitAgentAuthUser,
  type AgentConfig,
  type AgentEventEnvelope,
  type AgentMcpServer,
  type AgentPermissionDecision,
  type AgentProjectSkillsTrustStatus,
  type AgentRuntimeStatus,
  type AgentSessionMcpServer,
  type AgentSessionSummary,
  type AgentTimelineItem,
  type RecentProjectRoot
} from "@/services/agentRuntimeService";
import {
  createProjectOrderState,
  groupAgentSessionsByRoot,
  hasExceededProjectDragThreshold,
  mergeAgentProjectRoots,
  projectInsertionIndex,
  projectOrderForExistingRegistration,
  projectOrderReducer,
  reorderProjectRoots
} from "@/services/agentProjectOrdering";
import {
  isMcpConnectionErrorEvent,
  mcpConnectionErrorMessage,
  userFacingAgentError
} from "@/services/agentMcpErrors";
import { reconcileNewChatMcpServerNames } from "@/services/agentMcpServers";
import {
  AgentProxyManualConfigConflictError,
  AgentProxyReplacementSetupError,
  proxyService
} from "@/services/proxyService";
import { agentOperationFence } from "@/services/agentOperationFence";
import {
  clearAgentThoughtLabelsForSession,
  clearAgentThoughtLabelsForTurn,
  getAgentThoughtLabel,
  requestAgentThoughtLabel,
  startAgentThoughtLabelDisplay
} from "@/services/agentThoughtLabels";
import {
  AgentLiveThoughtPhaseTracker,
  activeAgentThinkingItemId,
  agentThinkingPhaseId,
  agentThinkingPhaseTurnId,
  coalesceAdjacentThinkingItems,
  getAgentTurnCopyText,
  groupAgentTimelineItems,
  hasAgentUserMessage,
  isRenderableAgentTimelineItem,
  shouldShowAgentAssistantLoader
} from "@/services/agentTimeline";
import {
  DEFAULT_AGENT_MODEL,
  PRIMARY_AGENT_MODEL_IDS,
  reconcileAgentModel,
  resolveAgentModelVisionCapability
} from "@/services/agentModels";
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
import {
  usePersistentHomeNavigation,
  usePersistentSidebarState
} from "@/contexts/PersistentHomeNavigationContext";
import type {
  ModelAccessTier,
  OpenSecretModel,
  OpenSecretModelAlias,
  OpenSecretModelCatalog
} from "@/state/LocalStateContextDef";

const DEFAULT_MODEL = DEFAULT_AGENT_MODEL;
const DEFAULT_MODE = "smart_approve";
const NEW_SESSION_PENDING_KEY = "__maple-agent-new-session__";
const NEW_PROJECT_OPTION_VALUE = "__maple-agent-new-project__";
const MAX_STABLE_SESSION_LOAD_ATTEMPTS = 3;
const AUTO_SCROLL_BOTTOM_THRESHOLD_PX = 100;
const SIDEBAR_REORDER_ANIMATION_MS = 150;
const SIDEBAR_ICON_STROKE = 2;
const AGENT_SESSION_DELETED_EVENT = "maple:agent-session-deleted";
// Mode switches remount AgentMode, so project-root mutations must be ordered outside it.
const projectRootPersistenceQueues = new Map<string, Promise<void>>();
const AGENT_SIDEBAR_ELLIPSIS_FADE =
  "pointer-events-none w-4 shrink-0 self-stretch bg-gradient-to-r from-transparent to-[hsl(var(--muted))] dark:to-[hsl(var(--sidebar))]";
const AGENT_SIDEBAR_ELLIPSIS_TRIGGER_ROW_BASE =
  "absolute inset-y-0 right-0 z-30 flex min-h-0 items-stretch";
const AGENT_SIDEBAR_ELLIPSIS_BUTTON =
  "relative z-10 shrink-0 rounded-full border-0 bg-muted p-1.5 text-foreground/40 transition-colors dark:bg-[hsl(var(--sidebar))] hover:text-foreground group-hover:text-foreground focus-visible:text-foreground focus-visible:outline-none";

function thoughtLabelTurnGenerationKey(sessionId: string, assistantTurnId: string): string {
  return `${sessionId}\u0000${assistantTurnId}`;
}

function thoughtLabelDisplayKey(sessionId: string, phaseId: string): string {
  return `${sessionId}\u0000${phaseId}`;
}

class PendingAgentSendCancelledError extends Error {
  constructor() {
    super("Agent message cancelled before the run started");
    this.name = "PendingAgentSendCancelledError";
  }
}

function agentSidebarEllipsisTriggerRowClass(isCompactLayout: boolean): string {
  if (isCompactLayout) return AGENT_SIDEBAR_ELLIPSIS_TRIGGER_ROW_BASE;
  return `${AGENT_SIDEBAR_ELLIPSIS_TRIGGER_ROW_BASE} transition-opacity duration-150 opacity-0 pointer-events-none group-hover:pointer-events-auto group-hover:opacity-100 group-focus-within:pointer-events-auto group-focus-within:opacity-100 has-[[data-state=open]]:pointer-events-auto has-[[data-state=open]]:opacity-100`;
}

type AgentPermissionMode = "smart_approve" | "auto";

const AGENT_PERMISSION_MODES: Array<{
  value: AgentPermissionMode;
  label: string;
  description: string;
}> = [
  {
    value: "smart_approve",
    label: "Read only",
    description: "Auto-runs local reads; asks before writes and remote access"
  },
  {
    value: "auto",
    label: "Allow all",
    description: "Allows all tool calls without prompting"
  }
];

const QUICK_AGENT_MODEL = {
  id: QUICK_MODEL_ALIAS,
  label: "Quick",
  icon: Zap,
  description: "Fast, everyday responses",
  access: "free" as ModelAccessTier,
  capabilities: { vision: false, reasoning: true }
} as const;

const LEGACY_POWERFUL_AGENT_ALIAS = {
  id: POWERFUL_MODEL_ALIAS,
  label: "Powerful",
  icon: Brain,
  description: "Deeper thinking & analysis",
  access: "pro" as ModelAccessTier,
  capabilities: { vision: true, reasoning: true }
} as const;

const PRIMARY_AGENT_MODELS = PRIMARY_AGENT_MODEL_IDS.map((id) =>
  id === DEFAULT_AGENT_MODEL
    ? {
        id: DEFAULT_AGENT_MODEL,
        label: "GLM 5.2",
        icon: Brain,
        description: "Recommended for Agent Mode",
        access: "pro" as ModelAccessTier,
        capabilities: { vision: false, reasoning: true }
      }
    : QUICK_AGENT_MODEL
);

const FALLBACK_AGENT_MODEL_ALIASES = [QUICK_AGENT_MODEL, LEGACY_POWERFUL_AGENT_ALIAS] as const;

const FALLBACK_ALIAS_TARGETS = {
  [QUICK_MODEL_ALIAS]: "gpt-oss-120b",
  [POWERFUL_MODEL_ALIAS]: "kimi-k2-6"
} as const;

type ModelCatalogClient = {
  fetchModelCatalog?: () => Promise<OpenSecretModelCatalog>;
  fetchModels?: () => Promise<OpenSecretModel[]>;
};

function normalizeAgentPermissionMode(mode?: string | null): AgentPermissionMode {
  return mode === "auto" ? "auto" : DEFAULT_MODE;
}

function isSelectableChatModel(model: OpenSecretModel): boolean {
  return model.enabled !== false && model.deprecated !== true && model.capabilities?.chat !== false;
}

function buildFallbackModelAliases(models: OpenSecretModel[]): OpenSecretModelAlias[] {
  const modelById = new Map(models.map((availableModel) => [availableModel.id, availableModel]));

  return FALLBACK_AGENT_MODEL_ALIASES.map((primaryModel) => {
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

export function AgentMode({ userId }: { userId: string }) {
  const os = useOpenSecret();
  const openai = useOpenAI();
  const { createApiKey, deleteApiKey } = os;
  const { availableModels, modelAliases } = useLocalState();
  const { agentSessionSelection } = usePersistentHomeNavigation();
  const isMobile = useIsMobile();
  const isLandscapeMobile = useIsLandscapeMobile();
  const isCompactLayout = isMobile || isLandscapeMobile;
  const [isSidebarOpen, setIsSidebarOpen] = usePersistentSidebarState(isCompactLayout);
  const [runtimeStatus, setRuntimeStatus] = useState<AgentRuntimeStatus | null>(null);
  const [projectOrderState, dispatchProjectOrder] = useReducer(
    projectOrderReducer<RecentProjectRoot>,
    createProjectOrderState<RecentProjectRoot>([])
  );
  const recentRoots = projectOrderState.visible;
  const [sessions, setSessions] = useState<AgentSessionSummary[]>([]);
  const [isSessionHistoryReady, setIsSessionHistoryReady] = useState(false);
  const [sessionToDelete, setSessionToDelete] = useState<AgentSessionSummary | null>(null);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [projectRoot, setProjectRoot] = useState("");
  const [model, setModel] = useState(DEFAULT_MODEL);
  const [mode, setMode] = useState<AgentPermissionMode>(DEFAULT_MODE);
  const [timelineItems, setTimelineItems] = useState<AgentTimelineItem[]>([]);
  const [generatedThoughtLabels, setGeneratedThoughtLabels] = useState<
    Record<string, Record<string, string>>
  >({});
  const [mcpServers, setMcpServers] = useState<AgentMcpServer[]>([]);
  const [newChatMcpServerNames, setNewChatMcpServerNames] = useState<Set<string>>(() => new Set());
  const [sessionMcpServers, setSessionMcpServers] = useState<AgentSessionMcpServer[]>([]);
  const [sessionMcpServersSessionId, setSessionMcpServersSessionId] = useState<string | null>(null);
  const [isMcpServersDialogOpen, setIsMcpServersDialogOpen] = useState(false);
  const [isMcpServersLoading, setIsMcpServersLoading] = useState(true);
  const [isSessionMcpServersLoading, setIsSessionMcpServersLoading] = useState(false);
  const [isMcpServerTogglePending, setIsMcpServerTogglePending] = useState(false);
  const [input, setInput] = useState("");
  const [isAgentFullscreen, setIsAgentFullscreen] = useState(
    () => localStorage.getItem("agentFullscreen") === "true"
  );
  const [error, setError] = useState<string | null>(null);
  const [hasManualProxyConflict, setHasManualProxyConflict] = useState(false);
  const [isAuthTransitionReady, setIsAuthTransitionReady] = useState(false);
  const [isInitializing, setIsInitializing] = useState(true);
  const [isReplacingManualProxy, setIsReplacingManualProxy] = useState(false);
  const [isStarting, setIsStarting] = useState(false);
  const [isPermissionModeUpdating, setIsPermissionModeUpdating] = useState(false);
  const [isProjectRootRegistrationPending, setIsProjectRootRegistrationPending] = useState(false);
  const [projectSkillsTrustPrompt, setProjectSkillsTrustPrompt] =
    useState<AgentProjectSkillsTrustStatus | null>(null);
  const [isProjectSkillsTrustLoading, setIsProjectSkillsTrustLoading] = useState(false);
  const [projectSkillsTrustSavingDecision, setProjectSkillsTrustSavingDecision] = useState<
    boolean | null
  >(null);
  const [projectSkillsTrustError, setProjectSkillsTrustError] = useState<string | null>(null);
  const [pendingSendSessionIds, setPendingSendSessionIds] = useState<Set<string>>(() => new Set());
  const [pendingSessionSelectionId, setPendingSessionSelectionId] = useState<string | null>(null);
  const [activeRunsBySession, setActiveRunsBySession] = useState<Record<string, string>>({});
  const [completedUnreadSessionIds, setCompletedUnreadSessionIds] = useState<Set<string>>(
    () => new Set()
  );
  const chatContainerRef = useRef<HTMLDivElement>(null);
  const activeSessionIdRef = useRef(activeSessionId);
  const deletedSessionIdsRef = useRef(new Set<string>());
  const shouldAutoScrollRef = useRef(true);
  const permissionModeUpdateRef = useRef<Promise<void>>(Promise.resolve());
  const permissionModeUpdateGenerationRef = useRef(0);
  const selectedModeRef = useRef<AgentPermissionMode>(mode);
  const committedModeRef = useRef<AgentPermissionMode>(mode);
  const terminalRunIdsRef = useRef(new Set<string>());
  const pendingSendTokensRef = useRef(new Map<string, number>());
  const cancelledPendingSendTokensRef = useRef(new Set<number>());
  const nextSendTokenRef = useRef(0);
  const activeRunsBySessionRef = useRef<Record<string, string>>({});
  const timelineRevisionBySessionRef = useRef(new Map<string, number>());
  const sessionSelectionGenerationRef = useRef(0);
  const pendingSessionSelectionIdRef = useRef<string | null>(null);
  const interactionGenerationRef = useRef(0);
  const startRequestGenerationRef = useRef(0);
  const runStateGenerationRef = useRef(0);
  const isAgentModelLockedRef = useRef(false);
  const mcpSessionLoadGenerationRef = useRef(0);
  const mcpToggleGenerationRef = useRef(0);
  const previousIsCompactLayoutRef = useRef(isCompactLayout);
  const hasAttemptedSessionRestoreRef = useRef(false);
  const isAgentModeMountedRef = useRef(true);
  const projectOrderRequestIdRef = useRef(0);
  const projectSkillsTrustGenerationRef = useRef(0);
  const thoughtPhaseTrackerRef = useRef(new AgentLiveThoughtPhaseTracker());
  const thoughtLabelGenerationBySessionRef = useRef(new Map<string, number>());
  const thoughtLabelGenerationByTurnRef = useRef(new Map<string, number>());
  const thoughtLabelDisplayControllersRef = useRef(new Map<string, () => void>());

  const cancelThoughtLabelDisplays = useCallback((sessionId?: string, assistantTurnId?: string) => {
    const prefix =
      sessionId === undefined
        ? ""
        : `${sessionId}\u0000${assistantTurnId ? `${assistantTurnId}:thought-` : ""}`;
    thoughtLabelDisplayControllersRef.current.forEach((cancel, key) => {
      if (!key.startsWith(prefix)) return;
      cancel();
      thoughtLabelDisplayControllersRef.current.delete(key);
    });
  }, []);

  const generateThoughtLabel = useCallback(
    (phase: { sessionId: string; phaseId: string; userRequest: string; reasoningText: string }) => {
      const sessionGeneration =
        thoughtLabelGenerationBySessionRef.current.get(phase.sessionId) ?? 0;
      const assistantTurnId = agentThinkingPhaseTurnId(phase.phaseId);
      const turnGenerationKey = thoughtLabelTurnGenerationKey(phase.sessionId, assistantTurnId);
      const turnGeneration = thoughtLabelGenerationByTurnRef.current.get(turnGenerationKey) ?? 0;
      const identifiers = {
        userId,
        sessionId: phase.sessionId,
        phaseId: phase.phaseId
      };
      const displayKey = thoughtLabelDisplayKey(phase.sessionId, phase.phaseId);
      const generationIsCurrent = () =>
        isAgentModeMountedRef.current &&
        !deletedSessionIdsRef.current.has(phase.sessionId) &&
        (thoughtLabelGenerationBySessionRef.current.get(phase.sessionId) ?? 0) ===
          sessionGeneration &&
        (thoughtLabelGenerationByTurnRef.current.get(turnGenerationKey) ?? 0) === turnGeneration;
      const commitDisplayLabel = (label: string, expectedLabel?: string) => {
        if (!generationIsCurrent()) return;
        setGeneratedThoughtLabels((current) => {
          const currentLabel = current[phase.sessionId]?.[phase.phaseId];
          if (expectedLabel !== undefined && currentLabel !== expectedLabel) return current;
          if (currentLabel === label) return current;
          return {
            ...current,
            [phase.sessionId]: {
              ...current[phase.sessionId],
              [phase.phaseId]: label
            }
          };
        });
      };
      thoughtLabelDisplayControllersRef.current.get(displayKey)?.();
      thoughtLabelDisplayControllersRef.current.delete(displayKey);
      let cancelDisplay: (() => void) | null = null;
      cancelDisplay = startAgentThoughtLabelDisplay({
        cachedLabel: getAgentThoughtLabel(identifiers),
        commit: commitDisplayLabel,
        request: () =>
          requestAgentThoughtLabel(openai, identifiers, {
            userRequest: phase.userRequest,
            reasoningText: phase.reasoningText
          }).finally(() => {
            if (
              cancelDisplay &&
              thoughtLabelDisplayControllersRef.current.get(displayKey) === cancelDisplay
            ) {
              thoughtLabelDisplayControllersRef.current.delete(displayKey);
            }
          })
      });
      if (cancelDisplay) thoughtLabelDisplayControllersRef.current.set(displayKey, cancelDisplay);
    },
    [openai, userId]
  );

  const invalidateThoughtLabelsForTurn = useCallback(
    (sessionId: string, assistantTurnId: string) => {
      cancelThoughtLabelDisplays(sessionId, assistantTurnId);
      const generationKey = thoughtLabelTurnGenerationKey(sessionId, assistantTurnId);
      thoughtLabelGenerationByTurnRef.current.set(
        generationKey,
        (thoughtLabelGenerationByTurnRef.current.get(generationKey) ?? 0) + 1
      );
      clearAgentThoughtLabelsForTurn(userId, sessionId, assistantTurnId);
      const phasePrefix = `${assistantTurnId}:thought-`;
      setGeneratedThoughtLabels((current) => {
        const sessionLabels = current[sessionId];
        if (!sessionLabels) return current;
        const retainedLabels = Object.fromEntries(
          Object.entries(sessionLabels).filter(([phaseId]) => !phaseId.startsWith(phasePrefix))
        );
        return {
          ...current,
          [sessionId]: retainedLabels
        };
      });
    },
    [cancelThoughtLabelDisplays, userId]
  );

  const invalidateThoughtLabelsForSession = useCallback(
    (sessionId: string) => {
      cancelThoughtLabelDisplays(sessionId);
      thoughtLabelGenerationBySessionRef.current.set(
        sessionId,
        (thoughtLabelGenerationBySessionRef.current.get(sessionId) ?? 0) + 1
      );
      clearAgentThoughtLabelsForSession(userId, sessionId);
      setGeneratedThoughtLabels((current) => {
        if (!current[sessionId]) return current;
        const next = { ...current };
        delete next[sessionId];
        return next;
      });
    },
    [cancelThoughtLabelDisplays, userId]
  );

  const applyAuthoritativeMode = useCallback((value: AgentPermissionMode) => {
    selectedModeRef.current = value;
    committedModeRef.current = value;
    setMode(value);
  }, []);

  useEffect(() => {
    isAgentModeMountedRef.current = true;
    return () => {
      isAgentModeMountedRef.current = false;
      cancelThoughtLabelDisplays();
    };
  }, [cancelThoughtLabelDisplays]);

  useEffect(() => {
    const wasCompactLayout = previousIsCompactLayoutRef.current;
    previousIsCompactLayoutRef.current = isCompactLayout;
    if (isCompactLayout && !wasCompactLayout) {
      setIsSidebarOpen(false);
    }
  }, [isCompactLayout, setIsSidebarOpen]);

  useEffect(() => {
    activeSessionIdRef.current = activeSessionId;
  }, [activeSessionId]);

  useEffect(() => {
    localStorage.setItem("agentFullscreen", isAgentFullscreen.toString());
  }, [isAgentFullscreen]);

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

  const visibleProjectRoots = useMemo(
    () => mergeAgentProjectRoots(recentRoots, projectRoot, sessions),
    [projectRoot, recentRoots, sessions]
  );
  const visibleProjectRootsRef = useRef<RecentProjectRoot[]>([]);
  visibleProjectRootsRef.current = visibleProjectRoots;
  const activeRootLabel = useMemo(() => {
    if (!projectRoot) return "Select folder";
    return (
      visibleProjectRoots.find((root) => root.path === projectRoot)?.name || basename(projectRoot)
    );
  }, [projectRoot, visibleProjectRoots]);
  const activeSessionTitle = useMemo(() => {
    const activeSession = sessions.find((session) => session.id === activeSessionId);
    return activeSession ? sessionTitle(activeSession) : "New task";
  }, [activeSessionId, sessions]);
  const activeRunId = activeSessionId ? (activeRunsBySession[activeSessionId] ?? null) : null;
  const activePendingSendKey = activeSessionId || NEW_SESSION_PENDING_KEY;
  const isSubmitting = pendingSendSessionIds.has(activePendingSendKey);
  const isSessionSelectionPending = pendingSessionSelectionId !== null;
  const isProjectOrderSaving = projectOrderState.pendingRequestId !== null;
  const isProjectSkillsTrustSaving = projectSkillsTrustSavingDecision !== null;
  const areAgentSettingsLocked =
    !isAuthTransitionReady ||
    isInitializing ||
    isStarting ||
    isPermissionModeUpdating ||
    isSessionSelectionPending ||
    isSubmitting ||
    isProjectOrderSaving ||
    isProjectRootRegistrationPending ||
    isProjectSkillsTrustLoading ||
    isProjectSkillsTrustSaving ||
    projectSkillsTrustPrompt !== null ||
    isReplacingManualProxy ||
    hasManualProxyConflict;
  const hasStartedAgentSession =
    hasAgentUserMessage(timelineItems) ||
    sessions.some((session) => session.id === activeSessionId && session.messageCount > 0);
  const isAgentModelLocked = hasStartedAgentSession || Boolean(activeRunId) || isSubmitting;
  isAgentModelLockedRef.current = isAgentModelLocked;
  const isAgentModelSelectionDisabled = areAgentSettingsLocked || isAgentModelLocked;
  const isAgentSendLocked = areAgentSettingsLocked;
  const isSending = Boolean(activeRunId) || isSubmitting;
  const selectedNewChatMcpServerNames = useMemo(
    () =>
      mcpServers
        .filter((server) => newChatMcpServerNames.has(server.name))
        .map((server) => server.name),
    [mcpServers, newChatMcpServerNames]
  );
  const composerMcpServers = useMemo<AgentSessionMcpServer[]>(
    () =>
      activeSessionId
        ? sessionMcpServersSessionId === activeSessionId
          ? sessionMcpServers
          : []
        : mcpServers.map((server) => ({
            name: server.name,
            description: server.description,
            transport: server.transport.type,
            enabled: newChatMcpServerNames.has(server.name),
            available: true
          })),
    [
      activeSessionId,
      mcpServers,
      newChatMcpServerNames,
      sessionMcpServers,
      sessionMcpServersSessionId
    ]
  );
  const isMcpToggleDisabled =
    areAgentSettingsLocked || Boolean(activeRunId) || isMcpServerTogglePending;
  const isComposerMcpLoading = activeSessionId
    ? isSessionMcpServersLoading || sessionMcpServersSessionId !== activeSessionId
    : isMcpServersLoading;
  const runningSessionIds = useMemo(() => {
    const ids = new Set(Object.keys(activeRunsBySession));
    for (const sessionId of pendingSendSessionIds) {
      if (sessionId !== NEW_SESSION_PENDING_KEY) ids.add(sessionId);
    }
    if (pendingSessionSelectionId && pendingSessionSelectionId !== NEW_SESSION_PENDING_KEY) {
      ids.add(pendingSessionSelectionId);
    }
    return ids;
  }, [activeRunsBySession, pendingSendSessionIds, pendingSessionSelectionId]);

  const toggleSidebar = useCallback(() => setIsSidebarOpen((prev) => !prev), [setIsSidebarOpen]);

  const beginSessionSelection = useCallback((sessionId: string): number => {
    interactionGenerationRef.current += 1;
    const generation = sessionSelectionGenerationRef.current + 1;
    sessionSelectionGenerationRef.current = generation;
    pendingSessionSelectionIdRef.current = sessionId;
    setPendingSessionSelectionId(sessionId);
    return generation;
  }, []);

  const finishSessionSelection = useCallback((generation: number): boolean => {
    if (sessionSelectionGenerationRef.current !== generation) return false;
    pendingSessionSelectionIdRef.current = null;
    setPendingSessionSelectionId(null);
    return true;
  }, []);

  const invalidateSessionSelection = useCallback(() => {
    interactionGenerationRef.current += 1;
    sessionSelectionGenerationRef.current += 1;
    pendingSessionSelectionIdRef.current = null;
    setPendingSessionSelectionId(null);
  }, []);

  const markPendingSend = useCallback((sessionKey: string, token: number) => {
    pendingSendTokensRef.current.set(sessionKey, token);
    setPendingSendSessionIds((current) => {
      if (current.has(sessionKey)) return current;
      const next = new Set(current);
      next.add(sessionKey);
      return next;
    });
  }, []);

  const movePendingSend = useCallback((fromKey: string, toKey: string, token: number) => {
    if (fromKey === toKey || pendingSendTokensRef.current.get(fromKey) !== token) return;
    pendingSendTokensRef.current.delete(fromKey);
    pendingSendTokensRef.current.set(toKey, token);
    setPendingSendSessionIds((current) => {
      const next = new Set(current);
      next.delete(fromKey);
      next.add(toKey);
      return next;
    });
  }, []);

  const clearPendingSend = useCallback((sessionKey: string, token?: number) => {
    if (token !== undefined && pendingSendTokensRef.current.get(sessionKey) !== token) return;
    if (!pendingSendTokensRef.current.delete(sessionKey)) return;
    setPendingSendSessionIds((current) => {
      if (!current.has(sessionKey)) return current;
      const next = new Set(current);
      next.delete(sessionKey);
      return next;
    });
  }, []);

  const applyRuntimeStatus = useCallback(
    (status: AgentRuntimeStatus, expectedRunStateGeneration?: number) => {
      if (
        expectedRunStateGeneration !== undefined &&
        runStateGenerationRef.current !== expectedRunStateGeneration
      ) {
        return;
      }
      setRuntimeStatus(status);
      const activeRuns = status.activeRuns || {};
      activeRunsBySessionRef.current = activeRuns;
      setActiveRunsBySession(activeRuns);
    },
    []
  );

  const recordActiveRun = useCallback((sessionId: string, runId: string) => {
    const next = { ...activeRunsBySessionRef.current, [sessionId]: runId };
    activeRunsBySessionRef.current = next;
    setActiveRunsBySession(next);
  }, []);

  const clearActiveRun = useCallback((sessionId: string, expectedRunId?: string) => {
    const current = activeRunsBySessionRef.current;
    if (expectedRunId && current[sessionId] !== expectedRunId) return;
    if (!(sessionId in current)) return;
    const next = { ...current };
    delete next[sessionId];
    activeRunsBySessionRef.current = next;
    setActiveRunsBySession(next);
  }, []);

  const bumpTimelineRevision = useCallback((sessionId: string): number => {
    const revision = (timelineRevisionBySessionRef.current.get(sessionId) || 0) + 1;
    timelineRevisionBySessionRef.current.set(sessionId, revision);
    return revision;
  }, []);

  const replaceSessionTimeline = useCallback(
    (sessionId: string, items: AgentTimelineItem[], expectedRevision?: number): boolean => {
      if (
        expectedRevision !== undefined &&
        (timelineRevisionBySessionRef.current.get(sessionId) || 0) !== expectedRevision
      ) {
        return false;
      }
      bumpTimelineRevision(sessionId);
      if (activeSessionIdRef.current === sessionId) {
        setTimelineItems(items);
      }
      return true;
    },
    [bumpTimelineRevision]
  );

  const mergeSessionTimelineItem = useCallback(
    (sessionId: string, item: AgentTimelineItem) => {
      bumpTimelineRevision(sessionId);
      if (activeSessionIdRef.current === sessionId) {
        setTimelineItems((current) => mergeTimelineItem(current, item));
      }
    },
    [bumpTimelineRevision]
  );

  const clearCompletedUnreadSession = useCallback((sessionId: string) => {
    setCompletedUnreadSessionIds((current) => {
      if (!current.has(sessionId)) return current;
      const next = new Set(current);
      next.delete(sessionId);
      return next;
    });
  }, []);

  const markCompletedUnreadSession = useCallback((sessionId: string) => {
    setCompletedUnreadSessionIds((current) => {
      if (current.has(sessionId)) return current;
      const next = new Set(current);
      next.add(sessionId);
      return next;
    });
  }, []);

  const trackAgentWorkflow = useCallback(
    async <T,>(workflow: () => Promise<T>): Promise<T> => {
      return await agentOperationFence.run(userId, workflow);
    },
    [userId]
  );

  const ensureMapleProxyReady = useCallback(async () => {
    try {
      const status = await trackAgentWorkflow(async () => {
        return await proxyService.ensureProxyReady(
          userId,
          async (name) => {
            const response = await createApiKey(name);
            return response.key;
          },
          async (name) => {
            await deleteApiKey(name);
          }
        );
      });
      setHasManualProxyConflict(false);
      return status;
    } catch (proxyError) {
      if (proxyError instanceof AgentProxyManualConfigConflictError) {
        setHasManualProxyConflict(true);
      }
      throw proxyError;
    }
  }, [createApiKey, deleteApiKey, trackAgentWorkflow, userId]);

  const enqueueProjectRootMutation = useCallback(
    async <T,>(mutation: () => Promise<T>): Promise<T> => {
      const previousOperation = projectRootPersistenceQueues.get(userId) ?? Promise.resolve();
      const operation = (async () => {
        await previousOperation;
        return await trackAgentWorkflow(mutation);
      })();
      const queueTail = operation.then(
        () => undefined,
        () => undefined
      );
      projectRootPersistenceQueues.set(userId, queueTail);
      void queueTail.then(() => {
        if (projectRootPersistenceQueues.get(userId) === queueTail) {
          projectRootPersistenceQueues.delete(userId);
        }
      });
      return await operation;
    },
    [trackAgentWorkflow, userId]
  );

  const persistSelectedProjectRoot = useCallback(
    async (path: string): Promise<void> => {
      await enqueueProjectRootMutation(async () => {
        const config = await agentRuntimeService.loadConfig(userId);
        const nextConfig: AgentConfig = {
          ...config,
          defaultProjectRoot: path
        };
        await agentRuntimeService.saveConfig(userId, nextConfig);
      });
    },
    [enqueueProjectRootMutation, userId]
  );

  const registerProjectRoot = useCallback(
    async (
      path: string,
      orderedVisibleRoots: RecentProjectRoot[]
    ): Promise<RecentProjectRoot[]> => {
      return await enqueueProjectRootMutation(async () => {
        const config = await agentRuntimeService.loadConfig(userId);
        const existingOrder = projectOrderForExistingRegistration(
          orderedVisibleRoots,
          projectOrderState.confirmed,
          path
        );
        // A legacy-capped project can already be visible through session history without being in
        // recent_roots.json. Add only that project after the confirmed roots instead of promoting
        // it or materializing unrelated session-derived projects.
        const roots = existingOrder
          ? await agentRuntimeService.saveProjectRootOrder(userId, existingOrder)
          : await agentRuntimeService.saveRecentProjectRoot(userId, path);
        const nextConfig: AgentConfig = {
          ...config,
          defaultProjectRoot: path
        };
        await agentRuntimeService.saveConfig(userId, nextConfig);
        return roots;
      });
    },
    [enqueueProjectRootMutation, projectOrderState.confirmed, userId]
  );

  const persistProjectRootOrder = useCallback(
    async (roots: RecentProjectRoot[]): Promise<RecentProjectRoot[]> => {
      return await enqueueProjectRootMutation(async () => {
        return await agentRuntimeService.saveProjectRootOrder(
          userId,
          roots.map((root) => root.path)
        );
      });
    },
    [enqueueProjectRootMutation, userId]
  );

  const saveProjectRootOrder = useCallback(
    (nextRoots: RecentProjectRoot[]) => {
      if (projectOrderState.pendingRequestId !== null) return;
      const requestId = projectOrderRequestIdRef.current + 1;
      projectOrderRequestIdRef.current = requestId;
      setError(null);
      dispatchProjectOrder({ type: "optimistic", requestId, roots: nextRoots });

      void persistProjectRootOrder(nextRoots).then(
        (roots) => {
          dispatchProjectOrder({ type: "confirmed", requestId, roots });
        },
        (orderError) => {
          dispatchProjectOrder({ type: "rejected", requestId });
          setError(errorMessage(orderError));
        }
      );
    },
    [persistProjectRootOrder, projectOrderState.pendingRequestId]
  );

  useEffect(() => {
    const generation = projectSkillsTrustGenerationRef.current + 1;
    projectSkillsTrustGenerationRef.current = generation;
    setProjectSkillsTrustPrompt(null);
    setProjectSkillsTrustError(null);
    if (!isAuthTransitionReady || isInitializing || !projectRoot) {
      setIsProjectSkillsTrustLoading(false);
      return;
    }

    setIsProjectSkillsTrustLoading(true);
    void trackAgentWorkflow(() => agentRuntimeService.getProjectSkillsTrust(userId, projectRoot))
      .then((status) => {
        if (projectSkillsTrustGenerationRef.current !== generation) return;
        if (status.available && status.decision == null) {
          setProjectSkillsTrustPrompt(status);
        }
      })
      .catch((trustError) => {
        if (projectSkillsTrustGenerationRef.current === generation) {
          setError(errorMessage(trustError));
        }
      })
      .finally(() => {
        if (projectSkillsTrustGenerationRef.current === generation) {
          setIsProjectSkillsTrustLoading(false);
        }
      });
  }, [isAuthTransitionReady, isInitializing, projectRoot, trackAgentWorkflow, userId]);

  const saveProjectSkillsTrust = useCallback(
    async (trusted: boolean) => {
      const prompt = projectSkillsTrustPrompt;
      if (!prompt || projectSkillsTrustSavingDecision !== null) return;
      setError(null);
      setProjectSkillsTrustError(null);
      setProjectSkillsTrustSavingDecision(trusted);
      try {
        await trackAgentWorkflow(() =>
          agentRuntimeService.setProjectSkillsTrust(userId, prompt.path, trusted)
        );
        setProjectSkillsTrustPrompt((current) => (current?.path === prompt.path ? null : current));
      } catch (trustError) {
        setProjectSkillsTrustError(errorMessage(trustError));
      } finally {
        setProjectSkillsTrustSavingDecision(null);
      }
    },
    [projectSkillsTrustPrompt, projectSkillsTrustSavingDecision, trackAgentWorkflow, userId]
  );

  const refreshSessionList = useCallback(async () => {
    return await trackAgentWorkflow(async () => {
      if (!isTauriDesktop()) return;
      const nextSessions = await agentRuntimeService.listSessions(userId, null);
      setSessions(nextSessions.filter((session) => !deletedSessionIdsRef.current.has(session.id)));
      setIsSessionHistoryReady(true);
    });
  }, [trackAgentWorkflow, userId]);

  const refreshSessions = useCallback(async () => {
    return await trackAgentWorkflow(async () => {
      if (!isTauriDesktop()) return;
      const runStateGeneration = runStateGenerationRef.current;
      const status = await agentRuntimeService.getRuntimeStatus(userId);
      applyRuntimeStatus(status, runStateGeneration);
      if (!status.running) {
        // Session history is account-scoped local data and does not require a
        // live runtime or verified proxy credential.
        await refreshSessionList();
        return;
      }
      await refreshSessionList();
    });
  }, [applyRuntimeStatus, refreshSessionList, trackAgentWorkflow, userId]);

  const refreshSessionMcpServers = useCallback(
    async (sessionId: string) => {
      const generation = mcpSessionLoadGenerationRef.current + 1;
      mcpSessionLoadGenerationRef.current = generation;
      setIsSessionMcpServersLoading(true);
      try {
        const nextServers = await agentRuntimeService.listSessionMcpServers(userId, sessionId);
        if (
          mcpSessionLoadGenerationRef.current === generation &&
          activeSessionIdRef.current === sessionId
        ) {
          setSessionMcpServers(nextServers);
          setSessionMcpServersSessionId(sessionId);
        }
        return nextServers;
      } finally {
        if (mcpSessionLoadGenerationRef.current === generation) {
          setIsSessionMcpServersLoading(false);
        }
      }
    },
    [userId]
  );

  const saveMcpServers = useCallback(
    async (nextServers: AgentMcpServer[]) => {
      const previousServers = mcpServers;
      const savedServers = await agentRuntimeService.saveMcpServers(userId, nextServers);
      setMcpServers(savedServers);
      setNewChatMcpServerNames((current) =>
        reconcileNewChatMcpServerNames(previousServers, savedServers, current)
      );

      const sessionId = activeSessionIdRef.current;
      if (sessionId) {
        void refreshSessionMcpServers(sessionId).catch((loadError) => {
          if (activeSessionIdRef.current === sessionId) {
            setError(errorMessage(loadError));
          }
        });
      }
    },
    [mcpServers, refreshSessionMcpServers, userId]
  );

  const toggleMcpServer = useCallback(
    (name: string, enabled: boolean) => {
      const sessionId = activeSessionIdRef.current;
      if (!sessionId) {
        setNewChatMcpServerNames((current) => {
          const next = new Set(current);
          if (enabled) {
            next.add(name);
          } else {
            next.delete(name);
          }
          return next;
        });
        return;
      }

      const toggleGeneration = mcpToggleGenerationRef.current + 1;
      mcpToggleGenerationRef.current = toggleGeneration;
      setError(null);
      setIsMcpServerTogglePending(true);
      void agentRuntimeService
        .setSessionMcpServerEnabled(userId, sessionId, name, enabled)
        .then((nextServers) => {
          if (activeSessionIdRef.current === sessionId) {
            setSessionMcpServers(nextServers);
            setSessionMcpServersSessionId(sessionId);
          }
        })
        .catch((toggleError) => {
          if (activeSessionIdRef.current === sessionId) {
            setError(errorMessage(toggleError));
          }
        })
        .finally(() => {
          if (mcpToggleGenerationRef.current === toggleGeneration) {
            setIsMcpServerTogglePending(false);
          }
        });
    },
    [userId]
  );

  useEffect(() => {
    mcpToggleGenerationRef.current += 1;
    setIsMcpServerTogglePending(false);
    if (!activeSessionId) {
      mcpSessionLoadGenerationRef.current += 1;
      setSessionMcpServers([]);
      setSessionMcpServersSessionId(null);
      setIsSessionMcpServersLoading(false);
      return;
    }

    void refreshSessionMcpServers(activeSessionId).catch((loadError) => {
      if (activeSessionIdRef.current === activeSessionId) {
        setError(errorMessage(loadError));
      }
    });
  }, [activeSessionId, refreshSessionMcpServers]);

  useEffect(() => {
    let cancelled = false;
    const initializationGeneration = interactionGenerationRef.current;
    setIsInitializing(true);
    async function loadInitialState() {
      if (!isTauriDesktop()) return;
      try {
        // A mode switch can remount AgentMode while the previous mount is still saving a selected
        // root or manual order. Read only after that user-scoped queue reaches its latest tail.
        await (projectRootPersistenceQueues.get(userId) ?? Promise.resolve());
        if (cancelled || interactionGenerationRef.current !== initializationGeneration) {
          return;
        }

        const runStateGeneration = runStateGenerationRef.current;
        const [status, config, roots, savedMcpServers] = await Promise.all([
          agentRuntimeService.getRuntimeStatus(userId),
          agentRuntimeService.loadConfig(userId),
          agentRuntimeService.listRecentProjectRoots(userId),
          agentRuntimeService.listMcpServers(userId)
        ]);
        if (cancelled || interactionGenerationRef.current !== initializationGeneration) {
          return;
        }

        applyRuntimeStatus(status, runStateGeneration);
        dispatchProjectOrder({ type: "replace", roots });
        setMcpServers(savedMcpServers);
        setNewChatMcpServerNames(
          new Set(savedMcpServers.filter((server) => server.enabled).map((server) => server.name))
        );
        setIsMcpServersLoading(false);
        const root = config.defaultProjectRoot || status.projectRoot || roots[0]?.path || "";
        const nextModel = status.model || config.defaultModel || DEFAULT_MODEL;
        const nextMode = normalizeAgentPermissionMode(status.mode);
        setProjectRoot(root);
        setModel(nextModel);
        applyAuthoritativeMode(nextMode);

        // Session history is local account data and remains browseable even
        // when an existing proxy credential requires explicit replacement.
        await refreshSessionList();
        if (cancelled || interactionGenerationRef.current !== initializationGeneration) {
          return;
        }

        await ensureMapleProxyReady();
        if (cancelled || interactionGenerationRef.current !== initializationGeneration) {
          return;
        }
        if (status.running) {
          await refreshSessions();
        } else if (root) {
          const startRunStateGeneration = runStateGenerationRef.current;
          const startedStatus = await agentRuntimeService.startRuntime(userId, {
            projectRoot: root,
            model: nextModel,
            mode: nextMode
          });
          if (cancelled || interactionGenerationRef.current !== initializationGeneration) {
            return;
          }
          applyRuntimeStatus(startedStatus, startRunStateGeneration);
          await refreshSessions();
        }
      } catch (loadError) {
        if (
          !cancelled &&
          interactionGenerationRef.current === initializationGeneration &&
          !(loadError instanceof AgentProxyManualConfigConflictError)
        ) {
          setError(errorMessage(loadError));
        }
      }
    }
    void awaitAgentAuthUser(userId)
      .then(async () => {
        if (cancelled) return;
        setIsAuthTransitionReady(true);
        await trackAgentWorkflow(loadInitialState);
      })
      .catch((loadError) => {
        if (
          !cancelled &&
          interactionGenerationRef.current === initializationGeneration &&
          !(loadError instanceof AgentProxyManualConfigConflictError)
        ) {
          setError(errorMessage(loadError));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setIsInitializing(false);
          setIsMcpServersLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [
    applyAuthoritativeMode,
    applyRuntimeStatus,
    ensureMapleProxyReady,
    refreshSessionList,
    refreshSessions,
    trackAgentWorkflow,
    userId
  ]);

  const chooseProjectRoot = useCallback(async () => {
    if (!isTauriDesktop()) return;
    try {
      await trackAgentWorkflow(async () => {
        const { open } = await import("@tauri-apps/plugin-dialog");
        const selected = await open({
          directory: true,
          multiple: false,
          title: "Select project folder"
        });
        if (typeof selected === "string") {
          invalidateSessionSelection();
          agentSessionSelection.forget(userId);
          shouldAutoScrollRef.current = true;
          setProjectRoot(selected);
          activeSessionIdRef.current = null;
          setActiveSessionId(null);
          setTimelineItems([]);
          setIsProjectRootRegistrationPending(true);
          try {
            const roots = await registerProjectRoot(selected, visibleProjectRootsRef.current);
            dispatchProjectOrder({ type: "replace", roots });
          } finally {
            setIsProjectRootRegistrationPending(false);
          }
        }
      });
    } catch (chooseError) {
      setError(errorMessage(chooseError));
    }
  }, [
    agentSessionSelection,
    invalidateSessionSelection,
    registerProjectRoot,
    trackAgentWorkflow,
    userId
  ]);

  const selectProjectRoot = useCallback(
    (value: string) => {
      invalidateSessionSelection();
      agentSessionSelection.forget(userId);
      const interactionGeneration = interactionGenerationRef.current;
      setProjectRoot(value);
      setActiveSessionId(null);
      activeSessionIdRef.current = null;
      setTimelineItems([]);
      shouldAutoScrollRef.current = true;
      void (async () => {
        try {
          await persistSelectedProjectRoot(value);
          if (interactionGenerationRef.current === interactionGeneration) {
            await refreshSessions();
          }
        } catch (selectError) {
          if (interactionGenerationRef.current === interactionGeneration) {
            setError(errorMessage(selectError));
          }
        }
      })();
    },
    [
      agentSessionSelection,
      invalidateSessionSelection,
      persistSelectedProjectRoot,
      refreshSessions,
      userId
    ]
  );

  const selectModel = useCallback((value: string) => {
    if (isAgentModelLockedRef.current) return;
    interactionGenerationRef.current += 1;
    setModel(value);
  }, []);

  const selectMode = useCallback(
    (value: AgentPermissionMode) => {
      if (value === selectedModeRef.current) return;
      const interactionGeneration = interactionGenerationRef.current + 1;
      interactionGenerationRef.current = interactionGeneration;
      setError(null);

      const sessionId = activeSessionIdRef.current;
      if (!sessionId) {
        applyAuthoritativeMode(value);
        return;
      }
      const updateGeneration = permissionModeUpdateGenerationRef.current + 1;
      permissionModeUpdateGenerationRef.current = updateGeneration;
      setIsPermissionModeUpdating(true);
      // A relaxation may be shown before the backend catches up because that
      // only understates current restrictions. Keep showing Auto during a
      // restrictive transition until the backend has made Read only live, so
      // the selector never promises protection that is not authoritative yet.
      if (value === "auto") {
        selectedModeRef.current = value;
        setMode(value);
      }
      const update = permissionModeUpdateRef.current.then(() =>
        agentRuntimeService.setPermissionMode(userId, sessionId, value)
      );
      permissionModeUpdateRef.current = update
        .then(
          () => {
            if (activeSessionIdRef.current === sessionId) {
              committedModeRef.current = value;
              if (interactionGenerationRef.current === interactionGeneration) {
                selectedModeRef.current = value;
                setMode(value);
              }
            }
          },
          (modeError) => {
            if (
              activeSessionIdRef.current === sessionId &&
              interactionGenerationRef.current === interactionGeneration
            ) {
              selectedModeRef.current = committedModeRef.current;
              setMode(committedModeRef.current);
              setError(errorMessage(modeError));
            }
          }
        )
        .finally(() => {
          if (permissionModeUpdateGenerationRef.current === updateGeneration) {
            setIsPermissionModeUpdating(false);
          }
        });
    },
    [applyAuthoritativeMode, userId]
  );

  const startRuntime = useCallback(
    async (restart = false) => {
      const requestGeneration = startRequestGenerationRef.current + 1;
      startRequestGenerationRef.current = requestGeneration;
      const interactionGeneration = interactionGenerationRef.current;
      setError(null);
      setIsStarting(true);
      try {
        return await trackAgentWorkflow(async () => {
          if (!projectRoot) {
            throw new Error("Select a project folder first");
          }
          await ensureMapleProxyReady();
          const requestedMode = selectedModeRef.current;
          const request = { projectRoot, model: model || DEFAULT_MODEL, mode: requestedMode };
          const runStateGeneration = runStateGenerationRef.current;
          const status = restart
            ? await agentRuntimeService.restartRuntime(userId, request)
            : await agentRuntimeService.startRuntime(userId, request);
          if (
            startRequestGenerationRef.current !== requestGeneration ||
            interactionGenerationRef.current !== interactionGeneration
          ) {
            return status;
          }
          applyRuntimeStatus(status, runStateGeneration);
          setProjectRoot(status.projectRoot || projectRoot);
          setModel(status.model || model || DEFAULT_MODEL);
          applyAuthoritativeMode(normalizeAgentPermissionMode(status.mode || requestedMode));
          await refreshSessions();
          return status;
        });
      } catch (startError) {
        if (
          startRequestGenerationRef.current === requestGeneration &&
          interactionGenerationRef.current === interactionGeneration &&
          !(startError instanceof AgentProxyManualConfigConflictError)
        ) {
          setError(errorMessage(startError));
        }
        throw startError;
      } finally {
        if (startRequestGenerationRef.current === requestGeneration) {
          setIsStarting(false);
        }
      }
    },
    [
      applyAuthoritativeMode,
      applyRuntimeStatus,
      ensureMapleProxyReady,
      model,
      projectRoot,
      refreshSessions,
      trackAgentWorkflow,
      userId
    ]
  );

  const replaceManualProxyForAgent = useCallback(async () => {
    interactionGenerationRef.current += 1;
    setError(null);
    setIsReplacingManualProxy(true);
    try {
      await trackAgentWorkflow(async () => {
        await proxyService.replaceOwnerlessProxyAndEnsureReady(
          userId,
          async (name) => {
            const response = await createApiKey(name);
            return response.key;
          },
          async (name) => {
            await deleteApiKey(name);
          }
        );
      });
      setHasManualProxyConflict(false);
      if (projectRoot) {
        await startRuntime(Boolean(runtimeStatus?.running));
      }
    } catch (replaceError) {
      if (replaceError instanceof AgentProxyManualConfigConflictError) {
        setHasManualProxyConflict(true);
      } else if (replaceError instanceof AgentProxyReplacementSetupError) {
        setHasManualProxyConflict(false);
        setError(replaceError.message);
      } else {
        setError(errorMessage(replaceError));
      }
    } finally {
      setIsReplacingManualProxy(false);
    }
  }, [
    createApiKey,
    deleteApiKey,
    projectRoot,
    runtimeStatus?.running,
    startRuntime,
    trackAgentWorkflow,
    userId
  ]);

  const ensureRuntimeAndSession = useCallback(
    async (
      expectedSelectionGeneration: number,
      expectedInteractionGeneration: number,
      requestedSessionId: string | null
    ) => {
      if (!projectRoot) {
        throw new Error("Select a project folder first");
      }

      const status = await agentRuntimeService.getRuntimeStatus(userId);
      if (!status.running) {
        await startRuntime(false);
      }

      let sessionId = requestedSessionId;
      if (!sessionId) {
        const detail = await agentRuntimeService.createSession(userId, {
          projectRoot,
          title: "New task",
          model: model || DEFAULT_MODEL,
          mode: selectedModeRef.current,
          mcpServerNames: selectedNewChatMcpServerNames
        });
        // Goose may reuse the newest deleted session ID. This detail represents
        // a new persisted session, so it supersedes any local deletion tombstone.
        deletedSessionIdsRef.current.delete(detail.session.id);
        sessionId = detail.session.id;
        setSessions((current) => [
          detail.session,
          ...current.filter((item) => item.id !== detail.session.id)
        ]);
        replaceSessionTimeline(sessionId, detail.timeline);

        // A send that creates a session may finish after the user selects a
        // different chat. Keep the new chat/run, but never steal focus back.
        if (
          isAgentModeMountedRef.current &&
          sessionSelectionGenerationRef.current === expectedSelectionGeneration &&
          interactionGenerationRef.current === expectedInteractionGeneration &&
          activeSessionIdRef.current === null
        ) {
          shouldAutoScrollRef.current = true;
          activeSessionIdRef.current = sessionId;
          setActiveSessionId(sessionId);
          agentSessionSelection.remember(userId, sessionId);
          applyAuthoritativeMode(normalizeAgentPermissionMode(detail.session.mode));
          replaceSessionTimeline(sessionId, detail.timeline);
          const mcpError = mcpConnectionErrorMessage(detail.mcpErrors);
          if (mcpError) setError(mcpError);
        }
      }

      return sessionId;
    },
    [
      applyAuthoritativeMode,
      agentSessionSelection,
      model,
      projectRoot,
      replaceSessionTimeline,
      selectedNewChatMcpServerNames,
      startRuntime,
      userId
    ]
  );

  const createSession = useCallback(async () => {
    if (pendingSessionSelectionIdRef.current === NEW_SESSION_PENDING_KEY) return;
    const selectionGeneration = beginSessionSelection(NEW_SESSION_PENDING_KEY);
    const interactionGeneration = interactionGenerationRef.current;
    setError(null);
    try {
      const detail = await trackAgentWorkflow(async () => {
        if (!runtimeStatus?.running) {
          await startRuntime(false);
        }
        return await agentRuntimeService.createSession(userId, {
          projectRoot,
          title: "New task",
          model: model || DEFAULT_MODEL,
          mode: selectedModeRef.current,
          mcpServerNames: selectedNewChatMcpServerNames
        });
      });
      deletedSessionIdsRef.current.delete(detail.session.id);
      setSessions((current) => [
        detail.session,
        ...current.filter((session) => session.id !== detail.session.id)
      ]);
      replaceSessionTimeline(detail.session.id, detail.timeline);

      if (
        isAgentModeMountedRef.current &&
        sessionSelectionGenerationRef.current === selectionGeneration &&
        interactionGenerationRef.current === interactionGeneration
      ) {
        shouldAutoScrollRef.current = true;
        activeSessionIdRef.current = detail.session.id;
        setActiveSessionId(detail.session.id);
        agentSessionSelection.remember(userId, detail.session.id);
        applyAuthoritativeMode(normalizeAgentPermissionMode(detail.session.mode));
        replaceSessionTimeline(detail.session.id, detail.timeline);
        const mcpError = mcpConnectionErrorMessage(detail.mcpErrors);
        if (mcpError) setError(mcpError);
      }
    } catch (createError) {
      if (
        isAgentModeMountedRef.current &&
        sessionSelectionGenerationRef.current === selectionGeneration &&
        interactionGenerationRef.current === interactionGeneration
      ) {
        setError(errorMessage(createError));
      }
    } finally {
      if (isAgentModeMountedRef.current) {
        finishSessionSelection(selectionGeneration);
      }
    }
  }, [
    applyAuthoritativeMode,
    agentSessionSelection,
    beginSessionSelection,
    finishSessionSelection,
    model,
    projectRoot,
    replaceSessionTimeline,
    runtimeStatus?.running,
    selectedNewChatMcpServerNames,
    startRuntime,
    trackAgentWorkflow,
    userId
  ]);

  const loadSession = useCallback(
    async (sessionId: string) => {
      const selectionGeneration = beginSessionSelection(sessionId);
      const interactionGeneration = interactionGenerationRef.current;
      setError(null);
      clearCompletedUnreadSession(sessionId);
      try {
        const loaded = await trackAgentWorkflow(async () => {
          for (let attempt = 0; attempt < MAX_STABLE_SESSION_LOAD_ATTEMPTS; attempt += 1) {
            const timelineRevision = timelineRevisionBySessionRef.current.get(sessionId) || 0;
            const detail = await agentRuntimeService.loadSession(userId, sessionId);
            if ((timelineRevisionBySessionRef.current.get(sessionId) || 0) === timelineRevision) {
              return { detail, timelineRevision };
            }
          }
          throw new Error("This task is still updating. Try selecting it again shortly.");
        });
        const { detail, timelineRevision } = loaded;
        if (
          !isAgentModeMountedRef.current ||
          sessionSelectionGenerationRef.current !== selectionGeneration ||
          interactionGenerationRef.current !== interactionGeneration ||
          deletedSessionIdsRef.current.has(sessionId)
        ) {
          return;
        }

        // Validate and install the snapshot before switching focus. A live
        // event can arrive between the native read and this continuation; in
        // that case leave the previous chat intact instead of overwriting the
        // newer timeline with a stale snapshot.
        if (!replaceSessionTimeline(detail.session.id, detail.timeline, timelineRevision)) {
          throw new Error("This task changed while loading. Try selecting it again.");
        }

        // Commit the selected session and all of its settings together. Until
        // this point the previous chat remains active and its composer is gated.
        shouldAutoScrollRef.current = true;
        activeSessionIdRef.current = detail.session.id;
        setActiveSessionId(detail.session.id);
        agentSessionSelection.remember(userId, detail.session.id);
        setProjectRoot(detail.session.projectRoot);
        if (detail.session.model) {
          setModel(detail.session.model);
        }
        applyAuthoritativeMode(normalizeAgentPermissionMode(detail.session.mode));
        setTimelineItems(detail.timeline);
        if (activeRunsBySessionRef.current[detail.session.id]) {
          thoughtPhaseTrackerRef.current.seedActiveTimeline(detail.session.id, detail.timeline);
        }
        const mcpError = mcpConnectionErrorMessage(detail.mcpErrors);
        if (mcpError) setError(mcpError);
        finishSessionSelection(selectionGeneration);

        try {
          await persistSelectedProjectRoot(detail.session.projectRoot);
        } catch (persistError) {
          if (
            isAgentModeMountedRef.current &&
            sessionSelectionGenerationRef.current === selectionGeneration &&
            interactionGenerationRef.current === interactionGeneration &&
            activeSessionIdRef.current === detail.session.id
          ) {
            setError(errorMessage(persistError));
          }
        }
      } catch (loadError) {
        if (
          isAgentModeMountedRef.current &&
          sessionSelectionGenerationRef.current === selectionGeneration &&
          interactionGenerationRef.current === interactionGeneration
        ) {
          setError(errorMessage(loadError));
        }
      } finally {
        if (isAgentModeMountedRef.current) {
          finishSessionSelection(selectionGeneration);
        }
      }
    },
    [
      applyAuthoritativeMode,
      agentSessionSelection,
      beginSessionSelection,
      clearCompletedUnreadSession,
      finishSessionSelection,
      persistSelectedProjectRoot,
      replaceSessionTimeline,
      trackAgentWorkflow,
      userId
    ]
  );

  useEffect(() => {
    if (
      hasAttemptedSessionRestoreRef.current ||
      !isAuthTransitionReady ||
      isInitializing ||
      !isSessionHistoryReady
    ) {
      return;
    }

    hasAttemptedSessionRestoreRef.current = true;
    const rememberedSessionId = agentSessionSelection.resolve(userId, sessions);
    if (rememberedSessionId) {
      void loadSession(rememberedSessionId);
    }
  }, [
    agentSessionSelection,
    isAuthTransitionReady,
    isInitializing,
    isSessionHistoryReady,
    loadSession,
    sessions,
    userId
  ]);

  const sendMessage = useCallback(async () => {
    const text = input.trim();
    const requestedSessionId = activeSessionIdRef.current;
    let pendingSessionKey = requestedSessionId || NEW_SESSION_PENDING_KEY;
    if (
      !text ||
      isAgentSendLocked ||
      pendingSessionSelectionIdRef.current !== null ||
      pendingSendTokensRef.current.has(pendingSessionKey) ||
      (requestedSessionId && activeRunsBySession[requestedSessionId])
    ) {
      return;
    }

    const selectionGeneration = sessionSelectionGenerationRef.current;
    const interactionGeneration = interactionGenerationRef.current;
    const sendToken = nextSendTokenRef.current + 1;
    nextSendTokenRef.current = sendToken;
    let targetSessionId = requestedSessionId;
    markPendingSend(pendingSessionKey, sendToken);

    setError(null);
    setInput("");
    shouldAutoScrollRef.current = true;
    requestAnimationFrame(() => scrollTimelineToBottom("smooth"));
    try {
      await trackAgentWorkflow(async () => {
        const sessionId = await ensureRuntimeAndSession(
          selectionGeneration,
          interactionGeneration,
          requestedSessionId
        );
        targetSessionId = sessionId;
        if (pendingSessionKey !== sessionId) {
          movePendingSend(pendingSessionKey, sessionId, sendToken);
          pendingSessionKey = sessionId;
        }
        if (cancelledPendingSendTokensRef.current.has(sendToken)) {
          throw new PendingAgentSendCancelledError();
        }
        // The selector reflects only committed policy. Wait for any in-flight
        // update so this send cannot replay a stale mode snapshot afterward.
        await permissionModeUpdateRef.current;
        if (cancelledPendingSendTokensRef.current.has(sendToken)) {
          throw new PendingAgentSendCancelledError();
        }
        thoughtPhaseTrackerRef.current.prepareUserRequest(sessionId, text);
        const response = await agentRuntimeService.sendMessage(userId, {
          sessionId,
          text,
          model: model || DEFAULT_MODEL,
          mode: selectedModeRef.current,
          visionCapable: resolveAgentModelVisionCapability(
            model || DEFAULT_MODEL,
            availableModels,
            modelAliases
          )
        });
        if (cancelledPendingSendTokensRef.current.has(sendToken)) {
          // The native command may have crossed the start boundary while the
          // user clicked Cancel. Cancel the concrete run before returning.
          await agentRuntimeService.cancelRun(userId, response.runId);
          return;
        }
        if (!terminalRunIdsRef.current.has(response.runId)) {
          recordActiveRun(sessionId, response.runId);
        }
      });
    } catch (sendError) {
      if (sendError instanceof PendingAgentSendCancelledError) {
        setInput((current) => (current ? current : text));
        return;
      }
      const message = errorMessage(sendError);
      if (
        (targetSessionId && activeSessionIdRef.current === targetSessionId) ||
        (!targetSessionId &&
          activeSessionIdRef.current === null &&
          sessionSelectionGenerationRef.current === selectionGeneration &&
          interactionGenerationRef.current === interactionGeneration)
      ) {
        setError(message);
      }
      if (targetSessionId && !deletedSessionIdsRef.current.has(targetSessionId)) {
        mergeSessionTimelineItem(targetSessionId, {
          id: `error-${Date.now()}-${sendToken}`,
          itemType: "error",
          role: "system",
          title: "Agent error",
          text: message,
          status: "failed",
          createdMs: Date.now(),
          merge: "replace"
        });
      }
    } finally {
      cancelledPendingSendTokensRef.current.delete(sendToken);
      clearPendingSend(pendingSessionKey, sendToken);
    }
  }, [
    activeRunsBySession,
    availableModels,
    clearPendingSend,
    ensureRuntimeAndSession,
    input,
    isAgentSendLocked,
    markPendingSend,
    mergeSessionTimelineItem,
    model,
    modelAliases,
    movePendingSend,
    recordActiveRun,
    scrollTimelineToBottom,
    trackAgentWorkflow,
    userId
  ]);

  const cancelPrompt = useCallback(async () => {
    const sessionId = activeSessionIdRef.current;
    const currentRunId = sessionId ? activeRunsBySessionRef.current[sessionId] : activeRunId;
    if (!currentRunId) {
      const pendingSessionKey = activeSessionIdRef.current || NEW_SESSION_PENDING_KEY;
      const pendingSendToken = pendingSendTokensRef.current.get(pendingSessionKey);
      if (pendingSendToken !== undefined) {
        cancelledPendingSendTokensRef.current.add(pendingSendToken);
      }
      return;
    }
    try {
      await agentRuntimeService.cancelRun(userId, currentRunId);
    } catch (cancelError) {
      if (activeSessionIdRef.current === sessionId) {
        setError(errorMessage(cancelError));
      }
    }
  }, [activeRunId, userId]);

  const respondToPermission = useCallback(
    async (item: AgentTimelineItem, decision: AgentPermissionDecision) => {
      const sessionId = activeSessionIdRef.current;
      try {
        if (!sessionId) throw new Error("No active task for this permission request");
        await agentRuntimeService.respondToPermission(
          userId,
          sessionId,
          permissionRequestId(item),
          decision
        );
        // Rust emits the authoritative revision-aware timelineItem before this
        // command returns. Replacing a render-closure snapshot here could erase
        // tool output that arrived while the permission response was in flight.
      } catch (permissionError) {
        if (activeSessionIdRef.current === sessionId) {
          setError(errorMessage(permissionError));
        }
      }
    },
    [userId]
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
  const removeSessionFromState = useCallback(
    (sessionId: string) => {
      deletedSessionIdsRef.current.add(sessionId);
      thoughtPhaseTrackerRef.current.forgetSession(sessionId);
      cancelThoughtLabelDisplays(sessionId);
      thoughtLabelGenerationBySessionRef.current.set(
        sessionId,
        (thoughtLabelGenerationBySessionRef.current.get(sessionId) ?? 0) + 1
      );
      agentSessionSelection.forget(userId, sessionId);
      timelineRevisionBySessionRef.current.delete(sessionId);
      setSessions((current) => current.filter((session) => session.id !== sessionId));
      setCompletedUnreadSessionIds((current) => {
        if (!current.has(sessionId)) return current;
        const next = new Set(current);
        next.delete(sessionId);
        return next;
      });
      setGeneratedThoughtLabels((current) => {
        if (!current[sessionId]) return current;
        const next = { ...current };
        delete next[sessionId];
        return next;
      });
      clearActiveRun(sessionId);
      setSessionToDelete((current) => (current?.id === sessionId ? null : current));

      if (activeSessionIdRef.current === sessionId) {
        activeSessionIdRef.current = null;
        shouldAutoScrollRef.current = true;
        setActiveSessionId(null);
        setTimelineItems([]);
        setInput("");
      }
    },
    [agentSessionSelection, cancelThoughtLabelDisplays, clearActiveRun, userId]
  );

  const deleteSession = useCallback(
    async (sessionId: string) => {
      setError(null);
      try {
        await agentRuntimeService.deleteSession(userId, sessionId);
        removeSessionFromState(sessionId);
        // A mode switch can remount AgentMode while native deletion is pending.
        // Notify the current mount so it cannot keep or restore the deleted session.
        window.dispatchEvent(
          new CustomEvent(AGENT_SESSION_DELETED_EVENT, { detail: { userId, sessionId } })
        );
      } catch (deleteError) {
        setError(errorMessage(deleteError));
      }
    },
    [removeSessionFromState, userId]
  );

  useEffect(() => {
    const handleSessionDeleted = (event: Event) => {
      if (!(event instanceof CustomEvent)) return;
      const detail = event.detail as { userId?: unknown; sessionId?: unknown } | null;
      if (detail?.userId === userId && typeof detail.sessionId === "string" && detail.sessionId) {
        removeSessionFromState(detail.sessionId);
      }
    };

    window.addEventListener(AGENT_SESSION_DELETED_EVENT, handleSessionDeleted);
    return () => {
      window.removeEventListener(AGENT_SESSION_DELETED_EVENT, handleSessionDeleted);
    };
  }, [removeSessionFromState, userId]);

  const upsertSessionSummary = useCallback((summary: AgentSessionSummary) => {
    if (deletedSessionIdsRef.current.has(summary.id)) return;
    setSessions((current) => {
      let replaced = false;
      const next = current.map((session) => {
        if (session.id !== summary.id) return session;
        replaced = true;
        return summary;
      });
      return replaced ? next : [summary, ...current];
    });
  }, []);

  const observeLiveThoughtItem = useCallback(
    (sessionId: string, item: AgentTimelineItem) => {
      const completedPhase = thoughtPhaseTrackerRef.current.observeTimelineItem(sessionId, item);
      if (completedPhase) generateThoughtLabel(completedPhase);
    },
    [generateThoughtLabel]
  );

  const handleAgentEvent = useCallback(
    (event: AgentEventEnvelope) => {
      const eventSessionId = event.sessionId || event.session?.id;
      if (eventSessionId && deletedSessionIdsRef.current.has(eventSessionId)) {
        return;
      }

      switch (event.eventType) {
        case "runtimeStatus":
          if (event.status) {
            runStateGenerationRef.current += 1;
            applyRuntimeStatus(event.status);
          }
          break;
        case "sessionCreated":
          if (event.session) {
            upsertSessionSummary(event.session);
          }
          break;
        case "sessionUpdated":
          if (event.session) {
            upsertSessionSummary(event.session);
          }
          break;
        case "runStarted":
          runStateGenerationRef.current += 1;
          if (event.sessionId && event.runId && !terminalRunIdsRef.current.has(event.runId)) {
            bumpTimelineRevision(event.sessionId);
            clearCompletedUnreadSession(event.sessionId);
            recordActiveRun(event.sessionId, event.runId);
          }
          break;
        case "timelineItem":
          if (event.item && event.sessionId) {
            observeLiveThoughtItem(event.sessionId, event.item);
            mergeSessionTimelineItem(event.sessionId, event.item);
          }
          break;
        case "runFinished": {
          runStateGenerationRef.current += 1;
          if (event.runId) terminalRunIdsRef.current.add(event.runId);
          let finishedTimelineRevision: number | undefined;
          if (event.sessionId) {
            finishedTimelineRevision = bumpTimelineRevision(event.sessionId);
            clearActiveRun(event.sessionId, event.runId || undefined);
            if (event.message === "completed") {
              const completedPhase = thoughtPhaseTrackerRef.current.finishRun(event.sessionId);
              if (completedPhase) generateThoughtLabel(completedPhase);
            } else {
              const discardedTurnId = thoughtPhaseTrackerRef.current.discardRun(event.sessionId);
              if (event.message === "cancelled") {
                if (discardedTurnId) {
                  invalidateThoughtLabelsForTurn(event.sessionId, discardedTurnId);
                } else {
                  invalidateThoughtLabelsForSession(event.sessionId);
                }
              }
            }
          }
          // The terminal event is authoritative for run state. Refresh only
          // persisted session metadata here: the native task removes its
          // active-run entry immediately after emitting this event, so a
          // concurrent status snapshot could otherwise resurrect the run.
          void refreshSessionList().catch(() => {});
          if (event.sessionId && (event.message === "completed" || event.message === "cancelled")) {
            if (event.message === "completed" && event.sessionId !== activeSessionIdRef.current) {
              markCompletedUnreadSession(event.sessionId);
            }
            void agentRuntimeService
              .loadSession(userId, event.sessionId)
              .then((detail) => {
                if (!deletedSessionIdsRef.current.has(event.sessionId!)) {
                  replaceSessionTimeline(
                    event.sessionId!,
                    detail.timeline,
                    finishedTimelineRevision
                  );
                }
              })
              .catch(() => {});
          }
          break;
        }
        case "error":
          if (event.message && !event.sessionId) {
            setError(userFacingAgentError(event.message));
            const sessionId = activeSessionIdRef.current;
            if (sessionId && isMcpConnectionErrorEvent(event.message)) {
              void refreshSessionMcpServers(sessionId).catch(() => {});
            }
          }
          if (event.item && event.sessionId) {
            observeLiveThoughtItem(event.sessionId, event.item);
            mergeSessionTimelineItem(event.sessionId, event.item);
          }
          break;
        case "historyReplaced":
          void (async () => {
            const id = event.sessionId || activeSessionIdRef.current;
            if (!id) return;
            const invalidatedTurnId = thoughtPhaseTrackerRef.current.resetForHistoryReplacement(id);
            if (invalidatedTurnId) {
              invalidateThoughtLabelsForTurn(id, invalidatedTurnId);
            } else {
              // A remount can observe replacement before this tracker has seen
              // the active turn. Invalidate immediately so old requests cannot
              // race the authoritative load or survive an empty replacement.
              invalidateThoughtLabelsForSession(id);
            }
            const historyTimelineRevision = bumpTimelineRevision(id);
            try {
              const detail = await agentRuntimeService.loadSession(userId, id);
              if (!deletedSessionIdsRef.current.has(id)) {
                const replaced = replaceSessionTimeline(
                  id,
                  detail.timeline,
                  historyTimelineRevision
                );
                if (replaced && activeRunsBySessionRef.current[id]) {
                  thoughtPhaseTrackerRef.current.seedActiveTimeline(id, detail.timeline);
                }
              }
            } catch (historyError) {
              if (activeSessionIdRef.current === id) {
                setError(errorMessage(historyError));
              }
            }
          })();
          break;
      }
    },
    [
      applyRuntimeStatus,
      bumpTimelineRevision,
      clearActiveRun,
      clearCompletedUnreadSession,
      generateThoughtLabel,
      invalidateThoughtLabelsForSession,
      invalidateThoughtLabelsForTurn,
      markCompletedUnreadSession,
      mergeSessionTimelineItem,
      observeLiveThoughtItem,
      refreshSessionList,
      recordActiveRun,
      refreshSessionMcpServers,
      replaceSessionTimeline,
      upsertSessionSummary,
      userId
    ]
  );

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    void awaitAgentAuthUser(userId)
      .then(async () => {
        return await agentRuntimeService.listenToEvents((event) => {
          if (!cancelled) handleAgentEvent(event);
        });
      })
      .then((nextUnlisten) => {
        if (cancelled) {
          nextUnlisten();
        } else {
          unlisten = nextUnlisten;
        }
      })
      .catch((listenError) => {
        if (!cancelled) setError(errorMessage(listenError));
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [handleAgentEvent, userId]);

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
            activeSessionId={
              pendingSessionSelectionId && pendingSessionSelectionId !== NEW_SESSION_PENDING_KEY
                ? pendingSessionSelectionId
                : activeSessionId
            }
            isCompactLayout={isCompactLayout}
            projectRoot={projectRoot}
            recentRoots={visibleProjectRoots}
            completedUnreadSessionIds={completedUnreadSessionIds}
            disabled={areAgentSettingsLocked}
            runningSessionIds={runningSessionIds}
            sessions={sessions}
            onChooseProjectRoot={chooseProjectRoot}
            onCreateSession={() => void createSession()}
            onProjectOrderChange={saveProjectRootOrder}
            onProjectRootChange={selectProjectRoot}
            onSessionDelete={setSessionToDelete}
            onSessionSelect={(sessionId) => void loadSession(sessionId)}
          />
        }
        onNewItem={areAgentSettingsLocked ? undefined : () => void createSession()}
        onToggle={toggleSidebar}
      />

      {sessionToDelete ? (
        <DeleteChatDialog
          open
          onOpenChange={(open) => {
            if (!open) setSessionToDelete(null);
          }}
          chatTitle={sessionTitle(sessionToDelete)}
          itemLabel="task"
          description={`This will permanently delete the task "${sessionTitle(sessionToDelete)}". This action cannot be undone.`}
          onConfirm={() => void deleteSession(sessionToDelete.id)}
        />
      ) : null}

      <AgentMcpServersDialog
        open={isMcpServersDialogOpen}
        servers={mcpServers}
        disabled={!isAuthTransitionReady || isInitializing}
        onOpenChange={setIsMcpServersDialogOpen}
        onSave={saveMcpServers}
      />

      <AlertDialog open={projectSkillsTrustPrompt !== null}>
        <AlertDialogContent onEscapeKeyDown={(event) => event.preventDefault()}>
          <AlertDialogHeader>
            <AlertDialogTitle>Trust this folder?</AlertDialogTitle>
            <AlertDialogDescription>
              Project skills can add local instructions and supporting files that guide the agent.
              Maple&apos;s existing tool permissions still apply to anything those instructions ask
              it to do. Personal skills remain available either way, and Maple remembers this choice
              for this folder.
            </AlertDialogDescription>
          </AlertDialogHeader>
          {projectSkillsTrustPrompt ? (
            <div className="rounded-md border bg-muted/40 px-3 py-2 font-mono text-xs break-all">
              {projectSkillsTrustPrompt.path}
            </div>
          ) : null}
          {projectSkillsTrustError ? (
            <p className="text-sm text-destructive" role="alert" aria-live="assertive">
              {projectSkillsTrustError}
            </p>
          ) : null}
          <AlertDialogFooter>
            <AlertDialogCancel
              disabled={isProjectSkillsTrustSaving}
              onClick={() => void saveProjectSkillsTrust(false)}
            >
              {projectSkillsTrustSavingDecision === false ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : null}
              Continue without project skills
            </AlertDialogCancel>
            <AlertDialogAction
              disabled={isProjectSkillsTrustSaving}
              onClick={() => void saveProjectSkillsTrust(true)}
            >
              {projectSkillsTrustSavingDecision === true ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : null}
              Trust folder
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

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

        {timelineItems.length > 0 ? (
          <ChatDesktopConversationHeader
            title={activeSessionTitle}
            isSidebarOpen={isSidebarOpen}
            onNewChat={() => void createSession()}
            newItemLabel="New Task"
          />
        ) : null}

        {hasManualProxyConflict && (
          <div className="mx-auto mt-3 w-full max-w-6xl px-4">
            <div className="flex flex-col gap-3 rounded-md border border-maple-warning/40 bg-maple-warning/10 px-3 py-3 text-sm sm:flex-row sm:items-center sm:justify-between">
              <div className="flex min-w-0 items-start gap-2">
                <AlertCircle className="mt-0.5 h-4 w-4 shrink-0 text-maple-warning" />
                <div className="min-w-0">
                  <p className="font-medium">Saved local proxy credential</p>
                  <p className="mt-0.5 text-xs text-muted-foreground">
                    Maple cannot verify that this existing Local OpenAI Proxy key belongs to the
                    signed-in account. This can happen once after upgrading from an older Agent Mode
                    build. Your tasks remain available; replace the saved local setup before sending
                    another message. The existing backend key will remain in API Management.
                  </p>
                </div>
              </div>
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="shrink-0"
                disabled={isReplacingManualProxy}
                onClick={() => void replaceManualProxyForAgent()}
              >
                {isReplacingManualProxy ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
                Replace local setup
              </Button>
            </div>
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
            className="relative flex min-h-0 flex-1 flex-col overflow-y-auto overscroll-y-contain"
            onScroll={updateAutoScrollFromPosition}
          >
            <div
              className={cn(
                "mx-auto w-full p-4 md:p-6 landscape-short:p-2",
                timelineItems.length === 0 && isAgentFullscreen
                  ? "flex min-h-full max-w-6xl flex-col"
                  : "max-w-4xl"
              )}
            >
              {timelineItems.length === 0 ? (
                <EmptyAgentState
                  activeRootLabel={activeRootLabel}
                  areSettingsDisabled={areAgentSettingsLocked}
                  input={input}
                  isSendDisabled={isAgentSendLocked}
                  isSending={isSending}
                  isStarting={isStarting}
                  isMcpLoading={isComposerMcpLoading}
                  isMcpToggleDisabled={isMcpToggleDisabled}
                  isModelSelectionDisabled={isAgentModelSelectionDisabled}
                  mcpServers={composerMcpServers}
                  mode={mode}
                  model={model}
                  projectRoot={projectRoot}
                  recentRoots={visibleProjectRoots}
                  isExpanded={isAgentFullscreen}
                  onCancelPrompt={cancelPrompt}
                  onChooseProjectRoot={chooseProjectRoot}
                  onInputChange={setInput}
                  onKeyDown={handleKeyDown}
                  onManageMcpServers={() => setIsMcpServersDialogOpen(true)}
                  onMcpToggle={toggleMcpServer}
                  onModeChange={selectMode}
                  onModelChange={selectModel}
                  onProjectRootChange={selectProjectRoot}
                  onSendMessage={() => void sendMessage()}
                  onToggleExpanded={() => setIsAgentFullscreen((current) => !current)}
                />
              ) : (
                <AgentTimeline
                  items={timelineItems}
                  isResponsePending={isSending}
                  isRunActive={Boolean(activeRunId) && !isSubmitting}
                  generatedThoughtLabels={generatedThoughtLabels}
                  sessionId={activeSessionId}
                  userId={userId}
                  onPermissionDecision={respondToPermission}
                />
              )}
            </div>
          </div>

          {timelineItems.length > 0 ? (
            <div className="shrink-0 bg-background pb-[env(safe-area-inset-bottom)]">
              <div className="mx-auto max-w-4xl px-4 landscape-short:px-3">
                <AgentComposer
                  activeRootLabel={activeRootLabel}
                  areSettingsDisabled={areAgentSettingsLocked}
                  input={input}
                  isSendDisabled={isAgentSendLocked}
                  isSending={isSending}
                  isStarting={isStarting}
                  isMcpLoading={isComposerMcpLoading}
                  isMcpToggleDisabled={isMcpToggleDisabled}
                  isModelSelectionDisabled={isAgentModelSelectionDisabled}
                  mcpServers={composerMcpServers}
                  mode={mode}
                  model={model}
                  projectRoot={projectRoot}
                  recentRoots={visibleProjectRoots}
                  onCancelPrompt={cancelPrompt}
                  onChooseProjectRoot={chooseProjectRoot}
                  onInputChange={setInput}
                  onKeyDown={handleKeyDown}
                  onManageMcpServers={() => setIsMcpServersDialogOpen(true)}
                  onMcpToggle={toggleMcpServer}
                  onModeChange={selectMode}
                  onModelChange={selectModel}
                  onProjectRootChange={selectProjectRoot}
                  onSendMessage={() => void sendMessage()}
                />
                <p className="mb-2 mt-1 text-center text-[10px] text-muted-foreground/50 landscape-short:mb-1">
                  AI can make mistakes. Check important info.
                </p>
              </div>
            </div>
          ) : null}
        </section>
      </div>
    </div>
  );
}

function EmptyAgentState(props: AgentComposerProps) {
  const isExpanded = props.isExpanded ?? false;

  return (
    <div
      className={cn(
        "flex items-center justify-center",
        isExpanded ? "min-h-0 flex-1" : "min-h-[52vh]"
      )}
    >
      <div className="flex w-full flex-col items-center gap-6 text-center landscape-short:gap-3">
        {!isExpanded ? (
          <h1 className="mb-6 overflow-visible pb-1 font-displayWide text-4xl font-normal leading-relaxed brand-gradient-text landscape-short:mb-2 landscape-short:text-2xl">
            Work in a folder...
          </h1>
        ) : null}
        <AgentComposer {...props} />
        {!isExpanded ? (
          <p className="flex items-center justify-center gap-1 text-center text-xs text-muted-foreground/60">
            <Lock className="h-3 w-3" />
            Encrypted and private at every step
          </p>
        ) : null}
      </div>
    </div>
  );
}

interface AgentSidebarContentProps {
  activeSessionId: string | null;
  isCompactLayout: boolean;
  projectRoot: string;
  recentRoots: RecentProjectRoot[];
  completedUnreadSessionIds: Set<string>;
  disabled: boolean;
  runningSessionIds: Set<string>;
  sessions: AgentSessionSummary[];
  onChooseProjectRoot: () => void;
  onCreateSession: () => void;
  onProjectOrderChange: (roots: RecentProjectRoot[]) => void;
  onProjectRootChange: (value: string) => void;
  onSessionDelete: (session: AgentSessionSummary) => void;
  onSessionSelect: (sessionId: string) => void;
}

interface PendingProjectPointer {
  pointerId: number;
  path: string;
  name: string;
  startX: number;
  startY: number;
  grabOffsetX: number;
  grabOffsetY: number;
  ghostWidth: number;
  headerElement: HTMLElement;
}

interface ProjectDragState extends PendingProjectPointer {
  clientX: number;
  clientY: number;
  insertionIndex: number | null;
  markerTop: number | null;
}

function AgentSidebarContent({
  activeSessionId,
  isCompactLayout,
  projectRoot,
  recentRoots,
  completedUnreadSessionIds,
  disabled,
  runningSessionIds,
  sessions,
  onChooseProjectRoot,
  onCreateSession,
  onProjectOrderChange,
  onProjectRootChange,
  onSessionDelete,
  onSessionSelect
}: AgentSidebarContentProps) {
  const rowElementsRef = useRef(new Map<string, HTMLElement>());
  const previousRowTopsRef = useRef(new Map<string, number>());
  const projectListRef = useRef<HTMLDivElement>(null);
  const projectGroupElementsRef = useRef(new Map<string, HTMLDivElement>());
  const projectHeaderElementsRef = useRef(new Map<string, HTMLDivElement>());
  const pendingProjectPointerRef = useRef<PendingProjectPointer | null>(null);
  const projectDragRef = useRef<ProjectDragState | null>(null);
  const autoScrollFrameRef = useRef<number | null>(null);
  const autoScrollDirectionRef = useRef(0);
  const suppressProjectClickUntilRef = useRef(0);
  const [projectDrag, setProjectDrag] = useState<ProjectDragState | null>(null);
  const isProjectDragging = projectDrag !== null;
  const [collapsedProjectRoots, setCollapsedProjectRoots] = useState<Set<string>>(() => new Set());
  const projectRows = recentRoots;
  const sessionsByRoot = useMemo(() => groupAgentSessionsByRoot(sessions), [sessions]);
  const setAnimatedRowRef = useCallback((key: string, node: HTMLElement | null) => {
    if (node) {
      rowElementsRef.current.set(key, node);
    } else {
      rowElementsRef.current.delete(key);
    }
  }, []);

  useLayoutEffect(() => {
    const prefersReducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const previousTops = previousRowTopsRef.current;
    const nextTops = new Map<string, number>();
    const deltas = new Map<string, number>();
    const sessionProjectRoots = new Map(
      sessions.map((session) => [session.id, session.projectRoot] as const)
    );

    rowElementsRef.current.forEach((node, key) => {
      const nextTop = node.getBoundingClientRect().top;
      nextTops.set(key, nextTop);
      const previousTop = previousTops.get(key);
      if (previousTop !== undefined) deltas.set(key, previousTop - nextTop);
    });

    if (!prefersReducedMotion) {
      rowElementsRef.current.forEach((node, key) => {
        let delta = deltas.get(key) || 0;
        if (key.startsWith("session:")) {
          const sessionId = key.slice("session:".length);
          const projectPath = sessionProjectRoots.get(sessionId);
          if (projectPath) delta -= deltas.get(`project:${projectPath}`) || 0;
        }
        if (Math.abs(delta) < 1) return;

        node.animate([{ transform: `translateY(${delta}px)` }, { transform: "translateY(0)" }], {
          duration: SIDEBAR_REORDER_ANIMATION_MS,
          easing: "cubic-bezier(0.2, 0, 0, 1)"
        });
      });
    }

    previousRowTopsRef.current = nextTops;
  }, [collapsedProjectRoots, projectRows, sessions]);

  const setProjectGroupRef = useCallback(
    (path: string, node: HTMLDivElement | null) => {
      setAnimatedRowRef(`project:${path}`, node);
      if (node) {
        projectGroupElementsRef.current.set(path, node);
      } else {
        projectGroupElementsRef.current.delete(path);
      }
    },
    [setAnimatedRowRef]
  );

  const setProjectHeaderRef = useCallback((path: string, node: HTMLDivElement | null) => {
    if (node) {
      projectHeaderElementsRef.current.set(path, node);
    } else {
      projectHeaderElementsRef.current.delete(path);
    }
  }, []);

  const measureProjectDrop = useCallback(
    (clientX: number, clientY: number, draggedPath: string) => {
      const list = projectListRef.current;
      if (!list) return { insertionIndex: null, markerTop: null };
      const listRect = list.getBoundingClientRect();
      if (
        clientX < listRect.left ||
        clientX > listRect.right ||
        clientY < listRect.top ||
        clientY > listRect.bottom
      ) {
        return { insertionIndex: null, markerTop: null };
      }

      const centers = projectRows.flatMap((root) => {
        const header = projectHeaderElementsRef.current.get(root.path);
        if (!header) return [];
        const rect = header.getBoundingClientRect();
        return [{ path: root.path, centerY: rect.top + rect.height / 2 }];
      });
      const insertionIndex = projectInsertionIndex(clientY, centers, draggedPath);
      if (insertionIndex === null) return { insertionIndex: null, markerTop: null };

      const remaining = projectRows.filter((root) => root.path !== draggedPath);
      let markerViewportY: number;
      if (remaining.length === 0) {
        markerViewportY = listRect.top + 8;
      } else if (insertionIndex < remaining.length) {
        const target = projectGroupElementsRef.current.get(remaining[insertionIndex].path);
        if (!target) return { insertionIndex: null, markerTop: null };
        markerViewportY = target.getBoundingClientRect().top - 4;
      } else {
        const target = projectGroupElementsRef.current.get(remaining.at(-1)!.path);
        if (!target) return { insertionIndex: null, markerTop: null };
        markerViewportY = target.getBoundingClientRect().bottom + 4;
      }

      return {
        insertionIndex,
        markerTop: Math.max(0, Math.min(listRect.height, markerViewportY - listRect.top))
      };
    },
    [projectRows]
  );

  const stopProjectAutoScroll = useCallback(() => {
    autoScrollDirectionRef.current = 0;
    if (autoScrollFrameRef.current !== null) {
      cancelAnimationFrame(autoScrollFrameRef.current);
      autoScrollFrameRef.current = null;
    }
  }, []);

  const updateProjectDragPosition = useCallback(
    (active: ProjectDragState, clientX: number, clientY: number): ProjectDragState => {
      const measurement = measureProjectDrop(clientX, clientY, active.path);
      const next = {
        ...active,
        clientX,
        clientY,
        insertionIndex: measurement.insertionIndex,
        markerTop: measurement.markerTop
      };
      projectDragRef.current = next;
      setProjectDrag(next);
      return next;
    },
    [measureProjectDrop]
  );

  const updateProjectAutoScroll = useCallback(
    (clientX: number, clientY: number) => {
      const scrollContainer = projectListRef.current?.closest("nav");
      if (!(scrollContainer instanceof HTMLElement)) {
        stopProjectAutoScroll();
        return;
      }
      const rect = scrollContainer.getBoundingClientRect();
      const edgeSize = 40;
      const isInsideHorizontally = clientX >= rect.left && clientX <= rect.right;
      const direction =
        !isInsideHorizontally || clientY < rect.top || clientY > rect.bottom
          ? 0
          : clientY < rect.top + edgeSize
            ? -1
            : clientY > rect.bottom - edgeSize
              ? 1
              : 0;
      autoScrollDirectionRef.current = direction;
      if (direction === 0) {
        stopProjectAutoScroll();
        return;
      }
      if (autoScrollFrameRef.current !== null) return;

      const tick = () => {
        const active = projectDragRef.current;
        const nextDirection = autoScrollDirectionRef.current;
        if (!active || nextDirection === 0) {
          autoScrollFrameRef.current = null;
          return;
        }
        const previousScrollTop = scrollContainer.scrollTop;
        scrollContainer.scrollTop += nextDirection * 10;
        if (scrollContainer.scrollTop === previousScrollTop) {
          stopProjectAutoScroll();
          return;
        }
        updateProjectDragPosition(active, active.clientX, active.clientY);
        autoScrollFrameRef.current = requestAnimationFrame(tick);
      };
      autoScrollFrameRef.current = requestAnimationFrame(tick);
    },
    [stopProjectAutoScroll, updateProjectDragPosition]
  );

  const clearProjectDrag = useCallback(
    (suppressClick: boolean) => {
      const active = projectDragRef.current;
      pendingProjectPointerRef.current = null;
      projectDragRef.current = null;
      setProjectDrag(null);
      stopProjectAutoScroll();
      if (active && suppressClick) suppressProjectClickUntilRef.current = Date.now() + 250;
      if (active?.headerElement.hasPointerCapture(active.pointerId)) {
        active.headerElement.releasePointerCapture(active.pointerId);
      }
    },
    [stopProjectAutoScroll]
  );

  const beginProjectPointer = useCallback(
    (event: React.PointerEvent<HTMLDivElement>, root: RecentProjectRoot) => {
      if (
        disabled ||
        !event.isPrimary ||
        event.button !== 0 ||
        pendingProjectPointerRef.current ||
        projectDragRef.current
      ) {
        return;
      }
      const rect = event.currentTarget.getBoundingClientRect();
      pendingProjectPointerRef.current = {
        pointerId: event.pointerId,
        path: root.path,
        name: root.name,
        startX: event.clientX,
        startY: event.clientY,
        grabOffsetX: event.clientX - rect.left,
        grabOffsetY: event.clientY - rect.top,
        ghostWidth: rect.width,
        headerElement: event.currentTarget
      };
    },
    [disabled]
  );

  const suppressActivatedProjectClick = useCallback((event: React.MouseEvent<HTMLDivElement>) => {
    if (Date.now() > suppressProjectClickUntilRef.current) return;
    suppressProjectClickUntilRef.current = 0;
    event.preventDefault();
    event.stopPropagation();
  }, []);

  const finishProjectDrag = useCallback(
    (event: PointerEvent) => {
      const active = projectDragRef.current;
      if (!active || active.pointerId !== event.pointerId) {
        if (pendingProjectPointerRef.current?.pointerId === event.pointerId) {
          pendingProjectPointerRef.current = null;
        }
        return;
      }

      const measurement = measureProjectDrop(event.clientX, event.clientY, active.path);
      const nextRoots = reorderProjectRoots(projectRows, active.path, measurement.insertionIndex);
      clearProjectDrag(true);
      if (nextRoots !== projectRows) onProjectOrderChange([...nextRoots]);
    },
    [clearProjectDrag, measureProjectDrop, onProjectOrderChange, projectRows]
  );

  useEffect(() => {
    const handlePointerMove = (event: PointerEvent) => {
      const candidate = pendingProjectPointerRef.current;
      if (!candidate || candidate.pointerId !== event.pointerId) return;
      if (disabled) {
        clearProjectDrag(Boolean(projectDragRef.current));
        return;
      }

      let active = projectDragRef.current;
      if (!active) {
        if (
          !hasExceededProjectDragThreshold(
            candidate.startX,
            candidate.startY,
            event.clientX,
            event.clientY
          )
        ) {
          return;
        }
        const measurement = measureProjectDrop(event.clientX, event.clientY, candidate.path);
        active = {
          ...candidate,
          clientX: event.clientX,
          clientY: event.clientY,
          insertionIndex: measurement.insertionIndex,
          markerTop: measurement.markerTop
        };
        projectDragRef.current = active;
        setProjectDrag(active);
        candidate.headerElement.setPointerCapture(candidate.pointerId);
      } else {
        active = updateProjectDragPosition(active, event.clientX, event.clientY);
      }

      event.preventDefault();
      updateProjectAutoScroll(active.clientX, active.clientY);
    };
    const handlePointerUp = (event: PointerEvent) => finishProjectDrag(event);
    const handlePointerCancel = (event: PointerEvent) => {
      if (
        pendingProjectPointerRef.current?.pointerId === event.pointerId ||
        projectDragRef.current?.pointerId === event.pointerId
      ) {
        clearProjectDrag(Boolean(projectDragRef.current));
      }
    };
    const handleLostPointerCapture = (event: PointerEvent) => {
      if (projectDragRef.current?.pointerId === event.pointerId) clearProjectDrag(true);
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || !projectDragRef.current) return;
      event.preventDefault();
      clearProjectDrag(true);
    };
    const handleWindowBlur = () => {
      if (projectDragRef.current) clearProjectDrag(true);
      else pendingProjectPointerRef.current = null;
    };

    window.addEventListener("pointermove", handlePointerMove, { passive: false });
    window.addEventListener("pointerup", handlePointerUp);
    window.addEventListener("pointercancel", handlePointerCancel);
    window.addEventListener("lostpointercapture", handleLostPointerCapture);
    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("blur", handleWindowBlur);
    return () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
      window.removeEventListener("pointercancel", handlePointerCancel);
      window.removeEventListener("lostpointercapture", handleLostPointerCapture);
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("blur", handleWindowBlur);
    };
  }, [
    clearProjectDrag,
    disabled,
    finishProjectDrag,
    measureProjectDrop,
    updateProjectAutoScroll,
    updateProjectDragPosition
  ]);

  useEffect(() => {
    if (!disabled) return;
    if (projectDragRef.current) clearProjectDrag(true);
    else pendingProjectPointerRef.current = null;
  }, [clearProjectDrag, disabled]);

  useEffect(() => {
    if (!isProjectDragging) return;
    const previousUserSelect = document.body.style.userSelect;
    const previousCursor = document.body.style.cursor;
    document.body.style.userSelect = "none";
    document.body.style.cursor = "grabbing";
    return () => {
      document.body.style.userSelect = previousUserSelect;
      document.body.style.cursor = previousCursor;
    };
  }, [isProjectDragging]);

  useEffect(() => {
    return () => {
      const active = projectDragRef.current;
      pendingProjectPointerRef.current = null;
      projectDragRef.current = null;
      stopProjectAutoScroll();
      if (active?.headerElement.hasPointerCapture(active.pointerId)) {
        active.headerElement.releasePointerCapture(active.pointerId);
      }
    };
  }, [stopProjectAutoScroll]);

  const toggleProjectCollapsed = useCallback((path: string) => {
    setCollapsedProjectRoots((current) => {
      const next = new Set(current);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  }, []);

  return (
    <>
      <div className="mb-2 flex items-center justify-between gap-3">
        <p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
          Projects
        </p>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-7 w-7 text-muted-foreground hover:text-foreground"
          onClick={onChooseProjectRoot}
          disabled={disabled}
          aria-label="Choose project folder"
        >
          <FolderOpen className="h-4 w-4" />
        </Button>
      </div>

      {projectRows.length === 0 ? (
        <button
          type="button"
          className="flex w-full items-center gap-2 rounded-2xl py-1.5 pr-1 text-left text-sm text-foreground/95 transition-colors hover:text-foreground"
          onClick={onChooseProjectRoot}
          disabled={disabled}
        >
          <FolderOpen className="h-4 w-4 shrink-0" />
          Select a folder
        </button>
      ) : (
        <div ref={projectListRef} className="relative -mt-3 pb-4 pt-3">
          {projectRows.map((root, rootIndex) => {
            const isActive = root.path === projectRoot;
            const projectSessions = sessionsByRoot.get(root.path) || [];
            const isCollapsed = collapsedProjectRoots.has(root.path);
            const hasRunningSession = projectSessions.some((session) =>
              runningSessionIds.has(session.id)
            );
            const hasUnreadCompletedSession = projectSessions.some((session) =>
              completedUnreadSessionIds.has(session.id)
            );
            const showProjectRunningIndicator = isCollapsed && hasRunningSession;
            const showProjectUnreadIndicator =
              isCollapsed && !hasRunningSession && hasUnreadCompletedSession;

            return (
              <div
                key={root.path}
                ref={(node) => setProjectGroupRef(root.path, node)}
                className={cn(
                  "space-y-2 will-change-transform",
                  rootIndex < projectRows.length - 1 && "mb-2",
                  isProjectDragging && "opacity-25"
                )}
              >
                <div
                  ref={(node) => setProjectHeaderRef(root.path, node)}
                  className={cn(
                    "flex select-none items-center gap-1 rounded-2xl text-foreground",
                    !disabled && projectRows.length > 1 && "touch-none",
                    !disabled &&
                      (projectDrag?.path === root.path ? "cursor-grabbing" : "cursor-grab")
                  )}
                  onPointerDown={(event) => beginProjectPointer(event, root)}
                  onClickCapture={suppressActivatedProjectClick}
                >
                  <button
                    type="button"
                    className={cn(
                      "flex min-w-0 flex-1 items-center gap-2 rounded-2xl py-1.5 pr-1 text-left text-sm transition-colors",
                      isActive
                        ? "font-bold text-foreground"
                        : "text-foreground/95 hover:text-foreground"
                    )}
                    onClick={() => onProjectRootChange(root.path)}
                    disabled={disabled}
                    title={root.path}
                  >
                    <FolderOpen className="h-4 w-4 shrink-0" />
                    <span className="min-w-0 flex-1 truncate">{root.name}</span>
                    {showProjectRunningIndicator ? (
                      <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin text-[hsl(var(--maple-primary))]" />
                    ) : showProjectUnreadIndicator ? (
                      <span className="h-2 w-2 shrink-0 rounded-full bg-maple-success" />
                    ) : null}
                  </button>
                  {projectSessions.length > 0 ? (
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7 shrink-0 text-muted-foreground hover:text-foreground"
                      onClick={() => toggleProjectCollapsed(root.path)}
                      aria-label={isCollapsed ? "Expand project tasks" : "Collapse project tasks"}
                    >
                      {isCollapsed ? (
                        <ChevronRight className="h-4 w-4" />
                      ) : (
                        <ChevronDown className="h-4 w-4" />
                      )}
                    </Button>
                  ) : null}
                  {isActive ? (
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7 shrink-0 text-muted-foreground hover:text-foreground"
                      onClick={onCreateSession}
                      disabled={disabled || !projectRoot}
                      aria-label="New Task"
                    >
                      <MessageSquarePlus className="h-4 w-4" />
                    </Button>
                  ) : null}
                </div>

                {!isCollapsed ? (
                  <div className="mt-2 w-full space-y-2 pl-6">
                    {projectSessions.length === 0 ? (
                      isActive ? (
                        <p className="py-1 text-xs text-muted-foreground/75">No tasks yet</p>
                      ) : null
                    ) : (
                      projectSessions.map((session) => {
                        const isActiveSession = session.id === activeSessionId;
                        const isRunning = runningSessionIds.has(session.id);
                        const isUnreadCompleted = completedUnreadSessionIds.has(session.id);
                        const title = sessionTitle(session);
                        const accessibleStatus = isRunning
                          ? "running"
                          : isUnreadCompleted
                            ? "completed, unread"
                            : null;

                        return (
                          <div
                            key={session.id}
                            ref={(node) => setAnimatedRowRef(`session:${session.id}`, node)}
                            className="group relative isolate flex w-full min-w-0 select-none items-stretch gap-0.5 rounded-2xl will-change-transform"
                            onContextMenu={(event) => event.preventDefault()}
                          >
                            <button
                              type="button"
                              className={cn(
                                "relative z-0 min-w-0 flex-1 cursor-pointer py-1 pr-2 text-left text-sm transition-colors",
                                isActiveSession
                                  ? "font-bold text-foreground"
                                  : "text-foreground/95 group-hover:text-foreground"
                              )}
                              onClick={() => onSessionSelect(session.id)}
                              disabled={disabled}
                              aria-current={isActiveSession ? "page" : undefined}
                              aria-label={
                                accessibleStatus ? `${title}, ${accessibleStatus}` : title
                              }
                            >
                              <div className="pr-8">
                                <div className="relative z-0 flex min-w-0 items-center gap-1.5">
                                  {isRunning ? (
                                    <Loader2
                                      className="h-3 w-3 shrink-0 animate-spin text-[hsl(var(--maple-primary))]"
                                      aria-hidden="true"
                                    />
                                  ) : isUnreadCompleted ? (
                                    <span
                                      className="h-2 w-2 shrink-0 rounded-full bg-maple-success"
                                      aria-hidden="true"
                                    />
                                  ) : null}
                                  <span className="min-w-0 flex-1 truncate">{title}</span>
                                </div>
                              </div>
                            </button>

                            <div className={agentSidebarEllipsisTriggerRowClass(isCompactLayout)}>
                              <div className={AGENT_SIDEBAR_ELLIPSIS_FADE} aria-hidden="true" />
                              <div className="flex items-center">
                                <DropdownMenu>
                                  <DropdownMenuTrigger asChild>
                                    <button
                                      type="button"
                                      className={AGENT_SIDEBAR_ELLIPSIS_BUTTON}
                                      disabled={disabled}
                                      onClick={(event) => {
                                        event.preventDefault();
                                        event.stopPropagation();
                                      }}
                                      aria-label={`Open task menu for ${title}`}
                                    >
                                      <MoreHorizontal
                                        className="h-4 w-4"
                                        strokeWidth={SIDEBAR_ICON_STROKE}
                                      />
                                    </button>
                                  </DropdownMenuTrigger>
                                  <DropdownMenuContent align="end">
                                    <DropdownMenuItem
                                      disabled={disabled || isRunning}
                                      onClick={() => onSessionDelete(session)}
                                    >
                                      <Trash
                                        className="mr-2 h-4 w-4"
                                        strokeWidth={SIDEBAR_ICON_STROKE}
                                      />
                                      {isRunning
                                        ? "Stop Agent Before Deleting Task"
                                        : "Delete Task"}
                                    </DropdownMenuItem>
                                  </DropdownMenuContent>
                                </DropdownMenu>
                              </div>
                            </div>
                          </div>
                        );
                      })
                    )}
                  </div>
                ) : null}
              </div>
            );
          })}
          {projectDrag && projectDrag.markerTop !== null && projectDrag.insertionIndex !== null ? (
            <div
              aria-hidden="true"
              className="pointer-events-none absolute inset-x-0 z-40 h-0.5 rounded-full bg-[hsl(var(--blue))]"
              style={{ top: projectDrag.markerTop }}
            >
              <span className="absolute -left-0.5 -top-[3px] h-2 w-2 rounded-full border-2 border-[hsl(var(--blue))] bg-muted dark:bg-[hsl(var(--sidebar))]" />
            </div>
          ) : null}
        </div>
      )}

      {projectDrag ? (
        <div
          aria-hidden="true"
          className="pointer-events-none fixed z-50 flex min-w-0 items-center gap-2 rounded-2xl border border-border/40 bg-muted/95 px-3 py-2 text-sm font-medium text-foreground shadow-lg backdrop-blur dark:bg-[hsl(var(--sidebar)/0.95)]"
          style={{
            left: projectDrag.clientX - projectDrag.grabOffsetX,
            top: projectDrag.clientY - projectDrag.grabOffsetY,
            width: Math.min(projectDrag.ghostWidth, 264),
            maxWidth: "calc(100vw - 24px)"
          }}
        >
          <FolderOpen className="h-4 w-4 shrink-0" />
          <span className="min-w-0 flex-1 truncate">{projectDrag.name}</span>
        </div>
      ) : null}

      <div className="mt-7">
        <p className="mb-3 text-xs font-medium uppercase tracking-wider text-muted-foreground">
          Tasks
        </p>
        <p className="text-xs text-muted-foreground/75">
          Folderless Agent tasks are not available yet.
        </p>
      </div>
    </>
  );
}

interface AgentComposerProps {
  activeRootLabel: string;
  areSettingsDisabled: boolean;
  input: string;
  isSendDisabled: boolean;
  isSending: boolean;
  isStarting: boolean;
  isMcpLoading: boolean;
  isMcpToggleDisabled: boolean;
  isModelSelectionDisabled: boolean;
  mcpServers: AgentSessionMcpServer[];
  mode: AgentPermissionMode;
  model: string;
  projectRoot: string;
  recentRoots: RecentProjectRoot[];
  isExpanded?: boolean;
  onCancelPrompt: () => void;
  onChooseProjectRoot: () => void;
  onInputChange: (value: string) => void;
  onKeyDown: (event: React.KeyboardEvent<HTMLTextAreaElement>) => void;
  onManageMcpServers: () => void;
  onMcpToggle: (name: string, enabled: boolean) => void;
  onModeChange: (value: AgentPermissionMode) => void;
  onModelChange: (value: string) => void;
  onProjectRootChange: (value: string) => void;
  onSendMessage: () => void;
  onToggleExpanded?: () => void;
}

function AgentComposer({
  activeRootLabel,
  areSettingsDisabled,
  input,
  isSendDisabled,
  isSending,
  isStarting,
  isMcpLoading,
  isMcpToggleDisabled,
  isModelSelectionDisabled,
  mcpServers,
  mode,
  model,
  projectRoot,
  recentRoots,
  isExpanded = false,
  onCancelPrompt,
  onChooseProjectRoot,
  onInputChange,
  onKeyDown,
  onManageMcpServers,
  onMcpToggle,
  onModeChange,
  onModelChange,
  onProjectRootChange,
  onSendMessage,
  onToggleExpanded
}: AgentComposerProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const rootOptions = recentRoots;

  useLayoutEffect(() => {
    const textarea = textareaRef.current;
    if (!textarea) return;

    if (isExpanded) {
      textarea.style.height = "";
      return;
    }

    textarea.style.height = "auto";
    textarea.style.height = `${Math.min(textarea.scrollHeight, 200)}px`;
  }, [input, isExpanded]);

  return (
    <ChatComposerSurface
      className={cn(
        "transition-all duration-300",
        isExpanded && "flex h-[70vh] max-h-[800px] min-h-0 flex-col"
      )}
    >
      {onToggleExpanded ? (
        <button
          type="button"
          onClick={onToggleExpanded}
          className="absolute right-2 top-2 z-10 rounded-full p-1.5 text-muted-foreground/60 transition-colors hover:bg-muted/50 hover:text-foreground"
          aria-label={isExpanded ? "Exit fullscreen" : "Enter fullscreen"}
        >
          {isExpanded ? <Shrink className="h-4 w-4" /> : <Expand className="h-4 w-4" />}
        </button>
      ) : null}
      <Textarea
        ref={textareaRef}
        id="agent-message"
        value={input}
        onChange={(event) => onInputChange(event.target.value)}
        onKeyDown={onKeyDown}
        disabled={isSendDisabled}
        placeholder="Ask Maple to work in this folder..."
        className={cn(
          CHAT_COMPOSER_TEXTAREA_CLASS,
          onToggleExpanded && "pr-8",
          isExpanded &&
            "min-h-0 max-h-none flex-1 overflow-y-auto landscape-short:min-h-0 landscape-short:max-h-none"
        )}
        rows={isExpanded ? undefined : 1}
      />

      <div className="grid shrink-0 grid-cols-[minmax(0,1fr)_auto] items-end gap-x-2 gap-y-2 px-2 pb-2 pt-1">
        <div className="flex min-w-0 flex-wrap items-center gap-1.5 sm:gap-2">
          <AgentModelSelector
            disabled={isModelSelectionDisabled}
            model={model}
            onModelChange={onModelChange}
          />

          <AgentModeSelector
            disabled={areSettingsDisabled}
            mode={mode}
            onModeChange={onModeChange}
          />

          <AgentMcpMenu
            servers={mcpServers}
            disabled={areSettingsDisabled}
            togglesDisabled={isMcpToggleDisabled}
            loading={isMcpLoading}
            onToggle={onMcpToggle}
            onManage={onManageMcpServers}
          />

          <Select
            disabled={areSettingsDisabled}
            value={projectRoot || undefined}
            onValueChange={(value) => {
              if (value === NEW_PROJECT_OPTION_VALUE) {
                onChooseProjectRoot();
                return;
              }
              onProjectRootChange(value);
            }}
          >
            <SelectTrigger className="h-8 w-auto max-w-[12rem] gap-1 border-0 bg-transparent px-2 text-[hsl(var(--maple-secondary-700))] hover:bg-[hsl(var(--maple-primary-container))] hover:text-[hsl(var(--maple-secondary-700))] focus:ring-0 focus:ring-offset-0">
              <FolderOpen className="h-4 w-4 shrink-0" />
              <SelectValue placeholder={activeRootLabel} />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={NEW_PROJECT_OPTION_VALUE}>New project…</SelectItem>
              {rootOptions.length > 0 ? <SelectSeparator /> : null}
              {rootOptions.map((root) => (
                <SelectItem key={root.path} value={root.path}>
                  {root.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
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
              disabled={isSendDisabled || !input.trim() || !projectRoot}
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
    </ChatComposerSurface>
  );
}

function AgentModeSelector({
  disabled,
  mode,
  onModeChange
}: {
  disabled?: boolean;
  mode: AgentPermissionMode;
  onModeChange: (value: AgentPermissionMode) => void;
}) {
  const activeMode =
    AGENT_PERMISSION_MODES.find((candidate) => candidate.value === mode) ||
    AGENT_PERMISSION_MODES[0];

  return (
    <Select
      disabled={disabled}
      value={activeMode.value}
      onValueChange={(value) => onModeChange(normalizeAgentPermissionMode(value))}
    >
      <SelectTrigger
        className="h-8 w-auto max-w-[11.5rem] gap-1 border-0 bg-transparent px-2 text-[hsl(var(--maple-secondary-700))] hover:bg-[hsl(var(--maple-primary-container))] hover:text-[hsl(var(--maple-secondary-700))] focus:ring-0 focus:ring-offset-0"
        aria-label={`Current permission mode: ${activeMode.label}. Click to change mode.`}
      >
        <ShieldCheck className="h-4 w-4 shrink-0" />
        <span className="truncate text-xs font-medium">{activeMode.label}</span>
      </SelectTrigger>
      <SelectContent>
        {AGENT_PERMISSION_MODES.map((permissionMode) => (
          <SelectItem
            key={permissionMode.value}
            value={permissionMode.value}
            textValue={permissionMode.label}
          >
            <div className="flex flex-col">
              <span>{permissionMode.label}</span>
              <span className="text-xs text-muted-foreground">{permissionMode.description}</span>
            </div>
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

function AgentModelSelector({
  disabled,
  model,
  onModelChange
}: {
  disabled?: boolean;
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
      const reconciledModel = reconcileAgentModel(currentModel, models);
      if (reconciledModel !== currentModel) {
        onModelChange(reconciledModel);
      }
    },
    [onModelChange]
  );

  const fetchCatalog = useCallback(async () => {
    if (hasFetched.current || isFetching.current) return;

    // LocalState may hold only normal chat's last selected model plus placeholder
    // aliases, so it is not proof that Agent Mode has a complete catalog.
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
  }, [os, reconcileSelectedConcreteModel, setAvailableModels, setHasWhisperModel, setModelAliases]);

  useEffect(() => {
    void fetchCatalog();
  }, [fetchCatalog]);

  const getAlias = useCallback(
    (modelId: string): OpenSecretModelAlias | undefined => {
      const alias = aliasById.get(modelId as OpenSecretModelAlias["id"]);
      if (alias) return alias;

      const fallback = FALLBACK_AGENT_MODEL_ALIASES.find(
        (primaryModel) => primaryModel.id === modelId
      );
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
      const primaryModel = PRIMARY_AGENT_MODELS.find((candidate) => candidate.id === modelId);
      return modelById.get(modelId)?.access || primaryModel?.access || "free";
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
    const primaryModel = PRIMARY_AGENT_MODELS.find((candidate) => candidate.id === modelId);
    return (
      selectedModel?.short_name || selectedModel?.display_name || primaryModel?.label || modelId
    );
  };

  const getDisplayNameText = (modelId: string): string => {
    const alias = getAlias(modelId);
    if (alias) return alias.label;

    const selectedModel = modelById.get(modelId);
    const primaryModel = PRIMARY_AGENT_MODELS.find((candidate) => candidate.id === modelId);
    return (
      selectedModel?.display_name || selectedModel?.short_name || primaryModel?.label || modelId
    );
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
            disabled={disabled}
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
  isResponsePending,
  isRunActive,
  generatedThoughtLabels,
  sessionId,
  userId,
  onPermissionDecision
}: {
  items: AgentTimelineItem[];
  isResponsePending: boolean;
  isRunActive: boolean;
  generatedThoughtLabels: Record<string, Record<string, string>>;
  sessionId: string | null;
  userId: string;
  onPermissionDecision: (item: AgentTimelineItem, decision: AgentPermissionDecision) => void;
}) {
  const visibleItems = coalesceAdjacentThinkingItems(items).filter(isRenderableAgentTimelineItem);
  const turns = groupAgentTimelineItems(visibleItems);
  const activeThinkingItemId = activeAgentThinkingItemId(visibleItems, isRunActive);
  const showAssistantLoader = shouldShowAgentAssistantLoader(turns, isResponsePending);

  return (
    <div className="space-y-1">
      {turns.map((turn, turnIndex) => {
        const copyText = getAgentTurnCopyText(turn);

        if (turn.type === "user") {
          return (
            <ChatUserTurn
              key={turn.id}
              actions={copyText ? <ChatCopyButton text={copyText} /> : undefined}
            >
              <Markdown content={turn.item.text || ""} />
            </ChatUserTurn>
          );
        }

        let thinkingPhaseIndex = 0;
        const assistantItems = turn.items.map((item) => {
          const phaseId =
            item.itemType === "thinking"
              ? agentThinkingPhaseId(turn.id, thinkingPhaseIndex++)
              : null;
          const completedThinkingLabel =
            phaseId && sessionId
              ? (generatedThoughtLabels[sessionId]?.[phaseId] ??
                getAgentThoughtLabel({ userId, sessionId, phaseId }))
              : null;
          return { item, completedThinkingLabel };
        });

        return (
          <ChatAssistantTurn
            key={turn.id}
            actions={
              copyText && !(isRunActive && turnIndex === turns.length - 1) ? (
                <ChatCopyButton text={copyText} />
              ) : undefined
            }
          >
            {assistantItems.map(({ item, completedThinkingLabel }) => (
              <AgentAssistantItem
                key={item.id}
                item={item}
                isThinking={item.id === activeThinkingItemId}
                completedThinkingLabel={completedThinkingLabel ?? undefined}
                onPermissionDecision={onPermissionDecision}
              />
            ))}
          </ChatAssistantTurn>
        );
      })}
      {showAssistantLoader ? <ChatAssistantPendingTurn /> : null}
    </div>
  );
}

function AgentAssistantItem({
  item,
  isThinking,
  completedThinkingLabel,
  onPermissionDecision
}: {
  item: AgentTimelineItem;
  isThinking: boolean;
  completedThinkingLabel?: string;
  onPermissionDecision: (item: AgentTimelineItem, decision: AgentPermissionDecision) => void;
}) {
  if (item.itemType === "message") {
    return (
      <div className="prose prose-sm max-w-none dark:prose-invert">
        <Markdown content={item.text || ""} />
      </div>
    );
  }
  if (item.itemType === "thinking") {
    return (
      <ThinkingBlock
        content={item.text || ""}
        isThinking={isThinking}
        showDuration={false}
        completedLabel={completedThinkingLabel}
      />
    );
  }
  if (item.itemType === "tool") return <ToolCallRow item={item} />;
  if (item.itemType === "permission") {
    return <PermissionRow item={item} onPermissionDecision={onPermissionDecision} />;
  }
  return <SystemRow item={item} />;
}

function ToolCallRow({ item }: { item: AgentTimelineItem }) {
  const status = item.status || "running";
  const failed = status === "failed" || status === "error";
  const active = isActiveAgentStatus(status);
  const hasDetails =
    Boolean(item.text?.trim()) || item.input !== undefined || item.output !== undefined;
  const statusIcon = active ? (
    <Loader2 className="h-4 w-4 shrink-0 animate-spin text-muted-foreground" />
  ) : failed ? (
    <X className="h-4 w-4 shrink-0 text-destructive" />
  ) : (
    <Check className="h-4 w-4 shrink-0 text-maple-success" />
  );

  const summary = (
    <div className="flex min-w-0 flex-1 items-center gap-2">
      {statusIcon}
      <span className="min-w-0 flex-1 truncate text-sm font-medium">{toolTitle(item)}</span>
      <span className={cn("shrink-0 text-xs text-muted-foreground", failed && "text-destructive")}>
        {formatStatus(status)}
      </span>
    </div>
  );

  if (!hasDetails) {
    return (
      <div
        className={cn(
          "flex items-center gap-2 rounded-2xl bg-muted/30 px-3 py-2 text-sm",
          failed && "bg-destructive/5"
        )}
      >
        {summary}
      </div>
    );
  }

  return (
    <details
      open={failed}
      className={cn(
        "group rounded-3xl border border-muted/40 bg-muted/20 px-4 py-3 text-sm",
        failed && "border-destructive/35 bg-destructive/5"
      )}
    >
      <summary className="flex cursor-pointer list-none items-center gap-2">
        <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground transition-transform group-open:rotate-90" />
        {summary}
      </summary>
      <div className="mt-2 space-y-2 pl-6">
        {item.text ? <ToolDetail label="Summary" value={item.text} /> : null}
        {item.input !== undefined ? (
          <ToolDetail label="Input" value={formatUnknown(item.input)} />
        ) : null}
        {item.output !== undefined ? (
          <ToolDetail label="Output" value={formatUnknown(item.output)} />
        ) : null}
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
        "rounded-3xl border border-muted/40 bg-muted/20 px-4 py-3 text-sm",
        resolved
          ? "border-muted/40"
          : "border-[hsl(var(--maple-primary)/0.45)] bg-[hsl(var(--maple-primary)/0.06)]"
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 items-start gap-2">
          <ShieldCheck className="mt-0.5 h-4 w-4 shrink-0 text-[hsl(var(--maple-primary))]" />
          <div className="min-w-0">
            <p className="font-medium">{item.title || "Permission requested"}</p>
            {item.text ? (
              <p className="mt-1 whitespace-pre-wrap break-words text-xs text-muted-foreground">
                {item.text}
              </p>
            ) : null}
          </div>
        </div>
        <span className="shrink-0 text-xs text-muted-foreground" role="status" aria-live="polite">
          {formatPermissionStatus(item.status || "pending")}
        </span>
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

function SystemRow({ item }: { item: AgentTimelineItem }) {
  const failed = item.itemType === "error" || item.status === "failed";
  return (
    <div
      className={cn(
        "rounded-2xl px-3 py-2 text-sm",
        failed
          ? "border border-destructive/35 bg-destructive/5 text-destructive"
          : "bg-muted/30 text-muted-foreground"
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
    text: appendText
      ? `${previous.text || ""}${incoming.text || ""}`
      : (incoming.text ?? previous.text)
  };

  return next;
}

function permissionRequestId(item: AgentTimelineItem): string {
  return item.id.startsWith("permission-") ? item.id.slice("permission-".length) : item.id;
}

function sessionTitle(session: AgentSessionSummary): string {
  return session.title || "New task";
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

function isActiveAgentStatus(status: string | null | undefined): boolean {
  return ["running", "in_progress", "streaming", "pending", "queued"].includes(status || "");
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

function basename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() || path;
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return "Agent Mode failed";
}
