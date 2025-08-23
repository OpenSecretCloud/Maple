import { useState, useEffect } from "react";
import { useLocalState } from "@/state/useLocalState";

const MIN_CHATS = 3;

/**
 * Hook to track the total number of chats a user has created
 * Returns the count and a function to check if the user has made 3 or more chats
 */
export function useChatCount() {
  // Start with null to indicate loading state - prevents initial render of InfoCard
  const [chatCount, setChatCount] = useState<number | null>(null);
  const { fetchOrCreateHistoryList } = useLocalState();

  useEffect(() => {
    let cancelled = false;
    async function loadChatCount() {
      try {
        const historyList = await fetchOrCreateHistoryList();
        if (!cancelled) {
          setChatCount(historyList.length);
        }
      } catch (error: unknown) {
        if (!cancelled) {
          if (error instanceof Error) {
            console.error("Error loading chat count:", error.message);
          } else {
            console.error("Error loading chat count:", error);
          }
          // Preserve previous count to avoid flicker, only set 0 if no previous value
          setChatCount((prev) => (prev !== null ? prev : 0));
        }
      }
    }

    loadChatCount();

    // Set up a periodic check to detect new chats
    const interval = setInterval(() => {
      // Skip polling when the tab is not visible to reduce churn
      if (typeof document !== "undefined" && document.visibilityState === "hidden") return;
      loadChatCount();
    }, 1000);

    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [fetchOrCreateHistoryList]);

  const hasMinChats = chatCount !== null && chatCount >= MIN_CHATS;
  const isLoading = chatCount === null;

  return {
    chatCount: chatCount ?? 0,
    hasMinChats,
    isLoading
  };
}
