import { useState, useMemo, useCallback, useEffect, useRef, useContext } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  MoreHorizontal,
  Trash,
  Pencil,
  ChevronDown,
  ChevronRight,
  CheckSquare,
  RefreshCw
} from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger
} from "@/components/ui/dropdown-menu";
import { Checkbox } from "@/components/ui/checkbox";
import { RenameChatDialog } from "@/components/RenameChatDialog";
import { DeleteChatDialog } from "@/components/DeleteChatDialog";
import { BulkDeleteDialog } from "@/components/BulkDeleteDialog";
import { useOpenAI } from "@/ai/useOpenAi";
import { useOpenSecret } from "@opensecret/react";
import { useRouter } from "@tanstack/react-router";
import { LocalStateContext } from "@/state/LocalStateContext";

interface ChatHistoryListProps {
  currentChatId?: string;
  searchQuery?: string;
  isMobile?: boolean;
  isSelectionMode?: boolean;
  onExitSelectionMode?: () => void;
  selectedIds: Set<string>;
  onSelectionChange: (ids: Set<string>) => void;
  containerRef?: React.RefObject<HTMLElement>;
}

interface Conversation {
  id: string;
  object: "conversation";
  created_at: number;
  metadata?: {
    title?: string;
    [key: string]: unknown;
  };
}

interface ArchivedChat {
  id: string;
  title: string;
  updated_at: number;
  created_at: number;
}

