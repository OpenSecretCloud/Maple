import { useState, useRef, useEffect, useCallback } from "react";
import { Send, Bot, User, Loader2, Copy, Check, Plus, Image, FileText, X } from "lucide-react";
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
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger
} from "@/components/ui/dropdown-menu";
import { isTauri } from "@/utils/platform";
import type { ChatContentPart } from "@/state/LocalStateContextDef";

// Types
interface Message {
  id: string;
  role: "user" | "assistant";
  content: string | ChatContentPart[];
  timestamp: number;
  status?: "complete" | "streaming" | "error";
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
  status?: "completed" | "in_progress";
  content?: Array<{
    type: "input_text" | "input_image" | "output_text";
    text?: string;
    image_url?: string;
    detail?: "high" | "low" | "auto";
  }>;
  created_at?: number;
}

export function UnifiedChat() {
  const isMobile = useIsMobile();
  const openai = useOpenAI();
  const localState = useLocalState();
  const os = useOpenSecret();
  const isTauriEnv = isTauri();

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

  // Attachment states
  const [draftImages, setDraftImages] = useState<File[]>([]);
  const [imageUrls, setImageUrls] = useState<Map<File, string>>(new Map());
  const [documentText, setDocumentText] = useState<string>("");
  const [documentName, setDocumentName] = useState<string>("");
  const [isProcessingDocument, setIsProcessingDocument] = useState(false);
  const [attachmentError, setAttachmentError] = useState<string | null>(null);
  const [upgradeDialogOpen, setUpgradeDialogOpen] = useState(false);
  const [upgradeFeature, setUpgradeFeature] = useState<"image" | "document">("image");

  const abortControllerRef = useRef<AbortController | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const documentInputRef = useRef<HTMLInputElement>(null);

  // Refs
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);

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
      // Clear attachments
      clearAllAttachments();
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
  }, [chatId, clearAllAttachments]);

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
                // Handle both input_text (user) and output_text (assistant)
                if ((part.type === "input_text" || part.type === "output_text") && part.text) {
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
      console.log("RAW API RESPONSE from conversations.items.list:", response);
      console.log("response.data:", JSON.stringify(response.data, null, 2));

      if (response.data.length > 0) {
        // Convert API items to UI messages
        const newMessages: Message[] = [];

        for (const item of response.data) {
          if (item.type === "message" && item.role && item.content) {
            // Preserve the content structure - could be string or array
            let messageContent: any = "";

            if (Array.isArray(item.content)) {
              // Check if this is a multimodal message with images
              const hasImages = item.content.some((part: any) => part.type === "input_image");

              if (hasImages) {
                // Preserve the full multimodal structure
                messageContent = item.content;
              } else {
                // Extract text for text-only messages
                let text = "";
                for (const part of item.content) {
                  // Handle both input_text (user) and output_text (assistant)
                  if ((part.type === "input_text" || part.type === "output_text") && part.text) {
                    text += part.text;
                  }
                }
                messageContent = text;
              }
            } else if (typeof item.content === "string") {
              messageContent = item.content;
            }

            if (
              messageContent &&
              (typeof messageContent === "string" ? messageContent.length > 0 : true)
            ) {
              console.log("Polled message from server:");
              console.log("  - Role:", item.role);
              console.log("  - Original content:", JSON.stringify(item.content, null, 2));
              console.log("  - Processed content:", JSON.stringify(messageContent, null, 2));
              console.log("  - Content type:", typeof messageContent);
              console.log("  - Is array?", Array.isArray(messageContent));

              // The backend will use our internal_message_id as the actual ID
              console.log("Processing polled message:", {
                id: item.id,
                role: item.role,
                contentType: typeof messageContent
              });

              newMessages.push({
                id: item.id, // Backend returns our UUID for user messages
                role: item.role as "user" | "assistant",
                content: messageContent,
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
            const uniqueNewMessages = newMessages.filter((m) => !existingIds.has(m.id));

            if (uniqueNewMessages.length === 0) return prev;

            // Simple approach: just add new messages that we don't have
            // The backend should handle the internal_message_id mapping
            return [...prev, ...uniqueNewMessages];
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

  // Check user's billing access
  const billingStatus = localState.billingStatus;
  const hasStarterAccess =
    billingStatus &&
    (billingStatus.product_name?.toLowerCase().includes("starter") ||
      billingStatus.product_name?.toLowerCase().includes("pro") ||
      billingStatus.product_name?.toLowerCase().includes("max") ||
      billingStatus.product_name?.toLowerCase().includes("team"));

  const hasProTeamAccess =
    billingStatus &&
    (billingStatus.product_name?.toLowerCase().includes("pro") ||
      billingStatus.product_name?.toLowerCase().includes("max") ||
      billingStatus.product_name?.toLowerCase().includes("team"));

  const canUseImages = hasStarterAccess;
  const canUseDocuments = hasProTeamAccess;

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
          setDocumentText(text);
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
          const result = await invoke<{ text: string }>("parse_document", {
            filename: file.name,
            contentBase64: base64Data,
            fileType: "pdf"
          });

          const parsed = JSON.parse(result.text);
          if (parsed.document?.text_content) {
            // Clean up image references from parsed text
            const cleanedText = parsed.document.text_content.replace(/!\[Image\]\([^)]+\)/g, "");
            setDocumentText(cleanedText);
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
    [isTauriEnv, os]
  );

  const removeDocument = useCallback(() => {
    setDocumentText("");
    setDocumentName("");
  }, []);

  // Send message handler
  const handleSendMessage = useCallback(
    async (e?: React.FormEvent) => {
      e?.preventDefault();

      // Allow send if we have text, images, or document
      const trimmedInput = input.trim();
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

      // Build the message content with proper typing
      let messageContent: any = ""; // Use any for now, will be properly typed when sent to API

      // Combine document text with input if both exist
      let finalText = trimmedInput;
      if (documentText) {
        finalText = documentText + (trimmedInput ? `\n\n${trimmedInput}` : "");
      }

      // If we have images, create multimodal content
      if (draftImages.length > 0) {
        const parts: ChatContentPart[] = [];

        // Add text part if exists
        if (finalText) {
          parts.push({ type: "input_text", text: finalText });
        }

        // Add image parts with proper OpenAI format
        for (const file of draftImages) {
          try {
            const dataUrl = await fileToDataURL(file);
            parts.push({
              type: "input_image",
              image_url: dataUrl
            } as ChatContentPart);
          } catch (error) {
            console.error("Failed to convert image:", error);
          }
        }

        messageContent = parts;
      } else {
        messageContent = finalText;
      }

      // Add user message immediately with a local UUID
      const localMessageId = crypto.randomUUID();
      const userMessage: Message = {
        id: localMessageId,
        role: "user",
        content: messageContent, // Store the actual content structure
        timestamp: Date.now(),
        status: "complete"
      };

      console.log("Creating user message with local ID:", localMessageId);
      console.log("Message content:", JSON.stringify(messageContent, null, 2));

      setMessages((prev) => [...prev, userMessage]);
      setInput("");
      clearAllAttachments();
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

        // Create streaming response - the API expects the content directly as we built it
        console.log("Sending to API with metadata:", {
          conversation: conversationId,
          model: localState.model || DEFAULT_MODEL_ID,
          input: [{ role: "user", content: messageContent }],
          metadata: { internal_message_id: localMessageId }
        });

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

        // Track server-assigned IDs and accumulated content
        let serverAssistantId: string | undefined;
        let assistantMessageAdded = false;
        let accumulatedContent = "";

        // Process streaming events
        for await (const event of stream) {
          // Log EVERY SSE event we receive
          console.log("SSE Event received:", {
            type: event.type,
            item_id: (event as any).item_id,
            item: (event as any).item,
            delta: (event as any).delta,
            full_event: JSON.stringify(event)
          });

          if (event.type === "response.output_item.added" && event.item?.type === "message") {
            // Get the server-assigned ID from item.id
            if ((event as any).item?.id) {
              serverAssistantId = (event as any).item.id;
              console.log("Got server assistant ID:", serverAssistantId);

              // Add the assistant message with the correct server ID
              const assistantMessage: Message = {
                id: serverAssistantId!,
                role: "assistant",
                content: "",
                timestamp: Date.now(),
                status: "streaming"
              };
              setMessages((prev) => [...prev, assistantMessage]);
              assistantMessageAdded = true;
            }
          } else if (event.type === "response.output_text.delta" && event.delta) {
            // Accumulate text chunks
            accumulatedContent += event.delta;

            // Only update if we have the message added
            if (serverAssistantId && assistantMessageAdded) {
              setMessages((prev) =>
                prev.map((msg) =>
                  msg.id === serverAssistantId ? { ...msg, content: accumulatedContent } : msg
                )
              );
            }
          } else if (event.type === "response.output_item.done") {
            // Mark message as complete and update lastSeenItemId
            if (serverAssistantId) {
              setMessages((prev) =>
                prev.map((msg) =>
                  msg.id === serverAssistantId ? { ...msg, status: "complete" } : msg
                )
              );
              setLastSeenItemId(serverAssistantId);
            }
          } else if (event.type === "response.failed" || event.type === "error") {
            // Handle streaming errors
            console.error("Streaming error:", event);
            if (serverAssistantId) {
              setMessages((prev) =>
                prev.map((msg) =>
                  msg.id === serverAssistantId ? { ...msg, status: "error" } : msg
                )
              );
            }
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
    [
      input,
      isGenerating,
      openai,
      conversation,
      localState.model,
      draftImages,
      documentText,
      clearAllAttachments
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
        {/* Mobile sidebar toggle */}
        {!isSidebarOpen && (
          <div className="fixed top-4 left-4 z-20 md:hidden">
            <SidebarToggle onToggle={toggleSidebar} />
          </div>
        )}

        {/* Only show header when there are messages (conversation exists) */}
        {messages.length > 0 && (
          <div className="h-14 flex items-center px-4">
            <div className="flex-1 flex items-center justify-center">
              <h1 className="text-base font-medium truncate max-w-[20rem] text-muted-foreground">
                {conversation?.metadata?.title || "Chat"}
              </h1>
            </div>
          </div>
        )}

        {/* Messages Area */}
        <div className="flex-1 overflow-y-auto flex flex-col relative">
          {/* Error message */}
          {error && (
            <div className="max-w-4xl mx-auto w-full p-6">
              <div className="bg-red-50 text-red-600 p-3 rounded mb-4">{error}</div>
            </div>
          )}

          {/* Only show messages when there are messages */}
          {messages.length > 0 && (
            <div className="max-w-4xl mx-auto p-6 w-full">
              {/* Message list with modern ChatGPT/Claude style */}
              <div className="space-y-1">
                {messages.map((message) => (
                  <div
                    key={message.id}
                    className={`group py-6 px-4 ${message.role === "user" ? "bg-muted/30" : ""}`}
                  >
                    <div className="flex gap-3 max-w-4xl mx-auto">
                      {/* Avatar */}
                      <div className="flex-shrink-0">
                        {message.role === "user" ? (
                          <div className="w-8 h-8 rounded-full bg-foreground/10 flex items-center justify-center">
                            <User className="h-4 w-4 text-foreground" />
                          </div>
                        ) : (
                          <div className="w-8 h-8 rounded-full bg-primary/10 flex items-center justify-center">
                            <Bot className="h-4 w-4 text-primary" />
                          </div>
                        )}
                      </div>

                      {/* Message content */}
                      <div className="flex-1 space-y-2 overflow-hidden">
                        <div className="font-semibold text-sm">
                          {message.role === "user" ? "You" : "Maple"}
                        </div>
                        <div className="prose prose-sm dark:prose-invert max-w-none">
                          {/* Render based on content type */}
                          {(() => {
                            console.log("Rendering message:", {
                              role: message.role,
                              content: message.content,
                              contentType: typeof message.content,
                              isArray: Array.isArray(message.content)
                            });

                            if (typeof message.content === "string") {
                              return (
                                <Markdown
                                  content={message.content}
                                  loading={message.status === "streaming"}
                                  chatId={chatId || ""}
                                />
                              );
                            } else if (Array.isArray(message.content)) {
                              return (
                                // Render multimodal content (images + text)
                                <div className="space-y-3">
                                  {message.content.map((part: any, partIdx: number) => (
                                    <div key={partIdx}>
                                      {part.type === "input_text" || part.type === "output_text" ? (
                                        <Markdown
                                          content={part.text || ""}
                                          loading={false}
                                          chatId={chatId || ""}
                                        />
                                      ) : part.type === "input_image" ? (
                                        <img
                                          src={part.image_url || ""}
                                          alt={`Image ${partIdx + 1}`}
                                          className="max-w-full rounded-lg"
                                          style={{ maxHeight: "400px", objectFit: "contain" }}
                                        />
                                      ) : null}
                                    </div>
                                  ))}
                                </div>
                              );
                            } else {
                              // Fallback for unexpected content type
                              return (
                                <div className="text-muted-foreground">
                                  [Unable to display message content]
                                </div>
                              );
                            }
                          })()}
                        </div>

                        {/* Actions - only show on hover for assistant messages */}
                        {message.role === "assistant" &&
                          message.content &&
                          typeof message.content === "string" && (
                            <div className="flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                              <CopyButton text={message.content} />
                            </div>
                          )}
                      </div>
                    </div>
                  </div>
                ))}

                {/* Loading indicator - modern style */}
                {isGenerating &&
                  !messages.some((m) => m.role === "assistant" && m.status === "streaming") && (
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
                <form onSubmit={handleSendMessage} className="w-full">
                  <div className="space-y-2">
                    {/* Model selector and attachment button */}
                    <div className="flex items-center gap-2">
                      <ModelSelector
                        messages={messages.map((m) => ({
                          role: m.role as "user" | "assistant" | "system",
                          content: m.content
                        }))}
                        draftImages={draftImages}
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
                            {!canUseImages && (
                              <span className="ml-auto text-xs text-muted-foreground">Pro</span>
                            )}
                          </DropdownMenuItem>
                          {isTauriEnv && (
                            <DropdownMenuItem
                              onClick={() => {
                                if (!canUseDocuments) {
                                  setUpgradeFeature("document");
                                  setUpgradeDialogOpen(true);
                                } else {
                                  documentInputRef.current?.click();
                                }
                              }}
                            >
                              <FileText className="mr-2 h-4 w-4" />
                              <span>Add Document</span>
                              {!canUseDocuments && (
                                <span className="ml-auto text-xs text-muted-foreground">Pro</span>
                              )}
                            </DropdownMenuItem>
                          )}
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
                    {attachmentError && (
                      <div className="text-sm text-red-500 px-2">{attachmentError}</div>
                    )}

                    <div className="relative">
                      <Textarea
                        ref={textareaRef}
                        value={input}
                        onChange={(e) => setInput(e.target.value)}
                        onKeyDown={handleKeyDown}
                        placeholder="Message Maple..."
                        disabled={isGenerating}
                        className="w-full resize-none min-h-[100px] max-h-[200px] pr-14 py-4 px-5 rounded-xl border bg-background focus:outline-none focus:ring-2 focus:ring-ring/20 placeholder:text-muted-foreground/60 text-base"
                        rows={3}
                        id="message"
                      />
                      <Button
                        type="submit"
                        disabled={
                          (!input.trim() && !draftImages.length && !documentText) || isGenerating
                        }
                        size="icon"
                        className="absolute bottom-3 right-3 h-9 w-9 rounded-lg"
                      >
                        {isGenerating ? (
                          <Loader2 className="h-4 w-4 animate-spin" />
                        ) : (
                          <Send className="h-4 w-4" />
                        )}
                      </Button>
                    </div>

                    {/* Hidden file inputs */}
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
              <form onSubmit={handleSendMessage}>
                <div className="space-y-2">
                  {/* Model selector and attachment button */}
                  <div className="flex items-center gap-2">
                    <ModelSelector
                      messages={messages.map((m) => ({
                        role: m.role as "user" | "assistant" | "system",
                        content: m.content
                      }))}
                      draftImages={draftImages}
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
                          {!canUseImages && (
                            <span className="ml-auto text-xs text-muted-foreground">Starter</span>
                          )}
                        </DropdownMenuItem>
                        {isTauriEnv && (
                          <DropdownMenuItem
                            onClick={() => {
                              if (!canUseDocuments) {
                                setUpgradeFeature("document");
                                setUpgradeDialogOpen(true);
                              } else {
                                documentInputRef.current?.click();
                              }
                            }}
                          >
                            <FileText className="mr-2 h-4 w-4" />
                            <span>Add Document</span>
                            {!canUseDocuments && (
                              <span className="ml-auto text-xs text-muted-foreground">Pro</span>
                            )}
                          </DropdownMenuItem>
                        )}
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
                  {attachmentError && (
                    <div className="text-xs text-red-500 px-2">{attachmentError}</div>
                  )}

                  <div className="relative">
                    <Textarea
                      ref={textareaRef}
                      value={input}
                      onChange={(e) => setInput(e.target.value)}
                      onKeyDown={handleKeyDown}
                      placeholder="Message Maple..."
                      disabled={isGenerating}
                      className="w-full resize-none min-h-[52px] max-h-[200px] pr-14 py-3 px-4 rounded-xl border bg-background focus:outline-none focus:ring-2 focus:ring-ring/20 placeholder:text-muted-foreground/60"
                      rows={1}
                      id="message"
                    />
                    <Button
                      type="submit"
                      disabled={
                        (!input.trim() && !draftImages.length && !documentText) || isGenerating
                      }
                      size="icon"
                      className="absolute bottom-[0.45rem] right-2 h-8 w-8 rounded-lg"
                    >
                      {isGenerating ? (
                        <Loader2 className="h-4 w-4 animate-spin" />
                      ) : (
                        <Send className="h-4 w-4" />
                      )}
                    </Button>
                  </div>
                </div>
              </form>
              <p className="text-sm text-center text-muted-foreground/60 mt-2">
                AI can make mistakes. Check important info.
              </p>
            </div>
          </div>
        )}

        {/* Upgrade dialog for attachments */}
        <UpgradePromptDialog
          open={upgradeDialogOpen}
          onOpenChange={setUpgradeDialogOpen}
          feature={upgradeFeature === "document" ? "document" : "image"}
        />
      </div>
    </div>
  );
}
