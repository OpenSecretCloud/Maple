import { useState, useRef, useEffect, useCallback } from "react";
import { Send, ArrowLeft, Loader2, Image, X } from "lucide-react";
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
          <>
            {message.imageUrls && message.imageUrls.length > 0 && (
              <div className="flex gap-1.5 flex-wrap mb-2">
                {message.imageUrls.map((url, i) => (
                  <img
                    key={i}
                    src={url}
                    alt={`Attachment ${i + 1}`}
                    className="max-w-full rounded-lg"
                    style={{ maxHeight: "200px", objectFit: "contain" }}
                  />
                ))}
              </div>
            )}
            <p className="whitespace-pre-wrap">{message.content}</p>
          </>
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
  const [draftImages, setDraftImages] = useState<File[]>([]);
  const [imageUrls, setImageUrls] = useState<Map<File, string>>(new Map());
  const [attachmentError, setAttachmentError] = useState<string | null>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
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

  useEffect(() => {
    return () => {
      imageUrls.forEach((url) => URL.revokeObjectURL(url));
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleAddImages = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      if (!e.target.files) return;

      const supportedTypes = ["image/jpeg", "image/jpg", "image/png", "image/webp"];
      const maxSizeInBytes = 10 * 1024 * 1024;

      const validFiles = Array.from(e.target.files).filter((file) => {
        if (!supportedTypes.includes(file.type.toLowerCase())) {
          setAttachmentError("Only JPEG, PNG, and WebP images are supported");
          setTimeout(() => setAttachmentError(null), 5000);
          return false;
        }
        if (file.size > maxSizeInBytes) {
          setAttachmentError("Image too large (max 10MB)");
          setTimeout(() => setAttachmentError(null), 5000);
          return false;
        }
        return true;
      });

      const newUrlMap = new Map(imageUrls);
      validFiles.forEach((file) => {
        if (!newUrlMap.has(file)) {
          newUrlMap.set(file, URL.createObjectURL(file));
        }
      });
      setImageUrls(newUrlMap);
      setDraftImages((prev) => [...prev, ...validFiles]);
      e.target.value = "";
    },
    [imageUrls]
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

  const handleSend = useCallback(async () => {
    if (!input.trim() || isLoading) return;
    const msg = input;
    const images = [...draftImages];
    setInput("");
    setDraftImages([]);
    imageUrls.forEach((url) => URL.revokeObjectURL(url));
    setImageUrls(new Map());
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
    }
    await sendMessage(msg, images.length > 0 ? images : undefined);
  }, [input, isLoading, sendMessage, draftImages, imageUrls]);

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
        <div className="max-w-3xl mx-auto space-y-2">
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

          {attachmentError && <div className="text-xs text-red-500 px-1">{attachmentError}</div>}

          <div className="flex items-end gap-2">
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="h-10 w-10 rounded-full shrink-0 text-muted-foreground"
              onClick={() => fileInputRef.current?.click()}
              disabled={isLoading}
            >
              <Image className="h-5 w-5" />
            </Button>
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

        <input
          type="file"
          ref={fileInputRef}
          accept="image/jpeg,image/jpg,image/png,image/webp"
          multiple
          onChange={handleAddImages}
          className="hidden"
        />
      </div>
    </div>
  );
}
