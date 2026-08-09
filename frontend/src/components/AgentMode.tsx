import {
  memo,
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
  Blocks,
  Brain,
  Camera,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Circle,
  Expand,
  FilePenLine,
  FileSearch,
  Folder,
  FolderOpen,
  FolderPlus,
  Globe2,
  Loader2,
  Lock,
  MessageSquare,
  MessageSquarePlus,
  MoreHorizontal,
  ShieldCheck,
  Shrink,
  SquareTerminal,
  Trash,
  Wrench,
  X,
  Zap
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
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
  ChatAssistantPendingIndicator,
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
import { HoverCard, HoverCardContent, HoverCardTrigger } from "@/components/ui/hover-card";
import { Sidebar, SidebarToggle } from "@/components/Sidebar";
import { MapleWordmark } from "@/components/MapleWordmark";
import { DeleteChatDialog } from "@/components/DeleteChatDialog";
import { RenameAgentProjectDialog } from "@/components/RenameAgentProjectDialog";
import { UpgradePromptDialog } from "@/components/UpgradePromptDialog";
import { AgentMcpMenu, AgentMcpServersDialog } from "@/components/agent/AgentMcpControls";
import { AgentSidebarInfoCard } from "@/components/agent/AgentSidebarInfoCard";
import { handleAgentModeThoughtRunFinished } from "@/components/agent/agentModeThoughtRun";
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
  firstVisibleProjectRoot,
  mergeAgentProjectRoots,
  projectInsertionIndex,
  projectOrderForExistingRegistration,
  projectOrderReducer,
  projectRootFallbackAfterRemoval,
  reorderProjectRoots,
  visibleAgentSessions
} from "@/services/agentProjectOrdering";
import {
  isMcpConnectionErrorEvent,
  mcpConnectionErrorMessage,
  userFacingAgentError
} from "@/services/agentMcpErrors";
import { reconcileNewChatMcpServerNames } from "@/services/agentMcpServers";
import { agentOperationFence } from "@/services/agentOperationFence";
import {
  agentToolKind,
  agentToolKindLabel,
  type AgentToolKind
} from "@/services/agentToolPresentation";
import {
  AgentThoughtLabelFinalRequestRegistry,
  AgentThoughtLabelProvisionalScheduler,
  requestAgentThoughtLabel,
  startAgentThoughtLabelDisplay
} from "@/services/agentThoughtLabels";
import {
  AgentLiveThoughtPhaseTracker,
  activeAgentThinkingItemId,
  agentThinkingPhaseId,
  coalesceAdjacentThinkingItems,
  getAgentTurnCopyText,
  groupAgentTimelineItems,
  hasAgentUserMessage,
  isRenderableAgentTimelineItem,
  shouldShowAgentAssistantLoader,
  type AgentThoughtPhase
} from "@/services/agentTimeline";
import {
  DEFAULT_AGENT_MODEL,
  PRIMARY_AGENT_MODEL_IDS,
  reconcileAgentModel,
  resolveAgentModelForSession,
  resolveAgentModelContextLimit,
  resolveAgentModelVisionCapability
} from "@/services/agentModels";
import { SIDEBAR_GRID_COLUMNS_CLASS, getSidebarLayoutStyle } from "@/constants/layout";
import {
  cn,
  POWERFUL_MODEL_ALIAS,
  QUICK_MODEL_ALIAS,
  useIsCoarsePointer,
  useIsLandscapeMobile,
  useIsMobile
} from "@/utils/utils";
import { isTauriDesktop } from "@/utils/platform";
import { useLazyRef } from "@/utils/useLazyRef";
import { revealAgentProjectFolder } from "@/services/agentProjectFolder";
import {
  aggregateAgentSidebarStatus,
  agentProjectProgressLabel,
  agentProjectTaskSummaryLabel,
  agentTaskAccessibleLabel
} from "@/services/agentSidebarPresentation";
import {
  loadAgentSidebarPreferences,
  projectRootsWithDisplayNames,
  renameAgentProjectDisplayName,
  saveAgentSidebarPreferences,
  toggleAgentProjectCollapsed,
  type AgentProjectRootView,
  type AgentSidebarPreferences
} from "@/services/agentSidebarPreferences";
import { useNotification } from "@/contexts/NotificationContext";
import { useBillingState, useModelState } from "@/state/useLocalState";
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
const THOUGHT_PHASE_SEED_RETRY_MS = 250;
const AUTO_SCROLL_BOTTOM_THRESHOLD_PX = 100;
const SIDEBAR_REORDER_ANIMATION_MS = 150;
const SIDEBAR_ICON_STROKE = 2;
const AGENT_SESSION_DELETED_EVENT = "maple:agent-session-deleted";
const AGENT_MODEL_PREFERENCE_KEY = "selectedAgentModel";
// Mode switches remount AgentMode, so project-root mutations must be ordered outside it.
const projectRootPersistenceQueues = new Map<string, Promise<void>>();
const AGENT_SIDEBAR_ACTION_ROW_BASE = "absolute inset-y-0 right-0 z-30 flex min-h-0 items-stretch";
const AGENT_SIDEBAR_ACTION_BUTTON =
  "relative z-10 flex shrink-0 items-center justify-center rounded-full border-0 bg-transparent text-muted-foreground transition-colors hover:bg-foreground/5 hover:text-foreground focus-visible:bg-foreground/5 focus-visible:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring/70";

class PendingAgentSendCancelledError extends Error {
  constructor() {
    super("Agent message cancelled before the run started");
    this.name = "PendingAgentSendCancelledError";
  }
}

type AgentModelPreference = {
  preferred: string | null;
  configuredDefault: string;
};

function readAgentModelPreference(): string | null {
  try {
    return localStorage.getItem(AGENT_MODEL_PREFERENCE_KEY)?.trim() || null;
  } catch {
    return null;
  }
}

function persistAgentModelPreference(model: string | null): void {
  try {
    if (model) localStorage.setItem(AGENT_MODEL_PREFERENCE_KEY, model);
    else localStorage.removeItem(AGENT_MODEL_PREFERENCE_KEY);
  } catch {
    // Storage failures should not prevent changing the in-memory selection.
  }
}

function newTaskAgentModel(preference: AgentModelPreference): string {
  return preference.preferred || preference.configuredDefault;
}

function projectActionRowClass(
  isTouchLayout: boolean,
  menuOpen: boolean,
  hasKeyboardFocus: boolean
): string {
  return cn(
    AGENT_SIDEBAR_ACTION_ROW_BASE,
    "transition-opacity duration-150 motion-reduce:transition-none",
    isTouchLayout || menuOpen || hasKeyboardFocus
      ? "pointer-events-auto opacity-100"
      : "pointer-events-none opacity-0 group-hover/project:pointer-events-auto group-hover/project:opacity-100"
  );
}

function taskActionRowClass(
  isTouchLayout: boolean,
  menuOpen: boolean,
  hasKeyboardFocus: boolean
): string {
  return cn(
    AGENT_SIDEBAR_ACTION_ROW_BASE,
    "transition-opacity duration-150 motion-reduce:transition-none",
    isTouchLayout || menuOpen || hasKeyboardFocus
      ? "pointer-events-auto opacity-100"
      : "pointer-events-none opacity-0 group-hover/task:pointer-events-auto group-hover/task:opacity-100"
  );
}

