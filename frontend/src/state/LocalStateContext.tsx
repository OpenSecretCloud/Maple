import { useOpenSecret } from "@opensecret/react";
import { useState } from "react";
import { BillingStatus } from "@/billing/billingApi";
import { LocalStateContext, Chat, HistoryItem, OpenSecretModel } from "./LocalStateContextDef";
import { aliasModelName } from "@/utils/utils";

export {
  LocalStateContext,
  type Chat,
  type HistoryItem,
  type LocalState
} from "./LocalStateContextDef";

export const DEFAULT_MODEL_ID = "llama-3.3-70b";
const QUICK_MODEL_ID = "gpt-oss-120b";

// Helper to get default model based on cached billing status
function getInitialModel(): string {
  // Check for dev override first
  if (import.meta.env.VITE_DEV_MODEL_OVERRIDE) {
    return aliasModelName(import.meta.env.VITE_DEV_MODEL_OVERRIDE);
  }

  try {
    // Priority 1: Check local storage for last used model
    const selectedModel = localStorage.getItem("selectedModel");
    if (selectedModel) {
      return selectedModel;
    }

    // Priority 2: Check cached billing status for pro/max/team users
    const cachedBillingStr = localStorage.getItem("cachedBillingStatus");
    if (cachedBillingStr) {
      const cachedBilling = JSON.parse(cachedBillingStr) as BillingStatus;
      const planName = cachedBilling.product_name?.toLowerCase() || "";

      // Pro, Max, or Team users get Quick model
      if (planName.includes("pro") || planName.includes("max") || planName.includes("team")) {
        return QUICK_MODEL_ID;
      }
    }
  } catch (error) {
    console.error("Failed to load initial model:", error);
  }

  // Priority 3: Default to free model
  return DEFAULT_MODEL_ID;
}

export const LocalStateProvider = ({ children }: { children: React.ReactNode }) => {
  /** The model that should be assumed when a chat doesn't yet have one */
  const llamaModel: OpenSecretModel = {
    id: DEFAULT_MODEL_ID,
    object: "model",
    created: Date.now(),
    owned_by: "meta",
    tasks: ["generate"]
  };

  const [localState, setLocalState] = useState({
    userPrompt: "",
    systemPrompt: null as string | null,
    userImages: [] as File[],
    sentViaVoice: false,
    model: getInitialModel(),
    availableModels: [llamaModel] as OpenSecretModel[],
    hasWhisperModel: true, // Default to true to avoid hiding button during loading
    thinkingEnabled: false, // Default to reasoning without thinking (V3.1)
    billingStatus: null as BillingStatus | null,
    searchQuery: "",
    isSearchVisible: false,
    draftMessages: new Map<string, string>()
  });

  const { get, put, list, del } = useOpenSecret();

  async function persistChat(chat: Chat) {
    const chatToSave = {
      /** If a model is missing, assume the default Llama and write it now */
      model: aliasModelName(chat.model) || DEFAULT_MODEL_ID,
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

  function setSentViaVoice(sentViaVoice: boolean) {
    setLocalState((prev) => ({ ...prev, sentViaVoice }));
  }

  function setBillingStatus(status: BillingStatus) {
    setLocalState((prev) => ({ ...prev, billingStatus: status }));

    const planName = status.product_name?.toLowerCase() || "";
    const isPaidPlan =
      planName.includes("pro") ||
      planName.includes("max") ||
      planName.includes("team") ||
      planName.includes("starter");

    const isProMaxOrTeam =
      planName.includes("pro") || planName.includes("max") || planName.includes("team");

    // Check if billing plan changed from cached version
    let billingChanged = false;
    try {
      const cachedBillingStr = localStorage.getItem("cachedBillingStatus");
      if (cachedBillingStr) {
        const cachedBilling = JSON.parse(cachedBillingStr) as BillingStatus;
        const cachedPlan = cachedBilling.product_name?.toLowerCase() || "";
        billingChanged = cachedPlan !== planName;
      }
    } catch (error) {
      console.error("Failed to check cached billing:", error);
    }

    // Cache billing status to localStorage only for paid users
    try {
      if (isPaidPlan) {
        localStorage.setItem("cachedBillingStatus", JSON.stringify(status));
      } else {
        // Clear cache for free users
        localStorage.removeItem("cachedBillingStatus");
      }
    } catch (error) {
      console.error("Failed to cache billing status:", error);
    }

    // Update model if: 1) no custom selectedModel OR 2) billing plan changed
    try {
      const selectedModel = localStorage.getItem("selectedModel");
      const shouldUpdateModel = !selectedModel || billingChanged;

      if (shouldUpdateModel) {
        if (isProMaxOrTeam) {
          setModel(QUICK_MODEL_ID);
        } else if (billingChanged) {
          // User downgraded, switch back to free model
          setModel(DEFAULT_MODEL_ID);
        }
      }
    } catch (error) {
      console.error("Failed to update model based on billing status:", error);
    }
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
    setLocalState((prev) => {
      if (prev.model === aliasedModel) return prev;

      // Save to localStorage when model changes
      try {
        localStorage.setItem("selectedModel", aliasedModel);
      } catch (error) {
        console.error("Failed to save model to localStorage:", error);
      }

      return { ...prev, model: aliasedModel };
    });
  }

  function setAvailableModels(models: OpenSecretModel[]) {
    setLocalState((prev) => ({ ...prev, availableModels: models }));
  }

  function setHasWhisperModel(hasWhisper: boolean) {
    setLocalState((prev) => ({ ...prev, hasWhisperModel: hasWhisper }));
  }

  function setThinkingEnabled(enabled: boolean) {
    setLocalState((prev) => ({ ...prev, thinkingEnabled: enabled }));
  }

  return (
    <LocalStateContext.Provider
      value={{
        model: localState.model,
        availableModels: localState.availableModels,
        setModel,
        setAvailableModels,
        hasWhisperModel: localState.hasWhisperModel,
        setHasWhisperModel,
        thinkingEnabled: localState.thinkingEnabled,
        setThinkingEnabled,
        userPrompt: localState.userPrompt,
        systemPrompt: localState.systemPrompt,
        userImages: localState.userImages,
        sentViaVoice: localState.sentViaVoice,
        billingStatus: localState.billingStatus,
        searchQuery: localState.searchQuery,
        setSearchQuery,
        isSearchVisible: localState.isSearchVisible,
        setIsSearchVisible,
        setBillingStatus,
        setUserPrompt,
        setSystemPrompt,
        setUserImages,
        setSentViaVoice,
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