export function ChatHistoryList({
  currentChatId,
  searchQuery = "",
  isMobile = false,
  isSelectionMode = false,
  onExitSelectionMode,
  selectedIds,
  onSelectionChange,
  containerRef
}: ChatHistoryListProps) {
  const openai = useOpenAI();
  const opensecret = useOpenSecret();
  const router = useRouter();
  const queryClient = useQueryClient();
  const localState = useContext(LocalStateContext);
  const [isRenameDialogOpen, setIsRenameDialogOpen] = useState(false);
  const [isDeleteDialogOpen, setIsDeleteDialogOpen] = useState(false);
  const [isBulkDeleteDialogOpen, setIsBulkDeleteDialogOpen] = useState(false);
  const [isBulkDeleting, setIsBulkDeleting] = useState(false);
  const [selectedChat, setSelectedChat] = useState<{ id: string; title: string } | null>(null);
  const [isArchivedExpanded, setIsArchivedExpanded] = useState(false);
  const longPressTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Pagination states
  const [oldestConversationId, setOldestConversationId] = useState<string | undefined>();
  const [hasMoreConversations, setHasMoreConversations] = useState(false);
  const [isLoadingMore, setIsLoadingMore] = useState(false);
  const lastConversationRef = useRef<HTMLDivElement>(null);
  const [conversations, setConversations] = useState<Conversation[]>([]);

  // Pull-to-refresh states
  const [isPullRefreshing, setIsPullRefreshing] = useState(false);
  const [pullDistance, setPullDistance] = useState(0);
  const pullStartY = useRef(0);
  const isPulling = useRef(false);
  const pullDistanceRef = useRef(0);
  const isRefreshingRef = useRef(false);

  // Fetch initial conversations from API using the OpenSecret SDK
  const { isPending, error } = useQuery({
    queryKey: ["conversations"],
    queryFn: async () => {
      if (!opensecret) return [];

      try {
        // Load initial 20 conversations (newest first by default)
        const response = await opensecret.listConversations({
          limit: 20
        });

        const loadedConversations = response.data || [];

        // Set initial state only on first load
        setConversations(loadedConversations);

        // Set pagination state
        if (loadedConversations.length > 0) {
          // Last conversation in the array is the oldest (for pagination)
          const oldestId = loadedConversations[loadedConversations.length - 1].id;
          setOldestConversationId(oldestId);
          // If we got a full page, there might be more
          setHasMoreConversations(loadedConversations.length === 20);
        } else {
          setOldestConversationId(undefined);
          setHasMoreConversations(false);
        }

        return loadedConversations;
      } catch (error) {
        console.error("Failed to load conversations:", error);
        return [];
      }
    },
    enabled: !!opensecret
    // No refetchInterval - we'll use manual polling instead
  });

  // Smart polling function - only fetches NEW conversations using the same pattern as UnifiedChat
  const pollForUpdates = useCallback(async () => {
    if (!opensecret) return;

    try {
      // Fetch latest conversations to detect new ones or metadata changes
      // Default order is desc (newest first), fetches up to 20 conversations
      const response = await opensecret.listConversations({
        limit: 20 // Fetch up to 20 to catch any new conversations
      });

      const newConversations = response.data || [];

      if (newConversations.length > 0) {
        // Merge new conversations with deduplication and metadata updates
        setConversations((prev) => {
          const newConversationsMap = new Map(newConversations.map((c) => [c.id, c]));
          let hasChanges = false;

          // First, update existing conversations if their metadata changed (e.g., title)
          const updatedConversations = prev.map((existingConv) => {
            const newVersion = newConversationsMap.get(existingConv.id);
            if (newVersion) {
              // Remove from map so we don't add it again
              newConversationsMap.delete(existingConv.id);
              // Check if title or other metadata changed
              if (existingConv.metadata?.title !== newVersion.metadata?.title) {
                hasChanges = true;
                return newVersion;
              }
            }
            return existingConv;
          });

          // Add any remaining new conversations (ones we haven't seen before)
          const trulyNewConversations = Array.from(newConversationsMap.values());

          if (!hasChanges && trulyNewConversations.length === 0) {
            return prev;
          }

          // Conversations come in desc order (newest first), so no need to reverse
          // Prepend new conversations to the beginning (newest first in our list)
          return [...trulyNewConversations, ...updatedConversations];
        });
      }
    } catch (error) {
      console.error("Polling error:", error);
      // Fail silently - don't disrupt the UI
    }
  }, [opensecret]);

  // Pull-to-refresh handler
  const handleRefresh = useCallback(async () => {
    isRefreshingRef.current = true;
    setIsPullRefreshing(true);
    try {
      await pollForUpdates();
    } catch (error) {
      console.error("Refresh failed:", error);
    } finally {
      setTimeout(() => {
        setIsPullRefreshing(false);
        setPullDistance(0);
        isRefreshingRef.current = false;
      }, 300);
    }
  }, [pollForUpdates]);

  // Pull-to-refresh event handlers - unified for all platforms (touch + mouse drag only)
  useEffect(() => {
    const container = containerRef?.current;
    if (!container) return;

    const handleTouchStart = (e: TouchEvent) => {
      if (container.scrollTop === 0 && !isRefreshingRef.current) {
        if (longPressTimerRef.current) {
          clearTimeout(longPressTimerRef.current);
          longPressTimerRef.current = null;
        }
        pullStartY.current = e.touches[0].clientY;
        isPulling.current = true;
      }
    };

    const handleTouchMove = (e: TouchEvent) => {
      if (!isPulling.current || isRefreshingRef.current) return;

      const currentY = e.touches[0].clientY;
      const distance = currentY - pullStartY.current;

      if (distance > 0 && container.scrollTop === 0) {
        e.preventDefault();
        if (longPressTimerRef.current) {
          clearTimeout(longPressTimerRef.current);
          longPressTimerRef.current = null;
        }
        const resistanceFactor = 0.4;
        const adjustedDistance = Math.min(distance * resistanceFactor, 80);
        pullDistanceRef.current = adjustedDistance;
        setPullDistance(adjustedDistance);
      }
    };

    const handleTouchEnd = () => {
      if (!isPulling.current) return;
      isPulling.current = false;

      if (pullDistanceRef.current > 60) {
        handleRefresh();
      } else {
        setPullDistance(0);
      }
      pullDistanceRef.current = 0;
    };

    const handleMouseDown = (e: MouseEvent) => {
      if (e.button !== 0) return;
      if (container.scrollTop === 0 && !isRefreshingRef.current) {
        const target = e.target as HTMLElement;
        if (target.closest('button, a, input, [role="menuitem"]')) return;
        if (longPressTimerRef.current) {
          clearTimeout(longPressTimerRef.current);
          longPressTimerRef.current = null;
        }
        pullStartY.current = e.clientY;
        isPulling.current = true;
      }
    };

    const handleMouseMove = (e: MouseEvent) => {
      if (!isPulling.current || isRefreshingRef.current) return;

      const currentY = e.clientY;
      const distance = currentY - pullStartY.current;

      if (distance > 0 && container.scrollTop === 0) {
        e.preventDefault();
        if (longPressTimerRef.current) {
          clearTimeout(longPressTimerRef.current);
          longPressTimerRef.current = null;
        }
        const resistanceFactor = 0.4;
        const adjustedDistance = Math.min(distance * resistanceFactor, 80);
        pullDistanceRef.current = adjustedDistance;
        setPullDistance(adjustedDistance);
      }
    };

    const handleMouseUp = () => {
      if (!isPulling.current) return;
      isPulling.current = false;

      if (pullDistanceRef.current > 60) {
        handleRefresh();
      } else {
        setPullDistance(0);
      }
      pullDistanceRef.current = 0;
    };

    // Touch events for mobile
    container.addEventListener("touchstart", handleTouchStart, { passive: true });
    container.addEventListener("touchmove", handleTouchMove, { passive: false });
    container.addEventListener("touchend", handleTouchEnd);

    // Mouse events for desktop (click and drag)
    container.addEventListener("mousedown", handleMouseDown);
    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);

    return () => {
      container.removeEventListener("touchstart", handleTouchStart);
      container.removeEventListener("touchmove", handleTouchMove);
      container.removeEventListener("touchend", handleTouchEnd);
      container.removeEventListener("mousedown", handleMouseDown);
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
    };
  }, [containerRef, handleRefresh]);

  // Set up polling every 60 seconds
  useEffect(() => {
    if (!opensecret || conversations.length === 0) return;

    const intervalId = setInterval(() => {
      pollForUpdates();
    }, 60000); // Poll every 60 seconds

    return () => {
      clearInterval(intervalId);
    };
  }, [opensecret, pollForUpdates, conversations.length]);

  // Load more older conversations for pagination
  const loadMoreConversations = useCallback(async () => {
    if (!opensecret || !oldestConversationId || isLoadingMore) return;

    setIsLoadingMore(true);

    try {
      // Fetch next 10 older conversations using the oldest conversation ID we have
      const response = await opensecret.listConversations({
        limit: 10,
        after: oldestConversationId
      });

      const olderConversations = response.data || [];

      if (olderConversations.length > 0) {
        // Append older conversations to the end of existing conversations
        setConversations((prev) => [...prev, ...olderConversations]);

        // Update pagination state
        const newOldestId = olderConversations[olderConversations.length - 1].id;
        setOldestConversationId(newOldestId);
        setHasMoreConversations(olderConversations.length === 10);
      } else {
        // No more conversations to load
        setHasMoreConversations(false);
      }
    } catch (error) {
      console.error("Failed to load more conversations:", error);
    } finally {
      setIsLoadingMore(false);
    }
  }, [opensecret, oldestConversationId, isLoadingMore]);

  // Set up IntersectionObserver for loading more conversations
  useEffect(() => {
    if (!lastConversationRef.current || !hasMoreConversations) return;

    const observer = new IntersectionObserver(
      (entries) => {
        // When the last conversation comes into view, load more
        if (entries[0].isIntersecting && hasMoreConversations && !isLoadingMore) {
          loadMoreConversations();
        }
      },
      {
        rootMargin: "100px", // Start loading a bit before reaching the bottom
        threshold: 0.1
      }
    );

    observer.observe(lastConversationRef.current);

    return () => {
      observer.disconnect();
    };
  }, [hasMoreConversations, isLoadingMore, loadMoreConversations]);

  // Fetch archived chats from KV store
  const { data: archivedChats } = useQuery({
    queryKey: ["archivedChats"],
    queryFn: async () => {
      if (!opensecret?.get) return [];

      try {
        const historyListStr = await opensecret.get("history_list");
        if (!historyListStr) return [];

        const historyList = JSON.parse(historyListStr) as ArchivedChat[];
        if (!Array.isArray(historyList)) return [];

        // Sort by updated_at descending (most recent first)
        return historyList.sort((a, b) => (b.updated_at || 0) - (a.updated_at || 0));
      } catch (error) {
        console.error("Error loading archived chats:", error);
        return [];
      }
    },
    enabled: !!opensecret?.get,
    retry: false
  });

  // Filter conversations based on search query
  const filteredConversations = useMemo(() => {
    if (!conversations) return [];
    if (!searchQuery.trim()) return conversations;

    const normalizedQuery = searchQuery.trim().toLowerCase();
    return conversations.filter((conv: Conversation) => {
      const title = conv.metadata?.title || "Untitled Chat";
      return title.toLowerCase().includes(normalizedQuery);
    });
  }, [conversations, searchQuery]);

  // Filter archived chats based on search query
  const filteredArchivedChats = useMemo(() => {
    if (!archivedChats) return [];
    if (!searchQuery.trim()) return archivedChats;

    const normalizedQuery = searchQuery.trim().toLowerCase();
    return archivedChats.filter((chat) => chat.title.toLowerCase().includes(normalizedQuery));
  }, [archivedChats, searchQuery]);

  // Auto-expand archived section when searching with results
  useEffect(() => {
    if (searchQuery.trim() && filteredArchivedChats.length > 0) {
      setIsArchivedExpanded(true);
    }
  }, [searchQuery, filteredArchivedChats.length]);

  // Handle conversation deletion via API
  const handleDeleteConversation = useCallback(
    async (conversationId: string) => {
      // Check if it's an archived chat
      const isArchived = archivedChats?.some((chat) => chat.id === conversationId);

      if (isArchived) {
        // Handle archived chat deletion
        if (!localState?.deleteChat) return;
        try {
          await localState.deleteChat(conversationId);
          // Refresh archived chats list
          queryClient.invalidateQueries({ queryKey: ["archivedChats"] });

          // If deleting the current chat, navigate to home
          if (conversationId === currentChatId) {
            router.navigate({ to: "/" });
          }
        } catch (error) {
          console.error("Error deleting archived chat:", error);
        }
      } else {
        // Handle API conversation deletion
        if (!openai) return;
        try {
          await openai.conversations.delete(conversationId);

          // Remove from local state immediately
          setConversations((prev) => prev.filter((conv) => conv.id !== conversationId));

          // If deleting the current conversation, navigate to home
          if (conversationId === currentChatId) {
            // Clear URL and start fresh
            const params = new URLSearchParams(window.location.search);
            params.delete("conversation_id");
            window.history.replaceState({}, "", params.toString() ? `/?${params}` : "/");

            // Dispatch event to clear UnifiedChat
            window.dispatchEvent(new Event("newchat"));
          }
        } catch (error) {
          console.error("Error deleting conversation:", error);
        }
      }
    },
    [openai, currentChatId, archivedChats, localState, queryClient, router]
  );

  const MAX_SELECTION = 20;

  // Toggle selection of a single chat
  const toggleSelection = useCallback(
    (chatId: string) => {
      const newSelection = new Set(selectedIds);
      if (newSelection.has(chatId)) {
        newSelection.delete(chatId);
      } else {
        // Enforce max selection limit
        if (newSelection.size >= MAX_SELECTION) {
          return;
        }
        newSelection.add(chatId);
      }
      onSelectionChange(newSelection);
    },
    [selectedIds, onSelectionChange]
  );

  // Handle bulk delete
  const handleBulkDelete = useCallback(async () => {
    if (selectedIds.size === 0) return;

    setIsBulkDeleting(true);
    try {
      const idsToDelete = Array.from(selectedIds);

      // Separate archived chats from API conversations
      const archivedIds = idsToDelete.filter((id) => archivedChats?.some((chat) => chat.id === id));
      const conversationIds = idsToDelete.filter(
        (id) => !archivedChats?.some((chat) => chat.id === id)
      );

      // Delete API conversations using batch delete
      if (conversationIds.length > 0 && opensecret) {
        const result = await opensecret.batchDeleteConversations(conversationIds);

        // Remove successfully deleted conversations from local state
        const deletedIds = new Set(
          result.data.filter((item) => item.deleted).map((item) => item.id)
        );
        setConversations((prev) => prev.filter((conv) => !deletedIds.has(conv.id)));
      }

      // Delete archived chats individually
      for (const id of archivedIds) {
        if (localState?.deleteChat) {
          await localState.deleteChat(id);
        }
      }

      // Refresh archived chats if any were deleted
      if (archivedIds.length > 0) {
        queryClient.invalidateQueries({ queryKey: ["archivedChats"] });
      }

      // If current chat was deleted, navigate to home
      if (selectedIds.has(currentChatId || "")) {
        const params = new URLSearchParams(window.location.search);
        params.delete("conversation_id");
        window.history.replaceState({}, "", params.toString() ? `/?${params}` : "/");
        window.dispatchEvent(new Event("newchat"));
      }

      // Clear selection and exit selection mode
      onSelectionChange(new Set());
      onExitSelectionMode?.();
      setIsBulkDeleteDialogOpen(false);
    } catch (error) {
      console.error("Error bulk deleting chats:", error);
    } finally {
      setIsBulkDeleting(false);
    }
  }, [
    selectedIds,
    archivedChats,
    opensecret,
    localState,
    queryClient,
    currentChatId,
    onSelectionChange,
    onExitSelectionMode
  ]);

  // Long press handlers for mobile selection mode activation
  const handleLongPressStart = useCallback(
    (chatId: string) => {
      if (isSelectionMode) return; // Already in selection mode

      longPressTimerRef.current = setTimeout(() => {
        // Enter selection mode and select this chat
        onSelectionChange(new Set([chatId]));
      }, 500); // 500ms long press
    },
    [isSelectionMode, onSelectionChange]
  );

  const handleLongPressEnd = useCallback(() => {
    if (longPressTimerRef.current) {
      clearTimeout(longPressTimerRef.current);
      longPressTimerRef.current = null;
    }
  }, []);

  // Cancel long-press when user starts scrolling (touch move)
  const handleLongPressMove = useCallback(() => {
    if (longPressTimerRef.current) {
      clearTimeout(longPressTimerRef.current);
      longPressTimerRef.current = null;
    }
  }, []);

  // Cleanup long-press timer on unmount
  useEffect(() => {
    return () => {
      if (longPressTimerRef.current) {
        clearTimeout(longPressTimerRef.current);
        longPressTimerRef.current = null;
      }
    };
  }, []);

  // Expose bulk delete dialog trigger
  useEffect(() => {
    const handleOpenBulkDelete = () => {
      if (selectedIds.size > 0) {
        setIsBulkDeleteDialogOpen(true);
      }
    };
    window.addEventListener("openbulkdelete", handleOpenBulkDelete);
    return () => window.removeEventListener("openbulkdelete", handleOpenBulkDelete);
  }, [selectedIds.size]);

  const handleOpenRenameDialog = useCallback((conv: Conversation) => {
    const title = conv.metadata?.title || "Untitled Chat";
    setSelectedChat({ id: conv.id, title });
    setIsRenameDialogOpen(true);
  }, []);

  const handleOpenRenameDialogArchived = useCallback((chat: ArchivedChat) => {
    setSelectedChat({ id: chat.id, title: chat.title });
    setIsRenameDialogOpen(true);
  }, []);

  const handleOpenDeleteDialog = useCallback((conv: Conversation) => {
    const title = conv.metadata?.title || "Untitled Chat";
    setSelectedChat({ id: conv.id, title });
    setIsDeleteDialogOpen(true);
  }, []);

  const handleOpenDeleteDialogArchived = useCallback((chat: ArchivedChat) => {
    setSelectedChat({ id: chat.id, title: chat.title });
    setIsDeleteDialogOpen(true);
  }, []);

  // Handle conversation renaming via API
  const handleRenameConversation = useCallback(
    async (conversationId: string, newTitle: string) => {
      // Check if it's an archived chat
      const isArchived = archivedChats?.some((chat) => chat.id === conversationId);

      if (isArchived) {
        // Handle archived chat rename
        if (!localState?.renameChat) return;
        try {
          await localState.renameChat(conversationId, newTitle);
          // Refresh archived chats list
          queryClient.invalidateQueries({ queryKey: ["archivedChats"] });
        } catch (error) {
          console.error("Error renaming archived chat:", error);
          throw error;
        }
      } else {
        // Handle API conversation rename
        if (!openai) return;
        try {
          // Update conversation metadata with new title
          await openai.conversations.update(conversationId, {
            metadata: { title: newTitle }
          });

          // Update local state immediately
          setConversations((prev) =>
            prev.map((conv) =>
              conv.id === conversationId
                ? { ...conv, metadata: { ...conv.metadata, title: newTitle } }
                : conv
            )
          );
        } catch (error) {
          console.error("Error renaming conversation:", error);
          throw error;
        }
      }
    },
    [openai, archivedChats, localState, queryClient]
  );

  // Handle conversation selection
  const handleSelectConversation = useCallback((conversationId: string) => {
    // Update URL with conversation ID
    const params = new URLSearchParams(window.location.search);
    params.set("conversation_id", conversationId);
    const newUrl = `/?${params}`;

    // Update the URL
    window.history.pushState({}, "", newUrl);

    // Always dispatch event for UnifiedChat to handle
    // This ensures UnifiedChat knows about the change even if URL is the same
    window.dispatchEvent(
      new CustomEvent("conversationselected", {
        detail: { conversationId }
      })
    );
  }, []);

  // Listen for conversation created event to refresh the list
  useEffect(() => {
    const handleConversationCreated = () => {
      // Trigger immediate poll to get the new conversation
      pollForUpdates();
    };

    window.addEventListener("conversationcreated", handleConversationCreated);
    return () => window.removeEventListener("conversationcreated", handleConversationCreated);
  }, [pollForUpdates]);

  if (error) {
    return <div>{error.message}</div>;
  }

  if (isPending) {
    return <div>Loading chat history...</div>;
  }

  // Only show no results message if we have a trimmed search query and no results anywhere
  const trimmedQuery = searchQuery.trim();
  if (trimmedQuery && filteredConversations.length === 0 && filteredArchivedChats.length === 0) {
    return (
      <div className="text-muted-foreground text-center py-4">
        <p>No chats found matching "{trimmedQuery}"</p>
        <p className="text-sm mt-1">Try a different search term</p>
      </div>
    );
  }

  return (
    <>
      {/* Pull-to-refresh indicator */}
      {pullDistance > 0 && (
        <div
          className="flex items-center justify-center transition-all duration-200 ease-out"
          style={{
            height: `${pullDistance}px`,
            opacity: Math.min(pullDistance / 60, 1)
          }}
        >
          <RefreshCw
            className={`h-4 w-4 text-muted-foreground ${isPullRefreshing ? "animate-spin" : ""}`}
            style={{
              transform: isPullRefreshing ? "none" : `rotate(${pullDistance * 3}deg)`
            }}
          />
        </div>
      )}

      {filteredConversations.map((conv: Conversation, index: number) => {
        const title = conv.metadata?.title || "Untitled Chat";
        const isActive = conv.id === currentChatId;
        const isSelected = selectedIds.has(conv.id);
        const isLastConversation = index === filteredConversations.length - 1;
        // Only attach ref when not searching and it's the last item
        const shouldAttachRef = isLastConversation && !searchQuery.trim();

        return (
          <div
            key={conv.id}
            className={`relative group select-none ${isSelected ? "bg-primary/10 rounded-lg" : ""}`}
            ref={shouldAttachRef ? lastConversationRef : undefined}
            onContextMenu={(e) => e.preventDefault()}
          >
            <div
              onClick={() => {
                if (isSelectionMode) {
                  toggleSelection(conv.id);
                } else {
                  handleSelectConversation(conv.id);
                }
              }}
              onMouseDown={() => isMobile && handleLongPressStart(conv.id)}
              onMouseUp={handleLongPressEnd}
              onMouseLeave={handleLongPressEnd}
              onTouchStart={() => isMobile && handleLongPressStart(conv.id)}
              onTouchMove={handleLongPressMove}
              onTouchEnd={handleLongPressEnd}
              onTouchCancel={handleLongPressEnd}
              className={`rounded-lg py-2 transition-all hover:text-primary cursor-pointer ${
                isActive && !isSelectionMode ? "text-primary" : "text-muted-foreground"
              } ${isSelectionMode ? "pl-8" : ""}`}
            >
              {isSelectionMode && (
                <div className="absolute left-1.5 top-1/2 -translate-y-1/2">
                  <Checkbox
                    checked={isSelected}
                    onCheckedChange={() => toggleSelection(conv.id)}
                    onClick={(e) => e.stopPropagation()}
                    className="data-[state=checked]:bg-primary"
                  />
                </div>
              )}
              <div
                className={`overflow-hidden whitespace-nowrap ${!isSelectionMode ? "hover:underline" : ""} pr-8`}
              >
                {title}
              </div>
              <div className="text-xs opacity-70 mt-1">
                {new Date(conv.created_at * 1000).toLocaleDateString()}
              </div>
            </div>
            {!isSelectionMode && (
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <button
                    className={`z-50 bg-background/80 absolute right-2 top-1/2 transform -translate-y-1/2 text-primary transition-opacity p-2 ${
                      isMobile ? "opacity-100" : "opacity-0 group-hover:opacity-100"
                    }`}
                    onClick={(e) => {
                      e.preventDefault();
                      e.stopPropagation();
                    }}
                  >
                    <MoreHorizontal size={16} />
                  </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent>
                  <DropdownMenuItem onClick={() => onSelectionChange(new Set([conv.id]))}>
                    <CheckSquare className="mr-2 h-4 w-4" />
                    <span>Select</span>
                  </DropdownMenuItem>
                  <DropdownMenuItem onClick={() => handleOpenRenameDialog(conv)}>
                    <Pencil className="mr-2 h-4 w-4" />
                    <span>Rename Chat</span>
                  </DropdownMenuItem>
                  <DropdownMenuItem onClick={() => handleOpenDeleteDialog(conv)}>
                    <Trash className="mr-2 h-4 w-4" />
                    <span>Delete Chat</span>
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            )}
            {!isSelectionMode && (
              <div className="absolute inset-y-0 right-0 w-[3rem] bg-gradient-to-l from-background to-transparent pointer-events-none"></div>
            )}
          </div>
        );
      })}

      {/* Loading indicator for pagination */}
      {isLoadingMore && (
        <div className="flex items-center justify-center py-4">
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <div className="w-2 h-2 bg-foreground/60 rounded-full animate-pulse" />
            <div className="w-2 h-2 bg-foreground/60 rounded-full animate-pulse delay-75" />
            <div className="w-2 h-2 bg-foreground/60 rounded-full animate-pulse delay-150" />
          </div>
        </div>
      )}

      {/* Archived Chats Section - only show if there are archived chats */}
      {filteredArchivedChats && filteredArchivedChats.length > 0 && (
        <div className="mt-4">
          <button
            onClick={() => setIsArchivedExpanded(!isArchivedExpanded)}
            className="flex items-center gap-2 w-full text-sm text-muted-foreground hover:text-foreground transition-colors mb-2"
          >
            {isArchivedExpanded ? (
              <ChevronDown className="h-4 w-4" />
            ) : (
              <ChevronRight className="h-4 w-4" />
            )}
            <span>Archived ({filteredArchivedChats.length})</span>
          </button>

          {isArchivedExpanded && (
            <div className="flex flex-col gap-2">
              {filteredArchivedChats.map((chat) => {
                const isActive = chat.id === currentChatId;
                return (
                  <div key={chat.id} className="relative group">
                    <div
                      onClick={() => {
                        router.navigate({ to: "/chat/$chatId", params: { chatId: chat.id } });
                      }}
                      className={`rounded-lg py-2 transition-all hover:text-primary cursor-pointer ${
                        isActive ? "text-primary" : "text-muted-foreground"
                      }`}
                    >
                      <div className="overflow-hidden whitespace-nowrap hover:underline pr-8">
                        {chat.title}
                      </div>
                      <div className="text-xs opacity-70 mt-1">
                        {new Date(chat.updated_at || chat.created_at).toLocaleDateString()}
                      </div>
                    </div>
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <button
                          className={`z-50 bg-background/80 absolute right-2 top-1/2 transform -translate-y-1/2 text-primary transition-opacity p-2 ${
                            isMobile ? "opacity-100" : "opacity-0 group-hover:opacity-100"
                          }`}
                          onClick={(e) => {
                            e.preventDefault();
                            e.stopPropagation();
                          }}
                        >
                          <MoreHorizontal size={16} />
                        </button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent>
                        <DropdownMenuItem onClick={() => handleOpenRenameDialogArchived(chat)}>
                          <Pencil className="mr-2 h-4 w-4" />
                          <span>Rename Chat</span>
                        </DropdownMenuItem>
                        <DropdownMenuItem onClick={() => handleOpenDeleteDialogArchived(chat)}>
                          <Trash className="mr-2 h-4 w-4" />
                          <span>Delete Chat</span>
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                    <div className="absolute inset-y-0 right-0 w-[3rem] bg-gradient-to-l from-background to-transparent pointer-events-none"></div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}

      {selectedChat && (
        <>
          <RenameChatDialog
            open={isRenameDialogOpen}
            onOpenChange={setIsRenameDialogOpen}
            chatId={selectedChat.id}
            currentTitle={selectedChat.title}
            onRename={handleRenameConversation}
          />
          <DeleteChatDialog
            open={isDeleteDialogOpen}
            onOpenChange={setIsDeleteDialogOpen}
            chatTitle={selectedChat.title}
            onConfirm={() => handleDeleteConversation(selectedChat.id)}
          />
        </>
      )}

      <BulkDeleteDialog
        open={isBulkDeleteDialogOpen}
        onOpenChange={setIsBulkDeleteDialogOpen}
        onConfirm={handleBulkDelete}
        count={selectedIds.size}
        isDeleting={isBulkDeleting}
      />
    </>
  );
}
