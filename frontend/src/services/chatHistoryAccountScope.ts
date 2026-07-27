export const INITIAL_CHAT_HISTORY_PAGE_SIZE = 20;
export const INITIAL_CHAT_HISTORY_RETRY_COUNT = 1;

export function conversationHistoryQueryKey(userId: string | undefined) {
  return ["conversations", userId ?? null] as const;
}

export function shouldLoadConversationHistory(userId: string | undefined): userId is string {
  return typeof userId === "string" && userId.length > 0;
}

export type InitialChatHistoryPage<TConversation> = Readonly<{
  conversations: readonly TConversation[];
  oldestConversationId: string | undefined;
  hasMoreConversations: boolean;
}>;

export function initialChatHistoryPage<TConversation extends { id: string }>(
  conversations: readonly TConversation[]
): InitialChatHistoryPage<TConversation> {
  return {
    conversations: [...conversations],
    oldestConversationId: conversations.at(-1)?.id,
    hasMoreConversations: conversations.length === INITIAL_CHAT_HISTORY_PAGE_SIZE
  };
}
