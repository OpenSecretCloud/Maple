import {
  createChatDraftKey,
  createConversationChatKey,
  type ChatRuntimeKey,
  type DraftChatRuntimeKey
} from "./chatRuntimeStore";

const CHAT_DRAFT_RUNTIME_HISTORY_STATE_KEY = "mapleChatDraftRuntimeKey";

export type NewChatNavigationDetail = Readonly<{
  projectId?: string | null;
  draftRuntimeKey?: DraftChatRuntimeKey;
}>;

export type ChatHistoryLocation = Readonly<{
  pathname: string;
  search: string;
  hash: string;
}>;

function historyStateRecord(state: unknown): Record<string, unknown> {
  return state !== null && typeof state === "object" && !Array.isArray(state)
    ? (state as Record<string, unknown>)
    : {};
}

export function isDraftChatRuntimeKey(value: unknown): value is DraftChatRuntimeKey {
  return typeof value === "string" && value.startsWith("draft:") && value.length > "draft:".length;
}

export function draftRuntimeKeyFromHistoryState(state: unknown): DraftChatRuntimeKey | null {
  const value = historyStateRecord(state)[CHAT_DRAFT_RUNTIME_HISTORY_STATE_KEY];
  return isDraftChatRuntimeKey(value) ? value : null;
}

export function historyStateWithDraftRuntimeKey(
  state: unknown,
  draftRuntimeKey: DraftChatRuntimeKey
): Record<string, unknown> {
  return {
    ...historyStateRecord(state),
    [CHAT_DRAFT_RUNTIME_HISTORY_STATE_KEY]: draftRuntimeKey
  };
}

export function runtimeKeyForChatLocation(
  conversationId: string | undefined,
  historyState: unknown,
  createDraftKey: () => DraftChatRuntimeKey = createChatDraftKey
): ChatRuntimeKey {
  if (conversationId) return createConversationChatKey(conversationId);
  return draftRuntimeKeyFromHistoryState(historyState) ?? createDraftKey();
}

export function conversationIdFromChatRuntimeKey(key: ChatRuntimeKey): string | undefined {
  const prefix = "conversation:";
  return key.startsWith(prefix) && key.length > prefix.length
    ? key.slice(prefix.length)
    : undefined;
}

export function canonicalConversationHistoryHref(
  location: ChatHistoryLocation,
  conversationId: string
): string {
  const params = new URLSearchParams(location.search);
  params.delete("project_id");
  params.set("conversation_id", conversationId);
  const search = params.toString();
  return `${location.pathname}${search ? `?${search}` : ""}${location.hash}`;
}

export function shouldProjectMigratedConversation(
  isUnifiedChatMounted: boolean,
  sourceWasSelected: boolean,
  destinationWasSelected: boolean
): boolean {
  return isUnifiedChatMounted && (sourceWasSelected || destinationWasSelected);
}

export function createFreshChatHistoryEntry(draftId?: string): Readonly<{
  draftRuntimeKey: DraftChatRuntimeKey;
  historyState: Record<string, unknown>;
}> {
  const draftRuntimeKey = createChatDraftKey(draftId);
  return {
    draftRuntimeKey,
    historyState: historyStateWithDraftRuntimeKey({}, draftRuntimeKey)
  };
}

export function pushFreshChatHistoryEntry(
  history: Pick<History, "pushState">,
  url: string,
  projectId: string | null,
  draftId?: string
): NewChatNavigationDetail {
  const freshChat = createFreshChatHistoryEntry(draftId);
  history.pushState(freshChat.historyState, "", url);
  return { projectId, draftRuntimeKey: freshChat.draftRuntimeKey };
}
