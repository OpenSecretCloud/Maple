/**
 * UnifiedChat Component
 *
 * This is the main chat interface that replaced the old ChatBox component.
 * It uses the OpenAI Conversations/Responses API instead of localStorage.
 *
 * Recent changes (2025-01-25):
 * - Simplified type system to use OpenAI's native types directly
 * - Content is always an array of proper OpenAI content types (no more string | array)
 * - Uses ConversationContent union for all message content
 * - ModelSelector refactored to just take hasImages boolean instead of full messages
 *
 * Old files to handle:
 * - DELETE: frontend/src/components/ChatBox.tsx (replaced by this component)
 * - DELETE: frontend/src/hooks/useChatSession.ts (old localStorage chat management)
 * - DELETE: frontend/src/routes/index.backup.tsx (just a backup)
 * - SIMPLIFY: frontend/src/routes/_auth.chat.$chatId.tsx
 *   - Make read-only for viewing old localStorage chats
 *   - Remove all interaction code
 *   - Add "archived chat" banner
 *   - Keep for backwards compatibility with chat history
 */
import { useState, useRef, useEffect, useCallback, memo } from "react";
import {
  Send,
  Bot,
  User,
  Copy,
  Check,
  Plus,
  Image,
  FileText,
  X,
  Mic,
  SquarePen
} from "lucide-react";
import RecordRTC from "recordrtc";
import { useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Sidebar, SidebarToggle } from "@/components/Sidebar";
import { useIsMobile } from "@/utils/utils";
import { useOpenAI } from "@/ai/useOpenAi";
import { DEFAULT_MODEL_ID } from "@/state/LocalStateContext";
import { Markdown } from "@/components/markdown";
import { ModelSelector, MODEL_CONFIG } from "@/components/ModelSelector";
import { useLocalState } from "@/state/useLocalState";
import { useOpenSecret } from "@opensecret/react";
import { UpgradePromptDialog } from "@/components/UpgradePromptDialog";
import { DocumentPlatformDialog } from "@/components/DocumentPlatformDialog";
import { RecordingOverlay } from "@/components/RecordingOverlay";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { AlertCircle } from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger
} from "@/components/ui/dropdown-menu";
import { isTauri } from "@/utils/platform";
import type {
  InputTextContent,
  OutputTextContent,
  TextContent,
  SummaryTextContent,
  RefusalContent,
  InputImageContent,
  ComputerScreenshotContent,
  InputFileContent
} from "openai/resources/conversations/conversations.js";

type ConversationContent =
  | InputTextContent
  | OutputTextContent
  | TextContent
  | SummaryTextContent
  | RefusalContent
  | InputImageContent
  | ComputerScreenshotContent
  | InputFileContent;

// Types
interface Message {
  id: string;
  role: "user" | "assistant";
  content: ConversationContent[];
  timestamp: number;
  status?: "completed" | "streaming" | "error" | "in_progress" | "incomplete";
}

// Helper function to merge messages while ensuring uniqueness by ID
// This prevents duplicate key warnings in React by deduplicating messages
function mergeMessagesById(existingMessages: Message[], newMessages: Message[]): Message[] {
  const messagesMap = new Map<string, Message>();

  // First, add all existing messages
  existingMessages.forEach((msg) => messagesMap.set(msg.id, msg));

  // Then, add/update with new messages (overwrites if ID already exists)
  newMessages.forEach((msg) => messagesMap.set(msg.id, msg));

  // Return as array, maintaining insertion order (Map preserves insertion order)
  return Array.from(messagesMap.values());
}

// Custom hook for copy to clipboard functionality
function useCopyToClipboard(text: string) {
  const [isCopied, setIsCopied] = useState(false);

  const handleCopy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(text);
      setIsCopied(true);
      setTimeout(() => setIsCopied(false), 2000);
    } catch (error) {
      console.error("Failed to copy text:", error);
    }
  }, [text]);

  return { isCopied, handleCopy };
}

