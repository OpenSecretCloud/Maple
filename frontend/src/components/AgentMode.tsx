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
  continueChatComposerList,
  continueChatComposerListBeforeInput
} from "@/components/chatComposerListContinuation";
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
import { RenameAgentTaskDialog } from "@/components/RenameAgentTaskDialog";
import { UpgradePromptDialog } from "@/components/UpgradePromptDialog";
import { AgentMcpMenu, AgentMcpServersDialog } from "@/components/agent/AgentMcpControls";
import { AgentSidebarInfoCard } from "@/components/agent/AgentSidebarInfoCard";
import {
  isDeliberateAgentComposerFocusTarget,
  settleAgentComposerFocusRequest,
  type AgentComposerFocusRequest
} from "@/components/agent/agentComposerFocus";
import { handleAgentModeThoughtRunFinished } from "@/components/agent/agentModeThoughtRun";
import {
  agentPromptHistory,
  agentPromptHistoryDirection,
  AgentPromptHistoryReplacementTracker,
  navigateAgentPromptHistory,
  type AgentPromptHistoryNavigation,
  type AgentPromptHistoryReplacementAttempt
} from "@/components/agent/agentPromptHistory";
import {
  DEFAULT_AGENT_PAGE_SIZE,
  agentRuntimeService as defaultAgentRuntimeService,
  awaitAgentAuthUser,
  type AgentConfig,
  type AgentEventEnvelope,
  type AgentLiveChannelFrame,
  type AgentLiveEventCursor,
  type AgentPendingHistoryAttach,
  type AgentMcpServer,
  type AgentPermissionDecision,
  isAgentPageStaleError,
  isAgentLiveSnapshotRequiredError,
  type AgentProjectSkillsTrustStatus,
  type AgentDesktopQueueSnapshot,
  type AgentRuntimeStatus,
  type AgentRuntimeService,
  type AgentSessionMcpServer,
  type AgentSessionSummary,
  type AgentTimelineItem,
  type RecentProjectRoot
} from "@/services/agentRuntimeService";
import { AgentHistoryPaginationCache } from "@/services/agentHistoryPagination";
import {
  AgentLiveConnectionRegistry,
  recoverAgentLiveConnectionAfterReplacementFailure
} from "@/services/agentLiveConnectionLifecycle";
import { AgentSessionPaginationCache } from "@/services/agentSessionPagination";
import {
  CHAT_HISTORY_TOP_MARGIN_PX,
  ChatHistoryPaginationGate,
  type ChatHistoryScrollSnapshot,
  preferredChatHistoryScrollSnapshot,
  requiredChatHistoryBottomCompensation,
  restoredChatHistoryAnchorScrollTop,
  restoredChatHistoryScrollTop,
  usesFirstCancelableWheelGestureStart
} from "@/components/chatHistoryPagination";
import {
  applyAgentDesktopQueueSnapshot,
  beginQueuedMessageEdit,
  discardQueuedMessageEdit,
  emptyAgentDesktopQueueSnapshot,
  queuedMessageEditStillPresent,
  queueSnapshotWithoutItem,
  shouldPrepareThoughtAfterAgentSend,
  type AgentQueuedMessageEdit
} from "@/services/agentComposerQueue";
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
import {
  agentComposerCanSend,
  agentComposerShowsStop,
  canSubmitAgentComposerMessage,
  isAgentComposerSendLocked,
  planAgentComposerStop,
  shouldClearStoppingSendLock
} from "@/services/agentComposerSend";
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
  AgentAssistantTurnKeyRegistry,
  AgentLiveThoughtPhaseTracker,
  activeAgentThinkingItemId,
  agentTimelineHistoryAnchorIds,
  agentThinkingPhaseId,
  agentUserTurnReactKey,
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
import { ResizableSidebarLayout } from "@/components/ResizableSidebarLayout";
import {
  cn,
  POWERFUL_MODEL_ALIAS,
  QUICK_MODEL_ALIAS,
  useIsCoarsePointer,
  useIsLandscapeMobile,
  useIsMobile
} from "@/utils/utils";
import { isMacOS, isTauri, isTauriDesktop } from "@/utils/platform";
import { useLazyRef } from "@/utils/useLazyRef";
import {
  canUseLocalAgentProjectFolderActions,
  revealAgentProjectFolder
} from "@/services/agentProjectFolder";
import {
  aggregateAgentSidebarStatus,
  agentProjectProgressLabel,
  agentProjectTaskSummaryLabel,
  agentTaskAccessibleLabel,
  agentTaskRowInteractionPresentation,
  type AgentTaskRowKeyboardFocusTarget
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
  "relative z-10 flex shrink-0 items-center justify-center rounded-full border-0 bg-transparent text-muted-foreground transition-colors hover:bg-foreground/5 hover:text-foreground focus-visible:bg-foreground/5 focus-visible:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring/70 data-[silent-focus=true]:focus-visible:bg-transparent data-[silent-focus=true]:focus-visible:text-muted-foreground data-[silent-focus=true]:focus-visible:ring-0";

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

export function AgentMode({
  userId,
  agentRuntimeService = defaultAgentRuntimeService
}: {
  userId: string;
  agentRuntimeService?: AgentRuntimeService;
}) {
  const openai = useOpenAI();
  const os = useOpenSecret();
  const agentOwnerKey = JSON.stringify([userId, String(agentRuntimeService.target.id)]);
  const localProjectFolderActionsAvailable = canUseLocalAgentProjectFolderActions(
    agentRuntimeService.target,
    isTauriDesktop()
  );
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
  const [isSidebarTransitioning, setIsSidebarTransitioning] = useState(false);
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
  const sessionPaginationCacheRef = useLazyRef(() => new AgentSessionPaginationCache());
  const [hasMoreSessions, setHasMoreSessions] = useState(false);
  const [isLoadingOlderSessions, setIsLoadingOlderSessions] = useState(false);
  const [isSessionHistoryReady, setIsSessionHistoryReady] = useState(false);
  const [sessionToDelete, setSessionToDelete] = useState<AgentSessionSummary | null>(null);
  const [sessionToRename, setSessionToRename] = useState<AgentSessionSummary | null>(null);
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
  const historyPaginationCacheRef = useLazyRef(
    () =>
      new AgentHistoryPaginationCache({
        accountId: userId,
        targetId: String(agentRuntimeService.target.id)
      })
  );
  const [hasMoreOlderHistory, setHasMoreOlderHistory] = useState(false);
  const [isLoadingOlderHistory, setIsLoadingOlderHistory] = useState(false);
  const [queueBySession, setQueueBySession] = useState<Record<string, AgentDesktopQueueSnapshot>>(
    {}
  );
  const queueBySessionRef = useRef(queueBySession);
  queueBySessionRef.current = queueBySession;
  const [queueEdit, setQueueEdit] = useState<AgentQueuedMessageEdit | null>(null);
  const queueEditRef = useRef(queueEdit);
  queueEditRef.current = queueEdit;
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
  const promptHistoryEntries = useMemo(() => agentPromptHistory(timelineItems), [timelineItems]);
  const promptHistoryEntriesRef = useRef<readonly string[]>(promptHistoryEntries);
  const promptHistoryNavigationRef = useRef<AgentPromptHistoryNavigation | null>(null);
  const promptHistoryGenerationRef = useRef(0);
  const promptHistoryReplacementTrackerRef = useLazyRef(
    () => new AgentPromptHistoryReplacementTracker()
  );
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
  const [stoppingSessionIds, setStoppingSessionIds] = useState<Set<string>>(() => new Set());
  const [pendingSessionSelectionId, setPendingSessionSelectionId] = useState<string | null>(null);
  const [activeRunsBySession, setActiveRunsBySession] = useState<Record<string, string>>({});
  const [completedUnreadSessionIds, setCompletedUnreadSessionIds] = useState<Set<string>>(
    () => new Set()
  );
  const chatContainerRef = useRef<HTMLDivElement>(null);
  const historyTopSentinelRef = useRef<HTMLDivElement>(null);
  const historyBottomCompensationRef = useRef<HTMLDivElement>(null);
  const pendingHistoryScrollRestoreRef = useRef<ChatHistoryScrollSnapshot | null>(null);
  const pendingHistoryScrollRestoreSessionIdRef = useRef<string | null>(null);
  const historyGestureEndTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const historyTouchGestureEndTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const historyKeyIntentTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const historyWheelGestureStartPendingRef = useRef(false);
  const historyPreviousWheelCancelableRef = useRef<boolean | null>(null);
  const previousHistoryTouchYRef = useRef<number | null>(null);
  const historyTouchGestureActiveRef = useRef(false);
  const historyPointerGestureActiveRef = useRef(false);
  const previousHistoryPointerScrollTopRef = useRef(0);
  const suppressedHistoryScrollEndsRef = useRef(0);
  const eventGapRecoveryRef = useRef<Promise<void> | null>(null);
  const pendingEventGapSessionIdsRef = useLazyRef(() => new Set<string>());
  const hasUnknownEventGapRef = useRef(false);
  const liveConnectionsRef = useLazyRef(() => new AgentLiveConnectionRegistry());
  const liveRetirementRef = useRef<Promise<void> | null>(null);
  const liveResumeInFlightRef = useRef<Promise<void> | null>(null);
  const liveStreamGenerationRef = useRef(0);
  const liveChannelHandlerRef = useRef<(frame: AgentLiveChannelFrame) => void>(() => {});
  const agentOwnerKeyRef = useRef(agentOwnerKey);
  agentOwnerKeyRef.current = agentOwnerKey;
  const agentComposerTextareaRef = useRef<HTMLTextAreaElement>(null);
  const agentComposerFocusRequestRef = useRef<AgentComposerFocusRequest | null>(null);
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
  const renameTaskMenuTriggerRef = useRef<HTMLButtonElement | null>(null);
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
  const historyPaginationLifecycle = useMemo(
    () => ({ sessionId: activeSessionId, gate: new ChatHistoryPaginationGate() }),
    [activeSessionId]
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

  useLayoutEffect(() => {
    if (
      activeSessionId === null ||
      !promptHistoryReplacementTrackerRef.current.isReplacing(activeSessionId)
    ) {
      promptHistoryEntriesRef.current = promptHistoryEntries;
    }
  }, [activeSessionId, promptHistoryEntries, promptHistoryReplacementTrackerRef]);

  const resetPromptHistoryNavigation = useCallback(() => {
    promptHistoryNavigationRef.current = null;
    promptHistoryGenerationRef.current += 1;
  }, []);

  useLayoutEffect(() => {
    promptHistoryReplacementTrackerRef.current.abandonInactive(activeSessionId);
    resetPromptHistoryNavigation();
  }, [activeSessionId, promptHistoryReplacementTrackerRef, resetPromptHistoryNavigation, userId]);

  const handleAgentInputChange = useCallback(
    (value: string) => {
      resetPromptHistoryNavigation();
      setInput(value);
    },
    [resetPromptHistoryNavigation]
  );

  useEffect(() => {
    const cancelFocusRequestForPointer = (event: PointerEvent) => {
      if (
        agentComposerFocusRequestRef.current &&
        event.target !== agentComposerTextareaRef.current
      ) {
        agentComposerFocusRequestRef.current = null;
      }
    };
    const cancelFocusRequestForWindowBlur = () => {
      agentComposerFocusRequestRef.current = null;
    };
    const cancelFocusRequestForFocus = (event: FocusEvent) => {
      if (
        agentComposerFocusRequestRef.current &&
        isDeliberateAgentComposerFocusTarget(
          event.target,
          agentComposerTextareaRef.current,
          document.body,
          document
        )
      ) {
        agentComposerFocusRequestRef.current = null;
      }
    };
    window.addEventListener("pointerdown", cancelFocusRequestForPointer, true);
    window.addEventListener("focusin", cancelFocusRequestForFocus, true);
    window.addEventListener("blur", cancelFocusRequestForWindowBlur);
    return () => {
      window.removeEventListener("pointerdown", cancelFocusRequestForPointer, true);
      window.removeEventListener("focusin", cancelFocusRequestForFocus, true);
      window.removeEventListener("blur", cancelFocusRequestForWindowBlur);
    };
  }, []);

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

  const publishHistorySnapshot = useCallback(
    (sessionId: string) => {
      if (activeSessionIdRef.current !== sessionId) return;
      const snapshot = historyPaginationCacheRef.current.snapshot(sessionId);
      setTimelineItems([...snapshot.timeline]);
      setHasMoreOlderHistory(snapshot.hasMore);
      setIsLoadingOlderHistory(snapshot.isLoading);
    },
    [historyPaginationCacheRef]
  );

  const reconcileHistoryRetention = useCallback(() => {
    const protectedSessionIds = new Set<string>();
    if (activeSessionIdRef.current) protectedSessionIds.add(activeSessionIdRef.current);
    const pendingSelection = pendingSessionSelectionIdRef.current;
    if (pendingSelection && pendingSelection !== NEW_SESSION_PENDING_KEY) {
      protectedSessionIds.add(pendingSelection);
    }
    historyPaginationCacheRef.current.reconcileRetention(protectedSessionIds);
  }, [historyPaginationCacheRef]);

  const retireAgentLiveConnection = useCallback(async () => {
    liveStreamGenerationRef.current += 1;
    const previousRetirement = liveRetirementRef.current;
    // With no predecessor, invoke retirement synchronously so every service
    // handle publishes close intent before a same-commit remount can open. A
    // later caller still waits for and then retries handles retained by an
    // earlier failed cancellation.
    const retirement = previousRetirement
      ? (async () => {
          await previousRetirement.catch(() => {});
          await liveConnectionsRef.current.retire();
        })()
      : liveConnectionsRef.current.retire();
    liveRetirementRef.current = retirement;
    try {
      await retirement;
    } finally {
      if (liveRetirementRef.current === retirement) liveRetirementRef.current = null;
    }
  }, [liveConnectionsRef]);

  const resumeAgentLiveConnection = useCallback(
    async (retainedCursor?: AgentLiveEventCursor) => {
      if (agentRuntimeService.target.kind !== "remote") return;
      const existingResume = liveResumeInFlightRef.current;
      if (existingResume) {
        if (!retainedCursor) return await existingResume;
        // A replacement attach may have fenced this older resume while it was
        // opening. Wait for its owned cleanup, then make a fresh attempt from
        // the replacement's retained cursor instead of treating it as recovery.
        await existingResume.catch(() => {});
        if (liveResumeInFlightRef.current === existingResume) {
          liveResumeInFlightRef.current = null;
        }
      }
      const cursor = retainedCursor ?? historyPaginationCacheRef.current.eventCursor();
      if (!cursor) throw new Error("Agent live resume requires an event cursor");

      const resume = (async () => {
        // Cursor replay closes the short retirement gap without fetching any
        // history page. Snapshot-required failures are recovered separately by
        // the existing bounded head coordinator.
        await retireAgentLiveConnection();
        const generation = ++liveStreamGenerationRef.current;
        const resumeOwnerKey = agentOwnerKey;
        const active = await agentRuntimeService.resumeLiveEvents(userId, cursor, (frame) => {
          if (
            generation === liveStreamGenerationRef.current &&
            resumeOwnerKey === agentOwnerKeyRef.current &&
            isAgentModeMountedRef.current
          ) {
            liveChannelHandlerRef.current(frame);
          }
        });
        if (
          generation !== liveStreamGenerationRef.current ||
          resumeOwnerKey !== agentOwnerKeyRef.current ||
          !isAgentModeMountedRef.current ||
          userIdRef.current !== userId
        ) {
          await liveConnectionsRef.current.cancelActive(active);
          return;
        }
        liveConnectionsRef.current.trackActive(active);
      })();
      liveResumeInFlightRef.current = resume;
      try {
        await resume;
      } finally {
        if (liveResumeInFlightRef.current === resume) liveResumeInFlightRef.current = null;
      }
    },
    [
      agentOwnerKey,
      agentRuntimeService,
      historyPaginationCacheRef,
      liveConnectionsRef,
      retireAgentLiveConnection,
      userId
    ]
  );

  const loadHistoryHead = useCallback(
    async (sessionId: string): Promise<AgentTimelineItem[]> => {
      if (agentRuntimeService.target.kind === "remote") {
        await retireAgentLiveConnection();
        const attachGeneration = ++liveStreamGenerationRef.current;
        const attachOwnerKey = agentOwnerKey;
        const token = historyPaginationCacheRef.current.beginHead(sessionId);
        if (activeSessionIdRef.current === sessionId) setIsLoadingOlderHistory(true);
        let pending: AgentPendingHistoryAttach | null = null;
        try {
          pending = await agentRuntimeService.beginSessionHistoryAttach(
            userId,
            { sessionId, limit: DEFAULT_AGENT_PAGE_SIZE },
            (frame) => {
              if (
                attachGeneration === liveStreamGenerationRef.current &&
                attachOwnerKey === agentOwnerKeyRef.current &&
                isAgentModeMountedRef.current
              ) {
                liveChannelHandlerRef.current(frame);
              }
            }
          );
          liveConnectionsRef.current.trackPending(pending);
          if (
            attachGeneration !== liveStreamGenerationRef.current ||
            !isAgentModeMountedRef.current ||
            userIdRef.current !== userId
          ) {
            historyPaginationCacheRef.current.fail(token);
            publishHistorySnapshot(sessionId);
            await liveConnectionsRef.current.cancelPending(pending);
            return [...historyPaginationCacheRef.current.snapshot(sessionId).timeline];
          }
          const response = pending.response;
          const result = historyPaginationCacheRef.current.installSynchronizedAccountHead(
            token,
            response.page,
            {
              liveSessionsComplete: response.liveSessionsComplete,
              liveSessionCount: response.liveSessionCount,
              liveSessions: response.liveSessions,
              throughEventCursor: response.throughEventCursor
            }
          );
          if (result !== "applied") {
            historyPaginationCacheRef.current.fail(token);
            publishHistorySnapshot(sessionId);
            await liveConnectionsRef.current.cancelPending(pending);
            return [...historyPaginationCacheRef.current.snapshot(sessionId).timeline];
          }
          publishHistorySnapshot(sessionId);
          const active = await pending.activate();
          if (
            attachGeneration !== liveStreamGenerationRef.current ||
            !isAgentModeMountedRef.current ||
            userIdRef.current !== userId
          ) {
            historyPaginationCacheRef.current.fail(token);
            publishHistorySnapshot(sessionId);
            await liveConnectionsRef.current.cancelPending(pending);
            return [...historyPaginationCacheRef.current.snapshot(sessionId).timeline];
          }
          liveConnectionsRef.current.promote(pending, active);
          return [...historyPaginationCacheRef.current.snapshot(sessionId).timeline];
        } catch (loadError) {
          historyPaginationCacheRef.current.fail(token);
          if (pending) liveConnectionsRef.current.trackPending(pending);
          publishHistorySnapshot(sessionId);
          const resumeCursor = historyPaginationCacheRef.current.eventCursor();
          const canResume =
            resumeCursor !== null &&
            isAgentModeMountedRef.current &&
            userIdRef.current === userId &&
            agentOwnerKeyRef.current === agentOwnerKey;
          return await recoverAgentLiveConnectionAfterReplacementFailure({
            replacementError: loadError,
            cursor: canResume ? resumeCursor : null,
            retire: retireAgentLiveConnection,
            resume: async (retainedCursor) => {
              try {
                await resumeAgentLiveConnection(retainedCursor);
              } catch (resumeError) {
                historyPaginationCacheRef.current.requireSynchronizedReload();
                throw resumeError;
              }
            }
          });
        } finally {
          reconcileHistoryRetention();
        }
      }

      const token = historyPaginationCacheRef.current.beginHead(sessionId);
      if (activeSessionIdRef.current === sessionId) setIsLoadingOlderHistory(true);
      try {
        const page = await agentRuntimeService.listSessionRecordsPage(userId, {
          sessionId,
          limit: DEFAULT_AGENT_PAGE_SIZE
        });
        // Ordinary head loads commit persisted records only. Installing an
        // absolute live snapshot/checkpoint requires the attach coordinator's
        // subscribe-buffer-replay ordering and must not be inferred from a page.
        const result = historyPaginationCacheRef.current.commit(token, page);
        if (result === "stale") {
          return [...historyPaginationCacheRef.current.snapshot(sessionId).timeline];
        }
        publishHistorySnapshot(sessionId);
        return [...historyPaginationCacheRef.current.snapshot(sessionId).timeline];
      } catch (loadError) {
        historyPaginationCacheRef.current.fail(token);
        publishHistorySnapshot(sessionId);
        throw loadError;
      } finally {
        reconcileHistoryRetention();
      }
    },
    [
      agentRuntimeService,
      agentOwnerKey,
      historyPaginationCacheRef,
      liveConnectionsRef,
      publishHistorySnapshot,
      reconcileHistoryRetention,
      retireAgentLiveConnection,
      resumeAgentLiveConnection,
      userId
    ]
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
            const timeline = await loadHistoryHead(sessionId);
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
              thoughtPhaseTrackerRef.current.seedActiveTimeline(sessionId, timeline);
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
      loadHistoryHead,
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
      void retireAgentLiveConnection().catch(() => {
        console.error("Unable to retire the Agent live connection during unmount");
      });
      cancelThoughtLabelDisplays();
    };
  }, [cancelThoughtLabelDisplays, retireAgentLiveConnection]);

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

  const clearHistoryBottomCompensation = useCallback(() => {
    if (historyBottomCompensationRef.current) {
      historyBottomCompensationRef.current.style.height = "0px";
    }
  }, []);

  useLayoutEffect(() => {
    const ownerBinding = historyPaginationCacheRef.current.bindOwner({
      accountId: userId,
      targetId: String(agentRuntimeService.target.id)
    });
    if (ownerBinding === "reset") {
      void retireAgentLiveConnection().catch((retirementError) => {
        setError(errorMessage(retirementError));
      });
      sessionPaginationCacheRef.current.clear();
      sessionSelectionGenerationRef.current += 1;
      interactionGenerationRef.current += 1;
      pendingSessionSelectionIdRef.current = null;
      deletedSessionIdsRef.current.clear();
      activeSessionIdRef.current = null;
      setActiveSessionId(null);
      setSessions([]);
      setTimelineItems([]);
      setHasMoreSessions(false);
      setHasMoreOlderHistory(false);
      setIsSessionHistoryReady(false);
    }
    clearHistoryBottomCompensation();
    pendingHistoryScrollRestoreRef.current = null;
    pendingHistoryScrollRestoreSessionIdRef.current = null;
    previousHistoryTouchYRef.current = null;
    historyTouchGestureActiveRef.current = false;
    historyPointerGestureActiveRef.current = false;
    suppressedHistoryScrollEndsRef.current = 0;
    historyPaginationLifecycle.gate.resetIntent();
  }, [
    agentOwnerKey,
    agentRuntimeService.target.id,
    clearHistoryBottomCompensation,
    deletedSessionIdsRef,
    historyPaginationCacheRef,
    historyPaginationLifecycle,
    retireAgentLiveConnection,
    sessionPaginationCacheRef,
    userId
  ]);

  useEffect(() => {
    const protectedSessionIds = new Set<string>();
    if (activeSessionId) protectedSessionIds.add(activeSessionId);
    if (pendingSessionSelectionId && pendingSessionSelectionId !== NEW_SESSION_PENDING_KEY) {
      protectedSessionIds.add(pendingSessionSelectionId);
    }
    historyPaginationCacheRef.current.reconcileRetention(protectedSessionIds);
  }, [activeSessionId, historyPaginationCacheRef, pendingSessionSelectionId]);

  const captureHistoryScrollSnapshot = useCallback(
    (sessionId: string): ChatHistoryScrollSnapshot | null => {
      if (activeSessionIdRef.current !== sessionId) return null;
      const container = chatContainerRef.current;
      if (!container) return null;
      const containerRect = container.getBoundingClientRect();
      const anchor = Array.from(
        container.querySelectorAll<HTMLElement>("[data-history-anchor-ids]")
      ).find((candidate) => {
        const rect = candidate.getBoundingClientRect();
        return rect.bottom > containerRect.top && rect.top < containerRect.bottom;
      });
      return {
        scrollTop: container.scrollTop,
        scrollHeight: container.scrollHeight,
        anchorId: anchor?.dataset.historyAnchorIds?.split(" ").find(Boolean),
        anchorOffset: anchor ? anchor.getBoundingClientRect().top - containerRect.top : undefined
      };
    },
    []
  );

  const isHistoryTopBoundaryNear = useCallback(() => {
    const container = chatContainerRef.current;
    const sentinel = historyTopSentinelRef.current;
    if (!container || !sentinel) return false;
    if (container.scrollHeight <= container.clientHeight + 1) return true;
    const containerRect = container.getBoundingClientRect();
    const sentinelRect = sentinel.getBoundingClientRect();
    return (
      sentinelRect.bottom >= containerRect.top - CHAT_HISTORY_TOP_MARGIN_PX &&
      sentinelRect.top <= containerRect.top + CHAT_HISTORY_TOP_MARGIN_PX
    );
  }, []);

  const loadOlderHistory = useCallback(async () => {
    const { gate, sessionId } = historyPaginationLifecycle;
    if (!sessionId) {
      gate.finishLoad();
      return;
    }
    const token = historyPaginationCacheRef.current.beginOlder(sessionId);
    if (!token || !token.cursor) {
      gate.finishLoad();
      return;
    }

    const requestStartSnapshot = captureHistoryScrollSnapshot(sessionId);
    publishHistorySnapshot(sessionId);
    let pageProgressed = false;
    try {
      const page = await agentRuntimeService.listSessionRecordsPage(userId, {
        sessionId,
        cursor: token.cursor,
        limit: DEFAULT_AGENT_PAGE_SIZE
      });
      const commitSnapshot = captureHistoryScrollSnapshot(sessionId);
      const result = historyPaginationCacheRef.current.commit(token, page);
      if (result === "history-replaced") {
        await loadHistoryHead(sessionId);
      } else if (result === "applied") {
        pageProgressed = page.records.length > 0;
        if (pageProgressed && activeSessionIdRef.current === sessionId) {
          pendingHistoryScrollRestoreRef.current = preferredChatHistoryScrollSnapshot({
            requestStartSnapshot,
            commitSnapshot
          });
          pendingHistoryScrollRestoreSessionIdRef.current = sessionId;
        }
      }
    } catch (loadError) {
      if (isAgentPageStaleError(loadError)) {
        historyPaginationCacheRef.current.invalidate(sessionId);
        try {
          await loadHistoryHead(sessionId);
        } catch (headError) {
          if (activeSessionIdRef.current === sessionId) setError(errorMessage(headError));
        }
      } else {
        historyPaginationCacheRef.current.fail(token);
        if (activeSessionIdRef.current === sessionId) setError(errorMessage(loadError));
      }
    } finally {
      gate.finishLoad({ preserveQueuedLoad: pageProgressed });
      publishHistorySnapshot(sessionId);
      reconcileHistoryRetention();
    }
  }, [
    agentRuntimeService,
    captureHistoryScrollSnapshot,
    historyPaginationCacheRef,
    historyPaginationLifecycle,
    loadHistoryHead,
    publishHistorySnapshot,
    reconcileHistoryRetention,
    userId
  ]);

  const maybeLoadOlderHistory = useCallback(() => {
    const { gate, sessionId } = historyPaginationLifecycle;
    const shouldLoad = gate.tryStartLoad({
      canLoad: Boolean(sessionId && hasMoreOlderHistory),
      topBoundaryVisible: isHistoryTopBoundaryNear(),
      requestInFlight: isLoadingOlderHistory
    });
    if (shouldLoad) void loadOlderHistory();
  }, [
    hasMoreOlderHistory,
    historyPaginationLifecycle,
    isHistoryTopBoundaryNear,
    isLoadingOlderHistory,
    loadOlderHistory
  ]);

  useEffect(() => {
    const container = chatContainerRef.current;
    const sessionId = historyPaginationLifecycle.sessionId;
    if (!container || !sessionId) return;
    const gate = historyPaginationLifecycle.gate;
    const usesMacOSWheelGestureStart = usesFirstCancelableWheelGestureStart({
      isTauriEnvironment: isTauri(),
      browserPlatform: navigator.platform
    });
    const delaysWheelGestureEndAfterScrollEnd = isTauri() && isMacOS();

    const finishWheelGesture = () => {
      historyWheelGestureStartPendingRef.current = false;
      historyPreviousWheelCancelableRef.current = null;
      gate.endGesture();
      historyGestureEndTimeoutRef.current = null;
    };
    const finishTouchGesture = () => {
      historyTouchGestureActiveRef.current = false;
      gate.endGesture();
      historyTouchGestureEndTimeoutRef.current = null;
    };
    const scheduleTouchGestureEnd = () => {
      if (historyTouchGestureEndTimeoutRef.current) {
        clearTimeout(historyTouchGestureEndTimeoutRef.current);
      }
      historyTouchGestureEndTimeoutRef.current = setTimeout(finishTouchGesture, 250);
    };
    const handleWheel = (event: WheelEvent) => {
      if (event.ctrlKey) return;

      const startsMacOSWheelGesture =
        usesMacOSWheelGestureStart &&
        event.cancelable &&
        historyPreviousWheelCancelableRef.current !== true;
      if (usesMacOSWheelGestureStart) {
        historyPreviousWheelCancelableRef.current = event.cancelable;
      }

      if (usesMacOSWheelGestureStart && event.deltaY === 0) {
        if (startsMacOSWheelGesture) historyWheelGestureStartPendingRef.current = true;
        if (historyGestureEndTimeoutRef.current) {
          clearTimeout(historyGestureEndTimeoutRef.current);
        }
        historyGestureEndTimeoutRef.current = setTimeout(finishWheelGesture, 180);
        return;
      }

      if (event.deltaY >= 0) {
        if (event.deltaY > 0) {
          historyWheelGestureStartPendingRef.current = false;
          historyPreviousWheelCancelableRef.current = null;
          gate.endGesture();
          clearHistoryBottomCompensation();
          if (historyGestureEndTimeoutRef.current) {
            clearTimeout(historyGestureEndTimeoutRef.current);
            historyGestureEndTimeoutRef.current = null;
          }
        }
        return;
      }

      if (usesMacOSWheelGestureStart) {
        const isNewWheelGesture =
          startsMacOSWheelGesture || historyWheelGestureStartPendingRef.current;
        historyWheelGestureStartPendingRef.current = false;
        gate.beginWheelGesture(isNewWheelGesture);
      } else {
        gate.beginGesture();
      }
      if (container.scrollTop <= CHAT_HISTORY_TOP_MARGIN_PX) maybeLoadOlderHistory();
      if (historyGestureEndTimeoutRef.current) {
        clearTimeout(historyGestureEndTimeoutRef.current);
      }
      historyGestureEndTimeoutRef.current = setTimeout(finishWheelGesture, 180);
    };
    const handleTouchStart = (event: TouchEvent) => {
      if (historyTouchGestureEndTimeoutRef.current) {
        clearTimeout(historyTouchGestureEndTimeoutRef.current);
        historyTouchGestureEndTimeoutRef.current = null;
      }
      historyTouchGestureActiveRef.current = true;
      previousHistoryTouchYRef.current = event.touches[0]?.clientY ?? null;
      gate.endGesture();
    };
    const handleTouchMove = (event: TouchEvent) => {
      const nextY = event.touches[0]?.clientY;
      const previousY = previousHistoryTouchYRef.current;
      if (nextY === undefined || previousY === null) return;
      previousHistoryTouchYRef.current = nextY;
      if (nextY > previousY + 2) {
        gate.beginGesture();
        maybeLoadOlderHistory();
      } else if (nextY < previousY - 2) {
        gate.endGesture();
        clearHistoryBottomCompensation();
      }
    };
    const handleTouchEnd = () => {
      previousHistoryTouchYRef.current = null;
      maybeLoadOlderHistory();
      scheduleTouchGestureEnd();
    };
    const handleTouchCancel = () => {
      previousHistoryTouchYRef.current = null;
      finishTouchGesture();
    };
    const handlePointerDown = (event: PointerEvent) => {
      if (event.pointerType !== "mouse" || !event.isPrimary || event.button !== 0) return;
      if (historyGestureEndTimeoutRef.current) {
        clearTimeout(historyGestureEndTimeoutRef.current);
        historyGestureEndTimeoutRef.current = null;
      }
      gate.endGesture();
      historyPointerGestureActiveRef.current = true;
      previousHistoryPointerScrollTopRef.current = container.scrollTop;
    };
    const handlePointerEnd = () => {
      if (!historyPointerGestureActiveRef.current) return;
      historyPointerGestureActiveRef.current = false;
      gate.endGesture();
    };
    const isBackwardKey = (event: KeyboardEvent) =>
      event.key === "ArrowUp" ||
      event.key === "PageUp" ||
      event.key === "Home" ||
      (event.shiftKey && (event.key === " " || event.key === "Spacebar"));
    const isForwardKey = (event: KeyboardEvent) =>
      event.key === "ArrowDown" ||
      event.key === "PageDown" ||
      event.key === "End" ||
      (!event.shiftKey && (event.key === " " || event.key === "Spacebar"));
    const handleKeyDown = (event: KeyboardEvent) => {
      if (
        event.target instanceof Element &&
        event.target.closest(
          "input, textarea, select, button, a, [contenteditable='true'], [role='button'], [role='textbox']"
        )
      ) {
        return;
      }
      if (isForwardKey(event)) {
        gate.endGesture();
        clearHistoryBottomCompensation();
        if (historyKeyIntentTimeoutRef.current) {
          clearTimeout(historyKeyIntentTimeoutRef.current);
          historyKeyIntentTimeoutRef.current = null;
        }
        return;
      }
      if (!isBackwardKey(event)) return;
      gate.beginGesture();
      maybeLoadOlderHistory();
      if (historyKeyIntentTimeoutRef.current) {
        clearTimeout(historyKeyIntentTimeoutRef.current);
      }
      historyKeyIntentTimeoutRef.current = setTimeout(() => {
        gate.endGesture();
        historyKeyIntentTimeoutRef.current = null;
      }, 500);
    };
    const handleKeyUp = (event: KeyboardEvent) => {
      if (!isBackwardKey(event)) return;
      if (historyKeyIntentTimeoutRef.current) {
        clearTimeout(historyKeyIntentTimeoutRef.current);
        historyKeyIntentTimeoutRef.current = null;
      }
      gate.endGesture();
    };
    const handleScroll = () => {
      const nextScrollTop = container.scrollTop;
      if (
        historyPointerGestureActiveRef.current &&
        nextScrollTop < previousHistoryPointerScrollTopRef.current
      ) {
        gate.beginGesture();
        maybeLoadOlderHistory();
      } else if (
        (historyPointerGestureActiveRef.current ||
          (historyTouchGestureActiveRef.current && previousHistoryTouchYRef.current === null)) &&
        nextScrollTop > previousHistoryPointerScrollTopRef.current
      ) {
        clearHistoryBottomCompensation();
      }
      previousHistoryPointerScrollTopRef.current = nextScrollTop;
      if (historyTouchGestureActiveRef.current && previousHistoryTouchYRef.current === null) {
        maybeLoadOlderHistory();
        scheduleTouchGestureEnd();
      }
    };
    const handleScrollEnd = () => {
      if (suppressedHistoryScrollEndsRef.current > 0) {
        suppressedHistoryScrollEndsRef.current -= 1;
        return;
      }
      if (historyPointerGestureActiveRef.current) return;
      if (historyTouchGestureActiveRef.current) {
        if (historyTouchGestureEndTimeoutRef.current) {
          clearTimeout(historyTouchGestureEndTimeoutRef.current);
        }
        finishTouchGesture();
        return;
      }
      if (historyGestureEndTimeoutRef.current) {
        clearTimeout(historyGestureEndTimeoutRef.current);
        if (delaysWheelGestureEndAfterScrollEnd) {
          historyGestureEndTimeoutRef.current = setTimeout(finishWheelGesture, 80);
        } else {
          finishWheelGesture();
        }
        return;
      }
      if (historyKeyIntentTimeoutRef.current) {
        clearTimeout(historyKeyIntentTimeoutRef.current);
        historyKeyIntentTimeoutRef.current = null;
      }
      gate.endGesture();
    };

    container.addEventListener("wheel", handleWheel, {
      passive: !usesMacOSWheelGestureStart
    });
    container.addEventListener("touchstart", handleTouchStart, { passive: true });
    container.addEventListener("touchmove", handleTouchMove, { passive: true });
    container.addEventListener("touchend", handleTouchEnd, { passive: true });
    container.addEventListener("touchcancel", handleTouchCancel, { passive: true });
    container.addEventListener("pointerdown", handlePointerDown);
    container.addEventListener("scroll", handleScroll, { passive: true });
    container.addEventListener("scrollend", handleScrollEnd);
    window.addEventListener("pointerup", handlePointerEnd);
    window.addEventListener("pointercancel", handlePointerEnd);
    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("keyup", handleKeyUp);
    return () => {
      container.removeEventListener("wheel", handleWheel);
      container.removeEventListener("touchstart", handleTouchStart);
      container.removeEventListener("touchmove", handleTouchMove);
      container.removeEventListener("touchend", handleTouchEnd);
      container.removeEventListener("touchcancel", handleTouchCancel);
      container.removeEventListener("pointerdown", handlePointerDown);
      container.removeEventListener("scroll", handleScroll);
      container.removeEventListener("scrollend", handleScrollEnd);
      window.removeEventListener("pointerup", handlePointerEnd);
      window.removeEventListener("pointercancel", handlePointerEnd);
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("keyup", handleKeyUp);
      if (historyGestureEndTimeoutRef.current) {
        clearTimeout(historyGestureEndTimeoutRef.current);
        historyGestureEndTimeoutRef.current = null;
      }
      if (historyTouchGestureEndTimeoutRef.current) {
        clearTimeout(historyTouchGestureEndTimeoutRef.current);
        historyTouchGestureEndTimeoutRef.current = null;
      }
      if (historyKeyIntentTimeoutRef.current) {
        clearTimeout(historyKeyIntentTimeoutRef.current);
        historyKeyIntentTimeoutRef.current = null;
      }
      historyWheelGestureStartPendingRef.current = false;
      historyPreviousWheelCancelableRef.current = null;
      historyTouchGestureActiveRef.current = false;
      historyPointerGestureActiveRef.current = false;
      gate.resetIntent();
    };
  }, [clearHistoryBottomCompensation, historyPaginationLifecycle, maybeLoadOlderHistory]);

  useLayoutEffect(() => {
    if (isLoadingOlderHistory || !pendingHistoryScrollRestoreRef.current) return;
    const sessionId = pendingHistoryScrollRestoreSessionIdRef.current;
    const snapshot = pendingHistoryScrollRestoreRef.current;
    pendingHistoryScrollRestoreRef.current = null;
    pendingHistoryScrollRestoreSessionIdRef.current = null;
    const container = chatContainerRef.current;
    if (!container || !sessionId || activeSessionIdRef.current !== sessionId) return;

    let restoredTop = restoredChatHistoryScrollTop(snapshot, container.scrollHeight);
    if (snapshot.anchorId && snapshot.anchorOffset !== undefined) {
      const anchor = Array.from(
        container.querySelectorAll<HTMLElement>("[data-history-anchor-ids]")
      ).find((candidate) =>
        candidate.dataset.historyAnchorIds?.split(" ").includes(snapshot.anchorId!)
      );
      if (anchor) {
        restoredTop = restoredChatHistoryAnchorScrollTop(
          container.scrollTop,
          snapshot.anchorOffset,
          anchor.getBoundingClientRect().top - container.getBoundingClientRect().top
        );
      }
    }
    const compensation = historyBottomCompensationRef.current;
    const currentCompensation = compensation?.offsetHeight ?? 0;
    const missingRange = requiredChatHistoryBottomCompensation(
      restoredTop,
      container.scrollHeight - currentCompensation,
      container.clientHeight
    );
    if (compensation)
      compensation.style.height = missingRange > 0 ? `${missingRange + 1}px` : "0px";
    const previousScrollTop = container.scrollTop;
    container.scrollTop = restoredTop;
    if (Math.abs(container.scrollTop - previousScrollTop) > 0.5) {
      suppressedHistoryScrollEndsRef.current += 1;
    }
  }, [isLoadingOlderHistory, timelineItems]);

  useLayoutEffect(() => {
    if (isLoadingOlderHistory) return;
    if (
      historyPaginationLifecycle.gate.tryStartQueuedLoad({
        canLoad: Boolean(activeSessionId && hasMoreOlderHistory)
      })
    ) {
      void loadOlderHistory();
    }
  }, [
    activeSessionId,
    hasMoreOlderHistory,
    historyPaginationLifecycle,
    isLoadingOlderHistory,
    loadOlderHistory
  ]);

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
  const isStopping = Boolean(
    activeSessionId && activeRunId && stoppingSessionIds.has(activeSessionId)
  );
  const isAgentSendLocked = isAgentComposerSendLocked({
    areSettingsLocked: areAgentSettingsLocked,
    isStopping
  });
  const isSending = Boolean(activeRunId) || isSubmitting;
  const queuedMessages = useMemo(
    () => (activeSessionId ? (queueBySession[activeSessionId]?.items ?? []) : []),
    [activeSessionId, queueBySession]
  );
  const editingQueueId =
    queueEdit && activeSessionId && queueEdit.sessionId === activeSessionId
      ? queueEdit.queueId
      : null;

  useEffect(() => {
    const current = queueEditRef.current;
    if (!current) return;
    if (current.sessionId !== activeSessionId) {
      void agentRuntimeService
        .endQueuedMessageEdit(userId, {
          sessionId: current.sessionId,
          queueId: current.queueId
        })
        .catch(() => undefined);
      setQueueEdit(null);
      return;
    }
    if (!queuedMessageEditStillPresent(current, queuedMessages)) {
      setQueueEdit(null);
    }
  }, [activeSessionId, agentRuntimeService, queuedMessages, userId]);
  useLayoutEffect(() => {
    const request = agentComposerFocusRequestRef.current;
    if (!request) return;
    const settled = settleAgentComposerFocusRequest(request, {
      currentInteractionGeneration: interactionGenerationRef.current,
      isSubmitting,
      hasTimeline: timelineItems.length > 0,
      textarea: agentComposerTextareaRef.current,
      activeElement: document.activeElement,
      documentBody: document.body,
      documentRoot: document
    });
    if (settled && agentComposerFocusRequestRef.current === request) {
      agentComposerFocusRequestRef.current = null;
    }
  });
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

  const beginSessionSelection = useCallback(
    (sessionId: string): number => {
      interactionGenerationRef.current += 1;
      const generation = sessionSelectionGenerationRef.current + 1;
      sessionSelectionGenerationRef.current = generation;
      pendingSessionSelectionIdRef.current = sessionId;
      setPendingSessionSelectionId(sessionId);
      const protectedSessionIds = new Set<string>();
      if (activeSessionIdRef.current) protectedSessionIds.add(activeSessionIdRef.current);
      if (sessionId !== NEW_SESSION_PENDING_KEY) protectedSessionIds.add(sessionId);
      historyPaginationCacheRef.current.reconcileRetention(protectedSessionIds);
      return generation;
    },
    [historyPaginationCacheRef]
  );

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

  const markStoppingSession = useCallback((sessionId: string) => {
    setStoppingSessionIds((current) => {
      if (current.has(sessionId)) return current;
      const next = new Set(current);
      next.add(sessionId);
      return next;
    });
  }, []);

  const clearStoppingSession = useCallback((sessionId: string) => {
    setStoppingSessionIds((current) => {
      if (!current.has(sessionId)) return current;
      const next = new Set(current);
      next.delete(sessionId);
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

  const clearActiveRun = useCallback(
    (sessionId: string, expectedRunId?: string) => {
      const current = activeRunsBySessionRef.current;
      if (expectedRunId && current[sessionId] !== expectedRunId) return;
      if (!(sessionId in current)) return;
      const next = { ...current };
      delete next[sessionId];
      activeRunsBySessionRef.current = next;
      setActiveRunsBySession(next);
      clearStoppingSession(sessionId);
    },
    [clearStoppingSession]
  );

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
      promptHistoryReplacementTrackerRef.current.authoritativeReplace(sessionId);
      if (activeSessionIdRef.current === sessionId) {
        // Routine reconciliation (including runFinished) refreshes entries for
        // the next navigation session without disturbing the active snapshot.
        // Edit, send, pointer, task/account, and historyReplaced paths own exits.
        promptHistoryEntriesRef.current = agentPromptHistory(items);
        setTimelineItems(items);
      }
      return true;
    },
    [bumpTimelineRevision, promptHistoryReplacementTrackerRef, timelineRevisionBySessionRef]
  );

  const mergeSessionTimelineItem = useCallback(
    (sessionId: string, item: AgentTimelineItem) => {
      bumpTimelineRevision(sessionId);
      const result = historyPaginationCacheRef.current.mergeLiveItem(sessionId, item);
      publishHistorySnapshot(sessionId);
      return result;
    },
    [bumpTimelineRevision, historyPaginationCacheRef, publishHistorySnapshot]
  );

  const applyQueueSnapshot = useCallback(
    (sessionId: string, snapshot: AgentDesktopQueueSnapshot) => {
      setQueueBySession((current) => {
        const applied = applyAgentDesktopQueueSnapshot(current[sessionId], snapshot);
        if (current[sessionId] === applied) {
          return current;
        }
        return { ...current, [sessionId]: applied };
      });
    },
    []
  );

  const forgetSessionQueue = useCallback((sessionId: string) => {
    setQueueBySession((current) => {
      if (!current[sessionId]) return current;
      const next = { ...current };
      delete next[sessionId];
      return next;
    });
  }, []);

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
    [agentRuntimeService, enqueueProjectRootMutation, userId]
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
    [agentRuntimeService, enqueueProjectRootMutation, projectOrderState.confirmed, userId]
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
    [agentRuntimeService, enqueueProjectRootMutation, userId]
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
  }, [
    agentRuntimeService,
    isAuthTransitionReady,
    isInitializing,
    projectRoot,
    trackAgentWorkflow,
    userId
  ]);

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
    [
      agentRuntimeService,
      projectSkillsTrustPrompt,
      projectSkillsTrustSavingDecision,
      trackAgentWorkflow,
      userId
    ]
  );

  const publishSessionPageSnapshot = useCallback(() => {
    const snapshot = sessionPaginationCacheRef.current.snapshot();
    setSessions([...snapshot.items]);
    setHasMoreSessions(snapshot.hasMore);
    setIsLoadingOlderSessions(snapshot.isLoading && snapshot.headLoaded);
  }, [sessionPaginationCacheRef]);

  const refreshSessionList = useCallback(async () => {
    return await trackAgentWorkflow(async () => {
      const token = sessionPaginationCacheRef.current.beginHead();
      try {
        const page = await agentRuntimeService.listSessionsPage(userId, {
          projectRoot: null,
          limit: DEFAULT_AGENT_PAGE_SIZE
        });
        sessionPaginationCacheRef.current.commit(token, page);
        publishSessionPageSnapshot();
        setIsSessionHistoryReady(true);
      } catch (loadError) {
        sessionPaginationCacheRef.current.fail(token);
        publishSessionPageSnapshot();
        throw loadError;
      }
    });
  }, [
    agentRuntimeService,
    publishSessionPageSnapshot,
    sessionPaginationCacheRef,
    trackAgentWorkflow,
    userId
  ]);

  const loadOlderSessions = useCallback(async () => {
    const token = sessionPaginationCacheRef.current.beginOlder();
    if (!token) return;
    publishSessionPageSnapshot();
    try {
      const page = await trackAgentWorkflow(() =>
        agentRuntimeService.listSessionsPage(userId, {
          projectRoot: null,
          cursor: token.cursor,
          limit: DEFAULT_AGENT_PAGE_SIZE
        })
      );
      sessionPaginationCacheRef.current.commit(token, page);
    } catch (loadError) {
      if (isAgentPageStaleError(loadError)) {
        sessionPaginationCacheRef.current.clear();
        try {
          await refreshSessionList();
        } catch (headError) {
          setError(errorMessage(headError));
        }
      } else {
        sessionPaginationCacheRef.current.fail(token);
        setError(errorMessage(loadError));
      }
    } finally {
      publishSessionPageSnapshot();
    }
  }, [
    agentRuntimeService,
    publishSessionPageSnapshot,
    refreshSessionList,
    sessionPaginationCacheRef,
    trackAgentWorkflow,
    userId
  ]);

  const refreshSessions = useCallback(async () => {
    return await trackAgentWorkflow(async () => {
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
  }, [agentRuntimeService, applyRuntimeStatus, refreshSessionList, trackAgentWorkflow, userId]);

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
    [agentRuntimeService, userId]
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
    [agentRuntimeService, mcpServers, refreshSessionMcpServers, userId]
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
    [agentRuntimeService, userId]
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
    agentRuntimeService,
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
    if (!localProjectFolderActionsAvailable) return;
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
            agentSessionSelection.forget(agentOwnerKey);
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
    agentOwnerKey,
    agentSessionSelection,
    invalidateSessionSelection,
    localProjectFolderActionsAvailable,
    registerProjectRoot,
    restoreNewTaskModel,
    trackAgentWorkflow
  ]);

  const selectProjectRoot = useCallback(
    (value: string) => {
      invalidateSessionSelection();
      agentSessionSelection.forget(agentOwnerKey);
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
      agentOwnerKey,
      agentSessionSelection,
      invalidateSessionSelection,
      persistSelectedProjectRoot,
      refreshSessions,
      restoreNewTaskModel
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
    [agentRuntimeService, applyAuthoritativeMode, permissionModeUpdateRef, userId]
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
      agentRuntimeService,
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
        sessionPaginationCacheRef.current.upsert(detail.session);
        publishSessionPageSnapshot();
        let createdTimeline = detail.timeline;
        if (
          historyPaginationCacheRef.current.seedLiveTimeline(sessionId, detail.timeline) ===
          "synchronized-reload-required"
        ) {
          throw new Error("Agent live history requires a synchronized reconnect");
        }
        if (agentRuntimeService.target.kind === "remote") {
          createdTimeline = await loadHistoryHead(sessionId);
        }
        replaceSessionTimeline(sessionId, createdTimeline);
        applyQueueSnapshot(sessionId, detail.queue ?? emptyAgentDesktopQueueSnapshot());

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
          agentSessionSelection.remember(agentOwnerKey, sessionId);
          isAgentModelLockedRef.current = false;
          currentAgentModelRef.current = requestModel;
          setModel(requestModel);
          applyAuthoritativeMode(normalizeAgentPermissionMode(detail.session.mode));
          replaceSessionTimeline(sessionId, createdTimeline);
          applyQueueSnapshot(sessionId, detail.queue ?? emptyAgentDesktopQueueSnapshot());
          const mcpError = mcpConnectionErrorMessage(detail.mcpErrors);
          if (mcpError) setError(mcpError);
        }
      }

      return { sessionId, requestModel };
    },
    [
      agentOwnerKey,
      agentRuntimeService,
      agentModelPreferenceRef,
      applyAuthoritativeMode,
      applyQueueSnapshot,
      agentSessionSelection,
      contextLimitForModel,
      deletedSessionIdsRef,
      historyPaginationCacheRef,
      loadHistoryHead,
      projectRoot,
      publishSessionPageSnapshot,
      replaceSessionTimeline,
      sessionPaginationCacheRef,
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
        sessionPaginationCacheRef.current.upsert(detail.session);
        publishSessionPageSnapshot();
        let createdTimeline = detail.timeline;
        if (
          historyPaginationCacheRef.current.seedLiveTimeline(detail.session.id, detail.timeline) ===
          "synchronized-reload-required"
        ) {
          throw new Error("Agent live history requires a synchronized reconnect");
        }
        if (agentRuntimeService.target.kind === "remote") {
          createdTimeline = await loadHistoryHead(detail.session.id);
        }
        replaceSessionTimeline(detail.session.id, createdTimeline);
        applyQueueSnapshot(detail.session.id, detail.queue ?? emptyAgentDesktopQueueSnapshot());

        if (
          isAgentModeMountedRef.current &&
          sessionSelectionGenerationRef.current === selectionGeneration &&
          interactionGenerationRef.current === interactionGeneration
        ) {
          shouldAutoScrollRef.current = true;
          activeSessionIdRef.current = detail.session.id;
          setActiveSessionId(detail.session.id);
          agentSessionSelection.remember(agentOwnerKey, detail.session.id);
          setProjectRoot(detail.session.projectRoot);
          isAgentModelLockedRef.current = false;
          currentAgentModelRef.current = requestModel;
          setModel(requestModel);
          applyAuthoritativeMode(normalizeAgentPermissionMode(detail.session.mode));
          replaceSessionTimeline(detail.session.id, createdTimeline);
          applyQueueSnapshot(detail.session.id, detail.queue ?? emptyAgentDesktopQueueSnapshot());
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
      agentOwnerKey,
      agentRuntimeService,
      agentModelPreferenceRef,
      applyAuthoritativeMode,
      applyQueueSnapshot,
      agentSessionSelection,
      beginSessionSelection,
      contextLimitForModel,
      deletedSessionIdsRef,
      finishSessionSelection,
      isAgentModelCatalogLoading,
      historyPaginationCacheRef,
      loadHistoryHead,
      projectRoot,
      publishSessionPageSnapshot,
      replaceSessionTimeline,
      runtimeStatus?.running,
      selectedNewChatMcpServerNames,
      sessionPaginationCacheRef,
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
        const session = sessionPaginationCacheRef.current
          .snapshot()
          .items.find((candidate) => candidate.id === sessionId);
        if (!session) throw new Error("This task is not in the loaded task history.");
        const [timeline, queue] = await trackAgentWorkflow(async () => {
          return await Promise.all([
            loadHistoryHead(sessionId),
            agentRuntimeService.target.kind === "local"
              ? agentRuntimeService.getDesktopQueueSnapshot(userId, sessionId)
              : Promise.resolve(emptyAgentDesktopQueueSnapshot())
          ]);
        });
        if (
          !isAgentModeMountedRef.current ||
          sessionSelectionGenerationRef.current !== selectionGeneration ||
          interactionGenerationRef.current !== interactionGeneration ||
          deletedSessionIdsRef.current.has(sessionId)
        ) {
          return;
        }

        // Commit the selected session and all of its settings together. Until
        // this point the previous chat remains active and its composer is gated.
        shouldAutoScrollRef.current = true;
        promptHistoryReplacementTrackerRef.current.abandonInactive(session.id);
        promptHistoryEntriesRef.current = agentPromptHistory(timeline);
        activeSessionIdRef.current = session.id;
        clearCompletedUnreadSession(session.id);
        setActiveSessionId(session.id);
        agentSessionSelection.remember(agentOwnerKey, session.id);
        setProjectRoot(session.projectRoot);
        const isModelLocked =
          session.messageCount > 0 ||
          hasAgentUserMessage(timeline) ||
          Boolean(activeRunsBySessionRef.current[session.id]) ||
          pendingSendTokensRef.current.has(session.id);
        isAgentModelLockedRef.current = isModelLocked;
        const sessionModel = resolveAgentModelForSession(
          newTaskAgentModel(agentModelPreferenceRef.current),
          session.model,
          isModelLocked
        );
        currentAgentModelRef.current = sessionModel;
        setModel(sessionModel);
        applyAuthoritativeMode(normalizeAgentPermissionMode(session.mode));
        publishHistorySnapshot(session.id);
        applyQueueSnapshot(session.id, queue);
        if (activeRunsBySessionRef.current[session.id]) {
          thoughtPhaseTrackerRef.current.seedActiveTimeline(session.id, timeline);
          observeActiveThoughtPhase(session.id);
        }
        finishSessionSelection(selectionGeneration);

        try {
          await persistSelectedProjectRoot(session.projectRoot);
        } catch (persistError) {
          if (
            isAgentModeMountedRef.current &&
            sessionSelectionGenerationRef.current === selectionGeneration &&
            interactionGenerationRef.current === interactionGeneration &&
            activeSessionIdRef.current === session.id
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
      agentOwnerKey,
      agentRuntimeService,
      agentModelPreferenceRef,
      applyAuthoritativeMode,
      applyQueueSnapshot,
      agentSessionSelection,
      beginSessionSelection,
      clearCompletedUnreadSession,
      deletedSessionIdsRef,
      finishSessionSelection,
      observeActiveThoughtPhase,
      pendingSendTokensRef,
      persistSelectedProjectRoot,
      loadHistoryHead,
      publishHistorySnapshot,
      sessionPaginationCacheRef,
      promptHistoryReplacementTrackerRef,
      thoughtPhaseTrackerRef,
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

    const rememberedSessionId = agentSessionSelection.resolve(agentOwnerKey, visibleSessions, {
      historyComplete: !hasMoreSessions
    });
    if (rememberedSessionId) {
      hasAttemptedSessionRestoreRef.current = true;
      void loadSession(rememberedSessionId);
    } else if (!hasMoreSessions) {
      hasAttemptedSessionRestoreRef.current = true;
    }
  }, [
    agentOwnerKey,
    agentSessionSelection,
    isAuthTransitionReady,
    isInitializing,
    isSessionHistoryReady,
    hasMoreSessions,
    loadSession,
    visibleSessions,
    userId
  ]);

  const sendMessage = useCallback(
    async (restoreComposerFocus = false) => {
      let text = input.trim();
      const requestedSessionId = activeSessionIdRef.current;
      let pendingSessionKey = requestedSessionId || NEW_SESSION_PENDING_KEY;
      if (
        !canSubmitAgentComposerMessage({
          text,
          isSendLocked: isAgentSendLocked,
          isSessionSelectionPending: pendingSessionSelectionIdRef.current !== null,
          hasInFlightSend: pendingSendTokensRef.current.has(pendingSessionKey),
          hasQueuedMessages: queuedMessages.length > 0,
          hasActiveRun: Boolean(activeRunId)
        })
      ) {
        return;
      }

      const activeEdit =
        queueEditRef.current &&
        requestedSessionId &&
        queueEditRef.current.sessionId === requestedSessionId
          ? queueEditRef.current
          : null;
      if (activeEdit && requestedSessionId) {
        const stashedDraft = discardQueuedMessageEdit(activeEdit);
        setQueueEdit(null);
        try {
          const snapshot = await agentRuntimeService.updateQueuedMessage(userId, {
            sessionId: requestedSessionId,
            queueId: activeEdit.queueId,
            text
          });
          applyQueueSnapshot(requestedSessionId, snapshot);
        } catch (queueError) {
          if (activeSessionIdRef.current === requestedSessionId) {
            setInput(text);
            setError(errorMessage(queueError));
          }
          return;
        }
        if (activeRunId) {
          setInput(stashedDraft);
          return;
        }
        text = stashedDraft.trim();
      }

      const selectionGeneration = sessionSelectionGenerationRef.current;
      const interactionGeneration = interactionGenerationRef.current;
      const sendToken = nextSendTokenRef.current + 1;
      nextSendTokenRef.current = sendToken;
      let targetSessionId = requestedSessionId;
      if (restoreComposerFocus) {
        agentComposerFocusRequestRef.current = {
          sendToken,
          interactionGeneration,
          waitForTimeline: timelineItems.length === 0
        };
      }
      markPendingSend(pendingSessionKey, sendToken);

      setError(null);
      resetPromptHistoryNavigation();
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
          if (shouldPrepareThoughtAfterAgentSend(response.queued)) {
            thoughtPhaseTrackerRef.current.prepareUserRequest(sessionId, text);
          }
          if (response.queue) {
            applyQueueSnapshot(sessionId, response.queue);
          }
          if (!terminalRunIdsRef.current.has(response.runId)) {
            recordActiveRun(sessionId, response.runId);
          }
        });
      } catch (sendError) {
        const focusRequest = agentComposerFocusRequestRef.current;
        if (focusRequest?.sendToken === sendToken) {
          focusRequest.waitForTimeline = false;
        }
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
    },
    [
      activeRunId,
      agentRuntimeService,
      applyQueueSnapshot,
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
      queuedMessages.length,
      recordActiveRun,
      resetPromptHistoryNavigation,
      scrollTimelineToBottom,
      terminalRunIdsRef,
      thoughtPhaseTrackerRef,
      timelineItems.length,
      trackAgentWorkflow,
      userId
    ]
  );

  const cancelPrompt = useCallback(async () => {
    const sessionId = activeSessionIdRef.current;
    const currentRunId = sessionId ? activeRunsBySessionRef.current[sessionId] : activeRunId;
    const pendingSessionKey = sessionId || NEW_SESSION_PENDING_KEY;
    const pendingSendToken = pendingSendTokensRef.current.get(pendingSessionKey);
    const plan = planAgentComposerStop({
      hasActiveRun: Boolean(currentRunId),
      hasInFlightSend: pendingSendToken !== undefined
    });
    if (plan.markInFlightSendCancelled && pendingSendToken !== undefined) {
      cancelledPendingSendTokensRef.current.add(pendingSendToken);
    }
    if (!plan.cancelActiveRun || !currentRunId) {
      return;
    }
    if (plan.lockSendUntilRunFinished && sessionId) {
      markStoppingSession(sessionId);
    }
    try {
      await agentRuntimeService.cancelRun(userId, currentRunId);
      if (
        sessionId &&
        shouldClearStoppingSendLock({
          cancelledRunId: currentRunId,
          trackedRunId: activeRunsBySessionRef.current[sessionId]
        })
      ) {
        clearStoppingSession(sessionId);
      }
    } catch (cancelError) {
      if (sessionId) {
        clearStoppingSession(sessionId);
      }
      if (activeSessionIdRef.current === sessionId) {
        setError(errorMessage(cancelError));
      }
    }
  }, [
    activeRunId,
    agentRuntimeService,
    cancelledPendingSendTokensRef,
    clearStoppingSession,
    markStoppingSession,
    pendingSendTokensRef,
    userId
  ]);

  const discardQueueEdit = useCallback(() => {
    const current = queueEditRef.current;
    if (!current) return;
    setInput(discardQueuedMessageEdit(current));
    setQueueEdit(null);
    void agentRuntimeService
      .endQueuedMessageEdit(userId, {
        sessionId: current.sessionId,
        queueId: current.queueId
      })
      .catch((queueError) => {
        if (activeSessionIdRef.current === current.sessionId) {
          setError(errorMessage(queueError));
        }
      });
  }, [agentRuntimeService, userId]);

  const cancelQueuedMessage = useCallback(
    async (queueId: string) => {
      const sessionId = activeSessionIdRef.current;
      if (!sessionId) return;
      const currentEdit = queueEditRef.current;
      if (currentEdit?.sessionId === sessionId && currentEdit.queueId === queueId) {
        setInput(discardQueuedMessageEdit(currentEdit));
        setQueueEdit(null);
      }
      applyQueueSnapshot(
        sessionId,
        queueSnapshotWithoutItem(queueBySessionRef.current[sessionId], queueId)
      );
      try {
        const snapshot = await agentRuntimeService.cancelQueuedMessage(userId, {
          sessionId,
          queueId
        });
        applyQueueSnapshot(sessionId, snapshot);
      } catch (queueError) {
        if (activeSessionIdRef.current === sessionId) {
          setError(errorMessage(queueError));
        }
      }
    },
    [agentRuntimeService, applyQueueSnapshot, userId]
  );

  const editQueuedMessage = useCallback(
    async (queueId: string) => {
      const sessionId = activeSessionIdRef.current;
      if (!sessionId) return;
      const item = (queueBySessionRef.current[sessionId]?.items ?? []).find(
        (queued) => queued.queueId === queueId
      );
      if (!item) return;
      const next = beginQueuedMessageEdit({
        current: queueEditRef.current,
        sessionId,
        item,
        composerText: input
      });
      if (!next) {
        discardQueueEdit();
        return;
      }
      try {
        await agentRuntimeService.beginQueuedMessageEdit(userId, {
          sessionId,
          queueId
        });
        if (activeSessionIdRef.current !== sessionId) {
          void agentRuntimeService
            .endQueuedMessageEdit(userId, { sessionId, queueId })
            .catch(() => undefined);
          return;
        }
        setQueueEdit(next.edit);
        setInput(next.composer);
      } catch (queueError) {
        if (activeSessionIdRef.current === sessionId) {
          setError(errorMessage(queueError));
        }
      }
    },
    [agentRuntimeService, discardQueueEdit, input, userId]
  );

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
    [agentRuntimeService, userId]
  );

  const handleKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (event.nativeEvent.isComposing) return;
      if (event.key === "Escape" && queueEditRef.current) {
        event.preventDefault();
        discardQueueEdit();
        return;
      }
      const direction = agentPromptHistoryDirection(
        {
          key: event.key,
          value: event.currentTarget.value,
          selectionStart: event.currentTarget.selectionStart,
          selectionEnd: event.currentTarget.selectionEnd,
          altKey: event.altKey,
          ctrlKey: event.ctrlKey,
          metaKey: event.metaKey,
          shiftKey: event.shiftKey,
          isComposing: event.nativeEvent.isComposing
        },
        promptHistoryNavigationRef.current
      );
      if (direction) {
        const step = navigateAgentPromptHistory(
          promptHistoryNavigationRef.current,
          promptHistoryEntriesRef.current,
          direction
        );
        if (step) {
          event.preventDefault();
          promptHistoryNavigationRef.current = step.navigation;
          promptHistoryGenerationRef.current += 1;
          const promptHistoryGeneration = promptHistoryGenerationRef.current;
          setInput(step.value);
          const textarea = event.currentTarget;
          requestAnimationFrame(() => {
            if (
              promptHistoryGenerationRef.current === promptHistoryGeneration &&
              textarea.isConnected &&
              document.activeElement === textarea
            ) {
              const caret = textarea.value.length;
              textarea.setSelectionRange(caret, caret);
            }
          });
          return;
        }
      }
      if (
        (event.shiftKey || isCompactLayout) &&
        continueChatComposerList(event, handleAgentInputChange)
      ) {
        return;
      }
      if (event.key === "Enter" && !event.shiftKey && !isCompactLayout) {
        event.preventDefault();
        void sendMessage(true);
      }
    },
    [discardQueueEdit, handleAgentInputChange, isCompactLayout, sendMessage]
  );

  const handleBeforeInput = useCallback(
    (event: React.FormEvent<HTMLTextAreaElement>) => {
      continueChatComposerListBeforeInput(event, handleAgentInputChange);
    },
    [handleAgentInputChange]
  );

  const removeSessionFromState = useCallback(
    (sessionId: string) => {
      deletedSessionIdsRef.current.add(sessionId);
      sessionPaginationCacheRef.current.remove(sessionId);
      historyPaginationCacheRef.current.remove(sessionId);
      thoughtPhaseTrackerRef.current.forgetSession(sessionId);
      cancelThoughtLabelDisplays(sessionId);
      agentSessionSelection.forget(agentOwnerKey, sessionId);
      timelineRevisionBySessionRef.current.delete(sessionId);
      publishSessionPageSnapshot();
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
      forgetSessionQueue(sessionId);
      setSessionToDelete((current) => (current?.id === sessionId ? null : current));
      setSessionToRename((current) => (current?.id === sessionId ? null : current));

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
      agentOwnerKey,
      agentSessionSelection,
      cancelThoughtLabelDisplays,
      clearActiveRun,
      deletedSessionIdsRef,
      forgetSessionQueue,
      historyPaginationCacheRef,
      publishSessionPageSnapshot,
      restoreNewTaskModel,
      sessionPaginationCacheRef,
      thoughtPhaseTrackerRef,
      timelineRevisionBySessionRef
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
    [agentRuntimeService, removeSessionFromState, userId]
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
          agentSessionSelection.forget(agentOwnerKey);
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
      agentOwnerKey,
      agentRuntimeService,
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
      sessionPaginationCacheRef.current.upsert(summary);
      publishSessionPageSnapshot();
    },
    [deletedSessionIdsRef, publishSessionPageSnapshot, sessionPaginationCacheRef]
  );

  const renameAgentSession = useCallback(
    async (sessionId: string, title: string) => {
      const revision = sessionPaginationCacheRef.current.summaryRevision(sessionId);
      const summary = await agentRuntimeService.renameSession(userId, { sessionId, title });
      if (
        !isAgentModeMountedRef.current ||
        userIdRef.current !== userId ||
        deletedSessionIdsRef.current.has(sessionId) ||
        sessionPaginationCacheRef.current.summaryRevision(sessionId) !== revision
      ) {
        return;
      }
      upsertSessionSummary(summary);
    },
    [
      agentRuntimeService,
      deletedSessionIdsRef,
      sessionPaginationCacheRef,
      upsertSessionSummary,
      userId
    ]
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

  const reconcileAgentEventGap = useCallback(
    (affectedSessionId: string | null) => {
      if (affectedSessionId) {
        pendingEventGapSessionIdsRef.current.add(affectedSessionId);
      } else {
        hasUnknownEventGapRef.current = true;
      }
      if (eventGapRecoveryRef.current) return;
      const recovery = (async () => {
        do {
          const sessionIds = new Set(pendingEventGapSessionIdsRef.current);
          pendingEventGapSessionIdsRef.current.clear();
          const hadUnknownGap = hasUnknownEventGapRef.current;
          hasUnknownEventGapRef.current = false;
          try {
            const runStateGeneration = runStateGenerationRef.current;
            const status = await agentRuntimeService.getRuntimeStatus(userId);
            applyRuntimeStatus(status, runStateGeneration);
            await refreshSessionList();
            if (activeSessionIdRef.current) sessionIds.add(activeSessionIdRef.current);
            if (hadUnknownGap && sessionIds.size === 0) {
              const newestSession = sessionPaginationCacheRef.current.snapshot().items[0];
              if (newestSession) sessionIds.add(newestSession.id);
            }
            const reloadableSessionIds = [...sessionIds].filter(
              (sessionId) => !deletedSessionIdsRef.current.has(sessionId)
            );
            if (agentRuntimeService.target.kind === "remote") {
              // One synchronized remote attach replaces every account live
              // overlay at the same C0. Loading multiple heads would only
              // churn the single account stream and cannot improve recovery.
              const sessionId =
                activeSessionIdRef.current &&
                reloadableSessionIds.includes(activeSessionIdRef.current)
                  ? activeSessionIdRef.current
                  : reloadableSessionIds[0];
              if (sessionId) await loadHistoryHead(sessionId);
            } else {
              await Promise.all(
                reloadableSessionIds.map((sessionId) => loadHistoryHead(sessionId))
              );
            }
          } catch (gapError) {
            if (isAgentModeMountedRef.current && userIdRef.current === userId) {
              setError(errorMessage(gapError));
            }
          }
        } while (pendingEventGapSessionIdsRef.current.size > 0 || hasUnknownEventGapRef.current);
      })();
      eventGapRecoveryRef.current = recovery;
      void recovery.finally(() => {
        if (eventGapRecoveryRef.current === recovery) eventGapRecoveryRef.current = null;
      });
    },
    [
      agentRuntimeService,
      applyRuntimeStatus,
      deletedSessionIdsRef,
      loadHistoryHead,
      pendingEventGapSessionIdsRef,
      refreshSessionList,
      sessionPaginationCacheRef,
      userId
    ]
  );

  const settleFinishedAgentRun = useCallback(
    (sessionId: string, runId: string, terminal: "completed" | "cancelled" | "failed") => {
      runStateGenerationRef.current += 1;
      terminalRunIdsRef.current.add(runId);
      thoughtPhaseSeededRunIdsRef.current.delete(runId);
      const finishedTimelineRevision = bumpTimelineRevision(sessionId);
      clearActiveRun(sessionId, runId);
      // The terminal event is authoritative for run state. Refresh only
      // persisted session metadata here: a concurrent status snapshot could
      // otherwise resurrect the completed run.
      void refreshSessionList().catch(() => {});
      const thoughtRunFinished = handleAgentModeThoughtRunFinished({
        event: {
          eventType: "runFinished",
          sessionId,
          runId,
          message: terminal
        },
        timelineRevision: finishedTimelineRevision,
        tracker: thoughtPhaseTrackerRef.current,
        finalizePhase: completeThoughtPhase,
        releaseProvisional: (phase) => {
          thoughtLabelProvisionalSchedulerRef.current?.complete(phase.sessionId, phase.phaseId);
        },
        cancelAndInvalidateLabels: (finishedSessionId, assistantTurnId) => {
          if (assistantTurnId) {
            invalidateThoughtLabelsForTurn(finishedSessionId, assistantTurnId);
          } else {
            invalidateThoughtLabelsForSession(finishedSessionId);
          }
        },
        loadTimeline: async (finishedSessionId) => await loadHistoryHead(finishedSessionId),
        canApplyTimeline: (finishedSessionId) =>
          isAgentModeMountedRef.current &&
          userIdRef.current === userId &&
          !deletedSessionIdsRef.current.has(finishedSessionId),
        replaceTimeline: replaceSessionTimeline
      });
      if (thoughtRunFinished) {
        if (terminal === "completed" && sessionId !== activeSessionIdRef.current) {
          markCompletedUnreadSession(sessionId);
        }
        void thoughtRunFinished.catch(() => {});
      }
    },
    [
      bumpTimelineRevision,
      clearActiveRun,
      completeThoughtPhase,
      deletedSessionIdsRef,
      invalidateThoughtLabelsForSession,
      invalidateThoughtLabelsForTurn,
      loadHistoryHead,
      markCompletedUnreadSession,
      refreshSessionList,
      replaceSessionTimeline,
      terminalRunIdsRef,
      thoughtPhaseSeededRunIdsRef,
      thoughtPhaseTrackerRef,
      userId
    ]
  );

  const handleAgentEvent = useCallback(
    (event: AgentEventEnvelope) => {
      const eventSessionId = event.sessionId || event.session?.id;
      const acceptance = historyPaginationCacheRef.current.acceptEvent(event);
      if (acceptance === "duplicate" || acceptance === "invalid") return;
      if (acceptance === "gap") {
        // Event sequence is account-wide, so reconcile account/runtime and
        // bounded affected heads together. The replay journal will become the
        // first recovery step when its attach contract is wired.
        reconcileAgentEventGap(eventSessionId ?? null);
        return;
      }
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
            historyPaginationCacheRef.current.startLiveSuffix(event.sessionId);
            bumpTimelineRevision(event.sessionId);
            clearCompletedUnreadSession(event.sessionId);
            recordActiveRun(event.sessionId, event.runId);
          }
          break;
        case "timelineItem":
          if (event.item && event.sessionId) {
            const mergeResult = mergeSessionTimelineItem(event.sessionId, event.item);
            if (mergeResult === "applied") {
              observeLiveThoughtItem(event.sessionId, event.item);
            } else {
              setError(
                "Agent live history exceeded its safe cache window. Reconnect to reload this task."
              );
              reconcileAgentEventGap(event.sessionId);
            }
          }
          break;
        case "queueChanged":
          if (event.sessionId && event.queue) {
            applyQueueSnapshot(event.sessionId, event.queue);
          }
          break;
        case "queuePromoted":
          if (event.sessionId && event.queue) {
            applyQueueSnapshot(event.sessionId, event.queue);
          }
          break;
        case "runFinished": {
          if (
            event.sessionId &&
            event.runId &&
            (event.message === "completed" ||
              event.message === "cancelled" ||
              event.message === "failed")
          ) {
            settleFinishedAgentRun(event.sessionId, event.runId, event.message);
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
            const mergeResult = mergeSessionTimelineItem(event.sessionId, event.item);
            if (mergeResult === "applied") {
              observeLiveThoughtItem(event.sessionId, event.item);
            } else {
              setError(
                "Agent live history exceeded its safe cache window. Reconnect to reload this task."
              );
              reconcileAgentEventGap(event.sessionId);
            }
          }
          break;
        case "historyReplaced":
          void (async () => {
            const id = event.sessionId || activeSessionIdRef.current;
            if (!id) return;
            let replacementAttempt: AgentPromptHistoryReplacementAttempt | null = null;
            const recoverPromptHistory = (activeId: string | null) => {
              if (!replacementAttempt) return;
              const fallback = promptHistoryReplacementTrackerRef.current.recover(
                replacementAttempt,
                activeId
              );
              if (fallback !== null) {
                promptHistoryEntriesRef.current = fallback;
              }
            };
            if (activeSessionIdRef.current === id) {
              resetPromptHistoryNavigation();
              replacementAttempt = promptHistoryReplacementTrackerRef.current.begin(
                id,
                promptHistoryEntriesRef.current
              );
              promptHistoryEntriesRef.current = [];
            }
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
              historyPaginationCacheRef.current.invalidate(id);
              const timeline = await loadHistoryHead(id);
              if (
                !isAgentModeMountedRef.current ||
                userIdRef.current !== userId ||
                deletedSessionIdsRef.current.has(id)
              ) {
                recoverPromptHistory(null);
                return;
              }
              const replaced = replaceSessionTimeline(id, timeline, historyTimelineRevision);
              if (!replaced) {
                recoverPromptHistory(activeSessionIdRef.current);
              }
              if (replaced && activeRunsBySessionRef.current[id]) {
                thoughtPhaseTrackerRef.current.seedActiveTimeline(id, timeline);
                observeActiveThoughtPhase(id);
              }
            } catch (historyError) {
              recoverPromptHistory(activeSessionIdRef.current);
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
      applyQueueSnapshot,
      applyRuntimeStatus,
      bumpTimelineRevision,
      clearCompletedUnreadSession,
      deletedSessionIdsRef,
      invalidateThoughtLabelsForSession,
      invalidateThoughtLabelsForTurn,
      historyPaginationCacheRef,
      loadHistoryHead,
      mergeSessionTimelineItem,
      observeLiveThoughtItem,
      observeActiveThoughtPhase,
      promptHistoryReplacementTrackerRef,
      recordActiveRun,
      refreshSessionMcpServers,
      reconcileAgentEventGap,
      replaceSessionTimeline,
      settleFinishedAgentRun,
      resetPromptHistoryNavigation,
      terminalRunIdsRef,
      thoughtPhaseTrackerRef,
      upsertSessionSummary,
      userId
    ]
  );

  const handleAgentLiveChannelFrame = useCallback(
    (frame: AgentLiveChannelFrame) => {
      if (frame.eventType === "snapshotRequired") {
        historyPaginationCacheRef.current.requireSynchronizedReload();
        reconcileAgentEventGap(null);
        return;
      }

      // The closed stream is account-wide. Consume ordering before deciding
      // whether this session is selected, deleted, or has a visible mutation.
      const acceptance = historyPaginationCacheRef.current.acceptEvent(frame);
      if (acceptance === "duplicate" || acceptance === "invalid") return;
      if (acceptance === "gap") {
        reconcileAgentEventGap(frame.sessionId);
        return;
      }
      if (
        frame.eventType !== "sessionDeleted" &&
        deletedSessionIdsRef.current.has(frame.sessionId)
      ) {
        return;
      }

      switch (frame.eventType) {
        case "runStarted":
          // runStarted is lifecycle only. The durable stream publishes a
          // distinct timelineCleared event when the overlay is obsolete.
          runStateGenerationRef.current += 1;
          if (!terminalRunIdsRef.current.has(frame.runId)) {
            clearCompletedUnreadSession(frame.sessionId);
            recordActiveRun(frame.sessionId, frame.runId);
          }
          break;
        case "timelineUpsert":
        case "userFacingError":
          if (mergeSessionTimelineItem(frame.sessionId, frame.item) === "applied") {
            observeLiveThoughtItem(frame.sessionId, frame.item);
          } else {
            setError(
              "Agent live history exceeded its safe cache window. Reconnect to reload this task."
            );
            reconcileAgentEventGap(frame.sessionId);
          }
          break;
        case "timelineCleared": {
          historyPaginationCacheRef.current.clearLiveTimeline(frame.sessionId);
          const invalidatedTurnId = thoughtPhaseTrackerRef.current.resetForHistoryReplacement(
            frame.sessionId
          );
          if (invalidatedTurnId) {
            invalidateThoughtLabelsForTurn(frame.sessionId, invalidatedTurnId);
          } else {
            invalidateThoughtLabelsForSession(frame.sessionId);
          }
          bumpTimelineRevision(frame.sessionId);
          publishHistorySnapshot(frame.sessionId);
          break;
        }
        case "historyReplaced":
          void (async () => {
            const invalidatedTurnId = thoughtPhaseTrackerRef.current.resetForHistoryReplacement(
              frame.sessionId
            );
            if (invalidatedTurnId) {
              invalidateThoughtLabelsForTurn(frame.sessionId, invalidatedTurnId);
            } else {
              invalidateThoughtLabelsForSession(frame.sessionId);
            }
            const historyTimelineRevision = bumpTimelineRevision(frame.sessionId);
            try {
              historyPaginationCacheRef.current.invalidate(frame.sessionId);
              const timeline = await loadHistoryHead(frame.sessionId);
              if (
                !isAgentModeMountedRef.current ||
                userIdRef.current !== userId ||
                deletedSessionIdsRef.current.has(frame.sessionId)
              ) {
                return;
              }
              const replaced = replaceSessionTimeline(
                frame.sessionId,
                timeline,
                historyTimelineRevision
              );
              if (replaced && activeRunsBySessionRef.current[frame.sessionId]) {
                thoughtPhaseTrackerRef.current.seedActiveTimeline(frame.sessionId, timeline);
                observeActiveThoughtPhase(frame.sessionId);
              }
            } catch (historyError) {
              if (
                isAgentModeMountedRef.current &&
                userIdRef.current === userId &&
                activeSessionIdRef.current === frame.sessionId
              ) {
                setError(errorMessage(historyError));
              }
            }
          })();
          break;
        case "cursorAdvanced":
          // Ordering-only storage acknowledgement; no presentation mutation.
          break;
        case "sessionUpdated":
          upsertSessionSummary(frame.session);
          break;
        case "runFinished":
          settleFinishedAgentRun(frame.sessionId, frame.runId, frame.terminal);
          break;
        case "sessionDeleted":
          removeSessionFromState(frame.sessionId);
          break;
        default: {
          const exhaustiveFrame: never = frame;
          return exhaustiveFrame;
        }
      }
    },
    [
      bumpTimelineRevision,
      clearCompletedUnreadSession,
      deletedSessionIdsRef,
      historyPaginationCacheRef,
      invalidateThoughtLabelsForSession,
      invalidateThoughtLabelsForTurn,
      loadHistoryHead,
      mergeSessionTimelineItem,
      observeActiveThoughtPhase,
      observeLiveThoughtItem,
      publishHistorySnapshot,
      reconcileAgentEventGap,
      recordActiveRun,
      removeSessionFromState,
      replaceSessionTimeline,
      settleFinishedAgentRun,
      terminalRunIdsRef,
      thoughtPhaseTrackerRef,
      upsertSessionSummary,
      userId
    ]
  );
  liveChannelHandlerRef.current = handleAgentLiveChannelFrame;

  useEffect(() => {
    if (agentRuntimeService.target.kind === "remote") return;
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    void awaitAgentAuthUser(userId)
      .then(async () => {
        return await agentRuntimeService.listenToEvents(userId, (event) => {
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
  }, [agentRuntimeService, handleAgentEvent, userId]);

  useEffect(() => {
    if (agentRuntimeService.target.kind !== "remote") return;
    const resume = () => {
      if (document.visibilityState !== "visible" || liveConnectionsRef.current.hasPending) {
        return;
      }
      const cursor = historyPaginationCacheRef.current.eventCursor();
      const sessionId = activeSessionIdRef.current;
      if (!cursor || !sessionId) return;
      const resumeOwnerKey = agentOwnerKey;
      void resumeAgentLiveConnection().catch((resumeError) => {
        if (
          resumeOwnerKey === agentOwnerKeyRef.current &&
          isAgentModeMountedRef.current &&
          userIdRef.current === userId
        ) {
          historyPaginationCacheRef.current.requireSynchronizedReload();
          reconcileAgentEventGap(sessionId);
          if (!isAgentLiveSnapshotRequiredError(resumeError)) {
            setError(errorMessage(resumeError));
          }
        }
      });
    };
    document.addEventListener("visibilitychange", resume);
    window.addEventListener("online", resume);
    return () => {
      document.removeEventListener("visibilitychange", resume);
      window.removeEventListener("online", resume);
    };
  }, [
    agentOwnerKey,
    agentRuntimeService.target.kind,
    historyPaginationCacheRef,
    liveConnectionsRef,
    reconcileAgentEventGap,
    resumeAgentLiveConnection,
    userId
  ]);

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
  const handlePromptSessionRename = useCallback(
    (session: AgentSessionSummary, menuTrigger: HTMLButtonElement) => {
      renameTaskMenuTriggerRef.current = menuTrigger;
      setSessionToRename(session);
    },
    []
  );
  const handleReturnRenameTaskFocus = useCallback((focusVisible: boolean) => {
    const menuTrigger = renameTaskMenuTriggerRef.current;
    renameTaskMenuTriggerRef.current = null;
    if (!menuTrigger?.isConnected) return;
    if (focusVisible) delete menuTrigger.dataset.silentFocus;
    else menuTrigger.dataset.silentFocus = "true";
    menuTrigger.focus({ preventScroll: true });
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
      if (!localProjectFolderActionsAvailable) return;
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
    [localProjectFolderActionsAvailable, showNotification]
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

  if (!isTauriDesktop() && agentRuntimeService.target.kind !== "remote") {
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
    <ResizableSidebarLayout
      isCompactLayout={isCompactLayout}
      isOpen={isSidebarOpen}
      mode="agent"
      onOpenChange={setIsSidebarOpen}
      onTransitionChange={setIsSidebarTransitioning}
      userId={userId}
      sidebar={
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
              hasMoreSessions={hasMoreSessions}
              isLoadingOlderSessions={isLoadingOlderSessions}
              localProjectFolderActionsAvailable={localProjectFolderActionsAvailable}
              onChooseProjectRoot={chooseProjectRoot}
              onCreateSession={handleCreateSessionForProject}
              onProjectDisclosureToggle={handleToggleProjectDisclosure}
              onProjectOrderChange={saveProjectRootOrder}
              onProjectRename={handlePromptProjectRename}
              onProjectRemove={handlePromptProjectRemoval}
              onRevealProjectRoot={handleRevealProjectRoot}
              onSessionDelete={setSessionToDelete}
              onSessionRename={handlePromptSessionRename}
              onSessionSelect={handleSelectSession}
              onLoadOlderSessions={() => void loadOlderSessions()}
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
      }
    >
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

      {sessionToRename ? (
        <RenameAgentTaskDialog
          key={sessionToRename.id}
          open
          currentTitle={sessionTitle(sessionToRename)}
          onOpenChange={(open) => {
            if (!open) setSessionToRename(null);
          }}
          onRename={(title) => renameAgentSession(sessionToRename.id, title)}
          onReturnFocus={handleReturnRenameTaskFocus}
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
        {!isSidebarOpen && !isSidebarTransitioning && (
          <div className="fixed left-4 top-[9.5px] z-20 flex items-center gap-1.5">
            <SidebarToggle onToggle={toggleSidebar} agentStatus={agentSidebarStatus} />
            <MapleWordmark
              className="h-4 w-auto animate-in fade-in-0 slide-in-from-left-1 duration-300"
              aria-hidden
            />
          </div>
        )}

        {activeSessionId ? (
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
            tabIndex={0}
          >
            <div
              className={cn(
                "mx-auto w-full",
                activeSessionId
                  ? "max-w-4xl p-4 md:p-6 landscape-short:p-2"
                  : isAgentFullscreen
                    ? "flex min-h-full max-w-6xl flex-col p-4 md:p-6 landscape-short:p-2"
                    : "flex min-h-full flex-col px-4"
              )}
            >
              {!activeSessionId ? (
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
                  localProjectFolderActionsAvailable={localProjectFolderActionsAvailable}
                  mcpServers={composerMcpServers}
                  mode={mode}
                  model={model}
                  projectRoot={projectRoot}
                  recentRoots={displayProjectRoots}
                  isExpanded={isAgentFullscreen}
                  textareaRef={agentComposerTextareaRef}
                  onCancelPrompt={cancelPrompt}
                  onChooseProjectRoot={chooseProjectRoot}
                  onInputChange={handleAgentInputChange}
                  onInputPointerDown={resetPromptHistoryNavigation}
                  onKeyDown={handleKeyDown}
                  onBeforeInput={handleBeforeInput}
                  onManageMcpServers={handleManageMcpServers}
                  onMcpToggle={toggleMcpServer}
                  onModeChange={selectMode}
                  onModelChange={selectModel}
                  onProjectRootChange={selectProjectRoot}
                  onSendMessage={handleSendMessage}
                  onToggleExpanded={handleToggleAgentFullscreen}
                  queuedMessages={queuedMessages}
                  editingQueueId={editingQueueId}
                  onCancelQueuedMessage={cancelQueuedMessage}
                  onEditQueuedMessage={editQueuedMessage}
                  onDiscardQueuedMessageEdit={discardQueueEdit}
                />
              ) : (
                <>
                  <div ref={historyTopSentinelRef} className="h-px" aria-hidden="true" />
                  {isLoadingOlderHistory ? (
                    <div
                      className="flex h-8 items-center justify-center text-muted-foreground"
                      role="status"
                      aria-label="Loading older task history"
                    >
                      <Loader2 className="h-4 w-4 animate-spin" />
                    </div>
                  ) : null}
                  <AgentTimeline
                    items={timelineItems}
                    isResponsePending={isSending}
                    isRunActive={Boolean(activeRunId) && !isSubmitting}
                    generatedThoughtLabels={generatedThoughtLabels}
                    sessionId={activeSessionId}
                    onPermissionDecision={respondToPermission}
                  />
                  <div ref={historyBottomCompensationRef} className="h-0" aria-hidden="true" />
                </>
              )}
            </div>
          </div>

          {activeSessionId ? (
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
                  localProjectFolderActionsAvailable={localProjectFolderActionsAvailable}
                  mcpServers={composerMcpServers}
                  mode={mode}
                  model={model}
                  projectRoot={projectRoot}
                  recentRoots={displayProjectRoots}
                  textareaRef={agentComposerTextareaRef}
                  onCancelPrompt={cancelPrompt}
                  onChooseProjectRoot={chooseProjectRoot}
                  onInputChange={handleAgentInputChange}
                  onInputPointerDown={resetPromptHistoryNavigation}
                  onKeyDown={handleKeyDown}
                  onBeforeInput={handleBeforeInput}
                  onManageMcpServers={handleManageMcpServers}
                  onMcpToggle={toggleMcpServer}
                  onModeChange={selectMode}
                  onModelChange={selectModel}
                  onProjectRootChange={selectProjectRoot}
                  onSendMessage={handleSendMessage}
                  queuedMessages={queuedMessages}
                  editingQueueId={editingQueueId}
                  onCancelQueuedMessage={cancelQueuedMessage}
                  onEditQueuedMessage={editQueuedMessage}
                  onDiscardQueuedMessageEdit={discardQueueEdit}
                />
                <p className="mb-2 mt-1 text-center text-[10px] text-muted-foreground/50 landscape-short:mb-1">
                  AI can make mistakes. Check important info.
                </p>
              </div>
            </div>
          ) : null}
        </section>
      </div>
    </ResizableSidebarLayout>
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
  hasMoreSessions: boolean;
  isLoadingOlderSessions: boolean;
  localProjectFolderActionsAvailable: boolean;
  onChooseProjectRoot: () => void;
  onCreateSession: (projectRoot: string) => void;
  onProjectDisclosureToggle: (path: string) => void;
  onProjectOrderChange: (roots: AgentProjectRootView[]) => void;
  onProjectRename: (root: AgentProjectRootView) => void;
  onProjectRemove: (root: AgentProjectRootView) => void;
  onRevealProjectRoot: (projectRoot: string) => void;
  onSessionDelete: (session: AgentSessionSummary) => void;
  onSessionRename: (session: AgentSessionSummary, menuTrigger: HTMLButtonElement) => void;
  onSessionSelect: (sessionId: string) => void;
  onLoadOlderSessions: () => void;
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
  localProjectFolderActionsAvailable: boolean;
  onDelete: (session: AgentSessionSummary) => void;
  onRename: (session: AgentSessionSummary, menuTrigger: HTMLButtonElement) => void;
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
  localProjectFolderActionsAvailable,
  onDelete,
  onRename,
  onRevealProjectRoot,
  onSelect,
  projectDisplayName,
  rowRef,
  session
}: AgentSidebarTaskRowProps) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [keyboardFocusTarget, setKeyboardFocusTarget] =
    useState<AgentTaskRowKeyboardFocusTarget>(null);
  const [infoCardOpen, setInfoCardOpen] = useState(false);
  const taskSelectionButtonRef = useRef<HTMLButtonElement>(null);
  const menuTriggerRef = useRef<HTMLButtonElement>(null);
  const title = sessionTitle(session);
  const hasVisualStatus = isRunning || isUnreadCompleted;
  const visibleInfoCardOpen = !isTouchLayout && infoCardOpen;
  const interactionPresentation = agentTaskRowInteractionPresentation(
    menuOpen,
    keyboardFocusTarget,
    visibleInfoCardOpen
  );

  useEffect(() => {
    if (isTouchLayout) setInfoCardOpen(false);
  }, [isTouchLayout]);

  const activeSurface = interactionPresentation.emphasizeSurface
    ? isActive
      ? "bg-[hsl(var(--sidebar-row-selected-hover))] ring-1 ring-ring/70"
      : "bg-[hsl(var(--sidebar-row-hover))] ring-1 ring-ring/70"
    : null;
  const taskSelectionButton = (
    <button
      ref={taskSelectionButtonRef}
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
      onFocusCapture={(event) => {
        const focusedElement = event.target as Element;
        if (!isKeyboardFocusTarget(focusedElement)) {
          setKeyboardFocusTarget(null);
          return;
        }
        setKeyboardFocusTarget(
          focusedElement === taskSelectionButtonRef.current ? "selection" : "action"
        );
      }}
      onBlurCapture={(event) => {
        const nextTarget = event.relatedTarget;
        if (
          !(nextTarget instanceof Element) ||
          !event.currentTarget.contains(nextTarget) ||
          !isKeyboardFocusTarget(nextTarget)
        ) {
          setKeyboardFocusTarget(null);
          return;
        }
        setKeyboardFocusTarget(
          nextTarget === taskSelectionButtonRef.current ? "selection" : "action"
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
              onOpenProjectFolder={
                localProjectFolderActionsAvailable
                  ? () => onRevealProjectRoot(session.projectRoot)
                  : undefined
              }
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
            !isTouchLayout && interactionPresentation.revealActions && "opacity-0"
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
          interactionPresentation.revealActions
        )}
      >
        <div
          aria-hidden="true"
          className={cn(
            "pointer-events-none w-5 shrink-0 self-stretch bg-gradient-to-r from-transparent transition-colors",
            isActive
              ? "to-[hsl(var(--sidebar-row-selected))] group-hover/task:to-[hsl(var(--sidebar-row-selected-hover))]"
              : "to-[hsl(var(--sidebar))] group-hover/task:to-[hsl(var(--sidebar-row-hover))]",
            interactionPresentation.emphasizeSurface &&
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
            interactionPresentation.emphasizeSurface &&
              (isActive
                ? "bg-[hsl(var(--sidebar-row-selected-hover))]"
                : "bg-[hsl(var(--sidebar-row-hover))]")
          )}
        >
          <DropdownMenu open={menuOpen} onOpenChange={setMenuOpen}>
            <DropdownMenuTrigger asChild>
              <button
                ref={menuTriggerRef}
                type="button"
                className={cn(AGENT_SIDEBAR_ACTION_BUTTON, isTouchLayout ? "h-9 w-9" : "h-7 w-7")}
                onClick={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                }}
                onBlur={(event) => {
                  delete event.currentTarget.dataset.silentFocus;
                }}
                onKeyDown={(event) => {
                  delete event.currentTarget.dataset.silentFocus;
                  setKeyboardFocusTarget("action");
                }}
                aria-label={`Open task menu for ${title}`}
              >
                <MoreHorizontal className="h-4 w-4" strokeWidth={SIDEBAR_ICON_STROKE} />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" collisionPadding={8} className="max-w-48">
              <DropdownMenuItem
                disabled={actionsLocked}
                onClick={() => {
                  if (menuTriggerRef.current) onRename(session, menuTriggerRef.current);
                }}
              >
                <FilePenLine className="mr-2 h-4 w-4 shrink-0" strokeWidth={SIDEBAR_ICON_STROKE} />
                <span className="min-w-0 whitespace-normal leading-snug">Rename Task</span>
              </DropdownMenuItem>
              <DropdownMenuSeparator />
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
  hasMoreSessions,
  isLoadingOlderSessions,
  localProjectFolderActionsAvailable,
  onChooseProjectRoot,
  onCreateSession,
  onProjectDisclosureToggle,
  onProjectOrderChange,
  onProjectRename,
  onProjectRemove,
  onRevealProjectRoot,
  onSessionDelete,
  onSessionRename,
  onSessionSelect,
  onLoadOlderSessions
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
        {localProjectFolderActionsAvailable ? (
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
        ) : null}
      </div>

      {projectRows.length === 0 ? (
        localProjectFolderActionsAvailable ? (
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
          <p className="text-xs text-muted-foreground/75">
            No projects are available on this host.
          </p>
        )
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
                          onOpenProjectFolder={
                            localProjectFolderActionsAvailable
                              ? () => onRevealProjectRoot(root.path)
                              : undefined
                          }
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
                          {localProjectFolderActionsAvailable ? (
                            <DropdownMenuItem onClick={() => onRevealProjectRoot(root.path)}>
                              <FolderOpen
                                className="mr-2 h-4 w-4 shrink-0"
                                strokeWidth={SIDEBAR_ICON_STROKE}
                              />
                              <span className="whitespace-nowrap">Open Project Folder</span>
                            </DropdownMenuItem>
                          ) : null}
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
                          localProjectFolderActionsAvailable={localProjectFolderActionsAvailable}
                          onDelete={onSessionDelete}
                          onRename={onSessionRename}
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

      {hasMoreSessions ? (
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="mt-2 w-full text-xs text-muted-foreground"
          disabled={disabled || isLoadingOlderSessions}
          onClick={onLoadOlderSessions}
        >
          {isLoadingOlderSessions ? <Loader2 className="mr-2 h-3.5 w-3.5 animate-spin" /> : null}
          {isLoadingOlderSessions ? "Loading older tasks…" : "Load older tasks"}
        </Button>
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
  localProjectFolderActionsAvailable: boolean;
  mcpServers: AgentSessionMcpServer[];
  mode: AgentPermissionMode;
  model: string;
  projectRoot: string;
  recentRoots: AgentProjectRootView[];
  isExpanded?: boolean;
  textareaRef: React.RefObject<HTMLTextAreaElement>;
  onCancelPrompt: () => void;
  onChooseProjectRoot: () => void;
  onInputChange: (value: string) => void;
  onInputPointerDown: () => void;
  onBeforeInput: (event: React.FormEvent<HTMLTextAreaElement>) => void;
  onKeyDown: (event: React.KeyboardEvent<HTMLTextAreaElement>) => void;
  onManageMcpServers: () => void;
  onMcpToggle: (name: string, enabled: boolean) => void;
  onModeChange: (value: AgentPermissionMode) => void;
  onModelChange: (value: string) => void;
  onProjectRootChange: (value: string) => void;
  onSendMessage: () => void;
  onToggleExpanded?: () => void;
  queuedMessages?: AgentDesktopQueueSnapshot["items"];
  editingQueueId?: string | null;
  onCancelQueuedMessage?: (queueId: string) => void;
  onEditQueuedMessage?: (queueId: string) => void;
  onDiscardQueuedMessageEdit?: () => void;
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
  localProjectFolderActionsAvailable,
  mcpServers,
  mode,
  model,
  projectRoot,
  recentRoots,
  isExpanded = false,
  textareaRef,
  onCancelPrompt,
  onChooseProjectRoot,
  onInputChange,
  onInputPointerDown,
  onBeforeInput,
  onKeyDown,
  onManageMcpServers,
  onMcpToggle,
  onModeChange,
  onModelChange,
  onProjectRootChange,
  onSendMessage,
  onToggleExpanded,
  queuedMessages = [],
  editingQueueId = null,
  onCancelQueuedMessage,
  onEditQueuedMessage,
  onDiscardQueuedMessageEdit
}: AgentComposerProps) {
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
  }, [input, isExpanded, textareaRef]);

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
      {queuedMessages.length > 0 ? (
        <div className="flex flex-col gap-1 px-3 pt-2">
          {queuedMessages.map((item) => (
            <div
              key={item.queueId}
              className={cn(
                "flex items-center gap-1 rounded-lg bg-muted/70 px-2 py-1 text-left text-xs text-muted-foreground",
                item.queueId === editingQueueId && "bg-muted text-foreground ring-1 ring-border"
              )}
            >
              <span className="min-w-0 flex-1 truncate" title={item.text}>
                {item.text}
              </span>
              {onEditQueuedMessage ? (
                <button
                  type="button"
                  className="rounded-md p-1 text-muted-foreground transition-colors hover:bg-background hover:text-foreground"
                  onClick={() => onEditQueuedMessage(item.queueId)}
                  aria-label="Edit queued message"
                >
                  <FilePenLine className="h-3.5 w-3.5" />
                </button>
              ) : null}
              {onCancelQueuedMessage ? (
                <button
                  type="button"
                  className="rounded-md p-1 text-muted-foreground transition-colors hover:bg-background hover:text-foreground"
                  onClick={() => onCancelQueuedMessage(item.queueId)}
                  aria-label="Remove queued message"
                >
                  <X className="h-3.5 w-3.5" />
                </button>
              ) : null}
            </div>
          ))}
        </div>
      ) : null}
      <Textarea
        ref={textareaRef}
        id="agent-message"
        value={input}
        onChange={(event) => onInputChange(event.target.value)}
        onPointerDown={onInputPointerDown}
        onBeforeInput={onBeforeInput}
        onKeyDown={onKeyDown}
        disabled={isSendDisabled}
        placeholder={
          editingQueueId
            ? "Edit the queued message, then send to keep its place..."
            : "Ask Maple to work in this folder..."
        }
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

          {localProjectFolderActionsAvailable || rootOptions.length > 0 ? (
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
                {localProjectFolderActionsAvailable ? (
                  <>
                    <SelectItem value={NEW_PROJECT_OPTION_VALUE}>New project…</SelectItem>
                    {rootOptions.length > 0 ? <SelectSeparator /> : null}
                  </>
                ) : null}
                {rootOptions.map((root) => (
                  <SelectItem key={root.path} value={root.path}>
                    {root.displayName}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          ) : (
            <span
              className="flex h-8 items-center gap-1 px-2 text-xs font-medium text-muted-foreground"
              aria-label="No projects are available on this host"
            >
              <FolderOpen className="h-4 w-4 shrink-0" />
              No host projects
            </span>
          )}
        </div>

        <div className="flex shrink-0 items-center self-end gap-1.5 sm:gap-2">
          {editingQueueId && onDiscardQueuedMessageEdit ? (
            <Button
              type="button"
              size="sm"
              variant="ghost"
              className="h-8 px-2 text-xs text-muted-foreground"
              onClick={onDiscardQueuedMessageEdit}
            >
              Discard
            </Button>
          ) : null}
          {agentComposerShowsStop(isSending) ? (
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
          ) : null}
          <button
            type="button"
            className={cn(
              "flex h-8 w-8 items-center justify-center rounded-full bg-gradient-to-b from-[hsl(var(--maple-primary))] to-[hsl(var(--maple-primary-strong))] text-[hsl(var(--maple-on-primary))]/90 transition-all duration-200 ease-out active:scale-[0.95] disabled:pointer-events-none disabled:opacity-40",
              onToggleExpanded && !isExpanded && "sm:h-9 sm:w-9"
            )}
            onClick={onSendMessage}
            disabled={
              !agentComposerCanSend({
                text: input,
                isSendDisabled,
                projectRoot,
                hasQueuedMessages: queuedMessages.length > 0,
                hasActiveRun: isSending
              })
            }
            aria-label="Send agent message"
          >
            {isStarting ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <ArrowUp className="h-4 w-4" />
            )}
          </button>
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
  const assistantTurnKeyRegistryRef = useLazyRef(() => new AgentAssistantTurnKeyRegistry());
  const visibleItems = coalesceAdjacentThinkingItems(items).filter(isRenderableAgentTimelineItem);
  const turns = groupAgentTimelineItems(visibleItems);
  const assistantTurnKeys = assistantTurnKeyRegistryRef.current.resolve(sessionId, turns);
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
              key={agentUserTurnReactKey(turn.id)}
              historyAnchorIds={turn.item.id}
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
            key={assistantTurnKeys.get(turn)}
            historyAnchorIds={turn.items.flatMap(agentTimelineHistoryAnchorIds).join(" ")}
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
