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
    [key: string]: any;
  };
}

export function UnifiedChat() {
  const isMobile = useIsMobile();
  const openai = useOpenAI();

  // Extract chatId from query params (e.g., ?conversation_id=xxx)
  // We're on the home page "/" so we only use query params for now
  const searchParams = new URLSearchParams(window.location.search);
  const chatId = searchParams.get("conversation_id") || undefined;

  // State
  const [conversation, setConversation] = useState<Conversation | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [isGenerating, setIsGenerating] = useState(false);
  const [isSidebarOpen, setIsSidebarOpen] = useState(!isMobile);
  const [error, setError] = useState<string | null>(null);
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

  // Listen for new chat event from sidebar
  useEffect(() => {
    const handleNewChat = () => {
      setConversation(null); // Clear the conversation
      setMessages([]);
      setInput("");
      setError(null);
    };

    window.addEventListener("newchat", handleNewChat);
    return () => window.removeEventListener("newchat", handleNewChat);
  }, []);

  // Clear messages and conversation when conversation_id is removed from URL
  useEffect(() => {
    if (!chatId) {
      // Only clear if we previously had a chatId (going from chat to new)
      setConversation(null); // Clear the conversation state
      setMessages([]);
    }
  }, [chatId]);

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
          setConversation(newConv as any);

          // Update URL with new conversation ID
          const usp = new URLSearchParams(window.location.search);
          usp.set("conversation_id", conversationId);
          window.history.replaceState(null, "", `${window.location.pathname}?${usp.toString()}`);
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

        // Initialize assistant message
        const assistantMessage: Message = {
          id: crypto.randomUUID(),
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
            // Update assistant message ID with server ID if available
            if (event.item?.id) {
              assistantMessage.id = event.item.id;
            }
          } else if (event.type === "response.output_text.delta" && event.delta) {
            // Accumulate text chunks
            accumulatedContent += event.delta;
            setMessages((prev) =>
              prev.map((msg) =>
                msg.id === assistantMessage.id ? { ...msg, content: accumulatedContent } : msg
              )
            );
          } else if (event.type === "response.output_item.done") {
            // Mark message as complete
            setMessages((prev) =>
              prev.map((msg) =>
                msg.id === assistantMessage.id ? { ...msg, status: "complete" } : msg
              )
            );
          } else if (event.type === "response.failed" || event.type === "error") {
            // Handle streaming errors
            console.error("Streaming error:", event);
            setMessages((prev) =>
              prev.map((msg) =>
                msg.id === assistantMessage.id ? { ...msg, status: "error" } : msg
              )
            );
            setError("Failed to generate response. Please try again.");
          }
        }
      } catch (error: any) {
        console.error("Failed to send message:", error);
        if (error.name !== "AbortError") {
          setError(error.message || "Something went wrong. Please try again.");
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
