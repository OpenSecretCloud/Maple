import { useState, useRef, useEffect, useCallback } from "react";
import { Send, Bot, User, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Sidebar, SidebarToggle } from "@/components/Sidebar";
import { cn } from "@/utils/utils";
import { useIsMobile } from "@/utils/utils";
import { useOpenAI } from "@/ai/useOpenAi";
import { DEFAULT_MODEL_ID } from "@/state/LocalStateContext";

// Types
interface Message {
  id: string;
  role: "user" | "assistant";
  content: string;
  timestamp: number;
  status?: "complete" | "streaming" | "error";
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
  status?: "completed" | "in_progress";
  content?: Array<{
    type: "text" | "input_text";
    text?: string;
  }>;
  created_at?: number;
}

export function UnifiedChat() {
  const isMobile = useIsMobile();
  const openai = useOpenAI();

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
  const abortControllerRef = useRef<AbortController | null>(null);

  // Refs
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  // Auto-resize textarea
  useEffect(() => {
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
      const scrollHeight = textareaRef.current.scrollHeight;
      textareaRef.current.style.height = `${Math.min(scrollHeight, 200)}px`;
    }
  }, [input]);

  // Scroll to bottom when messages change
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

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
    };

    // Handle conversation selection from sidebar
    const handleConversationSelected = (event: CustomEvent) => {
      const { conversationId } = event.detail;
      if (conversationId && conversationId !== chatId) {
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
  }, [chatId]);

  // Load conversation from API
  const loadConversation = useCallback(
    async (conversationId: string) => {
      if (!openai) return;

      try {
        // Fetch conversation metadata
        const conv = await openai.conversations.retrieve(conversationId);
        setConversation(conv as Conversation);

        // Fetch all conversation items
        const itemsResponse = await openai.conversations.items.list(conversationId, {
          limit: 100 // Get up to 100 most recent items
        });

        // Convert items to messages
        const loadedMessages: Message[] = [];

        for (const item of itemsResponse.data) {
          if (item.type === "message" && item.role && item.content) {
            let text = "";
            if (Array.isArray(item.content)) {
              for (const part of item.content) {
                if ((part.type === "text" || part.type === "input_text") && part.text) {
                  text += part.text;
                }
              }
            } else if (typeof item.content === "string") {
              text = item.content;
            }

            if (text) {
              loadedMessages.push({
                id: item.id,
                role: item.role as "user" | "assistant",
                content: text,
                timestamp:
                  ((item as ConversationItem & { created_at?: number }).created_at ??
                    Date.now() / 1000) * 1000,
                status: "complete"
              });
            }
          }
        }

        setMessages(loadedMessages);

        // Set last seen ID for polling
        if (itemsResponse.data.length > 0) {
          const lastItem = itemsResponse.data[itemsResponse.data.length - 1];
          setLastSeenItemId(lastItem.id);
        }
      } catch (error) {
        const err = error as { status?: number; message?: string };
        if (err.status === 404) {
          // Conversation doesn't exist - clear and start fresh
          console.log("Conversation not found, starting new");
          setConversation(null);
          setMessages([]);
          setError(null);
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

  // Polling mechanism for conversation updates
  const pollForNewItems = useCallback(async () => {
    if (!conversation?.id || !openai || !lastSeenItemId) return;

    try {
      // Fetch items after the last seen ID
      const response = await openai.conversations.items.list(conversation.id, {
        after: lastSeenItemId,
        limit: 100
      });

      if (response.data.length > 0) {
        // Convert API items to UI messages
        const newMessages: Message[] = [];

        for (const item of response.data) {
          if (item.type === "message" && item.role && item.content) {
            let text = "";
            if (Array.isArray(item.content)) {
              for (const part of item.content) {
                if ((part.type === "text" || part.type === "input_text") && part.text) {
                  text += part.text;
                }
              }
            }

            if (text) {
              newMessages.push({
                id: item.id,
                role: item.role as "user" | "assistant",
                content: text,
                timestamp:
                  ((item as ConversationItem & { created_at?: number }).created_at ??
                    Date.now() / 1000) * 1000,
                status: "complete"
              });
            }
          }
        }

        if (newMessages.length > 0) {
          // Merge new messages with deduplication
          setMessages((prev) => {
            const existingIds = new Set(prev.map((m) => m.id));
            const existingSignatures = new Set(
              prev.map((m) => `${m.role}:${m.content.substring(0, 100)}`)
            );

            const uniqueNewMessages = newMessages.filter((m) => {
              // Skip if we already have this ID
              if (existingIds.has(m.id)) return false;

              // Skip if we have a message with same role and similar content
              const signature = `${m.role}:${m.content.substring(0, 100)}`;
              if (existingSignatures.has(signature)) return false;

              return true;
            });

            if (uniqueNewMessages.length === 0) return prev;

            // Replace local messages with server versions when they match
            const updatedMessages = prev.map((msg) => {
              // If this is a local message (UUID format)
              if (msg.id.includes("-") && msg.id.length === 36) {
                const serverVersion = uniqueNewMessages.find(
                  (newMsg) => newMsg.role === msg.role && newMsg.content === msg.content
                );
                if (serverVersion) {
                  // Remove from unique list to avoid duplication
                  uniqueNewMessages.splice(uniqueNewMessages.indexOf(serverVersion), 1);
                  return { ...msg, id: serverVersion.id };
                }
              }
              return msg;
            });

            return [...updatedMessages, ...uniqueNewMessages];
          });

          // Update last seen item ID
          const lastItem = response.data[response.data.length - 1];
          if (lastItem?.id) {
            setLastSeenItemId(lastItem.id);
          }

          // Check if we're no longer generating
          if (isGenerating && newMessages.some((m) => m.role === "assistant")) {
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
    if (chatId && openai) {
      // Load the conversation from URL
      loadConversation(chatId);
    } else if (!chatId) {
      // Clear if no conversation ID
      setConversation(null);
      setMessages([]);
      setLastSeenItemId(undefined);
    }
  }, [chatId, openai, loadConversation]);

  // Set up polling interval
  useEffect(() => {
    if (!conversation?.id || !openai) return;

    // Don't poll immediately - loadConversation already fetched everything
    // Start polling after 5 seconds to check for updates
    const intervalId = setInterval(pollForNewItems, 5000);

    return () => clearInterval(intervalId);
  }, [conversation?.id, openai, pollForNewItems]);

  // Toggle sidebar
  const toggleSidebar = useCallback(() => setIsSidebarOpen((prev) => !prev), []);

  // Send message handler
  const handleSendMessage = useCallback(
    async (e?: React.FormEvent) => {
      e?.preventDefault();

      const trimmedInput = input.trim();
      if (!trimmedInput || isGenerating || !openai) return;

      // Clear any previous error
      setError(null);

      // Add user message immediately
      const userMessage: Message = {
        id: crypto.randomUUID(),
        role: "user",
        content: trimmedInput,
        timestamp: Date.now(),
        status: "complete"
      };

      setMessages((prev) => [...prev, userMessage]);
      setInput("");
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

          // Update local state
          setChatId(conversationId);

          // Trigger sidebar refresh to show the new conversation
          window.dispatchEvent(new Event("conversationcreated"));
        }

        // Create abort controller for this request
        const abortController = new AbortController();
        abortControllerRef.current = abortController;

        // Create streaming response
        const stream = await openai.responses.create(
          {
            conversation: conversationId,
            model: DEFAULT_MODEL_ID, // Use the default model constant
            input: [{ role: "user", content: trimmedInput }],
            stream: true,
            store: true // Store in conversation history
          },
          { signal: abortController.signal }
        );

        // Initialize assistant message with local ID
        const localAssistantId = crypto.randomUUID();
        let serverItemId: string | undefined;
        const assistantMessage: Message = {
          id: localAssistantId,
          role: "assistant",
          content: "",
          timestamp: Date.now(),
          status: "streaming"
        };

        setMessages((prev) => [...prev, assistantMessage]);
        let accumulatedContent = "";

        // Process streaming events
        for await (const event of stream) {
          if (event.type === "response.output_item.added" && event.item?.type === "message") {
            // Store the server-assigned ID
            if ((event as { item_id?: string }).item_id) {
              serverItemId = (event as { item_id?: string }).item_id;
              // Update message with server ID
              setMessages((prev) =>
                prev.map((msg) =>
                  msg.id === localAssistantId ? { ...msg, id: serverItemId || msg.id } : msg
                )
              );
            }
          } else if (event.type === "response.output_text.delta" && event.delta) {
            // Accumulate text chunks
            accumulatedContent += event.delta;
            setMessages((prev) =>
              prev.map((msg) =>
                msg.id === (serverItemId || localAssistantId)
                  ? { ...msg, content: accumulatedContent }
                  : msg
              )
            );
          } else if (event.type === "response.output_item.done") {
            // Mark message as complete and update lastSeenItemId
            setMessages((prev) =>
              prev.map((msg) =>
                msg.id === (serverItemId || localAssistantId) ? { ...msg, status: "complete" } : msg
              )
            );
            if (serverItemId) {
              setLastSeenItemId(serverItemId);
            }
          } else if (event.type === "response.failed" || event.type === "error") {
            // Handle streaming errors
            console.error("Streaming error:", event);
            setMessages((prev) =>
              prev.map((msg) =>
                msg.id === (serverItemId || localAssistantId) ? { ...msg, status: "error" } : msg
              )
            );
            setError("Failed to generate response. Please try again.");
          }
        }
      } catch (error) {
        console.error("Failed to send message:", error);
        const errorMessage = error instanceof Error ? error.message : "Something went wrong";
        if (error instanceof Error && error.name !== "AbortError") {
          setError(errorMessage + ". Please try again.");
        }
      } finally {
        setIsGenerating(false);
        abortControllerRef.current = null;
      }
    },
    [input, isGenerating, openai, conversation]
  );

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSendMessage();
    }
  };

  const formatTime = (timestamp: number) => {
    return new Date(timestamp).toLocaleTimeString("en-US", {
      hour: "numeric",
      minute: "2-digit",
      hour12: true
    });
  };

  return (
    <div className="grid h-screen w-full grid-cols-1 md:grid-cols-[280px_1fr]">
      {/* Use the existing Sidebar component */}
      <Sidebar chatId={chatId} isOpen={isSidebarOpen} onToggle={toggleSidebar} />

      {/* Main Content */}
      <div className="flex flex-col flex-1 min-w-0 bg-card/90 backdrop-blur-lg bg-center overflow-hidden">
        {/* Mobile sidebar toggle */}
        {!isSidebarOpen && (
          <div className="fixed top-4 left-4 z-20 md:hidden">
            <SidebarToggle onToggle={toggleSidebar} />
          </div>
        )}

        {/* Header */}
        <div className="h-14 border-b bg-background/95 backdrop-blur flex items-center px-4">
          <div className="flex-1 flex items-center gap-2 justify-center">
            <Bot className="h-5 w-5 text-primary" />
            <h1 className="text-lg font-semibold">{chatId ? "Chat" : "New Chat"}</h1>
          </div>
        </div>

        {/* Messages Area */}
        <div className="flex-1 overflow-y-auto">
          <div className="max-w-3xl mx-auto p-6">
            {/* Error message */}
            {error && <div className="bg-red-50 text-red-600 p-3 rounded mb-4">{error}</div>}

            {/* Welcome message when no messages */}
            {messages.length === 0 && !isGenerating && (
              <div className="text-center py-24 space-y-4">
                <div className="mx-auto w-20 h-20 rounded-full bg-primary/10 flex items-center justify-center">
                  <Bot className="h-10 w-10 text-primary" />
                </div>
                <h1 className="text-3xl font-semibold">Welcome to Maple</h1>
                <p className="text-lg text-muted-foreground">Start a conversation below</p>
              </div>
            )}

            {/* Message list */}
            <div className="space-y-6">
              {messages.map((message) => (
                <div
                  key={message.id}
                  className={cn(
                    "flex gap-3",
                    message.role === "user" ? "justify-end" : "justify-start"
                  )}
                >
                  {/* Assistant avatar */}
                  {message.role === "assistant" && (
                    <div className="w-8 h-8 rounded-full bg-primary/10 flex items-center justify-center flex-shrink-0">
                      <Bot className="h-4 w-4 text-primary" />
                    </div>
                  )}

                  {/* Message bubble */}
                  <div
                    className={cn(
                      "max-w-[80%] rounded-2xl px-4 py-3",
                      message.role === "user" ? "bg-primary text-primary-foreground" : "bg-muted"
                    )}
                  >
                    <div className="whitespace-pre-wrap break-words">{message.content}</div>
                    <div
                      className={cn(
                        "text-xs mt-1",
                        message.role === "user"
                          ? "text-primary-foreground/70"
                          : "text-muted-foreground"
                      )}
                    >
                      {formatTime(message.timestamp)}
                    </div>
                  </div>

                  {/* User avatar */}
                  {message.role === "user" && (
                    <div className="w-8 h-8 rounded-full bg-muted flex items-center justify-center flex-shrink-0">
                      <User className="h-4 w-4" />
                    </div>
                  )}
                </div>
              ))}

              {/* Loading indicator - only show if generating and no streaming message yet */}
              {isGenerating &&
                !messages.some((m) => m.role === "assistant" && m.status === "streaming") && (
                  <div className="flex gap-3 justify-start">
                    <div className="w-8 h-8 rounded-full bg-primary/10 flex items-center justify-center flex-shrink-0">
                      <Bot className="h-4 w-4 text-primary" />
                    </div>
                    <div className="bg-muted rounded-2xl px-4 py-3">
                      <div className="flex items-center gap-1">
                        <div className="w-2 h-2 bg-foreground/60 rounded-full animate-pulse" />
                        <div className="w-2 h-2 bg-foreground/60 rounded-full animate-pulse delay-75" />
                        <div className="w-2 h-2 bg-foreground/60 rounded-full animate-pulse delay-150" />
                      </div>
                    </div>
                  </div>
                )}
            </div>

            <div ref={messagesEndRef} />
          </div>
        </div>

        {/* Input Area */}
        <div className="border-t bg-background/95 backdrop-blur p-4">
          <form onSubmit={handleSendMessage} className="max-w-3xl mx-auto">
            <div className="flex gap-2">
              <Textarea
                ref={textareaRef}
                value={input}
                onChange={(e) => setInput(e.target.value)}
                onKeyDown={handleKeyDown}
                placeholder="Type your message..."
                disabled={isGenerating}
                className="flex-1 resize-none min-h-[56px] max-h-[200px]"
                rows={1}
                id="message"
              />
              <Button
                type="submit"
                disabled={!input.trim() || isGenerating}
                size="icon"
                className="h-[56px] w-[56px]"
              >
                {isGenerating ? (
                  <Loader2 className="h-5 w-5 animate-spin" />
                ) : (
                  <Send className="h-5 w-5" />
                )}
              </Button>
            </div>
            <p className="text-xs text-muted-foreground text-center mt-2">
              Press Enter to send, Shift+Enter for new line
            </p>
          </form>
        </div>
      </div>
    </div>
  );
}
