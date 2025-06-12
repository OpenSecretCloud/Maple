import { useState, useCallback, useEffect, useMemo, useRef } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { Chat, ChatMessage } from "@/state/LocalStateContext";
import { ChatContentPart } from "@/state/LocalStateContextDef";
import { fileToDataURL } from "@/utils/file";

type ChatPhase = "idle" | "streaming" | "persisting";

interface UseChatSessionOptions {
  getChatById: (chatId: string) => Promise<Chat | undefined>;
  persistChat: (chat: Chat) => Promise<void>;
  openai: ReturnType<typeof import("@/ai/useOpenAi").useOpenAI>;
  model: string;
}

export function useChatSession(chatId: string, options: UseChatSessionOptions) {
  const { getChatById, persistChat, openai, model } = options;
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
    async (content: string, images?: File[]) => {
      if (phase !== "idle") {
        return;
      }

      if (processingRef.current) {
        return;
      }

      // Handle images for Gemma model
      const isGemma = model === "leon-se/gemma-3-27b-it-fp8-dynamic";
      let userMessage: ChatMessage;

      if (isGemma && images && images.length > 0) {
        const parts: ChatContentPart[] = [{ type: "text", text: content }];
        for (const file of images) {
          const url = await fileToDataURL(file);
          parts.push({ type: "image_url", image_url: { url } });
        }
        userMessage = { role: "user", content: parts };
      } else {
        userMessage = { role: "user", content };
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

        // Generate title if needed
        const title = chat.title === "New Chat" ? await generateTitle(finalMessages) : chat.title;

        // Persist to backend
        setPhase("persisting");
        await persistMutation.mutateAsync({
          id: chatId,
          title,
          model,
          messages: finalMessages
        });

        // Update title in optimistic state if changed
        if (title !== chat.title) {
          setOptimisticChat((prev) => ({ ...prev!, title }));
        }
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
async function generateTitle(messages: ChatMessage[]): Promise<string> {
  const userMessage = messages.find((m) => m.role === "user");
  if (!userMessage) return "New Chat";

  const messageText =
    typeof userMessage.content === "string"
      ? userMessage.content
      : (userMessage.content as ChatContentPart[]).find((p) => p.type === "text")
        ? (
            (userMessage.content as ChatContentPart[]).find((p) => p.type === "text") as {
              text: string;
            }
          ).text
        : "New Chat";

  // Simple title for now - just truncate
  return messageText.slice(0, 50).trim();
}
