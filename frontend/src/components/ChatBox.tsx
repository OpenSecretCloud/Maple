import { CornerRightUp, Bot, ImageIcon, X, FileText, Loader2, Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger
} from "@/components/ui/dropdown-menu";
import { useEffect, useRef, useState } from "react";
import { useLocalState } from "@/state/useLocalState";
import { cn, useIsMobile } from "@/utils/utils";
import { useQuery } from "@tanstack/react-query";
import { getBillingService } from "@/billing/billingService";
import { BillingStatus } from "@/billing/billingApi";
import { Route as ChatRoute } from "@/routes/_auth.chat.$chatId";
import { ChatMessage } from "@/state/LocalStateContext";
import { useNavigate, useRouter } from "@tanstack/react-router";
import { ModelSelector, MODEL_CONFIG } from "@/components/ModelSelector";
import { useOpenSecret } from "@opensecret/react";
import type { DocumentResponse } from "@opensecret/react";

interface ParsedDocument {
  document: {
    filename: string;
    md_content: string | null;
    json_content: string | null;
    html_content: string | null;
    text_content: string | null;
    doctags_content: string | null;
  };
  status: string;
  errors: unknown[];
  processing_time: number;
  timings: Record<string, unknown>;
}

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
  billingStatus,
  onCompress,
  isCompressing = false
}: {
  messages: ChatMessage[];
  currentInput: string;
  chatId?: string;
  className?: string;
  billingStatus?: BillingStatus;
  onCompress?: () => void;
  isCompressing?: boolean;
}) {
  const totalTokens =
    messages.reduce((acc, msg) => {
      if (typeof msg.content === "string") {
        return acc + estimateTokenCount(msg.content);
      } else {
        // For multimodal content, estimate tokens from text parts
        return (
          acc +
          msg.content.reduce((sum, part) => {
            if (part.type === "text") {
              return sum + estimateTokenCount(part.text);
            }
            // Rough estimate for images
            return sum + 85;
          }, 0)
        );
      }
    }, 0) + (currentInput ? estimateTokenCount(currentInput) : 0);

  const navigate = useNavigate();

  // Check if user is on starter plan
  const isStarter = billingStatus?.product_name?.toLowerCase().includes("starter") || false;

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

  // Determine button text based on compression state
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
        <span className="min-w-0">This chat is getting long. Compress it to save tokens.</span>
      </div>
      {chatId && (
        <button
          onClick={!isCompressing ? onCompress || handleNewChat : undefined}
          disabled={isCompressing}
          className={cn(
            "font-medium text-primary transition-colors whitespace-nowrap shrink-0 ml-4",
            isCompressing ? "opacity-70 cursor-default" : "hover:text-primary/80 hover:underline"
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
  isSummarizing = false,
  imageConversionError
}: {
  onSubmit: (
    input: string,
    systemPrompt?: string,
    images?: File[],
    documentText?: string,
    documentMetadata?: { filename: string; fullContent: string }
  ) => void;
  startTall?: boolean;
  messages?: ChatMessage[];
  isStreaming?: boolean;
  onCompress?: () => void;
  isSummarizing?: boolean;
  imageConversionError?: string | null;
}) {
  const [inputValue, setInputValue] = useState("");
  const [systemPromptValue, setSystemPromptValue] = useState("");
  const [isSystemPromptExpanded, setIsSystemPromptExpanded] = useState(false);
  const { billingStatus, setBillingStatus, draftMessages, setDraftMessage, clearDraftMessage } =
    useLocalState();
  const { model } = useLocalState();

  const isGemma = MODEL_CONFIG[model]?.supportsVision || false;
  const [images, setImages] = useState<File[]>([]);
  const [imageUrls, setImageUrls] = useState<Map<File, string>>(new Map());
  const [uploadedDocument, setUploadedDocument] = useState<{
    original: DocumentResponse;
    parsed: ParsedDocument;
    cleanedText: string;
  } | null>(null);
  const [isUploadingDocument, setIsUploadingDocument] = useState(false);
  const [documentError, setDocumentError] = useState<string | null>(null);
  const [imageError, setImageError] = useState<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const documentInputRef = useRef<HTMLInputElement>(null);
  const os = useOpenSecret();

  const handleAddImages = (e: React.ChangeEvent<HTMLInputElement>) => {
    if (!e.target.files) return;

    const supportedTypes = ["image/jpeg", "image/jpg", "image/png", "image/webp"];
    const maxSizeInBytes = 5 * 1024 * 1024; // 5MB for images
    const errors: string[] = [];

    const validFiles = Array.from(e.target.files).filter((file) => {
      // Check file type
      if (!supportedTypes.includes(file.type.toLowerCase())) {
        return false;
      }

      // Check file size
      if (file.size > maxSizeInBytes) {
        const sizeInMB = (file.size / (1024 * 1024)).toFixed(2);
        errors.push(`${file.name} is too large (${sizeInMB}MB)`);
        return false;
      }

      return true;
    });

    if (validFiles.length < e.target.files.length) {
      const skippedCount = e.target.files.length - validFiles.length;
      const typeErrors = e.target.files.length - validFiles.length - errors.length;

      if (errors.length > 0) {
        setImageError(`${errors.join(", ")}. Max size is 5MB per image.`);
      } else if (typeErrors > 0) {
        setImageError(
          `${skippedCount} file(s) skipped. Only JPEG, PNG, and WebP images are supported.`
        );
      }
      // Clear error after 5 seconds
      setTimeout(() => setImageError(null), 5000);
    } else {
      setImageError(null);
    }

    // Create object URLs for the new images
    const newUrlMap = new Map(imageUrls);
    validFiles.forEach((file) => {
      if (!newUrlMap.has(file)) {
        newUrlMap.set(file, URL.createObjectURL(file));
      }
    });
    setImageUrls(newUrlMap);
    setImages((prev) => [...prev, ...validFiles]);
  };

  const removeImage = (idx: number) => {
    setImages((prev) => {
      const fileToRemove = prev[idx];
      // Revoke the object URL when removing the image
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
    // Clear any image errors when removing images
    setImageError(null);
  };

  const handleDocumentUpload = async (e: React.ChangeEvent<HTMLInputElement>) => {
    if (!e.target.files || e.target.files.length === 0) return;

    const file = e.target.files[0];

    // Check file size (1MB limit = 1024 * 1024 bytes)
    const maxSizeInBytes = 1 * 1024 * 1024; // 1MB
    if (file.size > maxSizeInBytes) {
      const sizeInMB = (file.size / (1024 * 1024)).toFixed(2);
      setDocumentError(`File too large (${sizeInMB}MB). Maximum size is 1MB.`);
      e.target.value = ""; // Reset input
      return;
    }

    setIsUploadingDocument(true);
    setDocumentError(null);

    try {
      const result = await os.uploadDocument(file);

      // Parse the JSON response
      const parsed = JSON.parse(result.text) as ParsedDocument;

      // Extract content with fallbacks (currently not used since we pass the full JSON)
      // const content =
      //   parsed.document.md_content ||
      //   parsed.document.json_content ||
      //   parsed.document.html_content ||
      //   parsed.document.text_content ||
      //   parsed.document.doctags_content ||
      //   "";

      // Create a cleaned version of the parsed document with image tags stripped from md_content
      const cleanedParsed = {
        ...parsed,
        document: {
          ...parsed.document,
          md_content: parsed.document.md_content
            ? parsed.document.md_content.replace(/!\[Image\]\([^)]+\)/g, "")
            : parsed.document.md_content
        }
      };

      setUploadedDocument({
        original: result,
        parsed: parsed,
        cleanedText: JSON.stringify(cleanedParsed) // Store the cleaned JSON as a string
      });
    } catch (error) {
      console.error("Document upload failed:", error);
      if (error instanceof Error) {
        if (error.message.includes("exceeds maximum limit")) {
          setDocumentError("File too large. Maximum size is 10MB.");
        } else if (error.message.includes("401")) {
          setDocumentError("Authentication required. Please log in to upload documents.");
        } else if (error.message.includes("403")) {
          setDocumentError("Usage limit exceeded. Please upgrade your plan.");
        } else {
          setDocumentError("Failed to process document. Please try again.");
        }
      } else {
        setDocumentError("An unexpected error occurred.");
      }
    } finally {
      setIsUploadingDocument(false);
      if (e.target) e.target.value = "";
    }
  };

  const removeDocument = () => {
    setUploadedDocument(null);
    setDocumentError(null);
  };
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

  // Check if user has access to Pro/Team features (Pro or Team plan)
  const hasProTeamAccess =
    freshBillingStatus &&
    (freshBillingStatus.product_name?.toLowerCase().includes("pro") ||
      freshBillingStatus.product_name?.toLowerCase().includes("team"));

  const canUseVision = isGemma && hasProTeamAccess;
  const canUseDocuments = hasProTeamAccess;

  const handleSubmit = (e?: React.FormEvent) => {
    e?.preventDefault();

    // Allow submission if there's text input, images, or a document
    const hasContent = inputValue.trim() || images.length > 0 || uploadedDocument;
    if (!hasContent || isSubmitDisabled) return;

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
    onSubmit(
      inputValue.trim(),
      isFirstMessage ? systemPromptValue.trim() || undefined : undefined,
      images,
      uploadedDocument?.cleanedText, // Now contains the full JSON with cleaned md_content
      uploadedDocument
        ? {
            filename: uploadedDocument.parsed.document.filename,
            fullContent:
              uploadedDocument.parsed.document.md_content ||
              uploadedDocument.parsed.document.json_content ||
              uploadedDocument.parsed.document.html_content ||
              uploadedDocument.parsed.document.text_content ||
              uploadedDocument.parsed.document.doctags_content ||
              ""
          }
        : undefined
    );
    setInputValue("");

    // Clean up image URLs when clearing images
    imageUrls.forEach((url) => URL.revokeObjectURL(url));
    setImageUrls(new Map());
    setImages([]);

    setUploadedDocument(null);
    setDocumentError(null);
    setImageError(null);

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

  // Cleanup effect for object URLs
  useEffect(() => {
    return () => {
      // Revoke all object URLs when component unmounts
      imageUrls.forEach((url) => URL.revokeObjectURL(url));
    };
  }, [imageUrls]);

  // No longer need token calculation or plan type check since we removed the hard limit
  // Just keeping the TokenWarning component which handles its own calculations
  const placeholderText = (() => {
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
        billingStatus={freshBillingStatus}
        onCompress={onCompress}
        isCompressing={isSummarizing}
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
              aria-label="Toggle system prompt"
              aria-expanded={isSystemPromptExpanded}
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
        {(images.length > 0 ||
          uploadedDocument ||
          documentError ||
          imageError ||
          imageConversionError) && (
          <div className="mb-2 space-y-2">
            {images.length > 0 && (
              <div className="flex gap-2 items-center flex-wrap">
                {images.map((f, i) => (
                  <div key={i} className="relative">
                    <img
                      src={imageUrls.get(f) || ""}
                      className="w-10 h-10 object-cover rounded-md"
                      alt={`Uploaded image ${i + 1}`}
                    />
                    <button
                      type="button"
                      onClick={() => removeImage(i)}
                      className="absolute -top-1 -right-1 bg-background rounded-full shadow-sm"
                      aria-label={`Remove image ${i + 1}`}
                    >
                      <X className="h-3 w-3" />
                    </button>
                  </div>
                ))}
              </div>
            )}
            {(imageError || imageConversionError) && (
              <div className="text-xs text-destructive p-2 bg-destructive/10 rounded-md">
                {imageError || imageConversionError}
              </div>
            )}
            {isUploadingDocument && !uploadedDocument && (
              <div className="flex items-center gap-2 p-2 bg-muted/50 rounded-md animate-in fade-in duration-200">
                <Loader2 className="h-4 w-4 text-muted-foreground animate-spin" />
                <span className="text-sm text-muted-foreground">Processing document...</span>
              </div>
            )}
            {uploadedDocument && (
              <div className="flex items-center gap-2 p-2 bg-muted/50 rounded-md">
                <FileText className="h-4 w-4 text-muted-foreground" />
                <span className="text-sm truncate flex-1">
                  {uploadedDocument.parsed.document.filename} (
                  {Math.round(uploadedDocument.original.size / 1024)}KB)
                </span>
                <button
                  type="button"
                  onClick={removeDocument}
                  className="text-muted-foreground hover:text-foreground"
                  aria-label="Remove document"
                >
                  <X className="h-3 w-3" />
                </button>
              </div>
            )}
            {documentError && (
              <div className="text-xs text-destructive p-2 bg-destructive/10 rounded-md">
                {documentError}
              </div>
            )}
          </div>
        )}
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
          <ModelSelector messages={messages} draftImages={images} />

          {/* Hidden file inputs */}
          <input
            type="file"
            accept="image/jpeg,image/jpg,image/png,image/webp"
            multiple
            ref={fileInputRef}
            onChange={handleAddImages}
            className="hidden"
          />
          <input
            type="file"
            accept=".pdf,.doc,.docx,.txt,.rtf,.xlsx,.xls,.pptx,.ppt"
            ref={documentInputRef}
            onChange={handleDocumentUpload}
            className="hidden"
          />

          {/* Consolidated upload button */}
          {(canUseVision || (canUseDocuments && !uploadedDocument)) && (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  className="ml-2"
                  aria-label="Upload files"
                >
                  <Plus className="h-4 w-4" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-48">
                {canUseVision && (
                  <DropdownMenuItem
                    onClick={() => fileInputRef.current?.click()}
                    className="flex items-center gap-2"
                  >
                    <ImageIcon className="h-4 w-4" />
                    <span>Upload Images</span>
                    <span className="ml-auto text-xs text-muted-foreground">
                      {isGemma ? "Vision model" : "Needs vision model"}
                    </span>
                  </DropdownMenuItem>
                )}
                {canUseDocuments && !uploadedDocument && (
                  <DropdownMenuItem
                    onClick={() => documentInputRef.current?.click()}
                    disabled={isUploadingDocument}
                    className="flex items-center gap-2"
                  >
                    {isUploadingDocument ? (
                      <Loader2 className="h-4 w-4 animate-spin" />
                    ) : (
                      <FileText className="h-4 w-4" />
                    )}
                    <span>Upload Document</span>
                    {isUploadingDocument && (
                      <span className="ml-auto text-xs text-muted-foreground">Processing...</span>
                    )}
                  </DropdownMenuItem>
                )}
              </DropdownMenuContent>
            </DropdownMenu>
          )}

          <Button
            type="submit"
            size="sm"
            className="ml-auto gap-1.5"
            disabled={
              (!inputValue.trim() && images.length === 0 && !uploadedDocument) || isSubmitDisabled
            }
            aria-label="Send message"
          >
            <CornerRightUp className="size-3.5" />
          </Button>
        </div>
      </form>
    </div>
  );
}
