import { useOpenSecret } from "@opensecret/react";
import { useState, useEffect } from "react";
import { BillingStatus } from "@/billing/billingApi";
import { LocalStateContext, Chat, HistoryItem, OpenSecretModel } from "./LocalStateContextDef";
import { aliasModelName } from "@/utils/utils";

export {
  LocalStateContext,
  type Chat,
  type ChatMessage,
  type HistoryItem,
  type LocalState
} from "./LocalStateContextDef";

export const FREE_USER_DEFAULT_MODEL_ID = "llama3-3-70b";
export const PAID_USER_DEFAULT_MODEL_ID = "deepseek-r1-0528";

// Get default model based on user's billing status
export function getDefaultModelId(billingStatus: BillingStatus | null): string {
  // If no billing status, assume free user
  if (!billingStatus) {
    return FREE_USER_DEFAULT_MODEL_ID;
  }

  // Check if user has any paid plan (starter, pro, max, or team)
  const planName = billingStatus.product_name?.toLowerCase() || "";
  const isPaidUser =
    billingStatus.is_subscribed &&
    (planName.includes("starter") ||
      planName.includes("pro") ||
      planName.includes("max") ||
      planName.includes("team"));

  return isPaidUser ? PAID_USER_DEFAULT_MODEL_ID : FREE_USER_DEFAULT_MODEL_ID;
}

