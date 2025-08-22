import { useState, useEffect } from "react";
import { useLocalState } from "@/state/useLocalState";

/**
 * Hook to track the total number of chats a user has created
 * Returns the count and a function to check if the user has made 3 or more chats
 */
export function useChatCount() {
  const [chatCount, setChatCount] = useState<number>(0);
  const { fetchOrCreateHistoryList } = useLocalState();

  useEffect(() => {
    async function loadChatCount() {
      try {
        const historyList = await fetchOrCreateHistoryList();
        setChatCount(historyList.length);
      } catch (error) {
        console.error("Error loading chat count:", error);
        setChatCount(0);
      }
    }

    loadChatCount();

    // Set up a periodic check to detect new chats
    const interval = setInterval(loadChatCount, 1000);

    return () => clearInterval(interval);
  }, [fetchOrCreateHistoryList]);

  const hasMinChats = chatCount >= 3;

  return {
    chatCount,
    hasMinChats
  };
}
