import { useState, useCallback, useEffect, useMemo, useRef } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { Chat, ChatMessage } from "@/state/LocalStateContext";
// import { DEFAULT_MODEL_ID } from "@/state/LocalStateContext"; // Commented out - used in legacy code
import { ChatContentPart } from "@/state/LocalStateContextDef";
import { fileToDataURL } from "@/utils/file";
// import { BillingStatus } from "@/billing/billingApi"; // Commented out - used in legacy title generation
import { MODEL_CONFIG } from "@/components/ModelSelector";

type ChatPhase = "idle" | "streaming" | "persisting";

interface UseChatSessionOptions {
  getChatById: (chatId: string) => Promise<Chat | undefined>;
  persistChat: (chat: Chat) => Promise<void>;
  openai: ReturnType<typeof import("@/ai/useOpenAi").useOpenAI>;
  model: string;
  onThreadCreated?: (threadId: string) => void; // Callback when a new thread is created
}

export function useChatSession(
  chatId: string,
  options: UseChatSessionOptions & {
    onImageConversionError?: (failedCount: number) => void;
  }
) {
  const { getChatById, persistChat, openai, model, onImageConversionError, onThreadCreated } = options;
  const queryClient = useQueryClient();
  const [phase, setPhase] = useState<ChatPhase>("idle");
  const [optimisticChat, setOptimisticChat] = useState<Chat | null>(null);
  const [currentStreamingMessage, setCurrentStreamingMessage] = useState<string>();
  const [streamingError, setStreamingError] = useState<string | null>(null);
  const processingRef = useRef(false);
  const abortControllerRef = useRef<AbortController | null>(null);
  const threadIdRef = useRef<string | null>(null); // Track the actual thread ID

  // Query the chat from backend
  const { data: serverChat, isPending } = useQuery({
    queryKey: ["chat", chatId],
    queryFn: () => getChatById(chatId),
    retry: false
  });

  // Reset optimistic chat when chatId changes
  useEffect(() => {
    // Abort any ongoing streaming when chatId changes
    if (abortControllerRef.current) {
      abortControllerRef.current.abort();
      abortControllerRef.current = null;
    }

    setOptimisticChat(null);
    setPhase("idle");
    setCurrentStreamingMessage(undefined);
    setStreamingError(null);
    processingRef.current = false;
    
    // Reset thread ID when navigating to a different chat
    // But keep it if we're loading an existing thread
    if (chatId === "new") {
      threadIdRef.current = null;
    } else if (/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(chatId)) {
      // This is an existing thread ID
      threadIdRef.current = chatId;
    }
  }, [chatId]);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      if (abortControllerRef.current) {
        abortControllerRef.current.abort();
      }
    };
  }, []);

  // Part A: Apply guard when syncing server data to optimistic state
  useEffect(() => {
    if (!serverChat || isPending) return;

    setOptimisticChat((prev) => {
      if (!prev) return serverChat; // first load
      // Never downgrade - if server has fewer messages, keep local state
      if (serverChat.messages.length <= prev.messages.length) return prev;
      return serverChat;
    });
  }, [serverChat, isPending]);

  // Mutation for persisting chat
  const persistMutation = useMutation({
    mutationFn: persistChat,
    onSuccess: () => {
      setPhase("idle");
      queryClient.invalidateQueries({ queryKey: ["chat", chatId] });
      queryClient.invalidateQueries({ queryKey: ["chatHistory"] });
      queryClient.invalidateQueries({ queryKey: ["billingStatus"] });
    },
    onError: () => {
      setPhase("idle");
    }
  });

  // Current chat is optimistic if we have it, otherwise server data
  const chat: Chat = useMemo(
    () =>
      optimisticChat ||
      serverChat || {
        id: chatId,
        title: "New Chat",
        messages: []
      },
    [optimisticChat, serverChat, chatId]
  );
  const appendUserMessage = useCallback(
    async (
      content: string,
      images?: File[],
      documentText?: string,
      documentMetadata?: { filename: string; fullContent: string },
      systemPrompt?: string
    ) => {
      if (phase !== "idle") {
        return;
      }

      if (processingRef.current) {
        return;
      }

      // Handle images for vision-capable models
      const modelSupportsVision = MODEL_CONFIG[model]?.supportsVision || false;
      let userMessage: ChatMessage;

      // If document text is provided, combine it with the content
      let finalContent = content;
      if (documentText) {
        finalContent = documentText + (content ? `\n\n${content}` : "");
      }

      if (modelSupportsVision && images && images.length > 0) {
        const parts: ChatContentPart[] = [{ type: "text", text: finalContent }];
        let failedImageCount = 0;

        for (const file of images) {
          try {
            const url = await fileToDataURL(file);
            parts.push({ type: "image_url", image_url: { url } });
          } catch (error) {
            console.error("[useChatSession] Failed to convert image to data URL:", error);
            failedImageCount++;
            continue;
          }
        }

        // Notify about failed conversions
        if (failedImageCount > 0 && onImageConversionError) {
          onImageConversionError(failedImageCount);
        }

        // If we have at least text content (and potentially some images), create multimodal message
        // If no images were successfully processed, the message will just have text
        userMessage = { role: "user", content: parts };
      } else {
        userMessage = { role: "user", content: finalContent };
      }

      // Add document metadata if provided
      if (documentMetadata) {
        userMessage.document = documentMetadata;
      }

      // Check again after async operations to prevent double execution
      if (processingRef.current) {
        return;
      }

      processingRef.current = true;
      setPhase("streaming");
      setStreamingError(null); // Clear any previous errors

      // Add system message first if provided
      const newMessages = [...chat.messages];
      if (systemPrompt && systemPrompt.trim()) {
        newMessages.push({ role: "system", content: systemPrompt.trim() } as ChatMessage);
      }
      newMessages.push(userMessage);

      // Update optimistic state immediately
      setOptimisticChat({
        ...chat,
        messages: newMessages
      });

      try {
        // const useResponses = import.meta.env.VITE_USE_RESPONSES === "true";
        const useResponses = true;

        if (useResponses) {
          // Responses API path (no client-side title generation, server persists/title)
          const abortController = new AbortController();
          abortControllerRef.current = abortController;

          setCurrentStreamingMessage("");

          let accumulated = "";
          const inputText = typeof finalContent === "string" ? finalContent : String(finalContent);
          const instructions = systemPrompt?.trim() || undefined;

          let stream: any;
          try {
            // Use the thread ID if we have it (for follow-up messages in the same session)
            // or use the chatId if it's a valid UUID (for loading existing threads)
            const effectiveThreadId = threadIdRef.current || 
              (chatId !== "new" && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(chatId) ? chatId : null);
            
            const requestParams: any = {
              model,
              input: inputText,
              instructions,
              stream: true,
              store: true // Ensure messages are persisted
            };
            
            // If we have a thread ID, this is a continuation
            if (effectiveThreadId) {
              requestParams.previous_response_id = effectiveThreadId;
              console.log("Continuing thread with previous_response_id:", effectiveThreadId);
            } else {
              console.log("Starting new thread (no previous_response_id)");
            }
            
            console.log("Creating response with params:", requestParams);
            stream = await openai.responses.create(requestParams);
          } catch (error) {
            console.error("Error creating stream:", error);
            if (error instanceof Error) {
              console.error("Error details:", error.message, error.stack);
            }
            setStreamingError("Failed to create response stream");
            setPhase("idle");
            processingRef.current = false;
            return;
          }

          let responseId: string | null = null;
          
          try {
            for await (const event of stream) {
              if (abortController.signal.aborted) break;
              
              // Handle different event types based on OpenAI Responses API spec
              if (event.type === "response.created") {
                // Capture the response ID
                responseId = event.response?.id;
                console.log("Response created with ID:", responseId);
                
                // For new chats, store the thread ID for follow-up messages
                if (chatId === "new" && !threadIdRef.current && responseId) {
                  console.log("New thread created with ID:", responseId);
                  threadIdRef.current = responseId;
                  // Call the callback to update the URL via React Router
                  if (onThreadCreated) {
                    onThreadCreated(responseId);
                  }
                }
              } else if (event.type === "response.output_item.added") {
                // New output item started
              } else if (event.type === "response.output_text.delta") {
                const delta = event.delta || "";
                if (delta) {
                  accumulated += delta;
                  setCurrentStreamingMessage(accumulated);
                }
              } else if (event.type === "response.output_text.done") {
                // Text output completed
              } else if (event.type === "response.completed" || event.type === "response.done") {
                // Response completed
                const finalMessages = [
                  ...newMessages,
                  { role: "assistant", content: accumulated } as ChatMessage
                ];
                setOptimisticChat((prev) => ({ ...prev!, messages: finalMessages }));
                setCurrentStreamingMessage(undefined);
                setPhase("idle");
                
                // Store messages locally for display purposes (Responses API maintains real context)
                // This is a temporary solution until we have a thread history endpoint
                if (threadIdRef.current) {
                  try {
                    localStorage.setItem(
                      `responses_chat_${threadIdRef.current}`,
                      JSON.stringify({
                        messages: finalMessages,
                        lastUpdated: Date.now()
                      })
                    );
                  } catch (error) {
                    console.error("Failed to store messages locally:", error);
                  }
                }
                
                queryClient.invalidateQueries({ queryKey: ["chatHistory"] });
                break;
              } else if (event.type === "error") {
                setStreamingError(event.message || "Streaming error");
                break;
              }
            }
          } finally {
            // Clean up but don't throw in finally block
            if (abortController.signal.aborted) {
              console.log("Stream was aborted");
            }
          }
        } else {
          // Legacy completions path is deprecated
          throw new Error("Completions API is deprecated - please use Responses API");
        }
      } catch (error) {
        setPhase("idle");
        processingRef.current = false;

        // Don't throw if it was an intentional abort
        if (error instanceof Error && error.message === "Stream aborted") {
          return;
        }

        // For streaming errors, we've already set the error state
        // Don't throw to prevent uncaught promise rejection
        console.error("Chat streaming error:", error);
      } finally {
        processingRef.current = false;
      }
    },
    [chat, model, phase, persistMutation, chatId, openai, queryClient]
  );

  return {
    chat,
    phase,
    currentStreamingMessage,
    appendUserMessage,
    streamingError,
    isPending
  };
}