// Copy button component with cleaner design
function CopyButton({ text }: { text: string }) {
  const { isCopied, handleCopy } = useCopyToClipboard(text);

  return (
    <Button
      variant="ghost"
      size="sm"
      className="h-7 w-7 p-0 text-muted-foreground hover:text-foreground"
      onClick={handleCopy}
      aria-label={isCopied ? "Copied" : "Copy to clipboard"}
    >
      {isCopied ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
    </Button>
  );
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

// Will be needed for future features with conversation items
interface ConversationItem {
  id: string;
  type: "message" | "web_search_call";
  object?: string;
  role?: "user" | "assistant" | "system";
  status?: "completed" | "in_progress" | "incomplete";
  content?: Array<{
    type: "input_text" | "input_image" | "output_text";
    text?: string;
    image_url?: string;
    detail?: "high" | "low" | "auto";
  }>;
  created_at?: number;
}

// Memoized message list component to prevent re-renders on input changes
const MessageList = memo(
  ({
    messages,
    isGenerating,
    chatId,
    firstMessageRef,
    isLoadingOlderMessages
  }: {
    messages: Message[];
    isGenerating: boolean;
    chatId?: string;
    firstMessageRef?: React.RefObject<HTMLDivElement>;
    isLoadingOlderMessages?: boolean;
  }) => {
    return (
      <>
        {/* Loading indicator for older messages */}
        {isLoadingOlderMessages && (
          <div className="flex items-center justify-center py-4">
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <div className="w-2 h-2 bg-foreground/60 rounded-full animate-pulse" />
              <div className="w-2 h-2 bg-foreground/60 rounded-full animate-pulse delay-75" />
              <div className="w-2 h-2 bg-foreground/60 rounded-full animate-pulse delay-150" />
            </div>
          </div>
        )}

        {messages.map((message, index) => (
          <div
            key={message.id}
            ref={index === 0 ? firstMessageRef : undefined}
            className={`group py-6 px-4 ${
              message.role === "user" ? "bg-muted/30" : ""
            } hover:bg-muted/20 transition-colors`}
          >
            <div className="flex gap-3 max-w-4xl mx-auto">
              <div className="flex-shrink-0">
                {message.role === "user" ? (
                  <div className="w-8 h-8 rounded-full bg-primary/10 flex items-center justify-center">
                    <User className="h-4 w-4 text-primary" />
                  </div>
                ) : (
                  <div className="w-8 h-8 rounded-full bg-primary/10 flex items-center justify-center">
                    <Bot className="h-4 w-4 text-primary" />
                  </div>
                )}
              </div>
              <div className="flex-1 overflow-hidden">
                <div className="space-y-2">
                  <div className="font-semibold text-sm">
                    {message.role === "user" ? "You" : "Maple"}
                  </div>
                  <div className="prose prose-sm dark:prose-invert max-w-none">
                    {/* Render content array */}
                    <div className="space-y-3">
                      {message.content.map((part: ConversationContent, partIdx: number) => (
                        <div key={partIdx}>
                          {(part.type === "input_text" ||
                            part.type === "output_text" ||
                            part.type === "text") &&
                          part.text ? (
                            <Markdown
                              content={part.text}
                              loading={message.status === "streaming"}
                              chatId={chatId || ""}
                            />
                          ) : part.type === "input_image" && part.image_url ? (
                            <img
                              src={part.image_url}
                              alt={`Image ${partIdx + 1}`}
                              className="max-w-full rounded-lg"
                              style={{ maxHeight: "400px", objectFit: "contain" }}
                            />
                          ) : null}
                        </div>
                      ))}
                    </div>
                  </div>

                  {/* Status indicators */}
                  {message.role === "assistant" && message.status === "in_progress" && (
                    <div className="flex items-center gap-1 text-muted-foreground">
                      <div className="w-2 h-2 bg-foreground/60 rounded-full animate-pulse" />
                      <div className="w-2 h-2 bg-foreground/60 rounded-full animate-pulse delay-75" />
                      <div className="w-2 h-2 bg-foreground/60 rounded-full animate-pulse delay-150" />
                    </div>
                  )}
                  {message.role === "assistant" && message.status === "incomplete" && (
                    <div className="inline-flex items-center gap-2 text-sm text-muted-foreground bg-muted/50 px-3 py-1.5 rounded-md mt-2">
                      <div className="w-1.5 h-1.5 rounded-full bg-yellow-500" />
                      <span>Chat Canceled</span>
                    </div>
                  )}

                  {/* Actions - only show on hover for assistant messages */}
                  {message.role === "assistant" &&
                    message.content &&
                    message.content.length > 0 && (
                      <div className="flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                        <CopyButton
                          text={message.content
                            .filter((p: ConversationContent) => "text" in p && p.text)
                            .map((p: ConversationContent) => ("text" in p ? p.text : ""))
                            .join("")}
                        />
                      </div>
                    )}
                </div>
              </div>
            </div>
          </div>
        ))}

        {/* Loading indicator - modern style */}
        {isGenerating &&
          !messages.some(
            (m) =>
              m.role === "assistant" && (m.status === "streaming" || m.status === "in_progress")
          ) && (
            <div className="group py-6 px-4">
              <div className="flex gap-3 max-w-4xl mx-auto">
                <div className="flex-shrink-0">
                  <div className="w-8 h-8 rounded-full bg-primary/10 flex items-center justify-center">
                    <Bot className="h-4 w-4 text-primary" />
                  </div>
                </div>
                <div className="flex-1 space-y-2">
                  <div className="font-semibold text-sm">Maple</div>
                  <div className="flex items-center gap-1">
                    <div className="w-2 h-2 bg-foreground/60 rounded-full animate-pulse" />
                    <div className="w-2 h-2 bg-foreground/60 rounded-full animate-pulse delay-75" />
                    <div className="w-2 h-2 bg-foreground/60 rounded-full animate-pulse delay-150" />
                  </div>
                </div>
              </div>
            </div>
          )}
      </>
    );
  }
);

MessageList.displayName = "MessageList";

export function UnifiedChat() {
  const isMobile = useIsMobile();
  const openai = useOpenAI();
  const localState = useLocalState();
  const os = useOpenSecret();
  const isTauriEnv = isTauri();
  const queryClient = useQueryClient();

  // Track chatId from URL - use state so we can update it
  const [chatId, setChatId] = useState<string | undefined>(() => {
    const params = new URLSearchParams(window.location.search);
    return params.get("conversation_id") || undefined;
  });

  // State
  const [conversation, setConversation] = useState<Conversation | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [isGenerating, setIsGenerating] = useState(false);
  const [isSidebarOpen, setIsSidebarOpen] = useState(!isMobile);
  const [error, setError] = useState<string | null>(null);
  const [lastSeenItemId, setLastSeenItemId] = useState<string | undefined>();
  const [isNewConversationJustCreated, setIsNewConversationJustCreated] = useState(false);
  const [currentResponseId, setCurrentResponseId] = useState<string | undefined>();
  const [titleJustUpdated, setTitleJustUpdated] = useState(false);

  // Pagination states
  const [oldestItemId, setOldestItemId] = useState<string | undefined>();
  const [isLoadingOlderMessages, setIsLoadingOlderMessages] = useState(false);
  const [hasMoreOlderMessages, setHasMoreOlderMessages] = useState(false);

  // Attachment states
  const [draftImages, setDraftImages] = useState<File[]>([]);
  const [imageUrls, setImageUrls] = useState<Map<File, string>>(new Map());
  const [documentText, setDocumentText] = useState<string>("");
  const [documentName, setDocumentName] = useState<string>("");
  const [isProcessingDocument, setIsProcessingDocument] = useState(false);
  const [attachmentError, setAttachmentError] = useState<string | null>(null);
  const [upgradeDialogOpen, setUpgradeDialogOpen] = useState(false);
  const [upgradeFeature, setUpgradeFeature] = useState<
    "image" | "document" | "voice" | "usage" | "tokens"
  >("image");
  const [documentPlatformDialogOpen, setDocumentPlatformDialogOpen] = useState(false);

  // Audio recording states
  const [isRecording, setIsRecording] = useState(false);
  const [isTranscribing, setIsTranscribing] = useState(false);
  const [isProcessingSend, setIsProcessingSend] = useState(false);
  const [audioError, setAudioError] = useState<string | null>(null);

  // Scroll state
  const [isUserScrolling, setIsUserScrolling] = useState(false);
  const prevMessageCountRef = useRef(0);
  const prevStreamingRef = useRef(false);
  const [hasNewPolledMessages, setHasNewPolledMessages] = useState(false);
  const recorderRef = useRef<RecordRTC | null>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const assistantStreamingRef = useRef(false);

  const abortControllerRef = useRef<AbortController | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const documentInputRef = useRef<HTMLInputElement>(null);

  // Refs
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const chatContainerRef = useRef<HTMLDivElement>(null);
  const firstMessageRef = useRef<HTMLDivElement>(null);

  // Attachment cleanup function - defined early to avoid reference errors
  const clearAllAttachments = useCallback(() => {
    // Clean up image URLs
    imageUrls.forEach((url) => URL.revokeObjectURL(url));
    setImageUrls(new Map());
    setDraftImages([]);
    setDocumentText("");
    setDocumentName("");
    setAttachmentError(null);
  }, [imageUrls]);

  // Auto-resize textarea
  useEffect(() => {
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
      const scrollHeight = textareaRef.current.scrollHeight;
      textareaRef.current.style.height = `${Math.min(scrollHeight, 200)}px`;
    }
  }, [input]);

  // Improved scroll detection - track if user is near bottom
  const handleScroll = useCallback(() => {
    const container = chatContainerRef.current;
    if (!container) return;

    const { scrollTop, scrollHeight, clientHeight } = container;
    const isNearBottom = scrollHeight - scrollTop - clientHeight < 100;
    setIsUserScrolling(!isNearBottom);
  }, []);

  // Scroll to bottom helper
  const scrollToBottom = useCallback((behavior: ScrollBehavior = "smooth") => {
    if (chatContainerRef.current) {
      chatContainerRef.current.scrollTo({
        top: chatContainerRef.current.scrollHeight,
        behavior
      });
    }
  }, []);

  // Attach scroll listener
  useEffect(() => {
    const container = chatContainerRef.current;
    if (!container) return;

    container.addEventListener("scroll", handleScroll);
    // Initial check
    handleScroll();

    return () => container.removeEventListener("scroll", handleScroll);
  }, [handleScroll]);

  // Initial load - scroll to bottom instantly
  useEffect(() => {
    if (messages.length > 0 && prevMessageCountRef.current === 0 && !isLoadingOlderMessages) {
      // First load of messages - scroll instantly to bottom
      setTimeout(() => {
        scrollToBottom("instant");
      }, 0);
    }
    // Don't update count when loading older messages, to avoid triggering scroll
    if (!isLoadingOlderMessages) {
      prevMessageCountRef.current = messages.length;
    }
  }, [messages.length, scrollToBottom, isLoadingOlderMessages]);

  // Auto-scroll when user sends a message
  // Track the LAST message ID (at the end of the array), not the count
  const lastMessageId = messages.length > 0 ? messages[messages.length - 1].id : null;
  const prevLastMessageId = useRef(lastMessageId);

  useEffect(() => {
    // Only scroll if the LAST message changed AND it's a user message
    // This means a message was added to the END (not prepended)
    if (
      lastMessageId !== prevLastMessageId.current &&
      messages.length > 0 &&
      messages[messages.length - 1].role === "user"
    ) {
      // User just sent a message - scroll to bottom
      setTimeout(() => {
        scrollToBottom("smooth");
      }, 50);
    }
    prevLastMessageId.current = lastMessageId;
  }, [lastMessageId, messages, scrollToBottom]);

  // Auto-scroll when assistant starts streaming (but not while streaming)
  useEffect(() => {
    const hasStreamingMessage = messages.some((m) => m.status === "streaming");

    if (hasStreamingMessage && !prevStreamingRef.current && !isUserScrolling) {
      // Just started streaming - scroll slightly to show the loading indicator
      setTimeout(() => {
        const container = chatContainerRef.current;
        if (container) {
          // Scroll just enough to show the streaming message started
          const currentScroll = container.scrollTop;
          const maxScroll = container.scrollHeight - container.clientHeight;
          // Scroll down 100px or to bottom, whichever is less
          const targetScroll = Math.min(currentScroll + 100, maxScroll);
          container.scrollTo({
            top: targetScroll,
            behavior: "smooth"
          });
        }
      }, 100);
    }

    prevStreamingRef.current = hasStreamingMessage;
  }, [messages, isUserScrolling]);

  // Auto-scroll when new messages arrive from polling
  useEffect(() => {
    if (hasNewPolledMessages) {
      // New messages arrived from polling - scroll to bottom to show them
      setTimeout(() => {
        scrollToBottom("smooth");
      }, 100);

      // Reset the flag
      setHasNewPolledMessages(false);
    }
  }, [hasNewPolledMessages, scrollToBottom]);

  // Unified event handling for conversation changes
  useEffect(() => {
    // Handle new chat event
    const handleNewChat = () => {
      setChatId(undefined);
      setConversation(null);
      setMessages([]);
      setInput("");
      setError(null);
      setLastSeenItemId(undefined);
      // Clear pagination state
      setOldestItemId(undefined);
      setHasMoreOlderMessages(false);
      setIsLoadingOlderMessages(false);
      // Clear attachments
      clearAllAttachments();
      // Reset scroll tracking
      prevMessageCountRef.current = 0;
    };

    // Handle conversation selection from sidebar
    const handleConversationSelected = (event: CustomEvent) => {
      const { conversationId } = event.detail;
      if (conversationId && conversationId !== chatId) {
        // Reset scroll tracking for new conversation
        prevMessageCountRef.current = 0;
        // Update our local chatId state to trigger load
        setChatId(conversationId);
        setError(null);
      }
    };

    // Handle browser back/forward navigation
    const handlePopState = () => {
      const params = new URLSearchParams(window.location.search);
      const newChatId = params.get("conversation_id") || undefined;
      if (newChatId !== chatId) {
        // Reset scroll tracking for navigation
        prevMessageCountRef.current = 0;
        setChatId(newChatId);
      }
    };

    window.addEventListener("newchat", handleNewChat);
    window.addEventListener("conversationselected", handleConversationSelected as EventListener);
    window.addEventListener("popstate", handlePopState);

    return () => {
      window.removeEventListener("newchat", handleNewChat);
      window.removeEventListener(
        "conversationselected",
        handleConversationSelected as EventListener
      );
      window.removeEventListener("popstate", handlePopState);
    };
  }, [chatId, clearAllAttachments]);

  // Cancel the current response
  const handleCancelResponse = useCallback(async () => {
    if (!currentResponseId || !openai) return;

    try {
      await (openai.responses as { cancel: (id: string) => Promise<unknown> }).cancel(
        currentResponseId
      );
      setIsGenerating(false);
      setCurrentResponseId(undefined);
      abortControllerRef.current?.abort();
      abortControllerRef.current = null;
      assistantStreamingRef.current = false;
    } catch (error) {
      console.error("Failed to cancel response:", error);
      setError("Failed to cancel response. Please try again.");
    }
  }, [currentResponseId, openai]);

  // Load conversation from API
  const loadConversation = useCallback(
    async (conversationId: string) => {
      if (!openai) return;

      // Reset message count before loading new conversation
      prevMessageCountRef.current = 0;

      try {
        // Fetch conversation metadata
        const conv = await openai.conversations.retrieve(conversationId);
        setConversation(conv as Conversation);

        // Fetch initial 10 most recent items in descending order (newest first)
        const itemsResponse = await openai.conversations.items.list(conversationId, {
          limit: 10,
          order: "desc"
        });

        // Convert items to messages
        const loadedMessages: Message[] = [];

        for (const item of itemsResponse.data) {
          if (item.type === "message" && item.role && item.content) {
            const convItem = item as ConversationItem;
            loadedMessages.push({
              id: item.id,
              role: item.role as "user" | "assistant",
              content: item.content,
              timestamp:
                ((item as ConversationItem & { created_at?: number }).created_at ??
                  Date.now() / 1000) * 1000,
              status: convItem.status || "completed"
            });
          }
        }

        // Reverse the array for display (we want oldest first/at top, newest last/at bottom)
        // API returns desc (newest first), but chat UI needs chronological order
        const messagesInChronologicalOrder = loadedMessages.reverse();
        setMessages(messagesInChronologicalOrder);

        // Set pagination state
        // Before reversal, last item was the oldest. After reversal, it's now first item.
        // But for the API "after" parameter, we still need the chronologically oldest ID
        if (loadedMessages.length > 0) {
          // After reversal, the FIRST message is the oldest (chronologically earliest)
          const oldestId = messagesInChronologicalOrder[0].id;
          setOldestItemId(oldestId);
          // If we got a full page, there might be more
          const hasMore = loadedMessages.length === 10;
          setHasMoreOlderMessages(hasMore);
        } else {
          setOldestItemId(undefined);
          setHasMoreOlderMessages(false);
        }

        // Set last seen ID for polling - use the NEWEST message (first in desc response)
        // Skip in_progress messages by finding the first completed one
        const newestCompletedItem = itemsResponse.data.find(
          (item) => (item as ConversationItem).status !== "in_progress"
        );
        if (newestCompletedItem) {
          setLastSeenItemId(newestCompletedItem.id);
        }
      } catch (error) {
        const err = error as { status?: number; message?: string };
        if (err.status === 404) {
          // Conversation doesn't exist - clear and start fresh
          // Conversation not found, starting new
          setConversation(null);
          setMessages([]);
          setError(null);
          setOldestItemId(undefined);
          setHasMoreOlderMessages(false);
          // Clear the invalid conversation_id from URL
          const params = new URLSearchParams(window.location.search);
          params.delete("conversation_id");
          window.history.replaceState({}, "", params.toString() ? `/?${params}` : "/");
        } else {
          console.error("Failed to load conversation:", error);
          setError(err.message || "Failed to load conversation");
        }
      }
    },
    [openai]
  );

  // Load older messages for pagination
  const loadOlderMessages = useCallback(async () => {
    if (!conversation?.id || !openai || !oldestItemId || isLoadingOlderMessages) return;

    setIsLoadingOlderMessages(true);

    try {
      // Fetch next 10 older items using the oldest item ID we have
      const itemsResponse = await openai.conversations.items.list(conversation.id, {
        limit: 10,
        order: "desc",
        after: oldestItemId
      });

      // Convert items to messages
      const olderMessages: Message[] = [];

      for (const item of itemsResponse.data) {
        if (item.type === "message" && item.role && item.content) {
          const convItem = item as ConversationItem;
          olderMessages.push({
            id: item.id,
            role: item.role as "user" | "assistant",
            content: item.content,
            timestamp:
              ((item as ConversationItem & { created_at?: number }).created_at ??
                Date.now() / 1000) * 1000,
            status: convItem.status || "completed"
          });
        }
      }

      if (olderMessages.length > 0) {
        // Reverse for chronological order (API returns desc, we need asc for display)
        const olderMessagesInChronologicalOrder = olderMessages.reverse();

        // Prepend older messages to the existing messages using merge helper
        // This ensures no duplicates if a message was already loaded
        setMessages((prev) => mergeMessagesById(olderMessagesInChronologicalOrder, prev));

        // Update pagination state
        // After reversal, the FIRST message is the chronologically oldest
        const newOldestId = olderMessagesInChronologicalOrder[0].id;
        const hasMore = olderMessages.length === 10;
        setOldestItemId(newOldestId);
        setHasMoreOlderMessages(hasMore);
      } else {
        // No more messages to load
        setHasMoreOlderMessages(false);
      }
    } catch (error) {
      console.error("Failed to load older messages:", error);
    } finally {
      setIsLoadingOlderMessages(false);
    }
  }, [conversation?.id, openai, oldestItemId, isLoadingOlderMessages]);

  // Polling mechanism for conversation updates
  const pollForNewItems = useCallback(async () => {
    if (!conversation?.id || !openai) return;
    if (assistantStreamingRef.current) return;

    try {
      // Fetch NEW items that came after the last seen ID
      // Use order=asc to get items chronologically after the lastSeenItemId
      const response = await openai.conversations.items.list(conversation.id, {
        ...(lastSeenItemId ? { after: lastSeenItemId, order: "asc" } : {}),
        limit: 20 // Smaller limit since we only expect a few new messages
      });

      if (response.data.length > 0) {
        // Convert API items to UI messages
        const newMessages: Message[] = [];

        for (const item of response.data) {
          if (item.type === "message" && item.role) {
            const convItem = item as ConversationItem;
            // Convert content to array if it's a string
            const messageContent =
              typeof item.content === "string"
                ? [{ type: "text", text: item.content }]
                : Array.isArray(item.content)
                  ? item.content
                  : [];

            newMessages.push({
              id: item.id,
              role: item.role as "user" | "assistant",
              content: messageContent as ConversationContent[],
              timestamp:
                ((item as ConversationItem & { created_at?: number }).created_at ??
                  Date.now() / 1000) * 1000,
              status: convItem.status || "completed"
            });
          }
        }

        if (newMessages.length > 0) {
          // Merge new messages with deduplication using helper
          setMessages((prev) => {
            // Check if there are truly new messages (not already in prev)
            const prevIds = new Set(prev.map((m) => m.id));
            const trulyNewMessages = newMessages.filter((m) => !prevIds.has(m.id));

            // Mark that we have new polled messages for scrolling
            if (trulyNewMessages.length > 0) {
              setHasNewPolledMessages(true);
            }

            // Use merge helper to combine, which will deduplicate and update existing messages
            return mergeMessagesById(prev, newMessages);
          });

          // Update last seen item ID for next poll
          // Since we're using order=asc, the LAST item is the newest
          // Skip in_progress messages by finding the last completed one
          const newestCompletedItem = [...response.data]
            .reverse()
            .find((item) => (item as ConversationItem).status !== "in_progress");
          if (newestCompletedItem) {
            setLastSeenItemId(newestCompletedItem.id);
          }

          // Check if we're no longer generating
          // Only stop if assistant message is completed or incomplete, not if still in_progress
          if (
            isGenerating &&
            newMessages.some(
              (m) =>
                m.role === "assistant" && (m.status === "completed" || m.status === "incomplete")
            )
          ) {
            setIsGenerating(false);
          }
        }
      }
    } catch (error) {
      console.error("Polling error:", error);
      // Don't throw - polling should fail silently
    }
  }, [conversation?.id, lastSeenItemId, isGenerating, openai]);

  // Load conversation when URL changes or on mount
  useEffect(() => {
    // Skip if we just created a new conversation
    if (isNewConversationJustCreated) {
      // Reset the flag for next time
      setIsNewConversationJustCreated(false);
      return;
    }

    if (chatId && openai) {
      // Load the conversation from URL
      loadConversation(chatId);
    } else if (!chatId) {
      // Clear if no conversation ID
      setConversation(null);
      setMessages([]);
      setLastSeenItemId(undefined);
      // Clear pagination state
      setOldestItemId(undefined);
      setHasMoreOlderMessages(false);
      setIsLoadingOlderMessages(false);
      // Reset scroll tracking
      prevMessageCountRef.current = 0;
    }
  }, [chatId, openai, loadConversation]);

  // Set up progressive polling interval
  useEffect(() => {
    if (!conversation?.id || !openai) return;

    // Progressive intervals: 2s, 5s, 10s, 15s, 20s, 30s, 60s (then 60s forever)
    const intervals = [2000, 5000, 10000, 15000, 20000, 30000, 60000];
    let currentIntervalIndex = 0;
    let timeoutId: ReturnType<typeof setTimeout>;

    const scheduleNextPoll = () => {
      // Get current interval (use last interval if we've reached the end)
      const currentInterval = intervals[Math.min(currentIntervalIndex, intervals.length - 1)];

      timeoutId = setTimeout(() => {
        pollForNewItems();

        // Move to next interval if not at the end
        if (currentIntervalIndex < intervals.length - 1) {
          currentIntervalIndex++;
        }

        // Schedule the next poll
        scheduleNextPoll();
      }, currentInterval);
    };

    // Start the progressive polling (first poll after 2s)
    scheduleNextPoll();

    return () => {
      if (timeoutId) clearTimeout(timeoutId);
    };
  }, [conversation?.id, openai, pollForNewItems]);

  // Poll for title updates when it's "New Conversation" with exponential backoff
  useEffect(() => {
    const currentTitle = conversation?.metadata?.title;

    // Only poll if we have a conversation and the title is "New Conversation"
    if (!conversation?.id || !openai || currentTitle !== "New Conversation") {
      return;
    }

    // Exponential backoff: 0.5s, 1s, 2s, 4s, 8s, 10s (max)
    let currentDelay = 500; // Start at 0.5s
    const maxDelay = 10000; // Cap at 10s
    let timeoutId: ReturnType<typeof setTimeout>;

    const checkTitle = async () => {
      try {
        // Fetch updated conversation metadata
        const updatedConv = await openai.conversations.retrieve(conversation.id);
        const newTitle = (updatedConv as Conversation).metadata?.title;

        // If title changed from "New Conversation", update local state and sidebar
        if (newTitle && newTitle !== "New Conversation") {
          setConversation(updatedConv as Conversation);
          // Trigger title animation
          setTitleJustUpdated(true);
          // Remove animation class after animation completes (800ms for flash animation)
          setTimeout(() => setTitleJustUpdated(false), 850);
          // Refresh the sidebar conversation list
          queryClient.invalidateQueries({ queryKey: ["conversations"] });
          return; // Stop polling once title is updated
        }

        // Schedule next check with exponential backoff
        currentDelay = Math.min(currentDelay * 2, maxDelay);
        timeoutId = setTimeout(checkTitle, currentDelay);
      } catch (error) {
        console.error("Failed to check title update:", error);
        // Continue polling even on error
        currentDelay = Math.min(currentDelay * 2, maxDelay);
        timeoutId = setTimeout(checkTitle, currentDelay);
      }
    };

    // Start the first check after 0.5s
    timeoutId = setTimeout(checkTitle, currentDelay);

    return () => {
      if (timeoutId) clearTimeout(timeoutId);
    };
  }, [conversation?.id, conversation?.metadata?.title, openai, queryClient]);

  // Set up IntersectionObserver for loading older messages
  useEffect(() => {
    if (!firstMessageRef.current || !hasMoreOlderMessages) return;

    const observer = new IntersectionObserver(
      (entries) => {
        // When the first message comes into view, load older messages
        if (entries[0].isIntersecting && hasMoreOlderMessages && !isLoadingOlderMessages) {
          loadOlderMessages();
        }
      },
      {
        root: chatContainerRef.current,
        rootMargin: "100px", // Start loading a bit before the message is visible
        threshold: 0.1
      }
    );

    observer.observe(firstMessageRef.current);

    return () => {
      observer.disconnect();
    };
  }, [hasMoreOlderMessages, isLoadingOlderMessages, loadOlderMessages]);

  // Auto-clear error after 3 seconds
  useEffect(() => {
    if (error) {
      const timeoutId = setTimeout(() => {
        setError(null);
      }, 3000);

      return () => clearTimeout(timeoutId);
    }
  }, [error]);

  // Toggle sidebar
  const toggleSidebar = useCallback(() => setIsSidebarOpen((prev) => !prev), []);

  // Check user's billing access
  const billingStatus = localState.billingStatus;
  const hasProAccess =
    billingStatus &&
    (billingStatus.product_name?.toLowerCase().includes("pro") ||
      billingStatus.product_name?.toLowerCase().includes("max") ||
      billingStatus.product_name?.toLowerCase().includes("team"));

  const canUseImages = hasProAccess;
  const canUseDocuments = hasProAccess;
  const canUseVoice = hasProAccess && localState.hasWhisperModel;

  const handleAddImages = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      if (!e.target.files) return;

      const supportedTypes = ["image/jpeg", "image/jpg", "image/png", "image/webp"];
      const maxSizeInBytes = 10 * 1024 * 1024; // 10MB

      const validFiles = Array.from(e.target.files).filter((file) => {
        if (!supportedTypes.includes(file.type.toLowerCase())) {
          setAttachmentError("Only JPEG, PNG, and WebP images are supported");
          setTimeout(() => setAttachmentError(null), 5000);
          return false;
        }
        if (file.size > maxSizeInBytes) {
          setAttachmentError(`Image too large (max 10MB)`);
          setTimeout(() => setAttachmentError(null), 5000);
          return false;
        }
        return true;
      });

      // Create object URLs for previews
      const newUrlMap = new Map(imageUrls);
      validFiles.forEach((file) => {
        if (!newUrlMap.has(file)) {
          newUrlMap.set(file, URL.createObjectURL(file));
        }
      });
      setImageUrls(newUrlMap);
      setDraftImages((prev) => [...prev, ...validFiles]);

      // Auto-switch to vision model if needed
      const supportsVision = MODEL_CONFIG[localState.model]?.supportsVision;
      if (!supportsVision && validFiles.length > 0) {
        // Find first vision-capable model user has access to
        const visionModels = localState.availableModels.filter(
          (m) => MODEL_CONFIG[m.id]?.supportsVision
        );
        if (visionModels.length > 0) {
          localState.setModel(visionModels[0].id);
        }
      }

      // Clear input to allow re-uploading same file
      e.target.value = "";
    },
    [imageUrls, localState]
  );

  const removeImage = useCallback(
    (idx: number) => {
      setDraftImages((prev) => {
        const fileToRemove = prev[idx];
        const url = imageUrls.get(fileToRemove);
        if (url) {
          URL.revokeObjectURL(url);
          setImageUrls((prevUrls) => {
            const newUrls = new Map(prevUrls);
            newUrls.delete(fileToRemove);
            return newUrls;
          });
        }
        return prev.filter((_, i) => i !== idx);
      });
    },
    [imageUrls]
  );

  const handleDocumentUpload = useCallback(
    async (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0];
      if (!file) return;

      const maxSizeInBytes = 10 * 1024 * 1024; // 10MB
      if (file.size > maxSizeInBytes) {
        setAttachmentError("Document too large (max 10MB)");
        setTimeout(() => setAttachmentError(null), 5000);
        e.target.value = "";
        return;
      }

      setIsProcessingDocument(true);
      setAttachmentError(null);

      try {
        // For text files, read directly
        if (file.name.endsWith(".txt") || file.name.endsWith(".md")) {
          const text = await file.text();
          // Format as JSON for consistency with PDF handling
          const documentData = {
            document: {
              filename: file.name,
              text_content: text
            }
          };
          setDocumentText(JSON.stringify(documentData));
          setDocumentName(file.name);
        } else if (file.name.endsWith(".pdf") && isTauriEnv) {
          // For PDFs in Tauri, use the parseDocument API
          const reader = new FileReader();
          const base64Data = await new Promise<string>((resolve, reject) => {
            reader.onload = () => {
              const base64 = (reader.result as string).split(",")[1];
              resolve(base64);
            };
            reader.onerror = reject;
            reader.readAsDataURL(file);
          });

          // Use the Tauri API directly for parsing PDFs
          const { invoke } = await import("@tauri-apps/api/core");

          // Define the response type to match Rust
          interface RustDocumentResponse {
            document: {
              filename: string;
              text_content: string;
            };
            status: string;
          }

          const result = await invoke<RustDocumentResponse>("extract_document_content", {
            fileBase64: base64Data,
            filename: file.name,
            fileType: "pdf"
          });

          if (result.document?.text_content) {
            // Create a cleaned version with image references removed
            const cleanedParsed = {
              document: {
                filename: result.document.filename,
                text_content: result.document.text_content.replace(/!\[Image\]\([^)]+\)/g, "")
              }
            };

            // Store as JSON string for markdown.tsx to parse and display properly
            setDocumentText(JSON.stringify(cleanedParsed));
            setDocumentName(file.name);
          }
        } else if (file.name.endsWith(".pdf")) {
          setAttachmentError("PDF files can only be processed in the desktop app");
          setTimeout(() => setAttachmentError(null), 5000);
        }
      } catch (error) {
        console.error("Document processing error:", error);
        setAttachmentError("Failed to process document");
        setTimeout(() => setAttachmentError(null), 5000);
      } finally {
        setIsProcessingDocument(false);
        e.target.value = "";
      }
    },
    [isTauriEnv]
  );

  const removeDocument = useCallback(() => {
    setDocumentText("");
    setDocumentName("");
  }, []);

  // Audio recording functions
  const startRecording = async () => {
    // Prevent duplicate starts
    if (isRecording || isTranscribing) return;

    // Check if user has access
    if (!canUseVoice) {
      setUpgradeFeature("voice");
      setUpgradeDialogOpen(true);
      return;
    }

    try {
      // Check if getUserMedia is available
      if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
        setAudioError(
          "Microphone access is blocked. Please check your browser permissions or disable Lockdown Mode for this site (Settings > Safari > Advanced > Lockdown Mode)."
        );
        setTimeout(() => setAudioError(null), 8000);
        return;
      }

      const stream = await navigator.mediaDevices.getUserMedia({
        audio: {
          echoCancellation: false,
          noiseSuppression: true,
          autoGainControl: false,
          sampleRate: 16000
        }
      });

      streamRef.current = stream;

      // Create RecordRTC instance configured for WAV
      const recorder = new RecordRTC(stream, {
        type: "audio",
        mimeType: "audio/wav",
        recorderType: RecordRTC.StereoAudioRecorder,
        numberOfAudioChannels: 1,
        desiredSampRate: 16000
      });

      recorderRef.current = recorder;
      recorder.startRecording();
      setIsRecording(true);
      setAudioError(null);
    } catch (error) {
      console.error("Failed to start recording:", error);
      const err = error as Error & { name?: string };

      // Handle different error types
      if (err.name === "NotAllowedError" || err.name === "PermissionDeniedError") {
        setAudioError(
          "Microphone access denied. Please enable microphone permissions in Settings > Maple."
        );
      } else if (err.name === "NotFoundError" || err.name === "DevicesNotFoundError") {
        setAudioError("No microphone found. Please check your device.");
      } else if (err.name === "NotReadableError" || err.name === "TrackStartError") {
        setAudioError("Microphone is already in use by another app.");
      } else {
        setAudioError(
          `Failed to access microphone: ${err.name || "Unknown error"} - ${err.message || "Please try again"}`
        );
      }

      setTimeout(() => setAudioError(null), 5000);
    }
  };

  const stopRecording = (shouldSend: boolean = false) => {
    if (recorderRef.current && isRecording) {
      // Only hide immediately if canceling, keep visible if sending
      if (!shouldSend) {
        setIsRecording(false);
      } else {
        setIsProcessingSend(true);
      }

      recorderRef.current.stopRecording(async () => {
        const blob = recorderRef.current?.getBlob();

        if (!blob || blob.size === 0) {
          console.error("No audio recorded or empty recording");
          if (shouldSend) {
            setAudioError("No audio was recorded. Please try again.");
            setTimeout(() => setAudioError(null), 5000);
          }
          // Clean up
          if (streamRef.current) {
            streamRef.current.getTracks().forEach((track) => track.stop());
            streamRef.current = null;
          }
          recorderRef.current = null;
          setIsProcessingSend(false);
          setIsRecording(false);
          return;
        }

        // Create a proper WAV file
        const audioFile = new File([blob], "recording.wav", {
          type: "audio/wav"
        });

        if (shouldSend) {
          setIsTranscribing(true);
          try {
            const result = await os.transcribeAudio(audioFile, "whisper-large-v3");
            const transcribedText = result.text.trim();

            if (transcribedText) {
              // Combine with existing input if any
              const newValue = input ? `${input} ${transcribedText}` : transcribedText;

              // Clear states before sending
              setInput("");
              clearAllAttachments();
              setIsRecording(false);
              setIsTranscribing(false);
              setIsProcessingSend(false);

              // Send the message directly with the transcribed text
              await handleSendMessage(undefined, newValue);
            } else {
              setAudioError("No speech detected. Please try again.");
              setTimeout(() => setAudioError(null), 5000);
            }
          } catch (error) {
            console.error("Transcription failed:", error);
            setAudioError("Failed to transcribe audio. Please try again.");
            setTimeout(() => setAudioError(null), 5000);
          } finally {
            setIsTranscribing(false);
            setIsProcessingSend(false);
            setIsRecording(false);
          }
        }

        // Clean up resources
        if (streamRef.current) {
          streamRef.current.getTracks().forEach((track) => track.stop());
          streamRef.current = null;
        }
        recorderRef.current = null;
      });
    }
  };

  // Helper function to process streaming response - used by both initial request and retry
  const processStreamingResponse = useCallback(async (stream: AsyncIterable<unknown>) => {
    let serverAssistantId: string | undefined;
    let assistantMessageAdded = false;
    let accumulatedContent = "";

    for await (const event of stream) {
      if ((event as { type: string }).type === "response.created") {
        const eventWithResponse = event as { response?: { id?: string } };
        if (eventWithResponse.response?.id) {
          setCurrentResponseId(eventWithResponse.response.id);
        }
      } else if (
        (event as { type: string }).type === "response.output_item.added" &&
        (event as { item?: { type?: string } }).item?.type === "message"
      ) {
        const eventWithItem = event as { item?: { id?: string } };
        if (eventWithItem.item?.id) {
          serverAssistantId = eventWithItem.item.id;

          const assistantMessage: Message = {
            id: serverAssistantId!,
            role: "assistant",
            content: [],
            timestamp: Date.now(),
            status: "streaming"
          };
          // Use merge helper to prevent duplicates if this message already exists
          setMessages((prev) => mergeMessagesById(prev, [assistantMessage]));
          assistantMessageAdded = true;
          assistantStreamingRef.current = true;
        }
      } else if (
        (event as { type: string }).type === "response.output_text.delta" &&
        (event as { delta?: string }).delta
      ) {
        const delta = (event as { delta: string }).delta;
        accumulatedContent += delta;

        if (serverAssistantId && assistantMessageAdded) {
          const outputContent: OutputTextContent = {
            type: "output_text",
            text: accumulatedContent,
            annotations: []
          };
          // Update the message content using merge helper for consistency
          const updatedMessage: Message = {
            id: serverAssistantId,
            role: "assistant",
            content: [outputContent],
            timestamp: Date.now(),
            status: "streaming"
          };
          setMessages((prev) => mergeMessagesById(prev, [updatedMessage]));
        }
      } else if ((event as { type: string }).type === "response.output_item.done") {
        if (serverAssistantId) {
          // Update status to completed using merge helper
          setMessages((prev) => {
            const msgToUpdate = prev.find((m) => m.id === serverAssistantId);
            if (msgToUpdate) {
              return mergeMessagesById(prev, [{ ...msgToUpdate, status: "completed" }]);
            }
            return prev;
          });
          setLastSeenItemId(serverAssistantId);
          assistantStreamingRef.current = false;
        }
      } else if (
        (event as { type: string }).type === "response.failed" ||
        (event as { type: string }).type === "error"
      ) {
        console.error("Streaming error:", event);
        if (serverAssistantId) {
          // Update status to error using merge helper
          setMessages((prev) => {
            const msgToUpdate = prev.find((m) => m.id === serverAssistantId);
            if (msgToUpdate) {
              return mergeMessagesById(prev, [{ ...msgToUpdate, status: "error" }]);
            }
            return prev;
          });
        }
        setError("Failed to generate response. Please try again.");
        assistantStreamingRef.current = false;
      } else if ((event as { type: string }).type === "response.cancelled") {
        if (serverAssistantId) {
          // Update status to incomplete using merge helper
          setMessages((prev) => {
            const msgToUpdate = prev.find((m) => m.id === serverAssistantId);
            if (msgToUpdate) {
              return mergeMessagesById(prev, [{ ...msgToUpdate, status: "incomplete" }]);
            }
            return prev;
          });
        }
        assistantStreamingRef.current = false;
        break;
      }
    }
  }, []);

  // Send message handler - now accepts optional override text for voice input
  const handleSendMessage = useCallback(
    async (e?: React.FormEvent, overrideInput?: string) => {
      e?.preventDefault();

      // Use override input (from voice) or regular input
      const textToSend = overrideInput || input;
      const trimmedInput = textToSend.trim();
      const hasContent = trimmedInput || draftImages.length > 0 || documentText;
      if (!hasContent || isGenerating || !openai) return;

      // Clear any previous error
      setError(null);

      // Helper function to convert File to data URL
      const fileToDataURL = (file: File): Promise<string> => {
        return new Promise((resolve, reject) => {
          const reader = new FileReader();
          reader.onload = () => resolve(reader.result as string);
          reader.onerror = reject;
          reader.readAsDataURL(file);
        });
      };

      // Build the message content - always as an array
      // Using the input types since we're building user input
      const messageContent: (InputTextContent | InputImageContent)[] = [];

      // Combine document text with input if both exist
      let finalText = trimmedInput;
      if (documentText) {
        finalText = documentText + (trimmedInput ? `\n\n${trimmedInput}` : "");
      }

      // Add text part if exists
      if (finalText) {
        const textContent: InputTextContent = {
          type: "input_text",
          text: finalText
        };
        messageContent.push(textContent);
      }

      // Add image parts if we have images
      for (const file of draftImages) {
        try {
          const dataUrl = await fileToDataURL(file);
          const imageContent: InputImageContent = {
            type: "input_image",
            image_url: dataUrl,
            detail: "auto",
            file_id: null
          };
          messageContent.push(imageContent);
        } catch (error) {
          console.error("Failed to convert image:", error);
        }
      }

      // Add user message immediately with a local UUID
      const localMessageId = crypto.randomUUID();
      const userMessage: Message = {
        id: localMessageId,
        role: "user",
        content: messageContent, // Store the actual content structure
        timestamp: Date.now(),
        status: "completed"
      };

      // Use merge helper to add user message (prevents duplicates)
      setMessages((prev) => mergeMessagesById(prev, [userMessage]));
      // Set lastSeenItemId to our local message ID
      // The backend should map this via internal_message_id
      setLastSeenItemId(localMessageId);

      // Store the original input and attachments in case we need to restore them
      const originalInput = trimmedInput;
      const originalImages = [...draftImages];
      const originalImageUrls = new Map(imageUrls);
      const originalDocumentText = documentText;
      const originalDocumentName = documentName;

      // Only clear input if not using override (voice already cleared it)
      if (!overrideInput) {
        setInput("");
        clearAllAttachments();
      }
      setIsGenerating(true);

      try {
        // Create conversation if we don't have one
        let conversationId = conversation?.id;
        if (!conversationId) {
          const newConv = await openai.conversations.create({
            metadata: {}
          });
          conversationId = newConv.id;
          setConversation(newConv as Conversation);

          // Update URL with new conversation ID
          const usp = new URLSearchParams(window.location.search);
          usp.set("conversation_id", conversationId);
          window.history.replaceState(null, "", `${window.location.pathname}?${usp.toString()}`);

          // Update local state but flag that we just created it
          setIsNewConversationJustCreated(true);
          setChatId(conversationId);

          // Trigger sidebar refresh to show the new conversation
          window.dispatchEvent(new Event("conversationcreated"));
        }

        // Create abort controller for this request
        const abortController = new AbortController();
        abortControllerRef.current = abortController;

        // Create streaming response - the API expects the content directly as we built it
        const stream = await openai.responses.create(
          {
            conversation: conversationId,
            model: localState.model || DEFAULT_MODEL_ID, // Use selected model or default
            input: [{ role: "user", content: messageContent }],
            metadata: { internal_message_id: localMessageId }, // Pass our local ID
            stream: true,
            store: true // Store in conversation history
          },
          { signal: abortController.signal }
        );

        // Process the streaming response
        await processStreamingResponse(stream);
        setCurrentResponseId(undefined);
      } catch (error) {
        console.error("Failed to send message:", error);

        // Handle usage limit errors with upsell dialogs
        // The SDK throws errors with the message "Request failed with status 403: {json}"
        // We need to parse this to extract the actual error details
        let errorMessage = error instanceof Error ? error.message : "Something went wrong";

        // Also check the cause property if it exists
        const causeMessage = (error as Error & { cause?: { message?: string } })?.cause?.message;
        if (causeMessage && causeMessage.includes("Request failed with status 403:")) {
          errorMessage = causeMessage;
        }

        let status403Error: { status: number; message: string } | null = null;

        // Check if this is a 403 error from the SDK
        if (errorMessage.includes("Request failed with status 403:")) {
          try {
            // Extract the JSON part from the error message
            const jsonMatch = errorMessage.match(/Request failed with status 403:\s*({.*})/);
            if (jsonMatch && jsonMatch[1]) {
              status403Error = JSON.parse(jsonMatch[1]);
            }
          } catch (parseError) {
            console.error("Failed to parse 403 error:", parseError);
          }
        }

        if (status403Error) {
          // Remove the user message from history and restore input
          setMessages((prev) => prev.filter((msg) => msg.id !== localMessageId));

          // Restore the original input and attachments
          if (!overrideInput) {
            setInput(originalInput);
            setDraftImages(originalImages);
            setImageUrls(originalImageUrls);
            setDocumentText(originalDocumentText);
            setDocumentName(originalDocumentName);
          }

          if (status403Error.message === "Free tier token limit exceeded") {
            // Token limit exceeded - conversation too long for free tier
            setUpgradeFeature("tokens");
            setUpgradeDialogOpen(true);
            setError(
              "This conversation is too long for the free tier. Upgrade to Pro for longer conversations."
            );
          } else if (status403Error.message === "Usage limit reached") {
            // Usage limit reached - could be daily (free) or monthly (paid)
            const isFreeTier =
              !billingStatus?.product_name || billingStatus.product_name.toLowerCase() === "free";

            if (isFreeTier) {
              // Free tier hit daily limits
              setUpgradeFeature("usage");
              setUpgradeDialogOpen(true);
              setError("You've reached your daily usage limit. Upgrade to Pro for more chats.");
            } else {
              // Paid tier hit monthly limits - upsell to next tier
              setUpgradeFeature("usage");
              setUpgradeDialogOpen(true);
              const isPro =
                billingStatus.product_name?.toLowerCase().includes("pro") &&
                !billingStatus.product_name?.toLowerCase().includes("max");
              setError(
                isPro
                  ? "You've reached your monthly Pro limit. Upgrade to Max for 10x more usage."
                  : "You've reached your monthly usage limit. Please wait for the next billing cycle."
              );
            }
          } else {
            setError(status403Error.message || "Access denied. Please check your subscription.");
          }
        } else if (error instanceof Error && error.name !== "AbortError") {
          // Retry logic for non-rate-limit errors on follow-up conversations
          // Only retry if this is a follow-up (conversation already existed before this request)
          const isFollowUpConversation = conversation?.id && messages.length > 1;

          if (isFollowUpConversation && conversation?.id) {
            try {
              // Wait 1 second before retrying
              console.log("Waiting 1s before retry...");
              await new Promise((resolve) => setTimeout(resolve, 1000));

              console.log("Retrying request once...");
              // TODO: Consider calling os.getAttestation() here if needed for attestation refresh

              // Create new abort controller for retry
              const retryAbortController = new AbortController();
              abortControllerRef.current = retryAbortController;

              const retryStream = await openai.responses.create(
                {
                  conversation: conversation.id,
                  model: localState.model || DEFAULT_MODEL_ID,
                  input: [{ role: "user", content: messageContent }],
                  metadata: { internal_message_id: localMessageId }, // Server prevents duplicate IDs
                  stream: true,
                  store: true
                },
                { signal: retryAbortController.signal }
              );

              // Process the retry stream using the same helper function
              await processStreamingResponse(retryStream);
              console.log("Retry completed successfully");
              return;
            } catch (retryError) {
              // Retry failed - check one last time if message actually went through
              console.error("Retry failed:", retryError);

              try {
                // Check the last 5 items to see if our message is there
                const finalCheckResponse = await openai.conversations.items.list(conversation.id, {
                  limit: 5,
                  order: "desc"
                });

                // Look for our message by ID - server uses our internal_message_id as the item's ID
                const foundMessage = finalCheckResponse.data.find(
                  (item) => item.id === localMessageId
                );

                if (!foundMessage) {
                  // Message definitely didn't go through - restore input and remove from UI
                  console.log("Message not found after retry - restoring input");

                  // Remove the user message from the messages list
                  setMessages((prev) => prev.filter((msg) => msg.id !== localMessageId));

                  // Restore the original input and attachments
                  if (!overrideInput) {
                    setInput(originalInput);
                    setDraftImages(originalImages);
                    setImageUrls(originalImageUrls);
                    setDocumentText(originalDocumentText);
                    setDocumentName(originalDocumentName);
                  }

                  setError("Failed to send message. Please try again.");
                } else {
                  // Message actually went through! Just log it
                  console.log("Message found after retry failure - it actually went through");
                }
              } catch (finalCheckError) {
                // If we can't even check, assume message failed and restore input
                console.error("Final check failed:", finalCheckError);

                // Remove the user message from the messages list
                setMessages((prev) => prev.filter((msg) => msg.id !== localMessageId));

                // Restore the original input and attachments
                if (!overrideInput) {
                  setInput(originalInput);
                  setDraftImages(originalImages);
                  setImageUrls(originalImageUrls);
                  setDocumentText(originalDocumentText);
                  setDocumentName(originalDocumentName);
                }

                setError("Failed to send message. Please try again.");
              }
            }
          } else {
            // Not a follow-up conversation or other non-retryable error
            setError(errorMessage + ". Please try again.");
          }
        }
      } finally {
        setIsGenerating(false);
        setCurrentResponseId(undefined);
        abortControllerRef.current = null;
        assistantStreamingRef.current = false;
      }
    },
    [
      input,
      isGenerating,
      openai,
      conversation,
      localState.model,
      draftImages,
      documentText,
      clearAllAttachments,
      processStreamingResponse
    ]
  );

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSendMessage();
    }
  };

  return (
    <div className="grid h-screen w-full grid-cols-1 md:grid-cols-[280px_1fr]">
      {/* Use the existing Sidebar component */}
      <Sidebar chatId={chatId} isOpen={isSidebarOpen} onToggle={toggleSidebar} />

      {/* Main Content */}
      <div className="flex flex-col flex-1 min-w-0 bg-background overflow-hidden relative">
        {/* Error message - fixed at top below header, always visible */}
        {error && (
          <div className="fixed top-16 left-1/2 -translate-x-1/2 z-50 w-full max-w-2xl px-4 md:left-[calc(50%+140px)]">
            <Alert variant="destructive" className="bg-background">
              <AlertCircle className="h-4 w-4" />
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          </div>
        )}

        {/* Mobile sidebar toggle */}
        {!isSidebarOpen && (
          <div className="fixed top-[9.5px] left-4 z-20 md:hidden">
            <SidebarToggle onToggle={toggleSidebar} />
          </div>
        )}

        {/* Only show header when there are messages (conversation exists) */}
        {messages.length > 0 && (
          <div className="h-14 flex items-center px-4">
            <div className="flex-1 flex items-center justify-center relative">
              <h1
                className={`text-base font-medium truncate max-w-[20rem] text-foreground transition-colors duration-300 ${
                  titleJustUpdated ? "title-update-animation" : ""
                }`}
              >
                {conversation?.metadata?.title || "Chat"}
              </h1>
              {/* Mobile new chat button - positioned on the right */}
              <Button
                variant="outline"
                size="icon"
                className="md:hidden absolute right-0 h-9 w-9"
                onClick={() => {
                  // Clear conversation and start new chat
                  const usp = new URLSearchParams(window.location.search);
                  usp.delete("conversation_id");
                  const newUrl = usp.toString()
                    ? `${window.location.pathname}?${usp.toString()}`
                    : window.location.pathname;
                  window.history.replaceState(null, "", newUrl);
                  window.dispatchEvent(new Event("newchat"));
                  setChatId(undefined);
                  setConversation(null);
                  setMessages([]);
                  setLastSeenItemId(undefined);
                  // Clear pagination state
                  setOldestItemId(undefined);
                  setHasMoreOlderMessages(false);
                  setIsLoadingOlderMessages(false);
                  // Close sidebar if open
                  if (isSidebarOpen) {
                    toggleSidebar();
                  }
                }}
                aria-label="New chat"
              >
                <SquarePen className="h-4 w-4" />
              </Button>
            </div>
          </div>
        )}

        {/* Messages Area */}
        <div ref={chatContainerRef} className="flex-1 overflow-y-auto flex flex-col relative">
          {/* Only show messages when there are messages */}
          {messages.length > 0 && (
            <div className="max-w-4xl mx-auto p-6 w-full">
              {/* Message list with modern ChatGPT/Claude style */}
              <div className="space-y-1">
                <MessageList
                  messages={messages}
                  isGenerating={isGenerating}
                  chatId={chatId}
                  firstMessageRef={firstMessageRef}
                  isLoadingOlderMessages={isLoadingOlderMessages}
                />
              </div>

              <div ref={messagesEndRef} />
            </div>
          )}
        </div>

        {/* Input Area - centered when no messages, fixed at bottom when chatting */}
        {messages.length === 0 && !chatId ? (
          // Centered input for new chat
          <div className="absolute inset-0 flex flex-col justify-center px-4">
            <div className="w-full max-w-4xl mx-auto">
              {/* Logo section - raised higher */}
              <div className="flex flex-col items-center -mt-20 mb-16">
                {/* Logo with Maple - using the same images as TopNav */}
                <div className="flex items-center justify-center gap-2 mb-3">
                  <img src="/maple-icon-nobg.png" alt="" className="h-10 w-10" />
                  <img src="/maple-logo.svg" alt="Maple" className="w-32 hidden dark:block" />
                  <img src="/maple-logo-dark.svg" alt="Maple" className="w-32 block dark:hidden" />
                </div>

                {/* Subtitle right under the logo */}
                <p className="text-xl font-light text-muted-foreground">Private AI Chat</p>
              </div>

              {/* Main prompt section with more emphasis */}
              <div className="flex flex-col items-center gap-6">
                <h1 className="text-3xl font-medium text-foreground">How can I help you today?</h1>

                {/* Input form */}
                <form onSubmit={handleSendMessage} className="w-full relative">
                  <div className="space-y-2">
                    {/* Model selector and attachment button */}
                    <div className="flex items-center gap-2">
                      <ModelSelector
                        hasImages={
                          draftImages.length > 0 ||
                          messages.some((msg) =>
                            msg.content.some(
                              (part: ConversationContent) => part.type === "input_image"
                            )
                          )
                        }
                      />

                      {/* Attachment dropdown */}
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button
                            type="button"
                            variant="ghost"
                            size="sm"
                            className="h-8 w-8 p-0"
                            disabled={isProcessingDocument}
                          >
                            <Plus className="h-4 w-4" />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="start">
                          <DropdownMenuItem
                            onClick={() => {
                              if (!canUseImages) {
                                setUpgradeFeature("image");
                                setUpgradeDialogOpen(true);
                              } else {
                                fileInputRef.current?.click();
                              }
                            }}
                          >
                            <Image className="mr-2 h-4 w-4" />
                            <span>Add Images</span>
                          </DropdownMenuItem>
                          <DropdownMenuItem
                            onClick={() => {
                              if (!isTauriEnv) {
                                setDocumentPlatformDialogOpen(true);
                              } else if (!canUseDocuments) {
                                setUpgradeFeature("document");
                                setUpgradeDialogOpen(true);
                              } else {
                                documentInputRef.current?.click();
                              }
                            }}
                          >
                            <FileText className="mr-2 h-4 w-4" />
                            <span>Add Document</span>
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </div>

                    {/* Attachment previews */}
                    {(draftImages.length > 0 || documentName) && (
                      <div className="space-y-2">
                        {/* Image previews */}
                        {draftImages.length > 0 && (
                          <div className="flex gap-2 flex-wrap">
                            {draftImages.map((file, i) => (
                              <div key={i} className="relative group">
                                <img
                                  src={imageUrls.get(file) || ""}
                                  alt={`Attachment ${i + 1}`}
                                  className="w-16 h-16 object-cover rounded-md border"
                                />
                                <button
                                  type="button"
                                  onClick={() => removeImage(i)}
                                  className="absolute -top-1 -right-1 bg-background border rounded-full p-0.5 opacity-0 group-hover:opacity-100 transition-opacity"
                                >
                                  <X className="h-3 w-3" />
                                </button>
                              </div>
                            ))}
                          </div>
                        )}

                        {/* Document preview */}
                        {documentName && (
                          <div className="flex items-center gap-2 p-2 bg-muted/50 rounded-md">
                            <FileText className="h-4 w-4 text-muted-foreground" />
                            <span className="text-sm truncate flex-1">{documentName}</span>
                            <button
                              type="button"
                              onClick={removeDocument}
                              className="text-muted-foreground hover:text-foreground"
                            >
                              <X className="h-3 w-3" />
                            </button>
                          </div>
                        )}
                      </div>
                    )}

                    {/* Error display */}
                    {(attachmentError || audioError) && (
                      <div className="text-sm text-red-500 px-2">
                        {attachmentError || audioError}
                      </div>
                    )}

                    <div className="relative">
                      <Textarea
                        ref={textareaRef}
                        value={input}
                        onChange={(e) => setInput(e.target.value)}
                        onKeyDown={handleKeyDown}
                        placeholder="Message Maple..."
                        disabled={isGenerating || isRecording}
                        className="w-full resize-none min-h-[120px] max-h-[200px] pr-24 py-4 px-5 rounded-xl border bg-background focus:outline-none focus:ring-2 focus:ring-ring/20 placeholder:text-muted-foreground/60 text-base"
                        rows={4}
                        id="message"
                      />
                      {/* Mic button */}
                      <Button
                        type="button"
                        onClick={startRecording}
                        disabled={isGenerating || isRecording || !canUseVoice}
                        size="icon"
                        variant="ghost"
                        className="absolute bottom-3 right-14 h-9 w-9 rounded-lg hover:bg-muted"
                      >
                        <Mic className="h-4 w-4" />
                      </Button>
                      {/* Send/Stop button */}
                      {isGenerating ? (
                        <Button
                          type="button"
                          onClick={handleCancelResponse}
                          size="icon"
                          variant="destructive"
                          className="absolute bottom-3 right-3 h-9 w-9 rounded-lg"
                        >
                          <div className="h-3 w-3 bg-current rounded-sm" />
                        </Button>
                      ) : (
                        <Button
                          type="submit"
                          disabled={!input.trim() && !draftImages.length && !documentText}
                          size="icon"
                          className="absolute bottom-3 right-3 h-9 w-9 rounded-lg"
                        >
                          <Send className="h-4 w-4" />
                        </Button>
                      )}
                    </div>

                    {/* Recording overlay for centered input */}
                    {isRecording && (
                      <RecordingOverlay
                        isRecording={isRecording}
                        isProcessing={isProcessingSend || isTranscribing}
                        onSend={() => stopRecording(true)}
                        onCancel={() => stopRecording(false)}
                        isCompact={false}
                        className="absolute inset-0 rounded-xl"
                      />
                    )}
                  </div>
                </form>

                {/* Footer text */}
                <p className="text-sm text-center text-muted-foreground/60">
                  Encrypted at every step
                </p>
              </div>
            </div>
          </div>
        ) : (
          // Fixed at bottom when there are messages
          <div className="bg-background">
            <div className="max-w-4xl mx-auto p-4">
              <form onSubmit={handleSendMessage} className="relative">
                <div className="space-y-2">
                  {/* Model selector and attachment button */}
                  <div className="flex items-center gap-2">
                    <ModelSelector
                      hasImages={
                        draftImages.length > 0 ||
                        messages.some((msg) =>
                          msg.content.some(
                            (part: ConversationContent) => part.type === "input_image"
                          )
                        )
                      }
                    />

                    {/* Attachment dropdown */}
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <Button
                          type="button"
                          variant="ghost"
                          size="sm"
                          className="h-8 w-8 p-0"
                          disabled={isProcessingDocument}
                        >
                          <Plus className="h-4 w-4" />
                        </Button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="start">
                        <DropdownMenuItem
                          onClick={() => {
                            if (!canUseImages) {
                              setUpgradeFeature("image");
                              setUpgradeDialogOpen(true);
                            } else {
                              fileInputRef.current?.click();
                            }
                          }}
                        >
                          <Image className="mr-2 h-4 w-4" />
                          <span>Add Images</span>
                        </DropdownMenuItem>
                        <DropdownMenuItem
                          onClick={() => {
                            if (!isTauriEnv) {
                              setDocumentPlatformDialogOpen(true);
                            } else if (!canUseDocuments) {
                              setUpgradeFeature("document");
                              setUpgradeDialogOpen(true);
                            } else {
                              documentInputRef.current?.click();
                            }
                          }}
                        >
                          <FileText className="mr-2 h-4 w-4" />
                          <span>Add Document</span>
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </div>

                  {/* Attachment previews */}
                  {(draftImages.length > 0 || documentName) && (
                    <div className="space-y-2">
                      {/* Image previews */}
                      {draftImages.length > 0 && (
                        <div className="flex gap-2 flex-wrap">
                          {draftImages.map((file, i) => (
                            <div key={i} className="relative group">
                              <img
                                src={imageUrls.get(file) || ""}
                                alt={`Attachment ${i + 1}`}
                                className="w-12 h-12 object-cover rounded-md border"
                              />
                              <button
                                type="button"
                                onClick={() => removeImage(i)}
                                className="absolute -top-1 -right-1 bg-background border rounded-full p-0.5 opacity-0 group-hover:opacity-100 transition-opacity"
                              >
                                <X className="h-2.5 w-2.5" />
                              </button>
                            </div>
                          ))}
                        </div>
                      )}

                      {/* Document preview */}
                      {documentName && (
                        <div className="flex items-center gap-2 p-1.5 bg-muted/50 rounded-md text-xs">
                          <FileText className="h-3 w-3 text-muted-foreground" />
                          <span className="truncate flex-1">{documentName}</span>
                          <button
                            type="button"
                            onClick={removeDocument}
                            className="text-muted-foreground hover:text-foreground"
                          >
                            <X className="h-2.5 w-2.5" />
                          </button>
                        </div>
                      )}
                    </div>
                  )}

                  {/* Error display */}
                  {(attachmentError || audioError) && (
                    <div className="text-xs text-red-500 px-2">{attachmentError || audioError}</div>
                  )}

                  <div className="relative">
                    <Textarea
                      ref={textareaRef}
                      value={input}
                      onChange={(e) => setInput(e.target.value)}
                      onKeyDown={handleKeyDown}
                      placeholder="Message Maple..."
                      disabled={isGenerating || isRecording}
                      className="w-full resize-none min-h-[52px] max-h-[200px] pr-20 py-3 px-4 rounded-xl border bg-background focus:outline-none focus:ring-2 focus:ring-ring/20 placeholder:text-muted-foreground/60"
                      rows={1}
                      id="message"
                    />
                    {/* Mic button */}
                    <Button
                      type="button"
                      onClick={startRecording}
                      disabled={isGenerating || isRecording || !canUseVoice}
                      size="icon"
                      variant="ghost"
                      className="absolute bottom-[0.45rem] right-11 h-8 w-8 rounded-lg hover:bg-muted"
                    >
                      <Mic className="h-4 w-4" />
                    </Button>
                    {/* Send/Stop button */}
                    {isGenerating ? (
                      <Button
                        type="button"
                        onClick={handleCancelResponse}
                        size="icon"
                        variant="destructive"
                        className="absolute bottom-[0.45rem] right-2 h-8 w-8 rounded-lg"
                      >
                        <div className="h-3 w-3 bg-current rounded-sm" />
                      </Button>
                    ) : (
                      <Button
                        type="submit"
                        disabled={!input.trim() && !draftImages.length && !documentText}
                        size="icon"
                        className="absolute bottom-[0.45rem] right-2 h-8 w-8 rounded-lg"
                      >
                        <Send className="h-4 w-4" />
                      </Button>
                    )}
                  </div>

                  {/* Recording overlay for bottom input */}
                  {isRecording && (
                    <RecordingOverlay
                      isRecording={isRecording}
                      isProcessing={isProcessingSend || isTranscribing}
                      onSend={() => stopRecording(true)}
                      onCancel={() => stopRecording(false)}
                      isCompact={true}
                      className="absolute inset-0 rounded-xl"
                    />
                  )}
                </div>
              </form>
              <p className="text-sm text-center text-muted-foreground/60 mt-2">
                AI can make mistakes. Check important info.
              </p>
            </div>
          </div>
        )}

        {/* Upgrade dialog for attachments and usage limits */}
        <UpgradePromptDialog
          open={upgradeDialogOpen}
          onOpenChange={setUpgradeDialogOpen}
          feature={
            upgradeFeature === "document"
              ? "document"
              : upgradeFeature === "voice"
                ? "voice"
                : upgradeFeature === "usage"
                  ? "usage"
                  : upgradeFeature === "tokens"
                    ? "tokens"
                    : "image"
          }
        />

        {/* Document platform dialog for web users */}
        <DocumentPlatformDialog
          open={documentPlatformDialogOpen}
          onOpenChange={setDocumentPlatformDialogOpen}
          hasProAccess={canUseDocuments || false}
        />

        {/* Hidden file inputs - must be outside conditional rendering to work in both views */}
        <input
          type="file"
          ref={fileInputRef}
          accept="image/jpeg,image/jpg,image/png,image/webp"
          multiple
          onChange={handleAddImages}
          className="hidden"
        />
        <input
          type="file"
          ref={documentInputRef}
          accept=".pdf,.txt,.md"
          onChange={handleDocumentUpload}
          className="hidden"
        />
      </div>
    </div>
  );
}
