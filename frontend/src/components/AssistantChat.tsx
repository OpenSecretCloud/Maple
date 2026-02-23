import { useState, useRef, useEffect, useCallback } from "react";
import { Send, ArrowLeft, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { useAgent, type AgentMessage } from "@/hooks/useAgent";
import { useRouter } from "@tanstack/react-router";
import { useOpenSecret } from "@opensecret/react";
import { updateAgentConfig } from "@opensecret/react";
import { cn } from "@/utils/utils";
import { Markdown } from "@/components/markdown";

function MessageBubble({ message }: { message: AgentMessage }) {
  const isUser = message.role === "user";

  return (
    <div className={cn("flex w-full mb-3", isUser ? "justify-end" : "justify-start")}>
      <div
        className={cn(
          "max-w-[85%] md:max-w-[70%] rounded-2xl px-4 py-2.5 text-sm leading-relaxed",
          isUser
            ? "bg-[hsl(var(--purple))] text-white rounded-br-md"
            : "bg-muted text-foreground rounded-bl-md"
        )}
      >
        {isUser ? (
          <p className="whitespace-pre-wrap">{message.content}</p>
        ) : (
          <div className="assistant-bubble-markdown">
            <Markdown content={message.content} />
          </div>
        )}
        <div
          className={cn(
            "text-[10px] mt-1",
            isUser ? "text-white/60 text-right" : "text-muted-foreground text-left"
          )}
        >
          {message.timestamp.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}
        </div>
      </div>
    </div>
  );
}

function TypingIndicator() {
  return (
    <div className="flex justify-start mb-3">
      <div className="bg-muted rounded-2xl rounded-bl-md px-4 py-3">
        <div className="flex gap-1.5 items-center">
          <div className="w-2 h-2 rounded-full bg-muted-foreground/40 animate-bounce [animation-delay:0ms]" />
          <div className="w-2 h-2 rounded-full bg-muted-foreground/40 animate-bounce [animation-delay:150ms]" />
          <div className="w-2 h-2 rounded-full bg-muted-foreground/40 animate-bounce [animation-delay:300ms]" />
        </div>
      </div>
    </div>
  );
}

export function AssistantChat() {
  const { messages, isLoading, isTyping, error, sendMessage, loadHistory } = useAgent();
  const [input, setInput] = useState("");
  const [initialized, setInitialized] = useState(false);
  const [initializing, setInitializing] = useState(true);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const router = useRouter();
  const os = useOpenSecret();

  useEffect(() => {
    async function init() {
      try {
        await updateAgentConfig({ enabled: true });
        await loadHistory();
        setInitialized(true);
      } catch {
        setInitialized(true);
      } finally {
        setInitializing(false);
      }
    }
    init();
  }, [loadHistory]);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, isTyping]);

  const handleSend = useCallback(async () => {
    if (!input.trim() || isLoading) return;
    const msg = input;
    setInput("");
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
    }
    await sendMessage(msg);
  }, [input, isLoading, sendMessage]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        handleSend();
      }
    },
    [handleSend]
  );

  const handleBack = useCallback(() => {
    router.navigate({ to: "/" });
  }, [router]);

  const handleTextareaInput = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setInput(e.target.value);
    const el = e.target;
    el.style.height = "auto";
    el.style.height = Math.min(el.scrollHeight, 120) + "px";
  }, []);

  if (!os.auth.user) {
    return null;
  }

  if (initializing) {
    return (
      <div className="flex items-center justify-center h-full bg-background">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full bg-background">
      {/* Header */}
      <div className="h-14 flex items-center px-4 border-b border-input bg-background/80 backdrop-blur-lg shrink-0">
        <Button variant="ghost" size="icon" className="h-9 w-9 mr-2" onClick={handleBack}>
          <ArrowLeft className="h-5 w-5" />
        </Button>
        <div className="flex items-center gap-3 flex-1 min-w-0">
          <div className="h-8 w-8 rounded-full bg-gradient-to-br from-[hsl(var(--purple))] to-[hsl(var(--blue))] flex items-center justify-center shrink-0">
            <span className="text-white text-xs font-semibold">A</span>
          </div>
          <div className="min-w-0">
            <h1 className="text-sm font-semibold truncate">Assistant</h1>
            <p className="text-[11px] text-muted-foreground">
              {isTyping ? "typing..." : "Maple AI"}
            </p>
          </div>
        </div>
      </div>

      {/* Messages area */}
      <div className="flex-1 overflow-y-auto px-4 py-4">
        {initialized && messages.length === 0 && !isLoading && (
          <div className="flex flex-col items-center justify-center h-full text-center px-6">
            <div className="h-16 w-16 rounded-full bg-gradient-to-br from-[hsl(var(--purple))] to-[hsl(var(--blue))] flex items-center justify-center mb-4">
              <span className="text-white text-2xl font-semibold">A</span>
            </div>
            <h2 className="text-lg font-semibold mb-2">Meet your Assistant</h2>
            <p className="text-sm text-muted-foreground max-w-sm">
              Your persistent AI assistant that remembers across conversations. Start a conversation
              and it will learn about you over time.
            </p>
          </div>
        )}

        {messages.map((msg) => (
          <MessageBubble key={msg.id} message={msg} />
        ))}

        {isTyping && <TypingIndicator />}

        {error && (
          <div className="flex justify-center mb-3">
            <div className="bg-destructive/10 text-destructive text-xs rounded-lg px-3 py-2 max-w-[80%]">
              {error}
            </div>
          </div>
        )}

        <div ref={messagesEndRef} />
      </div>

      {/* Input area */}
      <div className="shrink-0 border-t border-input bg-background/80 backdrop-blur-lg px-4 py-3 pb-[max(0.75rem,env(safe-area-inset-bottom))]">
        <div className="flex items-end gap-2 max-w-3xl mx-auto">
          <Textarea
            ref={textareaRef}
            value={input}
            onChange={handleTextareaInput}
            onKeyDown={handleKeyDown}
            placeholder="Message..."
            className="min-h-[40px] max-h-[120px] resize-none rounded-2xl border-input bg-muted/50 px-4 py-2.5 text-sm focus-visible:ring-1 focus-visible:ring-[hsl(var(--purple))]/50"
            rows={1}
            disabled={isLoading}
          />
          <Button
            onClick={handleSend}
            disabled={!input.trim() || isLoading}
            size="icon"
            className={cn(
              "h-10 w-10 rounded-full shrink-0 transition-colors",
              input.trim() && !isLoading
                ? "bg-[hsl(var(--purple))] hover:bg-[hsl(var(--purple))]/90 text-white"
                : "bg-muted text-muted-foreground"
            )}
          >
            {isLoading ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Send className="h-4 w-4" />
            )}
          </Button>
        </div>
      </div>
    </div>
  );
}
