import { CornerRightUp, Bot } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { useEffect, useRef, useState } from "react";
import { useLocalState } from "@/state/useLocalState";
import { cn, useIsMobile } from "@/utils/utils";
import { useQuery } from "@tanstack/react-query";
import { getBillingService } from "@/billing/billingService";
import { Route as ChatRoute } from "@/routes/_auth.chat.$chatId";
import { ChatMessage } from "@/state/LocalStateContext";
import { useNavigate, useRouter } from "@tanstack/react-router";
import { ModelSelector, getModelTokenLimit } from "@/components/ModelSelector";
import { encode } from "gpt-tokenizer";

// Accurate token counting using gpt-tokenizer
function estimateTokenCount(text: string): number {
  // Use gpt-tokenizer for accurate token counting
  return encode(text).length;
}

function TokenWarning({
  messages,
  currentInput,
  chatId,
  className,
  onCompress,
  isCompressing = false,
  modelId
}: {
  messages: ChatMessage[];
  currentInput: string;
  chatId?: string;
  className?: string;
  onCompress?: () => void;
  isCompressing?: boolean;
  modelId: string;
}) {
  const totalTokens =
    messages.reduce((acc, msg) => acc + estimateTokenCount(msg.content), 0) +
    (currentInput ? estimateTokenCount(currentInput) : 0);

  const navigate = useNavigate();

  // Get model-specific token limit
  const tokenLimit = getModelTokenLimit(modelId);
  const tokenPercentage = (totalTokens / tokenLimit) * 100;

  // Only show warning if above 50%
  if (tokenPercentage < 50) return null;

  // Determine the severity and behavior based on percentage
  const isAt95Percent = tokenPercentage >= 95;
  const isAt99Percent = tokenPercentage >= 99;

  const handleNewChat = async (e: React.MouseEvent) => {
    e.preventDefault();
    try {
      await navigate({ to: "/" });
      // Ensure element is available after navigation
      setTimeout(() => document.getElementById("message")?.focus(), 0);
    } catch (error) {
      console.error("Navigation failed:", error);
    }
  };

  // Get appropriate message and styling based on threshold
  const getMessage = () => {
    if (isAt99Percent) {
      return "This chat is too long to continue.";
    } else if (isAt95Percent) {
      return "Chat is at capacity. Compress to continue.";
    } else {
      return "This chat is getting long. Compress it to save tokens.";
    }
  };

  const getButtonText = () => {
    if (isCompressing) {
      return { desktop: "Compressing...", mobile: "Compressing..." };
    }
    if (onCompress) {
      return { desktop: "Compress chat", mobile: "Compress" };
    }
    return { desktop: "Start a new chat", mobile: "New chat" };
  };

  const buttonText = getButtonText();

  // Determine background color based on severity
  const bgClass = isAt99Percent
    ? "bg-destructive/20 border border-destructive/30"
    : isAt95Percent
      ? "bg-warning/20 border border-warning/30"
      : "bg-muted/50";

  return (
    <div
      className={cn(
        "flex items-center justify-between px-3 py-1.5 mb-1",
        "backdrop-blur-sm rounded-t-lg",
        "text-xs",
        bgClass,
        isAt99Percent
          ? "text-destructive"
          : isAt95Percent
            ? "text-warning-foreground"
            : "text-muted-foreground/90",
        className
      )}
    >
      <div className="flex items-center gap-2 min-w-0">
        <span className="text-[11px] font-semibold shrink-0">
          {isAt99Percent ? "Error:" : isAt95Percent ? "Warning:" : "Tip:"}
        </span>
        <span className="min-w-0">{getMessage()}</span>
      </div>
      {chatId && !isAt99Percent && (
        <button
          onClick={!isCompressing ? onCompress || handleNewChat : undefined}
          disabled={isCompressing}
          className={cn(
            "font-medium transition-colors whitespace-nowrap shrink-0 ml-4",
            isCompressing ? "opacity-70 cursor-default" : "hover:underline",
            isAt99Percent
              ? "text-destructive"
              : isAt95Percent
                ? "text-warning-foreground hover:text-warning-foreground/80"
                : "text-primary hover:text-primary/80"
          )}
        >
          <span className="hidden md:inline">{buttonText.desktop}</span>
          <span className="md:hidden">{buttonText.mobile}</span>
          <span className="sr-only">, to reduce token usage</span>
        </button>
      )}
    </div>
  );
}

