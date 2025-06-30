import { useState, useCallback, useEffect, useMemo, useRef } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { Chat, ChatMessage, DEFAULT_MODEL_ID } from "@/state/LocalStateContext";
import { ChatContentPart } from "@/state/LocalStateContextDef";
import { fileToDataURL } from "@/utils/file";
import { BillingStatus } from "@/billing/billingApi";
import { MODEL_CONFIG } from "@/components/ModelSelector";

type ChatPhase = "idle" | "streaming" | "persisting";

interface UseChatSessionOptions {
  getChatById: (chatId: string) => Promise<Chat | undefined>;
  persistChat: (chat: Chat) => Promise<void>;
  openai: ReturnType<typeof import("@/ai/useOpenAi").useOpenAI>;
  model: string;
}

export function useChatSession(
  chatId: string,
  options: UseChatSessionOptions & {
    onImageConversionError?: (failedCount: number) => void;
  }
) {
  const { getChatById, persistChat, openai, model, onImageConversionError } = options;
  const queryClient = useQueryClient();
  const [phase, setPhase] = useState<ChatPhase>("idle");
  const [optimisticChat, setOptimisticChat] = useState<Chat | null>(null);
  const [currentStreamingMessage, setCurrentStreamingMessage] = useState<string>();
  const processingRef = useRef(false);
  const abortControllerRef = useRef<AbortController | null>(null);

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
    processingRef.current = false;
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

  const streamAssistant = useCallback(
    async (messages: ChatMessage[]): Promise<string> => {
      // Create new abort controller for this stream
      const abortController = new AbortController();
      abortControllerRef.current = abortController;

      try {
        const stream = openai.beta.chat.completions.stream({
          model,
          messages: messages as Parameters<
            typeof openai.beta.chat.completions.stream
          >[0]["messages"],
          stream: true
        });

        let fullResponse = "";
        setCurrentStreamingMessage("");

        for await (const chunk of stream) {
          // Check if we should abort
          if (abortController.signal.aborted) {
            stream.controller.abort();
            throw new Error("Stream aborted");
          }

          const content = chunk.choices[0]?.delta?.content || "";
          fullResponse += content;
          setCurrentStreamingMessage(fullResponse);
        }

        await stream.finalChatCompletion();
        setCurrentStreamingMessage(undefined);
        return fullResponse;
      } catch (error) {
        if (abortController.signal.aborted) {
          throw new Error("Stream aborted");
        }
        throw error;
      } finally {
        if (abortControllerRef.current === abortController) {
          abortControllerRef.current = null;
        }
      }
    },
    [openai, model]
  );

  const appendUserMessage = useCallback(
    async (
      content: string,
      images?: File[],
      documentText?: string,
      documentMetadata?: { filename: string; fullContent: string }
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

      const newMessages = [...chat.messages, userMessage];

      // Update optimistic state immediately
      setOptimisticChat({
        ...chat,
        messages: newMessages
      });

      try {
        // Start title generation in background if needed
        let titlePromise: Promise<string> | undefined;
        if (chat.title === "New Chat") {
          titlePromise = generateTitle(newMessages, openai, queryClient);
          // Update title in UI as soon as it's ready
          titlePromise.then((generatedTitle) => {
            setOptimisticChat((prev) => {
              if (!prev) return prev;
              return { ...prev, title: generatedTitle };
            });
          });
        }

        // Stream assistant response
        const assistantResponse = await streamAssistant(newMessages);

        // Add assistant message
        const finalMessages = [
          ...newMessages,
          { role: "assistant", content: assistantResponse } as ChatMessage
        ];

        // Update optimistic state with assistant message
        setOptimisticChat((prev) => ({
          ...prev!,
          messages: finalMessages
        }));

        // Wait for title generation if it was started
        const title = titlePromise ? await titlePromise : chat.title;

        // Persist to backend
        setPhase("persisting");
        await persistMutation.mutateAsync({
          id: chatId,
          title,
          model,
          messages: finalMessages
        });
      } catch (error) {
        setPhase("idle");
        processingRef.current = false;

        // Don't throw if it was an intentional abort
        if (error instanceof Error && error.message === "Stream aborted") {
          return;
        }

        throw error;
      } finally {
        processingRef.current = false;
      }
    },
    [chat, model, phase, streamAssistant, persistMutation, chatId, openai, queryClient]
  );

  return {
    chat,
    phase,
    currentStreamingMessage,
    appendUserMessage,
    streamAssistant
  };
}

// Helper to generate chat title
async function generateTitle(
  messages: ChatMessage[],
  openai: ReturnType<typeof import("@/ai/useOpenAi").useOpenAI>,
  queryClient: ReturnType<typeof useQueryClient>
): Promise<string> {
  // Helper function to check if the user is on a free plan
  function isUserOnFreePlan(): boolean {
    try {
      const billingStatus = queryClient.getQueryData(["billingStatus"]) as
        | BillingStatus
        | undefined;

      return (
        !billingStatus ||
        !billingStatus.product_name ||
        billingStatus.product_name.toLowerCase().includes("free")
      );
    } catch (error) {
      console.log("Error checking billing status, defaulting to free plan", error);
      return true; // Default to free plan if there's an error
    }
  }

  const userMessage = messages.find((m) => m.role === "user");
  if (!userMessage) return "New Chat";

  let messageText = "New Chat";

  if (typeof userMessage.content === "string") {
    messageText = userMessage.content;
  } else if (Array.isArray(userMessage.content)) {
    // Find the first text part safely
    const textPart = userMessage.content.find(
      (part): part is { type: "text"; text: string } =>
        part.type === "text" && "text" in part && typeof part.text === "string"
    );
    if (textPart) {
      messageText = textPart.text;
    }
  }

  // Simple title generation - truncate first message to 50 chars
  const simpleTitleFromMessage = messageText.slice(0, 50).trim();

  // For free plan users, just use the simple title
  // For paid plans, try to generate AI title
  if (isUserOnFreePlan()) {
    console.log("Using simple title generation for free plan user");
    return simpleTitleFromMessage;
  }

  // For paid plans, use LLM to generate a smart title
  try {
    console.log("Using AI title generation for paid plan user");
    // Get the user's first message, truncate if too long
    const userContent = messageText.slice(0, 500); // Reduced to 500 chars to optimize token usage

    // Use the OpenAI API to generate a concise title - use the default model
    const stream = openai.beta.chat.completions.stream({
      model: DEFAULT_MODEL_ID, // Use the default model instead of user selected model
      messages: [
        {
          role: "system",
          content:
            "You are a helpful assistant that generates concise, meaningful titles (3-5 words) for chat conversations based on the user's first message. Return only the title without quotes or explanations."
        },
        {
          role: "user",
          content: `Generate a concise, contextual title (3-5 words) for a chat that starts with this message: "${userContent}"`
        }
      ],
      temperature: 0.7,
      max_tokens: 15, // Keep response very short
      stream: true
    });

    let generatedTitle = "";
    for await (const chunk of stream) {
      const content = chunk.choices[0]?.delta?.content || "";
      generatedTitle += content;
    }

    // Get the final completion
    await stream.finalChatCompletion();

    // Remove quotes if present and limit length
    const cleanTitle = generatedTitle
      .replace(/^["']|["']$/g, "") // Remove leading/trailing quotes
      .trim()
      .slice(0, 50);

    console.log("Generated title:", cleanTitle);
    return cleanTitle || simpleTitleFromMessage; // Fallback if generation fails
  } catch (error) {
    console.error("Error generating AI title, falling back to simple title:", error);
    return simpleTitleFromMessage;
  }
}
