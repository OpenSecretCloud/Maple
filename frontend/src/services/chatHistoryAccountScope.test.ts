import { describe, expect, test } from "bun:test";
import { QueryClient } from "@tanstack/react-query";
import {
  INITIAL_CHAT_HISTORY_RETRY_COUNT,
  conversationHistoryQueryKey,
  initialChatHistoryPage,
  shouldLoadConversationHistory
} from "./chatHistoryAccountScope";

describe("chat history account scope", () => {
  test("keeps cached conversation pages isolated by authenticated user", () => {
    const queryClient = new QueryClient();
    const userAKey = conversationHistoryQueryKey("user-a");
    const userBKey = conversationHistoryQueryKey("user-b");
    const cachedConversations = [{ id: "conversation-a" }];

    queryClient.setQueryData(userAKey, cachedConversations);

    expect(queryClient.getQueryData<typeof cachedConversations>(userAKey)).toEqual(
      cachedConversations
    );
    expect(queryClient.getQueryData<typeof cachedConversations>(userBKey)).toBeUndefined();
    expect(conversationHistoryQueryKey(undefined)).toEqual(["conversations", null]);
  });

  test("loads only for a concrete authenticated user and retries one transient failure", () => {
    expect(shouldLoadConversationHistory(undefined)).toBe(false);
    expect(shouldLoadConversationHistory("")).toBe(false);
    expect(shouldLoadConversationHistory("user-a")).toBe(true);
    expect(INITIAL_CHAT_HISTORY_RETRY_COUNT).toBe(1);
  });

  test("recovers a first authenticated load after one transient failure", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } }
    });
    let attempts = 0;

    const conversations = await queryClient.fetchQuery({
      queryKey: conversationHistoryQueryKey("first-login-user"),
      queryFn: async () => {
        attempts += 1;
        if (attempts === 1) throw new Error("authentication still settling");
        return [{ id: "existing-chat" }];
      },
      retry: INITIAL_CHAT_HISTORY_RETRY_COUNT,
      retryDelay: 0
    });

    expect(attempts).toBe(2);
    expect(initialChatHistoryPage(conversations).conversations).toEqual([{ id: "existing-chat" }]);
  });

  test("hydrates local pagination from returned or cached query data", () => {
    const fullPage = Array.from({ length: 20 }, (_, index) => ({ id: `chat-${index}` }));
    expect(initialChatHistoryPage(fullPage)).toEqual({
      conversations: fullPage,
      oldestConversationId: "chat-19",
      hasMoreConversations: true
    });
    expect(initialChatHistoryPage([])).toEqual({
      conversations: [],
      oldestConversationId: undefined,
      hasMoreConversations: false
    });
  });
});