export default function Component({
  onSubmit,
  startTall,
  messages = [],
  isStreaming = false,
  onCompress,
  isSummarizing = false
}: {
  onSubmit: (input: string, systemPrompt?: string) => void;
  startTall?: boolean;
  messages?: ChatMessage[];
  isStreaming?: boolean;
  onCompress?: () => void;
  isSummarizing?: boolean;
}) {
  const [inputValue, setInputValue] = useState("");
  const [systemPromptValue, setSystemPromptValue] = useState("");
  const [isSystemPromptExpanded, setIsSystemPromptExpanded] = useState(false);
  const {
    billingStatus,
    setBillingStatus,
    draftMessages,
    setDraftMessage,
    clearDraftMessage,
    model
  } = useLocalState();
  const [isFocused, setIsFocused] = useState(false);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const systemPromptRef = useRef<HTMLTextAreaElement>(null);
  const lastDraftRef = useRef<string>("");
  const previousChatIdRef = useRef<string | undefined>(undefined);
  const currentInputRef = useRef<string>("");

  // Get the chatId from the current route state
  const router = useRouter();
  const chatId = router.state.matches.find((m) => m.routeId === ChatRoute.id)?.params?.chatId as
    | string
    | undefined;

  const { data: freshBillingStatus } = useQuery({
    queryKey: ["billingStatus"],
    queryFn: async () => {
      const billingService = getBillingService();
      const status = await billingService.getBillingStatus();
      setBillingStatus(status);
      return status;
    }
  });

  // Use the centralized hook for mobile detection directly
  const isMobile = useIsMobile();

  // Check if user can use system prompts (paid users only - exclude free plans)
  const canUseSystemPrompt =
    freshBillingStatus && !freshBillingStatus.product_name?.toLowerCase().includes("free");

  // Check if system prompt can be edited (only for new chats)
  const canEditSystemPrompt = canUseSystemPrompt && messages.length === 0;

  const handleSubmit = (e?: React.FormEvent) => {
    e?.preventDefault();
    if (!inputValue.trim() || isSubmitDisabled) return;

    // Clear the drafts when submitting
    if (chatId) {
      try {
        clearDraftMessage(chatId);
        lastDraftRef.current = "";
        currentInputRef.current = "";
      } catch (error) {
        console.error("Failed to clear draft messages:", error);
        // Continue with submission even if draft clearing fails
      }
    }

    // Only pass system prompt if this is the first message
    const isFirstMessage = messages.length === 0;
    onSubmit(inputValue.trim(), isFirstMessage ? systemPromptValue.trim() || undefined : undefined);
    setInputValue("");

    // Re-focus input after submitting
    setTimeout(() => {
      inputRef.current?.focus();
    }, 0);
  };

  // Keep currentInputRef in sync with inputValue
  useEffect(() => {
    currentInputRef.current = inputValue;
  }, [inputValue]);

  // Handle draft loading and saving only on chat switches
  useEffect(() => {
    // 1. Save drafts from previous chat before switching
    if (previousChatIdRef.current && previousChatIdRef.current !== chatId) {
      const oldChatId = previousChatIdRef.current;

      // Save message draft
      const currentInput = currentInputRef.current.trim();
      if (currentInput !== "") {
        setDraftMessage(oldChatId, currentInput);
      } else {
        clearDraftMessage(oldChatId);
      }
    }

    // 2. Load drafts for new chat
    if (chatId) {
      try {
        // Load message draft
        const draft = draftMessages.get(chatId) || "";
        setInputValue(draft);
        lastDraftRef.current = draft;
        currentInputRef.current = draft;

        // Reset system prompt for new chat
        setSystemPromptValue("");
        setIsSystemPromptExpanded(false);
      } catch (error) {
        console.error("Failed to load draft messages:", error);
        setInputValue("");
        setSystemPromptValue("");
        lastDraftRef.current = "";
        currentInputRef.current = "";
      }
    }

    // Update previous chat id reference
    previousChatIdRef.current = chatId;
  }, [chatId, draftMessages, setDraftMessage, clearDraftMessage, canEditSystemPrompt, messages]);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter") {
      if (isMobile || e.shiftKey || isStreaming) {
        // On mobile, when Shift is pressed, or when streaming, allow newline
        return;
      } else if (isSubmitDisabled || !inputValue.trim()) {
        // Prevent form submission when disabled or empty input
        e.preventDefault();
        return;
      } else {
        // On desktop without Shift and not streaming, submit the form
        e.preventDefault();
        handleSubmit();
      }
    }
  };

  // Auto-resize effect for main input
  useEffect(() => {
    if (inputRef.current) {
      inputRef.current.style.height = "auto";
      inputRef.current.style.height = `${inputRef.current.scrollHeight}px`;
    }
  }, [inputValue]);

  // Auto-resize effect for system prompt
  useEffect(() => {
    if (systemPromptRef.current) {
      systemPromptRef.current.style.height = "auto";
      systemPromptRef.current.style.height = `${systemPromptRef.current.scrollHeight}px`;
    }
  }, [systemPromptValue]);

  // Calculate token usage percentage
  const totalTokens =
    messages.reduce((acc, msg) => acc + estimateTokenCount(msg.content), 0) +
    (inputValue ? estimateTokenCount(inputValue) : 0);
  const tokenLimit = getModelTokenLimit(model);
  const tokenPercentage = (totalTokens / tokenLimit) * 100;
  const isAt99Percent = tokenPercentage >= 99;

  // Determine when the submit button should be disabled
  const isSubmitDisabled =
    (freshBillingStatus !== undefined &&
      (!freshBillingStatus.can_chat ||
        (freshBillingStatus.chats_remaining !== null &&
          freshBillingStatus.chats_remaining <= 0))) ||
    isStreaming ||
    isAt99Percent;

  // Disable the input box only when the user is out of chats or when streaming
  const isInputDisabled =
    (freshBillingStatus !== undefined &&
      (!freshBillingStatus.can_chat ||
        (freshBillingStatus.chats_remaining !== null &&
          freshBillingStatus.chats_remaining <= 0))) ||
    isStreaming;

  // Auto-focus effect - runs on mount, when chat ID changes, and after streaming completes
  useEffect(() => {
    // Skip if user is already focused on an input elsewhere
    if (document.activeElement?.matches("input, textarea")) {
      return;
    }

    // Short delay to ensure DOM is ready
    const timer = setTimeout(() => {
      // Only focus if input isn't disabled
      if (inputRef.current && !isInputDisabled) {
        inputRef.current.focus();
      }
    }, 100);

    return () => clearTimeout(timer);
  }, [chatId, isStreaming, isInputDisabled]); // Re-run when chat ID changes, streaming completes, or input state changes

  // No longer need token calculation or plan type check since we removed the hard limit
  // Just keeping the TokenWarning component which handles its own calculations
  const placeholderText = (() => {
    if (isAt99Percent) {
      return "Chat is too long to continue.";
    }
    if (billingStatus === null || freshBillingStatus === undefined)
      return "Type your message here...";
    if (freshBillingStatus.can_chat === false) {
      return "You've used up all your messages. Upgrade to continue.";
    }
    return "Type your message here...";
  })();

  return (
    <div className="flex flex-col w-full">
      <TokenWarning
        messages={messages}
        currentInput={inputValue}
        chatId={chatId}
        onCompress={onCompress}
        isCompressing={isSummarizing}
        modelId={model}
      />

      {/* Simple System Prompt Section - just a gear button and input when expanded */}
      {canEditSystemPrompt && (
        <div className="mb-2">
          <div className="flex items-center gap-2 mb-1">
            <button
              type="button"
              onClick={() => setIsSystemPromptExpanded(!isSystemPromptExpanded)}
              className="flex items-center gap-1.5 text-xs font-medium transition-colors text-muted-foreground hover:text-foreground cursor-pointer"
              title="System Prompt"
            >
              <Bot className="size-6" />
              {systemPromptValue.trim() && (
                <div className="size-2 bg-primary rounded-full" title="System prompt active" />
              )}
            </button>
          </div>

          {isSystemPromptExpanded && (
            <textarea
              ref={systemPromptRef}
              value={systemPromptValue}
              onChange={(e) => setSystemPromptValue(e.target.value)}
              placeholder="Enter instructions for the AI (e.g., 'You are a helpful coding assistant...')"
              rows={2}
              className="w-full p-2 text-sm border border-muted-foreground/20 rounded-md bg-muted/50 placeholder:text-muted-foreground/70 focus:outline-none focus:ring-1 focus:ring-ring resize-none transition-colors"
              style={{
                height: "auto",
                resize: "none",
                overflowY: "auto",
                maxHeight: "8rem",
                minHeight: "3rem"
              }}
            />
          )}
        </div>
      )}

      <form
        className={cn(
          "p-2 rounded-lg border border-primary bg-background/80 backdrop-blur-lg focus-within:ring-1 focus-within:ring-ring",
          isInputDisabled && "opacity-50"
        )}
        onSubmit={handleSubmit}
        onClick={(e) => {
          if (isInputDisabled) {
            e.preventDefault();
            return;
          }
          if (!isFocused) {
            inputRef.current?.focus();
          }
        }}
      >
        <Label htmlFor="message" className="sr-only">
          Message
        </Label>
        <textarea
          disabled={isInputDisabled}
          ref={inputRef}
          onKeyDown={handleKeyDown}
          onFocus={() => setIsFocused(true)}
          onBlur={() => setIsFocused(false)}
          id="message"
          name="message"
          autoComplete="off"
          placeholder={placeholderText}
          rows={1}
          style={{
            height: "auto",
            resize: "none",
            overflowY: "auto",
            maxHeight: "12rem",
            ...(startTall ? { minHeight: "6rem" } : {})
          }}
          className={cn(
            "flex w-full ring-offset-background bg-background/0",
            "placeholder:text-muted-foreground focus-visible:outline-none",
            "disabled:cursor-not-allowed disabled:opacity-50",
            "!border-0 shadow-none !border-none focus-visible:ring-0 !ring-0",
            billingStatus === null && "animate-pulse"
          )}
          value={inputValue}
          onChange={(e) => setInputValue(e.target.value)}
        />
        <div className="flex items-center pt-0">
          <ModelSelector />
          <Button
            type="submit"
            size="sm"
            className="ml-auto gap-1.5"
            disabled={!inputValue.trim() || isSubmitDisabled}
          >
            <CornerRightUp className="size-3.5" />
          </Button>
        </div>
      </form>
    </div>
  );
}