function isKeyboardFocusTarget(target: EventTarget | null): boolean {
  return target instanceof Element && target.matches(":focus-visible");
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
    description:
      "Auto-runs local reads and Maple web research; asks before writes and other external access"
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
  const openai = useOpenAI();
  const os = useOpenSecret();
  const { availableModels, setAvailableModels, modelAliases, setModelAliases, setHasWhisperModel } =
    useModelState();
  const { agentSessionSelection } = usePersistentHomeNavigation();
  const { showNotification } = useNotification();
  const isMobile = useIsMobile();
  const isLandscapeMobile = useIsLandscapeMobile();
  const isCoarsePointer = useIsCoarsePointer();
  const isCompactLayout = isMobile || isLandscapeMobile;
  const isTouchLayout = isCompactLayout || isCoarsePointer;
  const [isSidebarOpen, setIsSidebarOpen] = usePersistentSidebarState(isCompactLayout);
  const [sidebarPreferences, setSidebarPreferences] = useState<AgentSidebarPreferences>(() =>
    loadAgentSidebarPreferences(userId)
  );
  const sidebarPreferencesRef = useRef(sidebarPreferences);
  const [runtimeStatus, setRuntimeStatus] = useState<AgentRuntimeStatus | null>(null);
  const [projectOrderState, dispatchProjectOrder] = useReducer(
    projectOrderReducer<RecentProjectRoot>,
    createProjectOrderState<RecentProjectRoot>([])
  );
  const recentRoots = projectOrderState.visible;
  const [removedProjectRoots, setRemovedProjectRoots] = useState<Set<string>>(() => new Set());
  const [sessions, setSessions] = useState<AgentSessionSummary[]>([]);
  const [isSessionHistoryReady, setIsSessionHistoryReady] = useState(false);
  const [sessionToDelete, setSessionToDelete] = useState<AgentSessionSummary | null>(null);
  const [projectToRename, setProjectToRename] = useState<AgentProjectRootView | null>(null);
  const [projectToRemove, setProjectToRemove] = useState<AgentProjectRootView | null>(null);
  const [projectRemovalError, setProjectRemovalError] = useState<string | null>(null);
  const [isProjectRemovalPending, setIsProjectRemovalPending] = useState(false);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [projectRoot, setProjectRoot] = useState("");
  const agentModelPreferenceRef = useLazyRef<AgentModelPreference>(() => ({
    preferred: readAgentModelPreference(),
    configuredDefault: DEFAULT_MODEL
  }));
  const [model, setModel] = useState(() => newTaskAgentModel(agentModelPreferenceRef.current));
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
  const [isAuthTransitionReady, setIsAuthTransitionReady] = useState(false);
  const [isInitializing, setIsInitializing] = useState(true);
  const [isStarting, setIsStarting] = useState(false);
  const [isAgentModelCatalogLoading, setIsAgentModelCatalogLoading] = useState(true);
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
  const deletedSessionIdsRef = useLazyRef(() => new Set<string>());
  const shouldAutoScrollRef = useRef(true);
  const permissionModeUpdateRef = useLazyRef<Promise<void>>(() => Promise.resolve());
  const permissionModeUpdateGenerationRef = useRef(0);
  const selectedModeRef = useRef<AgentPermissionMode>(mode);
  const committedModeRef = useRef<AgentPermissionMode>(mode);
  const terminalRunIdsRef = useLazyRef(() => new Set<string>());
  const pendingSendTokensRef = useLazyRef(() => new Map<string, number>());
  const cancelledPendingSendTokensRef = useLazyRef(() => new Set<number>());
  const nextSendTokenRef = useRef(0);
  const activeRunsBySessionRef = useRef<Record<string, string>>({});
  const timelineRevisionBySessionRef = useLazyRef(() => new Map<string, number>());
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
  const thoughtPhaseTrackerRef = useLazyRef(() => new AgentLiveThoughtPhaseTracker());
  const thoughtLabelFinalRequestRegistryRef = useLazyRef(
    () => new AgentThoughtLabelFinalRequestRegistry()
  );
  const thoughtPhaseSeededRunIdsRef = useLazyRef(() => new Set<string>());
  const openaiRef = useRef(openai);
  const openSecretRef = useRef(os);
  const currentAgentModelRef = useRef(model);
  const agentModelStateSettersRef = useRef({
    setAvailableModels,
    setModelAliases,
    setHasWhisperModel
  });
  const authoritativeModelCatalogRef = useRef<OpenSecretModelCatalog | null>(null);
  const selectableAgentModelsRef = useRef<OpenSecretModel[] | null>(null);
  const userIdRef = useRef(userId);
  const thoughtLabelProvisionalSchedulerRef = useRef<AgentThoughtLabelProvisionalScheduler | null>(
    null
  );

  useLayoutEffect(() => {
    openaiRef.current = openai;
    openSecretRef.current = os;
    currentAgentModelRef.current = model;
    agentModelStateSettersRef.current = {
      setAvailableModels,
      setModelAliases,
      setHasWhisperModel
    };
    userIdRef.current = userId;
  }, [model, openai, os, setAvailableModels, setHasWhisperModel, setModelAliases, userId]);

  const restoreNewTaskModel = useCallback(() => {
    const nextModel = newTaskAgentModel(agentModelPreferenceRef.current);
    isAgentModelLockedRef.current = false;
    currentAgentModelRef.current = nextModel;
    setModel(nextModel);
  }, [agentModelPreferenceRef]);

  const reconcileNewTaskModel = useCallback(
    (models: OpenSecretModel[]) => {
      const preference = agentModelPreferenceRef.current;
      preference.configuredDefault = reconcileAgentModel(preference.configuredDefault, models);
      if (
        preference.preferred &&
        reconcileAgentModel(preference.preferred, models) !== preference.preferred
      ) {
        preference.preferred = null;
        persistAgentModelPreference(null);
      }

      const nextModel = newTaskAgentModel(preference);
      if (!isAgentModelLockedRef.current) {
        currentAgentModelRef.current = nextModel;
        setModel(nextModel);
      }
    },
    [agentModelPreferenceRef]
  );

  const commitSidebarPreferences = useCallback(
    (update: (current: AgentSidebarPreferences) => AgentSidebarPreferences) => {
      const next = update(sidebarPreferencesRef.current);
      sidebarPreferencesRef.current = next;
      setSidebarPreferences(next);
      saveAgentSidebarPreferences(userId, next);
    },
    [userId]
  );

  if (!thoughtLabelProvisionalSchedulerRef.current) {
    thoughtLabelProvisionalSchedulerRef.current = new AgentThoughtLabelProvisionalScheduler({
      request: (phase, signal) =>
        requestAgentThoughtLabel(
          openaiRef.current,
          {
            userRequest: phase.userRequest,
            reasoningText: phase.reasoningText
          },
          {
            phaseState: "streaming",
            signal
          }
        ),
      commit: (phase, label) => {
        if (!isAgentModeMountedRef.current || deletedSessionIdsRef.current.has(phase.sessionId)) {
          return;
        }
        setGeneratedThoughtLabels((current) => {
          if (current[phase.sessionId]?.[phase.phaseId] === label) return current;
          return {
            ...current,
            [phase.sessionId]: {
              ...current[phase.sessionId],
              [phase.phaseId]: label
            }
          };
        });
      }
    });
  }

  const cancelThoughtLabelDisplays = useCallback(
    (sessionId?: string, assistantTurnId?: string) => {
      thoughtLabelProvisionalSchedulerRef.current?.cancelMatching(sessionId, assistantTurnId);
      thoughtLabelFinalRequestRegistryRef.current.cancelMatching(sessionId, assistantTurnId);
    },
    [thoughtLabelFinalRequestRegistryRef]
  );

  const generateThoughtLabel = useCallback(
    (phase: AgentThoughtPhase, retainedLabel: string | null = null) => {
      const finalRequest = thoughtLabelFinalRequestRegistryRef.current.begin(phase, retainedLabel);
      if (!finalRequest) return;
      const generationIsCurrent = () =>
        isAgentModeMountedRef.current &&
        !deletedSessionIdsRef.current.has(phase.sessionId) &&
        finalRequest.isCurrent();
      const commitDisplayLabel = (label: string, expectedLabel?: string) => {
        if (!generationIsCurrent()) return;
        if (expectedLabel === undefined) finalRequest.recordLabel(label);
        setGeneratedThoughtLabels((current) => {
          if (!generationIsCurrent()) return current;
          const currentLabel = current[phase.sessionId]?.[phase.phaseId];
          if (expectedLabel !== undefined && currentLabel !== expectedLabel) return current;
          if (expectedLabel !== undefined) finalRequest.recordLabel(label);
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
      const cancelDisplay = startAgentThoughtLabelDisplay({
        retainedLabel: finalRequest.retainedLabel,
        commit: commitDisplayLabel,
        request: (signal) =>
          requestAgentThoughtLabel(
            openai,
            {
              userRequest: phase.userRequest,
              reasoningText: phase.reasoningText
            },
            {
              phaseState: "complete",
              signal
            }
          ).finally(finalRequest.finish)
      });
      finalRequest.setCancel(cancelDisplay);
    },
    [deletedSessionIdsRef, openai, thoughtLabelFinalRequestRegistryRef]
  );

  const completeThoughtPhase = useCallback(
    (phase: AgentThoughtPhase) => {
      const retainedLabel = thoughtLabelProvisionalSchedulerRef.current?.complete(
        phase.sessionId,
        phase.phaseId
      );
      generateThoughtLabel(phase, retainedLabel ?? null);
    },
    [generateThoughtLabel]
  );

  const observeActiveThoughtPhase = useCallback(
    (sessionId: string) => {
      const activePhase = thoughtPhaseTrackerRef.current.activePhase(sessionId);
      if (activePhase) thoughtLabelProvisionalSchedulerRef.current?.observe(activePhase);
    },
    [thoughtPhaseTrackerRef]
  );

  const seedActiveThoughtPhases = useCallback(
    (activeRuns: Record<string, string>) => {
      for (const [sessionId, runId] of Object.entries(activeRuns)) {
        if (thoughtPhaseSeededRunIdsRef.current.has(runId)) continue;
        thoughtPhaseSeededRunIdsRef.current.add(runId);
        void (async () => {
          const timelineRevision = timelineRevisionBySessionRef.current.get(sessionId) || 0;
          try {
            const detail = await agentRuntimeService.loadSession(userId, sessionId);
            if (
              !isAgentModeMountedRef.current ||
              userIdRef.current !== userId ||
              deletedSessionIdsRef.current.has(sessionId) ||
              activeRunsBySessionRef.current[sessionId] !== runId
            ) {
              thoughtPhaseSeededRunIdsRef.current.delete(runId);
              return;
            }
            if ((timelineRevisionBySessionRef.current.get(sessionId) || 0) === timelineRevision) {
              thoughtPhaseTrackerRef.current.seedActiveTimeline(sessionId, detail.timeline);
              observeActiveThoughtPhase(sessionId);
              return;
            }
            globalThis.setTimeout(() => {
              thoughtPhaseSeededRunIdsRef.current.delete(runId);
              if (
                isAgentModeMountedRef.current &&
                userIdRef.current === userId &&
                !deletedSessionIdsRef.current.has(sessionId) &&
                activeRunsBySessionRef.current[sessionId] === runId
              ) {
                seedActiveThoughtPhases({ [sessionId]: runId });
              }
            }, THOUGHT_PHASE_SEED_RETRY_MS);
          } catch {
            thoughtPhaseSeededRunIdsRef.current.delete(runId);
          }
        })();
      }
    },
    [
      deletedSessionIdsRef,
      observeActiveThoughtPhase,
      thoughtPhaseSeededRunIdsRef,
      thoughtPhaseTrackerRef,
      timelineRevisionBySessionRef,
      userId
    ]
  );

  const invalidateThoughtLabelsForTurn = useCallback(
    (sessionId: string, assistantTurnId: string) => {
      cancelThoughtLabelDisplays(sessionId, assistantTurnId);
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
    [cancelThoughtLabelDisplays]
  );

  const invalidateThoughtLabelsForSession = useCallback(
    (sessionId: string) => {
      cancelThoughtLabelDisplays(sessionId);
      setGeneratedThoughtLabels((current) => {
        if (!current[sessionId]) return current;
        const next = { ...current };
        delete next[sessionId];
        return next;
      });
    },
    [cancelThoughtLabelDisplays]
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
    () => mergeAgentProjectRoots(recentRoots, projectRoot, sessions, removedProjectRoots),
    [projectRoot, recentRoots, removedProjectRoots, sessions]
  );
  const displayProjectRoots = useMemo(
    () => projectRootsWithDisplayNames(visibleProjectRoots, sidebarPreferences),
    [sidebarPreferences, visibleProjectRoots]
  );
  const visibleSessions = useMemo(
    () => visibleAgentSessions(sessions, removedProjectRoots),
    [removedProjectRoots, sessions]
  );
  const visibleProjectRootsRef = useRef<RecentProjectRoot[]>([]);
  useLayoutEffect(() => {
    visibleProjectRootsRef.current = visibleProjectRoots;
  }, [visibleProjectRoots]);
  const activeRootLabel = useMemo(() => {
    if (!projectRoot) return "Select folder";
    return (
      displayProjectRoots.find((root) => root.path === projectRoot)?.displayName ||
      basename(projectRoot)
    );
  }, [displayProjectRoots, projectRoot]);
  const activeSessionTitle = useMemo(() => {
    const activeSession = sessions.find((session) => session.id === activeSessionId);
    return activeSession ? sessionTitle(activeSession) : "New task";
  }, [activeSessionId, sessions]);
  const activeRunId = activeSessionId ? (activeRunsBySession[activeSessionId] ?? null) : null;
  const activePendingSendKey = activeSessionId || NEW_SESSION_PENDING_KEY;
  const isSubmitting = pendingSendSessionIds.has(activePendingSendKey);
  const isTaskSelectionPending =
    pendingSessionSelectionId !== null && pendingSessionSelectionId !== NEW_SESSION_PENDING_KEY;
  const isNewTaskCreationPending = pendingSessionSelectionId === NEW_SESSION_PENDING_KEY;
  // Existing-task loads and their follow-up project trust lookup block actions without
  // visually dimming the global New Task control.
  const isTaskTransitionPending = isTaskSelectionPending || isProjectSkillsTrustLoading;
  const isProjectOrderSaving = projectOrderState.pendingRequestId !== null;
  const isProjectSkillsTrustSaving = projectSkillsTrustSavingDecision !== null;
  const areAgentSettingsLockedOutsideTaskTransition =
    !isAuthTransitionReady ||
    isInitializing ||
    isStarting ||
    isPermissionModeUpdating ||
    isNewTaskCreationPending ||
    isSubmitting ||
    isProjectOrderSaving ||
    isProjectRootRegistrationPending ||
    isProjectRemovalPending ||
    isAgentModelCatalogLoading ||
    isProjectSkillsTrustSaving ||
    projectSkillsTrustPrompt !== null;
  const areAgentSettingsLocked =
    areAgentSettingsLockedOutsideTaskTransition || isTaskTransitionPending;
  const hasStartedAgentSession =
    hasAgentUserMessage(timelineItems) ||
    sessions.some((session) => session.id === activeSessionId && session.messageCount > 0);
  const isAgentModelLocked = hasStartedAgentSession || Boolean(activeRunId) || isSubmitting;
  useLayoutEffect(() => {
    isAgentModelLockedRef.current = isAgentModelLocked;
  }, [isAgentModelLocked]);
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
  const agentRunningSessionIds = useMemo(() => {
    const ids = new Set(Object.keys(activeRunsBySession));
    for (const sessionId of pendingSendSessionIds) {
      if (sessionId !== NEW_SESSION_PENDING_KEY) ids.add(sessionId);
    }
    return ids;
  }, [activeRunsBySession, pendingSendSessionIds]);
  const runningSessionIds = useMemo(() => {
    const ids = new Set(agentRunningSessionIds);
    if (pendingSessionSelectionId && pendingSessionSelectionId !== NEW_SESSION_PENDING_KEY) {
      ids.add(pendingSessionSelectionId);
    }
    return ids;
  }, [agentRunningSessionIds, pendingSessionSelectionId]);
  const agentSidebarStatus = useMemo(() => {
    const sessionById = new Map(sessions.map((session) => [session.id, session]));
    const isVisibleSessionId = (sessionId: string) => {
      const session = sessionById.get(sessionId);
      return !session || !removedProjectRoots.has(session.projectRoot);
    };
    const visibleRunningSessionIds = new Set(
      [...agentRunningSessionIds].filter(isVisibleSessionId)
    );
    if (pendingSendSessionIds.has(NEW_SESSION_PENDING_KEY)) {
      visibleRunningSessionIds.add(NEW_SESSION_PENDING_KEY);
    }
    const visibleUnreadSessionIds = new Set(
      [...completedUnreadSessionIds].filter(isVisibleSessionId)
    );
    return aggregateAgentSidebarStatus(visibleRunningSessionIds, visibleUnreadSessionIds);
  }, [
    completedUnreadSessionIds,
    agentRunningSessionIds,
    pendingSendSessionIds,
    removedProjectRoots,
    sessions
  ]);

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

  const markPendingSend = useCallback(
    (sessionKey: string, token: number) => {
      pendingSendTokensRef.current.set(sessionKey, token);
      setPendingSendSessionIds((current) => {
        if (current.has(sessionKey)) return current;
        const next = new Set(current);
        next.add(sessionKey);
        return next;
      });
    },
    [pendingSendTokensRef]
  );

  const movePendingSend = useCallback(
    (fromKey: string, toKey: string, token: number) => {
      if (fromKey === toKey || pendingSendTokensRef.current.get(fromKey) !== token) return;
      pendingSendTokensRef.current.delete(fromKey);
      pendingSendTokensRef.current.set(toKey, token);
      setPendingSendSessionIds((current) => {
        const next = new Set(current);
        next.delete(fromKey);
        next.add(toKey);
        return next;
      });
    },
    [pendingSendTokensRef]
  );

  const clearPendingSend = useCallback(
    (sessionKey: string, token?: number) => {
      if (token !== undefined && pendingSendTokensRef.current.get(sessionKey) !== token) return;
      if (!pendingSendTokensRef.current.delete(sessionKey)) return;
      setPendingSendSessionIds((current) => {
        if (!current.has(sessionKey)) return current;
        const next = new Set(current);
        next.delete(sessionKey);
        return next;
      });
    },
    [pendingSendTokensRef]
  );

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
      const activeRunIds = new Set(Object.values(activeRuns));
      thoughtPhaseSeededRunIdsRef.current.forEach((runId) => {
        if (!activeRunIds.has(runId)) thoughtPhaseSeededRunIdsRef.current.delete(runId);
      });
      seedActiveThoughtPhases(activeRuns);
    },
    [seedActiveThoughtPhases, thoughtPhaseSeededRunIdsRef]
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

  const bumpTimelineRevision = useCallback(
    (sessionId: string): number => {
      const revision = (timelineRevisionBySessionRef.current.get(sessionId) || 0) + 1;
      timelineRevisionBySessionRef.current.set(sessionId, revision);
      return revision;
    },
    [timelineRevisionBySessionRef]
  );

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
    [bumpTimelineRevision, timelineRevisionBySessionRef]
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
    ): Promise<{ projectRoot: string; roots: RecentProjectRoot[]; config: AgentConfig }> => {
      return await enqueueProjectRootMutation(async () => {
        const existingOrder = projectOrderForExistingRegistration(
          orderedVisibleRoots,
          projectOrderState.confirmed,
          path
        );
        // A legacy-capped project can already be visible through session history without being in
        // recent_roots.json. Add only that project after the confirmed roots instead of promoting
        // it or materializing unrelated session-derived projects.
        if (!existingOrder) {
          return await agentRuntimeService.saveRecentProjectRoot(userId, path);
        }

        const roots = await agentRuntimeService.saveProjectRootOrder(userId, existingOrder);
        const config = await agentRuntimeService.loadConfig(userId);
        const nextConfig: AgentConfig = { ...config, defaultProjectRoot: path };
        await agentRuntimeService.saveConfig(userId, nextConfig);
        return { projectRoot: path, roots, config: nextConfig };
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
  }, [deletedSessionIdsRef, trackAgentWorkflow, userId]);

  const refreshSessions = useCallback(async () => {
    return await trackAgentWorkflow(async () => {
      if (!isTauriDesktop()) return;
      const runStateGeneration = runStateGenerationRef.current;
      const status = await agentRuntimeService.getRuntimeStatus(userId);
      applyRuntimeStatus(status, runStateGeneration);
      if (!status.running) {
        // Session history is account-scoped local data and does not require a live runtime.
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
        const removedRoots = new Set(config.removedProjectRoots || []);
        setRemovedProjectRoots(removedRoots);
        setMcpServers(savedMcpServers);
        setNewChatMcpServerNames(
          new Set(savedMcpServers.filter((server) => server.enabled).map((server) => server.name))
        );
        setIsMcpServersLoading(false);
        const root = firstVisibleProjectRoot(
          [
            config.defaultProjectRoot,
            status.projectRoot,
            ...roots.map((candidate) => candidate.path)
          ],
          removedRoots
        );
        const configuredModel = status.model || config.defaultModel || DEFAULT_MODEL;
        agentModelPreferenceRef.current.configuredDefault = configuredModel;
        isAgentModelLockedRef.current = false;
        const selectableModels = selectableAgentModelsRef.current;
        if (selectableModels) reconcileNewTaskModel(selectableModels);
        else restoreNewTaskModel();
        const nextMode = normalizeAgentPermissionMode(status.mode);
        setProjectRoot(root);
        applyAuthoritativeMode(nextMode);

        // Session history is local account data and remains browseable before
        // the authenticated native runtime is started.
        await refreshSessionList();
        if (cancelled || interactionGenerationRef.current !== initializationGeneration) {
          return;
        }

        if (status.running) {
          await refreshSessions();
        } else if (root) {
          const startRunStateGeneration = runStateGenerationRef.current;
          const startedStatus = await agentRuntimeService.startRuntime(userId, {
            projectRoot: root,
            model: agentModelPreferenceRef.current.configuredDefault,
            mode: nextMode
          });
          if (cancelled || interactionGenerationRef.current !== initializationGeneration) {
            return;
          }
          applyRuntimeStatus(startedStatus, startRunStateGeneration);
          await refreshSessions();
        }
      } catch (loadError) {
        if (!cancelled && interactionGenerationRef.current === initializationGeneration) {
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
        if (!cancelled && interactionGenerationRef.current === initializationGeneration) {
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
    agentModelPreferenceRef,
    applyAuthoritativeMode,
    applyRuntimeStatus,
    reconcileNewTaskModel,
    refreshSessionList,
    refreshSessions,
    restoreNewTaskModel,
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
          setIsProjectRootRegistrationPending(true);
          try {
            const registration = await registerProjectRoot(
              selected,
              visibleProjectRootsRef.current
            );
            invalidateSessionSelection();
            agentSessionSelection.forget(userId);
            shouldAutoScrollRef.current = true;
            setProjectRoot(registration.projectRoot);
            activeSessionIdRef.current = null;
            setActiveSessionId(null);
            setTimelineItems([]);
            restoreNewTaskModel();
            setRemovedProjectRoots(new Set(registration.config.removedProjectRoots || []));
            dispatchProjectOrder({ type: "replace", roots: registration.roots });
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
    restoreNewTaskModel,
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
      restoreNewTaskModel();
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
      restoreNewTaskModel,
      userId
    ]
  );

  const selectModel = useCallback(
    (value: string) => {
      if (isAgentModelLockedRef.current || currentAgentModelRef.current === value) return;
      interactionGenerationRef.current += 1;
      agentModelPreferenceRef.current.preferred = value;
      persistAgentModelPreference(value);
      currentAgentModelRef.current = value;
      setModel(value);
    },
    [agentModelPreferenceRef]
  );

  useEffect(() => {
    let cancelled = false;
    authoritativeModelCatalogRef.current = null;
    selectableAgentModelsRef.current = null;
    setIsAgentModelCatalogLoading(true);

    void (async () => {
      try {
        const modelClient = openSecretRef.current as unknown as ModelCatalogClient;
        const {
          setAvailableModels: updateAvailableModels,
          setModelAliases: updateModelAliases,
          setHasWhisperModel: updateHasWhisperModel
        } = agentModelStateSettersRef.current;

        if (modelClient.fetchModelCatalog) {
          try {
            const catalog = await modelClient.fetchModelCatalog();
            if (cancelled) return;
            const selectableModels = catalog.data.filter(isSelectableChatModel);
            const hasCatalogWhisperModel = catalog.data.some(
              (catalogModel) => catalogModel.id === "whisper-large-v3"
            );
            authoritativeModelCatalogRef.current = catalog;
            selectableAgentModelsRef.current = selectableModels;
            updateAvailableModels(selectableModels);
            updateModelAliases(catalog.aliases);
            updateHasWhisperModel(
              catalog.audio?.transcription?.available ?? hasCatalogWhisperModel
            );
            reconcileNewTaskModel(selectableModels);
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
          if (cancelled) return;
          const availableGenerateModels = models.filter((availableModel) => {
            const tasks = availableModel.tasks || [];
            if (tasks.length > 0) return tasks.includes("generate");
            const id = availableModel.id.toLowerCase();
            return !id.includes("whisper") && !id.includes("embed");
          });
          updateHasWhisperModel(
            models.some((availableModel) => availableModel.id === "whisper-large-v3")
          );
          selectableAgentModelsRef.current = availableGenerateModels;
          updateAvailableModels(availableGenerateModels);
          updateModelAliases(buildFallbackModelAliases(availableGenerateModels));
          reconcileNewTaskModel(availableGenerateModels);
        }
      } catch (fetchError) {
        if (import.meta.env.DEV) {
          console.warn("Failed to fetch model metadata:", fetchError);
        }
      } finally {
        if (!cancelled) setIsAgentModelCatalogLoading(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [reconcileNewTaskModel, userId]);

  const contextLimitForModel = useCallback(
    (modelId: string) =>
      resolveAgentModelContextLimit(modelId, authoritativeModelCatalogRef.current),
    []
  );

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
    [applyAuthoritativeMode, permissionModeUpdateRef, userId]
  );

  const startRuntime = useCallback(
    async (restart = false, requestedProjectRoot = projectRoot) => {
      const requestGeneration = startRequestGenerationRef.current + 1;
      startRequestGenerationRef.current = requestGeneration;
      const interactionGeneration = interactionGenerationRef.current;
      setError(null);
      setIsStarting(true);
      try {
        return await trackAgentWorkflow(async () => {
          if (!requestedProjectRoot.trim()) {
            throw new Error("Select a project folder first");
          }
          const targetProjectRoot = requestedProjectRoot;
          const requestedMode = selectedModeRef.current;
          const request = {
            projectRoot: targetProjectRoot,
            model: agentModelPreferenceRef.current.configuredDefault,
            mode: requestedMode
          };
          const runStateGeneration = runStateGenerationRef.current;
          const restartOutcome = restart
            ? await agentRuntimeService.restartRuntime(userId, request)
            : null;
          const status = restartOutcome
            ? restartOutcome.status
            : await agentRuntimeService.startRuntime(userId, request);
          if (
            startRequestGenerationRef.current !== requestGeneration ||
            interactionGenerationRef.current !== interactionGeneration
          ) {
            return status;
          }
          applyRuntimeStatus(status, runStateGeneration);
          setProjectRoot(status.projectRoot || targetProjectRoot);
          applyAuthoritativeMode(normalizeAgentPermissionMode(status.mode || requestedMode));
          await refreshSessions();
          if (restartOutcome?.acpShutdownError) {
            setError(
              `Agent Mode restarted, but ACP cleanup failed: ${restartOutcome.acpShutdownError}`
            );
          }
          return status;
        });
      } catch (startError) {
        if (
          startRequestGenerationRef.current === requestGeneration &&
          interactionGenerationRef.current === interactionGeneration
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
      agentModelPreferenceRef,
      applyAuthoritativeMode,
      applyRuntimeStatus,
      projectRoot,
      refreshSessions,
      trackAgentWorkflow,
      userId
    ]
  );

  const ensureRuntimeAndSession = useCallback(
    async (
      expectedSelectionGeneration: number,
      expectedInteractionGeneration: number,
      requestedSessionId: string | null
    ) => {
      if (!projectRoot) {
        throw new Error("Select a project folder first");
      }

      const requestModel = requestedSessionId
        ? currentAgentModelRef.current
        : newTaskAgentModel(agentModelPreferenceRef.current);

      const status = await agentRuntimeService.getRuntimeStatus(userId);
      if (!status.running) {
        await startRuntime(false);
      }

      let sessionId = requestedSessionId;
      if (!sessionId) {
        const detail = await agentRuntimeService.createSession(userId, {
          projectRoot,
          title: "New task",
          model: requestModel,
          contextLimit: contextLimitForModel(requestModel),
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
          isAgentModelLockedRef.current = false;
          currentAgentModelRef.current = requestModel;
          setModel(requestModel);
          applyAuthoritativeMode(normalizeAgentPermissionMode(detail.session.mode));
          replaceSessionTimeline(sessionId, detail.timeline);
          const mcpError = mcpConnectionErrorMessage(detail.mcpErrors);
          if (mcpError) setError(mcpError);
        }
      }

      return { sessionId, requestModel };
    },
    [
      agentModelPreferenceRef,
      applyAuthoritativeMode,
      agentSessionSelection,
      contextLimitForModel,
      deletedSessionIdsRef,
      projectRoot,
      replaceSessionTimeline,
      selectedNewChatMcpServerNames,
      startRuntime,
      userId
    ]
  );

  const createSession = useCallback(
    async (requestedProjectRoot = projectRoot) => {
      if (isAgentModelCatalogLoading) return;
      if (!requestedProjectRoot.trim()) {
        setError("Select a project folder before creating a task");
        return;
      }
      const targetProjectRoot = requestedProjectRoot;
      if (pendingSessionSelectionIdRef.current === NEW_SESSION_PENDING_KEY) return;
      const requestModel = newTaskAgentModel(agentModelPreferenceRef.current);
      const selectionGeneration = beginSessionSelection(NEW_SESSION_PENDING_KEY);
      const interactionGeneration = interactionGenerationRef.current;
      setError(null);
      try {
        const detail = await trackAgentWorkflow(async () => {
          if (!runtimeStatus?.running) {
            await startRuntime(false, targetProjectRoot);
          }
          return await agentRuntimeService.createSession(userId, {
            projectRoot: targetProjectRoot,
            title: "New task",
            model: requestModel,
            contextLimit: contextLimitForModel(requestModel),
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
          setProjectRoot(detail.session.projectRoot);
          isAgentModelLockedRef.current = false;
          currentAgentModelRef.current = requestModel;
          setModel(requestModel);
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
    },
    [
      agentModelPreferenceRef,
      applyAuthoritativeMode,
      agentSessionSelection,
      beginSessionSelection,
      contextLimitForModel,
      deletedSessionIdsRef,
      finishSessionSelection,
      isAgentModelCatalogLoading,
      projectRoot,
      replaceSessionTimeline,
      runtimeStatus?.running,
      selectedNewChatMcpServerNames,
      startRuntime,
      trackAgentWorkflow,
      userId
    ]
  );

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
        clearCompletedUnreadSession(detail.session.id);
        setActiveSessionId(detail.session.id);
        agentSessionSelection.remember(userId, detail.session.id);
        setProjectRoot(detail.session.projectRoot);
        const isModelLocked =
          detail.session.messageCount > 0 ||
          hasAgentUserMessage(detail.timeline) ||
          Boolean(activeRunsBySessionRef.current[detail.session.id]) ||
          pendingSendTokensRef.current.has(detail.session.id);
        isAgentModelLockedRef.current = isModelLocked;
        const sessionModel = resolveAgentModelForSession(
          newTaskAgentModel(agentModelPreferenceRef.current),
          detail.session.model,
          isModelLocked
        );
        currentAgentModelRef.current = sessionModel;
        setModel(sessionModel);
        applyAuthoritativeMode(normalizeAgentPermissionMode(detail.session.mode));
        setTimelineItems(detail.timeline);
        if (activeRunsBySessionRef.current[detail.session.id]) {
          thoughtPhaseTrackerRef.current.seedActiveTimeline(detail.session.id, detail.timeline);
          observeActiveThoughtPhase(detail.session.id);
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
      agentModelPreferenceRef,
      applyAuthoritativeMode,
      agentSessionSelection,
      beginSessionSelection,
      clearCompletedUnreadSession,
      deletedSessionIdsRef,
      finishSessionSelection,
      observeActiveThoughtPhase,
      pendingSendTokensRef,
      persistSelectedProjectRoot,
      replaceSessionTimeline,
      thoughtPhaseTrackerRef,
      timelineRevisionBySessionRef,
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
    const rememberedSessionId = agentSessionSelection.resolve(userId, visibleSessions);
    if (rememberedSessionId) {
      void loadSession(rememberedSessionId);
    }
  }, [
    agentSessionSelection,
    isAuthTransitionReady,
    isInitializing,
    isSessionHistoryReady,
    loadSession,
    visibleSessions,
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
        const { sessionId, requestModel } = await ensureRuntimeAndSession(
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
          model: requestModel,
          contextLimit: contextLimitForModel(requestModel),
          mode: selectedModeRef.current,
          visionCapable: resolveAgentModelVisionCapability(
            requestModel,
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
    cancelledPendingSendTokensRef,
    clearPendingSend,
    contextLimitForModel,
    deletedSessionIdsRef,
    ensureRuntimeAndSession,
    input,
    isAgentSendLocked,
    markPendingSend,
    mergeSessionTimelineItem,
    modelAliases,
    movePendingSend,
    pendingSendTokensRef,
    permissionModeUpdateRef,
    recordActiveRun,
    scrollTimelineToBottom,
    terminalRunIdsRef,
    thoughtPhaseTrackerRef,
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
  }, [activeRunId, cancelledPendingSendTokensRef, pendingSendTokensRef, userId]);

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
        restoreNewTaskModel();
      }
    },
    [
      agentSessionSelection,
      cancelThoughtLabelDisplays,
      clearActiveRun,
      deletedSessionIdsRef,
      restoreNewTaskModel,
      thoughtPhaseTrackerRef,
      timelineRevisionBySessionRef,
      userId
    ]
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

  const removeProjectRoot = useCallback(
    async (root: RecentProjectRoot) => {
      if (isProjectRemovalPending) return;
      const visibleRoots = visibleProjectRootsRef.current;
      const fallback = projectRootFallbackAfterRemoval(visibleRoots, root.path);
      setError(null);
      setProjectRemovalError(null);
      setIsProjectRemovalPending(true);
      try {
        const config = await enqueueProjectRootMutation(() =>
          agentRuntimeService.removeProjectRoot(userId, root.path, fallback)
        );
        setRemovedProjectRoots(new Set(config.removedProjectRoots || []));
        setProjectRemovalError(null);
        setProjectToRemove(null);

        if (projectRoot === root.path) {
          invalidateSessionSelection();
          agentSessionSelection.forget(userId);
          activeSessionIdRef.current = null;
          setActiveSessionId(null);
          setTimelineItems([]);
          setSessionMcpServers([]);
          setSessionMcpServersSessionId(null);
          setProjectRoot(fallback || "");
          restoreNewTaskModel();
          shouldAutoScrollRef.current = true;
        }
        await refreshSessions().catch((refreshError) => {
          setError(errorMessage(refreshError));
        });
      } catch (removeError) {
        setProjectRemovalError(errorMessage(removeError));
      } finally {
        setIsProjectRemovalPending(false);
      }
    },
    [
      agentSessionSelection,
      enqueueProjectRootMutation,
      invalidateSessionSelection,
      isProjectRemovalPending,
      projectRoot,
      refreshSessions,
      restoreNewTaskModel,
      userId
    ]
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

  const upsertSessionSummary = useCallback(
    (summary: AgentSessionSummary) => {
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
    },
    [deletedSessionIdsRef]
  );

  const observeLiveThoughtItem = useCallback(
    (sessionId: string, item: AgentTimelineItem) => {
      const previousActivePhase = thoughtPhaseTrackerRef.current.activePhase(sessionId);
      const completedPhase = thoughtPhaseTrackerRef.current.observeTimelineItem(sessionId, item);
      if (completedPhase) {
        completeThoughtPhase(completedPhase);
      } else {
        const activePhase = thoughtPhaseTrackerRef.current.activePhase(sessionId);
        if (previousActivePhase && previousActivePhase.phaseId !== activePhase?.phaseId) {
          thoughtLabelProvisionalSchedulerRef.current?.complete(
            previousActivePhase.sessionId,
            previousActivePhase.phaseId
          );
        }
      }
      observeActiveThoughtPhase(sessionId);
    },
    [completeThoughtPhase, observeActiveThoughtPhase, thoughtPhaseTrackerRef]
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
          if (event.runId) {
            terminalRunIdsRef.current.add(event.runId);
            thoughtPhaseSeededRunIdsRef.current.delete(event.runId);
          }
          let finishedTimelineRevision: number | undefined;
          if (event.sessionId) {
            finishedTimelineRevision = bumpTimelineRevision(event.sessionId);
            clearActiveRun(event.sessionId, event.runId || undefined);
          }
          // The terminal event is authoritative for run state. Refresh only
          // persisted session metadata here: the native task removes its
          // active-run entry immediately after emitting this event, so a
          // concurrent status snapshot could otherwise resurrect the run.
          void refreshSessionList().catch(() => {});
          const thoughtRunFinished = handleAgentModeThoughtRunFinished({
            event,
            timelineRevision: finishedTimelineRevision,
            tracker: thoughtPhaseTrackerRef.current,
            finalizePhase: completeThoughtPhase,
            releaseProvisional: (phase) => {
              thoughtLabelProvisionalSchedulerRef.current?.complete(phase.sessionId, phase.phaseId);
            },
            cancelAndInvalidateLabels: (sessionId, assistantTurnId) => {
              if (assistantTurnId) {
                invalidateThoughtLabelsForTurn(sessionId, assistantTurnId);
              } else {
                invalidateThoughtLabelsForSession(sessionId);
              }
            },
            loadTimeline: async (sessionId) =>
              (await agentRuntimeService.loadSession(userId, sessionId)).timeline,
            canApplyTimeline: (sessionId) =>
              isAgentModeMountedRef.current &&
              userIdRef.current === userId &&
              !deletedSessionIdsRef.current.has(sessionId),
            replaceTimeline: replaceSessionTimeline
          });
          if (thoughtRunFinished) {
            if (event.message === "completed" && event.sessionId !== activeSessionIdRef.current) {
              markCompletedUnreadSession(event.sessionId!);
            }
            void thoughtRunFinished.catch(() => {});
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
              if (
                !isAgentModeMountedRef.current ||
                userIdRef.current !== userId ||
                deletedSessionIdsRef.current.has(id)
              ) {
                return;
              }
              const replaced = replaceSessionTimeline(id, detail.timeline, historyTimelineRevision);
              if (replaced && activeRunsBySessionRef.current[id]) {
                thoughtPhaseTrackerRef.current.seedActiveTimeline(id, detail.timeline);
                observeActiveThoughtPhase(id);
              }
            } catch (historyError) {
              if (
                isAgentModeMountedRef.current &&
                userIdRef.current === userId &&
                activeSessionIdRef.current === id
              ) {
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
      completeThoughtPhase,
      deletedSessionIdsRef,
      invalidateThoughtLabelsForSession,
      invalidateThoughtLabelsForTurn,
      markCompletedUnreadSession,
      mergeSessionTimelineItem,
      observeLiveThoughtItem,
      observeActiveThoughtPhase,
      refreshSessionList,
      recordActiveRun,
      refreshSessionMcpServers,
      replaceSessionTimeline,
      terminalRunIdsRef,
      thoughtPhaseSeededRunIdsRef,
      thoughtPhaseTrackerRef,
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

  const handleCreateSession = useCallback(() => {
    void createSession(projectRoot);
  }, [createSession, projectRoot]);
  const handleCreateSessionForProject = useCallback(
    (targetProjectRoot: string) => {
      if (targetProjectRoot !== projectRoot) {
        selectProjectRoot(targetProjectRoot);
      }
      void createSession(targetProjectRoot);
    },
    [createSession, projectRoot, selectProjectRoot]
  );
  const handlePromptProjectRemoval = useCallback((root: AgentProjectRootView) => {
    setProjectRemovalError(null);
    setProjectToRemove(root);
  }, []);
  const handlePromptProjectRename = useCallback((root: AgentProjectRootView) => {
    setProjectToRename(root);
  }, []);
  const handleRenameProject = useCallback(
    (root: AgentProjectRootView, displayName: string) => {
      commitSidebarPreferences((current) =>
        renameAgentProjectDisplayName(current, root, displayName)
      );
    },
    [commitSidebarPreferences]
  );
  const handleToggleProjectDisclosure = useCallback(
    (path: string) => {
      commitSidebarPreferences((current) => toggleAgentProjectCollapsed(current, path));
    },
    [commitSidebarPreferences]
  );
  const handleRevealProjectRoot = useCallback(
    (path: string) => {
      void revealAgentProjectFolder(path).catch((revealError) => {
        console.error("Unable to reveal Agent project folder", revealError);
        showNotification({
          type: "error",
          title: "Couldn’t reveal project folder",
          message: "The folder may have been moved or is no longer available.",
          duration: 7000
        });
      });
    },
    [showNotification]
  );
  const handleSelectSession = useCallback(
    (sessionId: string) => {
      void loadSession(sessionId);
    },
    [loadSession]
  );
  const handleManageMcpServers = useCallback(() => {
    setIsMcpServersDialogOpen(true);
  }, []);
  const handleSendMessage = useCallback(() => {
    void sendMessage();
  }, [sendMessage]);
  const handleToggleAgentFullscreen = useCallback(() => {
    setIsAgentFullscreen((current) => !current);
  }, []);

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
          <MemoizedAgentSidebarContent
            activeSessionId={
              pendingSessionSelectionId && pendingSessionSelectionId !== NEW_SESSION_PENDING_KEY
                ? pendingSessionSelectionId
                : activeSessionId
            }
            collapsedProjectRoots={sidebarPreferences.collapsedProjectRoots}
            isTouchLayout={isTouchLayout}
            projectRoot={projectRoot}
            recentRoots={displayProjectRoots}
            completedUnreadSessionIds={completedUnreadSessionIds}
            disabled={areAgentSettingsLocked}
            inProgressSessionIds={agentRunningSessionIds}
            runningSessionIds={runningSessionIds}
            sessions={visibleSessions}
            onChooseProjectRoot={chooseProjectRoot}
            onCreateSession={handleCreateSessionForProject}
            onProjectDisclosureToggle={handleToggleProjectDisclosure}
            onProjectOrderChange={saveProjectRootOrder}
            onProjectRename={handlePromptProjectRename}
            onProjectRemove={handlePromptProjectRemoval}
            onRevealProjectRoot={handleRevealProjectRoot}
            onSessionDelete={setSessionToDelete}
            onSessionSelect={handleSelectSession}
          />
        }
        isNewItemTemporarilyDisabled={isTaskTransitionPending}
        onNewItem={
          areAgentSettingsLockedOutsideTaskTransition || !projectRoot
            ? undefined
            : handleCreateSession
        }
        onToggle={toggleSidebar}
      />

      {projectToRename ? (
        <RenameAgentProjectDialog
          key={projectToRename.path}
          open
          currentDisplayName={projectToRename.displayName}
          onOpenChange={(open) => {
            if (!open) setProjectToRename(null);
          }}
          onRename={(displayName) => handleRenameProject(projectToRename, displayName)}
        />
      ) : null}

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

      <AlertDialog
        open={projectToRemove !== null}
        onOpenChange={(open) => {
          if (!open && !isProjectRemovalPending) {
            setProjectRemovalError(null);
            setProjectToRemove(null);
          }
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Remove {projectToRemove?.displayName || "project"}?</AlertDialogTitle>
            <AlertDialogDescription>
              Removing this project from Maple won&apos;t delete its files or existing tasks. Add
              the same folder again to restore its tasks on this account and device.
            </AlertDialogDescription>
          </AlertDialogHeader>
          {projectToRemove ? (
            <div className="rounded-md border bg-muted/40 px-3 py-2 font-mono text-xs break-all">
              {projectToRemove.path}
            </div>
          ) : null}
          {projectRemovalError ? (
            <div
              role="alert"
              className="flex items-start gap-2 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive"
            >
              <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
              <span className="min-w-0 break-words">{projectRemovalError}</span>
            </div>
          ) : null}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={isProjectRemovalPending}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              disabled={isProjectRemovalPending || !projectToRemove}
              onClick={(event) => {
                event.preventDefault();
                if (projectToRemove) void removeProjectRoot(projectToRemove);
              }}
            >
              {isProjectRemovalPending ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
              {projectRemovalError ? "Retry" : "Remove"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

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
            <SidebarToggle onToggle={toggleSidebar} agentStatus={agentSidebarStatus} />
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
            onNewChat={handleCreateSession}
            newItemLabel="New Task"
          />
        ) : null}

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
                "mx-auto w-full",
                timelineItems.length > 0
                  ? "max-w-4xl p-4 md:p-6 landscape-short:p-2"
                  : isAgentFullscreen
                    ? "flex min-h-full max-w-6xl flex-col p-4 md:p-6 landscape-short:p-2"
                    : "flex min-h-full flex-col px-4"
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
                  recentRoots={displayProjectRoots}
                  isExpanded={isAgentFullscreen}
                  onCancelPrompt={cancelPrompt}
                  onChooseProjectRoot={chooseProjectRoot}
                  onInputChange={setInput}
                  onKeyDown={handleKeyDown}
                  onManageMcpServers={handleManageMcpServers}
                  onMcpToggle={toggleMcpServer}
                  onModeChange={selectMode}
                  onModelChange={selectModel}
                  onProjectRootChange={selectProjectRoot}
                  onSendMessage={handleSendMessage}
                  onToggleExpanded={handleToggleAgentFullscreen}
                />
              ) : (
                <AgentTimeline
                  items={timelineItems}
                  isResponsePending={isSending}
                  isRunActive={Boolean(activeRunId) && !isSubmitting}
                  generatedThoughtLabels={generatedThoughtLabels}
                  sessionId={activeSessionId}
                  onPermissionDecision={respondToPermission}
                />
              )}
            </div>
          </div>

          {timelineItems.length > 0 ? (
            <div className="shrink-0 bg-background pb-[env(safe-area-inset-bottom)]">
              <div className="mx-auto max-w-4xl px-4 landscape-short:px-3">
                <MemoizedAgentComposer
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
                  recentRoots={displayProjectRoots}
                  onCancelPrompt={cancelPrompt}
                  onChooseProjectRoot={chooseProjectRoot}
                  onInputChange={setInput}
                  onKeyDown={handleKeyDown}
                  onManageMcpServers={handleManageMcpServers}
                  onMcpToggle={toggleMcpServer}
                  onModeChange={selectMode}
                  onModelChange={selectModel}
                  onProjectRootChange={selectProjectRoot}
                  onSendMessage={handleSendMessage}
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
        "flex w-full flex-col",
        isExpanded
          ? "min-h-0 flex-1 items-center justify-center"
          : "mx-auto min-h-0 max-w-[650px] flex-1 justify-center"
      )}
    >
      {!isExpanded ? <div className="mb-16 landscape-short:mb-4" /> : null}

      <div className="flex w-full flex-col items-center gap-6 text-center landscape-short:gap-3">
        {!isExpanded ? (
          <h1 className="mb-6 w-full overflow-visible pb-1 text-center font-displayWide text-4xl font-normal leading-tight brand-gradient-text landscape-short:mb-2 landscape-short:text-2xl sm:leading-relaxed">
            Work on anything...
          </h1>
        ) : null}
        <MemoizedAgentComposer {...props} />
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
  collapsedProjectRoots: ReadonlySet<string>;
  isTouchLayout: boolean;
  projectRoot: string;
  recentRoots: AgentProjectRootView[];
  completedUnreadSessionIds: Set<string>;
  disabled: boolean;
  inProgressSessionIds: Set<string>;
  runningSessionIds: Set<string>;
  sessions: AgentSessionSummary[];
  onChooseProjectRoot: () => void;
  onCreateSession: (projectRoot: string) => void;
  onProjectDisclosureToggle: (path: string) => void;
  onProjectOrderChange: (roots: AgentProjectRootView[]) => void;
  onProjectRename: (root: AgentProjectRootView) => void;
  onProjectRemove: (root: AgentProjectRootView) => void;
  onRevealProjectRoot: (projectRoot: string) => void;
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
  isCollapsed: boolean;
}

interface ProjectDragState extends PendingProjectPointer {
  clientX: number;
  clientY: number;
  insertionIndex: number | null;
  markerTop: number | null;
}

interface AgentSidebarTaskRowProps {
  actionsLocked: boolean;
  isActive: boolean;
  isInProgress: boolean;
  isRunning: boolean;
  isTouchLayout: boolean;
  isUnreadCompleted: boolean;
  onDelete: (session: AgentSessionSummary) => void;
  onRevealProjectRoot: (projectRoot: string) => void;
  onSelect: (sessionId: string) => void;
  projectDisplayName: string;
  rowRef: (node: HTMLDivElement | null) => void;
  session: AgentSessionSummary;
}

function AgentSidebarTaskRow({
  actionsLocked,
  isActive,
  isInProgress,
  isRunning,
  isTouchLayout,
  isUnreadCompleted,
  onDelete,
  onRevealProjectRoot,
  onSelect,
  projectDisplayName,
  rowRef,
  session
}: AgentSidebarTaskRowProps) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [hasKeyboardFocusWithin, setHasKeyboardFocusWithin] = useState(false);
  const [infoCardOpen, setInfoCardOpen] = useState(false);
  const title = sessionTitle(session);
  const hasVisualStatus = isRunning || isUnreadCompleted;
  const visibleInfoCardOpen = !isTouchLayout && infoCardOpen;

  useEffect(() => {
    if (isTouchLayout) setInfoCardOpen(false);
  }, [isTouchLayout]);

  const activeSurface =
    menuOpen || hasKeyboardFocusWithin || visibleInfoCardOpen
      ? isActive
        ? "bg-[hsl(var(--sidebar-row-selected-hover))] ring-1 ring-ring/70"
        : "bg-[hsl(var(--sidebar-row-hover))] ring-1 ring-ring/70"
      : null;
  const taskSelectionButton = (
    <button
      type="button"
      className={cn(
        "relative z-0 flex min-w-0 flex-1 cursor-pointer items-center pl-8 text-left text-sm text-foreground/95 transition-colors focus-visible:outline-none aria-disabled:cursor-default",
        isTouchLayout ? "min-h-10" : "min-h-[30px]",
        isActive && "font-medium text-foreground",
        isTouchLayout
          ? hasVisualStatus
            ? "pr-[4.75rem]"
            : "pr-10"
          : hasVisualStatus
            ? "pr-8"
            : "pr-0"
      )}
      onClick={() => {
        if (!actionsLocked) onSelect(session.id);
      }}
      aria-disabled={actionsLocked || undefined}
      aria-current={isActive ? "page" : undefined}
      aria-label={agentTaskAccessibleLabel(title, {
        running: isRunning,
        unread: isUnreadCompleted
      })}
    >
      <span className="min-w-0 flex-1 truncate">{title}</span>
    </button>
  );

  return (
    <div
      ref={rowRef}
      className={cn(
        "group/task relative isolate flex w-full min-w-0 select-none items-stretch rounded-xl transition-colors motion-reduce:transition-none",
        isTouchLayout ? "min-h-10" : "min-h-[30px]",
        isActive
          ? "bg-[hsl(var(--sidebar-row-selected))] hover:bg-[hsl(var(--sidebar-row-selected-hover))]"
          : "hover:bg-[hsl(var(--sidebar-row-hover))]",
        activeSurface
      )}
      onContextMenu={(event) => event.preventDefault()}
      onFocusCapture={(event) => setHasKeyboardFocusWithin(isKeyboardFocusTarget(event.target))}
      onBlurCapture={(event) => {
        const nextTarget = event.relatedTarget;
        setHasKeyboardFocusWithin(
          event.currentTarget.contains(nextTarget as Node) && isKeyboardFocusTarget(nextTarget)
        );
      }}
    >
      <HoverCard
        open={visibleInfoCardOpen}
        openDelay={450}
        closeDelay={180}
        onOpenChange={setInfoCardOpen}
      >
        {isTouchLayout ? (
          taskSelectionButton
        ) : (
          <HoverCardTrigger asChild>{taskSelectionButton}</HoverCardTrigger>
        )}
        {!isTouchLayout ? (
          <HoverCardContent side="right">
            <AgentSidebarInfoCard
              folderPath={session.projectRoot}
              icon={MessageSquare}
              isInProgress={isInProgress}
              metadata={projectDisplayName}
              metadataIcon={Folder}
              onDismiss={() => setInfoCardOpen(false)}
              onOpenProjectFolder={() => onRevealProjectRoot(session.projectRoot)}
              progressLabel={isInProgress ? "In progress" : "Not in progress"}
              title={title}
            />
          </HoverCardContent>
        ) : null}
      </HoverCard>

      {hasVisualStatus ? (
        <span
          aria-hidden="true"
          className={cn(
            "pointer-events-none absolute top-1/2 z-40 flex -translate-y-1/2 items-center justify-center transition-opacity duration-150 motion-reduce:transition-none",
            isTouchLayout ? "right-10" : "right-2",
            !isTouchLayout && "group-hover/task:opacity-0",
            !isTouchLayout &&
              (menuOpen || hasKeyboardFocusWithin || visibleInfoCardOpen) &&
              "opacity-0"
          )}
        >
          {isRunning ? (
            <Loader2 className="h-3.5 w-3.5 text-[hsl(var(--maple-primary))] motion-safe:animate-spin" />
          ) : (
            <span className="h-2 w-2 rounded-full bg-maple-success" />
          )}
        </span>
      ) : null}

      <div
        className={taskActionRowClass(
          isTouchLayout,
          menuOpen,
          hasKeyboardFocusWithin || visibleInfoCardOpen
        )}
      >
        <div
          aria-hidden="true"
          className={cn(
            "pointer-events-none w-5 shrink-0 self-stretch bg-gradient-to-r from-transparent transition-colors",
            isActive
              ? "to-[hsl(var(--sidebar-row-selected))] group-hover/task:to-[hsl(var(--sidebar-row-selected-hover))]"
              : "to-[hsl(var(--sidebar))] group-hover/task:to-[hsl(var(--sidebar-row-hover))]",
            (menuOpen || hasKeyboardFocusWithin || visibleInfoCardOpen) &&
              (isActive
                ? "to-[hsl(var(--sidebar-row-selected-hover))]"
                : "to-[hsl(var(--sidebar-row-hover))]")
          )}
        />
        <div
          className={cn(
            "flex items-center rounded-r-xl pr-0.5",
            isActive
              ? "bg-[hsl(var(--sidebar-row-selected))] group-hover/task:bg-[hsl(var(--sidebar-row-selected-hover))]"
              : "bg-[hsl(var(--sidebar))] group-hover/task:bg-[hsl(var(--sidebar-row-hover))]",
            (menuOpen || hasKeyboardFocusWithin || visibleInfoCardOpen) &&
              (isActive
                ? "bg-[hsl(var(--sidebar-row-selected-hover))]"
                : "bg-[hsl(var(--sidebar-row-hover))]")
          )}
        >
          <DropdownMenu open={menuOpen} onOpenChange={setMenuOpen}>
            <DropdownMenuTrigger asChild>
              <button
                type="button"
                className={cn(AGENT_SIDEBAR_ACTION_BUTTON, isTouchLayout ? "h-9 w-9" : "h-7 w-7")}
                onClick={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                }}
                aria-label={`Open task menu for ${title}`}
              >
                <MoreHorizontal className="h-4 w-4" strokeWidth={SIDEBAR_ICON_STROKE} />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" collisionPadding={8} className="max-w-48">
              <DropdownMenuItem
                disabled={actionsLocked || isRunning}
                onClick={() => onDelete(session)}
              >
                <Trash className="mr-2 h-4 w-4 shrink-0" strokeWidth={SIDEBAR_ICON_STROKE} />
                <span className="min-w-0 whitespace-normal leading-snug">
                  {isRunning ? "Stop Agent Before Deleting Task" : "Delete Task"}
                </span>
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>
    </div>
  );
}

function AgentSidebarContent({
  activeSessionId,
  collapsedProjectRoots,
  isTouchLayout,
  projectRoot,
  recentRoots,
  completedUnreadSessionIds,
  disabled,
  inProgressSessionIds,
  runningSessionIds,
  sessions,
  onChooseProjectRoot,
  onCreateSession,
  onProjectDisclosureToggle,
  onProjectOrderChange,
  onProjectRename,
  onProjectRemove,
  onRevealProjectRoot,
  onSessionDelete,
  onSessionSelect
}: AgentSidebarContentProps) {
  const rowElementsRef = useLazyRef(() => new Map<string, HTMLElement>());
  const previousRowTopsRef = useLazyRef(() => new Map<string, number>());
  const projectListRef = useRef<HTMLDivElement>(null);
  const projectGroupElementsRef = useLazyRef(() => new Map<string, HTMLDivElement>());
  const projectHeaderElementsRef = useLazyRef(() => new Map<string, HTMLDivElement>());
  const pendingProjectPointerRef = useRef<PendingProjectPointer | null>(null);
  const projectDragRef = useRef<ProjectDragState | null>(null);
  const autoScrollFrameRef = useRef<number | null>(null);
  const autoScrollDirectionRef = useRef(0);
  const suppressProjectClickUntilRef = useRef(0);
  const [projectDrag, setProjectDrag] = useState<ProjectDragState | null>(null);
  const isProjectDragging = projectDrag !== null;
  const [openProjectInfoCardPath, setOpenProjectInfoCardPath] = useState<string | null>(null);
  const [openProjectMenuPath, setOpenProjectMenuPath] = useState<string | null>(null);
  const [keyboardFocusProjectPath, setKeyboardFocusProjectPath] = useState<string | null>(null);
  const projectRows = recentRoots;
  const sessionsByRoot = useMemo(() => groupAgentSessionsByRoot(sessions), [sessions]);

  useEffect(() => {
    if (isTouchLayout) setOpenProjectInfoCardPath(null);
  }, [isTouchLayout]);

  const setAnimatedRowRef = useCallback(
    (key: string, node: HTMLElement | null) => {
      if (node) {
        rowElementsRef.current.set(key, node);
      } else {
        rowElementsRef.current.delete(key);
      }
    },
    [rowElementsRef]
  );

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
  }, [collapsedProjectRoots, previousRowTopsRef, projectRows, rowElementsRef, sessions]);

  const setProjectGroupRef = useCallback(
    (path: string, node: HTMLDivElement | null) => {
      setAnimatedRowRef(`project:${path}`, node);
      if (node) {
        projectGroupElementsRef.current.set(path, node);
      } else {
        projectGroupElementsRef.current.delete(path);
      }
    },
    [projectGroupElementsRef, setAnimatedRowRef]
  );

  const setProjectHeaderRef = useCallback(
    (path: string, node: HTMLDivElement | null) => {
      if (node) {
        projectHeaderElementsRef.current.set(path, node);
      } else {
        projectHeaderElementsRef.current.delete(path);
      }
    },
    [projectHeaderElementsRef]
  );

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
    [projectGroupElementsRef, projectHeaderElementsRef, projectRows]
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
    (event: React.PointerEvent<HTMLDivElement>, root: AgentProjectRootView) => {
      if (
        disabled ||
        !event.isPrimary ||
        event.button !== 0 ||
        pendingProjectPointerRef.current ||
        projectDragRef.current
      ) {
        return;
      }
      setOpenProjectInfoCardPath(null);
      const rect = event.currentTarget.getBoundingClientRect();
      pendingProjectPointerRef.current = {
        pointerId: event.pointerId,
        path: root.path,
        name: root.displayName,
        startX: event.clientX,
        startY: event.clientY,
        grabOffsetX: event.clientX - rect.left,
        grabOffsetY: event.clientY - rect.top,
        ghostWidth: rect.width,
        headerElement: event.currentTarget,
        isCollapsed: collapsedProjectRoots.has(root.path)
      };
    },
    [collapsedProjectRoots, disabled]
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
          aria-label="Add project folder"
        >
          <FolderPlus className="h-4 w-4" />
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
            const inProgressSessionCount = projectSessions.reduce(
              (count, session) => count + (inProgressSessionIds.has(session.id) ? 1 : 0),
              0
            );
            const unreadSessionCount = projectSessions.reduce(
              (count, session) => count + (completedUnreadSessionIds.has(session.id) ? 1 : 0),
              0
            );
            const hasUnreadCompletedSession = unreadSessionCount > 0;
            const showProjectRunningIndicator = isCollapsed && hasRunningSession;
            const showProjectUnreadIndicator =
              isCollapsed && !hasRunningSession && hasUnreadCompletedSession;
            const hasProjectStatus = showProjectRunningIndicator || showProjectUnreadIndicator;
            const projectInfoCardOpen = openProjectInfoCardPath === root.path;
            const projectMenuOpen = openProjectMenuPath === root.path;
            const hasProjectKeyboardFocus = keyboardFocusProjectPath === root.path;
            const projectDisclosureButton = (
              <button
                type="button"
                className={cn(
                  "flex min-w-0 flex-1 items-center gap-2 rounded-xl px-2 text-left text-sm text-foreground/95 transition-colors focus-visible:outline-none",
                  isTouchLayout ? "min-h-10" : "min-h-[30px]",
                  isActive && "font-semibold text-foreground",
                  isTouchLayout
                    ? hasProjectStatus
                      ? "pr-24"
                      : "pr-[4.75rem]"
                    : hasProjectStatus
                      ? "pr-8"
                      : "pr-2"
                )}
                onClick={() => onProjectDisclosureToggle(root.path)}
                aria-expanded={!isCollapsed}
                aria-label={`${isCollapsed ? "Expand" : "Collapse"} ${root.displayName}${
                  inProgressSessionCount > 0
                    ? `, ${agentProjectProgressLabel(inProgressSessionCount)}`
                    : ""
                }${unreadSessionCount > 0 ? `, ${unreadSessionCount} unread` : ""}`}
              >
                {isCollapsed ? (
                  <Folder className="h-4 w-4 shrink-0" />
                ) : (
                  <FolderOpen className="h-4 w-4 shrink-0" />
                )}
                <span className="min-w-0 flex-1 truncate">{root.displayName}</span>
              </button>
            );

            return (
              <div
                key={root.path}
                ref={(node) => setProjectGroupRef(root.path, node)}
                className={cn(
                  "space-y-px",
                  rootIndex < projectRows.length - 1 && "mb-3",
                  isProjectDragging && "opacity-25"
                )}
              >
                <div
                  ref={(node) => setProjectHeaderRef(root.path, node)}
                  className={cn(
                    "group/project relative flex select-none items-center rounded-xl text-foreground transition-colors motion-reduce:transition-none hover:bg-[hsl(var(--sidebar-row-hover))]",
                    isTouchLayout ? "min-h-10" : "min-h-[30px]",
                    (projectInfoCardOpen || projectMenuOpen || hasProjectKeyboardFocus) &&
                      "bg-[hsl(var(--sidebar-row-hover))] ring-1 ring-ring/70",
                    !disabled && projectRows.length > 1 && "touch-none",
                    !disabled &&
                      (projectDrag?.path === root.path ? "cursor-grabbing" : "cursor-grab")
                  )}
                  onPointerDown={(event) => beginProjectPointer(event, root)}
                  onClickCapture={suppressActivatedProjectClick}
                  onFocusCapture={(event) =>
                    setKeyboardFocusProjectPath(
                      isKeyboardFocusTarget(event.target) ? root.path : null
                    )
                  }
                  onBlurCapture={(event) => {
                    const nextTarget = event.relatedTarget;
                    const staysKeyboardFocused =
                      event.currentTarget.contains(nextTarget as Node) &&
                      isKeyboardFocusTarget(nextTarget);
                    setKeyboardFocusProjectPath((current) =>
                      staysKeyboardFocused ? root.path : current === root.path ? null : current
                    );
                  }}
                >
                  <HoverCard
                    open={projectInfoCardOpen && !isProjectDragging && !isTouchLayout}
                    openDelay={450}
                    closeDelay={180}
                    onOpenChange={(open) =>
                      setOpenProjectInfoCardPath((current) =>
                        open ? root.path : current === root.path ? null : current
                      )
                    }
                  >
                    {isTouchLayout ? (
                      projectDisclosureButton
                    ) : (
                      <HoverCardTrigger asChild>{projectDisclosureButton}</HoverCardTrigger>
                    )}
                    {!isTouchLayout ? (
                      <HoverCardContent side="right">
                        <AgentSidebarInfoCard
                          folderPath={root.path}
                          icon={isCollapsed ? Folder : FolderOpen}
                          isInProgress={inProgressSessionCount > 0}
                          metadata={agentProjectTaskSummaryLabel(
                            projectSessions.length,
                            unreadSessionCount
                          )}
                          metadataIcon={MessageSquare}
                          onDismiss={() => setOpenProjectInfoCardPath(null)}
                          onOpenProjectFolder={() => onRevealProjectRoot(root.path)}
                          progressLabel={agentProjectProgressLabel(inProgressSessionCount)}
                          title={root.displayName}
                        />
                      </HoverCardContent>
                    ) : null}
                  </HoverCard>

                  {hasProjectStatus ? (
                    <span
                      aria-hidden="true"
                      className={cn(
                        "pointer-events-none absolute top-1/2 z-40 flex -translate-y-1/2 items-center justify-center transition-opacity duration-150 motion-reduce:transition-none",
                        isTouchLayout ? "right-[4.75rem]" : "right-2",
                        !isTouchLayout && "group-hover/project:opacity-0",
                        !isTouchLayout &&
                          (projectInfoCardOpen || projectMenuOpen || hasProjectKeyboardFocus) &&
                          "opacity-0"
                      )}
                    >
                      {showProjectRunningIndicator ? (
                        <Loader2 className="h-3.5 w-3.5 text-[hsl(var(--maple-primary))] motion-safe:animate-spin" />
                      ) : (
                        <span className="h-2 w-2 rounded-full bg-maple-success" />
                      )}
                    </span>
                  ) : null}

                  <div
                    className={projectActionRowClass(
                      isTouchLayout,
                      projectMenuOpen,
                      hasProjectKeyboardFocus || projectInfoCardOpen
                    )}
                  >
                    <div
                      aria-hidden="true"
                      className={cn(
                        "pointer-events-none w-5 shrink-0 self-stretch bg-gradient-to-r from-transparent to-[hsl(var(--sidebar))] transition-colors group-hover/project:to-[hsl(var(--sidebar-row-hover))]",
                        (projectInfoCardOpen || projectMenuOpen || hasProjectKeyboardFocus) &&
                          "to-[hsl(var(--sidebar-row-hover))]"
                      )}
                    />
                    <div
                      className={cn(
                        "flex items-center rounded-r-xl bg-[hsl(var(--sidebar))] pr-0.5 transition-colors group-hover/project:bg-[hsl(var(--sidebar-row-hover))]",
                        (projectInfoCardOpen || projectMenuOpen || hasProjectKeyboardFocus) &&
                          "bg-[hsl(var(--sidebar-row-hover))]"
                      )}
                    >
                      <DropdownMenu
                        modal={false}
                        open={projectMenuOpen}
                        onOpenChange={(open) => setOpenProjectMenuPath(open ? root.path : null)}
                      >
                        <DropdownMenuTrigger asChild>
                          <button
                            type="button"
                            className={cn(
                              AGENT_SIDEBAR_ACTION_BUTTON,
                              isTouchLayout ? "h-9 w-9" : "h-7 w-7"
                            )}
                            disabled={disabled}
                            onPointerDown={(event) => event.stopPropagation()}
                            onClick={(event) => {
                              event.preventDefault();
                              event.stopPropagation();
                            }}
                            aria-label={`Open project menu for ${root.displayName}`}
                          >
                            <MoreHorizontal className="h-4 w-4" strokeWidth={SIDEBAR_ICON_STROKE} />
                          </button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent
                          align="end"
                          collisionPadding={8}
                          className={cn(
                            "max-w-[calc(100vw-1rem)]",
                            hasRunningSession ? "w-48" : "w-max"
                          )}
                        >
                          <DropdownMenuItem
                            disabled={disabled}
                            onClick={() => onProjectRename(root)}
                          >
                            <FilePenLine
                              className="mr-2 h-4 w-4 shrink-0"
                              strokeWidth={SIDEBAR_ICON_STROKE}
                            />
                            Rename Project
                          </DropdownMenuItem>
                          <DropdownMenuItem onClick={() => onRevealProjectRoot(root.path)}>
                            <FolderOpen
                              className="mr-2 h-4 w-4 shrink-0"
                              strokeWidth={SIDEBAR_ICON_STROKE}
                            />
                            <span className="whitespace-nowrap">Open Project Folder</span>
                          </DropdownMenuItem>
                          <DropdownMenuSeparator />
                          <DropdownMenuItem
                            disabled={disabled || hasRunningSession}
                            onClick={() => onProjectRemove(root)}
                          >
                            <X
                              className="mr-2 h-4 w-4 shrink-0"
                              strokeWidth={SIDEBAR_ICON_STROKE}
                            />
                            <span className="min-w-0 whitespace-normal leading-snug">
                              {hasRunningSession
                                ? "Stop Agent Before Removing Project"
                                : "Remove Project"}
                            </span>
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                      <button
                        type="button"
                        className={cn(
                          AGENT_SIDEBAR_ACTION_BUTTON,
                          isTouchLayout ? "h-9 w-9" : "h-7 w-7"
                        )}
                        onPointerDown={(event) => event.stopPropagation()}
                        onClick={(event) => {
                          event.preventDefault();
                          event.stopPropagation();
                          onCreateSession(root.path);
                        }}
                        disabled={disabled || !root.path}
                        aria-label={`New task in ${root.displayName}`}
                      >
                        <MessageSquarePlus className="h-4 w-4" />
                      </button>
                    </div>
                  </div>
                </div>

                {!isCollapsed && projectSessions.length === 0 && isActive ? (
                  <p
                    className={cn(
                      "flex items-center pl-8 text-xs text-muted-foreground/75",
                      isTouchLayout ? "min-h-10" : "min-h-[30px]"
                    )}
                  >
                    No tasks yet
                  </p>
                ) : null}

                {!isCollapsed
                  ? projectSessions.map((session) => {
                      const isActiveSession = session.id === activeSessionId;
                      const isInProgress = inProgressSessionIds.has(session.id);
                      const isRunning = runningSessionIds.has(session.id);
                      const isUnreadCompleted = completedUnreadSessionIds.has(session.id);

                      return (
                        <AgentSidebarTaskRow
                          key={session.id}
                          actionsLocked={disabled}
                          isActive={isActiveSession}
                          isInProgress={isInProgress}
                          isRunning={isRunning}
                          isTouchLayout={isTouchLayout}
                          isUnreadCompleted={isUnreadCompleted}
                          onDelete={onSessionDelete}
                          onRevealProjectRoot={onRevealProjectRoot}
                          onSelect={onSessionSelect}
                          projectDisplayName={root.displayName}
                          rowRef={(node) => setAnimatedRowRef(`session:${session.id}`, node)}
                          session={session}
                        />
                      );
                    })
                  : null}
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
          {projectDrag.isCollapsed ? (
            <Folder className="h-4 w-4 shrink-0" />
          ) : (
            <FolderOpen className="h-4 w-4 shrink-0" />
          )}
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
  recentRoots: AgentProjectRootView[];
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
          onToggleExpanded && !isExpanded && "landscape-short:min-h-[52px]",
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
                  {root.displayName}
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
              className={cn(
                "flex h-8 w-8 items-center justify-center rounded-full bg-gradient-to-b from-[hsl(var(--maple-primary))] to-[hsl(var(--maple-primary-strong))] text-[hsl(var(--maple-on-primary))]/90 transition-all duration-200 ease-out active:scale-[0.95] disabled:pointer-events-none disabled:opacity-40",
                onToggleExpanded && !isExpanded && "sm:h-9 sm:w-9"
              )}
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

const MemoizedAgentSidebarContent = memo(AgentSidebarContent);
const MemoizedAgentComposer = memo(AgentComposer);

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
  const { availableModels, modelAliases } = useModelState();
  const { billingStatus } = useBillingState();
  const [upgradeDialogOpen, setUpgradeDialogOpen] = useState(false);
  const [selectedModelName, setSelectedModelName] = useState("");
  const [showAdvanced, setShowAdvanced] = useState(false);

  const modelById = useMemo(() => {
    return new Map(availableModels.map((availableModel) => [availableModel.id, availableModel]));
  }, [availableModels]);

  const aliasById = useMemo(() => {
    return new Map(modelAliases.map((alias) => [alias.id, alias]));
  }, [modelAliases]);

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
      return planName.includes("pro") || planName.includes("max") || planName.includes("team");
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
    const selectedModel = modelById.get(modelId);
    const badges = selectedModel?.badges || [];
    return badges.filter(
      (badge) =>
        badge !== "Pro" &&
        (selectedModel?.access === "free" || badge.toLowerCase() !== selectedModel?.access)
    );
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
  onPermissionDecision
}: {
  items: AgentTimelineItem[];
  isResponsePending: boolean;
  isRunActive: boolean;
  generatedThoughtLabels: Record<string, Record<string, string>>;
  sessionId: string | null;
  onPermissionDecision: (item: AgentTimelineItem, decision: AgentPermissionDecision) => void;
}) {
  const visibleItems = coalesceAdjacentThinkingItems(items).filter(isRenderableAgentTimelineItem);
  const turns = groupAgentTimelineItems(visibleItems);
  const activeThinkingItemId = activeAgentThinkingItemId(visibleItems, isRunActive);
  const showAssistantLoader = shouldShowAgentAssistantLoader(turns, isResponsePending);
  const trailingTurn = turns[turns.length - 1];
  const pendingIndicatorTurnId =
    showAssistantLoader && trailingTurn?.type === "assistant" ? trailingTurn.id : null;

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
          const thoughtLabel =
            phaseId && sessionId ? generatedThoughtLabels[sessionId]?.[phaseId] : null;
          return { item, thoughtLabel };
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
            {assistantItems.map(({ item, thoughtLabel }) => (
              <AgentAssistantItem
                key={item.id}
                item={item}
                isThinking={item.id === activeThinkingItemId}
                thoughtLabel={thoughtLabel ?? undefined}
                onPermissionDecision={onPermissionDecision}
              />
            ))}
            {pendingIndicatorTurnId === turn.id ? <ChatAssistantPendingIndicator /> : null}
          </ChatAssistantTurn>
        );
      })}
      {showAssistantLoader && pendingIndicatorTurnId === null ? <ChatAssistantPendingTurn /> : null}
    </div>
  );
}

function AgentAssistantItem({
  item,
  isThinking,
  thoughtLabel,
  onPermissionDecision
}: {
  item: AgentTimelineItem;
  isThinking: boolean;
  thoughtLabel?: string;
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
        label={thoughtLabel}
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
  const toolKind = agentToolKind(item.id, item.title);
  const ToolKindIcon = AGENT_TOOL_KIND_ICONS[toolKind];
  const toolKindLabel = agentToolKindLabel(toolKind);
  const hasDetails =
    Boolean(item.text?.trim()) || item.input !== undefined || item.output !== undefined;
  const statusIcon = active ? (
    <Loader2
      aria-hidden="true"
      className="h-3.5 w-3.5 shrink-0 animate-spin text-muted-foreground"
    />
  ) : failed ? (
    <X aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-destructive" />
  ) : (
    <Check aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-maple-success" />
  );

  const summary = (
    <div className="flex min-w-0 flex-1 items-center gap-1.5">
      <span
        role="img"
        aria-label={toolKindLabel}
        title={toolKindLabel}
        className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md bg-background/70 text-muted-foreground"
      >
        <ToolKindIcon aria-hidden="true" className="h-3.5 w-3.5" />
      </span>
      <span
        className="min-w-0 flex-1 truncate text-[13px] font-medium leading-5"
        title={toolTitle(item)}
      >
        {toolTitle(item)}
      </span>
      <span
        className={cn(
          "shrink-0 text-[11px] leading-5 text-muted-foreground",
          failed && "text-destructive"
        )}
      >
        {formatStatus(status)}
      </span>
      {statusIcon}
    </div>
  );

  if (!hasDetails) {
    return (
      <div
        className={cn(
          "flex min-h-8 items-center rounded-xl bg-muted/30 px-2 py-1 text-sm",
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
        "group rounded-xl border border-muted/40 bg-muted/20 px-2 py-1 text-sm",
        failed && "border-destructive/35 bg-destructive/5"
      )}
    >
      <summary className="flex min-h-6 cursor-pointer list-none items-center gap-1">
        <ChevronRight
          aria-hidden="true"
          className="h-3.5 w-3.5 shrink-0 text-muted-foreground transition-transform group-open:rotate-90"
        />
        {summary}
      </summary>
      <div className="mt-1.5 space-y-2 border-t border-muted/40 pb-1 pl-7 pr-1 pt-2">
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

const AGENT_TOOL_KIND_ICONS: Record<AgentToolKind, LucideIcon> = {
  shell: SquareTerminal,
  "file-read": FileSearch,
  "file-write": FilePenLine,
  web: Globe2,
  mcp: Blocks,
  generic: Wrench
};

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
