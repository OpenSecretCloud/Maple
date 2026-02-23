import { useState, useCallback, useRef } from "react";
import { useOpenSecret } from "@opensecret/react";
import {
  getAgentConfig,
  updateAgentConfig,
  listMemoryBlocks,
  listAgentConversations,
  listAgentConversationItems,
  type AgentConfigResponse,
  type UpdateAgentConfigRequest,
  type MemoryBlockResponse,
  type AgentMessageEvent,
  type AgentErrorEvent
} from "@opensecret/react";

export type AgentMessage = {
  id: string;
  role: "user" | "assistant";
  content: string;
  timestamp: Date;
  step?: number;
};

type AgentTypingEvent = {
  step: number;
};

type AgentChatState = {
  messages: AgentMessage[];
  isLoading: boolean;
  isTyping: boolean;
  error: string | null;
};

export function useAgent() {
  const { aiCustomFetch, apiUrl } = useOpenSecret();
  const [state, setState] = useState<AgentChatState>({
    messages: [],
    isLoading: false,
    isTyping: false,
    error: null
  });
  const abortControllerRef = useRef<AbortController | null>(null);
  const messageIdCounter = useRef(0);

  const nextId = useCallback(() => {
    messageIdCounter.current += 1;
    return `msg-${Date.now()}-${messageIdCounter.current}`;
  }, []);

  const loadHistory = useCallback(async () => {
    try {
      console.log("[agent] Loading history...");
      const config = await getAgentConfig();
      console.log("[agent] Config:", config);
      if (!config.conversation_id) {
        console.log("[agent] No conversation_id yet, skipping history load");
        return;
      }

      const conversations = await listAgentConversations();
      console.log("[agent] Conversations:", conversations.data.length);
      if (conversations.data.length === 0) return;

      const items = await listAgentConversationItems(conversations.data[0].id, {
        limit: 100,
        order: "asc"
      });
      console.log("[agent] History items:", items.data.length);

      const historicMessages: AgentMessage[] = [];
      for (const item of items.data) {
        if (item.type === "message") {
          const content =
            item.content
              ?.filter(
                (c: { type: string; text?: string }) =>
                  c.type === "output_text" || c.type === "input_text"
              )
              .map((c: { text?: string }) => c.text || "")
              .join("") || "";

          if (content) {
            historicMessages.push({
              id: nextId(),
              role: item.role as "user" | "assistant",
              content,
              timestamp: new Date(item.id)
            });
          }
        }
      }

      console.log("[agent] Loaded", historicMessages.length, "messages from history");
      setState((prev) => ({
        ...prev,
        messages: historicMessages
      }));
    } catch (err) {
      console.log("[agent] History load skipped/failed:", err);
    }
  }, [nextId]);

  const sendMessage = useCallback(
    async (input: string) => {
      if (!input.trim() || state.isLoading) return;

      const userMessage: AgentMessage = {
        id: nextId(),
        role: "user",
        content: input.trim(),
        timestamp: new Date()
      };

      setState((prev) => ({
        ...prev,
        messages: [...prev.messages, userMessage],
        isLoading: true,
        isTyping: true,
        error: null
      }));

      const controller = new AbortController();
      abortControllerRef.current = controller;

      try {
        console.log("[agent] Sending message:", input.trim().slice(0, 50) + "...");
        console.log("[agent] POST", `${apiUrl}/v1/agent/chat`);

        // aiCustomFetch handles encryption/decryption + throws on !response.ok
        const response = await aiCustomFetch(`${apiUrl}/v1/agent/chat`, {
          method: "POST",
          headers: {
            "Content-Type": "application/json",
            Accept: "text/event-stream"
          },
          body: JSON.stringify({ input: input.trim() }),
          signal: controller.signal
        });

        console.log("[agent] Response received, status:", response.status);
        console.log("[agent] Content-Type:", response.headers.get("content-type"));

        // Read the decrypted SSE stream incrementally
        const reader = response.body?.getReader();
        if (!reader) {
          console.error("[agent] No response body / reader");
          throw new Error("No response body");
        }

        const decoder = new TextDecoder();
        let buffer = "";
        let currentEvent = "";
        let chunkCount = 0;

        while (true) {
          const { done, value } = await reader.read();
          if (done) {
            console.log("[agent] Stream done after", chunkCount, "chunks");
            break;
          }

          chunkCount++;
          // SDK's SSE transform enqueues strings, not Uint8Arrays
          const chunk = typeof value === "string" ? value : decoder.decode(value, { stream: true });
          console.log("[agent] Chunk", chunkCount, `(${chunk.length} chars):`, chunk.slice(0, 200));
          buffer += chunk;

          // Process complete lines from buffer
          let newlineIdx: number;
          while ((newlineIdx = buffer.indexOf("\n")) !== -1) {
            const line = buffer.slice(0, newlineIdx).trim();
            buffer = buffer.slice(newlineIdx + 1);

            if (!line) continue;

            if (line.startsWith("event: ")) {
              currentEvent = line.slice(7);
              console.log("[agent] SSE event type:", currentEvent);
            } else if (line.startsWith("data: ")) {
              const dataStr = line.slice(6);
              if (dataStr === "[DONE]") {
                console.log("[agent] SSE [DONE]");
                continue;
              }

              console.log("[agent] SSE data:", dataStr.slice(0, 200));

              try {
                const data = JSON.parse(dataStr);
                console.log("[agent] Parsed event:", currentEvent, data);

                if (currentEvent === "agent.typing") {
                  const event = data as AgentTypingEvent;
                  console.log("[agent] Typing indicator (step", event.step + ")");
                  setState((prev) => ({
                    ...prev,
                    isTyping: true
                  }));
                } else if (currentEvent === "agent.message") {
                  const event = data as AgentMessageEvent;
                  for (const msg of event.messages) {
                    console.log(
                      "[agent] Assistant message (step",
                      event.step + "):",
                      msg.slice(0, 100)
                    );
                    const assistantMessage: AgentMessage = {
                      id: nextId(),
                      role: "assistant",
                      content: msg,
                      timestamp: new Date(),
                      step: event.step
                    };
                    setState((prev) => ({
                      ...prev,
                      messages: [...prev.messages, assistantMessage],
                      isTyping: false
                    }));
                  }
                } else if (currentEvent === "agent.error") {
                  const event = data as AgentErrorEvent;
                  console.error("[agent] Agent error:", event.error);
                  setState((prev) => ({
                    ...prev,
                    error: event.error,
                    isLoading: false,
                    isTyping: false
                  }));
                  return;
                } else if (currentEvent === "agent.done") {
                  console.log("[agent] Agent done:", data);
                  setState((prev) => ({
                    ...prev,
                    isTyping: false
                  }));
                }
              } catch (parseErr) {
                console.warn(
                  "[agent] Failed to parse SSE data:",
                  parseErr,
                  "raw:",
                  dataStr.slice(0, 200)
                );
              }
            } else {
              console.log("[agent] Unrecognized line:", line.slice(0, 100));
            }
          }
        }

        console.log("[agent] Finished processing stream");
        setState((prev) => ({ ...prev, isLoading: false, isTyping: false }));
      } catch (err) {
        console.error("[agent] Error:", (err as Error).name, (err as Error).message);
        if ((err as Error).name === "AbortError") {
          setState((prev) => ({ ...prev, isLoading: false, isTyping: false }));
          return;
        }
        setState((prev) => ({
          ...prev,
          isLoading: false,
          isTyping: false,
          error: (err as Error).message || "Failed to send message"
        }));
      } finally {
        abortControllerRef.current = null;
      }
    },
    [aiCustomFetch, apiUrl, state.isLoading, nextId]
  );

  const cancelRequest = useCallback(() => {
    abortControllerRef.current?.abort();
  }, []);

  const clearMessages = useCallback(() => {
    setState({ messages: [], isLoading: false, isTyping: false, error: null });
  }, []);

  return {
    messages: state.messages,
    isLoading: state.isLoading,
    isTyping: state.isTyping,
    error: state.error,
    sendMessage,
    cancelRequest,
    clearMessages,
    loadHistory,
    getConfig: getAgentConfig,
    updateConfig: updateAgentConfig,
    getMemoryBlocks: listMemoryBlocks
  };
}

export type { AgentConfigResponse, UpdateAgentConfigRequest, MemoryBlockResponse };