export const LocalStateProvider = ({ children }: { children: React.ReactNode }) => {
  /** The model that should be assumed when a chat doesn't yet have one */
  const llamaModel: OpenSecretModel = {
    id: FREE_USER_DEFAULT_MODEL_ID,
    object: "model",
    created: Date.now(),
    owned_by: "meta",
    tasks: ["generate"]
  };

  const deepSeekModel: OpenSecretModel = {
    id: PAID_USER_DEFAULT_MODEL_ID,
    object: "model",
    created: Date.now(),
    owned_by: "deepseek",
    tasks: ["generate"]
  };

  const [localState, setLocalState] = useState({
    userPrompt: "",
    systemPrompt: null as string | null,
    userImages: [] as File[],
    model: aliasModelName(import.meta.env.VITE_DEV_MODEL_OVERRIDE) || FREE_USER_DEFAULT_MODEL_ID,
    availableModels: [llamaModel, deepSeekModel] as OpenSecretModel[],
    billingStatus: null as BillingStatus | null,
    searchQuery: "",
    isSearchVisible: false,
    draftMessages: new Map<string, string>()
  });

  // Update default model when billing status changes (only if model hasn't been explicitly set by user)
  useEffect(() => {
    // Only update if we're still using the default model and not overridden by dev env
    if (!import.meta.env.VITE_DEV_MODEL_OVERRIDE) {
      const currentModel = localState.model;
      const isCurrentlyDefault =
        currentModel === FREE_USER_DEFAULT_MODEL_ID || currentModel === PAID_USER_DEFAULT_MODEL_ID;

      if (isCurrentlyDefault) {
        const newDefaultModel = getDefaultModelId(localState.billingStatus);
        if (currentModel !== newDefaultModel) {
          setLocalState((prev) => ({ ...prev, model: newDefaultModel }));
        }
      }
    }
  }, [localState.billingStatus, localState.model]);

  const { get, put, list, del } = useOpenSecret();

  async function persistChat(chat: Chat) {
    const chatToSave = {
      /** If a model is missing, assume the appropriate default model based on billing status */
      model: aliasModelName(chat.model) || getDefaultModelId(localState.billingStatus),
      ...chat
    };

    console.log("Persisting chat:", chatToSave);
    try {
      // Save the chat to storage
      await put(`chat_${chat.id}`, JSON.stringify(chatToSave));

      // Now we need to update the history_list
      const historyList = await fetchOrCreateHistoryList();

      // If the item already exists, update it, otherwise add it
      if (historyList.some((item) => item.id === chat.id)) {
        const updatedHistory = historyList.map((item) => {
          if (item.id === chat.id) {
            return {
              id: chat.id,
              title: chatToSave.title,
              updated_at: Date.now(),
              created_at: item.created_at
            };
          } else {
            return item;
          }
        });
        await put("history_list", JSON.stringify(updatedHistory));
      } else {
        const updatedHistory = [
          {
            id: chat.id,
            title: chat.title,
            updated_at: Date.now(),
            created_at: Date.now()
          },
          ...historyList
        ];
        await put("history_list", JSON.stringify(updatedHistory));
      }
    } catch (error) {
      console.error("Failed to persist chat:", error);
    }
  }

  function setUserPrompt(prompt: string) {
    setLocalState((prev) => ({ ...prev, userPrompt: prompt }));
  }

  function setSystemPrompt(prompt: string | null) {
    setLocalState((prev) => ({ ...prev, systemPrompt: prompt }));
  }

  function setUserImages(images: File[]) {
    setLocalState((prev) => ({ ...prev, userImages: images }));
  }

  function setBillingStatus(status: BillingStatus) {
    setLocalState((prev) => ({ ...prev, billingStatus: status }));
  }

  function setSearchQuery(query: string) {
    setLocalState((prev) => ({ ...prev, searchQuery: query }));
  }

  function setIsSearchVisible(visible: boolean) {
    setLocalState((prev) => ({ ...prev, isSearchVisible: visible }));
  }

  async function addChat(title: string = "New Chat") {
    const newChat = {
      id: window.crypto.randomUUID(),
      title,
      messages: [],
      model: localState.model
    };
    await persistChat(newChat);
    return newChat.id;
  }

  async function getChatById(id: string): Promise<Chat | undefined> {
    try {
      const chat = await get(`chat_${id}`);
      if (!chat) return undefined;
      const parsedChat = JSON.parse(chat) as Chat;
      // Alias the model name for backward compatibility
      if (parsedChat.model) {
        parsedChat.model = aliasModelName(parsedChat.model);
      }
      return parsedChat;
    } catch (error) {
      console.error("Error fetching chat:", error);
      return undefined;
    }
  }

  async function fetchOrCreateHistoryList() {
    let historyList = "[]";
    try {
      const existingHistory = await get("history_list");
      if (existingHistory) {
        historyList = existingHistory;
      }
    } catch (error) {
      console.error("Error fetching history_list:", error);
    }

    // Parse the history_list item
    let parsedHistory: HistoryItem[];
    try {
      parsedHistory = JSON.parse(historyList) as HistoryItem[];
      if (!Array.isArray(parsedHistory)) {
        throw new Error("Parsed history is not an array");
      }
    } catch (error) {
      console.error("Error parsing history_list:", error);
      console.log("Raw history_list content:", historyList);
      parsedHistory = [];
    }

    // TODO REMOVE: this fallback is because we didn't always have a history_list
    if (parsedHistory.length === 0) {
      try {
        const allKeys = await list();
        const chatKeys = allKeys.filter((item) => item.key.startsWith("chat_"));

        const newHistoryList = await Promise.all(
          chatKeys.map(async (item) => {
            const chat = JSON.parse(item.value) as Chat;
            return {
              id: chat.id,
              title: chat.title,
              updated_at: item.updated_at,
              created_at: item.created_at
            };
          })
        );

        // Sort by updated_at timestamp
        newHistoryList.sort((a, b) => b.updated_at - a.updated_at);

        await put("history_list", JSON.stringify(newHistoryList));
        return newHistoryList;
      } catch (error) {
        console.error("Error creating new history list:", error);
        return [];
      }
    }

    return parsedHistory;
  }

  async function clearHistory() {
    const items = await list();
    await del("history_list");
    await Promise.all(
      items.filter((item) => item.key.startsWith("chat_")).map(async (chat) => del(chat.key))
    );
  }

  async function deleteChat(chatId: string) {
    // Delete the chat contents
    await del(`chat_${chatId}`);
    // Update the chat history
    const chatHistory = await fetchOrCreateHistoryList();
    const updatedChatHistory = chatHistory.filter((item) => item.id !== chatId);
    await put("history_list", JSON.stringify(updatedChatHistory));
  }

  async function renameChat(chatId: string, newTitle: string) {
    try {
      // Get the current chat
      const chat = await getChatById(chatId);
      if (!chat) {
        console.error("Chat not found for renaming:", chatId);
        throw new Error("Chat not found");
      }

      // Update the chat title
      chat.title = newTitle;

      // Save the updated chat
      await persistChat(chat);

      // The persistChat function already updates the history list
      return;
    } catch (error) {
      console.error("Error renaming chat:", error);
      throw new Error("Error renaming chat");
    }
  }

  function setDraftMessage(chatId: string, draft: string) {
    if (!chatId?.trim()) {
      console.error("Invalid chatId provided to setDraftMessage");
      return;
    }
    setLocalState((prev) => ({
      ...prev,
      draftMessages: new Map(prev.draftMessages).set(chatId, draft)
    }));
  }

  function clearDraftMessage(chatId: string) {
    if (!chatId?.trim()) {
      console.error("Invalid chatId provided to clearDraftMessage");
      return;
    }
    setLocalState((prev) => {
      const newDrafts = new Map(prev.draftMessages);
      if (!newDrafts.has(chatId)) {
        return prev; // No state update needed if draft doesn't exist
      }
      newDrafts.delete(chatId);
      return { ...prev, draftMessages: newDrafts };
    });
  }

  function setModel(model: string) {
    const aliasedModel = aliasModelName(model);
    setLocalState((prev) =>
      prev.model === aliasedModel ? prev : { ...prev, model: aliasedModel }
    );
  }

  function setAvailableModels(models: OpenSecretModel[]) {
    setLocalState((prev) => ({ ...prev, availableModels: models }));
  }

  return (
    <LocalStateContext.Provider
      value={{
        model: localState.model,
        availableModels: localState.availableModels,
        setModel,
        setAvailableModels,
        userPrompt: localState.userPrompt,
        systemPrompt: localState.systemPrompt,
        userImages: localState.userImages,
        billingStatus: localState.billingStatus,
        searchQuery: localState.searchQuery,
        setSearchQuery,
        isSearchVisible: localState.isSearchVisible,
        setIsSearchVisible,
        setBillingStatus,
        setUserPrompt,
        setSystemPrompt,
        setUserImages,
        addChat,
        getChatById,
        persistChat,
        fetchOrCreateHistoryList,
        clearHistory,
        deleteChat,
        renameChat,
        draftMessages: localState.draftMessages,
        setDraftMessage,
        clearDraftMessage
      }}
    >
      {children}
    </LocalStateContext.Provider>
  );
};
