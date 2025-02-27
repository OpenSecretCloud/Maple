import { Blend, CornerRightUp } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { useEffect, useRef, useState } from "react";
import { useLocalState } from "@/state/useLocalState";
import { cn } from "@/utils/utils";
import { useQuery } from "@tanstack/react-query";
import { getBillingService } from "@/billing/billingService";
import { BillingStatus } from "@/billing/billingApi";
import { Route as ChatRoute } from "@/routes/_auth.chat.$chatId";
import { ChatMessage } from "@/state/LocalStateContext";
import { useNavigate, useRouter } from "@tanstack/react-router";

// Rough token estimation function
function estimateTokenCount(text: string): number {
  // A very rough estimation: ~4 characters per token on average
  return Math.ceil(text.length / 4);
}

function TokenWarning({
  messages,
  currentInput,
  chatId,
  className,
  billingStatus
}: {
  messages: ChatMessage[];
  currentInput: string;
  chatId?: string;
  className?: string;
  billingStatus?: BillingStatus;
}) {
  const totalTokens =
    messages.reduce((acc, msg) => acc + estimateTokenCount(msg.content), 0) +
    (currentInput ? estimateTokenCount(currentInput) : 0);

  const navigate = useNavigate();

  // Check if user is on starter plan
  const isStarter = billingStatus?.product_name.toLowerCase().includes("starter") || false;

  // Token thresholds for different plan types
  const STARTER_WARNING_THRESHOLD = 4000;
  const PRO_WARNING_THRESHOLD = 10000;

  // Different thresholds for starter vs pro users
  const warningThreshold = isStarter ? STARTER_WARNING_THRESHOLD : PRO_WARNING_THRESHOLD;

  // Only show warning if above the threshold
  if (totalTokens < warningThreshold) return null;

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

  return (
    <div
      className={cn(
        "flex items-center justify-between px-3 py-1.5 mb-1",
        "bg-muted/50 backdrop-blur-sm rounded-t-lg",
        "text-xs text-muted-foreground/90",
        className
      )}
    >
      <div className="flex items-center gap-2 min-w-0">
        <span className="text-[11px] font-semibold text-foreground/70 shrink-0">Tip:</span>
        <span className="min-w-0">Long chats cause you to reach your usage limits faster.</span>
      </div>
      {chatId && (
        <button
          onClick={handleNewChat}
          className="font-medium text-primary hover:text-primary/80 hover:underline transition-colors whitespace-nowrap shrink-0 ml-4"
        >
          <span className="hidden md:inline">Start a new chat</span>
          <span className="md:hidden">New chat</span>
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
  isStreaming = false
}: {
  onSubmit: (input: string) => void;
  startTall?: boolean;
  messages?: ChatMessage[];
  isStreaming?: boolean;
}) {
  const [inputValue, setInputValue] = useState("");
  const {
    model,
    billingStatus,
    setBillingStatus,
    draftMessages,
    setDraftMessage,
    clearDraftMessage
  } = useLocalState();
  const [isFocused, setIsFocused] = useState(false);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const [isMobile, setIsMobile] = useState(false);
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

  useEffect(() => {
    const checkMobile = () => {
      setIsMobile(window.matchMedia("(max-width: 768px)").matches);
    };
    checkMobile();
    window.addEventListener("resize", checkMobile);
    return () => window.removeEventListener("resize", checkMobile);
  }, []);

  const handleSubmit = (e?: React.FormEvent) => {
    e?.preventDefault();
    if (!inputValue.trim() || isSubmitDisabled) return;

    // Clear the draft when submitting
    if (chatId) {
      try {
        clearDraftMessage(chatId);
        lastDraftRef.current = "";
        currentInputRef.current = "";
      } catch (error) {
        console.error("Failed to clear draft message:", error);
        // Continue with submission even if draft clearing fails
      }
    }

    onSubmit(inputValue.trim());
    setInputValue("");
  };

  // Keep currentInputRef in sync with inputValue
  useEffect(() => {
    currentInputRef.current = inputValue;
  }, [inputValue]);

  // Handle draft loading and saving only on chat switches
  useEffect(() => {
    // 1. Save draft from previous chat before switching
    if (previousChatIdRef.current && previousChatIdRef.current !== chatId) {
      const oldChatId = previousChatIdRef.current;
      const currentInput = currentInputRef.current.trim();
      if (currentInput !== "") {
        setDraftMessage(oldChatId, currentInput);
      } else {
        clearDraftMessage(oldChatId);
      }
    }

    // 2. Load draft for new chat
    if (chatId) {
      try {
        const draft = draftMessages.get(chatId) || "";
        setInputValue(draft);
        lastDraftRef.current = draft;
        currentInputRef.current = draft;
      } catch (error) {
        console.error("Failed to load draft message:", error);
        setInputValue("");
        lastDraftRef.current = "";
        currentInputRef.current = "";
      }
    }

    // Update previous chat id reference
    previousChatIdRef.current = chatId;
  }, [chatId, draftMessages, setDraftMessage, clearDraftMessage]);

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

  // Auto-resize effect
  useEffect(() => {
    if (inputRef.current) {
      inputRef.current.style.height = "auto";
      inputRef.current.style.height = `${inputRef.current.scrollHeight}px`;
    }
  }, [inputValue]);

  // No longer need token calculation or plan type check since we removed the hard limit
  // Just keeping the TokenWarning component which handles its own calculations

  // Determine when the submit button should be disabled
  const isSubmitDisabled =
    (freshBillingStatus !== undefined &&
      (!freshBillingStatus.can_chat ||
        (freshBillingStatus.chats_remaining !== null &&
          freshBillingStatus.chats_remaining <= 0))) ||
    isStreaming;

  // Disable the input box only when the user is out of chats or when streaming
  const isInputDisabled =
    (freshBillingStatus !== undefined &&
      (!freshBillingStatus.can_chat ||
        (freshBillingStatus.chats_remaining !== null &&
          freshBillingStatus.chats_remaining <= 0))) ||
    isStreaming;
  const placeholderText = (() => {
    if (billingStatus === null || freshBillingStatus === undefined)
      return "Type your message here...";
    if (freshBillingStatus.can_chat === false) {
      return "You've used up all your chats. Upgrade to continue.";
    }
    return "Type your message here...";
  })();

  return (
    <div className="flex flex-col w-full">
      <TokenWarning
        messages={messages}
        currentInput={inputValue}
        chatId={chatId}
        billingStatus={freshBillingStatus}
      />
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
          <div className="gap-2 text-xs opacity-50 flex items-center">
            <Blend className="size-3" />
            <span className={cn([startTall ? "" : "truncate max-w-[8rem]"])}>
              {model.startsWith("ibnzterrell") ? "Llama 3.3 70B" : model}
            </span>
          </div>
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
