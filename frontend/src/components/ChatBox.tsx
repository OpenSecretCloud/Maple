import { Blend, CornerRightUp } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { useEffect, useRef, useState } from "react";
import { useLocalState } from "@/state/useLocalState";
import { cn } from "@/utils/utils";
import { useQuery } from "@tanstack/react-query";
import { getBillingService } from "@/billing/billingService";
import { Route as ChatRoute } from "@/routes/_auth.chat.$chatId";

export default function Component({
  onSubmit,
  startTall
}: {
  onSubmit: (input: string) => void;
  startTall?: boolean;
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

  // Get the chatId from the route params, but handle the case where we're not in a chat route
  let chatId: string | undefined;
  try {
    chatId = ChatRoute.useParams().chatId;
  } catch {
    // We're not in a chat route (e.g., we're on the home page)
    chatId = undefined;
  }

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
    if (!inputValue.trim()) return;

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
      if (isMobile || e.shiftKey) {
        // On mobile or when Shift is pressed, allow newline
        return;
      } else {
        // On desktop without Shift, submit the form
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

  const isDisabled =
    freshBillingStatus !== undefined &&
    (!freshBillingStatus.can_chat ||
      (freshBillingStatus.chats_remaining !== null && freshBillingStatus.chats_remaining <= 0));
  const placeholderText = (() => {
    if (billingStatus === null || freshBillingStatus === undefined)
      return "Type your message here...";
    if (freshBillingStatus.can_chat === false) {
      return "You've used up all your chats. Upgrade to continue.";
    }
    return "Type your message here...";
  })();

  return (
    <form
      className={cn(
        "p-2 rounded-lg border border-primary bg-background/80 backdrop-blur-lg focus-within:ring-1 focus-within:ring-ring",
        isDisabled && "opacity-50"
      )}
      onSubmit={handleSubmit}
      onClick={(e) => {
        if (isDisabled) {
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
        disabled={isDisabled}
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
            {model.startsWith("ibnzterrell") ? (
              <span className="flex items-center gap-1.5">
                Llama 3.3 70B
                <span className="text-[9px] bg-primary/10 text-primary px-1 rounded-full font-medium">
                  New
                </span>
              </span>
            ) : (
              model
            )}
          </span>
        </div>
        <Button
          type="submit"
          size="sm"
          className="ml-auto gap-1.5"
          disabled={!inputValue.trim() || isDisabled}
        >
          <CornerRightUp className="size-3.5" />
        </Button>
      </div>
    </form>
  );
}
