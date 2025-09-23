import { useState, useRef, useEffect, useCallback } from "react";
import { Send, Bot, User, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Sidebar, SidebarToggle } from "@/components/Sidebar";
import { cn } from "@/utils/utils";
import { useIsMobile } from "@/utils/utils";

// Types
interface Message {
  id: string;
  role: "user" | "assistant";
  content: string;
  timestamp: number;
}

export function UnifiedChat() {
  const isMobile = useIsMobile();

  // Extract chatId from query params (e.g., ?conversation_id=xxx)
  // We're on the home page "/" so we only use query params for now
  const searchParams = new URLSearchParams(window.location.search);
  const chatId = searchParams.get("conversation_id") || undefined;

  // State - just local for now, will be replaced with OpenAI API
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [isGenerating, setIsGenerating] = useState(false);
  const [isSidebarOpen, setIsSidebarOpen] = useState(!isMobile);

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
      setMessages([]);
      setInput("");
    };

    window.addEventListener("newchat", handleNewChat);
    return () => window.removeEventListener("newchat", handleNewChat);
  }, []);

  // Clear messages when conversation_id is removed from URL
  useEffect(() => {
    if (!chatId) {
      // Only clear if we previously had a chatId (going from chat to new)
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
      if (!trimmedInput || isGenerating) return;

      // If no chat ID, create one and update URL without navigation
      if (!chatId) {
        const newChatId = `chat-${Date.now()}`;
        // First update with query param (doesn't require route to exist)
        const usp = new URLSearchParams(window.location.search);
        usp.set("conversation_id", newChatId);
        window.history.replaceState(null, "", `${window.location.pathname}?${usp.toString()}`);
      }

      // Add user message
      const userMessage: Message = {
        id: `msg-${Date.now()}`,
        role: "user",
        content: trimmedInput,
        timestamp: Date.now()
      };

      setMessages((prev) => [...prev, userMessage]);
      setInput("");
      setIsGenerating(true);

      // Mock AI response - will be replaced with OpenAI conversations API
      setTimeout(() => {
        const assistantMessage: Message = {
          id: `msg-${Date.now()}-ai`,
          role: "assistant",
          content:
            "Hello world! This is a mocked response. The OpenAI conversations API integration will be added here.",
          timestamp: Date.now()
        };

        setMessages((prev) => [...prev, assistantMessage]);
        setIsGenerating(false);
      }, 1000);
    },
    [input, isGenerating, chatId]
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

              {/* Loading indicator */}
              {isGenerating && (
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
